# Proposal: yielding loops

**Status: active proposal** (owner keep 2026-07-25). Revisit sequencing after
the `#` sigil sweep (#732) lands. Builds on D-LOOPLABEL3=A: `outer :: loop`
naming, `outer.break()` / `outer.next()` exits, bare `break`/`next` keywords
for the innermost loop.

## The idea

Two payloads and one self-reference, making loops expressions:

1. `break(v)` — the loop ends with the answer `v`.
2. `next(v)` — the loop continues with its declared state set to `v`.
3. `loop` (keyword, inside a loop) refers to the loop itself, like `self` —
   so unnamed loops get the same dot exits: `loop.break(v)`.

A loop declares state up front; falling off an iteration means "next, state
unchanged"; when iteration ends without `break`, **the state is the value**.
That makes every loop form total — no empty case, no `??` patch, no `else` arm.

## Worked examples

```jet
// running total — no mutation anywhere
total :: loop (acc :: 0) n; nums {
    next(acc + n)
}

// search with the default built in
first_bad :: loop (found :: Cell.none) cell; grid {
    if cell.bad { loop.break(cell) }
}   // no break → found

// retry with backoff — no collection behind it; iterators can't do this
conn :: loop (delay :: 1) {
    attempt :: connect(server)
    if attempt.ok { loop.break(attempt) }
    sleep(delay)
    next(delay * 2)
}
```

## Why it might earn its place

- Kills the mutable-accumulator idiom: all `::`, no `:=`. Fits Jet's
  immutable-first identity.
- The retry/poll/converge/game-tick family has no collection to iterate;
  today it forces mutable variables.
- With payloads, `break()`/`next()` parens usually carry something — the
  empty-parens case becomes rare.

## Why it might not

- Collections are already covered: `.find`, `.position`, `.reduce` exist.
  A state loop is a second way to fold — an I8 judgment call.
- Biggest version of the feature: sema must unify the state type across
  `next(...)` sites and the result type across `break(...)` sites.

## Open questions (owner)

1. Does `loop.break()` — the keyword referring to itself — feel right, or too
   clever? If adopted, do bare `break`/`next` keywords retire (one shape
   everywhere) or stay as innermost-loop short forms?
2. Does `loop (acc :: 0) n; nums` read as "a loop carrying acc, starting
   at 0", or does the parenthesized state slot feel bolted on? Syntax
   variants for the state slot were not yet explored.
3. Scope cut: nothing / `break(v)` only (cheap, but "ended without breaking"
   needs `T?` + `??`) / full state loop (no wart, biggest surface).
