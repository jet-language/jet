//! D-CAP8 (= C): resolve unmarked (`Infer`) parameter capabilities from body
//! usage, deterministically, before checking and codegen run. An unmarked param
//! starts as `Infer` and elevates by what the body does to it:
//!
//! - a field/index assignment rooted at the param (`p.hp = …`)      → `Write`
//! - the param passed as a `~`/`^`/`&` argument (`os.close(^file)`)  → that capability
//! - otherwise                                                       → `Read`
//!
//! Elevation takes the strongest signal by the lattice `Read < Share < Write <
//! Move` (a move consumes, so it dominates). `Raw` is never inferred — it requires
//! the explicit `*` sigil (D-CAP9). The pass mutates the AST param conventions in
//! place so the existing convention-driven checks (E0202/E0205/…) and codegen see
//! the resolved capability, never `Infer`. Determinism: params and statements are
//! visited in source order; no hashed iteration feeds the decision.

use crate::AST::{
    AccessConvention, ElseBranch, Expr, ForKind, Func, IfStmt, Item, LValue, ProgramBundle, Stmt,
};
use std::collections::HashMap;

/// Lattice rank for elevation. `Read`/`Infer` floor at 0; `Move` dominates.
fn rank(c: AccessConvention) -> u8 {
    match c {
        AccessConvention::Infer | AccessConvention::Read => 0,
        AccessConvention::Share => 1,
        AccessConvention::Write => 2,
        AccessConvention::Move => 3,
        AccessConvention::Raw => 4, // never inferred; defensive ceiling
    }
}

/// Resolve every function/method's `Infer` params across the whole bundle.
pub(crate) fn resolve_capabilities(bundle: &mut ProgramBundle) {
    for module in bundle.modules.iter_mut() {
        for item in module.items.iter_mut() {
            resolve_item(item);
        }
    }
}

fn resolve_item(item: &mut Item) {
    match item {
        Item::Func(f) => resolve_fn(f),
        Item::Impl(im) => {
            for m in im.methods.iter_mut() {
                resolve_fn(m);
            }
        }
        _ => {}
    }
}

fn resolve_fn(f: &mut Func) {
    // Source-ordered list of the param names still to infer.
    let infer: Vec<String> = f
        .params
        .iter()
        .filter(|p| p.convention == AccessConvention::Infer)
        .map(|p| p.name.clone())
        .collect();
    if infer.is_empty() {
        return;
    }
    // Floor every inferred param at Read, then elevate from the body.
    let mut caps: HashMap<String, AccessConvention> =
        infer.iter().map(|n| (n.clone(), AccessConvention::Read)).collect();
    scan_stmts(&f.body, &mut caps);
    for p in f.params.iter_mut() {
        if p.convention == AccessConvention::Infer {
            if let Some(c) = caps.get(&p.name) {
                p.convention = *c;
            }
        }
    }
}

/// Raise `name`'s resolved capability to at least `cap` (lattice max).
fn elevate(caps: &mut HashMap<String, AccessConvention>, name: &str, cap: AccessConvention) {
    if let Some(cur) = caps.get_mut(name) {
        if rank(cap) > rank(*cur) {
            *cur = cap;
        }
    }
}

/// The base identifier an lvalue/expr is ultimately rooted at, following
/// field/index/opt-field chains down to the leaf `Ident`. `p.a.b[i]` → `p`.
fn expr_root(e: &Expr) -> Option<&str> {
    match e {
        Expr::Ident(n, _) => Some(n),
        Expr::Field(base, _, _) => expr_root(base),
        Expr::OptField { base, .. } => expr_root(base),
        Expr::Index { base, .. } => expr_root(base),
        _ => None,
    }
}

fn lvalue_root(lv: &LValue) -> Option<&str> {
    match lv {
        LValue::Local { name, .. } => Some(name),
        LValue::Field { base, .. } => expr_root(base),
        LValue::Index { base, .. } => expr_root(base),
    }
}

fn scan_stmts(stmts: &[Stmt], caps: &mut HashMap<String, AccessConvention>) {
    for stmt in stmts {
        scan_stmt(stmt, caps);
    }
}

