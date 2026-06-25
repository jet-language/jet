# core.encoding — unified serialization (merges c89 + c90 + c104)

**Decisions:** D-SERDE1 (A, ratified 2026-06-22) · D-JSONOUT1 (A) · D-CSVROW1 (A) ·
**D-ENC1 (ratified 2026-06-24 by owner)** — namespace + access surface + clean migration.

This card supersedes and folds in **c89 (typed CSV rows)** and **c90 (typed JSON output)**.
One library, one data model, one derive — every format is an arm of it. No parallel paths.

---

## The vision (Serde, for Jet)

A type says *once* that it serializes; every format — JSON, CSV, TOML, YAML, and any
future one — gets it for free. Beginners write `encoding.json.render(order)` and it
works; experts add field attributes for exact wire control. The front end owns the whole
story; rustc only verifies the generated `impl`s.

```jet
use core.encoding

#[Serialize, Deserialize]
struct Order {
    id: Int
    #[rename("customer")] who: String
    items: [String]
    note: String?
}

fn main() {
    raw  @= core.fs.read("orders.csv") ?? panic("no file")
    rows @= encoding.csv.decode<Order>(raw) ?? panic("bad csv")   // CSV → [Order]
    print(encoding.json.render(rows))                              // [Order] → JSON
}
```

CSV-in → typed struct → JSON-out is Elena's pipeline (the through-line of c89+c90). Under
this model it is three calls against one derive.

---

## Architecture (the Serde shape)

```
            #[Serialize] / #[Deserialize]   (built-in derive, compiler-owned codegen)
                          │
                   to_data / from_data
                          ▼
                    ┌───────────┐
                    │ DataValue │   format-agnostic value tree (the wire)
                    └───────────┘
                    ▲     ▲     ▲     ▲
            json ── │     │     │     │ ── yaml
                  csv ────┘     └──── toml      (each: Serializer + Deserializer adapter)
```

- **`DataValue`** — the one value tree all formats speak. Lives in Core (`CoreLib.rs`).
- **`Serialize` / `Deserialize`** — `to_data() -> DataValue` / `from_data(&DataValue) ->
  Result<Self, EncodingError>`. Hand-written blanket impls for primitives, `[T]`,
  `Map<K,V>`, `T?`; compiler-derived impls for user structs/enums.
- **Format adapters** — `encoding.{json,csv,toml,yaml}`, each a `Serializer`/`Deserializer`
  over `DataValue`. One derive drives all of them; adapters never call each other.
- **`decode<T>` / `encode<T>`** — generic surface: `decode<T>(text) = T::from_data(parse(text))`,
  `encode<T>(v) = render(v.to_data())`. Pure monomorphization + the derived trait impl.

### Why built-in derive, not comptime, not S56

The c89/c90/c104 plans assumed comptime field reflection drives this. It does not exist
(comptime introspects *values*, not type declarations; there's no generic-type-token path
into comptime). The correct, already-precedented vehicle is a **built-in derive whose
codegen the compiler owns** — exactly how `derive Comparable` lowers to `PartialOrd`. Sema
knows the struct's fields; codegen emits the `to_data`/`from_data` walk. This is *not* the
S56 user-defined-derive system (users authoring their own derive logic via `~~`), so it is
**not gated on S56**. S56 later lets users write *their own* derives against this same model.

---

## D-ENC1 — what the owner decided (2026-06-24)

1. **One library: `core.encoding`**, each format a submodule (`core.encoding.json`,
   `.csv`, `.toml`, `.yaml`; extensible — binary, etc.).
2. **Two import surfaces, both supported:**
   - whole library — `use core.encoding` → `encoding.json.render(x)`,
     `encoding.csv.decode<Row>(rec)` (new nested-namespace access machinery);
   - terse leaf — `use core.encoding.json as json` → `json.render(x)` (existing flat path).
3. **Clean break, migrate all.** Retire `core.json` and `jet.csv` / `jet.toml` /
   `jet.yaml`; everything moves under `core.encoding.*`. No deprecated alias. The four
   existing examples (`30_json`, `51_csv`, `52_toml`, `53_yaml`) migrate.
