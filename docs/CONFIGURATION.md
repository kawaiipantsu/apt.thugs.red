# Configuration

`/etc/xxc/aptd.conf` is authoritative TOML. The complete supported configuration
is [config/aptd.conf.example](../config/aptd.conf.example). No field is silently
ignored: unknown fields fail validation. Missing legacy fields fail validation;
the `[admin]` and `[xxc_trust]` sections and their fields default safely for older configuration files. Options from future
milestones are deliberately absent from the example until implemented.

On the repository server:

```sh
xxc-aptd --config /etc/xxc/aptd.conf config check
xxc-apt-cli --config /etc/xxc/aptd.conf config validate
xxc-apt-cli config show
```

`config show` uses the authorized socket. The current schema contains no secret
values. Never add passwords, bearer tokens or private keys to it. All changes
require a service restart; SIGHUP reload and web editing are not implemented.

## Supported sections

| Section | Behavior |
| --- | --- |
| server | HTTP bind addresses, Unix socket, external origin, `/repo`, byte/time/concurrency upload limits |
| paths | Archive, uploads, staging, temporary, database, keys and runtime locations |
| repository | Default suite/codename, optional suite list, components, machine architectures, Release identity and policy |
| signing | gpg or xxc-trust backend, pinned fingerprint, optional remote key ID, public export filenames and command deadline |
| web | Site name, tagline and main site link |
| logging | tracing filter, operational/audit paths, strict file startup policy |
| admin | Enable flag, explicit remote-bind opt-in, canonical browser-facing origin, session lifetime, login window/attempt limit and session cap |
| analytics | Private public-traffic aggregates, enable flag and retention days |
| xxc_trust | Optional HTTPS X.509 inventory, systemd credential name, authority/template selection, public CA file and request bounds |

The [configuration man page](man/aptd.conf.5.scd) documents every field and
range. `max_upload_bytes` uses bytes; deadline and Valid-Until fields use seconds.
The example gives 2 GiB, 900 seconds per upload, two concurrent uploads and
14 days of metadata validity. Republish before expiry; automatic refresh is
pending. An empty fingerprint permits local GPG startup but publishing fails. Remote
signing requires enabled Trust, an explicit key ID and a nonempty fingerprint.
See [SIGNING.md](SIGNING.md) for backend setup.

Administrative HTTP defaults to loopback and always requires local authentication.
Binding a non-loopback address, including `0.0.0.0` or `[::]`, requires
`admin.allow_remote = true`. Public wildcard binding needs no opt-in.
Set each external URL to its concrete browser-facing hostname/IP and port;
unspecified wildcard addresses are rejected as external origins.
See [INSTALLATION.md](INSTALLATION.md) for trusted-network testing.
`admin.enabled = false` returns 503 for the whole TCP admin listener; the Unix
socket stays available. Authentication and CSRF cannot be disabled.
See [ADMINISTRATION.md](ADMINISTRATION.md) for origin and cookie requirements.
The public external URL must be an HTTP(S) origin without embedded credentials,
query, path, fragment or trailing slash. It controls downloadable source
definitions and setup commands; request Host/forwarding headers do not change it.
When moving from direct IP testing to a public reverse proxy, update
`server.external_url` to the public HTTPS origin and restart. The admin origin is
independent; only `admin.external_url` determines Secure session cookies.

All paths must be absolute. State directories cannot have symlink ancestors or
overlap one another. Keys, uploads, staging and temporary directories require
0700. Private files cannot live under the public archive. Upload/staging/tmp
paths stay inside the database parent tree. The first configured component
receives uploads; architecture `all` is generated automatically.

Public Host values must match the configured external hostname, configured bind
address or loopback. Forwarded Host and identity headers are ignored. Private GPG
key files are checked for group/other permissions and escaping symlinks at startup.

## Optional XXC Trust

See [XXC-TRUST.md](XXC-TRUST.md) for credential delivery and inventory use.
The token is read once at startup from `CREDENTIALS_DIRECTORY/token_credential`.
The config contains its name only. TLS verification, bounded responses and
redirect rejection are mandatory. Configuration changes and token rotation
require restart. An upstream outage does not prevent APT serving.

## Suites and project automation

Keep `repository.suite = "zerotrust"` for existing clients. Set
`repository.suites = ["zerotrust", "experimental", "development", "nightly", "production"]`
to enable additional channels. The default is an empty list, meaning only the
primary suite. All suites share components, architectures, signing identity and
Release policy. The primary codename remains configurable; additional codenames
match suite names. Changes require restart. Removing an already published suite
is rejected during publication; use an explicit archive migration.

`server.trusted_proxies = []` ignores forwarded client addresses. Add the actual
immediate proxy addresses as CIDRs in the installed configuration, for example
`["127.0.0.1/32", "::1/128"]` for a proxy on the same server. These addresses
influence analytics only. The parser follows X-Forwarded-For from right to left
through trusted peers; malformed, ambiguous or oversized headers fall back to
the immediate peer. Authentication and login throttling never use these headers.

`[analytics]` defaults to `enabled = true`, `retention_days = 90`. Retention accepts
1–365 days; the UI offers 7, 30 and 90-day windows. Older days in a window may be
empty when retention is shorter. Disabling collection preserves existing data
until retention removes it. See [ANALYTICS.md](ANALYTICS.md).
