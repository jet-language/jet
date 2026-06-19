# Sidequest: remove the space before a constructor block

## Goal

The owner wants struct construction to read `Point{x: 1.0}`, not
`Point {x: 1.0}` — the type name and its `{ … }` field block sit flush, the
way a call's `(` hugs its callee. This is a **formatter-canonical-style change**
plus a **ratified-syntax amendment** (S29 currently shows the space; S44 is the
formatter-style decision). The parser already accepts both spacings unchanged,
so no grammar work is needed — but because S29 is Ratified, the owner must
ratify the amendment before any code lands.

## Current state (verified)

- **Parser is whitespace-insensitive already.** `Point{x:1}` and `Point {x:1}`
  parse identically. `expr_primary` (src/parser/exprs.rs, `TokKind::Ident` arm)
  recognizes a struct literal purely by token lookahead — `Ident` followed by
  `LBrace` when `allow_struct_lit` is true — then calls `struct_lit_after_name`
  (src/parser/exprs.rs). There is **no span-adjacency check** between the name and
  the `{`, so spacing never affects parsing. **No parser change is required.**
- **The space lives in exactly one formatter site.** The `Expr::StructLit` arm in
  `fmt_expr` (src/fmt/exprs.rs) writes `self.write(" {")` (the leading space is
  the whole bug). This one arm covers both plain literals and the
  `import_ns`-qualified form (`ns.Point { … }`). Inner fields already emit **no**
  space after `{` (`{x: 1.0}`) and `": "` after each field name.
- **Declaration/block `{` sites must NOT change.** fmt has many `" {"` writes
  across src/fmt/{items,stmts}.rs; all the others are declaration or block
  openers — `struct X {`, `enum {`, `fn f() {`, `impl X {`, `trait {`,
  lambda/`if`/`loop` blocks. S44 ratifies **same-line `{`** for these; they stay
  spaced. Only the `Expr::StructLit` arm in src/fmt/exprs.rs is in scope.
- **Destructuring is a separate path that still has the space.** A bind pattern
  `Point { x, y } :: make()` is formatted by `fmt_bind_pattern`
  (`BindPattern::Struct`) in **src/fmt/stmts.rs** — `self.write(" { ")` …
  `self.write(" }")` — and is round-trip-asserted by
  **`fmt_preserves_destructuring_targets`** (tests/fmt.rs). This is *matching*, not
  *construction*; see Decision 1.
- **Examples use the spaced form today** (I5 corpus): e.g.
  examples/features/10_structs.jet:11,15 (`Point {x: 1.0, …}`),
  25_traits.jet:54,60, 24_callbacks.jet:15, 35_zerocopy.jet:36-38,
  47_library.jet:37,63,71, 57_http_server.jet:9,21, 27_printable.jet:5. Re-fmt
  canonicalizes these to the flush form (golden tests re-bless).
- **ui stderr snapshots echo fixture source, not formatter output.** Snapshots
  like tests/ui/struct_missing_field.stderr already render `Point {x: 1}`
  (their fixture `.jet` was hand-written). They change **only if the fixture
  `.jet` is rewritten**, which is a separate choice (Decision 3).
- **No ambiguity is introduced.** Condition position parses with
  `expr_no_struct_lit` (src/parser/exprs.rs), so `if Point{x:1} { … }` still
  requires parens around the literal regardless of spacing — flush spacing
  changes nothing here.
- **Out of scope / pre-existing:** the `as_trait` field on `Expr::StructLit` is
  not rendered by fmt today (only `import_ns` is). Leave it alone; not this
  sidequest.

## Proposed approach (workflow loop)

Pre-code gate (owner): ratify the S29 amendment and resolve Decisions 1–3
below. Do not touch code until ratified.

Then, following the loop:

1. **Failing test first** — add a fmt round-trip test (tests/fmt.rs) asserting
   `Point{x: 1.0, y: 2.0}` is the canonical output for a struct literal, plus
   idempotency (`fmt(fmt(src)) == fmt(src)`). It fails against current ` {`.
2. **Spec** — amend S29 in docs/spec/syntax-decisions.md (show the flush form;
   note "no space between the type name and its field block"); add a one-liner
   to the S44 row in docs/spec/syntax-decisions.md and to docs/spec/spec.md's
   formatter-style section.
3. **Parser** — no change (verified). Record "no parser change" in the commit.
4. **Sema** — no change.
5. **Codegen** — no change (codegen emits Rust, not Jet surface text).
6. **fmt** — in the `Expr::StructLit` arm (src/fmt/exprs.rs) change
   `self.write(" {")` → `self.write("{")`. If Decision 1 = "apply to
   destructuring too", also change `fmt_bind_pattern`'s `BindPattern::Struct` arm
   (src/fmt/stmts.rs) `" { "` → `"{"` and `" }"` → `"}"`, and update
   `fmt_preserves_destructuring_targets`'s expected string (tests/fmt.rs).
