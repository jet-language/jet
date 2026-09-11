# Video mine: Rust vs C++, Python infinite loops, SIMD, C++ build systems — 2026-09-03

Four new videos from the owner's "Jet Research Queue" playlist, mined in full (auto-caption transcripts, 758 audience comments, linked primary sources), cross-checked against the live Jet binary with about 90 probe programs across AOT, `jet run`, and `jet run --interpret`, and confirmed by a fresh-context second reader for the three most surprising numbers. The owner asked four pointed questions on top of the standing lens; each is answered below under its own heading, and the tier question got its own document: `docs/audits/tier-parity-architecture-2026-09-03.md`.

## Verdict

The four videos are one argument told four ways: **a language is replaced when the thing you must reason about is the problem, not the tool.** The Rust-vs-C++ video's losses are all places where Rust makes you reason about the borrow checker instead of the trading system. The Python video's whole seven minutes is spent reasoning about how to spell "forever" and whether one spelling is slower. The SIMD video's point is that the *layout* you must reason about (AoS vs SoA) is a compiler-shaped concern leaking into source. The build-system video's conclusion is that CMake wins because everyone else makes you reason about the platform.

Jet's *design* answers all four better than the incumbents: one `loop` keyword with Bool-only conditions, a proof-based vectorizer with `#Scalar` opt-out and `#Layout(columnar)`, one manifest with strict keys and a sandbox, no GC. Jet's *binary* does not yet deliver three of the four: the tiers disagree (five programs, three outcomes), the columnar layout that the SIMD video recommends runs 15x slower than the layout it warns against, and a fresh `jet new` project cannot use a path dependency, build `--locked`, or target the web.

