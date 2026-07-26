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
use std::collections::HashSet;

fn literal_string(expression: &crate::AST::Expr) -> Option<String> {
    match expression {
        crate::AST::Expr::Str(parts, _) if parts.len() == 1 => match &parts[0] {
            crate::AST::StrPart::Lit(text) => Some(text.clone()),
            crate::AST::StrPart::Interp(..) => None,
        },
        _ => None,
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
    let facts = module.rule_facts.clone();
    for application in facts {
        let marker = application.marker;
        if !matches!(
            marker.name.as_str(),
            Syntax::ATTR_INVARIANT | Syntax::ATTR_HTML
        ) {
            continue;
        }
        let Some(rule) = crate::Policy::applied_rule(&marker.name) else {
            continue;
        };
        let Some(bindings) = rule.signature.marker_argument_bindings(&marker) else {
            diags.push(crate::Policy::marker_argument_shape_error(
                &marker.name,
                marker.span,
            ));
            continue;
        };
        let Some(binding) = bindings
            .iter()
            .find(|binding| binding.parameter_index == Some(0))
        else {
            continue;
        };
        let expression = &marker.args[binding.source_index];
        let text = match literal_string(expression) {
            Some(text) => text,
            None => {
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
                let Ok((crate::Comptime::CtValue::Str(text), _)) = value else {
                    diags.push(crate::Policy::marker_argument_shape_error(
                        &marker.name,
                        marker.span,
                    ));
                    continue;
                };
                text
            }
        };
        match marker.name.as_str() {
            Syntax::ATTR_HTML => module.html_path = Some(text),
            Syntax::ATTR_INVARIANT => {
                if let Some(crate::AST::Item::Distinct(distinct)) = module
                    .items
                    .iter_mut()
                    .find(|item| matches!(item, crate::AST::Item::Distinct(distinct)
                        if distinct.span.start <= marker.span.start && marker.span.end <= distinct.span.end))
                {
                    match crate::Policy::parse_invariant_bounds(&text) {
                        Some((lo, hi)) if lo <= hi => {
                            distinct.range = Some((lo, hi, marker.span));
                            distinct.invariant = Some((text, marker.span));
                        }
                        Some((lo, hi)) => diags.push(Diagnostic::error(
                            "E0137",
                            format!("this invariant range is empty — {lo} is after {hi}"),
                            "a refinement's low bound must not be greater than its high bound"
                                .to_string(),
                            "fix the `#Invariant` bounds".to_string(),
                            Some(expression.span()),
                        )),
                        None => diags.push(Diagnostic::error(
                            "E0003",
                            "`#Invariant` only supports linear integer bounds over `value`"
                                .to_string(),
                            "the first D-REFINE1 prover accepts comparisons joined with `&&`"
                                .to_string(),
                            "write `value >= lo && value < hi`, `lo <= value && value <= hi`, or `value == n`"
                                .to_string(),
                            Some(marker.span),
                        )),
                    }
                }
            }
            _ => {}
        }
    }
    for item in &mut module.items {
        let (name, expression, prefix, marker_name, marker_span) = match item {
            Item::Test(test) => {
                let Some(expression) = test.name_expr.as_ref() else {
                    continue;
                };
                (
                    &mut test.name,
                    expression,
                    test.name_prefix.as_deref(),
                    Syntax::KW_TEST,
                    test.name_span,
                )
            }
            Item::Bench(bench) => (
                &mut bench.name,
                &bench.name_expr,
                bench.name_prefix.as_deref(),
                Syntax::KW_BENCH,
                bench.name_span,
            ),
            _ => continue,
        };
        let text = match literal_string(expression) {
            Some(text) => text,
            None => {
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
                let Ok((crate::Comptime::CtValue::Str(text), _)) = value else {
                    diags.push(crate::Policy::marker_argument_shape_error(
                        marker_name,
                        marker_span,
                    ));
                    continue;
                };
                text
            }
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

impl<'a> crate::Sema::Checker<'a> {
    pub(crate) fn validate_rule_signature(
        &mut self,
        marker: &mut Marker,
    ) -> Option<ValidatedRuleArguments> {
        let rule = crate::Policy::applied_rule(&marker.name)?;
        let Some(bindings) = rule.signature.marker_argument_bindings(marker) else {
            self.diags.push(crate::Policy::marker_argument_shape_error(
                &marker.name,
                marker.span,
            ));
            return None;
        };
        let mut types = Vec::with_capacity(bindings.len());
        let mut constants = Vec::with_capacity(bindings.len());
        let mut mismatch = false;
        for binding in &bindings {
            let argument = &mut marker.args[binding.source_index];
            let ty = match binding.ty {
                crate::Policy::RuleArgType::Any => None,
                crate::Policy::RuleArgType::Ident => None,
                crate::Policy::RuleArgType::Bool
                    if marker.name == Syntax::ATTR_META
                        && matches!(argument, crate::AST::Expr::Ident(name, _) if name == Syntax::META_FIELD_TUNABLE) =>
                {
                    Some(crate::AST::Type::Bool)
                }
                crate::Policy::RuleArgType::DurationOrString
                    if matches!(argument, crate::AST::Expr::UnitLit { .. }) =>
                {
                    None
                }
                _ => self.infer(argument),
            };
            let matches = match binding.ty {
                crate::Policy::RuleArgType::Any => true,
                crate::Policy::RuleArgType::String => {
                    matches!(ty, Some(crate::AST::Type::String))
                }
                crate::Policy::RuleArgType::Ident => binding.ty.matches_expr(argument),
                crate::Policy::RuleArgType::Bool => {
                    matches!(ty, Some(crate::AST::Type::Bool))
                }
                crate::Policy::RuleArgType::Int => {
                    matches!(ty, Some(crate::AST::Type::Int))
                }
                crate::Policy::RuleArgType::DurationOrString => {
                    matches!(argument, crate::AST::Expr::UnitLit { .. })
                        || matches!(ty, Some(crate::AST::Type::String))
                        || matches!(ty, Some(crate::AST::Type::Named(ref name)) if name == crate::Syntax::DURATION_TYPE)
                }
            };
            mismatch |= !matches;
            types.push(ty);
            constants.push(match binding.ty {
                crate::Policy::RuleArgType::String
                | crate::Policy::RuleArgType::Bool
                | crate::Policy::RuleArgType::Int => self.evaluate_constant(argument),
                _ => None,
            });
        }
        if mismatch {
            self.diags.push(crate::Policy::marker_argument_shape_error(
                &marker.name,
                marker.span,
            ));
            return None;
        }
        Some(ValidatedRuleArguments {
            bindings,
            types,
            constants,
        })
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

/// D-MARKSIG1=A: one sema pass checks every source-order rule fact against
/// the registry signature. Parser owns grammar/labels/arity; sema owns
/// argument kinds. One bad marker produces one E0930.
pub(crate) fn check_rule_signatures(
    applications: &[crate::AST::AppliedRuleApplication],
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for application in applications {
        let marker = &application.marker;
        if matches!(
            marker.name.as_str(),
            Syntax::ATTR_META
                | Syntax::CTX_BLOCK
                | Syntax::KW_TEST
                | Syntax::KW_BENCH
                | Syntax::ATTR_INVARIANT
                | Syntax::ATTR_HTML
        ) {
            continue;
        }
        let Some(rule) = crate::Policy::applied_rule(&marker.name) else {
            continue;
        };
        if application.target.is_some()
            || rule.sites.iter().any(|site| {
                matches!(
                    site,
                    crate::Policy::RuleSite::Function
                        | crate::Policy::RuleSite::Block
                        | crate::Policy::RuleSite::Statement
                        | crate::Policy::RuleSite::Declaration
                        | crate::Policy::RuleSite::Expression
                        | crate::Policy::RuleSite::Operation
                )
            })
        {
            continue;
        }
        if matches!(rule.status, crate::Policy::RuleStatus::Retired { .. })
            || marker.name == Syntax::KW_UNSAFE && marker.args.is_empty()
        {
            continue;
        }
        let Some(bindings) = rule.signature.marker_argument_bindings(marker) else {
            out.push(crate::Policy::marker_argument_shape_error(
                &marker.name,
                marker.span,
            ));
            continue;
        };
        if bindings.iter().any(|binding| {
            !binding
                .ty
                .matches_expr(&marker.args[binding.source_index])
        }) {
            out.push(crate::Policy::marker_argument_shape_error(
                &marker.name,
                marker.span,
            ));
        }
    }
    out
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
