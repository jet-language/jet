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

/// One compile-time number attached to a type. The measure plane owns the
/// value; the use site gives it meaning through kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Measure {
    Literal { kind: String, value: u64 },
    Symbol { kind: String, name: String },
}

impl Measure {
    pub fn literal(kind: impl Into<String>, value: u64) -> Self {
        Self::Literal {
            kind: kind.into(),
            value,
        }
    }

    pub fn symbol(kind: impl Into<String>, name: impl Into<String>) -> Self {
        Self::Symbol {
            kind: kind.into(),
            name: name.into(),
        }
    }

    fn canonical(&self) -> String {
        match self {
            Self::Literal { kind, value } => format!("{kind}:{value}"),
            Self::Symbol { kind, name } => format!("{kind}:{name}"),
        }
    }
}

/// Function obligations are facts about how a callable may be used. The
/// D-APILABEL1 call contract is also part of callable identity; effects and
/// returned-view provenance remain sema-checked by subsumption.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FunctionObligations {
    pub effect_bound: Option<Vec<String>>,
    pub param_contract: Option<Vec<(String, super::ParamZone)>>,
    pub return_view_provenance: Option<super::ViewProvenanceMap>,
}

impl FunctionObligations {
    fn canonical(&self) -> String {
        let effects = self.effect_bound.as_ref().map_or_else(String::new, |row| {
            let mut sorted = row.clone();
            sorted.sort();
            sorted.join(",")
        });
        let contract = self
            .param_contract
            .as_ref()
            .map(|row| {
                row.iter()
                    .map(|(name, zone)| format!("{name}:{zone:?}"))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .unwrap_or_default();
        let provenance = self
            .return_view_provenance
            .as_ref()
            .map(super::canonical_view_provenance_map)
            .unwrap_or_default();
        format!("effects=[{effects}];contract=[{contract}];provenance=[{provenance}]")
    }

    /// required is the contract at the use site. offered is the contract
    /// carried by the function value. A value satisfies a contract when its
    /// obligations are at least as strong as the required ones.
    pub fn satisfies(&self, required: &Self) -> bool {
        let effects_ok = match &required.effect_bound {
            None => true,
            Some(required) => self.effect_bound.as_ref().is_some_and(|offered| {
                offered.iter().all(|effect| required.contains(effect))
            }),
        };
        if !effects_ok {
            return false;
        }

        let contract_ok = match &required.param_contract {
            None => true,
            // An absent call contract is the unconstrained structural form:
            // it accepts either spelling at the call site. A declared
            // contract can narrow that surface and must be checked below.
            Some(required) => self.param_contract.as_ref().is_none_or(|offered| {
                offered.len() == required.len()
                    && offered.iter().zip(required).all(|((on, oz), (rn, rz))| {
                        on == rn && match (oz, rz) {
                            (super::ParamZone::Either, _) => true,
                            (offered, required) => offered == required,
                        }
                    })
            }),
        };
        if !contract_ok {
            return false;
        }

        match (
            &required.return_view_provenance,
            &self.return_view_provenance,
        ) {
            (None, _) => true,
            (Some(required), Some(offered)) => required.iter().all(|(slot, contract)| {
                offered.get(slot).is_some_and(|candidate| {
                    (!contract.mutable || candidate.mutable)
                        && candidate.sources.is_subset(&contract.sources)
                })
            }),
            (Some(_), None) => false,
        }
    }
}

/// A fact projected onto one registered plane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KnowledgeFact {
    Interval { lo: i128, hi: i128 },
    Layout { bytes: u8 },
    Measure(Measure),
    Dimension(Dimension),
    Classification(String),
    Nominal(String),
    Obligation(FunctionObligations),
}

impl KnowledgeFact {
    pub fn canonical(&self) -> String {
        match self {
            Self::Interval { lo, hi } => format!("interval:{lo}..{hi}"),
            Self::Layout { bytes } => format!("layout:{bytes}"),
            Self::Measure(measure) => format!("measure:{}", measure.canonical()),
            Self::Dimension(dimension) => format!("dimension:{}", dimension.identity()),
            Self::Classification(name) => format!("classification:{name}"),
            Self::Nominal(name) => format!("nominal:{name}"),
            Self::Obligation(obligation) => {
                format!("obligation:{}", obligation.canonical())
            }
        }
    }
}

/// The one semantic knowledge vector carried by a type. Entries are sorted and
/// deduplicated so identity and reflection do not depend on declaration order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KnowledgeVector {
    entries: Vec<KnowledgeEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEntry {
    /// Structural slot of the fact inside the carrier. The empty path is the
    /// type itself; nested paths keep `Map<Length, Int>` distinct from
    /// `Map<Int, Length>` after their runtime carriers erase dimensions.
    pub path: Vec<String>,
    pub plane: &'static str,
    pub fact: KnowledgeFact,
}

impl KnowledgeVector {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, plane: &'static str, fact: KnowledgeFact) {
        self.push_at(&[], plane, fact);
    }

