# Compiler algorithms, selection, and maintainer assurance

Date: 2026-09-10. Method: `mine-for-jet`. Mining owner: Tower #3009.

## Verdict

**Keep the ambition. Change how Jet establishes it.** Eliminate avoidable superlinear work, hidden copies and repeated analysis. Preserve mathematical lower bounds, input assumptions, semantics, safety and every required execution mode. A linear bound is not a synonym for fast, and a green test count is not a synonym for trustworthy.

The right extension is a **maintainer-only algorithm view over Jet’s existing source-derived inventory and evidence records**, with optional measurement through existing test machinery. It is not another benchmark framework, handwritten API catalog, runtime instrumentation layer, public annotation family or end-user command.

This investigation produced a concrete wrong-answer case. A freshly rebuilt production comptime library returns `min=10` and `max=2` for map values `[2, 10]`. It orders display strings instead of numeric values. The same path works for `[1, 2]`. Tower #3013 owns the fix and cross-mode proof.

It also produced a useful optimization experiment. Adaptive order detection plus standard-library selection matched the current quantile kernel in 117,216 bounded cases. It substantially reduced measured cost on several large inputs, but a small sorted case regressed. This is evidence for further work, not a qualified Jet speedup. Tower #3012 retains the loss.

**Jet is not established as ready for bootstrap, general production, or critical-system qualification by this work.** The fresh main compiler build failed. Existing composed, measurement, professional-handoff and bootstrap gates remain open. Passing those gates would establish their stated scope, not prove every possible program bug-free.

This report completes research and records implementation work. It does not claim implementation completion. No compiler, library, test-suite or public API source was changed. Temporary programs exercised real code. Their source and results are retained here; scratch files are removed at closeout.

## Scope and evidence quality

The owner asked for both playlist entries, compiler and library algorithm design at every abstraction level, runtime/build/codegen improvements, a complete first-party audit approach, test integration, one source of truth, peer assurance practices, AI-code trust, critical-system readiness, bootstrap readiness, and the full beginner/expert/tooling/ecosystem goals. The traceability table below accounts for each.

