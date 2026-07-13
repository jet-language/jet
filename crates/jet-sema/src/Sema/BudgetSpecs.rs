use crate::AST::{BudgetDecl, ContribValue, EnumLitArg, Expr, Item, ModuleDecl, Namespace, Program, ProgramBundle, SystemFieldValue};
use crate::Diagnostics::{Diagnostic, Span};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct BudgetSpec {
    pub role: String,
    pub name: String,
    pub metric: String,
    pub scope: String,
    pub provider: String,
    pub applicability: BudgetApplicability,
    pub enforcement: String,
    pub comparison: String,
    pub limit: String,
    pub comparison_fact: BudgetComparisonFact,
    pub limit_fact: BudgetLimitFact,
    pub span: Span,
    pub field_spans: BTreeMap<String, Span>,
}

#[derive(Debug, Clone)]
pub struct LocatedBudgetSpec {
    pub spec: BudgetSpec,
    pub module_index: usize,
}

impl std::ops::Deref for LocatedBudgetSpec {
    type Target = BudgetSpec;

    fn deref(&self) -> &Self::Target { &self.spec }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetComparisonFact {
    pub kind: String,
    pub baseline: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetLimitFact {
    pub kind: String,
    pub quantity: BudgetQuantity,
    pub raw: BudgetRawQuantity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetQuantity {
    DurationNs(u128),
    Bytes(u128),
    Count(u128),
    Rate { numerator: u128, denominator_ns: u128 },
    PercentBasisPoints(u128),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetRawQuantity {
    Scalar { digits: String, suffix: Option<String> },
    Rate { count_digits: String, per_digits: String, per_suffix: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetApplicability {
    pub targets: BudgetAxis,
    pub profiles: BudgetAxis,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BudgetAxis {
    Current,
    All,
    Only(std::collections::BTreeSet<String>),
}

/// Elaborate every performance role into the canonical typed budget form.
///
/// Callers that need the resolved budgets use this entry point; the normal
/// semantic pass calls the same elaborator and reports its diagnostics.
pub fn collect_budget_specs(program: &Program) -> Result<Vec<BudgetSpec>, Vec<Diagnostic>> {
    let (specs, diags) = validate_items(&program.items);
    if diags.is_empty() { Ok(specs) } else { Err(diags) }
}

/// Collect the canonical budget facts from a fully loaded package bundle.
///
/// Budget commands consume this after the ordinary front-end check, so command
/// execution cannot grow a second parser or semantic interpretation.
pub fn collect_budget_specs_bundle(bundle: &ProgramBundle) -> Result<Vec<BudgetSpec>, Vec<Diagnostic>> {
    collect_located_budget_specs_bundle(bundle)
        .map(|specs| specs.into_iter().map(|located| located.spec).collect())
}

/// Collect budgets while retaining the module that owns each source span.
pub fn collect_located_budget_specs_bundle(bundle: &ProgramBundle) -> Result<Vec<LocatedBudgetSpec>, Vec<Diagnostic>> {
    let mut specs = Vec::new();
    let mut diags = Vec::new();
    for (module_index, module) in bundle.modules.iter().enumerate() {
        let (mut module_specs, mut module_diags) = validate_items(&module.items);
        specs.extend(module_specs.drain(..).map(|spec| LocatedBudgetSpec { spec, module_index }));
        diags.append(&mut module_diags);
    }
    let plain_specs = specs.iter().map(|located| located.spec.clone()).collect::<Vec<_>>();
    validate_collisions(&plain_specs, &mut diags);
    if diags.is_empty() { Ok(specs) } else { Err(diags) }
}

pub fn validate_program(program: &Program) -> Vec<Diagnostic> {
    validate_items(&program.items).1
}

pub fn validate_bundle(bundle: &ProgramBundle) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for module in &bundle.modules {
        out.extend(validate_items(&module.items).1);
    }
    out
}

fn validate_items(items: &[Item]) -> (Vec<BudgetSpec>, Vec<Diagnostic>) {
    let mut specs = Vec::new();
    let mut diags = Vec::new();
    for item in items {
        if let Item::Module(module) = item {
            validate_module(module, &mut specs, &mut diags);
        }
    }
    validate_resolution(&specs, &attachment_catalog(items), &mut diags);
    validate_collisions(&specs, &mut diags);
    (specs, diags)
}

#[derive(Default)]
struct AttachmentCatalog {
    envs: std::collections::BTreeSet<String>,
    services: std::collections::BTreeSet<String>,
    targets: std::collections::BTreeSet<String>,
    scenes: std::collections::BTreeSet<String>,
    benches: std::collections::BTreeSet<String>,
}

fn attachment_catalog(items: &[Item]) -> AttachmentCatalog {
    let mut catalog = AttachmentCatalog::default();
    for item in items {
        let Item::Module(module) = item else { continue };
        for contribution in &module.contributions {
            match contribution.namespace {
                Namespace::Env => { catalog.envs.insert(contribution.path.clone()); }
                Namespace::System | Namespace::Image => { catalog.targets.insert(contribution.path.clone()); }
                _ => {}
            }
            match &contribution.value {
                ContribValue::Env(env) => catalog.services.extend(env.services.iter().map(|service| service.name.clone())),
                ContribValue::System(system) => {
                    for field in &system.fields {
                        if let SystemFieldValue::Services(services) = &field.value {
                            catalog.services.extend(services.iter().map(|service| service.name.clone()));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    catalog
}

fn validate_resolution(specs: &[BudgetSpec], catalog: &AttachmentCatalog, diags: &mut Vec<Diagnostic>) {
    for spec in specs {
        if let Some((kind, name)) = named_key(&spec.scope) {
            let found = match kind {
                "Env" => catalog.envs.contains(name),
                "Service" => catalog.services.contains(name),
                "Scene" => catalog.scenes.contains(name),
                "Bench" => catalog.benches.contains(name),
                "Target" => catalog.targets.contains(name),
                _ => true,
            };
            if !found {
                diags.push(unresolved(spec, "scope", format!("no canonical {kind} attachment named `{name}` exists in the containing package"), "declare that attachment or use its canonical package-qualified identity"));
                continue;
            }
        }
        if let Some((kind, name)) = named_key(&spec.provider) {
            let found = match kind {
                "BuildArtifact" => catalog.targets.contains(name) || name == "Package",
                "AllocationProbe" => catalog.services.contains(name) || catalog.scenes.contains(name) || catalog.benches.contains(name),
                "BenchMeasurement" => catalog.benches.contains(name),
                "ServiceProbe" => catalog.services.contains(name),
                "SceneProbe" => catalog.scenes.contains(name),
                _ => true,
            };
            if !found {
                diags.push(unresolved(spec, "provider", format!("no canonical {kind} provider named `{name}` exists in the containing package"), "name a provider attached to a resolved scope"));
            }
        }
    }
}

fn unresolved(spec: &BudgetSpec, attachment: &str, why: String, fix: &str) -> Diagnostic {
    Diagnostic::error(
        "E2905",
        format!("performance budget {} cannot resolve {}", spec.name, attachment),
        why,
        fix.to_string(),
        spec.field_spans.get(attachment).copied().or(Some(spec.span)),
    )
}

fn named_key(value: &str) -> Option<(&str, &str)> {
    let open = value.find('(')?;
    value.ends_with(')').then(|| (&value[..open], &value[open + 1..value.len() - 1]))
}

fn validate_collisions(specs: &[BudgetSpec], diags: &mut Vec<Diagnostic>) {
    for (index, right) in specs.iter().enumerate() {
        for left in &specs[..index] {
            if left.role == right.role && left.name == right.name {
                diags.push(overlap(left, right, "their containing package, perf role, and name form the same budget identity"));
                break;
            }
            if left.scope == right.scope
                && left.metric == right.metric
                && left.provider == right.provider
                && left.applicability.overlaps(&right.applicability)
            {
                diags.push(overlap(left, right, "their scope, metric, provider, and intersecting applicability form the same effective key"));
                break;
            }
        }
    }
}

fn overlap(left: &BudgetSpec, right: &BudgetSpec, why: &str) -> Diagnostic {
    Diagnostic::error(
        "E2904",
        format!("performance budgets {} and {} overlap", left.name, right.name),
        why.to_string(),
        "remove one budget or make their target/profile applicability disjoint".to_string(),
        Some(right.span),
    )
}

impl BudgetApplicability {
    fn overlaps(&self, other: &Self) -> bool {
        self.targets.overlaps(&other.targets) && self.profiles.overlaps(&other.profiles)
    }
}

impl BudgetAxis {
    fn overlaps(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Only(left), Self::Only(right)) => left.iter().any(|item| right.contains(item)),
            (Self::All, _) | (_, Self::All) | (Self::Current, _) | (_, Self::Current) => true,
        }
    }
}

fn invalid(name: &str, why: impl Into<String>, fix: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E2903",
        format!("performance budget {name} is not valid"),
        why.into(),
        fix.into(),
        Some(span),
    )
}

fn validate_module(module: &ModuleDecl, specs: &mut Vec<BudgetSpec>, diags: &mut Vec<Diagnostic>) {
    for contribution in &module.contributions {
        let ContribValue::Perf(perf) = &contribution.value else { continue };
        if !snake_case(&contribution.path) {
            diags.push(invalid(
                &format!("role `{}`", contribution.path),
                "performance role names are nonempty lowercase snake_case",
                "rename the role using lowercase words joined by `_`",
                contribution.path_span,
            ));
            continue;
        }
        for entry in &perf.budgets {
            match elaborate(&contribution.path, entry) {
                Ok(spec) => specs.push(spec),
                Err(diag) => diags.push(diag),
            }
        }
    }
}

fn elaborate(role: &str, entry: &BudgetDecl) -> Result<BudgetSpec, Diagnostic> {
    let span = entry.span;
    const ALLOWED: &[&str] = &["name", "scope", "metric", "provider", "comparison", "limit", "enforcement", "applies"];
    let mut map: BTreeMap<&str, (&Expr, Span)> = BTreeMap::new();
    for field in &entry.fields {
        if !ALLOWED.contains(&field.name.as_str()) {
            return Err(invalid("entry", format!("`{}` is not a Budget field", field.name), format!("use one of: {}", ALLOWED.join(", ")), field.name_span));
        }
        if map.insert(field.name.as_str(), (&field.value, field.name_span)).is_some() {
            return Err(invalid("entry", format!("`{}` is written more than once", field.name), "keep one value for this field", field.name_span));
        }
    }
    let (name_expr, _name_span) = required(&map, "name", span)?;
    let name = string_value(name_expr).ok_or_else(|| invalid("entry", "`name` must be constant quoted text", "write `name: \"lowercase-kebab-name\"`", name_expr.span()))?;
    if name.is_empty() {
        return Err(invalid("entry", "`name` cannot be empty", "give this budget a stable name", name_expr.span()));
    }
    let (metric_expr, _) = required(&map, "metric", span)?;
    let (metric, metric_variant) = closed_metric(&name, metric_expr)?;
    let (limit_expr, _) = required(&map, "limit", span)?;
    let limit = enum_variant(limit_expr).ok_or_else(|| invalid(&name, "`limit` must be a typed comparison limit", "use `.AtMost(value)`, `.AtLeast(value)`, `.RegressionAtMost(percent)`, or `.ImprovementAtLeast(percent)`", limit_expr.span()))?;
    let deterministic = matches!(metric_variant.as_str(), "BinarySize" | "ArtifactSize" | "GeneratedUnsafe" | "PublicApiItems" | "DependencyCount" | "EffectCount");
    let comparison_fact = match map.get("comparison") {
        Some((expr, _)) => closed_comparison(&name, expr)?,
        None if deterministic => BudgetComparisonFact { kind: "Absolute".into(), baseline: None },
        None => BudgetComparisonFact { kind: String::new(), baseline: None },
    };
    let comparison = comparison_fact.kind.clone();
    if comparison.is_empty() {
        return Err(invalid(&name, "statistical metrics require `.AbsoluteFrom(baseline)` or `.RelativeTo(baseline)`", "add a pinned statistical comparison", metric_expr.span()));
    }
    let higher = metric_variant == "Throughput";
    let direction_ok = if comparison == "RelativeTo" { if higher { limit == "ImprovementAtLeast" } else { limit == "RegressionAtMost" } } else if higher { limit == "AtLeast" } else { limit == "AtMost" };
    if !direction_ok {
        return Err(invalid(&name, format!("`{metric_variant}` and `{comparison}` cannot use `.{limit}`"), if higher { "use `.AtLeast(...)` or relative `.ImprovementAtLeast(...)`" } else { "use `.AtMost(...)` or relative `.RegressionAtMost(...)`" }, limit_expr.span()));
    }
    if deterministic && comparison != "Absolute" {
        return Err(invalid(&name, "deterministic metrics use `.Absolute`", "remove the baseline comparison", metric_expr.span()));
    }
    if !deterministic && comparison == "Absolute" {
        return Err(invalid(&name, "statistical metrics cannot use `.Absolute`", "use `.AbsoluteFrom(baseline)` or `.RelativeTo(baseline)`", metric_expr.span()));
    }
    let limit_fact = normalize_limit(&name, &metric_variant, &limit, limit_expr)?;
    let scope = match map.get("scope") {
        Some((expr, _)) => closed_scope(&name, expr)?,
        None => "Package".into(),
    };
    let provider = if let Some((expr, _)) = map.get("provider") {
        closed_provider(&name, expr)?
    } else if matches!(metric_variant.as_str(), "GeneratedUnsafe" | "PublicApiItems" | "DependencyCount" | "EffectCount") {
        "CompilerFacts".into()
    } else if matches!(metric_variant.as_str(), "BinarySize" | "ArtifactSize") {
        match named_key(&scope) {
            Some(("Target", target)) => format!("BuildArtifact({target})"),
            _ => "BuildArtifact(Package)".into(),
        }
    } else {
        String::new()
    };
    if provider.is_empty() {
        return Err(invalid(
            &name,
            format!("`{metric_variant}` has no unambiguous measurement provider"),
            "add the metric's explicit typed `provider`",
            metric_expr.span(),
        ));
    }
    let applicability = match map.get("applies") {
        Some((expr, _)) => parse_applicability(&name, expr)?,
        None => BudgetApplicability { targets: BudgetAxis::Current, profiles: BudgetAxis::Current },
    };
    let enforcement = match map.get("enforcement") {
        Some((expr, _)) => match enum_variant(expr).as_deref() {
            Some("Fail") => "Fail".into(),
            Some("Warn") if !deterministic => "Warn".into(),
            Some("Warn") => return Err(invalid(&name, "deterministic budgets must use `.Fail` enforcement", "remove `enforcement` or write `.Fail`", expr.span())),
            _ => return Err(invalid(&name, "`enforcement` must be `.Fail` or `.Warn`", "use `.Fail`, or `.Warn` for a statistical budget", expr.span())),
        },
        None => "Fail".into(),
    };
    validate_scope_provider_pair(&name, &metric_variant, &scope, &provider, span)?;
    let field_spans = map.into_iter().map(|(k, (_, s))| (k.to_string(), s)).collect();
    Ok(BudgetSpec { role: role.into(), name, metric, scope, provider, applicability, enforcement, comparison, limit, comparison_fact, limit_fact, span, field_spans })
}

fn closed_scope(name: &str, expr: &Expr) -> Result<String, Diagnostic> {
    let key = enum_key(expr).ok_or_else(|| invalid(name, "`scope` must be one closed scope value", "use `.Package`, `.Target(name)`, `.Env(name)`, `.Service(name)`, `.Scene(name)`, or `.Bench(name)`", expr.span()))?;
    match (enum_variant(expr).as_deref(), named_key(&key)) {
        (Some("Package"), None) => Ok(key),
        (Some("Env" | "Service" | "Scene" | "Bench" | "Target"), Some((_, value))) if !value.is_empty() => Ok(key),
        _ => Err(invalid(name, "`scope` must be one closed scope value with the required name", "use `.Package` or a named scope such as `.Service(\"api\")`", expr.span())),
    }
}

fn closed_provider(name: &str, expr: &Expr) -> Result<String, Diagnostic> {
    let key = enum_key(expr).ok_or_else(|| invalid(name, "`provider` must be one closed provider value", "use a typed provider such as `.CompilerFacts` or `.BuildArtifact(name)`", expr.span()))?;
    match (enum_variant(expr).as_deref(), named_key(&key)) {
        (Some("CompilerFacts"), None) => Ok(key),
        (Some("BuildArtifact" | "AllocationProbe" | "BenchMeasurement" | "ServiceProbe" | "SceneProbe"), Some((_, value))) if !value.is_empty() => Ok(key),
        _ => Err(invalid(name, "`provider` must be one closed provider value with the required name", "use `.CompilerFacts` or a named provider such as `.ServiceProbe(\"api\")`", expr.span())),
    }
}

fn closed_metric(name: &str, expr: &Expr) -> Result<(String, String), Diagnostic> {
    let Expr::EnumLit { variant, args, .. } = expr else {
        return Err(invalid(name, "`metric` must be one closed performance metric", "use a metric such as `.BinarySize` or `.Latency(.P99)`", expr.span()));
    };
    if !crate::Syntax::PERF_BUDGET_METRICS.contains(&variant.as_str()) {
        return Err(invalid(name, format!("`.{variant}` is not a performance metric"), "use one metric from the registered performance-budget vocabulary", expr.span()));
    }
    let percentile_metric = matches!(variant.as_str(), "FrameTime" | "Latency" | "BenchTime" | "DrawCalls");
    if percentile_metric {
        let [arg] = args.as_slice() else {
            return Err(invalid(name, format!("`.{variant}` requires exactly one percentile"), "use `.P50`, `.P90`, `.P95`, `.P99`, or `.P999`", expr.span()));
        };
        let value = match arg { EnumLitArg::Positional(value) => value, EnumLitArg::Named { expr, .. } => expr };
        let Some(percentile) = enum_variant(value) else {
            return Err(invalid(name, format!("`.{variant}` requires a typed percentile"), "use `.P50`, `.P90`, `.P95`, `.P99`, or `.P999`", value.span()));
        };
        if !crate::Syntax::PERF_BUDGET_PERCENTILES.contains(&percentile.as_str()) || !enum_args(value).is_empty() {
            return Err(invalid(name, format!("`.{percentile}` is not a supported percentile"), "use `.P50`, `.P90`, `.P95`, `.P99`, or `.P999`", value.span()));
        }
        return Ok((format!("{variant}({percentile})"), variant.clone()));
    }
    if !args.is_empty() {
        return Err(invalid(name, format!("`.{variant}` does not take a percentile or argument"), format!("write `.{variant}`"), expr.span()));
    }
    Ok((variant.clone(), variant.clone()))
}

fn closed_comparison(name: &str, expr: &Expr) -> Result<BudgetComparisonFact, Diagnostic> {
    let Expr::EnumLit { variant, args, .. } = expr else {
        return Err(invalid(name, "`comparison` must be `.Absolute`, `.AbsoluteFrom(name)`, or `.RelativeTo(name)`", "use one closed comparison variant", expr.span()));
    };
    match (variant.as_str(), args.as_slice()) {
        ("Absolute", []) => Ok(BudgetComparisonFact { kind: variant.clone(), baseline: None }),
        ("AbsoluteFrom" | "RelativeTo", [EnumLitArg::Positional(value)]) => {
            let Some(baseline) = string_value(value) else {
                return Err(invalid(name, format!("`.{variant}` requires one constant baseline name"), "write a lowercase slash-separated name such as `\"ci/linux-x64\"`", value.span()));
            };
            if !baseline_name(&baseline) {
                return Err(invalid(name, format!("`{baseline}` is not a valid baseline name"), "use nonempty slash-separated lowercase kebab-case segments", value.span()));
            }
            Ok(BudgetComparisonFact { kind: variant.clone(), baseline: Some(baseline) })
        }
        _ => Err(invalid(name, "`comparison` must be `.Absolute`, `.AbsoluteFrom(name)`, or `.RelativeTo(name)`", "use one closed comparison variant with its exact arguments", expr.span())),
    }
}

fn enum_args(expr: &Expr) -> &[EnumLitArg] {
    match expr { Expr::EnumLit { args, .. } => args, _ => &[] }
}

fn baseline_name(value: &str) -> bool {
    !value.is_empty() && value.split('/').all(|segment| {
        !segment.is_empty()
            && segment != "."
            && segment != ".."
            && !segment.starts_with('-')
            && !segment.ends_with('-')
            && !segment.contains("--")
            && segment.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    })
}

fn validate_scope_provider_pair(name: &str, metric: &str, scope: &str, provider: &str, span: Span) -> Result<(), Diagnostic> {
    let valid = match metric {
        "BinarySize" | "ArtifactSize" => (scope == "Package" || scope.starts_with("Target(")) && provider.starts_with("BuildArtifact("),
        "GeneratedUnsafe" | "PublicApiItems" | "DependencyCount" | "EffectCount" => (scope == "Package" || scope.starts_with("Target(")) && provider == "CompilerFacts",
        "ServiceReadiness" => match (named_key(scope), named_key(provider)) {
            (Some(("Service", scope_name)), Some(("ServiceProbe", provider_name))) => scope_name == provider_name,
            _ => false,
        },
        _ => true,
    };
    if valid { Ok(()) } else { Err(invalid(name, format!("`{metric}` cannot use scope `{scope}` with provider `{provider}`"), "choose the metric's ratified scope/provider pair", span)) }
}

fn parse_applicability(name: &str, expr: &Expr) -> Result<BudgetApplicability, Diagnostic> {
    let Expr::StructLit { type_name, fields, .. } = expr else {
        return Err(invalid(name, "`applies` must be a typed `BudgetApplies` value", "write `applies: BudgetApplies.{ targets: ..., profiles: ... }`", expr.span()));
    };
    if type_name != "BudgetApplies" {
        return Err(invalid(name, "`applies` has type `BudgetApplies`", "write `BudgetApplies.{ targets: ..., profiles: ... }`", expr.span()));
    }
    let mut targets = None;
    let mut profiles = None;
    for (field, _, value) in fields {
        let slot = match field.as_str() {
            "targets" => &mut targets,
            "profiles" => &mut profiles,
            _ => return Err(invalid(name, format!("`{field}` is not a BudgetApplies field"), "use only `targets` and `profiles`", value.span())),
        };
        if slot.replace(parse_axis(name, field, value)?).is_some() {
            return Err(invalid(name, format!("`{field}` is written more than once"), "keep one applicability value for this axis", value.span()));
        }
    }
    Ok(BudgetApplicability {
        targets: targets.unwrap_or(BudgetAxis::Current),
        profiles: profiles.unwrap_or(BudgetAxis::Current),
    })
}

fn parse_axis(name: &str, axis: &str, expr: &Expr) -> Result<BudgetAxis, Diagnostic> {
    let Expr::EnumLit { variant, args, .. } = expr else {
        return Err(invalid(name, format!("applicability `{axis}` must be `.Current`, `.All`, or `.Only([...])`"), "use one closed applicability variant", expr.span()));
    };
    match (variant.as_str(), args.as_slice()) {
        ("Current", []) => Ok(BudgetAxis::Current),
        ("All", []) => Ok(BudgetAxis::All),
        ("Only", [arg]) => {
            let value = match arg { EnumLitArg::Positional(value) => value, EnumLitArg::Named { expr, .. } => expr };
            let Expr::ListLit(values, _) = value else { return Err(invalid(name, format!("applicability `{axis}` Only requires a list"), "write `.Only([selector, ...])`", value.span())); };
            if values.is_empty() { return Err(invalid(name, format!("applicability `{axis}` Only list cannot be empty"), "add at least one selector", value.span())); }
            let mut selectors = std::collections::BTreeSet::new();
            for selector in values {
                let key = if axis == "targets" {
                    target_selector(name, selector)?
                } else {
                    profile_selector(name, selector)?
                };
                selectors.insert(key);
            }
            Ok(BudgetAxis::Only(selectors))
        }
        _ => Err(invalid(name, format!("applicability `{axis}` must be `.Current`, `.All`, or `.Only([...])`"), "use one closed applicability variant", expr.span())),
    }
}

fn target_selector(name: &str, expr: &Expr) -> Result<String, Diagnostic> {
    let Expr::EnumLit { variant, args, .. } = expr else {
        return Err(invalid(name, "target applicability contains a non-target selector", "use `.Class(.Native)` or `.Triple(\"canonical-triple\")`", expr.span()));
    };
    match (variant.as_str(), args.as_slice()) {
        ("Class", [arg]) => {
            let value = match arg { EnumLitArg::Positional(value) => value, EnumLitArg::Named { expr, .. } => expr };
            let Some(class) = enum_variant(value) else {
                return Err(invalid(name, "target class must be one typed class", "use `.Native`, `.Web`, `.Freestanding`, `.Plugin`, or `.OsImage`", value.span()));
            };
            if !crate::Syntax::PERF_BUDGET_TARGET_CLASSES.contains(&class.as_str()) || !enum_args(value).is_empty() {
                return Err(invalid(name, format!("`.{class}` is not a target class"), "use `.Native`, `.Web`, `.Freestanding`, `.Plugin`, or `.OsImage`", value.span()));
            }
            Ok(format!("Class({class})"))
        }
        ("Triple", [arg]) => {
            let value = match arg { EnumLitArg::Positional(value) => value, EnumLitArg::Named { expr, .. } => expr };
            let Some(triple) = string_value(value) else {
                return Err(invalid(name, "target triple selector requires constant quoted text", "write `.Triple(\"x86_64-unknown-linux-gnu\")`", value.span()));
            };
            if !canonical_triple(&triple) {
                return Err(invalid(name, format!("`{triple}` is not a canonical target triple"), "use the canonical lowercase target triple", value.span()));
            }
            Ok(format!("Triple({triple})"))
        }
        _ => Err(invalid(name, "target applicability contains a non-target selector", "use `.Class(.Native)` or `.Triple(\"canonical-triple\")`", expr.span())),
    }
}

fn profile_selector(name: &str, expr: &Expr) -> Result<String, Diagnostic> {
    let Expr::EnumLit { variant, args, .. } = expr else {
        return Err(invalid(name, "profile applicability contains a non-profile selector", "use `.Dev`, `.Release`, `.Small`, `.Test`, `.Bench`, or `.Named(\"profile\")`", expr.span()));
    };
    if matches!(variant.as_str(), "Dev" | "Release" | "Small" | "Test" | "Bench") && args.is_empty() {
        return Ok(variant.clone());
    }
    if variant == "Named" && args.len() == 1 {
        let value = match &args[0] { EnumLitArg::Positional(value) => value, EnumLitArg::Named { expr, .. } => expr };
        let Some(profile) = string_value(value) else {
            return Err(invalid(name, "named profile selector requires constant quoted text", "write `.Named(\"lowercase_snake_case\")`", value.span()));
        };
        let built_in = ["dev", "release", "small", "test", "bench"];
        if !snake_case(&profile) || built_in.iter().any(|item| profile.eq_ignore_ascii_case(item)) {
            return Err(invalid(name, format!("`{profile}` is not a legal named profile"), "use a declared lowercase_snake_case profile distinct from built-ins", value.span()));
        }
        return Ok(format!("Named({profile})"));
    }
    Err(invalid(name, "profile applicability contains a non-profile selector", "use `.Dev`, `.Release`, `.Small`, `.Test`, `.Bench`, or `.Named(\"profile\")`", expr.span()))
}

fn canonical_triple(value: &str) -> bool {
    value.split('-').count() >= 3
        && value.split('-').all(|part| !part.is_empty())
        && value.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_' || byte == b'-')
}

fn required<'a>(map: &'a BTreeMap<&str, (&'a Expr, Span)>, field: &str, span: Span) -> Result<(&'a Expr, Span), Diagnostic> {
    map.get(field).copied().ok_or_else(|| invalid("entry", format!("every Budget requires `{field}`"), format!("add `{field}: ...`"), span))
}

fn enum_variant(expr: &Expr) -> Option<String> {
    match expr { Expr::EnumLit { variant, .. } => Some(variant.clone()), _ => None }
}

fn enum_key(expr: &Expr) -> Option<String> {
    let Expr::EnumLit { variant, args, .. } = expr else { return None };
    if args.is_empty() { return Some(variant.clone()); }
    let mut rendered = Vec::new();
    for arg in args {
        let value = match arg { EnumLitArg::Positional(value) => value, EnumLitArg::Named { expr, .. } => expr };
        rendered.push(match value {
            Expr::Str(_, _) => string_value(value)?,
            Expr::EnumLit { .. } => enum_key(value)?,
            _ => return None,
        });
    }
    Some(format!("{}({})", variant, rendered.join(",")))
}

fn normalize_limit(name: &str, metric: &str, limit: &str, expr: &Expr) -> Result<BudgetLimitFact, Diagnostic> {
    let Expr::EnumLit { args, .. } = expr else { unreachable!("limit variant checked by caller") };
    let [EnumLitArg::Positional(value)] = args.as_slice() else {
        return Err(invalid(name, format!("`.{limit}` requires exactly one positional value"), format!("write `.{limit}(value)`"), expr.span()));
    };
    if matches!(limit, "RegressionAtMost" | "ImprovementAtLeast") {
        let (quantity, raw) = normalize_percent(value).ok_or_else(|| invalid(name, "relative limits use the `pct` unit with at most two decimal places", "write a percent such as `3pct` or `0.25pct`", value.span()))?;
        return Ok(BudgetLimitFact { kind: limit.into(), quantity: BudgetQuantity::PercentBasisPoints(quantity), raw });
    }
    if metric == "Throughput" {
        let (quantity, raw) = normalize_rate(name, value)?;
        return Ok(BudgetLimitFact { kind: limit.into(), quantity, raw });
    }
    let allowed = if matches!(metric, "BinarySize" | "ArtifactSize" | "AllocationBytes" | "MemoryHighWater" | "SceneAssetBytes") { &["B", "KiB", "MiB", "GiB"][..] } else if matches!(metric, "StartupTime" | "FrameTime" | "Latency" | "BenchTime" | "ServiceReadiness") { &["ns", "us", "ms", "s"][..] } else { &[][..] };
    if allowed.is_empty() {
        let Expr::Int(n, _, _, _) = value else { return Err(invalid(name, "this metric uses a nonnegative Count value", "write a nonnegative integer", value.span())) };
        let count = u128::try_from(*n).map_err(|_| invalid(name, "this metric uses a nonnegative Count value", "write a nonnegative integer", value.span()))?;
        return Ok(BudgetLimitFact { kind: limit.into(), quantity: BudgetQuantity::Count(count), raw: BudgetRawQuantity::Scalar { digits: n.to_string(), suffix: None } });
    }
    let Expr::UnitLit { raw, suffix, .. } = value else { return Err(invalid(name, format!("`{metric}` requires one of these units: {}", allowed.join(", ")), format!("write the limit with a {} suffix", allowed[0]), value.span())) };
    if !allowed.contains(&suffix.as_str()) { return Err(invalid(name, format!("`{metric}` requires one of these units: {}", allowed.join(", ")), format!("write the limit with a {} suffix", allowed[0]), value.span())); }
    let multiplier = unit_multiplier(suffix).expect("allowed normalized unit");
    let base = raw.parse::<u128>().ok().and_then(|n| n.checked_mul(multiplier)).ok_or_else(|| invalid(name, "limit value is not a nonnegative integer in range", "write a nonnegative whole value that fits the normalized unit", value.span()))?;
    let quantity = if matches!(suffix.as_str(), "B" | "KiB" | "MiB" | "GiB") { BudgetQuantity::Bytes(base) } else { BudgetQuantity::DurationNs(base) };
    Ok(BudgetLimitFact { kind: limit.into(), quantity, raw: BudgetRawQuantity::Scalar { digits: raw.clone(), suffix: Some(suffix.clone()) } })
}

fn normalize_percent(value: &Expr) -> Option<(u128, BudgetRawQuantity)> {
    let Expr::UnitLit { raw, suffix, .. } = value else { return None };
    if suffix != "pct" { return None; }
    let mut parts = raw.split('.');
    let whole = parts.next()?;
    let fraction = parts.next().unwrap_or("");
    if parts.next().is_some() || fraction.len() > 2 || whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) || !fraction.bytes().all(|b| b.is_ascii_digit()) { return None; }
    let whole = whole.parse::<u128>().ok()?.checked_mul(100)?;
    let fraction = match fraction.len() { 0 => 0, 1 => fraction.parse::<u128>().ok()?.checked_mul(10)?, _ => fraction.parse::<u128>().ok()? };
    Some((whole.checked_add(fraction)?, BudgetRawQuantity::Scalar { digits: raw.clone(), suffix: Some(suffix.clone()) }))
}

fn normalize_rate(name: &str, value: &Expr) -> Result<(BudgetQuantity, BudgetRawQuantity), Diagnostic> {
    let Expr::StructLit { type_name, fields, .. } = value else { return Err(invalid(name, "Throughput limits use `Rate.{ count: Int, per: Duration }`", "write a rate such as `Rate.{ count: 100, per: 1s }`", value.span())) };
    if type_name != "Rate" { return Err(invalid(name, "Throughput limits use the `Rate` value family", "write `Rate.{ count: 100, per: 1s }`", value.span())); }
    let mut count = None;
    let mut per = None;
    for (field, _, expr) in fields {
        let slot = match field.as_str() { "count" => &mut count, "per" => &mut per, _ => return Err(invalid(name, format!("`{field}` is not a Rate field"), "use exactly `count` and `per`", expr.span())) };
        if slot.replace(expr).is_some() { return Err(invalid(name, format!("Rate `{field}` is written more than once"), "keep one value for each Rate field", expr.span())); }
    }
    let count_expr = count.ok_or_else(|| invalid(name, "Rate requires `count`", "add `count: <nonnegative integer>`", value.span()))?;
    let per_expr = per.ok_or_else(|| invalid(name, "Rate requires `per`", "add `per: <positive duration>`", value.span()))?;
    let Expr::Int(count, _, _, Some(count_raw)) = count_expr else { return Err(invalid(name, "Rate count must be a nonnegative integer", "write `count: 100`", count_expr.span())) };
    let count = u128::try_from(*count).map_err(|_| invalid(name, "Rate count must be a nonnegative integer", "write `count: 100`", count_expr.span()))?;
    let Expr::UnitLit { raw: per_raw, suffix, .. } = per_expr else { return Err(invalid(name, "Rate per must be a positive Duration", "write `per: 1s`", per_expr.span())) };
    let multiplier = unit_multiplier(suffix).filter(|_| matches!(suffix.as_str(), "ns" | "us" | "ms" | "s")).ok_or_else(|| invalid(name, "Rate per must use ns, us, ms, or s", "write `per: 1s`", per_expr.span()))?;
    let per_ns = per_raw.parse::<u128>().ok().and_then(|n| n.checked_mul(multiplier)).filter(|n| *n > 0).ok_or_else(|| invalid(name, "Rate per must normalize to a positive Duration", "write a positive whole duration", per_expr.span()))?;
    let divisor = gcd(count, per_ns);
    Ok((BudgetQuantity::Rate { numerator: count / divisor, denominator_ns: per_ns / divisor }, BudgetRawQuantity::Rate { count_digits: count_raw.clone(), per_digits: per_raw.clone(), per_suffix: suffix.clone() }))
}

fn unit_multiplier(suffix: &str) -> Option<u128> {
    Some(match suffix { "ns" | "B" => 1, "us" => 1_000, "ms" => 1_000_000, "s" => 1_000_000_000, "KiB" => 1_024, "MiB" => 1_048_576, "GiB" => 1_073_741_824, _ => return None })
}

fn gcd(mut left: u128, mut right: u128) -> u128 {
    while right != 0 { let remainder = left % right; left = right; right = remainder; }
    left.max(1)
}

fn string_value(expr: &Expr) -> Option<String> {
    let Expr::Str(parts, _) = expr else { return None };
    let [crate::AST::StrPart::Lit(value)] = parts.as_slice() else { return None };
    Some(value.clone())
}

fn snake_case(value: &str) -> bool {
    !value.is_empty() && value.bytes().enumerate().all(|(i, b)| b.is_ascii_lowercase() || b.is_ascii_digit() && i > 0 || b == b'_' && i > 0) && !value.ends_with('_') && !value.contains("__")
}
