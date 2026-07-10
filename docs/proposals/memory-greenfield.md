# Memory model v5: the borrow checker, humanized

**Status:** **RATIFIED — D-MEM1 = A, 2026-07-03** (Tower card #187). Implementation
plan: [../plans/memory-v5-migration.md](../plans/memory-v5-migration.md).
**Owner directive (2026-07-03, final reset):** use the Rust borrow checker system;
make it much more ergonomic and beginner-friendly; read-only by default; sigils not
words; no `~`; prefer `&`/`^`. Earlier explorations (v1 value semantics, v2 explicit
sigils + from-clauses, v3 progressive ceremony, v4 identity objects) are dead; kept in
git history only. If the owner approves v5, the next deliverable is the migration path
from the current implementation.

**Usage gallery:** [memory-v5-gallery.md](memory-v5-gallery.md) — 13 side-by-side
Jet-vs-Rust examples across scripting, game, web, concurrency, graphs, low-level.

---

## The thesis

Rust's semantics are right: read-only by default, one writer at a time, ownership
with deterministic cleanup, no garbage collector. That core is proven and Jet keeps
it unchanged.

Rust's *experience* fails beginners in five specific, well-documented ways. Each has
a targeted fix that does not weaken the semantics:

| Rust pain | cause | Jet fix |
|---|---|---|
| lifetime annotations (`<'a>` soup) | borrows are storable/returnable first-class values | borrows are **second-class**: they flow *down* into calls, never stored in structs, never returned raw. Lifetimes then need no names — they're all obvious to the compiler. View-shaped APIs are served by view *values* (below) |
| invisible moves (`push(kai)` → E0382 three lines later) | moves are silent at call sites | moves are **always written**: `push(^kai)`. A later use errors pointing at the `^` *you typed* |
| errors describe the checker, not the fix | diagnostics grew out of the implementation | every borrow error is a teaching diagnostic with a fix menu (what/why/fix, I4-pinned) |
| walls need wrapper-assembly (`Rc<RefCell<…>>`, `Arc<Mutex<…>>`, slotmap crate) | escape hatches are library type-tetris | escape hatches are **named language-level types**: `Shared<T>`, `Pool<T>`. Errors suggest them by name |
| `String` vs `&str`, `Vec<T>` vs `&[T]` dualism | exposed borrow types split every API in two | one `String`, one `[T]` list; cheap views are internal (counted slices), invisible in signatures |

Everything else about the checker — exclusivity, move analysis, RAII — is kept
verbatim. This is Rust with the sharp edges machined off, not a new model.

## The surface — three sigils

| sigil | param | call site | meaning |
|---|---|---|---|
| *(none)* | `player: Player` | `f(player)` | **read** — borrow, look only. The default everywhere. Signature can never lie: unmarked params are untouchable, period. No elevation, no inference surprises |
| `&` | `player: &Player` | `f(&player)` | **write** — exclusive mutable borrow for the call. One writer at a time, enforced |
| `^` | `item: ^Item` | `f(^item)` | **take** — ownership moves; caller's binding is over, visibly |

`*T` stays the raw/`#Unsafe` tier, unchanged. `~` is gone. No word-spellings
(`mut`/`inout`/`take` remain non-keywords). Receivers declare on self
(`fn heal(&self)`, `fn destroy(^self)`); call sites stay `kai.heal()` — the
subject is visible left of the dot (D-MUTSELF1, kept).

```jet
fn strongest(party: Party) -> String     // read
fn heal(player: &Player, amount: Int)      // write
fn enroll(party: &Party, player: ^Player)  // write party, take player

heal(&kai, 10)
enroll(&party, ^kai)                       // both effects visible at the call
```

Locals: `x := f()` owns; `x := y` moves iff `y` is written `^y`, otherwise it's an
error for heap data with a fix menu (`copy y` / `^y`), and a plain copy for scalars
and small POD — same rule Rust's `Copy` draws, but the failure mode is a fix menu,
not folklore. `copy x` (D-CAP2) is the one spelling for duplicating heap data.
`^` marks giving away a *named binding*; temporaries — literals, call results,
`copy x` — pass without it, since nothing survives to be used-after.

## What beginners never see

- **No lifetime syntax, ever.** Second-class borrows make every borrow's extent
  obvious to the compiler (down-the-stack only). There is nothing to annotate;
  there is no annotation to learn. (Precedent: this is the "second-class
  references" design — Hylo, and Graydon Hoare's stated preference for Rust
  itself — with soundness literature behind it.)
- **No stored borrows.** A struct field is an owned value, a `Shared<T>`, or an
  `Id<T>` — never a borrow. The single biggest lifetime-infection source in Rust
  (struct `<'a>`) cannot be expressed, so it cannot confuse.
- **No returned raw borrows.** Functions return owned values or *views* — library
  value-types that internally share storage (a `String` slice is a counted view; so
  are `[T]` slices). `text.trim()`, `email.after("@")` are zero-copy and their
  signatures are just `-> String`. No `&str`-vs-`String` split exists to explain.
- **No invisible moves.** Every transfer is `^` at the call site.
- **No aliased mutation.** As in Rust — but the error is a story with a fix menu,
  and the escape hatches have names.

The complete beginner rule set: **"unmarked = looking, `&` = changing, `^` =
giving away; one writer at a time."** One sentence.

## The classic walls, with the new answers

**Wall 1 — use after giving away**

```jet
party.members.push(^kai)
print(kai.name)
```
```text
error[E—]: kai was given away on line 1 (`^kai`)
what: `^` hands ownership to the list; kai's name no longer holds a value
fix:  keep a copy:      party.members.push(copy kai)
      or read it first: print(kai.name) before the push
      or one player, referenced from many places: use Pool<Player> (jet explain pools)
```

The kai question from review, answered honestly under borrow semantics: *"one kai
in the game, in the party, and I change his hp"* — one kai means one owner plus
references, and the checker's whole job is making that explicit. Three spellings,
all visible:

```jet
// (a) party owns kai; edit him where he lives
party.members.push(^kai)
heal(&party.members[0], 10)

// (b) the world owns players; party holds ids — the game-shaped answer
kai := world.players.add(Player.{ name: "Kai", hp: 90 })   // kai: Id<Player>
party.members.push(kai)                                     // ids are plain data
heal(&world.players[kai], 10)                               // one kai, everywhere

// (c) truly shared, editable from many holders
kai := Shared.new(Player.{ name: "Kai", hp: 90 })
party.members.push(kai)
kai.edit(p => p.hp -= 50)
```

**Wall 2 — iterate and mutate**

```jet
loop m in &party.members {
    if m.hp == 0 { party.members.push(reserve()) }
}
```
```text
error[E—]: party.members is being written one-at-a-time by this loop
what: adding to the list while walking it would shift the ground under the loop
fix:  collect first:  fallen := party.members.filter(m => m.hp == 0)
      push after the loop ends
```

**Wall 3 — "cannot borrow as mutable more than once"** (Rust's E0499 voice vs Jet's)

```text
error[E—]: two writers to `party` in one expression
what: swap(&party.members[i], &party.members[j]) — both arguments write through
      party, and writers must be exclusive
fix:  party.members.swap(i, j)   (the list method does it safely)
```

**Wall 4 — parent pointers / graphs / observers** (Rust: `Rc<RefCell<…>>` or a crate)

```jet
world := Pool<Node>.new()
a := world.add(Node.{})
b := world.add(Node.{ parent: a })     // Id<Node> is plain data — store freely
world[b].parent = a
```

**Wall 5 — shared state across threads** (Rust: `Arc<Mutex<T>>` + clone + lock + unwrap)

```jet
config := Shared.new(Config.{})
tasks.spawn(() => config.edit(c => c.volume = 0.5))
print(config.read(c => c.volume))
```

Borrows can't cross `spawn` (second-class — nothing to check, it just can't be
written); tasks capture owned values, `^` transfers, copies, or `Shared`. Data
races are unrepresentable, in the type system rather than by lint.

