use jet_foundation::Shape::{ShapeProjection, ShapeProjectionKind};

// ── core.encoding: Encode / Decode traits + blanket impls (D-SERDE1/2/4) ──────
// The built-in `#[Codable]`/`#[Encode]`/`#[Decode]` derive (D-ENC1) lowers to
// these traits. `jet_encode`/`jet_decode` are codegen-internal method names the
// user never types (they write the verbs `encode`/`decode` only in a hand-impl,
// D-SERDE2 — a later increment). Pure safe std Rust, no proc-macros (I1/I6).
#[allow(non_camel_case_types)]
pub trait __jet_Encode {
    /// True only for the byte scalar that carries native CBOR byte strings.
    const __JET_IS_BYTE: bool = false;

    fn jet_encode(&self) -> jet_std::DataTree;

    /// The byte projection is selected by the scalar fact above, never by a
    /// runtime type-name comparison.
    fn jet_encode_byte(&self) -> u8 {
        unreachable!()
    }
}
#[allow(non_camel_case_types)]
pub trait __jet_Decode: Sized {
    /// True only for the byte scalar accepted from native CBOR byte strings.
    const __JET_IS_BYTE: bool = false;

    fn jet_decode(tree: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>>;

    fn jet_decode_byte(value: u8) -> Result<Self, Vec<jet_std::FieldError>> {
        Self::jet_decode(&jet_std::DataTree::Int(i64::from(value)))
    }
}

impl __jet_Encode for i64 {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::jet_datatree_encode_i64(*self)
    }
}

// Exact `Int` owns its one-word carrier.  Values that fit the physical
// DataTree::Int slot stay there; spilled values retain their decimal lexeme in
// DataTree::Number instead of exposing the tagged owner word as a number.
impl __jet_Encode for jet_foundation::Numeric::JetInt {
    fn jet_encode(&self) -> jet_std::DataTree {
        self.to_i64()
            .map(jet_std::jet_datatree_encode_i64)
            .unwrap_or_else(|| jet_std::DataTree::Number(self.to_string_rep()))
    }
}

