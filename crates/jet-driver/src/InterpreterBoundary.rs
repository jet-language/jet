//! Shared dev-interpreter/source-debugger execution boundary.
//!
//! D-ARCH-SOURCE1=A: both outer products classify the same typed program.
//! Keeping the pure AST walk in the driver prevents either product from
//! depending on the root host or inventing a second boundary vocabulary.

use crate::AST::{Expr, ImportKind, Item, ProgramBundle, Stmt};
use crate::Diagnostics::{Diagnostic, Span};
use std::collections::HashSet;

struct Boundary {
    feature: String,
    span: Option<Span>,
}

pub fn dev_boundary_scan(bundle: &ProgramBundle) -> Option<Diagnostic> {
    boundary_scan(bundle, false).map(|boundary| dev_boundary_diagnostic(boundary.feature, boundary.span))
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
    boundary_scan(bundle, true).map(|boundary| {
        Diagnostic::error(
            "E2203",
            format!("`jet debug` can't step through this program yet — it {}", boundary.feature),
            "`jet debug` steps your program in the same interpreter `jet dev` uses; this feature touches threads, foreign code, raw memory, or the outside world, which the source-level stepper doesn't cover yet".to_string(),
            "run `jet build` then the binary, or `jet run <file>` to compile and run it; remove the unsupported feature to step the rest, or wait for the native-debugger milestone (D-DBG3 step 2)".to_string(),
            boundary.span,
        )
    })
}

fn boundary_scan(bundle: &ProgramBundle, debug_impure: bool) -> Option<Boundary> {
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
                if let Some(feature) = native_module_feature(name, debug_impure) {
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
                // An empty synthetic C module is only the resolution target for
                // an unused `use c.[…]`; it carries no foreign call to execute.
                // Keep the import runnable on tier 0, while real C surfaces
                // retain the native-only boundary.
                Item::CModule(module) if !module.functions.is_empty() => return Some(Boundary {
                    feature: "calls into a C library".to_string(),
                    span: Some(module.span),
                }),
                Item::Impl(item)
                    if !item.is_generated_serde
                        && matches!(item.trait_name.as_deref(), Some("Encode" | "Decode")) =>
                {
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

fn native_module_feature(name: &str, debug_impure: bool) -> Option<&'static str> {
    match name {
        "core.mem" => Some("uses the low-level `core.mem` tier"),
        "core.files" if debug_impure => Some("reads or writes files"),
        "core.env" => Some("reads the environment"),
        "core.process" => Some("runs another process or exits early"),
        // `core.time` / `core.random` are allowed: deterministic `Clock`/`Rng`
        // injection (D-DET1) is interpreted; ambient wall-clock / OS-RNG still
        // fail at the expression if unsupported.
        _ => None,
    }
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
        Stmt::Expr(expr) | Stmt::DeferClose { close: expr, .. } => {
            expr_mut_arg(expr, interpreted_functions)
        }
        Stmt::Val(binding) => expr_mut_arg(&binding.init, interpreted_functions),
        Stmt::Assign { value, .. } => expr_mut_arg(value, interpreted_functions),
        Stmt::Return(Some(expr), _) => expr_mut_arg(expr, interpreted_functions),
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

fn expr_mut_arg(expr: &Expr, interpreted_functions: &HashSet<&str>) -> Option<Boundary> {
    // Writeback for `&ident` args is implemented in the TIR evaluator for both
    // direct calls and method calls (D-DET shuffle / Clock+Rng injection).
    match expr {
        Expr::Call(call) => call
            .args
            .iter()
            .find_map(|arg| expr_mut_arg(&arg.expr, interpreted_functions)),
        Expr::MethodCall { receiver, args, .. } => expr_mut_arg(receiver, interpreted_functions)
            .or_else(|| {
                args.iter()
                    .find_map(|arg| expr_mut_arg(&arg.expr, interpreted_functions))
            }),
        Expr::CallValue { callee, args, .. } => expr_mut_arg(callee, interpreted_functions)
            .or_else(|| {
                args.iter()
                    .find_map(|arg| expr_mut_arg(&arg.expr, interpreted_functions))
            }),
        Expr::Unary(_, inner, _) | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _) | Expr::RawOf(inner, _) | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Field(inner, _, _) => expr_mut_arg(inner, interpreted_functions),
        Expr::Binary(_, left, right, _) => expr_mut_arg(left, interpreted_functions)
            .or_else(|| expr_mut_arg(right, interpreted_functions)),
        Expr::Index { base, index, .. } => expr_mut_arg(base, interpreted_functions)
            .or_else(|| expr_mut_arg(index, interpreted_functions)),
        _ => None,
    }
}
