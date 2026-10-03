use anyhow::{Result, ensure};
use std::{ffi::CString, path::Path, process::Command};
use xxc_aptd_core::config::Config;
pub fn accounts() -> Result<()> {
    // SAFETY: effective uid query has no preconditions.
    ensure!(
        unsafe { libc::geteuid() } == 0,
        "init --system requires root"
    );
    let text = "g xxc-aptd-admin -\nu xxc-aptd - \"XXC APT repository daemon\" /var/lib/xxc-aptd /usr/sbin/nologin\nm xxc-aptd xxc-aptd-admin\n";
    let mut child = Command::new("systemd-sysusers")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    use std::io::Write;
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("sysusers stdin unavailable"))?
        .write_all(text.as_bytes())?;
    ensure!(child.wait()?.success(), "systemd-sysusers failed");
    Ok(())
}
pub fn ownership(c: &Config, config: &Path) -> Result<()> {
    let user = CString::new("xxc-aptd")?;
    let group = CString::new("xxc-aptd-admin")?;
    // SAFETY: C names are NUL terminated; values copied before further calls.
    let (uid, gid) = unsafe {
        let pw = libc::getpwnam(user.as_ptr());
        ensure!(!pw.is_null(), "service account missing");
        let uid = (*pw).pw_uid;
        let gr = libc::getgrnam(group.as_ptr());
        ensure!(!gr.is_null(), "admin group missing");
        (uid, (*gr).gr_gid)
    };
    for p in [
        &c.paths.repository,
        &c.paths.staging,
        &c.paths.uploads,
        &c.paths.temporary,
        &c.paths.keys,
        &c.paths.runtime,
        c.paths
            .database
            .parent()
            .ok_or_else(|| anyhow::anyhow!("state parent missing"))?,
    ] {
        change(p, uid, gid)?;
    }
    for p in [
        c.paths.repository.join("pool"),
        c.paths.repository.join(".generations"),
    ] {
        change(&p, uid, gid)?;
    }
    change(config, 0, gid)?;
    use std::os::unix::fs::OpenOptionsExt;
    for p in [&c.logging.file, &c.logging.audit_file] {
        if !p.exists() {
            std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o640)
                .open(p)?;
            change(p, uid, gid)?;
        }
    }
    Ok(())
}
fn change(path: &Path, uid: u32, gid: u32) -> Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let name = CString::new(path.as_os_str().as_bytes())?;
    // SAFETY: valid NUL terminated pathname; no pointers outlive this call.
    ensure!(
        unsafe { libc::chown(name.as_ptr(), uid, gid) } == 0,
        "Cannot assign service directory ownership"
    );
    Ok(())
}
