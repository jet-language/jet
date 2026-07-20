//! Card #392 packet A: effect-free List/FixedList/View/ViewMut method parity.

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap};

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{CtKey, CtValue, Type};

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
            | "enumerate"
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
            | "par_filter"
            | "par_fold"
            | "par_map"
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
    Some(eval(interp, xs, method, args, resolved_ret, span, scope).map(SequenceOutcome::Value))
}

fn eval(
    interp: &mut Interp<'_>,
    xs: &[CtValue],
    method: &str,
    args: &[CtValue],
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
        ("enumerate", []) => CtValue::List(
            xs.iter()
                .enumerate()
                .map(|(idx, item)| tuple(vec![
                    ("idx", CtValue::Int(idx as i64)),
                    ("item", item.clone()),
                ]))
                .collect(),
        ),
        ("filter_map", [f]) => {
            let mut out = Vec::new();
            for x in xs {
                match interp.call_inline_closure(f, vec![x.clone()], span, scope)? {
                    CtValue::ResOk(value) => out.push(*value),
                    CtValue::ResErr(_) => {}
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
        ("first", []) => option(xs.first().cloned(), xs),
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
        ("par_fold", [initial, f]) => {
            let mut acc = initial.clone();
            for x in xs {
                acc = interp.call_closure(f, vec![acc, x.clone()], span)?;
            }
            acc
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
        ("par_filter", [f]) => {
            let mut out = Vec::new();
            for x in xs {
                if as_bool(&interp.call_closure(f, vec![x.clone()], span)?, span)? {
                    out.push(x.clone());
                }
            }
            CtValue::List(out)
        }
        ("par_map", [f]) => {
            let mut out = Vec::with_capacity(xs.len());
            for x in xs {
                out.push(interp.call_closure(f, vec![x.clone()], span)?);
            }
            CtValue::List(out)
        }
        ("partition", [f]) => {
            let mut no = Vec::new();
            let mut yes = Vec::new();
            for x in xs {
                if as_bool(&interp.call_closure(f, vec![x.clone()], span)?, span)? {
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
            CtValue::List(xs.iter().skip(n).cloned().collect())
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
                    CtValue::ResOk(value) => out.push((**value).clone()),
                    CtValue::ResErr(error) => return Ok(CtValue::ResErr(error.clone())),
                    _ => return Err(unsupported("try_collect on a non-Result list", span)),
                }
            }
            CtValue::ResOk(Box::new(CtValue::List(out)))
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
        ("zip", [CtValue::List(other)]) => CtValue::List(
            xs.iter()
                .zip(other)
                .map(|(a, b)| tuple(vec![("a", a.clone()), ("b", b.clone())]))
                .collect(),
        ),
        _ => return Err(unsupported(&format!("the method `.{method}` with these arguments"), span)),
    };
    Ok(value)
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
        || CtValue::None(xs.first().map(CtValue::jet_type).unwrap_or(Type::Int)),
        |value| CtValue::Some(Box::new(value)),
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
            Some(Type::Float | Type::Float32) => {
                CtValue::Float(if product { 1.0 } else { 0.0 })
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
        CtValue::Float(_) => {
            let mut acc = if product { 1.0 } else { 0.0 };
            for x in xs {
                let CtValue::Float(value) = x else {
                    return Err(unsupported("sum/product on mixed numeric types", span));
                };
                acc = if product { acc * value } else { acc + value };
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
