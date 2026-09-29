# Contributing

Start with the [contributor guides](Docs/spec/contributing/). They explain
examples, domain changes, issue tracking, security closure, and repository
procedures without duplicating the project contract.

Read [AGENTS.md](AGENTS.md) before changing the repository. It defines
authority, source-of-truth boundaries, proof expectations, and the required
working environment. Run repository commands through
[`Tools/agent/jet-env`](Tools/agent/jet-env); the worker type-check lane is
[`Tools/agent/lane-check.sh`](Tools/agent/lane-check.sh).

[Tower](Docs/spec/contributing/issue-tracker.md) is the work ledger. Use the
[vendored Tower skill](Tools/tower/skills/tower/SKILL.md) for board operations
and never hand-edit Tower state. Keep plans, criteria, decisions, and work
state there rather than creating a second tracker in a document.

Rust source files use PascalCase names. Preserve that file identity when
adding or moving Rust modules.

Treat everyone with respect in issues, pull requests, and reviews; the
[Contributor Covenant](https://www.contributor-covenant.org/version/2/1/code_of_conduct/)
describes the conduct we expect. Report a conduct problem privately to the
repository owner on GitHub, not in a public thread.
