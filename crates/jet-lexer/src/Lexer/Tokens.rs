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
    KwPriv,
    KwIf,
    KwElse,
    KwWhile,
    KwFor,
    KwSwitch,
    KwBreak,
    KwTrue,
    KwFalse,
    KwMutate,
    KwMove,
    /// D-CAP2 (D-MEM1/S4): `copy x` — the one copy verb.
    KwCopy,
    KwStruct,
    KwEnum,
    KwImpl,
    KwTrait,
    /// D-QUAL2: `tag` — marker qualifier declaration keyword.
    KwTag,
    /// D-EFFECT-DECL1=A: package-scoped effect-leaf declaration.
    KwEffect,
    KwDerive,
    KwSelf,
    KwNull,
    KwIt,
    KwConst,
    KwComptime,
    KwReturn,
    KwLoop,
    KwYield,
    KwUnsafe,
    KwUse,
    KwExtern,
    KwModule,
    // D-CASING1 follow-on: `Test`/`Todo`/`Pure` are no longer keyword tokens —
    // they are `#`-markers recognized as `#` + ident in the parser.
    Ident(String),
    Str(Vec<StrTokPart>),
    /// Parsed value plus exact source spelling, including radix prefix,
    /// leading zeroes, separator placement, and digit case.
    Int(i64, String),
    Float(f64),
    /// D-UNITLIT1: a numeric literal immediately followed by an identifier
    /// suffix that isn't a float exponent — `500ms`, `12.50usd`. A NEW,
    /// SEPARATE token kind (not a field added to `Int`/`Float`) so every
    /// existing `TokKind::Int`/`Float` match across the parser is completely
    /// unaffected when no suffix is present. The lexer only carries the
    /// value + suffix text; resolving the suffix against an in-scope
    /// `#UnitFamily` member is sema's job (imports aren't known here).
    UnitNumber {
        /// Exact source digits, retained for policy/config consumers that require
        /// rational normalization without an f64 round-trip.
        raw: String,
        int: Option<i64>,
        float: Option<f64>,
        suffix: String,
    },
    /// S41: `'a'` character literal.
    Char(char),
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    /// D-EACH1=C: open a fenced-name statement expansion.
    FenceOpen,
    /// D-EACH1=C: close a fenced-name statement expansion.
    FenceClose,
    Colon,
    /// D-BIND4: `::` immutable binding sigil.
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
    /// D-RANGE-EXCL1=C: `..<` exclusive / half-open range in loop headers.
    DotDotLt,
    /// D-VARIADIC1: `...` spread/rest sigil — variadic params, call spread, list spread.
    DotDotDot,
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
    /// D-SHAPE-COPY1=A: `~x` is the one copy sigil (supersedes D-CAP2/S4's
    /// `copy` verb; was retired by D-MEM1 as the D-CAP7 write sigil, since
    /// superseded by `&`).
    Tilde,
    /// D-XORSPELL1=A: `a ~| b` is bitwise exclusive-or.
    TildePipe,
    /// D-XORSPELL1=A: `a ~|= b` is exclusive-or-assign.
    TildePipeEq,
    /// Retired external-method connector. Longest-match before `~` so parser
    /// can teach E0325.
    TildeTilde,
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
    /// D-INCR1: prefix/postfix increment `++`.
    PlusPlus,
    MinusEq,
    /// D-INCR1: prefix/postfix decrement `--` (adjacent dashes only — `x - -y` stays two `-`).
    MinusMinus,
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
    /// D-CTMARKER1=C: `$` — comptime splice marker in `emit()` templates.
    Dollar,
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
            TokKind::CaretEq => Some(BinOp::Pow),
            TokKind::TildePipeEq => Some(BinOp::BitXor),
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
        TokKind::KwPriv => format!("the keyword `{}`", Syntax::KW_PRIV),
        TokKind::KwIf => format!("the keyword `{}`", Syntax::KW_IF),
        TokKind::KwElse => format!("the keyword `{}`", Syntax::KW_ELSE),
        TokKind::KwWhile => format!("the keyword `{}`", Syntax::FOREIGN_WHILE),
        TokKind::KwFor => format!("the keyword `{}`", Syntax::FOREIGN_FOR),
        TokKind::KwSwitch => format!("the keyword `{}`", Syntax::KW_SWITCH),
        TokKind::KwBreak => format!("the keyword `{}`", Syntax::KW_BREAK),
        TokKind::KwTrue => "`true`".to_string(),
        TokKind::KwFalse => "`false`".to_string(),
        TokKind::KwMutate => format!("the keyword `{}`", Syntax::KW_MUTATE),
        TokKind::KwMove => format!("the keyword `{}`", Syntax::KW_MOVE),
        TokKind::KwCopy => format!("the keyword `{}`", Syntax::KW_COPY),
        TokKind::KwStruct => format!("the keyword `{}`", Syntax::KW_STRUCT),
        TokKind::KwEnum => format!("the keyword `{}`", Syntax::KW_ENUM),
        TokKind::KwImpl => format!("the keyword `{}`", Syntax::KW_IMPL),
        TokKind::KwTrait => format!("the keyword `{}`", Syntax::KW_TRAIT),
        TokKind::KwTag => format!("the keyword `{}`", Syntax::KW_TAG),
        TokKind::KwEffect => format!("the keyword `{}`", Syntax::KW_EFFECT_DECL),
        TokKind::KwDerive => format!("the keyword `{}`", Syntax::KW_DERIVE),
        TokKind::KwSelf => format!("the keyword `{}`", Syntax::KW_SELF),
        TokKind::KwNull => format!("the keyword `{}`", Syntax::LIT_NULL),
        TokKind::KwIt => format!("the keyword `{}`", Syntax::KW_IT),
        TokKind::KwConst => format!("the keyword `{}`", Syntax::KW_CONST),
        TokKind::KwComptime => format!("the keyword `{}`", Syntax::KW_COMPTIME),
        TokKind::KwReturn => format!("the keyword `{}`", Syntax::KW_RETURN),
        TokKind::KwLoop => format!("the keyword `{}`", Syntax::KW_LOOP),
        TokKind::KwYield => format!("the keyword `{}`", Syntax::KW_YIELD),
        TokKind::KwUnsafe => format!("the keyword `{}`", Syntax::KW_UNSAFE),
        TokKind::KwUse => format!("the keyword `{}`", Syntax::KW_USE),
        TokKind::KwExtern => format!("the keyword `{}`", Syntax::KW_EXTERN),
        TokKind::KwModule => format!("the keyword `{}`", Syntax::KW_MODULE),
        TokKind::Ident(name) => format!("the name `{}`", name),
        TokKind::Str(_) => "a piece of quoted text".to_string(),
        TokKind::Int(..) => "a number".to_string(),
        TokKind::Float(_) => "a decimal number".to_string(),
        TokKind::UnitNumber { .. } => "a number with a unit suffix".to_string(),
        TokKind::Char(_) => "a character".to_string(),
        TokKind::LParen => "`(`".to_string(),
        TokKind::RParen => "`)`".to_string(),
        TokKind::LBrace => "`{`".to_string(),
        TokKind::RBrace => "`}`".to_string(),
        TokKind::LBracket => "`[`".to_string(),
        TokKind::RBracket => "`]`".to_string(),
        TokKind::FenceOpen => format!("`{}`", Syntax::SIGIL_FENCE_OPEN),
        TokKind::FenceClose => format!("`{}`", Syntax::SIGIL_FENCE_CLOSE),
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
        TokKind::DotDotLt => "`..<`".to_string(),
        TokKind::DotDotDot => "`...`".to_string(),
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
        TokKind::Tilde => "`~`".to_string(),
        TokKind::TildePipe => "`~|`".to_string(),
        TokKind::TildePipeEq => "`~|=`".to_string(),
        TokKind::TildeTilde => "`~~`".to_string(),
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
        TokKind::PlusPlus => "`++`".to_string(),
        TokKind::MinusEq => "`-=`".to_string(),
        TokKind::MinusMinus => "`--`".to_string(),
        TokKind::StarEq => "`*=`".to_string(),
        TokKind::SlashEq => "`/=`".to_string(),
        TokKind::PercentEq => "`%=`".to_string(),
        TokKind::AmpEq => "`&=`".to_string(),
        TokKind::PipeEq => "`|=`".to_string(),
        TokKind::CaretEq => "`^=`".to_string(),
        TokKind::ShlEq => "`<<=`".to_string(),
        TokKind::ShrEq => "`>>=`".to_string(),
        TokKind::Hash => "`#`".to_string(),
        TokKind::Dollar => "`$`".to_string(),
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
