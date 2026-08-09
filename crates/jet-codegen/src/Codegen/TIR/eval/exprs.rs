//! Exhaustive TExprKind evaluation (#777).
use std::collections::HashMap;
use std::sync::Arc;
use crate::AST::{BinOp, CtFloat, Type, UnOp};
use crate::Codegen::TIR::{
    ListSpreadPart, TCallArg, TCoreClosureKind, TExpr, TExprKind, TFnValueKind, TModuleCallForm,
    TPlace, TStrPart,
};
use crate::Comptime::Builtins::{as_bool, as_int, eval_binop};
use crate::Comptime::{
    apply_core_call, apply_impure_core_call, apply_repl_authorized_core_call, CtReport, CtValue,
    DevSink,
};
use crate::Diagnostics::{Diagnostic, Span};
use super::builtins::eval_builtin;
use super::handles::eval_handle;
use super::local_cell::{internal_index, project_mut, project_pair_mut, project_ref};
use super::{
    materialize_view_mut_window, progress_elapsed, progress_emit, progress_iter_parts,
    progress_no_color, progress_now, progress_source_has_exact_total, reborrow_repl_authorizer,
    unsupported, EvalCallable, EvalCtx, Flow,
};

fn progress_parts(
    value: &CtValue,
) -> Option<(Vec<CtValue>, String, String, f64, Vec<usize>, usize, usize, bool)> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != "__JetProgressIter" {
        return None;
    }
    let items = fields.iter().find_map(|(name, value)| {
        (name == "items").then(|| match value {
            CtValue::List(items) => Some(items.clone()),
            _ => None,
        })
    })??;
    let description = fields.iter().find_map(|(name, value)| {
        (name == "description").then(|| match value {
            CtValue::Str(value) => Some(value.clone()),
            _ => None,
        })
    })??;
    let format = fields.iter().find_map(|(name, value)| {
        (name == "format").then(|| match value {
            CtValue::Str(value) => Some(value.clone()),
            _ => None,
        })
    })??;
    let started_at = fields
        .iter()
        .find_map(|(name, value)| {
            (name == "started_at").then(|| match value {
                CtValue::Float(value) => Some(value.as_f64()),
                CtValue::Int(value) => Some(*value as f64),
                _ => None,
            })
        })
        .flatten()
        .unwrap_or_else(progress_now);
    let pulls = fields
        .iter()
        .find(|(name, _)| name == "pulls")
        .and_then(|(_, value)| match value {
            CtValue::List(values) => values
                .iter()
                .map(|value| match value {
                    CtValue::Int(value) => Some((*value).max(0) as usize),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>(),
            _ => None,
        })
        .unwrap_or_else(|| vec![1; items.len()]);
    let tail = fields
        .iter()
        .find_map(|(name, value)| {
            (name == "tail").then(|| match value {
                CtValue::Int(value) => Some((*value).max(0) as usize),
                _ => None,
            })
        })
        .flatten()
        .unwrap_or(0);
    let total = fields
        .iter()
        .find_map(|(name, value)| {
            (name == "total").then(|| match value {
                CtValue::Int(value) => Some((*value).max(0) as usize),
                _ => None,
            })
        })
        .flatten()
        .unwrap_or(items.len());
    let known_total = fields
        .iter()
        .find_map(|(name, value)| {
            (name == "known_total").then(|| match value {
                CtValue::Bool(value) => Some(*value),
                _ => None,
            })
        })
        .flatten()
        .unwrap_or(true);
    Some((
        items,
        description,
        format,
        started_at,
        pulls,
        tail,
        total,
        known_total,
    ))
}

fn progress_value(
    items: Vec<CtValue>,
    description: String,
    format: String,
    started_at: f64,
    pulls: Vec<usize>,
    tail: usize,
    total: usize,
    known_total: bool,
) -> CtValue {
    CtValue::Struct {
        type_name: "__JetProgressIter".to_string(),
        fields: vec![
            ("items".to_string(), CtValue::List(items)),
            ("description".to_string(), CtValue::Str(description)),
            ("format".to_string(), CtValue::Str(format)),
            ("started_at".to_string(), CtValue::Float(CtFloat::f64(started_at))),
            (
                "pulls".to_string(),
                CtValue::List(pulls.into_iter().map(|n| CtValue::Int(n as i64)).collect()),
            ),
            ("tail".to_string(), CtValue::Int(tail as i64)),
            ("total".to_string(), CtValue::Int(total as i64)),
            ("known_total".to_string(), CtValue::Bool(known_total)),
        ],
    }
}

fn mark_unknown_progress_total(
    mut value: CtValue,
    module: &str,
    method: &str,
    args: &[TExpr],
    known_total: Option<bool>,
) -> CtValue {
    let is_iter = matches!(
        args.first().map(|arg| &arg.ty),
        Some(Type::Apply { name, .. }) if name == crate::Syntax::TYPE_ITER
    );
    let known_total = known_total.unwrap_or_else(|| {
        args.first()
            .is_some_and(progress_source_has_exact_total)
    });
    if module == "core.io" && method == "progress" && is_iter && !known_total {
        if let CtValue::Struct { type_name, fields } = &mut value {
            if type_name == "__JetProgressIter" {
                if let Some((_, known_total)) = fields
                    .iter_mut()
                    .find(|(name, _)| name == "known_total")
                {
                    *known_total = CtValue::Bool(false);
                } else {
                    fields.push(("known_total".to_string(), CtValue::Bool(false)));
                }
            }
        }
    }
    value
}

fn progress_lazy_builtin(op: &crate::Codegen::TIR::TBuiltinOp) -> bool {
    matches!(
        op,
        crate::Codegen::TIR::TBuiltinOp::Take
            | crate::Codegen::TIR::TBuiltinOp::Skip
            | crate::Codegen::TIR::TBuiltinOp::StepBy
            | crate::Codegen::TIR::TBuiltinOp::Dedup
            | crate::Codegen::TIR::TBuiltinOp::Chunks
            | crate::Codegen::TIR::TBuiltinOp::Windows
            | crate::Codegen::TIR::TBuiltinOp::Flatten
            | crate::Codegen::TIR::TBuiltinOp::Intersperse
            | crate::Codegen::TIR::TBuiltinOp::Indexed { .. }
            | crate::Codegen::TIR::TBuiltinOp::Indexes
            | crate::Codegen::TIR::TBuiltinOp::Zip { .. }
    )
}

fn progress_terminal_builtin(op: &crate::Codegen::TIR::TBuiltinOp) -> bool {
    matches!(
        op,
        crate::Codegen::TIR::TBuiltinOp::IterToList
            | crate::Codegen::TIR::TBuiltinOp::IterCollect
            | crate::Codegen::TIR::TBuiltinOp::TryCollect
            | crate::Codegen::TIR::TBuiltinOp::JoinSep
            | crate::Codegen::TIR::TBuiltinOp::Sum { .. }
            | crate::Codegen::TIR::TBuiltinOp::Product { .. }
            | crate::Codegen::TIR::TBuiltinOp::Min { .. }
            | crate::Codegen::TIR::TBuiltinOp::Max { .. }
    )
}

fn emit_progress_pulls(
    sink: Option<&Arc<std::sync::Mutex<DevSink>>>,
    description: &str,
    format: &str,
    started_at: f64,
    total: usize,
    known_total: bool,
    raw_pulls: usize,
) {
    for index in 0..raw_pulls {
        let text = progress_semantics::jet_progress_render(
            description,
            format,
            index + 1,
            known_total.then_some(total),
            progress_elapsed(started_at),
            progress_no_color(),
        );
        progress_emit(sink, &text);
    }
}

fn progress_builtin_plan(
    op: &crate::Codegen::TIR::TBuiltinOp,
    source_items: &[CtValue],
    output_items: &[CtValue],
    source_pulls: &[usize],
    old_tail: usize,
    arg: Option<&CtValue>,
) -> (Vec<usize>, usize) {
    let n = arg
        .and_then(|value| match value {
            CtValue::Int(value) => Some((*value).max(0) as usize),
            _ => None,
        })
        .unwrap_or(0);
    let source_len = source_items.len();
    let output_len = output_items.len();
    let pull_at = |index: usize| source_pulls.get(index).copied().unwrap_or(1);
    let sum_from = |index: usize| source_pulls.iter().copied().skip(index).sum::<usize>();
    match op {
        crate::Codegen::TIR::TBuiltinOp::Take => {
            let len = output_len.min(n).min(source_pulls.len());
            let tail = if n != 0 && n >= source_len { old_tail } else { 0 };
            (source_pulls.iter().copied().take(len).collect(), tail)
        }
        crate::Codegen::TIR::TBuiltinOp::Skip => {
            if n >= source_pulls.len() {
                (Vec::new(), source_pulls.iter().sum::<usize>() + old_tail)
            } else {
                let mut pulls = source_pulls[n..].to_vec();
                if let Some(first) = pulls.first_mut() {
                    *first = source_pulls[..=n].iter().sum();
                }
                (pulls, old_tail)
            }
        }
        crate::Codegen::TIR::TBuiltinOp::StepBy => {
            let n = n.max(1);
            let mut pulls = Vec::new();
            let mut index = 0;
            if !source_pulls.is_empty() {
                pulls.push(source_pulls[0]);
                index = 1;
                while index < source_pulls.len() {
                    let end = (index + n).min(source_pulls.len());
                    if end - index < n {
                        break;
                    }
                    pulls.push(source_pulls[index..end].iter().sum());
                    index = end;
                }
            }
            (pulls, source_pulls[index..].iter().sum::<usize>() + old_tail)
        }
        crate::Codegen::TIR::TBuiltinOp::Dedup => {
            let mut pulls = Vec::new();
            let mut pending = 0usize;
            let mut previous: Option<&CtValue> = None;
            for (index, item) in source_items.iter().enumerate() {
                let pull = pull_at(index);
                if previous.is_some_and(|previous| previous == item) {
                    pending = pending.saturating_add(pull);
                } else {
                    pulls.push(pending.saturating_add(pull));
                    pending = 0;
                    previous = Some(item);
                }
            }
            (pulls, pending.saturating_add(old_tail))
        }
        crate::Codegen::TIR::TBuiltinOp::Chunks => {
            let mut pulls = Vec::with_capacity(output_len);
            let mut source_index = 0usize;
            for output in output_items {
                let CtValue::List(chunk) = output else {
                    return (vec![1; output_len], old_tail);
                };
                let end = (source_index + chunk.len()).min(source_len);
                pulls.push(source_pulls[source_index..end].iter().sum());
                source_index = end;
            }
            (pulls, sum_from(source_index).saturating_add(old_tail))
        }
        crate::Codegen::TIR::TBuiltinOp::Windows => {
            let size = n.max(1);
            if source_len < size {
                (Vec::new(), sum_from(0).saturating_add(old_tail))
            } else {
                let mut pulls = Vec::with_capacity(output_len);
                if output_len > 0 {
                    pulls.push(source_pulls[..size].iter().sum());
                    for index in 1..output_len {
                        pulls.push(pull_at(size + index - 1));
                    }
                }
                let consumed = size.saturating_add(output_len.saturating_sub(1));
                (pulls, sum_from(consumed).saturating_add(old_tail))
            }
        }
        crate::Codegen::TIR::TBuiltinOp::Flatten => {
            let mut pulls = Vec::new();
            let mut pending = 0usize;
            for (index, item) in source_items.iter().enumerate() {
                let pull = pull_at(index);
                let CtValue::List(inner) = item else {
                    return (vec![1; output_len], old_tail);
                };
                if inner.is_empty() {
                    pending = pending.saturating_add(pull);
                } else {
                    pulls.push(pending.saturating_add(pull));
                    pulls.extend(std::iter::repeat_n(0, inner.len().saturating_sub(1)));
                    pending = 0;
                }
            }
            (pulls, pending.saturating_add(old_tail))
        }
        crate::Codegen::TIR::TBuiltinOp::Intersperse => {
            let mut pulls = Vec::with_capacity(output_len);
            for (index, pull) in source_pulls.iter().copied().take(source_len).enumerate() {
                pulls.push(pull);
                if index + 1 < source_len {
                    pulls.push(0);
                }
            }
            (pulls, old_tail)
        }
        crate::Codegen::TIR::TBuiltinOp::Zip { .. } => {
            let consumed = output_len.min(source_len);
            let tail = if consumed < source_len {
                // The AOT/JIT zip adapters ask the receiver for the next
                // item before discovering that the other side is exhausted.
                pull_at(consumed)
            } else {
                old_tail
            };
            (source_pulls.iter().copied().take(consumed).collect(), tail)
        }
        _ if output_len == source_len => (source_pulls.to_vec(), old_tail),
        _ => (vec![1; output_len], old_tail),
    }
}

fn try_collect_pulls(items: &[CtValue], pulls: &[usize], tail: usize) -> usize {
    let mut consumed = 0usize;
    for (index, item) in items.iter().enumerate() {
        consumed = consumed.saturating_add(pulls.get(index).copied().unwrap_or(1));
        if matches!(item, CtValue::Failed(CtReport::Told(_))) {
            return consumed;
        }
    }
    consumed.saturating_add(tail)
}

mod progress_semantics {
    include!("../../../Prelude/Core/Progress.rs");
}

/// jet-jit shares this for tier-identical address identity.
pub fn tir_place_address_key(expr: &TExpr) -> String {
    match &expr.kind {
        TExprKind::Local(local) => local.name.clone(),
        TExprKind::Field { recv, field, .. } => {
            format!("{}.{}", tir_place_address_key(recv), field)
        }
        TExprKind::Index { base, index, .. } => {
            format!("{}[{}]", tir_place_address_key(base), tir_place_address_key(index))
        }
        TExprKind::Borrow { place, .. } | TExprKind::Deref(place) => tir_place_address_key(place),
        _ => format!("ty:{}", expr.ty.show()),
    }
}

/// jet-jit shares this for tier-identical address identity.
pub fn stable_place_address(key: &str) -> i64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let addr = (hash as i64).wrapping_abs();
    if addr == 0 {
        1
    } else {
        addr
    }
}

/// TIR-eval representation of the erased `SQL = (String, Vec<String>)`
/// runtime value. The AOT emitter uses a Rust tuple; keeping named fields in
/// the evaluator makes the same representation inspectable without adding a
/// second semantic type or a host-only shortcut.
fn typed_sql_value(template: String, params: Vec<CtValue>) -> CtValue {
    CtValue::Struct {
        type_name: "SQL".to_string(),
        fields: vec![
            ("template".to_string(), CtValue::Str(template)),
            ("params".to_string(), CtValue::List(params)),
        ],
    }
}

fn typed_sql_parts(value: &CtValue, span: crate::Diagnostics::Span) -> Result<(String, Vec<CtValue>), crate::Diagnostics::Diagnostic> {
    let CtValue::Struct { type_name, fields } = value else {
        return Err(unsupported("SQL value", span));
    };
    if type_name != "SQL" {
        return Err(unsupported("SQL value", span));
    }
    let template = fields.iter().find_map(|(name, value)| {
        (name == "template").then(|| match value {
            CtValue::Str(value) => Some(value.clone()),
            _ => None,
        })
    }).flatten();
    let params = fields.iter().find_map(|(name, value)| {
        (name == "params").then(|| match value {
            CtValue::List(value) => Some(value.clone()),
            _ => None,
        })
    }).flatten();
    match (template, params) {
        (Some(template), Some(params)) => Ok((template, params)),
        _ => Err(unsupported("malformed SQL value", span)),
    }
}

fn local_cell_handle(type_name: &str, index: usize) -> CtValue {
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields: vec![("index".to_string(), CtValue::Int(index as i64))],
    }
}

fn local_cell_index(value: &CtValue, expected: &str) -> Option<usize> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    (type_name == expected)
        .then(|| internal_index(fields))
        .flatten()
}

fn range_parts(value: &CtValue) -> Option<(i64, i64, bool)> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != crate::Syntax::TYPE_RANGE {
        return None;
    }
    let field = |wanted: &str| {
        fields
            .iter()
            .find(|(name, _)| name == wanted)
            .map(|(_, value)| value)
    };
    let (Some(CtValue::Int(start)), Some(CtValue::Int(end))) =
        (field("start"), field("end"))
    else {
        return None;
    };
    Some((
        *start,
        *end,
        matches!(field("exclusive"), Some(CtValue::Bool(true))),
    ))
}

fn shared_guard_parts(value: &CtValue) -> Option<(usize, usize, bool, Vec<String>)> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != "__JetTirSharedGuard" {
        return None;
    }
    let shared = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("shared", CtValue::Int(index)) => Some(*index as usize),
        _ => None,
    })?;
    let lease = fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("lease", CtValue::Int(index)) => Some(*index as usize),
        _ => None,
    })?;
    let editable = fields.iter().any(
        |(name, value)| matches!((name.as_str(), value), ("editable", CtValue::Bool(true))),
    );
    let path = fields
        .iter()
        .find_map(|(name, value)| match (name.as_str(), value) {
            ("path", CtValue::List(path)) => Some(
                path.iter()
                    .filter_map(|part| match part {
                        CtValue::Str(part) => Some(part.clone()),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default();
    Some((shared, lease, editable, path))
}

fn append_shared_guard_path(value: &mut CtValue, suffix: &[String]) -> bool {
    let CtValue::Struct { type_name, fields } = value else {
        return false;
    };
    if type_name != "__JetTirSharedGuard" {
        return false;
    }
    let path = fields.iter_mut().find_map(|(name, value)| {
        (name == "path").then_some(value)
    });
    if let Some(CtValue::List(path)) = path {
        path.extend(suffix.iter().cloned().map(CtValue::Str));
    } else {
        fields.push((
            "path".to_string(),
            CtValue::List(suffix.iter().cloned().map(CtValue::Str).collect()),
        ));
    }
    true
}

fn condition_index(value: &CtValue) -> Option<usize> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    (type_name == "__JetTirCondition").then(|| {
        fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
            ("index", CtValue::Int(index)) => Some(*index as usize),
            _ => None,
        })
    })?
}

fn shared_projection<'a>(value: &'a CtValue, path: &[String]) -> Option<&'a CtValue> {
    let Some((field, rest)) = path.split_first() else {
        return Some(value);
    };
    let CtValue::Struct { fields, .. } = value else {
        return None;
    };
    let mangled = crate::Codegen::mangle(field);
    let value = fields.iter().find_map(|(name, value)| {
        (name == field
            || name == &mangled
            || name.strip_prefix("user_") == Some(field.as_str()))
        .then_some(value)
    })?;
    shared_projection(value, rest)
}

fn replace_shared_projection(value: &mut CtValue, path: &[String], replacement: CtValue) -> bool {
    let Some((field, rest)) = path.split_first() else {
        *value = replacement;
        return true;
    };
    let CtValue::Struct { fields, .. } = value else {
        return false;
    };
    let mangled = crate::Codegen::mangle(field);
    let Some(value) = fields.iter_mut().find_map(|(name, value)| {
        (name == field
            || name == &mangled
            || name.strip_prefix("user_") == Some(field.as_str()))
        .then_some(value)
    }) else {
        return false;
    };
    replace_shared_projection(value, rest, replacement)
}

fn datatree(variant: &str, payload: Option<CtValue>) -> CtValue {
    CtValue::Enum {
        type_name: "JSON".to_string(),
        variant: variant.to_string(),
        args: payload.into_iter().map(|value| (None, value)).collect(),
    }
}

fn datatree_variant(value: &CtValue) -> Option<(&str, Option<&CtValue>)> {
    match value {
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if type_name == "JSON" || type_name == "DataTree" => {
            Some((variant.as_str(), args.first().map(|(_, value)| value)))
        }
        CtValue::Bytes(_) => Some(("Bytes", Some(value))),
        _ => None,
    }
}

fn decode_error(path: impl Into<String>, reason: impl Into<String>) -> CtValue {
    CtValue::List(vec![CtValue::Struct {
        type_name: "FieldError".to_string(),
        fields: vec![
            ("path".to_string(), CtValue::Str(path.into())),
            ("reason".to_string(), CtValue::Str(reason.into())),
        ],
    }])
}

/// D-MIGRATE3=A: `MigrationStatus` for a record that arrived in the current
/// shape — `jet_std::MigrationStatus::fresh()`.
fn migration_status_fresh() -> CtValue {
    migration_status_value(false, String::new(), Vec::new())
}

/// D-MIGRATE4: `MigrationStatus` for a record that entered the chain at
/// historical shape `start` and was walked forward through `total` steps. The
/// names come from the same vocabulary codegen bakes into the chain-walker.
fn migration_status(start: usize, total: usize) -> CtValue {
    migration_status_value(
        true,
        crate::Codegen::TIR::migration_shape_name(start),
        (start..total)
            .map(|step| CtValue::Str(crate::Codegen::TIR::migration_step_name(step)))
            .collect(),
    )
}

fn migration_status_value(migrated: bool, from: String, steps: Vec<CtValue>) -> CtValue {
    CtValue::Struct {
        type_name: "MigrationStatus".to_string(),
        fields: vec![
            ("migrated".to_string(), CtValue::Bool(migrated)),
            ("from".to_string(), CtValue::Str(from)),
            ("steps".to_string(), CtValue::List(steps)),
        ],
    }
}

/// A CSV cell as text. The dynamic parser hands back `Str` cells; anything
/// else renders through the shared display so no row silently reads empty.
fn string_cell(value: &CtValue) -> String {
    match value {
        CtValue::Str(text) => text.clone(),
        other => other.jet_show(),
    }
}

fn migration_did_run(status: &CtValue) -> bool {
    matches!(status, CtValue::Struct { fields, .. }
        if fields.iter().any(|(name, value)|
            name == "migrated" && matches!(value, CtValue::Bool(true))))
}

/// The codec name the Prelude puts in `invalid <CODEC> (line n): …`.
fn codec_label(module: &str) -> &'static str {
    match module {
        "core.encoding.toml" => "TOML",
        "core.encoding.yaml" => "YAML",
        _ => "JSON",
    }
}

fn decode_error_under(segment: &str, error: CtValue) -> CtValue {
    let CtValue::List(entries) = error else {
        return decode_error(segment, error.jet_show());
    };
    CtValue::List(
        entries
            .into_iter()
            .map(|entry| {
                let CtValue::Struct { fields, .. } = entry else {
                    return entry;
                };
                let text = |name: &str| {
                    fields.iter().find_map(|(field, value)| {
                        (field == name).then_some(value).and_then(|value| match value {
                            CtValue::Str(value) => Some(value.clone()),
                            _ => None,
                        })
                    })
                };
                let path = text("path").unwrap_or_default();
                let reason = text("reason").unwrap_or_default();
                CtValue::Struct {
                    type_name: "FieldError".to_string(),
                    fields: vec![
                        (
                            "path".to_string(),
                            CtValue::Str(if path.is_empty() {
                                segment.to_string()
                            } else if path.starts_with('[') {
                                format!("{segment}{path}")
                            } else {
                                format!("{segment}.{path}")
                            }),
                        ),
                        ("reason".to_string(), CtValue::Str(reason)),
                    ],
                }
            })
            .collect(),
    )
}

fn ambient_http_json_decode_error(span: crate::Diagnostics::Span) -> Result<CtValue, Diagnostic> {
    let mut recv = CtValue::Unit;
    let mut args = [];
    crate::Comptime::try_ambient_handle(
        "HTTPJSONDecodeError",
        &mut recv,
        &mut args,
        span,
    )
    .unwrap_or_else(|| Err(unsupported("HTTP JSON decode error adapter", span)))
}

fn datatree_kind(value: &CtValue) -> &'static str {
    match datatree_variant(value) {
        Some(("Null", _)) => "null",
        Some(("Bool", _)) => "Bool",
        Some(("Int", _)) => "Int",
        Some(("Float", _)) => "Float",
        Some(("Text", _)) => "Text",
        Some(("Array", _)) => "a list",
        Some(("Object", _)) => "an object",
        Some(("Bytes", _)) => "Bytes",
        _ => "value",
    }
}

