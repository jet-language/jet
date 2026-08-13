//! E0927 (card #518): the closed marker vocabulary.
//!
//! `#Name` rules are structurally accepted by the parser for any PascalCase
//! identifier (including the `#[…]` bracket-list path) — the parser
//! only knows "this looks like a marker," not "this is a marker Jet knows
//! about." An unregistered name used to silently do nothing (I3: codegen
//! never saw it, so nothing rejected it either). This module is the one
//! place that closes the vocabulary: every marker name is checked against
//! the registered applied-rule vocabulary plus any `derive T.Name { … }` provider
//! visible in this build (D-METADERIVE1=A user derives are a legal, dynamic
//! addition to the contract vocabulary, not typos).
//!
//! D-AUTODERIVE-SYNTAX1=D restored `Debug` as an active signed type-site
//! auto-derive control. It follows the same closed-vocabulary checks as
//! `Printable` and `Equatable`.

use crate::AST::{Item, Marker};
use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use std::collections::{HashMap, HashSet, VecDeque};

pub(crate) struct ValidatedRuleArguments {
    pub(crate) bindings: Vec<crate::Policy::RuleArgumentBinding>,
    pub(crate) types: Vec<Option<crate::AST::Type>>,
    pub(crate) constants: Vec<Option<crate::Comptime::CtValue>>,
}

impl ValidatedRuleArguments {
    pub(crate) fn type_for_source(&self, source_index: usize) -> Option<crate::AST::Type> {
        self.bindings
            .iter()
            .position(|binding| binding.source_index == source_index)
            .and_then(|index| self.types.get(index))
            .cloned()
            .flatten()
    }

    pub(crate) fn constant_for_source(
        &self,
        source_index: usize,
    ) -> Option<&crate::Comptime::CtValue> {
        self.bindings
            .iter()
            .position(|binding| binding.source_index == source_index)
            .and_then(|index| self.constants.get(index))
            .and_then(Option::as_ref)
    }
}

fn rule_signature_bindings(
    marker: &Marker,
) -> Result<Vec<crate::Policy::RuleArgumentBinding>, Diagnostic> {
    let Some(rule) = crate::Policy::applied_rule(&marker.name) else {
        return Ok(Vec::new());
    };
    if marker.name == Syntax::KW_UNSAFE && marker.args.is_empty() {
        return Ok(Vec::new());
    }
    rule.signature
        .marker_argument_bindings(marker)
        .ok_or_else(|| crate::Policy::marker_argument_shape_error(&marker.name, marker.span))
}

struct RuleArgumentObservation {
    ty: Option<crate::AST::Type>,
    constant: Option<crate::Comptime::CtValue>,
}

