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
