use super::*;
use crate::Collections::is_map_key_type;
use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::Generics::{e0905, e0909, generic_depth_exceeded, substitute_type, COMPARABLE};
use crate::Sema::CheckerOwnership::e0141_unconsumed_branch;
use crate::Syntax;
use crate::AST::{
    AccessConvention, BinOp, BindPattern, Binding, CallArg, ElseBranch, Expr, ForKind, IfStmt,
    IncDecOp, IndexKind, LValue, MetaAttr, MetaField, Pattern, Stmt, StrPart, Type,
};
use std::collections::{HashMap, HashSet};

mod scopes;
mod type_assign;
mod blocks;
mod statements;
mod control_flow;
mod switches;
mod types;
mod bindings;
pub(crate) use bindings::*;
mod names_incdec;
mod helpers;
