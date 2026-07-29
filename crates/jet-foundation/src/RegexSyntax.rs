// Canonical syntax checker for Jet's std-only linear regex engine.
//
// Sema uses this parser for typed `Regex.{"…"}` literals. Generated programs
// use it before the runtime compiler, so compile-time and runtime patterns
// accept the same grammar.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegexSyntaxError {
    /// Character offset in the decoded pattern text.
    pub offset: usize,
    pub reason: String,
}

pub fn validate(pattern: &str) -> Result<(), RegexSyntaxError> {
    let mut parser = Parser {
        chars: pattern.chars().collect(),
        pos: 0,
    };
    parser.parse_alt(None)?;
    if parser.pos != parser.chars.len() {
        return parser.error("unexpected trailing input");
    }
    Ok(())
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn parse_alt(&mut self, terminator: Option<char>) -> Result<(), RegexSyntaxError> {
        self.parse_seq(terminator)?;
        while self.peek() == Some('|') {
            self.pos += 1;
            self.parse_seq(terminator)?;
        }
        if let Some(end) = terminator {
            if self.peek() != Some(end) {
                return self.error(&format!("missing `{end}`"));
            }
            self.pos += 1;
        }
        Ok(())
    }

    fn parse_seq(&mut self, terminator: Option<char>) -> Result<(), RegexSyntaxError> {
        while let Some(ch) = self.peek() {
            if Some(ch) == terminator || ch == '|' {
                break;
            }
            self.parse_atom()?;
            self.parse_quant()?;
        }
        Ok(())
    }

    fn parse_atom(&mut self) -> Result<(), RegexSyntaxError> {
        let Some(ch) = self.bump() else {
            return self.error("empty atom");
        };
        match ch {
            '.' | '^' | '$' => Ok(()),
            '(' => self.parse_group(),
            ')' => self.error("unmatched `)`"),
            '[' => self.parse_class(),
            '\\' => self.parse_escape_atom(),
            '*' | '+' | '?' => self.error(&format!("`{ch}` has nothing to repeat")),
            '{' => self.error("`{n}` has nothing to repeat"),
            _ => Ok(()),
        }
    }

    fn parse_group(&mut self) -> Result<(), RegexSyntaxError> {
        if self.peek() == Some('?') {
            self.pos += 1;
            match self.bump() {
                Some(':') => {}
                Some('<') => self.parse_group_name()?,
                Some('=') | Some('!') => {
                    return self.error("lookaround is not supported; use a linear rewrite");
                }
                Some(other) => return self.error(&format!("unsupported group `?{other}`")),
                None => return self.error("missing group kind after `?`"),
            }
        }
        self.parse_alt(Some(')'))
    }

    fn parse_group_name(&mut self) -> Result<(), RegexSyntaxError> {
        let start = self.pos;
        while self.peek().is_some_and(|ch| ch != '>') {
            self.pos += 1;
        }
        if self.bump() != Some('>') {
            return self.error("missing `>` in named group");
        }
        let name: String = self.chars[start..self.pos - 1].iter().collect();
        if name.is_empty()
            || !name
                .chars()
                .all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
        {
            return self.error("named group needs an identifier");
        }
        Ok(())
    }

    fn parse_quant(&mut self) -> Result<(), RegexSyntaxError> {
        match self.peek() {
            Some('*' | '+' | '?') => self.pos += 1,
            Some('{') => {
                self.pos += 1;
                let min = self.parse_number()?;
                let max = if self.peek() == Some(',') {
                    self.pos += 1;
                    if self.peek() == Some('}') {
                        None
                    } else {
                        Some(self.parse_number()?)
                    }
                } else {
                    Some(min)
                };
                if self.bump() != Some('}') {
                    return self.error("missing `}` in quantifier");
                }
                if max.is_some_and(|value| value < min) {
                    return self.error("quantifier max is below min");
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn parse_class(&mut self) -> Result<(), RegexSyntaxError> {
        if self.peek() == Some('^') {
            self.pos += 1;
        }
        while let Some(ch) = self.peek() {
            if ch == ']' {
                self.pos += 1;
                return Ok(());
            }
            let range_start = self.parse_class_item()?;
            if self.peek() == Some('-') && self.peek_n(1) != Some(']') {
                self.pos += 1;
                self.parse_class_char()?;
                if !range_start {
                    return self.error("class range needs literal endpoints");
                }
            }
        }
        self.error("missing `]`")
    }

    fn parse_class_item(&mut self) -> Result<bool, RegexSyntaxError> {
        if self.peek() != Some('\\') {
            self.parse_class_char()?;
            return Ok(true);
        }
        self.pos += 1;
        match self.bump() {
            Some('d' | 'w' | 's') => Ok(false),
            Some('p') => {
                self.parse_unicode_class()?;
                Ok(false)
            }
            Some('P') => {
                self.error("negated Unicode classes belong outside `[]` today")
            }
            Some(_) => Ok(true),
            None => self.error("missing escape"),
        }
    }

    fn parse_class_char(&mut self) -> Result<(), RegexSyntaxError> {
        match self.bump() {
            Some(']') | None => self.error("missing class character"),
            Some('\\') => match self.bump() {
                Some(_) => Ok(()),
                None => self.error("missing escape"),
            },
            Some(_) => Ok(()),
        }
    }

    fn parse_escape_atom(&mut self) -> Result<(), RegexSyntaxError> {
        match self.bump() {
            Some('d' | 'D' | 'w' | 'W' | 's' | 'S') => Ok(()),
            Some('p' | 'P') => self.parse_unicode_class(),
            Some(ch) if ch.is_ascii_digit() => {
                self.error("backreferences are not supported; captures stay linear")
            }
            Some(_) => Ok(()),
            None => self.error("missing escape"),
        }
    }

    fn parse_unicode_class(&mut self) -> Result<(), RegexSyntaxError> {
        if self.bump() != Some('{') {
            return self.error("Unicode class needs `{...}`");
        }
        let start = self.pos;
        while self.peek().is_some_and(|ch| ch != '}') {
            self.pos += 1;
        }
        if self.bump() != Some('}') {
            return self.error("missing `}` in Unicode class");
        }
        let name: String = self.chars[start..self.pos - 1].iter().collect();
        if matches!(
            name.as_str(),
            "L" | "Letter" | "N" | "Number" | "Alphabetic" | "White_Space" | "Whitespace"
        ) {
            Ok(())
        } else {
            self.error(&format!("unsupported Unicode class `{name}`"))
        }
    }

    fn parse_number(&mut self) -> Result<usize, RegexSyntaxError> {
        let start = self.pos;
        while self.peek().is_some_and(|ch| ch.is_ascii_digit()) {
            self.pos += 1;
        }
        if self.pos == start {
            return self.error("quantifier needs a number");
        }
        self.chars[start..self.pos]
            .iter()
            .collect::<String>()
            .parse()
            .map_err(|_| RegexSyntaxError {
                offset: start,
                reason: "quantifier is too large".to_string(),
            })
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek_n(&self, offset: usize) -> Option<char> {
        self.chars.get(self.pos + offset).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let ch = self.peek()?;
        self.pos += 1;
        Some(ch)
    }

    fn error<T>(&self, reason: &str) -> Result<T, RegexSyntaxError> {
        Err(RegexSyntaxError {
            offset: self.pos.saturating_sub(1),
            reason: reason.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::validate;

    #[test]
    fn accepts_runtime_engine_grammar() {
        for pattern in [
            r"\d{2,4}",
            r"^(?<word>[\p{Alphabetic}_]+)$",
            r"(?:a|b)*",
            r"[^a-z]\P{Number}",
        ] {
            assert!(validate(pattern).is_ok(), "{pattern}");
        }
    }

    #[test]
    fn reports_character_offset() {
        let error = validate("(unclosed").unwrap_err();
        assert_eq!(error.offset, 8);
        assert_eq!(error.reason, "missing `)`");
    }
}
