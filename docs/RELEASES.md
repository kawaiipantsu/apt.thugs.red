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
references. They do not commit or tag. Automatic selection reads commits since
the latest vX.Y.Z tag: breaking markers choose major, feat chooses minor, and
fix/known maintenance types choose patch. Untyped or ambiguous history fails.
An untagged repository requires an explicit selection.

`make release-patch` (or minor/major) is an explicit release operation: it requires
a clean worktree and configured global Git identity, bumps, runs make ci, commits
with a Conventional Commit and creates the annotated version tag. It does not
push automatically. Review the result before pushing the commit and tag.

The tag workflow builds the Debian artifact and a GitHub release. Set the
repository variable `RELEASE_DISCUSSION_CATEGORY` to an existing category such
as `Announcements` to attach a release discussion and notify its subscribers.
No announcement is sent during normal builds. For a manual release use
`make publish-release DISCUSSION_CATEGORY=Announcements` after pushing the tag.

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
