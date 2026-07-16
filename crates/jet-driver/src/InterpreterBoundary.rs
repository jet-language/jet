//! Shared dev-interpreter/source-debugger execution boundary.
//!
//! D-ARCH-SOURCE1=A: both outer products classify the same typed program.
//! Keeping the pure AST walk in the driver prevents either product from
//! depending on the root host or inventing a second boundary vocabulary.

use crate::AST::{AccessConvention, CallArg, ElseBranch, Expr, IfStmt, ImportKind, Item, ProgramBundle, Stmt};
use crate::Diagnostics::{Diagnostic, Span};
use std::collections::HashSet;

struct Boundary {
    feature: String,
    span: Option<Span>,
}

pub fn dev_boundary_scan(bundle: &ProgramBundle) -> Option<Diagnostic> {
    boundary_scan(bundle).map(|boundary| dev_boundary_diagnostic(boundary.feature, boundary.span))
}

pub fn dev_boundary_diagnostic(feature: impl Into<String>, span: Option<Span>) -> Diagnostic {
    Diagnostic::error(
        "E2201",
        format!("`jet dev` can't interpret this program yet — it {}", feature.into()),
        "`jet dev` runs your program in a built-in interpreter for instant feedback, but that interpreter doesn't cover every feature; this one needs the real native build".to_string(),
        "run `jet build` then the binary, or `jet run <file>` to compile and run it; `jet dev` will keep showing checks live".to_string(),
        span,
    )
}

pub fn debug_boundary_scan(bundle: &ProgramBundle) -> Option<Diagnostic> {
    boundary_scan(bundle).map(|boundary| {
        Diagnostic::error(
            "E2203",
            format!("`jet debug` can't step through this program yet — it {}", boundary.feature),
            "`jet debug` steps your program in the same interpreter `jet dev` uses; this feature touches threads, foreign code, raw memory, or the outside world, which the source-level stepper doesn't cover yet".to_string(),
            "run `jet build` then the binary, or `jet run <file>` to compile and run it; remove the unsupported feature to step the rest, or wait for the native-debugger milestone (D-DBG3 step 2)".to_string(),
            boundary.span,
        )
    })
}

