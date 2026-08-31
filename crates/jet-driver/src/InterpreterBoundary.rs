//! Shared dev-interpreter/source-debugger execution boundary.
//!
//! D-ARCH-SOURCE1=A: both outer products classify the same typed program.
//! Keeping the pure AST walk in the driver prevents either product from
//! depending on the root host or inventing a second boundary vocabulary.

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{
    core_import_maps, AccessConvention, CallArg, Expr, ImportKind, Item, ProgramBundle, Stmt,
};
use std::collections::{HashMap, HashSet};

struct Boundary {
    /// A NOUN PHRASE naming the construct, and nothing else: it is rendered
    /// as the object of "it uses …" by `dev_boundary_diagnostic` (E2201) and
    /// `debug_boundary_scan` (E2203), which own the whole sentence. A feature
    /// that carries its own clause splices two sentences together — that is
    /// exactly the E2201 garble this contract exists to prevent — so never
    /// append a reason, a "which …" tail, or a trailing "yet" here. The
    /// wrapper never parses this string back apart.
    feature: String,
    span: Option<Span>,
}

pub fn dev_boundary_scan(bundle: &ProgramBundle) -> Option<Diagnostic> {
    boundary_scan(bundle, false)
        .map(|boundary| dev_boundary_diagnostic(boundary.feature, boundary.span))
}

/// Why/fix for every `jet dev` boundary report. `why` already carries the
/// "that interpreter doesn't cover every feature" explanation, so no producer
/// restates it in the sentence.
const DEV_BOUNDARY_WHY: &str = "`jet dev` runs your program in a built-in interpreter for instant feedback, but that interpreter doesn't cover every feature; this one needs the real native build";
const DEV_BOUNDARY_FIX: &str = "run `jet build` then the binary, or `jet run <file>` to compile and run it; `jet dev` will keep showing checks live";

/// Render the dev-loop boundary around one construct. `feature` is a noun
/// phrase (see `Boundary::feature`) and this function owns the sentence.
pub fn dev_boundary_diagnostic(feature: impl Into<String>, span: Option<Span>) -> Diagnostic {
    Diagnostic::error(
        "E2201",
        format!(
            "`jet dev` can't interpret this program yet — it uses {}",
            feature.into()
        ),
        DEV_BOUNDARY_WHY.to_string(),
        DEV_BOUNDARY_FIX.to_string(),
        span,
    )
}

/// The same boundary for the shared evaluator's own mid-run refusal (E0956),
/// which stops during execution instead of at the AST pre-scan.
///
/// `construct` is the noun phrase the raise site named, taken from the
/// diagnostic's structured construct — never from E0956's rendered prose,
/// which is a sentence and splices into an ungrammatical report. A
/// hand-written E0956 that carries no structured construct has no noun phrase
/// to place, so its own sentence is quoted whole after a colon rather than
/// forced into "it uses …"; either way the dev loop replaces the
/// comptime-voiced why/fix, which is the point of this rewrap.
pub fn dev_boundary_for_refusal(
    construct: Option<&str>,
    refusal: &str,
    span: Option<Span>,
) -> Diagnostic {
    match construct {
        Some(construct) => dev_boundary_diagnostic(format!("`{construct}`"), span),
        None => Diagnostic::error(
            "E2201",
            format!(
                "`jet dev` can't interpret this program yet — the interpreter stopped here: {refusal}"
            ),
            DEV_BOUNDARY_WHY.to_string(),
            DEV_BOUNDARY_FIX.to_string(),
            span,
        ),
    }
}

