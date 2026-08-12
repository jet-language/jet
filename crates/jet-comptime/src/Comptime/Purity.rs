//! Purity check: walk the call graph reachable from a comptime `init` and
//! reject the first impure call (IO, FFI) with the path that reached it
//! (E3401 — D-META-EFFECT1 c3: the one call-graph walk, shared with the
//! run-time `=[]=>` declaration check in `jet-sema/Sema/Purity.rs`, since
//! `jet-sema` depends on `jet-comptime` and not the other way around).
//! `embed_file`, `embed_bytes`, `find`, `panic`, and `require` are allowed.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{
    EnumLitArg, Expr, Func, LambdaBody, LValue, OrFallback, Pattern, Stmt, StrPart,
    StructPatField,
};

use super::Diagnostics::impurity_diag;

/// D-META-EFFECT1 c3 (merge review, card #1543): the one shared walker
/// serves two callers whose meaning of "reachable at this stage" differs on
/// exactly two statement kinds. Everything else about the walk is identical
/// between the two — this enum is the only place they may differ, per the
/// review's "one walk, explicit mode parameters" requirement.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PurityStage {
    /// jet-sema's `=[]=>` declared-effect check (also the `jet eval --pure`
    /// whole-program root check): checks what actually executes at run
    /// time. `#Impure(...)` bodies DO run at run time — the marker records
    /// and gates the ambient call, it doesn't erase it — so a
    /// declared-pure function must still be checked inside one; an empty
    /// declared effect set can't silently admit an ambient call. `$ { ... }`
    /// comptime blocks emit no runtime code at all (I3), so they are
    /// excluded: nothing inside one can trip a run-time-voiced E3401.
    RunTime,
    /// jet-comptime's own build-time evaluation check (`check_purity`,
    /// `check_purity_stmts`): checks what runs while the compiler itself
    /// evaluates the expression. `#Impure(...)` bodies are gated by
    /// `--allow-impure`/E3411 at the point they would actually execute
    /// (unchanged prior behavior), so the build-time walk skips them here.
    /// A nested `$ { ... }` block runs for real during build-time
    /// evaluation too, so it stays in the walk.
    BuildTime,
}

/// Threaded through the syntax-only expr/stmt walk beneath the purity
/// checker. Three independent knobs because three different call shapes in
/// this file need three different combinations — see each field's doc.
#[derive(Clone, Copy)]
struct WalkOpts {
    /// `Expr::Lambda` bodies and `#AssumeDet` bodies: off for the purity
    /// walk (a closure's or `#AssumeDet` body isn't checked while walking
    /// its enclosing statement), on for `reachable_owned_funcs`, which wants
    /// every name reachable from anywhere, suppressed body or not.
    include_suppressed: bool,
    /// `#Impure(...)` bodies — see [`PurityStage`].
    descend_impure: bool,
    /// `$ { ... }` comptime blocks — see [`PurityStage`].
    descend_comptime_block: bool,
}

impl WalkOpts {
    /// The shape every non-purity-check caller in this file already used
    /// before the stage split existed: skip suppressed/impure bodies,
    /// always cross into comptime blocks. Also `PurityStage::BuildTime`'s
    /// shape — the split leaves build-time behavior unchanged.
    const PLAIN: WalkOpts = WalkOpts {
        include_suppressed: false,
        descend_impure: false,
        descend_comptime_block: true,
    };

    /// `reachable_owned_funcs` wants every reachable name, full stop.
    const REACHABLE: WalkOpts = WalkOpts {
        include_suppressed: true,
        descend_impure: true,
        descend_comptime_block: true,
    };

    fn for_stage(stage: PurityStage) -> WalkOpts {
        match stage {
            PurityStage::RunTime => WalkOpts {
                include_suppressed: false,
                descend_impure: true,
                descend_comptime_block: false,
            },
            PurityStage::BuildTime => WalkOpts::PLAIN,
        }
    }
}

/// Walk the call graph reachable from `init`; reject the first impure call
/// (IO, FFI) with the path that reached it (E3401). `embed_file`,
/// `embed_bytes`, `find`, `panic`, and `require` are allowed.
pub(super) fn check_purity_stmts(
    stmts: &[Stmt],
    funcs: &HashMap<String, &Func>,
    extern_names: &HashSet<String>,
) -> Result<(), Diagnostic> {
    walk_purity_stmts(
        stmts,
        funcs,
        &|name| impure_builtin(name) || extern_names.contains(name),
        &impurity_diag,
        PurityStage::BuildTime,
    )
}

