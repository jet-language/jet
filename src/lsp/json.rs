//! Minimal hand-rolled JSON (parse only what LSP needs) — invariant I6.

use std::collections::HashMap;

#[derive(Debug, Clone)]
pub(crate) enum JsonValue {
    Null,
    Bool(bool),
    Number(i64),
    Flt(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(HashMap<String, JsonValue>),
}

pub(crate) fn parse_json(text: &str) -> Result<JsonValue, ()> {
    let mut p = JsonParser { s: text, i: 0 };
    let v = p.value()?;
    p.skip_ws();
    if p.i < p.s.len() {
        return Err(());
    }
    Ok(v)
}

struct JsonParser<'a> {
    s: &'a str,
    i: usize,
}

impl<'a> JsonParser<'a> {
    fn peek(&self) -> Option<char> {
        self.s[self.i..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.i += c.len_utf8();
        Some(c)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t' | '\n' | '\r')) {
            self.bump();
        }
    }

    fn value(&mut self) -> Result<JsonValue, ()> {
        self.skip_ws();
        match self.peek() {
            Some('n') => {
                self.expect_literal("null")?;
                Ok(JsonValue::Null)
            }
            Some('t') => {
                self.expect_literal("true")?;
                Ok(JsonValue::Bool(true))
            }
            Some('f') => {
                self.expect_literal("false")?;
                Ok(JsonValue::Bool(false))
            }
            Some('"') => Ok(JsonValue::String(self.string()?)),
            Some('[') => {
                self.bump();
                let mut arr = Vec::new();
                self.skip_ws();
                if self.peek() == Some(']') {
                    self.bump();
                    return Ok(JsonValue::Array(arr));
                }
                loop {
                    arr.push(self.value()?);
                    self.skip_ws();
                    match self.bump() {
                        Some(',') => continue,
                        Some(']') => break,
                        _ => return Err(()),
                    }
                }
                Ok(JsonValue::Array(arr))
            }
            Some('{') => {
                self.bump();
                let mut obj = HashMap::new();
                self.skip_ws();
                if self.peek() == Some('}') {
                    self.bump();
                    return Ok(JsonValue::Object(obj));
                }
                loop {
                    self.skip_ws();
                    let key = self.string()?;
                    self.skip_ws();
                    if self.bump() != Some(':') {
                        return Err(());
                    }
                    obj.insert(key, self.value()?);
                    self.skip_ws();
                    match self.bump() {
                        Some(',') => continue,
                        Some('}') => break,
                        _ => return Err(()),
                    }
                }
                Ok(JsonValue::Object(obj))
            }
            Some(c) if c == '-' || c.is_ascii_digit() => {
                let start = self.i;
                if self.peek() == Some('-') {
                    self.bump();
                }
                while matches!(self.peek(), Some('0'..='9')) {
                    self.bump();
                }
                let is_float = matches!(self.peek(), Some('.') | Some('e') | Some('E'));
                if is_float {
                    if self.peek() == Some('.') {
                        self.bump();
                        while matches!(self.peek(), Some('0'..='9')) {
                            self.bump();
                        }
                    }
                    if matches!(self.peek(), Some('e') | Some('E')) {
                        self.bump();
                        if matches!(self.peek(), Some('+') | Some('-')) {
                            self.bump();
                        }
                        while matches!(self.peek(), Some('0'..='9')) {
                            self.bump();
                        }
                    }
                    let s = &self.s[start..self.i];
                    Ok(JsonValue::Flt(s.parse().map_err(|_| ())?))
                } else {
                    Ok(JsonValue::Number(
                        self.s[start..self.i].parse().map_err(|_| ())?,
                    ))
                }
            }
            _ => Err(()),
        }
    }

    fn expect_literal(&mut self, lit: &str) -> Result<(), ()> {
        if self.s[self.i..].starts_with(lit) {
            self.i += lit.len();
            Ok(())
        } else {
            Err(())
        }
    }

    fn string(&mut self) -> Result<String, ()> {
        if self.bump() != Some('"') {
            return Err(());
        }
        let mut out = String::new();
        while let Some(c) = self.peek() {
            if c == '"' {
                self.bump();
                return Ok(out);
            }
            if c == '\\' {
                self.bump();
                let esc = self.bump().ok_or(())?;
                out.push(match esc {
                    '"' => '"',
                    '\\' => '\\',
                    '/' => '/',
                    'b' => '\x08',
                    'f' => '\x0c',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'u' => {
                        let hex: String = self.s[self.i..].chars().take(4).collect();
                        if hex.len() != 4 {
                            return Err(());
                        }
                        self.i += 4;
                        char::from_u32(u32::from_str_radix(&hex, 16).map_err(|_| ())?).ok_or(())?
                    }
                    _ => return Err(()),
                });
            } else {
                self.bump();
                out.push(c);
            }
        }
        Err(())
    }
}

pub(crate) fn json_get<'a>(v: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    match v {
        JsonValue::Object(m) => m.get(key),
        _ => None,
    }
}

pub(crate) fn json_str(v: &JsonValue) -> Option<&str> {
    match v {
        JsonValue::String(s) => Some(s),
        _ => None,
    }
}

pub(crate) fn json_int(v: &JsonValue) -> Option<i64> {
    match v {
        JsonValue::Number(n) => Some(*n),
        JsonValue::Flt(f) => Some(*f as i64),
        _ => None,
    }
}

pub(crate) fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
