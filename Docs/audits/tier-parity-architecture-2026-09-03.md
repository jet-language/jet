# Tier parity architecture — 2026-09-03

Owner question (verbatim intent): the execution tiers should be *one implementation at different optimization levels*. The interpreter is slowest and starts instantly, the JIT is the middle, AOT is slowest to start and fastest to run. Behavior must never differ between tiers, even during development. If that is not what ships, it is a separate, full-fledged issue.

This document answers that question with the binary, the source tree, and one ballot. It is written by the orchestrator; Luna ran the probes and the scouts, and every number below has a file path.

## Verdict

**The invariant is not what ships.** Jet has *one* front end (lexer, parser, sema, TIR) and *four* hand-written back ends that each re-implement the language's runtime semantics: AOT emits Rust (`crates/jet-codegen/src/Codegen/TIR/emit`, about 20k lines), the JIT lowers TIR to Cranelift (`crates/jet-jit/src/jit/lower_ctx.rs`, about 34k lines; crate about 91k), the interpreter tree-walks TIR (`crates/jet-codegen/src/Codegen/TIR/eval`, about 30k lines), and the web target has its own emitter (`Codegen/Web.rs`, about 13k). Prelude functions are registered three times: AOT prelude projection, the JIT `host_fns!` table (`crates/jet-jit/src/lib.rs:74-177`), and the interpreter ambient arm (`crates/jet-foundation/src/Syntax/core_calls.rs:421-455`, `crates/jet-jit/src/ambient_interp.rs`).

That shape *guarantees* divergence over time, and this run found five in twenty minutes of probing on top of a corpus gate that reports zero (`tests/jit_corpus_gate.txt`). The gate measures the corpus it was given; it does not measure the architecture.

What *is* true, and matters: the front end is shared, so **what a program means** (types, effects, diagnostics E0xxx) is the same everywhere. What differs is **whether a tier can execute it** and **how fast**. The owner's model holds for the checker and fails for the executors.

## Evidence

### Same program, three outcomes

| # | Program | AOT (`jet build`) | JIT (`jet run`) | Interpreter (`--interpret`) | Card |
|---|---|---|---|---|---|
| D1 | `fn max_of<T: Comparable>(l: T, r: T) T -> if l > r -> ~l else -> ~r`; `print(max_of(3, 7))` | ICE, exit 101 (emitter declares `&T` params, call site passes `3i64`; `main/build/g2.rs:70860-70870`) | prints `7` | prints `7` | #2878 |
| D2 | `struct Node { value: Int, prev: ?Node, next: ?Node }`, back-link write | ICE, exit 101 (`build/r2_ref.rs:70865-70876`) | prints `1`, `2` | prints `1`, `2` | #2879 |
| D3 | shipped `examples/features/operators/spaceship.jet` (`sort_by((l, r) -> l.compare(r))`) | ICE, exit 101 | E0956 "isn't supported" after two lines of output | E2201 after the same two lines | #2880 |
| D4 | `loop line in io.stdin().lines()` echo until blank | correct | correct | E2201 "uses `handle`"; text blames `jet dev` | #2881 |
| D5 | `jet new demo` then `jet build --target web` | — | — | web emitter: E-WEB-TIR-UNSUPPORTED on the scaffold's own `run` | #2882 |
| R6 | same generic as D1 with a second `I64` call | ICE | E0956 | E2201 | #2878 |
| S7 | `para_map`/`para_fold` at N=2,000 | correct | correct | E2201 | logged on #2888 |

Parity sweep: 19 of 20 shipped examples matched on stdout and exit across the three tiers (`loops/results.md` §T1). The one that did not is D3, a *shipped example*.

### Silent tier substitution

The JIT trace for `if_expression`, `range_step`, `spaceship`, and `lists` contains `tier0 interp`: the JIT quietly handed whole functions to the interpreter because a construct hit a gate (`ClosureMethod:SortBy` and others). The user typed `jet run` and got interpreter speed with no notice. This is "correct" under the current design (D-JIT1/2 permit deopt) and it is exactly the mechanism that hides the cost the owner wants to see. Logged on #2862.

### Performance is not a monotone ladder either

Owner model: interpreter < JIT < AOT, uniformly. Measured (`loops/results.md` §L2, six spellings of the same counter loop):

