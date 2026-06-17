//! E2-M4 — `jet dev` whole-program interpreter driver.
//!
//! This is the dev-loop convenience layer (D-DEV1…D-DEV4): it re-checks and
//! re-runs the entry file on every save, streaming output, for sub-200ms
//! feedback. It does NOT introduce a second interpreter — it reuses the M9.5
//! comptime tree-walker (`crate::comptime`) to execute `fn main()`. The bytes
//! it produces are identical to the compiled program (I2); the differential
//! battery in `tests/dev.rs` is the enforcement.
//!
//! Hard line (I2/I3): nothing here ever produces a release artifact. `jet
//! build`/`jet run` never touch this path. When the interpreter can't run a
//! program (FFI, tasks/channels, `@unsafe`/`core.mem`, native-only std), it
//! stops with **E2201** naming the feature and `jet build`/`jet run` — unless
//! the user opted in with "try anyway" (D-DEV1), which runs past the boundary
//! with no guarantees.

use std::collections::HashMap;

use crate::ast::{Func, Item, ProgramBundle, Stmt};
use crate::diag::{Diagnostic, Span};

/// What a single dev iteration produced: either problems to show (front-end
/// diagnostics, an E2201 boundary note, or an E2202/E0956 run-time stop), or
/// the program's buffered output.
#[derive(Debug, Clone)]
pub enum RunOutcome {
    /// The program ran to completion in the interpreter. `stdout`/`stderr`
    /// are byte-identical to the compiled program.
    Ran { stdout: String, stderr: String },
    /// The front end (or the interpreter) reported problems; show them as in
    /// batch compilation. Includes E2201 boundary notes and E2202 fuel stops.
    Problems(Vec<Diagnostic>),
}

/// A named feature the dev interpreter cannot execute (D-DEV1). The boundary
/// scan returns the first one it finds so the E2201 note can name it.
struct Boundary {
    /// Plain-language feature name, e.g. "spawns a task".
    feature: String,
    /// Where in the source the feature appears (best-effort).
    span: Option<Span>,
}

/// Build the E2201 boundary diagnostic: name the feature and point at the
/// real execution path (`jet build` / `jet run`).
fn boundary_diag(b: &Boundary) -> Diagnostic {
    Diagnostic::error(
        "E2201",
        format!(
            "`jet dev` can't interpret this program yet — it {}",
            b.feature
        ),
        "`jet dev` runs your program in a built-in interpreter for instant feedback, but that interpreter doesn't cover every feature; this one needs the real native build"
            .to_string(),
        "run `jet build` then the binary, or `jet run <file>` to compile and run it; `jet dev` will keep showing checks live"
            .to_string(),
        b.span,
    )
}

