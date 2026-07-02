//! RFC 8259 JSON parse/render for the comptime/REPL interpreter (std-only, I6).
//! Mirrors `jet_std::parse_json` / `render_json` in the codegen prelude.

use std::collections::BTreeMap;

use crate::AST::{CtKey, CtValue};

#[derive(Clone, Debug)]
pub(super) struct JsonError {
    line: i64,
    message: String,
}

fn json_err(line: i64, message: impl Into<String>) -> JsonError {
    JsonError {
        line,
        message: message.into(),
    }
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn line(&self) -> i64 {
        self.chars[..self.pos.min(self.chars.len())]
            .iter()
            .filter(|c| **c == '\n')
            .count() as i64
            + 1
    }

    fn err(&self, msg: &str) -> JsonError {
        json_err(self.line(), msg)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn ws(&mut self) {
        while self.pos < self.chars.len() && self.chars[self.pos].is_whitespace() {
            self.pos += 1;
        }
    }

    fn parse_value(&mut self) -> Result<CtValue, JsonError> {
        self.ws();
        match self.peek() {
            Some('n') => self.word("null", CtValue::Unit),
            Some('t') => self.word("true", CtValue::Bool(true)),
            Some('f') => self.word("false", CtValue::Bool(false)),
            Some('"') => Ok(CtValue::Str(self.string()?)),
            Some('[') => self.array(),
            Some('{') => self.object(),
            Some('-') | Some('0'..='9') => self.number(),
            _ => Err(self.err("expected a JSON value")),
        }
    }

    fn word(&mut self, w: &str, v: CtValue) -> Result<CtValue, JsonError> {
        for ch in w.chars() {
            if self.peek() != Some(ch) {
                return Err(self.err("expected a JSON word"));
            }
            self.pos += 1;
        }
        Ok(v)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        if self.peek() != Some('"') {
            return Err(self.err("expected quoted text"));
        }
        self.pos += 1;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            self.pos += 1;
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let Some(e) = self.peek() else {
                        return Err(self.err("unfinished escape"));
                    };
                    self.pos += 1;
                    match e {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{0008}'),
                        'f' => out.push('\u{000c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => self.unicode_escape(&mut out)?,
                        _ => return Err(self.err("invalid escape in string")),
                    }
                }
                c if (c as u32) < 0x20 => return Err(self.err("control character in string")),
                other => out.push(other),
            }
        }
        Err(self.err("missing closing quote"))
    }

    fn unicode_escape(&mut self, out: &mut String) -> Result<(), JsonError> {
        let cp = self.hex4()?;
        match char::from_u32(cp) {
            Some(ch) => out.push(ch),
            None => return Err(self.err("invalid unicode escape")),
        }
        Ok(())
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let mut v = 0u32;
        for _ in 0..4 {
            let Some(c) = self.peek() else {
                return Err(self.err("truncated unicode escape"));
            };
            let d = c
                .to_digit(16)
                .ok_or_else(|| self.err("invalid unicode escape"))?;
            v = v * 16 + d;
            self.pos += 1;
        }
        Ok(v)
    }

    fn number(&mut self) -> Result<CtValue, JsonError> {
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
            _ => return Err(self.err("bad number")),
        }
        if self.peek() == Some('.') {
            self.pos += 1;
            if !matches!(self.peek(), Some('0'..='9')) {
                return Err(self.err("bad number"));
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
                return Err(self.err("bad number"));
            }
            while matches!(self.peek(), Some('0'..='9')) {
                self.pos += 1;
            }
        }
        let s: String = self.chars[start..self.pos].iter().collect();
        if s.contains('.') || s.contains('e') || s.contains('E') {
            s.parse::<f64>()
                .map(CtValue::Float)
                .map_err(|_| self.err("bad number"))
        } else {
            s.parse::<i64>()
                .map(CtValue::Int)
                .map_err(|_| self.err("bad number"))
        }
    }

    fn array(&mut self) -> Result<CtValue, JsonError> {
        self.pos += 1;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Ok(CtValue::List(items));
        }
        loop {
            items.push(self.parse_value()?);
            self.ws();
            match self.peek() {
                Some(',') => {
                    self.pos += 1;
                    self.ws();
                }
                Some(']') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(self.err("expected `,` or `]` in array")),
            }
        }
        Ok(CtValue::List(items))
    }

    fn object(&mut self) -> Result<CtValue, JsonError> {
        self.pos += 1;
        let mut map = BTreeMap::new();
        self.ws();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(CtValue::Map(map));
        }
        loop {
            self.ws();
            if self.peek() != Some('"') {
                return Err(self.err("expected object key"));
            }
            let key = self.string()?;
            self.ws();
            if self.peek() != Some(':') {
                return Err(self.err("expected `:` after object key"));
            }
            self.pos += 1;
            let val = self.parse_value()?;
            map.insert(CtKey::Str(key), val);
            self.ws();
            match self.peek() {
                Some(',') => {
                    self.pos += 1;
                    self.ws();
                }
                Some('}') => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(self.err("expected `,` or `}` in object")),
            }
        }
        Ok(CtValue::Map(map))
    }
}

