//! TIR lowering: AST -> TIR (`LowerEnv`, `lower_*`, render helpers).
//!
//! Split out of the original `TIR.rs` for maintainability; behavior unchanged.

use super::*;
use crate::Diagnostics::Span;
use crate::Syntax;
use crate::AST::{
    AccessConvention, BinOp, BindPattern, ElseBranch, EnumLitArg, Expr, ForKind, Func, IfStmt,
    IndexKind, LValue, Lambda, LambdaBody, OrFallback, Param, PatSlot, Pattern, Stmt, StrPart,
    StructPatField, SwitchArm, TryConvert, Type, UnOp, VariantPayload,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

include!("lower/env.rs");
include!("lower/functions.rs");
include!("lower/statements.rs");
include!("lower/control_flow.rs");
include!("lower/patterns.rs");
include!("lower/panic.rs");
include!("lower/expressions.rs");
include!("lower/fields.rs");
include!("lower/method_calls.rs");
include!("lower/core_calls.rs");
include!("lower/call_args.rs");
include!("lower/builtins.rs");
include!("lower/lambdas.rs");
