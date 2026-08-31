//! String-literal scanning: plain single-quote (`"…"`) and triple-quote
//! (`"""…"""`) literals own the ordinary escapes (S20) and `{expr}`
//! interpolation (S8); typed-head bodies hand backslashes to their head grammar
//! (D-BOUND-RAW1).

use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;

use super::Lexer;
use super::Scan::{lex_raw_at_depth, lex_raw_generated_at_depth};
use super::Tokens::{StrTokPart, TokKind, Token};

impl<'a> Lexer<'a> {
    /// D-FFI-RAWBODY1=A: unlike every ordinary Jet string, the triple-quoted
    /// payload of `#FFI(<lang>) fn` is an opaque byte-for-byte foreign-source
    /// carrier. Braces, backslashes, quotes, and indentation have no Jet
    /// meaning here.
    pub(super) fn raw_foreign_string(&mut self, start: usize) -> Option<Token> {
        let content_start = self.i + 3;
        let mut close = content_start;
        while close < self.chars.len()
            && !(self.at(close) == '"' && self.at(close + 1) == '"' && self.at(close + 2) == '"')
        {
            close += 1;
        }
        if close >= self.chars.len() {
            self.diags.push(Diagnostic::error(
                "E0002",
                "this raw foreign body never gets a closing `\"\"\"`".to_string(),
                "a `#FFI` body preserves everything between its opening and closing `\"\"\"`"
                    .to_string(),
                "add a closing `\"\"\"` after the foreign source".to_string(),
                Some(Span::new(start, self.end)),
            ));
            self.i = self.chars.len();
            return None;
        }
        let begin_byte = self.pos(content_start);
        let close_byte = self.pos(close);
        let source = self.src[begin_byte..close_byte].to_string();
        self.i = close + 3;
        Some(Token {
            kind: TokKind::Str(vec![StrTokPart::Lit(source)]),
            span: Span::new(start, self.pos(self.i)),
        })
    }

