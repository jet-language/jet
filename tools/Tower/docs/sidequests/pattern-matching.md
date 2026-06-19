# Structural pattern matching
**Status:** Draft plan — needs owner review (2026-06-19)
**Card:** c20

## Problem & why it matters

Jet already ships most of the *surface* for matching — but the *checking* is
shallow. Today (`Source/Sema/CheckerCore.rs`) exhaustiveness is a flat
`HashSet<variant_name>`: it only counts which **top-level** enum variants appear,
and unreachability (`L0301`) only fires when the same top-level variant name
repeats. So this compiles clean even though it can't possibly handle every case:

```jet
// outcome: Result<Status, Error>  where Status = enum { Active; Banned(reason: String); }
if outcome {
    ok(Active)          -> print("live")
    err(e)              -> print("failed: {e.message}")
    else                -> print("???")   // ← only reachable for ok(Banned(_))
}
```

The `else` silently swallows `ok(Banned(reason))`. A beginner who later adds a
`Suspended` variant to `Status` gets **no error** — the `else` keeps absorbing
it. That violates priority #2 (the compiler should catch the case you forgot)
and priority #1's spirit (no silent fall-through where the type system knows
better). Card c20 makes match-checking *structural*: it reasons about the value
**inside** a payload, across nesting depth, ranges, and or-alternatives — the
same depth the user can already *write* (S31 nested patterns are parsed and
bound today; only the exhaustiveness/unreachability analysis stops at depth 0).

What c20 does **not** do: invent a new match construct. The branching keyword is
`if subject { arm -> body }` (S68/D-IF1, ratified + implemented). c20 is the
sema upgrade behind that surface, plus three small surface gaps the owner must
rule on (`_` wildcard token, range patterns, structural or-pattern).

**Scope split (no overlap, no second range token).** c20/D-PATR owns range
**pattern semantics + exhaustiveness at all positions** — both an arm head
tested against the subject (`if score { 0..59 -> "F" }`) and a range nested in a
destructured payload slot (`Closing(500..599)`). One spelling, one exhaustiveness
rule: open `Int`/`Char` always requires an `else`/`_`. **S22** owns the range
**token** (the single inclusive `..`; there is no `..=`; that is Rust/Odin), and
c20 does not re-decide it. **Card c25** (range-switch-arms) owns only the
arm-head **sugar** — the terse `lo..hi ->` desugaring to `>= && <=` — plus its
porting-hazard teaching errors (`..=`, `step`, inverted band); c25 defers to
D-PATR's exhaustiveness rule and adds no competing rule.

## Prior art (terse, cross-language)

- **Rust** — the gold standard: nested patterns, exhaustiveness via the
  *usefulness* algorithm (Maranget 2007), `_` wildcard, `|` or-patterns,
  `1..=10` range patterns, `if let`, guards (`if x > 0`), unreachable-arm
  warnings. Jet's I2/I3 mean we must own this analysis ourselves — we can't
  lean on rustc's match checker (it speaks to nobody but us).
- **Swift** — `switch` is exhaustive-by-default, `case let .some(x)`, `where`
  guards, range patterns (`1...10`), `_`, tuple patterns. Strong beginner
  diagnostics ("switch must be exhaustive, add a default clause").
- **OCaml/Haskell** — origin of the usefulness algorithm; warn on
  non-exhaustive *and* on redundant (unreachable) clauses with a concrete
  witness ("here's an example you didn't cover: `ok(Banned(_))`").
- **Kotlin** — `when` is exhaustive only when used as an expression; statement
  position lets you forget cases. Jet should be stricter (always exhaustive for
  pattern arms — it already is at depth 0).
- **Zig** — `switch` exhaustive on enums/ints, `else` required for open ranges,
  no destructuring binds inside the switch head. Jet is friendlier (binds).

