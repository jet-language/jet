//! Raw scanning: `lex_raw` (no terminator insertion), the main `run` loop, and
//! number/char/digit scanning.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;

use super::Tokens::{TokKind, Token};
use super::{keyword, Lexer};

/// Raw lex with no S6-R terminator insertion. Used for interpolation
/// sub-streams (`{expr}`), which are single expressions and need no terminator.
pub fn lex_raw(src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
    let mut lx = Lexer {
        chars: src.char_indices().collect(),
        end: src.len(),
        src,
        i: 0,
        diags: Vec::new(),
    };
    let mut toks = lx.run();
    toks.push(Token {
        kind: TokKind::Eof,
        span: Span::new(src.len(), src.len()),
    });
    (toks, lx.diags)
}

impl<'a> Lexer<'a> {
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
                ']' => toks.push(simple(self, TokKind::RBracket, 1)),
                // D-BIND1: `:=` mutable binding sigil; `::` retained to lex (retired sigil → E0991).
                ':' if next == ':' => toks.push(simple(self, TokKind::ColonColon, 2)),
                ':' if next == '=' => toks.push(simple(self, TokKind::ColonEq, 2)),
                ':' => toks.push(simple(self, TokKind::Colon, 1)),
                ',' => toks.push(simple(self, TokKind::Comma, 1)),
                ';' => toks.push(simple(self, TokKind::Semi, 1)),
                // D-BIND2: `@=` immutable binding sigil — checked BEFORE bare `@`.
                '@' if next == '=' => toks.push(simple(self, TokKind::AtEq, 2)),
                '@' => toks.push(simple(self, TokKind::At, 1)),
                '#' => toks.push(simple(self, TokKind::Hash, 1)),
                '?' if next == '?' => toks.push(simple(self, TokKind::QuestionQuestion, 2)),
                '?' if next == '.' => toks.push(simple(self, TokKind::QuestionDot, 2)),
                '?' => toks.push(simple(self, TokKind::Question, 1)),
                '.' if next == '.' => toks.push(simple(self, TokKind::DotDot, 2)),
                '.' => toks.push(simple(self, TokKind::Dot, 1)),
                '=' if next == '=' => toks.push(simple(self, TokKind::EqEq, 2)),
                '=' if next == '>' => toks.push(simple(self, TokKind::LambdaArrow, 2)),
                '=' => toks.push(simple(self, TokKind::Eq, 1)),
                '!' if next == '=' => toks.push(simple(self, TokKind::NotEq, 2)),
                '!' => toks.push(simple(self, TokKind::Bang, 1)),
                '+' if next == '=' => toks.push(simple(self, TokKind::PlusEq, 2)),
                '+' => toks.push(simple(self, TokKind::Plus, 1)),
                '-' if next == '>' => toks.push(simple(self, TokKind::Arrow, 2)),
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
                // D-CAP7: `~` is the write/edit capability sigil (prefix). `~` was
                // previously unlexed; `~~` (S83 trait-attach) is not yet implemented.
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
                '"' => {
                    let tok = if next == '"' && next2 == '"' {
                        self.triple_string(start)
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
                        kind: TokKind::Int(0),
                        span,
                    };
                }
                return match i64::from_str_radix(&digits, radix) {
                    Ok(n) => Token {
                        kind: TokKind::Int(n),
                        span,
                    },
                    Err(_) => {
                        self.diags.push(self.too_big(span));
                        Token {
                            kind: TokKind::Int(0),
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
        let span = Span::new(start, self.pos(self.i));
        if is_float {
            // digits '.' digits (with optional exponent) always parses as f64.
            let v: f64 = text.parse().unwrap_or(0.0);
            return Token {
                kind: TokKind::Float(v),
                span,
            };
        }
        match text.parse::<i64>() {
            Ok(n) => Token {
                kind: TokKind::Int(n),
                span,
            },
            Err(_) => {
                self.diags.push(self.too_big(span));
                Token {
                    kind: TokKind::Int(0),
                    span,
                }
            }
        }
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
