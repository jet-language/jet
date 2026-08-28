//! D-SIMD2 / D-LINALG1: comptime/REPL mirror of AOT `jet_math_*` + lane ops.
//! Scalar-array fallback — same algorithms as `LinalgFns.rs` / `MathTaskMem.rs`.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;
use crate::AST::{BinOp, CtFloat, Type};

use super::Diagnostics::{comptime_panic, overflow, unsupported};
use crate::AST::CtValue;

mod fixed_arithmetic {
    include!("../../../jet-codegen/src/Prelude/Core/FixedArithmetic.rs");
}

mod simd_lanes {
    include!("../../../jet-codegen/src/Prelude/Core/SimdLanes.rs");
}

fn fixed_op(op: BinOp) -> Option<i64> {
    Some(match op {
        BinOp::Add => fixed_arithmetic::JET_FIXED_OP_ADD,
        BinOp::Sub => fixed_arithmetic::JET_FIXED_OP_SUB,
        BinOp::Mul => fixed_arithmetic::JET_FIXED_OP_MUL,
        BinOp::Div => fixed_arithmetic::JET_FIXED_OP_DIV,
        BinOp::Rem => fixed_arithmetic::JET_FIXED_OP_REM,
        BinOp::BitAnd => fixed_arithmetic::JET_FIXED_OP_BIT_AND,
        BinOp::BitOr => fixed_arithmetic::JET_FIXED_OP_BIT_OR,
        BinOp::BitXor => fixed_arithmetic::JET_FIXED_OP_BIT_XOR,
        BinOp::Shl => fixed_arithmetic::JET_FIXED_OP_SHL,
        BinOp::Shr => fixed_arithmetic::JET_FIXED_OP_SHR,
        BinOp::Pow => fixed_arithmetic::JET_FIXED_OP_POW,
        BinOp::FloorDiv => fixed_arithmetic::JET_FIXED_OP_FLOOR_DIV,
        BinOp::Mod => fixed_arithmetic::JET_FIXED_OP_MOD,
        _ => return None,
    })
}

fn fixed_result(
    result: fixed_arithmetic::JetFixedArithmeticResult,
    signed: bool,
    bits: u8,
    checked: bool,
    span: Span,
) -> Result<CtValue, Diagnostic> {
    match result {
        fixed_arithmetic::JetFixedArithmeticResult::Value(value) => {
            let value = CtValue::Int(value);
            if checked {
                Ok(CtValue::Present(Box::new(value)))
            } else {
                Ok(value)
            }
        }
        fixed_arithmetic::JetFixedArithmeticResult::Absent => {
            Ok(CtValue::absent(Type::IntN { signed, bits }))
        }
        fixed_arithmetic::JetFixedArithmeticResult::Trap(error) => {
            let message = error.message();
            Err(comptime_panic(&message, span))
        }
    }
}