4. **Full field attributes:** `#[rename]`, `#[default]`, `#[skip]`, `#[flatten]`,
   `#[rename_all]` (D-SERDE1) all ship.

---

## Build plan

### 1 — DataValue model + traits (CoreLib.rs)
```rust
pub enum DataValue {
    Null, Bool(bool), Int(i64), Float(f64), Str(String), Bytes(Vec<u8>),
    Seq(Vec<DataValue>),
    Map(Vec<(String, DataValue)>),   // ordered; keys are strings at this layer
}
pub trait Serialize   { fn to_data(&self) -> DataValue; }
pub trait Deserialize: Sized { fn from_data(v: &DataValue) -> Result<Self, EncodingError>; }
```
`EncodingError { path: String, reason: String }` carries a field path (`"order.items[2]"`).
Blanket impls for `bool/i64/f64/String/char`, `Vec<T: Serialize>`, `Option<T>`,
`BTreeMap<String,V>`. Pure std Rust, zero external crates (I6). The existing `Json` enum
becomes the json adapter's surface; its render/parse fold onto `DataValue`.

### 2 — Module registration + nested access
Register `core.encoding{,.json,.csv,.toml,.yaml}` in `Loader.rs::KNOWN_CORE_MODULES`,
`CheckerCoreLib::core_module_items` + `core_fixed_sig`, and `TIR::emit_tir_core_call`.
Nested access (`encoding.json.render(x)`): in call resolution, recognize
`alias.<leaf>.method(args)` where `alias` is the `core.encoding` library and `<leaf>` a
registered submodule → dispatch as module `core.encoding.<leaf>`, method `method`. Leaf
imports reuse the flat-module machinery unchanged.

### 3 — Format adapters over DataValue
`encoding.json` (render/render_pretty/parse + decode/encode), `encoding.csv`
(decode<Row>/encode rows, header-name mapping + typed per-row error composing with `??`),
`encoding.toml`, `encoding.yaml`. Each routes through `DataValue`; toml/yaml gain nested
Map/Seq support via the model. Folds the three duplicate ad-hoc JSON emitters
(`quote_json`, `jet_log_json_escape`, the coerce emitter) onto one path where practical.

### 4 — `#[Serialize, Deserialize]` derive codegen
Parse bracket markers on a struct/enum (D-ATTR2) → AST `derives`. Codegen emits
`impl Serialize for user_<T>` / `impl Deserialize for user_<T>`: `to_data` walks fields in
order building `DataValue::Map`/`Seq`; `from_data` reverse-walks, erroring on missing
required fields. Plain Rust, no proc macros (I6), no `unsafe` (I1). Enums serialize as a
tagged `Map`.

### 5 — Field attributes (sema + codegen)
Parse field-level `#[...]`. Sema checks before codegen; codegen applies during the walk:
- `#[rename("x")]` — substitute the key.
- `#[rename_all("camelCase"|"snake_case"|"PascalCase")]` — struct-level key transform.
- `#[default]` — use `T::default()` when the key is absent on decode.
- `#[skip]` — omit on serialize; require `#[default]` or `T?` on deserialize.
- `#[flatten]` — inline a struct-typed field's `Map` entries into the parent.

### Diagnostics (I4)
| Code | Meaning |
|------|---------|
| E2407 | `#[rename]` expects a string literal |
| E2408 | `#[flatten]` requires a struct-typed field |
| E2409 | `#[rename_all]` unrecognized casing style |
| E2410 | deserialization missing required field `x: T` |
| E2411 | type isn't serializable (e.g. holds a closure/handle) |

E2401–E2406 are taken; E2407 is the first free E24xx slot. Each needs a
`docs/spec/diagnostics.md` row + a `tests/ui/` snapshot.

---

## Examples (I5)
- `examples/features/30_json.jet`, `51_csv.jet`, `52_toml.jet`, `53_yaml.jet` — migrated to
  `core.encoding.*`.
