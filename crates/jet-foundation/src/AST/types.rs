use crate::Diagnostics::Span;

/// D-DIMENSION-OPEN1=D: a normalized open physical dimension.
///
/// Keys are nominal base-dimension identities. Entries stay sorted and zero
/// exponents are removed, so equality and serialized API identity are stable.
/// Runtime values carry no dimension metadata.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Dimension(std::collections::BTreeMap<String, i32>);

impl Dimension {
    pub fn scalar() -> Self {
        Self(std::collections::BTreeMap::new())
    }

    pub fn base(identity: impl Into<String>) -> Self {
        Self(std::iter::once((identity.into(), 1)).collect())
    }

    pub fn multiply(&self, rhs: &Self) -> Option<Self> {
        self.combine(rhs, false)
    }

    pub fn divide(&self, rhs: &Self) -> Option<Self> {
        self.combine(rhs, true)
    }

    fn combine(&self, rhs: &Self, subtract: bool) -> Option<Self> {
        let mut out = self.0.clone();
        for (axis, exponent) in &rhs.0 {
            let exponent = if subtract {
                exponent.checked_neg()?
            } else {
                *exponent
            };
            let next = out
                .get(axis)
                .copied()
                .unwrap_or(0)
                .checked_add(exponent)?;
            if next == 0 {
                out.remove(axis);
            } else {
                out.insert(axis.clone(), next);
            }
        }
        Some(Self(out))
    }

    pub fn pow(&self, exponent: i32) -> Option<Self> {
        let mut out = std::collections::BTreeMap::new();
        for (axis, current) in &self.0 {
            let next = current.checked_mul(exponent)?;
            if next != 0 {
                out.insert(axis.clone(), next);
            }
        }
        Some(Self(out))
    }

    pub fn axes(&self) -> impl Iterator<Item = (&str, i32)> {
        self.0.iter().map(|(axis, exponent)| (axis.as_str(), *exponent))
    }

    /// Stable identity used by API/type serialization.
    pub fn identity(&self) -> String {
        self.0
            .iter()
            .map(|(axis, exponent)| format!("{}:{exponent}", escape_axis(axis)))
            .collect::<Vec<_>>()
            .join(";")
    }

    pub fn from_identity(identity: &str) -> Option<Self> {
        let mut axes = std::collections::BTreeMap::new();
        if identity.is_empty() {
            return Some(Self(axes));
        }
        for part in identity.split(';') {
            let (axis, exponent) = part.rsplit_once(':')?;
            let exponent = exponent.parse().ok()?;
            if exponent == 0 || axes.insert(unescape_axis(axis)?, exponent).is_some() {
                return None;
            }
        }
        Some(Self(axes))
    }

    pub fn display_name(&self) -> String {
        let mut parts = Vec::new();
        for (axis, exponent) in &self.0 {
            let name = axis.rsplit("::").next().unwrap_or(axis);
            parts.push(if *exponent == 1 {
                name.to_string()
            } else {
                format!("{name}^{exponent}")
            });
        }
        if parts.is_empty() {
            "Scalar".to_string()
        } else {
            parts.join(" * ")
        }
    }
}

fn escape_axis(axis: &str) -> String {
    axis.replace('%', "%25").replace(';', "%3B").replace(':', "%3A")
}

fn unescape_axis(axis: &str) -> Option<String> {
    let bytes = axis.as_bytes();
    let mut out = String::with_capacity(axis.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            out.push(bytes[index] as char);
            index += 1;
            continue;
        }
        let code = axis.get(index + 1..index + 3)?;
        out.push(match code {
            "25" => '%',
            "3B" => ';',
            "3A" => ':',
            _ => return None,
        });
        index += 3;
    }
    Some(out)
}

/// Internal-only provenance tag for purpose-bound `core.crypto` nominal types.
/// The NUL prefix cannot be written as a Jet marker identifier.
pub const CORE_CRYPTO_NOMINAL_MARKER: &str = "\0core.crypto";

