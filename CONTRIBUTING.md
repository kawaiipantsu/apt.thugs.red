# Contributing

Read AGENTS.md and docs/DEVELOPMENT.md. Keep core logic independent of Axum.
Use Conventional Commits and your configured Git identity. Do not include
credentials, deployment configuration, generated signing keys or build artifacts.

Every behavior change includes meaningful tests and documentation in the same
change. Update the sample config and configuration man page for new settings;
update API/OpenAPI and CLI man pages when those interfaces change. Update
ROADMAP.md only after verification. Run `make ci` before declaring completion.

Do not rewrite repository history or make automated commits outside an explicit
release operation. Use `make wiki-build` to derive wiki pages from canonical docs.