fn validate_rule_arguments(
    marker: &mut Marker,
    mut observe: impl FnMut(
        crate::Policy::RuleArgType,
        &mut crate::AST::Expr,
    ) -> RuleArgumentObservation,
) -> Result<ValidatedRuleArguments, Diagnostic> {
    let bindings = rule_signature_bindings(marker)?;
    let marker_name = marker.name.clone();
    let marker_span = marker.span;
    let rule = crate::Policy::applied_rule(&marker_name);
    let mut types = Vec::with_capacity(bindings.len());
    let mut constants = Vec::with_capacity(bindings.len());
    let mut mismatch = false;
    for binding in &bindings {
        let argument = &mut marker.args[binding.source_index];
        let source_type = binding
            .parameter_index
            .and_then(|index| rule.and_then(|rule| rule.signature.params.get(index)))
            .map(|parameter| parameter.source_type)
            .or_else(|| rule.and_then(|rule| rule.signature.variadic_source_type));
        if let Some(declaration) =
            source_type.and_then(crate::Policy::rule_arg_declaration)
        {
            let path = match argument {
                crate::AST::Expr::Ident(name, _) => Some(name.clone()),
                crate::AST::Expr::Field(base, member, _) => {
                    fn path(expression: &crate::AST::Expr) -> Option<String> {
                        match expression {
                            crate::AST::Expr::Ident(name, _) => Some(name.clone()),
                            crate::AST::Expr::Field(base, member, _) => {
                                Some(format!("{}.{}", path(base)?, member))
                            }
                            _ => None,
                        }
                    }
                    path(base).map(|base| format!("{base}.{member}"))
                }
                crate::AST::Expr::EnumLit { variant, .. } => Some(variant.clone()),
                _ => None,
            };
            let candidate = path.as_deref().map(|path| {
                let segments: Vec<&str> = path.split('.').collect();
                let variant_segments = segments
                    .iter()
                    .position(|segment| *segment == declaration.name)
                    .and_then(|index| segments.get(index + 1..))
                    .filter(|segments| !segments.is_empty())
                    .unwrap_or(&segments);
                match declaration.variant_segment {
                    crate::Policy::VariantSegment::First => {
                        variant_segments.first().copied().unwrap_or(path)
                    }
                    crate::Policy::VariantSegment::Last => {
                        variant_segments.last().copied().unwrap_or(path)
                    }
                }
            });
            // A rule that teaches its own menu downstream (E3220 / E2409) says
            // so in its registry row; a generic signature error here would
            // preempt the product diagnostic.
            let owns_its_menu = crate::Policy::applied_rule(&marker_name)
                .is_some_and(|row| row.owns_menu);
            if !owns_its_menu && !declaration.variants.is_empty() {
                if let Some(written) =
                    candidate.filter(|candidate| !declaration.variants.contains(candidate))
                {
                    return Err(crate::Policy::marker_argument_unknown_variant(
                        &marker_name,
                        *declaration,
                        written,
                        marker_span,
                    ));
                }
            }
        }
        let observation = if binding.ty == crate::Policy::RuleArgType::Ident
            || binding.ty == crate::Policy::RuleArgType::Any
                && marker_name != Syntax::MARKER_DEFAULT
            || binding.ty == crate::Policy::RuleArgType::DurationOrString
                && matches!(argument, crate::AST::Expr::UnitLit { .. })
        {
            RuleArgumentObservation {
                ty: None,
                constant: None,
            }
        } else if binding.ty == crate::Policy::RuleArgType::Bool
            && marker_name == Syntax::MARKER_META
            && matches!(argument, crate::AST::Expr::Ident(name, _)
                if name == Syntax::META_FIELD_TUNABLE)
        {
            RuleArgumentObservation {
                ty: Some(crate::AST::Type::Bool),
                constant: None,
            }
        } else {
            observe(binding.ty, argument)
        };
        let matches = match binding.ty {
            crate::Policy::RuleArgType::Any => true,
            crate::Policy::RuleArgType::String => {
                matches!(observation.ty, Some(crate::AST::Type::String))
            }
            crate::Policy::RuleArgType::Ident => matches!(
                argument,
                crate::AST::Expr::Ident(..)
                    | crate::AST::Expr::Field(..)
                    | crate::AST::Expr::EnumLit { .. }
            ),
            crate::Policy::RuleArgType::Bool => {
                matches!(observation.ty, Some(crate::AST::Type::Bool))
            }
            crate::Policy::RuleArgType::Int => {
                matches!(observation.ty, Some(crate::AST::Type::Int))
            }
            crate::Policy::RuleArgType::DurationOrString => {
                matches!(argument, crate::AST::Expr::UnitLit { .. })
                    || matches!(observation.ty, Some(crate::AST::Type::String))
                    || matches!(observation.ty, Some(crate::AST::Type::Named(ref name))
                        if name == crate::Syntax::DURATION_TYPE)
            }
        };
        mismatch |= !matches;
        types.push(observation.ty);
        constants.push(observation.constant);
    }
    if mismatch {
        return Err(crate::Policy::marker_argument_shape_error(
            &marker_name,
            marker_span,
        ));
    }
    Ok(ValidatedRuleArguments {
        bindings,
        types,
        constants,
    })
}

/// Static products need an actual compile-time value (for example `#HTML` or
/// `#Test`). Other sites still run through the same comptime evaluator below,
/// but their arguments may name locals or parameters, so signature diagnostics
/// remain in the ordinary sema pass where those bindings exist.
fn static_product_site(site: crate::Policy::RuleSite) -> bool {
    matches!(
        site,
        crate::Policy::RuleSite::Package
            | crate::Policy::RuleSite::File
            | crate::Policy::RuleSite::Module
            | crate::Policy::RuleSite::Type
            | crate::Policy::RuleSite::Constant
            | crate::Policy::RuleSite::Field
            | crate::Policy::RuleSite::Variant
            | crate::Policy::RuleSite::Test
            | crate::Policy::RuleSite::Bench
    )
}

