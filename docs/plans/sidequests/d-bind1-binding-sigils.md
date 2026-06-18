# D-BIND1 — Binding sigils

**Status: ratified 2026-06-18 (option A — full sigils)** — recorded in
`syntax-decisions.md` (amends S2); ready to implement.

Switch fully to Odin-style sigils: `name :: expr` (immutable), `name := expr`
(mutable). `val` / `var` are retired to teaching errors. The owner accepted
**spending the `::` token** on immutable bindings — S83 (external definitions)
must now choose a different separator.

## Plan

1. **Lexer** (`src/lexer.rs`) — add `::` and `:=` as two-char tokens. Remove
   `val` / `var` from the keyword set; they become teaching-error identifiers.
2. **Parser** — `binding_stmt → ident (':' type)? ('::' | ':=') expr`
   (no trailing `;` under S6-R = B; the lexer-inserted terminator ends it).
3. **Sema** — `::` → immutable binding (was `val`); `:=` → mutable binding (was
   `var`). `=` stays reassignment of an existing `:=` binding (S17).
4. **Diagnostics** — `E_KEYWORD_RETIRED` for `val` / `var`, teaching
   `name :: value` / `name := value`. Claim in `docs/spec/diagnostics.md` (I4)
   with ui snapshots.
5. **`src/syntax.rs`** — remove the S2 `KW_VAL` / `KW_VAR` constants; add the
   `::` / `:=` sigil constants tagged with decision **D-BIND1** (now ratified, so
   they may land in `syntax.rs` per `tests/decisions.rs`).
6. **Teach `jet fmt` the canonicalization** (`src/fmt.rs`) — `val name = e` →
   `name :: e`, `var name = e` → `name := e`. The example corpus is NOT
   hand-migrated here; it is migrated mechanically in the shared final
   consolidation phase by running `jet fmt` over `examples/`, then re-blessing
   golden + ui snapshots once (owner: migrate via fmt, not by hand).
7. **Diagnostic code: `E0985`** — retired binding keyword (`val`/`var`).
7. **Cross-decision** — S83's `TypeName::item` external form is now blocked on a
   new separator (note already recorded in `syntax-decisions.md`); no action
   here beyond not reusing `::`. `extern rust "rust::path"` strings (S50) are
   unaffected — the `::` there is inside a string literal.

## Sequencing note

D-BIND1, S6-R, and D-IF1 all touch every example. Implement each feature's
grammar/sema/codegen/fmt + its own new tests first; defer the corpus migration
to a single final phase that runs `jet fmt` over `examples/` once and re-blesses
all snapshots together.