## Rust vs Jet, side by side

```rust
// Rust: the getter that teaches lifetimes
fn domain<'a>(email: &'a str) -> &'a str { … }
```
```jet
fn domain(email: String) -> String { return email.after("@") }   // view; no 'a
```

```rust
// Rust: the struct that infects everything
struct Parser<'a> { src: &'a str, pos: usize }
```
```jet
struct Parser { src: String, pos: Int }   // view value field; no lifetime exists
```

```rust
// Rust: invisible move, delayed explosion
party.members.push(kai);
println!("{}", kai.name);   // error[E0382], three lines and one concept away
```
```jet
party.members.push(^kai)    // the ^ is the explosion, defused, at the site
```

```rust
// Rust: mutation invisible at call sites
update(&mut world);   // ok — but map.insert(k, v) mutates with no marker at all
```
```jet
update(&world)        // & required at every mutating call — reviewers see effects
```

## The expert story

Unchanged from ratified Jet, and full Rust parity:

- exclusivity + moves + RAII (S63) are the semantics — zero-cost, no runtime
  ownership machinery anywhere in this model;
- `copy` verb (D-CAP2); `@NoCopy`/#SingleUse resource linearity (`fn close(^self)`);
- arenas/regions (D-REGION1/ALLOC1-2), `:= uninit` (D-UNINIT-SENTINEL1),
  `*T` + `#Unsafe("reason")` (S58/D-CAP9), layout/volatile/FFI via `core.mem`;
