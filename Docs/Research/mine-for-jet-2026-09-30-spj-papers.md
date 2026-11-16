# Mine for Jet: Simon Peyton Jones papers (2026-09-30)

First written as a report-only run. Later the same day, at the owner's
request, the defects became Tower cards and every other item became a ballot.
The finding dispositions at the end name each card and ballot.

## Verdict

The papers mostly confirm choices Jet has already made: required signatures
and local inference, one diagnostics home, explicit rollback, an audited
unsafe gate, and a single-mechanism law. The most valuable finding is not an
idea to add but a defect the STM paper predicts.

- **`#Transact` can lose updates.** Composable Memory Transactions shows that
  an optimistic transaction must check at commit that nothing it read has
  changed, and re-run if something has. Jet's committed Shared transaction
  code copies a value at first touch without a lock, edits the copy, and
  publishes the copy at commit with no such check. Two concurrent transfers
  can therefore overwrite each other. The shipped `shared_transact` example
  printed wrong, varying totals (1000 to 1200; the golden is 1000) on the
  current `jet run` binary. The ratified D-CONC-STM1=A rests on the premise
  that "both designs are atomic", and the code does not support that premise.
  This needs an owner decision.
- **Pattern coverage is close to Lower Your Guards but has three holes.** A
  table with a guarded arm and no `else` fails to parse, while lint L0303
  tells the user to delete that `else`. An arm made impossible by an earlier
  test is not reported. The L0303 text shows doubled backticks.
- **The edit loop is stricter than GHC's.** A type error in a function that
  never runs blocks `jet run`. On the probed binary, `#Todo` as a function
  body is also rejected, which breaks the shipped `todo_stop` golden.
- **The optimizer has no automatic inliner.** The shared MIR optimizer expands
  only `#Inline(Always)`, and the JIT runs Cranelift at `opt_level=none`. The
  inliner thesis measures 26% left on the table by hand-tuned thresholds
  alone.

## Sources and capture limits

The source list came from the Tower idea `b0amz2jq` (read-only). All 27
sources were `new` in `Docs/spec/reference/prior-art.md`. The run used
focused reads: each paper's thesis, mechanism, design discussion, and
evaluation. Proofs and appendices were skipped. The "Read" column states
exactly what was covered.

