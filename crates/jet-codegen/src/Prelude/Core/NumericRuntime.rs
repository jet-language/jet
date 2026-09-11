/// Attach source context to the shared numeric widening policy. Both hosted
/// and portable Prelude closures supply the same arithmetic stop interface.
pub fn jet_numeric_checked_widen_at(
    raw: u64,
    signed: bool,
    target_f32: bool,
    file: &str,
    line: u32,
) -> f64 {
    jet_numeric_checked_widen(raw, signed, target_f32).unwrap_or_else(|| {
        crate::jet_arithmetic_stop(file, line, JET_NUMERIC_WIDEN_TRAP)
    })
}

pub fn jet_numeric_int_bit_count(value: i64, operation: i64, width: i64) -> i64 {
    let method = match operation {
        0 => "count_ones",
        1 => "count_zeros",
        2 => "leading_zeros",
        3 => "trailing_zeros",
        _ => panic!("checked integer population operation"),
    };
    jet_std::jet_int_bit_count(value, width as u32, method)
}
