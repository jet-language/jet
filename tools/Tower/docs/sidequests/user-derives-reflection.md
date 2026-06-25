# c155 — User-authored derives + typed reflection (S56)

**Status:** active sidequest plan. Decision ratified (D-METADEPTH1=A, 2026-06-25).
Supersedes `tools/Tower/docs/plans/epoch-3/user-derives-reflection.md` (D-DX milestone,
2026-06-16) — folded in and reconciled to the 2026-06-25 ratifications.
Two user-facing surfaces still need ballots (see Open Owner-Q) → **stop at those gates**;
the read-API and re-entry plumbing below can start now.

## Goal

Let library authors author derives in typed Jet on top of the compiler's built-ins, and
let user code READ a type's shape (fields, types, attrs) via reflection. This is the v1
metaprogramming ceiling: read + derive, no rewrite/inject/macros. A user derive emits a
typed *source fragment* that re-enters lexer→parser→sema like hand-written code, so its
diagnostics pin at the user's trigger site.

```jet
derive Encode for T { … }   // author's body, run at build time, output re-checked

@MyWireFormat                // user marker triggers the derive on a struct
struct Event { … }
```

## Current state (verified, file:line)

**Built-in derive surface (the model to extend).**
- Marker syntax is `#[Name]` / `#[Name(arg)]`, parsed in `parse_marker_groups`
  at `Source/Parser/Items.rs:1771-1811`.
- `split_type_markers` (`Source/Parser/Items.rs:1817-1840`) lowers markers at parse time:
  `#[Codable]` → `Encode` + `Decode` (`:1821-1826`); serde container/field attrs returned
  raw (`:1827-1834`); any other name (incl. `Comparable`, user trait names) → `derives`
  (`:1836`).
- AST: `derives: Vec<(String, Span)>` on `StructDef` (`Source/AST.rs:986`) and `EnumDef`
  (`:1089`); `Marker` struct at `Source/AST.rs:967-972`.
- Marker/trait name constants: `Source/Syntax.rs:1314-1326` (`ATTR_CODABLE`, `ATTR_ENCODE`,
  `ATTR_DECODE`, serde attrs `Rename`/`Skip`/`Default`/`Flatten`/`RenameAll`/`Tag`/…);
  trait names `Source/Generics.rs:9-19` (`Printable`, `Equatable`, `Comparable`, `Serialize`,
  `Encode`, `Decode`).
- Sema validates derives but generates no code: `Source/Sema/CheckerCoreLib.rs:2599-2601`.
- **Codegen emits Rust directly** for the built-ins: `impl user_Encode` / `impl user_Decode`
  field-walks via `out.push_str(format!(…))` at `Source/Codegen/Items.rs:401-515`
  (Encode `:422-451`, Decode `:453-514`). `Comparable` → `PartialEq` derive flag in
  `Source/Codegen/Context.rs:677-681`.

  Note: built-in derive output is **Rust, generated at codegen**, not Jet source that
  re-enters sema. That is sound for compiler-owned built-ins on already-checked types (I3).
  It is NOT the path a *user* derive can use — user output must re-enter sema so its errors
  are Jet diagnostics pinned at the trigger (see D-CTCODEGEN1 reconciliation below).

**Reflection API: ABSENT.** No user-facing introspection exists. Compiler-internal only:
`struct_fields_of()` (`Source/Sema/CheckerItems.rs:375`), `field_names()`
(`Source/Sema/mod.rs:140`, used in `Source/Sema/Registration.rs:1067,1202,1258`). No
`type_info`/`TypeInfo`/`fields_of`/`introspect` in `Source/Comptime/`.

**Generated-code re-entry (D-CTCODEGEN1=A): NOT YET built.** No path today takes generated
Jet *source text* and re-feeds it through lexer→parser→sema. Built-in derives bypass this by
emitting Rust at codegen (`Source/Codegen/Items.rs:401-515`). The ratified decision frames
re-entry as "a standing rule the existing `#[Codable]` derive already follows" — accurate in
spirit (codegen is dumb, no AST injection) but the literal source-fragment-re-enters-sema
loop is unbuilt and is the core deliverable for *user* derives.

**`$name` splice (D-CTMARKER1=C, card c162): ABSENT.** No `$`/`Dollar`/`Splice` token in
`Source/Lexer/Tokens.rs` or `Source/Lexer/Scan.rs`. `$` currently unused in the lexer.

