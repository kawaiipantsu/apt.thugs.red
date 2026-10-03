#!/usr/bin/env python3
"""Run the browser suite against an isolated, signed fixture repository."""
from apt_e2e import Harness, PROJECT, run
from remote_signing_e2e import configure, SigningCA
import os
import json
import secrets
from pathlib import Path
import shutil
import subprocess
import tempfile

with tempfile.TemporaryDirectory(prefix="xxc-aptd-ui-") as temporary:
    h = Harness(Path(temporary))
    ca, service_env = configure(h)
    try:
        h.start(service_env)
        fixture = h.fixture()
        status, package = h.api("POST", "uploads", fixture.read_bytes(), raw=True)
        assert status == 200
        h.api("POST", "uploads/" + package["id"] + "/stage", {})
        h.job("repository/publish")
        password = secrets.token_urlsafe(24)
        for name,role in [("fixture-admin","administrator"),("fixture-viewer","viewer")]:
            assert h.api("POST","users",{"username":name,"password":password,"role":role})[0] == 200
        credentials = h.root / "browser-credentials.json"
        credentials.write_text(json.dumps({"username":"fixture-admin","viewer":"fixture-viewer","password":password}))
        credentials.chmod(0o600)
        new_fixture = h.fixture(version="3.0",name="xxc-browser-fixture")
        env = dict(os.environ, XXC_TEST_LOGIN=str(credentials), XXC_TEST_PACKAGE=str(new_fixture), XXC_TEST_ORIGIN=h.origin, XXC_TEST_ADMIN=f"http://127.0.0.1:{h.admin_port}")
        if shutil.which("chromium"):
            env.setdefault("CHROMIUM_PATH", shutil.which("chromium"))
        result = subprocess.run([str(PROJECT / "node_modules/.bin/playwright"), "test"], env=env)
        if result.returncode:
            raise SystemExit(result.returncode)
    finally:
        h.stop()
        ca.shutdown()
        ca.server_close()
        run(["gpgconf", "--homedir", SigningCA.home, "--kill", "gpg-agent"])
        subprocess.run(["gpgconf", "--homedir", str(h.keys), "--kill", "gpg-agent"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        h.log.close()
