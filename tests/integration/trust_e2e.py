#!/usr/bin/env python3
"""Real HTTPS CA fixture plus wildcard HTTP listeners; all identities are synthetic."""
from apt_e2e import Harness, CLI, run, DAEMON
from admin_e2e import Browser, PASSWORD
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlsplit, parse_qs
import concurrent.futures
import json
import html as html_parser
import os
import ssl
import subprocess
import tempfile
import threading
import time

TOKEN = "synthetic_fixture_credential_123456"
OMITTED = "synthetic_sensitive_field_not_for_projection"
ID = "1" * 32


class FakeCA(BaseHTTPRequestHandler):
    mode = "ok"
    requests = []

    def log_message(self, *args):
        pass

    def do_GET(self):
        try:
            assert self.headers.get("Authorization") == "Bearer " + TOKEN
            parsed = urlsplit(self.path)
            FakeCA.requests.append((parsed.path, parse_qs(parsed.query)))
            mode = FakeCA.mode
            if mode == "timeout":
                time.sleep(2)
            if mode in ["401", "403", "429", "500", "redirect"]:
                self.send_response(302 if mode == "redirect" else int(mode))
                if mode == "redirect":
                    self.send_header("Location", "/private-key-export")
                self.end_headers()
                self.wfile.write(OMITTED.encode())
                return
            if parsed.path == "/api/v1/authorities":
                data = {"items": [{"id":ID,"parent_id":"","name":"Fixture CA","not_after":"2030-01-01","active":1,"private_key":OMITTED}]}
            elif parsed.path == "/api/v1/templates":
                data = {"items":[{"id":ID,"name":"Fixture client","days":30,"eku":"clientAuth","algorithm":"ec","domain_suffix":".example.invalid","token":OMITTED}]}
            elif parsed.path == "/api/v1/certificates":
                query = parse_qs(parsed.query)
                page = int(query.get("page", [1])[0])
                data = {"items":[{"id":ID,"authority_id":ID,"template_id":ID,"serial":"01","label":"<b>fixture</b>","not_before":"2026-01-01","not_after":"2030-01-01","revoked_at":None,"status":"active","sans":["client.example.invalid"],"fingerprint":"A"*64,"algorithm":"ec","eku":"clientAuth","csr":OMITTED,"owner_id":OMITTED,"has_private_key":True,"private_key":OMITTED}],"total":2,"page":page,"pages":2}
            else:
                raise AssertionError("Unexpected external request")
            body = json.dumps(data).encode()
            if mode == "malformed":
                body = b'not-json ' + OMITTED.encode()
            if mode in ["oversize", "stream-oversize"]:
                body = b'x' * 2048
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            if mode != "stream-oversize":
                self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError, ssl.SSLError):
            pass


def tls_server(root, handler):
    tls = root / "tls"
    tls.mkdir()
    cert, key = tls / "ca.pem", tls / "ca.key"
    run(["openssl","req","-x509","-newkey","rsa:2048","-nodes","-days","1","-subj","/CN=XXC disposable CA fixture","-addext","subjectAltName=IP:127.0.0.1","-keyout",key,"-out",cert])
    key.chmod(0o600)
    leaf, leaf_key, csr = tls/'server.pem', tls/'server.key', tls/'server.csr'
    extensions = tls/'server.ext'
    extensions.write_text('basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\nsubjectAltName=IP:127.0.0.1\n')
    run(['openssl','req','-new','-newkey','rsa:2048','-nodes','-subj','/CN=XXC disposable API fixture','-keyout',leaf_key,'-out',csr])
    run(['openssl','x509','-req','-in',csr,'-CA',cert,'-CAkey',key,'-CAcreateserial','-days','1','-extfile',extensions,'-out',leaf])
    leaf_key.chmod(0o600)
    ca = ThreadingHTTPServer(("127.0.0.1",0),handler)
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.load_cert_chain(leaf,leaf_key)
    ca.socket = context.wrap_socket(ca.socket, server_side=True)
    worker = threading.Thread(target=ca.serve_forever,daemon=True)
    worker.start()
    return ca, cert


