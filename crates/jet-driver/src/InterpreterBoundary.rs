//! Shared source-debugger execution boundary.
//!
//! D-ARCH-SOURCE1=A: the source debugger classifies the same typed program
//! as the compiler. Keeping the pure AST walk in the driver prevents the
//! debugger from depending on the root host or inventing a second boundary
//! vocabulary.

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{
    core_import_maps, AccessConvention, CallArg, Expr, ImportKind, Item, ProgramBundle, Stmt,
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterpreterInvocation {
    RunInterpret,
    DevInterpret,
    RunDefault,
    DevDefault,
}

impl InterpreterInvocation {
    pub const fn command(self) -> &'static str {
        match self {
            Self::RunInterpret => "jet run --interpret",
            Self::DevInterpret => "jet dev --interpret",
            Self::RunDefault => "jet run",
            Self::DevDefault => "jet dev",
        }
    }

    pub const fn uses_interpreter(self) -> bool {
        matches!(self, Self::RunInterpret | Self::DevInterpret)
    }
}

struct Boundary {
    /// A NOUN PHRASE naming the construct, and nothing else: it is rendered
    /// as the object of "it uses …" by `debug_boundary_scan` (E2203), which
    /// owns the whole sentence. A feature that carries its own clause would
    /// splice two sentences together, so never append a reason, a "which …"
    /// tail, or a trailing "yet" here. The wrapper never parses this string
    /// back apart.
    feature: String,
    span: Option<Span>,
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
    // Whether the MIR evaluator runs a callee's frame depends on that callee's
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
                let mut imported_modules = imported_modules.into_iter().collect::<Vec<_>>();
                imported_modules.sort_unstable_by(|left, right| {
                    left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1))
                });
                if let Some(feature) = imported_modules
                    .iter()
                    .find_map(|(_, name)| native_module_feature(name, true))
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
                    if let Some(boundary) =
                        scan_stmts_for_process_edge(&function.body, &core_modules, &core_items)
                    {
                        return Some(boundary);
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
        // `jet debug` classifies `core.process` per leaf: argv/args and the
        // supported process prelude stay in the interpreter, while an
        // unregistered process operation remains a native boundary below.
        // `core.sys` remains import-level for now because its source stepper
        // cannot distinguish an ambient read from unsupported process control
        // without executing the call.
        "core.sys" if debug_impure => Some("an environment read"),
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
/// The Core-call registry is the one source of truth for ambient interpreter
/// routes. A newly registered `core.sys` member stays native-only until its
/// registry row and evaluator route both declare an executable ambient path.
fn process_leaf_feature(module: &str, item: &str) -> Option<&'static str> {
    match (module, item) {
        // Run by the shared evaluator itself: `process.argv` reads the argv
        // installed for this run and `process.args` projects that same list
        // through the shared `jet_process_args_view` kernel.
        // `process.run` is marshalled by the interpreter ambient through the
        // same Process Prelude as AOT and Cranelift. The authority argument
        // is ordinary data at this boundary; sema has already checked it is
        // the named `Authority` carrier.
        ("core.process", "argv" | "args" | "cmd" | "exit" | "run" | "pipeline" | "workspace") => {
            None
        }
        ("core.sys", _) => {
            let ambient = jet_foundation::Syntax::core_call_ambient_routes()
                .iter()
                .any(|(known_module, known_item)| *known_module == module && *known_item == item);
            if ambient {
                None
            } else {
                Some("an OS fact or process control call")
            }
        }
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

/// Does the MIR evaluator itself run the frame this direct call names?
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
