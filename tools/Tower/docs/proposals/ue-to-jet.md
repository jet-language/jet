# UE-to-Jet: Language Design Proposals from Unreal Engine Systems

> Draft: agent-generated research synthesis — adversarially reviewed. Syntax sketches use valid Jet wherever decisions are ratified; unratified constructs carry the blocking decision ID in a comment. §6 is a v2 planning section — tasks and channels are deferred to v2 per S53.

Unreal Engine's Gameplay Ability System, Blueprint visual scripting, Gameplay Tags, module system, and Verse language validate patterns that map onto Jet's goals and expose where UE fell short: C++/Blueprint duality, string-keyed types, implicit tag counters, and a meta-DSL build system. This document extracts the proven patterns, proposes Jet mappings, and enumerates every decision that must be balloted before implementation.

**Entity model prerequisite.** Examples throughout use `Entity` and `World` as placeholder framework types. The entity model — ECS (entity as opaque ID with components), actor (entity as object with behavior), or hybrid — is unresolved. Ballot **D-ENTITY1** is a prerequisite for §§2 and 4.

---

## 1. Hierarchical Tags

**UE lesson.** `FGameplayTag` drives ability conditions, effect targeting, and cross-system communication without hard references. Its strengths: hierarchy is structural (prefix = ancestor), composition is additive, and parent-set queries are O(1). Its failures: (a) hierarchy lives in naming convention — renames break silently; (b) Blueprint-defined tags are not compile-checked; (c) counters are tracked separately from the tag set and routinely wrong — two stuns applied, one removed, entity is still stunned but GAS clears the effect; (d) there is no composable query object, only three boolean predicates, so complex activation conditions become chains of `&&` / `||` in host code with no serialization story.

**Jet mapping.** Tags become a first-class type tree. `TagSet<T>` tracks each tag as a counter: `add` increments, `remove` decrements, `has(t)` means `count(t) > 0`. Counter semantics prevent the double-add / single-remove trap. `TagQuery<T>` is a composable, storable boolean expression over tag presence. Closed tags support exhaustive matching; open tags require a wildcard arm. A tag-addressed event bus routes typed payloads by tag with no sender-to-receiver coupling.

**Keyword collision note.** D-QUAL2 defines "tag" as the internal name for erasing `#`-markers (e.g., `#Tainted`, `#SingleUse`). Users write those as `#PascalCase` markers, not a `tag` keyword. The `tag` declaration keyword proposed below is therefore likely available, but FR-1 must confirm this and ballot the names before any parser work.

```jet
// [FR-1 pending — `tag`, `open tag`, `extend` spellings unratified (D-TAG1)]
tag Ability {
    Attack { Melee; Ranged { Arrow; Bolt } }
    Buff   { Haste; Shield }
    Status { Stunned; Burning { Fire; Cold } }
}

// TagSet — counting container; add/remove increment/decrement per-tag
// [D-TAG2: set literal syntax needed — `.{ }` is struct construction, cannot be reused]
active := TagSet<Ability>.from([.Attack.Melee, .Status.Burning.Fire])

active.has(.Attack)              // true  — Attack.Melee count propagates up the lattice
active.has_exact(.Attack)        // false — only Attack.Melee is stored
active.count(.Status.Stunned)    // 0     — raw counter; has() is count() > 0
active.has_any([.Buff, .Status]) // true

active.add(.Status.Stunned)      // counter: Stunned=1
active.add(.Status.Stunned)      // counter: Stunned=2
active.remove(.Status.Stunned)   // counter: Stunned=1 — entity still stunned

// Composable query — storable, serializable, not just a predicate chain [D-TAG6]
alive_unimpaired @= TagQuery<Status>.{
    all:  [.Alive, .Grounded],
    none: [.Stunned, .Frozen],
}
if alive_unimpaired.eval(entity.tags) { ... }

// GAS-style full boolean query for complex activation conditions
activation @= TagQuery<Ability>.and([
    TagQuery<Ability>.has(.Buff.Haste),
    TagQuery<Ability>.not(TagQuery<Ability>.has(.Status.Stunned)),
])

// Exhaustive match over a single tag value — closed tag, compiler verifies full coverage
if ability_tag {
    .Attack.Melee  -> handle_melee()
    .Attack.Ranged -> handle_ranged()
    .Buff          -> apply_buff()     // covers Haste + Shield
    .Status        -> apply_status()  // covers Stunned + Burning.*
}

// Reactive subscriptions — methods returning RAII guard; deregistered when guard drops
// [D-TAG3: method names and guard type; no `on` keyword needed with method-call form]
sub_a @= active.on_added(.Status.Stunned,   fn { disable_movement() })
sub_b @= active.on_removed(.Status.Stunned, fn { enable_movement() })
sub_c @= active.on_changed(.Status,         fn { update_status_ui() })

// Open tag — extension from another module [D-TAG1, `extend` unratified]
open tag DamageType { Physical; Fire; Cold }
// another module:
extend DamageType { Lightning }  // match against DamageType must have `else ->` arm

// Tag-addressed event bus — sender holds no reference to receiver [D-TAG7]
// Payload type is declared with the tag; emit routes by tag at runtime
emit(Ability.Attack.Melee.Activated, HitEvent.{ origin: pos, target: entity })
sub_d @= on_event(Ability.Attack.Melee.Activated, fn(e: HitEvent) {
    apply_effect(e.target, DamageEffect.{ magnitude: 40.0, element: .Physical })
})
```