fn materialize_static_marker_values(
    items: &mut [Item],
    validated: &HashMap<usize, ValidatedRuleArguments>,
    invalid: &HashSet<usize>,
) {
    fn apply(marker: &mut Marker, validated: &HashMap<usize, ValidatedRuleArguments>) {
        marker.ct = validated
            .get(&marker.name_span.start)
            .and_then(|arguments| arguments.constant_for_source(0))
            .cloned();
    }
    fn apply_all(
        markers: &mut Vec<Marker>,
        validated: &HashMap<usize, ValidatedRuleArguments>,
        invalid: &HashSet<usize>,
    ) {
        markers.retain(|marker| !invalid.contains(&marker.name_span.start));
        for marker in markers {
            apply(marker, validated);
        }
    }
    for item in items {
        match item {
            Item::Struct(item) => {
                apply_all(&mut item.type_markers, validated, invalid);
                apply_all(&mut item.serde_markers, validated, invalid);
                for field in &mut item.fields {
                    apply_all(&mut field.serde_markers, validated, invalid);
                }
                for function in &mut item.methods {
                    apply_all(&mut function.markers, validated, invalid);
                }
                for implementation in &mut item.trait_impls {
                    for function in &mut implementation.methods {
                        apply_all(&mut function.markers, validated, invalid);
                    }
                }
            }
            Item::Enum(item) => {
                apply_all(&mut item.type_markers, validated, invalid);
                apply_all(&mut item.serde_markers, validated, invalid);
                for variant in &mut item.variants {
                    apply_all(&mut variant.serde_markers, validated, invalid);
                }
                for function in &mut item.methods {
                    apply_all(&mut function.markers, validated, invalid);
                }
                for implementation in &mut item.trait_impls {
                    for function in &mut implementation.methods {
                        apply_all(&mut function.markers, validated, invalid);
                    }
                }
            }
            Item::Distinct(item) => apply_all(&mut item.type_markers, validated, invalid),
            Item::Func(item) => apply_all(&mut item.markers, validated, invalid),
            Item::Impl(item) => {
                for function in &mut item.methods {
                    apply_all(&mut function.markers, validated, invalid);
                }
            }
            _ => {}
        }
    }
}

