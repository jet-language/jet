# Jet documentation

This directory explains Jet's durable contracts and the reasons behind them.
It does not prove behavior: code, registries, tests, and executable examples
do. It does not track work either: plans, decisions, and status live in
[Tower](../Tools/tower/skills/tower/SKILL.md). [AGENTS.md](../AGENTS.md#documentation-boundaries)
sets the documentation rules, and [philosophy](Spec/philosophy.md) is the one
strategic guidance document.

## Learn Jet

| Read | For |
|---|---|
| [First-hour guide](Spec/Guides/first-hour.md) | Install Jet and learn the `new` → `run` → `check` → `test` → `fix` → `explain` loop |
| [Diagnostic recovery](Spec/Guides/diagnostic-recovery.md) | Reading and fixing compiler errors |
| [Examples](../Examples/README.md) | Small golden-tested programs for each feature |
| [Language spec](Spec/spec.md) | The language contract, topic by topic |
| [Core library reference](Spec/Reference/core-library.md) | The `core.*` modules |
| [Vocabulary](Spec/vocabulary.md) | The terms these documents use |

## Contracts and design reasons

| Read | For |
|---|---|
| [Syntax decisions](Spec/syntax-decisions.md) | Ratified syntax and semantics decisions and their rationale |
| [Architecture](Spec/architecture.md) | Compiler structure, execution tiers, and the self-hosting boundary |
| [Compiler data](Spec/compiler-data.md) | How the compiler holds its data: id spaces, columns, phase lifetimes, no reference counts |
| [Safety](Spec/safety.md) | The memory-safety model and audited `#Unsafe` regions |
| [Diagnostics](Spec/diagnostics.md) | What every diagnostic must contain |
| [Standard library API laws](Spec/stdlib-api-laws.md) and [native contract](Spec/core-library-native-contract.md) | Rules every Core API follows |
| [Release policy](Spec/release-policy.md) | Pre-1.0 compatibility and release rules |
| [Reference](Spec/Reference/) | Focused references: CLI, Canvas, Jetpack, FFI hosts, embedded, web, and more |
| [Environment variables](Spec/Reference/environment.md) | The canonical reference for `JET_*` environment variables |
| [Packaging](Spec/Packaging/) | Toolchain channels, the package index, and the trust root |

## Contributing

- [Contributor workflow and Tower cards](Spec/Contributing/issue-tracker.md)
- [Adding an example](Spec/Contributing/examples.md)
- [Technical traps](Spec/Contributing/agent-engineering.md)
- [Security closure](Spec/Contributing/security-closure.md)
- [Rebuilding Jet from a seed](Spec/Contributing/bootstrap.md)

## Where current behavior lives

| Question | Source of truth |
|---|---|
| Keywords, sigils, and names | [Syntax registry](../Compiler/JetFoundation/Data/Syntax/) and [examples](../Examples/) |
| Language checks | The semantic checker ([`Compiler/JetSema/`](../Compiler/JetSema/Source/)) and [tests](../Tests/) |
| Core APIs and runtime meaning | [Core library sources](../Core/) and the [Prelude](../Compiler/Prelude/) |
| Diagnostic text | [Diagnostic registry](../Compiler/Prelude/Diagnostics.jet), [UI snapshots](../Tests/UI/), and `jet explain <CODE>` |
| Commands and flags | `jet help` and the [command registry](../Compiler/JetFoundation/Source/Registry/CLICommands.jet) |

The Jet-authored compiler under `Compiler/` is the production implementation;
it builds itself from an admitted seed compiler. See
[architecture](Spec/architecture.md) for the compiler-hosting boundary.

## Evidence and proposals

These directories hold dated material. They record what was found or proposed
at a point in time, not how Jet behaves now.

| Directory | Holds |
|---|---|
| [Audits](Audits/README.md) | Dated audit findings and retained evidence |
| [Research](research/) | Source-backed investigations and prior alternatives |
| [Proposals](proposals/) | Design alternatives linked to Tower decisions; not a work queue |

Check the source and exercise the behavior before relying on any claim here.

## Website and assets

| Directory | Holds |
|---|---|
| [site](site/) | The project website. `generate.jet` writes `Site/Dist/`, which also carries the per-diagnostic pages and the published trust-root keys |
| [assets](assets/) | The logo and visual design references |
