# Jai-adoption ballots — implementation plan (ratified 2026-07-01)

Eight ballots, eight cards, one sitting: c7methodmacro, c7dynarray, c7cliflag,
c7shift, c7pointerchain, c7jaiany, c7uninitsentinel, c7refshorthand. All
ratified 2026-07-01; full ballot text lives in `tools/Tower/tower.json`
`decisions[]` under the matching `D-*` id — this doc extracts the executable
parts, it does not restate the story/comparisons.

**Blocking prerequisite — read first.** D-MARKER-FAMILY1(=B)/D-MARKERMOVE1(=B)/
D-CONTRACTCASE1(=A) ratified the `@` contract plane (PascalCase), but it is
**not implemented**. Today `@` at item position is a hard parse error:

```
crates/jet-parser/src/Parser/Items.rs:767-778
TokKind::At => {
    // E0990: "attributes use `#`, not `@` ... in Jet, `@` is for loop
    // labels; attributes and markers use `#` (D-ATTR1)"
```

`CONTRACT_PREFIX = "@"` is registered (`crates/jet-foundation/src/Syntax.rs:1847`)
but unconsumed. The plane build-out plan now exists —
`tools/Tower/docs/plans/epoch-3/marker-family.md` (gates resolved 2026-07-02:
D-MARKERMOVE2=B, D-MARKERMOVE3=B) — and must execute before §1 (`@Inline`),
§3 (`@[Cli]`/`@Doc`), and §8 (`@Ref`) can land their surface syntax. Sema/codegen
groundwork for those three cards can proceed against the CURRENT `#`-spelled
grammar and be re-pointed at `@` in one mechanical pass once marker-family
lands — do not block the semantic work on it, only the final syntax swap.

Formatter/fmt-stability: every card below that touches parseable syntax needs
formatter emission + a round-trip fmt STABILITY test (own-CLAUDE-memory rule:
new syntax without it silently corrupts on `jet fmt`). Noted per-section.

---

## 1. c7methodmacro — checked `@Inline` / `@InlineAlways` contracts

**D-METHODMACRO1 = A.** Methods stay ordinary functions — no macro-rewrite
hooks. Deliverable: `@Inline` / `@InlineAlways` are checked contracts on a
function/method declaration. `@InlineAlways` that the compiler cannot
actually inline is a **compile error naming why** (recursion, address-taken,
too large post-monomorphization, etc.), not a silent miss. `@Inline` is
soft (a hint; never errors). PascalCase per D-CONTRACTCASE1.

Sequencing: implement against `#Inline`/`#InlineAlways` now (matches every
other still-`#`-spelled contract, e.g. `#Pure`/`#MustUse`); swap the sigil
to `@` in the marker-family migration pass, one parser-table edit.

**Files:**
- `crates/jet-foundation/src/AST.rs:1191+` — add `is_inline: bool`,
  `is_inline_always: bool`, `inline_span: Option<Span>` to `Func` (sits next
  to existing `is_pure`/`is_must_use`).
- `crates/jet-parser/src/Parser/Items.rs` — extend the existing single-marker
  function path (`at_unsafe_fn`/`at_reactive_fn` sibling, ~line 720) with an
  `at_inline_fn` check; reject `@Inline` + `@InlineAlways` together (pick one).
- `crates/jet-sema/src/Sema/CheckerItems.rs` (or a new `CheckerInline.rs`
  seam) — the "can this actually inline" check: reject `@InlineAlways` on
  self-recursive functions, functions whose address is taken (`fn` value /
  passed as a callback), and functions above a size/complexity ceiling
  (define one constant, document it in the diagnostic fix text).
- `crates/jet-codegen/src/Codegen/Items.rs:1138-1142` — already emits
  `#[inline]` for `ConstAttr::ForceInline`; add the analogous path for
  `is_inline` → `#[inline]`, `is_inline_always` → `#[inline(always)]`, gated
  on the sema check passing (I3: sema decides, codegen just emits).
- `jet expand --facts inline` — **does not exist** (`grep -r "expand.*facts"
  crates/jet-driver Source/` is empty). Out of scope for this card unless the
  owner wants it now; if deferred, say so explicitly in the PR/card note —
  do not silently drop it, it's a named ballot deliverable.

**New diagnostics** (E09xx contract-check family, next free after E0916):
- `E0917` — `@InlineAlways fn {name}` cannot be inlined: self-recursive.
  What: "`{name}` calls itself, so `@InlineAlways` cannot expand it."
  Why: "inlining a recursive call would either loop forever at compile time
  or require an artificial depth cutoff — neither is a real inline."
  Fix: "drop `@InlineAlways` (use `@Inline` as a hint), or restructure the
  function to be non-recursive."
- `E0918` — `@InlineAlways fn {name}` cannot be inlined: address taken /
  passed as a value. What/why/fix analogous — fix says "drop
  `@InlineAlways`, or call `{name}` directly instead of through a value."
- Fixtures: `tests/ui/inline_always_recursive.jet` / `.stderr`,
  `tests/ui/inline_always_address_taken.jet` / `.stderr`.

**Example:** `examples/metaprogramming/inline-contracts.jet` (topic dir name
only — do not add a numeric prefix, the examples tree reorg is in flight).

**Tests:** `cargo test --test ui -- inline` (targeted), full `cargo test`
once at the end.

**Exit criteria:**
- [ ] `@Inline`/`@InlineAlways` (or `#`-spelled interim) parse on `fn` and
      `fn Type.method`.
