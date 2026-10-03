# Verification record — repository and administration slices

Local evaluation date: 2026-10-03. Environment: Debian 13, Rust/Cargo 1.94.1,
systemd, Debian APT/dpkg/GPG tools and Chromium. The repository began without
application code or history; the supplied README banner was retained.

## Passing checks

| Check | Evidence and scope |
| --- | --- |
| Release compilation | `make build-release`; both daemon and CLI built |
| Full local CI | `make ci`: formatting, clippy with warnings denied, compile, tests, docs/version/wiki, browser, audit, Debian build and lintian error gate |
| Core/security tests | Strict/unknown config, idempotent init, control metadata injection, corrupt/future SQLite rejection, bounded child deadlines, FTS pagination, public/private separation and traversal |
| Real APT | Temporary OpenPGP key and .deb, API upload/stage/publish, key download, isolated signed `apt-get update`, package discovery, acquisition, SHA256 match and disposable extraction |
| Atomicity/recovery | Failed signer retains current generation; old files remain available during blocked publication; second publisher rejected; SIGKILL before activation preserves generation; restart records interrupted job failure |
| Repository protocol | Architecture all in amd64/arm64/all indexes, gzip/xz, SHA256/SHA512 by-hash, forced APT by-hash, old by-hash after publication, rollback and verification |
| HTTP | HEAD, byte ranges, ETag/If-None-Match, Last-Modified/If-Modified-Since, conditional precedence, unsafe Host rejection, ignored forwarded identity |
| XXC Trust | Disposable real HTTPS CA: safe field projections, CLI/admin views, role restrictions, escaped HTML, pagination, bad credentials, TLS rejection, redirect rejection, bounded responses/concurrency, timeout and outage isolation |
| Wildcard listening | Both listeners verified on 0.0.0.0 with explicit admin opt-in, valid HTTP sessions and retained Host/Origin/CSRF checks |
| Upload safety | Malformed control/payload archives, identity conflict, identical duplicate, oversized body and interrupted-upload scratch cleanup |
| Browser | Seven Playwright tests: public browsing/search, no-JavaScript admin upload/stage/review/publish/job completion, user creation/disable, viewer restrictions, CSRF rejection, desktop/mobile layout, keyboard focus, remote key generation/public export and axe WCAG checks |
| HTTP authentication | Unix CLI bootstrap, Argon2 login, login challenge/replay, cookies, roles, Host/Origin/CSRF and spoofed-proxy rejection, durable rate limits, logout/password/role/disable/expiry invalidation and disabled-listener behavior |
| Publication review | Debian epoch/tilde ordering, added records/size delta, token changes after staging/config changes and stale-token rejection before HTTP publication |
| Database upgrade | Schema 1 data preserved; mode 0600 pre-v2 backup remains readable at schema 1; incompatible migration rolls back; future schemas refused |
| Systemd | `make test-systemd`: shipped hardening in an isolated transient unit with an unprivileged account; upload, metadata generation, GPG signing, verification, public fetch and real systemd credential loading succeeded |
| Remote OpenPGP | TLS fixture keeps private keys outside daemon paths; real APT acquisition, pinned public-key checks, valid SHA-256 signatures, malicious artifact rejection, failed-signature atomicity, offline startup/verify/rollback and remote-key CLI/API checks |
| Live remote signing | Approved identity generated in the live CA, pinned in local configuration, first empty archive signed, isolated fixture APT update/download verified against live remote signatures, installed archive APT update passed; private keys stayed remote |
| Installed service | Debian package installed on the development host; enabled dedicated-account systemd service, wildcard public/admin HTTP, login/logout/CSRF and live CA inventory verified; credentials and deployment values kept outside source control |
| Maintenance | Conventional Commit bump classification, ambiguous-history rejection, repeated install/uninstall preserving config, keys, database and pool sentinels |
| Packaging | `dist/xxc-aptd_0.1.0-1_amd64.deb`, runtime dependencies, manuals, conffile, unit, sysusers/tmpfiles, logrotate and dependency notice sets with pinned supplemental upstream notices |
| Dependency audit | cargo-audit completed with no reported vulnerabilities; npm reported no vulnerabilities for browser-test dependencies |

Lintian reports `initial-upload-closes-no-bugs`. This independent initial package
does not claim a Debian ITP bug. There are no lintian errors. Building as root
also prints lintian's environment warning; it is not a package diagnostic.

Browser review artifacts are generated under `.build/ui/desktop.png` and
`.build/ui/mobile.png`; administration has matching `admin-desktop.png` and
`admin-mobile.png` artifacts. Screenshots are not committed. Test signing keys and packages
exist only in temporary test trees. Host APT sources and package state are not
used by the real APT acquisition test. The systemd test does not install the
production service.

## Not established by these tests

This is not production 1.0 acceptance. Real production key/package migration,
complete CLI/key management, trusted-proxy identity integration,
multi-suite changes, automatic expiry refresh, retention/GC, disk quotas,
package removal/replacement workflows, clean-VM package upgrade and offline restore tests,
reproducible offline builds and an independent security review remain outstanding.
Automated accessibility checks do not replace assistive-technology user testing.

GitHub repository metadata and the release discussion category are configured.
The workflow files have not yet run as remote Actions because this work is not
committed or pushed. No release announcement or wiki synchronization was sent.