/// Scan the whole bundle for the first feature the interpreter can't run
/// (D-DEV1). Pure walk over the typed AST — no execution.
fn boundary_scan(bundle: &ProgramBundle) -> Option<Boundary> {
    for module in &bundle.modules {
        // Native std modules whose results aren't pure/deterministic enough to
        // interpret. The interpreter supports `print`/`eprint` only; anything
        // that reaches the filesystem, network, clock, RNG, environment, or
        // process table needs the real build.
        for imp in &module.imports {
            if let crate::ast::ImportKind::Module(name, span) = &imp.kind {
                if let Some(feature) = native_module_feature(name) {
                    return Some(Boundary {
                        feature: feature.to_string(),
                        span: Some(*span),
                    });
                }
            }
        }
        for item in &module.items {
            match item {
                Item::ExternRust(b) => {
                    return Some(Boundary {
                        feature: "calls into Rust code through `extern rust`".to_string(),
                        span: Some(b.span),
                    });
                }
                Item::CModule(c) => {
                    return Some(Boundary {
                        feature: "calls into a C library".to_string(),
                        span: Some(c.span),
                    });
                }
                Item::Func(f) => {
                    if f.is_unsafe {
                        return Some(Boundary {
                            feature: "uses an `@unsafe` function".to_string(),
                            span: Some(f.name_span),
                        });
                    }
                    if let Some(b) = scan_stmts_for_unsafe(&f.body) {
                        return Some(b);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Map a `use core.<x>` module name to the boundary feature it represents, or
/// `None` if the interpreter can run it (only `core.io` reaches IO we support,
/// and even there `input`/`read_all_input` are non-deterministic — but those
/// surface naturally as E0956 if reached, keeping the scan conservative).
fn native_module_feature(name: &str) -> Option<&'static str> {
    match name {
        "core.tasks" => Some("spawns a task or uses a channel"),
        "core.mem" => Some("uses the low-level `core.mem` tier"),
        "core.fs" => Some("reads or writes files"),
        "core.env" => Some("reads the environment"),
        "core.process" => Some("runs another process or exits early"),
        "core.random" => Some("uses random numbers"),
        "core.time" => Some("reads the clock or sleeps"),
        _ => None,
    }
}

/// Find the first `@unsafe { … }` block anywhere in a statement list.
fn scan_stmts_for_unsafe(stmts: &[Stmt]) -> Option<Boundary> {
    for s in stmts {
        if let Some(b) = scan_stmt_for_unsafe(s) {
            return Some(b);
        }
    }
    None
}

fn scan_stmt_for_unsafe(s: &Stmt) -> Option<Boundary> {
    match s {
        Stmt::Unsafe { span, .. } => Some(Boundary {
            feature: "uses an `@unsafe` block".to_string(),
            span: Some(*span),
        }),
        Stmt::If(ifs) => scan_if_for_unsafe(ifs),
        Stmt::While { body, .. } | Stmt::Loop(body, _) => scan_stmts_for_unsafe(body),
        Stmt::For { body, .. } => scan_stmts_for_unsafe(body),
        Stmt::Switch {
            arms, else_body, ..
        } => {
            for a in arms {
                if let Some(b) = scan_stmts_for_unsafe(&a.body) {
                    return Some(b);
                }
            }
            else_body.as_ref().and_then(|b| scan_stmts_for_unsafe(b))
        }
        _ => None,
    }
}

fn scan_if_for_unsafe(ifs: &crate::ast::IfStmt) -> Option<Boundary> {
    if let Some(b) = scan_stmts_for_unsafe(&ifs.then_body) {
        return Some(b);
    }
    match &ifs.else_branch {
        Some(crate::ast::ElseBranch::ElseIf(inner)) => scan_if_for_unsafe(inner),
        Some(crate::ast::ElseBranch::Else(body)) => scan_stmts_for_unsafe(body),
        None => None,
    }
}

/// Collect every top-level function across all modules into the flat name→func
/// map the comptime evaluator expects. (Module-qualified user functions aren't
/// dev-interpreted yet; they surface as E0956 if called.)
fn collect_funcs(bundle: &ProgramBundle) -> HashMap<String, &Func> {
    let mut funcs = HashMap::new();
    for module in &bundle.modules {
        for item in &module.items {
            if let Item::Func(f) = item {
                funcs.entry(f.name.clone()).or_insert(f);
            }
        }
    }
    funcs
}

/// Run a *checked* bundle in the interpreter (E2-M4). The caller has already
/// run the front end and confirmed there are no errors. `try_anyway` (D-DEV1)
/// skips the E2201 boundary scan and attempts execution with no guarantees.
pub fn run_checked(bundle: &ProgramBundle, try_anyway: bool) -> RunOutcome {
    if !try_anyway {
        if let Some(b) = boundary_scan(bundle) {
            return RunOutcome::Problems(vec![boundary_diag(&b)]);
        }
    }
    let funcs = collect_funcs(bundle);
    let main = match funcs.get("main") {
        Some(f) => *f,
        None => {
            return RunOutcome::Problems(vec![Diagnostic::error(
                "E2201",
                "`jet dev` needs a `main` function to run".to_string(),
                "`jet dev` runs a program; a library with no `main` has nothing to execute"
                    .to_string(),
                "add `fn main() { … }`, or use `jet check <file>` to look for problems without running"
                    .to_string(),
                None,
            )]);
        }
    };
    let base_dir = &bundle.project_root;
    let mut sink = crate::comptime::DevSink::new();
    match crate::comptime::run_main(main, &funcs, base_dir, &mut sink) {
        Ok(()) => RunOutcome::Ran {
            stdout: sink.stdout,
            stderr: sink.stderr,
        },
        Err(d) => RunOutcome::Problems(vec![d]),
    }
}

/// One iteration of the `jet dev` watch loop, factored out so it can be
/// golden-tested without the long-running file watcher (the outer loop is a
/// thin shell around this). Loads + checks the file exactly like batch
/// compilation (D-DEV: identical diagnostics), then either runs it in the
/// interpreter or explains the boundary.
pub fn dev_iteration(file: &str, try_anyway: bool) -> RunOutcome {
    match crate::loader::load_entry_with_overlay(file, None, false) {
        Ok(mut bundle) => {
            let diags = crate::sema::check_bundle(&mut bundle, crate::sema::CompileMode::Run);
            let errors: Vec<Diagnostic> = diags
                .into_iter()
                .filter(|d| matches!(d.severity, crate::diag::Severity::Error))
                .collect();
            if !errors.is_empty() {
                return RunOutcome::Problems(errors);
            }
            run_checked(&bundle, try_anyway)
        }
        Err(diags) => RunOutcome::Problems(diags),
    }
}
