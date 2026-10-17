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

    fn jet_encode(&self) -> jet_std::Dyn;

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

    fn jet_decode(tree: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>>;

    fn jet_decode_byte(value: u8) -> Result<Self, Vec<jet_std::FieldError>> {
        Self::jet_decode(&jet_std::Dyn::Int((i64::from(value)).into()))
    }
}

impl __jet_Encode for i64 {
    fn jet_encode(&self) -> jet_std::Dyn {
        jet_std::jet_dyn_encode_i64(*self)
    }
}

// Encoding exact Int retains its owner and its Int variant at every magnitude.
impl __jet_Encode for jet_foundation::Numeric::JetInt {
    fn jet_encode(&self) -> jet_std::Dyn {
        jet_std::Dyn::Int(self.clone())
    }
}

impl __jet_Encode for f64 {
    fn jet_encode(&self) -> jet_std::Dyn {
        jet_std::Dyn::Float(*self)
    }
}
impl __jet_Encode for bool {
    fn jet_encode(&self) -> jet_std::Dyn {
        jet_std::Dyn::Bool(*self)
    }
}
impl __jet_Encode for String {
    fn jet_encode(&self) -> jet_std::Dyn {
        jet_std::Dyn::Text(self.clone())
    }
}
impl __jet_Encode for char {
    fn jet_encode(&self) -> jet_std::Dyn {
        jet_std::Dyn::Text(self.to_string())
    }
}
impl __jet_Encode for JetDate {
    fn jet_encode(&self) -> jet_std::Dyn {
        jet_std::Dyn::Text(jet_codec_date_encode(self.year(), self.month(), self.day()))
    }
}
impl __jet_Encode for JetLocalTime {
    fn jet_encode(&self) -> jet_std::Dyn {
        jet_std::Dyn::Text(jet_codec_local_time_encode(
            self.hour(),
            self.minute(),
            self.second(),
        ))
    }
}
impl __jet_Encode for JetDateTime {
    fn jet_encode(&self) -> jet_std::Dyn {
        jet_std::Dyn::datetime(self.unix_seconds_anchor(), self.nanosecond() as u32,
            self.is_leap_second(), 0)
    }
}
impl __jet_Encode for jet_std::Duration {
    fn jet_encode(&self) -> jet_std::Dyn {
        jet_std::Dyn::Int((jet_codec_duration_encode(self.ns)).into())
    }
}
impl __jet_Encode for u8 {
    const __JET_IS_BYTE: bool = true;

    fn jet_encode(&self) -> jet_std::Dyn {
        jet_std::Dyn::Int((i64::from(*self)).into())
    }

    fn jet_encode_byte(&self) -> u8 {
        *self
    }
}
impl __jet_Encode for u64 {
    fn jet_encode(&self) -> jet_std::Dyn {
        jet_std::jet_dyn_encode_u64(*self)
    }
}
macro_rules! jet_impl_sized_int_encode {
    ($($ty:ty),* $(,)?) => {$(
        impl __jet_Encode for $ty {
            fn jet_encode(&self) -> jet_std::Dyn {
                jet_std::Dyn::Int((*self as i64).into())
            }
        }
    )*};
}
jet_impl_sized_int_encode!(i8, i16, i32, u16, u32);
impl __jet_Encode for f32 {
    fn jet_encode(&self) -> jet_std::Dyn {
        jet_std::Dyn::Float(*self as f64)
    }
}
impl __jet_Encode for jet_std::JetDecimal {
    fn jet_encode(&self) -> jet_std::Dyn {
        // Decimal stays exact through the shared tree; text preserves scale.
        jet_std::Dyn::Text(jet_codec_decimal_encode(self))
    }
}
impl<T: __jet_Encode> __jet_Encode for Vec<T> {
    fn jet_encode(&self) -> jet_std::Dyn {
        // Byte lists retain binary identity; text codecs render standard base64.
        if <T as __jet_Encode>::__JET_IS_BYTE {
            let mut bytes = Vec::with_capacity(self.len());
            for value in self {
                bytes.push(value.jet_encode_byte());
            }
            return jet_std::Dyn::Bytes(bytes);
        }
        jet_std::Dyn::Array(self.iter().map(|x| x.jet_encode()).collect())
    }
}
impl<T: __jet_Encode, const N: usize> __jet_Encode for [T; N] {
    fn jet_encode(&self) -> jet_std::Dyn {
        if <T as __jet_Encode>::__JET_IS_BYTE {
            let mut bytes = Vec::with_capacity(N);
            for value in self {
                bytes.push(value.jet_encode_byte());
            }
            return jet_std::Dyn::Bytes(bytes);
        }
        jet_std::Dyn::Array(self.iter().map(|value| value.jet_encode()).collect())
    }
}
// D-FAIL-CARRIER1=A: `?T` is the carrier, so the carrier is what codes.
impl<T: __jet_Encode> __jet_Encode for JetOutcome<T, JetAbsent> {
    fn jet_encode(&self) -> jet_std::Dyn {
        match self {
            Ok(x) => x.jet_encode(),
            Err(JetAbsent) => jet_std::Dyn::Null,
        }
    }
}
impl<V: __jet_Encode> __jet_Encode for std::collections::BTreeMap<String, V> {
    fn jet_encode(&self) -> jet_std::Dyn {
        jet_std::Dyn::Object(
            self.iter()
                .map(|(k, v)| (k.clone(), v.jet_encode()))
                .collect(),
        )
    }
}
impl<V: __jet_Encode> __jet_Encode for JetMap<String, V> {
    fn jet_encode(&self) -> jet_std::Dyn {
        // Keep the JetMap surface as a transparent BTreeMap codec. Calling
        // `self.jet_encode()` (or relying on method lookup through Deref)
        // re-enters this impl for JetMap and recurses forever.
        <std::collections::BTreeMap<String, V> as __jet_Encode>::jet_encode(&**self)
    }
}
// D-ENC-CBOR-SURFACE1: Dyn itself is Codable. Whole-value codec
// composition must not fall through to rustc after front-end acceptance.
impl __jet_Encode for jet_std::Dyn {
    fn jet_encode(&self) -> jet_std::Dyn { self.clone() }
}

