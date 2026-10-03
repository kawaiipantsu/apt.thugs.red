# Debian packaging

`make deb` uses `dpkg-buildpackage -us -uc -b` with debhelper, then collects binary
artifacts into `dist/`. Build prerequisites are in debian/control. Application
SemVer is independent of the Debian revision: `0.1.0` initially packages as
`0.1.0-1`. `make deb-revision` increments only the packaging revision.

The daemon installs to `/usr/sbin`, CLI to `/usr/bin`, configuration to `/etc/xxc`,
and systemd/sysusers/tmpfiles files to `/usr/lib`. Templates and assets are
embedded in the daemon. Man pages use reproducible scdoc input and deterministic
gzip. debhelper discovers ELF shared-library dependencies. The package depends
on Debian metadata/signing tools and never downloads a runtime Node dependency.

Postinst creates service accounts and missing paths without replacing existing
configuration or generating keys. `/etc/xxc/aptd.conf` is a dpkg conffile.
Automatic initial service start is disabled; the operator enables it after
reviewing configuration. Upgrades must preserve state and require the migration
tests on the roadmap before production release. There is no purge script that
deletes repository data or signing material.

Cargo uses the tracked lockfile. This initial packaging workflow fetches Cargo
dependencies when absent; offline Debian source-package vendoring and reproducible
build verification remain pending. The local workflow produces unsigned binary
packages; signing and distributing release artifacts is an explicit operation.

The package metadata currently uses a reserved example contact rather than
publishing private maintainer details. Set an approved project contact before a
public package release. The project is licensed under MIT.

Lintian's `initial-upload-closes-no-bugs` warning is expected for this independently
distributed first package: no Debian ITP bug is claimed. Do not invent a bug
number merely to suppress the warning. All lintian errors must be fixed.

## Supplemental upstream notices

`make licenses` collects the actual binary dependency graph's notices. The
published debversion 0.5.4 crate omits the workspace COPYING file. Its upstream
Apache-2.0 text is retained in `licenses/debversion-0.5.4/` with the exact source
commit, URL and SHA-256 in provenance.json. Collection verifies the locked
crate's name, version, declared license, source commit and notice hash. A newer
crate version needs its own reviewed notice if it still omits that file.
The lazy-regex-proc_macros 3.6.2 crate has the same workspace packaging omission;
its notice is copied from lazy-regex 3.6.2 at the identical Git commit and verified
by the same provenance checks. No build-time network download or generic license
substitution is used.