impl __jet_Encode for f64 {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Float(*self)
    }
}
impl __jet_Encode for bool {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Bool(*self)
    }
}
impl __jet_Encode for String {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Text(self.clone())
    }
}
impl __jet_Encode for char {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Text(self.to_string())
    }
}
impl __jet_Encode for JetDate {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Text(jet_codec_date_encode(self.year(), self.month(), self.day()))
    }
}
impl __jet_Encode for JetLocalTime {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Text(jet_codec_local_time_encode(
            self.hour(),
            self.minute(),
            self.second(),
        ))
    }
}
impl __jet_Encode for JetDateTime {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Text(jet_codec_datetime_encode(
            self.unix_seconds_anchor(),
            self.nanosecond() as u32,
            self.is_leap_second(),
        ))
    }
}
impl __jet_Encode for jet_std::Duration {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Int(jet_codec_duration_encode(self.ns))
    }
}
impl __jet_Encode for u8 {
    const __JET_IS_BYTE: bool = true;

    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Int(i64::from(*self))
    }

    fn jet_encode_byte(&self) -> u8 {
        *self
    }
}
impl __jet_Encode for u64 {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::jet_datatree_encode_u64(*self)
    }
}
macro_rules! jet_impl_sized_int_encode {
    ($($ty:ty),* $(,)?) => {$(
        impl __jet_Encode for $ty {
            fn jet_encode(&self) -> jet_std::DataTree {
                jet_std::DataTree::Int(*self as i64)
            }
        }
    )*};
}
jet_impl_sized_int_encode!(i8, i16, i32, u16, u32);
impl __jet_Encode for f32 {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Float(*self as f64)
    }
}
impl __jet_Encode for jet_std::JetDecimal {
    fn jet_encode(&self) -> jet_std::DataTree {
        // Decimal stays exact through the shared tree; text preserves scale.
        jet_std::DataTree::Text(jet_codec_decimal_encode(self))
    }
}
impl<T: __jet_Encode> __jet_Encode for Vec<T> {
    fn jet_encode(&self) -> jet_std::DataTree {
        // D-ENC-CBOR-SURFACE1: `[U8]` carries binary identity through the shared
        // Codable tree. Text codecs already render Bytes as a number list, while
        // CBOR emits major type 2. The byte fact is carried by the scalar
        // implementation rather than inferred from a Rust type name.
        if <T as __jet_Encode>::__JET_IS_BYTE {
            let mut bytes = Vec::with_capacity(self.len());
            for value in self {
                bytes.push(value.jet_encode_byte());
            }
            return jet_std::DataTree::Bytes(bytes);
        }
        jet_std::DataTree::Array(self.iter().map(|x| x.jet_encode()).collect())
    }
}
impl<T: __jet_Encode, const N: usize> __jet_Encode for [T; N] {
    fn jet_encode(&self) -> jet_std::DataTree {
        if <T as __jet_Encode>::__JET_IS_BYTE {
            let mut bytes = Vec::with_capacity(N);
            for value in self {
                bytes.push(value.jet_encode_byte());
            }
            return jet_std::DataTree::Bytes(bytes);
        }
        jet_std::DataTree::Array(self.iter().map(|value| value.jet_encode()).collect())
    }
}
// D-FAIL-CARRIER1=A: `?T` is the carrier, so the carrier is what codes.
impl<T: __jet_Encode> __jet_Encode for JetOutcome<T, JetAbsent> {
    fn jet_encode(&self) -> jet_std::DataTree {
        match self {
            Ok(x) => x.jet_encode(),
            Err(JetAbsent) => jet_std::DataTree::Null,
        }
    }
}
impl<V: __jet_Encode> __jet_Encode for std::collections::BTreeMap<String, V> {
    fn jet_encode(&self) -> jet_std::DataTree {
        jet_std::DataTree::Object(
            self.iter()
                .map(|(k, v)| (k.clone(), v.jet_encode()))
                .collect(),
        )
    }
}
impl<V: __jet_Encode> __jet_Encode for JetMap<String, V> {
    fn jet_encode(&self) -> jet_std::DataTree {
        // Keep the JetMap surface as a transparent BTreeMap codec. Calling
        // `self.jet_encode()` (or relying on method lookup through Deref)
        // re-enters this impl for JetMap and recurses forever.
        <std::collections::BTreeMap<String, V> as __jet_Encode>::jet_encode(&**self)
    }
}
// D-ENC-CBOR-SURFACE1: DataTree itself is Codable. Whole-value codec
// composition must not fall through to rustc after front-end acceptance.
impl __jet_Encode for jet_std::DataTree {
    fn jet_encode(&self) -> jet_std::DataTree { self.clone() }
}

impl __jet_Decode for i64 {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::jet_datatree_decode_fixed_integer(t, true, 64, "I64")?
            .try_into()
            .map_err(|_| jet_std::FieldError::one("expected I64, found out-of-range Int"))
    }
}

impl __jet_Decode for jet_foundation::Numeric::JetInt {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::decode_int_with(
            t,
            jet_foundation::Numeric::JetInt::from_i64,
            |text| {
                jet_foundation::Numeric::CtBigInt::from_str(text)
                    .map(jet_foundation::Numeric::JetInt::from_big)
                    .map_err(|error| error.to_string())
            },
        )
    }
}


// D-TYPE2-SPELL1: inline ranges are transparent in the Rust carrier, so a
// generic container decoder cannot see the interval through `T = i64`. Typed
// DataTree decode emits these Prelude-owned walks whenever a range appears
// below a list, option, fixed list, or string-keyed map.
fn jet_decode_inline_range(
    t: &jet_std::DataTree,
    lo: i64,
    hi: i64,
) -> Result<i64, Vec<jet_std::FieldError>> {
    let value = <i64 as __jet_Decode>::jet_decode(t)?;
    jet_inline_range_from_int(value, lo, hi).map_err(jet_std::FieldError::one)
}