- [ ] `@InlineAlways` failure is a compile error, not a warning or silent no-op.
- [ ] Rust codegen emits `#[inline]`/`#[inline(always)]` only when sema passed.
- [ ] E0917/E0918 registered in `docs/spec/diagnostics.md` with snapshot fixtures.
- [ ] Formatter round-trips the marker; fmt STABILITY test added.
- [ ] `nix develop -c cargo test` green.

---

## 2. c7dynarray — `View<T>` zero-copy library type

**D-DYNARRAY1 = A.** No new bracket syntax — `[T]`/`[K,V]`/`[T#N]` stay the
only list-family spellings. New: a `View<T>` library type riding the
**existing** stored-ref ownership machinery, so `incidents.view(0..9)` is a
zero-copy window whose owner (`incidents`) the compiler tracks and cannot
outlive. `incidents[0..9]` (range-index) stays the safe, default, **copying**
slice — unchanged.

**Existing machinery to ride (already implemented, do not rebuild):**
- `crates/jet-foundation/src/AST.rs:1511-1512` — `Field.is_stored_ref`,
  `Field.stored_ref_label`.
- `crates/jet-parser/src/Parser/Items.rs:3548-3630` — `#Ref(owner) name: T`
  field parser.
- `crates/jet-sema/src/Sema/CheckerOwnership.rs:83` —
  `check_stored_ref_fields()`, the owner-outlives check.
- `crates/jet-sema/src/Sema/Registration.rs:1406` — `E0207` ambiguous-owner
  check (see §8 — this is the exact code the ref-shorthand card reworks).
- Working examples today: `examples/features/09_ref_field.jet`,
  `examples/features/183_ref_owner.jet`.

**New work — `View<T>` is a stdlib type, not a language feature:**
- `crates/jet-codegen/src/Prelude/CoreLib.rs` — add `View<T>` alongside the
  other `core.*` bridge types (same file that defines `JetFileReader` etc.).
  Internally a `(ptr, len)` pair over the owning list's backing storage, plus
  a compiler-tracked stored-ref back to the owner (reuse `is_stored_ref`
  machinery — the view's "owner field" IS the list it was cut from).
  Must satisfy `docs/spec/stdlib-api-laws.md`'s 8 laws (naming, fallibility,
  ownership/views section is literally about this).
