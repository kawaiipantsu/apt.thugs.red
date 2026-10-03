# Changelog

## 0.1.0

- Add a five-crate Rust workspace and strict, validated configuration.
- Add idempotent initialization, unprivileged service execution and split listeners.
- Stream uploads into quarantine; inspect .deb metadata with Debian tools.
- Stage packages and publish verified OpenPGP-signed atomic generations.
- Preserve old by-hash objects and support verified rollback and restart reconciliation.
- Add SQLite migrations, FTS package search, background jobs and audit records.
- Add the THUGS(red) public browser, package pages and client setup instructions.
- Add Unix-socket CLI/API, build/release tooling, operational docs and real APT tests.
- Add Argon2id local users, expiring/revocable sessions, RBAC, CSRF and login throttling.
- Add server-rendered admin upload/staging/publication/jobs, users, audit and settings pages.
- Bind publication to a reviewed diff; the CLI now requires --review-token.
- Back up existing SQLite state before the transactional administrative schema upgrade.
- Add user management CLI/API, per-user audit identity and HTTP/browser security tests.
- Validate payload archives, reject unsafe Host values and protect private key file modes.
- Include MIT project licensing and third-party Rust dependency notices.
- Support explicit authenticated remote admin binds and concrete browser-facing origins.
- Add administrator-only XXC Trust inventory via systemd credentials and verified outbound HTTPS.
- Add Trust CLI/API/UI, bounded CA requests and isolated TLS failure tests.
- Add XXC Trust OpenPGP custody and remote Debian Release signing with fingerprint pins and local verification.
- Add remote key generation/list/show/public-export/verify CLI/API and admin pages.
- Verify and roll back retained generations offline using isolated public-key snapshots.
