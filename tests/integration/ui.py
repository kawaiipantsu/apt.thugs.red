#!/usr/bin/env python3
"""Run the browser suite against an isolated, signed fixture repository."""
from apt_e2e import Harness, PROJECT, run
from remote_signing_e2e import configure, SigningCA
from proxy_fixture import start_proxy
import os
import json
import secrets
from pathlib import Path
import shutil
import subprocess
import tempfile
import argparse

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--screenshots", action="store_true", help="capture documentation images using disposable fixture data")
parser.add_argument("--proxy", action="store_true", help="test through a real TLS proxy using /admin routing")
options = parser.parse_args()

with tempfile.TemporaryDirectory(prefix="xxc-aptd-ui-") as temporary:
    h = Harness(Path(temporary))
    ca, service_env = configure(h)
    proxy = None
    public_origin, admin_origin = h.origin, h.admin_origin
    try:
        if options.proxy:
            proxy, _ = start_proxy(h.root / 'proxy', h)
            public_origin = admin_origin = f'https://127.0.0.1:{proxy.server_port}'
            h.config.write_text(h.config.read_text().replace(h.origin, public_origin).replace(h.admin_origin, admin_origin))
        elif options.screenshots:
            # Only the generated client instructions use this reserved example
            # origin. Browser traffic still goes to the isolated loopback listener.
            h.config.write_text(h.config.read_text().replace(h.origin, "http://apt.example.test")
                                .replace("fixture-public.asc", "thugsred.gpg.key")
                                .replace("fixture-public.gpg", "thugsred-archive-keyring.gpg"))
        else:
            # Public HTTPS must not force Secure cookies onto the separate HTTP
            # admin test origin. Exercise the actual browser login/session flow.
            h.config.write_text(h.config.read_text().replace(h.origin, "https://archive.example.invalid"))
        h.start(service_env)
        fixture = h.fixture()
        status, package = h.api("POST", "uploads", fixture.read_bytes(), raw=True)
        assert status == 200
        h.api("POST", "uploads/" + package["id"] + "/stage", {})
        if options.screenshots:
            extra = h.fixture(version="1.0", name="xxc-browser-fixture")
            status, package = h.api("POST", "uploads", extra.read_bytes(), raw=True)
            assert status == 200
            assert h.api("POST", "uploads/" + package["id"] + "/stage", {})[0] == 200
        h.job("repository/publish")
        if options.screenshots:
            from analytics_fixture import seed
            seed(h.root)
        password = secrets.token_urlsafe(24)
        for name,role in [("fixture-admin","administrator"),("fixture-viewer","viewer")]:
            assert h.api("POST","users",{"username":name,"password":password,"role":role})[0] == 200
        credentials = h.root / "browser-credentials.json"
        credentials.write_text(json.dumps({"username":"fixture-admin","viewer":"fixture-viewer","password":password}))
        credentials.chmod(0o600)
        new_fixture = h.fixture(version="3.0",name="xxc-browser-fixture")
        env = dict(os.environ, XXC_TEST_LOGIN=str(credentials), XXC_TEST_PACKAGE=str(new_fixture), XXC_TEST_ORIGIN=public_origin, XXC_TEST_ADMIN=admin_origin)
        if options.proxy:
            env['XXC_TEST_SELF_SIGNED_PROXY'] = '1'
        if shutil.which("chromium"):
            env.setdefault("CHROMIUM_PATH", shutil.which("chromium"))
        command = (["node", str(PROJECT / "tests/ui/screenshots.mjs")] if options.screenshots
                   else [str(PROJECT / "node_modules/.bin/playwright"), "test"])
        result = subprocess.run(command, env=env)
        if result.returncode:
            raise SystemExit(result.returncode)
    finally:
        if proxy:
            proxy.shutdown()
            proxy.server_close()
        h.stop()
        ca.shutdown()
        ca.server_close()
        run(["gpgconf", "--homedir", SigningCA.home, "--kill", "gpg-agent"])
        subprocess.run(["gpgconf", "--homedir", str(h.keys), "--kill", "gpg-agent"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        h.log.close()
