    // JSON/DataTree bridges layered on the canonical JSONCodec.rs core.
    pub fn io_error_at(operation: IOOperation, path: &str, e: std::io::Error) -> IOError {
        let context = IOContext::new(operation, Some(path.to_string()), e.raw_os_error().map(i64::from), Some(e.to_string()));
        match e.kind() {
            std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => IOError::InvalidInput(context),
            std::io::ErrorKind::NotFound => IOError::NotFound(context),
            std::io::ErrorKind::PermissionDenied => IOError::PermissionDenied(context),
            std::io::ErrorKind::TimedOut => IOError::TimedOut(context),
            std::io::ErrorKind::WouldBlock => IOError::Other(context),
            std::io::ErrorKind::NotConnected | std::io::ErrorKind::BrokenPipe => IOError::Closed(context),
            _ => IOError::Other(context),
        }
    }

    // Render a DataTree as JSON, preserving Object field order. Int prints with no
    // decimal (`5`), Float keeps its decimal (`5.0`); Bytes render as a number array.
    pub fn render_datatree_json(t: &DataTree, pretty: bool, depth: usize) -> String {
        match t {
            DataTree::Null => "null".to_string(),
            DataTree::Bool(b) => b.to_string(),
            DataTree::Int(n) => format!("{}", n),
            DataTree::Float(f) => format!("{:?}", f),
            DataTree::Text(s) => quote_json(s),
            DataTree::Bytes(bs) => {
                let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                format!("[{}]", parts.join(","))
            }
            DataTree::Array(items) => {
                if items.is_empty() {
                    return "[]".to_string();
                }
                if !pretty {
                    let parts: Vec<String> = items
                        .iter()
                        .map(|x| render_datatree_json(x, false, depth))
                        .collect();
                    return format!("[{}]", parts.join(","));
                }
                let pad = "  ".repeat(depth + 1);
                let end = "  ".repeat(depth);
                let parts: Vec<String> = items
                    .iter()
                    .map(|x| format!("{}{}", pad, render_datatree_json(x, true, depth + 1)))
                    .collect();
                format!("[\n{}\n{}]", parts.join(",\n"), end)
            }
            DataTree::Object(entries) => {
                if entries.is_empty() {
                    return "{}".to_string();
                }
                if !pretty {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(k, v)| {
                            format!(
                                "{}:{}",
                                quote_json(k),
                                render_datatree_json(v, false, depth)
                            )
                        })
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
                            render_datatree_json(v, true, depth + 1)
                        )
                    })
                    .collect();
                format!("{{\n{}\n{}}}", parts.join(",\n"), end)
            }
        }
    }

    // JSON (dynamic, BTreeMap-keyed) → DataTree. Numbers that are integral collapse
    // to `Int`, so a round-trip through JSON keeps `5` an Int.
    pub fn datatree_from_json(j: &JSON) -> DataTree {
        match j {
            JSON::Null => DataTree::Null,
            JSON::Boolean(b) => DataTree::Bool(*b),
            JSON::Integer(n) => DataTree::Int(*n),
            JSON::Number(n) => {
                if n.fract() == 0.0
                    && n.is_finite()
                    && *n >= i64::MIN as f64
                    && *n <= i64::MAX as f64
                {
                    DataTree::Int(*n as i64)
                } else {
                    DataTree::Float(*n)
                }
            }
            JSON::Text(s) => DataTree::Text(s.clone()),
            JSON::Array(items) => DataTree::Array(items.iter().map(datatree_from_json).collect()),
            JSON::Object(m) => DataTree::Object(
                m.iter()
                    .map(|(k, v)| (k.clone(), datatree_from_json(v)))
                    .collect(),
            ),
        }
    }

    /// Parse typed wire data directly into the ordered tree. Dynamic `JSON`
    /// remains BTreeMap-backed; typed codecs need the wire order so a
    /// `#PublishedSchema` can retain unknown fields without a second parser
    /// or a host-side policy.
    pub fn parse_json_datatree(text: &str) -> Result<DataTree, JSONError> {
        let mut parser = DataTreeParser {
            chars: text.chars().collect(),
            pos: 0,
        };
        let value = parser.value()?;
        parser.ws();
        if parser.pos != parser.chars.len() {
            return Err(parser.err("extra text after JSON value"));
        }
        Ok(value)
    }

    struct DataTreeParser {
        chars: Vec<char>,
        pos: usize,
    }

    impl DataTreeParser {
        fn err(&self, message: &str) -> JSONError {
            let line = self.chars[..self.pos.min(self.chars.len())]
                .iter()
                .filter(|c| **c == '\n')
                .count() as i64
                + 1;
            JSONError {
                line,
                message: message.to_string(),
            }
        }

        fn peek(&self) -> Option<char> {
            self.chars.get(self.pos).copied()
        }

        fn ws(&mut self) {
            while self.pos < self.chars.len()
                && is_json_structural_whitespace(self.chars[self.pos])
            {
                self.pos += 1;
            }
        }

        fn value(&mut self) -> Result<DataTree, JSONError> {
            self.ws();
            match self.peek() {
                Some('n') => self.word("null", DataTree::Null),
                Some('t') => self.word("true", DataTree::Bool(true)),
                Some('f') => self.word("false", DataTree::Bool(false)),
                Some('"') => Ok(DataTree::Text(self.string()?)),
                Some('[') => self.array(),
                Some('{') => self.object(),
                Some('-') | Some('0'..='9') => self.number(),
                _ => Err(self.err("expected a JSON value")),
            }
        }

        fn word(&mut self, word: &str, value: DataTree) -> Result<DataTree, JSONError> {
            for ch in word.chars() {
                if self.peek() != Some(ch) {
                    return Err(self.err("expected a JSON word"));
                }
                self.pos += 1;
            }
            Ok(value)
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
                        let Some(escape) = self.peek() else {
                            return Err(self.err("unfinished escape"));
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
                            _ => return Err(self.err("invalid escape in string")),
                        }
                    }
                    c if (c as u32) < 0x20 => {
                        return Err(self.err("control character in string"));
                    }
                    other => out.push(other),
                }
            }
            Err(self.err("missing closing quote"))
        }

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
                let low = self.hex4()?;
                if !(0xDC00..=0xDFFF).contains(&low) {
                    return Err(self.err("unpaired surrogate in string"));
                }
                let combined = 0x10000 + ((cp - 0xD800) << 10) + (low - 0xDC00);
                let Some(ch) = char::from_u32(combined) else {
                    return Err(self.err("invalid unicode escape"));
                };
                out.push(ch);
            } else if (0xDC00..=0xDFFF).contains(&cp) {
                return Err(self.err("unpaired surrogate in string"));
            } else {
                let Some(ch) = char::from_u32(cp) else {
                    return Err(self.err("invalid unicode escape"));
                };
                out.push(ch);
            }
            Ok(())
        }

        fn hex4(&mut self) -> Result<u32, JSONError> {
            let mut value = 0u32;
            for _ in 0..4 {
                let Some(ch) = self.peek() else {
                    return Err(self.err("truncated unicode escape"));
                };
                let digit = ch
                    .to_digit(16)
                    .ok_or_else(|| self.err("invalid unicode escape"))?;
                value = value * 16 + digit;
                self.pos += 1;
            }
            Ok(value)
        }

        fn number(&mut self) -> Result<DataTree, JSONError> {
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
            let text: String = self.chars[start..self.pos].iter().collect();
            if !text.contains('.') && !text.contains('e') && !text.contains('E') {
                if let Ok(value) = text.parse::<i64>() {
                    return Ok(DataTree::Int(value));
                }
            }
            text.parse::<f64>()
                .map(DataTree::Float)
                .map_err(|_| self.err("bad number"))
        }

        fn array(&mut self) -> Result<DataTree, JSONError> {
            self.pos += 1;
            let mut values = Vec::new();
            loop {
                self.ws();
                if self.peek() == Some(']') {
                    self.pos += 1;
                    return Ok(DataTree::Array(values));
                }
                values.push(self.value()?);
                self.ws();
                match self.peek() {
                    Some(',') => self.pos += 1,
                    Some(']') => {}
                    _ => return Err(self.err("expected `,` or `]`")),
                }
            }
        }

        fn object(&mut self) -> Result<DataTree, JSONError> {
            self.pos += 1;
            let mut fields = Vec::new();
            loop {
                self.ws();
                if self.peek() == Some('}') {
                    self.pos += 1;
                    return Ok(DataTree::Object(fields));
                }
                let key = self.string()?;
                self.ws();
                if self.peek() != Some(':') {
                    return Err(self.err("expected `:` after object key"));
                }
                self.pos += 1;
                let value = self.value()?;
                if let Some((_, current)) = fields.iter_mut().find(|(field, _)| field == &key) {
                    *current = value;
                } else {
                    fields.push((key, value));
                }
                self.ws();
                match self.peek() {
                    Some(',') => self.pos += 1,
                    Some('}') => {}
                    _ => return Err(self.err("expected `,` or `}`")),
                }
            }
        }
    }

    // A short kind name for decode error messages.
    pub fn datatree_kind(t: &DataTree) -> &'static str {
        match t {
            DataTree::Null => "null",
            DataTree::Bool(_) => "Bool",
            DataTree::Int(_) => "Int",
            DataTree::Float(_) => "Float",
            DataTree::Text(_) => "Text",
            DataTree::Bytes(_) => "Bytes",
            DataTree::Array(_) => "a list",
            DataTree::Object(_) => "an object",
        }
    }

    // Look up a key in an ordered Object.
    pub fn datatree_get<'a>(t: &'a DataTree, key: &str) -> Option<&'a DataTree> {
        match t {
            DataTree::Object(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
