# Proposal: checked transaction regions (rollback on failure)

**Status: DRAFT — parked for owner review. Not a ballot. Do not ballot yet.**

Source: Verse `decides` failure contexts and the Logan Smith video series,
mined 2026-07-24. The source report is
`docs/archive/2026-07-24-verse-video-mining.md`. Jet's current disposition is
**watch**: ordinary `?` propagation never implies rollback.

This document is a paper design only. It does not change parser, sema, TIR,
runtime, or `?` behavior.

## Glossary

- **Failure-returning function:** a function that returns `T ? E`.
- **Rollback:** undoing the state changes made by a region.
- **Transaction region:** an explicit block that commits all checked changes or
  restores them when a typed failure leaves the block.
- **Compensation:** a typed inverse for an effect that memory journaling cannot
  undo.

## The idea

Give an expert an explicit block where a typed failure undoes every checked
memory change made by that block. The body then reads like the success path,
while the region marker shows where rollback starts and ends.

Verse uses a `decides` effect for this shape. A failure stops the failure
context and rolls back its earlier mutations. Jet must keep its errors-as-values
model: `T ? E` carries the failure payload, and `?` keeps its current meaning.

## Current Jet boundary

The rule is fixed:

> Ordinary `?` propagation never implies rollback.

A checked region is therefore a separate, expert-only rollback region. A function
outside a region keeps its writes when `?` returns an error.

Jet already has the `#Transact` rail. `D-CONC-STM1=A` gives its shared-value
form one-run, ordered commit semantics. The recorded `D-TXN1–4` design gives
local rollback, custom `Rollback` snapshots, and commit/rollback hooks.
`D-BOUND-UNDO1=A` gives foreign calls an explicit `#Undo(inverse)` contract.

This proposal must extend that one rail. It must not add a second `transaction`
mechanism beside `#Transact`.

## Hard design constraints

Any version that ships must satisfy all five constraints:

1. The region is explicit. No other use of `?` starts rollback.
2. Sema proves every mutation is rollback-safe, or the author supplies a
   checked compensation contract.
3. Native and FFI calls name a typed rollback contract. Sema cannot inspect a
   foreign body.
4. Audit output shows the commit and compensation boundaries.
5. Failure remains `T ? E`; the region adds undo but does not replace typed
   failure propagation.

## Invariant check

- **I1:** snapshots and restore preserve ownership and memory safety. The
  `^T` case cannot use a byte copy or a fabricated moved-from value.
- **I2:** generated journal code is an internal compiler concern. A rustc
  rejection is never a user-facing rollback diagnostic.
- **I3:** sema proves rollback safety before codegen. Runtime observation does
  not replace the proof.
- **I8:** `#Transact` is one transaction mechanism. Local, shared, and
  external participants use one visible region and commit boundary.
- **I9:** no implementation exists in this proposal. If built later, the
  rollback meaning must live in the Prelude path shared by AOT, JIT, and the
  interpreter.

## Two-facet read

- **Beginner:** keep this out of the default surface. Beginners use ordinary
  `?` and `??`; neither implies rollback.
- **Expert:** expose the region, write set, compensation contracts, and audit
  boundaries. Experts must be able to see what commits and what can undo.

## Kill-criteria check

- **Invariant break:** kill the design if rollback safety cannot be proved.
- **Duplicate mechanism:** kill it if `#Transact` becomes a second error
  system or if a new keyword duplicates the marker rail.
- **Beginner burden:** keep it expert-only and explicit.
- **Hidden control:** kill it if callers cannot see the rollback boundary or
  the compensation contract.

## Rollback semantics

### Journal boundary

On entry, the region creates one rollback context. Sema proves which mutable
places the context can touch. The journal records logical places, not raw
addresses:

- local mutable bindings;
- fields and elements of containers reachable through an exclusive `&` write;
- structural changes such as insert, remove, and resize;
- custom values through the existing `Rollback` trait;
- `Shared<T>` changes through its buffered STM write set, not a second journal.

The first write to a place records its old value or its type-defined snapshot.
Later writes do not create duplicate snapshots. A failed region restores the
entries in reverse order. `Rollback.restore` is total, as required by
`D-ROLLBACK-TRAIT`; the compiler rejects a type that cannot provide a complete
restore operation.

