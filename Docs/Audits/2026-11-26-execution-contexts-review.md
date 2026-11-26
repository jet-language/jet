# Adversarial Design Review: One Rule for Execution Contexts (#7015 / P17)

- **Date:** 2026-11-26
- **Scope:** Phase 1 adversarial design review of card `#7015` ("One rule for execution contexts: code may run where its effect row is covered, with one diagnostic naming the uncovered effect and call path"), implementing P17 from `Docs/Proposals/2026-11-24-greenfield-language.md` (Part 2, Section 2.5 & Section 3.6; reviewed in `Docs/Audits/2026-11-24-greenfield-language-review.md`).
- **Terminology:** Uses Jet terms as defined in [Jet vocabulary](../Spec/vocabulary.md).
- **Verdict:** **NEEDS BALLOT** (five load-bearing design questions — Q1 through Q5 — must be ratified before Phase 2 implementation begins).

---

## Executive Summary

P17 proposes replacing Jet's fragmented per-environment restrictions with **one rule**: an execution context (a hosted target, a no-OS board, the browser, a WebAssembly module, a GPU/parallel kernel, a hardware interrupt, a realtime audio callback, compile time, a C callback, a test world, or a user-declared context) is defined by the effects it provides and denies. Code may run in a context if and only if its inferred effect row is covered by that context, and every refusal emits **one diagnostic** naming the uncovered effect, the context, and the shortest call path that reaches it.

Auditing the live compiler reveals that Jet currently enforces **at least 15 distinct per-context checkers** scattered across `JetSema`, `JetDriver`, `JetEval`, `JetFoundation`, and `JetCLI`, emitting **25 different error codes** pinned across **186 test files**, using **7 separate call-graph traversals** with inconsistent call-path reporting. Worse, the live audit uncovers **three real semantic holes** today:

1. **The deny-only trap (`Checks.jet:1130-1145`):** `sema_effect_uncovered` strips deny-only effects (`Mem.Alloc`, `Mem.Rc`, `Mem.Gc`, `Mem.Stack`, `Time.Wait`, `Panic`, `Unsafe`). As a direct consequence, [`Core/Rt/Rt.jet:82`](../../Core/Rt/Rt.jet) bounds realtime audio callbacks with `fn([Float]) -[]>`, which **silently permits heap allocation, reference counting, and blocking waits** inside realtime callbacks.
2. **Hardcoded module-name and method-name bypasses:** No-OS OS-module checks (`Calls/Core.jet:650-671`, `TargetDenials.jet:127-222`) hardcode 7 module paths; browser denials (`TargetMachine.jet:563`) only deny `Exec` and `FFI.C` while web placement (`WebPartition.jet:193`) only checks `DB` and `Secret`, leaving `FS` unguarded at the browser boundary; comptime network checks (`Checks.jet:944-962`) hardcode `method == "fetch"` to exempt content-addressed `core.net.fetch(url, sha256:)`.
3. **Missing call-path diagnostics:** While `E3401`, `E0921`, `E3303`, and hook `E3410` print a call chain, `E1123` (target denials), `E3301` (direct no-OS calls), `E2823`/`E2824` (web placement), `E3412` (comptime net), and `E0713` (`#On(.Deep)`) do not.

The core unification thesis of `#7015` is **sound and high-leverage**. However, five design decisions cannot be unilaterally invented by an implementation lane without creating syntax collisions (notably with `#Context(dir:, env:)` from `#7016`), breaking existing diagnostic contracts, or silently misclassifying deny-only effects and structural non-effect obligations. This review maps all prior art, inventories every live checker, attacks every claim, and specifies ballots **Q1–Q5** plus a concrete binding cutover list.

---

## 1. Prior Art and Strongest Existing Solutions

| System | Mechanism | Primary Source | What It Gets Right | Where It Falls Short for Jet |
|---|---|---|---|---|
| **WebAssembly Component Model & WASI Worlds** | Declarative `world` listing imported interfaces (capabilities); a component runs in a world iff its imports are a subset of the world's imports | Bytecode Alliance, *Component Model Design / WIT Worlds* & WASI 0.2 (2024) | Closest industrial precedent to P17: one subset rule (`imports(component) ⊆ provides(world)`), extensible to new targets (`wasi:cli/command`, `wasi:http/proxy`, custom host worlds) with zero compiler changes | Operates only at coarse component/module boundaries; cannot distinguish realtime callbacks, interrupts, comptime, or pure functions inside one program |
| **Koka** | Algebraic effect rows (`<exn, div, io \| e>`); handlers discharge effects from the row; top-level context = installed handler set | Daan Leijen, *Algebraic Effects for Functional Programming* (MSR-TR-2016-29; POPL 2017) | Clean row-polymorphic composition: higher-order functions automatically propagate effect rows; zero duplicate function coloring | Does not model hardware/target resource ceilings (`!Mem.Alloc`, `!Mem.Stack(above: N)`, no-OS MMIO/DMA) or foreign-thread task-context absence |
| **Eff & OCaml 5** | Deep and shallow algebraic effect handlers | Bauer & Pretnar, *Programming with Algebraic Effects and Handlers* (JLP 2015); Sivaramakrishnan et al., *Retrofitting Effect Handlers onto OCaml* (PLDI 2021) | Demonstrates low-overhead one-shot handler dispatch on native stacks | OCaml 5 effects are **untyped** — an uncovered effect crashes at runtime (`Effect.Unhandled`), unacceptable for targets, interrupts, kernels, or comptime |
| **Rust** | Separate systems: `const fn`, `#[no_std]`/`alloc`, `Send`/`Sync`, `#[target_feature]`, stalled Keyword Generics | Rust Reference; RFC 2632 (`const` traits); Wuyts et al., *Keyword Generics* (2022–2024) | Strong zero-cost enforcement of each individual property | Four incompatible coloring/feature axes with separate diagnostics, duplicate std/core/alloc crates, and function-duplication pressure |
| **Zig** | Unified `comptime` evaluator + lazy `@compileError` | Zig Language Reference (0.13/0.14) | Single language across comptime and runtime with no `const fn` duplication | Enforced lazily at instantiation/evaluation time without static call-path diagnostics or declarative capability sets for interrupts/realtime/GPU |
| **Swift** | Global actors (`@MainActor`), actor isolation, `Sendable` | Swift Evolution SE-0306 (Actors), SE-0316 (Global Actors), SE-0414 (Region-based Isolation) | Static isolation-domain checking across call sites and closures with clear cross-domain diagnostics | Models thread/actor affinity only; does not unify I/O, allocation, comptime, or freestanding target providers |
| **CUDA / SYCL / Metal** | Execution-space qualifiers (`__host__`, `__device__`, `SYCL_EXTERNAL`) | NVIDIA CUDA C++ Programming Guide; Khronos SYCL 2020 Specification | Enforces device-only API restrictions and GPU kernel call-graph closure | Binary function coloring requiring duplicated `__host__ __device__` annotations and separate compiler pass diagnostics |

