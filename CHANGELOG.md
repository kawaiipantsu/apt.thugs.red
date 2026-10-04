# Changelog

## Unreleased

This development release adds automation, suite management and repository traffic
statistics. It preserves `/repo`, `zerotrust main` and the legacy public-key URL.
Production 1.0 acceptance remains in progress; see the
[roadmap](https://github.com/kawaiipantsu/apt.thugs.red/blob/main/docs/ROADMAP.md).

- Publish a public /api integration guide, Markdown/OpenAPI downloads, executable CI example and coding-agent checklist.
- Add scoped, expiring project API tokens with one-time UI creation and revocation.
- Add suite membership, scoped CLI/API/UI workflows, per-suite signed metadata and atomic archive-wide rollback.
- Add private traffic charts, package/asset rankings, client estimates and configurable trusted proxy CIDRs/retention.
- Mount admin assets and APIs beneath /admin for a single upstream proxy location.
- Reserve scrollbar space to prevent horizontal layout shifts.
- Select Secure admin cookies from the admin origin so public HTTPS can coexist with explicit LAN HTTP administration.
- Document proxy cutover origin validation and static-asset 400/MIME troubleshooting.
- Publish exact-version Debian artifacts with checksums and clean commit metadata; retain historical documentation during version bumps.
- Fix clean Debian CI bootstrap certificate/Git prerequisites and run all local quality gates before tagged publication.

Install the amd64 package on Debian 13 using `sudo apt install ./xxc-aptd_0.2.0-1_amd64.deb`.
Back up configuration, signing credentials and state before upgrading. Startup
migrates existing management databases to schema 4 with private recovery backups;
configuration, users, keys and published generations are retained. Additional suites
are configured explicitly. See [installation](https://github.com/kawaiipantsu/apt.thugs.red/blob/main/docs/INSTALLATION.md),
[automation](https://github.com/kawaiipantsu/apt.thugs.red/blob/main/docs/AUTOMATION.md)
and the [screenshot gallery](https://github.com/kawaiipantsu/apt.thugs.red/blob/main/docs/SCREENSHOTS.md).

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
- Add a public/admin/mobile screenshot gallery and isolated `make screenshots` capture workflow.
