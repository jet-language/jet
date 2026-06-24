# c104 — Serde-grade serialization: unified Serialize + Deserialize
**Decision:** D-SERDE1 (ratified 2026-06-22, option A)
**GATE: S56 user-defined derives (Epoch 3).** Do not implement the `#[Serialize, Deserialize]`
derive path until S56 lands. Land the data model and format adapters first; wire the derive
surface when S56 is shipped.

---

## What is decided (D-SERDE1 option A)

- A type derives `Serialize`/`Deserialize` once against a format-agnostic abstract data model.
- Each format (JSON, CSV, TOML, binary) is a ring library implementing `Serializer`/
  `Deserializer` protocol adapters. One derive drives every present and future format.
- `Deserialize` is the symmetric counterpart to S55's existing `Serialize`.
- Field attributes: `#[rename("x")]`, `#[default]`, `#[skip]`, `#[flatten]`, `#[rename_all("camelCase")]`.
- CSV (D-CSVROW1) and JSON (D-JSONOUT1) are arms of this model, not parallel paths.
- Data model lives in Core; format adapters are ring libraries.

---

## Current state

`Source/Publish/SBOM.rs` and `Source/Sema/SchemaMigration.rs` exist. S55's `#[Serialize]`
is in `Source/Syntax.rs`. No abstract data model or `Deserializer` protocol exists yet.
Existing JSON output (`jet.json`) is ad hoc. The `#[Serialize, Deserialize]` derive surface
requires S56 user-defined derives, which is an Epoch 3 deliverable.

---

## Plan (build-ready for when S56 lands; pre-work is unblocked now)

### Phase 1 — Abstract data model (unblocked now, no S56 needed)

New file `Source/Core/Serde.rs` (ring-level, not compiler internals; lives alongside other
Core ring implementations). Defines:

```rust
/// The format-agnostic value tree that Serializer/Deserializer speak.
pub enum DataValue {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    Seq(Vec<DataValue>),
    Map(Vec<(String, DataValue)>),  // ordered; keys are always strings at this layer
}

/// A format adapter implements this to receive a DataValue tree.
pub trait Serializer {
    type Output;
    type Error;
    fn serialize(&self, v: &DataValue) -> Result<Self::Output, Self::Error>;
}

/// A format adapter implements this to produce a DataValue tree from raw bytes.
pub trait Deserializer {
    type Input;
    type Error;
    fn deserialize(&self, input: Self::Input) -> Result<DataValue, Self::Error>;
}
```

This is pure Rust, zero external crates (I6). `DataValue` is the wire between derives and
format adapters.

### Phase 2 — `Serialize` trait in Core (unblocked now)

`Core/Serialize.rs` (amends S55):

```rust
pub trait Serialize {
    fn to_data(&self) -> DataValue;
}

pub trait Deserialize: Sized {
    fn from_data(v: &DataValue) -> Result<Self, SerdeError>;
}
```

Blanket impls for all primitive types (Bool, Int, Float, String, Char, `[T]`, `Map<K,V>`,
`T?`). These do NOT require S56 — they are hand-written trait impls, not derived.

**Diagnostic:** `SerdeError` carries a field path (`"user.name"`) and a reason string.

### Phase 3 — Format adapters (ring libraries, unblocked now)

Each adapter is a separate ring library (no new language features needed — they implement
`Serializer`/`Deserializer` in Rust behind the ring boundary):

- `jet.json` (D-JSONOUT1): `JsonSerializer` → `String`; `JsonDeserializer` → `DataValue`.
  Replaces the ad hoc JSON emission. Field attr `#[rename("x")]` → key rename in the
  `DataValue::Map` layer.
- `jet.csv` (D-CSVROW1): `CsvSerializer` → row string; `CsvDeserializer` → `DataValue::Map`
  with header-keyed columns.
- `jet.toml`: `TomlSerializer` → TOML string; `TomlDeserializer` → `DataValue`.
- `jet.binary`: length-prefixed binary encoding; deterministic for hashing/caching.

Each adapter is tested independently against the data model. No adapter calls another.

### Phase 4 — Field attribute sema (GATED on S56)

