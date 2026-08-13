//! Comptime-evaluation detection: does any part of a bundle reach for a
//! compile-time evaluated construct? Split out of `Bundle.rs` to keep the
//! module under the card #510 boundary; this slice has no dependency on the
//! rest of Bundle beyond the AST types every checker function reads.

use crate::AST::{
    EnumLitArg, Expr, ForKind, Func, Item, LambdaBody, OrFallback, Pattern, ProgramBundle, Stmt,
    StrPart, StructPatField,
};

pub fn bundle_has_comptime_evaluation(bundle: &ProgramBundle) -> bool {
    bundle
        .modules
        .iter()
        .flat_map(|module| &module.items)
        .any(item_has_comptime_evaluation)
}

fn item_has_comptime_evaluation(item: &Item) -> bool {
    let function = |function: &Func| stmts_have_comptime_evaluation(&function.body);
    match item {
        Item::Func(value) => function(value),
        Item::Struct(value) => {
            value.methods.iter().any(function)
                || value
                    .trait_impls
                    .iter()
                    .flat_map(|implementation| &implementation.methods)
                    .any(function)
        }
        Item::Enum(value) => {
            value.methods.iter().any(function)
                || value
                    .trait_impls
                    .iter()
                    .flat_map(|implementation| &implementation.methods)
                    .any(function)
        }
        Item::Trait(value) => value
            .methods
            .iter()
            .filter_map(|method| method.default_body.as_deref())
            .any(stmts_have_comptime_evaluation),
        Item::Impl(value) => value.methods.iter().any(function),
        Item::Const(value) => value.is_comptime,
        Item::Test(value) => stmts_have_comptime_evaluation(&value.body),
        Item::Bench(value) => stmts_have_comptime_evaluation(&value.body),
        Item::CodeModule(value) => value
            .body
            .as_deref()
            .is_some_and(|body| body.iter().any(item_has_comptime_evaluation)),
        Item::ErrorConv(value) => stmts_have_comptime_evaluation(&value.body),
        Item::UserDerive(value) => stmts_have_comptime_evaluation(&value.body),
        Item::GenericModule(value) => value.body.iter().any(item_has_comptime_evaluation),
        _ => false,
    }
}

