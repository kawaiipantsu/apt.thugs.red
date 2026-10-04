aptd.conf(5) "XXC-APTD 0.2.0"

# NAME

aptd.conf - authoritative XXC-APTD TOML configuration

# DESCRIPTION

The default file is /etc/xxc/aptd.conf. Unknown fields are errors. Every field in
the original schema is required; the admin and xxc_trust sections and their fields have defaults. Configuration is read-only during operation.
Validate with *xxc-aptd config check* or *xxc-apt-cli config validate*.
All changes require restart; reload and web editing are not implemented.

# SERVER

*public_listen*, *admin_listen*: HTTP socket addresses, including wildcard binds.
Non-loopback admin binds require admin.allow_remote=true.
*admin_socket*: absolute Unix socket path directly inside paths.runtime.
*external_url*: HTTP(S) origin without credentials, path, query or trailing slash. Use a concrete ASCII DNS name or IP, never 0.0.0.0 or [::].
Public Host must match that origin, the bind address or loopback.
*repository_prefix*: /repo in this release.
*max_upload_bytes*: 1 byte to 16 GiB; default 2147483648.
*upload_timeout_seconds*: 1 to 3600, default 900.
*max_concurrent_uploads*: 1 to 32, default 2.
*trusted_proxies*: up to 64 CIDRs, default empty. Only these immediate peers may
supply X-Forwarded-For for traffic statistics. Zero-prefix networks are rejected.
Forwarded identity headers never grant authentication.

# PATHS

*repository*, *staging*, *uploads*, *temporary*, *database*, *keys*, *runtime*:
absolute normalized paths without symlink ancestors. Private directories cannot
overlap repository paths. Staging, uploads and temporary must remain inside the
database parent tree. Keys and private state directories require mode 0700.

# REPOSITORY

*origin*, *label*, *suite*, *codename*, *components*, *architectures*, *description*,
*version*: Debian archive identity. Machine architectures exclude all, which is
included automatically in each binary index. The first component receives uploads.
*not_automatic*, *but_automatic_upgrades*: Release policy booleans.
*valid_until_seconds*: 3600 to 31536000, default 1209600.
*acquire_by_hash*: must be true in this release.
*suites*: optional list of up to 32 unique suite names including suite. Empty
means only the default suite. Additional suites use their name as codename.
Components, architectures, signer and Release policy are shared. Published
suites cannot be silently removed; archive migration is required.

# SIGNING

*backend*: gpg or xxc-trust. *fingerprint*: full 40 or 64 hexadecimal
fingerprint; an empty value is accepted only for gpg and disables publication.
*remote_key_id*: empty for gpg; a required 32-character lowercase hexadecimal
XXC Trust key ID for xxc-trust. That backend also requires xxc_trust.enabled.
Remote signatures are verified locally before atomic activation. Verification
and rollback use retained public snapshots without contacting the CA.
*public_ascii_name*, *public_keyring_name*: safe public filenames.
*command_timeout_seconds*: 1 to 1800, default 120.
Private key material is never a configuration value.

# WEB

*site_name*, *tagline*, *main_site_url*: escaped display text and HTTP(S) main site.
The public site uses locally embedded assets. CSS custom properties define colors.

# LOGGING

*level*: tracing filter. *file*, *audit_file*: absolute log paths.
*strict*: fail startup if the operational file cannot open. Otherwise stderr and
journald remain available. Audit metadata is durable in SQLite even if its file
stream is unavailable. Log files reopen for each event to support rotation.

# ADMIN

*enabled*: default true; false disables all administrative TCP routes. Unix
socket administration remains available. Authentication and CSRF are mandatory.
*allow_remote*: default false; explicitly permits non-loopback admin binds.
Authentication, roles, Host/Origin validation and CSRF remain required. Use a
trusted test network or an upstream TLS proxy; the daemon serves plain HTTP.
*external_url*: canonical browser-facing HTTP(S) origin, default
https://admin.apt.thugs.red. No path, credentials or explicit default port.
Host must match this authority or the bind address. Mutating Origin must match
exactly. Forwarded identity and client-IP headers are ignored.
*session_lifetime_seconds*: 300..604800, default 43200; absolute expiry.
*login_window_seconds*: 60..86400, default 900.
*login_max_attempts*: 1..100, default 10 per account/window; immediate peer IP
allowance is five times this limit.
*max_sessions_per_user*: 1..32, default 8; excess logins evict oldest sessions.
Administrative cookies are Secure when admin external_url uses HTTPS. The public
external_url and proxy headers do not change administrative cookie security.

# ANALYTICS

*enabled*: collect private aggregate public GET traffic, default true.
*retention_days*: 1..365, default 90. Raw client addresses, search queries,
cookies and user agents are not stored. Keyed address hashes estimate clients.
The admin dashboard offers 7/30/90-day windows and excludes upstream cache hits.
Changes require restart.

# XXC TRUST

The xxc_trust section enables X.509/OpenPGP inventory, explicit remote key
generation and optional OpenPGP Release signing. Key administration requires
administrator authority; operators may publish with the configured signer.
*enabled*: default false. *api_url*: HTTPS URL ending in /api/v1, without
credentials, query, fragment or trailing slash; replace the reserved example URL.
*token_credential*: systemd credential name, default xxc-trust-token, 1..128
ASCII letters, digits, hyphens or underscores. No path or bearer token here.
Use LoadCredential with an owner-only source file; systemd delivers it through
CREDENTIALS_DIRECTORY. Missing credentials fail startup when enabled.
*authority_id*, *template_id*: optional 32-character lowercase hexadecimal IDs.
Status checks their availability; authority_id also filters certificate inventory.
*ca_certificate*: optional absolute path to a public PEM trust root (128 KiB max).
System trust roots remain enabled. TLS verification cannot be disabled.
*request_timeout_seconds*: 1..60, default 10. Two concurrent requests maximum.
*max_response_bytes*: 1024..16777216, default 1048576. Redirects are rejected.
Upstream failure returns a bounded API error; public APT serving continues.
APT signatures use the OpenPGP API, separately from X.509. X.509 issuance,
revocation and all private-key exports are not implemented by this integration.


Systemd may deliver credentials as root-owned mode 0440 files with an ACL for
this service. That format is accepted only on a read-only mount within a
root-owned mode 0550 credential directory. Ordinary credential source files
must remain owner-only. The systemd integration test covers this delivery mode.

# UNIMPLEMENTED OPTIONS

Proxy identity authentication, metrics, scheduled refresh,
multi-suite configuration and automatic retention are
not accepted yet. They remain explicit roadmap work. Republish before Valid-Until.

# SEE ALSO

*xxc-aptd*(8), *xxc-apt-cli*(1)