/// Internal provenance for deterministic and system-backed `Clock` values.
/// The NUL prefix keeps both markers unspellable in Jet source.
pub const DETERMINISTIC_CLOCK_MARKER: &str = "\0clock.deterministic";
pub const SYSTEM_CLOCK_MARKER: &str = "\0clock.system";

/// Internal provenance for the temporary read-only value lent by
/// `ExpiringSecret.with`. It cannot be named in source or stored anywhere.
pub const EXPIRING_SECRET_LOAN_MARKER: &str = "\0expiring_secret.loan";
/// Compiler-only access modes for the single public `SharedGuard<T>` type.
pub const SHARED_GUARD_READ_MARKER: &str = "\0shared_guard.read";
pub const SHARED_GUARD_EDIT_MARKER: &str = "\0shared_guard.edit";

/// Internal provenance for the open terminal capability-key set.
/// The NUL prefix keeps the marker unspellable in Jet source.
pub const TERMINAL_FACT_SET_MARKER: &str = "\0terminal.fact_set";

/// Compiler-owned representation tag used by generated C++ facade functions.
/// The tag keeps the source-level callback shape while telling the backend that
/// this parameter is already a raw C function pointer, not a boxed Jet closure.
pub const CPP_CALLBACK_ABI_MARKER: &str = "\0cpp.callback_abi";

/// The access capability of a parameter / argument / receiver (D-MEM1, was
/// D-CAP7/8/9/10).
///
/// Surface sigils map here: unmarked `T`→`Read`, `&T`→`Write`, `^T`→`Move`.
/// S2 ("signatures can't lie"): an unmarked param is decided as
/// `Read` at parse time, period — no body-usage inference, no elevation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessConvention {
    /// Shared read borrow (`&T` in Rust; scalars pass by value).
    Read,
    /// `&T`: exclusive write/edit access (mutable borrow, `&mut T`). D-MEM1 —
    /// was spelled `~T` pre-migration.
    Write,
    /// `^T`: ownership transfer / move (`T` by value).
    Move,
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
    /// S47 (M8): function type `fn(T1, T2) => R` (`ret` omitted = no return value).
    ///
    /// D-EFF2 / D-SHAPE8: an optional effect row lives inside the function
    /// type's callable arrow — `fn(T) =[]=> U` requires purity and
    /// `fn(T) =[Net]=> U` permits at most the listed effects. `effect_bound`
    /// is `None` when unannotated, `Some(empty)` for `=[]=>`, and
    /// `Some([(name, span), …])` for a nonempty row. Names are validated
    /// against the effect vocabulary in sema, not the parser. The bound is a
    /// call-site obligation on whatever callback is passed (E0747) — it is **not**
    /// part of structural type identity (see the manual `PartialEq for Type`,
    /// which ignores it in the `Fn` arm), so `fn(Int) =[]=>` and `fn(Int)` are the
    /// same type for assignability; the bound is an *extra* check, not a subtype.
    Fn {
        params: Vec<Type>,
        ret: Option<Box<Type>>,
        effect_bound: Option<Vec<(String, Span)>>,
        /// Relation from returned view slots to possible parameter owners.
        /// D-MEMPROVENANCE3=A: a trailing `from` on the function type fills this
        /// at parse time (names resolve then and are not kept on the type).
        /// When absent, sema conservatively freezes every non-scalar argument.
        return_view_provenance: Option<super::ViewProvenanceMap>,
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
    /// `[T]` with a statically-known length. Lowers to inline Rust `[T; N]`.
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
    /// D-QUAL4=A: value-tag type qualifier — `#TagName T` in signature/binding
    /// position. Transparent to type identity (the tag is a flow annotation only,
    /// not a structural difference); sema treats it as `inner` for all purposes.
    Tagged {
        marker: String,
        inner: Box<Type>,
    },
    /// D-UNIONTYPE1=A: closed structural sum `A | B | …`. Canonical form is
    /// flattened, duplicate-free, and sorted by member `name()` spelling so
    /// identity is order-insensitive. Desugars to one compiler-generated enum
    /// whose arms are named by the member types.
    Union(Vec<Type>),
}

