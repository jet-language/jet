//! Binary operators, comparisons, builtin method dispatch, and the `as_*`
//! coercions shared by the interpreter spine.

use crate::AST::{BinOp, Type};
use crate::Diagnostics::{Diagnostic, Span};

use super::Diagnostics::{divide_by_zero, index_oob, overflow, unsupported};
use super::Value::{CtKey, CtValue};

pub(super) fn as_bool(v: &CtValue, span: Span) -> Result<bool, Diagnostic> {
    match v {
        CtValue::Bool(b) => Ok(*b),
        _ => Err(unsupported("a non-Bool used as a condition", span)),
    }
}

pub(super) fn as_int(v: &CtValue, span: Span) -> Result<i64, Diagnostic> {
    match v {
        CtValue::Int(n) => Ok(*n),
        _ => Err(unsupported("a non-Int used as a number", span)),
    }
}

/// Binary operators with runtime-identical semantics (i64 wrapping is
/// rejected: debug-profile rustc panics on overflow, so comptime does too).
pub(super) fn eval_binop(op: BinOp, l: CtValue, r: CtValue, span: Span) -> Result<CtValue, Diagnostic> {
    use CtValue::*;
    match (op, l, r) {
        (BinOp::Add, Int(a), Int(b)) => a
            .checked_add(b)
            .map(Int)
            .ok_or_else(|| overflow("add", span)),
        (BinOp::Sub, Int(a), Int(b)) => a
            .checked_sub(b)
            .map(Int)
            .ok_or_else(|| overflow("subtract", span)),
        (BinOp::Mul, Int(a), Int(b)) => a
            .checked_mul(b)
            .map(Int)
            .ok_or_else(|| overflow("multiply", span)),
        (BinOp::Div, Int(_), Int(0)) => Err(divide_by_zero(span)),
        (BinOp::Div, Int(a), Int(b)) => a
            .checked_div(b)
            .map(Int)
            .ok_or_else(|| overflow("divide", span)),
        (BinOp::Rem, Int(_), Int(0)) => Err(divide_by_zero(span)),
        (BinOp::Rem, Int(a), Int(b)) => a
            .checked_rem(b)
            .map(Int)
            .ok_or_else(|| overflow("take the remainder of", span)),
        (BinOp::BitAnd, Int(a), Int(b)) => Ok(Int(a & b)),
        (BinOp::BitOr, Int(a), Int(b)) => Ok(Int(a | b)),
        (BinOp::BitXor, Int(a), Int(b)) => Ok(Int(a ^ b)),
        (BinOp::Shl, Int(a), Int(b)) => Ok(Int(a.wrapping_shl(b as u32))),
        (BinOp::Shr, Int(a), Int(b)) => Ok(Int(a.wrapping_shr(b as u32))),
        (BinOp::Add, Float(a), Float(b)) => Ok(Float(a + b)),
        (BinOp::Sub, Float(a), Float(b)) => Ok(Float(a - b)),
        (BinOp::Mul, Float(a), Float(b)) => Ok(Float(a * b)),
        (BinOp::Div, Float(a), Float(b)) => Ok(Float(a / b)),
        (BinOp::Eq, a, b) => Ok(Bool(a == b)),
        (BinOp::Ne, a, b) => Ok(Bool(a != b)),
        (BinOp::Lt, a, b) => cmp(a, b, span).map(|o| Bool(o == std::cmp::Ordering::Less)),
        (BinOp::Gt, a, b) => cmp(a, b, span).map(|o| Bool(o == std::cmp::Ordering::Greater)),
        (BinOp::Le, a, b) => cmp(a, b, span).map(|o| Bool(o != std::cmp::Ordering::Greater)),
        (BinOp::Ge, a, b) => cmp(a, b, span).map(|o| Bool(o != std::cmp::Ordering::Less)),
        _ => Err(unsupported("this operation", span)),
    }
}

