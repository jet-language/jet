//! Lowercase hexadecimal byte encoding shared by host tools.
//!
//! Encoding always writes two lowercase digits per byte. Decoding accepts
//! either case, requires an even number of digits, and rejects anything else
//! (signs, whitespace, non-ASCII). Callers that need a stricter alphabet or a
//! different error shape map the `None` themselves.

const DIGITS: &[u8; 16] = b"0123456789abcdef";

/// Two lowercase hex digits per byte.
pub fn encode(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push(DIGITS[(byte >> 4) as usize] as char);
        text.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    text
}

/// Value of one ASCII hex digit in either case.
pub fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Decode an even-length run of hex digits in either case.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let bytes = text.as_bytes();
    if bytes.len() % 2 != 0 {
        return None;
    }
    let mut decoded = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        decoded.push((nibble(pair[0])? << 4) | nibble(pair[1])?);
    }
    Some(decoded)
}
