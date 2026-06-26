//! A complete, std-only TOML 1.0 parser (I6).
//!
//! Produces a flat, line-numbered sequence of statements — table headers and
//! key/value assignments — with every value fully typed (strings with escapes
//! and multi-line forms, integers in every base, floats incl. `inf`/`nan`,
//! booleans, datetimes, arrays, and inline tables). Schema layers (e.g.
//! `jetpack.toml` in [`super::ManifestTOML`]) walk these statements with their
//! own table-context rules; this module owns *syntax* only.
//!
//! Errors carry a 1-based line number and a specific message. Parsing recovers
//! at the next statement after an error so one run surfaces every problem.

// ──────────────────────────────────────────────
// Document model
// ──────────────────────────────────────────────

/// A fully-typed TOML value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    /// An offset/local datetime, local date, or local time — kept as its
    /// original lexeme (TOML's date types need no interpretation here).
    Datetime(String),
    Array(Vec<Value>),
    /// An inline table `{ a = 1, b = "x" }`, key order preserved.
    InlineTable(Vec<(String, Value)>),
}

/// One top-level statement.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// A `[path]` or `[[path]]` (array-of-tables) header.
    Header { path: Vec<String>, array: bool, line: usize },
    /// A `key = value` assignment; `path` is the (possibly dotted) key.
    KeyVal { path: Vec<String>, value: Value, line: usize },
}

/// A syntax error: a 1-based line and a specific message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

/// Parse a TOML document into a line-numbered statement list plus any syntax
/// errors. The statement list holds everything that parsed; on error the parser
/// skips to the next line and continues.
pub fn parse(raw: &str) -> (Vec<Item>, Vec<ParseError>) {
    let mut p = Parser { chars: raw.chars().collect(), pos: 0, line: 1 };
    let mut items = Vec::new();
    let mut errors = Vec::new();
    loop {
        p.skip_between_statements();
        if p.peek().is_none() {
            break;
        }
        let start_line = p.line;
        let was_header = p.peek() == Some('[');
        match p.statement() {
            Ok(Some(item)) => items.push(item),
            Ok(None) => {}
            Err(e) => {
                errors.push(e);
                // A failed header still emits a sentinel (empty path) so a
                // schema layer can suppress cascading errors for the keys that
                // would have belonged to it.
                if was_header {
                    items.push(Item::Header { path: Vec::new(), array: false, line: start_line });
                }
                p.recover();
            }
        }
    }
    (items, errors)
}

