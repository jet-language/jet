//! Compiler-owned scoped policy registry and resolution ladder (D-MARK-SCOPE1).

use crate::Diagnostics::Span;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum PolicyScope { Organization, Package, Module, Function, Block }

impl PolicyScope {
    pub const fn name(self) -> &'static str { match self { Self::Organization => "organization", Self::Package => "package", Self::Module => "module", Self::Function => "function", Self::Block => "block" } }
    const fn rank(self) -> u8 { match self { Self::Organization => 0, Self::Package => 1, Self::Module => 2, Self::Function => 3, Self::Block => 4 } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum PolicyKey { NoAlloc, ZeroRc, ArenaBounded, Unsafe, Impure, Nondeterministic, ScopedGc, ExplicitUnits, Sentries }

impl PolicyKey {
    pub const fn name(self) -> &'static str { match self { Self::NoAlloc => "no_alloc", Self::ZeroRc => "zero_rc", Self::ArenaBounded => "arena_bounded", Self::Unsafe => "unsafe", Self::Impure => "impure", Self::Nondeterministic => "nondeterministic", Self::ScopedGc => "gc", Self::ExplicitUnits => "explicit_units", Self::Sentries => "sentries" } }
    pub fn parse(name: &str) -> Option<Self> { match name { "no_alloc" => Some(Self::NoAlloc), "zero_rc" => Some(Self::ZeroRc), "arena_bounded" => Some(Self::ArenaBounded), "unsafe" => Some(Self::Unsafe), "impure" => Some(Self::Impure), "nondeterministic" => Some(Self::Nondeterministic), "gc" => Some(Self::ScopedGc), "explicit_units" => Some(Self::ExplicitUnits), "sentries" => Some(Self::Sentries), _ => None } }
    pub const fn is_audited_gate(self) -> bool { matches!(self, Self::Unsafe | Self::Impure | Self::Nondeterministic) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolicyValue {
    Enabled,
    Limit(u64),
    Forbid,
    Default,
    GateOnly,
    Obligations,
    Relaxed,
    PerSite,
    Track,
    Skip,
    Allow,
    On,
    Off,
}

impl PolicyValue {
    pub fn display(self) -> String { match self {
        Self::Enabled => "true".into(), Self::Limit(n) => n.to_string(),
        Self::Forbid => ".Forbid".into(), Self::Default => ".Default".into(),
        Self::GateOnly => ".GateOnly".into(), Self::Obligations => ".Obligations".into(),
        Self::Relaxed => ".Relaxed".into(), Self::PerSite => ".PerSite".into(),
        Self::Track => ".Track".into(), Self::Skip => ".Skip".into(), Self::Allow => ".Allow".into(),
        Self::On => ".On".into(), Self::Off => ".Off".into(),
    } }
}

/// The six lexical source rungs used by every build fact. Package is the
/// outermost source and Item is the nearest source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum SourceScope { Package, File, Module, Function, Block, Item }

impl SourceScope {
    pub const fn name(self) -> &'static str { match self {
        Self::Package => "package",
        Self::File => "file",
        Self::Module => "module",
        Self::Function => "function",
        Self::Block => "block",
        Self::Item => "item",
    } }

    const fn rank(self) -> u8 { match self {
        Self::Package => 0,
        Self::File => 1,
        Self::Module => 2,
        Self::Function => 3,
        Self::Block => 4,
        Self::Item => 5,
    } }
}

/// Contribution layers are ordered from least to most explicit. A later
/// layer may replace an earlier layer; two different values at one deciding
/// layer are a hard conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum ContributionLayer {
    Declaration,
    OptimizationBundle,
    Workspace,
    Environment,
    System,
    Fleet,
    CommandLine,
}

impl ContributionLayer {
    pub const fn name(self) -> &'static str { match self {
        Self::Declaration => "declaration",
        Self::OptimizationBundle => "optimization bundle",
        Self::Workspace => "workspace",
        Self::Environment => "environment",
        Self::System => "system",
        Self::Fleet => "fleet",
        Self::CommandLine => "command line",
    } }

    const fn rank(self) -> u8 { match self {
        Self::Declaration => 0,
        Self::OptimizationBundle => 1,
        Self::Workspace => 2,
        Self::Environment => 3,
        Self::System => 4,
        Self::Fleet => 5,
        Self::CommandLine => 6,
    } }

    const fn can_force(self) -> bool {
        matches!(self, Self::System | Self::Fleet)
    }
}

/// How a fact combines across the contribution ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FactMerge { Override, TightenOnly }

/// A registered fact key. The registry supplies the key's merge direction;
/// contributors supply only a value and provenance.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FactKey {
    pub name: String,
    pub merge: FactMerge,
}

impl FactKey {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), merge: FactMerge::Override }
    }

    pub fn tighten_only(name: impl Into<String>) -> Self {
        Self { name: name.into(), merge: FactMerge::TightenOnly }
    }
}

/// Tier-0 values that can travel through the contribution law. Facts erase
/// before runtime; this carrier exists only for folding and explanation.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FactValue {
    Bool(bool),
    Int(i64),
    Char(char),
    Text(String),
    Enum(String),
    OptionalText(Option<String>),
    Policy(PolicyValue),
}

impl FactValue {
    pub fn display(&self) -> String { match self {
        Self::Bool(value) => value.to_string(),
        Self::Int(value) => value.to_string(),
        Self::Char(value) => format!("'{value}'"),
        Self::Text(value) => format!("\"{value}\""),
        Self::Enum(value) => format!(".{value}"),
        Self::OptionalText(Some(value)) => format!("Some(\"{value}\")"),
        Self::OptionalText(None) => "None".to_string(),
        Self::Policy(value) => value.display(),
    } }

