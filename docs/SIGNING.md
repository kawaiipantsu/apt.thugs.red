# OpenPGP repository signing

Choose `gpg` for locally held keys or `xxc-trust` for keys held by the XXC Trust
OpenPGP service. Both backends produce `InRelease` and `Release.gpg` and use the
same generation workflow. A full fingerprint is mandatory before publishing.
X.509 certificates remain separate from APT's OpenPGP trust chain.

Before activation, XXC-APTD verifies both signatures locally with GPG, requires
SHA-256 or SHA-512 and the pinned primary/signing fingerprint, and checks that
InRelease decodes to the exact Release bytes. No remote verification assertion
can replace these checks. Failure leaves the active generation unchanged.

## XXC Trust backend

Use the credential setup in [XXC-TRUST.md](XXC-TRUST.md). The token needs
`openpgp.read` and `openpgp.sign`. Grant `openpgp.manage` only when remote key
generation is needed. The integration never requests `openpgp.export` and has no
private export, revoke, decrypt or arbitrary-data signing route.

On the repository server, an authorized administrator can use:

```sh
xxc-apt-cli key list
xxc-apt-cli key generate --name 'Example archive' --email archive@example.invalid --algorithm Ed25519 --days 365
xxc-apt-cli key show KEY_ID
xxc-apt-cli key verify KEY_ID
xxc-apt-cli key export KEY_ID --armor --output archive-public.asc
```

Generation stores the secret only in the CA. Its name/email become part of the
public OpenPGP key. The admin page `/admin/keys` supports the same workflow.
Generation is explicit, audited and never automatically retried. After a timeout,
check inventory before trying again because the CA may already have created it.
Neither generation nor export activates a signer or publishes a CA exchange key.

As root, select the returned ID and full fingerprint in `/etc/xxc/aptd.conf`:

```toml
[signing]
backend = "xxc-trust"
remote_key_id = "REPLACE_WITH_32_LOWERCASE_HEX_ID"
fingerprint = "REPLACE_WITH_FULL_FINGERPRINT"
# Preserve the other signing fields from the installed example.
```

The replacement markers deliberately fail validation until replaced. Then:

```sh
xxc-aptd config check
systemctl restart xxc-aptd
xxc-apt-cli repo diff
xxc-apt-cli repo publish --review-token REVIEW_TOKEN
xxc-apt-cli jobs show JOB_ID
xxc-apt-cli repo verify
```

Review and public-key distribution precede activation for an existing repository;
follow [KEY-ROTATION.md](KEY-ROTATION.md). Do not generate a replacement during
an existing-site migration when continuity with its current key is required.

The adapter uses authenticated HTTPS GET for key metadata and binary public
export, then POST `/debian/sign` with `kind=release` and base64 Release bytes.
It accepts exactly `InRelease` and `Release.gpg` artifacts, rejects unrecognized
or duplicate filenames, and never uses remote names as filesystem paths.
Public exports are limited to one primary key matching the pin; private packets
are rejected before import. Exported keys are normalized with GPG public-only
export and kept in the signed generation. The daemon's private key directory
can remain empty for this backend.

Limits: 8 MiB Release input and 1 MiB public key. Outbound responses obey
`xxc_trust.max_response_bytes` (1 MiB by default, up to 16 MiB). Large Release
signatures may require increasing that response cap. CA request deadlines use
`xxc_trust.request_timeout_seconds`; local GPG commands use
`signing.command_timeout_seconds`. The existing single publisher bounds signing
jobs, and Trust requests share the two-request concurrency limit.

Exchange publication is optional. Authenticated key downloads and signing do
not require a key to be listed on the exchange. Repository clients obtain the
public key from XXC-APTD's own canonical URLs. Exchange publication exposes the
key and all its identity fields through the CA's public discovery service.
XXC-APTD leaves that independent setting unchanged.

A CA outage prevents new remote signatures. Downloads, public-key distribution,
startup verification and rollback use retained public snapshots and continue
without contacting the CA. Restore operations still validate signatures and
checksums against the stored public key. Offline verification does not query
new CA revocations; handle revocation distribution and rollback policy through
the operator workflow. Verification does not extend an old Release's Valid-Until.

## Local GPG backend

Root initializes directories, then the service account can generate an evaluation
key with standard GPG:

```sh
sudo xxc-aptd init --system
sudo -u xxc-aptd gpg --homedir /etc/xxc/apt.keys --quick-generate-key 'XXC archive signing' ed25519 sign 1y
sudo -u xxc-aptd gpg --homedir /etc/xxc/apt.keys --with-colons --list-secret-keys
```

Set `backend = "gpg"`, leave `remote_key_id` empty, and select the full fingerprint.
Arrange an operator-reviewed unattended key/agent policy. This release does not
supply key passphrases or support hardware signing adapters. Directory mode is
0700; private key files are 0600 and owned by the service account.

## Public distribution and package bytes

The legacy ASCII URL `/repo/thugsred.gpg.key` and canonical
`/repo/thugsred-archive-keyring.asc` and `.gpg` export the active generation's
public key. These aliases survive custom export filenames. Clients should
compare fingerprints through a separate trusted channel and use Signed-By.

APT authenticates packages through signed Release checksums and the Packages
indexes. The uploaded `.deb` bytes remain immutable. The CA's optional standalone
`deb` signing operation returns a detached `.asc`, has an 8 MiB input limit and
does not implement embedded debsigs/dpkg-sig. XXC-APTD does not call it during
archive publication. Source metadata formats and standalone package signature
management remain explicit roadmap work.

API contract: [XXC Trust developer documentation](https://ca.xxc.dk/developers).