fn jet_decode_sequence<T, F>(
    length: usize,
    mut decode: F,
) -> Result<Vec<T>, Vec<jet_std::FieldError>>
where
    F: FnMut(usize) -> Result<T, Vec<jet_std::FieldError>>,
{
    let mut out = Vec::with_capacity(length);
    let mut errors = Vec::new();
    for index in 0..length {
        match decode(index) {
            Ok(value) => out.push(value),
            Err(error) => errors.extend(jet_std::FieldError::under_errors(
                &format!("[{index}]"),
                error,
            )),
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

fn jet_decode_inline_range_list<T, F>(
    t: &jet_std::DataTree,
    mut decode: F,
) -> Result<Vec<T>, Vec<jet_std::FieldError>>
where
    F: FnMut(&jet_std::DataTree) -> Result<T, Vec<jet_std::FieldError>>,
{
    let jet_std::DataTree::Array(items) = t else {
        return Err(jet_std::FieldError::one(format!(
            "expected a list, found {}",
            jet_std::datatree_kind_for(t)
        )));
    };
    jet_decode_sequence(items.len(), |index| decode(&items[index]))
}

fn jet_decode_inline_range_fixed<T, F, const N: usize>(
    t: &jet_std::DataTree,
    decode: F,
) -> Result<[T; N], Vec<jet_std::FieldError>>
where
    F: FnMut(&jet_std::DataTree) -> Result<T, Vec<jet_std::FieldError>>,
{
    let values = jet_decode_inline_range_list(t, decode)?;
    let found = values.len();
    values.try_into().map_err(|_| {
        jet_std::FieldError::one(format!(
            "expected a fixed list of length {N}, found {found}"
        ))
    })
}

fn jet_decode_inline_range_option<T, F>(
    t: &jet_std::DataTree,
    mut decode: F,
) -> Result<JetOutcome<T, JetAbsent>, Vec<jet_std::FieldError>>
where
    F: FnMut(&jet_std::DataTree) -> Result<T, Vec<jet_std::FieldError>>,
{
    match t {
        jet_std::DataTree::Null => Ok(Err(JetAbsent)),
        other => Ok(Ok(decode(other)?)),
    }
}

fn jet_decode_inline_range_map<V, F>(
    t: &jet_std::DataTree,
    mut decode: F,
) -> Result<JetMap<String, V>, Vec<jet_std::FieldError>>
where
    F: FnMut(&jet_std::DataTree) -> Result<V, Vec<jet_std::FieldError>>,
{
    let jet_std::DataTree::Object(entries) = t else {
        return Err(jet_std::FieldError::one(format!(
            "expected an object, found {}",
            jet_std::datatree_kind_for(t)
        )));
    };
    let mut out = Vec::with_capacity(entries.len());
    let mut errors = Vec::new();
    for (key, value) in entries {
        match decode(value) {
            Ok(value) => out.push((key.clone(), value)),
            Err(error) => errors.extend(jet_std::FieldError::under_errors(key, error)),
        }
    }
    if errors.is_empty() {
        Ok(out.into_iter().collect())
    } else {
        Err(errors)
    }
}

impl __jet_Decode for f64 {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::decode_float(t)
    }
}
impl __jet_Decode for bool {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::decode_bool(t)
    }
}
impl __jet_Decode for String {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::decode_string(t)
    }
}
impl __jet_Decode for char {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        let s = String::jet_decode(t)?;
        let mut it = s.chars();
        match (it.next(), it.next()) {
            (Some(c), None) => Ok(c),
            _ => Err(jet_std::FieldError::one(format!(
                "expected a single Char, found {:?}",
                s
            ))),
        }
    }
}
impl __jet_Decode for JetDate {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        let value = String::jet_decode(t)?;
        jet_codec_date_decode(&value)
            .map(|(year, month, day)| JetDate::new(year, month, day))
            .map_err(|error| jet_std::FieldError::one(format!("expected Date: {error}")))
    }
}
impl __jet_Decode for JetLocalTime {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        let value = String::jet_decode(t)?;
        jet_codec_local_time_decode(&value)
            .map(|(hour, minute, second)| JetLocalTime::new(hour, minute, second))
            .map_err(|error| jet_std::FieldError::one(format!("expected LocalTime: {error}")))
    }
}
impl __jet_Decode for JetDateTime {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        let value = String::jet_decode(t)?;
        jet_codec_datetime_decode(&value)
            .map(|(secs, nanos, leap_second)| {
                JetDateTime::from_timestamp_ns_with_leap(secs, nanos, leap_second)
            })
            .map_err(|error| jet_std::FieldError::one(format!("expected DateTime: {error}")))
    }
}
impl __jet_Decode for jet_std::Duration {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        let ns = jet_std::decode_int_with(
            t,
            |value| value,
            |text| {
                text.parse::<i64>()
                    .map_err(|_| "exact Int is outside i64".to_owned())
            },
        )?;
        Ok(jet_std::Duration {
            ns: jet_codec_duration_decode(ns),
        })
    }
}
impl __jet_Decode for u8 {
    const __JET_IS_BYTE: bool = true;

    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::jet_datatree_decode_fixed_integer(t, false, 8, "U8")?
            .try_into()
            .map_err(|_| jet_std::FieldError::one("expected U8, found out-of-range Int"))
    }

    fn jet_decode_byte(value: u8) -> Result<Self, Vec<jet_std::FieldError>> {
        Ok(value)
    }
}
macro_rules! jet_impl_sized_int_decode {
    ($($ty:ty => ($signed:literal, $bits:literal, $name:literal)),* $(,)?) => {$(
        impl __jet_Decode for $ty {
            fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
                jet_std::jet_datatree_decode_fixed_integer(t, $signed, $bits, $name)?
                    .try_into()
                    .map_err(|_| {
                        jet_std::FieldError::one(format!(
                            "expected {}, found out-of-range Int",
                            $name
                        ))
                    })
            }
        }
    )*};
}
jet_impl_sized_int_decode!(
    i8 => (true, 8, "I8"),
    i16 => (true, 16, "I16"),
    i32 => (true, 32, "I32"),
    u16 => (false, 16, "U16"),
    u32 => (false, 32, "U32"),
    u64 => (false, 64, "U64"),
);
impl __jet_Decode for f32 {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::decode_f32(t)
    }
}
impl __jet_Decode for jet_std::JetDecimal {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        match t {
            jet_std::DataTree::Number(text) => {
                jet_std::JetDecimal::from_json_number(text).map_err(|error| {
                    jet_std::FieldError::one(format!("expected Decimal: {error}"))
                })
            }
            jet_std::DataTree::TypedText(text) => Err(jet_std::FieldError::one(format!(
                "expected Decimal, found text {:?}",
                text
            ))),
            jet_std::DataTree::Text(text) => {
                jet_codec_decimal_decode_text(text).map_err(|error| {
                    jet_std::FieldError::one(format!("expected Decimal: {error}"))
                })
            }
            jet_std::DataTree::Int(n) => jet_codec_decimal_decode_int(*n)
                .map_err(jet_std::FieldError::one),
            other => Err(jet_std::FieldError::one(format!(
                "expected Decimal, found {}", jet_std::datatree_kind_for(other)
            ))),
        }
    }
}
impl<T: __jet_Decode> __jet_Decode for Vec<T> {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        match t {
            jet_std::DataTree::Bytes(bytes) if <T as __jet_Decode>::__JET_IS_BYTE => {
                jet_decode_sequence(bytes.len(), |index| T::jet_decode_byte(bytes[index]))
            }
            jet_std::DataTree::Array(items) => {
                jet_decode_sequence(items.len(), |index| T::jet_decode(&items[index]))
            }
            other => Err(jet_std::FieldError::one(format!(
                "expected a list, found {}",
                jet_std::datatree_kind_for(other)
            ))),
        }
    }
}
impl<T: __jet_Decode, const N: usize> __jet_Decode for [T; N] {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        let values = Vec::<T>::jet_decode(t)?;
        let found = values.len();
        values.try_into().map_err(|_| {
            jet_std::FieldError::one(format!(
                "expected a fixed list of length {}, found {}",
                N, found
            ))
        })
    }
}
impl __jet_Decode for jet_std::DataTree {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> { Ok(t.clone()) }
}
impl<T: __jet_Decode> __jet_Decode for JetOutcome<T, JetAbsent> {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        match t {
            jet_std::DataTree::Null => Ok(Err(JetAbsent)),
            other => Ok(Ok(T::jet_decode(other)?)),
        }
    }
}
impl<V: __jet_Decode> __jet_Decode for std::collections::BTreeMap<String, V> {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        match t {
            jet_std::DataTree::Object(entries) => {
                let mut out = std::collections::BTreeMap::new();
                let mut errors = Vec::new();
                for (k, v) in entries {
                    match V::jet_decode(v) {
                        Ok(value) => {
                            out.insert(k.clone(), value);
                        }
                        Err(error) => {
                            errors.extend(jet_std::FieldError::under_errors(k, error));
                        }
                    }
                }
                if !errors.is_empty() {
                    return Err(errors);
                }
                Ok(out)
            }
            other => Err(jet_std::FieldError::one(format!(
                "expected an object, found {}",
                jet_std::datatree_kind_for(other)
            ))),
        }
    }
}
impl<V: __jet_Decode> __jet_Decode for JetMap<String, V> {
    fn jet_decode(t: &jet_std::DataTree) -> Result<Self, Vec<jet_std::FieldError>> {
        <std::collections::BTreeMap<String, V> as __jet_Decode>::jet_decode(t)
            .map(|map| map.into_iter().collect())
    }
}

