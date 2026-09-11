# Card #676 — compiler-latency video cross-check

Source: [M9UWgw_aW28](https://www.youtube.com/watch?v=M9UWgw_aW28).

This is an evidence crosswalk, not a second compiler-speed plan, profiler,
budget engine, or task runtime. The architecture and measurement owner remain
`docs/plans/compiler-speed.md` and #666. Follow-up owners remain #666 (speed),
#669 (self-host), #441 (trace), #241 (budgets), and #126 (task runtime).

## Changes in this pass

- `tools/perf/corpus.tsv` now pins a base source/golden and a separate
  representative-edit source/golden for every checked program.
- `tools/perf/dashboard.sh` now uses the same production runner for clean,
  no-change, and representative-edit JIT/AOT rows. An edit warms the base
  source, then measures the edited source in the same cache. The row receipt
  includes the source role, profile, backend, linker, cache hit/miss totals,
  generated-Rust bytes, artifact bytes, top measured phase, and stable JSON
  output (`--json` without baseline mutation). `JET_PERF_ARTIFACT_DIR` publishes
  the bounded inspection artifact.
- `tools/perf/ci-perf-check.sh` expects the six-row matrix and gates the exact
  rustc `-vV` and LLVM identities when a refreshed baseline is supplied.
- `tools/perf/construct-scale.tsv` records 21 sources across seven construct
  families at scales 1, 2, and 4. `tools/perf/dashboard.sh --construct-scale`
  measures those sources through the existing JIT/AOT runner and reports
  instantiations, glue units, generated-Rust bytes, artifact size, and adjacent
  superlinear growth.
- The existing dashboard now runs a pre-publication parity rail across every
  checked base/edit workload plus trait-object, effect-row, and closure probes.
  It compares JIT, `dev`, and release AOT output/effects, requires explicit tier
  receipts, and checks the registered diagnostic snapshot's code/why/fix text.
  Missing parity, a non-native speed tier, a changed release profile, or a
  weaker diagnostic rejects the matrix; this remains one #666 runner.

The committed baseline is intentionally not refreshed in this pass. It is a
version-2, four-row receipt and is stale against the new corpus contract. A
fresh #666 run must produce and review the replacement receipt before any speed
claim is made.

## Evidence crosswalk

| Exit | Evidence and result | Owner or uncovered gap |
| --- | --- | --- |
| 1 | The source ledger, transcript/article/comment separation, and inference labels were already met by the source mine. This pass does not change that ledger. | No new mechanism. Keep the existing #676 ledger. |
| 2 | The #666 corpus and runner have distinct clean, no-change, and representative-edit states. Production AOT receipts now expose `parse`, `sema`, `ffi`, `tir`, `emission`, `build_plan`, `backend`, and `link`; the existing row receipt carries source role/hash, cache hits/misses, toolchain, host, target, profile, linker, and variance identity. | **MET for the corpus contract.** The existing #666 twenty-sample runner remains the sole matrix publisher; no second benchmark mechanism is added. |
| 3 | `Source/CmdCompile.rs::hostile_cache_invalidation_matrix` proves final-cache misses for compiler, schema, target, profile, backend, flags, linker, Core, generated-code, and dependency identities; the existing source-edit test covers source invalidation. `Source/RuntimeCache.rs::hostile_cache_invalidation_matrix` covers schema/compiler/target/profile/backend/compile flags/generated source/Core/dependency keys. `hostile_cache_invalidation_matrix_linker_inputs_reuse_unaffected_runtime_and_core_artifacts` proves exact work selection: program edits hit both rlibs, Core edits rebuild Core only, runtime edits rebuild runtime plus dependent Core, compile flags rebuild both, and linker-only edits hit both rlibs. | Met by the existing #666/#669 cache seams; no second cache. |
| 4 | `tools/perf/construct-scale.tsv` has 21 checked-in rows: generic instantiations, bounded variadics, derives/reflection, large matches, closures, taskgroups/select, and Drop cleanup at scales 1/2/4. The shared dashboard's construct-scale report exposes `instantiations`, `glue_units`, `generated_rust_bytes`, `artifact_size_bytes`, and `superlinear_growth`, with JIT/AOT latency and output parity for every row. | **MET.** The matrix and report stay on #666's production runner. #669 remains the owner of compiler structure changes. |
| 5 | `tools/perf/dashboard.sh::run_parity_checks` gates publication on matching JIT/`dev`/release-AOT stdout/stderr for every checked base/edit workload and on explicit tier receipts. Trait-object, effect-row, and closure probes cover dynamic dispatch/boxing, effects, and feature loss; corpus speed rows require matching native JIT/`dev` tiers. `tests/ui/arg_type_mismatch.stderr` is checked as a snapshot prefix with `Error [E0112]`, `Why`, and `Fix` present across JIT, `dev`, and AOT diagnostics. `ci-perf-check.sh` rejects any row without the verified parity/profile receipt. | **MET for the pre-publication gate.** The parity rail remains inside #666's dashboard; no second speed runner or semantic mechanism is added. |
| 6 | `tools/perf/dashboard.sh --environment` now produces the checked-in receipt at `docs/reference/compiler-speed-environment-2026-08-25.json` from the current five-row corpus. It records exact compiler, rustc, target, LLVM, libc path/version/digest, hosted allocator and override digest, CPU topology/affinity/hardware digest, kernel/governor/load, toolchain digest, corpus/manifest hashes, and the AOT linker provenance fields emitted on builder rows. Its contract names `Clean`/`NoChange`/`Edit`, profile/backend, full workload identity, same-workload-only comparison, and rejection of unmatched workloads. | **MET on #666's canonical runner.** Keep linker/build isolation in the same runner; #669 remains the owner of self-host/build architecture. |
| 7 | `tools/perf/dashboard.sh --json` emits the bounded receipt without rewriting the stale baseline. Rows name the Jet source and role, phase totals, top cause, cache hits/misses, profile, backend, linker, generated-Rust/artifact sizes, and `full_artifact`; the header carries environment identity. A fresh production artifact root (`compiler-speed-artifact-1023`) contains source, golden, and timing JSON for clean, no-change, and representative-edit JIT rows. | **MET for this bounded explanation surface.** #441 owns separate parse/TIR/emission trace detail; no duplicate profiler is added here. |
| 8 | The new runner/corpus changes are limited to the existing #666 path. The uncovered gaps above are mapped to canonical owners, but no owner patch, targeted adversarial test, before/after measurement, or independent review has landed in this card. | Not met. Feed each row to its named owner; do not create a parallel subsystem here. |

## Hostile cache checklist

The required invalidation dimensions are: compiler identity, cache/schema
version, target, profile, backend, flags, Core artifact, source, dependency,
generated code, and linker input. Existing implementation anchors are
`Source/CmdCompile.rs` native cache salt/key construction,
`Source/BuildCache.rs` whole-program cache schema, `Source/RuntimeCache.rs`
runtime/Core inputs, and `crates/jet-codegen/src/Codegen/mod.rs` Core closure
fingerprinting. The hostile matrices now show one miss per changed key input,
reuse for unchanged program work, and exact runtime/Core action selection for
Core, runtime, compile-flag, and linker-only changes.

## Construct-scale matrix in #666

| Construct | Existing representative | Required scale axis |
| --- | --- | --- |
| Generic instantiations | `examples/features/modules/generic_modules.jet` | number of distinct type arguments and downstream uses |
| Bounded variadics | `examples/features/basics/variadics_spread.jet`, `examples/features/functions/variadic_method.jet` | argument count and nested forwarding |
| Derives/reflection | `examples/features/comptime/reflect.jet`, `examples/features/reflection/derive_loop.jet` | field count, derive count, and generated glue |
| Large matches | `examples/features/basics/pattern_matching.jet` | arm count and payload width |
| Closures | `examples/features/basics/closures.jet` | capture count and nesting |
| Taskgroups/select | `examples/features/concurrency/task_group.jet`, `examples/features/concurrency/select_generic.jet` | task/arm count and generic payload width |
| Drop cleanup | `examples/features/io/scope_guard.jet` | cleanup depth and aggregate width |

The checked matrix uses isolated fixtures under `tools/perf/constructs/`.
Each family has three checked-in source/golden pairs. The `axis` field records the
construct count that grows; `instantiations` records generic type-argument
instances; `glue_units` records generated construct units. The report measures
generated-Rust bytes and AOT artifact size, then marks an adjacent AOT latency
step as `yes` when its growth exceeds the axis growth. `baseline` marks the
first point in each family.
