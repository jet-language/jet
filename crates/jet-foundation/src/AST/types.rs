use crate::Diagnostics::Span;

/// The access capability of a parameter / argument / receiver (D-MEM1, was
/// D-CAP7/8/9/10).
///
/// Surface sigils map here: unmarked `T`→`Read`, `&T`→`Write`, `^T`→`Move`,
/// `*T`→`Raw`. S2 ("signatures can't lie"): an unmarked param is decided as
/// `Read` at parse time, period — no body-usage inference, no elevation.
/// `Raw` is only produced by the `*` sigil inside `#Unsafe` and never inferred
/// in safe code. `Share` has no surface sigil as of D-MEM1 (the `&` glyph
/// moved to `Write`) — it is unreachable from parsing and kept only for the
/// out-of-scope tier-2 storable-borrow future (see migration plan's "Out of
/// scope" section); nothing currently produces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessConvention {
    /// Shared read borrow (`&T` in Rust; scalars pass by value).
    Read,
    /// `&T`: exclusive write/edit access (mutable borrow, `&mut T`). D-MEM1 —
    /// was spelled `~T` pre-migration.
    Write,
    /// `^T`: ownership transfer / move (`T` by value).
    Move,
    /// No surface sigil under D-MEM1 (was `&T` pre-migration): the value may
    /// escape / be retained beyond the call (share). Composes with
    /// regions/arenas (D-CAP10/c130); a safe escaping handle, not a raw
    /// pointer. Reserved for a post-v1 tier; never produced by the parser.
    Share,
    /// `*T`: raw unsafe pointer/address (`#Unsafe`-only, never inferred; D-CAP9).
    Raw,
}

impl AccessConvention {
    /// The D-MEM1 prefix sigil for this resolved capability, as it appears on a
    /// public type (`&T`/`^T`/`*T`). Read — the unmarked default — emits no
    /// sigil. Used by the published-API surface so the snapshot carries the
    /// sigil the caller must honour.
    pub fn sigil(self) -> &'static str {
        match self {
            AccessConvention::Read => "",
            AccessConvention::Write => "&",
            AccessConvention::Move => "^",
            AccessConvention::Share => "&",
            AccessConvention::Raw => "*",
        }
    }
}

