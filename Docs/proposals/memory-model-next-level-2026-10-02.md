# Jet's memory model, next level: one owner settles, checks prove or stop

Status: proposal, 2026-10-02, revised the same day. Evidence:
[Vale and Valen mining report](../research/mine-for-jet-2026-10-02-vale-valen.md).
Work state lives on the Tower cards named below. Live-probe findings from an
obsolete binary were withdrawn; the items they motivated are listed only as
"check on a fresh build".

## The idea in one paragraph

Jet already has the core that Vale spent a decade proving out: single
ownership, move-only values that must be used, generation-checked handles, and
effect-tracked purity. What it lacks is the rule that ties them together. That
rule has two halves:

- **Exactly one owner settles every value.** Settling means dropping it, or
  doing the job it owes.
- **Every other way to reach a value either proves it is still alive, or checks
  and stops.** No other reference keeps the value alive, and none silently sees
  the wrong value.

Purity then removes most checks for free. Replay records every input that
breaks determinism. Beginners write none of this. Experts can see and forbid
every inserted check.

## Syntax changes

Each change below shows today's spelling, then the recommended one. Every change
needs an owner ballot unless it is marked as covered by a ratified decision.

### S1. One marker for values that owe a job: `#Owes`

Today, `#SingleUse` says a value must be consumed but not how.

```
#SingleUse struct Lock {
    resource: String
}

fn release(lock: ^Lock) { print("unlocked {lock.resource}") }
```

Recommended: name the settling actions on the type. A settle may be a method
on another type and may take extra arguments (Vale's Higher RAII). `on_fail`
names the no-argument action that runs if the scope fails before settling.

```
#Owes(release)
struct Lock {
    resource: String
}

#Owes(commit, rollback, on_fail: rollback)
struct Txn {
    conn: DBLease
}

#Owes(Shipyard.dock)
struct Ship {
    name: String
}

impl Shipyard {
    fn dock(&self, ship: ^Ship, cargo: Cargo) { … }
}
```

`#Owes` replaces `#SingleUse`, and task handles become `#Owes(join, detach)`.
This is D-OWES1 option A (#4139); the qualified settle `Shipyard.dock` is an
addition to that ballot.

### S2. One way to abandon a job: `.drop("reason")`

Today there are three spellings:

```
#Unsafe("event cancelled; the ticket admits to nothing") {
    consume(b)
}
world.remove(kai).drop("this example only needs the generation bump")
// #MustUse results: .drop("reason")
```

Recommended: one method for every value that owes something.

```
ticket.drop("event cancelled; the ticket admits to nothing")
```

The reason string is the audit note, and `jet explain` lists every one.
`#Unsafe` goes back to meaning memory safety only. D-OWES1's plan already names
`.drop("reason")` as the single abandon gate; this makes it explicit.

### S3. The duty follows the value into a parameter

Today `fn sink(lock: ^Lock) {}` counts as settling the lock, because parameters
never carry the duty (`crates/jet-sema/src/Sema/Registration.rs:736-741`).
Recommended: the callee inherits the duty, so it must settle, move, return, or
`.drop("reason")` the value.

```
fn sink(lock: ^Lock) {}
// Error: `lock` still owes `release`
// Fix: call release(lock), return it, or write lock.drop("reason")
```

Generic code that throws a value away states it with a bound. Every type that
owes nothing satisfies `Droppable` automatically.

```
fn ignore<T: Droppable>(value: ^T) {}

ignore(lock)
// Error: `Lock` owes `release`, so it is not Droppable
```

A container is droppable exactly when its elements are, which is how
D-LIN-CONTAINER1=A (#3966) falls out without a special rule.

### S4. Last-use moves are unmarked

Today the examples write `release(^db)`. Ratified D-COPY-DEFAULT1=A makes every
last use a move with no mark. The examples and `jet fmt` should match:

```
db :: acquire("db")
release(db)            // last use: moves. `^db` only pins it.
```

This is covered by D-COPY-DEFAULT1=A (#3741). No ballot is needed.

### S5. Links: stop by default, ask with `.get()`

Ratified `@T` links (D-MEMREF1=A) are not built yet. Recommended pairing,
matching `Pool` exactly:

```
world[id].name         // stops if the entity was removed
world.get(id) ?? fallback   // optional: absent once removed

w.target.name          // stops if the Lease was settled (D-LINK-DEBT1 option A)
w.target.get() ?? fallback
```

The ballot is D-LINK-DEBT1 (#4251). The `.get()` pairing is a detail to add to
it.

### S6. Change list items in place

Today a loop only reads its source, so changing elements needs indexes:

```
loop i in players.indexes() -> players[i].hp -= 1
```

Recommended: a `&` source gives each iteration one element to change.

```
loop p in &players -> p.hp -= 1
```

Inside the loop the list itself cannot be named, so `&players.push(…)` there is
an error. Parallel loops use the same form: `loop row in &image.rows.parallel()`.

### S7. One denial for "no hidden costs": `!Hidden`

Ratified D-DENY-COST1=A adds `!Mem.Gc`, `!Lock`, `!Notify`, `!Dyn`,
`!Mem.Check`, and `!Mem.Copy(above: N)`. Hot code that wants all of them must
list them:

```
fn step(world: &World) -[!Mem.Alloc, !Mem.Copy(above: 0), !Mem.Check, !Mem.Rc, !Mem.Gc, !Lock, !Dyn]> { … }
```

Recommended: one named group with the same meaning.

```
fn step(world: &World) -[!Hidden]> { … }
```

The compiler then reports every copy, check, count, lock, or dynamic call it
would have added, each with its call path.

### S8. A key that proves its entry exists

`Pool.add` returns a copyable `Id<T>`, so every lookup must check it. Vale's
linear key is returned once and proves the entry exists until it is handed back:

```
claim :: &world.claim(Player{name: "Ada"})   // Claim<Player>, #Owes(release)
world[claim].hp -= 1                          // no check: the claim proves it
&world.release(claim)                         // returns the Player, ends the claim
```

This is optional expert control. `Id<T>` stays the default. It depends on
D-OWES1.

## Behavior work

| Step | What | Tower |
|---|---|---|
| 1 | Decide declared debts and the duty-in-parameter rule (S1–S3) | D-OWES1, #4139 |
| 2 | Decide how links treat values that owe a job (S5) | D-LINK-DEBT1, #4251 |
| 3 | Use proven purity to skip checks and counts on pre-existing data | #4246 |
| 4 | Record task message and lock order so concurrent runs replay exactly | D-REPLAY-ORDER1, #4252 |
| 5 | Measure what native `extern` code can reach in Jet memory | #4248 |
| 6 | Lint that suggests a declared debt where docs say "remember to" | #4253 |
| 7 | Later: group borrowing for several writes into one collection | idea `b0880hj6` |

Check on a fresh build. These came from an obsolete binary and have no cards:

- `Pool` generation wraparound: `u32` with `wrapping_add` in `MathTaskMem.rs`.
- `^` parameters discharging a duty.
- Duty in lists and fields (#3966).
- Two write lends with run-time indexes, and E0204's `~` fix.
- `jet run --record` parity.

## What Jet should not copy

- **Probabilistic checks.** Random 64-bit generations catch misuse only with
  very high probability. Jet's safety claim needs certainty.
- **Tethering and hybrid-generational memory.** Vale abandoned it after more
  than 30 attempts.
- **Region annotations.** Vale's `r'` and `'r!` markers are optional hints. Jet
  infers purity and spells it once as `-[]>`.
- **Counted references to values that owe a job.** See S5 and D-LINK-DEBT1.
