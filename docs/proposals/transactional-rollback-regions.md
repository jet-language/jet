# Proposal: checked transaction regions (rollback on failure)

**Status: DRAFT — parked for owner review. Not a ballot. Do not ballot yet.**
*Source: mined from Verse (`decides` effect) and the Logan Smith video series, 2026-07-24. See `docs/archive/2026-07-24-verse-video-mining.md`. Current Jet disposition on this idea is "watch" — see `docs/archive/language-shape-research.md` and `docs/archive/language-lessons-and-regrets.md:399-407`.*

## Glossary

- **Fallible function:** a function that can fail. In Jet it returns `T ? E`.
- **Rollback:** to undo the state changes a piece of code made.
- **Transaction region:** a marked block of code that either finishes fully or undoes all its own changes.
- **Compensation:** a paired "undo" action for a step that cannot roll back on its own (for example, a step that sent a network request).

## The idea in one line

Give an expert a marked block where, if any step fails, Jet undoes every state change that block made — so the code reads as the happy path only.

## What Verse does (the source of the idea)

Verse has a `decides` effect. A function with `decides` can fail. You write the code as if every step will succeed. If a step fails, Verse:

1. stops the block, and
2. rolls back every change the block made before the failure.

So you can write an optimistic path and add other paths for when it fails. You never write explicit undo code. This is Verse "failure contexts with rollback."

## What Jet has today

Jet expresses failure as a value: `T ? E`. You propagate failure with the postfix `?`. This is deliberate and it is the stronger choice for most code (typed error payloads, one visible marker at each exit point).

But Jet's `?` does **not** roll anything back. If a function changes some state and then a later `?` fails, the earlier change stays. The current rule is exact:

> Ordinary `?` propagation never implies rollback.

So the rollback idea is a **new, separate** capability, not a change to `?`.

## What a Jet version could look like

A marked region. Inside it, writes to tracked state are journaled. On failure, Jet replays the journal backwards.

```jet
// SKETCH ONLY — syntax not decided, not balloted.
fn transfer(from &Account, to &Account, amount Money) => Void ? {
    transaction {
        from.withdraw(amount)?   // changes state
        to.deposit(amount)?      // if THIS fails, the withdraw above is undone
        ledger.record(from, to, amount)?
    }
}
```

If `to.deposit` fails, the region undoes `from.withdraw`, and `transfer` returns the failure. The author writes no undo code.

## Hard design constraints (from Jet's existing "watch" note)

Any real version must keep all of these, or it does not ship:

1. The region is **explicit**. There is no hidden rollback anywhere else.
2. Sema **proves** every effect in the region is rollback-safe, or the author gives a compensation for it (I3 — all checking in sema).
3. Native and FFI calls need a **typed rollback contract**. Sema cannot see inside them, so the author must state how they undo.
4. Audit output shows the **commit and compensation boundaries** — an expert can see exactly what will undo and where.
5. `?` outside a region keeps its current meaning: no rollback.

## Invariant check

- **I3 (sema checks):** rollback-safety is a sema proof, not a runtime guess. Fits.
- **I8 (one mechanism):** the region must not become a second error mechanism. Failure stays `T ? E`; the region only adds *undo*. It must compose with `?`, not replace it. This is the main risk to watch.
- **I2 (rustc hidden):** the journal-and-replay code is generated Rust; a rollback that rustc rejects is an internal compiler error, never a user error.
- **I1 (safety):** the journal must not break memory safety; a rolled-back `^T` take is the hard case (see open questions).

## Two-facet read

- **Beginner:** a beginner almost never needs this. Keep it out of the beginner surface. Beginners get `?` and `??`, nothing more.
- **Expert:** the expert wants full control — which state rolls back, how non-memory effects compensate, and a clear audit of the boundaries. This is an expert-only feature behind an explicit region.

## Kill-criteria check

- Breaks an invariant? No, if the constraints above hold.
- Duplicates a mechanism? **Maybe** — this is the real danger. If the region starts to look like a second way to "handle errors," kill it. It must only add undo.
- Burdens the beginner default? No, if it stays expert-only and opt-in.
- Hides expert control or audit? No — constraint 4 makes the boundaries visible.

Verdict: worth keeping as a proposal. It does not clearly break a rule, but I8 needs care.

## Open questions for the owner (not balloted)

1. **Scope of automatic rollback.** Memory state only? Or also `Fs`/`Db`/`Net` effects through compensations? Memory-only is far simpler and safe; effects need compensation contracts.
2. **The `^T` take case.** If a region takes ownership of a value and then fails, what does the rolled-back state hold? A destructive move has no valid moved-from value to restore.
3. **Syntax.** `transaction { }`? A marker on the function? This is owner syntax and needs a ballot if the idea proceeds.
4. **Is it worth it?** Manual undo with `??` and normal code already works. The gain is real only when regions are common and the undo code is error-prone. Do we have that case in Jet's target work?
5. **Interaction with the effect system.** Rollback-safety could be a new effect fact (like `no_alloc`). Should it join the effect row model, or stay separate?

## Prior art

- Verse failure contexts with rollback (Epic). [Epic docs](https://dev.epicgames.com/documentation/en-us/fortnite/basics-of-writing-code-9-failure-and-control-flow-in-verse)
- Software transactional memory (STM), as in Haskell's `STM` monad.
- Database transactions (begin / commit / rollback).
- RAII scope guards (C++ `scope_exit`), the manual version of the same idea.

## Recommendation

Keep this parked. If the owner wants it, the next step is a ballot for open questions 1, 3, and 5 (scope, syntax, effect-model fit), plus a small design spike on the `^T` case (question 2). Do not build before that ballot.
