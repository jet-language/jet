//! Token kinds, the `Token` type, string-part pieces, and the human-readable
//! `describe`/keyword/compound-op tables.

use crate::Diagnostics::Span;
use crate::Syntax;

/// One piece of a string literal: literal text (escapes already decoded), an
/// explicit byte escape, or an interpolated expression pre-lexed into its own
/// token stream with spans into the original source (S8).
#[derive(Debug, Clone, PartialEq)]
pub enum StrTokPart {
    Lit(String),
    /// D-BYTELIT1=B: `\xNN` inside a `[U8]{ "..." }` body. Keep the byte
    /// before it becomes a Rust/Unicode string so values above ASCII survive.
    Byte(u8),
    Interp(Vec<Token>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokKind {
    KwFn,
    KwPub,
    KwPriv,
    KwIf,
    KwElse,
    /// D-LOOP-IN1=A / D-TIME-IN1=C: source-loop `in`; postfix `.in` is
    /// reclassified as `Ident` by the lexer and accepted by the parser.
    KwIn,
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
    KwUse,
    KwExtern,
    KwModule,
    // D-CASING1 follow-on: `Test`/`Todo`/`Pure` are no longer keyword tokens —
    // they are `#`-markers recognized as `#` + ident in the parser.
    Ident(String),
    /// D-RAWSTR1=A: a backtick-fenced ordinary `String`. The lexer keeps this
    /// token distinct so formatters can choose a lossless raw spelling while
    /// the parser lowers it to the ordinary string AST.
    RawStr(String),
    Str(Vec<StrTokPart>),
    /// Parsed value plus exact source spelling, including radix prefix,
    /// leading zeroes, separator placement, and digit case.
    Int(i64, String),
    /// Decimal source spelling stays beside the parsed value so sema can
    /// adopt an untyped decimal as exact `Decimal` without a binary-float
    /// round trip. Explicit Float contexts still use the parsed value.
    Float(f64, String),
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
    /// D-EACH1=C / D-FENCE2=A: `<:` opens a fenced statement expansion.
    FenceOpen,
    /// D-EACH1=C / D-FENCE2=A: `:>` closes a fenced statement expansion.
    FenceClose,
    Colon,
    /// D-BIND4: `::` immutable binding sigil.
    ColonColon,
    /// D-BIND1 (ratified 2026-06-18; predates the current D-BIND4/D-BIND-BARE1
    /// numbering): `:=` mutable binding sigil (was `var`).
    ColonEq,
    Comma,
    /// D-ARROW-RESPELL1=A: one callable/control arrow, `->`.
    UnifiedArrow,
    /// Retired D-ARROW-UNIFY1 spelling, `=>`.
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
    /// D-FLOORDIV1=A: `a /% b` divides and rounds down.
    SlashPercent,
    Percent,
    /// D-MODSEM1=A: `a %% b` is the truncated remainder.
    PercentPercent,
    Amp,
    Pipe,
    Caret,
    /// D-COPY-WORD1=A: prefix `~x` is the retired copy sigil; it lexes only so
    /// the parser can teach E0941 and its rewrite to `x.copy()`.
    Tilde,
    /// D-SIGIL-ALGEBRA1=B: retired exclusive-or, lexed to teach `bit_xor`.
    TildePipe,
    /// D-SIGIL-ALGEBRA1=B: retired exclusive-or assignment, lexed for E0398.
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
    /// D-CMP3WAY1=B: longest-match three-way comparison operator.
    Compare,
    // Compound assignment (S17).
    PlusEq,
    /// D-INCR1: prefix/postfix increment `++`.
    PlusPlus,
    MinusEq,
    /// D-INCR1: prefix/postfix decrement `--` (adjacent dashes only — `x - -y` stays two `-`).
    MinusMinus,
    StarEq,
    SlashEq,
    /// D-FLOORDIV1=A: `a /%= b` divides in place and rounds down.
    SlashPercentEq,
    PercentEq,
    /// D-MODSEM1=A: `a %%= b` is truncated-remainder-assign.
    PercentPercentEq,
    AmpEq,
    PipeEq,
    CaretEq,
    ShlEq,
    ShrEq,
    /// S76 (2026-06-16): `#` separates the element type and size in `[T#N]`.
    Hash,
    /// D-ONCE-DOLLAR1=B: `$` is the config-surface environment-read token;
    /// the parser keeps it on the E0003 teaching path in ordinary source.
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
            TokKind::SlashPercentEq => Some(BinOp::FloorDiv),
            TokKind::PercentEq => Some(BinOp::Mod),
            TokKind::PercentPercentEq => Some(BinOp::Rem),
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
        TokKind::KwIn => format!("the keyword `{}`", Syntax::KW_IN),
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
        TokKind::KwUse => format!("the keyword `{}`", Syntax::KW_USE),
        TokKind::KwExtern => format!("the keyword `{}`", Syntax::KW_EXTERN),
        TokKind::KwModule => format!("the keyword `{}`", Syntax::KW_MODULE),
        TokKind::Ident(name) => format!("the name `{}`", name),
        TokKind::Str(_) => "a piece of quoted text".to_string(),
        TokKind::RawStr(_) => "a piece of raw text".to_string(),
        TokKind::Int(..) => "a number".to_string(),
        TokKind::Float(..) => "a decimal number".to_string(),
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
        TokKind::UnifiedArrow => "`->`".to_string(),
        TokKind::LambdaArrow => "`=>`".to_string(),
        // Users never type `;`: the lexer inserts it at a line end (S6-R), and
        // an explicit one is retired (D-SEMI1=A). Name what the reader sees.
        TokKind::Semi => "the end of the line".to_string(),
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
        TokKind::SlashPercent => "`/%`".to_string(),
        TokKind::Percent => "`%`".to_string(),
        TokKind::PercentPercent => "`%%`".to_string(),
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
        TokKind::Compare => "`<=>`".to_string(),
        TokKind::PlusEq => "`+=`".to_string(),
        TokKind::PlusPlus => "`++`".to_string(),
        TokKind::MinusEq => "`-=`".to_string(),
        TokKind::MinusMinus => "`--`".to_string(),
        TokKind::StarEq => "`*=`".to_string(),
        TokKind::SlashEq => "`/=`".to_string(),
        TokKind::SlashPercentEq => "`/%=`".to_string(),
        TokKind::PercentEq => "`%=`".to_string(),
        TokKind::PercentPercentEq => "`%%=`".to_string(),
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

/// Compiler-internal Core demand from the existing canonical token stream.
/// String payload and comments are not identifiers; interpolation expressions
/// already carry their own canonical tokens and are visited without re-lexing.
pub fn core_identifier_demand(tokens: &[Token]) -> jet_foundation::AST::CoreIdentifierDemand {
    use jet_foundation::{CoreModuleExports, Prelude};

    fn collect(
        tokens: &[Token],
        bare_names: &mut [bool],
        prelude_entries: &[Prelude::Entry],
        demand: &mut jet_foundation::AST::CoreIdentifierDemand,
    ) {
        for (index, token) in tokens.iter().enumerate() {
            match &token.kind {
                TokKind::Ident(name) => {
                    demand.option_or_err |= name == "or_err";
                    // Source-backed receiver candidates schedule their owner;
                    // Sema still validates the canonical primitive identity.
                    // Plain names and literal/comment payloads do not demand it.
                    if name == "abs"
                        && tokens[..index].iter().rev().find(|token| !is_comment(&token.kind))
                            .is_some_and(|token| matches!(token.kind, TokKind::Dot | TokKind::QuestionDot))
                        && !demand.bare_modules.contains(&"core.math")
                    {
                        demand.bare_modules.push("core.math");
                    }
                    for (index, bare) in CoreModuleExports::CORE_BARE_SOURCE_MODULES.iter().enumerate() {
                        if name == bare.name || (name == "url" && bare.name == "URL") {
                            bare_names[index] = true;
                        }
                    }
                    if let Some(entry) = prelude_entries.iter().find(|entry| entry.name == name) {
                        if let Prelude::Target::Core { module, .. } = entry.target {
                            if !demand.prelude_modules.contains(&module) {
                                demand.prelude_modules.push(module);
                            }
                        }
                    }
                }
                TokKind::Str(parts) => {
                    for part in parts {
                        if let StrTokPart::Interp(nested) = part {
                            collect(nested, bare_names, prelude_entries, demand);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    let mut demand = jet_foundation::AST::CoreIdentifierDemand::default();
    let mut bare_names = [false; CoreModuleExports::CORE_BARE_SOURCE_MODULES.len()];
    collect(tokens, &mut bare_names, Prelude::entries(), &mut demand);
    for (index, bare) in CoreModuleExports::CORE_BARE_SOURCE_MODULES.iter().enumerate() {
        if bare_names[index] && !demand.bare_modules.contains(&bare.module_name) {
            demand.bare_modules.push(bare.module_name);
        }
    }
    // Preserve the alias injector's existing BTreeSet module order.
    demand.prelude_modules.sort_unstable();
    demand
}

#[cfg(test)]
mod core_identifier_demand_tests {
    use crate::Lexer;

    #[test]
    fn core_identifier_demand_ignores_literal_and_comment_payloads() {
        for source in [
            "\"URL Perf RNG Vec2 url or_err Clock Path read_file\"",
            "`URL Perf RNG Vec2 url or_err Clock Path read_file`",
            "\"\"\"\nURL Perf RNG Vec2 url or_err Clock Path read_file\n\"\"\"",
            "\"{{URL}} {{RNG}} {{or_err}}\"",
            "// URL RNG url or_err\n/* Perf /* Vec2 */ Clock Path read_file */\n",
            "#FFI(c) fn f() { \"\"\"URL RNG {Vec2} url or_err\"\"\" }",
            "URLExtra prefixURL _URL 变量URL URL变量",
        ] {
            let (tokens, diagnostics) = Lexer::lex(source);
            assert!(diagnostics.is_empty(), "{source:?}: {diagnostics:?}");
            let demand = Lexer::core_identifier_demand(&tokens);
            assert!(demand.bare_modules.is_empty(), "{source:?}: {demand:?}");
            assert!(!demand.option_or_err, "{source:?}: {demand:?}");
            assert!(demand.prelude_modules.is_empty(), "{source:?}: {demand:?}");
        }
    }

    #[test]
    fn exact_int_abs_receiver_demands_source_without_importing_a_bare_name() {
        for source in ["value.abs()", "value?.abs()", "value. /* source */ abs()", "\"{value.abs()}\""] {
            let (tokens, diagnostics) = Lexer::lex(source);
            assert!(diagnostics.is_empty(), "{source:?}: {diagnostics:?}");
            assert_eq!(Lexer::core_identifier_demand(&tokens).bare_modules, ["core.math"]);
        }
        for source in ["fn abs(value: Int) -> Int { value }", "\"value.abs()\"", "`value.abs()`", "// value.abs()\n"] {
            let (tokens, diagnostics) = Lexer::lex(source);
            assert!(diagnostics.is_empty(), "{source:?}: {diagnostics:?}");
            assert!(Lexer::core_identifier_demand(&tokens).bare_modules.is_empty(), "{source:?}");
        }
    }

    #[test]
    fn core_identifier_demand_keeps_real_and_nested_interpolation_identifiers() {
        let source = "\"literal URL {RNG} {\"Perf {Vec2}\"} {value.or_err(\"url\")}\"";
        let (tokens, diagnostics) = Lexer::lex(source);
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let demand = Lexer::core_identifier_demand(&tokens);
        assert_eq!(demand.bare_modules, ["core.testing", "core.math"]);
        assert!(demand.option_or_err);

        let (tokens, diagnostics) = Lexer::lex("Vec2 RNG Perf URL url value.or_err()");
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let demand = Lexer::core_identifier_demand(&tokens);
        assert_eq!(demand.bare_modules, ["core.net.url", "core.perf", "core.testing", "core.math"]);
        assert!(demand.option_or_err);
        assert!(demand.prelude_modules.windows(2).all(|pair| pair[0] < pair[1]));
    }
}
