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
            Some('n') => self.word("null", json_variant("Null", None)),
            Some('t') => self.word("true", json_variant("Bool", Some(CtValue::Bool(true)))),
            Some('f') => self.word("false", json_variant("Bool", Some(CtValue::Bool(false)))),
            Some('"') => Ok(json_variant("Text", Some(CtValue::Str(self.string()?)))),
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
                .map(|f| json_variant("Float", Some(CtValue::Float(f))))
                .map_err(|_| self.err("bad number"))
        } else {
            s.parse::<i64>()
                .map(|n| json_variant("Int", Some(CtValue::Int(n))))
                .map_err(|_| self.err("bad number"))
        }
    }

    fn array(&mut self) -> Result<CtValue, JsonError> {
        self.pos += 1;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Ok(json_variant("Array", Some(CtValue::List(items))));
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
        Ok(json_variant("Array", Some(CtValue::List(items))))
    }

    fn object(&mut self) -> Result<CtValue, JsonError> {
        self.pos += 1;
        let mut map = BTreeMap::new();
        self.ws();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Ok(json_variant("Object", Some(CtValue::Map(map))));
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
        Ok(json_variant("Object", Some(CtValue::Map(map))))
    }
}

/// D-SERDE-ACCESS=B / D-DYNAMIC-TYPE1=A: build one node of the `Json`/`Data`
/// dynamic-value tree — a `CtValue::Enum` so it round-trips through the exact
/// same pattern-matching (`data == .Object(entries)`, S31) and explicit
/// construction (`Json.Text("jet")`) machinery a user enum already gets, with
/// no interpreter-specific special case needed on either of those paths.
pub(super) fn json_variant(variant: &str, payload: Option<CtValue>) -> CtValue {
    CtValue::Enum {
        type_name: "Json".to_string(),
        variant: variant.to_string(),
        args: match payload {
            Some(v) => vec![(None, v)],
            None => Vec::new(),
        },
    }
}

/// The payload of a `Json`-tagged `CtValue` with the given variant name, or
/// `None` if `v` isn't that shape (used by the `.field`/`.at`/`.int`/`.text`/
/// `.bool`/`.float` accessor methods in `Builtins.rs`).
pub(super) fn json_payload<'a>(v: &'a CtValue, variant: &str) -> Option<&'a CtValue> {
    match v {
        CtValue::Enum {
            type_name,
            variant: vname,
            args,
        } if type_name == "Json" && vname == variant => args.first().map(|(_, v)| v),
        _ => None,
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

/// D-JSON3: lenient-decode coercion — walk a parsed `Json`-tagged tree and
/// promote a `Text` leaf that reads as a number or a boolean to that type.
/// Applied only by `core.encoding.json.decode` (untyped); `.parse()` stays
/// literal.
pub(super) fn coerce_json(v: CtValue) -> CtValue {
    let CtValue::Enum {
        type_name,
        variant,
        args,
    } = v
    else {
        return v;
    };
    if type_name != "Json" {
        return CtValue::Enum {
            type_name,
            variant,
            args,
        };
    }
    match (variant.as_str(), args.into_iter().next()) {
        ("Text", Some((_, CtValue::Str(s)))) => {
            if let Ok(n) = s.parse::<i64>() {
                json_variant("Int", Some(CtValue::Int(n)))
            } else if s == "true" || s == "false" {
                json_variant("Bool", Some(CtValue::Bool(s == "true")))
            } else {
                json_variant("Text", Some(CtValue::Str(s)))
            }
        }
        ("Array", Some((_, CtValue::List(xs)))) => json_variant(
            "Array",
            Some(CtValue::List(xs.into_iter().map(coerce_json).collect())),
        ),
        ("Object", Some((_, CtValue::Map(m)))) => json_variant(
            "Object",
            Some(CtValue::Map(
                m.into_iter().map(|(k, v)| (k, coerce_json(v))).collect(),
            )),
        ),
        (variant, payload) => json_variant(variant, payload.map(|(_, v)| v)),
    }
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

/// `core.encoding.jsonl.parse` (`EncodingLite.rs`) reports a JSON parse error
/// on line `idx` of the JSONL document at `idx + e.line` — mirrors AOT's
/// `jet_std_jsonl_parse` (`MathRandomTime.rs`), which adds the 0-based JSONL
/// line index to the per-line JSON parser's own line number.
pub(super) fn json_error_value_at_line(e: JsonError, line_offset: i64) -> CtValue {
    json_error_value(JsonError {
        line: line_offset + e.line,
        message: e.message,
    })
}

pub(super) fn render_json_pretty(v: &CtValue, pretty: bool, depth: usize) -> String {
    match v {
        // D-SERDE-ACCESS=B: a `Json`-tagged dynamic value (from `.parse()`, or
        // built by hand with `Json.Text(…)`/`Json.Object(…)`) — unwrap the tag
        // and render its payload the same way the untagged shapes below do.
        CtValue::Enum {
            type_name,
            variant,
            args,
        } if type_name == "Json" => match variant.as_str() {
            "Null" => "null".to_string(),
            _ => match args.first() {
                Some((_, payload)) => render_json_pretty(payload, pretty, depth),
                None => "null".to_string(),
            },
        },
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
