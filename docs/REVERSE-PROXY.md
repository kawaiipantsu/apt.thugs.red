# Reverse proxies and XXC Trust

The daemon always serves HTTP. TLS, certificates and HSTS belong to the proxy.
These examples use the intentionally public project domains from the specification.
Replace deployment-specific certificate paths locally; do not commit credentials.

## Configure the public origin first

On the repository server, update the existing `[server]` section in
`/etc/xxc/aptd.conf` before switching proxy traffic:

```toml
[server]
external_url = "https://apt.thugs.red"
# Keep the existing listener, socket and upload settings.
```

Validate and restart as root:

```sh
xxc-aptd config check
systemctl restart xxc-aptd
curl -I -H 'Host: apt.thugs.red' http://127.0.0.1:8088/static/site.js
curl -I https://apt.thugs.red/static/site.js
```

The script response must be 200 with `Content-Type: text/javascript`. If it is
400 with `text/html`, check `server.external_url` and the proxy's Host header.
The browser's subsequent nosniff/MIME error is caused by the HTML error response;
keep `X-Content-Type-Options: nosniff` enabled. The same origin controls generated
APT source definitions and setup commands. The separate admin origin and its
cookie policy remain configured under `[admin]`.

## nginx public server

```nginx
server {
    listen 443 ssl;
    server_name apt.thugs.red;
    ssl_certificate /etc/nginx/tls/public-chain.pem;
    ssl_certificate_key /etc/nginx/tls/public-key.pem;
    add_header Strict-Transport-Security "max-age=31536000" always;
    location / {
        proxy_pass http://127.0.0.1:8088;
        proxy_set_header Host apt.thugs.red;
        proxy_set_header X-Forwarded-Proto https;
        proxy_set_header X-Forwarded-For $remote_addr;
        proxy_read_timeout 300s;
        proxy_buffering off;
    }
}
```

Honor upstream Cache-Control. Pool/by-hash objects are immutable; Release and
moving indexes use no-cache. Do not enable an unconditional cache over `/repo`.
Public uploads are not accepted. Keep Range, If-None-Match and If-Modified-Since
headers intact. Set redirects from port 80 and HSTS according to your domain policy.

## Caddy public server

```caddyfile
apt.thugs.red {
    header Strict-Transport-Security "max-age=31536000"
    reverse_proxy 127.0.0.1:8088 {
        header_up Host apt.thugs.red
    }
}
```

## Administrative nginx with XXC Trust client certificates

XXC Trust at `https://ca.xxc.dk` is an X.509 identity service. Obtain the client
CA chain and administrator client certificates using its operator-managed
workflow. Do not invent an APT signing certificate or exchange OpenPGP trust for
an X.509 certificate. Read-only XXC Trust inventory uses systemd credentials; see
[XXC-TRUST.md](XXC-TRUST.md). Issuance and proxy identity login remain future
work. No API token belongs in aptd.conf.

Set `[admin].external_url = "https://admin.apt.thugs.red"` and bootstrap a local
administrator through the Unix socket before using this topology. Successful
mTLS admits the connection; the application then requires its own login.
Keep Origin and Cookie headers intact, never cache admin responses, and forward
Host exactly as configured. Login throttling uses the immediate proxy IP;
add a proxy per-client rate limit for deployments with many administrators.

```nginx
server {
    listen 443 ssl;
    server_name admin.apt.thugs.red;
    ssl_certificate /etc/nginx/tls/admin-chain.pem;
    ssl_certificate_key /etc/nginx/tls/admin-key.pem;
    ssl_client_certificate /etc/nginx/tls/xxc-trust-client-ca.pem;
    ssl_verify_client on;
    ssl_verify_depth 3;
    client_max_body_size 2g;
    client_body_timeout 900s;
    location / {
        proxy_pass http://127.0.0.1:8089;
        proxy_set_header Host admin.apt.thugs.red;
        proxy_set_header X-Forwarded-Proto https;
        proxy_set_header X-Forwarded-For $remote_addr;
        proxy_set_header X-Remote-User "";
        proxy_request_buffering off;
        proxy_read_timeout 960s;
    }
}
```

mTLS is an additional control. It does not authorize an arbitrary identity header
inside this daemon. All proxy-authentication headers are ignored. Future header
support must restrict the immediate peer to explicitly configured proxy networks
and retain application RBAC. Never expose the raw administrative listener publicly.

The public daemon rejects unknown Host names. Preserve the configured external
hostname as shown above; forwarded host headers cannot override this validation.

## Public site and administration on one hostname

Route the whole `/admin` namespace to port 8089. Keep that prefix intact: nginx
`proxy_pass` must have no trailing slash; Caddy must use `handle`, not `handle_path`.
Administrative scripts, CSS, favicon and JSON calls all live under that prefix.
Keep both listeners separate inside the daemon.

Once the proxy route is ready, set `[admin].external_url = "https://apt.thugs.red"`
on the server, validate and restart. Do not append `/admin` to the origin.
This enables Secure cookies and requires HTTPS Origin on browser mutations.
Until the route is ready, retain the existing LAN origin for direct testing.
Sessions use host-only cookies with Path=/; public handlers never interpret them.
A separate admin hostname remains supported and provides stronger browser isolation.

Add inside the public nginx server block above:

```nginx
location = /admin { return 308 /admin/; }
location ^~ /admin/ {
    proxy_pass http://127.0.0.1:8089;
    proxy_set_header Host apt.thugs.red;
    proxy_set_header X-Forwarded-Proto https;
    proxy_set_header X-Forwarded-For $remote_addr;
    proxy_set_header X-Remote-User "";
    client_max_body_size 2g;
    client_body_timeout 900s;
    proxy_read_timeout 960s;
    proxy_request_buffering off;
    proxy_cache off;
}
```

For Caddy, replace the public reverse_proxy block with:

```caddyfile
@admin path /admin /admin/*
handle @admin {
    request_body { max_size 2GB }
    reverse_proxy 127.0.0.1:8089 {
        header_up Host apt.thugs.red
    }
}
handle {
    reverse_proxy 127.0.0.1:8088 {
        header_up Host apt.thugs.red
    }
}
```

Keep Authorization intact for project API tokens and never cache administrative
responses. Apply mTLS/SSO controls at the proxy if required; automation clients must
satisfy those controls too. Application sessions or scoped API tokens remain required.
For a proxy on another machine, replace loopback upstreams with the private backend
address, restrict network access, and configure the actual peer CIDRs under
`server.trusted_proxies` in aptd.conf. Never use a broad network merely to make
forwarding work. At the Internet-facing proxy, overwrite incoming X-Forwarded-For
with the verified peer address as above. In a controlled proxy chain, append only
after configuring that proxy's own trusted peers. The daemon walks from the
rightmost trusted peer to the first untrusted client address for analytics only.