- `serde_basic.jet` — hand-written `impl Serialize`, no derive (proves the trait layer).
- `serde_derive.jet` — `#[Serialize, Deserialize]` round-trip through JSON with `#[rename]`.
- `csv_typed.jet` — `csv.decode<Order>`, one malformed row skipped via `??`, typed totals.
- `json_typed.jet` — struct → JSON → struct round-trip, nested struct + list + optional.

## Test plan
1. Golden output for every example above.
2. Per-field CSV coercion-failure snapshot (row + column + expected type).
3. Header-reorder: same struct decodes when columns are reordered.
4. Missing-column / missing-required-field snapshots (E2410).
5. `#[rename]`/`#[rename_all]`/`#[skip]`/`#[default]`/`#[flatten]` round-trips.
6. Non-serializable field → E2411 snapshot.

## Status
D-ENC1 ratified 2026-06-24. No open owner decision. Buildable now end-to-end — no S56
gate (built-in derive, not user-defined derives). c89 + c90 folded here.

### Shipped (2026-06-24)
- **Build plan §1 (partial), §2, §3.** `core.encoding` library + `{json,csv,toml,yaml}`
  submodules registered (Loader / CheckerCoreLib sigs+items / TIR dispatch).
- Both import surfaces: leaf `use core.encoding.json as json` AND whole-library
  `use core.encoding` → `encoding.json.to_string(x)` (new nested-namespace access wired
  through sema infer, the codegen subset gate, codegen lowering, and `collect_used_core`).
- D-JSONVERB1 verbs `to_string` / `to_string_pretty` implemented, uniform across formats;
  `parse` (dynamic) + json `decode` (lenient, D-JSON3) carried over.
- Clean break done: `core.json` + `jet.{csv,toml,yaml,json}` retired; all examples/tests
  migrated; example `54_encoding.jet` (+ golden) covers the library form. Suite green
  (pre-existing `tests/arena.rs` failures are unrelated — confirmed by stashing this work).

### Shipped — increment 2 (2026-06-24): the typed-derive heart ✅
All blocking ballots ratified (batch 4) and built end-to-end:
- **§1 model:** `DataTree` value tree + `Encode`/`Decode` traits (`user_Encode`/`user_Decode`,
  methods `jet_encode`/`jet_decode`) + `DecodeError` + blanket impls for primitives, `[T]`,
  `T?`, `Map<String,V>` (CoreLib.rs). `DataTree`↔`Json` converters; ordered-Object JSON renderer
  (field order preserved, Int vs Float kept). Encode is infallible (no `EncodeError`, per D-SERDE2).
- **§4 derive:** `#[Codable]`/`#[Encode]`/`#[Decode]` bracket markers (D-ATTR2 form — first real
  consumer; parser builds the `#[…]` marker list + classifies derive-traits vs serde attrs).
  Compiler-owned `impl user_Encode`/`impl user_Decode` field-walk for structs AND enums
  (externally tagged default; `#[Tag("k")]` internal + `#[Untagged]` shipped per owner). Typed
  `decode<T>` via **call-site turbofish** (`json.decode<Order>(s)` — Jet's first `<T>` on a call,
  blessed generally) + expected-type-from-Result. Routing by lowered arg type / resolved return
  type keeps the dynamic `Json`/`[[String]]`/`Map` forms working alongside the typed forms.
- **§5 attrs:** `#[Rename]`, `#[Skip]`, `#[Default]`/`#[Default(literal)]`, `#[Flatten]`,
  `#[RenameAll(camel|snake|pascal|kebab|screaming)]`, `#[DenyUnknownFields]`. Absent optionals
  omitted on encode (owner-Q).
- **Formats:** json (encode/typed-decode), csv (`decode<T>` header-mapped + typed `to_string`),
  toml/yaml (flat typed decode/encode) — all routed through one `DataTree`.
