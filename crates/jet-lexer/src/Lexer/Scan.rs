//! Raw scanning: `lex_raw` (no terminator insertion), the main `run` loop, and
//! number/char/digit scanning.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;

use super::Tokens::{TokKind, Token};
use super::{keyword, Lexer};

/// Raw lex with no S6-R terminator insertion. Used for interpolation
/// sub-streams (`{expr}`), which are single expressions and need no terminator.
pub fn lex_raw(src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    lex_raw_with_policy(src, false)
}

pub(super) fn lex_raw_generated(src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    lex_raw_with_policy(src, true)
}

fn lex_raw_with_policy(src: &str, allow_reserved_identifiers: bool) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut lx = Lexer {
        chars: src.char_indices().collect(),
        end: src.len(),
        src,
        i: 0,
        diags: Vec::new(),
        allow_reserved_identifiers,
    };
    let mut toks = lx.run();
    toks.push(Token {
        kind: TokKind::Eof,
        span: Span::new(src.len(), src.len()),
    });
    (toks, lx.diags)
}

impl<'a> Lexer<'a> {
    fn starts_inline_foreign_body(toks: &[Token]) -> bool {
        if !matches!(toks.last().map(|t| &t.kind), Some(TokKind::LBrace)) {
            return false;
        }
        let Some(fn_pos) = toks.iter().rposition(|t| matches!(t.kind, TokKind::KwFn)) else {
            return false;
        };
        let significant = toks[..fn_pos]
            .iter()
            .filter(|token| {
                !matches!(
                    token.kind,
                    TokKind::LineComment(_)
                        | TokKind::BlockComment(_)
                        | TokKind::KwPub
                        | TokKind::Semi
                )
            })
            .collect::<Vec<_>>();
        let is_marker_name = |token: &&Token| matches!(token.kind, TokKind::Ident(_));
        let is_ffi = |token: &&Token| {
            matches!(&token.kind, TokKind::Ident(name) if name == Syntax::ATTR_FFI)
        };
        let mut cursor = significant.len();
        let mut saw_ffi = false;
        loop {
            if cursor >= 2
                && matches!(significant[cursor - 2].kind, TokKind::Hash)
                && is_marker_name(&significant[cursor - 1])
            {
                saw_ffi |= is_ffi(&significant[cursor - 1]);
                cursor -= 2;
                continue;
            }
            if cursor > 0 && matches!(significant[cursor - 1].kind, TokKind::RParen) {
                let mut depth = 0usize;
                let mut open = None;
                for index in (0..cursor).rev() {
                    match significant[index].kind {
                        TokKind::RParen => depth += 1,
                        TokKind::LParen => {
                            depth -= 1;
                            if depth == 0 {
                                open = Some(index);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(open) = open {
                    if open >= 2
                        && matches!(significant[open - 2].kind, TokKind::Hash)
                        && is_marker_name(&significant[open - 1])
                    {
                        saw_ffi |= is_ffi(&significant[open - 1]);
                        cursor = open - 2;
                        continue;
                    }
                }
            }
            if cursor > 0 && matches!(significant[cursor - 1].kind, TokKind::RBracket) {
                let mut depth = 0usize;
                let mut open = None;
                for index in (0..cursor).rev() {
                    match significant[index].kind {
                        TokKind::RBracket => depth += 1,
                        TokKind::LBracket => {
                            depth -= 1;
                            if depth == 0 {
                                open = Some(index);
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(open) = open {
                    if open > 0 && matches!(significant[open - 1].kind, TokKind::Hash) {
                        let mut paren_depth = 0usize;
                        let mut bracket_depth = 0usize;
                        let mut expects_name = true;
                        for token in &significant[open + 1..cursor - 1] {
                            if expects_name && paren_depth == 0 && bracket_depth == 0 {
                                saw_ffi |= is_ffi(token);
                                expects_name = false;
                            }
                            match token.kind {
                                TokKind::LParen => paren_depth += 1,
                                TokKind::RParen => paren_depth = paren_depth.saturating_sub(1),
                                TokKind::LBracket => bracket_depth += 1,
                                TokKind::RBracket => {
                                    bracket_depth = bracket_depth.saturating_sub(1);
                                }
                                TokKind::Comma if paren_depth == 0 && bracket_depth == 0 => {
                                    expects_name = true;
                                }
                                _ => {}
                            }
                        }
                        cursor = open - 1;
                        continue;
                    }
                }
            }
            break;
        }
        saw_ffi
    }

    pub(super) fn at(&self, i: usize) -> char {
        if i < self.chars.len() {
            self.chars[i].1
        } else {
            '\0'
        }
    }

    pub(super) fn pos(&self, i: usize) -> usize {
        if i < self.chars.len() {
            self.chars[i].0
        } else {
            self.end
        }
    }

    fn run(&mut self) -> Vec<Token> {
        let mut toks = Vec::new();
        while self.i < self.chars.len() {
            let c = self.at(self.i);

            if c.is_whitespace() {
                self.i += 1;
                continue;
            }

            // Line comments (decision S5) — retained for fmt (M6/S44).
            if c == '/' && self.at(self.i + 1) == '/' {
                let comment_start = self.pos(self.i);
                while self.i < self.chars.len() && self.at(self.i) != '\n' {
                    self.i += 1;
                }
                let text = self.src[comment_start..self.pos(self.i)].to_string();
                toks.push(Token {
                    kind: TokKind::LineComment(text),
                    span: Span::new(comment_start, self.pos(self.i)),
                });
                continue;
            }

            // Block comments (decision S5) — nest, so a region containing other
            // comments can always be commented out. Retained for fmt (M6/S44).
            if c == '/' && self.at(self.i + 1) == '*' {
                let comment_start = self.pos(self.i);
                self.i += 2;
                let mut depth = 1usize;
                while self.i < self.chars.len() && depth > 0 {
                    if self.at(self.i) == '/' && self.at(self.i + 1) == '*' {
                        depth += 1;
                        self.i += 2;
                    } else if self.at(self.i) == '*' && self.at(self.i + 1) == '/' {
                        depth -= 1;
                        self.i += 2;
                    } else {
                        self.i += 1;
                    }
                }
                let end = self.pos(self.i);
                if depth > 0 {
                    self.diags.push(Diagnostic::error(
                        "E0002",
                        "this `/*` comment never gets a closing `*/`".to_string(),
                        "a block comment starts with `/*` and runs until a matching `*/`"
                            .to_string(),
                        "add a `*/` to close the comment".to_string(),
                        Some(Span::new(comment_start, end)),
                    ));
                }
                let text = self.src[comment_start..end].to_string();
                toks.push(Token {
                    kind: TokKind::BlockComment(text),
                    span: Span::new(comment_start, end),
                });
                continue;
            }

            let start = self.pos(self.i);
            let simple = |lx: &mut Self, kind: TokKind, len: usize| {
                let tok = Token {
                    kind,
                    span: Span::new(start, lx.pos(lx.i + len)),
                };
                lx.i += len;
                tok
            };

            let next = self.at(self.i + 1);
            let next2 = self.at(self.i + 2);
            match c {
                '(' => toks.push(simple(self, TokKind::LParen, 1)),
                ')' => toks.push(simple(self, TokKind::RParen, 1)),
                '{' => toks.push(simple(self, TokKind::LBrace, 1)),
                '}' => toks.push(simple(self, TokKind::RBrace, 1)),
                '[' => toks.push(simple(self, TokKind::LBracket, 1)),
                // D-VERDICT-1320-1: fence close digraph `]$` is longest-match
                // before plain `]`. No legal program puts `$` right after `]`.
                ']' if next == '$' => toks.push(simple(self, TokKind::FenceClose, 2)),
                ']' => toks.push(simple(self, TokKind::RBracket, 1)),
                // D-BIND4: `:=` mutable binding sigil.
                ':' if next == ':' => toks.push(simple(self, TokKind::ColonColon, 2)),
                ':' if next == '=' => toks.push(simple(self, TokKind::ColonEq, 2)),
                ':' => toks.push(simple(self, TokKind::Colon, 1)),
                ',' => toks.push(simple(self, TokKind::Comma, 1)),
                ';' => toks.push(simple(self, TokKind::Semi, 1)),
                '@' => toks.push(simple(self, TokKind::At, 1)),
                '#' => toks.push(simple(self, TokKind::Hash, 1)),
                // D-VERDICT-1320-1: fence open digraph `$[` is longest-match
                // before the D-CTMARKER1 `$name` comptime splice.
                '$' if next == '[' => toks.push(simple(self, TokKind::FenceOpen, 2)),
                '$' => toks.push(simple(self, TokKind::Dollar, 1)),
                '?' if next == '?' => toks.push(simple(self, TokKind::QuestionQuestion, 2)),
                '?' if next == '.' => toks.push(simple(self, TokKind::QuestionDot, 2)),
                '?' => toks.push(simple(self, TokKind::Question, 1)),
                '.' if next == '.' && next2 == '.' => {
                    toks.push(simple(self, TokKind::DotDotDot, 3))
                }
                // D-RANGE-EXCL1=C: `..<` before plain `..` (longest match).
                '.' if next == '.' && next2 == '<' => {
                    toks.push(simple(self, TokKind::DotDotLt, 3))
                }
                '.' if next == '.' => toks.push(simple(self, TokKind::DotDot, 2)),
                '.' => toks.push(simple(self, TokKind::Dot, 1)),
                '=' if next == '=' => toks.push(simple(self, TokKind::EqEq, 2)),
                '=' if next == '>' => toks.push(simple(self, TokKind::LambdaArrow, 2)),
                '=' => toks.push(simple(self, TokKind::Eq, 1)),
                '!' if next == '=' => toks.push(simple(self, TokKind::NotEq, 2)),
                '!' => toks.push(simple(self, TokKind::Bang, 1)),
                '+' if next == '+' => toks.push(simple(self, TokKind::PlusPlus, 2)),
                '+' if next == '=' => toks.push(simple(self, TokKind::PlusEq, 2)),
                '+' => toks.push(simple(self, TokKind::Plus, 1)),
                '-' if next == '>' => toks.push(simple(self, TokKind::Arrow, 2)),
                '-' if next == '-' => toks.push(simple(self, TokKind::MinusMinus, 2)),
                '-' if next == '=' => toks.push(simple(self, TokKind::MinusEq, 2)),
                '-' => toks.push(simple(self, TokKind::Minus, 1)),
                '*' if next == '=' => toks.push(simple(self, TokKind::StarEq, 2)),
                '*' => toks.push(simple(self, TokKind::Star, 1)),
                '/' if next == '=' => toks.push(simple(self, TokKind::SlashEq, 2)),
                '/' => toks.push(simple(self, TokKind::Slash, 1)),
                '%' if next == '=' => toks.push(simple(self, TokKind::PercentEq, 2)),
                '%' => toks.push(simple(self, TokKind::Percent, 1)),
                '^' if next == '=' => toks.push(simple(self, TokKind::CaretEq, 2)),
                '^' => toks.push(simple(self, TokKind::Caret, 1)),
                // D-SHAPE-COPY1=A: `~` is the copy sigil. `~~` is longest-match
                // lexed first so the parser can still emit the retired
                // external-method connector diagnostic (E0325).
                '~' if next == '~' => toks.push(simple(self, TokKind::TildeTilde, 2)),
                // D-XORSPELL1=A: `~|=` before `~|` before `~` (longest match).
                '~' if next == '|' && next2 == '=' => {
                    toks.push(simple(self, TokKind::TildePipeEq, 3))
                }
                '~' if next == '|' => toks.push(simple(self, TokKind::TildePipe, 2)),
                '~' => toks.push(simple(self, TokKind::Tilde, 1)),
                '&' if next == '&' => toks.push(simple(self, TokKind::AndAnd, 2)),
                '&' if next == '=' => toks.push(simple(self, TokKind::AmpEq, 2)),
                '&' => toks.push(simple(self, TokKind::Amp, 1)),
                '|' if next == '|' => toks.push(simple(self, TokKind::OrOr, 2)),
                '|' if next == '=' => toks.push(simple(self, TokKind::PipeEq, 2)),
                '|' => toks.push(simple(self, TokKind::Pipe, 1)),
                '<' if next == '<' && next2 == '=' => toks.push(simple(self, TokKind::ShlEq, 3)),
                '<' if next == '<' => toks.push(simple(self, TokKind::Shl, 2)),
                '<' if next == '=' => toks.push(simple(self, TokKind::Le, 2)),
                '<' => toks.push(simple(self, TokKind::Lt, 1)),
                '>' if next == '>' && next2 == '=' => toks.push(simple(self, TokKind::ShrEq, 3)),
                '>' if next == '>' => toks.push(simple(self, TokKind::Shr, 2)),
                '>' if next == '=' => toks.push(simple(self, TokKind::Ge, 2)),
                '>' => toks.push(simple(self, TokKind::Gt, 1)),
                // D-UNIFYLIT1=A: `b"…"` retired — byte patterns use `[U8].{"…"}`.
                // Bare `b` followed by `"` falls through to the identifier lexer.
                '"' => {
                    let tok = if next == '"' && next2 == '"' {
                        if Self::starts_inline_foreign_body(&toks) {
                            self.raw_foreign_string(start)
                        } else {
                            self.triple_string(start)
                        }
                    } else {
                        self.string(start)
                    };
                    if let Some(tok) = tok {
                        toks.push(tok);
                    }
                }
                '\'' => {
                    if let Some(tok) = self.char_lit(start) {
                        toks.push(tok);
                    }
                }
                c if c.is_ascii_digit() => toks.push(self.number(start)),
                c if c.is_alphabetic() || c == '_' => {
                    let mut name = String::new();
                    while self.i < self.chars.len() {
                        let ch = self.at(self.i);
                        if ch.is_alphanumeric() || ch == '_' {
                            name.push(ch);
                            self.i += 1;
                        } else {
                            break;
                        }
                    }
                    let span = Span::new(start, self.pos(self.i));
                    if !self.allow_reserved_identifiers
                        && Syntax::classify_identifier(&name) == Syntax::IdentifierClass::Reserved
                    {
                        self.diags.push(Diagnostic::error(
                            "E0067",
                            format!("`{name}` is reserved for Jet"),
                            "double-underscore names belong to the compiler, generated binders, debugger, serializer, and tools"
                                .to_string(),
                            "rename it without the second leading underscore".to_string(),
                            Some(span),
                        ));
                    }
                    let kind = keyword(&name).unwrap_or(TokKind::Ident(name));
                    toks.push(Token { kind, span });
                }
                other => {
                    self.diags.push(Diagnostic::error(
                        "E0001",
                        format!("the character `{}` doesn't mean anything here (yet)", other),
                        "check docs/spec/spec.md for what's supported so far".to_string(),
                        "remove it, or use supported syntax".to_string(),
                        Some(Span::new(start, self.pos(self.i + 1))),
                    ));
                    self.i += 1; // skip it and keep lexing (error recovery)
                }
            }
        }
        toks
    }

    /// Lex digits, with an optional decimal part (S11 Float).
    /// S34: `_` digit separators (stripped), `0x`/`0o`/`0b` base prefixes, and
    /// a `e`/`E` exponent on floats. `1..10` stays Int DotDot Int: a `.` only
    /// starts the decimal part when a digit follows it.
    fn number(&mut self, start: usize) -> Token {
        // Base-prefixed integers: 0x / 0o / 0b (S34).
        if self.at(self.i) == '0' {
            let radix = match self.at(self.i + 1) {
                'x' | 'X' => Some(16u32),
                'o' | 'O' => Some(8),
                'b' | 'B' => Some(2),
                _ => None,
            };
            if let Some(radix) = radix {
                self.i += 2; // consume `0x` / `0o` / `0b`
                let mut digits = String::new();
                while self.i < self.chars.len() {
                    let ch = self.at(self.i);
                    if ch == Syntax::DIGIT_SEPARATOR {
                        self.i += 1;
                    } else if ch.to_digit(radix).is_some() {
                        digits.push(ch);
                        self.i += 1;
                    } else {
                        break;
                    }
                }
                let span = Span::new(start, self.pos(self.i));
                if digits.is_empty() {
                    self.diags.push(Diagnostic::error(
                        "E0001",
                        "this number prefix has no digits after it".to_string(),
                        "`0x`, `0o`, and `0b` must be followed by digits, e.g. `0xFF`, `0o17`, `0b1010`"
                            .to_string(),
                        "add digits after the base prefix".to_string(),
                        Some(span),
                    ));
                    return Token {
                        kind: TokKind::Int(0, self.src[span.start..span.end].to_string()),
                        span,
                    };
                }
                return match i64::from_str_radix(&digits, radix) {
                    Ok(n) => Token {
                        kind: TokKind::Int(n, self.src[span.start..span.end].to_string()),
                        span,
                    },
                    Err(_) => {
                        self.diags.push(self.too_big(span));
                        Token {
                            kind: TokKind::Int(0, self.src[span.start..span.end].to_string()),
                            span,
                        }
                    }
                };
            }
        }

        let mut text = String::new();
        self.lex_digits(&mut text);
        let mut is_float = false;
        if self.at(self.i) == '.' && self.at(self.i + 1).is_ascii_digit() {
            is_float = true;
            text.push('.');
            self.i += 1;
            self.lex_digits(&mut text);
        }
        // Exponent (S34): `e`/`E`, an optional sign, then digits — makes a Float.
        if matches!(self.at(self.i), 'e' | 'E') {
            let after = self.at(self.i + 1);
            let exp_ok = after.is_ascii_digit()
                || ((after == '+' || after == '-') && self.at(self.i + 2).is_ascii_digit());
            if exp_ok {
                is_float = true;
                text.push('e');
                self.i += 1;
                if matches!(self.at(self.i), '+' | '-') {
                    text.push(self.at(self.i));
                    self.i += 1;
                }
                self.lex_digits(&mut text);
            }
        }
        // D-UNITLIT1: a trailing identifier run right after the number is a
        // unit-suffix candidate — `500ms`, `12.50usd`. The `e`/`E`+digits
        // exponent form was already consumed above (`UNIT_SUFFIX_EXPONENT_RESERVED`),
        // so anything reaching here is never a float exponent; a bare `e`/`E`
        // with no following digit falls through and IS eligible as a suffix.
        let suffix = self.lex_unit_suffix();

        if is_float {
            // digits '.' digits (with optional exponent) always parses as f64.
            let v: f64 = text.parse().unwrap_or(0.0);
            if let Some(suffix) = suffix {
                let span = Span::new(start, self.pos(self.i));
                return Token {
                    kind: TokKind::UnitNumber {
                        raw: text,
                        int: None,
                        float: Some(v),
                        suffix,
                    },
                    span,
                };
            }
            let span = Span::new(start, self.pos(self.i));
            return Token {
                kind: TokKind::Float(v),
                span,
            };
        }
        match text.parse::<i64>() {
            Ok(n) => {
                if let Some(suffix) = suffix {
                    let span = Span::new(start, self.pos(self.i));
                    return Token {
                        kind: TokKind::UnitNumber {
                            raw: text,
                            int: Some(n),
                            float: None,
                            suffix,
                        },
                        span,
                    };
                }
                let span = Span::new(start, self.pos(self.i));
                Token {
                    kind: TokKind::Int(n, self.src[span.start..span.end].to_string()),
                    span,
                }
            }
            Err(_) => {
                let span = Span::new(start, self.pos(self.i));
                self.diags.push(self.too_big(span));
                Token {
                    kind: TokKind::Int(0, self.src[span.start..span.end].to_string()),
                    span,
                }
            }
        }
    }

    /// D-UNITLIT1: greedily read a trailing identifier run (ASCII letter/`_`
    /// start, alphanumeric/`_` continue) right after a numeric literal, with
    /// no space between. Returns `None` when the next char doesn't start an
    /// identifier (the common case — a plain number).
    fn lex_unit_suffix(&mut self) -> Option<String> {
        let c = self.at(self.i);
        if !(c.is_ascii_alphabetic() || c == '_') {
            return None;
        }
        let mut suffix = String::new();
        while self.i < self.chars.len() {
            let ch = self.at(self.i);
            if ch.is_alphanumeric() || ch == '_' {
                suffix.push(ch);
                self.i += 1;
            } else {
                break;
            }
        }
        Some(suffix)
    }

    /// Consume a run of decimal digits, skipping `_` separators (S34).
    fn lex_digits(&mut self, text: &mut String) {
        while self.i < self.chars.len() {
            let ch = self.at(self.i);
            if ch.is_ascii_digit() {
                text.push(ch);
                self.i += 1;
            } else if ch == Syntax::DIGIT_SEPARATOR {
                self.i += 1;
            } else {
                break;
            }
        }
    }

    fn too_big(&self, span: Span) -> Diagnostic {
        Diagnostic::error(
            "E0007",
            "this number is too big".to_string(),
            "numbers currently top out at 9223372036854775807 (a 64-bit integer)".to_string(),
            "use a smaller number".to_string(),
            Some(span),
        )
    }

    /// S41: `'a'` or `'\n'` — exactly one Unicode scalar.
    fn char_lit(&mut self, start: usize) -> Option<Token> {
        self.i += 1; // opening quote
        if self.i >= self.chars.len() {
            return None;
        }
        let mut ch = self.at(self.i);
        if ch == '\\' {
            let esc = self.at(self.i + 1);
            if let Some(&(_, decoded)) = Syntax::ESCAPES.iter().find(|&&(e, _)| e == esc) {
                ch = decoded;
                self.i += 2;
            } else {
                self.diags.push(Diagnostic::error(
                    "E0001",
                    format!("`\\{}` isn't an escape Jet knows", esc),
                    "inside a character literal, `\\` starts an escape: `\\n`, `\\t`, `\\'`, `\\\\`"
                        .to_string(),
                    "use a supported escape or a plain character".to_string(),
                    Some(Span::new(self.pos(self.i), self.pos(self.i + 2))),
                ));
                self.i += 2;
                ch = '?';
            }
        } else {
            self.i += 1;
        }
        if self.at(self.i) != '\'' {
            self.diags.push(Diagnostic::error(
                "E0002",
                "a character literal must be exactly one character".to_string(),
                "write `'x'` with a single character between the quotes".to_string(),
                "use a String for longer text".to_string(),
                Some(Span::new(start, self.pos(self.i))),
            ));
            while self.i < self.chars.len() && self.at(self.i) != '\'' {
                self.i += 1;
            }
        }
        if self.at(self.i) == '\'' {
            self.i += 1;
        }
        Some(Token {
            kind: TokKind::Char(ch),
            span: Span::new(start, self.pos(self.i)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::lex_raw;
    use crate::Lexer::{StrTokPart, TokKind};

    #[test]
    fn raw_foreign_body_marker_does_not_capture_a_later_function() {
        let source = r#"#FFI(c) fn foreign() => Int {
    """int64_t foreign(void) { return 1; }"""
}

fn ordinary(value: Int) {
    text :: """
value={value}
"""
}"#;
        let (tokens, diagnostics) = lex_raw(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let strings = tokens
            .iter()
            .filter_map(|token| match &token.kind {
                TokKind::Str(parts) => Some(parts),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(strings.len(), 2);
        assert!(matches!(strings[0].as_slice(), [StrTokPart::Lit(_)]));
        assert!(strings[1]
            .iter()
            .any(|part| matches!(part, StrTokPart::Interp(_))), "{strings:?}");
    }

    #[test]
    fn grouped_ffi_marker_keeps_foreign_body_raw() {
        let source = r#"#[Unsafe("scalar registers"), FFI(asm)]
fn add(a: Int, b: Int) => Int {
    """add {a}, {b} ; -> return"""
}"#;
        let (tokens, diagnostics) = lex_raw(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let body = tokens.iter().find_map(|token| match &token.kind {
            TokKind::Str(parts)
                if matches!(parts.as_slice(), [StrTokPart::Lit(text)] if text.contains("-> return")) =>
            {
                Some(parts)
            }
            _ => None,
        });
        assert!(body.is_some(), "{tokens:?}");
    }

    #[test]
    fn adjacent_ffi_markers_keep_foreign_body_raw_in_both_orders() {
        for source in [
            "#Unsafe(\"registers\") #FFI(asm) fn add() { \"\"\"add {a}, {b}\"\"\" }",
            "#FFI(asm) #Unsafe(\"registers\") fn add() { \"\"\"add {a}, {b}\"\"\" }",
        ] {
            let (tokens, diagnostics) = lex_raw(source);
            assert!(diagnostics.is_empty(), "{diagnostics:?}");
            assert!(tokens.iter().any(|token| matches!(
                &token.kind,
                TokKind::Str(parts)
                    if matches!(parts.as_slice(), [StrTokPart::Lit(text)] if text == "add {a}, {b}")
            )), "{tokens:?}");
        }
    }

    #[test]
    fn grouped_ffi_argument_name_does_not_make_an_ordinary_body_raw() {
        let source =
            "#[Meta(category: FFI(asm)), Task] fn work() { text :: \"\"\"\nvalue={value}\n\"\"\" }";
        let (tokens, diagnostics) = lex_raw(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        assert!(
            tokens.iter().any(|token| matches!(
                &token.kind,
                TokKind::Str(parts)
                    if parts.iter().any(|part| matches!(part, StrTokPart::Interp(_)))
            )),
            "{tokens:?}"
        );
    }
}
