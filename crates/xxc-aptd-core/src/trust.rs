//! Read-only X.509 inventory. APT signing remains exclusively OpenPGP.
use crate::config::XxcTrust;
pub mod openpgp;
use anyhow::{Context, Result, ensure};
use reqwest::{Client, header::HeaderValue};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{
    fs::OpenOptions,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::sync::Semaphore;
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct TrustClient {
    client: Client,
    config: XxcTrust,
    authorization: HeaderValue,
    requests: Arc<Semaphore>,
}
#[derive(Debug, Clone, Copy)]
pub struct TrustError(pub &'static str);
impl std::fmt::Display for TrustError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for TrustError {}
type RemoteResult<T> = std::result::Result<T, TrustError>;

#[derive(Debug, Serialize, Deserialize)]
pub struct Items<T> {
    pub items: Vec<T>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Authority {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub not_after: String,
    pub active: u8,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub days: u32,
    pub eku: String,
    pub algorithm: String,
    pub domain_suffix: String,
}
/// Explicit projection: no owner identity, CSR, private material or arbitrary JSON.
#[derive(Debug, Serialize, Deserialize)]
pub struct Certificate {
    pub id: String,
    pub authority_id: String,
    pub template_id: Option<String>,
    pub serial: String,
    pub label: String,
    pub not_before: String,
    pub not_after: String,
    pub revoked_at: Option<String>,
    pub status: String,
    pub sans: Vec<String>,
    pub fingerprint: String,
    pub algorithm: String,
    pub eku: String,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Certificates {
    pub items: Vec<Certificate>,
    pub total: u64,
    pub page: u32,
    pub pages: u32,
}
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct CertificateQuery {
    pub page: u32,
    pub q: String,
    pub status: String,
}
impl CertificateQuery {
    pub fn validate(&self) -> RemoteResult<()> {
        if self.page > 1_000_000
            || self.q.len() > 256
            || self.q.chars().any(char::is_control)
            || !["", "active", "expiring", "expired", "revoked"].contains(&self.status.as_str())
        {
            return Err(TrustError("trust_invalid_query"));
        }
        Ok(())
    }
}
#[derive(Debug, Serialize)]
pub struct TrustStatus {
    pub enabled: bool,
    pub connected: bool,
    pub authorities: usize,
    pub templates: usize,
    pub authority_available: bool,
    pub template_available: bool,
}
impl TrustClient {
    /// Construction validates local credentials, without depending on CA availability.
    pub fn new(config: &XxcTrust, credential_directory: Option<&Path>) -> Result<Option<Self>> {
        config.validate()?;
        if !config.enabled {
            return Ok(None);
        }
        let directory = credential_directory.context("XXC Trust requires CREDENTIALS_DIRECTORY; configure systemd LoadCredential for token_credential")?;
        ensure!(
            directory.is_absolute(),
            "XXC Trust credential directory must be absolute"
        );
        let authorization = read_credential(&directory.join(&config.token_credential))?;
        let timeout = Duration::from_secs(config.request_timeout_seconds);
        let mut builder = Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .connect_timeout(timeout)
            .user_agent(concat!("XXC-APTD/", env!("CARGO_PKG_VERSION")));
        if let Some(path) = &config.ca_certificate {
            let mut pem = Vec::new();
            std::fs::File::open(path)
                .context("Cannot open XXC Trust public CA certificate")?
                .take(131073)
                .read_to_end(&mut pem)
                .context("Cannot read XXC Trust public CA certificate")?;
            ensure!(
                pem.len() <= 131072,
                "XXC Trust CA certificate exceeds 128 KiB"
            );
            let cert = reqwest::Certificate::from_pem(&pem)
                .context("Invalid XXC Trust public CA certificate")?;
            builder = builder.add_root_certificate(cert);
        }
        let client = builder
            .build()
            .map_err(|_| anyhow::anyhow!("Cannot initialize XXC Trust HTTPS client"))?;
        Ok(Some(Self {
            client,
            config: config.clone(),
            authorization,
            requests: Arc::new(Semaphore::new(2)),
        }))
    }
    async fn get<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        query: &[(&str, String)],
    ) -> RemoteResult<T> {
        let bytes = self
            .request(reqwest::Method::GET, endpoint, query, None, 200)
            .await?;
        serde_json::from_slice(&bytes).map_err(|_| TrustError("trust_invalid_response"))
    }
    async fn request(
        &self,
        method: reqwest::Method,
        endpoint: &str,
        query: &[(&str, String)],
        body: Option<&serde_json::Value>,
        expected: u16,
    ) -> RemoteResult<Zeroizing<Vec<u8>>> {
        let _permit = self
            .requests
            .try_acquire()
            .map_err(|_| TrustError("trust_busy"))?;
        let mut request = self
            .client
            .request(method, format!("{}/{}", self.config.api_url, endpoint))
            .header(reqwest::header::AUTHORIZATION, self.authorization.clone())
            .header(reqwest::header::ACCEPT, "application/json")
            .query(query);
        if let Some(body) = body {
            request = request.json(body);
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| TrustError("trust_unavailable"))?;
        if response.status().as_u16() != expected {
            return Err(TrustError(match response.status().as_u16() {
                401 | 403 => "trust_credential_rejected",
                404 => "trust_key_not_found",
                409 => "trust_key_unavailable",
                413 => "trust_request_too_large",
                429 => "trust_rate_limited",
                300..=399 => "trust_redirect_rejected",
                _ => "trust_unavailable",
            }));
        }
        if response
            .content_length()
            .is_some_and(|n| n > self.config.max_response_bytes as u64)
        {
            return Err(TrustError("trust_response_too_large"));
        }
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| TrustError("trust_unavailable"))?
        {
            if bytes.len().saturating_add(chunk.len()) > self.config.max_response_bytes {
                return Err(TrustError("trust_response_too_large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
    pub async fn authorities(&self) -> RemoteResult<Items<Authority>> {
        self.get("authorities", &[]).await
    }
    pub async fn templates(&self) -> RemoteResult<Items<Template>> {
        self.get("templates", &[]).await
    }
    pub async fn certificates(&self, query: &CertificateQuery) -> RemoteResult<Certificates> {
        query.validate()?;
        let result: Certificates = self
            .get(
                "certificates",
                &[
                    ("page", query.page.max(1).to_string()),
                    ("q", query.q.clone()),
                    ("status", query.status.clone()),
                    ("authority", self.config.authority_id.clone()),
                ],
            )
            .await?;
        if result.page != query.page.max(1) {
            return Err(TrustError("trust_invalid_response"));
        }
        Ok(result)
    }
    pub async fn status(&self) -> RemoteResult<TrustStatus> {
        let authorities = self.authorities().await?;
        let templates = self.templates().await?;
        Ok(TrustStatus {
            enabled: true,
            connected: true,
            authorities: authorities.items.len(),
            templates: templates.items.len(),
            authority_available: self.config.authority_id.is_empty()
                || authorities
                    .items
                    .iter()
                    .any(|a| a.id == self.config.authority_id && a.active == 1),
            template_available: self.config.template_id.is_empty()
                || templates
                    .items
                    .iter()
                    .any(|t| t.id == self.config.template_id),
        })
    }
}
fn read_credential(path: &Path) -> Result<HeaderValue> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| {
            anyhow::anyhow!(
                "Cannot open XXC Trust token credential; check LoadCredential and permissions"
            )
        })?;
    let metadata = file
        .metadata()
        .context("Cannot inspect XXC Trust credential")?;
    let owner_only = metadata.mode() & 0o077 == 0;
    let systemd_credential = systemd_credential(&file, path, &metadata);
    ensure!(
        metadata.is_file()
            && metadata.len() <= 8194
            && (owner_only || systemd_credential)
            && (metadata.uid() == 0 || metadata.uid() == unsafe { libc::geteuid() }),
        "XXC Trust credential must be owner-only or a protected systemd credential, owned by root or the service user"
    );
    let mut token = Zeroizing::new(String::new());
    file.take(8195)
        .read_to_string(&mut token)
        .map_err(|_| anyhow::anyhow!("Cannot read XXC Trust credential"))?;
    let token = token.trim_end_matches(['\r', '\n']);
    ensure!(
        (16..=8192).contains(&token.len())
            && token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b)),
        "XXC Trust credential has an invalid length or encoding"
    );
    let value = Zeroizing::new(format!("Bearer {token}"));
    let mut header = HeaderValue::from_str(&value)
        .map_err(|_| anyhow::anyhow!("Invalid XXC Trust authorization credential"))?;
    header.set_sensitive(true);
    Ok(header)
}

/// Systemd can deliver root:root 0440 credentials with a named service-user ACL.
/// Only accept that form on its read-only mount under a root-owned 0550 directory.
fn systemd_credential(file: &std::fs::File, path: &Path, metadata: &std::fs::Metadata) -> bool {
    if metadata.uid() != 0 || metadata.gid() != 0 || metadata.mode() & 0o777 != 0o440 {
        return false;
    }
    let Some(parent) = path.parent() else {
        return false;
    };
    let Ok(directory) = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(parent)
    else {
        return false;
    };
    let Ok(parent) = directory.metadata() else {
        return false;
    };
    parent.uid() == 0
        && parent.gid() == 0
        && parent.mode() & 0o777 == 0o550
        && read_only_mount(file)
        && read_only_mount(&directory)
}
fn read_only_mount(file: &std::fs::File) -> bool {
    use std::os::fd::AsRawFd;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: live file descriptor and correctly sized writable output pointer.
    if unsafe { libc::fstatvfs(file.as_raw_fd(), stat.as_mut_ptr()) } != 0 {
        return false;
    }
    // SAFETY: successful fstatvfs initialized the output.
    unsafe { stat.assume_init().f_flag & libc::ST_RDONLY != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};
    #[test]
    fn credential_boundary() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("token");
        std::fs::write(&path, "synthetic_fixture_token_123456\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(read_credential(&path).unwrap().is_sensitive());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_credential(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o440)).unwrap();
        assert!(
            read_credential(&path).is_err(),
            "Group-readable files outside protected credential mounts must fail"
        );
        let link = directory.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(read_credential(&link).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        for token in [
            "short".to_owned(),
            "x".repeat(8193),
            "synthetic\ninjected_header_123456".into(),
        ] {
            std::fs::write(&path, token).unwrap();
            assert!(read_credential(&path).is_err());
        }
    }
    #[test]
    fn strict_configuration() {
        let mut c = XxcTrust::default();
        assert!(c.validate().is_ok());
        for url in [
            "http://ca.example.invalid/api/v1",
            "https://secret@ca.example.invalid/api/v1",
            "https://ca.example.invalid/api/v1?token=secret",
            "https://0.0.0.0/api/v1",
        ] {
            c.api_url = url.into();
            assert!(c.validate().is_err());
        }
        c = XxcTrust::default();
        c.token_credential = "../secret".into();
        assert!(c.validate().is_err());
        c = XxcTrust::default();
        c.authority_id = "invalid".into();
        assert!(c.validate().is_err());
        assert!(
            CertificateQuery {
                status: "../../download".into(),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}
