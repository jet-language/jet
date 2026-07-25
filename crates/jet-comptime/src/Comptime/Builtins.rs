//! Binary operators, comparisons, builtin method dispatch, and the `as_*`
//! coercions shared by the interpreter spine.

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{BinOp, CtFloat, Type};

use super::Diagnostics::{comptime_panic, divide_by_zero, index_oob, overflow, unsupported};
use super::Value::{CtKey, CtValue};

pub fn as_bool(v: &CtValue, span: Span) -> Result<bool, Diagnostic> {
    match v {
        CtValue::Bool(b) => Ok(*b),
        _ => Err(unsupported("a non-Bool used as a condition", span)),
    }
}

pub fn as_int(v: &CtValue, span: Span) -> Result<i64, Diagnostic> {
    match v {
        CtValue::Int(n) => Ok(*n),
        _ => Err(unsupported("a non-Int used as a number", span)),
    }
}

/// Binary operators with runtime-identical semantics (i64 wrapping is
/// rejected: debug-profile rustc panics on overflow, so comptime does too).
pub fn eval_binop(
    op: BinOp,
    l: CtValue,
    r: CtValue,
    span: Span,
) -> Result<CtValue, Diagnostic> {
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
        // D-NUMOPS1: a shift count outside the value's width traps (mirrors the
        // runtime `jet_shl`/`jet_shr`). Comptime only models the default `Int`
        // (i64), so the width is 64.
        (BinOp::Shl, Int(_), Int(b)) if !(0..64).contains(&b) => Err(overflow("shift left", span)),
        (BinOp::Shr, Int(_), Int(b)) if !(0..64).contains(&b) => Err(overflow("shift right", span)),
        (BinOp::Shl, Int(a), Int(b)) => Ok(Int(a << (b as u32))),
        (BinOp::Shr, Int(a), Int(b)) => Ok(Int(a >> (b as u32))),
        (op @ (BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div), Float(a), Float(b)) => a
            .binop(op, b)
            .map(Float)
            .ok_or_else(|| unsupported("mixing float widths", span)),
        // D-BIGINT1: arbitrary-precision arithmetic never overflows (that's
        // the whole point), so no `checked_*`/`overflow()` path here.
        (BinOp::Add, BigInt(a), BigInt(b)) => Ok(BigInt(a.add(&b))),
        (BinOp::Sub, BigInt(a), BigInt(b)) => Ok(BigInt(a.sub(&b))),
        (BinOp::Mul, BigInt(a), BigInt(b)) => Ok(BigInt(a.mul(&b))),
        // D-SIMD2 / D-LINALG1: element-wise / matmul / Mat*Vec.
        (op @ (BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div), left, right)
            if super::MathLayout::lanes(&left).is_some()
                || super::MathLayout::lanes(&right).is_some() =>
        {
            match super::MathLayout::eval_binop(op, &left, &right, span) {
                std::option::Option::Some(result) => result,
                std::option::Option::None => Err(unsupported("this math operator", span)),
            }
        }
        (op @ (BinOp::Add | BinOp::Sub | BinOp::Mul), left, right)
            if matches!(
                (&left, &right),
                (
                    CtValue::Struct {
                        type_name: left_name,
                        ..
                    },
                    CtValue::Struct {
                        type_name: right_name,
                        ..
                    }
                ) if left_name == crate::Syntax::TYPE_DECIMAL
                    && right_name == crate::Syntax::TYPE_DECIMAL
            ) =>
        {
            let left = crate::Numeric::CtDecimal::from_value(&left)
                .map_err(|error| unsupported(&error, span))?;
            let right = crate::Numeric::CtDecimal::from_value(&right)
                .map_err(|error| unsupported(&error, span))?;
            let out = match op {
                BinOp::Add => left.add(&right),
                BinOp::Sub => left.sub(&right),
                BinOp::Mul => left.mul(&right),
                _ => unreachable!("decimal binop guard"),
            };
            Ok(out.to_value())
        }
        (BinOp::Eq, a, b) => Ok(Bool(a == b)),
        (BinOp::Ne, a, b) => Ok(Bool(a != b)),
        (BinOp::Lt, a, b) => cmp(a, b, span).map(|o| Bool(o == std::cmp::Ordering::Less)),
        (BinOp::Gt, a, b) => cmp(a, b, span).map(|o| Bool(o == std::cmp::Ordering::Greater)),
        (BinOp::Le, a, b) => cmp(a, b, span).map(|o| Bool(o != std::cmp::Ordering::Greater)),
        (BinOp::Ge, a, b) => cmp(a, b, span).map(|o| Bool(o != std::cmp::Ordering::Less)),
        (BinOp::And, Bool(a), Bool(b)) => Ok(Bool(a && b)),
        (BinOp::Or, Bool(a), Bool(b)) => Ok(Bool(a || b)),
        (_op, _left, _right) => Err(unsupported("this operation", span)),
    }
}

pub fn cmp(a: CtValue, b: CtValue, span: Span) -> Result<std::cmp::Ordering, Diagnostic> {
    use CtValue::*;
    match (a, b) {
        (Int(a), Int(b)) => Ok(a.cmp(&b)),
        (Float(a), Float(b)) => a
            .partial_cmp(b)
            .ok_or_else(|| unsupported("comparing NaN", span)),
        (Bool(a), Bool(b)) => Ok(a.cmp(&b)),
        (Char(a), Char(b)) => Ok(a.cmp(&b)),
        (Str(a), Str(b)) => Ok(a.cmp(&b)),
        (BigInt(a), BigInt(b)) => Ok(a.compare(&b)),
        _ => Err(unsupported("comparing these values", span)),
    }
}

