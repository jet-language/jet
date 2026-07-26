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
    /// Executable source signature shared by parsing, diagnostics, and explain.
    pub signature: RuleSignature,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleArgType {
    Any,
    String,
    Ident,
    Bool,
    Int,
    DurationOrString,
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
}

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

macro_rules! param {
    ($name:literal, $ty:ident) => {
        RuleParam { name: $name, ty: RuleArgType::$ty, source_type: RuleArgType::$ty.name(), default: None }
    };
    ($name:literal, $ty:ident, $default:literal) => {
        RuleParam { name: $name, ty: RuleArgType::$ty, source_type: RuleArgType::$ty.name(), default: Some($default) }
    };
    ($name:literal, $ty:ident => $source_type:literal) => {
        RuleParam { name: $name, ty: RuleArgType::$ty, source_type: $source_type, default: None }
    };
    ($name:literal, $ty:ident => $source_type:literal, $default:literal) => {
        RuleParam { name: $name, ty: RuleArgType::$ty, source_type: $source_type, default: Some($default) }
    };
}

macro_rules! sig {
    () => {
        RuleSignature { params: &[], variadic: None, variadic_source_type: None }
    };
    ($($param:expr),+ $(,)?) => {
        RuleSignature { params: &[$($param),+], variadic: None, variadic_source_type: None }
    };
    (variadic $ty:ident) => {
        RuleSignature { params: &[], variadic: Some(RuleArgType::$ty), variadic_source_type: Some(RuleArgType::$ty.name()) }
    };
    (variadic $ty:ident => $source_type:literal) => {
        RuleSignature { params: &[], variadic: Some(RuleArgType::$ty), variadic_source_type: Some($source_type) }
    };
}

