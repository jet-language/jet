use crate::Diagnostics::Span;
use crate::AST::Expr;

/// D-DIMENSION-OPEN1=D: a normalized open physical dimension.
///
/// Keys are nominal base-dimension identities. Entries stay sorted and zero
/// exponents are removed, so equality and serialized API identity are stable.
/// Runtime values carry no dimension metadata.
/// Exponents use the shared measure representation; the `axes` and identity
/// methods keep the existing concrete dimension API for callers.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Dimension(std::collections::BTreeMap<String, Measure>);

impl Dimension {
    pub fn scalar() -> Self {
        Self(std::collections::BTreeMap::new())
    }

    pub fn base(identity: impl Into<String>) -> Self {
        Self(std::iter::once((identity.into(), Measure::signed_literal("exponent", 1))).collect())
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
            let factor = Measure::signed_literal("exponent", if subtract { -1 } else { 1 });
            let exponent = exponent.combine(&factor, MeasureRule::Mul)?;
            let current = out
                .get(axis)
                .cloned()
                .unwrap_or_else(|| Measure::signed_literal("exponent", 0));
            let next = current.combine(&exponent, MeasureRule::Add)?;
            let value = i32::try_from(next.signed_literal_value()?).ok()?;
            if value == 0 {
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
            let factor = Measure::signed_literal("exponent", i64::from(exponent));
            let next = current.combine(&factor, MeasureRule::Mul)?;
            let value = i32::try_from(next.signed_literal_value()?).ok()?;
            if value != 0 {
                out.insert(axis.clone(), next);
            }
        }
        Some(Self(out))
    }

    pub fn axes(&self) -> impl Iterator<Item = (&str, i32)> {
        self.0.iter().map(|(axis, exponent)| {
            (
                axis.as_str(),
                Self::exponent_value(exponent)
                    .expect("dimension exponent must be a concrete i32 measure"),
            )
        })
    }

    /// D-TYPE2-MEASURE1=A: expose dimension exponents through the same measure
    /// projection used by lengths, shapes, and lanes.
    pub fn measure_exponents(&self) -> impl Iterator<Item = (&str, Measure)> {
        self.0
            .iter()
            .map(|(axis, exponent)| (axis.as_str(), exponent.clone()))
    }

    /// Stable identity used by API/type serialization.
    pub fn identity(&self) -> String {
        self.axes()
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
            let exponent = exponent.parse::<i32>().ok()?;
            if exponent == 0
                || axes
                    .insert(
                        unescape_axis(axis)?,
                        Measure::signed_literal("exponent", i64::from(exponent)),
                    )
                    .is_some()
            {
                return None;
            }
        }
        Some(Self(axes))
    }

    pub fn display_name(&self) -> String {
        let mut parts = Vec::new();
        for (axis, exponent) in self.axes() {
            let name = axis.rsplit("::").next().unwrap_or(axis);
            parts.push(if exponent == 1 {
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

    fn exponent_value(measure: &Measure) -> Option<i32> {
        measure
            .signed_literal_value()
            .and_then(|value| i32::try_from(value).ok())
    }
}

fn escape_axis(axis: &str) -> String {
    axis.replace('%', "%25")
        .replace(';', "%3B")
        .replace(':', "%3A")
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
/// resolved value; only literals, module value parameters, and declared
/// combination rules can construct it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Measure {
    Literal {
        kind: String,
        value: u64,
    },
    SignedLiteral {
        kind: String,
        value: i64,
    },
    Symbol {
        kind: String,
        name: String,
    },
    Combined {
        kind: String,
        rule: MeasureRule,
        left: Box<Measure>,
        right: Box<Measure>,
    },
}

/// The closed combination algebra declared by measure-bearing type surfaces.
/// User code never selects a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeasureRule {
    Add,
    /// Scaling: a declared measure times a declared measure. A vector width of
    /// `(@lanes * 2)` is the shipped spelling this rule exists for.
    Mul,
    Match,
}

impl Measure {
    pub fn literal(kind: impl Into<String>, value: u64) -> Self {
        Self::Literal {
            kind: kind.into(),
            value,
        }
    }

    pub fn signed_literal(kind: impl Into<String>, value: i64) -> Self {
        Self::SignedLiteral {
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

    pub fn literal_value(&self) -> Option<u64> {
        match self {
            Self::Literal { value, .. } => Some(*value),
            Self::Combined {
                rule: MeasureRule::Add,
                left,
                right,
                ..
            } => left.literal_value()?.checked_add(right.literal_value()?),
            Self::Combined {
                rule: MeasureRule::Mul,
                left,
                right,
                ..
            } => left.literal_value()?.checked_mul(right.literal_value()?),
            _ => None,
        }
    }

    pub fn signed_literal_value(&self) -> Option<i64> {
        match self {
            Self::SignedLiteral { value, .. } => Some(*value),
            Self::Literal { value, .. } => i64::try_from(*value).ok(),
            Self::Combined {
                rule: MeasureRule::Add,
                left,
                right,
                ..
            } => left
                .signed_literal_value()?
                .checked_add(right.signed_literal_value()?),
            Self::Combined {
                rule: MeasureRule::Mul,
                left,
                right,
                ..
            } => left
                .signed_literal_value()?
                .checked_mul(right.signed_literal_value()?),
            _ => None,
        }
    }

    pub fn require_literal(&self) -> u64 {
        self.literal_value()
            .expect("symbolic measure reached runtime type lowering before specialization")
    }

    pub fn symbol_name(&self) -> Option<&str> {
        match self {
            Self::Symbol { name, .. } => Some(name),
            _ => None,
        }
    }

    pub fn combine(&self, rhs: &Self, rule: MeasureRule) -> Option<Self> {
        let (left_kind, right_kind) = (self.kind(), rhs.kind());
        if left_kind != right_kind {
            return None;
        }
        match rule {
            MeasureRule::Match => (self == rhs).then(|| self.clone()),
            MeasureRule::Add | MeasureRule::Mul => {
                match (self.literal_value(), rhs.literal_value()) {
                    (Some(left), Some(right)) => {
                        let folded = if matches!(rule, MeasureRule::Add) {
                            left.checked_add(right)
                        } else {
                            left.checked_mul(right)
                        };
                        folded.map(|value| Self::literal(left_kind, value))
                    }
                    _ => match (self.signed_literal_value(), rhs.signed_literal_value()) {
                        (Some(left), Some(right)) => {
                            let folded = if matches!(rule, MeasureRule::Add) {
                                left.checked_add(right)
                            } else {
                                left.checked_mul(right)
                            };
                            folded.map(|value| Self::signed_literal(left_kind, value))
                        }
                        _ => Some(Self::Combined {
                            kind: left_kind.to_string(),
                            rule,
                            left: Box::new(self.clone()),
                            right: Box::new(rhs.clone()),
                        }),
                    },
                }
            }
        }
    }

    pub fn resolve_symbols(&self, resolve: &impl Fn(&str) -> Option<u64>) -> Self {
        match self {
            Self::Symbol { kind, name } => resolve(name)
                .map(|value| Self::literal(kind, value))
                .unwrap_or_else(|| self.clone()),
            Self::Combined {
                rule, left, right, ..
            } => left
                .resolve_symbols(resolve)
                .combine(&right.resolve_symbols(resolve), *rule)
                .unwrap_or_else(|| self.clone()),
            _ => self.clone(),
        }
    }

    pub fn kind(&self) -> &str {
        match self {
            Self::Literal { kind, .. }
            | Self::SignedLiteral { kind, .. }
            | Self::Symbol { kind, .. }
            | Self::Combined { kind, .. } => kind,
        }
    }

    pub fn expression(&self) -> String {
        match self {
            Self::Literal { value, .. } => value.to_string(),
            Self::SignedLiteral { value, .. } => value.to_string(),
            Self::Symbol { name, .. } => name.clone(),
            Self::Combined {
                rule: MeasureRule::Add,
                left,
                right,
                ..
            } => format!("({} + {})", left.expression(), right.expression()),
            Self::Combined {
                rule: MeasureRule::Mul,
                left,
                right,
                ..
            } => format!("({} * {})", left.expression(), right.expression()),
            Self::Combined {
                rule: MeasureRule::Match,
                left,
                right,
                ..
            } => format!("match({}, {})", left.expression(), right.expression()),
        }
    }

    fn canonical(&self) -> String {
        format!("{}:{}", self.kind(), self.expression())
    }
}
impl std::fmt::Display for Measure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.expression())
    }
}

/// The exactness grade carried by a numeric type. Exactness is compile-time
/// knowledge; the runtime carrier remains the ordinary numeric type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exactness {
    Exact,
    Approximate { precision: u16 },
    Measured,
}