fn boundary_scan(bundle: &ProgramBundle) -> Option<Boundary> {
    for module in &bundle.modules {
        let interpreted_functions: HashSet<&str> = module
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Func(function) if function.inline_foreign.is_none() => {
                    Some(function.name.as_str())
                }
                _ => None,
            })
            .collect();
        for import in &module.imports {
            if let ImportKind::Module(name, span) = &import.kind {
                if let Some(feature) = native_module_feature(name) {
                    return Some(Boundary { feature: feature.to_string(), span: Some(*span) });
                }
            }
        }
        for item in &module.items {
            match item {
                Item::ExternRust(block) => return Some(Boundary {
                    feature: "calls into Rust code through `extern rust`".to_string(),
                    span: Some(block.span),
                }),
                Item::CModule(module) => return Some(Boundary {
                    feature: "calls into a C library".to_string(),
                    span: Some(module.span),
                }),
                Item::Impl(item) if matches!(item.trait_name.as_deref(), Some("Encode" | "Decode")) => {
                    return Some(Boundary {
                        feature: "uses a typed encoding implementation".to_string(),
                        span: item.trait_span.or(Some(item.type_span)),
                    });
                }
                Item::Func(function) => {
                    if function.is_unsafe {
                        return Some(Boundary {
                            feature: "uses an `#Unsafe` function".to_string(),
                            span: Some(function.name_span),
                        });
                    }
                    if function.name == "run" && !function.params.is_empty() {
                        return Some(Boundary {
                            feature: "uses a typed CLI entry signature (`fn run(args: T)`)".to_string(),
                            span: Some(function.name_span),
                        });
                    }
                    if let Some(boundary) = scan_stmts_for_unsafe(&function.body) {
                        return Some(boundary);
                    }
                    if let Some(boundary) =
                        scan_stmts_for_mut_arg(&function.body, &interpreted_functions)
                    {
                        return Some(boundary);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

fn native_module_feature(name: &str) -> Option<&'static str> {
    match name {
        "core.tasks" => Some("spawns a task or uses a channel"),
        "core.mem" => Some("uses the low-level `core.mem` tier"),
        "core.files" => Some("reads or writes files"),
        "core.env" => Some("reads the environment"),
        "core.process" => Some("runs another process or exits early"),
        "core.random" => Some("uses random numbers"),
        "core.time" => Some("reads the clock or sleeps"),
        _ => None,
    }
}

fn scan_stmts_for_unsafe(stmts: &[Stmt]) -> Option<Boundary> {
    stmts.iter().find_map(scan_stmt_for_unsafe)
}

fn scan_stmt_for_unsafe(stmt: &Stmt) -> Option<Boundary> {
    match stmt {
        Stmt::Unsafe { span, .. } => Some(Boundary {
            feature: "uses an `#Unsafe` block".to_string(),
            span: Some(*span),
        }),
        Stmt::If(statement) => scan_if_for_unsafe(statement),
        Stmt::While { body, .. } | Stmt::Loop { body, .. } | Stmt::CountedLoop { body, .. }
        | Stmt::For { body, .. } => scan_stmts_for_unsafe(body),
        Stmt::Switch { arms, else_body, .. } => arms.iter()
            .find_map(|arm| scan_stmts_for_unsafe(&arm.body))
            .or_else(|| else_body.as_ref().and_then(|body| scan_stmts_for_unsafe(body))),
        _ => None,
    }
}

fn scan_if_for_unsafe(statement: &IfStmt) -> Option<Boundary> {
    scan_stmts_for_unsafe(&statement.then_body).or_else(|| match &statement.else_branch {
        Some(ElseBranch::ElseIf(inner)) => scan_if_for_unsafe(inner),
        Some(ElseBranch::Else(body)) => scan_stmts_for_unsafe(body),
        None => None,
    })
}

fn scan_stmts_for_mut_arg(
    stmts: &[Stmt],
    interpreted_functions: &HashSet<&str>,
) -> Option<Boundary> {
    stmts
        .iter()
        .find_map(|stmt| scan_stmt_for_mut_arg(stmt, interpreted_functions))
}

fn scan_stmt_for_mut_arg(
    stmt: &Stmt,
    interpreted_functions: &HashSet<&str>,
) -> Option<Boundary> {
    match stmt {
        Stmt::Expr(expr) => expr_mut_arg(expr, interpreted_functions),
        Stmt::Val(binding) => expr_mut_arg(&binding.init, interpreted_functions),
        Stmt::Assign { value, .. } => expr_mut_arg(value, interpreted_functions),
        Stmt::Return(Some(expr), _) => expr_mut_arg(expr, interpreted_functions),
        Stmt::If(statement) => scan_if_for_mut_arg(statement, interpreted_functions),
        Stmt::While { cond, body, .. } | Stmt::CountedLoop { cond, body, .. } => {
            expr_mut_arg(cond, interpreted_functions)
                .or_else(|| scan_stmts_for_mut_arg(body, interpreted_functions))
        }
        Stmt::Loop { body, .. } | Stmt::For { body, .. } => {
            scan_stmts_for_mut_arg(body, interpreted_functions)
        }
        Stmt::Switch { arms, else_body, .. } => arms.iter()
            .find_map(|arm| scan_stmts_for_mut_arg(&arm.body, interpreted_functions))
            .or_else(|| else_body.as_ref().and_then(|body| {
                scan_stmts_for_mut_arg(body, interpreted_functions)
            })),
        _ => None,
    }
}

fn scan_if_for_mut_arg(
    statement: &IfStmt,
    interpreted_functions: &HashSet<&str>,
) -> Option<Boundary> {
    expr_mut_arg(&statement.cond, interpreted_functions)
        .or_else(|| scan_stmts_for_mut_arg(&statement.then_body, interpreted_functions))
        .or_else(|| match &statement.else_branch {
            Some(ElseBranch::ElseIf(inner)) => {
                scan_if_for_mut_arg(inner, interpreted_functions)
            }
            Some(ElseBranch::Else(body)) => {
                scan_stmts_for_mut_arg(body, interpreted_functions)
            }
            None => None,
        })
}

fn expr_mut_arg(expr: &Expr, interpreted_functions: &HashSet<&str>) -> Option<Boundary> {
    let argument = |arg: &CallArg| {
        if matches!(arg.convention, AccessConvention::Write) && matches!(arg.expr, Expr::Ident(..)) {
            Some(Boundary {
                feature: "passes a `&` argument to a function (writeback isn't interpreted yet)".to_string(),
                span: Some(arg.span),
            })
        } else {
            expr_mut_arg(&arg.expr, interpreted_functions)
        }
    };
    match expr {
        // Direct user calls write back `&ident` arguments after the callee
        // frame returns. Method and call-value writeback remain outside the
        // interpreter subset.
        Expr::Call(call) if interpreted_functions.contains(call.name.as_str()) => call
            .args
            .iter()
            .find_map(|arg| expr_mut_arg(&arg.expr, interpreted_functions)),
        Expr::Call(call) => call.args.iter().find_map(argument),
        Expr::MethodCall { receiver, args, .. } => expr_mut_arg(receiver, interpreted_functions)
            .or_else(|| args.iter().find_map(argument)),
        Expr::CallValue { callee, args, .. } => expr_mut_arg(callee, interpreted_functions)
            .or_else(|| args.iter().find_map(argument)),
        Expr::Unary(_, inner, _) | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _) | Expr::RawOf(inner, _) | Expr::Copy(inner, _)
        | Expr::Field(inner, _, _) => expr_mut_arg(inner, interpreted_functions),
        Expr::Binary(_, left, right, _) => expr_mut_arg(left, interpreted_functions)
            .or_else(|| expr_mut_arg(right, interpreted_functions)),
        Expr::Index { base, index, .. } => expr_mut_arg(base, interpreted_functions)
            .or_else(|| expr_mut_arg(index, interpreted_functions)),
        _ => None,
    }
}