The journal does not cover arbitrary aliases, frozen values, or state that has
escaped to another task. A new allocation is discarded on failure only when no
reachable value escapes it. A container that owns the allocation is restored
as part of that container's snapshot.

### Region outcomes

- A `?` failure that leaves the region restores the journal, releases the
  transaction context, and returns the same `E` to the caller.
- A failure handled inside the region by ordinary control flow does not abort
  the region. The region can still commit later.
- Normal completion commits the journal and the buffered `Shared<T>` writes.
- A panic does not roll back. `D-NOPANIC1=D` keeps panic distinct from an
  expected `T ? E` failure; the region does not catch or reinterpret it.

The region body runs once. Rollback does not retry it. This preserves
`D-CONC-STM1=A` and prevents logs or other body actions from running twice.

### Nesting

Nested regions use the same transaction context when one exists.

- An inner failure restores changes made since the inner entry. The outer
  region may handle that error and continue with its earlier changes.
- An inner success merges its journal into the outer journal. It is not
  externally committed before the outer region succeeds.
- A failure that leaves both regions restores the merged journal once, in
  reverse order.

An independent transaction context is allowed only when no outer context
contains the same write set. Sema must reject overlapping independent
contexts instead of guessing which snapshot wins.

### Task boundaries

A checked region may not cross a `task`, task-group, channel, or parallel
boundary. A task cannot capture a journaled place, and a region cannot return a
journal handle to a task. This follows `D-CONC-FREEZE1=A` and the architecture
rule that mutable captures and values that cannot cross are rejected at the
boundary.

`freeze(x)` may send an immutable value, but later writes to the frozen copy do
not belong to the original region. A `^` transfer ends the region's ability to
mutate the transferred value unless an explicit rollback contract covers it.

### The `^T` take case: paper spike

A move is harder than a write. After a `^T` take, the source has no valid
moved-from value to restore. A byte copy would violate ownership and memory
safety.

The candidate rule is:

1. Before a region takes a value, sema requires a total snapshot or a typed
   move inverse for that value.
2. On success, the move commits and the destination keeps ownership.
3. On typed failure, the inverse restores the source and drops or invalidates
   the destination according to its checked contract.
4. If no total snapshot or inverse exists, sema rejects the take before
   codegen. It never fabricates a moved-from value.

The rule must also reject a `^T` parameter that would need to restore a value
owned by the caller. Ordinary function return does not restore a caller's
consumed binding. The design spike must settle whether `Copy`, `Rollback`, or a
new ownership-aware contract is sufficient. Until then, the safe default is to
reject such a take inside a checked region.

## Effect interaction

### Memory-only baseline

The recommended first scope is memory rollback plus explicit typed rollback
contracts. `FS`, `DB`, `Net`, and `Exec` effects are not made reversible by a
transaction marker. A function effect ceiling such as `=[FS]=>` limits the
allowed effect set; it does not prove that a filesystem action can be undone.

The region should not add a new effect root. The normal inferred effect row
continues to describe ambient effects, and an existing `=[...]=>` function
ceiling or `#Caps(...)` block ceiling remains available when an author wants a
local limit. Rollback safety is a separate sema fact checked at each call.

### External effects and compensation

An external effect can enter a region only through a contract that names its
inverse and its limits:

- **FS:** an inverse may remove or restore a known file version, but arbitrary
  replacement, permission, rename, and concurrent-writer cases need a typed
  contract. There is no automatic best-effort undo.
- **DB:** a database transaction can supply begin, rollback, and commit as an
  external participant. The region must use that participant contract; a
  memory journal cannot undo a committed database write.
- **Net:** a request may already be observed or acted on by a peer. A response
  or compensating request is a new operation, not automatic rollback.

The current safe path is to reject irreversible effects inside `#Transact`
with E0746, move them after the region, or use an existing `on_commit` hook.
An FFI binding may instead declare `#Undo(inverse)` under `D-BOUND-UNDO1=A`.
Compensations run in reverse call order and must satisfy the same sema and
effect checks as the forward call.

### Relation to `#Transact`

`#Transact` should be the one visible transaction mechanism:

- local memory uses the journal and `Rollback` participant;
- `Shared<T>` uses the ordered STM participant;
- a declared foreign or database contract supplies an external participant;
- all participants share one visible commit boundary and one failure rule.

This keeps `D-CONC-STM1=A`'s stable lock order, one-run body, and no-retry law.
It also keeps I8: participant implementations may differ, but Jet has one
transaction region and one rollback proof.

