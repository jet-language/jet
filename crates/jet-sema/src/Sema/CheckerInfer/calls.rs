//! Type inference: calls, lambdas, method calls, and call checking.
//!
//! Split out of the original `CheckerInfer.rs`; behavior unchanged.

use super::*;
use crate::Collections;
use crate::Diagnostics::{Diagnostic, Span};
use crate::Generics::{e0901, e0904};
use crate::Sema::CheckerOwnership::{e0142_aliased, e0143_drop_unaudited};
use crate::Syntax;
use crate::AST::{
    AccessConvention, BinOp, Call, EnumLitArg, Expr, Lambda, LambdaBody, Stmt, StrPart, Type,
};
use std::collections::{HashMap, HashSet};

include!("calls/helpers_call_values.rs");
include!("calls/lambdas.rs");
include!("calls/builtin_methods.rs");
include!("calls/options_rng.rs");
include!("calls/method_calls.rs");
include!("calls/direct_calls.rs");
include!("calls/variadic.rs");
include!("calls/helpers.rs");
