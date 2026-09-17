// D-TYPEDTEXT1=D: one typed-text semantic core for AOT, JIT, and TIR
// adapters. SQL keeps its binding element type at the adapter boundary so the
// checked SQL carrier can use `DBValue` while the standalone kernel stays
// independent of the database wire module.

pub fn jet_typed_sql_raw<T>(template: String) -> (String, Vec<T>) {
    (template, Vec::new())
}

pub fn jet_typed_sql_interpolate<T>(
    literals: &[&str],
    holes: Vec<T>,
) -> (String, Vec<T>) {
    let capacity = literals.iter().map(|literal| literal.len()).sum::<usize>()
        + holes.len();
    let mut template = String::with_capacity(capacity);
    for (index, literal) in literals.iter().enumerate() {
        template.push_str(literal);
        if index < holes.len() {
            template.push('?');
        }
    }
    (template, holes)
}

pub fn jet_typed_sql_template<T>(value: &(String, Vec<T>)) -> String {
    value.0.clone()
}

pub fn jet_typed_sql_params<T: Clone>(value: &(String, Vec<T>)) -> Vec<T> {
    value.1.clone()
}

pub fn jet_typed_html_raw(value: String) -> String {
    value
}

pub fn jet_typed_html_text(value: String) -> String {
    value
}

pub fn jet_typed_html_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn jet_typed_html_interpolate(
    literals: &[&str],
    holes: Vec<String>,
    trusted_html: &[bool],
) -> String {
    let capacity = literals.iter().map(|literal| literal.len()).sum::<usize>()
        + holes.iter().map(String::len).sum::<usize>();
    let mut out = String::with_capacity(capacity);
    let mut holes = holes.into_iter();
    for (index, literal) in literals.iter().enumerate() {
        out.push_str(literal);
        if let Some(hole) = holes.next() {
            if trusted_html.get(index).copied().unwrap_or(false) {
                out.push_str(&hole);
            } else {
                out.push_str(&jet_typed_html_escape(&hole));
            }
        }
    }
    out
}

pub fn jet_typed_sh_raw(value: String) -> Vec<String> {
    value.split_whitespace().map(str::to_string).collect()
}

pub fn jet_typed_sh_interpolate(literals: &[&str], holes: Vec<String>) -> Vec<String> {
    let mut argv = Vec::with_capacity(holes.len());
    let mut holes = holes.into_iter();
    for literal in literals {
        argv.extend(literal.split_whitespace().map(str::to_string));
        if let Some(hole) = holes.next() {
            argv.push(hole);
        }
    }
    argv
}

// D-BOUND-HEAD1=A: checked boundary heads use this same interpolation kernel
// in AOT, JIT, interpreter, and comptime adapters. The adapters marshal lists
// and nominal values; they do not decide encoding or traversal policy.

pub fn jet_typed_path_interpolate(literals: &[&str], holes: &[String]) -> String {
    interpolate_typed_head(literals, holes, encode_path_component)
}

pub fn jet_typed_datetime_interpolate(literals: &[&str], holes: &[String]) -> String {
    interpolate_typed_head(literals, holes, |hole| hole.to_string())
}

pub fn jet_validate_typed_path_literal(literals: &[&str]) -> Result<(), String> {
    if literals.iter().any(|literal| literal.contains('\0')) {
        Err("a Path cannot contain a NUL character".to_string())
    } else {
        Ok(())
    }
}

fn interpolate_typed_head(
    literals: &[&str],
    holes: &[String],
    encode_hole: impl Fn(&str) -> String,
) -> String {
    let capacity = literals.iter().map(|literal| literal.len()).sum::<usize>()
        + holes.iter().map(String::len).sum::<usize>();
    let mut out = String::with_capacity(capacity);
    for (index, literal) in literals.iter().enumerate() {
        out.push_str(literal);
        if let Some(hole) = holes.get(index) {
            out.push_str(&encode_hole(hole));
        }
    }
    out
}

fn encode_path_component(value: &str) -> String {
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
    percent_encode_path(value)
}

fn percent_encode_path(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for &byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(typed_text_hex_digit(byte >> 4));
            out.push(typed_text_hex_digit(byte & 0x0f));
        }
    }
    out
}

fn typed_text_hex_digit(nibble: u8) -> char {
    match nibble {
        0..=9 => char::from(b'0' + nibble),
        10..=15 => char::from(b'A' + nibble - 10),
        _ => unreachable!("percent hex nibble is always four bits"),
    }
}

#[cfg(test)]
mod typed_boundary_tests {
    use super::*;

    #[test]
    fn html_interpolation_escapes_text_and_composes_html() {
        assert_eq!(
            jet_typed_html_interpolate(
                &["<p>", "</p>", ""],
                vec!["<script>&".to_string(), "<strong>safe</strong>".to_string(),],
                &[false, true],
            ),
            "<p>&lt;script&gt;&amp;</p><strong>safe</strong>"
        );
    }

    #[test]
    fn missing_html_trust_flags_default_to_escaped() {
        assert_eq!(
            jet_typed_html_interpolate(&["", ""], vec!["<em>text</em>".to_string()], &[]),
            "&lt;em&gt;text&lt;/em&gt;"
        );
    }

    #[test]
    fn path_holes_are_one_component() {
        assert_eq!(
            jet_typed_path_interpolate(&["/data/", ".json"], &["ada/../etc".to_string()],),
            "/data/ada%2F..%2Fetc.json"
        );
        assert_eq!(
            jet_typed_path_interpolate(&["/data/", ""], &["..".to_string()]),
            "/data/%2E%2E"
        );
    }

    #[test]
    fn path_literal_validation_rejects_nul() {
        assert!(jet_validate_typed_path_literal(&["/data/log"]).is_ok());
        assert!(jet_validate_typed_path_literal(&["/data/\0log"]).is_err());
    }
}