/// Manual structural equality (D-EFF2). Identical to a derived `PartialEq`
/// except the `Fn` arm ignores `effect_bound`: a callback effect bound is a
/// call-site obligation, not part of a function type's identity, so a
/// `fn(Int) =[]=>` value is assignable wherever a `fn(Int)` is expected. The
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
            // Internal core nominal provenance is identity-bearing. User-written
            // D-QUAL4 tags remain transparent flow annotations.
            (Tagged { marker: ma, inner: a }, Tagged { marker: mb, inner: b })
                if ma == CORE_CRYPTO_NOMINAL_MARKER && mb == CORE_CRYPTO_NOMINAL_MARKER =>
            {
                a == b
            }
            (Tagged { marker, inner }, other) if marker != CORE_CRYPTO_NOMINAL_MARKER => {
                inner.as_ref() == other
            }
            (other, Tagged { marker, inner }) if marker != CORE_CRYPTO_NOMINAL_MARKER => {
                other == inner.as_ref()
            }
            (Tagged { marker, .. }, _) | (_, Tagged { marker, .. })
                if marker == CORE_CRYPTO_NOMINAL_MARKER => false,
            (Union(a), Union(b)) => a == b,
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

/// D-UNIONTYPE1=A: flatten nested unions, drop exact duplicates, sort by
/// canonical source spelling. A single surviving member collapses to that
/// member (so `Int | Int` is just `Int`).
pub fn canonicalize_union(members: Vec<Type>) -> Type {
    let mut flat: Vec<Type> = Vec::new();
    fn push_flat(out: &mut Vec<Type>, ty: Type) {
        match ty {
            Type::Union(inner) => {
                for m in inner {
                    push_flat(out, m);
                }
            }
            other => out.push(other),
        }
    }
    for m in members {
        push_flat(&mut flat, m);
    }
    let mut unique: Vec<Type> = Vec::new();
    for m in flat {
        if !unique.iter().any(|u| u == &m) {
            unique.push(m);
        }
    }
    unique.sort_by(|a, b| a.name().cmp(&b.name()));
    match unique.len() {
        0 => Type::Named("Void".to_string()),
        1 => unique.pop().unwrap(),
        _ => Type::Union(unique),
    }
}

