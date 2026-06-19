# Typed error families + error conversion
**Status:** Draft plan — needs owner review (2026-06-19)
**Card:** c22

## Problem & why it matters

Today `?` only crosses two kinds of error boundary:

1. The error types match exactly — `parse(s)?` inside `fn f() -> Int ? ParseError`
   when `parse` also returns `… ? ParseError`.
2. The function returns the universal `Error`, and the propagated error type
   implements the `Fallible` **trait** — `impl MyErr: Fallible { fn to_error(self)
   -> Error { … } }` (verified: `Source/Syntax.rs` `TRAIT_FALLIBLE`/`FN_TO_ERROR`,
   resolved at `Source/Sema/CheckerInfer.rs:565-572`, lowered at
   `Source/Codegen/Expression.rs:332-340` as `.map_err(|e| e.to_error())`). This
   is the D-ERR2 path, ratified 2026-06-16 — it collapses *any* error down to the
   one rich carrier.

Everything else is a hard `E0403` ("the error type must match exactly — there's
no conversion in v1"). That blocks the common, healthy pattern: a module defines
its *own* typed error family and wants callers' lower-level errors folded into
it.

```jet
// config.jet
enum ConfigError { Missing(String); BadInt(ParseError); Io(IoError); }

fn load(path: String) -> Config ? ConfigError {
    val raw = read_file(path)?;     // read_file -> String ? IoError
    val port = parse_int(raw)?;     // parse_int -> Int ? ParseError
    Config { port }
}
```

Both `?` lines fail today with `E0403` ("the error type must match exactly —
there's no conversion in v1", confirmed at `Source/Sema/CheckerInfer.rs:603-608`).
`read_file` yields `IoError`, `parse_int` yields `ParseError`, but the function
returns `ConfigError`. The author's only escapes
are: (a) change the return type to the universal `Error` and lose the typed
family (callers can no longer `when` on `ConfigError.Missing` vs `.Io`), or
(b) hand-write `read_file(path) ?? return err(ConfigError.Io(it))` at every call
site — ceremony that buries the happy path.

The card asks: how does a *typed* error become another *typed* error as it
crosses a `?`, staying safe (no silent lossy coercion) and readable (the
conversion is declared once, not repeated per call site)?

This is squarely priority #2 (beginner experience: a library author building a
typed error family is exactly who hits this) without touching priority #1
(conversion is total and explicit — no runtime failure path is introduced).

## Prior art (terse)

- **Rust — `From`/`Into` + `?` + `thiserror`.** `?` desugars to
  `Err(e) => return Err(From::from(e))`. Any `impl From<ParseError> for
  ConfigError` makes `?` cross automatically. `thiserror` derives those `From`
  impls from `#[from]` on a variant field. Cheap, total, compile-checked — but
  the conversion is *invisible* at the call site, and a stray blanket impl can
  make unrelated errors flow silently.
- **Swift — typed `throws` + manual mapping.** `throws(ConfigError)` is typed,
  but there is no auto-conversion: you write `do { try … } catch { throw
  ConfigError.io($0) }`. Explicit, never surprising, but verbose.
- **Go — `errors.As` / wrapping with `%w`.** No conversion at the boundary;
  errors are values you wrap (`fmt.Errorf("load: %w", err)`) and later unwrap by
  type with `errors.As`. Maximum flexibility, zero compile-time guarantee that a
  given error was handled.
- **Zig — error sets + inferred unions.** `error{A,B} || error{C}` auto-unions;
  `try` widens a narrow set into a wider one structurally, no declaration needed.
  Totally checked and zero-cost, but the set is anonymous/structural — it can't
  carry per-variant payload the way a Jet `enum` variant can.

Synthesis for Jet: we want Rust's *declared-once, `?`-crossing* ergonomics and
Zig's *totality*, but Jet's existing rule is "no silent conversion of unrelated
types" (D-ERR2). So the conversion must be **declared and named**, like a trait
impl, not inferred from a blanket rule.

## Proposed design (worked Jet example)

Generalize the existing `Fallible`-trait conversion from *one fixed target*
(`Error`) to *any error type*, by giving the conversion a typed target. The
*plumbing* already exists in sema and codegen for the `→ Error` case (the
`via_fallible` flag + `map_err`); this widens it. Note the surface differs:
today the `→ Error` conversion is spelled as a **trait impl**
(`impl T: Fallible { fn to_error(self) -> Error }`), so "unify `Fallible` with the
general form" means re-expressing that one prelude conversion in whatever spelling
this card picks — a deliberate redesign of the `Fallible` surface, not a free
rename (tracked as open decision 2).

The author declares, once, how `ParseError` and `IoError` become a
`ConfigError`:

```jet
// config.jet
enum ConfigError { Missing(String); BadInt(ParseError); Io(IoError); }

impl ParseError -> ConfigError { ConfigError.BadInt(self) }
impl IoError    -> ConfigError { ConfigError.Io(self) }

fn load(path: String) -> Config ? ConfigError {
    val raw  = read_file(path)?;   // IoError    -> ConfigError.Io      (declared)
    val port = parse_int(raw)?;    // ParseError -> ConfigError.BadInt  (declared)
    Config { port }
}
```

`impl Source -> Target { … }` reads "this is how a `Source` becomes a `Target`";
inside the body `self` is the source error and the block returns the target.
There is exactly one such conversion per (Source, Target) pair (a second is
`E2405`, "two ways to convert `ParseError` into `ConfigError`"). Both `E2404` and
`E2405` are confirmed free in `docs/spec/diagnostics.md` (E24xx is the reserved
M6 library-authoring block; E2401–E2403 are taken).

What `?` does at `val raw = read_file(path)?;`:

- success → unwrap to `String`, bind to `raw`.
- failure → look for a declared `IoError -> ConfigError` conversion. Found →
  apply it, `return err(...)`. Not found → compile error `E2404` naming both
  types and offering the one-line `impl … -> …` fix.

Crucially this is **not** a blanket rule: `IoError` only flows into `ConfigError`
because the author wrote that line. An undeclared pair is rejected, so an error
never silently changes identity. The universal `Error` stays the special case:
`Fallible` (D-ERR2) is exactly `impl T -> Error`, so the two mechanisms unify
rather than compete — `Fallible`/`to_error` becomes sugar/an alias for the
`-> Error` instance, and prelude types keep their default `-> Error` conversion.

Readability check: the happy path stays clean (`read_file(path)?`), the
conversions live in one declared block near the error enum, and the call site
carries no per-line mapping noise. A reader who wants to know "what happens to an
`IoError` here" reads the `impl IoError -> ConfigError` line, not every `?`.

Cross-module boundary: conversions obey the same orphan rule as trait impls
(S28) — at least one of `Source`/`Target` is defined in this program. So a
downstream crate can declare `impl their.IoError -> my.ConfigError` because
`ConfigError` is local; it cannot declare conversions between two foreign error
types it doesn't own.

## Implementation sketch — pipeline touchpoints

The `Fallible`→`Error` path already threads parser → sema (`via_fallible`) →
codegen (`.map_err(|e| e.to_error())`). This widens that thread to a typed
target.

**Parser** (`Source/Parser/Items.rs`): add the `impl Type -> Type { … }` item
form (or whichever spelling D-ERR-CONV picks). The lexer already has `->`
(`OP_ARROW`); no new sigil if Option A wins. Produce an `ErrorConversion { from:
Type, to: Type, body: Block }` item in `Source/AST.rs`. Keyword/sigil already
registered in `Source/Syntax.rs` (no I7 addition for the `->` spelling; a new
word like `convert` would need a `Syntax.rs` constant + decision ID).

**Sema — conversion resolution** (`Source/Sema/CheckerInfer.rs:545-608`,
`infer_try`): today the `Type::Result { err: ret_err, .. }` arm only succeeds
when `ret_err` is the default `Error` *and* the source implements `Fallible`.
Replace that with a conversion-table lookup:

1. exact match (existing) → ok.
2. else look up `(source_err, ret_err)` in a `conversions: Map<(Type,Type),
   ConvId>` built in a pre-pass over `ErrorConversion` items.
3. found → set the existing `via_fallible` flag (rename to `via_convert:
   Option<ConvId>`) so codegen emits the right map.
4. not found → `E2404` (typed, names both types + the `impl … -> …` fix);
   the old `E0403` "no conversion in v1" wording retires.

Register conversions alongside trait impls in the `m9` table (the same table
`infer_try` already queries via `self.m9.implements_trait(…, TRAIT_FALLIBLE)`);
enforce one-per-pair (`E2405` duplicate) and the orphan rule (reuse S28's check).

**Codegen** (`Source/Codegen/Expression.rs:325-350`, `Expr::Try`): generalize
the `via_fallible` branch. Instead of the fixed `.to_error()`, emit
`.map_err(|e| <converted>)` where `<converted>` is the lowered conversion body
(a plain function call to the generated `fn` for that conversion). Codegen stays
dumb (I3): it just calls the conversion sema already resolved; it never decides
*whether* a conversion exists.

**Diagnostics** (`docs/spec/diagnostics.md` + `tests/ui/`): re-aim `E0403`/
`E2402`, add `E2404` (no declared conversion) and `E2405` (duplicate conversion),
each with what/why/fix and a snapshot (I4).

## Test plan — ui snapshots + example

- `tests/ui/error_convert_missing.jet` → `E2404`: `?` propagates `IoError` into a
  `… ? ConfigError` function with no declared conversion; fix names
  `impl IoError -> ConfigError`.
- `tests/ui/error_convert_duplicate.jet` → `E2405`: two `impl ParseError ->
  ConfigError` blocks; error names both and points at the second.
- `tests/ui/error_convert_orphan.jet` → orphan-rule error: conversion between two
  foreign error types.
- `tests/ui/error_convert_to_error.jet`: confirm the `-> Error` instance still
  works (D-ERR2 unchanged) — guards against regression of the `Fallible` path.
- `examples/features/NN_typed_error_families.jet` + `.out` (I5): the `config.jet`
  worked example above, end to end, printing a converted-and-handled error,
  enforced by golden test.
- `tests/decisions.rs`: ratification row for D-ERR-CONV once owner decides.

## Risks & invariant check

- **I1 (safe by default):** conversion is total — a declared block always returns
  a target value; no new runtime failure path, no `unsafe`. Pass.
- **I2 (rustc never speaks):** the conversion body is a normal expression — sema
  must type-check it returns the declared `Target` *before* codegen lowers it,
  exactly as it already checks `to_error`'s body returns `Error`. If sema let a
  body whose type ≠ `Target` through, the emitted `map_err` closure would fail to
  compile and rustc would surface the error to the user. So the conversion body's
  return type is checked in the same pass that registers the conversion. Pass once
  that check is in place.
- **I3 (dumb codegen):** all resolution stays in sema; codegen emits a `map_err`
  calling a sema-resolved conversion. Pass.
- **I4 / I5:** every new diagnostic gets a snapshot; the feature ships an example.
  Pass once written.
- **I7:** `->` is already registered. A word-based spelling (`convert`) would need
  a `Syntax.rs` constant + ID — flagged in the ballot.
- **I8 (simplicity ratchet):** this *generalizes* an existing mechanism rather
  than adding a parallel one — `Fallible` (`-> Error`) becomes the prelude
  instance of the general `-> Target` form. Net concept count does not grow; it
  arguably shrinks (one rule instead of "exact match OR Fallible-to-Error").
- **Risk — silent flow.** Mitigated by requiring a declared conversion per pair
  + the orphan rule; no blanket `From`-style impls. An error can never change
  identity without a line the author wrote.
- **Risk — chaining.** Should `A -> B` then `B -> C` auto-compose so `A` flows
  into a `… ? C` function? Rust does *not* transitively chain `From`. Recommend
  v1 = **no transitive chaining** (one declared hop only; deeper nesting is an
  `E2404` that names the missing direct conversion). Listed as an open decision.
- **Risk — interaction with `?? return err(...)`.** The fallback path (S35/S71)
  is unaffected; it never relied on conversion. Confirmed no overlap.

## Open decisions

1. **Spelling of the conversion declaration** — `impl A -> B { … }` vs. a
   keyword form vs. a method on the target. → D-ERR-CONV below.
2. **Does `Fallible`/`to_error` survive as named sugar for `-> Error`, or is it
   subsumed and retired to a teaching alias?** Recommend: keep `Fallible` as the
   prelude's `-> Error` instance (beginners never see the general form), document
   it as the special case.
3. **Transitive chaining** (`A->B`, `B->C` ⇒ `A` into `?C`)? Recommend **no** in
   v1 (match Rust; avoid surprising multi-hop coercions). Owner call.
4. **Per-variant `#[from]`-style derive** (auto-generate `impl ParseError ->
   ConfigError` from a `ConfigError.BadInt(ParseError)` variant)? Powerful but
   adds magic. Recommend defer to a follow-up card once the explicit form lands.

## Proposed decision card(s)

### D-ERR-CONV — Typed error→error conversion across `?` (rec A)

Today `?` crosses a typed-error boundary only when the target is the universal
`Error` (via `Fallible`, D-ERR2). A library with its own typed error family
(`enum ConfigError { … }`) can't fold a lower-level `IoError`/`ParseError` into
it without per-call-site ceremony. This card picks the mechanism *and its
spelling* for declaring "a `Source` error becomes a `Target` error", which `?`
then applies automatically. In every option, the conversion is declared once,
total, and rejected unless declared (no silent/blanket coercion); the orphan rule
(S28) applies.

- **Option A — `impl Source -> Target { … }` (recommended).** Reuses the
  existing `->` token and `impl` keyword; reads "Source becomes Target". `self`
  is the source error; the block returns the target. `Fallible` becomes the
  prelude's `impl T -> Error` instance, unifying the two mechanisms.

    ```jet
    enum ConfigError { Missing(String); BadInt(ParseError); Io(IoError); }

    impl IoError    -> ConfigError { ConfigError.Io(self) }
    impl ParseError -> ConfigError { ConfigError.BadInt(self) }

    fn load(p: String) -> Config ? ConfigError {
        val raw  = read_file(p)?;    // IoError    -> ConfigError.Io
        val port = parse_int(raw)?;  // ParseError -> ConfigError.BadInt
        Config { port }
    }
    ```

    Seen on a missing conversion:

    ```
    Error [E2404]: `?` can't turn an `IoError` into a `ConfigError` here
      --> config.jet:7:25
        |
      7 |     val raw = read_file(p)?;
        |                         ^
     Why: `?` only changes an error's type when you've declared how; there's no
          declared way to turn `IoError` into `ConfigError`
     Fix: add `impl IoError -> ConfigError { ConfigError.Io(self) }`
    ```

- **Option B — `convert Source -> Target { … }` keyword.** A dedicated word
  instead of `impl`, so error conversions read distinctly from trait impls.
  Costs a new keyword (`Syntax.rs` + I7 ID); `Fallible` stays separate rather
  than unifying.

    ```jet
    convert IoError    -> ConfigError { ConfigError.Io(self) }
    convert ParseError -> ConfigError { ConfigError.BadInt(self) }

    fn load(p: String) -> Config ? ConfigError {
        val raw = read_file(p)?;
        Config { port: parse_int(raw)? }
    }
    ```

- **Option C — method on the target via a trait (`FromError`).** Mirror Rust's
  `From`: a trait whose method builds the target from the source. Familiar to
  Rustaceans, but `from`/`Into` directionality is a known beginner stumbling
  block, and it needs a generic trait (`trait FromError<E>`), heavier than v1's
  signature-only traits (S28).

    ```jet
    impl ConfigError: FromError<IoError> {
        fn from_error(e: IoError) -> ConfigError { ConfigError.Io(e) }
    }

    fn load(p: String) -> Config ? ConfigError {
        val raw = read_file(p)?;   // ? calls ConfigError.from_error(e)
        Config { port: parse_int(raw)? }
    }
    ```

- **Option D — call-site explicit map, no auto-conversion.** Reject silent
  crossing entirely; the author maps at each `?` with a fallback. Most explicit,
  zero new declaration form — but repeats the mapping per call site and clutters
  the happy path (the very thing the card exists to remove).

    ```jet
    fn load(p: String) -> Config ? ConfigError {
        val raw  = read_file(p) ?? return err(ConfigError.Io(it));
        val port = parse_int(raw) ?? return err(ConfigError.BadInt(it));
        Config { port }
    }
    ```

**Recommendation:** **A (`impl Source -> Target { … }`)** — no new sigil or
keyword (I7-clean), reuses `impl`'s mental model and orphan rule, and *unifies*
with the existing `Fallible`/`to_error` path (which becomes the prelude's
`impl T -> Error`), so the language gains zero net concepts (I8). It keeps the
happy path clean (Option D's flaw), avoids Rust's `from`/`Into` directionality
confusion (Option C), and avoids spending a keyword (Option B). Pairs with the
open decisions on retaining `Fallible` as named sugar and on (not) chaining
conversions transitively.