- **Diagnostics:** E2407 (rename non-string), E2408 (flatten non-struct), E2409 (bad rename_all),
  E2410 (missing required field, runtime), E2411 (non-serializable — also keeps generated `impl`s
  rustc-clean, I2, and fires at the use site for a non-codable generic argument), E2412 (unknown
  field, runtime). E2413 retired (D-SERDE12). Sema validation pass in Bundle.rs. ui snapshots for
  the compile-time codes; docs/spec/diagnostics.md rows added.
- **Examples (I5):** `106_serde_derive`, `107_csv_typed`, `108_json_typed`, `120_serde_generic`
  (+ expected outputs); golden-tested (rustc verifies every generated `impl`). Suite green
  (pre-existing arena.rs parallel-harness flake aside).

**Generic-type serde (c136, D-SERDE9-12, done):** `#[Codable]`/`#[Encode]`/`#[Decode]` is
first-class on generic structs and enums. The derive auto-injects `T: user_Encode`/`T: user_Decode`
bounds on exactly the wire-reaching type params (those a non-`#[Skip]` field type mentions),
reusing the `Generics::rust_extra_*_bounds` precedent; a phantom/skip-only param gets no serde
bound (only structural `Clone`). The `TraitRegistry` records each Codable type's wire-param indices
so a non-codable type argument fails at the **use** site (E2411), not the definition (keeps emitted
`impl`s rustc-clean, I2). E2413 and its `type_params > 0` early-out / codegen bail are gone.

**Not yet built (future increments):** the manual bound override `#[Bound(…)]` (D-SERDE11 reserved
it; tracked as board card c147); hand-impl path (D-SERDE2 surface — `impl T: Encode { fn encode … }`,
the `DataTree` fluent accessor D-SERDE-ACCESS); `#[Default(expr)]` beyond literals; variant-level
`#[RenameAll]`; `#[Flatten]` map catch-all.

### (Historical) Remaining (next increment — the typed-derive heart) — was BLOCKED on owner ratification
The internal layer (`DataValue` tree + `Serialize`/`Deserialize` traits + `EncodingError` +
blanket impls in CoreLib, §1 rest) uses **codegen-internal names only** and needs no
ratification — buildable now. But every *user-facing* surface the derive path exposes is
unratified spelling and must be ratified before implementation (owner directive 2026-06-24:
"I want to see the EXACT shape of all of this before you implement it"):

| Surface | Ballot card | Recommendation |
|---|---|---|
| derive marker (one vs two) | **D-SERDE4** | single `#[Codable]` umbrella (sugar for `#[Serialize, Deserialize]`) |
| field attrs spelling/args | **D-SERDE5** | PascalCase bracket markers `#[Rename("x")]`/`#[Skip]`/`#[Default]`/`#[Flatten]` |
| `RenameAll` casing menu | **D-SERDE3** | typed 5-keyword menu `camel/snake/pascal/kebab/screaming` |
| typed `decode<T>` + verb coherence | **D-SERDE6** | both turbofish + expected-type; first call-site `<T>` in Jet |
| enum wire representation | **D-SERDE7** | externally tagged default; reserve `#[Tag(...)]` |
| unknown-field policy | **D-SERDE8** | lenient default + `#[DenyUnknownFields]` opt-in |
| hand-impl names/tree/error | **D-SERDE2** | *(expert path — NOT a blocker; derive generates the impls)* |

Casing across all field/container attrs is settled by **D-CASING1** (PascalCase marker names,
own-case arguments), not an open question.

Once ratified, build order:
- **§1 (rest):** `DataValue` tree + traits + `EncodingError` + blanket impls (internal; can start now).
- **§4:** built-in derive → compiler-owned `to_data`/`from_data` field-walk (structs + enums);
  generic `decode<T>` / expected-typed decode — call-site type-argument machinery (D-SERDE6).
- **§5:** field attributes (parse + sema + codegen) per D-SERDE5/3/8.
- **Diagnostics:** E2407–E2411 + docs + ui snapshots.
- **Examples:** `serde_derive`, `csv_typed`, `json_typed` (+ `serde_basic` only if D-SERDE2 lands).

---

# COMPLETION — full TOML & YAML adapters (the "serde-complete" gap)

