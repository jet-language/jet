//! D-SIMD2 / D-LINALG1: comptime/REPL mirror of AOT `jet_math_*` + lane ops.
//! Scalar-array fallback — same algorithms as `LinalgFns.rs` / `MathTaskMem.rs`.

use crate::AST::{BinOp, CtFloat, Type};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;

use super::Diagnostics::{comptime_panic, overflow, unsupported};
use super::Value::CtValue;

pub fn integer_type_layout(ty: &Type) -> Option<(bool, u8)> {
    match ty {
        Type::Int => Some((true, 64)),
        Type::IntN { signed, bits } => Some((*signed, *bits)),
        Type::Named(name) => match name.as_str() {
            "I8" => Some((true, 8)),
            "I16" => Some((true, 16)),
            "I32" => Some((true, 32)),
            "I64" | "Int" => Some((true, 64)),
            "U8" => Some((false, 8)),
            "U16" => Some((false, 16)),
            "U32" => Some((false, 32)),
            "U64" => Some((false, 64)),
            _ => None,
        },
        _ => None,
    }
}

pub fn integer_widen(value: i64, signed: bool) -> i128 {
    if signed {
        value as i128
    } else {
        value as u64 as i128
    }
}

pub fn integer_narrow(value: i128, signed: bool, bits: u8) -> i64 {
    if bits == 64 {
        return value as u64 as i64;
    }
    let mask = (1i128 << bits) - 1;
    let mut value = value & mask;
    if signed && value & (1i128 << (bits - 1)) != 0 {
        value |= !mask;
    }
    value as i64
}

pub fn integer_show(value: i64, signed: bool) -> String {
    if signed {
        value.to_string()
    } else {
        (value as u64).to_string()
    }
}

pub fn integer_bound(signed: bool, bits: u8, maximum: bool) -> i64 {
    let (lo, hi) = crate::AST::int_range(signed, bits);
    integer_narrow(if maximum { hi } else { lo }, signed, bits)
}

pub fn integer_bit_count(value: i64, width: u32, method: &str) -> Option<i64> {
    let mask = if width == 64 {
        u64::MAX
    } else {
        (1_u64 << width) - 1
    };
    let bits = (value as u64) & mask;
    let ones = bits.count_ones();
    let count = match method {
        "count_ones" => ones,
        "count_zeros" => width - ones,
        "leading_zeros" => bits.leading_zeros() - (64 - width),
        "trailing_zeros" => bits.trailing_zeros().min(width),
        _ => return None,
    };
    Some(i64::from(count))
}

pub fn integer_shift_trap(op: BinOp, count: i128, bits: u8) -> Option<String> {
    if !matches!(op, BinOp::Shl | BinOp::Shr) || (0..i128::from(bits)).contains(&count) {
        return None;
    }
    let direction = if op == BinOp::Shl { "left" } else { "right" };
    Some(format!(
        "shifting {direction} by {count} bits is out of range (this type is {bits} bits wide)"
    ))
}

/// D-MODSEM1=A: the smallest value of a signed width divided by -1 leaves the
/// width, but its REMAINDER is 0, which every width holds. Both `%` and `%%`
/// answer 0 there — the decision says the two agree whenever they can, and a
/// trap on one but not the other would break that. Only a zero divisor stops
/// the program.
pub fn integer_remainder_trap(right: i64) -> Option<&'static str> {
    (right == 0).then_some(INTEGER_DIVIDE_ZERO)
}
/// The one wording for a zero divisor, shared by `%` and by `/%`
/// (D-FLOORDIV1=A). `Prelude/Core/Division.rs` carries the same text.
pub const INTEGER_DIVIDE_ZERO: &str = "divided by zero";
/// D-EXPSEM1=A: the two power traps. `Prelude/Core/Power.rs` is the one place
/// the rule lives; these constants carry its exact wording to the tiers that
/// cannot include that file — the comptime interpreter and the Cranelift host.
/// `tests/power_and_exclusive_or.rs` proves the Prelude text still matches, so
/// the wordings cannot drift apart.
pub const INTEGER_POWER_NEGATIVE: &str =
    "a negative exponent has no whole-number result (make the base a Float to raise it to a negative power)";
