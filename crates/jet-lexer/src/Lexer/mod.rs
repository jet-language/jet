//! Lexer: source text -> tokens. Every token carries a byte span so that
//! diagnostics anywhere downstream can point at real source.
//!
//! M1: the lexer recovers from errors — it reports every problem it finds
//! in one run instead of stopping at the first.

use crate::Diagnostics::Diagnostic;
use crate::Syntax;

mod Scan;
mod Strings;
mod Terminators;
mod Tokens;

pub use Terminators::lex;
pub use Tokens::{comments, describe, is_comment, without_comments, StrTokPart, TokKind, Token};

// `lex_raw` is part of the public surface (interpolation sub-streams) and is
// also called by sibling scan/strings modules.
pub use Scan::lex_raw;

struct Lexer<'a> {
    chars: Vec<(usize, char)>,
    end: usize,
    src: &'a str,
    i: usize,
    diags: Vec<Diagnostic>,
}

fn keyword(name: &str) -> Option<TokKind> {
    match name {
        s if s == Syntax::KW_FN => Some(TokKind::KwFn),
        s if s == Syntax::KW_PUB => Some(TokKind::KwPub),
        s if s == Syntax::KW_PRIV => Some(TokKind::KwPriv),
        s if s == Syntax::KW_IF => Some(TokKind::KwIf),
        s if s == Syntax::KW_ELSE => Some(TokKind::KwElse),
        s if s == Syntax::KW_IN => Some(TokKind::KwIn),
        s if s == Syntax::KW_SWITCH => Some(TokKind::KwSwitch),
        s if s == Syntax::KW_BREAK => Some(TokKind::KwBreak),
        s if s == Syntax::KW_CONTINUE => Some(TokKind::KwContinue),
        s if s == Syntax::LIT_TRUE => Some(TokKind::KwTrue),
        s if s == Syntax::LIT_FALSE => Some(TokKind::KwFalse),
        s if s == Syntax::KW_MUTATE => Some(TokKind::KwMutate),
        s if s == Syntax::KW_MOVE => Some(TokKind::KwMove),
        s if s == Syntax::KW_COPY => Some(TokKind::KwCopy),
        s if s == Syntax::KW_STRUCT => Some(TokKind::KwStruct),
        s if s == Syntax::KW_ENUM => Some(TokKind::KwEnum),
        s if s == Syntax::KW_IMPL => Some(TokKind::KwImpl),
        s if s == Syntax::KW_TRAIT => Some(TokKind::KwTrait),
        s if s == Syntax::KW_TAG => Some(TokKind::KwTag),
        s if s == Syntax::KW_DERIVE => Some(TokKind::KwDerive),
        s if s == Syntax::KW_SELF => Some(TokKind::KwSelf),
        s if s == Syntax::LIT_NULL => Some(TokKind::KwNull),
        s if s == Syntax::LIT_OK => Some(TokKind::KwOk),
        s if s == Syntax::LIT_ERR => Some(TokKind::KwErr),
        s if s == Syntax::KW_IT => Some(TokKind::KwIt),
        s if s == Syntax::KW_CONST => Some(TokKind::KwConst),
        s if s == Syntax::KW_COMPTIME => Some(TokKind::KwComptime),
        s if s == Syntax::KW_RETURN => Some(TokKind::KwReturn),
        s if s == Syntax::KW_LOOP => Some(TokKind::KwLoop),
        s if s == Syntax::KW_YIELD => Some(TokKind::KwYield),
        s if s == Syntax::KW_UNSAFE => Some(TokKind::KwUnsafe),
        s if s == Syntax::KW_USE => Some(TokKind::KwUse),
        s if s == Syntax::KW_EXTERN => Some(TokKind::KwExtern),
        s if s == Syntax::KW_MODULE => Some(TokKind::KwModule),
        // D-CASING1 follow-on: `Test`/`Todo`/`Pure` are NOT lexer keywords —
        // they are PascalCase `#`-markers recognized as `#` + ident in the
        // parser, so the bare words stay usable as ordinary identifiers (e.g. a
        // struct named `Test`). The lowercase foreign spellings are likewise
        // plain idents, matched in the parser for teaching errors.
        _ => None,
    }
}
