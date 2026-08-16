//! Shared dev-interpreter/source-debugger execution boundary.
//!
//! D-ARCH-SOURCE1=A: both outer products classify the same typed program.
//! Keeping the pure AST walk in the driver prevents either product from
//! depending on the root host or inventing a second boundary vocabulary.

use crate::AST::{
    core_import_maps, AccessConvention, CallArg, Expr, ImportKind, Item, ProgramBundle, Stmt,
};
use crate::Diagnostics::{Diagnostic, Span};
use std::collections::{HashMap, HashSet};

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
    let has_typed_cli = jet_foundation::CLISchema::entry_schema_for_bundle(bundle).is_some();
    // Whether the TIR evaluator runs a callee's frame depends on that callee's
    // own body, not on which module the call site sits in, so this set spans
    // the bundle: a `pub fn` imported unqualified and called bare is the same
    // interpretable frame as a local one. `inline_foreign` bodies are excluded
    // — those frames belong to the FFI bridge, which performs no writeback.
    let interpreted_functions: HashSet<&str> = bundle
        .modules
        .iter()
        .flat_map(|module| module.items.iter())
        .filter_map(|item| match item {
            Item::Func(function) if function.inline_foreign.is_none() => {
                Some(function.name.as_str())
            }
            _ => None,
        })
        .collect();
    for module in &bundle.modules {
        let (core_modules, core_items) = core_import_maps(&module.imports);
        for import in &module.imports {
            if let ImportKind::Module(name, span) = &import.kind {
                if let Some(feature) = native_module_feature(name, debug_impure) {
                    return Some(Boundary { feature: feature.to_string(), span: Some(*span) });
                }
            }
            if debug_impure {
                let (imported_modules, _) = core_import_maps(std::slice::from_ref(import));
                if let Some(feature) = imported_modules
                    .values()
                    .find_map(|name| native_module_feature(name, true))
                {
                    return Some(Boundary { feature: feature.to_string(), span: Some(import.span) });
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
                Item::Func(function) => {
                    if function.is_unsafe {
                        return Some(Boundary {
                            feature: "uses an `#Unsafe` function".to_string(),
                            span: Some(function.name_span),
                        });
                    }
                    if function.name == "run" && !function.params.is_empty() && !has_typed_cli {
                        return Some(Boundary {
                            feature: "uses a typed CLI entry signature (`fn run(args: T)`)".to_string(),
                            span: Some(function.name_span),
                        });
                    }
                    if !debug_impure {
                        if let Some(boundary) = scan_stmts_for_process_edge(
                            &function.body,
                            &core_modules,
                            &core_items,
                        ) {
                            return Some(boundary);
                        }
                    }
                    if let Some(boundary) =
                        scan_stmts_for_mut_arg(&function.body, &interpreted_functions, &core_items)
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
        "core.sys" | "core.process" if debug_impure => process_module_feature(name),
        // `core.time` / `core.math.random` are allowed: deterministic `Clock`/`Rng`
        // injection (D-DET1) is interpreted; ambient wall-clock / OS-RNG still
        // fail at the expression if unsupported.
        _ => None,
    }
}

fn process_module_feature(name: &str) -> Option<&'static str> {
    match name {
        "core.sys" => Some("reads the environment"),
        "core.process" => Some("runs another process or exits early"),
        _ => None,
    }
}

fn scan_stmts_for_process_edge(
    stmts: &[Stmt],
    core_modules: &HashMap<String, String>,
    core_items: &HashMap<String, String>,
) -> Option<Boundary> {
    let mut stmts = stmts.to_vec();
    for stmt in &mut stmts {
        let mut boundary = None;
        stmt.for_each_expr_mut(|expr| {
            if boundary.is_none() {
                boundary = process_edge_boundary(expr, core_modules, core_items);
            }
        });
        if boundary.is_some() {
            return boundary;
        }
    }
    None
}

fn process_edge_boundary(
    expr: &Expr,
    core_modules: &HashMap<String, String>,
    core_items: &HashMap<String, String>,
) -> Option<Boundary> {
    let (module, item, span) = match expr {
        Expr::Call(call) => (
            core_modules.get(&call.name)?.as_str(),
            core_items.get(&call.name)?.as_str(),
            call.name_span,
        ),
        Expr::MethodCall {
            receiver,
            method,
            method_span,
            ..
        } => {
            let Expr::Ident(alias, _) = receiver.as_ref() else {
                return None;
            };
            (core_modules.get(alias)?.as_str(), method.as_str(), *method_span)
        }
        _ => return None,
    };

    if is_interpreter_supported_process_leaf(module, item) {
        return None;
    }

    process_module_feature(module).map(|feature| Boundary {
        feature: feature.to_string(),
        span: Some(span),
    })
}

fn is_interpreter_supported_process_leaf(module: &str, item: &str) -> bool {
    matches!(
        (module, item),
        ("core.process", "argv")
            | ("core.process", "exit")
            | ("core.sys", "atexit")
            | ("core.sys", "stop")
    )
}

fn scan_stmts_for_mut_arg(
    stmts: &[Stmt],
    interpreted_functions: &HashSet<&str>,
    core_items: &HashMap<String, String>,
) -> Option<Boundary> {
    stmts
        .iter()
        .find_map(|stmt| scan_stmt_for_mut_arg(stmt, interpreted_functions, core_items))
}

fn scan_stmt_for_mut_arg(
    stmt: &Stmt,
    interpreted_functions: &HashSet<&str>,
    core_items: &HashMap<String, String>,
) -> Option<Boundary> {
    match stmt {
        Stmt::Expr(expr) | Stmt::DeferClose { close: expr, .. } => {
            expr_mut_arg(expr, interpreted_functions, core_items)
        }
        Stmt::Val(binding) => expr_mut_arg(&binding.init, interpreted_functions, core_items),
        Stmt::Assign { value, .. } => expr_mut_arg(value, interpreted_functions, core_items),
        Stmt::Return(Some(expr), _) => expr_mut_arg(expr, interpreted_functions, core_items),
        Stmt::While { cond, body, .. } | Stmt::CountedLoop { cond, body, .. } => {
            expr_mut_arg(cond, interpreted_functions, core_items)
                .or_else(|| scan_stmts_for_mut_arg(body, interpreted_functions, core_items))
        }
        Stmt::Loop { body, .. } | Stmt::For { body, .. } => {
            scan_stmts_for_mut_arg(body, interpreted_functions, core_items)
        }
        Stmt::Switch { arms, else_body, .. } => arms
            .iter()
            .find_map(|arm| scan_stmts_for_mut_arg(&arm.body, interpreted_functions, core_items))
            .or_else(|| {
                else_body.as_ref().and_then(|body| {
                    scan_stmts_for_mut_arg(body, interpreted_functions, core_items)
                })
            }),
        _ => None,
    }
}

/// Does the TIR evaluator itself run the frame this direct call names?
///
/// `&ident` writeback is a property of the CALLEE, not of the argument: the
/// evaluator copies the argument back into the caller's environment slot after
/// the callee frame returns, so it can only do that for a frame it executes.
/// Two callees qualify — a function declared in this module whose body is Jet
/// (`interpreted_functions`, which already excludes `inline_foreign`), and a
/// selectively imported Core leaf, whose writeback is the shared Prelude's
/// (`rng.shuffle(&deck)` and friends, #1217). Anything else — an unresolved
/// name, or a same-module function whose body is `#C`/foreign — is a frame the
/// evaluator does not run, so writeback there is silently dropped.
fn direct_call_writeback_is_interpreted(
    name: &str,
    interpreted_functions: &HashSet<&str>,
    core_items: &HashMap<String, String>,
) -> bool {
    interpreted_functions.contains(name) || core_items.contains_key(name)
}

fn expr_mut_arg(
    expr: &Expr,
    interpreted_functions: &HashSet<&str>,
    core_items: &HashMap<String, String>,
) -> Option<Boundary> {
    // Method and call-value forms never open. A module-qualified Core call
    // reaches the AST as `Expr::MethodCall { receiver: Ident(alias), .. }` (the
    // same shape `process_edge_boundary` matches above), and its writeback is
    // interpreted; S47 makes a `&`/`^` function direct-call-only, so a Write
    // argument through a function value is not constructible at all. Opening
    // either arm would only produce false boundaries.
    fn unwritten_arg(
        arg: &CallArg,
        interpreted_functions: &HashSet<&str>,
        core_items: &HashMap<String, String>,
    ) -> Option<Boundary> {
        if matches!(arg.convention, AccessConvention::Write) && matches!(arg.expr, Expr::Ident(..)) {
            return Some(Boundary {
                feature: "passes a `&` argument to a function (writeback isn't interpreted yet)"
                    .to_string(),
                span: Some(arg.span),
            });
        }
        expr_mut_arg(&arg.expr, interpreted_functions, core_items)
    }
    match expr {
        Expr::Call(call)
            if direct_call_writeback_is_interpreted(
                call.name.as_str(),
                interpreted_functions,
                core_items,
            ) =>
        {
            call.args
                .iter()
                .find_map(|arg| expr_mut_arg(&arg.expr, interpreted_functions, core_items))
        }
        Expr::Call(call) => call
            .args
            .iter()
            .find_map(|arg| unwritten_arg(arg, interpreted_functions, core_items)),
        Expr::MethodCall { receiver, args, .. } => {
            expr_mut_arg(receiver, interpreted_functions, core_items).or_else(|| {
                args.iter()
                    .find_map(|arg| expr_mut_arg(&arg.expr, interpreted_functions, core_items))
            })
        }
        Expr::CallValue { callee, args, .. } => {
            expr_mut_arg(callee, interpreted_functions, core_items).or_else(|| {
                args.iter()
                    .find_map(|arg| expr_mut_arg(&arg.expr, interpreted_functions, core_items))
            })
        }
        Expr::Unary(_, inner, _) | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _) | Expr::RawOf(inner, _) | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Field(inner, _, _) => expr_mut_arg(inner, interpreted_functions, core_items),
        Expr::Binary(_, left, right, _) => expr_mut_arg(left, interpreted_functions, core_items)
            .or_else(|| expr_mut_arg(right, interpreted_functions, core_items)),
        Expr::Index { base, index, .. } => expr_mut_arg(base, interpreted_functions, core_items)
            .or_else(|| expr_mut_arg(index, interpreted_functions, core_items)),
        _ => None,
    }
}
