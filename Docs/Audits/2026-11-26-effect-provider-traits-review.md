# Phase 1 Adversarial Design Review — `#Effect` Provider Traits (`#7030`)

- **Date**: 2026-11-26
- **Base commit audited**: `b84063a8e0f181f70c46325f5dcc55d15a6a0640` (`dev`)
- **Scope**: Unified `#Effect` trait declarations, `#Context` provider installation and effect discharge, sub-effects, generic `#Effect` traits (including `#7003` `Emit<T>`), `#7035` `EffectAccess` / `irreversible` metadata, target/authority consolidation, specialization, and cross-tier I9 parity.
- **Verdict**: **PASS WITH DESIGN CONSTRAINTS — READY TO BUILD WAVE 1**

---

## Executive Summary

Jet currently splits every effectful service across **four disconnected declarations**:

1. An empty effect-name tag (`struct` or `enum` with `impl Effect`) in `Compiler/Prelude/Effects.jet`, `Core/Effects/Effects.jet`, or `Core/Compiler/Meta.jet`.
2. A separate, inconsistently shaped provider abstraction (`trait` for `Allocator`, `Driver`, `Scheduler`, `Accelerator`; concrete `Shared`-backed `struct` for `Clock`; `#ContextKey` struct `Handler` for `Log`; ambient globals or TLS callbacks for `Time`, `Rand`, `FS`, `Net`, `Env`).
3. A hardcoded `#Context` menu in `Compiler/Prelude/Markers.jet` and `Compiler/JetSema/Source/Sema/ContextKeys.jet` whose block checker (`Compiler/JetSema/Source/Sema/Effects/Checks.jet:9723-9728`) **never discharges any effect** from the enclosed block.
4. Multiple hand-synced tables duplicating the effect catalog across target profiles (`Core/Target/Target.jet`, `Compiler/Prelude/Facts.jet`, `Compiler/JetFoundation/Source/Target/TargetMachine.jet`), authority prose (`Compiler/JetFoundation/Source/Policy/Authority.jet`), build-time provider injection (`Compiler/JetDriver/Source/Driver/SchedulerProvider.jet`), and guards (`Tests/Guards/effect_names.jet`).

Unifying each service into **one ordinary `trait` marked `#Effect`** — with sub-effects as sub-traits (`#Effect(parent: Root)`), `#Context` installing any implementation and discharging the effect statically, and O1/O2 specializing known concrete handlers to direct inlined calls — is architecturally sound and cleanly removes every duplicated mechanism above.

However, adversarial inspection against live `dev` (`b84063a8e0f`) surfaced **eight concrete hazards** that would break soundness, I9 parity, `#7035` safety classification, or `#7003` (`Emit<T>`) compatibility if left unspecified before Wave 1:

