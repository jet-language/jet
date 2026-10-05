// ── core.encoding: format-agnostic value tree (D-SERDE2 = A) ───────────────
// The shared Foundation Dyn carrier is embedded by Codegen before this
// runtime extension. The built-in `#[Codable]` derive (D-ENC1) lowers
// `encode`/`decode` to walks over the carrier; adapters turn it into wire data.
// The carrier preserves field order (ordered `Object`), keeps Int vs Float, and
// keeps a parsed number's exact lexeme as `Number` until a typed read (#4602).

// #4602: decoding is strict. A `Text` leaf never becomes an Int, Float, or
// Bool, so JSON `"42"` does not decode into an `Int` field. The one exception
// is a text-native source (environment variables, CSV cells, route and query
// parameters), where every scalar is text by construction: its adapter runs
// the decode inside `jet_decode_text_cells`, and only there a `Text` leaf
// parses into a scalar target.
std::thread_local! {
    static JET_TEXT_CELLS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

struct JetTextCellsScope(bool);

impl Drop for JetTextCellsScope {
    fn drop(&mut self) {
        let previous = self.0;
        JET_TEXT_CELLS.with(|cell| cell.set(previous));
    }
}

/// Run a text-native source's decode with scalar parsing of `Text` leaves.
pub fn jet_decode_text_cells<R>(decode: impl FnOnce() -> R) -> R {
    let _scope = JetTextCellsScope(JET_TEXT_CELLS.with(|cell| cell.replace(true)));
    decode()
}

pub fn jet_text_cells_active() -> bool {
    JET_TEXT_CELLS.with(|cell| cell.get())
}

// D-VALIDATE-DECODE1=B (ratified 2026-08-03): every typed decoder returns
// the same accumulated field-error list. Structural failures use the empty
// path; callers add their field/index segment with `under`. There is no
// second decode error envelope.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldError {
    pub path: String,
    pub reason: String,
}

impl FieldError {
    pub fn one(reason: impl Into<String>) -> Vec<FieldError> {
        vec![FieldError {
            path: String::new(),
            reason: reason.into(),
        }]
    }

    pub fn at(path: impl Into<String>, reason: impl Into<String>) -> Vec<FieldError> {
        vec![FieldError {
            path: path.into(),
            reason: reason.into(),
        }]
    }

    // Prefix every failure from a child decode. Keeping this operation on
    // the canonical error type makes nested records, lists, and maps
    // preserve all failures instead of collapsing to the first one.
    pub fn under_errors(seg: &str, mut errors: Vec<FieldError>) -> Vec<FieldError> {
        for error in &mut errors {
            error.path = jet_field_error_kernel_under(seg, &error.path);
        }
        errors
    }

    // The Jet-facing transform keeps a child Result intact on success and
    // frames every member of its accumulated error list on failure.
    pub fn under<T>(seg: &str, result: Result<T, Vec<FieldError>>) -> Result<T, Vec<FieldError>> {
        result.map_err(|errors| Self::under_errors(seg, errors))
    }
}

impl super::JetShow for Dyn {
    fn jet_show(&self) -> String {
        self.to_text().unwrap_or_else(|| render_dyn_json(self, false, 0))
    }
}

impl super::JetDisplay for Dyn {
    fn jet_display(&self) -> String {
        <Self as super::JetShow>::jet_show(self)
    }
}

impl super::JetDebug for Dyn {
    fn jet_debug(&self) -> String {
        render_dyn_json(self, false, 0)
    }
}
/// Native integer encoders construct an owned exact Int, never a Number token.
pub(crate) fn jet_dyn_encode_i64(value: i64) -> Dyn {
    Dyn::Int(value.into())
}

pub(crate) fn jet_dyn_encode_u64(value: u64) -> Dyn {
    Dyn::Int(value.into())
}

/// Construct the canonical owned payload. Parsed Number tokens stay distinct.
pub fn jet_dyn_int(value: jet_foundation::Numeric::JetInt) -> Dyn {
    Dyn::Int(value)
}