pub(crate) fn resolve_static_rule_products(
    module: &mut crate::AST::LoadedModule,
    base_dir: &std::path::Path,
    core_imports: &std::collections::HashMap<String, String>,
    diags: &mut Vec<Diagnostic>,
) {
    let (funcs_owned, externs, globals) =
        crate::Sema::Registration::comptime_context_from_items(&module.items);
    let funcs = funcs_owned
        .iter()
        .map(|(name, function)| (name.clone(), function))
        .collect::<std::collections::HashMap<_, _>>();
    let mut facts = module
        .rule_facts
        .iter()
        .filter(|application| {
            !matches!(
                application.marker.name.as_str(),
                Syntax::KW_TEST | Syntax::KW_BENCH
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    for item in &module.items {
        let (name, expression, span) = match item {
            Item::Test(test) => {
                let Some(expression) = &test.name_expr else {
                    continue;
                };
                (Syntax::KW_TEST, expression.clone(), test.span)
            }
            Item::Bench(bench) => (Syntax::KW_BENCH, bench.name_expr.clone(), bench.span),
            _ => continue,
        };
        facts.push(crate::AST::AppliedRuleApplication {
            marker: Marker {
                name: name.to_string(),
                negated: false,
                name_span: span,
                args: vec![expression],
                arg_labels: vec![None],
                span,
                ct: None,
            },
            target: Some(span),
            site: Some(if name == Syntax::KW_TEST {
                crate::Policy::RuleSite::Test
            } else {
                crate::Policy::RuleSite::Bench
            }),
        });
    }
    let mut validated = HashMap::new();
    let mut invalid = HashSet::new();
    let mut static_strings = Vec::new();
    for application in &facts {
        let marker = &application.marker;
        let Some(rule) = crate::Policy::applied_rule(&marker.name) else {
            continue;
        };
        if matches!(rule.status, crate::Policy::RuleStatus::Retired { .. }) {
            continue;
        }
        let Some(site) = application.site else {
            continue;
        };
        if application
            .site
            .is_some_and(|_| !crate::Policy::rule_allows(&marker.name, site))
        {
            diags.push(crate::Policy::marker_wrong_site_error(
                &marker.name,
                site,
                marker.span,
            ));
            continue;
        }
        if !static_product_site(site) {
            // D-VERDICT-1455-1: site binding is no longer optional. Run the
            // comptime evaluator for dynamic sites too, but defer any type
            // error until ordinary sema can see locals and parameters.
            for expression in &marker.args {
                let mut expression = expression.clone();
                let _ = crate::Comptime::evaluate_with_imports_opts_collecting(
                    &mut expression,
                    &funcs,
                    &externs,
                    base_dir,
                    &globals,
                    core_imports,
                    crate::Policy::GateSet::default(),
                    0,
                    None,
                );
            }
            continue;
        }
        let mut evaluated_marker = marker.clone();
        let arguments = match validate_rule_arguments(&mut evaluated_marker, |_, expression| {
            let value = crate::Comptime::evaluate_with_imports_opts_collecting(
                expression,
                &funcs,
                &externs,
                base_dir,
                &globals,
                core_imports,
                crate::Policy::GateSet::default(),
                0,
                None,
            );
            let Ok((value, _)) = value else {
                return RuleArgumentObservation {
                    ty: None,
                    constant: None,
                };
            };
            let ty = match &value {
                crate::Comptime::CtValue::Str(_) => Some(crate::AST::Type::String),
                crate::Comptime::CtValue::Bool(_) => Some(crate::AST::Type::Bool),
                crate::Comptime::CtValue::Int(_) => Some(crate::AST::Type::Int),
                _ => None,
            };
            RuleArgumentObservation {
                ty,
                constant: Some(value),
            }
        }) {
            Ok(arguments) => arguments,
            Err(diagnostic) => {
                diags.push(diagnostic);
                invalid.insert(marker.name_span.start);
                continue;
            }
        };
        if let Some(crate::Comptime::CtValue::Str(text)) =
            arguments.constant_for_source(0)
        {
            static_strings.push((marker.name.clone(), marker.span, text.clone()));
        }
        validated.insert(marker.name_span.start, arguments);
    }
    materialize_static_marker_values(&mut module.items, &validated, &invalid);
    // D-FIELDDEF1=C: promote retired `#Default(expr)` into `field: T = expr`.
    for item in &mut module.items {
        let Item::Struct(item) = item else { continue };
        for field in &mut item.fields {
            if let Some(idx) = field
                .serde_markers
                .iter()
                .position(|m| m.name == Syntax::MARKER_DEFAULT)
            {
                let marker = field.serde_markers.remove(idx);
                if field.default.is_none() {
                    if let Some(arg) = marker.args.first() {
                        field.default = Some(Box::new(arg.clone()));
                    }
                }
                if field.default_ct.is_none() {
                    field.default_ct = marker.ct.clone();
                }
                diags.push(Diagnostic::error(
                    "E0375",
                    format!(
                        "`#{}` on field `{}` is retired — write an `=` default on the field",
                        Syntax::MARKER_DEFAULT,
                        field.name
                    ),
                    "field defaults use the same `=` spelling as parameter defaults (D-FIELDDEF1)"
                        .to_string(),
                    format!(
                        "write `{}: … = …` instead of `#{}(…)`",
                        field.name,
                        Syntax::MARKER_DEFAULT
                    ),
                    Some(marker.span),
                ));
            }
        }
    }
    // Evaluate `field: T = expr` defaults to compile-time values (D-SERDE5).
    for item in &mut module.items {
        let Item::Struct(item) = item else { continue };
        let needs_baked_default = item.derives.iter().any(|(t, _)| {
            matches!(
                t.as_str(),
                "Codable" | "Decode" | "Encode" | Syntax::MARKER_CLI
            )
        });
        for field in &mut item.fields {
            let Some(expr) = field.default.clone() else {
                continue;
            };
            if field.default_ct.is_some() {
                continue;
            }
            let mut expr = (*expr).clone();
            match crate::Comptime::evaluate_with_imports_opts_collecting(
                &mut expr,
                &funcs,
                &externs,
                base_dir,
                &globals,
                core_imports,
                crate::Policy::GateSet::default(),
                0,
                None,
            ) {
                Ok((value, _)) => field.default_ct = Some(value),
                Err(_) if needs_baked_default => {
                    diags.push(crate::Sema::e2414(&field.name, expr.span()));
                }
                Err(_) => {}
            }
        }
    }
    for (name, marker_span, text) in &static_strings {
        match name.as_str() {
            Syntax::MARKER_HTML => module.html_path = Some(text.clone()),
            Syntax::MARKER_INVARIANT => {
                if let Some(crate::AST::Item::Distinct(distinct)) = module
                    .items
                    .iter_mut()
                    .find(|item| matches!(item, crate::AST::Item::Distinct(distinct)
                        if distinct.span.start <= marker_span.start && marker_span.end <= distinct.span.end))
                {
                    match crate::Policy::parse_invariant_bounds(text) {
                        Some((lo, hi)) if lo <= hi => {
                            distinct.range = Some((lo, hi, *marker_span));
                            distinct.invariant = Some((text.clone(), *marker_span));
                        }
                        Some((lo, hi)) => diags.push(Diagnostic::error(
                            "E0137",
                            format!("this invariant range is empty — {lo} is after {hi}"),
                            "a refinement's low bound must not be greater than its high bound"
                                .to_string(),
                            "fix the `#Invariant` bounds".to_string(),
                            Some(*marker_span),
                        )),
                        None => diags.push(Diagnostic::error(
                            "E0003",
                            "`#Invariant` only supports linear integer bounds over `value`"
                                .to_string(),
                            "the first D-REFINE1 prover accepts comparisons joined with `&&`"
                                .to_string(),
                            "write `value >= lo && value < hi`, `lo <= value && value <= hi`, or `value == n`"
                                .to_string(),
                            Some(*marker_span),
                        )),
                    }
                }
            }
            _ => {}
        }
    }
    let mut test_names: VecDeque<_> = static_strings
        .iter()
        .filter(|(name, _, _)| name == Syntax::KW_TEST)
        .map(|(_, _, text)| text.clone())
        .collect();
    let mut bench_names: VecDeque<_> = static_strings
        .iter()
        .filter(|(name, _, _)| name == Syntax::KW_BENCH)
        .map(|(_, _, text)| text.clone())
        .collect();
    for item in &mut module.items {
        let (name, prefix, text) = match item {
            Item::Test(test) => {
                if test.name_expr.is_none() {
                    continue;
                }
                (
                    &mut test.name,
                    test.name_prefix.as_deref(),
                    test_names.pop_front(),
                )
            }
            Item::Bench(bench) => (
                &mut bench.name,
                bench.name_prefix.as_deref(),
                bench_names.pop_front(),
            ),
            _ => continue,
        };
        let Some(text) = text else {
            continue;
        };
        *name = Some(match prefix {
            Some(prefix) => format!(
                "{}_{}",
                prefix.trim_end_matches('_'),
                text.trim_start_matches('_')
            ),
            None => text,
        });
    }
}

impl<'a> crate::Sema::Checker<'a> {
    pub(crate) fn validate_rule_signature(
        &mut self,
        marker: &mut Marker,
    ) -> Option<ValidatedRuleArguments> {
        crate::Policy::applied_rule(&marker.name)?;
        match validate_rule_arguments(marker, |ty, argument| {
            let inferred = self.infer(argument);
            let constant = if matches!(
                ty,
                crate::Policy::RuleArgType::String
                    | crate::Policy::RuleArgType::Bool
                    | crate::Policy::RuleArgType::Int
            ) {
                self.evaluate_constant(argument)
            } else {
                None
            };
            RuleArgumentObservation {
                ty: inferred,
                constant,
            }
        }) {
            Ok(arguments) => Some(arguments),
            Err(diagnostic) => {
                self.diags.push(diagnostic);
                None
            }
        }
    }

    pub(crate) fn take_rule_fact(
        &mut self,
        name: &str,
        target: crate::Diagnostics::Span,
    ) -> Option<Marker> {
        let index = self.rule_facts.iter().position(|application| {
            application.marker.name == name
                && (application.target == Some(target)
                    || application.marker.span == target
                    || target.start <= application.marker.name_span.start
                        && application.marker.name_span.end <= target.end)
        })?;
        Some(self.rule_facts.remove(index).marker)
    }

    pub(crate) fn take_targeted_rule_facts(
        &mut self,
        target: crate::Diagnostics::Span,
    ) -> Vec<Marker> {
        let mut out = Vec::new();
        let mut index = 0;
        while index < self.rule_facts.len() {
            if self.rule_facts[index].target == Some(target) {
                out.push(self.rule_facts.remove(index).marker);
            } else {
                index += 1;
            }
        }
        out
    }

    pub(crate) fn take_statement_rule_fact(
        &mut self,
        target: crate::Diagnostics::Span,
    ) -> Option<Marker> {
        let index = self.rule_facts.iter().position(|application| {
            matches!(
                application.site,
                Some(crate::Policy::RuleSite::Block | crate::Policy::RuleSite::Statement)
            )
                && (application.target.is_none()
                    || application.target == Some(target))
                && application.marker.span.start <= target.start.saturating_add(1)
                && target.start <= application.marker.span.start
                && !matches!(
                    application.marker.name.as_str(),
                    Syntax::MARKER_META | Syntax::CTX_BLOCK
                )
        })?;
        Some(self.rule_facts.remove(index).marker)
    }
}

/// E0927: `name` isn't a registered applied rule. `vocab` supplies nearest
/// spelling suggestions. The text itself lives in the registry so the parser's
/// function-site check and this type-site check cannot drift apart.
/// Every marker name written on `items`, wherever it sits: on the type
/// (`s.type_markers` / `e.type_markers`, the full pre-classification list, so
/// plane info from `Marker.sigil` survives) and on a field or a variant
/// (`f.serde_markers` / `v.serde_markers`, which keep their `Marker`s whole;
/// only `#Redact` is pulled out into `f.redact` upstream). One walk, so a new
/// marker position is checked the moment it is read.
fn markers_in(items: &[Item]) -> impl Iterator<Item = &Marker> {
    items.iter().flat_map(|item| {
        let (type_markers, member_markers): (&[Marker], Vec<&Marker>) = match item {
            Item::Struct(s) => {
                let mut members = s
                    .fields
                    .iter()
                    .flat_map(|field| field.serde_markers.iter())
                    .collect::<Vec<_>>();
                members.extend(s.methods.iter().flat_map(|function| function.markers.iter()));
                members.extend(s.trait_impls.iter().flat_map(|implementation| {
                    implementation
                        .methods
                        .iter()
                        .flat_map(|function| function.markers.iter())
                }));
                (&s.type_markers, members)
            }
            Item::Enum(e) => {
                let mut members = e
                    .variants
                    .iter()
                    .flat_map(|variant| variant.serde_markers.iter())
                    .collect::<Vec<_>>();
                members.extend(e.methods.iter().flat_map(|function| function.markers.iter()));
                members.extend(e.trait_impls.iter().flat_map(|implementation| {
                    implementation
                        .methods
                        .iter()
                        .flat_map(|function| function.markers.iter())
                }));
                (&e.type_markers, members)
            }
            Item::Distinct(d) => (&d.type_markers, Vec::new()),
            Item::Func(f) => (&f.markers, Vec::new()),
            Item::Impl(i) => (
                &[],
                i.methods.iter().flat_map(|f| f.markers.iter()).collect(),
            ),
            _ => (&[], Vec::new()),
        };
        type_markers.iter().chain(member_markers)
    })
}

/// D-MARK-VOCAB1 (card #518) + D-META-ONE1=A: validate every marker name on
/// `items` against the one vocabulary (E0927). `vocabulary` is the registry
/// read from `Prelude/Markers.jet` plus the `derive T.Name { … }` providers
/// visible to this build (bundle-wide in `Bundle.rs`, so a cross-module user
/// derive is never a false unknown).
pub(crate) fn check_marker_vocabulary(
    items: &[Item],
    rule_facts: &[crate::AST::AppliedRuleApplication],
    vocabulary: &crate::Policy::MarkerVocabulary,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut seen = HashSet::new();
    for marker in markers_in(items).chain(
        rule_facts
            .iter()
            .filter(|fact| fact.target.is_some())
            .map(|fact| &fact.marker),
    ) {
        if !seen.insert(marker.name_span.start) {
            continue;
        }
        // A retired row already received its teaching diagnostic in the
        // parser's shared marker reader. The sema vocabulary walk must not
        // echo it when the retained AST/fact is still present for recovery.
        if crate::Policy::applied_rule(&marker.name)
            .is_some_and(|row| matches!(row.status, crate::Policy::RuleStatus::Retired { .. }))
        {
            continue;
        }
        // A name known on the OTHER plane already got E0062/E0063 from the
        // parser's shared marker reader — never double-report.
        if !vocabulary.knows(&marker.name) {
            diagnostics.push(vocabulary.unknown(&marker.name, marker.name_span));
            continue;
        }
        if let Some(site) = rule_facts
            .iter()
            .find(|fact| fact.marker.name_span == marker.name_span)
            .and_then(|fact| fact.site)
        {
            if !crate::Policy::rule_allows(&marker.name, site) {
                diagnostics.push(crate::Policy::marker_wrong_site_error(
                    &marker.name,
                    site,
                    marker.span,
                ));
            }
        }
    }
    diagnostics
}

/// Validate the four declaration checks for source-authored marker rules. The
/// parser can only consult compiler rows while it reads one file; this bundle
/// pass sees declarations from every loaded module and applies the same site,
/// signature, and repeat rules to them.
pub(crate) fn check_declared_rule_facts(
    facts: &[crate::AST::AppliedRuleApplication],
    vocabulary: &crate::Policy::MarkerVocabulary,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let mut seen: HashMap<(Option<crate::Diagnostics::Span>, String), crate::Diagnostics::Span> =
        HashMap::new();
    for application in facts {
        let Some(declaration) = vocabulary.declaration(&application.marker.name) else {
            if crate::Policy::applied_rule(&application.marker.name).is_none()
                && !matches!(
                    application.site,
                    Some(
                        crate::Policy::RuleSite::Type
                            | crate::Policy::RuleSite::Field
                            | crate::Policy::RuleSite::Variant
                            | crate::Policy::RuleSite::Function
                            | crate::Policy::RuleSite::Method
                    )
                )
            {
                diagnostics.push(vocabulary.unknown(
                    &application.marker.name,
                    application.marker.name_span,
                ));
            }
            continue;
        };
        if !declared_rule_arguments_match(&application.marker, declaration) {
            diagnostics.push(declared_rule_argument_shape_error(
                &application.marker.name,
                declaration,
                application.marker.span,
            ));
        }
        let sites = declared_rule_sites(declaration);
        if application.site.is_some_and(|site| !sites.contains(&site)) {
            diagnostics.push(Diagnostic::error(
                "E0355",
                format!("`#{}` cannot attach at this site", application.marker.name),
                format!(
                    "the declared rule allows only these sites: {}",
                    sites.iter().map(|site| format!(".{:?}", site)).collect::<Vec<_>>().join(", ")
                ),
                "remove the marker or move it to one of its declared sites".to_string(),
                Some(application.marker.span),
            ));
        }
        if !declared_rule_repeatable(declaration) {
            let key = (application.target, application.marker.name.clone());
            if let Some(previous) = seen.insert(key, application.marker.span) {
                diagnostics.push(crate::Policy::marker_repeated_error(
                    &application.marker.name,
                    "target",
                    application.marker.span,
                ).with_detail(format!(
                    "first application span: {}..{}\nsecond application span: {}..{}",
                    previous.start,
                    previous.end,
                    application.marker.span.start,
                    application.marker.span.end,
                )));
            }
        }
    }
    diagnostics
}

fn declared_rule_arguments_match(
    marker: &Marker,
    declaration: &crate::AST::MarkerDecl,
) -> bool {
    let params: Vec<_> = declaration
        .params
        .iter()
        .filter(|param| !param.name.starts_with('$'))
        .collect();
    let mut supplied = vec![false; params.len()];
    let mut next_positional = 0usize;
    let mut saw_named = false;
    for (index, _argument) in marker.args.iter().enumerate() {
        let parameter = if let Some((label, _)) = marker.arg_labels.get(index).and_then(Option::as_ref) {
            saw_named = true;
            params.iter().position(|param| param.name == *label)
        } else if saw_named {
            return false;
        } else if next_positional < params.len() {
            let position = next_positional;
            next_positional += 1;
            Some(position)
        } else if params.iter().any(|param| param.variadic) {
            Some(params.len().saturating_sub(1))
        } else {
            None
        };
        let Some(parameter) = parameter else {
            return false;
        };
        if supplied[parameter] && !params[parameter].variadic {
            return false;
        }
        if declared_argument_type_mismatch(
            &marker.args[index],
            params[parameter],
        ) {
            return false;
        }
        supplied[parameter] = true;
    }
    params.iter().enumerate().all(|(index, parameter)| {
        supplied[index] || parameter.value.is_some() || parameter.variadic
    })
}

fn declared_rule_argument_shape_error(
    name: &str,
    declaration: &crate::AST::MarkerDecl,
    span: crate::Diagnostics::Span,
) -> Diagnostic {
    let expected = declaration
        .params
        .iter()
        .filter(|parameter| !parameter.name.starts_with('$'))
        .map(|parameter| {
            let ty = parameter
                .ty
                .as_ref()
                .map(crate::AST::Type::name)
                .unwrap_or_else(|| "Value".to_string());
            let variadic = parameter.variadic.then_some("...").unwrap_or("");
            let default = parameter
                .value
                .as_deref()
                .map(|_| " = …".to_string())
                .unwrap_or_default();
            format!("{}: {variadic}{ty}{default}", parameter.name)
        })
        .collect::<Vec<_>>()
        .join(", ");
    Diagnostic::error(
        "E0930",
        format!("`#{name}` arguments do not match `{name}({expected})`"),
        "marker arguments use the same call grammar and typed signature as function arguments"
            .to_string(),
        format!("match the declared signature `{name}({expected})`"),
        Some(span),
    )
}

/// Reject only a type mismatch that the source expression proves by itself.
/// Names and calls stay open for ordinary sema inference; this pass owns the
/// declaration contract, not a second expression type checker.
fn declared_argument_type_mismatch(
    expression: &crate::AST::Expr,
    parameter: &crate::AST::MarkerDeclParam,
) -> bool {
    let Some(expected) = parameter.ty.as_ref() else {
        return false;
    };
    let actual = match expression {
        crate::AST::Expr::Str(..) => Some("String"),
        crate::AST::Expr::Int(..) => Some("Int"),
        crate::AST::Expr::Float(..) => Some("Float"),
        crate::AST::Expr::Bool(..) => Some("Bool"),
        crate::AST::Expr::Char(..) => Some("Char"),
        crate::AST::Expr::ListLit(..) => Some("List"),
        crate::AST::Expr::MapLit(..) => Some("Map"),
        _ => None,
    };
    let expected = match expected {
        crate::AST::Type::String => Some("String"),
        crate::AST::Type::Int => Some("Int"),
        crate::AST::Type::Float => Some("Float"),
        crate::AST::Type::Bool => Some("Bool"),
        crate::AST::Type::Char => Some("Char"),
        crate::AST::Type::List(..) => Some("List"),
        crate::AST::Type::Map { .. } => Some("Map"),
        _ => None,
    };
    expected.zip(actual).is_some_and(|(expected, actual)| expected != actual)
}

fn declared_rule_sites(declaration: &crate::AST::MarkerDecl) -> Vec<crate::Policy::RuleSite> {
    declaration
        .params
        .iter()
        .find(|param| param.name == "$sites")
        .and_then(|param| param.value.as_deref())
        .and_then(|value| match value {
            crate::AST::Expr::ListLit(values, _) => Some(values.as_slice()),
            _ => None,
        })
        .map(|values| {
            values
                .iter()
                .filter_map(declared_site)
                .filter_map(|name| crate::Policy::RuleSite::ALL.iter().copied().find(|site| site.name() == name))
                .collect()
        })
        .unwrap_or_default()
}

fn declared_site(value: &crate::AST::Expr) -> Option<String> {
    match value {
        crate::AST::Expr::Ident(name, _) => Some(name.trim_start_matches('.').to_string()),
        crate::AST::Expr::Field(_, member, _) => Some(member.clone()),
        crate::AST::Expr::EnumLit { variant, .. } => Some(variant.clone()),
        _ => None,
    }
}

fn declared_rule_repeatable(declaration: &crate::AST::MarkerDecl) -> bool {
    declaration
        .params
        .iter()
        .find(|param| param.name == "$repeatable")
        .and_then(|param| param.value.as_deref())
        .is_some_and(|value| matches!(value, crate::AST::Expr::Bool(true, _)))
}