| spelling | AOT release, N=3e8 | JIT, N=3e6 | interpreter, N=3e6 |
|---|---|---|---|
| `loop {}` + break | 4.9 ms | 3.18 s | 64.0 s |
| `loop true {}` | 0.7 ms | 3.12 s | 74.0 s |
| `loop running {}` | 0.4 ms | 3.16 s | 80.9 s |
| `loop i < n {}` | 0.4 ms | 3.13 s | 56.5 s |
| `loop j := 0, j < n {}` | 0.5 ms | 3.58 s | 87.8 s |
| `loop k in 0..n {}` | 0.6 ms | 2.75 s | 35.8 s |

AOT collapses all six to the same machine code (rustc folds them). The interpreter shows a **2.5x spread across spellings of the same loop**, so on the dev tier the spelling matters and on the release tier it does not. That is a tier-dependent behavior in the owner's sense even though every row prints the same number. The JIT is about 1 µs per iteration on a counter loop (boxed host calls; #2863). Card #2886 fixes the spread at the one place all tiers share: fold constant conditions and canonicalize loop forms in TIR.

### Optimization below AOT: there is none

The owner asked whether inlining and the other optimizations are accounted for on the JIT and interpreter tiers, not only AOT. The answer from the source tree is that Jet performs **no optimization of its own on any tier**, and the lower two tiers receive none from anyone.

- `#Inline` and `#Inline(Always)` are sema-checked contracts (E0917–E0919 bound recursion, address-taking, and body size) that lower to a Rust `#[inline]`/`#[inline(always)]` attribute in the emitter (`crates/jet-codegen/src/Codegen/TIR/emit/functions.rs:807-813`). Nothing else consumes the marker: `crates/jet-jit/src` has no reference to the inline contract, and the interpreter evaluates the call as written.
- The resident JIT is constructed with `JITBuilder::new(cranelift_module::default_libcall_names())` (`crates/jet-jit/src/jit/runtime_host.rs:3807`), which takes Cranelift's default flags; the default `opt_level` is `none`. The debug-object path sets `opt_level none` explicitly (`crates/jet-jit/src/jit/api_debug.rs:74-77`). No code path sets `speed`.
- The only pass TIR lowering applies before handing off to tiers, `canonicalize_pre_tier_expr` (`crates/jet-codegen/src/Codegen/TIR/lower/expressions.rs:1103-1111`), erases user type tags. It folds no constants and canonicalizes no control flow.
- Consequently the spelling table above is explained entirely by rustc: it folds `while true`, hoists the loop-invariant `running`, and strength-reduces the counter; Jet did none of that, so the interpreter, which sees Jet's TIR as lowered, pays for each spelling's literal shape.

This is a second, independent way the owner's model fails. Even with one shared lowering (option A below), tiers would be "the same program at different optimization levels" only if Jet owns at least the optimizations that change what the lower tiers execute. Card #2892 proposes a `TIR/opt` module run once after lowering (constant-condition folding, loop-form canonicalization, `#Inline(Always)` expansion, dead-branch removal) so every back end consumes the same optimized TIR, plus explicit Cranelift flags for the JIT with the chosen `opt_level` justified by a corpus-gate run. Under option A that module is the natural home for all Jet-owned optimization; under option C it is the cheapest way to make the interpreter and JIT stop paying for spelling.

### Why it happens (structural, not accidental)

1. **Four lowerings of semantics.** Each back end decides, on its own, what `sort_by` with a comparator means, how a generic parameter is passed, how a `?Node` back-link is written. Three of the five defects above are exactly "back end X made a different call than back end Y".
2. **Three registration tables for Prelude.** A Core function exists only where someone added it. `ambient_interp.rs:2487-2491, 2842-2845, 5422-5425` are comments recording features that ran on AOT and JIT and failed on the interpreter until an arm was added. That is the failure mode, written down by the people who hit it.
3. **The AOT back end is a Rust source emitter.** Its "ICE" is rustc rejecting emitted Rust (`&T` vs value, moved `Box`). No other tier has that failure class, so no other tier can share the fix.
4. **Deopt is per-function and silent.** D-JIT1/2 allow it; nothing surfaces it in ordinary output.
5. **The gate is a corpus, not an invariant.** `tests/jit_corpus_gate.txt` counts frontend-rejected, gate-excluded, expected-exit, AOT-broken, resident-JIT, deopt-interpreter. "AOT-broken" is an accepted category. A gate that has an "AOT-broken" bucket is documenting divergence, not preventing it.

### What already points the right way