fn datatree_object_pairs(value: &CtValue) -> Option<Vec<(String, CtValue)>> {
    let ("Object", Some(payload)) = datatree_variant(value)? else {
        return None;
    };
    match payload {
        CtValue::Struct { type_name, fields } if type_name == "JSONObject" => {
            Some(fields.clone())
        }
        CtValue::Map(fields) => fields
            .iter()
            .map(|(key, value)| match key {
                crate::AST::CtKey::Str(key) => Some((key.clone(), value.clone())),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

fn datatree_object(pairs: Vec<(String, CtValue)>) -> CtValue {
    datatree(
        "Object",
        Some(CtValue::Struct {
            type_name: "JSONObject".to_string(),
            fields: pairs,
        }),
    )
}

fn handle_index(value: &CtValue, type_name: &str) -> Option<usize> {
    let CtValue::Struct {
        type_name: actual,
        fields,
    } = value
    else {
        return None;
    };
    (actual == type_name).then_some(())?;
    fields.iter().find_map(|(name, value)| match (name.as_str(), value) {
        ("index", CtValue::Int(index)) => Some(*index as usize),
        _ => None,
    })
}

fn struct_int(value: &CtValue, field: &str) -> Option<i64> {
    let CtValue::Struct { fields, .. } = value else {
        return None;
    };
    fields.iter().find_map(|(name, value)| match value {
        CtValue::Int(value) if name == field => Some(*value),
        _ => None,
    })
}

fn show_typed_value(value: &CtValue, ty: &Type, debug: bool) -> Option<String> {
    match (value, ty) {
        (CtValue::Int(value), ty) => {
            let (signed, _) = crate::Comptime::MathLayout::integer_type_layout(ty)?;
            Some(crate::Comptime::MathLayout::integer_show(*value, signed))
        }
        (CtValue::Present(value), Type::Option(inner)) => {
            Some(show_typed_value(value, inner, debug).unwrap_or_else(|| {
                if debug {
                    value.debug_rust()
                } else {
                    value.jet_show()
                }
            }))
        }
        (CtValue::Failed(CtReport::Clean(_)), Type::Option(_)) => Some("null".to_string()),
        (CtValue::List(values), Type::List(inner) | Type::FixedList { elem: inner, .. }) => {
            let parts = values
                .iter()
                .map(|value| {
                    show_typed_value(value, inner, debug).unwrap_or_else(|| {
                        if debug {
                            value.debug_rust()
                        } else {
                            value.jet_show()
                        }
                    })
                })
                .collect::<Vec<_>>();
            Some(format!("[{}]", parts.join(", ")))
        }
        _ => None,
    }
}

impl<'a> EvalCtx<'a> {
    // #1799: these calls read or mutate runtime-owned clock/global state. A
    // build-time fold uses a throwaway evaluator, so materializing any of them
    // would freeze state that the running program cannot resync. Runtime and
    // REPL evaluation are the live execution paths and remain allowed. Keep
    // argument-only time constructors/conversions and the constant
    // `core.perf.default_fidelity` out of this list. The current parity leaks
    // are `date.today`'s SystemTime read and `time.instant`'s placeholder
    // monotonic sample. E3403 remains the determinism gate, while this
    // predicate only backs off D-VERDICT-1308-1.
    fn should_decline_ambient_fold(&self, module: &str, method: &str) -> bool {
        !self.runtime_execution
            && !self.repl_mode
            && matches!(
                (module, method),
                ("core.time", "now" | "now_utc" | "today" | "instant" | "start")
                    | ("core.time.date", "today")
                    | ("core.time.datetime", "now")
                    | ("core.perf", "fidelity" | "override_fidelity" | "reset_fidelity")
            )
    }

    fn serde_codec(&self, ty: &Type, method: &str) -> Option<&'a crate::Codegen::TIR::TFunc> {
        let concrete = format!("{}::{method}", ty.name());
        self.funcs.get(&concrete).copied().or_else(|| match ty {
            Type::Apply { name, .. } => self.funcs.get(&format!("{name}::{method}")).copied(),
            _ => None,
        })
    }

    fn eval_serde_encode_value(
        &mut self,
        value: CtValue,
        ty: &Type,
    ) -> Result<CtValue, Diagnostic> {
        match ty {
            Type::Named(name) if name == "DataTree" || name == "JSON" => Ok(value),
            Type::Int | Type::IntN { .. } => Ok(datatree("Int", Some(value))),
            Type::Float | Type::Float32 => Ok(datatree("Float", Some(value))),
            Type::Bool => Ok(datatree("Bool", Some(value))),
            Type::String => Ok(datatree("Text", Some(value))),
            Type::Char => match value {
                CtValue::Char(value) => Ok(datatree("Text", Some(CtValue::Str(value.to_string())))),
                _ => Err(unsupported("Encode Char value", self.span())),
            },
            Type::Option(inner) => match value {
                CtValue::Present(value) => self.eval_serde_encode_value(*value, inner),
                CtValue::Failed(CtReport::Clean(_)) => Ok(datatree("Null", None)),
                _ => Err(unsupported("Encode Option value", self.span())),
            },
            Type::List(inner) | Type::FixedList { elem: inner, .. }
                if matches!(
                    inner.as_ref(),
                    Type::IntN {
                        signed: false,
                        bits: 8
                    }
                ) =>
            {
                let CtValue::List(values) = value else {
                    return Err(unsupported("Encode byte list value", self.span()));
                };
                let bytes = values
                    .into_iter()
                    .map(|value| match value {
                        CtValue::Int(value) => u8::try_from(value)
                            .map_err(|_| unsupported("Encode U8 value", self.span())),
                        _ => Err(unsupported("Encode U8 value", self.span())),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(CtValue::Bytes(bytes))
            }
            Type::List(inner) | Type::FixedList { elem: inner, .. } => {
                let CtValue::List(values) = value else {
                    return Err(unsupported("Encode list value", self.span()));
                };
                let mut out = Vec::with_capacity(values.len());
                for value in values {
                    out.push(self.eval_serde_encode_value(value, inner)?);
                }
                Ok(datatree("Array", Some(CtValue::List(out))))
            }
            Type::Map { key, value: item, .. } if matches!(key.as_ref(), Type::String) => {
                let CtValue::Map(values) = value else {
                    return Err(unsupported("Encode map value", self.span()));
                };
                let mut out = std::collections::BTreeMap::new();
                for (key, value) in values {
                    let crate::AST::CtKey::Str(key) = key else {
                        return Err(unsupported("Encode map key", self.span()));
                    };
                    out.insert(
                        crate::AST::CtKey::Str(key),
                        self.eval_serde_encode_value(value, item)?,
                    );
                }
                Ok(datatree("Object", Some(CtValue::Map(out))))
            }
            Type::Tagged { inner, .. } => self.eval_serde_encode_value(value, inner),
            Type::Named(_) | Type::Apply { .. } => {
                if let Type::Named(name) = ty {
                    if let Some(base) = self.distinct_bases.get(name).cloned() {
                        return self.eval_serde_encode_value(value, &base);
                    }
                }
                let func = self
                    .serde_codec(ty, "encode")
                    .ok_or_else(|| unsupported(&format!("Encode body for `{}`", ty.name()), self.span()))?;
                let mut child = HashMap::new();
                child.insert("self".to_string(), value);
                self.run_func(func, Vec::new(), &mut child)
            }
            _ => Err(unsupported(&format!("Encode `{}`", ty.name()), self.span())),
        }
    }

    /// Replay the D-MIGRATE4 chain over a tree the current shape rejected.
    /// `Ok` carries the migrated tree and the `MigrationStatus` describing the
    /// walk, the same `from`/`steps` the generated chain-walker reports.
    fn apply_codec_migration(
        &mut self,
        type_name: &str,
        tree: &CtValue,
    ) -> Result<Option<Result<(CtValue, CtValue), CtValue>>, Diagnostic> {
        let Some(plan) = self.codec_migrations.get(type_name).cloned() else {
            return Ok(None);
        };
        let Some(mut pairs) = datatree_object_pairs(tree) else {
            return Ok(None);
        };
        let keys: std::collections::BTreeSet<&str> =
            pairs.iter().map(|(key, _)| key.as_str()).collect();
        let Some(start) = (0..plan.historical_shapes.len()).rev().find(|index| {
            let shape = &plan.historical_shapes[*index];
            shape.len() == keys.len() && shape.iter().all(|key| keys.contains(key.as_str()))
        }) else {
            return Ok(None);
        };
        for step in plan.steps.iter().skip(start) {
            for op in step {
                match op {
                    crate::Codegen::TIR::TCodecMigrationOp::Rename {
                        from_key,
                        to_key,
                    } => {
                        if let Some((key, _)) =
                            pairs.iter_mut().find(|(key, _)| key == from_key)
                        {
                            *key = to_key.clone();
                        }
                    }
                    crate::Codegen::TIR::TCodecMigrationOp::Remove { key } => {
                        pairs.retain(|(field, _)| field != key);
                    }
                    crate::Codegen::TIR::TCodecMigrationOp::Add {
                        key,
                        ty,
                        default_fn,
                    } => {
                        let func = self.funcs.get(default_fn).copied().ok_or_else(|| {
                            unsupported(
                                &format!("migration default `{default_fn}`"),
                                self.span(),
                            )
                        })?;
                        let mut child = HashMap::new();
                        let value = self.run_func(func, Vec::new(), &mut child)?;
                        let encoded = self.eval_serde_encode_value(value, ty)?;
                        pairs.push((key.clone(), encoded));
                    }
                    crate::Codegen::TIR::TCodecMigrationOp::Change {
                        key,
                        from_ty,
                        to_ty,
                        converter_fn,
                    } => {
                        let Some((_, encoded)) =
                            pairs.iter_mut().find(|(field, _)| field == key)
                        else {
                            return Ok(None);
                        };
                        let old = match self.eval_datatree_decode(encoded.clone(), from_ty)? {
                            CtValue::Present(value) => *value,
                            CtValue::Failed(CtReport::Told(error)) => {
                                return Ok(Some(Err(decode_error_under(key, *error))));
                            }
                            _ => unreachable!(),
                        };
                        let func = self.funcs.get(converter_fn).copied().ok_or_else(|| {
                            unsupported(
                                &format!("migration converter `{converter_fn}`"),
                                self.span(),
                            )
                        })?;
                        let mut child = HashMap::new();
                        let converted = self.run_func(func, vec![old], &mut child)?;
                        *encoded = self.eval_serde_encode_value(converted, to_ty)?;
                    }
                }
            }
        }
        Ok(Some(Ok((
            datatree_object(pairs),
            migration_status(start, plan.steps.len()),
        ))))
    }

    fn eval_datatree_decode(
        &mut self,
        tree: CtValue,
        ty: &Type,
    ) -> Result<CtValue, Diagnostic> {
        self.eval_datatree_decode_status(tree, ty, &mut None)
    }

    /// D-MIGRATE3=A: `json|toml|yaml|csv . decode<T>` and `. decode_traced<T>`.
    ///
    /// Marshalling only (I9). The tree comes from the codec's own parser, the
    /// one `apply_core_call` already hosts for `parse`; the value comes out of
    /// the type's generated Decode TIR, the body AOT compiles. `decode` is
    /// `decode_traced(…)?.value`, exactly as `jet_enc_*_decode` defines it, so
    /// both spellings walk the same migration chain.
    fn eval_typed_codec_decode(
        &mut self,
        module: &str,
        method: &str,
        ret_ty: &Type,
        argv: &[CtValue],
        span: Span,
    ) -> Result<CtValue, Diagnostic> {
        let shape = || unsupported(&format!("`{module}.{method}()` resolved return type"), span);
        let Type::Result { ok, .. } = ret_ty else {
            return Err(shape());
        };
        let traced = method == "decode_traced";
        let target = if traced {
            match &**ok {
                Type::Apply { name, args } if name == "DecodeResult" => {
                    args.first().cloned().ok_or_else(shape)?
                }
                _ => return Err(shape()),
            }
        } else {
            (**ok).clone()
        };
        let Some(CtValue::Str(text)) = argv.first() else {
            return Err(unsupported(
                &format!("`{module}.{method}()` text argument"),
                span,
            ));
        };
        let text = text.clone();
        let decoded = if module == "core.encoding.csv" {
            self.decode_codec_rows(&target, text, span)?
        } else {
            self.decode_codec_value(module, &target, text, span)?
        };
        let (value, migration) = match decoded {
            Ok(pair) => pair,
            Err(error) => return Ok(CtValue::failed(Box::new(error))),
        };
        Ok(CtValue::Present(Box::new(if traced {
            CtValue::Struct {
                type_name: "DecodeResult".to_string(),
                fields: vec![
                    ("value".to_string(), value),
                    ("migration".to_string(), migration),
                ],
            }
        } else {
            value
        })))
    }

    /// One whole record: parse, then decode. `Err` is the `[FieldError]` value.
    fn decode_codec_value(
        &mut self,
        module: &str,
        target: &Type,
        text: String,
        span: Span,
    ) -> Result<Result<(CtValue, CtValue), CtValue>, Diagnostic> {
        let parsed = apply_core_call(
            module,
            "parse",
            vec![CtValue::Str(text)],
            span,
            self.repl_mode,
        )?;
        let tree = match parsed {
            CtValue::Present(tree) => *tree,
            CtValue::Failed(CtReport::Told(error)) => {
                return Ok(Err(crate::Comptime::codec_parse_error_for_tir(
                    codec_label(module),
                    *error,
                )))
            }
            _ => return Err(unsupported(&format!("`{module}.parse()` result"), span)),
        };
        let mut status = None;
        Ok(match self.eval_datatree_decode_status(tree, target, &mut status)? {
            CtValue::Present(value) => {
                Ok((*value, status.unwrap_or_else(migration_status_fresh)))
            }
            CtValue::Failed(CtReport::Told(error)) => Err(*error),
            _ => unreachable!("Decode protocol returns Result"),
        })
    }

    /// CSV decodes to `[T]`: the header row names the fields, every later row
    /// becomes an object of `Text` cells. Row errors collect under `row <n>`
    /// (1-based) and a short row leaves its cells empty, as the Prelude does.
    /// A file is one column layout, so the batch reports the first row that
    /// actually migrated.
    fn decode_codec_rows(
        &mut self,
        target: &Type,
        text: String,
        span: Span,
    ) -> Result<Result<(CtValue, CtValue), CtValue>, Diagnostic> {
        let Type::List(item) = target else {
            return Err(unsupported("`core.encoding.csv.decode()` row type", span));
        };
        let parsed = apply_core_call(
            "core.encoding.csv",
            "parse",
            vec![CtValue::Str(text)],
            span,
            self.repl_mode,
        )?;
        let rows = match parsed {
            CtValue::Present(rows) => match *rows {
                CtValue::List(rows) => rows,
                _ => return Err(unsupported("`core.encoding.csv.parse()` result", span)),
            },
            CtValue::Failed(CtReport::Told(error)) => {
                return Ok(Err(decode_error("", string_cell(&error))))
            }
            _ => return Err(unsupported("`core.encoding.csv.parse()` result", span)),
        };
        let mut rows = rows.into_iter();
        let Some(CtValue::List(header)) = rows.next() else {
            return Ok(Ok((CtValue::List(Vec::new()), migration_status_fresh())));
        };
        let header: Vec<String> = header.iter().map(string_cell).collect();
        let mut values = Vec::new();
        let mut errors = Vec::new();
        let mut migration = migration_status_fresh();
        for (index, row) in rows.enumerate() {
            let cells = match row {
                CtValue::List(cells) => cells,
                _ => Vec::new(),
            };
            let tree = datatree_object(
                header
                    .iter()
                    .enumerate()
                    .map(|(column, name)| {
                        let cell = cells.get(column).map(string_cell).unwrap_or_default();
                        (name.clone(), datatree("Text", Some(CtValue::Str(cell))))
                    })
                    .collect(),
            );
            let mut status = None;
            match self.eval_datatree_decode_status(tree, item, &mut status)? {
                CtValue::Present(value) => {
                    if let Some(status) = status {
                        if migration_did_run(&status) && !migration_did_run(&migration) {
                            migration = status;
                        }
                    }
                    values.push(*value);
                }
                CtValue::Failed(CtReport::Told(error)) => {
                    if let CtValue::List(entries) =
                        decode_error_under(&format!("row {}", index + 1), *error)
                    {
                        errors.extend(entries);
                    }
                }
                _ => unreachable!("Decode protocol returns Result"),
            }
        }
        if !errors.is_empty() {
            return Ok(Err(CtValue::List(errors)));
        }
        Ok(Ok((CtValue::List(values), migration)))
    }

    /// `status` reports the D-MIGRATE3 `MigrationStatus` of the top-level
    /// record. Only `decode_traced` asks for it; `eval_datatree_decode` passes
    /// `None`, so a nested field that migrates cannot leak into its parent's
    /// status — matching AOT, where nested fields call plain `jet_decode`.
    fn eval_datatree_decode_status(
        &mut self,
        tree: CtValue,
        ty: &Type,
        status: &mut Option<CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let result: Result<CtValue, CtValue> = match ty {
            Type::Named(name) if name == "DataTree" || name == "JSON" => Ok(tree),
            Type::Int | Type::IntN { .. } => {
                let decoded = match datatree_variant(&tree) {
                    Some(("Int", Some(CtValue::Int(value)))) => Ok(CtValue::Int(*value)),
                    Some(("Float", Some(CtValue::Float(value))))
                        if value.as_f64().fract() == 0.0 =>
                    {
                        Ok(CtValue::Int(value.as_f64() as i64))
                    }
                    Some(("Text", Some(CtValue::Str(value)))) => value
                        .trim()
                        .parse::<i64>()
                        .map(CtValue::Int)
                        .map_err(|_| {
                            decode_error(
                                "",
                                format!("expected {}, found text {:?}", ty.name(), value),
                            )
                        }),
                    _ => Err(decode_error(
                        "",
                        format!("expected {}, found {}", ty.name(), datatree_kind(&tree)),
                    )),
                };
                let decoded = match decoded {
                    Ok(value) => value,
                    Err(error) => return Ok(CtValue::failed(Box::new(error))),
                };
                if let Type::IntN { signed, bits } = ty {
                    let CtValue::Int(int_value) = &decoded else {
                        unreachable!();
                    };
                    let in_range = if *signed {
                        let shift = u32::from(*bits - 1);
                        (-(1_i128 << shift)..=(1_i128 << shift) - 1)
                            .contains(&i128::from(*int_value))
                    } else {
                        (0..=(1_i128 << u32::from(*bits)) - 1)
                            .contains(&i128::from(*int_value))
                    };
                    if !in_range {
                        let found = if !*signed && *bits == 8 {
                            "Int"
                        } else {
                            "out-of-range Int"
                        };
                        return Ok(CtValue::failed(Box::new(decode_error(
                            "",
                            format!("expected {}, found {found}", ty.name()),
                        ))));
                    }
                }
                Ok(decoded)
            }
            Type::Float => match datatree_variant(&tree) {
                Some(("Float", Some(CtValue::Float(value)))) => {
                    Ok(CtValue::Float(CtFloat::f64(value.as_f64())))
                }
                Some(("Int", Some(CtValue::Int(value)))) => {
                    Ok(CtValue::Float(CtFloat::f64(*value as f64)))
                }
                Some(("Text", Some(CtValue::Str(value)))) => value
                    .trim()
                    .parse::<f64>()
                    .map(|value| CtValue::Float(CtFloat::f64(value)))
                    .map_err(|_| {
                        decode_error(
                            "",
                            format!("expected Float, found text {:?}", value),
                        )
                    }),
                _ => Err(decode_error(
                    "",
                    format!("expected {}, found {}", ty.name(), datatree_kind(&tree)),
                )),
            },
            Type::Float32 => {
                let value = match datatree_variant(&tree) {
                    Some(("Float", Some(CtValue::Float(value)))) => value.as_f64(),
                    Some(("Int", Some(CtValue::Int(value)))) => *value as f64,
                    _ => {
                        return Ok(CtValue::failed(Box::new(decode_error(
                            "",
                            format!("expected F32, found {}", datatree_kind(&tree)),
                        ))));
                    }
                };
                if value.is_finite()
                    && value >= -(f32::MAX as f64)
                    && value <= f32::MAX as f64
                {
                    Ok(CtValue::Float(CtFloat::f32(value as f32)))
                } else {
                    Err(decode_error(
                        "",
                        "expected F32, found out-of-range Float",
                    ))
                }
            }
            Type::Bool => match datatree_variant(&tree) {
                Some(("Bool", Some(CtValue::Bool(value)))) => Ok(CtValue::Bool(*value)),
                Some(("Text", Some(CtValue::Str(value)))) => match value.trim() {
                    "true" => Ok(CtValue::Bool(true)),
                    "false" => Ok(CtValue::Bool(false)),
                    _ => Err(decode_error(
                        "",
                        format!("expected Bool, found text {:?}", value),
                    )),
                },
                _ => Err(decode_error(
                    "",
                    format!("expected Bool, found {}", datatree_kind(&tree)),
                )),
            },
            Type::String => match datatree_variant(&tree) {
                Some(("Text", Some(CtValue::Str(value)))) => Ok(CtValue::Str(value.clone())),
                Some(("Int", Some(CtValue::Int(value)))) => Ok(CtValue::Str(value.to_string())),
                Some(("Float", Some(CtValue::Float(value)))) => Ok(CtValue::Str(format!("{value:?}"))),
                Some(("Bool", Some(CtValue::Bool(value)))) => Ok(CtValue::Str(value.to_string())),
                _ => Err(decode_error(
                    "",
                    format!("expected Text, found {}", datatree_kind(&tree)),
                )),
            },
            Type::Char => match self.eval_datatree_decode(tree, &Type::String)? {
                CtValue::Present(value) => {
                    let CtValue::Str(value) = *value else {
                        unreachable!();
                    };
                    let mut chars = value.chars();
                    match (chars.next(), chars.next()) {
                        (Some(value), None) => Ok(CtValue::Char(value)),
                        _ => Err(decode_error("", format!("expected a single Char, found {value:?}"))),
                    }
                }
                CtValue::Failed(CtReport::Told(error)) => Err(*error),
                _ => unreachable!(),
            },
            Type::Option(inner) => match datatree_variant(&tree) {
                Some(("Null", _)) => Ok(CtValue::absent((**inner).clone())),
                _ => match self.eval_datatree_decode(tree, inner)? {
                    CtValue::Present(value) => Ok(CtValue::Present(value)),
                    CtValue::Failed(CtReport::Told(error)) => Err(*error),
                    _ => unreachable!(),
                },
            },
            Type::List(inner) | Type::FixedList { elem: inner, .. }
                if matches!(
                    inner.as_ref(),
                    Type::IntN {
                        signed: false,
                        bits: 8
                    }
                ) && matches!(tree, CtValue::Bytes(_)) =>
            {
                let CtValue::Bytes(bytes) = tree else {
                    unreachable!();
                };
                if let Type::FixedList { len, .. } = ty {
                    if bytes.len() != *len as usize {
                        return Ok(CtValue::failed(Box::new(decode_error(
                            "",
                            format!(
                                "expected a fixed list of length {len}, found {}",
                                bytes.len()
                            ),
                        ))));
                    }
                }
                Ok(CtValue::List(
                    bytes.into_iter().map(|byte| CtValue::Int(i64::from(byte))).collect(),
                ))
            }
            Type::List(inner) | Type::FixedList { elem: inner, .. } => {
                let Some(("Array", Some(CtValue::List(values)))) = datatree_variant(&tree) else {
                    return Ok(CtValue::failed(Box::new(decode_error(
                        "",
                        format!("expected a list, found {}", datatree_kind(&tree)),
                    ))));
                };
                if let Type::FixedList { len, .. } = ty {
                    if values.len() != *len as usize {
                        return Ok(CtValue::failed(Box::new(decode_error(
                            "",
                            format!("expected a fixed list of length {len}, found {}", values.len()),
                        ))));
                    }
                }
                let mut out = Vec::with_capacity(values.len());
                let mut errors = Vec::new();
                for (index, value) in values.iter().cloned().enumerate() {
                    match self.eval_datatree_decode(value, inner)? {
                        CtValue::Present(value) => out.push(*value),
                        CtValue::Failed(CtReport::Told(error)) => {
                            if let CtValue::List(items) = decode_error_under(
                                &format!("[{index}]"),
                                *error,
                            ) {
                                errors.extend(items);
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                if errors.is_empty() {
                    Ok(CtValue::List(out))
                } else {
                    Err(CtValue::List(errors))
                }
            }
            Type::Map { key, value: item, .. } if matches!(key.as_ref(), Type::String) => {
                let Some(("Object", Some(object))) = datatree_variant(&tree) else {
                    return Ok(CtValue::failed(Box::new(decode_error(
                        "",
                        format!("expected an object, found {}", datatree_kind(&tree)),
                    ))));
                };
                let values: Vec<(String, CtValue)> = match object {
                    CtValue::Map(values) => values
                        .iter()
                        .filter_map(|(key, value)| match key {
                            crate::AST::CtKey::Str(key) => Some((key.clone(), value.clone())),
                            _ => None,
                        })
                        .collect(),
                    CtValue::Struct { type_name, fields } if type_name == "JSONObject" => {
                        fields.clone()
                    }
                    _ => {
                        return Ok(CtValue::failed(Box::new(decode_error(
                            "",
                            format!("expected an object, found {}", datatree_kind(&tree)),
                        ))));
                    }
                };
                let mut out = std::collections::BTreeMap::new();
                let mut errors = Vec::new();
                for (key, value) in values {
                    match self.eval_datatree_decode(value, item)? {
                        CtValue::Present(value) => {
                            out.insert(crate::AST::CtKey::Str(key.clone()), *value);
                        }
                        CtValue::Failed(CtReport::Told(error)) => {
                            if let CtValue::List(items) = decode_error_under(&key, *error) {
                                errors.extend(items);
                            }
                        }
                        _ => unreachable!(),
                    }
                }
                if errors.is_empty() {
                    Ok(CtValue::Map(out))
                } else {
                    Err(CtValue::List(errors))
                }
            }
            Type::Tagged { inner, .. } => {
                return self.eval_datatree_decode(tree, inner);
            }
            Type::Named(_) | Type::Apply { .. } => {
                if let Type::Named(name) = ty {
                    if let Some(base) = self.distinct_bases.get(name).cloned() {
                        let decoded = self.eval_datatree_decode(tree, &base)?;
                        if let (
                            Some((lo, hi)),
                            CtValue::Present(value),
                        ) = (self.distinct_ranges.get(name), &decoded)
                        {
                            if !matches!(value.as_ref(), CtValue::Int(n) if (*lo..=*hi).contains(n))
                            {
                                return Ok(CtValue::failed(Box::new(decode_error(
                                    "",
                                    format!("expected {name} within {lo}..{hi}"),
                                ))));
                            }
                        }
                        return Ok(decoded);
                    }
                }
                let func = self
                    .serde_codec(ty, "decode")
                    .ok_or_else(|| unsupported(&format!("Decode body for `{}`", ty.name()), self.span()))?;
                let mut child = HashMap::new();
                let migration_trace_start = self
                    .codec_migrations
                    .contains_key(&ty.name())
                    .then(|| {
                        self.sink.as_ref().map_or(0, |sink| {
                            sink.lock().expect("evaluator sink poisoned").stderr.len()
                        })
                    });
                let result = self.run_func(func, vec![tree.clone()], &mut child)?;
                if matches!(result, CtValue::Failed(CtReport::Told(_))) {
                    // The emitted migration walker probes the current shape
                    // through Rust's short-circuiting `?`, so only its first
                    // failed field contributes a propagation frame. Generated
                    // Decode TIR can visit later fields while constructing the
                    // failed value; keep the same observable trace as AOT.
                    if let (Some(start), Some(sink)) = (migration_trace_start, self.sink.as_ref()) {
                        let mut sink = sink.lock().expect("evaluator sink poisoned");
                        if let Some(line_end) = sink.stderr[start..].find('\n') {
                            sink.stderr.truncate(start + line_end + 1);
                        }
                    }
                    match self.apply_codec_migration(&ty.name(), &tree)? {
                        Some(Ok((migrated, walked))) => {
                            let mut child = HashMap::new();
                            let out = self.run_func(func, vec![migrated], &mut child)?;
                            if matches!(out, CtValue::Present(_)) {
                                *status = Some(walked);
                            }
                            return Ok(out);
                        }
                        Some(Err(error)) => {
                            return Ok(CtValue::failed(Box::new(error)));
                        }
                        None => {}
                    }
                }
                *status = Some(migration_status_fresh());
                return Ok(result);
            }
            _ => Err(decode_error(
                "",
                format!("cannot decode `{}`", ty.name()),
            )),
        };
        Ok(match result {
            Ok(value) => CtValue::Present(Box::new(value)),
            Err(error) => CtValue::failed(Box::new(error)),
        })
    }

    fn eval_cell_guard_project(
        &mut self,
        recv: &'a TExpr,
        paths: &[Vec<String>],
        editable: bool,
        edit_paths_disjoint: bool,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let handle = self.eval_expr(recv, scope)?;
        if editable {
            let index = local_cell_index(&handle, "__JetTirCellEditGuard")
                .ok_or_else(|| unsupported("Cell edit guard projection", self.span()))?;
            let (guard, owner) = self
                .local_cells
                .take_edit_guard(index)
                .map_err(|message| unsupported(&message, self.span()))?;
            match paths {
                [path] => {
                    let projected = guard.map(|value| {
                        project_mut(value, path)
                            .expect("sema validated Cell edit guard projection")
                    });
                    let index = self.local_cells.insert_edit_guard_for(projected, owner);
                    Ok(local_cell_handle("__JetTirCellEditGuard", index))
                }
                [first, second] => {
                    debug_assert!(edit_paths_disjoint);
                    let (first_guard, second_guard) = guard.split(|value| {
                        project_pair_mut(value, first, second)
                            .expect("sema proved disjoint Cell edit guard projections")
                    });
                    let first_index =
                        self.local_cells.insert_edit_guard_for(first_guard, owner);
                    let second_index =
                        self.local_cells.insert_edit_guard_for(second_guard, owner);
                    Ok(CtValue::Struct {
                        type_name: "tuple".to_string(),
                        fields: vec![
                            (
                                "first".to_string(),
                                local_cell_handle("__JetTirCellEditGuard", first_index),
                            ),
                            (
                                "second".to_string(),
                                local_cell_handle("__JetTirCellEditGuard", second_index),
                            ),
                        ],
                    })
                }
                _ => Err(unsupported("Cell edit guard projection shape", self.span())),
            }
        } else {
            let index = local_cell_index(&handle, "__JetTirCellReadGuard")
                .ok_or_else(|| unsupported("Cell read guard projection", self.span()))?;
            let (guard, owner) = self
                .local_cells
                .take_read_guard(index)
                .map_err(|message| unsupported(&message, self.span()))?;
            match paths {
                [path] => {
                    let projected = guard.map(|value| {
                        project_ref(value, path)
                            .expect("sema validated Cell read guard projection")
                    });
                    let index = self.local_cells.insert_read_guard_for(projected, owner);
                    Ok(local_cell_handle("__JetTirCellReadGuard", index))
                }
                [first, second] => {
                    let (first_guard, second_guard) = guard.split(|value| {
                        (
                            project_ref(value, first)
                                .expect("sema validated Cell read projection"),
                            project_ref(value, second)
                                .expect("sema validated Cell read projection"),
                        )
                    });
                    let first_index =
                        self.local_cells.insert_read_guard_for(first_guard, owner);
                    let second_index =
                        self.local_cells.insert_read_guard_for(second_guard, owner);
                    Ok(CtValue::Struct {
                        type_name: "tuple".to_string(),
                        fields: vec![
                            (
                                "first".to_string(),
                                local_cell_handle("__JetTirCellReadGuard", first_index),
                            ),
                            (
                                "second".to_string(),
                                local_cell_handle("__JetTirCellReadGuard", second_index),
                            ),
                        ],
                    })
                }
                _ => Err(unsupported("Cell read guard projection shape", self.span())),
            }
        }
    }

    fn eval_local_cell_method(
        &mut self,
        receiver: &CtValue,
        method: &str,
        args: &'a [TExpr],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        if let Some(index) = local_cell_index(receiver, "__JetTirCell") {
            let cell = self
                .local_cells
                .cell(index)
                .ok_or_else(|| unsupported("Cell handle", self.span()))?;
            return match method {
                "get" => Ok(cell.get()),
                "set" => {
                    let value = self.eval_expr(
                        args.first()
                            .ok_or_else(|| unsupported("Cell.set argument", self.span()))?,
                        scope,
                    )?;
                    cell.set(value);
                    Ok(CtValue::Unit)
                }
                "replace" => {
                    let value = self.eval_expr(
                        args.first()
                            .ok_or_else(|| unsupported("Cell.replace argument", self.span()))?,
                        scope,
                    )?;
                    Ok(cell.replace(value))
                }
                "read" | "edit" => {
                    let Some(TExpr {
                        kind: TExprKind::Lambda(lambda),
                        ..
                    }) = args.first()
                    else {
                        return Err(unsupported("Cell callback", self.span()));
                    };
                    if method == "read" {
                        cell.read(|value| {
                            self.eval_tlambda(lambda, vec![value.clone()], scope)
                        })
                    } else {
                        cell.edit(|value| {
                            let (result, updated) =
                                self.eval_tlambda_mut_arg(lambda, value.clone(), scope)?;
                            *value = updated;
                            Ok(result)
                        })
                    }
                }
                "guard_read" => {
                    let guard = cell.guard_read();
                    let index = self.local_cells.insert_read_guard(guard);
                    Ok(local_cell_handle("__JetTirCellReadGuard", index))
                }
                "guard_edit" => {
                    let guard = cell.guard_edit();
                    let index = self.local_cells.insert_edit_guard(guard);
                    Ok(local_cell_handle("__JetTirCellEditGuard", index))
                }
                "get_or_set" => {
                    let Some(TExpr {
                        kind: TExprKind::Lambda(lambda),
                        ..
                    }) = args.first()
                    else {
                        return Err(unsupported("Cell.get_or_set callback", self.span()));
                    };
                    cell.try_get_or_set(|| self.eval_tlambda(lambda, Vec::new(), scope))
                }
                _ => Err(unsupported(&format!("Cell.{method}"), self.span())),
            };
        }
        if let Some(index) = local_cell_index(receiver, "__JetTirCellReadGuard") {
            let guard = self
                .local_cells
                .read_guard(index)
                .ok_or_else(|| unsupported("Cell read guard", self.span()))?;
            return match method {
                "get" => Ok(guard.get()),
                "read" => {
                    let Some(TExpr {
                        kind: TExprKind::Lambda(lambda),
                        ..
                    }) = args.first()
                    else {
                        return Err(unsupported("Cell guard callback", self.span()));
                    };
                    guard.read(|value| {
                        self.eval_tlambda(lambda, vec![value.clone()], scope)
                    })
                }
                _ => Err(unsupported(
                    &format!("CellReadGuard.{method}"),
                    self.span(),
                )),
            };
        }
        let index = local_cell_index(receiver, "__JetTirCellEditGuard")
            .ok_or_else(|| unsupported("Cell edit guard", self.span()))?;
        let guard = self
            .local_cells
            .edit_guard(index)
            .ok_or_else(|| unsupported("Cell edit guard", self.span()))?;
        match method {
            "get" => Ok(guard.get()),
            "set" => {
                let value = self.eval_expr(
                    args.first().ok_or_else(|| {
                        unsupported("Cell guard set argument", self.span())
                    })?,
                    scope,
                )?;
                guard.set(value);
                Ok(CtValue::Unit)
            }
            "read" | "edit" => {
                let Some(TExpr {
                    kind: TExprKind::Lambda(lambda),
                    ..
                }) = args.first()
                else {
                    return Err(unsupported("Cell guard callback", self.span()));
                };
                if method == "read" {
                    guard.read(|value| {
                        self.eval_tlambda(lambda, vec![value.clone()], scope)
                    })
                } else {
                    guard.edit(|value| {
                        let (result, updated) =
                            self.eval_tlambda_mut_arg(lambda, value.clone(), scope)?;
                        *value = updated;
                        Ok(result)
                    })
                }
            }
            _ => Err(unsupported(
                &format!("CellEditGuard.{method}"),
                self.span(),
            )),
        }
    }

    pub(crate) fn eval_expr(
        &mut self,
        expr: &'a TExpr,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        self.enter_source_nesting()?;
        let mut expr = expr;
        let mut transparent_depth = 0;
        while let TExprKind::Clone(inner) = &expr.kind {
            if let Err(diagnostic) = self.enter_source_nesting() {
                for _ in 0..=transparent_depth {
                    self.leave_source_nesting();
                }
                return Err(diagnostic);
            }
            transparent_depth += 1;
            expr = inner;
        }
        let result = self.eval_expr_inner(expr, scope);
        for _ in 0..=transparent_depth {
            self.leave_source_nesting();
        }
        result
    }

    fn eval_expr_inner(
        &mut self,
        expr: &'a TExpr,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        self.burn()?;
        match &expr.kind {
            TExprKind::IntLit(n, _) => Ok(CtValue::Int(*n)),
            TExprKind::FloatLit(v) => {
                let is_f32 = matches!(&expr.ty, Type::Float32);
                Ok(CtValue::Float(CtFloat::literal(*v, is_f32)))
            }
            TExprKind::BoolLit(b) => Ok(CtValue::Bool(*b)),
            TExprKind::CharLit(c) => Ok(CtValue::Char(*c)),
            TExprKind::SharedGuardValue { guard, .. } => {
                let guard = self.eval_expr(guard, scope)?;
                let (index, _, _, path) = shared_guard_parts(&guard)
                    .ok_or_else(|| unsupported("SharedGuard handle", self.span()))?;
                let value = self.runtime
                    .lock()
                    .expect("evaluator runtime poisoned")
                    .shared_values
                    .get(index)
                    .cloned()
                    .ok_or_else(|| unsupported("SharedGuard value", self.span()))?;
                let value = value
                    .value
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                shared_projection(&value, &path)
                    .cloned()
                    .ok_or_else(|| unsupported("SharedGuard projection", self.span()))
            }
            TExprKind::SharedGuardMap { guard, path, .. } => {
                let mut guard = self.eval_expr(guard, scope)?;
                append_shared_guard_path(&mut guard, path)
                    .then_some(guard)
                    .ok_or_else(|| unsupported("SharedGuard map", self.span()))
            }
            TExprKind::SharedGuardSplit {
                guard,
                first,
                second,
                ..
            } => {
                let guard = self.eval_expr(guard, scope)?;
                let mut first_guard = guard.clone();
                let mut second_guard = guard;
                if !append_shared_guard_path(&mut first_guard, first)
                    || !append_shared_guard_path(&mut second_guard, second)
                {
                    return Err(unsupported("SharedGuard split", self.span()));
                }
                Ok(CtValue::Struct {
                    type_name: "tuple".to_string(),
                    fields: vec![
                        ("first".to_string(), first_guard),
                        ("second".to_string(), second_guard),
                    ],
                })
            }
            TExprKind::SharedGuardWait {
                guard,
                condition,
                predicate,
            } => {
                let guard = self.eval_expr(guard, scope)?;
                let (shared_index, lease_index, editable, path) = shared_guard_parts(&guard)
                    .ok_or_else(|| unsupported("SharedGuard wait", self.span()))?;
                if !editable {
                    return Ok(CtValue::failed(Box::new(CtValue::Str(
                        "a condition wait needs an edit guard".to_string(),
                    ))));
                }
                let condition = self.eval_expr(condition, scope)?;
                let condition_index = condition_index(&condition)
                    .ok_or_else(|| unsupported("Condition wait", self.span()))?;
                let state = self
                    .runtime
                    .lock()
                    .expect("evaluator runtime poisoned")
                    .shared_conditions
                    .get(condition_index)
                    .cloned()
                    .ok_or_else(|| unsupported("Condition wait", self.span()))?;
                let lease = self
                    .runtime
                    .lock()
                    .expect("evaluator runtime poisoned")
                    .shared_guards
                    .get(lease_index)
                    .cloned()
                    .ok_or_else(|| unsupported("SharedGuard wait lease", self.span()))?;
                let shared = self
                    .runtime
                    .lock()
                    .expect("evaluator runtime poisoned")
                    .shared_values
                    .get(shared_index)
                    .cloned()
                    .ok_or_else(|| unsupported("SharedGuard wait value", self.span()))?;
                let cancel = self.task_cancel.clone();
                match super::shared_protocol::jet_shared_condition_wait(
                    &lease,
                    &state,
                    || {
                    let root = shared
                        .value
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    let value = shared_projection(&root, &path)
                        .cloned()
                        .ok_or_else(|| unsupported("SharedGuard wait value", self.span()))?;
                    drop(root);
                        match self.eval_tlambda(predicate, vec![value], scope)? {
                            CtValue::Bool(ready) => Ok(ready),
                            _ => Err(unsupported(
                                "SharedGuard wait predicate result",
                                self.span(),
                            )),
                        }
                    },
                    || {
                        std::sync::Arc::new(super::EvalConditionWaiter::new(cancel.clone()))
                    },
                ) {
                    Ok(()) => Ok(CtValue::Present(Box::new(CtValue::Unit))),
                    Err(super::shared_protocol::JetConditionWaitError::Predicate(error)) => {
                        Err(error)
                    }
                    Err(super::shared_protocol::JetConditionWaitError::Cancelled) => {
                        self.task_wait_cancel_check()?;
                        Err(unsupported("SharedGuard condition wait", self.span()))
                    }
                }
            }
            TExprKind::ConditionNotify { condition, all } => {
                let condition = self.eval_expr(condition, scope)?;
                let index = condition_index(&condition)
                    .ok_or_else(|| unsupported("Condition notify", self.span()))?;
                let state = self
                    .runtime
                    .lock()
                    .expect("evaluator runtime poisoned")
                    .shared_conditions
                    .get(index)
                    .cloned()
                    .ok_or_else(|| unsupported("Condition notify", self.span()))?;
                if *all {
                    state.notify_all();
                } else {
                    state.notify_one();
                }
                Ok(CtValue::Unit)
            }
            TExprKind::StrLit(parts) => {
                let mut out = String::new();
                for part in parts {
                    match part {
                        TStrPart::Lit(s) => out.push_str(s),
                        TStrPart::Interp(e, fmt) => {
                            let v = self.eval_expr(e, scope)?;
                            let text = match fmt {
                                crate::AST::StrFormat::Debug => {
                                    let manual = match &e.ty {
                                        Type::Named(type_name) => {
                                            self.funcs.get(&format!("{type_name}::debug")).copied()
                                        }
                                        _ => None,
                                    };
                                    if let Some(func) = manual {
                                        let mut child = HashMap::new();
                                        child.insert("self".to_string(), v.clone());
                                        match self.run_func(func, Vec::new(), &mut child)? {
                                            CtValue::Str(text) => text,
                                            _ => self.debug_value(&v),
                                        }
                                    } else {
                                        show_typed_value(&v, &e.ty, true)
                                            .unwrap_or_else(|| self.debug_value(&v))
                                    }
                                }
                                crate::AST::StrFormat::Display => {
                                    show_typed_value(&v, &e.ty, false)
                                        .unwrap_or(self.show_value(&v, scope)?)
                                }
                                crate::AST::StrFormat::Fixed(_) => {
                                    unreachable!("Fixed interpolation lowers to core.fmt.decimal")
                                }
                                crate::AST::StrFormat::Unit(_) => {
                                    unreachable!("Unit interpolation lowers to a String")
                                }
                            };
                            out.push_str(&text);
                        }
                    }
                }
                Ok(CtValue::Str(out))
            }
            TExprKind::Local(local) => {
                let value = scope
                .get(&local.name)
                .cloned()
                .or_else(|| self.globals.get(&local.name).cloned())
                    .ok_or_else(|| {
                        unsupported(&format!("unbound `{}`", local.name), self.span())
                    })?;
                // D-MEM1 S9 / D-PIN1=A: a whole-place window local reads the
                // owner's current storage, never the value it held at binding.
                if let Some(read) = super::read_place_mut(&value, scope, self.span()) {
                    return read;
                }
                if local.uninit_fixed {
                    if matches!(value, CtValue::List(_)) {
                        Ok(value)
                    } else {
                        super::uninit_fixed_materialize(&value).ok_or_else(|| {
                            unsupported("uninitialized fixed-list local read", self.span())
                        })
                    }
                } else {
                    Ok(value)
                }
            }
            TExprKind::InlineBlock(stmts) => {
                // Raw comptime fragments reach TIR before sema rewrites the
                // private yielding-loop sends to `List.push`. Collect them
                // here; checked runtime programs use the ordinary List path.
                let raw_collecting = matches!(&expr.ty, Type::List(_))
                    && matches!(
                        stmts.last(),
                        Some(
                            crate::Codegen::TIR::TStmt::ForIn { .. }
                                | crate::Codegen::TIR::TStmt::Range { .. }
                                | crate::Codegen::TIR::TStmt::CountedLoop { .. }
                        )
                    );
                if raw_collecting {
                    self.collecting_items.push(Vec::new());
                    let flow = self.exec_stmts(stmts, scope);
                    let items = self
                        .collecting_items
                        .pop()
                        .expect("raw collecting loop installs one item sink");
                    return match flow? {
                        Flow::Normal => Ok(CtValue::List(items)),
                        Flow::Return(value) => {
                            self.pending_return = Some(value);
                            Ok(CtValue::Unit)
                        }
                        other => {
                            self.pending_flow = Some(other);
                            Err(unsupported("pending loop control", self.span()))
                        }
                    };
                }
                let Some((tail, prefix)) = stmts.split_last() else {
                    return Ok(CtValue::Unit);
                };
                // AOT emits a real Rust block here and the JIT saves and
                // restores its slots, so this block's own `let` bindings must
                // not outlive it in the interpreter either. Restore exactly the
                // names it introduces — a collecting loop still needs to write
                // through to the enclosing scope, so a blanket child scope
                // would be wrong.
                let bound: Vec<(String, Option<CtValue>)> = prefix
                    .iter()
                    .filter_map(|stmt| match stmt {
                        crate::Codegen::TIR::TStmt::Let { name, .. } => {
                            Some((name.clone(), scope.get(name).cloned()))
                        }
                        _ => None,
                    })
                    .collect();
                let restore = |scope: &mut HashMap<String, CtValue>| {
                    for (name, prior) in &bound {
                        match prior {
                            Some(value) => {
                                scope.insert(name.clone(), value.clone());
                            }
                            None => {
                                scope.remove(name);
                            }
                        }
                    }
                };
                match self.exec_stmts(prefix, scope)? {
                    Flow::Normal => {}
                    Flow::Return(value) => {
                        self.pending_return = Some(value);
                        return Ok(CtValue::Unit);
                    }
                    other => {
                        self.pending_flow = Some(other);
                        return Err(unsupported("pending loop control", self.span()));
                    }
                }
                if let crate::Codegen::TIR::TStmt::Loop { label, body } = tail {
                    let value = self.exec_loop_value(label.as_deref(), body, scope);
                    restore(scope);
                    return value;
                }
                let value = match tail {
                    crate::Codegen::TIR::TStmt::ExprStmt(value) => self.eval_expr(value, scope),
                    crate::Codegen::TIR::TStmt::Return(value) => {
                        let value = match value {
                            Some(value) => self.eval_expr(value, scope)?,
                            None => CtValue::Unit,
                        };
                        self.pending_return = Some(value);
                        Ok(CtValue::Unit)
                    }
                    _ => match self.exec_stmt(tail, scope)? {
                        Flow::Normal => Ok(CtValue::Unit),
                        Flow::Return(value) => {
                            self.pending_return = Some(value);
                            Ok(CtValue::Unit)
                        }
                        other => {
                            self.pending_flow = Some(other);
                            Err(unsupported("pending loop control", self.span()))
                        }
                    },
                };
                restore(scope);
                value
            }
            TExprKind::Uninit => match &expr.ty {
                Type::FixedList { len, .. } => Ok(super::uninit_fixed_carrier(*len as usize)),
                _ => Ok(CtValue::Unit),
            },
            TExprKind::Unit | TExprKind::DefaultLit => Ok(CtValue::Unit),
            TExprKind::CtLit(v) => Ok(v.clone()),
            TExprKind::ConstRef(name) => self
                .globals
                .get(name)
                .cloned()
                .ok_or_else(|| unsupported(&format!("const `{name}`"), self.span())),
            TExprKind::Print(inner) => {
                let v = self.eval_expr(inner, scope)?;
                if self.pending_return.is_some() {
                    return Ok(CtValue::Unit);
                }
                let shown = match show_typed_value(&v, &inner.ty, false) {
                    Some(shown) => shown,
                    None => self.show_value(&v, scope)?,
                };
                self.write_print(&shown, false)?;
                Ok(CtValue::Unit)
            }
            TExprKind::Drop(inner) => {
                let _ = self.eval_expr(inner, scope)?;
                Ok(CtValue::Unit)
            }
            TExprKind::Close(inner) => {
                let value = self.eval_expr(inner, scope)?;
                let type_name = match &inner.ty {
                    Type::Named(n) | Type::Apply { name: n, .. } => n.as_str(),
                    _ => return Ok(CtValue::Unit),
                };
                let key = format!("{type_name}::close");
                if let Some(func) = self.funcs.get(&key).copied() {
                    let mut child = HashMap::new();
                    child.insert("self".to_string(), value);
                    let _ = self.run_func(func, Vec::new(), &mut child)?;
                }
                Ok(CtValue::Unit)
            }
            TExprKind::Binary { op, lhs, rhs, .. } => {
                let l = self.eval_expr(lhs, scope)?;
                let r = self.eval_expr(rhs, scope)?;
                if matches!(op, BinOp::Eq | BinOp::Ne)
                    && matches!(
                        &lhs.ty,
                        Type::Named(name) if name == crate::Syntax::TYPE_RANGE
                    )
                {
                    let Some((left_start, left_end, left_exclusive)) = range_parts(&l) else {
                        return Err(unsupported("Range equality", self.span()));
                    };
                    let Some((right_start, right_end, right_exclusive)) = range_parts(&r) else {
                        return Err(unsupported("Range equality", self.span()));
                    };
                    let equal = super::range_semantics::jet_range_equal(
                        left_start,
                        left_end,
                        left_exclusive,
                        right_start,
                        right_end,
                        right_exclusive,
                    );
                    return Ok(CtValue::Bool(if matches!(op, BinOp::Eq) {
                        equal
                    } else {
                        !equal
                    }));
                }
                if let Type::IntN { signed, bits } = &lhs.ty {
                    let a = as_int(&l, self.span())?;
                    let b = as_int(&r, self.span())?;
                    let right_signed =
                        crate::Comptime::MathLayout::integer_type_layout(&rhs.ty)
                            .map(|(signed, _)| signed)
                            .unwrap_or(true);
                    return crate::Comptime::MathLayout::integer_binop(
                        *op,
                        a,
                        b,
                        *signed,
                        *bits,
                        right_signed,
                        self.span(),
                    );
                }
                eval_binop(*op, l, r, self.span())
            }
            TExprKind::Unary { op, operand } => {
                let v = self.eval_expr(operand, scope)?;
                match (*op, v) {
                    (UnOp::Neg, CtValue::Int(n))
                        if matches!(&operand.ty, Type::IntN { signed: true, .. }) =>
                    {
                        let (_, bits) =
                            crate::Comptime::MathLayout::integer_type_layout(&operand.ty)
                                .expect("IntN layout");
                        crate::Comptime::MathLayout::integer_neg(n, bits, self.span())
                    }
                    (UnOp::Neg, CtValue::Int(n)) => n
                        .checked_neg()
                        .map(CtValue::Int)
                        .ok_or_else(|| unsupported("integer negation overflow", self.span())),
                    (UnOp::Neg, CtValue::Float(n)) => Ok(CtValue::Float(n.neg())),
                    (UnOp::Neg, CtValue::BigInt(n)) => Ok(CtValue::BigInt(n.neg())),
                    (UnOp::Not, CtValue::Bool(b)) => Ok(CtValue::Bool(!b)),
                    // D-BITNOT1=A: on a whole number `!` turns over every bit.
                    // A sized type flips exactly its own width, so the result
                    // is narrowed back to it, the same way `-` is above; the
                    // width-free default `Int` keeps all 64, which is `-x - 1`.
                    (UnOp::Not, CtValue::Int(n))
                        if matches!(&operand.ty, Type::IntN { .. }) =>
                    {
                        let (signed, bits) =
                            crate::Comptime::MathLayout::integer_type_layout(&operand.ty)
                                .expect("IntN layout");
                        Ok(CtValue::Int(
                            crate::Comptime::MathLayout::integer_narrow(
                                !(n as i128),
                                signed,
                                bits,
                            ),
                        ))
                    }
                    (UnOp::Not, CtValue::Int(n)) => Ok(CtValue::Int(!n)),
                    _ => Err(unsupported("unary form", self.span())),
                }
            }
            TExprKind::CompareChain { operands, ops, .. } => {
                let mut vals = Vec::with_capacity(operands.len());
                for o in operands {
                    vals.push(self.eval_expr(o, scope)?);
                }
                for (i, op) in ops.iter().enumerate() {
                    let part = if let Type::IntN { signed, bits } = &operands[i].ty {
                        let right_signed =
                            crate::Comptime::MathLayout::integer_type_layout(&operands[i + 1].ty)
                                .map(|(signed, _)| signed)
                                .unwrap_or(true);
                        crate::Comptime::MathLayout::integer_binop(
                            *op,
                            as_int(&vals[i], self.span())?,
                            as_int(&vals[i + 1], self.span())?,
                            *signed,
                            *bits,
                            right_signed,
                            self.span(),
                        )?
                    } else {
                        eval_binop(*op, vals[i].clone(), vals[i + 1].clone(), self.span())?
                    };
                    if !as_bool(&part, self.span())? {
                        return Ok(CtValue::Bool(false));
                    }
                }
                Ok(CtValue::Bool(true))
            }
            TExprKind::Call { name, args, .. } => self.eval_call(name, args, scope),
            TExprKind::IfExpr {
                cond,
                then_body,
                then_value,
                else_body,
                else_value,
            } => {
                if self.eval_if_cond(cond, scope)? {
                    match self.exec_stmts(then_body, scope)? {
                        Flow::Return(v) => return Ok(v),
                        Flow::Normal => {}
                        other => {
                            self.pending_flow = Some(other);
                            return Err(unsupported("pending loop control", self.span()));
                        }
                    }
                    self.eval_expr(then_value, scope)
                } else {
                    match self.exec_stmts(else_body, scope)? {
                        Flow::Return(v) => return Ok(v),
                        Flow::Normal => {}
                        other => {
                            self.pending_flow = Some(other);
                            return Err(unsupported("pending loop control", self.span()));
                        }
                    }
                    self.eval_expr(else_value, scope)
                }
            }
            TExprKind::BuiltinMethod { recv, op, args } => {
                let is_tensor = matches!(&recv.ty, Type::Named(name) if name == "Tensor")
                    || matches!(&recv.ty, Type::Apply { name, .. } if name == "Tensor");
                if matches!(
                    op,
                    crate::Codegen::TIR::TBuiltinOp::ViewMutNew { .. }
                        | crate::Codegen::TIR::TBuiltinOp::ComputeViewMutNew { .. }
                ) {
                    let base_name = match &recv.kind {
                        TExprKind::Local(local) => local.name.clone(),
                        TExprKind::Borrow { place, .. } => match &place.kind {
                            TExprKind::Local(local) => local.name.clone(),
                            _ => {
                                return Err(unsupported("view-mut base", self.span()));
                            }
                        },
                        _ => return Err(unsupported("view-mut base", self.span())),
                    };
                    let base_value = scope
                        .get(&base_name)
                        .cloned()
                        .ok_or_else(|| unsupported("view-mut unbound base", self.span()))?;
                    if is_tensor {
                        let evaluated_args = args
                            .iter()
                            .map(|arg| self.eval_expr(arg, scope))
                            .collect::<Result<Vec<_>, _>>()?;
                        let (start, end_exclusive) =
                            crate::Comptime::ComputeLite::tensor_view_window(
                                &base_value,
                                &evaluated_args,
                                self.span(),
                            )?;
                        return Ok(CtValue::Struct {
                            type_name: "__JetViewMut".into(),
                            fields: vec![
                                ("base".into(), CtValue::Str(base_name)),
                                ("start".into(), CtValue::Int(start as i64)),
                                ("end".into(), CtValue::Int(end_exclusive as i64 - 1)),
                            ],
                        });
                    }
                    let CtValue::List(xs) = base_value else {
                        return Err(unsupported("view-mut list base", self.span()));
                    };
                    let (start, end_exclusive) = if args.len() == 1 {
                        let range = self.eval_expr(&args[0], scope)?;
                        super::range_window(&range, xs.len(), self.span())?
                    } else {
                        let start = as_int(&self.eval_expr(&args[0], scope)?, self.span())?;
                        let end = as_int(&self.eval_expr(&args[1], scope)?, self.span())?;
                        if start < 0 || end < start || end as usize >= xs.len() {
                            return Err(super::view_bounds_diagnostic(
                                xs.len(),
                                start,
                                end,
                                false,
                                self.span(),
                            ));
                        }
                        (start, end + 1)
                    };
                    let end = end_exclusive - 1;
                    return Ok(CtValue::Struct {
                        type_name: "__JetViewMut".into(),
                        fields: vec![
                            ("base".into(), CtValue::Str(base_name)),
                            ("start".into(), CtValue::Int(start)),
                            ("end".into(), CtValue::Int(end)),
                        ],
                    });
                }
                if matches!(
                    op,
                    crate::Codegen::TIR::TBuiltinOp::SplitWrite { .. }
                        | crate::Codegen::TIR::TBuiltinOp::GetDisjointWrite
                ) {
                    let base_name = match &recv.kind {
                        TExprKind::Local(local) => local.name.clone(),
                        TExprKind::Borrow { place, .. } => match &place.kind {
                            TExprKind::Local(local) => local.name.clone(),
                            _ => return Err(unsupported("disjoint-view base", self.span())),
                        },
                        _ => return Err(unsupported("disjoint-view base", self.span())),
                    };
                    let CtValue::List(xs) = scope
                        .get(&base_name)
                        .cloned()
                        .ok_or_else(|| unsupported("disjoint-view unbound base", self.span()))?
                    else {
                        return Err(unsupported("disjoint-view list base", self.span()));
                    };
                    let view = |start: i64, end: i64| CtValue::Struct {
                        type_name: "__JetViewMut".into(),
                        fields: vec![
                            ("base".into(), CtValue::Str(base_name.clone())),
                            ("start".into(), CtValue::Int(start)),
                            ("end".into(), CtValue::Int(end)),
                        ],
                    };
                    match op {
                        crate::Codegen::TIR::TBuiltinOp::SplitWrite { tuple_struct } => {
                            let mid =
                                as_int(&self.eval_expr(&args[0], scope)?, self.span())?;
                            let ((left_start, left_end), (right_start, right_end)) =
                                match super::disjoint_semantics::split(xs.len(), mid) {
                                    Ok(bounds) => bounds,
                                    Err(error) => {
                                        return Ok(CtValue::failed(Box::new(CtValue::Str(error))));
                                    }
                                };
                            return Ok(CtValue::Present(Box::new(CtValue::Struct {
                                type_name: tuple_struct.clone(),
                                fields: vec![
                                    (
                                        "left".into(),
                                        view(left_start as i64, left_end as i64 - 1),
                                    ),
                                    (
                                        "right".into(),
                                        view(right_start as i64, right_end as i64 - 1),
                                    ),
                                ],
                            })));
                        }
                        crate::Codegen::TIR::TBuiltinOp::GetDisjointWrite => {
                            let CtValue::List(targets) = self.eval_expr(&args[0], scope)? else {
                                return Err(unsupported("disjoint-view targets", self.span()));
                            };
                            let mut indexes = Vec::with_capacity(targets.len());
                            for target in targets {
                                indexes.push(as_int(&target, self.span())?);
                            }
                            let ordered =
                                match super::disjoint_semantics::indexes(xs.len(), &indexes) {
                                    Ok(bounds) => bounds,
                                    Err(error) => {
                                        return Ok(CtValue::failed(Box::new(CtValue::Str(error))));
                                    }
                                };
                            let mut views = ordered
                                .into_iter()
                                .map(|(start, end, position)| {
                                    (position, view(start as i64, end as i64 - 1))
                                })
                                .collect::<Vec<_>>();
                            views.sort_by_key(|(position, _)| *position);
                            return Ok(CtValue::Present(Box::new(CtValue::List(
                                views.into_iter().map(|(_, view)| view).collect(),
                            ))));
                        }
                        _ => unreachable!(),
                    }
                }
                let mut r = self.eval_expr(recv, scope)?;
                let progress = progress_parts(&r);
                let iter = progress_iter_parts(&r);
                if let Some((items, _, _, _, _, _, _, _)) = &progress {
                    r = CtValue::List(items.clone());
                } else if let Some((items, _)) = &iter {
                    r = CtValue::List(items.clone());
                }
                // `__JetViewMut` is a write-through handle; read builtins see the
                // inclusive window as a List (same surface as View after ViewNew).
                // Do not write the temporary List back over the ViewMut binding.
                let mut skip_view_mut_wb = matches!(
                    op,
                    crate::Codegen::TIR::TBuiltinOp::ComputeViewNew { .. }
                );
                if let CtValue::Struct {
                    type_name,
                    fields,
                } = &r
                {
                    if type_name == "__JetViewMut"
                        && matches!(
                            *op,
                            crate::Codegen::TIR::TBuiltinOp::LenList
                                | crate::Codegen::TIR::TBuiltinOp::IsEmpty
                                | crate::Codegen::TIR::TBuiltinOp::GetList
                                | crate::Codegen::TIR::TBuiltinOp::First
                                | crate::Codegen::TIR::TBuiltinOp::Last
                                | crate::Codegen::TIR::TBuiltinOp::Contains
                                | crate::Codegen::TIR::TBuiltinOp::IndexOf
                                | crate::Codegen::TIR::TBuiltinOp::JoinSep
                        )
                    {
                        r = materialize_view_mut_window(fields, scope, self.span())?;
                        skip_view_mut_wb = true;
                    }
                }
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(a, scope)?);
                }
                let progress_arg = argv.first().cloned();
                if let Some((source_items, description, format, started_at, pulls, tail, total, known_total)) = &progress {
                    if progress_terminal_builtin(op) {
                        let raw_pulls = if matches!(op, crate::Codegen::TIR::TBuiltinOp::TryCollect) {
                            try_collect_pulls(source_items, pulls, *tail)
                        } else {
                            pulls.iter().sum::<usize>().saturating_add(*tail)
                        };
                        emit_progress_pulls(
                            self.sink.as_ref(),
                            description,
                            format,
                            *started_at,
                            *total,
                            *known_total,
                            raw_pulls,
                        );
                    }
                }
                let mut result = eval_builtin(op, &mut r, argv, self.span())?;
                if let Some((source_items, description, format, started_at, source_pulls, source_tail, total, known_total)) = progress {
                    if progress_lazy_builtin(op) {
                        let CtValue::List(items) = result else {
                            return Err(unsupported("progress adapter result", self.span()));
                        };
                        let (pulls, tail) = progress_builtin_plan(
                            op,
                            &source_items,
                            &items,
                            &source_pulls,
                            source_tail,
                            progress_arg.as_ref(),
                        );
                        r = progress_value(
                            items,
                            description,
                            format,
                            started_at,
                            pulls,
                            tail,
                            total,
                            known_total,
                        );
                        result = r.clone();
                    }
                }
                if !skip_view_mut_wb {
                    self.write_back_place(recv, r, scope)?;
                }
                Ok(result)
            }
            TExprKind::HandleMethod { recv, op, args } => {
                let mut r = self.eval_expr(recv, scope)?;
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(a, scope)?);
                }
                if matches!(op, crate::Codegen::TIR::THandleOp::ReflectValueDisplay) {
                    let CtValue::Struct { type_name, fields } = &r else {
                        return Err(unsupported("reflect value", self.span()));
                    };
                    if type_name != "__Reflect" {
                        return Err(unsupported("reflect value", self.span()));
                    }
                    let value = fields
                        .iter()
                        .find_map(|(name, value)| (name == "value").then_some(value))
                        .ok_or_else(|| unsupported("reflect value", self.span()))?;
                    // Reflection display is the ordinary Display surface. Keep
                    // it on the evaluator path so user `display` methods and
                    // pure core display semantics match AOT `jet_display()`.
                    return self.show_value(value, scope).map(CtValue::Str);
                }
                if let crate::Codegen::TIR::THandleOp::WebAppMethod { method } = op {
                    return self.eval_web_app_method(&r, method, argv);
                }
                if let Some(index) = handle_index(&r, "__JetTirClock") {
                    let delta = argv.first().and_then(|value| match value {
                        CtValue::Int(value) => Some(*value),
                        CtValue::Struct { type_name, fields }
                            if type_name == crate::Syntax::DURATION_TYPE
                                || type_name == "Duration" =>
                        {
                            fields.iter().find_map(|(name, v)| match (name.as_str(), v) {
                                ("ms", CtValue::Int(ms)) => Some(*ms),
                                _ => None,
                            })
                        }
                        _ => None,
                    });
                    let span = self.span();
                    let mut runtime = self.runtime.lock().expect("evaluator runtime poisoned");
                    let Some(clock) = runtime.clocks.get_mut(index) else {
                        return Err(unsupported("clock handle", span));
                    };
                    let result = match op {
                        crate::Codegen::TIR::THandleOp::ClockNow => CtValue::Int(*clock),
                        // D-DET-CAPAPI: `advance(to_ms)` sets an absolute instant;
                        // `tick` / `wait` advance relatively. Match AOT `jet_clock_*`.
                        crate::Codegen::TIR::THandleOp::ClockAdvance => {
                            let Some(to_ms) = delta else {
                                return Err(unsupported("clock advance target", span));
                            };
                            *clock = to_ms;
                            CtValue::Int(*clock)
                        }
                        crate::Codegen::TIR::THandleOp::ClockTick
                        | crate::Codegen::TIR::THandleOp::ClockWait => {
                            let Some(delta) = delta else {
                                return Err(unsupported("clock delta", span));
                            };
                            *clock = clock.saturating_add(delta);
                            CtValue::Int(*clock)
                        }
                        _ => return Err(unsupported("clock method", self.span())),
                    };
                    return Ok(result);
                }
                if let crate::Codegen::TIR::THandleOp::EventMethod { method } = op {
                    return self.eval_event_method(method, &mut r, &argv);
                }
                match op {
                    crate::Codegen::TIR::THandleOp::TaskJoin => {
                        return self.take_task(&r);
                    }
                    crate::Codegen::TIR::THandleOp::TaskDetach => {
                        let _ = self.take_task(&r)?;
                        return Ok(CtValue::Unit);
                    }
                    crate::Codegen::TIR::THandleOp::SerdeEncode => {
                        return self.eval_serde_encode_value(r, &recv.ty);
                    }
                    crate::Codegen::TIR::THandleOp::DataTreeDecode(target) => {
                        return self.eval_datatree_decode(r, target);
                    }
                    _ => {}
                }
                if let crate::Codegen::TIR::THandleOp::RegexMethod { method, .. } = op {
                    if method == "replace_all_with" {
                        return self.eval_regex_replace_all_with(&r, &argv);
                    }
                }
                if matches!(
                    op,
                    crate::Codegen::TIR::THandleOp::ExpiringMethod { .. }
                ) {
                    let deadline = struct_int(&r, "deadline")
                        .ok_or_else(|| unsupported("expiring deadline", self.span()))?;
                    let clock_index = argv
                        .first()
                        .and_then(|clock| handle_index(clock, "__JetTirClock"))
                        .ok_or_else(|| unsupported("expiring clock", self.span()))?;
                    let now = *self
                        .runtime
                        .lock()
                        .expect("evaluator runtime poisoned")
                        .clocks
                        .get(clock_index)
                        .ok_or_else(|| unsupported("expiring clock handle", self.span()))?;
                    let valid = now <= deadline;
                    let result = match op {
                        crate::Codegen::TIR::THandleOp::ExpiringMethod { method }
                            if method == "is_valid" =>
                        {
                            CtValue::Bool(valid)
                        }
                        crate::Codegen::TIR::THandleOp::ExpiringMethod { method }
                            if method == "get" =>
                        {
                            let value = if valid {
                                let CtValue::Struct { fields, .. } = &r else {
                                    unreachable!();
                                };
                                fields
                                    .iter()
                                    .find_map(|(name, value)| {
                                        (name == "value").then(|| value.clone())
                                    })
                                    .unwrap_or(CtValue::Unit)
                            } else {
                                CtValue::Str("expired".to_string())
                            };
                            if valid {
                                CtValue::Present(Box::new(value))
                            } else {
                                CtValue::failed(Box::new(value))
                            }
                        }
                        _ => return Err(unsupported("expiring method", self.span())),
                    };
                    return Ok(result);
                }
                // D-VERDICT-1323-1 / D-COROUTINE1=A: the task control plane
                // reaches the evaluator's task table, which the shared handle
                // dispatch has no access to. Each `*_all` twin is exactly its
                // single-handle counterpart applied in order.
                {
                    use crate::Codegen::TIR::THandleOp as Op;
                    match op {
                        Op::ChannelReceive => {
                            let index = handle_index(&r, "Receiver")
                                .ok_or_else(|| unsupported("channel receiver", self.span()))?;
                            return self.receive_eval_channel(index);
                        }
                        Op::SenderSend => {
                            let index = handle_index(&r, "Sender")
                                .ok_or_else(|| unsupported("channel sender", self.span()))?;
                            let value = argv
                                .first()
                                .cloned()
                                .ok_or_else(|| unsupported("channel send value", self.span()))?;
                            self.send_eval_channel(index, value)?;
                            return Ok(CtValue::Unit);
                        }
                        Op::ChannelClose => {
                            let index = handle_index(&r, "Sender")
                                .or_else(|| handle_index(&r, "Receiver"))
                                .ok_or_else(|| unsupported("channel", self.span()))?;
                            self.close_eval_channel(index)?;
                            return Ok(CtValue::Unit);
                        }
                        _ => {}
                    }
                    let each = |this: &mut Self, value: &CtValue| -> Result<(), Diagnostic> {
                        match op {
                            Op::TaskCancel | Op::TaskCancelAll => this.cancel_task_value(value),
                            Op::TaskPause | Op::TaskPauseAll => {
                                this.set_task_paused_value(value, true)
                            }
                            Op::TaskResume | Op::TaskResumeAll => {
                                this.set_task_paused_value(value, false)
                            }
                            Op::TaskDetach | Op::TaskDetachAll => this.detach_task_value(value),
                            _ => unreachable!("guarded by the outer match"),
                        }
                    };
                    match op {
                        Op::TaskCancel | Op::TaskPause | Op::TaskResume | Op::TaskDetach => {
                            let receiver = r.clone();
                            each(self, &receiver)?;
                            return Ok(CtValue::Unit);
                        }
                        Op::TaskCancelAll
                        | Op::TaskPauseAll
                        | Op::TaskResumeAll
                        | Op::TaskDetachAll => {
                            let CtValue::List(tasks) = &r else {
                                return Err(unsupported("task group receiver", self.span()));
                            };
                            for task in tasks.clone() {
                                each(self, &task)?;
                            }
                            return Ok(CtValue::Unit);
                        }
                        Op::TaskTrace => return self.trace_task_value(&r.clone()),
                        Op::TaskException => {
                            let index = Self::task_index(&r)
                                .ok_or_else(|| unsupported("task receiver", self.span()))?;
                            let runtime =
                                self.runtime.lock().expect("evaluator runtime poisoned");
                            let cancel = match runtime.tasks.get(index) {
                                Some(Some(task)) => {
                                    task.cancel.load(std::sync::atomic::Ordering::Acquire)
                                }
                                _ => false,
                            };
                            return Ok(CtValue::Str(if cancel {
                                "cancelled".to_string()
                            } else {
                                String::new()
                            }));
                        }
                        Op::TaskTraceAll => {
                            let CtValue::List(tasks) = &r else {
                                return Err(unsupported("task group receiver", self.span()));
                            };
                            let mut traces = Vec::new();
                            for task in tasks.clone() {
                                traces.push(self.trace_task_value(&task)?);
                            }
                            return Ok(CtValue::List(traces));
                        }
                        _ => {}
                    }
                }
                let mut result = eval_handle(op, &mut r, &mut argv, self.span())?;
                let http_json = matches!(
                    op,
                    crate::Codegen::TIR::THandleOp::HTTPClientMethod { method, .. }
                        | crate::Codegen::TIR::THandleOp::HTTPServerMethod { method, .. }
                        if method == "json"
                );
                if http_json {
                    result = match result {
                        CtValue::Failed(CtReport::Told(_)) => result,
                        CtValue::Present(value) => {
                            let CtValue::Str(text) = *value else {
                                return Err(unsupported("HTTP JSON text adapter", self.span()));
                            };
                            let parsed = apply_core_call(
                                "core.encoding.json",
                                "parse",
                                vec![CtValue::Str(text)],
                                self.span(),
                                self.repl_mode,
                            )?;
                            let tree = match parsed {
                                CtValue::Present(tree) => *tree,
                                CtValue::Failed(CtReport::Told(_)) => {
                                    ambient_http_json_decode_error(self.span())?
                                }
                                _ => {
                                    return Err(unsupported(
                                        "HTTP JSON parse result",
                                        self.span(),
                                    ))
                                }
                            };
                            if matches!(tree, CtValue::Failed(CtReport::Told(_))) {
                                tree
                            } else {
                                let Type::Result { ok, .. } = &expr.ty else {
                                    return Err(unsupported(
                                        "HTTP JSON resolved result type",
                                        self.span(),
                                    ));
                                };
                                match self.eval_datatree_decode(tree, ok)? {
                                    CtValue::Present(value) => CtValue::Present(value),
                                    CtValue::Failed(CtReport::Told(_)) => {
                                        ambient_http_json_decode_error(self.span())?
                                    }
                                    _ => unreachable!("DataTree decode returns Result"),
                                }
                            }
                        }
                        _ => return Err(unsupported("HTTP JSON ambient result", self.span())),
                    };
                }
                self.write_back_place(recv, r, scope)?;
                // `Rng.shuffle(&list)` mutates the list arg in place. Fragment
                // lowering may keep `&deck` as Local (Write convention on the
                // AST CallArg) rather than wrapping TExprKind::Borrow.
                let force_arg_wb = matches!(*op, crate::Codegen::TIR::THandleOp::RngShuffle);
                for (place, value) in args.iter().zip(argv.into_iter()) {
                    if force_arg_wb {
                        self.write_back_place(place, value, scope)?;
                        continue;
                    }
                    if matches!(place.kind, TExprKind::Borrow { .. }) {
                        self.write_back_place(place, value, scope)?;
                    }
                }
                Ok(result)
            }
            TExprKind::CoreCall {
                module,
                method,
                args,
                source_span,
                ..
            } => {
                if module == "core.data" {
                    return self.eval_core_data_call(method, args, &expr.ty, scope);
                }
                if module == "core.compute" {
                    return self.eval_core_compute_call(method, args, *source_span, scope);
                }
                if module == "core.services" {
                    return self.eval_core_services_call(method, args, *source_span, scope);
                }
                // D-PIN1 / S58: `mem.address_of(place)` is an inert address cast.
                // AOT lowers to `(&place as *const _ as usize as i64)`. The
                // interpreter has no real addresses, so mint a stable non-zero
                // identity from the place path (I9: same non-zero / inequality
                // facts a program can observe) — but ONLY when this evaluator is
                // actually running the program (`runtime_execution`). A sema-time
                // comptime fold (D-VERDICT-1308-1's implicit fold, or an explicit
                // `#Known`) calls this exact same code path to *try* folding the
                // binding; baking the synthetic identity as an AOT `i64` literal
                // there would compile a wild-pointer dereference into the
                // program — a real memory-safety bug, not just a wrong value.
                // Refuse so the fold declines and the call lowers to real runtime
                // codegen instead (I1).
                if module == "core.mem" && method == "address_of" && args.len() == 1 {
                    if !self.runtime_execution {
                        return Err(unsupported(
                            "`mem.address_of` at compile time",
                            *source_span,
                        ));
                    }
                    let key = tir_place_address_key(&args[0]);
                    return Ok(CtValue::Int(stable_place_address(&key)));
                }
                if module == "core.tasks" && method == "channel" {
                    if !self.runtime_execution {
                        return Err(unsupported("`tasks.channel` at compile time", *source_span));
                    }
                    let capacity = args
                        .first()
                        .map(|arg| self.eval_expr(arg, scope))
                        .transpose()?
                        .map(|value| as_int(&value, self.span()))
                        .transpose()?;
                    return Ok(self.new_eval_channel(capacity));
                }
                if module == "core.tasks" && method == "yield_now" && args.is_empty() {
                    std::thread::yield_now();
                    return Ok(CtValue::Unit);
                }
                if module == "core.tasks" && method == "current_task" && args.is_empty() {
                    // Outside a spawned evaluator task: idle defaults match Prelude.
                    return Ok(CtValue::Str(
                        jet_foundation::StructuralDebug::jet_task_control_trace(false, false),
                    ));
                }
                if module == "core.mem" && method == "volatile_write" && args.len() == 2 {
                    let pointer = self.eval_expr(&args[0], scope)?;
                    let value = self.eval_expr(&args[1], scope)?;
                    let CtValue::Struct { type_name, fields } = pointer else {
                        return Err(unsupported("raw pointer carrier", self.span()));
                    };
                    if type_name != "__JetRawLocal" {
                        return Err(unsupported("raw pointer target", self.span()));
                    }
                    let name = fields.iter().find_map(|(field, value)| {
                        match (field.as_str(), value) {
                            ("name", CtValue::Str(name)) => Some(name.clone()),
                            _ => None,
                        }
                    });
                    let Some(name) = name else {
                        return Err(unsupported("raw pointer local", self.span()));
                    };
                    scope.insert(name, value);
                    return Ok(CtValue::Unit);
                }
                if module == "core.mem" && method == "volatile_read" && args.len() == 1 {
                    let pointer = self.eval_expr(&args[0], scope)?;
                    let CtValue::Struct { type_name, fields } = pointer else {
                        return Err(unsupported("raw pointer carrier", self.span()));
                    };
                    if type_name != "__JetRawLocal" {
                        return Err(unsupported("raw pointer target", self.span()));
                    }
                    let name = fields.iter().find_map(|(field, value)| {
                        match (field.as_str(), value) {
                            ("name", CtValue::Str(name)) => Some(name.as_str()),
                            _ => None,
                        }
                    });
                    return name
                        .and_then(|name| scope.get(name).cloned())
                        .ok_or_else(|| unsupported("raw pointer local", self.span()));
                }
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(a, scope)?);
                }
                let progress_known_total = if module == "core.io" && method == "progress" {
                    if let Some((items, known_total)) =
                        argv.first().and_then(progress_iter_parts)
                    {
                        if let Some(source) = argv.first_mut() {
                            *source = CtValue::List(items);
                        }
                        Some(known_total)
                    } else {
                        match args.first().map(|arg| &arg.ty) {
                            Some(Type::List(_)) | Some(Type::FixedList { .. }) => Some(true),
                            Some(Type::Apply { name, .. })
                                if name == crate::Syntax::TYPE_ITER => Some(
                                    args.first()
                                        .is_some_and(progress_source_has_exact_total),
                                ),
                            _ => None,
                        }
                    }
                } else {
                    None
                };
                // AOT reflection uses the resolved user-struct layout. Keep
                // that static fact in the erased TIR carrier so `.fields()`
                // cannot expose implementation fields from built-ins or
                // guess from a runtime `CtValue::Struct` alone.
                if module == "core.reflect" && method == "of" && args.len() == 1 {
                    let value = argv
                        .pop()
                        .ok_or_else(|| unsupported("reflect value", *source_span))?;
                    let field_names = match &args[0].ty {
                        Type::Named(type_name) => self
                            .struct_fields
                            .get(type_name)
                            .or_else(|| {
                                self.struct_fields
                                    .get(type_name.strip_prefix("user_").unwrap_or(type_name))
                            })
                            .map(|fields| {
                                CtValue::List(
                                    fields
                                        .iter()
                                        .map(|(name, _)| CtValue::Str(name.clone()))
                                        .collect(),
                                )
                            }),
                        _ => None,
                    };
                    let mut fields = vec![("value".to_string(), value)];
                    if let Some(field_names) = field_names {
                        fields.push(("field_names".to_string(), field_names));
                    }
                    return Ok(CtValue::Struct {
                        type_name: "__Reflect".to_string(),
                        fields,
                    });
                }
                if module == "core.web" && matches!(method.as_str(), "app" | "page") {
                    return self.eval_web_core_call(method, argv);
                }
                if module == "core.http.server" && method == "json" && args.len() == 2 {
                    let tree =
                        self.eval_serde_encode_value(argv[1].clone(), &args[1].ty)?;
                    argv[1] = CtValue::Str(crate::Comptime::render_datatree_for_tir(&tree));
                }
                if module == "core.encoding.cbor"
                    && matches!(method.as_str(), "to_bytes" | "to_bytes_canonical")
                {
                    let value = argv.first().ok_or_else(|| {
                        unsupported("core.encoding.cbor encoder missing its value", *source_span)
                    })?;
                    let tree = self.eval_serde_encode_value(value.clone(), &args[0].ty)?;
                    let fields = HashMap::new();
                    return Ok(match crate::Comptime::cbor_encode_typed_for_tir(
                        &tree,
                        &Type::Named("DataTree".to_string()),
                        &fields,
                        method == "to_bytes_canonical",
                    ) {
                        Ok(bytes) => CtValue::Present(Box::new(CtValue::Bytes(bytes))),
                        Err(reason) => CtValue::failed(Box::new(CtValue::Struct {
                            type_name: "CBORError".to_string(),
                            fields: vec![
                                (
                                    "kind".to_string(),
                                    CtValue::Enum {
                                        type_name: "CBORErrorKind".to_string(),
                                        variant: "Unsupported".to_string(),
                                        args: Vec::new(),
                                    },
                                ),
                                ("byte_offset".to_string(), CtValue::Int(0)),
                                ("path".to_string(), CtValue::Str("$".to_string())),
                                ("reason".to_string(), CtValue::Str(reason)),
                            ],
                        })),
                    });
                }
                // D-MIGRATE3=A: the text codecs' typed decode. `TExprKind::CoreCall`
                // carries no type arguments, so the target comes from the call's
                // resolved return type — `Result<T, [FieldError]>` for `decode`,
                // `Result<DecodeResult<T>, [FieldError]>` for `decode_traced`.
                if matches!(
                    module.as_str(),
                    "core.encoding.json"
                        | "core.encoding.toml"
                        | "core.encoding.yaml"
                        | "core.encoding.csv"
                ) && matches!(method.as_str(), "decode" | "decode_traced")
                {
                    return self.eval_typed_codec_decode(
                        module,
                        method,
                        &expr.ty,
                        &argv,
                        *source_span,
                    );
                }
                if module == "core.encoding.cbor" && method == "decode" {
                    let Type::Result { ok, .. } = &expr.ty else {
                        return Err(unsupported(
                            "core.encoding.cbor.decode resolved return type",
                            *source_span,
                        ));
                    };
                    let bytes = match argv.first() {
                        Some(CtValue::Bytes(bytes)) => bytes.clone(),
                        Some(CtValue::List(values)) => values
                            .iter()
                            .map(|value| match value {
                                CtValue::Int(byte) => u8::try_from(*byte).map_err(|_| {
                                    unsupported(
                                        "core.encoding.cbor.decode byte argument",
                                        *source_span,
                                    )
                                }),
                                _ => Err(unsupported(
                                    "core.encoding.cbor.decode byte argument",
                                    *source_span,
                                )),
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                        _ => {
                            return Err(unsupported(
                                "core.encoding.cbor.decode byte argument",
                                *source_span,
                            ))
                        }
                    };
                    let tree = match crate::Comptime::cbor_parse_for_tir(
                        &bytes,
                        argv.get(1),
                        true,
                    ) {
                        Ok(tree) => tree,
                        Err(error) => {
                            return Ok(CtValue::failed(Box::new(
                                crate::Comptime::cbor_decode_source_error_for_tir(error),
                            )))
                        }
                    };
                    return Ok(match self.eval_datatree_decode(tree, ok)? {
                        CtValue::Present(value) => CtValue::Present(value),
                        CtValue::Failed(CtReport::Told(error)) => CtValue::Failed(CtReport::Told(error)),
                        _ => unreachable!("Decode protocol returns Result"),
                    });
                }
                if module == "core.crypto"
                    && method == "__signing_generate"
                    && argv.is_empty()
                {
                    return Ok(CtValue::Present(Box::new(CtValue::Int(1))));
                }
                if module == "core.crypto"
                    && method == "__signing_public"
                    && argv.len() == 1
                {
                    return Ok(argv.remove(0));
                }
                if module == "core.browser" && self.runtime_execution {
                    return super::browser::core_call(method, argv, *source_span);
                }
                if !self.runtime_execution && module == "core.net" && method == "fetch" {
                    return crate::Comptime::eval_net_fetch(
                        &argv,
                        self.embed_inputs.as_deref_mut(),
                        *source_span,
                    );
                }
                if !self.runtime_execution && module == "core.vault" {
                    return Err(crate::Comptime::vault_comptime_denied(
                        module,
                        method,
                        *source_span,
                    ));
                }
                // #1788: `core.random` (besides `.rng`, a pure function of its
                // explicit seed argument) reads/writes ambient PRNG state — the
                // real runtime `Rand` when this evaluator is truly running the
                // program (`runtime_execution`) or a live REPL session
                // (`repl_mode`, which *is* the one execution), but a throwaway
                // interpreter-only stream otherwise. That "otherwise" is a
                // sema-time D-VERDICT-1308-1 implicit `::` fold or an explicit
                // `$`/#Known demand (same call path as the `mem.address_of`
                // guard above): baking its draw as a literal would freeze a
                // value that never resyncs with whatever `random.seed()` the
                // compiled program's Prelude RNG sees at real runtime. Decline
                // plainly so the fold backs off to ordinary runtime codegen
                // (D-VERDICT-1308-1: failure is silent); an explicit demand
                // surfaces this as a normal "not available at compile time"
                // error. Do not route through the Tier-2 `#Impure` gate below —
                // random stays outside that gate (D-META-EFFECT1).
                if !self.runtime_execution
                    && !self.repl_mode
                    && module == "core.random"
                    && method != "rng"
                {
                    return Err(unsupported(
                        &format!("`{module}.{method}()` at compile time"),
                        *source_span,
                    ));
                }
                if self.should_decline_ambient_fold(module, method) {
                    return Err(unsupported(
                        &format!("`{module}.{method}()` at compile time"),
                        *source_span,
                    ));
                }
                let is_tier2 =
                    crate::Comptime::is_tier2_core_call(module, method, self.repl_mode);
                if !is_tier2 {
                    return apply_core_call(module, method, argv, *source_span, self.repl_mode)
                        .map(|value| {
                            mark_unknown_progress_total(
                                value,
                                module,
                                method,
                                args,
                                progress_known_total,
                            )
                        });
                }
                // Runtime deopt / `jet run` sets impure_depth>0 so Tier-2
                // ambient I/O matches AOT (env/fs/process/auth store). Pure
                // comptime keeps depth 0 and must reject — never fall through
                // to `apply_core_call`, which still hosts AuthLite/SyncLite
                // and would const-fold storeful Ok(literals) (I9).
                if self.repl_mode {
                    let mut sink = self
                        .sink
                        .as_ref()
                        .map(|sink| sink.lock().expect("evaluator sink poisoned"));
                    apply_repl_authorized_core_call(
                        module,
                        method,
                        argv,
                        *source_span,
                        &self.base_dir,
                        sink.as_deref_mut(),
                        &self.repl_grants,
                        reborrow_repl_authorizer(&mut self.repl_authorizer),
                    )
                    .map(|value| {
                        mark_unknown_progress_total(
                            value,
                            module,
                            method,
                            args,
                            progress_known_total,
                        )
                    })
                } else if self.impure_depth > 0 && self.allow_impure {
                    let mut sink = self
                        .sink
                        .as_ref()
                        .map(|sink| sink.lock().expect("evaluator sink poisoned"));
                    let result = apply_impure_core_call(
                        module,
                        method,
                        argv,
                        *source_span,
                        &self.base_dir,
                        sink.as_deref_mut(),
                        false,
                        None,
                        None,
                    );
                    result.map(|value| {
                        mark_unknown_progress_total(
                            value,
                            module,
                            method,
                            args,
                            progress_known_total,
                        )
                    })
                } else if self.impure_depth == 0 {
                    Err(Diagnostic::error(
                        "E3410",
                        format!(
                            "`{module}.{method}()` is a Tier-2 comptime effect — it requires a `#Impure` gate"
                        ),
                        "ambient I/O and storeful Core APIs are not allowed in \
                         pure comptime evaluation"
                            .to_string(),
                        "wrap the comptime binding in `#Impure(\"reason\") { … }` and \
                         pass `--allow-impure` to the build, or keep the call at runtime"
                            .to_string(),
                        Some(*source_span),
                    ))
                } else {
                    Err(Diagnostic::error(
                        "E3411",
                        format!(
                            "`{module}.{method}()` inside `#Impure` gate, but `--allow-impure` was not passed"
                        ),
                        "the `#Impure` block opts in to ambient comptime I/O, but the build flag is required so CI can audit builds that touch the host".to_string(),
                        "add `--allow-impure` to your `jet build` / `jet run` invocation".to_string(),
                        Some(*source_span),
                    ))
                }
            }
            TExprKind::StructLit {
                fields, as_trait, ..
            } => {
                let mut out = Vec::with_capacity(fields.len());
                for (name, val, _) in fields {
                    out.push((name.clone(), self.eval_expr(val, scope)?));
                }
                let type_name = as_trait
                    .as_ref()
                    .map(|(_, concrete)| concrete.clone())
                    .unwrap_or_else(|| match &expr.ty {
                        crate::AST::Type::Named(n) => n.clone(),
                        crate::AST::Type::Apply { name, .. } => name.clone(),
                        _ => "struct".into(),
                    });
                Ok(CtValue::Struct {
                    type_name,
                    fields: out,
                })
            }
            TExprKind::Field { recv, field, .. } => {
                let r = self.eval_expr(recv, scope)?;
                // D-LAYOUT-FACTS1=B: `$layout` is a contextual projection of
                // the TypeInfo value bound to a derive type parameter. It is
                // not a second stored TypeInfo member; ordinary `.layout`
                // remains the full-reflection projection.
                if let Some(projected) = crate::Syntax::compiler_fact_member(field) {
                    let CtValue::Struct { type_name, fields } = r else {
                        return Err(Diagnostic::error(
                            "E0302",
                            format!("`{field}` needs a reflected type value"),
                            "compiler facts attach to the type parameter in a derive body"
                                .to_string(),
                            format!("use `T.{field}`, or `T.reflect().{projected}` for full reflection"),
                            Some(self.span()),
                        ));
                    };
                    if type_name != crate::Syntax::TYPE_TYPE_INFO {
                        return Err(Diagnostic::error(
                            "E0302",
                            format!("`{field}` needs a reflected type value"),
                            "compiler facts attach to the type parameter in a derive body"
                                .to_string(),
                            format!("use `T.{field}`, or `T.reflect().{projected}` for full reflection"),
                            Some(self.span()),
                        ));
                    }
                    return fields
                        .into_iter()
                        .find(|(name, _)| name == projected)
                        .map(|(_, value)| value)
                        .ok_or_else(|| {
                            Diagnostic::error(
                                "E0302",
                                format!("the reflected type has no `{field}` fact"),
                                "the compiler fact projection is fixed by D-LAYOUT-FACTS1"
                                    .to_string(),
                                format!("use `T.reflect().{projected}` for the full reflection object"),
                                Some(self.span()),
                            )
                        });
                }
                // D-LAYOUT-FACTS1=B: `LayoutInfo[.field]` is lowered as an
                // ordinary field read with an internal projection name. The
                // value still comes from the one reflected `fields` list, so
                // source and `jet inspect expand` cannot drift.
                if let Some(selected) = field
                    .strip_prefix(crate::Syntax::LAYOUT_FIELD_PROJECTION_PREFIX)
                {
                    let CtValue::Struct { type_name, fields } = r else {
                        return Err(Diagnostic::error(
                            "E0302",
                            "layout field selector needs a `LayoutInfo` value".to_string(),
                            "typed field selection is only defined on compiler layout facts"
                                .to_string(),
                            "use `T.$layout[.field]` for a reflected field fact".to_string(),
                            Some(self.span()),
                        ));
                    };
                    if type_name != crate::Syntax::TYPE_LAYOUT_INFO {
                        return Err(Diagnostic::error(
                            "E0302",
                            "layout field selector needs a `LayoutInfo` value".to_string(),
                            "typed field selection is only defined on compiler layout facts"
                                .to_string(),
                            "use `T.$layout[.field]` for a reflected field fact".to_string(),
                            Some(self.span()),
                        ));
                    }
                    let Some(CtValue::List(layout_fields)) = fields
                        .into_iter()
                        .find(|(name, _)| name == "fields")
                        .map(|(_, value)| value)
                    else {
                        return Err(Diagnostic::error(
                            "E0302",
                            "the reflected layout has no field facts".to_string(),
                            "typed selectors read the canonical `LayoutInfo.fields` list"
                                .to_string(),
                            "use `T.reflect().layout.fields` for dynamic field iteration"
                                .to_string(),
                            Some(self.span()),
                        ));
                    };
                    return layout_fields
                        .into_iter()
                        .find(|value| {
                            matches!(
                                value,
                                CtValue::Struct { type_name, fields }
                                    if type_name == crate::Syntax::TYPE_LAYOUT_FIELD
                                        && fields.iter().any(|(name, value)| {
                                            name == "name"
                                                && matches!(value, CtValue::Str(value) if value == selected)
                                        })
                            )
                        })
                        .ok_or_else(|| {
                            Diagnostic::error(
                                "E0302",
                                format!("the reflected layout has no field `{selected}`"),
                                "typed selectors must name a field declared by the reflected type"
                                    .to_string(),
                                "use one of the names in `T.reflect().fields`".to_string(),
                                Some(self.span()),
                            )
                        });
                }
                match r {
                    // TupleLit stores Rust-mangled `user_<f>` names (emit needs them);
                    // Field TIR keeps Jet names. Accept either so named-tuple reads work.
                    CtValue::Struct {
                        type_name,
                        fields,
                    } if type_name == "__JetViewMut" => {
                        let window =
                            materialize_view_mut_window(&fields, scope, self.span())?;
                        let CtValue::List(xs) = window else {
                            return Err(unsupported("view-mut window", self.span()));
                        };
                        if xs.len() != 1 {
                            return Err(unsupported("field on multi-element view", self.span()));
                        }
                        match &xs[0] {
                            CtValue::Struct { fields, .. } => {
                                let mangled = crate::Codegen::mangle(field);
                                fields
                                    .iter()
                                    .find(|(n, _)| {
                                        n == field
                                            || n == &mangled
                                            || n.strip_prefix("user_") == Some(field.as_str())
                                    })
                                    .map(|(_, v)| v.clone())
                                    .ok_or_else(|| {
                                        unsupported(&format!("field `{field}`"), self.span())
                                    })
                            }
                            _ => Err(unsupported("field recv", self.span())),
                        }
                    }
                    CtValue::Struct { fields, .. } => {
                        let mangled = crate::Codegen::mangle(field);
                        fields
                            .into_iter()
                            .find(|(n, _)| {
                                n == field
                                    || n == &mangled
                                    || n.strip_prefix("user_") == Some(field.as_str())
                            })
                            .map(|(_, v)| v)
                            .ok_or_else(|| unsupported(&format!("field `{field}`"), self.span()))
                    }
                    _ => Err(unsupported("field recv", self.span())),
                }
            }
            TExprKind::ListLit(elems) => {
                if let [inner] = elems.as_slice() {
                    // Early typed-list lowering wraps `T.{ value }` in one
                    // ListLit. Preserve the value when it already has T.
                    if expr.ty == inner.ty {
                        return self.eval_expr(inner, scope);
                    }
                }
                let mut out = Vec::with_capacity(elems.len());
                for e in elems {
                    out.push(self.eval_expr(e, scope)?);
                }
                Ok(CtValue::List(out))
            }
            TExprKind::Clone(inner) => self.eval_expr(inner, scope),
            TExprKind::Present(inner) => {
                Ok(CtValue::Present(Box::new(self.eval_expr(inner, scope)?)))
            }
            TExprKind::Absent => Ok(CtValue::absent(expr.ty.clone())),
            TExprKind::Ok(inner) => Ok(CtValue::Present(Box::new(self.eval_expr(inner, scope)?))),
            TExprKind::Err(inner) => Ok(CtValue::failed(Box::new(self.eval_expr(inner, scope)?))),
            TExprKind::TupleLit { fields, .. } => {
                let mut out = Vec::with_capacity(fields.len());
                for (name, e) in fields {
                    out.push((name.clone(), self.eval_expr(e, scope)?));
                }
                Ok(CtValue::Struct {
                    type_name: "tuple".into(),
                    fields: out,
                })
            }
            TExprKind::MapLit(entries) => {
                let mut m = std::collections::BTreeMap::new();
                for (k, v) in entries {
                    let key = crate::AST::CtKey::from_value(self.eval_expr(k, scope)?)
                        .ok_or_else(|| unsupported("map key", self.span()))?;
                    m.insert(key, self.eval_expr(v, scope)?);
                }
                Ok(CtValue::Map(m))
            }
            TExprKind::Index {
                base,
                index,
                is_map,
                ..
            } => {
                let b = match self.eval_expr(base, scope)? {
                    CtValue::Present(inner) => *inner,
                    other => other,
                };
                let i = self.eval_expr(index, scope)?;
                if *is_map || matches!(&b, CtValue::Map(_)) {
                    let key = crate::AST::CtKey::from_value(i)
                        .ok_or_else(|| unsupported("map index key", self.span()))?;
                    match b {
                        CtValue::Map(m) => m
                            .get(&key)
                            .cloned()
                            .ok_or_else(|| unsupported("missing map key", self.span())),
                        _ => Err(unsupported("map index recv", self.span())),
                    }
                } else {
                    let idx = as_int(&i, self.span())?;
                    // Mutable place-window (`__JetViewMut`) — index into the owner.
                    if let CtValue::Struct {
                        type_name,
                        fields,
                    } = &b
                    {
                        if type_name == "__JetViewMut" {
                            let window =
                                materialize_view_mut_window(fields, scope, self.span())?;
                            let CtValue::List(xs) = window else {
                                return Err(unsupported("view-mut window", self.span()));
                            };
                            if idx < 0 || idx as usize >= xs.len() {
                                return Err(unsupported("list index oob", self.span()));
                            }
                            return Ok(xs[idx as usize].clone());
                        }
                    }
                    match b {
                        ref value @ CtValue::Struct { ref type_name, .. }
                            if type_name == super::UNINIT_FIXED_CARRIER =>
                        {
                            super::uninit_fixed_read(
                                value,
                                usize::try_from(idx).map_err(|_| {
                                    unsupported("negative uninit index", self.span())
                                })?,
                            )
                            .ok_or_else(|| {
                                unsupported("uninit fixed-list index", self.span())
                            })
                        }
                        CtValue::List(xs) => {
                            if idx < 0 || idx as usize >= xs.len() {
                                Err(unsupported("list index oob", self.span()))
                            } else {
                                Ok(xs[idx as usize].clone())
                            }
                        }
                        CtValue::Bytes(bs) => {
                            if idx < 0 || idx as usize >= bs.len() {
                                Err(unsupported("bytes index oob", self.span()))
                            } else {
                                Ok(CtValue::Int(bs[idx as usize] as i64))
                            }
                        }
                        CtValue::Str(s) => {
                            let ch = s
                                .chars()
                                .nth(idx as usize)
                                .ok_or_else(|| unsupported("string index oob", self.span()))?;
                            Ok(CtValue::Char(ch))
                        }
                        other => {
                            if let Some(r) =
                                crate::Comptime::MathLayout::lane_at(&other, idx, self.span())
                            {
                                r
                            } else {
                                Err(unsupported("index recv", self.span()))
                            }
                        }
                    }
                }
            }
            TExprKind::Slice {
                base, start, end, range, ..
            } => {
                let b = self.eval_expr(base, scope)?;
                let (a, z, exclusive) = if let Some(range) = range {
                    let value = self.eval_expr(range, scope)?;
                    let CtValue::Struct { type_name, fields } = value else {
                        return Err(unsupported("Range slice", self.span()));
                    };
                    if type_name != crate::Syntax::TYPE_RANGE {
                        return Err(unsupported("Range slice type", self.span()));
                    }
                    let field = |name: &str| {
                        fields.iter().find(|(field, _)| field == name).map(|(_, value)| value)
                    };
                    (
                        field("start")
                            .ok_or_else(|| unsupported("Range.start", self.span()))
                            .and_then(|value| as_int(value, self.span()))?,
                        field("end")
                            .ok_or_else(|| unsupported("Range.end", self.span()))
                            .and_then(|value| as_int(value, self.span()))?,
                        matches!(field("exclusive"), Some(CtValue::Bool(true))),
                    )
                } else {
                    (
                        as_int(&self.eval_expr(start, scope)?, self.span())?,
                        as_int(&self.eval_expr(end, scope)?, self.span())?,
                        false,
                    )
                };
                match &b {
                    CtValue::List(xs) => {
                        let end_valid = if exclusive {
                            z as usize <= xs.len()
                        } else {
                            (z as usize) < xs.len()
                        };
                        if a < 0 || z < a || !end_valid {
                            Err(unsupported("slice bounds", self.span()))
                        } else if exclusive {
                            Ok(CtValue::List(xs[a as usize..z as usize].to_vec()))
                        } else {
                            Ok(CtValue::List(xs[a as usize..=z as usize].to_vec()))
                        }
                    }
                    CtValue::Str(s) => {
                        let chars: Vec<char> = s.chars().collect();
                        let end_valid = if exclusive {
                            z as usize <= chars.len()
                        } else {
                            (z as usize) < chars.len()
                        };
                        if a < 0 || z < a || !end_valid {
                            Err(unsupported("slice bounds", self.span()))
                        } else if exclusive {
                            Ok(CtValue::Str(chars[a as usize..z as usize].iter().collect()))
                        } else {
                            Ok(CtValue::Str(
                                chars[a as usize..=z as usize].iter().collect(),
                            ))
                        }
                    }
                    CtValue::Struct { type_name, .. }
                        if type_name == "Tensor" || type_name == "JetTensor" =>
                    {
                        crate::Comptime::ComputeLite::tensor_slice_value(
                            &b,
                            a,
                            z,
                            exclusive,
                            self.span(),
                        )
                    }
                    _ => Err(unsupported("slice recv", self.span())),
                }
            }
            TExprKind::Borrow { place, .. } => self.eval_expr(place, scope),
            TExprKind::MaterializeView(inner) => self.eval_expr(inner, scope),
            TExprKind::MethodCall {
                recv,
                method,
                args,
                source_first_string_literal,
                ..
            } => {
                let mut r = self.eval_expr(recv, scope)?;
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(&a.value, scope)?);
                }
                if method.name == "clone" {
                    return Ok(r);
                }
                if method.name == "apply" {
                    if let (
                        CtValue::Struct {
                            type_name,
                            fields,
                        },
                        Some(CtValue::Struct {
                            type_name: patch_name,
                            fields: patch_fields,
                        }),
                    ) = (&r, argv.first())
                    {
                        if patch_name == &format!("{type_name}.Patch") {
                            let fields = fields
                                .iter()
                                .map(|(name, old)| {
                                    let value = patch_fields
                                        .iter()
                                        .find_map(|(patch_name, value)| {
                                            (patch_name == name).then_some(value)
                                        })
                                        .and_then(|value| match value {
                                            CtValue::Present(value) => Some((**value).clone()),
                                            _ => None,
                                        })
                                        .unwrap_or_else(|| old.clone());
                                    (name.clone(), value)
                                })
                                .collect();
                            return Ok(CtValue::Struct {
                                type_name: type_name.clone(),
                                fields,
                            });
                        }
                    }
                }
                if method.name == "merge" {
                    if let (
                        CtValue::Struct {
                            type_name,
                            fields,
                        },
                        Some(CtValue::Struct {
                            type_name: other_name,
                            fields: other_fields,
                        }),
                    ) = (&r, argv.first())
                    {
                        if type_name.ends_with(".Patch") && type_name == other_name {
                            let fields = fields
                                .iter()
                                .map(|(name, current)| {
                                    let incoming = other_fields
                                        .iter()
                                        .find_map(|(other_name, value)| {
                                            (other_name == name).then_some(value)
                                        })
                                        .filter(|value| matches!(value, CtValue::Present(_)))
                                        .cloned()
                                        .unwrap_or_else(|| current.clone());
                                    (name.clone(), incoming)
                                })
                                .collect();
                            return Ok(CtValue::Struct {
                                type_name: type_name.clone(),
                                fields,
                            });
                        }
                    }
                }
                let span = self.span();
                let base_dir = self.base_dir.clone();
                if let Some(result) =
                    crate::Comptime::Build::eval_program_build_input_method(
                        &r,
                        &method.name,
                        &argv,
                        source_first_string_literal.as_deref(),
                        &base_dir,
                        self.embed_inputs.as_deref_mut(),
                        span,
                    )
                {
                    return result;
                }
                if let Some(result) = crate::Comptime::Build::eval_program_build_method(
                    &r,
                    &method.name,
                    argv.clone(),
                    self.span(),
                    self.impure_depth > 0,
                ) {
                    return result;
                }
                const MUTATING: &[&str] = &[
                    "push", "pop", "add", "add_new", "insert", "remove", "extend", "clear", "reverse",
                    "sort", "tick", "advance", "wait", "int", "float", "float_range", "bool",
                    "normal", "exponential", "bytes", "split", "pick", "weighted_pick", "sample",
                    "shuffle", "require",
                ];
                let try_mutating = MUTATING.contains(&method.name.as_str())
                    || matches!(
                        &r,
                        CtValue::Struct { type_name, .. }
                            if type_name == crate::Syntax::CLOCK_TYPE
                                || type_name == crate::Syntax::RNG_TYPE
                                || type_name == crate::Syntax::SOLVER_TYPE
                                || (type_name == crate::Syntax::MEM_POOL
                                    && matches!(method.name.as_str(), "add" | "remove"))
                    );
                // Mutating dispatch first — `apply_method` for Pool.add returns the
                // Id but drops the updated arena (write-back lives in apply_mutating).
                if try_mutating {
                    if method.name == "shuffle" {
                        if let CtValue::Struct { type_name, fields } = &r {
                            if type_name == crate::Syntax::RNG_TYPE {
                                let mut state = fields
                                    .iter()
                                    .find_map(|(name, value)| match (name.as_str(), value) {
                                        ("state", CtValue::Int(state)) => Some(*state as u64),
                                        _ => None,
                                    })
                                    .unwrap_or(0);
                                let ret = crate::Comptime::apply_seeded_rng_method(
                                    &mut state,
                                    "shuffle",
                                    &mut argv,
                                    self.span(),
                                )?;
                                r = CtValue::Struct {
                                    type_name: crate::Syntax::RNG_TYPE.to_string(),
                                    fields: vec![("state".to_string(), CtValue::Int(state as i64))],
                                };
                                self.write_back_place(recv, r, scope)?;
                                for (a, v) in args.iter().zip(argv.into_iter()) {
                                    let place_like = matches!(
                                        &a.value.kind,
                                        TExprKind::Borrow { .. }
                                            | TExprKind::Local(_)
                                            | TExprKind::Field { .. }
                                    );
                                    if place_like {
                                        self.write_back_place(&a.value, v, scope)?;
                                    }
                                }
                                return Ok(ret);
                            }
                        }
                    }
                    if let Ok(ret) = crate::Comptime::Builtins::apply_mutating(
                        &mut r,
                        &method.name,
                        argv.clone(),
                        self.span(),
                    ) {
                        self.write_back_place(recv, r, scope)?;
                        return Ok(ret);
                    }
                }
                if method.name == "compare" {
                    if let (CtValue::Int(lhs), [CtValue::Int(rhs)]) = (&r, argv.as_slice()) {
                        let variant = match lhs.cmp(rhs) {
                            std::cmp::Ordering::Less => "Less",
                            std::cmp::Ordering::Equal => "Equal",
                            std::cmp::Ordering::Greater => "Greater",
                        };
                        return Ok(CtValue::Enum {
                            type_name: "Ordering".to_string(),
                            variant: variant.to_string(),
                            args: Vec::new(),
                        });
                    }
                }
                if let Ok(v) = crate::Comptime::Builtins::apply_method(
                    &r,
                    &method.name,
                    argv.clone(),
                    self.span(),
                ) {
                    return Ok(v);
                }
                let mut names = vec![method.name.clone()];
                if method.mangled {
                    names.push(format!("user_{}", method.name));
                }
                if let Type::Named(type_name) = &recv.ty {
                    names.push(format!("{type_name}::{}", method.name));
                }
                if let CtValue::Struct { type_name, .. } = &r {
                    names.push(format!("{type_name}::{}", method.name));
                }
                for name in names {
                    if let Some(func) = self.funcs.get(&name).copied() {
                        let mut child = HashMap::new();
                        // Instance methods lower `self` into the env, not `params`.
                        let has_receiver = matches!(
                            &func.kind,
                            crate::Codegen::TIR::TFuncKind::Method {
                                self_conv: Some(_),
                                ..
                            }
                                | crate::Codegen::TIR::TFuncKind::TraitMethod { .. }
                        );
                        let argv_for_params = if has_receiver {
                            child.insert("self".to_string(), r.clone());
                            argv
                        } else {
                            let mut full = vec![r.clone()];
                            full.extend(argv);
                            full
                        };
                        let result = self.run_func(func, argv_for_params, &mut child)?;
                        if matches!(
                            &func.kind,
                            crate::Codegen::TIR::TFuncKind::Method {
                                self_conv: Some(crate::AST::AccessConvention::Write),
                                ..
                            }
                        ) {
                            if let Some(updated) = child.get("self") {
                                self.write_back_place(recv, updated.clone(), scope)?;
                            }
                        }
                        return Ok(result);
                    }
                }
                Err(unsupported(
                    &format!("method `{}`", method.name),
                    self.span(),
                ))
            }
            TExprKind::Try {
                inner,
                convert: _,
                file,
                line,
                fn_name,
            } => {
                let v = self.eval_expr(inner, scope)?;
                match v {
                    CtValue::Present(inner) => Ok(*inner),
                    CtValue::Failed(CtReport::Told(e)) => {
                        // D-ERRCTX1: match AOT `jet_trace_err` / JIT host (dev builds).
                        let file = file.trim_matches('"');
                        let fn_name = fn_name.trim_matches('"');
                        let frame = format!(
                            "error propagated from: {fn_name} ({file}:{line}) via ?\n"
                        );
                        if let Some(sink) = self.sink.as_ref() {
                            let mut sink = sink.lock().expect("evaluator sink poisoned");
                            let skip = sink
                                .stderr
                                .ends_with(&frame);
                            if !skip {
                                sink.stderr.push_str(&frame);
                            }
                        }
                        // Propagate as a function return of the error value.
                        self.pending_return = Some(CtValue::failed(e));
                        Ok(CtValue::Unit)
                    }
                    CtValue::Failed(CtReport::Clean(_)) => {
                        self.pending_return = Some(CtValue::absent(crate::AST::Type::Int));
                        Ok(CtValue::Unit)
                    }
                    other => Ok(other),
                }
            }
            TExprKind::OrFallback { value, fallback } => {
                let v = self.eval_expr(value, scope)?;
                // D-FAIL-CARRIER1=A: one carrier — the report side is the miss,
                // whether the report is a clean absence or a failure.
                let miss = matches!(v, CtValue::Failed(CtReport::Clean(_)) | CtValue::Failed(CtReport::Told(_)));
                if !miss {
                    return match v {
                        CtValue::Present(inner) => Ok(*inner),
                        other => Ok(other),
                    };
                }
                match fallback {
                    crate::Codegen::TIR::TOrFallback::Value(fb) => self.eval_expr(fb, scope),
                    crate::Codegen::TIR::TOrFallback::Return(Some(fb)) => {
                        let ret = self.eval_expr(fb, scope)?;
                        self.pending_return = Some(ret);
                        Ok(CtValue::Unit)
                    }
                    crate::Codegen::TIR::TOrFallback::Return(None) => {
                        self.pending_return = Some(CtValue::Unit);
                        Ok(CtValue::Unit)
                    }
                    crate::Codegen::TIR::TOrFallback::Panic { msg, loc } => {
                        let message = self.eval_expr(msg, scope)?.jet_show();
                        let file = loc.file.trim_matches('"');
                        let fn_name = loc.fn_name.trim_matches('"');
                        let src_line = loc.src_line.trim_matches('"');
                        let line_s = loc.line.to_string();
                        let margin = line_s.len();
                        let pad = " ".repeat(margin);
                        let col_offset = loc.col.saturating_sub(1) as usize;
                        let caret = "^".repeat(loc.caret.max(1) as usize);
                        let rendered = format!(
                            "panic: {message}\n  --> {file}:{} in {fn_name}\n   {pad}|\n{line_s} | {src_line}\n   {pad}| {}{caret}\n",
                            loc.line,
                            " ".repeat(col_offset)
                        );
                        if let Some(sink) = self.sink.as_ref() {
                            let mut sink = sink.lock().expect("evaluator sink poisoned");
                            sink.stderr.push_str(&rendered);
                            sink.exit_code = Some(70);
                            return Err(Diagnostic::error(
                                "SOFT_EXIT",
                                "70".to_string(),
                                "or-fallback panic stop".to_string(),
                                String::new(),
                                Some(self.span()),
                            ));
                        }
                        Err(unsupported("or-fallback panic", self.span()))
                    }
                    _ => Err(unsupported("or-fallback form", self.span())),
                }
            }
            TExprKind::EnumLit {
                enum_type,
                variant,
                payload,
            } => {
                // Positional payloads keep `label: None` so `jet_show` matches
                // AOT `user_Wrap(user_Num(1))` Debug shape (I2 / #777).
                let args = match payload {
                    crate::Codegen::TIR::TEnumPayload::Unit => Vec::new(),
                    crate::Codegen::TIR::TEnumPayload::Positional(pos) => {
                        let mut out = Vec::with_capacity(pos.len());
                        for a in pos {
                            out.push((None, self.eval_expr(&a.value, scope)?));
                        }
                        out
                    }
                    crate::Codegen::TIR::TEnumPayload::Named(named) => {
                        let mut out = Vec::with_capacity(named.len());
                        for (name, a) in named {
                            out.push((Some(name.clone()), self.eval_expr(&a.value, scope)?));
                        }
                        out
                    }
                };
                Ok(CtValue::Enum {
                    type_name: enum_type.clone(),
                    variant: variant.clone(),
                    args,
                })
            }
            TExprKind::HostCall(host) => match host.as_ref() {
                crate::Codegen::TIR::THostCall::ExpectSnapshot { value, .. } => {
                    // Comptime/transcript: evaluate the wrapped value; snapshot I/O
                    // is an AOT harness concern.
                    let _ = self.eval_expr(value, scope)?;
                    Ok(CtValue::Unit)
                }
                crate::Codegen::TIR::THostCall::NumericBounds { ty, member } => {
                    use crate::AST::Type;
                    match (ty, member.as_str()) {
                        (Type::Float32, "MAX") => {
                            Ok(CtValue::Float(CtFloat::literal(f32::MAX as f64, true)))
                        }
                        (Type::Float32, "MIN") => {
                            Ok(CtValue::Float(CtFloat::literal(f32::MIN as f64, true)))
                        }
                        (Type::Float32, "NAN") => {
                            Ok(CtValue::Float(CtFloat::literal(f32::NAN as f64, true)))
                        }
                        (Type::Float32, "INFINITY") => {
                            Ok(CtValue::Float(CtFloat::literal(f32::INFINITY as f64, true)))
                        }
                        (Type::Float32, "NEG_INFINITY") => Ok(CtValue::Float(CtFloat::literal(
                            f32::NEG_INFINITY as f64,
                            true,
                        ))),
                        (Type::Float32, "EPSILON") => {
                            Ok(CtValue::Float(CtFloat::literal(f32::EPSILON as f64, true)))
                        }
                        (Type::Float, "MAX") => {
                            Ok(CtValue::Float(CtFloat::literal(f64::MAX, false)))
                        }
                        (Type::Float, "MIN") => {
                            Ok(CtValue::Float(CtFloat::literal(f64::MIN, false)))
                        }
                        (Type::Float, "NAN") => {
                            Ok(CtValue::Float(CtFloat::literal(f64::NAN, false)))
                        }
                        (Type::Float, "INFINITY") => {
                            Ok(CtValue::Float(CtFloat::literal(f64::INFINITY, false)))
                        }
                        (Type::Float, "NEG_INFINITY") => {
                            Ok(CtValue::Float(CtFloat::literal(f64::NEG_INFINITY, false)))
                        }
                        (Type::Float, "EPSILON") => {
                            Ok(CtValue::Float(CtFloat::literal(f64::EPSILON, false)))
                        }
                        (Type::Int, "MAX") => Ok(CtValue::Int(i64::MAX)),
                        (Type::Int, "MIN") => Ok(CtValue::Int(i64::MIN)),
                        (Type::IntN { signed: false, bits: 8 }, "MAX") => {
                            Ok(CtValue::Int(u8::MAX as i64))
                        }
                        (Type::IntN { signed: false, bits: 8 }, "MIN") => Ok(CtValue::Int(0)),
                        (Type::IntN { signed: true, bits: 8 }, "MAX") => {
                            Ok(CtValue::Int(i8::MAX as i64))
                        }
                        (Type::IntN { signed: true, bits: 8 }, "MIN") => {
                            Ok(CtValue::Int(i8::MIN as i64))
                        }
                        (Type::IntN { signed: false, bits: 16 }, "MAX") => {
                            Ok(CtValue::Int(u16::MAX as i64))
                        }
                        (Type::IntN { signed: false, bits: 16 }, "MIN") => Ok(CtValue::Int(0)),
                        (Type::IntN { signed: true, bits: 16 }, "MAX") => {
                            Ok(CtValue::Int(i16::MAX as i64))
                        }
                        (Type::IntN { signed: true, bits: 16 }, "MIN") => {
                            Ok(CtValue::Int(i16::MIN as i64))
                        }
                        (Type::IntN { signed: false, bits: 32 }, "MAX") => {
                            Ok(CtValue::Int(u32::MAX as i64))
                        }
                        (Type::IntN { signed: false, bits: 32 }, "MIN") => Ok(CtValue::Int(0)),
                        (Type::IntN { signed: true, bits: 32 }, "MAX") => {
                            Ok(CtValue::Int(i32::MAX as i64))
                        }
                        (Type::IntN { signed: true, bits: 32 }, "MIN") => {
                            Ok(CtValue::Int(i32::MIN as i64))
                        }
                        (Type::IntN { signed, bits }, "MAX") => Ok(CtValue::Int(
                            crate::Comptime::MathLayout::integer_bound(*signed, *bits, true),
                        )),
                        (Type::IntN { signed, bits }, "MIN") => Ok(CtValue::Int(
                            crate::Comptime::MathLayout::integer_bound(*signed, *bits, false),
                        )),
                        _ => Err(unsupported(
                            &format!("numeric bounds `{member}`"),
                            self.span(),
                        )),
                    }
                }
                crate::Codegen::TIR::THostCall::FixedListIndex { base, index } => {
                    let b = self.eval_expr(base, scope)?;
                    let idx = as_int(&self.eval_expr(index, scope)?, self.span())?;
                    match b {
                        CtValue::List(xs) => {
                            if idx < 0 || idx as usize >= xs.len() {
                                Err(unsupported("fixed-list index oob", self.span()))
                            } else {
                                Ok(xs[idx as usize].clone())
                            }
                        }
                        other => {
                            if let Some(r) =
                                crate::Comptime::MathLayout::lane_at(&other, idx, self.span())
                            {
                                r
                            } else {
                                Err(unsupported("fixed-list index recv", self.span()))
                            }
                        }
                    }
                }
                crate::Codegen::TIR::THostCall::TypedText { kind, arg } => {
                    use crate::Codegen::TIR::TTypedTextForm;
                    let value = self.eval_expr(arg, scope)?;
                    match kind {
                        TTypedTextForm::SQLRaw => match value {
                            CtValue::Str(template) => {
                                let (template, params) = crate::typed_text::jet_typed_sql_raw(template);
                                Ok(typed_sql_value(template, params.into_iter().map(CtValue::Str).collect()))
                            }
                            _ => Err(unsupported("SQL.raw expects String", self.span())),
                        },
                        TTypedTextForm::HTMLRaw => match value {
                            CtValue::Str(value) => Ok(CtValue::Str(crate::typed_text::jet_typed_html_raw(value))),
                            _ => Err(unsupported("HTML value expects String", self.span())),
                        },
                        TTypedTextForm::HTMLText => match value {
                            CtValue::Str(value) => Ok(CtValue::Str(crate::typed_text::jet_typed_html_text(value))),
                            _ => Err(unsupported("HTML value expects String", self.span())),
                        },
                        TTypedTextForm::ShRaw => match value {
                            CtValue::Str(text) => Ok(CtValue::List(
                                crate::typed_text::jet_typed_sh_raw(text)
                                    .into_iter()
                                    .map(CtValue::Str)
                                    .collect(),
                            )),
                            _ => Err(unsupported("Sh.raw expects String", self.span())),
                        },
                        TTypedTextForm::SQLTemplate => {
                            Ok(CtValue::Str(typed_sql_parts(&value, self.span())?.0))
                        }
                        TTypedTextForm::SQLParams => {
                            Ok(CtValue::List(typed_sql_parts(&value, self.span())?.1))
                        }
                    }
                }
                crate::Codegen::TIR::THostCall::TypedTextInterp {
                    kind,
                    literals,
                    holes,
                } => {
                    use crate::Codegen::TIR::TTypedTextInterpKind;
                    let mut values = Vec::with_capacity(holes.len());
                    for hole in holes {
                        values.push(self.eval_expr(hole, scope)?);
                    }
                    match kind {
                        TTypedTextInterpKind::SQL => {
                            let literal_refs = literals.iter().map(String::as_str).collect::<Vec<_>>();
                            let (template, params) = crate::typed_text::jet_typed_sql_interpolate(
                                &literal_refs,
                                values.into_iter().map(|value| value.jet_show()).collect(),
                            );
                            Ok(typed_sql_value(
                                template,
                                params.into_iter().map(CtValue::Str).collect(),
                            ))
                        }
                        TTypedTextInterpKind::Sh => {
                            let literal_refs = literals.iter().map(String::as_str).collect::<Vec<_>>();
                            let argv = crate::typed_text::jet_typed_sh_interpolate(
                                &literal_refs,
                                values.into_iter().map(|value| value.jet_show()).collect(),
                            );
                            Ok(CtValue::List(argv.into_iter().map(CtValue::Str).collect()))
                        }
                        TTypedTextInterpKind::HTML => {
                            let literal_refs = literals.iter().map(String::as_str).collect::<Vec<_>>();
                            Ok(CtValue::Str(crate::typed_text::jet_typed_html_interpolate(
                                &literal_refs,
                                values.into_iter().map(|value| value.jet_show()).collect(),
                            )))
                        }
                    }
                }
                crate::Codegen::TIR::THostCall::SwitchSubjectField { field } => {
                    let CtValue::Struct { fields, .. } = self
                        .switch_subject
                        .as_ref()
                        .ok_or_else(|| unsupported("switch subject field outside switch", self.span()))?
                    else {
                        return Err(unsupported("switch subject is not a struct", self.span()));
                    };
                    fields
                        .iter()
                        .find_map(|(name, value)| (name == field).then(|| value.clone()))
                        .ok_or_else(|| unsupported(&format!("switch subject field `{field}`"), self.span()))
                }
                crate::Codegen::TIR::THostCall::SwitchSubjectValue => self
                    .switch_subject
                    .clone()
                    .ok_or_else(|| unsupported("switch subject", self.span())),
                crate::Codegen::TIR::THostCall::CellGuardProject {
                    recv,
                    paths,
                    result_ty: _,
                    editable,
                    edit_paths_disjoint,
                } => self.eval_cell_guard_project(
                    recv,
                    paths,
                    *editable,
                    *edit_paths_disjoint,
                    scope,
                ),
                // D-FAIL-CARRIER1=A: marshalling only. The interpreter supplies
                // the projection onto the report and calls the very same
                // prelude reader every other tier calls, so what a success and
                // a failure each answer is decided in one place.
                crate::Codegen::TIR::THostCall::CarrierFact { recv, field, notes } => {
                    let outcome = self.eval_expr(recv, scope)?;
                    crate::Comptime::Builtins::carrier_fact(&outcome, field, *notes)
                        .ok_or_else(|| {
                            unsupported(
                                "this middle state needs an error type that carries it",
                                self.span(),
                            )
                        })
                }
                crate::Codegen::TIR::THostCall::Method { recv, method, args } => {
                    let mut r = self.eval_expr(recv, scope)?;
                    if matches!(
                        &r,
                        CtValue::Struct { type_name, .. }
                            if matches!(
                                type_name.as_str(),
                                "__JetTirCell"
                                    | "__JetTirCellReadGuard"
                                    | "__JetTirCellEditGuard"
                            )
                    ) {
                        return self.eval_local_cell_method(
                            &r,
                            method,
                            args,
                            scope,
                        );
                    }
                    if matches!(&r, CtValue::Struct { type_name, .. } if type_name == "__JetTirExpiring")
                        && method == "with"
                    {
                        let clock_index = struct_int(&r, "clock")
                            .ok_or_else(|| unsupported("expiring secret clock", self.span()))?
                            as usize;
                        let deadline = struct_int(&r, "deadline")
                            .ok_or_else(|| unsupported("expiring secret deadline", self.span()))?;
                        let valid = self
                            .runtime
                            .lock()
                            .expect("evaluator runtime poisoned")
                            .clocks
                            .get(clock_index)
                            .is_some_and(|now| *now <= deadline);
                        if !valid {
                            return Ok(CtValue::failed(Box::new(CtValue::Str(
                                "expired".to_string(),
                            ))));
                        }
                        let CtValue::Struct { fields, .. } = &r else {
                            unreachable!();
                        };
                        let value = fields
                            .iter()
                            .find_map(|(name, value)| (name == "value").then(|| value.clone()))
                            .unwrap_or(CtValue::Unit);
                        let Some(TExpr {
                            kind: TExprKind::Lambda(lambda),
                            ..
                        }) = args.first()
                        else {
                            return Err(unsupported("expiring secret lambda", self.span()));
                        };
                        let result = self.eval_tlambda(lambda, vec![value], scope)?;
                        return Ok(CtValue::Present(Box::new(result)));
                    }
                    if matches!(&r, CtValue::Struct { type_name, .. } if type_name == "__JetTirSharedWeak")
                        && method == "upgrade"
                        && args.is_empty()
                    {
                        let CtValue::Struct { fields, .. } = &r else {
                            unreachable!();
                        };
                        let index = fields
                            .iter()
                            .find_map(|(name, value)| match (name.as_str(), value) {
                                ("index", CtValue::Int(index)) => Some(*index as usize),
                                _ => None,
                            })
                            .ok_or_else(|| unsupported("shared weak handle", self.span()))?;
                        let alive = self
                            .runtime
                            .lock()
                            .expect("evaluator runtime poisoned")
                            .shared_values
                            .get(index)
                            .is_some();
                        return Ok(if alive {
                            CtValue::Present(Box::new(CtValue::Struct {
                                type_name: "__JetTirShared".to_string(),
                                fields: vec![("index".to_string(), CtValue::Int(index as i64))],
                            }))
                        } else {
                            CtValue::absent(Type::Shared(Box::new(Type::Int)))
                        });
                    }
                    if matches!(&r, CtValue::Struct { type_name, .. } if type_name == "__JetTirShared") {
                        let CtValue::Struct { fields, .. } = &r else {
                            unreachable!();
                        };
                        let index = fields
                            .iter()
                            .find_map(|(name, value)| match (name.as_str(), value) {
                                ("index", CtValue::Int(index)) => Some(*index as usize),
                                _ => None,
                            })
                            .ok_or_else(|| unsupported("shared handle", self.span()))?;
                        // D-SHARED-CYCLE1=C: weak-handle methods (no lambda).
                        if method == "downgrade" && args.is_empty() {
                            return Ok(CtValue::Struct {
                                type_name: "__JetTirSharedWeak".to_string(),
                                fields: vec![("index".to_string(), CtValue::Int(index as i64))],
                            });
                        }
                        if method == "strong_count" && args.is_empty() {
                            let count = self
                                .runtime
                                .lock()
                                .expect("evaluator runtime poisoned")
                                .shared_values
                                .get(index)
                                .map(|shared| Arc::strong_count(shared) as i64)
                                .unwrap_or(0);
                            return Ok(CtValue::Int(count));
                        }
                        let transactional = method == "edit_txn";
                        if matches!(method.as_str(), "guard_read" | "guard_edit") {
                            let editable = method == "guard_edit";
                            let shared = self
                                .runtime
                                .lock()
                                .expect("evaluator runtime poisoned")
                                .shared_values
                                .get(index)
                                .cloned()
                                .ok_or_else(|| unsupported("shared handle", self.span()))?;
                            let lease = shared
                                .acquire(editable, self.task_cancel.as_ref())
                                .ok_or_else(|| {
                                    Diagnostic::error(
                                        "TASK_CANCELLED",
                                        "task cancelled".to_string(),
                                        "the owning taskgroup stopped this task".to_string(),
                                        String::new(),
                                        Some(self.span()),
                                    )
                                })?;
                            let mut runtime =
                                self.runtime.lock().expect("evaluator runtime poisoned");
                            let lease_index = runtime.shared_guards.len();
                            runtime.shared_guards.push(lease);
                            drop(runtime);
                            self.shared_guards.push(lease_index);
                            return Ok(CtValue::Struct {
                                type_name: "__JetTirSharedGuard".to_string(),
                                fields: vec![
                                    ("shared".to_string(), CtValue::Int(index as i64)),
                                    ("lease".to_string(), CtValue::Int(lease_index as i64)),
                                    (
                                        "editable".to_string(),
                                        CtValue::Bool(editable),
                                    ),
                                ],
                            });
                        }
                        let shared = self
                            .runtime
                            .lock()
                            .expect("evaluator runtime poisoned")
                            .shared_values
                            .get(index)
                            .cloned()
                            .ok_or_else(|| unsupported("shared value", self.span()))?;
                        let Some(TExpr {
                            kind: TExprKind::Lambda(lambda),
                            ..
                        }) = args.first()
                        else {
                            return Err(unsupported("shared method lambda", self.span()));
                        };
                        if transactional {
                            let Some(transaction) = self.shared_transactions.last_mut() else {
                                return Err(unsupported(
                                    "Shared.edit_txn outside #Transact",
                                    self.span(),
                                ));
                            };
                            transaction.push(super::EvalSharedDelta {
                                shared_index: index,
                                lambda,
                                captured: scope.clone(),
                            });
                            return Ok(CtValue::Unit);
                        }
                        let editable = method != "read";
                        let _lease = shared
                            .acquire(editable, self.task_cancel.as_ref())
                            .ok_or_else(|| {
                                Diagnostic::error(
                                    "TASK_CANCELLED",
                                    "task cancelled".to_string(),
                                    "the owning taskgroup stopped this task".to_string(),
                                    String::new(),
                                    Some(self.span()),
                                )
                            })?;
                        let shared_value = shared
                            .value
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .clone();
                        let current = shared_value;
                        let (result, updated) =
                            self.eval_tlambda_mut_arg(lambda, current, scope)?;
                        if method == "edit" {
                            *shared
                                .value
                                .lock()
                                .unwrap_or_else(|error| error.into_inner()) = updated;
                        }
                        return Ok(result);
                    }
                    let mut argv = Vec::with_capacity(args.len());
                    for a in args {
                        argv.push(self.eval_expr(a, scope)?);
                    }
                    let result = match crate::Comptime::Builtins::apply_mutating(
                        &mut r,
                        method,
                        argv.clone(),
                        self.span(),
                    ) {
                        Ok(v) => v,
                        Err(_) => crate::Comptime::Builtins::apply_method(
                            &r,
                            method,
                            argv,
                            self.span(),
                        )?,
                    };
                    self.write_back_place(recv, r, scope)?;
                    Ok(result)
                }
                crate::Codegen::TIR::THostCall::YieldSend { value } => {
                    let yielded = self.eval_expr(value, scope)?;
                    if let Some(items) = self.collecting_items.last_mut() {
                        items.push(yielded);
                        return Ok(CtValue::Unit);
                    }
                    let consumer = self
                        .yield_consumer
                        .clone()
                        .ok_or_else(|| unsupported("yield outside a stream consumer", self.span()))?;
                    let mut consumer_scope = self
                        .yield_scope
                        .take()
                        .ok_or_else(|| unsupported("stream consumer scope", self.span()))?;
                    consumer_scope.insert(consumer.var, yielded);
                    let result = self.exec_stmts(consumer.body, &mut consumer_scope);
                    self.yield_scope = Some(consumer_scope);
                    match result? {
                        Flow::Normal | Flow::Continue => Ok(CtValue::Unit),
                        Flow::Break => {
                            self.pending_return = Some(CtValue::Unit);
                            Ok(CtValue::Unit)
                        }
                        other => Err(unsupported(
                            &format!("stream consumer control flow {other:?}"),
                            self.span(),
                        )),
                    }
                }
                crate::Codegen::TIR::THostCall::Helper { helper, args } => {
                    let leaf = helper
                        .rsplit("::")
                        .next()
                        .unwrap_or(helper.as_str());
                    let mut argv = Vec::with_capacity(args.len());
                    for a in args {
                        match a {
                            crate::Codegen::TIR::THostArg::Expr(e)
                            | crate::Codegen::TIR::THostArg::Borrow(e) => {
                                argv.push(self.eval_expr(e, scope)?);
                            }
                            crate::Codegen::TIR::THostArg::Lambda(_) => {
                                return Err(unsupported(
                                    "expr `HostCall` helper lambda",
                                    self.span(),
                                ));
                            }
                        }
                    }
                    if leaf == "jet_std_clock_new" || leaf.ends_with("jet_std_clock_new") {
                        let seed = match argv.first() {
                            Some(CtValue::Int(n)) => *n,
                            _ => {
                                return Err(unsupported(
                                    "Clock.new expects an Int seed",
                                    self.span(),
                                ));
                            }
                        };
                        let mut runtime =
                            self.runtime.lock().expect("evaluator runtime poisoned");
                        let index = runtime.clocks.len();
                        runtime.clocks.push(seed);
                        return Ok(CtValue::Struct {
                            type_name: "__JetTirClock".to_string(),
                            fields: vec![("index".to_string(), CtValue::Int(index as i64))],
                        });
                    }
                    if leaf == "jet_std_clock_system" || leaf.ends_with("jet_std_clock_system") {
                        return Ok(CtValue::Struct {
                            type_name: crate::Syntax::CLOCK_TYPE.to_string(),
                            fields: vec![("now".to_string(), CtValue::Int(0))],
                        });
                    }
                    // D-ERRCTX1: `.context(msg)` — prepend message on Err only.
                    if leaf == "jet_context" || leaf.ends_with("jet_context") {
                        let msg = match argv.get(1) {
                            Some(CtValue::Str(s)) => s.clone(),
                            Some(other) => other.jet_show(),
                            None => String::new(),
                        };
                        return Ok(match argv.first() {
                            Some(CtValue::Present(v)) => CtValue::Present(v.clone()),
                            Some(CtValue::Failed(CtReport::Told(err))) => {
                                CtValue::failed(Box::new(CtValue::Str(format!(
                                    "{}: {}",
                                    msg,
                                    err.jet_show()
                                ))))
                            }
                            Some(other) => other.clone(),
                            None => CtValue::Unit,
                        });
                    }
                    Err(unsupported(
                        &format!("expr `HostCall` helper `{leaf}`"),
                        self.span(),
                    ))
                }
                crate::Codegen::TIR::THostCall::ExpiringValueNew {
                    value,
                    duration,
                    clock,
                }
                | crate::Codegen::TIR::THostCall::ExpiringSecretNew {
                    value,
                    duration,
                    clock,
                    ..
                } => {
                    let value = self.eval_expr(value, scope)?;
                    let duration = self.eval_expr(duration, scope)?;
                    let duration = struct_int(&duration, "ms")
                        .ok_or_else(|| unsupported("expiring duration", self.span()))?;
                    let clock = self.eval_expr(clock, scope)?;
                    let clock_index = handle_index(&clock, "__JetTirClock")
                        .ok_or_else(|| unsupported("expiring clock", self.span()))?;
                    let now = *self
                        .runtime
                        .lock()
                        .expect("evaluator runtime poisoned")
                        .clocks
                        .get(clock_index)
                        .ok_or_else(|| unsupported("expiring clock handle", self.span()))?;
                    Ok(CtValue::Struct {
                        type_name: "__JetTirExpiring".to_string(),
                        fields: vec![
                            ("value".to_string(), value),
                            (
                                "deadline".to_string(),
                                CtValue::Int(now.saturating_add(duration)),
                            ),
                            ("clock".to_string(), CtValue::Int(clock_index as i64)),
                        ],
                    })
                }
                other => {
                    let tag = match other {
                        crate::Codegen::TIR::THostCall::Helper { .. } => "Helper",
                        crate::Codegen::TIR::THostCall::Method { .. } => "Method",
                        crate::Codegen::TIR::THostCall::FixedListIndex { .. } => "FixedListIndex",
                        crate::Codegen::TIR::THostCall::TypedText { .. } => "TypedText",
                        crate::Codegen::TIR::THostCall::FnName(_) => "FnName",
                        crate::Codegen::TIR::THostCall::GcEdit { .. } => "GcEdit",
                        crate::Codegen::TIR::THostCall::GcRead { .. } => "GcRead",
                        crate::Codegen::TIR::THostCall::OptionProbe { .. } => "OptionProbe",
                        crate::Codegen::TIR::THostCall::StrMatchScan { .. } => "StrMatchScan",
                        crate::Codegen::TIR::THostCall::BinMatchScan { .. } => "BinMatchScan",
                        crate::Codegen::TIR::THostCall::TupleIndex { .. } => "TupleIndex",
                        crate::Codegen::TIR::THostCall::SwitchSubjectField { .. } => {
                            "SwitchSubjectField"
                        }
                        crate::Codegen::TIR::THostCall::YieldSend { .. } => unreachable!(),
                        crate::Codegen::TIR::THostCall::TypedTextInterp { .. } => "TypedTextInterp",
                        crate::Codegen::TIR::THostCall::ExpectSnapshot { .. } => "ExpectSnapshot",
                        crate::Codegen::TIR::THostCall::EnvSet { .. } => "EnvSet",
                        _ => "Other",
                    };
                    Err(unsupported(
                        &format!("expr `HostCall` {tag}"),
                        self.span(),
                    ))
                }
            },
            TExprKind::DataEntriesToMap(local) => {
                let value = scope
                    .get(&local.name)
                    .cloned()
                    .or_else(|| self.globals.get(&local.name).cloned())
                    .ok_or_else(|| unsupported(&format!("unbound `{}`", local.name), self.span()))?;
                // DataTree.Object binds its ordered payload as the evaluator's
                // JSONObject record. The AOT/JIT paths collect that payload into
                // the user-facing Map before the generated decoder iterates it;
                // keep the same boundary in TIR so named deopt executes the same
                // generated source instead of rejecting the internal record.
                match value {
                    CtValue::Struct { type_name, fields } if type_name == "JSONObject" => {
                        Ok(CtValue::Map(
                            fields
                                .into_iter()
                                .map(|(key, value)| (crate::AST::CtKey::Str(key), value))
                                .collect(),
                        ))
                    }
                    other => Ok(other),
                }
            }
            TExprKind::DistinctCtor { name: _, arg, base: _ } => {
                // Distinct is a zero-cost nominal wrapper over its base scalar.
                self.eval_expr(arg, scope)
            }
            TExprKind::RangeCheckedCtor { name, arg } => {
                let v = self.eval_expr(arg, scope)?;
                Ok(CtValue::Present(Box::new(v)))
                // Range bounds are enforced by sema for literals; dynamic checks
                // reuse the same ok-wrapping Result shape as AOT try_new.
                .map(|ok| {
                    let _ = name;
                    ok
                })
            }
            TExprKind::DistinctConvert {
                name: _,
                arg,
                op,
                range,
                fallible,
            } => {
                let v = self.eval_expr(arg, scope)?;
                let converted = self.eval_numeric_op(&v, op, &arg.ty, &expr.ty)?;
                let inner = match converted {
                    CtValue::Present(v) => *v,
                    CtValue::Failed(CtReport::Told(e)) if *fallible => return Ok(CtValue::Failed(CtReport::Told(e))),
                    other if !*fallible => other,
                    other => other,
                };
                if let Some((lo, hi)) = range {
                    let CtValue::Int(n) = &inner else {
                        return Err(unsupported("distinct range check on non-Int", self.span()));
                    };
                    if *n < *lo || *n > *hi {
                        let err = CtValue::Str(format!("value doesn't fit in range {lo}..{hi}"));
                        return Ok(if *fallible {
                            CtValue::failed(Box::new(err))
                        } else {
                            return Err(unsupported("distinct out of range", self.span()));
                        });
                    }
                }
                Ok(if *fallible {
                    CtValue::Present(Box::new(inner))
                } else {
                    inner
                })
            }
            TExprKind::UnitConvert {
                arg,
                scale,
                offset,
                rounding,
                fallible,
                ..
            } => {
                let CtValue::Float(value) = self.eval_expr(arg, scope)? else {
                    return Err(unsupported("unit conversion on non-Float", self.span()));
                };
                let converted = if let Some((mode, digits)) = rounding {
                    let CtValue::Int(digits) = self.eval_expr(digits, scope)? else {
                        return Err(unsupported(
                            "unit conversion digits on non-Int",
                            self.span(),
                        ));
                    };
                    jet_foundation::jet_unit_conversion_rounded(
                        value.as_f64(),
                        &scale.num.to_string(),
                        &scale.den.to_string(),
                        &offset.num.to_string(),
                        &offset.den.to_string(),
                        *mode,
                        digits,
                    )
                    .map_err(str::to_string)
                } else {
                    jet_foundation::jet_unit_conversion_exact(
                        value.as_f64(),
                        &scale.num.to_string(),
                        &scale.den.to_string(),
                        &offset.num.to_string(),
                        &offset.den.to_string(),
                    )
                    .ok_or_else(|| "unit conversion would round".to_string())
                };
                match converted {
                    Ok(value) if *fallible || rounding.is_some() => Ok(CtValue::Present(Box::new(
                        CtValue::Float(CtFloat::f64(value)),
                    ))),
                    Ok(value) => Ok(CtValue::Float(CtFloat::f64(value))),
                    Err(error) if *fallible || rounding.is_some() => {
                        Ok(CtValue::failed(Box::new(CtValue::Str(error))))
                    }
                    Err(error) => Err(unsupported(&error, self.span())),
                }
            }
            TExprKind::MathBuiltin {
                type_name,
                func,
                args,
            } => {
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(a, scope)?);
                }
                if let Some(res) = crate::Comptime::Builtins::apply_static_type_method(
                    type_name,
                    func,
                    argv.clone(),
                    self.span(),
                ) {
                    return res;
                }
                // Instance-style math ops arrive as MathBuiltin with the receiver
                // already folded into `args` for free-function emit; try method form.
                if let Some((recv, rest)) = argv.split_first() {
                    match crate::Comptime::Builtins::apply_method(
                        recv,
                        func,
                        rest.to_vec(),
                        self.span(),
                    ) {
                        Ok(v) => return Ok(v),
                        Err(_) => {}
                    }
                }
                Err(unsupported(
                    &format!("`{type_name}.{func}`"),
                    self.span(),
                ))
            }
            TExprKind::PreciseBuiltin {
                type_name,
                func,
                args,
            } => {
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(a, scope)?);
                }
                eval_precise_builtin(type_name, func, argv, self.span())
            }
            TExprKind::ResourceNew(inner) => self.eval_expr(inner, scope),
            TExprKind::ResourceTake(place) => scope
                .get(place)
                .cloned()
                .or_else(|| place.strip_prefix("user_").and_then(|name| scope.get(name).cloned()))
                .or_else(|| self.globals.get(place).cloned())
                .ok_or_else(|| unsupported(&format!("resource `{place}`"), self.span())),
            TExprKind::AmbientInput { .. } => Err(unsupported("expr `AmbientInput`", self.span())),
            TExprKind::RequireStop {
                kind,
                loc,
                always_stops,
            } => {
                let failed = if *always_stops {
                    true
                } else {
                    match kind {
                        crate::Codegen::TIR::TRequireKind::Require { cond, .. } => {
                            !as_bool(&self.eval_expr(cond, scope)?, self.span())?
                        }
                        crate::Codegen::TIR::TRequireKind::RequireEq { left, right, .. } => {
                            self.eval_expr(left, scope)? != self.eval_expr(right, scope)?
                        }
                        crate::Codegen::TIR::TRequireKind::Panic { .. } => true,
                    }
                };
                if !failed {
                    return Ok(CtValue::Unit);
                }
                let msg = match kind {
                    crate::Codegen::TIR::TRequireKind::Require { msg: Some(msg), .. }
                    | crate::Codegen::TIR::TRequireKind::Panic { msg } => {
                        self.eval_expr(msg, scope)?.jet_show()
                    }
                    crate::Codegen::TIR::TRequireKind::Require { msg: None, .. } => {
                        "requirement failed".to_string()
                    }
                    crate::Codegen::TIR::TRequireKind::RequireEq { .. } => {
                        "values are not equal".to_string()
                    }
                };
                let file = loc.file.trim_matches('"');
                let fn_name = loc.fn_name.trim_matches('"');
                let src_line = loc.src_line.trim_matches('"');
                let line_s = loc.line.to_string();
                let margin = line_s.len();
                let pad = " ".repeat(margin);
                let col_offset = loc.col.saturating_sub(1) as usize;
                let caret = "^".repeat(loc.caret.max(1) as usize);
                let rendered = format!(
                    "panic: {msg}\n  --> {file}:{} in {fn_name}\n   {pad}|\n{line_s} | {src_line}\n   {pad}| {}{caret}\n",
                    loc.line,
                    " ".repeat(col_offset)
                );
                if let Some(sink) = self.sink.as_ref() {
                    let mut sink = sink.lock().expect("evaluator sink poisoned");
                    sink.stderr.push_str(&rendered);
                    sink.exit_code = Some(70);
                    return Err(Diagnostic::error(
                        "SOFT_EXIT",
                        "70".to_string(),
                        "require/panic stop".to_string(),
                        String::new(),
                        Some(self.span()),
                    ));
                }
                Err(unsupported("require/panic stop", self.span()))
            }
            TExprKind::LayoutCompare { .. } => Err(unsupported("expr `LayoutCompare`", self.span())),
            TExprKind::LayoutLit { .. } => Err(unsupported("expr `LayoutLit`", self.span())),
            TExprKind::IncDec {
                op,
                place,
                postfix,
                ..
            } => {
                let TPlace::Local(local) = place else {
                    return Err(unsupported("inc/dec place", self.span()));
                };
                let key = local.name.clone();
                let cur = scope
                    .get(&key)
                    .cloned()
                    .or_else(|| self.globals.get(&key).cloned())
                    .unwrap_or(CtValue::Unit);
                let n = as_int(&cur, self.span())?;
                let next = match op {
                    crate::AST::IncDecOp::Inc => n.wrapping_add(1),
                    crate::AST::IncDecOp::Dec => n.wrapping_sub(1),
                };
                scope.insert(key, CtValue::Int(next));
                Ok(if *postfix {
                    CtValue::Int(n)
                } else {
                    CtValue::Int(next)
                })
            }
            TExprKind::PtrFromAddr { .. } => Err(unsupported("expr `PtrFromAddr`", self.span())),
            TExprKind::Deref(inner) => {
                let pointer = self.eval_expr(inner, scope)?;
                let CtValue::Struct { type_name, fields } = pointer else {
                    return Err(unsupported("raw pointer carrier", self.span()));
                };
                if type_name != "__JetRawLocal" {
                    return Err(unsupported("raw pointer target", self.span()));
                }
                if let Some(value) = fields.iter().find_map(|(field, value)| {
                    (field == "value").then(|| value.clone())
                }) {
                    return Ok(value);
                }
                let name = fields.iter().find_map(|(field, value)| match (field.as_str(), value) {
                    ("name", CtValue::Str(name)) => Some(name.as_str()),
                    _ => None,
                });
                name.and_then(|name| scope.get(name).cloned())
                    .ok_or_else(|| unsupported("raw pointer local", self.span()))
            }
            TExprKind::RawOf(inner) => {
                if matches!(
                    &inner.ty,
                    Type::Apply { name, args }
                        if name == crate::Syntax::TYPE_PTR && args.len() == 1
                ) {
                    return self.eval_expr(inner, scope);
                }
                let local = super::raw_place_local(inner);
                let fields = if let Some(local) = local {
                    vec![("name".to_string(), CtValue::Str(local.name.clone()))]
                } else {
                    vec![("value".to_string(), self.eval_expr(inner, scope)?)]
                };
                Ok(CtValue::Struct {
                    type_name: "__JetRawLocal".to_string(),
                    fields,
                })
            }
            TExprKind::AllocNew { ctor } => Ok(CtValue::Struct {
                type_name: "__JetTirAllocator".to_string(),
                fields: vec![("ctor".to_string(), CtValue::Str(ctor.clone()))],
            }),
            TExprKind::JSONLit { variant, arg } => {
                let payload = match arg {
                    Some(inner) => Some(self.eval_expr(&inner.0, scope)?),
                    None => None,
                };
                Ok(CtValue::Enum {
                    type_name: "JSON".to_string(),
                    variant: variant.clone(),
                    args: match payload {
                        Some(v) => vec![(None, v)],
                        None => Vec::new(),
                    },
                })
            }
            TExprKind::DBValueLit { variant, arg } => {
                let payload = match arg {
                    Some(inner) => Some(self.eval_expr(&inner.0, scope)?),
                    None => None,
                };
                Ok(CtValue::Enum {
                    type_name: "DBValue".to_string(),
                    variant: variant.clone(),
                    args: match payload {
                        Some(v) => vec![(None, v)],
                        None => Vec::new(),
                    },
                })
            }
            TExprKind::ListSpread { parts } => {
                let mut values = Vec::new();
                for part in parts {
                    match part {
                        ListSpreadPart::Elem(expr) => {
                            values.push(self.eval_expr(expr, scope)?);
                        }
                        ListSpreadPart::Spread(expr) => {
                            let CtValue::List(items) = self.eval_expr(expr, scope)? else {
                                return Err(unsupported("list spread operand", self.span()));
                            };
                            values.extend(items);
                        }
                    }
                }
                Ok(CtValue::List(values))
            }
            TExprKind::ColumnarListLit { .. } => {
                Err(unsupported("expr `ColumnarListLit`", self.span()))
            }
            TExprKind::ColumnarGather { .. } => {
                Err(unsupported("expr `ColumnarGather`", self.span()))
            }
            TExprKind::ColumnarColumnRead { .. } => {
                Err(unsupported("expr `ColumnarColumnRead`", self.span()))
            }
            TExprKind::PoolSlot {
                pool,
                id,
                field,
                ..
            } => {
                let pool_value = self.eval_expr(pool, scope)?;
                let id_value = self.eval_expr(id, scope)?;
                let Some((index, generation)) = pool_id_parts(&id_value) else {
                    return Err(pool_stale_diagnostic());
                };
                let CtValue::Struct { fields, .. } = pool_value else {
                    return Err(pool_stale_diagnostic());
                };
                let slots = fields.iter().find_map(|(name, value)| match value {
                    CtValue::List(slots) if name == "slots" => Some(slots),
                    _ => None,
                });
                let Some(CtValue::Enum {
                    variant,
                    args,
                    ..
                }) = slots.and_then(|slots| slots.get(index))
                else {
                    return Err(pool_stale_diagnostic());
                };
                if variant != "Occupied"
                    || !matches!(args.first(), Some((_, CtValue::Int(found))) if *found == generation)
                {
                    return Err(pool_stale_diagnostic());
                }
                let Some((_, mut value)) = args.get(1).cloned() else {
                    return Err(pool_stale_diagnostic());
                };
                if let Some(field) = field {
                    let CtValue::Struct { fields, .. } = value else {
                        return Err(unsupported("Pool field on a non-struct", self.span()));
                    };
                    value = fields
                        .into_iter()
                        .find_map(|(name, value)| (name == *field).then_some(value))
                        .ok_or_else(|| unsupported(&format!("Pool field `{field}`"), self.span()))?;
                }
                Ok(value)
            }
            TExprKind::IndexHook {
                type_name,
                base,
                index,
                ..
            } => {
                let recv = self.eval_expr(base, scope)?;
                let key = self.eval_expr(index, scope)?;
                let func = self
                    .funcs
                    .get(&format!("{type_name}::get"))
                    .copied()
                    .ok_or_else(|| unsupported("Index.get", self.span()))?;
                let mut child = HashMap::new();
                child.insert("self".to_string(), recv);
                match self.run_func(func, vec![key], &mut child)? {
                    CtValue::Present(value) => Ok(*value),
                    CtValue::Failed(CtReport::Clean(_)) => Err(unsupported("index miss", self.span())),
                    _ => Err(unsupported("Index.get result", self.span())),
                }
            }
            TExprKind::MathLaneIndex { base, index, .. } => {
                let b = self.eval_expr(base, scope)?;
                let i = as_int(&self.eval_expr(index, scope)?, self.span())?;
                match crate::Comptime::MathLayout::lane_at(&b, i, self.span()) {
                    Some(r) => r,
                    None => Err(unsupported("expr `MathLaneIndex`", self.span())),
                }
            }
            TExprKind::MathSwizzleRead { .. } => {
                Err(unsupported("expr `MathSwizzleRead`", self.span()))
            }
            TExprKind::FnFieldCall { recv, field, args } => {
                let value = self.eval_expr(recv, scope)?;
                let CtValue::Struct { fields, .. } = value else {
                    return Err(unsupported("function field receiver", self.span()));
                };
                let callable = fields
                    .into_iter()
                    .find_map(|(name, value)| (name == *field).then_some(value))
                    .ok_or_else(|| unsupported(&format!("function field `{field}`"), self.span()))?;
                let mut argv = Vec::with_capacity(args.len());
                for arg in args {
                    argv.push(self.eval_expr(&arg.value, scope)?);
                }
                self.call_callable(&callable, argv)
            }
            TExprKind::DecodeUnder { segment, inner } => {
                let segment = match self.eval_expr(segment, scope)? {
                    CtValue::Str(value) => value,
                    other => other.jet_show(),
                };
                let result = self.eval_expr(inner, scope)?;
                match result {
                    CtValue::Failed(CtReport::Told(error)) => Ok(CtValue::failed(Box::new(
                        decode_error_under(&segment, *error),
                    ))),
                    other => Ok(other),
                }
            }
            TExprKind::StaticCall {
                owner,
                owner_type,
                method,
                args,
                ..
            } => {
                let mut argv = Vec::with_capacity(args.len());
                for a in args {
                    argv.push(self.eval_expr(&a.value, scope)?);
                }
                match owner {
                    crate::Codegen::TIR::TStaticOwner::User(type_name) => {
                        if method.name == "diff" && argv.len() == 2 {
                            if let (
                                CtValue::Struct {
                                    type_name: new_name,
                                    fields: new_fields,
                                },
                                CtValue::Struct {
                                    type_name: old_name,
                                    fields: old_fields,
                                },
                            ) = (&argv[0], &argv[1])
                            {
                                if new_name == type_name && old_name == type_name {
                                    let fields = new_fields
                                        .iter()
                                        .map(|(name, new_value)| {
                                            let changed = old_fields
                                                .iter()
                                                .find_map(|(old_name, old_value)| {
                                                    (old_name == name).then_some(old_value)
                                                })
                                                != Some(new_value);
                                            (
                                                name.clone(),
                                                if changed {
                                                    CtValue::Present(Box::new(new_value.clone()))
                                                } else {
                                                    CtValue::absent(new_value.jet_type())
                                                },
                                            )
                                        })
                                        .collect();
                                    return Ok(CtValue::Struct {
                                        type_name: format!("{type_name}.Patch"),
                                        fields,
                                    });
                                }
                            }
                        }
                        if let Some(res) = crate::Comptime::Builtins::apply_static_type_method(
                            type_name,
                            &method.name,
                            argv.clone(),
                            self.span(),
                        ) {
                            return res;
                        }
                        // Core-import alias may still lower as StaticCall when
                        // function bodies were typed before imports propagated.
                        if let Some(module) = self.core_imports.get(type_name) {
                            if self.should_decline_ambient_fold(module, &method.name) {
                                return Err(unsupported(
                                    &format!("`{module}.{}()` at compile time", method.name),
                                    self.span(),
                                ));
                            }
                            return apply_core_call(
                                module,
                                &method.name,
                                argv,
                                self.span(),
                                self.repl_mode,
                            );
                        }
                        // Match Cranelift JIT `static_method_key`: mono methods are
                        // keyed by the concrete owner (`Box<Int>::new`). When a
                        // concrete owner_type is present, never fall back to a bare
                        // method name — an unrelated same-name fn could bind wrong.
                        let concrete = owner_type
                            .as_ref()
                            .map(|ty| ty.name())
                            .unwrap_or_else(|| type_name.clone());
                        let mut candidates = vec![
                            format!("{concrete}::{}", method.name),
                            format!("{concrete}.{}", method.name),
                            format!("{type_name}::{}", method.name),
                            format!("{type_name}.{}", method.name),
                        ];
                        if owner_type.is_none() {
                            candidates.push(method.name.clone());
                            candidates.push(format!("user_{}", method.name));
                        }
                        for name in candidates {
                            if let Some(func) = self.funcs.get(&name).copied() {
                                let mut child = HashMap::new();
                                return self.run_func(func, argv, &mut child);
                            }
                        }
                        Err(unsupported(
                            &format!("static `{type_name}.{}`", method.name),
                            self.span(),
                        ))
                    }
                    crate::Codegen::TIR::TStaticOwner::Prelude { path, .. } => {
                        if let Some(value) =
                            crate::Comptime::xml_safe_static_for_tir(path, &method.name)
                        {
                            return Ok(value);
                        }
                        if path == "jet_std::JetShared" && method.name == "new" && argv.len() == 1 {
                            let mut runtime =
                                self.runtime.lock().expect("evaluator runtime poisoned");
                            let index = runtime.shared_values.len();
                            runtime
                                .shared_values
                                .push(std::sync::Arc::new(super::EvalSharedState::new(
                                    argv.remove(0),
                                )));
                            return Ok(CtValue::Struct {
                                type_name: "__JetTirShared".to_string(),
                                fields: vec![("index".to_string(), CtValue::Int(index as i64))],
                            });
                        }
                        if path == "jet_std::JetCondition"
                            && method.name == "new"
                            && argv.is_empty()
                        {
                            let mut runtime =
                                self.runtime.lock().expect("evaluator runtime poisoned");
                            let index = runtime.shared_conditions.len();
                            runtime
                                .shared_conditions
                                .push(super::shared_protocol::JetConditionProtocol::new());
                            return Ok(CtValue::Struct {
                                type_name: "__JetTirCondition".to_string(),
                                fields: vec![("index".to_string(), CtValue::Int(index as i64))],
                            });
                        }
                        if matches!(
                            path.as_str(),
                            "jet_std::JetCell" | "jet_std::jet_cell::JetCell"
                        )
                            && method.name == "new"
                            && argv.len() == 1
                        {
                            let index = self.local_cells.insert_cell(argv.remove(0));
                            return Ok(local_cell_handle("__JetTirCell", index));
                        }
                        if let Some(res) = crate::Comptime::Builtins::apply_static_type_method(
                            path,
                            &method.name,
                            argv,
                            self.span(),
                        ) {
                            res
                        } else {
                            Err(unsupported(
                                &format!("prelude static `{path}.{}`", method.name),
                                self.span(),
                            ))
                        }
                    }
                }
            }
            TExprKind::Todo { expected_type, .. } => Err(unsupported(&format!("expr Todo ({expected_type})"), self.span())),
            // Card #1440: sema proved this arm dead (E0307) — reaching it in
            // the interpreter is a compiler bug, reported as an internal error.
            TExprKind::Unreachable { line } => Err(unsupported(
                &format!("proven-unreachable exhaustive-dispatch arm (line {line})"),
                self.span(),
            )),
            TExprKind::DistinctRaw(inner) => self.eval_expr(inner, scope),
            TExprKind::OptField {
                base,
                member,
                flatten,
            } => {
                let v = self.eval_expr(base, scope)?;
                match v {
                    CtValue::Failed(CtReport::Clean(_)) => Ok(CtValue::absent(expr.ty.clone())),
                    CtValue::Present(inner) => {
                        let field = match *inner {
                            CtValue::Struct { fields, .. } => fields
                                .into_iter()
                                .find(|(n, _)| n == member)
                                .map(|(_, v)| v)
                                .ok_or_else(|| {
                                    unsupported(&format!("opt field `{member}`"), self.span())
                                })?,
                            _ => {
                                return Err(unsupported("opt-field recv", self.span()));
                            }
                        };
                        if *flatten {
                            Ok(field)
                        } else {
                            Ok(CtValue::Present(Box::new(field)))
                        }
                    }
                    CtValue::Struct { fields, .. } => fields
                        .into_iter()
                        .find(|(n, _)| n == member)
                        .map(|(_, v)| {
                            if *flatten {
                                v
                            } else {
                                CtValue::Present(Box::new(v))
                            }
                        })
                        .ok_or_else(|| unsupported(&format!("opt field `{member}`"), self.span())),
                    _ => Err(unsupported("opt-field recv", self.span())),
                }
            }
            TExprKind::Lambda(lambda) => Ok(self.store_callable(EvalCallable::Lambda {
                lambda,
                captured: scope.clone(),
            })),
            TExprKind::PatternMatches { subj, pattern } => {
                let value = self.eval_expr(subj, scope)?;
                // Binding-free `x == .Variant` — reuse match-arm binder, discard locals.
                let mut scratch = HashMap::new();
                Ok(CtValue::Bool(super::stmts::bind_match_pattern(
                    &pattern.pattern,
                    &value,
                    &mut scratch,
                )?))
            }
            TExprKind::OptionLift2 { .. } => Err(unsupported("expr `OptionLift2`", self.span())),
            TExprKind::ClosureMethod { recv, op, args } => {
                self.eval_closure_method(recv, op, args, scope)
            }
            TExprKind::HostBorrowCallback { .. } => {
                Err(unsupported("expr `HostBorrowCallback`", self.span()))
            }
            TExprKind::NumericMethod { recv, op } => {
                let v = self.eval_expr(recv, scope)?;
                self.eval_numeric_op(&v, op, &recv.ty, &expr.ty)
            }
            TExprKind::OverflowOpt {
                prefix,
                op,
                lhs,
                rhs,
            } => {
                let l = self.eval_expr(lhs, scope)?;
                let r = self.eval_expr(rhs, scope)?;
                let a = as_int(&l, self.span())?;
                let b = as_int(&r, self.span())?;
                let width_ty = match &expr.ty {
                    Type::Option(inner) => inner.as_ref(),
                    other => other,
                };
                let (signed, bits) = match width_ty {
                    Type::IntN { signed, bits } => (*signed, *bits),
                    Type::Int => (true, 64),
                    Type::Named(n) => match n.as_str() {
                        "U8" => (false, 8),
                        "I8" => (true, 8),
                        "U16" => (false, 16),
                        "I16" => (true, 16),
                        "U32" => (false, 32),
                        "I32" => (true, 32),
                        "U64" => (false, 64),
                        "I64" | "Int" => (true, 64),
                        _ => match &lhs.ty {
                            Type::IntN { signed, bits } => (*signed, *bits),
                            Type::Int => (true, 64),
                            _ => (true, 64),
                        },
                    },
                    _ => match &lhs.ty {
                        Type::IntN { signed, bits } => (*signed, *bits),
                        Type::Int => (true, 64),
                        _ => (true, 64),
                    },
                };
                let bin = match *op {
                    "add" => crate::AST::BinOp::Add,
                    "sub" => crate::AST::BinOp::Sub,
                    "mul" => crate::AST::BinOp::Mul,
                    "div" => crate::AST::BinOp::Div,
                    other => {
                        return Err(unsupported(
                            &format!("OverflowOpt op `{other}`"),
                            self.span(),
                        ));
                    }
                };
                crate::Comptime::MathLayout::overflow_opt(
                    prefix,
                    bin,
                    a,
                    b,
                    signed,
                    bits,
                    self.span(),
                )
            }
            TExprKind::CoreClosureCall {
                kind: TCoreClosureKind::Spawn { group, site, .. },
            } => self.eval_spawn(*site, group.as_deref(), scope),
            // D-VERDICT-1323-1: n tasks from one callable — the same spawn the
            // single form uses, repeated, so the group carries identical meaning.
            TExprKind::CoreClosureCall {
                kind: TCoreClosureKind::SpawnGroup { count, site, .. },
            } => {
                let CtValue::Int(count) = self.eval_expr(count, scope)? else {
                    return Err(unsupported("spawn_group count", self.span()));
                };
                let mut tasks = Vec::new();
                for _ in 0..count.max(0) {
                    tasks.push(self.eval_spawn(*site, None, scope)?);
                }
                Ok(CtValue::List(tasks))
            }
            TExprKind::CoreClosureCall {
                kind: TCoreClosureKind::Guard { executable, .. },
            } => {
                self.scope_guards.push(executable.as_ref());
                Ok(CtValue::Unit)
            }
            TExprKind::CoreClosureCall {
                kind: TCoreClosureKind::OnCommit { executable, .. },
            } => {
                let Some(frame) = self.txn_stack.last_mut() else {
                    return Err(unsupported("on_commit outside transaction", self.span()));
                };
                frame.on_commit.push(executable.as_ref());
                Ok(CtValue::Unit)
            }
            TExprKind::CoreClosureCall {
                kind: TCoreClosureKind::OnRollback { executable, .. },
            } => {
                let Some(frame) = self.txn_stack.last_mut() else {
                    return Err(unsupported("on_rollback outside transaction", self.span()));
                };
                frame.on_rollback.push(executable.as_ref());
                Ok(CtValue::Unit)
            }
            TExprKind::CoreClosureCall { .. } => {
                Err(unsupported("expr `CoreClosureCall`", self.span()))
            }
            TExprKind::TaskGroupAll { tasks } => {
                let CtValue::List(tasks) = self.eval_expr(tasks, scope)? else {
                    return Err(unsupported("taskgroup all list", self.span()));
                };
                self.task_select(&tasks, crate::task_group::JetTaskSelectMode::All)
            }
            TExprKind::TaskGroupRace { tasks } => {
                let CtValue::List(tasks) = self.eval_expr(tasks, scope)? else {
                    return Err(unsupported("taskgroup race list", self.span()));
                };
                self.task_select(&tasks, crate::task_group::JetTaskSelectMode::Race)
            }
            TExprKind::TaskGroupAny { tasks } => {
                let CtValue::List(tasks) = self.eval_expr(tasks, scope)? else {
                    return Err(unsupported("taskgroup any list", self.span()));
                };
                self.task_select(&tasks, crate::task_group::JetTaskSelectMode::Any)
            }
            TExprKind::SelectStart => Ok(self.new_eval_select()),
            TExprKind::SelectRecv { builder, channel } => {
                let builder = self.eval_expr(builder, scope)?;
                let channel = self.eval_expr(channel, scope)?;
                let receiver = handle_index(&channel, "Receiver")
                    .ok_or_else(|| unsupported("select receiver", self.span()))?;
                self.eval_select_recv(builder, receiver)
            }
            TExprKind::SelectAfter {
                builder,
                millis,
                value,
            } => {
                let builder = self.eval_expr(builder, scope)?;
                let millis = as_int(&self.eval_expr(millis, scope)?, self.span())?;
                let value = value
                    .as_ref()
                    .map(|value| self.eval_expr(value, scope))
                    .transpose()?
                    .unwrap_or(CtValue::Unit);
                self.eval_select_after(builder, millis, value)
            }
            TExprKind::SelectRead { builder, stream } => {
                let builder = self.eval_expr(builder, scope)?;
                let _ = self.eval_expr(stream, scope)?;
                Ok(builder)
            }
            TExprKind::SelectWait { builder } => {
                let builder = self.eval_expr(builder, scope)?;
                self.eval_select_wait(builder)
            }
            TExprKind::FnValue { kind } => match kind {
                TFnValueKind::NamedFn {
                    name: Some(name), ..
                } => Ok(self.store_callable(EvalCallable::Named(name))),
                TFnValueKind::NamedFn { name: None, .. } => {
                    Err(unsupported("rendered function coercion", self.span()))
                }
                TFnValueKind::Call { callee, args } => {
                    let callable = self.eval_expr(callee, scope)?;
                    let mut argv = Vec::with_capacity(args.len());
                    for arg in args {
                        argv.push(self.eval_expr(&arg.value, scope)?);
                    }
                    self.call_callable(&callable, argv)
                }
            },
            TExprKind::ModuleCall { form, args, .. } => {
                let target = match form {
                    TModuleCallForm::Qualified { rust_mod, rust_fn } => {
                        format!("{rust_mod}::{rust_fn}")
                    }
                    TModuleCallForm::InlineMangled { mangled } => mangled.clone(),
                };
                self.eval_call(&target, args, scope)
            }
            TExprKind::ExternCall { .. } => Err(unsupported("expr `ExternCall`", self.span())),
        }
    }

    pub(crate) fn write_back_place(
        &mut self,
        place: &'a TExpr,
        value: CtValue,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<(), Diagnostic> {
        match &place.kind {
            TExprKind::Local(local) => {
                // D-MEM1 S9 / D-PIN1=A: writing a whole-place window writes the
                // owner's storage, not the window binding.
                if let Some(handle) = scope.get(&local.name).cloned() {
                    if let Some(written) =
                        super::write_place_mut(&handle, value.clone(), scope, self.span())
                    {
                        return written;
                    }
                }
                scope.insert(local.name.clone(), value);
                Ok(())
            }
            TExprKind::Borrow { place, .. } => self.write_back_place(place, value, scope),
            // Fragment lowering (`lower_stmts_for_eval`) rewrites every binding
            // that already existed when the fragment started into a `ConstRef`
            // read from `globals`. A mutating receiver — `reader.read_u32_le()`,
            // `cursor.skip_ws()` — must write there and into the statement scope
            // the caller keeps, or the advance is dropped on the floor.
            TExprKind::ConstRef(name) if self.globals.contains_key(name) => {
                self.globals.insert(name.clone(), value.clone());
                scope.insert(name.clone(), value);
                Ok(())
            }
            TExprKind::SharedGuardValue { guard, .. } => {
                let guard = self.eval_expr(guard, scope)?;
                let (index, _, editable, path) = shared_guard_parts(&guard)
                    .ok_or_else(|| unsupported("SharedGuard write-back", self.span()))?;
                if !editable {
                    return Err(unsupported("read SharedGuard write-back", self.span()));
                }
                let runtime = self.runtime.lock().expect("evaluator runtime poisoned");
                let shared = runtime
                    .shared_values
                    .get(index)
                    .cloned()
                    .ok_or_else(|| unsupported("SharedGuard write-back", self.span()))?;
                drop(runtime);
                let mut root = shared
                    .value
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                replace_shared_projection(&mut root, &path, value)
                    .then_some(())
                    .ok_or_else(|| unsupported("SharedGuard write-back", self.span()))
            }
            TExprKind::Deref(inner) => {
                let pointer = self.eval_expr(inner, scope)?;
                let CtValue::Struct { type_name, fields } = pointer else {
                    return Err(unsupported("raw pointer carrier", self.span()));
                };
                if type_name != "__JetRawLocal" {
                    return Err(unsupported("raw pointer target", self.span()));
                }
                let name = fields.iter().find_map(|(field, value)| match (field.as_str(), value) {
                    ("name", CtValue::Str(name)) => Some(name.clone()),
                    _ => None,
                });
                let Some(name) = name else {
                    return Err(unsupported("raw pointer local", self.span()));
                };
                scope.insert(name, value);
                Ok(())
            }
            TExprKind::Field { recv, field, .. } => {
                // Single-element `__JetViewMut` write-through (`a.position = v`
                // when `a :: &xs[i]`).
                if let TExprKind::Local(local) = &recv.kind {
                    if let Some(CtValue::Struct {
                        type_name,
                        fields: vm_fields,
                    }) = scope.get(&local.name).cloned()
                    {
                        if type_name == "__JetViewMut" {
                            let mut start = None;
                            let mut end = None;
                            for (n, v) in &vm_fields {
                                match (n.as_str(), v) {
                                    ("start", CtValue::Int(n)) => start = Some(*n),
                                    ("end", CtValue::Int(n)) => end = Some(*n),
                                    _ => {}
                                }
                            }
                            if let (Some(start), Some(end)) = (start, end) {
                                if start == end {
                                    let mut items = super::load_view_mut_owner_list(
                                        &vm_fields,
                                        scope,
                                        self.span(),
                                    )?;
                                    let i = start as usize;
                                    if i >= items.len() {
                                        return Err(unsupported("view-mut OOB", self.span()));
                                    }
                                    let mut elem = items[i].clone();
                                    match &mut elem {
                                        CtValue::Struct { fields, .. } => {
                                            let mangled = crate::Codegen::mangle(field);
                                            if let Some((_, slot)) = fields.iter_mut().find(|(n, _)| {
                                                n == field
                                                    || n == &mangled
                                                    || n.strip_prefix("user_")
                                                        == Some(field.as_str())
                                            }) {
                                                *slot = value;
                                            } else {
                                                fields.push((field.clone(), value));
                                            }
                                        }
                                        _ => {
                                            return Err(unsupported(
                                                "field write-back on a non-struct",
                                                self.span(),
                                            ));
                                        }
                                    }
                                    items[i] = elem;
                                    return super::store_view_mut_owner_list(
                                        &vm_fields,
                                        scope,
                                        items,
                                        self.span(),
                                    );
                                }
                            }
                        }
                    }
                }
                let mut base_val = self.eval_expr(recv, scope)?;
                match &mut base_val {
                    CtValue::Struct {
                        type_name,
                        fields,
                    } if type_name == "__JetViewMut" => {
                        return Err(unsupported("field write-back on view-mut", self.span()));
                    }
                    CtValue::Struct { fields, .. } => {
                        let mangled = crate::Codegen::mangle(field);
                        if let Some((_, slot)) = fields.iter_mut().find(|(n, _)| {
                            n == field
                                || n == &mangled
                                || n.strip_prefix("user_") == Some(field.as_str())
                        }) {
                            *slot = value;
                        } else {
                            fields.push((field.clone(), value));
                        }
                    }
                    _ => {
                        return Err(unsupported("field write-back on a non-struct", self.span()));
                    }
                }
                self.write_back_place(recv, base_val, scope)
            }
            TExprKind::PoolSlot {
                pool,
                id,
                field,
                ..
            } => {
                let mut pool_value = self.eval_expr(pool, scope)?;
                let id_value = self.eval_expr(id, scope)?;
                let Some((index, generation)) = pool_id_parts(&id_value) else {
                    return Err(pool_stale_diagnostic());
                };
                let CtValue::Struct { fields, .. } = &mut pool_value else {
                    return Err(pool_stale_diagnostic());
                };
                let slots = fields.iter_mut().find_map(|(name, value)| match value {
                    CtValue::List(slots) if name == "slots" => Some(slots),
                    _ => None,
                });
                let Some(CtValue::Enum {
                    variant,
                    args,
                    ..
                }) = slots.and_then(|slots| slots.get_mut(index))
                else {
                    return Err(pool_stale_diagnostic());
                };
                if variant != "Occupied"
                    || !matches!(args.first(), Some((_, CtValue::Int(found))) if *found == generation)
                {
                    return Err(pool_stale_diagnostic());
                }
                let Some((_, payload)) = args.get_mut(1) else {
                    return Err(pool_stale_diagnostic());
                };
                if let Some(field) = field {
                    let CtValue::Struct { fields, .. } = payload else {
                        return Err(unsupported("Pool field on a non-struct", self.span()));
                    };
                    let slot = fields
                        .iter_mut()
                        .find_map(|(name, value)| (name == field).then_some(value))
                        .ok_or_else(|| unsupported(&format!("Pool field `{field}`"), self.span()))?;
                    *slot = value;
                } else {
                    *payload = value;
                }
                self.write_back_place(pool, pool_value, scope)
            }
            _ => Ok(()),
        }
    }

    fn eval_numeric_op(
        &self,
        v: &CtValue,
        op: &crate::Codegen::TIR::TNumericOp,
        recv_ty: &crate::AST::Type,
        result_ty: &crate::AST::Type,
    ) -> Result<CtValue, Diagnostic> {
        let _ = recv_ty;
        use crate::Codegen::TIR::TNumericOp;
        match op {
            TNumericOp::BitCount { method, width } => {
                let CtValue::Int(value) = v else {
                    return Err(unsupported("numeric bit-count recv", self.span()));
                };
                crate::Comptime::MathLayout::integer_bit_count(*value, *width, method)
                    .map(CtValue::Int)
                    .ok_or_else(|| unsupported(&format!("numeric `{method}`"), self.span()))
            }
            TNumericOp::ToShow => Ok(CtValue::Str(
                show_typed_value(v, recv_ty, false).unwrap_or_else(|| v.jet_show()),
            )),
            TNumericOp::Predicate(method) => {
                crate::Comptime::Builtins::apply_method(v, method, vec![], self.span())
            }
            TNumericOp::Origin(origin) => Ok(CtValue::Str(
                origin.clone().unwrap_or_else(|| "untracked".to_string()),
            )),
            TNumericOp::CastAs { dst_rust } => {
                // Match AOT `(({recv}) as {dst_rust})` / JIT CastAs lowering:
                // int→float and F32↔F64 must change the CtFloat width tag so
                // later math/print keep the destination precision (D-FLOATW1,
                // D-SHAPE-CONVERT1). Integer→integer stays i64 host storage.
                match dst_rust.as_str() {
                    "f64" => match v {
                        CtValue::Float(f) => Ok(CtValue::Float(CtFloat::f64(f.as_f64()))),
                        CtValue::Int(n) => Ok(CtValue::Float(CtFloat::f64(*n as f64))),
                        _ => Err(unsupported("CastAs to f64", self.span())),
                    },
                    "f32" => match v {
                        CtValue::Float(f) => Ok(CtValue::Float(CtFloat::f32(f.as_f32()))),
                        CtValue::Int(n) => Ok(CtValue::Float(CtFloat::f32(*n as f32))),
                        _ => Err(unsupported("CastAs to f32", self.span())),
                    },
                    _ => Ok(v.clone()),
                }
            }
            TNumericOp::CheckedIntToFloat {
                source_signed,
                target_f32,
                ..
            } => {
                let CtValue::Int(value) = v else {
                    return Err(unsupported("checked numeric widening expects Int", self.span()));
                };
                let Some(value) = crate::numeric_widen::jet_numeric_checked_widen(
                    *value as u64,
                    *source_signed,
                    *target_f32,
                ) else {
                    if let Some(sink) = self.sink.as_ref() {
                        let mut sink = sink.lock().expect("evaluator sink poisoned");
                        sink.stderr
                            .push_str(crate::numeric_widen::JET_NUMERIC_WIDEN_TRAP);
                        sink.stderr.push('\n');
                        sink.exit_code = Some(70);
                        return Err(Diagnostic::error(
                            "SOFT_EXIT",
                            "70".to_string(),
                            crate::numeric_widen::JET_NUMERIC_WIDEN_TRAP.to_string(),
                            String::new(),
                            Some(self.span()),
                        ));
                    }
                    return Err(unsupported(
                        crate::numeric_widen::JET_NUMERIC_WIDEN_TRAP,
                        self.span(),
                    ));
                };
                Ok(CtValue::Float(if *target_f32 {
                    CtFloat::f32(value as f32)
                } else {
                    CtFloat::f64(value)
                }))
            }
            TNumericOp::TryFrom {
                dst_spelling,
                host_kind,
                ..
            } => {
                let CtValue::Int(n) = v else {
                    return Err(unsupported("TryFrom expects Int", self.span()));
                };
                let (lo, hi) = match *host_kind {
                    0 => (i8::MIN as i64, i8::MAX as i64),
                    1 => (i16::MIN as i64, i16::MAX as i64),
                    2 => (i32::MIN as i64, i32::MAX as i64),
                    3 => (i64::MIN, i64::MAX),
                    4 => (0, u8::MAX as i64),
                    5 => (0, u16::MAX as i64),
                    6 => (0, u32::MAX as i64),
                    7 => (0, i64::MAX), // U64 in i64 domain for pure-parity
                    _ => (i64::MIN, i64::MAX),
                };
                if *n < lo || *n > hi {
                    return Ok(CtValue::failed(Box::new(CtValue::Str(format!(
                        "value doesn't fit in {dst_spelling}"
                    )))));
                }
                let _ = result_ty;
                Ok(CtValue::Present(Box::new(CtValue::Int(*n))))
            }
            TNumericOp::FloatToInt {
                dst_spelling,
                lower,
                upper_exclusive,
                ..
            } => {
                let CtValue::Float(f) = v else {
                    return Err(unsupported("FloatToInt expects Float", self.span()));
                };
                let lo: f64 = lower.parse().unwrap_or(f64::NEG_INFINITY);
                let hi: f64 = upper_exclusive.parse().unwrap_or(f64::INFINITY);
                if f.is_finite() && f.as_f64() >= lo && f.as_f64() < hi {
                    Ok(CtValue::Present(Box::new(CtValue::Int(f.as_f64().trunc() as i64))))
                } else {
                    Ok(CtValue::failed(Box::new(CtValue::Str(format!(
                        "value doesn't fit in {dst_spelling}"
                    )))))
                }
            }
            TNumericOp::FloatNarrow { dst_spelling } => {
                let CtValue::Float(f) = v else {
                    return Err(unsupported("FloatNarrow expects Float", self.span()));
                };
                let n = f.as_f64();
                if n.is_finite() && n >= -(f32::MAX as f64) && n <= f32::MAX as f64 {
                    Ok(CtValue::Present(Box::new(CtValue::Float(
                        crate::AST::CtFloat::f32(n as f32),
                    ))))
                } else {
                    Ok(CtValue::failed(Box::new(CtValue::Str(format!(
                        "value doesn't fit in {dst_spelling}"
                    )))))
                }
            }
        }
    }

    fn write_print(&mut self, text: &str, to_stderr: bool) -> Result<(), Diagnostic> {
        let Some(sink) = self.sink.as_ref() else {
            return Err(unsupported("print at comptime", self.span()));
        };
        let mut sink = sink.lock().expect("evaluator sink poisoned");
        if to_stderr {
            sink.stderr.push_str(text);
            sink.stderr.push('\n');
        } else {
            sink.stdout.push_str(text);
            sink.stdout.push('\n');
        }
        Ok(())
    }

    fn eval_call(
        &mut self,
        name: &str,
        args: &'a [TCallArg],
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<CtValue, Diagnostic> {
        let mut argv = Vec::with_capacity(args.len());
        for a in args {
            let mut v = self.eval_expr(&a.value, scope)?;
            // D-UNIONTYPE1=A: member → union inject at the call boundary (mirrors emit).
            if let Some(Type::Union(members)) = &a.widen_to_union {
                let tag = crate::AST::union_member_tag(&a.value.ty);
                if members.iter().any(|m| m == &a.value.ty) {
                    v = CtValue::Enum {
                        type_name: crate::AST::union_enum_name(members),
                        variant: tag,
                        args: vec![(None, v)],
                    };
                }
            }
            argv.push(v);
            // Try/`?` may set pending_return mid-arg; abort the call (don't print Unit).
            if self.pending_return.is_some() {
                return Ok(CtValue::Unit);
            }
        }
        // D-VERDICT-1321-1: variadic print — each argument on its own line.
        if name == "print" {
            let text = argv
                .iter()
                .map(|v| v.jet_show())
                .collect::<Vec<_>>()
                .join("\n");
            self.write_print(&text, false)?;
            return Ok(CtValue::Unit);
        }
        if name == "eprint" {
            let text = argv
                .iter()
                .map(|v| v.jet_show())
                .collect::<Vec<_>>()
                .join("\n");
            self.write_print(&text, true)?;
            return Ok(CtValue::Unit);
        }
        // D-TOOL4: `expect(x)` — wrap Display text for `.snapshot()`.
        if name == crate::Syntax::BUILTIN_EXPECT && self.funcs.get(name).is_none() {
            if argv.len() != 1 {
                return Err(unsupported("`expect` needs exactly one value", self.span()));
            }
            let shown = argv[0].jet_show();
            return Ok(CtValue::Struct {
                type_name: "__JetExpect__".to_string(),
                fields: vec![("value".into(), CtValue::Str(shown))],
            });
        }
        if name == "consume" && self.funcs.get(name).is_none() {
            if argv.len() != 1 {
                return Err(unsupported("`consume` discards exactly one value", self.span()));
            }
            return Ok(CtValue::Unit);
        }
        // D-METADERIVE1=A: `emit(source_string)` — push a re-entry fragment.
        if name == "emit" {
            let Some(CtValue::Str(s)) = argv.into_iter().next() else {
                return Err(unsupported("`emit` argument must be a string", self.span()));
            };
            let fragment = crate::Comptime::apply_dollar_splices(&s, scope);
            if let Some(out) = self.emitted_fragments.as_mut() {
                out.push(fragment);
            }
            return Ok(CtValue::Unit);
        }
        // D-CTIO1: the sanctioned build-time IO builtins. The path law, the
        // reads, and every diagnostic (E0955/E0957) stay in the one Comptime
        // implementation; this arm only marshals the literal and the sink.
        if matches!(
            name,
            crate::Syntax::BUILTIN_EMBED_FILE
                | crate::Syntax::BUILTIN_EMBED_BYTES
                | crate::Syntax::BUILTIN_FIND
        ) && self.funcs.get(name).is_none()
        {
            let literal = args.first().and_then(|arg| match &arg.value.kind {
                TExprKind::StrLit(parts) => match parts.as_slice() {
                    [super::super::TStrPart::Lit(text)] => Some(text.clone()),
                    _ => None,
                },
                _ => None,
            });
            let span = self.span();
            let base_dir = self.base_dir.clone();
            return crate::Comptime::eval_build_time_io(
                name,
                &base_dir,
                literal.as_deref(),
                self.embed_inputs.as_deref_mut(),
                span,
            );
        }
        let func = self.funcs.get(name).copied();
        // Codec-sensitive named deopts must retain the canonical migration
        // plan. Other deopts keep ordinary cross-tier native dispatch.
        if !self.prefer_tir_calls || func.is_none() {
            if let Some(hook) = super::native_call_hook() {
                if let Some(result) = hook(name, &argv) {
                    return result;
                }
            }
        }
        let Some(func) = func else {
            return Err(unsupported(&format!("call `{name}`"), self.span()));
        };
        if matches!(
            &func.ret,
            Some(Type::Apply { name, .. }) if name == crate::Syntax::TYPE_STREAM
        ) {
            return Ok(self.store_stream(func, argv));
        }
        let mut owner_rebases = HashMap::new();
        for ((parameter, _, _), argument) in func.params.iter().zip(args.iter()) {
            if let Some(owner) = super::raw_place_local(&argument.value) {
                let jet_parameter = parameter
                    .strip_prefix("user_")
                    .unwrap_or(parameter.as_str());
                owner_rebases.insert(parameter.clone(), owner.name.clone());
                owner_rebases.insert(jet_parameter.to_string(), owner.name.clone());
            }
        }
        let mut child = HashMap::new();
        let mut result = self.run_func(func, argv, &mut child)?;
        super::rebase_view_mut_owners(&mut result, &owner_rebases);
        // CtValue params are copy-in/copy-out. Fragment lowering often lacks
        // `cx.sigs`, so call-site `borrow`/`mut_borrow` flags may be false —
        // use the callee's own param conventions instead (#722).
        for ((pname, pty, conv), carg) in func.params.iter().zip(args.iter()) {
            let needs_wb = match conv {
                crate::AST::AccessConvention::Write => true,
                crate::AST::AccessConvention::Read if !pty.is_scalar() => true,
                _ => false,
            };
            if !needs_wb {
                continue;
            }
            let jet = pname.strip_prefix("user_").unwrap_or(pname.as_str());
            if let Some(updated) = child.get(jet) {
                self.write_back_place(&carg.value, updated.clone(), scope)?;
            }
        }
        Ok(result)
    }

    pub(super) fn show_value(
        &mut self,
        v: &CtValue,
        scope: &mut HashMap<String, CtValue>,
    ) -> Result<String, Diagnostic> {
        let _ = scope;
        if let CtValue::List(entries) = v {
            let mut rendered = Vec::new();
            for entry in entries {
                let CtValue::Struct { type_name, fields } = entry else { break };
                if type_name != "FieldError" {
                    rendered.clear();
                    break;
                }
                let string_field = |name: &str| {
                    fields.iter().find_map(|(field, value)| {
                        (field == name).then_some(value).and_then(|value| match value {
                            CtValue::Str(text) => Some(text.as_str()),
                            _ => None,
                        })
                    })
                };
                let path = string_field("path").unwrap_or_default();
                let reason = string_field("reason").unwrap_or_default();
                rendered.push(if path.is_empty() {
                    reason.to_string()
                } else {
                    format!("at `{path}`: {reason}")
                });
            }
            if !rendered.is_empty() {
                return Ok(format!("[{}]", rendered.join(", ")));
            }
        }
        if let Some(text) = crate::Comptime::display_core_pure_value(v) {
            return Ok(text);
        }
        // `JetDisplay` is recursive for containers.  Calling `jet_show()` on
        // the outer value loses a user Display implementation held inside a
        // list/map/option/result, which makes reflection and interpolation
        // disagree with AOT.  Keep the evaluator on the same Display path at
        // every nested value; structural user records still use their normal
        // JetShow body when they do not declare Display.
        match v {
            CtValue::Bytes(bytes) => {
                let parts = bytes.iter().map(u8::to_string).collect::<Vec<_>>();
                return Ok(format!("[{}]", parts.join(", ")));
            }
            CtValue::List(values) => {
                let parts = values
                    .iter()
                    .map(|value| self.show_value(value, scope))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(format!("[{}]", parts.join(", ")));
            }
            CtValue::Map(entries) => {
                let parts = entries
                    .iter()
                    .map(|(key, value)| {
                        Ok((
                            self.show_value(&key.to_value(), scope)?,
                            self.show_value(value, scope)?,
                        ))
                    })
                    .collect::<Result<Vec<_>, Diagnostic>>()?;
                return Ok(jet_foundation::StructuralDebug::jet_debug_map(parts));
            }
            // D-FAIL-CARRIER1=A: a payload shows as itself on both views of the
            // carrier, the same way the prelude's `JetShow` unwraps a clean
            // outcome. Only a told report needs the `Err(…)` wrapper.
            CtValue::Present(inner) => return self.show_value(inner, scope),
            CtValue::Failed(CtReport::Clean(_)) => return Ok("null".to_string()),
            CtValue::Failed(CtReport::Told(inner)) => {
                return Ok(format!("Err({})", self.show_value(inner, scope)?));
            }
            _ => {}
        }
        if let CtValue::Struct { type_name, .. } | CtValue::Enum { type_name, .. } = v {
            let key = format!("{type_name}::display");
            if let Some(func) = self.funcs.get(&key).copied() {
                let mut child = HashMap::new();
                child.insert("self".to_string(), v.clone());
                if let CtValue::Str(s) = self.run_func(func, Vec::new(), &mut child)? {
                    return Ok(s);
                }
            }
            // I2: no user `display` — render Jet-source names, the same body AOT
            // `JetShow` uses for records. `jet_show` still mirrors Rust's mangled
            // derive for the internal differential corpus.
            return Ok(self.debug_value(v));
        }
        Ok(match v {
            CtValue::Int(value) => value.to_string(),
            CtValue::Float(value) => value.render(),
            CtValue::Bool(value) => value.to_string(),
            CtValue::Char(value) => value.to_string(),
            CtValue::Str(value) => value.clone(),
            CtValue::BigInt(value) => value.to_string_rep(),
            CtValue::Unit => String::new(),
            CtValue::Closure(_) => "<closure>".to_string(),
            // All composite cases are returned above. Keep this arm explicit
            // so this method cannot silently fall back to JetShow when a new
            // CtValue variant is added.
            CtValue::Struct { .. } | CtValue::Enum { .. } => self.debug_value(v),
            CtValue::Bytes(_)
            | CtValue::List(_)
            | CtValue::Map(_)
            | CtValue::Present(_)
            | CtValue::Failed(CtReport::Clean(_))
            | CtValue::Failed(CtReport::Told(_)) => unreachable!("composite display case handled above"),
        })
    }

    pub(super) fn debug_value(&self, v: &CtValue) -> String {
        match v {
            CtValue::Struct { type_name, fields } => {
                let ty = type_name.strip_prefix("user_").unwrap_or(type_name);
                if ty == crate::Syntax::TYPE_RANGE {
                    if let Some((start, end, exclusive)) = range_parts(v) {
                        return super::range_semantics::jet_range_structural_text(
                            start,
                            end,
                            exclusive,
                        );
                    }
                }
                let Some(defs) = self.struct_fields.get(ty) else {
                    // Builtin struct with no declared fields on hand (Vec3, …).
                    // Adapt its fields to the same record assembler AOT uses.
                    let fields = fields
                        .iter()
                        .map(|(name, value)| {
                            let name = name.strip_prefix("user_").unwrap_or(name);
                            (name.to_string(), self.debug_value(value))
                        })
                        .collect::<Vec<_>>();
                    return jet_foundation::StructuralDebug::jet_debug_record(ty, fields);
                };
                let fields = defs
                    .iter()
                    .map(|(name, redact)| {
                        if *redact {
                            (name.clone(), "[redacted]".to_string())
                        } else {
                            let rendered = fields
                                .iter()
                                .find(|(n, _)| {
                                    n == name
                                        || n.strip_prefix("user_") == Some(name.as_str())
                                })
                                .map(|(_, value)| self.debug_value(value))
                                .unwrap_or_else(|| self.debug_value(&CtValue::Unit));
                            (name.clone(), rendered)
                        }
                    })
                    .collect::<Vec<_>>();
                jet_foundation::StructuralDebug::jet_debug_record(ty, fields)
            }
            CtValue::Enum {
                type_name,
                variant,
                args,
            } => {
                let ty = type_name.strip_prefix("user_").unwrap_or(type_name);
                let var = variant.strip_prefix("user_").unwrap_or(variant);
                if ty.starts_with("__JetUnion_") {
                    let payload = args
                        .first()
                        .map(|(_, value)| self.debug_value(value))
                        .unwrap_or_default();
                    return jet_foundation::StructuralDebug::jet_debug_union(payload);
                }
                if ty == "IOError" {
                    let parts: Vec<String> = args
                        .iter()
                        .map(|(_, value)| match value {
                            CtValue::Struct { type_name, fields }
                                if type_name
                                    .strip_prefix("user_")
                                    .unwrap_or(type_name)
                                    == "IOContext" =>
                            {
                                let fields: Vec<String> =
                                    ["operation", "resource", "os_code", "cause"]
                                    .iter()
                                    .map(|wanted| {
                                        let value = fields.iter().find_map(|(name, value)| {
                                            (name.strip_prefix("user_").unwrap_or(name)
                                                == *wanted)
                                                .then_some(value)
                                        });
                                        let value = match value {
                                            Some(CtValue::Enum { variant, args, .. })
                                                if args.is_empty() =>
                                            {
                                                variant
                                                    .strip_prefix("user_")
                                                    .unwrap_or(variant)
                                                    .to_string()
                                            }
                                            Some(value) => value.debug_rust(),
                                            None => CtValue::Unit.debug_rust(),
                                        };
                                        format!("{wanted}: {value}")
                                    })
                                    .collect();
                                format!("IOContext {{ {} }}", fields.join(", "))
                            }
                            _ => value.debug_rust(),
                        })
                        .collect();
                    return if parts.is_empty() {
                        var.to_string()
                    } else {
                        format!("{var}({})", parts.join(", "))
                    };
                }
                // Bare variant, matching AOT `JetShow`/`JetDebug` for enums.
                let _ = ty;
                if args.is_empty() {
                    var.to_string()
                } else if args.iter().all(|(label, _)| label.is_some()) {
                    let parts: Vec<String> = args
                        .iter()
                        .map(|(label, val)| {
                            format!(
                                "{}: {}",
                                label.as_deref().unwrap_or(""),
                                self.debug_value(val)
                            )
                        })
                        .collect();
                    format!("{var} {{ {} }}", parts.join(", "))
                } else {
                    let parts: Vec<String> = args
                        .iter()
                        .map(|(_, val)| self.debug_value(val))
                        .collect();
                    format!("{var}({})", parts.join(", "))
                }
            }
            CtValue::Map(entries) => jet_foundation::StructuralDebug::jet_debug_map(
                entries.iter().map(|(key, value)| {
                    (
                        self.debug_value(&key.to_value()),
                        self.debug_value(value),
                    )
                }),
            ),
            _ => v.debug_rust(),
        }
    }

}

fn pool_id_parts(value: &CtValue) -> Option<(usize, i64)> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if type_name != "Id" {
        return None;
    }
    let int_field = |wanted: &str| {
        fields.iter().find_map(|(name, value)| match value {
            CtValue::Int(value) if name == wanted => Some(*value),
            _ => None,
        })
    };
    Some((usize::try_from(int_field("index")?).ok()?, int_field("generation")?))
}