#[derive(Debug, Clone)]
pub enum Type {
    Int,
    Float,
    Bool,
    String,
    /// S41 (M5): Unicode scalar value.
    Char,
    List(Box<Type>),
    /// S38/D-LISTMAP-CANON1=A: keyed collection `[K: V]`.
    Map {
        key: Box<Type>,
        /// Parser-owned boundary of the key inside `[K: V]`.
        /// Synthesized map types carry `None`; diagnostics then fall back to
        /// the enclosing type span.
        key_span: Option<Span>,
        value: Box<Type>,
    },
    Shared(Box<Type>),
    /// S32: `T?` optional value.
    Option(Box<Type>),
    /// S34: `T ? E` fallible return. Internally lowered through Rust `Result<T, E>`.
    Result {
        ok: Box<Type>,
        err: Box<Type>,
    },
    /// S47 (M8): function type `fn(T1, T2) -> R` (`ret` omitted = no return value).
    ///
    /// D-EFF2 (callback param effect bound): an optional effect annotation may
    /// ride the *front* of a function type — `@Pure fn(T) -> U` (the callback
    /// must be pure) or `#(Net) fn(T) -> U` (the callback may use at most the
    /// listed effects). `effect_bound` is `None` when unannotated, `Some(empty)`
    /// for `@Pure`, and `Some([(name, span), …])` for `#(…)`. Names are validated
    /// against the effect vocabulary in sema, not the parser. The bound is a
    /// call-site obligation on whatever callback is passed (E0747) — it is **not**
    /// part of structural type identity (see the manual `PartialEq for Type`,
    /// which ignores it in the `Fn` arm), so `@Pure fn(Int)` and `fn(Int)` are the
    /// same type for assignability; the bound is an *extra* check, not a subtype.
    Fn {
        params: Vec<Type>,
        ret: Option<Box<Type>>,
        effect_bound: Option<Vec<(String, Span)>>,
    },
    /// User-defined monomorphic type name.
    Named(String),
    /// S45 (M9): generic application — `Pair<Int>`, `Stack<T>`.
    Apply {
        name: String,
        args: Vec<Type>,
    },
    /// S48 (M9): trait object — dynamic dispatch with invisible boxing.
    /// D-ANY-JAI1/D-VARARGBOUND1: a trait-bounded variadic loop element
    /// (`...[A, B]`) types its body binding as a multi-name `TraitObject` so
    /// method dispatch and interpolation can check EVERY bound trait, not
    /// just the first — codegen never constructs (or sees) more than one
    /// name here; it always synthesizes a real generic type param with all
    /// bounds instead (`Codegen/VariadicBound.rs`), so every codegen-side
    /// `TraitObject` match arm still only ever handles a singleton list.
    TraitObject(Vec<String>),
    /// S73 (D-SG7): named tuple `(x: Int, y: Int)` — fields stored sorted by name.
    Tuple(Vec<(String, Box<Type>)>),
    /// S76 (2026-06-16): fixed-size list `[T#N]` — a compile-time refinement of
    /// `[T]` with a statically-known length. Erases to `Vec<T>` at codegen (I3).
    FixedList {
        elem: Box<Type>,
        len: u64,
        /// Generic-module value parameter retained until specialization.
        len_symbol: Option<(String, Span)>,
    },
    /// D-SG9/S42: explicit fixed-width integer. The default 64-bit *signed*
    /// integer is spelled `Int` (and equivalently `I64`) and lives in
    /// `Type::Int`, so it never appears here — `I64` canonicalises to
    /// `Type::Int` at parse time. Every other width is an `IntN`: `bits` ∈
    /// {8,16,32,64}, and `(signed: true, bits: 64)` is excluded by construction
    /// because that *is* `Int`. So `U8` = `{signed:false, bits:8}`,
    /// `U64` = `{signed:false, bits:64}`, `I32` = `{signed:true, bits:32}`.
    IntN {
        signed: bool,
        bits: u8,
    },
    /// D-SG9/S42: 32-bit float. The default 64-bit float is spelled `Float`
    /// (and `F64`) and lives in `Type::Float`; only `F32` is a `Float32`.
    Float32,
    /// D-QUAL4=A: value-tag type qualifier — `#Marker T` in signature/binding
    /// position. Transparent to type identity (the tag is a flow annotation only,
    /// not a structural difference); sema treats it as `inner` for all purposes.
    Tagged {
        marker: String,
        inner: Box<Type>,
    },
}

/// Manual structural equality (D-EFF2). Identical to a derived `PartialEq`
/// except the `Fn` arm ignores `effect_bound`: a callback effect bound is a
/// call-site obligation, not part of a function type's identity, so a
/// `@Pure fn(Int)` value is assignable wherever a `fn(Int)` is expected. The
/// bound is enforced separately at the call site (E0747).
impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        use Type::*;
        match (self, other) {
            (Int, Int)
            | (Float, Float)
            | (Bool, Bool)
            | (String, String)
            | (Char, Char)
            | (Float32, Float32) => true,
            (List(a), List(b)) => a == b,
            (
                Map {
                    key: k1,
                    value: v1,
                    ..
                },
                Map {
                    key: k2,
                    value: v2,
                    ..
                },
            ) => k1 == k2 && v1 == v2,
            (Shared(a), Shared(b)) => a == b,
            (Option(a), Option(b)) => a == b,
            (Result { ok: o1, err: e1 }, Result { ok: o2, err: e2 }) => o1 == o2 && e1 == e2,
            // D-EFF2: effect_bound deliberately excluded from the comparison.
            (
                Fn {
                    params: p1,
                    ret: r1,
                    ..
                },
                Fn {
                    params: p2,
                    ret: r2,
                    ..
                },
            ) => p1 == p2 && r1 == r2,
            (Named(a), Named(b)) => a == b,
            (Apply { name: n1, args: a1 }, Apply { name: n2, args: a2 }) => n1 == n2 && a1 == a2,
            (TraitObject(a), TraitObject(b)) => a == b,
            (Tuple(a), Tuple(b)) => a == b,
            (FixedList { elem: e1, len: l1, len_symbol: s1 }, FixedList { elem: e2, len: l2, len_symbol: s2 }) => {
                e1 == e2 && l1 == l2 && s1 == s2
            }
            (
                IntN {
                    signed: s1,
                    bits: b1,
                },
                IntN {
                    signed: s2,
                    bits: b2,
                },
            ) => s1 == s2 && b1 == b2,
            // D-QUAL4: tagged types are transparent — identity is on the inner type.
            (Tagged { inner: a, .. }, Tagged { inner: b, .. }) => a == b,
            (Tagged { inner, .. }, other) | (other, Tagged { inner, .. }) => {
                inner.as_ref() == other
            }
            _ => false,
        }
    }
}

