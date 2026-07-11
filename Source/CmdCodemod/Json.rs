use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(u64),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

pub fn parse(input: &str) -> Result<Value, String> {
    let mut p = Parser { bytes: input.as_bytes(), at: 0 };
    let value = p.value()?;
    p.ws();
    if p.at != p.bytes.len() { return Err(format!("unexpected data at byte {}", p.at)); }
    Ok(value)
}

struct Parser<'a> { bytes: &'a [u8], at: usize }
impl Parser<'_> {
    fn ws(&mut self) { while self.at < self.bytes.len() && self.bytes[self.at].is_ascii_whitespace() { self.at += 1; } }
    fn value(&mut self) -> Result<Value, String> {
        self.ws();
        match self.bytes.get(self.at).copied() {
            Some(b'{') => self.object(), Some(b'[') => self.array(), Some(b'"') => self.string().map(Value::String),
            Some(b't') => { self.word(b"true")?; Ok(Value::Bool(true)) },
            Some(b'f') => { self.word(b"false")?; Ok(Value::Bool(false)) },
            Some(b'n') => { self.word(b"null")?; Ok(Value::Null) },
            Some(b'0'..=b'9') => self.number().map(Value::Number),
            _ => Err(format!("expected JSON value at byte {}", self.at)),
        }
    }
    fn word(&mut self, word: &[u8]) -> Result<(), String> {
        if self.bytes.get(self.at..self.at + word.len()) == Some(word) { self.at += word.len(); Ok(()) }
        else { Err(format!("expected JSON value at byte {}", self.at)) }
    }
    fn number(&mut self) -> Result<u64, String> {
        let start = self.at;
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) { self.at += 1; }
        std::str::from_utf8(&self.bytes[start..self.at]).ok().and_then(|s| s.parse().ok())
            .ok_or_else(|| format!("invalid integer at byte {start}"))
    }
    fn string(&mut self) -> Result<String, String> {
        self.at += 1;
        let mut out = String::new();
        while let Some(&b) = self.bytes.get(self.at) {
            self.at += 1;
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let esc = *self.bytes.get(self.at).ok_or("unfinished JSON escape")?; self.at += 1;
                    match esc { b'"' => out.push('"'), b'\\' => out.push('\\'), b'/' => out.push('/'), b'b' => out.push('\u{8}'), b'f' => out.push('\u{c}'), b'n' => out.push('\n'), b'r' => out.push('\r'), b't' => out.push('\t'), b'u' => {
                        let raw = self.bytes.get(self.at..self.at + 4).ok_or("unfinished unicode escape")?;
                        let hex = std::str::from_utf8(raw).map_err(|_| "bad unicode escape")?;
                        let n = u16::from_str_radix(hex, 16).map_err(|_| "bad unicode escape")?; self.at += 4;
                        let c = char::from_u32(n as u32).ok_or("bad unicode scalar")?; out.push(c);
                    }, _ => return Err(format!("unknown JSON escape at byte {}", self.at - 1)) }
                }
                0..=31 => return Err("control byte in JSON string".into()),
                _ if b < 128 => out.push(b as char),
                _ => { self.at -= 1; let rest = std::str::from_utf8(&self.bytes[self.at..]).map_err(|_| "invalid UTF-8")?; let c = rest.chars().next().ok_or("invalid UTF-8")?; out.push(c); self.at += c.len_utf8(); }
            }
        }
        Err("unterminated JSON string".into())
    }
    fn array(&mut self) -> Result<Value, String> {
        self.at += 1; let mut out = Vec::new(); self.ws();
        if self.bytes.get(self.at) == Some(&b']') { self.at += 1; return Ok(Value::Array(out)); }
        loop { out.push(self.value()?); self.ws(); match self.bytes.get(self.at) { Some(b',') => self.at += 1, Some(b']') => { self.at += 1; break; }, _ => return Err(format!("expected `,` or `]` at byte {}", self.at)) } }
        Ok(Value::Array(out))
    }
    fn object(&mut self) -> Result<Value, String> {
        self.at += 1; let mut out = BTreeMap::new(); self.ws();
        if self.bytes.get(self.at) == Some(&b'}') { self.at += 1; return Ok(Value::Object(out)); }
        loop { self.ws(); if self.bytes.get(self.at) != Some(&b'"') { return Err(format!("expected object key at byte {}", self.at)); }
            let key = self.string()?; self.ws(); if self.bytes.get(self.at) != Some(&b':') { return Err(format!("expected `:` at byte {}", self.at)); } self.at += 1;
            if out.insert(key.clone(), self.value()?).is_some() { return Err(format!("duplicate field `{key}`")); }
            self.ws(); match self.bytes.get(self.at) { Some(b',') => self.at += 1, Some(b'}') => { self.at += 1; break; }, _ => return Err(format!("expected `,` or `}}` at byte {}", self.at)) }
        }
        Ok(Value::Object(out))
    }
}

impl Value {
    pub fn object(self) -> Result<BTreeMap<String, Value>, String> { if let Value::Object(v)=self {Ok(v)} else {Err("expected JSON object".into())} }
}
