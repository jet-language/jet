# Advocate dossier — Jet's current + ratified memory system

**Commission:** first-principles memory audit, advocate side. Steelman what ships, then attack it, all from live probes.
**Probe environment:** `scripts/agent/jet-env jet` (nix-provisioned `Jet 1.0.0`), repo `/home/nate/Projects/Github/jet` @ `420b11b93` (2026-08-11), dirty files are docs/skills/board only — no compiler source mid-edit. Every regression below was confirmed on a second binary (`target/debug/jet`).
**Evidence law observed:** spec text is never cited as proof of behavior. Every load-bearing claim carries a probe ID (P01–P17); all 57 probe programs and their verbatim outputs are in the Appendix. Tags: **SHIPPED** (probed working), **DEFECTIVE** (probed broken), **RATIFIED-UNBUILT**, **OPEN/PROPOSED**, **ABSENT**.

## The reframe

The popular reading of Jet's memory model — "Rust semantics with friendlier spelling" — is wrong in both directions.
Where it works, it is *stronger* than Rust's surface: compile-time use-after-free for allocator-backed values with zero lifetime syntax (P05b, P05h) is something Rust cannot express without exposing lifetimes, and no peer language catches `arena.reset()` → later read at compile time at all.
Where it fails, it fails *below* Rust's floor: the default tier and the release tier disagree on program meaning (P12 prints `0` vs `1` from the same source), sema approves programs whose generated Rust cannot compile (P03, P04a, P04d, P04i), and 8 of the repo's own 31 memory examples do not run today (P17).
The honest verdict: a categorical design with a broken enforcement spine — the *checker* is ahead of every peer, the *tier contract* behind all of them.

---

## 1. The strongest honest case FOR the current model

### 1.1 Three sigils carry the whole ownership story — and the callsite never lies

```jet
heal(&p, 30)     // & at the callsite: this argument will be mutated
name :: consume(^p)  // ^ at the callsite: p is gone after this line
backup :: ~p     // ~: this is a deliberate deep copy
```

P01 (both tiers pass): bare = read, `&` = exclusive write, `^` = take, `~` = copy. Rust needs `&`, `&mut`, move-by-default, `.clone()`, and `Copy`-vs-`Clone` trait knowledge to say the same four things; Jet says them with three characters that appear **at the callsite**, so a reader (or an agent consuming a diff) sees every mutation and every move without opening the callee.
Forgetting the mirror is E0202 with the exact repair (P02b): *"write the write-capability marker `&` (`&p`) when calling `heal`"* — one error, one fix, zero inference.
Use-after-move is E0121 with the repair on the offending line (P02): *"give away a copy instead (`~p`) where it moved"*.
No peer language puts the mutation marker at the callsite AND enforces it: C++ has nothing, Rust's `&mut` at callsites is the closest but arrives with lifetime vocabulary attached.

### 1.2 Compile-time use-after-free with zero lifetime syntax — the categorical win

| What the program did | What the compiler said | Probe |
|---|---|---|
| Read an arena value after `arena.reset()` | **E0632** compile error: *"`x` is a view into `arena`; `arena.reset()` invalidated everything stored in `arena`, so reading `x` now would read freed memory"* | P05b |
| Return a view of arena storage out of its region | **E0631** compile error: *"sharing it outside the region would let it outlive `arena` and point into freed memory"* + the `~` escape | P05h |
| `close(^arena)` while an alloc'd value is still used later | E0212/E0220 compile rejection | P05d |
| `values.push(50)` while a window into `values` is live | **E0212**: names the view, the owner, the invalidating call, and three fixes | P03b |
| Overlapping `&values[0..2]` / `&values[1..3]` write windows | **E0220 + E0212**, correct aliasing law stated in one sentence | P03c |
| Read one slot of a `[U8#4].{ uninit }` buffer before writing all slots | **E0420** definite-initialization rejection | P13 |

Not one of these programs contains a lifetime annotation, a borrow-checker vocabulary word, or an unsafe block. Zig catches none of these at compile time (arena UAF is a runtime crash or silent corruption); Rust catches the list-push case but cannot even *express* "arena reset invalidates these handles" in the type system — `bumpalo` UAF-after-reset is prevented by lifetimes only because allocation returns `&'bump T`, which infects every signature it touches. Jet's one-fact-graph does it invisibly. **This cluster is SHIPPED and it is the model's crown.**

### 1.3 Stored views without lifetime syntax — partially real, and where it works it embarrasses Rust

P16 (both tiers): the borrow-ceiling closeout example `owner_backed_views.jet` — a struct owning `[Book]`, returning read and write element windows through `View<Book>`/`ViewMut<Book>` with sema-inferred provenance — compiles and runs in debug AND release. The Rust equivalent forces `<'a>` onto the collection, the method, and every caller.
P04a/P04e: a struct field `value: View<str>` filled from `email.after("@")` works end-to-end when the owner is runtime data. `fn longer(left, right) => View<str> from left | right` (runtime-selected owner!) is expressible and checked — Rust needs explicit `'a` unification; Go/Java/C# need a GC to answer at all.
The escape rules fail closed with taught repairs: view of a local owner is E2307 with *"return a view derived from parameters or the receiver on every path, or return an owned `String` copy with `~`"* (P04c); borrowing from a temporary argument is E2307 naming the temporary and the fix (P04g).

### 1.4 Transitive memory facts — a contract no mainstream peer has at all

```jet
#Policy(no_alloc)
fn label(n: Int) => String = "value {n}"   // rejected: E0921, full call path
```

P06: the violation is reported **through the call chain** (`run -> describe -> label`), with declaration provenance and the operation named ("string interpolation allocates a new `String`"). P06c: the same fact works at block scope. P13c: `zero_rc` correctly rejects `Shared.new` ("introduces reference-counted ownership").
Rust's `#[no_std]` is a crate-granularity hack; C has nothing; Zig approximates it socially (allocator parameters) but not as a checked transitive fact. A game studio can put `#Policy(no_alloc)` on the frame-tick module and the compiler polices every reachable call including dependencies. **SHIPPED** (module/function/block scope probed; package scope and sealed-dispatch edges unprobed).

### 1.5 The unsafe tier is audited, not just gated

P07b: bare `#Unsafe { }` is a **hard error** (E3112) — a reason string is mandatory. P07c: raw-pointer ops outside the gate are E0208 with the exact wrapper spelled out.
P07d: `jet inspect unsafe file.jet` prints each gate, its reason, and **each raw operation with its named obligations** (`required=[no_alias]`, `required=[valid_ptr,aligned]`) and discharge status.
Rust ships ~4,000-unsafe-block codebases (Bun) with no reason field, no obligation taxonomy, and no first-class audit command. Jet's is SHIPPED, and the ratchet that stops silent growth is on the board (#1879, ready).

### 1.6 The FFI boundary has one memory rule and it is enforced

P08: `&String` in an `extern rust` signature is **E0702**: *"foreign functions take owned copies — `&` and `^` aren't allowed here."* P08b: by-value round-trip works.
"Who frees what" has a one-sentence answer — each side frees its own copy, always — which eliminates the entire cross-boundary double-free/leak class **by construction** rather than by convention (C), by annotation (Rust `extern` + manual `Box::into_raw` discipline), or by finalizer prayer (Java/Go).

### 1.7 The verdict loop is fast where agents live

Measured this session: tiny-program verdicts on the default tier arrive in **~0.75–0.8s** wall time, uniformly, for accepts and rejects alike (every probe). That is a real edit→verdict loop.
Compile-time cost of the whole ownership analysis is invisible at this scale; the 26–30s figure belongs entirely to the `--release` rustc bridge, not to the checker.

### The four standing-lens questions, answered

| Q | Answer |
|---|---|
| **Beat on a level field?** | Three vectors peers cannot adopt without breaking their models: (1) compile-time allocator-lifetime checking with zero lifetime syntax — Rust's surface *is* lifetime syntax, it cannot hide them retroactively; (2) callsite capability mirroring — C++/Go/Java have no slot for it; (3) transitive `no_alloc`/`zero_rc` facts over dependencies — no peer has scope-laddered memory contracts. All three probed SHIPPED (P02b, P05b, P06). |
| **What do we avoid?** | Rust's lifetime-annotation tax (probed absent: P04a/P16 store views with no syntax), Rust's reasonless `unsafe` (P07b), C's unsafe-by-default (P07c: raw ops are opt-in), GC-by-default pauses (GC is scoped opt-in, P16b), Zig's runtime-only UAF discovery (P05b is compile-time). Exposure rows in §3. |
| **AI-driven development?** | See agent-quantity scorecard (§2b). Summary: best-in-class verdict actionability and context economy when the checker speaks; catastrophic verdict-fidelity holes where tiers diverge (P12) or codegen ICEs on sema-approved programs (P03, P04d, P04i). The loop terminates fast on the happy 80%; on the broken 20% the oracle itself lies, which is worse for an agent than a hard language. |
| **Surfaces to cover?** | §"Concrete surfaces" table after the scorecard. |

---

## 2. Scorecard — fixed rubric, every cell

Grades A–F judge the **current shipped reality** (not the design intent). Tags: **S** shipped/probed · **D** defective (shipped, probed broken) · **R** ratified-unbuilt · **O** open/proposed · **A** absent. Probe refs in parentheses.

### 2a. Core metrics × domains

