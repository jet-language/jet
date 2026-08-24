# E3 burndown session handoff 2026-08-15

E3 burndown session handoff (orchestrator, master b09b614dd). Master was UNBUILDABLE at session start: 56da7903f installed the DataTree decode helpers in jet-comptime without that tier's marshalling adapters, and the exact-JSON-number carrier had landed in the Prelude without syncing jet-jit's hand-mirrored enc_stream shim, so four crates failed to compile. Everything below was therefore invisible to the board until the build was repaired (8c5633c35).

CLOSED WITH BUILDER EVIDENCE (10): #1513 #1652 #1871 #1872 #1522 #1545 #1647 #1427 #1454 #1452.

SUITES NOW GREEN: fmt 186/186, fmt_project 27/27, diagnostic_snapshots 6/6, auto_derive_policy 16/16, jet_test 36/36, cli_expand 34/34, cli_runtime 28/28, jet_bench 5/5, generics 10/10, numops 12/12, numeric_widening 13/13, package_outputs 11/11, memory_semantics 11/11, grammar 45/45, cli_surface 45/45, refutable_test_bind 17/17.

~25 REAL DEFECTS FIXED, none of which the board recorded. The most consequential:
- Silent data loss: tree[root].children.push(child) compiled clean and did nothing. A speculative comptime probe stole sema's one-shot borrow_ctx flag, so an index-rooted mutating receiver was reclassified as an owning read and took an implicit copy; the push targeted a dropped temporary and jet_pool_get_mut was never emitted (b09b614dd).
- #Policy(no_alloc) was incompatible with declaring ANY struct: auto-derive's codec allocates and MemoryFacts rooted synthesized functions as policy roots (c697a15a8).
- Authority guard fired on the running command's own writes, because directory identity included size and mtime -- likely the cause of assorted "flaky" test behaviour (7fd0c4ef3).
- Five separate instances of one class, a name emitted without its definition: validate_json_number missing from the emitted jet_std; run() unmangled then __jet_run dropped by include_main; __JET_PACKAGE_EDITION absent from the fuzz harness; the command-suite root forwarders gated behind a used_core probe that a suite-as-parameter can never satisfy.
- Formatter losslessness: type bodies were printed as parallel vectors, rewriting any type whose fn preceded an in-body impl, and assoc_type_impls were dropped outright.
- I9 splits: imported generated codecs never lowered into the JIT program; list ordering had no JIT arm so a derived compare declined; the CLI banner terminator was decided three times.

