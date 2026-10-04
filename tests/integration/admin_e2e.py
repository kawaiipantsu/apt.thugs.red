#!/usr/bin/env python3
"""Exercise the real HTTP authentication boundary and Unix bootstrap with disposable data."""
from apt_e2e import Harness, CLI, PROJECT, run
from pathlib import Path
import http.client
import json
import sqlite3
import subprocess
import tempfile
import time

PASSWORD = "disposable-integration-password"

class Browser:
    def __init__(self, h):
        self.h, self.cookies, self.csrf = h, {}, None

    def request(self, method, path, data=None, headers=None, raw=False):
        h = {"Host": f"127.0.0.1:{self.h.admin_port}"}
        if self.cookies:
            h["Cookie"] = "; ".join(f"{k}={v}" for k, v in self.cookies.items())
        if method != "GET":
            h["Origin"] = self.h.admin_origin
            if self.csrf:
                h["X-CSRF-Token"] = self.csrf
        if data is not None:
            h["Content-Type"] = "application/octet-stream" if raw else "application/json"
        h.update(headers or {})
        c = http.client.HTTPConnection("127.0.0.1", self.h.admin_port, timeout=30)
        c.request(method, path, body=data if raw else (json.dumps(data).encode() if data is not None else None), headers=h)
        response = c.getresponse()
        body = response.read()
        fields = response.getheaders()
        for key, value in fields:
            if key.lower() == "set-cookie":
                name, value = value.split(";", 1)[0].split("=", 1)
                self.cookies[name] = value
        result = response.status, dict(fields), json.loads(body) if body.startswith(b'{') else body
        c.close()
        return result

    def login(self, username, password=PASSWORD):
        status, _, challenge = self.request("GET", "/api/v1/auth/challenge")
        assert status == 200
        result = self.request("POST", "/api/v1/auth/login", {"username": username, "password": password, "csrf": challenge["csrf"]})
        if result[0] == 200:
            self.csrf = result[2]["csrf"]
        return result