impl Eq for Type {}

/// D-SG9: the spelling of a fixed-width integer (`U8`, `I32`, …).
pub fn int_spelling(signed: bool, bits: u8) -> String {
    format!("{}{}", if signed { 'I' } else { 'U' }, bits)
}

/// D-SG9: parse a numeric type spelling to its `Type` — `Int`/`Float` and the
/// fixed widths, with `I64`/`F64` folding to the 64-bit defaults. `None` for any
/// non-numeric name. Inverse of `Type::name` for the numeric types.
pub fn numeric_type_from_name(name: &str) -> Option<Type> {
    match name {
        "Int" | "I64" => Some(Type::Int),
        "Float" | "F64" => Some(Type::Float),
        "F32" => Some(Type::Float32),
        _ => {
            let signed = name.starts_with('I');
            if !(signed || name.starts_with('U')) || name.len() < 2 {
                return None;
            }
            let bits: u8 = name[1..].parse().ok()?;
            match bits {
                8 | 16 | 32 => Some(Type::IntN { signed, bits }),
                64 if !signed => Some(Type::IntN {
                    signed: false,
                    bits: 64,
                }),
                _ => None,
            }
        }
    }
}

/// D-SG9: inclusive `(min, max)` value range of a fixed-width integer, used for
/// literal-fits checks. `i128` holds every Jet integer width exactly.
pub fn int_range(signed: bool, bits: u8) -> (i128, i128) {
    if signed {
        let max = (1i128 << (bits - 1)) - 1;
        (-(max + 1), max)
    } else {
        ((0i128), (1i128 << bits) - 1)
    }
}

/// S73: sort tuple fields by name so type identity ignores source order.
pub fn canonicalize_tuple_fields<T>(mut fields: Vec<(String, T)>) -> Vec<(String, T)> {
    fields.sort_by(|a, b| a.0.cmp(&b.0));
    fields
}

impl Type {
    /// Plain-words name for diagnostics (docs/spec/diagnostics.md voice: name both types).
    pub fn show(&self) -> String {
        match self {
            Type::Int => "Int (a whole number)".to_string(),
            Type::Float => "Float (a decimal number)".to_string(),
            Type::Bool => "Bool (true or false)".to_string(),
            Type::String => "String (text)".to_string(),
            Type::Char => "Char (one character)".to_string(),
            Type::List(inner) => format!("[{}]", inner.name()),
            Type::Map { key, value, .. } => format!("[{}: {}]", key.name(), value.name()),
            Type::Shared(inner) => format!("Shared<{}>", inner.name()),
            Type::Option(inner) => format!("{}?", inner.name()),
            Type::Result { ok, err } => format!("{} ? {}", ok.name(), err.name()),
            Type::Fn { params, ret, .. } => {
                let ps = params
                    .iter()
                    .map(|p| p.name())
                    .collect::<Vec<_>>()
                    .join(", ");
                match ret {
                    Some(r) => format!("fn({}) -> {}", ps, r.name()),
                    None => format!("fn({})", ps),
                }
            }
            Type::Named(n) => format!("`{}`", n),
            // D-CAP9: the raw-pointer type shows as the canonical `*T`.
            Type::Apply { name, args } if name == crate::Syntax::TYPE_PTR && args.len() == 1 => {
                format!("`*{}`", args[0].name())
            }
            Type::Apply { name, args } => {
                let a = args.iter().map(|x| x.name()).collect::<Vec<_>>().join(", ");
                format!("`{}`<{}>", name, a)
            }
            Type::TraitObject(t) => format!("`{}` (a trait value)", t.join(" + ")),
            Type::Tuple(fields) => {
                let parts = fields
                    .iter()
                    .map(|(n, t)| format!("{}: {}", n, t.name()))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({parts})")
            }
            Type::FixedList { elem, len, len_symbol } => format!("[{}#{}]", elem.name(), len_symbol.as_ref().map(|v| v.0.as_str()).map_or_else(|| len.to_string(), str::to_string)),
            Type::IntN { signed, bits } => {
                let (lo, hi) = int_range(*signed, *bits);
                let article = if *bits == 8 { "an" } else { "a" };
                format!(
                    "{} ({} {}-bit whole number, {} to {})",
                    int_spelling(*signed, *bits),
                    article,
                    bits,
                    lo,
                    hi
                )
            }
            Type::Float32 => "F32 (a 32-bit decimal number)".to_string(),
            Type::Tagged { marker, inner } => format!("#{} {}", marker, inner.show()),
        }
    }

