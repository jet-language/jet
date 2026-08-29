    // ── core.encoding: format-agnostic value tree (D-SERDE2 = A) ───────────────
    // The one tree every format adapter speaks. The built-in `#[Codable]` derive
    // (D-ENC1) lowers `encode`/`decode` to walks over this; each adapter turns it
    // into / parses it from wire text. Distinct from the dynamic `JSON` enum:
    // `DataTree` preserves field order (ordered `Object`) and keeps Int vs Float.
    #[derive(Clone, Debug, PartialEq)]
    pub enum DataTree {
        Null,
        Bool(bool),
        Int(i64),
        Float(f64),
        // Internal typed-JSON carriers. Sema does not expose these as public
        // DataTree constructors; the typed visitor consumes them before a
        // user-visible tree can escape.
        Number(String),
        TypedText(String),
        Text(String),
        Bytes(Vec<u8>),
        Array(Vec<DataTree>),
        Object(Vec<(String, DataTree)>),
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
        pub fn under_errors(seg: &str, errors: Vec<FieldError>) -> Vec<FieldError> {
            let mut framed = Vec::with_capacity(errors.len());
            for mut error in errors {
                error.path = if error.path.is_empty() {
                    seg.to_string()
                } else if error.path.starts_with('[') {
                    format!("{}{}", seg, error.path)
                } else {
                    format!("{}.{}", seg, error.path)
                };
                framed.push(error);
            }
            framed
        }

        // The Jet-facing transform keeps a child Result intact on success and
        // frames every member of its accumulated error list on failure.
        pub fn under<T>(seg: &str, result: Result<T, Vec<FieldError>>) -> Result<T, Vec<FieldError>> {
            result.map_err(|errors| Self::under_errors(seg, errors))
        }
    }

    impl super::JetShow for DataTree {
        fn jet_show(&self) -> String {
            render_datatree_json(self, false, 0)
        }
    }

    // D-MIGRATE3=A / D-MIGRATE4=A: migration is transparent inside the one
    // canonical `__jet_Decode::jet_decode` operation. The generated decoder
    // tries the current shape first, then walks a matching historical shape
    // forward before returning the value. No second result envelope exists.

    /// D-MIGRATE4: the sorted set of top-level object keys of a `DataTree`, used
    /// by a `#PublishedSchema` type's migration chain-walker to detect which
    /// historical shape a decoded record matches. A non-object tree has no keys.
    pub fn jet_datatree_key_set(t: &DataTree) -> std::collections::BTreeSet<String> {
        match t {
            DataTree::Object(pairs) => pairs.iter().map(|(k, _)| k.clone()).collect(),
            _ => std::collections::BTreeSet::new(),
        }
    }

    /// D-BOUND-EVOLVE1=A: replace known values in the original wire order and
    /// append newly-created known fields in schema order. Unknown entries stay
    /// byte-for-byte in their original position relative to the known fields.
    pub fn jet_datatree_merge_wire_order(known: &DataTree, original: &DataTree) -> DataTree {
        let (DataTree::Object(known), DataTree::Object(original)) = (known, original) else {
            return known.clone();
        };
        DataTree::Object(jet_wire_order_merge(known, original))
    }

    // Engine adapters reduce their resident value to this tag before calling
    // the Prelude-owned diagnostic vocabulary.
    pub fn datatree_kind_for(t: &DataTree) -> &'static str {
        let tag = match t {
            DataTree::Null => "Null",
            DataTree::Bool(_) => "Bool",
            DataTree::Int(_) => "Int",
            DataTree::Float(_) => "Float",
            DataTree::Number(_) => "Number",
            DataTree::TypedText(_) => "Text",
            DataTree::Text(_) => "Text",
            DataTree::Bytes(_) => "Bytes",
            DataTree::Array(_) => "Array",
            DataTree::Object(_) => "Object",
        };
        datatree_kind(tag)
    }
    // D-SERDE-ACCESS=B + D-SERDE14=A: dynamic accessor methods on DataTree. Each
    // read returns `Result<T, [FieldError]>` so a `?` chain composes cleanly
    // inside a hand `decode`. `.field`/`.at` auto-fill the path with the
    // segment they read; scalar readers leave it empty, so a containing
    // field/list/map decoder frames the child result with `FieldError.under`.
    impl DataTree {
        pub fn field(&self, name: &str) -> Result<DataTree, Vec<FieldError>> {
            match self {
                DataTree::Object(pairs) => pairs
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| FieldError::at(name, format!("field `{}` not found", name))),
                _ => Err(FieldError::at(
                    name,
                    format!("expected object, got {}", render_datatree_json(self, false, 0)),
                )),
            }
        }
        pub fn at(&self, i: i64) -> Result<DataTree, Vec<FieldError>> {
            match self {
                DataTree::Array(items) => {
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
                    items.get(idx).cloned().ok_or_else(|| FieldError::at(
                        format!("[{}]", i),
                        format!("index {} out of bounds (len {})", i, items.len()),
                    ))
                }
                _ => Err(FieldError::at(
                    format!("[{}]", i),
                    format!("expected array, got {}", render_datatree_json(self, false, 0)),
                )),
            }
        }
        pub fn int(&self) -> Result<i64, Vec<FieldError>> {
            match self {
                DataTree::Int(n) => Ok(*n),
                // D-SERDE2: a hand `decode` runs inside the typed walk, so the
                // lexical `Number`/`TypedText` carriers reach these accessors.
                // Project them here exactly as the `decode_*` helpers do for
                // derived codecs — one protocol for hand and derived codecs.
                DataTree::Number(text) => crate::jet_json_number::json_exact_integer_text(text)
                    .ok()
                    .and_then(|digits| jet_int_from_str(&digits).ok())
                    .ok_or_else(|| {
                        FieldError::one(format!(
                            "expected int, got {}",
                            render_datatree_json(self, false, 0)
                        ))
                    }),
                _ => Err(FieldError::one(format!(
                    "expected int, got {}",
                    render_datatree_json(self, false, 0)
                ))),
            }
        }
        pub fn text(&self) -> Result<String, Vec<FieldError>> {
            match self {
                DataTree::Text(s) | DataTree::TypedText(s) => Ok(s.clone()),
                _ => Err(FieldError::one(format!(
                    "expected text, got {}",
                    render_datatree_json(self, false, 0)
                ))),
            }
        }
        pub fn bool(&self) -> Result<bool, Vec<FieldError>> {
            match self {
                DataTree::Bool(b) => Ok(*b),
                _ => Err(FieldError::one(format!(
                    "expected bool, got {}",
                    render_datatree_json(self, false, 0)
                ))),
            }
        }
        pub fn float(&self) -> Result<f64, Vec<FieldError>> {
            match self {
                DataTree::Float(f) => Ok(*f),
                DataTree::Int(n) => {
                    let value = jet_int_to_f64(*n);
                    if value.is_finite() {
                        Ok(value)
                    } else {
                        Err(FieldError::one("expected float, got out-of-range Int"))
                    }
                }
                DataTree::Number(text) => text
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite())
                    .ok_or_else(|| {
                        FieldError::one(format!(
                            "expected float, got {}",
                            render_datatree_json(self, false, 0)
                        ))
                    }),
                _ => Err(FieldError::one(format!(
                    "expected float, got {}",
                    render_datatree_json(self, false, 0)
                ))),
            }
        }
    }

    macro_rules! jet_datatree_decode_helpers {
        () => {
    // Primitive typed-decode rules live beside the tree and are shared by the
    // AOT Prelude and the JIT's handle marshalling adapter. The engines only
    // convert their resident representation to this tree and back.
    pub fn decode_int(t: &DataTree) -> Result<i64, Vec<FieldError>> {
        match t {
            DataTree::Int(n) => Ok(*n),
            DataTree::Float(f)
                if f.is_finite()
                    && f.fract() == 0.0
                    && *f >= i64::MIN as f64
                    && *f < i64::MAX as f64 =>
            {
                Ok(jet_int_from_i64(*f as i64))
            }
            DataTree::Number(text) => {
                let integer = crate::jet_json_number::json_exact_integer_text(text)
                    .map_err(FieldError::one)?;
                jet_int_from_str(&integer)
                    .map_err(|_| FieldError::one(format!("expected Int, found number {text}")))
            }
            DataTree::TypedText(text) => Err(FieldError::one(format!(
                "expected Int, found text {:?}",
                text
            ))),
            DataTree::Text(text) => jet_int_from_str(text.trim())
                .map_err(|_| FieldError::one(format!("expected Int, found text {:?}", text))),
            other => Err(FieldError::one(format!(
                "expected Int, found {}",
                datatree_kind_for(other)
            ))),
        }
    }

    pub fn decode_float(t: &DataTree) -> Result<f64, Vec<FieldError>> {
        match t {
            DataTree::Float(f) => Ok(*f),
            DataTree::Int(n) => {
                let value = jet_int_to_f64(*n);
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err(FieldError::one("expected float, got out-of-range Int"))
                }
            }
            DataTree::Number(text) | DataTree::Text(text) => {
                let value = text.trim().parse::<f64>().map_err(|_| {
                    FieldError::one(format!("expected Float, found text {:?}", text))
                })?;
                if value.is_finite() {
                    Ok(value)
                } else {
                    Err(FieldError::one("expected Float, found out-of-range Float"))
                }
            }
            DataTree::TypedText(text) => Err(FieldError::one(format!(
                "expected Float, found text {:?}",
                text
            ))),
            other => Err(FieldError::one(format!(
                "expected Float, found {}",
                datatree_kind_for(other)
            ))),
        }
    }

    pub fn decode_bool(t: &DataTree) -> Result<bool, Vec<FieldError>> {
        match t {
            DataTree::Bool(value) => Ok(*value),
            DataTree::TypedText(text) => Err(FieldError::one(format!(
                "expected Bool, found text {:?}",
                text
            ))),
            DataTree::Text(text) => match text.trim() {
                "true" => Ok(true),
                "false" => Ok(false),
                _ => Err(FieldError::one(format!(
                    "expected Bool, found text {:?}",
                    text
                ))),
            },
            other => Err(FieldError::one(format!(
                "expected Bool, found {}",
                datatree_kind_for(other)
            ))),
        }
    }

    pub fn decode_string(t: &DataTree) -> Result<String, Vec<FieldError>> {
        match t {
            DataTree::Text(text) | DataTree::TypedText(text) => Ok(text.clone()),
            DataTree::Number(text) => {
                Err(FieldError::one(format!("expected Text, found number {text}")))
            }
            DataTree::Int(n) => Ok(jet_int_to_string(*n)),
            DataTree::Float(f) => Ok(format!("{:?}", f)),
            DataTree::Bool(value) => Ok(value.to_string()),
            other => Err(FieldError::one(format!(
                "expected Text, found {}",
                datatree_kind_for(other)
            ))),
        }
    }

    pub fn decode_f32(t: &DataTree) -> Result<f32, Vec<FieldError>> {
        let value = match t {
            DataTree::Float(value) => *value,
            DataTree::Int(value) => jet_int_to_f64(*value),
            DataTree::Number(text) | DataTree::Text(text) => {
                text.trim().parse::<f64>().map_err(|_| {
                    FieldError::one(format!("expected F32, found text {:?}", text))
                })?
            }
            DataTree::TypedText(text) => {
                return Err(FieldError::one(format!(
                    "expected F32, found text {:?}",
                    text
                )))
            }
            other => {
                return Err(FieldError::one(format!(
                    "expected F32, found {}",
                datatree_kind_for(other)
                )))
            }
        };
        if value.is_finite() && value >= -(f32::MAX as f64) && value <= f32::MAX as f64 {
            Ok(value as f32)
        } else {
            Err(FieldError::one("expected F32, found out-of-range Float"))
        }
    }

    pub fn check_int_range(
        value: Result<i64, Vec<FieldError>>,
        lo: i64,
        hi: i64,
        type_name: &str,
    ) -> Result<i64, Vec<FieldError>> {
        let packed = value?;
        let Some(value) = jet_int_to_i64(packed) else {
            return Err(FieldError::one(format!(
                "expected {type_name}, found out-of-range Int"
            )));
        };
        if (lo..=hi).contains(&value) {
            Ok(value)
        } else {
            Err(FieldError::one(format!(
                "expected {type_name}, found out-of-range Int"
            )))
        }
    }

    pub fn check_f32_range(value: Result<f64, Vec<FieldError>>) -> Result<f64, Vec<FieldError>> {
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
