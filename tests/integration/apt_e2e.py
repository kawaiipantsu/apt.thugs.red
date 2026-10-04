#!/usr/bin/env python3
"""Disposable real APT test: no host sources, keys, package database or services change."""
import hashlib
import http.client
import json
import os
import pathlib
import shutil
import signal
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request

PROJECT = pathlib.Path(__file__).resolve().parents[2]
PROFILE = os.environ.get("XXC_TEST_PROFILE", "debug")
DAEMON = PROJECT / "target" / PROFILE / "xxc-aptd"
CLI = PROJECT / "target" / PROFILE / "xxc-apt-cli"


def run(argv, **kwargs):
    p = subprocess.run([str(x) for x in argv], stdout=subprocess.PIPE,
                       stderr=subprocess.PIPE, timeout=120, **kwargs)
    if p.returncode:
        raise AssertionError(f"{pathlib.Path(str(argv[0])).name} failed with exit {p.returncode}; test diagnostics retained only in temporary storage")
    return p.stdout


def free_port():
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


class UnixHTTP(http.client.HTTPConnection):
    def __init__(self, path):
        super().__init__("localhost", timeout=30)
        self.path = path

    def connect(self):
        self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.sock.settimeout(self.timeout)
        self.sock.connect(str(self.path))


class Harness:
    def __init__(self, root):
        self.root = root
        self.config = root / "aptd.conf"
        self.port, self.admin_port = free_port(), free_port()
        while self.port == self.admin_port:
            self.admin_port = free_port()
        self.origin = f"http://127.0.0.1:{self.port}"
        self.admin_origin = f"http://127.0.0.1:{self.admin_port}"
        self.process = None
        self.log = open(root / "process.log", "wb")
        run([DAEMON, "--config", self.config, "init", "--root", root])
        self.keys = root / "keys"
        run(["gpg", "--batch", "--homedir", self.keys, "--pinentry-mode", "loopback",
             "--passphrase", "", "--quick-generate-key", "XXC disposable integration fixture", "ed25519", "sign", "1d"])
        records = run(["gpg", "--homedir", self.keys, "--with-colons", "--list-secret-keys"]).decode()
        self.fingerprint = next(x.split(":")[9] for x in records.splitlines() if x.startswith("fpr:"))
        text = self.config.read_text().replace("127.0.0.1:8088", f"127.0.0.1:{self.port}").replace("127.0.0.1:8089", f"127.0.0.1:{self.admin_port}").replace('fingerprint = ""', f'fingerprint = "{self.fingerprint}"').replace("https://apt.thugs.red", self.origin).replace("https://admin.apt.thugs.red", self.admin_origin)
        self.config.write_text(text)
        self.config.write_text(self.config.read_text().replace('max_upload_bytes = 2147483648', 'max_upload_bytes = 8388608'))
        self.config.write_text(self.config.read_text().replace('public_ascii_name = "thugsred.gpg.key"', 'public_ascii_name = "fixture-public.asc"').replace('public_keyring_name = "thugsred-archive-keyring.gpg"', 'public_keyring_name = "fixture-public.gpg"'))

    def start(self, env=None):
        args = [DAEMON, "--config", self.config, "serve"]
        if os.geteuid() == 0:
            args.append("--allow-root")
        self.process = subprocess.Popen([str(x) for x in args], stdout=self.log, stderr=self.log, env=env)
        for _ in range(100):
            if self.process.poll() is not None:
                raise AssertionError("daemon failed to start")
            try:
                if self.api("GET", "status")[0] == 200:
                    return
            except (OSError, http.client.HTTPException):
                pass
            time.sleep(.05)
        raise AssertionError("daemon did not become ready")

    def stop(self, hard=False):
        if self.process and self.process.poll() is None:
            self.process.send_signal(signal.SIGKILL if hard else signal.SIGTERM)
            try:
                self.process.wait(timeout=30)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait(timeout=10)
                raise AssertionError("daemon did not drain within the test deadline")
            if not hard:
                assert self.process.returncode == 0, "graceful shutdown failed"

    def api(self, method, path, data=None, raw=False):
        connection = UnixHTTP(self.root / "run/admin.sock")
        body = data if raw else (json.dumps(data).encode() if data is not None else None)
        connection.request(method, "/api/v1/" + path, body, {"Content-Type": "application/octet-stream" if raw else "application/json"})
        response = connection.getresponse()
        result = response.status, json.loads(response.read())
        connection.close()
        return result

    def job(self, path, data=None, success=True):
        if path == "repository/publish" and data is None:
            data = {"review_token": self.api("GET", "repository/diff")[1]["token"]}
        status, result = self.api("POST", path, data or {})
        assert status == 202, result
        for _ in range(400):
            _, job = self.api("GET", "jobs/" + result["job_id"])
            if job["state"] != "running":
                assert (job["state"] == "succeeded") == success, job
                return job
            time.sleep(.05)
        raise AssertionError("repository job timed out")

    def fetch(self, path, method="GET", headers=None):
        # http.client preserves malicious paths which URL libraries may normalize.
        c = http.client.HTTPConnection("127.0.0.1", self.port, timeout=10)
        c.request(method, path, headers=headers or {})
        r = c.getresponse()
        result = r.status, dict(r.getheaders()), r.read()
        c.close()
        return result

    def fixture(self, version="1.0", content="fixture\n", arch="all", name="xxc-fixture"):
        tree = self.root / f"build-{name}-{version}-{arch}"
        (tree / "DEBIAN").mkdir(parents=True, exist_ok=True)
        (tree / "DEBIAN/control").write_text(f"Package: {name}\nVersion: {version}\nArchitecture: {arch}\nMaintainer: XXC integration fixture\nSection: misc\nPriority: optional\nDescription: searchable isolated APT fixture\n Tests the real archive acquisition path.\n")
        (tree / "usr/share/xxc-fixture").mkdir(parents=True, exist_ok=True)
        (tree / "usr/share/xxc-fixture/data").write_text(content)
        output = tree.with_suffix(".deb")
        run(["dpkg-deb", "--root-owner-group", "--build", tree, output])
        return output


