# Card #1974 — Open criteria 3, 6, and 7

Date: 2026-08-22. Card: #1974.

## Result

The current source has two related but separate fact carriers. `SemIndex` is
the rich checked-program index used by inspect tools. `ProgramSemanticFacts` is
a smaller carrier used to build `ProgramInfo` for build reflection and
`core.compiler`. The inspect commands do not expose `ProgramInfo`, and the
build path does not consume `SemIndex`.

The execution split is real. A selected `fn build` runs in the host comptime
interpreter before runtime codegen. `jet build` and selected-build `jet run`
reach that path. Ordinary inspect commands only check and project source.
`jet run --interpret` and `jet dev --target=web` bypass programmable-build
staging, so they do not run the same `fn build` reflection/check path.

## Criterion 3 — fact carrier

`ProgramSemanticFacts` contains only solved function effects, panic reachability,
the fact registry, and the name ledger (`crates/jet-comptime/src/Comptime/Reflect.rs:19-25`).
`Driver::program_semantic_facts` fills those four fields from
`SemIndexEffectFacts`; it does not convert or attach a `SemIndex`
(`crates/jet-driver/src/Driver/mod.rs:4327-4358`).

`build_program_info` receives the checked `ProgramBundle` plus that narrow
carrier (`crates/jet-comptime/src/Comptime/Reflect.rs:2204-2215`). It walks the
bundle AST to build `types`, `functions`, and `packages` and adds the shared
fact registry to reflected type rows (`:2252-2327,2433-2440`). Function rows
read only the effect map and `reaches_panic` set from `ProgramSemanticFacts`
(`crates/jet-comptime/src/Comptime/Reflect.rs:2443-2494`).

`SemIndex` stores definitions, references, calls, effects, members, structural
nodes, definition facts, bypasses, instances, outputs, and optional package
facts (`crates/jet-semindex/src/Types.rs:442-463`). `build_index` builds those
rows by walking the bundle and applying `SemIndexEffectFacts`
(`crates/jet-semindex/src/Build.rs:924-953,1240-1243`). No call graph,
reference, structural, bypass, instance, or output row enters `ProgramInfo`.

The compiler API confirms the split: `core.compiler` builds `ProgramInfo` from
`program_semantic_facts`, then builds a separate `SemIndex` from the same
checked bundle and effect facts (`Source/Compiler.rs:754-771`).

**Finding:** criterion 3 is not satisfied by a shared `ProgramSemanticFacts`
seam. The source has one checked sema pass, but two projections with different
coverage.

## Criterion 6 — inspect projections

The grouped `inspect` argv is normalized to the existing leaf handlers
(`Source/main.rs:658-735`). The semantic handlers are then dispatched directly
(`Source/main.rs:1887-1958`). Their actual inputs are:

| Surface | Execution and source evidence |
|---|---|
| `semindex` | `jet_semindex::open` checks the entry with `Driver::check_file_with_effect_facts`, builds `SemIndex`, and renders JSON or counts (`Source/CmdSemIndex.rs:20-32`; `crates/jet-semindex/src/lib.rs:181-197`). This is check-only; it does not run `fn build`. |
| `expand` | One check produces `bundle` and `SemIndex`; every lens then reads those values (`Source/CmdExpand.rs:84-172,174-227`). Effects read `index.effects()`; memory and web read `SemIndexEffectFacts` (`Source/CmdExpand.rs:293-533`). No `ProgramSemanticFacts` or build staging appears. |
| `impact` | `open` supplies `SemIndex` to `ImpactReport::analyze`; the report walks references and call edges only (`Source/CmdImpact.rs:10-63`; `crates/jet-impact/src/lib.rs:43-71`). |
| `dossier` | The main dossier opens `SemIndex` and calls `idx.dossier` (`Source/CmdDossier.rs:119-177`). Its budget, command, and allocator supplement performs a second ordinary checked-front-end pass (`Source/CmdDossier.rs:180-215`). `module_explain` uses the same index path with one profile (`Source/CmdDossier.rs:13-31`). |
| `CmdInspect` | `guarantees` loads a bundle and collects `GateLedger`; it does not build `SemIndex` (`Source/CmdInspect.rs:511-584`). `provenance`, `digest`, and `env` read lock, registry, or config-model data (`Source/CmdInspect.rs:73-139,188-323,589-647`). |

