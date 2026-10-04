use crate::{
    config::Config,
    db::Database,
    package::{Package, hash_file},
    signing::{PublicKey, Signer},
    tools,
};
use anyhow::{Context, Result, ensure};
use chrono::{Duration as ChronoDuration, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    os::unix::fs::symlink,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub id: String,
    pub created: String,
    pub suite: String,
    pub fingerprint: String,
    pub packages: Vec<Package>,
    #[serde(default)]
    pub suites: BTreeMap<String, Vec<Package>>,
    pub files: BTreeMap<String, String>,
}
impl Manifest {
    pub fn suite_packages(&self, suite: &str) -> &[Package] {
        if self.suites.is_empty() && suite == self.suite {
            &self.packages
        } else {
            self.suites.get(suite).map(Vec::as_slice).unwrap_or(&[])
        }
    }
    pub fn suite_names(&self) -> Vec<String> {
        if self.suites.is_empty() {
            vec![self.suite.clone()]
        } else {
            self.suites.keys().cloned().collect()
        }
    }
}
pub fn now() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}
pub fn current(c: &Config) -> Result<Option<Manifest>> {
    current_id(c)?.map(|id| load(c, &id)).transpose()
}
pub fn current_id(c: &Config) -> Result<Option<String>> {
    let link = c.paths.repository.join("dists");
    let target = match fs::read_link(link) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    let parts = target
        .components()
        .map(|p| p.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    ensure!(
        parts.len() == 3 && parts[0] == ".generations" && parts[2] == "dists",
        "Malformed current generation pointer"
    );
    uuid::Uuid::parse_str(&parts[1]).context("Invalid active generation ID")?;
    Ok(Some(parts[1].clone()))
}
pub fn load(c: &Config, id: &str) -> Result<Manifest> {
    uuid::Uuid::parse_str(id).context("Invalid generation ID")?;
    let path = c.paths.repository.join(".generations").join(id);
    ensure!(
        path.canonicalize()?
            .starts_with(c.paths.repository.join(".generations")),
        "Generation escapes archive"
    );
    let m: Manifest = serde_json::from_slice(&fs::read(path.join("manifest.json"))?)?;
    ensure!(
        m.id == id && m.suite == c.repository.suite,
        "Generation manifest identity mismatch"
    );
    Ok(m)
}
pub trait MetadataGenerator {
    fn packages(&self, root: &Path) -> Result<Vec<u8>>;
    fn release(&self, root: &Path) -> Result<Vec<u8>>;
}
pub struct AptFtparchive;
impl MetadataGenerator for AptFtparchive {
    fn packages(&self, root: &Path) -> Result<Vec<u8>> {
        tools::run(
            "apt-ftparchive",
            ["packages", "pool"],
            root,
            Duration::from_secs(600),
            128 * 1024 * 1024,
        )
    }
    fn release(&self, root: &Path) -> Result<Vec<u8>> {
        tools::run(
            "apt-ftparchive",
            [
                "-o",
                "APT::FTPArchive::Release::MD5=false",
                "-o",
                "APT::FTPArchive::Release::SHA1=false",
                "release",
                ".",
            ],
            root,
            Duration::from_secs(300),
            32 * 1024 * 1024,
        )
    }
}
fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn sync_tree(path: &Path) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            sync_tree(&entry.path())?;
        } else {
            fs::File::open(entry.path())?.sync_all()?;
        }
    }
    fs::File::open(path)?.sync_all()?;
    Ok(())
}
fn collect_hashes(root: &Path, path: &Path, out: &mut BTreeMap<String, String>) -> Result<()> {
    for entry in fs::read_dir(path)? {
        let e = entry?;
        if e.file_type()?.is_dir() {
            collect_hashes(root, &e.path(), out)?;
        } else {
            let relative = e.path().strip_prefix(root)?.to_string_lossy().into_owned();
            out.insert(relative, hash_file(&e.path())?.1);
        }
    }
    Ok(())
}
pub fn publish(c: &Config, db: &Database, signer: &dyn Signer) -> Result<Manifest> {
    c.check_directories()?;
    ensure!(
        !c.signing.fingerprint.is_empty(),
        "No signing fingerprint configured"
    );
    let previous = current(c)?;
    if let Some(m) = &previous {
        ensure!(
            m.suite_names().iter().all(|s| c.repository.has_suite(s)),
            "Removing a published suite requires an explicit migration"
        );
    }
    let mut suites = BTreeMap::new();
    for name in c.repository.suite_names() {
        let packages = if name == db.suite() {
            db.selected()?
        } else {
            previous
                .as_ref()
                .map(|m| m.suite_packages(&name).to_vec())
                .unwrap_or_default()
        };
        suites.insert(name, packages);
    }
    let packages: Vec<Package> = suites
        .values()
        .flatten()
        .map(|p| (p.id.clone(), p.clone()))
        .collect::<BTreeMap<_, _>>()
        .into_values()
        .collect();
    let id = uuid::Uuid::new_v4().to_string();
    let generations = c.paths.repository.join(".generations");
    let scratch = tempfile::Builder::new()
        .prefix(".building-")
        .tempdir_in(&generations)?;
    let root = scratch.path();
    fs::create_dir(root.join("pool"))?;
    for p in &packages {
        let pool = c.paths.repository.join(&p.filename);
        if !pool.exists() {
            let upload = c.paths.uploads.join(format!("{}.deb", p.id));
            ensure!(
                hash_file(&upload)? == (p.size, p.sha256.clone(), p.sha512.clone()),
                "Quarantine package integrity failure"
            );
            let parent = pool.parent().context("Pool parent missing")?;
            fs::create_dir_all(parent)?;
            let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
            std::io::copy(&mut fs::File::open(upload)?, &mut tmp)?;
            tmp.as_file().sync_all()?;
            // Never overwrite an existing pool object, including on a race.
            tmp.persist_noclobber(&pool)?;
            fs::File::open(parent)?.sync_all()?;
        }
        ensure!(
            hash_file(&pool)? == (p.size, p.sha256.clone(), p.sha512.clone()),
            "Immutable pool object conflicts with package identity"
        );
        let target = root.join(&p.filename);
        fs::create_dir_all(target.parent().context("Pool parent")?)?;
        fs::hard_link(&pool, target)?;
    }
    let generated = AptFtparchive.packages(root)?;
    let text = std::str::from_utf8(&generated)?;
    let mut stanzas = Vec::new();
    for stanza in text.split("\n\n").filter(|s| !s.trim().is_empty()) {
        let fields = crate::package::control(stanza)?;
        let filename = fields
            .get("filename")
            .context("apt-ftparchive omitted Filename")?;
        let p = packages
            .iter()
            .find(|p| p.filename == *filename)
            .context("Unexpected package from metadata generator")?;
        ensure!(
            fields.get("sha256") == Some(&p.sha256),
            "Generated package digest mismatch"
        );
        stanzas.push((p, format!("{}\n\n", stanza.trim_end())));
    }
    ensure!(
        stanzas.len() == packages.len(),
        "Metadata generator omitted packages"
    );
    let r = &c.repository;
    for (suite_name, members) in &suites {
        let suite = root.join("dists").join(suite_name);
        fs::create_dir_all(&suite)?;
        for component in &c.repository.components {
            for architecture in c
                .repository
                .architectures
                .iter()
                .map(String::as_str)
                .chain(std::iter::once("all"))
            {
                let index = suite.join(component).join(format!("binary-{architecture}"));
                let bytes = stanzas
                    .iter()
                    .filter(|(p, _)| {
                        members.iter().any(|m| m.id == p.id)
                            && p.component == *component
                            && (p.architecture == architecture || p.architecture == "all")
                    })
                    .map(|(_, s)| s.as_str())
                    .collect::<String>()
                    .into_bytes();
                let mut gz =
                    flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                gz.write_all(&bytes)?;
                let mut xz = xz2::write::XzEncoder::new(Vec::new(), 6);
                xz.write_all(&bytes)?;
                for (name, data) in [
                    ("Packages", bytes),
                    ("Packages.gz", gz.finish()?),
                    ("Packages.xz", xz.finish()?),
                ] {
                    let path = index.join(name);
                    write(&path, &data)?;
                    let (_, sha256, sha512) = hash_file(&path)?;
                    for (algorithm, hash) in [("SHA256", sha256), ("SHA512", sha512)] {
                        let by_hash = index.join("by-hash").join(algorithm);
                        fs::create_dir_all(&by_hash)?;
                        let object = by_hash.join(hash);
                        if !object.exists() {
                            fs::hard_link(&path, object)?;
                        }
                    }
                }
            }
        }
        let r = &c.repository;
        let valid = (Utc::now() + ChronoDuration::seconds(r.valid_until_seconds as i64))
            .format("%a, %d %b %Y %H:%M:%S UTC");
        let header = format!(
            "Origin: {}\nLabel: {}\nSuite: {}\nCodename: {}\nVersion: {}\nArchitectures: {} all\nComponents: {}\nDescription: {}\nAcquire-By-Hash: yes\nValid-Until: {}\nNotAutomatic: {}\nButAutomaticUpgrades: {}\n",
            r.origin,
            r.label,
            suite_name,
            if suite_name == &r.suite {
                &r.codename
            } else {
                suite_name
            },
            r.version,
            r.architectures.join(" "),
            r.components.join(" "),
            r.description,
            valid,
            if r.not_automatic { "yes" } else { "no" },
            if r.but_automatic_upgrades {
                "yes"
            } else {
                "no"
            }
        );
        let mut release = header.into_bytes();
        release.extend(AptFtparchive.release(&suite)?);
        write(&suite.join("Release"), &release)?;
        signer.sign(&suite.join("Release"))?;
    }
    write(&root.join("public.asc"), &signer.export(true)?)?;
    write(&root.join("public.gpg"), &signer.export(false)?)?;
    fs::remove_dir_all(root.join("pool"))?;
    let mut files = BTreeMap::new();
    collect_hashes(root, root, &mut files)?;
    let manifest = Manifest {
        id: id.clone(),
        created: now(),
        suite: r.suite.clone(),
        fingerprint: c.signing.fingerprint.clone(),
        packages,
        suites,
        files,
    };
    write(
        &root.join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    sync_tree(root)?;
    let final_path = generations.join(&id);
    fs::rename(root, &final_path)?;
    fs::File::open(&generations)?.sync_all()?;
    verify(c, &manifest)?;
    activate(c, db, &manifest)?;
    Ok(manifest)
}
pub fn verify(c: &Config, m: &Manifest) -> Result<()> {
    let root = c.paths.repository.join(".generations").join(&m.id);
    for (file, hash) in &m.files {
        let path = Path::new(file);
        ensure!(
            !path.is_absolute()
                && path
                    .components()
                    .all(|x| matches!(x, std::path::Component::Normal(_))),
            "Invalid manifest file path"
        );
        ensure!(
            hash_file(&root.join(path))?.1 == *hash,
            "Generation file checksum mismatch"
        );
    }
    for p in &m.packages {
        ensure!(
            p.filename.starts_with("pool/")
                && !p.filename.contains("..")
                && !p.filename.contains('\\'),
            "Invalid manifest pool path"
        );
        ensure!(
            hash_file(&c.paths.repository.join(&p.filename))?
                == (p.size, p.sha256.clone(), p.sha512.clone()),
            "Package integrity failure"
        );
    }
    // Every suite must have signed metadata. Retained keys verify offline.
    ensure!(
        fs::metadata(root.join("public.gpg"))?.len() <= 1024 * 1024,
        "Retained public signing key exceeds 1 MiB"
    );
    let bytes = zeroize::Zeroizing::new(fs::read(root.join("public.gpg"))?);
    let key = PublicKey::new(c, &m.fingerprint, bytes)?;
    let mut union = BTreeMap::new();
    for name in m.suite_names() {
        ensure!(crate::config::identifier(&name), "Invalid manifest suite");
        for p in m.suite_packages(&name) {
            union.insert(p.id.clone(), p);
        }
        for file in [
            "public.asc".to_owned(),
            "public.gpg".to_owned(),
            format!("dists/{name}/Release"),
            format!("dists/{name}/InRelease"),
            format!("dists/{name}/Release.gpg"),
        ] {
            ensure!(
                m.files.contains_key(&file),
                "Manifest omits a required signed generation file"
            );
        }
        key.verify(&root.join("dists").join(name).join("Release"))?;
    }
    ensure!(
        union.len() == m.packages.len()
            && m.packages.iter().all(|p| union
                .get(&p.id)
                .is_some_and(|q| q.sha256 == p.sha256 && q.filename == p.filename)),
        "Manifest suite membership mismatch"
    );
    Ok(())
}
pub fn reconcile(c: &Config, db: &Database) -> Result<()> {
    let mut conn = db.connect()?;
    let tx = conn.transaction()?;
    tx.execute("UPDATE packages SET active=0", [])?;
    tx.execute("UPDATE package_suites SET active=0", [])?;
    if let Some(m) = current(c)? {
        for p in &m.packages {
            tx.execute("INSERT INTO packages(id,name,version,architecture,sha256,state,active,metadata) VALUES(?1,?2,?3,?4,?5,'published',1,?6) ON CONFLICT(id) DO UPDATE SET state='published',active=1",params![p.id,p.name,p.version,p.architecture,p.sha256,serde_json::to_string(p)?])?;
            tx.execute("DELETE FROM package_search WHERE id=?1", [&p.id])?;
            tx.execute(
                "INSERT INTO package_search(id,name,description) VALUES(?1,?2,?3)",
                params![p.id, p.name, p.description],
            )?;
        }
        for suite in m.suite_names() {
            for p in m.suite_packages(&suite) {
                tx.execute("INSERT INTO package_suites(suite,package_id,state,active) VALUES(?1,?2,'published',1) ON CONFLICT(suite,package_id) DO UPDATE SET state='published',active=1",params![suite,p.id])?;
            }
        }
        tx.execute(
            "INSERT OR IGNORE INTO generations(id,created,manifest) VALUES(?1,?2,?3)",
            params![m.id, m.created, serde_json::to_string(&m)?],
        )?;
    }
    tx.commit()?;
    Ok(())
}
pub fn activate(c: &Config, db: &Database, m: &Manifest) -> Result<()> {
    let root = &c.paths.repository;
    let temporary = root.join(format!(".dists-{}", uuid::Uuid::new_v4()));
    symlink(format!(".generations/{}/dists", m.id), &temporary)?;
    if let Err(e) = fs::rename(&temporary, root.join("dists")) {
        let _ = fs::remove_file(temporary);
        return Err(e.into());
    }
    fs::File::open(root)?.sync_all()?;
    reconcile(c, db)
}
pub fn rollback(c: &Config, db: &Database, id: &str) -> Result<Manifest> {
    let m = load(c, id)?;
    verify(c, &m)?;
    activate(c, db, &m)?;
    Ok(m)
}
pub fn generations(c: &Config) -> Result<Vec<Manifest>> {
    let mut out = Vec::new();
    for e in fs::read_dir(c.paths.repository.join(".generations"))? {
        let name = e?.file_name().to_string_lossy().into_owned();
        if uuid::Uuid::parse_str(&name).is_ok() {
            out.push(load(c, &name)?);
        }
    }
    out.sort_by(|a, b| b.created.cmp(&a.created));
    Ok(out)
}
/// Resolve public paths against one generation. Only the selected namespaces
/// are reachable; .generations itself is never a public path.
pub fn resolve(c: &Config, relative: &str) -> Result<PathBuf> {
    let pieces = relative
        .split('/')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    ensure!(
        !relative.starts_with('/')
            && !relative.contains('\\')
            && !relative.contains('%')
            && pieces
                .iter()
                .all(|s| !s.starts_with('.') && !s.chars().any(char::is_control)),
        "Forbidden repository path"
    );
    if pieces.is_empty() {
        return Ok(c.paths.repository.clone());
    }
    let namespace = pieces[0];
    ensure!(
        namespace == "pool" || namespace == "dists",
        "Unknown repository namespace"
    );
    let (base, path) = if namespace == "dists" {
        let id = current_id(c)?.context("Repository not published")?;
        let base = c
            .paths
            .repository
            .join(".generations")
            .join(id)
            .join("dists");
        let rest = pieces[1..].join("/");
        let candidate = base.join(&rest);
        if !candidate.exists()
            && pieces.len() >= 7
            && pieces[pieces.len() - 3] == "by-hash"
            && matches!(pieces[pieces.len() - 2], "SHA256" | "SHA512")
            && pieces.last().is_some_and(|s| {
                s.len()
                    == if pieces[pieces.len() - 2] == "SHA256" {
                        64
                    } else {
                        128
                    }
                    && s.bytes().all(|b| b.is_ascii_hexdigit())
            })
        {
            for old in generations(c)? {
                let oldbase = c
                    .paths
                    .repository
                    .join(".generations")
                    .join(old.id)
                    .join("dists");
                let oldpath = oldbase.join(&rest);
                if oldpath.is_file() {
                    let real = oldpath.canonicalize()?;
                    ensure!(real.starts_with(&oldbase), "Symlink escape");
                    return Ok(real);
                }
            }
        }
        (base, candidate)
    } else {
        (
            c.paths.repository.join("pool"),
            c.paths.repository.join(pieces.join("/")),
        )
    };
    let canonical = path.canonicalize()?;
    ensure!(canonical.starts_with(base), "Symlink escape");
    Ok(canonical)
}
