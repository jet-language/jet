use crate::Dyn::Dyn;

// The same handle-free EncodingError ABI used by the emitted Prelude.
#[allow(dead_code)]
mod encoding_types {
    use crate::Outcome::{JetAbsent, JetOutcome};
    include!("../../jet-codegen/src/Prelude/CoreLib/JetStd/EncodingTypes.rs");
    include!("../../jet-codegen/src/Prelude/Core/EncodingError.rs");
}
pub use encoding_types::{EncodingCause, EncodingError, EncodingErrorKind, EncodingFormat};

pub fn json_unsupported(reason: impl Into<String>) -> EncodingError {
    EncodingError::new(
        EncodingFormat::JSON,
        EncodingErrorKind::Unsupported,
        0,
        Err(crate::Outcome::JetAbsent),
        Err(crate::Outcome::JetAbsent),
        "$",
        reason,
    )
}

pub fn encoding_error_show(error: &EncodingError) -> String {
    encoding_types::jet_encoding_error_kernel_show(
        error.format.as_str(),
        &format!("{:?}", error.kind),
        error.byte_offset,
        error.line.as_ref().ok().copied(),
        error.column.as_ref().ok().copied(),
        &error.path,
        &error.reason,
    )
}

// One JSON parser for the embedded Prelude and comptime adapters.