    pub fn push_at(&mut self, path: &[String], plane: &'static str, fact: KnowledgeFact) {
        let entry = KnowledgeEntry {
            path: path.to_vec(),
            plane,
            fact,
        };
        if self.entries.contains(&entry) {
            return;
        }
        self.entries.push(entry);
        self.entries.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then_with(|| left.plane
                .cmp(right.plane)
                .then_with(|| left.fact.canonical().cmp(&right.fact.canonical())))
        });
    }

    pub fn extend(&mut self, other: &Self) {
        self.extend_at(&[], other);
    }

    pub fn extend_at(&mut self, path: &[String], other: &Self) {
        for entry in &other.entries {
            let mut full_path = path.to_vec();
            full_path.extend(entry.path.iter().cloned());
            self.push_at(&full_path, entry.plane, entry.fact.clone());
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &KnowledgeEntry> {
        self.entries.iter()
    }

    pub fn facts(&self, plane: &'static str) -> impl Iterator<Item = &KnowledgeFact> {
        self.entries
            .iter()
            .filter(move |entry| entry.plane == plane)
            .map(|entry| &entry.fact)
    }

    pub fn identity_only(&self) -> Self {
        Self {
            entries: self
                .entries
                .iter()
                .filter_map(|entry| {
                    if crate::Registry::row(entry.plane)
                        .is_some_and(|row| row.is_identity_bearing())
                    {
                        return Some(entry.clone());
                    }
                    if entry.plane != crate::Registry::TYPE_PLANE_OBLIGATION {
                        return None;
                    }
                    // D-APILABEL1: this mixed plane contributes only its call
                    // contract to identity; effects and return provenance
                    // remain directional obligations.
                    let KnowledgeFact::Obligation(obligations) = &entry.fact else {
                        return None;
                    };
                    let Some(param_contract) = &obligations.param_contract else {
                        return None;
                    };
                    Some(KnowledgeEntry {
                        path: entry.path.clone(),
                        plane: entry.plane,
                        fact: KnowledgeFact::Obligation(FunctionObligations {
                            effect_bound: None,
                            param_contract: Some(param_contract.clone()),
                            return_view_provenance: None,
                        }),
                    })
                })
                .collect(),
        }
    }

    pub fn identity_key(&self) -> String {
        self.identity_only()
            .entries
            .iter()
            .map(|entry| {
                let path = if entry.path.is_empty() {
                    "self".to_string()
                } else {
                    entry.path.join(".")
                };
                format!("{path}:{}={}", entry.plane, entry.fact.canonical())
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn interval_i128(&self) -> Option<(i128, i128)> {
        self.entries
            .iter()
            .find_map(|entry| match (&entry.path.is_empty(), &entry.fact) {
                (true, KnowledgeFact::Interval { lo, hi }) => Some((*lo, *hi)),
                _ => None,
            })
    }

    pub fn interval(&self) -> Option<(i64, i64)> {
        let (lo, hi) = self.interval_i128()?;
        Some((lo.try_into().ok()?, hi.try_into().ok()?))
    }

    pub fn obligations(&self) -> Option<&FunctionObligations> {
        self.entries.iter().find_map(|entry| {
            match (&entry.path.is_empty(), &entry.fact) {
                (true, KnowledgeFact::Obligation(obligations)) => Some(obligations),
                _ => None,
            }
        })
    }

    pub fn from_interval(lo: i64, hi: i64) -> Self {
        let mut vector = Self::new();
        vector.push(
            crate::Registry::TYPE_PLANE_INTERVAL,
            KnowledgeFact::Interval {
                lo: i128::from(lo),
                hi: i128::from(hi),
            },
        );
        vector
    }
}

/// Stable type identity: runtime carrier plus the identity-bearing projection
/// of the knowledge vector. Call contracts are identity-bearing; other
/// function obligations remain outside identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeIdentity {
    pub carrier: String,
    pub knowledge: KnowledgeVector,
}

/// The marker carried by `Type::Tagged`: either a user-written D-QUAL4 tag
/// name, or one compiler-internal provenance/access fact. Card #1662: the
/// internal facts used to be NUL-prefixed strings smuggled through the same
/// `String` field as a real user tag name — unspellable in source, but still
/// a `String` pretending to be one. `Internal` gives each fact a real enum
/// variant instead; no parser path ever constructs it, so it stays exactly as
/// unspellable as the retired marker constants.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagMarker {
    /// D-QUAL4=A: user-written `#TagName T` in signature/binding position.
    User(String),
    /// Compiler-internal provenance/access fact (card #1662).
    Internal(InternalTag),
}

impl std::fmt::Display for TagMarker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TagMarker::User(name) => write!(f, "{name}"),
            TagMarker::Internal(tag) => write!(f, "{}", tag.spelling()),
        }
    }
}

/// One compiler-internal `Type::Tagged` provenance/access fact (card #1662).
/// Every variant here retires one `\0`-prefixed marker constant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternalTag {
    /// Was `CORE_CRYPTO_NOMINAL_MARKER`: purpose-bound `core.crypto` nominal
    /// provenance. Identity-bearing (see the manual `PartialEq for Type`).
    CoreCryptoNominal,
    /// Was `DETERMINISTIC_CLOCK_MARKER` / `SYSTEM_CLOCK_MARKER`: provenance
    /// for deterministic and system-backed `Clock` values.
    DeterministicClock,
    SystemClock,
    /// Was `EXPIRING_SECRET_LOAN_MARKER`: the temporary read-only value lent
    /// by `ExpiringSecret.with`. Cannot be named in source or stored anywhere.
    ExpiringSecretLoan,
    /// Was `SHARED_GUARD_READ_MARKER` / `SHARED_GUARD_EDIT_MARKER`: the
    /// compiler-only access modes for the single public `SharedGuard<T>` type.
    SharedGuardRead,
    SharedGuardEdit,
    /// Was `TERMINAL_FACT_SET_MARKER`: the open terminal capability-key set.
    TerminalFactSet,
    /// Was `CPP_CALLBACK_ABI_MARKER`: a generated C++ facade parameter that
    /// keeps the source-level callback shape while telling the backend it is
    /// already a raw C function pointer, not a boxed Jet closure.
    CppCallbackAbi,
}

