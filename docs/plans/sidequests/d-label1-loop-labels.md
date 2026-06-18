# D-LABEL1 — Named loops and labeled break/continue

**Status: ratified 2026-06-18 (option B — `@name` label)** — recorded in
`syntax-decisions.md` (amends S19, S23); ready to implement.

A loop carries an **`@name` label**; `break @name` / `continue @name` target it.
Reuses the S82 `@` marker sigil in a **new inline position** (immediately before
`loop`), so it can never be confused with an S61 labeled argument. Rust-style
`'outer` was rejected (S41 already makes `'x'` a char literal — lexer clash).

## Plan

1. **Lexer** — no new tokens. `@ident` already lexes (S82 attribute prefix).
2. **Parser** — `loop_stmt → ('@' IDENT)? loop_kind block`, producing a
   `LabeledLoop { label, body }` node. `break_stmt → 'break' ('@' IDENT)?`,
   `continue_stmt → 'continue' ('@' IDENT)?` (no trailing `;` under S6-R = B).
   Disambiguate `@name loop` (label) from `@Marker` declaration attributes (S82)
   by what follows: a `loop` keyword → label; a `struct`/`enum`/`fn` → attribute.
3. **Sema** — maintain a label-scope stack. Resolve `break @name` / `continue
   @name`; `E_UNDEFINED_LABEL` if not in scope (help lists labels in scope).
4. **Codegen** — Rust labeled loops map directly: Jet `@outer` → Rust `'outer:`,
   `break @outer` → `break 'outer`.
5. **`src/syntax.rs`** — record the `@`-label usage under D-LABEL1 (ratified);
   note it extends S82's `@` positions.
6. **Diagnostics** — `E_UNDEFINED_LABEL`, `E_LABEL_NOT_LOOP` (`@name` followed by
   a non-loop construct). Claim in `docs/spec/diagnostics.md` (I4) with ui
   snapshots.
7. **Example** — `examples/features/labeled_loops.jet` (nested grid scan,
   `break @outer`). Golden test + expected output.