When S56 user-defined derives land, add sema passes for:

- `#[rename("x")]` on a struct field: substitute the key in `DataValue::Map`.
- `#[default]` on a field: use `T::default()` when the key is absent during deserialization.
- `#[skip]` on a field: omit from serialization; require `#[default]` or `T?` for
  deserialization.
- `#[flatten]` on a struct-typed field: inline its `DataValue::Map` entries into the parent.
- `#[rename_all("camelCase"|"snake_case"|"PascalCase")]` on the struct: applies the casing
  transform to every key.

Each attribute is a sema check over the struct's fields before codegen. Sema fires:
- **E2401** — `#[rename]` given a non-string literal.
- **E2402** — `#[flatten]` on a non-struct field.
- **E2403** — `#[rename_all]` given an unrecognized casing style.

### Phase 5 — `#[Serialize, Deserialize]` derive codegen (GATED on S56)

When S56 lands:

In `Source/Sema/CheckerItems.rs` and `Source/Codegen/TIR.rs`, handle `#[Serialize,
Deserialize]` on a struct/enum:

- `to_data()`: walk fields in order; apply field attributes; build `DataValue::Map` or
  `DataValue::Seq`.
- `from_data()`: reverse walk; apply `#[default]`/`#[skip]`; error on missing required fields
  with E2404 naming the field and its type.

Codegen emits a Rust `impl Serialize for user_<TypeName>` and `impl Deserialize for
user_<TypeName>` block — plain Rust, no proc macros (I6), no `unsafe` (I1).

**Diagnostic (I4)**

| Code | Meaning |
|------|---------|
| E2401 | `#[rename]` expects a string literal |
| E2402 | `#[flatten]` requires a struct-typed field |
| E2403 | `#[rename_all]` unrecognized casing style |
| E2404 | deserialization missing required field `x: T` |

All four need `docs/spec/diagnostics.md` entries and `tests/ui/` snapshots.

---

## Examples (I5)

`examples/features/serde_basic.jet` (Phase 1–3, no derive):
```jet
use jet.json

struct Point { x: Int, y: Int }

impl Serialize for Point {
    fn to_data(self) -> DataValue {
        DataValue.Map([("x", DataValue.Int(self.x)), ("y", DataValue.Int(self.y))])
    }
}

fn main() {
    p := Point { x: 1, y: 2 }
    print(json.to_string(p))
}
```
Expected: `{"x":1,"y":2}`

`examples/features/serde_derive.jet` (Phase 5, GATED on S56):
```jet
use jet.json

#[Serialize, Deserialize]
struct User {
    name: String,
    #[rename("emailAddress")]
    email: String,
}

fn main() {
    u := json.decode<User>("{\"name\":\"Alice\",\"emailAddress\":\"a@b.com\"}")
    print(u.name)
}
```
Expected: `Alice`

---

## Files touched

| File | Change |
|------|--------|
| `Source/Core/Serde.rs` (new) | `DataValue`, `Serializer`, `Deserializer` traits |
| `Source/Core/Serialize.rs` (amend S55) | `Serialize` + `Deserialize` traits; blanket impls |
| `Source/ring/json/` (amend) | `JsonSerializer`/`JsonDeserializer` against data model |
| `Source/ring/csv/` (amend) | `CsvSerializer`/`CsvDeserializer` |
| `Source/ring/toml/` (new or amend) | TOML adapter |
| `Source/ring/binary/` (new) | binary adapter |
| `Source/Sema/CheckerItems.rs` | field attr sema (S56-gated) |
| `Source/Codegen/TIR.rs` | derive codegen (S56-gated) |
| `docs/spec/diagnostics.md` | E2401–E2404 entries |
| `tests/ui/` | e2401–e2404 snapshots |
| `examples/features/serde_basic.jet` + expected | (I5) |
| `examples/features/serde_derive.jet` + expected | (I5, S56-gated) |

---

## Decision verdict

No decision needed — D-SERDE1 is ratified. **GATE: S56 user-defined derives (Epoch 3)**
for Phases 4–5. Phases 1–3 (data model + format adapters) are unblocked and should be built
now.
