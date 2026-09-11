# Prior Art

Every external source Jet has learned from: the languages, tools, ecosystems,
papers, talks, and creator retrospectives that shaped its semantics, syntax,
philosophy, and ecosystem plans. This is a provenance record — a trace from
today's design back to its inputs.

**How to read this.** Jet's rule is "take the invariant, not the surface
syntax": copy the successful idea, understand the constraint that produced it,
and decline the historical baggage. Most entries below are therefore listed
with *what Jet drew from them* — a feature adopted, a mistake avoided, or a
choice deliberately rejected.

**Where the deep synthesis lives.** This page indexes sources. The reasoned
analysis of each one lives in Jet's own primary documents:

- `docs/audits/language-lessons-and-regrets.md` — the master lineage study
  (2026-07-15): 30+ languages, each with creator regrets, official
  retrospectives, and Jet's defensive position.
- `docs/research/language-shape-research.md` — surface-shape research mining
  other languages for concrete syntax/semantics transplants (archived).
- `docs/audits/ecosystem-shape.md` — package/config/ecosystem research.
- `docs/proposals/epoch-3/universal-language-core.md` — web/UI/notebook/numeric
  reach research.
- `docs/proposals/jetpack/world-class-package-manager.md` — package-manager and
  supply-chain research.
- `docs/research/*` — active dated deep-dive reports; finished mines live under
  `docs/archive/`.

---

## Videos, talks & podcasts

Mined in full with transcript + comment analysis (see `docs/research/`,
`docs/archive/`, and the `mine-for-jet` skill).

**September research playlist (14 videos, mined 2026-09-09).** Full captured
transcript arguments, stratified audience samples, and selected visual frames.
The dated [mining report](../../audits/mine-for-jet-2026-09-09.md) retains the
claims, corrections, executable evidence, and unratified owner choices.

- Let's Get Rusty — How Rust Made Search 8,000x Faster — https://www.youtube.com/watch?v=H1TSClkkHy0
- Pokelego Dev — Zig 0.16 IO: Everything You Need to Know — https://www.youtube.com/watch?v=BUX01em5eG4
- commonLuke — I Tried to Learn Rust — https://www.youtube.com/watch?v=_2xQog4EBzg
- You Suck at Programming — Why I Don't Use #!/bin/bash — Shebangs Explained! — https://www.youtube.com/watch?v=aoHMiCzqCNw
- Flo Woelki — TypeScript 7.0 is 10x Faster! Why Microsoft Chose Go?! — https://www.youtube.com/watch?v=WXo6ial-vm4
- Matt Pocock — How To De-Slop A Codebase Ruined By AI (with one skill) — https://www.youtube.com/watch?v=3MP8D-mdheA
- Casey Muratori — The Root of The Root of All Evil — BSC 2026 — https://www.youtube.com/watch?v=hpj6r6CjJf8
- Casey Muratori — The Big OOPs: Anatomy of a Thirty-five-year Mistake — BSC 2025 — https://www.youtube.com/watch?v=wo84LFzx5nI
- Practical Coder — Wails V3: Go's Best GUI Framework (Electron Killer?) — https://www.youtube.com/watch?v=N5bH6ALXX4U
- Richard Hipp — Reliability Lessons From SQLite — SSW 2026 — https://www.youtube.com/watch?v=V_qzqY1bb7I
- Awesome — Web dev is finally healing — https://www.youtube.com/watch?v=9T0LVlVKjGo
- D_00 — I Write Python Like THIS and now Everyone Hates Me — https://www.youtube.com/watch?v=Xz2f-PtQAdk
- Suboptimal Engineer — Why C++ Static Variables Secretly Use Mutexes — https://www.youtube.com/watch?v=GnAwY55hux0
- Indently — "tqdm" is Awesome in Python — https://www.youtube.com/watch?v=gDDvH0ZyM7E

**Logan Smith — Rust series (9 videos, mined 2026-07-24).** External validation
of Jet's ratified safety/error/ownership design; report at
`docs/audits/2026-07-24-logan-smith-rust-series-mining.md`.

- 5 Strong Opinions On Everyday Rust — https://www.youtube.com/watch?v=8j_FbjiowvE
- Constructors Are Broken (build-then-construct-valid; "named constructor" not "factory") — https://www.youtube.com/watch?v=KWB-gDVuy_I
- Moves Are Broken (destructive moves + strong invariants) — https://www.youtube.com/watch?v=Klq-sNxuP2g
- Comprehending Proc Macros (bounded metaprogramming; why Jet rejects arbitrary syntax) — https://www.youtube.com/watch?v=SMCRQj9Hbx8
- Two Ways To Do Dynamic Dispatch (auto-box vs monomorphize; no user-facing `dyn`) — https://www.youtube.com/watch?v=wU8hQvU8aKM
- A Simpler Way to See Results (typed errors as values) — https://www.youtube.com/watch?v=s5S2Ed5T-dc
- Use Arc Instead of Vec (why Jet does *not* default to `Arc`) — https://www.youtube.com/watch?v=A4cKi7PTJSs
- Rust Functions Are Weird (But Be Glad) — https://www.youtube.com/watch?v=SqT5YglW3qU
- Choose the Right Option (`Option<&T>` not `&Option<T>`) — https://www.youtube.com/watch?v=6c7pZYP_iIE

**Logan Smith — "Verse: A New Scripting Language? In THIS Economy?"** (mined
2026-07-24). Crash course on Epic's Verse; source of the transactional-rollback
"watch" lesson and strategic validation of the Epoch-6 Canvas bet. Report at
`docs/audits/2026-07-24-verse-video-mining.md`.
https://www.youtube.com/watch?v=ebqKYLKjL6U

**Cross-facet polish batch (11 videos, mined 2026-08-03).** These sources
checked Jet's compiler seams, memory safety, runtime, layout, module, tooling,
environment, onboarding, UI, UX, and DX. The mine kept small polish cuts as
well as large design lessons.