pub fn debug_boundary_scan(bundle: &ProgramBundle) -> Option<Diagnostic> {
    boundary_scan(bundle, true).map(|boundary| {
        Diagnostic::error(
            "E2203",
            format!("`jet debug` can't step through this program yet — it uses {}", boundary.feature),
            "The interpreter debugger cannot model threads, foreign code, raw memory, or host state. The native `jet debug` CLI normally selects the LLDB backend for this program.".to_string(),
            "Use the native `jet debug <file>` path with LLDB installed, or use `jet run <file>` when native debugging is unavailable.".to_string(),
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
                    return Some(Boundary {
                        feature: feature.to_string(),
                        span: Some(*span),
                    });
                }
            }
            if debug_impure {
                let (imported_modules, _) = core_import_maps(std::slice::from_ref(import));
                if let Some(feature) = imported_modules
                    .values()
                    .find_map(|name| native_module_feature(name, true))
                {
                    return Some(Boundary {
                        feature: feature.to_string(),
                        span: Some(import.span),
                    });
                }
            }
        }
        for item in &module.items {
            match item {
                Item::ExternRust(block) => {
                    return Some(Boundary {
                        feature: "Rust code called through `extern rust`".to_string(),
                        span: Some(block.span),
                    })
                }
                // An empty synthetic C module is only the resolution target for
                // an unused `use c.[…]`; it carries no foreign call to execute.
                // Supported hidden-bridge signatures are also runnable on tier
                // 0: the evaluator marshals them through the same prepared
                // `*_cabi` bridge as the resident JIT. Keep other C signatures
                // on the native-only boundary until their adapter exists.
                Item::CModule(module)
                    if !module.functions.is_empty()
                        && !module
                            .functions
                            .iter()
                            .all(|function| function.hidden_c_bridge_compatible()) =>
                {
                    return Some(Boundary {
                        feature: "a C library".to_string(),
                        span: Some(module.span),
                    });
                }
                Item::Func(function) => {
                    if function.is_unsafe {
                        return Some(Boundary {
                            feature: "an `#Unsafe` function".to_string(),
                            span: Some(function.name_span),
                        });
                    }
                    if function.name == "run" && !function.params.is_empty() && !has_typed_cli {
                        return Some(Boundary {
                            feature: "a typed CLI entry signature (`fn run(args: T)`)".to_string(),
                            span: Some(function.name_span),
                        });
                    }
                    if !debug_impure {
                        if let Some(boundary) =
                            scan_stmts_for_process_edge(&function.body, &core_modules, &core_items)
                        {
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
        "core.mem" => Some("the low-level `core.mem` tier"),
        "core.files" if debug_impure => Some("a file read or write"),
        // `jet debug` refuses the impure module at its IMPORT, because the
        // source stepper has no per-call pass: `scan_stmts_for_process_edge`
        // runs for `jet dev` only, so nothing here can tell an interpretable
        // `env.get` from `sys.fork()`. `jet dev` classifies per leaf instead
        // (`process_leaf_feature`), which is why these two arms name the
        // module and that one names the call.
        "core.sys" if debug_impure => Some("an environment read"),
        "core.process" if debug_impure => Some("a process launch or an early exit"),
        // `core.time` / `core.math.random` are allowed: deterministic `Clock`/`Rng`
        // injection (D-DET1) is interpreted; ambient wall-clock / OS-RNG still
        // fail at the expression if unsupported.
        _ => None,
    }
}

/// `jet dev`'s per-call verdict for the two process-edge modules: `None` means
/// the shared evaluator runs this exact leaf, `Some(feature)` is the noun
/// phrase naming why it cannot.
///
/// Keyed by LEAF, never by module. `core.sys` registers ~55 members
/// (`jet-sema` `module_items`) and the interpreter ambient marshals only the
/// handful listed below, so a module-level "yes" would admit `sys.fork()` along
/// with `env.get`. The default arms therefore REFUSE: a newly registered
/// `core.sys` / `core.process` member stays native-only until an ambient arm
/// exists for it, rather than silently inheriting a neighbour's coverage.
fn process_leaf_feature(module: &str, item: &str) -> Option<&'static str> {
    match (module, item) {
        // Run by the shared evaluator itself: `process.argv` reads the argv
        // installed for this run and `process.args` projects that same list
        // through the shared `jet_process_args_view` kernel, and
        // `process.exit` / `sys.atexit` / `sys.stop` drive its own exit and
        // cleanup path.
        // `process.run` is marshalled by the interpreter ambient through the
        // same Process Prelude as AOT and Cranelift. The authority argument
        // is ordinary data at this boundary; sema has already checked it is
        // the named `Authority` carrier.
        ("core.process", "argv" | "args" | "cmd" | "exit" | "run" | "pipeline" | "workspace")
        | ("core.sys", "atexit" | "stop") => None,
        // #2003: the interpreter ambient marshals these three through the one
        // CoreHost accessor over Jet's logical environment table — the same
        // owner AOT and the resident JIT read (`jet-jit` `ambient_interp`
        // `("core.sys", "get" | "set" | "home_dir")`). `env.set` arrives as
        // the `EnvSet` host call, which the TIR evaluator marshals to that
        // same `core.sys.set` adapter.
        ("core.sys", "get" | "set" | "home_dir") => None,
        // The platform-family fact is the one OS fact implemented by the
        // ambient interpreter; other core.sys facts remain native-only below.
        ("core.sys", "family") => None,
        // Environment surfaces with no ambient arm. Without one the evaluator
        // falls through to the comptime host-env effect, which reads the
        // compiler's own `std::env` instead of Jet's table — so these must
        // stay native-only or one program would see two environments.
        ("core.sys", "vars" | "expand") => Some("an environment read"),
        ("core.sys", "unset") => Some("an environment change"),
        ("core.sys", "current_dir" | "set_current_dir") => {
            Some("a working-directory read or change")
        }
        // #2027: the evaluator has the interrupt ambient this refusal predated.
        // It arms through the ONE shared count
        // (`Codegen/TIR/eval/mod.rs::register_interrupt_callback`, which calls
        // `interrupt_runtime::jet_interrupt_arm`), keeps the handler as a
        // callable-arena index, and drains through the one Prelude rule
        // (`jet_interrupt_dispatch`) at every statement and loop boundary —
        // including a bare `loop { }` (`eval/stmts.rs::exec_infinite`), which is
        // the shape a signalled program actually waits in. A handler's terminal
        // transfer is ended the way `Prelude/CoreLib/Top/Interrupt.rs` names for
        // this tier: the drain returns the diagnostic, so `process.exit` ends the
        // run with its code and a handler panic reports and stops with 70.
        ("core.sys", "on_interrupt") => None,
        ("core.sys", _) => Some("an OS fact or process control call"),
        ("core.process", _) => Some("a process launch or an early exit"),
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
            (
                core_modules.get(alias)?.as_str(),
                method.as_str(),
                *method_span,
            )
        }
        _ => return None,
    };

    process_leaf_feature(module, item).map(|feature| Boundary {
        feature: feature.to_string(),
        span: Some(span),
    })
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
        Stmt::Switch {
            arms, else_body, ..
        } => arms
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
        if matches!(arg.convention, AccessConvention::Write) && matches!(arg.expr, Expr::Ident(..))
        {
            return Some(Boundary {
                feature: "a `&` writeback argument passed to a function".to_string(),
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
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Field(inner, _, _) => expr_mut_arg(inner, interpreted_functions, core_items),
        Expr::Binary(_, left, right, _) => expr_mut_arg(left, interpreted_functions, core_items)
            .or_else(|| expr_mut_arg(right, interpreted_functions, core_items)),
        Expr::Index { base, index, .. } => expr_mut_arg(base, interpreted_functions, core_items)
            .or_else(|| expr_mut_arg(index, interpreted_functions, core_items)),
        _ => None,
    }
}
