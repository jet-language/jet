# S19 — Loop unification implementation

Ratified 2026-06-17 (S19-amend option A). `loop` is the one loop form; `while`
and `for` become teaching errors. This file tracks the remaining implementation
work.

## What is decided

- `loop { }` — infinite loop
- `loop cond { }` — conditional loop (replaces `while`)
- `loop item in iter { }` — iterator loop (replaces `for`)
- `loop i, item in iter { }` — indexed iterator loop
- `while` / `for` → parse, reject with a teaching error pointing to `loop`

## Tasks

1. **Parser** — confirm `while` and `for` are lexed but produce `E_RETIRED`
   diagnostics with `help: use loop` text.
2. **Sema** — no changes needed; all loop forms already lower to the same IR.
3. **Diagnostics** — add snapshot tests for:
   - `while cond { }` → teaching error
   - `for x in iter { }` → teaching error
4. **Example** — `examples/features/loop_forms.jet` covering all four forms.
5. **Update syntax-decisions.md** — confirm S19 ratification date is recorded.

## Not in scope here

Labeled loops / named `break` — that is a separate amendment tracked in
`d-label1-loop-labels.md`, blocked on D-LABEL1 ballot.
