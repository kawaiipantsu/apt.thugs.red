//! Remote signing runs inside the existing bounded publication worker.
use super::{PublicKey, Signer};
use crate::{config::Config, trust::TrustClient};
use anyhow::{Result, ensure};
use std::{fs, path::Path};
use tokio::runtime::Handle;

pub struct TrustSigner {
    client: TrustClient,
    runtime: Handle,
    public_key: PublicKey,
    key_id: String,
    fingerprint: String,
}
impl TrustSigner {
    /// Call only from a blocking worker. HTTP serving remains on Tokio workers.
    pub fn new(c: &Config, client: TrustClient, runtime: Handle) -> Result<Self> {
        let (key, bytes) = runtime.block_on(async {
            let key = client.openpgp_key(&c.signing.remote_key_id).await?;
            ensure!(
                key.status == "active"
                    && key.has_private_key
                    && key.capabilities.to_ascii_lowercase().contains('s'),
                "Remote key is not available for signing"
            );
            ensure!(
                key.fingerprint.eq_ignore_ascii_case(&c.signing.fingerprint),
                "Remote key differs from pinned fingerprint"
            );
            let bytes = client.openpgp_public_key(&key.id).await?;
            Ok::<_, anyhow::Error>((key, bytes))
        })?;
        let public_key = PublicKey::new(c, &c.signing.fingerprint, bytes)?;
        Ok(Self {
            client,
            runtime,
            public_key,
            key_id: key.id,
            fingerprint: c.signing.fingerprint.clone(),
        })
    }
}
impl Signer for TrustSigner {
    fn sign(&self, release: &Path) -> Result<()> {
        ensure!(
            fs::metadata(release)?.len() <= 8 * 1024 * 1024,
            "XXC Trust Release input exceeds 8 MiB"
        );
        let bytes = fs::read(release)?;
        let signatures = self.runtime.block_on(self.client.sign_release(
            &self.key_id,
            &self.fingerprint,
            &bytes,
        ))?;
        // Remote filenames are allowlisted by the client, never joined to a path.
        fs::write(release.with_file_name("InRelease"), signatures.inline)?;
        fs::write(release.with_file_name("Release.gpg"), signatures.detached)?;
        self.verify(release)
    }
    fn verify(&self, release: &Path) -> Result<()> {
        self.public_key.verify(release)
    }
    fn export(&self, armored: bool) -> Result<Vec<u8>> {
        self.public_key.export(armored)
    }
}