- `Shared<T>` lowers to Arc+lock; `Pool<T>` to a generational arena; view values
  to counted slices — all vetted std internals (I1), all checked by our sema
  first and rustc second (I2);
- embedded/kernel: no stored borrows and no returned borrows means *every*
  function is lifetime-trivial — the checker's proofs are local and complete;
  `no_alloc` floors and arenas as already ratified.

What experts give up vs raw Rust: storable/returnable raw borrows (the `<'a>`
tier). The replacement set — views, `Shared`, `Pool`, arenas, `*T` under
`#Unsafe` — covers the use cases; C1 (philosophy) reserves first-class borrows
as a possible post-v1 tier-2 if real programs demand them, behind explicit
syntax, without disturbing tier 1.

## Consistency check

- One meaning per sigil, all positions: bare=read, `&`=write, `^`=take/own
  (param, call, receiver, local move, field-`^` not needed — fields own by
  default). No dual-meaning glyphs remain.
- Signatures can't lie: nothing unmarked can be changed or kept; no inference
  ever upgrades an API (kills D-CAP8 elevation/freeze + `api: explicit`).
- No silent duplication: heap copies are `copy`, scalar copies are free —
  kills L0201.
- No stored borrows: kills D-REF-SHORTHAND1/2 (#Ref, E0207/E0427) and every
  lifetime question with it.
- Call-site mirroring (D-CAP7 spirit) survives exactly where it informs: `&`
  and `^`.

## D-MEM1 (v5) — the decision

**A — Humanized borrow checker as specified (recommended).**
Bare/read, `&`/write, `^`/take; second-class borrows; view values; named
escape hatches; teaching diagnostics.

**B — Same model, swapped glyphs: `^` = write, `&` = take.**
For the owner to judge by eye: `heal(^kai, 10)` / `enroll(&party… )` — pick
whichever pair reads better; semantics identical.

**C — Same model, Rust-aligned glyphs: `&` = read (explicit, optional), `&&` or
other = write.** Preserves Rust muscle memory (`&` never means write) at the
cost of a heavier write sigil. Listed because option A deliberately flips `&`
away from Rust's read-borrow meaning — Rust immigrants will feel it; one-time
relearn, flagged honestly.

**D — Status quo** (ratified sigil set: `~` edit, `&` share, `^` take, elevation,
freeze-at-API, stored-ref shorthand).

Recommendation: **A**. On approval, next deliverable: migration path from the
current implementation (AccessConvention/`~`/L0201/REF-SHORTHAND machinery →
v5), staged, test-pinned, with each ratified-decision supersession named.
