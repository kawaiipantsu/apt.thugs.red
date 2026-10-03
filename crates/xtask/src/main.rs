fn main() -> anyhow::Result<()> {
    let status = std::process::Command::new("python3")
        .arg("scripts/maintain.py")
        .args(std::env::args().skip(1))
        .status()?;
    anyhow::ensure!(status.success(), "Maintenance task failed");
    Ok(())
}
