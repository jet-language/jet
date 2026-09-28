# Mine for Jet: Casey Muratori, performance and expert code generation

Dated 2026-09-10. Supplemental mine for Tower #3015. This report preserves, rather than replaces, the [earlier same-day mine](mine-for-jet-2026-09-10.md).

## Verdict

The strongest lesson is not “write assembly” or “replace LLVM.” Preserve a good algorithm and data layout through the compiler, then inspect and measure the code that actually runs. A local hotspot can be the wrong target when the whole job contains repeated work or a serial network chain.

The previous five cards were justified, but did not exhaust the product questions. This pass adds two worked owner ballots: **D-CODE-INSPECT1** on #3016 for exact generated-code inspection, and **D-RANK-SELECT1** on #3018 for a non-shipping rank-selection prototype. Neither is ratified or implemented. The rank ballot authorizes only a prototype. A separate public-adoption ballot must wait for canonical candidate/plain performance proof.

Inline assembly is already approved. Current source has concrete contract gaps, now homed on **#3017**, rather than reopening the approved syntax. LLVM-backed AOT remains the native-quality baseline. Cranelift remains the existing low-latency backend. QBE, MIR, TinyCC and Cuik/TB merit bounded evidence, not an unsupported replacement promise.

A standalone experiment verified nine native variants on six input sizes. QBE compiled its SSA input much faster than LLVM compiled equivalent IR, but its million-element sum took about twice as long. The dependent-load kernel showed little difference. **These are not Jet results or a Gauntlet pass.** The fresh Jet build failed before a new binary was produced.

## Source coverage and limits

