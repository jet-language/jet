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

include!("subset/entry.rs");
include!("subset/types.rs");
include!("subset/structs_enums.rs");
include!("subset/statements.rs");
include!("subset/patterns.rs");
include!("subset/expressions.rs");
include!("subset/methods.rs");
include!("subset/builtin_methods.rs");
include!("subset/core_calls.rs");
include!("subset/handles.rs");
