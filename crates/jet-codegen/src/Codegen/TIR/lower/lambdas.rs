use crate::AST::{AccessConvention, Expr, Lambda, LambdaBody, Stmt, Type};
use crate::Codegen::Cx;
use crate::Codegen::mangle;
use crate::Codegen::rust_param_type;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::emit_tir_lambda_block;
use crate::Codegen::TIR::emit_tir_stmts;
use crate::Codegen::TIR::fork_panic;
use crate::Codegen::TIR::JitSpawnCapture;
use crate::Codegen::TIR::lambda_body_ty;
use crate::Codegen::TIR::lambda_body_ty_expecting;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower_owned_expr;
use crate::Codegen::TIR::lower::lambda_block_tail;
use crate::Codegen::TIR::lower_stmts;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TJitSpawnBody;
use crate::Codegen::TIR::TJitSpawnLambda;
use crate::Codegen::TIR::TLambda;
use crate::Codegen::TIR::TLambdaBody;
use crate::Codegen::TIR::TLocal;
use crate::Codegen::TIR::unit_type;
use std::collections::HashSet;

/// c109 Phase 11: lower a lambda/closure literal (`Expr::Lambda`) to a `TLambda`,
/// reproducing `emit_lambda` (Source/Codegen/Expression.rs) byte-for-byte. Every
/// capture/escape/Fn-vs-FnMut decision is the TOTAL `Lambda.meta` fact — no capture
/// analysis here. The body is lowered on a CLONED env extended with: the cloned
/// captures (rebound to `_jet_cap_<n>`, place = that name, type `None` — matching the
/// AST slot) and the params (place = mangled name, type from the annotation). The
/// rendered closure body string is produced now so emit is a pure wrapper.
pub(crate) fn lower_lambda(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> TLambda {
    lower_lambda_expecting(lam, cx, env, None)
}

/// `lower_lambda`, but with the expected parameter types from the fn-typed slot
/// this lambda flows into (a user fn-typed parameter). A bare lambda param
/// (`(x) => …`, no annotation) takes its Rust type from there so codegen emits
/// `move |user_x: i64| …` instead of an un-annotated `move |user_x| …` that
/// rustc can't infer (c142). Builtin closure methods use the host-borrow helper
/// below because their runtime helpers lend callback inputs directly.
pub(crate) fn lower_lambda_expecting(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: Option<&[Type]>,
) -> TLambda {
    lower_lambda_expecting_with_host_borrow(lam, cx, env, expected_params, None, false)
}

/// Lower a callback for a native helper whose Rust contract consumes each
/// payload as `Fn(T)`, rather than applying Jet's ordinary read convention.
pub(crate) fn lower_lambda_expecting_value(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: &[Type],
) -> TLambda {
    lower_lambda_expecting_with_host_borrow(lam, cx, env, Some(expected_params), None, true)
}

/// Runtime helpers such as `Shared.read` and collection adapters already lend
/// their payload to the callback. Render that host borrow exactly once instead
/// of applying function-value Read rules on top of it (`&&T` / `&mut &T`).
pub(crate) fn lower_lambda_expecting_host_borrow(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: &[Type],
    write: bool,
) -> TLambda {
    lower_lambda_expecting_with_host_borrow(
        lam,
        cx,
        env,
        Some(expected_params),
        Some(write),
        false,
    )
}

fn lower_lambda_expecting_with_host_borrow(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: Option<&[Type]>,
    host_borrow: Option<bool>,
    by_value: bool,
) -> TLambda {
    let param_types: Vec<Type> = lam
        .params
        .iter()
        .enumerate()
        .map(|(i, param)| {
            param
                .ty
                .clone()
                .or_else(|| expected_params.and_then(|types| types.get(i)).cloned())
                .unwrap_or(Type::Int)
        })
        .collect();
    let body_ty = lambda_body_ty_expecting(lam, cx, env, expected_params);
    // HTTP handlers cross the server boundary as owned requests. Their public
    // callback type is `Fn(HTTPRequest)`, never Jet's ordinary read-borrowed
    // function convention.
    let http_handler = lam.meta.escapes
        && lam.params.len() == 1
        && matches!(
            lam.params[0]
                .ty
                .as_ref()
                .or_else(|| expected_params.and_then(|params| params.first())),
            Some(Type::Named(name)) if name == "HTTPRequest"
        );
    let by_value = by_value || http_handler;
    // `emit_lambda` clones the env (`lam_env = env.clone()`), so a `??` panic inside the
    // lambda body dumps the lambda's lexical env (outer locals + captures + params) and
    // does not leak its own bindings into the enclosing function.
    let mut lam_env = fork_panic(env);
    // `move ` keyword: the AST emits it UNLESS the lambda is FnMut and does not escape.
    // Computed before the clone-capture prelude so a moving escape can also clone
    // borrowed/Fn captures rustc would otherwise reject (E0521).
    let is_move = !(lam.meta.needs_fn_mut && !lam.meta.escapes);
    // The clone-capture prelude: `let _jet_cap_<n> = (<outer place>).clone();`. The
    // outer place comes from the *outer* env (the capture is an outer local). The cap
    // rebinds the name with place `_jet_cap_<n>`, no deref, type `None` (matching the
    // AST slot `{ rust_name: cap, deref: false, jet_ty: None }`).
    let mut prep = String::new();
    let mut extra_cloned: Vec<String> = Vec::new();
    let mut captures: Vec<(String, String, Type)> = Vec::new();
    // Moving escape into `jet_iter_map` / similar hosts needs owned captures. A
    // borrowed Fn parameter (`&Box<dyn Fn…>`) is not always in `cloned_captures`
    // yet, and a bare `move || f(…)` trips rustc E0521. Clone it into an owned
    // temp so the move closure owns the Box.
    if is_move {
        let param_names: HashSet<&str> = lam.params.iter().map(|p| p.name.as_str()).collect();
        let reads = match &lam.body {
            LambdaBody::Block(stmts) => crate::Sema::block_free_var_reads(stmts),
            LambdaBody::Expr(e) => {
                crate::Sema::block_free_var_reads(&[Stmt::Expr((**e).clone())])
            }
        };
        for name in reads {
            if param_names.contains(name.as_str())
                || lam.meta.cloned_captures.iter().any(|c| c == &name)
                || extra_cloned.iter().any(|c| c == &name)
            {
                continue;
            }
            if !env.locals.contains_key(&name) {
                continue;
            }
            let needs_clone = env.is_borrowed(&name)
                || matches!(env.ty_of(&name), Some(Type::Fn { .. }));
            if needs_clone {
                extra_cloned.push(name);
            }
        }
    }
    for name in lam
        .meta
        .cloned_captures
        .iter()
        .chain(extra_cloned.iter())
    {
        let cap = format!("_jet_cap_{}", mangle(name));
        // Clone temps must be `mut` when the closure body assigns through them
        // (FnMut / captured `:=` locals). Always emit `let mut` for cloned
        // captures — over-mutability is safe; missing mut is rustc E0594 (I2).
        prep.push_str(&format!(
            "let mut {} = ({}).clone();\n    ",
            cap,
            env.place_of(name)
        ));
        let cap_ty = env
            .ty_of(name)
            .unwrap_or_else(|| Type::Named("Unit".to_string()));
        captures.push((name.clone(), cap.clone(), cap_ty.clone()));
        lam_env.bind(name, TLocal::generated(&cap), Some(cap_ty));
    }
    // Taken resources (`owned :: ~next`) are neither cloned nor moved-captured in
    // sema — AOT relies on Rust lexical capture. Cranelift needs an explicit pack.
    {
        let param_names: HashSet<&str> = lam.params.iter().map(|p| p.name.as_str()).collect();
        let reads = match &lam.body {
            LambdaBody::Block(stmts) => crate::Sema::block_free_var_reads(stmts),
            LambdaBody::Expr(e) => {
                crate::Sema::block_free_var_reads(&[Stmt::Expr((**e).clone())])
            }
        };
        for name in reads {
            if param_names.contains(name.as_str()) {
                continue;
            }
            if captures.iter().any(|(n, _, _)| n == &name) {
                continue;
            }
            if !env.locals.contains_key(&name) {
                continue;
            }
            let cap_ty = env
                .ty_of(&name)
                .unwrap_or_else(|| Type::Named("Unit".to_string()));
            // Body still reads the outer place (no `_jet_cap_` rebind).
            captures.push((name.clone(), crate::Codegen::TIR::local_place(&name), cap_ty));
        }
    }
    // Params bind as `mangle(name)` (no deref), typed from the annotation, falling
    // back to `expected_params` at the same position (D-MEM1 S6: this fallback
    // already drove the RENDERED param text below — a bare param's ENV type must
    // match, or a chained field/method read off it resolves against the wrong
    // type, e.g. `Type::Int`'s `struct_field_type` default. `Shared<T>.read(s =>
    // s.field.method())` is the first caller to actually chain a method off a
    // bare closure param's field, which is what surfaced the gap).
    for (i, p) in lam.params.iter().enumerate() {
        let ty =
            p.ty.clone()
                .or_else(|| expected_params.and_then(|ps| ps.get(i)).cloned());
        let place = if host_borrow.is_some()
            || (!by_value && ty.as_ref().is_some_and(|t| !t.is_scalar()))
        {
            TLocal::user(&p.name).through_ref()
        } else {
            TLocal::user(&p.name)
        };
        lam_env.bind(&p.name, place, ty);
    }
    // The rendered param list: `name[: ty]`, exactly as `emit_lambda`. A bare
    // param (no annotation) falls back to the expected fn-type's param at the
    // same position (c142), so a closure passed to a user fn-typed parameter
    // always carries the Rust type rustc needs.
    let params: Vec<String> = lam
        .params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let ty =
                p.ty.clone()
                    .or_else(|| expected_params.and_then(|ps| ps.get(i)).cloned())
                    .map(|t| match (by_value, host_borrow) {
                        (true, _) => format!(": {}", cx.rust_type(&t)),
                        (false, Some(write)) => format!(
                            ": {}{}",
                            if write { "&mut " } else { "&" },
                            cx.rust_type(&t)
                        ),
                        (false, None) => format!(
                            ": {}",
                            rust_param_type(cx, AccessConvention::Read, &t)
                        ),
                    })
                    .unwrap_or_default();
            format!("{}{}", mangle(&p.name), ty)
        })
        .collect();
    // The body: an expression body lowers + emits directly; a block body lowers its
    // statements (on the lambda env) and emits a `{ … }` at indent 1 — byte-for-byte
    // `emit_lambda`'s `emit_stmts(…, 1, false)` then `format!("{{ {} }}", inner)`.
    // D-STM1=A (card #506): a lambda body is a deferred execution context (it may
    // run after the `#Transact` block commits — an `on_commit` hook, a spawned
    // task). So a `Shared.edit` inside a lambda must NOT route to the transaction's
    // deferred `edit_txn` (whose thread-local transaction is gone by then); it stays
    // an immediate edit. This mirrors sema zeroing `txn_depth` for lambda bodies
    // (the same D-TXN2 reason E0746 doesn't fire inside `on_commit`).
    let prev_in_stm = cx.in_stm_transact.replace(false);
    let (body, executable) = match &lam.body {
        LambdaBody::Expr(e) => {
            // An expression-bodied lambda returns an owned value, just like an
            // explicit `return`; clone a borrowed non-scalar parameter here.
            let lowered = lower_owned_expr(e, cx, &mut lam_env);
            (emit_tir_expr(&lowered, cx), TLambdaBody::Expr(Box::new(lowered)))
        }
        LambdaBody::Block(stmts) => {
            let lowered = lower_stmts(stmts, cx, &mut lam_env);
            let mut inner = String::new();
            emit_tir_lambda_block(&lowered, cx, &mut inner, 1);
            (format!("{{ {} }}", inner), TLambdaBody::Block(lowered))
        }
    };
    cx.in_stm_transact.set(prev_in_stm);
    TLambda {
        prep,
        params,
        body,
        executable,
        source_params: lam.params.iter().map(|p| p.name.clone()).collect(),
        jit_name: format!("__jit_lambda_{}_{}", lam.span.start, lam.span.end),
        param_types,
        ret: (!matches!(&body_ty, Type::Named(name) if name == "Unit" || name == "Void"))
            .then_some(body_ty),
        is_move,
        boxed: lam.meta.escapes,
        rc: lam.meta.escapes && !lam.meta.needs_fn_mut && !http_handler,
        arc: http_handler,
        captures,
    }
}