**Design takeaway for Jet:** Combining **WASI Worlds' declarative provider-set subset model** with **Koka's inferred effect rows** and **Jet's existing deny-only resource lattice (`!Mem.Alloc`, `!Mem.Stack`, `!Time.Wait`, `!Panic`, `!Unsafe`)** outperforms every single-axis precedent — provided Jet explicitly handles both *positive capability sets* and *negative resource denials*, and keeps *non-effect structural proofs* honest instead of faking them as effects.

---

## 2. Complete Map of Every Per-Context Checker Today

Every per-context restriction currently enforced in the compiler was audited directly against the tree:

| # | Context Today | Enforcement Site (`file:line`) | Error Code(s) | Pinned Test Files | Uses Solved Effect Graph? | Emits Call Path? |
|---|---|---|---|---|---|---|
| 1 | **Pure functions (`-[]>`)** | [`PureRoot.jet:30-188`](../../Compiler/JetSema/Source/Sema/Effects/PureRoot.jet), [`Checks.jet:817-838, 1551-1610, 5013`](../../Compiler/JetSema/Source/Sema/Effects/Checks.jet) | `E3401`, `E3403` | 24 + 12 | Yes + separate AST/body walk | Yes (`E3401`), single-site (`E3403`) |
| 2 | **Comptime (`#run` & module-level values)** | [`Checks.jet:924-962`](../../Compiler/JetSema/Source/Sema/Effects/Checks.jet), [`Native.jet:1255-1259`](../../Compiler/JetEval/Source/IR/Native.jet), [`Pipeline.jet:2861`](../../Compiler/JetDriver/Source/Driver/Pipeline.jet), [`Assignments.jet:899`](../../Compiler/JetSema/Source/Sema/Statements/Assignments.jet) | `E3403`, `E3412`, `E3410`, `E3411`, `E0643`, `E2914` | 12, 1, 13, 14, 9, 1 | Mixed: sema call-site check + evaluator trap + driver fallback | No (evaluator/call-site only) |
| 3 | **`#Impure` comptime gate (`Meta.Host`)** | [`Native.jet:1192-1259`](../../Compiler/JetEval/Source/IR/Native.jet), [`Core/Compiler/Meta.jet:21-29`](../../Core/Compiler/Meta.jet) | `E3410`, `E3411` | 13 + 14 | Evaluated at comptime execution | No |
| 4 | **Compiler hooks / `meta.events` handlers** | [`Checks.jet:2497-2615`](../../Compiler/JetSema/Source/Sema/Effects/Checks.jet) | `E3410` | (shared with #2) | Yes (call-graph walk for `Exec`) | Yes (`a → b → core.process.call`) |
| 5 | **No-OS targets (OS-requiring Core modules)** | [`Calls/Core.jet:650-671`](../../Compiler/JetSema/Source/Sema/Calls/Core.jet), [`TargetDenials.jet:127-222`](../../Compiler/JetSema/Source/Sema/TargetDenials.jet) | `E3301` | 9 | Separate BFS over `call_edges`/`value_edges` + hardcoded 7-module list | Transitive callee only, not full chain |
| 6 | **No-OS heap-free targets** | [`Checks.jet:6198-6250`](../../Compiler/JetSema/Source/Sema/Effects/Checks.jet), [`TargetDenials.jet:251-253`](../../Compiler/JetSema/Source/Sema/TargetDenials.jet) | `E3303` | 2 | Yes (memory-event walk) | Yes (first own-package crossing + site) |
| 7 | **No-OS target capabilities (clock, entropy, sink, scheduler)** | [`TargetDenials.jet:291-315`](../../Compiler/JetSema/Source/Sema/TargetDenials.jet), [`TargetMachine.jet:7-43, 1616-1627`](../../Compiler/JetFoundation/Source/Target/TargetMachine.jet) | `E3311` | 4 | No — per-module `core_usage` API string scan | No |
| 8 | **Browser target denied effects** | [`TargetDenials.jet:34-117, 275-277`](../../Compiler/JetSema/Source/Sema/TargetDenials.jet), [`TargetMachine.jet:562-565`](../../Compiler/JetFoundation/Source/Target/TargetMachine.jet) | `E1123` | 2 | Yes (separate BFS, hardcoded `["Exec", "FFI.C"]`) | No |
| 9 | **Hardware interrupt handlers** | [`TargetMachine.jet:123-126, 1709-1712`](../../Compiler/JetFoundation/Source/Target/TargetMachine.jet) | `E3314` | 3 | Target profile validation only (not wired into general sema call graph) | No |
| 10 | **Web JS/Wasm partition & App Server/Client placement** | [`WebPartition.jet:153-267`](../../Compiler/JetSema/Source/Sema/WebPartition.jet) | `E-WEB-TARGET-BROWSER`, `E-WEB-CROSS-PARTITION`, `E2823`, `E2824` | 2, 4, 1, 1 | Separate reachability graph (`SEMA_WEB_SERVER_RESOURCES = ["DB", "Secret"]`) | No |
| 11 | **C FFI callbacks (foreign OS thread)** | [`Calls/Checking.jet:425-491, 735-745`](../../Compiler/JetSema/Source/Sema/Calls/Checking.jet) | `E3218`, `E3203` | 2 (+ C ABI tests) | AST scan (single-body, no calls allowed in callback subset) | No |
| 12 | **`#Kernel(.parallel)` (compute/GPU worker)** | [`Kernel.jet:9-221`](../../Compiler/JetSema/Source/Sema/Kernel.jet) | `E1102` | 29 (shared w/ sendability) | AST allowlist (`SEMA_KERNEL_SAFE_COMPUTE_CALLS`) + zero-effect check | No |
| 13 | **Autodiff (`compute.gradient`/`vjp`/`jvp`)** | [`Calls/ComputeModels.jet:362`](../../Compiler/JetSema/Source/Sema/Calls/ComputeModels.jet) | `E0112` | (shared w/ type errors) | Checks declared signature is pure | No |
| 14 | **Realtime / audio callbacks (`core.rt.callback`)** | [`Core/Rt/Rt.jet:82`](../../Core/Rt/Rt.jet), [`Checks.jet:5560-5570`](../../Compiler/JetSema/Source/Sema/Effects/Checks.jet) | `E0747` | 9 | Callback row bound `-[]>` (**bug: strips deny-only `Mem.Alloc`/`Mem.Rc`/`Time.Wait`/`Panic`**) | Yes (callback bound chain) |
| 15 | **Scoped caps, deep checks, live queries, memo, eval probe, memory denials, package authority** | [`Checks.jet:1443, 5523, 5543, 5560, 7439-7458, 7535`](../../Compiler/JetSema/Source/Sema/Effects/Checks.jet), [`Calls/Live.jet:477-500`](../../Compiler/JetSema/Source/Sema/Calls/Live.jet), [`EvalProbe.jet:442, 530-533`](../../Compiler/JetSema/Source/Sema/EvalProbe.jet), [`CLI/Authority.jet:408`](../../Compiler/JetCLI/Source/CLI/Authority.jet), [`Calls/Direct.jet:298`](../../Compiler/JetSema/Source/Sema/Calls/Direct.jet) | `E0740`, `E0712`, `E0713`, `E0921`, `E0938`, `E1803`, `E3405` | 18, 6, 2, 6, 1, 17 | Yes (multiple separate passes) | Only `E0921` and `E0740`/`E0712`/`E0747` partially |

**Additional gaps observed in the map:**
- **UI main-thread affinity:** There is currently **no** `UI` or `UI.MainThread` effect in [`Compiler/Prelude/Effects.jet`](../../Compiler/Prelude/Effects.jet) or [`Core/Effects/Effects.jet`](../../Core/Effects/Effects.jet), and no sema checker verifying UI-thread confinement (only `#Reactive` at comptime `E2914` and game frame scheduling `E2967`).
- **Sandbox / guest plugins:** [`Calls/Plugin.jet`](../../Compiler/JetSema/Source/Sema/Calls/Plugin.jet) checks component ABI descriptors; host sandboxing is enforced in package authority (`CLI/Authority.jet`, `E1803`) and bubblewrap process specs, not as a unified execution-context provider set.
- **Test world (`#Test`):** Tests currently inherit full ambient host authority unless manually narrowed with `#FX(...)` or `--deny`; there is no declarative `Test` context profile yet.

---

## 3. Adversarial Attacks on Claims and Edge Cases

### Attack 1: `#Context` Syntax Collision and Source-Declared Contexts (`c4`)

Criterion `c4` requires that **a new execution context can be declared in Jet source without a compiler change** (e.g., an audio-thread context, a sandboxed plugin context, or a custom embedded board context).

**Collision found in live tree:** `#Context(...)` is **already shipped** in [`Statements/ContextBlocks.jet:21-68`](../../Compiler/JetSema/Source/Sema/Statements/ContextBlocks.jet) and [`MarkerNames.jet:20-22`](../../Compiler/JetFoundation/Source/Policy/MarkerNames.jet) as the **block-level handler/slot installer**:
- `#Context(allocator: a, logger: l, on_stop: s, dir: d, env: {CC: "clang"}) { ... }` (implemented for `#7016` and `#ContextKey` constants).
- Under P16 (the open effect-trait ballot), `#Context(Clock: FixedClock{...}) { ... }` will also install effect-trait handlers in scope.

Overloading `#Context` on a type or function declaration to mean *both* "declare an execution context's capability ceiling" and "install runtime handlers for a block" would be ambiguous and error-prone.

**Candidate designs for declaring and entering an execution context in Jet source (no compiler change needed):**

- **Option 1A — Trait/struct declaration matching `Prelude/Effects.jet` (zero new keywords):**
  Just as `Compiler/Prelude/Effects.jet` declares effects as ordinary Jet types implementing `Effect` (with `#FX(...)` on Core/library effects in `Core/Effects/Effects.jet`), a context is an ordinary `struct` implementing `core.effects.ExecutionContext` (or marked `#ExecutionContext`) whose `#FX(...)` / `provides` & `denies` clauses declare its coverage:
  ```jet
  // Declaring a new context in library or user source:
  #FX(!Mem.Alloc, !Mem.Rc, !Mem.Gc, !Time.Wait, !Panic, !Unsafe)
  AudioCallback: struct {
      impl ExecutionContext {}
  }
  ```
  And applying a context to a function, callback parameter, or target uses an explicit marker or type-level bound:
  - Function / entry / module site: `#RunIn(AudioCallback)` (or `#InContext(AudioCallback)` / `#Target(AudioCallback)`)
  - Callback parameter: `fn(buf: ^[Float]) -[in AudioCallback]>` or `callback: fn(buf: ^[Float]) -[AudioCallback]>`
- **Option 1B — Dedicated marker `#ExecutionContext(provides: [...], denies: [...])` on a `struct`:**
  Explicitly separates positive `provides:` from negative `denies:` (and optional resource caps like `stack_above: 4096`), avoiding overloading `#FX(...)` (which currently means "a library effect's underlying machine effects" on `Core/Effects/Effects.jet:10` and "a region cap" on blocks in `Checks.jet:5523`, and explicitly forbids `!` entries on library effects via `E0757` in [`Effects/Library.jet:441`](../../Compiler/JetSema/Source/Sema/Effects/Library.jet)).
- **Option 1C — Comptime `ExecutionContext` value (`const` / `#run`) + `#InContext(AUDIO_CONTEXT)`:**
  A context is a typed Core record value (`ExecutionContext{name: "audio_callback", provides: [...], denies: [...]}`), matching how `TargetMachine` works in [`TargetMachine.jet`](../../Compiler/JetFoundation/Source/Target/TargetMachine.jet). This composes naturally with `#run` board profiles and target definitions (`TargetMachine.context()`), without needing a new marker grammar for context definitions — only one marker (`#InContext(ctx)`) or callback/entry attachment!

**Adversarial critique:** Option 1A collides with `E0757` (library `#FX(...)` forbids `!` entries) and mixes positive capability sets with negative ceilings in one list. Option 1C composes directly with `TargetMachine` profiles, `Output` definitions, and `#run` metaprogramming — *and* works identically whether effects are today's `EffectVocabulary` types or P16's `#Effect` traits. **Needs Ballot (Q1).**

---

### Attack 2: The Deny-Only Trap (`Mem.Alloc`, `Mem.Rc`, `Mem.Gc`, `Mem.Stack`, `Time.Wait`, `Panic`, `Unsafe`)

Look at [`Compiler/Prelude/Effects.jet:16-17`](../../Compiler/Prelude/Effects.jet) and [`Checks.jet:1130-1145`](../../Compiler/JetSema/Source/Sema/Effects/Checks.jet):
- Today, `Panic`, `Mem` (`Mem.Alloc`, `Mem.Rc`, `Mem.Stack`, `Mem.Gc`), `Time.Wait`, and `Unsafe` are **deny-only**: ordinary functions use them without writing them in `-[...]>`, and `sema_effect_uncovered` strips them before checking subset coverage.
- **Fatal flaw of a single-set subset rule:**
  - If the rule is `inferred_positive ⊆ ctx.provides` (stripping deny-only effects as `sema_effect_uncovered` does today), then **no-OS heap-free (`!Mem.Alloc`), realtime audio (`!Mem.Alloc, !Mem.Rc, !Time.Wait, !Panic`), interrupts (`!Mem.Alloc, !Time.Wait, !Mem.Stack(above: N)`), and `#Kernel` (`!Mem.Alloc, !Unsafe`) CANNOT be expressed by the one rule!** That is literally why `Core/Rt/Rt.jet:82` is broken today.
  - Conversely, if `Mem.Alloc` and `Panic` become mandatory positive effects in every function's row, every ordinary Jet function allocating a `String` or indexing a list would require `-[Mem.Alloc, Panic]>`, destroying Jet's ergonomic effect rows.

**Sound formalization of the ONE rule:**
Every execution context `C` is a pair `(provides(C), denies(C))`, where:
- `provides(C)` is the set of **ambient positive effects** (e.g., `FS`, `Net`, `IO`, `Time`, `Rand`, `Env`, `Exec`, `GPU`, `FFI.*`, `Browser`, `DB`, `Log`, `Secret`, `Meta.*`, plus user-declared library/trait effects) whose handlers exist in `C`.
- `denies(C)` is the set of **explicit prohibitions / bounds** on deny-only or bounded effects (e.g., `!Mem.Alloc`, `!Mem.Rc`, `!Mem.Gc`, `!Mem.Stack(above: N)`, `!Time.Wait`, `!Panic`, `!Unsafe`).

For any root function `f` executing in context `C`, the **single coverage predicate** checks every effect `e` in `f`'s solved transitive effect set `reached(f)` (including deny-only effects and memory/stack facts):
1. If `e` is a positive effect: `e` is covered iff `∃ p ∈ provides(C). covers(p, e)`.
2. If `e` is a deny-only / bounded effect: `e` is covered iff `¬∃ d ∈ denies(C). covers(d, e)` (and any numeric ceiling like `Mem.Stack(above: N)` holds).

Wait — what about `#Context(Clock: FixedClock{...}) { ... }` (local handler installation) inside a body running in context `C`?
- Installing a local handler for effect `E` inside a `#Context` block **discharges `E`** from the body's outward requirement (just like Koka's handler elimination), so code inside `#Context(Time: FixedClock{...}) { ... }` can run inside `Comptime` or `Pure` even though `Comptime` does not provide ambient `Time`!
- **Crucial edge case:** A `#Context` block can *add* a positive handler (`provides ∪ {E}`), or *narrow* authority (`#FX(...)`), **never weaken** a hardware/target denial (`!Mem.Alloc` on a heap-free machine cannot be discharged unless `#Context(allocator: region)` supplies an explicit bounded allocator!). **Needs Ballot (Q2).**

