# D-IF1 — `if` as universal branching

**Status: DONE (implemented 2026-06-18).** `if_or_dispatch` parses `if subject
{ … }`; `if_body_is_arms` (one-token `->` lookahead) chooses multi-arm mode,
lowering to the shared `Stmt::Switch` IR (bare-value arms via `switch_pipe_cond`,
braceless or block bodies, `else ->` catch-all). `when` → E0984 (still parsed
via `switch_after_kw` for fmt recovery). `jet fmt` renders `Stmt::Switch` as
`if subject { head -> body }` and migrates `when { | arm {} }`. Example
`if_universal.jet`; ui `retired_when_keyword`.

**Fully ratified 2026-06-18** — D-IF1 (option A) + D-IF2 (surface) in
`syntax-decisions.md` (amends S24, S68).

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
5. **Diagnostics — `E0984`**: `when` retired (teaching error → "use `if`").
   Claim in `docs/spec/diagnostics.md` (I4) with a ui snapshot.
6. **Teach `jet fmt`** (`src/fmt.rs`) `when X { … }` → `if X { … }`. Add a NEW
   example exercising bare-value arms, a full-condition arm, a braceless body, a
   block body, and `else` (its own golden test). The existing `when`-using
   examples are migrated mechanically in the final `jet fmt` consolidation pass,
   not by hand.

## Coupling

D-IF1's arm termination assumes S6-R = B (no semicolons) and examples use
D-BIND1 sigils — so land S6-R and D-BIND1 first, and let the single final
`jet fmt` pass migrate the corpus (no per-feature example sweep).
