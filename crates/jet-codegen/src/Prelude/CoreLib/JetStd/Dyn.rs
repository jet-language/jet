// ── core.encoding: format-agnostic value tree (D-SERDE2 = A) ───────────────
// The shared Foundation Dyn carrier is embedded by Codegen before this
// runtime extension. The built-in `#[Codable]` derive (D-ENC1) lowers
// `encode`/`decode` to walks over the carrier; adapters turn it into wire data.
// The carrier preserves field order (ordered `Object`) and keeps Int vs Float.

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
        render_dyn_json(self, false, 0)
    }
}

impl super::JetDisplay for Dyn {
    fn jet_display(&self) -> String {
        <Self as super::JetShow>::jet_show(self)
    }
}

impl super::JetDebug for Dyn {
    fn jet_debug(&self) -> String {
        <Self as super::JetShow>::jet_show(self)
    }
}
/// Preserve exact integer semantics at the Dyn boundary. The Int variant
/// carries only the inline JetInt payload; larger values stay lexical Numbers
/// so a tagged owner word is never mistaken for a host integer.
pub(crate) fn jet_dyn_encode_i64(value: i64) -> Dyn {
    if (jet_foundation::Numeric::JetInt::inline_min()
        ..=jet_foundation::Numeric::JetInt::inline_max())
        .contains(&value)
    {
        Dyn::Int(value)
    } else {
        Dyn::Number(value.to_string())
    }
}

pub(crate) fn jet_dyn_encode_u64(value: u64) -> Dyn {
    if value <= jet_foundation::Numeric::JetInt::inline_max() as u64 {
        Dyn::Int(value as i64)
    } else {
        Dyn::Number(value.to_string())
    }
}

/// Jet's `Dyn.Int` holds an exact `Int`. The native carrier keeps a value
/// that does not fit the inline slot as its decimal `Number` lexeme (the rule
/// `jet_encode` uses), and these three functions are the one crossing of the
/// Jet variant over that carrier: construct, test, and read the payload.
pub fn jet_dyn_int(value: jet_foundation::Numeric::JetInt) -> Dyn {
    value
        .to_i64()
        .map(jet_dyn_encode_i64)
        .unwrap_or_else(|| Dyn::Number(value.to_string_rep()))
}

pub fn jet_dyn_is_int(tree: &Dyn) -> bool {
    jet_dyn_int_payload(tree).is_some()
}

pub fn jet_dyn_int_payload(tree: &Dyn) -> Option<jet_foundation::Numeric::JetInt> {
    match tree {
        Dyn::Int(value) => Some(jet_foundation::Numeric::JetInt::from(*value)),
        Dyn::Number(text) => jet_foundation::Numeric::CtBigInt::from_str(text)
            .map(jet_foundation::Numeric::JetInt::from_big)
            .ok(),
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
        Dyn::TypedText(_) => "Text",
        Dyn::Text(_) => "Text",
        Dyn::Bytes(_) => "Bytes",
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
            Dyn::Int(n) => Ok(*n),
            // D-SERDE2: a hand `decode` runs inside the typed walk, so the
            // lexical `Number`/`TypedText` carriers reach these accessors.
            // Project them here exactly as the `decode_*` helpers do for
            // derived codecs — one protocol for hand and derived codecs.
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
            Dyn::Text(s) | Dyn::TypedText(s) => Ok(s.clone()),
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
                let value = jet_int_to_f64(*n);
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
            Dyn::Text(text) | Dyn::TypedText(text) => Some(text.clone()),
            Dyn::Int(value) => Some(jet_int_to_string(*value)),
            Dyn::Float(value) => Some(format!("{:?}", value)),
            Dyn::Bool(value) => Some(value.to_string()),
            Dyn::Number(text) => Some(text.clone()),
            Dyn::Null | Dyn::Bytes(_) | Dyn::Array(_) | Dyn::Object(_) => None,
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
            (Dyn::TypedText(left), Dyn::TypedText(right)) => left == right,
            (Dyn::Text(left), Dyn::Text(right)) => left == right,
            (Dyn::Bytes(left), Dyn::Bytes(right)) => left == right,
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
/// the destination width. `TypedText` never reaches `decode_int_with`, so JSON
/// quoted numerals remain rejected; callers perform the final native cast.
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
            parse_exact: impl Fn(&str) -> Result<T, String>,
        ) -> Result<T, Vec<FieldError>> {
            match t {
                Dyn::Int(n) => Ok(from_i64(*n)),
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
                Dyn::TypedText(text) => Err(FieldError::one(format!(
                    "expected Int, found text {:?}",
                    text
                ))),
                Dyn::Text(text) => parse_exact(text.trim())
                    .map_err(|_| FieldError::one(format!("expected Int, found text {:?}", text))),
                other => Err(FieldError::one(format!(
                    "expected Int, found {}",
                    dyn_kind_for(other)
                ))),
            }
        }

        /// Resident raw exact-Int wrapper. Every successful result is a
        /// managed carrier, including a large `Dyn::Int`.
        pub fn decode_int(t: &Dyn) -> Result<i64, Vec<FieldError>> {
            decode_int_with(t, jet_int_from_i64, |text| jet_int_from_str(text))
        }
        pub fn decode_float(t: &Dyn) -> Result<f64, Vec<FieldError>> {
            match t {
                Dyn::Float(f) => Ok(*f),
                Dyn::Int(n) => {
                    let value = jet_int_to_f64(*n);
                    if value.is_finite() {
                        Ok(value)
                    } else {
                        Err(FieldError::one("expected float, got out-of-range Int"))
                    }
                }
                Dyn::Number(text) | Dyn::Text(text) => {
                    let value = text.trim().parse::<f64>().map_err(|_| {
                        FieldError::one(format!("expected Float, found text {:?}", text))
                    })?;
                    if value.is_finite() {
                        Ok(value)
                    } else {
                        Err(FieldError::one("expected Float, found out-of-range Float"))
                    }
                }
                Dyn::TypedText(text) => Err(FieldError::one(format!(
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
                Dyn::TypedText(text) => Err(FieldError::one(format!(
                    "expected Bool, found text {:?}",
                    text
                ))),
                Dyn::Text(text) => match text.trim() {
                    "true" => Ok(true),
                    "false" => Ok(false),
                    _ => Err(FieldError::one(format!(
                        "expected Bool, found text {:?}",
                        text
                    ))),
                },
                other => Err(FieldError::one(format!(
                    "expected Bool, found {}",
                    dyn_kind_for(other)
                ))),
            }
        }

        pub fn decode_string(t: &Dyn) -> Result<String, Vec<FieldError>> {
            match t {
                Dyn::Text(text) | Dyn::TypedText(text) => Ok(text.clone()),
                Dyn::Number(text) => Err(FieldError::one(format!(
                    "expected Text, found number {text}"
                ))),
                Dyn::Int(n) => Ok(jet_int_to_string(*n)),
                Dyn::Float(f) => Ok(format!("{:?}", f)),
                Dyn::Bool(value) => Ok(value.to_string()),
                other => Err(FieldError::one(format!(
                    "expected Text, found {}",
                    dyn_kind_for(other)
                ))),
            }
        }

        pub fn decode_f32(t: &Dyn) -> Result<f32, Vec<FieldError>> {
            let value = match t {
                Dyn::Float(value) => *value,
                Dyn::Int(value) => jet_int_to_f64(*value),
                Dyn::Number(text) | Dyn::Text(text) => text
                    .trim()
                    .parse::<f64>()
                    .map_err(|_| FieldError::one(format!("expected F32, found text {:?}", text)))?,
                Dyn::TypedText(text) => {
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
