/// D-NUMWIDEN-CROSS1=E: canonical checked integer-to-float widening policy.
///
/// A binary float with `precision` significant bits holds an integer exactly
/// when either the integer fits in that precision or its discarded low bits
/// are all zero. Engines pass the integer's raw bits and signedness; no engine
/// reconstructs this rule.
fn jet_numeric_integer_is_exact(raw: u64, signed: bool, precision: u32) -> bool {
    let magnitude = if signed {
        (raw as i64).unsigned_abs()
    } else {
        raw
    };
    if magnitude == 0 {
        return true;
    }
    let significant = u64::BITS - magnitude.leading_zeros();
    significant <= precision || magnitude.trailing_zeros() >= significant - precision
}

pub const JET_NUMERIC_WIDEN_TRAP: &str =
    "whole number cannot cross into the decimal without losing precision";

pub fn jet_numeric_checked_widen(
    raw: u64,
    signed: bool,
    target_f32: bool,
) -> Option<f64> {
    let precision = if target_f32 { 24 } else { 53 };
    if !jet_numeric_integer_is_exact(raw, signed, precision) {
        return None;
    }
    let value = if signed {
        (raw as i64) as f64
    } else {
        raw as f64
    };
    Some(if target_f32 {
        (value as f32) as f64
    } else {
        value
    })
}

#[cfg(test)]
mod tests {
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