- D-DEVMODE1 (ratified): dev-runtime output must be byte-identical to release. The *law* is the owner's invariant. The implementation is four independent attempts to honor it.
- Shared front end through TIR means every tier sees the same typed program. The seam exists; it is just too high.
- `#Scalar`/vector kernels: the sema proof (`CheckerKernel.rs:52-320`) is shared, and only the AOT emitter consumes it. Lane types `F32x8`/`F64x4` produced identical output on all four paths (S6). When a feature is decided once in the front end and *consumed* per tier, parity holds; when it is *re-decided* per tier, it breaks.

## The owner's model, restated precisely

"One implementation, tiers differ only in optimization" means: there is one lowering from TIR to an executable form, and the tiers differ in **how that one form is executed**: interpreted, compiled lazily, compiled ahead. Any Core function, any language construct, any layout decision appears in that one form once and is therefore available everywhere the moment it lands. A tier may *refuse to optimize* a construct; it may never *refuse to execute* it, and it may never compute a different answer.

Under that model the interpreter is not a separate 30k-line semantics; it is the reference executor of the shared low-level form. The JIT is a compiler of that same form. AOT is the same compiler run to completion with the most expensive passes on.

## Enforcement: how one implementation stays one

The owner's second question is not "how do we get to one lowering" but "how do we make it impossible to leave." A migration that lands and then drifts back is worse than none. Four mechanisms, each mechanical, each failing the *compiler's* build rather than a user's program. They apply under option A in full and are worth landing under B or C as the ratchet.

### E1. A Core function is declared once; every tier's table is generated

Today a Prelude function has a `CoreCallRecord` in `core_calls.rs`, a `host_fns!` symbol in the JIT, an ambient arm in `ambient_interp.rs`, and a projection in the AOT prelude. Four hand-written places; a function reaches a tier when someone remembered it. The fix is a single declaration (the record) from which the other three are derived by a build script, and a compile-time assertion in each crate that its table has exactly the record set's rows. Adding a Core function then means editing one file; forgetting a tier becomes a build error in `crates/jet-jit` or `crates/jet-codegen`, not an E2201 in a user's terminal. The census that makes this safe to land is #2898: rows in `core_calls.rs` minus arms in `ambient_interp.rs` minus symbols in `host_fns!`.

### E2. Every TIR construct has one lowering or the compiler does not build

The four back ends each `match` over `TExprKind` and `TStmt`. Rust's exhaustiveness check already forces every arm to exist, but a `_ => refuse()` arm satisfies it, and that is exactly what produces E0956 and E2201. The enforcement is a rule (a proc-macro attribute or a test over the source) that no back-end `match` on a TIR variant may have a wildcard arm, plus a unit test that constructs one minimal program per variant and runs it through every back end asserting equal output. Under option A this collapses to one lowering with no wildcard; under C it is the ratchet that makes the `interpreter_refused` and `aot_broken` buckets of `tests/jit_corpus_gate.txt` shrink to zero and stay there (today the gate accepts both as categories, `tests/dev_parts/corpus_gate.rs:251-264`).

### E3. The differential test is the definition of correct

`tests/jit_corpus_gate.txt` today records per-example outcomes per tier and permits categories of divergence. Invert it: the gate holds no categories, only programs; each program's expected stdout and exit code are recorded once; every tier must produce them or CI fails. A tier that cannot run a program is a failure, not a row. Every example under `examples/`, every `tests/ui` positive case, every Core row from E1, and every safety allow/refuse pair from #2904 is in the corpus. New examples enter automatically (a test walks `examples/`), so the corpus grows with the language instead of being curated. This is the mechanism that would have caught D3 (a shipped example) the day it broke.

### E4. Deopt and refusal are visible, then forbidden

