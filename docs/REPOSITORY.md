# Debian repository behavior

The compatibility contract is `/repo`, suite/codename `zerotrust`, component
`main`, and `/repo/thugsred.gpg.key`. Binary keyring and canonical ASCII exports
are available alongside generated `.sources` and `.list` definitions.

Only binary `.deb` ingestion is supported. `.udeb`, source packages, `.changes`,
`.buildinfo` and source tarballs remain roadmap work. Source package names from
binary metadata determine pool prefixes: `libfoo` uses `libf/libfoo`, other
names use their first character. Debian epochs remain in Version and are omitted
from pool filenames. Pool filename collisions are rejected, including epoch
changes that would reuse a filename with different bytes.

Uploads are quarantined and inspected without executing maintainer scripts.
Identical name/version/architecture/content is idempotent. Different content for
the same identity is rejected. There is no override endpoint yet. Upload records
are explicitly staged before inclusion. Existing versions are retained in indexes;
the current package detail view does not yet implement Debian version ordering.

`apt-ftparchive` produces Packages and Release metadata. Each machine architecture
index includes matching packages and architecture `all`; binary-all is also
generated. Gzip and xz indexes, SHA-256/SHA-512 Release checksums and SHA256/SHA512
by-hash paths are published. Release has Valid-Until and Acquire-By-Hash.

Each generation has signed indexes, exported public keys and a JSON manifest.
The shared pool is immutable. Publication fsyncs artifacts, verifies signatures,
then atomically replaces the dists symlink. HTTP resolves a single generation
per request. Requests for old by-hash objects search retained generations.
Clients requesting moving paths across publication boundaries rely on APT's
checksum validation/retry; a sequence of HTTP requests is not one transaction.

The filesystem pointer is authoritative. On restart SQLite reconciles active
records from the manifest. `repo reindex` repeats that reconciliation. It does
not yet import arbitrary third-party archives or rebuild all historical audit
state. `repo verify` checks manifest hashes, pool bytes and both signatures.

Rollback checks a retained generation before activation. Retention and GC are
not automatic yet: all generations and uploaded data remain on disk. Do not
manually remove referenced pool objects or generation directories.

## Reviewed publication

`repo diff` and `/api/v1/repository/diff` compare the current manifest with the
selected package records. A SHA-256 review token binds that generation, selection
and repository/signing configuration. Publish requires the token under the same
single-publisher lock used by staging and rollback. A stale review returns 409.
Version comparisons use Debian epoch/upstream/revision semantics, including `~`.
Existing versions remain selected; an older addition does not necessarily change
APT's preferred version. Removal and replacement overrides are not implemented.

## Multiple suites

Configure an explicit `repository.suites` list while retaining the primary
`repository.suite`. Each suite owns uploaded/staged/active memberships in shared
immutable package objects. UI suite selectors and CLI `--suite` choose a channel;
API calls use `?suite=nightly`. Identical package bytes may belong to several suites.
Publishing consumes only the target suite's staged membership. Other suites retain
their currently published selections, including when they have pending changes.

A generation contains all configured suites. Each receives signed Release,
InRelease, Release.gpg and by-hash indexes before the single atomic switch. Other
suites' metadata is refreshed with unchanged selections. Rollback restores the
entire generation across all suites. Verification checks every suite. Removing a
published suite from configuration blocks publication instead of silently removing
its files. Additional suites share the configured primary suite's Release policy;
per-suite NotAutomatic/expiry/architecture overrides remain future work.

Schema migration assigns existing package records to the primary suite. Legacy
single-suite manifests remain readable, verifiable and usable for rollback and
reindex. The filesystem manifest records complete suite membership independently
of SQLite. Public `/releases` lists channels; `/releases/<suite>` includes its source
definition. Download `/repo/thugsred.sources?suite=nightly` or the `.list` equivalent.
The canonical URLs without a query preserve existing default-suite clients.

See [AUTOMATION.md](AUTOMATION.md) for project tokens and automated publication.