**Open tag versioning.** When module A compiles against `DamageType { Physical; Fire; Cold }` and B adds `Lightning`, A's `else ->` arm silently absorbs the new variant. There is no mechanism to warn A when new variants appear. This is correct for binary-stable plugins but surprising for libraries. Ballot **D-TAG8**: opt-in "warn on new variants" flag on wildcard arms.

**Subscription lifetime and I1.** The RAII guard pattern is the I1-compliant answer: the subscription cannot outlive the guard, and the guard type does not implement `Copy` or `Share`, so it cannot be stored in a longer-lived scope than the subscriber. The callback must not capture borrowed references that outlive the guard.

**Runtime.** Tags get dense integer IDs at compile time. `has()` is an ID-range check, O(1). Open tags use range-segment allocation. Serialized as path strings; resolved to IDs at load with a typed error on unknown tags (ballot **D-TAG4**: hard error vs logged warning).

**No-std.** `TagSet<T>` requires an allocator. A fixed-capacity `StaticTagSet<T, N>` is needed for embedded targets. Ballot **D-TAG9**.

**Decisions.** D-TAG1 (`tag` / `open tag` / `extend` vs FR-1 option B — see FR section), D-TAG2 (set literal syntax — ballot a new token since `.{ }` is struct construction), D-TAG3 (subscription method names and RAII guard type), D-TAG4 (unknown tag at load: hard error or warn+skip), D-TAG5 (`remove` at count 0 — clamp to 0 or error), D-TAG6 (`TagQuery<T>` DSL surface — field names and combinator API), D-TAG7 (tag event bus — payload type association and `emit`/`on_event` syntax), D-TAG8 (open tag warn-on-new-variant flag), D-TAG9 (no-std fixed-capacity variant).

---

## 2. Typed Composable Effects

**UE lesson.** A GAS `GameplayEffect` is a value that *describes* a state change rather than a procedure that mutates. This enables preview, undo, serialization, and rollback by removing the descriptor. Modifiers aggregate as a pure fold; `Attribute<T>` has base (permanent) and current (derived) values with hooks at mutation boundaries. GAS falls short on: (a) modifier ordering within the same op class is unspecified; (b) `MaxHealth` bounds `CurrentHealth` but this cross-attribute link is not typed — developers discover the ordering dependency at runtime; (c) the aggregate formula has no clamping step; (d) `@predicted` and `@server_only` are game-engine names embedded as if they were language primitives — they don't generalize.

**Jet mapping.** `Attribute<T>`, `Modifier<T>`, and effect descriptors are stdlib generic types. The aggregate is `#Pure`. Clamping is a declared field. Cross-attribute bounds declare their dependency explicitly. Execution context constraints (predicted vs authoritative) are a use case of D-EFF1's effect system — not new language annotations.