7. **Diagnostics** — no new diagnostic. Any diagnostic text that quotes a
   constructor literal in prose (none found that show the space for
   *construction*) stays put; re-bless only if output drifts.
8. **Examples/tests** — re-fmt examples/ to canonicalize (`jet fmt`), re-bless
   golden + ui-fmt snapshots with `UPDATE_EXPECT=1`. Per Decision 3, decide
   whether to also rewrite ui *stderr* fixture `.jet` sources.

## Decisions for the owner (ratify before coding)

### Decision A — amend S29 (the core gate)

S29 is Ratified with the spaced form `Type { field: expr, … }`. Flush spacing
is a user-facing syntax change, so it needs ratification even though the owner
asked for it.

- **Option A1 — flush construction (recommended).**
  Before: `p :: Point {x: 3.0, y: 4.0}`
  After:  `p :: Point{x: 3.0, y: 4.0}`
  Rationale: reads like a call hugging its args; one canonical style; trivial,
  isolated change.
- **Option A2 — keep the space (status quo).** Reject the request. Only if the
  owner decides the flush form reads worse than expected.

Recommendation: **A1.**

### Decision 1 — does the no-space rule extend to destructuring patterns?

The owner's ask names the *constructor block*. Destructuring (matching) is a
distinct path (`fmt_bind_pattern`'s `BindPattern::Struct` arm in
src/fmt/stmts.rs) that today keeps the space.

- **Option 1A — construction only.**
  Construction: `Point{x: 1, y: 2}` (flush)
  Destructure:  `Point { x, y } :: make()` (spaced, unchanged)
  Effect: literally what was asked; the two forms look different.
- **Option 1B — both, for symmetry (recommended).**
  Construction: `Point{x: 1, y: 2}`
  Destructure:  `Point{x, y} :: make()`
  Effect: build-vs-match use the same flush shape; updates
  `fmt_preserves_destructuring_targets` (tests/fmt.rs).
  Rationale: the whole point of fmt is to kill this kind of asymmetry; building
  a `Point{…}` and matching a `Point{…}` should look alike. Low stakes either way.

Recommendation: **1B**, but it is the owner's call.

### Decision 2 — field-colon spacing (confirm, don't assume)

The owner wrote `Point{x:1}` with no space after the colon either. That almost
certainly contradicts S4/S44 (annotations and map/field colons get `": "`),
so we read it as terse shorthand, not a real ask.

- **Option 2A — keep `x: 1` (recommended).** `Point{x: 1, y: 2}` — colon
  spacing matches every other `: ` in the language.
- **Option 2B — drop the colon space too.** `Point{x:1, y:2}` — would also
  force reconsidering `: ` in annotations/maps for consistency. Not recommended.

Recommendation: **2A.** Surfaced only so the owner confirms rather than us
guessing from the shorthand.

### Decision 3 — canonicalize ui stderr fixture sources?

ui *stderr* snapshots echo their fixture `.jet` source verbatim. Some already
show `Point {x: 1}` (spaced) in error frames (e.g. struct_missing_field,
struct_extra_field, struct_duplicate_field, ref_field_dangles_local).

- **Option 3A — rewrite the fixtures to flush form (recommended).** Error
  frames then show `Point{x: 1}`, matching what users will type post-fmt.
- **Option 3B — leave fixtures as-is.** Less churn now; error frames briefly
  show the old spacing until touched later.

Recommendation: **3A** for a consistent corpus, but it is cosmetic and can be a
follow-up.

## Test / acceptance checklist

- [ ] S29 amended in docs/spec/syntax-decisions.md; S44 + spec.md formatter
      section note the flush rule.
- [ ] New tests/fmt.rs round-trip: `Point{x: 1.0, y: 2.0}` is canonical output;
      `fmt(fmt(src)) == fmt(src)` (idempotent).
- [ ] The `Expr::StructLit` arm (src/fmt/exprs.rs) emits `{` not ` {`; the
      `import_ns` path (`ns.Point{…}`) also flush.
- [ ] (If Decision 1 = 1B) `BindPattern::Struct` arm (src/fmt/stmts.rs) flush;
      `fmt_preserves_destructuring_targets` expectation updated (tests/fmt.rs);
      destructure round-trip still idempotent.
- [ ] Declaration/block `{` sites unchanged — spot-check `struct`, `enum`, `fn`,
      `impl`, `trait`, `if`, `loop`, lambda fmt output still spaced (S44).
- [ ] `jet fmt` re-canonicalizes examples/; golden tests green after re-bless.
- [ ] (If Decision 3 = 3A) ui fixture `.jet` sources rewritten; ui stderr
      snapshots re-blessed.
- [ ] `if Point{x:1} { … }` still requires parens (no new ambiguity) — confirm
      a parse test covers condition position.
- [ ] Full `cargo test` green; no parser/sema/codegen diff in the change.
