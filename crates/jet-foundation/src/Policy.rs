//! Compiler-owned scoped policy registry and resolution ladder (D-MARK-SCOPE1).

use crate::Diagnostics::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolicyScope { Organization, Package, Module, Function, Block }

impl PolicyScope {
    pub const fn name(self) -> &'static str { match self { Self::Organization => "organization", Self::Package => "package", Self::Module => "module", Self::Function => "function", Self::Block => "block" } }
    const fn rank(self) -> u8 { match self { Self::Organization => 0, Self::Package => 1, Self::Module => 2, Self::Function => 3, Self::Block => 4 } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PolicyKey { NoAlloc, ZeroRc, ArenaBounded, Unsafe, ScopedGc, ExplicitUnits }

impl PolicyKey {
    pub const fn name(self) -> &'static str { match self { Self::NoAlloc => "no_alloc", Self::ZeroRc => "zero_rc", Self::ArenaBounded => "arena_bounded", Self::Unsafe => "unsafe", Self::ScopedGc => "gc", Self::ExplicitUnits => "explicit_units" } }
    pub fn parse(name: &str) -> Option<Self> { match name { "no_alloc" => Some(Self::NoAlloc), "zero_rc" => Some(Self::ZeroRc), "arena_bounded" => Some(Self::ArenaBounded), "unsafe" => Some(Self::Unsafe), "gc" => Some(Self::ScopedGc), "explicit_units" => Some(Self::ExplicitUnits), _ => None } }
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
    PolicyRule { key: PolicyKey::ExplicitUnits, scopes: ALL_SCOPES, combine: PolicyCombine::Tighten },
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
                (PolicyKey::NoAlloc | PolicyKey::ZeroRc | PolicyKey::ScopedGc | PolicyKey::ExplicitUnits, PolicyValue::Enabled, PolicyValue::Enabled) => false,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AppliedRule {
    pub name: &'static str,
    /// Typed source signature shown by parser diagnostics and `jet explain`.
    pub signature: &'static str,
    /// Non-empty only for rules that participate in lexical policy inheritance.
    pub policy_scopes: &'static [PolicyScope],
    /// Exact sound attachment sites for non-inheriting rules.
    pub sites: &'static [RuleSite],
    pub form: RuleForm,
    pub status: RuleStatus,
    pub inherits: bool,
    pub resolution: RuleResolution,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleForm {
    Bare,
    Call,
    BareOrCall,
    Block,
    Prefix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleStatus {
    Active,
    Retired { replacement: &'static str },
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

macro_rules! rule {
    ($name:expr, $sig:expr, $sites:expr, $form:ident) => {
        AppliedRule {
            name: $name,
            signature: $sig,
            policy_scopes: NO_POLICY_SCOPES,
            sites: $sites,
            form: RuleForm::$form,
            status: RuleStatus::Active,
            inherits: false,
            resolution: RuleResolution::SiteBound,
        }
    };
    (retired $name:expr, $sig:expr, $sites:expr, $form:ident, $replacement:expr) => {
        AppliedRule {
            name: $name,
            signature: $sig,
            policy_scopes: NO_POLICY_SCOPES,
            sites: $sites,
            form: RuleForm::$form,
            status: RuleStatus::Retired { replacement: $replacement },
            inherits: false,
            resolution: RuleResolution::SiteBound,
        }
    };
}

/// D-MARKSIG1=A: the sole compiler registry for active and retired markers.
/// Parser, sema, formatter, LSP, highlighting, explain, and retirement
/// diagnostics read these rows.
pub const APPLIED_RULES: &[AppliedRule] = &[
    AppliedRule {
        name: "Policy",
        signature: "(setting: PolicySetting, ...)",
        policy_scopes: ALL_SCOPES,
        sites: &[RuleSite::Package, RuleSite::Module, RuleSite::Function, RuleSite::Block],
        form: RuleForm::Call,
        status: RuleStatus::Active,
        inherits: true,
        resolution: RuleResolution::Tighten,
    },
    rule!("Unsafe", "(reason: String, obligations: ObligationMode = .None)", &[RuleSite::Function, RuleSite::Block, RuleSite::Operation], Call),
    rule!("Grant", "(capability: Capability, ...)", &[RuleSite::Block, RuleSite::Operation], Call),
    rule!("Tainted", "(kind: TaintKind = .Input)", &[RuleSite::Expression, RuleSite::Operation], BareOrCall),
    rule!("Sanitizer", "()", FUNCTION_SITE, Bare),
    rule!("Pure", "()", FUNCTION_SITE, Bare),
    rule!("Pre", "(condition: Bool, message: String)", FUNCTION_SITE, Call),
    rule!("Post", "(condition: Bool, message: String)", FUNCTION_SITE, Call),
    rule!("Inline", "(mode: InlineMode = .Hint)", &[RuleSite::Function, RuleSite::Constant], BareOrCall),
    rule!("Task", "()", FUNCTION_SITE, Bare),
    rule!("Every", "(schedule: Duration | String)", FUNCTION_SITE, Call),
    rule!("Replayable", "()", FUNCTION_SITE, Bare),
    rule!("WasmExport", "()", FUNCTION_SITE, Bare),
    rule!("State", "(state: State)", FUNCTION_SITE, Call),
    rule!("Transition", "(from: State, to: State)", FUNCTION_SITE, Call),
    rule!("FFI", "(language: FfiLanguage)", FUNCTION_SITE, Call),
    rule!("Abi", "(name: Abi)", FUNCTION_SITE, Call),
    rule!("MustUse", "()", &[RuleSite::Function, RuleSite::Type], Bare),
    rule!("Codable", "()", TYPE_SITE, Bare),
    rule!("Encode", "()", TYPE_SITE, Bare),
    rule!("Decode", "()", TYPE_SITE, Bare),
    rule!("PublishedSchema", "()", TYPE_SITE, Bare),
    rule!("Summarize", "()", TYPE_SITE, Bare),
    rule!("Comparable", "()", TYPE_SITE, Bare),
    rule!("Numeric", "()", TYPE_SITE, Bare),
    rule!("Printable", "()", TYPE_SITE, Bare),
    rule!("CodableAsBase", "()", TYPE_SITE, Bare),
    rule!("Cli", "()", TYPE_SITE, Bare),
    rule!("Patchable", "()", TYPE_SITE, Bare),
    rule!("UnitFamily", "(family: Ident, base: Ident = first member)", TYPE_SITE, Call),
    rule!("SingleUse", "()", TYPE_SITE, Bare),
    rule!("Invariant", "(condition: String)", TYPE_SITE, Call),
    rule!("Layout", "(kind: Layout, tag: IntType = I32)", TYPE_SITE, Call),
    rule!("RenameAll", "(case: NamingCase)", TYPE_SITE, Call),
    rule!("DenyUnknownFields", "()", TYPE_SITE, Bare),
    rule!("Tag", "(field: String)", TYPE_SITE, Call),
    rule!("Untagged", "()", TYPE_SITE, Bare),
    rule!("Redact", "()", FIELD_SITE, Bare),
    rule!("Rename", "(name: String)", FIELD_SITE, Call),
    rule!("Skip", "()", FIELD_SITE, Bare),
    rule!("Default", "(value: T = T.default)", FIELD_SITE, BareOrCall),
    rule!("Flatten", "()", FIELD_SITE, Bare),
    rule!("Doc", "(text: String)", FIELD_SITE, Call),
    rule!("Flag", "()", FIELD_SITE, Bare),
    rule!("Persist", "()", DECLARATION_SITE, Bare),
    rule!("Track", "()", DECLARATION_SITE, Bare),
    rule!("Local", "()", DECLARATION_SITE, Bare),
    rule!("Shared", "()", DECLARATION_SITE, Bare),
    rule!("Meta", "(category: String = \"\", tunable: Bool = false, maturity: Maturity = .Tested)", &[RuleSite::Function, RuleSite::Declaration, RuleSite::Constant], Call),
    rule!("Todo", "()", EXPR_SITE, Bare),
    rule!("Shield", "()", BLOCK_SITE, Block),
    rule!("Impure", "(reason: String)", BLOCK_SITE, Block),
    rule!("Caps", "(capability: Capability, ...)", BLOCK_SITE, Block),
    rule!("Transact", "(name: Ident)", BLOCK_SITE, Block),
    rule!("Region", "(name: Ident)", BLOCK_SITE, Block),
    rule!("Live", "()", BLOCK_SITE, Block),
    rule!("Nondeterministic", "(reason: String)", BLOCK_SITE, Block),
    rule!("Context", "(allocator: Allocator = default, logger: Logger = default, deadline: Int = default)", BLOCK_SITE, Block),
    rule!("Reactive", "()", &[RuleSite::Function, RuleSite::Block], Block),
    rule!("Off", "()", STATEMENT_SITE, Prefix),
    rule!("DebugOnly", "()", STATEMENT_SITE, Prefix),
    rule!("Test", "(name: String)", &[RuleSite::Test], Block),
    rule!("Bench", "(name: String)", &[RuleSite::Bench], Block),
    rule!("Target", "(target: Target)", &[RuleSite::File, RuleSite::Module, RuleSite::Function], Call),
    rule!("Html", "(path: String)", FILE_SITE, Call),
    rule!("PubFile", "()", FILE_SITE, Bare),
    rule!("NoPrelude", "()", FILE_SITE, Bare),
    rule!("Sql", "()", BLOCK_SITE, Block),
    rule!("Extern", "(library: String)", MODULE_SITE, Call),
    rule!("Bindgen", "(library: String)", MODULE_SITE, Call),
    rule!("allow", "(lint: Ident)", &[RuleSite::Declaration, RuleSite::Statement], Call),
    rule!("Static", "()", CONST_SITE, Bare),
    rule!("Authority", "()", &[RuleSite::Operation], Bare),
    rule!("wire", "()", FIELD_SITE, Bare),
    rule!(retired "InlineAlways", "()", FUNCTION_SITE, Bare, "#Inline(Always)"),
    rule!(retired "static", "()", CONST_SITE, Bare, "#Static"),
    rule!(retired "inline", "()", CONST_SITE, Bare, "#Inline"),
    rule!(retired "Add", "()", EXPR_SITE, Bare, ".Add"),
    rule!(retired "Mul", "()", EXPR_SITE, Bare, ".Mul"),
    rule!(retired "Min", "()", EXPR_SITE, Bare, ".Min"),
    rule!(retired "Max", "()", EXPR_SITE, Bare, ".Max"),
    rule!(retired "Audit", "(reason: String)", &[RuleSite::Function, RuleSite::Block], Call, "#Unsafe(reason)"),
    rule!(retired "Debug", "()", TYPE_SITE, Bare, "remove the marker; Debug derives automatically"),
    rule!(retired "Wasm", "()", FUNCTION_SITE, Bare, "#Target(Wasm)"),
    rule!(retired "Js", "()", FUNCTION_SITE, Bare, "#Target(Js)"),
    rule!(retired "Suppress", "(reason: String)", BLOCK_SITE, Block, ".drop(\"reason\")"),
    rule!(retired "Uninit", "()", FIELD_SITE, Bare, "give the field a real initial value — stored uninitialized-sentinel fields were retired outright (D-UNINIT-SENTINEL1)"),
    rule!(retired "Ref", "()", FIELD_SITE, Bare, "use an owned value"),
];

pub fn applied_rule_registry() -> &'static [AppliedRule] {
    APPLIED_RULES
}

pub fn applied_rule(name: &str) -> Option<&'static AppliedRule> {
    APPLIED_RULES.iter().find(|row| row.name == name)
}

pub fn rule_allows(name: &str, site: RuleSite) -> bool {
    applied_rule(name).is_some_and(|row| row.sites.contains(&site))
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_registered_rule_has_one_exact_applicability_row() {
        let rows = super::applied_rule_registry();
        for row in rows {
            assert_eq!(rows.iter().filter(|candidate| candidate.name == row.name).count(), 1, "{}", row.name);
            assert!(!row.sites.is_empty(), "{}", row.name);
            assert!(!row.signature.is_empty(), "{}", row.name);
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
