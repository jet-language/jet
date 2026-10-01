//! Edition-aware base-encoding decode dispatch (D-ENCBASE-STRICT1).

use crate::base_encoding_strict;
use crate::XmlPull::base_encoding_2026;

pub fn decode_base64(
    edition: &str,
    text: &str,
    allow_whitespace: bool,
    allow_missing_padding: bool,
) -> Result<Vec<u8>, String> {
    if crate::PackageEdition::edition_at_least(edition, "2027") {
        base_encoding_strict::decode_base64(text, allow_whitespace, allow_missing_padding)
    } else {
        base_encoding_2026::decode_base64(text)
    }
}

pub fn decode_base64url(
    edition: &str,
    text: &str,
    allow_whitespace: bool,
    allow_padding: bool,
) -> Result<Vec<u8>, String> {
    if crate::PackageEdition::edition_at_least(edition, "2027") {
        base_encoding_strict::decode_base64url(text, allow_whitespace, allow_padding)
    } else {
        base_encoding_2026::decode_base64url(text)
    }
}

pub fn decode_base32(
    edition: &str,
    text: &str,
    allow_whitespace: bool,
    allow_missing_padding: bool,
    allow_lowercase: bool,
) -> Result<Vec<u8>, String> {
    if crate::PackageEdition::edition_at_least(edition, "2027") {
        base_encoding_strict::decode_base32(
            text,
            allow_whitespace,
            allow_missing_padding,
            allow_lowercase,
        )
    } else {
        base_encoding_2026::decode_base32(text)
    }
}

/// RFC 4648 §7 extended-hex alphabet: each symbol maps one-to-one onto the
/// standard alphabet, so byte offsets in the standard decoder's errors stay
/// the caller's offsets.
pub fn decode_base32hex(edition: &str, text: &str) -> Result<Vec<u8>, String> {
    const STANDARD: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut canonical = String::with_capacity(text.len());
    for byte in text.bytes() {
        let mapped = match byte {
            b'0'..=b'9' => STANDARD[usize::from(byte - b'0')],
            b'A'..=b'V' => STANDARD[usize::from(byte - b'A' + 10)],
            b'=' => b'=',
            _ => return Err("invalid Base32hex character".to_string()),
        };
        canonical.push(char::from(mapped));
    }
    decode_base32(edition, &canonical, false, false, false)
}