KNOWN REMAINING SURFACE, all verified as pre-existing (each reproduces at 8c5633c35):
1. jit_coverage_audit: the ledger declares gaps: 0 but the audit OBSERVES 51 real gaps with concrete reasons, and the compile_covered set differs from observation by 80 entries in each direction. See the #1663 log for the full analysis. This is the epoch's largest honest gap and it blocks every I9 tier-parity claim.
2. tests/dev.rs cannot complete under any parallelism: three mutually recursive TIR lowering functions have 528/272/256 KiB debug frames (each is one multi-thousand-line match, and opt-level 0 emits no lifetime markers so the frame sums every arm), so ~2 levels of method nesting exhaust libtest's 2 MiB worker stack and the process aborts, reporting ~100 bogus cascade failures. Individual dev tests pass on the 8 MiB main thread, which is how criteria were proven this session. A second defect hides behind it: a JIT panic can escalate via runtime_stop_unwind's resume_unwind.
3. cli suite: 11 failures in cli_core (budget_*, inspect_unsafe_*, jobs_*, perf_report_*).
4. golden: 4 mismatches -- memory/string_view, text/duration_ns, tooling/provenance_track, types/dimensional_quantities_mass.
5. 64 e3 cards still open. The audit-first pass found most "ready" cards were shipped-pending-proof, so the binding constraint is machine proof, not implementation -- except M11 concurrency (#1560, #1561 have no code path) and M12 authority (#1566, #1567 are ABSENT: 31 effect roots ship where the card specifies 13, and no FFI.* leaf exists).

NEXT ACTION: mint one card per observed JIT gap from the audit output (#1663 warns against another umbrella), then fix the dev-suite stack frames, since without a runnable dev suite no I9 criterion can be proven at scale.


## Session addendum (later commits, master 6cfe66872)

FOUR MORE CARDS CLOSED: #1436, #1451, #1523, #1524. Total 14.

MORE DEFECTS FIXED, all pre-existing:
- FFI bridge crate did not compile, so every core.crypto example failed the front end with E0705 blaming the user's extern rust path -- including programs with no extern rust line at all. Two independent projections strip the same host-only block out of Outcome.rs; codegen re-supplies a replacement, jet-pkg-model does not, while its comment claims it matches. Fixed by removing the name dependency: the entropy shim no longer renders a report and is now closed over std plus itself (10be01662).
- jet fmt reported E0003 on every package.jet carrying a comment: the typed manifest formatter ran only as a fallback behind the module grammar, and it fails closed on comments by design so the decline was discarded (8acbb3454).
- tests/golden.rs handed the compiler an absolute entry path on one branch and a relative one on the other, because has_package_build_entry finds a SIBLING's fn build when an example directory has no manifest. 59 examples got an absolute source-map; two could notice it (8acbb3454).
- Four budget fixtures still wrote src/main.jet, retired since 2026-07-17 when the default-entry constant became run.jet; three further suites were red for the identical reason. The compile-workload provider also put the content-addressed runtime rlib cache inside the project cache its Clean sample deletes, so each of 42 cold child builds recompiled ~12,000 lines of runtime before touching the user program (90da049dd).
- A nonexistent user-named source reported E1334 "authority file is missing" because the authority layer answered before the loader could give the ordinary diagnostic (90da049dd).
- A bare foreign loop word still got the unused-value lie; the block-shaped spellings were already truthful (f9d5886a4).
- Three stale goldens, each proved against the emitted Rust rather than blessed: duration_ns line order, dimensional_quantities_mass second^2 -> ns^2, string_view untrimmed trailing whitespace (8acbb3454).

DEV SUITE: NOT FIXED, and the remaining approach is wrong. Two large refactors moved 974 lowering branch bodies into #[inline(never)] closures -- TIR side 14321cb6c, Cranelift side 6cfe66872 -- cutting per-nesting-level cost from ~804 KiB to ~144 KiB (TIR) and from 265 KiB to ~51 KiB (JIT). Both are worth keeping. But the overflow simply MOVED each time: resident_jit_safe_named_tuples -> cranelift_covers_float_lists -> body_only_edit_is_type_stable. Incremental frame shrinking is not converging, because the compiler's lowering is inherently a deep recursive descent over user syntax.

RECOMMENDED FIX, and it is a product fix rather than a test fix: run compilation on an explicitly sized stack, the way rustc itself does. rustc spawns its main work on a thread with a large explicit stack precisely because AST recursion depth is unbounded in user input. Today a user compiling a deeply nested program from any thread with a small stack hits the same wall, so this is not a test-harness problem -- the dev suite is just the first thing to notice. Put the sized-thread boundary at the compiler entry points (the driver's compile/check/run seams), not in tests/dev.rs, and then the suite needs no special treatment and neither does an embedder.

ALSO NEWLY FOUND, unfixed: the Panic effect root breaks the corpus. examples/features/effects/effect_grant.jet fails with E0712 "this #Caps region uses the effect Panic, which it has no capability for" and examples/features/lowlevel/polyglot_fortran fails with E0740 "run uses the effect Panic, which its signature doesn't allow". Panic ships as an effect root (Prelude/Effects.jet), so every #Caps region and every signature that can panic now needs to declare it. This lands squarely in M12: card #1567 already records that 31 effect roots ship where it specifies 13. Decide whether Panic is an effect root at all before fixing the examples -- if it is, the beginner surface just acquired a large new declaration burden, which is an owner call.

Also unfixed: examples/features/io/io_prelude.jet fails E0405 "?? return here needs a value".

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| `JIT-COVERAGE` | card | #1663 |
| `DEV-STACK` | card | #1319 |
| `PANIC-EFFECT-ROOT` | card | #1567 |
| `CLI-RED-SUITE` | card | #1362 |
| `STALE-GOLDENS` | card | #1302 |
| `IO-PRELUDE` | card | #1480 |
| `FOREIGN-LOOP` | card | #1887 |
| `OPEN-CARD-COUNT` | no-action | archived: a handoff count is historical state, not a separate product finding |
<!-- /audit-dispositions -->