pub(super) fn parse_json(text: &str) -> Result<CtValue, JsonError> {
    let mut p = Parser {
        chars: text.chars().collect(),
        pos: 0,
    };
    let v = p.parse_value()?;
    p.ws();
    if p.pos != p.chars.len() {
        return Err(p.err("extra text after JSON value"));
    }
    Ok(v)
}

pub(super) fn json_error_value(e: JsonError) -> CtValue {
    CtValue::Struct {
        type_name: "JsonError".to_string(),
        fields: vec![
            ("line".to_string(), CtValue::Int(e.line)),
            ("message".to_string(), CtValue::Str(e.message)),
        ],
    }
}

pub(super) fn render_json_pretty(v: &CtValue, pretty: bool, depth: usize) -> String {
    match v {
        CtValue::Unit => "null".to_string(),
        CtValue::Bool(b) => b.to_string(),
        CtValue::Int(n) => n.to_string(),
        CtValue::Float(f) => format!("{:?}", f),
        CtValue::Str(s) => quote_json(s),
        CtValue::List(xs) => {
            if xs.is_empty() {
                return "[]".to_string();
            }
            if !pretty {
                let parts: Vec<String> = xs
                    .iter()
                    .map(|x| render_json_pretty(x, false, depth))
                    .collect();
                return format!("[{}]", parts.join(","));
            }
            let pad = "  ".repeat(depth + 1);
            let end = "  ".repeat(depth);
            let parts: Vec<String> = xs
                .iter()
                .map(|x| format!("{}{}", pad, render_json_pretty(x, true, depth + 1)))
                .collect();
            format!("[\n{}\n{}]", parts.join(",\n"), end)
        }
        CtValue::Map(m) => {
            if m.is_empty() {
                return "{}".to_string();
            }
            if !pretty {
                let parts: Vec<String> = m
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{}:{}",
                            quote_json(&match k {
                                CtKey::Str(s) => s.clone(),
                                other => other.to_value().jet_show(),
                            }),
                            render_json_pretty(v, false, depth)
                        )
                    })
                    .collect();
                return format!("{{{}}}", parts.join(","));
            }
            let pad = "  ".repeat(depth + 1);
            let end = "  ".repeat(depth);
            let parts: Vec<String> = m
                .iter()
                .map(|(k, v)| {
                    let key = match k {
                        CtKey::Str(s) => quote_json(s),
                        other => quote_json(&other.to_value().jet_show()),
                    };
                    format!("{}{}: {}", pad, key, render_json_pretty(v, true, depth + 1))
                })
                .collect();
            format!("{{\n{}\n{}}}", parts.join(",\n"), end)
        }
        other => other.to_json(),
    }
}

fn quote_json(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}
