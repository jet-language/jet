    // Canonical std-only JSON codec shared by AOT, JIT encoding, and tier 0.
    pub fn parse_json(text: &str) -> Result<JSON, JSONError> {
        parse_json_with(text, false)
    }

    pub fn parse_json_strict(text: &str) -> Result<JSON, JSONError> {
        parse_json_with(text, true)
    }

    fn parse_json_with(text: &str, reject_duplicate_keys: bool) -> Result<JSON, JSONError> {
        let mut p = JSONParser {
            chars: text.chars().collect(),
            pos: 0,
            reject_duplicate_keys,
        };
        let v = p.value()?;
        p.ws();
        if p.pos != p.chars.len() {
            return Err(p.err("extra text after JSON value"));
        }
        Ok(v)
    }

    pub fn render_json(j: &JSON, pretty: bool, depth: usize) -> String {
        match j {
            JSON::Null => "null".to_string(),
            JSON::Boolean(b) => b.to_string(),
            JSON::Integer(n) => n.to_string(),
            JSON::Number(n) => format!("{:?}", n),
            JSON::Text(s) => quote_json(s),
            JSON::Array(items) => {
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
            JSON::Object(entries) => {
                if entries.is_empty() {
                    return "{}".to_string();
                }
                if !pretty {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(k, v)| format!("{}:{}", quote_json(k), render_json(v, false, depth)))
                        .collect();
                    return format!("{{{}}}", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = entries
                    .iter()
                    .map(|(k, v)| {
                        format!(
                            "{}{}: {}",
                            pad,
                            quote_json(k),
                            render_json(v, true, depth + 1)
                        )
                    })
                    .collect();
                format!("{{\n{}\n{}}}", parts.join(",\n"), end)
            }
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

    struct JSONParser {
        chars: Vec<char>,
        pos: usize,
        reject_duplicate_keys: bool,
    }

    impl JSONParser {
        fn err(&self, msg: &str) -> JSONError {
            let line = self.chars[..self.pos.min(self.chars.len())]
                .iter()
                .filter(|c| **c == '\n')
                .count() as i64
                + 1;
            JSONError {
                line,
                message: msg.to_string(),
            }
        }

        fn ws(&mut self) {
            while self.pos < self.chars.len() && self.chars[self.pos].is_whitespace() {
                self.pos += 1;
            }
        }

        fn value(&mut self) -> Result<JSON, JSONError> {
            self.ws();
            match self.peek() {
                Some('n') => self.word("null", JSON::Null),
                Some('t') => self.word("true", JSON::Boolean(true)),
                Some('f') => self.word("false", JSON::Boolean(false)),
                Some('"') => Ok(JSON::Text(self.string()?)),
                Some('[') => self.array(),
                Some('{') => self.object(),
                Some('-') | Some('0'..='9') => self.number(),
                _ => Err(self.err("expected a JSON value")),
            }
        }

        fn peek(&self) -> Option<char> {
            self.chars.get(self.pos).copied()
        }

        fn word(&mut self, w: &str, v: JSON) -> Result<JSON, JSONError> {
            for ch in w.chars() {
                if self.peek() != Some(ch) {
                    return Err(self.err("expected a JSON word"));
                }
                self.pos += 1;
            }
            Ok(v)
        }

        fn string(&mut self) -> Result<String, JSONError> {
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

        // A `\uXXXX` escape, already past the `u`. Combines a high+low surrogate
        // pair into one code point; rejects a lone or malformed surrogate.
        fn unicode_escape(&mut self, out: &mut String) -> Result<(), JSONError> {
            let cp = self.hex4()?;
            if (0xD800..=0xDBFF).contains(&cp) {
                if self.peek() != Some('\\') {
                    return Err(self.err("unpaired surrogate in string"));
                }
                self.pos += 1;
                if self.peek() != Some('u') {
                    return Err(self.err("unpaired surrogate in string"));
                }
                self.pos += 1;
                let lo = self.hex4()?;
                if !(0xDC00..=0xDFFF).contains(&lo) {
                    return Err(self.err("unpaired surrogate in string"));
                }
                let combined = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                match char::from_u32(combined) {
                    Some(ch) => out.push(ch),
                    None => return Err(self.err("invalid unicode escape")),
                }
            } else if (0xDC00..=0xDFFF).contains(&cp) {
                return Err(self.err("unpaired surrogate in string"));
            } else {
                match char::from_u32(cp) {
                    Some(ch) => out.push(ch),
                    None => return Err(self.err("invalid unicode escape")),
                }
            }
            Ok(())
        }

        fn hex4(&mut self) -> Result<u32, JSONError> {
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

        fn number(&mut self) -> Result<JSON, JSONError> {
            let start = self.pos;
            if self.peek() == Some('-') {
                self.pos += 1;
            }
            // Integer part: `0` alone, or a non-zero digit then more digits.
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
            // Fraction: a `.` must be followed by at least one digit.
            if self.peek() == Some('.') {
                self.pos += 1;
                if !matches!(self.peek(), Some('0'..='9')) {
                    return Err(self.err("bad number"));
                }
                while matches!(self.peek(), Some('0'..='9')) {
                    self.pos += 1;
                }
            }
            // Exponent: `e`/`E`, optional sign, at least one digit.
            if matches!(self.peek(), Some('e') | Some('E')) {
                self.pos += 1;
                if matches!(self.peek(), Some('+') | Some('-')) {
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
            if !s.contains('.') && !s.contains('e') && !s.contains('E') {
                if let Ok(n) = s.parse::<i64>() {
                    return Ok(JSON::Integer(n));
                }
            }
            match s.parse::<f64>() {
                Ok(n) => Ok(JSON::Number(n)),
                Err(_) => Err(self.err("bad number")),
            }
        }

        fn array(&mut self) -> Result<JSON, JSONError> {
            self.pos += 1;
            let mut out = Vec::new();
            loop {
                self.ws();
                if self.peek() == Some(']') {
                    self.pos += 1;
                    return Ok(JSON::Array(out));
                }
                out.push(self.value()?);
                self.ws();
                match self.peek() {
                    Some(',') => self.pos += 1,
                    Some(']') => {}
                    _ => return Err(self.err("expected `,` or `]`")),
                }
            }
        }

        fn object(&mut self) -> Result<JSON, JSONError> {
            self.pos += 1;
            let mut out = std::collections::BTreeMap::new();
            loop {
                self.ws();
                if self.peek() == Some('}') {
                    self.pos += 1;
                    return Ok(JSON::Object(out));
                }
                let key = self.string()?;
                self.ws();
                if self.peek() != Some(':') {
                    return Err(self.err("expected `:` after object key"));
                }
                self.pos += 1;
                let value = self.value()?;
                if self.reject_duplicate_keys && out.contains_key(&key) {
                    return Err(self.err("duplicate JSON object key"));
                }
                out.insert(key, value);
                self.ws();
                match self.peek() {
                    Some(',') => self.pos += 1,
                    Some('}') => {}
                    _ => return Err(self.err("expected `,` or `}`")),
                }
            }
        }
    }
