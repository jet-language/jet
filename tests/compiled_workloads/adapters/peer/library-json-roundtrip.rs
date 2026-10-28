// #1414 peer adapter: dependency-free Rust implementation of the frozen
// public JSON roundtrip boundary. Serde JSON at the pinned revision below is
// the lineage for the library task; this adapter does not invoke serde_json,
// jq, a shell, or any installed incumbent.
// Upstream lineage: https://github.com/serde-rs/json @ efa66e3a1d61459ab2d325f92ebe3acbd6ca18b1.
use std::{env, fmt::Write, fs};

const MAX_INPUT_BYTES: usize = 1 << 20;
const MAX_DEPTH: usize = 64;

enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    BigInt(String),
    Text(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

struct Parser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn parse(mut self) -> Result<Value, ()> {
        let value = self.value(0)?;
        self.ws();
        if self.pos == self.input.len() { Ok(value) } else { Err(()) }
    }

    fn byte(&self) -> Option<u8> {
        self.input.as_bytes().get(self.pos).copied()
    }

    fn ws(&mut self) {
        while matches!(self.byte(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.pos += 1;
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, ()> {
        self.ws();
        match self.byte() {
            Some(b'n') => self.literal(b"null", Value::Null),
            Some(b't') => self.literal(b"true", Value::Bool(true)),
            Some(b'f') => self.literal(b"false", Value::Bool(false)),
            Some(b'"') => Ok(Value::Text(self.string()?)),
            Some(b'[') => self.array(depth),
            Some(b'{') => self.object(depth),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(()),
        }
    }

    fn literal(&mut self, expected: &[u8], value: Value) -> Result<Value, ()> {
        let end = self.pos.checked_add(expected.len()).ok_or(())?;
        if self.input.as_bytes().get(self.pos..end) != Some(expected) {
            return Err(());
        }
        self.pos = end;
        Ok(value)
    }

    fn string(&mut self) -> Result<String, ()> {
        if self.byte() != Some(b'"') {
            return Err(());
        }
        self.pos += 1;
        let mut out = String::new();
        loop {
            let current = self.byte().ok_or(())?;
            self.pos += 1;
            match current {
                b'"' => return Ok(out),
                b'\\' => self.escape(&mut out)?,
                0..=0x1f => return Err(()),
                0x20..=0x7f => out.push(current as char),
                _ => {
                    let rest = self.input.get(self.pos - 1..).ok_or(())?;
                    let ch = rest.chars().next().ok_or(())?;
                    if (ch as u32) < 0x20 {
                        return Err(());
                    }
                    out.push(ch);
                    self.pos = self.pos - 1 + ch.len_utf8();
                }
            }
        }
    }

    fn escape(&mut self, out: &mut String) -> Result<(), ()> {
        let escaped = self.byte().ok_or(())?;
        self.pos += 1;
        match escaped {
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'b' => out.push('\u{0008}'),
            b'f' => out.push('\u{000c}'),
            b'n' => out.push('\n'),
            b'r' => out.push('\r'),
            b't' => out.push('\t'),
            b'u' => self.unicode_escape(out)?,
            _ => return Err(()),
        }
        Ok(())
    }

    fn unicode_escape(&mut self, out: &mut String) -> Result<(), ()> {
        let high = self.hex4()?;
        if (0xd800..=0xdbff).contains(&high) {
            if self.byte() != Some(b'\\') {
                return Err(());
            }
            self.pos += 1;
            if self.byte() != Some(b'u') {
                return Err(());
            }
            self.pos += 1;
            let low = self.hex4()?;
            if !(0xdc00..=0xdfff).contains(&low) {
                return Err(());
            }
            let code = 0x10000 + ((high - 0xd800) << 10) + (low - 0xdc00);
            out.push(char::from_u32(code).ok_or(())?);
        } else if (0xdc00..=0xdfff).contains(&high) {
            return Err(());
        } else {
            out.push(char::from_u32(high).ok_or(())?);
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, ()> {
        let mut value = 0u32;
        for _ in 0..4 {
            let digit = self.byte().and_then(|byte| (byte as char).to_digit(16)).ok_or(())?;
            self.pos += 1;
            value = value * 16 + digit;
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<Value, ()> {
        let start = self.pos;
        if self.byte() == Some(b'-') {
            self.pos += 1;
        }
        match self.byte() {
            Some(b'0') => self.pos += 1,
            Some(b'1'..=b'9') => {
                self.pos += 1;
                while matches!(self.byte(), Some(b'0'..=b'9')) {
                    self.pos += 1;
                }
            }
            _ => return Err(()),
        }
        if self.byte() == Some(b'.') {
            self.pos += 1;
            if !matches!(self.byte(), Some(b'0'..=b'9')) {
                return Err(());
            }
            while matches!(self.byte(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        if matches!(self.byte(), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.byte(), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !matches!(self.byte(), Some(b'0'..=b'9')) {
                return Err(());
            }
            while matches!(self.byte(), Some(b'0'..=b'9')) {
                self.pos += 1;
            }
        }
        let token = self.input.get(start..self.pos).ok_or(())?;
        if token.bytes().any(|byte| matches!(byte, b'.' | b'e' | b'E')) {
            let value = token.parse::<f64>().map_err(|_| ())?;
            if value.is_finite() { Ok(Value::Float(value)) } else { Err(()) }
        } else if let Ok(value) = token.parse::<i64>() {
            Ok(Value::Int(value))
        } else {
            Ok(Value::BigInt(token.to_owned()))
        }
    }

    fn array(&mut self, depth: usize) -> Result<Value, ()> {
        if depth >= MAX_DEPTH {
            return Err(());
        }
        self.pos += 1;
        let mut values = Vec::new();
        self.ws();
        if self.byte() == Some(b']') {
            self.pos += 1;
            return Ok(Value::Array(values));
        }
        loop {
            values.push(self.value(depth + 1)?);
            self.ws();
            match self.byte() {
                Some(b',') => {
                    self.pos += 1;
                    self.ws();
                    if self.byte() == Some(b']') {
                        return Err(());
                    }
                }
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Value::Array(values));
                }
                _ => return Err(()),
            }
        }
    }

    fn object(&mut self, depth: usize) -> Result<Value, ()> {
        if depth >= MAX_DEPTH {
            return Err(());
        }
        self.pos += 1;
        let mut fields = Vec::new();
        self.ws();
        if self.byte() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Object(fields));
        }
        loop {
            let key = self.string()?;
            self.ws();
            if self.byte() != Some(b':') {
                return Err(());
            }
            self.pos += 1;
            let value = self.value(depth + 1)?;
            if let Some((_, current)) = fields.iter_mut().find(|(field, _)| field == &key) {
                *current = value;
            } else {
                fields.push((key, value));
            }
            self.ws();
            match self.byte() {
                Some(b',') => {
                    self.pos += 1;
                    self.ws();
                    if self.byte() == Some(b'}') {
                        return Err(());
                    }
                }
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Value::Object(fields));
                }
                _ => return Err(()),
            }
        }
    }
}

fn canonicalize(value: &mut Value) {
    match value {
        Value::Array(items) => items.iter_mut().for_each(canonicalize),
        Value::Object(fields) => {
            fields.iter_mut().for_each(|(_, value)| canonicalize(value));
            fields.sort_by(|left, right| left.0.cmp(&right.0));
        }
        _ => {}
    }
}

struct Writer {
    output: String,
}

impl Writer {
    fn new() -> Self {
        Self { output: String::new() }
    }

    fn push(&mut self, text: &str) -> Result<(), ()> {
        self.output.push_str(text);
        Ok(())
    }

    fn value(&mut self, value: &Value) -> Result<(), ()> {
        match value {
            Value::Null => self.push("null"),
            Value::Bool(value) => self.push(if *value { "true" } else { "false" }),
            Value::Int(value) => self.push(&value.to_string()),
            Value::Float(value) => self.push(&format!("{:?}", value)),
            Value::BigInt(value) => self.push(value),
            Value::Text(value) => self.string(value),
            Value::Array(items) => {
                self.push("[")?;
                for (index, item) in items.iter().enumerate() {
                    if index != 0 { self.push(",")?; }
                    self.value(item)?;
                }
                self.push("]")
            }
            Value::Object(fields) => {
                self.push("{")?;
                for (index, (key, value)) in fields.iter().enumerate() {
                    if index != 0 { self.push(",")?; }
                    self.string(key)?;
                    self.push(":")?;
                    self.value(value)?;
                }
                self.push("}")
            }
        }
    }

    fn string(&mut self, value: &str) -> Result<(), ()> {
        self.push("\"")?;
        for ch in value.chars() {
            match ch {
                '"' => self.push("\\\"")?,
                '\\' => self.push("\\\\")?,
                '\u{0008}' => self.push("\\b")?,
                '\u{000c}' => self.push("\\f")?,
                '\n' => self.push("\\n")?,
                '\r' => self.push("\\r")?,
                '\t' => self.push("\\t")?,
                c if (c as u32) < 0x20 => {
                    let mut escaped = String::with_capacity(6);
                    write!(&mut escaped, "\\u{:04x}", c as u32).map_err(|_| ())?;
                    self.push(&escaped)?;
                }
                _ => {
                    let mut encoded = [0u8; 4];
                    self.push(ch.encode_utf8(&mut encoded))?;
                }
            }
        }
        self.push("\"")
    }
}

fn transform_bytes(raw: Vec<u8>) -> Result<String, ()> {
    if raw.len() > MAX_INPUT_BYTES {
        return Err(());
    }
    let input = String::from_utf8(raw).map_err(|_| ())?;
    let mut value = Parser::new(&input).parse()?;
    canonicalize(&mut value);
    let mut writer = Writer::new();
    writer.value(&value)?;
    Ok(writer.output)
}

fn transform(path: &str) -> Result<String, ()> {
    let metadata = fs::metadata(path).map_err(|_| ())?;
    if metadata.len() > MAX_INPUT_BYTES as u64 {
        return Err(());
    }
    transform_bytes(fs::read(path).map_err(|_| ())?)
}

fn main() {
    let result = env::args().nth(1).and_then(|path| transform(&path).ok());
    match result {
        Some(canonical) => {
            println!("canonical={canonical}");
            println!("valid=true");
        }
        None => {
            println!("reject=json");
            println!("valid=false");
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn canonical(input: &str) -> Result<String, ()> {
        transform_bytes(input.as_bytes().to_vec())
    }

    #[test]
    fn canonicalization_depends_on_parsed_structure_and_values() {
        let left = canonical(r#"{"b":[true,null],"a":1}"#).unwrap();
        let right = canonical(r#"{"b":[true,false],"a":1}"#).unwrap();
        assert_ne!(left, right);
        assert_eq!(left, r#"{"a":1,"b":[true,null]}"#);
    }

    #[test]
    fn malformed_and_non_utf8_inputs_are_rejected() {
        for input in [
            br#"{"items":[1,2,}"#.to_vec(),
            br#"{"name":"unterminated}"#.to_vec(),
            vec![b'{', 0xff, b'}'],
        ] {
            assert!(transform_bytes(input).is_err());
        }
    }

    #[test]
    fn deep_and_oversize_inputs_are_rejected() {
        let depth = MAX_DEPTH + 1;
        let deep = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        assert!(canonical(&deep).is_err());
        assert!(transform_bytes(vec![b' '; MAX_INPUT_BYTES + 1]).is_err());
    }

    #[test]
    fn duplicate_keys_follow_shared_json_parser_last_value_rule() {
        assert_eq!(canonical(r#"{"a":1,"a":2}"#).unwrap(), r#"{"a":2}"#);
    }
}
