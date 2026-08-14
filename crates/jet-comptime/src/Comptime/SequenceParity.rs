//! Card #392 packet A: effect-free List/FixedList/View/ViewMut method parity.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{BinOp, CtFloat, CtKey, CtReport, CtValue, Type};

use super::super::super::Builtins::{as_bool, as_int, cmp};
use super::super::super::Diagnostics::{index_oob, overflow, unsupported};
use super::super::super::Interpreter::Interp;

pub(super) enum SequenceOutcome {
    Value(CtValue),
    WriteBack(CtValue),
}

pub(super) fn eval_sequence_method(
    interp: &mut Interp<'_>,
    recv: &CtValue,
    method: &str,
    args: &[CtValue],
    arg_labels: &[Option<String>],
    resolved_ret: Option<&Type>,
    span: Span,
    scope: &mut HashMap<String, CtValue>,
) -> Option<Result<SequenceOutcome, Diagnostic>> {
    let CtValue::List(xs) = recv else {
        return None;
    };
    if !matches!(
        method,
        "all"
            | "any"
            | "chunks"
            | "count_by"
            | "dedup"
            | "indexed"
            | "indexes"
            | "filter_map"
            | "first"
            | "flat_map"
            | "flatten"
            | "fold"
            | "group_by"
            | "index_of"
            | "insert"
            | "intersperse"
            | "last"
            | "max"
            | "max_by"
            | "min"
            | "min_by"
            | "para_filter"
            | "para_fold"
            | "para_map"
            | "para_partition"
            | "partition"
            | "position"
            | "product"
            | "reduce"
            | "scan"
            | "skip"
            | "skip_while"
            | "step_by"
            | "sum"
            | "take"
            | "take_while"
            | "try_collect"
            | "unzip"
            | "windows"
            | "zip"
            | "zip_short"
            | "zip_pad"
    ) {
        return None;
    }
    if method == "insert" {
        return Some((|| {
            let [index, item] = args else {
                return Err(unsupported("the method `.insert` with these arguments", span));
            };
            let index = as_int(index, span)?;
            if index < 0 || index as usize > xs.len() {
                return Err(index_oob(xs.len(), index, span));
            }
            let mut out = xs.to_vec();
            out.insert(index as usize, item.clone());
            Ok(SequenceOutcome::WriteBack(CtValue::List(out)))
        })());
    }
    Some(
        eval(
            interp,
            xs,
            method,
            args,
            arg_labels,
            resolved_ret,
            span,
            scope,
        )
        .map(SequenceOutcome::Value),
    )
}