pub const MAX_JSON_DEPTH: usize = 64;


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_nested_json_is_rejected_before_unbounded_recursion() {
        let depth = MAX_JSON_DEPTH + 1;
        let input = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        let error = parse_json(&input, false).expect_err("depth limit must reject input");
        assert!(error.message.contains("nested too deeply"));
    }

    #[test]
    fn integer_tokens_never_round_through_float() {
        assert_eq!(
            parse_json("-9223372036854775808", false),
            Ok(Dyn::Int(i64::MIN.into()))
        );
        assert_eq!(
            parse_json("9223372036854775808", false),
            Ok(Dyn::Number("9223372036854775808".to_string()))
        );
        assert_eq!(
            parse_json("123456789012345678901234567890", false),
            Ok(Dyn::Number("123456789012345678901234567890".to_string()))
        );
    }

    #[test]
    fn exact_json_keeps_quoted_numbers_distinct() {
        assert_eq!(
            parse_json_exact_numbers(r#"["123",123,1.50]"#, false),
            Ok(Dyn::Array(vec![
                Dyn::Text("123".to_string()),
                Dyn::Number("123".to_string()),
                Dyn::Number("1.50".to_string()),
            ]))
        );
    }

    #[test]
    fn rejects_trailing_commas_and_non_finite_numbers() {
        for input in ["[1,]", "{\"a\":1,}", "1e400", "-1e400"] {
            parse_json(input, false).expect_err("invalid JSON must be rejected");
        }
    }

    #[test]
    fn native_dyn_scalars_render_as_text_without_changing_json_parse_kinds() {
        let tree = Dyn::Object(vec![
            ("b".into(), Dyn::Bytes(vec![0, 255])),
            ("a".into(), Dyn::parse_datetime("2024-01-01T05:30:00.120+05:30").unwrap()),
        ]);
        let text = render_json(&tree, false, 0);
        assert_eq!(text, r#"{"b":"AP8=","a":"2024-01-01T05:30:00.12+05:30"}"#);
        let parsed = parse_json(&text, false).unwrap();
        assert!(matches!(parsed.get("b").unwrap(), Dyn::Text(_)));
        assert!(matches!(parsed.get("a").unwrap(), Dyn::Text(_)));
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub line: i64,
    pub message: String,
}

/// Host-internal parse: integers that fit `i64` become `Int`, other integers
/// keep their lexeme as `Number`, and fractions become `Float`.
pub fn parse_json(text: &str, reject_duplicate_keys: bool) -> Result<Dyn, Error> {
    parse_json_with_modes(text, reject_duplicate_keys, false)
}

/// The Jet `json.parse` meaning (#4602): every number token keeps its exact
/// lexeme as `Number` until a typed read range-checks it.
pub fn parse_json_exact_numbers(
    text: &str,
    reject_duplicate_keys: bool,
) -> Result<Dyn, Error> {
    parse_json_with_modes(text, reject_duplicate_keys, true)
}

fn parse_json_with_modes(
    text: &str,
    reject_duplicate_keys: bool,
    preserve_numbers: bool,
) -> Result<Dyn, Error> {
    let mut parser = Parser {
        chars: text.chars().collect(),
        pos: 0,
        reject_duplicate_keys,
        preserve_numbers,
    };
    let value = parser.value(0)?;
    parser.ws();
    if parser.pos != parser.chars.len() {
        return Err(parser.err(super::jet_encoding_errors::JSON_EXTRA_TEXT));
    }
    Ok(value)
}

pub fn is_json_structural_whitespace(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    reject_duplicate_keys: bool,
    preserve_numbers: bool,
}

impl Parser {
    fn err(&self, message: &str) -> Error {
        let line = self.chars[..self.pos.min(self.chars.len())]
            .iter()
            .filter(|c| **c == '\n')
            .count() as i64
            + 1;
        Error {
            line,
            message: message.to_string(),
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn ws(&mut self) {
        while self.pos < self.chars.len() && is_json_structural_whitespace(self.chars[self.pos]) {
            self.pos += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<Dyn, Error> {
        self.ws();
        match self.peek() {
            Some('n') => self.word("null", Dyn::Null),
            Some('t') => self.word("true", Dyn::Bool(true)),
            Some('f') => self.word("false", Dyn::Bool(false)),
            Some('"') => Ok(Dyn::Text(self.string()?)),
            Some('[') => self.array(depth),
            Some('{') => self.object(depth),
            Some('-') | Some('0'..='9') => self.number(),
            _ => Err(self.err(super::jet_encoding_errors::JSON_EXPECTED_VALUE)),
        }
    }

    fn word(&mut self, word: &str, value: Dyn) -> Result<Dyn, Error> {
        for ch in word.chars() {
            if self.peek() != Some(ch) {
                return Err(self.err(super::jet_encoding_errors::JSON_EXPECTED_WORD));
            }
            self.pos += 1;
        }
        Ok(value)
    }

    fn string(&mut self) -> Result<String, Error> {
        if self.peek() != Some('"') {
            return Err(self.err(super::jet_encoding_errors::JSON_EXPECTED_QUOTED_TEXT));
        }
        self.pos += 1;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.pos += 1;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let Some(escape) = self.peek() else {
                        return Err(self.err(super::jet_encoding_errors::JSON_UNFINISHED_ESCAPE));
                    };
                    self.pos += 1;
                    match escape {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{0008}'),
                        'f' => out.push('\u{000c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => self.unicode_escape(&mut out)?,
                        _ => return Err(self.err(super::jet_encoding_errors::JSON_INVALID_ESCAPE)),
                    }
                }
                c if (c as u32) < 0x20 => {
                    return Err(self.err(super::jet_encoding_errors::JSON_CONTROL_CHARACTER));
                }
                other => out.push(other),
            }
        }
        Err(self.err(super::jet_encoding_errors::JSON_MISSING_CLOSING_QUOTE))
    }

    fn unicode_escape(&mut self, out: &mut String) -> Result<(), Error> {
        let code_point = self.hex4()?;
        if (0xD800..=0xDBFF).contains(&code_point) {
            if self.peek() != Some('\\') {
                return Err(self.err(super::jet_encoding_errors::JSON_UNPAIRED_SURROGATE));
            }
            self.pos += 1;
            if self.peek() != Some('u') {
                return Err(self.err(super::jet_encoding_errors::JSON_UNPAIRED_SURROGATE));
            }
            self.pos += 1;
            let low = self.hex4()?;
            if !(0xDC00..=0xDFFF).contains(&low) {
                return Err(self.err(super::jet_encoding_errors::JSON_UNPAIRED_SURROGATE));
            }
            let combined = 0x10000 + ((code_point - 0xD800) << 10) + (low - 0xDC00);
            match char::from_u32(combined) {
                Some(ch) => out.push(ch),
                None => {
                    return Err(self.err(super::jet_encoding_errors::JSON_INVALID_UNICODE_ESCAPE));
                }
            }
        } else if (0xDC00..=0xDFFF).contains(&code_point) {
            return Err(self.err(super::jet_encoding_errors::JSON_UNPAIRED_SURROGATE));
        } else {
            match char::from_u32(code_point) {
                Some(ch) => out.push(ch),
                None => {
                    return Err(self.err(super::jet_encoding_errors::JSON_INVALID_UNICODE_ESCAPE));
                }
            }
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, Error> {
        let mut value = 0u32;
        for _ in 0..4 {
            let Some(ch) = self.peek() else {
                return Err(self.err(super::jet_encoding_errors::JSON_TRUNCATED_UNICODE_ESCAPE));
            };
            let digit = ch
                .to_digit(16)
                .ok_or_else(|| self.err(super::jet_encoding_errors::JSON_INVALID_UNICODE_ESCAPE))?;
            value = value * 16 + digit;
            self.pos += 1;
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<Dyn, Error> {
        let start = self.pos;
        if self.peek() == Some('-') {
            self.pos += 1;
        }
        match self.peek() {
            Some('0') => self.pos += 1,
            Some('1'..='9') => {
                self.pos += 1;
                while matches!(self.peek(), Some('0'..='9')) {
                    self.pos += 1;
                }
            }
            _ => return Err(self.err(super::jet_encoding_errors::JSON_BAD_NUMBER)),
        }
        if self.peek() == Some('.') {
            self.pos += 1;
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err(self.err(super::jet_encoding_errors::JSON_BAD_NUMBER));
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.pos += 1;
            }
        }
        if matches!(self.peek(), Some('e' | 'E')) {
            self.pos += 1;
            if matches!(self.peek(), Some('+' | '-')) {
                self.pos += 1;
            }
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err(self.err(super::jet_encoding_errors::JSON_BAD_NUMBER));
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.pos += 1;
            }
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        let is_integer = !text.contains('.') && !text.contains('e') && !text.contains('E');
        if self.preserve_numbers {
            super::jet_json_number::validate_json_number(&text)
                .map_err(|message| self.err(&message))?;
            return Ok(Dyn::Number(text));
        }
        if is_integer {
            if let Ok(value) = text.parse::<i64>() {
                return Ok(Dyn::Int((value).into()));
            }
            super::jet_json_number::validate_json_number(&text)
                .map_err(|message| self.err(&message))?;
            return Ok(Dyn::Number(text));
        }
        let value = text
            .parse::<f64>()
            .map_err(|_| self.err(super::jet_encoding_errors::JSON_BAD_NUMBER))?;
        if !value.is_finite() {
            return Err(self.err(super::jet_encoding_errors::JSON_BAD_NUMBER));
        }
        Ok(Dyn::Float(value))
    }

    fn array(&mut self, depth: usize) -> Result<Dyn, Error> {
        if depth >= MAX_JSON_DEPTH {
            return Err(self.err("JSON value is nested too deeply"));
        }
        self.pos += 1;
        let mut values = Vec::new();
        loop {
            self.ws();
            if self.peek() == Some(']') {
                self.pos += 1;
                return Ok(Dyn::Array(values));
            }
            values.push(self.value(depth + 1)?);
            self.ws();
            match self.peek() {
                Some(',') => {
                    self.pos += 1;
                    self.ws();
                    if self.peek() == Some(']') {
                        return Err(self.err(super::jet_encoding_errors::JSON_EXPECTED_ARRAY_SEPARATOR));
                    }
                }
                Some(']') => {}
                _ => {
                    return Err(self.err(super::jet_encoding_errors::JSON_EXPECTED_ARRAY_SEPARATOR));
                }
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Dyn, Error> {
        if depth >= MAX_JSON_DEPTH {
            return Err(self.err("JSON value is nested too deeply"));
        }
        self.pos += 1;
        let mut fields = Vec::new();
        loop {
            self.ws();
            if self.peek() == Some('}') {
                self.pos += 1;
                return Ok(Dyn::Object(fields));
            }
            let key = self.string()?;
            self.ws();
            if self.peek() != Some(':') {
                return Err(self.err(super::jet_encoding_errors::JSON_EXPECTED_OBJECT_COLON));
            }
            self.pos += 1;
            if self.reject_duplicate_keys && fields.iter().any(|(field, _)| field == &key) {
                return Err(self.err(&format!(
                    "{} `{key}`",
                    super::jet_encoding_errors::JSON_DUPLICATE_OBJECT_KEY
                )));
            }
            let value = self.value(depth + 1)?;
            if let Some((_, current)) = fields.iter_mut().find(|(field, _)| field == &key) {
                *current = value;
            } else {
                fields.push((key, value));
            }
            self.ws();
            match self.peek() {
                Some(',') => {
                    self.pos += 1;
                    self.ws();
                    if self.peek() == Some('}') {
                        return Err(self.err(super::jet_encoding_errors::JSON_EXPECTED_OBJECT_SEPARATOR));
                    }
                }
                Some('}') => {}
                _ => {
                    return Err(
                        self.err(super::jet_encoding_errors::JSON_EXPECTED_OBJECT_SEPARATOR)
                    );
                }
            }
        }
    }
}
    pub fn render_json(j: &Dyn, pretty: bool, depth: usize) -> String {
        match j {
            Dyn::Null => "null".to_string(),
            Dyn::Bool(value) => value.to_string(),
            Dyn::Int(value) => value.to_string(),
            Dyn::Float(value) => {
                assert!(value.is_finite(), "JSON cannot encode a non-finite Float");
                format!("{value:?}")
            }
            Dyn::Number(value) => value.clone(),
            Dyn::Text(value) => quote_json(value),
            Dyn::Bytes(value) => quote_json(&Dyn::bytes_text(value)),
            Dyn::DateTime(value) => quote_json(&value.to_rfc3339()),
            Dyn::Array(items) => {
                if items.is_empty() {
                    return "[]".to_string();
                }
                if !pretty {
                    let parts: Vec<String> =
                        items.iter().map(|x| render_json(x, false, depth)).collect();
                    return format!("[{}]", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = items
                    .iter()
                    .map(|x| format!("{}{}", pad, render_json(x, true, depth + 1)))
                    .collect();
                format!("[\n{}\n{}]", parts.join(",\n"), end)
            }
            Dyn::Object(entries) => {
                if entries.is_empty() {
                    return "{}".to_string();
                }
                if !pretty {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(key, value)| {
                            format!("{}:{}", quote_json(key), render_json(value, false, depth))
                        })
                        .collect();
                    return format!("{{{}}}", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(key, value)| {
                        format!(
                            "{}{}: {}",
                            pad,
                            quote_json(key),
                            render_json(value, true, depth + 1)
                        )
                    })
                    .collect();
                format!("{{\n{}\n{}}}", parts.join(",\n"), end)
            }
        }
    }

    pub fn quote_json(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 2);
        append_quote_json(&mut out, s);
        out
    }

pub fn append_quote_json(out: &mut String, s: &str) {
    use std::fmt::Write as _;
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => write!(out, "\\u{:04x}", c as u32).unwrap(),
            _ => out.push(c),
        }
    }
    out.push('"');
}
