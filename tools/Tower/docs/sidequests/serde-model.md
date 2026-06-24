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
  rustc-clean, I2), E2412 (unknown field, runtime), E2413 (generic serde gated). Sema validation
  pass in Bundle.rs. ui snapshots for the 5 compile-time codes; docs/spec/diagnostics.md rows added.
- **Examples (I5):** `106_serde_derive`, `107_csv_typed`, `108_json_typed` (+ expected outputs);
  golden-tested (rustc verifies every generated `impl`). Suite green (pre-existing arena.rs
  parallel-harness flake aside).

**Not yet built (future increments):** generic-type serde (E2413-gated for now); hand-impl path
(D-SERDE2 surface — `impl T: Encode { fn encode … }`, the `DataTree` fluent accessor D-SERDE-ACCESS);
`#[Default(expr)]` beyond literals; variant-level `#[RenameAll]`; `#[Flatten]` map catch-all.

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
