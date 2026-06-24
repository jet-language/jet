# if `subject ==` explicit dispatch (D-IF3)

**Status: GATED on owner ratification of decision D-IF3.** This revises ratified
D-IF1/D-IF2. Nothing in the Implementation slice ships until the owner picks the
options below. Build something else meanwhile (I8 / syntax decision protocol).

## Goal

Make value/pattern dispatch explicit and drop the repeated subject from arms.

```jet
// boolean if — unchanged
if ready {
    launch()
}

// value/pattern dispatch — NEW explicit form
if status == {
    "active"  -> connect()
    "idle"    -> wait()
    "closed"  -> {
        log("done")
        cleanup()
    }
    else      -> report()
}
```

Today dispatch is *implicit*: `if subject { arm -> body }` (D-IF1/D-IF2). Bare
values auto-prepend `subject ==` (D-IF2 Q3, `switch_pipe_cond`,
`Source/Parser/Statements.rs:1389`), but enum/pattern arms still write the
subject on every line — `examples/features/71_pattern_matching.jet:15` has
`c == Active(id) | Reconnecting(id) ->`, repeated on every arm. The owner wants
two things: (1) a required `==` marker between subject and `{` to *enter*
dispatch, removing the "is this body arms or statements?" lookahead ambiguity;
(2) no `subject ==` on any arm, primitives **and** patterns alike.

This trades against priority #4 (one mechanical path) in the right direction:
one unambiguous spelling for "dispatch on a value," replacing the current
overloaded `if subject { … }` whose meaning depends on whether the first item
happens to contain `->`.

## Open syntax questions (for D-IF3 ballot)

Each card below needs one owner pick. Recommendations weigh only the ranked
priorities (safety, beginner experience, performance, one path); never effort.

