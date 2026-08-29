// D-WRAP-SCOPE1=A / I9: one fixed-width arithmetic kernel for every tier.
//
// AOT, JIT, TIR evaluation, and comptime include this source. Their adapters
// only carry values in and project `Value`/`Absent`/`Trap` out. Keep operation
// selection and overflow rules here; a host must not grow a second table.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JetFixedArithmeticError {
    AddOverflow,
    SubOverflow,
    MulOverflow,
    DivideZero,
    DivisionOverflow,
    RemainderOverflow,
    PowerNegative,
    PowerOverflow,
    RotateNegative,
    ShiftOutOfRange {
        direction: &'static str,
        count: i128,
        bits: u8,
    },
    UnknownOperation,
}

impl JetFixedArithmeticError {
    pub(crate) fn message(self) -> String {
        match self {
            Self::AddOverflow => {
                "This addition overflows the value's type (the result is outside its range)"
                    .to_string()
            }
            Self::SubOverflow => {
                "This subtraction overflows the value's type (the result is outside its range)"
                    .to_string()
            }
            Self::MulOverflow => {
                "This multiplication overflows the value's type (the result is outside its range)"
                    .to_string()
            }
            Self::DivideZero => "Divided by zero".to_string(),
            Self::DivisionOverflow => {
                "This division overflows the value's type (the result is outside its range)"
                    .to_string()
            }
            Self::RemainderOverflow => "Attempt to calculate the remainder with overflow".to_string(),
            Self::PowerNegative => {
                "A negative exponent has no whole-number result (make the base a Float to raise it to a negative power)"
                    .to_string()
            }
            Self::PowerOverflow => {
                "This power overflows the value's type (the result is outside its range)"
                    .to_string()
            }
            Self::RotateNegative => "A rotation count cannot be negative".to_string(),
            Self::ShiftOutOfRange {
                direction,
                count,
                bits,
            } => format!(
                "Shifting {direction} by {count} bits is out of range (this type is {bits} bits wide)"
            ),
            Self::UnknownOperation => "This fixed-width arithmetic operation is unsupported".to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JetFixedArithmeticResult {
    Value(i64),
    Absent,
    Trap(JetFixedArithmeticError),
}

// Operation codes are part of the resident ABI. Keep them aligned with the
// JIT lowering table so that lowering is value marshalling, not semantics.
pub(crate) const JET_FIXED_OP_ADD: i64 = 0;
pub(crate) const JET_FIXED_OP_SUB: i64 = 1;
pub(crate) const JET_FIXED_OP_MUL: i64 = 2;
pub(crate) const JET_FIXED_OP_DIV: i64 = 3;
pub(crate) const JET_FIXED_OP_REM: i64 = 4;
pub(crate) const JET_FIXED_OP_BIT_AND: i64 = 5;
pub(crate) const JET_FIXED_OP_BIT_OR: i64 = 6;
pub(crate) const JET_FIXED_OP_BIT_XOR: i64 = 7;
pub(crate) const JET_FIXED_OP_SHL: i64 = 8;
pub(crate) const JET_FIXED_OP_SHR: i64 = 9;
pub(crate) const JET_FIXED_OP_POW: i64 = 10;
pub(crate) const JET_FIXED_OP_FLOOR_DIV: i64 = 11;
pub(crate) const JET_FIXED_OP_MOD: i64 = 12;
pub(crate) const JET_FIXED_OP_ROTATE_LEFT: i64 = 13;
pub(crate) const JET_FIXED_OP_ROTATE_RIGHT: i64 = 14;
pub(crate) const JET_FIXED_OP_NEG: i64 = 15;

pub(crate) const JET_FIXED_MODE_TRAP: i64 = 0;
pub(crate) const JET_FIXED_MODE_WRAPPING: i64 = 1;
pub(crate) const JET_FIXED_MODE_SATURATING: i64 = 2;
pub(crate) const JET_FIXED_MODE_CHECKED: i64 = 3;

fn bounds(signed: bool, bits: u8) -> Option<(i128, i128)> {
    if !(1..=64).contains(&bits) {
        return None;
    }
    if signed {
        let half = 1_i128 << (bits - 1);
        Some((-half, half - 1))
    } else {
        Some((0, (1_i128 << bits) - 1))
    }
}

pub(crate) fn jet_fixed_integer_widen(value: i128, signed: bool) -> i128 {
    if signed {
        value
    } else {
        value as u64 as i128
    }
}

pub(crate) fn jet_fixed_integer_narrow(value: i128, signed: bool, bits: u8) -> i64 {
    if bits == 64 {
        return value as u64 as i64;
    }
    let mask = (1_i128 << bits) - 1;
    let mut value = value & mask;
    if signed && value & (1_i128 << (bits - 1)) != 0 {
        value |= !mask;
    }
    value as i64
}

fn failure(mode: i64, error: JetFixedArithmeticError) -> JetFixedArithmeticResult {
    if mode == JET_FIXED_MODE_CHECKED {
        JetFixedArithmeticResult::Absent
    } else {
        JetFixedArithmeticResult::Trap(error)
    }
}

fn checked_value(
    mode: i64,
    value: Option<i128>,
    lo: i128,
    hi: i128,
    error: JetFixedArithmeticError,
    signed: bool,
    bits: u8,
) -> JetFixedArithmeticResult {
    match value.filter(|value| (lo..=hi).contains(value)) {
        Some(value) => JetFixedArithmeticResult::Value(jet_fixed_integer_narrow(value, signed, bits)),
        None => failure(mode, error),
    }
}

fn wrapping_value(value: i128, signed: bool, bits: u8) -> JetFixedArithmeticResult {
    JetFixedArithmeticResult::Value(jet_fixed_integer_narrow(value, signed, bits))
}

fn saturating_value(value: i128, lo: i128, hi: i128, signed: bool, bits: u8) -> JetFixedArithmeticResult {
    JetFixedArithmeticResult::Value(jet_fixed_integer_narrow(value.clamp(lo, hi), signed, bits))
}

fn floor_div(left: i128, right: i128) -> Option<i128> {
    let quotient = left.checked_div(right)?;
    let remainder = left % right;
    if remainder != 0 && (remainder < 0) != (right < 0) {
        quotient.checked_sub(1)
    } else {
        Some(quotient)
    }
}

fn floored_mod(left: i128, right: i128) -> Option<i128> {
    if right == 0 {
        return None;
    }
    let remainder = left.wrapping_rem(right);
    if remainder != 0 && (remainder < 0) != (right < 0) {
        remainder.checked_add(right)
    } else {
        Some(remainder)
    }
}

fn wrapping_pow(mut base: i128, mut exponent: i128, bits: u8, signed: bool) -> i64 {
    let mask = if bits == 64 {
        u128::from(u64::MAX)
    } else {
        (1_u128 << bits) - 1
    };
    base = (base as u128 & mask) as i128;
    let mut result = (1_u128 & mask) as i128;
    while exponent != 0 {
        if exponent & 1 != 0 {
            result = ((result as u128).wrapping_mul(base as u128) & mask) as i128;
        }
        exponent >>= 1;
        if exponent != 0 {
            base = ((base as u128).wrapping_mul(base as u128) & mask) as i128;
        }
    }
    jet_fixed_integer_narrow(result, signed, bits)
}

fn saturating_pow(
    mut base: i128,
    mut exponent: i128,
    lo: i128,
    hi: i128,
    signed: bool,
    bits: u8,
) -> i64 {
    let clamp_product = |left: i128, right: i128| {
        left.checked_mul(right)
            .map(|value| value.clamp(lo, hi))
            .unwrap_or_else(|| if (left < 0) == (right < 0) { hi } else { lo })
    };
    base = base.clamp(lo, hi);
    let mut result = 1_i128.clamp(lo, hi);
    while exponent != 0 {
        if exponent & 1 != 0 {
            result = clamp_product(result, base);
        }
        exponent >>= 1;
        if exponent != 0 {
            base = clamp_product(base, base);
        }
    }
    jet_fixed_integer_narrow(result, signed, bits)
}

fn power(
    left: i128,
    right: i128,
    mode: i64,
    lo: i128,
    hi: i128,
    signed: bool,
    bits: u8,
) -> JetFixedArithmeticResult {
    if right < 0 {
        return failure(mode, JetFixedArithmeticError::PowerNegative);
    }
    if right > u32::MAX as i128 {
        return failure(mode, JetFixedArithmeticError::PowerOverflow);
    }
    match mode {
        JET_FIXED_MODE_WRAPPING => {
            JetFixedArithmeticResult::Value(wrapping_pow(left, right, bits, signed))
        }
        JET_FIXED_MODE_SATURATING => JetFixedArithmeticResult::Value(saturating_pow(
            left, right, lo, hi, signed, bits,
        )),
        JET_FIXED_MODE_CHECKED | JET_FIXED_MODE_TRAP => checked_value(
            mode,
            left.checked_pow(right as u32),
            lo,
            hi,
            JetFixedArithmeticError::PowerOverflow,
            signed,
            bits,
        ),
        _ => JetFixedArithmeticResult::Trap(JetFixedArithmeticError::UnknownOperation),
    }
}

pub(crate) fn jet_fixed_arithmetic(
    left: i64,
    right: i128,
    op: i64,
    mode: i64,
    signed: bool,
    bits: u8,
    right_signed: bool,
) -> JetFixedArithmeticResult {
    let Some((lo, hi)) = bounds(signed, bits) else {
        return JetFixedArithmeticResult::Trap(JetFixedArithmeticError::UnknownOperation);
    };
    let left = jet_fixed_integer_widen(left as i128, signed);
    let right = jet_fixed_integer_widen(right, right_signed);
    match op {
        JET_FIXED_OP_ADD => match mode {
            JET_FIXED_MODE_WRAPPING => wrapping_value(left.wrapping_add(right), signed, bits),
            JET_FIXED_MODE_SATURATING => saturating_value(
                left.saturating_add(right),
                lo,
                hi,
                signed,
                bits,
            ),
            JET_FIXED_MODE_CHECKED | JET_FIXED_MODE_TRAP => checked_value(
                mode,
                left.checked_add(right),
                lo,
                hi,
                JetFixedArithmeticError::AddOverflow,
                signed,
                bits,
            ),
            _ => JetFixedArithmeticResult::Trap(JetFixedArithmeticError::UnknownOperation),
        },
        JET_FIXED_OP_SUB => match mode {
            JET_FIXED_MODE_WRAPPING => wrapping_value(left.wrapping_sub(right), signed, bits),
            JET_FIXED_MODE_SATURATING => saturating_value(
                left.saturating_sub(right),
                lo,
                hi,
                signed,
                bits,
            ),
            JET_FIXED_MODE_CHECKED | JET_FIXED_MODE_TRAP => checked_value(
                mode,
                left.checked_sub(right),
                lo,
                hi,
                JetFixedArithmeticError::SubOverflow,
                signed,
                bits,
            ),
            _ => JetFixedArithmeticResult::Trap(JetFixedArithmeticError::UnknownOperation),
        },
        JET_FIXED_OP_MUL => match mode {
            JET_FIXED_MODE_WRAPPING => wrapping_value(left.wrapping_mul(right), signed, bits),
            JET_FIXED_MODE_SATURATING => saturating_value(
                left.saturating_mul(right),
                lo,
                hi,
                signed,
                bits,
            ),
            JET_FIXED_MODE_CHECKED | JET_FIXED_MODE_TRAP => checked_value(
                mode,
                left.checked_mul(right),
                lo,
                hi,
                JetFixedArithmeticError::MulOverflow,
                signed,
                bits,
            ),
            _ => JetFixedArithmeticResult::Trap(JetFixedArithmeticError::UnknownOperation),
        },
        JET_FIXED_OP_NEG if signed => match mode {
            JET_FIXED_MODE_WRAPPING => wrapping_value(left.wrapping_neg(), signed, bits),
            JET_FIXED_MODE_SATURATING => saturating_value(
                left.saturating_neg(),
                lo,
                hi,
                signed,
                bits,
            ),
            JET_FIXED_MODE_CHECKED | JET_FIXED_MODE_TRAP => checked_value(
                mode,
                left.checked_neg(),
                lo,
                hi,
                JetFixedArithmeticError::SubOverflow,
                signed,
                bits,
            ),
            _ => JetFixedArithmeticResult::Trap(JetFixedArithmeticError::UnknownOperation),
        },
        JET_FIXED_OP_POW => power(left, right, mode, lo, hi, signed, bits),
        JET_FIXED_OP_DIV => {
            if right == 0 {
                return failure(mode, JetFixedArithmeticError::DivideZero);
            }
            match mode {
                JET_FIXED_MODE_WRAPPING => {
                    wrapping_value(left.wrapping_div(right), signed, bits)
                }
                JET_FIXED_MODE_SATURATING => saturating_value(
                    left.saturating_div(right),
                    lo,
                    hi,
                    signed,
                    bits,
                ),
                JET_FIXED_MODE_CHECKED | JET_FIXED_MODE_TRAP => checked_value(
                    mode,
                    left.checked_div(right),
                    lo,
                    hi,
                    JetFixedArithmeticError::DivisionOverflow,
                    signed,
                    bits,
                ),
                _ => JetFixedArithmeticResult::Trap(JetFixedArithmeticError::UnknownOperation),
            }
        }
        JET_FIXED_OP_REM => {
            if right == 0 {
                return failure(mode, JetFixedArithmeticError::DivideZero);
            }
            if mode == JET_FIXED_MODE_CHECKED {
                return match left.checked_rem(right) {
                    Some(value) => JetFixedArithmeticResult::Value(
                        jet_fixed_integer_narrow(value, signed, bits),
                    ),
                    None => JetFixedArithmeticResult::Absent,
                };
            }
            if !matches!(mode, JET_FIXED_MODE_TRAP) {
                return JetFixedArithmeticResult::Trap(JetFixedArithmeticError::UnknownOperation);
            }
            JetFixedArithmeticResult::Value(jet_fixed_integer_narrow(
                left.wrapping_rem(right),
                signed,
                bits,
            ))
        }
        JET_FIXED_OP_FLOOR_DIV => {
            if right == 0 {
                return failure(mode, JetFixedArithmeticError::DivideZero);
            }
            if !matches!(mode, JET_FIXED_MODE_TRAP | JET_FIXED_MODE_CHECKED) {
                return JetFixedArithmeticResult::Trap(JetFixedArithmeticError::UnknownOperation);
            }
            checked_value(
                mode,
                floor_div(left, right),
                lo,
                hi,
                JetFixedArithmeticError::DivisionOverflow,
                signed,
                bits,
            )
        }
        JET_FIXED_OP_MOD => {
            if right == 0 {
                return failure(mode, JetFixedArithmeticError::DivideZero);
            }
            if !matches!(mode, JET_FIXED_MODE_TRAP | JET_FIXED_MODE_CHECKED) {
                return JetFixedArithmeticResult::Trap(JetFixedArithmeticError::UnknownOperation);
            }
            checked_value(
                mode,
                floored_mod(left, right),
                lo,
                hi,
                JetFixedArithmeticError::DivisionOverflow,
                signed,
                bits,
            )
        }
        JET_FIXED_OP_BIT_AND => {
            JetFixedArithmeticResult::Value(jet_fixed_integer_narrow(left & right, signed, bits))
        }
        JET_FIXED_OP_BIT_OR => {
            JetFixedArithmeticResult::Value(jet_fixed_integer_narrow(left | right, signed, bits))
        }
        JET_FIXED_OP_BIT_XOR => {
            JetFixedArithmeticResult::Value(jet_fixed_integer_narrow(left ^ right, signed, bits))
        }
        JET_FIXED_OP_SHL | JET_FIXED_OP_SHR => {
            let direction = if op == JET_FIXED_OP_SHL { "left" } else { "right" };
            if !(0..i128::from(bits)).contains(&right) {
                return JetFixedArithmeticResult::Trap(JetFixedArithmeticError::ShiftOutOfRange {
                    direction,
                    count: right,
                    bits,
                });
            }
            let count = right as u32;
            let value = if op == JET_FIXED_OP_SHL {
                if signed {
                    left << count
                } else {
                    ((left as u64) << count) as i128
                }
            } else if signed {
                left >> count
            } else {
                ((left as u64) >> count) as i128
            };
            JetFixedArithmeticResult::Value(jet_fixed_integer_narrow(value, signed, bits))
        }
        JET_FIXED_OP_ROTATE_LEFT | JET_FIXED_OP_ROTATE_RIGHT => {
            if right < 0 {
                return JetFixedArithmeticResult::Trap(JetFixedArithmeticError::RotateNegative);
            }
            let width = u32::from(bits);
            let mask = if bits == 64 {
                u128::from(u64::MAX)
            } else {
                (1_u128 << bits) - 1
            };
            let value = (left as u128) & mask;
            let count = (right as u128 % u128::from(width)) as u32;
            let rotated = if count == 0 {
                value
            } else if op == JET_FIXED_OP_ROTATE_LEFT {
                ((value << count) | (value >> (width - count))) & mask
            } else {
                ((value >> count) | (value << (width - count))) & mask
            };
            JetFixedArithmeticResult::Value(jet_fixed_integer_narrow(
                rotated as i128,
                signed,
                bits,
            ))
        }
        _ => JetFixedArithmeticResult::Trap(JetFixedArithmeticError::UnknownOperation),
    }
}