```jet
struct Attribute<T: Numeric> {
    base:      T,
    modifiers: [Modifier<T>],
    clamp_min: T?,
    clamp_max: T?,                    // may reference another attribute — see D-EFFECT4
    on_change: fn(old: T, new: T) -> T,
}

struct Modifier<T: Numeric> {
    op:        ModifierOp,            // Add | Mul | Div | Override
    magnitude: T,
    source:    Tag,
    duration:  Duration,
    priority:  Int,                   // Override conflicts: highest wins; ties ballot D-EFFECT5
    stack:     StackPolicy,
}

// Pure aggregation — explicit op class ordering: Add, then Mul, then Div; Override checked first
// Caller applies clamp_min / clamp_max after the fold
#Pure fn aggregate<T: Numeric>(base: T, mods: [Modifier<T>]) -> T {
    overrides @= mods.filter(fn(m) { m.op == .Override })
    if overrides.len() > 0 {
        // highest priority Override wins; tie → D-EFFECT5
        overrides.max_by(fn(m) { m.priority }).magnitude
    } else {
        adds @= mods.filter(fn(m) { m.op == .Add }).map(fn(m) { m.magnitude }).sum()
        muls @= mods.filter(fn(m) { m.op == .Mul }).map(fn(m) { m.magnitude }).product()
        divs @= mods.filter(fn(m) { m.op == .Div }).map(fn(m) { m.magnitude }).product()
        (base + adds) * muls / divs
    }
}

// Effect descriptor — a value, not a procedure
damage_effect @= DamageEffect.{
    magnitude:   50.0,
    element:     .Fire,
    duration:    .Instant,
    stack:       .Replace,
    tags_grant:  [.DamageType.Fire],
    priority:    10,
}

// Execution context constraints use D-EFF1's effect system [gated on D-EFF2+D-EFF3 impl]
// User defines effects; compiler infers and propagates transitively
// [D-EFFECT1: confirm this maps cleanly onto D-EFF1 before implementation]
//
//   effect Predictable   — may run speculatively; rolled back on server rejection
//   effect ServerAuth    — authoritative only
//
//   fn apply_buff(target: ~Entity, effect: DurationEffect) #(Predictable) { ... }
//   fn apply_exec_calc(ctx: EffectCtx) -> [Modifier<Float>] #(ServerAuth) { ... }
//
// Calling a #(ServerAuth) fn from a #(Predictable) context → compile error.
// This is orthogonal to D-CAP7 capability sigils (which govern data access, not execution context).
```

**Cross-attribute dependencies.** `MaxHealth` bounds `CurrentHealth`. When both change simultaneously, evaluation order must be declared: `MaxHealth` aggregate runs first, then `CurrentHealth` is clamped to the result. The `clamp_max` field references another attribute by path; the compiler builds a dependency graph and rejects cycles. Ballot **D-EFFECT4**: syntax for referencing another attribute as a bound.

**Periodic effects.** `ForDuration(t)` effects tick on some interval. Tick interval, on-entry / on-exit application, and accumulator type need **D-EFFECT6**.

**Replication.** Effect descriptors as values are the prerequisite for replication — a descriptor can be sent over the wire, applied speculatively on the client, and rolled back if the server rejects it. The actual replication model — prediction keys, server reconciliation, rep-notify hooks (`GetLifetimeReplicatedProps` analog), and rollback mechanics — is a network design separate from the effect system. Ballot: **D-REP1**. Nothing here implements replication; the data shapes are replication-compatible by construction.

**No-std.** `[Modifier<T>]` requires allocation. Fixed-capacity `[Modifier<T>#N]` for embedded targets — ballot **D-EFFECT7**.

**Decisions.** D-EFFECT1 (confirm `#(effects)` from D-EFF1 covers execution context constraints; map Predictable/ServerAuth as user-defined effects), D-EFFECT2 (`ModifierOp` — closed stdlib enum or user-extensible), D-EFFECT3 (`StackPolicy` design), D-EFFECT4 (cross-attribute dependency syntax), D-EFFECT5 (Override priority tie-breaking), D-EFFECT6 (periodic tick semantics), D-EFFECT7 (no-std modifier list), D-REP1 (replication model — full ballot).

---

## 3. Type-Directed Authoring and LSP

**UE lesson.** Blueprint's most powerful affordance is "drag from a typed pin into empty space" — the system shows only nodes compatible with that specific type. The second affordance: connecting a mismatched pin auto-inserts a cast node. In text, the analog of the first is type-filtered completions; the analog of the second is a quick-fix code action that wraps the wrong-type expression, not just a passive hover message.

**Jet mapping.** The language must propagate expected types everywhere so the LSP can filter completions. Typed holes let users ask "what can go here." Error messages speak in expected-type terms. `.{ }` and `.[…]` elision works because expected types eliminate the need to write the constructor name explicitly.

```jet
// Expected-type elaboration — type known from parameter; constructor elided
fn heal(target: ~Entity, amount: HealEffect) { ... }
heal(player, .{ magnitude: 30.0, duration: .ForDuration(5.0) })
//           ↑ .{ } is HealEffect from the parameter annotation

// Fan-out with expected type (S75, ratified)
entities.[take_damage(fireball)]     // element type inferred from take_damage's param

// Error messages in expected-type terms (not positional)
// NOT: "type mismatch at argument 2"
// YES: "expected HealEffect, found DamageEffect
//       hint: to damage instead, use Entity.take_damage(DamageEffect)"

// Typed hole — D-HOLE1: must not use `?` (S7, error propagation) or `??` (S71, fallback)
// Provisional candidates: `_?`, `@?`, `#Hole` — ballot required before parser work
effect @= <TYPED_HOLE>    // LSP: show all DamageEffect constructors and fns returning DamageEffect