pub fn jet_dyn_is_int(tree: &Dyn) -> bool {
    matches!(tree, Dyn::Int(_))
}

pub fn jet_dyn_int_payload(tree: &Dyn) -> Option<jet_foundation::Numeric::JetInt> {
    match tree {
        Dyn::Int(value) => Some(value.clone()),
        _ => None,
    }
}

// D-MIGRATE3=A / D-MIGRATE4=A: migration is transparent inside the one
// canonical `__jet_Decode::jet_decode` operation. The generated decoder
// tries the current shape first, then walks a matching historical shape
// forward before returning the value. No second result envelope exists.

/// D-MIGRATE4: the sorted set of top-level object keys of a `Dyn`, used
/// by a `#PublishedSchema` type's migration chain-walker to detect which
/// historical shape a decoded record matches. A non-object tree has no keys.
pub fn jet_dyn_key_set(t: &Dyn) -> std::collections::BTreeSet<String> {
    match t {
        Dyn::Object(pairs) => pairs.iter().map(|(k, _)| k.clone()).collect(),
        _ => std::collections::BTreeSet::new(),
    }
}

/// D-BOUND-EVOLVE1=A: replace known values in the original wire order and
/// append newly-created known fields in schema order. Unknown entries stay
/// byte-for-byte in their original position relative to the known fields.
pub fn jet_dyn_merge_wire_order(known: &Dyn, original: &Dyn) -> Dyn {
    let (Dyn::Object(known), Dyn::Object(original)) = (known, original) else {
        return known.clone();
    };
    Dyn::Object(jet_wire_order_merge(known, original))
}

/// D-SHAPE-ONE1=A: apply one already-resolved shape projection to a
/// `Dyn`. The front end selects each `ShapeProjectedField::name` for its
/// consumer (including per-format Rename); codec adapters only render the
/// resulting ordered tree and never rediscover field names.
pub fn jet_dyn_project(
    tree: &Dyn,
    projection: &ShapeProjection,
) -> Result<Dyn, Vec<FieldError>> {
    let project_object = |value: &Dyn| {
        let Dyn::Object(entries) = value else {
            return Err(FieldError::one("shape projection needs object values"));
        };
        let mut projected = Vec::with_capacity(projection.fields.len());
        for field in &projection.fields {
            let Some((_, value)) = entries
                .iter()
                .find(|(name, _)| name == &field.decode_name)
            else {
                continue;
            };
            projected.push((field.name.clone(), value.clone()));
        }
        Ok(Dyn::Object(projected))
    };

    match tree {
        Dyn::Object(_) => project_object(tree),
        Dyn::Array(values) => values
            .iter()
            .map(project_object)
            .collect::<Result<Vec<_>, _>>()
            .map(Dyn::Array),
        _ => Err(FieldError::one("shape projection needs object values")),
    }
}


