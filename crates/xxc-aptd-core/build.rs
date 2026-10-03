use std::process::Command;

fn output(args: &[&str]) -> Option<String> {
    let result = Command::new("git").args(args).output().ok()?;
    result
        .status
        .success()
        .then(|| String::from_utf8_lossy(&result.stdout).trim().to_owned())
}
fn main() {
    for path in [
        "../../.git/HEAD",
        "../../.git/index",
        "../../.git/refs/heads",
        "src",
        "../xxc-aptd/src",
        "../xxc-aptd-web/src",
        "../xxc-apt-cli/src",
    ] {
        if std::path::Path::new(path).exists() {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    let sha = output(&["rev-parse", "--short=12", "HEAD"]).unwrap_or_else(|| "uncommitted".into());
    let dirty = output(&["status", "--porcelain"]).is_some_and(|s| !s.is_empty());
    let version = std::env::var("CARGO_PKG_VERSION").expect("Cargo version");
    println!(
        "cargo:rustc-env=XXC_BUILD_VERSION={version} ({sha}{})",
        if dirty { ", dirty" } else { "" }
    );
}