/// c139 M4: lower a spawn lambda to compilable TIR for the Cranelift JIT.
pub(crate) fn lower_spawn_lambda_for_jit(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> TJitSpawnLambda {
    lower_spawn_lambda_for_jit_expecting(lam, cx, env, &[])
}

/// Like [`lower_spawn_lambda_for_jit`], but bare params take types from
/// `expected_params` (watch/event callbacks pass `WatchEvent`, etc.).
pub(crate) fn lower_spawn_lambda_for_jit_expecting(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: &[Type],
) -> TJitSpawnLambda {
    let param_names: HashSet<&str> = lam.params.iter().map(|p| p.name.as_str()).collect();
    let cloned: HashSet<&str> = lam
        .meta
        .cloned_captures
        .iter()
        .map(|s| s.as_str())
        .collect();
    let reads = match &lam.body {
        LambdaBody::Block(stmts) => crate::Sema::block_free_var_reads(stmts),
        LambdaBody::Expr(e) => crate::Sema::block_free_var_reads(&[Stmt::Expr((**e).clone())]),
    };
    let mut captures: Vec<JitSpawnCapture> = reads
        .into_iter()
        .filter(|n| !param_names.contains(n.as_str()))
        .filter(|n| env.locals.contains_key(n))
        .map(|name| JitSpawnCapture {
            clone_at_spawn: cloned.contains(name.as_str()),
            // D-TASKBORROW1=A: a borrowed split-view crosses as its window
            // handle, not as the element type its Jet binding shows.
            ty: env
                .split_view_handle(&name)
                .or_else(|| env.ty_of(&name))
                .unwrap_or_else(|| Type::Named("Unit".to_string())),
            name,
        })
        .collect();
    captures.sort_by(|a, b| a.name.cmp(&b.name));

    let mut lam_env = fork_panic(env);
    for cap in &captures {
        lam_env.bind(&cap.name, TLocal::user(&cap.name), Some(cap.ty.clone()));
    }
    for (i, p) in lam.params.iter().enumerate() {
        let ty = p
            .ty
            .clone()
            .or_else(|| expected_params.get(i).cloned())
            .or_else(|| Some(Type::Int));
        lam_env.bind(&p.name, TLocal::user(&p.name), ty);
    }

    let ret = lambda_body_ty(lam, cx, env);
    let body = match &lam.body {
        LambdaBody::Expr(e) => TJitSpawnBody::Expr(Box::new(lower_expr(e, cx, &mut lam_env))),
        LambdaBody::Block(stmts) => {
            if let Some((prefix, tail)) = lambda_block_tail(stmts) {
                let prefix_lowered = lower_stmts(prefix, cx, &mut lam_env);
                let tail_lowered = Some(Box::new(match tail {
                    Stmt::Return(Some(e), _) => lower_expr(e, cx, &mut lam_env),
                    Stmt::Expr(e) => lower_expr(e, cx, &mut lam_env),
                    _ => TExpr {
                        ty: unit_type(),
                        kind: TExprKind::IntLit(0, None),
                    },
                }));
                TJitSpawnBody::Block {
                    prefix: prefix_lowered,
                    tail: tail_lowered,
                }
            } else {
                TJitSpawnBody::Block {
                    prefix: lower_stmts(stmts, cx, &mut lam_env),
                    tail: None,
                }
            }
        }
    };

    TJitSpawnLambda {
        params: lam
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                (
                    p.name.clone(),
                    p.ty
                        .clone()
                        .or_else(|| expected_params.get(i).cloned())
                        .unwrap_or_else(|| Type::Int),
                )
            })
            .collect(),
        captures,
        body,
        ret,
    }
}

