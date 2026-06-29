# UE Architectural DNA: Core Language Design Lessons for Jet

> Scope: core language primitives only. Each section maps a UE architectural pattern to a language design principle, then assesses what Jet already has and what is genuinely missing.
>
> Adversarially reviewed (12 issues corrected). Draft: agent research synthesis.

The question this document answers: not "what UE systems should Jet add," but "what does UE's architecture prove or disprove about language-level primitives — type systems, execution models, composition primitives, effect tracking — that should inform Jet's design?"

---

## 1. Algebraic Effect Tracking

**UE pattern.** Verse defines effects on two axes: an ordered exclusivity hierarchy (converges < computes < varies < transacts < no_rollback) and additive modifiers (`<decides>`, `<suspends>`). The hierarchy has subtyping — a `<computes>` function satisfies any slot expecting `<transacts>`. Critically: `<decides>` (failable) requires `<transacts>` (rollback) — they are mechanically coupled. Every failable expression inside a transactional context rolls back mutations on failure. This eliminates the partial-mutation class of bugs at the type level.

**Language design lesson.** The failure↔rollback coupling removes a real footgun: partial mutation before a failure propagates. Verse achieves this automatically; opting in is the design question. Verse's `<decides>` carries no error payload — trading diagnostic richness for composability. The hierarchy's subtyping (fewer effects ⊆ more effects) lets library authors write functions that work at any purity level.

**What Jet has.** D-EFF1–EFF5 is a complete Koka-style inferred effect row system: flat vocabulary of built-in effects (Net, Fs, Io, Db, Time, Rand, Env, Exec, Log, Gpu), inferred at the call graph level, erased in codegen. `#Pure` is the empty set. Effect polymorphism via transparent flow-through (D-EFF2). Trait-method effect bounds (D-EFF3). Scoped capability grants (D-SCAP1 / `#Grant`). Taint tracking (D-TAINT1). `#Transact` blocks with rollback (D-TXN1–4, D-TXN-ROLLBACK, D-ROLLBACK-TRAIT — ratified; implementation ongoing). Irreversible effects rejected inside `#Transact` (D-TXN2 / E0746). `scope.on_commit` and `tx.on_rollback` hooks. `<suspends>` maps to M:N green threads (D-ASYNCRT1, ratified, not yet implemented) and coroutines (D-COROUTINE1, ratified, not yet implemented).

**What's missing.**

*Partial-mutation lint is a beginner safety issue, not polish.* Jet's `?` and `#Transact` are orthogonal — `?` outside a `#Transact` block can leave partial mutations. For beginners (priority #2), this is the exact footgun Verse's automatic coupling prevents. The right answer is a diagnostic: E0xxx: "`?` propagates failure after mutation without `#Transact` — wrap in `#Transact` to roll back, or restructure to avoid partial state." This belongs in the diagnostic work and should be linted by default in the safe tier. It does not require a new language primitive.

*Effect-set ordering not formally documented.* Verse's lattice makes subtyping explicit. Jet's flat effect set has no formalized ordering — the question of whether `#(Net)` subsumes `#(Net, Fs)` or vice versa (the direction matters for function slot compatibility) is not yet documented. This is not a new feature; it is a specification gap that library authors will hit when writing effect-polymorphic APIs. The documentation of D-EFF2 should make the subsumption direction explicit.

*User-defined effects reserved but not open.* D-EFF4=B reserves `effect <Name>` syntax. This is correct. Verse's closed lattice prevents library authors from abstracting over effectful DSLs. Jet's reserved door is the right future extension.

**Status: already addressed.** D-EFF1–5, D-TXN1–4, D-ROLLBACK-TRAIT, D-SCAP1 collectively cover and exceed Verse's effect model. Action items: (a) partial-mutation lint in the diagnostic layer — no new ballot, existing E-series; (b) document D-EFF2 subsumption direction.

---

## 2. Composition Over Inheritance

