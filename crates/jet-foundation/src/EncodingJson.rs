// One JSON parser for the embedded Prelude and comptime adapters.

pub const MAX_JSON_DEPTH: usize = 64;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    /// Original number token for an integer that does not fit `i64`, or for
    /// typed projection. Keeping the token avoids a lossy binary-float step.
    Number(String),
    Text(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

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
            Ok(Value::Int(i64::MIN))
        );
        assert_eq!(
            parse_json("9223372036854775808", false),
            Ok(Value::Number("9223372036854775808".to_string()))
        );
        assert_eq!(
            parse_json("123456789012345678901234567890", false),
            Ok(Value::Number("123456789012345678901234567890".to_string()))
        );
    }

    #[test]
    fn rejects_trailing_commas_and_non_finite_numbers() {
        for input in ["[1,]", "{\"a\":1,}", "1e400", "-1e400"] {
            parse_json(input, false).expect_err("invalid JSON must be rejected");
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Error {
    pub line: i64,
    pub message: String,
}

pub fn parse_json(text: &str, reject_duplicate_keys: bool) -> Result<Value, Error> {
    parse_json_with_number_mode(text, reject_duplicate_keys, false)
}

pub fn parse_json_exact_numbers(text: &str, reject_duplicate_keys: bool) -> Result<Value, Error> {
    parse_json_with_number_mode(text, reject_duplicate_keys, true)
}

fn parse_json_with_number_mode(
    text: &str,
    reject_duplicate_keys: bool,
    preserve_numbers: bool,
) -> Result<Value, Error> {
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

    fn value(&mut self, depth: usize) -> Result<Value, Error> {
        self.ws();
        match self.peek() {
            Some('n') => self.word("null", Value::Null),
            Some('t') => self.word("true", Value::Bool(true)),
            Some('f') => self.word("false", Value::Bool(false)),
            Some('"') => Ok(Value::Text(self.string()?)),
            Some('[') => self.array(depth),
            Some('{') => self.object(depth),
            Some('-') | Some('0'..='9') => self.number(),
            _ => Err(self.err(super::jet_encoding_errors::JSON_EXPECTED_VALUE)),
        }
    }

    fn word(&mut self, word: &str, value: Value) -> Result<Value, Error> {
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

    fn number(&mut self) -> Result<Value, Error> {
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
            return Ok(Value::Number(text));
        }
        if is_integer {
            if let Ok(value) = text.parse::<i64>() {
                return Ok(Value::Int(value));
            }
            super::jet_json_number::validate_json_number(&text)
                .map_err(|message| self.err(&message))?;
            return Ok(Value::Number(text));
        }
        let value = text
            .parse::<f64>()
            .map_err(|_| self.err(super::jet_encoding_errors::JSON_BAD_NUMBER))?;
        if !value.is_finite() {
            return Err(self.err(super::jet_encoding_errors::JSON_BAD_NUMBER));
        }
        Ok(Value::Float(value))
    }

    fn array(&mut self, depth: usize) -> Result<Value, Error> {
        if depth >= MAX_JSON_DEPTH {
            return Err(self.err("JSON value is nested too deeply"));
        }
        self.pos += 1;
        let mut values = Vec::new();
        loop {
            self.ws();
            if self.peek() == Some(']') {
                self.pos += 1;
                return Ok(Value::Array(values));
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

    fn object(&mut self, depth: usize) -> Result<Value, Error> {
        if depth >= MAX_JSON_DEPTH {
            return Err(self.err("JSON value is nested too deeply"));
        }
        self.pos += 1;
        let mut fields = Vec::new();
        loop {
            self.ws();
            if self.peek() == Some('}') {
                self.pos += 1;
                return Ok(Value::Object(fields));
            }
            let key = self.string()?;
            self.ws();
            if self.peek() != Some(':') {
                return Err(self.err(super::jet_encoding_errors::JSON_EXPECTED_OBJECT_COLON));
            }
            self.pos += 1;
            let value = self.value(depth + 1)?;
            if self.reject_duplicate_keys && fields.iter().any(|(field, _)| field == &key) {
                return Err(self.err(super::jet_encoding_errors::JSON_DUPLICATE_OBJECT_KEY));
            }
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