pub(super) fn check_purity(
    init: &Expr,
    funcs: &HashMap<String, &Func>,
    extern_names: &HashSet<String>,
) -> Result<(), Diagnostic> {
    walk_purity_expr(
        init,
        funcs,
        &|name| impure_builtin(name) || extern_names.contains(name),
        &impurity_diag,
        PurityStage::BuildTime,
    )
}

fn impure_builtin(name: &str) -> bool {
    crate::Syntax::IMPURE_BUILTINS.contains(&name)
}

/// D-META-EFFECT1 c3: the one call-graph purity walk. Walks the call graph
/// reachable from `e`, recursing into `funcs` when a callee is known (with
/// cycle detection via `visited`), and reports the first call
/// `is_leaf_impure` accepts — built into a diagnostic by `diag(name, path,
/// span)` — with the full call-chain path that reached it. Empty `funcs`
/// makes this a direct-body-only check (no transitive recursion), which is
/// what a `=[]=>`-declared function's own body check needs; a populated
/// `funcs` map (comptime's reachable functions, or a whole program's) makes
/// it the transitive `jet eval --pure` / comptime-evaluation check. `stage`
/// selects which statement kinds the walk descends into — see
/// [`PurityStage`].
pub fn walk_purity_expr(
    e: &Expr,
    funcs: &HashMap<String, &Func>,
    is_leaf_impure: &impl Fn(&str) -> bool,
    diag: &impl Fn(&str, &[String], Span) -> Diagnostic,
    stage: PurityStage,
) -> Result<(), Diagnostic> {
    walk_purity_expr_from(
        e,
        funcs,
        is_leaf_impure,
        diag,
        &mut HashSet::new(),
        &mut Vec::new(),
        stage,
    )
}

/// Like [`walk_purity_expr`] but over a statement list, and like
/// [`walk_purity_stmts_from`] but with a fresh `visited`/`path`.
pub fn walk_purity_stmts(
    stmts: &[Stmt],
    funcs: &HashMap<String, &Func>,
    is_leaf_impure: &impl Fn(&str) -> bool,
    diag: &impl Fn(&str, &[String], Span) -> Diagnostic,
    stage: PurityStage,
) -> Result<(), Diagnostic> {
    walk_purity_stmts_from(
        stmts,
        funcs,
        is_leaf_impure,
        diag,
        &mut HashSet::new(),
        &mut Vec::new(),
        stage,
    )
}

/// [`walk_purity_stmts`] with a caller-seeded `visited`/`path` — used by the
/// `jet eval --pure` whole-program root check, which seeds both with the
/// entry function's own name so a direct violation in the root reads
/// "`entry` calls `x`" instead of "`x` is impure, but `entry` declares
/// `=[]=>`".
pub fn walk_purity_stmts_from(
    stmts: &[Stmt],
    funcs: &HashMap<String, &Func>,
    is_leaf_impure: &impl Fn(&str) -> bool,
    diag: &impl Fn(&str, &[String], Span) -> Diagnostic,
    visited: &mut HashSet<String>,
    path: &mut Vec<String>,
    stage: PurityStage,
) -> Result<(), Diagnostic> {
    for stmt in stmts {
        walk_purity_stmt(stmt, funcs, is_leaf_impure, diag, visited, path, stage)?;
    }
    Ok(())
}

fn walk_purity_expr_from(
    e: &Expr,
    funcs: &HashMap<String, &Func>,
    is_leaf_impure: &impl Fn(&str) -> bool,
    diag: &impl Fn(&str, &[String], Span) -> Diagnostic,
    visited: &mut HashSet<String>,
    path: &mut Vec<String>,
    stage: PurityStage,
) -> Result<(), Diagnostic> {
    let mut result = Ok(());
    walk_calls(e, &mut |name, span| {
        if result.is_err() {
            return;
        }
        if is_leaf_impure(name) {
            result = Err(diag(name, path, span));
        } else if let Some(f) = funcs.get(name) {
            if visited.insert(name.to_string()) {
                path.push(name.to_string());
                for stmt in &f.body {
                    if result.is_err() {
                        break;
                    }
                    result =
                        walk_purity_stmt(stmt, funcs, is_leaf_impure, diag, visited, path, stage);
                }
                path.pop();
            }
        }
    });
    result
}