**UE pattern.** `AActor` is a container whose behavior comes entirely from `UActorComponent` objects. Each component has its own lifecycle (tick, GC traversal, serialization, replication) and registers autonomously with engine subsystems on attachment. The integration point is registration — adding a component notifies all relevant systems simultaneously. This is a separate primitive from OOP: a behavior unit that owns its own participation in the runtime, not a class that is called by its owner.

**Language design lesson.** What makes component composition first-class is not syntax but semantics: (a) a behavior unit owns its update entry point, not the container; (b) a behavior unit manages its own runtime participation (GC, replication, serialization) directly; (c) adding/removing a behavior unit is a protocol between the unit and the runtime, not the container. Inheritance adds capabilities only at compile time; a capability bag with runtime registration can add or remove them dynamically. UE's failure is that all of this is implemented via macro-generated boilerplate rather than language semantics — the result is that forgetting `UPROPERTY` on a pointer silently breaks GC.

**What Jet has.** Traits and impls: compile-time shape compatibility. Generics: parametric composition. Typestate (D-STATE1, D-STATE-DECL, ratified 2026-06-22 and 2026-06-25): type-level lifecycle states with enforced transitions. `#SingleUse` (D-LIN1): linear ownership. Structured taskgroups (D-TASKSCOPE1, ratified, not yet implemented): scoped lifetime for child work. Session types / protocol declarations (D-PROTO1, D-PROTO2, ratified 2026-06-27): typed ordered-interaction lifecycles where `.Client`/`.Server` handle types make out-of-order calls compile errors. `#Reactive fn` / `#Reactive {}` (D-REACTCORE1, ratified, gated on D-SIGNAL1): reactive update-phase declarations.

D-PROTO1/PROTO2 already covers the typed ordered-lifecycle slice. D-REACTCORE1 reserves the reactive-update-phase slot. What neither covers is **spontaneous entry**: a value that, when placed in a container, autonomously registers its update handler with the container's execution scope.

**What's missing.** The autonomy property. In Jet, a struct implementing a trait is passive shape — it doesn't fire on a schedule, register with a subsystem, or clean up when detached. A type has no way to say "when I am placed in a scope, subscribe my `on_update` to that scope's update phase." This intersects with taskgroups and reactive signals but is not fully covered by either.