---

### Attack 3: Exact Built-in Context Effect Matrices and Hidden Inconsistencies

Applying the unified `(provides, denies)` rule across every built-in Jet context exposes concrete discrepancies that must be resolved explicitly:

| Built-in Context | Proposed `provides` | Proposed `denies` | Live Code Discrepancies / Edge Cases |
|---|---|---|---|
| **Hosted (`run` / `main`)** | All machine + Core effects granted by package `authority.allow` (except `Browser`, `Meta.*` unless build entry) | Package `authority.deny` / `--deny` | Today `E1803` runs in `CLI/Authority.jet` rather than the unified context checker. |
| **Pure (`-[]>`)** | `∅` | None (or optional `!Panic`/`!Mem.*` if written) | Today `E3401` also forbids reading/writing mutable module state (`Checks.jet:1568-1609`), and `E3403` intercepts `Clock.*` (`Checks.jet:833`) unless inside `#AssumeDet`. |
| **Comptime (Tier 1 `#run`, module `const`)** | `Meta.Generate` (in macros/derives), `Meta.Include` (`embed_file`), **pinned fetch?** | `Time`, `Rand`, `Net` (except pinned fetch), `FS`, `Env`, `Exec`, `IO`, `Browser`, `FFI`, `DB`, `Secret`, `Reactive` (`Mem.Alloc` and `Panic` **allowed**) | **Major gap:** `core.net.fetch(url, sha256:)` is currently exempted by hardcoded `method == "fetch"` (`Checks.jet:950`). Needs a distinct effect leaf (e.g., `Meta.Fetch` or `Net.PinnedFetch` under `#7035`) so the rule has zero method-name hacks! Also: `FS.Read` is **not** allowed in Tier-1 `Comptime` (only `embed_file` -> `Meta.Include`). |
| **Comptime `#Impure` (Tier 2)** | Tier 1 `∪ {FS, Env, IO, Exec, Net}` (gated by `Meta.Host`) | `Browser`, `Reactive`, `GPU`, `FFI` (unless permitted) | Today checked dynamically in `JetEval/Source/IR/Native.jet:1255-1259` (`E3410`/`E3411`), **not statically in Sema**! Moving Tier-2 coverage to the static context checker catches unpermitted comptime host calls *before* evaluation starts. |
| **Compiler Hook (`Compiler` / `meta.events`)** | `Meta.*` (per authority) | `Exec` (no child process launches — `Checks.jet:2497-2615`) | Already walks call graph (`E3410`), directly subsumed by `denies: [Exec]`. |
| **Web.JS (Browser)** | `Browser`, `Net`, `Time`, `Rand`, `IO` (console), `Log` | `FS`, `Exec`, `FFI.C`, `DB`, `Secret`, `GPU` (unless WebGPU) | **Live hole:** `TargetMachine.jet:563` denies only `["Exec", "FFI.C"]`; `WebPartition.jet:193` blocks only `["DB", "Secret"]`. `FS` is currently **not** denied in `Web.JS`! |
| **Web.Wasm** | `Time`, `Rand`, `IO`, `Log` (via host imports) | `Browser`, `FS`, `Exec`, `FFI.C`, `DB`, `Secret` | Unifies `E-WEB-TARGET-BROWSER` and `E1123` under one rule. (`E-WEB-CROSS-PARTITION` and `E-WEB-ABI-TYPE` remain structural web-bridge checks.) |
| **Web.Server** | Hosted effects | `Browser` | Unifies `E2824` (`WebPartition.jet:250`) under the one rule. |
| **No-OS (Freestanding)** | Derived directly from `TargetMachine` providers: `wall_clock`/`monotonic_clock` -> `Time`, `sleep` -> `Time.Wait`, `entropy` -> `Rand.Draw`, `byte_sink` -> `IO`, `allocator` -> `Mem.Alloc` | Every omitted provider (`FS`, `Net`, `Exec`, `DB`, `Secret`, `Browser`, `FFI`, `Log`, plus `!Mem.Alloc` if no heap, `!Time.Wait` if no sleep/scheduler) | **Major gap:** Replaces hardcoded 7-module `sema_call_core_no_os_hint` (`Calls/Core.jet:650`) + `E3301` + `E3303` + `E3311` with one rule! **Blocker:** `TargetCapability` (`TargetMachine.jet:7-24`) includes `MMIO`, `Hardware`, `Atomic64`, `Startup`, `PanicReport` which currently have **no** corresponding effect leaves in `Prelude/Effects.jet`. |
| **Hardware Interrupt** | `MMIO` (if modeled), hardware regs | All positive effects + `!Mem.Alloc`, `!Mem.Rc`, `!Mem.Gc`, `!Time.Wait`, `!Panic`, `!Mem.Stack(above: N)` | Currently checked only on profile metadata (`TargetMachine.jet:1711`, `E3314`), not transitively over checked call graphs! |
| **Realtime Audio (`core.rt.callback`)** | `∅` | `!Mem.Alloc`, `!Mem.Rc`, `!Mem.Gc`, `!Time.Wait`, `!Panic`, `!Unsafe` | **Fixes live soundness bug** in `Core/Rt/Rt.jet:82` where `-[]>` currently allows allocation and blocking! |
| **GPU / Parallel Kernel (`#Kernel(.parallel)`)** | `∅` (audited pure compute ops) | All positive effects + `!Mem.Alloc`, `!Mem.Rc`, `!Mem.Gc`, `!Time.Wait`, `!Panic`, `!Unsafe` | Effect portion unifies cleanly; structural proofs (uniform control flow, sendability, bounds, aliasing in `Kernel.jet:63-191`) stay separate (see Attack 5). |
| **C FFI Callback (Foreign Thread)** | `∅` (or `Mem.Alloc`? **must decide!**) | All positive effects + **no task context** (`E3218`) | **Major question:** Does `C.Callback` forbid `Mem.Alloc` and `Panic` (currently call-free scalar subset in `Checking.jet:428`), or allow `Mem.Alloc` once C callbacks install/capture a context (per proposal Section 3.6 line 1553)? Also: task-context absence (`E3218`) contradicts `Effects.jet:17-19` ("reading the task context is no effect") unless either modeled as a context-availability predicate or kept as a separate structural check. |
| **Deep Check (`#On(.Deep)`)** | `∅` | All positive effects (`Panic` allowed for assertions, `Mem.Alloc` allowed) | Directly unifies `E0713` (`Checks.jet:5543`). |
| **Debugger Eval Probe** | `∅` | All positive effects + `!Unsafe` + no module-state writes | Directly unifies `EvalProbe.jet:530-533`. |
| **Test (`#Test`)** | Configurable (default: hosted or sandboxed `Test` context?) | Configurable (`--deny`, `#FX(...)`) | **Question:** Does `Test` default to full hosted effects (status quo) or a hermetic default (requiring explicit `#Context` or authority for `Net`/`FS.Write`)? Changing default breaks existing tests unless opt-in. |

