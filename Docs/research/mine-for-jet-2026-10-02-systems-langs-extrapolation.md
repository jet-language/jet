# Mine for Jet, second pass: every comment answered, every source extrapolated (2026-10-02)

## Verdict

The first pass (`Docs/research/mine-for-jet-2026-10-01-systems-langs.md`) answered 36 of the 68 traced comment rows well. It answered 28 thinly and missed 4: the Nix replacement, the EYG sandbox as a WebAssembly replacement, replacing Docker and Nix, and per-language extrapolation. This pass closes all 68. It harvests 884 items from ten source and Jet maps, deduplicates 69 extrapolated ideas against Tower and code, probes Jet live, and records the work as follows:

- 7 new ballots in the owner's decide lane (3 full, 4 short), each a genuine owner gate;
- 11 new implementation cards that need no owner decision;
- evidence logged on 5 existing cards, and four open first-pass ballots corrected with ratified law they had missed.

The most important finding is about process, not languages. Docs/spec lags Tower. While drafting, I reached two duplicate cards, #4196 and #4202, and deleted them. #4196 repeated D-LITCARRIER1=D, under which `3.4` is already a `Decimal`. #4202 repeated D-DEV-DEFER1=A, under which `jet dev` already runs past type errors. Earlier drafts also nearly repeated D-NOTEBOOK-SURFACE1=D, whose Jupyter adapter shipped as #442, and D-UNSAFE-DEPS1=A. Every one of these was ratified but absent from, or stale in, the spec. Future mining must search `tower decision` records in full, not only titles and the spec.

## Method, coverage, and limits

- Eleven read-only Luna 5.6 harvests, notes under the deleted capture directory `target-mine-2026-10-02/notes/`:

  | Source | Items | Snapshot |
  |---|---|---|
  | Roc | 90 | `d16a6598` |
  | Odin | 96 | `7fac9818` |
  | Zig | 102 | `738d2be9` |
  | Mojo docs and stdlib (structure only, no code copied) | 91 | `7cb52002` |
  | Skip and Hack | 80 | — |
  | EYG and Unison | 100 | — |
  | Hare and Verse (Verse from docs and talks) | 64 | — |
  | GHC and Python/PyO3 | 80 | — |
  | Hardware exposure survey | 61 | — |
  | Jet broad map | 120 | — |
  | Jet platform map | — | — |

