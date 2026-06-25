# c80 — `benchmark` target support

**Status:** ready to build. No new owner decision required — D-BENCH1 (surface +
runner) and D-TGT2 (reserved keyword) already settle every user-facing choice.
One I8 check to honor (below), not a blocker.

## Goal

Turn the reserved manifest target keyword `benchmark` into a working build
target. A package may declare `benchmark` in its `targets:` list; `jetpack`
then knows the package's artifact is a benchmark binary built and run by the
existing `jet bench` path. Today `benchmark` parses but is rejected with E1210
"has no backend yet".

## Relationship: `benchmark` target vs `#Bench` blocks (the core clarification)

These are **distinct, complementary, and must not become two ways to do one
thing (I8).**

- `#Bench "name" { … }` (D-BENCH1, c121, **shipped**) is the in-source surface:
  a region benchmark, exact sibling of `#Test`. `jet bench file.jet` discovers
  the blocks and times each (ns/iter + ops/sec). This is the *mechanism* — it
  already exists end-to-end (AST → sema `Bench` mode → harness codegen → runner).
- `benchmark` (D-TGT2, **reserved**) is a package-level *manifest target type*,
  the peer of `library`/`executable`/`test`/`example`. It is a declaration —
  "this package/entry is a benchmark" — not a second benchmarking engine.

The correct design: the `benchmark` target reuses 100% of the shipped
`#Bench`/`jet bench` machinery. It is to `#Bench`/`jet bench` exactly what the
`test` target is to `#Test`/`jet test`. The target points the existing engine at
a package entry; it does **not** introduce new benchmark codegen, a new runner
verb, or a new in-source surface. Anything else would violate I8 — D-BENCH1
already resolved the runner to `jet bench` with "NO new `jet test --bench` form".

## Current state (verified)

Targets:
- `Target` enum — `Source/Jetpack/PackageManifest/mod.rs:74-80` — `Library`,
  `Executable`, `Test`, `Example`. No `Benchmark` variant.
- Keyword reserved — `Source/Syntax.rs:1075` `TARGET_RESERVED = ["benchmark","plugin"]`.
- Parse + rejection — `Source/Jetpack/PackageManifest/ParseBlocks.rs:176-181`:
  known keywords map to `Target::*`; anything in `TARGET_RESERVED` →
  `ManifestError::ReservedTarget` (`mod.rs:175-177`).
- Diagnostic — `Source/Manifest.rs:351-356`, code **E1210**: "package `{name}`
  lists target `{value}`, which has no backend yet (reserved for a future
  increment)". Reserved-target test at `mod.rs:374-384`.
- **How targets are consumed today:** only via `package_kind()`
  (`mod.rs:63-70, 207`), which collapses the target list to `PackageKind::Library`
  vs `Executable`. There is no per-`Target` build dispatch. `test`/`example`
  targets are parsed but have no dedicated realize backend either — they ride the
  lib/exe split. Realize reads the store at `Source/Loader.rs:72-85` (library
  stages source with empty `bin`; executable installs a binary). The rustc call
  (`Source/CmdCompile.rs:671-723`) always emits a binary (`-o bin`); no
  `--crate-type`.

`#Bench` / `jet bench` (shipped, the engine to reuse):
- AST — `Source/AST.rs:521-523, 868-873` `Item::Bench(BenchDef)`.
- Parser — `Source/Parser/Items.rs:369-370, 618-633`; `Modules.rs:219-220`.
- Sema — `CompileMode::Bench` (`Source/Sema/Registration.rs:361`,
  `Source/Sema/Bundle.rs:870-873`); bodies checked in Bench mode at
  `Bundle.rs:1484-1487`.
- Codegen — `Source/lib.rs:362-369` `compile_benches_with_path`; emits a harness
  binary whose generated `main` warms up, auto-scales, times each region.
  Detection: `lib.rs:386-395` `has_bench_blocks`.
- Runner — `jet bench` at `Source/main.rs:698`, `Source/CmdDevTools.rs:638-749`:
  `run_bench` → if `has_bench_blocks` → `run_bench_regions` (per-region report),
  else whole-program timing.
- Example/test — `examples/features/105_bench.jet`; `tests/jet_test.rs:41`.

## Decision context (ratified)

- **D-TGT2** (2026-06-21): shipped targets are library/executable/test/example;
  `benchmark`/`plugin` reserved, rejected with a "no backend yet" message (not
  unknown-keyword). `syntax-decisions.md:1781`.