/// Function obligations are facts about how a callable may be used. The
/// D-APILABEL1 call contract is also part of callable identity; effects and
/// returned-view provenance remain sema-checked by subsumption.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FunctionObligations {
    pub effect_bound: Option<Vec<String>>,
    pub param_contract: Option<Vec<(String, super::ParamZone)>>,
    /// Declaration-ordered rest-slot facts. Local names and default bodies
    /// stay out of callable identity.
    pub variadic: Option<Vec<bool>>,
    pub return_view_provenance: Option<super::ViewProvenanceMap>,
}

/// One typed wrapper policy on a callable value. Built-in names remain the
/// compiler's initial vocabulary; package declarations add nominal names at
/// sema. Arguments are retained as their checked source shape so inspection
/// can report the exact chain without making a backend interpret policy syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallablePolicy {
    pub name: String,
    pub arguments: Vec<String>,
}

/// The declaration default or call-site replacement for a callable policy.
/// An empty chain is meaningful: it is the explicit bare-function form.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CallablePolicyChain {
    pub policies: Vec<CallablePolicy>,
}

impl CallablePolicyChain {
    pub const BUILTIN_NAMES: &'static [&'static str] =
        &["cache", "retry", "trace", "registration", "route"];
    /// Compatibility name for existing diagnostics/index consumers. The
    /// accepted vocabulary is open through the bundle's user declarations;
    /// this slice is only the built-in suggestion set.
    pub const NAMES: &'static [&'static str] = Self::BUILTIN_NAMES;

    pub fn is_builtin(name: &str) -> bool {
        Self::BUILTIN_NAMES.contains(&name)
    }

    /// D-STRUCT-POLICY1=A: compiler-private identity for one checked
    /// policy/target wrapper. Sema and TIR share this spelling.
    pub fn user_wrapper_name(policy: &str, target: &str) -> String {
        format!("__jet_policy_{policy}_{target}")
    }

    /// Parse the one typed policy value used by both declaration markers and
    /// `apply(...)`. A policy is a call expression, never one of the retired
    /// bare scoped-policy identifiers.
    pub fn parse(expressions: &[Expr]) -> Result<Self, String> {
        expressions
            .iter()
            .map(CallablePolicy::parse)
            .collect::<Result<Vec<_>, _>>()
            .map(|policies| Self { policies })
    }

    pub fn replace(&self) -> Self {
        self.clone()
    }

    pub fn display(&self) -> String {
        self.policies
            .iter()
            .map(CallablePolicy::display)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl CallablePolicy {
    fn parse(expr: &Expr) -> Result<Self, String> {
        let Expr::Call(call) = expr else {
            return Err("a callable policy is a call such as `retry(3)`".to_string());
        };
        if call
            .args
            .iter()
            .any(|arg| arg.label.is_some() || arg.spread)
        {
            return Err(format!(
                "`{}` policies take ordinary positional values",
                call.name
            ));
        }
        Ok(Self {
            name: call.name.clone(),
            arguments: call
                .args
                .iter()
                .map(|arg| policy_argument_text(&arg.expr))
                .collect(),
        })
    }

    pub fn display(&self) -> String {
        format!("{}({})", self.name, self.arguments.join(", "))
    }
}

fn policy_argument_text(expr: &Expr) -> String {
    match expr {
        Expr::Str(parts, _) if parts.len() == 1 => match &parts[0] {
            super::StrPart::Lit(value) => format!("{value:?}"),
            _ => "<string>".to_string(),
        },
        Expr::Int(value, _, _, source) => source.clone().unwrap_or_else(|| value.to_string()),
        Expr::Float(value, _, _, _) => value.to_string(),
        Expr::Bool(value, _) => value.to_string(),
        Expr::Char(value, _) => format!("'{value}'"),
        Expr::Ident(name, _) => name.clone(),
        Expr::Field(base, field, _) => format!("{}.{}", policy_argument_text(base), field),
        Expr::Call(call) => format!(
            "{}({})",
            call.name,
            call.args
                .iter()
                .map(|arg| policy_argument_text(&arg.expr))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        _ => "<expression>".to_string(),
    }
}

/// Declaration-side call slots carried by a function value. Public labels and
/// zones remain callable obligations; this row carries the defaults,
/// conventions, and rest slots needed to bind a value call.
#[derive(Debug, Clone, Default)]
pub struct FunctionCallMetadata {
    /// Declaration-local names used only to resolve default bodies. They are
    /// not public callable labels or semantic-index identity.
    pub names: Vec<String>,
    pub defaults: Vec<Option<Expr>>,
    pub variadic: Vec<bool>,
    pub conventions: Vec<AccessConvention>,
    /// Exact declaration default or call-site replacement chain.
    pub policies: CallablePolicyChain,
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
        let variadic = self.variadic.as_ref().map_or_else(String::new, |row| {
            row.iter()
                .map(|value| value.to_string())
                .collect::<Vec<_>>()
                .join(",")
        });
        let provenance = self
            .return_view_provenance
            .as_ref()
            .map(super::canonical_view_provenance_map)
            .unwrap_or_default();
        format!(
            "effects=[{effects}];contract=[{contract}];variadic=[{variadic}];provenance=[{provenance}]"
        )
    }

    /// required is the contract at the use site. offered is the contract
    /// carried by the function value. A value satisfies a contract when its
    /// obligations are at least as strong as the required ones.
    pub fn satisfies(&self, required: &Self) -> bool {
        let effects_ok = match &required.effect_bound {
            None => true,
            Some(required) => self
                .effect_bound
                .as_ref()
                .is_some_and(|offered| offered.iter().all(|effect| required.contains(effect))),
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
                        on == rn
                            && match (oz, rz) {
                                (super::ParamZone::Either, _) => true,
                                (offered, required) => offered == required,
                            }
                    })
            }),
        };
        if !contract_ok {
            return false;
        }

        let variadic_ok = match &required.variadic {
            None => true,
            Some(required) => self
                .variadic
                .as_ref()
                .is_none_or(|offered| offered == required),
        };
        if !variadic_ok {
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
    Exactness(Exactness),
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
            Self::Exactness(exactness) => match exactness {
                Exactness::Exact => "exactness:exact".to_string(),
                Exactness::Approximate { precision } => {
                    format!("exactness:approximate:{precision}")
                }
                Exactness::Measured => "exactness:measured".to_string(),
            },
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
            left.path.cmp(&right.path).then_with(|| {
                left.plane
                    .cmp(right.plane)
                    .then_with(|| left.fact.canonical().cmp(&right.fact.canonical()))
            })
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
                    if entry.plane != crate::Registry::type_plane("Obligation") {
                        return None;
                    }
                    // D-APILABEL1/D-VARIADIC1: this mixed plane contributes
                    // only declaration-ordered public call facts to identity;
                    // effects, defaults, and return provenance remain
                    // directional or local implementation details.
                    let KnowledgeFact::Obligation(obligations) = &entry.fact else {
                        return None;
                    };
                    if obligations.param_contract.is_none() && obligations.variadic.is_none() {
                        return None;
                    }
                    Some(KnowledgeEntry {
                        path: entry.path.clone(),
                        plane: entry.plane,
                        fact: KnowledgeFact::Obligation(FunctionObligations {
                            effect_bound: None,
                            param_contract: obligations.param_contract.clone(),
                            variadic: obligations.variadic.clone(),
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
        let interval_plane = crate::Registry::type_plane("Interval");
        self.entries.iter().find_map(|entry| {
            (entry.plane == interval_plane && entry.path.is_empty())
                .then_some(&entry.fact)
                .and_then(|fact| match fact {
                    KnowledgeFact::Interval { lo, hi } => Some((*lo, *hi)),
                    _ => None,
                })
        })
    }

    pub fn interval(&self) -> Option<(i64, i64)> {
        let (lo, hi) = self.interval_i128()?;
        Some((lo.try_into().ok()?, hi.try_into().ok()?))
    }

    pub fn obligations(&self) -> Option<&FunctionObligations> {
        self.entries
            .iter()
            .find_map(|entry| match (&entry.path.is_empty(), &entry.fact) {
                (true, KnowledgeFact::Obligation(obligations)) => Some(obligations),
                _ => None,
            })
    }

    pub fn from_interval(lo: i64, hi: i64) -> Self {
        let mut vector = Self::new();
        vector.push(
            crate::Registry::type_plane("Interval"),
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
    /// D-ALLOCFAIL1=A: the compiler-only mutable reference carrier returned by
    /// `mem.*.try_alloc`. It stays transparent to Jet's `T !AllocError`
    /// identity while preserving the allocator slot through every tier.
    AllocatorView,
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
            InternalTag::AllocatorView => "allocator.view",
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
    /// S38/D-LISTMAP-CANON1=A / D-MAPSPACE1=A: keyed collection `[K:V]`.
    Map {
        key: Box<Type>,
        /// Parser-owned boundary of the key inside `[K:V]`.
        /// Synthesized map types carry `None`; diagnostics then fall back to
        /// the enclosing type span.
        key_span: Option<Span>,
        value: Box<Type>,
    },
    Shared(Box<Type>),
    /// S32: `?T` optional value.
    Option(Box<Type>),
    /// D-FAILURE-FOUNDATION1: one result carrier for a success type and
    /// prefixed error contract. `?T` is optional success; `!E` is the error
    /// contract.
    /// Internally lowered through Rust `Result<T, E>`.
    Result {
        ok: Box<Type>,
        err: Box<Type>,
    },
    /// S47 (M8): function type `fn(T1, T2) R` (`ret` omitted = no return value).
    ///
    /// D-EFF2 / D-SHAPE8: an optional effect row follows the function type's
    /// return type — `fn(T) U -[]>` requires purity and `fn(T) U -[Net]>`
    /// permits at most the listed effects. `effect_bound`
    /// is `None` when unannotated, `Some(empty)` for `-[]>`, and
    /// `Some([(name, span), …])` for a nonempty row. Names are validated
    /// against the effect vocabulary in sema, not the parser. The bound is a
    /// call-site obligation on whatever callback is passed (E0747) — it is **not**
    /// part of structural type identity (see the manual `PartialEq for Type`,
    /// which ignores it in the `Fn` arm), so `fn(Int) -[]>` and `fn(Int)` are the
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
        /// D-APILABEL1/D-NARG-D2/D-VARIADIC1: declaration-side call slots for
        /// function-value calls.
        call_metadata: Option<FunctionCallMetadata>,
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
    /// S76 / D-TYPE2-MEASURE1=A: fixed-size list `[T#N]`. `N` resolves through
    /// the ordinary closed comptime evaluator, then joins the same measure
    /// substrate used by shapes, lanes, and exponents.
    FixedList {
        elem: Box<Type>,
        len: Measure,
    },
    /// D-SG9/S42: explicit fixed-width integer. `Int` is exact and arbitrary
    /// precision; every fixed width, including `I64`, is an `IntN`.
    IntN {
        signed: bool,
        bits: u8,
    },
    /// D-TYPE2-SPELL1 / card #1549: an inline value-range refinement. The
    /// interval is knowledge, not a runtime wrapper; the shared TIR boundary
    /// removes it before tier-specific code generation.
    InlineRange {
        base: Box<Type>,
        lo: i64,
        hi: i64,
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
    /// D-TYPE2-MEASURE1=A: one compile-time measure in a type position.
    /// Shape arguments use this same node as list lengths, lanes, and
    /// dimension exponents; the owning surface supplies the combination rule.
    Measure(Measure),
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

/// D-SG9/D-INTBIG1: parse an exact default numeric spelling or a fixed width.
/// `I64` is fixed-width like the other integer spellings; `None` means the name
/// is not numeric.
pub fn numeric_type_from_name(name: &str) -> Option<Type> {
    match name {
        "Int" => Some(Type::Int),
        "I64" => Some(Type::IntN {
            signed: true,
            bits: 64,
        }),
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
    row.iter()
        .map(|(name, _)| name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
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
        parts.push(label.map_or_else(
            || param.name(),
            |label| format!("{label}: {}", param.name()),
        ));
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
    /// Project every nested Jet function value onto the shared callable return
    /// carrier. A written `fn(T) U` describes the same callable shape as an
    /// ordinary `fn` declaration returning `U`, whose executable return is
    /// `Result<U, Err>` by default. Keep C++ callback ABI tags raw: that tag is
    /// an already-decided foreign boundary, not an ordinary Jet callable.
    pub fn with_effective_fn_returns(&self) -> Type {
        fn normalize(ty: &Type) -> Type {
            match ty {
                Type::List(inner) => Type::List(Box::new(normalize(inner))),
                Type::Map {
                    key,
                    key_span,
                    value,
                } => Type::Map {
                    key: Box::new(normalize(key)),
                    key_span: *key_span,
                    value: Box::new(normalize(value)),
                },
                Type::Shared(inner) => Type::Shared(Box::new(normalize(inner))),
                Type::Option(inner) => Type::Option(Box::new(normalize(inner))),
                Type::Result { ok, err } => Type::Result {
                    ok: Box::new(normalize(ok)),
                    err: Box::new(normalize(err)),
                },
                Type::Fn {
                    params,
                    ret,
                    effect_bound,
                    param_contract,
                    call_metadata,
                    return_view_provenance,
                } => Type::Fn {
                    params: params.iter().map(normalize).collect(),
                    ret: ret.as_ref().map(|ret| {
                        let ret = normalize(ret);
                        Box::new(FailureContract::from_return_type(Some(&ret)).effective_type())
                    }),
                    effect_bound: effect_bound.clone(),
                    param_contract: param_contract.clone(),
                    call_metadata: call_metadata.clone(),
                    return_view_provenance: return_view_provenance.clone(),
                },
                Type::Apply { name, args } => Type::Apply {
                    name: name.clone(),
                    args: args.iter().map(normalize).collect(),
                },
                Type::Tuple(fields) => Type::Tuple(
                    fields
                        .iter()
                        .map(|(name, ty)| (name.clone(), Box::new(normalize(ty))))
                        .collect(),
                ),
                Type::FixedList { elem, len } => Type::FixedList {
                    elem: Box::new(normalize(elem)),
                    len: len.clone(),
                },
                Type::InlineRange { base, lo, hi } => Type::InlineRange {
                    base: Box::new(normalize(base)),
                    lo: *lo,
                    hi: *hi,
                },
                Type::Tagged { marker, inner }
                    if matches!(
                        marker,
                        TagMarker::Internal(InternalTag::CppCallbackAbi)
                    ) => self_tagged_clone(marker, inner),
                Type::Tagged { marker, inner } => Type::Tagged {
                    marker: marker.clone(),
                    inner: Box::new(normalize(inner)),
                },
                Type::Union(members) => Type::Union(members.iter().map(normalize).collect()),
                Type::Quantity { base, dimension } => Type::Quantity {
                    base: Box::new(normalize(base)),
                    dimension: dimension.clone(),
                },
                other => other.clone(),
            }
        }

        fn self_tagged_clone(marker: &TagMarker, inner: &Type) -> Type {
            Type::Tagged {
                marker: marker.clone(),
                inner: Box::new(inner.clone()),
            }
        }

        normalize(self)
    }

    /// Replace only the callable policy chain. Every other `Type::Fn` field is
    /// cloned unchanged, so labels, defaults, access, effects, variadics, and
    /// returned-view provenance cannot be laundered by `apply`.
    pub fn replace_callable_policies(&self, policies: CallablePolicyChain) -> Option<Type> {
        let Type::Fn {
            params,
            ret,
            effect_bound,
            param_contract,
            call_metadata,
            return_view_provenance,
        } = self
        else {
            return None;
        };
        let mut metadata = call_metadata.clone().unwrap_or_default();
        metadata.policies = policies;
        Some(Type::Fn {
            params: params.clone(),
            ret: ret.clone(),
            effect_bound: effect_bound.clone(),
            param_contract: param_contract.clone(),
            call_metadata: Some(metadata),
            return_view_provenance: return_view_provenance.clone(),
        })
    }

    pub fn callable_policies(&self) -> Option<&CallablePolicyChain> {
        match self {
            Type::Fn { call_metadata, .. } => {
                call_metadata.as_ref().map(|metadata| &metadata.policies)
            }
            _ => None,
        }
    }

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

    /// D-ALLOCFAIL1=A: retain the live allocator slot behind a fallible
    /// allocation result without exposing a second source-level type.
    pub fn allocator_view(inner: Type) -> Type {
        Type::Tagged {
            marker: TagMarker::Internal(InternalTag::AllocatorView),
            inner: Box::new(inner),
        }
    }

    /// True for the compiler-only mutable reference carrier used by
    /// `try_alloc`'s success branch.
    pub fn is_allocator_view(&self) -> bool {
        matches!(
            self,
            Type::Tagged {
                marker: TagMarker::Internal(InternalTag::AllocatorView),
                ..
            }
        )
    }

    /// True for `Result<allocator_view<T>, AllocError>`.
    pub fn is_allocator_result(&self) -> bool {
        matches!(
            self,
            Type::Result { ok, .. } if ok.is_allocator_view()
        )
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
            (Type::List(left), Type::List(right)) | (Type::Option(left), Type::Option(right)) => {
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
            Type::Int => {
                vector.push(
                    crate::Registry::type_plane("Exactness"),
                    KnowledgeFact::Exactness(Exactness::Exact),
                );
            }
            Type::Float => {
                vector.push(
                    crate::Registry::type_plane("Exactness"),
                    KnowledgeFact::Exactness(Exactness::Approximate { precision: 53 }),
                );
            }
            Type::Float32 => {
                vector.push(
                    crate::Registry::type_plane("Exactness"),
                    KnowledgeFact::Exactness(Exactness::Approximate { precision: 24 }),
                );
            }
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
                    crate::Registry::type_plane("Interval"),
                    KnowledgeFact::Interval { lo, hi },
                );
                vector.push(
                    crate::Registry::type_plane("Layout"),
                    KnowledgeFact::Layout {
                        bytes: (*bits + 7) / 8,
                    },
                );
                vector.push(
                    crate::Registry::type_plane("Exactness"),
                    KnowledgeFact::Exactness(Exactness::Exact),
                );
            }
            Type::InlineRange { base, lo, hi } => {
                vector.extend(&base.knowledge_vector());
                vector.push(
                    crate::Registry::type_plane("Interval"),
                    KnowledgeFact::Interval {
                        lo: i128::from(*lo),
                        hi: i128::from(*hi),
                    },
                );
            }
            Type::FixedList { elem, len } => {
                vector.extend_at(&["element".to_string()], &elem.knowledge_vector());
                vector.push(
                    crate::Registry::type_plane("Measure"),
                    KnowledgeFact::Measure(len.clone()),
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
                        crate::Registry::type_plane("Obligation"),
                        KnowledgeFact::Obligation(obligations),
                    );
                }
            }
            Type::Quantity { base, dimension } => {
                vector.extend_at(&["base".to_string()], &base.knowledge_vector());
                vector.push(
                    crate::Registry::type_plane("Dimension"),
                    KnowledgeFact::Dimension(dimension.clone()),
                );
                for (axis, measure) in dimension.measure_exponents() {
                    vector.push_at(
                        &["exponent".to_string(), axis.to_string()],
                        crate::Registry::type_plane("Measure"),
                        KnowledgeFact::Measure(measure),
                    );
                }
            }
            Type::Measure(measure) => {
                vector.push(
                    crate::Registry::type_plane("Measure"),
                    KnowledgeFact::Measure(measure.clone()),
                );
            }
            Type::Tagged { marker, inner } if is_core_crypto(marker) => {
                vector.extend_at(&["inner".to_string()], &inner.knowledge_vector());
                vector.push(
                    crate::Registry::type_plane("Nominal"),
                    KnowledgeFact::Nominal(marker.to_string()),
                );
            }
            Type::Tagged { marker, inner } => {
                // User tags are transparent flow classifications. Do not add
                // a structural path around the identity-bearing inner facts.
                vector.extend(&inner.knowledge_vector());
                vector.push(
                    crate::Registry::type_plane("Classification"),
                    KnowledgeFact::Classification(marker.to_string()),
                );
            }
            Type::Apply { name, args }
                if name == crate::Syntax::TYPE_MEASUREMENT && args.len() == 1 =>
            {
                vector.extend_at(&["value".to_string()], &args[0].knowledge_vector());
                vector.push(
                    crate::Registry::type_plane("Exactness"),
                    KnowledgeFact::Exactness(Exactness::Measured),
                );
            }
            Type::Apply { name, args } => {
                for (index, arg) in args.iter().enumerate() {
                    if matches!(name.as_str(), "Vec" | "Matrix") && matches!(arg, Type::Measure(_))
                    {
                        continue;
                    }
                    vector.extend_at(
                        &["argument".to_string(), index.to_string()],
                        &arg.knowledge_vector(),
                    );
                }
                if let Some(measures) = self.compute_shape_measures() {
                    for (index, measure) in measures.into_iter().enumerate() {
                        vector.push_at(
                            &["shape".to_string(), index.to_string()],
                            crate::Registry::type_plane("Measure"),
                            KnowledgeFact::Measure(measure),
                        );
                    }
                }
            }
            Type::Named(name) => {
                if matches!(
                    name.as_str(),
                    crate::Syntax::TYPE_DECIMAL | crate::Syntax::TYPE_FRACTION
                ) {
                    vector.push(
                        crate::Registry::type_plane("Exactness"),
                        KnowledgeFact::Exactness(Exactness::Exact),
                    );
                }
                let lanes = crate::Syntax::simd_lane_arity(name);
                if let Some(lanes) = lanes {
                    vector.push(
                        crate::Registry::type_plane("Measure"),
                        KnowledgeFact::Measure(Measure::literal("lane", lanes as u64)),
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
            call_metadata,
            return_view_provenance,
            ..
        } = self
        else {
            return None;
        };
        let variadic = call_metadata.as_ref().and_then(|metadata| {
            metadata
                .variadic
                .iter()
                .any(|is_variadic| *is_variadic)
                .then(|| metadata.variadic.clone())
        });
        if effect_bound.is_none()
            && param_contract.is_none()
            && variadic.is_none()
            && return_view_provenance.is_none()
        {
            return None;
        }
        Some(FunctionObligations {
            effect_bound: effect_bound
                .as_ref()
                .map(|row| row.iter().map(|(name, _)| name.clone()).collect()),
            param_contract: param_contract.clone(),
            variadic,
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
                ) => nested(required_key, offered_key) && nested(required_value, offered_value),
                (Type::Tuple(required_fields), Type::Tuple(offered_fields)) => {
                    required_fields.len() == offered_fields.len()
                        && required_fields.iter().zip(offered_fields).all(
                            |((required_name, required), (offered_name, offered))| {
                                required_name == offered_name && nested(required, offered)
                            },
                        )
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
            Type::Map {
                key,
                key_span,
                value,
            } => Type::Map {
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
            Type::InlineRange { base, .. } => base.erased_carrier(),
            Type::Measure(_) => Type::Int,
            Type::Tagged { inner, .. } => inner.erased_carrier(),
            Type::Fn { params, ret, .. } => Type::Fn {
                params: params.iter().map(Type::erased_carrier).collect(),
                ret: ret
                    .as_ref()
                    .map(|return_type| Box::new(return_type.erased_carrier())),
                effect_bound: None,
                param_contract: None,
                call_metadata: None,
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
            Type::Union(members) => {
                canonicalize_union(members.iter().map(Type::erased_carrier).collect())
            }
            _ => self.clone(),
        }
    }

    /// Remove only inline-range knowledge while preserving the surrounding
    /// carrier, compiler tags, callable contracts, and fixed-list shape.
    /// TIR uses this narrower projection at its shared boundary; the broader
    /// `erased_carrier` operation remains available to ABI/layout consumers.
    pub fn erased_inline_ranges(&self) -> Type {
        match self {
            Type::List(inner) => Type::List(Box::new(inner.erased_inline_ranges())),
            Type::Map {
                key,
                key_span,
                value,
            } => Type::Map {
                key: Box::new(key.erased_inline_ranges()),
                key_span: *key_span,
                value: Box::new(value.erased_inline_ranges()),
            },
            Type::Shared(inner) => Type::Shared(Box::new(inner.erased_inline_ranges())),
            Type::Option(inner) => Type::Option(Box::new(inner.erased_inline_ranges())),
            Type::Result { ok, err } => Type::Result {
                ok: Box::new(ok.erased_inline_ranges()),
                err: Box::new(err.erased_inline_ranges()),
            },
            Type::Fn {
                params,
                ret,
                effect_bound,
                param_contract,
                call_metadata,
                return_view_provenance,
            } => Type::Fn {
                params: params.iter().map(Type::erased_inline_ranges).collect(),
                ret: ret
                    .as_ref()
                    .map(|return_type| Box::new(return_type.erased_inline_ranges())),
                effect_bound: effect_bound.clone(),
                param_contract: param_contract.clone(),
                call_metadata: call_metadata.clone(),
                return_view_provenance: return_view_provenance.clone(),
            },
            Type::Apply { name, args } => Type::Apply {
                name: name.clone(),
                args: args.iter().map(Type::erased_inline_ranges).collect(),
            },
            Type::Tuple(fields) => Type::Tuple(
                fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), Box::new(ty.erased_inline_ranges())))
                    .collect(),
            ),
            Type::FixedList { elem, len } => Type::FixedList {
                elem: Box::new(elem.erased_inline_ranges()),
                len: len.clone(),
            },
            Type::InlineRange { base, .. } => base.erased_inline_ranges(),
            Type::Tagged { marker, inner } => Type::Tagged {
                marker: marker.clone(),
                inner: Box::new(inner.erased_inline_ranges()),
            },
            Type::Union(members) => {
                canonicalize_union(members.iter().map(Type::erased_inline_ranges).collect())
            }
            Type::Quantity { base, dimension } => Type::Quantity {
                base: Box::new(base.erased_inline_ranges()),
                dimension: dimension.clone(),
            },
            _ => self.clone(),
        }
    }

    fn carrier_identity_name(&self) -> String {
        match self {
            Type::List(inner) => format!("[{}]", inner.carrier_identity_name()),
            Type::Map { key, value, .. } => format!(
                "[{}:{}]",
                key.carrier_identity_name(),
                value.carrier_identity_name()
            ),
            Type::Shared(inner) => format!("Shared<{}>", inner.carrier_identity_name()),
            Type::Option(inner) => format!("?{}", inner.carrier_identity_name()),
            Type::Result { ok, err } => {
                format!(
                    "{} ! {}",
                    ok.carrier_identity_name(),
                    err.carrier_identity_name()
                )
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
            Type::InlineRange { base, .. } => base.carrier_identity_name(),
            Type::Tagged { inner, .. } => inner.carrier_identity_name(),
            Type::Union(members) => members
                .iter()
                .map(Type::carrier_identity_name)
                .collect::<Vec<_>>()
                .join(" | "),
            Type::Quantity { base, .. } => base.carrier_identity_name(),
            Type::Measure(_) => "Int".to_string(),
            _ => self.name(),
        }
    }

    /// D-TYPE2-MEASURE1=A: preserve a fixed shape measure in the type tree.
    pub fn compute_dimension_type(value: u64) -> Type {
        Type::Measure(Measure::literal("shape", value))
    }

    /// Return a fixed shape measure, if this is a compiler-owned measure node.
    pub fn compute_dimension_value(&self) -> Option<u64> {
        match self {
            Type::Measure(measure) if measure.kind() == "shape" => measure.literal_value(),
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

    /// Return the literal or symbolic measures carried by a compute shape.
    pub fn compute_shape_measures(&self) -> Option<Vec<Measure>> {
        let Type::Apply { name, args } = self else {
            return None;
        };
        let expected = match name.as_str() {
            "Vec" => 1,
            "Matrix" => 2,
            _ => return None,
        };
        if args.len() != expected {
            return None;
        }
        args.iter()
            .map(|argument| match argument {
                Type::Measure(measure) if measure.kind() == "shape" => Some(measure.clone()),
                _ => None,
            })
            .collect()
    }

    /// Return resolved dimensions carried by `Vec<N>` or `Matrix<M, N>`.
    pub fn compute_shape_dimensions(&self) -> Option<Vec<u64>> {
        self.compute_shape_measures()?
            .iter()
            .map(Measure::literal_value)
            .collect()
    }

    /// Compute values all share the `JetTensor` storage substrate. An erased
    /// `Tensor` is compatible with a shaped alias; two shaped aliases still
    /// require exact equality, so `Vec<3>` cannot silently become `Vec<4>`.
    pub fn is_compute_tensor_family(&self) -> bool {
        match self {
            Type::Tagged { inner, .. } => inner.is_compute_tensor_family(),
            Type::Named(name) => name == "Tensor",
            Type::Apply { name, args } if name == "Tensor" => args.len() <= 1,
            Type::Apply { name, args } if matches!(name.as_str(), "Vec" | "Matrix") => {
                let expected = if name == "Vec" { 1 } else { 2 };
                args.len() == expected
                    // Sema retains the fixed Measure facts. TIR receives
                    // only their erased Int carrier, so both exact forms
                    // belong to the same compiler-owned compute family.
                    && (self.compute_shape_dimensions().is_some()
                        || args.iter().all(|arg| matches!(arg, Type::Int)))
            }
            _ => false,
        }
    }

    /// Compiler-internal mutable Tensor windows retain the Tensor owner and
    /// original range through all backends. This is not a user-typeable form;
    /// lowering introduces it for a written Tensor place.
    pub fn is_compute_view_mut(&self) -> bool {
        matches!(
            self,
            Type::Apply { name, args }
                if name == "ComputeViewMut"
                    && args.len() == 1
                    && matches!(args.first(), Some(Type::Float))
        )
    }

    pub fn compute_tensor_compatible(want: &Type, got: &Type) -> bool {
        if want == got {
            return true;
        }
        (matches!(want, Type::Named(name) if name == "Tensor") && got.is_compute_tensor_family())
            || (matches!(got, Type::Named(name) if name == "Tensor")
                && want.is_compute_tensor_family())
    }

    /// Recursively rewrite nominal leaves while preserving every container and
    /// callback shape. Import resolution uses this to attach module identity to
    /// unit-family members in signatures.
    pub fn map_named_types(&self, map: &impl Fn(&str) -> Option<String>) -> Type {
        match self {
            Type::Named(name) => map(name).map_or_else(|| self.clone(), Type::Named),
            Type::List(inner) => Type::List(Box::new(inner.map_named_types(map))),
            Type::Map {
                key,
                key_span,
                value,
            } => Type::Map {
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
            Type::Fn {
                params,
                ret,
                effect_bound,
                param_contract,
                call_metadata,
                return_view_provenance,
            } => Type::Fn {
                params: params.iter().map(|ty| ty.map_named_types(map)).collect(),
                ret: ret.as_ref().map(|ty| Box::new(ty.map_named_types(map))),
                effect_bound: effect_bound.clone(),
                param_contract: param_contract.clone(),
                call_metadata: call_metadata.clone(),
                return_view_provenance: return_view_provenance.clone(),
            },
            Type::Apply { name, args } => Type::Apply {
                name: name.clone(),
                args: args.iter().map(|ty| ty.map_named_types(map)).collect(),
            },
            Type::Tuple(fields) => Type::Tuple(
                fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), Box::new(ty.map_named_types(map))))
                    .collect(),
            ),
            Type::FixedList { elem, len } => Type::FixedList {
                elem: Box::new(elem.map_named_types(map)),
                len: len.clone(),
            },
            Type::InlineRange { base, lo, hi } => Type::InlineRange {
                base: Box::new(base.map_named_types(map)),
                lo: *lo,
                hi: *hi,
            },
            Type::Tagged { marker, inner } => Type::Tagged {
                marker: marker.clone(),
                inner: Box::new(inner.map_named_types(map)),
            },
            Type::Union(members) => {
                canonicalize_union(members.iter().map(|m| m.map_named_types(map)).collect())
            }
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

    fn error_contract_name(err: &Type) -> String {
        if let Type::Union(members) = err {
            format!(
                "({})",
                members
                    .iter()
                    .map(|member| member.name())
                    .collect::<Vec<_>>()
                    .join(" | ")
            )
        } else {
            err.name()
        }
    }

    fn result_surface_name(ok: &Type, err: &Type) -> String {
        let default_error = matches!(err, Type::Named(name) if name == crate::Syntax::TYPE_ERR);
        let unit_success =
            matches!(ok, Type::Named(name) if name == crate::Syntax::INTERNAL_UNIT_TYPE);
        let error = if default_error {
            None
        } else {
            Some(Self::error_contract_name(err))
        };
        if unit_success {
            error.map_or_else(
                || format!("{}{}", crate::Syntax::TYPE_FALLIBLE_SEP, crate::Syntax::TYPE_ERR),
                |error| format!("{}{}", crate::Syntax::TYPE_FALLIBLE_SEP, error),
            )
        } else if let Some(error) = error {
            format!(
                "{} {}{}",
                ok.name(),
                crate::Syntax::TYPE_FALLIBLE_SEP,
                error
            )
        } else {
            ok.name()
        }
    }

    /// Plain-words name for diagnostics (docs/spec/diagnostics.md voice: name both types).
    pub fn show(&self) -> String {
        match self {
            Type::Int => "Int (a whole number)".to_string(),
            Type::Float => "Float (an approximate binary number)".to_string(),
            Type::Bool => "Bool (true or false)".to_string(),
            Type::String => "String (text)".to_string(),
            Type::Char => "Char (one character)".to_string(),
            Type::List(inner) => format!("[{}]", inner.name()),
            Type::Map { key, value, .. } => format!("[{}:{}]", key.name(), value.name()),
            Type::Shared(inner) => format!("Shared<{}>", inner.name()),
            Type::Option(inner) => format!("?{}", inner.name()),
            Type::Result { ok, err } => Self::result_surface_name(ok, err),
            Type::Fn {
                params,
                ret,
                effect_bound,
                param_contract,
                ..
            } => {
                let ps = fn_param_names(params, param_contract.as_deref());
                let mut signature = format!("fn({ps})");
                if let Some(r) = ret {
                    signature.push(' ');
                    signature.push_str(&r.name());
                }
                if let Some(row) = effect_bound {
                    signature.push_str(" -[");
                    signature.push_str(&effect_names(row));
                    signature.push_str("]>");
                }
                signature
            }
            Type::Named(n) if n == crate::Syntax::TYPE_DECIMAL => {
                "Decimal (an exact base-10 number)".to_string()
            }
            Type::Named(n) => format!("`{}`", n),
            Type::Measure(measure) => format!("{} (a compile-time measure)", measure.expression()),
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
                        dimensions
                            .iter()
                            .map(u64::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
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
            Type::FixedList { elem, len } => format!("[{}#{}]", elem.name(), len.expression()),
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
            Type::InlineRange { base, lo, hi } => {
                format!("{} (a whole number from {} to {})", base.name(), lo, hi)
            }
            Type::Float32 => "F32 (a 32-bit approximate binary number)".to_string(),
            Type::Tagged {
                marker: TagMarker::Internal(_),
                inner,
            } => inner.show(),
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
            Type::Map { key, value, .. } => format!("[{}:{}]", key.name(), value.name()),
            Type::Shared(inner) => format!("Shared<{}>", inner.name()),
            Type::Option(inner) => format!("?{}", inner.name()),
            Type::Result { ok, err } => Self::result_surface_name(ok, err),
            Type::Fn {
                params,
                ret,
                effect_bound,
                param_contract,
                ..
            } => {
                let ps = fn_param_names(params, param_contract.as_deref());
                let mut signature = format!("fn({ps})");
                if let Some(r) = ret {
                    signature.push(' ');
                    signature.push_str(&r.name());
                }
                if let Some(row) = effect_bound {
                    signature.push_str(" -[");
                    signature.push_str(&effect_names(row));
                    signature.push_str("]>");
                }
                signature
            }
            Type::Named(n) => n.clone(),
            Type::Measure(measure) => measure.expression(),
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
                        dimensions
                            .iter()
                            .map(u64::to_string)
                            .collect::<Vec<_>>()
                            .join(", ")
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
            Type::FixedList { elem, len } => format!("[{}#{}]", elem.name(), len.expression()),
            Type::IntN { signed, bits } => int_spelling(*signed, *bits),
            Type::InlineRange { base, lo, hi } => format!("{}({lo}..{hi})", base.name()),
            Type::Float32 => "F32".to_string(),
            Type::Tagged {
                marker: TagMarker::Internal(_),
                inner,
            } => inner.name(),
            Type::Tagged { marker, inner } => format!("#{} {}", marker, inner.name()),
            Type::Union(members) => members
                .iter()
                .map(|m| m.name())
                .collect::<Vec<_>>()
                .join(" | "),
        }
    }

    /// User-facing leaf spelling of a nominal type. Generic arguments keep
    /// their full spelling; only the nominal head loses its module qualifier.
    pub fn leaf_name(&self) -> String {
        match self {
            Type::Named(name) => name
                .rsplit_once('.')
                .map_or_else(|| name.clone(), |(_, leaf)| leaf.to_string()),
            Type::Apply { name, .. } => {
                let full = self.name();
                let leaf = name
                    .rsplit_once('.')
                    .map_or(name.as_str(), |(_, leaf)| leaf);
                full.strip_prefix(name.as_str())
                    .map_or(full.clone(), |suffix| format!("{leaf}{suffix}"))
            }
            _ => self.name(),
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
            Type::InlineRange { base, .. } => base.is_scalar(),
            Type::Apply { name, args } if name == crate::Syntax::TYPE_PTR && args.len() == 1 => {
                true
            }
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
            Type::InlineRange { base, .. } => base.is_integer(),
            _ => matches!(self, Type::Int | Type::IntN { .. }),
        }
    }

    /// Bounds projected from the interval plane for an integer carrier.
    /// Exact `Int` has no finite static bounds.
    pub fn integer_range(&self) -> Option<(i128, i128)> {
        match self {
            Type::Int => None,
            Type::IntN { .. } | Type::InlineRange { .. } => self.knowledge_vector().interval_i128(),
            Type::Tagged { inner, .. } => inner.integer_range(),
            _ => None,
        }
    }

    fn integer_layout(&self) -> Option<(bool, u8)> {
        match self {
            Type::Int => Some((true, 64)),
            Type::IntN { signed, bits } => Some((*signed, *bits)),
            Type::InlineRange { base, .. } => base.integer_layout(),
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
            // An inline range is a transparent proof on its carrier. Once
            // interval containment does not discharge the move, the ordinary
            // carrier widening still applies (for example, Int(0..10) to Int).
            (Type::InlineRange { base, .. }, target) => base.numeric_widening_to(target),
            // Exact Int can cross into a fixed width only through the checked
            // destination conversion. Every fixed width widens exactly into
            // the arbitrary-precision carrier.
            (Type::IntN { .. }, Type::Int) => Some(false),
            (Type::Int, Type::IntN { .. }) => Some(true),
            (Type::Float32, Type::Float) => Some(false),
            (Type::Int, Type::Float | Type::Float32) => Some(true),
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
        matches!(self, Type::Option(_))
            || matches!(self, Type::Result { err, .. }
                if !matches!(err.as_ref(), Type::Named(name) if name == crate::Syntax::TYPE_NEVER))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        numeric_type_from_name, AccessConvention, CallablePolicy, CallablePolicyChain, Dimension,
        FunctionCallMetadata, InternalTag, KnowledgeFact, KnowledgeVector, Measure, TagMarker,
        Type,
    };
    use crate::Diagnostics::Span;
    use crate::AST::{Expr, ParamZone, StrPart};

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
            call_metadata: None,
            return_view_provenance: None,
        };
        let labelled = Type::Fn {
            params: vec![Type::Bool],
            ret: Some(Box::new(Type::Int)),
            effect_bound: None,
            param_contract: Some(vec![("force".to_string(), ParamZone::LabelOnly)]),
            call_metadata: None,
            return_view_provenance: None,
        };

        assert_ne!(bare, labelled);
        assert_eq!(bare.name(), "fn(Bool) Int");
        assert_eq!(labelled.name(), "fn(*, force: Bool) Int");
        assert_eq!(labelled.show(), "fn(*, force: Bool) Int");
        assert_eq!(Type::Named("dep.Point".to_string()).leaf_name(), "Point");
        assert_eq!(
            Type::Apply {
                name: "dep.Box".to_string(),
                args: vec![Type::Named("other.Item".to_string())],
            }
            .leaf_name(),
            "Box<other.Item>"
        );
    }

    #[test]
    fn failure_surface_names_use_prefixes_in_diagnostics() {
        let fallible = Type::Result {
            ok: Box::new(Type::Option(Box::new(Type::Int))),
            err: Box::new(Type::Union(vec![
                Type::Named("DbError".to_string()),
                Type::Named("TimeoutError".to_string()),
            ])),
        };
        assert_eq!(fallible.name(), "?Int !(DbError | TimeoutError)");
        assert_eq!(fallible.show(), "?Int !(DbError | TimeoutError)");

        let unit_fallible = Type::Result {
            ok: Box::new(Type::Named(crate::Syntax::INTERNAL_UNIT_TYPE.to_string())),
            err: Box::new(Type::Named("IOError".to_string())),
        };
        assert_eq!(unit_fallible.name(), "!IOError");
        assert_eq!(
            Type::Result {
                ok: Box::new(Type::Int),
                err: Box::new(Type::Named(crate::Syntax::TYPE_ERR.to_string())),
            }
            .name(),
            "Int"
        );

        let callback = Type::Fn {
            params: vec![fallible],
            ret: Some(Box::new(unit_fallible)),
            effect_bound: Some(Vec::new()),
            param_contract: None,
            call_metadata: None,
            return_view_provenance: None,
        };
        assert_eq!(
            callback.name(),
            "fn(?Int !(DbError | TimeoutError)) !IOError -[]>"
        );
    }

    #[test]
    fn callable_policy_replacement_preserves_the_complete_function_contract() {
        let original_return_view_provenance = Some(
            [(
                Vec::new(),
                crate::AST::ViewProvenance {
                    sources: [crate::AST::ViewSourcePath {
                        source: crate::AST::ViewSource::Parameter(0),
                        projections: Vec::new(),
                    }]
                    .into_iter()
                    .collect(),
                    mutable: true,
                },
            )]
            .into_iter()
            .collect(),
        );
        let original = Type::Fn {
            params: vec![Type::List(Box::new(Type::String))],
            ret: Some(Box::new(Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::Named("NetError".to_string())),
            })),
            effect_bound: Some(vec![("Net".to_string(), Span::new(1, 2))]),
            param_contract: Some(vec![("users".to_string(), ParamZone::LabelOnly)]),
            call_metadata: Some(FunctionCallMetadata {
                names: vec!["path".to_string()],
                defaults: vec![Some(Expr::Str(
                    vec![StrPart::Lit("/me".to_string())],
                    Span::new(3, 7),
                ))],
                variadic: vec![true],
                conventions: vec![AccessConvention::Write],
                policies: CallablePolicyChain {
                    policies: vec![CallablePolicy {
                        name: "trace".to_string(),
                        arguments: vec!["\"old\"".to_string()],
                    }],
                },
            }),
            return_view_provenance: original_return_view_provenance.clone(),
        };
        let replacement = CallablePolicyChain {
            policies: vec![CallablePolicy {
                name: "retry".to_string(),
                arguments: vec!["3".to_string()],
            }],
        };
        let wrapped = original
            .replace_callable_policies(replacement.clone())
            .expect("function value");
        let Type::Fn {
            params,
            ret,
            effect_bound,
            param_contract,
            call_metadata: Some(metadata),
            return_view_provenance,
        } = wrapped
        else {
            crate::ice!(None, "function value")
        };
        assert_eq!(params, vec![Type::List(Box::new(Type::String))]);
        assert_eq!(
            ret.unwrap().as_ref(),
            &Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::Named("NetError".to_string())),
            }
        );
        assert_eq!(
            effect_bound.as_ref().map(|row| row[0].0.as_str()),
            Some("Net")
        );
        assert_eq!(
            param_contract,
            Some(vec![("users".to_string(), ParamZone::LabelOnly)])
        );
        assert_eq!(metadata.names, vec!["path"]);
        assert!(metadata.defaults[0].is_some());
        assert_eq!(metadata.variadic, vec![true]);
        assert_eq!(metadata.conventions, vec![AccessConvention::Write]);
        assert_eq!(metadata.policies, replacement);
        assert_eq!(return_view_provenance, original_return_view_provenance);
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
        assert_eq!(
            force
                .measure_exponents()
                .find(|(axis, _)| *axis == "pkg::Time")
                .map(|(_, measure)| measure),
            Some(Measure::signed_literal("exponent", -2))
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
                Type::Apply {
                    name: "Box".into(),
                    args: vec![unit.clone()],
                },
            ],
            ret: Some(Box::new(Type::Result {
                ok: Box::new(unit),
                err: Box::new(Type::String),
            })),
            effect_bound: None,
            param_contract: None,
            call_metadata: None,
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
        assert!(vector
            .iter()
            .any(|entry| { matches!(&entry.fact, KnowledgeFact::Dimension(_)) }));
        assert!(vector
            .iter()
            .any(|entry| { matches!(&entry.fact, KnowledgeFact::Interval { lo: 0, hi: 255 }) }));
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
        assert!(quantity
            .identity()
            .knowledge
            .identity_key()
            .contains("Type.Dimension"));
        assert!(Type::List(Box::new(quantity.clone()))
            .identity()
            .knowledge
            .identity_key()
            .contains("Type.Dimension"));
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
            .facts(crate::Registry::type_plane("Measure"))
            .any(|fact| matches!(
                fact,
                KnowledgeFact::Measure(Measure::Literal { kind, value })
                    if kind == "lane" && *value == 4
            )));
    }

    #[test]
    fn integer_range_reads_the_interval_plane_for_widths_and_ranges() {
        // D-TYPE2-REFINE1: sized widths and user ranges share one interval fact.
        let width = Type::IntN {
            signed: false,
            bits: 8,
        };
        let range = Type::InlineRange {
            base: Box::new(Type::Int),
            lo: 1,
            hi: 6,
        };

        for (ty, expected) in [(&width, (0_i128, 255_i128)), (&range, (1_i128, 6_i128))] {
            assert_eq!(ty.knowledge_vector().interval_i128(), Some(expected));
            assert_eq!(ty.integer_range(), Some(expected));
        }
    }

    #[test]
    fn interval_projection_ignores_interval_shaped_facts_on_other_planes() {
        let mut vector = KnowledgeVector::new();
        vector.push(
            crate::Registry::type_plane("Layout"),
            KnowledgeFact::Interval { lo: 1, hi: 6 },
        );

        assert_eq!(vector.interval_i128(), None);
    }

    #[test]
    fn function_identity_is_transitive_and_obligations_use_subsumption() {
        let callable = |zone| Type::Fn {
            params: vec![Type::Bool],
            ret: Some(Box::new(Type::Int)),
            effect_bound: None,
            param_contract: Some(vec![("force".to_string(), zone)]),
            call_metadata: None,
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
            call_metadata: None,
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
        assert_eq!(u64.numeric_widening_to(&Type::Int), Some(false));
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
                    Type::Int => (true, 127),
                    Type::IntN { signed, bits } => (signed, bits),
                    _ => unreachable!(),
                };
                let (target_signed, target_bits) = match target_ty {
                    Type::Int => (true, 127),
                    Type::IntN { signed, bits } => (signed, bits),
                    _ => unreachable!(),
                };
                let expected = match (&source_ty, &target_ty) {
                    (Type::Int, Type::IntN { .. }) => Some(true),
                    (Type::IntN { .. }, Type::Int) => Some(false),
                    _ => {
                        let (source_min, source_max) = super::int_range(source_signed, source_bits);
                        let (target_min, target_max) = super::int_range(target_signed, target_bits);
                        (target_min <= source_min && source_max <= target_max).then_some(false)
                    }
                };
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