    fn safety_relation(&self, next: &Self) -> SafetyRelation {
        if self == next {
            return SafetyRelation::Same;
        }
        match (self, next) {
            (Self::Bool(outer), Self::Bool(inner)) => {
                if *outer && !*inner { SafetyRelation::Tighten } else { SafetyRelation::Widen }
            }
            (Self::Int(outer), Self::Int(inner)) => {
                if inner < outer { SafetyRelation::Tighten } else { SafetyRelation::Widen }
            }
            (Self::Policy(outer), Self::Policy(inner)) => {
                if gate_widens(*outer, *inner) { SafetyRelation::Widen } else { SafetyRelation::Tighten }
            }
            _ => SafetyRelation::Incomparable,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SafetyRelation { Same, Tighten, Widen, Incomparable }

/// One written contributor to one fact. `target` lets a source-scoped value
/// identify the item it governs without changing the global ordering law.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FactContribution {
    pub key: String,
    pub value: FactValue,
    pub scope: SourceScope,
    pub layer: ContributionLayer,
    pub span: Span,
    pub target: Option<Span>,
    pub source: String,
    pub force: bool,
    pub force_reason: Option<String>,
}

impl FactContribution {
    pub fn new(
        key: impl Into<String>,
        value: FactValue,
        scope: SourceScope,
        layer: ContributionLayer,
        source: impl Into<String>,
    ) -> Self {
        Self {
            key: key.into(),
            value,
            scope,
            layer,
            span: Span::new(0, 0),
            target: None,
            source: source.into(),
            force: false,
            force_reason: None,
        }
    }

    pub fn at(mut self, span: Span) -> Self {
        self.span = span;
        self
    }

    pub fn for_target(mut self, target: Span) -> Self {
        self.target = Some(target);
        self
    }

    pub fn force_with_reason(mut self, reason: impl Into<String>) -> Self {
        self.force = true;
        self.force_reason = Some(reason.into());
        self
    }

    pub fn force(self) -> Self {
        self.force_with_reason(".Force")
    }
}

/// The resolved value plus the complete writer chain. `effective` indexes
/// `provenance`, so explain output can mark exactly one writer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveFact {
    pub key: FactKey,
    pub value: FactValue,
    pub provenance: Vec<FactContribution>,
    pub effective: usize,
    pub forced: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactError {
    InvalidForce { key: String, layer: ContributionLayer, source: String },
    Conflict { key: String, layer: ContributionLayer, first: FactContribution, second: FactContribution },
    SafetyWidening { key: String, previous: FactContribution, next: FactContribution },
    SafetyIncomparable { key: String, previous: FactContribution, next: FactContribution },
}

impl FactError {
    pub fn message(&self) -> String { match self {
        Self::InvalidForce { key, layer, source } => format!("fact `{key}` uses `.Force` at the unsupported {} layer ({source})", layer.name()),
        Self::Conflict { key, layer, first, second } => format!("fact `{key}` has conflicting values at the {} layer: {}:{} and {}:{}", layer.name(), first.source, first.span.start, second.source, second.span.start),
        Self::SafetyWidening { key, previous, next } => format!("fact `{key}` widens from {} at {} to {} at {}", previous.value.display(), previous.source, next.value.display(), next.source),
        Self::SafetyIncomparable { key, previous, next } => format!("fact `{key}` has unrelated safety values at {} and {}", previous.source, next.source),
    } }

    /// The same typed conflict reaches the product diagnostic and names both
    /// written locations. Other resolver failures remain typed resolver errors.
    pub fn diagnostic(&self) -> Option<crate::Diagnostics::Diagnostic> {
        let Self::Conflict { key, layer, first, second } = self else { return None };
        Some(crate::Diagnostics::Diagnostic::error(
            "E3521",
            format!("fact `{key}` has conflicting values at the {} layer", layer.name()),
            format!("`{}` writes {} at {}:{}; `{}` writes {} at {}:{}.", first.source, first.value.display(), first.span.start, first.span.end, second.source, second.value.display(), second.span.start, second.span.end),
            "make same-layer writers agree, or move one value to a more explicit contribution layer".to_string(),
            Some(second.span),
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyCombine { Tighten, Override, Merge }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyRule {
    pub key: PolicyKey,
    pub scopes: &'static [PolicyScope],
    pub combine: PolicyCombine,
}

const PACKAGE_SCOPES: &[PolicyScope] = &[PolicyScope::Package, PolicyScope::Module, PolicyScope::Function, PolicyScope::Block];
const ALL_SCOPES: &[PolicyScope] = &[PolicyScope::Organization, PolicyScope::Package, PolicyScope::Module, PolicyScope::Function, PolicyScope::Block];
pub const POLICY_RULES: &[PolicyRule] = &[
    PolicyRule { key: PolicyKey::NoAlloc, scopes: PACKAGE_SCOPES, combine: PolicyCombine::Tighten },
    PolicyRule { key: PolicyKey::ZeroRc, scopes: PACKAGE_SCOPES, combine: PolicyCombine::Tighten },
    PolicyRule { key: PolicyKey::ArenaBounded, scopes: PACKAGE_SCOPES, combine: PolicyCombine::Tighten },
    PolicyRule { key: PolicyKey::Unsafe, scopes: ALL_SCOPES, combine: PolicyCombine::Tighten },
    PolicyRule { key: PolicyKey::Impure, scopes: ALL_SCOPES, combine: PolicyCombine::Tighten },
    PolicyRule { key: PolicyKey::Nondeterministic, scopes: ALL_SCOPES, combine: PolicyCombine::Tighten },
    PolicyRule { key: PolicyKey::ScopedGc, scopes: PACKAGE_SCOPES, combine: PolicyCombine::Override },
    PolicyRule { key: PolicyKey::ExplicitUnits, scopes: PACKAGE_SCOPES, combine: PolicyCombine::Tighten },
    PolicyRule { key: PolicyKey::Sentries, scopes: PACKAGE_SCOPES, combine: PolicyCombine::Tighten },
];

pub const AUDITED_GATE_KEYS: &[PolicyKey] = &[PolicyKey::Unsafe, PolicyKey::Impure, PolicyKey::Nondeterministic];

pub fn rule(key: PolicyKey) -> &'static PolicyRule { POLICY_RULES.iter().find(|r| r.key == key).expect("registered policy key") }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyDeclaration {
    pub key: PolicyKey,
    pub value: PolicyValue,
    pub scope: PolicyScope,
    pub span: Span,
    /// Span of the function/block governed by this declaration; module/package use `None`.
    pub target: Option<Span>,
    /// Stable source identity (`package.jet` or module display path) for explain provenance.
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectivePolicy {
    pub key: PolicyKey,
    pub value: PolicyValue,
    /// Outer-to-inner declaration chain, including overridden declarations.
    pub provenance: Vec<PolicyDeclaration>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    ProhibitedScope { key: PolicyKey, scope: PolicyScope, span: Span },
    Conflict { key: PolicyKey, scope: PolicyScope, first: Span, second: Span },
    Widening { key: PolicyKey, outer: PolicyValue, inner: PolicyValue, span: Span },
}

/// Resolve declarations ordered outer-to-inner. One effective value is returned,
/// while every shadowed/tightened declaration remains available to explain tools.
fn resolve_policy(key: PolicyKey, declarations: impl IntoIterator<Item = PolicyDeclaration>) -> Result<Option<EffectivePolicy>, PolicyError> {
    let mut declarations = declarations.into_iter().filter(|d| d.key == key).collect::<Vec<_>>();
    declarations.sort_by_key(|declaration| declaration.scope.rank());
    let mut chain = Vec::new();
    let mut effective = None;
    for declaration in declarations {
        if !rule(key).scopes.contains(&declaration.scope) {
            return Err(PolicyError::ProhibitedScope { key, scope: declaration.scope, span: declaration.span });
        }
        if let Some(previous) = chain.iter().find(|previous: &&PolicyDeclaration| previous.scope == declaration.scope && previous.target == declaration.target) {
            return Err(PolicyError::Conflict { key, scope: declaration.scope, first: previous.span, second: declaration.span });
        }
        let mut next = declaration.value;
        if let Some(outer) = effective {
            let widens = match (key, outer, declaration.value) {
                (PolicyKey::ArenaBounded, PolicyValue::Limit(a), PolicyValue::Limit(b)) => b > a,
                (PolicyKey::NoAlloc | PolicyKey::ZeroRc | PolicyKey::ScopedGc | PolicyKey::ExplicitUnits, PolicyValue::Enabled, PolicyValue::Enabled) => false,
                (PolicyKey::Sentries, PolicyValue::On, PolicyValue::On | PolicyValue::Off) => false,
                (PolicyKey::Sentries, PolicyValue::Off, PolicyValue::On) => true,
                (PolicyKey::Sentries, PolicyValue::Off, PolicyValue::Off) => false,
                (PolicyKey::Unsafe | PolicyKey::Impure | PolicyKey::Nondeterministic, outer, inner) => gate_widens(outer, inner),
                _ => true,
            };
            if rule(key).combine == PolicyCombine::Tighten && widens {
                return Err(PolicyError::Widening { key, outer, inner: declaration.value, span: declaration.span });
            }
            if rule(key).combine == PolicyCombine::Merge {
                next = match (outer, declaration.value) {
                    (PolicyValue::Limit(a), PolicyValue::Limit(b)) => PolicyValue::Limit(a.min(b)),
                    (PolicyValue::Enabled, PolicyValue::Enabled) => PolicyValue::Enabled,
                    (PolicyValue::Forbid, PolicyValue::Forbid) => PolicyValue::Forbid,
                    _ => return Err(PolicyError::Conflict { key, scope: declaration.scope, first: chain.last().unwrap().span, second: declaration.span }),
                };
            }
        }
        effective = Some(next);
        chain.push(declaration);
    }
    Ok(effective.map(|value| EffectivePolicy { key, value, provenance: chain }))
}

/// One resolver seam for policies and build facts. The key selects the
/// contribution law; no caller gets to choose a second merge algorithm.
pub trait ResolutionKey: Sized {
    type Declaration;
    type Effective;
    type Error;

    fn resolve_key(
        key: Self,
        declarations: Vec<Self::Declaration>,
    ) -> Result<Option<Self::Effective>, Self::Error>;
}

impl ResolutionKey for PolicyKey {
    type Declaration = PolicyDeclaration;
    type Effective = EffectivePolicy;
    type Error = PolicyError;

    fn resolve_key(
        key: Self,
        declarations: Vec<Self::Declaration>,
    ) -> Result<Option<Self::Effective>, Self::Error> {
        resolve_policy(key, declarations)
    }
}

impl ResolutionKey for FactKey {
    type Declaration = FactContribution;
    type Effective = EffectiveFact;
    type Error = FactError;

    fn resolve_key(
        key: Self,
        declarations: Vec<Self::Declaration>,
    ) -> Result<Option<Self::Effective>, Self::Error> {
        resolve_fact(key, declarations)
    }
}

pub fn resolve<K, I>(
    key: K,
    declarations: I,
) -> Result<Option<K::Effective>, K::Error>
where
    K: ResolutionKey,
    I: IntoIterator<Item = K::Declaration>,
{
    K::resolve_key(key, declarations.into_iter().collect())
}

fn resolve_fact(
    key: FactKey,
    declarations: Vec<FactContribution>,
) -> Result<Option<EffectiveFact>, FactError> {
    let mut chain = declarations
        .into_iter()
        .filter(|declaration| declaration.key == key.name)
        .collect::<Vec<_>>();
    if chain.is_empty() {
        return Ok(None);
    }
    chain.sort_by(|left, right| {
        left.layer
            .rank()
            .cmp(&right.layer.rank())
            .then_with(|| left.scope.rank().cmp(&right.scope.rank()))
            .then_with(|| left.source.cmp(&right.source))
            .then_with(|| left.span.start.cmp(&right.span.start))
            .then_with(|| left.span.end.cmp(&right.span.end))
    });

    for declaration in &chain {
        if declaration.force && !declaration.layer.can_force() {
            return Err(FactError::InvalidForce {
                key: key.name.clone(),
                layer: declaration.layer,
                source: declaration.source.clone(),
            });
        }
    }

    let mut group: Option<(ContributionLayer, SourceScope)> = None;
    let mut first_in_group: Option<&FactContribution> = None;
    for declaration in &chain {
        let identity = (declaration.layer, declaration.scope);
        if group != Some(identity) {
            group = Some(identity);
            first_in_group = Some(declaration);
        } else if declaration.value != first_in_group.expect("fact conflict group has a first writer").value {
            let first = first_in_group.expect("fact conflict group has a first writer");
            return Err(FactError::Conflict {
                key: key.name.clone(),
                layer: declaration.layer,
                first: first.clone(),
                second: declaration.clone(),
            });
        }
    }

    let force_layer = chain
        .iter()
        .filter(|declaration| declaration.force)
        .map(|declaration| declaration.layer.rank())
        .max();
    let eligible = |declaration: &FactContribution| {
        force_layer.map_or(true, |layer| declaration.layer.rank() <= layer)
    };

    let effective_layer = chain
        .iter()
        .filter(|declaration| eligible(declaration))
        .map(|declaration| declaration.layer.rank())
        .max()
        .expect("non-empty fact chain");
    let effective_scope = chain
        .iter()
        .filter(|declaration| eligible(declaration) && declaration.layer.rank() == effective_layer)
        .map(|declaration| declaration.scope.rank())
        .max()
        .expect("non-empty effective fact layer");
    let candidates = chain
        .iter()
        .filter(|declaration| {
            eligible(declaration)
                && declaration.layer.rank() == effective_layer
                && declaration.scope.rank() == effective_scope
        })
        .collect::<Vec<_>>();
    let first = candidates.first().expect("non-empty effective fact candidates");
    for second in candidates.iter().skip(1) {
        if second.value != first.value {
            return Err(FactError::Conflict {
                key: key.name.clone(),
                layer: first.layer,
                first: (**first).clone(),
                second: (**second).clone(),
            });
        }
    }
    if key.merge == FactMerge::TightenOnly {
        let mut previous: Option<&FactContribution> = None;
        for declaration in chain.iter().filter(|declaration| eligible(declaration)) {
            if let Some(previous) = previous {
                match previous.value.safety_relation(&declaration.value) {
                    SafetyRelation::Same | SafetyRelation::Tighten => {}
                    SafetyRelation::Widen => {
                        return Err(FactError::SafetyWidening {
                            key: key.name.clone(),
                            previous: previous.clone(),
                            next: declaration.clone(),
                        });
                    }
                    SafetyRelation::Incomparable => {
                        return Err(FactError::SafetyIncomparable {
                            key: key.name.clone(),
                            previous: previous.clone(),
                            next: declaration.clone(),
                        });
                    }
                }
            }
            previous = Some(declaration);
        }
    }
    let effective = chain
        .iter()
        .position(|declaration| std::ptr::eq(declaration, *first))
        .expect("effective fact candidate belongs to chain");
    let value = first.value.clone();
    let forced = first.force || force_layer.is_some();
    drop(candidates);
    Ok(Some(EffectiveFact {
        key,
        value,
        provenance: chain,
        effective,
        forced,
    }))
}

fn gate_widens(outer: PolicyValue, inner: PolicyValue) -> bool {
    use PolicyValue::*;
    match (outer, inner) {
        (Forbid, Allow) => true,
        (_, Allow) => false,
        (Forbid, Forbid) => false,
        (Forbid, _) => true,
        (Obligations, Obligations | Track) => false,
        (Obligations, _) => true,
        (PerSite, Track | Skip) => false,
        (PerSite, PerSite) => false,
        (Default | GateOnly | Relaxed, Track | Obligations) => false,
        (Default | GateOnly | Relaxed, Default | GateOnly | Relaxed | Skip) => false,
        (Track, Track) | (Skip, Skip) => false,
        (On, On | Off) | (Off, Off) => false,
        (Off, On) => true,
        _ => true,
    }
}

pub fn default_gate_value(key: PolicyKey) -> PolicyValue {
    match key {
        PolicyKey::Unsafe => PolicyValue::Default,
        PolicyKey::Impure | PolicyKey::Nondeterministic => PolicyValue::GateOnly,
        PolicyKey::Sentries => PolicyValue::On,
        _ => PolicyValue::Enabled,
    }
}

pub fn parse_value(key: PolicyKey, raw: &str) -> Result<PolicyValue, String> {
    let raw = raw.trim();
    match key {
        PolicyKey::NoAlloc | PolicyKey::ZeroRc | PolicyKey::ScopedGc | PolicyKey::ExplicitUnits if raw == "true" => Ok(PolicyValue::Enabled),
        PolicyKey::Sentries => match raw {
            ".On" => Ok(PolicyValue::On),
            ".Off" => Ok(PolicyValue::Off),
            _ => Err("`sentries` must be `.On` or `.Off`".to_string()),
        },
        PolicyKey::ArenaBounded => raw.parse::<u64>().ok().filter(|n| *n > 0).map(PolicyValue::Limit).ok_or_else(|| format!("`{}` needs a positive byte limit", key.name())),
        key if key.is_audited_gate() => match raw {
            ".Forbid" => Ok(PolicyValue::Forbid),
            ".Default" => Ok(PolicyValue::Default),
            ".GateOnly" => Ok(PolicyValue::GateOnly),
            ".Obligations" => Ok(PolicyValue::Obligations),
            ".Relaxed" => Ok(PolicyValue::Relaxed),
            ".PerSite" => Ok(PolicyValue::PerSite),
            ".Track" => Ok(PolicyValue::Track),
            ".Skip" => Ok(PolicyValue::Skip),
            _ => Err(format!("`{}` must be one of `.Forbid`, `.Default`, `.GateOnly`, `.Obligations`, `.Relaxed`, `.PerSite`, `.Track`, or `.Skip`", key.name())),
        },
        _ => Err(format!("package policy `{}` has an unsupported value", key.name())),
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GateSet { bits: u8 }

impl GateSet {
    pub fn allow(key: PolicyKey) -> Self {
        let mut gates = Self::default();
        gates.insert(key);
        gates
    }

    fn bit(key: PolicyKey) -> u8 {
        match key {
            PolicyKey::Unsafe => 1,
            PolicyKey::Impure => 2,
            PolicyKey::Nondeterministic => 4,
            _ => 0,
        }
    }

    pub fn insert(&mut self, key: PolicyKey) { self.bits |= Self::bit(key); }

    /// Resolve an invocation allowance through the same policy resolver used
    /// by package and organization declarations. The bitset is only the
    /// transport shape; it is not a second policy mechanism.
    pub fn allows(self, key: PolicyKey) -> bool {
        resolve_invocation(key, &self).unwrap_or(false)
    }

    fn contains(self, key: PolicyKey) -> bool { self.bits & Self::bit(key) != 0 }
    pub fn is_empty(self) -> bool { self.bits == 0 }

    /// Parse one CLI `name=allow` entry. The synthetic declaration goes
    /// through the same resolver as package and organization policy.
    pub fn parse(spec: &str) -> Result<PolicyKey, String> {
        let (name, value) = spec.split_once('=').ok_or_else(|| "use `--gate name=allow`".to_string())?;
        let key = PolicyKey::parse(name.trim()).ok_or_else(|| format!("`{}` is not an audited gate", name.trim()))?;
        if !key.is_audited_gate() || value.trim() != "allow" {
            return Err(format!("`--gate {spec}` must name an audited gate with `=allow`"));
        }
        resolve_invocation(key, &Self::allow(key))
            .map_err(|error| format!("invalid gate `{spec}`: {error:?}"))?;
        Ok(key)
    }
}

/// Resolve one invocation gate with the canonical policy ladder. This is the
/// only bridge from the CLI gate transport to policy semantics.
pub fn resolve_invocation(key: PolicyKey, gates: &GateSet) -> Result<bool, PolicyError> {
    if !gates.contains(key) {
        return Ok(false);
    }
    let declaration = PolicyDeclaration {
        key,
        value: PolicyValue::Allow,
        scope: PolicyScope::Block,
        span: Span::new(0, 0),
        target: None,
        source: "<invocation>".to_string(),
    };
    resolve(key, [declaration]).map(|_| true)
}

pub fn resolve_with_gates(
    key: PolicyKey,
    declarations: impl IntoIterator<Item = PolicyDeclaration>,
    gates: &GateSet,
) -> Result<Option<EffectivePolicy>, PolicyError> {
    let declarations = declarations.into_iter().collect::<Vec<_>>();
    let effective = resolve(key, declarations.clone())?;
    if gates.contains(key) {
        let mut invocation_chain = declarations;
        invocation_chain.push(PolicyDeclaration {
            key,
            value: PolicyValue::Allow,
            scope: PolicyScope::Block,
            span: Span::new(0, 0),
            target: None,
            source: "<invocation>".to_string(),
        });
        // The invocation allowance is checked by the same ladder, but its
        // synthetic value is not returned as the effective package policy.
        resolve(key, invocation_chain)?;
    }
    Ok(effective)
}

pub trait ExplainableResolution {
    fn explain_resolution(&self) -> String;
}

impl ExplainableResolution for EffectivePolicy {
    fn explain_resolution(&self) -> String {
        let mut out = format!("{} = {}", self.key.name(), self.value.display());
        for (index, declaration) in self.provenance.iter().enumerate() {
            let status = if index + 1 == self.provenance.len() { "effective" } else { "shadowed" };
            out.push_str(&format!("\n  [{status}] {} {} at {}:{}..{}", declaration.scope.name(), declaration.value.display(), declaration.source, declaration.span.start, declaration.span.end));
        }
        out
    }
}

impl ExplainableResolution for EffectiveFact {
    fn explain_resolution(&self) -> String {
        let mut out = format!("{} = {}", self.key.name, self.value.display());
        for (index, contribution) in self.provenance.iter().enumerate() {
            let status = if index == self.effective { "effective" } else if contribution.force { "pinned" } else { "shadowed" };
            let pin = contribution
                .force_reason
                .as_deref()
                .map_or(String::new(), |reason| format!(" pin={reason}"));
            out.push_str(&format!("\n  [{status}] {} / {} {} at {}:{}..{}{}", contribution.layer.name(), contribution.scope.name(), contribution.value.display(), contribution.source, contribution.span.start, contribution.span.end, pin));
        }
        out
    }
}

pub fn explain<R: ExplainableResolution>(resolution: &R) -> String {
    resolution.explain_resolution()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuleSite { Package, File, Module, Function, Method, Block, Statement, Expression, Type, Impl, Declaration, Constant, Field, Variant, Parameter, Test, Bench, Operation, Text }

impl RuleSite {
    pub const ALL: [Self; 19] = [
        Self::Package,
        Self::File,
        Self::Module,
        Self::Function,
        Self::Method,
        Self::Block,
        Self::Statement,
        Self::Expression,
        Self::Type,
        Self::Impl,
        Self::Declaration,
        Self::Constant,
        Self::Field,
        Self::Variant,
        Self::Parameter,
        Self::Test,
        Self::Bench,
        Self::Operation,
        Self::Text,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Package => "Package",
            Self::File => "File",
            Self::Module => "Module",
            Self::Function => "Function",
            Self::Method => "Method",
            Self::Block => "Block",
            Self::Statement => "Statement",
            Self::Expression => "Expression",
            Self::Type => "Type",
            Self::Impl => "Impl",
            Self::Declaration => "Declaration",
            Self::Constant => "Constant",
            Self::Field => "Field",
            Self::Variant => "Variant",
            Self::Parameter => "Parameter",
            Self::Test => "Test",
            Self::Bench => "Bench",
            Self::Operation => "Operation",
            Self::Text => "Text",
        }
    }
}

/// D-META-FORM1=A: `@sites` on a `marker` declaration takes `[Site]`, so the
/// nineteen attachment points are published as an ordinary `core.lang` enum
/// beside the other marker-argument menus (D-RULEARG-TYPES1=A). `RuleSite::ALL`
/// stays the one source; `site_variants_match_the_enum` proves this list is it.
pub const SITE_VARIANTS: &[&str] = &[
    "Package",
    "File",
    "Module",
    "Function",
    "Method",
    "Block",
    "Statement",
    "Expression",
    "Type",
    "Impl",
    "Declaration",
    "Constant",
    "Field",
    "Variant",
    "Parameter",
    "Test",
    "Bench",
    "Operation",
    "Text",
];

/// D-MARK-FORM1=A: one placement law. A marker, or one bracket group, is
/// written immediately before its target; the registry says which targets it
/// accepts; parentheses appear exactly when arguments are written. There is no
/// written-form column — the signature and the site list carry everything the
/// five retired `RuleForm` categories used to approximate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppliedRule {
    pub name: &'static str,
    /// Executable source signature shared by parsing, diagnostics, and explain.
    pub signature: RuleSignature,
    /// Non-empty only for rules that participate in lexical policy inheritance.
    pub policy_scopes: &'static [PolicyScope],
    /// Exact sound attachment sites for non-inheriting rules.
    pub sites: &'static [RuleSite],
    /// D-MARK-REPEAT1=A: may this rule be written more than once on one target?
    pub repeatable: bool,
    /// This rule teaches its own argument menu downstream, so the shared binder
    /// stays quiet about an unknown variant and lets the rule's product
    /// diagnostic explain it (`#FFI` → E3220, `#RenameAll` → E2409).
    pub owns_menu: bool,
    /// One extra legal site that opens only when a companion rule sits on the
    /// same target. `#Doc` describes CLI fields/types/variants and can also
    /// describe a `#Job`.
    pub companion_site: Option<CompanionSite>,
    pub status: RuleStatus,
    pub inherits: bool,
    pub resolution: RuleResolution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleStatus {
    Active,
    Retired { replacement: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleResolution { SiteBound, Override, Merge, Tighten }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleArgType {
    Any,
    String,
    Ident,
    Bool,
    Int,
    DurationOrString,
    /// D-TESTFAULT1=A: a closed list of effect-root operation paths used by
    /// the test harness. This is a marker contract shape, not a runtime type.
    EffectRoots,
}

/// D-RULEARG-TYPES1=A + D-LANGNS-NAME1=A: compiler vocabulary is published as
/// ordinary generated enums in `core.lang`. Marker signatures point at these
/// declarations; diagnostics, reflection, and editor tools read the same rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleArgDeclaration {
    pub name: &'static str,
    pub variants: &'static [&'static str],
    /// Which segment of a written path names the variant. `core.lang.Target.Web`
    /// and `Capability.FS` are read from the front because their variants own
    /// nested names; every other menu reads the last segment.
    pub variant_segment: VariantSegment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariantSegment { First, Last }

/// An extra attachment site that a rule earns from a companion on the same
/// target (D-TASKS-LIST1=A: `#Doc` describes a `#Job`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompanionSite {
    pub rule: &'static str,
    pub site: RuleSite,
}

fn canonical_rule_arg_variants(name: &str) -> Option<&'static [&'static str]> {
    Some(match name {
        "ABI" => &["system", "cdecl", "stdcall", "fastcall", "win64", "sysv64"],
        "Capability" => crate::Facts::EFFECT_ROOTS.as_slice(),
        "FfiLanguage" => &["c", "cpp", "asm"],
        "InlineMode" => &["Hint", "Always", "Never"],
        "JobScope" => crate::Syntax::JOB_SCOPE_VARIANTS,
        "KernelMode" => &["parallel"],
        "MemoBound" => &["Default", "none"],
        "IntType" => &[
            "I8", "I16", "I32", "I64", "I128", "U8", "U16", "U32", "U64", "U128",
        ],
        "Layout" => &[crate::Syntax::LAYOUT_C, crate::Syntax::LAYOUT_COLUMNAR],
        "Maturity" => &["Experimental", "Tested", "Hardened"],
        "NamingCase" => &[
            crate::Syntax::RENAME_ALL_CAMEL,
            crate::Syntax::RENAME_ALL_SNAKE,
            crate::Syntax::RENAME_ALL_PASCAL,
            crate::Syntax::RENAME_ALL_KEBAB,
            crate::Syntax::RENAME_ALL_SCREAMING,
        ],
        "ObligationMode" => &["None", "GateOnly", "Obligations", "PerSite", "Track", "Skip"],
        "Site" => SITE_VARIANTS,
        "PolicySetting" => &[
            "no_alloc",
            "zero_rc",
            "arena_bounded",
            "unsafe",
            "gc",
            "explicit_units",
            "sentries",
        ],
        "State" => &[],
        "TaintKind" => crate::Syntax::BUILTIN_TAGS,
        "Target" => &["Native", "Web", "Wasm", "JS", "Freestanding", "OS"],
        "Track" => &["Frontend", "Backend", "Runtime", "Tooling"],
        _ => return None,
    })
}

/// Generated from the active/retired applied-rule signatures. `Track` is the
/// compatibility reflection enum retained by D-RULEARG-TYPES1.
pub static RULE_ARG_DECLARATIONS: LazyLock<Vec<RuleArgDeclaration>> = LazyLock::new(|| {
    // `Site` is published for `@sites` on a `marker` declaration (D-META-FORM1=A)
    // and `Track` for reflection; neither appears in a marker signature, so both
    // are seeded rather than found.
    let mut names = std::collections::BTreeSet::from(["Site", "Track"]);
    for row in APPLIED_RULES.iter() {
        names.extend(row.signature.params.iter().map(|parameter| parameter.source_type));
        names.extend(row.signature.variadic_source_type);
    }
    names
        .into_iter()
        .filter_map(|name| {
            canonical_rule_arg_variants(name).map(|variants| RuleArgDeclaration {
                name,
                variants,
                variant_segment: canonical_variant_segment(name),
            })
        })
        .collect()
});

/// D-RULEARG-TYPES1=A: `Capability` and `Target` variants own nested names
/// (`FS.read`, `Web.dom`), so the written path names its variant in the first
/// segment after the enum. Every other menu names it in the last.
const fn canonical_variant_segment(name: &str) -> VariantSegment {
    match name.as_bytes() {
        b"Capability" | b"Target" => VariantSegment::First,
        _ => VariantSegment::Last,
    }
}

pub fn rule_arg_declaration(name: &str) -> Option<&'static RuleArgDeclaration> {
    RULE_ARG_DECLARATIONS.iter().find(|declaration| declaration.name == name)
}

impl RuleArgType {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Any => "Value",
            Self::String => "String",
            Self::Ident => "Ident",
            Self::Bool => "Bool",
            Self::Int => "Int",
            Self::DurationOrString => "Duration | String",
            Self::EffectRoots => "[Effect]",
        }
    }

}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleParam {
    pub name: &'static str,
    pub ty: RuleArgType,
    pub source_type: &'static str,
    pub default: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleSignature {
    pub params: &'static [RuleParam],
    pub variadic: Option<RuleArgType>,
    pub variadic_source_type: Option<&'static str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleArgumentBinding {
    pub source_index: usize,
    pub parameter_index: Option<usize>,
    pub ty: RuleArgType,
}

impl RuleSignature {
    pub fn render(self) -> String {
        let mut parts = self
            .params
            .iter()
            .map(|param| {
                let mut part = format!("{}: {}", param.name, param.source_type);
                if let Some(default) = param.default {
                    part.push_str(" = ");
                    part.push_str(default);
                }
                part
            })
            .collect::<Vec<_>>();
        if let Some(ty) = self.variadic_source_type {
            parts.push(format!("{ty}..."));
        }
        format!("({})", parts.join(", "))
    }

    /// D-MARK-FORM1=A: may this rule be written with parentheses at all? A row
    /// that declares no parameter and no variadic list never takes arguments.
    pub const fn accepts_arguments(self) -> bool {
        !self.params.is_empty() || self.variadic.is_some()
    }

    /// D-MARK-FORM1=A: must arguments be written? A row with a required
    /// parameter needs them, and so does a row whose only parameter is a
    /// variadic list — an empty list carries no rule.
    pub const fn arguments_required(self) -> bool {
        self.required() > 0 || (self.params.is_empty() && self.variadic.is_some())
    }

    pub const fn required(self) -> usize {
        let mut count = 0;
        let mut index = 0;
        while index < self.params.len() {
            if self.params[index].default.is_none() {
                count += 1;
            }
            index += 1;
        }
        count
    }

    /// Bind source arguments to normalized parameter slots. The parser and
    /// every marker applicator use this one arity/label/variadic algorithm.
    pub fn argument_bindings(
        self,
        labels: &[Option<&str>],
    ) -> Option<Vec<RuleArgumentBinding>> {
        let mut supplied = vec![false; self.params.len()];
        let mut positional = 0usize;
        let mut saw_named = false;
        let mut bindings = Vec::with_capacity(labels.len());
        for (source_index, label) in labels.iter().enumerate() {
            let parameter = if let Some(label) = label {
                saw_named = true;
                let index = self.params.iter().position(|param| param.name == *label)?;
                if supplied[index] {
                    return None;
                }
                supplied[index] = true;
                Some(index)
            } else {
                if saw_named {
                    return None;
                }
                while positional < supplied.len() && supplied[positional] {
                    positional += 1;
                }
                if positional < supplied.len() {
                    let index = positional;
                    supplied[index] = true;
                    positional += 1;
                    Some(index)
                } else {
                    None
                }
            };
            bindings.push(RuleArgumentBinding {
                source_index,
                parameter_index: parameter,
                ty: match parameter {
                    Some(index) => self.params[index].ty,
                    None => self.variadic?,
                },
            });
        }
        if self
            .params
            .iter()
            .zip(supplied)
            .any(|(param, supplied)| param.default.is_none() && !supplied)
        {
            return None;
        }
        Some(bindings)
    }

    pub fn argument_types(self, labels: &[Option<&str>]) -> Option<Vec<RuleArgType>> {
        self.argument_bindings(labels)
            .map(|bindings| bindings.into_iter().map(|binding| binding.ty).collect())
    }

    pub fn marker_argument_bindings(
        self,
        marker: &crate::AST::Marker,
    ) -> Option<Vec<RuleArgumentBinding>> {
        // `#Meta` has dedicated duplicate-field diagnostics (E0346). Preserve
        // repeated known labels here so the semantic checker can teach the
        // actual mistake instead of collapsing it into the generic E0930.
        if marker.name == crate::Syntax::MARKER_META
            && marker.arg_labels.iter().all(Option::is_some)
        {
            return marker
                .arg_labels
                .iter()
                .enumerate()
                .map(|(source_index, label)| {
                    let name = &label.as_ref()?.0;
                    let parameter_index =
                        self.params.iter().position(|parameter| parameter.name == name)?;
                    Some(RuleArgumentBinding {
                        source_index,
                        parameter_index: Some(parameter_index),
                        ty: self.params[parameter_index].ty,
                    })
                })
                .collect();
        }
        if marker.name == crate::Syntax::MARKER_POLICY {
            return marker
                .arg_labels
                .iter()
                .enumerate()
                .map(|(source_index, label)| {
                    if let Some((name, _)) = label {
                        (name == "sentries").then_some(RuleArgumentBinding {
                            source_index,
                            parameter_index: None,
                            ty: self.variadic?,
                        })
                    } else {
                        Some(RuleArgumentBinding {
                            source_index,
                            parameter_index: None,
                            ty: self.variadic?,
                        })
                    }
                })
                .collect();
        }
        let labels = marker
            .args
            .iter()
            .zip(&marker.arg_labels)
            .map(|(argument, label)| {
                if let Some((name, _)) = label {
                    return Some(name.as_str());
                }
                if marker.name == crate::Syntax::MARKER_META
                    && matches!(argument, crate::AST::Expr::Ident(name, _) if name == crate::Syntax::META_FIELD_TUNABLE)
                {
                    return Some(crate::Syntax::META_FIELD_TUNABLE);
                }
                None
            })
            .collect::<Vec<_>>();
        self.argument_bindings(&labels)
    }
}

pub fn marker_argument_shape_error(
    name: &str,
    span: crate::Diagnostics::Span,
) -> crate::Diagnostics::Diagnostic {
    let expected = applied_rule(name)
        .map(|row| row.signature.render())
        .unwrap_or_else(|| "()".to_string());
    crate::Diagnostics::Diagnostic::error(
        "E0930",
        format!("`#{name}` arguments do not match `{name}{expected}`"),
        "marker arguments use the same call grammar and typed signature as function arguments"
            .to_string(),
        format!("match the registered signature `{name}{expected}`"),
        Some(span),
    )
}

/// The active registry vocabulary, plus any visible `derive T.Name { … }`
/// providers, is the only list a "did you mean" suggestion may draw from.
pub fn active_rule_names() -> Vec<String> {
    APPLIED_RULES
        .iter()
        .filter(|row| matches!(row.status, RuleStatus::Active))
        .map(|row| row.name.to_string())
        .collect()
}

/// D-MARK-VOCAB1 + D-META-ONE1=A: the whole marker vocabulary, in one value.
///
/// The registry rows read from `Prelude/Markers.jet` are the closed half. A
/// `derive T.Name { … }` provider the build can see is the open half. Every
/// legality test and every "did you mean" list reads this one value, so the
/// compiler keeps no second registry of marker names.
#[derive(Debug, Clone, Default)]
pub struct MarkerVocabulary {
    declared: BTreeSet<String>,
    /// Source declarations are retained in the same bundle-local vocabulary
    /// as derive providers. Consumers that need the declaration body (sema
    /// expansion and reflection) read this map; name-only consumers read the
    /// set above.
    declarations: BTreeMap<String, crate::AST::MarkerDecl>,
}

impl MarkerVocabulary {
    /// The registry, plus the derive providers visible to this build.
    pub fn with_derives(names: impl IntoIterator<Item = String>) -> Self {
        Self {
            declared: names.into_iter().collect(),
            declarations: BTreeMap::new(),
        }
    }

    /// Add source-declared rules to the same registry as derive providers.
    pub fn with_derives_and_declarations(
        names: impl IntoIterator<Item = String>,
        declarations: impl IntoIterator<Item = crate::AST::MarkerDecl>,
    ) -> Self {
        let mut vocabulary = Self::with_derives(names);
        for declaration in declarations {
            vocabulary.declared.insert(declaration.name.clone());
            vocabulary
                .declarations
                .insert(declaration.name.clone(), declaration);
        }
        vocabulary
    }

    /// True when a writer may spell this name as a marker.
    pub fn knows(&self, name: &str) -> bool {
        applied_rule(name).is_some_and(|row| matches!(row.status, RuleStatus::Active))
            || self.declared.contains(name)
    }

    /// Every spellable name, for a nearest-spelling suggestion.
    pub fn names(&self) -> Vec<String> {
        active_rule_names().into_iter().chain(self.declared.iter().cloned()).collect()
    }

    /// Return the source declaration for a user rule, if this build supplied
    /// one. Built-in rows remain in `APPLIED_RULES` and intentionally return
    /// `None` here.
    pub fn declaration(&self, name: &str) -> Option<&crate::AST::MarkerDecl> {
        self.declarations.get(name)
    }

    /// Source-declared rule rows in deterministic name order.
    pub fn declarations(&self) -> impl Iterator<Item = &crate::AST::MarkerDecl> {
        self.declarations.values()
    }

    /// The one unknown-marker diagnostic. No caller writes its own.
    pub fn unknown(
        &self,
        name: &str,
        span: crate::Diagnostics::Span,
    ) -> crate::Diagnostics::Diagnostic {
        marker_unknown_error(name, &self.names(), span)
    }
}

/// D-VERDICT-1455-1: every registered wrong-site report names the legal sites
/// from the row that caused it. Parser and sema use this constructor instead
/// of keeping site prose beside a marker-name branch.
pub fn marker_wrong_site_error(
    name: &str,
    site: RuleSite,
    span: crate::Diagnostics::Span,
) -> crate::Diagnostics::Diagnostic {
    let legal = applied_rule(name)
        .map(|row| {
            let mut sites = row
                .sites
                .iter()
                .map(|site| site.name().to_string())
                .collect::<Vec<_>>();
            if let Some(companion) = row.companion_site {
                sites.push(format!(
                    "{} with companion `#{}`",
                    companion.site.name(), companion.rule
                ));
            }
            sites
        })
        .unwrap_or_default();
    let legal = if legal.is_empty() {
        "no active attachment sites".to_string()
    } else {
        legal
            .into_iter()
            .map(|site| format!("`{site}`"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    crate::Diagnostics::Diagnostic::error(
        "E0355",
        format!("`#{name}` cannot attach at the {} site", site.name()),
        format!("the registry allows `#{name}` only at: {legal}"),
        format!("move `#{name}` to one of its registered sites"),
        Some(span),
    )
}

/// D-VERDICT-1455-1: one E0927 family for an unregistered or retired marker
/// name, at every site. Parser and sema both call this; neither writes its own
/// unknown-marker text, so a typo reads the same wherever it sits.
pub fn marker_unknown_error(
    name: &str,
    vocabulary: &[String],
    span: crate::Diagnostics::Span,
) -> crate::Diagnostics::Diagnostic {
    if let Some(AppliedRule {
        status: RuleStatus::Retired { replacement },
        ..
    }) = applied_rule(name)
    {
        let fix = if replacement.starts_with('#') || replacement.starts_with('.') {
            format!("write `{replacement}` instead")
        } else {
            replacement.to_string()
        };
        return crate::Diagnostics::Diagnostic::error(
            "E0927",
            format!("`#{name}` is retired"),
            "the registry keeps this old spelling only to teach its replacement; \
             it no longer applies a rule"
                .to_string(),
            fix,
            Some(span),
        );
    }
    let nearest = vocabulary
        .iter()
        .map(|candidate| (candidate, crate::Syntax::edit_distance(name, candidate)))
        .filter(|(_, distance)| *distance <= 2)
        .min_by_key(|(_, distance)| *distance)
        .map(|(candidate, _)| candidate.clone());
    crate::Diagnostics::Diagnostic::error(
        "E0927",
        format!("`#{name}` isn't a known applied rule"),
        format!(
            "`{name}` isn't registered as an applied rule — Jet rules are a closed, \
             registered vocabulary (I7), not any PascalCase word."
        ),
        nearest.map_or_else(
            || {
                "check the spelling, or see docs/spec/syntax-decisions.md for the full applied-rule list."
                    .to_string()
            },
            |nearest| format!("did you mean `#{nearest}`?"),
        ),
        Some(span),
    )
}

/// D-MARK-REPEAT1=A: the one repeated-rule diagnostic. Every site calls this,
/// so a duplicate reads the same on a file, a type, or a function; the
/// per-marker duplicate codes E0416 and E0428 retired into it.
pub fn marker_repeated_error(
    name: &str,
    noun: &str,
    span: crate::Diagnostics::Span,
) -> crate::Diagnostics::Diagnostic {
    crate::Diagnostics::Diagnostic::error(
        "E0999",
        format!("`#{name}` is already applied to this {noun}"),
        format!(
            "`#{name}` is not a repeatable rule, so the second copy adds nothing and can only disagree with the first"
        ),
        "remove the repeated marker".to_string(),
        Some(span),
    )
}

/// D-MARK-FORM1=A: parentheses appear exactly when arguments are written, so an
/// empty pair is always a leftover. `parens` covers `(` through `)` alone, so
/// the autofix deletes exactly that and `jet fmt` can apply it.
pub fn marker_empty_arguments_error(
    name: &str,
    parens: crate::Diagnostics::Span,
) -> crate::Diagnostics::Diagnostic {
    let mut diagnostic = crate::Diagnostics::Diagnostic::error(
        // E0999 is the marker-spelling family the formatter canonicalizes
        // (D-MARK-STACK1's bracket edges live here too); E0930 is reserved for
        // arguments that were written and do not match the signature.
        "E0999",
        format!("`#{name}()` has empty parentheses"),
        "a marker writes parentheses exactly when it passes arguments".to_string(),
        format!("write `#{name}`"),
        Some(parens),
    );
    diagnostic.set_structured_edit(crate::Diagnostics::TextEdit {
        span: parens,
        new_text: String::new(),
    });
    diagnostic
}

/// A marker argument whose shape is right but whose value is outside the
/// declared menu. Naming the menu is the whole point: the writer typed a name
/// the compiler knows nothing about.
pub fn marker_argument_unknown_variant(
    name: &str,
    declaration: RuleArgDeclaration,
    written: &str,
    span: crate::Diagnostics::Span,
) -> crate::Diagnostics::Diagnostic {
    let menu = declaration
        .variants
        .iter()
        .map(|variant| format!("`{variant}`"))
        .collect::<Vec<_>>()
        .join(", ");
    crate::Diagnostics::Diagnostic::error(
        "E0930",
        format!("`#{name}` has no `{}` called `{written}`", declaration.name),
        format!("`{}` is a closed menu: {menu}", declaration.name),
        format!("pick one of {menu}"),
        Some(span),
    )
}

#[path = "Policy/MarkerSource.rs"]
mod MarkerSource;

pub use MarkerSource::MARKER_SOURCE;

/// D-MARKSIG1=A + D-META-ONE1=A: the sole compiler registry for active and
/// retired markers. Parser, sema, formatter, LSP, highlighting, explain, and
/// retirement diagnostics read these rows; the rows themselves are written as
/// ordinary Jet `marker` declarations in `Prelude/Markers.jet`, so this file
/// holds no copy of the vocabulary.
pub static APPLIED_RULES: LazyLock<Vec<AppliedRule>> = LazyLock::new(MarkerSource::read);

pub fn applied_rule_registry() -> &'static [AppliedRule] {
    &APPLIED_RULES
}

pub fn applied_rule(name: &str) -> Option<&'static AppliedRule> {
    APPLIED_RULES.iter().find(|row| row.name == name)
}

pub fn rule_allows(name: &str, site: RuleSite) -> bool {
    applied_rule(name).is_some_and(|row| row.sites.contains(&site))
}

/// `rule_allows`, plus the extra site a companion on the same target opens.
/// `companions` is every rule name attached to that target, so `#Doc` is legal
/// on a function exactly when `#Job` sits beside it (D-TASKS-LIST1=A).
pub fn rule_allows_with_companions<'a>(
    name: &str,
    site: RuleSite,
    companions: impl Iterator<Item = &'a str>,
) -> bool {
    if rule_allows(name, site) {
        return true;
    }
    let Some(companion) = applied_rule(name).and_then(|row| row.companion_site) else {
        return false;
    };
    companion.site == site && companions.into_iter().any(|other| other == companion.rule)
}

pub const DERIVE_RULES: &[&str] = &[
    "Codable", "Encode", "Decode", "Comparable", "Equatable", "Debug",
    "Numeric", "Printable", "CLI", "Patchable", "UnitFamily",
];

/// A trait may rhyme with a type-site marker only when that marker is the
/// trait's derive spelling (`trait X` → `#X`). Other type markers own their
/// names and cannot silently collide with dispatch.
pub fn nonderive_marker_trait_collision(name: &str) -> bool {
    applied_rule(name).is_some_and(|row| {
        matches!(row.status, RuleStatus::Active)
            && row.sites.contains(&RuleSite::Type)
            && !DERIVE_RULES.contains(&name)
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_registered_rule_has_one_exact_applicability_row() {
        let rows = super::applied_rule_registry();
        for row in rows {
            assert_eq!(rows.iter().filter(|candidate| candidate.name == row.name).count(), 1, "{}", row.name);
            // D-META-STAGE1=B: an active row must keep at least one legal site.
            // A retired row may keep none, because a stage that is no longer a
            // rule about a target has nowhere left to be written.
            if matches!(row.status, super::RuleStatus::Active) {
                assert!(!row.sites.is_empty(), "{}", row.name);
            }
            assert_eq!(row.signature.variadic.is_some(), row.signature.variadic_source_type.is_some(), "{}", row.name);
            assert!(row.signature.required() <= row.signature.params.len(), "{}", row.name);
            for param in row.signature.params {
                assert!(!param.name.is_empty(), "{}", row.name);
                assert!(!param.source_type.is_empty(), "{}", row.name);
            }
        }
    }

    #[test]
    fn site_variants_match_the_enum() {
        use super::RuleSite;

        assert_eq!(super::SITE_VARIANTS.len(), 19);
        let published: Vec<&str> = RuleSite::ALL.iter().map(|site| site.name()).collect();
        assert_eq!(published, super::SITE_VARIANTS);

        let declaration =
            super::rule_arg_declaration("Site").expect("`Site` is published in `core.lang`");
        assert_eq!(declaration.variants, super::SITE_VARIANTS);
        assert_eq!(declaration.variant_segment, super::VariantSegment::Last);
    }

    #[test]
    fn authoritative_attachment_matrix_covers_declaration_sites() {
        use super::RuleSite;

        let legal = [
            ("allow", RuleSite::Field),
            ("Rename", RuleSite::Variant),
            ("Inline", RuleSite::Method),
            ("Policy", RuleSite::Module),
            ("HTML", RuleSite::File),
            ("Test", RuleSite::Test),
            ("Bench", RuleSite::Bench),
            ("Job", RuleSite::Function),
            ("Doc", RuleSite::Type),
            ("Doc", RuleSite::Variant),
        ];
        for (name, site) in legal {
            assert!(super::rule_allows(name, site), "#{name} at {site:?}");
        }

        let illegal = [
            ("Job", RuleSite::Field),
            ("Skip", RuleSite::Variant),
            ("Job", RuleSite::Method),
            ("HTML", RuleSite::Module),
            ("Inline", RuleSite::File),
            ("Bench", RuleSite::Test),
            ("Test", RuleSite::Bench),
            ("Codable", RuleSite::Function),
            ("Doc", RuleSite::Function),
        ];
        for (name, site) in illegal {
            assert!(!super::rule_allows(name, site), "#{name} at {site:?}");
        }
    }

    #[test]
    fn authority_rows_are_site_bound_and_policy_is_lexical() {
        let rows = super::applied_rule_registry();
        for name in ["Unsafe", "Grant", "Scrub", "wire"] {
            let row = rows.iter().find(|row| row.name == name).unwrap();
            assert_eq!(row.resolution, super::RuleResolution::SiteBound);
            assert!(row.policy_scopes.is_empty());
        }
        let policy = rows.iter().find(|row| row.name == "Policy").unwrap();
        assert_eq!(policy.policy_scopes, &[super::PolicyScope::Package, super::PolicyScope::Module, super::PolicyScope::Function, super::PolicyScope::Block]);
        assert!(!super::rule_allows("Pure", super::RuleSite::Type));
        assert!(!super::rule_allows("Codable", super::RuleSite::Function));
        assert!(!super::rule_allows("Doc", super::RuleSite::Block));
        assert!(!super::rule_allows("Region", super::RuleSite::File));
        assert!(!super::rule_allows("PubFile", super::RuleSite::Field));
    }

    #[test]
    fn every_typed_marker_argument_has_one_core_lang_declaration() {
        // One declaration per typed marker-argument menu, plus the `Track`
        // reflection enum retained by D-RULEARG-TYPES1.
        assert_eq!(super::RULE_ARG_DECLARATIONS.len(), 18);
        let mut expected = std::collections::BTreeSet::from(["Site", "Track"]);
        for row in super::APPLIED_RULES.iter() {
            expected.extend(row.signature.params.iter().map(|parameter| parameter.source_type));
            expected.extend(row.signature.variadic_source_type);
        }
        expected.retain(|name| super::canonical_rule_arg_variants(name).is_some());
        let actual = super::RULE_ARG_DECLARATIONS
            .iter()
            .map(|declaration| declaration.name)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(actual, expected);
        for declaration in super::RULE_ARG_DECLARATIONS.iter() {
            assert_eq!(
                declaration.variants,
                super::canonical_rule_arg_variants(declaration.name).unwrap(),
                "{}",
                declaration.name
            );
        }
        for row in super::APPLIED_RULES.iter() {
            for param in row.signature.params {
                if matches!(
                    param.source_type,
                    "Value" | "String" | "Ident" | "Bool" | "Int" | "Duration | String" | "[Effect]" | "T.default"
                ) {
                    continue;
                }
                assert!(
                    super::rule_arg_declaration(param.source_type).is_some(),
                    "#{} parameter {} names missing core.lang.{}",
                    row.name,
                    param.name,
                    param.source_type
                );
            }
            if let Some(source_type) = row.signature.variadic_source_type {
                if !matches!(
                    source_type,
                    "Value" | "String" | "Ident" | "Bool" | "Int" | "Duration | String"
                ) {
                    assert!(
                        super::rule_arg_declaration(source_type).is_some(),
                        "#{} variadic type core.lang.{source_type}",
                        row.name
                    );
                }
            }
        }
    }

    #[test]
    fn generated_rule_argument_variants_match_canonical_surface() {
        let variants = |name| super::rule_arg_declaration(name).unwrap().variants;
        assert_eq!(
            variants("ABI"),
            &["system", "cdecl", "stdcall", "fastcall", "win64", "sysv64"]
        );
        assert_eq!(variants("FfiLanguage"), &["c", "cpp", "asm"]);
        assert_eq!(variants("Layout"), &["c", "columnar"]);
        assert_eq!(
            variants("NamingCase"),
            &["camel", "snake", "pascal", "kebab", "screaming"]
        );
        assert_eq!(variants("Capability"), crate::Facts::EFFECT_ROOTS.as_slice());
    }

    /// D-MARK-FORM1=A / D-MARK-REPEAT1=A / D-VERDICT-1455-1: the facts the
    /// consumers used to hard-code by name are registry columns now.
    #[test]
    fn marker_behaviour_lives_in_columns_not_name_lists() {
        let row = |name| super::applied_rule(name).unwrap();

        // Repeatable is a column: contracts and lints repeat, nothing else does.
        for name in ["Pre", "Post", "allow"] {
            assert!(row(name).repeatable, "{name} must be repeatable");
        }
        for name in ["Inline", "Codable", "Job", "PubFile", "NoPrelude"] {
            assert!(!row(name).repeatable, "{name} must not be repeatable");
        }

        // Owning a menu is a column, not a name match in the shared binder.
        for name in ["FFI", "RenameAll"] {
            assert!(row(name).owns_menu, "{name} teaches its own menu");
        }
        assert!(!row("Layout").owns_menu);

        // The Doc-with-Job coupling is row data.
        let companion = row("Doc").companion_site.expect("Doc declares a companion");
        assert_eq!(companion.rule, "Job");
        assert_eq!(companion.site, super::RuleSite::Function);
        assert!(!super::rule_allows("Doc", super::RuleSite::Function));
        assert!(super::rule_allows_with_companions(
            "Doc",
            super::RuleSite::Function,
            ["Job", "Doc"].into_iter()
        ));
        assert!(!super::rule_allows_with_companions(
            "Doc",
            super::RuleSite::Function,
            ["Doc"].into_iter()
        ));
        // A companion never opens a site the column did not name.
        assert!(!super::rule_allows_with_companions(
            "Doc",
            super::RuleSite::Block,
            ["Job", "Doc"].into_iter()
        ));

        // Which path segment names a variant is declaration data.
        assert_eq!(
            super::rule_arg_declaration("Capability").unwrap().variant_segment,
            super::VariantSegment::First
        );
        assert_eq!(
            super::rule_arg_declaration("Target").unwrap().variant_segment,
            super::VariantSegment::First
        );
        assert_eq!(
            super::rule_arg_declaration("NamingCase").unwrap().variant_segment,
            super::VariantSegment::Last
        );

        // D-VERDICT-1455-1: the two ghost rows are gone for good.
        assert!(super::applied_rule("Authority").is_none());
        assert!(super::applied_rule("Summarize").is_none());
        assert!(!super::DERIVE_RULES.contains(&"Summarize"));
    }

    #[test]
    fn nonderive_type_markers_cannot_rhyme_with_traits() {
        assert!(super::nonderive_marker_trait_collision("Layout"));
        assert!(super::nonderive_marker_trait_collision("Discriminant"));
        assert!(!super::nonderive_marker_trait_collision("Comparable"));
        assert!(!super::nonderive_marker_trait_collision("Debug"));
    }

    #[test]
    fn rule_argument_bindings_cover_variadics_and_named_arguments() {
        let policy = super::applied_rule("Policy").unwrap().signature;
        assert_eq!(policy.argument_bindings(&[]), Some(Vec::new()));
        assert_eq!(
            policy.argument_types(&[None, None, None]),
            Some(vec![
                super::RuleArgType::Any,
                super::RuleArgType::Any,
                super::RuleArgType::Any,
            ])
        );
        assert!(policy.argument_bindings(&[Some("setting")]).is_none());
        let meta = super::applied_rule("Meta").unwrap().signature;
        let reordered = meta
            .argument_bindings(&[Some("maturity"), Some("category")])
            .unwrap();
        assert_eq!(
            reordered
                .iter()
                .map(|binding| binding.parameter_index)
                .collect::<Vec<_>>(),
            vec![Some(2), Some(0)]
        );
        assert_eq!(
            meta.argument_types(&[Some("maturity"), Some("category")]),
            Some(vec![super::RuleArgType::Ident, super::RuleArgType::String])
        );
        assert!(meta.argument_bindings(&[None, Some("maturity")]).is_some());
        assert!(meta.argument_bindings(&[Some("maturity")]).is_some());
        assert!(
            meta.argument_bindings(&[Some("category"), None])
                .is_none()
        );
        assert!(
            meta.argument_bindings(&[Some("category"), Some("category")])
                .is_none()
        );
        assert!(meta.argument_types(&[Some("unknown")]).is_none());
        let transition = super::applied_rule("Transition").unwrap().signature;
        assert!(transition.argument_bindings(&[Some("to")]).is_none());
    }

    #[test]
    fn package_module_function_block_chain_keeps_provenance_and_rejects_widening() {
        let declaration = |scope, value, start, source: &str| super::PolicyDeclaration {
            key: super::PolicyKey::ArenaBounded,
            value: super::PolicyValue::Limit(value),
            scope,
            span: crate::Diagnostics::Span::new(start, start + 1),
            target: None,
            source: source.to_string(),
        };
        let chain = vec![
            declaration(super::PolicyScope::Package, 65536, 1, "package.jet"),
            declaration(super::PolicyScope::Module, 32768, 2, "Source/main.jet"),
            declaration(super::PolicyScope::Function, 16384, 3, "Source/main.jet"),
            declaration(super::PolicyScope::Block, 8192, 4, "Source/main.jet"),
        ];
        let effective = super::resolve(super::PolicyKey::ArenaBounded, chain).unwrap().unwrap();
        assert_eq!(effective.value, super::PolicyValue::Limit(8192));
        assert_eq!(effective.provenance.len(), 4);
        let widening = vec![
            declaration(super::PolicyScope::Package, 1024, 1, "package.jet"),
            declaration(super::PolicyScope::Module, 2048, 2, "Source/main.jet"),
        ];
        assert!(matches!(super::resolve(super::PolicyKey::ArenaBounded, widening), Err(super::PolicyError::Widening { .. })));
    }

    #[test]
    fn contribution_law_orders_all_scopes_and_layers() {
        let key = super::FactKey::new("Build.Profile");
        let contribution = |value, scope, layer, source| {
            super::FactContribution::new(
                "Build.Profile",
                super::FactValue::Text(value.to_string()),
                scope,
                layer,
                source,
            )
        };
        let resolved = super::resolve(
            key.clone(),
            [
                contribution("package", super::SourceScope::Package, super::ContributionLayer::Declaration, "package.jet"),
                contribution("file", super::SourceScope::File, super::ContributionLayer::Declaration, "src/main.jet"),
                contribution("module", super::SourceScope::Module, super::ContributionLayer::Declaration, "src/main.jet"),
                contribution("function", super::SourceScope::Function, super::ContributionLayer::Declaration, "src/main.jet"),
                contribution("block", super::SourceScope::Block, super::ContributionLayer::Declaration, "src/main.jet"),
                contribution("item", super::SourceScope::Item, super::ContributionLayer::Declaration, "src/main.jet"),
                contribution("bundle", super::SourceScope::Package, super::ContributionLayer::OptimizationBundle, "build bundle"),
                contribution("workspace", super::SourceScope::Package, super::ContributionLayer::Workspace, "workspace.jet"),
                contribution("environment", super::SourceScope::Package, super::ContributionLayer::Environment, "env.dev"),
                contribution("system", super::SourceScope::Package, super::ContributionLayer::System, "system.dev"),
                contribution("fleet", super::SourceScope::Package, super::ContributionLayer::Fleet, "fleet.ci"),
                contribution("cli", super::SourceScope::Package, super::ContributionLayer::CommandLine, "command line"),
            ],
        )
        .unwrap()
        .unwrap();
        assert_eq!(resolved.key, key);
        assert_eq!(resolved.value, super::FactValue::Text("cli".to_string()));
        assert_eq!(resolved.provenance.len(), 12);
        assert_eq!(resolved.effective, 11);
    }

    #[test]
    fn contribution_law_conflicts_same_layer_and_force_pins_later_layers() {
        let key = super::FactKey::new("Build.Profile");
        let same_layer = [
            super::FactContribution::new(
                "Build.Profile",
                super::FactValue::Text("debug".to_string()),
                super::SourceScope::Package,
                super::ContributionLayer::Workspace,
                "workspace-a.jet",
            )
            .at(crate::Diagnostics::Span::new(10, 11)),
            super::FactContribution::new(
                "Build.Profile",
                super::FactValue::Text("release".to_string()),
                super::SourceScope::Package,
                super::ContributionLayer::Workspace,
                "workspace-b.jet",
            )
            .at(crate::Diagnostics::Span::new(20, 21)),
        ];
        let error = super::resolve(key.clone(), same_layer).unwrap_err();
        assert!(matches!(&error, super::FactError::Conflict { .. }));
        assert!(error.diagnostic().is_some());

        let resolved = super::resolve(
            key,
            [
                super::FactContribution::new(
                    "Build.Profile",
                    super::FactValue::Text("debug".to_string()),
                    super::SourceScope::Package,
                    super::ContributionLayer::Declaration,
                    "package.jet",
                ),
                super::FactContribution::new(
                    "Build.Profile",
                    super::FactValue::Text("release".to_string()),
                    super::SourceScope::Package,
                    super::ContributionLayer::System,
                    "system.jet",
                )
                .force_with_reason("release certification"),
                super::FactContribution::new(
                    "Build.Profile",
                    super::FactValue::Text("debug".to_string()),
                    super::SourceScope::Package,
                    super::ContributionLayer::CommandLine,
                    "command line",
                ),
            ],
        )
        .unwrap()
        .unwrap();
        assert_eq!(resolved.value, super::FactValue::Text("release".to_string()));
        let explanation = super::explain(&resolved);
        assert!(explanation.contains("[effective] system"));
        assert!(explanation.contains("pin=release certification"));
        assert!(explanation.contains("[shadowed] command line"));
    }

    #[test]
    fn tighten_only_facts_reject_widening() {
        let key = super::FactKey::tighten_only("Build.Limit");
        let declarations = [
            super::FactContribution::new(
                "Build.Limit",
                super::FactValue::Int(8),
                super::SourceScope::Package,
                super::ContributionLayer::Declaration,
                "package.jet",
            ),
            super::FactContribution::new(
                "Build.Limit",
                super::FactValue::Int(4),
                super::SourceScope::Package,
                super::ContributionLayer::Workspace,
                "workspace.jet",
            ),
        ];
        assert!(super::resolve(key.clone(), declarations).is_ok());
        let widening = [
            super::FactContribution::new(
                "Build.Limit",
                super::FactValue::Int(4),
                super::SourceScope::Package,
                super::ContributionLayer::Declaration,
                "package.jet",
            ),
            super::FactContribution::new(
                "Build.Limit",
                super::FactValue::Int(8),
                super::SourceScope::Package,
                super::ContributionLayer::Workspace,
                "workspace.jet",
            ),
        ];
        assert!(matches!(super::resolve(key, widening), Err(super::FactError::SafetyWidening { .. })));
    }
}