Resulting work: Tower cards #1388 (`inspect expand --json`), #1389 (`inspect
unsafe` diagnostics and locations), #1390 (`layout` facts), and #1391 (final
S33 reconciliation), with ratified decisions D-LAYOUT-FACTS1=B and
D-GENERIC-CALL1=A recorded in `docs/spec/syntax-decisions.md`.

- lolzdev — Making my own programming language: keep keyword tables simple;
  preserve sema and TIR; reject unsafe-by-default design.
  https://www.youtube.com/watch?v=6lXZCOXCRME
- Tsoding Daily — Writing Garbage Collector in C: mark before traversal;
  prefer exact roots and edges over conservative stack scans.
  https://www.youtube.com/watch?v=2JgEKEd3tw8
- CsMadeEz — Goroutines Are NOT Threads: keep M:N scheduling, bounded work,
  known task exits, cancellation, and clear concurrency facts.
  https://www.youtube.com/watch?v=vfrAX26cqtg
- Cache Miss — Why Your C++ Struct Is Bigger Than It Should Be: keep explicit
  C and columnar layouts; measure locality instead of sorting fields blindly.
  https://www.youtube.com/watch?v=w50ofEmhRoc
- Adumh00man — Dendritic Nix is the Best Way to Configure a System: keep one
  reproducible graph with one-file defaults and optional module structure.
  https://www.youtube.com/watch?v=buxopFR4VXQ
- Semicolon — Rustc Commands Every Rust Developer Should Know: make errors,
  compiler facts, profiles, and targets easy to inspect without leaking rustc.
  https://www.youtube.com/watch?v=-DaVwuQeQD0
- Adumh00man — Installing Nixos on my Server! (500 ish sub special): preview
  destructive work; preserve locked bootstrap, recovery, driver, and secret state.
  https://www.youtube.com/watch?v=E3G2tl0GAb0
- Indently — `__init__.py` Explained in Just 7 minutes: keep short explicit
  module facades and reject import-time execution.
  https://www.youtube.com/watch?v=xn7nZLWXYSg
- DistroTube — The Age Of Beginner Friendly Distros Is Over: judge beginner
  UX by install, maintenance, recovery, hardware, docs, and offline states.
  https://www.youtube.com/watch?v=A6XI_0DWQOw
- Code to the Moon — 10 Underrated Rust Features & Patterns: keep exclusive
  mutation, bounded structured work, explicit modules, and uncolored tasks.
  https://www.youtube.com/watch?v=7QwqShxyHtc
- Let's Get Rusty — How unsafe Rust made Polars 30x times faster than Pandas:
  keep audited unsafe internals behind safe APIs; require measured attribution.
  https://www.youtube.com/watch?v=l6tisoOzTuk

**Visually Explained — Python mechanisms (4 videos, mined 2026-08-03).**
Stepwise visual explanations checked Jet's source-local transformations,
pull-driven streams, typed variadics, and JSON boundaries. Jet keeps one typed
mechanism for each job and rejects Python's dynamic baggage.

- Python Decorators - Visually Explained: keep transformations visible and
  inspectable; reject a second runtime decorator mechanism.
  https://www.youtube.com/watch?v=3tyaO-OE0K0
- Python Generators - Visually Explained: teach suspend/resume and one-pass
  streams; reject memory claims until Jet's thread-backed stream is measured.
  https://www.youtube.com/watch?v=GWZf_B129zs
- Python *args vs **kwargs - Visually Explained: use explicit typed parameters
  by default and typed variadics only for genuine open arity.
  https://www.youtube.com/watch?v=FFpDsC6B2qw
- JSON in Python - Visually Explained: preserve missing versus explicit null;
  reject object-only JSON roots and direct file truncation as safe defaults.
  https://www.youtube.com/watch?v=4rmBOxn0PdI

**Five-language release batch (5 videos, mined 2026-08-28).** The owner's
"Jet Research Queue" playlist mined together with live binary probes; full
report at `docs/audits/video-mine-five-languages-2026-08-28.md`. Meta-lesson:
every mature language is paying down a default it can no longer change.

- Let's Get Rusty — The biggest change to the Rust compiler is here!: next-gen
  trait solver; 4-year dual-solver cost, instruction-counts-not-wall-time perf
  plot; Jet's one-solver design confirmed live with product diagnostics.
  https://www.youtube.com/watch?v=aPL6y2oJjMw
- Indently — 5 Uncommon Python Features I Love: slice objects, set operators,
  `__format__`, walrus, currying; found D-DISPLAYDBG1 broken on all tiers and
  the S40 `slice(a..b)` drift; declined operator/walrus second spellings.
  https://www.youtube.com/watch?v=sQ1Q96-Vhjk
- Better Stack — JavaScript's Biggest Update in Years (ES2027): Temporal,
  `using`, `Iterator.zip`, `Atomics.pause`; Jet's zoned DST arithmetic and
  compile-time date-literal rejection ran live; ownership-checked `close(^r)`
  beats runtime disposal; E0620/E0041 immunities make `import defer` and
  `Atomics.pause` unnecessary. https://www.youtube.com/watch?v=DLT6n3wCkuc
- Coding with Patrik — Everything New in Go 1.27: generic methods (interface
  fence), stdlib `uuid`, `json/v2`, `simd`, `goroutineleak`, `synctest`;
  fed the E0956 reproducer batch (#2252) and the task-leak/observability gap.
  https://www.youtube.com/watch?v=rTgROnXIwnI
- Awesome — Go is becoming shockingly good: Go 1.27 opinion pass; 150-like
  audience verdict that generics erode Go's readability identity; JSON-default
  and enum/sum-type contrasts favor Jet live.
  https://www.youtube.com/watch?v=c7eLIsaDL7U

**Never, operators, and supply-chain batch (4 videos, mined 2026-09-01).** The
remaining "Jet Research Queue" entries, mined with 30 live probes on AOT,
`jet run`, and `jet eval` and a fresh second reader; full report at
`docs/audits/video-mine-never-operators-supply-chain-2026-09-01.md`.
Meta-lesson: the default path decides the outcome, and a name for a fact is
not the fact. Cards #2430–#2438; ballots D-NEVER2 and D-OPMIX1.

- Semicolon — Rust Condvar Explained: Stop Wasting 100% CPU: Jet's
  `Shared`/`Condition`/`guard.wait` is shipped with predicate re-check and
  tier parity; the busy loop compiles in silence (lint card #2435) and E0041
  claims Jet never shares memory (copy card #2434).
  https://www.youtube.com/watch?v=kHpEolpE3pU
- Let's Get Rusty — Rust just introduced a new "never" type: `!` merged for
  1.100 after ten years of `()`-fallback and `Infallible` debt; Jet's
  `fn f() Never` is an AOT ICE and D-NEVER1=C's own sample fails E0124
  (#2431); ballot D-NEVER2 on the named contract.
  https://www.youtube.com/watch?v=wpqiH56ITZo
- Indently — "NotImplemented" is Awesome in Python: run-time operand
  ownership versus Jet's static hooks; `Money + Money` prints zero on the
  default tier and ICEs on AOT, including the repo example (#2430); ballot
  D-OPMIX1 on mixed-type operands. https://www.youtube.com/watch?v=xUBIbhPC_rQ
- Low Level — a lot of people are upset: the 2026-08-20 `arrayref`
  compile-time backdoor (RUSTSEC-2026-0260); Jet's `extern rust` bridge calls
  host cargo outside D-JPK-SANDBOX2 (#2432); `jet new` emits an unparseable
  manifest since 8b9933668 (#2433). https://www.youtube.com/watch?v=uQV6hYwyjMY

**Rust-vs-C++, Python infinite loops, SIMD, C++ build systems batch (4 videos,
mined 2026-09-03).** Owner playlist batch, mined in full and cross-checked
against the live binary with about 90 probes on AOT, `jet run`, and
`--interpret`, plus a fresh-context second reader. Report:
`docs/audits/video-mine-cpp-loops-simd-build-2026-09-03.md`; companion tier
audit: `docs/audits/tier-parity-architecture-2026-09-03.md`. Meta-lesson: a
language is replaced when the thing you must reason about is the problem and
not the tool; Jet's design answers all four videos and its binary delivers one.
Second lesson: no tier receives Jet-owned optimization today (inlining and
folding exist only as rustc's work on emitted Rust), so "tiers are optimization
levels" needs a shared TIR pass before it can be true. Every finding maps to
one of seven defect shapes (report section "Generalize every finding") so the
fix closes the shape; five census cards enumerate the other instances; the tier
audit carries the enforcement design (E1–E4) that keeps one lowering one. Cards
#2878–#2906; ballots D-DO1, D-TIER-ONEIR1, D-PLACE1, D-FRED1, D-ACCEL1,
D-LOOPREAD1; owner gates #2897 and #2905; research #2901. Owner rulings
2026-09-03: S3 (a performance surface must beat the plain spelling) and S7 (CI
walks the first hour on every tier) become `AGENTS.md` invariants plus CI cards
#2905/#2900; D-TIER-ONEIR1 option A keeps rustc/LLVM as the release back end
with the Rust emitter reduced to a mechanical printer of one shared MIR (speed
and the borrow-checker safety witness untouched; Cranelift consumes the same MIR
for `jet run`; an LLVM back end is a separate axis from self-hosting).

- ForrestKnight — Why Rust Can't Replace C++: the losses are exact placement,
  alignment, and lock-free control, not templates; Jet has none of the four
  surfaces without `#Unsafe` (#2887) and its first-try doubly linked node ICEs
  on AOT while the index arena runs everywhere (#2879).
  https://www.youtube.com/watch?v=QNPwKMOQIKM
- Indently — The Infinite Loop Problem in Python: `while 1` is a trap truthy
  conditions set; Jet is immune (E0110) but its interpreter charges 2.5x for
  the spelling of a loop (#2886); no `do` keyword, ballot D-DO1 (#2884).
  https://www.youtube.com/watch?v=L6r0HBz9sT4
- Core Dumped — Simple Instructions, Weird Algorithms: layout, not
  instruction choice, decides vectorization; Jet's proof vectorizer emits zmm
  `vaddpd` unasked, yet `#Layout(columnar)` runs 15x slower than AoS (#2889),
  every elementwise loop clones its list (#2890), and Float reductions need a
  defined order to widen (#2891).
  https://www.youtube.com/watch?v=ryfbBB3pHfI
- Cakez reacting to Kea Sigma Delta — What is the BEST C++ Build System: every
  feature must earn its place; Jet's strict manifest already is the simplest
  surface in the comparison, and a fresh `jet new` project cannot use a path
  dependency, build `--locked`, or target the web (#2882, #2883).
  https://www.youtube.com/watch?v=lzhwEDkn6NY

**Compiled-scripting and explicitness batch (3 videos, mined 2026-08-21).**
A YouTube playlist mined together: the compiled-TypeScript moment, the
hardware-cost optimization thesis, and Zig 0.16 explicitness pedagogy.
Findings: unused-code lints card (#2141), footprint receipts card (#2142),
coverage-scoreboard lesson on #1156, version-matched local docs evidence on
#86, autovectorization expectation on #2059.

- ThePrimeTime — I tried Compiled Typescript (vercel-labs/scriptc): footprint
  and cold-start are the marketed axes; typed source becomes real structs;
  audience caught naive field-order padding and RC-cycle risk; keep the
  `coverage` remainder scoreboard idea, decline the RC default.
  https://www.youtube.com/watch?v=2g63UXaynaA
- dreadjordan — Why the HW Crisis forces Software to catch up: RAM ~4x cost
  makes performance a software-first problem; AI writes correct-not-best code,
  so perf must be a machine-checked verdict (Jet's budget law), not a habit.
  https://www.youtube.com/watch?v=Kr9ai5zxAcY
- tony — How to Actually Learn Zig (2027 Edition): Zig 0.16 writer-gate print
  ceremony vs Jet's one-line `print`; view-store copy semantics make Zig's
  dangling-slice `dupe` lesson unnecessary; `zig std` version-matched local
  docs are the learning workflow; audience rejects unused-variable hard errors.
  https://www.youtube.com/watch?v=dYGSPyp41vY

**Jonathan Blow on Jai** (The Standup w/ ThePrimeagen, transcript) — closed-beta
migration discipline; staged spelling migration through coexistence → warning →
removal → changelog. https://podscripts.co/podcasts/the-standup-with-theprimeagen/legendary-game-dev-jonathan-blow

**Onboarding reference** (Tower card note) — https://youtu.be/OPuztQfM3Fg

### Function-design canon

Mining date: 2026-08-21. One video plus its three
linked sources, mined together for function/API design law. Findings: rubric
rows and review vocabulary (card #2137), E0150 free-function wording (#2138),
sequence-algorithm gaps (#2139), stored invariant facts design (#2140), and
independent corroboration of D-DEVR-TWICE1 logged on card #2062.

- Logan Smith — How to write the perfect function: honest/dishonest signatures,
  inject dishonesty at the top, signature empathy, invariant-bearing types, one
  level of abstraction per body. Jet ships the enforcement the talk wishes for
  (effect rows, `#Abilities`, typestate, labels).
  https://www.youtube.com/watch?v=2OMRWPOSw9s
- Tony Van Eerd — Value Oriented Programming Part 1 (CppNow 2023): complecting,
  separate calculating from doing, narrow value arguments, non-member functions,
  strong IDs, sink arguments. https://www.youtube.com/watch?v=b4p_tcLYDV0
- Sean Parent — C++ Seasoning (GoingNative 2013): no raw loops / synchronization
  / owning pointers; rotate, gather, task dataflow, value-semantic polymorphism.
  https://www.youtube.com/watch?v=W2tWOdzgXHA
- Robert C. Martin — The Single Responsibility Principle (2014): reasons to
  change are people; gather what changes together, separate what does not.
  https://blog.cleancoder.com/uncle-bob/2014/05/08/SingleReponsibilityPrinciple.html

---

## Programming languages

### Systems & safety languages

**C** — predictable representation, direct FFI, freestanding reach; avoid:
unsafe-by-default, NUL-terminated strings, array-to-pointer decay, `_s` parallel
APIs, textual headers.
- WG14 N2659 (ordinary C is unsafe) — https://www.open-std.org/JTC1/SC22/WG14/www/docs/n2659.htm
- WG14 N1990 / N2660 / N3360 (recover array bounds) — https://www.open-std.org/jtc1/sc22/wg14/www/docs/n1990.htm · https://www.open-std.org/JTC1/SC22/WG14/www/docs/n2660.pdf · https://www.open-std.org/jtc1/sc22/wg14/www/docs/n3360.htm
- WG14 N1967 (Annex K bounds-checking failure) — https://open-std.org/jtc1/sc22/wg14/www/docs/n1967.htm
- WG14 N1400 / N2896 (preprocessor/header semantics) — https://www.open-std.org/JTC1/SC22/wg14/www/docs/n1400.htm · https://open-std.org/jtc1/sc22/wg14/www/docs/n2896.htm
- WG14 N1254 / N2885 (integer promotion/overflow) — https://www.open-std.org/jtc1/sc22/wg14/www/docs/n1254.htm · https://www.open-std.org/JTC1/SC22/WG14/www/docs/n2885.pdf
- "The Most Expensive One-byte Mistake" (ACM Queue) — https://queue.acm.org/detail.cfm?id=2010365

**C++** — zero-cost abstraction, deterministic lifetime, generics, native
interop; reject: inheritance, implicit narrowing, C-style casts, raw array
decay, dangling views, no-common-ABI.
- Stroustrup interviews: Slashdot / DevX / Italian — https://www.stroustrup.com/slashdot_interview.html · https://www.stroustrup.com/devXinterview.html · https://www.stroustrup.com/italian_interview.html
- P2771R1 (memory-safety views) — https://isocpp.org/files/papers/P2771R1.html
- CppCoreGuidelines view-lifetime issue #2276 — https://github.com/isocpp/CppCoreGuidelines/issues/2276
- mp-units — points & quantities (affine space) — https://mpusz.github.io/mp-units/latest/tutorials/affine_space/points_and_quantities/

**Rust** — ownership, deterministic cleanup, exhaustive ADTs, strong
diagnostics, Cargo's integrated workflow; watch: learning-curve accidental
detail, async-as-parallel-surface, compile time, orphan-rule glue friction,
build-script supply chain, feature unification.
- The Rust I Wanted Had No Future (Graydon Hoare) — https://graydon2.dreamwidth.org/307291.html
- 2024 lang roadmap / 2024 State of Rust / 2025 compiler-perf survey — https://blog.rust-lang.org/inside-rust/2022/04/04/lang-roadmap-2024/ · https://blog.rust-lang.org/2025/02/13/2024-State-Of-Rust-Survey-results/ · https://blog.rust-lang.org/2025/09/10/rust-compiler-performance-survey-2025-results/
- Async project goals 2024h2 / 2026 roadmap — https://rust-lang.github.io/rust-project-goals/2024h2/async.html · https://rust-lang.github.io/rust-project-goals/2026/roadmap-just-add-async.html
- Relaxing the orphan rule — https://rust-lang.github.io/rust-project-goals/2024h2/Relaxing-the-Orphan-Rule.html
- RFC 2451 re-rebalancing coherence — https://github.com/rust-lang/rfcs/blob/master/text/2451-re-rebalancing-coherence.md
- Little Orphan Impls (Niko Matsakis) — https://smallcultfollowing.com/babysteps/blog/2015/01/14/little-orphan-impls/
- Cargo: build scripts / features / workspaces / resolver — https://doc.rust-lang.org/stable/cargo/reference/build-scripts.html · https://doc.rust-lang.org/cargo/reference/features.html · https://doc.rust-lang.org/cargo/reference/workspaces.html · https://doc.rust-lang.org/nightly/cargo/reference/resolver.html
- crates.io malware postmortem — https://blog.rust-lang.org/inside-rust/2023/09/01/crates-io-malware-postmortem/
- Cargo issues #14414 / #8088 — https://github.com/rust-lang/cargo/issues/14414 · https://github.com/rust-lang/cargo/issues/8088
- The Book: ownership ch04 / enums ch06 / patterns ch19-01 — https://doc.rust-lang.org/book/ch04-00-understanding-ownership.html · https://doc.rust-lang.org/book/ch06-00-enums.html · https://doc.rust-lang.org/book/ch19-01-all-the-places-for-patterns.html
- API Guidelines: predictability — https://rust-lang.github.io/api-guidelines/predictability.html
- struct/impl colocation study (`docs/audits/2026-07-24-rust-struct-impl-colocation.md`): Clippy `multiple_inherent_impl` — https://github.com/rust-lang/rust-clippy/blob/master/clippy_lints/src/inherent_impl.rs · Clippy #6446 explicit-drop — https://github.com/rust-lang/rust-clippy/issues/6446 · Canonical ordering discipline — https://canonical.github.io/rust-best-practices/ordering-discipline.html · PingCAP trait style — https://pingcap.github.io/style-guide/rust/traits.html · users.rust-lang style thread — https://users.rust-lang.org/t/a-question-of-style-for-impl-of-structs/118029

[Showing lines 1-300 of 306. Use :301 to continue]

## Developer experience (e14)

Sources from the nine e14 DX census manifests. IDs are manifest IDs; local paths remain provenance records.

### web-tooling (64 sources)

- VITE-GUIDE — https://vite.dev/guide/
- VITE-FEATURES — https://vite.dev/guide/features.html
- VITE-PLUGIN — https://vite.dev/guide/api-plugin.html
- VITE-CONFIG — https://vite.dev/config/
- BUN-QUICKSTART — https://bun.sh/docs/quickstart
- BUN-RUNTIME — https://bun.sh/docs/runtime
- BUN-WATCH — https://bun.sh/docs/runtime/watch-mode
- BUN-TEST — https://bun.sh/docs/test/
- BUN-BUILD — https://bun.sh/docs/bundler/
- BUN-BUNX — https://bun.sh/docs/pm/bunx
- BUN-INSTALL — https://bun.sh/docs/pm/cli/install
- DENO-INIT — https://docs.deno.com/runtime/reference/cli/init/
- DENO-TASK — https://docs.deno.com/runtime/reference/cli/task/
- DENO-WATCH — https://docs.deno.com/runtime/run/watch_mode/
- DENO-FMT — https://docs.deno.com/runtime/reference/cli/fmt/
- DENO-LINT — https://docs.deno.com/runtime/reference/cli/lint/
- DENO-TEST — https://docs.deno.com/runtime/reference/cli/test/
- DENO-BENCH — https://docs.deno.com/runtime/reference/cli/bench/
- DENO-COVERAGE — https://docs.deno.com/runtime/reference/cli/coverage/
- DENO-DOC — https://docs.deno.com/runtime/reference/cli/doc/
- DENO-SECURITY — https://docs.deno.com/runtime/fundamentals/security/
- TANSTACK-OVERVIEW — https://tanstack.com/devtools/latest/docs/overview
- TANSTACK-QUICKSTART — https://tanstack.com/devtools/latest/docs/quick-start
- TANSTACK-PLUGIN — https://tanstack.com/devtools/latest/docs/plugin-configuration
- TANSTACK-CUSTOM — https://tanstack.com/devtools/latest/docs/building-custom-plugins
- TANSTACK-PRODUCTION — https://tanstack.com/devtools/latest/docs/production
- TANSTACK-VITE — https://tanstack.com/devtools/latest/docs/vite-plugin
- TANSTACK-EVENTS — https://tanstack.com/devtools/latest/docs/event-system
- TANSTACK-CONFIG — https://tanstack.com/devtools/latest/docs/configuration
- TANSTACK-UTILS — https://tanstack.com/devtools/latest/docs/devtools-utils
- TANSTACK-QUERY — https://tanstack.com/query/latest/docs/framework/react/devtools
- TANSTACK-ROUTER — https://tanstack.com/router/latest/docs/devtools
- TANSTACK-FORM — https://tanstack.com/form/latest/docs/framework/react/guides/devtools
- TANSTACK-FORM-SOURCE — https://raw.githubusercontent.com/TanStack/form/main/packages/form-devtools/src/components/DetailsPanel.tsx
- TANSTACK-FORM-ACTIONS — https://raw.githubusercontent.com/TanStack/form/main/packages/form-devtools/src/components/ActionButtons.tsx
- TANSTACK-TABLE — https://tanstack.com/table/latest/docs/devtools
- ASTRO-APP-REF — https://docs.astro.build/en/reference/dev-toolbar-app-reference/
- ASTRO-GUIDE — https://docs.astro.build/en/guides/dev-toolbar/
- ASTRO-OLD-REF — https://docs.astro.build/en/reference/dev-toolbar/
- ASTRO-OLD-GUIDE — https://docs.astro.build/en/guides/dev-toolbar-apps/
- REACT-DEVTOOLS — https://react.dev/learn/react-developer-tools
- REACT-SETUP — https://react.dev/learn/setup
- REACT-PROFILER — https://react.dev/reference/react/Profiler
- REACT-PERF-TRACKS — https://react.dev/reference/dev-tools/react-performance-tracks
- CHROME-OVERVIEW — https://developer.chrome.com/docs/devtools/overview
- CHROME-WORKSPACES — https://developer.chrome.com/docs/devtools/workspaces
- CHROME-PERFORMANCE — https://developer.chrome.com/docs/devtools/performance/overview
- CHROME-RECORDER — https://developer.chrome.com/docs/devtools/recorder
- NEXT-ERROR — https://nextjs.org/docs/pages/building-your-application/configuring/error-handling
- NEXT-INDICATORS — https://nextjs.org/docs/app/api-reference/config/next-config-js/devIndicators
- NEXT-15-2 — https://nextjs.org/blog/next-15-2
- STORYBOOK-INSTALL — https://storybook.js.org/docs/get-started/install
- STORYBOOK-TESTS — https://storybook.js.org/docs/writing-tests
- STORYBOOK-CONTROLS — https://storybook.js.org/docs/essentials/controls
- PLAYWRIGHT-INTRO — https://playwright.dev/docs/intro
- PLAYWRIGHT-UI — https://playwright.dev/docs/test-ui-mode
- PLAYWRIGHT-TRACE — https://playwright.dev/docs/trace-viewer
- PLAYWRIGHT-CODEGEN — https://playwright.dev/docs/codegen
- PLAYWRIGHT-CONFIG — https://playwright.dev/docs/test-configuration
- NX-GRAPH — https://nx.dev/docs/features/explore-graph
- NX-CACHE — https://nx.dev/docs/features/cache-task-results
- NX-CONSOLE — https://nx.dev/docs/getting-started/editor-setup
- TURBO-RUN — https://turborepo.dev/docs/crafting-your-repository/running-tasks
- TURBO-CACHE — https://turborepo.dev/docs/crafting-your-repository/caching

### web-frameworks (48 sources)

- W01 — https://tanstack.com/router/latest/docs/overview
- W02 — https://tanstack.com/router/latest/docs/routing/file-based-routing
- W03 — https://tanstack.com/router/latest/docs/guide/data-loading
- W04 — https://tanstack.com/query/latest/docs/framework/react/overview
- W05 — https://tanstack.com/query/latest/docs/framework/react/guides/mutations
- W06 — https://tanstack.com/query/latest/docs/framework/react/guides/query-invalidation
- W07 — https://tanstack.com/query/latest/docs/framework/react/guides/suspense
- W08 — https://tanstack.com/query/latest/docs/framework/react/guides/network-mode
- W09 — https://tanstack.com/query/latest/docs/framework/react/devtools
- W10 — https://tanstack.com/form/latest/docs/overview
- W11 — https://tanstack.com/form/latest/docs/framework/react/guides/validation
- W12 — https://tanstack.com/table/v8/docs/introduction
- W13 — https://tanstack.com/table/v8/docs/guide/data
- W14 — https://tanstack.com/table/v8/docs/guide/sorting
- W15 — https://tanstack.com/table/v8/docs/guide/pagination
- W16 — https://tanstack.com/virtual/latest/docs/introduction
- W17 — https://tanstack.com/store/latest/docs/overview
- W18 — https://tanstack.com/store/latest/docs/quick-start
- W19 — https://tanstack.com/store/latest/docs/reference/functions/createStore
- W20 — https://tanstack.com/start/latest/docs/framework/react/overview
- W21 — https://tanstack.com/start/latest/docs/framework/react/guide/server-functions
- W22 — https://tanstack.com/start/latest/docs/framework/react/guide/middleware
- W23 — https://nextjs.org/docs/app/getting-started/server-and-client-components
- W24 — https://nextjs.org/docs/app/getting-started/mutating-data
- W25 — https://nextjs.org/docs/app/guides/caching-without-cache-components
- W26 — https://nextjs.org/docs/app/api-reference/file-conventions/loading
- W27 — https://nextjs.org/docs/app/getting-started/caching
- W28 — https://github.com/vercel/next.js/discussions/54075
- W29 — https://github.com/vercel/next.js/issues/58723
- W30 — https://github.com/vercel/next.js/discussions/86538
- W31 — https://github.com/vercel/next.js/discussions/59167
- W32 — https://github.com/vercel/next.js/discussions/73537
- W33 — https://github.com/vercel/next.js/issues/54514
- W34 — https://docs.astro.build/en/concepts/islands/
- W35 — https://docs.astro.build/en/reference/directives-reference/
- W36 — https://docs.astro.build/en/guides/content-collections/
- W37 — https://docs.astro.build/en/guides/view-transitions/
- W38 — https://qwik.dev/docs/concepts/resumable/
- W39 — https://qwik.dev/docs/core/overview/
- W40 — https://svelte.dev/docs/kit/load
- W41 — https://svelte.dev/docs/kit/form-actions
- W42 — https://svelte.dev/docs/svelte/what-are-runes
- W43 — https://docs.solidjs.com/solid-start/v1
- W44 — https://docs.solidjs.com/solid-start/v2
- W45 — https://github.com/solidjs/solid-start
- W46 — https://reactrouter.com/start/framework/data-loading
- W47 — https://reactrouter.com/start/framework/actions
- W48 — https://usefresh.dev/docs/concepts/islands

### live (37 sources)

- elm-debugger — https://guide.elm-lang.org/architecture/debugger.html
- elm-reactor — https://github.com/elm-lang/elm-reactor
- elm-debug-site — https://github.com/elm-lang/debug.elm-lang.org
- elm-architecture-buttons — https://guide.elm-lang.org/architecture/buttons
- redux-devtools-guide — https://redux.js.org/usage/configuring-your-store#redux-devtools
- redux-devtools — https://github.com/reduxjs/redux-devtools
- redux-devtools-api — https://github.com/reduxjs/redux-devtools/blob/main/extension/docs/API/Arguments.md
- redux-devtools-methods — https://github.com/reduxjs/redux-devtools/blob/main/extension/docs/API/Methods.md
- redux-devtools-trace — https://github.com/reduxjs/redux-devtools/blob/main/extension/docs/Features/Trace.md
- replay-docs — https://docs.replay.io/
- replay-time-travel — https://docs.replay.io/getting-started/what-is-replay-io
- replay-live-logs — https://docs.replay.io/reference-guide/debugging/print-statements
- replay-react — https://docs.replay.io/reference-guide/dev-tools/react
- replay-recording — https://docs.replay.io/quickstart
- replay-runtime — https://docs.replay.io/reference/replay-runtimes/replay-chrome
- pharo-by-example — https://books.pharo.org/pharo-by-example/
- pharo-by-example9 — https://raw.githubusercontent.com/SquareBracketAssociates/PharoByExample9/master/Chapters/FirstApplication/FirstApplication.md
- clojure-repl — https://clojure.org/guides/repl/introduction
- cider-interactive — https://docs.cider.mx/cider-nrepl/usage/interactive_programming.html
- cider-interactive-current — https://docs.cider.mx/cider/usage/interactive_programming.html
- portal — https://raw.githubusercontent.com/djblue/portal/master/README.md
- erlang-code-loading — https://www.erlang.org/doc/system_principles/code_loading.html
- erlang-release-handling — https://www.erlang.org/doc/system_principles/release_handling.html
- erlang-observer — https://www.erlang.org/doc/apps/observer/observer_ug.html
- elixir-reloading — https://hexdocs.pm/elixir/code-reloading.html
- elixir-mix-compiler — https://hexdocs.pm/mix/Mix.Task.Compiler.html
- swift-playgrounds — https://developer.apple.com/documentation/swift-playgrounds
- xcode-previews — https://developer.apple.com/documentation/xcode/previewing-your-apps-interface-in-xcode
- revise — https://timholy.github.io/Revise.jl/stable/
- vscode-live-share — https://code.visualstudio.com/learn/collaboration/live-share
- vscode-live-share-current — https://learn.microsoft.com/en-us/visualstudio/liveshare/
- vscode-live-share-features — https://learn.microsoft.com/en-us/visualstudio/liveshare/overview/features
- rr-site — https://rr-project.org/
- rr-repo — https://github.com/rr-debugger/rr
- jupyter — https://docs.jupyter.org/en/latest/
- jupyter-notebook — https://jupyter-notebook.readthedocs.io/en/latest/notebook.html
- marimo — https://docs.marimo.io/

### games (47 sources)

- UE-PIE — https://dev.epicgames.com/documentation/en-us/unreal-engine/playing-and-simulating-in-unreal-engine
- UE-LIVE — https://dev.epicgames.com/documentation/en-us/unreal-engine/using-live-coding-to-recompile-unreal-engine-applications-at-runtime
- UE-BP — https://dev.epicgames.com/documentation/en-us/unreal-engine/blueprints-visual-scripting-in-unreal-engine
- UE-STAT — https://dev.epicgames.com/documentation/en-us/unreal-engine/stat-commands-in-unreal-engine
- UE-INSIGHTS — https://dev.epicgames.com/documentation/en-us/unreal-engine/unreal-insights-in-unreal-engine
- UE-GDT — https://dev.epicgames.com/documentation/en-us/unreal-engine/using-the-gameplay-debugger-in-unreal-engine
- UE-CRASH — https://dev.epicgames.com/documentation/en-us/unreal-engine/crash-reporting-in-unreal-engine
- UE-PACKAGE — https://dev.epicgames.com/documentation/en-us/unreal-engine/packaging-your-project
- UE-AUTOREIMPORT — https://dev.epicgames.com/documentation/en-us/unreal-engine/reimporting-assets-automatically-in-unreal-engine
- U-MANUAL — https://docs.unity3d.com/6000.0/Documentation/Manual/UnityManual.html
- U-PLAY — https://docs.unity3d.com/6000.0/Documentation/Manual/configurable-enter-play-mode.html
- U-CODELOAD — https://docs.unity3d.com/6000.0/Documentation/Manual/code-reloading-editor.html
- U-DOMAIN — https://docs.unity3d.com/6000.0/Documentation/Manual/domain-reloading.html
- U-INSPECTOR — https://docs.unity3d.com/6000.0/Documentation/Manual/UsingTheInspector.html
- U-INSPECTOR-OPTIONS — https://docs.unity3d.com/6000.0/Documentation/Manual/InspectorOptions.html
- U-EDITOR-API — https://docs.unity3d.com/6000.0/Documentation/ScriptReference/Editor.html
- U-PROFILER — https://docs.unity3d.com/6000.0/Documentation/Manual/Profiler.html
- U-PROFILER-CUSTOM — https://docs.unity3d.com/6000.0/Documentation/Manual/profiler-create-modules.html
- U-FRAME — https://docs.unity3d.com/6000.0/Documentation/Manual/FrameDebugger.html
- U-FRAME-CONTROLS — https://docs.unity3d.com/6000.0/Documentation/Manual/FrameDebugger-debug.html
- U-PACKAGES — https://docs.unity3d.com/6000.0/Documentation/Manual/Packages.html
- U-IL2CPP — https://docs.unity3d.com/6000.0/Documentation/Manual/il2cpp-introduction.html
- U-CONSOLE — https://docs.unity3d.com/6000.0/Documentation/Manual/Console.html
- U-DEBUGBUILD — https://docs.unity3d.com/6000.0/Documentation/ScriptReference/Debug-isDebugBuild.html
- U-BUILDPROFILES — https://docs.unity3d.com/6000.0/Documentation/Manual/build-profiles-reference.html
- U-ASSETREFRESH — https://docs.unity3d.com/6000.0/Documentation/Manual/AssetDatabaseRefreshing.html
- GODOT-PROJECT — https://docs.godotengine.org/en/stable/tutorials/editor/project_manager.html
- GODOT-SCENE — https://docs.godotengine.org/en/stable/getting_started/step_by_step/nodes_and_scenes.html
- GODOT-EMBED — https://docs.godotengine.org/en/stable/tutorials/editor/game_embedding.html
- GODOT-DEBUG — https://docs.godotengine.org/en/stable/tutorials/scripting/debug/overview_of_debugging_tools.html
- GODOT-DEBUGGER — https://docs.godotengine.org/en/stable/tutorials/scripting/debug/debugger_panel.html
- GODOT-PROFILER — https://docs.godotengine.org/en/stable/tutorials/scripting/debug/the_profiler.html
- GODOT-OUTPUT — https://docs.godotengine.org/en/stable/tutorials/scripting/debug/output_panel.html
- GODOT-INSPECTOR — https://docs.godotengine.org/en/stable/tutorials/editor/inspector_dock.html
- GODOT-IMPORT — https://docs.godotengine.org/en/stable/tutorials/assets_pipeline/import_process.html
- GODOT-EXPORT — https://docs.godotengine.org/en/stable/tutorials/export/exporting_projects.html
- GODOT-CLI — https://docs.godotengine.org/en/stable/tutorials/editor/command_line_tutorial.html
- BEVY-START — https://bevy.org/learn/quick-start/getting-started/
- BEVY-APPS — https://bevy.org/learn/quick-start/getting-started/apps/
- BEVY-ECS — https://bevy.org/learn/quick-start/getting-started/ecs/
- BEVY-FEATURES — https://github.com/bevyengine/bevy/blob/latest/docs/cargo_features.md
- BEVY-REMOTE — https://docs.rs/bevy/latest/bevy/remote/index.html
- BEVY-INSPECTOR — https://github.com/jakobhellermann/bevy-inspector-egui
- BEVY-DIAGNOSTIC — https://docs.rs/bevy/latest/bevy/diagnostic/index.html
- BEVY-DEVTOOLS — https://docs.rs/bevy/latest/bevy/dev_tools/index.html
- BEVY-EXAMPLES — https://github.com/bevyengine/bevy/blob/latest/examples/README.md
- CARGO-BUILD — https://doc.rust-lang.org/cargo/commands/cargo-build.html

### backend (43 sources)

- PHX-DASH — https://phoenix-live-dashboard.hexdocs.pm/Phoenix.LiveDashboard.html
- PHX-PAGE — https://phoenix-live-dashboard.hexdocs.pm/Phoenix.LiveDashboard.PageBuilder.html
- PHX-METRICS — https://phoenix-live-dashboard.hexdocs.pm/metrics.html
- PHX-ECTO — https://phoenix-live-dashboard.hexdocs.pm/ecto_stats.html
- PHX-MIGRATE — https://hexdocs.pm/ecto_sql/3.14.0/Mix.Tasks.Ecto.Migrate.html
- PHX-RELOAD — https://phoenix-live-reload.hexdocs.pm/Phoenix.LiveReloader.html
- PHX-HTML — https://phoenix.hexdocs.pm/1.8.11/Mix.Tasks.Phx.Gen.Html.html
- PHX-SCHEMA — https://phoenix.hexdocs.pm/1.8.11/Mix.Tasks.Phx.Gen.Schema.html
- PHX-SERVER — https://phoenix.hexdocs.pm/1.8.11/Mix.Tasks.Phx.Server.html
- LARAVEL-TELESCOPE — https://laravel.com/docs/12.x/telescope
- LARAVEL-HORIZON — https://laravel.com/docs/12.x/horizon
- HORIZON-ROUTES — https://github.com/laravel/horizon/blob/5.x/resources/js/routes.js
- HORIZON-DASH — https://github.com/laravel/horizon/blob/5.x/resources/js/screens/dashboard.vue
- TELESCOPE-REG — https://raw.githubusercontent.com/laravel/telescope/5.x/src/RegistersWatchers.php
- TELESCOPE-WATCHER — https://raw.githubusercontent.com/laravel/telescope/5.x/src/Watchers/Watcher.php
- LARAVEL-PULSE — https://laravel.com/docs/12.x/pulse
- LARAVEL-ARTISAN — https://laravel.com/docs/12.x/artisan
- LARAVEL-SAIL — https://laravel.com/docs/12.x/sail
- LARAVEL-ERRORS — https://laravel.com/docs/12.x/errors
- RAILS-CLI — https://guides.rubyonrails.org/command_line.html
- RAILS-JOBS — https://guides.rubyonrails.org/active_job_basics.html
- RAILS-DEBUG — https://guides.rubyonrails.org/debugging_rails_applications.html
- RAILS-JS — https://guides.rubyonrails.org/working_with_javascript_in_rails.html
- RAILS-WEBCONSOLE — https://github.com/rails/web-console
- SPRING-INITIAL — https://spring.io/quickstart
- SPRING-DEVTOOLS — https://docs.spring.io/spring-boot/reference/using/devtools.html
- SPRING-ACTUATOR — https://docs.spring.io/spring-boot/reference/actuator/endpoints.html
- SPRING-MONITOR — https://docs.spring.io/spring-boot/reference/actuator/monitoring.html
- SPRING-WEB — https://docs.spring.io/spring-boot/reference/web/servlet.html
- SBA-README — https://github.com/codecentric/spring-boot-admin
- SBA-UI — https://docs.spring-boot-admin.com/4.1.2/docs/samples/sample-custom-ui.md
- SBA-ENDPOINTS — https://docs.spring-boot-admin.com/4.1.2/docs/reference/actuator-endpoints.md
- DOTNET-WATCH — https://learn.microsoft.com/en-us/dotnet/core/tools/dotnet-watch
- ASPIRE-OVERVIEW — https://aspire.dev/dashboard/overview
- ASPIRE-EXPLORE — https://aspire.dev/dashboard/explore
- ASPIRE-COMMANDS — https://aspire.dev/fundamentals/custom-resource-commands
- DJANGO-ADMIN — https://docs.djangoproject.com/en/5.2/ref/django-admin/
- DJANGO-MIGRATIONS — https://docs.djangoproject.com/en/5.2/topics/migrations/
- DJANGO-ADMIN-SITE — https://docs.djangoproject.com/en/5.2/intro/tutorial02/
- DDT-PANELS — https://django-debug-toolbar.readthedocs.io/en/latest/panels.html
- DDT-INSTALL — https://django-debug-toolbar.readthedocs.io/en/latest/installation.html
- DDT-PANEL-SOURCE — https://raw.githubusercontent.com/django-commons/django-debug-toolbar/main/debug_toolbar/panels/__init__.py
- ERLANG-OBSERVER — https://www.erlang.org/doc/apps/observer/observer_ug.html

### systems (42 sources)

- CARGO-NEW — https://doc.rust-lang.org/cargo/commands/cargo-new.html
- CARGO-RUN — https://doc.rust-lang.org/cargo/commands/cargo-run.html
- CARGO-TEST — https://doc.rust-lang.org/cargo/commands/cargo-test.html
- CARGO-BENCH — https://doc.rust-lang.org/cargo/commands/cargo-bench.html
- CARGO-DOC — https://doc.rust-lang.org/cargo/commands/cargo-doc.html
- CARGO-ADD — https://doc.rust-lang.org/cargo/commands/cargo-add.html
- CARGO-WORKSPACE — https://doc.rust-lang.org/cargo/reference/workspaces.html
- CARGO-FEATURES — https://doc.rust-lang.org/cargo/reference/features.html
- CARGO-PROFILES — https://doc.rust-lang.org/cargo/reference/profiles.html
- CARGO-EXT — https://doc.rust-lang.org/cargo/reference/external-tools.html
- RUSTC-EXPLAIN — https://doc.rust-lang.org/rustc/command-line-arguments.html
- RUSTC-JSON — https://doc.rust-lang.org/rustc/json.html
- RA-FEATURES — https://rust-analyzer.github.io/book/features.html
- RA-ASSISTS — https://rust-analyzer.github.io/book/assists.html
- CARGO-WATCH — https://github.com/watchexec/cargo-watch
- BACON — https://dystroy.org/bacon/
- BACON-COOKBOOK — https://dystroy.org/bacon/cookbook/
- NEXTEST — https://nexte.st/docs/filtersets/
- ZIG-START — https://ziglang.org/learn/getting-started/
- ZIG-BUILD — https://ziglang.org/learn/build-system/
- ZIG-LANGREF — https://ziglang.org/documentation/master/
- GO-CMD — https://go.dev/cmd/go/
- GO-MOD — https://go.dev/ref/mod
- GO-FUZZ — https://go.dev/doc/tutorial/fuzz
- GO-RACE — https://go.dev/doc/articles/race_detector
- GO-WORK — https://go.dev/doc/tutorial/workspaces
- GO-GOPLS — https://go.dev/gopls/
- GO-DELVE — https://github.com/go-delve/delve
- GO-PPROF — https://go.dev/blog/pprof
- CRITERION — https://bheisler.github.io/criterion.rs/book/
- CRITERION-CLI — https://bheisler.github.io/criterion.rs/book/user_guide/command_line_options.html
- CARGO-EXPAND — https://github.com/dtolnay/cargo-expand
- FLAMEGRAPH — https://github.com/flamegraph-rs/flamegraph
- DOCSRS — https://docs.rs/about
- MIETTE — https://github.com/zkat/miette
- ARIADNE — https://github.com/zesterer/ariadne
- JUST — https://just.systems/man/en/
- JUST-PARAMS — https://just.systems/man/en/recipe-parameters.html
- MISE-TASKS — https://mise.jdx.dev/tasks/
- MISE-RUN — https://mise.jdx.dev/tasks/running-tasks.html
- DIRENV — https://direnv.net/
- DEVENV — https://devenv.sh/getting-started/

### mobile (43 sources)

- flutter-install — https://docs.flutter.dev/install
- flutter-cli — https://docs.flutter.dev/reference/flutter-cli
- flutter-hot-reload — https://docs.flutter.dev/tools/hot-reload
- flutter-devtools — https://docs.flutter.dev/tools/devtools
- flutter-inspector — https://docs.flutter.dev/tools/devtools/inspector
- flutter-layout-explorer — https://docs.flutter.dev/tools/devtools/layout-explorer
- flutter-performance — https://docs.flutter.dev/tools/devtools/performance
- flutter-cpu-profiler — https://docs.flutter.dev/tools/devtools/cpu-profiler
- flutter-memory — https://docs.flutter.dev/tools/devtools/memory
- flutter-network — https://docs.flutter.dev/tools/devtools/network
- flutter-extensions — https://docs.flutter.dev/tools/devtools/extensions
- flutter-custom-tool — https://docs.flutter.dev/tools/devtools/custom-tool
- expo-environment — https://docs.expo.dev/get-started/set-up-your-environment
- expo-start — https://docs.expo.dev/get-started/start-developing
- expo-dev-builds — https://docs.expo.dev/develop/development-builds/introduction
- expo-build — https://docs.expo.dev/build/introduction
- expo-update — https://docs.expo.dev/eas-update/introduction
- expo-router — https://docs.expo.dev/router/introduction
- expo-devtools-plugins — https://docs.expo.dev/debugging/devtools-plugins
- expo-create-plugin — https://docs.expo.dev/debugging/create-devtools-plugins
- rn-fast-refresh — https://reactnative.dev/docs/fast-refresh
- rn-hermes — https://reactnative.dev/docs/hermes
- rn-debugging — https://reactnative.dev/docs/debugging
- rn-devtools — https://reactnative.dev/docs/react-native-devtools
- swiftui-previews — https://developer.apple.com/documentation/swiftui/previews-in-xcode
- xcode-preview-interface — https://developer.apple.com/documentation/xcode/adding-previews-to-your-interface-files
- xcode-preview-canvas — https://developer.apple.com/documentation/xcode/interacting-with-previews-in-the-canvas
- xcode-run-device — https://developer.apple.com/documentation/xcode/running-your-app-on-simulated-or-physical-devices
- wwdc-previews — https://developer.apple.com/videos/play/wwdc2023/10252/
- xcode-diagnostics — https://developer.apple.com/documentation/xcode/diagnosing-memory-thread-and-crash-issues-early
- instruments-archive — https://developer.apple.com/library/archive/documentation/AnalysisTools/Conceptual/instruments_help-collection/Chapter/Chapter.html
- xcode-cloud — https://developer.apple.com/xcode-cloud/
- swiftpm-cli — https://www.swift.org/getting-started/cli-swiftpm/
- android-studio — https://developer.android.com/studio/intro
- compose-tooling — https://developer.android.com/develop/ui/compose/tooling
- compose-live-edit — https://developer.android.com/develop/ui/compose/tooling/iterative-development
- compose-previews — https://developer.android.com/develop/ui/compose/tooling/previews
- android-layout-inspector — https://developer.android.com/studio/debug/layout-inspector
- android-profiler — https://developer.android.com/studio/profile
- android-logcat — https://developer.android.com/studio/debug/logcat
- android-gradle-cli — https://developer.android.com/build/building-cmdline
- kmp-first-app — https://kotlinlang.org/docs/multiplatform/multiplatform-create-first-app.html
- kmp-hot-reload — https://kotlinlang.org/docs/multiplatform/compose-hot-reload.html

### data (40 sources)

- jupyter-architecture — https://docs.jupyter.org/en/latest/projects/architecture/content-architecture.html
- jupyter-notebook — https://jupyter-notebook.readthedocs.io/en/stable/notebook.html
- jupyterlab-extensions — https://jupyterlab.readthedocs.io/en/stable/user/extensions.html
- ipywidgets-interact — https://ipywidgets.readthedocs.io/en/stable/examples/Using%20Interact.html
- ipython-magics — https://ipython.readthedocs.io/en/stable/interactive/magics.html
- marimo-reactivity — https://docs.marimo.io/guides/reactivity/
- marimo-apps — https://docs.marimo.io/guides/apps/
- marimo-export — https://docs.marimo.io/guides/exporting/
- marimo-old-notebooks — https://docs.marimo.io/guides/understanding_notebooks/
- observable-framework — https://raw.githubusercontent.com/observablehq/framework/main/docs/what-is-framework.md
- observable-loaders — https://raw.githubusercontent.com/observablehq/framework/main/docs/data-loaders.md
- observable-plot — https://raw.githubusercontent.com/observablehq/plot/main/README.md
- pluto-reactivity — https://plutojl.org/en/docs/reactivity/
- pluto-packages — https://plutojl.org/en/docs/packages/
- livebook — https://livebook.dev/
- kino — https://kino.hexdocs.pm/
- kino-datatable — https://kino.hexdocs.pm/Kino.DataTable.html
- polars-lazy — https://docs.pola.rs/user-guide/lazy/using/
- polars-optimizations — https://docs.pola.rs/user-guide/lazy/optimizations/
- polars-streaming — https://docs.pola.rs/user-guide/concepts/streaming/
- polars-plugins — https://docs.pola.rs/user-guide/plugins/
- duckdb-cli — https://duckdb.org/docs/current/clients/cli/overview.html
- duckdb-dot — https://duckdb.org/docs/current/clients/cli/dot_commands.html
- duckdb-ui — https://duckdb.org/docs/current/core_extensions/ui
- pandas-copy-on-write — https://pandas.pydata.org/docs/user_guide/copy_on_write.html
- streamlit-run — https://docs.streamlit.io/develop/concepts/architecture/run-your-app
- streamlit-architecture — https://docs.streamlit.io/develop/concepts/architecture/architecture
- streamlit-caching — https://docs.streamlit.io/develop/concepts/architecture/caching
- streamlit-session — https://docs.streamlit.io/develop/concepts/architecture/session-state
- gradio-quickstart — https://gradio.app/guides/quickstart
- gradio-blocks — https://www.gradio.app/guides/blocks-and-event-listeners
- quarto-kernel — https://quarto.org/docs/advanced/jupyter/kernel-execution.html
- quarto-widgets — https://quarto.org/docs/interactive/widgets/jupyter.html
- quarto-publishing — https://quarto.org/docs/publishing/
- jupyter-source-registry — https://jupyter.org/
- jet-data-stats — crates/jet-codegen/src/Prelude/CoreLib/Top/DataStats.rs
- jet-data-tests — tests/dataflow_stream.rs
- jet-data-evaluator — crates/jet-codegen/src/Codegen/TIR/eval/data_calls.rs
- jet-notebook — crates/jet-repl/src/Notebook/document.rs
- jet-notebook-trust — crates/jet-repl/src/Notebook/kernel.rs

### cli (31 sources)

- BT — https://github.com/charmbracelet/bubbletea
- LG — https://github.com/charmbracelet/lipgloss
- BB — https://github.com/charmbracelet/bubbles
- HUH — https://github.com/charmbracelet/huh
- GUM — https://github.com/charmbracelet/gum
- VHS — https://github.com/charmbracelet/vhs
- WISH — https://github.com/charmbracelet/wish
- RATATUI-SITE — https://ratatui.rs
- RATATUI-WIDGET — https://ratatui.rs/concepts/widgets/
- RATATUI-LAYOUT — https://ratatui.rs/concepts/layout/
- RATATUI — https://github.com/ratatui/ratatui
- RATATUI-TEMPLATES — https://github.com/ratatui/templates
- INK — https://github.com/vadimdemedes/ink
- TEXTUAL — https://textual.textualize.io
- TEXTUAL-DEV — https://textual.textualize.io/guide/devtools/
- TEXTUAL-TEST — https://textual.textualize.io/guide/testing/
- CLAP — https://docs.rs/clap/latest/clap/
- CLAP-COMPLETE — https://docs.rs/clap_complete/latest/clap_complete/
- CLAP-MAN — https://docs.rs/clap_mangen/latest/clap_mangen/
- TYPER — https://typer.tiangolo.com
- TYPER-CMD — https://typer.tiangolo.com/tutorial/typer-command/
- OCLIF — https://oclif.io/docs/introduction/
- OCLIF-UX — https://oclif.io/docs/user_experience/
- OCLIF-TEST — https://oclif.io/docs/testing/
- COBRA — https://github.com/spf13/cobra
- RICH — https://rich.readthedocs.io/en/stable/
- RICH-CONSOLE — https://rich.readthedocs.io/en/stable/console.html
- RICH-TABLE — https://rich.readthedocs.io/en/stable/tables.html
- RICH-PROGRESS — https://rich.readthedocs.io/en/stable/progress.html
- INDICATIF — https://docs.rs/indicatif/latest/indicatif/
- NO-COLOR — https://no-color.org/