    /// Bare type name, no gloss.
    pub fn name(&self) -> String {
        match self {
            Type::Int => "Int".to_string(),
            Type::Float => "Float".to_string(),
            Type::Bool => "Bool".to_string(),
            Type::String => "String".to_string(),
            Type::Char => "Char".to_string(),
            Type::List(inner) => format!("[{}]", inner.name()),
            Type::Map { key, value, .. } => format!("[{}: {}]", key.name(), value.name()),
            Type::Shared(inner) => format!("Shared<{}>", inner.name()),
            Type::Option(inner) => format!("{}?", inner.name()),
            Type::Result { ok, err } => format!("{} ? {}", ok.name(), err.name()),
            Type::Fn { params, ret, .. } => {
                let ps = params
                    .iter()
                    .map(|p| p.name())
                    .collect::<Vec<_>>()
                    .join(", ");
                match ret {
                    Some(r) => format!("fn({}) -> {}", ps, r.name()),
                    None => format!("fn({})", ps),
                }
            }
            Type::Named(n) => n.clone(),
            // D-CAP9: the raw-pointer type names as the canonical `*T`.
            Type::Apply { name, args } if name == crate::Syntax::TYPE_PTR && args.len() == 1 => {
                format!("*{}", args[0].name())
            }
            Type::Apply { name, args } => {
                let a = args.iter().map(|x| x.name()).collect::<Vec<_>>().join(", ");
                format!("{}<{}>", name, a)
            }
            Type::TraitObject(t) => t.join(" + "),
            Type::Tuple(fields) => {
                let parts = fields
                    .iter()
                    .map(|(n, t)| format!("{}: {}", n, t.name()))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("({parts})")
            }
            Type::FixedList { elem, len, len_symbol } => format!("[{}#{}]", elem.name(), len_symbol.as_ref().map(|v| v.0.as_str()).map_or_else(|| len.to_string(), str::to_string)),
            Type::IntN { signed, bits } => int_spelling(*signed, *bits),
            Type::Float32 => "F32".to_string(),
            Type::Tagged { marker, inner } => format!("#{} {}", marker, inner.name()),
        }
    }

    /// Base name for struct/enum/trait references (without generic args).
    pub fn base_name(&self) -> Option<&str> {
        match self {
            Type::Named(n) => Some(n.as_str()),
            Type::Apply { name, .. } => Some(name.as_str()),
            Type::TraitObject(t) => t.first().map(String::as_str),
            _ => None,
        }
    }

    pub fn is_scalar(&self) -> bool {
        match self {
            Type::Tagged { inner, .. } => inner.is_scalar(),
            Type::Apply { name, args }
                if name == crate::Syntax::TYPE_PTR && args.len() == 1 => true,
            _ => matches!(
                self,
                Type::Int | Type::Float | Type::Bool | Type::IntN { .. } | Type::Float32
            ),
        }
    }

    /// D-SG9: any integer type — the default `Int` or an explicit fixed width.
    pub fn is_integer(&self) -> bool {
        match self {
            Type::Tagged { inner, .. } => inner.is_integer(),
            _ => matches!(self, Type::Int | Type::IntN { .. }),
        }
    }

    /// D-SG9/D-FLOATW1: any float type — the default `Float` or `F32`.
    pub fn is_float(&self) -> bool {
        match self {
            Type::Tagged { inner, .. } => inner.is_float(),
            _ => matches!(self, Type::Float | Type::Float32),
        }
    }

    /// D-SG9: any numeric type (integer or float).
    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    pub fn unwrap_option(&self) -> Option<&Type> {
        match self {
            Type::Option(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn unwrap_result(&self) -> Option<(&Type, &Type)> {
        match self {
            Type::Result { ok, err } => Some((ok, err)),
            _ => None,
        }
    }

    pub fn is_fallible(&self) -> bool {
        matches!(self, Type::Option(_) | Type::Result { .. })
    }
}
