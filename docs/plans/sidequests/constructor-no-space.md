# Sidequest: remove the space before a constructor block

**Status:** ratified 2026-06-19 (S29-FLUSH = A) — ready to implement on owner's word.

## Goal

The owner wants struct construction to read `Point{x: 1.0}`, not
`Point {x: 1.0}` — the type name and its `{ … }` field block sit flush, the
way a call's `(` hugs its callee. This is a **formatter-canonical-style change**
plus a **ratified-syntax amendment** (S29-FLUSH amends S29; S44 is the
formatter-style decision). The parser already accepts both spacings unchanged,
so no grammar work is needed.

## Current state (verified)

- **Parser is whitespace-insensitive already.** `Point{x:1}` and `Point {x:1}`
  parse identically. `expr_primary` (Source/Parser/Expressions.rs, `TokKind::Ident` arm)
  recognizes a struct literal purely by token lookahead — `Ident` followed by
  `LBrace` when `allow_struct_lit` is true — then calls `struct_lit_after_name`
  (Source/Parser/Expressions.rs). There is **no span-adjacency check** between the name and
  the `{`, so spacing never affects parsing. **No parser change is required.**
- **The space lives in exactly one formatter site.** The `Expr::StructLit` arm in
  `fmt_expr` (Source/Formatter/Expressions.rs) writes `self.write(" {")` (the leading space is
  the whole bug). This one arm covers both plain literals and the
  `import_ns`-qualified form (`ns.Point { … }`). Inner fields already emit **no**
  space after `{` (`{x: 1.0}`) and `": "` after each field name.
- **Declaration/block `{` sites must NOT change.** fmt has many `" {"` writes
  across Source/Formatter/{items,stmts}.rs; all the others are declaration or block
  openers — `struct X {`, `enum {`, `fn f() {`, `impl X {`, `trait {`,
  lambda/`if`/`loop` blocks. S44 ratifies **same-line `{`** for these; they stay
  spaced. Only the `Expr::StructLit` arm in Source/Formatter/Expressions.rs is in scope.
- **Destructuring is a separate path that still has the space.** A bind pattern
  `Point { x, y } :: make()` is formatted by `fmt_bind_pattern`
  (`BindPattern::Struct`) in **Source/Formatter/Statements.rs** — `self.write(" { ")` …
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
  `expr_no_struct_lit` (Source/Parser/Expressions.rs), so `if Point{x:1} { … }` still
  requires parens around the literal regardless of spacing — flush spacing
  changes nothing here.
- **Out of scope / pre-existing:** the `as_trait` field on `Expr::StructLit` is
  not rendered by fmt today (only `import_ns` is). Leave it alone; not this
  sidequest.

## Proposed approach (workflow loop)

Pre-code gate: passed — S29-FLUSH ratified 2026-06-19. Implement on owner's word.

Then, following the loop:

1. **Failing test first** — add a fmt round-trip test (tests/fmt.rs) asserting
   `Point{x: 1.0, y: 2.0}` is the canonical output for a struct literal, plus
   idempotency (`fmt(fmt(src)) == fmt(src)`). It fails against current ` {`.
2. **Spec** — S29-FLUSH is recorded in docs/spec/syntax-decisions.md (ratified
   2026-06-19). Add a one-liner to the S44 row and to docs/spec/spec.md's
   formatter-style section noting the flush rule for struct literals and
   destructuring patterns.
3. **Parser** — no change (verified). Record "no parser change" in the commit.
4. **Sema** — no change.
5. **Codegen** — no change (codegen emits Rust, not Jet surface text).
6. **fmt** — in the `Expr::StructLit` arm (Source/Formatter/Expressions.rs) change
   `self.write(" {")` → `self.write("{")`. Also change `fmt_bind_pattern`'s
   `BindPattern::Struct` arm (Source/Formatter/Statements.rs) `" { "` → `"{"` and `" }"` →
   `"}"`, and update `fmt_preserves_destructuring_targets`'s expected string
   (tests/fmt.rs). Both construction and destructuring go flush (S29-FLUSH = A).
7. **Diagnostics** — no new diagnostic. Any diagnostic text that quotes a
   constructor literal in prose (none found that show the space for
   *construction*) stays put; re-bless only if output drifts.
8. **Examples/tests** — re-fmt examples/ to canonicalize (`jet fmt`), re-bless
   golden + ui-fmt snapshots with `UPDATE_EXPECT=1`. Per Decision 3, decide
   whether to also rewrite ui *stderr* fixture `.jet` sources.

## Decisions (resolved 2026-06-19)

### S29-FLUSH = A — flush construction and destructuring. RESOLVED.

S29 is amended. Canonical form:
- Before: `p :: Point {x: 3.0, y: 4.0}` / `Point { x, y } :: make()`
- After:  `p :: Point{x: 3.0, y: 4.0}` / `Point{x, y} :: make()`

Flush applies to both construction (`Expr::StructLit` in Source/Formatter/Expressions.rs) and
destructuring (`BindPattern::Struct` in Source/Formatter/Statements.rs). This is option 1B —
build-vs-match use the same flush shape.

### Colon spacing unchanged — RESOLVED.

`Point{x: 1, y: 2}` — `: ` spacing (S4/S44) is unchanged. The owner's terse
shorthand `Point{x:1}` was shorthand, not a style request.

### Decision 3 — canonicalize ui stderr fixture sources? OPEN.

ui *stderr* snapshots echo their fixture `.jet` source verbatim. Some already
show `Point {x: 1}` (spaced) in error frames (e.g. struct_missing_field,
struct_extra_field, struct_duplicate_field, ref_field_dangles_local).

- **Option 3A — rewrite the fixtures to flush form.** Error frames then show
  `Point{x: 1}`, matching what users will type post-fmt.
- **Option 3B — leave fixtures as-is.** Less churn now; error frames briefly
  show the old spacing until touched later.

Recommendation: **3A** for a consistent corpus, but it is cosmetic and can be a
follow-up.

## Test / acceptance checklist

- [ ] S29-FLUSH recorded in docs/spec/syntax-decisions.md; S44 + spec.md
      formatter section note the flush rule for literals and destructuring.
- [ ] New tests/fmt.rs round-trip: `Point{x: 1.0, y: 2.0}` is canonical output;
      `fmt(fmt(src)) == fmt(src)` (idempotent).
- [ ] The `Expr::StructLit` arm (Source/Formatter/Expressions.rs) emits `{` not ` {`; the
      `import_ns` path (`ns.Point{…}`) also flush.
- [ ] `BindPattern::Struct` arm (Source/Formatter/Statements.rs) flush (`"{"`/`"}"`);
      `fmt_preserves_destructuring_targets` expectation updated (tests/fmt.rs);
      destructure round-trip still idempotent.
- [ ] Declaration/block `{` sites unchanged — spot-check `struct`, `enum`, `fn`,
      `impl`, `trait`, `if`, `loop`, lambda fmt output still spaced (S44).
- [ ] `jet fmt` re-canonicalizes examples/; golden tests green after re-bless.
- [ ] (Decision 3 open) ui fixture `.jet` sources rewritten to flush form if
      Decision 3 = 3A; otherwise deferred.
- [ ] `if Point{x:1} { … }` still requires parens (no new ambiguity) — confirm
      a parse test covers condition position.
- [ ] Full `cargo test` green; no parser/sema/codegen diff in the change.
