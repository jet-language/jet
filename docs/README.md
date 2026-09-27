# Jet documentation

This directory explains Jet's durable contracts and the reasons behind them.
It does not prove behavior: code, registries, tests, and executable examples
do. It does not track work either: plans, decisions, and status live in
[Tower](../plugins/tower/skills/tower/SKILL.md). [AGENTS.md](../AGENTS.md#documentation-boundaries)
sets the documentation rules, and [philosophy](spec/philosophy.md) is the one
strategic guidance document.

## Learn Jet

| Read | For |
|---|---|
| [First-hour guide](spec/guides/first-hour.md) | Install Jet and learn the `new` → `run` → `check` → `test` → `fix` → `explain` loop |
| [Diagnostic recovery](spec/guides/diagnostic-recovery.md) | Reading and fixing compiler errors |
| [Examples](../examples/README.md) | Small golden-tested programs for each feature |
| [Language spec](spec/spec.md) | The language contract, topic by topic |
| [Core library reference](spec/reference/core-library.md) | The `core.*` modules |
| [Vocabulary](spec/vocabulary.md) | The terms these documents use |

## Contracts and design reasons

| Read | For |
|---|---|
| [Syntax decisions](spec/syntax-decisions.md) | Ratified syntax and semantics decisions and their rationale |
| [Architecture](spec/architecture.md) | Compiler structure, execution tiers, and the self-hosting boundary |
| [Safety](spec/safety.md) | The memory-safety model and audited `#Unsafe` regions |
| [Diagnostics](spec/diagnostics.md) | What every diagnostic must contain |
| [Standard library API laws](spec/stdlib-api-laws.md) and [native contract](spec/core-library-native-contract.md) | Rules every Core API follows |
| [Release policy](spec/release-policy.md) | Pre-1.0 compatibility and release rules |
| [Reference](spec/reference/) | Focused references: CLI, Canvas, Jetpack, FFI hosts, embedded, web, and more |
| [Environment variables](spec/reference/environment.md) | The canonical reference for `JET_*` environment variables |
| [Packaging](spec/packaging/) | Toolchain channels, the package index, and the trust root |

## Contributing

- [Contributor workflow and Tower cards](spec/contributing/issue-tracker.md)
- [Adding an example](spec/contributing/examples.md)
- [Technical traps](spec/contributing/agent-engineering.md)
- [Security closure](spec/contributing/security-closure.md)

## Where current behavior lives

| Question | Source of truth |
|---|---|
| Keywords, sigils, and names | [Syntax registry](../crates/jet-foundation/src/Syntax.rs) and [examples](../examples/features/) |
| Language checks | The semantic checker ([`crates/jet-sema/`](../crates/jet-sema/src/)) and [tests](../tests/) |
| Core APIs and runtime meaning | [Core library sources](../Core/) and the [Prelude](../crates/jet-codegen/src/Prelude/) |
| Diagnostic text | [Diagnostic registry](../crates/jet-codegen/src/Prelude/Diagnostics.jet), [UI snapshots](../tests/ui/), and `jet explain <CODE>` |
| Commands and flags | `jet help` and the [CLI implementation](../crates/jet-cli/src/) |

The Rust-hosted compiler is the production and reference implementation; the
Jet-authored compiler under `Compiler/` is staged work that becomes
authoritative only through its bootstrap gates. See
[architecture](spec/architecture.md) for the compiler-hosting boundary.

## Evidence and proposals

These directories hold dated material. They record what was found or proposed
at a point in time, not how Jet behaves now.

| Directory | Holds |
|---|---|
| [Audits](audits/README.md) | Dated audit findings and retained evidence |
| [Research](research/) | Source-backed investigations and prior alternatives |
| [Proposals](proposals/) | Design alternatives linked to Tower decisions; not a work queue |

Check the source and exercise the behavior before relying on any claim here.
