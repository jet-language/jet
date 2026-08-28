// D-FMT-INTERP3=B: AOT adapts the packed exact-Int carrier to the shared
// formatting kernel. The conversion is carrier marshalling; the hexadecimal
// algorithm remains in Core/Fmt.rs.

pub(crate) fn jet_fmt_hex(value: i64, width: i64) -> String {
    jet_fmt_hex_decimal(&jet_std::jet_int_to_string(value), width)
}
