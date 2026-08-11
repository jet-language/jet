// D-SERDE2 / R12: named temporal and Decimal wire semantics live here once.
// AOT, JIT, and the interpreter include this adapter with their own value
// handles; none of those tiers re-implement parsing or canonical formatting.

pub(crate) fn jet_codec_date_encode(value: &JetDate) -> String {
    value.to_string_fmt()
}

pub(crate) fn jet_codec_date_decode(value: &str) -> Result<JetDate, String> {
    JetDate::parse(value)
}

pub(crate) fn jet_codec_local_time_encode(value: &JetLocalTime) -> String {
    value.to_string_fmt()
}

pub(crate) fn jet_codec_local_time_decode(value: &str) -> Result<JetLocalTime, String> {
    JetLocalTime::parse(value)
}

pub(crate) fn jet_codec_datetime_encode(value: &JetDateTime) -> String {
    value.format_rfc3339()
}

pub(crate) fn jet_codec_datetime_decode(value: &str) -> Result<JetDateTime, String> {
    JetDateTime::parse_rfc3339(value)
}

pub(crate) fn jet_codec_duration_encode(ns: i64) -> i64 {
    ns
}

pub(crate) fn jet_codec_duration_decode(ns: i64) -> i64 {
    ns
}

pub(crate) fn jet_codec_decimal_encode(value: &jet_std::JetDecimal) -> String {
    value.to_string_rep()
}

pub(crate) fn jet_codec_decimal_decode_text(
    value: &str,
) -> Result<jet_std::JetDecimal, String> {
    jet_std::JetDecimal::from_str(value)
}

pub(crate) fn jet_codec_decimal_decode_int(
    value: i64,
) -> Result<jet_std::JetDecimal, String> {
    jet_std::JetDecimal::from_str(&value.to_string())
}