fn walk_purity_stmt(
    s: &Stmt,
    funcs: &HashMap<String, &Func>,
    is_leaf_impure: &impl Fn(&str) -> bool,
    diag: &impl Fn(&str, &[String], Span) -> Diagnostic,
    visited: &mut HashSet<String>,
    path: &mut Vec<String>,
    stage: PurityStage,
) -> Result<(), Diagnostic> {
    let mut result = Ok(());
    walk_stmt_expr_nodes(s, WalkOpts::for_stage(stage), &mut |e| {
        if result.is_ok() {
            result = walk_purity_expr_from(e, funcs, is_leaf_impure, diag, visited, path, stage);
        }
    });
    result
}

/// D-CTIO1: run every build-time IO call in an initializer through the one
/// shared implementation before evaluation, using the call's own span. The
/// evaluator repeats the call for its value, but it carries no spans, so this
/// pre-pass is what makes `embed_file`/`embed_bytes`/`find` diagnostics point
/// at the call. Returns false when it reported at least one problem.
pub fn check_build_time_io(
    e: &Expr,
    base_dir: &std::path::Path,
    diags: &mut Vec<crate::Diagnostics::Diagnostic>,
) -> bool {
    let before = diags.len();
    walk_expr_nodes(e, WalkOpts::PLAIN, &mut |expr| {
        let Expr::Call(call) = expr else { return };
        if !matches!(
            call.name.as_str(),
            crate::Syntax::BUILTIN_EMBED_FILE
                | crate::Syntax::BUILTIN_EMBED_BYTES
                | crate::Syntax::BUILTIN_FIND
        ) {
            return;
        }
        let Some(arg) = call.args.first() else { return };
        // Lock inputs are recorded by the evaluating pass, not here.
        if let Err(diagnostic) = crate::Comptime::eval_build_time_io(
            &call.name,
            base_dir,
            crate::Comptime::Methods::arg_string_literal(arg),
            None,
            call.name_span,
        ) {
            diags.push(diagnostic);
        }
    });
    diags.len() == before
}

/// Visit every direct `Call` name in an expression tree (shallow over
/// nested functions — recursion is driven by the purity walker).
pub fn walk_calls(e: &Expr, f: &mut impl FnMut(&str, Span)) {
    walk_expr_nodes(e, WalkOpts::PLAIN, &mut |expr| {
        if let Expr::Call(call) = expr {
            f(&call.name, call.name_span);
        }
    });
}

/// Visit every identifier read by an expression.
///
/// This is intentionally a read-only syntax walk. Consumers that need
/// semantic dependency information must filter the names against their own
/// declaration table; function names and type names are also represented by
/// `Expr::Ident` while the front end is still doing pure syntax traversal.
pub fn walk_identifiers(e: &Expr, f: &mut impl FnMut(&str, Span)) {
    walk_expr_nodes(e, WalkOpts::PLAIN, &mut |expr| {
        if let Expr::Ident(name, span) = expr {
            f(name, *span);
        }
    });
}

pub(super) fn reachable_owned_funcs(
    init: &Expr,
    funcs: &HashMap<String, Func>,
) -> HashMap<String, Func> {
    fn known_name(name: &str, funcs: &HashMap<String, Func>) -> Option<String> {
        if funcs.contains_key(name) {
            return Some(name.to_string());
        }
        name.split_once('.')
            .map(|(module, symbol)| format!("{module}::{symbol}"))
            .filter(|qualified| funcs.contains_key(qualified))
    }

    let mut roots = BTreeSet::new();
    walk_expr_nodes(init, WalkOpts::REACHABLE, &mut |expr| match expr {
        Expr::Call(call) => {
            if let Some(name) = known_name(&call.name, funcs) {
                roots.insert(name);
            }
        }
        Expr::Ident(name, _) => {
            if let Some(name) = known_name(name, funcs) {
                roots.insert(name);
            }
        }
        _ => {}
    });

    let mut reverse = BTreeMap::<String, BTreeSet<String>>::new();
    for (name, function) in funcs {
        for statement in &function.body {
            walk_stmt_expr_nodes(statement, WalkOpts::REACHABLE, &mut |expr| {
                let dependency = match expr {
                    Expr::Call(call) => known_name(&call.name, funcs),
                    Expr::Ident(name, _) => known_name(name, funcs),
                    _ => None,
                };
                if let Some(dependency) = dependency {
                    reverse.entry(dependency).or_default().insert(name.clone());
                }
            });
        }
    }

    let seeds = roots
        .into_iter()
        .map(|root| (root, BTreeSet::from(["reachable".to_string()])))
        .collect();
    let reachable_names = jet_foundation::Facts::project_reachability(
        &reverse,
        [jet_foundation::Facts::ReachabilityRow::new("reachable", seeds)],
    )
    .nodes_with("reachable", "reachable");
    reachable_names
        .into_iter()
        .filter_map(|name| funcs.get(&name).cloned().map(|function| (name, function)))
        .collect()
}

