// D-FMT-INTERP3=B: AOT adapts the packed exact-Int carrier to the shared
// formatting kernel. The conversion is carrier marshalling; the formatting
// algorithms remain in Core/Fmt.rs.

pub(crate) fn jet_fmt_decimal_int_aot(value: i64, precision: i64) -> String {
    jet_fmt_decimal_int(&jet_std::jet_int_to_string(value), precision)
}

pub(crate) fn jet_fmt_grouped_int_aot(value: i64, precision: i64) -> String {
    jet_fmt_grouped_int(&jet_std::jet_int_to_string(value), precision)
}

pub(crate) fn jet_fmt_hex(value: i64, width: i64) -> String {
    jet_fmt_hex_decimal(&jet_std::jet_int_to_string(value), width)
}

pub(crate) fn jet_fmt_bin(value: i64) -> String {
    jet_fmt_bin_decimal(&jet_std::jet_int_to_string(value))
}

pub(crate) fn jet_fmt_oct(value: i64) -> String {
    jet_fmt_oct_decimal(&jet_std::jet_int_to_string(value))
}