**Needs Ballot (Q3).**

---

### Attack 4: Diagnostic Code Unification (`c3`) vs. 186 Pinned Test Files

Criterion `c3` states: *"with one diagnostic naming the uncovered effect and call path"*.

Our census found **25 existing error codes pinned across 186 test files**:
- High-pin effect-coverage codes: `E3401` (24), `E0740` (18), `E1803` (17), `E3411` (14), `E3410` (13), `E3403` (12), `E3301` (9), `E0643` (9), `E0747` (9), `E0712` (6), `E0921` (6), `E3311` (4), `E3303` (2), `E1123` (2), `E0713` (2), `E-WEB-TARGET-BROWSER` (2), `E2823` (1), `E2824` (1), `E3412` (1), `E0938` (1), `E2914` (1).
- Structural / non-effect codes: `E1102` (29), `E-WEB-CROSS-PARTITION` (4), `E3314` (3), `E3218` (2).

Additionally, [`Compiler/Prelude/Diagnostics.jet`](../../Compiler/Prelude/Diagnostics.jet) treats every error code as a stable public identifier with `safety_class`, `expert_path`, and `jet explain Jet.<Name>`.

**Three concrete options for `c3`:**

- **Option 4A — Single canonical code (`E0762` / `Jet.EffectNotCoveredInContext`) for ALL execution-context effect refusals, retiring legacy context-specific codes:**
  - Every context-coverage failure (`Comptime`, `NoOS`, `Browser`, `Web.Wasm`, `Web.Server`, `Realtime`, `Interrupt`, `Kernel` effects, `DeepCheck`, `EvalProbe`, `CompilerHook`, `AudioCallback`, user contexts) emits **one code** (`E0762`) with one uniform shape:
    - **What:** `` `<root>` uses `<effect>`, which the `<context>` execution context does not cover ``
    - **Why:** `` Call path: `<root> → <mid> → <leaf>` (at `<file>:<line>`). `<context>` provides `<provides>` and forbids `<denies>`. ``
    - **Fix:** Context-aware remediation (move work outside `<context>`, install a handler via `#Context(...)`, add the provider to the target/context, or remove the call).
  - Keeps **non-context** signature/structural codes intact (`E0740` function row mismatch, `E0747` callback parameter row mismatch, `E0712` `#FX` cap, `E1102` kernel structural/sendability proofs, `E3218` foreign-thread missing task context, `E3314` SVD/DMA hardware profile errors, `E-WEB-CROSS-PARTITION` / `E-WEB-ABI-TYPE`).
  - Migrates ~70–90 test files from retired context codes (`E3301`, `E3303`, `E3311`, `E1123`, `E2823`, `E2824`, `E-WEB-TARGET-BROWSER`, `E3410`, `E3411`, `E3412`, `E0713`, `E3403` at comptime) to `E0762` and marks retired codes `status: .Retired` in `Diagnostics.jet`.