impl InternalTag {
    fn spelling(self) -> &'static str {
        match self {
            InternalTag::CoreCryptoNominal => "core.crypto",
            InternalTag::DeterministicClock => "clock.deterministic",
            InternalTag::SystemClock => "clock.system",
            InternalTag::ExpiringSecretLoan => "expiring_secret.loan",
            InternalTag::SharedGuardRead => "shared_guard.read",
            InternalTag::SharedGuardEdit => "shared_guard.edit",
            InternalTag::TerminalFactSet => "terminal.fact_set",
            InternalTag::CppCallbackAbi => "cpp.callback_abi",
        }
    }
}

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
    /// same identity; sema still rejects an offered callable whose obligation
    /// cannot satisfy a stricter required bound.
    Fn {
        params: Vec<Type>,
        ret: Option<Box<Type>>,
        effect_bound: Option<Vec<(String, Span)>>,
        /// D-APILABEL1=A: the declared call contract — public label and zone
        /// per parameter, parallel to `params`. Labels and zones are callable
        /// identity; sema also checks directional compatibility. `None` means
        /// the callable has no declared call contract.
        param_contract: Option<Vec<(String, super::ParamZone)>>,
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
    /// `marker` is `TagMarker::User` for these. `TagMarker::Internal` reuses
    /// the same shape for compiler-owned provenance/access facts (card #1662);
    /// `CoreCryptoNominal` is the one exception made identity-bearing (see the
    /// manual `PartialEq for Type` below).
    Tagged {
        marker: TagMarker,
        inner: Box<Type>,
    },
    /// D-UNIONTYPE1=A: closed structural sum `A | B | …`. Canonical form is
    /// flattened, duplicate-free, and sorted by member `name()` spelling so
    /// identity is order-insensitive. Desugars to one compiler-generated enum
    /// whose arms are named by the member types.
    Union(Vec<Type>),
    /// D-COMPUTE-TYPE1 family: a physical quantity — a numeric `base` type
    /// tagged with an open `Dimension`. Card #1662: replaces the retired
    /// unwriteable-marker `Type::Apply` string encoding with a real variant.
    /// Runtime values still carry no dimension metadata; the dimension is
    /// compile-time only.
    Quantity {
        base: Box<Type>,
        dimension: Dimension,
    },
    /// D-COMPUTE-TYPE1: a fixed const dimension in `Vec<N>` / `Matrix<M, N>`,
    /// carried as one `Type::Apply` argument. Card #1662: replaces the retired
    /// `\0compute.dimension.<N>`-prefixed `Type::Named` string encoding — a
    /// real `u64` payload can't be confused with a user type name, so unlike
    /// that encoding this needs no unspellable-prefix trick.
    ComputeDim(u64),
}

/// Failure returned when two composite types do not have the same shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompositeTypePairError {
    ShapeMismatch,
}

fn is_core_crypto(marker: &TagMarker) -> bool {
    matches!(marker, TagMarker::Internal(InternalTag::CoreCryptoNominal))
}