impl __jet_Decode for i64 {
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::jet_dyn_decode_fixed_integer(t, true, 64, "I64")?
            .try_into()
            .map_err(|_| jet_std::FieldError::one("expected I64, found out-of-range Int"))
    }
}

impl __jet_Decode for jet_foundation::Numeric::JetInt {
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::decode_int_with(
            t,
            jet_foundation::Numeric::JetInt::from_i64,
            |value| Ok(value.clone()),
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
// Dyn decode emits these Prelude-owned walks whenever a range appears
// below a list, option, fixed list, or string-keyed map.
fn jet_decode_inline_range(
    t: &jet_std::Dyn,
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
    t: &jet_std::Dyn,
    mut decode: F,
) -> Result<Vec<T>, Vec<jet_std::FieldError>>
where
    F: FnMut(&jet_std::Dyn) -> Result<T, Vec<jet_std::FieldError>>,
{
    let jet_std::Dyn::Array(items) = t else {
        return Err(jet_std::FieldError::one(format!(
            "expected a list, found {}",
            jet_std::dyn_kind_for(t)
        )));
    };
    jet_decode_sequence(items.len(), |index| decode(&items[index]))
}

fn jet_decode_inline_range_fixed<T, F, const N: usize>(
    t: &jet_std::Dyn,
    decode: F,
) -> Result<[T; N], Vec<jet_std::FieldError>>
where
    F: FnMut(&jet_std::Dyn) -> Result<T, Vec<jet_std::FieldError>>,
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
    t: &jet_std::Dyn,
    mut decode: F,
) -> Result<JetOutcome<T, JetAbsent>, Vec<jet_std::FieldError>>
where
    F: FnMut(&jet_std::Dyn) -> Result<T, Vec<jet_std::FieldError>>,
{
    match t {
        jet_std::Dyn::Null => Ok(Err(JetAbsent)),
        other => Ok(Ok(decode(other)?)),
    }
}

fn jet_decode_inline_range_map<V, F>(
    t: &jet_std::Dyn,
    mut decode: F,
) -> Result<JetMap<String, V>, Vec<jet_std::FieldError>>
where
    F: FnMut(&jet_std::Dyn) -> Result<V, Vec<jet_std::FieldError>>,
{
    let jet_std::Dyn::Object(entries) = t else {
        return Err(jet_std::FieldError::one(format!(
            "expected an object, found {}",
            jet_std::dyn_kind_for(t)
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
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::decode_float(t)
    }
}
impl __jet_Decode for bool {
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::decode_bool(t)
    }
}
impl __jet_Decode for String {
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::decode_string(t)
    }
}
impl __jet_Decode for char {
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
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
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
        let value = String::jet_decode(t)?;
        jet_codec_date_decode(&value)
            .map(|(year, month, day)| JetDate::new(year, month, day))
            .map_err(|error| jet_std::FieldError::one(format!("expected Date: {error}")))
    }
}
impl __jet_Decode for JetLocalTime {
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
        let value = String::jet_decode(t)?;
        jet_codec_local_time_decode(&value)
            .map(|(hour, minute, second)| JetLocalTime::new(hour, minute, second))
            .map_err(|error| jet_std::FieldError::one(format!("expected LocalTime: {error}")))
    }
}
impl __jet_Decode for JetDateTime {
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
        let tree = match t {
            jet_std::Dyn::DateTime(_) => t.clone(),
            jet_std::Dyn::Text(text) =>
                jet_std::Dyn::parse_datetime(text).map_err(jet_std::FieldError::one)?,
            other => return Err(jet_std::FieldError::one(format!(
                "expected DateTime, found {}", jet_std::dyn_kind_for(other)))),
        };
        let jet_std::Dyn::DateTime(value) = tree else { unreachable!() };
        Ok(JetDateTime::from_timestamp_ns_with_leap(
            value.unix_seconds, value.nanos, value.leap_second))
    }
}
impl __jet_Decode for jet_std::Duration {
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
        let ns = jet_std::decode_int_with(
            t,
            |value| value,
            |value| value.to_i64().ok_or_else(|| "exact Int is outside i64".to_owned()),
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

    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::jet_dyn_decode_fixed_integer(t, false, 8, "U8")?
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
            fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
                jet_std::jet_dyn_decode_fixed_integer(t, $signed, $bits, $name)?
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
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
        jet_std::decode_f32(t)
    }
}
impl __jet_Decode for jet_std::JetDecimal {
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
        match t {
            jet_std::Dyn::Number(text) => {
                jet_std::JetDecimal::from_json_number(text).map_err(|error| {
                    jet_std::FieldError::one(format!("expected Decimal: {error}"))
                })
            }
            jet_std::Dyn::Text(text) if jet_std::jet_text_cells_active() => {
                jet_codec_decimal_decode_text(text).map_err(|error| {
                    jet_std::FieldError::one(format!("expected Decimal: {error}"))
                })
            }
            jet_std::Dyn::Int(n) => jet_std::JetDecimal::from_json_number(&n.to_string_rep())
                .map_err(jet_std::FieldError::one),
            other => Err(jet_std::FieldError::one(format!(
                "expected Decimal, found {}", jet_std::dyn_kind_for(other)
            ))),
        }
    }
}
impl<T: __jet_Decode> __jet_Decode for Vec<T> {
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
        match t {
            jet_std::Dyn::Bytes(bytes) if <T as __jet_Decode>::__JET_IS_BYTE => {
                jet_decode_sequence(bytes.len(), |index| T::jet_decode_byte(bytes[index]))
            }
            jet_std::Dyn::Text(text)
                if <T as __jet_Decode>::__JET_IS_BYTE => {
                let bytes = jet_foundation::base_encoding_strict::decode_base64(text, false, false)
                    .map_err(|error| jet_std::FieldError::one(error.to_string()))?;
                jet_decode_sequence(bytes.len(), |index| T::jet_decode_byte(bytes[index]))
            }
            jet_std::Dyn::Array(items) => {
                jet_decode_sequence(items.len(), |index| T::jet_decode(&items[index]))
            }
            other => Err(jet_std::FieldError::one(format!(
                "expected a list, found {}",
                jet_std::dyn_kind_for(other)
            ))),
        }
    }
}
impl<T: __jet_Decode, const N: usize> __jet_Decode for [T; N] {
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
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
impl __jet_Decode for jet_std::Dyn {
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> { Ok(t.clone()) }
}
impl<T: __jet_Decode> __jet_Decode for JetOutcome<T, JetAbsent> {
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
        match t {
            jet_std::Dyn::Null => Ok(Err(JetAbsent)),
            other => Ok(Ok(T::jet_decode(other)?)),
        }
    }
}
impl<V: __jet_Decode> __jet_Decode for std::collections::BTreeMap<String, V> {
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
        match t {
            jet_std::Dyn::Object(entries) => {
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
                jet_std::dyn_kind_for(other)
            ))),
        }
    }
}
impl<V: __jet_Decode> __jet_Decode for JetMap<String, V> {
    fn jet_decode(t: &jet_std::Dyn) -> Result<Self, Vec<jet_std::FieldError>> {
        <std::collections::BTreeMap<String, V> as __jet_Decode>::jet_decode(t)
            .map(|map| map.into_iter().collect())
    }
}