- **Option 4B — Single canonical code (`E0762`) for EVERYTHING including `E3401`, `E0740`, `E0747`, `E0712`, `E0921`, `E1803`:**
  - Maximum consolidation (~160+ test files rewritten), but conflates *type-level signature mismatch* (`fn` declares `-[FS.Read]>` but calls `FS.Write`) and *manifest authority refusal* (`--deny Net`) with *execution context coverage*.
- **Option 4C — One shared witness engine + unified message template, preserving per-context error codes:**
  - Zero test churn on code numbers, but directly weakens P17's stated "one diagnostic" promise — users still see 12+ different `Jet.*` error names for the same rule.

**Recommended:** **Option 4A** (one canonical `Jet.EffectNotCoveredInContext` code for all execution-context coverage refusals, preserving signature-type and structural non-effect codes). **Needs Ballot (Q4).**

---

### Attack 5: What Does NOT Reduce to Effect Coverage (Honest Boundary)

An adversarial review must call out where "everything is just an effect row" **breaks down** if pushed past truth:

1. **Structural kernel proofs (`Kernel.jet:63-191`, `E1102`):** Read-only monomorphic parameters, sendability, immutable scalar locals, uniform straight-line control flow, and statically proved array bounds are *type/control-flow invariants*, not effects. Effect coverage proves `#Kernel` is effect-free and allocation-free; `Kernel.jet` must retain its structural proof pass.
2. **Foreign-thread task-context absence (`Checking.jet:425-491`, `E3218`):** [`Compiler/Prelude/Effects.jet:17-19`](../../Compiler/Prelude/Effects.jet) explicitly states: *"Reading the task context (`context.deadline`) is no effect: it is an implicit input of every function, as its arguments are."* Either (a) reading the ambient task context stays a separate structural check (`E3218`) for foreign-thread contexts, OR (b) task-context presence is a boolean property on `ExecutionContext` (`has_task_context: Bool`) checked by the same context engine (reusing `E3218` or the unified diagnostic), without polluting ordinary function effect rows with a `TaskContext` effect.
3. **Web JS/Wasm bridge ABI (`WebPartition.jet:156-185`):** `E-WEB-ABI-TYPE` (parameter/return `#Codable` ABI safety) and `E-WEB-CROSS-PARTITION` (direct call across JS/Wasm boundary without `#Export(wasm)`) are ABI/partition-boundary rules, not effect-coverage rules.
4. **Hardware layout/DMA/register-width facts (`TargetMachine.jet:1692-1721`, `E3314`):** SVD provenance, register bit-width, MMIO address bounds, and DMA buffer ownership are hardware-contract invariants, not effects.
5. **Comptime non-termination / runtime-only values (`Pipeline.jet:2861`, `E0643`):** A pure function can still fail at comptime by panicking, dividing by zero, or hitting step limits; `E0643` remains needed for evaluation-time failures even after effect coverage moves to static Sema checking.
6. **Mutable module globals in `-[]>` (`Checks.jet:1568-1609`, `E3401`):** Until P10 retires mutable module globals in favor of context slots, reading/writing mutable module globals in pure/comptime contexts must remain checked alongside the effect subset rule.