def apt_acquire(h, package, suite="zerotrust"):
    key = h.root / "client-keyring.gpg"
    key.write_bytes(h.fetch("/repo/thugsred-archive-keyring.gpg")[2])
    assert b"BEGIN PGP PUBLIC KEY" in h.fetch("/repo/thugsred.gpg.key")[2]
    assert b"BEGIN PGP PUBLIC KEY" in h.fetch("/repo/thugsred-archive-keyring.asc")[2]
    assert h.fetch('/repo/fixture-public.gpg')[2] == key.read_bytes()
    assert h.fetch('/repo/fixture-public.asc')[2] == h.fetch('/repo/thugsred.gpg.key')[2]
    apt = h.root / "apt"
    for directory in ["etc", "state/lists/partial", "cache/archives/partial", "download"]:
        (apt / directory).mkdir(parents=True)
    (apt / "state/status").write_text("")
    (apt / "etc/sources.list").write_text(f"deb [arch=amd64 signed-by={key}] {h.origin}/repo {suite} main\n")
    cfg = apt / "apt.conf"
    import pwd
    cfg.write_text(f'Dir "{apt}";\nDir::Etc "{apt}/etc";\nDir::Etc::main "-";\nDir::Etc::parts "-";\nDir::Etc::sourcelist "sources.list";\nDir::Etc::sourceparts "-";\nDir::State "{apt}/state";\nDir::State::status "{apt}/state/status";\nDir::Cache "{apt}/cache";\nAPT::Sandbox::User "{pwd.getpwuid(os.getuid()).pw_name}";\nAPT::Get::List-Cleanup "false";\nAcquire::By-Hash "force";\nAcquire::Languages "none";\n')
    env = dict(os.environ, APT_CONFIG=str(cfg), LC_ALL="C")
    run(["apt-get", "update", "-o", "APT::Update::Error-Mode=any"], env=env)
    assert b"xxc-fixture" in run(["apt-cache", "show", "xxc-fixture"], env=env)
    run(["apt-get", "download", "xxc-fixture"], env=env, cwd=apt / "download")
    acquired = next((apt / "download").glob("*.deb"))
    assert hashlib.sha256(acquired.read_bytes()).hexdigest() == package["sha256"]
    run(["dpkg-deb", "--extract", acquired, apt / "extracted"])
    assert (apt / "extracted/usr/share/xxc-fixture/data").read_text() == "fixture\n"