fn eval(
    interp: &mut Interp<'_>,
    xs: &[CtValue],
    method: &str,
    args: &[CtValue],
    arg_labels: &[Option<String>],
    resolved_ret: Option<&Type>,
    span: Span,
    scope: &mut HashMap<String, CtValue>,
) -> Result<CtValue, Diagnostic> {
    let value = match (method, args) {
        ("all", [f]) => {
            for x in xs {
                if !as_bool(
                    &interp.call_inline_closure(f, vec![x.clone()], span, scope)?,
                    span,
                )? {
                    return Ok(CtValue::Bool(false));
                }
            }
            CtValue::Bool(true)
        }
        ("any", [f]) => {
            for x in xs {
                if as_bool(
                    &interp.call_inline_closure(f, vec![x.clone()], span, scope)?,
                    span,
                )? {
                    return Ok(CtValue::Bool(true));
                }
            }
            CtValue::Bool(false)
        }
        ("chunks", [n]) => {
            let n = as_int(n, span)?.max(1) as usize;
            CtValue::List(xs.chunks(n).map(|chunk| CtValue::List(chunk.to_vec())).collect())
        }
        ("count_by", [f]) => {
            let mut out = BTreeMap::new();
            for x in xs {
                let key = key(
                    interp.call_inline_closure(f, vec![x.clone()], span, scope)?,
                    span,
                )?;
                let count = out.entry(key).or_insert(CtValue::Int(0));
                let CtValue::Int(count) = count else { unreachable!() };
                *count += 1;
            }
            CtValue::Map(out)
        }
        ("dedup", []) => {
            let mut out = Vec::new();
            for x in xs {
                if out.last() != Some(x) {
                    out.push(x.clone());
                }
            }
            CtValue::List(out)
        }
        ("indexed", []) => CtValue::List(
            xs.iter()
                .enumerate()
                .map(|(idx, item)| tuple(vec![
                    ("idx", CtValue::Int(idx as i64)),
                    ("item", item.clone()),
                ]))
                .collect(),
        ),
        ("indexes", []) => CtValue::List(
            (0..xs.len())
                .map(|idx| CtValue::Int(idx as i64))
                .collect(),
        ),
        ("filter_map", [f]) => {
            let mut out = Vec::new();
            for x in xs {
                match interp.call_inline_closure(f, vec![x.clone()], span, scope)? {
                    CtValue::Present(value) => out.push(*value),
                    CtValue::Failed(CtReport::Told(_)) => {}
                    _ => {
                        return Err(unsupported(
                            "filter_map callback returning a non-Result",
                            span,
                        ))
                    }
                }
            }
            CtValue::List(out)
        }
        ("first", []) => option(
            crate::Comptime::CollectionEval::iter_first(xs.to_vec()),
            xs,
        ),
        ("flat_map", [f]) => {
            let mut out = Vec::new();
            for x in xs {
                let CtValue::List(values) =
                    interp.call_inline_closure(f, vec![x.clone()], span, scope)?
                else {
                    return Err(unsupported("flat_map callback returning a non-list", span));
                };
                out.extend(values);
            }
            CtValue::List(out)
        }
        ("flatten", []) => {
            let mut out = Vec::new();
            for x in xs {
                let CtValue::List(values) = x else {
                    return Err(unsupported("flatten on a non-nested list", span));
                };
                out.extend(values.iter().cloned());
            }
            CtValue::List(out)
        }
        ("reduce" | "fold", [initial, f]) => {
            let mut acc = initial.clone();
            for x in xs {
                acc = interp.call_inline_closure(f, vec![acc, x.clone()], span, scope)?;
            }
            acc
        }
        ("para_fold", [seed, step, merge]) => {
            const CHUNK_ITEMS: usize = 64;
            if xs.is_empty() {
                interp.call_closure(seed, vec![], span)?
            } else {
                let mut partials = Vec::new();
                for chunk in xs.chunks(CHUNK_ITEMS) {
                    let mut acc = interp.call_closure(seed, vec![], span)?;
                    for x in chunk {
                        acc = interp.call_closure(step, vec![acc, x.clone()], span)?;
                    }
                    partials.push(acc);
                }
                while partials.len() > 1 {
                    let mut next = Vec::with_capacity((partials.len() + 1) / 2);
                    let mut values = partials.into_iter();
                    while let Some(left) = values.next() {
                        if let Some(right) = values.next() {
                            next.push(interp.call_closure(merge, vec![left, right], span)?);
                        } else {
                            next.push(left);
                        }
                    }
                    partials = next;
                }
                partials.pop().expect("non-empty input makes one partial")
            }
        }
        ("group_by", [f]) => {
            let mut out = BTreeMap::new();
            for x in xs {
                let key = key(
                    interp.call_inline_closure(f, vec![x.clone()], span, scope)?,
                    span,
                )?;
                match out.entry(key).or_insert_with(|| CtValue::List(Vec::new())) {
                    CtValue::List(group) => group.push(x.clone()),
                    _ => unreachable!(),
                }
            }
            CtValue::Map(out)
        }
        ("index_of", [needle]) => option(
            xs.iter()
                .position(|x| x == needle)
                .map(|index| CtValue::Int(index as i64)),
            &[],
        ),
        ("intersperse", [separator]) => {
            let mut out = Vec::with_capacity(xs.len().saturating_mul(2).saturating_sub(1));
            for (index, x) in xs.iter().enumerate() {
                if index != 0 {
                    out.push(separator.clone());
                }
                out.push(x.clone());
            }
            CtValue::List(out)
        }
        ("last", []) => option(xs.last().cloned(), xs),
        ("min", []) => option(extreme(xs, false, span)?, xs),
        ("max", []) => option(extreme(xs, true, span)?, xs),
        ("min_by", [f]) => option(extreme_by(interp, xs, f, false, span, scope)?, xs),
        ("max_by", [f]) => option(extreme_by(interp, xs, f, true, span, scope)?, xs),
        ("para_filter", [f]) => {
            let mut out = Vec::new();
            for x in xs {
                if as_bool(&interp.call_closure(f, vec![x.clone()], span)?, span)? {
                    out.push(x.clone());
                }
            }
            CtValue::List(out)
        }
        ("para_map", [f]) => {
            let mut out = Vec::with_capacity(xs.len());
            for x in xs {
                out.push(interp.call_closure(f, vec![x.clone()], span)?);
            }
            CtValue::List(out)
        }
        ("partition" | "para_partition", [f]) => {
            let mut no = Vec::new();
            let mut yes = Vec::new();
            for x in xs {
                if as_bool(
                    &interp.call_inline_closure(f, vec![x.clone()], span, scope)?,
                    span,
                )? {
                    yes.push(x.clone());
                } else {
                    no.push(x.clone());
                }
            }
            tuple(vec![("false_", CtValue::List(no)), ("true_", CtValue::List(yes))])
        }
        ("position", [f]) => {
            let mut found = None;
            for (index, x) in xs.iter().enumerate() {
                if as_bool(
                    &interp.call_inline_closure(f, vec![x.clone()], span, scope)?,
                    span,
                )? {
                    found = Some(CtValue::Int(index as i64));
                    break;
                }
            }
            option(found, &[])
        }
        ("sum", []) => aggregate(xs, false, resolved_ret, span)?,
        ("product", []) => aggregate(xs, true, resolved_ret, span)?,
        ("scan", [initial, f]) => {
            let mut acc = initial.clone();
            let mut out = Vec::with_capacity(xs.len());
            for x in xs {
                acc = interp.call_inline_closure(f, vec![acc, x.clone()], span, scope)?;
                out.push(acc.clone());
            }
            CtValue::List(out)
        }
        ("take", [n]) => {
            let n = as_int(n, span)?.max(0) as usize;
            CtValue::List(xs.iter().take(n).cloned().collect())
        }
        ("skip", [n]) => {
            let n = as_int(n, span)?.max(0) as usize;
            CtValue::List(crate::Comptime::CollectionEval::iter_skip(
                xs.to_vec(),
                n as i64,
            ))
        }
        ("step_by", [n]) => {
            let n = as_int(n, span)?;
            CtValue::List(if n <= 0 {
                Vec::new()
            } else {
                xs.iter().step_by(n as usize).cloned().collect()
            })
        }
        ("take_while", [f]) => {
            let mut out = Vec::new();
            for x in xs {
                if !as_bool(
                    &interp.call_inline_closure(f, vec![x.clone()], span, scope)?,
                    span,
                )? {
                    break;
                }
                out.push(x.clone());
            }
            CtValue::List(out)
        }
        ("skip_while", [f]) => {
            let mut skipping = true;
            let mut out = Vec::new();
            for x in xs {
                if skipping {
                    skipping = as_bool(
                        &interp.call_inline_closure(f, vec![x.clone()], span, scope)?,
                        span,
                    )?;
                }
                if !skipping {
                    out.push(x.clone());
                }
            }
            CtValue::List(out)
        }
        ("try_collect", []) => {
            let mut out = Vec::with_capacity(xs.len());
            for x in xs {
                match x {
                    CtValue::Present(value) => out.push((**value).clone()),
                    CtValue::Failed(CtReport::Told(error)) => return Ok(CtValue::failed(error.clone())),
                    _ => return Err(unsupported("try_collect on a non-Result list", span)),
                }
            }
            CtValue::Present(Box::new(CtValue::List(out)))
        }
        ("unzip", []) => {
            let mut left = Vec::with_capacity(xs.len());
            let mut right = Vec::with_capacity(xs.len());
            for x in xs {
                let CtValue::Struct { fields, .. } = x else {
                    return Err(unsupported("unzip on a non-tuple list", span));
                };
                let a = fields.iter().find(|(name, _)| name == "a").map(|(_, v)| v.clone());
                let b = fields.iter().find(|(name, _)| name == "b").map(|(_, v)| v.clone());
                match (a, b) {
                    (Some(a), Some(b)) => {
                        left.push(a);
                        right.push(b);
                    }
                    _ => return Err(unsupported("unzip on a tuple without `a` and `b`", span)),
                }
            }
            tuple(vec![("a", CtValue::List(left)), ("b", CtValue::List(right))])
        }
        ("windows", [n]) => {
            let n = as_int(n, span)?.max(1) as usize;
            CtValue::List(if n > xs.len() {
                Vec::new()
            } else {
                xs.windows(n).map(|window| CtValue::List(window.to_vec())).collect()
            })
        }
        ("zip" | "zip_short" | "zip_pad", _) => {
            eval_zip(xs, method, args, arg_labels, span)?
        }
        _ => return Err(unsupported(&format!("the method `.{method}` with these arguments"), span)),
    };
    Ok(value)
}

