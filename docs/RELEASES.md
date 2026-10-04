# Releases and wiki

VERSION is the application SemVer source. Workspace crates inherit it; Debian
changelog uses a separate revision. Binaries display the application version.
`make version-check` verifies workspace/lockfile/changelog/man-page consistency.

```sh
make bump-patch
make bump-minor
make bump-major
make bump-auto
make deb-revision
```

Bumps update VERSION, Cargo metadata/lockfile, changelogs and current documentation
references. Write concrete `Unreleased` changelog notes first: a bump moves them
into a dated release section and leaves a fresh `Unreleased` section. Historical
verification records, screenshot captions and version examples are preserved.
Bumps do not commit or tag. Automatic selection reads commits since
the latest vX.Y.Z tag: breaking markers choose major, feat chooses minor, and
fix/known maintenance types choose patch. Untyped or ambiguous history fails.
An untagged repository requires an explicit selection.

`make release-patch` (or minor/major) is an explicit release operation: it requires
a clean worktree and configured global Git identity, bumps, runs make ci, commits
with a Conventional Commit and creates the annotated version tag. It then
rebuilds and checks the package from that clean commit so both binaries report
the tagged commit without a dirty marker. It does not push automatically.
Review the result before pushing the commit and tag.

The tag workflow runs `make ci` on Debian 13, builds the Debian artifact and
publishes a GitHub release with the version's changelog notes and `SHA256SUMS`.
Only packages matching the current application version and Debian revision are
included; older files left in `dist/` are excluded. Both packaged binaries must
report the clean tagged source before publication. Set the
repository variable `RELEASE_DISCUSSION_CATEGORY` to an existing category such
as `Announcements` to attach a release discussion and notify its subscribers.
No announcement is sent during normal builds. Pushing `vX.Y.Z` starts the release
workflow automatically; do not also publish manually while that workflow runs.
The workflow also accepts an existing tag through `workflow_dispatch`, so a
runner/workflow repair can retry an unpublished version without moving its tag:

```sh
gh workflow run release.yml --ref main -f tag=vX.Y.Z
```

The runner trusts only its checked-out workspace and rebuilds the core crate
after restoring Cargo caches, ensuring its embedded Git identity is fresh.
If the workflow is unavailable, build locally from the clean release tag and use:

```sh
make ci
make prepare-release
make publish-release DISCUSSION_CATEGORY=Announcements
```

The tag must already exist on GitHub. `prepare-release` checks the artifacts and
writes checksums and notes into `dist/vX.Y.Z/` without publishing. Omitting the
discussion category publishes without an announcement. Existing releases are
never overwritten. Download the package and `SHA256SUMS` into one directory and
run `sha256sum --check SHA256SUMS` before installation. These checksums detect
download corruption; they are distributed through GitHub's HTTPS release page.

## Publish the Debian package to APT

Project Debian releases also belong in the official archive's `zerotrust` suite.
Publish the **same bytes** distributed by the GitHub release. Rebuilding the same
Debian version on another machine can produce different bytes and will be rejected
as a conflicting upload. Wait for the tagged GitHub workflow to succeed, then run
these commands on a release worker that can reach the administrative API:

```sh
release_version=$(cat VERSION)
release_dir="$PWD/dist/github-v$release_version"
mkdir -p "$release_dir"
gh release download "v$release_version" --repo kawaiipantsu/apt.thugs.red \
  --pattern '*.deb' --pattern SHA256SUMS --dir "$release_dir"
(cd "$release_dir" && sha256sum --check SHA256SUMS)
export XXC_SUITE=zerotrust
export XXC_DEB="$release_dir/xxc-aptd_${release_version}-1_amd64.deb"
```

Select the actual packaging revision/architecture if different. Supply
`XXC_API_BASE` from the operator and `XXC_TOKEN_FILE` through a private file or
CI file-secret facility. The token needs read, upload, stage and publish scopes
for `zerotrust`. Keep both deployment addresses and token values out of Git,
release notes, screenshots and shell tracing. A private deployment may require
running this step on its internal release worker; hosted GitHub runners do not
automatically have access to that network.

Run the [tested automation example](AUTOMATION.md#ci-example). It streams the
upload, stages it, checks the diff, posts the review token and waits for the job
to succeed. Reject any unrelated staged additions, removals or downgrades.
Do not publish a suite merely because the upload succeeded. An already published
identical package needs verification rather than repeated staging.

The public contract is documented at [the API guide](https://apt.thugs.red/api)
and [OpenAPI](https://apt.thugs.red/api/openapi.json). These public documentation
URLs do not expose publishing operations. Do not infer an administrative API base
from the public hostname or the example `/admin` prefix.

After publication, use an isolated APT configuration with the repository-specific
keyring to run `apt-get update` and download the exact package version. Verify the
download's SHA-256 against the GitHub artifact and confirm the package appears in
the public package browser. Check through the public proxy when reachable; record
origin-only verification explicitly when the release worker cannot reach it.

## Wiki

Canonical docs live in docs/. `docs/wiki/manifest.json` selects source pages.
`make wiki-build` produces `.build/wiki/`; `make wiki-check` checks the mapping
and local links. `make wiki-sync WIKI_REMOTE=...` clones the wiki into a temporary
tree, updates generated pages, commits using global Git identity and pushes.
It preserves unrelated wiki content. This is an explicit publication operation.

The configured project wiki is:

```sh
make wiki-sync WIKI_REMOTE=https://github.com/kawaiipantsu/apt.thugs.red.wiki.git
```

No remote, credentials or approval to publish is inferred from a documentation
build. Source origin is `git@github.com:kawaiipantsu/apt.thugs.red.git`.