// ── core.encoding.datatree: checked DataTree projections ─────────────────────
// Keep the carrier access policy in `JetDataTreeAccess`; these thin ABI
// functions only expose that one implementation to generated MIR calls.
fn jet_datatree_field(
    tree: &jet_std::DataTree,
    name: &String,
) -> Result<jet_std::DataTree, Vec<jet_std::FieldError>> {
    <jet_std::DataTree as jet_std::JetDataTreeAccess>::field(tree, name)
}
fn jet_datatree_at(
    tree: &jet_std::DataTree,
    index: i64,
) -> Result<jet_std::DataTree, Vec<jet_std::FieldError>> {
    <jet_std::DataTree as jet_std::JetDataTreeAccess>::at(tree, index)
}
fn jet_datatree_int(
    tree: &jet_std::DataTree,
) -> Result<i64, Vec<jet_std::FieldError>> {
    <jet_std::DataTree as jet_std::JetDataTreeAccess>::int(tree)
}
fn jet_datatree_text(
    tree: &jet_std::DataTree,
) -> Result<String, Vec<jet_std::FieldError>> {
    <jet_std::DataTree as jet_std::JetDataTreeAccess>::text(tree)
}
fn jet_datatree_bool(
    tree: &jet_std::DataTree,
) -> Result<bool, Vec<jet_std::FieldError>> {
    <jet_std::DataTree as jet_std::JetDataTreeAccess>::bool(tree)
}
fn jet_datatree_float(
    tree: &jet_std::DataTree,
) -> Result<f64, Vec<jet_std::FieldError>> {
    <jet_std::DataTree as jet_std::JetDataTreeAccess>::float(tree)
}
fn jet_datatree_to_text(tree: &jet_std::DataTree) -> JetOutcome<String, JetAbsent> {
    jet_outcome_of(<jet_std::DataTree as jet_std::JetDataTreeAccess>::to_text(tree))
}
fn jet_datatree_equal_unordered(
    tree: &jet_std::DataTree,
    other: &jet_std::DataTree,
) -> bool {
    <jet_std::DataTree as jet_std::JetDataTreeAccess>::equal_unordered(tree, other)
}

