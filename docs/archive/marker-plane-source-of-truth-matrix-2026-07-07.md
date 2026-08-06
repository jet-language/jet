# Applied-rule source-of-truth matrix - 2026-07-17

Purpose: one compact map for marker sigils and marker-like planes. This is law
reading aid, not a new syntax decision. If a future marker does not fit one row
here, queue a Tower ballot before adding it.

## Rule law

D-SHAPE2 supersedes D-MARKER-FAMILY1: `#Rule` is the sole syntax for applying
a typed rule to a declaration, expression, or brace scope. `#[...]` is the list form.
Effect rows `--[...]->` are not rules. Fixed lists `[T#N]`, package selectors
`pkg#version`, and `#Caller()` retain `#`. `derive T.Trait` is a body form;
`comptime` and `$name` are metaprogramming syntax.

Current registry census: 75 registered applied rules (all `@`).

| Row | Plane | Canonical spellings | Law | Parser/formatter anchors | Status |
|-----|-------|---------------------|-----|--------------------------|--------|
| `file-target-directives` | `#` directive | `#PubFile`, `#NoPrelude`, `#Target(Web)`, `#Target(OS.Linux)`, `#Target(Wasm)`, `#Target(JS)`, `#HTML("path.html")`, `#WasmExport` | S18, D-VISDEFAULT2, D-PRELUDEX1, D-WEBDEFAULT1, D-HTMLPAIR1, D-OSTARGET1, D-WASM1, D-MARK-TARGET1 | `MARKER_PUB_FILE`, `MARKER_NO_PRELUDE`, `MARKER_TARGET`, `MARKER_HTML`, `MARKER_WASM_EXPORT`; parser fixed-position/file and item markers; fmt web/OS marker tests | Shipped; bare `#Wasm`/`#JS` retired |
| `type-layout-directives` | `#` directive | `#Layout(c)`, `#Layout(columnar)`, `#SingleUse`, `#UnitFamily(Time)` | D-REPRC1, D-SOA1, D-LIN1, D-QUAL3 | `MARKER_LAYOUT`, `MARKER_SINGLE_USE`, `MARKER_UNIT_FAMILY`; parser type-prefix marker paths; fmt layout/single-use/unit-family tests | Shipped; `packed`, `align`, and partial columnar remain reserved |
| `serde-directive-attributes` | `#` directive list | `#[Rename("wire")]`, `#[Skip]`, `#[Default]`, `#[Flatten]`, `#RenameAll(camel)`, `#Tag("type")`, `#Untagged`, `#DenyUnknownFields` | D-SERDE3, D-SERDE5, D-SERDE7, D-SERDE8, D-MARKERMOVE1 | `MARKER_RENAME`, `MARKER_SKIP`, `MARKER_DEFAULT`, `MARKER_FLATTEN`, `MARKER_RENAME_ALL`, `MARKER_TAG`, `MARKER_UNTAGGED`, `MARKER_DENY_UNKNOWN_FIELDS`; `parse_hash_marker_group`; fmt serde marker tests | Shipped |
| `derive-contract-markers` | `@` contract | `#Codable`, `#[Encode, Decode]`, `#Summarize`, `#Comparable` | S55, D-SERDE4, D-MARKERMOVE3 | `MARKER_CODABLE`, `MARKER_ENCODE`, `MARKER_DECODE`, `MARKER_SUMMARIZE`, `MARKER_COMPARABLE`; `parse_at_marker_group`; fmt derive/contract tests | Shipped |
| `general-contract-markers` | `@` rule | `#Pure`, `#MustUse`, `#Pre(...)`, `#Post(...)`, `#Inline`, `#InlineAlways`, `#Persist`, `#CLI`, `#[Doc("...")]`, `#Patchable`, `#PublishedSchema` | D-SHAPE2, D-EFF3, D-MUSTUSE1, D-PREPOST1, D-METHODMACRO1, D-PERSIST1, D-CLIFLAG1, D-PATCH1, D-MIGRATE1 | `APPLIED_RULES`, `MARKER_MUST_USE`, `MARKER_PRE`, `MARKER_POST`, `MARKER_INLINE`, `MARKER_INLINE_ALWAYS`, `MARKER_PERSIST`, `MARKER_CLI`, `MARKER_DOC`, `MARKER_PATCHABLE`, `MARKER_PUBLISHED_SCHEMA`; parser item/type/field rule paths; fmt rule tests | Shipped |
| `distinct-capability-bundles` | `@` contract bundle | `#Numeric`, `#Comparable`, `#Printable`, `#CodableAsBase` | D-DIST3, D-CAPBUNDLE1 | `MARKER_NUMERIC`, `MARKER_BUNDLE_COMPARABLE`, `MARKER_BUNDLE_PRINTABLE`, `MARKER_BUNDLE_CODABLE_AS_BASE`; `distinct_def`; fmt distinct bundle tests | Shipped |
| `effect-capability-directives` | effect row / `@` rule | `--[FS]->`, `--[FS.Read, !FS.Write]->`, `--[via f]->`, `#Caps(Net) { ... }`, `#Grant(FS.Read) { caps -> ... }` | D-EFF1, D-EFF2, D-EFFTREE1, D-PROP1, D-SCAP1, D-SHAPE8 | `EFFECT_ARROW_OPEN`, `EFFECT_ARROW_CLOSE`, `KW_CAPS`, `KW_GRANT`, `GRANT_ARROW`; parser effect-bound, caps, and grant paths; fmt dotted effect paths test | Shipped; former `#(Effects)` / `#(via f)` rows are rejected with E0066 |
| `unsafe-impure-gates` | `#` audit gate | `#Unsafe("reason") { ... }`, `#Unsafe("reason") fn`, `#Impure("reason") { ... }` | D-UNSAFE2, D-UNSAFE-REASON1, D-CTEFFECT1 | `#Unsafe`/`#Impure` parser statement/item paths; fmt unsafe and impure gate tests; diagnostics enforce reason for unsafe | Shipped; bare unsafe is rejected |
| `test-bench-directives` | `#` runnable block | `#Test("name") { ... }`, `#Test fn prop(p: T)`, `#Bench("name") { ... }` | S43, D-TESTPAREN1, D-TEST1, D-BENCH1, D-BENCH-MARKER1 | `KW_TEST`, `KW_BENCH`; `test_def`, `bench_def`; fmt test marker tests | Shipped for current grammar |
| `typing-fact-directives` | `#` value/type fact | `#Tainted expr`, `#Tainted String`, `#Sanitizer fn`, `#Replayable fn`, `#State(S)`, `#Transition(A -> B)`, `#Track name :: expr` | D-TAINT1, D-REPLAY1, D-QUAL4, D-STATE1, D-PROVENANCE1 | `KW_TAINTED`, `KW_SANITIZER`, `MARKER_REPLAYABLE`, `MARKER_TRACK`; parser expression/type/fn/binding marker paths; fmt taint/replayable/state/track tests | Shipped; `#Suppress` retired by D-MARK-DISCARD1 |
| `comptime-metaprogramming` | keyword/splice, not marker | `comptime { ... }`, `comptime name = ...`, `$name`, `derive T.Trait { ... }`, `find(glob)` | D-CTMARKER1, D-CTBLOCKEXPOSE1, D-METADERIVE1, D-CTCODEGEN1, D-CTFIND1/2 | `KW_COMPTIME`, `KW_DERIVE`, `BUILTIN_FIND`; parser comptime binding/block/splice and derive body paths; comptime sorted glob + lock inputs; fmt comptime/derive tests | Shipped |
| `capability-sigils` | ownership/capability surface | `^T`, `&T`, `~x`, postfix `p.*`; reserved words `edit` and `share` | S10, D-CAP7, D-SHAPE-COPY1, D-CAP3, D-CAP9 | capability syntax constants and parser type/expression paths; syntax reconciliation retired-word guard | Shipped current memory-v5 surface (D-SHAPE-COPY1=A supersedes D-CAP2's `copy x`); `edit` and `share` reserved only |
| `maturity-markers` | `#Meta` tooling field | `#Meta(maturity: .Experimental | .Tested | .Hardened)` | D-MARK-META1=B | `META_FIELD_MATURITY`; closed maturity values; `MaturityTag` on `Func`; fmt preserves; standalone `@`/`#` forms reject; `examples/features/syntax/maturity_tags.jet` | Shipped doc-only (no sema propagation) |
| `retired-paused-marker-spellings` | none | `@unsafe`, `@audit`, `@extern`, `@bindgen`, `#extern`, `#bindgen`, `#layout`, `#grant`, `#context`, `#test`, `#pure`, `#todo`, `#Bench "name"`, `#[Serialize]`, `#[Deserialize]` | D-S14-PAUSE, D-CANON-SOURCE1 | `tests/syntax_reconciliation.rs` forbidden-list guard | Retired; ordinary syntax errors unless a future teaching-mode decision reopens them |
| `retired-derive-markers` | `@` contract (retired) | `#Debug`, `#[.., Debug]`, body `derive Debug;` | D-MARK-DEBUG1=A (card #498) | `Traits.rs` explicit-derive check; `Generics::e0922` | Retired: `Debug` auto-derives (S55); writing it explicitly is E0922, not a wrong-plane teaching error — `impl T.Debug { … }` hand overrides and `{value#Debug}` reflection stay valid |

## Reconciliation notes

- `#Extern` and `#Bindgen` have uppercase constants for historical C/FFI work,
  but the D-S14 pause means old lowercase spellings stay out of live teaching
  paths. Do not add examples without checking the current FFI card.
- `#Comparable` appears in two rows because the spelling is shared by derive
  contracts and distinct-type capability bundles. Context disambiguates it:
  type marker groups derive behavior; distinct declarations bundle operators.
- `#Suppress(MustUse)` is retired by D-MARK-DISCARD1. `.drop("reason")` is the
  sole explicit discard spelling.
- `derive T.Trait { ... }` belongs in this matrix because agents often confuse
  it with marker planes. It is intentionally a keyword body form, not `@` or
  `#`.
