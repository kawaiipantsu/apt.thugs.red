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
| repository | One suite/codename, components, machine architectures, Release identity and policy |
| signing | gpg or xxc-trust backend, pinned fingerprint, optional remote key ID, public export filenames and command deadline |
| web | Site name, tagline and main site link |
| logging | tracing filter, operational/audit paths, strict file startup policy |
| admin | Enable flag, explicit remote-bind opt-in, canonical browser-facing origin, session lifetime, login window/attempt limit and session cap |
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
