//! Token kinds, the `Token` type, string-part pieces, and the human-readable
//! `describe`/keyword/compound-op tables.

use crate::Diagnostics::Span;
use crate::Syntax;

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
    /// D-TOOL2 (E2-M11): typed hole `todo`.
    KwTodo,
    /// S60 (E2-M16): `pure fn` checked modifier.
    KwPure,
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
    /// D-BIND1 (ratified 2026-06-18): `::` immutable binding sigil (was `val`).
    ColonColon,
    /// D-BIND1 (ratified 2026-06-18): `:=` mutable binding sigil (was `var`).
    ColonEq,
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
    pub fn compound_op(&self) -> Option<crate::AST::BinOp> {
        use crate::AST::BinOp;
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
        TokKind::KwFn => format!("the keyword `{}`", Syntax::KW_FN),
        TokKind::KwPub => format!("the keyword `{}`", Syntax::KW_PUB),
        TokKind::KwIf => format!("the keyword `{}`", Syntax::KW_IF),
        TokKind::KwElse => format!("the keyword `{}`", Syntax::KW_ELSE),
        TokKind::KwWhile => format!("the keyword `{}`", Syntax::FOREIGN_WHILE),
        TokKind::KwFor => format!("the keyword `{}`", Syntax::FOREIGN_FOR),
        TokKind::KwIn => format!("the keyword `{}`", Syntax::KW_IN),
        TokKind::KwSwitch => format!("the keyword `{}`", Syntax::KW_SWITCH),
        TokKind::KwBreak => format!("the keyword `{}`", Syntax::KW_BREAK),
        TokKind::KwContinue => format!("the keyword `{}`", Syntax::KW_CONTINUE),
        TokKind::KwTrue => "`true`".to_string(),
        TokKind::KwFalse => "`false`".to_string(),
        TokKind::KwMutate => format!("the keyword `{}`", Syntax::KW_MUTATE),
        TokKind::KwMove => format!("the keyword `{}`", Syntax::KW_MOVE),
        TokKind::KwView => format!("the keyword `{}`", Syntax::KW_VIEW),
        TokKind::KwStored => format!("the keyword `{}`", Syntax::KW_STORED),
        TokKind::KwStruct => format!("the keyword `{}`", Syntax::KW_STRUCT),
        TokKind::KwEnum => format!("the keyword `{}`", Syntax::KW_ENUM),
        TokKind::KwImpl => format!("the keyword `{}`", Syntax::KW_IMPL),
        TokKind::KwTrait => format!("the keyword `{}`", Syntax::KW_TRAIT),
        TokKind::KwDerive => format!("the keyword `{}`", Syntax::KW_DERIVE),
        TokKind::KwSelf => format!("the keyword `{}`", Syntax::KW_SELF),
        TokKind::KwNull => format!("the keyword `{}`", Syntax::LIT_NULL),
        TokKind::KwOk => format!("the keyword `{}`", Syntax::LIT_OK),
        TokKind::KwErr => format!("the keyword `{}`", Syntax::LIT_ERR),
        TokKind::KwIt => format!("the keyword `{}`", Syntax::KW_IT),
        TokKind::KwConst => format!("the keyword `{}`", Syntax::KW_CONST),
        TokKind::KwComptime => format!("the keyword `{}`", Syntax::KW_COMPTIME),
        TokKind::KwReturn => format!("the keyword `{}`", Syntax::KW_RETURN),
        TokKind::KwLoop => format!("the keyword `{}`", Syntax::KW_LOOP),
        TokKind::KwUnsafe => format!("the keyword `{}`", Syntax::KW_UNSAFE),
        TokKind::KwUse => format!("the keyword `{}`", Syntax::KW_USE),
        TokKind::KwExtern => format!("the keyword `{}`", Syntax::KW_EXTERN),
        TokKind::KwModule => format!("the keyword `{}`", Syntax::KW_MODULE),
        TokKind::KwTest => format!("the keyword `{}`", Syntax::KW_TEST),
        TokKind::KwTodo => format!("the keyword `{}`", Syntax::KW_TODO),
        TokKind::KwPure => format!("the keyword `{}`", Syntax::KW_PURE),
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
        TokKind::ColonColon => format!("`{}`", Syntax::SIGIL_BIND_IMMUT),
        TokKind::ColonEq => format!("`{}`", Syntax::SIGIL_BIND_MUT),
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
