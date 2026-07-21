//! D-SIMD2 / D-LINALG1: comptime/REPL mirror of AOT `jet_math_*` + lane ops.
//! Scalar-array fallback — same algorithms as `LinalgFns.rs` / `MathTaskMem.rs`.

use crate::AST::{BinOp, CtFloat, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;

use super::Diagnostics::{overflow, unsupported};
use super::Value::CtValue;

const MATH_TYPES: &[&str] = &[
    Syntax::SIMD_F32X4_TYPE,
    Syntax::SIMD_F64X2_TYPE,
    Syntax::LINALG_VEC2_TYPE,
    Syntax::LINALG_VEC3_TYPE,
    Syntax::LINALG_VEC4_TYPE,
    Syntax::LINALG_MAT3_TYPE,
    Syntax::LINALG_MAT4_TYPE,
];

pub(super) fn is_math_type(name: &str) -> bool {
    MATH_TYPES.contains(&name)
}

pub(super) fn arity(name: &str) -> Option<usize> {
    match name {
        Syntax::SIMD_F64X2_TYPE | Syntax::LINALG_VEC2_TYPE => Some(2),
        Syntax::LINALG_VEC3_TYPE => Some(3),
        Syntax::SIMD_F32X4_TYPE | Syntax::LINALG_VEC4_TYPE => Some(4),
        Syntax::LINALG_MAT3_TYPE => Some(9),
        Syntax::LINALG_MAT4_TYPE => Some(16),
        _ => None,
    }
}

fn is_f32_lanes(name: &str) -> bool {
    name == Syntax::SIMD_F32X4_TYPE
}

fn float_value(name: &str, n: f64) -> CtValue {
    if is_f32_lanes(name) {
        CtValue::Float(CtFloat::f32(n as f32))
    } else {
        CtValue::Float(CtFloat::f64(n))
    }
}

fn as_lane(v: &CtValue, span: Span) -> Result<f64, Diagnostic> {
    match v {
        CtValue::Float(f) => Ok(f.as_f64()),
        CtValue::Int(n) => Ok(*n as f64),
        _ => Err(unsupported("math component must be a Float", span)),
    }
}

pub(super) fn from_lanes(type_name: &str, lanes: &[f64]) -> CtValue {
    let fields = lanes
        .iter()
        .enumerate()
        .map(|(i, n)| (i.to_string(), float_value(type_name, *n)))
        .collect();
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields,
    }
}

pub(super) fn lanes(value: &CtValue) -> Option<(&str, Vec<f64>)> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    if !is_math_type(type_name) {
        return None;
    }
    let n = arity(type_name)?;
    if fields.len() != n {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let key = i.to_string();
        let (_, v) = fields.iter().find(|(name, _)| name == &key)?;
        out.push(match v {
            CtValue::Float(f) => f.as_f64(),
            CtValue::Int(n) => *n as f64,
            _ => return None,
        });
    }
    Some((type_name.as_str(), out))
}

pub(super) fn construct(name: &str, args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let n = arity(name).ok_or_else(|| unsupported(&format!("`{name}`"), span))?;
    if args.len() != n {
        return Err(unsupported(
            &format!("`{name}` expects {n} components"),
            span,
        ));
    }
    let mut lanes = Vec::with_capacity(n);
    for arg in args {
        let mut v = as_lane(arg, span)?;
        if is_f32_lanes(name) {
            v = (v as f32) as f64;
        }
        lanes.push(v);
    }
    Ok(from_lanes(name, &lanes))
}

fn zip_op(op: BinOp, a: &[f64], b: &[f64], f32_lanes: bool) -> Option<Vec<f64>> {
    if a.len() != b.len() {
        return None;
    }
    let mut out = Vec::with_capacity(a.len());
    for (l, r) in a.iter().zip(b.iter()) {
        let n = match op {
            BinOp::Add => l + r,
            BinOp::Sub => l - r,
            BinOp::Mul => l * r,
            BinOp::Div => l / r,
            _ => return None,
        };
        out.push(if f32_lanes { (n as f32) as f64 } else { n });
    }
    Some(out)
}

