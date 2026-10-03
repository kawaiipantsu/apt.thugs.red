use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    net::SocketAddr,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub server: Server,
    pub paths: Paths,
    pub repository: Repository,
    pub signing: Signing,
    pub web: Web,
    pub logging: Logging,
    #[serde(default)]
    pub admin: Admin,
    #[serde(default)]
    pub xxc_trust: XxcTrust,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {
    pub public_listen: SocketAddr,
    pub admin_listen: SocketAddr,
    pub admin_socket: PathBuf,
    pub external_url: String,
    pub repository_prefix: String,
    pub max_upload_bytes: u64,
    pub upload_timeout_seconds: u64,
    pub max_concurrent_uploads: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Paths {
    pub repository: PathBuf,
    pub staging: PathBuf,
    pub uploads: PathBuf,
    pub temporary: PathBuf,
    pub database: PathBuf,
    pub keys: PathBuf,
    pub runtime: PathBuf,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    pub origin: String,
    pub label: String,
    pub suite: String,
    pub codename: String,
    pub components: Vec<String>,
    pub architectures: Vec<String>,
    pub description: String,
    pub version: String,
    pub not_automatic: bool,
    pub but_automatic_upgrades: bool,
    pub valid_until_seconds: u64,
    pub acquire_by_hash: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Signing {
    pub backend: String,
    #[serde(default)]
    pub remote_key_id: String,
    pub fingerprint: String,
    pub public_ascii_name: String,
    pub public_keyring_name: String,
    pub command_timeout_seconds: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Web {
    pub site_name: String,
    pub tagline: String,
    pub main_site_url: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Logging {
    pub level: String,
    pub file: PathBuf,
    pub audit_file: PathBuf,
    pub strict: bool,
}

/// Authentication and CSRF are mandatory whenever this listener is enabled.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Admin {
    pub enabled: bool,
    pub allow_remote: bool,
    pub external_url: String,
    pub session_lifetime_seconds: u64,
    pub login_window_seconds: u64,
    pub login_max_attempts: u32,
    pub max_sessions_per_user: u32,
}
impl Default for Admin {
    fn default() -> Self {
        Self {
            enabled: true,
            allow_remote: false,
            external_url: "https://admin.apt.thugs.red".into(),
            session_lifetime_seconds: 43200,
            login_window_seconds: 900,
            login_max_attempts: 10,
            max_sessions_per_user: 8,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct XxcTrust {
    pub enabled: bool,
    pub api_url: String,
    pub token_credential: String,
    pub authority_id: String,
    pub template_id: String,
    pub ca_certificate: Option<PathBuf>,
    pub request_timeout_seconds: u64,
    pub max_response_bytes: usize,
}
impl Default for XxcTrust {
    fn default() -> Self {
        Self {
            enabled: false,
            api_url: "https://ca.example.invalid/api/v1".into(),
            token_credential: "xxc-trust-token".into(),
            authority_id: String::new(),
            template_id: String::new(),
            ca_certificate: None,
            request_timeout_seconds: 10,
            max_response_bytes: 1048576,
        }
    }
}
impl XxcTrust {
    pub fn validate(&self) -> Result<()> {
        let url = url::Url::parse(&self.api_url).context("Invalid xxc_trust.api_url")?;
        ensure!(
            url.scheme() == "https"
                && concrete_origin(&url)
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && !self.api_url.ends_with('/')
                && url.path() == "/api/v1",
            "xxc_trust.api_url must be an HTTPS /api/v1 URL without credentials, query, fragment or trailing slash"
        );
        ensure!(
            !self.token_credential.is_empty()
                && self.token_credential.len() <= 128
                && !self.token_credential.starts_with('.')
                && self
                    .token_credential
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b)),
            "xxc_trust.token_credential must be a systemd credential name, not a path or token"
        );
        for id in [&self.authority_id, &self.template_id] {
            ensure!(
                id.is_empty()
                    || (id.len() == 32
                        && id
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))),
                "XXC Trust authority/template IDs must be empty or 32 lowercase hexadecimal characters"
            );
        }
        ensure!(
            (1..=60).contains(&self.request_timeout_seconds),
            "XXC Trust request timeout must be 1..60 seconds"
        );
        ensure!(
            (1024..=16 * 1024 * 1024).contains(&self.max_response_bytes),
            "XXC Trust response limit must be 1 KiB..16 MiB"
        );
        if let Some(path) = &self.ca_certificate {
            ensure!(
                path.is_absolute()
                    && !path
                        .components()
                        .any(|p| matches!(p, std::path::Component::ParentDir)),
                "XXC Trust CA certificate path must be absolute without parent components"
            );
        }
        Ok(())
    }
}

pub fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn concrete_origin(url: &url::Url) -> bool {
    match url.host() {
        Some(url::Host::Ipv4(ip)) => !ip.is_unspecified(),
        Some(url::Host::Ipv6(ip)) => !ip.is_unspecified(),
        Some(url::Host::Domain(_)) => true,
        None => false,
    }
}
fn plain(s: &str) -> bool {
    !s.chars().any(char::is_control)
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path).with_context(|| {
            format!(
                "Cannot read {}. Initialize with xxc-aptd init --system, or select --config.",
                path.display()
            )
        })?;
        let c: Self =
            toml::from_str(&text).context("Invalid aptd.conf (unknown fields are rejected)")?;
        c.validate()?;
        Ok(c)
    }
    pub fn validate(&self) -> Result<()> {
        self.xxc_trust.validate()?;
        ensure!(
            self.server.admin_listen.ip().is_loopback() || self.admin.allow_remote,
            "Non-loopback admin_listen requires admin.allow_remote = true; authentication and CSRF remain required"
        );
        ensure!(
            self.server.public_listen != self.server.admin_listen,
            "Listeners must differ"
        );
        let admin_url = url::Url::parse(&self.admin.external_url)?;
        ensure!(
            concrete_origin(&admin_url),
            "admin.external_url must contain the client-facing address, not an unspecified listen address"
        );
        ensure!(
            matches!(admin_url.scheme(), "http" | "https")
                && admin_url.host_str().is_some()
                && self.admin.external_url == admin_url.origin().ascii_serialization()
                && admin_url.username().is_empty()
                && admin_url.password().is_none()
                && admin_url.path() == "/"
                && admin_url.query().is_none()
                && admin_url.fragment().is_none()
                && !self.admin.external_url.ends_with('/')
                && self
                    .admin
                    .external_url
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b":/.-[]".contains(&b)),
            "admin.external_url must be an ASCII HTTP(S) origin without credentials or a path"
        );
        ensure!(
            (300..=604800).contains(&self.admin.session_lifetime_seconds),
            "session_lifetime_seconds must be 300..604800"
        );
        ensure!(
            (60..=86400).contains(&self.admin.login_window_seconds),
            "login_window_seconds must be 60..86400"
        );
        ensure!(
            (1..=100).contains(&self.admin.login_max_attempts),
            "login_max_attempts must be 1..100"
        );
        ensure!(
            (1..=32).contains(&self.admin.max_sessions_per_user),
            "max_sessions_per_user must be 1..32"
        );
        let url = url::Url::parse(&self.server.external_url)?;
        ensure!(
            concrete_origin(&url),
            "server.external_url must contain the client-facing address, not an unspecified listen address"
        );
        ensure!(
            matches!(url.scheme(), "http" | "https")
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.path() == "/",
            "external_url must be an HTTP(S) origin without credentials, path, query or fragment"
        );
        ensure!(
            !self.server.external_url.ends_with('/'),
            "external_url must not end in /"
        );
        ensure!(
            self.server
                .external_url
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b":/.-[]".contains(&b)),
            "external_url must use an ASCII DNS name or IP literal and contain no shell metacharacters"
        );
        ensure!(
            self.server.repository_prefix == "/repo",
            "This release preserves repository_prefix = /repo"
        );
        ensure!(
            self.server.max_upload_bytes > 0
                && self.server.max_upload_bytes <= 16 * 1024_u64.pow(3),
            "max_upload_bytes must be between 1 and 16 GiB"
        );
        ensure!(
            (1..=32).contains(&self.server.max_concurrent_uploads),
            "max_concurrent_uploads must be 1..32"
        );
        ensure!(
            (1..=3600).contains(&self.server.upload_timeout_seconds),
            "upload_timeout_seconds must be 1..3600"
        );
        let r = &self.repository;
        ensure!(
            identifier(&r.suite) && identifier(&r.codename),
            "Invalid suite/codename"
        );
        ensure!(
            !r.components.is_empty() && !r.architectures.is_empty(),
            "components and architectures cannot be empty"
        );
        for list in [&r.components, &r.architectures] {
            let mut seen = std::collections::HashSet::new();
            for item in list {
                ensure!(
                    identifier(item) && seen.insert(item),
                    "Invalid or duplicate component/architecture"
                );
            }
        }
        ensure!(
            !r.architectures.iter().any(|x| x == "all" || x == "source"),
            "List machine architectures only; all is handled automatically"
        );
        for value in [
            &r.origin,
            &r.label,
            &r.description,
            &r.version,
            &self.web.site_name,
            &self.web.tagline,
        ] {
            ensure!(
                plain(value) && value.len() <= 1024,
                "Invalid text configuration value"
            );
        }
        ensure!(
            r.acquire_by_hash,
            "Acquire-By-Hash is required in this release"
        );
        ensure!(
            (3600..=365 * 86400).contains(&r.valid_until_seconds),
            "valid_until_seconds must be 1 hour..365 days"
        );
        ensure!(
            matches!(self.signing.backend.as_str(), "gpg" | "xxc-trust"),
            "Signing backend must be gpg or xxc-trust"
        );
        if self.signing.backend == "xxc-trust" {
            ensure!(
                self.xxc_trust.enabled,
                "xxc-trust signing requires xxc_trust.enabled"
            );
            ensure!(
                self.signing.remote_key_id.len() == 32
                    && self
                        .signing
                        .remote_key_id
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
                "Select signing.remote_key_id as a 32-character lowercase hexadecimal ID"
            );
            ensure!(
                !self.signing.fingerprint.is_empty(),
                "Remote signing requires an explicitly pinned fingerprint"
            );
        } else {
            ensure!(
                self.signing.remote_key_id.is_empty(),
                "remote_key_id is only valid for xxc-trust signing"
            );
        }
        let f = &self.signing.fingerprint;
        ensure!(
            f.is_empty()
                || (matches!(f.len(), 40 | 64) && f.bytes().all(|x| x.is_ascii_hexdigit())),
            "Select a full signing fingerprint"
        );
        ensure!(
            (1..=1800).contains(&self.signing.command_timeout_seconds),
            "Signing timeout must be 1..1800 seconds"
        );
        for name in [
            &self.signing.public_ascii_name,
            &self.signing.public_keyring_name,
        ] {
            ensure!(
                !name.starts_with('.')
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
                    && !name.is_empty(),
                "Invalid public key filename"
            );
        }
        ensure!(
            self.signing.public_ascii_name != self.signing.public_keyring_name,
            "Public key filenames must differ"
        );
        let main = url::Url::parse(&self.web.main_site_url)?;
        ensure!(
            matches!(main.scheme(), "http" | "https")
                && main.username().is_empty()
                && main.password().is_none(),
            "Invalid main_site_url"
        );
        let paths = [
            &self.paths.repository,
            &self.paths.staging,
            &self.paths.uploads,
            &self.paths.temporary,
            &self.paths.database,
            &self.paths.keys,
            &self.paths.runtime,
            &self.server.admin_socket,
            &self.logging.file,
            &self.logging.audit_file,
        ];
        for p in paths {
            ensure!(
                p.is_absolute()
                    && !p.components().any(|c| matches!(
                        c,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )),
                "All paths must be absolute and normalized"
            );
            ensure!(p != Path::new("/"), "Root path is forbidden");
        }
        let roots = [
            &self.paths.repository,
            &self.paths.staging,
            &self.paths.uploads,
            &self.paths.temporary,
            &self.paths.keys,
            &self.paths.runtime,
        ];
        for (i, a) in roots.iter().enumerate() {
            for b in roots.iter().skip(i + 1) {
                ensure!(
                    !a.starts_with(b) && !b.starts_with(a),
                    "State/key directories must not overlap"
                );
            }
        }
        ensure!(
            self.server.admin_socket.parent() == Some(self.paths.runtime.as_path()),
            "admin_socket must be directly inside runtime"
        );
        for p in [
            &self.paths.database,
            &self.logging.file,
            &self.logging.audit_file,
        ] {
            ensure!(
                !p.starts_with(&self.paths.repository),
                "Private files must not be inside repository"
            );
        }
        let state = self
            .paths
            .database
            .parent()
            .context("database parent missing")?;
        for p in [
            &self.paths.staging,
            &self.paths.uploads,
            &self.paths.temporary,
        ] {
            ensure!(
                p.starts_with(state),
                "staging/uploads/temporary must remain inside the database state tree"
            );
        }
        Ok(())
    }
    pub fn check_directories(&self) -> Result<()> {
        for p in [
            &self.paths.repository,
            &self.paths.staging,
            &self.paths.uploads,
            &self.paths.temporary,
            &self.paths.keys,
            &self.paths.runtime,
        ] {
            let canonical = p
                .canonicalize()
                .with_context(|| format!("Missing directory {}; run init", p.display()))?;
            ensure!(
                canonical == *p && p.is_dir(),
                "State directories must be real directories without symlink ancestors"
            );
        }
        let mode = fs::metadata(&self.paths.keys)?.permissions().mode() & 0o777;
        ensure!(mode == 0o700, "Signing key directory must have mode 0700");
        let private = self.paths.keys.join("private-keys-v1.d");
        if private.exists() {
            ensure!(
                private.canonicalize()? == private
                    && fs::metadata(&private)?.permissions().mode() & 0o077 == 0,
                "Private key directory must be a real mode 0700 directory"
            );
            for entry in fs::read_dir(private)? {
                let entry = entry?;
                let metadata = fs::symlink_metadata(entry.path())?;
                ensure!(
                    metadata.is_file()
                        && !metadata.file_type().is_symlink()
                        && metadata.permissions().mode() & 0o077 == 0,
                    "Private key files must be regular mode 0600 files"
                );
            }
        }
        for p in [
            &self.paths.uploads,
            &self.paths.staging,
            &self.paths.temporary,
        ] {
            ensure!(
                fs::metadata(p)?.permissions().mode() & 0o077 == 0,
                "Private state directories require mode 0700"
            );
        }
        Ok(())
    }
    pub fn initialize(path: &Path, root: Option<&Path>) -> Result<Self> {
        let mut c: Self = toml::from_str(include_str!("../../../config/aptd.conf.example"))?;
        if let Some(root) = root {
            fs::create_dir_all(root)?;
            let root = root.canonicalize()?;
            c.paths.repository = root.join("state/repository");
            c.paths.staging = root.join("state/staging");
            c.paths.uploads = root.join("state/uploads");
            c.paths.temporary = root.join("state/tmp");
            c.paths.database = root.join("state/state.db");
            c.paths.keys = root.join("keys");
            c.paths.runtime = root.join("run");
            c.server.admin_socket = root.join("run/admin.sock");
            c.logging.file = root.join("aptd.log");
            c.logging.audit_file = root.join("audit.log");
        }
        if path.exists() {
            c = Self::load(path)?;
        }
        c.validate()?;
        let state = c
            .paths
            .database
            .parent()
            .context("Database parent missing")?;
        if !state.exists() {
            fs::create_dir_all(state)?;
            fs::set_permissions(state, fs::Permissions::from_mode(0o700))?;
        }
        for (p, mode) in [
            (&c.paths.repository, 0o755),
            (&c.paths.staging, 0o700),
            (&c.paths.uploads, 0o700),
            (&c.paths.temporary, 0o700),
            (&c.paths.keys, 0o700),
            (&c.paths.runtime, 0o750),
        ] {
            if !p.exists() {
                fs::create_dir_all(p)?;
                fs::set_permissions(p, fs::Permissions::from_mode(mode))?;
            }
        }
        for p in [
            c.paths.repository.join("pool"),
            c.paths.repository.join(".generations"),
        ] {
            fs::create_dir_all(p)?;
        }
        c.check_directories()?;
        if let Some(p) = path.parent() {
            fs::create_dir_all(p)?;
        }
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o640)
            .open(path)
        {
            Ok(mut f) => {
                f.write_all(toml::to_string_pretty(&c)?.as_bytes())?;
                f.sync_all()?;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => bail!(e),
        }
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_signing_requires_enabled_trust_and_explicit_pin() {
        let mut c: Config =
            toml::from_str(include_str!("../../../config/aptd.conf.example")).unwrap();
        c.signing.backend = "xxc-trust".into();
        assert!(c.validate().is_err());
        c.xxc_trust.enabled = true;
        assert!(c.validate().is_err());
        c.signing.remote_key_id = "1".repeat(32);
        assert!(c.validate().is_err());
        c.signing.fingerprint = "A".repeat(40);
        assert!(c.validate().is_ok());
        c.signing.remote_key_id = "../key".into();
        assert!(c.validate().is_err());
        c.signing.backend = "gpg".into();
        assert!(c.validate().is_err());
    }
    #[test]
    fn example_is_valid_and_strict() {
        let text = include_str!("../../../config/aptd.conf.example");
        let c: Config = toml::from_str(text).unwrap();
        c.validate().unwrap();
        assert!(toml::from_str::<Config>(&format!("{text}\nunknown = true")).is_err());
        let mut bad = c.clone();
        bad.server.admin_listen = "0.0.0.0:8089".parse().unwrap();
        assert!(bad.validate().is_err());
        bad = c.clone();
        bad.paths.uploads = bad.paths.repository.join("uploads");
        assert!(bad.validate().is_err());
        bad = c;
        bad.repository.suite = "../escape".into();
        assert!(bad.validate().is_err());
    }
    #[test]
    fn remote_listeners_require_explicit_administration_opt_in() {
        let mut c: Config =
            toml::from_str(include_str!("../../../config/aptd.conf.example")).unwrap();
        assert!(!c.admin.allow_remote);
        c.server.public_listen = "0.0.0.0:8088".parse().unwrap();
        c.validate().unwrap();
        for bind in ["0.0.0.0:8089", "[::]:8089", "192.0.2.1:8089"] {
            c.server.admin_listen = bind.parse().unwrap();
            assert!(
                c.validate()
                    .unwrap_err()
                    .to_string()
                    .contains("admin.allow_remote")
            );
            c.admin.allow_remote = true;
            c.validate().unwrap();
            c.admin.allow_remote = false;
        }
        c.admin.allow_remote = true;
        for value in ["http://0.0.0.0:8088", "http://[::]:8088"] {
            let mut bad = c.clone();
            bad.server.external_url = value.into();
            assert!(bad.validate().is_err());
            bad = c.clone();
            bad.admin.external_url = value.into();
            assert!(bad.validate().is_err());
        }
        let legacy =
            include_str!("../../../config/aptd.conf.example").replace("allow_remote = false\n", "");
        assert!(
            !toml::from_str::<Config>(&legacy)
                .unwrap()
                .admin
                .allow_remote
        );
    }
    #[test]
    fn administrative_defaults_and_dangerous_options() {
        let text = include_str!("../../../config/aptd.conf.example");
        let legacy = text.split("[admin]").next().unwrap();
        let c: Config = toml::from_str(legacy).unwrap();
        assert!(c.admin.enabled);
        c.validate().unwrap();
        for origin in [
            "https://user:pass@admin.invalid",
            "https://admin.invalid/path",
            "https://ADMIN.invalid",
            "https://admin.invalid:443",
            "https://admin.invalid/",
            "javascript:bad",
        ] {
            let mut bad = c.clone();
            bad.admin.external_url = origin.into();
            assert!(bad.validate().is_err());
        }
        let mut bad = c.clone();
        bad.admin.session_lifetime_seconds = 0;
        assert!(bad.validate().is_err());
        bad = c.clone();
        bad.admin.login_max_attempts = 0;
        assert!(bad.validate().is_err());
        bad = c;
        bad.admin.max_sessions_per_user = 0;
        assert!(bad.validate().is_err());
        assert!(toml::from_str::<Config>(&format!("{text}\nrequire_auth = false\n")).is_err());
    }
    #[test]
    fn initialization_preserves_config_and_key() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("aptd.conf");
        let c = Config::initialize(&p, Some(d.path())).unwrap();
        let key = c.paths.keys.join("private");
        fs::write(&key, b"sentinel").unwrap();
        let original = fs::read(&p).unwrap();
        Config::initialize(&p, Some(d.path())).unwrap();
        assert_eq!(original, fs::read(p).unwrap());
        assert_eq!(fs::read(key).unwrap(), b"sentinel");
    }
}
