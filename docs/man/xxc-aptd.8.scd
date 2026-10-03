xxc-aptd(8) "XXC-APTD 0.1.0"

# NAME

xxc-aptd - signed Debian APT repository daemon

# SYNOPSIS

*xxc-aptd* [*--config* _FILE_] [*serve* [*--allow-root*]]

*xxc-aptd* [*--config* _FILE_] *init* [*--system* | *--root* _DIRECTORY_]

*xxc-aptd* [*--config* _FILE_] *config check*

*xxc-aptd* *--version* | *--about* | *--help*

# DESCRIPTION

One unprivileged process serves public HTTP, a separate authenticated administrative
HTTP listener and an authorized Unix management socket. TLS belongs to an
upstream reverse proxy. Configuration is never rewritten during normal operation.

Default public listener: 127.0.0.1:8088. Default administrative listener:
127.0.0.1:8089. Administrative HTTP requires a local user session and role.
Create the first account with *xxc-apt-cli user add* --role administrator.
Configure admin.external_url before proxying /admin/login.
Use *xxc-apt-cli*(1) through /run/xxc-aptd/admin.sock.

# INITIALIZATION

*init* creates missing directories and configuration using create-new semantics.
Existing configuration and keys are preserved. *--system* requires root and
creates the service account and assigns required directory ownership.
*--root* creates an isolated development tree; also select *--config* explicitly.

# SIGNALS

SIGTERM and SIGINT drain requests and wait for the active publication job.
SIGHUP reload is not implemented. All configuration changes require restart.

# FILES

/etc/xxc/aptd.conf
/etc/xxc/apt.keys/
/var/lib/xxc-aptd/
/var/log/xxc-aptd.log
/var/log/xxc-aptd-audit.log

# EXIT STATUS

Zero indicates success. Nonzero indicates invalid configuration, failed
initialization or a service failure. *--allow-root* is for disposable tests only.

# SEE ALSO

*xxc-apt-cli*(1), *aptd.conf*(5), *systemd*(1)