**Status (2026-06-25): OPEN. This is the remaining work to make `core.encoding`
100% serde-equivalent.** Tracked as board card **c152** (card spec at the end of
this section). One owner decision gates the dynamic surface (**D-ENC-DYN1**);
everything else is buildable once that's picked. YAML advanced-feature scope is
**D-ENC-YAML1**.

## Where the library actually stands

| Format | dynamic `parse` | typed `decode<T>` | `to_string` encode | Verdict |
|---|---|---|---|---|
| **json** | `JSON` enum, **full RFC 8259** (D-PARSE-1) | nested, arrays, typed — full | full | ✅ complete |
| **csv** | `[[String]]` rows | header-mapped typed | full | ✅ complete (rows model is correct for CSV) |
| **toml** | `Map<String,String>` — **flat subset**, ignores `[table]` headers | **flat only** (values arrive as `Text`) | **flat only** | ❌ lossy |
| **yaml** | `Map<String,String>` — **flat subset** | **flat only** | **flat only** | ❌ lossy |

The derive engine and the `DataTree` decode path are **already full** — JSON
proves nested structs, arrays, and typed scalars all decode correctly. The TOML
and YAML *adapters* are the only thing feeding a flat, all-`Text` tree. So
completion = make those two adapters produce/consume a **rich `DataTree`**, with
no change to the derive side.

Current adapter code (the lossy part): `Source/Prelude/CoreLib.rs` —
`jet_ring_toml_parse` / `jet_ring_toml_render` / `jet_ring_yaml_parse` /
`jet_ring_yaml_render` (each is a `lines()` + `split('=')`/`split(':')` flat
loop). Sema types: `Source/Sema/CheckerCoreLib.rs` (`core.encoding.{toml,yaml}`
arms return `Map<String,String>`).

## What "full" means per format

**TOML** — a real parser already exists internally: `Source/Jetpack/TOML.rs`
(written for D-PARSE-1 / `jetpack.toml`) parses the complete TOML 1.0 grammar
into a typed `Value` tree. **Reuse it.** The jetpack copy is compiler-internal
Rust; the encoding adapter lives in the *emitted prelude*, so the parser must be
available to generated programs. Two implementation options (no owner call):
  - factor the parser into a small text module included by BOTH the compiler and
    the prelude, or
  - port `Source/Jetpack/TOML.rs` into `CoreLib.rs` as `jet_ring_toml_*` (it is
    std-only and already avoids the bare word "unsafe" — golden-safe).
Then map `TOML::Value` → `DataTree` (`Integer→Int`, `Float→Float`,
`Boolean→Bool`, `String→Text`, `Datetime→Text`, `Array→Array`,
`InlineTable`/header tables→`Object`). Encode = `DataTree` → TOML text (emit
`[table]` headers for nested objects, `key = typed-value`, arrays).

**YAML** — no engine exists; write a full one (std-only, I6). Scope to the YAML
1.2 core that serde_yaml covers:
  - block mappings + sequences (indentation-driven), flow `{}`/`[]`,
  - scalars typed by the YAML core schema (`null/~`, `true/false`, int, float,
    str), single/double-quoted + plain + block scalars (`|`, `>`),
  - comments, `---` document markers.
  - **D-ENC-YAML1 = A (RATIFIED 2026-06-25):** support anchors/aliases (`&a`/`*a`)
    + `---` document markers; **defer explicit/custom tags (`!!str`, `!MyType`)**
    to a separate frozen card (**c153**, full YAML 1.2). So c152's YAML scope is:
    block+flow maps/sequences, typed core scalars, block scalars (`|`/`>`),
    comments, documents, anchors/aliases — NOT explicit tags. On encode, anchors
    are always expanded (lossless). YAML is the biggest single piece here.

## The dynamic surface — RATIFIED (no longer a gate)

