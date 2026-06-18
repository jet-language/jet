# D-IF1 — `if` as universal branching

**Status: fully ratified 2026-06-18** — D-IF1 (option A) + D-IF2 (surface) in
`syntax-decisions.md` (amends S24, S68). Ready to implement; no open questions.

## Ratified design

- `if` is the one branching keyword. `when` → teaching error `E_KEYWORD_RETIRED`
  pointing at `if`.
- `if subject { arm -> body … }` is multi-arm dispatch (the former `when`).
- **Inferred comparator:** a bare value arm is compared against the subject —
  `if code { 200 -> …; 404 -> …; }` ≡ `code == 200` / `code == 404`.
- **Catch-all arm:** `else -> body` (D-IF2 Q1). `...` was rejected.
- **Arm bodies:** braceless single expression (`200 -> print("ok")`) or a `{ … }`
  block for multiple statements (D-IF2 Q2).
- **Bare-value vs condition — structural mix (D-IF2 Q3):** an arm head with **no
  top-level comparison/logical operator** is a bare value (compiler prepends
  `subject ==`); an arm containing one is a full `Bool` condition; the two mix
  freely in one block.
- Arm termination follows S6-R = B (no semicolons).

## Plan

1. **Parser** — inside an `if` body, peek the first arm: if the first expression
   is immediately followed by `->`, switch to **arm mode** (`->` is not a valid
   expression-position operator elsewhere, so one token of lookahead suffices).
   In arm mode:
   - parse each arm as `arm_head -> arm_body`;
   - `arm_body` is a single expression/statement **or** a `{ … }` block;
   - `else -> body` is the catch-all arm;
   - classify `arm_head`: if it has **no top-level comparison/logical operator**,
     mark it a bare value; otherwise a full condition.
2. **Sema** — for a bare-value arm, synthesize `subject == arm_head` (reuse S31
   pattern-test / S25 comparison machinery; the value's type must match the
   subject). Multi-arm `if` lowers to the same IR the old `when` used;
   exhaustiveness and type checks unchanged. Two-arm `if` lowers as today.
3. **`when` keyword** — `E_KEYWORD_RETIRED` → "use `if scrutinee { arm -> body }`".
4. **`src/syntax.rs`** — `KW_SWITCH` (`when`) becomes a teaching-only foreign
   keyword; the arm-arrow `->` constant stays. Tag the multi-arm `if` under D-IF1.
5. **Diagnostics** — `E_KEYWORD_RETIRED` for `when`; claim in
   `docs/spec/diagnostics.md` (I4) with a ui snapshot.
6. **Examples/tests** — migrate every `when` to `if`; add an example exercising
   bare-value arms, a full-condition arm, a braceless body, a block body, and
   `else`. Re-bless golden + ui snapshots. Examples use the no-semicolon style
   (S6-R = B).

## Coupling

D-IF1's examples assume S6-R = B (no semicolons) and D-BIND1 sigils. Sequence the
`when → if` migration with the shared S6-R / D-BIND1 example + snapshot sweep so
the re-bless happens once.
