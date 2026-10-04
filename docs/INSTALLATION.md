# Installation

This development slice is for staging and evaluation. The production 1.0 gates
in ROADMAP.md are not complete. Do not switch the existing live archive yet.

## Debian package (repository server, root)

Build with `make deb` on the documented Debian development environment, then:

```sh
sudo apt install ./dist/xxc-aptd_*.deb
sudo xxc-aptd config check
sudo systemctl enable --now xxc-aptd
sudo xxc-apt-cli status
```

Installation creates the service user, configuration, directories and log files.
It never generates a signing key. No daemon restart is needed to initialize an
empty archive; configure signing and restart before publishing. Add operators to
`xxc-aptd-admin` only if they should have full repository management authority.

## Manual installation (repository server)

```sh
make build-release man
sudo make install
sudo /usr/sbin/xxc-aptd init --system
sudo systemctl daemon-reload
sudo systemctl enable --now xxc-aptd
```

`make install` supports DESTDIR, PREFIX, SYSCONFDIR and LOCALSTATEDIR. Default
configuration remains targeted at standard Debian paths; custom prefixes require
explicit configuration/unit review. Install and uninstall preserve existing
configuration, state and keys. `make uninstall` removes shipped executables and
integration files; stop the service first. There is no destructive purge target.

## Existing apt.thugs.red migration

Back up the existing archive and signing keys first. Work on an isolated host or
ports. Import the existing private archive key into the restricted service key
directory through a local root-controlled process, never through public HTTP.
Set the full existing fingerprint. Upload, inspect and stage each binary package.
Publish, verify, and run an isolated APT client using the existing source line.
The test suite checks `/repo`, `zerotrust main` and the legacy public-key URL.

Preserve reverse-proxy routing and the existing trusted public fingerprint.
Actual migration of current production packages/key material and traffic cutover
are separate, unperformed acceptance steps. See KEY-ROTATION.md if trust changes.

## First web administrator

On the server as root or an authorized socket-group member:

```sh
xxc-apt-cli user add archive-admin --role administrator
```

Configure `[admin].external_url` and the separate TLS reverse proxy, then open
`/admin/login`. Password entry is interactive. No account or signing key is
created or replaced during an upgrade. See [ADMINISTRATION.md](ADMINISTRATION.md).

## Direct testing on a trusted network

On the server, edit the existing sections of `/etc/xxc/aptd.conf`. Use the
server's reachable address in place of this documentation-only example:

```toml
[server]
public_listen = "0.0.0.0:8088"
admin_listen = "0.0.0.0:8089"
external_url = "http://192.0.2.10:8088"
# Keep the remaining server fields.

[admin]
allow_remote = true
external_url = "http://192.0.2.10:8089"
# Keep the remaining admin fields.
```

As root, validate and restart:

```sh
xxc-aptd config check
systemctl restart xxc-aptd
xxc-apt-cli status
```

Open the public origin and the administrative origin plus `/admin/`. Both URLs
can be configured independently: an HTTPS admin origin enables Secure admin
cookies, while the public origin can use its own HTTPS reverse proxy. Bind
addresses and external origins have different purposes;
`0.0.0.0` is never a browser URL. Authentication, Host checks and CSRF stay
mandatory. Limit access through network policy; normal deployment uses the
TLS reverse proxy. IPv6 `[::]:PORT` is also supported with the same admin opt-in.

Enable optional infrastructure inventory using [XXC-TRUST.md](XXC-TRUST.md).
