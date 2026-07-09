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

include!("CheckerCore/scopes.rs");
include!("CheckerCore/type_assign.rs");
include!("CheckerCore/blocks.rs");
include!("CheckerCore/statements.rs");
include!("CheckerCore/control_flow.rs");
include!("CheckerCore/switches.rs");
include!("CheckerCore/types.rs");
include!("CheckerCore/bindings.rs");
include!("CheckerCore/names_incdec.rs");
include!("CheckerCore/helpers.rs");
