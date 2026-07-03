# Memory model v3.1: progressive ceremony, adversarially hardened

**Status:** PROPOSAL — owner gate **D-MEM1** (Tower card #187). No code until ratified.
**History:** v1 One-Home (rejected: hidden magic, thin expert story) → v2 completed
sigils (rejected: `from` clauses, `~` frequency) → v3 progressive ceremony → **v3.1:
v3 run through an adversarial review** (veteran systems/memory engineer persona; owner
directive 2026-07-03). Three attacks landed and changed the design; the pass is
recorded at the bottom.

---

## The idea

Rust charges every program the kernel's ceremony price. Jet inverts it:

> **Safety is constant. Ceremony scales with how much control the code claims —
> and the default claim is zero.**

- **Tier 0 — quiet (≈99% of code):** no sigils. Values, methods, resources.
- **Tier 1 — loud (rare, visible):** `~` non-receiver mutation, `^` gives,
  `copy`, `Shared`, `Pool`.
- **Tier 2 — claimed (opt-in, scoped):** per-type and per-module control:
  `@NoImplicitCopy`, `policy` floors, arenas, `uninit`, `*T`, `#Unsafe`.

## Tier 0 — the quiet tier

The beginner story, complete:

1. **Your values are yours.** Nobody else's writes can ever reach a value you
   hold. No aliasing exists in safe code.
2. **Code acts on its subject.** A method changes the thing before the dot
   (`player.heal(10)`, `file.close()`). A function changes an argument only
   when the call says so (`swap(~a, ~b)`). Everything else is read-only to it.
3. **Sharing is asked for by name.** `Shared<T>` — one value, many doors.
   `Pool<T>` — many values, stable ids. Nothing else aliases.

```jet
fn strongest(party: Party) -> Str {
    best := party.members[0]
    loop m in party.members { if m.hp > best.hp { best = m } }
    best.name
}

fn main() {
    kai := Player.{ name: "Kai", hp: 90 }
    party := Party.{ members: [] }
    party.members.push(kai)            // party stores its own Kai
    kai.hp = 40                        // yours; party's untouched
    party.members[0].hp += 10          // place path, direct
    print(strongest(party))

    save := fs.create("save.txt")?
    save.write("{party}")?
    save.close()                       // resource used up; later use = error
}
```

**Functions DO things.** The design does not neuter procedures — it makes the
*target* explicit. Three ways to mutate, none exotic:

```jet
fn Player.heal(self, amount: Int) { self.hp += amount }   // subject: free
kai.heal(10)

fn transfer(from: ~Account, to: ~Account, amount: Money)   // multi-target: marked
transfer(~checking, ~savings, 50)

fn parse_into(src: Str, out: ~Ast)                         // out-param: marked
```

Any function whose primary object is being mutated *is* a method — and methods
can be written externally (`fn Type.method(self)`, D-EXTMETH1), so this costs
no restructuring. Measured against real APIs, the overwhelming majority of
mutating functions mutate exactly one primary object; the multi-target tail is
`swap`/`transfer`-shaped and gets one character per argument. This is Swift's
`inout`/`&` rule, which nobody describes as "functions can't do anything."

**Semantics of a handoff:** *as-if* copy. Compiler lowering, in priority order:
move (arg dead after the call — the majority), borrow (callee only reads),
memcpy (small POD), CoW snapshot (heap-backed, still-live: share the buffer,
bump a count; the *writer* pays a real copy only if it writes while shared).
Drops: scope-end RAII (S63). **User-visible destructors exist only on
`@Resource` types, which are move-only and never snapshotted — so destructor
points are exactly as static as Rust's.** Plain-data frees are memory-only;
timing is the optimizer's.

**Receiver mutation is inferred and displayed, not typed:** ✎ (mutates) /
⌫ (consumes) badges in `jet doc` and LSP hover/inlay; recorded in published
capability metadata; `jet publish` diffs it (a method that starts mutating is
semver-major). `@Pure fn` (S60) is the checked source-level claim when an
author wants the guarantee in text. Safe because the danger `&mut self` guards
in Rust — aliased mutation — is unrepresentable here.

## Tier 1 — the loud tier

| op | spelling | notes |
|---|---|---|
| mutate a non-receiver arg | `swap(~a, ~b)` | declaration + call site; statement-bound lend, never storable, never bindable |
| give a value away | `channel.send(^packet)`; `fn close(^self)` | visible move; later use errors at *your* `^`. Declaration-side only for receivers |
| explicit duplicate | `copy x` | required only where policy/`@NoImplicitCopy` demands |
| shared mutable identity | `Shared<T>` | the only "many doors to one value"; edit/read access blocks |
| stable many-object graphs | `Pool<T>` + `Id<T>` | ids are plain data; the ECS answer, first-class |

There is **no reference, view, or borrow that can be stored** — v3's `&` hold
is deleted (adversarial finding F3 below). Zero-copy sharing of immutable data
is what plain assignment already does; visible-update sharing is `Shared`;
everything else was a footgun.

The complete static rule set a user can ever trip over:

1. A `~` lend may not overlap another access to the same place in one
   statement (`swap(~a, ~a)`; `enroll(~party, party.members[0])` — hoist to a
   local, the error says how).
2. A resource may not be used after its `^` (error quotes the consuming line).
3. That's all. No borrow errors, no lifetime errors, no dangling — and **no
   runtime ownership checks anywhere** (v3's checked-exclusivity tier is gone;
   everything is static).

## Tier 2 — the claimed tier

Control is claimed per *type* and per *module*:

```jet
@NoImplicitCopy                       // this TYPE never silently copies or counts:
struct Image { pixels: [U8] }         // a handoff that can't move/borrow = compile
                                      // error at that line ("add copy, or restructure")

// per module / package:
policy no_implicit_copy               // every handoff must lower to move/borrow;
                                      // anything else = compile error (Rust discipline)
policy no_count                       // no refcounts may survive (embedded ABI floor)
policy no_alloc                       // arenas/static only (kernel, hot loops)
```

**A policy is not a dialect.** Semantics are identical everywhere — *as-if*
copy; a policy only converts silent lowering choices into required explicit
ones. Moving code between modules can never change what it means, only whether
it still compiles there.

Floor unchanged: `region`/arenas (D-REGION1/ALLOC1-2), `:= uninit`
(D-UNINIT-SENTINEL1), `*T` + `#Unsafe("reason")` (S58/D-CAP9), layout /
volatile / FFI via `core.mem`.

## "Isn't copying everywhere terrible for memory?"

No — because the copies are *logical*. Physically:

- A CoW snapshot shares the buffer: **footprint ≈ Rust + one count word per
  heap allocation.** An eager-copy design would be terrible; this isn't one.
- The majority of handoffs never even count: last-use moves and read-only
  borrows are resolved statically (Perceus-class elision; whole-program
  compilation makes this strong).
- The two *real* memory risks are named and armed, not hidden:
  - **Latency cliff** — writing a 100 MB value while snapshotted pays the copy
    at the write. Armed: `@NoImplicitCopy` on exactly the types where that's
    catastrophic (Image, Buffer, World) makes the silent path a compile error
    program-wide; `jet build --explain-copies` lists every possible deferred-
    copy site; `jet dev` traces copy events live.
  - **Retention** — a held snapshot keeps a big buffer alive. Armed: same
    marker, same tooling; resources can't be snapshotted at all.
- Aggregate handoff cost (a struct of 10 `Str`s = up to 10 count bumps when it
  can't move — Swift's ARC-traffic problem) is real. Armed: move-first
  lowering, borrow inference, and the ratification gate below.

**Ratification gate (owner-visible exit criteria):** before any build, a
benchmark suite — game tick, JSON parse/transform, HTTP echo server, allocation
churn — must land within an owner-set envelope of the equivalent Rust under the
*default* tier, and match it under policy modules. If the default tier can't
hit the envelope, D-MEM1 reopens with data. No vibes-based perf claims.

## Against Rust, head to head

**1. Use after insert — the beginner-killer**
```rust
party.members.push(kai);
println!("{}", kai.name);      // error[E0382]: borrow of moved value: `kai`
```
```jet
party.members.push(kai)
print(kai.name)                // fine
```

**2. Returning a view**
```rust
fn domain<'a>(email: &'a str) -> &'a str { … }
```
```jet
fn domain(email: Str) -> Str { email.after("@") }   // snapshot slice; zero-copy
```

**3. Struct holding text — the lifetime infection**
```rust
struct Parser<'a> { src: &'a str, pos: usize }      // 'a infects every user
```
```jet
struct Parser { src: Str, pos: Int }                // snapshot; zero-copy; no sigil at all
```

**4. Shared mutable config + tasks**
```rust
let config = Arc::new(Mutex::new(Config::default()));
let c = Arc::clone(&config);
thread::spawn(move || { c.lock().unwrap().volume = 0.5; });
```
```jet
config := Shared.new(Config.{})
tasks.spawn(() => config.edit(c => c.volume = 0.5))
```

**5. Mutate while iterating**
```rust
for m in party.members.iter_mut() {
    if m.hp == 0 { party.log_death(m); }   // E0499 second mutable borrow
}
```
```jet
loop m in party.members {
    if m.hp == 0 { party.log_death(m) }    // error only if log_death edits members —
}                                          // said in English with the collect-after fix
```

**6. Multi-target mutation — matched, not conceded**
```rust
mem::swap(&mut a, &mut b);
```
```jet
swap(~a, ~b)
```

**7. Graphs — where Rust sends you to crates**
```rust
let a = Rc::new(RefCell::new(Node::default()));
a.borrow_mut().next = Some(Rc::clone(&b));          // runtime borrow panics await
```
```jet
world := Pool<Node>.new()
a := world.add(Node.{})
world[a].next = world.add(Node.{})
```

**8. Hot path — Rust's turf**
```jet
// sim/: policy no_implicit_copy no_alloc
fn tick(world: ~World, dt: F32) {
    loop e in world.entities { e.pos += e.vel * dt }
}   // identical machine code, enforced; the other 20k lines pay nothing
```

Scorecard: same guarantees (memory-safe, race-free, no GC, deterministic
destruction of resources, zero-cost reachable and *enforceable*), minus
lifetimes, minus invisible moves, minus wrapper-type assembly kits, minus
global ceremony.

## APIs

```jet
pub fn parse(src: Str) -> Json            // cannot change or consume src — by rule
pub fn merge(base: ~Json, patch: Json)    // the rare visible edit-arg
pub fn Json.compact(self) -> Json         // ✎/⌫/pure badges: inferred, rendered, semver-diffed
pub fn Sink.send(self, packet: ^Packet)   // visible take
@NoImplicitCopy pub struct Image { … }    // perf contract is part of the type
```

Unmarked = complete contract, zero annotations. Behavioral metadata (mutates /
consumes / pure / copy-class) is inferred, shown by `jet doc`/LSP, recorded at
publish, and diffed — sigil or badge changes require a major version.
A `no_alloc`/`no_count` package advertises its floor; embedded consumers can
require it.

## Domain fit

- **CLI / scripts / web handlers / data:** tier 0 end to end; sigils absent.
- **Servers:** tier 0 + `Shared` sessions + tasks (values crossing tasks are
  snapshots/moves — races unrepresentable; counts on crossing types are atomic,
  chosen by whole-program escape analysis, non-atomic elsewhere).
- **Games:** tier 0 gameplay; `Pool` worlds; `@NoImplicitCopy` on big assets;
  `policy` sim/render; per-frame arenas.
- **Embedded / kernel:** package-wide `no_alloc no_count` + `@NoImplicitCopy`
  defaults + tier-2 floor (`uninit`, `*T`, volatile, MMIO). Full static
  discipline, zero counts, zero heap — enforced, not hoped.
- **Libraries:** tier-0 internals, loud surface, diffed metadata.

## Adversarial pass (veteran systems/memory engineer) — record

| # | attack | verdict | disposition |
|---|---|---|---|
| F1 | "Copies everywhere = memory blowup" | **deflected** | logical copies, physical sharing; footprint ≈ Rust + count word/allocation; eager-copy strawman ≠ this design |
| F2 | "CoW latency cliffs + retention are 3am bugs Rust prevents by construction" | **landed** | `@NoImplicitCopy` per-type marker added (compile error at any silent copy/count of marked types, program-wide); `--explain-copies` + `jet dev` copy tracing; policy floors for whole modules |
| F3 | "Your `&` hold is incoherent: live view vs snapshot unspecified; if live, writer+holder needs runtime exclusivity checks = implicit RefCell panics" | **landed, fatally for `&`** | `&` holds **deleted** from the safe tier. Plain assignment already gives zero-copy immutable sharing (CoW); visible-update sharing is `Shared` by name. With no storable views, *every* remaining check is static — the runtime-checked exclusivity tier is gone from the design |
| F4 | "Functions that can't mutate args are useless" | **deflected, wording landed** | rule restated: code acts on its *subject* for free (methods, external-method syntax); non-subject mutation is one char (`transfer(~a, ~b)`); Swift precedent; v3's "no function can change what you pass" overclaim retired |
| F5 | "Unmarked mutating receivers = signatures lie where it matters most (review diffs without LSP)" | **held as explicit trade** | badges/metadata/semver-diff + `@Pure`; option B (declared `~self`) preserved on the ballot as the const-correct variant; owner picks the axis |
| F6 | "ARC traffic on aggregate handoffs (N count bumps per escaping struct)" | **landed** | move-first lowering + borrow inference acknowledged as necessary-not-sufficient; **benchmark ratification gate** added — default tier must hit an owner-set envelope vs Rust on real workloads or D-MEM1 reopens |
| F7 | "Refcounts and threads: atomic tax or unsoundness" | **deflected** | whole-program escape analysis: atomic only for types that actually cross task boundaries; conservative fallback atomic; unsoundness impossible (counts are compiler-emitted, never user-visible) |
| F8 | "Deterministic destruction dies under CoW" | **deflected** | destructors exist only on `@Resource` types; resources are move-only and never snapshotted; drop points as static as Rust. Plain-data frees have no user-visible timing |
| F9 | "Policy modules fragment the language into dialects" | **deflected** | semantics identical everywhere; policy converts silent lowering into required explicitness; moving code changes compilability, never meaning |
| F10 | "FFI: CoW containers can't cross a C boundary" | **deflected** | `#Unsafe` tier forces uniqueness (`make_mut` class) before exposing pointers; `no_count` modules use arenas/fixed arrays — the embedded story already |

Net design changes from the pass: `&` deleted (safe surface is now `~` and `^`
only, both rare); all ownership checking static; `@NoImplicitCopy` added;
atomicity by escape analysis; benchmark gate added; mutation rule restated.

## What this deletes / keeps (vs ratified state)

**Deleted if A:** D-CAP8 elevation/freeze + `api: explicit`; L0201 (implicit
copy is defined semantics, surfaced and bannable); D-REF-SHORTHAND1/2 + `#Ref`
+ E0207/E0427 (no storable views exist to infer owners for); `&T` as share
param *and* as stored ref (glyph reserved, unminted in the safe tier); routine
call-site mirroring (survives only on `~` args and `^` gives).
**Kept:** `~`/`^`/`*` glyph meanings (D-CAP7, collapsed frequency); `copy`
(D-CAP2); D-MUTSELF1 receiver-on-self declarations (`^self`; `~self` under
option B); S63 RAII; #SingleUse → `@Resource`; arenas/`uninit`/`*T`/`#Unsafe`;
S53/D-DETACH1 posture; D-CAP5 metadata (extended with badges).
**New (follow-on ballots if A):** `@NoImplicitCopy`; `policy no_implicit_copy |
no_count | no_alloc`; `Shared<T>`/`Pool<T>` APIs; `@Resource` naming; badge
voice; `--explain-copies` voice; rare-mutation marker spelling menu
(`~x` / `mut x` / `inout x` / `x!`).

## D-MEM1 (v3.1) — the decision

**A — Progressive ceremony, hardened (recommended).** Everything above. Safe
surface: no sigils except rare `~`/`^`; no storable views; all checks static;
per-type and per-module control; benchmark gate before build.

**B — A + declared mutating receivers (`fn heal(~self)`).** Const-correct
source at the price of `~` on every mutating method (its highest-frequency
site). Call sites unchanged. Badge machinery identical.

**C — v2 completed sigils** (explicit everywhere + `from` clauses). On record;
already disliked.

**D — Status quo** (D-CAP7/8 + D-REF-SHORTHAND, six diagnosed inconsistencies).

Recommendation: **A**, with B the live alternative if source-level const-
correctness outweighs sigil frequency.