**Needs Ballot (Q5)** to confirm this exact boundary between effect-coverage refusals and retained structural checks.

---

### Attack 6: Interaction with the Open Effect-Trait Ballot and `#7035` (`EffectAccess`)

1. **Open Effect-Trait Ballot (P16 — `#Effect` traits & `#Context(Effect: Handler)`):**
   - Today, effects in [`Compiler/Prelude/Effects.jet`](../../Compiler/Prelude/Effects.jet), [`Core/Effects/Effects.jet`](../../Core/Effects/Effects.jet), and [`Core/Compiler/Meta.jet`](../../Core/Compiler/Meta.jet) are nominal `struct`/`enum` types implementing `Effect`, indexed by `EffectVocabulary`.
   - Under P16, effects become `#Effect` traits whose operations dispatch to the handler installed in `#Context`.
   - **Coupling risk:** If `#7015` hardcodes string-based `EffectVocabulary` rows into user-facing context declarations, P16 will require a breaking syntax rewrite.
   - **De-coupling design:** Represent a context's `provides` / `denies` entries as **checked effect references** (the exact same `EffectRowEntry` grammar already accepted in `-[...]>` and `#FX(...)`). When P16 lands `#Effect` traits and `#Context(EffectTrait: Handler)`, the exact same context syntax continues working unchanged, and `#Context(EffectTrait: Handler)` simply discharges `EffectTrait` from the block's required row before checking against the enclosing execution context!
2. **`#7035` (`EffectAccess` — read/write/scope granularity):**
   - `#7035` refines effect access (e.g., `FS.Read` vs `FS.Write`, `DB.Read` vs `DB.Write`, and potentially `Net.PinnedFetch` vs `Net`).
   - `#7015`'s context coverage predicate must use the **canonical effect subsumption function** (`sema_effect_covers`, [`Checks.jet:1138-1155`](../../Compiler/JetSema/Source/Sema/Effects/Checks.jet)) — never string equality — so any leaf/access refinement added by `#7035` automatically composes with execution contexts (e.g., a context providing `FS.Read` automatically covers `FS.Read` and refuses `FS.Write`, and a `Comptime` context providing `Net.PinnedFetch` eliminates the hardcoded `method == "fetch"` check at `Checks.jet:950`).
3. **Compile-time performance win:**
   - Today, `JetSema` performs **7 separate call-graph traversals** for context checks (`PureRoot.jet`, `Checks.jet` pure/region/callback/heap-free/memory-denial/compiler-hooks, `TargetDenials.jet` denied-effects + no-OS reach, `WebPartition.jet` placement).
   - Replacing them with **one shared solver** (`Sema/Effects/ExecutionContexts.jet`) over the already-solved `SemaEffectNode` graph — with one lazy shortest-path BFS computed *only* when a violation is detected — reduces compilation overhead to **O(|roots|)** in the clean (zero-error) case!