/// c97/D-STRPARSE1: static method dispatch for built-in types (`Int.parse`,
/// `Float.parse`). Returns `None` when the receiver is not a recognised
/// built-in type name; the caller falls through to user-defined methods.
pub fn apply_static_type_method(
    type_name: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if method == "new" {
        if let Some(result) = super::CollectionEval::prelude_new(type_name, args.clone(), span) {
            return Some(result);
        }
        if super::MathLayout::is_math_type(type_name) {
            return Some(super::MathLayout::construct(type_name, &args, span));
        }
    }
    if let Some(result) = super::MathLayout::apply_static(type_name, method, args.clone(), span) {
        return Some(result);
    }
    if let (Some(target), Some(source_name)) = (
        crate::AST::numeric_type_from_name(type_name),
        crate::Syntax::numeric_conversion_source(method),
    ) {
        let source = crate::AST::numeric_type_from_name(source_name)?;
        let value = args.into_iter().next().unwrap_or(CtValue::Unit);
        let int_kind = |ty: &Type| match ty {
            Type::Int => Some((true, 64)),
            Type::IntN { signed, bits } => Some((*signed, *bits)),
            _ => None,
        };
        if matches!(source, Type::Float) && matches!(target, Type::Float32) {
            let CtValue::Float(CtFloat::F64(n)) = value else {
                return Some(Err(unsupported(
                    "numeric conversion with the wrong source type",
                    span,
                )));
            };
            let fits = n.is_finite() && n >= -(f32::MAX as f64) && n <= f32::MAX as f64;
            return Some(Ok(if fits {
                CtValue::ResOk(Box::new(CtValue::Float(CtFloat::f32(n as f32))))
            } else {
                CtValue::ResErr(Box::new(CtValue::Str(format!(
                    "value doesn't fit in {type_name}"
                ))))
            }));
        }
        if let (CtValue::Float(n), Some((signed, bits))) = (&value, int_kind(&target)) {
            let (lo, hi) = crate::AST::int_range(signed, bits);
            let n = n.as_f64();
            let in_range = n.is_finite() && n >= lo as f64 && n < (hi + 1) as f64;
            return Some(Ok(if in_range {
                CtValue::ResOk(Box::new(CtValue::Int(n.trunc() as i64)))
            } else {
                CtValue::ResErr(Box::new(CtValue::Str(format!(
                    "value doesn't fit in {type_name}"
                ))))
            }));
        }
        let converted = match (value, &target) {
            (CtValue::Int(n), Type::Float) => CtValue::Float(CtFloat::f64(n as f64)),
            (CtValue::Int(n), Type::Float32) => CtValue::Float(CtFloat::f32(n as f32)),
            (CtValue::Float(CtFloat::F32(n)), Type::Float) => {
                CtValue::Float(CtFloat::f64(n as f64))
            }
            (CtValue::Float(n), Type::Float) => CtValue::Float(n),
            (CtValue::Float(n), Type::Float32) => CtValue::Float(CtFloat::f32(n.as_f32())),
            (CtValue::Float(n), _) => CtValue::Int(n.trunc_i64()),
            (CtValue::Int(n), _) => CtValue::Int(n),
            _ => return Some(Err(unsupported("numeric conversion with the wrong source type", span))),
        };
        let narrowing = match (int_kind(&source), int_kind(&target)) {
            (Some(src), Some(dst)) => {
                let (slo, shi) = crate::AST::int_range(src.0, src.1);
                let (dlo, dhi) = crate::AST::int_range(dst.0, dst.1);
                !(dlo <= slo && shi <= dhi)
            }
            _ => false,
        };
        if narrowing {
            let CtValue::Int(n) = converted else { unreachable!() };
            let (signed, bits) = int_kind(&target).unwrap();
            let (lo, hi) = crate::AST::int_range(signed, bits);
            return Some(Ok(if (lo..=hi).contains(&(n as i128)) {
                CtValue::ResOk(Box::new(CtValue::Int(n)))
            } else {
                CtValue::ResErr(Box::new(CtValue::Str(format!(
                    "value doesn't fit in {type_name}"
                ))))
            }));
        }
        return Some(Ok(converted));
    }
    match (type_name, method) {
        ("Int", "parse") => {
            let s = match args.into_iter().next() {
                Some(CtValue::Str(s)) => s,
                _ => return Some(Err(unsupported("Int.parse with a non-text argument", span))),
            };
            Some(Ok(match s.trim().parse::<i64>() {
                Ok(n) => CtValue::ResOk(Box::new(CtValue::Int(n))),
                Err(_) => CtValue::ResErr(Box::new(CtValue::Str(format!(
                    "cannot parse `{}` as an integer",
                    s
                )))),
            }))
        }
        ("Float", "parse") => {
            let s = match args.into_iter().next() {
                Some(CtValue::Str(s)) => s,
                _ => {
                    return Some(Err(unsupported(
                        "Float.parse with a non-text argument",
                        span,
                    )))
                }
            };
            Some(Ok(match s.trim().parse::<f64>() {
                Ok(f) => CtValue::ResOk(Box::new(CtValue::Float(CtFloat::f64(f)))),
                Err(_) => CtValue::ResErr(Box::new(CtValue::Str(format!(
                    "cannot parse `{}` as a float",
                    s
                )))),
            }))
        }
        ("String", "from_bytes") => {
            let bytes = match args.into_iter().next() {
                Some(CtValue::Bytes(bytes)) => bytes,
                Some(CtValue::List(items)) => {
                    let mut bytes = Vec::with_capacity(items.len());
                    for item in items {
                        let CtValue::Int(n) = item else {
                            return Some(Err(unsupported(
                                "String.from_bytes expects a [U8] byte list",
                                span,
                            )));
                        };
                        if !(0..=255).contains(&n) {
                            return Some(Err(unsupported(
                                "String.from_bytes expects bytes in 0..255",
                                span,
                            )));
                        }
                        bytes.push(n as u8);
                    }
                    bytes
                }
                _ => {
                    return Some(Err(unsupported(
                        "String.from_bytes with a non-bytes argument",
                        span,
                    )))
                }
            };
            Some(Ok(match String::from_utf8(bytes) {
                Ok(text) => CtValue::ResOk(Box::new(CtValue::Str(text))),
                Err(error) => CtValue::ResErr(Box::new(CtValue::Struct {
                    type_name: "Utf8Error".to_string(),
                    fields: vec![(
                        "message".to_string(),
                        CtValue::Str(error.to_string()),
                    )],
                })),
            }))
        }
        ("Secret", "from_bytes") => {
            let bytes = match args.into_iter().next() {
                Some(CtValue::Bytes(bytes)) => bytes,
                Some(CtValue::List(items)) => {
                    let mut bytes = Vec::with_capacity(items.len());
                    for item in items {
                        let CtValue::Int(n) = item else {
                            return Some(Err(unsupported(
                                "Secret.from_bytes expects a [U8] byte list",
                                span,
                            )));
                        };
                        if !(0..=255).contains(&n) {
                            return Some(Err(unsupported(
                                "Secret.from_bytes expects bytes in 0..255",
                                span,
                            )));
                        }
                        bytes.push(n as u8);
                    }
                    bytes
                }
                _ => {
                    return Some(Err(unsupported(
                        "Secret.from_bytes with a non-bytes argument",
                        span,
                    )))
                }
            };
            Some(Ok(CtValue::Struct {
                type_name: "Secret".to_string(),
                fields: vec![("bytes".to_string(), CtValue::Bytes(bytes))],
            }))
        }
        _ => None,
    }
}

