//! D-CAP8 (= C): resolve unmarked (`Infer`) parameter capabilities from body
//! usage, deterministically, before checking and codegen run. An unmarked param
//! starts as `Infer` and elevates by what the body does to it:
//!
//! - a field/index assignment rooted at the param (`p.hp = …`)      → `Write`
//! - the param passed as a `~`/`^`/`&` argument (`os.close(^file)`)  → that capability
//! - calling a `~self`/`^self`/`&self` method on a param             → that capability
//! - otherwise                                                       → `Read`
//!
//! Elevation takes the strongest signal by the lattice `Read < Share < Write <
//! Move` (a move consumes, so it dominates). `Raw` is never inferred — it requires
//! the explicit `*` sigil (D-CAP9). The pass mutates the AST param conventions in
//! place so the existing convention-driven checks (E0202/E0205/…) and codegen see
//! the resolved capability, never `Infer`. Determinism: params and statements are
//! visited in source order; no hashed iteration feeds the decision.

use crate::Syntax;
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

/// Build a map from `(type_name, method_name)` to the receiver's `AccessConvention`
/// for all impl methods whose first param is an explicit non-Read/non-Infer `self`.
/// This is used to infer caller capabilities when a method with a mutating receiver
/// is called on an inferred param.
fn build_method_map(bundle: &ProgramBundle) -> HashMap<(String, String), AccessConvention> {
    let mut map = HashMap::new();
    for module in &bundle.modules {
        for item in &module.items {
            if let Item::Impl(im) = item {
                for method in &im.methods {
                    if let Some(recv) = method.params.first() {
                        if recv.name == Syntax::KW_SELF {
                            match recv.convention {
                                AccessConvention::Write
                                | AccessConvention::Move
                                | AccessConvention::Share => {
                                    map.insert(
                                        (im.type_name.clone(), method.name.clone()),
                                        recv.convention,
                                    );
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
    map
}

/// Resolve every function/method's `Infer` params across the whole bundle.
pub fn resolve_capabilities(bundle: &mut ProgramBundle) {
    // Build the method-convention map once from all impl blocks before mutating.
    let method_map = build_method_map(bundle);
    for module in bundle.modules.iter_mut() {
        for item in module.items.iter_mut() {
            resolve_item(item, &method_map);
        }
    }
}

fn resolve_item(item: &mut Item, method_map: &HashMap<(String, String), AccessConvention>) {
    match item {
        Item::Func(f) => resolve_fn(f, method_map),
        Item::Impl(im) => {
            for m in im.methods.iter_mut() {
                resolve_fn(m, method_map);
            }
        }
        _ => {}
    }
}

fn resolve_fn(f: &mut Func, method_map: &HashMap<(String, String), AccessConvention>) {
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
    // Build param_name -> type_name for inferred params (only Named/Apply types
    // that can be impl targets; primitives return None from base_name and are skipped).
    let param_types: HashMap<String, String> = f
        .params
        .iter()
        .filter(|p| p.convention == AccessConvention::Infer)
        .filter_map(|p| p.ty.base_name().map(|n| (p.name.clone(), n.to_string())))
        .collect();
    // Floor every inferred param at Read, then elevate from the body.
    let mut caps: HashMap<String, AccessConvention> = infer
        .iter()
        .map(|n| (n.clone(), AccessConvention::Read))
        .collect();
    scan_stmts(&f.body, &mut caps, method_map, &param_types);
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

fn scan_stmts(
    stmts: &[Stmt],
    caps: &mut HashMap<String, AccessConvention>,
    method_map: &HashMap<(String, String), AccessConvention>,
    param_types: &HashMap<String, String>,
) {
    for stmt in stmts {
        scan_stmt(stmt, caps, method_map, param_types);
    }
}

fn scan_stmt(
    stmt: &Stmt,
    caps: &mut HashMap<String, AccessConvention>,
    method_map: &HashMap<(String, String), AccessConvention>,
    param_types: &HashMap<String, String>,
) {
    match stmt {
        Stmt::Expr(e) => scan_expr(e, caps, method_map, param_types),
        Stmt::Val(b) => scan_expr(&b.init, caps, method_map, param_types),
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
                LValue::Field { base, .. } => scan_expr(base, caps, method_map, param_types),
                LValue::Index { base, index, .. } => {
                    scan_expr(base, caps, method_map, param_types);
                    scan_expr(index, caps, method_map, param_types);
                }
                LValue::Local { .. } => {}
            }
            scan_expr(value, caps, method_map, param_types);
        }
        Stmt::Return(Some(e), _) => scan_expr(e, caps, method_map, param_types),
        Stmt::Return(None, _) => {}
        Stmt::If(ifs) => scan_if(ifs, caps, method_map, param_types),
        Stmt::While { cond, body, .. } => {
            scan_expr(cond, caps, method_map, param_types);
            scan_stmts(body, caps, method_map, param_types);
        }
        Stmt::For { kind, body, .. } => {
            match kind {
                ForKind::Range { start, end, step } => {
                    scan_expr(start, caps, method_map, param_types);
                    scan_expr(end, caps, method_map, param_types);
                    if let Some(step) = step {
                        scan_expr(step, caps, method_map, param_types);
                    }
                }
                ForKind::In { collection } => scan_expr(collection, caps, method_map, param_types),
            }
            scan_stmts(body, caps, method_map, param_types);
        }
        Stmt::Switch {
            subject,
            arms,
            else_body,
            ..
        } => {
            scan_expr(subject, caps, method_map, param_types);
            for a in arms {
                scan_expr(&a.cond, caps, method_map, param_types);
                scan_stmts(&a.body, caps, method_map, param_types);
            }
            if let Some(eb) = else_body {
                scan_stmts(eb, caps, method_map, param_types);
            }
        }
        Stmt::Loop { body, .. }
        | Stmt::Unsafe { body, .. }
        | Stmt::Impure { body, .. }
        | Stmt::Region { body, .. }
        | Stmt::Caps { body, .. }
        | Stmt::Grant { body, .. }
        | Stmt::Live { body, .. }
        | Stmt::AssumeDet { body, .. }
        | Stmt::Transact { body, .. } => scan_stmts(body, caps, method_map, param_types),
        // D-CTMARKER1: comptime block erases; walk body conservatively for caps scan.
        Stmt::ComptimeBlock { body, .. } => scan_stmts(body, caps, method_map, param_types),
        Stmt::ComptimeIf {
            cond,
            then_body,
            else_body,
            ..
        } => {
            scan_expr(cond, caps, method_map, param_types);
            scan_stmts(then_body, caps, method_map, param_types);
            if let Some(eb) = else_body {
                scan_stmts(eb, caps, method_map, param_types);
            }
        }
        Stmt::ContextBlock { fields, body, .. } => {
            for (_, e, _) in fields {
                scan_expr(e, caps, method_map, param_types);
            }
            scan_stmts(body, caps, method_map, param_types);
        }
        Stmt::Break(_) | Stmt::Continue(_) | Stmt::BreakLabel(..) | Stmt::ContinueLabel(..) => {}
    }
}

fn scan_if(
    ifs: &IfStmt,
    caps: &mut HashMap<String, AccessConvention>,
    method_map: &HashMap<(String, String), AccessConvention>,
    param_types: &HashMap<String, String>,
) {
    scan_expr(&ifs.cond, caps, method_map, param_types);
    scan_stmts(&ifs.then_body, caps, method_map, param_types);
    match &ifs.else_branch {
        Some(ElseBranch::Else(b)) => scan_stmts(b, caps, method_map, param_types),
        Some(ElseBranch::ElseIf(next)) => scan_if(next, caps, method_map, param_types),
        None => {}
    }
}

/// Walk an expression, elevating any param passed as a `~`/`^`/`&` argument and
/// recursing into nested sub-expressions. Also elevates params whose type has a
/// mutating-receiver method called on them (D-CAP8 receiver-method signal).
/// Leaf/uninteresting forms are skipped (the `_` arm).
fn scan_expr(
    e: &Expr,
    caps: &mut HashMap<String, AccessConvention>,
    method_map: &HashMap<(String, String), AccessConvention>,
    param_types: &HashMap<String, String>,
) {
    match e {
        Expr::Call(c) => {
            for a in &c.args {
                scan_arg(a, caps, method_map, param_types);
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
            ..
        } => {
            // Receiver-method mutation signal: if the receiver is rooted at an
            // inferred param whose type `T` is known, and `(T, method)` has an
            // explicit non-Read receiver convention, elevate the param.
            if let Some(root) = expr_root(receiver) {
                if caps.contains_key(root) {
                    if let Some(ty) = param_types.get(root) {
                        if let Some(&conv) = method_map.get(&(ty.clone(), method.clone())) {
                            elevate(caps, root, conv);
                        }
                    }
                }
            }
            scan_expr(receiver, caps, method_map, param_types);
            for a in args {
                scan_arg(a, caps, method_map, param_types);
            }
        }
        Expr::CallValue { callee, args, .. } => {
            scan_expr(callee, caps, method_map, param_types);
            for a in args {
                scan_arg(a, caps, method_map, param_types);
            }
        }
        Expr::Field(base, _, _) | Expr::OptField { base, .. } => {
            scan_expr(base, caps, method_map, param_types)
        }
        Expr::Index { base, index, .. } => {
            scan_expr(base, caps, method_map, param_types);
            scan_expr(index, caps, method_map, param_types);
        }
        Expr::Binary(_, lhs, rhs, _) => {
            scan_expr(lhs, caps, method_map, param_types);
            scan_expr(rhs, caps, method_map, param_types);
        }
        Expr::Unary(_, inner, _) => scan_expr(inner, caps, method_map, param_types),
        Expr::Deref(inner, _) | Expr::RawOf(inner, _) => {
            scan_expr(inner, caps, method_map, param_types)
        }
        Expr::Str(parts, _) => {
            for p in parts {
                if let crate::AST::StrPart::Interp(pe) = p {
                    scan_expr(pe, caps, method_map, param_types);
                }
            }
        }
        _ => {}
    }
}

/// A call argument carrying an explicit `~`/`^`/`&` capability elevates the param
/// it is rooted at; then recurse into the argument expression.
fn scan_arg(
    a: &crate::AST::CallArg,
    caps: &mut HashMap<String, AccessConvention>,
    method_map: &HashMap<(String, String), AccessConvention>,
    param_types: &HashMap<String, String>,
) {
    if a.convention != AccessConvention::Read {
        if let Some(root) = expr_root(&a.expr) {
            elevate(caps, root, a.convention);
        }
    }
    scan_expr(&a.expr, caps, method_map, param_types);
}
