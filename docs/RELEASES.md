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
