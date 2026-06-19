//! A tiny std-only JSON reader (I6: no external crates).
//!
//! Only as much as Jetpack needs to read `nix build --json` output and write
//! its own small state files. Not a general-purpose library — it parses into a
//! `Json` tree and offers typed accessors that return clear errors, the same
//! shape Forge's `nixbridge` used (see forge-salvage.md).

use std::collections::BTreeMap;
use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Array(Vec<Json>),
    Object(BTreeMap<String, Json>),
}

impl Json {
    pub fn as_array(&self) -> Result<&Vec<Json>, String> {
        match self {
            Json::Array(v) => Ok(v),
            _ => Err("expected a JSON array".into()),
        }
    }
    pub fn as_object(&self) -> Result<&BTreeMap<String, Json>, String> {
        match self {
            Json::Object(m) => Ok(m),
            _ => Err("expected a JSON object".into()),
        }
    }
    pub fn as_str(&self) -> Result<&str, String> {
        match self {
            Json::Str(s) => Ok(s),
            _ => Err("expected a JSON string".into()),
        }
    }
    /// Look up a key in an object, erroring if absent.
    pub fn get<'a>(&'a self, key: &str) -> Result<&'a Json, String> {
        self.as_object()?
            .get(key)
            .ok_or_else(|| format!("missing key `{key}`"))
    }
}

// ── parsing ──────────────────────────────────────────────────────────────

pub fn parse(input: &str) -> Result<Json, String> {
    let mut p = Parser {
        chars: input.chars().collect(),
        pos: 0,
    };
    p.skip_ws();
    let v = p.value()?;
    p.skip_ws();
    if p.pos != p.chars.len() {
        return Err("trailing characters after JSON value".into());
    }
    Ok(v)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek();
        if c.is_some() {
            self.pos += 1;
        }
        c
    }
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.skip_ws();
        match self.peek() {
            Some('{') => self.object(),
            Some('[') => self.array(),
            Some('"') => Ok(Json::Str(self.string()?)),
            Some('t') | Some('f') => self.boolean(),
            Some('n') => self.null(),
            Some(c) if c == '-' || c.is_ascii_digit() => self.number(),
            Some(c) => Err(format!("unexpected character `{c}`")),
            None => Err("unexpected end of input".into()),
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.bump(); // {
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some('}') {
            self.bump();
            return Ok(Json::Object(map));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some('"') {
                return Err("expected a string key in object".into());
            }
            let key = self.string()?;
            self.skip_ws();
            if self.bump() != Some(':') {
                return Err("expected `:` after object key".into());
            }
            let val = self.value()?;
            map.insert(key, val);
            self.skip_ws();
            match self.bump() {
                Some(',') => continue,
                Some('}') => break,
                _ => return Err("expected `,` or `}` in object".into()),
            }
        }
        Ok(Json::Object(map))
    }

    fn array(&mut self) -> Result<Json, String> {
        self.bump(); // [
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(']') {
            self.bump();
            return Ok(Json::Array(items));
        }
        loop {
            let val = self.value()?;
            items.push(val);
            self.skip_ws();
            match self.bump() {
                Some(',') => continue,
                Some(']') => break,
                _ => return Err("expected `,` or `]` in array".into()),
            }
        }
        Ok(Json::Array(items))
    }

    fn string(&mut self) -> Result<String, String> {
        self.bump(); // opening "
        let mut out = String::new();
        loop {
            match self.bump() {
                Some('"') => return Ok(out),
                Some('\\') => match self.bump() {
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some('/') => out.push('/'),
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('b') => out.push('\u{08}'),
                    Some('f') => out.push('\u{0C}'),
                    Some('u') => {
                        let mut code = 0u32;
                        for _ in 0..4 {
                            let c = self.bump().ok_or("truncated \\u escape")?;
                            let d = c.to_digit(16).ok_or("invalid \\u escape")?;
                            code = code * 16 + d;
                        }
                        out.push(char::from_u32(code).unwrap_or('\u{FFFD}'));
                    }
                    _ => return Err("invalid escape in string".into()),
                },
                Some(c) => out.push(c),
                None => return Err("unterminated string".into()),
            }
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        while matches!(self.peek(), Some(c) if c.is_ascii_digit() || "-+.eE".contains(c)) {
            self.pos += 1;
        }
        let slice: String = self.chars[start..self.pos].iter().collect();
        slice
            .parse::<f64>()
            .map(Json::Num)
            .map_err(|_| format!("invalid number `{slice}`"))
    }

    fn boolean(&mut self) -> Result<Json, String> {
        if self.literal("true") {
            Ok(Json::Bool(true))
        } else if self.literal("false") {
            Ok(Json::Bool(false))
        } else {
            Err("invalid literal".into())
        }
    }

    fn null(&mut self) -> Result<Json, String> {
        if self.literal("null") {
            Ok(Json::Null)
        } else {
            Err("invalid literal".into())
        }
    }

    fn literal(&mut self, word: &str) -> bool {
        let end = self.pos + word.len();
        if end <= self.chars.len() && self.chars[self.pos..end].iter().collect::<String>() == word {
            self.pos = end;
            true
        } else {
            false
        }
    }
}

// ── writing (for Jetpack's own small state files) ────────────────────────

/// Serialize a string as a JSON string literal (quotes + escapes).
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Pretty-print a flat string→string object as JSON (used for store metadata).
pub fn object_of(pairs: &[(&str, &str)]) -> String {
    let mut out = String::from("{\n");
    for (i, (k, v)) in pairs.iter().enumerate() {
        let comma = if i + 1 < pairs.len() { "," } else { "" };
        let _ = writeln!(out, "  {}: {}{}", quote(k), quote(v), comma);
    }
    out.push('}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nix_build_shape() {
        let input =
            r#"[{"drvPath":"/nix/store/x.drv","outputs":{"out":"/nix/store/abc-fastfetch-2.0"}}]"#;
        let j = parse(input).unwrap();
        let first = &j.as_array().unwrap()[0];
        let out = first.get("outputs").unwrap().get("out").unwrap();
        assert_eq!(out.as_str().unwrap(), "/nix/store/abc-fastfetch-2.0");
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("not json").is_err());
        assert!(parse("").is_err());
        assert!(parse("[1,2").is_err());
    }

    #[test]
    fn roundtrips_quote() {
        assert_eq!(quote("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }
}