// Engine adapters reduce their resident value to this tag before calling
// the Prelude-owned diagnostic vocabulary.
pub fn dyn_kind_for(t: &Dyn) -> &'static str {
    let tag = match t {
        Dyn::Null => "Null",
        Dyn::Bool(_) => "Bool",
        Dyn::Int(_) => "Int",
        Dyn::Float(_) => "Float",
        Dyn::Number(_) => "Number",
        Dyn::Text(_) => "Text",
        Dyn::Bytes(_) => "Bytes",
        Dyn::DateTime(_) => "DateTime",
        Dyn::Array(_) => "Array",
        Dyn::Object(_) => "Object",
    };
    dyn_kind(tag)
}
// D-SERDE-ACCESS=B + D-SERDE14=A: dynamic accessor methods on Dyn. Each
// read returns `Result<T, [FieldError]>` so a `?` chain composes cleanly
// inside a hand `decode`. `.field`/`.at` auto-fill the path with the
// segment they read; scalar readers leave it empty, so a containing
// field/list/map decoder frames the child result with `FieldError.under`.
//
// The carrier type is owned by Foundation; this tier-side extension is a
// trait because Rust only admits inherent impls in the defining crate, and the
// accessors read the tier's `jet_int_*` marshalling (D-INTBIG1). AOT, the
// Cranelift host, and the TIR evaluator each include this one file.
pub trait JetDynAccess {
    fn field(&self, name: &str) -> Result<Dyn, Vec<FieldError>>;
    fn at(&self, i: i64) -> Result<Dyn, Vec<FieldError>>;
    fn int(&self) -> Result<i64, Vec<FieldError>>;
    fn text(&self) -> Result<String, Vec<FieldError>>;
    fn bool(&self) -> Result<bool, Vec<FieldError>>;
    fn float(&self) -> Result<f64, Vec<FieldError>>;
    /// D-DATATREE-ERGO1=A: project scalar leaves to text without weakening the
    /// strict `.text()` accessor. Containers and non-textual leaves have no
    /// scalar projection.
    fn to_text(&self) -> Option<String>;
    /// D-DATATREE-ERGO1=A: compare trees while ignoring object insertion
    /// order. Arrays stay ordered, numeric variants stay distinct, and NaN is
    /// never equal to anything, including another NaN.
    fn equal_unordered(&self, other: &Dyn) -> bool;
}

