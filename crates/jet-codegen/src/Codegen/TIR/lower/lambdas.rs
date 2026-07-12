use crate::AST::{Expr, Lambda, LambdaBody, Stmt, Type};
use crate::Codegen::Cx;
use crate::Codegen::mangle;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::emit_tir_lambda_block;
use crate::Codegen::TIR::emit_tir_stmts;
use crate::Codegen::TIR::fork_panic;
use crate::Codegen::TIR::JitSpawnCapture;
use crate::Codegen::TIR::lambda_body_ty;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower::lambda_block_tail;
use crate::Codegen::TIR::lower_stmts;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TJitSpawnBody;
use crate::Codegen::TIR::TJitSpawnLambda;
use crate::Codegen::TIR::TLambda;
use crate::Codegen::TIR::TLambdaBody;
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
/// rustc can't infer (c142). Builtin closure methods (`.each`/`.map`/…) keep
/// passing `None`: their helper signatures drive the closure-param type (often
/// by-ref), so annotating it would mismatch.
pub(crate) fn lower_lambda_expecting(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: Option<&[Type]>,
) -> TLambda {
    // `emit_lambda` clones the env (`lam_env = env.clone()`), so a `??` panic inside the
    // lambda body dumps the lambda's lexical env (outer locals + captures + params) and
    // does not leak its own bindings into the enclosing function.
    let mut lam_env = fork_panic(env);
    // The clone-capture prelude: `let _jet_cap_<n> = (<outer place>).clone();`. The
    // outer place comes from the *outer* env (the capture is an outer local). The cap
    // rebinds the name with place `_jet_cap_<n>`, no deref, type `None` (matching the
    // AST slot `{ rust_name: cap, deref: false, jet_ty: None }`).
    let mut prep = String::new();
    for name in &lam.meta.cloned_captures {
        let cap = format!("_jet_cap_{}", mangle(name));
        prep.push_str(&format!(
            "let {} = ({}).clone();\n    ",
            cap,
            env.place_of(name)
        ));
        lam_env.bind(name, cap, None);
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
        lam_env.bind(&p.name, mangle(&p.name), ty);
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
                    .map(|t| format!(": {}", cx.rust_type(&t)))
                    .unwrap_or_default();
            format!("{}{}", mangle(&p.name), ty)
        })
        .collect();
    // The body: an expression body lowers + emits directly; a block body lowers its
    // statements (on the lambda env) and emits a `{ … }` at indent 1 — byte-for-byte
    // `emit_lambda`'s `emit_stmts(…, 1, false)` then `format!("{{ {} }}", inner)`.
    let (body, executable) = match &lam.body {
        LambdaBody::Expr(e) => {
            let lowered = lower_expr(e, cx, &mut lam_env);
            (emit_tir_expr(&lowered, cx), TLambdaBody::Expr(Box::new(lowered)))
        }
        LambdaBody::Block(stmts) => {
            let lowered = lower_stmts(stmts, cx, &mut lam_env);
            let mut inner = String::new();
            emit_tir_lambda_block(&lowered, cx, &mut inner, 1);
            (format!("{{ {} }}", inner), TLambdaBody::Block(lowered))
        }
    };
    // `move ` keyword: the AST emits it UNLESS the lambda is FnMut and does not escape.
    let is_move = !(lam.meta.needs_fn_mut && !lam.meta.escapes);
    TLambda {
        prep,
        params,
        body,
        executable,
        source_params: lam.params.iter().map(|p| p.name.clone()).collect(),
        is_move,
        boxed: lam.meta.escapes,
        arc: lam.meta.escapes
            && lam.params.len() == 1
            && matches!(lam.params[0].ty.as_ref(), Some(Type::Named(name)) if name == "HttpSrvReq"),
    }
}

/// c139 M4: lower a spawn lambda to compilable TIR for the Cranelift JIT.
pub(crate) fn lower_spawn_lambda_for_jit(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> TJitSpawnLambda {
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
            ty: env
                .ty_of(&name)
                .clone()
                .unwrap_or_else(|| Type::Named("Unit".to_string())),
            name,
        })
        .collect();
    captures.sort_by(|a, b| a.name.cmp(&b.name));

    let mut lam_env = fork_panic(env);
    for cap in &captures {
        lam_env.bind(&cap.name, mangle(&cap.name), Some(cap.ty.clone()));
    }
    for p in &lam.params {
        lam_env.bind(
            &p.name,
            mangle(&p.name),
            p.ty.clone().or_else(|| Some(Type::Int)),
        );
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
            .map(|p| (p.name.clone(), p.ty.clone().unwrap_or_else(|| Type::Int)))
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
        lam_env.bind(name, cap, None);
    }
    for p in &lam.params {
        lam_env.bind(&p.name, mangle(&p.name), p.ty.clone());
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
        prep.push_str(&format!(
            "let {} = ({}).clone();\n    ",
            cap,
            outer_env.place_of(name)
        ));
        lam_env.bind(name, cap, outer_env.ty_of(name));
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

/// c109 Phase 13: render a lambda via the plain `emit_lambda` form (used by
/// `http.serve`'s lambda handler and `scope.guard`). Returns the full closure string.
pub(crate) fn render_lambda_str(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> String {
    render_lambda_str_expecting(lam, cx, env, None)
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
    let tl = lower_lambda_expecting(lam, cx, env, expected_params);
    let move_kw = if tl.is_move { "move " } else { "" };
    let closure = format!("{}|{}| {}", move_kw, tl.params.join(", "), tl.body);
    let wrapped = if tl.boxed {
        format!("Box::new({})", closure)
    } else {
        closure
    };
    if tl.prep.is_empty() {
        wrapped
    } else {
        format!("{{ {} {} }}", tl.prep, wrapped)
    }
}

// ---------------------------------------------------------------------------
// Emission: TIR -> Rust. PURE formatting. No type inference, no decisions.
// ---------------------------------------------------------------------------

/// c109 Phase 25: render the router handler (arg 1) exactly as `emit_router_handler`
/// (Source/Codegen/Expression.rs) does, at lowering. A bare top-level fn name (not a
/// local) becomes the `Box::new(move |__req: …| user_<fn>(&__req)) as Box<dyn Fn(…) -> …
/// + Send + Sync>` wrapper; a lambda becomes `Box::new(<lambda>) as Box<…>`.
pub(crate) fn render_router_handler(
    args: &[crate::AST::CallArg],
    cx: &Cx,
    env: &LowerEnv,
) -> String {
    let root = &cx.root_prefix;
    let boxed_dyn = format!(
        "as Box<dyn Fn({}JetHttpRequest) -> {}JetHttpResponse + Send + Sync>",
        root, root
    );
    match &args[1].expr {
        Expr::Ident(name, _) if !env.locals.contains_key(name) => {
            let rust_name = mangle(name);
            format!(
                "Box::new(move |__req: {}JetHttpRequest| {}(&__req)) {}",
                root, rust_name, boxed_dyn
            )
        }
        Expr::Lambda(lam) => {
            format!(
                "Box::new({}) {}",
                render_lambda_str(lam, cx, env),
                boxed_dyn
            )
        }
        // The gate (`router_register_in_subset`) proved arg 1 is one of the two above.
        _ => unreachable!("router handler gate proved a named-fn or lambda handler"),
    }
}