// ── core.encoding.dyn: checked Dyn projections ─────────────────────
// Keep the carrier access policy in `JetDynAccess`; these thin ABI
// functions only expose that one implementation to generated MIR calls.
fn jet_dyn_field(
    tree: &jet_std::Dyn,
    name: &String,
) -> Result<jet_std::Dyn, Vec<jet_std::FieldError>> {
    <jet_std::Dyn as jet_std::JetDynAccess>::field(tree, name)
}
fn jet_dyn_at(
    tree: &jet_std::Dyn,
    index: i64,
) -> Result<jet_std::Dyn, Vec<jet_std::FieldError>> {
    <jet_std::Dyn as jet_std::JetDynAccess>::at(tree, index)
}
fn jet_dyn_int(
    tree: &jet_std::Dyn,
) -> Result<i64, Vec<jet_std::FieldError>> {
    <jet_std::Dyn as jet_std::JetDynAccess>::int(tree)
}
fn jet_dyn_text(
    tree: &jet_std::Dyn,
) -> Result<String, Vec<jet_std::FieldError>> {
    <jet_std::Dyn as jet_std::JetDynAccess>::text(tree)
}
fn jet_dyn_bool(
    tree: &jet_std::Dyn,
) -> Result<bool, Vec<jet_std::FieldError>> {
    <jet_std::Dyn as jet_std::JetDynAccess>::bool(tree)
}
fn jet_dyn_float(
    tree: &jet_std::Dyn,
) -> Result<f64, Vec<jet_std::FieldError>> {
    <jet_std::Dyn as jet_std::JetDynAccess>::float(tree)
}
fn jet_dyn_to_text(tree: &jet_std::Dyn) -> JetOutcome<String, JetAbsent> {
    jet_outcome_of(<jet_std::Dyn as jet_std::JetDynAccess>::to_text(tree))
}
fn jet_dyn_equal_unordered(
    tree: &jet_std::Dyn,
    other: &jet_std::Dyn,
) -> bool {
    <jet_std::Dyn as jet_std::JetDynAccess>::equal_unordered(tree, other)
}

