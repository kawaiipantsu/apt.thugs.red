# Development rules

- Read docs/ARCHITECTURE.md, docs/ROADMAP.md and docs/SECURITY.md before modifying architecture.
- Never expose private signing material or secrets in tools, logs, UI or API.
- Never add HTTPS termination to xxc-aptd.
- Never break /repo compatibility without an explicit migration.
- Never bypass staging or the signed publication generation model.
- Every behavior change requires tests; every user-visible change requires documentation.
- Config changes require the sample config and aptd.conf(5) to change together.
- CLI changes require xxc-apt-cli(1) updates; API changes require API/OpenAPI updates.
- Complete roadmap items only after tests pass. Run make ci before declaring a task complete.
- Do not silently weaken authentication or systemd security to make a test pass.
- Preserve existing configuration, private keys, repository bytes and Git history.
- Use Conventional Commits and configured global Git identity. Do not auto-commit outside an explicitly requested release operation.
- Canonical documentation lives in docs; generate wiki pages from it.
