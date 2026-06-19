//! Lexer: source text -> tokens. Every token carries a byte span so that
//! diagnostics anywhere downstream can point at real source.
//!
//! M1: the lexer recovers from errors — it reports every problem it finds
//! in one run instead of stopping at the first.

use crate::diag::Diagnostic;
use crate::syntax;

mod scan;
mod strings;
mod terminators;
mod tokens;

pub use terminators::lex;
pub use tokens::{
    comments, describe, is_comment, without_comments, StrTokPart, TokKind, Token,
};

// `lex_raw` is part of the public surface (interpolation sub-streams) and is
// also called by sibling scan/strings modules.
pub use scan::lex_raw;

struct Lexer<'a> {
    chars: Vec<(usize, char)>,
    end: usize,
    src: &'a str,
    i: usize,
    diags: Vec<Diagnostic>,
}

fn keyword(name: &str) -> Option<TokKind> {
    match name {
        s if s == syntax::KW_FN => Some(TokKind::KwFn),
        s if s == syntax::KW_PUB => Some(TokKind::KwPub),
        s if s == syntax::KW_IF => Some(TokKind::KwIf),
        s if s == syntax::KW_ELSE => Some(TokKind::KwElse),
        s if s == syntax::FOREIGN_WHILE => Some(TokKind::KwWhile),
        s if s == syntax::FOREIGN_FOR => Some(TokKind::KwFor),
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
        s if s == syntax::KW_TODO => Some(TokKind::KwTodo),
        s if s == syntax::KW_PURE => Some(TokKind::KwPure),
        _ => None,
    }
}