fn scan_stmt(stmt: &Stmt, caps: &mut HashMap<String, AccessConvention>) {
    match stmt {
        Stmt::Expr(e) => scan_expr(e, caps),
        Stmt::Val(b) => scan_expr(&b.init, caps),
        Stmt::Assign { target, value, .. } => {
            // A through-reference mutation of a param requires Write. A bare
            // `LValue::Local` reassigns the local binding (not the caller's value),
            // so only field/index targets count as mutating the parameter.
            if matches!(target, LValue::Field { .. } | LValue::Index { .. }) {
                if let Some(root) = lvalue_root(target) {
                    elevate(caps, root, AccessConvention::Write);
                }
            }
            // The lvalue base and index can themselves contain calls.
            match target {
                LValue::Field { base, .. } => scan_expr(base, caps),
                LValue::Index { base, index, .. } => {
                    scan_expr(base, caps);
                    scan_expr(index, caps);
                }
                LValue::Local { .. } => {}
            }
            scan_expr(value, caps);
        }
        Stmt::Return(Some(e), _) => scan_expr(e, caps),
        Stmt::Return(None, _) => {}
        Stmt::If(ifs) => scan_if(ifs, caps),
        Stmt::While { cond, body, .. } => {
            scan_expr(cond, caps);
            scan_stmts(body, caps);
        }
        Stmt::For { kind, body, .. } => {
            match kind {
                ForKind::Range { start, end, step } => {
                    scan_expr(start, caps);
                    scan_expr(end, caps);
                    if let Some(step) = step {
                        scan_expr(step, caps);
                    }
                }
                ForKind::In { collection } => scan_expr(collection, caps),
            }
            scan_stmts(body, caps);
        }
        Stmt::Switch { subject, arms, else_body, .. } => {
            scan_expr(subject, caps);
            for a in arms {
                scan_expr(&a.cond, caps);
                scan_stmts(&a.body, caps);
            }
            if let Some(eb) = else_body {
                scan_stmts(eb, caps);
            }
        }
        Stmt::Loop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Live { body, .. } => scan_stmts(body, caps),
        Stmt::ComptimeIf { cond, then_body, else_body, .. } => {
            scan_expr(cond, caps);
            scan_stmts(then_body, caps);
            if let Some(eb) = else_body {
                scan_stmts(eb, caps);
            }
        }
        Stmt::ContextBlock { fields, body, .. } => {
            for (_, e, _) in fields {
                scan_expr(e, caps);
            }
            scan_stmts(body, caps);
        }
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => {}
    }
}

fn scan_if(ifs: &IfStmt, caps: &mut HashMap<String, AccessConvention>) {
    scan_expr(&ifs.cond, caps);
    scan_stmts(&ifs.then_body, caps);
    match &ifs.else_branch {
        Some(ElseBranch::Else(b)) => scan_stmts(b, caps),
        Some(ElseBranch::ElseIf(next)) => scan_if(next, caps),
        None => {}
    }
}

/// Walk an expression, elevating any param passed as a `~`/`^`/`&` argument and
/// recursing into nested sub-expressions. Leaf/uninteresting forms are skipped
/// (the `_` arm); this covers the call-bearing forms that carry conventions.
fn scan_expr(e: &Expr, caps: &mut HashMap<String, AccessConvention>) {
    match e {
        Expr::Call(c) => {
            for a in &c.args {
                scan_arg(a, caps);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            scan_expr(receiver, caps);
            for a in args {
                scan_arg(a, caps);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            scan_expr(callee, caps);
            for a in args {
                scan_arg(a, caps);
            }
        }
        Expr::Field(base, _, _) | Expr::OptField { base, .. } => scan_expr(base, caps),
        Expr::Index { base, index, .. } => {
            scan_expr(base, caps);
            scan_expr(index, caps);
        }
        Expr::Binary(_, lhs, rhs, _) => {
            scan_expr(lhs, caps);
            scan_expr(rhs, caps);
        }
        Expr::Unary(_, inner, _) => scan_expr(inner, caps),
        Expr::Deref(inner, _) => scan_expr(inner, caps),
        Expr::Str(parts, _) => {
            for p in parts {
                if let crate::AST::StrPart::Interp(pe) = p {
                    scan_expr(pe, caps);
                }
            }
        }
        _ => {}
    }
}

/// A call argument carrying an explicit `~`/`^`/`&` capability elevates the param
/// it is rooted at; then recurse into the argument expression.
fn scan_arg(a: &crate::AST::CallArg, caps: &mut HashMap<String, AccessConvention>) {
    if a.convention != AccessConvention::Read {
        if let Some(root) = expr_root(&a.expr) {
            elevate(caps, root, a.convention);
        }
    }
    scan_expr(&a.expr, caps);
}