/// Type equality is the carrier plus identity-bearing knowledge projection
/// (D-TYPE2-FOUND1). Call contracts are identity-bearing; other callable
/// obligations remain in the vector for reflection and sema subsumption.
impl PartialEq for Type {
    fn eq(&self, other: &Self) -> bool {
        self.identity() == other.identity()
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
        0 => Type::Named(crate::Syntax::INTERNAL_UNIT_TYPE.to_string()),
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

fn fn_param_names(params: &[Type], contract: Option<&[(String, super::ParamZone)]>) -> String {
    let contract = contract.unwrap_or(&[]);
    let mut parts = Vec::with_capacity(params.len() + 2);
    let mut star_done = false;
    for (index, param) in params.iter().enumerate() {
        let zone = contract.get(index).map(|(_, zone)| *zone);
        if zone == Some(super::ParamZone::LabelOnly) && !star_done {
            parts.push(crate::Syntax::PARAM_ZONE_LABEL_ONLY.to_string());
            star_done = true;
        }
        let label = contract
            .get(index)
            .map(|(label, _)| label.as_str())
            .filter(|label| !label.is_empty());
        parts.push(label.map_or_else(|| param.name(), |label| format!("{label}: {}", param.name())));
        if zone == Some(super::ParamZone::PositionalOnly)
            && contract
                .get(index + 1)
                .is_none_or(|(_, next)| *next != super::ParamZone::PositionalOnly)
        {
            parts.push(crate::Syntax::PARAM_ZONE_POSITIONAL_ONLY.to_string());
        }
    }
    parts.join(", ")
}

impl Type {
    /// D-QUAL4=A: remove only user value-fact tags while preserving compiler
    /// tags that carry nominal identity or access policy.
    pub fn without_user_tags(&self) -> &Type {
        let mut ty = self;
        while let Type::Tagged {
            marker: TagMarker::User(_),
            inner,
        } = ty
        {
            ty = inner.as_ref();
        }
        ty
    }

    /// Visit matched pairs under the shared composite carriers.
    ///
    /// The walker owns structural recursion for `List`, `Option`, `Result`,
    /// and `Apply`. Judgments own leaf meaning. `Apply` names stay with the
    /// caller because unification and obligation checks use different rules.
    pub fn for_each_composite_pair(
        left: &Type,
        right: &Type,
        visit: &mut impl FnMut(&Type, &Type),
    ) -> Result<(), CompositeTypePairError> {
        match (left, right) {
            (Type::List(left), Type::List(right))
            | (Type::Option(left), Type::Option(right)) => {
                visit(left, right);
                Self::for_each_composite_pair(left, right, visit)
            }
            (
                Type::Result {
                    ok: left_ok,
                    err: left_err,
                },
                Type::Result {
                    ok: right_ok,
                    err: right_err,
                },
            ) => {
                visit(left, right);
                Self::for_each_composite_pair(left_ok, right_ok, visit)?;
                Self::for_each_composite_pair(left_err, right_err, visit)
            }
            (
                Type::Apply {
                    args: left_args, ..
                },
                Type::Apply {
                    args: right_args, ..
                },
            ) if left_args.len() == right_args.len() => {
                visit(left, right);
                for (left, right) in left_args.iter().zip(right_args) {
                    Self::for_each_composite_pair(left, right, visit)?;
                }
                Ok(())
            }
            (Type::List(_), _)
            | (_, Type::List(_))
            | (Type::Option(_), _)
            | (_, Type::Option(_))
            | (Type::Result { .. }, _)
            | (_, Type::Result { .. })
            | (Type::Apply { .. }, _)
            | (_, Type::Apply { .. }) => Err(CompositeTypePairError::ShapeMismatch),
            _ => {
                visit(left, right);
                Ok(())
            }
        }
    }

    /// Project all compile-time facts carried by this type onto the one
    /// knowledge vector. The old enum payloads are read here once; sema
    /// consumers must use this projection instead of inventing a second
    /// identity or obligation check.
    pub fn knowledge_vector(&self) -> KnowledgeVector {
        let mut vector = KnowledgeVector::new();
        match self {
            Type::List(inner) => {
                vector.extend_at(&["element".to_string()], &inner.knowledge_vector());
            }
            Type::Shared(inner) => {
                vector.extend_at(&["inner".to_string()], &inner.knowledge_vector());
            }
            Type::Option(inner) => {
                vector.extend_at(&["some".to_string()], &inner.knowledge_vector());
            }
            Type::Map { key, value, .. } => {
                vector.extend_at(&["key".to_string()], &key.knowledge_vector());
                vector.extend_at(&["value".to_string()], &value.knowledge_vector());
            }
            Type::Result { ok, err } => {
                vector.extend_at(&["ok".to_string()], &ok.knowledge_vector());
                vector.extend_at(&["err".to_string()], &err.knowledge_vector());
            }
            Type::Tuple(fields) => {
                for (name, field) in fields {
                    vector.extend_at(
                        &["field".to_string(), name.clone()],
                        &field.knowledge_vector(),
                    );
                }
            }
            Type::Union(members) => {
                for (index, member) in members.iter().enumerate() {
                    vector.extend_at(
                        &["member".to_string(), index.to_string()],
                        &member.knowledge_vector(),
                    );
                }
            }
            Type::IntN { signed, bits } => {
                let (lo, hi) = int_range(*signed, *bits);
                vector.push(
                    crate::Registry::TYPE_PLANE_INTERVAL,
                    KnowledgeFact::Interval { lo, hi },
                );
                vector.push(
                    crate::Registry::TYPE_PLANE_LAYOUT,
                    KnowledgeFact::Layout {
                        bytes: (*bits + 7) / 8,
                    },
                );
            }
            Type::FixedList {
                elem,
                len, len_symbol, ..
            } => {
                vector.extend_at(&["element".to_string()], &elem.knowledge_vector());
                let measure = len_symbol
                    .as_ref()
                    .map_or_else(|| Measure::literal("length", *len), |(name, _)| {
                        Measure::symbol("length", name)
                    });
                vector.push(
                    crate::Registry::TYPE_PLANE_MEASURE,
                    KnowledgeFact::Measure(measure),
                );
            }
            Type::Fn { params, ret, .. } => {
                for (index, param) in params.iter().enumerate() {
                    vector.extend_at(
                        &["param".to_string(), index.to_string()],
                        &param.knowledge_vector(),
                    );
                }
                if let Some(ret) = ret {
                    vector.extend_at(&["return".to_string()], &ret.knowledge_vector());
                }
                if let Some(obligations) = self.function_obligations() {
                    vector.push(
                        crate::Registry::TYPE_PLANE_OBLIGATION,
                        KnowledgeFact::Obligation(obligations),
                    );
                }
            }
            Type::Quantity { base, dimension } => {
                vector.extend_at(&["base".to_string()], &base.knowledge_vector());
                vector.push(
                    crate::Registry::TYPE_PLANE_DIMENSION,
                    KnowledgeFact::Dimension(dimension.clone()),
                );
            }
            Type::ComputeDim(value) => {
                vector.push(
                    crate::Registry::TYPE_PLANE_MEASURE,
                    KnowledgeFact::Measure(Measure::literal("shape", *value)),
                );
            }
            Type::Tagged { marker, inner } if is_core_crypto(marker) => {
                vector.extend_at(&["inner".to_string()], &inner.knowledge_vector());
                vector.push(
                    crate::Registry::TYPE_PLANE_NOMINAL,
                    KnowledgeFact::Nominal(marker.to_string()),
                );
            }
            Type::Tagged { marker, inner } => {
                // User tags are transparent flow classifications. Do not add
                // a structural path around the identity-bearing inner facts.
                vector.extend(&inner.knowledge_vector());
                vector.push(
                    crate::Registry::TYPE_PLANE_CLASSIFICATION,
                    KnowledgeFact::Classification(marker.to_string()),
                );
            }
            Type::Apply { args, .. } => {
                for (index, arg) in args.iter().enumerate() {
                    if matches!(arg, Type::ComputeDim(_)) {
                        continue;
                    }
                    vector.extend_at(
                        &["argument".to_string(), index.to_string()],
                        &arg.knowledge_vector(),
                    );
                }
                for (index, dimension) in self
                    .compute_shape_dimensions()
                    .into_iter()
                    .flatten()
                    .enumerate()
                {
                    vector.push_at(
                        &["shape".to_string(), index.to_string()],
                        crate::Registry::TYPE_PLANE_MEASURE,
                        KnowledgeFact::Measure(Measure::literal("shape", dimension)),
                    );
                }
            }
            Type::Named(name) => {
                let lanes = match name.as_str() {
                    "F32x4" => Some(4),
                    "F64x2" => Some(2),
                    _ => None,
                };
                if let Some(lanes) = lanes {
                    vector.push(
                        crate::Registry::TYPE_PLANE_MEASURE,
                        KnowledgeFact::Measure(Measure::literal("lane", lanes)),
                    );
                }
            }
            _ => {}
        }
        vector
    }

    fn function_obligations(&self) -> Option<FunctionObligations> {
        let Type::Fn {
            effect_bound,
            param_contract,
            return_view_provenance,
            ..
        } = self
        else {
            return None;
        };
        if effect_bound.is_none()
            && param_contract.is_none()
            && return_view_provenance.is_none()
        {
            return None;
        }
        Some(FunctionObligations {
            effect_bound: effect_bound
                .as_ref()
                .map(|row| row.iter().map(|(name, _)| name.clone()).collect()),
            param_contract: param_contract.clone(),
            return_view_provenance: return_view_provenance.clone(),
        })
    }

    /// Compare callable obligations from required type to offered type.
    pub fn obligations_satisfy(required: &Type, offered: &Type) -> bool {
        fn nested(required: &Type, offered: &Type) -> bool {
            match (required, offered) {
                (
                    Type::Fn {
                        params: required_params,
                        ret: required_ret,
                        ..
                    },
                    Type::Fn {
                        params: offered_params,
                        ret: offered_ret,
                        ..
                    },
                ) => {
                    let function_ok = match required.function_obligations() {
                        None => true,
                        Some(required) => offered
                            .function_obligations()
                            .unwrap_or_default()
                            .satisfies(&required),
                    };
                    function_ok
                        && required_params.len() == offered_params.len()
                        && required_params
                            .iter()
                            .zip(offered_params)
                            .all(|(required, offered)| nested(required, offered))
                        && match (required_ret, offered_ret) {
                            (None, None) => true,
                            (Some(required), Some(offered)) => nested(required, offered),
                            _ => false,
                        }
                }
                (Type::List(_), Type::List(_))
                | (Type::Option(_), Type::Option(_))
                | (Type::Result { .. }, Type::Result { .. })
                | (Type::Apply { .. }, Type::Apply { .. }) => {
                    let mut satisfied = true;
                    let shape = Type::for_each_composite_pair(
                        required,
                        offered,
                        &mut |required, offered| {
                            if satisfied {
                                match (required, offered) {
                                    (Type::List(_), Type::List(_))
                                    | (Type::Option(_), Type::Option(_))
                                    | (Type::Result { .. }, Type::Result { .. })
                                    | (Type::Apply { .. }, Type::Apply { .. }) => {}
                                    _ => satisfied = nested(required, offered),
                                }
                            }
                        },
                    );
                    shape.is_ok() && satisfied
                }
                (Type::Shared(required), Type::Shared(offered)) => nested(required, offered),
                (
                    Type::Map {
                        key: required_key,
                        value: required_value,
                        ..
                    },
                    Type::Map {
                        key: offered_key,
                        value: offered_value,
                        ..
                    },
                ) => {
                    nested(required_key, offered_key) && nested(required_value, offered_value)
                }
                (
                    Type::Tuple(required_fields),
                    Type::Tuple(offered_fields),
                ) => {
                    required_fields.len() == offered_fields.len()
                        && required_fields
                            .iter()
                            .zip(offered_fields)
                            .all(|((required_name, required), (offered_name, offered))| {
                                required_name == offered_name && nested(required, offered)
                            })
                }
                (Type::FixedList { elem: required, .. }, Type::FixedList { elem: offered, .. })
                | (Type::Quantity { base: required, .. }, Type::Quantity { base: offered, .. }) => {
                    nested(required, offered)
                }
                (Type::Union(required), Type::Union(offered)) => {
                    required.len() == offered.len()
                        && required
                            .iter()
                            .zip(offered)
                            .all(|(required, offered)| nested(required, offered))
                }
                (Type::Tagged { marker, inner }, offered) if !is_core_crypto(marker) => {
                    nested(inner, offered)
                }
                (required, Type::Tagged { marker, inner }) if !is_core_crypto(marker) => {
                    nested(required, inner)
                }
                _ => true,
            }
        }

        nested(required, offered)
    }

    /// Return the stable identity projection required by D-TYPE2-FOUND1.
    pub fn identity(&self) -> TypeIdentity {
        TypeIdentity {
            carrier: self.carrier_identity_name(),
            knowledge: self.knowledge_vector().identity_only(),
        }
    }

    pub fn identity_key(&self) -> String {
        let identity = self.identity();
        if identity.knowledge.is_empty() {
            identity.carrier
        } else {
            format!("{}|{}", identity.carrier, identity.knowledge.identity_key())
        }
    }

    /// Remove compile-time knowledge before a typed-IR boundary. This method
    /// is a foundation operation; engines only receive its carrier result.
    pub fn erased_carrier(&self) -> Type {
        match self {
            Type::List(inner) => Type::List(Box::new(inner.erased_carrier())),
            Type::Map { key, key_span, value } => Type::Map {
                key: Box::new(key.erased_carrier()),
                key_span: *key_span,
                value: Box::new(value.erased_carrier()),
            },
            Type::Shared(inner) => Type::Shared(Box::new(inner.erased_carrier())),
            Type::Option(inner) => Type::Option(Box::new(inner.erased_carrier())),
            Type::Result { ok, err } => Type::Result {
                ok: Box::new(ok.erased_carrier()),
                err: Box::new(err.erased_carrier()),
            },
            Type::Quantity { base, .. } => base.erased_carrier(),
            Type::FixedList { elem, .. } => Type::List(Box::new(elem.erased_carrier())),
            Type::ComputeDim(_) => Type::Int,
            Type::Tagged { inner, .. } => inner.erased_carrier(),
            Type::Fn {
                params, ret, ..
            } => Type::Fn {
                params: params.iter().map(Type::erased_carrier).collect(),
                ret: ret
                    .as_ref()
                    .map(|return_type| Box::new(return_type.erased_carrier())),
                effect_bound: None,
                param_contract: None,
                return_view_provenance: None,
            },
            Type::Apply { name, args } => Type::Apply {
                name: name.clone(),
                args: args.iter().map(Type::erased_carrier).collect(),
            },
            Type::Tuple(fields) => Type::Tuple(
                fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), Box::new(ty.erased_carrier())))
                    .collect(),
            ),
            Type::Union(members) => canonicalize_union(
                members.iter().map(Type::erased_carrier).collect(),
            ),
            _ => self.clone(),
        }
    }

    fn carrier_identity_name(&self) -> String {
        match self {
            Type::List(inner) => format!("[{}]", inner.carrier_identity_name()),
            Type::Map { key, value, .. } => format!(
                "[{}: {}]",
                key.carrier_identity_name(),
                value.carrier_identity_name()
            ),
            Type::Shared(inner) => format!("Shared<{}>", inner.carrier_identity_name()),
            Type::Option(inner) => format!("{}?", inner.carrier_identity_name()),
            Type::Result { ok, err } => {
                format!("{} ? {}", ok.carrier_identity_name(), err.carrier_identity_name())
            }
            Type::Fn { params, ret, .. } => {
                let params = params
                    .iter()
                    .map(Type::carrier_identity_name)
                    .collect::<Vec<_>>()
                    .join(", ");
                let ret = ret
                    .as_ref()
                    .map(|return_type| format!(" => {}", return_type.carrier_identity_name()))
                    .unwrap_or_default();
                format!("fn({params}){ret}")
            }
            Type::Apply { name, args } => format!(
                "{}<{}>",
                name,
                args.iter()
                    .map(Type::carrier_identity_name)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Type::Tuple(fields) => format!(
                "({})",
                fields
                    .iter()
                    .map(|(name, ty)| format!("{name}: {}", ty.carrier_identity_name()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Type::FixedList { elem, .. } => {
                format!("[{}]", elem.carrier_identity_name())
            }
            Type::Tagged { inner, .. } => inner.carrier_identity_name(),
            Type::Union(members) => members
                .iter()
                .map(Type::carrier_identity_name)
                .collect::<Vec<_>>()
                .join(" | "),
            Type::Quantity { base, .. } => base.carrier_identity_name(),
            Type::ComputeDim(_) => "Int".to_string(),
            _ => self.name(),
        }
    }

    /// D-COMPUTE-TYPE1: preserve a fixed compute dimension in the type tree.
    pub fn compute_dimension_type(value: u64) -> Type {
        Type::ComputeDim(value)
    }

    /// Return a fixed compute dimension, if this is the compiler-owned marker.
    pub fn compute_dimension_value(&self) -> Option<u64> {
        match self {
            Type::ComputeDim(value) => Some(*value),
            _ => None,
        }
    }

    /// Build one of the fixed-shape compute aliases from its dimensions.
    pub fn compute_shape_type(name: &str, dimensions: &[u64]) -> Type {
        Type::Apply {
            name: name.to_string(),
            args: dimensions
                .iter()
                .copied()
                .map(Self::compute_dimension_type)
                .collect(),
        }
    }

    /// Return the dimensions carried by `Vec<N>` or `Matrix<M, N>`.
    pub fn compute_shape_dimensions(&self) -> Option<Vec<u64>> {
        let Type::Apply { name, args } = self else {
            return None;
        };
        if !matches!(name.as_str(), "Vec" | "Matrix") {
            return None;
        }
        args.iter().map(Self::compute_dimension_value).collect()
    }

    /// Compute values all share the `JetTensor` storage substrate. An erased
    /// `Tensor` is compatible with a shaped alias; two shaped aliases still
    /// require exact equality, so `Vec<3>` cannot silently become `Vec<4>`.
    pub fn is_compute_tensor_family(&self) -> bool {
        matches!(self, Type::Named(name) if name == "Tensor")
            || matches!(self, Type::Apply { name, .. } if matches!(name.as_str(), "Tensor" | "Vec" | "Matrix"))
    }

    pub fn compute_tensor_compatible(want: &Type, got: &Type) -> bool {
        if want == got {
            return true;
        }
        (matches!(want, Type::Named(name) if name == "Tensor") && got.is_compute_tensor_family())
            || (matches!(got, Type::Named(name) if name == "Tensor") && want.is_compute_tensor_family())
    }

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
            Type::Fn { params, ret, effect_bound, param_contract, return_view_provenance } => Type::Fn {
                params: params.iter().map(|ty| ty.map_named_types(map)).collect(),
                ret: ret.as_ref().map(|ty| Box::new(ty.map_named_types(map))),
                effect_bound: effect_bound.clone(),
                param_contract: param_contract.clone(),
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
            Type::Quantity { base, dimension } => Type::Quantity {
                base: Box::new(base.map_named_types(map)),
                dimension: dimension.clone(),
            },
            other => other.clone(),
        }
    }

    pub fn quantity(base: Type, dimension: Dimension) -> Type {
        Type::Quantity {
            base: Box::new(base),
            dimension,
        }
    }

    pub fn quantity_parts(&self) -> Option<(&Type, Dimension)> {
        match self {
            Type::Quantity { base, dimension } => Some((base, dimension.clone())),
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
            Type::Fn { params, ret, effect_bound, param_contract, .. } => {
                let ps = fn_param_names(params, param_contract.as_deref());
                match (effect_bound, ret) {
                    (Some(row), Some(r)) => format!("fn({}) =[{}]=> {}", ps, effect_names(row), r.name()),
                    (Some(row), None) => format!("fn({}) =[{}]=>", ps, effect_names(row)),
                    (None, Some(r)) => format!("fn({}) => {}", ps, r.name()),
                    (None, None) => format!("fn({})", ps),
                }
            }
            Type::Named(n) => format!("`{}`", n),
            Type::ComputeDim(value) => format!("{value} (a fixed compute dimension)"),
            // D-CAP9: the raw-pointer type shows as the canonical `*T`.
            Type::Apply { name, args } if name == crate::Syntax::TYPE_PTR && args.len() == 1 => {
                format!("`*{}`", args[0].name())
            }
            Type::Quantity { dimension, .. } => {
                format!("{} (a physical quantity)", dimension.display_name())
            }
            Type::Apply { name, args } => {
                if let Some(dimensions) = self.compute_shape_dimensions() {
                    return format!(
                        "`{}`<{}>",
                        name,
                        dimensions.iter().map(u64::to_string).collect::<Vec<_>>().join(", ")
                    );
                }
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
            Type::Tagged { marker: TagMarker::Internal(_), inner } => inner.show(),
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
            Type::Fn { params, ret, effect_bound, param_contract, .. } => {
                let ps = fn_param_names(params, param_contract.as_deref());
                match (effect_bound, ret) {
                    (Some(row), Some(r)) => format!("fn({}) =[{}]=> {}", ps, effect_names(row), r.name()),
                    (Some(row), None) => format!("fn({}) =[{}]=>", ps, effect_names(row)),
                    (None, Some(r)) => format!("fn({}) => {}", ps, r.name()),
                    (None, None) => format!("fn({})", ps),
                }
            }
            Type::Named(n) => n.clone(),
            Type::ComputeDim(value) => value.to_string(),
            // D-CAP9: the raw-pointer type names as the canonical `*T`.
            Type::Apply { name, args } if name == crate::Syntax::TYPE_PTR && args.len() == 1 => {
                format!("*{}", args[0].name())
            }
            Type::Quantity { base, dimension } => {
                format!(
                    "Quantity<{}, {}; {}>",
                    dimension.display_name(),
                    base.name(),
                    dimension.identity()
                )
            }
            Type::Apply { name, args } => {
                if let Some(dimensions) = self.compute_shape_dimensions() {
                    return format!(
                        "{}<{}>",
                        name,
                        dimensions.iter().map(u64::to_string).collect::<Vec<_>>().join(", ")
                    );
                }
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
            Type::Tagged { marker: TagMarker::Internal(_), inner } => inner.name(),
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

    /// Bounds projected from the interval plane for an integer carrier.
    pub fn integer_range(&self) -> Option<(i128, i128)> {
        match self {
            Type::Int => Some(int_range(true, 64)),
            Type::IntN { .. } => self.knowledge_vector().interval_i128(),
            Type::Tagged { inner, .. } => inner.integer_range(),
            _ => None,
        }
    }

    fn integer_layout(&self) -> Option<(bool, u8)> {
        match self {
            Type::Int => Some((true, 64)),
            Type::IntN { signed, bits } => Some((*signed, *bits)),
            Type::Tagged { inner, .. } => inner.integer_layout(),
            _ => None,
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

        if let (Some((source_min, source_max)), Some((target_min, target_max))) =
            (self.integer_range(), target.integer_range())
        {
            return (target_min <= source_min && source_max <= target_max).then_some(false);
        }

        match (self, target) {
            (Type::Float32, Type::Float) => Some(false),
            (source, Type::Float | Type::Float32) if source.is_integer() => {
                let (signed, bits) = source.integer_layout()?;
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
    use super::{
        numeric_type_from_name, Dimension, InternalTag, KnowledgeFact, Measure, TagMarker, Type,
    };
    use crate::AST::ParamZone;

    fn core_secret() -> Type {
        Type::Tagged {
            marker: TagMarker::Internal(InternalTag::CoreCryptoNominal),
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
            marker: TagMarker::User("Tainted(Credential)".to_string()),
            inner: Box::new(core.clone()),
        };

        assert_ne!(core, local);
        assert_eq!(tainted_core, core);
    }

    #[test]
    fn function_contract_is_identity_and_renders_in_type_names() {
        let bare = Type::Fn {
            params: vec![Type::Bool],
            ret: Some(Box::new(Type::Int)),
            effect_bound: None,
            param_contract: None,
            return_view_provenance: None,
        };
        let labelled = Type::Fn {
            params: vec![Type::Bool],
            ret: Some(Box::new(Type::Int)),
            effect_bound: None,
            param_contract: Some(vec![("force".to_string(), ParamZone::LabelOnly)]),
            return_view_provenance: None,
        };

        assert_ne!(bare, labelled);
        assert_eq!(bare.name(), "fn(Bool) => Int");
        assert_eq!(labelled.name(), "fn(*, force: Bool) => Int");
        assert_eq!(labelled.show(), "fn(*, force: Bool) => Int");
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
            param_contract: None,
            return_view_provenance: None,
        };
        let length = nested.map_named_types(&|name| (name == "Unit").then(|| "length.Unit".into()));
        let time = nested.map_named_types(&|name| (name == "Unit").then(|| "time.Unit".into()));
        assert_ne!(length, time);
        assert!(length.name().contains("length.Unit"));
        assert!(time.name().contains("time.Unit"));
    }

    #[test]
    fn knowledge_vector_projects_facts_and_erases_before_typed_ir() {
        let quantity = Type::quantity(
            Type::IntN {
                signed: false,
                bits: 8,
            },
            Dimension::base("Length"),
        );
        let vector = quantity.knowledge_vector();
        assert!(vector.iter().any(|entry| {
            matches!(&entry.fact, KnowledgeFact::Dimension(_))
        }));
        assert!(vector.iter().any(|entry| {
            matches!(&entry.fact, KnowledgeFact::Interval { lo: 0, hi: 255 })
        }));
        assert_eq!(
            quantity.erased_carrier(),
            Type::IntN {
                signed: false,
                bits: 8
            }
        );
        assert_eq!(
            Type::compute_shape_type("Vec", &[3]).erased_carrier(),
            Type::Apply {
                name: "Vec".to_string(),
                args: vec![Type::Int]
            }
        );
        assert!(
            quantity
                .identity()
                .knowledge
                .identity_key()
                .contains("Type.Dimension")
        );
        assert!(
            Type::List(Box::new(quantity.clone()))
                .identity()
                .knowledge
                .identity_key()
                .contains("Type.Dimension")
        );
        let length_key = Type::Map {
            key: Box::new(quantity.clone()),
            key_span: None,
            value: Box::new(Type::Int),
        };
        let length_value = Type::Map {
            key: Box::new(Type::Int),
            key_span: None,
            value: Box::new(quantity.clone()),
        };
        assert_ne!(length_key.identity_key(), length_value.identity_key());
        assert!(Type::Named("F32x4".to_string())
            .knowledge_vector()
            .facts(crate::Registry::TYPE_PLANE_MEASURE)
            .any(|fact| matches!(
                fact,
                KnowledgeFact::Measure(Measure::Literal { kind, value })
                    if kind == "lane" && *value == 4
            )));
    }

    #[test]
    fn function_identity_is_transitive_and_obligations_use_subsumption() {
        let callable = |zone| Type::Fn {
            params: vec![Type::Bool],
            ret: Some(Box::new(Type::Int)),
            effect_bound: None,
            param_contract: Some(vec![("force".to_string(), zone)]),
            return_view_provenance: None,
        };
        let positional = callable(ParamZone::PositionalOnly);
        let labelled = callable(ParamZone::LabelOnly);
        let flexible = callable(ParamZone::Either);
        let bare = Type::Fn {
            params: vec![Type::Bool],
            ret: Some(Box::new(Type::Int)),
            effect_bound: None,
            param_contract: None,
            return_view_provenance: None,
        };

        assert_ne!(positional, bare);
        assert_ne!(labelled, bare);
        assert_ne!(positional, labelled);
        assert!(!Type::obligations_satisfy(&positional, &labelled));
        assert!(Type::obligations_satisfy(&positional, &flexible));
        assert!(!Type::obligations_satisfy(
            &Type::List(Box::new(positional.clone())),
            &Type::List(Box::new(labelled.clone()))
        ));
        assert!(Type::obligations_satisfy(
            &Type::List(Box::new(positional.clone())),
            &Type::List(Box::new(bare.clone()))
        ));
        assert_ne!(positional.identity(), bare.identity());
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