**Spec.** No S56 section in `docs/spec/spec.md` yet (only an S55 reference at
`docs/spec/spec.md:373`). Decisions live at `docs/spec/syntax-decisions.md`:
D-METADEPTH1 `:2948`, D-CTCODEGEN1 `:2945`, D-CTMARKER1 `:2949`; S56 currently marked
"Deferred to Epoch 3" `:2525`, S26 layering `:644-662`, S55 `:441-480`.

## Decision (ratified)

- **D-METADEPTH1=A** — v1 ceiling is reflection + derives ONLY. User code READS type info via
  reflection and authors derives (`derive Encode for T`) on top of S55 built-ins. No
  rewrite, no AST injection, no macros. B (read-only rejection lint) and C (full Jai, c154)
  stay off v1.
- **D-CTCODEGEN1=A** — generated code re-enters lexer→parser→sema exactly like hand-written
  code; never inject pre-parsed AST past the sema gatekeeper. A user derive's output is
  sema-checked, with diagnostics pinned at the user trigger site.
- **D-CTMARKER1=C** (card c162, splice) — `$name` marks a compile-time value woven into
  generated code; `comptime` keeps declaring bindings / `comptime if`. Not yet implemented;
  the user-derive output mechanism is its first consumer (see Sequencing).

## Implementation (staged)

Build in order; each stage is independently testable. Stages 1 and the early plumbing of 3
do not touch unsettled syntax; stages 2 and the surface of 1 are gated (Open Owner-Q).

**Stage 1 — Reflection read-API (type info).**
Expose, in the pure comptime subset, read-only introspection over a type: ordered fields,
each field's type, and the markers/attrs attached. Back it with existing internals
(`struct_fields_of` `Source/Sema/CheckerItems.rs:375`, `field_names` `Source/Sema/mod.rs:140`)
rather than new machinery. Typed and total: every query resolves in sema, errors are Jet
diagnostics (I4), never a runtime probe. Surface spelling is **Open Owner-Q #1** — build the
sema-side resolver behind a provisional name, wire the public spelling after the ballot.

**Stage 2 — User `derive` authoring surface.**
A library author writes a derive body that, given reflection info for the triggering type,
produces the impl. Parse the authoring form; register user trait names alongside the built-in
set (`Source/Generics.rs:9-19`) so `split_type_markers` (`Source/Parser/Items.rs:1836`) routes
an unknown marker to a user derive instead of erroring. The author body runs in the pure
comptime sandbox (no FFI/IO/time/random, per S26 layer-1 law). Authoring spelling +
orphan/coherence rules are **Open Owner-Q #2** — gated.

**Stage 3 — Generated-fragment re-enters sema (the D-CTCODEGEN1 loop).**
Build the missing pipeline: derive body emits a typed Jet *source fragment* (text), which is
lexed → parsed → sema-checked like hand-written code. Do NOT inject AST. Reuse the structural
shape of the `#[Codable]` path (`Source/Codegen/Items.rs:401-515`) for what an impl looks like,
but the user path stops at "produce Jet source" and lets the normal front end take it the rest
of the way. Splice points in the fragment use `$name` (D-CTMARKER1=C / c162) to weave computed
values — this is the hard dependency below. Carry the trigger Span through so re-entry
diagnostics resolve to the user's `@Marker` site, not synthesized text.

**Stage 4 — Diagnostics pinned at trigger site.**
New codes in `docs/spec/diagnostics.md` for: unknown/duplicate derive, derive body produces
ill-typed/non-compiling fragment (surfaced as a Jet error at the trigger, NOT an ICE — I2),
reflection query on an unsupported type, derive used on a type whose fields don't satisfy the
trait's needs. Every code gets a `tests/ui_lint` snapshot (I4). Re-entry errors must show the
user's marker span with the fragment error chained beneath.

**Stage 5 — Example + golden output.**
A runnable `examples/features/` example: author a small derive (e.g. a wire/format trait),
apply it to a struct, print/use the result; expected `.out` enforced by golden tests (I5). One
example exercising a deliberate derive-body error feeding a `ui_lint` fixture.

**Stage 6 — Tests.**
Unit: reflection resolver returns correct fields/types/attrs. Parser: authoring form + marker
routing. Re-entry: fragment round-trips through sema, span pins correctly. fmt: authoring
syntax emits + has a STABILITY round-trip test (formatter-roundtrip rule). Full suite green at
the end only.

**Stage 7 — Docs.**
Add S56 section to `docs/spec/spec.md` (it has none today; S55 ref at `:373`). Flip
`docs/spec/syntax-decisions.md:2525` S56 from "Deferred to Epoch 3" to ratified-and-building,
record the balloted spellings, log new keywords/sigils in `Source/Syntax.rs` with decision ids
(I7). Update D-METADEPTH1 / D-CTCODEGEN1 status notes to point at this card.