/// Mutating list/map methods (`push`, `pop`, …). Returns the method's value.
pub fn apply_mutating(
    recv: &mut CtValue,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    if let Some(result) = super::CollectionEval::apply_mutating(recv, method, args.clone(), span) {
        return result;
    }
    if let Some(result) = super::Methods::apply_pool(recv, method, &args, span) {
        let (value, updated) = result?;
        if let Some(updated) = updated {
            *recv = updated;
        }
        return Ok(value);
    }
    // D-DET1 / #777: Clock + seeded Rng mutate in place for TirBridge handles.
    let clock_next = if let CtValue::Struct { type_name, fields } = &*recv {
        if type_name == crate::Syntax::CLOCK_TYPE && matches!(method, "tick" | "advance" | "wait")
        {
            let now = fields
                .iter()
                .find_map(|(name, value)| match (name.as_str(), value) {
                    ("now", CtValue::Int(now)) => Some(*now),
                    _ => None,
                })
                .unwrap_or(0);
            Some(match method {
                "tick" => now.wrapping_add(as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?),
                "advance" => as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?,
                "wait" => match args.first() {
                    Some(CtValue::Struct {
                        type_name,
                        fields: dfields,
                    }) if type_name == crate::Syntax::DURATION_TYPE =>
                    {
                        let millis = dfields
                            .iter()
                            .find_map(|(name, value)| match (name.as_str(), value) {
                                ("ms", CtValue::Int(millis)) => Some(*millis),
                                _ => None,
                            })
                            .unwrap_or(0);
                        now.wrapping_add(millis)
                    }
                    _ => return Err(unsupported("Clock.wait expects a Duration", span)),
                },
                _ => unreachable!(),
            })
        } else {
            None
        }
    } else {
        None
    };
    if let Some(next) = clock_next {
        *recv = CtValue::Struct {
            type_name: crate::Syntax::CLOCK_TYPE.to_string(),
            fields: vec![("now".to_string(), CtValue::Int(next))],
        };
        return Ok(CtValue::Int(next));
    }
    let rng_state = if let CtValue::Struct { type_name, fields } = &*recv {
        if type_name == crate::Syntax::RNG_TYPE {
            Some(
                fields
                    .iter()
                    .find_map(|(name, value)| match (name.as_str(), value) {
                        ("state", CtValue::Int(state)) => Some(*state as u64),
                        _ => None,
                    })
                    .unwrap_or(0),
            )
        } else {
            None
        }
    } else {
        None
    };
    if let Some(mut state) = rng_state {
        let mut argv = args.clone();
        let value = super::Methods::apply_seeded_rng_method(&mut state, method, &mut argv, span)?;
        *recv = CtValue::Struct {
            type_name: crate::Syntax::RNG_TYPE.to_string(),
            fields: vec![("state".to_string(), CtValue::Int(state as i64))],
        };
        return Ok(value);
    }
    if matches!(&*recv, CtValue::Struct { type_name, .. } if type_name == crate::Syntax::SOLVER_TYPE)
        && method == "require"
    {
        if let Some(result) = super::Methods::solver_require(recv, &args, span) {
            let (ret, updated) = result?;
            *recv = updated;
            return Ok(ret);
        }
    }
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
        (CtValue::Map(m), "add") => {
            let mut it = args.into_iter();
            let k = CtKey::from_value(it.next().unwrap_or(CtValue::Unit))
                .ok_or_else(|| unsupported("this map key type", span))?;
            let v = it.next().unwrap_or(CtValue::Unit);
            Ok(match m.insert(k, v) {
                Some(old) => CtValue::Some(Box::new(old)),
                None => CtValue::None(Type::Int),
            })
        }
        (CtValue::Map(m), "add_new") => {
            let mut it = args.into_iter();
            let k = CtKey::from_value(it.next().unwrap_or(CtValue::Unit))
                .ok_or_else(|| unsupported("this map key type", span))?;
            let v = it.next().unwrap_or(CtValue::Unit);
            if m.contains_key(&k) {
                Ok(CtValue::Bool(false))
            } else {
                m.insert(k, v);
                Ok(CtValue::Bool(true))
            }
        }
        (CtValue::Map(m), "remove") => {
            let k = CtKey::from_value(args.into_iter().next().unwrap_or(CtValue::Unit))
                .ok_or_else(|| unsupported("this map key type", span))?;
            Ok(match m.remove(&k) {
                Some(old) => CtValue::Some(Box::new(old)),
                None => CtValue::None(Type::Int),
            })
        }
        (CtValue::Map(m), "clear") => {
            m.clear();
            Ok(CtValue::Unit)
        }
        _ => Err(unsupported(
            &format!("the method `.{}` at compile time", method),
            span,
        )),
    }
}

