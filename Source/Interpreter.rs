//! E2-M4 — `jet dev` whole-program interpreter driver.
//!
//! This is the dev-loop convenience layer (D-DEV1…D-DEV4): it re-checks and
//! re-runs the entry file on every save, streaming output, for sub-200ms
//! feedback. It does NOT introduce a second interpreter — it reuses the M9.5
//! comptime tree-walker (`crate::comptime`) to execute `fn run()`. The bytes
//! it produces are identical to the compiled program (I2); the differential
//! battery in `tests/dev.rs` is the enforcement.
//!
//! Hard line (I2/I3): nothing here ever produces a release artifact. `jet
//! build`/`jet run` never touch this path. When the interpreter can't run a
//! program (FFI, tasks/channels, `@unsafe`/`core.mem`, native-only Core), it
//! stops with **E2201** naming the feature and `jet build`/`jet run` — unless
//! the user opted in with "try anyway" (D-DEV1), which runs past the boundary
//! with no guarantees.

use std::collections::HashMap;

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{Expr, Func, Item, ProgramBundle, Stmt};

// c139: RunOutcome moved to jet-foundation so the jet-jit/ sibling crate
// can implement JitBackend without a dep cycle. Re-exported here so callers
// using `jet::Interpreter::RunOutcome` still work unchanged.
pub use jet_foundation::JitBackend::RunOutcome;

/// c77 (D-DEVMODE1=A): how `jet dev` should react to a save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DevMode {
    /// A program that finishes on its own — rerun it from scratch on each save.
    RunToCompletion,
    /// A program that stays up (a top-level `loop`, or a `task.spawn`) — a
    /// type-stable edit takes the swap path, a type-changing edit announces a
    /// clean restart (D-HOTSWAP1).
    Resident,
}

/// c77 (D-DEVMODE1=A): auto-detect whether `run` runs to completion or stays
/// resident. A `run` whose body contains a top-level `loop { … }` or a
/// `*.spawn(...)` call (the `core.tasks` spawn surface) is `Resident`;
/// everything else is `RunToCompletion`. The scan only looks at `run`'s own
/// statement list (top level) per the D-DEVMODE1 Q2 rule — a `loop` buried
/// inside a helper does not make a program resident.
pub fn detect_dev_mode(bundle: &ProgramBundle) -> DevMode {
    let funcs = collect_funcs(bundle);
    if let Some(run) = funcs.get("run") {
        for stmt in &run.body {
            if stmt_is_resident(stmt) {
                return DevMode::Resident;
            }
        }
    }
    DevMode::RunToCompletion
}

/// A single top-level statement that marks a program resident: a `loop { … }`
/// or any statement whose expression is (or contains, top-level) a `.spawn(…)`
/// method call.
fn stmt_is_resident(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Loop { .. } => true,
        Stmt::Expr(e) => expr_has_spawn(e),
        Stmt::Val(b) => expr_has_spawn(&b.init),
        Stmt::Assign { value, .. } => expr_has_spawn(value),
        _ => false,
    }
}

