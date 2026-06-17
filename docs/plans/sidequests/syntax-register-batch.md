# Sidequest: Register recently-ratified syntax in syntax.rs + decisions.rs

**Ratified:** 2026-06-16/17 batch (see docs/spec/syntax-decisions.md)  
**Blocks:** tests/decisions.rs enforcement for every new S/U/D code; parser work for M6-M18

## What and why

Every user-typeable keyword/sigil must live in `src/syntax.rs` with a decision ID (I7).
The 2026-06-16/17 ratification batch added many new IDs. Walk `docs/spec/syntax-decisions.md`
against `src/syntax.rs` and add any missing entries. `tests/decisions.rs` must then enforce them.

## Decisions to check / register

| Decision | Surface to register |
|---|---|
| S19-amend | `loop` stays; `while`/`for` → S14 teaching-error bucket (see also `s19-amend-loop-unification.md`) |
| S75 | fan-out operator `.[…]` (e.g. `f.[a, b, c]`) |
| S76 | fixed-size list type `[T#N]` |
| S80 | `Error` built-in type; `Fallible` trait name |
| S81 | `?continue` loop-skip syntax |
| S82 | `@` attribute sigil (if not already present); `@unsafe`, `@audit`, `@bindgen`, `@extern` |
| VERSION-# | `#` version-pin operator (`pkg#1.2.0`) in package expressions |
| U-series | `module name { }` typed declaration (D-FP3); `payload.jet` / `pack.jet` source names |
| D-PAT1-3 | refutable bind `if let`-style patterns; guard syntax; nested patterns |
| S84 | loop keyword amendment (check if row exists) |

## Files to change

1. `src/syntax.rs` — add missing constants with their decision ID comments
2. `tests/decisions.rs` — add enforcement assertions for each new constant
3. Re-bless any `tests/ui/` snapshots that reference keywords moved to S14 teaching errors

## Exit criteria

- `nix develop -c cargo test` passes (decisions.rs green)
- Every token listed above has a named constant in syntax.rs with its decision ID
- No S14 teaching-error token appears as a primary keyword constant
