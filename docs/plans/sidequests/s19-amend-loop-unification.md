# Sidequest: S19-amend — enforce unified `loop` keyword in code

**Ratified:** 2026-06-17, S19-amend option A (see docs/spec/syntax-decisions.md §S19)  
**Blocker for:** any milestone that touches loop syntax; parser correctness

## What and why

The owner ratified `loop` as the single loop keyword. The header picks the mode:

```jet
loop { … }           // infinite
loop n > 0 { … }    // conditional (was `while`)
loop i in 1..5 { … } // iteration (was `for`)
```

`while` and `for` become S14 teaching errors pointing at `loop`. Currently `src/syntax.rs` still registers them as primary S19 keywords, so `tests/decisions.rs` does not catch the drift — S19 is listed as ratified but the code hasn't been updated to match the amendment.

## Files to change

1. **`src/syntax.rs`** — move `KW_WHILE` and `KW_FOR` from primary S19 constants to S14 foreign-syntax teaching-error constants (same pattern as `KW_FUNC`, `KW_DEF`, etc.). `KW_LOOP` already exists and stays.

2. **Parser** (`src/jetpack/` or wherever the loop parser lives) — collapse the three parse branches into one `loop` keyword with header disambiguation. `while` / `for` inputs hit the teaching-error path.

3. **`tests/ui/`** — re-bless any snapshot that contains `while` or `for` loop syntax. New snapshots needed for `while`/`for` teaching errors.

4. **`examples/features/`** — rewrite any example that uses `while`/`for` to use `loop`. Golden tests enforce these.

5. **`docs/spec/spec.md`** — update loop section to match the new unified syntax.

## Exit criteria

- `nix develop -c cargo test` passes.
- `while true { }` and `for i in 1..5 { }` both produce S14 teaching errors pointing at `loop`.
- All examples that used `while`/`for` are rewritten to `loop` and their golden tests pass.
- `tests/decisions.rs` continues to pass (S19 stays listed as ratified; the amendment is reflected in code).
