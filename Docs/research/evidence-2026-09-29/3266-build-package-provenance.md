# #3266 — Core build/package/debug provenance coverage (CORE-F042)

Date: 2026-09-29. Author: Closer12. No code change. Transcripts are under
`~/.cache/jet-dev/scratch/Closer12/prov/logs/NN-*.txt` (one file per command, with its exit code).

## Identity header (`logs/00-identity.txt`)

```
binary: /home/nate/.cache/jet-test-scratch/jet-debug-snapshot14
binary_sha256: 00b1e35e25ed941cdecb248ec8ffbfde0c1d5b48b563177443436288af2ad3fd
source_rev: e9c708fa7102d3e923a26e9ea432a52c34cf991d   (44 uncommitted paths in the shared checkout)
jet version: Jet 0.1.0, prerelease, editions 2026-2028, registry protocol v1
```

The snapshot binary predates the source rev, so nothing below claims the current tree builds clean.
Only the named binary was run. No archived receipt was used.

## Fixtures (copied unchanged into scratch)

- `Examples/features/packages/pub_package_visibility` + `pub_package_visibility_dep` (a path dependency)
- `Examples/features/packages/monorepo` (workspace with `find("./packages")` members hello and ranker)
- `Examples/features/foundations/tooling` (the only example that calls `core.build.graph`)
- `graph3/` is a minimal `fn build` that calls `build.graph` and `build.receipt_diff` with no `core.compiler`
- `diag/` is `pub_package_visibility` with its dep pointed at a missing directory

## Observed

| Row | Command | Result |
|---|---|---|
| Export discovery | `jet doc --json dep.jet` (dep package) | ok. `inspect.doc` JSON lists item `hidden`: `public:true`, `package_public:true`, signature `fn hidden() -> String`, failure contract `String (implicit default !Err)`, source span |
| Dependency resolution | `jet run run.jet` (path dep) | `same package` / `dependency package`, exit 0 |
| Lock + provenance | `jet inspect provenance --json` before fetch | E1202 `No lockfile found — run jet fetch first`, exit 1 |
| | `jet fetch`, then `jet inspect provenance --json` | dep `dep@0.1.0`, integrity `sha256-3995ff…9ddd` status `enforced` (E1204). Transparency, publisher and build are `not recorded` |
| Build facts | `jet build --json run.jet` | `jet.status/v1` diagnostics (L0105 unreachable export) plus `build.effects`: required `[IO]`, granted `[Exec.Args, IO, Mem.Alloc]`, authority `package.jet default`. The stderr trailer is `history: optional persistence failed … optional receipt job input path is outside its project root` |
| | `jet inspect build run.jet` | `default pipeline: no root fn build` |
| | `jet inspect output run.jet` | E2104 `The checked source has no selected Output` |
| Workspace members | `jet run packages/hello` (path form) | `hello from the monorepo`, exit 0 |
| | `jet run hello` (bare form, documented in `monorepo/workspace.jet:11`) | **E2105 `Can't securely read the file hello`**, exit 1. The documented form does not work |
| | `jet check` at the workspace root | E2104 `jet check is ambiguous — 2 workspace members can run` (hello, ranker), exit 2 |
| Build graph (`core.build.graph`) | `foundations/tooling`: `jet run run.jet` / `jet inspect graph run.jet` | **E3520 Two build entries** (`graph/graph.jet:1` and `run.jet:18`). The example directory is not runnable as shipped |
| | the same `run.jet` isolated (with `input.jet`, `package.jet`) | **E0956** `source Core call core.compiler.lex has no loaded signature` (and parse, check; `unknown checked host method tokens on Int`) |
| | `graph3/run.jet` (`build.graph` + `receipt_diff` only): `jet inspect graph`, `jet run`, `jet inspect explain-build graph_after` | **E0956 `duplicate checked type definition __comptime::BuildGraph…`** on all three. The named type changes per invocation (`BuildGraphFileDelta`, `BuildGraphCacheDelta`, `BuildGraphTarget`) |
| Diagnostics | dep path to a missing directory: `jet check run.jet` | E1334 `Authority file …/no_such_dep is missing`. The error blames an "authority file" for a missing dependency directory (actionability issue) |
| Features | — | No package fixture under `Examples/features/packages` declares features. Feature resolution was not exercised (unknown) |

## Proof command

`target-integ/debug/deps/package_outputs-d534153bb0d15007` (built 2026-09-29 01:59 from the shared tree),
run under the jetwork slice with a 6G cap and `--test-threads=2` → **aborted** by the 900 s suite
budget, after 11 FAILED and 14 ok. Re-running single failures with `--exact` confirms they are deterministic:

- `build_action_dependency_cycle_reports_the_full_chain`: panics at `tests/package_outputs.rs:842`; the
  CLI prints `E3502 Build plan is invalid: ActionDependencyCycle(alpha -> gamma -> beta -> alpha)`, which the test does not accept.
- `jet_build_accepts_an_explicit_package_directory`: `couldn't create the .jet/build folder safely: web output directory escapes the working directory`.
- `semantic_corpus_policy_runs_with_package_templates`: corpus manifest names unclassified selectors (`root:Examples/features`, …).
- `package_default_output_alias_invokes_nested_leaf_from_path_and_cwd`: `copy Jetpack fixture package.jet: No such file or directory` (`tests/package_outputs.rs:314`).
- `outputs_block_drives_jet_run_jit` passes alone (0.27 s), so part of the parallel failure set is contention [INFERENCE].

## Capability table

| Finding row | Status |
|---|---|
| Export discovery (`jet doc --json`, `CoreModuleExportNames` generated rows) | Supported |
| Dependency graph + lock integrity (`jet fetch`, `jet inspect provenance`) | Supported. Publisher, transparency and build provenance are "not recorded" for path deps |
| Dependency features | Unknown (no fixture) |
| Build graph query (`core.build.graph`, `jet inspect graph`, `explain-build`) | **Broken on snapshot14** (E0956 duplicate comptime type; the tooling example also hits E3520 and E0956 for core.compiler) |
| Build/effect facts (`jet build --json`, `jet inspect build`) | Supported |
| Workspace member addressing | Path form supported. Bare form documented but fails (E2105) |
| Diagnostics for missing lock, ambiguous member, duplicate build entry, dependency cycle | Present (E1202, E2104, E3520, E3502). The missing-dep text is misleading (E1334) |

Criterion 2 (retain approved package/flash/build-query work): this card changed nothing. `jet flash`,
`jet package`, `jet inspect query build` and `jet inspect explain-build` stay the owners. No gate is opened.
Criterion 3: no archived receipt was used and no registry was created. The identity header above says what ran.

## Verdict

PARTIAL. Criteria 2 and 3 are met. Criterion 1 ("demonstrate") is not met. Export discovery,
dependency/lock provenance, build/effect facts and diagnostics are demonstrated, but the build graph
fails on every command and feature resolution has no fixture. The named `package_outputs` proof also
fails. Each failure is filed as a defect.
