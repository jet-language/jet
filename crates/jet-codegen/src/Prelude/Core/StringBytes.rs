/// D-BYTESDECODE1: strict and lossy UTF-8 conversion share one Prelude kernel.
pub fn jet_string_decode_utf8(bytes: &[u8]) -> Result<String, String> {
    String::from_utf8(bytes.to_vec()).map_err(|error| error.to_string())
}

/// D-BYTESDECODE1: lossy decoding is explicit at the API boundary, but uses
/// the same embedded Prelude kernel on every execution tier.
pub fn jet_string_decode_utf8_lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
