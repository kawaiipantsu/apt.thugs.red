# Backup and restore

Backups containing `/etc/xxc/apt.keys` contain private signing material. Store
them mode 0600 inside mode 0700 directories, encrypt offline copies, and limit
access to signing-key custodians. Do not attach backups to issues or releases.

For this release use a stopped-service backup; copying a live SQLite file alone
is unsafe. On the repository server as root:

```sh
systemctl stop xxc-aptd
install -d -m 0700 /var/backups/xxc-aptd
umask 077
tar --acls --xattrs -C / -czf /var/backups/xxc-aptd/offline-backup.tar.gz \
  etc/xxc/aptd.conf etc/xxc/apt.keys var/lib/xxc-aptd
systemctl start xxc-aptd
```

Include operational/audit logs separately if required. Keep the complete state
directory, including any SQLite WAL/SHM files, pool, generations, uploads and
staging. Preserve symlinks and ownership. Test backups before discarding old ones.

Restore while the proxy is in maintenance mode and the daemon stopped. Restore
onto a new isolated instance first; do not overwrite the only surviving copy.
Install the matching binary/package and service account, restore files and check
0700/0600 key permissions. Validate aptd.conf and start on isolated loopback ports.
Run `xxc-apt-cli repo verify`, inspect its job result, check the public fingerprint,
then run an isolated real APT update/download against that instance. Only expose
it publicly after those checks. If active search state is lost, `repo reindex`
reconciles it from the active manifest; it cannot recreate historical users/audit.

A complete backup/restore rehearsal and packaged upgrade test remain
production acceptance gates. No backup CLI is provided in this slice.

## Administrative schema upgrades

When upgrading an existing schema 1 database, startup creates a SQLite online
backup named `state.pre-v2-UUID.db` alongside the database, mode 0600, and syncs
it before applying schema 2 in a transaction. Backup or migration failure stops
startup. The backup retains the pre-upgrade schema; failed migrations roll back.
The backup does not include filesystem package/signing state and does not replace
the complete stopped-service backup above. Protect and retain it until the
upgrade has been verified. Do not downgrade the binary against a newer schema. The future-schema guard
runs before changing the database journal mode.

State now includes password hashes and sessions. Before exposing a restored
instance, invalidate restored sessions while the daemon is stopped. On the
repository server, using the optional sqlite3 operator tool:

```sh
sudo -u xxc-aptd sqlite3 /var/lib/xxc-aptd/state.db \
  'DELETE FROM sessions; DELETE FROM login_challenges;'
```

A pre-v2 database has no sessions table. Restore that backup only with matching
schema/binary expectations and retain the failed upgrade's state for diagnosis.

For a remote signer, back up its configuration and credential references using
your secret-management process. The CA operator owns backup/recovery of the
remote private key. Preserve repository generation public-key snapshots: they
support offline verification and rollback even when the CA is unreachable.
A filesystem backup of XXC-APTD alone cannot restore a lost remote private key.
