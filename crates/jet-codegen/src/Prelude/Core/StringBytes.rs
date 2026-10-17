/// D-BYTESDECODE1: strict public decoding and consuming Core adapters use one
/// UTF-8 validator and therefore the same failure message on every tier.
pub fn jet_string_validate_utf8(bytes: &[u8]) -> Result<(), String> {
    std::str::from_utf8(bytes).map(|_| ()).map_err(|error| error.to_string())
}

pub fn jet_string_decode_utf8(bytes: &[u8]) -> Result<String, String> {
    jet_string_validate_utf8(bytes)?;
    // The shared validator accepted the exact bytes copied here.
    Ok(unsafe { String::from_utf8_unchecked(bytes.to_vec()) })
}

/// D-BYTESDECODE1: lossy decoding is explicit at the API boundary, but uses
/// the same embedded Prelude kernel on every execution tier.
pub fn jet_string_decode_utf8_lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

// Compiler-private carriers: Jet checks validity/bounds before these transfers.
fn jet_raw_bytes_into_text(bytes: Vec<u8>) -> String {
    unsafe { String::from_utf8_unchecked(bytes) }
}

fn jet_raw_string_byte(text: &str, index: u64) -> u8 {
    unsafe { *text.as_ptr().add(index as usize) }
}

fn jet_raw_text_into_bytes(text: String) -> Vec<u8> {
    text.into_bytes()
}