## Sequencing / gates

- **Hard dep on c162 (`$name` splice, D-CTMARKER1=C).** Stage 3's fragment weaving uses
  `$name` to inject computed values into generated source. The splice token does not exist in
  the lexer yet (`Source/Lexer/Tokens.rs`, `Source/Lexer/Scan.rs`). Sequence: c162 lands the
  `$` lexer/parser splice + the `comptime { … }` execution block FIRST, then c155 Stage 3
  consumes it. Stages 1–2 and the diagnostic/test scaffolding can proceed in parallel with
  c162; only the re-entry weaving blocks on it.
- **Built-in derives stay as-is.** Do not rewrite the Codable/Comparable codegen
  (`Source/Codegen/Items.rs:401-515`) onto the re-entry path; that is compiler-owned, already
  sound under I3, and churn there risks the serde suite. The re-entry loop is new, for user
  derives only.
- **Gated stages (do not implement until balloted):** the reflection API public spelling
  (Stage 1 surface) and the `derive`-authoring spelling + coherence rules (Stage 2). See
  Open Owner-Q. Sema-side resolver and the re-entry plumbing are NOT gated and may start.
- **Dependencies satisfied:** pure comptime sandbox (S26 layer 1, ratified), built-in derive
  infra (S55, shipped), D-METADEPTH1/D-CTCODEGEN1 (ratified 2026-06-25).

## Open Owner-Q

Two user-facing surfaces are unsettled. Both need a ballot card with worked examples before
any code on the public surface. Recommendations included; **owner decides**.

### Q1 — Reflection read-API spelling

How does user/derive code ask a type for its fields, field types, and attrs?

- **A. Comptime free functions** — `fields_of(T)`, `type_name(T)`, `attrs_of(T, field)`
  returning comptime values. Familiar, low syntax cost, composes with existing comptime
  bindings. Risk: a loose bag of builtins.
- **B. A reflected `Type` value with methods** — `T.reflect().fields`, `.fields[i].type`,
  `.fields[i].attrs`. Discoverable (one entry point, LSP-completable — fits the Blueprint
  north-star), self-documenting. Risk: introduces a first-class `Type` comptime value, more
  surface.
- **C. `comptime for field in T { … }`** — iteration construct that binds field shape per
  iteration. Reads naturally for the dominant use (walk fields to build an impl). Risk: a
  second comptime control form; narrower than a general read-API.

**Recommendation: B**, a reflected handle with fields/types/attrs accessors, because the
Blueprint/LSP goal rewards a single discoverable entry point and it keeps reflection from
becoming scattered builtins. Pair with C as sugar over B if field-iteration dominates real
derive bodies.

### Q2 — `derive`-authoring spelling + marker trigger + coherence

How does a library author DECLARE a derive, and how is it TRIGGERED on a type?

Declaration:
- **A. `derive Trait for T { … }`** — mirrors the ratified read example; `T` is the bound
  type handle, body uses reflection (Q1) to build the impl. Reads like an impl block.
- **B. `derive fn Trait(T: Type) { … }`** — derive as an explicit comptime function over a
  reflected type. More obviously "a function the compiler calls"; less impl-like.
- **C. `@derive(Trait) impl … `** — attribute-driven, closest to today's marker world.

Trigger on a user type:
- **A. Bare marker `@MyTrait` / `#[MyTrait]`** — reuses the existing marker router
  (`Source/Parser/Items.rs:1836` already sends unknown names to `derives`). Uniform with
  built-ins; zero new trigger syntax.
- **B. Explicit `derive MyTrait;` line inside the type body** — matches the S55
  `derives: Vec<…>` field already in the AST (`Source/AST.rs:986`); very explicit.

Coherence/orphan: may an author derive a trait they don't own for a type they don't own?
- **A. Local-only** — derive allowed only where the trait OR the type is defined (Rust-like
  orphan rule). Safe, predictable.
- **B. Open** — any derive anywhere. Flexible, risks conflicting impls across packages.

**Recommendation:** declaration **A** (`derive Trait for T`, matches the ratified example and
reads as an impl), trigger **A** (reuse the existing `@Marker` router — no new syntax, uniform
with built-ins), coherence **A** (local-only orphan rule — predictable, no cross-package impl
collisions). Needs a ballot with a worked end-to-end example (author a derive, trigger it,
show the re-entered generated impl + a deliberate-error diagnostic) before Stage 2 code.