Today the JIT silently substitutes the interpreter (`tier plan gate failed`, visible only under `--trace-tiers`). Step one: `jet run` prints one line per deopted function by default (function, gate, tier), so a user who asked for the JIT and got the interpreter knows; this is a row in the decision ledger (#2899). Step two: once E2 and E3 hold, deopt has no legitimate reason except performance heuristics, and any deopt for *coverage* is a build failure of the compiler. E0956 and E2201 are then retired: there is no "construct the evaluator does not cover" because the evaluator covers the one lowering.

### What enforcement does not need

It does not need a proof that the four back ends are equivalent; it needs the back ends to stop being four. It does not need a per-feature parity checklist in every card; it needs the gate to be total. It does not need reviewers to remember; it needs the compiler's own build to refuse. `AGENTS.md` I9 today says every tier honors the same Prelude semantics. Under option A it should say: *every construct and every Core function is lowered exactly once; a back end that cannot consume the lowering fails the compiler's build, never the user's program.* That sentence is the owner gate; the mechanisms above are how it is kept.

## Options (ballot D-TIER-ONEIR1, card #2888)

**A. One low-level IR, lowered once from TIR; every back end is a dumb translator of it, including the Rust emitter.** Introduce a Jet MIR (or bytecode). TIR → MIR happens exactly once and owns all semantic decisions: generics, layout, closures, Prelude binding. The interpreter executes MIR. Cranelift compiles MIR lazily (`jet run`) and, for the dev profile, eagerly. The release `jet build` path stays MIR → Rust → rustc/LLVM, but the Rust emitter becomes a mechanical printer of MIR with no semantic decisions of its own; rustc keeps its two jobs, the LLVM optimizer and the independent memory-safety witness (a wrong lowering fails `rustc`'s borrow checker, which is exactly the trust argument for transpiling). Prelude registers once against MIR. Cost: a multi-quarter rewrite of about 100k lines of back end; a transitional period where the old emitter and the new path coexist behind the gate. Benefit: the invariant becomes structural, today's release performance is untouched, and the safety witness is kept. It is the only option under which "implement once, available everywhere" is literally true. (An earlier draft of this paragraph retired the Rust emitter; the filed ballot never did, and the owner's 2026-09-03 follow-up confirms the emitter stays as the release path and the safety witness.)

**B. Keep Rust-emission AOT; JIT and interpreter share one lowering.** Merge the interpreter into the JIT crate as the tier-0 executor of Cranelift's input (or of a small bytecode the JIT already produces). Prelude registration drops from three tables to two. Cost: months, not quarters. Benefit: kills the D4/S7-class defects (interpreter-only refusals). Does not touch the D1/D2/D3 class (AOT emitter disagreeing with everyone else), which is the P0 class.

**C. Status quo plus a parity ratchet.** Every shipped example, every `tests/ui` positive case, and every Prelude function gets a three-tier fixture; the "AOT-broken" bucket is forbidden to grow; CI fails on any new divergence. Cost: weeks. Benefit: stops regressions. Does not fix the architecture, so the 30k/34k/20k lines keep drifting, and every new feature still costs three or four implementations.

**Recommendation: A, staged; C immediately as the ratchet that protects the migration.** B is the tempting middle and it leaves the worst class untouched. The evidence that decides it: the P0 defects in this run are all AOT-emitter disagreements. Any plan that keeps a separate Rust-source emitter as *the* AOT path keeps the P0 class open forever.

Sequencing under A:

1. Ratchet (C) now: three-tier fixtures for `examples/` and Prelude; deopt becomes visible (`jet run` prints a one-line notice naming the function and the gate unless `--quiet`).
2. Define MIR from what the Cranelift lowering already needs; move generic instantiation, layout, and closure lowering out of the three back ends into TIR→MIR.
3. Interpreter becomes a MIR walker (delete `TIR/eval`); Prelude becomes one MIR-level table.
4. Rust emitter rewritten as a MIR printer (delete `TIR/emit`'s semantic arms); gated output-identical with the old emitter on the corpus, then the old emitter is deleted. Release `jet build` is now MIR → Rust → rustc, same speed as today, same borrow-checker witness.
5. Optional, later: Cranelift object emission from MIR (`cranelift-object` is already a dependency) as the *dev-profile* AOT so `jet build` without `--release` no longer waits on rustc. Never the release default until it beats rustc on the tracked cells (#2905 rule).
6. A direct LLVM back end from MIR is a separate, later decision. It is *not* tied to self-hosting: self-hosting changes which language the compiler is written in, not which back end it emits to. It would only be worth doing once the Rust bridge and the borrow-checker witness are no longer wanted, and #2905 says it must first win on the cells.

### The owner's three follow-up questions (2026-09-03), answered

| Question | Answer | Evidence |
|---|---|---|
| Is "Cranelift AOT" the same thing as Jet's JIT? | Same compiler library, different driver. The JIT calls Cranelift per function at first call with `opt_level = "none"` (`crates/jet-jit/src/jit/api_debug.rs:76`) and no Jet-side passes, which is why `jet run` loops cost about 1 µs/iteration (#2863). "Cranelift AOT" would call the same library once, whole program, at its `speed` level, through `cranelift-object` (already in `crates/jet-jit/Cargo.toml:31`). So the delta between today's JIT and a Cranelift AOT is roughly the delta between Cranelift unoptimized and Cranelift optimized, plus startup. | `api_debug.rs:76`, `Cargo.toml:21-31`, #2863 |
| How much slower would Cranelift AOT be than rustc/LLVM AOT? | Published range: about 14% slower on the Wasm suites Cranelift itself reports; rustc's own Cranelift back end reports parity-to-faster only against LLVM *debug* builds and slower than LLVM release; the 2020 academic number was up to 2x on some kernels. Cranelift has no autovectorizer and no LTO, so the loss concentrates exactly where this run found Jet's AOT wins today: the `vmulpd`/`vaddpd` packed arithmetic in the AoS particle kernel (#2889 disassembly). Not measured on Jet yet; it is a #2905 cell once a Cranelift AOT exists. | [cranelift.dev](https://cranelift.dev/), [bjorn3 2023 report](https://bjorn3.github.io/2023/07/29/progress-report-july-2023.html), [LWN 2024](https://lwn.net/Articles/964735/) |
| Is the LLVM back end the self-hosting step? | No, independent axis. Self-hosting = compiler written in Jet. LLVM back end = compiler emits LLVM IR instead of Rust text. Under option A the release path keeps rustc for as long as the memory-safety witness is wanted, whether or not the compiler is self-hosted. | option A, step 6 |

Best of all worlds under option A, stated as tiers of one MIR: interpreter walks MIR (instant, slowest); `jet run` compiles MIR with Cranelift lazily (fast start, middle speed; today's `opt_level none` should become `speed` once the Jet-side TIR/opt pass lands, #2892); `jet build --release` prints MIR as Rust and lets rustc/LLVM optimize and borrow-check it (slowest start, fastest binary, safety witness). Single source of truth is MIR; nothing below it may decide meaning.

## What "different optimization levels" would look like when done

| tier | starts | executes | optimizes |
|---|---|---|---|
| interpreter | immediately | MIR, directly | nothing (maybe constant folding already done in TIR) |
| JIT | after first call | MIR → Cranelift per function, lazily | Cranelift default; inlining across MIR; vector kernels where the sema proof says so |
| AOT | after full compile | MIR → Cranelift/LLVM whole program | everything, plus LTO, plus `target-cpu=native` under D-SIMD3=B |

Same MIR, same Prelude table, same generic instantiation. The interpreter can be slow; it cannot be *wrong* or *missing*.

## Where the owner's expectation already holds, and should be protected

- Checker diagnostics: E0110 (`loop 1`), E0003 (`do`), E0507 (mutate while iterating), E0905, E2104: identical on every path because they are emitted before any tier.
- Explicit lane types (S6), typed channels (R1: same sum on five paths), the index-arena graph (R2b), the C binding (R5): identical output across every tier that ran them.

## Strongest unverified assumption

That a Rust emitter reduced to a mechanical MIR printer produces Rust that rustc optimizes as well as today's hand-shaped emission (today's emitter chooses `Vec` vs slice, `&` vs move, and iterator shapes per construct; a printer would choose them once, in MIR). If it does not, the release tier loses speed without any Cranelift involved. The counterweight is that this run's AOT numbers are dominated by *Jet's own* emission choices (a `Vec` clone per elementwise loop, #2890; columnar 15x slower than AoS, #2889), not by rustc's optimizer, so the headroom Jet controls is larger than the headroom rustc adds. The second reader's independent runs confirmed both numbers (columnar 14.4x on release; interpreter 235 s at 20k elements).

## Files

- Probes: `~/.cache/jet-test-scratch/mine-20260903/{loops,build,simd,main}/results.md`, `main/g2.jet`, `main/build/g2.rs`
- Ballot as filed: `~/.cache/jet-luna/mine-20260903/ballots/D-TIER-ONEIR1.json`; card #2888
- Source seams: `crates/jet-codegen/src/Codegen/TIR/{emit,eval}`, `crates/jet-jit/src/jit/{lower_ctx.rs,tiers.rs,deopt.rs}`, `crates/jet-jit/src/lib.rs:74-177`, `crates/jet-jit/src/ambient_interp.rs`, `crates/jet-foundation/src/Syntax/core_calls.rs:421-455`, `Source/CmdCompile.rs:1603-1667`, `tests/jit_corpus_gate.txt`, `docs/spec/syntax-decisions.md` (D-JIT1/2, D-DEVMODE1)