fn mat_mul(n: usize, a: &[f64], b: &[f64]) -> Vec<f64> {
    let mut r = vec![0.0f64; n * n];
    for c in 0..n {
        for row in 0..n {
            let mut acc = 0.0f64;
            for k in 0..n {
                acc += a[k * n + row] * b[c * n + k];
            }
            r[c * n + row] = acc;
        }
    }
    r
}

fn mat_transform(n: usize, m: &[f64], v: &[f64]) -> Vec<f64> {
    let mut r = vec![0.0f64; n];
    for row in 0..n {
        let mut a = 0.0f64;
        for k in 0..n {
            a += m[k * n + row] * v[k];
        }
        r[row] = a;
    }
    r
}

pub(super) fn eval_binop(
    op: BinOp,
    left: &CtValue,
    right: &CtValue,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let (Some((ln, ll)), Some((rn, rl))) = (lanes(left), lanes(right)) else {
        return None;
    };
    if ln != rn {
        // MatN * VecN transform.
        if matches!(op, BinOp::Mul) {
            if ln == Syntax::LINALG_MAT3_TYPE && rn == Syntax::LINALG_VEC3_TYPE {
                return Some(Ok(from_lanes(rn, &mat_transform(3, &ll, &rl))));
            }
            if ln == Syntax::LINALG_MAT4_TYPE && rn == Syntax::LINALG_VEC4_TYPE {
                return Some(Ok(from_lanes(rn, &mat_transform(4, &ll, &rl))));
            }
        }
        return Some(Err(unsupported("mixing math types", span)));
    }
    let out = if matches!(ln, Syntax::LINALG_MAT3_TYPE | Syntax::LINALG_MAT4_TYPE)
        && matches!(op, BinOp::Mul)
    {
        let n = if ln == Syntax::LINALG_MAT3_TYPE { 3 } else { 4 };
        mat_mul(n, &ll, &rl)
    } else {
        match zip_op(op, &ll, &rl, is_f32_lanes(ln)) {
            Some(v) => v,
            None => return Some(Err(unsupported("this math operator", span))),
        }
    };
    Some(Ok(from_lanes(ln, &out)))
}

fn list_to_lanes(args: &[CtValue], n: usize, span: Span) -> Result<Vec<f64>, Diagnostic> {
    let list = match args.first() {
        Some(CtValue::List(items)) => items,
        _ => return Err(unsupported("from_array expects a fixed list", span)),
    };
    if list.len() != n {
        return Err(unsupported(
            &format!("from_array expects {n} elements"),
            span,
        ));
    }
    list.iter().map(|v| as_lane(v, span)).collect()
}

pub(super) fn apply_static(
    type_name: &str,
    method: &str,
    args: Vec<CtValue>,
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    if !is_math_type(type_name) {
        return None;
    }
    let n = arity(type_name)?;
    Some(match method {
        "splat" => {
            let Some(arg) = args.first() else {
                return Some(Err(unsupported(
                    &format!("`{type_name}.splat` needs one argument"),
                    span,
                )));
            };
            match as_lane(arg, span) {
                Ok(v) => {
                    let v = if is_f32_lanes(type_name) {
                        (v as f32) as f64
                    } else {
                        v
                    };
                    Ok(from_lanes(type_name, &vec![v; n]))
                }
                Err(e) => Err(e),
            }
        }
        "from_array" => list_to_lanes(&args, n, span).map(|lanes| {
            let lanes = if is_f32_lanes(type_name) {
                lanes
                    .into_iter()
                    .map(|v| (v as f32) as f64)
                    .collect::<Vec<_>>()
            } else {
                lanes
            };
            from_lanes(type_name, &lanes)
        }),
        _ => Err(unsupported(&format!("`{type_name}.{method}`"), span)),
    })
}

fn to_array_list(type_name: &str, lanes: &[f64]) -> CtValue {
    CtValue::List(
        lanes
            .iter()
            .map(|n| float_value(type_name, *n))
            .collect(),
    )
}

fn reduce_op(name: &str, lanes: &[f64], op: &str) -> Option<f64> {
    if lanes.is_empty() {
        return None;
    }
    let f32 = is_f32_lanes(name);
    let acc = match op {
        "Add" | "sum" => lanes.iter().sum(),
        "Mul" | "product" => lanes.iter().copied().product(),
        "Min" => lanes.iter().copied().fold(f64::INFINITY, f64::min),
        "Max" => lanes.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        _ => return None,
    };
    Some(if f32 { (acc as f32) as f64 } else { acc })
}