def exercise(h):
    h.start()
    socket = h.root / "run/admin.sock"
    users = {}
    for name, role in [("admin", "administrator"), ("operator", "operator"), ("viewer", "viewer")]:
        users[name] = json.loads(run([CLI, "--socket", socket, "--json", "user", "add", name, "--role", role, "--password-stdin"], input=(PASSWORD + "\n").encode()))
    assert "password" not in json.dumps(h.api("GET", "users")[1])
    unauth = Browser(h)
    assert unauth.request("GET", "/api/v1/status", headers={"X-Remote-User": "admin", "X-Forwarded-For": "127.0.0.1"})[0] == 401
    assert unauth.request("GET", "/admin/")[0] == 303
    assert unauth.request("GET", "/healthz")[0] == 401
    assert unauth.request("GET", "/admin/login", headers={"Host": "host.invalid"})[0] == 400
    assert unauth.request("GET", "/admin/login")[0] == 200
    challenge = unauth.request("GET", "/api/v1/auth/challenge")[2]["csrf"]
    input = {"username":"admin", "password":PASSWORD, "csrf":challenge}
    assert unauth.request("POST", "/api/v1/auth/login", input, headers={"Origin":"https://untrusted.invalid"})[0] == 403
    assert unauth.request("POST", "/api/v1/auth/login", {**input,"csrf":"0"*64})[0] == 403
    admin = Browser(h)
    status, headers, identity = admin.login("admin")
    assert status == 200 and identity["user"]["role"] == "administrator"
    session_cookie = admin.cookies["xxc-session"]
    # Login preauthentication token is single-use.
    assert unauth.request("POST", "/api/v1/auth/login", {**input,"password":"incorrect"})[0] == 401
    assert unauth.request("POST", "/api/v1/auth/login", input)[0] == 403
    assert admin.request("GET", "/api/v1/status")[0] == 200
    malformed = admin.request("POST","/api/v1/auth/login",b"{",headers={"Content-Type":"application/json"},raw=True)
    assert malformed[0] == 400 and "request_id" in malformed[2]["error"]
    assert admin.request("GET", "/api/v1/auth/session")[2]["csrf"] == admin.csrf
    assert admin.request("GET", "/admin/users")[0] == 200
    assert admin.request("POST", "/api/v1/repository/verify", {}, headers={"X-CSRF-Token":"bad"})[0] == 403
    assert admin.request("POST", "/api/v1/repository/verify", {}, headers={"Origin":"null"})[0] == 403
    for name in ["viewer", "operator"]:
        browser = Browser(h)
        assert browser.login(name)[0] == 200
        assert browser.request("GET", "/api/v1/status")[0] == 200
        assert browser.request("GET", "/api/v1/users")[0] == 403
        assert browser.request("GET", "/api/v1/config")[0] == 403
        assert browser.request("POST", "/api/v1/users", {"username":"forbidden","password":PASSWORD,"role":"administrator"})[0] == 403
        if name == "viewer":
            assert browser.request("POST", "/api/v1/uploads", b"fixture", raw=True)[0] == 403
            viewer = browser
        else:
            operator = browser
    fixture = h.fixture()
    # Multipart rejects CSRF before accepting file bytes; client paths are ignored.
    boundary = "xxc-disposable-boundary"
    def multipart(csrf, content, filename="../../outside.deb"):
        return (f'--{boundary}\r\nContent-Disposition: form-data; name="csrf"\r\n\r\n{csrf}\r\n'
                f'--{boundary}\r\nContent-Disposition: form-data; name="package"; filename="{filename}"\r\n'
                'Content-Type: application/vnd.debian.binary-package\r\n\r\n').encode() + content + f'\r\n--{boundary}--\r\n'.encode()
    multipart_headers = {"Content-Type":f"multipart/form-data; boundary={boundary}"}
    assert operator.request("POST","/admin/uploads",multipart("0"*64,b"invalid"),headers=multipart_headers,raw=True)[0] == 403
    assert operator.request("POST","/admin/uploads",multipart(operator.csrf,b"x"*(8*1024*1024+1)),headers=multipart_headers,raw=True)[0] == 413
    assert operator.request("POST","/admin/uploads",multipart(operator.csrf,fixture.read_bytes()),headers=multipart_headers,raw=True)[0] == 303
    assert not (h.root/"outside.deb").exists()
    assert not list((h.root/"state/uploads").glob('.tmp*'))
    incomplete = multipart(operator.csrf,b"partial")[:-len(boundary)-8]
    assert operator.request("POST","/admin/uploads",incomplete,headers=multipart_headers,raw=True)[0] == 400
    assert not list((h.root/"state/uploads").glob('.tmp*'))
    status, _, pkg = operator.request("POST", "/api/v1/uploads", fixture.read_bytes(), raw=True)
    assert status == 200
    initial = operator.request("GET", "/api/v1/repository/diff")[2]["token"]
    assert operator.request("POST", f"/api/v1/uploads/{pkg['id']}/stage", {})[0] == 200
    stale = operator.request("POST", "/api/v1/repository/publish", {"review_token":initial})
    assert stale[0] == 409 and stale[1]["x-request-id"] == stale[2]["error"]["request_id"]
    failure = admin.request("POST", "/api/v1/users/"+users["admin"]["id"], {"action":"disable"})
    assert failure[0] == 400 and failure[1]["x-request-id"] == failure[2]["error"]["request_id"]
    diff = operator.request("GET", "/api/v1/repository/diff")[2]
    assert len(diff["added"]) == 1 and diff["size_delta"] == pkg["size"]
    assert operator.request("GET", "/admin/publish")[0] == 200
    result = operator.request("POST", "/api/v1/repository/publish", {"review_token":diff["token"]})
    assert result[0] == 202
    for _ in range(300):
        job = operator.request("GET", "/api/v1/jobs/"+result[2]["job_id"])[2]
        if job["state"] != "running":
            break
        time.sleep(.05)
    assert job["state"] == "succeeded"
    # Core authorization and revocation also apply through the Unix API.
    assert h.api("POST", "users/"+users["admin"]["id"], {"action":"disable"})[0] == 400
    assert admin.request("POST", "/api/v1/users/"+users["viewer"]["id"], {"action":"role","role":"operator"})[0] == 200
    assert viewer.request("GET", "/api/v1/status")[0] == 401
    assert viewer.login("viewer")[0] == 200
    assert h.api("POST", "users/"+users["viewer"]["id"], {"action":"password","password":PASSWORD+"changed"})[0] == 200
    assert viewer.request("GET", "/api/v1/status")[0] == 401
    assert viewer.login("viewer")[0] == 401
    assert viewer.login("viewer",PASSWORD+"changed")[0] == 200
    assert h.api("POST", "users/"+users["viewer"]["id"], {"action":"disable"})[0] == 200
    assert viewer.request("GET", "/api/v1/status")[0] == 401
    assert viewer.login("viewer",PASSWORD+"changed")[0] == 401
    assert operator.request("POST", "/api/v1/auth/logout", {})[0] == 200
    assert operator.request("GET", "/api/v1/status")[0] == 401
    audit = h.api("GET", "audit")[1]["audit"]
    assert any(row["actor"] == users["operator"]["id"] and row["interface"] == "http" and row["action"] == "publish" and row["request_id"] == result[1]["x-request-id"] for row in audit)
    c = sqlite3.connect(h.root/"state/state.db")
    assert c.execute("SELECT count(*) FROM sessions WHERE token_hash=?", (session_cookie,)).fetchone()[0] == 0
    c.execute("UPDATE sessions SET expires=0"); c.commit();c.close()
    assert admin.request("GET", "/api/v1/status")[0] == 401
    h.stop()
    for path in [h.root/"aptd.log",h.root/"audit.log",h.root/"process.log"]:
        text = path.read_text()
        assert PASSWORD not in text and session_cookie not in text and admin.csrf not in text
    # Public HTTPS and explicitly configured HTTP administration are independent.
    original = h.config.read_text()
    h.config.write_text(original.replace(h.origin, "https://archive.example.invalid"))
    h.start()
    for path, mime, source in [
        ("/static/site.js", "text/javascript", PROJECT / "web/static/js/site.js"),
        ("/static/site.css", "text/css", PROJECT / "web/static/css/site.css"),
    ]:
        status, headers, body = h.fetch(path, headers={"Host":"archive.example.invalid"})
        assert status == 200 and headers["content-type"].startswith(mime)
        assert headers["x-content-type-options"] == "nosniff" and body == source.read_bytes()
        assert h.fetch(path, headers={"Host":"untrusted.invalid","X-Forwarded-Host":"archive.example.invalid"})[0] == 400
    assert h.fetch("/", headers={"Host":"archive.example.invalid"})[0] == 200
    assert b"URIs: https://archive.example.invalid/repo" in h.fetch("/repo/thugsred.sources", headers={"Host":"archive.example.invalid"})[2]
    assert h.fetch("/api/v1/status", headers={"Host":"archive.example.invalid"})[0] == 404
    separate_admin = Browser(h)
    status, headers, _ = separate_admin.request("GET", "/api/v1/auth/challenge", headers={"X-Forwarded-Proto":"https"})
    cookie = headers["set-cookie"]
    assert status == 200 and cookie.startswith("xxc-login=") and "; Secure" not in cookie
    assert "HttpOnly" in cookie and "SameSite=Strict" in cookie and "Domain=" not in cookie
    assert separate_admin.login("admin")[0] == 200
    assert separate_admin.request("GET", "/api/v1/status")[0] == 200
    assert separate_admin.request("POST", "/api/v1/repository/verify", {}, headers={"Origin":"https://archive.example.invalid"})[0] == 403
    assert separate_admin.request("POST", "/api/v1/auth/logout", {})[0] == 200
    h.stop()
    h.config.write_text(original)
    # Settings persist, and HTTPS origin selects a Secure host-only cookie.
    c = sqlite3.connect(h.root/"state/state.db")
    c.execute("DELETE FROM login_limits"); c.commit(); c.close()
    h.config.write_text(h.config.read_text().replace("login_max_attempts = 10", "login_max_attempts = 2"))
    h.start()
    attacker = Browser(h)
    for attempt in range(3):
        challenge = attacker.request("GET", "/api/v1/auth/challenge")[2]["csrf"]
        status = attacker.request("POST", "/api/v1/auth/login", {"username":"missing","password":"invalid","csrf":challenge}, headers={"X-Forwarded-For":f"192.0.2.{attempt}"})[0]
        assert status == (401 if attempt < 2 else 429)
    # Limits are durable across daemon restart, and spoofed peer headers do not reset them.
    h.stop(); h.start()
    assert Browser(h).login("missing","invalid")[0] == 429
    for index in range(6):
        assert Browser(h).login(f"missing-{index}","invalid")[0] == 401
    challenge = attacker.request("GET","/api/v1/auth/challenge")[2]["csrf"]
    assert attacker.request("POST","/api/v1/auth/login",{"username":"fresh","password":"invalid","csrf":challenge},headers={"X-Forwarded-For":"192.0.2.88"})[0] == 429
    h.stop()
    h.config.write_text(h.config.read_text().replace(h.admin_origin,"https://admin.example.invalid"))
    h.start()
    status, headers, _ = Browser(h).request("GET", "/api/v1/auth/challenge", headers={"X-Forwarded-Proto":"http"})
    cookie = headers["set-cookie"]
    assert status == 200 and "__Host-xxc-login=" in cookie and "; Secure" in cookie and "HttpOnly" in cookie and "SameSite=Strict" in cookie and "Domain=" not in cookie
    h.stop()
    h.config.write_text(h.config.read_text().replace("enabled = true","enabled = false"))
    h.start()
    assert Browser(h).request("GET","/admin/login")[0] == 503
    assert h.api("GET","status")[0] == 200
    print("PASS: Unix user bootstrap, Argon2 sessions, login CSRF/replay, roles, review binding, HTTP publication, audit actors, logout/reset/disable/expiry, Host/Origin/proxy rejection, throttling and Secure cookies")

if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="xxc-aptd-auth-") as temporary:
        h = Harness(Path(temporary))
        try:
            exercise(h)
        finally:
            h.stop()
            subprocess.run(["gpgconf","--homedir",str(h.keys),"--kill","gpg-agent"],stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
            h.log.close()