// This route is intentionally generic: TIR carries the checked type argument,
// while the trait keeps all scalar/container decode policy in one place.
fn jet_codec_decode_typed<T: __jet_Decode>(
    tree: &jet_std::DataTree,
) -> Result<T, Vec<jet_std::FieldError>> {
    T::jet_decode(tree)
}

// ── core.encoding: typed format verbs over Encode/Decode (D-ENC1, D-SERDE6) ────
// `to_string`/`to_string_pretty` (D-JSONVERB1) and the typed `decode<T>` route
// every format through the one DataTree model.
// Internal MIR codec projections carry a checked kind ID so resident hosts can
// use the same ABI while AOT dispatches to these trait implementations.
fn jet_codec_encode<T: __jet_Encode>(_kind: i64, value: &T) -> jet_std::DataTree {
    value.jet_encode()
}
fn jet_codec_decode<T: __jet_Decode>(
    _kind: i64,
    tree: &jet_std::DataTree,
) -> Result<T, Vec<jet_std::FieldError>> {
    T::jet_decode(tree)
}
fn jet_enc_json_to_string<T: __jet_Encode>(v: &T) -> String {
    jet_std::render_datatree_json(&v.jet_encode(), false, 0)
}
fn jet_enc_json_to_string_pretty<T: __jet_Encode>(v: &T) -> String {
    jet_std::render_datatree_json(&v.jet_encode(), true, 0)
}