pub(super) fn apply_method(
    recv: &CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let (name, vals) = lanes(recv)?;
    Some(match (method, args.len()) {
        ("to_array", 0) => Ok(to_array_list(name, &vals)),
        ("sum", 0) => Ok(float_value(
            name,
            reduce_op(name, &vals, "sum").unwrap_or(0.0),
        )),
        // AOT exposes product/min/max as named methods; same reduce_op as
        // `reduce(#Mul/#Min/#Max)` so comptime/REPL stay byte-identical.
        ("product", 0) => Ok(float_value(
            name,
            reduce_op(name, &vals, "product").unwrap_or(0.0),
        )),
        ("min", 0) => Ok(float_value(
            name,
            reduce_op(name, &vals, "Min").unwrap_or(0.0),
        )),
        ("max", 0) => Ok(float_value(
            name,
            reduce_op(name, &vals, "Max").unwrap_or(0.0),
        )),
        ("reduce", 1) => {
            let op = match &args[0] {
                CtValue::Enum { variant, .. } => variant.as_str(),
                CtValue::Str(s) => s.as_str(),
                _ => {
                    return Some(Err(unsupported(
                        "reduce expects #Add/#Mul/#Min/#Max",
                        span,
                    )))
                }
            };
            match reduce_op(name, &vals, op) {
                Some(n) => Ok(float_value(name, n)),
                None => Err(unsupported(&format!("reduce({op})"), span)),
            }
        }
        ("dot", 1) => {
            let Some((_, other)) = lanes(&args[0]) else {
                return Some(Err(unsupported("dot argument", span)));
            };
            if other.len() != vals.len() {
                return Some(Err(unsupported("dot size mismatch", span)));
            }
            let mut acc = 0.0f64;
            for (a, b) in vals.iter().zip(other.iter()) {
                acc += a * b;
            }
            Ok(CtValue::Float(CtFloat::f64(acc)))
        }
        ("cross", 1) if name == Syntax::LINALG_VEC3_TYPE => {
            let Some((_, o)) = lanes(&args[0]) else {
                return Some(Err(unsupported("cross argument", span)));
            };
            if o.len() != 3 {
                return Some(Err(unsupported("cross needs Vec3", span)));
            }
            Ok(from_lanes(
                name,
                &[
                    vals[1] * o[2] - vals[2] * o[1],
                    vals[2] * o[0] - vals[0] * o[2],
                    vals[0] * o[1] - vals[1] * o[0],
                ],
            ))
        }
        ("length", 0) => {
            let acc: f64 = vals.iter().map(|n| n * n).sum();
            Ok(CtValue::Float(CtFloat::f64(acc.sqrt())))
        }
        ("normalize", 0) => {
            let len: f64 = vals.iter().map(|n| n * n).sum::<f64>().sqrt();
            if len == 0.0 {
                Ok(from_lanes(name, &vals))
            } else {
                Ok(from_lanes(
                    name,
                    &vals.iter().map(|n| n / len).collect::<Vec<_>>(),
                ))
            }
        }
        ("matmul", 1) => {
            let Some((on, other)) = lanes(&args[0]) else {
                return Some(Err(unsupported("matmul argument", span)));
            };
            if on != name {
                return Some(Err(unsupported("matmul type mismatch", span)));
            }
            if !(name == Syntax::LINALG_MAT3_TYPE || name == Syntax::LINALG_MAT4_TYPE) {
                return Some(Err(unsupported("matmul on a matrix", span)));
            }
            let n = if name == Syntax::LINALG_MAT3_TYPE { 3 } else { 4 };
            Ok(from_lanes(name, &mat_mul(n, &vals, &other)))
        }
        ("transform", 1) => {
            let (n, expect_v) = if name == Syntax::LINALG_MAT3_TYPE {
                (3, Syntax::LINALG_VEC3_TYPE)
            } else if name == Syntax::LINALG_MAT4_TYPE {
                (4, Syntax::LINALG_VEC4_TYPE)
            } else {
                return Some(Err(unsupported("transform on a matrix", span)));
            };
            let Some((vn, v)) = lanes(&args[0]) else {
                return Some(Err(unsupported("transform argument", span)));
            };
            if vn != expect_v {
                return Some(Err(unsupported("transform vector type", span)));
            }
            Ok(from_lanes(expect_v, &mat_transform(n, &vals, &v)))
        }
        ("transpose", 0) => {
            let n = if name == Syntax::LINALG_MAT3_TYPE {
                3
            } else if name == Syntax::LINALG_MAT4_TYPE {
                4
            } else {
                return Some(Err(unsupported("transpose on a matrix", span)));
            };
            let mut r = vec![0.0f64; n * n];
            for c in 0..n {
                for row in 0..n {
                    r[c * n + row] = vals[row * n + c];
                }
            }
            Ok(from_lanes(name, &r))
        }
        _ => Err(unsupported(&format!("`{name}.{method}`"), span)),
    })
}