// Framework entry points — stdlib traits, not fixed annotation tokens [D-ANNOT1]
// A type implements the trait; the runtime calls the method.
// Example trait shape:
//   trait Startable  { fn on_start(~self, world: ~World) }
//   trait Tickable   { fn on_tick(~self, dt: Float) }
//   trait EventSink<E> { fn on_event(~self, e: E) }
// Specific lifecycle names come from the framework, not the language surface.
```

**LSP behaviors.**

- Completions filtered to expected type at every expression site; ranked exact type first, subtypes second, coercible third.
- Typed hole triggers inline "what fits here" with one-line descriptions.
- Hover shows inferred type, not just declaration site.
- Rename refactor for tags: compile errors at all use sites surfaced in one pass.
- Quick-fix code action when wrong-type expression is written: LSP offers wrapping in the correct conversion. This is the Blueprint auto-cast analog — active insertion, not passive suggestion.

**Retroactive conformance.** Blueprint Interfaces (UE's structural typing for cross-class communication) solve retroactive protocol conformance: a type can satisfy an interface defined after the type. In Jet, trait impl requires an explicit block. For extension slots (§4), this means a package can add `impl Type.SlotTrait { ... }` for a type it doesn't own, using S83's `~~` out-of-body connector. Ballot **D-IFACE1** if any slot design requires conformance on a type the contributing package doesn't own.

**Debugger gap.** Blueprint-north-star also requires debugger integration. The text analog of "watch a node execute" is DAP: step through ability flows, inspect tag state on a paused entity, see which task is running mid-suspension. This is gated on D-OBS1 (DAP at GA / M17). The designs in §§1, 2, and 6 should define what DAP must expose: tag state as first-class watch expressions, modifier lists as inspectable values, task stack showing logical yield points not Rust frames (D-DBG2).

**Decisions.** D-HOLE1 (typed hole token — not `?` or `??`; nominate candidates and ballot), D-ANNOT1 (lifecycle entry-point trait names and shapes in stdlib), D-IFACE1 (retroactive trait conformance for slot contributions — when and whether needed), D-DAP-TAGS (tag state exposure in DAP watch expressions).

---

## 4. Module Extension Points and the Subsystem Pattern

**UE lesson.** `USubsystem` binds a service to a lifetime context (auto-created and destroyed with it) and makes it discoverable by type without a global singleton or forced base-class subclassing. GameFeatures + ModularGameplay extends this: named slots in host systems let plugins contribute without modifying the host. UE's failure: slot names are strings, so misspellings are silent runtime gaps, not compile errors. This is the exact failure mode the proposal's tag system is designed to prevent — and the original draft of this document reproduced it in the extension system.

**Jet mapping.** `#Subsystem(Context)` binds a module service to a lifecycle. Slots are traits, not strings: the host declares a trait; packages implement it and annotate with `#Contributes(TraitName)`. The compiler collects registered implementors at build time. Misspelling the trait name is a compile error.

```jet
// Subsystem — bound to context lifetime; auto-created/destroyed with that context
// [D-SUB1: `#Subsystem` spelling, lookup API, one-per-context enforcement]
#Subsystem(World)
struct PhysicsService {
    gravity: Vec3,
    bodies:  [RigidBody],
}

// Lookup via type-parameterized method on context
fn apply_physics(world: ~World, dt: Float) {
    phys @= world.subsystem<PhysicsService>()
    phys.step(dt)
}

// Slot — host declares a trait; the trait IS the slot identity, not a string [D-SLOT1]
#Slot
trait PostProcessEffect {
    fn apply(~self, ~frame: Frame, ctx: RenderCtx)
}

// Package contributes to the slot by implementing the trait and declaring
// [D-SLOT1: `#Contributes` spelling and registration mechanism]
#Contributes(PostProcessEffect)
struct BloomEffect { intensity: Float }

impl BloomEffect.PostProcessEffect {
    fn apply(~self, ~frame: Frame, ctx: RenderCtx) { ... }
}

// Slot dispatch — compiler-generated or runtime registry [D-SLOT2]
fn render_frame(~frame: Frame, ctx: RenderCtx) {
    loop effect in PostProcessEffect.contributors() {
        effect.apply(frame, ctx)
    }
}

// Phase-ordered initialization — eliminates static-init ordering bugs [D-PHASE1]
#InitPhase("post_engine")
fn register_audio(ctx: ~AppCtx) { ... }

