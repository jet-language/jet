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
//! A retired `Debug` registry row is deliberately not flagged here: E0922
//! (`crates/jet-foundation/src/Traits.rs`) already owns that retired name
//! end to end, with its own text. Duplicating it here would double-report.

use crate::AST::{Item, Marker};
use crate::Diagnostics::{Diagnostic, Span};
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
    let mut types = Vec::with_capacity(bindings.len());
    let mut constants = Vec::with_capacity(bindings.len());
    let mut mismatch = false;
    for binding in &bindings {
        let argument = &mut marker.args[binding.source_index];
        let observation = if binding.ty == crate::Policy::RuleArgType::Ident
            || binding.ty == crate::Policy::RuleArgType::Any
                && marker_name != Syntax::ATTR_DEFAULT
            || binding.ty == crate::Policy::RuleArgType::DurationOrString
                && matches!(argument, crate::AST::Expr::UnitLit { .. })
        {
            RuleArgumentObservation {
                ty: None,
                constant: None,
            }
        } else if binding.ty == crate::Policy::RuleArgType::Bool
            && marker_name == Syntax::ATTR_META
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

fn static_rule_site(site: Option<crate::Policy::RuleSite>) -> bool {
    matches!(
        site,
        Some(
            crate::Policy::RuleSite::Package
                | crate::Policy::RuleSite::File
                | crate::Policy::RuleSite::Module
                | crate::Policy::RuleSite::Type
                | crate::Policy::RuleSite::Declaration
                | crate::Policy::RuleSite::Constant
                | crate::Policy::RuleSite::Field
                | crate::Policy::RuleSite::Variant
                | crate::Policy::RuleSite::Test
                | crate::Policy::RuleSite::Bench
        )
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
            }
            Item::Enum(item) => {
                apply_all(&mut item.type_markers, validated, invalid);
                apply_all(&mut item.serde_markers, validated, invalid);
                for variant in &mut item.variants {
                    apply_all(&mut variant.serde_markers, validated, invalid);
                }
            }
            Item::Distinct(item) => apply_all(&mut item.type_markers, validated, invalid),
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
        if matches!(rule.status, crate::Policy::RuleStatus::Retired { .. })
            || !static_rule_site(application.site)
        {
            continue;
        }
        if application
            .site
            .is_some_and(|site| !crate::Policy::rule_allows(&marker.name, site))
        {
            diags.push(Diagnostic::error(
                "E0355",
                format!("`#{}` cannot attach at this site", marker.name),
                "the applied-rule registry gives every rule exact attachment sites".to_string(),
                "remove the marker or move it to one of its registered sites".to_string(),
                Some(marker.span),
            ));
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
                false,
                0,
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
    for item in &module.items {
        let Item::Struct(item) = item else { continue };
        for field in &item.fields {
            for marker in &field.serde_markers {
                if marker.name == Syntax::ATTR_DEFAULT
                    && !marker.args.is_empty()
                    && marker.ct.is_none()
                    && validated.contains_key(&marker.name_span.start)
                {
                    diags.push(crate::Sema::e2414(&field.name, marker.span));
                }
            }
        }
    }
    for (name, marker_span, text) in &static_strings {
        match name.as_str() {
            Syntax::ATTR_HTML => module.html_path = Some(text.clone()),
            Syntax::ATTR_INVARIANT => {
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
            application.target.is_none()
                && application.marker.span.start <= target.start.saturating_add(1)
                && target.start <= application.marker.span.start
                && !matches!(
                    application.marker.name.as_str(),
                    Syntax::ATTR_META | Syntax::CTX_BLOCK
                )
        })?;
        Some(self.rule_facts.remove(index).marker)
    }
}

/// E0927: `name` isn't a registered applied rule. `vocab` supplies nearest
/// spelling suggestions.
fn e0927_unknown_marker(name: &str, vocab: &[String], span: Span) -> Diagnostic {
    if let Some(crate::Policy::AppliedRule {
        status: crate::Policy::RuleStatus::Retired { replacement },
        ..
    }) = crate::Policy::applied_rule(name)
    {
        let fix = if replacement.starts_with('#') || replacement.starts_with('.') {
            format!("write `{replacement}` instead")
        } else {
            replacement.to_string()
        };
        return Diagnostic::error(
            "E0927",
            format!("`#{name}` is retired"),
            format!(
                "the registry keeps this old spelling only to teach its replacement; \
                 it no longer applies a rule"
            ),
            fix,
            Some(span),
        );
    }
    let fix = match crate::Sema::Diagnostics::suggest_field(name, vocab) {
        Some(s) => format!("did you mean `#{s}`?"),
        None => format!(
            "check the spelling, or see docs/spec/syntax-decisions.md for the full applied-rule list."
        ),
    };
    Diagnostic::error(
        "E0927",
        format!("`#{name}` isn't a known applied rule"),
        format!("`{name}` isn't registered as an applied rule — Jet rules are a closed, \
                 registered vocabulary (I7), not any PascalCase word."),
        fix,
        Some(span),
    )
}

/// True when `name` is a built-in rule or visible user derive.
fn is_legal_rule_name(name: &str, known_derive_names: &HashSet<String>) -> bool {
    Syntax::is_applied_rule(name) || known_derive_names.contains(name)
}

/// Check one marker against its sigil's plane. Returns `None` when it's
/// legal, or already reported elsewhere:
/// - a name known on the OTHER plane already got E0062/E0063 from the
///   parser's shared marker reader — never double-report.
/// - `#Debug` is E0922's job (see module docs).
fn check_one(m: &Marker, known_derive_names: &HashSet<String>) -> Option<Diagnostic> {
    let e0922_owns_debug = crate::Policy::applied_rule(&m.name).is_some_and(|row| {
        row.name == "Debug"
            && matches!(row.status, crate::Policy::RuleStatus::Retired { .. })
    });
    if e0922_owns_debug || is_legal_rule_name(&m.name, known_derive_names) {
        return None;
    }
    let vocab: Vec<String> = crate::Policy::APPLIED_RULES
        .iter()
        .filter(|row| matches!(row.status, crate::Policy::RuleStatus::Active))
        .map(|row| row.name.to_string())
        .chain(known_derive_names.iter().cloned())
        .collect();
    Some(e0927_unknown_marker(&m.name, &vocab, m.name_span))
}

/// D-MARK-VOCAB1 (card #518): validate every marker name on `items` against
/// its plane's registered vocabulary (E0927). Covers type-level markers
/// (`s.type_markers`/`e.type_markers`, the full pre-classification list —
/// `Syntax.rs` module docs — so plane info from `Marker.sigil` survives)
/// and field/variant-level bracket markers (`f.serde_markers`,
/// `v.serde_markers`, which keep their `Marker`s whole; only `#Redact` is
/// pulled out into `f.redact` upstream). `known_derive_names` is the set of
/// `derive T.Name { … }` providers visible to this build (bundle-wide in
/// `Bundle.rs`, so a cross-module user derive is never a false unknown).
pub(crate) fn check_marker_vocabulary(items: &[Item], known_derive_names: &HashSet<String>) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for item in items {
        match item {
            Item::Struct(s) => {
                for m in &s.type_markers {
                    if let Some(d) = check_one(m, known_derive_names) {
                        out.push(d);
                    }
                }
                for f in &s.fields {
                    for m in &f.serde_markers {
                        if let Some(d) = check_one(m, known_derive_names) {
                            out.push(d);
                        }
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.type_markers {
                    if let Some(d) = check_one(m, known_derive_names) {
                        out.push(d);
                    }
                }
                for v in &e.variants {
                    for m in &v.serde_markers {
                        if let Some(d) = check_one(m, known_derive_names) {
                            out.push(d);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}