pub const INTEGER_POWER_OVERFLOW: &str =
    "this power overflows the value's type (the result is outside its range)";
/// D-FLOORDIV1=A / D-MODSEM1=A: the division-family traps. Same contract as the
/// power ones above — `Prelude/Core/Division.rs` owns the rule, these carry its
/// exact wording to the tiers that cannot include that file, and
/// `tests/floor_division.rs` proves every tier still reports them.
pub const INTEGER_DIVIDE_OVERFLOW: &str =
    "this division overflows the value's type (the result is outside its range)";

/// D-FLOORDIV1=A: the `/%` rule, mirroring `Prelude/Core/Division.rs` for the
/// tiers that cannot include that file. Rust's `/` rounds toward zero, so an
/// answer whose remainder falls on the other side of zero from the divisor sat
/// one step too high. `None` means the division itself overflowed.
pub fn floor_div(left: i128, right: i128) -> Option<i128> {
    let quotient = left.checked_div(right)?;
    let remainder = left % right;
    if remainder != 0 && (remainder < 0) != (right < 0) {
        quotient.checked_sub(1)
    } else {
        Some(quotient)
    }
}

/// D-MODSEM1=A: the `%` rule, mirroring `Prelude/Core/Division.rs`. Rust's `%`
/// gives the remainder the dividend's sign; the floored modulo takes the
/// divisor's, so the answer is moved across when the two disagree. `None`
/// means the remainder itself overflowed.
pub fn floored_mod(left: i128, right: i128) -> Option<i128> {
    if right == 0 {
        return None;
    }
    // `MIN % -1` is 0 and fits every width, so it answers rather than trapping.
    let remainder = left.wrapping_rem(right);
    if remainder != 0 && (remainder < 0) != (right < 0) {
        remainder.checked_add(right)
    } else {
        Some(remainder)
    }
}

pub fn integer_binop(
    op: BinOp,
    left: i64,
    right: i64,
    signed: bool,
    bits: u8,
    right_signed: bool,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let a = integer_widen(left, signed);
    let b = integer_widen(right, right_signed);
    let (lo, hi) = crate::AST::int_range(signed, bits);
    let checked = |value: Option<i128>, name: &str| {
        value
            .filter(|value| (lo..=hi).contains(value))
            .map(|value| CtValue::Int(integer_narrow(value, signed, bits)))
            .ok_or_else(|| overflow(name, span))
    };
    if let Some(message) = integer_shift_trap(op, b, bits) {
        return Err(comptime_panic(&message, span));
    }
    if op == BinOp::Rem {
        if let Some(message) = integer_remainder_trap(right) {
            return Err(comptime_panic(message, span));
        }
    }
    match op {
        BinOp::Add => checked(a.checked_add(b), "add"),
        BinOp::Sub => checked(a.checked_sub(b), "subtract"),
        BinOp::Mul => checked(a.checked_mul(b), "multiply"),
        BinOp::Div if b == 0 => Err(comptime_panic(INTEGER_DIVIDE_ZERO, span)),
        BinOp::Div => checked(a.checked_div(b), "divide"),
        // D-FLOORDIV1=A: `/%` rounds down, in the same words `/` uses when the
        // divisor is zero.
        BinOp::FloorDiv if b == 0 => Err(comptime_panic(INTEGER_DIVIDE_ZERO, span)),
        BinOp::FloorDiv => checked(floor_div(a, b), "divide"),
        // D-MODSEM1=A: `%` is the floored modulo.
        BinOp::Mod if b == 0 => Err(comptime_panic(INTEGER_DIVIDE_ZERO, span)),
        BinOp::Mod => checked(floored_mod(a, b), "take the remainder of"),
        // D-MODSEM1=A: `MIN %% -1` is 0, which fits every width.
        BinOp::Rem => Ok(CtValue::Int(integer_narrow(a.wrapping_rem(b), signed, bits))),
        // D-EXPSEM1=A: exact whole-number power, trapping outside the range,
        // in the same words every other tier uses.
        BinOp::Pow if b < 0 => Err(comptime_panic(INTEGER_POWER_NEGATIVE, span)),
        BinOp::Pow => u32::try_from(b)
            .ok()
            .and_then(|e| a.checked_pow(e))
            .filter(|value| (lo..=hi).contains(value))
            .map(|value| CtValue::Int(integer_narrow(value, signed, bits)))
            .ok_or_else(|| comptime_panic(INTEGER_POWER_OVERFLOW, span)),
        BinOp::BitAnd => Ok(CtValue::Int(integer_narrow(a & b, signed, bits))),
        BinOp::BitOr => Ok(CtValue::Int(integer_narrow(a | b, signed, bits))),
        BinOp::BitXor => Ok(CtValue::Int(integer_narrow(a ^ b, signed, bits))),
        BinOp::Shl => Ok(CtValue::Int(integer_narrow(
            a << (b as u32),
            signed,
            bits,
        ))),
        BinOp::Shr => {
            let value = if signed {
                a >> (b as u32)
            } else {
                ((left as u64) >> (b as u32)) as i128
            };
            Ok(CtValue::Int(integer_narrow(value, signed, bits)))
        }
        BinOp::Eq => Ok(CtValue::Bool(a == b)),
        BinOp::Ne => Ok(CtValue::Bool(a != b)),
        BinOp::Lt => Ok(CtValue::Bool(a < b)),
        BinOp::Gt => Ok(CtValue::Bool(a > b)),
        BinOp::Le => Ok(CtValue::Bool(a <= b)),
        BinOp::Ge => Ok(CtValue::Bool(a >= b)),
        _ => Err(unsupported("this fixed-width integer operation", span)),
    }
}