---

## 4. Open Ballots Required Before Implementation (Q1–Q5)

### Q1 — How a New Execution Context Is Declared and Applied in Jet Source (`c4`)
- **Option A (Recommended):** A Core record type `ExecutionContext{name, provides, denies, max_stack_bytes?, has_task_context?}` evaluated at `#run` (composable with `TargetMachine` profiles and `Output` declarations), applied to functions/modules/entries via `#InContext(CTX)` and to callback parameters via `fn(…) -[in CTX]>`. No collision with `#Context(dir:, env:)` (`#7016`) or `#FX(...)` (`E0757`).
- **Option B:** A type declaration `MyCtx: struct { impl ExecutionContext { ... } }` with a new `#ExecutionContext(provides: [...], denies: [...])` marker.
- **Option C:** Extend `#FX(...)` to allow `!` denials on `struct` declarations implementing `ExecutionContext` (requires relaxing `E0757` and differentiating library effects from execution contexts).

### Q2 — Deny-Only Effects (`Mem.Alloc`, `Mem.Rc`, `Mem.Gc`, `Mem.Stack`, `Time.Wait`, `Panic`, `Unsafe`) in the Subset Rule
- **Option A (Recommended):** Two-part coverage predicate on every context: `(provides, denies)`. Positive effects must be covered by `provides`; deny-only and bounded effects (`Mem.*`, `Time.Wait`, `Panic`, `Unsafe`) are allowed unless matched by `denies` (or exceeding `Mem.Stack(above: N)`). Fixes `Core/Rt/Rt.jet:82` immediately by declaring `Realtime` with `denies: [Mem.Alloc, Mem.Rc, Mem.Gc, Time.Wait, Panic, Unsafe]`. Local `#Context` blocks can discharge positive effects (or `Mem.Alloc` when installing an explicit bounded allocator), never hardware denials.
- **Option B:** Promote `Mem.Alloc`, `Time.Wait`, `Panic`, `Unsafe` to positive effects required in every function's row (breaking change for all existing Jet code).

