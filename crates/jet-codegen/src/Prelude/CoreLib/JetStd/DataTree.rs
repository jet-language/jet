    // ── core.encoding: format-agnostic value tree (D-SERDE2 = A) ───────────────
    // The one tree every format adapter speaks. The built-in `@[Codable]` derive
    // (D-ENC1) lowers `encode`/`decode` to walks over this; each adapter turns it
    // into / parses it from wire text. Distinct from the dynamic `Json` enum:
    // `DataTree` preserves field order (ordered `Object`) and keeps Int vs Float.
    #[derive(Clone, Debug, PartialEq)]
    pub enum DataTree {
        Null,
        Bool(bool),
        Int(i64),
        Float(f64),
        Text(String),
        Bytes(Vec<u8>),
        Array(Vec<DataTree>),
        Object(Vec<(String, DataTree)>),
    }

    // D-SERDE2 = A: the decode-side error carries a field path (`order.items[2]`)
    // and a plain reason. Encode is infallible, so no `EncodeError` is minted (I8).
    #[derive(Clone, Debug, PartialEq)]
    pub struct DecodeError {
        pub path: String,
        pub reason: String,
    }

    impl DecodeError {
        pub fn new(reason: impl Into<String>) -> DecodeError {
            DecodeError {
                path: String::new(),
                reason: reason.into(),
            }
        }
        // Prefix a child error with the field/index segment it occurred under.
        pub fn under(seg: &str, mut e: DecodeError) -> DecodeError {
            e.path = if e.path.is_empty() {
                seg.to_string()
            } else if e.path.starts_with('[') {
                format!("{}{}", seg, e.path)
            } else {
                format!("{}.{}", seg, e.path)
            };
            e
        }
    }

    impl super::JetShow for DataTree {
        fn jet_show(&self) -> String {
            render_datatree_json(self, false, 0)
        }
    }

    // D-VALIDATE1 (ratified 2026-07-12, card #506): the accumulated-validation
    // error — same shape as `DecodeError` (a field path plus a plain reason),
    // named separately because a `validate { }` block's failures are always
    // reported as a LIST (every failing rule at once), never a single
    // fail-fast error. `Type.validate(value)`, `Validate.over(s).finish()`,
    // and the `decode<T>()` auto-run all build this type.
    #[derive(Clone, Debug, PartialEq)]
    pub struct FieldError {
        pub path: String,
        pub reason: String,
    }

    // D-MIGRATE3=A / D-MIGRATE4=A: decode-time migration transparency plus the
    // runtime engine. `decode_traced<T>` sits beside `decode<T>` on every codec
    // that shares this decode machinery. Decoding a `@PublishedSchema` type
    // with `migration { }` blocks tries the current shape first; on mismatch
    // the type's generated `jet_decode_traced` override detects which
    // historical shape the data's key set matches and walks the step functions
    // forward (oldest matching version → current). Plain `decode` walks the
    // same chain silently; `decode_traced` reports it here — `migrated`,
    // `from` (the source shape's version label), and `steps` (one entry per
    // step applied, "v1->v2" style). Types without migrations keep the trait's
    // default identity path: `migrated` false, `from`/`steps` empty, no
    // per-type code emitted.
    #[derive(Clone, Debug, PartialEq)]
    pub struct MigrationStatus {
        pub migrated: bool,
        pub from: String,
        pub steps: Vec<String>,
    }

    impl MigrationStatus {
        pub fn fresh() -> MigrationStatus {
            MigrationStatus {
                migrated: false,
                from: String::new(),
                steps: Vec::new(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct DecodeResult<T> {
        pub value: T,
        pub migration: MigrationStatus,
    }

    /// D-MIGRATE4: the sorted set of top-level object keys of a `DataTree`, used
    /// by a `@PublishedSchema` type's migration chain-walker to detect which
    /// historical shape a decoded record matches. A non-object tree has no keys.
    pub fn jet_datatree_key_set(t: &DataTree) -> std::collections::BTreeSet<String> {
        match t {
            DataTree::Object(pairs) => pairs.iter().map(|(k, _)| k.clone()).collect(),
            _ => std::collections::BTreeSet::new(),
        }
    }

    // D-SERDE-ACCESS=B + D-SERDE14=A: dynamic accessor methods on DataTree. Each
    // read returns `Result<T, DecodeError>` so a `?` chain composes cleanly inside
    // a hand `decode`. `.field`/`.at` auto-fill `DecodeError.path` with the segment
    // they read (the field name, or `[index]`); the scalar readers leave `path`
    // empty (they read a leaf that has no name of its own — an enclosing `.field`
    // frames it via `DecodeError::under` when the caller propagates).
    impl DataTree {
        pub fn field(&self, name: &str) -> Result<DataTree, DecodeError> {
            match self {
                DataTree::Object(pairs) => pairs
                    .iter()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.clone())
                    .ok_or_else(|| DecodeError {
                        path: name.to_string(),
                        reason: format!("field `{}` not found", name),
                    }),
                _ => Err(DecodeError {
                    path: name.to_string(),
                    reason: format!("expected object, got {}", render_datatree_json(self, false, 0)),
                }),
            }
        }
        pub fn at(&self, i: i64) -> Result<DataTree, DecodeError> {
            match self {
                DataTree::Array(items) => {
                    let idx = if i < 0 {
                        items.len().wrapping_sub((-i) as usize)
                    } else {
                        i as usize
                    };
                    items.get(idx).cloned().ok_or_else(|| DecodeError {
                        path: format!("[{}]", i),
                        reason: format!("index {} out of bounds (len {})", i, items.len()),
                    })
                }
                _ => Err(DecodeError {
                    path: format!("[{}]", i),
                    reason: format!("expected array, got {}", render_datatree_json(self, false, 0)),
                }),
            }
        }
        pub fn int(&self) -> Result<i64, DecodeError> {
            match self {
                DataTree::Int(n) => Ok(*n),
                _ => Err(DecodeError::new(format!(
                    "expected int, got {}",
                    render_datatree_json(self, false, 0)
                ))),
            }
        }
        pub fn text(&self) -> Result<String, DecodeError> {
            match self {
                DataTree::Text(s) => Ok(s.clone()),
                _ => Err(DecodeError::new(format!(
                    "expected text, got {}",
                    render_datatree_json(self, false, 0)
                ))),
            }
        }
        pub fn bool(&self) -> Result<bool, DecodeError> {
            match self {
                DataTree::Bool(b) => Ok(*b),
                _ => Err(DecodeError::new(format!(
                    "expected bool, got {}",
                    render_datatree_json(self, false, 0)
                ))),
            }
        }
        pub fn float(&self) -> Result<f64, DecodeError> {
            match self {
                DataTree::Float(f) => Ok(*f),
                DataTree::Int(n) => Ok(*n as f64),
                _ => Err(DecodeError::new(format!(
                    "expected float, got {}",
                    render_datatree_json(self, false, 0)
                ))),
            }
        }
    }

    impl super::JetShow for DecodeError {
        fn jet_show(&self) -> String {
            if self.path.is_empty() {
                self.reason.clone()
            } else {
                format!("at `{}`: {}", self.path, self.reason)
            }
        }
    }