1. **Non-swappable structural/deny-only effects vs swappable service `#Effect` traits** (`Unsafe`, `Panic`, `Mem.Rc`/`Stack`/`Gc`, `FFI.*`, `Meta.*` must never be dischargeable via `#Context`).
2. **Effect discharge soundness, `E0742` trait ceilings, and type-erased handler spoofing** (installing a dynamic trait object or unbounded generic handler must not drop the effect below its ceiling).
3. **Generic `#Effect` traits (`#7003` `Emit<T>`) and Jet's actual trait-parameter grammar** (`Emit: trait<T>`, per-monomorphization context slot identity, non-aliasing between `Emit<Int>` and `Emit<String>`, and fiber-backed `-> Stream<T>` parity).
4. **Static specialization vs dynamic lookup I9 parity across Native O0/O1/O2, Comptime (`JetEval`), and Web (`JetWeb`)** (identical evaluation order, drop order, `^self` mutation visibility, panic/early-return unwind, and indirect-call/task-spawn escape barriers).
5. **Ownership, `^self` mutability, and task/closure capture soundness** (reconciling Jet's stack-borrow `allocator` pinning with ref-counted `values@32` task inheritance so concurrent tasks can never race on a `^self` handler).
6. **Ambient host defaults, target profiles, `authority:`, `--allow`/`--deny`, and runtime shadow stores** (`World.jet`, `HostTime.jet`/`ProveReplay.jet`, `FaultSchedule.jet`, and replacing `SchedulerProvider.jet` with table-driven target binding).
7. **Name reconciliation and sub-effect declaration syntax** (`Time` vs `Clock`, dotted trait names not parsed by `parser_parse_trait`, and preserving existing `time.now()`/`files.read()` call sites via thin Core delegates).
8. **Paired benchmark gate** verifying zero-cost specialization (`<= 1.05x` vs direct concrete call).

Each hazard has a binding design constraint and exact wave owner below. No blocking owner decision is required to start Wave 1.

---

## 1. Strongest Existing Prior Art & Comparison Matrix

The design draws selectively from five proven production and research systems, explicitly rejecting their failure modes:

| System | Core Mechanism | What Jet Adopts | What Jet Rejects (and Why) |
| --- | --- | --- | --- |
| **Koka** (Leijen, POPL 2017; Xie & Leijen, ICFP 2021 *Generalized Evidence Passing for Effect Handlers*) | Row-polymorphic algebraic effects compiled via evidence passing; known handlers specialized to direct calls; tail-resumptive handlers compile without delimited continuations. | Evidence-passing mental model; specializing known handlers to direct calls; treating ordinary service calls as tail-resumptive (no continuation capture). | General multi-shot delimited continuations on every effect (unnecessary for I/O/clock/FS services and incompatible with linear ownership/RAII drops). Non-tail suspension is isolated strictly to `-> Stream<T>` (`Emit<T>` over `Generator.jet`). |
| **OCaml 5** (Sivaramakrishnan et al., PLDI 2021 *Retrofitting Effect Handlers onto OCaml*) | Dynamic effect handlers searched up the fiber stack; effect operations decoupled from static effect typing. | Scoped dynamic handler stack on the current task/fiber as the O0/dynamic fallback (matching Jet's existing `Context` record in `JetRuntime/Source/Context.jet`). | Untyped/unchecked effect rows (Jet enforces static effect rows, `E0742` trait ceilings, `E0749` denials, and `#7035` `EffectAccess` classification at compile time). |
| **Scala 3** (Odersky et al., OOPSLA 2022 *Scoped Capabilities for Polymorphic Effects*; Context Functions) | Context functions (`CanThrow ?=> T`) pass capabilities implicitly; capture checking tracks whether a capability escapes its introducing scope via closures or spawned fibers. | Lexical capability introduction (`#Context(E: h) { ... }`) + static non-escape rules preventing scoped mutable (`^self`) handlers from leaking into detached tasks or escaping closures. | Heavy user-facing capture-set annotations on every closure type (Jet splits cleanly between `self` shareable handlers moved into ref-counted `Context` nodes vs `^self` single-task scoped borrows). |
| **Zig** (`std.mem.Allocator`, `std.Io` explicit interface structs) | Explicit capability passing via vtable structs (`Allocator`, `Io`) so any subsystem can swap implementations with zero ambient globals and zero runtime cost when comptime-specialized. | Uniform provider-trait contract for every machine/Core service (`Allocator`, `FS`, `Time`, `Net`, `Rand`, `IO`, `Scheduler`, `Accelerator`); comptime/O1 specialization to direct calls. | Threading 6–10 explicit capability parameters (`alloc`, `io`, `clock`, `rng`, `logger`, `fs`) through every function signature across an entire codebase. |
| **Haskell** (`mtl` typeclasses, `effectful` reader-over-`IO`) & **Rust** (traits + monomorphization + `dyn Trait`) | One trait/typeclass describes a service's operations; monomorphization eliminates dispatch overhead for known concrete types; `dyn Trait` provides type-erased fallback. | **One trait per service** (`#Effect Time: trait { ... }`); trait laws (`#Test` inside trait bodies, already supported by `TraitDef.laws`) verify every handler; monomorphization specializes concrete `#Context` handlers. | `mtl`'s $O(n^2)$ transformer lift boilerplate and Rust's lack of ambient scoped capability discharge in function signatures. |

**Key takeaway from prior art**: Jet sits in a uniquely strong sweet spot — **tail-resumptive `#Effect` traits** give Koka's evidence-passing specialization and Zig's swappable zero-overhead providers with Rust's familiar trait model, while reserving fiber suspension strictly for `-> Stream<T>` (`Emit<T>`).

---

## 2. Complete Live-Code Inventory (`dev` `b84063a8e0f`)

Every location in live `dev` that currently declares, parses, scans, checks, lowers, or duplicates effect/provider knowledge:

### 2.1 Effect Declarations (Struct/Enum Tags + `impl Effect`)

| File | Lines | What Lives Here Today |
| --- | --- | --- |
| `Compiler/Prelude/Effects.jet` | 1–155 | 13 machine roots: `Net` (struct, irrev, `.Replaces`), `FS` (enum `Read`/`Write`, irrev on `Write`, `.Reads`/`.Replaces`), `IO` (struct, `.Reads`), `Time` (enum `Wait`, `.Reads`), `Rand` (enum `Draw`, `.Reads`), `Env` (struct, `.Replaces`), `Exec` (enum `Exit`/`Args`, irrev on `Exit`, `.Replaces`/`.Reads`), `GPU` (struct, `.Reads`), `FFI` (enum of 24 foreign languages, irrev, `.Replaces`), `Browser` (struct, `.Replaces`), `Panic` (struct, `.Reads`), `Mem` (enum `Alloc`/`Rc`/`Stack`/`Gc`, `.Reads`), `Unsafe` (struct, `.Reads`). Note: `#7035` Wave 1 added `access: fn(self) -> EffectAccess`; `FS.Write`/`DB.Write` have not yet split into `Add`/`Replace`. |
| `Core/Effects/Effects.jet` | 1–33 | 3 Core service effects with `#FX(...)` machine ceilings: `DB` (`#FX(Net, FS)`, enum `Read`/`Write`), `Log` (`#FX(IO, FS, Net)`, struct), `Secret` (`#FX(Env, FS, Net)`, struct). |
| `Core/Compiler/Meta.jet` | 1–51 | Metaprogramming effects with `#FX`, `expert`, `access`, and `parent: Meta.Code`: `Meta` (enum `Generate`/`Include`/`Code`/`Host`) and `MetaCode` (enum `Raw`/`Types`/`Rewrite`, `parent: Meta.Code`). |

### 2.2 Build-Time Vocabulary Scanner & Closed Root Registry

| File | Lines | What Lives Here Today |
| --- | --- | --- |
| `Compiler/JetFoundation/Source/Registry/EffectVocabulary.jet` | 1–602 | Line-oriented source scanner (`effect_vocabulary_source_scan`, lines 301–494) that **hard-requires** `Name: struct {` or `Name: enum {` plus `impl Effect { ... }` (lines 352–354, 428–437). Generates `EFFECT_VOCABULARY_ROWS`, `EFFECT_VOCABULARY_ROOTS`, `EFFECT_VOCABULARY_LEAVES`, `EFFECT_VOCABULARY_MACHINE_ROOTS`, `EFFECT_CANONICAL_MODULE`, and parses `#7035` `EffectAccess` (`effect_vocabulary_access_fact`, lines 549–601). |
| `Compiler/JetFoundation/Source/Registry/EffectRoots.jet` | 1–91 | Closed `Effect: enum` (lines 10–24) guarded at build time by `EFFECT_ROOT_COUNT` (line 8) and `EFFECT_ROOTS_REMOVED` (line 45, currently `["Context"]`, used by `jet fix`). |

### 2.3 Parser & AST

| File | Lines | What Lives Here Today |
| --- | --- | --- |
| `Compiler/JetFoundation/Source/AST/Items.jet` | 276–307, 588–615 | `EffectAccess`, `EffectDecl`, `EffectImpl` (`irreversible`, `parent`, `decls`, `expert`, `access`) at lines 276–307. `TraitDef` at lines 588–615 **already has** `supertraits: [String]`, `markers: [Marker]`, `cap: ModuleCap?` (`#FX(...)`), `laws: [TestDef]`, and `type_params: [TypeParam]` (`Store: trait<T>`). |
| `Compiler/JetParser/Source/Parser/Declarations/Effects.jet` | 261–490 | `parser_finish_effect_type` and `parser_effect_impl_from` run only on `struct`/`enum` with `impl Effect`. |
| `Compiler/JetParser/Source/Parser/Items.jet` | 318–337, 2918–2985 | `TOKEN_KW_STRUCT` (line 328) and `TOKEN_KW_ENUM` (line 332) emit `parser_type_companions(..., value.effect_impl)`. `TOKEN_KW_TRAIT` (lines 334–336) emits **no** `EffectDecl` companions. `parser_parse_trait` (line 2920) parses a single identifier (`parser_expect_name`), then `type_params` (`Name: trait<T>`), then `: [SuperA, SuperB]`. |
| `Compiler/Prelude/Markers.jet` | 635–653 | `Context: struct` hardcodes only two fields (`allocator`, `logger`); `#Effect` is **not yet registered** as a Prelude marker. |

### 2.4 Sema Effect Checking, Library Effects, `#Context`, and Ownership

| File | Lines | What Lives Here Today |
| --- | --- | --- |
| `Compiler/JetSema/Source/Sema/Effects/Library.jet` | 1–729 | `sema_library_effect_declared` (lines 109–128) only accepts `.Struct`/`.Enum` with `.EffectDecl`; already supports `parent:` (`SemaLibraryEffect.ancestors`) and value-scoped struct effects (`Bucket{name: "photos"}`). |
| `Compiler/JetSema/Source/Sema/Effects/Checks.jet` | 1576–1621, 2260, 2739–2945, 5601–5606, 6093–6165, 7899–7911, **9723–9728** | `ContextBlock` walk at **9723–9728** walks `guards` and `body` with **zero effect discharge or substitution** — every effect in `body` leaks unchanged to the caller. `E0742` trait-method effect ceiling check lives at 6133–6165; `E0749` denial check at 1576–1621 & 2739–2945; trait dispatch reach at 6093–6131 & 7899–7911 (matching `Docs/Spec/spec.md:3725–3730`: declared row -> `#FX(...)` cap -> union of implementers). |
| `Compiler/JetSema/Source/Sema/Effects/Authority.jet` | 281–285, 504, 583, 627 | Authority handle escape walks `ContextBlock`. |
| `Compiler/JetSema/Source/Sema/ContextKeys.jet` | 38–83 | Hardcoded `SEMA_CONTEXT_RESERVED_NAMES` (12 names), `sema_context_field_settable` (`allocator \| deadline \| frame`), `SEMA_CONTEXT_CORE_FIELDS` (8 names), and `sema_context_field_module`. |
| `Compiler/JetSema/Source/Sema/Statements/ContextBlocks.jet` | 57–252 | `sema_stmt_check_context_block` accepts only the 3 hardcoded settable fields + `#ContextKey` constants (rejects `#Context(Time: ...)` with `E0761` at line 68). `allocator` uses bespoke `sema_stmt_context_allocator_handler` (lines 233–251). |
| `Compiler/JetSema/Source/Sema/Ownership/Checks.jet` | 395–415, 669–670 | `SemaOwnershipBlockAllocator` pins a local `allocator` binding for the duration of a `#Context(allocator: a)` block (emitting `E0204` on move/escape). |

### 2.5 Fragmented Provider Traits & Concrete Fakes Today

| File | Lines | Current Shape | Gap vs Unified `#Effect` Trait |
| --- | --- | --- | --- |
| `Compiler/Prelude/Traits.jet` | 269–276 | `Driver: trait` (`query`, `query_one`, `execute`, `begin`; all take `self`) | Disconnected from `DB` effect in `Core/Effects/Effects.jet`; cannot be installed in `#Context`. |
| `Core/Mem/Mem.jet` | 356–444 | `Allocator: trait` (`allocate`, `release`, `resize`; all take `^self`), `__allocator_handler` (raw `mem.address_of` under `#Policy(sentries: .Off)`), `__ContextAllocator` | Disconnected from `Mem.Alloc`; uses bespoke compiler plumbing instead of generic `#Context` provider machinery. |
| `Core/Tasks/Tasks.jet` | 111–114 | `Scheduler: trait` (`push`/`pop` take `^self`) | Bound only at build time via AST text injection in `Compiler/JetDriver/Source/Driver/SchedulerProvider.jet:1–145`. |
| `Core/Compute/Compute.jet` | 472–481 | `Accelerator: trait` (`launch` takes `self`, charges `-[GPU]>`) | Disconnected from `GPU` effect; cannot be swapped in `#Context`. |
| `Core/Time/Time.jet` & `Core/Testing/Testing.jet` | `Time.jet:76–178`, `Testing.jet:282–290` | `Clock` is a **struct** wrapping `Shared<ClockState>` (not a trait); `fake_clock` and `fake_rng` return structs | Cannot be installed in `#Context`; `time.now()` ignores `fake_clock` unless passed manually or hooked via `World.jet`. |
| `Core/Log/Log.jet` | 601–639 | `Handler` struct + `#ContextKey logger` (line 623) | Disconnected from `Log` effect; `log.info`/`log.warn` do not even dispatch through `context.logger`. |

### 2.6 IR Lowering, Runtime Stores, Side Channels, and Cross-Tier Execution (I9)

| File | Lines | What Lives Here Today |
| --- | --- | --- |
| `Compiler/JetIR/Source/BuildTransact.jet` | 301–585, 704–712 | `ir_context_block`, `ir_context_push_value`, and unwind restoration in `ir_tx_unwind`/`ir_stop_return`. |
| `Compiler/JetIR/Source/BuildTraits.jet` | 228–275 | Trait vtable construction and dynamic dispatch lowering. |
| `Compiler/JetRuntime/Source/Context.jet` | 1–125 | 48-byte `Context` record (`deadline_present@0`, `deadline_ms@8`, `effect_scope@16`, `allocator@24`, `values@32`, `scratch@40`). `values@32` is a ref-counted linked list (`owners@0`, `release@16`, `key@24`, `next@32`, `value@40`) retained across spawned tasks by `context_inherit`. |
| `Compiler/JetRuntime/Source/World.jet` + `Core/Testing/Testing.jet` | `World.jet:1–110`, `Testing.jet:1685–1704` | Second runtime store (`World144` on `TCB@104`) + TLS `rt_time_monotonic_provider_set` callback. |
| `Compiler/JetRuntime/Source/HostTime.jet` + `Compiler/JetCLI/Source/CLI/ProveReplay.jet` | `HostTime.jet:172–210`, `ProveReplay.jet:152–175` | `ENV_JET_PROVE_REPLAY_TIME_MS` / `ProveJournal` replay hook bypassing `Context`. |
| `Compiler/JetRuntime/Source/FaultSchedule.jet` + `Core/Files/Sealed.jet` | `FaultSchedule.jet:1–60`, `Sealed.jet:281–325` | TLS `fault_allocation_key` / `rt.fault_should_fail` side channel. |
| `Compiler/JetEval/Source/IR/Native.jet` & `Compiler/JetWeb/Source/Web/Build.jet` | `Native.jet:1–22`, `Build.jet:1–45` | **Crucial I9 finding**: Native O0/O1/O2, Comptime (`JetEval`, native x86-64 O0 in seccomp child), and Web (`JetWeb`, JetIR -> Wasm) **all execute `JetIR` linked against `JetRuntime/Source/Context.jet`**. |

### 2.7 Duplicated Hand-Synced Tables to Consolidate / Delete

| File | Lines | Duplicated Knowledge |
| --- | --- | --- |
| `Core/Target/Target.jet` | 26–84 | `MachineFacts` 15 `String?` provider fields + hand-coded `row()` (lines 62–83) mapping absent fields to `"!Net"`, `"!FS"`, ..., `"!Mem.Alloc"`. |
| `Compiler/Prelude/Facts.jet` | 358–505 | 16 hand-written `Target*` `impl Fact` structs duplicating `TargetCapability`. |
| `Compiler/JetFoundation/Source/Target/TargetMachine.jet` | 7–43 | `TargetCapability: enum` + `TARGET_CAPABILITY_ALL` (16 entries). |
| `Compiler/JetFoundation/Source/Policy/Authority.jet` | 288–321 | `authority_effect_action` hardcoded 8-leaf + 16-root prose switch (consumed by `Compiler/JetCLI/Source/CLI/Authority.jet:340, 380` and `Compiler/JetSemIndex/Source/SemIndex/LocalStages.jet:213`). |
| `Compiler/JetDriver/Source/Driver/SchedulerProvider.jet` | 1–145 | Bespoke single-service AST-injection provider binder for `Scheduler`. |
| `Tests/Guards/effect_names.jet` | 12–62 | Line scanner hardcoding `: struct` and `: enum` for effect declarations. |

---

## 3. Adversarial Analysis — Eight Attacks & Binding Constraints

### Attack 1: Non-Swappable Structural / Deny-Only Effects & `#7035` Metadata

**Attack**: If every effect becomes a `#Context`-swappable trait, a caller could write `#Context(Unsafe: NoopUnsafe{}) { unsafe_ptr_deref() }` or `#Context(Panic: FakePanic{}) { panic("x") }` or `#Context(FFI.C: Stub{}) { c_call() }` and **discharge `Unsafe`, `Panic`, `FFI.C`, `Mem.Rc`/`Stack`/`Gc`, or `Meta.*` from its effect row**. That would bypass memory safety, determinism, `#Transact` irreversibility, and deny-only policies (`--deny Unsafe`, `-[!Panic]>`, `-[!Mem.Gc]>`).

Additionally, `#7035` just landed `access: EffectAccess` (`.Reads` / `.Adds` / `.Replaces` / `.Destroys`) and `irreversible: Bool` on every effect (`Compiler/Prelude/Effects.jet:21–155`), and `Compiler/JetFoundation/Source/Policy/Authority.jet:288–321` carries per-effect human prose (`authority_effect_action`) and `expert: Bool` (`Core/Compiler/Meta.jet`). If `#Effect` traits drop this metadata or split it back into a second struct, `#7035` and `EffectVocabulary` regress into two sources of truth.

**Binding Constraint (C1)**:
1. Register `#Effect` in `Compiler/Prelude/Markers.jet` with `sites: [.Trait]` and explicit metadata fields:
   ```jet
   Effect: struct {
       parent: Effect?{None},
       access: EffectAccess{.Reads},
       irreversible: Bool{false},
       swappable: Bool{true},
       deny_only: Bool{false},
       expert: Bool{false},
       action: String{""},

       impl Marker {
           sites: [.Trait],
       }
   }
   ```
2. **Partition the catalog cleanly**:
   - **Swappable service `#Effect` traits (`swappable: true`, default)**: `Time`, `Time.Wait` (or `Wait`), `FS`, `FS.Read`, `FS.Write` (and `#7035` `FS.Add`/`FS.Replace`/`FS.Destroy`), `Net`, `IO`, `Rand`, `Env`, `Exec`, `Exec.Args`, `Exec.Exit`, `GPU`, `Browser`, `Mem.Alloc`, `DB`, `DB.Read`, `DB.Write`, `Log`, `Secret`, `Emit<T>`, plus any user-defined `#Effect` trait.
   - **Non-swappable structural/deny-only `#Effect` traits (`swappable: false`)**: `Unsafe`, `Panic`, `Mem.Rc`, `Mem.Stack`, `Mem.Gc`, `FFI` (and `FFI.*`), `Meta` (and `Meta.*`). These are zero-method `#Effect(swappable: false, deny_only: ...)` traits (or retained structural effects compiled through the same `EffectDecl` metadata). Attempting `#Context(Unsafe: x)` or `impl Unsafe for T` emits a dedicated diagnostic (**new `E0764`** / reuse `#Context` family) explaining that structural machine effects cannot be replaced by a user provider.
3. **Sub-effect metadata inheritance**: A parent effect's `irreversible` is `true` iff its own flag OR any child's flag is `true`; a parent's `access` is the lattice join (`max`) of its children's `EffectAccess` (`.Reads < .Adds < .Replaces < .Destroys`), matching `Compiler/JetFoundation/Source/Registry/EffectVocabulary.jet:549–601`.
4. **Delete `authority_effect_action` switch**: `action: String` on `#Effect(...)` becomes the single source of truth for `Authority.jet`, `CLI/Authority.jet`, and `SemIndex/LocalStages.jet`.
5. **Preserve value-scoped effects**: Existing value-scoped struct effects (`Bucket{name: "photos"}` in `Library.jet:109–128`) continue to work during migration, and Wave 2 can allow `#Effect` traits with associated scope key/constants or keep `impl Effect` on value-scoped structs as a supported subset until unified.

---

### Attack 2: Effect Discharge Soundness, `E0742` Ceilings, and Type-Erased Spoofing

**Attack**: Suppose a function writes:
```jet
spoof: fn(c: Time) -[]> Int {
    #Context(Time: c) { Time.now() }
}
```
Here `c` is a **type-erased `Time` trait object** (or a generic `<C: Time>` without a pure bound). At runtime `c` could be `SystemClock` (which actually performs `-[Time]>` / host syscalls!). If `#Context(Time: c)` naively subtracted `Time` from the body's row without adding `eff(impl(typeof(c), Time))`, then `spoof` would typecheck as pure `-[]>` while performing real `Time` effects — completely breaking effect soundness and `E0749` (`-[!Time]>`) denials!

Similarly, what if a handler `TracingClock` implements `Time` using `-[Log]>`? Discharging `Time` must **substitute** `Log` into the enclosing caller's row, not silently drop `Log`. And what if a function installs `#Context(FS.Read: StubRead{})` and the body calls both `FS.Read.read(p)` and `FS.Write.write(p, b)`?

**Binding Constraint (C2 — Exact Discharge Formula)**:
For a block `#Context(E_1: h_1, ..., E_k: h_k) { body }`, let `T_i = typeof(h_i)` and let `covered(E_i)` be the set `{E_i} ∪ descendants(E_i)` (via `#Effect(parent: ...)` and supertrait composition).

1. **Concrete handler `T_i` (known concrete struct/enum implementing `E_i`)**:
   - Let `handler_eff(T_i, E_i)` be the union of the inferred/declared effect rows of all methods of `impl E_i for T_i` (and any sub-effect traits in `covered(E_i)` implemented by `T_i`).
   - Example: `FixedClock` implements `Time.now` as `-[]>`. Then `handler_eff(FixedClock, Time) = ∅`.
   - Example: `TracingClock` implements `Time.now` as `-[Log]>`. Then `handler_eff(TracingClock, Time) = {Log}`.
   - Example: `MemoryFS` implements `FS` (supertrait of `FS.Read` + `FS.Write`) purely. Then `covered(FS) = {FS, FS.Read, FS.Write}` and `handler_eff(MemoryFS, FS) = ∅`.
2. **Type-erased (`E_i` trait object) or generic (`C: E_i`) handler**:
   - Compute `handler_eff(T_i, E_i)` using the **existing trait-method effect ceiling rule** (`Checks.jet:6093–6165`, `Docs/Spec/spec.md:3725–3730`): declared method row -> `#FX(...)` cap on the trait -> union of all visible implementers (including `E_i` itself if an ambient host impl exists!).
   - Therefore, installing an opaque `c: Time` has `handler_eff(Time, Time) ⊇ {Time}` unless the parameter explicitly carries a narrower effect bound (e.g., `c: Time` with `#FX()` or concrete type). **Opaque handlers can NEVER launder a denied effect (`E0749` stays airtight)!**
3. **Exact block effect row**:
   ```text
   eff(#Context(E_i: h_i) { body })
       = (⋃_i eff(h_i))
       ∪ (eff(body) \ ⋃_i covered(E_i))
       ∪ (⋃_{i : eff(body) ∩ covered(E_i) != ∅} handler_eff(T_i, E_i))
   ```
   - Notice the last guard (`eff(body) ∩ covered(E_i) != ∅` — or conservatively unconditionally `handler_eff(T_i, E_i)` if a dynamic indirect call inside `body` may invoke `E_i`): if `body` uses `FS.Read` and `FS.Write` under `#Context(FS.Read: StubRead{})`, `FS.Read` is discharged (replaced by `handler_eff(StubRead, FS.Read)`), while `FS.Write` remains undischarged!
   - Guard expressions `h_i` are evaluated **outside** the installed context (in the enclosing effect row), so `#Context(Time: SystemClock.from_host())` still charges `h_i`'s own construction effects to the caller.
   - **Self-recursion / re-wrapping**: If `TracingClock.now` itself uses `Time` (e.g., a wrapper delegating to the outer `Time` captured at construction), `handler_eff(TracingClock, Time)` includes `Time`, which stays in the enclosing caller's row — so wrapping a real clock in a logger still accurately requires `-[Time, Log]>` from the outer caller!
   - **`E0742` interaction**: A concrete `impl E for T` whose methods exceed `E`'s `#FX(...)` cap (e.g., `DB` is capped at `#FX(Net, FS)`) continues to trigger `E0742` at the `impl` site (`Checks.jet:6133–6165`). For traits with no `#FX(...)` cap (like `Time` or user `#Effect` traits), `handler_eff(T, E)` carries the exact concrete row of `T`'s methods, so a `FixedClock` is `-[]>` and a `LoggingClock` is `-[Log]>`.

---

### Attack 3: Generic `#Effect` Traits (`#7003` `Emit<T>`) & Multi-Instantiation Shadowing

**Attack**: `#7003` requires `#Effect Emit: trait<T> { emit: fn(^self, item: T) }` to power both synchronous push collectors (`#Context(Emit<Int>: ^buf)`) and lazy streams (`-> Stream<T>`). Three sharp traps hide here:

1. **Parser syntax trap**: Live `dev` (`Compiler/JetFoundation/Source/AST/Items.jet:610–614`, `Compiler/JetParser/Source/Parser/Items.jet:2920–2921`) parses parameterized traits as **`Name: trait<T> { ... }`**, NOT `Name<T>: trait { ... }`! Writing `#Effect Emit<T>: trait` would fail to parse unless either the canonical spelling uses `#Effect Emit: trait<T>` (matching existing `Store: trait<T>`) or the parser accepts both.
2. **Type-erasure slot collision trap**: If the runtime `#Context` slot key were computed from the uninstantiated trait name `"Emit"`, then nesting `#Context(Emit<Int>: ^ints) { #Context(Emit<String>: ^strs) { Emit<Int>.emit(1) } }` would **shadow `Emit<Int>` with `Emit<String>` and corrupt memory (calling `Emit<String>.emit` with an `Int`)**!
3. **Generic discharge & polymorphism trap**: Inside a generic function `map_emit: fn<A, B>(xs: [A], f: fn(A) -> B) -[Emit<B>]>`, the effect row contains `Emit<B>` where `B` is a type parameter; inside `#Context(Emit<Int>: ...)`, `Emit<Int>` must NOT discharge `Emit<String>` or an unrelated `Emit<T>`.

**Binding Constraint (C3)**:
1. **Canonical spelling**: Use Jet's existing parameterized-trait syntax `#Effect Emit: trait<T> { emit: fn(^self, item: T) }` everywhere in Prelude/Core/spec (and add a clear parser fixit if someone writes `Emit<T>: trait`, or check whether `Store: trait<T>` is already locked in `Spellings.jet`).
2. **Per-instantiation identity (reusing existing `sema_trait_instances_rewrite_inputs`)**:
   - `JetSema` already rewrites each concrete instantiation `Trait<Arg>` into a canonical monomorphized trait identity (`AST/Items.jet:611–613`).
   - Key both sema effect-row membership AND runtime `Context.values@32` slot keys by the **canonical monomorphized trait ID** (e.g., `core.iter.Emit<Int>` vs `core.iter.Emit<String>`), NEVER by the bare uninstantiated name `"Emit"`.
   - When discharging in `Checks.jet`, `Emit<X>` discharges `Emit<Y>` iff `X` and `Y` unify identically after substitution.
   - In `JetIR` monomorphization, every generic function body referencing `Emit<T>` resolves its context slot key constant AFTER substituting `T`, so `map_emit<Int, String>` looks up `core.iter.Emit<String>` while `map_emit<Int, Int>` looks up `core.iter.Emit<Int>`.
3. **`-> Stream<T>` unification**: A stream-producing function `fn(...) -> Stream<T>` lowers its body as an implicit `#Context(Emit<T>: ^generator_emitter)` backed by `JetRuntime`'s existing `Generator.jet` fiber/resume handle. Calls inside the stream body (including helper functions with `-[Emit<T>]>`) invoke the exact same `Emit<T>.emit` operation — so any helper written against `-[Emit<T>]>` works identically inside a lazy `-> Stream<T>` AND inside a synchronous `#Context(Emit<T>: ^vec_collector)` with zero code duplication.

---

### Attack 4: Static Specialization vs Dynamic Dispatch — I9 Parity & Clone Explosion

**Attack**: `#7030` promises that `#Context(Time: FixedClock{...}) { work() }` specializes known handlers to direct inlined calls, while dynamic/opaque handlers use `Context.jet`'s runtime `values@32` slot lookup. Four failure modes threaten I9 parity across **Native O0, Native O1, Native O2, Comptime (`JetEval`), and Web (`JetWeb`)**:

1. **Indirect calls and spawned tasks inside a specialized block**: If O1/O2 elides pushing `FixedClock` onto `Context.values@32` because direct callees were specialized, then an indirect function pointer `cb: fn() -[Time]>` or a `task.spawn` called inside `work()` will read the **outer** `Time` provider instead of `FixedClock` — a silent O0-vs-O2 behavioral divergence (I9 violation!).
2. **Evaluation order, drop order, and early-exit unwind**: Guard expressions (`h_1`, `h_2`) must evaluate left-to-right before `body`; on normal exit, `return`, `break`, error propagation (`?`), or `panic`, handlers must pop and drop in strict LIFO order (`h_k` ... `h_1`) across all tiers (`BuildTransact.jet:704–712` already wires `ir_tx_unwind`/`ir_stop_return` — `#Context` handlers must use the exact same unwind frames).
3. **`^self` state visibility across specialized and unspecialized callees**: If a `^self` handler (like `Emit<T>` into a `^collector` or a stateful `^mem_fs`) is copied by value into a specialized callee instead of passed by pointer (`^H`), mutations made in the callee disappear on return — diverging from O0!
4. **Combinatorial clone explosion**: Nested `#Context(A: h1, B: h2, C: h3)` calling a deep call graph could multiply IR functions by $O(\prod |H_i|)$ and blow up compile times and binary size.

**Binding Constraint (C4)**:
1. **Single semantic lowering first (Wave 1 — guarantees I9 automatically)**:
   - Lower `#Context(E: h) { body }` in `JetIR` using the unified `Context` push/pop + unwind cleanup in `Compiler/JetRuntime/Source/Context.jet`. Because Native O0/O1/O2, Comptime (`JetEval/Source/IR/Native.jet`), and Web (`JetWeb/Source/Web/Build.jet`) all execute `JetIR` + `JetRuntime/Source/Context.jet`, **every tier gets identical semantics on day one**, and pure handlers like `#Context(Time: FixedClock{...})` work inside `#run` comptime automatically!
2. **Specialization as a semantics-preserving `JetIR` optimization pass (Wave 3, O1/O2)**:
   - For a `#Context(E: h)` where `h` has a statically known concrete type `H`, rewrite direct callees in the lexical call cone that require `E` into specialized clones receiving `^H` (pointer to the handler's slot — NEVER a by-value copy!) and replacing `E.op(...)` lookups with direct calls `H.op(h_ptr, ...)`, which the existing inliner then inlines to zero overhead.
   - **Elision soundness rule**: The runtime `context_push`/`context_pop` on `values@32` may be elided **ONLY IF** the entire reachable call cone inside `body` contains: (a) no indirect calls (`CallIndirect` / trait-object dispatch) whose effect row may include `E`, (b) no `task.spawn` / `task.group` / closure capture that inherits `Context`, and (c) no unspecialized external/FFI/budget-exceeded callee that uses `E`. Otherwise, keep the `context_push`/`context_pop` pointing to the **exact same `h` storage** so specialized direct callees and dynamic indirect callees observe the **identical handler instance and mutations**!
   - **Specialization budget**: Cap clones per `(func, concrete_handler_tuple)` (e.g., max 4 clones per callee, max call depth 8, skip cold/large functions above an IR instruction threshold), falling back seamlessly to dynamic `Context` lookup when exceeded.
   - **I9 Differential Test Gate**: Every Wave 1 and Wave 3 test matrix must run identical programs (covering nested shadowing, `^self` mutation, early `return`, `?` propagation, indirect callbacks, and `Emit<T>`) across Native O0, Native O2, Comptime (`#run`), and Web, asserting byte-identical output.

---

### Attack 5: Ownership, `^self` Mutability, and Task/Closure Escape Soundness

**Attack**: Live `dev` currently has **two incompatible `#Context` lifetime models**:
- `allocator` (`Statements/ContextBlocks.jet:233–251`, `Ownership/Checks.jet:395–415`) borrows a stack local `^a`, pins it via `SemaOwnershipBlockAllocator` (`E0204`), and stores a raw non-owning pointer in `Context.allocator@24`.
- `#ContextKey` (`Context.jet:1–125`) moves an owned value into a heap-allocated ref-counted `ContextNode` (`owners@0`, `release@16`, `key@24`, `next@32`, `value@40`) on `Context.values@32`, which `context_inherit` **retains and shares across concurrently spawned tasks**!

If a user installs a mutable `^self` handler (like `#Context(Emit<Int>: ^buf)` or `#Context(FS: ^mem_fs)`) and spawns a task inside the block:
- Storing a raw stack pointer in an inherited `Context` node creates a **use-after-free** when the parent block exits before a detached task finishes!
- Sharing a mutable `^self` handler across two concurrent tasks in `task.group` creates an **unsynchronized data race**!

**Binding Constraint (C5 — Clean Split Between Shareable `self` Traits and Scoped `^self` Traits)**:
1. **Receiver convention for `#Effect` traits**:
   - **Concurrent service `#Effect` traits** (`Time`, `FS`, `FS.Read`, `FS.Write`, `Net`, `IO`, `Rand`, `Env`, `Exec`, `GPU`, `Browser`, `DB`, `Log`, `Secret`): every method takes **`self` (shared/immutable receiver)** — matching existing `Driver: trait` (`Traits.jet:269–276`), `Accelerator: trait` (`Compute.jet:472–481`), and `Clock: struct` (`Core/Time/Time.jet:81`, which uses `Shared<ClockState>` for mutable test fakes!). Because all methods take `self`, an owned handler value implementing `Send`/`Share` (or wrapped in `Shared<State>`) moves safely into a ref-counted `Context.values@32` node and is **safely inherited by spawned tasks with zero data races and zero use-after-free**.
   - **Single-task stateful `#Effect` traits** (`Emit<T>`, `Allocator`, or any user `#Effect` trait with at least one `^self` method): marked internally by sema as **scoped-mutable** (inferred automatically whenever any method takes `^self`, zero extra user annotation needed!).
2. **Two explicit `#Context` binding modes (zero new grammar — uses existing `^expr` borrow syntax)**:
   - **Owned install `#Context(Time: FixedClock{...})`**: Moves the handler into a ref-counted `Context.values@32` node (or stack slot when specialization proves non-escape). Inheritable by spawned tasks iff the trait is `self`-only (not scoped-mutable).
   - **Borrowed install `#Context(Emit<Int>: ^buf)` or `#Context(Allocator: ^arena)` or `#Context(FS: ^mem_fs)`**: Borrows a local variable for the exact lexical extent of the block, generalizing `SemaOwnershipBlockAllocator` (`Ownership/Checks.jet:395–415`) into **`SemaOwnershipContextBorrow`**:
     - The borrowed local cannot be moved, reassigned, or mutably aliased while the `#Context` block is active (`E0204`).
     - **Escape prohibition (Scala 3 capture lesson)**: Inside a `#Context` block with an active borrowed or `^self` handler `E`:
       1. A detached `task.spawn` or escaping closure whose effect row includes `E` is **rejected at compile time** (`E0765`: scoped/borrowed `#Context` handler for `E` cannot outlive its block or be shared across concurrent tasks).
       2. A structured `task.group` inside the block may inherit `self`-only borrowed handlers only if the trait is `self`-only and the group joins strictly before block exit; `^self` handlers (`Emit<T>`, `Allocator`) **cannot be accessed concurrently across tasks** (a task spawned inside `task.group` does not inherit a `^self` handler — if its body calls `Emit<T>`, sema rejects it unless the task installs its own `Emit<T>` or uses a channel).
   - **Bonus win**: This replaces `Core/Mem/Mem.jet:372–391`'s unsafe `__allocator_handler` (`#Policy(sentries: .Off)` + raw `mem.address_of`) with the exact same checked `SemaOwnershipContextBorrow` mechanism!

---

### Attack 6: Ambient Host Defaults, Target Profiles, `authority:`, `--allow`/`--deny`, and Runtime Shadow Stores

**Attack**: Today four separate systems bypass `#Context`:
1. `Compiler/JetDriver/Source/Driver/SchedulerProvider.jet:1–145` injects a build-time provider for `Scheduler` only, while `Core/Target/Target.jet:62–83` hardcodes `!Net`, `!FS`, ..., `!Mem.Alloc` from 15 `MachineFacts` `String?` fields, duplicated in `Compiler/Prelude/Facts.jet:358–505` and `Compiler/JetFoundation/Source/Target/TargetMachine.jet:7–43`.
2. `Compiler/JetRuntime/Source/World.jet:1–110` (`World144` on `TCB@104`) + `Core/Testing/Testing.jet:1685–1704` (`rt_time_monotonic_provider_set`), `HostTime.jet:172–210` (`ENV_JET_PROVE_REPLAY_TIME_MS` / `ProveJournal`), and `FaultSchedule.jet:1–60` (`fault_should_fail`) are parallel runtime hooks that intercept time, clocks, and faults outside `Context.values@32`.
3. `Compiler/JetFoundation/Source/Policy/Authority.jet:266–275` supports **scoped grants** like `FS.Read("/data")` and `Net("api.example.com")` in `jet.toml` `authority:` and `--allow`/`--deny`.

If `#7030` ignores these, we end up with **two competing provider layers** (`#Context` vs `World.jet`/`ProveReplay`/`FaultSchedule`) or break scoped `authority:` grants and target-denied effect diagnostics (`TargetDeniedEffect`).

**Binding Constraint (C6)**:
1. **Ambient root defaults**: When `Context.values@32` has no override for a swappable `#Effect` trait `E`, dispatch falls back to the target's registered default provider for `E` (e.g., `HostTime`, `HostFS`, `HostNet`, `HostRand`, `HostAllocator`). On a target profile that supplies **no default provider** for `E` (e.g., `wasm32-freestanding` has no `HostFS` or `HostNet`), the compiler automatically derives `!E` (`TargetDeniedEffect`) from the **single target-provider table** — deleting the hand-synced `row()` switch in `Core/Target/Target.jet:62–83`, the 16 `Target*` copies in `Prelude/Facts.jet:358–505`, and `SchedulerProvider.jet`'s bespoke AST-injection pass.
   - Subtle win: even on a target where host `FS` is absent (`!FS` on ambient calls), a function that installs `#Context(FS: MemoryFS{})` **discharges `FS` locally to `-[]>`** and compiles cleanly on freestanding/Wasm/Comptime targets!
2. **Scoped `authority:` and `--allow`/`--deny` stay orthogonal and intact**:
   - Static manifest/CLI checks (`authority:`, `--allow`, `--deny`, `Authority.jet`, `CLI/Authority.jet`) inspect the **post-discharge effect row** of `main` / exported entry points.
   - If a test installs `#Context(Time: FixedClock{})` and calls `service()`, `Time` is discharged, so the test passes under `--deny Time`!
   - If code actually reaches the ambient host `FS.Read` (undischarged), scoped grants like `FS.Read("/data")` are enforced by the host provider / `EffectScope` guard (`Context.effect_scope@16`), preserving 100% of `Authority.jet` scoped-grant semantics.
3. **Staged retirement of runtime shadow stores (no big-bang breakage)**:
   - **Wave 1**: Keep `World.jet`, `HostTime.jet`/`ProveReplay.jet`, and `FaultSchedule.jet` operating inside the **ambient host default provider** (`HostTime`, `HostFS`, `HostAllocator`). When a `#Context` override is active, the `#Context` provider runs directly; when no `#Context` override is active, the host provider consults the existing replay/fault hooks. Zero breakage of `jet prove --replay` or fault-injection tests during Wave 1.
   - **Wave 2/3**: Migrate `testing.fake_clock` / `testing.fake_rng`, `ProveReplay`, and `FaultSchedule` to install `ReplayTime`/`FaultFS`/`FaultAllocator` as ordinary `#Context` providers, then delete `World.jet`'s redundant monotonic-clock TLS slot and `rt_time_monotonic_provider_set`.

---

### Attack 7: Name Reconciliation, Sub-Effect Declaration Syntax, and Ambient Call Ergonomics

**Attack**: Three surface-syntax mismatches exist between the conceptual design examples and live `dev`:

1. **`Time` vs `Clock`**: Live `Compiler/Prelude/Effects.jet:44` declares the machine effect as **`Time`** (with child `Time.Wait`), and every spec, manifest, diagnostic, and `--allow`/`--deny` flag in the repo uses `Time`. Meanwhile `Core/Time/Time.jet:81` has a concrete `Clock` struct (`now`, `monotonic`, `sleep`). Renaming the effect `Time` -> `Clock` would churn every manifest, guard, test, and doc for zero benefit.
2. **Dotted sub-effect trait names (`FS.Read: trait`)**: Verified in `Compiler/JetParser/Source/Parser/Items.jet:2920`, `parser_parse_trait` calls `parser_expect_name` (single identifier only) — **`FS.Read: trait` does not parse today**, and `FS` cannot simultaneously be a module and a top-level trait in the same namespace unless resolved cleanly.
3. **Calling operations (`Time.now()` vs `time.now()`)**: Existing Jet code calls `time.now()`, `files.read(path)`, `log.info(msg)`, `rand.int()`. Users should NOT have to rewrite every call site to `Time.now()` to benefit from `#Context` testing, AND users writing custom `#Effect` traits (like `#Effect Emit: trait<T>` or `#Effect RateLimit: trait`) need a clean direct call syntax (`Emit.emit(x)` / `RateLimit.check(k)`).

**Binding Constraint (C7)**:
1. **Keep canonical Prelude effect names**: Preserve `Time`, `FS`, `Net`, `IO`, `Rand`, `Env`, `Exec`, `GPU`, `FFI`, `Browser`, `Panic`, `Mem`, `Unsafe`, `DB`, `Log`, `Secret` as the canonical `#Effect` names. Migrate `Core/Time/Time.jet`'s `Clock` struct into `SystemClock` and `ManualClock` (both implementing `Time`), and keep `testing.fake_clock(...)` returning `ManualClock` so existing test helpers compose directly with `#Context(Time: testing.fake_clock(0))`.
2. **Sub-effect declaration syntax (zero parser ambiguity)**:
   - Declare sub-effects as ordinary traits linked by `#Effect(parent: Root)` and composed by supertraits:
     ```jet
     #Effect(access: .Reads, action: "read files")
     FSRead: trait {
         read: fn(self, path: String) -[FS.Read]> [U8] IOError!
     }

     #Effect(parent: FS, access: .Replaces, irreversible: true, action: "write files")
     FSWrite: trait { ... }

     #Effect(action: "read and write files")
     FS: trait: [FSRead, FSWrite] { }
     ```
     Or, if dotted member names (`FS.Read`) are written in effect rows and `#Context(FS.Read: stub)`, add a `leaf: "Read"` field on `#Effect(parent: FS, leaf: "Read")` (or allow nested trait members inside a trait/module namespace) so:
     - Effect rows (`-[FS.Read]>`, `-[FS]>`, `-[!FS.Write]>`), manifests (`authority: [FS.Read]`), and `#Context(FS.Read: stub)` keep the **exact dotted spelling `FS.Read`** already locked across the language, while `parser_parse_trait` stays clean and unambiguous.
     - Implementing `FS` requires implementing its supertraits (`FSRead`, `FSWrite`), so `#Context(FS: MemoryFS{})` automatically provides and discharges `FS`, `FS.Read`, and `FS.Write`!
3. **Ambient receiver syntax + zero-rewrite Core delegation**:
   - Allow calling any `#Effect` trait's method directly on the trait name (`Time.now()`, `Emit.emit(x)`, `FS.Read.read(p)`): sema resolves `Trait.method(args...)` when `Trait` is an `#Effect` trait as: (a) charge `Trait` to the current function's effect row, and (b) lower to an implicit dispatch on the active `#Context` provider for `Trait`.
   - Core convenience modules (`time.now()`, `files.read(path)`, `log.info(msg)`, `rand.draw()`) become one-line thin wrappers calling `Time.now()`, `FS.Read.read(path)`, `Log.write(...)`, `Rand.draw(...)`. Result: **100% of existing Jet code automatically respects `#Context` with zero call-site migration!**

---

### Attack 8: Paired Performance Benchmark Specification (`#7030` Gate)

**Attack**: Without an explicit paired benchmark cell, a regression in specialization (falling back silently to `Context.values@32` linked-list traversal or failing to inline the handler method) would go undetected and leave `#Context` slower than manual parameter passing.

**Binding Constraint (C8 — Required Paired Benchmark Cell in Wave 3)**:
Add a paired benchmark (`Benchmarks/Micro/EffectContextDispatch.jet` or the repo's active benchmark harness) measuring $N = 10{,}000{,}000$ iterations of a hot loop computing a checksum from `Time.now()` (with black-box sink preventing dead-code elimination):

| Cell | Code Pattern | Expected Execution Path (O2) | Target Ratio vs Baseline |
| --- | --- | --- | --- |
| **Baseline (Direct)** | `c: FixedClock{t: 42}; loop ... { acc += c.now() }` | Direct concrete call, inlined | `1.00x` |
| **Specialized `#Context`** | `#Context(Time: FixedClock{t: 42}) { sum_clock(n) }` where `sum_clock: fn(n: Int) -[Time]> Int` | Cloned `sum_clock__FixedClock`, `context_push` elided, `FixedClock.now` inlined | **`<= 1.05x`** (goal: `1.00x`, identical inner loop assembly) |
| **Dynamic `#Context` (Opaque)** | `run_dyn(c: Time, n: Int)` calling `#Context(Time: c) { sum_clock(n) }` | Dynamic `Context.values@32` lookup + vtable call | Recorded as reference ceiling (verifies O0/dynamic path works and quantifies specialization win) |

Also include a second paired cell for `#7003` **`Emit<Int>`** (synchronous `#Context(Emit<Int>: ^counter)` vs direct `^counter.push(i)`), target **`<= 1.05x`** at O2, proving zero-overhead push collectors alongside lazy `-> Stream<T>`.

---

## 4. Binding Cutover List (Every Table, Guard, Diagnostic, and Doc)

To enforce one source of truth with zero shadow tables left behind:

### 4.1 Files / Duplicated Tables to Delete or Shrink to One Generator

| File | Cutover Action | Wave |
| --- | --- | --- |
| `Compiler/JetDriver/Source/Driver/SchedulerProvider.jet` | **Delete** bespoke AST-injection provider binder; replace with generic table-driven target default provider binding for all `#Effect` traits (including `Scheduler`). | Wave 2 |
| `Compiler/JetFoundation/Source/Policy/Authority.jet:288–321` | **Delete** hardcoded `authority_effect_action` switch; read `action` from `EffectVocabulary` (populated from `#Effect(action: ...)`). | Wave 2 |
| `Core/Target/Target.jet:62–83` | **Replace** hand-coded `MachineFacts.row()` 15-branch switch with derivation from the single target-provider table (absent provider => `!Effect`). | Wave 2 |
| `Compiler/Prelude/Facts.jet:358–505` & `Compiler/JetFoundation/Source/Target/TargetMachine.jet:7–43` | **Generate / derive** target capability facts from the same effect/target table (delete manual sync drift). | Wave 2 |
| `Compiler/JetSema/Source/Sema/ContextKeys.jet:38–83` | **Shrink** hardcoded `SEMA_CONTEXT_CORE_FIELDS` / `sema_context_field_settable`: keep only non-effect control fields (`deadline`, `frame`) plus `#ContextKey` constants; all service providers (`allocator`, `logger`, `Time`, `FS`, `Net`, `Rand`, `DB`, `Emit<T>`, ...) resolve uniformly through `#Effect` traits. | Wave 1–2 |
| `Core/Mem/Mem.jet:372–391` | **Delete** unsafe `__allocator_handler` (`#Policy(sentries: .Off)` raw pointer shim) once `Allocator` uses `#Effect` + `SemaOwnershipContextBorrow`. | Wave 2 |
| `Compiler/JetRuntime/Source/World.jet` (monotonic clock slot) & `Core/Testing/Testing.jet:1685–1704` | **Retire** `rt_time_monotonic_provider_set` TLS callback after `testing.fake_clock` installs `ManualClock` via `#Context(Time: ...)`. | Wave 2–3 |

### 4.2 Compiler & Registry Updates

- `Compiler/Prelude/Markers.jet`: Register `#Effect` marker (`sites: [.Trait]`, fields from C1); generalize `Context` marker beyond hardcoded `allocator`/`logger`.
- `Compiler/JetFoundation/Source/AST/Items.jet`: Add `effect_decl: EffectDecl?` (or `effect_meta`) on `TraitDef`; extend `EffectDecl` with `swappable`, `deny_only`, `action`, `leaf`, `type_params`.
- `Compiler/JetParser/Source/Parser/Items.jet` & `Declarations/Effects.jet`: Produce `EffectDecl` companion for `TOKEN_KW_TRAIT` when `#Effect` marker is present; keep `impl Effect` on `struct`/`enum` accepted during Wave 1 transition (for value-scoped struct effects and smooth migration).
- `Compiler/JetFoundation/Source/Registry/EffectVocabulary.jet`: Extend `effect_vocabulary_source_scan` to scan `#Effect(...) Name: trait` (and `parent:` / `leaf:` / `access:` / `irreversible:` / `swappable:` / `deny_only:` / `action:`) alongside existing `impl Effect` during migration, then make `#Effect` trait the sole canonical form.
- `Compiler/JetSema/Source/Sema/Effects/Library.jet` & `Checks.jet`:
  - Recognize `#Effect` traits (including parameterized `#Effect` traits via `sema_trait_instances_rewrite_inputs`).
  - Implement exact `#Context` effect discharge formula (C2) at `Checks.jet:9723–9728`.
  - Support ambient trait-method calls `Trait.method(...)` (C7).
- `Compiler/JetSema/Source/Sema/Statements/ContextBlocks.jet` & `Ownership/Checks.jet`:
  - Accept any swappable `#Effect` trait (including concrete instantiations like `Emit<Int>`) in `#Context(...)`.
  - Generalize `SemaOwnershipBlockAllocator` -> `SemaOwnershipContextBorrow` (C5), rejecting detached task/closure capture of borrowed or `^self` handlers.
- `Compiler/JetIR/Source/BuildTransact.jet` & `BuildTraits.jet`:
  - Lower `#Context(E: h)` push/pop + unwind cleanup using canonical monomorphized trait slot keys in `Context.values@32`.
  - Add Wave 3 specialization pass + escape analysis + clone budget (C4).

### 4.3 Diagnostics, Guards, Docs, and Editor Grammars

- **Diagnostics** (`Compiler/Prelude/Diagnostics.jet` + `Tests/Diagnostics/`):
  - Keep `E0742` (trait effect ceiling), `E0749` (denied effect), `E0761` (invalid `#Context` binding), `E0204` (pinned borrow escape).
  - Add dedicated codes for: installing a non-swappable structural `#Effect` (`E0764`), and leaking a borrowed/`^self` `#Context` handler across a detached task or escaping closure (`E0765`).
- **Guards**:
  - Update `Tests/Guards/effect_names.jet` (lines 12–62) to scan `#Effect` traits as well as (and after Wave 2, instead of) `: struct`/`: enum`.
  - Add a new guard verifying zero duplicated `authority_effect_action` / `MachineFacts.row()` switches and verifying every non-swappable structural effect (`Unsafe`, `Panic`, `FFI`, `Mem.Rc`/`Stack`/`Gc`, `Meta`) has `swappable: false`.
- **Docs, Specs, and Editor Grammars** (Wave 3):
  - Update `Docs/Spec/spec.md` (effect rows, `#Effect` trait declarations, `#Context` discharge rules, `Emit<T>`).
  - Update `Docs/Syntax/QuickRef.md`, `Docs/Guides/` (testing with `#Context` fakes), `Examples/` (add golden `#Context` provider test example).
  - Update `Editors/` syntax grammars if `#Effect` / `#Context(Trait: val)` highlighting needs adjustment (note: `#Effect` already highlights as a `#Marker` in standard Jet grammars; verify dotted `#Context(FS.Read: ...)`).

---

## 5. Concrete Wave Plan (Autonomous Execution Order)

### Wave 1 — Core Compiler Support + `#Context` Discharge + Generic `#Effect` Traits (Unblocks `#7003` `Emit<T>`)

**Goal**: Make `#Effect` traits (including parameterized `#Effect Emit: trait<T>`), `#Context(E: h)` installation, sound effect discharge, ambient `E.op(...)` dispatch, and `SemaOwnershipContextBorrow` work end-to-end across Native O0/O2, Comptime (`JetEval`), and Web (`JetWeb`) — without breaking any existing Prelude/Core effect declarations.

1. Register `#Effect` marker in `Compiler/Prelude/Markers.jet`; update `TraitDef` AST and `parser_parse_trait` companion emission.
2. Extend `EffectVocabulary.jet` and `Sema/Effects/Library.jet` to recognize `#Effect` traits (including `parent:`, `leaf:`, `access:`, `irreversible:`, `swappable:`, `deny_only:`, `expert:`, `action:`, and generic `Trait<T>` instantiations).
3. Update `Statements/ContextBlocks.jet` and `Ownership/Checks.jet` (`SemaOwnershipContextBorrow`) to accept any swappable `#Effect` trait (owned `h` or borrowed `^h`), rejecting `swappable: false` (`E0764`) and task/closure escapes of `^self`/borrowed handlers (`E0765`).
4. Implement the exact C2 discharge formula in `Sema/Effects/Checks.jet:9723–9728` (concrete narrowing, opaque/generic ceiling preservation, parent/child subsumption, guard evaluation outside scope).
5. Support ambient `Trait.op(...)` resolution in sema + `JetIR` lowering via canonical monomorphized trait keys on `Context.values@32` with full unwind cleanup in `BuildTransact.jet`.
6. **Verification**: Targeted unit + I9 differential tests covering: user `#Effect` trait, concrete pure discharge, concrete effect-substituting discharge (`LoggingClock` -> `-[Log]>`), opaque `Time` non-laundering (`E0749`), parent/child partial discharge, generic `#Effect Emit: trait<T>` with simultaneous `Emit<Int>` + `Emit<String>` non-aliasing, `^self` borrow pinning (`E0204`/`E0765`), panic/early-return unwind, and `#run` comptime execution.

### Wave 2 — Migrate Prelude & Core Services to `#Effect` Traits + Consolidate Duplicated Tables

**Goal**: Replace the split effect tags + provider structs/traits with unified `#Effect` traits and delete every hand-synced copy listed in Section 4.1.

1. Convert `Compiler/Prelude/Effects.jet`, `Core/Effects/Effects.jet`, and `Core/Compiler/Meta.jet` to `#Effect` traits (carrying `#7035` `access`, `irreversible`, `action`, `swappable`, `deny_only`, `expert`, `parent`/`leaf`, `#FX(...)`).
2. Unify Core service providers onto those traits:
   - `Time` (`SystemClock`, `ManualClock`; `time.now()` delegates to `Time.now()`).
   - `FS` / `FSRead` / `FSWrite` (plus `#7035` `Add`/`Replace` split as landed); `files.*` delegates to `FS.*`.
   - `Rand` (`SystemRand`, `SeededRand`; `rand.*` delegates to `Rand.*`).
   - `Mem.Alloc` (`Allocator` becomes the `#Effect(parent: Mem, leaf: "Alloc")` trait; delete `__allocator_handler` raw-pointer shim).
   - `Log` (`Log` trait; `log.info`/`warn` delegate to `Log`).
   - `DB` (`Driver` unified into `DB` / `DBRead` / `DBWrite`), `Net`, `IO`, `Env`, `Exec`, `GPU` (`Accelerator`), `Secret`, `Browser`.
3. Replace `SchedulerProvider.jet` with the unified target default-provider table; derive `Core/Target/Target.jet:row()`, `Prelude/Facts.jet`, `TargetMachine.jet`, and `Authority.jet:authority_effect_action` from the single catalog.
4. Keep `World.jet` / `ProveReplay.jet` / `FaultSchedule.jet` operating cleanly inside host defaults, then migrate `testing.fake_clock` / `testing.fake_rng` to `#Context`-compatible providers.
5. Update `Tests/Guards/effect_names.jet` and add the no-shadow-table / non-swappable-structural-effect guards.

### Wave 3 — O1/O2 Specialization, Paired Benchmark Gate, Spec/Docs/Examples

**Goal**: Achieve zero-overhead specialization for known concrete handlers while proving strict I9 parity.

1. Implement the `JetIR` `#Context` handler specialization pass (C4): concrete callee cloning with `^H` pointer passing, direct call rewriting for inlining, sound `context_push`/`context_pop` elision barrier (checking indirect calls, task spawns, and external callees), and clone budget caps.
2. Add and verify the paired benchmark cells (C8): `FixedClock` specialized `#Context` `<= 1.05x` direct call, and `Emit<Int>` push collector `<= 1.05x` direct push.
3. Run I9 parity matrix (O0 vs O2 vs Comptime vs Web) on all `#Context` specialization edge cases (nested shadowing, indirect callbacks inside specialized blocks, `^self` mutation, early returns, unwinds).
4. Retire redundant `World.jet` clock TLS hooks once replay/testing providers run through `#Context`.
5. Update `Docs/Spec/spec.md`, `Docs/Syntax/QuickRef.md`, `Docs/Guides/`, `Examples/`, and `Editors/` grammars.

---

## 6. Autonomous vs Optional Follow-Up Decisions

- **Ready to execute autonomously with zero blockers**: All of Wave 1 (C1–C5, C7 core rules) follows directly from the ratified `#7030`, `#7003`, and `#7035` designs and live `dev` conventions (`Name: trait<T>`, `SemaOwnershipContextBorrow`, canonical Prelude effect names `Time`/`FS`/`Net`/..., `swappable: false` for structural effects).
- **Optional owner confirmation (non-blocking, safe default chosen above)**:
  1. **Sub-effect trait naming in Prelude/Core**: Default chosen in C7 is `#Effect(parent: FS, leaf: "Read") FSRead: trait` with `FS: trait: [FSRead, FSWrite]` so `FS.Read` stays the exact row/manifest/`#Context(FS.Read: ...)` spelling without changing `parser_parse_trait`'s single-identifier rule. If the owner instead prefers extending the parser to allow `FS.Read: trait` directly on the declaration line, that is a small isolated parser change before Wave 2.
  2. **Borrowed `#Context(E: ^local)` syntax**: Default chosen in C5 supports both owned `#Context(Time: FixedClock{})` and borrowed `#Context(Emit<Int>: ^buf)` using Jet's existing `^expr` borrow syntax so stateful collectors (`Emit<T>`, `Allocator`, `MemoryFS`) can be inspected cleanly after the block exits.
