//! Isolated public-only GPG keyrings for offline archive verification.
use super::{GpgSigner, Signer};
use crate::{config::Config, tools};
use anyhow::{Result, ensure};
use std::{ffi::OsString, path::Path, time::Duration};
use zeroize::Zeroizing;

pub struct PublicKey {
    config: Config,
    _directory: tempfile::TempDir,
}
impl PublicKey {
    pub fn new(c: &Config, fingerprint: &str, bytes: Zeroizing<Vec<u8>>) -> Result<Self> {
        ensure!(
            matches!(fingerprint.len(), 40 | 64)
                && fingerprint.bytes().all(|b| b.is_ascii_hexdigit()),
            "Invalid pinned public fingerprint"
        );
        ensure!(
            !bytes.is_empty() && bytes.len() <= 1024 * 1024,
            "Public signing key exceeds 1 MiB"
        );
        let directory = tempfile::tempdir_in(&c.paths.temporary)?;
        let mut config = c.clone();
        config.paths.keys = directory.path().to_owned();
        config.signing.fingerprint = fingerprint.into();
        let key = Self {
            config,
            _directory: directory,
        };
        // show-only never imports private packets. Inspect before any persistent import.
        let listing = key.command(
            &[
                "--with-colons",
                "--import-options",
                "show-only",
                "--dry-run",
                "--import",
            ],
            Some(bytes.clone()),
        )?;
        let listing = String::from_utf8(listing)?;
        let rows: Vec<Vec<&str>> = listing
            .lines()
            .map(|line| line.split(':').collect())
            .collect();
        ensure!(
            !rows
                .iter()
                .any(|r| matches!(r.first(), Some(&"sec" | &"ssb"))),
            "Remote export contains private signing material"
        );
        ensure!(
            rows.iter().filter(|r| r.first() == Some(&"pub")).count() == 1,
            "Expected one public primary key"
        );
        let primary = rows
            .iter()
            .position(|r| r.first() == Some(&"pub"))
            .expect("one primary");
        let fpr = rows
            .iter()
            .skip(primary + 1)
            .find(|r| r.first() == Some(&"fpr"))
            .and_then(|r| r.get(9));
        ensure!(
            fpr.is_some_and(|f| f.eq_ignore_ascii_case(fingerprint)),
            "Public key differs from pinned fingerprint"
        );
        key.command(
            &["--import-options", "import-minimal", "--import"],
            Some(bytes),
        )?;
        Ok(key)
    }
    fn command(&self, args: &[&str], input: Option<Zeroizing<Vec<u8>>>) -> Result<Vec<u8>> {
        let mut argv: Vec<OsString> = [
            "--batch",
            "--no-tty",
            "--no-options",
            "--no-autostart",
            "--no-auto-key-retrieve",
            "--homedir",
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        argv.push(self.config.paths.keys.as_os_str().to_owned());
        argv.extend(args.iter().map(OsString::from));
        tools::run_input(
            "gpg",
            argv,
            &self.config.paths.temporary,
            Duration::from_secs(self.config.signing.command_timeout_seconds),
            2 * 1024 * 1024,
            input,
        )
    }
    pub fn verify(&self, release: &Path) -> Result<()> {
        GpgSigner(&self.config).verify(release)
    }
    pub fn export(&self, armored: bool) -> Result<Vec<u8>> {
        GpgSigner(&self.config).export(armored)
    }
}