Takeaway for Jet: adopt **Maranget usefulness** (the proven algorithm) for both
exhaustiveness *and* unreachability, emit a **concrete missing witness** in the
error (the owner's diagnostics voice: "missing: `ok(Banned(_))`"), and keep the
surface exactly as ratified — no `match`, no `|`-unless-the-owner-approves-it.

## Proposed design (worked Jet example)

Driving example — a small state machine a real beginner would write:

```jet
enum Conn {
    Idle
    Active(user: String)
    Closing(code: Int)
}

fn describe(c: Conn) -> String {
    if c {
        Idle                          -> "waiting"
        Active(user)                  -> "active: {user}"
        Closing(code) && code >= 500  -> "crashed ({code})"   // guard = && (S31/D-PAT2)
        Closing(code)                 -> "closing ({code})"
    }
}
```

With c20's structural checker this is **exhaustive with no `else`**: `Idle`,
`Active(_)`, and the two `Closing(_)` arms (guarded + unguarded) together cover
`Conn`. The checker knows the *guarded* `Closing` arm alone is **not** enough
(a guard can fail), so it requires the trailing unguarded `Closing(code)` arm —
and if you delete it, you get:

```
error[E0307]: this `if` doesn't cover every case — missing: Closing(_)
  --> conn.jet:9:5
   |
 9 |     if c {
   |     ^^ a guarded arm can fail, so `Closing` still needs an unguarded arm
   |
help: add an arm `Closing(code) -> …`, or an `else -> …` catch-all
```

Nested example — the silent-`else` bug from above now errors:

```jet
if outcome {
    ok(Active)          -> "live"
    ok(Banned(reason))  -> "banned: {reason}"
    err(e)              -> "failed: {e.message}"
}
```

If you drop the `ok(Banned(reason))` arm, the checker descends into the `ok(_)`
payload and reports `missing: ok(Banned(_))` — not a blanket "add else". The
witness is built from the first uncovered constructor at each depth.

**Wildcard.** Subset binding already lets you ignore payload fields, but there is
no token for "match this position, bind nothing." c20 needs one (see D-PATW).
Note `_` is **not** currently free: it is a legal identifier char (spec.md
lexical rules) so a bare `_` lexes today as an ordinary throwaway *name*, and it
is the S34 digit separator inside numerics. D-PATW therefore special-cases `_`
in pattern position (the Rust precedent), not a greenfield token. With the
recommended `_`:

```jet
if c {
    Active(_)   -> "someone's here"    // ignore the user name
    _           -> "nobody"            // catch-all in pattern position
}
```

`_` in tail position is the structural catch-all (distinct from `else ->`, which
stays the value/condition catch-all from D-IF1; both are accepted, `_` reads as
"any value of this type" and participates in the witness algorithm).

**Range patterns** (see D-PATR) let a range sit either in an arm head against the
subject (`0..59 -> …`) or inside a destructured slot — matching on the value
*inside* a variant without spelling a full `&&` guard, and counting toward
coverage. Both positions are c20-owned (semantics + exhaustiveness); the
arm-head *sugar* spelling is c25's, the `..` token is S22's.

```jet
// Closing(code: Int)
if c {
    Closing(500..599) -> "server crash"
    Closing(_)        -> "clean close"
    _                 -> "still up"
}
```

The spelling is S22's single inclusive `..` (no `..=`); c20 reuses the token
without re-deciding it (S22 owns it) but owns the slot's coverage/exhaustiveness,
including open-`Int` strictness.

**Or-patterns** (see D-PATO) — note Jet *already* has S25 value-distribution
(`200 || 404 -> …`) and D-IF1 bare-value arms. The open question is whether the
**structural** form `Active(u) || Closing(u)` (alternatives that *bind the same
name*) is in scope for c20 or stays rejected. Recommended: reuse `||`, require
every alternative to bind the same names at the same types.

## Implementation sketch — pipeline touchpoints

The surface is mostly built; c20 is ~80% a sema-deepening task. File-level:

**Parser (`Source/Parser/Expressions.rs`, `Source/AST.rs`)**
- `Pattern` enum (`AST.rs:672`, variants `Variant`/`Present`/`Absent`/`Ok`/`Err`)
  gains variants only if the owner approves new surface: `Pattern::Wildcard(Span)`
  (D-PATW), `Pattern::Range { lo, hi, span }` (D-PATR), `Pattern::Or(Vec<Pattern>,
  Span)` (D-PATO). Nested payloads: `Variant.bindings: Vec<String>` (`AST.rs:675`)
  must widen to `bindings: Vec<Pattern>` to carry sub-patterns — S31/D-PAT1
  nesting is already *parsed* into a flat string list today, so this is the
  load-bearing AST change (and is what makes a nested range like
  `Closing(500..599)` representable at all).
- The arm-head parser (`switch_arm_pattern` is reconstructed in sema, see below)
  needs the parser to actually retain nested pattern structure instead of
  flattening to names. Today nesting is recognized but lowered eagerly.

**Sema exhaustiveness (`Source/Sema/CheckerCore.rs` ~1165–1290, the `all_pattern`
block)** — the core of the card:
- Replace the flat `covered: HashSet<variant_name>` (CheckerCore.rs:1172) +
  `missing_pattern_coverage` helper (`Source/Sema/Diagnostics.rs:281`; renders via
  `missing_arms_text`, `pattern_variant_name`) with a **matrix-based usefulness
  check** (Maranget). New module
  `Source/Sema/Exhaustiveness.rs`: a `PatternMatrix`, `is_useful(matrix, row)`
  (drives both unreachability — a row useful w.r.t. rows above it is reachable;
  a non-useful row is `L0301` unreachable), and `witness(matrix, ty)` (drives
  E0307 — returns the first uncovered constructor stack, rendered as
  `ok(Banned(_))`). Pure std-only Rust (I6), tree-walking over the registry's
  enum/variant table (`self.registry`).
- Guards: a guarded row contributes to *reachability* but **not** to coverage
  (matches Rust). The existing `check_condition_with_bindings` path already
  separates guard arms; feed "this row has a guard" into the matrix as
  "covers nothing for exhaustiveness."
- Range patterns: coverage of a ranged slot or arm head uses interval arithmetic,
  not the constructor matrix (a small `IntervalSet` merge). Open-`Int`/`Char`
  strictness (always requires `else`/`_`) is c20/D-PATR's call — it is part of
  range-pattern exhaustiveness, which this card owns at all positions.
- Validation (`validate_pattern`, called at `CheckerCore.rs:1201`) extends to
  recurse into nested `Pattern`s, checking each sub-pattern against the payload
  field type (reuses E0305 pattern-type, E0306 arity). The guarded-arm split is
  already available: `switch_arm_pattern` (`CheckerItems.rs:759`) returns the
  pattern for pure pattern arms and `None` for arms carrying a `&&` guard, so a
  guard naturally "covers nothing for exhaustiveness."

**Codegen (`Source/Codegen/Expression.rs` / `Statement.rs`)** — stays dumb (I3).
- Pattern arms already lower to Rust `match`/if-chains. Nested patterns lower to
  nested Rust `match` arms; `_` → Rust `_`; ranges → Rust `lo..=hi` guards;
  or-patterns → Rust `a | b`. Because sema proved exhaustiveness, codegen emits
  a Rust match with **no** `_ => unreachable!()` unless an `else`/`_` arm exists
  — but to honor I2 (rustc never errors on our output), codegen appends an
  internal `_ => unreachable!("jet: exhaustiveness bug")` guarded by the I2
  banner path, never user-visible.

**Formatter (`Source/Formatter/Expressions.rs`)**
- Print nested patterns, `_`, `lo..hi` (S22 inclusive spelling, no spaces around
  `..` per S40/S22 house style), and `a || b` or-patterns. Align arm `->`?
  No — S44 is one-statement-per-line, no column alignment; arms print
  `pattern -> body` with a single space around `->`.

**Diagnostics (`docs/spec/diagnostics.md`)**
- Reuse **E0307** (not-exhaustive) but upgrade its message to carry a structural
  witness; reuse **L0301** (unreachable arm) now driven by usefulness, not name
  dedup; reuse **E0305**/**E0306** for nested pattern type/arity errors. New
  codes only if the owner approves new surface: **E0316** (range-pattern bounds
  invalid — lo > hi, or non-`Int`/`Char` domain), **E0317** (or-pattern
  alternatives bind different names/types). Both codes are free today (E0315 is
  the current max in the E031x band, diagnostics.md:163). Every new code ships a
  `tests/ui` snapshot (I4) or it does not exist.

## Test plan — ui snapshots + example(s)

Examples (I5 — executable spec, golden-tested):
- `examples/features/46_nested_match.jet` — the `ok(Banned(reason))` example,
  expected output proves each variant path runs.
- `examples/features/47_match_wildcard_range.jet` — `_` catch-all and a
  nested-payload range `Closing(500..599)` (gated on D-PATW / D-PATR
  ratification). Arm-head `0..59` range semantics are c20/D-PATR too; only the
  terse arm-head *sugar* spelling is c25's.

`tests/ui` snapshots (each is the proof the diagnostic exists, I4):
- `match_nonexhaustive_nested` — drops `ok(Banned)`, expects E0307 with witness
  `missing: ok(Banned(_))`.
- `match_unreachable_nested` — a `Closing(code)` arm after `Closing(_)`,
  expects L0301.
- `match_guard_not_exhaustive` — only a guarded `Closing` arm, expects E0307
  ("a guarded arm can fail").
- `match_range_gap` — nested `Closing(0..59)`, `Closing(70..100)` (missing
  60..69 in the slot), expects E0307 listing the uncovered interval.
- `match_wildcard_redundant` — an arm after `_`, expects L0301.
- `match_or_pattern_mismatch` — `Active(u) || Closing(c)` (different names),
  expects E0317.

Plus `tests/decisions.rs` rows for each new decision ID once ratified
(ratification enforcement). Re-bless with `UPDATE_EXPECT=1` only after checking
output against the diagnostics.md format.

## Risks & invariant check

- **I2 (rustc never speaks):** the whole point. If our usefulness algorithm is
  wrong and emits a non-exhaustive Rust match, rustc errors → ICE. Mitigation:
  codegen always appends an internal unreachable arm so generated Rust is
  *always* total regardless of our analysis; our E0307 is the user-facing gate.
- **I3 (codegen dumb):** all exhaustiveness lives in `Sema/Exhaustiveness.rs`;
  codegen never inspects coverage. Honored.
- **I6 (zero external crates):** Maranget usefulness is ~200 lines of plain Rust
  over the registry; no crate. Honored.
- **I7 (every sigil has a decision ID):** `_`, range-in-pattern, and structural
  `||`-pattern are new authoring surface → each needs a ratified D-PATW/PATR/PATO
  ID in `Source/Syntax.rs` before any parser code. **Blocked until owner
  ratifies.** Note `_` is not a free token (see D-PATW): it is special-cased in
  pattern position, not newly lexed.
- **I8 (simplicity ratchet):** the risk is over-building. Nested exhaustiveness
  and the witness are the must-haves (they fix a real silent-bug class). `_`,
  ranges, and or-patterns are each individually justifiable but should be
  separable so the owner can land the safety win without the syntax surface if
  desired. Recommendation: ship the sema deepening + `_` first; ranges and
  structural or-patterns are independent follow-ons.
- **Compile-speed (priority #5):** usefulness is exponential in the worst case
  but linear-ish for real enums; cap nesting/width with a fuel limit and fall
  back to "add an else" advice past the cap (same spirit as comptime fuel S26).

## Open decisions

1. Does c20 introduce a `_` wildcard token (special-cased in pattern position),
   or keep "ignore via subset binding / `else ->` only"? (D-PATW)
2. Are range patterns (arm-head `0..59 -> …` and nested-payload `Closing(500..599)`)
   in scope for c20, reusing S22's `..`? (D-PATR owns range-pattern semantics +
   exhaustiveness incl. open-`Int` strictness at all positions; S22 owns the token;
   c25 owns only the arm-head sugar.)
3. Structural or-patterns (`Active(u) || Closing(u)`) in scope, or stay rejected
   (use separate arms)? (D-PATO)
4. Is the **scope** of c20 the safety win only (nested exhaustiveness + witness,
   reusing existing surface), with W/R/O as optional add-ons? (recommended)

---

## Proposed decision card(s)

> Format mirrors `tools/Tower/docs/ballots/decision-ballots.md`. These are
> **drafts for the owner** — not ratified, not yet copied into the ballot file.

### D-PATW — Wildcard token in pattern position (rec A)

When matching an enum, you often want to match a variant but ignore its payload,
or write a structural catch-all. Today you can bind-then-ignore (`Active(u) ->`
and never use `u`, which warns) or use `else ->`. There's no "match, bind
nothing" token. `_` is **not free**: it is a legal identifier char (so a bare
`_` lexes today as a throwaway *name*) and the S34 digit separator in numerics —
so any `_`-as-wildcard option means special-casing `_` in pattern position (the
Rust precedent), not adding a new token. Pick the spelling.

- **Option A — `_` (underscore, recommended).** The universal "I don't care"
  token from Rust/Swift/Haskell/Go. Reads as a hole. Participates in the witness
  algorithm as "any value."

    ```jet
    if c {
        Active(_)  -> "someone is connected"
        Closing(_) -> "shutting down"
        _          -> "idle or unknown"
    }
    ```

    Forgot a variant and have no `_`:
    ```
    error[E0307]: this `if` doesn't cover every case — missing: Idle
    help: add an arm `Idle -> …`, or a `_ -> …` catch-all
    ```

- **Option B — bare `else` only (no pattern wildcard).** Keep D-IF1's `else ->`
  as the only catch-all; for ignored payloads, require a bound name (which then
  warns if unused) or a future `_`-as-name. Smaller surface, but `Active(_)`
  (ignore one field) is then impossible without naming + suppressing.

    ```jet
    if c {
        Active(name) -> "someone is connected"   // `name` unused → warning
        else         -> "other"
    }
    ```

- **Option C — `*` (star).** Free-ish token, "anything." But `*` is pointer
  deref in the expert tier (S58) and multiplication; overloading it in pattern
  position reads oddly next to those.

    ```jet
    if c {
        Active(*) -> "connected"
        *         -> "other"
    }
    ```

- **Option D — `_` for fields, `else` for catch-all (split).** Use `_` only as
  an ignored *payload field* (`Active(_)`), but keep `else ->` as the only
  tail catch-all (no bare `_` arm). Cleanest separation of "ignore a slot" vs
  "match anything," at the cost of two concepts.

    ```jet
    if c {
        Active(_) -> "connected"   // `_` = ignore this field
        else      -> "other"       // `else` = catch-all (not `_`)
    }
    ```

**Recommendation:** A. `_` is the single most recognized pattern token across
languages; one spelling for both "ignore a field" and "match anything" is the
mechanical-uniqueness answer (priority #4). It coexists with `else ->`, which
stays for value/condition arms (D-IF1).

### D-PATR — Range patterns: semantics & exhaustiveness (rec A)

c20/D-PATR owns range-pattern **meaning + exhaustiveness at all positions** —
both an arm head tested against the subject (`0..59 -> …`) and a range nested in
a destructured payload slot (`Closing(500..599)`). One spelling, one
exhaustiveness rule (open `Int`/`Char` always requires `else`/`_`). The range
**token** is S22's single inclusive `..` (no `..=`) and is not re-decided here;
**card c25** owns only the arm-head *sugar* (the terse `lo..hi ->` desugaring)
plus its porting-hazard teaching errors, deferring to this card's checking. The
c20 question: are range patterns in scope, with the checker gap-checking their
coverage?

- **Option A — yes, range patterns at all positions, reuse S22 `..` (recommended).**
  An arm head or a payload position may hold `lo..hi`; the checker gap-checks it,
  and the open `Int`/`Char` domain still always requires a trailing `else`/`_`.
  The payload case falls out of the `bindings: Vec<Pattern>` widening this card
  already needs; the arm-head case is the same interval check one level up.

    ```jet
    // Closing(code: Int)
    if c {
        Closing(500..599) -> "server crash"
        Closing(_)        -> "clean close"
        _                 -> "still up"
    }
    ```

    Drop `Closing(_)` so the slot has a gap below 500:
    ```
    error[E0307]: this `if` leaves a gap — `Closing(_)` below 500 is not covered
    help: add an arm `Closing(_) -> …`
    ```

- **Option B — no; write the `&&` guard.** Keep payload slots bind-only; to test
  the inner value, bind it and add a guard (D-PAT2). Smaller surface (I8), but
  the guard "covers nothing for exhaustiveness," so you always need a fallback
  arm even when the bands tile.

    ```jet
    if c {
        Closing(code) && code >= 500 -> "server crash"
        Closing(code)                -> "clean close"   // required: guard can fail
        _                            -> "still up"
    }
    ```

**Recommendation:** A. Reuse S22's `..` (one spelling across loops, slices, and
patterns), gap-check both arm-head and payload positions, and own the
exhaustiveness rule (open `Int`/`Char` always needs `else`/`_`). The token defers
to S22; c25 owns only the arm-head sugar shape + porting errors and defers its
checking here — exactly one range concept, one exhaustiveness story.

### D-PATO — Structural or-patterns binding shared names (rec A)

Jet already has two "or" mechanisms in arm heads: S25 value-distribution
(`200 || 404 -> …`) and D-IF1 bare-value arms. Neither lets you OR two *enum
patterns that bind a payload*. Should `Active(u) || Closing(u)` (alternatives
binding the same name `u`) be allowed?

- **Option A — reuse `||`, require identical bindings (recommended).** The
  alternatives must each bind the same set of names at the same types; the arm
  body sees those names. Same token as logical-or and S25 — no new sigil.

    ```jet
    // Status = enum { Active(id: Int); Reconnecting(id: Int); Closed }
    if s {
        Active(id) || Reconnecting(id) -> "live session {id}"
        Closed                         -> "done"
    }
    ```

    Mismatched bindings:
    ```
    error[E0317]: or-pattern alternatives must bind the same names
      --> s.jet:3:9
       |
     3 |     Active(id) || Closing(code) -> …
       |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^ left binds `id`, right binds `code`
    help: bind the same name in both, or split into two arms
    ```

- **Option B — `|` (single pipe) for structural or-patterns.** Match Rust
  exactly (`A(x) | B(x)`), reserving `||` for the value-distribution / boolean
  sense. Clear visual split between "pattern alternation" and "logical or," but
  introduces a new sigil (`|` is currently only bitwise-or, S17 `|=`), and two
  near-identical spellings risk confusion.

    ```jet
    if s {
        Active(id) | Reconnecting(id) -> "live session {id}"
        Closed                        -> "done"
    }
    ```

- **Option C — reject; use separate arms.** Keep the surface minimal (I8). If two
  variants share handling, write two arms — or, when they share a *field*, bind
  it in each. Costs duplication for the genuinely-shared-body case.

    ```jet
    if s {
        Active(id)       -> live(id)
        Reconnecting(id) -> live(id)
        Closed           -> "done"
    }
    ```

**Recommendation:** A. Reuse `||` (already the or-spelling for S25/value arms),
require identical bindings across alternatives (checked, E0317), and exclude this
from the *minimum* c20 scope — land nested exhaustiveness first, add structural
or-patterns as a clean follow-on once the matrix checker exists (it makes
or-patterns nearly free).
