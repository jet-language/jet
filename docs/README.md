# Jet documentation

[AGENTS.md](../AGENTS.md#documentation-boundaries) defines documentation policy.
[Philosophy](spec/philosophy.md) is the one strategic guidance document.
[Tower](../plugins/tower/skills/tower/SKILL.md) owns specific goals, plans,
decisions, and work state. No documentation page is a development dashboard.

## Start here

- [First hour](spec/guides/first-hour.md) and [diagnostic recovery](spec/guides/diagnostic-recovery.md)
- [Executable examples](../examples/README.md) and [syntax examples](../examples/canon.jet)
- [Contributor workflow](spec/contributing/issue-tracker.md) and [example procedure](spec/contributing/examples.md)

## Read the source for current behavior

| Question | Executable source or evidence |
|---|---|
| Syntax and names | [Syntax registry](../crates/jet-foundation/src/Syntax.rs), [parser](../crates/jet-parser/src/), and [examples](../examples/features/) |
| Language checks | [Semantic checker](../crates/jet-sema/src/) and [tests](../tests/) |
| Core APIs and runtime meaning | [Prelude](../crates/jet-codegen/src/Prelude/) and [CoreLib](../corelib/) |
| Diagnostic text and rendering | [Registered diagnostics](../crates/jet-codegen/src/Prelude/Diagnostics.jet), [UI snapshots](../tests/ui/), and `jet explain E0102` |
| CLI commands and flags | [CLI implementation](../crates/jet-cli/src/) and `jet help` |

## Explanations and evidence

| Category | Read it for |
|---|---|
| [Spec](spec/) | Durable contracts, design reasons, and usage or contributor guides. Not proof of implementation. |
| [Audits](audits/) | Dated findings and retained evidence. Not current status. |
| [Research](research/) | Source-backed findings and prior alternatives. |
| [Proposals](proposals/) | Design alternatives linked to a Tower decision. Not a work queue. |

For design reasons, see [syntax decisions](spec/syntax-decisions.md),
[architecture](spec/architecture.md), and the [diagnostic contract](spec/diagnostics.md).
Check the source and exercise the relevant behavior before relying on a claim.