impl JetDynAccess for Dyn {
    fn field(&self, name: &str) -> Result<Dyn, Vec<FieldError>> {
        match self {
            Dyn::Object(pairs) => pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| FieldError::at(name, format!("field `{}` not found", name))),
            _ => Err(FieldError::at(
                name,
                format!(
                    "expected object, got {}",
                    render_dyn_json(self, false, 0)
                ),
            )),
        }
    }
    fn at(&self, i: i64) -> Result<Dyn, Vec<FieldError>> {
        match self {
            Dyn::Array(items) => {
                let idx = if i < 0 {
                    i.checked_neg()
                        .and_then(|value| usize::try_from(value).ok())
                        .and_then(|value| items.len().checked_sub(value))
                } else {
                    usize::try_from(i).ok()
                };
                let Some(idx) = idx else {
                    return Err(FieldError::at(
                        format!("[{}]", i),
                        format!("index {} out of bounds (len {})", i, items.len()),
                    ));
                };
                items.get(idx).cloned().ok_or_else(|| {
                    FieldError::at(
                        format!("[{}]", i),
                        format!("index {} out of bounds (len {})", i, items.len()),
                    )
                })
            }
            _ => Err(FieldError::at(
                format!("[{}]", i),
                format!(
                    "expected array, got {}",
                    render_dyn_json(self, false, 0)
                ),
            )),
        }
    }
    fn int(&self) -> Result<i64, Vec<FieldError>> {
        match self {
            Dyn::Int(n) => Ok(n.clone().into_raw()),
            // #4602: a parsed number keeps its lexeme as `Number`; reading it
            // as an Int range-checks it exactly as the `decode_*` helpers do
            // for derived codecs — one protocol for hand and derived codecs.
            Dyn::Number(text) => crate::jet_json_number::json_exact_integer_text(text)
                .ok()
                .and_then(|digits| jet_int_from_str(&digits).ok())
                .ok_or_else(|| {
                    FieldError::one(format!(
                        "expected int, got {}",
                        render_dyn_json(self, false, 0)
                    ))
                }),
            _ => Err(FieldError::one(format!(
                "expected int, got {}",
                render_dyn_json(self, false, 0)
            ))),
        }
    }
    fn text(&self) -> Result<String, Vec<FieldError>> {
        match self {
            Dyn::Text(s) => Ok(s.clone()),
            _ => Err(FieldError::one(format!(
                "expected text, got {}",
                render_dyn_json(self, false, 0)
            ))),
        }
    }
    fn bool(&self) -> Result<bool, Vec<FieldError>> {
        match self {
            Dyn::Bool(b) => Ok(*b),
            _ => Err(FieldError::one(format!(
                "expected bool, got {}",
                render_dyn_json(self, false, 0)
            ))),
        }
    }
    fn float(&self) -> Result<f64, Vec<FieldError>> {
        match self {
            Dyn::Float(f) => Ok(*f),
            Dyn::Int(n) => {
                let value = n.to_f64();
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err(FieldError::one("expected float, got out-of-range Int"))
                }
            }
            Dyn::Number(text) => text
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| {
                    FieldError::one(format!(
                        "expected float, got {}",
                        render_dyn_json(self, false, 0)
                    ))
                }),
            _ => Err(FieldError::one(format!(
                "expected float, got {}",
                render_dyn_json(self, false, 0)
            ))),
        }
    }

    fn to_text(&self) -> Option<String> {
        match self {
            Dyn::Text(text) => Some(text.clone()),
            Dyn::Int(value) => Some(value.to_string_rep()),
            Dyn::Float(value) => Some(format!("{:?}", value)),
            Dyn::Bool(value) => Some(value.to_string()),
            Dyn::Number(text) => Some(text.clone()),
            Dyn::Bytes(bytes) => Some(Dyn::bytes_text(bytes)),
            Dyn::DateTime(value) => Some(value.to_rfc3339()),
            Dyn::Null | Dyn::Array(_) | Dyn::Object(_) => None,
        }
    }

    fn equal_unordered(&self, other: &Dyn) -> bool {
        match (self, other) {
            (Dyn::Null, Dyn::Null) => true,
            (Dyn::Bool(left), Dyn::Bool(right)) => left == right,
            (Dyn::Int(left), Dyn::Int(right)) => left == right,
            (Dyn::Float(left), Dyn::Float(right)) => {
                !left.is_nan() && !right.is_nan() && left == right
            }
            (Dyn::Number(left), Dyn::Number(right)) => left == right,
            (Dyn::Text(left), Dyn::Text(right)) => left == right,
            (Dyn::Bytes(left), Dyn::Bytes(right)) => left == right,
            (Dyn::DateTime(left), Dyn::DateTime(right)) => left == right,
            (Dyn::Array(left), Dyn::Array(right)) => {
                left.len() == right.len()
                    && left
                        .iter()
                        .zip(right)
                        .all(|(left, right)| left.equal_unordered(right))
            }
            (Dyn::Object(left), Dyn::Object(right)) => {
                // ponytail: keep the list-backed duplicate-key semantics; add
                // indexed matching only if large dynamic objects need it.
                if left.len() != right.len() {
                    return false;
                }
                let mut used = vec![false; right.len()];
                for (key, value) in left {
                    let mut matched = None;
                    for (index, (other_key, other_value)) in right.iter().enumerate() {
                        if !used[index]
                            && key == other_key
                            && value.equal_unordered(other_value)
                        {
                            matched = Some(index);
                            break;
                        }
                    }
                    let Some(index) = matched else {
                        return false;
                    };
                    used[index] = true;
                }
                true
            }
            _ => false,
        }
    }
}

/// Decode one fixed-width integer from the exact Dyn number carrier.
///
/// The result is an exact `i128` after the shared numeric kernel has checked
/// the destination width. A `Text` leaf decodes only inside a text-native
/// source, so JSON quoted numerals stay rejected; callers perform the final
/// native cast.
pub(crate) fn jet_dyn_decode_fixed_integer(
    t: &Dyn,
    signed: bool,
    bits: u8,
    type_name: &str,
) -> Result<i128, Vec<FieldError>> {
    let kind = match (signed, bits) {
        (true, 8) => 0,
        (true, 16) => 1,
        (true, 32) => 2,
        (true, 64) => 3,
        (false, 8) => 4,
        (false, 16) => 5,
        (false, 32) => 6,
        (false, 64) => 7,
        _ => {
            return Err(FieldError::one(format!(
                "expected {type_name}, found out-of-range Int"
            )))
        }
    };
    let value = decode_int_with(
        t,
        i128::from,
        |value| value.to_i128().ok_or_else(|| "exact Int is outside i128".to_string()),
        |text| {
            text.parse::<i128>()
                .map_err(|_| "exact Int is outside i128".to_string())
        },
    )?;
    if jet_foundation::NumericConversion::jet_numeric_fixed_from_i128(value, kind).is_some() {
        Ok(value)
    } else {
        Err(FieldError::one(format!(
            "expected {type_name}, found out-of-range Int"
        )))
    }
}

