use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum JSON {
    Null,
    Bool,
    Num(f64),
    Str(String),
    Array,
    Object(BTreeMap<String, JSON>),
}

impl JSON {
    pub(super) fn as_object(&self) -> Result<&BTreeMap<String, JSON>, String> {
        match self {
            Self::Object(value) => Ok(value),
            _ => Err("expected a JSON object".into()),
        }
    }

    pub(super) fn as_str(&self) -> Result<&str, String> {
        match self {
            Self::Str(value) => Ok(value),
            _ => Err("expected a JSON string".into()),
        }
    }
}

pub(super) fn parse(input: &str) -> Result<JSON, String> {
    let mut parser = Parser {
        chars: input.chars().collect(),
        pos: 0,
    };
    parser.skip_ws();
    let value = parser.value()?;
    parser.skip_ws();
    if parser.pos != parser.chars.len() {
        return Err("trailing characters after JSON value".into());
    }
    Ok(value)
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
        let value = self.peek();
        if value.is_some() {
            self.pos += 1;
        }
        value
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(value) if value.is_whitespace()) {
            self.pos += 1;
        }
    }

    fn value(&mut self) -> Result<JSON, String> {
        self.skip_ws();
        match self.peek() {
            Some('{') => self.object(),
            Some('[') => self.array(),
            Some('"') => Ok(JSON::Str(self.string()?)),
            Some('t') | Some('f') => self.boolean(),
            Some('n') => self.null(),
            Some(value) if value == '-' || value.is_ascii_digit() => self.number(),
            Some(value) => Err(format!("unexpected character `{value}`")),
            None => Err("unexpected end of input".into()),
        }
    }

    fn object(&mut self) -> Result<JSON, String> {
        self.bump();
        let mut values = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some('}') {
            self.bump();
            return Ok(JSON::Object(values));
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
            let value = self.value()?;
            if values.insert(key.clone(), value).is_some() {
                return Err(format!("duplicate object key `{key}`"));
            }
            self.skip_ws();
            match self.bump() {
                Some(',') => {}
                Some('}') => break,
                _ => return Err("expected `,` or `}` in object".into()),
            }
        }
        Ok(JSON::Object(values))
    }

    fn array(&mut self) -> Result<JSON, String> {
        self.bump();
        self.skip_ws();
        if self.peek() == Some(']') {
            self.bump();
            return Ok(JSON::Array);
        }
        loop {
            self.value()?;
            self.skip_ws();
            match self.bump() {
                Some(',') => {}
                Some(']') => break,
                _ => return Err("expected `,` or `]` in array".into()),
            }
        }
        Ok(JSON::Array)
    }

    fn string(&mut self) -> Result<String, String> {
        self.bump();
        let mut output = String::new();
        loop {
            match self.bump() {
                Some('"') => return Ok(output),
                Some('\\') => match self.bump() {
                    Some('"') => output.push('"'),
                    Some('\\') => output.push('\\'),
                    Some('/') => output.push('/'),
                    Some('n') => output.push('\n'),
                    Some('t') => output.push('\t'),
                    Some('r') => output.push('\r'),
                    Some('b') => output.push('\u{08}'),
                    Some('f') => output.push('\u{0c}'),
                    Some('u') => output.push(self.unicode_escape()?),
                    _ => return Err("invalid escape in string".into()),
                },
                Some(value) if value <= '\u{1f}' => {
                    return Err("unescaped control character in string".into())
                }
                Some(value) => output.push(value),
                None => return Err("unterminated string".into()),
            }
        }
    }

    fn unicode_escape(&mut self) -> Result<char, String> {
        let codepoint = self.hex4()?;
        if (0xd800..=0xdbff).contains(&codepoint) {
            if self.bump() != Some('\\') || self.bump() != Some('u') {
                return Err("unpaired surrogate in string".into());
            }
            let low = self.hex4()?;
            if !(0xdc00..=0xdfff).contains(&low) {
                return Err("unpaired surrogate in string".into());
            }
            let combined = 0x10000 + ((codepoint - 0xd800) << 10) + (low - 0xdc00);
            char::from_u32(combined).ok_or_else(|| "invalid unicode escape".into())
        } else if (0xdc00..=0xdfff).contains(&codepoint) {
            Err("unpaired surrogate in string".into())
        } else {
            char::from_u32(codepoint).ok_or_else(|| "invalid unicode escape".into())
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let mut codepoint = 0;
        for _ in 0..4 {
            let value = self.bump().ok_or("truncated unicode escape")?;
            codepoint = codepoint * 16 + value.to_digit(16).ok_or("invalid unicode escape")?;
        }
        Ok(codepoint)
    }

    fn number(&mut self) -> Result<JSON, String> {
        let start = self.pos;
        while matches!(self.peek(), Some(value) if value.is_ascii_digit() || "-+.eE".contains(value))
        {
            self.pos += 1;
        }
        let value: String = self.chars[start..self.pos].iter().collect();
        value
            .parse::<f64>()
            .map(JSON::Num)
            .map_err(|_| format!("invalid number `{value}`"))
    }

    fn boolean(&mut self) -> Result<JSON, String> {
        if self.literal("true") || self.literal("false") {
            Ok(JSON::Bool)
        } else {
            Err("invalid literal".into())
        }
    }

    fn null(&mut self) -> Result<JSON, String> {
        if self.literal("null") {
            Ok(JSON::Null)
        } else {
            Err("invalid literal".into())
        }
    }

    fn literal(&mut self, expected: &str) -> bool {
        let end = self.pos + expected.len();
        if end <= self.chars.len()
            && self.chars[self.pos..end].iter().collect::<String>() == expected
        {
            self.pos = end;
            true
        } else {
            false
        }
    }
}
