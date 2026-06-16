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
    KwTrait,
    KwDerive,
    KwSelf,
    KwNull,
    KwOk,
    KwErr,
    KwIt,
    KwConst,
    KwComptime,
    KwReturn,
    KwLoop,
    KwUnsafe,
    KwUse,
    KwExtern,
    KwModule,
    KwTest,
    Ident(String),
    Str(Vec<StrTokPart>),
    Int(i64),
    Float(f64),
    /// S41: `'a'` character literal.
    Char(char),
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Colon,
    Comma,
    Arrow,
    /// S46 (M8): lambda arrow `=>` — distinct from `->`.
    LambdaArrow,
    Semi,
    Eq,
    Dot,
    DotDot,
    At,
    Question,
    /// S71 (D-SG6): `??` fallback operator.
    QuestionQuestion,
    /// S71 (D-SG6): `?.` optional chaining.
    QuestionDot,
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
    /// S76 (2026-06-16): `#` separates the element type and size in `[T#N]`.
    Hash,
    /// S5: `//` through end of line (M6 fmt preserves these).
    LineComment(String),
    /// S5: `/* … */` block comment, nesting allowed (M6 fmt preserves these).
    BlockComment(String),
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
        TokKind::KwTrait => format!("the keyword `{}`", syntax::KW_TRAIT),
        TokKind::KwDerive => format!("the keyword `{}`", syntax::KW_DERIVE),
        TokKind::KwSelf => format!("the keyword `{}`", syntax::KW_SELF),
        TokKind::KwNull => format!("the keyword `{}`", syntax::LIT_NULL),
        TokKind::KwOk => format!("the keyword `{}`", syntax::LIT_OK),
        TokKind::KwErr => format!("the keyword `{}`", syntax::LIT_ERR),
        TokKind::KwIt => format!("the keyword `{}`", syntax::KW_IT),
        TokKind::KwConst => format!("the keyword `{}`", syntax::KW_CONST),
        TokKind::KwComptime => format!("the keyword `{}`", syntax::KW_COMPTIME),
        TokKind::KwReturn => format!("the keyword `{}`", syntax::KW_RETURN),
        TokKind::KwLoop => format!("the keyword `{}`", syntax::KW_LOOP),
        TokKind::KwUnsafe => format!("the keyword `{}`", syntax::KW_UNSAFE),
        TokKind::KwUse => format!("the keyword `{}`", syntax::KW_USE),
        TokKind::KwExtern => format!("the keyword `{}`", syntax::KW_EXTERN),
        TokKind::KwModule => format!("the keyword `{}`", syntax::KW_MODULE),
        TokKind::KwTest => format!("the keyword `{}`", syntax::KW_TEST),
        TokKind::Ident(name) => format!("the name `{}`", name),
        TokKind::Str(_) => "a piece of quoted text".to_string(),
        TokKind::Int(_) => "a number".to_string(),
        TokKind::Float(_) => "a decimal number".to_string(),
        TokKind::Char(_) => "a character".to_string(),
        TokKind::LParen => "`(`".to_string(),
        TokKind::RParen => "`)`".to_string(),
        TokKind::LBrace => "`{`".to_string(),
        TokKind::RBrace => "`}`".to_string(),
        TokKind::LBracket => "`[`".to_string(),
        TokKind::RBracket => "`]`".to_string(),
        TokKind::Colon => "`:`".to_string(),
        TokKind::Comma => "`,`".to_string(),
        TokKind::Arrow => "`->`".to_string(),
        TokKind::LambdaArrow => "`=>`".to_string(),
        TokKind::Semi => "`;`".to_string(),
        TokKind::Eq => "`=`".to_string(),
        TokKind::Dot => "`.`".to_string(),
        TokKind::DotDot => "`..`".to_string(),
        TokKind::At => "`@`".to_string(),
        TokKind::Question => "`?`".to_string(),
        TokKind::QuestionQuestion => "`??`".to_string(),
        TokKind::QuestionDot => "`?.`".to_string(),
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
        TokKind::Hash => "`#`".to_string(),
        TokKind::LineComment(_) => "a comment".to_string(),
        TokKind::BlockComment(_) => "a comment".to_string(),
        TokKind::Eof => "the end of the file".to_string(),
    }
}

/// True when this token is comment trivia (not code).
pub fn is_comment(kind: &TokKind) -> bool {
    matches!(kind, TokKind::LineComment(_) | TokKind::BlockComment(_))
}

/// Drop comment tokens; the parser and sema work on code tokens only.
pub fn without_comments(toks: &[Token]) -> Vec<Token> {
    toks.iter()
        .filter(|t| !is_comment(&t.kind))
        .cloned()
        .collect()
}

/// Collect comments (`//` and `/* … */`) in source order (for fmt).
pub fn comments(toks: &[Token]) -> Vec<Token> {
    toks.iter()
        .filter(|t| is_comment(&t.kind))
        .cloned()
        .collect()
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
        s if s == syntax::KW_TRAIT => Some(TokKind::KwTrait),
        s if s == syntax::KW_DERIVE => Some(TokKind::KwDerive),
        s if s == syntax::KW_SELF => Some(TokKind::KwSelf),
        s if s == syntax::LIT_NULL => Some(TokKind::KwNull),
        s if s == syntax::LIT_OK => Some(TokKind::KwOk),
        s if s == syntax::LIT_ERR => Some(TokKind::KwErr),
        s if s == syntax::KW_IT => Some(TokKind::KwIt),
        s if s == syntax::KW_CONST => Some(TokKind::KwConst),
        s if s == syntax::KW_COMPTIME => Some(TokKind::KwComptime),
        s if s == syntax::KW_RETURN => Some(TokKind::KwReturn),
        s if s == syntax::KW_LOOP => Some(TokKind::KwLoop),
        s if s == syntax::KW_UNSAFE => Some(TokKind::KwUnsafe),
        s if s == syntax::KW_USE => Some(TokKind::KwUse),
        s if s == syntax::KW_EXTERN => Some(TokKind::KwExtern),
        s if s == syntax::KW_MODULE => Some(TokKind::KwModule),
        s if s == syntax::KW_TEST => Some(TokKind::KwTest),
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
                ':' => toks.push(simple(self, TokKind::Colon, 1)),
                ',' => toks.push(simple(self, TokKind::Comma, 1)),
                ';' => toks.push(simple(self, TokKind::Semi, 1)),
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
                    if ch == syntax::DIGIT_SEPARATOR {
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
            } else if ch == syntax::DIGIT_SEPARATOR {
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
                    if let Some(&(_, decoded)) = syntax::ESCAPES.iter().find(|&&(e, _)| e == esc) {
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
                    let (mut inner_toks, inner_diags) = lex(inner);
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
    fn triple_string(&mut self, start: usize) -> Option<Token> {
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
                    if let Some(&(_, decoded)) =
                        syntax::ESCAPES.iter().find(|&&(e, _)| e == esc)
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
                    let (mut inner_toks, inner_diags) = lex(inner);
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

    /// S41: `'a'` or `'\n'` — exactly one Unicode scalar.
    fn char_lit(&mut self, start: usize) -> Option<Token> {
        self.i += 1; // opening quote
        if self.i >= self.chars.len() {
            return None;
        }
        let mut ch = self.at(self.i);
        if ch == '\\' {
            let esc = self.at(self.i + 1);
            if let Some(&(_, decoded)) = syntax::ESCAPES.iter().find(|&&(e, _)| e == esc) {
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