| Domain | Safety by default | R/W/Reason ergonomics | Runtime perf + predictability | Compile-time cost | Learnability (2-sentence test) | Expert ceiling | Diagnostics + repair | FFI/embedded fit |
|---|---|---|---|---|---|---|---|---|
| **Bare-metal MCU (≤2 KB, no allocator)** | **D-** — `--freestanding` compiles heap allocation AND OS file IO without complaint; E3301/E3303 registered but dead (P10, P10b, P10c) | **C** — `Fixed.over(&storage)` + `uninit` buffers are clean (P13b, P13); no probed path to a 2 KB profile | **B?** — no allocator, inline backing shipped (P13b); unmeasured on hardware [INFERENCE] | **A** — 0.8s verdicts (all probes) | **B** — "no heap unless you make one; `Fixed` wraps your bytes" — true but unenforced today | **C** — raw ptr + volatile tier exists (P07); typed board facts R (D-TARGET-ALLOC1, unprobed) | **B** — E0420/E0103 taught well (P13, P13b) | **D-** — the freestanding gate is the fit, and it is dead (P10) |
| **Kernel / driver** | **C** — #Unsafe islands audited (P07d); but no probed stable-address/volatile/interrupt story | **B** — reason-gated unsafe reads well (P07) | **B?** — value semantics + no GC default [INFERENCE] | **A** | **B** | **B** — `*T`, `p.*`, `*x` grammar shipped (P07); obligations tracked; inline asm surface exists (unprobed) | **A-** — E0208/E3112 name exact wrapper (P07b, P07c) | **B** — by-value FFI rule is wrong for kernel zero-copy; `#Unsafe` is the honest fallback (P08) |
| **AAA game frame loop (zero-alloc steady state)** | **B** — `#Policy(no_alloc)` on the tick is checked transitively (P06, P06b) | **D** — the three flagship game surfaces are broken today: `Pool.add` rejects structs (P17 entity_world), disjoint particle windows rejected (P15b), recursive enums (trees!) E0361 (P16c) | **C** — arenas + reset shipped (P05, P05g); Map on default tier corrupts data (P12) | **A** | **B+** — "arena per frame, reset each tick; the compiler rejects stale handles" — probed true (P05b) | **C** — no program-allocator swap (#1853 O), no probed SoA/hot-loop control beyond `#Layout` (unprobed) | **B** — E0632's what/why/fix is perfect (P05b) | n/a — say so: engine FFI unprobed |
| **Low-latency trading / audio (no pauses ever)** | **B+** — `no_alloc` + `zero_rc` facts both fire (P06, P13c); GC is opt-in-only (P16b) | **B** — facts are one line at the right scope (P06c) | **B?** — no GC, no RC unless imported; pause-freedom argued from mechanism, unmeasured [INFERENCE]; JIT-tier semantic divergence (P12) forbids trusting the dev tier in prod | **A** | **B** | **C** — allocator swap/cap absent (#1853 O); `arena_bounded(N)` unprobed | **B+** — E0921 names the allocating operation through the call path (P06) | **C** — by-value FFI copies on the hot path (P08b) |
| **Long-lived server (years, fragmentation)** | **B** — leaks structurally prevented by ownership + close; `Shared` works cross-task (P09c) | **C** — concurrency surface churned under the examples: `tasks.spawn` gone, spec + 2 examples still teach it (P09d, P17) | **C** — arena/reset fights fragmentation (P05); default-tier data corruption #1883 reproduced (P12) is disqualifying until fixed | **A** | **B** | **B** — guards, `Condition`, `#Transact` over `Shared` (spec'd; guard queue example passes sweep P17) | **B** — E1101 mutable-capture rejection is taught (P09) | **B** |
| **Data / compute pipeline (throughput, huge working sets)** | **A-** — zero-copy views checked end-to-end (P15, P16) | **C+** — parameter/return views excellent (P04e); but views cannot enter local collections even with live owners — forced `~` copies (P04j, P04h) | **B+** — zero-copy `View<str>` parse works both tiers (P15); owner-backed windows both tiers (P16) | **A** | **B+** | **B** — `from left \| right` provenance is expert-grade (P04d source) | **C** — the E2307/E0108 pair contradicts itself on the same line (P04h) | **B** |
| **Shell-script one-liner (zero ceremony)** | **A** — full checking with zero annotations (P01: not one sigil needed until mutation) | **A** — bare reads default; script mode exists; nothing said twice | **A** — 0.8s run (all probes) | **A** | **A** — "values copy like values; add `&` only to let a callee write" — two sentences, probed true (P01, P02b) | n/a — ceiling irrelevant here, say so | **A-** — parse errors carry e.g.-style fixes (P01b) | n/a |
| **Browser / wasm (foreign GC heap interop)** | **C?** — `--target=wasm32-unknown-unknown` builds (P14); no interop surface probed | **?** — unprobed beyond build | **?** | **A** | **?** | **A(bsent)** — foreign-GC-heap ownership story: no surface found | **?** | **D** — wasm builds, but the browser/JS ownership boundary is ABSENT today (P14 is the entire probed story) |
| **FFI-heavy embedding (C/C++/Rust ownership transfer)** | **A-** — E0702 makes the boundary rule impossible to get wrong (P08) | **B** — one signature form, no annotations (P08b) | **C** — mandatory copies at every crossing; no zero-copy transfer tier [INFERENCE from P08 rule] | **A** | **A** — "foreign calls copy in and copy out; each side frees its own" — two sentences, complete | **D** — an expert cannot opt into zero-copy or ownership transfer at all; no `extern c`/`cpp` probed path for callbacks (E0702 family) | **A-** — E0702's why/fix is exact (P08) | **B+** — bind generator + audit surfaces exist (`jet inspect bind`, unprobed) |
| **AI-agent-authored code at scale** | **B** — the compile-time UAF class (P05b) is exactly what agents can't debug at runtime | **A-** — capability visible at callsite = diff-readable without context (P01) | **C** — the agent's dev-tier oracle disagrees with prod tier (P12, P03) | **A** — 0.8s in-loop verdicts | **A-** — three sigils, one aliasing sentence | **B** | **B+** — see 2b | **B** |

`?` = honestly unprobed this session; `[INFERENCE]` = argued from mechanism, not measured.

### 2b. Agent-optimality quantities × domains

Domains collapse into three probed regimes for these five quantities; where a domain differs it is named.

| Quantity | Verdict, with evidence | Grade |
|---|---|---|
| **(a) Verdict fidelity** — compiler catches it, not production | Two-faced. The shipped checker catches UAF/invalidation/overlap/uninit/moves/races-by-capture at compile time (P02, P03b, P03c, P05b, P05d, P05h, P09, P13) — above every peer. But: sema-approved programs ICE in AOT (P03 windows, P04a/P04d const-folded views, P04i self-reference = rustc E0515 doing Jet's job); the default tier silently corrupts `Map.add` results (P12: prints `0` where release prints `1`); freestanding gates never fire (P10). For MCU/trading/server domains the broken cells are load-bearing. | **C** overall: **A** for the checker, **F** for tier parity |
| **(b) Verdict latency** — edit → verdict in-loop | 0.75–0.8s per default-tier verdict across all 57 probes; rejects as fast as accepts. `--release` truth costs 26–30s, and because of (a) you *need* the release check today for view-heavy code. | **A** dev tier, **C** when AOT confirmation is required |
| **(c) Verdict actionability** — the error names the fix | The best probed diagnostics anywhere in this repo's class: E0121/E0202/E0212/E0632/E0631/E3112/E0702 each carry a one-line what, a causal why, and a literal fix (P02, P02b, P03b, P05b, P05h, P07b, P08). Failures: E0921 spams absolute paths and byte offsets (P06); E2307+E0108 contradict each other on one line (P04h); `=> &Int` yields a bare parse error instead of teaching `View<Int>` (P04f); E0361 points at a comment/blank line (P16c, P11b); L0502 spans drift in release (P06b). | **B+** |
| **(d) Context economy** — tokens for the common case | Source: near-optimal — zero annotations for the read-only common case (P01), one sigil char per capability, no lifetime parameters anywhere (P16 vs the Rust equivalent's `<'a>` on three declarations). Diagnostics: mostly 6–8 tight lines; E0921 is the outlier (5× full paths + `62..79` offsets, P06). | **A-** |
| **(e) Repair determinism** — one error, one obvious repair | E0202→add `&`; E0121→`~`; E0632→move use before reset; E3112→add reason; E0702→drop sigil. Each admits exactly one repair (P02b, P02, P05b, P07b, P08). Nondeterministic spots: E0220 on provably-disjoint constant ranges offers "read through `edit` instead" which is *not* what the program means — the honest fix (reorder, or split support) is unstated (P03d, P15b); the view-in-collection wall offers only `~` copy, silently abandoning the zero-copy intent (P04j). | **B+** |

### Concrete surfaces (standing-lens Q4)

| Group | Surfaces |
|---|---|
| **Covered with proof** | `&`/`^`/`~` + bare-read params (P01); E0121, E0202, E0212, E0220, E0420, E0631, E0632, E0921, E2307, E3112, E0208, E1101, E0702, E0991 (all live-fired); `mem.Arena/Bump/Pool/Fixed.new`, `Fixed.over`, `.alloc/.reset`, `close(^)` (P05, P13b); `#Region(label)` block (P05g); `#Policy(no_alloc/zero_rc/gc)` at module+block scope (P06, P06c, P13c, P16b); `View<str>`/`View<T>`/`ViewMut` returns, struct fields, `from a \| b` provenance (P04, P15, P16); `[U8#N].{ uninit }` (P13); `#Unsafe("reason")`, `*T`, `p.*`, `*x` (P07); `jet inspect unsafe` (P07d); `extern rust` by-value boundary (P08b); `Shared.new/.read/.edit` cross-task (P09c); `Cell` guards runtime conflict panic (P09e); `task.all` mutable-capture rejection (P09); `--target=wasm32` build (P14) |
| **Worth checking next** | `arena_bounded(N)` fact; package-scope `policy:`; sealed-dispatch edges of D-MEM-FACTS1; `#Static $`; `#Layout(c/columnar)` on hot paths; inline asm + MMIO examples; `jet gc report`; `SharedGuard.split` disjointness prover; `#Transact` + `Shared` commit; AllocationCount/GeneratedUnsafe budget enforcement (surface exists, "no compatible canonical report" — dossier probe); `extern rust` bridge caching behavior under `--release` |
| **Missing (no surface found)** | Hosted program-allocator swap/wrap (#1853 open — Rust `#[global_allocator]` equivalent); browser/wasm foreign-GC-heap ownership; zero-copy FFI transfer tier; stored `&T` in any form (P04b: parse error); borrow-return `=> &T` (P04f: parse error); one-lookup map upsert (#1886) |

---

## 3. Known defects and unbuilt promises — no spin

**Repo example ground truth (P17): 23 of 31 `examples/features/memory/*.jet` pass on the default tier today; 8 fail.** Every failure below is reproduced on two binaries.

| # | Item | Status | Evidence |
|---|---|---|---|
| 1 | **Default tier and AOT disagree on program meaning**: `Map.add` returns `0` (handle) on `jet run`, `1` (correct previous value) on `--release` — silent data corruption, card #1883 (P0) | DEFECTIVE | P12: two-tier paste, same source |
| 2 | **Sema-approved view programs ICE in AOT**: const-folded view sources emit `String` into `&str` fields — including the repo's flagship `returned_views.jet` | DEFECTIVE | P04a, P04d ("internal compiler error: the generated Rust did not compile", rustc E0308) |
| 3 | **Self-referential struct (owner + view) accepted by sema, runs on JIT, impossible in AOT** — rustc E0515/E0505 is acting as the real checker; the one-fact-graph does not model this case | DEFECTIVE (fidelity hole) | P04i: JIT prints `key`, release ICEs |
| 4 | **Place-plan codegen ICE**: disjoint read+write windows + `~` copy, sema-accepted, generated Rust rejected (whole-root `&mut` plan) | DEFECTIVE | P03: JIT rc=0, release rc=101, rustc E0502 |
| 5 | **D-SHAPE-PLACE1 structural split not shipped**: constant disjoint ranges/indexes are rejected (E0220) — the spec's own example and `place_windows.jet` both fail | DEFECTIVE vs ratified law | P03d, P15b; contrast P03e (dead-window order passes) |
| 6 | **`--freestanding` enforces nothing**: heap-allocating and file-IO programs build clean; E3301/E3303 registered but never fire; the built "freestanding" binary reads a file at runtime | DEFECTIVE | P10, P10b, P10c |
| 7 | **Recursive enums are broken** (auto-derived `==` false-positive E0361, span on a comment/blank line) — kills trees/linked structures as values; breaks `gc_cyclic.jet` | DEFECTIVE | P16c, P11b |
| 8 | **`Pool<T>` API regressed**: `world.add(...)` wants `Int` for a struct; `entity_world.jet`, `entity_tree.jet`, `pool_stale_id.jet` all fail — the many-owner escape hatch is unusable today | DEFECTIVE | P17 sweep, P11 |
| 9 | **`tasks.spawn` removed while spec + `shared_config.jet` + `shared_transact.jet` still teach it** (E1004 lists the surviving items) | DEFECTIVE (doc/example drift) | P09d, P17 |
| 10 | **`expiring_secret.jet` panics mid-run on default tier** ("secret key handle is invalid or already moved") with output diverging from its expected file | DEFECTIVE | P17 sweep + direct run |
| 11 | Views cannot enter any local collection or stored lambda even with live owners — only chain/`~`; error text contradicts itself (E2307 says "it's a view, copy it"; E0108 says "should be `View<str>`, not String") | SHIPPED-LIMIT + diag defect | P04j, P04h, P09b |
| 12 | Spec's own sigil example binds `copy ::` — a reserved word since D-SHAPE-COPY1 (E0991) | Doc drift | P01b |
| 13 | `use core.mem` gate fires as generic E0107 name error, not the documented E3102 teaching gate | Minor drift | P05c |
| 14 | E0921 diagnostic quality: triplicated errors, absolute paths ×5, byte-offset provenance | SHIPPED-noisy | P06 |
| 15 | Release-tier lint L0502 fires with drifted spans (struct decl line, blank line) | DEFECTIVE (minor) | P06b |
| 16 | Hosted program allocator (swap/wrap/count/cap) | OPEN — ballot not yet drafted | card #1853 (planning, criteria all open) |
| 17 | Memory floors migrate to effect denials `=[!Mem.Alloc]=>`, retiring `no_alloc` spelling | RATIFIED-UNBUILT (D-AUTHORITY-MEM1/2 = ratified; #1568 ready) — note: the probed `#Policy(no_alloc)` surface is *scheduled to change* | board |
| 18 | One gate ladder (#1734), unsafe ratchet (#1879), hardened runtime-verify profile (#1888), one-lookup upsert (#1886), panic-report quality (#1884), fix-text trap (#1885), foreign-keyword lies (#1887) | OPEN/READY | board reads |
| 19 | Tier-2 stored references | ABSENT BY DESIGN in v1 ("no stored references" — type-unification audit); `&T` field/return are parse errors | P04b, P04f |
| 20 | D-REGION1 `#Region(r)` semantics beyond a scope label | UNVERIFIABLE by probe (label form works; no handle is bound) | P05e, P05g |
| 21 | D-MEMDISJOINT1 (runtime disjoint proof, ratified =A) | RATIFIED-UNBUILT — its target case is defect #5 | P15b + board decision |
| 22 | D-LL3 "wider core.mem" | OPEN/PROPOSED — no Tower record found; spec-only silhouette | board search |

**Mistakes-of-peers table (standing-lens Q2), with Jet's exposure:**

| Peer mistake | Evidence | Jet exposure |
|---|---|---|
| Rust: lifetime syntax metastasizes through APIs | every `&'a` signature | Structurally immune on the surface (P16)… until codegen leaks rustc's errors through an ICE (P04i) — the immunity is only as real as tier parity |
| Rust: reasonless `unsafe` accumulates silently (Bun ~4k blocks) | #1879 card | Immune by construction (P07b) + audit command (P07d); ratchet not yet built |
| C: unsafe is the default mode | WG14 N2659 | Immune: raw ops are opt-in and gated (P07c) |
| Zig: UAF found at runtime, if ever | GPA safety only in debug | Immune for arena/view class (P05b, P05h) — **the** categorical differentiator |
| GC languages: foreign heap interop and pause tails | — | GC opt-in only (P16b); but wasm/browser interop absent, so the GC-interop mistake is *unaddressed*, not avoided |
| Rust entry-API era: get-then-insert misery before NLL | #1886 body | Currently exposed: no one-lookup upsert, and `counts[w] += 1` is rejected with a fix-text trap (#1885) |

---

## 4. The three hardest questions the current model cannot answer today

**Q1 — "I have two long-lived values; one needs to point at the other. What do I write?"**
Not a parameter, not a return, not a scoped window — a doubly-linked node, an LRU cache entry, an interner, an observer list. Stored `&T` is a parse error (P04b), `View` refuses every collection/lambda store (P04j, P09b), `Shared<T>` answers only by giving up `zero_rc` (P13c) and taking a lock, and `Pool<T>`/`Id<T>` — the designed answer — does not compile today (P17). Recursive enums, the value-semantics answer for trees, are also broken (P16c). Today the honest answer is "copy it with `~`, or wait," and no ratified decision names the Tier-2 stored-reference design (C1 posture: "moot in v1").

**Q2 — "Which tier is the language?"**
The same source prints `0` on `jet run` and `1` on `jet run --release` (P12); a program sema blesses runs on the dev tier and cannot exist under AOT (P04i); the spec's flagship view example runs on one tier and ICEs on the other (P04d). Until the one-fact-graph — not rustc, not the JIT's RC runtime — is the single arbiter on every tier (I9), "is this program correct?" has no answer an agent or a beginner can trust, and every compile-time guarantee in §1 carries an asterisk.

**Q3 — "Certify that this program never allocates after startup — end to end, on this target."**
The pieces exist separately and don't compose: `#Policy(no_alloc)` checks reachable *Jet* code (P06) but its spelling is scheduled to be replaced by effect denials (#1568); the freestanding gate that should catch the runtime's own allocations is dead (P10); there is no hosted allocator swap/wrap to count or cap what the runtime does (#1853, all criteria open); allocation budgets exist as a metric vocabulary with "no compatible canonical report" (dossier probe). A trading or MCU engineer cannot get one signed verdict for the question their domain actually asks.

---

## Micro sweep (every category, one row each)

| Category | Finding | Probe |
|---|---|---|
| Syntax | `^` is callsite-only; `moved :: ^p` in binding position is a parse error with no teaching (what *is* a local move-rebind?) | P04k |
| Ergonomics | The zero-copy intent dies at any store: `~` is the only exit the compiler offers, so "zero-copy parse" quietly becomes O(n) copies in real code | P04j |
| Surfaces | `core.mem` discovery gate is a generic name error (helpful fix, wrong code vs docs) | P05c |
| APIs/types/methods | `Pool.add` signature regressed to `Int`; `tasks.spawn` removed with examples still teaching it | P17, P09d |
| Defaults | Read-only is genuinely the annotation-free default — the safety claim is won at the default | P01, P02b |
| Naming | "window/view/place/owner" vocabulary is consistent across E0212/E0220/E0631/E0632 — a real teaching asset; `arena[fresh]` leaks an internal place name into user text | P05d |
| Error text | Best: E0632 (what/why/fix all perfect). Worst: E0921 path-spam; E0361 span on a comment | P05b, P06, P16c |
| UX/DX | 0.8s dev verdicts; but view-heavy code needs the 27s AOT pass to learn the truth | P04a |
| Tooling/CLI | `jet inspect unsafe` is a genuine differentiator; budgets surface present but unpopulated | P07d |
| Ceremony vs control | Zero ceremony on the safe path; missing control at the ceiling (allocator swap, zero-copy FFI, stored refs) | P08, #1853 |

## Strongest unverified assumption in this dossier

That the JIT tier's memory *runtime* (as opposed to its checker) is refcount-based and therefore the P04i self-referential program is memory-safe on the tier where it runs — I observed it print the right answer, not why. If the dev tier is instead handing out unmanaged interior pointers there, defect #3 is not a parity bug but a live use-after-free generator on the default tier, and the whole §1 story inverts for every program that stores a view.

---

## 5. Appendix — every probe, verbatim, with full output

Scratch dir: `~/.cache/jet-audit-scratch` (removed after this dossier was written). `jet run` = default tier; `jet run --release` = AOT. Outputs are unedited except stripping the repo-dirty warning line.

### p01_sigils_ok

```jet
// P1: the three sigils working as designed
struct Player { name: String, hp: Int }

fn heal(p: &Player, amount: Int) { p.hp += amount }   // & = exclusive write
fn describe(p: Player) => String = "{p.name}: {p.hp} hp" // bare = read
fn consume(p: ^Player) => String = p.name             // ^ = take ownership

fn run() {
    p := Player.{ name: "Kai", hp: 70 }
    heal(&p, 30)              // callsite mirrors &
    print(describe(p))        // read borrow, p still usable
    backup :: ~p              // ~ = explicit deep copy
    name :: consume(^p)       // ^ moves p away
    print(name)
    print(describe(backup))   // the copy survives the move
}
```

**`jet run p01_sigils_ok.jet`** — exit 0 (0.803s)

```
Kai: 100 hp
Kai
Kai: 100 hp
```

**`jet run --release p01_sigils_ok.jet`** — exit 0 (26.336s)

```
Kai: 100 hp
Kai
Kai: 100 hp
```

### p01b_copy_keyword

```jet
// micro: the spec's own place-access example binds `copy ::` — a reserved word today
struct Player { name: String, hp: Int }

fn run() {
    p := Player.{ name: "Kai", hp: 70 }
    copy :: ~p
    print(copy.hp)
}
```

**`jet run p01b_copy_keyword.jet`** — exit 1 (0.75s)

```
Error [E0003]: expected a call, binding, assignment, or `return`, found the keyword `copy`
  --> /home/nate/.cache/jet-audit-scratch/p01b_copy_keyword.jet:6:5
    |
  6 |     copy :: ~p
    |     ^^^^
 Why: inside a function body, write a call, binding, assignment, or `return`
 Fix: e.g. print("hello") or x :: 1

Error [E0991]: `copy` is now the `~` sigil
  --> /home/nate/.cache/jet-audit-scratch/p01b_copy_keyword.jet:7:11
    |
  7 |     print(copy.hp)
    |           ^^^^
 Why: Jet has exactly one spelling for a copy — the `~` sigil (D-SHAPE-COPY1) — so all code reads the same
 Fix: write `~name` in place of `copy name`

Error [E0003]: expected a value, found `.`
  --> /home/nate/.cache/jet-audit-scratch/p01b_copy_keyword.jet:7:15
    |
  7 |     print(copy.hp)
    |               ^
 Why: a value can be a name, a number, quoted text, `true`/`false`, or a call
 Fix: e.g. `x`, `42`, `3.5`, or `"hello"`

3 problems found
run `jet explain E0003` to learn more
```

### p02_use_after_move

```jet
struct Player { name: String, hp: Int }
fn consume(p: ^Player) => String = p.name

fn run() {
    p := Player.{ name: "Kai", hp: 70 }
    name :: consume(^p)
    print(p.hp)          // use after move — should be rejected
}
```

**`jet run p02_use_after_move.jet`** — exit 1 (0.783s)

```
Error [E0121]: `p` was given away earlier, so it can't be used here
  --> /home/nate/.cache/jet-audit-scratch/p02_use_after_move.jet:7:13
    |
  7 |     print(p.hp)          // use after move — should be rejected
    |             ^^
 Why: after a value moves to another name, the old name no longer gives access to it
 Fix: give away a copy instead (`~p`) where it moved

1 problem found
run `jet explain E0121` to learn more
```

### p02b_missing_amp

```jet
struct Player { name: String, hp: Int }
fn heal(p: &Player) { p.hp += 5 }

fn run() {
    p := Player.{ name: "Kai", hp: 70 }
    heal(p)              // callsite must mirror & — should be rejected
    print(p.hp)
}
```

**`jet run p02b_missing_amp.jet`** — exit 1 (0.778s)

```
Error [E0202]: parameter `p` requires the write-capability marker `&` at the call site
  --> /home/nate/.cache/jet-audit-scratch/p02b_missing_amp.jet:6:10
    |
  6 |     heal(p)              // callsite must mirror & — should be rejected
    |          ^
 Why: `heal` needs to edit this value with the write-capability marker `&`; passing it without that marker grants only read access
 Fix: write the write-capability marker `&` (`&p`) when calling `heal`

1 problem found
run `jet explain E0202` to learn more
```

### p03_windows_aot_ice

```jet
// sema accepts; AOT generates invalid Rust (tier-parity defect witness)
fn run() {
    values := [10, 20, 30, 40]
    read :: values[0..1]
    edit :: &values[2..3]
    edit[0] = 99
    indep :: ~values[0..1]
    print(read[0])
    print(values[2])
    print(indep[0])
}
```

**`jet run p03_windows_aot_ice.jet`** — exit 0 (0.789s)

```
10
99
10
```

**`jet run --release p03_windows_aot_ice.jet`** — exit 101 (27.415s)

```
effects: IO
internal compiler error: the generated Rust did not compile.
This is a bug in jet, NOT in your program. Please report it,
attaching your source file and the generated file below.
  generated: build/p03_windows_aot_ice.rs
--- rustc said ---
error[E0502]: cannot borrow `__jet_values` as immutable because it is also borrowed as mutable
   --> build/.work.p03_windows_aot_ice.999851/p03_windows_aot_ice.rs:103:50
    |
 92 |     let __jet_place_plan_0_root = &mut (__jet_values)[..];
    |                                        -------------- mutable borrow occurs here
...
103 |     let __jet_indep: Vec<i64> = (jet_slice_range(&(__jet_values), &(JetRange { start: 0i64, end: 1i64, exclusive: false }), "/home/...
    |                                                  ^^^^^^^^^^^^^^^ immutable borrow occurs here
104 |     println!("{}", (jet_index_vec(&(__jet_read), 0i64, "/home/nate/.cache/jet-audit-scratch/p03_windows_aot_ice.jet", 8)).jet_show());
    |                                   ------------- mutable borrow later used here

error: aborting due to 1 previous error

For more information about this error, try `rustc --explain E0502`.
```

### p03b_view_invalidation

```jet
// mutating the owner while a read view is live must be rejected
fn run() {
    values := [10, 20, 30, 40]
    window :: values[0..2]
    values.push(50)        // may relocate storage while window is live
    print(window[0])
}
```

**`jet run p03b_view_invalidation.jet`** — exit 1 (0.784s)

```
Error [E0212]: `values` cannot be changed by `.push()` while `window` is still looking into it
  --> /home/nate/.cache/jet-audit-scratch/p03b_view_invalidation.jet:5:5
    |
  5 |     values.push(50)        // may relocate storage while window is live
    |     ^^^^^^
 Why: `window` is a live read view into `values[…]`; changing or moving the owner could invalidate that view
 Fix: finish using `window` before changing `values`, narrow the view's scope, or make an owned copy

1 problem found
run `jet explain E0212` to learn more
```

### p03c_window_overlap

```jet
// overlapping write windows must be rejected
fn run() {
    values := [10, 20, 30, 40]
    a :: &values[0..2]
    b :: &values[1..3]     // overlaps the live write window
    a[0] = 1
    b[0] = 2
    print(values[1])
}
```

**`jet run p03c_window_overlap.jet`** — exit 1 (0.78s)

```
Error [E0220]: `values` cannot be read while `a` has a live exclusive write window into it
  --> /home/nate/.cache/jet-audit-scratch/p03c_window_overlap.jet:5:11
    |
  5 |     b :: &values[1..3]     // overlaps the live write window
    |           ^^^^^^
 Why: `a` is an exclusive window into `values[…]`; reading the owner beside that window would be rejected after lowering
 Fix: read or edit through `a` instead of `values`

Error [E0212]: `values[…]` already has a live view that conflicts with `b`
  --> /home/nate/.cache/jet-audit-scratch/p03c_window_overlap.jet:5:5
    |
  5 |     b :: &values[1..3]     // overlaps the live write window
    |     ^
 Why: many read views may overlap, but an exclusive mutable view cannot overlap any other live view
 Fix: finish using the earlier view before creating this one, or make an owned copy

2 problems found
run `jet explain E0220` to learn more
```

### p03d_disjoint_rejected

```jet
// spec promises constant disjoint ranges split safely; keeping edit live rejects the copy
fn run() {
    values := [10, 20, 30, 40]
    read :: values[0..1]
    edit :: &values[2..3]
    copied :: ~values[0..1]
    print(read[0])
    print(edit[0])
    print(copied[0])
}
```

**`jet run p03d_disjoint_rejected.jet`** — exit 1 (0.782s)

```
Error [E0220]: `values` cannot be read while `edit` has a live exclusive write window into it
  --> /home/nate/.cache/jet-audit-scratch/p03d_disjoint_rejected.jet:6:16
    |
  6 |     copied :: ~values[0..1]
    |                ^^^^^^
 Why: `edit` is an exclusive window into `values[…]`; reading the owner beside that window would be rejected after lowering
 Fix: read or edit through `edit` instead of `values`

1 problem found
run `jet explain E0220` to learn more
```

### p03e_spec_verbatim

```jet
// spec order, no later use of the write window — accepted
fn run() {
    values := [10, 20, 30, 40]
    read :: values[0..1]
    edit :: &values[2..3]
    copied :: ~values[0..1]
    print(copied[0])
}
```

**`jet run p03e_spec_verbatim.jet`** — exit 0 (0.784s)

```
10
```

**`jet run --release p03e_spec_verbatim.jet`** — exit 0 (26.335s)

```
10
```

### p04a_struct_view_field

```jet
// Can a struct hold a reference today? View<str> field, owner outlives struct
struct Domain { value: View<str> }

fn domain(email: String) => Domain {
    value :: email.after("@")
    return Domain.{ value: value }
}

fn run() {
    email :: "user@example.com"
    d :: domain(email)
    print(d.value)
}
```

**`jet run p04a_struct_view_field.jet`** — exit 0 (0.79s)

```
example.com
```

**`jet run --release p04a_struct_view_field.jet`** — exit 101 (27.459s)

```
effects: IO
internal compiler error: the generated Rust did not compile.
This is a bug in jet, NOT in your program. Please report it,
attaching your source file and the generated file below.
  generated: build/p04a_struct_view_field.rs
--- rustc said ---
error[E0308]: mismatched types
   --> build/.work.p04a_struct_view_field.1005161/p04a_struct_view_field.rs:102:61
    |
102 |     let __jet_d: __jet_Domain = __jet_Domain { __jet_value: "example.com".to_string() };
    |                                                             ^^^^^^^^^^^^^^^^^^^^^^^^^ expected `&str`, found `String`
    |
help: try removing the method call
    |
102 -     let __jet_d: __jet_Domain = __jet_Domain { __jet_value: "example.com".to_string() };
102 +     let __jet_d: __jet_Domain = __jet_Domain { __jet_value: "example.com" };
    |

error: aborting due to 1 previous error

For more information about this error, try `rustc --explain E0308`.
```

### p04b_ref_struct_field

```jet
// Can a struct field be a raw borrow type &T ?
struct Holder { r: &Int }

fn run() {
    x :: 41
    h :: Holder.{ r: &x }
    print(h.r)
}
```

**`jet run p04b_ref_struct_field.jet`** — exit 1 (0.749s)

```
Error [E0003]: expected a type name, found `&`
  --> /home/nate/.cache/jet-audit-scratch/p04b_ref_struct_field.jet:2:20
    |
  2 | struct Holder { r: &Int }
    |                    ^
 Why: types look like `Int`, `String`, or `[Int]`
 Fix: e.g. `x: Int` or `items: [String]`

1 problem found
run `jet explain E0003` to learn more
```

### p04c_return_local_view

```jet
// returning a view of a local owner must be rejected
fn leak() => View<str> {
    local :: "temporary buffer"
    return local.trim()
}

fn run() {
    print(leak())
}
```

**`jet run p04c_return_local_view.jet`** — exit 1 (0.78s)

```
Error [E2307]: returned string views need a stable owner relationship
  --> /home/nate/.cache/jet-audit-scratch/p04c_return_local_view.jet:4:18
    |
  4 |     return local.trim()
    |                  ^^^^
 Why: each public `View<str>` slot must name a bounded set of receiver, parameter, or static `String` sources that stay live; this return did not prove those sources
 Fix: return a view derived from parameters or the receiver on every path, or return an owned `String` copy with `~`

1 problem found
run `jet explain E2307` to learn more
```

### p04d_repo_returned_views

```jet
// D-MEM-VIEWRET1=B: named View boundaries carry inferred owner provenance.
// The caller keeps each owner alive; no user-written lifetime syntax is needed.
struct Domain {
    value: View<str>
}

struct Token {
    text: View<str>
    rest: View<str>
}

struct BorrowedRecord {
    kind: View<str>
    body: View<str>
}

fn first(values: [Int]) => View<Int> from values = values[0..1]

// The result safely borrows whichever input wins at runtime. Both possible
// owners remain live at the call site. D-MEMPROVENANCE3=A: the `from` clause
// makes that contract explicit for library APIs.
fn longer(left: String, right: String) => View<str> from left | right {
    if left.len() >= right.len() {
        winner :: left.trim()
        return winner
    }
    winner :: right.trim()
    return winner
}

fn domain(email: String) => Domain {
    value :: email.after("@")
    return Domain.{value: value}
}

// Both parser outputs borrow one caller-owned input. `parse` proves that
// provenance composes through an ordinary wrapper call.
fn scan(source: String) => Token {
    text :: source.before(":")
    rest :: source.after(":")
    return Token.{text: text, rest: rest}
}

fn parse(source: String) => Token = scan(source)

// A parser can return a list of tokens borrowed from different input buffers.
fn tokenize(left: String, right: String) => [View<str>] {
    left_token :: left.before(":")
    right_token :: right.before(":")
    return [left_token, right_token]
}

// A borrowing deserializer can also keep one field from each input buffer.
fn deserialize(header: String, payload: String) => BorrowedRecord {
    kind :: header.after(":")
    body :: payload.after(":")
    return BorrowedRecord.{ kind: kind, body: body }
}

fn run() {
    values :: [7, 8]
    email :: "user@example.com"
    source :: "name:value"
    token :: parse(source)
    left :: "short"
    right :: "a longer value"
    left_source :: "left:1"
    right_source :: "right:2"
    header :: "kind:message"
    payload :: "body:hello"
    tokens :: tokenize(left_source, right_source)
    record :: deserialize(header, payload)
    print(first(values)[0])
    print(domain(email).value)
    print(token.text)
    print(token.rest)
    print(longer(left, right))
    print(tokens[0])
    print(tokens[1])
    print(record.kind)
    print(record.body)
}
```

**`jet run p04d_repo_returned_views.jet`** — exit 0 (0.913s)

```
7
example.com
name
value
a longer value
left
right
message
hello
```

**`jet run --release p04d_repo_returned_views.jet`** — exit 101 (27.715s)

```
effects: IO
internal compiler error: the generated Rust did not compile.
This is a bug in jet, NOT in your program. Please report it,
attaching your source file and the generated file below.
  generated: build/p04d_repo_returned_views.rs
--- rustc said ---
error[E0308]: mismatched types
   --> build/.work.p04d_repo_returned_views.1008052/p04d_repo_returned_views.rs:151:62
    |
151 |     let __jet_token: __jet_Token = __jet_Token { __jet_text: "name".to_string(), __jet_rest: "value".to_string() };
    |                                                              ^^^^^^^^^^^^^^^^^^ expected `&str`, found `String`
    |
help: try removing the method call
    |
151 -     let __jet_token: __jet_Token = __jet_Token { __jet_text: "name".to_string(), __jet_rest: "value".to_string() };
151 +     let __jet_token: __jet_Token = __jet_Token { __jet_text: "name", __jet_rest: "value".to_string() };
    |

error[E0308]: mismatched types
   --> build/.work.p04d_repo_returned_views.1008052/p04d_repo_returned_views.rs:151:94
    |
151 |     let __jet_token: __jet_Token = __jet_Token { __jet_text: "name".to_string(), __jet_rest: "value".to_string() };
    |                                                                                              ^^^^^^^^^^^^^^^^^^^ expected `&str`, found `String`
    |
help: try removing the method call
    |
151 -     let __jet_token: __jet_Token = __jet_Token { __jet_text: "name".to_string(), __jet_rest: "value".to_string() };
151 +     let __jet_token: __jet_Token = __jet_Token { __jet_text: "name".to_string(), __jet_rest: "value" };
    |

error[E0308]: mismatched types
   --> build/.work.p04d_repo_returned_views.1008052/p04d_repo_returned_views.rs:158:40
    |
158 |     let __jet_tokens: Vec<&str> = vec!["left".to_string(), "right".to_string()];
    |                                        ^^^^^^^^^^^^^^^^^^ expected `&str`, found `String`
    |
help: try removing the method call
    |
158 -     let __jet_tokens: Vec<&str> = vec!["left".to_string(), "right".to_string()];
158 +     let __jet_tokens: Vec<&str> = vec!["left", "right".to_string()];
    |

error[E0308]: mismatched types
   --> build/.work.p04d_repo_returned_views.1008052/p04d_repo_returned_views.rs:159:81
    |
159 | ...= __jet_BorrowedRecord { __jet_kind: "message".to_string(), __jet_body: "hello".to_string() };
    |                                         ^^^^^^^^^^^^^^^^^^^^^ expected `&str`, found `String`
    |
help: try removing the method call
    |
159 -     let __jet_record: __jet_BorrowedRecord = __jet_BorrowedRecord { __jet_kind: "message".to_string(), __jet_body: "hello".to_string() };
159 +     let __jet_record: __jet_BorrowedRecord = __jet_BorrowedRecord { __jet_kind: "message", __jet_body: "hello".to_string() };
    |

error[E0308]: mismatched types
   --> build/.work.p04d_repo_returned_views.1008052/p04d_repo_returned_views.rs:159:116
    |
159 | ...d: "message".to_string(), __jet_body: "hello".to_string() };
    |                                          ^^^^^^^^^^^^^^^^^^^ expected `&str`, found `String`
    |
help: try removing the method call
    |
159 -     let __jet_record: __jet_BorrowedRecord = __jet_BorrowedRecord { __jet_kind: "message".to_string(), __jet_body: "hello".to_string() };
159 +     let __jet_record: __jet_BorrowedRecord = __jet_BorrowedRecord { __jet_kind: "message".to_string(), __jet_body: "hello" };
    |

error: aborting due to 5 previous errors

For more information about this error, try `rustc --explain E0308`.
```

### p04e_runtime_view_aot

```jet
// defeat constant folding: owner assembled from runtime data
struct Domain { value: View<str> }

fn domain(email: String) => Domain {
    value :: email.after("@")
    return Domain.{ value: value }
}

fn run() {
    n := 0
    loop i, [1, 2, 3] { n += i }
    email :: "user{n}@example.com"   // runtime interpolation
    d :: domain(email)
    print(d.value)
}
```

**`jet run p04e_runtime_view_aot.jet`** — exit 0 (0.801s)

```
example.com
```

**`jet run --release p04e_runtime_view_aot.jet`** — exit 0 (28.096s)

```
example.com
effects: IO
```

### p04f_amp_return

```jet
// is there a borrow-return form?
fn pick(values: [Int]) => &Int = &values[0]

fn run() {
    values :: [1, 2, 3]
    print(pick(values))
}
```

**`jet run p04f_amp_return.jet`** — exit 1 (0.758s)

```
Error [E0003]: expected `{` to open the function body, found `&`
  --> /home/nate/.cache/jet-audit-scratch/p04f_amp_return.jet:2:27
    |
  2 | fn pick(values: [Int]) => &Int = &values[0]
    |                           ^
 Why: the structure here isn't what the compiler expected
 Fix: use `{` to open the function body

1 problem found
run `jet explain E0003` to learn more
```

### p04g_selfref_struct

```jet
// self-referential: struct holding owner AND view into it
struct Parsed {
    source: String
    head: View<str>
}

fn parse(text: String) => Parsed {
    head :: text.before(":")
    return Parsed.{ source: text, head: head }
}

fn run() {
    p :: parse("key:value")
    print(p.head)
}
```

**`jet run p04g_selfref_struct.jet`** — exit 1 (0.784s)

```
Error [E0120]: `text` was not moved here, so it cannot fill an owned field
  --> /home/nate/.cache/jet-audit-scratch/p04g_selfref_struct.jet:9:29
    |
  9 |     return Parsed.{ source: text, head: head }
    |                             ^^^^
 Why: this function has read access only and does not own the value
 Fix: copy it explicitly with `~text`

Error [E2307]: a returned view cannot borrow from a temporary argument
  --> /home/nate/.cache/jet-audit-scratch/p04g_selfref_struct.jet:13:16
    |
 13 |     p :: parse("key:value")
    |                ^^^^^^^^^^^
 Why: the temporary owner is dropped at the end of this statement, while the returned view remains live
 Fix: store the owner in a named binding first, then pass that binding to the view-returning call

2 problems found
run `jet explain E0120` to learn more
```

### p04h_view_cache

```jet
// long-lived cache of views: views outliving one call's owner
fn run() {
    cache := [View<str>].{}
    loop word, ["alpha:1", "beta:2"] {
        head :: word.before(":")
        cache.push(head)
    }
    print(cache[0])
    print(cache[1])
}
```

**`jet run p04h_view_cache.jet`** — exit 1 (0.783s)

```
Error [E2307]: `head` can't be used directly here yet
  --> /home/nate/.cache/jet-audit-scratch/p04h_view_cache.jet:6:20
    |
  6 |         cache.push(head)
    |                    ^^^^
 Why: `head` is a zero-copy view into `word` (`.trim()`/`.after()`/`.before()`); only chaining another `.trim()`/`.after()`/`.before()` on it works directly — other methods and calls need the full owned value
 Fix: write `~head` first to get an owned `String`, then use that

Error [E0108]: argument 1 to `.push()` should be `View`<str>, not String (text)
  --> /home/nate/.cache/jet-audit-scratch/p04h_view_cache.jet:6:20
    |
  6 |         cache.push(head)
    |                    ^^^^
 Why: built-in methods need arguments of the right type
 Fix: use `View`<str> here

2 problems found
run `jet explain E2307` to learn more
```

### p04i_selfref_move

```jet
// true self-reference: struct owns the String AND a view into it, owner moved in
struct Parsed {
    source: String
    head: View<str>
}

fn parse(text: ^String) => Parsed {
    head :: text.before(":")
    return Parsed.{ source: text, head: head }
}

fn run() {
    input :: "key:value"
    p :: parse(^input)
    print(p.head)
}
```

**`jet run p04i_selfref_move.jet`** — exit 0 (0.789s)

```
key
```

**`jet run --release p04i_selfref_move.jet`** — exit 101 (29.37s)

```
effects: IO
internal compiler error: the generated Rust did not compile.
This is a bug in jet, NOT in your program. Please report it,
attaching your source file and the generated file below.
  generated: build/p04i_selfref_move.rs
--- rustc said ---
error[E0308]: mismatched types
   --> build/.work.p04i_selfref_move.1019136/p04i_selfref_move.rs:103:99
    |
103 |     let __jet_p: __jet_Parsed = __jet_Parsed { __jet_source: "key:value".to_string(), __jet_head: "key".to_string() };
    |                                                                                                   ^^^^^^^^^^^^^^^^^ expected `&str`, found `String`
    |
help: try removing the method call
    |
103 -     let __jet_p: __jet_Parsed = __jet_Parsed { __jet_source: "key:value".to_string(), __jet_head: "key".to_string() };
103 +     let __jet_p: __jet_Parsed = __jet_Parsed { __jet_source: "key:value".to_string(), __jet_head: "key" };
    |

error[E0515]: cannot return value referencing function parameter `__jet_text`
  --> build/.work.p04i_selfref_move.1019136/p04i_selfref_move.rs:98:12
   |
97 |     let __jet_head: &str = jet_string_before_view(&(__jet_text), &":".to_string());
   |                                                   ------------- `__jet_text` is borrowed here
98 |     return __jet_Parsed { __jet_source: __jet_text, __jet_head: __jet_head };
   |            ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ returns a value referencing data owned by the current function

error[E0505]: cannot move out of `__jet_text` because it is borrowed
  --> build/.work.p04i_selfref_move.1019136/p04i_selfref_move.rs:98:41
   |
96 | pub fn __jet_parse<'__jet_view>(__jet_text: String) -> __jet_Parsed<'__jet_view> {
   |                    -----------  ---------- binding `__jet_text` declared here
   |                    |
   |                    lifetime `'__jet_view` defined here
97 |     let __jet_head: &str = jet_string_before_view(&(__jet_text), &":".to_string());
   |                                                   ------------- borrow of `__jet_text` occurs here
98 |     return __jet_Parsed { __jet_source: __jet_text, __jet_head: __jet_head };
   |            -----------------------------^^^^^^^^^^--------------------------
   |            |                            |
   |            |                            move out of `__jet_text` occurs here
   |            returning this value requires that `__jet_text` is borrowed for `'__jet_view`
   |
help: consider cloning the value if the performance cost is acceptable
   |
97 |     let __jet_head: &str = jet_string_before_view(&(__jet_text).clone(), &":".to_string());
   |                                                                ++++++++

error: aborting due to 3 previous errors

Some errors have detailed explanations: E0308, E0505, E0515.
For more information about an error, try `rustc --explain E0308`.
```

### p04j_caller_owned_cache

```jet
// owners live in caller scope; can bound views sit in a list?
fn run() {
    a :: "alpha:1"
    b :: "beta:2"
    ha :: a.before(":")
    hb :: b.before(":")
    cache :: [ha, hb]        // list literal of live views (tokenize-shaped)
    print(cache[0])
    print(cache[1])
}
```

**`jet run p04j_caller_owned_cache.jet`** — exit 1 (0.787s)

```
Error [E2307]: `ha` can't be used directly here yet
  --> /home/nate/.cache/jet-audit-scratch/p04j_caller_owned_cache.jet:7:15
    |
  7 |     cache :: [ha, hb]        // list literal of live views (tokenize-shaped)
    |               ^^
 Why: `ha` is a zero-copy view into `a` (`.trim()`/`.after()`/`.before()`); only chaining another `.trim()`/`.after()`/`.before()` on it works directly — other methods and calls need the full owned value
 Fix: write `~ha` first to get an owned `String`, then use that

Error [E2307]: `hb` can't be used directly here yet
  --> /home/nate/.cache/jet-audit-scratch/p04j_caller_owned_cache.jet:7:19
    |
  7 |     cache :: [ha, hb]        // list literal of live views (tokenize-shaped)
    |                   ^^
 Why: `hb` is a zero-copy view into `b` (`.trim()`/`.after()`/`.before()`); only chaining another `.trim()`/`.after()`/`.before()` on it works directly — other methods and calls need the full owned value
 Fix: write `~hb` first to get an owned `String`, then use that

Error [E2307]: `cache` can't be used directly here yet
  --> /home/nate/.cache/jet-audit-scratch/p04j_caller_owned_cache.jet:8:11
    |
  8 |     print(cache[0])
    |           ^^^^^
 Why: `cache` is a zero-copy view into `b` (`.trim()`/`.after()`/`.before()`); only chaining another `.trim()`/`.after()`/`.before()` on it works directly — other methods and calls need the full owned value
 Fix: write `~cache` first to get an owned `String`, then use that

Error [E2307]: `cache` can't be used directly here yet
  --> /home/nate/.cache/jet-audit-scratch/p04j_caller_owned_cache.jet:9:11
    |
  9 |     print(cache[1])
    |           ^^^^^
 Why: `cache` is a zero-copy view into `b` (`.trim()`/`.after()`/`.before()`); only chaining another `.trim()`/`.after()`/`.before()` on it works directly — other methods and calls need the full owned value
 Fix: write `~cache` first to get an owned `String`, then use that

4 problems found
run `jet explain E2307` to learn more
```

**`jet run --release p04j_caller_owned_cache.jet`** — exit 1 (26.91s)

```
Error [E2307]: `ha` can't be used directly here yet
  --> /home/nate/.cache/jet-audit-scratch/p04j_caller_owned_cache.jet:7:15
    |
  7 |     cache :: [ha, hb]        // list literal of live views (tokenize-shaped)
    |               ^^
 Why: `ha` is a zero-copy view into `a` (`.trim()`/`.after()`/`.before()`); only chaining another `.trim()`/`.after()`/`.before()` on it works directly — other methods and calls need the full owned value
 Fix: write `~ha` first to get an owned `String`, then use that

Error [E2307]: `hb` can't be used directly here yet
  --> /home/nate/.cache/jet-audit-scratch/p04j_caller_owned_cache.jet:7:19
    |
  7 |     cache :: [ha, hb]        // list literal of live views (tokenize-shaped)
    |                   ^^
 Why: `hb` is a zero-copy view into `b` (`.trim()`/`.after()`/`.before()`); only chaining another `.trim()`/`.after()`/`.before()` on it works directly — other methods and calls need the full owned value
 Fix: write `~hb` first to get an owned `String`, then use that

Error [E2307]: `cache` can't be used directly here yet
  --> /home/nate/.cache/jet-audit-scratch/p04j_caller_owned_cache.jet:8:11
    |
  8 |     print(cache[0])
    |           ^^^^^
 Why: `cache` is a zero-copy view into `b` (`.trim()`/`.after()`/`.before()`); only chaining another `.trim()`/`.after()`/`.before()` on it works directly — other methods and calls need the full owned value
 Fix: write `~cache` first to get an owned `String`, then use that

Error [E2307]: `cache` can't be used directly here yet
  --> /home/nate/.cache/jet-audit-scratch/p04j_caller_owned_cache.jet:9:11
    |
  9 |     print(cache[1])
    |           ^^^^^
 Why: `cache` is a zero-copy view into `b` (`.trim()`/`.after()`/`.before()`); only chaining another `.trim()`/`.after()`/`.before()` on it works directly — other methods and calls need the full owned value
 Fix: write `~cache` first to get an owned `String`, then use that

4 problems found
run `jet explain E2307` to learn more
```

### p04k_selfref_moved_again

```jet
// move the self-referential value after creation, then read the view
struct Parsed {
    source: String
    head: View<str>
}

fn parse(text: ^String) => Parsed {
    head :: text.before(":")
    return Parsed.{ source: text, head: head }
}

fn run() {
    input :: "key:value"
    p :: parse(^input)
    moved :: ^p          // relocate the struct; does head still point home?
    print(moved.head)
    print(moved.source)
}
```

**`jet run p04k_selfref_moved_again.jet`** — exit 1 (1.099s)

```
Error [E0003]: expected a value, found `^`
  --> /home/nate/.cache/jet-audit-scratch/p04k_selfref_moved_again.jet:15:14
    |
 15 |     moved :: ^p          // relocate the struct; does head still point home?
    |              ^
 Why: a value can be a name, a number, quoted text, `true`/`false`, or a call
 Fix: e.g. `x`, `42`, `3.5`, or `"hello"`

1 problem found
run `jet explain E0003` to learn more
```

### p05_arena_ok

```jet
// D-ALLOC1: four allocators, universal close(^)
use core.mem

fn run() {
    arena :: mem.Arena.new(capacity: 4096)
    x :: arena.alloc(42)
    print(x)
    arena.reset()
    y :: arena.alloc(99)
    print(y)
    bump :: mem.Bump.new(capacity: 64)
    b :: bump.alloc(7)
    print(b)
    fixed :: mem.Fixed.new(size: 256)
    f :: fixed.alloc(1)
    print(f)
    close(^arena)
    close(^bump)
    close(^fixed)
}
```

**`jet run p05_arena_ok.jet`** — exit 0 (0.789s)

```
42
99
7
1
```

**`jet run --release p05_arena_ok.jet`** — exit 0 (27.807s)

```
42
99
7
1
effects: IO
```

### p05b_arena_use_after_reset

```jet
// E0632 territory: read an arena value after reset
use core.mem

fn run() {
    arena :: mem.Arena.new(capacity: 1024)
    x :: arena.alloc(42)
    arena.reset()
    print(x)          // the arena's storage was recycled
}
```

**`jet run p05b_arena_use_after_reset.jet`** — exit 1 (0.781s)

```
Error [E0632]: `arena` was reset here, so the value `x` points into is gone
  --> /home/nate/.cache/jet-audit-scratch/p05b_arena_use_after_reset.jet:8:11
    |
  8 |     print(x)          // the arena's storage was recycled
    |           ^
 Why: `x` is a view into `arena`; `arena.reset()` invalidated everything stored in `arena`, so reading `x` now would read freed memory
 Fix: use `x` before `arena.reset()`, or re-`alloc` after the reset to get a fresh value

1 problem found
run `jet explain E0632` to learn more
```

### p05c_no_gate

```jet
// no 'use core.mem' — is the gate real? (expect E3102)
fn run() {
    arena :: mem.Arena.new(capacity: 1024)
    print(arena.alloc(1))
}
```

**`jet run p05c_no_gate.jet`** — exit 1 (0.773s)

```
Error [E0107]: nothing named `mem` exists here
  --> /home/nate/.cache/jet-audit-scratch/p05c_no_gate.jet:3:14
    |
  3 |     arena :: mem.Arena.new(capacity: 1024)
    |              ^^^
 Why: a name must be declared before it's used
 Fix: add `use core.mem as mem`

Error [E0311]: `alloc` isn't a method on this value
  --> /home/nate/.cache/jet-audit-scratch/p05c_no_gate.jet:4:17
    |
  4 |     print(arena.alloc(1))
    |                 ^^^^^
 Why: only struct and enum values have instance methods
 Fix: check the spelling of `alloc`

2 problems found
run `jet explain E0107` to learn more
```

### p05d_use_after_close

```jet
use core.mem

fn run() {
    arena :: mem.Arena.new(capacity: 1024)
    x :: arena.alloc(42)
    close(^arena)
    print(x)          // storage gone — expect compile rejection
}
```

**`jet run p05d_use_after_close.jet`** — exit 1 (0.789s)

```
Error [E0220]: `arena` cannot be read while `x` has a live exclusive write window into it
  --> /home/nate/.cache/jet-audit-scratch/p05d_use_after_close.jet:6:12
    |
  6 |     close(^arena)
    |            ^^^^^
 Why: `x` is an exclusive window into `arena[fresh]`; reading the owner beside that window would be rejected after lowering
 Fix: read or edit through `x` instead of `arena`

Error [E0212]: `arena` cannot be moved while `x` is still looking into it
  --> /home/nate/.cache/jet-audit-scratch/p05d_use_after_close.jet:6:12
    |
  6 |     close(^arena)
    |            ^^^^^
 Why: `x` is a live exclusive mutable view into `arena[fresh]`; changing or moving the owner could invalidate that view
 Fix: finish using `x` before changing `arena`, narrow the view's scope, or make an owned copy

2 problems found
run `jet explain E0220` to learn more
```

### p05e_region_explicit

```jet
// D-REGION1 explicit region block
use core.mem

fn run() {
    #Region(r) {
        scratch :: r.alloc("inside region")
        print(scratch)
    }
}
```

**`jet run p05e_region_explicit.jet`** — exit 1 (0.775s)

```
Error [E0107]: nothing named `r` exists here
  --> /home/nate/.cache/jet-audit-scratch/p05e_region_explicit.jet:6:20
    |
  6 |         scratch :: r.alloc("inside region")
    |                    ^
 Why: a name must be declared before it's used
 Fix: declare it first: `r :: ...`

1 problem found
run `jet explain E0107` to learn more
```

### p05f_region_escape

```jet
// value must not escape its region
use core.mem

fn run() {
    escaped := ""
    #Region(r) {
        scratch :: r.alloc("inside region")
        escaped = scratch      // escape attempt
    }
    print(escaped)
}
```

**`jet run p05f_region_escape.jet`** — exit 1 (0.769s)

```
Error [E0107]: nothing named `r` exists here
  --> /home/nate/.cache/jet-audit-scratch/p05f_region_escape.jet:7:20
    |
  7 |         scratch :: r.alloc("inside region")
    |                    ^
 Why: a name must be declared before it's used
 Fix: declare it first: `r :: ...`

Error [E0108]: `escaped` holds String (text), but this value is Int (a whole number)
  --> /home/nate/.cache/jet-audit-scratch/p05f_region_escape.jet:8:19
    |
  8 |         escaped = scratch      // escape attempt
    |                   ^^^^^^^
 Why: a binding keeps one type for its whole life
 Fix: put the value in text with interpolation: "{x}"

2 problems found
run `jet explain E0107` to learn more
```

### p05g_repo_arena_regions

```jet
// D-ALLOC2 / D-REGION1 (ratified 2026-06-21): real bump-allocated arenas with
// scope-bound regions. `arena.alloc(value)` hands back a *view* into the
// arena's storage — readable and writable, but it may not escape the arena's
// region (the lexical scope of the `arena` binding, or an explicit `region`),
// and it may not be used after the arena is reset or closed. Both rules are
// compile-time: a use-after-free is an error, never a runtime trap.
use core.mem

fn run() {
    // Implicit region: the region is the scope of the `arena` binding.
    arena :: mem.Arena.new()
    first :: arena.alloc(10)
    second :: arena.alloc(20)
    print(first)
    print(second)
    // reset keeps the backing buffer; everything allocated so far is gone, so
    // `first`/`second` may not be touched after this point (that would be
    // E0632). A fresh alloc after the reset is fine.
    arena.reset()
    reused :: arena.alloc(30)
    print(reused)
    // Explicit `region r { … }` (D-REGION1 opt B): a narrower, named region
    // spanning two allocators. Views made inside live only until the block ends.
    #Region(scratch) {
        a :: mem.Arena.new()
        b :: mem.Bump.new()
        x :: a.alloc(1)
        y :: b.alloc(2)
        print(x)
        print(y)
    }
    print(99)
}
// Memory safety, enforced at compile time — none of these would compile:
//   return first            // E0631: a view can't be returned (it would
//                           //        outlive `arena`)
//   stash :: first          // E0631: a view can't be stored in another
//                           //        binding that may outlive the region
//   arena.reset(); print(reused)  // (after reset) E0632: reading a view
//                           //        whose arena was reset reads freed memory
```

**`jet run p05g_repo_arena_regions.jet`** — exit 0 (0.809s)

```
10
20
30
1
2
99
```

**`jet run --release p05g_repo_arena_regions.jet`** — exit 0 (28.335s)

```
10
20
30
1
2
99
effects: IO
```

### p05h_arena_view_escape

```jet
// return a view of arena storage — expect E0631
use core.mem

fn grab() => Int {
    arena :: mem.Arena.new()
    v :: arena.alloc(10)
    return v      // view would outlive arena
}

fn run() {
    print(grab())
}
```

**`jet run p05h_arena_view_escape.jet`** — exit 1 (0.79s)

```
Error [E0631]: `v` cannot be shared — it does not live long enough to be returned
  --> /home/nate/.cache/jet-audit-scratch/p05h_arena_view_escape.jet:7:12
    |
  7 |     return v      // view would outlive arena
    |            ^
 Why: `v` is a view into `arena`; sharing it outside the region would let it outlive `arena` and point into freed memory
 Fix: keep `v` inside the `arena` region, or copy what you need out with `~` before it leaves

1 problem found
run `jet explain E0631` to learn more
```

### p06_no_alloc_violation

```jet
// D-MEM-FACTS1: transitive no_alloc, violated two calls deep
#Policy(no_alloc)

fn label(n: Int) => String = "value {n}"     // interpolation allocates

fn describe(n: Int) => String = label(n)

fn run() {
    print(describe(7))
}
```

**`jet run p06_no_alloc_violation.jet`** — exit 1 (0.781s)

```
Error [E0921]: string interpolation allocates a new `String` at /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet violates the effective `no_alloc` declared at /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet
  --> /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet:4:30
    |
  4 | fn label(n: Int) => String = "value {n}"     // interpolation allocates
    |                              ^^^^^^^^^^^
 Why: /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet is reachable through p06_no_alloc_violation::label from code governed by `no_alloc`; declaration provenance: no_alloc = true
  module true at /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet:62..79; operation provenance: label in /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet
 Fix: remove or replace the incompatible operation, call an implementation whose transitive memory facts satisfy the contract, or move the call outside this policy scope

Error [E0921]: string interpolation allocates a new `String` at /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet violates the effective `no_alloc` declared at /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet
  --> /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet:4:30
    |
  4 | fn label(n: Int) => String = "value {n}"     // interpolation allocates
    |                              ^^^^^^^^^^^
 Why: /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet is reachable through p06_no_alloc_violation::describe -> p06_no_alloc_violation::label from code governed by `no_alloc`; declaration provenance: no_alloc = true
  module true at /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet:62..79; operation provenance: label in /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet
 Fix: remove or replace the incompatible operation, call an implementation whose transitive memory facts satisfy the contract, or move the call outside this policy scope

Error [E0921]: string interpolation allocates a new `String` at /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet violates the effective `no_alloc` declared at /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet
  --> /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet:4:30
    |
  4 | fn label(n: Int) => String = "value {n}"     // interpolation allocates
    |                              ^^^^^^^^^^^
 Why: /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet is reachable through p06_no_alloc_violation::run -> p06_no_alloc_violation::describe -> p06_no_alloc_violation::label from code governed by `no_alloc`; declaration provenance: no_alloc = true
  module true at /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet:62..79; operation provenance: label in /home/nate/.cache/jet-audit-scratch/p06_no_alloc_violation.jet
 Fix: remove or replace the incompatible operation, call an implementation whose transitive memory facts satisfy the contract, or move the call outside this policy scope

3 problems found
run `jet explain E0921` to learn more
```

### p06b_no_alloc_ok

```jet
// scalar-only bodies prove the fact
#Policy(no_alloc)

struct Entity { pos: Float, vel: Float }

fn integrate(e: &Entity, dt: Float) { e.pos += e.vel * dt }

fn run() {
    e := Entity.{ pos: 0.0, vel: 1.0 }
    integrate(&e, 2.0)
    print("integrated ok")
}
```

**`jet run p06b_no_alloc_ok.jet`** — exit 0 (0.783s)

```
integrated ok
```

**`jet run --release p06b_no_alloc_ok.jet`** — exit 0 (27.642s)

```
integrated ok
Warning [L0502] (float_comparison): comparing floats with `==` is unreliable
  --> /home/nate/.cache/jet-audit-scratch/p06b_no_alloc_ok.jet:4:19
    |
  4 | struct Entity { pos: Float, vel: Float }
    |                   ^^^^^^^^^^^^^^
 Why: floating-point arithmetic is inexact; two values computed differently may not be bit-identical even when mathematically equal
 Fix: compare within a tolerance: `(a - b).abs() < 1e-9`

Warning [L0502] (float_comparison): comparing floats with `==` is unreliable
  --> /home/nate/.cache/jet-audit-scratch/p06b_no_alloc_ok.jet:5:1
    |
  5 | 
    | ^
 Why: floating-point arithmetic is inexact; two values computed differently may not be bit-identical even when mathematically equal
 Fix: compare within a tolerance: `(a - b).abs() < 1e-9`

2 warnings emitted (compilation continues)
effects: IO
```

### p06c_block_scope_fact

```jet
// block-scope fact on the D-MARK-SCOPE1 ladder
fn run() {
    greeting :: "hello {1 + 1}"
    print(greeting)
    #Policy(no_alloc) {
        x := 1
        x += 1
        bad :: "oops {x}"     // allocation inside the no_alloc block
        print(bad)
    }
}
```

**`jet run p06c_block_scope_fact.jet`** — exit 1 (0.776s)

```
Error [E0921]: string interpolation allocates a new `String` at /home/nate/.cache/jet-audit-scratch/p06c_block_scope_fact.jet violates the effective `no_alloc` declared at /home/nate/.cache/jet-audit-scratch/p06c_block_scope_fact.jet
  --> /home/nate/.cache/jet-audit-scratch/p06c_block_scope_fact.jet:8:16
    |
  8 |         bad :: "oops {x}"     // allocation inside the no_alloc block
    |                ^^^^^^^^^^
 Why: /home/nate/.cache/jet-audit-scratch/p06c_block_scope_fact.jet is reachable through p06c_block_scope_fact::run#policy@115..259 from code governed by `no_alloc`; declaration provenance: no_alloc = true
  block true at <source>:115..132; operation provenance: run block policy in /home/nate/.cache/jet-audit-scratch/p06c_block_scope_fact.jet
 Fix: remove or replace the incompatible operation, call an implementation whose transitive memory facts satisfy the contract, or move the call outside this policy scope

1 problem found
run `jet explain E0921` to learn more
```

### p07_unsafe_ok

```jet
// D-CAP9 raw pointers inside a reason-gated #Unsafe block
use core.mem

fn run() {
    cell :: 1337
    #Unsafe("cell is a live Int on this stack frame; the pointer never escapes") {
        p :: *Int.{*cell}
        print(p.*)
    }
}
```

**`jet run p07_unsafe_ok.jet`** — exit 0 (0.777s)

```
1337
```

**`jet run --release p07_unsafe_ok.jet`** — exit 0 (27.536s)

```
1337
effects: IO
```

### p07b_bare_unsafe

```jet
// D-UNSAFE-REASON1: bare #Unsafe must be a hard error (E3112)
use core.mem

fn run() {
    cell :: 1337
    #Unsafe {
        p :: *Int.{*cell}
        print(p.*)
    }
}
```

**`jet run p07b_bare_unsafe.jet`** — exit 1 (0.747s)

```
Error [E3112]: an `#Unsafe` block needs a reason
  --> /home/nate/.cache/jet-audit-scratch/p07b_bare_unsafe.jet:6:5
    |
  6 |     #Unsafe {
    |     ^^^^^^^
 Why: every unsafe gate records why its unchecked operations preserve memory safety
 Fix: write `#Unsafe("why this is safe") { … }`

1 problem found
run `jet explain E3112` to learn more
```

### p07c_rawptr_no_gate

```jet
// raw pointer outside any #Unsafe — expect rejection
use core.mem

fn run() {
    cell :: 1337
    p :: *Int.{*cell}
    print(p.*)
}
```

**`jet run p07c_rawptr_no_gate.jet`** — exit 1 (0.776s)

```
Error [E0208]: taking a raw pointer requires `#Unsafe`
  --> /home/nate/.cache/jet-audit-scratch/p07c_rawptr_no_gate.jet:6:10
    |
  6 |     p :: *Int.{*cell}
    |          ^^^^^^^^^^^^
 Why: `*x` takes a raw pointer to `x`; that is a raw memory operation, only valid inside a `#Unsafe { … }` region
 Fix: wrap this in `#Unsafe("why this is safe") { … }` — to dereference a pointer use postfix `p.*`

Error [E0208]: taking a raw pointer requires `#Unsafe`
  --> /home/nate/.cache/jet-audit-scratch/p07c_rawptr_no_gate.jet:6:16
    |
  6 |     p :: *Int.{*cell}
    |                ^^^^^
 Why: `*x` takes a raw pointer to `x`; that is a raw memory operation, only valid inside a `#Unsafe { … }` region
 Fix: wrap this in `#Unsafe("why this is safe") { … }` — to dereference a pointer use postfix `p.*`

Error [E0208]: reading through a raw pointer requires `#Unsafe`
  --> /home/nate/.cache/jet-audit-scratch/p07c_rawptr_no_gate.jet:7:11
    |
  7 |     print(p.*)
    |           ^^^
 Why: `p.*` dereferences a raw pointer; that is a raw memory access, only valid inside a `#Unsafe { … }` region
 Fix: wrap this in `#Unsafe("why this is safe") { … }`

3 problems found
run `jet explain E0208` to learn more
```

### p07d_inspect_unsafe

```jet
(command probe — no program; runs `jet inspect unsafe` on p07_unsafe_ok.jet)
```

**`jet inspect unsafe p07_unsafe_ok.jet`** — exit 0

```
unsafe gates: 1
/home/nate/.cache/jet-audit-scratch/p07_unsafe_ok.jet:6:5  GateOnly  reason=cell is a live Int on this stack frame; the pointer never escapes
  /home/nate/.cache/jet-audit-scratch/p07_unsafe_ok.jet:7:14  raw_pointer  discharged  required=[no_alias] asserted=[]
  /home/nate/.cache/jet-audit-scratch/p07_unsafe_ok.jet:7:20  raw_pointer  discharged  required=[no_alias] asserted=[]
  /home/nate/.cache/jet-audit-scratch/p07_unsafe_ok.jet:8:15  dereference  discharged  required=[valid_ptr,aligned] asserted=[]
```

### p08_ffi_borrowed_param

```jet
// E0702: borrowed parameters cannot cross the FFI boundary
extern rust "std" {
    fn bad(s: &String) => String = "std::convert::identity"
}

fn run() {
    print(bad("hi"))
}
```

**`jet run p08_ffi_borrowed_param.jet`** — exit 1 (0.769s)

```
Error [E0702]: `s` can't use `&` at the FFI boundary
  --> /home/nate/.cache/jet-audit-scratch/p08_ffi_borrowed_param.jet:3:12
    |
  3 |     fn bad(s: &String) => String = "std::convert::identity"
    |            ^
 Why: foreign functions take owned copies — the write-capability marker `&` and move-capability marker `^` aren't allowed here
 Fix: remove the capability sigil and pass by value

Error [E0102]: nothing named `bad` exists here
  --> /home/nate/.cache/jet-audit-scratch/p08_ffi_borrowed_param.jet:7:11
    |
  7 |     print(bad("hi"))
    |           ^^^
 Why: only functions that have been defined (or built in, like `print` / `input`) can be called
 Fix: define it first (fn bad() { ... }), or call one that exists

2 problems found
run `jet explain E0702` to learn more
```

### p08b_ffi_by_value_ok

```jet
// by-value crossing works: Jet owns its copy, Rust owns its copy
extern rust "std" {
    fn identity(s: String) => String = "std::convert::identity"
}

fn run() {
    print(identity("round trip"))
}
```

**`jet run p08b_ffi_by_value_ok.jet`** — exit 0 (1.088s)

```
round trip
```

### p09_race_rejected

```jet
// two concurrent branches mutating one plain local — must be rejected
struct Counter { hits: Int }

fn run() {
    counter := Counter.{ hits: 0 }
    task.group g {
        results :: task.all { { counter.hits += 1 }, { counter.hits += 1 } }
        print(results[0])
    }
    print(counter.hits)
}
```

**`jet run p09_race_rejected.jet`** — exit 1 (0.789s)

```
Error [E1101]: `counter` is a mutable value — the new task might outlive this scope
  --> /home/nate/.cache/jet-audit-scratch/p09_race_rejected.jet:7:31
    |
  7 |         results :: task.all { { counter.hits += 1 }, { counter.hits += 1 } }
    |                               ^^^^^^^^^^^^^^^^^^^^^
 Why: tasks run concurrently; changing an outer binding inside a task would make ownership unclear
 Fix: create task-local state inside the task, or send updates through a channel

Error [E1101]: `counter` is a mutable value — the new task might outlive this scope
  --> /home/nate/.cache/jet-audit-scratch/p09_race_rejected.jet:7:54
    |
  7 |         results :: task.all { { counter.hits += 1 }, { counter.hits += 1 } }
    |                                                      ^^^^^^^^^^^^^^^^^^^^^
 Why: tasks run concurrently; changing an outer binding inside a task would make ownership unclear
 Fix: create task-local state inside the task, or send updates through a channel

Error [E0112]: `print` doesn't know how to show `Unit`
  --> /home/nate/.cache/jet-audit-scratch/p09_race_rejected.jet:8:22
    |
  8 |         print(results[0])
    |                      ^^^
 Why: print shows values that have a display
 Fix: print one of its parts instead

3 problems found
run `jet explain E1101` to learn more
```

### p09b_view_across_task

```jet
// a view crossing a task boundary — expect E1102
use core.tasks as tasks

fn run() {
    text :: "alpha:beta"
    head :: text.before(":")
    t :: tasks.spawn(() => print(head))
    t.wait()
}
```

**`jet run p09b_view_across_task.jet`** — exit 1 (0.786s)

```
Error [E1004]: `core.tasks` has no item `spawn`
  --> /home/nate/.cache/jet-audit-scratch/p09b_view_across_task.jet:7:16
    |
  7 |     t :: tasks.spawn(() => print(head))
    |                ^^^^^
 Why: standard library modules expose a fixed set of public items
 Fix: use one of: join_all, wait_any, yield_now, current_task, channel, after, interval

Error [E2307]: `head` can't be captured by a stored lambda yet
  --> /home/nate/.cache/jet-audit-scratch/p09b_view_across_task.jet:7:22
    |
  7 |     t :: tasks.spawn(() => print(head))
    |                      ^^^^^^^^^^^
 Why: `head` is a zero-copy view into `text` (`.trim()`/`.after()`/`.before()`); only chaining another `.trim()`/`.after()`/`.before()` on it works directly — other methods and calls need the full owned value
 Fix: write `~head` first to get an owned `String`, then use that

Error [E0311]: `wait` isn't a method on this value
  --> /home/nate/.cache/jet-audit-scratch/p09b_view_across_task.jet:8:7
    |
  8 |     t.wait()
    |       ^^^^
 Why: only struct and enum values have instance methods
 Fix: check the spelling of `wait`

3 problems found
run `jet explain E1004` to learn more
```

### p09c_shared_ok

```jet
// sanctioned cross-task mutation: Shared<T> closures
struct Counter { hits: Int }

fn bump(c: Shared<Counter>) { c.edit(v => { v.hits += 1 }) }

fn run() {
    counter :: Shared.new(Counter.{ hits: 0 })
    task.group g {
        done :: task.all { bump(counter), bump(counter) }
        print("both done")
    }
    print(counter.read(v => v.hits))
}
```

**`jet run p09c_shared_ok.jet`** — exit 0 (0.805s)

```
both done
2
```

**`jet run --release p09c_shared_ok.jet`** — exit 0 (30.859s)

```
both done
2
effects: IO
```

### p09d_repo_shared_config

```jet
// D-MEM1 stage S6 (D-SHARED-API1=A): `Shared<T>` is a lock-guarded shared
// handle — "a copyable door". `Shared.new(x)` constructs (bare type-name call,
// `T` inferred from `x`); `.read(f)`/`.edit(f)` run a closure against a read-
// or write-locked view of the wrapped value, the lock scoped to the closure
// call only. Cloning `Shared<T>` is always a cheap handle clone, never a deep
// copy of `T` — that's what lets it cross a `tasks.spawn` boundary with no
// `take`, unlike an ordinary struct.
use core.tasks as tasks

struct AppConfig {
    name: String
    hits: Int
}

fn handle(id: Int, config: Shared<AppConfig>) => String {
    label :: config.read(c => c.name)
    return "request {id} on {label}"
}

fn run() {
    config :: Shared.new(AppConfig.{name: "jet-server", hits: 0})
    // Every spawned task captures `config` with no `take` — a `Shared<T>` is
    // meant to be handed to as many concurrent tasks as needed.
    t1 :: tasks.spawn(() => handle(1, config))
    t2 :: tasks.spawn(() => handle(2, config))
    t3 :: tasks.spawn(() => handle(3, config))
    print(t1.wait())
    print(t2.wait())
    print(t3.wait())
    config.edit(c => { c.hits += 1 })
    total :: config.read(c => c.hits)
    print("hits={total}")
}
```

**`jet run p09d_repo_shared_config.jet`** — exit 1 (0.806s)

```
Error [E1004]: `core.tasks` has no item `spawn`
  --> /home/nate/.cache/jet-audit-scratch/p09d_repo_shared_config.jet:24:17
    |
 24 |     t1 :: tasks.spawn(() => handle(1, config))
    |                 ^^^^^
 Why: standard library modules expose a fixed set of public items
 Fix: use one of: join_all, wait_any, yield_now, current_task, channel, after, interval

Error [E1004]: `core.tasks` has no item `spawn`
  --> /home/nate/.cache/jet-audit-scratch/p09d_repo_shared_config.jet:25:17
    |
 25 |     t2 :: tasks.spawn(() => handle(2, config))
    |                 ^^^^^
 Why: standard library modules expose a fixed set of public items
 Fix: use one of: join_all, wait_any, yield_now, current_task, channel, after, interval

Error [E1004]: `core.tasks` has no item `spawn`
  --> /home/nate/.cache/jet-audit-scratch/p09d_repo_shared_config.jet:26:17
    |
 26 |     t3 :: tasks.spawn(() => handle(3, config))
    |                 ^^^^^
 Why: standard library modules expose a fixed set of public items
 Fix: use one of: join_all, wait_any, yield_now, current_task, channel, after, interval

Error [E0311]: `wait` isn't a method on this value
  --> /home/nate/.cache/jet-audit-scratch/p09d_repo_shared_config.jet:27:14
    |
 27 |     print(t1.wait())
    |              ^^^^
 Why: only struct and enum values have instance methods
 Fix: check the spelling of `wait`

Error [E0311]: `wait` isn't a method on this value
  --> /home/nate/.cache/jet-audit-scratch/p09d_repo_shared_config.jet:28:14
    |
 28 |     print(t2.wait())
    |              ^^^^
 Why: only struct and enum values have instance methods
 Fix: check the spelling of `wait`

Error [E0311]: `wait` isn't a method on this value
  --> /home/nate/.cache/jet-audit-scratch/p09d_repo_shared_config.jet:29:14
    |
 29 |     print(t3.wait())
    |              ^^^^
 Why: only struct and enum values have instance methods
 Fix: check the spelling of `wait`

6 problems found
run `jet explain E1004` to learn more
```

### p09e_cell_guard_conflict

```jet
// Cell: dynamic loan conflict panics at runtime (compile passes)
fn run() {
    cell :: Cell.new(10)
    g1 :: cell.guard_edit()
    print(cell.get())         // read while an edit guard is live
}
```

**`jet run p09e_cell_guard_conflict.jet`** — exit 70 (0.781s)

```
panic: Cell borrow conflict: cannot read while an edit guard is active
```

### p10_freestanding_alloc

```jet
fn run() {
    parts :: ["a", "b"]
    joined :: "{parts[0]}{parts[1]}"   // heap allocation
    print(joined)
}
```

**`jet build --freestanding p10_freestanding_alloc.jet`** — exit 0

```
built: build/p10_freestanding_alloc
capabilities: none
effects: IO
```

### p10b_freestanding_os

```jet
use core.files as files

fn run() {
    text :: files.read("hello.txt") ?? "missing"
    print(text)
}
```

**`jet build --freestanding p10b_freestanding_os.jet`** — exit 0

```
built: build/p10b_freestanding_os
capabilities: file-io
effects: FS, IO
```

### p11_pool_stale_id

```jet
// D-MEM1 stage S6 (D-POOLID-API1=A): a stale `Id<T>` — its pool slot was
// removed — panics at runtime, mirroring the array-out-of-bounds panic
// precedent (jet_index_vec/jet_pool_get): a runtime report, not a new
// diagnostic code.
struct Player {
    name: String
}

fn run() {
    world := Pool<Player>.new()
    kai :: world.add(.{name: "Kai"})
    world.remove(kai).drop("this example only needs the generation bump, not the removed value")
    print(world[kai].name)
}
```

**`jet run p11_pool_stale_id.jet`** — exit 1 (0.781s)

```
Error [E0119]: `.{ … }` needs a known struct type here
  --> /home/nate/.cache/jet-audit-scratch/p11_pool_stale_id.jet:11:22
    |
 11 |     kai :: world.add(.{name: "Kai"})
    |                      ^^^^^^^^^^^^^^
 Why: the inferred construction form requires an expected type from the surrounding context (binding annotation, return type, etc.)
 Fix: add a type annotation, e.g. `x: Point :: .{ x: 1, y: 2 }`

Error [E0302]: `.name` only works on struct and tuple values
  --> /home/nate/.cache/jet-audit-scratch/p11_pool_stale_id.jet:13:22
    |
 13 |     print(world[kai].name)
    |                      ^^^^
 Why: enums and other values use methods or pattern tests instead
 Fix: use a struct or tuple value before `.name`

2 problems found
run `jet explain E0119` to learn more
```

### p11b_gc_cycle

```jet
// D-OPTGC1=A: scoped promotion traces nested ownership and cyclic mutations
// while the source keeps ordinary bare-value syntax.
#Policy(gc)

enum Link {
    End(Int)
    Next(Link)
}

fn promoted_cycle() => Link {
    // The payloads remain finite value snapshots. Their promoted identities
    // form the cycle: second -> first at construction, then first -> second.
    first := Link.End(1)
    second :: Link.Next(~first)
    first = Link.Next(~second)
    return first
}

fn promoted_replacement() => Link {
    stale :: Link.End(2)
    current := Link.Next(~stale)
    fresh :: Link.End(3)
    // Whole-value replacement removes current -> stale before adding
    // current -> fresh; the trace must not retain the stale identity.
    current = Link.Next(~fresh)
    return current
}

fn run() {
    cycle :: promoted_cycle()
    print(cycle)
    replacement :: promoted_replacement()
    print(replacement)
}
```

**`jet run p11b_gc_cycle.jet`** — exit 1 (0.799s)

```
Error [E0361]: `equal` calls itself through `==`
  --> /home/nate/.cache/jet-audit-scratch/p11b_gc_cycle.jet:11:58
    |
 11 |     // The payloads remain finite value snapshots. Their promoted identities
    |                                                          ^^^^^^^^^^^^^^^^^
 Why: the operator symbol dispatches back to this same hook, so evaluation would recurse forever
 Fix: combine the value's fields or call a different named helper inside the hook

1 problem found
run `jet explain E0361` to learn more
```

**`jet run --release p11b_gc_cycle.jet`** — exit 1 (26.303s)

```
Error [E0361]: `equal` calls itself through `==`
  --> /home/nate/.cache/jet-audit-scratch/p11b_gc_cycle.jet:11:58
    |
 11 |     // The payloads remain finite value snapshots. Their promoted identities
    |                                                          ^^^^^^^^^^^^^^^^^
 Why: the operator symbol dispatches back to this same hook, so evaluation would recurse forever
 Fix: combine the value's fields or call a different named helper inside the hook

1 problem found
run `jet explain E0361` to learn more
```

### p12_map_add_tier_divergence

```jet
// card #1883: Map.add's return value on default tier vs AOT
fn run() {
    counts := ["a": 1]
    prev :: counts.add("a", 2)     // should return the previous value (1) as Int?
    print(prev)
    print(counts["a"])
}
```

**`jet run p12_map_add_tier_divergence.jet`** — exit 0 (0.765s)

```
0
2
```

**`jet run --release p12_map_add_tier_divergence.jet`** — exit 0 (28.68s)

```
1
2
effects: IO
```

### p13_uninit_partial_read

```jet
// D-UNINIT-SENTINEL2: reading a slot before every slot is written
use core.mem

fn run() {
    bytes := [U8#4].{ uninit }
    bytes[0] = 65
    bytes[1] = 66
    print(bytes[3])     // slot 3 never written
}
```

**`jet run p13_uninit_partial_read.jet`** — exit 1 (0.779s)

```
Error [E0420]: `bytes` may be read before it is given a value
  --> /home/nate/.cache/jet-audit-scratch/p13_uninit_partial_read.jet:8:11
    |
  8 |     print(bytes[3])     // slot 3 never written
    |           ^^^^^
 Why: `bytes` was declared with `Type.{ uninit }`, so no value is available until you write to it — this read could see garbage
 Fix: write to `bytes` on every path before reading it (e.g. fill it via `mut bytes`)

1 problem found
run `jet explain E0420` to learn more
```

### p13b_fixed_over

```jet
// D-FIXED-BACKING1: Fixed.over borrows caller storage
use core.mem

fn run() {
    bytes := [U8#128].{ uninit }
    fixed :: mem.Fixed.over(&bytes)
    value :: fixed.alloc(9)
    print(value)
    close(^fixed)
}
```

**`jet run p13b_fixed_over.jet`** — exit 0 (0.787s)

```
9
```

**`jet run --release p13b_fixed_over.jet`** — exit 0 (27.551s)

```
9
effects: IO
```

### p13c_zero_rc_violation

```jet
// D-MEM-FACTS1: zero_rc fact vs Shared<T> (refcounted door)
#Policy(zero_rc)

struct Config { hits: Int }

fn run() {
    c :: Shared.new(Config.{ hits: 0 })
    print(c.read(v => v.hits))
}
```

**`jet run p13c_zero_rc_violation.jet`** — exit 1 (0.776s)

```
Error [E0921]: `Shared.new` introduces reference-counted ownership at /home/nate/.cache/jet-audit-scratch/p13c_zero_rc_violation.jet violates the effective `zero_rc` declared at /home/nate/.cache/jet-audit-scratch/p13c_zero_rc_violation.jet
  --> /home/nate/.cache/jet-audit-scratch/p13c_zero_rc_violation.jet:7:17
    |
  7 |     c :: Shared.new(Config.{ hits: 0 })
    |                 ^^^
 Why: /home/nate/.cache/jet-audit-scratch/p13c_zero_rc_violation.jet is reachable through p13c_zero_rc_violation::run from code governed by `zero_rc`; declaration provenance: zero_rc = true
  module true at /home/nate/.cache/jet-audit-scratch/p13c_zero_rc_violation.jet:61..77; operation provenance: run in /home/nate/.cache/jet-audit-scratch/p13c_zero_rc_violation.jet
 Fix: remove or replace the incompatible operation, call an implementation whose transitive memory facts satisfy the contract, or move the call outside this policy scope

1 problem found
run `jet explain E0921` to learn more
```

### p14_wasm_target

```jet
fn run() { print("hello wasm") }
```

**`jet build --target=wasm32-unknown-unknown p14_wasm.jet`** — exit 0

```
built: build/p14_wasm
target: wasm32-unknown-unknown
capabilities: none
effects: IO
```

### p15_repo_string_view

```jet
// D-MEM1 stage S5 (2026-07-04): `.trim()`/`.after(sep)`/`.before(sep)` bound to
// a local return a zero-copy view — a genuine `&str` borrow into the receiver's
// own buffer, not a fresh allocation — whenever sema can prove the binding
// can't outlive its owner (E2307 otherwise). `String` stays the one Jet-level
// string type end to end (D-MEM1 gallery): the view is an internal
// representation choice, invisible in the surface syntax.
fn run() {
    padded :: "  nate@jet.dev  "
    email :: padded.trim()
    domain :: email.after("@")
    user :: email.before("@")
    print("user={user} domain={domain}")
    // The view borrows `padded`'s buffer; reading `padded` again afterward
    // still works (this is a read, not a move) — proving the view didn't
    // take ownership away from its owner.
    print("padded still readable: {padded}")
    // `email` is itself a local, type-invisible view (`padded.trim()`). `~`
    // materializes an owned String for an owned-String boundary. Named
    // zero-copy return/field boundaries use `View<str>` instead; see
    // returned_views.jet. E2307 rejects an owner that dies too soon.
    print("escaped copy={domain_of(~email)}")
}

fn domain_of(s: String) => String {
    d :: s.after("@")
    return ~d
}

#Bench("string view: after/before/trim, no owning materialization") {
    email :: "nate@jet.dev"
    loop i, 0..1000 {
        d :: email.after("@")
        s :: "{d}"
        require_eq(s, "jet.dev")
    }
}
```

**`jet run p15_repo_string_view.jet`** — exit 0 (0.798s)

```
user=nate domain=jet.dev
padded still readable:   nate@jet.dev  
escaped copy=jet.dev
```

**`jet run --release p15_repo_string_view.jet`** — exit 0 (28.808s)

```
user=nate domain=jet.dev
padded still readable:   nate@jet.dev  
escaped copy=jet.dev
effects: IO
```

### p15b_repo_place_windows

```jet
// D-SHAPE-PLACE1=A: a particle/grid update using bare reads, `&` edits,
// and `~` copies. The two constant particle indexes are provably disjoint,
// so Jet lowers their live edit windows through a safe structural split.
struct Particle {
    position: Int
    velocity: Int
}

struct Tile {
    force: Int
}

fn run() {
    particles := [Particle].{
        .{position: 10, velocity: 2},
        .{position: 20, velocity: 3},
        .{position: 30, velocity: 4}
    }
    grid :: [Tile].{
        .{force: 5},
        .{force: 7},
        .{force: 11}
    }

    forces :: grid[0..2]
    middle_before :: ~particles[1]
    left :: &particles[0]
    right :: &particles[2]
    left.velocity += forces[0].force
    right.velocity += forces[2].force
    left.position += left.velocity
    right.position += right.velocity
    print("{left.position},{middle_before.position},{right.position}")
}
```

**`jet run p15b_repo_place_windows.jet`** — exit 1 (0.787s)

```
Error [E0220]: `particles` cannot be read while `left` has a live exclusive write window into it
  --> /home/nate/.cache/jet-audit-scratch/p15b_repo_place_windows.jet:28:15
    |
 28 |     right :: &particles[2]
    |               ^^^^^^^^^
 Why: `left` is an exclusive window into `particles[…]`; reading the owner beside that window would be rejected after lowering
 Fix: read or edit through `left` instead of `particles`

1 problem found
run `jet explain E0220` to learn more
```

**`jet run --release p15b_repo_place_windows.jet`** — exit 1 (26.932s)

```
Error [E0220]: `particles` cannot be read while `left` has a live exclusive write window into it
  --> /home/nate/.cache/jet-audit-scratch/p15b_repo_place_windows.jet:28:15
    |
 28 |     right :: &particles[2]
    |               ^^^^^^^^^
 Why: `left` is an exclusive window into `particles[…]`; reading the owner beside that window would be rejected after lowering
 Fix: read or edit through `left` instead of `particles`

1 problem found
run `jet explain E0220` to learn more
```

### p16_repo_owner_backed_views

```jet
// #1163 / D-MEM-VIEWRET1=B: an owner-backed collection keeps the list in a
// named field. Callers borrow element windows through `View` / `ViewMut` with
// public provenance — no lifetime syntax, no second view mechanism.
// Jet range windows are inclusive, so one element is `i..i`.
struct Book {
    title: String
    pages: Int
}

struct Library {
    books: [Book]
}

fn book_at(lib: Library, i: Int) => View<Book> = lib.books[i..i]

fn edit_at(lib: &Library, i: Int) => ViewMut<Book> {
    return &lib.books[i..i]
}

fn run() {
    lib := Library.{
        books: [
            Book.{title: "Dune", pages: 412},
            Book.{title: "Neuromancer", pages: 271}
        ]
    }
    first :: book_at(lib, 0)
    print(first[0].title)
    print(first[0].pages)
    dune :: edit_at(&lib, 0)
    dune[0].pages += 10
    neuro :: edit_at(&lib, 1)
    neuro[0].pages += 9
    print(lib.books[0].pages)
    print(lib.books[1].pages)
}
```

**`jet run p16_repo_owner_backed_views.jet`** — exit 0 (0.818s)

```
Dune
412
422
280
```

**`jet run --release p16_repo_owner_backed_views.jet`** — exit 0 (28.123s)

```
Dune
412
422
280
effects: IO
```

### p16b_gc_minimal

```jet
// D-OPTGC1: minimal gc-scoped program
#Policy(gc)

struct Node { value: Int }

fn run() {
    n :: Node.{ value: 7 }
    print(n.value)
}
```

**`jet run p16b_gc_minimal.jet`** — exit 0 (0.811s)

```
7
```

### p16c_recursive_enum

```jet
// isolate the gc_cyclic E0361: recursive enum, no gc policy
enum Link {
    End(Int)
    Next(Link)
}

fn run() {
    first :: Link.End(1)
    second :: Link.Next(~first)
    print(second)
}
```

**`jet run p16c_recursive_enum.jet`** — exit 1 (0.79s)

```
Error [E0361]: `equal` calls itself through `==`
  --> /home/nate/.cache/jet-audit-scratch/p16c_recursive_enum.jet:12:1
    |
 12 | 
    | ^
 Why: the operator symbol dispatches back to this same hook, so evaluation would recurse forever
 Fix: combine the value's fields or call a different named helper inside the hook

1 problem found
run `jet explain E0361` to learn more
```

### p17_example_sweep

```jet
(sweep — `jet run` on every examples/features/memory/*.jet, default tier)
```

**`jet sweep of 31 examples`** — exit 8

```
PASS arena.jet
PASS arena_parse.jet
PASS arena_regions.jet
PASS copy_verb.jet
FAIL entity_tree.jet  Error [E0112]: `add` wants Int (a whole number) for argument 1, but this is `Node`
FAIL entity_world.jet  Error [E0112]: `add` wants Int (a whole number) for argument 1, but this is `Player`
FAIL expiring_secret.jet
FAIL gc_cyclic.jet  Error [E0361]: `equal` calls itself through `==`
PASS local_cache.jet
PASS no_alloc_policy.jet
PASS owner_backed_views.jet
PASS ownership.jet
PASS parameter_modes.jet
PASS pin.jet
FAIL place_windows.jet  Error [E0220]: `particles` cannot be read while `left` has a live exclusive write window i
FAIL pool_stale_id.jet  Error [E0119]: `.{ … }` needs a known struct type here
PASS rawptr.jet
PASS ref_field.jet
PASS resource_close.jet
PASS returned_views.jet
FAIL shared_config.jet  Error [E1004]: `core.tasks` has no item `spawn`
PASS shared_guard_queue.jet
FAIL shared_transact.jet  Error [E1004]: `core.tasks` has no item `spawn`
PASS shared_weak_cycle.jet
PASS string_view.jet
PASS uninit.jet
PASS uninit_buffer.jet
PASS view_from_callback.jet
PASS view_from_param.jet
PASS view_from_slots.jet
PASS view_from_trait.jet
```