pub fn integer_type_layout(ty: &Type) -> Option<(bool, u8)> {
    match ty {
        Type::Int => Some((true, 64)),
        Type::IntN { signed, bits } => Some((*signed, *bits)),
        Type::InlineRange { base, .. } => integer_type_layout(base),
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
pub const INTEGER_ROTATE_NEGATIVE: &str = "a rotation count cannot be negative";
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
    if let Some(fixed_op) = fixed_op(op) {
        return fixed_result(
            fixed_arithmetic::jet_fixed_arithmetic(
                left,
                right as i128,
                fixed_op,
                fixed_arithmetic::JET_FIXED_MODE_TRAP,
                signed,
                bits,
                right_signed,
            ),
            signed,
            bits,
            false,
            span,
        );
    }
    match op {
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
    fixed_result(
        fixed_arithmetic::jet_fixed_arithmetic(
            value,
            0,
            fixed_arithmetic::JET_FIXED_OP_NEG,
            fixed_arithmetic::JET_FIXED_MODE_TRAP,
            true,
            bits,
            true,
        ),
        true,
        bits,
        false,
        span,
    )
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
    Syntax::is_simd_lane_type(name) || MATH_TYPES.contains(&name)
}

pub(super) fn arity(name: &str) -> Option<usize> {
    if let Some((_, lanes)) = Syntax::simd_lane_layout(name) {
        return Some(lanes);
    }
    match name {
        Syntax::LINALG_VEC2_TYPE => Some(2),
        Syntax::LINALG_VEC3_TYPE => Some(3),
        Syntax::LINALG_VEC4_TYPE => Some(4),
        Syntax::LINALG_MAT3_TYPE => Some(9),
        Syntax::LINALG_MAT4_TYPE => Some(16),
        _ => None,
    }
}

fn is_f32_lanes(name: &str) -> bool {
    matches!(
        Syntax::simd_lane_layout(name),
        Some((Syntax::SimdLaneKind::F32, _))
    )
}

fn integer_lane_layout(name: &str) -> Option<(bool, u8)> {
    Some(match Syntax::simd_lane_layout(name)?.0 {
        Syntax::SimdLaneKind::I8 => (true, 8),
        Syntax::SimdLaneKind::I16 => (true, 16),
        Syntax::SimdLaneKind::I32 => (true, 32),
        Syntax::SimdLaneKind::I64 => (true, 64),
        Syntax::SimdLaneKind::U8 => (false, 8),
        Syntax::SimdLaneKind::U16 => (false, 16),
        Syntax::SimdLaneKind::U32 => (false, 32),
        Syntax::SimdLaneKind::U64 => (false, 64),
        Syntax::SimdLaneKind::F32 | Syntax::SimdLaneKind::F64 => return None,
    })
}

fn lane_value(name: &str, n: f64) -> CtValue {
    if let Some((signed, bits)) = integer_lane_layout(name) {
        return CtValue::Int(integer_narrow(n as i128, signed, bits));
    }
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
        .map(|(i, n)| (field_name(type_name, i), lane_value(type_name, *n)))
        .collect();
    CtValue::Struct {
        type_name: type_name.to_string(),
        fields,
    }
}

fn from_int_lanes(type_name: &str, lanes: &[i64]) -> CtValue {
    let (signed, bits) = integer_lane_layout(type_name).expect("integer SIMD lane");
    let fields = lanes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            (
                field_name(type_name, i),
                CtValue::Int(integer_narrow(integer_widen(*n, signed), signed, bits)),
            )
        })
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

fn integer_lanes(value: &CtValue) -> Option<(&str, Vec<i64>)> {
    let CtValue::Struct { type_name, fields } = value else {
        return None;
    };
    let (signed, bits) = integer_lane_layout(type_name)?;
    let n = arity(type_name)?;
    if fields.len() != n {
        return None;
    }
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let key = field_name(type_name, i);
        let (_, value) = fields.iter().find(|(name, _)| name == &key)?;
        let CtValue::Int(value) = value else {
            return None;
        };
        out.push(integer_narrow(integer_widen(*value, signed), signed, bits));
    }
    Some((type_name.as_str(), out))
}

fn as_int_lane(value: &CtValue, name: &str, span: Span) -> Result<i64, Diagnostic> {
    let (signed, bits) = integer_lane_layout(name).expect("integer SIMD lane");
    match value {
        CtValue::Int(value) => Ok(integer_narrow(integer_widen(*value, signed), signed, bits)),
        _ => Err(unsupported("math component must be an integer", span)),
    }
}

pub(super) fn construct(name: &str, args: &[CtValue], span: Span) -> Result<CtValue, Diagnostic> {
    let n = arity(name).ok_or_else(|| unsupported(&format!("`{name}`"), span))?;
    if args.len() != n {
        return Err(unsupported(
            &format!("`{name}` expects {n} components"),
            span,
        ));
    }
    if integer_lane_layout(name).is_some() {
        let lanes = args
            .iter()
            .map(|arg| as_int_lane(arg, name, span))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(from_int_lanes(name, &lanes));
    }
    let mut lanes = Vec::with_capacity(n);
    for arg in args {
        let mut v = as_lane(arg, span)?;
        v = narrow_lane(name, v);
        lanes.push(v);
    }
    Ok(from_lanes(name, &lanes))
}

fn narrow_lane(name: &str, n: f64) -> f64 {
    if let Some((signed, bits)) = integer_lane_layout(name) {
        return integer_narrow(n as i128, signed, bits) as f64;
    }
    if is_f32_lanes(name) {
        (n as f32) as f64
    } else {
        n
    }
}

fn simd_binary_op(op: BinOp) -> Option<simd_lanes::JetSimdBinaryOp> {
    Some(match op {
        BinOp::Add => simd_lanes::JetSimdBinaryOp::Add,
        BinOp::Sub => simd_lanes::JetSimdBinaryOp::Sub,
        BinOp::Mul => simd_lanes::JetSimdBinaryOp::Mul,
        BinOp::Div => simd_lanes::JetSimdBinaryOp::Div,
        _ => return None,
    })
}

fn simd_reduce_op(op: &str) -> Option<simd_lanes::JetSimdReduceOp> {
    Some(match op {
        "Add" | "sum" => simd_lanes::JetSimdReduceOp::Add,
        "Mul" | "product" => simd_lanes::JetSimdReduceOp::Mul,
        "Min" => simd_lanes::JetSimdReduceOp::Min,
        "Max" => simd_lanes::JetSimdReduceOp::Max,
        "Avg" => simd_lanes::JetSimdReduceOp::Avg,
        _ => return None,
    })
}