- Current playlist capture contained one new video: [Why performant code matters (but gets widely ignored), with Casey Muratori](https://www.youtube.com/watch?v=8xBJPa_480Q), The Pragmatic Engineer, published 2026-08-26. Metadata: 1:53:58, 200,209 views, 173 comments at capture. Popularity is not technical evidence.
- Duplicate-source checking classified this video as new. The previous two videos were not recaptured; their retained evidence was explicitly revisited at the owner's request.
- All 3,337 normalized timestamp lines, about 23,767 words, were reviewed. The body reader used twelve bounded slices; Main separately read lines 1681–1683, closing a three-line gap in that receipt. Coverage runs from 00:00 through approximately 113:56.
- No creator subtitle track existed. Both English tracks were automatic captions. Transcript claims are qualified accordingly. The full video download failed HTTP 403. No frame-by-frame review is claimed.
- The [publisher article](https://newsletter.pragmaticengineer.com/p/why-performant-code-matters-but-gets) and its assembly image were read. Its NASM-style example uses `mov`, `add`, `sub` and `syscall`; it is not Jet syntax. Other on-screen details unavailable without video frames remain unverified.
- All 173 captured audience records were read: 103 roots and 70 replies. Strata included ten top-liked roots, recent roots, low-visibility technical comments, substantive replies and corrections. Sets overlap; they are not a survey.
- yt-dlp first estimated 175 records, then extracted 173; final metadata also said 173. This does not establish coverage of hidden, deleted, filtered or later comments.
- Linked sources were checked against official compiler/API documentation, Casey's articles, Linear's production account and the cited test-oracle paper. Antithesis's sponsor URL supplied no substantive evidence after redirection; its official documentation supported only the narrower testing mechanisms.
- Tool retrieval used isolated Nix packages. A garnix DNS warning did not prevent acquisition. `nixpkgs#cproc` was unavailable. The first probe source construction was corrupted by the host's percent-line magic; corrected source was compiled and only the successful run is reported below.

## What the argument supports

1. **Architecture can dominate local code.** Casey's 23:06–42:50 discussion distinguishes a replaceable implementation from a serial dependency chain that an API already exposes. Batch known work, avoid unnecessary round trips, and retain an explicit effect/retry contract. Do not invent automatic retries for unsafe effects.
2. **Use a hardware bound as a question, not a result.** Derive expected bytes, instructions, dependencies and available parallelism. Measure the gap. A roofline or instruction estimate does not prove the workload is correct or at its optimum.
3. **Read generated code before assuming what source costs.** Inlining, dispatch, copies, bounds checks, spills, vectorization and calling conventions can change the result. Writing assembly and reading compiler output are different activities.
4. **The clean-code demonstration is narrower than its slogan.** The linked [benchmark](https://www.computerenhance.com/p/clean-code-horrible-performance) reports roughly 1.5× for the flat/switch step; larger gains also change representation, layout and vectorization. “Switch is 15× faster” is not supported. [Rust trait objects](https://doc.rust-lang.org/book/ch18-02-trait-objects.html) retain a valid open-extension use case.
5. **Testing has costs, but independent oracles remain essential.** Casey's TDD comments are contextual, not “never test.” The [oracle study](https://arxiv.org/abs/2410.21136) shows that generated assertions can agree with wrong behavior. Its Java/model-specific results do not establish a rate for current Jet agents.
6. **Concrete-first APIs can stay deep.** [Semantic Compression](https://caseymuratori.com/blog_0015) argues for learning the real common structure before abstraction. Jet should hide algorithm choices behind one semantic operation, not multiply `fast_*` spellings.

### Corrections and audience evidence

| Claim or signal | Evidence and correction | Jet consequence |
| --- | --- | --- |
| N+1 database calls and serial network chains hurt real jobs | Comments `Ugz7CnPFNsAul6r-Sip4AaABAg` and `UgxHhgLAhH2RzSUcL8V4AaABAg`; a reply reports 118 queries. Anecdote, not prevalence or a measured multiplier. | #3011 must count calls and waits at whole-job scope. |
| All DB calls should take single-digit milliseconds | Comment `UgzFmYYlHJgtND9FJF54AaABAg.Aa17KdPEKs4Aa3XBjP2KiT` overclaims. Query structure, data, storage and concurrency matter; use [PostgreSQL EXPLAIN](https://www.postgresql.org/docs/current/using-explain.html). | No universal latency threshold or magical indexing promise. |
| Python is always about 100× slower | The episode gives an illustrative magnitude, not a reproduced benchmark. The 1BRC audience comparison omits extension/JIT/backend details. [Python extension modules](https://docs.python.org/3/extending/extending.html) make language labels insufficient. | Record exact implementation, runtime and native boundary for every peer. |
| Rust/Godot is nearly 3× slower | `Ugyaf1vuagn9xv0Q9td4AaABAg` lacks code, flags, ABI and harness. | Treat as a hypothesis, not a language ranking. |
| Premature optimization means never consider speed early | [Casey's source discussion](https://www.computerenhance.com/p/theroot) distinguishes context and contested attribution. | Separate irreversible architecture from swappable local algorithms. |
| AI quality or industry effects are settled | The episode and comments mix preference, autonomy, fatigue and unverified forecasts. | Measure oracle quality and repair outcomes; do not legislate enthusiasm or opposition. |
| Assembly literacy is mandatory for every programmer | Audience disagrees; no population evidence settles this. | Safe beginner defaults, optional expert descent through the same tools. |

Cross-resource conflicts are resolved by mechanism, not votes. The earlier selection sources describe worst-case bounds; this episode emphasizes hardware and whole-job cost. Both can be true. Linear's [delta-sync account](https://linear.app/now/rebuilding-delta-sync-read-path) is a concrete workload, not proof that one set algorithm wins everywhere. The topic matrix preserves the `selection-usage` dispute and the new `interpreter-overhead` dispute.

## Jet findings: observed, source-confirmed and still unproven

### F01. The current binary cannot be claimed fresh

`scripts/agent/jet-env cargo build --bin jet -j 4`, with `CARGO_INCREMENTAL=0`, exited **101**. Rust reported **E0282** at [MIRRust.rs:10455](../../crates/jet-codegen/src/Codegen/MIRRust.rs#L10455): the `BTreeMap<_, _>` type was unknown at `previous.same_checked_type`.

This is the current observed blocker on #3010. The previous report's MIRWeb failure remains historical evidence, not the current first error. No stale `target/debug/jet` was used to claim current `run`, `build`, interpreter, web or asm behavior. This report completes research; it does not complete product implementation or release proof.

### F02–F07. The earlier maintainer design still holds

| Requirement | Current evidence | Disposition |
| --- | --- | --- |
| Algorithms at all abstraction levels | Whole-job request graph; semantic API; data structure/layout; MIR legality; machine code; scheduling and cache behavior are distinct cost levels. | #3011 extends its existing inventory, not a new subsystem. |
| Runtime and compiler costs | [MIR legality](../../crates/jet-foundation/src/MIR.rs#L7999) repeats predecessor/dominator work; [MIROptimization](../../crates/jet-foundation/src/MIROptimization.rs#L3488) carries further validation. Existing effects/facts worklists already exist. | Measure phase time and invalidation frontier before changing the pipeline. No duplicate worklist. |
| Complete first-party API denominator | [Collection registry](../../crates/jet-foundation/src/Collections.rs), Core declarations and source-derived inventory are authoritative. Handwritten feature lists are not. | #3011 and #2986; missing/unsupported/unmeasured rows stay visible. |
| Statistics and selection | [quantile](../../crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs#L313) clones and sorts; `describe` repeats scans and calls. Existing interpolation/error/zero rules matter. | #3012 internal selection and shared scans; not authorization for a new API. |
| Map extrema and top_n | The prior production-helper contrast found numeric extrema `[2,10]` ordered by display text. Current source still exposes the relevant split. | #3013; retain numeric/tie/partial-order oracles, not a string workaround. No new runtime reproduction here. |
| Optional maintainer test measurements | `JETTESTMEASURE1`, `JETALLOC1`, TestReport and #2952 already carry useful measurement/economics contracts. A separate CLI loop remains in CmdCompile. | #3014 projects canonical results; #3011 adds inventory/cost evidence. No public annotations or compulsory profiling. |
| Trustworthy complexity metadata | Worst/expected/amortized bounds, comparison/key cost, memory, preconditions and provenance differ from observed slope or wall time. | #3011 records all separately, with source/target/profile/tool/input identity and expiry. |

The same mechanism must enumerate compiler, codegen, runtime, embedded Prelude and CoreLib algorithms/APIs. A known cost is not a correctness certificate. Coverage is not a proof of semantics. Complexity annotations cannot replace measured runtime or full-job costs.

### F08. Exact machine output is a real expert tooling gap

The [inspect registry](../../crates/jet-cli/src/CLI.rs#L267) has nine planes and further nested actions, but no assembly/disassembly view. [emit --rust](../../Source/CmdDevTools.rs#L7910) prints application MIR's Rust output, not prepared hidden FFI bridge code. [inspect decisions](../../Source/CmdInspect.rs#L3517) exposes reasons and identities, not final instructions.

**D-CODE-INSPECT1 / #3016** compares a source-linked `jet inspect code` projection, standalone emit files, and external tools only. The recommendation joins artifacts to existing decision records. It explicitly amends observe/record law: new --capture-code is required for bounded machine capture; --observe alone and ordinary dev do not enable it. It distinguishes pre-link assembly from final linked/JIT bytes. Normal runs remain unchanged; expert observation and bounded retention are opt-in. The detailed ballot defines selectors, status/exit behavior, privacy, inlining, stale records and unavailable output.

This is not a second semantic analysis or a new backend selector. Assembly shows what was emitted. It does not prove timing, constant-time behavior, safety or optimality.

### F09–F11. Assembly control exists; repair its contract

The existing example is real source, not new syntax:

```jet
use core.mem

#[Unsafe("the operands are scalar registers and add does not access memory"), FFI(asm)] fn add_registers(a: Int, b: Int) Int -> {
    """add {a}, {b} ; -> return"""
}
```

D-FFI-ASM1 approved inline assembly. **D-FFI-ASMOPS1=C** later approved named placeholders, the return anchor and declared clobbers. Archived #501 is historical acceptance; later I9 remains binding. No ballot is needed to reapprove these features.

- **Operand lowering:** [FFI.rs:5097–5101](../../crates/jet-pkg-model/src/FFI.rs#L5097) lowers declared clobbers as `lateout`. The ratified contract specifies `out` for discarded clobbers. An early-written clobber must not overlap an input consumed later. This is a source-confirmed contract defect; a new Jet miscompile has not been executed here.
- **Output inference:** [FFI.rs:5070–5133](../../crates/jet-pkg-model/src/FFI.rs#L5070) chooses an output from the return-line placeholder or textual first operand, with `rax` fallback. The checked operand model and actual lowering need one authoritative contract. Reading assembly text is not a proof of the programmer's unsafe obligations.
- **Selected target:** [FFI.rs:454–470](../../crates/jet-pkg-model/src/FFI.rs#L454) accepts only an `x86_64` target prefix. Its suggestion to provide a selected-target body does not bypass that unconditional rejection. A single raw-body AST alone does **not** prove conditional function variants are impossible; implementation must first exercise existing target-selection machinery.
- **Execution modes:** [TIR applicability](../../crates/jet-codegen/src/Codegen/TIR/mod.rs#L10269) marks accepted native inline foreign functions unavailable to Cranelift and interpreter. [Strict JIT](../../crates/jet-jit/src/jit/backend.rs#L22) rejects a plan gap. That source path conflicts with later native-mode parity law; current end-to-end reproduction is blocked by F01. A web target cannot execute arbitrary native ISA bytes; any inapplicable target scope must be explicit, not disguised as delayed implementation.

All four investigations are homed on **#3017**, with #3010 as the executable-proof prerequisite. Keep `#Unsafe`, sema-owned checks, registered diagnostics and target identity. Do not expose raw rustc errors as Jet user errors. Do not claim static instruction validation proves arbitrary assembly safe.

### F12–F14. Backend policy and expert improvement loop

Jet currently delegates AOT machine generation to Rust/LLVM. Default fast execution uses Cranelift under the documented routing conditions. [Resident JIT](../../crates/jet-jit/src/jit/runtime_host.rs#L10125) and [debug AOT](../../crates/jet-jit/src/jit/api_debug.rs#L70) explicitly set `opt_level=none`. This is a source fact, not a measured reason to flip a switch.

For experts, the useful loop is: identify the full job → verify its result → find cost by phase → inspect checked decisions → inspect matching machine output → change one cause → compare matched evidence. Use allocation/copy counts, instruction/byte counts, spills, branches, vectorization and debug mapping only when a real producer supplies them. [llvm-mca](https://llvm.org/docs/CommandGuide/llvm-mca.html) estimates scheduling/resource pressure; it is not a runtime timer.

| Backend | Grounded capability and risk | Decision for Jet |
| --- | --- | --- |
| LLVM | Mature target lowering, optimization, vectorizers, register allocation, debug information and assembly/object emission. | Keep the AOT quality baseline. Improve fact preservation and visibility first. |
| Cranelift | Existing Jet integration; prioritizes code-generation latency, verification and production Wasm/JIT use. Current Jet uses version 0.112 and disables optimization. | Measure none versus approved optimization settings under #2919; do not infer current upstream performance applies to this older integration. |
| QBE | Small SSA backend, C ABI, amd64/AArch64/RISC-V targets. Its “70% performance in 10% code” is an author claim. | Screen compile latency, runtime and size; no replacement evidence. |
| MIR project | Lightweight JIT/interpreter with optimization and published small-C-program comparisons. Not Jet's MIR representation. | Candidate evidence only. Published GCC comparisons do not transfer to Jet. |
| TinyCC | One-pass C compiler with direct binary generation and libtcc; limited optimizing/vector scope. Optional bounds checks do not confer Jet safety. | Low-latency contrast, not native-quality baseline. |
| Cuik/TB | Separate frontend/backend; repository explicitly calls the compiler unfinished and buggy. Optimizer breadth is not maturity proof. | No adoption claim. Benchmark and verify only if the bounded screen warrants it. |

“Not LLVM” is not one category: GCC and Go use their own backend pipelines; Rust normally uses LLVM; Zig offers LLVM and self-hosted backends. C and C++ must distinguish GCC from Clang. A small backend can compile faster while producing slower instructions. None of these facts proves a whole-language ranking.

## Executed backend experiment

This throwaway experiment isolates two kernels, not Jet or a production workload. Sources, exact commands, full samples, independent oracle and hashes are retained below. Nine variants passed both outputs at six sizes: 1, 2, 16, 1,024, 65,536 and 1,048,576. Each process used five warmups and twenty measured samples. Compile medians use twenty retained invocations after one warmup.

Host: x86-64 Linux, AMD Ryzen 9 7950X3D. The runner pinned itself to allowed CPU 0. Compiler inputs selected baseline x86-64 rather than host-native tuning. A common C driver generated unsigned 64-bit data; Python computed expected sums and dependent-load results independently. Arithmetic was explicit modulo 2^64. The C++ row compiles the same C-compatible kernel as C++, not a distinct idiomatic C++ program.

The QBE and LLVM-IR compile rows both stop at assembly. C/C++/Rust/Zig rows stop at object files. Final common-driver linking is outside those timings. Zig used fresh local/global cache directories per compile sample. Go's c-archive was built once, so no Go compile median is claimed; its runtime row includes the cgo boundary.

The dependent-load graph can revisit a subset of the table. It is not a proof of full-working-set memory latency. Background load, CPU governor, thermals, cache state and processor scheduling were not controlled as a release benchmark. No statistical confidence interval or multi-host result is claimed. Only the two kernel outputs were checked; floating-point, aliasing, exceptions, full ABI, debug quality, SIMD completeness and tier parity were not exercised.

| Variant | Compiler invocation median, ms | Sum, ns/element | Dependent load, ns/step | Linked .text bytes |
| --- | ---: | ---: | ---: | ---: |
| qbe | 1.709 | 0.2259 | 2.6427 | 1522 |
| llvm-ir | 23.612 | 0.1101 | 2.6581 | 1677 |
| clang-c | 26.424 | 0.1085 | 2.6703 | 1702 |
| gcc-c | 20.775 | 0.1129 | 2.6825 | 1612 |
| clang-cpp | 26.976 | 0.1108 | 2.6606 | 1702 |
| rust-llvm | 29.766 | 0.1119 | 2.6489 | 1677 |
| zig-llvm | 422.051 | 0.1978 | 2.6390 | 1695 |
| zig-native | 419.953 | 0.6372 | 2.6990 | 1763 |
| go | not measured | 0.2316 | 2.8838 | 514654 |

Runtime columns use 1,048,576 elements/steps. Linked text includes the common driver and runtime support, not just the kernels. Go includes its runtime and is not a like-for-like kernel-size result.

QBE/LLVM-IR invocation latency was about 0.0724, or 13.8× lower for these inputs. QBE/LLVM-IR sum time was about 2.05, or twice as slow. Disassembly explains a plausible cause: QBE emitted a scalar sum loop; LLVM emitted packed additions plus a tail. Zig LLVM unrolled scalar work; this Zig native path emitted a larger stack-heavy loop. These observations do not predict other programs.

**Can alternatives beat C, C++, Rust, Zig or Go?** On particular matched workloads, yes in principle; this experiment even changes the ranking between its two kernels. There is no evidence here for an across-the-board Jet win or a lightweight replacement that achieves it. Jet must preserve stronger semantic facts, choose better algorithms/data layout, remove avoidable work and reuse shared Core implementation. LLVM peers can exploit the same backend; a frontend win must survive comparison with competent peer source. Backend replacement alone does not create that advantage.

The standing gate remains per cell and metric: every non-Rust Jet/peer ratio must be below 1.00. Rust allows at most 1.05 as noise-band parity, not a win. Missing, wrong, mismatched, unsupported and inconclusive cells fail. #2858/#3011 own the required matrix; #2919 owns integrated executable proof.

## F15–F17. Which new product choices deserve ballots?

### Public rank selection: prototype first, then a separate adoption gate

Existing predicate `partition` groups by a Boolean callback. It does not select a sorted rank. Rust's `select_nth_unstable` and NumPy's `partition` establish useful but different result/mutation contracts.

**D-RANK-SELECT1 / #3018** compares in-place `partition_at` plus existing `get`, non-mutating `nth_smallest`, and keeping selection private. The proposed in-place contract fixes bounds, Float order, ties, key evaluation, mutation and explicit copy. It does not reuse predicate partition's name or add a new order relation. The public names are proposals, not registered features.

The previous report was right that internal quantile/top_n selection needs no new ballot. It was incomplete if read as saying no public selection choice existed. This pass separates those questions. D-RANK-SELECT1 can authorize only an isolated non-shipping prototype. The performance-motivated public API needs a separate adoption ballot after its canonical candidate/plain pair passes the strict gate. This research made no manifest or product edits; #3018 explicitly carries the missing proof.

### Batch quantiles: measure demand before an API vote

Repeated `core.data.quantile(values, q)` calls clone and sort each time. [pandas](https://pandas.pydata.org/docs/reference/api/pandas.Series.quantile.html) accepts scalar or array q with different result shapes. Jet would need to choose q order, duplicate/empty q behavior, output type and error precedence. That is a genuine future product choice, not permission to sneak a `quantiles` alias into #3012. #3011 now owns a concrete repeated-q demand experiment. No API is approved or assumed necessary.

### Typed Query.limit: conditional, not another spelling by default

Typed Query's classifier lacks `limit`, but SQL and the shared table plan already represent LIMIT. Measure a real deferred bounded-stream workload first. An API needs source-read/output-row bounds, sorting interaction, one-shot semantics and inspectable plan identity. Without that need, it duplicates SQL LIMIT or list.take. The demand check is homed on #3011; no unowned “later” recommendation remains.

### Choices deliberately not reopened

- Inline-asm existence, unsafe gate and placeholder/clobber spelling: existing D-FFI-ASM1 and D-FFI-ASMOPS1.
- One compiler-decision view: existing D-INSPECT-DECISIONS1. Exact bytes are the new boundary, not a rival explanation mechanism.
- Backend quality profiles and Cranelift debug-AOT direction: existing D-AOT-CRANELIFT1. Research is not permission for a new public backend switch or dependency.
- Map.top_n, scalar quantile, automatic SIMD/fusion, layout and allocation evidence: existing APIs/decisions or internal implementation work.
- AI-written code, TDD ideology or assembly literacy: no language restriction follows from personal preferences.

## F18–F22. Assurance, complete product goals and agent usefulness

| Owner goal | Concrete consequence | Evidence and owner |
| --- | --- | --- |
| All algorithms/APIs, not sampled “important” ones | Enumerate the complete first-party denominator. Keep missing, unsupported and unmeasured entries visible. | Canonical Core/collection registries; #3011/#2986. |
| Optional maintainer-only test integration | Reuse canonical correctness outcomes and opt-in measurements. No user-facing policy declarations or parallel runner. | TestReport, test economics, JETTESTMEASURE1/JETALLOC1; #3014/#3011. |
| Trustworthy complexity/correctness/performance metadata | Separate formal bounds, assumptions, oracle provenance and observed measurements. Bind all to source, inputs, target, profile and tools. | Existing proof/record identities; #3011. |
| No stale claims or semantic duplication | One source inventory, one checked MIR, one Prelude meaning, projections only. Reject expired evidence instead of refreshing it in a renderer. | I3/I9, D-INSPECT-DECISIONS1; #3011/#3016. |
| AI-heavy development | Challenge the expected result independently; mutation controls, metamorphic/differential tests, fuzz minimization and deliberate hostile inputs. | Oracle paper, prior peer assurance research; #3011/#2919. |
| Production and critical systems | Test deployment failure, cleanup, restart, effect replay, determinism, diagnostics and security. A kernel benchmark or generated test pass is not qualification. | Existing critical-system/security scope; #2919, #217. |
| Bootstrap | Stage identity, compiler correspondence and diverse independent checks remain separate gates. Fast self-compilation is not self-validation. | Existing #217; prior retained bootstrap evidence. |
| Beginner surface | Ordinary semantic operations and direct diagnostics; no required assembly, backend choice or measurement annotation. | Existing defaults; #3016/#3018 remain opt-in proposals. |
| Expert control | Exact selected code, target, ABI, cache/build identity and reasons; explicit unsafe assembly obligations. | #3016/#3017. |
| Ecosystem and competition | Real programs in required domains, competent peers, whole-job correctness, compile/runtime/resource costs and tail behavior. | #2858/#3011; no aggregate hides a loss. |

The Core-conformance hostile-fixture command passed in this run. It rejected hidden/package witnesses, unmarked ordinary files, missing exclusion metadata, bind-and-discard results, observerless/direct calls, comment/string ghosts, malformed markers/calls and denominator rows. This protects coverage accounting; it does not prove every API correct.

### Five agent-optimality quantities

- **Verdict fidelity:** independent expected values and hostile false-green controls beat tests copied from implementation. Weakest point now: no fresh working compiler to exercise changed paths.
- **Verdict latency:** measure check, frontend, MIR, backend, link, startup and steady-state costs separately. Fast Cranelift compilation must not hide slower execution.
- **Verdict actionability:** source-linked decisions and exact artifact identity narrow repairs. An unavailable native bridge plus a hidden backend failure currently leaves too much guesswork.
- **Context economy:** one semantic registry and one report record are better than copied catalogs. The expert can descend into one function rather than consume an entire generated file.
- **Repair determinism:** stable errors, replay identity, independent oracles and exact stale-state rejection reduce trial-and-error. An assembly register heuristic is not a deterministic semantic contract.

### Ranked beat vectors, with status

| Rank | Mechanism | Shipped/source state versus unbuilt work | What peers need to match it |
| --- | --- | --- | --- |
| 1 | One checked meaning through code, diagnostics, decisions and execution modes | Canonical MIR/Prelude structure exists; complete native foreign parity is not proved and has source gaps. | Join compiler, runtime and tooling facts without separate semantic implementations. |
| 2 | Evidence-led repair for humans and agents | Decisions and proof/economics infrastructure exist; exact machine view is proposed. | Supply source-to-byte identity, independent expected results and actionable failure records together. |
| 3 | Efficient semantic APIs without “fast” alternatives | Existing collection/Core surfaces; statistics/top_n improvements remain open. Rank API is an unratified proposal. | Improve algorithms while preserving simple contracts and expert ownership control. |
| 4 | Whole-job competitive evidence | Existing Gauntlet policy and cards; this run is only an exploratory kernel experiment. | Match inputs, setup, semantics, target and output validation, including failures and missing cells. |

None is a proven categorical runtime win. Peers can copy many tooling/API choices. The potential advantage is their consistent combination, not an exclusive algorithm.

### Avoid list

| Mistake | Evidence | Jet exposure and response |
| --- | --- | --- |
| Optimize a loop while ignoring a serial request chain | Episode and N+1 comments | #3011 includes whole-job graphs, waits and query counts. |
| Attribute layout/vector wins to one syntax change | Clean-code linked benchmark | Paired source and artifact evidence; no universal anti-dispatch rule. |
| Treat a smaller backend as faster generated code | QBE experiment and official scope | Keep compile, size and runtime separate. |
| Treat static throughput as measured runtime | llvm-mca's documented model | Producer/method labels and actual runtime oracles. |
| Let generated tests define expected truth | Oracle study | Independent oracles and hostile false-green controls. |
| Expose backend failures as user diagnostics | I2/I3 | Jet owns errors; expert artifacts are a separate opt-in product. |
| Declare native-only feature behavior complete | Current inline foreign applicability | #3017 repairs the applicable mode contract; no hidden fallback. |
| Add public APIs merely to expose private optimizations | rank/batch/query review | Ballot genuine contract choices; tune existing meaning without aliases. |
| Turn audience anecdotes into market or speed statistics | 173-comment review | Retain anonymous locators and qualifications, not applause counts. |

## Micro surface sweep

| Category | Exact surface or mechanism | Jet classification and action |
| --- | --- | --- |
| Syntax | Publisher assembly uses mov/add/sub/syscall; Jet has `#FFI(asm)` and `#Unsafe`. | Existing expert syntax; #3017 repairs law, no new sigil. |
| Ergonomics | Read a small generated function before demanding assembly authoring. | #3016 optional descent; ordinary source remains the default. |
| Surfaces | Source, MIR, backend IR, bridge source, pre-link assembly and final bytes differ. | #3016 exact stage labels and availability. |
| APIs/types/methods | `sort`, `sort_by`, `get`, predicate `partition`, `top_n`, scalar `quantile`; no public rank method. | #3012/#3013 internal work; #3018 owner choice. |
| Defaults | Fast/default/full profiles; Cranelift none; finite checked-statistics inputs; D-FLOATSORT1 collection order. | Do not conflate profile intent, backend policy or Float domains. |
| Naming | Meaningful function/field names help infer intent; cryptic names harmed the cited oracle study. | #3011 keeps semantic names and oracle provenance; no naming-style speed claim. |
| Diagnostics | E3222 hides native stderr; E3223 names operand/target constraints. | #3017 fixes actionable source checks; #3016 does not turn raw errors public. |
| UX/DX | Full-job wait, repeated calls, failure recovery and compiler turnaround matter. | #3011 stage/tail/resource evidence, not a naked stopwatch. |
| Tooling/CLI | `jet emit --rust`, `jet inspect decisions`, `jet inspect accel`; Rust emit, cargo-show-asm, objdump, llvm-mca. | Existing views plus one proposed code view and explicit capture opt-in; no duplicate performance engine. |
| Ceremony/control | TDD, AI and assembly are not universal rituals. | Safe defaults, explicit expert escapes and optional maintainer measurement. |

Covered by source/examples is not equivalent to newly exercised here. Current end-to-end confirmation remains blocked by #3010. Missing views and proposed names are stated explicitly, not added to a shipped-feature inventory.

## Priorities and acceptance

1. Restore the fresh compiler build (#3010), then reproduce and repair unsafe operand/target/native-mode gaps (#3017).
2. Keep the existing algorithm/test evidence work complete and canonical (#3011–#3014, #2986). Prove semantic correctness before timing.
3. Resolve exact-code inspection (#3016) and the non-shipping rank prototype (#3018) through their reviewed ballots. Do not treat “ready ballot” as implementation approval.
4. Measure Cranelift optimization and lightweight alternatives with matched semantics, compile stage, target, ABI, runtime and size. Keep every missing/losing cell open (#3011/#2858/#2919).
5. Establish repeated-q and deferred-query demand before proposing further API surface (#3011). No pending research recommendation is left without a card.

## Review and Tower receipts

Tower accepted both decisions as complete full-profile ballots after these independent passes:

| Ballot | Beginner | Adversarial | Final boundary |
| --- | --- | --- | --- |
| D-CODE-INSPECT1 | CodeInspectBeginner | CodeInspectAdversarial | Proposed exact-output contract; no implementation or ratification. |
| D-RANK-SELECT1 | RankSelectBeginner | RankSelectAdversarial | Preliminary non-shipping prototype authorization only; later public adoption requires paired proof. |

Both adversarial readers reported the OpenAI GPT family. Reviews changed the contracts, rather than supplying approval labels. The inspection review repaired exact input closure, exclusive scope selection, artifact-versus-executed wording, capture activation and retention. The rank review repaired callback failure traces, keyed workspace, ownership limits, Float/tie oracles and the circular pre-adoption proof requirement.

New cards: #3016 inspection, #3017 inline assembly/native-mode repair, #3018 rank prototype. Existing findings were attached to #3010, #3011, #2919 and #2858; #3012/#3013/#3014/#2986/#217 retain their existing coherent scope. Parent #3015 owns this research closeout. A report/card closure is not a product gate pass.

## Evidence identity and reproducibility

Observed HEAD: `5382a5b2055e1c32c8eb02a2f6cbf9e1598c9a6f`. The tree contained concurrent work. HEAD alone does not identify the inspected source; selected file hashes follow. No source edit, manifest edit, staging or commit was performed by this mine. No product implementation is claimed.

Temporary captures and throwaway binaries were stored under `~/.cache/jet-test-scratch/mine-casey-performance-2026-09-10`. They are removed at research closeout, as the mining method requires. The retained material below contains the normalized claims, input sources, command configuration, full measurement samples and hashes. Capture paths in these records are historical locators, not promises of permanent files.

### Capture and source hashes

```json
{
  "capture_sha256": {
    "transcript.txt": "c387dac0a7d58d363de3b125d1c8cc429cb2a520242b8065f1341031c6b88a23",
    "comments.json": "ec7b0d77bd94153f4c1499f55f6c90afda931bf07aaf38d10339cd497a2d3312",
    "8xBJPa_480Q.info.json": "43c9cf756ab47ebd1895712cf5395aaab14d51d68839235bb1261630d36b2543",
    "backend-probe/results.json": "6844c368b8e806607ee5534f29d10b89d8e045db8d93ea429183407d0ba1a6e0",
    "backend-probe/summary.json": "806f4521698da0d1f875561f6d09b475da77325cdbb62ff61467c5c47cb66d09",
    "backend-probe/runner.py": "5c928398ccdea3bd76015153ee2b607d618394fcb5082aaab176f5c317a6930d",
    "backend-probe/size-results.json": "e2e16f06f02fda1076a862630adc138f4e192039869b1554c1b39846f0ba6ba1"
  },
  "source_sha256": {
    "crates/jet-pkg-model/src/FFI.rs": "dbc991f24255eebcde79f038de4c79da0ff61c715c742ae7a7bd038a73330e32",
    "crates/jet-codegen/src/Codegen/MIRRust.rs": "45bb8256979a830790823d5cacabdfdf67372d1bbabd9f047eb2cb3b13ae63d1",
    "crates/jet-cli/src/CLI.rs": "46a31f2853e6894bdd3a1445cf940c1a162ad6fc7457bd7164a1ec0e580eb913",
    "crates/jet-codegen/src/Prelude/Core/FloatOrdering.rs": "7ab866b8bdb9fb9f864331e00f7bf2802657f2860f456dc38c4df9ff7558a79f"
  }
}
```

### Exact tool versions

```json
{
  "clang": {
    "exit": 0,
    "output": [
      "clang version 21.1.8",
      "Target: x86_64-unknown-linux-gnu"
    ]
  },
  "gcc": {
    "exit": 0,
    "output": [
      "gcc (GCC) 15.2.0",
      "Copyright (C) 2025 Free Software Foundation, Inc."
    ]
  },
  "rustc": {
    "exit": 0,
    "output": [
      "rustc 1.95.0 (59807616e 2026-04-14) (built from a source tarball)"
    ]
  },
  "go": {
    "exit": 0,
    "output": [
      "go version go1.26.3 linux/amd64"
    ]
  },
  "zig": {
    "exit": 0,
    "output": [
      "0.16.0"
    ]
  },
  "objdump": {
    "exit": 0,
    "output": [
      "GNU objdump (GNU Binutils) 2.46",
      "Copyright (C) 2026 Free Software Foundation, Inc."
    ]
  },
  "clang++": {
    "exit": 0,
    "output": [
      "clang version 21.1.8",
      "Target: x86_64-unknown-linux-gnu"
    ]
  },
  "g++": {
    "exit": 0,
    "output": [
      "g++ (GCC) 15.2.0",
      "Copyright (C) 2025 Free Software Foundation, Inc."
    ]
  },
  "qbe": {
    "exit": 0,
    "output": [
      "/nix/store/q5izjv3mxdpwmhsqjjarabmq2ql9c0zs-qbe-1.2/bin/qbe [OPTIONS] {file.ssa, -}",
      "\t-h          prints this help"
    ]
  }
}
```

### Probe replay

Restore the source blocks below into one disk-backed directory. Restore the recorded configuration, replacing only its scratch-root paths when necessary. The configuration names exact Nix tool paths and target flags. First run each `compile_commands` entry to produce the initial assembly/object/archive. Then run `runner.py` with that configuration. The runner assembles QBE/LLVM output, links the common driver, checks independent oracles and records samples.

The commands compile unsigned kernels, not safety-equivalent whole-language programs. The shared C driver deliberately excludes language startup from each timed inner loop. Go's FFI boundary remains inside its calls. No profile or compiler default is inferred from a language name.

<details>
<summary>Probe sources, configuration and complete results</summary>

#### kernels.c

```c
#include <stdint.h>
#include <stddef.h>
#ifdef __cplusplus
extern "C" {
#endif
uint64_t sum_u64(const uint64_t *a, size_t n) {
    uint64_t s=0; for(size_t i=0;i<n;i++) s+=a[i]; return s;
}
uint64_t chase_u64(const uint64_t *a, size_t n, uint64_t seed) {
    uint64_t x=seed, mask=n-1; for(size_t i=0;i<n;i++) x=a[x&mask]; return x;
}
#ifdef __cplusplus
}
#endif

```

#### kernels.ssa

```text
export function l $sum_u64(l %a, l %n) {
@entry
 jmp @loop
@loop
 %i =l phi @entry 0, @body %next
 %s =l phi @entry 0, @body %sum
 %more =w cultl %i, %n
 jnz %more, @body, @done
@body
 %off =l mul %i, 8
 %p =l add %a, %off
 %v =l loadl %p
 %sum =l add %s, %v
 %next =l add %i, 1
 jmp @loop
@done
 ret %s
}
export function l $chase_u64(l %a, l %n, l %seed) {
@entry
 %mask =l sub %n, 1
 jmp @loop
@loop
 %i =l phi @entry 0, @body %next
 %x =l phi @entry %seed, @body %v
 %more =w cultl %i, %n
 jnz %more, @body, @done
@body
 %idx =l and %x, %mask
 %off =l mul %idx, 8
 %p =l add %a, %off
 %v =l loadl %p
 %next =l add %i, 1
 jmp @loop
@done
 ret %x
}

```

#### kernels.ll

```llvm
target triple = "x86_64-unknown-linux-gnu"
define i64 @sum_u64(ptr %a, i64 %n) {
entry: br label %loop
loop:
 %i = phi i64 [0, %entry], [%next, %body]
 %s = phi i64 [0, %entry], [%sum, %body]
 %more = icmp ult i64 %i, %n
 br i1 %more, label %body, label %done
body:
 %p = getelementptr i64, ptr %a, i64 %i
 %v = load i64, ptr %p, align 8
 %sum = add i64 %s, %v
 %next = add i64 %i, 1
 br label %loop
done: ret i64 %s
}
define i64 @chase_u64(ptr %a, i64 %n, i64 %seed) {
entry:
 %mask = sub i64 %n, 1
 br label %loop
loop:
 %i = phi i64 [0, %entry], [%next, %body]
 %x = phi i64 [%seed, %entry], [%v, %body]
 %more = icmp ult i64 %i, %n
 br i1 %more, label %body, label %done
body:
 %idx = and i64 %x, %mask
 %p = getelementptr i64, ptr %a, i64 %idx
 %v = load i64, ptr %p, align 8
 %next = add i64 %i, 1
 br label %loop
done: ret i64 %x
}

```

#### kernels.rs

```rust
#![no_std]
// Caller supplies a valid aligned allocation of n u64 values. No pointer escapes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn sum_u64(a: *const u64, n: usize) -> u64 {
    let mut s=0u64; for i in 0..n { s=s.wrapping_add(unsafe { *a.add(i) }); } s
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn chase_u64(a: *const u64, n: usize, seed:u64) -> u64 {
    let mut x=seed; let mask=n.wrapping_sub(1) as u64;
    for _ in 0..n { x=unsafe { *a.add((x&mask) as usize) }; } x
}

```

#### kernels.zig

```zig
export fn sum_u64(a: [*]const u64, n: usize) u64 {
    var s:u64=0; for (0..n) |i| { s +%= a[i]; } return s;
}
export fn chase_u64(a: [*]const u64, n: usize, seed:u64) u64 {
    var x=seed; const mask=n-%1;
    for (0..n) |_| { x=a[@intCast(x&mask)]; } return x;
}

```

#### kernels.go

```go
package main
/*
#include <stdint.h>
#include <stddef.h>
*/
import "C"
import "unsafe"
//export sum_u64
func sum_u64(a *C.uint64_t,n C.size_t) C.uint64_t {
    s:=unsafe.Slice((*uint64)(unsafe.Pointer(a)),int(n))
    var sum uint64
    for _,v:=range s { sum+=v }; return C.uint64_t(sum)
}
//export chase_u64
func chase_u64(a *C.uint64_t,n C.size_t,seed C.uint64_t) C.uint64_t {
    s:=unsafe.Slice((*uint64)(unsafe.Pointer(a)),int(n))
    x:=uint64(seed); mask:=uint64(n)-1
    for i:=0;i<int(n);i++ { x=s[x&mask] }; return C.uint64_t(x)
}
func main() {}

```

#### driver.c

```c
#define _POSIX_C_SOURCE 200809L
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <time.h>
#include <inttypes.h>
uint64_t sum_u64(const uint64_t *,size_t);
uint64_t chase_u64(const uint64_t *,size_t,uint64_t);
static uint64_t ns(void){struct timespec t; clock_gettime(CLOCK_MONOTONIC,&t);return (uint64_t)t.tv_sec*1000000000+t.tv_nsec;}
static uint64_t refsum(const uint64_t*a,size_t n){uint64_t s=0;for(size_t i=0;i<n;i++)s+=a[i];return s;}
static uint64_t refchase(const uint64_t*a,size_t n,uint64_t x){for(size_t i=0;i<n;i++)x=a[x&(n-1)];return x;}
int main(int argc,char**argv){
 size_t n=argc>1?strtoull(argv[1],0,10):65536;
 if(!n || (n&(n-1)) || n>1048576) return 64;
 uint64_t *a=malloc(n*sizeof(*a));if(!a)return 70;
 uint64_t x=123456789;
 for(size_t i=0;i<n;i++){x^=x<<13;x^=x>>7;x^=x<<17;a[i]=x;}
 uint64_t expected=refsum(a,n),expected_chase=refchase(a,n,17);
 if(sum_u64(a,n)!=expected || chase_u64(a,n,17)!=expected_chase)return 1;
 printf("check,%zu,%" PRIu64 ",%" PRIu64 "\n",n,expected,expected_chase);
 volatile uint64_t sink=0;
 for(int phase=-5;phase<20;phase++){
  uint64_t start=ns(); for(int rep=0;rep<64;rep++)sink^=sum_u64(a,n);uint64_t end=ns();
  if(phase>=0)printf("sum,%zu,%d,%" PRIu64 "\n",n,phase,end-start);
  start=ns();for(int rep=0;rep<4;rep++)sink^=chase_u64(a,n,(uint64_t)rep+17);end=ns();
  if(phase>=0)printf("chase,%zu,%d,%" PRIu64 "\n",n,phase,end-start);
 }
 free(a);return sink==UINT64_C(0xdeadbeef)?2:0;
}

```

#### runner.py

```python
import json, os, statistics, subprocess, sys, time
from pathlib import Path
root=Path(__file__).parent
config=json.loads((root/'config.json').read_text())
tools=config['tools']; env=dict(os.environ, **config['environment'])
cpu=min(os.sched_getaffinity(0)); os.sched_setaffinity(0,{cpu})
def run(args):
    start=time.perf_counter_ns()
    r=subprocess.run(args,env=env,text=True,capture_output=True,timeout=180)
    if r.returncode: raise RuntimeError(str(args)+'\n'+r.stderr)
    return r,(time.perf_counter_ns()-start)/1e6
run([tools['clang'],'-O2','-c',str(root/'driver.c'),'-o',str(root/'driver.o')])
for name in ['qbe','llvm-ir']:
    run([tools['clang'],'-c',str(root/(name+'.s')),'-o',str(root/(name+'.o'))])
variants=list(config['compile_commands'])
for name in variants:
    kernel=root/(name+('.a' if name=='go' else '.o'))
    run([tools['clang'],str(root/'driver.o'),str(kernel),'-pthread','-ldl','-lm','-Wl,-z,noexecstack','-o',str(root/name)])
mask=(1<<64)-1
def oracle(n):
    a=[];x=123456789
    for _ in range(n):
        x=(x^(x<<13))&mask;x^=x>>7;x=(x^(x<<17))&mask;a.append(x)
    s=sum(a)&mask;x=17
    for _ in range(n):x=a[x&(n-1)]
    return s,x
expected={n:oracle(n) for n in [1,2,16,1024,65536,1048576]}
results={'cpu':cpu,'expected':expected,'runtime':{},'compile_ms':{},'commands':config['compile_commands']}
# Rotate variant order between two sizes; each process emits 5 warmups then 20 samples.
for n in expected:
    order=variants if n!=1048576 else list(reversed(variants))
    for name in order:
        r,_=run([str(root/name),str(n)])
        lines=r.stdout.splitlines(); check=lines[0].split(',')
        assert check[0]=='check' and tuple(map(int,check[2:]))==expected[n],(name,n,check,expected[n])
        samples={'sum':[],'chase':[]}
        for line in lines[1:]:
            kind,size,seq,value=line.split(',');samples[kind].append(int(value))
        assert all(len(v)==20 for v in samples.values())
        results['runtime'][name+':'+str(n)]=samples
        (root/(name+'-'+str(n)+'.stdout')).write_text(r.stdout)
        print('verified',name,n,flush=True)
# Backend-process cost only; do not equate Go package-cache hits with compiler work.
for name in ['qbe','llvm-ir','clang-c','gcc-c','clang-cpp','rust-llvm','zig-llvm','zig-native']:
    costs=[]
    for i in range(21):
        cmd=list(config['compile_commands'][name])
        if name.startswith('zig-'):
            cmd+=['--cache-dir',str(root/('local-'+name+'-'+str(i))),'--global-cache-dir',str(root/('global-'+name+'-'+str(i)))]
        r,ms=run(cmd)
        if i:costs.append(ms)
    results['compile_ms'][name]=costs
    print('compile',name,statistics.median(costs),flush=True)
(root/'results.json').write_text(json.dumps(results,indent=2))
print('RESULTS',str(root/'results.json'),flush=True)

```

#### config.json

```json
{
  "tools": {
    "clang": "/nix/store/874j5xydsj6nr6i1zdrjvhln5gmxvvrr-clang-wrapper-21.1.8/bin/clang",
    "clang++": "/nix/store/874j5xydsj6nr6i1zdrjvhln5gmxvvrr-clang-wrapper-21.1.8/bin/clang++",
    "gcc": "/nix/store/788mx070y81zjlg5ipcl0cra3afviw9k-gcc-wrapper-15.2.0/bin/gcc",
    "g++": "/nix/store/788mx070y81zjlg5ipcl0cra3afviw9k-gcc-wrapper-15.2.0/bin/g++",
    "rustc": "/nix/store/wnhmqix7bippbbzasj29qiyb422g9asg-rustc-wrapper-1.95.0/bin/rustc",
    "go": "/nix/store/33fw5m31lfcnk4ff2f0df7j2bxnh8lgk-go-1.26.3/bin/go",
    "zig": "/nix/store/n1hqsn4a4yfmipzyk2byavvx4ccx6ccr-zig-0.16.0/bin/zig",
    "qbe": "/nix/store/q5izjv3mxdpwmhsqjjarabmq2ql9c0zs-qbe-1.2/bin/qbe",
    "objdump": "/nix/store/874j5xydsj6nr6i1zdrjvhln5gmxvvrr-clang-wrapper-21.1.8/bin/objdump"
  },
  "environment": {
    "PATH": "/nix/store/874j5xydsj6nr6i1zdrjvhln5gmxvvrr-clang-wrapper-21.1.8/bin:/nix/store/788mx070y81zjlg5ipcl0cra3afviw9k-gcc-wrapper-15.2.0/bin:/nix/store/b1p9a901ds4wck8n1pd76vxnjm34sfap-gcc-15.2.0-man/bin:/nix/store/x720xa1b5sj14cg77i5i0nj4d5qrpbpg-rustc-1.95.0-man/bin:/nix/store/wnhmqix7bippbbzasj29qiyb422g9asg-rustc-wrapper-1.95.0/bin:/nix/store/33fw5m31lfcnk4ff2f0df7j2bxnh8lgk-go-1.26.3/bin:/nix/store/n1hqsn4a4yfmipzyk2byavvx4ccx6ccr-zig-0.16.0/bin:/nix/store/q5izjv3mxdpwmhsqjjarabmq2ql9c0zs-qbe-1.2/bin:/nix/store/5mkrabav193miwsc3ljm2dcfhg4l8zsm-binutils-2.46-man/bin:/nix/store/mbyy19mdwnfvfwmdi0gqgggx0njvpl1w-binutils-wrapper-2.46/bin:/nix/store/s2946bl9ciwzhafd66jhansrmxq9xhqm-binutils-2.46/bin:/nix/store/bsh7n2nx8ndmm1mmww6v2h4851nalj13-glibc-2.42-61-bin/bin:/run/wrappers/bin:/home/nate/.local/share/flatpak/exports/bin:/var/lib/flatpak/exports/bin:/home/nate/.nix-profile/bin:/home/nate/.local/state/nix/profile/bin:/home/nate/.local/state/nix/profile/bin:/etc/profiles/per-user/nate/bin:/nix/var/nix/profiles/default/bin:/run/current-system/sw/bin",
    "CC": "/nix/store/874j5xydsj6nr6i1zdrjvhln5gmxvvrr-clang-wrapper-21.1.8/bin/clang",
    "GOAMD64": "v1",
    "GOMAXPROCS": "1",
    "TMPDIR": "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10",
    "GOCACHE": "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/go-cache",
    "ZIG_GLOBAL_CACHE_DIR": "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/zig-cache"
  },
  "compile_commands": {
    "qbe": [
      "/nix/store/q5izjv3mxdpwmhsqjjarabmq2ql9c0zs-qbe-1.2/bin/qbe",
      "-t",
      "amd64_sysv",
      "-o",
      "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/qbe.s",
      "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.ssa"
    ],
    "llvm-ir": [
      "/nix/store/874j5xydsj6nr6i1zdrjvhln5gmxvvrr-clang-wrapper-21.1.8/bin/clang",
      "-x",
      "ir",
      "-O3",
      "-march=x86-64",
      "-mtune=generic",
      "-S",
      "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.ll",
      "-o",
      "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/llvm-ir.s"
    ],
    "clang-c": [
      "/nix/store/874j5xydsj6nr6i1zdrjvhln5gmxvvrr-clang-wrapper-21.1.8/bin/clang",
      "-O3",
      "-march=x86-64",
      "-mtune=generic",
      "-c",
      "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.c",
      "-o",
      "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/clang-c.o"
    ],
    "gcc-c": [
      "/nix/store/788mx070y81zjlg5ipcl0cra3afviw9k-gcc-wrapper-15.2.0/bin/gcc",
      "-O3",
      "-march=x86-64",
      "-mtune=generic",
      "-c",
      "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.c",
      "-o",
      "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/gcc-c.o"
    ],
    "clang-cpp": [
      "/nix/store/874j5xydsj6nr6i1zdrjvhln5gmxvvrr-clang-wrapper-21.1.8/bin/clang++",
      "-x",
      "c++",
      "-O3",
      "-march=x86-64",
      "-mtune=generic",
      "-c",
      "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.c",
      "-o",
      "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/clang-cpp.o"
    ],
    "rust-llvm": [
      "/nix/store/wnhmqix7bippbbzasj29qiyb422g9asg-rustc-wrapper-1.95.0/bin/rustc",
      "--edition=2024",
      "--crate-type=lib",
      "--emit=obj",
      "-Copt-level=3",
      "-Cpanic=abort",
      "-Ctarget-cpu=x86-64",
      "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.rs",
      "-o",
      "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/rust-llvm.o"
    ],
    "zig-llvm": [
      "/nix/store/n1hqsn4a4yfmipzyk2byavvx4ccx6ccr-zig-0.16.0/bin/zig",
      "build-obj",
      "-fllvm",
      "-O",
      "ReleaseFast",
      "-target",
      "x86_64-linux-gnu",
      "-mcpu",
      "baseline",
      "-femit-bin=/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/zig-llvm.o",
      "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.zig"
    ],
    "zig-native": [
      "/nix/store/n1hqsn4a4yfmipzyk2byavvx4ccx6ccr-zig-0.16.0/bin/zig",
      "build-obj",
      "-fno-llvm",
      "-O",
      "ReleaseFast",
      "-target",
      "x86_64-linux-gnu",
      "-mcpu",
      "baseline",
      "-femit-bin=/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/zig-native.o",
      "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.zig"
    ],
    "go": [
      "env",
      "CC=/nix/store/874j5xydsj6nr6i1zdrjvhln5gmxvvrr-clang-wrapper-21.1.8/bin/clang",
      "PATH=/nix/store/874j5xydsj6nr6i1zdrjvhln5gmxvvrr-clang-wrapper-21.1.8/bin:/nix/store/788mx070y81zjlg5ipcl0cra3afviw9k-gcc-wrapper-15.2.0/bin:/nix/store/b1p9a901ds4wck8n1pd76vxnjm34sfap-gcc-15.2.0-man/bin:/nix/store/x720xa1b5sj14cg77i5i0nj4d5qrpbpg-rustc-1.95.0-man/bin:/nix/store/wnhmqix7bippbbzasj29qiyb422g9asg-rustc-wrapper-1.95.0/bin:/nix/store/33fw5m31lfcnk4ff2f0df7j2bxnh8lgk-go-1.26.3/bin:/nix/store/n1hqsn4a4yfmipzyk2byavvx4ccx6ccr-zig-0.16.0/bin:/nix/store/q5izjv3mxdpwmhsqjjarabmq2ql9c0zs-qbe-1.2/bin:/nix/store/5mkrabav193miwsc3ljm2dcfhg4l8zsm-binutils-2.46-man/bin:/nix/store/mbyy19mdwnfvfwmdi0gqgggx0njvpl1w-binutils-wrapper-2.46/bin:/nix/store/s2946bl9ciwzhafd66jhansrmxq9xhqm-binutils-2.46/bin:/nix/store/bsh7n2nx8ndmm1mmww6v2h4851nalj13-glibc-2.42-61-bin/bin:/run/wrappers/bin:/home/nate/.local/share/flatpak/exports/bin:/var/lib/flatpak/exports/bin:/home/nate/.nix-profile/bin:/home/nate/.local/state/nix/profile/bin:/home/nate/.local/state/nix/profile/bin:/etc/profiles/per-user/nate/bin:/nix/var/nix/profiles/default/bin:/run/current-system/sw/bin",
      "GOCACHE=/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/go-cache",
      "TMPDIR=/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10",
      "GOAMD64=v1",
      "/nix/store/33fw5m31lfcnk4ff2f0df7j2bxnh8lgk-go-1.26.3/bin/go",
      "build",
      "-buildmode=c-archive",
      "-o",
      "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/go.a",
      "/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.go"
    ]
  }
}
```

#### Full samples and independent expected values

```json
{"cpu":0,"expected":{"1":[132100702508589007,132100702508589007],"2":[14703351627002566911,132100702508589007],"16":[10079337664205190020,6055851353308732780],"1024":[3656143017334117007,8275509850155659066],"65536":[11750348009061421910,2864846826420924064],"1048576":[1652615021282648946,12786620344250789881]},"runtime":{"qbe:1":{"sum":[90,90,91,90,91,90,100,100,100,100,90,100,90,100,100,100,100,90,90,91],"chase":[20,20,20,20,20,20,20,20,20,20,10,20,20,20,20,20,20,20,20,20]},"llvm-ir:1":{"sum":[81,80,80,80,80,80,81,80,80,80,80,80,80,80,80,81,80,80,80,80],"chase":[20,20,20,20,20,10,20,10,20,10,21,10,20,20,10,20,20,20,20,20]},"clang-c:1":{"sum":[80,80,80,81,80,80,80,80,80,80,81,80,80,80,80,80,80,81,80,81],"chase":[20,20,20,20,20,20,20,20,20,10,20,20,20,20,20,20,20,20,20,20]},"gcc-c:1":{"sum":[90,101,90,101,90,91,90,90,90,100,90,100,90,90,90,90,90,90,91,100],"chase":[20,20,10,20,20,20,20,20,20,20,20,11,20,20,20,20,20,20,20,20]},"clang-cpp:1":{"sum":[80,81,80,81,80,80,80,80,81,80,81,80,80,80,80,80,80,81,80,80],"chase":[20,20,20,20,20,21,20,20,20,20,20,20,21,20,20,20,20,20,10,20]},"rust-llvm:1":{"sum":[80,80,80,81,70,80,80,80,80,80,81,70,81,80,80,80,80,80,80,81],"chase":[10,20,20,20,20,20,20,21,20,20,20,10,20,20,20,20,20,20,20,20]},"zig-llvm:1":{"sum":[80,80,81,80,80,80,80,80,80,81,80,80,80,80,80,70,81,80,80,80],"chase":[10,20,20,20,20,20,20,20,10,20,20,20,20,20,20,20,20,20,20,20]},"zig-native:1":{"sum":[140,150,150,150,141,140,151,140,140,150,150,141,150,150,140,140,141,140,141,140],"chase":[20,20,20,20,20,20,20,20,20,20,20,20,20,20,20,20,20,20,20,20]},"go:1":{"sum":[2344,2355,2374,2394,2374,2365,2345,2365,2335,2344,2354,2344,2344,2375,2355,2355,2345,2345,2354,2344],"chase":[230,170,181,171,170,190,180,180,180,180,191,181,180,180,180,170,190,180,201,181]},"qbe:2":{"sum":[120,131,120,110,110,120,110,110,120,110,120,120,121,120,111,120,121,120,111,120],"chase":[20,20,21,20,20,20,20,20,20,20,20,20,10,20,20,20,20,20,20,20]},"llvm-ir:2":{"sum":[90,90,90,90,90,100,100,91,100,91,90,91,90,90,90,90,90,90,100,90],"chase":[20,20,10,20,20,20,20,20,20,20,10,20,20,20,10,21,20,20,10,20]},"clang-c:2":{"sum":[90,91,100,91,90,90,100,100,100,90,90,90,90,100,90,90,90,90,101,90],"chase":[20,20,20,20,10,20,20,20,20,20,20,21,20,20,20,20,10,20,20,20]},"gcc-c:2":{"sum":[90,90,100,100,91,100,101,90,91,90,90,90,100,90,90,90,100,90,90,90],"chase":[20,20,20,20,20,20,20,20,20,20,20,20,20,20,20,20,10,20,20,20]},"clang-cpp:2":{"sum":[130,141,140,140,141,140,140,130,140,130,140,141,140,130,141,140,130,141,140,140],"chase":[30,30,20,30,30,30,30,30,30,30,30,30,30,30,20,31,30,30,30,30]},"rust-llvm:2":{"sum":[90,100,90,90,90,90,90,91,90,91,90,101,90,90,90,90,90,90,90,100],"chase":[20,20,10,20,20,20,20,10,20,20,20,20,20,20,20,10,20,11,20,20]},"zig-llvm:2":{"sum":[90,100,90,100,90,90,90,100,90,100,100,90,100,91,90,91,90,91,90,90],"chase":[10,20,20,10,20,20,20,20,20,20,20,20,20,20,20,20,20,20,20,20]},"zig-native:2":{"sum":[190,200,201,190,190,191,190,190,191,190,190,201,190,190,191,190,190,191,190,190],"chase":[20,30,20,20,20,30,20,20,20,31,20,20,20,20,20,20,20,30,20,20]},"go:2":{"sum":[2395,2434,2395,2405,2385,2384,2384,2384,2395,2375,2405,2394,3176,2395,2404,5701,2385,2375,2435,2394],"chase":[160,160,160,160,170,171,161,170,160,170,160,161,170,160,161,170,170,170,171,161]},"qbe:16":{"sum":[451,451,441,451,441,451,441,450,441,441,441,441,451,451,441,451,451,441,441,451],"chase":[50,41,50,40,50,50,51,40,50,40,50,51,40,50,50,40,41,50,40,50]},"llvm-ir:16":{"sum":[170,180,180,170,171,160,170,170,170,171,170,170,170,171,170,171,170,170,171,170],"chase":[40,40,40,40,40,40,40,30,30,40,40,30,40,40,30,40,40,40,30,40]},"clang-c:16":{"sum":[171,181,170,170,171,170,170,171,170,170,171,180,170,161,170,160,171,160,170,171],"chase":[40,40,40,30,40,40,40,40,40,30,40,40,40,40,40,40,40,40,40,40]},"gcc-c:16":{"sum":[180,180,191,180,180,181,181,180,180,181,180,180,191,190,190,181,180,180,181,181],"chase":[50,40,40,40,40,40,40,40,40,40,40,50,50,40,40,40,40,40,40,40]},"clang-cpp:16":{"sum":[181,181,190,180,171,170,180,171,170,170,171,170,170,170,171,170,170,181,170,180],"chase":[40,40,41,40,40,40,40,40,40,40,40,40,30,40,30,40,40,40,40,40]},"rust-llvm:16":{"sum":[171,181,180,170,161,170,170,171,160,170,171,170,170,171,170,170,171,170,170,171],"chase":[40,40,30,40,40,40,40,40,41,40,40,41,40,30,41,40,40,40,40,40]},"zig-llvm:16":{"sum":[140,150,140,140,131,140,130,140,130,141,140,140,141,130,141,140,140,141,130,140],"chase":[40,40,40,40,40,40,40,40,40,40,41,40,40,40,40,41,40,40,40,40]},"zig-native:16":{"sum":[741,742,732,741,741,732,742,741,741,731,742,732,732,731,741,741,732,732,741,741],"chase":[80,70,70,71,70,70,60,60,71,60,70,70,70,71,60,70,70,70,71,60]},"go:16":{"sum":[2594,2615,2595,2595,2605,2605,2595,2605,2615,2635,2615,2595,2595,2614,2595,2615,2605,2625,2635,2645],"chase":[210,200,221,200,220,221,220,190,221,210,200,201,200,200,201,200,200,201,200,210]},"qbe:1024":{"sum":[14087,14077,14077,14086,14066,14086,14067,14077,14077,14077,14087,14067,14087,14077,14077,14087,14077,14087,14076,14076],"chase":[4709,4699,4699,4709,4708,4709,4709,4699,4709,4699,4709,4709,4699,4709,4699,4699,4699,4699,4709,4699]},"llvm-ir:1024":{"sum":[6502,6502,6493,6502,6493,6502,6503,6492,6503,6492,6492,6503,6492,6503,10670,6612,6622,6632,6612,6622],"chase":[4679,4669,4669,4669,4669,4669,4669,4669,4669,4668,4679,4669,4659,4669,4769,4759,4759,4759,4769,4769]},"clang-c:1024":{"sum":[6503,6493,6502,6493,6492,6502,6492,6502,6503,6492,6493,10640,6622,6632,6622,6632,6622,6612,6622,6622],"chase":[4669,4669,4669,4669,4678,4669,4669,4669,4669,4669,4669,4759,4770,4770,4759,4769,4769,4759,4769,4769]},"gcc-c:1024":{"sum":[7555,7545,7564,7545,7534,7544,7534,7544,7545,11732,7724,7734,7735,7725,7244,7244,7243,7244,7244,7244],"chase":[4669,4679,4669,4669,4668,4659,4668,4669,4659,4789,4790,4779,4779,4779,4789,4779,4790,4789,4779,4789]},"clang-cpp:1024":{"sum":[6462,6482,6473,6472,6473,6462,6462,6472,6462,6473,6472,6463,6472,6463,6462,6473,6462,6463,6472,6462],"chase":[4649,4659,4649,4649,4649,4649,4648,4649,4639,4649,4639,4649,4649,4639,4649,4649,4649,4649,4659,4648]},"rust-llvm:1024":{"sum":[6462,6472,6472,6453,6462,6473,6462,6473,6642,6622,6632,6612,6622,6622,6612,6632,6633,6623,6633,6623],"chase":[4649,4659,4648,4639,4639,4649,4639,8716,4759,4759,4759,4749,4759,4759,4759,4759,4749,4759,4759,4759]},"zig-llvm:1024":{"sum":[12043,12043,12053,12043,12033,16362,12323,12343,12323,12344,12324,12344,12324,12344,12333,12333,12333,12334,12334,12324],"chase":[4649,4659,4649,4659,4649,4769,4769,4769,4769,4769,4769,4769,4769,4759,4769,4769,4769,4759,4779,11502]},"zig-native:1024":{"sum":[45356,40186,40186,40186,40207,40207,40217,40207,40197,40197,40197,40197,40187,40207,40187,40206,41158,48492,44164,44094],"chase":[5189,5260,5270,5290,5260,5280,5270,5260,5260,5280,5270,5290,5260,5240,5250,5240,5290,5210,5169,5190]},"go:1024":{"sum":[16201,16181,16221,16221,16181,16211,16241,16180,16311,16361,16180,16341,16371,16171,16201,16201,18586,16311,24807,16271],"chase":[4889,4900,4879,4890,4889,4900,4909,4889,4960,4889,4889,4969,4909,4899,4879,4879,4899,5270,4899,4920]},"qbe:65536":{"sum":[971591,1054690,972945,964688,972744,982372,967154,964519,919844,949931,965751,902691,965641,960570,964258,957965,969678,967274,959769,964458],"chase":[790327,786369,786139,789725,796929,794094,781339,789775,797961,795286,789725,797190,795185,790216,802289,790056,793723,793533,789775,792521]},"llvm-ir:65536":{"sum":[428297,408328,408339,408318,414129,411575,428487,429709,423497,411564,408318,408328,408309,419530,427595,408318,425461,426593,435690,426663],"chase":[789986,789324,789565,800186,785147,789114,789344,795626,784976,788383,789265,788412,790727,785056,789475,793192,792952,789175,785117,789686]},"clang-c:65536":{"sum":[413979,432304,429749,408479,426282,425381,432444,424790,424569,408358,421844,430460,408359,426363,428336,423217,429539,429849,427706,412166],"chase":[788193,786369,792922,803070,793663,785137,793032,785036,792731,789114,785457,789635,791759,791449,789856,786339,789354,788102,790607,784725]},"gcc-c:65536":{"sum":[478402,475176,470276,472701,476168,475397,472460,472310,476618,472711,472771,443676,462792,472511,472781,475507,465427,472540,472570,480546],"chase":[789185,797340,794815,799183,789094,792651,792941,796418,793412,793463,793673,792340,793472,793393,793192,790657,794254,793323,793402,793172]},"clang-cpp:65536":{"sum":[410312,412397,416964,428717,410322,408379,412366,411634,426132,428837,428026,411925,411795,408709,408328,408339,428287,422775,408328,408329],"chase":[794785,785126,793782,793633,796258,797279,785227,788523,788894,789384,789725,785207,789405,789605,789625,789655,785146,788643,788884,789475]},"rust-llvm:65536":{"sum":[432314,437303,411835,423778,415072,412135,436832,411875,416373,437474,436893,436332,415351,433637,437103,413849,435681,415362,425451,436652],"chase":[789244,788884,795837,785267,789465,789135,789845,789554,785648,791148,789866,789905,785177,788533,789325,792240,792380,785317,789645,793232]},"zig-llvm:65536":{"sum":[814903,814793,816086,815024,811847,823560,815013,811376,810776,808020,810785,810655,815365,818359,811506,826325,810996,813120,825122,804062],"chase":[783804,779546,787661,784054,787141,790446,779696,780999,780127,781520,781489,779906,787220,776379,781379,781279,775658,783854,776520,777962]},"zig-native:65536":{"sum":[2640352,2643539,2572504,2723421,2702331,2713793,2594425,2646975,2625504,2642236,2625705,2712641,2709534,2706519,2715376,2715286,2645542,2640603,2586309,2665390],"chase":[821396,824762,820053,820444,819252,832827,822588,837225,825915,818741,830563,828499,824091,827688,821155,836965,823550,818851,828048,828419]},"go:65536":{"sum":[915254,966913,943418,913652,944210,912068,910856,914843,958456,981070,913621,912750,922548,915325,917909,926476,923771,910466,928911,971863],"chase":[794133,801858,785738,797560,782482,785827,786619,782060,791829,795265,786279,789605,789926,790938,791007,805365,786389,796268,792961,789775]},"go:1048576":{"sum":[15660961,15465548,15525142,15630984,15509031,15548136,15506397,15755841,15673966,15641092,15476619,15783164,15704314,15469666,15729602,15427446,15268824,15534099,15727398,15485677],"chase":[12111225,12092830,12100154,12158315,12095474,12100785,12025351,12138067,12064746,12095384,12044137,12060458,12096798,12066390,12093131,12197359,12087510,12040279,12104352,12095354]},"zig-native:1048576":{"sum":[42701973,43010060,43307958,42749804,43002255,42874181,42835257,42860455,42724746,42707644,42785512,42595019,42770935,42774561,42580842,42646126,42592885,42521238,42702884,42794259],"chase":[11249452,11302664,11335135,11277866,11296322,11350164,11329736,11341828,11328112,11300951,11352188,11318484,11312713,11367136,11288096,11297544,11336589,11322180,11303285,11325227]},"zig-llvm:1048576":{"sum":[13159703,13264212,13286325,13269312,13232993,13253132,13282788,13307354,13280103,13300171,13080913,13132131,13270615,13277828,13255626,13344585,13259944,13291755,13315260,13290463],"chase":[11080199,11111840,11071493,11056725,11049932,11075551,11057075,11024052,11099336,11066123,10962766,11085510,11041005,11072626,11114004,11127499,11057016,11050353,11065342,11118593]},"rust-llvm:1048576":{"sum":[7504774,7530574,7550702,7356031,7375307,7437617,7480248,7480448,7509274,7742007,7525144,7554359,7480749,7523660,7561142,7514703,7392952,7520956,7423940,7417979],"chase":[11127270,11079849,11153980,11132399,11201080,11108564,11100619,11154090,11006950,11115467,11090209,11161575,11127099,11094086,11095209,11122570,11106680,11090580,11105077,11111770]},"clang-cpp:1048576":{"sum":[7482312,7542055,7706469,7433860,8546099,8463432,8376005,8567010,8326902,7928923,7160980,7395456,7359828,7327798,7403120,7418189,7350351,7358015,7433018,7355560],"chase":[11102512,11059921,11129744,11104436,11026257,11054952,11075000,11068648,11093976,11224955,11248941,11227440,11301101,11184408,11159600,11177054,11159100,11175161,11163990,11180621]},"gcc-c:1048576":{"sum":[7728621,7497370,7432507,7701740,7518651,7551764,7609364,7500647,7594165,7607049,7596530,7556032,7640673,7678325,7493443,7530113,7633470,7431845,7545722,7702762],"chase":[11259551,11235014,11235005,11255444,11272055,11261545,11244343,11227200,11249793,11257848,11253240,11236257,11252899,11279720,11262156,11243932,11246437,11244883,11232039,11268599]},"clang-c:1048576":{"sum":[7246081,7276048,7262833,7445322,7284725,7444600,7384345,7881733,7231644,7128567,7243657,7279556,7437947,7258666,7214291,7370960,7385016,7176900,7369046,7545572],"chase":[11217271,11196772,11222180,11195489,11206110,11185480,11198154,11282475,11265563,11242198,11202252,11236067,11205569,11237670,11165743,11184599,11152988,11133121,11188366,11177184]},"llvm-ir:1048576":{"sum":[7344620,7450211,7370158,7424792,7323849,7329671,7341734,7452946,7327457,7383093,7329380,7303100,7470048,7588344,7398442,7519542,7460761,7409483,7369597,7500487],"chase":[11219495,11203284,11195620,11174850,11162186,11145945,11160293,11151796,11169390,11144923,11158529,11116148,11159260,11113212,11142748,11128732,11136577,11128562,11105087,11124815]},"qbe:1048576":{"sum":[15388522,15254496,15158724,15102476,15073772,15228496,15567713,15955462,16572558,15150197,14998619,15157682,15154826,15139427,15112495,15022534,15367482,15368724,15301586,15063512],"chase":[11014534,11065702,11053479,11085740,11124174,11057726,11199637,11114304,11056674,11102232,11107542,11082704,11104526,11073026,11067807,11035915,11124304,11064801,11177616,11094387]}},"compile_ms":{"qbe":[1.765665,1.727031,1.711633,1.716702,1.707876,1.705871,1.713667,1.709939,1.698758,1.7153,1.656298,1.687656,1.692967,1.705671,1.790193,1.697124,1.72076,1.759273,1.695142,1.696924],"llvm-ir":[25.769318,24.387664,23.634128,23.68752,24.360743,23.667762,23.675978,23.358894,23.51423,23.249074,23.321332,23.38848,24.098233,24.413684,23.894504,23.590515,23.588071,23.475165,23.325901,23.577881],"clang-c":[26.096993,25.990329,26.209297,28.36202,26.781778,27.199845,26.839578,26.839899,26.624689,26.344174,26.076274,26.222362,25.998104,26.097173,26.757051,26.761209,26.370033,26.41599,26.553223,26.432703],"gcc-c":[21.172386,20.950884,21.114535,20.759048,20.835353,20.620184,20.669808,20.952016,20.887493,21.003745,21.029334,20.5398,20.580878,20.470829,20.636355,20.630583,20.7919,21.741501,20.544749,20.582942],"clang-cpp":[28.869898,27.844794,27.943272,27.558488,27.676313,27.490759,27.826158,27.27629,26.98225,26.970287,26.575465,26.385753,26.600863,26.372728,26.235407,26.565536,26.867481,26.991898,26.855929,26.601805],"rust-llvm":[32.283234,30.664398,29.829948,29.759824,29.766617,29.518504,29.764974,29.421149,29.147807,29.913387,32.356424,27.008981,29.050011,32.074426,26.556689,29.129994,26.427062,30.134478,30.17185,31.019475],"zig-llvm":[428.47884,421.77378,420.644066,420.891378,421.210836,423.280461,422.141371,421.077803,421.336697,422.35134,422.07809,424.669189,422.022453,422.024097,421.560713,421.754332,424.186878,422.1569,422.519991,422.305433],"zig-native":[443.251427,407.416594,440.295823,423.32181,407.809963,417.149035,422.791148,410.92503,411.958771,409.965401,439.298031,410.539576,407.293459,418.988109,440.301314,421.654061,420.917988,417.095853,433.593037,424.925686]},"commands":{"qbe":["/nix/store/q5izjv3mxdpwmhsqjjarabmq2ql9c0zs-qbe-1.2/bin/qbe","-t","amd64_sysv","-o","/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/qbe.s","/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.ssa"],"llvm-ir":["/nix/store/874j5xydsj6nr6i1zdrjvhln5gmxvvrr-clang-wrapper-21.1.8/bin/clang","-x","ir","-O3","-march=x86-64","-mtune=generic","-S","/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.ll","-o","/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/llvm-ir.s"],"clang-c":["/nix/store/874j5xydsj6nr6i1zdrjvhln5gmxvvrr-clang-wrapper-21.1.8/bin/clang","-O3","-march=x86-64","-mtune=generic","-c","/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.c","-o","/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/clang-c.o"],"gcc-c":["/nix/store/788mx070y81zjlg5ipcl0cra3afviw9k-gcc-wrapper-15.2.0/bin/gcc","-O3","-march=x86-64","-mtune=generic","-c","/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.c","-o","/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/gcc-c.o"],"clang-cpp":["/nix/store/874j5xydsj6nr6i1zdrjvhln5gmxvvrr-clang-wrapper-21.1.8/bin/clang++","-x","c++","-O3","-march=x86-64","-mtune=generic","-c","/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.c","-o","/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/clang-cpp.o"],"rust-llvm":["/nix/store/wnhmqix7bippbbzasj29qiyb422g9asg-rustc-wrapper-1.95.0/bin/rustc","--edition=2024","--crate-type=lib","--emit=obj","-Copt-level=3","-Cpanic=abort","-Ctarget-cpu=x86-64","/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.rs","-o","/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/rust-llvm.o"],"zig-llvm":["/nix/store/n1hqsn4a4yfmipzyk2byavvx4ccx6ccr-zig-0.16.0/bin/zig","build-obj","-fllvm","-O","ReleaseFast","-target","x86_64-linux-gnu","-mcpu","baseline","-femit-bin=/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/zig-llvm.o","/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.zig"],"zig-native":["/nix/store/n1hqsn4a4yfmipzyk2byavvx4ccx6ccr-zig-0.16.0/bin/zig","build-obj","-fno-llvm","-O","ReleaseFast","-target","x86_64-linux-gnu","-mcpu","baseline","-femit-bin=/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/zig-native.o","/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.zig"],"go":["env","CC=/nix/store/874j5xydsj6nr6i1zdrjvhln5gmxvvrr-clang-wrapper-21.1.8/bin/clang","PATH=/nix/store/874j5xydsj6nr6i1zdrjvhln5gmxvvrr-clang-wrapper-21.1.8/bin:/nix/store/788mx070y81zjlg5ipcl0cra3afviw9k-gcc-wrapper-15.2.0/bin:/nix/store/b1p9a901ds4wck8n1pd76vxnjm34sfap-gcc-15.2.0-man/bin:/nix/store/x720xa1b5sj14cg77i5i0nj4d5qrpbpg-rustc-1.95.0-man/bin:/nix/store/wnhmqix7bippbbzasj29qiyb422g9asg-rustc-wrapper-1.95.0/bin:/nix/store/33fw5m31lfcnk4ff2f0df7j2bxnh8lgk-go-1.26.3/bin:/nix/store/n1hqsn4a4yfmipzyk2byavvx4ccx6ccr-zig-0.16.0/bin:/nix/store/q5izjv3mxdpwmhsqjjarabmq2ql9c0zs-qbe-1.2/bin:/nix/store/5mkrabav193miwsc3ljm2dcfhg4l8zsm-binutils-2.46-man/bin:/nix/store/mbyy19mdwnfvfwmdi0gqgggx0njvpl1w-binutils-wrapper-2.46/bin:/nix/store/s2946bl9ciwzhafd66jhansrmxq9xhqm-binutils-2.46/bin:/nix/store/bsh7n2nx8ndmm1mmww6v2h4851nalj13-glibc-2.42-61-bin/bin:/run/wrappers/bin:/home/nate/.local/share/flatpak/exports/bin:/var/lib/flatpak/exports/bin:/home/nate/.nix-profile/bin:/home/nate/.local/state/nix/profile/bin:/home/nate/.local/state/nix/profile/bin:/etc/profiles/per-user/nate/bin:/nix/var/nix/profiles/default/bin:/run/current-system/sw/bin","GOCACHE=/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/go-cache","TMPDIR=/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10","GOAMD64=v1","/nix/store/33fw5m31lfcnk4ff2f0df7j2bxnh8lgk-go-1.26.3/bin/go","build","-buildmode=c-archive","-o","/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/go.a","/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/kernels.go"]}}
```

#### Linked sizes

```json
[
  {
    "variant": "qbe",
    "executable_bytes": 16168,
    "text_bytes": 1522
  },
  {
    "variant": "llvm-ir",
    "executable_bytes": 16208,
    "text_bytes": 1677
  },
  {
    "variant": "clang-c",
    "executable_bytes": 16208,
    "text_bytes": 1702
  },
  {
    "variant": "gcc-c",
    "executable_bytes": 16208,
    "text_bytes": 1612
  },
  {
    "variant": "clang-cpp",
    "executable_bytes": 16208,
    "text_bytes": 1702
  },
  {
    "variant": "rust-llvm",
    "executable_bytes": 16304,
    "text_bytes": 1677
  },
  {
    "variant": "zig-llvm",
    "executable_bytes": 18832,
    "text_bytes": 1695
  },
  {
    "variant": "zig-native",
    "executable_bytes": 22928,
    "text_bytes": 1763
  },
  {
    "variant": "go",
    "executable_bytes": 2699768,
    "text_bytes": 514654
  }
]
```

#### Disassembly evidence

##### QBE sum

```text

/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/qbe:     file format elf64-x86-64


Disassembly of section .init:

Disassembly of section .plt:

Disassembly of section .plt.got:

Disassembly of section .text:

0000000000001640 <sum_u64>:
    1640:	55                   	push   rbp
    1641:	48 89 e5             	mov    rbp,rsp
    1644:	b9 00 00 00 00       	mov    ecx,0x0
    1649:	b8 00 00 00 00       	mov    eax,0x0
    164e:	48 39 f0             	cmp    rax,rsi
    1651:	73 0d                	jae    1660 <sum_u64+0x20>
    1653:	48 8b 14 c7          	mov    rdx,QWORD PTR [rdi+rax*8]
    1657:	48 01 d1             	add    rcx,rdx
    165a:	48 83 c0 01          	add    rax,0x1
    165e:	eb ee                	jmp    164e <sum_u64+0xe>
    1660:	48 89 c8             	mov    rax,rcx
    1663:	c9                   	leave
    1664:	c3                   	ret

Disassembly of section .fini:

```

##### LLVM IR sum

```text

/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/llvm-ir:     file format elf64-x86-64


Disassembly of section .init:

Disassembly of section .plt:

Disassembly of section .plt.got:

Disassembly of section .text:

0000000000001640 <sum_u64>:
    1640:	48 85 f6             	test   rsi,rsi
    1643:	74 0c                	je     1651 <sum_u64+0x11>
    1645:	48 83 fe 04          	cmp    rsi,0x4
    1649:	73 09                	jae    1654 <sum_u64+0x14>
    164b:	31 c0                	xor    eax,eax
    164d:	31 c9                	xor    ecx,ecx
    164f:	eb 4f                	jmp    16a0 <sum_u64+0x60>
    1651:	31 c0                	xor    eax,eax
    1653:	c3                   	ret
    1654:	48 89 f1             	mov    rcx,rsi
    1657:	48 83 e1 fc          	and    rcx,0xfffffffffffffffc
    165b:	66 0f ef c0          	pxor   xmm0,xmm0
    165f:	31 c0                	xor    eax,eax
    1661:	66 0f ef c9          	pxor   xmm1,xmm1
    1665:	66 2e 0f 1f 84 00 00 	cs nop WORD PTR [rax+rax*1+0x0]
    166c:	00 00 00 
    166f:	90                   	nop
    1670:	f3 0f 6f 14 c7       	movdqu xmm2,XMMWORD PTR [rdi+rax*8]
    1675:	66 0f d4 c2          	paddq  xmm0,xmm2
    1679:	f3 0f 6f 54 c7 10    	movdqu xmm2,XMMWORD PTR [rdi+rax*8+0x10]
    167f:	66 0f d4 ca          	paddq  xmm1,xmm2
    1683:	48 83 c0 04          	add    rax,0x4
    1687:	48 39 c1             	cmp    rcx,rax
    168a:	75 e4                	jne    1670 <sum_u64+0x30>
    168c:	66 0f d4 c8          	paddq  xmm1,xmm0
    1690:	66 0f 70 c1 ee       	pshufd xmm0,xmm1,0xee
    1695:	66 0f d4 c1          	paddq  xmm0,xmm1
    1699:	66 48 0f 7e c0       	movq   rax,xmm0
    169e:	eb 07                	jmp    16a7 <sum_u64+0x67>
    16a0:	48 03 04 cf          	add    rax,QWORD PTR [rdi+rcx*8]
    16a4:	48 ff c1             	inc    rcx
    16a7:	48 39 ce             	cmp    rsi,rcx
    16aa:	75 f4                	jne    16a0 <sum_u64+0x60>
    16ac:	c3                   	ret

Disassembly of section .fini:

```

##### Zig LLVM sum

```text

/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/zig-llvm:     file format elf64-x86-64


Disassembly of section .init:

Disassembly of section .plt:

Disassembly of section .plt.got:

Disassembly of section .text:

00000000000016c0 <sum_u64>:
    16c0:	55                   	push   rbp
    16c1:	48 89 e5             	mov    rbp,rsp
    16c4:	48 85 f6             	test   rsi,rsi
    16c7:	74 11                	je     16da <sum_u64+0x1a>
    16c9:	89 f1                	mov    ecx,esi
    16cb:	83 e1 07             	and    ecx,0x7
    16ce:	48 83 fe 08          	cmp    rsi,0x8
    16d2:	73 0a                	jae    16de <sum_u64+0x1e>
    16d4:	31 d2                	xor    edx,edx
    16d6:	31 c0                	xor    eax,eax
    16d8:	eb 46                	jmp    1720 <sum_u64+0x60>
    16da:	31 c0                	xor    eax,eax
    16dc:	5d                   	pop    rbp
    16dd:	c3                   	ret
    16de:	48 83 e6 f8          	and    rsi,0xfffffffffffffff8
    16e2:	31 d2                	xor    edx,edx
    16e4:	31 c0                	xor    eax,eax
    16e6:	66 2e 0f 1f 84 00 00 	cs nop WORD PTR [rax+rax*1+0x0]
    16ed:	00 00 00 
    16f0:	48 03 04 d7          	add    rax,QWORD PTR [rdi+rdx*8]
    16f4:	48 03 44 d7 08       	add    rax,QWORD PTR [rdi+rdx*8+0x8]
    16f9:	48 03 44 d7 10       	add    rax,QWORD PTR [rdi+rdx*8+0x10]
    16fe:	48 03 44 d7 18       	add    rax,QWORD PTR [rdi+rdx*8+0x18]
    1703:	48 03 44 d7 20       	add    rax,QWORD PTR [rdi+rdx*8+0x20]
    1708:	48 03 44 d7 28       	add    rax,QWORD PTR [rdi+rdx*8+0x28]
    170d:	48 03 44 d7 30       	add    rax,QWORD PTR [rdi+rdx*8+0x30]
    1712:	48 03 44 d7 38       	add    rax,QWORD PTR [rdi+rdx*8+0x38]
    1717:	48 83 c2 08          	add    rdx,0x8
    171b:	48 39 d6             	cmp    rsi,rdx
    171e:	75 d0                	jne    16f0 <sum_u64+0x30>
    1720:	48 85 c9             	test   rcx,rcx
    1723:	74 18                	je     173d <sum_u64+0x7d>
    1725:	48 8d 14 d7          	lea    rdx,[rdi+rdx*8]
    1729:	31 f6                	xor    esi,esi
    172b:	0f 1f 44 00 00       	nop    DWORD PTR [rax+rax*1+0x0]
    1730:	48 03 04 f2          	add    rax,QWORD PTR [rdx+rsi*8]
    1734:	48 83 c6 01          	add    rsi,0x1
    1738:	48 39 f1             	cmp    rcx,rsi
    173b:	75 f3                	jne    1730 <sum_u64+0x70>
    173d:	5d                   	pop    rbp
    173e:	c3                   	ret

Disassembly of section .fini:

```

##### Zig native sum

```text

/home/nate/.cache/jet-test-scratch/mine-casey-performance-2026-09-10/backend-probe/zig-native:     file format elf64-x86-64


Disassembly of section .init:

Disassembly of section .plt:

Disassembly of section .plt.got:

Disassembly of section .text:

0000000000001700 <sum_u64>:
    1700:	55                   	push   rbp
    1701:	48 89 e5             	mov    rbp,rsp
    1704:	53                   	push   rbx
    1705:	48 83 ec 30          	sub    rsp,0x30
    1709:	48 89 3c 24          	mov    QWORD PTR [rsp],rdi
    170d:	48 89 74 24 08       	mov    QWORD PTR [rsp+0x8],rsi
    1712:	48 89 7c 24 10       	mov    QWORD PTR [rsp+0x10],rdi
    1717:	48 c7 44 24 18 00 00 	mov    QWORD PTR [rsp+0x18],0x0
    171e:	00 00 
    1720:	48 c7 44 24 20 00 00 	mov    QWORD PTR [rsp+0x20],0x0
    1727:	00 00 
    1729:	48 8b 44 24 20       	mov    rax,QWORD PTR [rsp+0x20]
    172e:	48 89 c2             	mov    rdx,rax
    1731:	48 89 f3             	mov    rbx,rsi
    1734:	48 39 da             	cmp    rdx,rbx
    1737:	0f 83 27 00 00 00    	jae    1764 <sum_u64+0x64>
    173d:	48 89 44 24 28       	mov    QWORD PTR [rsp+0x28],rax
    1742:	48 8b 54 24 18       	mov    rdx,QWORD PTR [rsp+0x18]
    1747:	48 8b 5c 24 10       	mov    rbx,QWORD PTR [rsp+0x10]
    174c:	48 89 c1             	mov    rcx,rax
    174f:	48 8d 1c cb          	lea    rbx,[rbx+rcx*8]
    1753:	48 8b 0b             	mov    rcx,QWORD PTR [rbx]
    1756:	48 01 ca             	add    rdx,rcx
    1759:	48 89 54 24 18       	mov    QWORD PTR [rsp+0x18],rdx
    175e:	90                   	nop
    175f:	e9 05 00 00 00       	jmp    1769 <sum_u64+0x69>
    1764:	e9 0e 00 00 00       	jmp    1777 <sum_u64+0x77>
    1769:	48 83 c0 01          	add    rax,0x1
    176d:	48 89 44 24 20       	mov    QWORD PTR [rsp+0x20],rax
    1772:	e9 b2 ff ff ff       	jmp    1729 <sum_u64+0x29>
    1777:	48 8b 44 24 18       	mov    rax,QWORD PTR [rsp+0x18]
    177c:	48 8d 65 f8          	lea    rsp,[rbp-0x8]
    1780:	5b                   	pop    rbx
    1781:	5d                   	pop    rbp
    1782:	c3                   	ret

Disassembly of section .fini:

```

</details>

## Normalized claim ledgers and cross-resource matrix

Seventy new rows were parsed and enum-checked. Topics are unique within each resource ledger. The matrix also includes 104 claim rows recovered from the prior retained report, grouped into 141 topics. Repetition means independent recorded provenance, not factual confirmation. Inferences share `inference:Main`; repeated citations do not manufacture independent support.

<details>
<summary>video: 34 normalized claims</summary>

```json
[
  {
    "topic": "premise",
    "claim": "Host frames much software as tens-to-100× slower than necessary; Casey argues that assembly and performance reasoning are less mysterious than programmers assume, and that knowing the machine changes what can be optimized.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 00:00–03:14, 23:06–30:33",
    "confidence": "medium",
    "stance": "supports",
    "correction": "The “tens-to-100×” statement is host framing, not a measured result in this episode; Casey’s “20–30 instructions” is a pedagogical subset, not all x64.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C01"
  },
  {
    "topic": "incentives",
    "claim": "Casey says enterprise purchasers often optimize compliance, legal risk, and cost rather than end-user wait time; monopoly/network effects can also make a slower product win.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 16:02–23:06",
    "confidence": "medium",
    "stance": "supports",
    "correction": "This explains incentives, not a law that enterprise or platform software is slow.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C02"
  },
  {
    "topic": "methodology",
    "claim": "Casey rejects “profile the big parts, change them, measure” as sufficient. He proposes enumerating the operation, deriving a hardware-theoretical peak, measuring the real delta, and explaining the gap.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 23:06–30:33",
    "confidence": "medium",
    "stance": "supports",
    "correction": "The episode gives no reproducible derivation, hardware counters, or new benchmark; this is Casey’s method prescription.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C03"
  },
  {
    "topic": "dependency-structure",
    "claim": "Casey says hotspot-only optimization can reach a local minimum while missing a pervasive serial chain such as request→wait→compute→request. The longest unavoidable dependency chain controls throughput/latency; batching or parallelizing known requests must be designed up front.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 23:06–30:33, 30:37–42:50",
    "confidence": "medium",
    "stance": "supports",
    "correction": "“Longest serial chain determines performance” is a useful critical-path model, not a complete model of every workload (contention, bandwidth, scheduling, and variance also matter).  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C04"
  },
  {
    "topic": "premature-optimization",
    "claim": "Casey says the slogan is not wholly false: delay a swappable implementation (for example, a hash implementation), but do not defer an architectural choice that creates an irreversible dependency chain. His linked historical talk says the phrase’s attribution and circumstances are contested.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 30:37–42:50; [R] https://www.computerenhance.com/p/theroot",
    "confidence": "medium",
    "stance": "supports",
    "correction": "Knuth’s commonly quoted context is about small efficiencies in noncritical code, not a ban on performance-aware architecture. ACM DOI: `https://dl.acm.org/doi/10.1145/356635.356640` (the DOI endpoint was unavailable during this read; the Casey historical source was available).  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C05"
  },
  {
    "topic": "assembly-literacy",
    "claim": "Casey says most programmers need to read a small hot assembly fragment, CPU diagrams, and compiler output more often than write assembly; he recommends roughly a month or two of study and uses inline assembly mainly for test control.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 23:06–30:33, 42:54–55:49, 84:30–93:56",
    "confidence": "medium",
    "stance": "supports",
    "correction": "The recommendation is normative and personal; it does not establish that every Jet user or maintainer must hand-write assembly.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "owner-gate",
    "owner": "#3016",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C06"
  },
  {
    "topic": "hardware-model",
    "claim": "Casey groups performance reasoning into data movement (load/store/cache and layout), instruction flow (branches, mispredicts, I-cache), and execution-unit scheduling/throughput (for example integer add, float multiply, division). He notes assembly maps to micro-ops and hardware diagrams only approximately.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 42:54–55:49",
    "confidence": "medium",
    "stance": "supports",
    "correction": "M-series, Zen, and Core examples are conceptual; no target-specific measurement appears in the episode.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C07"
  },
  {
    "topic": "interpreter-overhead",
    "claim": "Casey uses Python `A + B` versus a C one-instruction add as an intuition pump and says Python may consume roughly 100× more CPU instructions for the same expression; he recommends optimized C libraries or a Cython-like escape when needed.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 42:54–55:49, 112:18–113:56",
    "confidence": "medium",
    "stance": "supports",
    "correction": "No benchmark, Python version, operand type, implementation, or workload is supplied; the ratio is not a language-wide constant. The captioned “Syon” name is uncertain and likely refers to Cython.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C08"
  },
  {
    "topic": "rewrite-evidence",
    "claim": "Host/Casey mention Facebook/Uber-style rewrites and claim AI companies are moving Python APIs toward Rust/Go for multithreading or connection handling.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 36:00–42:50",
    "confidence": "low",
    "stance": "neutral",
    "correction": "These are second-hand conversational examples; no linked migration post, workload, baseline, or counterfactual is supplied. A rewrite can change architecture, libraries, staffing, and deployment at once; do not attribute its outcome to language syntax alone.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C09"
  },
  {
    "topic": "game-history",
    "claim": "Casey recounts older studios building renderers/tooling in-house because engines were not broadly licensable; risks included technical inability to realize the game and inability to finish quickly. He explicitly says he is stale on current live-service studios and is not the historian to ask.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 55:51–65:50",
    "confidence": "medium",
    "stance": "neutral",
    "correction": "Personal memory and hearsay (including Thief timing and named studio examples) are not a complete history.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C10"
  },
  {
    "topic": "validation-strategy",
    "claim": "Casey describes the vertical slice: the whole studio rapidly makes a hacky but playable slice to test whether the core game is fun before scaling assets and schedules.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 60:00–65:50",
    "confidence": "medium",
    "stance": "supports",
    "correction": "He presents this as historical industry evolution and admits it is not a historian’s account; no comparative success data is given.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C11"
  },
  {
    "topic": "ecosystem",
    "claim": "Host/Casey agree that Unity/Godot/Unreal lower engine risk and expand amateur/artistic access, while the flood of Steam releases makes discoverability and marketing difficult.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 65:52–76:56",
    "confidence": "medium",
    "stance": "neutral",
    "correction": "“Tens/hundreds of thousands,” “near-zero” breakout odds, and organic-discovery claims are approximate and unsourced in the episode.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C12"
  },
  {
    "topic": "product-lifecycle",
    "claim": "Casey frames Fortnite/Roblox/GTA Online/Minecraft as persistent services consuming finite entertainment time and speculates that GTA6 must replace or cannibalize GTA5’s enormous existing business.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 70:00–76:56",
    "confidence": "low",
    "stance": "neutral",
    "correction": "Casey says this is not his specialty and gives a large “grain of salt”; profitability, launch outcome, and cannibalization are not established evidence.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C13"
  },
  {
    "topic": "abstraction-performance",
    "claim": "Casey discusses a rough C++ shape-area benchmark where a virtual/polymorphic form is slower than a flat form and a table/data-oriented form is much faster; the article reports roughly 35 cycles/shape versus 24 (about 1.5×), then roughly 3–3.5 cycles/shape for a table (about 10×), with a lightly optimized AVX path around 20–25×.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 76:57–84:29; [CC] https://www.computerenhance.com/p/clean-code-horrible-performance",
    "confidence": "medium",
    "stance": "supports",
    "correction": "The article calls the measurements rough rather than hardcore and uses a favorable synthetic hot loop; ratios are not universal costs of “clean code.”  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C14"
  },
  {
    "topic": "benchmark-attribution",
    "claim": "The article’s larger ratio comes from table/data layout and later property-aware lookup; the switch/flat version is closer to about 1.5–2× in the described tests.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "linked-source",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "[CC] https://www.computerenhance.com/p/clean-code-horrible-performance",
    "locator": "[CC] https://www.computerenhance.com/p/clean-code-horrible-performance",
    "confidence": "high",
    "stance": "disputes",
    "correction": "Do **not** cite Casey’s episode as proof that a switch alone is 15× faster than a virtual hierarchy. Mechanism is pointer indirection, layout, branch predictability, and what the compiler can see; the article also notes switch is not inherently “less polymorphic.”  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C15"
  },
  {
    "topic": "dispatch-tradeoff",
    "claim": "Rust’s primary documentation says a trait object carries an instance plus a runtime method table; dynamic dispatch costs lookup and can prevent inlining, but enables heterogeneous collections and open extensibility. Generic bounds instead support homogeneous, compiler-specialized use.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "linked-source",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "[Rust] https://doc.rust-lang.org/book/ch18-02-trait-objects.html",
    "locator": "[Rust] https://doc.rust-lang.org/book/ch18-02-trait-objects.html",
    "confidence": "high",
    "stance": "neutral",
    "correction": "Casey’s anti-polymorphism argument applies most strongly to hot, representation-sensitive paths; it is not a universal condemnation of dynamic dispatch or abstraction.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C16"
  },
  {
    "topic": "testing-strategy",
    "claim": "Casey says tests are worthwhile when saved debugging/regression time exceeds authoring, maintenance, and change friction; he used a regression tester for RAD routines but rejects mandatory test counts or one default strategy.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 76:57–84:29",
    "confidence": "medium",
    "stance": "supports",
    "correction": "This is a personal engineering rule, not evidence that TDD is generally harmful or unnecessary.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3014",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C17"
  },
  {
    "topic": "code-quality",
    "claim": "Casey defines good code as straightforwardly meeting the machine’s need, split into digestible pieces with meaningful names and minimal redundancy that can be recomposed by the compiler; his “Semantic Compression” article says to make code usable before reusable and wait for at least two instances before extracting reuse.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 84:30–93:56; [SC] https://caseymuratori.com/blog_0015",
    "confidence": "medium",
    "stance": "supports",
    "correction": "“Good” remains partly taste/context; concrete-first does not mean duplication forever, and the linked article is not a universal performance proof.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C18"
  },
  {
    "topic": "engineering-practice",
    "claim": "Casey values curiosity about deeper layers, the ability to read assembly, role-appropriate specialization, and testing claims empirically and repeatably rather than accepting received wisdom.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 84:30–93:56",
    "confidence": "medium",
    "stance": "supports",
    "correction": "“A great engineer can read assembly” is Casey’s strong opinion, not a hiring or language requirement.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C19"
  },
  {
    "topic": "sponsored-evidence",
    "claim": "The episode carries Antithesis claims about deterministic hostile simulation/reproduction, Sentry/Seer claims about AI root-cause-to-PR workflows, and Turbopuffer/Linear claims about low-flat delta-sync latency.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 00:00–03:14, 30:37–42:50, 93:58–112:18",
    "confidence": "medium",
    "stance": "neutral",
    "correction": "These are sponsored or host-marketing segments. Linear’s independently linked engineering article (`https://linear.app/now/rebuilding-delta-sync-read-path`) does document immutable logs, permission-aware intersections, Postgres tail-latency problems, and inverted posting-list indexes, but it does not establish universal “constant lookup” or a guarantee for every workload.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C20"
  },
  {
    "topic": "craft-choice",
    "claim": "Casey says Molly Rocket uses no AI because he wants to program the project himself; if he wanted AI to program it, he would use Unreal. He presents this as a craft/purpose choice, not a productivity, copyright, or ethics argument.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 93:58–101:30",
    "confidence": "medium",
    "stance": "neutral",
    "correction": "This is one project’s value choice and supplies no comparative productivity or quality measurement.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C21"
  },
  {
    "topic": "AI-evidence",
    "claim": "Casey says widespread AI coding use is only months old, there is no obvious evidence yet of bug-free weekly Fortnite-scale output or 5,000 engineers becoming five, and even a 10% uplift may be hard to observe externally. Host adds second-hand reports of fatigue/burnout; Casey distinguishes voluntary use for disliked tasks from mandated use after layoffs.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 101:30–112:18",
    "confidence": "medium",
    "stance": "neutral",
    "correction": "AI-lab self-reports and host/manager anecdotes are biased, unsourced, and not a controlled productivity study; “no evidence” here means none presented in this episode.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C22"
  },
  {
    "topic": "AI-test-oracle",
    "claim": "A controlled GPT-3.5 study over 24 Java repositories found assertion classification accuracy fell on buggy code even for correct assertions (roughly 40.77–46.26% on correct code versus 31.94–37.01% on wrong code across prompts); generated assertions passed on average only about 57–60%, and “at least one valid assertion” reached roughly 89–94% while mutation scores remained modest (GPT about 19.10 versus EvoSuite 17.32 in the reported comparison). Meaningful names helped; the authors warn passing the current implementation is not proof of intended behavior and require human inspection.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "linked-source",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "[AI] https://arxiv.org/abs/2410.21136 (HTML: https://arxiv.org/html/2410.21136v1)",
    "locator": "[AI] https://arxiv.org/abs/2410.21136 (HTML: https://arxiv.org/html/2410.21136v1)",
    "confidence": "high",
    "stance": "supports",
    "correction": "This is Java/GPT-3.5-specific, prompt/configuration-specific research, not a Jet or general coding-productivity result; “valid” means passing the current program, not necessarily semantically correct.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C23"
  },
  {
    "topic": "learning-method",
    "claim": "Casey recommends learning performance and domain practice from papers, Google Scholar, and reference chains; he speculates AI may help locate papers.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "primary",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "speaker:casey-muratori",
    "locator": "[V] 112:18–113:56",
    "confidence": "medium",
    "stance": "supports",
    "correction": "The AI-search suggestion is speculation, not a demonstrated workflow in the episode.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C24"
  },
  {
    "topic": "micro-syntax",
    "claim": "The transcript favors ordinary source syntax plus the ability to inspect a small assembly fragment; it does not ask ordinary developers to write assembly.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "inference",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "inference:Main",
    "locator": "derived from [V] 23:06–30:33, 42:54–55:49",
    "confidence": "medium",
    "stance": "supports",
    "correction": "No Jet syntax change is evidenced or proposed by the video.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "owner-gate",
    "owner": "#3016",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C25"
  },
  {
    "topic": "micro-ergonomics",
    "claim": "Casey’s month-or-two assembly curriculum and “digestible pieces/meaningful names” imply a two-speed experience: approachable defaults with an expert path.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "inference",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "inference:Main",
    "locator": "derived from [V] 42:54–55:49, 84:30–93:56",
    "confidence": "medium",
    "stance": "supports",
    "correction": "Learning-time estimates and ideal readability are personal prescriptions.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3016",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C26"
  },
  {
    "topic": "micro-surfaces",
    "claim": "Relevant surfaces are source code, lowered representation, generated machine code, and timing evidence; Casey’s claim is that the gap must be explainable.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "inference",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "inference:Main",
    "locator": "derived from [V] 23:06–30:33, 42:54–55:49",
    "confidence": "medium",
    "stance": "supports",
    "correction": "The video does not show or specify a Jet inspector.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "owner-gate",
    "owner": "#3016",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C27"
  },
  {
    "topic": "micro-apis-types-methods",
    "claim": "A semantic API should permit swappable implementations when the contract remains stable, while dispatch/type choices should reflect whether the workload needs closed specialization or open heterogeneous extensibility.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "inference",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "inference:Main",
    "locator": "derived from [V] 30:37–42:50, 76:57–93:56; [Rust] https://doc.rust-lang.org/book/ch18-02-trait-objects.html; [Rust] https://doc.rust-lang.org/book/ch10-01-syntax.html",
    "confidence": "medium",
    "stance": "supports",
    "correction": "This is a design implication, not evidence that Jet’s current API/type model has a gap.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "owner-gate",
    "owner": "#3018",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C28"
  },
  {
    "topic": "micro-defaults",
    "claim": "Casey’s architecture-first message supports a fast, safe default selected by the compiler/runtime, with expert overrides only when a measured workload justifies them.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "inference",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "inference:Main",
    "locator": "derived from [V] 23:06–30:33, 42:54–55:49",
    "confidence": "medium",
    "stance": "supports",
    "correction": "No default policy or benchmark for Jet is established by the episode.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C29"
  },
  {
    "topic": "micro-naming",
    "claim": "Casey values meaningful names; the oracle study directly found noisy names reduced LLM assertion performance by as much as about 16.10 points in one comparison.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "inference",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "inference:Main",
    "locator": "derived from [V] 84:30–93:56; [AI] https://arxiv.org/abs/2410.21136",
    "confidence": "high",
    "stance": "supports",
    "correction": "The study is Java/GPT-3.5-specific, and a name effect is not proof that every naming convention improves Jet agents equally.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C30"
  },
  {
    "topic": "micro-diagnostics",
    "claim": "If generated checks can pass an incorrect implementation, diagnostics must state the violated expectation and provide enough context for human/agent repair rather than only reporting pass/fail.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "inference",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "inference:Main",
    "locator": "derived from [V] 93:58–112:18; [AI] https://arxiv.org/abs/2410.21136",
    "confidence": "high",
    "stance": "supports",
    "correction": "The episode itself does not define diagnostic text or Jet error behavior; this follows from the linked oracle evidence.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3016",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C31"
  },
  {
    "topic": "micro-ux-dx",
    "claim": "The host calls 300 ms an eternity; Linear’s article shows a real local-first workload where tail latency and permission-aware intersection dominate the design.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "inference",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "inference:Main",
    "locator": "derived from [V] 16:02–23:06, 30:37–42:50; [L] https://linear.app/now/rebuilding-delta-sync-read-path",
    "confidence": "medium",
    "stance": "supports",
    "correction": "A single 300-ms opinion and Linear’s product-specific data are not universal latency thresholds.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C32"
  },
  {
    "topic": "micro-tooling-cli",
    "claim": "Casey’s empirical workflow requires repeatable measurement, hardware reasoning, and assembly inspection; “profile once” is insufficient.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "inference",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "inference:Main",
    "locator": "derived from [V] 23:06–30:33, 84:30–93:56",
    "confidence": "medium",
    "stance": "supports",
    "correction": "No particular profiler, CLI, counter set, or Jet command is named.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C33"
  },
  {
    "topic": "micro-ceremony-control",
    "claim": "Casey rejects mandatory TDD, mandatory assembly, and dogmatic clean-code rules; his craft choice also rejects mandatory AI. He still favors expert control when a real need is demonstrated.  ",
    "source_id": "8xBJPa_480Q",
    "source_kind": "inference",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "inference:Main",
    "locator": "derived from [V] 42:54–55:49, 76:57–93:56, 93:58–112:18",
    "confidence": "medium",
    "stance": "supports",
    "correction": "This does not imply “no tests,” “no abstraction,” or “no AI”; it argues for context-sensitive opt-in rather than ritual.  ",
    "jet_evidence": "Source/CmdInspect.rs; crates/jet-foundation/src/MIR.rs; crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs; prior retained mine report",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Preserve the qualified claim in the named evidence/card; measure the actual Jet workload before adoption.",
    "source_claim_id": "C34"
  }
]
```

</details>

<details>
<summary>audience: 17 normalized claims</summary>

```json
[
  {
    "topic": "dependency-structure",
    "claim": "Network serial dependency chains are a plausible common pain.",
    "source_id": "8xBJPa_480Q-audience",
    "source_kind": "audience",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "youtube-comment:UgxHhgLAhH2RzSUcL8V4AaABAg",
    "locator": "UgxHhgLAhH2RzSUcL8V4AaABAg, ts=1788566400",
    "confidence": "medium",
    "stance": "supports",
    "correction": "`source_kind=audience`; `confidence=medium`; `stance=supports`; `classification=pain/alternate cause`; `locator=UgxHhgLAhH2RzSUcL8V4AaABAg, ts=1788566400`. The commenter says single-machine discussion misses serial chains with network requests and guesses these dominate day-to-day web-backend failures. This is a plausible reprioritization, not a prevalence measurement; no comment benchmark or primary epidemiology was supplied. ",
    "jet_evidence": "#3011 complete workload/assurance evidence; #3016 exact code; #3017 asm repair",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Use as a qualified evidence input on the named card; no popularity or universal speed claim."
  },
  {
    "topic": "dependency-structure-nplus1",
    "claim": "The N+1 database pattern is a concrete hidden-serial-work example.",
    "source_id": "8xBJPa_480Q-audience",
    "source_kind": "audience",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "youtube-comment:Ugz7CnPFNsAul6r-Sip4AaABAg",
    "locator": "Ugz7CnPFNsAul6r-Sip4AaABAg, ts=1787875200; reply Ugz7CnPFNsAul6r-Sip4AaABAg.Aa0UM0cib1-Aa57yG1dOVV, ts=1788048000",
    "confidence": "medium",
    "stance": "supports",
    "correction": "`source_kind=audience`; `confidence=medium`; `stance=supports`; `classification=pain/workaround`; `locator=Ugz7CnPFNsAul6r-Sip4AaABAg, ts=1787875200; reply Ugz7CnPFNsAul6r-Sip4AaABAg.Aa0UM0cib1-Aa57yG1dOVV, ts=1788048000`. The root describes “get all IDs, fetch each ID, process” and estimates 10–100x slowdown; the reply reports tracing one endpoint with 118 SQL queries and suspects DDD/repository abstractions. The 118-query report is an anecdote, and the abstraction cause is conjecture. PostgreSQL’s `EXPLAIN` documentation supports inspecting the actual plan and costs, not the exact multiplier. ",
    "jet_evidence": "#3011 complete workload/assurance evidence; #3016 exact code; #3017 asm repair",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Use as a qualified evidence input on the named card; no popularity or universal speed claim."
  },
  {
    "topic": "absolute-database-latency",
    "claim": "“DB calls should always be single-digit milliseconds” is an overclaim.",
    "source_id": "8xBJPa_480Q-audience",
    "source_kind": "audience",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "youtube-comment:UgzFmYYlHJgtND9FJF54AaABAg.Aa17KdPEKs4Aa3XBjP2KiT",
    "locator": "UgzFmYYlHJgtND9FJF54AaABAg.Aa17KdPEKs4Aa3XBjP2KiT, ts=1787961600",
    "confidence": "medium",
    "stance": "disputes",
    "correction": "`source_kind=audience` with `linked-source` correction; `confidence=high` for the dispute; `stance=disputes`; `classification=correction`; `locator=UgzFmYYlHJgtND9FJF54AaABAg.Aa17KdPEKs4Aa3XBjP2KiT, ts=1787961600`. The reply says application DB calls should never exceed single-digit milliseconds regardless of dataset and attributes slower calls to poor craftsmanship. PostgreSQL’s official docs say plan choice depends on query structure and data, planner costs depend on platform parameters, indexes have overhead, and `EXPLAIN ANALYZE` is needed to inspect reality. ",
    "jet_evidence": "#3011 complete workload/assurance evidence; #3016 exact code; #3017 asm repair",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Use as a qualified evidence input on the named card; no popularity or universal speed claim."
  },
  {
    "topic": "workload-magnitude",
    "claim": "Workload magnitude affects users’ performance intuition, but the comment gives no metric.",
    "source_id": "8xBJPa_480Q-audience",
    "source_kind": "audience",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "youtube-comment:UgzFmYYlHJgtND9FJF54AaABAg",
    "locator": "UgzFmYYlHJgtND9FJF54AaABAg, ts=1787875200",
    "confidence": "low",
    "stance": "supports",
    "correction": "`source_kind=audience`; `confidence=low/medium`; `stance=supports`; `classification=pain`; `locator=UgzFmYYlHJgtND9FJF54AaABAg, ts=1787875200`. The root objects that “a million items” should excuse seconds and says people lack modern-computer timing intuition. **Correction:** without input size, output, storage, allocation, network, and hardware details, this cannot be a target number. ",
    "jet_evidence": "#3011 complete workload/assurance evidence; #3016 exact code; #3017 asm repair",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Use as a qualified evidence input on the named card; no popularity or universal speed claim."
  },
  {
    "topic": "maintainability-security",
    "claim": "Performance is one tradeoff among maintainability and security.",
    "source_id": "8xBJPa_480Q-audience",
    "source_kind": "audience",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "youtube-comment:UgyqeTz8OB3IhF7SARV4AaABAg",
    "locator": "UgyqeTz8OB3IhF7SARV4AaABAg, ts=1788912000",
    "confidence": "medium",
    "stance": "neutral",
    "correction": "`source_kind=audience`; `confidence=medium`; `stance=neutral`; `classification=preference/alternate tradeoff`; `locator=UgyqeTz8OB3IhF7SARV4AaABAg, ts=1788912000`. The commenter accepts performance as important but says business systems may prioritize maintainability and security. This does not refute the video’s performance examples. ",
    "jet_evidence": "#3011 complete workload/assurance evidence; #3016 exact code; #3017 asm repair",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Use as a qualified evidence input on the named card; no popularity or universal speed claim."
  },
  {
    "topic": "correctness-before-speed",
    "claim": "Correctness should precede optimization.",
    "source_id": "8xBJPa_480Q-audience",
    "source_kind": "audience",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "youtube-comment:Ugxsm1Ct1TmBVvpZGBN4AaABAg",
    "locator": "Ugxsm1Ct1TmBVvpZGBN4AaABAg, ts=1788307200",
    "confidence": "medium",
    "stance": "supports",
    "correction": "`source_kind=audience`, with narrow support from `https://www.computerenhance.com/p/theroot`; `confidence=medium`; `stance=supports`; `classification=workaround/process`; `locator=Ugxsm1Ct1TmBVvpZGBN4AaABAg, ts=1788307200` (comment points to video `31:45`). The comment says optimization is premature before checking that the algorithm produces correct values. Computer Enhance’s linked article explains that “premature optimization is the root of all evil” is historically/contextually ambiguous, not a blanket “never optimize” rule. ",
    "jet_evidence": "#3011 complete workload/assurance evidence; #3016 exact code; #3017 asm repair",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Use as a qualified evidence input on the named card; no popularity or universal speed claim."
  },
  {
    "topic": "assembly-literacy",
    "claim": "Assembly literacy has a beginner/expert split.",
    "source_id": "8xBJPa_480Q-audience",
    "source_kind": "audience",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "youtube-comment:UgyaKjzk9Awk_vmF7Zt4AaABAg",
    "locator": "UgyaKjzk9Awk_vmF7Zt4AaABAg, ts=1788220800; UgxnFdyeinzdpvjw7Xp4AaABAg",
    "confidence": "medium",
    "stance": "neutral",
    "correction": "`source_kind=audience` with `linked-source` corroboration; `confidence=medium`; `stance=neutral`; `classification=preference`; `locator=UgyaKjzk9Awk_vmF7Zt4AaABAg, ts=1788220800; UgxnFdyeinzdpvjw7Xp4AaABAg`. One commenter says CPU understanding helps but random line-by-line assembly reading is not necessary except for compiler/CPU/hardware/embedded work; another argues vibe coders should read resulting assembly. LLVM’s code-generator documentation confirms that final assembly is the product of target selection, scheduling, machine optimization, register allocation, prolog/epilog, late passes, and emission. ",
    "jet_evidence": "#3011 complete workload/assurance evidence; #3016 exact code; #3017 asm repair",
    "classification": "needs-measurement",
    "owner": "#3016",
    "action": "Use as a qualified evidence input on the named card; no popularity or universal speed claim."
  },
  {
    "topic": "read-versus-write-assembly",
    "claim": "Reading compiler output and writing assembly are different claims.",
    "source_id": "8xBJPa_480Q-audience",
    "source_kind": "audience",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "youtube-comment:UgwnUJmnL7Dam72iZXZ4AaABAg.Aa1GaTaXmEJAa56bjn5Ln-",
    "locator": "UgwnUJmnL7Dam72iZXZ4AaABAg.Aa1GaTaXmEJAa56bjn5Ln-, ts=1788048000; UgwnUJmnL7Dam72iZXZ4AaABAg.Aa1GaTaXmEJAa5F0d6wkj-, ts=1788048000",
    "confidence": "medium",
    "stance": "supports",
    "correction": "`source_kind=audience` with `linked-source` corroboration; `confidence=medium`; `stance=supports` only for the narrow distinction; `classification=correction/alternate explanation`; `locator=UgwnUJmnL7Dam72iZXZ4AaABAg.Aa1GaTaXmEJAa56bjn5Ln-, ts=1788048000; UgwnUJmnL7Dam72iZXZ4AaABAg.Aa1GaTaXmEJAa5F0d6wkj-, ts=1788048000`. Replies claim AAA teams read compiler assembly but do not write all code in assembly because tooling/time matters, and that low-level programmers can sometimes approximate compiler output. LLVM docs support the compiler-output pipeline, but not the AAA-wide claim or the accuracy of human approximation. ",
    "jet_evidence": "#3011 complete workload/assurance evidence; #3016 exact code; #3017 asm repair",
    "classification": "needs-measurement",
    "owner": "#3016",
    "action": "Use as a qualified evidence input on the named card; no popularity or universal speed claim."
  },
  {
    "topic": "typed-asm-operands",
    "claim": "Expert inline-assembly controls carry explicit semantic/effect contracts.",
    "source_id": "8xBJPa_480Q-audience",
    "source_kind": "linked-source",
    "source_ref": "https://doc.rust-lang.org/reference/inline-assembly.html",
    "source_identity": "https://doc.rust-lang.org/reference/inline-assembly.html",
    "locator": "https://doc.rust-lang.org/reference/inline-assembly.html",
    "confidence": "medium",
    "stance": "supports",
    "correction": "`source_kind=linked-source`; `confidence=high` for the API facts; `stance=supports`; `classification=technical mechanism`; `locator=https://doc.rust-lang.org/reference/inline-assembly.html`. Rust’s official Reference documents `in/out/lateout/inout` operands, explicit/register classes, ABI clobbers, target restrictions, and options such as `pure`, `nomem`, `readonly`, `preserves_flags`, `noreturn`, and `nostack`; incorrect memory/effect assertions can be undefined behavior. This is corroboration of a low-level control surface, not evidence that Jet should copy Rust syntax. ",
    "jet_evidence": "#3011 complete workload/assurance evidence; #3016 exact code; #3017 asm repair",
    "classification": "ratified-in-progress",
    "owner": "#3017",
    "action": "Use as a qualified evidence input on the named card; no popularity or universal speed claim."
  },
  {
    "topic": "interpreter-overhead",
    "claim": "“Python benchmark” is not a stable language-level category.",
    "source_id": "8xBJPa_480Q-audience",
    "source_kind": "audience",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "youtube-comment:UgxEoIK1Q9NRxjm_hSt4AaABAg",
    "locator": "UgxEoIK1Q9NRxjm_hSt4AaABAg, ts=1787875200; reply UgxEoIK1Q9NRxjm_hSt4AaABAg.Aa1CdnU2EjyAa1EtojTcfm, ts=1787961600",
    "confidence": "medium",
    "stance": "disputes",
    "correction": "`source_kind=audience` with `linked-source` corroboration; `confidence=high` for the confound mechanism, medium for the anecdote; `stance=disputes` the blanket interpretation; `classification=correction`; `locator=UgxEoIK1Q9NRxjm_hSt4AaABAg, ts=1787875200; reply UgxEoIK1Q9NRxjm_hSt4AaABAg.Aa1CdnU2EjyAa1EtojTcfm, ts=1787961600`. The root reports 1BRC at 5 seconds in Python versus 1.9 seconds in C, 100 versus 600 lines, and says C can be linked from Python; the reply says native code or Cython must be involved and that plain Python operators cannot produce the result. Python’s official extension docs confirm C/C++ extension modules and `ctypes`/cffi paths; Python 3.13 docs confirm an experimental JIT disabled by default. The reply’s “physically incapable” wording is too absolute across implementations, but the need to identify interpreter, extensions, hot loop, I/O, and profile is sound. ",
    "jet_evidence": "#3011 complete workload/assurance evidence; #3016 exact code; #3017 asm repair",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Use as a qualified evidence input on the named card; no popularity or universal speed claim."
  },
  {
    "topic": "ffi-comparison-confounds",
    "claim": "Rust-Godot “almost 3x slower” is a low-confidence anecdote.",
    "source_id": "8xBJPa_480Q-audience",
    "source_kind": "audience",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "youtube-comment:Ugyaf1vuagn9xv0Q9td4AaABAg",
    "locator": "Ugyaf1vuagn9xv0Q9td4AaABAg, ts=1788393600",
    "confidence": "low",
    "stance": "neutral",
    "correction": "`source_kind=audience`; `confidence=low`; `stance=neutral/needs verification`; `classification=alternate cause`; `locator=Ugyaf1vuagn9xv0Q9td4AaABAg, ts=1788393600`. The commenter compares a three-line godot-cpp/Odin implementation with Rust and mentions a function pointer, but supplies no repository, compiler version, profile, ABI, FFI boundary, measurement harness, or workload. ",
    "jet_evidence": "#3011 complete workload/assurance evidence; #3016 exact code; #3017 asm repair",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Use as a qualified evidence input on the named card; no popularity or universal speed claim."
  },
  {
    "topic": "codegen-unit-tradeoff",
    "claim": "Backend controls expose real compile/runtime tradeoffs.",
    "source_id": "8xBJPa_480Q-audience",
    "source_kind": "linked-source",
    "source_ref": "https://doc.rust-lang.org/rustc/codegen-options/index.html#codegen-units",
    "source_identity": "https://doc.rust-lang.org/rustc/codegen-options/index.html#codegen-units",
    "locator": "https://doc.rust-lang.org/rustc/codegen-options/index.html#codegen-units",
    "confidence": "medium",
    "stance": "supports",
    "correction": "`source_kind=linked-source`; `confidence=high`; `stance=supports`; `classification=technical mechanism`; `locator=https://doc.rust-lang.org/rustc/codegen-options/index.html#codegen-units`. rustc documents that multiple codegen units can improve compile parallelism while potentially producing slower runtime code, while one unit can improve generated code at compile-time cost; the page also documents LLVM IR emission and related backend controls. ",
    "jet_evidence": "#3011 complete workload/assurance evidence; #3016 exact code; #3017 asm repair",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Use as a qualified evidence input on the named card; no popularity or universal speed claim."
  },
  {
    "topic": "effect-aware-recovery",
    "claim": "Network failure recovery often needs idempotency and persisted state.",
    "source_id": "8xBJPa_480Q-audience",
    "source_kind": "audience",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "youtube-comment:Ugy2ph7q5IyyXEOZysJ4AaABAg",
    "locator": "Ugy2ph7q5IyyXEOZysJ4AaABAg, ts=1788048000",
    "confidence": "medium",
    "stance": "supports",
    "correction": "`source_kind=audience`; `confidence=medium`; `stance=supports`; `classification=workaround`; `locator=Ugy2ph7q5IyyXEOZysJ4AaABAg, ts=1788048000` (comment references video points `34:46` and `1:01:20`). The commenter says rerunning after network failures requires rerunnable code plus idempotency or a stored finite-state machine. This is an architecture/workflow observation, not a verified Jet feature requirement. ",
    "jet_evidence": "#3011 complete workload/assurance evidence; #3016 exact code; #3017 asm repair",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Use as a qualified evidence input on the named card; no popularity or universal speed claim."
  },
  {
    "topic": "cultural-causality",
    "claim": "The claimed Clean Code → performance cultural shift is unverified.",
    "source_id": "8xBJPa_480Q-audience",
    "source_kind": "audience",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "youtube-comment:UgzCcR_YmB4h3lzI1h14AaABAg",
    "locator": "UgzCcR_YmB4h3lzI1h14AaABAg, ts=1787961600",
    "confidence": "low",
    "stance": "neutral",
    "correction": "`source_kind=audience`; `confidence=low`; `stance=neutral`; `classification=alternate cause`; `locator=UgzCcR_YmB4h3lzI1h14AaABAg, ts=1787961600`. The root attributes a broad trend to Casey and hardware prices; a reply gives an anecdote about an older fast Stack Overflow-style stack using static classes, inline SQL, and simple networking. No independent trend data or causal evidence was supplied. ",
    "jet_evidence": "#3011 complete workload/assurance evidence; #3016 exact code; #3017 asm repair",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Use as a qualified evidence input on the named card; no popularity or universal speed claim."
  },
  {
    "topic": "cloud-hardware-incentives",
    "claim": "Cloud/SaaS cost internalization and slower hardware gains are plausible alternate causes.",
    "source_id": "8xBJPa_480Q-audience",
    "source_kind": "audience",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "youtube-comment:Ugzc-AJOVAp3x2sHllN4AaABAg",
    "locator": "Ugzc-AJOVAp3x2sHllN4AaABAg, ts=1788048000",
    "confidence": "low",
    "stance": "neutral",
    "correction": "`source_kind=audience`; `confidence=low/medium`; `stance=neutral`; `classification=alternate cause`; `locator=Ugzc-AJOVAp3x2sHllN4AaABAg, ts=1788048000`. The commenter argues that cloud/SaaS makes wasted compute visible to providers and that a 5950X→9950X jump feels smaller than earlier generations. This is a useful hypothesis, not corroborated economic or hardware evidence. ",
    "jet_evidence": "#3011 complete workload/assurance evidence; #3016 exact code; #3017 asm repair",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Use as a qualified evidence input on the named card; no popularity or universal speed claim."
  },
  {
    "topic": "ai-assurance",
    "claim": "AI-quality claims are mostly preference/anecdote; executable testing is the actionable part.",
    "source_id": "8xBJPa_480Q-audience",
    "source_kind": "audience",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "youtube-comment:UgxgT6pFe8FYS3vlieZ4AaABAg",
    "locator": "UgxgT6pFe8FYS3vlieZ4AaABAg, ts=1788393600; reply UgxgT6pFe8FYS3vlieZ4AaABAg.AaFNswP0xAZAaHaggWX2Zk; UgxWwN4p6UnLYucASOx4AaABAg; UgyF8jJCEs9rrX9Q4Ul4AaABAg",
    "confidence": "low",
    "stance": "neutral",
    "correction": "`source_kind=audience` plus `linked-source`; `confidence=low` for audience claims, high for the testing mechanism; `stance=neutral`; `classification=preference/noise`; `locator=UgxgT6pFe8FYS3vlieZ4AaABAg, ts=1788393600; reply UgxgT6pFe8FYS3vlieZ4AaABAg.AaFNswP0xAZAaHaggWX2Zk; UgxWwN4p6UnLYucASOx4AaABAg; UgyF8jJCEs9rrX9Q4Ul4AaABAg`. Comments dispute whether AI can produce equal/better code and give unsupported percentage/layoff claims. The Antithesis sponsor URL `https://antithesis.com/pragmatic` redirected without substantive content; official docs at `https://docs.antithesis.com/docs/introduction/welcome/` do support deterministic simulation, hostile testing, reproducible failures, and assertion-driven verification. They do not establish any sponsor-specific Jet result. ",
    "jet_evidence": "#3011 complete workload/assurance evidence; #3016 exact code; #3017 asm repair",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Use as a qualified evidence input on the named card; no popularity or universal speed claim."
  },
  {
    "topic": "simple-tools",
    "claim": "Simple tools plus explicit limits are a durable workaround.",
    "source_id": "8xBJPa_480Q-audience",
    "source_kind": "audience",
    "source_ref": "https://www.youtube.com/watch?v=8xBJPa_480Q",
    "source_identity": "youtube-comment:UgzI3qQI0ZPdggkeXnp4AaABAg",
    "locator": "UgzI3qQI0ZPdggkeXnp4AaABAg, ts=1788825600",
    "confidence": "low",
    "stance": "supports",
    "correction": "`source_kind=audience`; `confidence=low/medium`; `stance=supports`; `classification=workaround/preference`; `locator=UgzI3qQI0ZPdggkeXnp4AaABAg, ts=1788825600`. The commenter describes 1990s game-development practice: use simple existing tools, know their limits, avoid forcing real-time 3D where it is not needed. It is anecdotal but directly useful as a design principle. ",
    "jet_evidence": "#3011 complete workload/assurance evidence; #3016 exact code; #3017 asm repair",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Use as a qualified evidence input on the named card; no popularity or universal speed claim."
  }
]
```

</details>

<details>
<summary>local: 14 normalized claims</summary>

```json
[
  {
    "topic": "current-build-failure",
    "claim": "The fresh compiler build exits 101 on MIRRust E0282, so no current Jet runtime proof is available.",
    "source_id": "casey-crosscheck",
    "source_kind": "local-evidence",
    "source_ref": "docs/audits/mine-for-jet-casey-2026-09-10.md",
    "source_identity": "jet-working-tree-2026-09-10",
    "locator": "crates/jet-codegen/src/Codegen/MIRRust.rs:10455",
    "confidence": "high",
    "stance": "supports",
    "correction": "Source/contract evidence is not fresh Jet runtime proof.",
    "jet_evidence": "crates/jet-codegen/src/Codegen/MIRRust.rs:10455",
    "classification": "real-gap",
    "owner": "#3010",
    "action": "Repair the current build, then rerun the named focused scenarios."
  },
  {
    "topic": "exact-machine-code-inspection",
    "claim": "The inspect registry lacks exact machine output; emit --rust omits hidden foreign bridge preparation.",
    "source_id": "casey-crosscheck",
    "source_kind": "local-evidence",
    "source_ref": "docs/audits/mine-for-jet-casey-2026-09-10.md",
    "source_identity": "jet-working-tree-2026-09-10",
    "locator": "crates/jet-cli/src/CLI.rs:267-290,671-699; Source/CmdDevTools.rs:7910-7971",
    "confidence": "high",
    "stance": "supports",
    "correction": "Source/contract evidence is not fresh Jet runtime proof.",
    "jet_evidence": "crates/jet-cli/src/CLI.rs:267-290,671-699; Source/CmdDevTools.rs:7910-7971",
    "classification": "owner-gate",
    "owner": "#3016",
    "action": "Resolve D-CODE-INSPECT1, then implement one artifact projection."
  },
  {
    "topic": "typed-asm-operands",
    "claim": "Current clobbers lower as lateout, while D-FFI-ASMOPS1=C specifies out for discarded clobbers.",
    "source_id": "casey-crosscheck",
    "source_kind": "local-evidence",
    "source_ref": "docs/audits/mine-for-jet-casey-2026-09-10.md",
    "source_identity": "jet-working-tree-2026-09-10",
    "locator": "crates/jet-pkg-model/src/FFI.rs:5097-5101; D-FFI-ASMOPS1=C",
    "confidence": "high",
    "stance": "supports",
    "correction": "Source/contract evidence is not fresh Jet runtime proof.",
    "jet_evidence": "crates/jet-pkg-model/src/FFI.rs:5097-5101; D-FFI-ASMOPS1=C",
    "classification": "real-gap",
    "owner": "#3017",
    "action": "Reproduce early-written-clobber overlap and repair canonical checked lowering."
  },
  {
    "topic": "asm-selected-target",
    "claim": "The target diagnostic rejects every non-x86_64 target regardless of body.",
    "source_id": "casey-crosscheck",
    "source_kind": "local-evidence",
    "source_ref": "docs/audits/mine-for-jet-casey-2026-09-10.md",
    "source_identity": "jet-working-tree-2026-09-10",
    "locator": "crates/jet-pkg-model/src/FFI.rs:454-470",
    "confidence": "high",
    "stance": "supports",
    "correction": "Source/contract evidence is not fresh Jet runtime proof.",
    "jet_evidence": "crates/jet-pkg-model/src/FFI.rs:454-470",
    "classification": "real-gap",
    "owner": "#3017",
    "action": "Prove existing target-conditional body selection and wrong-target rejection; do not infer a new AST form is necessary."
  },
  {
    "topic": "inline-foreign-cross-mode",
    "claim": "Source applicability excludes inline native foreign functions from Cranelift and interpreter; later I9 forbids a feature gap.",
    "source_id": "casey-crosscheck",
    "source_kind": "local-evidence",
    "source_ref": "docs/audits/mine-for-jet-casey-2026-09-10.md",
    "source_identity": "jet-working-tree-2026-09-10",
    "locator": "crates/jet-codegen/src/Codegen/TIR/mod.rs:10269-10308; crates/jet-jit/src/jit/backend.rs:22-70",
    "confidence": "high",
    "stance": "supports",
    "correction": "Source/contract evidence is not fresh Jet runtime proof.",
    "jet_evidence": "crates/jet-codegen/src/Codegen/TIR/mod.rs:10269-10308; crates/jet-jit/src/jit/backend.rs:22-70",
    "classification": "real-gap",
    "owner": "#3017",
    "action": "Repair applicable native execution through the shared foreign contract; live contrast follows #3010."
  },
  {
    "topic": "cranelift-optimization-setting",
    "claim": "Resident and debug-AOT Cranelift currently use opt_level=none.",
    "source_id": "casey-crosscheck",
    "source_kind": "local-evidence",
    "source_ref": "docs/audits/mine-for-jet-casey-2026-09-10.md",
    "source_identity": "jet-working-tree-2026-09-10",
    "locator": "crates/jet-jit/src/jit/runtime_host.rs:10125-10178; crates/jet-jit/src/jit/api_debug.rs:70-97",
    "confidence": "high",
    "stance": "supports",
    "correction": "Source/contract evidence is not fresh Jet runtime proof.",
    "jet_evidence": "crates/jet-jit/src/jit/runtime_host.rs:10125-10178; crates/jet-jit/src/jit/api_debug.rs:70-97",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Compare compilation latency and runtime under the existing corpus gate, not a new backend selector."
  },
  {
    "topic": "backend-probe-compilation",
    "claim": "The local QBE SSA-to-assembly invocation median is 1.709 ms versus LLVM IR-to-assembly 23.612 ms.",
    "source_id": "casey-crosscheck",
    "source_kind": "local-evidence",
    "source_ref": "docs/audits/mine-for-jet-casey-2026-09-10.md",
    "source_identity": "local-backend-probe-2026-09-10",
    "locator": "backend-probe/runner.py:43-54; results.json",
    "confidence": "high",
    "stance": "supports",
    "correction": "This evidence does not establish a Jet performance win.",
    "jet_evidence": "backend-probe/runner.py:43-54; results.json",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Treat as warmed isolated process latency, not Jet whole-build speed."
  },
  {
    "topic": "backend-probe-runtime",
    "claim": "At 1048576 elements, QBE sum is 0.2259 ns/element versus LLVM IR 0.1101; dependent chase is 2.6427 versus 2.6581.",
    "source_id": "casey-crosscheck",
    "source_kind": "local-evidence",
    "source_ref": "docs/audits/mine-for-jet-casey-2026-09-10.md",
    "source_identity": "local-backend-probe-2026-09-10",
    "locator": "backend-probe/summary.json; 9 variants x 6 sizes with independent oracle",
    "confidence": "high",
    "stance": "supports",
    "correction": "This evidence does not establish a Jet performance win.",
    "jet_evidence": "backend-probe/summary.json; 9 variants x 6 sizes with independent oracle",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Retain per-kernel tradeoffs and require a real Jet candidate before any comparative win."
  },
  {
    "topic": "backend-probe-size",
    "claim": "The common-driver QBE executable is 16168 bytes; LLVM IR is 16208 bytes. Go includes its runtime and is not a pure-code-size comparator.",
    "source_id": "casey-crosscheck",
    "source_kind": "local-evidence",
    "source_ref": "docs/audits/mine-for-jet-casey-2026-09-10.md",
    "source_identity": "local-backend-probe-2026-09-10",
    "locator": "backend-probe/size-results.json",
    "confidence": "high",
    "stance": "supports",
    "correction": "This evidence does not establish a Jet performance win.",
    "jet_evidence": "backend-probe/size-results.json",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Separate kernel text, runtime, debug, object and linked-image sizes in canonical measurements."
  },
  {
    "topic": "selection-partition-api",
    "claim": "Lists lack rank selection; predicate partition has a different meaning.",
    "source_id": "casey-crosscheck",
    "source_kind": "local-evidence",
    "source_ref": "docs/audits/mine-for-jet-casey-2026-09-10.md",
    "source_identity": "jet-working-tree-2026-09-10",
    "locator": "crates/jet-foundation/src/Collections.rs:291-324,1512-1714",
    "confidence": "high",
    "stance": "supports",
    "correction": "Source/contract evidence is not fresh Jet runtime proof.",
    "jet_evidence": "crates/jet-foundation/src/Collections.rs:291-324,1512-1714",
    "classification": "owner-gate",
    "owner": "#3018",
    "action": "Resolve D-RANK-SELECT1 for non-shipping prototype authorization. If approved, gather canonical candidate/plain proof, then raise the separate public-adoption ballot; adoption requires the strict performance gate."
  },
  {
    "topic": "float-total-order",
    "claim": "Collection sorting puts all NaNs last and equal, while signed zeros compare equal.",
    "source_id": "casey-crosscheck",
    "source_kind": "local-evidence",
    "source_ref": "docs/audits/mine-for-jet-casey-2026-09-10.md",
    "source_identity": "jet-working-tree-2026-09-10",
    "locator": "crates/jet-codegen/src/Prelude/Core/FloatOrdering.rs:1-13",
    "confidence": "high",
    "stance": "supports",
    "correction": "Source/contract evidence is not fresh Jet runtime proof.",
    "jet_evidence": "crates/jet-codegen/src/Prelude/Core/FloatOrdering.rs:1-13",
    "classification": "already-implemented",
    "owner": "#3018",
    "action": "Reuse D-FLOATSORT1 for any rank surface; do not copy data.quantile nonfinite rejection into collections."
  },
  {
    "topic": "batch-quantile-demand",
    "claim": "Repeated scalar quantiles each clone and sort; batch q requests are a plausible but unproven product need.",
    "source_id": "casey-crosscheck",
    "source_kind": "local-evidence",
    "source_ref": "docs/audits/mine-for-jet-casey-2026-09-10.md",
    "source_identity": "jet-working-tree-2026-09-10",
    "locator": "crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs:313-340",
    "confidence": "high",
    "stance": "supports",
    "correction": "Source/contract evidence is not fresh Jet runtime proof.",
    "jet_evidence": "crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs:313-340",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Measure repeated-q workloads before raising a separate output-shape ballot; #3012 owns existing scalar tuning."
  },
  {
    "topic": "query-limit-demand",
    "claim": "Typed Query has no limit transition, although SQL and table plans already carry Limit.",
    "source_id": "casey-crosscheck",
    "source_kind": "local-evidence",
    "source_ref": "docs/audits/mine-for-jet-casey-2026-09-10.md",
    "source_identity": "jet-working-tree-2026-09-10",
    "locator": "crates/jet-sema/src/Sema/DataPlan.rs:48-64; crates/jet-codegen/src/Prelude/Core/LazyTablePlan.rs:825-880,1844-1860",
    "confidence": "high",
    "stance": "supports",
    "correction": "Source/contract evidence is not fresh Jet runtime proof.",
    "jet_evidence": "crates/jet-sema/src/Sema/DataPlan.rs:48-64; crates/jet-codegen/src/Prelude/Core/LazyTablePlan.rs:825-880,1844-1860",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Establish deferred bounded-stream demand before any Query API ballot; do not duplicate SQL LIMIT or list.take."
  },
  {
    "topic": "measurement-protocol-existing",
    "claim": "Core conformance hostile fixtures reject fake coverage witnesses and denominator rows in this run.",
    "source_id": "casey-crosscheck",
    "source_kind": "local-evidence",
    "source_ref": "docs/audits/mine-for-jet-casey-2026-09-10.md",
    "source_identity": "jet-working-tree-2026-09-10",
    "locator": "scripts/agent/core-conformance.mjs --hostile-fixtures",
    "confidence": "high",
    "stance": "supports",
    "correction": "Source/contract evidence is not fresh Jet runtime proof.",
    "jet_evidence": "scripts/agent/core-conformance.mjs --hostile-fixtures",
    "classification": "already-implemented",
    "owner": "#3011",
    "action": "Reuse these guards when extending first-party inventory; no second coverage registry."
  }
]
```

</details>

<details>
<summary>linked: 5 normalized claims</summary>

```json
[
  {
    "topic": "llvm-tool-reuse",
    "claim": "LLVM already supplies instruction selection, register allocation, vectorizers, debug mapping and assembly output.",
    "source_id": "casey-crosscheck",
    "source_kind": "linked-source",
    "source_ref": "https://llvm.org/docs/CodeGenerator.html",
    "source_identity": "https://llvm.org/docs/CodeGenerator.html",
    "locator": "https://llvm.org/docs/CodeGenerator.html",
    "confidence": "high",
    "stance": "supports",
    "correction": "This evidence does not establish a Jet performance win.",
    "jet_evidence": "https://llvm.org/docs/CodeGenerator.html",
    "classification": "already-implemented",
    "owner": "#3016",
    "action": "Expose actual artifacts and link existing checked reasons; do not rebuild an allocator for visibility."
  },
  {
    "topic": "qbe-quality-tradeoff",
    "claim": "QBE prioritizes small implementation and fast compilation; its own 70 percent performance claim is not a Jet result.",
    "source_id": "casey-crosscheck",
    "source_kind": "linked-source",
    "source_ref": "https://c9x.me/compile/",
    "source_identity": "https://c9x.me/compile/",
    "locator": "https://c9x.me/compile/",
    "confidence": "high",
    "stance": "supports",
    "correction": "This evidence does not establish a Jet performance win.",
    "jet_evidence": "https://c9x.me/compile/",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Screen on matched workloads without replacing the AOT baseline."
  },
  {
    "topic": "lightweight-backend-maturity",
    "claim": "MIR, TinyCC and Cuik/TB have distinct scope and maturity; Cuik labels itself unfinished and buggy.",
    "source_id": "casey-crosscheck",
    "source_kind": "linked-source",
    "source_ref": "https://github.com/vnmakarov/mir; https://bellard.org/tcc/tcc-doc.html; https://github.com/RealNeGate/Cuik",
    "source_identity": "https://github.com/vnmakarov/mir; https://bellard.org/tcc/tcc-doc.html; https://github.com/RealNeGate/Cuik",
    "locator": "https://github.com/vnmakarov/mir; https://bellard.org/tcc/tcc-doc.html; https://github.com/RealNeGate/Cuik",
    "confidence": "high",
    "stance": "supports",
    "correction": "This evidence does not establish a Jet performance win.",
    "jet_evidence": "https://github.com/vnmakarov/mir; https://bellard.org/tcc/tcc-doc.html; https://github.com/RealNeGate/Cuik",
    "classification": "needs-measurement",
    "owner": "#3011",
    "action": "Record unsupported target, ABI, SIMD, debug and safety cells; no adoption claim."
  },
  {
    "topic": "selection-partition-api",
    "claim": "Rust exposes in-place unstable rank partition; NumPy exposes copied and in-place forms.",
    "source_id": "casey-crosscheck",
    "source_kind": "linked-source",
    "source_ref": "https://doc.rust-lang.org/std/primitive.slice.html#method.select_nth_unstable; https://numpy.org/doc/stable/reference/generated/numpy.partition.html",
    "source_identity": "https://doc.rust-lang.org/std/primitive.slice.html#method.select_nth_unstable; https://numpy.org/doc/stable/reference/generated/numpy.partition.html",
    "locator": "https://doc.rust-lang.org/std/primitive.slice.html#method.select_nth_unstable; https://numpy.org/doc/stable/reference/generated/numpy.partition.html",
    "confidence": "high",
    "stance": "supports",
    "correction": "This evidence does not establish a Jet performance win.",
    "jet_evidence": "https://doc.rust-lang.org/std/primitive.slice.html#method.select_nth_unstable; https://numpy.org/doc/stable/reference/generated/numpy.partition.html",
    "classification": "owner-gate",
    "owner": "#3018",
    "action": "Compare ownership, result shape and bounds, not merely algorithm names."
  },
  {
    "topic": "oracle-provenance",
    "claim": "The linked LLM oracle study shows passing generated tests can contain wrong assertions; names influence results in its Java/model setup.",
    "source_id": "casey-crosscheck",
    "source_kind": "linked-source",
    "source_ref": "https://arxiv.org/abs/2410.21136",
    "source_identity": "https://arxiv.org/abs/2410.21136",
    "locator": "https://arxiv.org/abs/2410.21136",
    "confidence": "high",
    "stance": "supports",
    "correction": "This evidence does not establish a Jet performance win.",
    "jet_evidence": "https://arxiv.org/abs/2410.21136",
    "classification": "ratified-in-progress",
    "owner": "#3011",
    "action": "Keep independent expected values, mutation controls, metamorphic/differential checks and minimized failures."
  }
]
```

</details>

<details>
<summary>Combined topic matrix</summary>

```json
[
  {
    "topic": "AI-evidence",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "AI-test-oracle",
    "status": "single",
    "source_identities": [
      "[AI] https://arxiv.org/abs/2410.21136 (HTML: https://arxiv.org/html/2410.21136v1)"
    ],
    "claims": 1
  },
  {
    "topic": "abi-observations",
    "status": "single",
    "source_identities": [
      "youtube:YlFYXewYJ8M"
    ],
    "claims": 1
  },
  {
    "topic": "abi-target-specific",
    "status": "single",
    "source_identities": [
      "https://gitlab.com/x86-psABIs/x86-64-ABI/-/jobs/artifacts/master/raw/x86-64-ABI/abi.pdf?job=build"
    ],
    "claims": 1
  },
  {
    "topic": "absolute-database-latency",
    "status": "single",
    "source_identities": [
      "youtube-comment:UgzFmYYlHJgtND9FJF54AaABAg.Aa17KdPEKs4Aa3XBjP2KiT"
    ],
    "claims": 1
  },
  {
    "topic": "abstraction-performance",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "adversarial-deadline",
    "status": "single",
    "source_identities": [
      "comments:5JXpNOZWAHM"
    ],
    "claims": 1
  },
  {
    "topic": "ai-assurance",
    "status": "single",
    "source_identities": [
      "youtube-comment:UgxgT6pFe8FYS3vlieZ4AaABAg"
    ],
    "claims": 1
  },
  {
    "topic": "allocator-defaults",
    "status": "single",
    "source_identities": [
      "https://llvm.org/docs/CodeGenerator.html"
    ],
    "claims": 1
  },
  {
    "topic": "asm-selected-target",
    "status": "single",
    "source_identities": [
      "jet-working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "assembly-literacy",
    "status": "repeated",
    "source_identities": [
      "speaker:casey-muratori",
      "youtube-comment:UgyaKjzk9Awk_vmF7Zt4AaABAg"
    ],
    "claims": 2
  },
  {
    "topic": "backend-filetests",
    "status": "single",
    "source_identities": [
      "https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/testing.md"
    ],
    "claims": 1
  },
  {
    "topic": "backend-probe-compilation",
    "status": "single",
    "source_identities": [
      "local-backend-probe-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "backend-probe-runtime",
    "status": "single",
    "source_identities": [
      "local-backend-probe-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "backend-probe-size",
    "status": "single",
    "source_identities": [
      "local-backend-probe-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "batch-quantile-demand",
    "status": "single",
    "source_identities": [
      "jet-working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "benchmark-attribution",
    "status": "single",
    "source_identities": [
      "[CC] https://www.computerenhance.com/p/clean-code-horrible-performance"
    ],
    "claims": 1
  },
  {
    "topic": "bfprt-original-bound",
    "status": "single",
    "source_identities": [
      "paper:BFPRT73"
    ],
    "claims": 1
  },
  {
    "topic": "bitset-cost-model",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "bootstrap-ddc",
    "status": "single",
    "source_identities": [
      "https://dwheeler.com/trusting-trust/dissertation/html/wheeler-trusting-trust-ddc.html"
    ],
    "claims": 1
  },
  {
    "topic": "bootstrap-gates-open",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "bootstrap-stage-identity",
    "status": "single",
    "source_identities": [
      "https://rustc-dev-guide.rust-lang.org/building/bootstrapping/what-bootstrapping-does.html"
    ],
    "claims": 1
  },
  {
    "topic": "candidate-verifier-separation",
    "status": "single",
    "source_identities": [
      "youtube:YlFYXewYJ8M"
    ],
    "claims": 1
  },
  {
    "topic": "cloud-hardware-incentives",
    "status": "single",
    "source_identities": [
      "youtube-comment:Ugzc-AJOVAp3x2sHllN4AaABAg"
    ],
    "claims": 1
  },
  {
    "topic": "code-quality",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "codegen-unit-tradeoff",
    "status": "single",
    "source_identities": [
      "https://doc.rust-lang.org/rustc/codegen-options/index.html#codegen-units"
    ],
    "claims": 1
  },
  {
    "topic": "comparison-model",
    "status": "single",
    "source_identities": [
      "CMU:15451-f23-lecture01"
    ],
    "claims": 1
  },
  {
    "topic": "compiler-proof-not-universal",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "core-discovery-duplication",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "correctness-before-speed",
    "status": "single",
    "source_identities": [
      "youtube-comment:Ugxsm1Ct1TmBVvpZGBN4AaABAg"
    ],
    "claims": 1
  },
  {
    "topic": "coverage-not-correctness",
    "status": "single",
    "source_identities": [
      "https://rustc-dev-guide.rust-lang.org/tests/compiletest.html"
    ],
    "claims": 1
  },
  {
    "topic": "craft-choice",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "cranelift-block-parameters",
    "status": "single",
    "source_identities": [
      "https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/ir.md"
    ],
    "claims": 1
  },
  {
    "topic": "cranelift-optimization-setting",
    "status": "single",
    "source_identities": [
      "jet-working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "cultural-causality",
    "status": "single",
    "source_identities": [
      "youtube-comment:UgzCcR_YmB4h3lzI1h14AaABAg"
    ],
    "claims": 1
  },
  {
    "topic": "current-build-failure",
    "status": "repeated",
    "source_identities": [
      "jet-working-tree-2026-09-10",
      "jet:working-tree-2026-09-10"
    ],
    "claims": 2
  },
  {
    "topic": "data-one-kernel",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "dependency-structure",
    "status": "repeated",
    "source_identities": [
      "speaker:casey-muratori",
      "youtube-comment:UgxHhgLAhH2RzSUcL8V4AaABAg"
    ],
    "claims": 2
  },
  {
    "topic": "dependency-structure-nplus1",
    "status": "single",
    "source_identities": [
      "youtube-comment:Ugz7CnPFNsAul6r-Sip4AaABAg"
    ],
    "claims": 1
  },
  {
    "topic": "describe-repeat-work",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "differential-independence",
    "status": "single",
    "source_identities": [
      "https://plf.inf.ethz.ch/research/oopsla24-rustlantis.html"
    ],
    "claims": 1
  },
  {
    "topic": "dispatch-tradeoff",
    "status": "single",
    "source_identities": [
      "[Rust] https://doc.rust-lang.org/book/ch18-02-trait-objects.html"
    ],
    "claims": 1
  },
  {
    "topic": "duplicate-partition",
    "status": "single",
    "source_identities": [
      "youtube:5JXpNOZWAHM"
    ],
    "claims": 1
  },
  {
    "topic": "dynamic-model-limits",
    "status": "single",
    "source_identities": [
      "https://github.com/rust-lang/miri/blob/master/README.md"
    ],
    "claims": 1
  },
  {
    "topic": "ecosystem",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "ecosystem-real-programs",
    "status": "single",
    "source_identities": [
      "https://rustc-dev-guide.rust-lang.org/tests/ecosystem.html"
    ],
    "claims": 1
  },
  {
    "topic": "effect-aware-recovery",
    "status": "single",
    "source_identities": [
      "youtube-comment:Ugy2ph7q5IyyXEOZysJ4AaABAg"
    ],
    "claims": 1
  },
  {
    "topic": "effect-worklist-existing",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "empirical-worst",
    "status": "single",
    "source_identities": [
      "youtube:5JXpNOZWAHM"
    ],
    "claims": 1
  },
  {
    "topic": "engineering-practice",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "exact-machine-code-inspection",
    "status": "single",
    "source_identities": [
      "jet-working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "ffi-comparison-confounds",
    "status": "single",
    "source_identities": [
      "youtube-comment:Ugyaf1vuagn9xv0Q9td4AaABAg"
    ],
    "claims": 1
  },
  {
    "topic": "fft-existing-owner",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "float-total-order",
    "status": "repeated",
    "source_identities": [
      "jet-working-tree-2026-09-10",
      "rust:std-selection"
    ],
    "claims": 2
  },
  {
    "topic": "formal-boundary",
    "status": "single",
    "source_identities": [
      "https://compcert.org/man/manual001.html"
    ],
    "claims": 1
  },
  {
    "topic": "formal-model-correspondence",
    "status": "single",
    "source_identities": [
      "https://plv.mpi-sws.org/rustbelt/popl18/"
    ],
    "claims": 1
  },
  {
    "topic": "full-cost-inventory-gap",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "fuzz-minimized-regressions",
    "status": "single",
    "source_identities": [
      "https://rustc-dev-guide.rust-lang.org/fuzzing.html"
    ],
    "claims": 1
  },
  {
    "topic": "game-history",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "group-five",
    "status": "single",
    "source_identities": [
      "CMU:15451-f23-lecture01"
    ],
    "claims": 1
  },
  {
    "topic": "group-size-cost",
    "status": "single",
    "source_identities": [
      "comments:5JXpNOZWAHM"
    ],
    "claims": 1
  },
  {
    "topic": "group-three",
    "status": "single",
    "source_identities": [
      "CMU:15451-f23-lecture01"
    ],
    "claims": 1
  },
  {
    "topic": "half-subset-pivot",
    "status": "single",
    "source_identities": [
      "youtube:5JXpNOZWAHM"
    ],
    "claims": 1
  },
  {
    "topic": "hardware-model",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "incentives",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "incremental-frontier",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "incremental-hostile-cases",
    "status": "single",
    "source_identities": [
      "https://rustc-dev-guide.rust-lang.org/tests/compiletest.html"
    ],
    "claims": 1
  },
  {
    "topic": "independent-hash-oracle",
    "status": "single",
    "source_identities": [
      "inference:Main"
    ],
    "claims": 1
  },
  {
    "topic": "inline-foreign-cross-mode",
    "status": "single",
    "source_identities": [
      "jet-working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "interpreter-overhead",
    "status": "conflict",
    "source_identities": [
      "speaker:casey-muratori",
      "youtube-comment:UgxEoIK1Q9NRxjm_hSt4AaABAg"
    ],
    "claims": 2
  },
  {
    "topic": "isle-generated-rules",
    "status": "single",
    "source_identities": [
      "https://github.com/bytecodealliance/wasmtime/blob/main/cranelift/docs/isle-integration.md"
    ],
    "claims": 1
  },
  {
    "topic": "jit-allocation-choice",
    "status": "single",
    "source_identities": [
      "youtube:YlFYXewYJ8M"
    ],
    "claims": 1
  },
  {
    "topic": "joint-codegen-cost",
    "status": "single",
    "source_identities": [
      "youtube:YlFYXewYJ8M"
    ],
    "claims": 1
  },
  {
    "topic": "learning-method",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "lightweight-backend-maturity",
    "status": "single",
    "source_identities": [
      "https://github.com/vnmakarov/mir; https://bellard.org/tcc/tcc-doc.html; https://github.com/RealNeGate/Cuik"
    ],
    "claims": 1
  },
  {
    "topic": "llvm-tool-reuse",
    "status": "repeated",
    "source_identities": [
      "https://llvm.org/docs/CodeGenerator.html",
      "https://llvm.org/docs/TestingGuide.html"
    ],
    "claims": 2
  },
  {
    "topic": "maintainability-security",
    "status": "single",
    "source_identities": [
      "youtube-comment:UgyqeTz8OB3IhF7SARV4AaABAg"
    ],
    "claims": 1
  },
  {
    "topic": "map-extrema-order",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "map-topn-policy",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "measure-existing",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "measurement-protocol-existing",
    "status": "repeated",
    "source_identities": [
      "jet-working-tree-2026-09-10",
      "jet:working-tree-2026-09-10"
    ],
    "claims": 2
  },
  {
    "topic": "median-contract",
    "status": "single",
    "source_identities": [
      "blog:rcoh-2018"
    ],
    "claims": 1
  },
  {
    "topic": "median-registry",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "memo-cost-model",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "methodology",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "micro-apis-types-methods",
    "status": "single",
    "source_identities": [
      "inference:Main"
    ],
    "claims": 3
  },
  {
    "topic": "micro-ceremony-control",
    "status": "single",
    "source_identities": [
      "inference:Main"
    ],
    "claims": 3
  },
  {
    "topic": "micro-defaults",
    "status": "single",
    "source_identities": [
      "inference:Main"
    ],
    "claims": 3
  },
  {
    "topic": "micro-diagnostics",
    "status": "single",
    "source_identities": [
      "inference:Main"
    ],
    "claims": 3
  },
  {
    "topic": "micro-ergonomics",
    "status": "single",
    "source_identities": [
      "inference:Main"
    ],
    "claims": 3
  },
  {
    "topic": "micro-naming",
    "status": "single",
    "source_identities": [
      "inference:Main"
    ],
    "claims": 3
  },
  {
    "topic": "micro-surfaces",
    "status": "single",
    "source_identities": [
      "inference:Main"
    ],
    "claims": 3
  },
  {
    "topic": "micro-syntax",
    "status": "single",
    "source_identities": [
      "inference:Main"
    ],
    "claims": 3
  },
  {
    "topic": "micro-tooling-cli",
    "status": "single",
    "source_identities": [
      "inference:Main"
    ],
    "claims": 3
  },
  {
    "topic": "micro-ux-dx",
    "status": "single",
    "source_identities": [
      "inference:Main"
    ],
    "claims": 3
  },
  {
    "topic": "mir-dominance-duplication",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "mir-revalidation-cost",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "one-source-inventory",
    "status": "single",
    "source_identities": [
      "jet:source-ledger"
    ],
    "claims": 1
  },
  {
    "topic": "optimization-hardness",
    "status": "single",
    "source_identities": [
      "youtube:YlFYXewYJ8M"
    ],
    "claims": 1
  },
  {
    "topic": "oracle-provenance",
    "status": "single",
    "source_identities": [
      "https://arxiv.org/abs/2410.21136"
    ],
    "claims": 2
  },
  {
    "topic": "partial-metric",
    "status": "single",
    "source_identities": [
      "blog:rcoh-2018"
    ],
    "claims": 1
  },
  {
    "topic": "peephole-semantics",
    "status": "single",
    "source_identities": [
      "youtube:YlFYXewYJ8M"
    ],
    "claims": 1
  },
  {
    "topic": "pivot-not-free",
    "status": "single",
    "source_identities": [
      "youtube:5JXpNOZWAHM"
    ],
    "claims": 1
  },
  {
    "topic": "premature-optimization",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "premise",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "product-lifecycle",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "proof-assurance-levels",
    "status": "single",
    "source_identities": [
      "https://docs.adacore.com/spark2014-docs/html/ug/en/usage_scenarios.html"
    ],
    "claims": 1
  },
  {
    "topic": "provider-specific-measurement",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "qbe-quality-tradeoff",
    "status": "single",
    "source_identities": [
      "https://c9x.me/compile/"
    ],
    "claims": 1
  },
  {
    "topic": "qualification-not-selftest",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "qualification-scope",
    "status": "single",
    "source_identities": [
      "https://public-docs.ferrocene.dev/main/qualification/evaluation-plan/qualification-scope.html"
    ],
    "claims": 1
  },
  {
    "topic": "quantile-sort-cost",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "query-limit-demand",
    "status": "single",
    "source_identities": [
      "jet-working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "quickselect-expected",
    "status": "single",
    "source_identities": [
      "CMU:15451-f23-lecture01"
    ],
    "claims": 1
  },
  {
    "topic": "read-versus-write-assembly",
    "status": "single",
    "source_identities": [
      "youtube-comment:UgwnUJmnL7Dam72iZXZ4AaABAg.Aa1GaTaXmEJAa56bjn5Ln-"
    ],
    "claims": 1
  },
  {
    "topic": "regalloc-api",
    "status": "single",
    "source_identities": [
      "https://github.com/bytecodealliance/regalloc2/blob/main/doc/GENERAL.md"
    ],
    "claims": 1
  },
  {
    "topic": "regalloc-checker",
    "status": "single",
    "source_identities": [
      "https://github.com/bytecodealliance/regalloc2/blob/main/doc/ION.md"
    ],
    "claims": 1
  },
  {
    "topic": "register-allocation-model",
    "status": "single",
    "source_identities": [
      "youtube:YlFYXewYJ8M"
    ],
    "claims": 1
  },
  {
    "topic": "rewrite-evidence",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "rust-codegen-mir-tests",
    "status": "single",
    "source_identities": [
      "https://rustc-dev-guide.rust-lang.org/tests/compiletest.html"
    ],
    "claims": 1
  },
  {
    "topic": "rust-ui-oracle",
    "status": "single",
    "source_identities": [
      "https://rustc-dev-guide.rust-lang.org/tests/ui.html"
    ],
    "claims": 1
  },
  {
    "topic": "selection-partition-api",
    "status": "repeated",
    "source_identities": [
      "https://doc.rust-lang.org/std/primitive.slice.html#method.select_nth_unstable; https://numpy.org/doc/stable/reference/generated/numpy.partition.html",
      "jet-working-tree-2026-09-10",
      "rust:std-selection"
    ],
    "claims": 3
  },
  {
    "topic": "selection-usage",
    "status": "conflict",
    "source_identities": [
      "rust:std-selection",
      "youtube:5JXpNOZWAHM"
    ],
    "claims": 2
  },
  {
    "topic": "selection-vs-sort",
    "status": "single",
    "source_identities": [
      "CMU:15451-f23-lecture01"
    ],
    "claims": 1
  },
  {
    "topic": "simple-tools",
    "status": "single",
    "source_identities": [
      "youtube-comment:UgzI3qQI0ZPdggkeXnp4AaABAg"
    ],
    "claims": 1
  },
  {
    "topic": "sort-key-once",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "sponsored-evidence",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "stale-evidence-existing",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "target-matrix-gates",
    "status": "single",
    "source_identities": [
      "https://llvm.org/docs/ReleaseProcess.html"
    ],
    "claims": 1
  },
  {
    "topic": "target-specific-scheduling",
    "status": "single",
    "source_identities": [
      "youtube:YlFYXewYJ8M"
    ],
    "claims": 1
  },
  {
    "topic": "temporary-sublists",
    "status": "single",
    "source_identities": [
      "blog:rcoh-2018"
    ],
    "claims": 1
  },
  {
    "topic": "test-economics-controls",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "test-outcome-projection",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "testing-strategy",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "timing-sample",
    "status": "single",
    "source_identities": [
      "youtube:5JXpNOZWAHM"
    ],
    "claims": 1
  },
  {
    "topic": "total-job-cost",
    "status": "repeated",
    "source_identities": [
      "youtube:5JXpNOZWAHM",
      "youtube:YlFYXewYJ8M"
    ],
    "claims": 2
  },
  {
    "topic": "typed-asm-operands",
    "status": "repeated",
    "source_identities": [
      "https://doc.rust-lang.org/reference/inline-assembly.html",
      "jet-working-tree-2026-09-10"
    ],
    "claims": 2
  },
  {
    "topic": "unit-resolution-frontier",
    "status": "single",
    "source_identities": [
      "jet:working-tree-2026-09-10"
    ],
    "claims": 1
  },
  {
    "topic": "validation-strategy",
    "status": "single",
    "source_identities": [
      "speaker:casey-muratori"
    ],
    "claims": 1
  },
  {
    "topic": "virtual-registers",
    "status": "single",
    "source_identities": [
      "youtube:YlFYXewYJ8M"
    ],
    "claims": 1
  },
  {
    "topic": "workload-magnitude",
    "status": "single",
    "source_identities": [
      "youtube-comment:UgzFmYYlHJgtND9FJF54AaABAg"
    ],
    "claims": 1
  }
]
```

</details>

## Additional primary-source links

- <https://c9x.me/compile/>
- <https://c9x.me/compile/doc/llvm.html>
- <https://gcc.gnu.org/onlinedocs/gccint/Passes.html>
- <https://gcc.gnu.org/onlinedocs/gccint/RTL.html>
- <https://github.com/RealNeGate/Cuik/blob/master/tb/opt/optimizer.c>
- <https://llvm.org/docs/CodeGenerator.html>
- <https://llvm.org/docs/LangRef.html>
- <https://llvm.org/docs/SourceLevelDebugging.html>
- <https://llvm.org/docs/Vectorizers.html>
- <https://raw.githubusercontent.com/RealNeGate/Cuik/master/README.md>
- <https://raw.githubusercontent.com/TinyCC/tinycc/mob/tcc-doc.texi>
- <https://raw.githubusercontent.com/bytecodealliance/wasmtime/main/cranelift/README.md>
- <https://raw.githubusercontent.com/bytecodealliance/wasmtime/main/cranelift/docs/compare-llvm.md>
- <https://raw.githubusercontent.com/bytecodealliance/wasmtime/main/cranelift/docs/ir.md>
- <https://raw.githubusercontent.com/golang/go/master/src/cmd/compile/README.md>
- <https://raw.githubusercontent.com/rust-lang/rustc-dev-guide/master/src/backend/codegen.md>
- <https://raw.githubusercontent.com/vnmakarov/mir/master/README.md>
- <https://raw.githubusercontent.com/ziglang/zig/master/src/codegen.zig>
- <https://raw.githubusercontent.com/ziglang/zig/master/src/target.zig>

## Closeout checks

- Tower accepted both full ballots and read them back as open, non-draft records with beginner and adversarial provenance. Neither was ratified.
- The official disposition validator ran against this report, using the real live/retired Tower ledgers and real audit-skill directory through a read-only scoped root. Result: **24 dispositions, zero ledger errors**, exit 0.
- `tower lint` exited 1 on existing claimed-idle cards, stale historical drafts and duplicate-suspect groups. Its output named no new #3016/#3017/#3018 or D-CODE-INSPECT1/D-RANK-SELECT1 defect. This is not a globally clean board claim.
- The published report was read back byte-for-byte through Tower. The earlier same-day report remained unchanged.
- The new ledgers contain 70 validated rows; the combined matrix contains 141 topics from 174 prior-plus-new claim rows. Probe outputs passed independent correctness checks for nine variants, six sizes and two kernels. Product execution remains blocked by the fresh compiler-build error described above.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F01 Fresh compiler build | card | #3010 |
| F02 Complete maintainer algorithm/API inventory | card | #3011 |
| F03 Quantile and describe work | card | #3012 |
| F04 Map extrema and top_n semantics | card | #3013 |
| F05 Core reachability denominator | card | #2986 |
| F06 Canonical optional measurement projection | card | #3014 |
| F07 Compiler facts and invalidation costs | card | #3011 |
| F08 Exact machine-code inspection | card | #3016 |
| F09 Checked operand and clobber lowering | card | #3017 |
| F10 Selected-target asm behavior | card | #3017 |
| F11 Inline foreign native-mode meaning | card | #3017 |
| F12 Cranelift optimization evidence | card | #3011 |
| F13 Lightweight backend screen | card | #3011 |
| F14 Fair peer and whole-job matrix | card | #2858 |
| F15 Non-shipping rank prototype and later adoption | card | #3018 |
| F16 Batch quantile demand evidence | card | #3011 |
| F17 Deferred Query limit demand evidence | card | #3011 |
| F18 Independent AI/test oracles | card | #3011 |
| F19 Production and bootstrap qualification | card | #217 |
| F20 Stale complexity correctness and performance evidence | card | #3011 |
| F21 Beginner and expert inspection experience | card | #3016 |
| F22 Integrated behavior security and recovery proof | card | #2919 |
| F23 Reopening existing asm syntax | no-action | Existing D-FFI-ASM1 and D-FFI-ASMOPS1 already decide this; #3017 repairs implementation, not authority. |
| F24 Universal speed cultural and AI claims | no-action | Unsupported generalizations are rejected as conclusions; qualified workload hypotheses are homed on #3011. |
<!-- /audit-dispositions -->

**Strongest unverified assumption:** Jet can preserve enough algorithm, ownership and layout information through every applicable execution mode to turn these local opportunities into strict whole-job wins against competent peers. The current build failure and absent Jet/backend matrix leave that assumption unproved.
