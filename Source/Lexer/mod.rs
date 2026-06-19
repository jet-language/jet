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
pub use Tokens::{
    comments, describe, is_comment, without_comments, StrTokPart, TokKind, Token,
};

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
        s if s == Syntax::KW_IF => Some(TokKind::KwIf),
        s if s == Syntax::KW_ELSE => Some(TokKind::KwElse),
        s if s == Syntax::FOREIGN_WHILE => Some(TokKind::KwWhile),
        s if s == Syntax::FOREIGN_FOR => Some(TokKind::KwFor),
        s if s == Syntax::KW_IN => Some(TokKind::KwIn),
        s if s == Syntax::KW_SWITCH => Some(TokKind::KwSwitch),
        s if s == Syntax::KW_BREAK => Some(TokKind::KwBreak),
        s if s == Syntax::KW_CONTINUE => Some(TokKind::KwContinue),
        s if s == Syntax::LIT_TRUE => Some(TokKind::KwTrue),
        s if s == Syntax::LIT_FALSE => Some(TokKind::KwFalse),
        s if s == Syntax::KW_MUTATE => Some(TokKind::KwMutate),
        s if s == Syntax::KW_MOVE => Some(TokKind::KwMove),
        s if s == Syntax::KW_VIEW => Some(TokKind::KwView),
        s if s == Syntax::KW_STORED => Some(TokKind::KwStored),
        s if s == Syntax::KW_STRUCT => Some(TokKind::KwStruct),
        s if s == Syntax::KW_ENUM => Some(TokKind::KwEnum),
        s if s == Syntax::KW_IMPL => Some(TokKind::KwImpl),
        s if s == Syntax::KW_TRAIT => Some(TokKind::KwTrait),
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
        s if s == Syntax::KW_UNSAFE => Some(TokKind::KwUnsafe),
        s if s == Syntax::KW_USE => Some(TokKind::KwUse),
        s if s == Syntax::KW_EXTERN => Some(TokKind::KwExtern),
        s if s == Syntax::KW_MODULE => Some(TokKind::KwModule),
        s if s == Syntax::KW_TEST => Some(TokKind::KwTest),
        s if s == Syntax::KW_TODO => Some(TokKind::KwTodo),
        s if s == Syntax::KW_PURE => Some(TokKind::KwPure),
        _ => None,
    }
}