fn zip_op(op: BinOp, a: &[f64], b: &[f64], name: &str) -> Option<Vec<f64>> {
    let op = simd_binary_op(op)?;
    if is_f32_lanes(name) {
        let left = a.iter().map(|value| *value as f32).collect::<Vec<_>>();
        let right = b.iter().map(|value| *value as f32).collect::<Vec<_>>();
        return simd_lanes::jet_simd_binary_slice(&left, &right, op)
            .map(|values| values.into_iter().map(f64::from).collect());
    }
    simd_lanes::jet_simd_binary_slice(a, b, op)
}

fn zip_int_op(
    op: BinOp,
    a: &[i64],
    b: &[i64],
    name: &str,
    span: Span,
) -> Result<Vec<i64>, Diagnostic> {
    let (signed, bits) = integer_lane_layout(name).expect("integer SIMD lane");
    let op = simd_binary_op(op).ok_or_else(|| unsupported("this math operator", span))?;
    simd_lanes::jet_simd_integer_binary(a, b, op, signed, bits)
        .ok_or_else(|| unsupported("this math operator", span))
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
    if ln == rn && integer_lane_layout(ln).is_some() {
        let Some((_, left)) = integer_lanes(left) else {
            return Some(Err(unsupported("integer math lanes", span)));
        };
        let Some((_, right)) = integer_lanes(right) else {
            return Some(Err(unsupported("integer math lanes", span)));
        };
        return Some(zip_int_op(op, &left, &right, ln, span).map(|out| from_int_lanes(ln, &out)));
    }
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
        match zip_op(op, &ll, &rl, ln) {
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

fn list_to_int_lanes(
    args: &[CtValue],
    n: usize,
    name: &str,
    span: Span,
) -> Result<Vec<i64>, Diagnostic> {
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
    list.iter()
        .map(|value| as_int_lane(value, name, span))
        .collect()
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
            if integer_lane_layout(type_name).is_some() {
                list_to_int_lanes(&args, n, type_name, span)
                    .map(|lanes| from_int_lanes(type_name, &lanes))
            } else {
                list_to_lanes(&args, n, span).map(|lanes| {
                    let lanes = lanes
                        .into_iter()
                        .map(|v| narrow_lane(type_name, v))
                        .collect::<Vec<_>>();
                    from_lanes(type_name, &lanes)
                })
            }
        }
        "splat" => {
            let Some(arg) = args.first() else {
                return Some(Err(unsupported(
                    &format!("`{type_name}.splat` needs one argument"),
                    span,
                )));
            };
            if integer_lane_layout(type_name).is_some() {
                match as_int_lane(arg, type_name, span) {
                    Ok(v) => Ok(from_int_lanes(
                        type_name,
                        &simd_lanes::jet_simd_splat_slice(v, n),
                    )),
                    Err(e) => Err(e),
                }
            } else {
                match as_lane(arg, span) {
                    Ok(v) => {
                        let v = narrow_lane(type_name, v);
                        Ok(from_lanes(
                            type_name,
                            &simd_lanes::jet_simd_splat_slice(v, n),
                        ))
                    }
                    Err(e) => Err(e),
                }
            }
        }
        "from_array" => {
            if integer_lane_layout(type_name).is_some() {
                list_to_int_lanes(&args, n, type_name, span)
                    .map(|lanes| from_int_lanes(type_name, &lanes))
            } else {
                list_to_lanes(&args, n, span).map(|lanes| {
                    let lanes = lanes
                        .into_iter()
                        .map(|v| narrow_lane(type_name, v))
                        .collect::<Vec<_>>();
                    from_lanes(type_name, &lanes)
                })
            }
        }
        _ => Err(unsupported(&format!("`{type_name}.{method}`"), span)),
    })
}

fn to_array_list(type_name: &str, lanes: &[f64]) -> CtValue {
    CtValue::List(lanes.iter().map(|n| lane_value(type_name, *n)).collect())
}

fn to_int_array_list(type_name: &str, lanes: &[i64]) -> CtValue {
    let (signed, bits) = integer_lane_layout(type_name).expect("integer SIMD lane");
    CtValue::List(
        lanes
            .iter()
            .map(|n| CtValue::Int(integer_narrow(integer_widen(*n, signed), signed, bits)))
            .collect(),
    )
}