The right abstraction is not a game-engine primitive. It is: **an owned reactive subscription** — a value that, on creation, registers an autonomous listener in a scoped execution context, and deregisters when it is dropped. This is RAII applied to event registration. The pieces exist (RAII from Rust via Jet's ownership model, reactive subscriptions from D-REACTCORE1, structured scope from D-TASKSCOPE1) but the composition of them into a first-class registration primitive has not been designed.

**Status: genuinely new territory, gated.** D-COMP1 ballot should be written after D-NURSERY1 (task primitive — currently an open gate in D-CONCCOMB1, not yet ratified) and D-SIGNAL1 (reactive signal API) are decided. The ballot scope: can a type declare its own participation in a scoped reactive phase, and if so, what is the ownership model of the subscription handle? D-PROTO1/PROTO2 and D-REACTCORE1 should be cited as existing coverage that narrows the remaining gap.

---

## 3. Field-Level Metadata as a Language Primitive

**UE pattern.** `UPROPERTY()` annotates struct fields with policy: serialization, replication, editor visibility, value clamping. All systems (GC, replication differ, serializer, editor) read from the same runtime property table generated by Unreal Header Tool. The field declaration is the single source of truth for all cross-cutting field policy. UHT's failure: bad annotation combinations are runtime errors; UHT doesn't know types at expansion time; missing `UPROPERTY` on a pointer silently breaks GC.

**Language design lesson.** Fields benefit from co-located policy metadata that is compiler-validated against the field type. The macro approach creates the exact class of bugs (silent wrong combinations, missing annotations) that first-class type-checked annotations prevent. "Tooling-only" vs "runtime-required" metadata need distinct scoping to prevent tooling annotations from accidentally affecting codegen.

**What Jet has.** Per-field serialization annotations (D-SERDE5): `#[Rename("x")]`, `#[Skip]`, `#[Default(expr)]`, `#[Flatten]` — generated by the compiler, type-checked at codegen time. Schema migration tracking at the type level (D-MIGRATE1 / `#PublishedSchema`). Struct-level layout policy (D-SOA1/SOA2, D-REPRC1). User derives and typed reflection (S56, Epoch 3) — when landed, enables general per-field code generation for any user-defined concern.

**What's missing.**

*Field-access effects.* There is no way to declare that reading or writing a field incurs an effect — e.g., "reading this field requires `Fs` because it lazily loads from disk," or "writing this field has `Net` because it triggers replication." All D-EFF1 effect tracking is function-scoped. A field access that triggers side effects is invisible to the effect system. This is not required for v1 but matters for capability-annotated structs in systems programming contexts.

*Tooling-only annotation scoping.* Jet has no formal distinction between annotations that are erased at runtime (but drive editor/LSP behavior) and annotations that affect codegen. As Jet gains a richer LSP surface and S56-based derives, this distinction will need to be formalized to prevent accidental codegen coupling from what should be tooling-only metadata.

**Status: partially addressed.** Serde annotations cover the serialization slice. S56 (Epoch 3) covers the general-purpose slice at the user-code level. The genuine new question: field-access effects. **D-FIELDEFF1** ballot: can a field declaration carry an effect annotation that propagates to any read or write of that field? Low priority for v1, worth reserving before S56 lands and codifies field annotation conventions.

---

## 4. Data Layout Control

**UE pattern.** Mass Entity (UE5's ECS) stores entities with the same component set in contiguous memory chunks in SoA order — all `Transform` data before all `Velocity` data for the chunk. This separates "what data exists" (component types) from "how that data is laid out in memory" (SoA archetype chunks). Layout is a separately declared concern, not a runtime implementation detail. Unity's Burst compiler demonstrates the payoff: type-safe job interfaces where read vs. write access is encoded in types, enabling verified-safe auto-vectorization without unsafe.

**Language design lesson.** (1) Layout is a separate concern from structure and needs first-class syntax — Odin's `#soa [N]T` is the gold standard: access syntax unchanged, memory reorganized. (2) SoA at the collection level and SIMD at the leaf level are different primitives, both needed. (3) Expressing "any entity type that has at least these fields" (the ECS query pattern) requires structural record subtyping — more generally, **row polymorphism**, applicable to database queries, API request types, data pipelines, and ECS alike. (4) Parallel query safety requires that read vs. write access is tracked in types and the compiler verifies no aliasing.

**What Jet has.** `#Layout(columnar)` (D-SOA1, D-SOA2A–D, ratified, impl deferred post-v1): whole-struct SoA layout, field access syntax unchanged. `columnar [T]` per-container prefix reserved (D-SOA2C). `#Layout(c)`, `#Layout(packed)`, `#Layout(align(N))` for C repr and alignment (D-REPRC1, ratified). SIMD lane types: `F32x4`, `F64x2` (D-SIMD1, implemented). SIMD constructor/operator surface (D-SIMD2, implemented). Closed SIMD operator set (D-VECARITH1=A, ratified 2026-06-28): lane-wise arithmetic stays closed to built-ins; user structs use method calls, not operator overloading. Auto-parallelism (D-AUTOPAR1=A, ratified 2026-06-27): **explicit `par_*` adapters only; secret parallelization of maps/folds is rejected.** `&T` (shared read) vs `~T` (exclusive write) access distinction in the ownership model.

**What's missing.**

*Partial-field columnar.* D-SOA2B deferred this — only whole-struct SoA transformation is ratified for v1. ECS patterns often need "hot" fields columnar and "cold" fields row. Deferred until the ownership/aliasing surface is settled.

*Row polymorphism / structural record subtyping.* Expressing "any value that has at least these fields" is not covered by Jet's type system today. Generics provide parametric polymorphism; traits provide nominal interface conformance. Neither expresses open structural membership. This is broad language theory territory — not just an ECS concern. If entities in Jet are typed objects, the ECS query pattern can be approximated with traits. If they are opaque IDs with associated data, row polymorphism becomes load-bearing for query APIs. This question should be part of D-ENTITY1 (entity model ballot, currently open).

*Aliasing proofs for verified parallel access.* The `&T`/`~T` distinction is correct groundwork, but applying it to parallel task access to field arrays (guaranteeing no aliasing between concurrent readers/writers of different fields in the same SoA chunk) requires the concurrency safety model to be explicitly stated. Not yet designed.

**Status:** SoA layout — already addressed (D-SOA1/SOA2, ratified). SIMD lane types — already addressed (D-SIMD1/SIMD2, D-VECARITH1=A). Auto-parallelism — already closed (D-AUTOPAR1=A rejects secret parallelism; `par_*` adapters are the path). Partial-field layout — extends ratified decision, deferred. Row polymorphism — **genuinely new territory**, attach to D-ENTITY1 ballot. Parallel aliasing proofs — genuinely new, gated on concurrency model.

---

## 5. Blueprint Typed Pins → Type-Directed Authoring

**UE pattern.** Blueprint's most important property is not that it is visual. It is that pins are typed: you can only connect a `float` output to a `float` input; connecting incompatible types is structurally impossible, not a runtime error. This is the type system made spatially explicit. The "drag a typed pin into empty space" action — the system shows only compatible nodes — is type-directed autocomplete. Blueprint interfaces (structural duck-typed contracts without inheritance) are retroactive protocol conformance: a type satisfies an interface defined after the type was created.

**Language design lesson.** Blueprint's authoring experience is not about visual graphs. It is about expected-type elaboration permeating every authoring action. The text analog: at every expression site, the language propagates an expected type from context, and the LSP uses that type to filter and rank completions. Writing code becomes "drag a typed pin" — only expressions of the right type are surfaced. Mismatched types are errors at authoring time, not discovered at runtime. The downstream effect of getting this right: the "connected wrong pins" bug class is eliminated.

**What Jet has.** Expected-type elaboration is already the ratified direction: `.{ }` and `.[…]` constructor elision work because the expected type is known from context. D-SEMINDEX1 (semantic index, ratified 2026-06-27) is the LSP foundation — it provides the structured data over which type-directed completion queries are answered. Retroactive conformance: S83's `~~` out-of-body connector allows `impl Type.Trait { ... }` from outside the type's module.

**What's missing.** The LSP surface that exercises D-SEMINDEX1 to deliver type-filtered completions has not been designed at the proposal level. The index exists; the completion query protocol and ranking logic have not been specified. This is LSP implementation work, not a language design question — but it is the deliverable that makes the Blueprint-north-star concrete. It should be tracked as a specific LSP milestone with explicit behavioral spec (what query returns, what ranking looks like, when quick-fix auto-wraps a mismatched expression).

**Status: already addressed at the language level.** Expected-type elaboration is ratified direction. D-SEMINDEX1 is the infrastructure. The gap is LSP implementation specification — a tooling milestone, not a new language primitive.

---

## 6. Computation as Values / Declarative Dependency Graphs

**UE pattern.** UE's Render Dependency Graph: each rendering pass declares what resources it reads and writes; the system derives ordering, barrier placement, aliasing, and dead-pass culling. TaskGraph: task prerequisites are declared, not execution order — the scheduler extracts parallelism from the dependency structure. Blueprint compilation: the graph representation enables dead-code elimination and type propagation through edge constraints. GAS chained ability rollback: the implicit dependency chain through GAS activation means that half-committed chains cannot fully roll back — an explicit dependency graph would have made traversal possible.

**Language design lesson.** (1) `#Pure` expressions form an implicit value DAG — the compiler can reason about independence. (2) Task dependencies are more naturally declared as prerequisites ("A depends on B") than as execution order ("await B, then spawn A"). (3) Compile-time computed module expressions are a dependency graph at compile time — topological sort, incremental evaluation. (4) First-class rollback over a mutation sequence requires an explicit record of that sequence; GAS's chained-rollback failure is the proof that implicit dependency tracking doesn't give full traversal.

**What Jet has.** `#Pure` functions: statically verified purity boundary, compiler-provable independence. `#Transact` blocks (D-TXN1–4, D-TXN-ROLLBACK, D-ROLLBACK-TRAIT): mutations committed or rolled back atomically — the explicit-sequence form of a computation graph. The ROLLBACK-TRAIT handles custom undo paths for indirect mutation (method calls on borrowed values). Structured task combinators (D-CONCCOMB1: `race`/`all`/`any`, ratified, gated on D-NURSERY1; D-TASKSCOPE1, ratified, not yet implemented). Coroutines (D-COROUTINE1, ratified, not yet implemented). Comptime module expressions (owner's ratified direction, 4-stage arc extending comptime.rs — ratified direction, not yet fully implemented): will form a dependency graph evaluated in topological order with incremental recomputation. Deterministic replay (D-REPLAY1, ratified 2026-06-27): `#Replayable` proves no hidden non-determinism via inverse-propagation walk — computation sequences that are `#Replayable` are provably recordable and replayable. Auto-parallelism: **D-AUTOPAR1=A explicitly rejects secret parallelism** — explicit `par_*` adapters are the ratified path; the owner has already decided against silently parallelizing independent `#Pure` subexpressions.

**What's missing.**

*Task prerequisite DAGs.* Structured taskgroups (D-TASKSCOPE1) are a tree: siblings share a parent scope but have no inter-sibling dependency model. RDG-style dependency is a DAG: "task B starts only after A and C complete" where A and C are independent. This matters for pipeline-style programs inside Jet (e.g., a multi-stage data pipeline where stage ordering is determined by data dependencies, not program text). The ratified combinator model suggests a library function: `tasks.all([a, b]).then(|| ...)` — no new syntax required if this is the path. Whether `task after [a, b]` belongs in the language surface or the task library is the open design question.

*`#Transact` aliased-mutation coverage.* D-TXN-ROLLBACK layer 1 (auto-snapshot) covers direct assignment. The note explicitly defers indirect mutation: "a value mutated only through a `~self` method call is NOT auto-snapshotted in v1." Layer 2 (D-ROLLBACK-TRAIT) addresses custom types with explicit undo. The remaining gap — aliased mutation paths through multiple borrows in a single `#Transact` block — is acknowledged and deferred, not unaddressed.

**Status:** `#Pure` DAG reasoning — already addressed. `#Transact` rollback — already addressed (with acknowledged deferred corner). Structured task combinators — ratified, gated on D-NURSERY1. Computed modules as dependency graph — ratified direction, implementation ongoing. D-REPLAY1 deterministic replay — ratified. Task prerequisite DAGs — likely a library function over ratified combinators; no new syntax ballot needed unless the owner wants language-level prerequisite declaration.

---

## 7. The Safety/Power Tier Question

**UE pattern.** Unreal has three tiers: C++ (native, all footguns), Blueprint (safe VM, no power), Verse (safe VM, more power, native compilation planned). Every tier boundary costs: type impedance mismatch, asymmetric call costs, separate hot-reload stories, three toolchains. Blueprint Nativization — mechanically compile Blueprint to C++ for performance — was deprecated in UE5. It failed because the high-level tier's semantic invariants (null safety, transactionality, GC) have no sound mechanical lowering to the substrate. Hot-reload reliability differs by tier: Blueprint hot-reloads because its bytecode IS the reflection artifact; C++ doesn't because binary and reflection metadata are separate compilation products.

**Language design lesson.** (1) Tiers proliferate when the substrate is unsafe and slow to iterate. The fix is a safe, fast-to-iterate substrate — not a safer DSL layered on top. (2) Mechanical compilation of a high-level tier to a low-level tier fails when the high-level tier has semantic invariants (null safety, rollback, GC) that have no sound lowering. (3) Hot-reload reliability requires the compilation artifact to be self-contained — Blueprint hot-reloads because the VM unit and the type metadata are the same artifact. (4) A single tier with a compile-time meta-tier (Zig's comptime direction) eliminates all boundary costs.

**What Jet has.** One language, one compilation tier (native via rustc). One `@unsafe { } / @unsafe fn` escape hatch (D-UNSAFE2 / I1) requiring an audit string — not a separate tier, not a separate toolchain. Cranelift JIT as the dev-loop tier (D-JIT2): same language semantics, different execution path, not a semantic tier boundary. `jet dev` hot-reload at module granularity (D-HOTSWAP1): type-stable edits swap code; type-changing edits restart — the module is the reload unit, not the binary. Comptime evaluator: same Jet expressions at compile time and runtime.

**What's missing.** Nothing of architectural significance. Jet's single-tier design already makes the correct choices that UE failed to make. `@unsafe` is an expert escape hatch within the language, not a tier below it. The JIT is a dev-loop optimization, not a semantic boundary. The comptime direction follows Zig's successful model.

The one follow-on question worth noting: should the `@unsafe` audit trail have a first-class runtime enforcement mode (e.g., production builds log entries into unsafe regions for incident analysis)? This is a tooling/observability question gated on the observability surface (D-OBS), not a language primitive.

**Status: already addressed.** No ballot needed.

---

## What UE Validates (Already Covered)

These Jet decisions are confirmed correct by UE architectural evidence:

| Jet decision | UE evidence |
|---|---|
| Safe-by-default / expert opt-in (`@unsafe` gate) | C++/Blueprint tier failure proves the substrate must be safe; adding a safe tier atop an unsafe one doesn't work |
| D-EFF1–5 effect tracking | Verse proves effect tracking is load-bearing for safe composable systems; Jet's model is more expressive |
| D-TXN1–4 `#Transact` rollback | GAS chained-activation rollback failures prove that implicit mutation + failure = bugs at scale |
| D-AUTOPAR1=A explicit par_* | Secret parallelism of pure expressions is rejected; UE's `TaskGraph` requires explicit prerequisite declaration for the same reason |
| D-SOA1/SOA2 layout annotations | Mass Entity proves layout separation is not optional at scale |
| D-SIMD1/SIMD2 + D-VECARITH1=A | Closed SIMD lane types with built-in operators is the right call; user-extensible overloading adds complexity without proportional benefit |
| Single compilation tier, no scripting VM | Blueprint's tier proliferation and Nativization failure prove tiers accumulate cost faster than benefit |
| D-STATE1/D-STATE-DECL typestate | Component lifecycle enforcement in UE is boilerplate because C++ has no typestate; Jet's typestate is the correct primitive |
| D-PROTO1/D-PROTO2 session types | Typed ordered-interaction protocols eliminate the "called methods out of order" class UE's component lifecycle is vulnerable to |
| D-SEMINDEX1 + expected-type elaboration | Blueprint's "typed pins" model proves type-directed authoring is the right UX primitive; the language mechanism is expected-type propagation |

---

## Genuine Open Questions (Require Ballots)

| ID | Question | Gated on | Priority |
|---|---|---|---|
| D-COMP1 | Owned reactive subscription primitive — can a type declare its own participation in a scoped execution phase? What is the ownership model of the subscription handle? | D-NURSERY1, D-SIGNAL1 | After concurrency surface settles |
| Row polymorphism (attach to D-ENTITY1) | Structural record subtyping for "at least these fields" — motivated by ECS queries but general; what is the type system extension? | D-ENTITY1 | Part of entity model ballot |
| D-FIELDEFF1 | Can a field declaration carry an effect annotation that propagates to any read/write of that field? | S56 landing (Epoch 3) | Low priority v1; reserve before S56 codifies field annotation conventions |

Three additional action items that do not require ballots:

- **Partial-mutation lint**: add E0xxx diagnostic — "`?` propagates failure after mutation without `#Transact`" — default on in the safe tier. Diagnostic work, not a new primitive.
- **D-EFF2 subsumption documentation**: explicitly document the effect-set ordering direction (which set subsumes which) for library authors writing effect-polymorphic APIs.
- **LSP milestone spec**: specify the completion query protocol over D-SEMINDEX1 — what query returns, how results are ranked by type compatibility, when quick-fix auto-wraps a mismatched expression.
