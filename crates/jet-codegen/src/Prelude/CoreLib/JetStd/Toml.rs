    // ── core.encoding.toml: full TOML 1.0 → DataTree (D-ENC-DYN1=A+, c152) ────────
    // A complete std-only TOML 1.0 parser (ported from the compiler's
    // Source/Jetpack/TOML.rs, which the emitted prelude cannot reach) that lowers a
    // document onto the one rich `DataTree`. Strings (every escape + multi-line),
    // integers in every base, floats incl. inf/nan, booleans, datetimes (kept raw),
    // arrays, inline tables, dotted keys, `[table]` headers, and `[[array-of-tables]]`.
    pub mod toml {
        use super::DataTree;

        #[derive(Clone, Debug, PartialEq)]
        pub enum Value {
            String(String),
            Integer(i64),
            Float(f64),
            Boolean(bool),
            Datetime(String),
            Array(Vec<Value>),
            InlineTable(Vec<(String, Value)>),
        }

        #[derive(Clone, Debug, PartialEq)]
        pub enum Item {
            Header { path: Vec<String>, array: bool },
            KeyVal { path: Vec<String>, value: Value },
        }

        #[derive(Clone, Debug, PartialEq, Eq)]
        pub struct ParseError {
            pub line: usize,
            pub message: String,
        }

        pub fn parse_to_tree(raw: &str) -> Result<DataTree, ParseError> {
            let mut p = Parser {
                chars: raw.chars().collect(),
                pos: 0,
                line: 1,
            };
            let mut items = Vec::new();
            loop {
                p.skip_between_statements();
                if p.peek().is_none() {
                    break;
                }
                match p.statement()? {
                    Some(item) => items.push(item),
                    None => {}
                }
            }
            Ok(assemble(items))
        }

        // ── Assembly: a flat Item list → a nested ordered `DataTree::Object` ──────
        fn assemble(items: Vec<Item>) -> DataTree {
            let mut root = DataTree::Object(Vec::new());
            let mut current: Vec<String> = Vec::new();
            for item in items {
                match item {
                    Item::Header { path, array } => {
                        if array {
                            push_array_table(&mut root, &path);
                        } else {
                            // Ensure the table exists.
                            let _ = table_at(&mut root, &path);
                        }
                        current = path;
                    }
                    Item::KeyVal { path, value } => {
                        set_key(&mut root, &current, &path, value_to_tree(value));
                    }
                }
            }
            root
        }

        // Navigate to (creating along the way) the table at `path`. When a segment is
        // an array-of-tables, descend into its LAST element.
        fn table_at<'a>(mut node: &'a mut DataTree, path: &[String]) -> &'a mut DataTree {
            for seg in path {
                node = child_table_mut(node, seg);
            }
            node
        }

        fn child_table_mut<'a>(node: &'a mut DataTree, seg: &str) -> &'a mut DataTree {
            let entries = match node {
                DataTree::Object(entries) => entries,
                other => return other,
            };
            let idx = match entries.iter().position(|(k, _)| k == seg) {
                Some(i) => i,
                None => {
                    entries.push((seg.to_string(), DataTree::Object(Vec::new())));
                    entries.len() - 1
                }
            };
            // An existing array-of-tables: descend into its last element. Decide the
            // target index immutably first, then take exactly one mutable borrow per
            // branch of a match on the (non-borrowing) `Option` — sidesteps the NLL snag.
            let arr_last: Option<usize> = match &entries[idx].1 {
                DataTree::Array(arr) if !arr.is_empty() => Some(arr.len() - 1),
                _ => None,
            };
            match arr_last {
                Some(n) => match &mut entries[idx].1 {
                    DataTree::Array(arr) => &mut arr[n],
                    other => other,
                },
                None => &mut entries[idx].1,
            }
        }

        fn push_array_table(root: &mut DataTree, path: &[String]) {
            let (parent_path, last) = path.split_at(path.len() - 1);
            let parent = table_at(root, parent_path);
            if let DataTree::Object(entries) = parent {
                let idx = match entries.iter().position(|(k, _)| k == &last[0]) {
                    Some(i) => i,
                    None => {
                        entries.push((last[0].clone(), DataTree::Array(Vec::new())));
                        entries.len() - 1
                    }
                };
                if let DataTree::Array(arr) = &mut entries[idx].1 {
                    arr.push(DataTree::Object(Vec::new()));
                }
            }
        }

        fn set_key(root: &mut DataTree, current: &[String], key_path: &[String], value: DataTree) {
            let mut full: Vec<String> = current.to_vec();
            full.extend_from_slice(&key_path[..key_path.len() - 1]);
            let table = table_at(root, &full);
            let fk = &key_path[key_path.len() - 1];
            if let DataTree::Object(entries) = table {
                if let Some(slot) = entries.iter_mut().find(|(k, _)| k == fk) {
                    slot.1 = value;
                } else {
                    entries.push((fk.clone(), value));
                }
            }
        }

        fn value_to_tree(v: Value) -> DataTree {
            match v {
                Value::String(s) => DataTree::Text(s),
                Value::Integer(n) => DataTree::Int(n),
                Value::Float(f) => DataTree::Float(f),
                Value::Boolean(b) => DataTree::Bool(b),
                Value::Datetime(s) => DataTree::Text(s),
                Value::Array(xs) => DataTree::Array(xs.into_iter().map(value_to_tree).collect()),
                Value::InlineTable(es) => {
                    DataTree::Object(es.into_iter().map(|(k, v)| (k, value_to_tree(v))).collect())
                }
            }
        }

        // ── Render: a `DataTree` → TOML text (nested headers, arrays-of-tables) ───
        pub fn render(t: &DataTree) -> String {
            let mut out = String::new();
            render_table(t, &[], &mut out);
            out.trim_end().to_string()
        }

        fn is_table(v: &DataTree) -> bool {
            matches!(v, DataTree::Object(_))
        }
        fn is_array_of_tables(v: &DataTree) -> bool {
            matches!(v, DataTree::Array(arr)
                if !arr.is_empty() && arr.iter().all(|e| matches!(e, DataTree::Object(_))))
        }

        fn render_table(t: &DataTree, path: &[String], out: &mut String) {
            if let DataTree::Object(entries) = t {
                for (k, v) in entries {
                    if !is_table(v) && !is_array_of_tables(v) {
                        out.push_str(&format!("{} = {}\n", k, render_value(v)));
                    }
                }
                for (k, v) in entries {
                    if is_table(v) {
                        let mut p = path.to_vec();
                        p.push(k.clone());
                        out.push_str(&format!("\n[{}]\n", p.join(".")));
                        render_table(v, &p, out);
                    } else if is_array_of_tables(v) {
                        let mut p = path.to_vec();
                        p.push(k.clone());
                        if let DataTree::Array(arr) = v {
                            for elem in arr {
                                out.push_str(&format!("\n[[{}]]\n", p.join(".")));
                                render_table(elem, &p, out);
                            }
                        }
                    }
                }
            }
        }

        fn render_value(v: &DataTree) -> String {
            match v {
                DataTree::Null => "\"\"".to_string(),
                DataTree::Bool(b) => b.to_string(),
                DataTree::Int(n) => n.to_string(),
                DataTree::Float(f) => format!("{:?}", f),
                DataTree::Text(s) => super::quote_json(s),
                DataTree::Bytes(bs) => {
                    let parts: Vec<String> = bs.iter().map(|b| b.to_string()).collect();
                    format!("[{}]", parts.join(", "))
                }
                DataTree::Array(items) => {
                    let parts: Vec<String> = items.iter().map(render_value).collect();
                    format!("[{}]", parts.join(", "))
                }
                // An inline (non-header) object renders as a TOML inline table.
                DataTree::Object(entries) => {
                    let parts: Vec<String> = entries
                        .iter()
                        .map(|(k, val)| format!("{} = {}", k, render_value(val)))
                        .collect();
                    format!("{{ {} }}", parts.join(", "))
                }
            }
        }

        struct Parser {
            chars: Vec<char>,
            pos: usize,
            line: usize,
        }

        impl Parser {
            fn peek(&self) -> Option<char> {
                self.chars.get(self.pos).copied()
            }
            fn peek_at(&self, n: usize) -> Option<char> {
                self.chars.get(self.pos + n).copied()
            }
            fn bump(&mut self) -> Option<char> {
                let c = self.peek()?;
                self.pos += 1;
                if c == '\n' {
                    self.line += 1;
                }
                Some(c)
            }
            fn err(&self, message: impl Into<String>) -> ParseError {
                ParseError {
                    line: self.line,
                    message: message.into(),
                }
            }
            fn skip_inline_ws(&mut self) {
                while matches!(self.peek(), Some(' ' | '\t')) {
                    self.pos += 1;
                }
            }
            fn skip_between_statements(&mut self) {
                loop {
                    match self.peek() {
                        Some(' ' | '\t' | '\r' | '\n') => {
                            self.bump();
                        }
                        Some('#') => self.skip_comment(),
                        _ => break,
                    }
                }
            }
            fn skip_comment(&mut self) {
                while let Some(c) = self.peek() {
                    if c == '\n' {
                        break;
                    }
                    self.pos += 1;
                }
            }
            fn finish_line(&mut self) -> Result<(), ParseError> {
                self.skip_inline_ws();
                if self.peek() == Some('#') {
                    self.skip_comment();
                }
                match self.peek() {
                    None | Some('\n') | Some('\r') => Ok(()),
                    Some(c) => Err(self.err(format!("unexpected `{c}` after value"))),
                }
            }
            fn statement(&mut self) -> Result<Option<Item>, ParseError> {
                match self.peek() {
                    Some('[') => self.header().map(Some),
                    _ => self.key_value().map(Some),
                }
            }
            fn header(&mut self) -> Result<Item, ParseError> {
                self.bump(); // first '['
                let array = self.peek() == Some('[');
                if array {
                    self.bump();
                }
                self.skip_inline_ws();
                let path = self.key_path()?;
                self.skip_inline_ws();
                if self.peek() != Some(']') {
                    return Err(self.err("expected `]` to close a table header"));
                }
                self.bump();
                if array {
                    if self.peek() != Some(']') {
                        return Err(self.err("expected `]]` to close an array-of-tables header"));
                    }
                    self.bump();
                }
                if path.is_empty() {
                    return Err(self.err("a table header must name a table"));
                }
                self.finish_line()?;
                Ok(Item::Header { path, array })
            }
            fn key_value(&mut self) -> Result<Item, ParseError> {
                let path = self.key_path()?;
                if path.is_empty() {
                    return Err(self.err("expected a key"));
                }
                self.skip_inline_ws();
                if self.peek() != Some('=') {
                    return Err(self.err(format!("expected `=` after key `{}`", path.join("."))));
                }
                self.bump();
                self.skip_inline_ws();
                let value = self.value()?;
                self.finish_line()?;
                Ok(Item::KeyVal { path, value })
            }
            fn key_path(&mut self) -> Result<Vec<String>, ParseError> {
                let mut path = Vec::new();
                loop {
                    self.skip_inline_ws();
                    path.push(self.simple_key()?);
                    self.skip_inline_ws();
                    if self.peek() == Some('.') {
                        self.bump();
                    } else {
                        break;
                    }
                }
                Ok(path)
            }
            fn simple_key(&mut self) -> Result<String, ParseError> {
                match self.peek() {
                    Some('"') => self.basic_string(),
                    Some('\'') => self.literal_string(),
                    Some(c) if is_bare_key_char(c) => {
                        let mut s = String::new();
                        while let Some(c) = self.peek() {
                            if is_bare_key_char(c) {
                                s.push(c);
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                        Ok(s)
                    }
                    Some(c) => Err(self.err(format!("`{c}` is not a valid key character"))),
                    None => Err(self.err("expected a key")),
                }
            }
            fn value(&mut self) -> Result<Value, ParseError> {
                match self.peek() {
                    Some('"') => Ok(Value::String(self.basic_string()?)),
                    Some('\'') => Ok(Value::String(self.literal_string()?)),
                    Some('[') => self.array(),
                    Some('{') => self.inline_table(),
                    Some('t') | Some('f') => self.boolean(),
                    Some('+') | Some('-') | Some('0'..='9') | Some('i') | Some('n') => {
                        self.number_or_datetime()
                    }
                    Some(c) => Err(self.err(format!("`{c}` does not start a valid value"))),
                    None => Err(self.err("expected a value")),
                }
            }
            fn boolean(&mut self) -> Result<Value, ParseError> {
                if self.try_keyword("true") {
                    Ok(Value::Boolean(true))
                } else if self.try_keyword("false") {
                    Ok(Value::Boolean(false))
                } else {
                    Err(self.err("expected `true` or `false`"))
                }
            }
            fn try_keyword(&mut self, kw: &str) -> bool {
                let chars: Vec<char> = kw.chars().collect();
                for (i, c) in chars.iter().enumerate() {
                    if self.peek_at(i) != Some(*c) {
                        return false;
                    }
                }
                if let Some(after) = self.peek_at(chars.len()) {
                    if is_bare_key_char(after) || after == '.' {
                        return false;
                    }
                }
                for _ in 0..chars.len() {
                    self.bump();
                }
                true
            }
            fn basic_string(&mut self) -> Result<String, ParseError> {
                if self.peek() == Some('"')
                    && self.peek_at(1) == Some('"')
                    && self.peek_at(2) == Some('"')
                {
                    return self.multiline_basic_string();
                }
                self.bump();
                let mut out = String::new();
                loop {
                    match self.bump() {
                        None | Some('\n') => return Err(self.err("unterminated string")),
                        Some('"') => return Ok(out),
                        Some('\\') => out.push(self.string_escape()?),
                        Some(c) if (c as u32) < 0x20 => {
                            return Err(self.err("control character in string"))
                        }
                        Some(c) => out.push(c),
                    }
                }
            }
            fn multiline_basic_string(&mut self) -> Result<String, ParseError> {
                self.bump();
                self.bump();
                self.bump();
                if self.peek() == Some('\r') {
                    self.bump();
                }
                if self.peek() == Some('\n') {
                    self.bump();
                }
                let mut out = String::new();
                loop {
                    if self.peek() == Some('"')
                        && self.peek_at(1) == Some('"')
                        && self.peek_at(2) == Some('"')
                    {
                        self.bump();
                        self.bump();
                        self.bump();
                        return Ok(out);
                    }
                    match self.bump() {
                        None => return Err(self.err("unterminated multi-line string")),
                        Some('\\') => {
                            if matches!(
                                self.peek(),
                                Some('\n') | Some('\r') | Some(' ') | Some('\t')
                            ) {
                                let mut sawline = false;
                                let save = self.pos;
                                let saveline = self.line;
                                while matches!(
                                    self.peek(),
                                    Some(' ') | Some('\t') | Some('\r') | Some('\n')
                                ) {
                                    if self.peek() == Some('\n') {
                                        sawline = true;
                                    }
                                    self.bump();
                                }
                                if !sawline {
                                    self.pos = save;
                                    self.line = saveline;
                                    out.push(self.string_escape()?);
                                }
                            } else {
                                out.push(self.string_escape()?);
                            }
                        }
                        Some(c) => out.push(c),
                    }
                }
            }
            fn string_escape(&mut self) -> Result<char, ParseError> {
                match self.bump() {
                    Some('"') => Ok('"'),
                    Some('\\') => Ok('\\'),
                    Some('b') => Ok('\u{0008}'),
                    Some('f') => Ok('\u{000c}'),
                    Some('n') => Ok('\n'),
                    Some('r') => Ok('\r'),
                    Some('t') => Ok('\t'),
                    Some('u') => self.unicode_escape(4),
                    Some('U') => self.unicode_escape(8),
                    Some(c) => Err(self.err(format!("invalid escape `\\{c}`"))),
                    None => Err(self.err("unterminated escape")),
                }
            }
            fn unicode_escape(&mut self, n: usize) -> Result<char, ParseError> {
                let mut v = 0u32;
                for _ in 0..n {
                    let Some(c) = self.peek() else {
                        return Err(self.err("truncated unicode escape"));
                    };
                    let Some(d) = c.to_digit(16) else {
                        return Err(self.err("invalid unicode escape"));
                    };
                    v = v * 16 + d;
                    self.pos += 1;
                }
                char::from_u32(v).ok_or_else(|| self.err("invalid unicode scalar value"))
            }
            fn literal_string(&mut self) -> Result<String, ParseError> {
                if self.peek() == Some('\'')
                    && self.peek_at(1) == Some('\'')
                    && self.peek_at(2) == Some('\'')
                {
                    return self.multiline_literal_string();
                }
                self.bump();
                let mut out = String::new();
                loop {
                    match self.bump() {
                        None | Some('\n') => return Err(self.err("unterminated literal string")),
                        Some('\'') => return Ok(out),
                        Some(c) => out.push(c),
                    }
                }
            }
            fn multiline_literal_string(&mut self) -> Result<String, ParseError> {
                self.bump();
                self.bump();
                self.bump();
                if self.peek() == Some('\r') {
                    self.bump();
                }
                if self.peek() == Some('\n') {
                    self.bump();
                }
                let mut out = String::new();
                loop {
                    if self.peek() == Some('\'')
                        && self.peek_at(1) == Some('\'')
                        && self.peek_at(2) == Some('\'')
                    {
                        self.bump();
                        self.bump();
                        self.bump();
                        return Ok(out);
                    }
                    match self.bump() {
                        None => return Err(self.err("unterminated multi-line literal string")),
                        Some(c) => out.push(c),
                    }
                }
            }
            fn array(&mut self) -> Result<Value, ParseError> {
                self.bump();
                let mut items = Vec::new();
                loop {
                    self.skip_ws_newlines_comments();
                    match self.peek() {
                        Some(']') => {
                            self.bump();
                            return Ok(Value::Array(items));
                        }
                        None => return Err(self.err("unterminated array")),
                        _ => {}
                    }
                    items.push(self.value()?);
                    self.skip_ws_newlines_comments();
                    match self.peek() {
                        Some(',') => {
                            self.bump();
                        }
                        Some(']') => {
                            self.bump();
                            return Ok(Value::Array(items));
                        }
                        Some(c) => {
                            return Err(
                                self.err(format!("expected `,` or `]` in array, found `{c}`"))
                            )
                        }
                        None => return Err(self.err("unterminated array")),
                    }
                }
            }
            fn skip_ws_newlines_comments(&mut self) {
                loop {
                    match self.peek() {
                        Some(' ' | '\t' | '\r' | '\n') => {
                            self.bump();
                        }
                        Some('#') => self.skip_comment(),
                        _ => break,
                    }
                }
            }
            fn inline_table(&mut self) -> Result<Value, ParseError> {
                self.bump();
                let mut entries = Vec::new();
                self.skip_inline_ws();
                if self.peek() == Some('}') {
                    self.bump();
                    return Ok(Value::InlineTable(entries));
                }
                loop {
                    self.skip_inline_ws();
                    let path = self.key_path()?;
                    self.skip_inline_ws();
                    if self.bump() != Some('=') {
                        return Err(self.err("expected `=` in inline table"));
                    }
                    self.skip_inline_ws();
                    let value = self.value()?;
                    entries.push((path.join("."), value));
                    self.skip_inline_ws();
                    match self.bump() {
                        Some(',') => continue,
                        Some('}') => return Ok(Value::InlineTable(entries)),
                        Some(c) => {
                            return Err(self
                                .err(format!("expected `,` or `}}` in inline table, found `{c}`")))
                        }
                        None => return Err(self.err("unterminated inline table")),
                    }
                }
            }
            fn number_or_datetime(&mut self) -> Result<Value, ParseError> {
                if self.looks_like_date() || self.looks_like_time() {
                    return self.datetime();
                }
                self.number()
            }
            fn looks_like_date(&self) -> bool {
                let d = |n: usize| self.peek_at(n).map_or(false, |c| c.is_ascii_digit());
                d(0) && d(1)
                    && d(2)
                    && d(3)
                    && self.peek_at(4) == Some('-')
                    && d(5)
                    && d(6)
                    && self.peek_at(7) == Some('-')
                    && d(8)
                    && d(9)
            }
            fn looks_like_time(&self) -> bool {
                let d = |n: usize| self.peek_at(n).map_or(false, |c| c.is_ascii_digit());
                d(0) && d(1) && self.peek_at(2) == Some(':') && d(3) && d(4)
            }
            fn datetime(&mut self) -> Result<Value, ParseError> {
                let mut s = String::new();
                let is_dt =
                    |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | ':' | '.' | '+');
                while let Some(c) = self.peek() {
                    if is_dt(c) {
                        s.push(c);
                        self.pos += 1;
                    } else if c == ' '
                        && self.peek_at(1).map_or(false, |n| n.is_ascii_digit())
                        && self.peek_at(3) == Some(':')
                    {
                        s.push(' ');
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                Ok(Value::Datetime(s))
            }
            fn number(&mut self) -> Result<Value, ParseError> {
                if self.try_keyword("inf") {
                    return Ok(Value::Float(f64::INFINITY));
                }
                if self.try_keyword("nan") {
                    return Ok(Value::Float(f64::NAN));
                }
                let mut tok = String::new();
                if matches!(self.peek(), Some('+') | Some('-')) {
                    let sign = self.bump().unwrap();
                    tok.push(sign);
                    if self.try_keyword("inf") {
                        return Ok(Value::Float(if sign == '-' {
                            f64::NEG_INFINITY
                        } else {
                            f64::INFINITY
                        }));
                    }
                    if self.try_keyword("nan") {
                        return Ok(Value::Float(f64::NAN));
                    }
                }
                if self.peek() == Some('0') {
                    if let Some(r) = self.peek_at(1) {
                        if matches!(r, 'x' | 'o' | 'b') && tok.is_empty() {
                            return self.radix_integer();
                        }
                    }
                }
                let mut is_float = false;
                while let Some(c) = self.peek() {
                    match c {
                        '0'..='9' | '_' => {
                            tok.push(c);
                            self.pos += 1;
                        }
                        '.' | 'e' | 'E' | '+' | '-' => {
                            is_float = true;
                            tok.push(c);
                            self.pos += 1;
                        }
                        _ => break,
                    }
                }
                let clean: String = tok.chars().filter(|c| *c != '_').collect();
                if is_float {
                    clean
                        .parse::<f64>()
                        .map(Value::Float)
                        .map_err(|_| self.err(format!("invalid number `{tok}`")))
                } else {
                    clean
                        .parse::<i64>()
                        .map(Value::Integer)
                        .map_err(|_| self.err(format!("invalid number `{tok}`")))
                }
            }
            fn radix_integer(&mut self) -> Result<Value, ParseError> {
                self.bump();
                let prefix = self.bump().unwrap();
                let radix = match prefix {
                    'x' => 16,
                    'o' => 8,
                    'b' => 2,
                    _ => 16,
                };
                let mut tok = String::new();
                while let Some(c) = self.peek() {
                    if c == '_' {
                        self.pos += 1;
                    } else if c.is_digit(radix) {
                        tok.push(c);
                        self.pos += 1;
                    } else {
                        break;
                    }
                }
                if tok.is_empty() {
                    return Err(self.err("expected digits after numeric base prefix"));
                }
                i64::from_str_radix(&tok, radix)
                    .map(Value::Integer)
                    .map_err(|_| self.err(format!("invalid base-{radix} integer `{tok}`")))
            }
        }

        fn is_bare_key_char(c: char) -> bool {
            c.is_ascii_alphanumeric() || c == '_' || c == '-'
        }
    }

