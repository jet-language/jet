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
use std::collections::{HashMap, HashSet};

mod env;
mod functions;
mod statements;
mod control_flow;
mod patterns;
mod panic;
mod expressions;
mod fields;
mod method_calls;
mod core_calls;
mod call_args;
mod builtins;
mod lambdas;

pub(crate) use env::*;
pub(crate) use functions::*;
pub(crate) use statements::*;
pub(crate) use control_flow::*;
pub(crate) use patterns::*;
pub(crate) use panic::*;
pub(crate) use expressions::*;
pub(crate) use fields::*;
pub(crate) use method_calls::*;
pub(crate) use core_calls::*;
pub(crate) use call_args::*;
pub(crate) use builtins::*;
pub(crate) use lambdas::*;
