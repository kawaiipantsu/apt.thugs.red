# XXC Trust integration

XXC Trust provides X.509 infrastructure certificates. XXC-APTD can read its
CA, template and certificate inventory through an authenticated HTTPS API.
API 1.1 also supplies OpenPGP key custody and Debian Release signing, supported
by the `xxc-trust` signer in [SIGNING.md](SIGNING.md). X.509 certificates remain
separate. This integration does not issue/revoke X.509 certificates, export
private keys or authenticate users through a proxy. The existing [nginx mTLS topology](REVERSE-PROXY.md) remains separate.

The adapter implements the documented `/api/v1/authorities`, `/templates` and
paginated `/certificates` resources. Select an API credential with read access
to the intended inventory. OpenPGP signing uses `openpgp.read` and `openpgp.sign`; explicit remote key
generation additionally needs `openpgp.manage`. No private-export scope is needed.
Consult the CA service's `/developers` and `/assets/openapi.yaml` for its API
contract and permission model.

## Server configuration

As root on the repository server, create a private credential directory:

```sh
install -d -m 0700 /etc/xxc/credentials
```

Provision `/etc/xxc/credentials/xxc-trust-token` using a secret manager or a
private editor. Set ownership to root and mode 0600. Do not paste the token into
shell commands, normal TOML, source control, issue reports or service logs.
Ensure the service can traverse `/etc/xxc` and read its configuration; keep the
credentials subdirectory mode 0700. Use `systemctl edit xxc-aptd` to add:

```ini
[Service]
LoadCredential=xxc-trust-token:/etc/xxc/credentials/xxc-trust-token
```

Edit `/etc/xxc/aptd.conf`, replacing the reserved API URL with the service's
actual HTTPS API origin:

```toml
[xxc_trust]
enabled = true
api_url = "https://ca.example.invalid/api/v1"
token_credential = "xxc-trust-token"
authority_id = ""
template_id = ""
request_timeout_seconds = 10
max_response_bytes = 1048576
# ca_certificate = "/etc/xxc/trust-api-ca.pem"
```

`authority_id` and `template_id` are optional 32-character lowercase hex IDs.
Status checks that configured selections exist, and that the authority is
active. `authority_id` also filters certificate inventory. `template_id` records
the intended template; it does not issue certificates or filter inventory.

The optional CA file contains only a public PEM root for an internal HTTPS CA.
System roots remain enabled. Certificate verification cannot be disabled.
The Debian package depends on `ca-certificates` for system trust roots.

As root:

```sh
xxc-aptd config check
systemctl daemon-reload
systemctl restart xxc-aptd
xxc-apt-cli trust status
xxc-apt-cli trust authorities
xxc-apt-cli trust templates
xxc-apt-cli trust certificates --page 1 --status active
```

The CLI goes through the existing local socket. An administrator can also visit
`/admin/trust`. Certificate inventory may include private infrastructure names;
only administrator accounts and authorized socket users can read inventory.
Operators can trigger reviewed repository publication with the preconfigured
signer; they cannot browse/generate remote keys or change the fingerprint. API fields
are projected explicitly; owners, CSRs and private material are excluded.

## Failures and rotation

Enabled integration requires a valid credential at startup. Missing credentials,
unsafe permissions or malformed local configuration fail before serving.
The daemon does not contact the CA during startup. Remote outages leave APT
serving and offline verification available. New remote signatures and inventory
requests fail safely; local GPG signing is independent of CA availability.

Errors include `trust_credential_rejected`, `trust_rate_limited`,
`trust_redirect_rejected`, `trust_response_too_large`, `trust_invalid_response`,
`trust_unavailable`, `trust_busy`, `trust_invalid_query`, `trust_disabled`,
`trust_key_not_found`, `trust_key_unavailable`, `trust_request_too_large` and
`trust_signer_mismatch`.
Unauthorized/forbidden upstream replies never forward the remote body.
If a legitimate inventory exceeds the configured response cap, increase the cap
within its documented bound after checking expected inventory size.

For rotation, provision the replacement token into a mode 0600 temporary file,
atomically replace the credential file, restart the service, and verify
`trust status`. Revoke the old credential after the replacement works. The daemon
reads credentials once; editing the file does not alter a running process.
No web action writes credentials or `/etc/xxc/aptd.conf`.

Systemd may deliver credentials as root-owned mode 0440 files with an ACL for
this service. That format is accepted only on a read-only mount within a
root-owned mode 0550 credential directory. Ordinary credential source files
must remain owner-only. The systemd integration test covers this delivery mode.