// This route is intentionally generic: TIR carries the checked type argument,
// while the trait keeps all scalar/container decode policy in one place.
fn jet_codec_decode_typed<T: __jet_Decode>(
    tree: &jet_std::Dyn,
) -> Result<T, Vec<jet_std::FieldError>> {
    T::jet_decode(tree)
}

// Shared steps of natively printed derived codecs (MIRRust and the Jet
// emitter's `emit_native_derived_decode`). One keyed struct field:
// `FieldError.under(key, (tree.field(key) ?? Null).decode<T>())`, with its
// failures appended to `errors` in field order; `None` when it failed.
fn jet_decode_object_field<T: __jet_Decode>(
    tree: &jet_std::Dyn,
    key: &str,
    errors: &mut Vec<jet_std::FieldError>,
) -> Option<T> {
    let field = <jet_std::Dyn as jet_std::JetDynAccess>::field(tree, key)
        .unwrap_or(jet_std::Dyn::Null);
    match T::jet_decode(&field) {
        Ok(value) => Some(value),
        Err(failures) => {
            errors.extend(jet_std::FieldError::under_errors(key, failures));
            None
        }
    }
}

// One required struct field (no default, not optional): as
// `jet_decode_object_field`, except an absent key whose `Null` does not
// decode appends only `E2410: missing required field `key`` at `key`.
fn jet_decode_required_field<T: __jet_Decode>(
    tree: &jet_std::Dyn,
    key: &str,
    errors: &mut Vec<jet_std::FieldError>,
) -> Option<T> {
    let Ok(field) = <jet_std::Dyn as jet_std::JetDynAccess>::field(tree, key) else {
        return match T::jet_decode(&jet_std::Dyn::Null) {
            Ok(value) => Some(value),
            Err(_) => {
                errors.push(jet_std::FieldError {
                    path: key.to_string(),
                    reason: format!("E2410: missing required field `{key}`"),
                });
                None
            }
        };
    };
    match T::jet_decode(&field) {
        Ok(value) => Some(value),
        Err(failures) => {
            errors.extend(jet_std::FieldError::under_errors(key, failures));
            None
        }
    }
}

// The payload tree of an externally tagged enum variant:
// `tree.field(wire) ?? Null`.
fn jet_decode_enum_candidate(tree: &jet_std::Dyn, wire: &str) -> jet_std::Dyn {
    <jet_std::Dyn as jet_std::JetDynAccess>::field(tree, wire)
        .unwrap_or(jet_std::Dyn::Null)
}

