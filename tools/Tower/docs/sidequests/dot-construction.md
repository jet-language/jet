# c158 — Dot-inferred construction `.{ }` / `T.{ }` (D-DOTCTOR1=A)

**Status:** Ratified 2026-06-25, not yet implemented. Clean break from U18.
Build plan, handoff-ready.

## Goal

One leading-dot rule for every inferred construction — structs and enums alike:

- `.{ name: …, grade: .A }` — construct when the type is known from context
  (binding annotation, field type, param type, `-> T` return).
- `T.{ … }` — construct when the type must be named (no inferable context).
- `.A` — leading-dot enum variant, type from context (the enum analog of `.{ }`);
  the named form `T.A` already ships (S30).

This replaces U18's bare `{ … }` inferred constructor. Per I8 (one canonical
mechanism), `T.{ … }` becomes the canonical named struct construction and S29's
no-dot `T { … }` retires into it (see Open Owner-Q for scope confirmation).

## Current state (verified, file:line)

**What U18 is today.** U18 (D-INFER-CTOR, ratified 2026-06-16) =
"inferred constructors via expected type": when a value's expected type is known,
the constructor type name is *optional* and a bare `{ … }` elaborates to it.
Spec: `docs/spec/syntax-decisions.md:1612-1625`; `docs/spec/spec.md:1116`,
`:1124-1126`. Decision-log row `docs/spec/syntax-decisions.md:2856`.

U18 is implemented **only in the jetpack module-eval (jetos) surface**, not in
general expression position:
- Parser: `Source/Parser/Modules.rs:360-393` (`system_lit` + `opt_record_type` —
  the type name `System`/`Service`/`Image` before `{` is optional; `None` = bare
  `{ }`). Also `:434` (`services_map`), `:513` (`image_lit`).
- AST: `Source/AST.rs:622-720` — `SystemLit`/`ServiceLit`/`ImageLit` carry
  `explicit_type: Option<Span>` (Some = author wrote the name, None = bare).
- Sema/eval (field-check + capture): `Source/Jetpack/ModuleEval/Eval.rs:155`,
  `Source/Jetpack/ModuleEval/System.rs:1,23,124`, `Source/Jetpack/ModuleEval/mod.rs:335,457`.

In **general expression position** there is no bare `{ }`: `Expr::StructLit`
(`Source/AST.rs:1824-1835`) always carries a required `type_name: String` (S29,
`syntax-decisions.md:291`), and `Expr::EnumLit` (`:1836-1842`) always carries a
required `type_name` + `variant` (S30, `:298`). So today every general struct
literal is `T { … }` and every enum literal is `T.Variant(…)`.

**The enum-variant dot already shipped** = `T.Variant` (S30). Parsing: the
postfix `.` after an uppercase ident name builds `EnumLit` /
`StructLit` — `Source/Parser/Expressions.rs:1169` (`T { … }` → `struct_lit_after_name`)
and `:1172-1205` (`T.member(…)` → enum lit / assoc call). There is **no
leading-dot primary today**: `expr_primary` (`Source/Parser/Expressions.rs:765`)
has no `TokKind::Dot` arm — `.A` / `.{ }` at the start of an expression do not
parse. `.Variant` must be added.

**Type-from-context machinery to reuse.** `self.expected_type:
Option<Type>` is threaded through the checker and saved/restored around nested
values. Struct-lit fields already set it per field:
`Source/Sema/CheckerItems.rs:550-560` (`check_struct_lit` sets
`self.expected_type = Some(field_ty)` before inferring each field value); list
elems `Source/Sema/CheckerInfer/expr.rs:619-682`; call args
`Source/Sema/CheckerCoreLib.rs:69-72`. `Expr::Absent` (bare `null`) already reads
`self.expected_type` and errors E0308 when absent
(`Source/Sema/CheckerInfer/expr.rs:504-535`) — the exact pattern the inferred
`.{ }` / `.Variant` checks copy. The literal checkers are
`check_struct_lit` (`CheckerItems.rs:461`) and `check_enum_lit` (`:678`); dispatch
at `Source/Sema/CheckerInfer/expr.rs:480-494`.

**Formatter emission.** `Source/Formatter/Expressions.rs:358-389` emits
`StructLit` as `type_name{…}` (flush, S29-FLUSH); `:391-417` emits `EnumLit` as
`type_name.variant(args)`. Neither can emit a dot-inferred form — both assume a
present `type_name`. Round-trip + a fmt STABILITY test are required for the new
forms (project memory: idempotence alone misses dropped tokens — `jet fmt`
silently corrupted serde markers/turbofish for months).

**Examples that use U18 (migration surface).**
- `examples/jetpack-typed/system.jet` — `system.my-host: { … }`, each
  `service: { enable: true }`, `image.halcyon-iso: { … }`.
- `examples/jetpack-config/config.jet` — `system.halcyon: { … }`, `openssh: { … }`.
- Their golden outputs and the U18 prose in `system.jet`/`config.jet` header
  comments (and `docs/spec/spec.md:1116,1124`) migrate.