/// Non-mutating methods on values.
/// D-ANY-JAI1: the type name `reflect.of(x).type_name()` reports — Jet's
/// beginner-facing names for the built-ins, the struct/enum's own name for
/// everything else.
fn ctvalue_type_name(v: &CtValue) -> String {
    match v {
        CtValue::Int(_) => "Int".to_string(),
        CtValue::Float(value) => value.jet_type().show(),
        CtValue::Bool(_) => "Bool".to_string(),
        CtValue::Char(_) => "Char".to_string(),
        CtValue::Str(_) => "String".to_string(),
        CtValue::BigInt(_) => "BigInt".to_string(),
        CtValue::Bytes(_) => "[U8]".to_string(),
        CtValue::List(_) => "List".to_string(),
        CtValue::Map(_) => "Map".to_string(),
        CtValue::Struct { type_name, .. } | CtValue::Enum { type_name, .. } => type_name.clone(),
        CtValue::Some(_) | CtValue::None(_) => "Option".to_string(),
        CtValue::ResOk(_) | CtValue::ResErr(_) => "Result".to_string(),
        CtValue::Unit => "()".to_string(),
        CtValue::Closure(_) => "Fn".to_string(),
    }
}

pub fn apply_method(
    recv: &CtValue,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    if let Some(result) = super::MathLayout::apply_method(recv, method, &args, span) {
        return result;
    }
    if let Some(result) = super::Methods::apply_core_pure_method(recv, method, &args, span) {
        return result;
    }
    if let Some(result) = super::Methods::apply_pool(recv, method, &args, span) {
        return result.map(|(value, _)| value);
    }
    if let Some(result) = super::CollectionEval::apply_method(recv, method, &args, span) {
        return result;
    }
    match (recv, method) {
        // Universal
        (v, "to_string") => Ok(CtValue::Str(v.jet_show())),
        // D-BIGINT1: explicit method-call form of the same arithmetic the
        // `+`/`-`/`*` operators reach in `eval_binop` (mirrors AOT's
        // `bigint_method_return` table in `jet-foundation/Numeric.rs`).
        (CtValue::BigInt(a), "add") => match args.into_iter().next() {
            Some(CtValue::BigInt(b)) => Ok(CtValue::BigInt(a.add(&b))),
            _ => Err(unsupported("`BigInt.add` with a non-BigInt argument", span)),
        },
        (CtValue::BigInt(a), "sub") => match args.into_iter().next() {
            Some(CtValue::BigInt(b)) => Ok(CtValue::BigInt(a.sub(&b))),
            _ => Err(unsupported("`BigInt.sub` with a non-BigInt argument", span)),
        },
        (CtValue::BigInt(a), "mul") => match args.into_iter().next() {
            Some(CtValue::BigInt(b)) => Ok(CtValue::BigInt(a.mul(&b))),
            _ => Err(unsupported("`BigInt.mul` with a non-BigInt argument", span)),
        },
        (CtValue::BigInt(a), "neg") => Ok(CtValue::BigInt(a.neg())),
        // c139: `.raw()` unwraps a distinct/`#UnitFamily` type (D-DIST1/D-QUAL3).
        // Distinct types have zero runtime representation difference from
        // their base — the interpreter never wraps one, so unwrapping is
        // identity (`eval_distinct_ctor` in methods.rs constructs the same
        // unwrapped value).
        (v, "raw") => Ok(v.clone()),
        // c139: `.clone()` — every `CtValue` is already an owned, independently
        // mutable tree (no shared/interior-mutable state in this value model),
        // so a deep clone is just `Clone::clone`.
        (v, "clone") => Ok(v.clone()),
        // D-ANY-JAI1: `reflect.of(x)` accessors that don't need interpreter
        // access (`.display` does — see `eval_method`). `__Reflect`/
        // `__ReflectField` are `core.reflect`'s internal tags (never a real
        // Jet type — see the comment on `("core.reflect", "of")`).
        (CtValue::Struct { type_name, fields }, "type_name") if type_name == "__Reflect" => {
            let inner = fields.iter().find(|(n, _)| n == "value").map(|(_, v)| v);
            Ok(CtValue::Str(
                inner.map(ctvalue_type_name).unwrap_or_default(),
            ))
        }
        (CtValue::Struct { type_name, fields }, "fields") if type_name == "__Reflect" => {
            let inner = fields.iter().find(|(n, _)| n == "value").map(|(_, v)| v);
            Ok(CtValue::List(match inner {
                Some(CtValue::Struct { fields: sf, .. }) => sf
                    .iter()
                    .map(|(n, v)| CtValue::Struct {
                        type_name: "__ReflectField".to_string(),
                        fields: vec![
                            ("name".to_string(), CtValue::Str(n.clone())),
                            ("value".to_string(), v.clone()),
                        ],
                    })
                    .collect(),
                _ => Vec::new(),
            }))
        }
        (CtValue::Struct { type_name, fields }, "name") if type_name == "__ReflectField" => {
            Ok(fields
                .iter()
                .find(|(n, _)| n == "name")
                .map(|(_, v)| v.clone())
                .unwrap_or(CtValue::Unit))
        }
        (CtValue::Struct { type_name, fields }, "value") if type_name == "__ReflectField" => {
            Ok(fields
                .iter()
                .find(|(n, _)| n == "value")
                .map(|(_, v)| v.clone())
                .unwrap_or(CtValue::Unit))
        }
        // D-HOLE1: `.zip` — pair two `Option`s, `None` if either is absent.
        // `(v, "zip")` rather than guarding to `CtValue::Some`/`None` because
        // both arms of the pairing need the same fallback.
        (CtValue::Some(a), "zip") => Ok(match args.into_iter().next() {
            Some(CtValue::Some(b)) => CtValue::Some(Box::new(CtValue::Struct {
                type_name: "Pair".to_string(),
                fields: vec![("a".to_string(), (**a).clone()), ("b".to_string(), *b)],
            })),
            _ => CtValue::None(Type::Int),
        }),
        (CtValue::None(t), "zip") => Ok(CtValue::None(t.clone())),
        // D-SERDE-ACCESS=B: dynamic `Json`/`Data` accessors — `Option`-returning
        // reads over the tagged tree `JsonInterp::json_variant` builds
        // (`.parse()`'s result, or a value built by hand with `Json.Object(…)`).
        // `.field`/`.at` don't match a non-Object/Array receiver or a missing
        // key/index; `.int`/`.text`/`.bool`/`.float` don't match a value tagged
        // with a different variant — all four report absence via `None` rather
        // than an error, matching the `?? panic(…)` call-site convention.
        // Guarded to an actual `Json`-tagged value (`v @ CtValue::Enum { .. }`)
        // rather than matching any receiver — `.int`/`.float` in particular
        // would otherwise shadow the `core.random` RNG struct's own same-named
        // methods further down (match arms are tried in order).
        (v @ CtValue::Enum { .. }, "field") => {
            let key = match args.into_iter().next() {
                Some(CtValue::Str(s)) => s,
                _ => return Err(unsupported("`.field` requires a string argument", span)),
            };
            Ok(match super::JsonInterp::json_payload(v, "Object") {
                Some(CtValue::Map(m)) => match m.get(&CtKey::Str(key)) {
                    Some(found) => CtValue::Some(Box::new(found.clone())),
                    None => CtValue::None(Type::Named("Json".to_string())),
                },
                Some(CtValue::Struct { fields, .. }) => match fields.iter().find(|(n, _)| n == &key)
                {
                    Some((_, found)) => CtValue::Some(Box::new(found.clone())),
                    None => CtValue::None(Type::Named("Json".to_string())),
                },
                _ => CtValue::None(Type::Named("Json".to_string())),
            })
        }
        (v @ CtValue::Enum { .. }, "at") => {
            let i = as_int(args.first().unwrap_or(&CtValue::Int(-1)), span)?;
            Ok(match super::JsonInterp::json_payload(v, "Array") {
                Some(CtValue::List(xs)) if i >= 0 && (i as usize) < xs.len() => {
                    CtValue::Some(Box::new(xs[i as usize].clone()))
                }
                _ => CtValue::None(Type::Named("Json".to_string())),
            })
        }
        (v @ CtValue::Enum { .. }, "int") => Ok(match super::JsonInterp::json_payload(v, "Int") {
            Some(n) => CtValue::Some(Box::new(n.clone())),
            None => CtValue::None(Type::Int),
        }),
        (v @ CtValue::Enum { .. }, "text") => {
            Ok(match super::JsonInterp::json_payload(v, "Text") {
                Some(s) => CtValue::Some(Box::new(s.clone())),
                None => CtValue::None(Type::String),
            })
        }
        (v @ CtValue::Enum { .. }, "bool") => {
            Ok(match super::JsonInterp::json_payload(v, "Bool") {
                Some(b) => CtValue::Some(Box::new(b.clone())),
                None => CtValue::None(Type::Bool),
            })
        }
        (v @ CtValue::Enum { .. }, "float") => {
            Ok(match super::JsonInterp::json_payload(v, "Float") {
                Some(f) => CtValue::Some(Box::new(f.clone())),
                None => CtValue::None(Type::Float),
            })
        }
        (CtValue::Int(n), "abs") => n
            .checked_abs()
            .map(CtValue::Int)
            .ok_or_else(|| overflow("take the absolute value of", span)),
        (CtValue::Float(f), "abs") => Ok(CtValue::Float(f.abs())),
        (CtValue::Float(f), "is_nan") => Ok(CtValue::Bool(f.is_nan())),
        (CtValue::Float(f), "is_infinite") => Ok(CtValue::Bool(f.is_infinite())),
        (CtValue::Float(f), "is_finite") => Ok(CtValue::Bool(f.is_finite())),
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
        // D-ITERTOOLS1=A: Iter is List-shaped in comptime/TIR eval; materialize is
        // identity. Non-closure adapters mirror SequenceParity so JIT deopt works.
        (CtValue::List(xs), "to_list" | "collect") => Ok(CtValue::List(xs.clone())),
        (CtValue::List(xs), "take") => {
            let n = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?.max(0) as usize;
            Ok(CtValue::List(xs.iter().take(n).cloned().collect()))
        }
        (CtValue::List(xs), "skip") => {
            let n = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?.max(0) as usize;
            Ok(CtValue::List(xs.iter().skip(n).cloned().collect()))
        }
        (CtValue::List(xs), "step_by") => {
            let n = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            Ok(CtValue::List(if n <= 0 {
                Vec::new()
            } else {
                xs.iter().step_by(n as usize).cloned().collect()
            }))
        }
        (CtValue::List(xs), "dedup") => {
            let mut out = Vec::new();
            for x in xs {
                if out.last() != Some(x) {
                    out.push(x.clone());
                }
            }
            Ok(CtValue::List(out))
        }
        (CtValue::List(xs), "chunks") => {
            let n = as_int(args.first().unwrap_or(&CtValue::Int(1)), span)?.max(1) as usize;
            Ok(CtValue::List(
                xs.chunks(n)
                    .map(|chunk| CtValue::List(chunk.to_vec()))
                    .collect(),
            ))
        }
        (CtValue::List(xs), "windows") => {
            let n = as_int(args.first().unwrap_or(&CtValue::Int(1)), span)?.max(1) as usize;
            Ok(CtValue::List(
                xs.windows(n)
                    .map(|window| CtValue::List(window.to_vec()))
                    .collect(),
            ))
        }
        (CtValue::List(xs), "first") => Ok(match xs.first() {
            Some(v) => CtValue::Some(Box::new(v.clone())),
            None => CtValue::None(Type::Int),
        }),
        (CtValue::List(xs), "last") => Ok(match xs.last() {
            Some(v) => CtValue::Some(Box::new(v.clone())),
            None => CtValue::None(Type::Int),
        }),
        (CtValue::List(xs), "flatten") => {
            let mut out = Vec::new();
            for x in xs {
                let CtValue::List(values) = x else {
                    return Err(unsupported("flatten on a non-nested list", span));
                };
                out.extend(values.iter().cloned());
            }
            Ok(CtValue::List(out))
        }
        (CtValue::List(xs), "sum") => {
            let mut total = 0i64;
            for x in xs {
                total = total
                    .checked_add(as_int(x, span)?)
                    .ok_or_else(|| overflow("sum", span))?;
            }
            Ok(CtValue::Int(total))
        }
        (CtValue::List(xs), "product") => {
            let mut total = 1i64;
            for x in xs {
                total = total
                    .checked_mul(as_int(x, span)?)
                    .ok_or_else(|| overflow("product", span))?;
            }
            Ok(CtValue::Int(total))
        }
        (CtValue::List(xs), "min") => {
            let Some(mut best) = xs.first().cloned() else {
                return Ok(CtValue::None(Type::Int));
            };
            for candidate in xs.iter().skip(1) {
                if cmp(best.clone(), candidate.clone(), span)? == std::cmp::Ordering::Greater {
                    best = candidate.clone();
                }
            }
            Ok(CtValue::Some(Box::new(best)))
        }
        (CtValue::List(xs), "max") => {
            let Some(mut best) = xs.first().cloned() else {
                return Ok(CtValue::None(Type::Int));
            };
            for candidate in xs.iter().skip(1) {
                if cmp(best.clone(), candidate.clone(), span)? != std::cmp::Ordering::Greater {
                    best = candidate.clone();
                }
            }
            Ok(CtValue::Some(Box::new(best)))
        }
        (CtValue::List(xs), "intersperse") => {
            let sep = args.into_iter().next().unwrap_or(CtValue::Unit);
            let mut out = Vec::with_capacity(xs.len().saturating_mul(2).saturating_sub(1));
            for (index, x) in xs.iter().enumerate() {
                if index != 0 {
                    out.push(sep.clone());
                }
                out.push(x.clone());
            }
            Ok(CtValue::List(out))
        }
        (CtValue::List(xs), "unzip") => {
            let mut left = Vec::with_capacity(xs.len());
            let mut right = Vec::with_capacity(xs.len());
            for x in xs {
                let CtValue::Struct { fields, .. } = x else {
                    return Err(unsupported("unzip on a non-tuple list", span));
                };
                let a = fields
                    .iter()
                    .find(|(name, _)| name == "a")
                    .map(|(_, v)| v.clone());
                let b = fields
                    .iter()
                    .find(|(name, _)| name == "b")
                    .map(|(_, v)| v.clone());
                match (a, b) {
                    (Some(a), Some(b)) => {
                        left.push(a);
                        right.push(b);
                    }
                    _ => return Err(unsupported("unzip on a tuple without `a` and `b`", span)),
                }
            }
            Ok(CtValue::Struct {
                type_name: "(a,b)".to_string(),
                fields: vec![
                    ("a".to_string(), CtValue::List(left)),
                    ("b".to_string(), CtValue::List(right)),
                ],
            })
        }
        (CtValue::List(xs), "indexed") => Ok(CtValue::List(
            xs.iter()
                .enumerate()
                .map(|(idx, item)| CtValue::Struct {
                    type_name: "(idx,item)".to_string(),
                    fields: vec![
                        ("idx".to_string(), CtValue::Int(idx as i64)),
                        ("item".to_string(), item.clone()),
                    ],
                })
                .collect(),
        )),
        (CtValue::List(xs), "indexes") => Ok(CtValue::List(
            (0..xs.len() as i64).map(CtValue::Int).collect(),
        )),
        (CtValue::List(xs), "zip") => {
            let CtValue::List(other) = args.into_iter().next().unwrap_or(CtValue::Unit) else {
                return Err(unsupported("zip with a non-list argument", span));
            };
            Ok(CtValue::List(
                xs.iter()
                    .zip(other)
                    .map(|(a, b)| CtValue::Struct {
                        type_name: "(a,b)".to_string(),
                        fields: vec![
                            ("a".to_string(), a.clone()),
                            ("b".to_string(), b),
                        ],
                    })
                    .collect(),
            ))
        }
        (CtValue::List(xs), "index_of") => {
            let needle = args.into_iter().next().unwrap_or(CtValue::Unit);
            Ok(match xs.iter().position(|x| *x == needle) {
                Some(index) => CtValue::Some(Box::new(CtValue::Int(index as i64))),
                None => CtValue::None(Type::Int),
            })
        }
        // Bytes (`[U8]` from `embed_bytes`) — same surface as List, u8 elements.
        (CtValue::Bytes(bs), "len") => Ok(CtValue::Int(bs.len() as i64)),
        (CtValue::Bytes(bs), "is_empty") => Ok(CtValue::Bool(bs.is_empty())),
        (CtValue::Bytes(bs), "get") => {
            let i = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            Ok(if i < 0 || i as usize >= bs.len() {
                CtValue::None(Type::IntN {
                    signed: false,
                    bits: 8,
                })
            } else {
                CtValue::Some(Box::new(CtValue::Int(bs[i as usize] as i64)))
            })
        }
        // Map
        (CtValue::Map(m), "len") => Ok(CtValue::Int(m.len() as i64)),
        (CtValue::Map(m), "is_empty") => Ok(CtValue::Bool(m.is_empty())),
        (CtValue::Map(m), "has_key") => {
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
        (CtValue::Str(s), "to_upper") => Ok(CtValue::Str(super::TextLite::upper(s))),
        (CtValue::Str(s), "to_lower") => Ok(CtValue::Str(super::TextLite::lower(s))),
        (CtValue::Str(s), "trim") => Ok(CtValue::Str(super::TextLite::trim(s))),
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
        (CtValue::Str(s), "after") => match args.into_iter().next() {
            Some(CtValue::Str(sep)) => Ok(CtValue::Str(match s.find(&sep) {
                Some(i) => s[i + sep.len()..].to_string(),
                None => s.clone(),
            })),
            _ => Err(unsupported("after with a non-text argument", span)),
        },
        (CtValue::Str(s), "before") => match args.into_iter().next() {
            Some(CtValue::Str(sep)) => Ok(CtValue::Str(match s.find(&sep) {
                Some(i) => s[..i].to_string(),
                None => s.clone(),
            })),
            _ => Err(unsupported("before with a non-text argument", span)),
        },
        (CtValue::Str(s), "bytes") => Ok(CtValue::List(
            s.as_bytes()
                .iter()
                .map(|byte| CtValue::Int(i64::from(*byte)))
                .collect(),
        )),
        (CtValue::Str(s), "slice") => {
            let mut args = args.into_iter();
            let a = match args.next() {
                Some(CtValue::Int(n)) => n,
                _ => return Err(unsupported("slice with a non-Int start", span)),
            };
            let b = match args.next() {
                Some(CtValue::Int(n)) => n,
                _ => return Err(unsupported("slice with a non-Int end", span)),
            };
            let chars = s.chars().collect::<Vec<_>>();
            let len = chars.len() as i64;
            if a < 0 || b < 0 || a > b || b >= len {
                return Err(comptime_panic(
                    &format!("can't slice {len} characters from {a} to {b} (inclusive)"),
                    span,
                ));
            }
            Ok(CtValue::Str(
                chars[a as usize..=b as usize].iter().collect(),
            ))
        }
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
        // c97/D-STRPARSE1: `.lines()` — split on `\n`, stripping `\r\n` too (mirrors runtime).
        (CtValue::Str(s), "lines") => Ok(CtValue::List(
            s.lines().map(|l| CtValue::Str(l.to_string())).collect(),
        )),
        // D-METAREFLECT1=B: `.reflect()` on a TypeInfo struct is identity.
        (CtValue::Struct { type_name, .. }, "reflect") if type_name == "TypeInfo" => {
            Ok(recv.clone())
        }
        (CtValue::Struct { type_name, fields }, "types")
            if type_name == crate::Syntax::TYPE_PROGRAM_INFO =>
        {
            Ok(fields
                .iter()
                .find(|(name, _)| name == "types")
                .map(|(_, value)| value.clone())
                .unwrap_or_else(|| CtValue::List(Vec::new())))
        }
        (CtValue::Struct { type_name, fields }, method)
            if type_name == crate::Syntax::TYPE_PROGRAM_INFO
                && matches!(method, "functions" | "packages") =>
        {
            Ok(fields.iter().find(|(name, _)| name == method).map(|(_, value)| value.clone()).unwrap_or_else(|| CtValue::List(Vec::new())))
        }
        (CtValue::Struct { type_name, fields }, "has_method") if type_name == "TypeInfo" => {
            let needle = match args.first() { Some(CtValue::Str(value)) => value, _ => return Err(unsupported("`has_method` requires a string", span)) };
            let found = fields.iter().find(|(name, _)| name == "methods").and_then(|(_, value)| match value { CtValue::List(values) => Some(values), _ => None }).is_some_and(|values| values.iter().any(|value| matches!(value, CtValue::Struct { fields, .. } if fields.iter().any(|(name, value)| name == "name" && matches!(value, CtValue::Str(actual) if actual == needle)))));
            Ok(CtValue::Bool(found))
        }
        (CtValue::Struct { type_name, fields }, "implements") if type_name == "TypeInfo" => {
            let needle = match args.first() { Some(CtValue::Str(value)) => value, _ => return Err(unsupported("`implements` requires a string", span)) };
            let found = fields.iter().find(|(name, _)| name == "implements").and_then(|(_, value)| match value { CtValue::List(values) => Some(values), _ => None }).is_some_and(|values| values.iter().any(|value| matches!(value, CtValue::Str(actual) if actual == needle)));
            Ok(CtValue::Bool(found))
        }
        (CtValue::Struct { type_name, fields }, "reaches_panic") if type_name == "FunctionInfo" => {
            Ok(fields.iter().find(|(name, _)| name == "reaches_panic").map(|(_, value)| value.clone()).unwrap_or(CtValue::Bool(false)))
        }
        (CtValue::Struct { type_name, fields }, "has") if type_name == "EffectInfo" => {
            let needle = match args.first() { Some(CtValue::Str(value)) => value, _ => return Err(unsupported("`has` requires a string", span)) };
            let found = fields.iter().find(|(name, _)| name == "values").and_then(|(_, value)| match value { CtValue::List(values) => Some(values), _ => None }).is_some_and(|values| values.iter().any(|value| matches!(value, CtValue::Str(actual) if actual == needle)));
            Ok(CtValue::Bool(found))
        }
        // D-METAREFLECT1 / D-REFLECT1: `.has_marker(name)` on reflected member handles.
        (CtValue::Struct { type_name, fields }, "has_marker")
            if matches!(type_name.as_str(), "FieldInfo" | "MethodInfo" | "TypeInfo") =>
        {
            let needle = match args.into_iter().next() {
                Some(CtValue::Str(s)) => s,
                _ => return Err(unsupported("`has_marker` requires a string argument", span)),
            };
            if let Some((_, CtValue::List(markers))) = fields.iter().find(|(n, _)| n == "markers") {
                let found = markers
                    .iter()
                    .any(|m| matches!(m, CtValue::Str(s) if *s == needle));
                return Ok(CtValue::Bool(found));
            }
            Ok(CtValue::Bool(false))
        }
        (CtValue::Struct { type_name, fields }, "group") if type_name == "Match" => {
            let idx = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            let groups = fields
                .iter()
                .find(|(n, _)| n == "groups")
                .and_then(|(_, v)| match v {
                    CtValue::List(xs) => Some(xs),
                    _ => None,
                });
            Ok(match groups.and_then(|g| g.get(idx as usize)) {
                Some(v) => v.clone(),
                None => CtValue::None(crate::AST::Type::String),
            })
        }
        (CtValue::Enum { type_name, variant, .. }, method)
            if type_name == "Loadable"
                && matches!(method, "is_idle" | "is_loading" | "is_loaded" | "is_failed") =>
        {
            let expected = match method {
                "is_idle" => "Idle",
                "is_loading" => "Loading",
                "is_loaded" => "Loaded",
                _ => "Failed",
            };
            Ok(CtValue::Bool(variant == expected))
        }
        (CtValue::Enum { type_name, variant, args }, "loaded") if type_name == "Loadable" => {
            Ok(if variant == "Loaded" {
                args.first()
                    .map(|(_, value)| CtValue::Some(Box::new(value.clone())))
                    .unwrap_or_else(|| CtValue::None(crate::AST::Type::Named("Unit".to_string())))
            } else {
                CtValue::None(crate::AST::Type::Named("Unit".to_string()))
            })
        }
        (CtValue::Enum { type_name, variant, args: values }, "or_else") if type_name == "Loadable" => {
            if variant == "Loaded" {
                Ok(values.first().map(|(_, value)| value.clone()).unwrap_or(CtValue::Unit))
            } else {
                args.into_iter()
                    .next()
                    .ok_or_else(|| unsupported("`Loadable.or_else` requires a default", span))
            }
        }
        (CtValue::Struct { type_name, fields }, "now")
            if type_name == crate::Syntax::CLOCK_TYPE =>
        {
            Ok(fields
                .iter()
                .find(|(n, _)| n == "now")
                .map(|(_, v)| v.clone())
                .unwrap_or(CtValue::Int(0)))
        }
        (CtValue::Struct { type_name, fields }, "in")
            if type_name == crate::Syntax::DURATION_TYPE =>
        {
            let ms = fields
                .iter()
                .find(|(n, _)| n == "ms")
                .and_then(|(_, value)| match value {
                    CtValue::Int(value) => Some(*value),
                    _ => None,
                })
                .unwrap_or(0);
            let scale = match args.first() {
                Some(CtValue::Enum { type_name, variant, .. })
                    if type_name == crate::Syntax::DURATION_UNIT_TYPE => match variant.as_str() {
                        "Milliseconds" => 1,
                        "Seconds" => 1_000,
                        "Minutes" => 60_000,
                        "Hours" => 3_600_000,
                        _ => return Err(unsupported("this duration unit", span)),
                    },
                _ => return Err(unsupported("Duration.in expects a DurationUnit", span)),
            };
            Ok(CtValue::ResOk(Box::new(CtValue::Int(ms / scale))))
        }
        _ => Err(unsupported(
            &format!("the method `.{}` at compile time", method),
            span,
        )),
    }
}
