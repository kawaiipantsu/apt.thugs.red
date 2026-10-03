use anyhow::{Context, Result};
use std::{
    fs::OpenOptions,
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
};
use tracing_subscriber::EnvFilter;
use xxc_aptd_core::config::Config;
struct Writer {
    path: PathBuf,
}
impl Write for Writer {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        let _ = io::stderr().write_all(b);
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o640)
            .open(&self.path)
        {
            let _ = file.write_all(b);
        }
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }
}
pub fn initialize(c: &Config) -> Result<()> {
    let path = c.logging.file.clone();
    if let Err(e) = OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o640)
        .open(&path)
    {
        if c.logging.strict {
            return Err(e).context("Strict logging: cannot open operational log");
        }
        eprintln!("Operational file log unavailable; using stderr/journald.");
    }
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_new(&c.logging.level)?)
        .with_ansi(false)
        .with_writer(move || Writer { path: path.clone() })
        .init();
    Ok(())
}