def exercise(h):
    h.start()
    assert json.loads(run([CLI, "--socket", h.root / "run/admin.sock", "--json", "status"]))["packages"] == 0
    for route in ["/", "/packages", "/help", "/about", "/repo/", "/favicon.svg", "/robots.txt", "/sitemap.xml"]:
        assert h.fetch(route)[0] == 200, route
    assert h.fetch("/api/v1/status")[0] == 404
    assert h.fetch("/", headers={"Host": "untrusted.invalid"})[0] == 400
    assert h.fetch("/", headers={"X-Forwarded-Host": "untrusted.invalid"})[0] == 200
    private = urllib.request.Request(f"http://127.0.0.1:{h.admin_port}/api/v1/status")
    try:
        urllib.request.urlopen(private)
        raise AssertionError("administrative HTTP unexpectedly authorized")
    except urllib.error.HTTPError as e:
        assert e.code == 401
    assert h.api("POST", "uploads", b"not a Debian package", raw=True)[0] == 400
    assert h.api("POST", "uploads", b"x" * (8 * 1024 * 1024 + 1), raw=True)[0] == 413
    interrupted = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    interrupted.connect(str(h.root / "run/admin.sock"))
    interrupted.sendall(b"POST /api/v1/uploads HTTP/1.1\r\nHost: localhost\r\nContent-Length: 10000\r\n\r\npartial")
    interrupted.close()
    time.sleep(.1)
    assert not list((h.root / 'state/uploads').glob('.tmp*')), 'Interrupted upload leaked quarantine scratch data'
    fixture = h.fixture()
    malformed = h.root / "malformed.deb"
    shutil.copyfile(fixture, malformed)
    broken = h.root / 'data.tar.xz'
    broken.write_bytes(b'not an xz tar archive')
    run(['ar', 'r', malformed, broken])
    assert h.api('POST', 'uploads', malformed.read_bytes(), raw=True)[0] == 400
    status, package = h.api("POST", "uploads", fixture.read_bytes(), raw=True)
    assert status == 200
    assert h.api("POST", "uploads", fixture.read_bytes(), raw=True)[1]["id"] == package["id"]
    conflict = h.fixture(content="different bytes\n")
    assert h.api("POST", "uploads", conflict.read_bytes(), raw=True)[0] == 400
    assert h.api("POST", f"uploads/{package['id']}/stage", {})[0] == 200
    h.job("repository/publish")
    # A pre-suite manifest has no membership map; keep it usable for verify,
    # restart/reindex and subsequent rollback without rewriting its signatures.
    current = (h.root / 'state/repository/dists').resolve().parent
    manifest_path = current / 'manifest.json'
    legacy = json.loads(manifest_path.read_text())
    legacy.pop('suites')
    manifest_path.write_text(json.dumps(legacy))
    h.job('repository/verify')
    current = h.api("GET", "status")[1]["generation"]
    assert h.fetch("/packages/xxc-fixture")[0] == 200
    assert b"xxc-fixture" in h.fetch("/search?q=searchable")[2]
    for arch in ["amd64", "arm64", "all"]:
        assert b"Package: xxc-fixture" in h.fetch(f"/repo/dists/zerotrust/main/binary-{arch}/Packages")[2]
    pool = "/repo/" + package["filename"]
    status, headers, content = h.fetch(pool)
    assert status == 200 and hashlib.sha256(content).hexdigest() == package["sha256"]
    assert h.fetch(pool, "HEAD")[2] == b""
    assert h.fetch(pool, headers={"Range": "bytes=0-15"})[0::2] == (206, content[:16])
    assert h.fetch(pool, headers={"If-None-Match": headers["etag"]})[0] == 304
    assert h.fetch(pool, headers={"If-Modified-Since": headers["last-modified"]})[0] == 304
    assert h.fetch(pool, headers={"If-None-Match": '"mismatch"', "If-Modified-Since": headers["last-modified"]})[0] == 200
    for path in ["../keys", "%2e%2e/keys", "%252e%252e/keys", "pool/../../keys", "pool/%2e%2e/%2e%2e/keys", "pool%5c..%5ckeys", "/etc/passwd", ".generations/", "pool/.hidden"]:
        assert h.fetch("/repo/" + path)[0] >= 400, path
    os.symlink("/etc/passwd", h.root / "state/repository/pool/escape")
    assert h.fetch("/repo/pool/escape")[0] == 404
    (h.root / "state/repository/pool/escape").unlink()
    apt_acquire(h, package)
    old_index = h.fetch("/repo/dists/zerotrust/main/binary-amd64/Packages.xz")[2]
    old_hash = hashlib.sha256(old_index).hexdigest()
    second = h.fixture(version="2.0", arch="amd64")
    package2 = h.api("POST", "uploads", second.read_bytes(), raw=True)[1]
    h.api("POST", f"uploads/{package2['id']}/stage", {})
    h.job("repository/publish")
    assert h.fetch(f"/repo/dists/zerotrust/main/binary-amd64/by-hash/SHA256/{old_hash}")[2] == old_index
    h.job("repository/verify")
    h.job("repository/rollback", {"generation": current})
    assert h.api("GET", "status")[1]["generation"] == current
    assert h.fetch(pool)[2] == content
    # Invalid signer must fail without changing the live generation.
    h.stop()
    text = h.config.read_text()
    h.config.write_text(text.replace(h.fingerprint, "A" * 40))
    h.start()
    h.job("repository/publish", success=False)
    assert h.api("GET", "status")[1]["generation"] == current
    assert h.fetch(pool)[2] == content
    h.stop()
    h.config.write_text(text)
    # Block metadata generation, prove HTTP remains available, reject a second
    # publish, then kill the process before activation and recover on restart.
    toolbin = h.root / "tools"
    toolbin.mkdir()
    marker = h.root / "tool-started"
    tool = toolbin / "apt-ftparchive"
    tool.write_text("#!/usr/bin/python3\nimport pathlib,time\npathlib.Path(" + repr(str(marker)) + ").touch()\ntime.sleep(60)\n")
    tool.chmod(0o755)
    h.start(dict(os.environ, PATH=str(toolbin) + os.pathsep + os.environ["PATH"]))
    status, job = h.api("POST", "repository/publish", {"review_token": h.api("GET", "repository/diff")[1]["token"]})
    assert status == 202
    for _ in range(100):
        if marker.exists():
            break
        time.sleep(.02)
    assert marker.exists()
    assert h.api("POST", "repository/publish", {"review_token":"stale"})[0] == 409
    assert h.fetch(pool)[2] == content
    h.stop(hard=True)
    h.start()
    assert h.api("GET", "status")[1]["generation"] == current
    assert h.api("GET", "jobs/" + job["job_id"])[1]["state"] == "failed"
    h.job("repository/reindex")
    h.stop()
    print("PASS: real signed APT update/download, ingest, duplicates, all architecture, UI routes, traversal, symlink containment, HEAD/range/conditionals, rollback, retained by-hash, failed signer, duplicate jobs and interrupted publication")


if __name__ == "__main__":
    for tool in ["gpg", "apt-ftparchive", "dpkg-deb", "apt-get", "apt-cache"]:
        if not shutil.which(tool):
            raise SystemExit(f"Required test tool missing: {tool}")
    with tempfile.TemporaryDirectory(prefix="xxc-aptd-e2e-") as temporary:
        harness = Harness(pathlib.Path(temporary))
        try:
            exercise(harness)
        except Exception:
            # Tool messages are already sanitized by the application. Retain a
            # local diagnostic copy without printing signing output or identities.
            destination = PROJECT / ".build" / "last-e2e.log"
            destination.parent.mkdir(exist_ok=True)
            shutil.copyfile(pathlib.Path(temporary) / "process.log", destination)
            destination.chmod(0o600)
            raise
        finally:
            harness.stop(hard=True)
            subprocess.run(["gpgconf", "--homedir", str(harness.keys), "--kill", "gpg-agent"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            harness.log.close()