### Q1 — Marker spelling
Options: **A** `if subject == { … }` (owner's pick) · **B** `if subject is { … }`
· **C** keep implicit `if subject { … }`.

Recommend **A**. `==` is already the equality operator and is exactly what each
arm means against the subject — the marker *names the operation* it performs, so
a beginner reads `if status == { "active" -> … }` as "if status equals one of
these." `is` (B) introduces a new keyword for a meaning `==` already owns
(fights priority #4 and #2 — two spellings for equality). C is the status quo
the owner is explicitly replacing: its ambiguity (body = statements vs. arms)
is the problem. Note: A reads slightly oddly for pattern arms (`Active(id)` is a
match, not an `==`); the why-text and E-codes must teach "`==` here means
*matches*," and the E0312 (value `==` unsupported, `docs/spec/diagnostics.md:176`)
story must not leak — see Q4.

### Q2 — Mandatory vs. optional (does `==` replace the implicit form?)
Options: **A** `==` is **required** to enter dispatch; bare `if subject { arm -> }`
becomes a teaching error · **B** both forms coexist.

Recommend **A**. Priority #4: one path. Coexistence (B) means two ways to write
the identical dispatch, and keeps the lookahead ambiguity A is meant to kill.
Cost: a breaking migration of every implicit-form file (see Migration) and a
teaching error for the old form. That cost is paid once and buys an
unambiguous grammar — exactly the trade philosophy.md says to make. The
migration is mechanical and `jet fmt`-able (insert `==`, strip `subject ==`
prefixes), so beginners porting old code get a one-step fix, not a wall.

### Q3 — Arm head → body separator
Options: **A** keep `->` on every arm (`"active" -> connect()`) · **B** allow
bare `pattern { … }` for block bodies, `->` only for braceless single bodies.

Recommend **A**. `->` reads as "leads to," is already ratified (D-IF2), and is
uniform across braceless and block bodies — one shape to learn (#2, #4). B
splits arm syntax on whether the body is a block, which a beginner can't predict
and which collides visually with a nested `if x { … }` *inside* an arm. Keep
`->` always.

### Q4 — Predicate / guard arms in `== {` mode
Today an arm head may be a full Bool condition: `(code >= 400) && (code <= 499) ->`
(`examples/features/07_switch.jet:30`). In explicit `subject == {` mode the arm
head is matched *against the subject*, so a free-standing predicate no longer
fits the mental model ("each head is compared to `subject`").

Options: **A** disallow predicate arms in `== {` mode — ranges cover bands
(`400..499 ->`), arbitrary predicates go in a conventional boolean `if`/`else if`
chain · **B** still allow a Bool-typed head as a guard.

Recommend **A**. The whole point of the explicit marker (Q1) is that `subject ==`
gives every arm one meaning: *does the subject match this?* Re-admitting bare
predicates (B) reintroduces the "is this head a value or a condition?"
ambiguity at the arm level — the same overload Q1 removes at the block level.
Ranges (D-PATR, already ratified) cover the common `400..499` case cleanly;
genuinely arbitrary predicates are a different operation and belong in boolean
`if`/`else if`, which the owner is keeping (rule 6). This keeps #4 honest. The
migration of `07_switch.jet:30`'s `(code >= 400) && (code <= 499)` arm becomes a
`400..499 ->` range arm. A new teaching error (E0993, see Diagnostics) catches a
predicate head in `== {` mode and points at ranges or a boolean `if`.

### Q5 — Catch-all spelling
Recommend **keep `else ->`** unchanged (D-IF2 Q1). It already reads as the
default arm, is ratified, and needs no migration. No competing option worth a
card.

## Implementation slice

Assumes Q1=A, Q2=A, Q3=A, Q4=A, Q5=keep ratify.

**Failing test first.** Add `tests/ui`-style example + golden: rewrite
`examples/features/if_universal.jet` (or a new `07`-series example) to the
explicit form and pin its expected output; it must fail to parse on `master`
until the parser changes land. Plus a `tests/ui/dispatch_missing_eq.stderr`
snapshot for the old-form teaching error (Q2) — per I4, no snapshot means the
diagnostic doesn't exist.

**Parser** (`Source/Parser/Statements.rs`):
- `if_or_dispatch` (~835): the marker `==` must be detected *before* the subject
  parser swallows it. ⚠️ `expr_no_struct_lit` (`Source/Parser/Expressions.rs:230`)
  routes through `expr_cmp` (`:332`), which **consumes `==`** at `:335` (as a
  comparison or, via `try_pattern_rhs`, a `PatternTest`). So a plain
  `let subject = self.expr_no_struct_lit()?;` then `peek()==EqEq` will **not**
  see the `==` — the subject parse already ate it and then chokes on `{`. The
  subject must be parsed below comparison precedence (e.g. a dedicated
  `expr_no_cmp`/`expr_no_struct_lit_no_cmp` entry that stops at the first
  comparison operator), so the trailing `== {` is left for `if_or_dispatch` to
  detect. Then: if `peek()==TokKind::EqEq` and `peek2()==LBrace`, consume both
  and go straight to arm mode (`self.if_arms(subject, span)`). The `==` is the
  marker; no speculative body lookahead in this branch.
- The non-`==` branch becomes purely conventional `if`: expect `{`, parse
  `block_stmts`, optional `else`/`else if`. **Delete the `if_body_is_arms`
  call** from the conventional path.
- `if_body_is_arms` (~869): retire it (Q2 makes implicit dispatch illegal). Its
  range-arm lookahead logic (`lo .. hi ->`, E0318/E0319 porting hazards) moves
  *inside* `if_arms`, which already re-detects ranges at ~960 — keep that, drop
  the duplicate pre-scan.
- `if_arms` (~928) + `switch_pipe_cond` (~1389): the arms no longer carry
  `subject`. **`switch_pipe_cond` already does exactly the wanted transform** —
  it prepends `subject ==` to a bare head and recurses through `||`/`&&`,
  leaving `PatternTest`/`Bool`/comparison heads alone. Under Q4=A, the `Bool`
  and bare-comparison cases become *errors* (a predicate head), so trim
  `switch_pipe_cond` to: bare value / `||` chain of values → `subject ==`;
  `PatternTest` (enum/range/or-pattern) → leave as-is but **bind it to the
  subject** (today the arm wrote `c == Active(id)`, so the `PatternTest` already
  references `c`; now the head is bare `Active(id)` and `switch_pipe_cond` must
  attach `subject` to it). This is the core "drop `subject ==` from pattern
  arms" change — `Expr::PatternTest` needs its scrutinee filled in from
  `subject` rather than parsed from the arm text.
- `==` pattern-RHS parsing (`Source/Parser/Expressions.rs` ~335–352,
  `try_pattern_rhs` ~1288): today this fires for `c == Active(id)`. In arm heads
  the `c ==` no longer appears, so arm-head pattern parsing must call
  `try_pattern_rhs` directly on a bare `Active(id)` / `Good(200..299)` /
  `Active(id) | Reconnecting(id)` head. Confirm the pattern grammar parses
  standalone (no leading `subject ==`); add an arm-head entry point if needed.

**Sema** (`Source/Sema/CheckerInfer.rs`): unchanged in shape — both forms lower
to `Stmt::Switch { subject, arms, else_body, span }` (`Source/AST.rs:1134`),
which sema already checks. Exhaustiveness **E0307** still applies to enum/range
subjects. Subject typing is unchanged (the subject expr is parsed identically).
**E0110 boundary**: the conventional `if` path still requires a `Bool`
condition; the `== {` path imposes *no* `Bool` requirement on the subject (it's
a value being matched) — verify the E0110 check is keyed off `Stmt::If`, not the
subject of `Stmt::Switch`, so dispatching on a non-`Bool` subject is fine.
Per Q4, a predicate arm head is rejected in the parser (new code), so sema sees
only value/pattern heads.

**Codegen** (`Source/Codegen/TIR.rs`): **no change.** `Stmt::Switch` already
lowers through the TIR (`lower_switch`, `Source/Codegen/TIR.rs:5426`) to a Rust
`match`, with the in-subset gate at `:2386` (`switch_in_subset`); the TIR variants
are `RangeSwitch` (`:316`) and `MixedSwitch` (`:374`). Verified: both the
conventional and the new dispatch forms produce the *same* `Stmt::Switch` AST, so
they share this one path — the AST shape is unchanged, codegen is untouched
(confirms R1/I3, codegen stays dumb).

**Examples** (I5 — executable spec): migrate every dispatch example to the
explicit form (full list in Migration). Each keeps its `expected/*.out`
unchanged (behavior is identical).

## Migration

Q2=A breaks every file using implicit `if subject { arm -> }`. All must move to
`if subject == { … }` and drop per-arm `subject ==`. Full surveyed dispatch sites
(verified by inspecting each `if subject {` body for `head ->` arms):

- `examples/features/07_switch.jet` — `if day {` (line 9), `if code {` (line 23);
  the `(code >= 400) && (code <= 499) ->` arm (`:30`) becomes `400..499 ->` (Q4).
- `examples/features/if_universal.jet` — `if code {` (line 10); the
  `(code >= 500) ->` guard arm (`:18`) becomes a range or moves to a conventional
  `if` (Q4). The trailing conventional `if`/`else` stays as the boolean example.
- `examples/features/71_pattern_matching.jet` — three dispatch `if`s; drop
  `c ==` / `r ==` from every arm head (`Active(id) | Reconnecting(id) ->` `:15`,
  `Good(200..299) ->` `:37`, the `0..59 ->` grade range `:50`, etc.). The file
  the owner called out.
- `examples/features/11_enums.jet` — two enum-dispatch `if light {` (arms
  `Red`/`Yellow`/`Green`, lines 8 and 22).
- `examples/capstone/logbook/index.jet` — `if e {` (`:18`, enum errors
  `NoFrontmatter`/`MissingField`/`BadType`), `if note.parse(...) {` (`:59`, arms
  `it == ok(n)`/`it == err(e)`), `if maybe_note {` (`:76`, `== value(...)`/`== null`).
- `examples/capstone/logbook/logbook.jet` — `if maybe_note {` (`:57`,
  `== value(n)`/`== null`), `if cmd {` (`:118`, string dispatch
  `"version"`/`"index"`/… + `else`).
- `examples/capstone/logbook/note.jet` — `if k {` (`:126`, enum
  `User`/`Feedback`/`Project`/`Reference`), `if parse(...) {` (`:183`, arms
  `it == ok(n)`/`it == err(e)`).
- `examples/capstone/logbook/render.jet` — `if note_kind {` (`:99`, enum dispatch
  `User`/`Feedback`/`Project`/`Reference`).
- `examples/capstone/logbook/search.jet` — `if k {` (enum, `:38`), `if q {` (×2,
  `== Tag(t)`/`Kind(k)`/`Text(s)`), `if raw {` (×2, `== Tag`/`Text` + `else`).
- `examples/capstone/logbook/server.jet` — `if method {` (`:38`, `"GET"` + `else`),
  `if maybe_note {` (`:59`, `== value(...)`/`== null`).
- Re-confirm with `rg -nE '^\s*if [a-z_][a-z_0-9.()" ]* \{' examples/` then check
  each body's first non-blank line for ` -> `; boolean `if`s (`if found {`,
  `if as_json {`, `if backlinks.len() > 0 {`, etc.) stay as-is. The
  `35_zerocopy`/`47_library`/`63_named_args`/`67_scope_guard`/`jetgrep`/`library`
  hits for `->` are fn return types / closures / string interpolation, **not**
  dispatch arms — leave them.

UI/test fixtures to re-bless or migrate (all verified present):
`tests/ui/switch_not_exhaustive.{jet,fixed.jet,stderr}`,
`tests/ui_lint/switch_unreachable_arm.{jet,warn}` (lint → `.warn`, not `.stderr`),
`tests/ui/range_switch_missing_else.{jet,stderr}`,
`tests/ui/range_arm_{dot_dot_eq,inverted,step}.{jet,stderr}`,
`tests/ui/match_{or_pattern_mismatch,range_bad_bounds,wildcard_payload}.{jet,stderr}`,
`tests/ui/foreign_switch.{jet,stderr}` + `tests/ui/foreign_switch_case.{jet,fixed.jet,stderr}`,
`tests/ui/retired_when_keyword.jet`. The `tests/decisions.rs` ratification test
will need D-IF3 recorded.

**Teaching-error story for the old form.** Q2=A: a bare `if subject { head ->
… }` (implicit dispatch, no `==`) must not become a confusing parse error. Add a
teaching error that recognizes "a `{` body whose first item is an arm `head ->`,
with no `==` before the `{`," names the new `if subject == { … }` form, and —
following the S14 teaching pattern (diagnostics.md voice rules) — keeps parsing
as if `==` had been written so the rest of the file's errors still surface.
`jet fmt` performs the same fix automatically.

## Diagnostics

New codes (each needs a row in `docs/spec/diagnostics.md` + a `tests/ui`
snapshot, per I4). Reuse the next free `E09xx` slots.

- **E0992 (parse, teaching)** — implicit dispatch without `==`: "a multi-arm
  `if` now needs `==` between the subject and `{`." Why: `if subject == { … }`
  marks value dispatch explicitly so a plain `if` body is always statements.
  Fix: "write `if subject == { … }`." Recover by parsing the body as arms.
- **E0993 (parse)** — predicate/Bool arm head in `== {` mode (Q4=A): "an arm
  head here is matched against the subject, not a free condition." Why: every
  arm in `subject == { … }` tests the subject; an arbitrary predicate has no
  subject to match. Fix: use a range arm (`400..499 ->`) for a band, or a
  boolean `if … { } else if … { }` for arbitrary predicates.
- The retired `c == Active(id) ->` arm prefix (now redundant): optional
  teaching error **E0994 (parse)** — "drop the `subject ==`; the `==` on the
  `if` already applies it to every arm." Fix: delete `subject ==` from the arm
  head. Lower-priority; if omitted, the prefix would re-parse as a nested `==`
  inside the head and fail with a worse message, so this card is recommended.

Existing codes unchanged: E0307 (exhaustiveness), E0305/E0306 (pattern
mismatch), E0316/E0317/E0318/E0319 (range/or-pattern), L0301 (unreachable arm),
E0110 (boolean `if` condition), E0984 (`when` → if; still points at the new
form — update its fix text to show `if subject == { … }`).

## Tests

- New golden example in the explicit form (failing-first), `expected/*.out`
  pinned (I5).
- `tests/ui/dispatch_missing_eq.{jet,stderr}` → E0992.
- `tests/ui/dispatch_predicate_arm.{jet,stderr}` → E0993.
- `tests/ui/dispatch_redundant_subject.{jet,stderr}` → E0994.
- Re-bless all migrated `tests/ui` switch/match snapshots to the new arm form.
- `tests/decisions.rs`: record D-IF3 ratification.
- `nix develop -c cargo test` green; `nix develop -c jet run` each migrated
  example matches its `.out`.

## Dependencies / risks

- **Formatter (D-FMT1, single-line arms).** Braceless single-line arms (rule 5)
  depend on `jet fmt` **not** re-expanding `"idle" -> wait()` into a block. This
  is decided by D-FMT1, planned in
  `tools/Tower/docs/sidequests/fmt-single-line.md` (its "Dependency: D-IF3"
  section co-owns `fmt_preserves_single_line_dispatch_arm` and cross-links back
  here). D-IF3 should be ratified together with — or behind — D-FMT1, or the
  formatter will fight this syntax. The brace question (`"active" -> connect()`
  vs `-> { connect() }`) is D-IF3's (Q3); the keep-it-on-one-line question is
  D-FMT1's. Flag for the owner that the two ballots are coupled.
- **Pattern-head parsing without a scrutinee.** The riskiest change: arm heads
  must parse `Active(id) | Reconnecting(id)`, `Good(200..299)`, `Idle(_)` as
  standalone patterns and have `switch_pipe_cond` bind them to `subject`. The
  existing `try_pattern_rhs` assumes a `subject ==` prefix; verify it factors
  cleanly into a scrutinee-free pattern parser. If `Expr::PatternTest` stores
  its scrutinee inline, the binding step must inject `subject` after parsing.
- **Breaking change scope (Q2=A).** Every dispatch in the repo migrates at once;
  do it in one commit so no intermediate state has half-migrated examples
  failing golden tests (philosophy.md "do it right the first time").
- **`==` reads as match, not equality, for pattern arms.** Acceptable given the
  beginner mental model "matches one of these," but the E0993/E0994 why-text
  must teach it so a user never thinks `==` requires their enum to implement
  value equality (don't leak E0312).