fn cmp(a: CtValue, b: CtValue, span: Span) -> Result<std::cmp::Ordering, Diagnostic> {
    use CtValue::*;
    match (a, b) {
        (Int(a), Int(b)) => Ok(a.cmp(&b)),
        (Float(a), Float(b)) => a
            .partial_cmp(&b)
            .ok_or_else(|| unsupported("comparing NaN", span)),
        (Char(a), Char(b)) => Ok(a.cmp(&b)),
        (Str(a), Str(b)) => Ok(a.cmp(&b)),
        _ => Err(unsupported("comparing these values", span)),
    }
}

/// Mutating list/map methods (`push`, `pop`, …). Returns the method's value.
pub(super) fn apply_mutating(
    recv: &mut CtValue,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    match (recv, method) {
        (CtValue::List(xs), "push") => {
            xs.push(args.into_iter().next().unwrap_or(CtValue::Unit));
            Ok(CtValue::Unit)
        }
        (CtValue::List(xs), "pop") => Ok(match xs.pop() {
            Some(v) => CtValue::Some(Box::new(v)),
            None => CtValue::None(Type::Int),
        }),
        (CtValue::List(xs), "reverse") => {
            xs.reverse();
            Ok(CtValue::Unit)
        }
        (CtValue::List(xs), "sort") => {
            xs.sort_by(|a, b| cmp(a.clone(), b.clone(), span).unwrap_or(std::cmp::Ordering::Equal));
            Ok(CtValue::Unit)
        }
        (CtValue::List(xs), "clear") => {
            xs.clear();
            Ok(CtValue::Unit)
        }
        (CtValue::List(xs), "remove") => {
            let i = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            if i < 0 || i as usize >= xs.len() {
                return Err(index_oob(xs.len(), i, span));
            }
            Ok(xs.remove(i as usize))
        }
        (CtValue::Map(m), "insert") => {
            let mut it = args.into_iter();
            let k = CtKey::from_value(it.next().unwrap_or(CtValue::Unit))
                .ok_or_else(|| unsupported("this map key type", span))?;
            let v = it.next().unwrap_or(CtValue::Unit);
            m.insert(k, v);
            Ok(CtValue::Unit)
        }
        (CtValue::Map(m), "remove") => {
            let k = CtKey::from_value(args.into_iter().next().unwrap_or(CtValue::Unit))
                .ok_or_else(|| unsupported("this map key type", span))?;
            m.remove(&k);
            Ok(CtValue::Unit)
        }
        _ => Err(unsupported(
            &format!("the method `.{}` at compile time", method),
            span,
        )),
    }
}

