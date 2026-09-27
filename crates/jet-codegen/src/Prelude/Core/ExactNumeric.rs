// Shared exact-`Int` policy used by host adapters and emitted Prelude rows.
//
// The including module supplies `ExactBigInt` as the canonical carrier.  This
// file deliberately contains only route policy and delegates every arithmetic
// operation to that carrier; execution tiers marshal their resident carrier
// into this one before calling these functions.

pub(crate) fn add(left: &ExactBigInt, right: &ExactBigInt) -> ExactBigInt {
    left.add(right)
}

pub(crate) fn sub(left: &ExactBigInt, right: &ExactBigInt) -> ExactBigInt {
    left.sub(right)
}

pub(crate) fn mul(left: &ExactBigInt, right: &ExactBigInt) -> ExactBigInt {
    left.mul(right)
}

pub(crate) fn bit_and(left: &ExactBigInt, right: &ExactBigInt) -> ExactBigInt {
    left.bit_and(right)
}

pub(crate) fn bit_or(left: &ExactBigInt, right: &ExactBigInt) -> ExactBigInt {
    left.bit_or(right)
}

pub(crate) fn bit_xor(left: &ExactBigInt, right: &ExactBigInt) -> ExactBigInt {
    left.bit_xor(right)
}

pub(crate) fn neg(value: &ExactBigInt) -> ExactBigInt {
    value.neg()
}

pub(crate) fn bit_not(value: &ExactBigInt) -> ExactBigInt {
    value.neg().sub(&ExactBigInt::from_int(1))
}

pub(crate) fn div_rem(
    left: &ExactBigInt,
    right: &ExactBigInt,
) -> Option<(ExactBigInt, ExactBigInt)> {
    left.div_rem(right)
}

pub(crate) fn div_rem_euclid(
    left: &ExactBigInt,
    right: &ExactBigInt,
) -> Option<(ExactBigInt, ExactBigInt)> {
    left.div_rem_euclid(right)
}

pub(crate) fn floor_div(left: &ExactBigInt, right: &ExactBigInt) -> Option<ExactBigInt> {
    let (quotient, remainder) = left.div_rem(right)?;
    if !remainder.is_zero() && left.negative != right.negative {
        Some(quotient.sub(&ExactBigInt::from_int(1)))
    } else {
        Some(quotient)
    }
}

pub(crate) fn modulo(left: &ExactBigInt, right: &ExactBigInt) -> Option<ExactBigInt> {
    let (_, remainder) = left.div_rem(right)?;
    if !remainder.is_zero() && left.negative != right.negative {
        Some(remainder.add(right))
    } else {
        Some(remainder)
    }
}

pub(crate) fn pow(left: &ExactBigInt, right: &ExactBigInt) -> Option<ExactBigInt> {
    left.pow(right)
}

pub(crate) fn shl(left: &ExactBigInt, right: &ExactBigInt) -> Option<ExactBigInt> {
    left.shl(right)
}

pub(crate) fn shr(left: &ExactBigInt, right: &ExactBigInt) -> Option<ExactBigInt> {
    left.shr(right)
}

pub(crate) fn compare(left: &ExactBigInt, right: &ExactBigInt) -> core::cmp::Ordering {
    left.compare(right)
}

pub(crate) fn failure_message(member: &str) -> &'static str {
    match member {
        "pow" => "Negative default Int exponent",
        "shl" | "shr" => "Invalid shift count",
        _ => "divided by zero",
    }
}
