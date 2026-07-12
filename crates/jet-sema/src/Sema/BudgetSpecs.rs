use crate::AST::{ContribValue, EnumLitArg, Expr, Item, ModuleDecl, Program, ProgramBundle};
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
    pub comparison: String,
    pub limit: String,
    pub span: Span,
    pub field_spans: BTreeMap<String, Span>,
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
    validate_collisions(&specs, &mut diags);
    (specs, diags)
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
        let Expr::ListLit(entries, _) = &perf.budgets else { continue };
        for entry in entries {
            match elaborate(&contribution.path, entry) {
                Ok(spec) => specs.push(spec),
                Err(diag) => diags.push(diag),
            }
        }
    }
}

fn elaborate(role: &str, entry: &Expr) -> Result<BudgetSpec, Diagnostic> {
    let Expr::StructLit { type_name, fields, span, .. } = entry else {
        return Err(invalid("entry", "every budgets list item is a typed `Budget` value", "write `Budget.{ name: ..., metric: ..., limit: ... }`", entry.span()));
    };
    if type_name != "Budget" {
        return Err(invalid("entry", "every budgets list item has type `Budget`", "replace this value with `Budget.{ ... }`", *span));
    }
    const ALLOWED: &[&str] = &["name", "scope", "metric", "provider", "comparison", "limit", "enforcement", "applies"];
    let mut map: BTreeMap<&str, (&Expr, Span)> = BTreeMap::new();
    for (field, field_span, value) in fields {
        if !ALLOWED.contains(&field.as_str()) {
            return Err(invalid("entry", format!("`{field}` is not a Budget field"), format!("use one of: {}", ALLOWED.join(", ")), *field_span));
        }
        if map.insert(field, (value, *field_span)).is_some() {
            return Err(invalid("entry", format!("`{field}` is written more than once"), "keep one value for this field", *field_span));
        }
    }
    let (name_expr, _name_span) = required(&map, "name", *span)?;
    let name = string_value(name_expr).ok_or_else(|| invalid("entry", "`name` must be constant quoted text", "write `name: \"lowercase-kebab-name\"`", name_expr.span()))?;
    if name.is_empty() {
        return Err(invalid("entry", "`name` cannot be empty", "give this budget a stable name", name_expr.span()));
    }
    let (metric_expr, _) = required(&map, "metric", *span)?;
    let metric = enum_key(metric_expr).ok_or_else(|| invalid(&name, "`metric` must be one closed performance metric", "use a metric such as `.BinarySize` or `.Latency(.P99)`", metric_expr.span()))?;
    let metric_variant = enum_variant(metric_expr).expect("enum key requires enum literal");
    let (limit_expr, _) = required(&map, "limit", *span)?;
    let limit = enum_variant(limit_expr).ok_or_else(|| invalid(&name, "`limit` must be a typed comparison limit", "use `.AtMost(value)`, `.AtLeast(value)`, `.RegressionAtMost(percent)`, or `.ImprovementAtLeast(percent)`", limit_expr.span()))?;
    let deterministic = matches!(metric_variant.as_str(), "BinarySize" | "ArtifactSize" | "GeneratedUnsafe" | "PublicApiItems" | "DependencyCount" | "EffectCount");
    let comparison = map.get("comparison").and_then(|(e, _)| enum_variant(e)).unwrap_or_else(|| if deterministic { "Absolute".into() } else { "".into() });
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
    validate_limit_unit(&name, &metric_variant, &limit, limit_expr)?;
    let scope = match map.get("scope") {
        Some((expr, _)) => enum_key(expr).ok_or_else(|| invalid(&name, "`scope` must be one closed scope value", "use `.Package`, `.Target(name)`, `.Env(name)`, `.Service(name)`, `.Scene(name)`, or `.Bench(name)`", expr.span()))?,
        None => "Package".into(),
    };
    let provider = if let Some((expr, _)) = map.get("provider") {
        enum_key(expr).ok_or_else(|| invalid(&name, "`provider` must be one closed provider value", "use a typed provider such as `.CompilerFacts` or `.BuildArtifact(name)`", expr.span()))?
    } else if matches!(metric_variant.as_str(), "GeneratedUnsafe" | "PublicApiItems" | "DependencyCount" | "EffectCount") {
        "CompilerFacts".into()
    } else if matches!(metric_variant.as_str(), "BinarySize" | "ArtifactSize") {
        format!("BuildArtifact({scope})")
    } else {
        String::new()
    };
    let applicability = match map.get("applies") {
        Some((expr, _)) => parse_applicability(&name, expr)?,
        None => BudgetApplicability { targets: BudgetAxis::Current, profiles: BudgetAxis::Current },
    };
    let field_spans = map.into_iter().map(|(k, (_, s))| (k.to_string(), s)).collect();
    Ok(BudgetSpec { role: role.into(), name, metric, scope, provider, applicability, comparison, limit, span: *span, field_spans })
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
                let Some(key) = enum_key(selector) else { return Err(invalid(name, format!("applicability `{axis}` contains an invalid selector"), "use a typed target or profile selector", selector.span())); };
                selectors.insert(key);
            }
            Ok(BudgetAxis::Only(selectors))
        }
        _ => Err(invalid(name, format!("applicability `{axis}` must be `.Current`, `.All`, or `.Only([...])`"), "use one closed applicability variant", expr.span())),
    }
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

fn enum_arg(expr: &Expr) -> Option<&Expr> {
    let Expr::EnumLit { args, .. } = expr else { return None };
    match args.first()? { EnumLitArg::Positional(e) => Some(e), EnumLitArg::Named { expr, .. } => Some(expr) }
}

fn validate_limit_unit(name: &str, metric: &str, limit: &str, expr: &Expr) -> Result<(), Diagnostic> {
    let Some(value) = enum_arg(expr) else { return Err(invalid(name, format!("`.{limit}` requires one value"), "add the typed limit value", expr.span())) };
    if matches!(limit, "RegressionAtMost" | "ImprovementAtLeast") {
        return match value { Expr::UnitLit { suffix, .. } if suffix == "pct" => Ok(()), _ => Err(invalid(name, "relative limits use the `pct` unit", "write a percent such as `3pct`", value.span())) };
    }
    let allowed = if matches!(metric, "BinarySize" | "ArtifactSize" | "AllocationBytes" | "MemoryHighWater" | "SceneAssetBytes") { &["B", "KiB", "MiB", "GiB"][..] } else if matches!(metric, "StartupTime" | "FrameTime" | "Latency" | "BenchTime" | "ServiceReadiness") { &["ns", "us", "ms", "s"][..] } else { &[][..] };
    if allowed.is_empty() { return if matches!(value, Expr::Int(n, _, _) if *n >= 0) { Ok(()) } else { Err(invalid(name, "this metric uses a nonnegative Count value", "write a nonnegative integer", value.span())) }; }
    match value { Expr::UnitLit { suffix, .. } if allowed.contains(&suffix.as_str()) => Ok(()), _ => Err(invalid(name, format!("`{metric}` requires one of these units: {}", allowed.join(", ")), format!("write the limit with a {} suffix", allowed[0]), value.span())) }
}

fn string_value(expr: &Expr) -> Option<String> {
    let Expr::Str(parts, _) = expr else { return None };
    let [crate::AST::StrPart::Lit(value)] = parts.as_slice() else { return None };
    Some(value.clone())
}

fn snake_case(value: &str) -> bool {
    !value.is_empty() && value.bytes().enumerate().all(|(i, b)| b.is_ascii_lowercase() || b.is_ascii_digit() && i > 0 || b == b'_' && i > 0) && !value.ends_with('_') && !value.contains("__")
}
