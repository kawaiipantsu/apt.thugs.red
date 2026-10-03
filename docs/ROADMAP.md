# Roadmap

Version 0.1.0 implements the repository and authenticated administration slices. The supplied banner
is preserved, and the source and wiki remotes are configured separately.
Checked items have implementation and passing local tests. They do not imply
production 1.0 acceptance. See [VERIFICATION.md](VERIFICATION.md) for evidence.

## 0.1 — Foundation and first vertical slice

- [x] Five-crate Cargo workspace, strict config, logging and idempotent initialization
- [x] Daemon lifecycle, separate HTTP listeners and Unix CLI transport
- [x] Public streaming HTTP with HEAD, ranges, conditionals and path protection
- [x] SQLite migration, WAL, durable jobs/audit and FTS search
- [x] Hardened systemd unit exercised as an unprivileged transient service
- [x] Install/uninstall preserve configuration, repository bytes and keys
- [x] Upload, inspect, stage, signed atomic publish and isolated real APT update/download
- [x] Local Debian package installation, dedicated-account service, remote-bind login and live CA inventory verification
- [ ] Repeat packaged deployment and full upgrade acceptance in a clean VM

## 0.2 — Repository engine

- [x] Immutable pool, source-aware naming and architecture all
- [x] Packages/gzip/xz, Release, OpenPGP signatures and verification
- [x] Atomic generations, restart reconciliation, retained by-hash and rollback
- [x] Active-manifest reindex and full active-generation checksum/signature verification
- [ ] Reconstruct full retained history and import third-party archive metadata
- [x] Reviewed additive publication diff with Debian version comparisons, architecture/size changes and stale-review rejection
- [ ] Multi-suite lifecycle, package removal and replacement review
- [ ] Contents indexes and periodic Valid-Until refresh
- [ ] Audited conflicting-version override policy
- [ ] Source packages, .udeb, .dsc, .changes, .buildinfo and source tarballs
- [ ] Optional standalone .deb detached signatures (separate from normal APT archive authentication)

## 0.3 — Public UI

- [x] THUGS(red) theme, original favicon, home and setup instructions
- [x] Directory navigation, package search/detail and 50-record pagination
- [x] Primary-route sitemap and robots.txt
- [x] Command copy enhancement; core navigation works without JavaScript
- [x] Desktop/mobile browser, keyboard and automated WCAG checks on core public pages
- [ ] Directory sorting/filtering, timestamps, checksums and formatted metadata views
- [ ] Debian version ordering and complete retained-version detail pagination
- [ ] Package sitemap shards and large-repository performance tests

## 0.4 — Admin

- [x] Local socket API, CLI upload/stage/publish/rollback, jobs and audit inspection
- [x] Administrative HTTP requires authentication; proxy identity headers never authenticate
- [x] Users, Argon2id authentication, sessions, RBAC, durable login throttling and CSRF
- [x] Local user CLI/API, last-administrator protection and session revocation
- [x] Dashboard, upload, staging, reviewed publish, jobs, rollback, audit, users and read-only configuration/signing pages
- [x] No-JavaScript admin publication, mobile/keyboard and automated WCAG browser tests
- [ ] Full operational dashboard metrics, managed signing controls and removal UI
- [ ] Complete CLI/API command set, OpenAPI schemas and remote authenticated transport
- [x] HTTP audit actor IDs, transactional account/job auditing and account-change metadata
- [ ] Individual Unix peer identities and complete package lifecycle before/after audit metadata
- [x] Local GPG and XXC Trust OpenPGP signers with pinned fingerprints and independent local verification
- [x] Remote key list/show/generate/public-export/verify CLI/API and administrator key UI
- [ ] Private-key import, managed activation/overlapping rotation and additional signer backends

## 0.5 — Operations

- [x] Background publish jobs, single publisher and durable job results
- [x] Verified rollback CLI/API and interruption/restart tests
- [x] Reverse proxy and XXC Trust mTLS topology documentation
- [x] Explicit authenticated wildcard/non-loopback listener opt-in with Host/Origin/CSRF tests
- [x] Read-only XXC Trust API/CLI/admin inventory with systemd credentials, verified TLS and failure isolation tests
- [ ] XXC Trust CSR issuance/revocation, certificate lifecycle automation and proxy identity login
- [ ] Retention, reference-aware dry-run GC and audited deletion
- [ ] Metrics, comprehensive health and doctor
- [ ] Backup/restore rehearsal and complete corruption recovery
- [ ] Watchdog/readiness notification and optional reload
- [ ] Disk quotas, stale scratch cleanup and safe job cancellation

## 0.6 — Packaging and release quality

- [x] Debian binary package build and lintian error gate
- [x] Man pages, version consistency, Conventional Commit bump selection and wiki generation
- [x] Local CI covering Rust, APT, UI, dependency audit, docs and packaging
- [x] MIT license and shipped dependency license notices, including verified supplemental workspace notices
- [x] Transactional schema upgrade with private recovery backup and failure rollback tests
- [x] GitHub CI/release workflow and configurable release discussion tooling
- [ ] Tagged GitHub release workflow exercised against a reviewed source commit
- [ ] Package install/upgrade tests retaining config, keys, users, DB and repository
- [ ] Reproducible/offline source-package builds and vendored dependency supply chain

## 1.0 — Production replacement

- [ ] Actual existing apt.thugs.red packages/key migration and traffic cutover rehearsed
- [ ] Existing key continuity / overlapping rotation tested with client fleet
- [x] Fixture APT compatibility with /repo, zerotrust main and legacy public-key URL
- [x] Publication interruption, failed signer, concurrent-job rejection and rollback tests
- [ ] Polished public and authenticated admin UIs meeting the complete specification
- [ ] Unprivileged packaged service and upgrades validated on a clean Debian host
- [ ] Offline backup/restore and database upgrade recovery tested
- [ ] Independent security review completed
- [ ] Full production acceptance suite and all documentation complete
