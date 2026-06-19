# Range arms in `if` multi-arm dispatch (Odin-style range arms)

**Status:** Draft plan — needs owner review (2026-06-19)
**Card:** c25

## Problem & why it matters

The card asks for Odin-style range arms in dispatch — an arm that fires when the
subject falls in a numeric range, e.g. matching `1..5`. Today, in Jet's
multi-arm `if subject { … }` form (S24 → folded into `if` by D-IF1), a range
test is written as a full Bool condition:

```jet
if grade {
    score >= 90 && score <= 100 -> "A";
    score >= 80 && score <= 89  -> "B";
    else -> "F";
}
```

This works but is verbose and re-states the subject twice per band. The
inferred comparator (D-IF1) already lets `200 -> …` mean `subject == 200`; the
card wants the range analog: **`90..100 -> …` meaning `subject` is in `90..100`.**

Two hard facts the design must honor:

- **Jet ranges are a single inclusive `..` (S22).** There is **no `..=`** — the
  card's example `1..=5` is Rust/Odin spelling and must be rewritten `1..5`
  (which already means "1 through 5 inclusive" in Jet). Reusing the existing
  range token is mandatory; inventing `..=` would split S22.
- **This overlaps card c20** (structural pattern matching). c20's **D-PAT-R**
  is *also* about range arms in `if` arm heads (`if score { 0..59 -> … }`) — it
  is **not** a separate "destructuring-only" feature. So c20 and c25 govern the
  **same arm-head range construct**; the only real difference is depth: c20's
  Maranget checker reasons about *coverage* (gap-checking, exhaustiveness),
  while c25 frames the same arm as pure desugaring (`>= && <=`). Both spell it
  `lo..hi` (S22 inclusive). The ownership section (D-RANGE2) resolves this: S22
  owns the `..` *token*; **c20/D-PAT-R owns arm-head range semantics**
  (checking + exhaustiveness); c25 owns only the *desugaring shape* and the
  *porting-hazard teaching errors* (`..=`, `step`-in-arm, inverted band), and
  must use spelling and exhaustiveness rules identical to D-PAT-R. No second
  range syntax may appear, and the two cards must not record competing
  exhaustiveness rules.

## Prior art (terse)

- **Odin `case 1..=5:` / `case 1..<5:`** — inclusive and half-open range cases
  in `switch`. Jet has only inclusive `..` (S22), so only the inclusive form maps.
- **Rust `1..=5 => …` match arms** — inclusive range patterns; `1..5` is
  exclusive in Rust (the opposite of Jet's inclusive `1..5`). A porting hazard
  worth a teaching error.
- **Swift `case 1...5:`** — inclusive range in `switch`. Same idea, different
  token.
- **Jet today (S24/D-IF1):** range via `&&` Bool condition; works, verbose. The
  proposal is sugar over exactly this.

## Proposed design (worked Jet example)

A range arm is a bare range expression in arm-head position; it desugars to
`subject >= lo && subject <= hi` (inclusive, S22). It composes with the
existing inferred-comparator structural-mix rule (D-IF2 Q3): a head whose
top-level form is a range literal is a **range arm**; a head with a comparison
operator stays an ordinary condition.

```jet
fn letter(score: Int) -> String {
    if score {
        90..100 -> "A";          // score is in 90..100  (inclusive, S22)
        80..89  -> "B";
        70..79  -> "C";
        60..69  -> "D";
        else    -> "F";
    }
}
```

Mixing range, bare-value (D-IF1), and full-condition arms in one dispatch:

```jet
if code {
    200          -> "ok";                 // inferred ==  (D-IF1)
    400..499     -> "client error";       // range arm   (this card)
    500..599     -> "server error";
    code > 599   -> "out of range";       // full condition (S24)
    else         -> "unknown";
}
```

Semantics:

- Desugar: `lo..hi -> body` ⇒ `subject >= lo && subject <= hi -> body`.
- Subject and bounds must share an ordered type (`Int`, `Float`, `Char`).
- `step` (S22) is **rejected** in arm position — a stepped range is a sequence,
  not a contiguous band; teaching error.
- Overlap/exhaustiveness: range arms participate in the same first-match-wins
  order as all arms. **Exhaustiveness is c20/D-PAT-R's call, not c25's:** the
  ratified D-PAT-R rule is that range arms buy *gap-checking* (the checker
  reports uncovered intervals between bands) but the open `Int`/`Char` domain
  **always** still requires a trailing `else`/`_`. c25 must record the *same*
  rule — it does not get to say "ranges make a dispatch exhaustive." When
  c20/D-PAT-R lands, range arms count toward gap-checking; until then they
  desugar with `else` mandatory as today (S24).

How it differs from a runtime range loop: `1..5` in `loop i in 1..5` (S22)
*iterates*; `1..5` in an arm head *tests membership*. Same token, two
contexts — both already true of `..` in Jet (range value vs. slice S40), so no
new ambiguity is introduced beyond what S22/S40 established.

## Implementation sketch — pipeline touchpoints

1. **`Source/Syntax.rs` (I7).** No new token — reuse the S22 `..` range. Log the
   *new use of `..` in arm-head position* under the S24/D-IF1 arm block with the
   new decision ID (D-RANGE1). Cross-reference c20 ownership note.
2. **Parser.** In the multi-arm `if` arm-head parser (where D-IF2's structural
   mix already decides bare-value vs. condition), add: if the head parses as a
   range expression (`Expr::Range`), tag the arm `ArmHead::Range(lo, hi)`. The
   range expression parser already exists (S22 loops/slices); reuse it. No new
   grammar rule, only a new arm-head classification.
3. **Sema.** When checking an arm: for `ArmHead::Range`, check `lo`/`hi` are the
   subject's ordered type and `lo <= hi` (constant-fold both if literal → warn
   on empty/inverted band, new lint L-band); reject `step` (teaching error);
   then lower to the `&&` condition for the rest of checking. Reuse the existing
   arm Bool-condition machinery — a range arm is *defined as* its desugaring.
