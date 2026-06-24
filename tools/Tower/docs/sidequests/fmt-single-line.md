# Sidequest: `jet fmt` single-line bodies (D-FMT1)

**Owner-initiated revision of ratified S44.** Quote: *"i also want single line
statements not to be auto formatted to multilines."* Today `jet fmt` forces every
brace body onto its own lines; a short `if ready { launch() }` becomes a 3-line
block. The owner wants short single-line bodies left alone.

This is a **revision of a ratified decision (S44)**, so it is owner-gated. Per the
syntax protocol: this plan develops the options into a ballot card (D-FMT1) and
**stops at the implementation slice** until the owner picks. Build nothing under
"Implementation slice" until D-FMT1 ratifies.

## Status quo (verified, build on this — do not re-discover)

- `Source/Formatter/Statements.rs:232` `fmt_if` — after `fmt_cond`, always
  `write(" {")` then `newline()`, then `with_indent(fmt_block_stmts)`, then
  `end_block()`. Else-body (`:245`) same. No inline path exists.
- `Source/Formatter/Statements.rs:259` `fmt_switch_arm` — always
  `write(" {")` + `newline()` + `end_block()`. The comment at `:255-258` is
  explicit: *"A single-statement body could be braceless, but fmt always uses a
  block for a stable, idempotent shape."* That comment is the policy this revises.
- `Source/Formatter/Statements.rs:219` `fmt_value_block` (if-*expression* branch):
  always `write("{")` + `newline()` + `with_indent` + `end_block`. The if-expr
  path in `Source/Formatter/Expressions.rs:125,130` routes *through*
  `fmt_value_block` — not a separate site, so fixing `fmt_value_block` covers it.
- `Source/Formatter/mod.rs:422` `end_block` — unconditionally `newline()` + `"}"`.
  Every block close routes here.
- `Source/Formatter/mod.rs:380` `write` tracks `self.col`; `:414`
  `chain_break_between(from, to)` already inspects `src[from..to]` for `\n`. This
  is the existing **author-intent** primitive (S69/D-SG3) and is the model for
  Option 1.
- AST carries the spans needed: `IfStmt.span`, `SwitchArm.span`
  (`Source/AST.rs:1080,1095`), and `stmt_start(stmt)` (`Source/Formatter/mod.rs:448`)
  gives each statement's start offset. A body's "written on one line?" =
  no `\n` in `src` between the brace and the closing brace.
- Tests: `fmt_is_idempotent_on_examples` (`tests/fmt.rs:19`) loads every file
  under `examples/` and asserts `format_source(once) == once`; every other test
  fmts an inline source string and asserts a second `format_source` pass equals
  the first. S69's author-preservation test is
  `fmt_preserves_author_placed_chain_breaks` (`tests/fmt.rs:170`) — the template
  for the new tests (inline `r#"…"#` source, `jet::format_source`, assert
  `out == twice`).
- S44 text: `docs/spec/syntax-decisions.md:599-604`. Log row `:2578`.

## The crux for the owner

S44's promise is that fmt output is a **canonical function of the AST**: the same
program formats identically no matter how the author spaced it. *Preserving the
author's single-line choice breaks that* — same AST, two outputs depending on
source whitespace. It can still be **idempotent** (`fmt(fmt(x)) == fmt(x)`), which
is what the test suite enforces and what users actually feel, but it is no longer
canonical. That trade is a real decision, so it is the owner's, not ours.