pub fn integer_neg(value: i64, bits: u8, span: Span) -> Result<CtValue, Diagnostic> {
    let value = integer_widen(value, true);
    let (lo, hi) = crate::AST::int_range(true, bits);
    value
        .checked_neg()
        .filter(|value| (lo..=hi).contains(value))
        .map(|value| CtValue::Int(integer_narrow(value, true, bits)))
        .ok_or_else(|| overflow("negate", span))
}

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
        .map(|(i, n)| (field_name(type_name, i), float_value(type_name, *n)))
        .collect();
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields,
    }
}

fn field_name(type_name: &str, index: usize) -> String {
    match type_name {
        Syntax::LINALG_VEC2_TYPE | Syntax::LINALG_VEC3_TYPE | Syntax::LINALG_VEC4_TYPE => {
            ["x", "y", "z", "w"][index].to_string()
        }
        Syntax::LINALG_MAT3_TYPE => format!("m{}{}", index % 3, index / 3),
        Syntax::LINALG_MAT4_TYPE => format!("m{}{}", index % 4, index / 4),
        _ => index.to_string(),
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
        let key = field_name(type_name, i);
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
        "new" => {
            if args.len() != n {
                return Some(Err(unsupported(
                    &format!("`{type_name}.new` needs {n} lane arguments"),
                    span,
                )));
            }
            list_to_lanes(&args, n, span).map(|lanes| {
                let lanes = if is_f32_lanes(type_name) {
                    lanes
                        .into_iter()
                        .map(|v| (v as f32) as f64)
                        .collect::<Vec<_>>()
                } else {
                    lanes
                };
                from_lanes(type_name, &lanes)
            })
        }
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
        "Avg" => lanes.iter().sum::<f64>() / lanes.len() as f64,
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
        // `reduce(.Mul/.Min/.Max/.Avg)` so comptime/REPL stay byte-identical.
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
                        "reduce expects .Add/.Mul/.Min/.Max/.Avg",
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

pub fn lane_at(recv: &CtValue, index: i64, span: Span) -> Option<Result<CtValue, Diagnostic>> {
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
pub fn overflow_opt(
    mode: &str,
    op: BinOp,
    left: i64,
    right: i64,
    signed: bool,
    bits: u8,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    let (lo, hi) = crate::AST::int_range(signed, bits);
    let narrow = |n: i128| -> i64 { integer_narrow(n, signed, bits) };
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
    let a = integer_widen(left, signed);
    let b = integer_widen(right, signed);
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
            Some(raw) if (lo..=hi).contains(&raw) => Ok(CtValue::Present(Box::new(CtValue::Int(
                narrow(raw),
            )))),
            _ => Ok(CtValue::absent(Type::IntN { signed, bits })),
        },
        _ => Err(overflow(mode, span)),
    }
}