4. **Codegen (dumb, I3).** Already receives the desugared `>= && <=` condition
   from sema, or emits the same chained `if`/`else` it emits for any S24 arm. A
   contiguous all-`Int` band could lower to a Rust `match` range pattern as an
   optimization, but that is the compiler's call (S24 already says optimization
   is never the user's job); the simple lowering is `>= && <=`. No new codegen
   path required for correctness.
5. **`jet fmt`.** Format a range arm head as `lo..hi` (no spaces around `..`,
   matching S22 loop formatting); one classifier branch in `Source/Formatter.rs`.

## Test plan — ui snapshots + example

- **Example (I5):** `examples/features/NN_range_arms.jet` — the `letter(score)`
  grading function above, driven over a few scores; `.expected` golden output.
- **ui snapshot — `..=` porting hazard:** a user writes `90..=100 ->` → teaching
  error (E0xxx parse-band, S14-style) "Jet ranges use `..` and are inclusive;
  write `90..100`." Snapshot pinned (I4).
- **ui snapshot — wrong-Rust-semantics hazard:** detect `1..5` *intended*
  exclusive? Cannot — but document in the teaching error above that `1..5`
  includes both ends.
- **ui snapshot — type mismatch:** range bounds not the subject's type (e.g.
  `"a".."z"` against an `Int` subject) → existing type-error machinery, snapshot
  pinned.
- **ui snapshot — `step` in arm head:** `1..10 step 2 ->` → teaching error
  "a range arm tests a band; `step` belongs in a loop." Snapshot pinned (I4).
- **lint — inverted/empty band:** `100..90 ->` → warning (L-band), snapshot in
  `tests/ui_lint/`.

## Risks & invariant check

- **I1/I2/I3:** pure front-end sugar desugaring to existing `&&` conditions;
  codegen unchanged. OK.
- **I4:** four new diagnostics/lints listed, each gets a snapshot. OK.
- **I6:** std-only Rust. OK.
- **I7:** no new token; reuses S22 `..`; new arm-head use logged under D-RANGE1.
  OK.
- **I8** simplicity ratchet: this is sugar, not a new mechanism — it removes
  repetition the owner repeatedly cuts (anti-repetition memory). Low risk, but
  it *is* a new arm-head shape, so it needs ratification.
- **Cross-card consistency (the real risk):** c20/D-PAT-R already governs the
  *same* arm-head range construct. If c25 and c20 ratify different spellings or
  different exhaustiveness rules, Jet gets two range syntaxes / two coverage
  stories — a direct S22/I8 violation. Mitigation: D-RANGE2 below makes
  c20/D-PAT-R the owner of arm-head range *semantics* (spelling + exhaustiveness)
  and limits c25 to the desugar shape + porting-hazard teaching errors.

## Open decisions

