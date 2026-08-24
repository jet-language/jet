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
  output. `JET_PERF_ARTIFACT_DIR` publishes the bounded inspection artifact.
- `tools/perf/ci-perf-check.sh` expects the six-row matrix and gates the exact
  rustc `-vV` and LLVM identities when a refreshed baseline is supplied.

The committed baseline is intentionally not refreshed in this pass. It is a
version-2, four-row receipt and is stale against the new corpus contract. A
fresh #666 run must produce and review the replacement receipt before any speed
claim is made.

## Evidence crosswalk

| Exit | Evidence and result | Owner or uncovered gap |
| --- | --- | --- |
| 1 | The source ledger, transcript/article/comment separation, and inference labels were already met by the source mine. This pass does not change that ledger. | No new mechanism. Keep the existing #676 ledger. |
| 2 | The corpus and runner now have distinct base, no-change, and representative-edit states. The timing implementation still reports `load`, `sema`, `ffi`, `codegen`, `build_plan`, and `backend_link`; it does not yet expose separate parse, TIR, emission, and link records, and the plan's twenty-sample contract has not been freshly measured. | #666 owns the phase receipt and runner. Add real parse/TIR/emission/link phase boundaries and publish a fresh matrix. |
| 3 | Native cache salt code and tests cover several compiler, dependency, runtime, Core, mode, target, profile, backend, linker, and instance inputs. The existing evidence does not prove an affected-work mapping for every named input, especially schema, generated-code, linker, and artifact changes. | #666/#669 own cache architecture. Add a hostile invalidation matrix with exact hit/miss and affected-action assertions; do not add a second cache. |
| 4 | The checked examples exercise generics, variadics, reflection/derives, matches, closures, taskgroups/select, and cleanup-adjacent paths. No scale sweep records instantiation, glue, artifact-size, or superlinear growth. | #666 owns the corpus; #669 owns structural compiler consequences. Add scaled fixtures and report the four requested growth measures. |
| 5 | The runner now compares JIT and AOT output per scenario, so an edit with changed output is not incorrectly compared with its base output. There is still no receipt proving no boxing, dynamic dispatch, deoptimization, feature loss, weaker diagnostics, effect change, or JIT/dev/AOT divergence for the speed result. | #666 owns parity. Add adversarial semantic/diagnostic/effect checks and reject the row when any rail is missing. |
| 6 | The receipt now identifies OS, architecture, CPU count, host, kernel, memory, governor, target, compiler binary, rustc `-vV` digest, LLVM, profile, backend, and observed linker. It does not isolate libc, allocator, hardware topology, or unmatched-workload risk, and no official environment receipt was freshly published. | #666 owns the benchmark receipt; #669 owns toolchain/build isolation. Add libc/allocator/hardware provenance and workload-match checks. |
| 7 | The runner has bounded source names, phase totals, cache hit/miss counts, top-cause selection, stable JSON, artifact-size fields, and an opt-in full artifact directory. No fresh artifact or separate parse/TIR/emission cause report was produced. | #441 owns deeper trace format; #666 owns the bounded compiler receipt. Publish one fresh receipt and keep the full artifact explicit and inspectable. |
| 8 | The new runner/corpus changes are limited to the existing #666 path. The uncovered gaps above are mapped to canonical owners, but no owner patch, targeted adversarial test, before/after measurement, or independent review has landed in this card. | Not met. Feed each row to its named owner; do not create a parallel subsystem here. |

## Hostile cache checklist

The required invalidation dimensions are: compiler identity, cache/schema
version, target, profile, backend, flags, Core artifact, source, dependency,
generated code, and linker input. Existing implementation anchors are
`Source/CmdCompile.rs` native cache salt/key construction,
`Source/BuildCache.rs` whole-program cache schema, `Source/RuntimeCache.rs`
runtime/Core inputs, and `crates/jet-codegen/src/Codegen/mod.rs` Core closure
fingerprinting. These are evidence anchors, not a claim that the hostile
matrix is complete. The missing proof is one test per dimension showing the
unchanged action is reused and the affected action is rebuilt.

## Construct-scale matrix to add to #666

| Construct | Existing representative | Required scale axis |
| --- | --- | --- |
| Generic instantiations | `examples/features/modules/generic_modules.jet` | number of distinct type arguments and downstream uses |
| Bounded variadics | `examples/features/basics/variadics_spread.jet`, `examples/features/functions/variadic_method.jet` | argument count and nested forwarding |
| Derives/reflection | `examples/features/comptime/reflect.jet`, `examples/features/reflection/derive_loop.jet` | field count, derive count, and generated glue |
| Large matches | `examples/features/basics/pattern_matching.jet` | arm count and payload width |
| Closures | `examples/features/basics/closures.jet` | capture count and nesting |
| Taskgroups/select | `examples/features/concurrency/task_group.jet`, `examples/features/concurrency/select_generic.jet` | task/arm count and generic payload width |
| Drop cleanup | `examples/features/io/scope_guard.jet` | cleanup depth and aggregate width |

Until that matrix has measured rows, these examples are coverage pointers,
not construct-scale evidence.
