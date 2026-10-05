# #3232 — Typed JSON numbers across execution modes

Date: 2026-09-29. Binary: `jet-debug-snapshot14` (via `~/.cache/jet-dev/safe-jet.sh`).

## Question

Do typed JSON numbers (`json.decode<T>`) mean the same thing on AOT, default
`jet run`, the interpreter and web, keeping exact integers/decimals and
rejecting quoted, fractional, out-of-range and nonfinite inputs with typed
errors?

## Method

Witness `Examples/features/serde/json_typed_numbers.jet` (new): 25 cases over
`Int`, `U8`, `I32`, `Float`, `Decimal` and a nested struct with a `[Int]`
list. Each case prints the value or the `[FieldError]` text. Ran:

```
safe-jet.sh run            Examples/features/serde/json_typed_numbers.jet
safe-jet.sh run --interpret Examples/features/serde/json_typed_numbers.jet
safe-jet.sh build          Examples/features/serde/json_typed_numbers.jet && .jet/build/json_typed_numbers
```

Outputs kept at `~/.cache/jet-dev/scratch/Closer00/out/serde/json_typed_numbers.{run,int,aot}`.
Web was not run (no web runner used by this closer; `cargo test --test web_browser` is a cargo build).

## Evidence (observed)

Agreeing on all three tiers (22 of 25 lines):

| case | result |
|---|---|
| `42`, `9223372036854775807` → Int | exact |
| `123456789012345678901234567890`, `-9223372036854775809` → Int | exact (beyond i64 kept) |
| `1e3` → Int | `1000` (accepted) |
| `2.0` → Int | `2` (accepted) |
| `"42"` → Int | `[at `n`: expected Int, found text "42"]` |
| `1.5` → Int | `[at `n`: JSON number `1.5` is not an exact integer]` |
| `true` → Int | `[at `n`: expected Int, found Bool]` |
| `256`, `-1` → U8; `2147483648` → I32 | `expected U8/I32, found out-of-range Int` |
| `1e400` → Float | `expected Float, found out-of-range Float` |
| `"0.5"` → Float | `expected Float, found text "0.5"` |
| `12.340`, `0.1`, `1E-5` → Decimal | `12.340`, `0.1`, `0.00001` (scale kept) |
| nested `count: 7.5` | `[at `inner.count`: JSON number `7.5` is not an exact integer]` |

Disagreements (defects):

1. **Big integer inside a typed `[Int]` list is corrupted on `jet run` only.**
   Nested `tally: [1,2,99999999999999999999]`:
   interpreter and AOT print `[1, 2, 99999999999999999999]`;
   `jet run` printed `[1, 2, 4611826755488848992]` on one run and
   `[1, 2, 4611826755476022032]` on the next (nondeterministic, looks like a
   boxed big-int handle read as a machine word). Silent wrong value.
2. **Syntax-error text differs by tier** for `{"n":NaN}` / `{"n":Infinity}`:
   interpreter `JSON Syntax at byte 0, line 1, column 1: expected a JSON value`;
   `jet run` and AOT `[invalid JSON (line 1): expected a JSON value]`.
   The interpreter's byte/column (0/1) is also wrong: the bad token starts at byte 5.
3. **Interpreter E0956** when the error arm calls `.len()` on the `[FieldError]`
   (`errors.len()` inside `if json.decode<FloatBox>(raw) == { .Err(errors) -> … }`):
   `E0956 the method .len at compile time isn't supported by the current evaluator`.
   `jet run` accepts the same program. The witness now interpolates `{errors}`
   instead, which all tiers accept.

Policy observations (not defects, but the card's owner should confirm):
`2.0` and `1e3` are admitted into `Int` because their value is integral; only a
nonzero fraction is rejected.

## Verdict

**FAIL.** c1/c4 (fixtures exist for exact, quoted, fractional, overflow,
nonfinite and nested targets) are met by the new witness. c2/c5 hold on the
interpreter and AOT but not on `jet run` (defect 1 silently corrupts). c3/c6
fail: per-mode receipts differ (defects 1 and 2). c7 (golden + dev + web
tests) not met; no golden was blessed because tiers disagree. Web not exercised.
