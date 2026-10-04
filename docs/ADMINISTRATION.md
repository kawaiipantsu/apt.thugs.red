# Administrative accounts and web workflow

The administrative application is served at `/admin/` on the separate HTTP
listener, normally behind `https://admin.apt.thugs.red`. Authentication is
required on that listener even on loopback. Public APT routes never expose the
management API, users, sessions, configuration or audit records.

## Bootstrap on the repository server

As root, or an authorized member of `xxc-aptd-admin`:

```sh
xxc-apt-cli user add archive-admin --role administrator
xxc-apt-cli user list
```

The CLI prompts for a password twice without echo. Passwords must contain
12–1024 UTF-8 bytes. No password argument or environment variable is accepted.
Automation can feed a protected credential file to `--password-stdin`; keep the
file and its containing directory private and avoid shell tracing. The first
account must be an administrator. Socket access itself grants full authority;
HTTP roles do not restrict a person who can access that socket.

Set `[admin].external_url` in `/etc/xxc/aptd.conf` to the exact browser-facing
origin, then validate and restart. The default is
`https://admin.apt.thugs.red`. Follow [REVERSE-PROXY.md](REVERSE-PROXY.md).
The origin must use canonical lowercase ASCII spelling, no path/trailing slash,
and no explicit default port. Proxy headers cannot override it.

For an isolated local HTTP administrative interface, set
`[admin].external_url = "http://127.0.0.1:8089"`. Its scheme alone determines
whether administrative cookies require Secure transport. The public origin can
use HTTPS independently. Proxy headers cannot change cookie security. Keep normal
administrative deployments behind TLS; direct HTTP is for trusted-network testing.

## Roles

| Role | Allowed operations |
| --- | --- |
| viewer | Dashboard, packages, staging/review, generations, jobs, signing fingerprint, audit and basic operational health |
| operator | Viewer access plus upload, stage, publish, rollback, verify and reindex |
| administrator | Operator access plus local users and effective configuration |

Sign in at `/admin/login`. The dashboard links to all available pages.
Users and settings links appear only for administrators. Signing shows the
configured fingerprint and public key location; key generation/import/activation
remain server-side GPG operations until managed key controls are implemented.

## Publish through the browser

1. Open **uploads**, select one `.deb` and choose **upload and inspect**.
2. Inspect the package's metadata, architecture, checksums and pool path.
3. Choose **stage** for the inspected record.
4. Open **publish** and review additions, version comparisons, architecture
   changes and the size delta. Existing versions remain selected.
5. Choose **publish reviewed changes**. The job page shows the final result.

The workflow works without JavaScript. JavaScript polls job status when enabled;
without it, use **refresh job result**. Uploads stream to a private temporary file
and share the API's inspection pipeline. A failed upload removes scratch data.
The multipart form requires the CSRF field before package bytes. Browser-supplied
filenames never determine storage paths. Package content is not executed.

The publication review token binds the current generation, selected package
records and repository/signing settings. Staging or rollback after review makes
that token stale; publication returns 409 and requires another review. The
workflow currently adds versions. Removal and conflicting-version overrides are
future work. An older version appears in the comparison as an older addition;
APT's preferred version may remain the existing higher version.

**Releases** offers an explicit restore action for each retained generation.
Rollback verifies signatures/checksums and queues a job. Check its result and
metadata validity before announcing a rollback.

## Accounts and sessions

The **users** page creates accounts and changes roles, passwords or enabled state.
Equivalent local commands, with IDs from `user list`:

```sh
xxc-apt-cli user add archive-reader --role viewer
xxc-apt-cli user role USER_ID operator
xxc-apt-cli user passwd USER_ID
xxc-apt-cli user disable USER_ID
xxc-apt-cli user enable USER_ID
xxc-apt-cli user delete USER_ID
```

Password resets, role changes, disable/enable and deletion revoke all sessions
for that account. The last enabled administrator cannot be demoted, disabled or
deleted. Commands cannot create more than 1,000 local users. Existing requests
already authorized before revocation may finish; subsequent requests fail.

Sessions expire after the configured absolute lifetime (default 12 hours).
There is no sliding renewal. At most eight sessions per user are retained by
default; an additional login evicts the oldest. Signing out revokes the current
session. Authentication tokens are random 256-bit values stored only as SHA-256
hashes. Cookies use HttpOnly, SameSite=Strict, Path=/ and Secure for HTTPS origins;
HTTPS cookies also use the `__Host-` prefix and never set Domain.

Every HTTP mutation requires the exact admin Origin and a CSRF token. Login uses
a separate, single-use challenge that expires after ten minutes. Rate limits
count attempts before verification, including nonexistent users: ten attempts
per account per 15 minutes by default, and five times that allowance per immediate
peer IP. Forwarded IP/identity headers are ignored. A reverse proxy therefore
shares its peer allowance among clients; add an upstream per-client limit when
needed. Two concurrent password operations cap hashing work. Challenges are
bounded at 4,096 and login-limit buckets at 10,000; exhaustion fails closed.

Audit events identify HTTP users by opaque IDs and record the interface, action,
result and safe metadata. Passwords, password hashes, session cookies and CSRF
tokens must never be included in exported diagnostics. Database backups contain
authentication state and require restrictive permissions.

## XXC Trust inventory

Administrators can open `/admin/trust` for connection status, authorities,
templates and paginated certificate inventory. Search and navigation work
without JavaScript. Viewer/operator roles cannot access this inventory. CA
private material is never exposed. See [XXC-TRUST.md](XXC-TRUST.md) for setup.
Non-loopback test access requires the explicit opt-in in [INSTALLATION.md](INSTALLATION.md).

## OpenPGP keys and remote publication

Administrators can use `/admin/keys` to list, search, generate and export public
keys held by XXC Trust. Generation clearly identifies the name/email as public
key identity. It leaves activation to a root-managed configuration change.
`/admin/signing` shows the configured backend, remote ID and fingerprint.
Operators publish through the same reviewed generation workflow with either
backend. See [SIGNING.md](SIGNING.md) for scopes, limits and offline verification.

## Suites, project tokens and statistics

Upload, package, staging and review pages offer a suite selector and links that
carry the selected channel through the workflow. Review displays only that suite's
changes; publication regenerates the complete signed archive atomically. Rollback
restores all suites. Existing package versions remain available.

Administrators manage scoped CI credentials at `/admin/tokens`. The secret is
shown once; token metadata and revocation remain available afterward. See
[AUTOMATION.md](AUTOMATION.md) for expiry, scope rules and a complete curl pipeline.
The dashboard includes private traffic charts and rankings described in
[ANALYTICS.md](ANALYTICS.md). Counts begin when collection is enabled, with no
invented historical data.

All browser assets and API calls use `/admin/static`, `/admin/favicon.svg` and
`/admin/api/v1`. See [REVERSE-PROXY.md](REVERSE-PROXY.md) for a single-host path proxy.
Keep the current LAN admin origin until that proxy route is operational, then set
`admin.external_url` to the HTTPS origin without `/admin` and restart.