fn eval_zip(
    first: &[CtValue],
    method: &str,
    args: &[CtValue],
    arg_labels: &[Option<String>],
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let mut columns = vec![first.to_vec()];
    let mut common_fill = None;
    let mut column_fills = None;
    for (index, value) in args.iter().enumerate() {
        match arg_labels.get(index).and_then(Option::as_deref) {
            Some("fill") => common_fill = Some(value.clone()),
            Some("fills") => column_fills = Some(value.clone()),
            _ => match value {
                CtValue::List(values) => columns.push(values.clone()),
                _ => {
                    return Err(unsupported(
                        "zip with a non-list argument",
                        span,
                    ))
                }
            },
        }
    }

    if method == "zip" && columns.iter().any(|column| column.len() != columns[0].len()) {
        return Err(Diagnostic::error(
            "E0128",
            "zip inputs have different lengths".to_string(),
            "strict `zip` requires every input to end on the same row".to_string(),
            "use `zip_short` or `zip_pad` when lengths may differ".to_string(),
            Some(span),
        ));
    }

    let row_count = match method {
        "zip_pad" => columns.iter().map(Vec::len).max().unwrap_or(0),
        _ => columns.iter().map(Vec::len).min().unwrap_or(0),
    };
    if columns.len() == 1 {
        return Ok(CtValue::List(columns.pop().unwrap_or_default()));
    }
    let fields = (0..columns.len())
        .map(|index| zip_field_name(index).to_string())
        .collect::<Vec<_>>();
    let fill_for = |index: usize| -> CtValue {
        if let Some(value) = &common_fill {
            return value.clone();
        }
        if let Some(CtValue::Struct { fields, .. }) = &column_fills {
            let field = zip_field_name(index);
            if let Some((_, value)) = fields.iter().find(|(name, _)| {
                name == &field || name.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX) == Some(field.as_str())
            }) {
                return value.clone();
            }
        }
        CtValue::absent(
            columns[index]
                .first()
                .map(CtValue::jet_type)
                .unwrap_or(Type::Int),
        )
    };
    let rows = (0..row_count)
        .map(|row| {
            tuple(
                fields
                    .iter()
                    .enumerate()
                    .map(|(index, field)| {
                        (
                            field.as_str(),
                            columns[index]
                                .get(row)
                                .cloned()
                                .unwrap_or_else(|| fill_for(index)),
                        )
                    })
                    .collect(),
            )
        })
        .collect();
    Ok(CtValue::List(rows))
}