- **D-BENCH1** (2026-06-24): bench surface = `#Bench "name" { … }` blocks, run by
  the existing `jet bench`; no new verb. "Reserved target `benchmark`
  (TARGET_RESERVED) stays reserved." `syntax-decisions.md:2597`, `Syntax.rs:645-651`.

Together these fully specify c80: the keyword, the message, the surface, and the
runner are all decided. c80 is wiring, not design.

## Implementation (staged)

1. **Define what a benchmark target compiles to.** A `benchmark` target =
   a package target whose entry module's `#Bench` blocks are compiled by
   `compile_benches_with_path` and run by `jet bench`, producing a benchmark
   binary artifact. No new codegen. Add `Target::Benchmark` to the enum
   (`mod.rs:74-80`). It does **not** map to a `PackageKind` (it is neither an
   importable library nor a PATH-installed executable) — `package_kind()` ignores
   it, same as `Test`/`Example`. Write the failing test first (ui fixture for the
   removed rejection + a manifest-parse test asserting `Target::Benchmark`).

2. **Backend wiring (parser/manifest).** In `ParseBlocks.rs:176-181` add
   `k if k == "benchmark" => Target::Benchmark` and drop `"benchmark"` from
   `TARGET_RESERVED` in `Syntax.rs:1075` (leaving `["plugin"]`). The
   `entry:`/`name:` target-block fields (D-TGT4/D-TGT3) already parse — a
   benchmark target uses `entry:` to name the module carrying `#Bench` blocks,
   exactly like the other block-form targets.

3. **`jet bench` / realize integration.** The runner already exists; the target
   only needs to be discoverable. Decide the minimal honest scope:
   - Multi-target/`jetpack`: when a package declares a `benchmark` target, `jet
     bench` (or `jetpack bench`, if a verb is added — prefer reusing `jet bench`
     per D-BENCH1/I8) resolves the target's `entry:` and runs the existing
     region path. No new artifact-install semantics needed; a benchmark is run,
     not installed (mirror how `test`/`example` are run, not staged).
   - Guard: a `benchmark` target whose entry declares **no** `#Bench` blocks
     should fall back to whole-program timing (existing `run_bench` behavior) or
     warn — pick the existing `run_bench` fallback to avoid a new diagnostic.

4. **Diagnostics + snapshots (I4).** `benchmark` is no longer reserved, so its
   E1210 rejection path must stop firing for it. Keep E1210 for `plugin` only.
   Update the reserved-target test (`mod.rs:374-384`) to use `plugin`. If a new
   diagnostic emerges (e.g. "benchmark target's entry declares no `#Bench`
   blocks"), it needs a code in `docs/spec/diagnostics.md` + a `tests/ui`
   snapshot — only add if step 3 chooses a hard error over fallback.

5. **Example + golden (I5).** Add a package (under `examples/`) with a
   `benchmark` target in its `pkg.jet` `targets:` list, an entry module carrying
   `#Bench` blocks, and expected output the golden harness enforces. Reuse the
   shape of `examples/features/105_bench.jet`.

6. **Tests.** Manifest-parse test (`Target::Benchmark`), the moved
   reserved-target test, a `jet bench` integration test against the example
   (mirror `tests/jet_test.rs:41`), and `tests/decisions.rs` stays green.

7. **Docs.** Update `docs/spec/syntax-decisions.md` D-TGT2 row to note `benchmark`
   shipped (keep `plugin` reserved); extend the `#Bench` section in
   `docs/spec/spec.md:526` to state the target/block relationship; note the
   E1210 list now holds `plugin` only.

## Sequencing / gates

- **No upstream gate.** D-BENCH1 + D-TGT2 are ratified; the engine ships. Build
  now.
- Do c80 **before** wiring any future `test`/`example` realize backend so the
  pattern ("non-kind run targets ride the existing verb") is set once and reused.
- Keep `plugin` in `TARGET_RESERVED` and its E1210 path intact — c81 owns it.

## Open Owner-Q

None blocking. One thing to keep honest, not a question:

- **I8 watch.** The `benchmark` target must remain a manifest *pointer* at the
  shipped `#Bench`/`jet bench` engine — not a second benchmarking mechanism, not
  a new runner verb (D-BENCH1 already rejected `jet test --bench`). If
  implementation pressure pushes toward a separate `jetpack bench` artifact path
  with its own codegen, stop and re-confirm against I8 before proceeding.
