#!/usr/bin/env python3
"""Install only distributed files; never remove config, repository data or keys."""
import gzip
import os
from pathlib import Path
import shutil
import sys

base = Path(__file__).resolve().parents[1]
dest = Path(os.environ.get("DESTDIR") or "/")
prefix = os.environ.get("PREFIX", "/usr")
etc = os.environ.get("SYSCONFDIR", "/etc")
var = os.environ.get("LOCALSTATEDIR", "/var")
files = {
    "target/release/xxc-aptd": (f"{prefix}/sbin/xxc-aptd", 0o755),
    "target/release/xxc-apt-cli": (f"{prefix}/bin/xxc-apt-cli", 0o755),
    "systemd/xxc-aptd.service": (f"{prefix}/lib/systemd/system/xxc-aptd.service", 0o644),
    "systemd/xxc-aptd.sysusers": (f"{prefix}/lib/sysusers.d/xxc-aptd.conf", 0o644),
    "systemd/xxc-aptd.tmpfiles": (f"{prefix}/lib/tmpfiles.d/xxc-aptd.conf", 0o644),
    "debian/xxc-aptd.logrotate": (f"{etc}/logrotate.d/xxc-aptd", 0o644),
}
for name, section in [("xxc-aptd", "8"), ("xxc-apt-cli", "1"), ("aptd.conf", "5")]:
    files[f".build/man/{name}.{section}.gz"] = (f"{prefix}/share/man/man{section}/{name}.{section}.gz", 0o644)

mode = sys.argv[1]
if mode not in {"install", "uninstall"}:
    raise SystemExit("select install or uninstall")
for source, (target, permissions) in files.items():
    path = dest / target.lstrip("/")
    if mode == "uninstall":
        path.unlink(missing_ok=True)
    else:
        path.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(base / source, path)
        path.chmod(permissions)
if mode == "install":
    notices = dest / prefix.lstrip("/") / "share/doc/xxc-aptd/dependency-licenses"
    shutil.copytree(base / ".build/dependency-licenses", notices, dirs_exist_ok=True)
    config = dest / etc.lstrip("/") / "xxc/aptd.conf"
    config.parent.mkdir(parents=True, exist_ok=True)
    try:
        with config.open("x") as f:
            f.write((base / "config/aptd.conf.example").read_text().replace("/etc/xxc", f"{etc}/xxc").replace("/var/", var + "/"))
        config.chmod(0o640)
    except FileExistsError:
        pass
else:
    notices = dest / prefix.lstrip("/") / "share/doc/xxc-aptd/dependency-licenses"
    if notices.exists():
        shutil.rmtree(notices)
print(f"{mode}: configuration, repository state and private keys preserved")
