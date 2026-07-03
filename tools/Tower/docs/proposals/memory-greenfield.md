# Memory, greenfield: the One-Home model

**Status:** PROPOSAL — owner gate **D-MEM1** (Tower card #187). No code until ratified.
**Supersedes if adopted:** the user-facing half of the sigil capability model
(D-CAP7 `&T` share, D-CAP8 freeze scope, D-REF-SHORTHAND1/2 stored refs, L0201).
The expert tier (S58 `#Unsafe`/`*T`, D-REGION1 arenas, D-UNINIT-SENTINEL1, S63 RAII)
is untouched and load-bearing here.

---

## Why reopen this

The sigil model is Rust's ownership question with friendlier spellings. The user
still answers "view, edit, take, or share?" at every function boundary, mirrors
the answer at every call site, hits use-after-move errors on plain data, and — since
D-REF-SHORTHAND — reasons about stored references and owner-inference ambiguity
(E0207). Renaming the borrow checker doesn't remove it. The clunkiness is the
question itself.

C asked programmers to manage memory and punished mistakes with UB.
Rust made mistakes impossible but kept the management: you *prove* ownership to
the compiler. The jump left on the table: **make the proof the compiler's job**.
Safety stays absolute; the user-facing concept count drops to almost zero;
annotations become *performance contracts experts may add*, never permission
slips anyone must write.

## Glossary

- **place** — a named location a value lives in: a variable, a field, an element. `party.members[2].hp` is a place.
- **value** — the data in a place. Values are independent: yours is yours.
- **handoff** — giving a value to a function, struct, collection, or task.
- **resource** — a value that is "used up": file, socket, lock. The only kind with consume semantics.
- **CoW** — copy-on-write: a logical copy that shares storage until someone writes; then it physically copies. One refcount, no GC, deterministic.
- **elision** — the compiler proving a copy/refcount unnecessary and deleting it (move on last use, borrow on read-only use).

## The model — five rules, the whole thing

1. **One home.** Every value lives in exactly one place. Memory is a tree, never
   a graph. Nothing aliases in safe code, ever.
2. **Handoffs give a copy — logically.** After `party.add(player)`, `player` is
   still yours and untouched. The compiler makes the handoff a move, a borrow, or
   a CoW snapshot under the hood; meaning never changes, only cost.
3. **Mutation is visible.** Only a call marked `~` can change your value:
   `heal(~player)`. Unmarked calls provably cannot. (One exclusivity law: a place
   can't be handed `~` twice in the same call, or read while being `~`-handed —
   checked locally, since nothing stores references.)
4. **Resources are used up.** `close(file)` consumes; using `file` after is a
   compile error naming the consuming call. Only resource types (`@Resource`,
   today's #SingleUse machinery) ever produce this error. Plain data never does.
5. **Sharing has a name.** One value visible from many parts of the program is a
   thing you ask for: `Shared<T>` (one value, many doors) or `Pool<T>` (many
   values, stable ids). Nothing else in the language can alias.

The complete beginner curriculum is rules 1–3. Two sentences pass the
philosophy.md C1 bar: *"Your values are your own; a call that changes one is
marked `~`. Files and sockets are used up when you hand them over."*

Deleted from the user's world: use-after-move on data, view/edit/take/share
choice, `&T`, stored-reference owner inference, L0201 implicit-clone warnings,
lifetimes (still), and the borrow checker as a thing to fight. The one genuinely
new error is the exclusivity conflict, and it reads like an English sentence.

## Before / after

Today (sigil model):

```jet
fn add_member(party: ~Party, player: ^Player) { party.members.push(player) }

add_member(~party, ^player)
print(player.name)          // error: player was taken by add_member
                            // fix: copy player / &player / reorder
```

One-Home:

```jet
fn add_member(party: ~Party, player: Player) { party.members.push(player) }

add_member(~party, player)
print(player.name)          // fine. player is yours. always.
```

Compiler, invisibly: if `player` were dead after the call it moves (zero cost);
alive here, so — `Player` heap-backed → CoW snapshot (one refcount bump; physical
copy only if either side later writes); small POD → memcpy.

### Sharing that used to need `&T`

```jet
texture := load("hero.png")
sprite_a := Sprite.{ texture }     // each sprite owns its texture — logically
sprite_b := Sprite.{ texture }     // physically: one buffer, three counted owners
```

Nobody writes textures, so this compiles to exactly what `&Texture`/Arc was:
one allocation, counted. Zero concepts spent.

### Mutation, iteration, projection — no references needed

```jet
party.members[3].hp += 10          // place expression, direct

loop m in ~party.members {         // mutable iteration over places
    m.hp += 1
}

swap(~a, ~a)                       // error[E09xx]: `a` handed to `swap` as
                                   // changeable twice in one call
                                   // what: two exclusive writers would overlap
                                   // fix: pass two different places
```

### Resources — linearity only where it pays

```jet
file := fs.open("log.txt")?
file.write("hi")?
file.close()                       // consumes (fn close(^self))
file.write("again")?               // error: file was closed by `close` (line 3)
```

### Slices and views — values, not borrows

```jet
fn domain(email: Str) -> Str { email.after("@") }   // counted view, zero copy
```

A `Str` slice is a snapshot value backed by the same counted buffer. Safe because
it's immutable; no lifetime exists because nothing borrows.

### Graphs — the pool is the paved road

Trees are free (rule 1: memory is a tree). Actual graphs — entities that
reference each other — get first-class handles instead of borrowed pointers:

```jet
players := Pool<Player>.new()
kai := players.add(Player.{ name: "Kai" })   // kai: Id<Player>, Copy, stable
target := players.add(Player.{ name: "Rem" })
players[kai].target = target                  // ids stored freely, no lifetimes
players.remove(target)
players[kai].target                           // stale id: caught access (Option / checked)
```

This is where Rust users end up anyway (slotmap/ECS) after losing to the borrow
checker; Jet ships it as the blessed answer with real syntax support (place
expressions through `pool[id]`). Also the Blueprint/gameplay story's natural
substrate.

### True shared mutable — rare, loud, safe

```jet
config := Shared.new(Settings.{ volume: 0.8 })   // one value, many doors
tasks.spawn(() => {
    config.edit(s => s.volume = 0.5)             // exclusive inside the block
})
print(config.read(s => s.volume))
```

Lowered to `Arc<RwLock>`; the only cross-thread mutability in the language,
which keeps S53's "no shared mutable state by accident" intact.

## The expert tier: annotations are contracts, not permission

Everything above compiles with zero annotations. Experts *tighten*:

| lever | meaning | enforcement |
|---|---|---|
| `~T` | may mutate caller's value | already the visibility rule |
| `^T` | consumes; caller may not reuse | on resources: semantic. on data: **zero-copy guarantee** — compiler errors if it would need a copy |
| `copy x` | force an eager independent copy now | D-CAP2, unchanged |
| `@Unique` binding/field | statically one owner, no counts ever | compile error on violating handoff |
| `policy no_implicit_copy` (module/pkg) | any surviving implicit copy or refcount is a compile error at the handoff | Rust-strictness, opt-in, scoped |
| arenas / `region` | allocation control | D-REGION1/ALLOC1-2, unchanged |
| `buffer: [U8#4096] := uninit` | skip zero-fill, proof-checked | D-UNINIT-SENTINEL1, unchanged |
| `*T` + `#Unsafe("...")` | raw pointers, layout, FFI | S58/D-CAP9, unchanged |
| `jet build --copies` | list every surviving implicit copy/refcount with reason + fix | visibility instead of mandatory annotation |

The inversion that makes this revolutionary rather than incremental: in Rust,
annotations are the *price of compiling*. Here the program always compiles and
is always safe; annotations only ever **add guarantees**. A `policy
no_implicit_copy` module is exactly as strict as Rust — so Rust's model still
exists inside Jet, demoted to an opt-in lint profile for the 5% of code that
wants it. Nothing is lost; a game's hot loop demands proof, the other 20k lines
never think about ownership at all.

Full-capability check (nothing Rust can express is lost):

| Rust | Jet One-Home |
|---|---|
| `&T` param | unmarked param (inferred borrow) |
| `&mut T` | `~T` |
| move | inferred; forced/guaranteed by `^T` |
| `Box<T>` | invisible (recursive types auto-boxed) |
| `Rc`/`Arc` | invisible CoW; `Shared<T>` when identity matters |
| `Mutex`/`RefCell` | `Shared<T>` edit blocks |
| `&'a str` slices | counted view values |
| lifetimes | do not exist (nothing stores borrows) |
| `Cow<T>` | the default |
| self-referential/`Pin` | `Pool` ids, or `*T` tier |
| `unsafe` | `#Unsafe` tier, unchanged |

## How the compiler does it (sema machinery, all front-side per I3)

1. **Access inference** per param: read / write / consume — the D-CAP8 infer
   tier, kept as-is internally.
2. **Last-use analysis** per place: a handoff at last use lowers to a move.
   Most handoffs in real code are last uses — zero cost.
3. **Read-only handoffs** lower to `&T`. `~` lowers to `&mut T`.
4. **Escaping handoff with live caller value:** small POD → memcpy; heap-backed →
   CoW snapshot (`CowBox` in the jet runtime: Arc + make_mut). Perceus-grade
   static uniqueness analysis deletes counts it can prove; non-atomic counts
   unless the value provably crosses a task boundary.
5. **Drops:** scope-end RAII (S63), every exit path; optimizer may release
   non-observable values earlier.
6. **Exclusivity check:** overlapping-place analysis per call/statement. Local
   and complete precisely because no references are storable — the whole reason
   Rust needs lifetimes is gone, not hidden.

rustc remains the hidden verifier (I2); everything above is checked in sema and
lowered to boring safe Rust plus the vetted `CowBox` internals (I1).

## The honest cost (priority 3, faced squarely)

| situation | cost |
|---|---|
| handoff at last use | zero (move) |
| read-only handoff | zero (borrow) |
| handoff that escapes, caller still uses it, heap-backed | one refcount bump; physical copy only on later write-while-shared |
| same, small POD | memcpy |
| `policy no_implicit_copy` code | zero by construction — anything else is a compile error |

No GC, no pauses, no tracing, fully deterministic. But a surviving refcount
bump is real, and philosophy.md #3 says "no runtime overhead to buy
simplicity." The ranked-priority argument for accepting it: the overhead is
(a) elidable and statically elided in the common paths, (b) confined to the
exact spot where the *user's program semantics* demand a logical copy, and
(c) removable to literally-Rust levels by opt-in policy exactly where it
matters. Priorities 1 and 2 outrank 3, and the expert tier keeps 3 fully
reachable. Ratifying D-MEM1=A includes signing this reading of priority 3;
D-MEM1=B below is the strict reading.

Swift ships this machinery on every iPhone; Koka/Perceus and Lobster prove the
elision tech; Mojo proves inferred ownership on a systems language. The
synthesis — plus contracts-not-permission and pools-as-paved-road — is the
greenfield part.

## Lessons taken

- **Swift/Hylo:** mutable value semantics + CoW is mass-market-proven beginner-safe; visible `&x`-style mutation marks are liked, not tolerated.
- **Koka (Perceus), Lobster:** compile-time RC elision makes "everything is a value" near-zero-cost in practice.
- **Mojo:** ASAP/last-use ownership inference on an imperative surface works.
- **Rust:** exclusivity is the safety core worth keeping; mandatory proof is the adoption killer worth dropping. RAII/linear resources are the good part — scope them to resources.
- **Pony (capabilities):** vocabulary without removing the question doesn't lower the curve — the lesson of our own D-CAP arc.
- **Vale:** generational indices are gold — as library-level pool handles, not as the pointer model.
- **ECS practice:** when a paradigm's users converge on a workaround (slotmap), ship the workaround as the paradigm.

## Considered and rejected

- **Tracing GC** — priority 3, determinism, embedded/kernel targets. Dead on arrival.
- **Generational references as the core model (Vale)** — keeps pointer/aliasing mental model (spooky action at a distance stays), failure mode is a runtime panic not a compile error, per-deref checks tax the default path. Its good half survives as `Pool` ids.
- **Capability vocabulary (current)** — the question survives renaming; call-site mirroring spreads it to every line.
- **Pure functional + Perceus (Roc/Koka)** — collides with Jet's imperative surface; take the runtime tech, not the semantics.
- **Full region inference (MLKit)** — region-bloat pathologies; regions stay the expert lever they already are.

## What this supersedes / keeps (if D-MEM1 = A)

**Dies:** `&T` share sigil (D-CAP7 shrinks), stored-ref fields + owner inference
(D-REF-SHORTHAND1/2, E0207/E0427), L0201, share-vs-edit API metadata,
freeze-at-API for anything but `~`/`^` (E0912 scope shrinks).
**Survives untouched:** `~` mutation sigil + call-site mirror (D-CAP3/D-MUTSELF1),
`^` consume (rescoped: resources + zero-copy contract), `copy` verb (D-CAP2),
`*T`/`#Unsafe` (S58/D-CAP9), arenas/regions, `uninit`, S63 RAII, #SingleUse
(becomes the `@Resource` substrate), S53 concurrency direction (strengthened —
values crossing tasks are safe by rule 2).
**New surface (each its own follow-on ballot if A ratifies):** `Shared<T>`,
`Pool<T>`/`Id<T>`, `@Unique`, `@Resource` spelling, `policy no_implicit_copy`,
`--copies` diagnostics voice.

Sigil count drops from five to four; error classes drop by three; the beginner
curriculum drops to two sentences.

## Name menu (owner picks; used in docs/diagnostics voice)

- **Fly-by-Wire** — pilot states intent, computer moves the control surfaces; direct-law mode still exists for experts. (Closest metaphor to the actual design.)
- **One-Home** — the teaching rule itself as the name.
- **Autotrim** — the compiler continuously trims ownership so the airframe flies hands-off.
- **Slipstream** — values move in the wake of the program with no drag from annotations.
- **Feather** — props auto-feather; copies auto-elide.
- **Glidepath** — beginners stay on the glideslope without touching the controls.

## D-MEM1 — the decision

Same program under each option: `add_member(party, player)` then use `player`.

**A — One-Home model (recommended).** Everything above.
```jet
add_member(~party, player)
print(player.name)            // fine; compiler chose move/borrow/CoW
```

**B — One-Home, eager copies only.** Rules 1–5 identical, but no CoW/refcounts
ever: an escaping live handoff is a real deep copy (warning-visible), `copy` to
silence. Strictest priority-3 reading; predictable; big-value handoffs cost
real memory and the beginner perf story worsens (accidental O(n) copies).
```jet
add_member(~party, player)    // deep-copies player here, every time
```

**C — Generational references.** Familiar pointer semantics; every value
reachable by reference, deref checked against a generation at runtime; regions
elide checks in hot code. Aliasing mental model returns; errors move to runtime.
```jet
p := &player                  // references first-class again
party.add(p)
player.hp = 0                 // p sees it — spooky action returns
```

**D — Status quo.** D-CAP7/8 sigil capabilities stand as ratified; this
proposal archives as design history.
```jet
add_member(~party, ^player)
print(player.name)            // error: player was taken — today's world
```

Recommendation: **A.** B is A with the best beginner-perf machinery removed for
a purity point the expert tier already answers; C re-imports the aliasing
confusion this proposal exists to delete; D is the clunk the owner is reacting
to.
