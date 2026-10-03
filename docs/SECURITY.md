# Security model

Read this with ARCHITECTURE.md before changing trust boundaries. Production
deployment is gated on the outstanding security work in ROADMAP.md.

Public and administrative HTTP use separate routers and ports. The public
listener cannot dispatch management API operations. Administrative HTTP requires
a local session and an allowed role. Unix socket access confers administrator authority;
give membership of its group only to trusted operators. Proxy headers do not
authenticate requests. TLS termination belongs exclusively to nginx or Caddy.

Public files are selected from allowed repository paths after percent decoding,
rejecting hidden components, traversal, backslashes and escaping symlinks.
Private state, keys, upload files and generation internals have no public route.
The service account is trusted to manage the archive filesystem; another local
account must never have write permission there. The server is not a sandbox
against a compromised service account or root.

OpenPGP signs APT metadata. XXC Trust has distinct X.509 infrastructure and
OpenPGP key/signing APIs. X.509 certificates do not replace APT signatures. Select a full fingerprint explicitly. Keep key
directories mode 0700 and private key files mode 0600. Never return private key
material, passwords, credentials or session tokens through diagnostics or logs.
Public keys and fingerprints are intentionally distributable.

Commands use direct argv, timeouts and bounded captured output. Package bytes
are untrusted: metadata parsers, name validation, upload limits and immutable
pool checks apply before publication. No package scripts are executed during
ingestion. Signing is verified before switching the generation pointer.

Report vulnerabilities privately using the source repository's Security tab.
Do not post credentials, private keys, personal identifiers or host details in
public issues. Dependency auditing and independent security review are release
gates, not substitutes for careful design.

## Local authentication boundary

Passwords use Argon2id with a unique random salt, 19 MiB memory, two iterations
and one lane. Two concurrent password operations bound resource use. Sessions
use OS-generated 256-bit random tokens, SHA-256 token storage, absolute expiry
and a per-user cap. A security revision prevents a concurrent password reset
from completing an obsolete login. Account changes revoke stored sessions.
Requests already authorized before a change may complete.

Login challenges are single-use and short-lived. HTTP mutations require a
same-origin CSRF token and the exact configured Origin. Cookies are HttpOnly,
SameSite=Strict and Secure for HTTPS; no Domain attribute is used. The
same-origin Referrer-Policy preserves browser Origin on ordinary form POSTs
without sending referrers to other origins. No identity or client-IP forwarding
headers are trusted. The public router never mounts login/session handlers.

Viewer, operator and administrator permission checks apply at the HTTP boundary
and in mutation services. The socket installs an administrator principal only
on its own router. Password/user operations audit safe role/enabled changes in
the same database transaction. Jobs carry the authenticated actor through to
completion; session tokens, CSRF tokens and hashes are not audit metadata.

See [ADMINISTRATION.md](ADMINISTRATION.md) for limits and revocation behavior.
Design references: [OWASP session management](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html)
and [OWASP CSRF prevention](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html).
Independent security review remains a production gate.

## XXC Trust and remote listener opt-in

CA credentials are delivered outside normal configuration through systemd
LoadCredential. Token source files must be owner-only regular files owned by root or the
service account; symlinks and oversized/invalid values fail startup. Outbound
HTTPS verifies certificates, rejects redirects, ignores ambient proxy variables,
limits concurrency to two requests and bounds time/response size. Only projected
public certificate/key metadata is returned, to administrators. Remote error bodies
are discarded and never logged. Inventory names/SANs can reveal infrastructure;
protect administrator access accordingly. A CA outage does not stop APT serving.

Non-loopback administrative listening requires `admin.allow_remote=true` and
retains authentication/CSRF. Direct HTTP test sessions are suitable only on a
trusted network; deploy TLS at a reverse proxy for normal use. External URLs
must identify the actual browser origins and cannot be wildcard bind addresses.

Systemd may deliver credentials as root-owned mode 0440 files with an ACL for
this service. That format is accepted only on a read-only mount within a
root-owned mode 0550 credential directory. Ordinary credential source files
must remain owner-only. The systemd integration test covers this delivery mode.

## Remote signing boundary

The configured remote key ID and full fingerprint are pinned independently of
remote response filenames or identity labels. The CA must supply an active
signing-capable stored key and public material matching the pin. Private packets
are rejected with GPG show-only parsing before import. Normalized public exports
and local signature verification are required before generation activation.
Returned artifact names are exact allowlisted constants. Private keys never need
to leave the CA. The token itself confers the remote scopes granted by that CA;
restrict them to read/sign and optionally manage for explicit key generation.

Operators can publish reviewed repository metadata with the configured signer.
Only administrators can inspect or generate remote keys. No endpoint signs
arbitrary submitted content, rewrites the signer configuration or exports private
material. Key generation does not activate or publish it. Audit records omit
public user identity fields and upstream request/response bodies.