/// c109 Phase 13: render a `tasks.spawn` lambda, reproducing `emit_spawn_lambda`
/// (Source/Codegen/Expression.rs) byte-for-byte. It is `emit_lambda` minus the
/// Fn-vs-FnMut and escape logic: ALWAYS `move`, NEVER `Box::new`. The clone-capture
/// prelude is identical. Returns the full rendered closure string (wrapped in
/// `{ <prep> <closure> }` when there are cloned captures).
pub(crate) fn render_spawn_lambda(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> String {
    let mut lam_env = fork_panic(env);
    let mut prep = String::new();
    for name in &lam.meta.cloned_captures {
        let cap = format!("_jet_cap_{}", mangle(name));
        prep.push_str(&format!(
            "let {} = ({}).clone();\n    ",
            cap,
            env.place_of(name)
        ));
        lam_env.bind(name, TLocal::generated(&cap), None);
    }
    for p in &lam.params {
        lam_env.bind(&p.name, TLocal::user(&p.name), p.ty.clone());
    }
    let params: Vec<String> = lam
        .params
        .iter()
        .map(|p| {
            let ty =
                p.ty.as_ref()
                    .map(|t| format!(": {}", cx.rust_type(t)))
                    .unwrap_or_default();
            format!("{}{}", mangle(&p.name), ty)
        })
        .collect();
    let body = match &lam.body {
        LambdaBody::Expr(e) => emit_tir_expr(&lower_expr(e, cx, &mut lam_env), cx),
        LambdaBody::Block(stmts) => render_spawn_block_body(stmts, cx, &mut lam_env),
    };
    let closure = format!("move |{}| {}", params.join(", "), body);
    if prep.is_empty() {
        closure
    } else {
        format!("{{ {} {} }}", prep, closure)
    }
}

/// Render a `#Reactive { … }` block as a `move || { … }` closure for
/// `jet_reactive_effect`. Outer locals read inside the block are cloned into
/// `_jet_cap_*` bindings (byte-for-byte the stored-lambda capture prelude).
pub(super) fn render_reactive_block_closure(stmts: &[Stmt], cx: &Cx, outer_env: &LowerEnv) -> String {
    let reads = crate::Sema::block_free_var_reads(stmts);
    let mut caps: Vec<String> = reads
        .into_iter()
        .filter(|n| outer_env.locals.contains_key(n))
        .collect();
    caps.sort();
    let mut lam_env = fork_panic(outer_env);
    let mut prep = String::new();
    for name in &caps {
        let cap = format!("_jet_cap_{}", mangle(name));
        // Reactive bodies may update their private clone on every rerun. The
        // runtime serializes the resulting FnMut closure behind a Mutex.
        prep.push_str(&format!(
            "let mut {} = ({}).clone();\n    ",
            cap,
            outer_env.place_of(name)
        ));
        lam_env.bind(name, TLocal::generated(&cap), outer_env.ty_of(name));
    }
    let mut inner = String::new();
    let lowered = lower_stmts(stmts, cx, &mut lam_env);
    emit_tir_stmts(&lowered, cx, &mut inner, 1);
    let closure = format!("move || {{ {} }}", inner);
    if prep.is_empty() {
        closure
    } else {
        format!("{{ {} {} }}", prep, closure)
    }
}

/// Render a spawn-lambda block body: prefix statements keep `;`, the tail
/// expression is the closure's return value (no trailing `;`).
fn render_spawn_block_body(stmts: &[Stmt], cx: &Cx, lam_env: &mut LowerEnv) -> String {
    let Some((prefix, tail)) = lambda_block_tail(stmts) else {
        let mut inner = String::new();
        let lowered = lower_stmts(stmts, cx, lam_env);
        emit_tir_stmts(&lowered, cx, &mut inner, 1);
        return format!("{{ {} }}", inner);
    };
    let mut inner = String::new();
    if !prefix.is_empty() {
        let lowered = lower_stmts(prefix, cx, lam_env);
        emit_tir_stmts(&lowered, cx, &mut inner, 1);
    }
    let pad = "    ";
    match tail {
        Stmt::Return(Some(e), _) => {
            inner.push_str(&format!(
                "{}return {};",
                pad,
                emit_tir_expr(&lower_expr(e, cx, lam_env), cx)
            ));
        }
        Stmt::Expr(e) => {
            inner.push_str(&format!(
                "{}{}",
                pad,
                emit_tir_expr(&lower_expr(e, cx, lam_env), cx)
            ));
        }
        _ => {}
    }
    format!("{{ {} }}", inner)
}

fn wrap_lowered_lambda(tl: &TLambda) -> String {
    let move_kw = if tl.is_move { "move " } else { "" };
    let closure = format!("{}|{}| {}", move_kw, tl.params.join(", "), tl.body);
    let wrapped = if tl.arc {
        format!("std::sync::Arc::new({closure})")
    } else if tl.rc {
        format!("std::rc::Rc::new({closure})")
    } else if tl.boxed {
        format!("Box::new({closure})")
    } else {
        closure
    };
    if tl.prep.is_empty() {
        wrapped
    } else {
        format!("{{ {} {} }}", tl.prep, wrapped)
    }
}

/// c109 Phase 13: render a lambda via the plain `emit_lambda` form (used by
/// `http.serve`'s lambda handler and `scope.guard`). Returns the full closure string.
pub(crate) fn render_lambda_str(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> String {
    render_lambda_str_expecting(lam, cx, env, None)
}

/// Like [`render_lambda_str`], but force `Arc` wrapping for hosts that require
/// `Send + Sync` (UI `reactive_render` / portable `button` `on_click`). Escaping
/// Fn values otherwise prefer `Rc`, which is not Sync and fails rustc for those
/// prelude signatures.
pub(crate) fn render_lambda_str_sync(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> String {
    let mut tl = lower_lambda(lam, cx, env);
    tl.arc = true;
    tl.rc = false;
    wrap_lowered_lambda(&tl)
}

/// D-MEM1 S6: `render_lambda_str`, but seeding the closure param(s) with an
/// expected type when the source has no annotation (`Shared<T>.read(s => …)`'s
/// bare `s`) — needed so a chained field/method read off the param resolves
/// against the right type inside the lambda body (see `lower_lambda_expecting`).
pub(crate) fn render_lambda_str_expecting(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: Option<&[Type]>,
) -> String {
    wrap_lowered_lambda(&lower_lambda_expecting(lam, cx, env, expected_params))
}

pub(crate) fn render_lambda_str_expecting_value(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: &[Type],
) -> String {
    wrap_lowered_lambda(&lower_lambda_expecting_value(lam, cx, env, expected_params))
}

// ---------------------------------------------------------------------------
// Emission: TIR -> Rust. PURE formatting. No type inference, no decisions.
// ---------------------------------------------------------------------------

/// c109 Phase 25: render the router handler (arg 1) exactly as `emit_router_handler`
/// (Source/Codegen/Expression.rs) does, at lowering. A bare top-level fn name (not a
/// local) becomes the canonical shared `JetHTTPHandler`; a lambda does the same.
pub(crate) fn render_router_handler(
    args: &[crate::AST::CallArg],
    cx: &Cx,
    env: &LowerEnv,
) -> String {
    let root = &cx.root_prefix;
    let handler_dyn = format!(
        "as std::sync::Arc<dyn Fn({0}JetHTTPRequest) -> Result<{0}JetHTTPResponse, {0}JetHTTPError> + Send + Sync>",
        root
    );
    match &args[1].expr {
        Expr::Ident(name, _) if !env.locals.contains_key(name) => {
            let rust_name = mangle(name);
            format!(
                "std::sync::Arc::new(move |__req: {}JetHTTPRequest| {}(&__req)) {}",
                root, rust_name, handler_dyn
            )
        }
        Expr::Lambda(lam) => {
            format!(
                "std::sync::Arc::new({}) {}",
                render_lambda_str(lam, cx, env),
                handler_dyn
            )
        }
        // The gate (`router_register_in_subset`) proved arg 1 is one of the two above.
        _ => unreachable!("router handler gate proved a named-fn or lambda handler"),
    }
}