macro_rules! jet_dyn_decode_helpers {
    () => {
        // Primitive typed-decode rules live beside the tree and are shared by the
        // AOT Prelude and the JIT's handle marshalling adapter. The engines only
        // convert their resident representation to this tree and back.
        /// Decode exact `Int` values through an engine-owned carrier pair.
        ///
        /// `from_i64` owns every integral `Int`/`Float` conversion, while
        /// `parse_exact` owns the destination's exact text carrier. Number
        /// exponents are normalized here, and all coercion diagnostics stay on
        /// this one Prelude path.
        pub fn decode_int_with<T>(
            t: &Dyn,
            from_i64: impl Fn(i64) -> T,
            from_exact: impl Fn(&jet_foundation::Numeric::JetInt) -> Result<T, String>,
            parse_exact: impl Fn(&str) -> Result<T, String>,
        ) -> Result<T, Vec<FieldError>> {
            match t {
                Dyn::Int(n) => from_exact(n).map_err(FieldError::one),
                Dyn::Float(f)
                    if f.is_finite()
                        && f.fract() == 0.0
                        && *f >= i64::MIN as f64
                        && *f < i64::MAX as f64 =>
                {
                    Ok(from_i64(*f as i64))
                }
                Dyn::Number(text) => {
                    let integer = crate::jet_json_number::json_exact_integer_text(text)
                        .map_err(FieldError::one)?;
                    parse_exact(&integer)
                        .map_err(|_| FieldError::one(format!("expected Int, found number {text}")))
                }
                Dyn::Text(text) if jet_text_cells_active() => parse_exact(text.trim())
                    .map_err(|_| FieldError::one(format!("expected Int, found text {:?}", text))),
                Dyn::Text(text) => Err(FieldError::one(format!(
                    "expected Int, found text {:?}",
                    text
                ))),
                other => Err(FieldError::one(format!(
                    "expected Int, found {}",
                    dyn_kind_for(other)
                ))),
            }
        }

        /// Resident raw exact-Int wrapper. Every successful result is a
        /// managed carrier, including a large `Dyn::Int`.
        pub fn decode_int(t: &Dyn) -> Result<i64, Vec<FieldError>> {
            decode_int_with(t, jet_int_from_i64, |value| Ok(value.clone().into_raw()), |text| jet_int_from_str(text))
        }
        pub fn decode_float(t: &Dyn) -> Result<f64, Vec<FieldError>> {
            match t {
                Dyn::Float(f) => Ok(*f),
                Dyn::Int(n) => {
                    let value = n.to_f64();
                    if value.is_finite() {
                        Ok(value)
                    } else {
                        Err(FieldError::one("expected float, got out-of-range Int"))
                    }
                }
                Dyn::Number(text) => decode_finite_lexeme(text, "Float"),
                Dyn::Text(text) if jet_text_cells_active() => text
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite())
                    .ok_or_else(|| {
                        FieldError::one(format!("expected Float, found text {:?}", text))
                    }),
                Dyn::Text(text) => Err(FieldError::one(format!(
                    "expected Float, found text {:?}",
                    text
                ))),
                other => Err(FieldError::one(format!(
                    "expected Float, found {}",
                    dyn_kind_for(other)
                ))),
            }
        }

        pub fn decode_bool(t: &Dyn) -> Result<bool, Vec<FieldError>> {
            match t {
                Dyn::Bool(value) => Ok(*value),
                Dyn::Text(text) if jet_text_cells_active() => match text.trim() {
                    "true" => Ok(true),
                    "false" => Ok(false),
                    _ => Err(FieldError::one(format!(
                        "expected Bool, found text {:?}",
                        text
                    ))),
                },
                Dyn::Text(text) => Err(FieldError::one(format!(
                    "expected Bool, found text {:?}",
                    text
                ))),
                other => Err(FieldError::one(format!(
                    "expected Bool, found {}",
                    dyn_kind_for(other)
                ))),
            }
        }

        pub fn decode_string(t: &Dyn) -> Result<String, Vec<FieldError>> {
            match t {
                Dyn::Text(text) => Ok(text.clone()),
                Dyn::Number(text) => Err(FieldError::one(format!(
                    "expected Text, found number {text}"
                ))),
                other => Err(FieldError::one(format!(
                    "expected Text, found {}",
                    dyn_kind_for(other)
                ))),
            }
        }

        /// Range-check a `Number` lexeme read as a binary64 value: `1e400`
        /// keeps its exact text in the tree and fails only here.
        pub fn decode_finite_lexeme(text: &str, target: &str) -> Result<f64, Vec<FieldError>> {
            match text.parse::<f64>() {
                Ok(value) if value.is_finite() => Ok(value),
                Ok(_) => Err(FieldError::one(format!(
                    "expected {target}, found out-of-range number {text}"
                ))),
                Err(_) => Err(FieldError::one(format!(
                    "expected {target}, found number {text}"
                ))),
            }
        }

        pub fn decode_f32(t: &Dyn) -> Result<f32, Vec<FieldError>> {
            let value = match t {
                Dyn::Float(value) => *value,
                Dyn::Int(value) => value.to_f64(),
                Dyn::Number(text) => decode_finite_lexeme(text, "F32")?,
                Dyn::Text(text) if jet_text_cells_active() => text
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| FieldError::one(format!("expected F32, found text {:?}", text)))?,
                Dyn::Text(text) => {
                    return Err(FieldError::one(format!(
                        "expected F32, found text {:?}",
                        text
                    )))
                }
                other => {
                    return Err(FieldError::one(format!(
                        "expected F32, found {}",
                        dyn_kind_for(other)
                    )))
                }
            };
            if value.is_finite() && value >= -(f32::MAX as f64) && value <= f32::MAX as f64 {
                Ok(value as f32)
            } else {
                Err(FieldError::one("expected F32, found out-of-range Float"))
            }
        }

        pub fn check_f32_range(
            value: Result<f64, Vec<FieldError>>,
        ) -> Result<f64, Vec<FieldError>> {
            let value = value?;
            if value.is_finite() && value >= -(f32::MAX as f64) && value <= f32::MAX as f64 {
                Ok(value)
            } else {
                Err(FieldError::one("expected F32, found out-of-range Float"))
            }
        }

        pub fn fixed_list_length_error(found: usize, expected: usize) -> Vec<FieldError> {
            FieldError::one(format!(
                "expected a fixed list of length {expected}, found {found}"
            ))
        }
    };
}

// D-VALIDATE-DECODE1=B: one user-facing projection of a decode/validate
// failure. `JetShow` is the value shape; `JetDisplay` is the interpolation
// hook `{errs}` lowers to (`Vec<T>` has the blanket impl), so `print(errs)`
// and `print("{errs}")` render the same text. The text itself lives in
// `jet_field_error_kernel_show` (Prelude/Core/FieldError.rs), which the
// Cranelift host and the TIR evaluator call too — no tier re-encodes it.
impl super::JetShow for FieldError {
    fn jet_show(&self) -> String {
        jet_field_error_kernel_show(&self.path, &self.reason)
    }
}
impl super::JetDebug for FieldError {
    fn jet_debug(&self) -> String {
        <Self as super::JetShow>::jet_show(self)
    }
}
impl super::JetDisplay for FieldError {
    fn jet_display(&self) -> String {
        <Self as super::JetShow>::jet_show(self)
    }
}