/// Non-mutating methods on values.
pub(super) fn apply_method(
    recv: &CtValue,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    match (recv, method) {
        // Universal
        (v, "to_string") => Ok(CtValue::Str(v.jet_show())),
        // Int / Float conversions
        (CtValue::Int(n), "to_float") => Ok(CtValue::Float(*n as f64)),
        (CtValue::Int(n), "abs") => n
            .checked_abs()
            .map(CtValue::Int)
            .ok_or_else(|| overflow("take the absolute value of", span)),
        (CtValue::Float(f), "to_int") => Ok(CtValue::Int(*f as i64)),
        (CtValue::Float(f), "abs") => Ok(CtValue::Float(f.abs())),
        // List
        (CtValue::List(xs), "len") => Ok(CtValue::Int(xs.len() as i64)),
        (CtValue::List(xs), "is_empty") => Ok(CtValue::Bool(xs.is_empty())),
        (CtValue::List(xs), "get") => {
            let i = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            Ok(if i < 0 || i as usize >= xs.len() {
                CtValue::None(xs.first().map(|v| v.jet_type()).unwrap_or(Type::Int))
            } else {
                CtValue::Some(Box::new(xs[i as usize].clone()))
            })
        }
        (CtValue::List(xs), "contains") => {
            let needle = args.into_iter().next().unwrap_or(CtValue::Unit);
            Ok(CtValue::Bool(xs.iter().any(|x| *x == needle)))
        }
        (CtValue::List(xs), "join") => {
            let sep = match args.into_iter().next() {
                Some(CtValue::Str(s)) => s,
                _ => String::new(),
            };
            let parts: Vec<String> = xs.iter().map(|x| x.jet_show()).collect();
            Ok(CtValue::Str(parts.join(&sep)))
        }
        // Map
        (CtValue::Map(m), "len") => Ok(CtValue::Int(m.len() as i64)),
        (CtValue::Map(m), "is_empty") => Ok(CtValue::Bool(m.is_empty())),
        (CtValue::Map(m), "contains_key") => {
            let k = CtKey::from_value(args.into_iter().next().unwrap_or(CtValue::Unit))
                .ok_or_else(|| unsupported("this map key type", span))?;
            Ok(CtValue::Bool(m.contains_key(&k)))
        }
        (CtValue::Map(m), "get") => {
            let k = CtKey::from_value(args.into_iter().next().unwrap_or(CtValue::Unit))
                .ok_or_else(|| unsupported("this map key type", span))?;
            Ok(match m.get(&k) {
                Some(v) => CtValue::Some(Box::new(v.clone())),
                None => CtValue::None(Type::Int),
            })
        }
        (CtValue::Map(m), "keys") => Ok(CtValue::List(m.keys().map(|k| k.to_value()).collect())),
        (CtValue::Map(m), "values") => Ok(CtValue::List(m.values().cloned().collect())),
        // String (char-counted per S41)
        (CtValue::Str(s), "len") => Ok(CtValue::Int(s.chars().count() as i64)),
        (CtValue::Str(s), "is_empty") => Ok(CtValue::Bool(s.is_empty())),
        (CtValue::Str(s), "to_upper") => Ok(CtValue::Str(s.to_uppercase())),
        (CtValue::Str(s), "to_lower") => Ok(CtValue::Str(s.to_lowercase())),
        (CtValue::Str(s), "trim") => Ok(CtValue::Str(s.trim().to_string())),
        (CtValue::Str(s), "contains") => match args.into_iter().next() {
            Some(CtValue::Str(n)) => Ok(CtValue::Bool(s.contains(&n))),
            _ => Err(unsupported("contains with a non-text argument", span)),
        },
        (CtValue::Str(s), "starts_with") => match args.into_iter().next() {
            Some(CtValue::Str(n)) => Ok(CtValue::Bool(s.starts_with(&n))),
            _ => Err(unsupported("starts_with with a non-text argument", span)),
        },
        (CtValue::Str(s), "ends_with") => match args.into_iter().next() {
            Some(CtValue::Str(n)) => Ok(CtValue::Bool(s.ends_with(&n))),
            _ => Err(unsupported("ends_with with a non-text argument", span)),
        },
        (CtValue::Str(s), "repeat") => {
            let n = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            Ok(CtValue::Str(s.repeat(n.max(0) as usize)))
        }
        (CtValue::Str(s), "replace") => {
            let mut it = args.into_iter();
            match (it.next(), it.next()) {
                (Some(CtValue::Str(from)), Some(CtValue::Str(to))) => {
                    Ok(CtValue::Str(s.replace(&from, &to)))
                }
                _ => Err(unsupported("replace with non-text arguments", span)),
            }
        }
        (CtValue::Str(s), "split") => {
            let sep = match args.into_iter().next() {
                Some(CtValue::Str(s)) => s,
                _ => String::new(),
            };
            Ok(CtValue::List(
                s.split(&sep).map(|p| CtValue::Str(p.to_string())).collect(),
            ))
        }
        (CtValue::Str(s), "chars") => Ok(CtValue::List(s.chars().map(CtValue::Char).collect())),
        _ => Err(unsupported(
            &format!("the method `.{}` at compile time", method),
            span,
        )),
    }
}