- One Luna dedupe sweep classified 69 candidates as shipped, ratified but unbuilt, declined, owned by an open card, partial, or absent, with file or Tower evidence for each.
- Live probes used a debug build of HEAD with `--cap-lints warn`, because HEAD fails `deny(warnings)` (#4163). They exercised `jet run` and AOT builds of decimal literals, `scope.guard` capture, bit writes, DataTree indexing, unmet-bound errors, and `??` blocks.
- Every full ballot had two fresh beginner (RLI5) passes and one adversarial Opus pass. Every review changed the draft. Two recommendations flipped: D-MEMRAW-EFFECT1 and D-PORTABLE1.
- Limits:
  - Luna agents could not report their model identity.
  - Claims marked "unverified" in the notes were not used as facts here.
  - The Rust compiler is frozen. Probes ran on it, but fixes belong in `Compiler/`.

## Comment traceability

Status A means answered, with the evidence and disposition shown. "1st" marks rows the first pass already answered well.

| # | Comment | Answer | Disposition |
|---|---|---|---|
| G1 | Mine Roc and Skip | Roc section below; Skip and Hack section below | 1st + this report |
| G2 | Skip: excellent compiler performance | Skip's speed comes from memoized functions over a recorded dependency graph that invalidates only affected results. Jet's matching lever is its semantic index and incremental build facts. | #3852 |
| G3 | Skip: improved syntax and built-ins | Skip's best features already exist in Jet: `.!` nested updates (Jet `&place`), readonly views (`View<T>`), frozen values (`freeze`), memoized fields, and reactive invalidation (`core.reactive`). Memoized functions exist too, as `#Memo` (#3975). | no new work |
| G4 | Skip: error and value handling | Skip uses exceptions plus Option. Jet's typed failure rails, inferred unions, and `?(context)` trails already exceed it. | D-FAIL-INFER-UNION1=A |
| G5–G8 | Roc errors, inference, JSON, error unions | 1st | D-FAIL-INFER-UNION1, #4182, #4184 |
| G9 | Mojo: what to take, avoid, learn | Mojo section below | this report |
| G10, M13, M17 | Mojo: libraries hold most of the language | Jet ships 40,562 lines of Core Jet. Its Rust Prelude runtime is about as large (222,761 lines) as the self-hosted compiler (222,833). The "move it to libraries" lever is therefore the Prelude, already tracked. | #4179, #3549 |
| G11 | Mojo: keep the compiler light | Same measurement. The heavy part is runtime machinery that should become Core Jet, not compiler passes. | #3549 |
| G12, M16 | Mojo: 2D memory, tiles, matrices | Kernel tiles were ratified as scoped values. Mojo's layout algebra (shape × stride composition) matches `core.compute` strided views. | D-GPU-TILE-SCOPE1=B, #3215 |
| G13, E4 | EYG: narrow net and file rights | 1st | #4165 |
| G14 | EYG: the type system | EYG infers row-typed records and effects. Jet takes the effect rows and handlers (#4180). It declines extensible structural records, which would be a second record mechanism beside nominal structs (I8). | #4180 |
| G15, E3 | EYG: AST focus, editor | 1st | #4171 |
| G16 | Zig: interoperability wedge | The C import engine, plus `jet cc`/`jet c++`, `jet build --lib`, and cross toolchains. | #4174, #758, #1344 |
| G17, Q3 | Jet as a Nix and Docker replacement | Platform section below. Jetpack already owns a Nix evaluator, a content-addressed store, and build sandboxes. The missing piece is the shipped app unit. | #4195 (D-PORTABLE1) |
| Q2 | EYG sandbox as a WebAssembly replacement | Platform section below. Recommended: native builds plus a required confined WebAssembly build. The radical "Jet's own portable form" is option B. | #4195 |
| G18, M8 | prep should unify generics and more | The type-function ballot covers generics. prep already does embedding, discovery, and layout facts. Zig's `inline else` and Mojo's `comptime for` need nothing new. | #4164, #4166 |
| G19 | Thorough Roc research | Roc section below (90 items) | this report |
| G20 | Research Verse | Verse section below | this report |
| Z1, Z2, Z4–Z6 | Pointers, FFI defer, `<>`, prep limit, clean | 1st | #3645, #4164, #4166, #4186, #2531 |
| Z3 | Leak tracking, allocator failure, safe low-level types | The leak report is #4176. Allocation-failure injection already ships (D-ALLOCFAIL1=A, `Examples/features/memory/try_allocation.jet`). Safe low-level types become ballots for bit fields (D-BITS1) and small floats (D-LOWFLOAT1). | #4176, #4197, #4200 |
| H1 | Hare: stdlib, errors, types | Hare's stdlib mandate becomes a Core coverage ratchet. `nomem` is Jet's `AllocError`. Tagged unions and `!` errors already exist. | #4213 |
| H2 | Hare: linear types | 1st | #4139 |
| S1, S2 | SPJ: typed stack, desugar first | 1st | #4175 |
| O1, O3, O4 | Odin: SoA, methods, data philosophy | 1st | #4169, #4170, #4185 |
| O2, M3, M4, M14 | Hardware: bottom-up and top-down | Hardware matrix below | matrix rows |
| P1 | PyO3: fill both Python and Rust roles | Python-side gaps were checked. Notebooks ship (#442, D-NOTEBOOK-SURFACE1=D), inline script dependencies ship, and tensor broadcasting ships. JSON indexing is a ballot. Stable-ABI wheels are logged on host packaging. | #4203, #1345 |
| P2 | Beat Rust's Result | 1st | — |
| M1, M6, Q7 | Gradual and dynamic typing | Per-call checked helpers (#4182), one-step JSON indexing (#4203), and `jet dev` running past type errors, already ratified (D-DEV-DEFER1=A). | #4182, #4203 |
| M2, M12 | Parallelism first-class and via libraries | `task.all`, `task.any`, `task.race`, and `task.group` ship (Docs/spec/spec.md:2100-2229). The measured parallel gate is on #2922. Automatic parallel loops are ratified as D-ACCEL1=A. | #2922 |
| M5, M7, M10, M11, M15, M19, M20 | Mojo compiler and library topics | 1st | #4179, #4177, #4178, #4183, #4173 |
| M9 | Move and copy constructors, destructors, C malloc | Jet already covers these: `^` moves, `~` copies, `Close` and `defer close(^r)` for destruction, `#Close(fn)` for foreign handles, and `AllocError` for allocation failure. Mojo's explicit lifecycle methods would add a second mechanism. | no new work |
| M18 | Learn from the open Mojo stdlib | Mojo section below | this report |
| Q1 | Blueprint-style visual editing | Canvas is the visual editor. The first pass recommended replaying stale Canvas edits. | #4171 |
| Q4 | Content-addressed code: real or a toy? | Jet already gets the practical benefits: content-addressed packages, builds, and caches in the Hangar store. Hashing individual definitions (Unison) pays off only with a code database instead of files and git, which Jet does not need. | no new work |
| Q5 | Pointers with tracked memory effects | Ratified: pointer stepping (D-RAWPTR2=A) and per-package gate lists (D-UNSAFE-DEPS1=A). New: a per-function promise to reach no `#Unsafe` code. | #4198 (D-MEMRAW-EFFECT1) |
| Q6 | Interop: all four pieces | C headers (#4174), drop-in `jet cc`, foreign builds (#1347), C ABI export (`jet build --lib`) | #4174, #1347 |
| X | Extrapolate per language | Sections below | below |

## Per-language extrapolation

Each section lists what to take, how Jet improves on it, what to avoid, and the silhouettes it reveals in Jet.

### Zig

- **Take.**
  - Failure-only cleanup (`errdefer`) becomes ballot D-DEFER-FAIL1 (#4201). The probe found that Jet's usual workaround, a guard testing a `done` flag, runs on success too, because the escaping lambda copies the flag. That trap is now #4207.
  - Packed structs and arbitrary-width integers become ballot D-BITS1 (#4197).
  - The labeled `switch` with `continue :label` is how Zig's tokenizer reaches threaded dispatch. #4209 measures whether Jet's `loop` plus `if state == {…}` loses before any surface is proposed, as the performance-surface law requires.
- **Already in Jet.**
  - Allocation-failure injection (D-ALLOCFAIL1=A), doc tests (Examples/features/comptime/doctests.jet), and value loops with `break value`.
  - Closed enums with tolerant callers (D-ENUM-EVOLUTION1=A) cover non-exhaustive enums.
  - Runtime faults are registered diagnostics, which makes them Jet's illegal-behavior taxonomy.
  - Unused locals warn as L0101, and policy can deny the lint.
- **Improve on Zig.**
  - Zig's packed structs fill from the low bit while its byte reads are explicit code. Jet's ballot makes one high-bit-first vocabulary serve reading, writing, and registers.
  - Zig's `@setRuntimeSafety` turns checks off silently. Jet's hardened profile and sentries keep the check and move its cost.
- **Avoid.** Global error sets. `undefined` as an ordinary value; Jet's `uninit` is expert-gated.

### Odin

- **Take.**
  - The implicit `context` payload (allocator, temporary allocator, logger, assertion handler, random generator) is the checklist for Core service providers in `#Context`. Logged on #4180.
  - Enumerated arrays `[Enum]T` can come without syntax: a map keyed by a fieldless enum can lower densely. Logged on #3133 with `bit_set`.
- **Already in Jet.**
  - `or_break` and `or_continue` are `?? break` and `?? next` (Docs/spec/syntax-decisions.md:1702-1704).
  - `#partial switch` is an explicit `else` arm.
  - The tracking allocator's leak report is #4176.
  - Batteries such as games and UI ship in Core.
- **Improve on Odin.** Odin's context is untyped and dynamically scoped. Jet's `#Context` is typed and checked, so a test that replaces the logger cannot miss a callee.
- **Avoid.** No package manager. Zero-initialization as a silent default; Jet requires initialization or expert `uninit`. `#no_bounds_check`; Jet removes bounds checks by proof (D-TYPE2-REFINE1).

### Roc

- **Take.**
  - Rejecting bidirectional-control characters in source (Trojan Source) is #4204, a security diagnostic with no owner gate.
  - Tail-recursion-modulo-cons, which builds lists recursively without growing the stack, is #4211.
  - `roc bump`, a semver bump computed from the API diff, becomes #4215 on top of Jet's existing API snapshots.
- **Already in Jet.**
  - `dbg` is D-DBG1=A.
  - Decimal-by-default literals are D-LITCARRIER1=D. The probe confirmed that `0.1 + 0.2 == 0.3` on `jet run` and AOT, while a `Float` parameter keeps binary floats.
  - Run-despite-errors is D-DEV-DEFER1=A.
  - `expect` is `assert` plus `#Test` and doctests.
  - Roc's post-check LIR, where backends may not decide reference counting, mirrors Jet's MIR (D-TIER-FORM1) and I9.
- **Improve on Roc.** Roc's generated docs do not type-check code in doc comments; Jet's doctests run them. Roc rejects Option entirely; Jet keeps `T?` for absence and typed failures for reasons, which serves both beginners and experts.
- **Avoid.** No higher-kinded types as a permanent ban, which is not Jet's call to copy. The two-language compiler, since Jet self-hosts.

### Mojo (design and structure only)

- **Take.**
  - A trait author's own advice on the unmet-bound error becomes ballot D-BOUND-MSG1 (#4199), using Mojo's constraint message and Rust's `on_unimplemented`. The probe found Jet's E0905 correct but generic.
  - Small floats (`float16`, `bfloat16`, `float8`) become ballot D-LOWFLOAT1 (#4200).
  - Conditional conformance (a generic type implements a trait only when its parameter does) is #4214, an investigation first: Jet auto-derives built-in traits, but no example shows a hand-written conditional implementation.
- **Already in Jet.**
  - Doc generation is ratified (D-DOC-GEN1=A, D-JETDOC1=B).
  - Parametric `raises` is Jet's failure-generic callbacks.
  - Infer-only `//` parameters were declined with D-CAP8.
  - Origins are covered by inferred view provenance, which keeps lifetimes out of the beginner surface.
  - Benchmarks run through `jet perf`.
- **Stdlib structure lessons.** One module per hardware concept. A data-type parameter on SIMD values instead of one type per width, which is D-LANES1. Layout objects that compose shape and stride, which `core.compute` views already do. Mojo puts `Int` and `Float` in the library; Jet's analogue is #4179.
- **Avoid.** Python-shaped `def` versus `fn` duality. Exposing MLIR in user code.

### Skip and Hack

- **Take.** Hack contexts in signatures, which make hidden capabilities visible, sharpen the per-function `#Unsafe` ballot (D-MEMRAW-EFFECT1).
- **Already in Jet.**
  - Effect rows are coeffects, and `-[via f]>` is Hack's dependent context.
  - Readonly views, nested place updates, frozen values, reactive invalidation, and typed HTML (`HTML{…}`, Hack's XHP) all exist.
  - `task.race` and its siblings cover Skip's async scheduler.
- **Improve on Skip.** Skip needs two closure kinds (`->` and `~>`) for capture safety. Jet's escape analysis decides it with one, but the probe shows the copy is silent. The fix is a warning (#4207), not a second closure kind.
- **Avoid.** Hack shapes, a second structural record system (I8). Skip's whole-program memoization runtime.

### EYG and Unison

- **Take.**
  - The host handles every effect and the shipped program is checked code: this is the model for D-PORTABLE1, whose option B is the closest Jet analogue.
  - Typed holes listing valid fits extend Jet's existing `#Todo` goal (#4210).
- **Already in Jet.** Typed executable docs (doctests). The current directory is explicit inside the `FS` and `Env` effects. Loaded libraries are refused before they run when they ask for more than the host grants (D-LIB-DYNTRUST1=A).
- **Avoid.** Shipping code by hash to remote nodes (Unison): research value, no Jet workload needs it. Y-combinator recursion and no loops.

### Hare and Verse

- **Take.**
  - Hare's stdlib mandate, that everything is documented and tested, becomes a Core coverage ratchet (#4213).
  - Verse's `<localizes>` typed messages were logged on the catalog investigation (#3289), building on D-CORE-CATALOG1=A.
- **Already in Jet.**
  - Verse's `race`, `rush`, and `branch` map to `task.race`, `task.any`, and `task.group`.
  - Persistable evolution maps to schema migrations, and live update to D-SERVICE-UPGRADE1=D and D-DX-LIVE1=A.
  - Hare's `nomem` is `AllocError`.
- **Avoid.** Verse's failure-driven logic programming with speculative `[]` calls. It is elegant, but it is a second control-flow model beside `?` and `??`.

### GHC and Python

- **Take.** Typed-hole fits (#4210). Stable-ABI Python wheels (logged on #1345).
- **Already in Jet.**
  - Deferred type errors (D-DEV-DEFER1=A).
  - STM `retry`: `#Transact` plus `Condition.wait` (D-STM1=A).
  - PEP 723 inline dependencies.
  - NumPy broadcasting in `core.compute`.
  - Notebooks with a Jupyter adapter (#442).
  - An analytics query surface (D-SQL-SURFACE1=C).
- **Avoid.** DerivingVia and rewrite RULES. Both let libraries change meaning or performance invisibly, against I3 and the paired-cell rule.
- **Python-side silhouette.** Nested JSON reads need one call and one fallback per level, and the natural `?.` chain fails with a confusing E2402. This becomes ballot D-DATATREE-PATH1 (#4203).

## Silhouettes found in Jet's own patterns

These are places where Jet's existing pattern implies a missing piece. Each was found by a live probe.

| Pattern | What is missing | Record |
|---|---|---|
| Binary patterns read bit fields | Nothing writes them; `#Layout(packed)` is reserved | D-BITS1 (#4197) |
| `scope.guard` runs on every exit; `#Transact` has failure-only hooks | No failure-only cleanup outside transactions | D-DEFER-FAIL1 (#4201) |
| Escaping lambdas copy captured locals | A later write is silently invisible | #4207 |
| `?? { … }` blocks need a value or a diverging tail | E0116's Fix repeats the rejected form | #4208 |
| Lists and maps use `[ ]` and `.get` | DataTree uses neither | D-DATATREE-PATH1 (#4203) |
| Gates are listed per package | No per-function promise | D-MEMRAW-EFFECT1 (#4198) |
| `Output.Bundle` exists as a kind | No portable, confined app form | D-PORTABLE1 (#4195) |
| `#Todo` reports its expected type | No candidate fits | #4210 |
| API snapshots exist | Publishing trusts the written version | #4215 |

## Platform: replacing Docker, Nix, and WebAssembly

- **What Jet already owns.**
  - Content-addressed store and closures: Hangar.
  - A native Nix evaluator with self-verifying store paths (D-JPK-NIXENGINE1=D).
  - Build-step sandboxes (D-JPK-SANDBOX2=D).
  - OCI images projected from environments (D-ENV-IMAGE1=A).
  - wasm32-wasip2 targets (D-WASISRV1=A) and sandbox components with no ambient authority.
  - Wasmtime plugins with lent authority (D-PLUGIN1=A, D-DEP-WASM1=A).
  - Load-time rights checks for Jet libraries (D-LIB-DYNTRUST1=A).
- **Nix.** For Jet packages Jet already replaces Nix: it reproduces builds from pinned inputs without an installed Nix.
- **Docker.** Jet emits OCI images but has no run-time confinement of its own, because the runtime OS sandbox is unratified and unproven (#398).
- **WebAssembly.** Jet emits it and hosts it, but nothing combines it with native speed in one artifact.
- **The gap is the shipped unit.** D-PORTABLE1 recommends one signed `.jetapp` form of `Output.Bundle`: native builds for listed targets, plus a required WebAssembly build that any other machine runs confined. Each host honestly reports whether the app is confined, verified, or TRUSTED.
- **Better than WebAssembly.** Rights are checked against Jet's own effect facts. Listed machines get native speed. The WebAssembly build is the safety net, not the ceiling.
- **Radical alternative.** Option B ships Jet's own MIR to hosts that have Jet. It is the closest to EYG, and the owner may prefer it.
- **Unratified proposal.** `Docs/proposals/jetpack/portable-platform-specification.md` (JPM/1, JPH/1) had no card; D-PORTABLE1 replaces it as the decision point.

## Hardware matrix

Bottom-up asks what the hardware offers. Top-down asks how Jet code reaches it.

| Hardware capability | Jet today | Record |
|---|---|---|
| SIMD, 128 to 512 bits, plus scalable SVE and RVV | Auto-vectorization (D-SIMD3=B); 20 named lane types | D-LANES1 (#4167) |
| Per-CPU feature dispatch | Release builds pass `target-cpu=native` against law | D-CPU-DISPATCH1 (#4168), #4192 |
| Matrix tiles (AMX, SME, tensor cores) | Kernel-scoped tiles | D-GPU-TILE-SCOPE1=B, #3215 |
| fp16, bf16, fp8 | None | D-LOWFLOAT1 (#4200) |
| Prefetch, non-temporal stores | None | logged on #4168 |
| Atomics with orderings | Sequentially consistent, plus release/acquire `publish`/`observe` | D-ATOMIC-WIDTH1=A |
| GPU shared memory, subgroups, atomics | Ratified kernel scopes | D-GPU-GROUP-SCOPE1=A, D-GPU-SUBGROUP-SCOPE1=B, D-GPU-ATOMIC-SCOPE1=A |
| Memory tagging (MTE), capabilities (CHERI) | Software sentries and hardened profile | #4205 |
| Hardware counters (PMU) | Wall time only | #4212 |
| Bit-exact registers and volatile access | `mem.volatile_read`/`volatile_write`; no packed layout | D-BITS1 (#4197) |
| Hardware RNG | OS CSPRNG, which already mixes RDRAND | no new work |

Notes on the matrix:
- Relaxed atomic orderings were deliberately excluded by D-ATOMIC-WIDTH1=A. Revisiting that needs a measured loss first.
- Jet has no direct RDRAND path because the OS generator is the right default; a separate instruction surface would add risk without benefit.

## Owner gates created in this pass

| Ballot | Card | Profile | Recommendation |
|---|---|---|---|
| D-PORTABLE1 | #4195 | full | A: native builds plus a confined WebAssembly build |
| D-BITS1 | #4197 | full | A: byte-building holes and high-bit-first packed structs |
| D-MEMRAW-EFFECT1 | #4198 | full | A: a deny-only `Unsafe` fact per function |
| D-BOUND-MSG1 | #4199 | short | A: `#Hint` on a trait |
| D-LOWFLOAT1 | #4200 | short | A: `F16` and `BF16`; 8-bit formats as storage only |
| D-DEFER-FAIL1 | #4201 | short | A: `scope.on_fail` |
| D-DATATREE-PATH1 | #4203 | short | A: `[ ]` and `.get` on DataTree |

Corrections to open first-pass ballots, from the adversarial review:
- D-LANES1 now cites D-FRED1=A.
- D-SOA-SITE1 and D-CPU-DISPATCH1 now cite D-ACCEL1=A.
- D-AUTH-NARROW1 now cites D-CORE-FILEDIR1=A.

## Finding dispositions

<!-- audit-dispositions:v1 -->
| finding | disposition | target or reason |
| --- | --- | --- |
| F1 portable app unit | card | #4195 |
| F2 bit-field writes and packed structs | card | #4197 |
| F3 per-function unsafe reachability | card | #4198 |
| F4 trait-author bound hints | card | #4199 |
| F5 small floating-point types | card | #4200 |
| F6 failure-only cleanup | card | #4201 |
| F7 DataTree indexing | card | #4203 |
| F8 bidi-control source rejection | card | #4204 |
| F9 hardware memory tagging for hardened profile | card | #4205 |
| F10 stale capture warning | card | #4207 |
| F11 E0116 Fix text in `??` blocks | card | #4208 |
| F12 dispatch-loop performance pair | card | #4209 |
| F13 typed-hole fits for `#Todo` | card | #4210 |
| F14 tail-recursion-modulo-cons | card | #4211 |
| F15 hardware counters in jet perf | card | #4212 |
| F16 Core doc and test coverage ratchet | card | #4213 |
| F17 conditional trait implementations | card | #4214 |
| F18 semver bump from API diff | card | #4215 |
| F19 enum-indexed dense storage | card | #3133 |
| F20 Odin context payload for providers | card | #4180 |
| F21 typed localizable messages | card | #3289 |
| F22 stable-ABI Python wheels | card | #1345 |
| F23 prefetch and non-temporal stores | card | #4168 |
| F24 decimal-by-default literals | decision | D-LITCARRIER1=D |
| F25 run despite type errors | decision | D-DEV-DEFER1=A |
| F26 debug print in pure code | decision | D-DBG1=A |
| F27 allocation-failure injection | decision | D-ALLOCFAIL1=A |
| F28 package-level unsafe dependency list | decision | D-UNSAFE-DEPS1=A |
| F29 non-exhaustive enums | decision | D-ENUM-EVOLUTION1=A |
| F30 STM blocking retry | decision | D-STM1=A |
| F31 doc generation | decision | D-DOC-GEN1=A |
| F32 Jupyter notebooks | decision | D-NOTEBOOK-SURFACE1=D |
| F33 atomic memory orderings | decision | D-ATOMIC-WIDTH1=A |
| F34 GPU shared memory and subgroups | decision | D-GPU-GROUP-SCOPE1=A |
| F35 live code update | decision | D-DX-LIVE1=A |
| F36 Verse race and branch verbs | no-action | shipped: task.race, task.any, task.group (Docs/spec/spec.md:2100-2229) |
| F37 Skip two closure kinds | no-action | escape analysis decides capture with one closure kind; the copy trap is F10 |
| F38 Hack shapes and extensible records | no-action | a second structural record system conflicts with I8 |
| F39 DerivingVia and rewrite RULES | no-action | library-defined meaning or optimization conflicts with I3 and the paired-cell rule |
| F40 Unison hash-shipped execution | no-action | no Jet workload needs a code database; Hangar already content-addresses packages and builds |
| F41 Verse speculative failure `[]` | no-action | a second control-flow model beside `?` and `??` |
| F42 hardware RNG instruction | no-action | the OS CSPRNG already mixes it and is the safe default |
| F43 Odin `#no_bounds_check` | no-action | Jet removes checks by proof (D-TYPE2-REFINE1); unchecked access stays under #Unsafe |
<!-- /audit-dispositions -->

Strongest unverified assumption: D-PORTABLE1 assumes a confined WebAssembly build of a typical Jet app runs fast enough to be an acceptable fallback; no paired cell has measured it yet.