fn reduce_op(name: &str, lanes: &[f64], op: &str) -> Option<f64> {
    let op = simd_reduce_op(op)?;
    if is_f32_lanes(name) {
        let lanes = lanes
            .iter()
            .map(|value| *value as f32)
            .collect::<Vec<_>>();
        return simd_lanes::jet_simd_reduce_slice(&lanes, op).map(f64::from);
    }
    simd_lanes::jet_simd_reduce_slice(lanes, op)
}

fn reduce_int_op(name: &str, lanes: &[i64], op: &str) -> Option<i64> {
    let (signed, bits) = integer_lane_layout(name)?;
    simd_lanes::jet_simd_integer_reduce(lanes, simd_reduce_op(op)?, signed, bits)
}

fn reduce_value(
    name: &str,
    lanes: &[f64],
    int_lanes: Option<&[i64]>,
    op: &str,
) -> Option<CtValue> {
    if let Some(int_lanes) = int_lanes {
        return reduce_int_op(name, int_lanes, op).map(CtValue::Int);
    }
    reduce_op(name, lanes, op).map(|value| lane_value(name, value))
}

pub(super) fn apply_method(
    recv: &CtValue,
    method: &str,
    args: &[CtValue],
    span: Span,
) -> Option<Result<CtValue, Diagnostic>> {
    let (name, vals) = lanes(recv)?;
    let int_vals = integer_lanes(recv).map(|(_, values)| values);
    Some(match (method, args.len()) {
        ("to_array", 0) => Ok(int_vals
            .as_deref()
            .map_or_else(|| to_array_list(name, &vals), |values| to_int_array_list(name, values))),
        ("sum", 0) => Ok(reduce_value(name, &vals, int_vals.as_deref(), "sum")
            .unwrap_or_else(|| lane_value(name, 0.0))),
        // AOT exposes product/min/max as named methods; same reduce_op as
        // `reduce(.Mul/.Min/.Max/.Avg)` so comptime/REPL stay byte-identical.
        ("product", 0) => Ok(reduce_value(name, &vals, int_vals.as_deref(), "product")
            .unwrap_or_else(|| lane_value(name, 0.0))),
        ("min", 0) => Ok(reduce_value(name, &vals, int_vals.as_deref(), "Min")
            .unwrap_or_else(|| lane_value(name, 0.0))),
        ("max", 0) => Ok(reduce_value(name, &vals, int_vals.as_deref(), "Max")
            .unwrap_or_else(|| lane_value(name, 0.0))),
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
            match reduce_value(name, &vals, int_vals.as_deref(), op) {
                Some(value) => Ok(value),
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
            let n = if name == Syntax::LINALG_MAT3_TYPE {
                3
            } else {
                4
            };
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
    if !(Syntax::is_simd_lane_type(name)
        || matches!(
            name,
            Syntax::LINALG_VEC2_TYPE
                | Syntax::LINALG_VEC3_TYPE
                | Syntax::LINALG_VEC4_TYPE
        ))
    {
        return None;
    }
    let index = match simd_lanes::jet_simd_lane_index(index, name, vals.len()) {
        Ok(index) => index,
        Err(message) => return Some(Err(unsupported(&message, span))),
    };
    Some({
        if let Some((_, int_vals)) = integer_lanes(recv) {
            Ok(CtValue::Int(int_vals[index]))
        } else {
            Ok(lane_value(name, vals[index]))
        }
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
    let fixed_op = if mode == "rotate_left" {
        fixed_arithmetic::JET_FIXED_OP_ROTATE_LEFT
    } else if mode == "rotate_right" {
        fixed_arithmetic::JET_FIXED_OP_ROTATE_RIGHT
    } else {
        fixed_op(op).ok_or_else(|| overflow(mode, span))?
    };
    let fixed_mode = match mode {
        Syntax::BUILTIN_WRAPPING => fixed_arithmetic::JET_FIXED_MODE_WRAPPING,
        Syntax::BUILTIN_SATURATING => fixed_arithmetic::JET_FIXED_MODE_SATURATING,
        Syntax::BUILTIN_CHECKED => fixed_arithmetic::JET_FIXED_MODE_CHECKED,
        "checked_policy" => fixed_arithmetic::JET_FIXED_MODE_TRAP,
        "rotate_left" | "rotate_right" => fixed_arithmetic::JET_FIXED_MODE_WRAPPING,
        _ => return Err(overflow(mode, span)),
    };
    fixed_result(
        fixed_arithmetic::jet_fixed_arithmetic(
            left,
            right as i128,
            fixed_op,
            fixed_mode,
            signed,
            bits,
            true,
        ),
        signed,
        bits,
        mode == Syntax::BUILTIN_CHECKED,
        span,
    )
}