1. Confirm the inclusive `..` reuse (vs. introducing `..=` — rejected by S22,
   listed only to be explicitly closed again).
2. Exhaustiveness is deferred to c20/D-PAT-R (gap-checking, `else` always
   required for open `Int`/`Char`); c25 must not record a different rule.
3. `Char` and `Float` range arms in v1, or `Int` only first?
4. Ownership of arm-head range semantics — c20/D-PAT-R, not c25 (see D-RANGE2).

## Proposed decision card(s)

### D-RANGE1 — Range arms in multi-arm `if` (rec A)

Multi-arm `if` (D-IF1) lets `200 ->` mean `subject == 200`. This card adds the
range analog. Jet's range is inclusive `..` (S22); there is no `..=`.

- **Option A — reuse inclusive `..`, desugar to `>= && <=` (recommended).** One
  range syntax across the whole language (loops S22, slices S40, now arms).

    ```jet
    if score {
        90..100 -> "A";      // score >= 90 && score <= 100
        80..89  -> "B";
        else    -> "F";
    }
    ```

- **Option B — introduce `..=` for arm ranges only.** Matches Rust/Odin muscle
  memory, but splits S22 (two range tokens, one inclusive-by-default and one
  explicit-inclusive) — an I8/S22 violation for cosmetic familiarity.

    ```jet
    if score {
        90..=100 -> "A";     // a second range token Jet doesn't have
        else     -> "F";
    }
    ```

- **Option C — no sugar; keep the `&&` form (I8 default-no).** Smallest
  language. Cost: every band restates the subject twice; against the owner's
  standing anti-repetition direction.

    ```jet
    if score {
        score >= 90 && score <= 100 -> "A";
        else                        -> "F";
    }
    ```

**Recommendation: A.** Reuses the one ratified range token, reads as English,
desugars to existing machinery (codegen untouched), and cuts the repetition the
owner consistently trims. Exhaustiveness behaviour is governed by c20/D-PAT-R
(gap-checking; open `Int`/`Char` always requires `else`) — see D-RANGE2.

### D-RANGE2 — Ownership of arm-head range semantics across c25 and c20 (rec A)

c20's **D-PAT-R** already governs range arms in `if` arm heads — the *same*
construct this card proposes. They are not two positions (arm vs.
destructuring); they are two depths of the *one* arm-head feature. Jet must end
up with exactly one range spelling (S22/I8) **and** one exhaustiveness rule.

- **Option A — S22 owns the `..` token; c20/D-PAT-R owns arm-head range
  *semantics* (checking + exhaustiveness); c25 owns only the desugaring shape +
  porting-hazard teaching errors; c25 may ship the sugar first, deferring to
  D-PAT-R's rules (recommended).** One spelling, one exhaustiveness story. c25
  delivers the terse `lo..hi ->` arm and the `..=`/`step`/inverted-band errors
  now; when c20 lands its Maranget checker, the *same* arms gain gap-checking
  with no syntax change. c25 must not record an exhaustiveness rule that
  differs from D-PAT-R (open `Int`/`Char` always needs `else`).

    ```jet
    // c25 ships this arm-head sugar (desugars to >= && <=, else mandatory):
    if code { 400..499 -> "client error"; 500..599 -> "server error"; else -> "?"; }

    // c20/D-PAT-R later deepens the SAME arm with gap-checking (no syntax change):
    if code { 400..499 -> "client error"; else -> "?"; }
    //         ^ checker can now report an uncovered band between arms
    ```

- **Option B — c20/D-PAT-R owns the whole arm-head range feature; c25 is folded
  into c20 and ships nothing on its own.** Single owner, zero divergence risk,
  but blocks the cheap, shippable-now sugar on the larger c20 sema effort.

    ```jet
    // c25 ships nothing until c20's range-pattern checker lands.
    ```

- **Option C — c25 and c20 each own arm-head ranges independently.** Rejected:
  two cards editing the same arm-head classifier and the same `..` spelling
  with possibly different exhaustiveness rules — a direct I8/S22 split. Listed
  only to close it.

    ```jet
    // two ratification paths converge on one grammar — the divergence hazard.
    ```

**Recommendation: A.** S22 owns the `..` token; c20/D-PAT-R owns arm-head range
*meaning*; c25 ships the terse sugar + porting-error teaching now, under
D-PAT-R's spelling and exhaustiveness rules. One spelling, one checker, no
competing decisions — and the cheap win does not wait on full pattern matching.
