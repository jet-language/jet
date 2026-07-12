# Marker-plane source-of-truth matrix - 2026-07-07

Purpose: one compact map for marker sigils and marker-like planes. This is law
reading aid, not a new syntax decision. If a future marker does not fit one row
here, queue a Tower ballot before adding it.

## Plane law

D-MARKER-FAMILY1 is the top-level split: `@` states a checkable contract or
capability bundle. `#` changes what compiles, runs, is emitted, or is
authorized. `#[...]` is the directive attribute-list form used where
field/container attributes need list syntax. `derive T.Trait` is a body form,
not a marker. `comptime` and `$name` are metaprogramming syntax, not `#` or `@`
markers.

| Row | Plane | Canonical spellings | Law | Parser/formatter anchors | Status |
|-----|-------|---------------------|-----|--------------------------|--------|
| `file-target-directives` | `#` directive | `#PubFile`, `#NoPrelude`, `#Target(Web)`, `#Target(Os.Linux)`, `#Html("path.html")`, `#Js`, `#Wasm`, `#WasmExport` | S18, D-VISDEFAULT2, D-PRELUDEX1, D-WEBDEFAULT1, D-HTMLPAIR1, D-OSTARGET1, D-WASM1 | `MARKER_PUB_FILE`, `MARKER_NO_PRELUDE`, `ATTR_TARGET`, `ATTR_HTML`, `ATTR_JS`, `ATTR_WASM`, `ATTR_WASM_EXPORT`; parser fixed-position/file and item markers; fmt web/OS marker tests | Shipped |
| `type-layout-directives` | `#` directive | `#Layout(c)`, `#Layout(columnar)`, `#SingleUse`, `#UnitFamily(time)` | D-REPRC1, D-SOA1, D-LIN1, D-QUAL3 | `ATTR_LAYOUT`, `ATTR_SINGLE_USE`, `ATTR_UNIT_FAMILY`; parser type-prefix marker paths; fmt layout/single-use/unit-family tests | Shipped; `packed`, `align`, and partial columnar remain reserved |
| `serde-directive-attributes` | `#` directive list | `#[Rename("wire")]`, `#[Skip]`, `#[Default]`, `#[Flatten]`, `#RenameAll(camel)`, `#Tag("type")`, `#Untagged`, `#DenyUnknownFields` | D-SERDE3, D-SERDE5, D-SERDE7, D-SERDE8, D-MARKERMOVE1 | `ATTR_RENAME`, `ATTR_SKIP`, `ATTR_DEFAULT`, `ATTR_FLATTEN`, `ATTR_RENAME_ALL`, `ATTR_TAG`, `ATTR_UNTAGGED`, `ATTR_DENY_UNKNOWN_FIELDS`; `parse_hash_marker_group`; fmt serde marker tests | Shipped |
| `derive-contract-markers` | `@` contract | `@Codable`, `@[Encode, Decode]`, `@Debug`, `@Summarize`, `@Comparable` | S55, D-SERDE4, D-MARKERMOVE3 | `ATTR_CODABLE`, `ATTR_ENCODE`, `ATTR_DECODE`, `ATTR_SUMMARIZE`, `ATTR_COMPARABLE`; `parse_at_marker_group`; fmt derive/contract tests | Shipped |
| `general-contract-markers` | `@` contract | `@Pure`, `@MustUse`, `@Pre(...)`, `@Post(...)`, `@Inline`, `@InlineAlways`, `@Persist`, `@Cli`, `@[Doc("...")]`, `@Patchable`, `@PublishedSchema` | D-EFF3, D-MUSTUSE1, D-PREPOST1, D-METHODMACRO1, D-PERSIST1, D-CLIFLAG1, D-PATCH1, D-MIGRATE1 | `CONTRACT_MARKERS`, `ATTR_MUST_USE`, `CONTRACT_PRE`, `CONTRACT_POST`, `CONTRACT_INLINE`, `CONTRACT_INLINE_ALWAYS`, `CONTRACT_PERSIST`, `CONTRACT_CLI`, `CONTRACT_DOC`, `CONTRACT_PATCHABLE`, `ATTR_PUBLISHED_SCHEMA`; parser item/type/field contract paths; fmt contract tests | Shipped |
| `distinct-capability-bundles` | `@` contract bundle | `@Numeric`, `@Comparable`, `@Printable`, `@CodableAsBase` | D-DIST3, D-CAPBUNDLE1 | `ATTR_NUMERIC`, `CONTRACT_BUNDLE_COMPARABLE`, `CONTRACT_BUNDLE_PRINTABLE`, `CONTRACT_BUNDLE_CODABLE_AS_BASE`; `distinct_def`; fmt distinct bundle tests | Shipped |
| `effect-capability-directives` | `#` effect/capability | `#(Fs)`, `#(Fs.Read, !Fs.Write)`, `#(via f)`, `#Caps(Net) { ... }`, `#Grant(Fs.Read) { caps -> ... }` | D-EFF1, D-EFF2, D-EFFTREE1, D-PROP1, D-SCAP1 | `KW_CAPS`, `KW_GRANT`, `GRANT_ARROW`; parser effect-bound, caps, and grant paths; fmt dotted effect paths test | Shipped |
| `unsafe-impure-gates` | `#` audit gate | `#Unsafe("reason") { ... }`, `#Unsafe("reason") fn`, `#Impure("reason") { ... }` | D-UNSAFE2, D-UNSAFE-REASON1, D-CTEFFECT1 | `#Unsafe`/`#Impure` parser statement/item paths; fmt unsafe and impure gate tests; diagnostics enforce reason for unsafe | Shipped; bare unsafe is rejected |
| `test-bench-directives` | `#` runnable block | `#Test("name") { ... }`, `#Test fn prop(p: T)`, `#Bench("name") { ... }` | S43, D-TESTPAREN1, D-TEST1, D-BENCH1, D-BENCH-MARKER1 | `KW_TEST`, `KW_BENCH`; `test_def`, `bench_def`; fmt test marker tests | Shipped for current grammar |
| `typing-fact-directives` | `#` value/type fact | `#Tainted expr`, `#Tainted String`, `#Sanitizer fn`, `#Replayable fn`, `#State(S)`, `#Transition(A -> B)`, `#Suppress(MustUse)`, `#Track name :: expr` | D-TAINT1, D-REPLAY1, D-QUAL4, D-STATE1, D-IGNORERET2, D-PROVENANCE1 | `KW_TAINTED`, `KW_SANITIZER`, `ATTR_REPLAYABLE`, `ATTR_SUPPRESS`, `ATTR_TRACK`; parser expression/type/fn/binding marker paths; fmt taint/replayable/state/track tests | Shipped where parser accepts the row; `#Suppress` remains tied to must-use follow-up work |
| `comptime-metaprogramming` | keyword/splice, not marker | `comptime { ... }`, `comptime NAME = ...`, `$name`, `derive T.Trait { ... }`, `find(glob)` | D-CTMARKER1, D-CTBLOCKEXPOSE1, D-METADERIVE1, D-CTCODEGEN1, D-CTFIND1/2 | `KW_COMPTIME`, `KW_DERIVE`, `BUILTIN_FIND`; parser comptime binding/block/splice and derive body paths; comptime sorted glob + lock inputs; fmt comptime/derive tests | Shipped |
| `capability-sigils` | ownership/capability surface | `^T`, `&T`, `copy x`, postfix `p.*`; reserved words `edit` and `share` | S10, D-CAP7, D-CAP2, D-CAP3, D-CAP9 | capability syntax constants and parser type/expression paths; syntax reconciliation retired-word guard | Shipped current memory-v5 surface; `edit` and `share` reserved only |
| `maturity-markers` | `#Meta` tooling field | `#Meta(maturity: .Experimental | .Tested | .Hardened)` | D-MARK-META1=B | `META_FIELD_MATURITY`; closed maturity values; `MaturityTag` on `Func`; fmt preserves; standalone `@`/`#` forms reject; `examples/features/syntax/maturity_tags.jet` | Shipped doc-only (no sema propagation) |
| `retired-paused-marker-spellings` | none | `@unsafe`, `@audit`, `@extern`, `@bindgen`, `#extern`, `#bindgen`, `#layout`, `#grant`, `#context`, `#test`, `#pure`, `#todo`, `#Bench "name"`, `#[Serialize]`, `#[Deserialize]` | D-S14-PAUSE, D-CANON-SOURCE1 | `tests/syntax_reconciliation.rs` forbidden-list guard | Retired; ordinary syntax errors unless a future teaching-mode decision reopens them |

## Reconciliation notes

- `#Extern` and `#Bindgen` have uppercase constants for historical C/FFI work,
  but the D-S14 pause means old lowercase spellings stay out of live teaching
  paths. Do not add examples without checking the current FFI card.
- `@Comparable` appears in two rows because the spelling is shared by derive
  contracts and distinct-type capability bundles. Context disambiguates it:
  type marker groups derive behavior; distinct declarations bundle operators.
- `#Suppress(MustUse)` is accepted as the current ignore channel. Broader
  discard behavior is tracked separately by the must-use/ignore-return cards.
- `derive T.Trait { ... }` belongs in this matrix because agents often confuse
  it with marker planes. It is intentionally a keyword body form, not `@` or
  `#`.
