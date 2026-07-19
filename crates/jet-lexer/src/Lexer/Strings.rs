//! String-literal scanning: single-quote (`"…"`) and triple-quote (`"""…"""`)
//! literals, with escapes (S20) and `{expr}` interpolation (S8).

use crate::Diagnostics::{Diagnostic, Span};
use crate::Syntax;

use super::Lexer;
use super::Scan::{lex_raw, lex_raw_generated};
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

    /// Lex a string literal: escapes (S20), `{{`/`}}` literal braces (S20),
    /// and `{expr}` interpolation (S8). Interpolated expressions are lexed
    /// in place so their tokens carry real source spans.
    pub(super) fn string(&mut self, start: usize) -> Option<Token> {
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
                    let esc = self.at(self.i + 1);
                    if let Some(&(_, decoded)) = Syntax::ESCAPES.iter().find(|&&(e, _)| e == esc) {
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
                '{' if self.at(self.i + 1) == '{' => {
                    lit.push('{');
                    self.i += 2;
                }
                '}' if self.at(self.i + 1) == '}' => {
                    lit.push('}');
                    self.i += 2;
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
                    // Lex the inner expression; shift spans to absolute.
                    let (mut inner_toks, inner_diags) = if self.allow_reserved_identifiers {
                        lex_raw_generated(inner)
                    } else {
                        lex_raw(inner)
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
                    parts.push(StrTokPart::Interp(inner_toks));
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
    /// Escapes (S20) and `{interp}` (S8) stay active. The processed text is
    /// stored as ordinary [`StrTokPart`]s; `jet fmt` re-derives the triple-quoted
    /// shape from the span.
    pub(super) fn triple_string(&mut self, start: usize) -> Option<Token> {
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
                '\\' => j += 2,
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
                    let esc = self.at(k + 1);
                    if let Some(&(_, decoded)) = Syntax::ESCAPES.iter().find(|&&(e, _)| e == esc) {
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
                '{' if self.at(k + 1) == '{' => {
                    lit.push('{');
                    k += 2;
                }
                '}' if self.at(k + 1) == '}' => {
                    lit.push('}');
                    k += 2;
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
                    let (mut inner_toks, inner_diags) = if self.allow_reserved_identifiers {
                        lex_raw_generated(inner)
                    } else {
                        lex_raw(inner)
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
                    parts.push(StrTokPart::Interp(inner_toks));
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
}