- `crates/jet-sema/src/Sema/CheckerOwnership.rs` — extend
  `check_stored_ref_fields`-style analysis to `.view(range)` call sites:
  the view value carries the same "cannot outlive owner" proof as a `#Ref`
  field, checked wherever the view is stored, returned, or crosses a task
  boundary (reuse E2301-E2304's reasoning, do not invent a parallel model).
- Read-only surface first (`.fold`, `.map`-to-owned, indexing, iteration);
  mutation through a view is out of scope unless the ballot's `Compiler-
  tracked owner` language is read to include `~View<T>` — it isn't in the
  ratified `inWild` example, so ship immutable views only, note the write
  question as a follow-up if it comes up in review.

**New diagnostics** (extend Tier-2 reference family E23xx, next free after
E2304/L2301):
- `E2305` — a `View<T>` escapes its owner's scope (returned from a function
  that owns the source list, stored in a struct field without `@Ref`, or
  crosses a `tasks.spawn`/channel boundary). What/why/fix mirror E2301/E2302's
  wording but name `.view(...)` instead of `-> view`/`ref` field.
- Fixture: `tests/ui/view_outlives_owner.jet` / `.stderr` (mirror
  `ref_field_dangles_local.jet` naming pattern already in `tests/ui/`).

**Example:** `examples/collections/dynamic-array-view.jet` — copy-slice vs
`.view()` side by side (the ballot's own `inWild` is the starting point).

**Formatter:** `.view(...)` is an ordinary method call, no new grammar — no
fmt-stability test needed unless `View<T>` gets a literal/sigil spelling
later.

**Tests:** `cargo test --test ui -- view` (targeted), `cargo test -p
jet-codegen` for the CoreLib addition, full suite once at the end.

**Exit criteria:**
- [ ] `View<T>` defined in `CoreLib.rs`, satisfies stdlib-api-laws.md.
- [ ] `.view(a..b)` is zero-copy (verify: no allocation in generated Rust for
      the view construction itself).
- [ ] Owner-outlives check fires (E2305) with a working `.fixed.jet` repair
      fixture, same convention as `ref_field_dangles_local.fixed.jet`.
- [ ] `incidents[0..9]` behavior is unchanged (regression-tested).
- [ ] `docs/spec/spec.md` stored-ref section gets a `View<T>` cross-reference.
- [ ] `nix develop -c cargo test` green.

---

## 3. c7cliflag — typed entry-signature CLI parsing

**D-CLIFLAG1 = A.** `fn run(args: ServeArgs)` — the entry function's typed
parameter IS the CLI spec. Flags derive from struct fields, defaults from
field defaults, help text from `@Doc("...")` field attributes, subcommands
from an enum parameter (`enum Cmd { Serve(ServeArgs) Import(ImportArgs) }`,
`fn run(cmd: Cmd)`). `fn run()` (zero-arg) is unaffected — adding a typed
param is what opts in. Reuses the `#[Codable]`/derive+reflection machinery,
no new derive engine. Bad-flag errors are ordinary Jet diagnostics.

**Card is `blockedBy: c7markerfamily`** (recorded in tower.json) because the
ratified option's own code sample spells the struct marker `@[Cli]` and the
field marker `@Doc(...)` — both `@`-plane, neither exists yet (see header).
D-BUILDENTRY1 (separately ratified 2026-07-01, option B) is background only
— `fn build(ctx: BuildContext)` via the lifecycle-verb map is the analogous
build-time case, not a dependency of this card.

**What already exists — extend, don't duplicate (I8):**
- `core.args.spec()` builder (`ArgsSpec`/`ParsedArgs`, D-ARGS1, ratified
  2026-06-22): `.flag(name, help)`, `.option(name, help, meta)`,
  `.positional(name, help)`, `.parse(argv)`. Implementation:
  `crates/jet-codegen/src/Codegen/TIR/emit.rs:2602`,
  `crates/jet-sema/src/Sema/CheckerCoreLib.rs:3607`. This card's typed
  surface **generates down onto this builder** — do not write a second
  parser. Existing examples: `examples/features/88_args_spec.jet` (builder),
  `examples/features/64_cli_args.jet` (raw `io.args()`).
- `#[Codable]` derive (parser attrs `crates/jet-foundation/src/Syntax.rs:
  1627-1629`; sema validation `crates/jet-sema/src/Sema/CheckerCoreLib.rs:
  4280` `validate_serde_items()`; codegen `crates/jet-codegen/src/Codegen/
  Items.rs:407-644`). `@[Cli]` is a sibling derive on the same infrastructure
  — a struct gets `Cli` instead of (or alongside) `Codable`, each field
  becomes one `ArgsSpec` registration instead of one wire key.

**Build order:**
1. Land `@[Cli]` / `@Doc(...)` parsing once marker-family ships (or land
   provisionally as `#[Cli]` / `#Doc(...)` now, same swap-later plan as §1).
2. Sema: struct marked `Cli` → walk fields → emit one `.flag`/`.option`/
   `.positional` registration per field (bool fields → flag, `T?` fields →
   optional option, everything else → required option/positional per a
   simple, documented field-order rule — pin the exact rule in
   `docs/spec/spec.md` before coding, it's a real design decision the ballot
   didn't spell out numerically).
3. Sema: `enum Cmd { Variant(ArgsStruct) ... }` parameter on `fn run` →
   subcommand dispatch; each variant's payload struct must itself be `Cli`.
4. Codegen: `fn run(args: T)` where `T: Cli` → prelude wrapper that builds
   the `ArgsSpec`, parses `io.args()`, decodes into `T`, calls the user's
   `run` with it; parse failure prints the generated `--help`/error and
   exits nonzero (reuse `core.args` exit/help behavior, do not reinvent).

**New diagnostics** (extend "CLI arg-parsing diagnostics (D-ARGS1)" section,
next free slot after E1304, **before** E1310-1312 which D-VARIADIC1 already
owns — free range is E1305-E1309):
- `E1305` — a `Cli`-derived struct field has a type with no flag mapping
  (e.g. a nested non-`Cli`, non-primitive struct, a `Map`, a closure). Fix:
  name a supported field type (primitives, `Path`, `String`, `Bool`, `T?`,
  or a nested `Cli` struct for grouped flags if that's in scope — decide and
  document).
- `E1306` — two fields would derive the same flag name (e.g. case-only
  clash, or an explicit rename collision once/if `@Doc`/rename options ship).
- `E1307` — `enum Cmd` used as an entry parameter has a variant whose
  payload type is not `Cli`-derived.
- Fixtures: `tests/ui/cli_field_unsupported_type.jet` / `.stderr`,
  `tests/ui/cli_flag_name_collision.jet` / `.stderr`,
  `tests/ui/cli_subcommand_payload_not_cli.jet` / `.stderr`.
- Runtime bad-flag/missing-flag/parse-failure errors: reuse the existing
  `core.args` runtime error surface (same diagnostics as `ArgsSpec.parse`
  already produces) — do not mint new codes for those, that would violate I8.

**Example:** `examples/cli/typed-entry-args.jet` (flags from fields) and
`examples/cli/subcommands.jet` (enum-driven subcommands).

**Formatter:** `@[Cli]`/`@Doc(...)` are ordinary marker syntax once
marker-family lands — covered by that migration's fmt work, not new here.

**Tests:** `cargo test --test ui -- cli_` (targeted), run the two new
examples through the golden-example harness, full suite at the end.

**Exit criteria:**
- [ ] `fn run()` (zero-arg) behavior is completely unchanged.
- [ ] `fn run(args: ServeArgs)` where `ServeArgs: Cli` parses `io.args()`,
      applies defaults, and calls `run` with a decoded struct.
- [ ] `--help` is generated from field names/types/`@Doc` text, not hand-written.
- [ ] `enum Cmd` parameter dispatches subcommands correctly.
- [ ] E1305-E1307 registered with fixtures; existing `core.args` runtime
      errors reused for bad-flag-at-runtime (no duplicate codes).
- [ ] `88_args_spec.jet`/`64_cli_args.jet` still pass unmodified (builder
      layer is untouched, this card only adds the typed layer on top).
- [ ] `nix develop -c cargo test` green.

---

## 4. c7shift — `core.binary.Reader` + `core.text.Cursor`

**D-SHIFT1 = A.** No shift operator. New stdlib surface: `core.binary.Reader`
(`read_u8/u16/u32/u64` in `_le`/`_be` variants, `take(n)`, `remaining`,
`at_end`, all fallible/`?`-composed) and `core.text.Cursor` (`take_pattern`,
`take_until`, `skip_ws`). `take_pattern` reuses the D-PARSESTR1 pattern
syntax (**ratified**, not open — confirm in tower.json: `{name}` untyped
hole, `{name:Type}` typed hole, non-greedy, anchored by literal text) so one
pattern spelling both matches and consumes.

**What exists today:** nothing. `JetFileReader`/`JetStdinReader`
(`crates/jet-codegen/src/Prelude/CoreLib.rs:2886`, `:3137`) are file/stdin
I/O, not byte-buffer cursors — no overlap to reuse beyond the general
"fallible reader" shape. Postfix `p.*`/raw pointer ops exist (§5) but are
unrelated (this card is safe-tier, no `#Unsafe`).

**Build order:**
1. `core.binary.Reader` in `CoreLib.rs`: wraps `[U8]`/`&[U8]` (use the
   §2 `View<T>`/slice-of-U8 as the backing storage once that lands, or plain
   owned `[U8]` if sequenced first — note the dependency either way), tracks
   a cursor position, every read is `Fallible` (bounds check → error, not
   panic or truncation).
2. `core.text.Cursor` in `CoreLib.rs`/a Jet-side wrapper: same shape over
   `String`, `take_pattern` implemented on top of the D-PARSESTR1 matcher
   (find that matcher's implementation before writing a second one — grep
   `crates/jet-sema` and `crates/jet-codegen` for the pattern-match codegen
   D-PARSESTR1 already shipped, since it's ratified and may already have a
   matching engine to extend into "consume" mode).
3. Sema: bounds-check diagnostics on `.take(n)` where `n` is a compile-time
   constant provably exceeding a compile-time-known buffer size (best-effort
   only — general case is a runtime `Fallible` return, not a compile error).

**New diagnostics:** primarily **runtime** `Fallible` errors (bounds
exceeded → ordinary error value, not a new E-code — I8: reuse the existing
error-return/`?` diagnostic surface, not a bespoke one). One compile-time
code if step 3's constant-bounds check ships:
- `E1315` (next free after D-VARIADIC1's E1310-1312 and this card's own
  slot — recheck the registry immediately before landing, ranges shift as
  other cards claim E1305-1309/E1313) — `.take(n)` with a literal `n`
  provably exceeding a compile-time-sized buffer. What/why/fix: name the
  buffer size, suggest a runtime-checked `.take(n)?` pattern instead if `n`
  isn't actually constant.
- Fixture: `tests/ui/binary_take_const_oob.jet` / `.stderr`. Optional — cut
  if step 3 is deferred; note the cut in the PR, don't ship half a diagnostic.

**Example:** `examples/parsing/binary-reader.jet` (the ballot's own
`inWild` header/count/payload sample) and `examples/parsing/text-cursor.jet`
(`take_pattern` incident-line sample).

**Formatter:** method-call syntax only, no new grammar; `take_pattern("...")`
pattern-string contents are D-PARSESTR1's concern (already shipped) — no new
fmt-stability test needed here unless D-PARSESTR1 lacks one, which is out of
this card's scope to check.

**Tests:** `cargo test --test ui -- binary` / `-- cursor` (targeted), full
suite at the end.

**Exit criteria:**
- [ ] `binary.Reader.over(bytes)` + all listed methods implemented, every
      read fallible and `?`-composable.
- [ ] `text.Cursor.over(s)` + `take_pattern`/`take_until`/`skip_ws` implemented,
      `take_pattern` reuses the D-PARSESTR1 matcher (no second pattern engine).
- [ ] `docs/reference/core-library.md` gets `Reader`/`Cursor` entries.
- [ ] Two new examples pass golden tests.
- [ ] `nix develop -c cargo test` green.

---

## 5. c7pointerchain — docs only, no compiler change

**D-POINTERCHAIN1 = A.** Reject the Jai compact cast/deref chain
(`slot.value_pointer.(*Bool).* = true`) outright. Current Jet answer stands:
explicit cast + postfix deref, both inside `#Unsafe(...)`. This card ships
**documentation and an example**, not new syntax or sema.

**Current state (verify before writing docs, don't assume the ballot's
example already works):**
- Postfix deref `p.*` — **implemented**, `crates/jet-parser/src/Parser/
  Expressions.rs:646` (D-CAP9), gated to `#Unsafe` in sema (E0208, "raw
  pointer op outside `#Unsafe`").
- `mem.cast_ptr<T>(ptr)` — **does not exist**. The ballot's own worked
  example (`ptr: *Bool #= mem.cast_ptr<Bool>(slot.value_pointer)`) uses an
  API that isn't shipped. Two choices: (a) find/name whatever raw-pointer
  cast primitive *does* exist in `core.mem` today and rewrite the doc
  example around it, or (b) this card quietly also needs `mem.cast_ptr<T>`
  as a small addition. **Do not silently invent (b)** — check `core.mem`'s
  current surface first; if no cast primitive exists at all, that's a real
  gap to flag back to the ballot owner before writing docs that reference a
  function that doesn't exist.
- `slot.value_pointer` (reflection typed-pointer field) — **does not
  exist**. `crates/jet-comptime/src/Comptime/Reflect.rs` is comptime/build-
  time reflection (`FieldInfo`/`MethodInfo`/`TypeInfo`), not a runtime
  pointer-yielding API. The ballot's story (a debugger reading a reflected
  slot's raw pointer) has no runtime reflection surface to hang off yet —
  this overlaps §6's "reflect.Value/Data tooling floor," which is *also*
  unshipped. Recommend: write the doc/example against whatever raw-pointer
  operations exist today (plain `*T`, cast, `.* `) without inventing a
  `slot.value_pointer` API; cross-link to §6 as the future integration point.

**Files:** `docs/spec/spec.md` (or a low-level-tier reference page) gets a
short "Jai transliteration" note: chain expression → two Jet lines inside
`#Unsafe`. No parser/sema/codegen changes.

**New diagnostics:** none.

**Example:** `examples/lowlevel/pointer-cast-deref.jet` — mirrors the
existing `#Unsafe("...") { p: *Bool #= ...; p.* = true }` shape already
legal today; if no `cast_ptr` exists, use whatever the current raw-cast
spelling actually is (check `core.mem` or the raw-pointer type's own cast
method before writing).

**Formatter:** no new syntax — nothing to add.

**Tests:** the new example just needs to compile/run under golden tests;
no new `tests/ui` fixtures (no new errors).

**Exit criteria:**
- [ ] Doc section written, cites the *actually-existing* cast API (name it
      precisely after checking `core.mem`).
- [ ] Example compiles and runs under the golden-example harness.
- [ ] No compiler changes shipped under this card's name — if `mem.cast_ptr`
      turns out missing and blocking, that gap gets its own tiny card/ballot
      note, not a silent addition here.

---

## 6. c7jaiany — trait-bounded heterogeneous varargs + reflect floor

**D-ANY-JAI1 = A.** No top `Any` type (D-DYNAMIC-TYPE1 stands). New:
trait-bounded variadic parameters — `fn log_all(parts: ...Renderable)` — so
each call-site argument is checked against the trait and monomorphized per
call, zero boxing. `reflect.Value`/`Data` remain the tooling/expert floor
(folds in what would've been the "handle" option). Owner's follow-up
questions (recorded in the ballot's `comment` field) — **"can we do A & B?"**
and **"can @-varargs bind a set of traits, not just one?"** — read as: yes to
both, and multi-trait bounds should reuse **S45's already-ratified**
`<T: A + B>` bound syntax rather than inventing new variadic-bound grammar
(`parts: ...(Renderable + Debug)` or similar — confirm the exact spelling
against S45's existing multi-bound syntax, don't invent a second one, I8).

**What exists today:**
- Variadic parameters — **already implemented**. `name: ...T` parses and
  validates (`crates/jet-parser/src/Parser/Items.rs:2622`, D-VARIADIC1;
  `crates/jet-foundation/src/AST.rs:1270` `Param.variadic`). **Missing:**
  trait bounds on the element type — today `T` in `...T` is any concrete
  type, no `...Trait` form checked against implementors.
- Display/interpolation trait — `JetDisplay` (`crates/jet-codegen/src/
  Prelude/Core.rs:3`, `jet_display() -> String`) is the trait string
  interpolation already uses. **Reuse this as `Renderable`** (or rename in
  place / alias) rather than defining a parallel trait — the ballot's own
  `Renderable` name is illustrative, not a hard requirement; confirm the
  final name doesn't collide with anything before locking it in a snapshot.
- `reflect.Value`/`Data` — **do not exist**. Comptime-only reflection lives
  in `crates/jet-comptime/src/Comptime/Reflect.rs` (`FieldInfo`/
  `MethodInfo`/`TypeInfo`, build-time). A runtime `reflect.Value` handle is
  net-new stdlib work, not a rename of the comptime types.

**Build order:**
1. Parser: `name: ...Trait` / `name: ...(TraitA + TraitB)` — extend the
   existing variadic-param grammar (`Items.rs:2622`) to accept a trait-bound
   position instead of only a concrete type, reusing S45's bound-list parse
   function if one already exists as a callable unit (don't copy it).
2. Sema: at each call site, check every argument against the trait bound(s)
   (same mechanism as ordinary generic-bound checking, `E0905` "type doesn't
   implement required trait" family — reuse, don't fork). Monomorphize the
   variadic body per call-site argument list (same machinery as existing
   generic monomorphization).
3. Ship `Renderable` = existing `JetDisplay` (alias or rename — pick one,
   document the choice, this is a one-line decision not a ballot).
4. `reflect.Value`/`Data`: new stdlib surface in `CoreLib.rs`/`Reflect.rs`
   analog. Scope this conservatively for v1 — `type_name()`, `display()`,
   maybe field enumeration; this is real, separately-testable stdlib work,
   don't let it balloon inside the same PR as the variadic-bound feature.

**New diagnostics:**
- `E1313` (extends "Variadic and spread diagnostics (D-VARIADIC1)," next
  free after E1310-1312) — a variadic call-site argument doesn't implement
  the bound trait(s). What: "`{arg}` doesn't implement `{Trait}`." Why:
  "`{param}: ...{Trait}` checks every argument against `{Trait}` — that's
  how `{fn}` accepts a mix of types safely." Fix: "implement `{Trait}` for
  `{arg}`'s type, or drop the value from this call."
- Fixture: `tests/ui/variadic_trait_bound_unmet.jet` / `.stderr`.

**Example:** `examples/traits/renderable-varargs.jet` (the ballot's
`log_all` sample) and `examples/reflection/reflect-value.jet` if `reflect.
Value` ships in the same pass; split into a follow-up example otherwise.

**Formatter:** `...Trait` inside a param list is new grammar — needs
formatter emission + fmt STABILITY test (own-memory rule: don't skip this).

**Tests:** `cargo test --test ui -- variadic_trait` (targeted), full suite
at the end.

**Exit criteria:**
- [ ] `parts: ...Renderable` (or chosen trait name) parses, checks each call
      arg, monomorphizes per call site, zero boxing in generated Rust.
- [ ] Multi-trait bound spelling confirmed to match S45's existing syntax
      (no second bound-list grammar).
- [ ] E1313 registered with fixture.
- [ ] `reflect.Value`/`Data` floor shipped (or explicitly deferred with a
      named follow-up card — do not leave it silently undone while marking
      this card done).
- [ ] `nix develop -c cargo test` green.

---

## 7. c7uninitsentinel — `uninit` contextual keyword (supersedes `#Uninit` marker)

**D-UNINIT-SENTINEL1 = D.** New contextual keyword `uninit`, legal only in
initializer position: `bytes: [U8#1024] := uninit`. Sema proves
write-before-read on every control-flow path before any read is allowed;
reading a possibly-uninitialized slot is a compile error. This **replaces
the spelling** of the existing, already-shipped `#Uninit name: Type` marker
(D-UNINIT1, ratified 2026-06-21) — the flow-analysis engine is reused
unchanged, only the surface syntax and its diagnostics' wording move.

**What exists today (D-UNINIT1, fully implemented — reuse the engine):**
- `ATTR_UNINIT` marker (`crates/jet-foundation/src/Syntax.rs:264`).
- Flow analysis: `crates/jet-sema/src/Sema/CheckerCore.rs:584-690` (and the
  loop/if-merge logic around lines 1346-1615) tracks `self.uninit: Map<Name,
  ...>`, removes an entry on write, flags reads of still-uninit names.
- Diagnostics `E0420-E0424` (`docs/spec/diagnostics.md:762`, section
  "Uninitialized binding diagnostics (D-UNINIT1)").
- Test fixture: `tests/ui/uninit_bad_type.jet`/`.stderr` (naming precedent
  to follow for new fixtures).

**Build order:**
1. Lexer: register `uninit` as a **contextual keyword** (precedent already
   exists for position-constrained keywords — `region`, `live`, `step`,
   `state`, `protocol`, `migration`, `rename` are all contextual; follow
   that same pattern, don't add a global reserved word).
2. Parser: `uninit` legal only as the RHS of `:=` where a type annotation is
   present on the LHS (`name: Type := uninit`) — reuse the same
   "plain-data type only" constraint as today's `#Uninit` (E0423) and the
   same `use core.mem` gate (E0424).
3. Sema: point the *existing* `CheckerCore.rs` uninit-tracking map at
   bindings initialized with `uninit` instead of bindings marked `#Uninit`
   — this should be close to a one-line trigger-condition change, the
   tracking/merge logic itself doesn't change.
4. Retire `#Uninit name: Type` as a hard parse-time teaching error pointing
   at the new spelling (I8: one way to mean it — same treatment as the
   retired `ref[label]` bracket form, see `tests/ui/ref_retired_bracket.jet`
   for the exact pattern to mirror).

**Diagnostics — reword existing codes, retire one, add one:**
- `E0420` (read-before-write) — **keep**, reword message from "declared
  `#Uninit`" to "declared with `:= uninit`"; re-bless snapshot.
- `E0421` (needs a type annotation) — **keep**, same semantics: `name :=
  uninit` with no `: Type` is still illegal (the type can't be inferred
  from `uninit`).
- `E0422` (cannot have an initializer) — **retire**, mark "retired by
  D-UNINIT-SENTINEL1" in the registry table (same convention as the
  existing `E0210`/`E3101` retired-entry rows) — the new form's whole point
  is that `uninit` *is* the initializer, so the old rule is structurally
  inapplicable, not just unreachable.
- `E0423` (needs a plain-data type) — **keep**, reword.
- `E0424` (needs `use core.mem`) — **keep**, reword.
- `E0425` (new) — old `#Uninit name: Type` spelling used. What: "`#Uninit`
  is retired." Why: "uninitialized storage is a fact about the initializer,
  not the declaration — it now reads `name: Type := uninit`." Fix: "write
  `{name}: {Type} := uninit`."
- Fixtures: reuse/rename existing `uninit_*` fixtures to the new syntax
  where their semantics carry over; add `tests/ui/uninit_marker_retired.jet`
  / `.stderr` for E0425 (mirror `ref_retired_bracket.jet`'s structure).

**Example:** update whatever example currently demonstrates `#Uninit` (grep
`examples/` for `#Uninit` — none surfaced in this pass's research, so this
may be the first example) to `examples/memory/uninit-buffer.jet` using the
new `:= uninit` spelling; keep the packet-header `inWild` sample from the
ballot as the shape to follow.

**Formatter:** `:= uninit` is new grammar in the initializer slot — needs
formatter emission (should just print the keyword literally, but verify
round-trip) + fmt STABILITY test.

**Tests:** `cargo test --test ui -- uninit` (targeted), full suite at end.

**Exit criteria:**
- [ ] `name: Type := uninit` parses, requires an explicit type, requires
      `use core.mem`, requires a plain-data type.
- [ ] Write-before-read proof reuses `CheckerCore.rs`'s existing tracking —
      no parallel flow-analysis implementation.
- [ ] Old `#Uninit name: Type` is a hard parse error (E0425) pointing at the
      new spelling, not silently still accepted (I8).
- [ ] E0420/E0421/E0423/E0424 reworded and re-blessed; E0422 marked retired
      in the registry table.
- [ ] `nix develop -c cargo test` green.

---

## 8. c7refshorthand — inferred owner `&T`, `@Ref(label)` to disambiguate

**D-REF-SHORTHAND1 = D.** Stored-ref fields spell the type `&T` — the same
sigil already used for shared borrows at call sites (`Type::Shared`, D-CAP7)
— instead of a bare `T` plus a separate marker. The owner is **inferred**
when exactly one candidate exists at the construction site; `@Ref(label)` is
required only to disambiguate when several candidates exist, and the error
listing candidates is what teaches the label's purpose. `~T` and
always-required labels both stay rejected.

**⚠ Unresolved conflict — flag before implementing (see Ambiguities below):**
D-MARKERMOVE1's ratified text explicitly lists `#Ref(Label)` under markers
that **stay** `#` ("Clear stays: `#Unsafe`, `#Test`, `#Bench`, `#Ref(Label)`,
`#Grant`, ..."). D-REF-SHORTHAND1's ratified option D code sample spells it
`@Ref(pool)`. Both ratified the same day. Do not guess — get a one-line
reconciliation from the owner (or a follow-up ballot) before locking the
sigil in a snapshot test. This plan defaults to `@Ref(label)` (the literal
text of D-REF-SHORTHAND1's chosen option, per this task's own instruction to
implement the ratified option's exact text) but the two decisions
contradict each other and someone needs to close that gap.

**What exists today (implemented, reuse — do not rebuild):**
- `crates/jet-foundation/src/AST.rs:1511-1512` — `Field.is_stored_ref`,
  `Field.stored_ref_label`.
- `crates/jet-parser/src/Parser/Items.rs:3548-3630` — `#Ref(owner) name: T`
  parser (`field()` function). Old bracket form `ref[label]` already
  retired to a teaching error (`tests/ui/ref_retired_bracket.jet`) — expect
  this card to do the same migration dance a second time for `#Ref(...)` →
  `&T`/`@Ref(...)`.
- `crates/jet-sema/src/Sema/CheckerOwnership.rs:83` —
  `check_stored_ref_fields()`.
- `crates/jet-sema/src/Sema/Registration.rs:1406` — `E0207`, "multiple
  unlabeled ref fields," fires unconditionally today whenever 2+ `#Ref`
  fields lack labels. **This is exactly the rule the ballot replaces**: the
  new law only errors when genuinely ambiguous (2+ *candidate owners*, not
  2+ ref fields), and the error must list candidates by name.
- `Type::Shared(Box<Type>)` (`crates/jet-foundation/src/AST.rs:251`) is the
  existing `&T` AST node, currently parsed for params/`-> &T` returns
  (`crates/jet-parser/src/Parser/Items.rs:2574-2589`,
  `parse_view_return_marker`/`parse_access_prefix`) but **not yet accepted
  in field position** (`field()` at `Items.rs:3548` parses a plain type).

**Build order:**
1. Parser: allow `Type::Shared` (`&T`) as a field type in `field()`
   (`Items.rs:3548`) — this is the new signal that a field is a stored ref,
   replacing the separate `is_stored_ref` bool derived from a `#Ref`/`@Ref`
   prefix. `stored_ref_label` stays `Option<String>`, now populated by
   either inference (step 3) or an explicit `@Ref(label)`/`#Ref(label)`
   prefix per whichever spelling the reconciliation above settles.
2. Parser: retire the old `#Ref(owner) name: T` (plain-type) form to a
   teaching error once the new form ships — same "old spelling → E0xxx
   pointing at new spelling" pattern used for `ref[label]` and (§7) `#Uninit`.
3. Sema (`Registration.rs`/`CheckerOwnership.rs`): candidate-owner inference
   — at a struct literal's construction site, find every in-scope value
   whose type/lifetime could be "the owner" of a `&T`-typed field. Exactly
   one candidate → infer, no label needed. Zero or 2+ candidates → error
   listing them by name, pointing at `@Ref(label)`/`#Ref(label)` as the fix.
   This is genuinely new inference logic, not a reuse of anything — budget
   real design time for "what counts as a candidate owner" (likely: other
   fields of the same struct literal whose type matches the referent, plus
   parameters in scope at construction — pin the exact rule in
   `docs/spec/spec.md` before coding).
4. `jet expand --facts refs` (ballot's transparency mechanism, "materializes
   the resolved owner") — **does not exist** (no `expand`/`--facts`
   subcommand anywhere in `crates/jet-driver`/`Source/`). Same situation as
   §1's `--facts inline`: name it as deferred/out-of-scope explicitly, don't
   silently drop it.

**New diagnostics** (rework `E0207`, add new codes in the Tier-2 reference
family E23xx, next free after E2304/L2301 — and after whatever §2 claims as
E2305, so recheck the registry table immediately before landing):
- `E0207` — **reworded**, same code, narrower trigger: fires only when a
  `&T` field's owner is genuinely ambiguous (2+ candidates), and the message
  lists every candidate by name. Old unconditional "any unlabeled ref field"
  trigger goes away — re-bless `ref_fields_unlabeled.stderr` against the new
  narrower behavior (it may need a companion single-candidate-infers-fine
  test added alongside it, since the old fixture's premise may no longer
  error at all).
- `E2306` (next after §2's E2305) — `@Ref(label)`/`#Ref(label)` names a
  label that doesn't match any in-scope candidate owner. What: "no
  candidate named `{label}` for `{field}`." Why: "the label must name one of
  the values that could own this borrow." Fix: lists the actual candidate
  names.
- Fixtures: `tests/ui/ref_owner_inferred.jet` (positive case, no label
  needed, one candidate), `tests/ui/ref_owner_ambiguous.jet`/`.stderr`
  (reworked E0207), `tests/ui/ref_label_unknown_candidate.jet`/`.stderr`
  (E2306), `tests/ui/ref_old_marker_retired.jet`/`.stderr` (old `#Ref(owner)
  name: T` plain-type form → teaching error).

**Example:** `examples/ownership/ref-shorthand.jet` — the ballot's `Owner`/
`Index` sample (one inferred, one ambiguous-requires-label) — update or
supersede `examples/features/09_ref_field.jet`/`183_ref_owner.jet` in place
(they demonstrate the OLD spelling; either migrate them or add new
topic-dir examples and mark the old ones for removal once migration lands —
don't leave two examples teaching two spellings, I8).

**Formatter:** `&T` in field position + optional `@Ref(label)`/`#Ref(label)`
prefix is new-shape grammar even though `&` itself isn't new — needs
formatter emission + fmt STABILITY test (own-memory rule).

**Tests:** `cargo test --test ui -- ref_` (targeted), full suite at the end.

**Exit criteria:**
- [ ] Owner/marker sigil conflict (D-MARKERMOVE1 vs D-REF-SHORTHAND1)
      reconciled with the owner before any snapshot locks in a spelling.
- [ ] `field: &T` infers its owner when exactly one candidate exists — zero
      ceremony, verified by a passing example with no label.
- [ ] Two-candidate case is a compile error listing both candidates by name.
- [ ] `@Ref(label)`/`#Ref(label)` (per reconciled spelling) resolves the
      ambiguity; unknown label name is E2306.
- [ ] Old `#Ref(owner) name: T` (plain-type) form is a hard parse error
      pointing at the new spelling.
- [ ] `docs/spec/spec.md` stored-ref section rewritten for the new law.
- [ ] `nix develop -c cargo test` green.

---

## Cross-card notes

- **Sequencing:** §1/§3/§8 all need the `@` contract plane. Author and land
  `tools/Tower/docs/plans/epoch-3/marker-family.md` (does not exist yet) or
  get an explicit owner call to implement these three provisionally under
  `#` and mechanically re-sigil later. Either way, say which you're doing in
  the card log — don't leave it implicit.
- **E-code ranges are a moving target** across these eight cards — §2, §3,
  §4, §6, §8 all claim slots in adjoining families (E13xx, E23xx). Whoever
  implements each card should re-grep `docs/spec/diagnostics.md` for the
  actual next-free code immediately before landing, not trust the numbers
  drafted here if another card landed first and shifted the free range.
- **`jet expand --facts <lens>`** is named as the transparency mechanism in
  three separate ballots (inline §1, refs §8, and implicitly §2's owner
  tracking) and **does not exist anywhere in the driver**. This is worth its
  own small card/ballot rather than three separate half-built copies.
- Every card that touches parseable syntax needs a formatter emission path
  and a fmt STABILITY round-trip test — called out per-section above, don't
  skip it (memory: dropped tokens on `jet fmt` shipped silently before).

## Ambiguities to raise with the owner (verbatim, do not resolve unilaterally)

1. **"#Ref(Label) vs @Ref(label)"** — D-MARKERMOVE1(=B) ratified text: *"Clear
   stays: `#Unsafe`, `#Test`, `#Bench`, `#Ref(Label)`, `#Grant`, ..."*
   D-REF-SHORTHAND1(=D) ratified option code: `@Ref(pool) hot: &Incident`.
   Same ratification date, contradictory sigil for the same marker.
2. **"jet <verb> -> fn <verb>()" lifecycle-verbs status** — D-CLIFLAG1's own
   `detail` field says D-BUILDENTRY1 integrates it, but D-BUILDENTRY1's own
   `detail` field contains a self-correction: *"an earlier edit claimed the
   lifecycle-verbs law ... was ratified — it is NOT; it is an open proposal."*
   D-CLIFLAG1's `fn run(args: T)` entry-param mechanism does not actually
   depend on the open lifecycle-verbs law (it only needs `fn run` to exist,
   which it already does) — flagging so nobody blocks c7cliflag on an open
   ballot it doesn't structurally need.
3. **"@[Cli]/@Doc marker names"** — neither appears in D-MARKERMOVE1's move
   list (that ballot only resolves markers migrating FROM `#`; `Cli`/`Doc`
   are net-new markers, never `#`-spelled). D-CLIFLAG1's own ratified option
   code sample spells them `@[Cli]`/`@Doc` directly, so this plan takes that
   as binding, but it's worth a one-line owner confirmation since
   D-MARKERMOVE1 was framed as the definitive spelling authority and didn't
   cover this pair.
4. **Multi-trait variadic bounds** — the ballot's `comment` field ("can @-
   varargs bind a set of traits, not just one?") is an open owner question
   inside an otherwise-ratified decision, not itself ratified text. §6 above
   assumes reuse of S45's `<T: A + B>` syntax as the answer; get that
   confirmed before locking a `...{A + B}` (or whatever spelling) into a
   snapshot.
5. **c7pointerchain's `mem.cast_ptr<T>`** — the ratified option's own worked
   example uses an API (`mem.cast_ptr<Bool>(...)`) that does not exist in
   `core.mem` today. Confirm whether this card silently also needs to add
   that primitive, or whether an existing (differently-named) cast already
   covers it — don't invent a name unreviewed.