    /// Lex a string literal. Plain strings own the four-entry escape table
    /// (S20); a typed-head body leaves backslashes for its head grammar
    /// (D-BOUND-RAW1). D-BYTELIT1=B lets a `[U8]` body decode `\xNN` into a
    /// byte part. Both forms keep literal braces and `{expr}`
    /// interpolation (S8). Interpolated expressions are lexed in place so
    /// their tokens carry real source spans.
    pub(super) fn string(
        &mut self,
        start: usize,
        raw_head: bool,
        byte_head: bool,
    ) -> Option<Token> {
        self.i += 1; // opening quote
        let mut parts: Vec<StrTokPart> = Vec::new();
        let mut lit = String::new();
        let mut closed = false;

        while self.i < self.chars.len() {
            let ch = self.at(self.i);
            match ch {
                '"' => {
                    closed = true;
                    self.i += 1;
                    break;
                }
                '\n' => break,
                '\\' => {
                    if raw_head {
                        lit.push('\\');
                        if self.at(self.i + 1) == '"' {
                            // Keep the ordinary quote-delimiter rule: a quote
                            // preceded by a slash is part of the body. RAW1
                            // preserves both source characters instead of
                            // decoding the pair.
                            lit.push('"');
                            self.i += 2;
                        } else {
                            self.i += 1;
                        }
                    } else if byte_head && self.at(self.i + 1) == 'x' {
                        let Some(high) = hex_digit(self.at(self.i + 2)) else {
                            self.invalid_byte_escape(self.i);
                            self.i += 2;
                            continue;
                        };
                        let Some(low) = hex_digit(self.at(self.i + 3)) else {
                            self.invalid_byte_escape(self.i);
                            self.i += 2;
                            continue;
                        };
                        if !lit.is_empty() {
                            parts.push(StrTokPart::Lit(std::mem::take(&mut lit)));
                        }
                        parts.push(StrTokPart::Byte((high << 4) | low));
                        self.i += 4;
                    } else {
                        let esc = self.at(self.i + 1);
                        if let Some(&(_, decoded)) =
                            Syntax::ESCAPES.iter().find(|&&(e, _)| e == esc)
                        {
                            lit.push(decoded);
                            self.i += 2;
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E0001",
                                format!("`\\{}` isn't an escape Jet knows", esc),
                                "inside quoted text, `\\` starts an escape: `\\n` (new line), `\\t` (tab), `\\\"` (quote), `\\\\` (backslash)".to_string(),
                                "write `\\\\` for a real backslash".to_string(),
                                Some(Span::new(self.pos(self.i), self.pos(self.i + 2))),
                            ));
                            self.i += 2;
                        }
                    }
                }
                '{' if self.at(self.i + 1) == '{' => {
                    lit.push('{');
                    self.i += 2;
                }
                '}' if self.at(self.i + 1) == '}' => {
                    lit.push('}');
                    self.i += 2;
                }
                // A backslash-dollar-brace sequence is data intended for a
                // downstream template language (for example JS `${...}`).
                // The first two backslashes have already decoded to one
                // literal backslash, so keep the balanced braced payload out
                // of Jet's own interpolation parser.
                '{' if lit.ends_with("\\$") => {
                    lit.push('{');
                    self.i += 1;
                    let mut depth = 1usize;
                    while self.i < self.chars.len() && depth > 0 {
                        let value = self.at(self.i);
                        lit.push(value);
                        self.i += 1;
                        match value {
                            '{' => depth += 1,
                            '}' => depth -= 1,
                            _ => {}
                        }
                    }
                }
                '}' => {
                    self.diags.push(Diagnostic::error(
                        "E0001",
                        "a lone `}` inside quoted text".to_string(),
                        "inside quoted text, `{` and `}` mark an interpolated value, so a literal brace is doubled".to_string(),
                        "write `}}` to print a `}`".to_string(),
                        Some(Span::new(self.pos(self.i), self.pos(self.i + 1))),
                    ));
                    self.i += 1;
                }
                '{' => {
                    let open_pos = self.pos(self.i);
                    self.i += 1;
                    // Find the matching `}`, respecting nested quotes.
                    let expr_start = self.i;
                    let mut depth = 1usize;
                    let mut in_quote = false;
                    while self.i < self.chars.len() {
                        let c2 = self.at(self.i);
                        if in_quote {
                            if c2 == '\\' {
                                self.i += 1;
                            } else if c2 == '"' {
                                in_quote = false;
                            }
                        } else {
                            match c2 {
                                '"' => in_quote = true,
                                '{' => depth += 1,
                                '}' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        break;
                                    }
                                }
                                '\n' => break,
                                _ => {}
                            }
                        }
                        self.i += 1;
                    }
                    if depth != 0 || self.at(self.i) != '}' {
                        self.diags.push(Diagnostic::error(
                            "E0002",
                            "this `{` never gets a matching `}`".to_string(),
                            "`{` inside quoted text starts an interpolated value and needs a closing `}` before the text ends".to_string(),
                            "add a `}` after the value, or write `{{` for a literal brace".to_string(),
                            Some(Span::new(open_pos, self.pos(self.i))),
                        ));
                        // Skip to the end of the line; one error is enough.
                        while self.i < self.chars.len() && self.at(self.i) != '\n' {
                            self.i += 1;
                        }
                        return None;
                    }
                    let inner_start_byte = self.pos(expr_start);
                    let inner_end_byte = self.pos(self.i);
                    self.i += 1; // closing }
                    let inner = &self.src[inner_start_byte..inner_end_byte];
                    if inner.trim().is_empty() {
                        self.diags.push(Diagnostic::error(
                            "E0003",
                            "there's nothing inside this `{ }` to show".to_string(),
                            "interpolation puts a value into the text, so the braces need a value"
                                .to_string(),
                            "put a value inside, like `{name}`, or write `{{}}` for literal braces"
                                .to_string(),
                            Some(Span::new(open_pos, self.pos(self.i))),
                        ));
                        continue;
                    }
                    if !lit.is_empty() {
                        parts.push(StrTokPart::Lit(std::mem::take(&mut lit)));
                    }
                    parts.push(StrTokPart::Interp(
                        self.lex_interpolation(
                            inner_start_byte,
                            inner_end_byte,
                            Span::new(open_pos, self.pos(self.i)),
                        ),
                    ));
                }
                _ => {
                    lit.push(ch);
                    self.i += 1;
                }
            }
        }

        if !closed {
            self.diags.push(Diagnostic::error(
                "E0002",
                "this text never gets a closing quote".to_string(),
                "a piece of text must start and end with a `\"` on the same line".to_string(),
                "add a closing `\"` before the end of the line".to_string(),
                Some(Span::new(start, self.pos(self.i))),
            ));
            return None;
        }
        if !lit.is_empty() || parts.is_empty() {
            parts.push(StrTokPart::Lit(lit));
        }
        Some(Token {
            kind: TokKind::Str(parts),
            span: Span::new(start, self.pos(self.i)),
        })
    }

    /// S70 (D-SG5): `"""…"""` multi-line string. The line break right after the
    /// opening `"""` is dropped, the line break before the closing `"""` is
    /// dropped, and the closing `"""`'s indentation is stripped from every line.
    /// Plain-string escapes (S20) and `{interp}` (S8) stay active. A typed-head
    /// body leaves backslashes literal for its head grammar (D-BOUND-RAW1),
    /// except that D-BYTELIT1=B decodes `\xNN` in a `[U8]` body.
    /// The processed text is stored as ordinary [`StrTokPart`]s;
    /// `jet fmt` re-derives the triple-quoted shape from the span.
    pub(super) fn triple_string(
        &mut self,
        start: usize,
        raw_head: bool,
        byte_head: bool,
    ) -> Option<Token> {
        let open_end = self.i + 3; // char index just past the opening `"""`

        // Pass 1: locate the closing `"""`, skipping `\`-escapes and the
        // contents of `{ … }` interpolations (so a `"""` inside an interpolated
        // expression doesn't close the literal).
        let mut j = open_end;
        let mut close_at: Option<usize> = None;
        let mut depth = 0usize;
        while j < self.chars.len() {
            let c = self.at(j);
            if depth > 0 {
                match c {
                    '{' => depth += 1,
                    '}' => depth -= 1,
                    '"' => {
                        j += 1;
                        while j < self.chars.len() && self.at(j) != '"' {
                            if self.at(j) == '\\' {
                                j += 1;
                            }
                            j += 1;
                        }
                    }
                    _ => {}
                }
                j += 1;
                continue;
            }
            match c {
                '\\' if !raw_head => j += 2,
                '\\' if self.at(j + 1) == '"' => j += 2,
                '\\' => j += 1,
                '{' if self.at(j + 1) != '{' => {
                    depth += 1;
                    j += 1;
                }
                '{' => j += 2, // `{{` literal brace
                '"' if self.at(j + 1) == '"' && self.at(j + 2) == '"' => {
                    close_at = Some(j);
                    break;
                }
                _ => j += 1,
            }
        }

        let close = match close_at {
            Some(c) => c,
            None => {
                self.diags.push(Diagnostic::error(
                    "E0002",
                    "this multi-line text never gets a closing `\"\"\"`".to_string(),
                    "a multi-line string starts and ends with `\"\"\"`".to_string(),
                    "add a closing `\"\"\"` on its own line".to_string(),
                    Some(Span::new(start, self.pos(self.chars.len()))),
                ));
                self.i = self.chars.len();
                return None;
            }
        };

        // The closing delimiter's indentation: the whitespace from the start of
        // its line up to the `"""`. That exact prefix is stripped per line.
        let mut close_line_start = close;
        while close_line_start > open_end && self.at(close_line_start - 1) != '\n' {
            close_line_start -= 1;
        }
        let indent = if (close_line_start..close).all(|k| matches!(self.at(k), ' ' | '\t')) {
            close - close_line_start
        } else {
            0
        };

        // Drop the line break right after the opening `"""`.
        let mut content_begin = open_end;
        {
            let mut k = open_end;
            while k < close && self.at(k) != '\n' {
                k += 1;
            }
            if k < close && self.at(k) == '\n' {
                content_begin = k + 1;
            }
        }
        // Drop the line break right before the closing delimiter's line.
        let mut content_end = close_line_start;
        if content_end > content_begin && self.at(content_end - 1) == '\n' {
            content_end -= 1;
        }
        if content_end < content_begin {
            content_end = content_begin;
        }

        // Pass 2: build the parts, stripping the closing indentation at the
        // start of every line.
        let mut parts: Vec<StrTokPart> = Vec::new();
        let mut lit = String::new();
        let mut k = content_begin;
        let mut at_line_start = true;
        while k < content_end {
            if at_line_start {
                let mut stripped = 0;
                while stripped < indent && k < content_end && matches!(self.at(k), ' ' | '\t') {
                    k += 1;
                    stripped += 1;
                }
                at_line_start = false;
            }
            let ch = self.at(k);
            match ch {
                '\n' => {
                    lit.push('\n');
                    k += 1;
                    at_line_start = true;
                }
                '\\' => {
                    if raw_head {
                        lit.push('\\');
                        if self.at(k + 1) == '"' {
                            // As in an ordinary triple string, a quoted
                            // delimiter is not a closing delimiter. Preserve
                            // the slash and quote as raw body text.
                            lit.push('"');
                            k += 2;
                        } else {
                            k += 1;
                        }
                    } else if byte_head && self.at(k + 1) == 'x' {
                        let Some(high) = hex_digit(self.at(k + 2)) else {
                            self.invalid_byte_escape(k);
                            k += 2;
                            continue;
                        };
                        let Some(low) = hex_digit(self.at(k + 3)) else {
                            self.invalid_byte_escape(k);
                            k += 2;
                            continue;
                        };
                        if !lit.is_empty() {
                            parts.push(StrTokPart::Lit(std::mem::take(&mut lit)));
                        }
                        parts.push(StrTokPart::Byte((high << 4) | low));
                        k += 4;
                    } else {
                        let esc = self.at(k + 1);
                        if let Some(&(_, decoded)) =
                            Syntax::ESCAPES.iter().find(|&&(e, _)| e == esc)
                        {
                            lit.push(decoded);
                        } else {
                            self.diags.push(Diagnostic::error(
                                "E0001",
                                format!("`\\{}` isn't an escape Jet knows", esc),
                                "inside quoted text, `\\` starts an escape: `\\n` (new line), `\\t` (tab), `\\\"` (quote), `\\\\` (backslash)".to_string(),
                                "write `\\\\` for a real backslash".to_string(),
                                Some(Span::new(self.pos(k), self.pos(k + 2))),
                            ));
                        }
                        k += 2;
                    }
                }
                '{' if self.at(k + 1) == '{' => {
                    lit.push('{');
                    k += 2;
                }
                '}' if self.at(k + 1) == '}' => {
                    lit.push('}');
                    k += 2;
                }
                '{' if lit.ends_with("\\$") => {
                    lit.push('{');
                    k += 1;
                    let mut depth = 1usize;
                    while k < content_end && depth > 0 {
                        let value = self.at(k);
                        lit.push(value);
                        k += 1;
                        match value {
                            '{' => depth += 1,
                            '}' => depth -= 1,
                            _ => {}
                        }
                    }
                }
                '}' => {
                    self.diags.push(Diagnostic::error(
                        "E0001",
                        "a lone `}` inside quoted text".to_string(),
                        "inside quoted text, `{` and `}` mark an interpolated value, so a literal brace is doubled".to_string(),
                        "write `}}` to print a `}`".to_string(),
                        Some(Span::new(self.pos(k), self.pos(k + 1))),
                    ));
                    k += 1;
                }
                '{' => {
                    let open_pos = self.pos(k);
                    k += 1;
                    let expr_start = k;
                    let mut bdepth = 1usize;
                    let mut in_quote = false;
                    while k < content_end {
                        let c2 = self.at(k);
                        if in_quote {
                            if c2 == '\\' {
                                k += 1;
                            } else if c2 == '"' {
                                in_quote = false;
                            }
                        } else {
                            match c2 {
                                '"' => in_quote = true,
                                '{' => bdepth += 1,
                                '}' => {
                                    bdepth -= 1;
                                    if bdepth == 0 {
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                        k += 1;
                    }
                    if bdepth != 0 || k >= content_end || self.at(k) != '}' {
                        self.diags.push(Diagnostic::error(
                            "E0002",
                            "this `{` never gets a matching `}`".to_string(),
                            "`{` inside quoted text starts an interpolated value and needs a closing `}` before the text ends".to_string(),
                            "add a `}` after the value, or write `{{` for a literal brace".to_string(),
                            Some(Span::new(open_pos, self.pos(k))),
                        ));
                        self.i = close + 3;
                        return None;
                    }
                    let inner_start_byte = self.pos(expr_start);
                    let inner_end_byte = self.pos(k);
                    k += 1; // closing `}`
                    let inner = &self.src[inner_start_byte..inner_end_byte];
                    if inner.trim().is_empty() {
                        self.diags.push(Diagnostic::error(
                            "E0003",
                            "there's nothing inside this `{ }` to show".to_string(),
                            "interpolation puts a value into the text, so the braces need a value"
                                .to_string(),
                            "put a value inside, like `{name}`, or write `{{}}` for literal braces"
                                .to_string(),
                            Some(Span::new(open_pos, self.pos(k))),
                        ));
                        continue;
                    }
                    if !lit.is_empty() {
                        parts.push(StrTokPart::Lit(std::mem::take(&mut lit)));
                    }
                    parts.push(StrTokPart::Interp(
                        self.lex_interpolation(
                            inner_start_byte,
                            inner_end_byte,
                            Span::new(open_pos, self.pos(k)),
                        ),
                    ));
                }
                _ => {
                    lit.push(ch);
                    k += 1;
                }
            }
        }

        self.i = close + 3; // consume the closing `"""`
        if !lit.is_empty() || parts.is_empty() {
            parts.push(StrTokPart::Lit(lit));
        }
        Some(Token {
            kind: TokKind::Str(parts),
            span: Span::new(start, self.pos(self.i)),
        })
    }

    fn lex_interpolation(
        &mut self,
        inner_start_byte: usize,
        inner_end_byte: usize,
        span: Span,
    ) -> Vec<Token> {
        let next_depth = self.interpolation_depth.saturating_add(1);
        if self.interpolation_depth >= crate::Diagnostics::MAX_SOURCE_NESTING {
            self.diags
                .push(Diagnostic::source_nesting_exceeded(next_depth, span));
            return Vec::new();
        }

        let inner = &self.src[inner_start_byte..inner_end_byte];
        let (mut inner_toks, inner_diags) = if self.allow_reserved_identifiers {
            lex_raw_generated_at_depth(inner, next_depth)
        } else {
            lex_raw_at_depth(inner, next_depth)
        };
        for t in &mut inner_toks {
            t.span = Span::new(
                t.span.start + inner_start_byte,
                t.span.end + inner_start_byte,
            );
        }
        for mut d in inner_diags {
            if let Some(s) = d.span.as_mut() {
                *s = Span::new(s.start + inner_start_byte, s.end + inner_start_byte);
            }
            self.diags.push(d);
        }
        inner_toks
    }

    fn invalid_byte_escape(&mut self, at: usize) {
        self.diags.push(Diagnostic::error(
            "E0001",
            "`\\x` needs two hexadecimal digits".to_string(),
            "a byte escape writes one byte as `\\xNN`, where both digits are hexadecimal"
                .to_string(),
            "write two hexadecimal digits after `\\x`, for example `\\x00`".to_string(),
            Some(Span::new(self.pos(at), self.pos(at + 4))),
        ));
    }
}

fn hex_digit(ch: char) -> Option<u8> {
    match ch {
        '0'..='9' => Some(ch as u8 - b'0'),
        'a'..='f' => Some(ch as u8 - b'a' + 10),
        'A'..='F' => Some(ch as u8 - b'A' + 10),
        _ => None,
    }
}