pub(super) fn lane_at(recv: &CtValue, index: i64, span: Span) -> Option<Result<CtValue, Diagnostic>> {
    let (name, vals) = lanes(recv)?;
    if !matches!(
        name,
        Syntax::SIMD_F32X4_TYPE
            | Syntax::SIMD_F64X2_TYPE
            | Syntax::LINALG_VEC2_TYPE
            | Syntax::LINALG_VEC3_TYPE
            | Syntax::LINALG_VEC4_TYPE
    ) {
        return None;
    }
    Some(if index < 0 || index as usize >= vals.len() {
        Err(unsupported(
            &format!("lane index {index} out of range for {name}"),
            span,
        ))
    } else {
        Ok(float_value(name, vals[index as usize]))
    })
}

/// D-NUMOPS1: `wrapping`/`saturating`/`checked` over a single integer binary op.
pub(super) fn overflow_opt(
    mode: &str,
    op: BinOp,
    left: i64,
    right: i64,
    signed: bool,
    bits: u8,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let (lo, hi) = crate::AST::int_range(signed, bits);
    let narrow = |n: i128| -> i64 {
        if bits == 64 && signed {
            n as i64
        } else if bits == 64 {
            n as u64 as i64
        } else if signed {
            let mask = (1i128 << bits) - 1;
            let mut v = n & mask;
            let sign = 1i128 << (bits - 1);
            if v & sign != 0 {
                v |= !mask;
            }
            v as i64
        } else {
            (n & ((1i128 << bits) - 1)) as i64
        }
    };
    // Bit-pattern wrap matching Rust's wrapping_* on fixed widths.
    let wrapping = |n: i128| -> i64 { narrow(n) };
    let checked_op = |op: BinOp, a: i128, b: i128| -> Option<i128> {
        match op {
            BinOp::Add => a.checked_add(b),
            BinOp::Sub => a.checked_sub(b),
            BinOp::Mul => a.checked_mul(b),
            BinOp::Div => a.checked_div(b),
            _ => None,
        }
    };
    let a = left as i128;
    let b = right as i128;
    match mode {
        Syntax::BUILTIN_WRAPPING => {
            let raw = match op {
                BinOp::Add => a.wrapping_add(b),
                BinOp::Sub => a.wrapping_sub(b),
                BinOp::Mul => a.wrapping_mul(b),
                BinOp::Div => {
                    if b == 0 {
                        return Err(unsupported("division by zero", span));
                    }
                    a.wrapping_div(b)
                }
                _ => return Err(unsupported("wrapping on this operator", span)),
            };
            Ok(CtValue::Int(wrapping(raw)))
        }
        Syntax::BUILTIN_SATURATING => {
            let raw = match op {
                BinOp::Add => a.saturating_add(b),
                BinOp::Sub => a.saturating_sub(b),
                BinOp::Mul => a.saturating_mul(b),
                BinOp::Div => {
                    if b == 0 {
                        return Err(unsupported("division by zero", span));
                    }
                    a.saturating_div(b)
                }
                _ => return Err(unsupported("saturating on this operator", span)),
            };
            let clamped = raw.clamp(lo, hi);
            Ok(CtValue::Int(narrow(clamped)))
        }
        Syntax::BUILTIN_CHECKED => match checked_op(op, a, b) {
            Some(raw) if (lo..=hi).contains(&raw) => Ok(CtValue::Some(Box::new(CtValue::Int(
                narrow(raw),
            )))),
            _ => Ok(CtValue::None(Type::IntN { signed, bits })),
        },
        _ => Err(overflow(mode, span)),
    }
}