fn walk_expr_nodes(e: &Expr, opts: WalkOpts, f: &mut impl FnMut(&Expr)) {
    f(e);
    match e {
        Expr::Str(parts, _) => {
            for part in parts {
                if let StrPart::Interp(expr, _) = part {
                    walk_expr_nodes(expr, opts, f);
                }
            }
        }
        Expr::StrMatchLit(_, _)
        | Expr::BinMatchLit(_, _)
        | Expr::Int(_, _, _, _)
        | Expr::Float(_, _, _)
        | Expr::Bool(_, _)
        | Expr::Char(_, _)
        | Expr::Ident(_, _)
        | Expr::UnitLit { .. }
        | Expr::Absent(_)
        | Expr::Todo { .. }
        | Expr::NoElse(_)
        | Expr::ReduceMarker(_, _)
        | Expr::ComptimeName { .. } => {}
        Expr::ListLit(items, _) | Expr::CompareChain { operands: items, .. } => {
            for item in items {
                walk_expr_nodes(item, opts, f);
            }
        }
        Expr::MemberSpread { base, .. } => walk_expr_nodes(base, opts, f),
        Expr::Spread(inner, _)
        | Expr::Unary(_, inner, _)
        | Expr::Deref(inner, _)
        | Expr::RawOf(inner, _)
        | Expr::Copy(inner, _)
        | Expr::Place(inner, _, _)
        | Expr::Field(inner, _, _)
        | Expr::Tainted(inner, _, _)
        | Expr::Present(inner, _)
        | Expr::Ok(inner, _)
        | Expr::Err(inner, _)
        | Expr::Try(inner, _, _)
        | Expr::Paren(inner, _)
        | Expr::IncDec { operand: inner, .. } => {
            walk_expr_nodes(inner, opts, f);
        }
        Expr::OptField { base, .. } => walk_expr_nodes(base, opts, f),
        Expr::Range { start, end, .. } => {
            walk_expr_nodes(start, opts, f);
            walk_expr_nodes(end, opts, f);
        }
        Expr::MapLit(entries, _) => {
            for (key, value) in entries {
                walk_expr_nodes(key, opts, f);
                walk_expr_nodes(value, opts, f);
            }
        }
        Expr::Index { base, index, .. } => {
            walk_expr_nodes(base, opts, f);
            walk_expr_nodes(index, opts, f);
        }
        Expr::Slice {
            base,
            start,
            end,
            range,
            ..
        } => {
            walk_expr_nodes(base, opts, f);
            if let Some(range) = range {
                walk_expr_nodes(range, opts, f);
            } else {
                walk_expr_nodes(start, opts, f);
                walk_expr_nodes(end, opts, f);
            }
        }
        Expr::Call(call) => {
            for arg in &call.args {
                walk_expr_nodes(&arg.expr, opts, f);
            }
        }
        Expr::Binary(_, left, right, _) => {
            walk_expr_nodes(left, opts, f);
            walk_expr_nodes(right, opts, f);
        }
        Expr::MethodCall { receiver, args, .. } => {
            walk_expr_nodes(receiver, opts, f);
            for arg in args {
                walk_expr_nodes(&arg.expr, opts, f);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, _, value) in fields {
                walk_expr_nodes(value, opts, f);
            }
        }
        Expr::TypedLit { body, .. } => {
            body.for_each_expr(|value| walk_expr_nodes(value, opts, f));
        }
        Expr::EnumLit { args, .. } => {
            for arg in args {
                let value = match arg {
                    EnumLitArg::Positional(value)
                    | EnumLitArg::Named { expr: value, .. } => value,
                };
                walk_expr_nodes(value, opts, f);
            }
        }
        Expr::PatternTest {
            subject, pattern, ..
        } => {
            walk_expr_nodes(subject, opts, f);
            walk_pattern_expr_nodes(pattern, opts, f);
        }
        Expr::OrFallback {
            value, fallback, ..
        } => {
            walk_expr_nodes(value, opts, f);
            match fallback {
                OrFallback::Value(value) | OrFallback::Return(Some(value), _) => {
                    walk_expr_nodes(value, opts, f);
                }
                OrFallback::Panic { args, .. } => {
                    for arg in args {
                        walk_expr_nodes(&arg.expr, opts, f);
                    }
                }
                OrFallback::Return(None, _)
                | OrFallback::Break(_)
                | OrFallback::Continue(_)
                | OrFallback::BreakLabel(..)
                | OrFallback::ContinueLabel(..) => {}
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
            walk_expr_nodes(cond, opts, f);
            for stmt in then_body {
                walk_stmt_expr_nodes(stmt, opts, f);
            }
            walk_expr_nodes(then_value, opts, f);
            for stmt in else_body {
                walk_stmt_expr_nodes(stmt, opts, f);
            }
            walk_expr_nodes(else_value, opts, f);
        }
        Expr::TupleLit(fields, _, _) => {
            for (_, value) in fields {
                walk_expr_nodes(value, opts, f);
            }
        }
        Expr::Lambda(lambda) => {
            if opts.include_suppressed {
                match &lambda.body {
                    LambdaBody::Expr(body) => walk_expr_nodes(body, opts, f),
                    LambdaBody::Block(body) => {
                        for stmt in body {
                            walk_stmt_expr_nodes(stmt, opts, f);
                        }
                    }
                }
            }
        }
        Expr::CallValue { callee, args, .. } => {
            walk_expr_nodes(callee, opts, f);
            for arg in args {
                walk_expr_nodes(&arg.expr, opts, f);
            }
        }
        Expr::PtrFromAddr { addr, .. } => walk_expr_nodes(addr, opts, f),
    }
}

/// Visit every expression, including nested suppressed bodies. Comptime
/// access guards use the same complete syntax walk as purity checks.
pub fn walk_expr_nodes_for_validation(e: &Expr, f: &mut impl FnMut(&Expr)) {
    walk_expr_nodes(e, WalkOpts::REACHABLE, f);
}

/// Visit every expression in a statement tree. Comptime access guards need
/// the same complete walk when the TIR bridge evaluates a whole block.
pub fn walk_stmt_expr_nodes_for_validation(stmts: &[Stmt], f: &mut impl FnMut(&Expr)) {
    for stmt in stmts {
        walk_stmt_expr_nodes(stmt, WalkOpts::REACHABLE, f);
    }
}

fn walk_pattern_expr_nodes(pattern: &Pattern, opts: WalkOpts, f: &mut impl FnMut(&Expr)) {
    match pattern {
        Pattern::Or(patterns, _) => {
            for pattern in patterns {
                walk_pattern_expr_nodes(pattern, opts, f);
            }
        }
        Pattern::Struct { fields, .. } => {
            for field in fields {
                if let StructPatField::Value { value, .. } = field {
                    walk_expr_nodes(value, opts, f);
                }
            }
        }
        Pattern::Variant { .. }
        | Pattern::Present { .. }
        | Pattern::Absent(_)
        | Pattern::Ok { .. }
        | Pattern::Err { .. }
        | Pattern::Range { .. }
        | Pattern::StrMatch { .. }
        | Pattern::BinMatch { .. } => {}
    }
}

fn walk_stmt_expr_nodes(s: &Stmt, opts: WalkOpts, f: &mut impl FnMut(&Expr)) {
    macro_rules! walk {
        ($expr:expr) => {
            walk_expr_nodes($expr, opts, f)
        };
    }
    match s {
        Stmt::Expr(expr)
        | Stmt::Val(crate::AST::Binding { init: expr, .. })
        | Stmt::Yield(expr, _) => {
            walk!(expr);
        }
        Stmt::Assign { target, value, .. } => {
            match target {
                LValue::Local { .. } => {}
                LValue::Index { base, index, .. } => {
                    walk!(base);
                    walk!(index);
                }
                LValue::Field { base, .. } => walk!(base),
            }
            walk!(value);
        }
        Stmt::Return(Some(expr), _) | Stmt::BreakValue(expr, _) => walk!(expr),
        Stmt::BreakLabelValue(_, _, expr, _) => walk!(expr),
        Stmt::Return(None, _)
        | Stmt::Break(_)
        | Stmt::Continue(_)
        | Stmt::BreakLabel(..)
        | Stmt::ContinueLabel(..) => {}
        Stmt::While { cond, body, .. } => {
            walk!(cond);
            walk_stmt_body_nodes(body, opts, f);
        }
        Stmt::For { kind, body, .. } => {
            match kind {
                crate::AST::ForKind::Range {
                    start, end, step, ..
                } => {
                    walk!(start);
                    walk!(end);
                    if let Some(step) = step {
                        walk!(step);
                    }
                }
                crate::AST::ForKind::In { collection, step } => {
                    walk!(collection);
                    if let Some(step) = step {
                        walk!(step);
                    }
                }
            }
            walk_stmt_body_nodes(body, opts, f);
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        }
        | Stmt::ComptimeSwitch {
            subject,
            arms,
            else_body,
            ..
        } => {
            walk!(subject);
            for arm in arms {
                walk!(&arm.cond);
                walk_stmt_body_nodes(&arm.body, opts, f);
            }
            if let Some(body) = else_body {
                walk_stmt_body_nodes(body, opts, f);
            }
        }
        Stmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            walk!(&init.init);
            walk!(cond);
            if let Some(step) = step {
                walk_stmt_expr_nodes(step, opts, f);
            }
            walk_stmt_body_nodes(body, opts, f);
        }
        Stmt::Unsafe {
            audit_expr, body, ..
        } => {
            if let Some(audit) = audit_expr {
                walk!(audit);
            }
            walk_stmt_body_nodes(body, opts, f);
        }
        Stmt::Impure {
            reason_expr, body, ..
        } => {
            // See PurityStage: run-time descends (an `=[]=>` fn's declared
            // empty effect set must still reject an ambient call fenced only
            // by `#Impure`), build-time skips (the ambient call is checked
            // by --allow-impure/E3411 at the point it would actually run).
            if opts.descend_impure {
                if let Some(reason) = reason_expr {
                    walk!(reason);
                }
                walk_stmt_body_nodes(body, opts, f);
            }
        }
        Stmt::AssumeDet {
            reason_expr, body, ..
        } => {
            if opts.include_suppressed {
                walk!(reason_expr);
                walk_stmt_body_nodes(body, opts, f);
            }
        }
        Stmt::ComptimeIf {
            cond,
            then_body,
            else_body,
            ..
        } => {
            walk!(cond);
            walk_stmt_body_nodes(then_body, opts, f);
            if let Some(body) = else_body {
                walk_stmt_body_nodes(body, opts, f);
            }
        }
        Stmt::ContextBlock { fields, body, .. } => {
            for (_, value, _) in fields {
                walk!(value);
            }
            walk_stmt_body_nodes(body, opts, f);
        }
        Stmt::ScopeMember { args, body, .. } => {
            for arg in args {
                walk!(arg);
            }
            walk_stmt_body_nodes(body, opts, f);
        }
        Stmt::ComptimeBlock { body, .. } => {
            // See PurityStage: build-time descends (a nested `$ { ... }`
            // block runs for real during build-time evaluation), run-time
            // skips (it emits no runtime code at all — I3 — so nothing
            // inside one can trip a run-time-voiced E3401).
            if opts.descend_comptime_block {
                walk_stmt_body_nodes(body, opts, f);
            }
        }
        Stmt::Loop { body, .. }
        | Stmt::Reactive { body, .. }
        | Stmt::Shield { body, .. }
        | Stmt::Switched { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Policy { body, .. }
        | Stmt::TaskGroup { body, .. }
        | Stmt::Layout { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::Transact { body, .. } => {
            walk_stmt_body_nodes(body, opts, f);
        }
    }
}

fn walk_if_stmt_expr_nodes(if_stmt: &crate::AST::IfStmt, opts: WalkOpts, f: &mut impl FnMut(&Expr)) {
    walk_expr_nodes(&if_stmt.cond, opts, f);
    walk_stmt_body_nodes(&if_stmt.then_body, opts, f);
    match &if_stmt.else_branch {
        Some(crate::AST::ElseBranch::ElseIf(inner)) => {
            walk_if_stmt_expr_nodes(inner, opts, f);
        }
        Some(crate::AST::ElseBranch::Else(body)) => {
            walk_stmt_body_nodes(body, opts, f);
        }
        None => {}
    }
}

fn walk_stmt_body_nodes(body: &[Stmt], opts: WalkOpts, f: &mut impl FnMut(&Expr)) {
    for stmt in body {
        walk_stmt_expr_nodes(stmt, opts, f);
    }
}