// One single-payload variant: it matches when the payload tree decodes.
fn jet_decode_enum_payload<T: __jet_Decode>(tree: &jet_std::Dyn, wire: &str) -> Option<T> {
    T::jet_decode(&jet_decode_enum_candidate(tree, wire)).ok()
}

// ── core.encoding: typed format verbs over Encode/Decode (D-ENC1, D-SERDE6) ────
// `to_string`/`to_string_pretty` (D-JSONVERB1) and the typed `decode<T>` route
// every format through the one Dyn model.
// Internal MIR codec projections carry a checked kind ID so resident hosts can
// use the same ABI while AOT dispatches to these trait implementations.
fn jet_codec_encode<T: __jet_Encode>(_kind: i64, value: &T) -> jet_std::Dyn {
    value.jet_encode()
}
fn jet_codec_decode<T: __jet_Decode>(
    _kind: i64,
    tree: &jet_std::Dyn,
) -> Result<T, Vec<jet_std::FieldError>> {
    T::jet_decode(tree)
}

pub fn jet_enc_json_error_text(error: &jet_std::EncodingError) -> String {
    JetShow::jet_show(error)
}

// Typed JSON wire output checks JSON's exclusions before the diagnostic Dyn
// renderer. Exact Ints, ordered objects, Bytes, and DateTime keep Dyn policy.
fn jet_enc_json_validate(tree: &jet_std::Dyn) -> Result<(), jet_std::EncodingError> {
    match tree {
        jet_std::Dyn::Float(number) if !number.is_finite() => {
            return Err(jet_std::EncodingError::new(
                jet_std::EncodingFormat::JSON,
                jet_std::EncodingErrorKind::Unsupported,
                0,
                Err(JetAbsent),
                Err(JetAbsent),
                "$",
                "JSON cannot encode a non-finite Float",
            ));
        }
        jet_std::Dyn::Array(items) => {
            for item in items {
                jet_enc_json_validate(item)?;
            }
        }
        jet_std::Dyn::Object(entries) => {
            for (_, item) in entries {
                jet_enc_json_validate(item)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub fn jet_enc_json_to_string<T: __jet_Encode>(v: &T) -> Result<String, jet_std::EncodingError> {
    let tree = v.jet_encode();
    jet_enc_json_validate(&tree)?;
    Ok(jet_std::render_dyn_json(&tree, false, 0))
}
fn jet_enc_json_to_string_pretty<T: __jet_Encode>(v: &T) -> Result<String, jet_std::EncodingError> {
    let tree = v.jet_encode();
    jet_enc_json_validate(&tree)?;
    Ok(jet_std::render_dyn_json(&tree, true, 0))
}

pub fn jet_enc_json_decode<T: __jet_Decode>(text: &str) -> Result<T, Vec<jet_std::FieldError>> {
    let tree = jet_std::parse_json_dyn(text).map_err(|e| {
        let line = e.line.ok().unwrap_or(0);
        jet_std::FieldError::one(format!("invalid JSON (line {}): {}", line, e.reason))
    })?;
    // D-MIGRATE4: migration is part of the sole canonical decode operation.
    T::jet_decode(&tree)
}
fn jet_data_json_decode<T: __jet_Decode>(
    text: &str,
) -> Result<Vec<T>, Vec<jet_std::FieldError>> {
    jet_enc_json_decode::<Vec<T>>(text)
}
/// Decode an already-projected ordered object through the canonical typed
/// Dyn trait. DB supplies the entries after its carrier-only projection;
/// this function stays format-neutral and is shared by every host.
fn jet_db_decode<T: __jet_Decode>(
    entries: &[(String, jet_std::Dyn)],
) -> Result<T, Vec<jet_std::FieldError>> {
    T::jet_decode(&jet_std::Dyn::Object(entries.to_vec()))
}

// CSV typed decode: header row maps columns to fields by name; each data row
// becomes a Dyn::Object of Text cells, then decodes to `T`. A short row or a
// per-row decode failures are typed `[FieldError]` values naming the 1-based row.
fn jet_enc_csv_decode<T: __jet_Decode>(text: &str) -> Result<Vec<T>, Vec<jet_std::FieldError>> {
    let rows = jet_ring_csv_parse(text, ",", false, false)
        .map_err(jet_std::FieldError::one)?;
    jet_std::jet_enc_csv_decode_rows(
        rows,
        // CSV cells are text-native: each row decodes inside the text-cell scope.
        |tree| jet_std::jet_decode_text_cells(|| T::jet_decode(&tree)),
        |row, error| jet_std::FieldError::under_errors(row, error),
        jet_std::Dyn::Text,
        jet_std::Dyn::Object,
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

