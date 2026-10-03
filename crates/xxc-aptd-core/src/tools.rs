use anyhow::{Context, Result, ensure};
use std::{
    ffi::OsStr,
    io::{Read, Write},
    os::unix::process::CommandExt,
    path::Path,
    process::{Command, Stdio},
    time::Duration,
};
use wait_timeout::ChildExt;

/// Debian tools receive only argv, never a shell. Descendants share a process
/// group so a deadline also terminates helpers holding output pipes open.
pub fn run<I, S>(
    program: &str,
    args: I,
    cwd: &Path,
    timeout: Duration,
    limit: u64,
) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    run_input(program, args, cwd, timeout, limit, None)
}

/// Optional bounded stdin avoids putting credentials or key material in argv.
pub fn run_input<I, S>(
    program: &str,
    args: I,
    cwd: &Path,
    timeout: Duration,
    limit: u64,
    input: Option<zeroize::Zeroizing<Vec<u8>>>,
) -> Result<Vec<u8>>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let mut child = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .env("LC_ALL", "C")
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()
        .with_context(|| format!("Cannot start required tool {program}"))?;
    let writer = input.map(|input| {
        let mut stdin = child.stdin.take().expect("requested piped stdin");
        std::thread::spawn(move || stdin.write_all(&input))
    });
    let stdout = child.stdout.take().context("missing stdout")?;
    let stderr = child.stderr.take().context("missing stderr")?;
    let out = std::thread::spawn(move || {
        let mut b = Vec::new();
        stdout.take(limit + 1).read_to_end(&mut b).map(|_| b)
    });
    let err = std::thread::spawn(move || {
        let mut b = Vec::new();
        stderr.take(65537).read_to_end(&mut b).map(|_| b)
    });
    let result = child.wait_timeout(timeout);
    // SAFETY: pid is from this live child, placed in its own process group.
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.wait();
    let output = out
        .join()
        .map_err(|_| anyhow::anyhow!("tool output reader failed"))??;
    let diagnostic = err
        .join()
        .map_err(|_| anyhow::anyhow!("tool error reader failed"))??;
    let input_result = writer.map(|writer| writer.join());
    let status = result?.context("External tool timed out")?;
    ensure!(
        output.len() as u64 <= limit && diagnostic.len() <= 65536,
        "External tool exceeded output limit"
    );
    // Raw diagnostics may contain untrusted control values or secret filenames.
    ensure!(
        status.success(),
        "External tool {program} failed (exit {}); inspect tool configuration and input",
        status.code().unwrap_or(-1)
    );
    if let Some(writer) = input_result {
        writer.map_err(|_| anyhow::anyhow!("Tool input writer failed"))??;
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failures_and_deadlines_are_errors() {
        assert!(
            run(
                "false",
                [] as [&str; 0],
                Path::new("/"),
                Duration::from_secs(1),
                100
            )
            .is_err()
        );
        let now = std::time::Instant::now();
        assert!(
            run(
                "sleep",
                ["10"],
                Path::new("/"),
                Duration::from_millis(20),
                100
            )
            .is_err()
        );
        assert!(now.elapsed() < Duration::from_secs(2));
    }
}