pub(super) fn stmts_have_comptime_evaluation(stmts: &[Stmt]) -> bool {
    stmts.iter().any(|stmt| match stmt {
        Stmt::Expr(value) | Stmt::Yield(value, _) => expr_has_comptime_evaluation(value),
        Stmt::Val(binding) => {
            binding.is_comptime || expr_has_comptime_evaluation(&binding.init)
        }
        Stmt::Assign { value, .. } => expr_has_comptime_evaluation(value),
        Stmt::Return(Some(value), _) => expr_has_comptime_evaluation(value),
        Stmt::BreakValue(value, _) | Stmt::BreakLabelValue(_, _, value, _) => {
            expr_has_comptime_evaluation(value)
        }
        Stmt::Return(None, _) => false,
        Stmt::While { cond, body, .. } => {
            expr_has_comptime_evaluation(cond) || stmts_have_comptime_evaluation(body)
        }
        Stmt::For { kind, body, .. } => {
            let iterable = match kind {
                ForKind::Range { start, end, step, exclusive: _ } => {
                    expr_has_comptime_evaluation(start)
                        || expr_has_comptime_evaluation(end)
                        || step.as_ref().is_some_and(expr_has_comptime_evaluation)
                }
                ForKind::In { collection, step } => {
                    expr_has_comptime_evaluation(collection)
                        || step.as_ref().is_some_and(expr_has_comptime_evaluation)
                }
            };
            iterable || stmts_have_comptime_evaluation(body)
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => {
            expr_has_comptime_evaluation(subject)
                || arms.iter().any(|arm| {
                    expr_has_comptime_evaluation(&arm.cond)
                        || stmts_have_comptime_evaluation(&arm.body)
                })
                || else_body
                    .as_deref()
                    .is_some_and(stmts_have_comptime_evaluation)
        }
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            init.is_comptime
                || expr_has_comptime_evaluation(&init.init)
                || expr_has_comptime_evaluation(cond)
                || step
                    .as_deref()
                    .is_some_and(|step| {
                        stmts_have_comptime_evaluation(std::slice::from_ref(step))
                    })
                || stmts_have_comptime_evaluation(body)
        }
        Stmt::Loop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::Transact { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::ScopeMember { body, .. } => stmts_have_comptime_evaluation(body),
        Stmt::ContextBlock { fields, body, .. } => {
            fields
                .iter()
                .any(|(_, value, _)| expr_has_comptime_evaluation(value))
                || stmts_have_comptime_evaluation(body)
        }
        Stmt::ComptimeIf { .. }
        | Stmt::ComptimeSwitch { .. }
        | Stmt::ComptimeBlock { .. } => true,
        Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::BreakLabel(..)
        | Stmt::ContinueLabel(..) => false,
    })
}

fn expr_has_comptime_evaluation(expr: &Expr) -> bool {
    let argument = |arg: &crate::AST::CallArg| expr_has_comptime_evaluation(&arg.expr);
    match expr {
        Expr::Str(parts, _) => parts.iter().any(|part| match part {
            StrPart::Interp(value, _) => expr_has_comptime_evaluation(value),
            StrPart::Lit(_) => false,
        }),
        Expr::ListLit(values, _) => values.iter().any(expr_has_comptime_evaluation),
        Expr::MemberSpread { base, .. } => expr_has_comptime_evaluation(base),
        Expr::Spread(value, _)
        | Expr::Unary(_, value, _)
        | Expr::Deref(value, _)
        | Expr::RawOf(value, _)
        | Expr::Copy(value, _)
        | Expr::Place(value, _, _)
        | Expr::Field(value, _, _)
        | Expr::Tainted(value, _, _)
        | Expr::Present(value, _)
        | Expr::Ok(value, _)
        | Expr::Err(value, _)
        | Expr::Paren(value, _)
        | Expr::IncDec { operand: value, .. }
        | Expr::PtrFromAddr { addr: value, .. } => expr_has_comptime_evaluation(value),
        Expr::Try(value, _, _, note) => {
            expr_has_comptime_evaluation(value)
                || note
                    .as_deref()
                    .is_some_and(expr_has_comptime_evaluation)
        }
        Expr::MapLit(entries, _) => entries.iter().any(|(key, value)| {
            expr_has_comptime_evaluation(key) || expr_has_comptime_evaluation(value)
        }),
        Expr::Index { base, index, .. } => {
            expr_has_comptime_evaluation(base) || expr_has_comptime_evaluation(index)
        }
        Expr::Slice {
            base, start, end, range, ..
        } => {
            expr_has_comptime_evaluation(base)
                || range.as_deref().map_or_else(
                    || {
                        expr_has_comptime_evaluation(start)
                            || expr_has_comptime_evaluation(end)
                    },
                    expr_has_comptime_evaluation,
                )
        }
        Expr::Range { start, end, .. } => {
            expr_has_comptime_evaluation(start) || expr_has_comptime_evaluation(end)
        }
        Expr::Call(call) => call.args.iter().any(argument),
        Expr::Binary(_, left, right, _) => {
            expr_has_comptime_evaluation(left) || expr_has_comptime_evaluation(right)
        }
        Expr::CompareChain { operands, .. } => {
            operands.iter().any(expr_has_comptime_evaluation)
        }
        Expr::OptField { base, .. } => expr_has_comptime_evaluation(base),
        Expr::MethodCall { receiver, args, .. } => {
            expr_has_comptime_evaluation(receiver) || args.iter().any(argument)
        }
        Expr::StructLit { fields, .. } => fields
            .iter()
            .any(|(_, _, value)| expr_has_comptime_evaluation(value)),
        Expr::TypedLit { body, .. } => {
            let mut hit = false;
            body.for_each_expr(|value| {
                if expr_has_comptime_evaluation(value) {
                    hit = true;
                }
            });
            hit
        }
        Expr::EnumLit { args, .. } => args.iter().any(|arg| match arg {
            EnumLitArg::Positional(value) | EnumLitArg::Named { expr: value, .. } => {
                expr_has_comptime_evaluation(value)
            }
        }),
        Expr::PatternTest {
            subject, pattern, ..
        } => {
            expr_has_comptime_evaluation(subject)
                || match pattern {
                    Pattern::Struct { fields, .. } => fields.iter().any(|field| match field {
                        StructPatField::Value { value, .. } => {
                            expr_has_comptime_evaluation(value)
                        }
                        StructPatField::Bind { .. } => false,
                    }),
                    _ => false,
                }
        }
        Expr::OrFallback {
            value, fallback, ..
        } => {
            expr_has_comptime_evaluation(value)
                || match fallback {
                    OrFallback::Value(value) | OrFallback::Return(Some(value), _) => {
                        expr_has_comptime_evaluation(value)
                    }
                    _ => false,
                }
        }
        Expr::If {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            expr_has_comptime_evaluation(cond)
                || stmts_have_comptime_evaluation(then_body)
                || expr_has_comptime_evaluation(then_value)
                || stmts_have_comptime_evaluation(else_body)
                || expr_has_comptime_evaluation(else_value)
        }
        Expr::TupleLit(fields, _, _) => fields
            .iter()
            .any(|(_, value)| expr_has_comptime_evaluation(value)),
        Expr::Lambda(lambda) => match &lambda.body {
            LambdaBody::Expr(value) => expr_has_comptime_evaluation(value),
            LambdaBody::Block(body) => stmts_have_comptime_evaluation(body),
        },
        Expr::CallValue { callee, args, .. } => {
            expr_has_comptime_evaluation(callee) || args.iter().any(argument)
        }
        Expr::ComptimeName { .. } => true,
        Expr::Int(..)
        | Expr::Float(..)
        | Expr::Bool(..)
        | Expr::Char(..)
        | Expr::StrMatchLit(..)
        | Expr::BinMatchLit(..)
        | Expr::Ident(..)
        | Expr::UnitLit { .. }
        | Expr::Absent(_)
        | Expr::Todo { .. }
        | Expr::NoElse(_)
        | Expr::ReduceMarker(..) => false,
    }
}
