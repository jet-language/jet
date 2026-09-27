# Contributing

Start with the [contributor guides](docs/spec/contributing/). They explain
examples, domain changes, issue tracking, security closure, and repository
procedures without duplicating the project contract.

Read [AGENTS.md](AGENTS.md) before changing the repository. It defines
authority, source-of-truth boundaries, proof expectations, and the required
working environment. Run repository commands through
[`scripts/agent/jet-env`](scripts/agent/jet-env); the worker type-check lane is
[`scripts/agent/lane-check.sh`](scripts/agent/lane-check.sh).

[Tower](docs/spec/contributing/issue-tracker.md) is the work ledger. Use the
[vendored Tower skill](plugins/tower/skills/tower/SKILL.md) for board operations
and never hand-edit Tower state. Keep plans, criteria, decisions, and work
state there rather than creating a second tracker in a document.

Rust source files use PascalCase names. Preserve that file identity when
adding or moving Rust modules.