// ──────────────────────────────────────────────
// Parser
// ──────────────────────────────────────────────

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
        ParseError { line: self.line, message: message.into() }
    }

    /// Spaces and tabs only (never a newline).
    fn skip_inline_ws(&mut self) {
        while matches!(self.peek(), Some(' ' | '\t')) {
            self.pos += 1;
        }
    }

    /// Whitespace, newlines, and full-line/inline comments between statements.
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

    /// After an error, advance to the start of the next line.
    fn recover(&mut self) {
        while let Some(c) = self.peek() {
            if c == '\n' {
                self.bump();
                break;
            }
            self.pos += 1;
        }
    }

    /// Consume trailing inline whitespace + optional comment, then require a
    /// newline or end of input. Used after a complete statement.
    fn finish_line(&mut self) -> Result<(), ParseError> {
        self.skip_inline_ws();
        if self.peek() == Some('#') {
            self.skip_comment();
        }
        match self.peek() {
            None => Ok(()),
            Some('\n') | Some('\r') => Ok(()),
            Some(c) => Err(self.err(format!("unexpected `{c}` after value"))),
        }
    }

    fn statement(&mut self) -> Result<Option<Item>, ParseError> {
        match self.peek() {
            Some('[') => self.header().map(Some),
            _ => self.key_value().map(Some),
        }
    }

    // ── Table headers ────────────────────────────────────────────────

    fn header(&mut self) -> Result<Item, ParseError> {
        let line = self.line;
        self.bump(); // first '['
        let array = self.peek() == Some('[');
        if array {
            self.bump(); // second '['
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
            return Err(ParseError { line, message: "a table header must name a table".into() });
        }
        self.finish_line()?;
        Ok(Item::Header { path, array, line })
    }

    // ── Key/value ────────────────────────────────────────────────────

    fn key_value(&mut self) -> Result<Item, ParseError> {
        let line = self.line;
        let path = self.key_path()?;
        if path.is_empty() {
            return Err(self.err("expected a key"));
        }
        self.skip_inline_ws();
        if self.peek() != Some('=') {
            return Err(ParseError {
                line,
                message: format!("expected `=` after key `{}`", path.join(".")),
            });
        }
        self.bump();
        self.skip_inline_ws();
        let value = self.value()?;
        self.finish_line()?;
        Ok(Item::KeyVal { path, value, line })
    }

    /// A dotted key path: simple keys joined by `.`.
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

    /// A bare, basic-string, or literal-string key segment.
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

    // ── Values ───────────────────────────────────────────────────────

    fn value(&mut self) -> Result<Value, ParseError> {
        match self.peek() {
            Some('"') => Ok(Value::String(self.basic_string()?)),
            Some('\'') => Ok(Value::String(self.literal_string()?)),
            Some('[') => self.array(),
            Some('{') => self.inline_table(),
            Some('t') | Some('f') => self.boolean(),
            Some('+') | Some('-') | Some('0'..='9') | Some('i') | Some('n') => self.number_or_datetime(),
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

    /// Match an exact keyword that must not be followed by a bare-key char.
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

    // ── Strings ──────────────────────────────────────────────────────

    fn basic_string(&mut self) -> Result<String, ParseError> {
        // Distinguish `"""` multi-line from `"` single-line.
        if self.peek() == Some('"') && self.peek_at(1) == Some('"') && self.peek_at(2) == Some('"') {
            return self.multiline_basic_string();
        }
        self.bump(); // opening "
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
        self.bump(); // opening """
        // A newline immediately after the opening delimiter is trimmed.
        if self.peek() == Some('\r') {
            self.bump();
        }
        if self.peek() == Some('\n') {
            self.bump();
        }
        let mut out = String::new();
        loop {
            if self.peek() == Some('"') && self.peek_at(1) == Some('"') && self.peek_at(2) == Some('"') {
                self.bump();
                self.bump();
                self.bump();
                return Ok(out);
            }
            match self.bump() {
                None => return Err(self.err("unterminated multi-line string")),
                Some('\\') => {
                    // A backslash before a newline trims following whitespace.
                    if matches!(self.peek(), Some('\n') | Some('\r') | Some(' ') | Some('\t')) {
                        // Line-ending backslash: skip whitespace incl. newlines.
                        let mut sawline = false;
                        let save = self.pos;
                        let saveline = self.line;
                        while matches!(self.peek(), Some(' ') | Some('\t') | Some('\r') | Some('\n')) {
                            if self.peek() == Some('\n') {
                                sawline = true;
                            }
                            self.bump();
                        }
                        if !sawline {
                            // Not a line-ending backslash — restore and treat as escape.
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

    /// An escape sequence, already past the backslash.
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
        if self.peek() == Some('\'') && self.peek_at(1) == Some('\'') && self.peek_at(2) == Some('\'') {
            return self.multiline_literal_string();
        }
        self.bump(); // opening '
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
        self.bump(); // opening '''
        if self.peek() == Some('\r') {
            self.bump();
        }
        if self.peek() == Some('\n') {
            self.bump();
        }
        let mut out = String::new();
        loop {
            if self.peek() == Some('\'') && self.peek_at(1) == Some('\'') && self.peek_at(2) == Some('\'') {
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

    // ── Arrays & inline tables ───────────────────────────────────────

    fn array(&mut self) -> Result<Value, ParseError> {
        self.bump(); // [
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
                Some(c) => return Err(self.err(format!("expected `,` or `]` in array, found `{c}`"))),
                None => return Err(self.err("unterminated array")),
            }
        }
    }

    /// Whitespace, newlines, and comments — allowed between array elements.
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
        self.bump(); // {
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
                Some(c) => return Err(self.err(format!("expected `,` or `}}` in inline table, found `{c}`"))),
                None => return Err(self.err("unterminated inline table")),
            }
        }
    }

    // ── Numbers & datetimes ──────────────────────────────────────────

    fn number_or_datetime(&mut self) -> Result<Value, ParseError> {
        // Datetime: a date `DDDD-DD-DD` or a time `DD:DD:DD` at the value start.
        if self.looks_like_date() || self.looks_like_time() {
            return self.datetime();
        }
        self.number()
    }

    fn looks_like_date(&self) -> bool {
        // 4 digits, '-', 2 digits, '-', 2 digits.
        let d = |n: usize| self.peek_at(n).map_or(false, |c| c.is_ascii_digit());
        d(0) && d(1) && d(2) && d(3)
            && self.peek_at(4) == Some('-')
            && d(5) && d(6)
            && self.peek_at(7) == Some('-')
            && d(8) && d(9)
    }

    fn looks_like_time(&self) -> bool {
        let d = |n: usize| self.peek_at(n).map_or(false, |c| c.is_ascii_digit());
        d(0) && d(1) && self.peek_at(2) == Some(':') && d(3) && d(4)
    }

    fn datetime(&mut self) -> Result<Value, ParseError> {
        let mut s = String::new();
        // Greedily consume the date/time/offset characters.
        let is_dt = |c: char| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | ':' | '.' | '+')
        };
        while let Some(c) = self.peek() {
            if is_dt(c) {
                s.push(c);
                self.pos += 1;
            } else if c == ' ' {
                // A single space may separate date and time (`1979-05-27 07:32:00`).
                if self.peek_at(1).map_or(false, |n| n.is_ascii_digit())
                    && self.peek_at(3) == Some(':')
                {
                    s.push(' ');
                    self.pos += 1;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        Ok(Value::Datetime(s))
    }

    fn number(&mut self) -> Result<Value, ParseError> {
        // inf / nan, with optional sign.
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
                return Ok(Value::Float(if sign == '-' { f64::NEG_INFINITY } else { f64::INFINITY }));
            }
            if self.try_keyword("nan") {
                return Ok(Value::Float(f64::NAN));
            }
        }
        // Radix prefixes (hex/oct/bin) — only valid unsigned.
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
            clean.parse::<f64>().map(Value::Float).map_err(|_| self.err(format!("invalid number `{tok}`")))
        } else {
            clean.parse::<i64>().map(Value::Integer).map_err(|_| self.err(format!("invalid number `{tok}`")))
        }
    }

    fn radix_integer(&mut self) -> Result<Value, ParseError> {
        self.bump(); // 0
        let prefix = self.bump().unwrap(); // x | o | b
        let radix = match prefix {
            'x' => 16,
            'o' => 8,
            'b' => 2,
            _ => unreachable!(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(raw: &str) -> Vec<Item> {
        let (items, errors) = parse(raw);
        assert!(errors.is_empty(), "unexpected errors: {errors:?}");
        items
    }

    fn kv(items: &[Item], key: &str) -> Value {
        for it in items {
            if let Item::KeyVal { path, value, .. } = it {
                if path.join(".") == key {
                    return value.clone();
                }
            }
        }
        panic!("no key `{key}` in {items:?}");
    }

    #[test]
    fn typed_scalars() {
        let items = parse_ok(
            "i = 42\nneg = -7\nhex = 0xFF\noct = 0o17\nbin = 0b101\nf = 3.14\ne = 1.5e3\nb = true\nund = 1_000\n",
        );
        assert_eq!(kv(&items, "i"), Value::Integer(42));
        assert_eq!(kv(&items, "neg"), Value::Integer(-7));
        assert_eq!(kv(&items, "hex"), Value::Integer(255));
        assert_eq!(kv(&items, "oct"), Value::Integer(15));
        assert_eq!(kv(&items, "bin"), Value::Integer(5));
        assert_eq!(kv(&items, "f"), Value::Float(3.14));
        assert_eq!(kv(&items, "e"), Value::Float(1500.0));
        assert_eq!(kv(&items, "b"), Value::Boolean(true));
        assert_eq!(kv(&items, "und"), Value::Integer(1000));
    }

    #[test]
    fn strings_and_escapes() {
        let items = parse_ok("a = \"x\\ty\\u00e9\"\nb = 'literal\\nraw'\n");
        assert_eq!(kv(&items, "a"), Value::String("x\ty\u{e9}".into()));
        assert_eq!(kv(&items, "b"), Value::String("literal\\nraw".into()));
    }

    #[test]
    fn multiline_basic_string() {
        let items = parse_ok("a = \"\"\"\nfirst\nsecond\"\"\"\n");
        assert_eq!(kv(&items, "a"), Value::String("first\nsecond".into()));
    }

    #[test]
    fn multiline_line_ending_backslash() {
        let items = parse_ok("a = \"\"\"\\\n  trimmed\"\"\"\n");
        assert_eq!(kv(&items, "a"), Value::String("trimmed".into()));
    }

    #[test]
    fn arrays_multiline_and_mixed() {
        let items = parse_ok("a = [\n 1,\n 2,\n 3,\n]\nb = [\"x\", \"y\"]\n");
        assert_eq!(kv(&items, "a"), Value::Array(vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)]));
        assert_eq!(kv(&items, "b"), Value::Array(vec![Value::String("x".into()), Value::String("y".into())]));
    }

    #[test]
    fn inline_table() {
        let items = parse_ok("pt = { x = 1, y = 2 }\n");
        assert_eq!(
            kv(&items, "pt"),
            Value::InlineTable(vec![("x".into(), Value::Integer(1)), ("y".into(), Value::Integer(2))])
        );
    }

    #[test]
    fn dotted_keys_and_quoted_keys() {
        let items = parse_ok("a.b.c = 1\n\"quoted key\" = 2\n");
        assert_eq!(kv(&items, "a.b.c"), Value::Integer(1));
        assert_eq!(kv(&items, "quoted key"), Value::Integer(2));
    }

    #[test]
    fn headers_and_arrays_of_tables() {
        let items = parse_ok("[a.b]\nx = 1\n[[srv]]\nip = \"1\"\n[[srv]]\nip = \"2\"\n");
        assert_eq!(items[0], Item::Header { path: vec!["a".into(), "b".into()], array: false, line: 1 });
        assert_eq!(items[2], Item::Header { path: vec!["srv".into()], array: true, line: 3 });
    }

    #[test]
    fn datetimes_kept_raw() {
        let items = parse_ok("dt = 1979-05-27T07:32:00Z\nd = 1979-05-27\nt = 07:32:00\nls = 1979-05-27 07:32:00\n");
        assert_eq!(kv(&items, "dt"), Value::Datetime("1979-05-27T07:32:00Z".into()));
        assert_eq!(kv(&items, "d"), Value::Datetime("1979-05-27".into()));
        assert_eq!(kv(&items, "t"), Value::Datetime("07:32:00".into()));
        assert_eq!(kv(&items, "ls"), Value::Datetime("1979-05-27 07:32:00".into()));
    }

    #[test]
    fn inline_comment_and_recovery() {
        let (items, errors) = parse("a = 1 # ok\nbad line\nb = 2\n");
        assert_eq!(kv(&items, "a"), Value::Integer(1));
        assert_eq!(kv(&items, "b"), Value::Integer(2));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].line, 2);
    }
}
