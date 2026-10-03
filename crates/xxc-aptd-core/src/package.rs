use crate::{
    config::{Config, identifier},
    db::Database,
    tools,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256, Sha512};
use std::{collections::BTreeMap, fs, io::Read, path::Path, time::Duration};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Package {
    pub id: String,
    pub name: String,
    pub version: String,
    pub architecture: String,
    pub source: String,
    pub component: String,
    pub description: String,
    pub fields: BTreeMap<String, String>,
    pub filename: String,
    pub size: u64,
    pub sha256: String,
    pub sha512: String,
    pub uploaded: String,
}
pub trait PackageInspector {
    fn inspect(&self, path: &Path, config: &Config) -> Result<Package>;
}
pub struct DpkgInspector;

pub fn package_name(s: &str) -> bool {
    s.len() >= 2
        && s.len() <= 128
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"+.-".contains(&b))
}
pub fn control(text: &str) -> Result<BTreeMap<String, String>> {
    ensure!(
        text.len() <= 1024 * 1024
            && !text
                .chars()
                .any(|x| x.is_control() && x != '\n' && x != '\t'),
        "Malformed control metadata"
    );
    let mut fields = BTreeMap::<String, String>::new();
    let mut previous = String::new();
    for line in text.trim_end_matches('\n').lines() {
        if line.starts_with([' ', '\t']) {
            let value = fields
                .get_mut(&previous)
                .context("Continuation without field")?;
            value.push('\n');
            value.push_str(line);
            continue;
        }
        let (name, value) = line.split_once(':').context("Malformed control field")?;
        ensure!(
            !name.is_empty() && name.bytes().all(|x| x.is_ascii_alphanumeric() || x == b'-'),
            "Invalid field name"
        );
        let name = name.to_ascii_lowercase();
        ensure!(!fields.contains_key(&name), "Duplicate control field");
        previous = name.clone();
        fields.insert(name, value.trim().to_owned());
    }
    Ok(fields)
}
pub fn hash_file(path: &Path) -> Result<(u64, String, String)> {
    let mut file = fs::File::open(path)?;
    let mut b = [0u8; 65536];
    let mut a = Sha256::new();
    let mut c = Sha512::new();
    let mut size = 0;
    loop {
        let n = file.read(&mut b)?;
        if n == 0 {
            break;
        }
        a.update(&b[..n]);
        c.update(&b[..n]);
        size += n as u64;
    }
    Ok((
        size,
        format!("{:x}", a.finalize()),
        format!("{:x}", c.finalize()),
    ))
}
impl PackageInspector for DpkgInspector {
    fn inspect(&self, path: &Path, c: &Config) -> Result<Package> {
        let path = path.canonicalize()?;
        // Reading control metadata alone does not validate data.tar. Validate
        // the payload archive without extracting files or running scripts.
        tools::run(
            "dpkg-deb",
            [std::ffi::OsStr::new("--contents"), path.as_os_str()],
            &c.paths.temporary,
            Duration::from_secs(60),
            16 * 1024 * 1024,
        )?;
        let bytes = tools::run(
            "dpkg-deb",
            [std::ffi::OsStr::new("--field"), path.as_os_str()],
            &c.paths.temporary,
            Duration::from_secs(60),
            1024 * 1024,
        )?;
        let fields = control(std::str::from_utf8(&bytes)?)?;
        let get = |key: &str| {
            fields
                .get(key)
                .cloned()
                .with_context(|| format!("Required control field {key} missing"))
        };
        let name = get("package")?;
        let version = get("version")?;
        let architecture = get("architecture")?;
        let source = fields
            .get("source")
            .and_then(|s| s.split_whitespace().next())
            .unwrap_or(&name)
            .to_owned();
        ensure!(
            package_name(&name) && package_name(&source),
            "Invalid package/source name"
        );
        ensure!(
            identifier(&architecture)
                && (architecture == "all" || c.repository.architectures.contains(&architecture)),
            "Unsupported architecture"
        );
        ensure!(
            !version.is_empty()
                && version.len() <= 128
                && version.as_bytes()[0].is_ascii_digit()
                && version
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".+:~-".contains(&b)),
            "Invalid Debian version"
        );
        tools::run(
            "dpkg",
            ["--validate-version", &version],
            &c.paths.temporary,
            Duration::from_secs(5),
            1024,
        )?;
        let description = get("description")?;
        ensure!(!description.is_empty(), "Description cannot be empty");
        get("maintainer")?;
        for field in [
            "package",
            "version",
            "architecture",
            "source",
            "installed-size",
        ] {
            if let Some(value) = fields.get(field) {
                ensure!(!value.contains('\n'), "Unexpected multiline identity field");
            }
        }
        if let Some(value) = fields.get("installed-size") {
            value.parse::<u64>().context("Invalid Installed-Size")?;
        }
        let (size, sha256, sha512) = hash_file(&path)?;
        let prefix = if source.starts_with("lib") && source.len() >= 4 {
            &source[..4]
        } else {
            &source[..1]
        };
        let component = c.repository.components[0].clone();
        // Debian pool filenames omit the epoch, which belongs in Version only.
        let file_version = version.rsplit(':').next().context("Version missing")?;
        let filename =
            format!("pool/{component}/{prefix}/{source}/{name}_{file_version}_{architecture}.deb");
        Ok(Package {
            id: uuid::Uuid::new_v4().to_string(),
            name,
            version,
            architecture,
            source,
            component,
            description,
            fields,
            filename,
            size,
            sha256,
            sha512,
            uploaded: crate::repository::now(),
        })
    }
}

pub fn ingest(c: &Config, db: &Database, path: &Path) -> Result<Package> {
    let mut p = DpkgInspector.inspect(path, c)?;
    // Establish durable quarantine bytes before the database can reference them.
    let destination = c.paths.uploads.join(format!("{}.deb", p.id));
    fs::hard_link(path, &destination)?;
    fs::File::open(&destination)?.sync_all()?;
    fs::File::open(&c.paths.uploads)?.sync_all()?;
    match db.insert(&p) {
        Ok(id) if id != p.id => {
            fs::remove_file(destination)?;
            p = db.get(&id)?.context("Duplicate record disappeared")?;
        }
        Ok(_) => {}
        Err(e) => {
            let _ = fs::remove_file(destination);
            return Err(e);
        }
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_metadata_injection() {
        assert!(!package_name("../../bad"));
        assert!(!package_name("$(touch bad)"));
        assert!(package_name("libgood++"));
        assert!(control("Package: aa\npackage: bb\n").is_err());
        assert!(control(" continuation\n").is_err());
        assert!(control("Package: aa\n\nVersion: 1\n").is_err());
        assert_eq!(
            control("Description: first\n second\n .\n third\n").unwrap()["description"],
            "first\n second\n .\n third"
        );
    }
}
