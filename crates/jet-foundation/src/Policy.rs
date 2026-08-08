//! Compiler-owned scoped policy registry and resolution ladder (D-MARK-SCOPE1).

use crate::Diagnostics::Span;
use std::sync::LazyLock;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RuleSite { Package, File, Module, Function, Method, Block, Statement, Expression, Type, Impl, Declaration, Constant, Field, Variant, Parameter, Test, Bench, Operation }

impl RuleSite {
    pub const ALL: [Self; 18] = [
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
        }
    }
}

/// D-META-FORM1=A: `$sites` on a `marker` declaration takes `[Site]`, so the
/// eighteen attachment points are published as an ordinary `core.lang` enum
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
    /// same target. `#Doc` is a field rule that also describes a `#Job`.
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
        "KernelMode" => &["parallel"],
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
    // `Site` is published for `$sites` on a `marker` declaration (D-META-FORM1=A)
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
    declared: std::collections::BTreeSet<String>,
}

impl MarkerVocabulary {
    /// The registry, plus the derive providers visible to this build.
    pub fn with_derives(names: impl IntoIterator<Item = String>) -> Self {
        Self { declared: names.into_iter().collect() }
    }

    /// True when a writer may spell this name as a marker.
    pub fn knows(&self, name: &str) -> bool {
        crate::Syntax::is_applied_rule(name) || self.declared.contains(name)
    }

    /// Every spellable name, for a nearest-spelling suggestion.
    pub fn names(&self) -> Vec<String> {
        active_rule_names().into_iter().chain(self.declared.iter().cloned()).collect()
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
    diagnostic.edit = Some(crate::Diagnostics::TextEdit {
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

pub fn parse_invariant_bounds(text: &str) -> Option<(i64, i64)> {
    let mut lo = i64::MIN;
    let mut hi = i64::MAX;
    let mut saw = false;
    for raw in text.split("&&") {
        let clause = raw.trim();
        if clause.is_empty() {
            return None;
        }
        let (new_lo, new_hi) = parse_invariant_clause(clause)?;
        lo = lo.max(new_lo);
        hi = hi.min(new_hi);
        saw = true;
    }
    (saw && lo != i64::MIN && hi != i64::MAX).then_some((lo, hi))
}

fn parse_invariant_clause(clause: &str) -> Option<(i64, i64)> {
    for op in ["<=", ">=", "==", "<", ">"] {
        if let Some((left, right)) = clause.split_once(op) {
            let left = left.trim();
            let right = right.trim();
            return match (left == "value", right == "value") {
                (true, false) => {
                    let n = right.parse::<i64>().ok()?;
                    match op {
                        ">=" => Some((n, i64::MAX)),
                        ">" => n.checked_add(1).map(|value| (value, i64::MAX)),
                        "<=" => Some((i64::MIN, n)),
                        "<" => n.checked_sub(1).map(|value| (i64::MIN, value)),
                        "==" => Some((n, n)),
                        _ => None,
                    }
                }
                (false, true) => {
                    let n = left.parse::<i64>().ok()?;
                    match op {
                        "<=" => Some((n, i64::MAX)),
                        "<" => n.checked_add(1).map(|value| (value, i64::MAX)),
                        ">=" => Some((i64::MIN, n)),
                        ">" => n.checked_sub(1).map(|value| (i64::MIN, value)),
                        "==" => Some((n, n)),
                        _ => None,
                    }
                }
                _ => None,
            };
        }
    }
    None
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

        assert_eq!(super::SITE_VARIANTS.len(), 18);
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
        assert_eq!(super::RULE_ARG_DECLARATIONS.len(), 16);
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
                    "Value" | "String" | "Ident" | "Bool" | "Int" | "Duration | String" | "T.default"
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
}
