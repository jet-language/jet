/// D-TYPE2-SPELL1: the shared check-and-convert kernel for an inline integer
/// refinement. The carrier stays `i64`; only the interval fact is checked.
pub(crate) fn jet_inline_range_from_int(value: i64, lo: i64, hi: i64) -> Result<i64, String> {
    if value >= lo && value <= hi {
        Ok(value)
    } else {
        Err(format!("value is outside Int({lo}..{hi})"))
    }
}