These commands project the source closure that their check path loads. They do
not project the post-`fn build` runtime bundle or join a project check's input,
finding, and generated-source provenance.

**Finding:** criterion 6 has a usable `SemIndex` projection path, but no shared
`ProgramInfo`/`SemIndex` view and no post-build view.

## Criterion 7 — execution tiers

| Invocation or stage | Actual path | `fn build` fact/check behavior |
|---|---|---|
| Build stage | `prepare_build_front_end` runs sema, then `program_semantic_facts` and `build_program_info`; `run_build_entry_with_policy` evaluates the selected root in the comptime interpreter (`crates/jet-driver/src/Driver/mod.rs:2667-2705,2909-2930`; `crates/jet-comptime/src/Comptime/mod.rs:372-482`). | Runs once on the host, before runtime codegen. It is not a JIT or runtime-interpreter feature. |
| `jet build`, including `--target=web` | `CmdCompile` prepares and resumes the programmable-build front end (`Source/CmdCompile.rs:638-712,746-796`). | Uses the shared build stage, then emits the selected native or web target. |
| Default `jet run` with a selected build entry | The strict JIT branch excludes `selects_build_entry`; the later branch sends it to programmable build (`Source/CmdCompile.rs:489-505,761-775`). | It uses the build stage and does not run the selected build entry as JIT code. |
| Ordinary default `jet run` | The command calls strict Cranelift JIT (`Source/CmdCompile.rs:489-543`), whose check path loads, seeds facts, and runs ordinary sema (`Source/Interpreter.rs:527-604,765-897`). | No `ProgramSemanticFacts` or `fn build` evaluation is reached. |
| `jet run --interpret` | The interpreter branch runs before the selected-build routing and calls `run_interpreter_once...` directly (`Source/CmdCompile.rs:422-486`). That path only loads, seeds, checks, and runs the bundle (`Source/Interpreter.rs:971-1014`). | A selected `fn build` is not staged through `prepare_build_front_end`; this is an execution-tier gap. |
| `jet dev --target=web` | The web watcher calls `compile_web_with_gates_and_settings` directly (`Source/CmdCompile.rs:3897-4024`; `Source/lib.rs:1576-1598`). | It bypasses programmable-build staging. A web dev run does not reach `program_semantic_facts` for `fn build`. |
| Ordinary AOT effect summary | When no prepared build front end is reusable, `CmdCompile` runs another checked-front-end pass for the effect summary (`Source/CmdCompile.rs:874-897`). | It reuses `SemIndexEffectFacts`; it does not create `ProgramSemanticFacts` or run `fn build`. |
| Inspect commands | They run compiler/sema checks in the CLI process and serialize facts; they never enter AOT, Cranelift, tier-0, or web runtime execution (`Source/CmdExpand.rs:152-192`; `Source/CmdImpact.rs:37-63`; `Source/CmdDossier.rs:128-177`). | No runtime-tier proof exists in these projections. |

After build execution, the driver strips build-only entries and re-checks the
planned runtime bundle before codegen (`crates/jet-driver/src/Driver/mod.rs:3241-3293`).
That re-check does not invoke `build_program_info` again. Therefore the current
source has one host build-time reflection point, not a tier-neutral check API.

## Open-criteria conclusion

- Criterion 3: **open** — `ProgramSemanticFacts` is not `SemIndex` and does not carry its rich rows.
- Criterion 6: **open** — inspect projections use `SemIndex` or narrower command-specific sources, with no post-build join.
- Criterion 7: **open** — AOT and selected-build `jet run` use the build stage, but `jet run --interpret` and web dev bypass it.

This note records source evidence only. It changes no compiler, test, spec,
Tower, plugin, or git data.
