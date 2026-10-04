PREFIX ?= /usr
SYSCONFDIR ?= /etc
LOCALSTATEDIR ?= /var
DESTDIR ?=
export PREFIX SYSCONFDIR LOCALSTATEDIR DESTDIR

.DEFAULT_GOAL := build
.PHONY: all build build-debug build-release fmt fmt-check lint check test test-unit test-integration test-ui test-systemd screenshots licenses security audit man docs docs-check wiki-build wiki-check wiki-sync deb deb-clean lintian install uninstall clean distclean version version-check bump-patch bump-minor bump-major bump-auto deb-revision release-patch release-minor release-major prepare-release publish-release ci
all: build
build: build-debug
build-debug:
	cargo build --workspace --locked
build-release:
	cargo build --release --workspace --locked
fmt:
	cargo fmt --all
fmt-check:
	cargo fmt --all -- --check
lint:
	cargo clippy --workspace --all-targets --locked -- -D warnings
check:
	cargo check --workspace --locked
test: test-unit test-integration
test-unit:
	cargo test --workspace --locked
	python3 -m unittest discover -s tests/tooling -v
test-integration: build-debug
	python3 tests/integration/apt_e2e.py
	python3 tests/integration/automation_e2e.py
	python3 tests/integration/admin_e2e.py
	python3 tests/integration/trust_e2e.py
	python3 tests/integration/remote_signing_e2e.py
test-ui: build-debug
	python3 tests/integration/ui.py
	python3 tests/integration/ui.py --proxy
screenshots: build-debug
	python3 tests/integration/ui.py --screenshots
test-systemd: build-debug
	python3 tests/integration/systemd_smoke.py
security: audit test
audit:
	$(or $(CARGO_AUDIT),$(shell command -v cargo-audit 2>/dev/null),$(HOME)/.cargo/bin/cargo-audit) audit
man:
	python3 scripts/maintain.py man
docs:
	cargo doc --workspace --no-deps --locked
docs-check:
	python3 scripts/maintain.py docs-check
wiki-build:
	python3 scripts/maintain.py wiki-build
wiki-check:
	python3 scripts/maintain.py wiki-check
wiki-sync:
	python3 scripts/maintain.py wiki-sync
deb:
	dpkg-buildpackage -us -uc -b
	python3 scripts/maintain.py collect-deb
deb-clean:
	dh clean
lintian: deb
	lintian --fail-on error dist/*.deb
licenses:
	python3 scripts/licenses.py
install: build-release man licenses
	python3 scripts/install.py install
uninstall:
	python3 scripts/install.py uninstall
clean:
	cargo clean
	rm -rf .build/man .build/wiki dist
distclean: clean
	rm -rf node_modules test-results playwright-report
version:
	@cat VERSION
version-check:
	python3 scripts/maintain.py version-check
bump-patch bump-minor bump-major bump-auto:
	python3 scripts/maintain.py $@
deb-revision:
	python3 scripts/maintain.py deb-revision
release-patch release-minor release-major:
	python3 scripts/maintain.py $@
prepare-release:
	python3 scripts/maintain.py prepare-release
publish-release:
	python3 scripts/maintain.py publish-release
ci: fmt-check lint check test docs-check version-check wiki-check man test-ui audit lintian
