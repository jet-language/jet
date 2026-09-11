//! Shared pure evaluator for named computed fields.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::Comptime::{self, CtValue};
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{CtKey, EnumLitArg, Expr, Func};

use super::Eval::check_build_io;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComputedFieldProvenance {
    pub field: String,
    pub dependencies: Vec<String>,
    pub pure: bool,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ComputedFields {
    pub values: HashMap<String, CtValue>,
    pub provenance: Vec<ComputedFieldProvenance>,
}

pub(crate) fn dependencies_for_expression(
    expr: &Expr,
    field_names: &HashSet<String>,
) -> Vec<String> {
    let mut dependencies = Vec::new();
    Comptime::walk_identifiers(expr, &mut |name, _| {
        if field_names.contains(name) && !dependencies.iter().any(|seen| seen == name) {
            dependencies.push(name.to_string());
        }
    });
    dependencies.sort();
    dependencies
}

/// Evaluate one expression through the same purity and build-I/O gate used by
/// the named-field graph. Namespace-specific record shapes use this helper
/// for their nested values; they do not create a second evaluator.
pub(crate) fn evaluate_expression(
    expr: &Expr,
    globals: &HashMap<String, CtValue>,
    funcs: &HashMap<String, &Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
) -> Result<CtValue, Diagnostic> {
    check_build_io(expr)?;
    match expr.without_parens() {
        Expr::Ident(name, _) if globals.contains_key(name) => Ok(globals[name].clone()),
        Expr::ListLit(values, _) => values
            .iter()
            .map(|value| evaluate_expression(value, globals, funcs, extern_names, base_dir))
            .collect::<Result<Vec<_>, _>>()
            .map(CtValue::List),
        Expr::StructLit {
            type_name, fields, ..
        } => fields
            .iter()
            .map(|(name, _, value)| {
                evaluate_expression(value, globals, funcs, extern_names, base_dir)
                    .map(|value| (name.clone(), value))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|fields| CtValue::Struct {
                type_name: type_name.clone(),
                fields,
            }),
        Expr::EnumLit {
            type_name,
            variant,
            args,
            ..
        } => args
            .iter()
            .map(|arg| {
                let (name, value) = match arg {
                    EnumLitArg::Positional(value) => (None, value),
                    EnumLitArg::Named { label, expr } => (Some(label.clone()), expr),
                };
                evaluate_expression(value, globals, funcs, extern_names, base_dir)
                    .map(|value| (name, value))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|args| CtValue::Enum {
                type_name: type_name.clone(),
                variant: variant.clone(),
                args,
            }),
        Expr::MapLit(entries, _) => {
            let mut lowered = std::collections::BTreeMap::new();
            for (key, value) in entries {
                let key_value =
                    evaluate_expression(key, globals, funcs, extern_names, base_dir)?;
                let Some(key) = CtKey::from_value(key_value) else {
                    return Err(Diagnostic::error(
                        "E1333",
                        "module map keys must be deterministic scalar values".to_string(),
                        "named module facts become stable plan keys".to_string(),
                        "use a string, integer, boolean, or character key".to_string(),
                        Some(expr.span()),
                    ));
                };
                let value = evaluate_expression(value, globals, funcs, extern_names, base_dir)?;
                if lowered.insert(key, value).is_some() {
                    return Err(Diagnostic::error(
                        "E1333",
                        "module map contains a duplicate key".to_string(),
                        "one module fact cannot silently choose between two values for the same key"
                            .to_string(),
                        "remove the duplicate key or merge its values".to_string(),
                        Some(expr.span()),
                    ));
                }
            }
            Ok(CtValue::Map(lowered))
        }
        _ => Comptime::evaluate(expr, funcs, extern_names, base_dir, globals),
    }
}

pub(crate) fn evaluate_named_fields<'a>(
    fields: &HashMap<String, (Span, &'a Expr)>,
    globals: &HashMap<String, CtValue>,
    funcs: &HashMap<String, &'a Func>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    source: Option<&str>,
    cycle_why: &str,
    cycle_fix: &str,
) -> Result<ComputedFields, Diagnostic> {
    let mut states = HashMap::<String, u8>::new();
    let mut stack = Vec::new();
    let mut values = globals.clone();
    let mut names = fields.keys().cloned().collect::<Vec<_>>();
    names.sort();
    for name in &names {
        resolve_named_field(
            name,
            fields,
            &mut states,
            &mut values,
            &mut stack,
            extern_names,
            base_dir,
            funcs,
            cycle_why,
            cycle_fix,
        )?;
    }
    let provenance = names
        .into_iter()
        .map(|field| {
            let (field_span, expr) = fields
                .get(&field)
                .expect("computed field name came from map");
            ComputedFieldProvenance {
                field,
                dependencies: dependencies_for_expression(
                    expr,
                    &fields.keys().cloned().collect::<HashSet<_>>(),
                ),
                pure: true,
                source: source
                    .and_then(|text| text.get(field_span.start..field_span.end))
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
            }
        })
        .collect();
    Ok(ComputedFields { values, provenance })
}

fn resolve_named_field<'a>(
    name: &str,
    fields: &HashMap<String, (Span, &'a Expr)>,
    states: &mut HashMap<String, u8>,
    values: &mut HashMap<String, CtValue>,
    stack: &mut Vec<String>,
    extern_names: &HashSet<String>,
    base_dir: &Path,
    funcs: &HashMap<String, &'a Func>,
    cycle_why: &str,
    cycle_fix: &str,
) -> Result<(), Diagnostic> {
    if matches!(states.get(name), Some(2)) {
        return Ok(());
    }
    let Some((span, expr)) = fields.get(name).copied() else {
        return Ok(());
    };
    if matches!(states.get(name), Some(1)) {
        let start = stack.iter().position(|field| field == name).unwrap_or(0);
        let mut cycle = stack[start..].to_vec();
        cycle.push(name.to_string());
        return Err(Diagnostic::error(
            "E0338",
            format!(
                "computed module fields form a cycle: {}",
                cycle.join(" -> ")
            ),
            cycle_why.to_string(),
            cycle_fix.to_string(),
            Some(span),
        ));
    }
    states.insert(name.to_string(), 1);
    stack.push(name.to_string());
    let dependencies =
        dependencies_for_expression(expr, &fields.keys().cloned().collect::<HashSet<_>>());
    for dependency in dependencies {
        resolve_named_field(
            &dependency,
            fields,
            states,
            values,
            stack,
            extern_names,
            base_dir,
            funcs,
            cycle_why,
            cycle_fix,
        )?;
    }
    let value = evaluate_expression(expr, values, funcs, extern_names, base_dir)?;
    values.insert(name.to_string(), value);
    stack.pop();
    states.insert(name.to_string(), 2);
    Ok(())
}