Diagnostics: next free code is **E0320** (`docs/spec/diagnostics.md:178-195`,
last used E0319). Relevant existing codes: E0119 (no such type), E0303 (struct
field errors), E0304 (unknown variant), E0308 (bare `null` needs known type — the
template for the new "no inferable type" error).

## Decision (ratified, verbatim)

D-DOTCTOR1=A — replace U18 with one leading-dot rule for EVERY inferred
construction, structs and enums alike: `.{ name: …, grade: .A }` when the type is
known from context (binding annotation, etc.), `T.{ … }` when it must be named,
matching the enum-variant dot already shipped. Coexist-with-bare-`{}` (B) and
keep-U18 (C) rejected.

**Owner-Q DEFAULTS (confirmable — baked as this plan's assumption):**
- `.{}` is the empty/unit construct (zero-field record / unit-like).
- `.{ }` works in **return position** when `-> T` supplies the type.
- Positional `T.(a, b)` is **deferred** — named-fields only for v1.

## Implementation (staged)

Order: parser → sema → codegen → formatter → migrate → diagnostics → example →
tests → docs. Write the failing fixture first at each user-visible stage (I4/I5).

### 1. Syntax registry (I7)
`Source/Syntax.rs`: register the `.{` inferred-construction sigil and the
leading-dot variant marker under decision id `D-DOTCTOR1`. No new keyword; reuses
`TokKind::Dot` + `TokKind::LBrace`. Add a doc-comment row tying both to the id.

### 2. Parser (`Source/Parser/Expressions.rs`)
Add a `TokKind::Dot` arm to `expr_primary` (~`:765`):
- `.` `{` → inferred struct/record construction. Parse the field list exactly
  like `struct_lit_after_name` but with no type name. Empty `.{}` is legal
  (the empty/unit construct). Build `Expr::StructLit` with `type_name`
  empty/`None` (see AST change).
- `.` `Ident` (uppercase, not followed by `{`) → leading-dot enum variant.
  Parse optional `(args)`. Build `Expr::EnumLit` with `type_name` empty/`None`.
- Disambiguate from existing leading-dot uses: there is none in primary position
  today, but guard the `..` (range) and `.[` (fan-out is postfix only) cases.

Extend the **named** dot form: after an uppercase ident name, accept `T` `.` `{`
→ named struct construction (`T.{ … }`), alongside the existing `T { … }`
(`:1169`) and `T.Variant` (`:1172`). `T.{` and `T.Member` share the leading
`T.`; branch on `{` vs ident after the dot.

AST (`Source/AST.rs`): make `StructLit.type_name` and `EnumLit.type_name` carry
"inferred" — cleanest is `type_name: Option<String>` (None = dot-inferred), or a
sentinel empty string with an `inferred: bool`. `Option<String>` is clearer;
update all match sites (Taint/Effects/State/Purity/Captures/Formatter/Codegen).
`T.{ … }` and `T { … }` both produce `Some(name)`.

### 3. Sema (`Source/Sema/CheckerItems.rs` + dispatch in `CheckerInfer/expr.rs`)
`check_struct_lit` / `check_enum_lit` gain a leading branch: when `type_name` is
None (dot-inferred), resolve the type from `self.expected_type`:
- Struct: expected type must be a `Named`/`Apply` struct → use its name, then run
  the existing field-check body unchanged. `.{}` against a zero-field struct or a
  unit-like type is fine.
- Enum (`.Variant`): expected type must be an enum that has `variant` → reuse the
  existing variant/payload check (`check_enum_lit:686-723`). This is the same
  resolution the `==`-pattern bare-variant path uses against a known subject type.
- No inferable expected type → **E0320** (new): "`.{ … }` / `.X` needs a known
  type here", fix = "name it: `T.{ … }` / `T.X`". Model on the E0308 absent-null
  branch (`CheckerInfer/expr.rs:504-535`).
- Expected type present but not a struct/enum, or variant absent → reuse E0303 /
  E0304 against the *inferred* type (typos still report against it).

Return position: a function body's tail expression already sees the return type
as expected (verify the return-type plumbing sets `expected_type`; if not, set it
where the body tail is checked). This satisfies the `-> T` default.

### 4. Codegen (`Source/Codegen/…`, I3 — dumb)
By the time codegen runs, sema has resolved the concrete type. Codegen must NOT
re-infer. Two options: (a) sema rewrites the dot-inferred node's `type_name` to
the resolved `Some(name)` in place (preferred — matches the `resolved_ret` /
`Todo.expected_type` pattern at `AST.rs:1820`, `:1856`), so existing
`StructLit`/`EnumLit` lowering is unchanged; or (b) carry a `resolved_type` field.
Use (a): codegen sees only fully-named literals. No new lowering logic.

### 5. Formatter (`Source/Formatter/Expressions.rs`) — round-trip + STABILITY test
Emit dot-inferred forms verbatim:
- `StructLit` `type_name = None` → `.{…}` (flush, S29-FLUSH spacing).
- `StructLit` `Some(n)` → `n.{…}` (the new named form) — **decide with Open
  Owner-Q whether the formatter canonicalizes `T { }` → `T.{ }`**.
- `EnumLit` `None` → `.variant(args)`; `Some(n)` → `n.variant(args)` (unchanged).
Add a fmt STABILITY test in `tests/fmt.rs` covering every form (`.{}`, `.{a:1}`,
`T.{…}`, `.A`, `.A(x)`) — not just idempotence; assert the exact emitted bytes.

### 6. Migrate U18 + (per Open Owner-Q) S29
- Definite (U18): rewrite `examples/jetpack-typed/system.jet` &
  `examples/jetpack-config/config.jet` bare `{ … }` → `.{ … }`; update the U18
  prose in both headers and `docs/spec/spec.md:1116,1124-1126`. Re-bless their
  goldens.
- Module-eval parser (`Source/Parser/Modules.rs:360-393` etc.): `opt_record_type`
  must accept the new dot form (`.{` and `System.{`); `explicit_type` semantics
  unchanged (None = `.{`, Some = `System.{`). Bare `{ }` (no dot) becomes a
  teaching error pointing at `.{ }` (clean break — no silent alias).
- If S29 retires (recommended, Open Owner-Q): migrate every `T { … }` and
  destructuring `T{ x, y }` (S74) across `examples/`, `tests/`, prelude, and
  re-bless all touched goldens + ui snapshots; flip `T { … }` (no dot) to a
  teaching error E0320-adjacent pointing at `T.{ … }`.

### 7. Diagnostics (I4)
Add **E0320** to `docs/spec/diagnostics.md` table + what/why/fix prose, with a
`tests/ui` fixture: a `.{ … }` / `.A` in a bare binding with no annotation →
E0320 pointing at `T.{ … }` / `T.A`. (No snapshot → the diagnostic doesn't exist.)

### 8. Example + golden (I5)
New `examples/features/NN_dot_construction.jet` + `expected/NN_dot_construction.out`
exercising: inferred struct via binding annotation, inferred via field type,
inferred via `-> T` return, `.A` unit variant, `.A(x)` payload variant, named
`T.{ … }` where no context exists, and `.{}` empty. Golden test enforces it.

### 9. Spec/decisions docs
- `docs/spec/spec.md`: replace the U18 inferred-constructor prose (`:1116`,
  `:1124-1126`) with the `.{ }` / `T.{ }` / `.A` rule.
- `docs/spec/syntax-decisions.md`: mark U18 (`:1612`) and (if retired) S29
  (`:291`) **superseded by D-DOTCTOR1**; update the D-DOTCTOR1 row (`:2942`) from
  "not yet implemented" to implemented with the build date + touched files.

## Sequencing / gates

- AST `type_name: Option<String>` change fans out to every `Expr::StructLit` /
  `Expr::EnumLit` match site — do this first, mechanically, in one pass, build
  green, before behavior changes.
- Sema-rewrites-resolved-name (stage 4a) lets codegen stay untouched — verify
  that decision early; it gates how much codegen work exists (≈ none).
- The **Open Owner-Q below gates stage 6's scope** (U18-only vs full S29
  retirement). The U18 migration and all of stages 1–5, 7, 8 proceed regardless;
  only "retire `T { }`" waits on the answer. Do the U18 migration now; stage the
  S29 flip behind the gate.
- Full suite once at the end; targeted `--test fmt` / `--test golden` /
  `--test ui` while iterating (memory: targeted tests during iteration).

## Open Owner-Q

**Does `T.{ … }` retire S29's no-dot `T { … }` (and the S74 destructuring
`T{ x, y }` / S29-SHORT shorthand), or only U18's bare `{ }`?**

The ratified text literally says "replace **U18**." But the card also introduces
`T.{ … }` as the named struct-construction form, "matching the enum-variant dot."
For enums there is no no-dot spelling — it is always `T.Variant`. Keeping S29's
`T { … }` alongside a new `T.{ … }` gives two spellings for one job → an I8
violation in the shipped result. So the dot symmetry the owner asked for cannot
hold while `T { … }` survives.

**Recommendation: retire S29 `T { … }` into `T.{ … }`** — the full clean break,
true enum/struct symmetry, no I8 carve-out. Cost is a wide migration (~all struct
literals + destructuring patterns across examples/tests/prelude + re-bless), which
is not a deterrent. The plan implements `.{ }` / `T.{ }` / `.A` and migrates U18
unconditionally; this answer only decides whether stage 6 also flips every
`T { … }` and turns the no-dot form into a teaching error. Alternative (narrow):
keep `T { … }` as the named form and drop the `T.{ … }` spelling for structs — but
that abandons the dot symmetry the card states, so it is not recommended.