**D-ENC-DYN1 = A+ (RATIFIED 2026-06-25): one underlying `Data` value, per-format
type aliases.** Every format's untyped `parse` returns ONE shared rich dynamic
value, `Data` — the user-facing face of the internal `DataTree`
(`Data.Object/.Array/.Int/.Float/.Text/.Bool/.Null`) — replacing the flat
`Map<String,String>`. For discoverability, `Json`/`Toml`/`Yaml`/`Csv` are **type
aliases** over `Data` (`Json = Data`, …): `json.parse` is typed `Json`,
`toml.parse` is typed `Toml`, etc., but they're the same structure, so there is
one walker and one set of accessors to maintain (owner: "minimal code
maintenance, but discoverability/usage is clear for beginners").

**Migration (part of c152):** the shipped `JSON` enum collapses into `Data` (with
`Json` as its alias) — a clean break, no parallel path. Examples 30/73/108 + the
jsonfmt showcase migrate: `JSON.Text`→`Data.Text`/`Json.Text`, and integral
numbers split (`Number`→`.Int`/`.Float`). `csv.parse` yields a shallow
`Data.Array` of records (array-of-arrays, or array-of-objects when header-mapped).

## Build order (decisions ratified — fully buildable, no gates)

1. **`Data` value + aliases:** define the user-facing `Data` value (face of
   `DataTree`) and the `Json`/`Toml`/`Yaml`/`Csv` type aliases; migrate
   `json.parse` from the `JSON` enum to `Data` (re-bless JSON examples).
2. **TOML adapter full** (reuse `Source/Jetpack/TOML.rs`): parser→`Data`,
   `Data`→TOML renderer, wire `toml.decode<T>` to the rich tree, update the
   `core.encoding.toml` sema return type to `Toml` (= `Data`).
3. **YAML parser** (the big piece): full block+flow parser→`Data`, renderer,
   `yaml.decode<T>`, sema return type `Yaml`. Scope per D-ENC-YAML1 (core +
   anchors/aliases + documents; tags → frozen c153).
4. **Migrate examples** 52_toml / 53_yaml / 54_encoding to exercise nested
   tables, arrays, typed values (today they show only flat key/value); re-bless
   goldens. Add `toml_typed` / `yaml_typed` mirroring `108_json_typed` (decode
   into a nested `#[Codable]` struct).
5. **Diagnostics:** TOML/YAML parse errors get line+message (reuse the
   `ParseError` shape from `Source/Jetpack/TOML.rs`); a malformed-document code in
   the E27xx encoding range, doc'd + ui-snapshotted.
6. **Verify:** full `cargo test`; round-trip property (parse∘render≈identity) for
   nested fixtures; confirm generated `impl`s stay rustc-clean (I2).

## Done criteria

- [ ] `toml.parse` / `yaml.parse` return the D-ENC-DYN1 dynamic value (rich, not flat).
- [ ] `toml.decode<T>` / `yaml.decode<T>` decode nested `#[Codable]` structs with
      arrays and typed scalar fields (parity with `json.decode<T>`).
- [ ] `toml.to_string` / `yaml.to_string` emit valid nested documents.
- [ ] Examples 52/53/54 + `*_typed` exercise nesting; goldens re-blessed.
- [ ] Full suite green; I6 (no external crates) and I2 (rustc never speaks) intact.

## Card spec (for the owner to add to board.json — agents can't write it)

```json
{
  "id": "c152",
  "type": "task",
  "title": "Complete core.encoding: full TOML & YAML adapters (serde parity)",
  "body": "TOML and YAML are the last lossy adapters in core.encoding — flat Map<String,String> subsets that ignore [table] headers / nesting, while json+csv are full. Make them serde-complete: rich DataTree parse/decode<T>/encode. Reuse Source/Jetpack/TOML.rs for TOML; write a full std-only YAML parser. Gated on D-ENC-DYN1 (dynamic parse return type); D-ENC-YAML1 scopes YAML advanced features. Plan: sidequests/serde-model.md (COMPLETION section).",
  "stage": "deciding",
  "plan": "serde-model",
  "decisions": ["D-ENC-DYN1", "D-ENC-YAML1"],
  "priority": "P2",
  "workOrder": null
}
```