## Spelling candidates

All samples are design sketches. They are not current Jet syntax.

### A — marker block: reuse `#Transact`

```jet
#Transact {
    from.withdraw(amount)?
    to.deposit(amount)?
    ledger.record(from, to, amount)?
}
```

This reuses the ratified transaction word and the universal `#` expert-region
plane. The existing named form, `#Transact(name) { … }`, can keep explicit
`on_rollback` and `on_commit` hooks.

### B — keyword region

```jet
transaction {
    from.withdraw(amount)?
    to.deposit(amount)?
}
```

This gives the idea a clear word, but it creates a second spelling rail and
breaks the ratified rule that expert regions use `#` blocks.

### C — `#Policy(transactional)` setting

```jet
#Policy(transactional)
fn transfer(from: &Account, to: &Account, amount: Money) ? {
    from.withdraw(amount)?
    to.deposit(amount)?
}
```

This uses the existing policy mechanism from `D-STRUCT-POLICY1=A`, but makes
rollback a property of a declaration rather than a visible region. It also
makes every future failure path in the scope part of the rollback contract.

### Recommendation

Use candidate A. Reuse `#Transact`, keep the region marker visible, and make
the optional name serve existing hooks. Do not make rollback a function-wide
policy or a new keyword.

## Rejected alternatives

- **Implicit rollback on `?`** — breaks the ratified rule.
- **Verse-style `decides` effect on functions** — rollback becomes invisible at call sites — violates Jet's one-visible-marker failure law.
- **Library-only STM with closures** — loses the sema proof, I3.
- **Automatic FS/DB compensation** — unbounded contract surface.

## Open owner questions

1. **Scope:** Is the first version memory-only, with explicit typed contracts
   for selected external participants, or does it include DB/FS/Net
   compensation in the same decision?
2. **`^T`:** Which total snapshot or ownership-aware inverse, if any, makes a
   take safe inside a failing region? This needs a paper spike before code.
3. **Spelling:** Does `#Transact` remain the one visible region marker?
4. **Value:** Is the gain over explicit undo large enough for Jet's target
   work? Keep this as a watch question; it needs examples, not syntax.
5. **Effect model:** Is rollback safety a separate sema fact, or must a region
   carry an explicit effect-row contract?

## Unposted draft ballot slate

The following text is a local draft only. It is not a Tower decision and must
not move to the decide lane while the owner freeze remains.

### D-TXN-SCOPE1 — automatic rollback scope

- **A:** memory rollback only; admit external work only through named,
  sema-checked participant contracts.
- **B:** memory rollback plus typed DB/FS/Net compensation contracts in the
  first version.
- **C:** allow automatic best-effort compensation for known effects.

**Recommendation:** A. Keep the first proof bounded. Add a participant only
when its inverse is total, typed, and auditable.

### D-TXN-SPELL1 — region spelling

- **A:** reuse `#Transact { … }`, with an optional name for hooks.
- **B:** add a `transaction { … }` keyword region.
- **C:** use `#Policy(transactional)` on a function or block.

**Recommendation:** A. It follows the marker-plane law and avoids a second
transaction mechanism.

### D-TXN-EFFECT1 — effect-model fit

- **A:** keep rollback safety as a sema fact; use existing inferred rows,
  `=[...]=>`, and `#Caps(...)` for effect ceilings.
- **B:** add a `Rollback` effect root to the effect row.
- **C:** require an explicit `=[...]=>` ceiling on every transaction region.

**Recommendation:** A. A ceiling limits ambient power but does not prove an
inverse. The rollback proof belongs beside, not inside, the effect row.

## Prior art

- Verse failure contexts with rollback. [Epic failure and control-flow
  guide](https://dev.epicgames.com/documentation/en-us/fortnite/basics-of-writing-code-9-failure-and-control-flow-in-verse)
- Software transactional memory, such as Haskell's `STM`.
- Database transactions with begin, commit, and rollback.
- RAII scope guards, which provide a manual form of rollback.

## Recommendation

Keep this proposal parked. If the owner unfreezes it, ballot
D-TXN-SCOPE1, D-TXN-SPELL1, and D-TXN-EFFECT1, then run the `^T` paper spike.
Do not build parser, sema, TIR, runtime, or compensation code before those
decisions.