def exercise(h, root):
    ca, cert = tls_server(root, FakeCA)
    credentials = root / "credentials"
    credentials.mkdir(mode=0o700)
    token = credentials / "xxc-trust-token"
    token.write_text(TOKEN)
    token.chmod(0o600)
    env = {**os.environ,"CREDENTIALS_DIRECTORY":str(credentials)}
    text = h.config.read_text().replace(f'public_listen = "127.0.0.1:{h.port}"',f'public_listen = "0.0.0.0:{h.port}"').replace(f'admin_listen = "127.0.0.1:{h.admin_port}"',f'admin_listen = "0.0.0.0:{h.admin_port}"').replace('allow_remote = false','allow_remote = true')
    text = text.replace('[xxc_trust]\nenabled = false','[xxc_trust]\nenabled = true').replace('https://ca.example.invalid/api/v1',f'https://127.0.0.1:{ca.server_port}/api/v1').replace('request_timeout_seconds = 10','request_timeout_seconds = 1').replace('max_response_bytes = 1048576','max_response_bytes = 1024')
    text += f'\nca_certificate = "{cert}"\n'
    h.config.write_text(text)
    try:
        h.start(env)
        # Both real sockets are wildcard-bound; no host firewall is modified.
        listeners = Path('/proc/net/tcp').read_text().splitlines()
        for port in [h.port, h.admin_port]:
            assert any(f'00000000:{port:04X}' in line and line.split()[3] == '0A' for line in listeners)
        assert h.fetch('/')[0] == 200
        assert h.fetch('/api/v1/trust/certificates')[0] == 404
        assert h.fetch('/',headers={'Host':'evil.example.invalid'})[0] == 400
        socket = root / 'run/admin.sock'
        for role in ['administrator','operator','viewer']:
            run([CLI,'--socket',socket,'user','add',role,'--role',role,'--password-stdin'],input=(PASSWORD+'\n').encode())
        anonymous = Browser(h)
        assert anonymous.request('GET','/api/v1/trust/status',headers={'X-Remote-User':'administrator'})[0] == 401
        assert anonymous.request('GET','/admin/login',headers={'Host':'evil.example.invalid'})[0] == 400
        for role in ['viewer','operator']:
            browser = Browser(h)
            assert browser.login(role)[0] == 200
            for route in ['/admin/trust','/admin/trust/certificates','/api/v1/trust/status','/api/v1/trust/certificates']:
                assert browser.request('GET',route)[0] == 403
        admin = Browser(h)
        login = admin.login('administrator')
        assert login[0] == 200 and 'Secure' not in login[1]['set-cookie']
        assert admin.request('POST','/api/v1/repository/verify',{},headers={'X-CSRF-Token':'wrong'})[0] == 403
        assert admin.request('POST','/api/v1/repository/verify',{},headers={'Origin':'https://evil.example.invalid'})[0] == 403
        for operation in ['status','authorities','templates','certificates']:
            response = admin.request('GET','/api/v1/trust/'+operation)
            assert response[0] == 200, (operation,response[2])
            output = json.dumps(response[2])
            assert OMITTED not in output and TOKEN not in output
            assert not any(key in output for key in ['private_key','owner_id','"csr"'])
            argv = [CLI,'--socket',socket,'--json','trust',operation]
            data = json.loads(run(argv))
            assert data == response[2]
        assert admin.request('GET','/api/v1/trust/status')[2]['connected']
        assert admin.request('GET','/api/v1/trust/certificates?page=2&q=fixture&status=active')[2]['page'] == 2
        assert FakeCA.requests[-1][1] == {'page':['2'],'q':['fixture'],'status':['active']}
        assert admin.request('GET','/api/v1/trust/certificates?status=invalid')[0] == 400
        assert admin.request('GET','/api/v1/trust/certificates?token=forbidden')[0] == 400
        for route in ['/admin/trust','/admin/trust/authorities','/admin/trust/templates','/admin/trust/certificates']:
            response = admin.request('GET',route)
            assert response[0] == 200
            assert OMITTED.encode() not in response[2] and TOKEN.encode() not in response[2]
        html = admin.request('GET','/admin/trust/certificates')[2]
        assert b'<b>fixture</b>' not in html and '<b>fixture</b>' in html_parser.unescape(html.decode())
        assert b'page=2' in html
        for mode, code in [('401','trust_credential_rejected'),('403','trust_credential_rejected'),('429','trust_rate_limited'),('500','trust_unavailable'),('redirect','trust_redirect_rejected'),('malformed','trust_invalid_response'),('oversize','trust_response_too_large'),('stream-oversize','trust_response_too_large'),('timeout','trust_unavailable')]:
            FakeCA.mode = mode
            status, headers, data = admin.request('GET','/api/v1/trust/status')
            assert status == 502 and data['error']['code'] == code, mode
            assert data['error']['request_id'] == headers['x-request-id']
            assert OMITTED not in json.dumps(data) and TOKEN not in json.dumps(data)
            assert h.fetch('/healthz')[0] == 200
        start = len(FakeCA.requests)
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            work = [pool.submit(h.api,'GET','trust/status') for _ in range(2)]
            deadline = time.monotonic()+1
            while len(FakeCA.requests) < start+2 and time.monotonic() < deadline:
                time.sleep(.01)
            assert h.api('GET','trust/status')[0] == 503
            assert all(f.result()[0] == 502 for f in work)
        assert not any(path == '/private-key-export' for path,_ in FakeCA.requests)
        FakeCA.mode = 'ok'
        h.stop()
        # Untrusted TLS is rejected even with otherwise valid credentials.
        h.config.write_text(text.replace(f'\nca_certificate = "{cert}"\n','\n'))
        h.start(env)
        assert h.api('GET','trust/status')[0] == 502
        h.stop()
        h.config.write_text(text)
        ca.shutdown()
        ca.server_close()
        h.start(env)
        assert h.fetch('/healthz')[0] == 200
        assert h.api('GET','trust/status')[0] == 502
        h.stop()
        h.config.write_text(text.replace('[xxc_trust]\nenabled = true','[xxc_trust]\nenabled = false'))
        h.start()
        assert h.api('GET','trust/status') == (200, {'enabled':False,'connected':False})
        assert h.api('GET','trust/certificates')[0] == 503
        h.stop()
        h.config.write_text(text)
        # Fail before serving if a configured credential is absent.
        missing = {**env,'CREDENTIALS_DIRECTORY':str(root/'missing')}
        args = [str(DAEMON),'--config',str(h.config),'serve']
        if os.geteuid() == 0:
            args.append('--allow-root')
        failure = subprocess.run(args,env=missing,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=15)
        assert failure.returncode != 0 and b'Cannot open XXC Trust token credential' in failure.stderr
        assert TOKEN.encode() not in failure.stderr
        for path in [root/'process.log',root/'aptd.log',root/'audit.log']:
            if path.exists():
                assert TOKEN.encode() not in path.read_bytes() and OMITTED.encode() not in path.read_bytes()
    finally:
        h.stop()
        ca.shutdown()
        ca.server_close()


def main():
    with tempfile.TemporaryDirectory(prefix='xxc-trust-e2e-') as temp:
        root=Path(temp)
        h=Harness(root)
        try:
            exercise(h,root)
        finally:
            h.stop()
            run(['gpgconf','--homedir',h.keys,'--kill','gpg-agent'])
    print('XXC Trust and wildcard HTTP: TLS, credentials, projections, roles, CSRF, bounds, outages and CLI passed')


if __name__ == '__main__':
    main()
