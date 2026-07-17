//! Compiler-owned scoped policy registry and resolution ladder (D-MARK-SCOPE1).

use crate::Diagnostics::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolicyScope { Organization, Package, Module, Function, Block }

impl PolicyScope {
    pub const fn name(self) -> &'static str { match self { Self::Organization => "organization", Self::Package => "package", Self::Module => "module", Self::Function => "function", Self::Block => "block" } }
    const fn rank(self) -> u8 { match self { Self::Organization => 0, Self::Package => 1, Self::Module => 2, Self::Function => 3, Self::Block => 4 } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolicyKey { NoAlloc, ZeroRc, ArenaBounded, Unsafe, ScopedGc }

impl PolicyKey {
    pub const fn name(self) -> &'static str { match self { Self::NoAlloc => "no_alloc", Self::ZeroRc => "zero_rc", Self::ArenaBounded => "arena_bounded", Self::Unsafe => "unsafe", Self::ScopedGc => "gc" } }
    pub fn parse(name: &str) -> Option<Self> { match name { "no_alloc" => Some(Self::NoAlloc), "zero_rc" => Some(Self::ZeroRc), "arena_bounded" => Some(Self::ArenaBounded), "unsafe" => Some(Self::Unsafe), "gc" => Some(Self::ScopedGc), _ => None } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyValue {
    Enabled,
    Limit(u64),
    UnsafeForbid,
    UnsafeDefault,
    UnsafeGateOnly,
    UnsafeObligations,
    UnsafeRelaxed,
    UnsafePerSite,
    UnsafeTrack,
    UnsafeSkip,
}

impl PolicyValue {
    pub fn display(self) -> String { match self {
        Self::Enabled => "true".into(), Self::Limit(n) => n.to_string(),
        Self::UnsafeForbid => ".Forbid".into(), Self::UnsafeDefault => ".Default".into(),
        Self::UnsafeGateOnly => ".GateOnly".into(), Self::UnsafeObligations => ".Obligations".into(),
        Self::UnsafeRelaxed => ".Relaxed".into(), Self::UnsafePerSite => ".PerSite".into(),
        Self::UnsafeTrack => ".Track".into(), Self::UnsafeSkip => ".Skip".into(),
    } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyCombine { Tighten, Override, Merge }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyRule {
    pub key: PolicyKey,
    pub scopes: &'static [PolicyScope],
    pub combine: PolicyCombine,
}

const ALL_SCOPES: &[PolicyScope] = &[PolicyScope::Package, PolicyScope::Module, PolicyScope::Function, PolicyScope::Block];
const UNSAFE_SCOPES: &[PolicyScope] = &[PolicyScope::Organization, PolicyScope::Package, PolicyScope::Function, PolicyScope::Block];
pub const POLICY_RULES: &[PolicyRule] = &[
    PolicyRule { key: PolicyKey::NoAlloc, scopes: ALL_SCOPES, combine: PolicyCombine::Tighten },
    PolicyRule { key: PolicyKey::ZeroRc, scopes: ALL_SCOPES, combine: PolicyCombine::Tighten },
    PolicyRule { key: PolicyKey::ArenaBounded, scopes: ALL_SCOPES, combine: PolicyCombine::Tighten },
    PolicyRule { key: PolicyKey::Unsafe, scopes: UNSAFE_SCOPES, combine: PolicyCombine::Tighten },
    PolicyRule { key: PolicyKey::ScopedGc, scopes: ALL_SCOPES, combine: PolicyCombine::Override },
];

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
pub fn resolve(key: PolicyKey, declarations: impl IntoIterator<Item = PolicyDeclaration>) -> Result<Option<EffectivePolicy>, PolicyError> {
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
                (PolicyKey::NoAlloc | PolicyKey::ZeroRc | PolicyKey::ScopedGc, PolicyValue::Enabled, PolicyValue::Enabled) => false,
                (PolicyKey::Unsafe, outer, inner) => unsafe_widens(outer, inner),
                _ => true,
            };
            if rule(key).combine == PolicyCombine::Tighten && widens {
                return Err(PolicyError::Widening { key, outer, inner: declaration.value, span: declaration.span });
            }
            if rule(key).combine == PolicyCombine::Merge {
                next = match (outer, declaration.value) {
                    (PolicyValue::Limit(a), PolicyValue::Limit(b)) => PolicyValue::Limit(a.min(b)),
                    (PolicyValue::Enabled, PolicyValue::Enabled) => PolicyValue::Enabled,
                    (PolicyValue::UnsafeForbid, PolicyValue::UnsafeForbid) => PolicyValue::UnsafeForbid,
                    _ => return Err(PolicyError::Conflict { key, scope: declaration.scope, first: chain.last().unwrap().span, second: declaration.span }),
                };
            }
        }
        effective = Some(next);
        chain.push(declaration);
    }
    Ok(effective.map(|value| EffectivePolicy { key, value, provenance: chain }))
}

fn unsafe_widens(outer: PolicyValue, inner: PolicyValue) -> bool {
    use PolicyValue::*;
    match (outer, inner) {
        (UnsafeForbid, UnsafeForbid) => false,
        (UnsafeForbid, _) => true,
        (UnsafeObligations, UnsafeObligations | UnsafeTrack) => false,
        (UnsafeObligations, _) => true,
        (UnsafePerSite, UnsafeTrack | UnsafeSkip) => false,
        (UnsafePerSite, UnsafePerSite) => false,
        (UnsafeDefault | UnsafeGateOnly | UnsafeRelaxed, UnsafeTrack | UnsafeObligations) => false,
        (UnsafeDefault | UnsafeGateOnly | UnsafeRelaxed, UnsafeDefault | UnsafeGateOnly | UnsafeRelaxed | UnsafeSkip) => false,
        (UnsafeTrack, UnsafeTrack) | (UnsafeSkip, UnsafeSkip) => false,
        _ => true,
    }
}

pub fn explain(policy: &EffectivePolicy) -> String {
    let mut out = format!("{} = {}", policy.key.name(), policy.value.display());
    for declaration in &policy.provenance {
        out.push_str(&format!("\n  {} {} at {}:{}..{}", declaration.scope.name(), declaration.value.display(), declaration.source, declaration.span.start, declaration.span.end));
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleSite { Package, File, Module, Function, Block, Statement, Expression, Type, Impl, Declaration, Constant, Field, Parameter, Test, Bench, Operation }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedRule {
    pub name: &'static str,
    /// Non-empty only for rules that participate in lexical policy inheritance.
    pub policy_scopes: &'static [PolicyScope],
    /// Exact sound attachment sites for non-inheriting rules.
    pub sites: &'static [RuleSite],
    pub inherits: bool,
    pub resolution: RuleResolution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleResolution { SiteBound, Override, Merge, Tighten }

const NO_POLICY_SCOPES: &[PolicyScope] = &[];
const FILE_SITE: &[RuleSite] = &[RuleSite::File];
const MODULE_SITE: &[RuleSite] = &[RuleSite::Module];
const FUNCTION_SITE: &[RuleSite] = &[RuleSite::Function];
const BLOCK_SITE: &[RuleSite] = &[RuleSite::Block];
const STATEMENT_SITE: &[RuleSite] = &[RuleSite::Statement, RuleSite::Block];
const TYPE_SITE: &[RuleSite] = &[RuleSite::Type];
const DECLARATION_SITE: &[RuleSite] = &[RuleSite::Declaration];
const FIELD_SITE: &[RuleSite] = &[RuleSite::Field];
const CONST_SITE: &[RuleSite] = &[RuleSite::Constant];
const EXPR_SITE: &[RuleSite] = &[RuleSite::Expression];

/// Exact applicability row for every compiler-registered `@Rule`, plus the two
/// conceptual authority/wire groups whose concrete spellings have their own rows.
pub fn applied_rule_registry() -> Vec<AppliedRule> {
    let mut rows = crate::Syntax::APPLIED_RULES.iter().map(|&name| {
        let (policy_scopes, sites, inherits) = match name {
            "Policy" => (ALL_SCOPES, &[RuleSite::Package, RuleSite::Module, RuleSite::Function, RuleSite::Block][..], true),
            "Unsafe" => (NO_POLICY_SCOPES, &[RuleSite::Function, RuleSite::Block, RuleSite::Operation][..], false),
            "Grant" => (NO_POLICY_SCOPES, &[RuleSite::Block, RuleSite::Operation][..], false),
            "Tainted" => (NO_POLICY_SCOPES, &[RuleSite::Expression, RuleSite::Operation][..], false),
            "Sanitizer" | "Pure" | "Pre" | "Post" | "Inline" | "InlineAlways" | "Task" | "Every" | "Replayable" | "WasmExport" | "State" | "Transition" | "FFI" => (NO_POLICY_SCOPES, FUNCTION_SITE, false),
            "MustUse" => (NO_POLICY_SCOPES, &[RuleSite::Function, RuleSite::Type][..], false),
            "Codable" | "Encode" | "Decode" | "PublishedSchema" | "Summarize" | "Comparable" | "Numeric" | "Printable" | "CodableAsBase" | "Cli" | "Patchable" | "UnitFamily" | "SingleUse" | "Invariant" | "Layout" => (NO_POLICY_SCOPES, TYPE_SITE, false),
            "Redact" | "Rename" | "Skip" | "Default" | "Flatten" => (NO_POLICY_SCOPES, FIELD_SITE, false),
            "RenameAll" | "DenyUnknownFields" | "Tag" | "Untagged" => (NO_POLICY_SCOPES, TYPE_SITE, false),
            "Persist" | "Track" => (NO_POLICY_SCOPES, DECLARATION_SITE, false),
            "Meta" => (NO_POLICY_SCOPES, &[RuleSite::Function, RuleSite::Declaration, RuleSite::Constant][..], false),
            "Doc" => (NO_POLICY_SCOPES, FIELD_SITE, false),
            "Todo" => (NO_POLICY_SCOPES, EXPR_SITE, false),
            "Shield" | "Impure" | "Caps" | "Transact" | "Region" | "Live" | "Nondeterministic" | "Context" => (NO_POLICY_SCOPES, BLOCK_SITE, false),
            "Reactive" => (NO_POLICY_SCOPES, &[RuleSite::Function, RuleSite::Block][..], false),
            "Off" | "DebugOnly" => (NO_POLICY_SCOPES, STATEMENT_SITE, false),
            "Test" => (NO_POLICY_SCOPES, &[RuleSite::Test][..], false),
            "Bench" => (NO_POLICY_SCOPES, &[RuleSite::Bench][..], false),
            "Target" => (NO_POLICY_SCOPES, &[RuleSite::File, RuleSite::Module, RuleSite::Function][..], false),
            "Html" | "PubFile" | "NoPrelude" => (NO_POLICY_SCOPES, FILE_SITE, false),
            "Sql" => (NO_POLICY_SCOPES, BLOCK_SITE, false),
            "Abi" => (NO_POLICY_SCOPES, FUNCTION_SITE, false),
            "Extern" | "Bindgen" => (NO_POLICY_SCOPES, MODULE_SITE, false),
            "allow" => (NO_POLICY_SCOPES, &[RuleSite::Declaration, RuleSite::Statement][..], false),
            "static" | "inline" => (NO_POLICY_SCOPES, CONST_SITE, false),
            "Add" | "Mul" | "Min" | "Max" => (NO_POLICY_SCOPES, EXPR_SITE, false),
            _ => crate::ice!(None, "APPLIED_RULES entry `{name}` lacks an applicability row"),
        };
        AppliedRule { name, policy_scopes, sites, inherits, resolution: if inherits { RuleResolution::Tighten } else { RuleResolution::SiteBound } }
    }).collect::<Vec<_>>();
    rows.push(AppliedRule { name: "Authority", policy_scopes: NO_POLICY_SCOPES, sites: &[RuleSite::Operation], inherits: false, resolution: RuleResolution::SiteBound });
    rows.push(AppliedRule { name: "wire", policy_scopes: NO_POLICY_SCOPES, sites: FIELD_SITE, inherits: false, resolution: RuleResolution::SiteBound });
    rows
}

pub fn applied_rule(name: &str) -> Option<AppliedRule> {
    applied_rule_registry().into_iter().find(|row| row.name == name)
}

pub fn rule_allows(name: &str, site: RuleSite) -> bool {
    applied_rule(name).is_some_and(|row| row.sites.contains(&site))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleDeclaration { pub rule: String, pub site: RuleSite, pub span: Span, pub target: Span }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EffectiveRule { pub rule: String, pub provenance: Vec<RuleDeclaration> }

/// Resolve an applied rule while retaining its complete declaration chain.
/// Site-bound rules reject widening to a different target instead of inheriting.
pub fn resolve_applied_rule(rule: &str, declarations: impl IntoIterator<Item = RuleDeclaration>) -> Result<Option<EffectiveRule>, Span> {
    let Some(row) = applied_rule(rule) else { return Ok(None) };
    let mut provenance = Vec::new();
    let mut target = None;
    for declaration in declarations.into_iter().filter(|d| d.rule == rule) {
        if !row.sites.contains(&declaration.site) { return Err(declaration.span); }
        if !row.inherits && target.is_some_and(|prior| prior != declaration.target) { return Err(declaration.span); }
        target = Some(declaration.target);
        provenance.push(declaration);
    }
    Ok((!provenance.is_empty()).then(|| EffectiveRule { rule: rule.to_string(), provenance }))
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_registered_rule_has_one_exact_applicability_row() {
        let rows = super::applied_rule_registry();
        for name in crate::Syntax::APPLIED_RULES {
            assert_eq!(rows.iter().filter(|row| row.name == *name).count(), 1, "{name}");
            assert!(!rows.iter().find(|row| row.name == *name).unwrap().sites.is_empty(), "{name}");
        }
    }

    #[test]
    fn authority_rows_are_site_bound_and_policy_is_lexical() {
        let rows = super::applied_rule_registry();
        for name in ["Authority", "Unsafe", "Grant", "Tainted", "Sanitizer", "wire"] {
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
}