fn jet_enc_json_to_string_shape<T: __jet_Encode>(
    value: &T,
    projection: &ShapeProjection,
) -> Result<String, Vec<jet_std::FieldError>> {
    if projection.kind != ShapeProjectionKind::Json {
        return Err(jet_std::FieldError::one("JSON needs a JSON shape projection"));
    }
    let projected = jet_std::jet_datatree_project(&value.jet_encode(), projection)?;
    Ok(jet_std::render_datatree_json(&projected, false, 0))
}

fn jet_enc_json_to_string_shape_pretty<T: __jet_Encode>(
    value: &T,
    projection: &ShapeProjection,
) -> Result<String, Vec<jet_std::FieldError>> {
    if projection.kind != ShapeProjectionKind::Json {
        return Err(jet_std::FieldError::one("JSON needs a JSON shape projection"));
    }
    let projected = jet_std::jet_datatree_project(&value.jet_encode(), projection)?;
    Ok(jet_std::render_datatree_json(&projected, true, 0))
}
fn jet_enc_json_decode<T: __jet_Decode>(text: &String) -> Result<T, Vec<jet_std::FieldError>> {
    let tree = jet_std::parse_json_typed_datatree(text).map_err(|e| {
        let line = e.line.ok().unwrap_or(0);
        jet_std::FieldError::one(format!("invalid JSON (line {}): {}", line, e.reason))
    })?;
    // D-MIGRATE4: migration is part of the sole canonical decode operation.
    T::jet_decode(&tree)
}
fn jet_data_json_decode<T: __jet_Decode>(
    text: &String,
) -> Result<Vec<T>, Vec<jet_std::FieldError>> {
    jet_enc_json_decode::<Vec<T>>(text)
}
/// Decode an already-projected ordered object through the canonical typed
/// DataTree trait. DB supplies the entries after its carrier-only projection;
/// this function stays format-neutral and is shared by every host.
fn jet_db_decode<T: __jet_Decode>(
    entries: &[(String, jet_std::DataTree)],
) -> Result<T, Vec<jet_std::FieldError>> {
    T::jet_decode(&jet_std::DataTree::Object(entries.to_vec()))
}

// CSV typed decode: header row maps columns to fields by name; each data row
// becomes a DataTree::Object of Text cells, then decodes to `T`. A short row or a
// per-row decode failures are typed `[FieldError]` values naming the 1-based row.
fn jet_enc_csv_decode<T: __jet_Decode>(text: &String) -> Result<Vec<T>, Vec<jet_std::FieldError>> {
    let rows = jet_ring_csv_parse(text, &",".to_string(), false, false)
        .map_err(jet_std::FieldError::one)?;
    jet_std::jet_enc_csv_decode_rows(
        rows,
        |tree| T::jet_decode(&tree),
        |row, error| jet_std::FieldError::under_errors(row, error),
        jet_std::DataTree::Text,
        jet_std::DataTree::Object,
    )
}

trait JetDataCount {
    fn jet_data_count(&self) -> i64;
}

impl<T> JetDataCount for Vec<T> {
    fn jet_data_count(&self) -> i64 {
        self.len() as i64
    }
}


fn jet_data_count<T: JetDataCount + ?Sized>(rows: &T) -> i64 {
    rows.jet_data_count()
}