| ID | Source | Venue | Read |
|---|---|---|---|
| verse | [The Verse Calculus](https://simon.peytonjones.org/verse-calculus/) | ICFP 2023 | §1–2, 3.6–3.9, 5, 6.1–6.2, 6.7, 7; App. F by search |
| lyg | [Lower Your Guards](https://simon.peytonjones.org/lower-your-guards/) | ICFP 2020 (JFP ext.) | §1–3.3, 4.1–4.2, 4.8–4.9, 5.2, 6, 7 (intro), 9 |
| diag | [Diagnosing type errors with class](https://simon.peytonjones.org/diagnosing-type-errors/) | PLDI 2015 | abstract, §1–2, §8 results |
| sherrloc | [SHErrLoc](https://simon.peytonjones.org/sherrloc/) | TOPLAS 2017 | abstract, §1 |
| defer | [Equality proofs and deferred type errors](https://simon.peytonjones.org/equality-proofs/) | ICFP 2012 | abstract, §1–2 |
| awkward | [Tackling the awkward squad](https://simon.peytonjones.org/Tackling-the-awkward-squad/) | lecture notes 2001 | §1, 2.6, 5.2–5.4, 8 |
| safehs | [Safe Haskell](https://simon.peytonjones.org/safe-haskell/) | Haskell 2012 | abstract, §1–3.3, §6 results, §7 excerpt |
| build | [Build systems à la carte](https://simon.peytonjones.org/build-systems-a-la-carte-theory-and-practice/) | JFP 2020 | §1, 2 (Table 1), 3.6, 8.1, 8.3, 8.8 |
| history | [A History of Haskell](https://simon.peytonjones.org/history-of-haskell/) | HOPL III 2007 | §3.1–3.8, 6.2 (opening), 13 |
| linear | [Linear Haskell](https://simon.peytonjones.org/linear-haskell/) | POPL 2018 | abstract, §1–2.1, 6.1, 8 |
| stm | [Composable memory transactions](https://www.microsoft.com/en-us/research/publication/composable-memory-transactions/) | PPoPP 2005 | abstract, §1–3.5 |
| beautiful | [Beautiful concurrency](https://www.microsoft.com/en-us/research/publication/beautiful-concurrency/) | Beautiful Code 2007 | introduction only |
| inliner | [Secrets of the GHC inliner](https://www.microsoft.com/en-us/research/publication/secrets-of-the-glasgow-haskell-compiler-inliner/) | JFP 2002 | **not read**: scanned PDF, no extractable text |
| hollenbeck | [Inlining in GHC (thesis)](https://simon.peytonjones.org/hollenbeck-inlining/) | Edinburgh PhD 2025 | abstract, lay summary |
| joinhs | [Join points in practice](https://simon.peytonjones.org/join-points-hs/) | Haskell Symposium 2025 keynote (slides) | slides 1–42 |
| cwc | [Compiling without continuations](https://simon.peytonjones.org/compiling-without-continuations/) | PLDI 2017 | abstract, §1–2 |
| tiacs | [Type inference as constraint solving](https://simon.peytonjones.org/type-inference/) | 2019 talk (slides) | slides 1–20 |
| secretstc | [Secrets of the GHC typechecker](https://simon.peytonjones.org/secrets-of-typechecker/) | 2023 talk | slides by keyword; **video not captured** |
| outsideinx | [OutsideIn(X)](https://simon.peytonjones.org/outsideinx/) | JFP 2011 | abstract, contents, §1 (opening), 6.5, 9.7 |
| letgen | [Let should not be generalised](https://simon.peytonjones.org/let-generalised/) | TLDI 2010 | abstract, §1–2.1, §4 |
| ttg | [Trees that grow](https://simon.peytonjones.org/trees-that-grow/) | JUCS 2017 | abstract, §1–2.1 |
| th | [Template meta-programming for Haskell](https://www.microsoft.com/en-us/research/publication/template-meta-programming-for-haskell/) | Haskell Workshop 2002 | abstract, §1–4 (partial) |
| kacc | [Kinds are calling conventions](https://simon.peytonjones.org/kinds-are-calling-conventions/) | ICFP 2020 | abstract, §1–2.1 |
| levity | [Levity polymorphism](https://simon.peytonjones.org/levity-polymorphism/) | PLDI 2017 | abstract, §1–2.1 |
| esdf | [Elastic sheet-defined functions](https://simon.peytonjones.org/elastic-sdfs/) | JFP 2020 | abstract, §1, §10 user study |
| champagne | [Champagne prototyping](https://www.cl.cam.ac.uk/~afb21/publications/VLHCC04.pdf) | VL/HCC 2004 | §1–4.1 (first 40%) |
| detpar | [A monad for deterministic parallelism](https://simon.peytonjones.org/deterministic-parallelism/) | Haskell 2011 | abstract, §1–2 |

Capture notes:

- Microsoft Research blocks `curl` (HTTP 403). Those PDFs were read through
  the page reader. The inliner PDF is a scan with no text layer, so it is
  unread.
- Champagne Prototyping has no PDF on its Microsoft Research page. It was
  taken from Alan Blackwell's Cambridge publication page.
- SPJ's site links the 2026 "Haskell: origins, evolution, and future" entry
  to the `/abs-den/` page, which is a site bug. That talk was out of scope.
- No audience or discussion lanes were run; the outcome did not need them.

## Probe environment

Live probes used `target/debug/jet`, built at 12:12 today, one minute after
HEAD `d382af63b`. The working tree has about 885 uncommitted changes from
other work, including `crates/jet-jit` and parts of the parser and sema.
`cargo build` currently fails on that tree (unused imports in
`jet-foundation`). A clean HEAD build was not possible: `target/` is at 96 GB
of the 120 GB cap, and rebuilding the shared binary would disrupt that work.
Each finding below says whether it rests on committed code, on the probed
binary, or on both.

## Verified defects

### 1. `#Transact` on `Shared` values can lose updates (critical; owner gate)

**Source.** The STM paper, §2.1 and §3.1. An optimistic transaction keeps a
log of what it read. At commit it validates that log and re-runs the block if
another commit changed a value it read. Without that check, a transaction
publishes results computed from stale reads.

**Committed code (HEAD, AOT Prelude path).**

- `edit_txn` stages a clone of the value at first touch and applies the edit
  to that clone (`crates/jet-codegen/src/Prelude/CoreLib/JetStd/MathTaskMem.rs`
  at HEAD, lines 1884–1918). Its commit hook publishes the clone and bumps
  the revision.
- `JetSharedTransaction::try_commit_with`
  (`crates/jet-codegen/src/Prelude/SharedProtocol.rs:873-960`) takes the
  participants' locks in canonical order and then runs deltas, stage hooks,
  and publish hooks. Nothing compares the revision a value had when it was
  staged with the revision at commit.

Two tasks can each copy `balance = 1000`, subtract 100, and publish `900`. One
debit is lost. A read-then-decide transaction (`if a.balance >= 100 { … }`)
cannot be protected by any delta trick.

**Live evidence (probed binary).** The shipped example
`Examples/features/memory/shared_transact.jet` runs eight concurrent transfers
of 100 from 1000. Its golden
(`Examples/features/expected/memory/shared_transact.out`) is `from=200`,
`to=800`, `total=1000`. Five `jet run` results:

```
from=500 to=700 total=1200
from=500 to=600 total=1100
from=600 to=400 total=1000
from=400 to=700 total=1100
from=600 to=500 total=1100
```

`--trace-tiers` shows every function on the Cranelift tier (`tier1 native`).
The same program behaves differently on each tier:

- `jet run --interpret` stops with E3001 at the first `join`.
- `jet build` fails with an internal compiler error (exit 101): `MIR nominal
  type "Stm" has no declaration row`.

A second probe (eight tasks doing "withdraw 100 if funds allow" in
`#Transact`, plus a shared counter) ICEs on `jet run` with `Shared
constructor received an invalid checked type ID`. The cause was not
isolated.

**Jet state.**

- D-CONC-STM1=A (ratified 2026-08-06) amended D-STM1 so that "the block body
  runs exactly once, and the commit takes locks in a fixed order".
- The ballot said "Both designs are atomic and deadlock-free." Commit-time
  locking makes the *publication* atomic. It does not make the transaction
  serializable, because the body ran against values that may be stale by
  commit.
- Tower #2123 (done) once fixed a wrong answer in this same example.

**Inference, not verified.** Earlier runs may have passed because the
scheduler happened to run the tasks one after another.

**What would fix it.** One of two designs, each an owner choice:

- Validate at commit that every staged participant's revision is unchanged,
  and re-run the body otherwise (D-CONC-STM1 option B, retry).
  - Log lines can still print once if prints inside `#Transact` are routed
    through `on_commit`.
  - Jet already bans irreversible effects in the block (E0746), which is the
    paper's precondition for re-running.
- Or acquire each participant's lock at first touch and hold it until commit.
  - This needs a deadlock answer, because the touch order is dynamic.

Either way, add a stress oracle to the example suite: N tasks by M transfers,
with the total conserved on AOT, `jet run`, and the interpreter.

### 2. Guarded arms need an `else`, and L0303 says to delete it (probed binary)

A value table whose guarded arm has no `else` fails to parse:

```jet
fn shape_text(s: Shape) -> String {
    if s == {
        .Rect(w, h) && w == h -> "square {w}"
        .Rect(w, h) -> "rect {w}x{h}"
        .Circle(radius) -> "circle {radius}"
    }
}
```

The result is two E0003 errors ("Expected a call, binding, assignment, or
`return`, found a piece of quoted text"). Adding `else -> "other"` makes the
file parse, but `jet check` then reports L0303: "This `else` can never run …
Fix: Delete the `else` arm". The checker already treats guard plus unguarded
arm as total, which is Lower Your Guards' rule. The parser does not. The
comment in `Examples/features/patterns/nested_payload_safety.jet:26-27` still
says guards "need an `else` fallback".

Tower #1440 (done) fixed a similar E0003-before-E0307 ordering. #3587
("Non-exhaustive value-returning enum dispatch passes jet check") is building
now, so this may be in-flight breakage. No card covers it.

### 3. L0303 renders doubled backticks

The L0303 text reads ``every case of ``Shape`` is already handled`` (two
backticks on each side). It also appears on `nested_payload_safety.jet:35`.
The template and its UI snapshot need one pair of backticks.

### 4. `#Todo` as a function body is rejected (probed binary; tracked)

The shipped golden `Examples/features/errors/todo_stop.jet`
(`fn missing() -> Int { #Todo }`) fails with E0355: "`#Todo` cannot attach at
the Statement site". Its golden
(`Examples/features/expected/errors/todo_stop.err.out`) expects the runtime
stop E3011. E0355's Fix ("Move `#Todo` to one of its registered sites") does
not name the working spelling (`return #Todo`, per
`Examples/features/tooling/todo_hole.jet:7`). #3065 ("Restore the complete
shipped example corpus on ordinary jet run", building) covers the regression.
The Fix text needs its own repair.

### 5. An unannotated parameter produces a misleading cascade

`fn twice(x) -> Int { x * 2 }` yields three errors:

- E0305 "Multi-head variant `x` is not an enum variant"
- E0107 "Nothing named `x` exists here"
- E0104 "`twice` expects 0 arguments, got 1"

None says "`x` needs a type". This is the SHErrLoc lesson in miniature: the
first failed rule (multi-head parsing) takes the blame instead of the likely
mistake. It is a beginner-path defect, with I4 what/why/fix quality at stake.

### 6. `jet run` hides warnings that `jet check` reports (tracked)

A duplicated `.Warm` arm runs silently under `jet run`. `jet check` reports
L0301 `unreachable_dispatch_arm`. #3420 ("Separate execution lint visibility
from diagnostic enforcement", building) covers this.

## Lessons by theme

Classes follow the claim ledger: already-implemented, ratified-in-progress,
real-gap, rejected-conflict, needs-measurement, owner-gate.

### Concurrency and transactions

- **Validate and re-run** (STM §2.1, 3.1): real gap. See defect 1.
- **`retry` and `orElse`** (STM §3.2–3.4): owner gate.
  - `retry` blocks a transaction until a value it read changes.
  - `orElse` tries a second transaction if the first retries.
  - Together they make blocking operations composable and remove lost
    wake-ups.
  - Jet's answer is D-SHAREDGUARD1=A: explicit `guard_edit`, `Condition`, and
    `notify_one`. That is correct but manual, and a forgotten `notify` is
    exactly the bug class `retry` removes.
  - A transaction-scoped wait, the `#Transact` twin of `guard.wait`, would
    need a ballot. It also depends on re-run semantics from defect 1.
- **No irrevocable effects inside a transaction** (STM §3.1): already
  implemented. E0746 rejects network, file, and subprocess effects in
  `#Transact`, and FFI calls need `#Undo` (`crates/jet-sema/src/Sema/Effects.rs:863-879`).
- **Transactional failure contexts** (Verse §7, App. F): rejected by design.
  - Verse rolls back store effects whenever an `if` condition fails.
  - Jet keeps rollback explicit in `#Transact`; ordinary `?` never rolls back.
  - The parked proposal #775 already records this idea.
- **Executable semantics tested at scale** (Verse §3.9, about 100 million
  QuickCheck runs): ratified, in progress. The matching Jet tool is a random
  program generator checked for equal results across tiers (#3860, ready).
- **Explicit granularity for deterministic parallelism** (monad-par §1–2):
  in progress. #2922 (building) records `para_map` running 2.3 times slower
  than serial with a fixed 64-item chunk floor. The paper says chunk size
  must be a cost-model fact, with an expert override.
- **Beautiful concurrency** was read only as far as its introduction, which
  repeats the STM paper's bank-account framing. It adds no separate claim.

### Pattern coverage

- **Guard trees** (Lower Your Guards §3): already implemented in substance.
  Jet's checker (`crates/jet-sema/src/Sema/CheckerCore/switches.rs`) does
  nested witness search, or-patterns, finite integer intervals, and
  guard-aware totality.
- **Long-distance facts** (§4.1): real gap. After
  `if t == .Warm -> return …`, a later table's `.Warm` arm is not reported as
  redundant. Omitting that arm is rejected with E0307. Jet already narrows
  optionals through flow facts (#746); the same facts could seed the coverage
  start set.
  - Reporting the redundant arm is uncontroversial.
  - Accepting the omission is a readability choice for the owner, since
    reasoning comes first.
- **Graceful degradation** (§5.2): LYG caps its internal case splits at 30 so
  the checker cannot blow up. Jet's matrix witness search should have an
  explicit cap and a stress test. Not verified here.
- **A way to keep deliberately unreachable debug arms** (§6,
  `considerAccessible`): not needed in Jet. `debug(value)` and `#Todo` cover
  the same use without dead arms.

### Type errors and the edit loop

- **Deferred type errors** (ICFP 2012): owner gate.
  - GHC can compile a module with type errors: each error becomes a warning
    and a runtime stop at the exact site, so the rest runs.
  - Jet refuses the whole program. E0113 in a function `run` never calls
    blocks `jet run` (exit 1).
  - A `jet dev`-only mode that lowers each isolated type error to an
    E3011-style stop would match `#Todo`'s existing contract. `jet build`
    and `jet test` would stay strict.
  - This changes product behavior, so it needs a ballot.
- **Typed holes** (2023 typechecker talk): ratified, in progress. `#Todo`
  exists (D-TOOL2) and reports the expected type at runtime; see defect 4.
- **Holistic error localization** (PLDI 2015, TOPLAS 2017): needs
  measurement.
  - Blaming the last failed constraint found the true error for GHC in 48%
    of one benchmark and 68% of a corpus of first-year student programs
    (Helium). A Bayesian ranking over satisfiable and unsatisfiable paths
    reached 88–89%.
  - Jet's required signatures keep inference local, which limits blame
    distance.
  - Inferred failure unions and effects (#3418, deciding) bring distance
    back.
  - Measure blame accuracy on seeded Jet mistakes before building a ranker.
    Defect 5 is a small instance of the problem.
  - SHErrLoc also suggests the smallest missing assumption. For Jet, an
    authority or effect denial should name the single smallest `allow`
    edit. Not verified here.
- **Generate constraints, then solve** (2019 and 2023 talks): already
  implemented where it matters.
  - GHC reports every type error from the final unsolved constraint, in one
    10,000-line error module, with provenance on every constraint.
  - Jet's one diagnostics registry (I4) matches.
  - The talk also stresses type-checking source before desugaring so errors
    make sense. Jet's sema checks the source AST.
- **Local lets stay monomorphic** (TLDI 2010; OutsideIn(X) §4.2):
  already implemented. Only 12% of 793 packages needed a signature when local
  generalisation was removed. Jet requires signatures.
- **Inference that never guesses** (OutsideIn(X) §6.5): already implemented.
  Predictability matters more than completeness. Hold this rule for future
  inference, such as element types of list literals and inferred failure
  unions. D-TYPECHECK-BOUND1=A (a bounded check with a local diagnostic) is
  related but different.

### Optimizer

- **An automatic inliner** (inliner thesis; the 2002 JFP paper was unread):
  real gap.
  - #2892's evidence says the shared MIR optimizer expands only
    `#Inline(Always)`, and the JIT uses Cranelift `opt_level=none`.
  - The thesis measures 26% mean speedup from better threshold settings
    alone, and about 10% from profile-guided hints on hot call chains.
  - Recommended: an occurrence-based inliner in the shared MIR optimizer, so
    all tiers keep one meaning.
    - Always inline single-use, non-escaping bindings.
    - Size-bounded inlining elsewhere, gated by paired performance cells.
    - Thresholds tuned on a corpus, not by hand.
    - Later, JIT tier-up profiles can pick the hot chains.
  - #2919 (ready) holds the deferred proof for the current expansion only.
- **Join points** (PLDI 2017; 2025 keynote): needs measurement.
  - A join point is a `let` whose calls are jumps: it allocates nothing, makes
    case-of-case safe from code blow-up, and must be preserved by later
    passes.
  - Jet's MIR is a block graph, so jumps exist after lowering. A name search
    found no join-point or case-of-case transform before MIR.
  - Measure whether nested tables and `?` chains duplicate blocks or build
    closures before adding a pass.
- **Kinds as calling conventions; levity polymorphism**: needs measurement.
  Generic code can compile to efficient conventions when layout facts ride
  the type. MIR already carries `layout.abi`. This matters if the planned
  Cranelift dev backend (#3953) or Jet-owned instantiation (#2520) shares
  generic code instead of specializing it. It is not a user-facing surface.

### Safety and trust

- **Consumer-decided trust** (Safe Haskell §2.3, 3.3): already implemented.
  Safe Haskell lets the person compiling decide which unsafe-internals
  modules to trust, and "safe imports" defer trust to the final user. Jet
  has `policy.unsafe` (`.Deny` or `.Paths`,
  `crates/jet-pkg-model/src/Package/Blocks.rs:2149`) and audited gates
  (`--gate unsafe=allow`, E0355/E3415 in
  `crates/jet-sema/src/Sema/UnsafeObligations.rs:411-416`). Worth checking:
  a consumer should be able to deny a transitive dependency's audited
  `#Unsafe` without editing that dependency.
- **Why people reach for `unsafePerformIO`** (awkward squad §2.6): three
  patterns recur.
  - Debug tracing is covered by `debug(value)` (D-DBG1): it adds no effect
    and is stripped from shipped artifacts.
  - Once-per-run configuration and process-wide state are also covered.
    Module-scope `::` constants, `prep { … }` (including `embed_file`), and
    script-scope `counter := 0` bindings exist (see
    `Examples/features/basics/script_run.jet` and
    `Examples/features/comptime/`). No ballot was needed.
- **Linearity on arrows vs on types** (Linear Haskell §6.1): needs
  measurement.
  - The paper argues for arrows, because generic containers and functions
    then work for linear and ordinary values alike.
  - Jet puts `#SingleUse` on the type. Its per-parameter `^` and `&`
    conventions are arrow-like, so Jet mixes both.
  - Follow-up probe (same day): the check found a defect. A dropped
    `[Lease]` runs with no diagnostic, and a dropped `Lease?` gets only
    L0101, while a single dropped `Lease` is E0140. Card #3966 tracks the
    leak, and ballot D-LIN-CONTAINER1 decides the container rule.

### Builds and incrementality

- **Build systems à la carte**: ratified, in progress.
  - Any build system is a scheduler plus a rebuilder.
  - Verifying traces with early cutoff give minimal rebuilds even with
    dynamic dependencies.
  - Correctness means every non-input equals its recomputation from the
    final store.
  - #3852 (ready) carries value-hash early cutoff and determinism criteria;
    #2517 and #3860 cover persisted checks and the incremental-equals-clean
    oracle.
  - Two additions for those cards:
    - Use the paper's correctness definition as #3860's oracle.
    - Treat the toolchain version and flags as tracked inputs ("self-tracking",
      §8.8), so a compiler upgrade invalidates what it should.

### Metaprogramming and compiler structure

- **Template Haskell**: in progress.
  - Compile-time code is ordinary code, and generated code is type-checked
    before it runs.
  - Safe Haskell later found that Template Haskell could break module
    boundaries.
  - #3530 ("complete semantic rechecking", ready) should include visibility
    in its recheck.
- **Trees that grow**: no action now. One syntax tree indexed by phase avoids
  keeping several large syntax types in sync. Jet uses separate AST, TIR, and
  MIR, and keeps parallel Rust and Jet ASTs (parity cards #3896–#3919).
  Revisit only if LSP, `fmt`, and Canvas need decorations on one tree.

### Language design and process

- **History of Haskell**: already implemented.
  - The committee worked because an Editor drove debates to a close and a
    Syntax Czar made binding syntax calls. Jet's owner ratification plays
    that role.
  - The authors name their compromises:
    - many ways to say one thing (Jet's I8 forbids this);
    - the monomorphism restriction, a rule that rejects plausible programs
      only to avoid an ambiguity;
    - `seq` quietly breaking parametricity.
  - Jet's `jet --version` already reports a planned 1.0 compatibility policy.
  - Before 1.0, audit for monomorphism-restriction smells: rules a beginner
    would hit without understanding why.
- **Champagne Prototyping and the elastic sheet-defined functions study**:
  owner gate.
  - Champagne gets objective data before a feature exists: one crisp
    question, a few credible users, scenario interviews, and coding by
    Cognitive Dimensions (5 person-days, £100).
  - The elastic SDF study measured cognitive load with NASA-TLX on 20
    counterbalanced participants. Users also wanted hidden detail with a
    trace on demand.
  - Jet's ballots compare same-program alternatives by designer argument;
    no user-evidence step was found.
  - Whether high-impact syntax ballots get a small study is the owner's
    call.

## Jet alignment and gaps

| Topic | Jet today | Class | Record |
|---|---|---|---|
| Transaction commit validation | Commit publishes stale copies (code); wrong totals (probe) | real gap | D-CONC-STM1, #2123 |
| Composable blocking (`retry`/`orElse`) | Manual `Condition` + `notify` | owner gate | D-SHAREDGUARD1 |
| No irrevocable effects in transactions | E0746; `#Undo` for FFI | already implemented | — |
| Guard-tree coverage | Matrix witnesses, guard-aware totality | already implemented | — |
| Long-distance coverage | Missing | real gap | — |
| Guard arms without `else` | Parse error vs L0303 | real gap | — |
| Deferred type errors | Whole program blocked | owner gate | — |
| Typed holes | `#Todo`, regressed on probed binary | in progress | #3065 |
| Error localization | Local inference limits distance | needs measurement | #3418 |
| Local let generalisation | Signatures required | already implemented | — |
| Automatic inliner | `#Inline(Always)` only; JIT `opt_level=none` | real gap | #2919 (proof only) |
| Join points | MIR blocks; no pre-MIR pass found | needs measurement | — |
| Unsafe trust by consumer | `policy.unsafe`, gates | already implemented | — |
| Linearity placement | On types (`#SingleUse`) | needs measurement | #3916 |
| Build early cutoff | Planned | in progress | #3852, #2517, #3860 |
| Parallel granularity | Fixed 64-item floor | in progress | #2922 |
| Metaprogram rechecking | Planned | in progress | #3530 |
| User evidence for syntax | Designer argument only | owner gate | — |

## Avoid list

- **Commit-time locking alone for read-dependent transactions.** Evidence:
  the STM paper §2.1; defect 1. Exposure: shipped today.
- **Rules that reject plausible programs only to avoid ambiguity** (the
  monomorphism restriction; History §6.2). Exposure: defect 5 is the same
  kind of beginner trap.
- **Hidden primitives that break reasoning** (`seq` and parametricity;
  History §3.6). Structural immunity: `debug` is declared effect-free and is
  stripped from shipped artifacts, and `#Unsafe` requires a reason.
- **Many ways to say one thing** (History §3.6). Structural immunity: I8.
- **Local let generalisation** (TLDI 2010). Structural immunity: required
  signatures.
- **Hand-tuned optimizer constants with no corpus** (Hollenbeck).
  Exposure: any inliner Jet adds.

## Prioritized recommendations

1. **Owner decision on D-CONC-STM1.** Choose commit-time validation with
   re-run (prints routed through `on_commit`) or first-touch locking.
   - Acceptance: a stress oracle (8 tasks by 1000 transfers; total conserved;
     balances never negative under a check-then-debit) passes on AOT,
     `jet run`, and `--interpret`.
   - Also confirm defect 1's live evidence on a clean HEAD build.
2. **Parser: guarded arms without `else`.**
   - Coverage verdicts come only from E0307/L0303.
   - Fix the example comment and the doubled backticks in L0303.
   - Acceptance: the two probe tables in defect 2 check clean, and the UI
     snapshots are updated.
3. **Beginner diagnostics.**
   - A bare lowercase multi-head name that is not a variant gets one
     diagnostic naming the missing parameter type, with no cascade.
   - E0355 for `#Todo` names `return #Todo`, or `{ #Todo }` is accepted again
     as #3065 restores.
4. **Long-distance coverage.** Report arms made unreachable by earlier flow
   facts (a lint), then ballot whether those facts may also prove totality.
5. **Automatic inliner in the shared MIR optimizer.**
   - Occurrence-based: always inline single-use bindings; size-bounded
     elsewhere.
   - Thresholds tuned on the gauntlet corpus.
   - Gated by paired performance cells on every tier.
6. **Ballot for `jet dev` deferred type errors** (never `jet build`), reusing
   E3011.
7. **Ballot for a transaction-scoped wait** (the `retry` analogue). It
   depends on item 1.
8. **Measurements.**
   - Blame accuracy on seeded mistakes.
   - `#SingleUse` in generic containers.
   - Join-point duplication in MIR.
   - Consumer denial of a transitive dependency's `#Unsafe`.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 `#Transact` lost updates | card | #3967 (ballot D-CONC-STM2 amends D-CONC-STM1) |
| F2 `retry`/`orElse` blocking | card | #3967 (full-profile ballot D-TXN-WAIT1, reviewed and open) |
| F3 guard arms need `else` / L0303 contradiction | card | #3971 (ballot D-MATCH-GUARD1) |
| F4 L0303 doubled backticks | card | #3969 |
| F5 long-distance coverage | card | #3970 (ballot D-COVER-FLOW1) |
| F6 `jet run` hides warnings | card | #3420 |
| F7 `#Todo` body rejected / E0355 Fix text | card | #3065 |
| F8 deferred type errors in `jet dev` | card | #3970 (ballot D-DEV-DEFER1) |
| F9 unannotated-parameter cascade | card | #3968 |
| F10 automatic inliner | card | #3970 (ballot D-OPT-INLINE1) |
| F11 build early cutoff and self-tracking | card | #3852 |
| F12 parallel granularity | card | #2922 |
| F13 metaprogram rechecking includes visibility | card | #3530 |
| F14 user evidence for syntax ballots | card | #3970 (ballot D-BALLOT-EVIDENCE1) |
| F15 `#SingleUse` duty lost in containers | card | #3966 (ballot D-LIN-CONTAINER1) |
| F16 Verse transactional conditions | no-action | archived: parked proposal #775 already records the idea; Jet keeps rollback explicit |
| F17 error blame accuracy | card | #3970 (ballot D-DIAG-BLAME1) |
| F18 pre-1.0 beginner-trap audit | card | #3970 (ballot D-BEGINNER-TRAPS1) |
| F19 dependency `#Unsafe` trust | card | #3970 (full-profile ballot D-UNSAFE-DEPS1, reviewed and open) |
| F20 policy path check misreads in-tree path dependencies | card | #3977 |
| F21 `#Transact` rollback misses method-call changes to locals | card | #3978 |
<!-- /audit-dispositions -->

Strongest unverified assumption: that the wrong `shared_transact` totals
reproduce on a clean HEAD build. The lost-update path is present in committed
code, but the live evidence came from a binary built on a heavily modified
working tree.