fn zip_field_name(index: usize) -> String {
    match index {
        0 => "a".to_string(),
        1 => "b".to_string(),
        2 => "c".to_string(),
        3 => "d".to_string(),
        4 => "e".to_string(),
        5 => "f".to_string(),
        n => format!("column_{}", n + 1),
    }
}

fn tuple(fields: Vec<(&str, CtValue)>) -> CtValue {
    let type_name = format!(
        "({})",
        fields.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(",")
    );
    CtValue::Struct {
        type_name,
        fields: fields
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
    }
}

fn key(value: CtValue, span: Span) -> Result<CtKey, Diagnostic> {
    CtKey::from_value(value).ok_or_else(|| unsupported("this map key type", span))
}

fn option(value: Option<CtValue>, xs: &[CtValue]) -> CtValue {
    value.map_or_else(
        || CtValue::absent(xs.first().map(CtValue::jet_type).unwrap_or(Type::Int)),
        |value| CtValue::Present(Box::new(value)),
    )
}

fn extreme(xs: &[CtValue], maximum: bool, span: Span) -> Result<Option<CtValue>, Diagnostic> {
    let Some(mut best) = xs.first().cloned() else {
        return Ok(None);
    };
    for candidate in &xs[1..] {
        let order = cmp(best.clone(), candidate.clone(), span)?;
        if (maximum && order != Ordering::Greater) || (!maximum && order == Ordering::Greater) {
            best = candidate.clone();
        }
    }
    Ok(Some(best))
}

