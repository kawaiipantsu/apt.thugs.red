"""Disposable TLS reverse proxy: /admin stays intact and goes to the admin port."""
import http.client
from http.server import BaseHTTPRequestHandler
from trust_e2e import tls_server


def start_proxy(root, h):
    class Proxy(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def forward(self):
            path = self.path.split('?', 1)[0]
            port = h.admin_port if path == '/admin' or path.startswith('/admin/') else h.port
            connection = http.client.HTTPConnection('127.0.0.1', port, timeout=60)
            headers = {k: v for k, v in self.headers.items() if k.lower() not in ('connection', 'transfer-encoding')}
            headers['X-Forwarded-For'] = self.client_address[0]
            headers['X-Forwarded-Proto'] = 'https'
            body = self.rfile.read(int(self.headers.get('Content-Length', '0')))
            connection.request(self.command, self.path, body=body, headers=headers)
            response = connection.getresponse()
            data = response.read()
            self.send_response(response.status)
            for name, value in response.getheaders():
                if name.lower() not in ('connection', 'transfer-encoding', 'content-length'):
                    self.send_header(name, value)
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            if self.command != 'HEAD':
                self.wfile.write(data)
            connection.close()

        do_GET = do_POST = do_HEAD = do_DELETE = forward

    root.mkdir()
    return tls_server(root, Proxy)