Playlist: [Jet Research Queue](https://youtube.com/playlist?list=PLXgiJybdlzvk&si=gcrXhsjPMJZo1a9g), unlisted, two entries at capture. The prior-art checker reported both source IDs as new.

| Source | Publication metadata | Captured body | Audience capture |
| --- | --- | --- | --- |
| [Handling Infinite Variables in a Compiler](https://www.youtube.com/watch?v=YlFYXewYJ8M), Premature Abstraction | 2026-09-09; 18:20 | Automatic English and original-English JSON3 tracks; 514 normalized timestamp lines | 63 unique records: 37 roots, 26 replies |
| [The Smartest Algorithm No One Uses](https://www.youtube.com/watch?v=5JXpNOZWAHM), PurpleMind | 2025-08-29; 38:30 | Automatic English and original-English JSON3 tracks; 964 normalized timestamp lines | 167 unique records: 89 roots, 78 replies |

The full captured arguments were reviewed in bounded transcript sections. Audience review used top-liked roots, recent roots, low-visibility technical comments, substantive replies and corrections. Retrieved counts match the API metadata, but this does not prove coverage of deleted, hidden or later comments. Likes were discovery aids, not votes on correctness.

Capture limits:

- No usable creator-authored English subtitle track was available. Automatic-caption terms such as “nplete,” “risk” and “CISK” were not treated as exact technical spelling.
- The shared browser daemon was unavailable. `yt-dlp` retrieved metadata, captions and comments. Its version, 2026.03.17, warned that it was over 90 days old.
- Full video downloads failed with HTTP 403. An alternate format attempt also failed. Selected YouTube storyboard contact sheets were inspected, including the selection timing/comparison graphics. This is **not** complete frame-by-frame review, and tiny on-screen code/formulas were not transcribed as certain evidence.
- The original BFPRT paper scan had incomplete text extraction. The CMU lecture supports the recurrence, and the Princeton publication record supports the paper’s exact published comparison bound. The complete original proof was not reconstructed from the scan.
- Video and Cohen blog share the CMU/BFPRT provenance. They do not count as independent confirmations. The blog’s graph excludes pivot-selection work, unlike a whole-operation timing result.
- One helper received an inconsistent `tests/jet_measure.rs` view. A fresh read and line count established the actual 101-line file; the inconsistent view was discarded. A Python-kernel compiler launch also lacked `rustc` on its path; compilation succeeded through the project shell instead. Neither failed tool path is presented as product evidence.
- Scratch captures lived on disk under `~/.cache/jet-test-scratch`, not `/tmp`. The retained record is this report, its normalized claim appendix, source identities, commands, experiment source and outputs. Raw media/comment captures and temporary programs are temporary; this report is their durable evidence summary.

### Audience signals worth keeping

Compiler discussion separates four useful signals from noise:

1. `UgxGzUqFOCImu06IXf94AaABAg` challenges “NP-complete means impossible.” Its own “not polynomial” wording is also too strong without the usual complexity assumption.
2. `UgxDytJAtKQgO4QdBhB4AaABAg` points out tractable interval-graph coloring. Real allocation also has spill, split, coalescing and fixed-register constraints.
3. `UgwZPLHRUSmGlT_M-VN4AaABAg` challenges the cycle-count animation. The exact replacement estimate is not portable either. `UgwTaUd4zt8mzAgoswB4AaABAg` notes operating-system and floating-point calling-convention differences.
4. `UgwKvKmlAB8DMNKeUZ54AaABAg` suggests learned candidates with validity checks. The creator’s reply `Ugz8IAYq-iOofCOvbL54AaABAg.AaY9fRNgbc3AaYFlSSfo_1` correctly distinguishes embedding similarity from semantic equivalence. Register-renaming discussion under `UgzfsXGe65AANZwmumt4AaABAg` concerns hardware, not an unlimited supply of compiler-visible registers.

Selection discussion adds adversarial inputs, hard deadlines, duplicate handling, group-size constants, in-place implementations and the comparison-model qualification. Examples include `UgzaLXX2P9HeB_fasrx4AaABAg`, `Ugwrb2ZGk74p0jH0n5V4AaABAg`, `UgxSa8fE7HzdnryGcCh4AaABAg` and `UgwUqZIgXc9uvHGU2Y54AaABAg`. These are leads. Formal bounds come from primary sources; application anecdotes are not safety or performance evidence.

## What the sources actually establish

### Compiler video

The causal chain is sound when scoped: virtual values must become target instructions and physical resources; instruction choice changes register demand; allocation changes spill traffic; scheduling changes live ranges and stalls; ABI obligations constrain every stage. Local cleanup can become useful again after another lowering exposes new patterns.

The corrections matter more than the dramatic framing:

- NP-complete does not mean unsolvable. Restricted instruction-selection and allocation subproblems can be solved exactly. Unrestricted semantic equivalence, finite optimization problems and target-specific scheduling are different questions.
- “JITs use linear scan” is not a universal rule. Jet’s JIT delegates machine allocation to Cranelift/regalloc2. Its AOT path emits Rust, so rustc/LLVM own the eventual machine allocator. Jet should first improve the facts and shape it hands those backends.
- Multiplication-to-shift, compare-to-test and zeroing rewrites require exact checks for signedness, overflow, flags, traps, memory effects, error observations and target behavior. Fewer instructions alone is not an acceptance criterion.
- A bounded heuristic can be deterministic and well justified. Calling it guessing obscures its contract. Conversely, a learned candidate is not correct because it resembles known good code.

Primary checks: [LLVM code generator](https://llvm.org/docs/CodeGenerator.html), [Cranelift documentation](https://cranelift.dev/), and the [pinned Cranelift 0.112.3 allocation path](https://docs.rs/cranelift-codegen/0.112.3/src/cranelift_codegen/machinst/compile.rs.html).

### Selection video

Selecting a rank asks less than sorting every element. Randomized quickselect recurses into one partition and has expected linear work over its random choices, even for a fixed worst-case input. Repeated extreme pivots can still produce quadratic work.

The textbook groups-of-five construction gives

`T(n) <= T(n/5) + T(7n/10) + c*n`,

up to rounding and additive terms. Recursive-size mass is below one, so the bound is linear. The standard groups-of-three recurrence does not contract that way. Five is sufficient for this construction, not a universally fastest implementation constant. The original PICK paper’s `5.4305n` comparison bound is a separate, more precise result.

The comparison-sorting lower bound assumes the comparison model. Radix/counting/bucket methods exploit other assumptions, whose word width, key range, preprocessing and space costs must be counted. An exact answer that depends on every input generally still requires inspecting every relevant input. Arbitrarily large integers and strings do not have constant-cost comparisons.

The title is false literally. [Rust’s current slice API](https://doc.rust-lang.org/std/primitive.slice.html#method.select_nth_unstable) and [implementation](https://doc.rust-lang.org/src/core/slice/sort/select.rs.html) use introselect with a median-of-medians-family, Tukey-ninther fallback. That does not mean textbooks’ always-BFPRT implementation is the best default.

The API contract is equally important: in-place, unstable selection returns left/selected/right partitions; those partitions are not sorted; invalid ranks panic; comparator/key variants require an appropriate ordering. Statistical even median is not merely one selected middle element. Jet’s checked quantile also has finite-input, interpolation, error-precedence and input-preservation contracts.

Primary checks: [CMU lecture](https://www.cs.cmu.edu/~15451-f23/lectures/lecture01-filled.pdf), [Cohen article](https://rcoh.me/posts/linear-time-median-finding/), [BFPRT publication record](https://collaborate.princeton.edu/en/publications/time-bounds-for-selection/).

### Cross-resource synthesis

One shared lesson survives the different domains: **optimize the work required by the requested result, then measure the complete job under its real constraints.** Do not confuse a lower bound, a heuristic, an empirical fit or a benchmark winner.

The normalized ledgers contain 104 claims and 92 exact topics. They have one independently repeated topic, `total-job-cost`, and one conflict, `selection-usage`, resolved by Rust’s implementation. The ten micro-category comparisons are Main’s synthesis, not falsely counted as independent discoveries. The appendix preserves the classifications and owners.


## Live evidence and verified defects

### F1. Current main compiler build is broken

Command:

```text
CARGO_INCREMENTAL=0 scripts/agent/jet-env cargo build --bin jet -j 4
```

Expected: a freshly built current compiler that can run the public Jet probes.

Observed: exit 101, `E0061` at `crates/jet-codegen/src/Codegen/MIRWeb.rs:5695`. The call supplies `program, args`; `js_call_values` at line 7435 also requires the `MirFunction` and write-alias argument. The working tree was already extensively modified. This report neither attributes authorship nor treats the failure as a clean-release regression.

Tower **#3010** owns the remaining caller migration and the focused Web/alias proof. **#2919** remains the composed campaign owner. No stale `target/debug/jet` run was used to claim current public behavior.

### F2. Production comptime map extrema return the wrong numeric answer

The blocker above does not prevent building `jet-comptime` itself. A throwaway Cargo program linked the actual path dependency and called `Comptime::Builtins::apply_method`. Its final verdict inspects typed `CtValue::Present(CtValue::Int(...))`, not a substring of printed output.

```text
[1, 2] min: expected=1; actual=1
[1, 2] max: expected=2; actual=2
[2, 10] min: expected=2; actual=10
[2, 10] max: expected=10; actual=2
[2, 10] top_n(1): [__jet_ { __jet_key: 1, __jet_value: 10 }]
numeric extrema mismatches: 2
exit: 1
```

The cause is visible in [Builtins.rs](../../crates/jet-comptime/src/Comptime/Builtins.rs), lines 2363-2372: `min_by`/`max_by` compare `jet_show()` strings. The runtime [Collections.rs](../../crates/jet-codegen/src/Prelude/Core/Collections.rs) extrema kernel uses `V: Ord`. The public method type contracts are registered in [foundation Collections.rs](../../crates/jet-foundation/src/Collections.rs), lines 1910-1913 and 1926.

This proves a wrong answer in the freshly rebuilt production library entry point. It does **not** prove which public syntactic paths currently reach it in every execution mode. Tower **#3013** requires that complete reproduction and clean cutover after #3010, with no display-string ordering fallback.

The nearby `top_n` implementation is also duplicated. Runtime keeps a bounded sorted prefix; comptime materializes and sorts all entries and falls back to display ordering when comparison fails. The latter is an unproved semantic escape, not an acceptable way to recover a comparator error. Numeric `top_n(1)` worked in the displayed contrast; a broad `top_n` wrong-answer claim is not made.

### F3. Plain selection is not a universal sorting replacement

A Rust experiment compared `sort_unstable` with `select_nth_unstable`, checking rank, partitions, multiset preservation, duplicates and the even-middle pair. At `n=100001`:

| Input | Selection / sorting time | Observation |
| --- | ---: | --- |
| Random | 0.0992 | Selection did much less work |
| Sorted | 3.8391 | Selection lost |
| Reverse | 1.5260 | Selection lost |
| All equal | 3.7065 | Selection lost |
| Seven distinct values | 0.8128 | Selection won this sample |

Comparison counts were recorded separately. For random `n=100001`, sorting used 1,727,562 comparisons and selection 205,425. For sorted input, sorting used 100,000 and selection 205,456. Modern sorting already recognizes useful input structure.

This is a same-host Rust experiment, not a Jet/Rust competitive result. It refutes the blanket optimization rule before that rule reaches production.

### F4. A narrower adaptive statistics candidate is promising, not qualified

The current [DataStats.rs](../../crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs), lines 313-348, validates, clones, sorts and interpolates adjacent ranks. `median` delegates to `quantile(0.5)` and changes the error operation label.

The experiment compiled the exact production helper and checked-quantile bodies with **harness-only error carrier declarations**. A candidate combined finite/order discovery, direct indexing for ascending/descending input, and Rust selection otherwise. It preserved the original interpolation expression and zero normalization. The invalid-input route reused the production policy, so those comparisons are preservation checks, not an independently proved error specification.

The bounded differential used arrays of length 0 through 6 over `[-2, -0, +0, 1, 4]`, six quantiles, and named NaN/infinity/extreme/invalid-q cases. **117,216 cases passed** for result bits, returned errors and unchanged inputs. This does not cover all floats or every reachable program.

Each timing case alternated the two methods over eight trials, discarded the first, and reported the median of seven. Timed work included input validation, cloning where used and the operation itself. Inputs were prepared outside the timer. Results were consumed through `black_box` and checked. No affinity/isolation campaign or independent repeated session was performed.

| Input | n | Production sort, ns | Adaptive selection, ns | Candidate / baseline |
| --- | ---: | ---: | ---: | ---: |
| Random | 100001 | 1,907,625 | 206,323 | 0.1082 |
| Sorted | 100001 | 300,081 | 81,525 | 0.2717 |
| Reverse | 100001 | 316,543 | 81,505 | 0.2575 |
| All equal | 100001 | 305,172 | 81,536 | 0.2672 |
| Seven distinct values | 100001 | 699,202 | 359,475 | 0.5141 |
| Sorted | 1001 | 822 | 841 | **1.0231, loss** |

The small loss is not averaged away. Neither these medians nor a better big-O bound establish a universal win. Tower **#3012** requires independent numeric cases, missing input partitions, real Jet modes, allocation evidence and the standing performance gate before any qualified claim.

`describe`, lines 384-401, also recomputes sum/mean and variance/stddev through separate calls. Start by reusing already-computed scalar results. Do not casually replace compensated summation, reassociate floating arithmetic, or change which error is returned first. This additional same-kernel work is also in #3012.

### F5. Existing assurance machinery passes its own controls, not product qualification

Executed:

```text
scripts/agent/jet-env node scripts/agent/compiler-proof.mjs --check --json
exit 0: jet.compiler-proof.v1, status valid, compiler-proof.identity-direct proved

scripts/agent/jet-env node scripts/agent/test-economics.mjs --self-test --json
exit 0: PASS; all 6 false-green controls rejected
```

The six controls are zero-valid, all-excluded, discarded-output, broken-oracle, wrong-identity and harness-error. This is useful exercised evidence about the tools.

It is not a new proof of all compiler operations. Tower #2925 explicitly records a bounded proof boundary. #2952’s implementation is done, but its production qualification evidence records dirty/unqualified state and uncovered obligations. #2919 and #2858 own the outstanding composed and measurement runs. No test was removed on the basis of this self-test.

## One maintainer mechanism, not another subsystem

### F6. Extend the records and runners Jet already has

The existing implementation points are:

| Existing mechanism | Keep it responsible for |
| --- | --- |
| [Core source ledger](../../scripts/agent/check-core-surface-ledger.mjs) and executable syntax/collection registries | Discovering the public denominator and reconciling declarations with implementation |
| [Test economics](../../tools/agent-eval/test-economics/test-economics.mjs) | Joining obligations, defect shapes, suite costs, mutations, exclusions and replacement evidence |
| [`.measure` test coverage](../../tests/jet_measure.rs) and existing `jet test --measure` | Optional measured test claims; ordinary runs still check the claims’ behavior |
| [Comparison journal](../../proof/compiler/observations/comparison-journal.mjs) | Typed, identity-bound reference/candidate observations and honest non-universal verdicts |
| [Composition contract](../../proof/compiler/composition/contract.json) and [runtime contract](../../proof/compiler/runtime/contract.json) | Binding claims to the candidate, real implementation, modes, assumptions and unsupported edges |
| Existing Gauntlet and compiled-workload gate | Matched peer/whole-job measurements, not a second set of workload definitions |
| Golden/UI/property/fuzz and mode-agreement helpers | Executable correctness and diagnostics; independent expected behavior |
| Tower | Plans, owner decisions, uncovered work and qualification gates |

**Proposed flow:** source and registered operation identities identify the obligation; existing tests and workload fixtures exercise it; the existing measured mode collects optional cost evidence; the existing journal/receipt binds it to the candidate; test economics and Gauntlet query those same records.

This does not make compiler-internal auditing part of a user’s application. Heavy collection is selected by maintainers. Native compiler kernels keep native harnesses; Jet programs keep `#Test` and `.measure`. The integration is shared operation/evidence identity and runner selection, not forcing every test into a new language or runner.

No new `#Complexity`, public `jet audit`, custom proof language, source annotation requirement, always-on timer, background service or parallel catalog is proposed. No new end-user knob is needed to choose BFPRT, an allocator or a cache representation.

### F7. Complete inventory means accounting for omissions, not benchmarking only attractive APIs

Tower **#3011** owns the implementation of this extension. Its scope is all first-party compiler, codegen, runtime, Prelude, CoreLib and exposed API operations, including private support algorithms, generated forms, generic instantiations, delegated work and target-specific variants.

Discovery should classify every candidate callable or cost-bearing operation as a primary operation, helper of an operation, delegated operation, generated instance, platform variant, or a reviewed non-cost-bearing case. The public API registry alone cannot discover all private compiler work. Function counts alone do not identify algorithm boundaries. Unresolved ownership stays uncovered.

Do not benchmark every helper in isolation merely to increase counts. Link helpers to the smallest observable operation that exercises their cost. A generic algorithm needs representative key/value/callback classes plus its stated assumptions, not an impossible enumeration of all types. The source-derived denominator must still retain every supported variant and required target.

| Area | Dimensions and cases that must remain visible |
| --- | --- |
| Lexer/parser/formatting | Input bytes, tokens, nesting, malformed prefixes, source locations, output size |
| Types/sema/contracts | Items, constraints, type depth, overload/trait candidates, solved and unresolved facts |
| Queries/modules/build graph | Nodes, edges, fan-in/out, cycles, change frontier, environment and source edits |
| TIR/MIR/optimization/codegen | Blocks, edges, values, uses, loops, live pressure, pass iterations, target and ABI |
| Exact numerics/statistics | Element count, bit width, distribution, conditioning, finite/nonfinite and rounding policy |
| Text/Unicode/encoding/regex | Bytes and code points separately, pattern size, captures, malformed data, output expansion |
| Collections/memoization | Size, capacity, density, key comparison/hash/clone cost, duplicates, requested output size |
| Files/parsers/serialization | Input/output bytes, seeks, syscalls, streaming state, partial reads, limits and errors |
| Concurrency/async | Tasks, contention, schedules, cancellation, cleanup, retained memory, tail latency |
| Networking/security boundaries | Payloads, connections, protocol states, limits, adversarial inputs and foreign calls |
| Compute/data/graphics/media | Shape, precision, sparsity, transfers, allocation and output requirements |
| Packages/tooling/editor workflows | Cold/warm builds, edit kind, cache validity, diagnostic latency and repair action |
| Platform/FFI/web/embedded | Host/target, ABI, marshalling, unsupported calls, resource bounds and hardware assumptions |
| Real applications | Web, games, CLI/scripts, data analysis, services, AI/ML, GUI and embedded workloads |

A pilot validates the mechanism; it does not redefine the full acceptance scope. The remaining denominator stays explicitly uncovered until addressed.

### F8. Record what a cost claim actually means

Each record needs the existing operation and candidate identity plus these facts. These are proposed additions/joins, not a new standalone schema implementation:

- Input dimensions, output dimensions and operation boundaries. Include preprocessing, materialization, teardown and repeated-call amortization.
- Cost model: comparisons, bit operations, bytes moved, allocations, retained/peak memory, syscalls, work/span or measured time. Callback/key work is not free.
- Bound class: worst-case, expected, amortized, output-sensitive or measured scaling. Record assumptions and a derivation/proof reference. Empirical regression never becomes an asymptotic theorem.
- Input families: tiny and large, ordinary and adversarial, monotone, repeated, sparse/dense, malformed, boundary and resource-exhaustion cases where applicable.
- Exact implementation/helper closure, toolchain/checker/oracle identity, compilation flags, profile, target/ABI, execution mode, fixture seed and measurement environment.
- Correctness observations and oracle provenance. Retain errors, mutation, input consumption, effect order, cleanup, schedule and allowed nondeterminism, not stdout alone.
- Raw samples and aggregation rules, timeout/error status, uncertainty, operation counters, allocations and whole-job impact. Show unmeasured costs as unmeasured.
- Claimed scope, unsupported cases, known gaps, invalidation inputs and the Tower owner of incomplete work.

For every superlinear record, ask whether the requested result requires that work, whether a better algorithm exists under the same assumptions, and whether representation or repeated work is the real cause. Do not demand `O(n)` for comparison sorting, dense pairwise output, growing-width arithmetic or arbitrary optimization problems.

### F9. Staleness must invalidate the claim, not silently refresh a label

Reuse #2943’s candidate identity rather than inventing an algorithm-specific freshness service. Invalidate on changed implementation or helper bodies, generated sources, semantic contracts, dependency/toolchain/flags, target, oracle, fixtures and measurement procedure. For hardware measurements, bind the environment too.

Source discovery should run cheaply as part of maintainer correctness work. Heavy measurement remains optional and scheduled. A changed operation may pass ordinary correctness while its performance claim becomes stale. It must not keep a current green performance label.

Negative controls must reject an omitted operation, broken expected value, zero valid cases, all exclusions, discarded output, stale helper, wrong binary/target, changed flags, invalid input and harness failure. Interrupted and inconclusive measurements are not passes. Use the existing test-economics controls as the starting point.

### F10. Consolidate by preserved meaning and detection value

Reuse one operation implementation, one observation tuple, one source identity and one workload definition. Generate views for test economics, Gauntlet and qualification from those facts. Do not maintain independent fact catalogs or a parallel Markdown inventory. Generated views remain disposable projections, not owners of facts or work state.

Merge duplicate tests only when they detect the same defect shape under the same relevant conditions. Preserve unique UI, golden, error, mode, mutation, schedule and backend detectors. Shared fixtures do not imply identical oracles. A source-wiring assertion is not a substitute for an observable contract test.

The cheapest sound improvement often removes duplicate work: compute a statistic once; build a predecessor map once; propagate only newly changed facts; reuse an existing standard-library algorithm. A new solver, optimizer IR, algorithm DSL or general instrumentation framework needs evidence that the existing mechanism cannot carry the requirement.

### F10a. Two specific production consolidations, and one rejected shortcut

The runtime [TestReport.rs](../../crates/jet-codegen/src/Prelude/TestReport.rs), lines 18-44, and CLI [CmdCompile.rs](../../Source/CmdCompile.rs), lines 6104-6136, independently count the same five outcome classes with the same expected/unexpected-failure precedence. **#3014** records one borrowed production projection. It must preserve child failure, grade/floor and malformed-evidence behavior without cloning an entire report to get five counts. No wrong count was reproduced here.

Core conformance and the surface-ledger checker also parse the same `CoreModuleExports` declarations separately. **#3011** should reuse source discovery rather than add a third parser. Keep the independent comparison against source declarations and the witness admission rules; sharing extraction is not permission for a generator to certify its own output.

A proposed merge of hashing helpers in the compiled-workload gate and its self-check is **not accepted by this report**. The test’s independently reconstructed expected identity can catch production omissions. First establish what defect detection would survive. Removing textual duplication is not worth replacing an oracle with a call to the implementation under test.

### Concrete existing measurement contract

[CmdCompile::run_test_target](../../Source/CmdCompile.rs), lines 6679-7195, owns ordinary `jet test` execution. It binds source/build/evidence revisions, toolchain, target and profile, runs the generated harness and finalizes the report. `CmdTest` is the recorded-comparison replay path, not a second ordinary test runner.

[CmdDevTools::collect_measure_evidence](../../Source/CmdDevTools.rs), lines 8288-8505, already uses that test harness under D-CLAIM-BENCH1. It collects `JETTESTMEASURE1` timing and `JETALLOC1` allocation rows into `BenchEvidence`, with release, serial AOT, five warmups and twenty exact measured samples. Warmup/calibration allocations are outside the measured reset boundary.

That existing producer is AOT-specific. Its records cannot be relabeled as JIT, interpreter or Web performance. Join the applicable existing mode/workload producers when forming the full result. The quantile experiment above deliberately used a smaller exploratory protocol and is not a receipt satisfying this production policy.

Do not flatten every provider into one sample count: service evidence requires twenty samples; scene evidence requires six hundred samples for each metric. Share identity and availability handling while retaining the domain’s contract. Short, malformed or missing samples must remain error/unavailable, never a successful measurement.



## F11. Compiler and library improvement hypotheses

These are bounded interventions with falsifiers, not an instruction to replace every algorithm or weaken validation. #3011 records their measurement work. Existing #2951/#2954 provide checked-law and paired-experiment machinery; #2919 retains composed proof.

| Hypothesis | Current source evidence | Smallest useful intervention | Acceptance and falsifier |
| --- | --- | --- | --- |
| Share CFG structure before inventing a new optimizer | MIR legality builds dominators with repeated predecessor scans; MIROptimization has separate reachability/predecessor/dominator machinery | One indexed CFG view with explicit lifetime/revision; preserve each consumer’s root policy | Chain, diamond, loop, unreachable, failure and unwind graphs retain exact legality and diagnostics. Count edges, traversals, allocations and total compile time. Reject if root policy or invalidation is wrong, or whole-job cost does not improve. |
| Propagate changes rather than rescan every cache entry | QueryEngine pruning recursively validates broad memo sets; Bundle dirty propagation repeatedly scans module dependencies | Reverse dependency frontiers and memoized validity within an existing revision | Body-only, interface, unrelated-source, import and environment edits on chains/stars/cycles preserve exactly invalidated/recomputed keys. Reject stale hits, broader unnecessary invalidation or greater latency/memory. |
| Reuse the existing sparse fact solver | Effects already delegates to `Facts::project_reachability`; unit resolution still scans unresolved declarations/imports | Keep the fact worklist; test indexed unit dependency resolution under existing visibility rules | Preserve ambiguity/unresolved diagnostics, proof paths and deterministic ordering on chains, diamonds, cycles and dense fan-in. Reject changed meaning or nonconvergence. |
| Reduce repeated scans without weakening internal checks | Constant/dead-value analysis and repeated MIR validation can revisit the same structure | Measure def-use indexes and change frontiers first; retain full correctness boundaries | Compare pass decisions, final canonical identity, inline count and diagnostic provenance, including running optimization twice. Reject changed outcomes. Do not remove validation just because it is expensive. |
| Make backend work reflect sema facts | MIR already has types, places, ownership/drop and loop facts; machine code is delegated | Preserve range, alias, layout, effect and lifetime facts; shorten needless temporaries/materialization | Same source, target and profile; compare emitted IR/machine code, spill loads/stores, copies, size, compile time and runtime. Reject a local win that loses whole-job cost or any observation. |
| Reuse statistics results before loop fusion | `describe` calls sum/mean and variance/stddev separately; quantile sorts all data | Reuse scalar results, then qualify adaptive selection in the same source kernel | Preserve floating operation order, signed zero, finite checks, error precedence and immutability. Reject any required input-family or whole-job loss. #3012. |
| Choose top-k work by both input and output size | Runtime bounded insertion and comptime full sort differ | Share ordering first; measure bounded insertion, heap and select-plus-final-sort internally | Test `k<=0`, `k=1`, `k≈m`, oversized k and ties. Include copies and sorted output. Runtime’s cost is roughly `O(k log k + (m-k)k)` for bounded comparison cost, not uniformly quadratic when k=m. #3013. |
| Choose representations from actual density/capacity | LRU is Vec-backed; BitSet is BTreeSet-backed | Measure capacity/density and key/clone costs before local indexing or representation changes | Preserve eviction, reserved-slot/zero-capacity behavior, sparse huge IDs, negative-bit behavior and deterministic iteration. Reject hidden dense allocations or slower ordinary cases. |
| Keep algorithmic hotspots with existing owners | #2780 already owns the naive FFT/DFT hotspot | Use the same cost/correctness records rather than rediscovering or duplicating its campaign | Exact signal/output semantics and matching dimensions/precision, measured against the same peers; no claim from a renamed quadratic implementation. |

Source locations: [MIR.rs](../../crates/jet-foundation/src/MIR.rs) 7999-8072; [MIROptimization.rs](../../crates/jet-foundation/src/MIROptimization.rs) 3488-3572 and 4226-4366; [queries](../../crates/jet-queries/src/lib.rs) 109-175 and 246-287; [Bundle.rs](../../crates/jet-sema/src/Sema/Bundle.rs) 768-894; [Facts.rs](../../crates/jet-foundation/src/Facts.rs) 659-757; [Units.rs](../../crates/jet-sema/src/Sema/Bundle/Units.rs) 196-459; [Memo.rs](../../crates/jet-codegen/src/Prelude/Memo.rs) 8-53 and 81-149.

One important non-gap: effect/taint/panic reachability is already output-sensitive. “Replace the effect solver with a worklist” would duplicate implemented work. Another: checked sort-by-key already computes keys before mutating the receiver, through [SortKernel.rs](../../crates/jet-codegen/src/Prelude/Core/SortKernel.rs). Preserve its failure contract.

One source-only question remains under #2986: the `median` helper and plain comptime arm exist, while neighboring Core declarations/registry rows expose `describe` and `quantile`. A helper name does not establish a public API. Reconcile intended reachability before adding, removing or documenting a spelling.

## F12. What mature peers actually use for assurance

No credible peer relies on one test suite or claims every possible bug has been excluded.

| Practice | Primary evidence | Jet use and limit |
| --- | --- | --- |
| Diagnostic, pass/fail and repair tests | [Rust UI tests](https://rustc-dev-guide.rust-lang.org/tests/ui.html), [compiletest](https://rustc-dev-guide.rust-lang.org/tests/compiletest.html#test-suites) | Keep registered errors, source locations, successful repairs and negative cases. A snapshot alone does not prove runtime behavior. |
| IR/codegen/run-make and incremental tests | [Rust test suites](https://rustc-dev-guide.rust-lang.org/tests/compiletest.html#incremental-tests) | Exercise edits and generated artifacts as well as source acceptance. Match the exact candidate and configuration. |
| Small regressions and separate output predicates | [LLVM testing](https://llvm.org/docs/TestingGuide.html#llvm-testing-infrastructure-organization), [FileCheck](https://llvm.org/docs/CommandGuide/FileCheck.html#description) | Reuse existing harnesses. Keep a minimized semantic reproducer, not sprawling source-text assertions. |
| Defined-input fuzzing, reduction and differential engines | [Rust fuzzing](https://rustc-dev-guide.rust-lang.org/fuzzing.html), [Rustlantis](https://plf.inf.ethz.ch/research/oopsla24-rustlantis.html) | Compare defined, terminating, deterministically observed programs; minimize disagreement without losing preconditions. MIR-generated cases do not cover the frontend. |
| Dynamic undefined-behavior checking and schedule exploration | [Miri](https://github.com/rust-lang/miri/blob/master/README.md), [AddressSanitizer](https://clang.llvm.org/docs/AddressSanitizer.html#introduction) | Record the model, target, seed/schedule and unsupported operations. A detected violation is strong evidence; a clean run is finite coverage, not soundness. |
| Real ecosystem and target/configuration coverage | [Crater](https://rustc-dev-guide.rust-lang.org/tests/ecosystem.html#crater), [LLVM releases](https://llvm.org/docs/ReleaseProcess.html#overview-of-the-release-process), [GCC testing](https://gcc.gnu.org/install/test.html#how-to-interpret-test-results) | Use real applications and the required platform matrix. Compile-only and one-host results are not universal ecosystem compatibility. |
| Staged bootstrap and reproducibility | [Rust bootstrap](https://rustc-dev-guide.rust-lang.org/building/bootstrapping/what-bootstrapping-does.html#stages-of-bootstrapping) | Record which compiler and standard library produced each stage. Fixed-point equality does not establish correctness or independence. |
| Diverse Double-Compiling | [Wheeler’s DDC analysis](https://dwheeler.com/trusting-trust/dissertation/html/wheeler-trusting-trust-ddc.html) | A genuinely diverse, independently trusted compilation path tests source/executable correspondence under stated assumptions. A previous release from the same lineage is not automatically independent. |
| Mechanized semantic preservation | [CompCert](https://compcert.org/man/manual001.html#semantic-preservation) | State source/target representations, successful-compilation conditions, implementation binding and excluded tools. Do not extend a theorem to linker, OS, hardware or application requirements. |
| Mechanized language/library safety | [RustBelt](https://plv.mpi-sws.org/rustbelt/popl18/) | A model/subset and selected library proofs are not a proof of the whole compiler. Check model-to-implementation correspondence. |
| Explicit assurance levels and reproducible proof budgets | [SPARK assurance levels](https://docs.adacore.com/spark2014-docs/html/ug/en/usage_scenarios.html#levels-of-software-assurance), [GNATprove](https://docs.adacore.com/spark2014-docs/html/ug/en/source/how_to_run_gnatprove.html#running-gnatprove-from-the-command-line) | Distinguish flow, runtime-safety and functional proof. Deterministic solver steps and wall-clock timeouts are different evidence. Unknown is not proved. |
| Bounded tool/library qualification | [Ferrocene scope](https://public-docs.ferrocene.dev/main/qualification/evaluation-plan/qualification-scope.html), [safety manual](https://public-docs.ferrocene.dev/main/safety-manual/scope.html) | Name the release, tool, library subset, host/target and user obligations. These public development-branch documents are scope examples, not evidence that Jet is certified. |

Reuse Jet’s comparison journal, contract records, golden/UI corpora, deterministic worlds, differential manifest and existing three-mode helpers. Extend the missing obligations through #3011 and #2919; do not build a second “trust framework.”

## F13. Assurance for AI-written code must challenge the expected answer

The decisive question is not who typed the code. It is whether the evidence can expose a shared misunderstanding.

A model can write an implementation and tests that agree on the wrong contract. A second model or a different prompt reduces some correlation but does not itself supply an independent oracle. [Reported test-oracle research](https://arxiv.org/abs/2410.21136) finds generated assertions can follow actual behavior rather than intended behavior; [another study](https://arxiv.org/abs/2312.10622) reports incorrect generated assertions in some categories. These studies are not a Jet-specific measurement and do not justify a universal percentage claim about current models.

For load-bearing operations:

1. Derive expected behavior from the ratified requirement before looking at current output. Record that provenance.
2. Use an independently authored mathematical/reference model where feasible. For small domains, exhaustive enumeration is stronger than another handful of examples.
3. Add metamorphic relations, but state their preconditions. Floating-point reassociation, overflow and nondeterministic effects invalidate many tempting relations.
4. Compare independent engines or algorithms. Agreement among Jet modes that share a kernel cannot detect every shared-kernel defect.
5. Mutate real behavior and break the oracle, identity, output capture and denominator deliberately. The detector must reject these controls. Equivalent or invalid mutants do not count as useful coverage.
6. Minimize failures and retain the unique behavioral detector. Have a fresh reader inspect the expected value and the proof boundary, not just the patch.
7. Keep generators/shrinkers deterministic and preserve the failing seed, source, input, schedule and candidate. A smaller counterexample must still satisfy the original preconditions.

The map-extrema contrast is a concrete reason for this discipline: `[1,2]` passes, while a different digit width exposes the wrong comparator. More tests of the first shape would not have helped.

The same rule applies to learned optimization. A model may propose a candidate, never authorize it. An independent checker or exact bounded comparison must validate the stated relation. Timeout, unknown, unsupported semantics and failed checks leave the existing implementation in place and the candidate unqualified. Do not add an LLM dependency to the compiler for this research hypothesis.

## F14. Production, critical systems and bootstrap are distinct claims

The strongest defensible readiness statement is candidate- and scope-bound: which language/API operations, modes, targets, programs, resource envelopes and failure models were exercised or proved, with what remaining assumptions?

| Gate | Evidence required | Current disposition |
| --- | --- | --- |
| Basic build and wrong-answer closure | Fresh compiler, reproduced defects fixed at the source, exact regression and applicable-mode proof | #3010 and #3013 open |
| Full first-party algorithm assurance | Complete source-derived denominator; correctness, cost and proof status for every required operation/variant; no hidden exclusions | #3011 proposed; existing Core reconciliation #2986 in progress |
| Composed product behavior | AOT, default run, interpreter and applicable Web; diagnostics, failures, effects, cleanup, determinism and generated artifacts | #2919 open; this report ran no broad milestone suite |
| Competitive performance | Every required cell/metric against every named peer under the same workload and semantics | #2858 open; Rust ratio at most 1.05 is only the permitted noise band, not a win; every non-Rust ratio must be below 1.00 |
| Professional handoff | Existing ratified candidate window: 14 clean calendar days, 10,000,000 valid differential cases, at least 100 valid mutations per eligible callable, fresh red-team quota | #2420 remains frozen under owner control; no window was started or credited here |
| Bootstrap permission | Real dogfood programs, memory-model campaign, ratified-surface sweep and owner sign-off | #217 remains open; no port or comparator implementation was started |
| Bootstrap correspondence | Pinned reference/candidate comparator, exact observations and stage identities; separately distinguish reproducibility/fixed-point and diverse compilation | #670 and existing bootstrap chain remain owner-gated; no same-result check is called DDC |
| Critical-system use | Named release/tools/libraries/targets, hazards and operating envelope, requirements-to-evidence traceability, resource/schedule bounds, independent review and any applicable qualification process | No universal certification claim is supported. Existing gate records must retain unsupported and unproved edges. |

This does not reduce the owner’s goals to a narrow product. It prevents a broad ambition from being mistaken for a completed safety case. A language can target all eight required application areas while each release claim still names its actual evidence.

An `O(n)` bound alone is not a hard deadline. Critical use also depends on bounded input, bit width, allocation, recursion, scheduler behavior, interrupts, hardware, foreign libraries and worst-case latency. Memory safety does not imply numerical correctness, availability, security-protocol correctness or application safety.

Bootstrapping is not an assurance shortcut. Preserve the independent Rust-hosted reference and real application portfolio as evidence sources. A self-hosted compiler reproducing its own output can reproduce its own bug. DDC remains conditional on the independently trusted tool, source correspondence, deterministic build, comparer, environment and hardware; it does not prove that the source requirements are right.

No new owner ballot is needed for the recommended internal work. Maintainer-only scope was explicitly chosen by the owner, and the test/evidence/qualification mechanisms already have governing decisions. This report does not propose new user syntax, a public rank-selection API, dependencies, an execution-mode exception, a certification label or reopening bootstrap. Any such product choice needs its own worked ballot before implementation; none is smuggled into these cards.


## F15. Beat vectors and what Jet should avoid

### Ranked opportunities to beat peers fairly

| Rank | Mechanism | Current status and evidence | What a peer would need to match it |
| --- | --- | --- | --- |
| 1 | One checked meaning with source-bound observations across execution modes | Partly implemented through shared Prelude, MIR and evidence contracts; the map bug proves the boundary is not yet universally enforced | Align several engines or share their semantic implementation and evidence. This is architectural work, not something peers are mathematically unable to adopt. |
| 2 | Cheap beginner operations backed by the right algorithm and no policy ceremony | Existing quantile API; adaptive kernel only experimentally promising, #3012 | Use an equivalent hybrid and input-structure handling. Rust already supplies the selection primitive; this is not a unique algorithm invention. |
| 3 | Low edit/build latency from reusing proved facts and changed dependencies | Existing query/fact mechanisms; specific broad scans remain measurement targets | Comparable incremental indexes and invalidation precision. Rust already has mature query machinery, so Jet must earn a same-job win. |
| 4 | One honest machine-readable path from obligation to counterexample or qualified result | Existing evidence/test-economics foundations; full algorithm view is proposed in #3011 | Integrate source identity, oracle provenance, cost and unsupported cases across tools. A dashboard alone does not match it. |
| 5 | Short, direct, auditable programs across the required domains | Owner objective and existing Gauntlet/dogfood gates, not established by these videos | Supply similarly complete libraries/tooling and readable real programs. API count and tiny syntax examples do not prove this. |

No categorical competitive win was measured in this mine. The standing gate remains per cell and metric, with every required peer included. Do not average a loss away, compare different semantics, omit compilation/setup, or substitute a smaller domain for the required application workload.

### Avoid list

| Mistake | Evidence | Jet response |
| --- | --- | --- |
| Equate linear with fastest | Plain selection loses on ordered/equal inputs | Keep input families and whole-operation cost in #3011/#3012 |
| Count only convenient work | Cohen’s graph omits pivot computation | Include validation, setup, copies, allocation, callback and output cost |
| Treat sampled maxima as worst-case proof | Video’s hundred-trial comparison plot | Separate empirical evidence from formal bounds; force hostile paths |
| Import a textbook implementation wholesale | Copied sublists and higher pivot constants | Prefer existing std primitives and the smallest qualified internal change |
| Change the result while optimizing it | Even median and rank selection differ | Preserve interpolation, ties, error and mutation contracts |
| Use display output as value semantics | Reproduced numeric map extrema bug | Remove the wrong ordering mechanism, #3013 |
| Reimplement backend allocation or every graph solver | Jet delegates machine allocation and already has a fact worklist | Improve its inputs and reuse the existing mechanism first |
| Add public optimizer/algorithm knobs for maintainer research | Neither source establishes a user need | Keep measurement and candidate controls internal |
| Confuse registration, coverage or self-tests with product proof | Current wrong answer, build failure and open gates | Keep uncovered, stale and unqualified states visible |
| Collapse tests into self-checks while removing duplication | Same-implementation oracles can share a defect | Consolidate production logic; preserve independent expected behavior |
| Call self-bootstrap equality independent assurance | Rust stage3 and DDC answer different questions | Name the trust roots and retain the owner’s bootstrap gates |
| Promise universal correctness or qualification | CompCert/RustBelt/SPARK/Ferrocene all have explicit scope | State the exact claim and every remaining assumption |

## F16. Agent-optimality and the full product goals

Existing #2393 owns the neutral real-task preference rerun; this mine adds these evidence requirements there without claiming a new run.

| Quantity | What improves it | Current evidence and acceptance |
| --- | --- | --- |
| Verdict fidelity | Typed outcomes, independent oracles, no stale/unsupported passes | Weakest on the demonstrated path: map extrema returns a plausible wrong number. #3013 plus hostile evidence controls. |
| Feedback latency | Changed-fact worklists, bounded measurements, focused regressions | Measure edit-to-correct-verdict, not just compilation or test counts. #3011 and existing compiler-speed gates. |
| Actionability | First counterexample, exact operation/input/mode, registered what/why/fix and source location | Preserve the displayed map contrast and exact build error. A generic red badge is insufficient. |
| Context economy | One canonical operation record and concise evidence query | Avoid duplicated catalogs, raw log dumps and repeated derivation. The retained claim appendix supports drill-down. |
| Repair determinism | Pinned candidate/oracle/seed/schedule, minimized input, exact replay | A fix must pass the same observation check across applicable modes without hidden fallback. |

The other goals are not sacrificed to those five quantities:

- **Performance:** runtime, compile time, edit latency, memory, allocation, code size and relevant tail behavior remain separate metrics. A faster kernel cannot hide a slower whole program.
- **Safety and reliability:** sema owns checking; generated backend failures remain internal errors; safe Jet must not acquire hidden unsafe behavior to win a benchmark. Long-lived services need cancellation, cleanup, concurrency and fault evidence.
- **Maintainability:** one semantic kernel, source-derived discovery and borrowed evidence projections. Reuse reviewed library algorithms. Remove obsolete implementations only in a complete cutover.
- **Beginner experience:** the obvious high-level operation is safe and direct. No complexity labels, allocator choices or audit setup are required to write an application.
- **Expert control:** existing types, comparators, profiles, targets and audited escape mechanisms retain their meaning. Maintainers can inspect raw cost, generated code and rejected evidence without a parallel semantic path.
- **Tooling and ecosystem:** real package/workspace/editor/build workflows and the required application portfolio remain in the gate. A small compiler benchmark cannot establish ecosystem completeness.
- **Clarity and visual quality:** names must teach the correct distinction, such as statistical median versus rank selection. Evaluate novice task completion, cold-read understanding, diagnostic repair and visual presentation on actual tasks; do not declare beauty from token count alone.

## F17. Concrete surface coverage

The checked-in standing lens names ten micro categories. All ten are covered; performance/cost is also treated explicitly throughout this report rather than omitted over a numbering discrepancy.

| Category | Source detail | Jet cross-check and decision |
| --- | --- | --- |
| Syntax | Instruction rewrites and one-partition selection are implementation choices | No new keyword, sigil or algorithm annotation. Use existing operations. |
| Ergonomics | Hidden pivot/rank bookkeeping and debug/optimization tradeoffs | Keep ordinary code direct; maintainers select heavy evidence separately. |
| Surfaces | Values/registers/stack/ABI; rank/median/quantile/top-k | Preserve distinct output and semantic contracts instead of one vague “fast operation.” |
| APIs/types/methods | Rust `select_nth_unstable`, `_by`, `_by_key`; regalloc2 `MachineEnv`, `PReg`, `VReg`, `run`, `Output`; Cranelift block parameters/stack slots | Study/reuse backend APIs internally. Checked `core.data.quantile` and map operations are the immediate Jet targets. No new public selection API is proposed. |
| Defaults | Hybrid selection; LLVM debug/production allocator choices | Keep safe automatic defaults; no portable ten-times claim or forced BFPRT policy. |
| Naming | “Infinite” variables and ambiguous median | Use exact virtual/physical/register and statistical/order-statistic terms. |
| Diagnostics | Video has no Jet error UI; upstream APIs expose invalid-rank/order and verifier errors | Preserve typed data errors, registered diagnostics and backend-ICE status; probe invalid q, empty and nonfinite data. |
| UX/DX | Timings versus guarantees; source-location verifier feedback | Show actionable counterexamples and separately labeled cost/proof evidence. |
| Tooling/CLI | `llc -regalloc=...`, `.clif` filetests, existing `jet test --measure` | These justify maintainer inspection, not a new end-user audit command. |
| Ceremony/control | Fixed-register/ABI constraints and comparator choices | Hide internal strategy selection; retain existing explicit expert control and one semantic mechanism. |

**Covered in current source or this probe:** checked `quantile`/median helper, shared statistics source, `Map.min`, `Map.max`, `Map.top_n`, `SortKernel`, `JetLru`, `JetBitSet`, shared effect reachability, `.measure`, `collect_measure_evidence`, evidence records and test-economics negative controls. “Covered” here names evidence, not a claim that each item passes every product gate.

**Worth checking:** public median reachability under #2986; top-n ordering and limits; floating/key/duplicate semantics; LRU zero/reserved-slot behavior; BitSet sparse huge IDs; CFG root policy; cache invalidation; all non-AOT measurement records.

**Missing or unqualified:** full maintainer algorithm-cost accounting, current main build, correct numeric map extrema on the reproduced path, qualified adaptive-statistics performance and completed product/bootstrap gates. A dedicated first-party nth-selection implementation was not found in the bounded search; that is not a demand for a new public API.

## Owner-goal traceability

| Owner requirement | Answer in this report | Work/evidence home |
| --- | --- | --- |
| Mine every playlist item and audience | Full captured arguments, strata, primary checks and limitations | #3009; source and claim appendices |
| Reconsider infinite variables/codegen | Distinguish virtual resources, constrained allocation and backend ownership | F11; #3011 |
| Improve algorithms at every abstraction level | Whole first-party scope, not just Core APIs or hot functions | F7/F11; #3011 |
| Remove avoidable superlinear work | Output-sensitive work, frontier/index reuse, selection and representation analysis | #3011/#3012/#3013; existing #2780 |
| Preserve unavoidable lower bounds | Explicit cost model, bit width, output and callback dimensions | F8 |
| Improve runtime, builds and codegen | Separate whole-job metrics and bounded interventions | F11/F15; #2858 |
| Optional maintainer-only test integration | Existing runner, `.measure`, `BenchEvidence` and producer policies | F6; concrete measurement contract; #3011 |
| No end-user burden | No new syntax, command, instrumentation or policy knobs | F6/F17 |
| One source of truth and no stale metadata | Source-derived denominator and existing candidate invalidation | F7/F9; #2943/#3011 |
| Consolidate existing machinery | Shared source discovery and borrowed verdict projection; preserve independent oracles | F10/F10a; #3011/#3014 |
| Correctness, stability and reliability | Diverse assurance methods with explicit scope and hostile controls | F12/F13; #2919 |
| Learn from Rust and other peers | Rust, LLVM/GCC, Rustlantis, Miri, CompCert, RustBelt, SPARK and Ferrocene | F12 and primary links |
| Trust AI-generated implementation and tests | Expected-value provenance, independent models, mutation and minimized counterexamples | F13 |
| Critical-system ambition | Requirements/target/operating-envelope and qualification boundaries, not blanket claims | F14; existing readiness gates |
| Confidence before bootstrap | Keep reference implementation, staged identities, dogfood, memory-model and owner gates | #217/#218/#670; F14 |
| Beginner/expert, maintainability, tooling, ecosystem and visual quality | Preserve automatic safe defaults and expert control; evaluate real tasks across every required domain | F16/F17; #2858/#217 |
| Owner retains product decisions | No new product mechanism or scope cutover hidden in the recommendation | F14; no new ballot needed |

## Priorities and acceptance

This is a dated recommendation, not a second work queue. Tower owns sequence and current state.

1. **#3010:** restore the current compiler build with real caller arguments and Web/alias proof.
2. **#3013:** fix the reproduced map ordering defect through one canonical policy; prove the public programs in every applicable mode.
3. **#3011:** extend existing evidence into full maintainer algorithm accounting. Preserve the complete denominator, optional cost collection and fail-closed provenance.
4. **#3012:** qualify smaller statistics work, including the measured loss and numerical/error contracts. Do not ship the experiment merely because it passed bounded cases.
5. **#3014:** remove duplicate production verdict projection without cloning reports or weakening independent tests.
6. **Existing gates:** #2986 handles Core declaration reconciliation; #2780 retains FFT work; #2919, #2858, #217 and the owner-controlled #2420/#670 chain retain their original acceptance obligations.

Every implementation recommendation names an observable result, the relevant input partitions, failure conditions and its existing or new Tower owner. No elapsed qualification window, broad suite, performance gate, owner ballot or bootstrap permission was fabricated by this report.


## Evidence identity and reproducibility

Observed HEAD: `5382a5b2055e1c32c8eb02a2f6cbf9e1598c9a6f`. The tree was dirty, so that commit alone does not identify the tested source. These are **not commit-bound release receipts**.

Host: x86_64 Linux, AMD Ryzen 9 7950X3D. Toolchain: `rustc 1.97.1 (8bab26f4f 2026-07-14)`, `x86_64-unknown-linux-gnu`, LLVM 21.1.8. Rust experiments used `-O`; the production-library probe used a fresh Cargo development build with `CARGO_INCREMENTAL=0`. Its temporary manifest resolved the existing path dependency graph with its own lockfile. It did not replace the repository lockfile or add a production dependency.

| Evidence file | SHA-256 |
| --- | --- |
| `crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs` | `094629bc914bb11725b914c8a86eec0b2590f76dc39ce461b630f47c1da5e4e6` |
| `crates/jet-comptime/src/Comptime/Builtins.rs` | `8d8b6d58ff73878b1ef221812ca52d8c2f3710afcba5a1b6b3ed59b40d70e114` |
| `crates/jet-codegen/src/Prelude/Core/Collections.rs` | `5c829d045a6c4e00373459521c43d98fe93e14b2fb2f190d300c4418b20e4799` |
| `crates/jet-foundation/src/Collections.rs` | `461448fb88c871113864476cef342448734bb5155e054dafee5ad0775a74451b` |
| `tests/jet_measure.rs` | `9197cd6dcb9fe5f096a6eea7d06ea630c3a70571e55359249772f10d4f4e0890` |
| `scripts/agent/compiler-proof.mjs` | `3f7e6a5563090242a5568485a0bd068fb213de4f213f2d3b71b693280ed1172b` |
| `scripts/agent/test-economics.mjs` | `a835e99682ca35006a05d62ce28fae04a67b82487bcfebecc470eff39173e73b` |
| Temporary probe `selection_probe.rs` | `8da62bc1dd3966c1330820a24584ce4b09da09a3f60eed341d15de8ca47a646d` |
| Temporary probe `quantile_driver.rs` | `8bf595317ffbd6e80ef598e2755d412e0f65c1cf3738ea1d0d7c3cbb7b2f38dd` |
| Temporary probe `quantile_original.rs` | `63af418b2bcd0bef27e7bfbe826aa20cd91f305356e7745362d183bee5e71211` |
| Temporary probe `main.rs` | `d05eabb27f8d94a566ae9d854eb8293ece3eeabba19ed7d0fb47ef3d132adbe7` |
| Temporary probe `Cargo.lock` | `c97e60c9514ef0d860989d751dc12f05b9e63ac674422d6f59a3b646c9e49703` |
| Temporary probe `jet-mine-comptime-map-probe` | `17746ab043521a900d61f51c533fc6e00c780d93bff40d7df79dc06c1152d147` |

The DataStats full-source hash was unchanged between extraction and the final evidence identity check. The standalone numeric experiment extracted production lines 9-42, 221-233 and 313-348 verbatim. The error carriers in the driver are explicitly test-only. The map probe instead links the actual production crate and matches typed values.

<details>
<summary>Production map regression source and commands</summary>

Temporary Cargo manifest:

```toml
[package]
name = "jet-mine-comptime-map-probe"
version = "0.0.0"
edition = "2021"

[dependencies]
jet-comptime = { path = "/home/nate/Projects/Github/jet/crates/jet-comptime" }

[[bin]]
name = "jet-mine-comptime-map-probe"
path = "main.rs"
```

`main.rs`:

```rust
use jet_comptime::{AST::{CtKey,CtValue},Comptime::Builtins::apply_method,Diagnostics::Span};
fn main() {
  let mut mismatches = 0;
  for numbers in [[1,2],[2,10],[-2,-10]] {
    let m=CtValue::Map(numbers.into_iter().enumerate().map(|(k,v)|(CtKey::Int(k as i64),CtValue::Int(v))).collect());
    for method in ["min","max"] {
      let expected=if method=="min" {*numbers.iter().min().unwrap()} else {*numbers.iter().max().unwrap()};
      let result=apply_method(&m,method,vec![],Span::new(0,0)).unwrap();
      println!("{numbers:?} {method}: expected={expected}; actual={}",result.jet_show());
      if !matches!(result, CtValue::Present(value) if matches!(*value, CtValue::Int(actual) if actual == expected)) {
        mismatches += 1;
      }
    }
    let result=apply_method(&m,"top_n",vec![CtValue::Int(1)],Span::new(0,0)).unwrap();
    println!("{numbers:?} top_n(1): {}",result.jet_show());
  }
  println!("numeric extrema mismatches: {mismatches}");
  if mismatches != 0 { std::process::exit(1); }
}
```

Commands, with the temporary directory substituted for `<probe>`:

```text
CARGO_INCREMENTAL=0 scripts/agent/jet-env cargo build --manifest-path <probe>/Cargo.toml -j 4
CARGO_INCREMENTAL=0 scripts/agent/jet-env cargo build --offline --manifest-path <probe>/Cargo.toml -j 4 --quiet
scripts/agent/jet-env target/debug/jet-mine-comptime-map-probe
```

The second build includes the strengthened typed failing verdict. Its executable produced:

```text
[1, 2] min: expected=1; actual=1
[1, 2] max: expected=2; actual=2
[1, 2] top_n(1): [__jet_ { __jet_key: 1, __jet_value: 2 }]
[2, 10] min: expected=2; actual=10
[2, 10] max: expected=10; actual=2
[2, 10] top_n(1): [__jet_ { __jet_key: 1, __jet_value: 10 }]
[-2, -10] min: expected=-10; actual=-10
[-2, -10] max: expected=-2; actual=-2
[-2, -10] top_n(1): [__jet_ { __jet_key: 0, __jet_value: -2 }]
numeric extrema mismatches: 2
exit: 1
```

</details>

<details>
<summary>Exploratory Rust selection experiment and complete output</summary>

`selection_probe.rs`:

```rust
use std::{cell::Cell, hint::black_box, time::Instant};

fn input(n: usize, kind: &str) -> Vec<u64> {
    let mut state = 0x123456789abcdefu64;
    (0..n).map(|i| match kind {
        "sorted" => i as u64,
        "reverse" => (n - i) as u64,
        "equal" => 7,
        "few" => (i % 7) as u64,
        _ => { state = state.wrapping_mul(6364136223846793005).wrapping_add(1); state },
    }).collect()
}
fn main() {
    println!("kind,n,sort_comparisons,select_comparisons,sort_median_ns,select_median_ns,selection_over_sort");
    for kind in ["random", "sorted", "reverse", "equal", "few"] {
        for n in [1001, 10001, 100001] {
            let source = input(n, kind);
            let k = n / 2;
            let mut expected = source.clone();
            expected.sort_unstable();
            let comparisons = Cell::new(0u64);
            let mut sorted = source.clone();
            sorted.sort_unstable_by(|a,b| { comparisons.set(comparisons.get()+1); a.cmp(b) });
            let sort_count = comparisons.replace(0);
            let mut selected = source.clone();
            selected.select_nth_unstable_by(k, |a,b| { comparisons.set(comparisons.get()+1); a.cmp(b) });
            let select_count = comparisons.get();
            assert_eq!(selected[k], expected[k]);
            assert!(selected[..k].iter().all(|x| *x <= selected[k]));
            assert!(selected[k+1..].iter().all(|x| *x >= selected[k]));
            selected.sort_unstable();
            assert_eq!(selected, expected);
            let mut timings = [Vec::new(), Vec::new()];
            for trial in 0..8 {
                for offset in 0..2 {
                    let method = (trial + offset) % 2;
                    let mut data = source.clone();
                    let start = Instant::now();
                    if method == 0 { data.sort_unstable(); }
                    else { data.select_nth_unstable(k); }
                    black_box(data[k]);
                    let elapsed = start.elapsed().as_nanos();
                    assert_eq!(data[k], expected[k]);
                    if trial > 0 { timings[method].push(elapsed); }
                }
            }
            for times in &mut timings { times.sort_unstable(); }
            let a = timings[0][3]; let b = timings[1][3];
            println!("{kind},{n},{sort_count},{select_count},{a},{b},{:.4}", b as f64/a as f64);
        }
    }
    for values in [vec![1i64,4], vec![9,1,4,2], vec![7,7,7,7]] {
        let mut sorted = values.clone(); sorted.sort_unstable();
        let mut selected = values;
        let k = selected.len()/2;
        let (lower, upper, _) = selected.select_nth_unstable(k);
        let low = *lower.iter().max().unwrap();
        assert_eq!((low as i128 + *upper as i128), sorted[k-1] as i128 + sorted[k] as i128);
    }
    println!("CHECK: rank, partition, multiset, duplicates and even-middle-pair passed; no Jet performance claim");
}
```

Compiled with `scripts/agent/jet-env rustc -O <probe>/selection_probe.rs -o <probe>/selection_probe`, then executed through `scripts/agent/jet-env`.

```text
kind,n,sort_comparisons,select_comparisons,sort_median_ns,select_median_ns,selection_over_sort
random,1001,10266,2080,4639,1002,0.2160
random,10001,138546,25022,61027,10720,0.1757
random,100001,1727562,205425,851663,84511,0.0992
sorted,1001,1000,2159,210,861,4.1000
sorted,10001,10000,20818,1933,7465,3.8619
sorted,100001,100000,205456,19748,75815,3.8391
reverse,1001,1000,2078,521,872,1.6737
reverse,10001,10000,20461,5140,8076,1.5712
reverse,100001,100000,202976,51388,78419,1.5260
equal,1001,1000,2078,411,752,1.8297
equal,10001,10000,20240,1944,7243,3.7258
equal,100001,100000,200726,19287,71487,3.7065
few,1001,5509,3549,2215,1482,0.6691
few,10001,59457,43363,22944,17223,0.7507
few,100001,588502,473384,231001,187758,0.8128
CHECK: rank, partition, multiset, duplicates and even-middle-pair passed; no Jet performance claim
exit: 0
```

</details>

<details>
<summary>Exploratory source-bound quantile experiment and complete output</summary>

`quantile_original.rs`, the extracted production bodies:

```rust
pub(crate) fn jet_data_error(
    kind: jet_std::DataErrorKind,
    operation: &str,
    reason: impl Into<String>,
) -> jet_std::DataError {
    jet_std::DataError {
        kind,
        operation: operation.to_string(),
        row: Err(JetAbsent),
        column: Err(JetAbsent),
        index: Err(JetAbsent),
        reason: reason.into(),
        cause: Err(JetAbsent),
    }
}

pub(crate) fn jet_data_error_at(
    kind: jet_std::DataErrorKind,
    operation: &str,
    index: JetOutcome<i64, JetAbsent>,
    reason: impl Into<String>,
) -> jet_std::DataError {
    let mut error = jet_data_error(kind, operation, reason);
    error.index = index;
    error
}

pub(crate) fn jet_data_normalize_zero(value: f64) -> f64 {
    if value == 0.0 {
        0.0
    } else {
        value
    }
}

pub(crate) fn jet_data_reject_nonfinite(operation: &str, values: &[f64]) -> Result<(), jet_std::DataError> {
    for (index, value) in values.iter().copied().enumerate() {
        if !value.is_finite() {
            return Err(jet_data_error_at(
                jet_std::DataErrorKind::NonFinite,
                operation,
                Ok(index as i64),
                "numeric input must be finite",
            ));
        }
    }
    Ok(())
}

pub(crate) fn jet_data_quantile_checked(values: &Vec<f64>, q: f64) -> Result<f64, jet_std::DataError> {
    if !q.is_finite() || !(0.0..=1.0).contains(&q) {
        return Err(jet_data_error(
            jet_std::DataErrorKind::InvalidArgument,
            "quantile",
            "quantile q must be a finite value in 0.0 through 1.0",
        ));
    }
    if values.is_empty() {
        return Err(jet_data_error(
            jet_std::DataErrorKind::Empty,
            "quantile",
            "quantile of empty data is undefined",
        ));
    }
    jet_data_reject_nonfinite("quantile", values)?;
    let mut sorted = values.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let pos = q * (sorted.len().saturating_sub(1)) as f64;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let value = if lo == hi {
        sorted[lo]
    } else {
        let t = pos - lo as f64;
        sorted[lo] * (1.0 - t) + sorted[hi] * t
    };
    Ok(jet_data_normalize_zero(value))
}

pub(crate) fn jet_data_median_checked(values: &Vec<f64>) -> Result<f64, jet_std::DataError> {
    jet_data_quantile_checked(values, 0.5).map_err(|mut error| {
        error.operation = "median".to_string();
        error
    })
}

```

`quantile_driver.rs`, including the explicitly harness-only carrier types:

```rust
// Throwaway standalone experiment. Production functions are extracted verbatim.
// These carrier declarations are harness-only; this is not a Jet tier/ABI test.
#![allow(dead_code)]
use std::{hint::black_box, time::Instant};
#[derive(Clone, Debug, PartialEq, Eq)] struct JetAbsent;
type JetOutcome<T,E> = Result<T,E>;
mod jet_std {
    use super::*;
    #[derive(Clone, Debug, PartialEq, Eq)] pub enum DataErrorKind { Empty, InvalidArgument, NonFinite }
    #[derive(Clone, Debug, PartialEq, Eq)] pub struct DataError {
        pub kind: DataErrorKind, pub operation: String, pub row: Result<i64,JetAbsent>,
        pub column: Result<i64,JetAbsent>, pub index: Result<i64,JetAbsent>,
        pub reason: String, pub cause: Result<(),JetAbsent>,
    }
}
include!("quantile_original.rs");
fn candidate(values: &Vec<f64>, q:f64) -> Result<f64,jet_std::DataError> {
    // Reuse original policy for invalid inputs. Valid paths fuse finite/order discovery.
    if !q.is_finite() || !(0.0..=1.0).contains(&q) || values.is_empty() {
        return jet_data_quantile_checked(values,q);
    }
    let mut ascending=true; let mut descending=true;
    for (index,value) in values.iter().enumerate() {
        if !value.is_finite() { return jet_data_quantile_checked(values,q); }
        if index>0 { ascending &= values[index-1] <= *value; descending &= values[index-1] >= *value; }
    }
    let pos=q*(values.len()-1) as f64;
    let lo=pos.floor() as usize; let hi=pos.ceil() as usize;
    let (a,b) = if ascending { (values[lo],values[hi]) }
    else if descending { (values[values.len()-1-lo],values[values.len()-1-hi]) }
    else {
        let mut data=values.clone();
        let (_,pivot,upper)=data.select_nth_unstable_by(lo,|a,b| a.partial_cmp(b).unwrap());
        let a=*pivot;
        let b=if lo==hi {a} else { upper.iter().copied().min_by(|a,b| a.partial_cmp(b).unwrap()).unwrap() };
        (a,b)
    };
    Ok(jet_data_normalize_zero(if lo==hi {a} else {let t=pos-lo as f64; a*(1.0-t)+b*t}))
}
fn check(values:Vec<f64>, q:f64) {
    let before:Vec<_>=values.iter().map(|v|v.to_bits()).collect();
    let a=jet_data_quantile_checked(&values,q); let b=candidate(&values,q);
    match (a,b) {
        (Ok(a),Ok(b))=>assert_eq!(a.to_bits(),b.to_bits(),"values={values:?}, q={q}"),
        (Err(a),Err(b))=>assert_eq!(a,b),
        (a,b)=>panic!("mismatch {a:?} {b:?}"),
    }
    assert_eq!(before,values.iter().map(|v|v.to_bits()).collect::<Vec<_>>());
}
fn main() {
    let alphabet=[-2.0,-0.0,0.0,1.0,4.0]; let mut checked=0;
    for n in 0..=6u32 {
        for mut mask in 0..5usize.pow(n) {
            let v:Vec<f64>=(0..n).map(|_| {let x=alphabet[mask%5];mask/=5;x}).collect();
            for q in [0.0,0.1,0.25,0.5,0.9,1.0] {check(v.clone(),q);checked+=1;}
        }
    }
    for v in [vec![],vec![f64::NAN],vec![1.0,f64::INFINITY],vec![f64::NEG_INFINITY,2.0],vec![-f64::MAX,f64::MAX]] {
        for q in [f64::NAN,-1.0,0.0,0.5,1.0,2.0] {check(v.clone(),q);checked+=1;}
    }
    println!("checked_cases={checked}; bitwise result/error/input-preservation checks passed");
    println!("kind,n,production_sort_ns,adaptive_selection_ns,adaptive_over_sort");
    for kind in ["random","sorted","reverse","equal","few"] {
        for n in [1001,10001,100001] {
            let mut state=41u64;
            let v:Vec<f64>=(0..n).map(|i| match kind {
                "sorted"=>i as f64, "reverse"=>(n-i)as f64,"equal"=>7.0,"few"=>(i%7)as f64,
                _=>{state=state.wrapping_mul(6364136223846793005).wrapping_add(1);(state>>12)as f64},
            }).collect();
            let expected=jet_data_quantile_checked(&v,0.5).unwrap();
            let mut times=[Vec::new(),Vec::new()];
            for trial in 0..8 {
                for offset in 0..2 {
                    let method=(trial+offset)%2;
                    let start=Instant::now();
                    let result=if method==0 {jet_data_quantile_checked(black_box(&v),black_box(0.5))} else {candidate(black_box(&v),black_box(0.5))};
                    let elapsed=start.elapsed().as_nanos();
                    assert_eq!(black_box(result.unwrap()).to_bits(),expected.to_bits());
                    if trial>0 {times[method].push(elapsed);}
                }
            }
            for t in &mut times {t.sort_unstable();}
            println!("{kind},{n},{},{},{:.4}",times[0][3],times[1][3],times[1][3]as f64/times[0][3]as f64);
        }
    }
}
```

Compiled with `scripts/agent/jet-env rustc -O <probe>/quantile_driver.rs -o <probe>/quantile_probe`, then executed through `scripts/agent/jet-env`.

```text
checked_cases=117216; bitwise result/error/input-preservation checks passed
kind,n,production_sort_ns,adaptive_selection_ns,adaptive_over_sort
random,1001,7364,2294,0.3115
random,10001,108847,19477,0.1789
random,100001,1907625,206323,0.1082
sorted,1001,822,841,1.0231
sorted,10001,8236,8166,0.9915
sorted,100001,300081,81525,0.2717
reverse,1001,1062,841,0.7919
reverse,10001,10750,8175,0.7605
reverse,100001,316543,81505,0.2575
equal,1001,952,841,0.8834
equal,10001,8206,8166,0.9951
equal,100001,305172,81536,0.2672
few,1001,3707,2906,0.7839
few,10001,38704,33143,0.8563
few,100001,699202,359475,0.5141
exit: 0
```

These are bounded exploratory programs, not permanent test infrastructure. Their results must not be promoted to a product or peer-performance claim without the owning card’s proof.

</details>


## Normalized claim ledger

This is dated evidence, not current work state. The machine-readable capture ledgers were parsed, enum-checked and checked for unique topics within each resource before synthesis. Every non-`none` action has a Tower owner recorded in this run. Source provenance prevents the CMU/video/blog chain and Main’s repeated inferences from inflating independent support.

<details>
<summary>All source, peer, micro and Jet claims</summary>

### YlFYXewYJ8M: 20 claims

- **`virtual-registers`**: Virtual values exceed physical registers; liveness, spills and ABI constraints connect them.
  - Evidence: `primary`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: 00:03-01:02;08:31-10:43. Provenance: `youtube:YlFYXewYJ8M`.
  - Qualification/correction: Finite source programs do not literally require infinitely many machine registers; hardware rename registers are a different mechanism.
  - Jet evidence: TIR/MIR and sema facts; #2951/#2954; current fresh build blocked by #3010. Action: Use target-bound, semantics-preserving paired experiments through existing evidence records. Owner: #3011.

- **`joint-codegen-cost`**: Instruction selection, allocation and scheduling affect each other.
  - Evidence: `primary`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: 02:53-03:37. Provenance: `youtube:YlFYXewYJ8M`.
  - Qualification/correction: Changing one local score can increase copies, live ranges or spill cost elsewhere.
  - Jet evidence: TIR/MIR and sema facts; #2951/#2954; current fresh build blocked by #3010. Action: Use target-bound, semantics-preserving paired experiments through existing evidence records. Owner: #3011.

- **`optimization-hardness`**: The video frames optimal code generation as mathematically impossible.
  - Evidence: `primary`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: 00:14-00:33;05:11-07:03. Provenance: `youtube:YlFYXewYJ8M`.
  - Qualification/correction: NP-complete does not mean impossible; unrestricted equivalence/optimal-program search and restricted tree instruction selection are different problems.
  - Jet evidence: TIR/MIR and sema facts; #2951/#2954; current fresh build blocked by #3010. Action: Use target-bound, semantics-preserving paired experiments through existing evidence records. Owner: #3011.

- **`register-allocation-model`**: Graph coloring motivates register allocation.
  - Evidence: `primary`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: 09:21-10:43. Provenance: `youtube:YlFYXewYJ8M`.
  - Qualification/correction: Pure interval-graph coloring is tractable; realistic spill/split/coalesce and register constraints change the problem.
  - Jet evidence: TIR/MIR and sema facts; #2951/#2954; current fresh build blocked by #3010. Action: Use target-bound, semantics-preserving paired experiments through existing evidence records. Owner: #3011.

- **`jit-allocation-choice`**: The video describes JIT allocation as linear scan and quotes over 10x faster.
  - Evidence: `primary`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: 10:45-11:03. Provenance: `youtube:YlFYXewYJ8M`.
  - Qualification/correction: Not universal. Cranelift uses regalloc2; no portable 10x ratio follows without matched allocator, program and target evidence.
  - Jet evidence: TIR/MIR and sema facts; #2951/#2954; current fresh build blocked by #3010. Action: Use target-bound, semantics-preserving paired experiments through existing evidence records. Owner: #3011.

- **`target-specific-scheduling`**: Instruction ordering can hide dependency stalls.
  - Evidence: `primary`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: 11:05-13:24. Provenance: `youtube:YlFYXewYJ8M`.
  - Qualification/correction: Out-of-order hardware does not remove compiler scheduling tradeoffs; no exact cycle saving is portable.
  - Jet evidence: TIR/MIR and sema facts; #2951/#2954; current fresh build blocked by #3010. Action: Use target-bound, semantics-preserving paired experiments through existing evidence records. Owner: #3011.

- **`abi-observations`**: Stack frames and calling conventions determine register and memory obligations.
  - Evidence: `primary`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: 13:26-16:05. Provenance: `youtube:YlFYXewYJ8M`.
  - Qualification/correction: Argument registers, red zones, saved registers, stack alignment and unwind behavior depend on ABI and target.
  - Jet evidence: TIR/MIR and sema facts; #2951/#2954; current fresh build blocked by #3010. Action: Use target-bound, semantics-preserving paired experiments through existing evidence records. Owner: #3011.

- **`peephole-semantics`**: Local patterns can be cleaned after lowering exposes them.
  - Evidence: `primary`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: 16:08-17:31. Provenance: `youtube:YlFYXewYJ8M`.
  - Qualification/correction: Flags, overflow, traps, signed shifts, memory ordering and debug/error observations constrain each rewrite.
  - Jet evidence: TIR/MIR and sema facts; #2951/#2954; current fresh build blocked by #3010. Action: Use target-bound, semantics-preserving paired experiments through existing evidence records. Owner: #3011.

- **`total-job-cost`**: Useful optimizers use bounded heuristics and must account for compile cost and resulting code cost.
  - Evidence: `primary`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: 03:37-04:45;10:45-11:03;17:03-17:31. Provenance: `youtube:YlFYXewYJ8M`.
  - Qualification/correction: Neither minimum instruction count nor a local speedup establishes lower whole-job cost.
  - Jet evidence: TIR/MIR and sema facts; #2951/#2954; current fresh build blocked by #3010. Action: Use target-bound, semantics-preserving paired experiments through existing evidence records. Owner: #3011.

- **`candidate-verifier-separation`**: Audience suggests AI-generated candidates should be checked by a deterministic verifier.
  - Evidence: `audience`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: comment UgwKv...; uploader reply Ugz8.... Provenance: `youtube:YlFYXewYJ8M`.
  - Qualification/correction: Embedding similarity and fluent explanations are not semantic equivalence checks.
  - Jet evidence: TIR/MIR and sema facts; #2951/#2954; current fresh build blocked by #3010. Action: Use target-bound, semantics-preserving paired experiments through existing evidence records. Owner: #3011.

- **`micro-syntax`**: Local instruction examples are optimizer implementation details, not a reason for Jet source syntax.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: 05:11-07:03;16:08-17:00. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3011.

- **`micro-ergonomics`**: Lowering should spare programmers register and stack-management ceremony.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: 08:31-16:05. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3011.

- **`micro-surfaces`**: IR values, registers, stack slots, blocks and ABI boundaries are distinct operational surfaces.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: 02:53-03:37;08:31-16:05. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3011.

- **`micro-apis-types-methods`**: Typed operands, effects and calling-convention constraints must survive the backend boundary.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: 05:11-07:03;13:26-16:05. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3011.

- **`micro-defaults`**: A bounded heuristic is a practical default; no fixed machine cost is universal.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: 10:45-11:03;17:03-17:31. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3011.

- **`micro-naming`**: Virtual variables, hardware rename registers and physical ISA registers must not be conflated.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: 00:03-01:02; audience Ugzfs.... Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3011.

- **`micro-diagnostics`**: The source provides no user-diagnostic demonstration; Jet must preserve its registered errors and backend-ICE boundary.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: full transcript reviewed; no diagnostic UI. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #2919.

- **`micro-ux-dx`**: Debug visibility and optimized execution have real representation tradeoffs.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: 03:47-04:45. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3011.

- **`micro-tooling-cli`**: No Jet-relevant command or flag is demonstrated; internal measurement needs no new public CLI.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: full transcript reviewed. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3011.

- **`micro-ceremony-control`**: Beginners should not choose allocators; maintainers need exact target and evidence control.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=YlFYXewYJ8M>; locator: 10:45-11:03;13:26-16:05. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3011.

### 5JXpNOZWAHM: 26 claims

- **`selection-vs-sort`**: A single rank does not require fully sorting the input.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `needs-measurement`.
  - Source: <https://www.cs.cmu.edu/~15451-f23/lectures/lecture01-filled.pdf>; locator: 01:04-01:07;12:03-15:42. Provenance: `CMU:15451-f23-lecture01`.
  - Qualification/correction: Comparison sorting has a worst-case lower bound; rank selection has different output requirements.
  - Jet evidence: DataStats.rs:313-348; current source-kernel differential and timing probes; #3010 blocks full Jet smoke. Action: Record assumptions and input partitions; measure before changing the canonical kernel. Owner: #3012.

- **`quickselect-expected`**: Randomized quickselect has expected linear work, not a universal worst-case linear bound.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `needs-measurement`.
  - Source: <https://www.cs.cmu.edu/~15451-f23/lectures/lecture01-filled.pdf>; locator: 12:03-17:40; CMU pp19-25. Provenance: `CMU:15451-f23-lecture01`.
  - Qualification/correction: Expectation is over random choices even for fixed adversarial input; random input and random pivots are different assumptions.
  - Jet evidence: DataStats.rs:313-348; current source-kernel differential and timing probes; #3010 blocks full Jet smoke. Action: Record assumptions and input partitions; measure before changing the canonical kernel. Owner: #3011.

- **`comparison-model`**: Comparison sorting needs Omega(n log n) comparisons in the worst case.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `needs-measurement`.
  - Source: <https://www.cs.cmu.edu/~15451-f23/lectures/lecture01-filled.pdf>; locator: 11:26-11:41; uploader correction Ugx6-5Bh...AMQI_f.... Provenance: `CMU:15451-f23-lecture01`.
  - Qualification/correction: Counting, radix and bucket methods use additional key/word/range assumptions; costs must include those dimensions.
  - Jet evidence: DataStats.rs:313-348; current source-kernel differential and timing probes; #3010 blocks full Jet smoke. Action: Record assumptions and input partitions; measure before changing the canonical kernel. Owner: #3011.

- **`pivot-not-free`**: A good pivot must discard a constant fraction without spending too much finding it.
  - Evidence: `primary`; confidence `medium`; stance `supports`; classification `needs-measurement`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: 19:08-20:19. Provenance: `youtube:5JXpNOZWAHM`.
  - Jet evidence: DataStats.rs:313-348; current source-kernel differential and timing probes; #3010 blocks full Jet smoke. Action: Record assumptions and input partitions; measure before changing the canonical kernel. Owner: #3011.

- **`half-subset-pivot`**: Selecting a median of half the input does not give the desired linear recurrence.
  - Evidence: `primary`; confidence `medium`; stance `supports`; classification `needs-measurement`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: 20:22-26:35. Provenance: `youtube:5JXpNOZWAHM`.
  - Qualification/correction: The displayed n^1.32 framing is informal; the exact recurrence and its additive/rounding terms determine the bound.
  - Jet evidence: DataStats.rs:313-348; current source-kernel differential and timing probes; #3010 blocks full Jet smoke. Action: Record assumptions and input partitions; measure before changing the canonical kernel. Owner: #3011.

- **`group-three`**: The textbook groups-of-three recurrence has noncontracting recursive-size mass.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `needs-measurement`.
  - Source: <https://www.cs.cmu.edu/~15451-f23/lectures/lecture01-filled.pdf>; locator: 27:53-31:48. Provenance: `CMU:15451-f23-lecture01`.
  - Qualification/correction: This describes the standard construction, not an impossibility theorem for every algorithm using triples.
  - Jet evidence: DataStats.rs:313-348; current source-kernel differential and timing probes; #3010 blocks full Jet smoke. Action: Record assumptions and input partitions; measure before changing the canonical kernel. Owner: #3011.

- **`group-five`**: T(n)<=T(n/5)+T(7n/10)+O(n) gives a worst-case linear selection algorithm.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `needs-measurement`.
  - Source: <https://www.cs.cmu.edu/~15451-f23/lectures/lecture01-filled.pdf>; locator: 31:50-34:15; CMU pp30-35. Provenance: `CMU:15451-f23-lecture01`.
  - Qualification/correction: Five is sufficient in this construction, not a universal runtime-optimal engineering constant.
  - Jet evidence: DataStats.rs:313-348; current source-kernel differential and timing probes; #3010 blocks full Jet smoke. Action: Record assumptions and input partitions; measure before changing the canonical kernel. Owner: #3011.

- **`group-size-cost`**: Larger fixed odd groups can keep linear asymptotics while changing constants.
  - Evidence: `audience`; confidence `medium`; stance `supports`; classification `needs-measurement`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: comments UgxSa8f...; UgxRjfW.... Provenance: `comments:5JXpNOZWAHM`.
  - Qualification/correction: Choose no new group-size policy without representative measurements; Rust already supplies a hybrid.
  - Jet evidence: DataStats.rs:313-348; current source-kernel differential and timing probes; #3010 blocks full Jet smoke. Action: Record assumptions and input partitions; measure before changing the canonical kernel. Owner: #3011.

- **`median-contract`**: Even-list statistical median and lower/upper order statistics are different contracts.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `needs-measurement`.
  - Source: <https://rcoh.me/posts/linear-time-median-finding/>; locator: 22:52-23:23. Provenance: `blog:rcoh-2018`.
  - Qualification/correction: Jet quantile interpolates adjacent ranks. Replacing it with one middle element changes behavior.
  - Jet evidence: DataStats.rs:313-348; current source-kernel differential and timing probes; #3010 blocks full Jet smoke. Action: Record assumptions and input partitions; measure before changing the canonical kernel. Owner: #3012.

- **`duplicate-partition`**: Distinct-value teaching examples omit an important production partition case.
  - Evidence: `primary`; confidence `medium`; stance `supports`; classification `needs-measurement`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: 06:37-06:48;35:07-35:13. Provenance: `youtube:5JXpNOZWAHM`.
  - Qualification/correction: Use equal partitions or equivalent duplicate-aware behavior; test all-equal, few-distinct and tie order.
  - Jet evidence: DataStats.rs:313-348; current source-kernel differential and timing probes; #3010 blocks full Jet smoke. Action: Record assumptions and input partitions; measure before changing the canonical kernel. Owner: #3012.

- **`temporary-sublists`**: Pedagogical list partitioning can hide allocation and copying.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `needs-measurement`.
  - Source: <https://rcoh.me/posts/linear-time-median-finding/>; locator: 05:36-06:53;12:03-13:05. Provenance: `blog:rcoh-2018`.
  - Qualification/correction: Asymptotic comparison count does not establish in-place or constant-space behavior.
  - Jet evidence: DataStats.rs:313-348; current source-kernel differential and timing probes; #3010 blocks full Jet smoke. Action: Record assumptions and input partitions; measure before changing the canonical kernel. Owner: #3012.

- **`timing-sample`**: The video times random lists up to about 3000 and averages 100 trials.
  - Evidence: `primary`; confidence `medium`; stance `supports`; classification `needs-measurement`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: 34:21-35:38. Provenance: `youtube:5JXpNOZWAHM`.
  - Qualification/correction: Not representative of every size, input distribution, runtime or hardware.
  - Jet evidence: DataStats.rs:313-348; current source-kernel differential and timing probes; #3010 blocks full Jet smoke. Action: Record assumptions and input partitions; measure before changing the canonical kernel. Owner: #3011.

- **`empirical-worst`**: The largest comparison sample from 100 trials is not a worst-case theorem.
  - Evidence: `primary`; confidence `medium`; stance `supports`; classification `needs-measurement`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: 36:08-36:57. Provenance: `youtube:5JXpNOZWAHM`.
  - Qualification/correction: Force hostile pivots and distinguish operation counts from wall time.
  - Jet evidence: DataStats.rs:313-348; current source-kernel differential and timing probes; #3010 blocks full Jet smoke. Action: Record assumptions and input partitions; measure before changing the canonical kernel. Owner: #3011.

- **`total-job-cost`**: Deterministic pivot work can outweigh asymptotic advantages on practical jobs.
  - Evidence: `primary`; confidence `medium`; stance `supports`; classification `needs-measurement`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: 35:14-36:05. Provenance: `youtube:5JXpNOZWAHM`.
  - Qualification/correction: Local experiment confirms plain selection loses on ordered inputs; account for setup, validation, allocations and full output cost.
  - Jet evidence: DataStats.rs:313-348; current source-kernel differential and timing probes; #3010 blocks full Jet smoke. Action: Record assumptions and input partitions; measure before changing the canonical kernel. Owner: #3012.

- **`selection-usage`**: The title presents this selection method as unused.
  - Evidence: `primary`; confidence `low`; stance `supports`; classification `rejected-conflict`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: title;37:00-38:22. Provenance: `youtube:5JXpNOZWAHM`.
  - Qualification/correction: Literal claim contradicted by current Rust introselect with a median-of-medians/Tukey-ninther fallback.
  - Jet evidence: DataStats.rs:313-348; current source-kernel differential and timing probes; #3010 blocks full Jet smoke. Action: none

- **`adversarial-deadline`**: Audience connects deterministic bounds with denial-of-service resistance and time-critical work.
  - Evidence: `audience`; confidence `medium`; stance `supports`; classification `needs-measurement`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: UgzaLXX2P9HeB_fasrx4AaABAg;Ugwrb2ZGk74p0jH0n5V4AaABAg. Provenance: `comments:5JXpNOZWAHM`.
  - Qualification/correction: O(n) is not a hardware deadline or worst-case execution-time certificate; input, allocation, scheduler and target bounds remain necessary.
  - Jet evidence: DataStats.rs:313-348; current source-kernel differential and timing probes; #3010 blocks full Jet smoke. Action: Record assumptions and input partitions; measure before changing the canonical kernel. Owner: #3011.

- **`micro-syntax`**: Selecting one partition instead of sorting both is a change of work, not new syntax.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: 12:03-15:42. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3012.

- **`micro-ergonomics`**: The obvious statistical operation should hide pivot and adjacent-rank bookkeeping.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: 22:52-23:23;35:14-36:05. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3012.

- **`micro-surfaces`**: Median, quantile, top-k and sorted partitions have different output requirements.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: 12:03-15:42;22:52-23:23. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3011.

- **`micro-apis-types-methods`**: Rust exposes select_nth_unstable and comparator/key variants; Jet public selection is not assumed present.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: linked Rust slice API. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3011.

- **`micro-defaults`**: A robust hybrid can use a cheap path and deterministic fallback without a user strategy switch.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: 35:14-36:05; linked Rust implementation. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3012.

- **`micro-naming`**: Statistical median must not silently become a lower or upper middle element.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: 22:52-23:23. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3012.

- **`micro-diagnostics`**: The video has no error UI; linked API panics on invalid rank while Jet checked statistics return typed errors.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: Rust slice API; DataStats.rs:313-348. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3012.

- **`micro-ux-dx`**: A cost report must show guarantee, distribution and actual cost separately.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: 34:21-36:57. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3011.

- **`micro-tooling-cli`**: No source CLI contract justifies a new Jet audit command.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: full transcript reviewed. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3011.

- **`micro-ceremony-control`**: Hide pivot selection; preserve existing typed expert ordering controls and maintainer-only strategy experiments.
  - Evidence: `inference`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://www.youtube.com/watch?v=5JXpNOZWAHM>; locator: linked Rust comparator/key API. Provenance: `inference:Main`.
  - Qualification/correction: No source-level API or syntax is proposed by this audit.
  - Jet evidence: Core registry; Collections.rs; existing .measure test mode; I1-I9. Action: Apply this constraint to the named existing mechanism or recorded investigation. Owner: #3011.

### linked-sources: 12 claims

- **`selection-usage`**: Rust ships a median-of-medians-family deterministic fallback in introselect.
  - Evidence: `linked-source`; confidence `high`; stance `disputes`; classification `already-implemented`.
  - Source: <https://doc.rust-lang.org/std/primitive.slice.html#method.select_nth_unstable>; locator: Current slice API and core/slice/sort/select.rs. Provenance: `rust:std-selection`.
  - Jet evidence: Same-run source and std selection experiments; #3011 cost model. Action: none

- **`selection-partition-api`**: Rust exposes in-place unstable selection returning left, selected and right partitions.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `already-implemented`.
  - Source: <https://doc.rust-lang.org/std/primitive.slice.html#method.select_nth_unstable>; locator: select_nth_unstable, select_nth_unstable_by, select_nth_unstable_by_key. Provenance: `rust:std-selection`.
  - Jet evidence: Same-run source and std selection experiments; #3011 cost model. Action: none

- **`float-total-order`**: Float total order differs from ordinary partial comparisons for NaN and signed zero.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://doc.rust-lang.org/std/primitive.f64.html#method.total_cmp>; locator: total_cmp. Provenance: `rust:std-selection`.
  - Jet evidence: Same-run source and std selection experiments; #3011 cost model. Action: Preserve Jet finite-input rejection and signed-zero policy; do not silently import a different float ordering. Owner: #3012.

- **`bfprt-original-bound`**: The original PICK publication reports at most 5.4305n comparisons.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `already-implemented`.
  - Source: <https://collaborate.princeton.edu/en/publications/time-bounds-for-selection/>; locator: 1973 publication record. Provenance: `paper:BFPRT73`.
  - Jet evidence: Same-run source and std selection experiments; #3011 cost model. Action: none

- **`partial-metric`**: The linked Cohen graph excludes median-of-medians pivot-computation work.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `rejected-conflict`.
  - Source: <https://rcoh.me/posts/linear-time-median-finding/>; locator: Practical considerations. Provenance: `blog:rcoh-2018`.
  - Jet evidence: Same-run source and std selection experiments; #3011 cost model. Action: none

- **`regalloc-api`**: regalloc2 exposes MachineEnv, PReg, VReg, run and Output for target constraints and allocation results.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `needs-measurement`.
  - Source: <https://github.com/bytecodealliance/regalloc2/blob/main/doc/GENERAL.md>; locator: public allocator interface. Provenance: `https://github.com/bytecodealliance/regalloc2/blob/main/doc/GENERAL.md`.
  - Qualification/correction: These are upstream implementation/maintainer controls, not a request for new Jet syntax.
  - Jet evidence: Jet AOT delegates to rustc/LLVM; JIT uses Cranelift0.112; shared MIR facts remain canonical.. Action: Use relevant target constraints, phase costs and checked results in the existing maintainer evidence view. Owner: #3011.

- **`regalloc-checker`**: regalloc2 documents precise liveness, bundle cost, sparse structures and a checker used with fuzzing.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `needs-measurement`.
  - Source: <https://github.com/bytecodealliance/regalloc2/blob/main/doc/ION.md>; locator: design and testing. Provenance: `https://github.com/bytecodealliance/regalloc2/blob/main/doc/ION.md`.
  - Qualification/correction: These are upstream implementation/maintainer controls, not a request for new Jet syntax.
  - Jet evidence: Jet AOT delegates to rustc/LLVM; JIT uses Cranelift0.112; shared MIR facts remain canonical.. Action: Use relevant target constraints, phase costs and checked results in the existing maintainer evidence view. Owner: #3011.

- **`cranelift-block-parameters`**: Cranelift uses SSA block parameters and explicit stack slots.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `needs-measurement`.
  - Source: <https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md>; locator: SSA and stack slots. Provenance: `https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md`.
  - Qualification/correction: These are upstream implementation/maintainer controls, not a request for new Jet syntax.
  - Jet evidence: Jet AOT delegates to rustc/LLVM; JIT uses Cranelift0.112; shared MIR facts remain canonical.. Action: Use relevant target constraints, phase costs and checked results in the existing maintainer evidence view. Owner: #3011.

- **`isle-generated-rules`**: Cranelift ISLE expresses lowering rules compiled into Rust.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `needs-measurement`.
  - Source: <https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/isle-integration.md>; locator: ISLE integration. Provenance: `https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/isle-integration.md`.
  - Qualification/correction: These are upstream implementation/maintainer controls, not a request for new Jet syntax.
  - Jet evidence: Jet AOT delegates to rustc/LLVM; JIT uses Cranelift0.112; shared MIR facts remain canonical.. Action: Use relevant target constraints, phase costs and checked results in the existing maintainer evidence view. Owner: #3011.

- **`allocator-defaults`**: LLVM documents different debug and production allocator defaults and an llc allocator-selection control.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `needs-measurement`.
  - Source: <https://llvm.org/docs/CodeGenerator.html#built-in-register-allocators>; locator: Fast, Greedy and -regalloc. Provenance: `https://llvm.org/docs/CodeGenerator.html`.
  - Qualification/correction: These are upstream implementation/maintainer controls, not a request for new Jet syntax.
  - Jet evidence: Jet AOT delegates to rustc/LLVM; JIT uses Cranelift0.112; shared MIR facts remain canonical.. Action: Use relevant target constraints, phase costs and checked results in the existing maintainer evidence view. Owner: #3011.

- **`backend-filetests`**: Cranelift filetests and verifier diagnostics provide instruction/location-bound feedback.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `needs-measurement`.
  - Source: <https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/testing.md>; locator: filetests and verifier. Provenance: `https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/testing.md`.
  - Qualification/correction: These are upstream implementation/maintainer controls, not a request for new Jet syntax.
  - Jet evidence: Jet AOT delegates to rustc/LLVM; JIT uses Cranelift0.112; shared MIR facts remain canonical.. Action: Use relevant target constraints, phase costs and checked results in the existing maintainer evidence view. Owner: #3011.

- **`abi-target-specific`**: System V AMD64 specifies concrete argument, stack, register-save and related obligations.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `needs-measurement`.
  - Source: <https://gitlab.com/x86-psABIs/x86-64-ABI/-/jobs/artifacts/master/raw/x86-64-ABI/abi.pdf?job=build>; locator: psABI1.0, 2025-03-12. Provenance: `https://gitlab.com/x86-psABIs/x86-64-ABI/-/jobs/artifacts/master/raw/x86-64-ABI/abi.pdf?job=build`.
  - Qualification/correction: These are upstream implementation/maintainer controls, not a request for new Jet syntax.
  - Jet evidence: Jet AOT delegates to rustc/LLVM; JIT uses Cranelift0.112; shared MIR facts remain canonical.. Action: Use relevant target constraints, phase costs and checked results in the existing maintainer evidence view. Owner: #3011.

### jet-local: 28 claims

- **`current-build-failure`**: Fresh main compiler build fails with E0061 at MIRWeb js_call_values migration.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `real-gap`.
  - Source: `MIRWeb.rs:5695,7435; captured cargo build exit101`; locator: MIRWeb.rs:5695,7435; captured cargo build exit101. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: MIRWeb.rs:5695,7435; captured cargo build exit101. Action: Migrate the remaining caller with the real function and alias arguments, then rebuild and exercise Web calls. Owner: #3010.

- **`map-extrema-order`**: Production comptime map min/max use display strings and return wrong numeric results for [2,10].
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `real-gap`.
  - Source: `Builtins.rs:2363-2372; fresh standalone production-library contrast`; locator: Builtins.rs:2363-2372; fresh standalone production-library contrast. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: Builtins.rs:2363-2372; fresh standalone production-library contrast. Action: Share numeric value ordering across map operations and tiers. Owner: #3013.

- **`map-topn-policy`**: Runtime top_n uses bounded sorted-prefix insertion; comptime sorts every entry and has a string-order fallback.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `real-gap`.
  - Source: `Collections.rs:753-788; Builtins.rs:2341-2361`; locator: Collections.rs:753-788; Builtins.rs:2341-2361. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: Collections.rs:753-788; Builtins.rs:2341-2361. Action: Consolidate ordering policy and measure m-by-k strategy costs before changing the internal algorithm. Owner: #3013.

- **`data-one-kernel`**: Checked statistics source is included by AOT, JIT and comptime adapters.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `already-implemented`.
  - Source: `DataStats.rs:1-7; tests/data_one_kernel.rs`; locator: DataStats.rs:1-7; tests/data_one_kernel.rs. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: DataStats.rs:1-7; tests/data_one_kernel.rs. Action: none

- **`quantile-sort-cost`**: Single quantile clones and sorts all values, even though only one adjacent rank pair is needed.
  - Evidence: `local-evidence`; confidence `medium`; stance `supports`; classification `needs-measurement`.
  - Source: `DataStats.rs:313-348; source-kernel experiment`; locator: DataStats.rs:313-348; source-kernel experiment. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: DataStats.rs:313-348; source-kernel experiment. Action: Qualify adaptive selection without losing numeric or ordered-input behavior. Owner: #3012.

- **`describe-repeat-work`**: describe recomputes sum/mean and variance/stddev through separate calls.
  - Evidence: `local-evidence`; confidence `medium`; stance `supports`; classification `needs-measurement`.
  - Source: `DataStats.rs:384-401`; locator: DataStats.rs:384-401. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: DataStats.rs:384-401. Action: First reuse already-computed scalar results; only fuse passes if arithmetic and error order are preserved. Owner: #3012.

- **`median-registry`**: A median helper and plain comptime arm exist, while adjacent Core/registry rows expose describe and quantile.
  - Evidence: `local-evidence`; confidence `medium`; stance `supports`; classification `needs-measurement`.
  - Source: `Core.jet:544-546; core_calls.rs:2032-2034; plain_calls.rs:2176-2219`; locator: Core.jet:544-546; core_calls.rs:2032-2034; plain_calls.rs:2176-2219. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: Core.jet:544-546; core_calls.rs:2032-2034; plain_calls.rs:2176-2219. Action: Determine intended reachability through canonical declarations; do not add an API based on a helper name. Owner: #2986.

- **`mir-dominance-duplication`**: MIR legality and optimization maintain separate predecessor/dominance implementations.
  - Evidence: `local-evidence`; confidence `medium`; stance `supports`; classification `needs-measurement`.
  - Source: `MIR.rs:7999-8072; MIROptimization.rs:3488-3572`; locator: MIR.rs:7999-8072; MIROptimization.rs:3488-3572. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: MIR.rs:7999-8072; MIROptimization.rs:3488-3572. Action: Measure graph shape and traversal/allocation counts; reuse one view only after checking failure/unwind root policy. Owner: #3011.

- **`mir-revalidation-cost`**: The optimization pipeline repeatedly validates whole MIR; fingerprint and actual operation lists differ in purpose.
  - Evidence: `local-evidence`; confidence `medium`; stance `supports`; classification `needs-measurement`.
  - Source: `MIROptimization.rs:4226-4366; MIR.rs:2244-2269`; locator: MIROptimization.rs:4226-4366; MIR.rs:2244-2269. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: MIROptimization.rs:4226-4366; MIR.rs:2244-2269. Action: Measure validation cost and pipeline idempotence; do not remove correctness fences or equate internal operation rows with pass identity. Owner: #3011.

- **`incremental-frontier`**: Query and bundle invalidation still scan broad memo/dependency sets.
  - Evidence: `local-evidence`; confidence `medium`; stance `supports`; classification `needs-measurement`.
  - Source: `jet-queries/src/lib.rs:109-175,246-287; Bundle.rs:768-894`; locator: jet-queries/src/lib.rs:109-175,246-287; Bundle.rs:768-894. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: jet-queries/src/lib.rs:109-175,246-287; Bundle.rs:768-894. Action: Compare reverse dependency frontiers and shared validity memoization on chain/star/cycle edits. Owner: #3011.

- **`effect-worklist-existing`**: Effects already use a shared deterministic output-sensitive reachability worklist.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `already-implemented`.
  - Source: `Effects.rs:1408-1465; Facts.rs:659-757`; locator: Effects.rs:1408-1465; Facts.rs:659-757. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: Effects.rs:1408-1465; Facts.rs:659-757. Action: none

- **`unit-resolution-frontier`**: Derived unit resolution repeatedly scans unresolved declarations and imports.
  - Evidence: `local-evidence`; confidence `medium`; stance `supports`; classification `needs-measurement`.
  - Source: `Bundle/Units.rs:196-459`; locator: Bundle/Units.rs:196-459. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: Bundle/Units.rs:196-459. Action: Measure indexed reverse-frontier resolution while preserving visibility, ambiguity and diagnostics. Owner: #3011.

- **`sort-key-once`**: Checked key sorting evaluates keys once before mutating the receiver.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `already-implemented`.
  - Source: `Core/SortKernel.rs:1-29; Collections.rs:359-441`; locator: Core/SortKernel.rs:1-29; Collections.rs:359-441. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: Core/SortKernel.rs:1-29; Collections.rs:359-441. Action: none

- **`memo-cost-model`**: Vec-backed LRU operations scale with capacity and clone returned values.
  - Evidence: `local-evidence`; confidence `medium`; stance `supports`; classification `needs-measurement`.
  - Source: `Prelude/Memo.rs:8-53,81-149`; locator: Prelude/Memo.rs:8-53,81-149. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: Prelude/Memo.rs:8-53,81-149. Action: Measure capacity, key and clone costs, including zero/one/unbounded and reserved-slot behavior before changing representation. Owner: #3011.

- **`bitset-cost-model`**: BitSet uses BTreeSet rather than a dense bit vector.
  - Evidence: `local-evidence`; confidence `medium`; stance `supports`; classification `needs-measurement`.
  - Source: `Collections.rs:114-163`; locator: Collections.rs:114-163. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: Collections.rs:114-163. Action: Measure density and maximum-ID partitions; sparse huge IDs must not cause dense allocation blowups. Owner: #3011.

- **`fft-existing-owner`**: A naive quadratic FFT/DFT hotspot is already homed.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: `Tower #2780`; locator: Tower #2780. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: Tower #2780. Action: Keep FFT complexity/performance evidence with its existing owner. Owner: #2780.

- **`measure-existing`**: Jet already has .measure test claims and jet test --measure selection.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `already-implemented`.
  - Source: `tests/jet_measure.rs:1-101; CLI.rs:2041; CmdCompile.rs:6271-6273,6319,7397-7398`; locator: tests/jet_measure.rs:1-101; CLI.rs:2041; CmdCompile.rs:6271-6273,6319,7397-7398. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: tests/jet_measure.rs:1-101; CLI.rs:2041; CmdCompile.rs:6271-6273,6319,7397-7398. Action: none

- **`test-economics-controls`**: Existing test-economics self-test rejects six false-green controls.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `already-implemented`.
  - Source: `scripts/agent/test-economics.mjs --self-test --json exit0`; locator: scripts/agent/test-economics.mjs --self-test --json exit0. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: scripts/agent/test-economics.mjs --self-test --json exit0. Action: none

- **`qualification-not-selftest`**: Passing evidence-tool controls does not qualify the product or permit test deletion.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: `Tower #2952 criterion2 blocked; #2919/#2858 open`; locator: Tower #2952 criterion2 blocked; #2919/#2858 open. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: Tower #2952 criterion2 blocked; #2919/#2858 open. Action: Run candidate-bound composed proof under existing gates; retain unique behavioral detectors. Owner: #2919.

- **`compiler-proof-not-universal`**: Compiler-proof manifest check validates an identity-bound proof claim, not all compiler behavior.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `already-implemented`.
  - Source: `scripts/agent/compiler-proof.mjs --check --json; #2925`; locator: scripts/agent/compiler-proof.mjs --check --json; #2925. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: scripts/agent/compiler-proof.mjs --check --json; #2925. Action: none

- **`stale-evidence-existing`**: Candidate qualification already invalidates stale proof/test/artifact claims.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `already-implemented`.
  - Source: `Tower #2943; proof/compiler/composition/contract.json`; locator: Tower #2943; proof/compiler/composition/contract.json. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: Tower #2943; proof/compiler/composition/contract.json. Action: none

- **`full-cost-inventory-gap`**: No complete maintained algorithm-cost inventory was found in the scoped source/Tower search.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `real-gap`.
  - Source: `Core source ledger + test economics + Tower search; #3011`; locator: Core source ledger + test economics + Tower search; #3011. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: Core source ledger + test economics + Tower search; #3011. Action: Extend existing source-derived evidence records; retain every uncovered operation. Owner: #3011.

- **`bootstrap-gates-open`**: Bootstrap and professional-handoff gates are not complete.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: `Tower #217,#670,#2420 frozen; #2919 building`; locator: Tower #217,#670,#2420 frozen; #2919 building. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: Tower #217,#670,#2420 frozen; #2919 building. Action: Keep readiness claims conditional; do not reopen or bypass owner-gated bootstrap work. Owner: #217.

- **`test-outcome-projection`**: Runtime and CLI duplicate the same five outcome counters and expected-failure precedence.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `real-gap`.
  - Source: `Prelude/TestReport.rs:18-44; CmdCompile.rs:6104-6136`; locator: read source blocks. Provenance: `jet:working-tree-2026-09-10`.
  - Jet evidence: D-REPORT-TEST1; two current production loops. Action: Use one borrowed production projection without turning independent tests into self-checks. Owner: #3014.

- **`measurement-protocol-existing`**: collect_measure_evidence uses the ordinary test harness and requires release serial AOT, five warmups and twenty exact samples.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `already-implemented`.
  - Source: `Source/CmdDevTools.rs:8288-8505`; locator: JETTESTMEASURE1 and JETALLOC1. Provenance: `jet:working-tree-2026-09-10`.
  - Qualification/correction: This producer is AOT-specific; do not label its output as JIT/interpreter/Web performance.
  - Jet evidence: D-CLAIM-BENCH1; BenchEvidence; normal test evidence identities. Action: none

- **`core-discovery-duplication`**: Core conformance and the surface-ledger checker separately parse the same CoreModuleExports declarations.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `real-gap`.
  - Source: `core-conformance.mjs:474-511; check-core-surface-ledger.mjs:1717-1731`; locator: current source parsers. Provenance: `jet:working-tree-2026-09-10`.
  - Qualification/correction: Share source discovery only; keep independent source-versus-generated-view and witness admission checks.
  - Jet evidence: CoreModuleExports.rs; existing source-ledger comparison. Action: Use the canonical discovery result for the algorithm denominator without adding another parser/catalog. Owner: #3011.

- **`provider-specific-measurement`**: Timing/allocation and service evidence require20 samples, while scene evidence requires600 per metric.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: `CmdDevTools.rs:8399-8422,8479-8497,8587-8595,8708-8719`; locator: existing provider parsers. Provenance: `jet:working-tree-2026-09-10`.
  - Qualification/correction: Different provider contracts are not a defect. Missing or malformed samples must not become successful evidence.
  - Jet evidence: BenchEvidence, ServiceEvidence and SceneEvidence. Action: Share identity/availability envelopes while retaining domain-specific measurement policies. Owner: #3011.

- **`independent-hash-oracle`**: A validator self-check reconstructing report identity should not automatically share the production hash computation.
  - Evidence: `inference`; confidence `medium`; stance `disputes`; classification `rejected-conflict`.
  - Source: `tools/ci/compiled-workload-gate.sh; tools/ci/test-compiled-workload-gate.sh`; locator: review of consolidation suggestion. Provenance: `inference:Main`.
  - Qualification/correction: Independent expected-value reconstruction can detect omissions that a shared implementation would reproduce.
  - Jet evidence: Test economics requires preserved independent detector value.. Action: none

### peer-assurance: 18 claims

- **`rust-ui-oracle`**: Rust UI tests make pass/fail expectation and diagnostic output explicit.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://rustc-dev-guide.rust-lang.org/tests/ui.html>; locator: Cited primary documentation; bounded evidence review. Provenance: `https://rustc-dev-guide.rust-lang.org/tests/ui.html`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: tests/golden.rs and Jet UI snapshots. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #2919.

- **`rust-codegen-mir-tests`**: Rust separates codegen/MIR/run-make checks rather than treating successful compilation as runtime correctness.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://rustc-dev-guide.rust-lang.org/tests/compiletest.html#test-suites>; locator: Cited primary documentation; bounded evidence review. Provenance: `https://rustc-dev-guide.rust-lang.org/tests/compiletest.html`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: proof/compiler/observations/comparison-journal.mjs. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #2919.

- **`incremental-hostile-cases`**: Rust incremental and rustfix tests exercise edits and repaired programs.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://rustc-dev-guide.rust-lang.org/tests/compiletest.html#incremental-tests>; locator: Cited primary documentation; bounded evidence review. Provenance: `https://rustc-dev-guide.rust-lang.org/tests/compiletest.html`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: jet-queries and IncrementalSemaCache. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #3011.

- **`coverage-not-correctness`**: Coverage and crash tests expose different failures; reachability alone is not correctness.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://rustc-dev-guide.rust-lang.org/tests/compiletest.html#coverage-tests>; locator: Cited primary documentation; bounded evidence review. Provenance: `https://rustc-dev-guide.rust-lang.org/tests/compiletest.html`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: Core ledger and test-economics obligations. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #3011.

- **`bootstrap-stage-identity`**: Bootstrap stages have different compiler/std freshness and trust meanings.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://rustc-dev-guide.rust-lang.org/building/bootstrapping/what-bootstrapping-does.html#stages-of-bootstrapping>; locator: Cited primary documentation; bounded evidence review. Provenance: `https://rustc-dev-guide.rust-lang.org/building/bootstrapping/what-bootstrapping-does.html`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: Tower #217/#218/#670. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #217.

- **`bootstrap-ddc`**: Self-rebuild equality is not Diverse Double-Compiling or semantic correctness.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://dwheeler.com/trusting-trust/dissertation/html/wheeler-trusting-trust-ddc.html>; locator: Cited primary documentation; bounded evidence review. Provenance: `https://dwheeler.com/trusting-trust/dissertation/html/wheeler-trusting-trust-ddc.html`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: Tower #217/#218/#670; proof toolchain identities. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #217.

- **`ecosystem-real-programs`**: Crater tests real ecosystem consequences but covers only its selected build/test configurations.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://rustc-dev-guide.rust-lang.org/tests/ecosystem.html#crater>; locator: Cited primary documentation; bounded evidence review. Provenance: `https://rustc-dev-guide.rust-lang.org/tests/ecosystem.html`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: Gauntlet workload portfolio; bootstrap dogfood gate. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #2858.

- **`dynamic-model-limits`**: Miri detects concrete undefined behavior within a bounded model and executed schedules, not universal soundness.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://github.com/rust-lang/miri/blob/master/README.md>; locator: Cited primary documentation; bounded evidence review. Provenance: `https://github.com/rust-lang/miri/blob/master/README.md`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: proof/compiler/runtime/contract.json; deterministic-world tests. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #2919.

- **`fuzz-minimized-regressions`**: Defined-input fuzzing, reduction and retained regressions complement example-based testing.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://rustc-dev-guide.rust-lang.org/fuzzing.html>; locator: Cited primary documentation; bounded evidence review. Provenance: `https://rustc-dev-guide.rust-lang.org/fuzzing.html`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: tests/fuzz/sema/differential/manifest.tsv. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #2919.

- **`differential-independence`**: Rustlantis compares defined deterministic MIR programs across engines but does not cover the whole frontend.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://plf.inf.ethz.ch/research/oopsla24-rustlantis.html>; locator: Cited primary documentation; bounded evidence review. Provenance: `https://plf.inf.ethz.ch/research/oopsla24-rustlantis.html`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: tests/dev_parts/support.rs:2713-2850; tests/tir_support/mod.rs:242-315. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #2919.

- **`llvm-tool-reuse`**: LLVM lit/FileCheck and small minimized tests distinguish runner orchestration from output predicates.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://llvm.org/docs/TestingGuide.html#llvm-testing-infrastructure-organization>; locator: Cited primary documentation; bounded evidence review. Provenance: `https://llvm.org/docs/TestingGuide.html`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: existing golden/UI/codegen harnesses. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #3011.

- **`target-matrix-gates`**: LLVM/GCC release testing spans configurations and targets; one successful host run is insufficient.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://llvm.org/docs/ReleaseProcess.html#overview-of-the-release-process>; locator: Cited primary documentation; bounded evidence review. Provenance: `https://llvm.org/docs/ReleaseProcess.html`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: Gauntlet and release target matrix. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #2858.

- **`formal-boundary`**: CompCert proves a specified semantic-preservation relation with explicit excluded tool stages and premises.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://compcert.org/man/manual001.html#semantic-preservation>; locator: Cited primary documentation; bounded evidence review. Provenance: `https://compcert.org/man/manual001.html`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: proof/compiler/composition/contract.json. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #2919.

- **`formal-model-correspondence`**: RustBelt proves a formal language/library subset, not rustc or every Rust program.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://plv.mpi-sws.org/rustbelt/popl18/>; locator: Cited primary documentation; bounded evidence review. Provenance: `https://plv.mpi-sws.org/rustbelt/popl18/`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: proof/compiler/checking/contract.json; toolchain manifest. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #2919.

- **`proof-assurance-levels`**: SPARK distinguishes flow, runtime safety and functional proof; deterministic work budgets differ from wall timeouts.
  - Evidence: `linked-source`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://docs.adacore.com/spark2014-docs/html/ug/en/usage_scenarios.html#levels-of-software-assurance>; locator: Cited primary documentation; bounded evidence review. Provenance: `https://docs.adacore.com/spark2014-docs/html/ug/en/usage_scenarios.html`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: docs/spec/proof-replay-decisions.md; Core evidence rows. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #3011.

- **`qualification-scope`**: Ferrocene qualification names tools, library subsets, releases and host/target combinations.
  - Evidence: `linked-source`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://public-docs.ferrocene.dev/main/qualification/evaluation-plan/qualification-scope.html>; locator: Cited primary documentation; bounded evidence review. Provenance: `https://public-docs.ferrocene.dev/main/qualification/evaluation-plan/qualification-scope.html`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: existing runtime contract and professional-handoff gate. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #217.

- **`oracle-provenance`**: Studies find generated assertions can be wrong or reproduce observed behavior; Jet-specific correlation remains unmeasured.
  - Evidence: `linked-source`; confidence `medium`; stance `supports`; classification `ratified-in-progress`.
  - Source: <https://arxiv.org/abs/2410.21136>; locator: Cited primary documentation; bounded evidence review. Provenance: `https://arxiv.org/abs/2410.21136`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: test-economics mutation evidence; independent relation manifest. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #2919.

- **`one-source-inventory`**: Jet already derives Core inventory from executable source instead of a duplicated generated catalog.
  - Evidence: `local-evidence`; confidence `high`; stance `supports`; classification `ratified-in-progress`.
  - Source: `scripts/agent/check-core-surface-ledger.mjs:12-23,44-73`; locator: Cited primary documentation; bounded evidence review. Provenance: `jet:source-ledger`.
  - Qualification/correction: No claim of universal bug freedom, certification or Jet-specific empirical transfer.
  - Jet evidence: canonical Core roots; #2986. Action: Join this obligation to existing maintainer evidence and preserve its scope and unsupported cases. Owner: #3011.

</details>

### Exact-topic matrix

104 claims; 92 topics: 90 single-provenance topics, one independently repeated topic and one conflict.

| Topic | Result | Resolution |
| --- | --- | --- |
| `total-job-cost` | repeated | The two videos independently motivate whole-job cost; the local experiments show why input structure matters. |
| `selection-usage` | conflict | Reject literal non-use. Rust’s implementation ships a median-of-medians-family fallback. Standalone textbook BFPRT prevalence was not measured. |
| Ten `micro-*` topics | single provenance | The two source-specific rows are Main’s synthesis, not independent endorsements. |

<details>
<summary>All topic classifications and provenance</summary>

```json
[{"topic":"abi-observations","status":"single","source_identities":["youtube:YlFYXewYJ8M"],"resolution":null},{"topic":"abi-target-specific","status":"single","source_identities":["https://gitlab.com/x86-psABIs/x86-64-ABI/-/jobs/artifacts/master/raw/x86-64-ABI/abi.pdf?job=build"],"resolution":null},{"topic":"adversarial-deadline","status":"single","source_identities":["comments:5JXpNOZWAHM"],"resolution":null},{"topic":"allocator-defaults","status":"single","source_identities":["https://llvm.org/docs/CodeGenerator.html"],"resolution":null},{"topic":"backend-filetests","status":"single","source_identities":["https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/testing.md"],"resolution":null},{"topic":"bfprt-original-bound","status":"single","source_identities":["paper:BFPRT73"],"resolution":null},{"topic":"bitset-cost-model","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"bootstrap-ddc","status":"single","source_identities":["https://dwheeler.com/trusting-trust/dissertation/html/wheeler-trusting-trust-ddc.html"],"resolution":null},{"topic":"bootstrap-gates-open","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"bootstrap-stage-identity","status":"single","source_identities":["https://rustc-dev-guide.rust-lang.org/building/bootstrapping/what-bootstrapping-does.html"],"resolution":null},{"topic":"candidate-verifier-separation","status":"single","source_identities":["youtube:YlFYXewYJ8M"],"resolution":null},{"topic":"comparison-model","status":"single","source_identities":["CMU:15451-f23-lecture01"],"resolution":null},{"topic":"compiler-proof-not-universal","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"core-discovery-duplication","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"coverage-not-correctness","status":"single","source_identities":["https://rustc-dev-guide.rust-lang.org/tests/compiletest.html"],"resolution":null},{"topic":"cranelift-block-parameters","status":"single","source_identities":["https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md"],"resolution":null},{"topic":"current-build-failure","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"data-one-kernel","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"describe-repeat-work","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"differential-independence","status":"single","source_identities":["https://plf.inf.ethz.ch/research/oopsla24-rustlantis.html"],"resolution":null},{"topic":"duplicate-partition","status":"single","source_identities":["youtube:5JXpNOZWAHM"],"resolution":null},{"topic":"dynamic-model-limits","status":"single","source_identities":["https://github.com/rust-lang/miri/blob/master/README.md"],"resolution":null},{"topic":"ecosystem-real-programs","status":"single","source_identities":["https://rustc-dev-guide.rust-lang.org/tests/ecosystem.html"],"resolution":null},{"topic":"effect-worklist-existing","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"empirical-worst","status":"single","source_identities":["youtube:5JXpNOZWAHM"],"resolution":null},{"topic":"fft-existing-owner","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"float-total-order","status":"single","source_identities":["rust:std-selection"],"resolution":null},{"topic":"formal-boundary","status":"single","source_identities":["https://compcert.org/man/manual001.html"],"resolution":null},{"topic":"formal-model-correspondence","status":"single","source_identities":["https://plv.mpi-sws.org/rustbelt/popl18/"],"resolution":null},{"topic":"full-cost-inventory-gap","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"fuzz-minimized-regressions","status":"single","source_identities":["https://rustc-dev-guide.rust-lang.org/fuzzing.html"],"resolution":null},{"topic":"group-five","status":"single","source_identities":["CMU:15451-f23-lecture01"],"resolution":null},{"topic":"group-size-cost","status":"single","source_identities":["comments:5JXpNOZWAHM"],"resolution":null},{"topic":"group-three","status":"single","source_identities":["CMU:15451-f23-lecture01"],"resolution":null},{"topic":"half-subset-pivot","status":"single","source_identities":["youtube:5JXpNOZWAHM"],"resolution":null},{"topic":"incremental-frontier","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"incremental-hostile-cases","status":"single","source_identities":["https://rustc-dev-guide.rust-lang.org/tests/compiletest.html"],"resolution":null},{"topic":"independent-hash-oracle","status":"single","source_identities":["inference:Main"],"resolution":null},{"topic":"isle-generated-rules","status":"single","source_identities":["https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/isle-integration.md"],"resolution":null},{"topic":"jit-allocation-choice","status":"single","source_identities":["youtube:YlFYXewYJ8M"],"resolution":null},{"topic":"joint-codegen-cost","status":"single","source_identities":["youtube:YlFYXewYJ8M"],"resolution":null},{"topic":"llvm-tool-reuse","status":"single","source_identities":["https://llvm.org/docs/TestingGuide.html"],"resolution":null},{"topic":"map-extrema-order","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"map-topn-policy","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"measure-existing","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"measurement-protocol-existing","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"median-contract","status":"single","source_identities":["blog:rcoh-2018"],"resolution":null},{"topic":"median-registry","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"memo-cost-model","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"micro-apis-types-methods","status":"single","source_identities":["inference:Main"],"resolution":null},{"topic":"micro-ceremony-control","status":"single","source_identities":["inference:Main"],"resolution":null},{"topic":"micro-defaults","status":"single","source_identities":["inference:Main"],"resolution":null},{"topic":"micro-diagnostics","status":"single","source_identities":["inference:Main"],"resolution":null},{"topic":"micro-ergonomics","status":"single","source_identities":["inference:Main"],"resolution":null},{"topic":"micro-naming","status":"single","source_identities":["inference:Main"],"resolution":null},{"topic":"micro-surfaces","status":"single","source_identities":["inference:Main"],"resolution":null},{"topic":"micro-syntax","status":"single","source_identities":["inference:Main"],"resolution":null},{"topic":"micro-tooling-cli","status":"single","source_identities":["inference:Main"],"resolution":null},{"topic":"micro-ux-dx","status":"single","source_identities":["inference:Main"],"resolution":null},{"topic":"mir-dominance-duplication","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"mir-revalidation-cost","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"one-source-inventory","status":"single","source_identities":["jet:source-ledger"],"resolution":null},{"topic":"optimization-hardness","status":"single","source_identities":["youtube:YlFYXewYJ8M"],"resolution":null},{"topic":"oracle-provenance","status":"single","source_identities":["https://arxiv.org/abs/2410.21136"],"resolution":null},{"topic":"partial-metric","status":"single","source_identities":["blog:rcoh-2018"],"resolution":null},{"topic":"peephole-semantics","status":"single","source_identities":["youtube:YlFYXewYJ8M"],"resolution":null},{"topic":"pivot-not-free","status":"single","source_identities":["youtube:5JXpNOZWAHM"],"resolution":null},{"topic":"proof-assurance-levels","status":"single","source_identities":["https://docs.adacore.com/spark2014-docs/html/ug/en/usage_scenarios.html"],"resolution":null},{"topic":"provider-specific-measurement","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"qualification-not-selftest","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"qualification-scope","status":"single","source_identities":["https://public-docs.ferrocene.dev/main/qualification/evaluation-plan/qualification-scope.html"],"resolution":null},{"topic":"quantile-sort-cost","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"quickselect-expected","status":"single","source_identities":["CMU:15451-f23-lecture01"],"resolution":null},{"topic":"regalloc-api","status":"single","source_identities":["https://github.com/bytecodealliance/regalloc2/blob/main/doc/GENERAL.md"],"resolution":null},{"topic":"regalloc-checker","status":"single","source_identities":["https://github.com/bytecodealliance/regalloc2/blob/main/doc/ION.md"],"resolution":null},{"topic":"register-allocation-model","status":"single","source_identities":["youtube:YlFYXewYJ8M"],"resolution":null},{"topic":"rust-codegen-mir-tests","status":"single","source_identities":["https://rustc-dev-guide.rust-lang.org/tests/compiletest.html"],"resolution":null},{"topic":"rust-ui-oracle","status":"single","source_identities":["https://rustc-dev-guide.rust-lang.org/tests/ui.html"],"resolution":null},{"topic":"selection-partition-api","status":"single","source_identities":["rust:std-selection"],"resolution":null},{"topic":"selection-usage","status":"conflict","source_identities":["rust:std-selection","youtube:5JXpNOZWAHM"],"resolution":"Literal non-use rejected: Rust ships the family as a fallback."},{"topic":"selection-vs-sort","status":"single","source_identities":["CMU:15451-f23-lecture01"],"resolution":null},{"topic":"sort-key-once","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"stale-evidence-existing","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"target-matrix-gates","status":"single","source_identities":["https://llvm.org/docs/ReleaseProcess.html"],"resolution":null},{"topic":"target-specific-scheduling","status":"single","source_identities":["youtube:YlFYXewYJ8M"],"resolution":null},{"topic":"temporary-sublists","status":"single","source_identities":["blog:rcoh-2018"],"resolution":null},{"topic":"test-economics-controls","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"test-outcome-projection","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"timing-sample","status":"single","source_identities":["youtube:5JXpNOZWAHM"],"resolution":null},{"topic":"total-job-cost","status":"repeated","source_identities":["youtube:5JXpNOZWAHM","youtube:YlFYXewYJ8M"],"resolution":null},{"topic":"unit-resolution-frontier","status":"single","source_identities":["jet:working-tree-2026-09-10"],"resolution":null},{"topic":"virtual-registers","status":"single","source_identities":["youtube:YlFYXewYJ8M"],"resolution":null}]
```

</details>


## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 current compiler build | card | #3010 |
| F2 numeric map extrema and ordering policy | card | #3013 |
| F3 plain-selection counterexample | card | #3012 |
| F4 statistics work and retained performance loss | card | #3012 |
| F5 tool controls versus product qualification | card | #2919 |
| F6 existing maintainer integration | card | #3011 |
| F7 full first-party denominator | card | #3011 |
| F8 honest algorithm cost records | card | #3011 |
| F9 evidence invalidation | card | #3011; reuse implemented #2943 |
| F10 consolidation with preserved detector value | card | #3011 |
| F10a borrowed test outcome projection | card | #3014 |
| F10b Core source discovery | card | #3011 |
| F10c independent hash oracle | no-action | Do not merge an independent expected-identity oracle into production without preserved defect detection. No change recommended. |
| F11 compiler and collection hypotheses | card | #3011; statistics #3012; map #3013; existing FFT #2780 |
| F11 median public reachability | card | #2986 |
| F11 existing effect and sort kernels | no-action | Already present in source. Keep their shared semantics; no replacement is justified by this mine. |
| F12 peer assurance obligations | card | #2919 |
| F13 AI oracle independence | card | #2919 |
| F14 production and bootstrap scope | card | #217; existing #2919, #2858, #2420 and #670 retain their gates |
| F14 new public mechanism or certification choice | no-action | None proposed. Owner-selected maintainer scope and existing decisions govern; no new ballot or gate reopening is authorized. |
| F15 competitive evidence | card | #2858 |
| F16 agent and full product goals | card | #2393; algorithm evidence #3011 |
| F17 concrete surface checks | card | #3011; reachability #2986; statistics #3012; map #3013 |
| Source corrections and unsupported title claims | no-action | Corrected through primary sources and retained as dated evidence. They do not justify a Jet feature by themselves. |
<!-- /audit-dispositions -->

**Strongest unverified assumption:** the promising source-kernel and compiler-structure hypotheses will improve complete Jet workloads across every required mode, target and input family without weakening semantics. That transfer has not been established.
