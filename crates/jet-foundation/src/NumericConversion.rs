/// D-NUMWIDEN-CROSS1=E: canonical checked integer-to-float widening policy.
///
/// A binary float with `precision` significant bits holds an integer exactly
/// when either the integer fits in that precision or its discarded low bits
/// are all zero. Callers calculate representation metadata; this seam owns
/// exactness, finite-range, and target precision policy.
pub fn jet_numeric_checked_widen_parts(
    significant: usize,
    trailing: usize,
    value: f64,
    target_f32: bool,
) -> Option<f64> {
    let precision = if target_f32 { 24 } else { 53 };
    if significant > precision && trailing < significant - precision {
        return None;
    }
    if !value.is_finite() {
        return None;
    }
    if target_f32 {
        let narrowed = value as f32;
        narrowed.is_finite().then_some(narrowed as f64)
    } else {
        Some(value)
    }
}

pub const JET_NUMERIC_WIDEN_TRAP: &str =
    "whole number cannot cross into the decimal without losing precision";

pub fn jet_numeric_checked_widen(
    raw: u64,
    signed: bool,
    target_f32: bool,
) -> Option<f64> {
    let magnitude = if signed {
        (raw as i64).unsigned_abs()
    } else {
        raw
    };
    let significant = if magnitude == 0 {
        0
    } else {
        (u64::BITS - magnitude.leading_zeros()) as usize
    };
    let trailing = if magnitude == 0 {
        0
    } else {
        magnitude.trailing_zeros() as usize
    };
    let value = if signed {
        (raw as i64) as f64
    } else {
        raw as f64
    };
    jet_numeric_checked_widen_parts(significant, trailing, value, target_f32)
}

/// Fixed-width population queries share the source width across execution tiers.
pub fn jet_numeric_bit_count(value: i64, operation: i64, width: i64) -> i64 {
    let width = u32::try_from(width).expect("checked integer width");
    assert!((1..=64).contains(&width), "checked integer width");
    let bits = (value as u64) & (u64::MAX >> (64 - width));
    let ones = bits.count_ones();
    i64::from(match operation {
        0 => ones,
        1 => width - ones,
        2 => bits.leading_zeros() - (64 - width),
        3 => bits.trailing_zeros().min(width),
        _ => panic!("checked integer population operation"),
    })
}

/// The fixed-width host-kind table is shared by every checked numeric
/// conversion. Kinds `0..=7` are, in order, `i8`, `i16`, `i32`, `i64`,
/// `u8`, `u16`, `u32`, and `u64`.
fn jet_numeric_integer_bounds(kind: i64) -> Option<(i128, i128)> {
    Some(match kind {
        0 => (i8::MIN as i128, i8::MAX as i128),
        1 => (i16::MIN as i128, i16::MAX as i128),
        2 => (i32::MIN as i128, i32::MAX as i128),
        3 => (i64::MIN as i128, i64::MAX as i128),
        4 => (u8::MIN as i128, u8::MAX as i128),
        5 => (u16::MIN as i128, u16::MAX as i128),
        6 => (u32::MIN as i128, u32::MAX as i128),
        7 => (u64::MIN as i128, u64::MAX as i128),
        _ => return None,
    })
}

/// Return the exact fixed-width value when it fits the shared host-kind
/// range. The Option path is allocation-free and is shared by Int and fixed
/// carrier conversions.
pub fn jet_numeric_fixed_from_i128(value: i128, kind: i64) -> Option<i128> {
    let (lower, upper) = jet_numeric_integer_bounds(kind)?;
    (lower..=upper).contains(&value).then_some(value)
}

/// Use the exact exclusive integer successor before converting the upper
/// bound to a Float.
fn jet_numeric_float_bounds(kind: i64) -> Option<(f64, f64)> {
    let (lower, upper) = jet_numeric_integer_bounds(kind)?;
    Some((lower as f64, (upper + 1) as f64))
}

pub const JET_NUMERIC_CONVERSION_ERROR: &str = "value doesn't fit in destination type";
pub const JET_NUMERIC_CONVERSION_TRAP: &str = "Value doesn't fit in destination type";
pub const JET_NUMERIC_F32_CONVERSION_ERROR: &str = "value doesn't fit in F32";

/// Convert a finite Float to the selected fixed-width integer without
/// saturating at the Rust float-to-integer cast boundary.
pub fn jet_numeric_float_to_int(value: f64, kind: i64) -> Result<i128, &'static str> {
    let Some((lower, upper)) = jet_numeric_float_bounds(kind) else {
        return Err(JET_NUMERIC_CONVERSION_ERROR);
    };
    if !value.is_finite() || value < lower || value >= upper {
        return Err(JET_NUMERIC_CONVERSION_ERROR);
    }
    Ok(value as i128)
}

/// Narrow a Float only when the resulting F32 remains finite. Precision loss
/// inside that finite range is the declared narrowing operation.
pub fn jet_numeric_float_narrow(value: f64) -> Result<f32, &'static str> {
    if !value.is_finite() || value < -(f32::MAX as f64) || value > f32::MAX as f64 {
        return Err(JET_NUMERIC_F32_CONVERSION_ERROR);
    }
    let narrowed = value as f32;
    narrowed
        .is_finite()
        .then_some(narrowed)
        .ok_or(JET_NUMERIC_F32_CONVERSION_ERROR)
}

/// Reconstruct a fixed-width source's raw carrier, then use the same range
/// table as the default `Int` conversion. Unsigned sources never pass through
/// a signed intermediate.
pub fn jet_numeric_try_from_fixed(
    raw: u64,
    signed: bool,
    kind: i64,
) -> Result<i128, &'static str> {
    let value = if signed {
        (raw as i64) as i128
    } else {
        raw as i128
    };
    jet_numeric_fixed_from_i128(value, kind)
        .ok_or(JET_NUMERIC_CONVERSION_ERROR)
}

#[cfg(test)]
mod numeric_widen_tests {
    use super::jet_numeric_checked_widen;

    #[test]
    fn checked_widen_accepts_exact_runtime_values_and_rejects_rounding() {
        assert_eq!(
            jet_numeric_checked_widen(9_007_199_254_740_992, true, false),
            Some(9_007_199_254_740_992.0)
        );
        assert_eq!(
            jet_numeric_checked_widen(9_007_199_254_740_993, true, false),
            None
        );
        assert_eq!(
            jet_numeric_checked_widen((-9_007_199_254_740_992i64) as u64, true, false),
            Some(-9_007_199_254_740_992.0)
        );
        assert_eq!(
            jet_numeric_checked_widen((-9_007_199_254_740_993i64) as u64, true, false),
            None
        );
        assert_eq!(
            jet_numeric_checked_widen(16_777_216, false, true),
            Some(16_777_216.0)
        );
        assert_eq!(jet_numeric_checked_widen(16_777_217, false, true), None);
        assert!(jet_numeric_checked_widen(i64::MIN as u64, true, false).is_some());
        assert!(jet_numeric_checked_widen(u64::MAX, false, false).is_none());
    }
}
