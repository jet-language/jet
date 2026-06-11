//! Lexer: source text -> tokens. Every token carries a byte span so that
//! diagnostics anywhere downstream can point at real source.
//!
//! M1: the lexer recovers from errors — it reports every problem it finds
//! in one run instead of stopping at the first.

use crate::diag::{Diagnostic, Span};
use crate::syntax;

/// One piece of a string literal: literal text (escapes already decoded)
/// or an interpolated expression, pre-lexed into its own token stream
/// with spans into the original source (S8).
#[derive(Debug, Clone, PartialEq)]
pub enum StrTokPart {
    Lit(String),
    Interp(Vec<Token>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokKind {
    KwFn,
    KwPub,
    KwVal,
    KwVar,
    KwIf,
    KwElse,
    KwWhile,
    KwFor,
    KwIn,
    KwSwitch,
    KwBreak,
    KwContinue,
    KwTrue,
    KwFalse,
    KwMutate,
    KwMove,
    KwView,
    KwStored,
    KwStruct,
    KwEnum,
    KwImpl,
    KwSelf,
    KwValue,
    KwNull,
    KwConst,
    KwReturn,
    KwLoop,
    KwUnsafe,
    KwImport,
    Ident(String),
    Str(Vec<StrTokPart>),
    Int(i64),
    Float(f64),
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Colon,
    Comma,
    Arrow,
    Semi,
    Eq,
    Dot,
    DotDot,
    At,
    Question,
    // Arithmetic (M1).
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Amp,
    Pipe,
    Caret,
    Shl,
    Shr,
    // Logic & comparison (S13).
    AndAnd,
    OrOr,
    Bang,
    EqEq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
    // Compound assignment (S17).
    PlusEq,
    MinusEq,
    StarEq,
    SlashEq,
    PercentEq,
    AmpEq,
    PipeEq,
    CaretEq,
    ShlEq,
    ShrEq,
    Eof,
}

impl TokKind {
    /// The compound-assignment family, mapped to its base operation.
    pub fn compound_op(&self) -> Option<crate::ast::BinOp> {
        use crate::ast::BinOp;
        match self {
            TokKind::PlusEq => Some(BinOp::Add),
            TokKind::MinusEq => Some(BinOp::Sub),
            TokKind::StarEq => Some(BinOp::Mul),
            TokKind::SlashEq => Some(BinOp::Div),
            TokKind::PercentEq => Some(BinOp::Rem),
            TokKind::AmpEq => Some(BinOp::BitAnd),
            TokKind::PipeEq => Some(BinOp::BitOr),
            TokKind::CaretEq => Some(BinOp::BitXor),
            TokKind::ShlEq => Some(BinOp::Shl),
            TokKind::ShrEq => Some(BinOp::Shr),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokKind,
    pub span: Span,
}

/// A short, human description of a token, for error messages.
/// Never say "token" to a user; say what the thing is.
pub fn describe(kind: &TokKind) -> String {
    match kind {
        TokKind::KwFn => format!("the keyword `{}`", syntax::KW_FN),
        TokKind::KwPub => format!("the keyword `{}`", syntax::KW_PUB),
        TokKind::KwVal => format!("the keyword `{}`", syntax::KW_VAL),
        TokKind::KwVar => format!("the keyword `{}`", syntax::KW_VAR),
        TokKind::KwIf => format!("the keyword `{}`", syntax::KW_IF),
        TokKind::KwElse => format!("the keyword `{}`", syntax::KW_ELSE),
        TokKind::KwWhile => format!("the keyword `{}`", syntax::KW_WHILE),
        TokKind::KwFor => format!("the keyword `{}`", syntax::KW_FOR),
        TokKind::KwIn => format!("the keyword `{}`", syntax::KW_IN),
        TokKind::KwSwitch => format!("the keyword `{}`", syntax::KW_SWITCH),
        TokKind::KwBreak => format!("the keyword `{}`", syntax::KW_BREAK),
        TokKind::KwContinue => format!("the keyword `{}`", syntax::KW_CONTINUE),
        TokKind::KwTrue => "`true`".to_string(),
        TokKind::KwFalse => "`false`".to_string(),
        TokKind::KwMutate => format!("the keyword `{}`", syntax::KW_MUTATE),
        TokKind::KwMove => format!("the keyword `{}`", syntax::KW_MOVE),
        TokKind::KwView => format!("the keyword `{}`", syntax::KW_VIEW),
        TokKind::KwStored => format!("the keyword `{}`", syntax::KW_STORED),
        TokKind::KwStruct => format!("the keyword `{}`", syntax::KW_STRUCT),
        TokKind::KwEnum => format!("the keyword `{}`", syntax::KW_ENUM),
        TokKind::KwImpl => format!("the keyword `{}`", syntax::KW_IMPL),
        TokKind::KwSelf => format!("the keyword `{}`", syntax::KW_SELF),
        TokKind::KwValue => format!("the keyword `{}`", syntax::LIT_VALUE),
        TokKind::KwNull => format!("the keyword `{}`", syntax::LIT_NULL),
        TokKind::KwConst => format!("the keyword `{}`", syntax::KW_CONST),
        TokKind::KwReturn => format!("the keyword `{}`", syntax::KW_RETURN),
        TokKind::KwLoop => format!("the keyword `{}`", syntax::KW_LOOP),
        TokKind::KwUnsafe => format!("the keyword `{}`", syntax::KW_UNSAFE),
        TokKind::KwImport => format!("the keyword `{}`", syntax::KW_IMPORT),
        TokKind::Ident(name) => format!("the name `{}`", name),
        TokKind::Str(_) => "a piece of quoted text".to_string(),
        TokKind::Int(_) => "a number".to_string(),
        TokKind::Float(_) => "a decimal number".to_string(),
        TokKind::LParen => "`(`".to_string(),
        TokKind::RParen => "`)`".to_string(),
        TokKind::LBrace => "`{`".to_string(),
        TokKind::RBrace => "`}`".to_string(),
        TokKind::LBracket => "`[`".to_string(),
        TokKind::RBracket => "`]`".to_string(),
        TokKind::Colon => "`:`".to_string(),
        TokKind::Comma => "`,`".to_string(),
        TokKind::Arrow => "`->`".to_string(),
        TokKind::Semi => "`;`".to_string(),
        TokKind::Eq => "`=`".to_string(),
        TokKind::Dot => "`.`".to_string(),
        TokKind::DotDot => "`..`".to_string(),
        TokKind::At => "`@`".to_string(),
        TokKind::Question => "`?`".to_string(),
        TokKind::Plus => "`+`".to_string(),
        TokKind::Minus => "`-`".to_string(),
        TokKind::Star => "`*`".to_string(),
        TokKind::Slash => "`/`".to_string(),
        TokKind::Percent => "`%`".to_string(),
        TokKind::Amp => "`&`".to_string(),
        TokKind::Pipe => "`|`".to_string(),
        TokKind::Caret => "`^`".to_string(),
        TokKind::Shl => "`<<`".to_string(),
        TokKind::Shr => "`>>`".to_string(),
        TokKind::AndAnd => "`&&`".to_string(),
        TokKind::OrOr => "`||`".to_string(),
        TokKind::Bang => "`!`".to_string(),
        TokKind::EqEq => "`==`".to_string(),
        TokKind::NotEq => "`!=`".to_string(),
        TokKind::Lt => "`<`".to_string(),
        TokKind::Gt => "`>`".to_string(),
        TokKind::Le => "`<=`".to_string(),
        TokKind::Ge => "`>=`".to_string(),
        TokKind::PlusEq => "`+=`".to_string(),
        TokKind::MinusEq => "`-=`".to_string(),
        TokKind::StarEq => "`*=`".to_string(),
        TokKind::SlashEq => "`/=`".to_string(),
        TokKind::PercentEq => "`%=`".to_string(),
        TokKind::AmpEq => "`&=`".to_string(),
        TokKind::PipeEq => "`|=`".to_string(),
        TokKind::CaretEq => "`^=`".to_string(),
        TokKind::ShlEq => "`<<=`".to_string(),
        TokKind::ShrEq => "`>>=`".to_string(),
        TokKind::Eof => "the end of the file".to_string(),
    }
}

fn keyword(name: &str) -> Option<TokKind> {
    match name {
        s if s == syntax::KW_FN => Some(TokKind::KwFn),
        s if s == syntax::KW_PUB => Some(TokKind::KwPub),
        s if s == syntax::KW_VAL => Some(TokKind::KwVal),
        s if s == syntax::KW_VAR => Some(TokKind::KwVar),
        s if s == syntax::KW_IF => Some(TokKind::KwIf),
        s if s == syntax::KW_ELSE => Some(TokKind::KwElse),
        s if s == syntax::KW_WHILE => Some(TokKind::KwWhile),
        s if s == syntax::KW_FOR => Some(TokKind::KwFor),
        s if s == syntax::KW_IN => Some(TokKind::KwIn),
        s if s == syntax::KW_SWITCH => Some(TokKind::KwSwitch),
        s if s == syntax::KW_BREAK => Some(TokKind::KwBreak),
        s if s == syntax::KW_CONTINUE => Some(TokKind::KwContinue),
        s if s == syntax::LIT_TRUE => Some(TokKind::KwTrue),
        s if s == syntax::LIT_FALSE => Some(TokKind::KwFalse),
        s if s == syntax::KW_MUTATE => Some(TokKind::KwMutate),
        s if s == syntax::KW_MOVE => Some(TokKind::KwMove),
        s if s == syntax::KW_VIEW => Some(TokKind::KwView),
        s if s == syntax::KW_STORED => Some(TokKind::KwStored),
        s if s == syntax::KW_STRUCT => Some(TokKind::KwStruct),
        s if s == syntax::KW_ENUM => Some(TokKind::KwEnum),
        s if s == syntax::KW_IMPL => Some(TokKind::KwImpl),
        s if s == syntax::KW_SELF => Some(TokKind::KwSelf),
        s if s == syntax::LIT_VALUE => Some(TokKind::KwValue),
        s if s == syntax::LIT_NULL => Some(TokKind::KwNull),
        s if s == syntax::KW_CONST => Some(TokKind::KwConst),
        s if s == syntax::KW_RETURN => Some(TokKind::KwReturn),
        s if s == syntax::KW_LOOP => Some(TokKind::KwLoop),
        s if s == syntax::KW_UNSAFE => Some(TokKind::KwUnsafe),
        s if s == syntax::KW_IMPORT => Some(TokKind::KwImport),
        _ => None,
    }
}

struct Lexer<'a> {
    chars: Vec<(usize, char)>,
    end: usize,
    src: &'a str,
    i: usize,
    diags: Vec<Diagnostic>,
}

/// Lex the whole file. Always returns a token stream (ending in Eof) plus
/// every problem found along the way — M1 error recovery.
pub fn lex(src: &str) -> (Vec<Token>, Vec<Diagnostic>) {
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
    fn at(&self, i: usize) -> char {
        if i < self.chars.len() {
            self.chars[i].1
        } else {
            '\0'
        }
    }

    fn pos(&self, i: usize) -> usize {
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

            // Line comments (decision S5).
            if c == '/' && self.at(self.i + 1) == '/' {
                while self.i < self.chars.len() && self.at(self.i) != '\n' {
                    self.i += 1;
                }
                continue;
            }

            let start = self.pos(self.i);
            let mut simple = |lx: &mut Self, kind: TokKind, len: usize| {
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
                ':' => toks.push(simple(self, TokKind::Colon, 1)),
                ',' => toks.push(simple(self, TokKind::Comma, 1)),
                ';' => toks.push(simple(self, TokKind::Semi, 1)),
                '@' => toks.push(simple(self, TokKind::At, 1)),
                '?' => toks.push(simple(self, TokKind::Question, 1)),
                '.' if next == '.' => toks.push(simple(self, TokKind::DotDot, 2)),
                '.' => toks.push(simple(self, TokKind::Dot, 1)),
                '=' if next == '=' => toks.push(simple(self, TokKind::EqEq, 2)),
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
                    if let Some(tok) = self.string(start) {
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
                        "check docs/01-spec.md for what's supported so far".to_string(),
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
    /// `1..10` stays Int DotDot Int: a `.` only starts the decimal part
    /// when a digit follows it.
    fn number(&mut self, start: usize) -> Token {
        let mut text = String::new();
        while self.i < self.chars.len() && self.at(self.i).is_ascii_digit() {
            text.push(self.at(self.i));
            self.i += 1;
        }
        let mut is_float = false;
        if self.at(self.i) == '.' && self.at(self.i + 1).is_ascii_digit() {
            is_float = true;
            text.push('.');
            self.i += 1;
            while self.i < self.chars.len() && self.at(self.i).is_ascii_digit() {
                text.push(self.at(self.i));
                self.i += 1;
            }
        }
        let span = Span::new(start, self.pos(self.i));
        if is_float {
            // digits '.' digits always parses as f64.
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
                self.diags.push(Diagnostic::error(
                    "E0007",
                    "this number is too big".to_string(),
                    "numbers currently top out at 9223372036854775807 (a 64-bit integer)"
                        .to_string(),
                    "use a smaller number".to_string(),
                    Some(span),
                ));
                Token {
                    kind: TokKind::Int(0),
                    span,
                }
            }
        }
    }

    /// Lex a string literal: escapes (S20), `{{`/`}}` literal braces (S20),
    /// and `{expr}` interpolation (S8). Interpolated expressions are lexed
    /// in place so their tokens carry real source spans.
    fn string(&mut self, start: usize) -> Option<Token> {
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
                    if let Some(&(_, decoded)) =
                        syntax::ESCAPES.iter().find(|&&(e, _)| e == esc)
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
                            "interpolation puts a value into the text, so the braces need a value".to_string(),
                            "put a value inside, like `{name}`, or write `{{}}` for literal braces".to_string(),
                            Some(Span::new(open_pos, self.pos(self.i))),
                        ));
                        continue;
                    }
                    if !lit.is_empty() {
                        parts.push(StrTokPart::Lit(std::mem::take(&mut lit)));
                    }
                    // Lex the inner expression; shift spans to absolute.
                    let (mut inner_toks, inner_diags) = lex(inner);
                    for t in &mut inner_toks {
                        t.span =
                            Span::new(t.span.start + inner_start_byte, t.span.end + inner_start_byte);
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
}
