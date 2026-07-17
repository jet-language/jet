//! Binary operators, comparisons, builtin method dispatch, and the `as_*`
//! coercions shared by the interpreter spine.

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{BinOp, Type};

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
pub(super) fn eval_binop(
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
        (BinOp::Add, Float(a), Float(b)) => Ok(Float(a + b)),
        (BinOp::Sub, Float(a), Float(b)) => Ok(Float(a - b)),
        (BinOp::Mul, Float(a), Float(b)) => Ok(Float(a * b)),
        (BinOp::Div, Float(a), Float(b)) => Ok(Float(a / b)),
        // D-BIGINT1: arbitrary-precision arithmetic never overflows (that's
        // the whole point), so no `checked_*`/`overflow()` path here.
        (BinOp::Add, BigInt(a), BigInt(b)) => Ok(BigInt(a.add(&b))),
        (BinOp::Sub, BigInt(a), BigInt(b)) => Ok(BigInt(a.sub(&b))),
        (BinOp::Mul, BigInt(a), BigInt(b)) => Ok(BigInt(a.mul(&b))),
        (BinOp::Eq, a, b) => Ok(Bool(a == b)),
        (BinOp::Ne, a, b) => Ok(Bool(a != b)),
        (BinOp::Lt, a, b) => cmp(a, b, span).map(|o| Bool(o == std::cmp::Ordering::Less)),
        (BinOp::Gt, a, b) => cmp(a, b, span).map(|o| Bool(o == std::cmp::Ordering::Greater)),
        (BinOp::Le, a, b) => cmp(a, b, span).map(|o| Bool(o != std::cmp::Ordering::Greater)),
        (BinOp::Ge, a, b) => cmp(a, b, span).map(|o| Bool(o != std::cmp::Ordering::Less)),
        _ => Err(unsupported("this operation", span)),
    }
}

pub(super) fn cmp(a: CtValue, b: CtValue, span: Span) -> Result<std::cmp::Ordering, Diagnostic> {
    use CtValue::*;
    match (a, b) {
        (Int(a), Int(b)) => Ok(a.cmp(&b)),
        (Float(a), Float(b)) => a
            .partial_cmp(&b)
            .ok_or_else(|| unsupported("comparing NaN", span)),
        (Char(a), Char(b)) => Ok(a.cmp(&b)),
        (Str(a), Str(b)) => Ok(a.cmp(&b)),
        (BigInt(a), BigInt(b)) => Ok(a.compare(&b)),
        _ => Err(unsupported("comparing these values", span)),
    }
}

/// c97/D-STRPARSE1: static method dispatch for built-in types (`Int.parse`,
/// `Float.parse`). Returns `None` when the receiver is not a recognised
/// built-in type name; the caller falls through to user-defined methods.
pub(super) fn apply_static_type_method(
    type_name: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
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
        if let (CtValue::Float(n), Some((signed, bits))) = (&value, int_kind(&target)) {
            let (lo, hi) = crate::AST::int_range(signed, bits);
            let in_range = n.is_finite() && *n >= lo as f64 && *n < (hi + 1) as f64;
            return Some(Ok(if in_range {
                CtValue::ResOk(Box::new(CtValue::Int(n.trunc() as i64)))
            } else {
                CtValue::ResErr(Box::new(CtValue::Str(format!(
                    "value doesn't fit in {type_name}"
                ))))
            }));
        }
        let converted = match (value, &target) {
            (CtValue::Int(n), Type::Float) => CtValue::Float(n as f64),
            (CtValue::Int(n), Type::Float32) => CtValue::Float((n as f32) as f64),
            (CtValue::Float(n), Type::Float) => CtValue::Float(n),
            (CtValue::Float(n), Type::Float32) => CtValue::Float((n as f32) as f64),
            (CtValue::Float(n), _) => CtValue::Int(n as i64),
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
                Ok(f) => CtValue::ResOk(Box::new(CtValue::Float(f))),
                Err(_) => CtValue::ResErr(Box::new(CtValue::Str(format!(
                    "cannot parse `{}` as a float",
                    s
                )))),
            }))
        }
        _ => None,
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
/// D-ANY-JAI1: the type name `reflect.of(x).type_name()` reports — Jet's
/// beginner-facing names for the built-ins, the struct/enum's own name for
/// everything else.
fn ctvalue_type_name(v: &CtValue) -> String {
    match v {
        CtValue::Int(_) => "Int".to_string(),
        CtValue::Float(_) => "Float".to_string(),
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

pub(super) fn apply_method(
    recv: &CtValue,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Result<CtValue, Diagnostic> {
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
        // c139: `.raw()` unwraps a distinct/`@UnitFamily` type (D-DIST1/D-QUAL3).
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
        (CtValue::Struct { type_name, fields }, "now")
            if type_name == crate::Syntax::CLOCK_TYPE =>
        {
            Ok(fields
                .iter()
                .find(|(n, _)| n == "now")
                .map(|(_, v)| v.clone())
                .unwrap_or(CtValue::Int(0)))
        }
        (CtValue::Struct { type_name, fields }, "tick" | "advance" | "wait")
            if type_name == crate::Syntax::CLOCK_TYPE =>
        {
            let mut now = fields
                .iter()
                .find(|(n, _)| n == "now")
                .and_then(|(_, v)| match v {
                    CtValue::Int(n) => Some(*n),
                    _ => None,
                })
                .unwrap_or(0);
            if method == "wait" {
                if let Some(CtValue::Struct {
                    type_name: dur_ty,
                    fields: dur_fields,
                }) = args.first()
                {
                    if dur_ty == crate::Syntax::DURATION_TYPE {
                        if let Some(CtValue::Int(ms)) =
                            dur_fields.iter().find(|(n, _)| n == "ms").map(|(_, v)| v)
                        {
                            now += ms;
                        }
                    }
                }
            } else if let Some(ms) = args.first().and_then(|v| match v {
                CtValue::Int(n) => Some(*n),
                _ => None,
            }) {
                now = if method == "advance" { ms } else { now + ms };
            }
            Ok(CtValue::Struct {
                type_name: crate::Syntax::CLOCK_TYPE.to_string(),
                fields: vec![("now".to_string(), CtValue::Int(now))],
            })
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
        (CtValue::Struct { type_name, fields }, "int") if type_name == crate::Syntax::RNG_TYPE => {
            let mut state = fields
                .iter()
                .find(|(n, _)| n == "state")
                .and_then(|(_, v)| match v {
                    CtValue::Int(n) => Some(*n as u64),
                    _ => None,
                })
                .unwrap_or(0);
            let low = as_int(args.first().unwrap_or(&CtValue::Int(0)), span)?;
            let high = as_int(args.get(1).unwrap_or(&CtValue::Int(0)), span)?;
            let draw = super::Methods::random_int(&mut state, low, high);
            Ok(CtValue::Int(draw))
        }
        (CtValue::Struct { type_name, fields }, "float")
            if type_name == crate::Syntax::RNG_TYPE =>
        {
            let mut state = fields
                .iter()
                .find(|(n, _)| n == "state")
                .and_then(|(_, v)| match v {
                    CtValue::Int(n) => Some(*n as u64),
                    _ => None,
                })
                .unwrap_or(0);
            Ok(CtValue::Float(super::Methods::random_float(&mut state)))
        }
        _ => Err(unsupported(
            &format!("the method `.{}` at compile time", method),
            span,
        )),
    }
}