Philosophy bearing (`docs/spec/philosophy.md`): ranked priority #4 "one mechanical
path" explicitly **exempts structural arrangement** — its own text (`:57-58`):
*"Structural flexibility (where code lives, how it is nested or externalized) is not
constrained by this priority."* The §34 "One mechanical path, flexible structure"
section reinforces it: *"There is not exactly one way to* arrange *code"* (`:36-37`)
and *"`jet fmt` can enforce a project preference; the language never forces one"*
(`:44`). So canonical *layout* was never load-bearing for the ranked priorities; S44
chose it as a fmt-internal convenience, not a safety or beginner guarantee. Priority
#2 (beginner experience, `:49-51`) makes diagnostics/learnability the product;
fmt that preserves what the author wrote serves that better than fmt that reflows it.
That points away from the status quo. (Effort is not a factor in any ranking — per
philosophy.md `:64-65` tie-break "when it trades effort against anything, effort
loses" and CLAUDE.md.)

---

## Open question for D-FMT1

Three options. Each gives a worked before/after and the S44 clauses it amends.

### Option 1 — Author-intent preservation (recommended)

A body written entirely on one line **stays** one line if it still fits width 100;
a body the author broke across lines **stays** multiline. fmt never changes the
line-count decision — it only normalizes spacing/indent within whichever shape the
author chose. Same mechanism as S69 dot-chains (`chain_break_between`).

Idempotent. **Not** canonical (this is the trade above).

```jet
// before (author wrote these)
if ready { launch() }
if ready {
    launch()
}

// after `jet fmt` — both shapes survive, spacing normalized
if ready { launch() }
if ready {
    launch()
}
```

Amends S44: the "one statement per line" clause becomes "one statement per line for
multiline bodies; a body the author placed on a single line is preserved when it
fits the 100-column width." Adds an explicit non-canonical/idempotent note.

**Why recommended.** It is exactly the owner's literal ask and nothing more — it
never collapses a block the author chose to expand (Option 2 does that uninvited).
It reuses the already-blessed S69 author-intent model, so fmt stays internally
consistent (author breaks are honored for chains *and* bodies). The only thing it
costs is S44's canonical property, which philosophy.md #4 already declared
non-binding for layout.

### Option 2 — Width-canonical collapse

fmt decides purely from AST + width, ignoring source whitespace: a block holding
exactly one *simple* statement that fits under 100 cols renders inline; everything
else expands. Canonical **and** idempotent.

```jet
// before
if ready { launch() }
if ready {
    launch()
}

// after `jet fmt` — BOTH collapse to inline (they have the same AST)
if ready { launch() }
if ready { launch() }
```

⚠️ **Flag for owner:** this also collapses short *multiline* blocks the author
deliberately expanded — a behavior the owner did **not** ask for. An author who
spread `if ready {\n    launch()\n}` for emphasis gets it crushed to one line.

Amends S44: replaces "one statement per line" with a width-driven inline rule for
single-simple-statement blocks; keeps the canonical-function-of-AST promise intact.

### Option 3 — Status quo (move away from this)

Always expand. What we have now. Rejected by the owner's request; listed for the
ballot's completeness.

```jet
// before
if ready { launch() }
// after `jet fmt`
if ready {
    launch()
}
```

### Sub-questions the owner must also settle

1. **Scope: which constructs are eligible?** Candidates: `if`/`else` bodies,
   `while`/`for`/`loop` bodies, `fn` bodies, dispatch/`switch` arms, `if`-expression
   branches (`fmt_value_block`). Recommend: **all brace bodies uniformly** — a
   per-construct allowlist would itself violate "one mechanical path" by making fmt
   behave differently for visually identical blocks.
2. **Eligibility predicate (both Option 1 and 2 need a floor):** a body may render
   inline only if it (a) contains **exactly one** statement, (b) that statement is
   **simple** (not itself an `if`/`while`/`for`/`switch`/`loop`/block — no nesting
   inline), (c) contains **no comment** inside the braces, and (d) the rendered
   single line **fits width 100** at the current indent. Anything failing (a)–(d)
   expands. Option 1 adds the further gate: the author wrote it on one line.
3. **Braces stay required for multiline (S3).** This revision touches only line
   layout, never brace presence. No braceless bodies are introduced — `if x stmt`
   without braces remains illegal. (Dispatch arms are the one place a braceless
   *single-line* arm is proposed; see Dependency.)
4. **`else`:** an inline `then` with a multiline `else` is legal under Option 1
   (each branch judged independently) but reads oddly. Recommend: under Option 1, if
   any branch in the chain is multiline, expand the whole chain, for readability.
   Under Option 2 each branch is independent. Owner to confirm.

**Recommendation: Option 1, all brace bodies, with the (a)–(d) floor and the
"whole if/else chain shares one shape" rule.** It is the minimal faithful answer to
the request and reuses an existing, already-ratified fmt behavior.

---

## Implementation slice (do NOT start until D-FMT1 ratifies)

Assumes **Option 1**. If the owner picks Option 2, drop the source-span check and
key the decision purely on the eligibility predicate; everything else is the same.

**1. Failing test first** (`tests/fmt.rs`): add `fmt_preserves_single_line_if`
asserting `if ready { launch() }\n` round-trips unchanged, plus the multiline
counterpart stays multiline. Watch it fail against today's always-expand.

**2. Inline-vs-expand decision point.** Add a helper on `Fmt`:

```rust
/// Option 1: did the author keep this whole body on one line, and is it
/// eligible to stay inline? `open`/`close` bracket the body in `self.src`.
fn body_inline(&self, body: &[Stmt], open: usize, close: usize) -> bool {
    body.len() == 1
        && is_simple_stmt(&body[0])
        && !self.span_has_comment(open, close)        // gate (c)
        && !self.src.get(open..close).is_some_and(|s| s.contains('\n')) // author chose one line
    // width gate (d) is checked after rendering, see below
}
```

`span_has_comment` checks `self.comments` for any comment whose span falls in
`open..close` (comments are span-tracked: `mod.rs:69` `struct Comment { text, span }`,
field `self.comments`). `is_simple_stmt` must reject **every** block-bearing variant,
not just the loops: `Stmt::If`, `While`, `For`, `Switch`, `Loop`, `Unsafe`, `Region`,
`Caps`, `ComptimeIf`, `ContextBlock`, `Live` (the full list `end_block`-routing set
in `Statements.rs`). Allowed inline: `Expr`, `Val`, `Assign`, `Return`, `Break`,
`Continue`, label breaks/continues.

**3. Width-100 gate (d).** Render the inline candidate into a scratch buffer (or
record `self.out.len()` and `self.col` before, render, then measure
`self.col` against `100 - current_indent*INDENT`); if it overflows, roll back and
expand. Cleanest: a `try_inline` that renders into a temporary `String`, measures
the longest line, and only splices it in if it fits — keeping `end_block`/`newline`
untouched for the expand path.

**4. Wire into the three sites.** In `fmt_if` (`Statements.rs:232`),
`fmt_switch_arm` (`:259`), and `fmt_value_block` (`:219`): before the existing
`write(" {")` + `newline()`, try the inline path. Inline shape is
`{ <stmt> }` (one space inside each brace, no inner newline, no `end_block`).
The expand path is the current code verbatim. Drive every brace body through one
shared `fmt_body(open_after, body, close_before)` so the rule lands in exactly one
place (one mechanical path).

**5. Idempotence.** After fmt, an inline body has no `\n` between its braces, so on
the second pass `body_inline` sees the same single-line source and re-emits inline →
stable. A body that expanded (failed a gate) has a `\n`, so it stays expanded →
stable. The width gate is a pure function of rendered text, so it agrees on both
passes. This is why `tests/fmt.rs:19` keeps passing.

**6. Comments.** Gate (c) forces expansion when any comment sits inside the braces,
so the existing `emit_leading`/`emit_trailing` comment-reattachment path
(span-driven) is only ever used on the expand path — no new comment placement logic.

---

## S44 amendment text (for the recommended Option 1)

Replace, in `docs/spec/syntax-decisions.md:599-604`, the clause
"…spaces around binary operators, one statement per line, single blank line…" with:

> …spaces around binary operators; **one statement per line in multiline bodies,
> but a brace body the author wrote on a single line is preserved as-is when it
> holds one simple statement, contains no inner comment, and fits within the
> 100-column width** (author-intent preservation, matching S69 for dot-chains);
> single blank line…

Append to S44:

> *Revised by D-FMT1 (owner, <date>):* fmt output is **idempotent**
> (`fmt(fmt(x)) == fmt(x)`) but no longer a strict canonical function of the AST —
> a single-statement body's line shape follows the author's source. This is an
> intentional trade of layout canonicality (non-binding per philosophy #4) for
> author-respecting output (philosophy #2).

Add a log row to `docs/spec/syntax-decisions.md` (after `:2578`):
`| <date> | D-FMT1 | fmt preserves author single-line bodies (revises S44) | owner |`

---

## Dependency: D-IF3 / if-eq-dispatch

The `if subject == { … }` dispatch sugar (decision **D-IF3**, plan
`tools/Tower/docs/sidequests/if-eq-dispatch.md` — already written, gated on D-IF3)
introduces single-line dispatch **arms** like `"active" -> connect()`. That plan's
"Formatter (D-FMT1, single-line arms)" section already names this dependency: its
braceless single-line arms only survive if the formatter stops forcing arm bodies
multiline. For those arms to
*survive* `jet fmt`, the formatter must stop forcing arm bodies multiline — which is
exactly what D-FMT1 decides at `fmt_switch_arm` (`Statements.rs:259`). Without
D-FMT1, every `"active" -> connect()` an author writes is reflowed to:

```jet
"active" -> {
    connect()
}
```

defeating the point of the dispatch sugar. So **D-FMT1 is the gate that makes
D-IF3's single-line arms stick.** D-IF3 should additionally decide whether arms may
be **braceless** when single-line (`"active" -> connect()` vs
`"active" -> { connect() }`) — that brace question is D-IF3's, but the
keep-it-on-one-line question is D-FMT1's. if-eq-dispatch.md already points at this
file; update its stale note (`if-eq-dispatch.md:252`, "there is no `fmt-single-line`
sidequest file yet") to cite this plan once it is reviewed.

---

## Tests (add to `tests/fmt.rs`)

All must hold *in addition to* the existing `fmt_is_idempotent_on_examples`
(`:19`), which must keep passing unchanged.

1. `fmt_preserves_single_line_if` — `if ready { launch() }\n` round-trips byte-equal;
   second pass stable.
2. `fmt_preserves_multiline_if` — the 3-line form stays 3 lines; stable.
3. `fmt_single_line_comment_forces_expand` — `if ready { launch() /* go */ }`
   expands (gate c); stable in expanded form.
4. `fmt_single_line_over_width_forces_expand` — a one-line body whose rendered width
   exceeds 100 at indent expands (gate d); stable.
5. `fmt_single_line_nested_forces_expand` — `if a { if b { x() } }` expands the outer
   (gate b: inner stmt not simple); stable.
6. `fmt_preserves_single_line_dispatch_arm` — once D-IF3 lands, `"active" -> connect()`
   survives; stable. (Co-owned with if-eq-dispatch.md.)
7. Idempotence smoke: each of the above re-runs fmt and asserts pass 2 == pass 1.