#InitPhase("pre_default")
fn register_input(ctx: ~AppCtx) { ... }
```

**Why trait-not-string.** `@slot("render.post_process")` is UE's exact failure mode: misspelling `"render.post_processs"` in a contributing package compiles and ships, then silently contributes nothing. A trait is a named, importable type — misspelling is a compile error (unknown identifier), missing import is a compile error (unresolved path).

**No-std.** Subsystem registries and slot contributor tables require allocation. Static-allocation variants for embedded targets — ballot **D-SUB2**.

**Decisions.** D-ENTITY1 (prerequisite — entity model), D-SUB1 (`#Subsystem(Context)` syntax, `world.subsystem<T>()` lookup API, one-per-context enforcement), D-SUB2 (no-std static-allocation variant), D-SLOT1 (`#Slot` / `#Contributes` spelling and registration mechanism), D-SLOT2 (slot dispatch — static table generated by compiler vs runtime registry), D-PHASE1 (init phase names, ordering semantics, cycle detection).

---

## 5. Typed External Data

**UE lesson.** `UPrimaryDataAsset` subclasses replace stringly-typed config with typed C++ schemas; editor-authored instances are validated against the schema. Asset Bundles allow partial loading: the same schema's icon loads for UI, full mesh+audio for gameplay. `FPrimaryAssetId` (a `Type:Name` pair, not a file path) is stable across file moves. Failures: it is a subclass, not a value type, carrying OOP overhead for pure data; the editor is the only way to author assets (no text-file workflow); and inheritance is C++ inheritance (surprising behavior from OOP semantics applied to data).

**Jet mapping.** A `data` declaration defines a typed schema with a compiler-generated loader, bundle field annotations, stable `DataId<T>` addressing, and inheritance that is field-defaulting only (no OOP semantics). The file format is Jet syntax — `.data` files are parsed by the Jet parser; types are known at parse time; validation is compile-time.

**Why `data` and not annotated `struct`.** A `#[Serialize] struct` cannot generate a typed blocking loader, enforce the one-canonical-asset-per-ID invariant, or verify bundle field constraints at compile time. If ballot D-DATA1 determines all of this can be expressed as attributes on a struct, the `data` keyword is unnecessary — the ballot must include a worked example of both approaches before deciding.

```jet
// Typed external schema [D-DATA1: ballot `data` keyword vs annotated struct]
data MonsterData {
    name:        String,
    base_health: Float,
    base_damage: Float,
    abilities:   [AbilitySpec],

    // Bundle annotations — field only loads in named bundle [D-DATA2]
    #Bundle("ui")
    icon: Image,

    #Bundle("gameplay")
    mesh:  Mesh,
    #Bundle("gameplay")
    audio: AudioSet,
}

// Data inheritance — field-defaulting only, no OOP semantics [D-DATA5]
data GoblinBossData : MonsterData {
    base_health: 400.0,   // overrides default
    base_damage: 80.0,
}

// Stable address — Type:Name pair, not a file path
goblin_id @= DataId<MonsterData>.{ name: "Goblin" }

// Blocking load with bundle selection (uses v2 task scheduler; blocking-looking API)
fn show_inventory_item(id: DataId<MonsterData>) {
    data @= load_data(id, bundle: "ui")   // blocks current green thread; yields to scheduler
    render_icon(data.icon, data.name)
}

fn spawn_monster(id: DataId<MonsterData>, pos: Vec3) {
    data @= load_data(id, bundle: "gameplay")
    world.spawn(MonsterEntity.{ data, pos })
}
```

**File format.** A `.data` file is Jet syntax — a struct literal of the `data` type. The Jet parser reads it; field types are known at parse; unknown fields are compile errors, not runtime surprises.

```jet
// monsters/goblin.data — parsed as Jet, not TOML
MonsterData.{
    name:        "Goblin",
    base_health: 80.0,
    base_damage: 12.0,
    abilities:   [AbilitySpec.{ id: "melee_strike" }, AbilitySpec.{ id: "poison_bite" }],
}
```

**Load-time errors.** Unknown `DataId` name → typed error value, never a panic. Ballot **D-DATA4**: which IDs can be compile-time resolved (literal `DataId<T>.{ name: "Goblin" }` in source code) vs always runtime.

**No-std.** `data` loading requires I/O and allocation. It is `std`-only by definition; embedded targets do not use `data` declarations.

**Decisions.** D-DATA1 (`data` keyword vs annotated `struct` — worked example required before ballot), D-DATA2 (`#Bundle` — field-level or block-level grouping), D-DATA3 (`DataId<T>` type design — stable Type:Name addressing), D-DATA4 (compile-time vs runtime ID resolution), D-DATA5 (data inheritance — field-defaulting semantics, not OOP inheritance).

---

## 6. Task-Based Concurrency (v2 planning)

