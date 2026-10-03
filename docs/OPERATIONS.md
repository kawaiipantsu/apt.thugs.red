# Operations

Run the service as `xxc-aptd`, with only trusted administrators in
`xxc-aptd-admin`. Members can stage and publish packages through the socket.
Default HTTP is loopback; use the documented reverse proxy for public access.

## Publish

```sh
xxc-apt-cli upload add package.deb
xxc-apt-cli upload inspect PACKAGE_ID
xxc-apt-cli upload stage PACKAGE_ID
xxc-apt-cli package list
xxc-apt-cli repo diff
xxc-apt-cli repo publish --review-token TOKEN_FROM_DIFF
xxc-apt-cli jobs show JOB_ID
xxc-apt-cli repo verify
```

Inspect the job result before announcing a publication. `publish` only queues
the operation. A failed signer or invalid package leaves the current pointer
unchanged. Review the diff and pass its exact token; changes to the selection
require another review. Existing versions remain selected; removal is pending.
The equivalent browser workflow and local account setup are documented in
[ADMINISTRATION.md](ADMINISTRATION.md).

## Roll back

```sh
xxc-apt-cli repo generations
xxc-apt-cli repo rollback GENERATION_ID
xxc-apt-cli jobs show JOB_ID
```

Keep previous public keys available in the signing keyring while retaining their
generations. Rollback verifies the original fingerprint recorded in the manifest.
Check Valid-Until before returning to an old generation. An expired generation
needs a fresh signed publication before clients will accept it.

## Diagnose

```sh
xxc-apt-cli status
xxc-apt-cli health
xxc-apt-cli jobs list
xxc-apt-cli audit tail
sudo journalctl -u xxc-aptd --since today
```

Operational logs also use `/var/log/xxc-aptd.log`; audit events use SQLite and
`/var/log/xxc-aptd-audit.log`. File writers reopen on every event, so logrotate
needs no copytruncate or signal. File-log failure falls back to stderr unless
strict startup logging is enabled. No SIGHUP reload exists; validate and restart.

SIGTERM drains connections and waits for publication. A process-wide filesystem
lock prevents two daemons using one repository. Restart marks interrupted jobs
failed and reconciles active package state from the selected manifest. Abandoned
build directories are hidden; automatic cleanup and disk quotas are still pending.
Health does not yet diagnose disk capacity, key expiry or metadata age. Monitor
those externally. Publish at least daily to refresh the 14-day Valid-Until;
automatic scheduled refresh is a production gate.

## Infrastructure CA availability

Use `xxc-apt-cli trust status` to check the optional CA connection. A failed CA
request does not block package downloads, offline verification or local GPG
publication. New remote signatures require the CA to be available. Inspect the
structured error code and verify the credential, CA permissions, TLS root and
network route. Restart after changing the loaded token. See [XXC-TRUST.md](XXC-TRUST.md).