### Q3 — Built-in Context Provider/Denial Matrices (Key Disputed Cells)
Ratify the built-in table in Section 3 (Attack 3), specifically:
1. **`Comptime` (Tier 1):** Allows `Mem.Alloc`, `Panic`, `Meta.Generate`, `Meta.Include`, and content-addressed `Net.PinnedFetch` (retiring `Checks.jet:950`'s hardcoded `method == "fetch"`); denies `FS`, `Net` (other than pinned fetch), `Time`, `Rand`, `Env`, `Exec`, `IO`, `Browser`, `FFI`, `DB`, `Secret`, `Reactive`.
2. **`ComptimeImpure` (Tier 2, `#Impure` + `Meta.Host`):** Adds `FS`, `Net`, `Env`, `IO`, `Exec` checked **statically** in Sema (not only dynamically in `JetEval`).
3. **`Web.JS`:** Explicitly denies `FS` in addition to `Exec`, `FFI.C`, `DB`, `Secret` (closing the live `FS`-in-browser hole).
4. **`C.Callback`:** Keep `Mem.Alloc` / `Panic` denied (current call-free scalar subset) until captured/installed foreign-thread contexts land, OR allow `Mem.Alloc` behind explicit `#Context(...)` inside the callback.
5. **`NoOS` hardware effects:** Add `MMIO` (or `Hardware.MMIO`) effect leaf to `Compiler/Prelude/Effects.jet` so `TargetMachine` profiles map 1-to-1 to the effect lattice without `TargetCapability` side-tables, OR keep hardware register/DMA checks in `TargetMachine` (`E3314`).
6. **`Test` default:** Keep `#Test` defaulting to hosted authority (preserving existing tests) while providing an opt-in `Test.Hermetic` context.

### Q4 — Diagnostic Code Unification Strategy (`c3`)
- **Option A (Recommended):** Introduce **one** canonical error code (`E0762` / `Jet.EffectNotCoveredInContext`) with mandatory shortest call path (`a → b → c`) for all execution-context coverage refusals (`Comptime`, `ComptimeImpure`, `CompilerHook`, `NoOS`, `Browser`/`Web.Wasm`/`Web.Server`, `Interrupt`, `Realtime`, `Kernel` effects, `DeepCheck`, `EvalProbe`, user-declared contexts). Retire superseded context-specific codes (`E3301`, `E3303`, `E3311`, `E1123`, `E2823`, `E2824`, `E-WEB-TARGET-BROWSER`, `E3410`, `E3411`, `E3412`, `E0713`) in `Diagnostics.jet` (`status: .Retired`) and migrate their ~70–90 test files. Retain type-signature/policy/structural codes (`E0740`, `E0747`, `E0712`, `E0921`, `E1803`, `E1102`, `E3218`, `E3314`, `E-WEB-CROSS-PARTITION`, `E-WEB-ABI-TYPE`, `E0643`).
- **Option B:** Collapse `E3401`, `E0740`, `E0747`, `E0712`, `E0921`, `E1803` into `E0762` as well (~160+ test files).
- **Option C:** Share one diagnostic builder and call-path formatter while preserving per-context legacy codes.

### Q5 — Structural Non-Effect Invariants Boundary
- **Option A (Recommended):** Keep structural invariants outside the effect subset rule (`Kernel.jet` bounds/control-flow/sendability `E1102`, `WebPartition.jet` ABI/cross-partition `E-WEB-ABI-TYPE`/`E-WEB-CROSS-PARTITION`, `TargetMachine.jet` SVD/DMA/register-width `E3314`, `Pipeline.jet` evaluation-time `E0643`), and represent foreign-thread task-context absence as `has_task_context: false` on `C.Callback` (reporting `E3218` or `E0762` per Q4).
- **Option B:** Introduce synthetic effects (`TaskContext`, `DynamicControlFlow`, etc.) to pull structural invariants into effect rows (rejected by this review: would clutter ordinary function rows and contradict `Prelude/Effects.jet:17-19`).

---

## 5. Gap-to-Card Ledger and Binding Cutover List

### Gaps Identified (Tracked Against Cards / Ballots)

| Gap | Live Evidence | Disposition |
|---|---|---|
| Realtime callbacks allow `Mem.Alloc` / `Mem.Rc` / `Time.Wait` / `Panic` because `-[]>` strips deny-only effects | [`Core/Rt/Rt.jet:82`](../../Core/Rt/Rt.jet), [`Checks.jet:1130-1145`](../../Compiler/JetSema/Source/Sema/Effects/Checks.jet) | Fixed by `#7015` (under Q2-A + Q3) |
| Browser target does not deny `FS` (`TargetMachine.jet:563` denies only `Exec`, `FFI.C`; `WebPartition.jet:193` checks only `DB`, `Secret`) | [`TargetMachine.jet:563`](../../Compiler/JetFoundation/Source/Target/TargetMachine.jet), [`WebPartition.jet:193`](../../Compiler/JetSema/Source/Sema/WebPartition.jet) | Fixed by `#7015` (under Q3.3) |
| `core.net.fetch` comptime exemption hardcodes `method == "fetch"` instead of using effect lattice | [`Checks.jet:944-962`](../../Compiler/JetSema/Source/Sema/Effects/Checks.jet) | Coordinate `#7015` with `#7035` (`Net.PinnedFetch` or `Meta.Fetch`) |
| No-OS OS-module check hardcodes 7 Core module paths instead of using effect rows | [`Calls/Core.jet:650-671`](../../Compiler/JetSema/Source/Sema/Calls/Core.jet), [`TargetDenials.jet:127-222`](../../Compiler/JetSema/Source/Sema/TargetDenials.jet) | Fixed by `#7015` (delete hardcoded list once `NoOS` context derives from `TargetMachine`) |
| Hardware interrupt effect checks (`InterruptEffectForbidden`) are profile-only, not checked transitively over call graphs | [`TargetMachine.jet:1711`](../../Compiler/JetFoundation/Source/Target/TargetMachine.jet) | Fixed by `#7015` (wire interrupt handlers as `Interrupt` context roots) |
| Tier-2 comptime (`#Impure` / `Meta.Host`) is refused dynamically in `JetEval`, not statically in Sema | [`Native.jet:1255-1259`](../../Compiler/JetEval/Source/IR/Native.jet) | Fixed by `#7015` (check `#run` / `#Impure` roots statically in Sema) |
| Missing call paths on `E1123`, `E3301`, `E2823`, `E2824`, `E3412`, `E0713` | [`TargetDenials.jet:275`](../../Compiler/JetSema/Source/Sema/TargetDenials.jet), [`WebPartition.jet:250-259`](../../Compiler/JetSema/Source/Sema/WebPartition.jet), [`Checks.jet:960, 5543`](../../Compiler/JetSema/Source/Sema/Effects/Checks.jet) | Fixed by `#7015` (`c3` unified shortest-path witness builder) |
| No `UI` / `UI.MainThread` effect exists yet | [`Compiler/Prelude/Effects.jet`](../../Compiler/Prelude/Effects.jet), [`Core/Effects/Effects.jet`](../../Core/Effects/Effects.jet) | Follow-on card (out of `#7015` core engine; becomes trivial once `c4` lands) |
| `#Context` block syntax already taken by `#7016` (`dir:`, `env:`) and `#ContextKey` | [`ContextBlocks.jet:21-68`](../../Compiler/JetSema/Source/Sema/Statements/ContextBlocks.jet) | Resolved by **Q1** ballot before implementation |

### Binding Cutover List (Phase 2 — Blocked Until Q1–Q5 Ratified)

1. **Core & Prelude vocabulary:**
   - Add `ExecutionContext` definition in `Core/Effects/` (exact shape per Q1/Q2) and any ratified effect leaves (e.g., `Net.PinnedFetch` / `Meta.Fetch`, `MMIO` per Q3) in `Compiler/Prelude/Effects.jet` / `Core/Compiler/Meta.jet`.
   - Register the canonical unified diagnostic (`E0762` / `Jet.EffectNotCoveredInContext`) and retire superseded codes (per Q4) in `Compiler/Prelude/Diagnostics.jet` + `Compiler/JetFoundation/Source/Registry/DiagnosticRows.jet`.
   - Update `Core/Rt/Rt.jet:82` to bind `core.rt.callback` to the `Realtime` execution context.
2. **Unified Sema context engine (`Compiler/JetSema/Source/Sema/Effects/ExecutionContexts.jet`):**
   - Implement one `ExecutionContextSpec` (`name`, `provides`, `denies`, `max_stack_bytes`, `has_task_context`) and one `sema_check_execution_context(roots, spec, effect_graph, memory_facts)` checker with shortest-path BFS witness construction (`root → ... → leaf`) and `#Context` handler discharge support.
   - Register built-in context specs (`Comptime`, `ComptimeImpure`, `CompilerHook`, `Web.JS`, `Web.Wasm`, `Web.Server`, `NoOS(machine)`, `Interrupt(machine)`, `Realtime`, `Kernel`, `C.Callback`, `DeepCheck`, `EvalProbe`) plus source-declared `ExecutionContext` declarations (`c4`).
3. **Delete / delegate legacy per-context checkers (zero duplicate walks):**
   - Replace `TargetDenials.jet` (`sema_target_denial_check`, `sema_target_no_os_reach_check`, `sema_target_capability_check`) and `Calls/Core.jet:650-671` (`sema_call_core_no_os_hint` / `sema_call_core_no_os_refused`) with `NoOS(machine)` and `Browser` context checks.
   - Delegate `WebPartition.jet` (`E-WEB-TARGET-BROWSER`, `E2823`, `E2824`), `Checks.jet` (`sema_effect_comptime_nondeterministic`, `sema_effect_comptime_net_unavailable`, `sema_effect_check_heap_free`, `sema_effect_check_compiler_hooks`, `E0713`), `Kernel.jet` effect check, and `EvalProbe.jet` effect check to `ExecutionContexts.jet`.
4. **Tests & Proofs (`c1`–`c4`):**
   - Migrate affected UI/case snapshots per Q4 ratification.
   - Add dedicated `Tests/UI/` and `Tests/Cases/` proofs verifying:
     - Every built-in context enforces the one subset rule and prints the uncovered effect + exact call path (`a → b → c`).
     - `core.rt.callback` rejects transitive heap allocation (`Mem.Alloc`) and blocking (`Time.Wait`) with call path.
     - `Web.JS` rejects `FS`, `DB`, `Secret`, `Exec`, `FFI.C` with call path.
     - A brand-new user-declared execution context in Jet source (`c4`, e.g., `AudioDSP` or `SandboxedPlugin`) is enforced with zero compiler changes and emits the exact same diagnostic with call path.
