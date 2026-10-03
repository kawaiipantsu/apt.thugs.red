mod remote;
mod verification;
use crate::{config::Config, tools};
use anyhow::{Result, ensure};
use std::{ffi::OsString, fs, path::Path, time::Duration};
pub use verification::PublicKey;

pub trait Signer {
    fn sign(&self, release: &Path) -> Result<()>;
    fn verify(&self, release: &Path) -> Result<()>;
    fn export(&self, armored: bool) -> Result<Vec<u8>>;
}
pub struct GpgSigner<'a>(pub &'a Config);
impl GpgSigner<'_> {
    fn run(&self, args: Vec<OsString>) -> Result<Vec<u8>> {
        ensure!(
            !self.0.signing.fingerprint.is_empty(),
            "Configure signing.fingerprint before publication"
        );
        let mut argv = vec![
            "--batch".into(),
            "--no-tty".into(),
            "--no-options".into(),
            "--no-auto-key-retrieve".into(),
            "--homedir".into(),
            self.0.paths.keys.as_os_str().to_owned(),
        ];
        argv.extend(args);
        tools::run(
            "gpg",
            argv,
            &self.0.paths.temporary,
            Duration::from_secs(self.0.signing.command_timeout_seconds),
            8 * 1024 * 1024,
        )
    }
}
impl Signer for GpgSigner<'_> {
    fn sign(&self, release: &Path) -> Result<()> {
        for (mode, name) in [
            ("--clearsign", "InRelease"),
            ("--detach-sign", "Release.gpg"),
        ] {
            self.run(vec![
                "--yes".into(),
                "--local-user".into(),
                self.0.signing.fingerprint.clone().into(),
                "--digest-algo".into(),
                "SHA512".into(),
                "--output".into(),
                release.with_file_name(name).into_os_string(),
                mode.into(),
                release.as_os_str().to_owned(),
            ])?;
        }
        self.verify(release)
    }
    fn verify(&self, release: &Path) -> Result<()> {
        for inline in [false, true] {
            let mut args = vec![
                "--status-fd=1".into(),
                "--verify".into(),
                release
                    .with_file_name(if inline { "InRelease" } else { "Release.gpg" })
                    .into_os_string(),
            ];
            if !inline {
                args.push(release.as_os_str().to_owned());
            }
            let output = String::from_utf8(self.run(args)?)?;
            let wanted = self.0.signing.fingerprint.to_ascii_uppercase();
            ensure!(
                output.lines().any(|line| {
                    let fields: Vec<_> = line.split_whitespace().collect();
                    fields.len() >= 11
                        && fields[0] == "[GNUPG:]"
                        && fields[1] == "VALIDSIG"
                        && (fields[2] == wanted || fields.get(11) == Some(&wanted.as_str()))
                        && matches!(fields[9], "8" | "10")
                }),
                "Signature does not match pinned fingerprint or SHA-256/SHA-512 policy"
            );
            ensure!(
                !output
                    .lines()
                    .any(|line| ["BADSIG", "ERRSIG", "EXPKEYSIG", "REVKEYSIG"]
                        .iter()
                        .any(|flag| line.starts_with(&format!("[GNUPG:] {flag} ")))),
                "Invalid, expired or revoked signing key"
            );
        }
        // InRelease must contain exactly the detached Release content as well.
        let decoded = self.run(vec![
            "--decrypt".into(),
            release.with_file_name("InRelease").into_os_string(),
        ])?;
        ensure!(
            decoded == fs::read(release)?,
            "InRelease payload differs from Release"
        );
        Ok(())
    }
    fn export(&self, armored: bool) -> Result<Vec<u8>> {
        let mut args = Vec::new();
        if armored {
            args.push("--armor".into());
        }
        args.extend([
            "--export-options".into(),
            "export-minimal".into(),
            "--export".into(),
            self.0.signing.fingerprint.clone().into(),
        ]);
        let bytes = self.run(args)?;
        ensure!(
            !bytes.is_empty(),
            "Configured public signing key unavailable"
        );
        Ok(bytes)
    }
}

/// Construct a signing backend inside a bounded blocking publication worker.
pub fn configured<'a>(
    c: &'a Config,
    trust: Option<&crate::trust::TrustClient>,
    runtime: tokio::runtime::Handle,
) -> Result<Box<dyn Signer + 'a>> {
    match c.signing.backend.as_str() {
        "gpg" => Ok(Box::new(GpgSigner(c))),
        "xxc-trust" => Ok(Box::new(remote::TrustSigner::new(
            c,
            trust
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("XXC Trust client is disabled"))?,
            runtime,
        )?)),
        _ => anyhow::bail!("Unsupported signing backend"),
    }
}