**Status.** Tasks and channels are deferred to v2 per S53. This section records the design direction so post-v1 decisions don't foreclose options. The following are ratified and must not be re-balloted:

- M:N green threads; blocking-looking calls yield transparently — **D-ASYNCRT1, D-MNIO1**
- No `async fn` / `await` function-coloring bifurcation — **D-ASYNCRT1**
- Structured taskgroup scope with cancellation — **D-TASKSCOPE1**
- `race` / `all` / `any` stdlib combinators (gated on D-NURSERY1) — **D-CONCCOMB1**
- Coroutines as primitives; suspend/resume uncolored by async syntax — **D-COROUTINE1**
- Fluent select: `g.select().recv(...).after(...).wait()?` — **D-CONCSELECT1**

**UE lesson.** AbilityTasks are GAS's structured-concurrency primitive: async sub-units that fire named output delegates at completion. Blueprint latent nodes encode the same pattern spatially — async operations look sync (just a clock icon), and multiple completion outcomes appear as distinct output exec pins. Verse's `sync` / `race` / `rush` are the rationale for D-CONCCOMB1. The key design principle: suspension happens at logical yield points (animation frame, timer expiry, input event), not arbitrary OS scheduler points.

**Planned surface (gated on D-NURSERY1 + task primitive implementation).**

```jet
// No async fn / await — all fns yield at logical points; M:N scheduler is transparent
fn fireball(caster: &Caster, target: Entity) {
    play_animation(caster, "cast_fireball")     // blocks current task; scheduler yields
    hit @= wait_hit_window(caster, radius: 5.0)
    apply_effect(hit.targets, DamageEffect.{ magnitude: caster.spell_power * 1.5, element: .Fire })
}

// Named outcomes — existing `if subject` form; no new syntax (D-ASYNC1 is closed)
result @= fetch_asset(monster_id)      // blocking; returns an outcome enum
if result {
    .Loaded(data)  -> spawn(data)
    .NotFound      -> log_warning("asset missing")
    .Timeout       -> retry_later()
    else           -> panic("unreachable")
}

// Structured taskgroup — D-TASKSCOPE1
// Planned surface from S53 option A:
tasks.scoped(fn(~g: TaskGroup) {
    g.spawn(fn { load_terrain() })
    g.spawn(fn { load_audio() })
    g.spawn(fn { load_entities() })
})    // all complete before returning; any failure cancels siblings

// Combinators — D-CONCCOMB1 (gated on D-NURSERY1)
result @= tasks.race([
    fn { player_input_received() },
    fn { sleep_for(5.0) },
])

// Coroutines — D-COROUTINE1; uncolored suspend/resume
co @= coroutine(fn {
    x @= suspend()     // yields; resumes when caller calls co.resume(val)
    x * 2
})
val := co.resume(21)   // val = 42; types flow through the coroutine boundary
```

**Closed questions (no re-ballot).**

*Named-outcome syntax.* The original draft proposed new call-site block syntax for async outcomes. With M:N green threads, `fetch_asset(id)` returns an enum; the existing `if subject { arm -> body }` form handles all cases. No new syntax.

*`@pure @suspends` was contradictory.* A function that yields transfers control and affects program ordering. It cannot be `#Pure`. A `#Pure fn` may not suspend.

*`sync { }` / `race { }` as keywords.* Ratified direction (D-CONCCOMB1) uses stdlib functions, not keywords. `tasks.scoped`, `tasks.race`, `tasks.all`, `tasks.any` — all functions, all closures. No new keywords needed; closure boundaries yield transparently in the M:N model.

**Decisions.** D-NURSERY1 (taskgroup/nursery primitives — prerequisite for all combinators), task channel API spelling (follow-on to S53 option A: `tasks.channel<T>()`, send/recv types, buffer policy), coroutine resume/suspend method names (D-COROUTINE1 ratified, implementation pending).

---

## 7. Package Visibility Layers

**UE lesson.** UE module `Type` fields (`Runtime`, `Developer`, `Editor`) let a plugin contain code for all contexts; shipping strips non-runtime modules automatically. `PublicDependencyModuleNames` vs `PrivateDependencyModuleNames` controls transitive re-export. Engine → Plugin → Game enforces no upward dependencies by construction. BUILD.cs in C# is the system's central failure — build config in a meta-DSL requiring a separate runtime is pure friction.

**Jet mapping.** `pack.jet` is the build config in Jet syntax. Module tiers are declared in the manifest. Transitive re-export requires explicit `pub use` (not yet ratified — D-PKG2). The structural dependency hierarchy (stdlib → package → game) is enforced at build time (D-PKG3). Slot contributions in `pack.jet` reference trait types by import, not by string (connection to D-SLOT1 from §4).

