// D-BOUND-HEAD1=A: shared hole law for checked URL, Path, and DateTime heads.
//
// The compiler validates the literal skeleton. Runtime and interpreter
// adapters call these functions to turn evaluated holes into the head's
// grammar before invoking the existing nominal parser/constructor.

pub const HOLE_PLACEHOLDER: &str = "jet-hole";

pub fn jet_typed_url_interpolate(literals: &[&str], holes: &[String]) -> String {
    interpolate(literals, holes, percent_encode)
}

pub fn jet_typed_path_interpolate(literals: &[&str], holes: &[String]) -> String {
    interpolate(literals, holes, path_component)
}

pub fn jet_typed_datetime_interpolate(literals: &[&str], holes: &[String]) -> String {
    interpolate(literals, holes, |hole| hole.to_string())
}

fn interpolate(
    literals: &[&str],
    holes: &[String],
    encode_hole: impl Fn(&str) -> String,
) -> String {
    let mut out = String::new();
    for (index, literal) in literals.iter().enumerate() {
        out.push_str(literal);
        if let Some(hole) = holes.get(index) {
            out.push_str(&encode_hole(hole));
        }
    }
    out
}

fn path_component(value: &str) -> String {
    // Separators and control bytes are encoded, so a hole contributes one
    // filesystem component. Encode the two traversal spellings as well;
    // `..` must never become a parent component after interpolation.
    if matches!(value, "." | "..") {
        return value
            .as_bytes()
            .iter()
            .map(|byte| format!("%{byte:02X}"))
            .collect();
    }
    percent_encode(value)
}

fn percent_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for &byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(hex(byte >> 4));
            out.push(hex(byte & 0x0f));
        }
    }
    out
}

fn hex(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'A' + nibble - 10),
        _ => unreachable!("percent hex nibble is always four bits"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_holes_are_percent_encoded() {
        assert_eq!(
            jet_typed_url_interpolate(
                &["https://api.example/items/", ""],
                &["ada lovelace/../etc".to_string()],
            ),
            "https://api.example/items/ada%20lovelace%2F..%2Fetc"
        );
    }

    #[test]
    fn path_holes_are_one_component() {
        assert_eq!(
            jet_typed_path_interpolate(
                &["/data/", ".json"],
                &["ada/../etc".to_string()],
            ),
            "/data/ada%2F..%2Fetc.json"
        );
        assert_eq!(
            jet_typed_path_interpolate(&["/data/", ""], &["..".to_string()]),
            "/data/%2E%2E"
        );
    }
}
