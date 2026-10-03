# Development

Use Rust 1.94 or later and Debian 13 tooling. Runtime dependencies are
`apt-utils`, `dpkg`, `gnupg` and `liblzma5`; no Node runtime is shipped.
As root on a development machine:

```sh
apt install build-essential pkg-config liblzma-dev apt-utils gnupg curl \
  scdoc debhelper lintian python3 nodejs npm chromium
```

As a normal developer:

```sh
cargo install cargo-audit --locked
npm ci --ignore-scripts
make build
make fmt-check lint test docs-check version-check
make ci
```

Debian 13's base Rust toolchain is older than this workspace requires. Install
Rust/Cargo 1.94 or later from trixie-backports (also satisfying dpkg build
dependencies), or use an equivalent locally maintained toolchain. CI installs
the backports packages and pins rustup 1.94.1 for compilation and linting.

## Documentation screenshots

Run `make screenshots` with the same development dependencies as `make test-ui`.
It starts an isolated daemon and a local HTTPS signing fixture, publishes sample
packages, then captures the public and authenticated admin workflow using Chromium.
No production configuration, credentials or CA connection are used. The generated
client instructions use the reserved `apt.example.test` domain.

Review the 13 images in `.build/screenshots/`, then copy the reviewed PNG files to
`docs/screenshots/`. The [gallery](SCREENSHOTS.md) and README use those tracked
images. Signing fingerprints are masked, login fields are empty, and credentials
stay in the temporary test directory. Do not replace these with captures of a live
deployment containing private addresses, account information or key identifiers.

## Tests

On a systemd development host, root may also run `make test-systemd`. It uses
isolated paths and ports under .build, a temporary transient unit and an
unprivileged account to exercise the shipped sandbox while signing/publishing.
It does not install or modify the production service.

`make test` includes Rust tests and the disposable real APT suite.
`make test-ui` uses development-only Playwright and Chromium. `make ci` is local
and runs all implemented quality gates; passing it does not complete unimplemented
production requirements on the roadmap. CI must not skip failed security checks.

Do not run the test fixture as a production archive. Tests create their own
configuration, random loopback ports, keys and isolated APT state. They never
modify host APT sources or install a fixture into the host package database.
Acquisition is validated by checksum and extraction into a temporary directory.

Useful entry points: core/config.rs, core/package.rs, core/repository.rs,
web/api.rs, web/public.rs and tests/integration/apt_e2e.py. Core contains no Axum
dependency. Update docs/man sources alongside CLI/config changes.

The Trust integration test also needs `openssl` to create a disposable HTTPS
fixture. It tests verified TLS, outbound response bounds and wildcard listener
authentication without contacting a real CA or changing host firewall rules.