```jet
// pack.jet — Jet syntax, no meta-DSL [D-PKG1: confirm tier name set]
package MyPlugin.{
    version: "1.0.0",

    modules: {
        core: Module.{
            tier: .Runtime,
            deps: {
                public:  [jet_std, serde],    // consumers transitively get these [D-PKG2]
                private: [internal_math],     // not re-exported
            },
        },
        bench: Module.{
            tier: .Developer,                 // stripped from shipping builds
            deps: { private: [criterion] },
        },
        scripts: Module.{
            tier: .Tool,                      // build-tool-only
        },
    },

    // Typed slot contributions — trait import, not string [D-PKG4]
    contributes: [
        PostProcessEffect: BloomEffect,
    ],
}

// Source file — transitive re-export [D-PKG2: `pub use` not yet ratified; ballot required]
pub use serde.Serialize
pub use serde.Deserialize
```

**Structural hierarchy rule.** `jet_std` cannot depend on any jetpack package. A jetpack package cannot depend on a game module. Cycles between packages in the same tier are a build error with a message naming the cycle. This is structure, not convention.

**`contributes:` and typed slots.** The `PostProcessEffect: BloomEffect` pair in `pack.jet` references the `PostProcessEffect` trait by its Jet import path. A misspelling is a build-time compile error. This is the §4 string-free slot design applied to the package manifest.

**Decisions.** D-PKG1 (tier names — Runtime/Developer/Tool or different), D-PKG2 (`pub use` transitive re-export syntax — not covered by S16; ballot required), D-PKG3 (structural hierarchy enforcement — compiler, jetpack CLI, or both), D-PKG4 (`contributes:` field syntax in pack.jet and its connection to `#Slot` traits from §4).

---

## Fundamental Restructuring Proposals

These require owner attention before any implementation in the affected sections.

**FR-1 — `tag` as a new keyword vs extending `enum`.**

The proposal introduces `tag`, `open tag`, and `extend` as a hierarchy-capable alternative to `enum`. Adding `tag` potentially violates I8 (one mechanism per semantic job) if `enum` can be extended to do the same work.

*Option B — extend `enum` with optional child declarations.* Nested enum declarations establish the hierarchy. `is-a` is structural: a parent variant is the union of its child variants. `open` / `closed` become modifiers on `enum`.

*Option A — new `tag` keyword.* `tag` is a type-system primitive with subtype semantics, range-ID runtime representation, and open/closed distinction. `enum` remains closed discriminated unions with no subtyping. They are semantically distinct: `enum` is "a value IS one variant"; `tag` is "a label IS-A member of a lattice." `TagSet<Ability>` can hold any node of the lattice because all nodes share a common base type; no `EnumSet<Ability>` analog exists for enums without boxing. The range-ID representation for O(1) `has()` is incompatible with enum's discriminant layout. `extend` has no natural meaning for a closed `enum`.

The case for option A is strong, but the ballot must provide worked examples of both options showing exactly where option B breaks before committing to a new keyword.

One naming concern: D-QUAL2 defines "tag" as the internal name for the category of erasing `#`-markers. Users write those as `#PascalCaseName`, not a `tag` keyword. The `tag` declaration keyword proposed here is likely available, but the ballot should confirm this explicitly to avoid internal documentation confusion.

---

**FR-2 — Execution context constraints are D-EFF1, not new annotations.**

The original draft proposed `@predicted` and `@server_only` as new language annotations. These are game-networking concepts that do not generalize. D-EFF1 (ratified, gated on D-EFF2+D-EFF3 implementation — both now ratified) provides exactly what is needed: user-defined effects propagated transitively through call graphs with `#(effect_name)` on signatures. Game-engine execution contexts (`Predictable`, `ServerAuth`) are user-defined effects in this system. FR-2 is not a fundamental restructuring — it is a note that D-EFF1 is the right primitive and §2 implementations must wait for D-EFF1 to ship.

D-EFFECT1 should confirm that `#(Predictable)` and `#(ServerAuth)` as user-defined effects map cleanly onto D-EFF1's semantics before any §2 context-annotation work begins.

---

**FR-3 — Structured concurrency primitives.**

D-CONCCOMB1 (`race`/`all`/`any` combinators), D-TASKSCOPE1 (structured taskgroup scope), D-COROUTINE1 (coroutines, uncolored), and D-CONCSELECT1 (select surface) are ratified. These are not re-ballot items.

What remains: the task primitive API surface. S53 option A (`tasks.spawn(closure) -> Task<T>`, `t.join() -> T`, `tasks.channel<T>()`) was the planned direction, but the owner required a memory-capability model review before diving in. That review — how task spawning, closure capture, and the D-CAP7 capability sigils interact — is the outstanding prerequisite for D-NURSERY1.

