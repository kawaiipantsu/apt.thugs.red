xxc-apt-cli(1) "XXC-APTD 0.1.0"

# NAME

xxc-apt-cli - local administration of XXC-APTD

# SYNOPSIS

*xxc-apt-cli* [*--socket* _PATH_] [*--json*] [*--quiet*] _COMMAND_

# COMMANDS

*status*, *health*

*package list*, *package search* _QUERY_, *package show* _ID_, *package import* _FILE_

*upload add* _FILE_, *upload list*, *upload inspect* _ID_, *upload stage* _ID_

*repo status*, *repo verify*, *repo reindex*, *repo diff*,
*repo publish* *--review-token* _TOKEN_, *repo generations*,
*repo rollback* _GENERATION_

*user list*, *user add* _USERNAME_ [*--role* _ROLE_] [*--password-stdin*],
*user passwd* _ID_ [*--password-stdin*], *user role* _ID_ _ROLE_,
*user enable* _ID_, *user disable* _ID_, *user delete* _ID_

*trust status*, *trust authorities*, *trust templates*,
*trust certificates* [*--page* _PAGE_] [*--query* _TEXT_] [*--status* _STATUS_]

*key list* [*--page* _PAGE_] [*--query* _TEXT_], *key show* _ID_,
*key generate* *--name* _NAME_ *--email* _EMAIL_ [*--algorithm* _ALGORITHM_] [*--days* _DAYS_],
*key export* _ID_ *--output* _FILE_ [*--armor*], *key verify* _ID_

*jobs list*, *jobs show* _ID_

*audit tail*

*config show*, *config validate*

*--version*, *--about*, *--help*

# DESCRIPTION

The default transport is /run/xxc-aptd/admin.sock. Socket permissions authenticate
the operator. Membership of xxc-aptd-admin grants full administrative authority.
The CLI uses the local socket. HTTP accounts serve the separate administrative
UI/API; HTTP credential transport in the CLI is not implemented.

Uploads stream to disk. Import and upload add quarantine a package; neither
stages nor publishes it. Stage the returned package ID explicitly, review repo
diff, then publish with its exact review token. Changed selections return a
conflict and require another review. Existing versions remain selected.
Publication returns a job_id immediately. Poll jobs show until succeeded or
failed. A successful command that queues a job does not imply job success.

*--json* prints compact JSON; the default is readable JSON. *--quiet* suppresses
successful output. Errors remain on stderr. *--config* selects the local file for
config validate; config show reads the daemon's effective configuration.

# USERS

Passwords are prompted twice without echo. --password-stdin reads a bounded
password from standard input for automation. Passwords require 12..1024 UTF-8
bytes without NUL; never supply them in argv or environment variables.
Roles are viewer, operator and administrator. The first user must be an
administrator; user add otherwise defaults to viewer. Use IDs from user list.
Password, role, enable, disable and delete revoke the user's sessions. The last
enabled administrator cannot be removed, disabled or demoted.

# XXC TRUST

Read-only X.509 inventory through the daemon's systemd credential. The CLI
never accepts or prints the CA token. status reports enabled/connected and
configured authority/template availability. Certificate pages are one-based
(1..1000000); query is at most 256 bytes and status is active, expiring, expired or revoked.
No private keys, CSRs or certificate owner identities are returned. These
commands do not issue or revoke certificates or alter APT OpenPGP signing.

# OPENPGP KEYS

Key commands administer the XXC Trust inventory. Generation requires a public
name/email; those values appear in the distributed OpenPGP key. Algorithms are
Ed25519 (default), RSA-3072 and RSA-4096; validity is 1..3650 days (default 365).
Generation is never automatically retried or activated. After an ambiguous
failure, inspect inventory before retrying. Pin ID/fingerprint in aptd.conf to
select the remote signer. Public export writes a new file and refuses overwrite;
The --armor option selects ASCII instead of binary. Private export is never supported.
Verify checks public material against metadata and the configured pin when
selected; it does not assert independent ownership of the identity.

# EXIT STATUS

0: request accepted or completed. 1: operation or transport failure. 2: invalid
command line. Inspect the eventual job result for asynchronous commands.

# LIMITATIONS

Listing returns at most 50 package records; the API accepts a zero-based page
parameter. Audit tail returns the latest 100 records, without following.
Private key import, automated activation/rotation, package deletion, GC and
doctor commands are roadmap work.

# SEE ALSO

*xxc-aptd*(8), *aptd.conf*(5)

# SUITE SELECTION

*--suite* _name_ selects the suite for package queries, uploads, staging and
reviewed publication. Omission uses repository.suite. For example:

```
xxc-apt-cli --suite nightly upload add package.deb
xxc-apt-cli --suite nightly upload stage PACKAGE_ID
xxc-apt-cli --suite nightly repo diff
xxc-apt-cli --suite nightly repo publish --review-token REVIEW_TOKEN
```

Rollback restores every suite in a retained generation.
