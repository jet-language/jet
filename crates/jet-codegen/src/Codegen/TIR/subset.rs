//! TIR subset/coverage gate (`tir_covers*` and `is_covered_*`/`*_in_subset` predicates).
//!
//! Split out of the original `TIR.rs` for maintainability; behavior unchanged.

use super::*;
use crate::Syntax;
use crate::AST::{
    BinOp, BindPattern, ElseBranch, EnumLitArg, Expr, ForKind, Func, IfStmt, IndexKind, LValue,
    Lambda, LambdaBody, OrFallback, PatSlot, Pattern, Stmt, StrPart, StructPatField, SwitchArm,
    Type, VariantPayload,
};
use std::collections::HashSet;

mod entry;
mod types;
mod structs_enums;
mod statements;
mod patterns;
mod expressions;
mod methods;
mod builtin_methods;
mod core_calls;
mod handles;

pub(crate) use entry::*;
pub(crate) use types::*;
pub(crate) use structs_enums::*;
pub(crate) use statements::*;
pub(crate) use patterns::*;
pub(crate) use expressions::*;
pub(crate) use methods::*;
pub(crate) use builtin_methods::*;
pub(crate) use core_calls::*;
pub(crate) use handles::*;