/// D-UNIONTYPE1=A: stable arm / Rust-variant tag for one union member.
/// Builtins and named types keep their source spelling; compound types are
/// sanitized so the generated enum stays a valid identifier.
pub fn union_member_tag(ty: &Type) -> String {
    let raw = ty.name();
    if raw.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !raw.is_empty() {
        return raw;
    }
    let mut out = String::with_capacity(raw.len() + 4);
    out.push_str("M_");
    for c in raw.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

/// D-UNIONTYPE1=A: deterministic compiler-generated enum name for a canonical
/// union. Same members ⇒ same name regardless of source order.
pub fn union_enum_name(members: &[Type]) -> String {
    let tags: Vec<String> = members.iter().map(union_member_tag).collect();
    format!("__JetUnion_{}", tags.join("_"))
}

fn effect_names(row: &[(String, Span)]) -> String {
    row.iter().map(|(name, _)| name.as_str()).collect::<Vec<_>>().join(", ")
}

impl Type {
    /// Compiler-private fact marker on an implicit checked integer-to-float
    /// conversion. Sema writes it; lowering consumes it without reconstructing
    /// the language rule.
    pub const CHECKED_NUMERIC_WIDEN_MARKER: &'static str = "\0numeric.checked_widen";
    /// Compiler-private call name left by `approx(value)` until its surrounding
    /// numeric widening consumes the explicit loss opt-out.
    pub const APPROX_NUMERIC_WIDEN_MARKER: &'static str = "\0numeric.approx_widen";

    /// Recursively rewrite nominal leaves while preserving every container and
    /// callback shape. Import resolution uses this to attach module identity to
    /// unit-family members in signatures.
    pub fn map_named_types(&self, map: &impl Fn(&str) -> Option<String>) -> Type {
        match self {
            Type::Named(name) => map(name).map_or_else(|| self.clone(), Type::Named),
            Type::List(inner) => Type::List(Box::new(inner.map_named_types(map))),
            Type::Map { key, key_span, value } => Type::Map {
                key: Box::new(key.map_named_types(map)),
                key_span: *key_span,
                value: Box::new(value.map_named_types(map)),
            },
            Type::Shared(inner) => Type::Shared(Box::new(inner.map_named_types(map))),
            Type::Option(inner) => Type::Option(Box::new(inner.map_named_types(map))),
            Type::Result { ok, err } => Type::Result {
                ok: Box::new(ok.map_named_types(map)),
                err: Box::new(err.map_named_types(map)),
            },
            Type::Fn { params, ret, effect_bound, return_view_provenance } => Type::Fn {
                params: params.iter().map(|ty| ty.map_named_types(map)).collect(),
                ret: ret.as_ref().map(|ty| Box::new(ty.map_named_types(map))),
                effect_bound: effect_bound.clone(),
                return_view_provenance: return_view_provenance.clone(),
            },
            Type::Apply { name, args } => Type::Apply {
                name: name.clone(),
                args: args.iter().map(|ty| ty.map_named_types(map)).collect(),
            },
            Type::Tuple(fields) => Type::Tuple(
                fields.iter().map(|(name, ty)| (name.clone(), Box::new(ty.map_named_types(map)))).collect(),
            ),
            Type::FixedList { elem, len, len_symbol } => Type::FixedList {
                elem: Box::new(elem.map_named_types(map)),
                len: *len,
                len_symbol: len_symbol.clone(),
            },
            Type::Tagged { marker, inner } => Type::Tagged {
                marker: marker.clone(),
                inner: Box::new(inner.map_named_types(map)),
            },
            Type::Union(members) => canonicalize_union(
                members.iter().map(|m| m.map_named_types(map)).collect(),
            ),
            other => other.clone(),
        }
    }

    pub fn quantity(base: Type, dimension: Dimension) -> Type {
        Type::Apply {
            name: crate::Syntax::TYPE_QUANTITY.to_string(),
            args: vec![base, Type::Named(dimension.identity())],
        }
    }

    pub fn quantity_parts(&self) -> Option<(&Type, Dimension)> {
        match self {
            Type::Apply { name, args }
                if name == crate::Syntax::TYPE_QUANTITY && args.len() == 2 =>
            {
                let Type::Named(identity) = &args[1] else {
                    return None;
                };
                Some((&args[0], Dimension::from_identity(identity)?))
            }
            _ => None,
        }
    }

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
            Type::Fn { params, ret, effect_bound, .. } => {
                let ps = params
                    .iter()
                    .map(|p| p.name())
                    .collect::<Vec<_>>()
                    .join(", ");
                match (effect_bound, ret) {
                    (Some(row), Some(r)) => format!("fn({}) =[{}]=> {}", ps, effect_names(row), r.name()),
                    (Some(row), None) => format!("fn({}) =[{}]=>", ps, effect_names(row)),
                    (None, Some(r)) => format!("fn({}) => {}", ps, r.name()),
                    (None, None) => format!("fn({})", ps),
                }
            }
            Type::Named(n) => format!("`{}`", n),
            // D-CAP9: the raw-pointer type shows as the canonical `*T`.
            Type::Apply { name, args } if name == crate::Syntax::TYPE_PTR && args.len() == 1 => {
                format!("`*{}`", args[0].name())
            }
            Type::Apply { .. } if self.quantity_parts().is_some() => {
                let (_, dimension) = self.quantity_parts().unwrap();
                format!("{} (a physical quantity)", dimension.display_name())
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
            Type::Tagged { marker, inner }
                if matches!(
                    marker.as_str(),
                    CORE_CRYPTO_NOMINAL_MARKER
                        | DETERMINISTIC_CLOCK_MARKER
                        | EXPIRING_SECRET_LOAN_MARKER
                        | SHARED_GUARD_READ_MARKER
                        | SHARED_GUARD_EDIT_MARKER
                        | SYSTEM_CLOCK_MARKER
                        | TERMINAL_FACT_SET_MARKER
                ) =>
            {
                inner.show()
            }
            Type::Tagged { marker, inner } => format!("#{} {}", marker, inner.show()),
            Type::Union(members) => members
                .iter()
                .map(|m| m.name())
                .collect::<Vec<_>>()
                .join(" | "),
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
            Type::Fn { params, ret, effect_bound, .. } => {
                let ps = params
                    .iter()
                    .map(|p| p.name())
                    .collect::<Vec<_>>()
                    .join(", ");
                match (effect_bound, ret) {
                    (Some(row), Some(r)) => format!("fn({}) =[{}]=> {}", ps, effect_names(row), r.name()),
                    (Some(row), None) => format!("fn({}) =[{}]=>", ps, effect_names(row)),
                    (None, Some(r)) => format!("fn({}) => {}", ps, r.name()),
                    (None, None) => format!("fn({})", ps),
                }
            }
            Type::Named(n) => n.clone(),
            // D-CAP9: the raw-pointer type names as the canonical `*T`.
            Type::Apply { name, args } if name == crate::Syntax::TYPE_PTR && args.len() == 1 => {
                format!("*{}", args[0].name())
            }
            Type::Apply { .. } if self.quantity_parts().is_some() => {
                let (base, dimension) = self.quantity_parts().unwrap();
                format!(
                    "Quantity<{}, {}; {}>",
                    dimension.display_name(),
                    base.name(),
                    dimension.identity()
                )
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
            Type::Tagged { marker, inner }
                if matches!(
                    marker.as_str(),
                    CORE_CRYPTO_NOMINAL_MARKER
                        | DETERMINISTIC_CLOCK_MARKER
                        | EXPIRING_SECRET_LOAN_MARKER
                        | SHARED_GUARD_READ_MARKER
                        | SHARED_GUARD_EDIT_MARKER
                        | SYSTEM_CLOCK_MARKER
                        | TERMINAL_FACT_SET_MARKER
                ) =>
            {
                inner.name()
            }
            Type::Tagged { marker, inner } => format!("#{} {}", marker, inner.name()),
            Type::Union(members) => members
                .iter()
                .map(|m| m.name())
                .collect::<Vec<_>>()
                .join(" | "),
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
        if self.quantity_parts().is_some() {
            return true;
        }
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
        if let Some((base, _)) = self.quantity_parts() {
            return base.is_float();
        }
        match self {
            Type::Tagged { inner, .. } => inner.is_float(),
            _ => matches!(self, Type::Float | Type::Float32),
        }
    }

    /// D-SG9: any numeric type (integer or float).
    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    /// D-INTLIT-WIDTH1=F / D-VERDICT-1304-1 / D-NUMWIDEN-CROSS1=E:
    /// classify an implicit numeric move into `target`.
    /// The returned bool is true only when the crossing needs a runtime check.
    pub fn numeric_widening_to(&self, target: &Type) -> Option<bool> {
        if self == target && self.is_numeric() {
            return Some(false);
        }

        let integer = |ty: &Type| match ty {
            Type::Int => Some((true, 64)),
            Type::IntN { signed, bits } => Some((*signed, *bits)),
            _ => None,
        };

        if let (Some((source_signed, source_bits)), Some((target_signed, target_bits))) =
            (integer(self), integer(target))
        {
            let (source_min, source_max) = int_range(source_signed, source_bits);
            let (target_min, target_max) = int_range(target_signed, target_bits);
            return (target_min <= source_min && source_max <= target_max).then_some(false);
        }

        match (self, target) {
            (Type::Float32, Type::Float) => Some(false),
            (source, Type::Float | Type::Float32) if source.is_integer() => {
                let (signed, bits) = integer(source)?;
                let precision = if matches!(target, Type::Float32) {
                    24
                } else {
                    53
                };
                let exact = if signed {
                    bits <= precision + 1
                } else {
                    bits <= precision
                };
                Some(!exact)
            }
            _ => None,
        }
    }

    /// Numeric expression join. One operand must itself be the wider target;
    /// Jet does not search for a third numeric type that could hold both.
    pub fn numeric_join(&self, other: &Type) -> Option<Type> {
        if self.numeric_widening_to(other).is_some() {
            Some(other.clone())
        } else if other.numeric_widening_to(self).is_some() {
            Some(self.clone())
        } else {
            None
        }
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

    /// D-UNIONTYPE1=A: members of a canonical union, if any.
    pub fn unwrap_union(&self) -> Option<&[Type]> {
        match self {
            Type::Union(members) => Some(members.as_slice()),
            _ => None,
        }
    }

    /// D-UNIONTYPE1=A: true when `member` is exactly one arm of this union.
    pub fn union_contains(&self, member: &Type) -> bool {
        match self {
            Type::Union(members) => members.iter().any(|m| m == member),
            _ => false,
        }
    }

    pub fn is_fallible(&self) -> bool {
        matches!(self, Type::Option(_) | Type::Result { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::{numeric_type_from_name, Dimension, Type, CORE_CRYPTO_NOMINAL_MARKER};

    fn core_secret() -> Type {
        Type::Tagged {
            marker: CORE_CRYPTO_NOMINAL_MARKER.to_string(),
            inner: Box::new(Type::Named("Secret".to_string())),
        }
    }

    #[test]
    fn canonicalize_union_flattens_dedupes_and_sorts() {
        use super::canonicalize_union;
        let u = canonicalize_union(vec![
            Type::String,
            Type::Union(vec![Type::Int, Type::String]),
            Type::Int,
        ]);
        assert_eq!(u, Type::Union(vec![Type::Int, Type::String]));
        let collapsed = canonicalize_union(vec![Type::Int, Type::Int]);
        assert_eq!(collapsed, Type::Int);
        let flipped = canonicalize_union(vec![Type::String, Type::Int]);
        assert_eq!(flipped, Type::Union(vec![Type::Int, Type::String]));
    }

    #[test]
    fn core_crypto_nominal_provenance_is_identity_bearing_but_flow_tags_stay_transparent() {
        let local = Type::Named("Secret".to_string());
        let core = core_secret();
        let tainted_core = Type::Tagged {
            marker: "Tainted(Credential)".to_string(),
            inner: Box::new(core.clone()),
        };

        assert_ne!(core, local);
        assert_eq!(tainted_core, core);
    }

    #[test]
    fn physical_dimensions_normalize_and_serialize_stably() {
        let mass = Dimension::base("pkg::Mass");
        let length = Dimension::base("pkg::Length");
        let time = Dimension::base("pkg::Time");
        let force = mass
            .multiply(&length)
            .unwrap()
            .divide(&time)
            .unwrap()
            .divide(&time)
            .unwrap();
        assert_eq!(
            force.identity(),
            "pkg%3A%3ALength:1;pkg%3A%3AMass:1;pkg%3A%3ATime:-2"
        );
        assert_eq!(Dimension::from_identity(&force.identity()), Some(force));
        let max = Dimension::from_identity("pkg%3A%3ALength:2147483647").unwrap();
        assert_eq!(max.multiply(&length), None);
    }

    #[test]
    fn imported_unit_identity_maps_through_containers_and_callbacks() {
        let unit = Type::Named("Unit".into());
        let nested = Type::Fn {
            params: vec![
                Type::List(Box::new(unit.clone())),
                Type::Map {
                    key: Box::new(unit.clone()),
                    key_span: None,
                    value: Box::new(Type::Option(Box::new(unit.clone()))),
                },
                Type::Tuple(vec![("value".into(), Box::new(unit.clone()))]),
                Type::Apply { name: "Box".into(), args: vec![unit.clone()] },
            ],
            ret: Some(Box::new(Type::Result {
                ok: Box::new(unit),
                err: Box::new(Type::String),
            })),
            effect_bound: None,
        };
        let length = nested.map_named_types(&|name| (name == "Unit").then(|| "length.Unit".into()));
        let time = nested.map_named_types(&|name| (name == "Unit").then(|| "time.Unit".into()));
        assert_ne!(length, time);
        assert!(length.name().contains("length.Unit"));
        assert!(time.name().contains("time.Unit"));
    }

    #[test]
    fn numeric_widening_uses_value_containment_and_marks_checked_crossings() {
        let i8 = Type::IntN {
            signed: true,
            bits: 8,
        };
        let i16 = Type::IntN {
            signed: true,
            bits: 16,
        };
        let i32 = Type::IntN {
            signed: true,
            bits: 32,
        };
        let u8 = Type::IntN {
            signed: false,
            bits: 8,
        };
        let u32 = Type::IntN {
            signed: false,
            bits: 32,
        };
        let u64 = Type::IntN {
            signed: false,
            bits: 64,
        };

        assert_eq!(i8.numeric_widening_to(&i16), Some(false));
        assert_eq!(u8.numeric_widening_to(&i16), Some(false));
        assert_eq!(i16.numeric_widening_to(&u32), None);
        assert_eq!(u64.numeric_widening_to(&Type::Int), None);
        assert_eq!(Type::Float32.numeric_widening_to(&Type::Float), Some(false));
        assert_eq!(Type::Float.numeric_widening_to(&Type::Float32), None);

        assert_eq!(i32.numeric_widening_to(&Type::Float), Some(false));
        assert_eq!(Type::Int.numeric_widening_to(&Type::Float), Some(true));
        assert_eq!(i16.numeric_widening_to(&Type::Float32), Some(false));
        assert_eq!(i32.numeric_widening_to(&Type::Float32), Some(true));
        assert_eq!(u64.numeric_widening_to(&Type::Float32), Some(true));

        assert_eq!(u8.numeric_join(&i8), None);
        assert_eq!(i8.numeric_join(&i16), Some(i16));
        assert_eq!(Type::Int.numeric_join(&Type::Float), Some(Type::Float));
    }

    #[test]
    fn numeric_widening_covers_the_ratified_integer_float_matrix() {
        let ty = |name| numeric_type_from_name(name).unwrap();
        let integers = ["I8", "I16", "I32", "Int", "U8", "U16", "U32", "U64"];

        for source in integers {
            for target in integers {
                let source_ty = ty(source);
                let target_ty = ty(target);
                let (source_signed, source_bits) = match source_ty {
                    Type::Int => (true, 64),
                    Type::IntN { signed, bits } => (signed, bits),
                    _ => unreachable!(),
                };
                let (target_signed, target_bits) = match target_ty {
                    Type::Int => (true, 64),
                    Type::IntN { signed, bits } => (signed, bits),
                    _ => unreachable!(),
                };
                let (source_min, source_max) = super::int_range(source_signed, source_bits);
                let (target_min, target_max) = super::int_range(target_signed, target_bits);
                let expected =
                    (target_min <= source_min && source_max <= target_max).then_some(false);
                assert_eq!(
                    ty(source).numeric_widening_to(&ty(target)),
                    expected,
                    "{source} -> {target}"
                );
            }
        }

        for target in ["Float", "F32"] {
            for source in integers {
                let exact = match (source, target) {
                    ("I8" | "I16" | "I32" | "U8" | "U16" | "U32", "Float") => true,
                    ("I8" | "I16" | "U8" | "U16", "F32") => true,
                    _ => false,
                };
                assert_eq!(
                    ty(source).numeric_widening_to(&ty(target)),
                    Some(!exact),
                    "{source} -> {target}"
                );
                assert_eq!(
                    ty(target).numeric_widening_to(&ty(source)),
                    None,
                    "{target} must not narrow to {source}"
                );
            }
        }
    }
}