fn extreme_by(
    interp: &mut Interp<'_>,
    xs: &[CtValue],
    f: &CtValue,
    maximum: bool,
    span: Span,
    scope: &mut HashMap<String, CtValue>,
) -> Result<Option<CtValue>, Diagnostic> {
    let Some(mut best) = xs.first().cloned() else {
        return Ok(None);
    };
    let mut best_key = interp.call_inline_closure(f, vec![best.clone()], span, scope)?;
    for candidate in &xs[1..] {
        let candidate_key =
            interp.call_inline_closure(f, vec![candidate.clone()], span, scope)?;
        let order = cmp(best_key.clone(), candidate_key.clone(), span)?;
        if (maximum && order != Ordering::Greater) || (!maximum && order == Ordering::Greater) {
            best = candidate.clone();
            best_key = candidate_key;
        }
    }
    Ok(Some(best))
}

fn aggregate(
    xs: &[CtValue],
    product: bool,
    resolved_ret: Option<&Type>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let Some(first) = xs.first() else {
        return Ok(match resolved_ret {
            Some(Type::Float) => CtValue::Float(CtFloat::f64(if product { 1.0 } else { 0.0 })),
            Some(Type::Float32) => {
                CtValue::Float(CtFloat::f32(if product { 1.0 } else { 0.0 }))
            }
            Some(Type::Named(name)) if name == crate::Syntax::TYPE_BIGINT => {
                CtValue::BigInt(crate::Numeric::CtBigInt::from_int(if product { 1 } else { 0 }))
            }
            _ => CtValue::Int(if product { 1 } else { 0 }),
        });
    };
    match first {
        CtValue::Int(_) => {
            let mut acc: i64 = if product { 1 } else { 0 };
            for x in xs {
                let CtValue::Int(value) = x else {
                    return Err(unsupported("sum/product on mixed numeric types", span));
                };
                acc = if product {
                    acc.checked_mul(*value).ok_or_else(|| overflow("multiply", span))?
                } else {
                    acc.checked_add(*value).ok_or_else(|| overflow("add", span))?
                };
            }
            Ok(CtValue::Int(acc))
        }
        CtValue::Float(first) => {
            let mut acc = match first {
                CtFloat::F32(_) => CtFloat::f32(if product { 1.0 } else { 0.0 }),
                CtFloat::F64(_) => CtFloat::f64(if product { 1.0 } else { 0.0 }),
            };
            for x in xs {
                let CtValue::Float(value) = x else {
                    return Err(unsupported("sum/product on mixed numeric types", span));
                };
                acc = acc
                    .binop(if product { BinOp::Mul } else { BinOp::Add }, *value)
                    .ok_or_else(|| unsupported("sum/product on mixed float widths", span))?;
            }
            Ok(CtValue::Float(acc))
        }
        CtValue::BigInt(_) => {
            let mut acc = crate::Numeric::CtBigInt::from_int(if product { 1 } else { 0 });
            for x in xs {
                let CtValue::BigInt(value) = x else {
                    return Err(unsupported("sum/product on mixed numeric types", span));
                };
                acc = if product { acc.mul(value) } else { acc.add(value) };
            }
            Ok(CtValue::BigInt(acc))
        }
        _ => Err(unsupported("sum/product on non-numeric values", span)),
    }
}