/// D-MARKSIG1=A: the sole compiler registry for active and retired markers.
/// Parser, sema, formatter, LSP, highlighting, explain, and retirement
/// diagnostics read these rows.
pub const APPLIED_RULES: &[AppliedRule] = &[
    AppliedRule {
        name: "Policy",
        signature: sig!(variadic Any => "PolicySetting"),
        policy_scopes: ALL_SCOPES,
        sites: &[RuleSite::Package, RuleSite::Module, RuleSite::Function, RuleSite::Block],
        form: RuleForm::Call,
        status: RuleStatus::Active,
        inherits: true,
        resolution: RuleResolution::Tighten,
    },
    rule!("Unsafe", sig!(param!("reason", String), param!("obligations", Ident => "ObligationMode", ".None")), &[RuleSite::Function, RuleSite::Block, RuleSite::Operation], Call),
    rule!("Grant", sig!(variadic Ident => "Capability"), &[RuleSite::Block, RuleSite::Operation], Call),
    rule!("Tainted", sig!(param!("kind", Ident => "TaintKind", ".Input")), &[RuleSite::Expression, RuleSite::Operation], BareOrCall),
    rule!("Sanitizer", sig!(), FUNCTION_SITE, Bare),
    rule!(retired "Pure", sig!(), FUNCTION_SITE, Bare, "pure fn"),
    rule!("Pre", sig!(param!("condition", Any), param!("message", String)), FUNCTION_SITE, Call),
    rule!("Post", sig!(param!("condition", Any), param!("message", String)), FUNCTION_SITE, Call),
    rule!("Inline", sig!(param!("mode", Ident => "InlineMode", ".Hint")), &[RuleSite::Function, RuleSite::Constant], BareOrCall),
    rule!("Task", sig!(), FUNCTION_SITE, Bare),
    rule!("Every", sig!(param!("schedule", DurationOrString)), FUNCTION_SITE, Call),
    rule!("Replayable", sig!(), FUNCTION_SITE, Bare),
    rule!("WasmExport", sig!(), FUNCTION_SITE, Bare),
    rule!("State", sig!(param!("state", Ident => "State")), FUNCTION_SITE, Call),
    rule!("Transition", sig!(param!("from", Ident => "State"), param!("to", Ident => "State")), FUNCTION_SITE, Call),
    rule!("FFI", sig!(param!("language", Ident => "FfiLanguage")), FUNCTION_SITE, Call),
    rule!("Abi", sig!(param!("name", Ident => "Abi")), FUNCTION_SITE, Call),
    rule!("MustUse", sig!(), &[RuleSite::Function, RuleSite::Type], Bare),
    rule!("Codable", sig!(), TYPE_SITE, Bare),
    rule!("Encode", sig!(), TYPE_SITE, Bare),
    rule!("Decode", sig!(), TYPE_SITE, Bare),
    rule!("PublishedSchema", sig!(), TYPE_SITE, Bare),
    rule!("Summarize", sig!(), TYPE_SITE, Bare),
    rule!("Comparable", sig!(), TYPE_SITE, Bare),
    rule!("Numeric", sig!(), TYPE_SITE, Bare),
    rule!("Printable", sig!(), TYPE_SITE, Bare),
    rule!("CodableAsBase", sig!(), TYPE_SITE, Bare),
    rule!("Cli", sig!(), TYPE_SITE, Bare),
    rule!("Patchable", sig!(), TYPE_SITE, Bare),
    rule!("UnitFamily", sig!(param!("family", Ident), param!("base", Ident, "first member")), TYPE_SITE, Call),
    rule!("SingleUse", sig!(), TYPE_SITE, Bare),
    rule!("Invariant", sig!(param!("condition", String)), TYPE_SITE, Call),
    rule!("Layout", sig!(param!("kind", Ident => "Layout"), param!("tag", Ident => "IntType", "I32")), TYPE_SITE, Call),
    rule!("RenameAll", sig!(param!("case", Ident => "NamingCase")), TYPE_SITE, Call),
    rule!("DenyUnknownFields", sig!(), TYPE_SITE, Bare),
    rule!("Tag", sig!(param!("field", String)), TYPE_SITE, Call),
    rule!("Untagged", sig!(), TYPE_SITE, Bare),
    rule!("Redact", sig!(), FIELD_SITE, Bare),
    rule!("Rename", sig!(param!("name", String)), FIELD_SITE, Call),
    rule!("Skip", sig!(), FIELD_SITE, Bare),
    rule!("Default", sig!(param!("value", Any, "T.default")), FIELD_SITE, BareOrCall),
    rule!("Flatten", sig!(), FIELD_SITE, Bare),
    rule!("Doc", sig!(param!("text", String)), FIELD_SITE, Call),
    rule!("Flag", sig!(), FIELD_SITE, Bare),
    rule!("Persist", sig!(), DECLARATION_SITE, Bare),
    rule!("Track", sig!(), DECLARATION_SITE, Bare),
    rule!("Local", sig!(), DECLARATION_SITE, Bare),
    rule!("Shared", sig!(), DECLARATION_SITE, Bare),
    rule!("Meta", sig!(param!("category", String, "\"\""), param!("tunable", Bool, "false"), param!("maturity", Ident => "Maturity", ".Tested")), &[RuleSite::Function, RuleSite::Declaration, RuleSite::Constant], Call),
    rule!("Todo", sig!(), EXPR_SITE, Bare),
    rule!("Shield", sig!(), BLOCK_SITE, Block),
    rule!("Impure", sig!(param!("reason", String, "none")), BLOCK_SITE, Block),
    rule!("Caps", sig!(variadic Ident => "Capability"), BLOCK_SITE, Block),
    rule!("Transact", sig!(param!("name", Ident)), BLOCK_SITE, Block),
    rule!("Region", sig!(param!("name", Ident)), BLOCK_SITE, Block),
    rule!("Live", sig!(), BLOCK_SITE, Block),
    rule!("Nondeterministic", sig!(param!("reason", String)), BLOCK_SITE, Block),
    rule!("Context", sig!(param!("allocator", Any, "default"), param!("logger", Any, "default"), param!("deadline", Int, "default")), BLOCK_SITE, Block),
    rule!("Reactive", sig!(), &[RuleSite::Function, RuleSite::Block], Block),
    rule!("Off", sig!(), STATEMENT_SITE, Prefix),
    rule!("DebugOnly", sig!(), STATEMENT_SITE, Prefix),
    rule!("Test", sig!(param!("name", String)), &[RuleSite::Test], Block),
    rule!("Bench", sig!(param!("name", String)), &[RuleSite::Bench], Block),
    rule!("Target", sig!(param!("target", Ident => "Target")), &[RuleSite::File, RuleSite::Module, RuleSite::Function], Call),
    rule!("Html", sig!(param!("path", String)), FILE_SITE, Call),
    rule!("PubFile", sig!(), FILE_SITE, Bare),
    rule!("NoPrelude", sig!(), FILE_SITE, Bare),
    rule!("Sql", sig!(), BLOCK_SITE, Block),
    rule!("Extern", sig!(param!("library", String)), MODULE_SITE, Call),
    rule!("Bindgen", sig!(param!("library", String)), MODULE_SITE, Call),
    rule!("allow", sig!(param!("lint", Ident)), &[RuleSite::Declaration, RuleSite::Statement], Call),
    rule!("Static", sig!(), CONST_SITE, Bare),
    rule!("Authority", sig!(), &[RuleSite::Operation], Bare),
    rule!("wire", sig!(), FIELD_SITE, Bare),
    rule!(retired "InlineAlways", sig!(), FUNCTION_SITE, Bare, "#Inline(Always)"),
    rule!(retired "static", sig!(), CONST_SITE, Bare, "#Static"),
    rule!(retired "inline", sig!(), CONST_SITE, Bare, "#Inline"),
    rule!(retired "Add", sig!(), EXPR_SITE, Bare, ".Add"),
    rule!(retired "Mul", sig!(), EXPR_SITE, Bare, ".Mul"),
    rule!(retired "Min", sig!(), EXPR_SITE, Bare, ".Min"),
    rule!(retired "Max", sig!(), EXPR_SITE, Bare, ".Max"),
    rule!(retired "Audit", sig!(param!("reason", String)), &[RuleSite::Function, RuleSite::Block], Call, "#Unsafe(reason)"),
    rule!(retired "Debug", sig!(), TYPE_SITE, Bare, "remove the marker; Debug derives automatically"),
    rule!(retired "Wasm", sig!(), FUNCTION_SITE, Bare, "#Target(Wasm)"),
    rule!(retired "Js", sig!(), FUNCTION_SITE, Bare, "#Target(Js)"),
    rule!(retired "Suppress", sig!(param!("reason", String)), BLOCK_SITE, Block, ".drop(\"reason\")"),
    rule!(retired "Uninit", sig!(), FIELD_SITE, Bare, "give the field a real initial value — stored uninitialized-sentinel fields were retired outright (D-UNINIT-SENTINEL1)"),
    rule!(retired "Ref", sig!(), FIELD_SITE, Bare, "use an owned value"),
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
            assert_eq!(row.signature.variadic.is_some(), row.signature.variadic_source_type.is_some(), "{}", row.name);
            assert!(row.signature.required() <= row.signature.params.len(), "{}", row.name);
            for param in row.signature.params {
                assert!(!param.name.is_empty(), "{}", row.name);
                assert!(!param.source_type.is_empty(), "{}", row.name);
            }
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