fn pool_stale_diagnostic() -> Diagnostic {
    Diagnostic::error(
        "E0953",
        "your comptime code stopped the build".to_string(),
        "while computing this value at compile time, the program panicked: this Id no longer refers to a live value — its pool slot was removed".to_string(),
        "this is the sanctioned way to validate at compile time — fix the input the check rejects"
            .to_string(),
        None,
    )
}

fn eval_precise_builtin(
    type_name: &str,
    func: &str,
    args: Vec<CtValue>,
    span: crate::Diagnostics::Span,
) -> Result<CtValue, Diagnostic> {
    use jet_foundation::Numeric::{CtBigInt, CtDecimal};
    match (type_name, func) {
        ("BigInt", "from_int") => match args.into_iter().next() {
            Some(CtValue::Int(n)) => Ok(CtValue::BigInt(CtBigInt::from_int(n))),
            _ => Err(unsupported("`BigInt.from_int`", span)),
        },
        ("BigInt", "from_str") => match args.into_iter().next() {
            Some(CtValue::Str(s)) => CtBigInt::from_str(&s)
                .map(CtValue::BigInt)
                .map_err(|_| unsupported(&format!("`BigInt(\"{s}\")`"), span)),
            _ => Err(unsupported("`BigInt.from_str`", span)),
        },
        ("Decimal", "from_str") => match args.into_iter().next() {
            Some(CtValue::Str(s)) => CtDecimal::from_str(&s)
                .map(|d| d.to_value())
                .map_err(|_| unsupported(&format!("`Decimal(\"{s}\")`"), span)),
            _ => Err(unsupported("`Decimal.from_str`", span)),
        },
        ("BigInt" | "Decimal" | "Fraction", "add" | "sub" | "mul" | "neg" | "to_string")
        | (
            "Fraction",
            "div" | "equal" | "numerator" | "denominator" | "to_float" | "is_zero",
        ) => {
            let mut it = args.into_iter();
            let Some(recv) = it.next() else {
                return Err(unsupported(&format!("`{type_name}.{func}`"), span));
            };
            let rest: Vec<_> = it.collect();
            crate::Comptime::Builtins::apply_method(&recv, func, rest, span)
        }
        _ => Err(unsupported(
            &format!("precise `{type_name}.{func}`"),
            span,
        )),
    }
}