/// True when `e` is, at this level, a `*.spawn(...)` method call — the
/// `core.tasks` resident-task surface (`tasks.spawn(() => …)`).
fn expr_has_spawn(e: &Expr) -> bool {
    matches!(e, Expr::MethodCall { method, .. } if method == "spawn")
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

/// D-DBG3: the debugger's boundary scan. The `jet debug` source-level stepper
/// drives this same dev interpreter, so it declines the same features — but
/// with **E2203** (debug-specific) so the message names `jet debug` and points
/// at the real build (the native-backend follow-on, D-DBG3 step 2). Returns
/// `None` when the whole program is steppable.
pub fn debug_boundary_scan(bundle: &ProgramBundle) -> Option<Diagnostic> {
    boundary_scan(bundle).map(|b| {
        Diagnostic::error(
            "E2203",
            format!("`jet debug` can't step through this program yet — it {}", b.feature),
            "`jet debug` steps your program in the same interpreter `jet dev` uses; this feature touches threads, foreign code, raw memory, or the outside world, which the source-level stepper doesn't cover yet"
                .to_string(),
            "run `jet build` then the binary, or `jet run <file>` to compile and run it; remove the unsupported feature to step the rest, or wait for the native-debugger milestone (D-DBG3 step 2)"
                .to_string(),
            b.span,
        )
    })
}

/// Scan the whole bundle for the first feature the interpreter can't run
/// (D-DEV1). Pure walk over the typed AST — no execution.
fn boundary_scan(bundle: &ProgramBundle) -> Option<Boundary> {
    for module in &bundle.modules {
        // Native Core modules whose results aren't pure/deterministic enough to
        // interpret. The interpreter supports `print`/`eprint` only; anything
        // that reaches the filesystem, network, clock, RNG, environment, or
        // process table needs the real build.
        for imp in &module.imports {
            if let crate::AST::ImportKind::Module(name, span) = &imp.kind {
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
                            feature: "uses an `#Unsafe` function".to_string(),
                            span: Some(f.name_span),
                        });
                    }
                    // D-CLIFLAG1: `fn run(args: T)` — the typed entry-signature
                    // CLI surface. The real build parses `io.args()` into `T`
                    // before calling `run`; the interpreter has no argv to
                    // parse from and no synthesis for the defaults, so it has
                    // nothing to bind `args` to. Declining honestly here beats
                    // running with `args` unbound (which previously surfaced as
                    // a confusing, unrelated E0956 deep in the body, or — worse
                    // — silently printed the wrong value, c139 JIT/interpreter
                    // parity finding).
                    if f.name == "run" && !f.params.is_empty() {
                        return Some(Boundary {
                            feature: "uses a typed CLI entry signature (`fn run(args: T)`)"
                                .to_string(),
                            span: Some(f.name_span),
                        });
                    }
                    if let Some(b) = scan_stmts_for_unsafe(&f.body) {
                        return Some(b);
                    }
                    // c77 (Q2 hard rule): a call passing a `mut`/`^move`
                    // argument asks for writeback / ownership-move binding the
                    // scalar-by-value tree-walker doesn't faithfully reproduce
                    // (e.g. field access on a moved struct param), so its output
                    // could diverge from the compiled build. Stop honestly at
                    // the boundary rather than risk a silent miscompile.
                    if let Some(b) = scan_stmts_for_mut_arg(&f.body) {
                        return Some(b);
                    }
                    // c77 (Q2 hard rule): interpolating a whole struct/enum
                    // value (`"{g}"`) formats via Rust's derived `Debug` in the
                    // compiled build, which the interpreter's value printer does
                    // not reproduce byte-for-byte. Stop at the boundary rather
                    // than diverge.
                    if let Some(b) = scan_stmts_for_struct_interp(&f.body) {
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
            feature: "uses an `#Unsafe` block".to_string(),
            span: Some(*span),
        }),
        Stmt::If(ifs) => scan_if_for_unsafe(ifs),
        Stmt::While { body, .. } | Stmt::Loop { body, .. } | Stmt::CountedLoop { body, .. } => {
            scan_stmts_for_unsafe(body)
        }
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

fn scan_if_for_unsafe(ifs: &crate::AST::IfStmt) -> Option<Boundary> {
    if let Some(b) = scan_stmts_for_unsafe(&ifs.then_body) {
        return Some(b);
    }
    match &ifs.else_branch {
        Some(crate::AST::ElseBranch::ElseIf(inner)) => scan_if_for_unsafe(inner),
        Some(crate::AST::ElseBranch::Else(body)) => scan_stmts_for_unsafe(body),
        None => None,
    }
}

/// c77: find the first call passing a `mut` (Write-convention) argument — the
/// writeback the scalar-by-value tree-walker doesn't perform. Walks bodies and
/// the expressions inside them.
fn scan_stmts_for_mut_arg(stmts: &[Stmt]) -> Option<Boundary> {
    for s in stmts {
        if let Some(b) = scan_stmt_for_mut_arg(s) {
            return Some(b);
        }
    }
    None
}

fn scan_stmt_for_mut_arg(s: &Stmt) -> Option<Boundary> {
    match s {
        Stmt::Expr(e) => expr_mut_arg(e),
        Stmt::Val(b) => expr_mut_arg(&b.init),
        Stmt::Assign { value, .. } => expr_mut_arg(value),
        Stmt::Return(Some(e), _) => expr_mut_arg(e),
        Stmt::If(ifs) => scan_if_for_mut_arg(ifs),
        Stmt::While { cond, body, .. } => {
            expr_mut_arg(cond).or_else(|| scan_stmts_for_mut_arg(body))
        }
        Stmt::CountedLoop { cond, body, .. } => {
            expr_mut_arg(cond).or_else(|| scan_stmts_for_mut_arg(body))
        }
        Stmt::Loop { body, .. } => scan_stmts_for_mut_arg(body),
        Stmt::For { body, .. } => scan_stmts_for_mut_arg(body),
        Stmt::Switch {
            arms, else_body, ..
        } => {
            for a in arms {
                if let Some(b) = scan_stmts_for_mut_arg(&a.body) {
                    return Some(b);
                }
            }
            else_body.as_ref().and_then(|b| scan_stmts_for_mut_arg(b))
        }
        _ => None,
    }
}

fn scan_if_for_mut_arg(ifs: &crate::AST::IfStmt) -> Option<Boundary> {
    if let Some(b) = expr_mut_arg(&ifs.cond) {
        return Some(b);
    }
    if let Some(b) = scan_stmts_for_mut_arg(&ifs.then_body) {
        return Some(b);
    }
    match &ifs.else_branch {
        Some(crate::AST::ElseBranch::ElseIf(inner)) => scan_if_for_mut_arg(inner),
        Some(crate::AST::ElseBranch::Else(body)) => scan_stmts_for_mut_arg(body),
        None => None,
    }
}

/// Does this expression (or a subexpression) pass a `mut`/`^move` argument
/// *bound to a named variable* to a call? Those are the conventions whose
/// caller-side binding (writeback for `mut`, ownership-move for `^`) the
/// scalar-by-value tree-walker doesn't reproduce; moving a literal is fine.
fn expr_mut_arg(e: &Expr) -> Option<Boundary> {
    use crate::AST::AccessConvention;
    let boundary = |conv: AccessConvention, span: Span| {
        let feature = match conv {
            AccessConvention::Write => {
                "passes a `~` argument to a function (writeback isn't interpreted yet)"
            }
            _ => "passes a moved (`^`) variable to a function (move binding isn't interpreted yet)",
        };
        Some(Boundary {
            feature: feature.to_string(),
            span: Some(span),
        })
    };
    // A call arg is a boundary when it is `mut`/`^` on a named variable.
    let arg_boundary = |a: &crate::AST::CallArg| -> Option<Boundary> {
        if matches!(
            a.convention,
            AccessConvention::Write | AccessConvention::Move
        ) && matches!(a.expr, Expr::Ident(..))
        {
            return boundary(a.convention.clone(), a.span);
        }
        expr_mut_arg(&a.expr)
    };
    match e {
        Expr::Call(c) => {
            for a in &c.args {
                if let Some(b) = arg_boundary(a) {
                    return Some(b);
                }
            }
            None
        }
        Expr::MethodCall { receiver, args, .. } => {
            if let Some(b) = expr_mut_arg(receiver) {
                return Some(b);
            }
            for a in args {
                if let Some(b) = arg_boundary(a) {
                    return Some(b);
                }
            }
            None
        }
        Expr::CallValue { callee, args, .. } => {
            if let Some(b) = expr_mut_arg(callee) {
                return Some(b);
            }
            for a in args {
                if let Some(b) = arg_boundary(a) {
                    return Some(b);
                }
            }
            None
        }
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Field(inner, _, _) => expr_mut_arg(inner),
        Expr::Binary(_, l, r, _) => expr_mut_arg(l).or_else(|| expr_mut_arg(r)),
        Expr::Index { base, index, .. } => expr_mut_arg(base).or_else(|| expr_mut_arg(index)),
        _ => None,
    }
}

/// c77: flag a function body that interpolates a whole struct/enum value
/// (`"{g}"` where `g` was bound to a `Type { … }`/`Type.Variant(…)` literal).
/// Tracks locals bound to struct/enum literals as it walks, then flags an
/// interpolation of such a local.
fn scan_stmts_for_struct_interp(stmts: &[Stmt]) -> Option<Boundary> {
    let mut struct_locals: std::collections::HashSet<String> = std::collections::HashSet::new();
    scan_block_for_struct_interp(stmts, &mut struct_locals)
}

fn scan_block_for_struct_interp(
    stmts: &[Stmt],
    locals: &mut std::collections::HashSet<String>,
) -> Option<Boundary> {
    for s in stmts {
        match s {
            Stmt::Val(b) => {
                if let Some(found) = expr_struct_interp(&b.init, locals) {
                    return Some(found);
                }
                if !b.name.is_empty() && expr_is_struct_or_enum_lit(&b.init) {
                    locals.insert(b.name.clone());
                }
            }
            Stmt::Expr(e) => {
                if let Some(found) = expr_struct_interp(e, locals) {
                    return Some(found);
                }
            }
            Stmt::Assign { value, .. } => {
                if let Some(found) = expr_struct_interp(value, locals) {
                    return Some(found);
                }
            }
            Stmt::Return(Some(e), _) => {
                if let Some(found) = expr_struct_interp(e, locals) {
                    return Some(found);
                }
            }
            Stmt::If(ifs) => {
                if let Some(found) = scan_if_for_struct_interp(ifs, locals) {
                    return Some(found);
                }
            }
            Stmt::While { body, .. }
            | Stmt::Loop { body, .. }
            | Stmt::For { body, .. }
            | Stmt::CountedLoop { body, .. } => {
                if let Some(found) = scan_block_for_struct_interp(body, locals) {
                    return Some(found);
                }
            }
            Stmt::Switch {
                arms, else_body, ..
            } => {
                for a in arms {
                    if let Some(found) = scan_block_for_struct_interp(&a.body, locals) {
                        return Some(found);
                    }
                }
                if let Some(b) = else_body {
                    if let Some(found) = scan_block_for_struct_interp(b, locals) {
                        return Some(found);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn scan_if_for_struct_interp(
    ifs: &crate::AST::IfStmt,
    locals: &mut std::collections::HashSet<String>,
) -> Option<Boundary> {
    if let Some(b) = expr_struct_interp(&ifs.cond, locals) {
        return Some(b);
    }
    if let Some(b) = scan_block_for_struct_interp(&ifs.then_body, locals) {
        return Some(b);
    }
    match &ifs.else_branch {
        Some(crate::AST::ElseBranch::ElseIf(inner)) => scan_if_for_struct_interp(inner, locals),
        Some(crate::AST::ElseBranch::Else(body)) => scan_block_for_struct_interp(body, locals),
        None => None,
    }
}

fn expr_is_struct_or_enum_lit(e: &Expr) -> bool {
    matches!(
        e,
        Expr::StructLit { .. } | Expr::EnumLit { .. } | Expr::TupleLit(..)
    )
}

/// Find a `"{local}"` interpolation of a known struct/enum local anywhere in
/// `e` (recurses into calls, operators, and nested string parts).
fn expr_struct_interp(e: &Expr, locals: &std::collections::HashSet<String>) -> Option<Boundary> {
    match e {
        Expr::Str(parts, span) => {
            for p in parts {
                if let crate::AST::StrPart::Interp(inner, _) = p {
                    if let Expr::Ident(name, _) = inner.as_ref() {
                        if locals.contains(name) {
                            return Some(Boundary {
                                feature: "prints a whole struct or enum value via `{…}` interpolation (its format isn't interpreted yet)".to_string(),
                                span: Some(*span),
                            });
                        }
                    }
                    if let Some(b) = expr_struct_interp(inner, locals) {
                        return Some(b);
                    }
                }
            }
            None
        }
        Expr::Call(c) => c
            .args
            .iter()
            .find_map(|a| expr_struct_interp(&a.expr, locals)),
        Expr::MethodCall { receiver, args, .. } => {
            expr_struct_interp(receiver, locals).or_else(|| {
                args.iter()
                    .find_map(|a| expr_struct_interp(&a.expr, locals))
            })
        }
        Expr::CallValue { callee, args, .. } => expr_struct_interp(callee, locals).or_else(|| {
            args.iter()
                .find_map(|a| expr_struct_interp(&a.expr, locals))
        }),
        Expr::Unary(_, inner, _)
        | Expr::IncDec { operand: inner, .. }
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Field(inner, _, _) => expr_struct_interp(inner, locals),
        Expr::Binary(_, l, r, _) => {
            expr_struct_interp(l, locals).or_else(|| expr_struct_interp(r, locals))
        }
        Expr::Index { base, index, .. } => {
            expr_struct_interp(base, locals).or_else(|| expr_struct_interp(index, locals))
        }
        _ => None,
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

/// c139 JIT/interpreter-parity: extend `collect_funcs` with everything else
/// `jet dev` needs to run whole programs at parity with the real build —
/// D-MOD2 code-module namespaced functions (both the real `alias__fn`
/// mangling and, when unclaimed, the bare name — covers a private sibling
/// call inside the module and a `use mod.item` selective import in one move,
/// with `use mod.item as alias` handled explicitly below), user-written
/// instance/associated methods, D-FIELDPOL1 computed fields, and
/// D-RANGETYPE1/D-DIST1 distinct-type constructors. Consts/`comptime`
/// bindings are pre-evaluated into `globals` from sema's own `ConstDef::ct`
/// (I2: the exact value baked into the real build, not a re-derivation).
fn collect_funcs_and_info<'a>(
    bundle: &'a ProgramBundle,
) -> (HashMap<String, &'a Func>, crate::Comptime::ProgramInfo<'a>) {
    let mut funcs: HashMap<String, &Func> = HashMap::new();
    let mut info = crate::Comptime::ProgramInfo::empty();
    for module in &bundle.modules {
        walk_items_for_interp(&module.items, &mut funcs, &mut info);
    }
    // D-SELIMPORT1=A: `use mod.item as alias` for a *local* code module (not
    // `core`/`jet`) — the bare-name fallback above already covers an
    // unaliased `use mod.item`.
    for module in &bundle.modules {
        for imp in &module.imports {
            let crate::AST::ImportKind::Unqualified {
                module_alias,
                items,
                ..
            } = &imp.kind
            else {
                continue;
            };
            if module_alias == "core" || module_alias == "jet" {
                continue;
            }
            for (orig, alias_opt) in items {
                let Some(local) = alias_opt else { continue };
                let qualified = format!("{}__{}", module_alias, orig);
                if let Some(f) = funcs.get(qualified.as_str()).copied() {
                    funcs.entry(local.clone()).or_insert(f);
                }
            }
        }
    }
    // Core module aliases (`use core.math as math`, `use core.{sqrt}`),
    // merged across every module the same way `funcs` already is — dev mode
    // has no per-module scoping (see the doc comment on `collect_funcs`).
    for module in &bundle.modules {
        for imp in &module.imports {
            if let Some(core_module) = imp.core_module_path() {
                info.core_imports
                    .entry(imp.import_alias())
                    .or_insert(core_module);
                continue;
            }
            if let crate::AST::ImportKind::Unqualified {
                module_alias,
                items,
                ..
            } = &imp.kind
            {
                if module_alias == "core" || module_alias == "jet" {
                    for (orig, alias_opt) in items {
                        let local = alias_opt.clone().unwrap_or_else(|| orig.clone());
                        let full = format!("core.{}", orig);
                        if crate::Syntax::is_known_core_module(&full) {
                            info.core_imports.entry(local).or_insert(full);
                        }
                    }
                }
            }
        }
    }
    // Top-level `const`/`comptime` bindings: sema already evaluated every
    // `comptime NAME = …` into `ConstDef::ct` while checking the program (the
    // caller of `run_checked` guarantees the front end already ran) — reuse
    // that value rather than re-evaluating, so a `jet dev` run of the const
    // matches the real build bit-for-bit (I2).
    for module in &bundle.modules {
        for item in &module.items {
            if let Item::Const(c) = item {
                if let Some(v) = &c.ct {
                    info.globals.entry(c.name.clone()).or_insert_with(|| v.clone());
                }
            }
        }
    }
    (funcs, info)
}

/// One item-list pass for [`collect_funcs_and_info`]: top-level functions,
/// `impl`/in-struct instance methods, computed fields, distinct-type
/// constructors, and one level of D-MOD2 inline/generic-instantiated code
/// modules (their own `Item::Func`s only — module-in-module nesting isn't a
/// shape any current example produces, so it isn't walked recursively).
fn walk_items_for_interp<'a>(
    items: &'a [Item],
    funcs: &mut HashMap<String, &'a Func>,
    info: &mut crate::Comptime::ProgramInfo<'a>,
) {
    for item in items {
        match item {
            Item::Func(f) => {
                funcs.entry(f.name.clone()).or_insert(f);
            }
            Item::Impl(i) => {
                for m in &i.methods {
                    info.methods
                        .entry((i.type_name.clone(), m.name.clone()))
                        .or_insert(m);
                }
            }
            Item::Struct(s) => {
                for m in &s.methods {
                    info.methods
                        .entry((s.name.clone(), m.name.clone()))
                        .or_insert(m);
                }
                for blk in &s.trait_impls {
                    for m in &blk.methods {
                        info.methods
                            .entry((s.name.clone(), m.name.clone()))
                            .or_insert(m);
                    }
                }
                for field in &s.fields {
                    if let Some(expr) = &field.computed {
                        info.computed_fields
                            .entry((s.name.clone(), field.name.clone()))
                            .or_insert(expr.as_ref());
                    }
                }
            }
            Item::Enum(e) => {
                for m in &e.methods {
                    info.methods
                        .entry((e.name.clone(), m.name.clone()))
                        .or_insert(m);
                }
                for blk in &e.trait_impls {
                    for m in &blk.methods {
                        info.methods
                            .entry((e.name.clone(), m.name.clone()))
                            .or_insert(m);
                    }
                }
            }
            Item::Distinct(d) => {
                info.distinct_ranges
                    .entry(d.name.clone())
                    .or_insert(d.range.map(|(lo, hi, _)| (lo, hi)));
            }
            Item::UnitFamily(uf) => {
                for d in uf.distinct_defs() {
                    info.distinct_ranges
                        .entry(d.name.clone())
                        .or_insert(d.range.map(|(lo, hi, _)| (lo, hi)));
                }
            }
            Item::CodeModule(cm) => {
                if let Some(body) = &cm.body {
                    for it in body {
                        if let Item::Func(f) = it {
                            funcs
                                .entry(format!("{}__{}", cm.name, f.name))
                                .or_insert(f);
                            // Bare-name fallback: a private sibling call inside
                            // the module, or an unaliased `use mod.item`
                            // selective import — see `collect_funcs_and_info`.
                            funcs.entry(f.name.clone()).or_insert(f);
                        }
                    }
                }
            }
            _ => {}
        }
    }
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
    let (funcs, program) = collect_funcs_and_info(bundle);
    let main = match funcs.get("run") {
        Some(f) => *f,
        None => {
            return RunOutcome::Problems(vec![Diagnostic::error(
                "E2201",
                "`jet dev` needs a `run` function to run".to_string(),
                "`jet dev` runs a program; a library with no `run` has nothing to execute"
                    .to_string(),
                "add `fn run() { … }`, or use `jet check <file>` to look for problems without running"
                    .to_string(),
                None,
            )]);
        }
    };
    let base_dir = &bundle.project_root;
    let mut sink = crate::Comptime::DevSink::new();
    match crate::Comptime::run_main(main, &funcs, base_dir, &mut sink, &program) {
        Ok(()) => RunOutcome::Ran {
            stdout: sink.stdout,
            stderr: sink.stderr,
        },
        Err(d) => RunOutcome::Problems(vec![dev_boundary_from_comptime(d)]),
    }
}

/// c139 JIT-parity fix (2026-07-03): the dev interpreter IS the comptime
/// tree-walker (see module doc), so a construct it can't run leaks the
/// comptime evaluator's own E0956 ("unsupported")/E0951 ("impurity") codes —
/// correct for a real `comptime { }` block, but wrong voice here: the "compute
/// this at runtime" fix advice is nonsense when the user is already trying to
/// run this at runtime via `jet dev`. Rewrap as the dev-loop's own E2201
/// boundary diagnostic instead, preserving what construct tripped it.
fn dev_boundary_from_comptime(d: Diagnostic) -> Diagnostic {
    let detail = match d.code {
        "E0956" => d
            .what
            .strip_suffix(" can't run at compile time yet")
            .unwrap_or(&d.what)
            .replace(" at compile time", ""),
        "E0951" => "code that touches the outside world (network, filesystem, or environment)"
            .to_string(),
        _ => return d,
    };
    boundary_diag(&Boundary {
        feature: format!("uses {detail}, which isn't covered by the dev interpreter yet"),
        span: d.span,
    })
}

/// One iteration of the `jet dev` watch loop, factored out so it can be
/// golden-tested without the long-running file watcher (the outer loop is a
/// thin shell around this). Loads + checks the file exactly like batch
/// compilation (D-DEV: identical diagnostics), then runs via the selected
/// backend.
///
/// `use_interpreter` — D-JIT2=A: when false (default for `jet dev`), the
/// Cranelift tier-1 backend wraps the interpreter; when true (`--interpret`),
/// tier-0 interpreter only.
pub fn dev_iteration(file: &str, try_anyway: bool, use_interpreter: bool) -> RunOutcome {
    match crate::Loader::load_entry_with_overlay(file, None, false) {
        Ok(mut bundle) => {
            let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Run);
            let errors: Vec<Diagnostic> = diags
                .into_iter()
                .filter(|d| matches!(d.severity, crate::Diagnostics::Severity::Error))
                .collect();
            if !errors.is_empty() {
                return RunOutcome::Problems(errors);
            }
            dev_run_bundle(&bundle, try_anyway, use_interpreter)
        }
        Err(diags) => RunOutcome::Problems(diags),
    }
}

/// Run an already-checked bundle through the dev backend seam.
pub fn dev_run_bundle(
    bundle: &ProgramBundle,
    try_anyway: bool,
    use_interpreter: bool,
) -> RunOutcome {
    use crate::JitBackend::{InterpreterBackend, JitBackend};
    if use_interpreter {
        let mut backend = InterpreterBackend::new();
        backend.run(bundle, try_anyway)
    } else {
        let mut backend = jet_jit::CraneliftBackend::new(InterpreterBackend::new());
        backend.run(bundle, try_anyway)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse `src` into a bundle via a temp file (the only loader entry point).
    fn bundle_from(src: &str, tag: &str) -> ProgramBundle {
        let p = std::env::temp_dir().join(format!("jet_devmode_{tag}.jet"));
        std::fs::write(&p, src).unwrap();
        crate::Loader::load_entry(p.to_str().unwrap()).expect("bundle should load")
    }

    #[test]
    fn run_to_completion_is_the_default() {
        let b = bundle_from("fn run() {\n    print(\"hi\")\n}\n", "rtc");
        assert_eq!(detect_dev_mode(&b), DevMode::RunToCompletion);
    }

    #[test]
    fn top_level_loop_is_resident() {
        let b = bundle_from("fn run() {\n    loop {\n        break\n    }\n}\n", "loop");
        assert_eq!(detect_dev_mode(&b), DevMode::Resident);
    }

    #[test]
    fn loop_inside_a_helper_is_not_resident() {
        // Only a top-level `loop` in `run` makes a program resident; a loop in
        // a callee runs to completion.
        let src = "fn work() {\n    loop {\n        break\n    }\n}\nfn run() {\n    work()\n}\n";
        let b = bundle_from(src, "helper");
        assert_eq!(detect_dev_mode(&b), DevMode::RunToCompletion);
    }

    #[test]
    fn task_spawn_is_resident() {
        let src = "use core.tasks as tasks\nfn job() -> Int {\n    return 1\n}\nfn run() {\n    h :: tasks.spawn(() => job())\n    print(h.join())\n}\n";
        let b = bundle_from(src, "spawn");
        assert_eq!(detect_dev_mode(&b), DevMode::Resident);
    }

    #[test]
    fn jit_covers_task_examples() {
        for file in [
            "examples/features/concurrency/tasks.jet",
            "examples/features/concurrency/scheduler_spawn.jet",
        ] {
            let mut bundle =
                crate::Loader::load_entry(file).unwrap_or_else(|_| panic!("load {file}"));
            let diags = crate::Sema::check_bundle(&mut bundle, crate::Sema::CompileMode::Run);
            assert!(
                diags
                    .iter()
                    .all(|d| !matches!(d.severity, crate::Diagnostics::Severity::Error)),
                "{file} must type-check"
            );
            let detail = jet_jit::jit_covers_bundle_detail(&bundle);
            if !detail.is_empty() {
                eprintln!("{file}: {detail}");
                if file.contains("160") {
                    for line in jet_jit::jit_dump_main_stmts(&bundle) {
                        eprintln!("  {line}");
                    }
                }
            }
            assert!(detail.is_empty(), "{file} must be jit-covered: {detail}");
        }
    }
}