**Twenty-nine Tower cards minted (#2878–#2906), seven of them P0. Six full ballots filed (D-DO1, D-TIER-ONEIR1, D-PLACE1, D-FRED1, D-ACCEL1, D-LOOPREAD1); one standing-rule owner gate (#2905) and one research card (#2901) left for the owner. Evidence logged on eight existing cards. Section "Generalize every finding" maps every defect to one of seven shapes so the fix closes the shape, not the instance; five census cards (#2898, #2902, #2903, #2904, #2906) enumerate the other instances mechanically.**

### The owner's four questions, answered in one line each

1. *Would Jet be in the Rust-vs-C++ video's position?* Not on features (no borrow-checker prototyping wall: E0507 gives the fix; index arenas run on every tier), but yes on **exact placement and lock-free control**: no `size_of`, no `align_of`, no alignment rule, no atomics (#2887). Those four are the video's whole "why finance stays on C++" section.
2. *Should Jet add `do`?* No new keyword; ballot D-DO1 filed with the full argument. `loop { … if !cond { break } }` is the canonical body-first form, runs identically on all three tiers (L4), and the right investment is teaching text plus liveness lints (#2884), not syntax.
3. *Does the `while 1` performance trap exist in Jet?* Structurally impossible (`loop 1` is E0110). But the **spelling of a loop changes interpreter time by 2.5x** (35.8 s to 87.8 s for the same 3M iterations) while AOT folds every spelling to the same code. Card #2886: canonicalize once in TIR so no tier can see a spelling.
4. *Are the tiers one implementation at different optimization levels?* No. One front end, four hand-written back ends, three Prelude registration tables. See the tier document; ballot D-TIER-ONEIR1 recommends one shared low-level IR.

## The owner's specific comments, one answer each

The owner watched all four videos and attached seventeen pointed comments to the prompt. The four above are the headline ones; the rest are answered here in the order given, each with its evidence and where it landed. Items marked **gap** were not covered by the first draft of this report and are new in this revision.

### Video 1 — Rust vs C++

1. **Language features** (templates, specialization, inheritance, variadics, constexpr, overloading, default args, ABI, macros). The speaker never argues from these; the video's title says features and its evidence says defaults and control (Reframe 1). Where he does touch features, Jet's answer is measured: zero-cost generics exist and the AOT tier ICEs on the simplest scalar instance (#2878); `#Inline`/`#Inline(Always)` are checked contracts, stronger than C++'s hint. Not probed because not in the video: variadics, overloading, default arguments, stable ABI. Recorded in the ledger as `needs-measurement`.
2. **Memory safety** (borrow-checker limits, unsafe, self-referential and intrusive structures, arenas, graphs, interior mutability, lifetimes). Doubly linked nodes: the idiom users try first (`?Node` back-links) type-checks and crashes the AOT compiler (#2879); the idiom Jet recommends (index arena) runs on all tiers. Graphs: cyclic adjacency ran on all tiers. Mutation during iteration: E0507 names the fix in one line, which is the exact prototyping wall the speaker describes Rust building (R4, kept as a beat vector). No lifetime ceremony exists to complain about.
3. **UX/DX** (compile times, errors, learning curve, tooling, debugging). Warm build 0.39 s, edit 0.63 s, cold 21 s on the debug compiler; `jet test` 20.8 s cold. Error text is the strongest surface this run touched (E0507, E1206, E0110 all say what/why/fix). Debugging was not in the video and not probed.
4. **Concurrency** (threads, async, Send/Sync, colored functions, executors). Channels: same value on every tier, 0.20 s / 3.43 s / 18.65 s for 1M items. The video's concurrency argument is entirely about lock-free control, which Jet cannot express without `#Unsafe` (#2887). Async, colored functions, and executors were not in the video.
5. **Data structures and APIs** (containers, iterators, allocator APIs, std gaps, dynamic linking, plugins, C/C++ interop). C interop through `c@system` ran (`libc.strlen`, `libc.abs`). **Gap in the first draft, stated now:** Jet has no user-facing allocator API and no dynamic-linking or plugin story; the video names custom allocators as a first-class reason finance stays on C++ (cache-line pools, arena per frame). `Pool<T>`/`Id<T>` is the arena idiom but it is a library type, not an allocator seam. Folded into #2887's scope as the third leg (placement, atomics, allocation), not a separate card.
6. **Industry and ecosystem reasons** (hiring, codebases, standards bodies). Out of Jet's control and out of this report's scope; noted in the ledger as `out-of-scope` so the classification is explicit rather than absent.
7. **What the speaker says a replacement would need.** From the transcript, in his order: exact control of memory placement; lock-free primitives without ceremony; no fight when prototyping; interop with existing C++ (not just C); zero-cost generics that actually cost zero. Jet's scorecard on those five: no, no, yes, C only, ICE. Two of the five are #2887; one is #2878; C++ interop is not carded because no design exists to card against.

### Video 2 — Python's infinite loop

8. **`do` keyword.** No new keyword; D-DO1 filed. Answered above.
9. **`while 1` vs `while True` performance.** Structurally impossible in Jet (E0110), but spelling changes interpreter time 2.5x; #2886. Answered above.
10. **Inlining and other optimizations on JIT and interpreter, not just AOT.** **Gap in the first draft. The direct answer is no, none.** Every optimization a Jet program receives today happens after the AOT emitter hands Rust to rustc. `#Inline(Always)` lowers to a `#[inline(always)]` attribute (`emit/functions.rs:807-813`) and nothing else consumes the marker; the JIT crate does not reference it and the interpreter walks unmodified TIR. The resident JIT is built with `JITBuilder::new(default_libcall_names())` (`runtime_host.rs:3807`), Cranelift's default, which is `opt_level=none`; the debug object path sets `none` explicitly (`api_debug.rs:76`). The one pre-tier pass in TIR lowering, `canonicalize_pre_tier_expr`, erases user tags and folds nothing. So the lower two tiers receive zero optimization and the top tier receives rustc's, none of it Jet's. This is the mechanical reason behind #2886 (rustc folds `while true`, Jet does not) and #2863 (1 µs per JIT iteration). Card #2892: a Jet-owned TIR optimization pass (constant folding, loop canonicalization, contract-driven inlining) run once before any back end, plus explicit Cranelift flags. Added to the tier document as its own section.
11. **Tiers as optimization levels; the invariant.** Not met today; tier document and D-TIER-ONEIR1. Answered above.
12. **Python UX/DX wins, reason > read > write, friction proportional to frequency.** **Gap in the first draft.** The one frequency-weighted friction this video exposes is the read-until-sentinel loop. Python's walrus form `while (line := input()) != "":` is one line; Jet's state loop `loop line := io.readline(), line != ""` type-checks on every tier but evaluates its initializer once (probe L6), so the honest form is three statements (`loop { line := io.readline() ; if line == "" { break } ; … }`). The iterator form `loop line in io.stdin().lines()` is one line and is exactly right, and the interpreter refuses it (#2881). No lint tells the user their state-loop binding is loop-invariant. Card #2896 (ballot candidate D-LOOPREAD1): fix #2881 first, add the one-shot-loop lint, and record the "re-evaluating header" variant as rejected because one spelling with two meanings is the opposite of reasoning ease. Everything else Python offers here (`for…else`, `itertools`, sentinel objects) either did not appear in the video or is already covered by `loop … in` and `break`.

### Video 3 — SIMD

13. **Automatically enhance codegen for SIMD, with an expert opt-out.** Jet already has the shape the owner asked for: a proof-based vectorizer in sema (`CheckerKernel.rs`) that emits packed helpers, and `#Scalar` as the opt-out (D-SIMD3). It fires (S1 disassembly shows four zmm `vaddpd`) and its win is hidden by a list clone (#2890). The gap is coverage, not existence: see 15.
14. **Shallow copies to SoA when SIMD-shaped work runs over an array of structs.** **Gap in the first draft.** No pass transposes today; layout is a declared rule and the declared SoA layout is 15x slower than AoS (#2889). The owner's idea is sound under a cost gate (the transposition costs about N × touched-fields × 16 bytes, which the O(N²) gravity step amortizes trivially and a one-pass 20M add never does) and is unobservable, so it does not conflict with I9. Card #2893 (ballot candidate D-AUTOSOA1): options never / cost-gated AOT-only with an inspect line / on request via marker. Precondition: #2889, otherwise an automatic transposition lands on the slow accessors.
15. **Widen the autovectorization net; an optimizer phase intelligent enough to decide.** **Gap in the first draft as a card.** The proof accepts only flat `[Float]` elementwise loops with no calls, control, early exit, or aliasing. The four common shapes the video shows are all rejected: struct-field access (`p.x`), conditional accumulate (masked add), early-exit search (compare + movemask), and reductions (strict Float order, #2891). Jet's proof is the explainable alternative to the black box 25 comments complain about, and it currently covers the least common shape. Card #2895: accept each of the four with a named rule and make `jet inspect` say which rule accepted or which statement rejected. The "intelligent optimizer phase" the owner describes is this proof; it already runs in sema, ahead of every tier, which is the right place.
16. **Multithreading gains alongside SIMD, or when SIMD is impossible, by default.** **Gap in the first draft.** Parallelism is opt-in only (`para_*`, 64-item chunks, crossover logged on #1405). The proof's iteration-independence half is precisely the fact auto-parallel needs, and a loop rejected for SIMD (irregular control, calls) can still be thread-parallel if that half holds. Constraints the video and its audience name: spawn cost dwarfs small loops, bandwidth-bound loops gain nothing, and Float reductions change results under any reordering (so they wait on D-FRED1). Card #2894 (ballot candidate D-AUTOPAR1): cost-gated auto-parallel of proven-independent, non-reducing loops with `#Serial`/`#Scalar` opt-out and an inspect line; result must be bit-identical on every tier.

### Video 4 — C++ build systems

17. **Platform agnosticism like Zig.** Five requested targets: one compiler bug (web, #2882) and four E3302s that name rustc and rustup to the user (logged on #758/#1059). Jet has no Jet-owned target list. Answered in D5/D6.
18. **Keep the best of each build system, throw the worst, then improve on it.** **Gap in the first draft** (the table was in the harvest, not the report). Below, from the video's own evidence, what each system contributes and what Jet already does about it.

| System | Keep (why) | Throw (why) | Jet today |
|---|---|---|---|
| CMake | Three-line minimum; `FetchContent` fetch-build-link in one call; ubiquity | Generator layer (native scripts nobody reads); platform `if`s leaking into every real CMakeLists; policy version line carried forever | `package.jet` is 3 keys for a hello; `deps:` is the one-call fetch but path deps fail (#2883); no generator layer; no version-policy line, which is #2897's question |
| Make | Per-file control when you need it | Hand-maintained dependency lists; download scripts for missing libs; debug/release by directory convention | Receipts and the CAS store give incremental builds without rules; profiles are typed `build:` blocks |
| Meson | Two-line hello; fast to replicate | 20 lines of platform workarounds; wrap files as a second config language; docs that need web searches | One manifest language (Jet syntax); the sandbox removes the platform `if`s by construction; E1206 tells you the key you misspelled |
| Ninja | Speed; refusing to be a language | Writing it by hand | Jet has no user-facing low-level format; `jet inspect explain-build` is the readable view (blocked today by E1239, #2883) |
| Visual Studio | Play button for beginners | Hidden command line; IDE lock-in; per-platform rewrite | `jet run` is the play button and is the same command in CI |
| Bazel | Hermeticity; content-addressed cache | Install chain (Chocolatey → Bazelisk → Bazel); SHA in the module file by hand; cryptic Windows failures | Hermetic by sandbox and CAS; lock carries hashes for you; Windows sandbox (AppContainer) exists but was not probed this run |
| Jai/Odin/Zig | Compiler is the build tool; no download | Jai: build program can override core language behavior, so the same source can mean different things per environment | One binary; `package.jet` is data, not code, so it cannot redefine the language (I8); `settings:` are typed declarations, not scripts |
| Batch/shell | Nothing to learn for one platform | One file per platform | `jet build` is the one-line script and is already cross-platform in intent |

Improving on the best: the two things no system in the video does that Jet already has are a fail-closed sandbox and typed settings with unknown-key rejection. The two things Jet claims and does not deliver are cross-compilation (#758) and a path dependency that builds (#2883).

19. **Does the build system capture all metadata indefinitely, extensibly, with forward and backward compatibility?** **Gap in the first draft. Direct answer: it captures enough, it is extensible in the strict direction only, and it has no compatibility policy at all.** Five independent version counters exist and none is related to another by a rule: `package.jet` has a `jet:` compiler version but no schema version of its own (a future key is E1206 on every older compiler; an old manifest has nothing a newer compiler can branch on); `.jet/lock` is `version: 1` and `Lock.rs:1934-1938` rejects any other value with no migration; the store is `jet.store.v1` by magic prefix; build records are `jet.build-record/v1`; receipts are v2 on a separate counter; editions 2026–2028 govern language semantics. Strict rejection is the right greenfield default; at 1.0 every one of these becomes a promise, and none has a written policy or a fixture test. Card #2897 (owner gate): one policy that names which artifacts are user-authored versus tool-owned, what happens to each on a version mismatch (warn, migrate, regenerate, reject), and a fixture directory of version N−1 artifacts that the current binary must handle.
20. **How users change settings; shared shape across experience levels without fragmenting; the Jai trap.** **Gap in the first draft.** Jet has exactly one settings layer: `settings:` in `package.jet`, typed, declared by the package, overridable per invocation with `--set k=v` only for declared keys, and no user-level or machine-level config file (`jet config` is E2101). That is the anti-Jai position: there is no place to put an override that changes what the source means, so a file reads the same on every machine. The cost is that a beginner has one door and an expert has the same door; the video's "play button then graduate to CMake" ladder collapses to `jet run` then `package.jet`, which is the right collapse. What is missing is the middle rung the video praises in CMake presets: named profile sets a team can share (`build: { ci: …, dev: … }` exists as profiles; whether profiles can be composed or inherited was not probed). Not carded; the owner should say whether profile composition is wanted before anyone designs it.
21. **Jet as a software product: how do we sell it?** **Gap in the first draft.** Reframed as a product, the video's buyers have four complaints and Jet's pitch is one sentence each. *"I spent six months comparing build systems"* → there is one, it is the compiler, there is nothing to compare. *"Twenty lines of platform workarounds"* → the sandbox means your manifest has no platform branches; if it needs one, that is a Jet bug. *"Cryptic errors, poor docs, web searches"* → every diagnostic says what, why, fix (E1206 is the demo). *"It has to get out of the way so I can write code"* → `jet run` is the whole onboarding. The product is honest only when #2883 and #758 land; today a buyer who tries a path dependency or a cross-compile in their first hour bounces, and first-hour bounces are the ones that never come back.
22. **The simplification philosophy, applied to the build system and to the language at every level.** **Gap in the first draft** (the mapping was in the harvest, not the report). The speaker's thirteen points reduce to three tests; here is each with its Jet instance and where Jet fails its own test.

| Test | Build system | Language | Where Jet fails it today |
|---|---|---|---|
| Remove before adding; every feature earns its place | One manifest, no generator, no user config layer; `--set` only for declared keys | One `loop`, no `while`/`do`/`for` (E0003 teaches the migration); Bool-only conditions | The `do` request (D-DO1) is the live test of this rule; the report recommends refusing it |
| Do not build corrective machinery for a problem you introduced | The sandbox removes platform `if`s instead of adding a platform abstraction library on top of them | E0507's fix line instead of a borrow-checker escape hatch | Prelude registration tables (three per feature) are corrective machinery for having four back ends; D-TIER-ONEIR1 |
| Do not hide complexity to look sophisticated | `jet inspect explain-build` exists to show the graph, not hide it | Vectorization is a proof with a verdict, not a black box | The verdict is not surfaced (`jet inspect` has no vectorize view, #2895); JIT deopt to the interpreter is silent (#2862) |

The one place the philosophy cuts against this report: three of the six new cards (#2893, #2894, #2896) propose adding something. Each is filed as a ballot candidate rather than a task for exactly that reason; the owner decides whether they earn their place.

## Sources and capture quality

| # | Video | Channel | Length | Captions | Comments | Linked sources | Ledger |
|---|---|---|---|---|---|---|---|
| 1 | Why Rust Can't Replace C++ | ForrestKnight | 16:05 | auto | 352 | Citadel finance talk, rust-lang #32838, cuTile paper, Unity docs, Sutter trip report (all retrieved) | 55 claims |
[…83ln elided…]

## Generalize every finding: the shape, not the instance

The owner's rule for this section: do not fix a similarly shaped issue in one place and leave the same shape unaddressed elsewhere. Every defect and gap in this report is an instance of one of seven shapes. For each shape the table names the instance this run found, the mechanism that produced it, the other places in Jet where the same mechanism exists and therefore the same shape is predicted, and the one structural fix that closes the shape rather than the instance. Where a sweep is needed to enumerate the other instances, it is carded as a census, not as a guess.

| # | Shape | Instance this run | Mechanism | Same mechanism, predicted instances (unprobed) | Structural fix | Card |
|---|---|---|---|---|---|---|
| S1 | **A back end decides meaning the front end already decided.** | Generic scalar parameter passed by value at the call and by reference in the signature (#2878); `?Node` back-link moved then written (#2879); `sort_by` comparator wrapped in `?` (#2880) | Four lowerings each re-derive calling convention, ownership, and closure shape from TIR instead of consuming one decision | Every TIR construct with more than one lowering arm: closures capturing by `&`/`^`, `View<T>` returns, `Shared<T>` guards, `freeze` results, `?T`/`!E` carriers across calls, `#Layout(columnar)` field access, tagged types, `InlineRange` erasure. The corpus gate's own `aot_broken` bucket has six entries of this shape. Outside the back ends: formatter, language server, and canvas each walk the AST separately | One lowering that owns these decisions (D-TIER-ONEIR1 option A); until then a **construct census**: enumerate every `TExprKind`/`TStmt` variant, count how many back ends handle it, run the three-tier differential on a minimal program per variant; the same census for fmt/LSP/canvas | #2888, **#2898**, **#2902** |
| S2 | **A Core function exists only where someone registered it.** | `io.stdin().lines()` refused by the interpreter (#2881); `para_fold` refused by the interpreter (S7 probe); three comments in `ambient_interp.rs` recording prior instances | Three registration tables (AOT prelude projection, JIT `host_fns!`, interpreter ambient arm) with no check that a row exists in all three | Every `CoreCallRecord` whose `route` is not `Pure`/`Typed`: the interpreter reaches it only through an ambient arm. Also every run-time safety check (effect denial, authority hold, sentry, sandbox): enforced where someone wired it | One registration that generates all three (tier doc, E1), plus a build-time check that fails the *compiler's* build when a row is missing a tier; a safety-parity census with allow/refuse pairs per tier | **#2898**, **#2904** |
| S3 | **A ratified surface is slower than the thing it replaces.** | `#Layout(columnar)` 15x slower than AoS (#2889); `para_map` 2.3x slower than serial at 2M cheap elements (#1405); the elementwise loop the vectorizer accepts clones its list (#2890) | The surface was proven to *work* (output identical) and never proven to *win*; the budget law gates regressions on tracked cells, not new surfaces | Every ratified performance-motivated surface: `#Inline(Always)`, `freeze`, `View<T>`, `#Static`/`#Inline` constants, `Pool<T>/Id<T>` vs `[T]`, `channel` vs `Shared<T>`, `[T#N]` vs `[T]`, string views from `trim`/`after` | Every performance-motivated decision gets a **two-program gauntlet cell** (surface vs the plain spelling it claims to beat) at ratification, and "slower than plain" is a budget Fail. Standing rule; owner gate | **#2905** |
| S4 | **The spelling changes the cost but not the meaning.** | Six loop spellings, 2.5x interpreter spread (#2886); Python's `while 1` lesson | No canonicalization pass; each spelling reaches the back ends as written | Every sugar with a canonical form: `if`/`else` vs `match`, `??` vs `match` on `?T`, `x += 1` vs `x = x + 1`, interpolation vs `concat`, `loop x in list` vs indexed, `~x` vs implicit copy at a `Copy` type, `.map(f)` vs `loop`+push, `-> expr` vs block body | The shared `TIR/opt` pass (#2892) canonicalizes every sugar before any back end; a **spelling-cost test** runs each pair on `--interpret` and asserts within 10% (census column in #2898) | #2886, #2892, **#2898** |
| S5 | **A diagnostic points at the wrong thing or names the wrong tool.** | E2201 blames `jet dev` for a `--interpret` run (#2881); E2104 explains a generics bug in budget vocabulary (#2878); E3302 sends the user to rustup (#758); E1250 says run `jet update jet` for a lock the tool wrote (#2883); L0520 spans an empty line (#2885); L2510's three fix texts are not edits (#2325) | Diagnostic text is written at the emit site in the emit site's vocabulary; no test asserts the fix is an executable edit or that the command named is the one that ran | Every diagnostic whose fix is prose; every diagnostic emitted below tier selection (they know which command ran and rarely say it) | An **actionability lint over the diagnostic registry**: every Fix contains a code span, a command, or a path; every tier/toolchain diagnostic interpolates the invoking command. Census once, then CI | **#2906** |
| S6 | **Silence where a verdict is owed.** | JIT deopt with no notice (#2862); no unreachable/always-true lints (#2884); no vectorization verdict in `jet inspect` (#2261 log); no loop-invariant header lint (#2896); the `Vec<f64>` clone in #2890 visible only in emitted Rust | The compiler knows a fact and has no channel obligated to say it | Every decision the user would change code over: tier ran, functions inlined, loops vectorized or parallelized, copies inserted, bounds checks elided, allocations hoisted | One **decision ledger** per compile; `jet inspect decisions FILE` prints it, `-v` summarizes; lints are the rows that imply an edit | **#2899** |
| S7 | **The first-hour path is broken while the deep path works.** | `jet new` then path dep (#2883); scaffold `--target web` (#2882); scaffold `--locked` (#2883); first `?Node` (#2879); first `max_of(3, 7)` (#2878); shipped example fails (#2880) | Fixtures exercise features in isolation from the scaffold; nothing walks the beginner's sequence; nothing measures which constructs have *no* example (I5) | Every `jet new` follow-on in hour one: add a dep, a test, a second file, `--release`, `--target web`, `jet fmt`, `jet dev`, a self-referential struct, a generic helper. Every construct and Core row with zero examples | A **first-hour script** in CI that runs the beginner sequence end to end on every tier; an **example-coverage census** joining constructs and Core rows against what `examples/` exercises | **#2900**, **#2903** |

Three things follow. First, four of the seven shapes (S1, S2, S4, S6) share the root the tier document names: semantics and decisions live in four places. The one-lowering ballot is not one card among twenty-nine; it is the fix for more than half the table. Second, the five census cards are cheap and mechanical, each mints its own follow-ups, and together they are how this audit stops being a sample of ninety hand-written probes. Third, S3 and S7 are process rules, not code ("prove the surface wins when you ratify it"; "walk the beginner path when you change the scaffold"); #2905 proposes the first as an `AGENTS.md` invariant, owner gate, and #2900 makes the second a CI test that needs no rule.

One gap the shapes do not cover is carded separately: the C++ video's replacement bar includes interop with existing C++ (not C), and Jet has no design for it (#2901, research).

## Corrections and disputed claims

| Claim | Status | Evidence |
|---|---|---|
| "XMM/YMM/ZMM are three register sets" (video 3, 05:13) | corrected by audience; adopted | overlapping architectural portions of the same registers |
| "Audited unsafe Rust is C++ code that Rust fights" (video 1, 07:22) | disputed; audience correction stands | explicit `unsafe` boundaries are the intended mechanism; the *cost* claim (allocator API unstable a decade) is confirmed by rust-lang #32838 |
| "Rust GEMM at 96% of cuBLAS" (video 1, 10:18) | confirmed | cuTile paper abstract, B200 |
| "Profiles slipped to C++29" (video 1, 14:56) | confirmed | Sutter March 2026 report |
| "Unity's scripting layer is C" (auto caption) | caption error | Unity docs: C# scripting over a C++ engine |
| "`while 1` was optimized in Python 2" (video 2, 01:35) | unverified by the speaker ("quick Google"); irrelevant to Jet | `loop 1` is E0110 |
| "Human SIMD beats autovectorization" (video 3 audience) | opinion; Jet's own data says the opposite for simple kernels | S8: proof-based vectorizer emits zmm `vaddpd` without user intervention |
| "Zig build works on any platform" (video 4 audience) | partially; a reply notes breaking changes hit its C/C++ build path | logged as context for #758 |

## Jet alignment — shipped versus ratified-and-unbuilt

| Topic | Ratified | Shipped and proven live | Gap |
|---|---|---|---|
| Loop forms | one `loop` keyword, E0376 retires 3-slot header | `loop {}`, `loop cond`, `loop x := v, cond`, `loop x in`; E0110 Bool-only; E0003 for `while/for/do/repeat/until` | no liveness lints (#2884); interpreter spelling spread (#2886) |
| Tier parity | D-DEVMODE1 byte-identical output | shared front end; 19/20 examples match | five divergences; four back ends (#2888, D-TIER-ONEIR1) |
| SIMD | D-SIMD3=B native target-cpu, `#Scalar`, lane types | lane types identical on four paths; proof vectorizer emits zmm adds | columnar slow (#2889); clone per loop (#2890); no reductions (#2891); no inspect verdict |
| Parallel | explicit `para_*`, 64-item chunks | AOT/JIT match; parallel N=2M costs 27 ms vs serial 11.6 ms | interpreter E2201 on `para_map` (tier doc) |
| Layout/placement | `#Layout(c)`, `#Layout(columnar)`, D-MEM1 | `#Layout(c)` on all tiers | no `size_of`/`align_of`/alignment rule/atomics (#2887) |
| Build | strict manifest, typed settings, lock v1, CAS store, sandbox | E1206 text is good; warm build 0.39 s; `--set` on declared setting | path deps, `--locked`, `--offline`, web target (D5, D7) |
| Cross-compile | cards #758/#1058/#1059/#1060 | — | E3302 leaks rustc/rustup |
| Generics | #2520 instantiation | check passes | AOT ICE, E2104 copy, `Decimal` literal default (#2878) |

## Avoid list

From the four videos, things Jet should keep refusing, with the reason stated once:

- Truthy conditions (Python, C): the whole `while 1` discussion exists only because they exist. Keep E0110.
- A second loop keyword for body-first loops: no language that added `do … while` uses it for more than macro hygiene and menus; the audience of the video that discusses it asks for it zero times (D-DO1 option A).
- A generator layer between the manifest and the toolchain (CMake, Meson, Premake): every one of them in video 4 still needs platform lines. One binary, one manifest.
- Per-platform build scripts as the "simple" answer (Cakez, 34:05): 25 lines per platform is 25 lines the reader must reason about per platform. The compiler owns the platform.
- Option sprawl in the manifest (Cakez, 27:00): E1206's strict keys are the mechanism; do not loosen them for convenience.
- Hand-written SIMD as the default advice (video 3 audience): Jet's proof-based kernel must get wider before anyone reaches for lane types.
- Leaking the host toolchain's vocabulary (E3302 "rustup target add"): the user did not choose rustc.

## Beat vectors — ranked, shipped versus unbuilt

| # | Vector | Jet mechanism | Status | Incumbent's weakness it beats |
|---|---|---|---|---|
| 1 | Vectorization as a visible verdict: `jet inspect` says why a loop did or did not vectorize | sema proof already exists (`CheckerKernel.rs:52-320`) | **unbuilt** (no inspect entry, S9) | 25 audience comments call autovec a black box; no compiler shows the proof |
| 2 | Layout is a rule, not a rewrite: `#Layout(columnar)` on the struct, code unchanged | shipped | **currently a loss**: 15x slower (#2889) | C++/Rust require rewriting every access site |
| 3 | One loop keyword, Bool-only, no spelling faster than another | shipped syntax; E0110 | half: AOT yes, interpreter no (#2886) | Python's `while 1` folklore |
| 4 | Mutate-while-iterating is a diagnostic with a fix, not a borrow-checker wall | E0507 "collect into a second list, or loop over indices" | shipped | Rust prototyping friction (video 1, 12:41) |
| 5 | Exact placement without `unsafe`: `size_of`, `align_of`, alignment rule, atomic values | none | **unbuilt** (#2887, owner gate) | C++ needs `placement new` and raw pointers; Rust needs `unsafe` |
| 6 | Same program, same answer, every tier | D-DEVMODE1 | **currently a loss** (five divergences) | nothing; Jet must fix its own gap first |
| 7 | Fresh project → dependency → `--locked` build in one file, sandboxed | manifest + lock + CAS + sandbox | **currently a loss** (D7) | Cargo's `build.rs`; CMake's generator layer |
| 8 | Defined-order parallel Float reduction that is also vectorizable | none | ballot candidate (#2891) | every language leaves this to the user |

## Agent-optimality — the five quantities

| Q | This mine's evidence | Jet position |
|---|---|---|
| a Verdict fidelity | D1/D2/D3 are wrong verdicts (check passes, build crashes); silent JIT deopt is a hidden verdict | weakest quantity; all carded |
| b Verdict latency | JIT hello 1.33 s on a debug compiler; `jet new` cold build 21 s, warm 0.39 s; `jet test` cold 20.8 s | cold paths are the cost; warm is good |
| c Verdict actionability | E2201 blames `jet dev` for a `--interpret` run; E2104 talks budgets for a generics bug; E3302 sends the user to rustup; E1250 says "run `jet update jet`" for a lock the tool wrote | #2881, #2878, #758, #2883 |
| d Context economy | every defect isolated by a program under 12 lines | strong |
| e Repair determinism | every defect maps to one seam with line numbers; the tier class maps to one architectural decision | strong for defects; the architecture needs the ballot |

This mine moves **a** and **c**, and adds one item to **b** that is not about speed: an agent cannot tell from `jet run` output whether it ran the JIT or the interpreter.

## Surface coverage

**Covered with proof** (ran live): `loop {}`, `loop cond {}`, `loop x := v, cond {}`, `loop x in 0..n`, `loop (i, e) in list`, `break`, E0110, E0003 (`do`, `repeat`, `until`, `while`, `break if`), E0107, E0118, E0376 (by reading), L0101, L0520, L2510, L0507; `io.readline()`, `io.stdin().lines()`; `[Float]`, `[Float#8]`, `F32x8`, `F64x4`, `#Scalar`, `#Layout(c)`, `#Layout(columnar)`, manual SoA, `para_map`, `para_fold`, `map`, `fold`; `?Node` recursive fields, index arena in `[Node]`, cyclic adjacency; `channel<T>`, producer/consumer; `c@system` binding, `libc.strlen`, `libc.abs`; `fn max_of<T: Comparable>`, E0109, E0905, E2104; `jet new`, `jet build [--release|--locked|--offline|--target …|--set k=v|-v]`, `jet run [--interpret|--release]`, `jet test`, `jet check`, `jet fetch`, `jet emit --rust`, `jet inspect explain-build`, `jet self doctor`, `jet config` (E2101), E1206, E0302, E0603, E1239, E1250, E3302, E-WEB-TIR-UNSUPPORTED, E0956, E2201.

**Worth checking**: `#Layout(columnar)` on a struct with mixed field widths (the probe used five `Float` columns); `jet inspect` on a vector kernel once #2889 lands; `Shared<T>` under `para_*`; `freeze` interaction with columnar lists; `--target web` on a program with no `run` I/O; registry-backed (non-path) dependency through `--locked`; whether `jet run` prints anything on deopt with `-v`.

**Missing**: `size_of`, `align_of`, an alignment layout rule, an atomic value type (#2887); `jet inspect vectorize` or equivalent verdict; automatic AoS→SoA (by design: layout is a declared rule, #2889 must make the rule worth declaring); unreachable/always-true/never-reassigned loop lints (#2884); a defined-order Float reduction (#2891); a Jet-owned target list (#758 family).

## Owner gates — ballots filed

| Decision | Card | Question | Recommendation | Dissent recorded |
|---|---|---|---|---|
| D-DO1 | #2884 | Should Jet add a body-first loop keyword (`do`)? | **A**: no new keyword; canonical `loop { … if !cond { break } }`; E0003 for `do` shows that shape; liveness lints | Beginner pass (fresh RLI5) wanted the trailing-test form for readability; adversarial pass (OpenAI family) accepted A and pushed the lint list to be concrete. A's unavoidable loss: the exit intent is not visible in the header, and no formatter can put it there |
| D-TIER-ONEIR1 | #2888 | One low-level IR lowered once, or four back ends with a parity ratchet? | **A**, staged, with **C** as the immediate ratchet. A keeps rustc/LLVM as the release back end: the Rust emitter becomes a mechanical printer of the shared MIR, so release speed and the borrow-checker safety witness are untouched | Adversarial pass argued a Cranelift-only AOT would not match rustc steady-state; A was rewritten to keep rustc, and the owner's 2026-09-03 follow-up confirmed that reading. Remaining unverified assumption: a MIR printer yields Rust that rustc optimizes as well as today's hand-shaped emission |

Both ballots ran base, boil-the-ocean, hybrid, cooperative, fresh RLI5 beginner, and rival-family adversarial passes. No decision was ratified by the agent. #2887 (placement/atomics) and #2891 (Float reduction order) are marked as ballot candidates and left for the owner to promote.

## Tower

| Card | P | Title |
|---|---|---|
| #2878 | P0 | Generic function with scalar read parameters: AOT ICE while jet run and --interpret print the answer |
| #2879 | P0 | Struct with optional self-typed fields type-checks, then AOT ICEs on a back-link write |
| #2880 | P0 | Shipped example spaceship.jet: AOT ICE, E0956 on jet run, E2201 on --interpret |
| #2881 | P1 | Interpreter refuses `loop line in io.stdin().lines()` while AOT and jet run echo the lines |
| #2882 | P1 | `jet new` scaffold cannot build for the web target |
| #2883 | P0 | Path dependency, `--locked`, and `--offline` all fail on a fresh `jet new` project |
| #2884 | P2 | Loop liveness lints and the foreign `do` teaching text (ballot D-DO1) |
| #2885 | P2 | L0520 fires on `io.readline()` with its span on an empty line |
| #2886 | P1 | Canonicalize loop forms and fold constant conditions once in TIR (interpreter 2.5x spread) |
| #2887 | P1 | Exact placement and lock-free control without #Unsafe: size_of/align_of, alignment rule, atomics |
| #2888 | P0 | Execution tiers: one implementation or four? (audit + ballot D-TIER-ONEIR1) |
| #2889 | P1 | #Layout(columnar) particles run 15x slower than AoS and lose packed arithmetic |
| #2890 | P1 | Elementwise list loop copies the list: 473 MiB for 20M adds; interpreter super-linear (20k elements: 235 s) |
| #2891 | P2 | Fixed-width pairwise Float reduction with one defined order (ballot candidate D-FRED1) |
| #2892 | P1 | Optimization exists only in the Rust emitter: one shared TIR pass so inlining and folding reach JIT and interpreter |
| #2893 | P2 | Transient AoS→SoA for proven vector kernels, cost-gated, AOT-only, visible in inspect (ballot candidate D-AUTOSOA1) |
| #2894 | P2 | Default multithreading for proven-independent kernels alongside or instead of SIMD (ballot candidate D-AUTOPAR1) |
| #2895 | P1 | Widen the vector proof: struct-field access, conditional accumulate, early-exit search, reductions |
| #2896 | P2 | Read-until-sentinel loop is one line in Python, three in Jet (ballot candidate D-LOOPREAD1) |
| #2897 | P1 | Five unrelated version counters and no manifest schema version: declare the compatibility policy (owner gate) |
| #2898 | P0 | Tier census: every TIR construct and Core call row per back end, a three-tier differential program each, spelling-cost pairs |
| #2899 | P1 | `jet inspect decisions`: one ledger of every compiler decision a user would change code over (ballot candidate) |
| #2900 | P0 | First-hour script: the beginner sequence from `jet new` through dep, test, web, and a generic helper, on every tier, in CI |
| #2901 | P1 | Research: a C++ (not C) interop story; compare Zig, Swift, Carbon, cxx.rs; ballot D-CPPINTEROP1 or a recorded boundary |
| #2902 | P1 | Census: every syntax form round-trips through `jet fmt`, the language server, and the canvas projection |
| #2903 | P1 | Census: which TIR constructs and Core rows have no executable example under `examples/` (I5) |
| #2904 | P1 | Census: effect denials, authority holds, sentries, and sandbox enforced identically on all three tiers |
| #2905 | P1 | Standing rule: a performance-motivated surface must beat the plain spelling it replaces at ratification (owner gate) |
| #2906 | P1 | Diagnostic actionability census and CI check: every Fix names an edit, a command, or a path; tier diagnostics name the command run |


[Showing lines 1-84 and 168-300 of 300; 83 middle lines (11.2KB) elided. Use :301 to continue. Read artifact://636 for full output]