---

## Decision Requirements

| ID | Question | Type | Blocks |
|---|---|---|---|
| D-TAG1 | `tag` / `open tag` / `extend` keywords vs FR-1 option B (extend `enum`); confirm `tag` keyword does not conflict with D-QUAL2 internal category name | Ballot (FR-1) | §1 |
| D-TAG2 | Set literal syntax for `TagSet` construction — `.{ }` is taken; ballot a new token | Ballot | §1 syntax |
| D-TAG3 | Subscription method names and RAII guard type | Ballot | §1 |
| D-TAG4 | Unknown tag at load: hard error or warn+skip | Owner | §1 runtime |
| D-TAG5 | `remove` at count 0: clamp to 0 or error | Ballot | §1 runtime |
| D-TAG6 | `TagQuery<T>` DSL surface — field names and boolean combinator API | Ballot | §1 query |
| D-TAG7 | Tag event bus — payload type association and `emit`/`on_event` syntax | Ballot | §1 events |
| D-TAG8 | Open tag warn-on-new-variant opt-in flag for wildcard arms | Ballot | §1 open tags |
| D-TAG9 | No-std fixed-capacity `StaticTagSet<T, N>` | Ballot | §1 embedded |
| D-EFFECT1 | Confirm execution context constraints map onto D-EFF1 user-defined effects before any §2 annotation work | Ballot | §2 |
| D-EFFECT2 | `ModifierOp` — closed stdlib enum or user-extensible | Ballot | §2 stdlib |
| D-EFFECT3 | `StackPolicy` design | Ballot | §2 stdlib |
| D-EFFECT4 | Cross-attribute dependency syntax and evaluation order guarantee | Ballot | §2 sema |
| D-EFFECT5 | Override priority tie-breaking — compile error or deterministic rule | Ballot | §2 sema |
| D-EFFECT6 | Periodic effect tick semantics — interval, on-entry/on-exit, accumulator type | Ballot | §2 stdlib |
| D-EFFECT7 | No-std fixed-capacity modifier list `[Modifier<T>#N]` | Ballot | §2 embedded |
| D-REP1 | Replication model — prediction keys, server reconciliation, rep-notify hooks | Ballot | §2 networking |
| D-HOLE1 | Typed hole token — not `?` (S7) or `??` (S71); nominate candidates | Ballot | §3 LSP+parser |
| D-ANNOT1 | Lifecycle entry-point trait names and shapes in stdlib | Ballot | §3 |
| D-IFACE1 | Retroactive trait conformance for slot contributions on types the contributor doesn't own | Ballot | §3, §4 |
| D-DAP-TAGS | Tag state, modifier lists, and task stack as first-class DAP watch expressions | Ballot | §3 debugger |
| D-ENTITY1 | Entity model — ECS vs actor vs hybrid; what `Entity` and `World` are | Owner | §§2, 4 |
| D-SUB1 | `#Subsystem(Context)` syntax and `world.subsystem<T>()` lookup API | Ballot | §4 |
| D-SUB2 | No-std static-allocation subsystem variant | Ballot | §4 embedded |
| D-SLOT1 | `#Slot` / `#Contributes` spelling and typed-trait registration mechanism | Ballot | §4 |
| D-SLOT2 | Slot dispatch — static table (compiler-generated) or runtime registry | Ballot | §4 codegen |
| D-PHASE1 | Init phase names, ordering semantics, cycle detection | Ballot | §4 |
| D-DATA1 | `data` keyword vs annotated `struct` — worked examples of both required | Ballot | §5 |
| D-DATA2 | `#Bundle` — field-level or block-level grouping | Ballot | §5 |
| D-DATA3 | `DataId<T>` type design — stable Type:Name addressing | Ballot | §5 |
| D-DATA4 | Compile-time vs runtime ID resolution | Ballot | §5 |
| D-DATA5 | Data inheritance — field-defaulting semantics, no OOP behavior | Ballot | §5 |
| D-NURSERY1 | Taskgroup/nursery primitives — prerequisite for all v2 combinators | Owner (FR-3) | §6 |
| D-PKG1 | Package tier names — Runtime/Developer/Tool or different | Ballot | §7 |
| D-PKG2 | `pub use` transitive re-export syntax — not covered by S16; new ballot | Ballot | §7 |
| D-PKG3 | Structural hierarchy enforcement — compiler, jetpack CLI, or both | Ballot | §7 |
| D-PKG4 | `contributes:` field in pack.jet — typed trait reference, connection to D-SLOT1 | Ballot | §4 + §7 |
