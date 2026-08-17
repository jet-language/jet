use crate::AST::{AccessConvention, Expr, Lambda, LambdaBody, Stmt, Type};
use crate::Codegen::Cx;
use crate::Codegen::mangle;
use crate::Codegen::mangle_generated;
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
use crate::Codegen::TIR::lower::{
    prepare_interrupt_callback_local_expr, prepare_interrupt_callback_locals,
};
use crate::Codegen::TIR::lower_stmts;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TJitSpawnBody;
use crate::Codegen::TIR::TJitSpawnLambda;
use crate::Codegen::TIR::TLambda;
use crate::Codegen::TIR::TLambdaBody;
use crate::Codegen::TIR::TLocal;
use crate::Codegen::TIR::view_copy_owned_type;
use crate::Codegen::TIR::view_copy_symbol;
use crate::Codegen::TIR::TStmt;
use crate::Codegen::TIR::unit_type;
use crate::Codegen::TIR::with_lambda_body_expr_cache;
use std::collections::HashSet;
use std::sync::Arc;

fn lambda_jit_name(start: usize, end: usize) -> String {
    mangle_generated(&format!("lambda_{start}_{end}"))
}

fn reactive_capture_name(name: &str) -> String {
    mangle_generated(&format!(
        "cap_{}",
        crate::Syntax::generated_suffix(&mangle(name))
    ))
}

/// D-MEM-COPYSEM1=A: resolve the owning capture type and shared Prelude
/// operation for a read-only view. Sema records the names; lowering only
/// marshals that fact into the target tier, reading the same
/// `view_copy_symbol` / `view_copy_owned_type` tables every emitter reads so a
/// captured window and a stored window can never pick different kernels.
pub(super) fn materialized_capture_kind(
    name: &str,
    env: &LowerEnv,
) -> Option<(&'static str, Type)> {
    if env.is_string_view_local(name) {
        return Some((view_copy_symbol(&Type::String), Type::String));
    }
    let source = env.split_view_handle(name).or_else(|| env.ty_of(name))?;
    let owned = view_copy_owned_type(&source)?;
    Some((view_copy_symbol(&source), owned))
}

/// c109 Phase 11: lower a lambda/closure literal (`Expr::Lambda`) to a `TLambda`.
/// Every capture/escape/Fn-vs-FnMut decision is the TOTAL `Lambda.meta` fact — no capture
/// analysis here. The body is lowered on a CLONED env extended with: the cloned
/// captures (rebound to `__jet___cap_<n>`, place = that name, type `None` — matching the
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
    lower_lambda_expecting_with_host_borrow(lam, cx, env, expected_params, None, false, None)
}

/// Lower a callback for a native helper whose Rust contract consumes each
/// payload as `Fn(T)`, rather than applying Jet's ordinary read convention.
pub(crate) fn lower_lambda_expecting_value(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: &[Type],
) -> TLambda {
    lower_lambda_expecting_with_host_borrow(lam, cx, env, Some(expected_params), None, true, None)
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
        None,
    )
}

fn lower_lambda_expecting_with_host_borrow(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: Option<&[Type]>,
    host_borrow: Option<bool>,
    by_value: bool,
    shared_body: Option<Arc<[TStmt]>>,
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
    let body_ty = shared_body
        .as_ref()
        .map(|body| lowered_block_return_ty(body))
        .unwrap_or_else(|| lambda_body_ty_expecting(lam, cx, env, expected_params));
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
    // Sema suspends transaction checks inside deferred lambdas. Do not attach
    // a foreign call in a closure to the outer transaction at codegen time.
    lam_env.txn_handle = None;
    lam_env.txn_undo_needed = None;
    // `move ` keyword: the AST emits it UNLESS the lambda is FnMut and does not escape.
    // Computed before the clone-capture prelude so a moving escape can also clone
    // borrowed/Fn captures rustc would otherwise reject (E0521).
    let is_move = !(lam.meta.needs_fn_mut && !lam.meta.escapes);
    // The clone/materialization capture prelude: `let __jet___cap_<n> =
    // (<outer place>).clone();` or its shared Prelude copy equivalent. The
    // outer place comes from the *outer* env (the capture is an outer local).
    // The cap rebinds the name with place `__jet___cap_<n>`, no deref, type
    // `None` (matching the AST slot `{ rust_name: cap, deref: false, jet_ty: None }`).
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
        let cap = reactive_capture_name(name);
        // Clone temps must be `mut` when the closure body assigns through them
        // (FnMut / captured `:=` locals). Always emit `let mut` for cloned
        // captures — over-mutability is safe; missing mut is rustc E0594 (I2).
        let materialized = lam
            .meta
            .materialized_captures
            .iter()
            .any(|capture| capture == name);
        let (cap_ty, init) = if materialized {
            if let Some((helper, ty)) = materialized_capture_kind(name, env) {
                (ty, format!("{helper}(({}))", env.place_of(name)))
            } else {
                (
                    env.ty_of(name)
                        .unwrap_or_else(|| Type::Named("Unit".to_string())),
                    format!("({}).clone()", env.place_of(name)),
                )
            }
        } else {
            (
                env.ty_of(name)
                    .unwrap_or_else(|| Type::Named("Unit".to_string())),
                format!("({}).clone()", env.place_of(name)),
            )
        };
        prep.push_str(&format!("let mut {cap} = {init};\n    "));
        captures.push((name.clone(), cap.clone(), cap_ty.clone()));
        let slot = match env.origin_of(name) {
            Some(origin) => TLocal::generated(&cap).with_origin(origin),
            None => TLocal::generated(&cap),
        };
        lam_env.bind(name, slot, Some(cap_ty));
    }
    // Taken resources (`owned :: ~next`) are neither cloned nor moved-captured in
    // sema — AOT relies on Rust lexical capture. Cranelift needs an explicit pack.
    {
        let param_names: HashSet<&str> = lam.params.iter().map(|p| p.name.as_str()).collect();
        let (mut reads, called) = match &lam.body {
            LambdaBody::Block(stmts) => crate::Sema::block_free_reads_and_calls(stmts),
            LambdaBody::Expr(e) => {
                crate::Sema::block_free_reads_and_calls(&[Stmt::Expr((**e).clone())])
            }
        };
        reads.extend(lam.take_names.iter().map(|(name, _)| name.clone()));
        // A direct call carries its callee in `Call::name`, never an
        // `Expr::Ident`, so the free-read walker cannot see a fn-valued binding
        // invoked as `f(x)` — the pack used to ship without it and the body's
        // `Local` read had nothing to resolve against. Only a fn-typed local can
        // be that callee; every other spelling is a top-level function, a
        // builtin, or a module path, and none of those is a captured value.
        for name in called {
            if matches!(env.ty_of(&name), Some(Type::Fn { .. })) {
                reads.insert(name);
            }
        }
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
            // Body still reads the outer slot (no `__jet___cap_` rebind), so the
            // capture place is that slot's own Rust name — NOT one synthesized
            // from the Jet name. An enclosing lambda or `#Reactive` block may
            // already have rebound this name to its `__jet___cap_*` clone, and
            // the body reads that generated spelling; naming the slot keeps the
            // pack and the body's local reads on the same key. Deref is dropped
            // on purpose: every engine keys a local by its bare binding name.
            captures.push((name.clone(), env.rust_name_of(&name), cap_ty));
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
    // The clone pack rebound each capture above, so this body resolves names
    // against `lam_env`, not the caller's env. Lower it on its own memo.
    let (body, executable) = with_lambda_body_expr_cache(|| match &lam.body {
        LambdaBody::Expr(e) => {
            prepare_interrupt_callback_local_expr(e, cx, &mut lam_env);
            // An expression-bodied lambda returns an owned value, just like an
            // explicit `return`; clone a borrowed non-scalar parameter here.
            let lowered = lower_owned_expr(e, cx, &mut lam_env);
            (emit_tir_expr(&lowered, cx), TLambdaBody::Expr(Box::new(lowered)))
        }
        LambdaBody::Block(stmts) => {
            if let Some(shared) = shared_body.as_ref() {
                let mut inner = String::new();
                emit_tir_lambda_block(&shared[..], cx, &mut inner, 1);
                (
                    format!("{{ {} }}", inner),
                    TLambdaBody::SharedBlock(shared.clone()),
                )
            } else {
                prepare_interrupt_callback_locals(stmts, cx, &mut lam_env);
                let lowered = lower_stmts(stmts, cx, &mut lam_env);
                let mut inner = String::new();
                emit_tir_lambda_block(&lowered, cx, &mut inner, 1);
                (format!("{{ {} }}", inner), TLambdaBody::Block(lowered))
            }
        }
    });
    cx.in_stm_transact.set(prev_in_stm);
    TLambda {
        prep,
        params,
        body,
        executable,
        source_params: lam.params.iter().map(|p| p.name.clone()).collect(),
        jit_name: lambda_jit_name(lam.span.start, lam.span.end),
        param_types,
        ret: (!matches!(&body_ty, Type::Named(name) if name == "Unit"))
            .then_some(body_ty),
        is_move,
        boxed: lam.meta.escapes,
        // Native callback helpers consume the closure as an ordinary `Fn` value.
        // Keep the `Box` escape wrapper for those call sites; `Rc<closure>` is a
        // cloneable Jet fn value, but it does not satisfy a generic `F: Fn(...)`
        // parameter because the generic bound sees the wrapper type itself.
        rc: lam.meta.escapes
            && !lam.meta.needs_fn_mut
            && !http_handler
            && !by_value
            && host_borrow.is_none(),
        arc: http_handler,
        captures,
        materialized_captures: lam.meta.materialized_captures.clone(),
        frozen_captures: lam.meta.frozen_captures.clone(),
    }
}

fn lowered_block_return_ty(body: &[TStmt]) -> Type {
    match body.last() {
        Some(TStmt::ExprStmt(expr)) => expr.ty.clone(),
        Some(TStmt::Return(Some(expr))) => expr.ty.clone(),
        _ => unit_type(),
    }
}

pub(crate) fn lower_lambda_with_shared_block(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    body: Arc<[TStmt]>,
) -> TLambda {
    lower_lambda_expecting_with_host_borrow(lam, cx, env, None, None, false, Some(body))
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
    lower_spawn_lambda_for_jit_expecting_with_body(lam, cx, env, expected_params, None)
}

fn lower_spawn_lambda_for_jit_expecting_with_body(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: &[Type],
    shared_body: Option<Arc<[TStmt]>>,
) -> TJitSpawnLambda {
    let param_names: HashSet<&str> = lam.params.iter().map(|p| p.name.as_str()).collect();
    let cloned: HashSet<&str> = lam
        .meta
        .cloned_captures
        .iter()
        .map(|s| s.as_str())
        .collect();
    let (reads, called) = match &lam.body {
        LambdaBody::Block(stmts) => crate::Sema::block_free_reads_and_calls(stmts),
        LambdaBody::Expr(e) => {
            crate::Sema::block_free_reads_and_calls(&[Stmt::Expr((**e).clone())])
        }
    };
    let mut reads = reads;
    reads.extend(lam.take_names.iter().map(|(name, _)| name.clone()));
    // Same hole as the stored-lambda pack: `f(x)` keeps its callee in
    // `Call::name`, so a fn-valued local invoked by the body never reached the
    // free-read set. Only a fn-typed local can be that callee.
    for name in called {
        if matches!(env.ty_of(&name), Some(Type::Fn { .. })) {
            reads.insert(name);
        }
    }
    let mut captures: Vec<JitSpawnCapture> = reads
        .into_iter()
        .filter(|n| !param_names.contains(n.as_str()))
        .filter(|n| env.locals.contains_key(n))
        .map(|source| {
            let name = if shared_body.is_some() {
                reactive_capture_name(&source)
            } else {
                source.clone()
            };
            let materialize_at_spawn = lam
                .meta
                .materialized_captures
                .iter()
                .any(|capture| capture == &source)
                || (shared_body.is_some()
                    && materialized_capture_kind(&source, env).is_some());
            let source_ty = env
                .split_view_handle(&source)
                .or_else(|| env.ty_of(&source))
                .unwrap_or_else(|| Type::Named("Unit".to_string()));
            let ty = if materialize_at_spawn {
                materialized_capture_kind(&source, env)
                    .map(|(_, ty)| ty)
                    .unwrap_or_else(|| source_ty.clone())
            } else {
                source_ty
            };
            JitSpawnCapture {
                materialize_at_spawn,
                clone_at_spawn: cloned.contains(source.as_str()),
                frozen_at_spawn: lam
                    .meta
                    .frozen_captures
                    .iter()
                    .any(|capture| capture == &source),
                // D-TASKBORROW1=A: an unmaterialized borrowed split-view crosses
                // as its window handle, not as the element type its Jet binding
                // shows. Read-only captures marked for materialization use the
                // owned target type above.
                ty,
                name,
                source,
            }
        })
        .collect();
    captures.sort_by(|a, b| a.name.cmp(&b.name));

    let mut lam_env = fork_panic(env);
    lam_env.txn_handle = None;
    lam_env.txn_undo_needed = None;
    for cap in &captures {
        let slot = match env.origin_of(&cap.source) {
            Some(origin) => TLocal::user(&cap.name).with_origin(origin),
            None => TLocal::user(&cap.name),
        };
        lam_env.bind(&cap.name, slot, Some(cap.ty.clone()));
    }
    for (i, p) in lam.params.iter().enumerate() {
        let ty = p
            .ty
            .clone()
            .or_else(|| expected_params.get(i).cloned())
            .or_else(|| Some(Type::Int));
        lam_env.bind(&p.name, TLocal::user(&p.name), ty);
    }

    let ret = shared_body
        .as_ref()
        .map(|body| lowered_block_return_ty(body))
        .unwrap_or_else(|| lambda_body_ty(lam, cx, env));
    if shared_body.is_none() {
        match &lam.body {
            LambdaBody::Expr(expr) => prepare_interrupt_callback_local_expr(expr, cx, &mut lam_env),
            LambdaBody::Block(stmts) => prepare_interrupt_callback_locals(stmts, cx, &mut lam_env),
        }
    }
    // The spawn pack keeps every capture on its source slot, so this body must
    // NOT reuse a value the clone pack memoized under `__jet___cap_<n>`.
    let body = with_lambda_body_expr_cache(|| match &lam.body {
        LambdaBody::Expr(e) => TJitSpawnBody::Expr(Box::new(lower_expr(e, cx, &mut lam_env))),
        LambdaBody::Block(stmts) => {
            if let Some(shared) = shared_body.as_ref() {
                TJitSpawnBody::SharedBlock {
                    body: shared.clone(),
                    tail: lambda_block_tail(stmts).is_some(),
                }
            } else if let Some((prefix, tail)) = lambda_block_tail(stmts) {
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
    });

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
        frozen_captures: lam.meta.frozen_captures.clone(),
        body,
        ret,
    }
}

pub(crate) fn lower_spawn_lambda_for_jit_with_shared_block(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    body: Arc<[TStmt]>,
) -> TJitSpawnLambda {
    lower_spawn_lambda_for_jit_expecting_with_body(lam, cx, env, &[], Some(body))
}

/// c109 Phase 13: render a canonical `task` lambda. It is `emit_lambda` minus the
/// Fn-vs-FnMut and escape logic: ALWAYS `move`, NEVER `Box::new`. The clone-capture
/// prelude is identical. Returns the full rendered closure string (wrapped in
/// `{ <prep> <closure> }` when there are cloned captures).
pub(crate) fn render_spawn_lambda(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> String {
    let mut lam_env = fork_panic(env);
    lam_env.txn_handle = None;
    lam_env.txn_undo_needed = None;
    let mut prep = String::new();
    let mut cloned_captures = lam.meta.cloned_captures.clone();
    cloned_captures.retain(|capture| {
        !lam
            .meta
            .moved_captures
            .iter()
            .any(|moved| moved == capture)
    });
    // Sema sees the parser's compiler-private `task` receiver before it is
    // rewritten to the active lexical group. The AOT body is rendered after
    // that rewrite, so a nested `task.*` call would otherwise move the parent
    // `JetTaskGroup` into the child closure and rustc would reject the second
    // nested use. Clone only this injected group handle; user TaskGroup
    // captures remain rejected by sema (E1110).
    let reads = match &lam.body {
        LambdaBody::Block(stmts) => crate::Sema::block_free_var_reads(stmts),
        LambdaBody::Expr(e) => crate::Sema::block_free_var_reads(&[Stmt::Expr((**e).clone())]),
    };
    for name in reads {
        if !cloned_captures.iter().any(|capture| capture == &name)
            && matches!(
                env.ty_of(&name),
                Some(Type::Named(ty)) if ty == crate::Syntax::TYPE_TASKGROUP
            )
        {
            cloned_captures.push(name);
        }
    }
    cloned_captures.sort();
    for name in &cloned_captures {
        let cap = reactive_capture_name(name);
        prep.push_str(&format!(
            "let {} = ({}).clone();\n    ",
            cap,
            env.place_of(name)
        ));
        let slot = match env.origin_of(name) {
            Some(origin) => TLocal::generated(&cap).with_origin(origin),
            None => TLocal::generated(&cap),
        };
        lam_env.bind(name, slot, env.ty_of(name));
    }
    for p in &lam.params {
        lam_env.bind(&p.name, TLocal::user(&p.name), p.ty.clone());
    }
    match &lam.body {
        LambdaBody::Expr(expr) => prepare_interrupt_callback_local_expr(expr, cx, &mut lam_env),
        LambdaBody::Block(stmts) => prepare_interrupt_callback_locals(stmts, cx, &mut lam_env),
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
    // Rendering the AOT closure lowers its body a second time, under this
    // render env's `__jet___cap_*` rebindings. Nested task combinators collect
    // their JIT spawn lambdas while lowering, so keep that bookkeeping
    // pass-only — table AND site map, or a later pass would dedup onto an
    // index whose entry was just discarded. The memo is scoped for the same
    // reason: this env is not the env the executable pass uses.
    let saved_spawn_lambdas = std::mem::take(&mut *cx.jit_spawn_lambdas.borrow_mut());
    let saved_spawn_sites = cx.jit_spawn_sites.borrow().clone();
    let body = with_lambda_body_expr_cache(|| match &lam.body {
        LambdaBody::Expr(e) => emit_tir_expr(&lower_expr(e, cx, &mut lam_env), cx),
        LambdaBody::Block(stmts) => render_spawn_block_body(stmts, cx, &mut lam_env),
    });
    *cx.jit_spawn_lambdas.borrow_mut() = saved_spawn_lambdas;
    *cx.jit_spawn_sites.borrow_mut() = saved_spawn_sites;
    let closure = format!("move |{}| {}", params.join(", "), body);
    if prep.is_empty() {
        closure
    } else {
        format!("{{ {} {} }}", prep, closure)
    }
}

/// Render a `#Reactive { … }` block as a `move || { … }` closure for
/// `jet_reactive_effect`. Outer locals read inside the block are cloned into
/// `__jet___cap_*` bindings (byte-for-byte the stored-lambda capture prelude).
fn reactive_capture_setup(stmts: &[Stmt], outer_env: &LowerEnv) -> (String, LowerEnv) {
    let reads = crate::Sema::block_free_var_reads(stmts);
    let mut caps: Vec<String> = reads
        .into_iter()
        .filter(|n| outer_env.locals.contains_key(n))
        .collect();
    caps.sort();
    let mut lam_env = fork_panic(outer_env);
    let mut prep = String::new();
    for name in &caps {
        let cap = reactive_capture_name(name);
        // Reactive bodies may update their private clone on every rerun. The
        // runtime serializes the resulting FnMut closure behind a Mutex.
        let (cap_ty, init) = materialized_capture_kind(name, outer_env)
            .map(|(helper, ty)| {
                (
                    Some(ty),
                    format!("{helper}(({}))", outer_env.place_of(name)),
                )
            })
            .unwrap_or_else(|| {
                (
                    outer_env.ty_of(name),
                    format!("({}).clone()", outer_env.place_of(name)),
                )
            });
        prep.push_str(&format!("let mut {cap} = {init};\n    "));
        let slot = match outer_env.origin_of(name) {
            Some(origin) => TLocal::generated(&cap).with_origin(origin),
            None => TLocal::generated(&cap),
        };
        lam_env.bind(name, slot, cap_ty);
    }
    (prep, lam_env)
}

pub(super) fn reactive_block_env(stmts: &[Stmt], cx: &Cx, outer_env: &LowerEnv) -> LowerEnv {
    let (_, mut lam_env) = reactive_capture_setup(stmts, outer_env);
    prepare_interrupt_callback_locals(stmts, cx, &mut lam_env);
    lam_env
}

pub(super) fn render_reactive_block_closure(
    stmts: &[Stmt],
    lowered: &[TStmt],
    _cx: &Cx,
    outer_env: &LowerEnv,
) -> String {
    let (prep, _) = reactive_capture_setup(stmts, outer_env);
    let mut inner = String::new();
    emit_tir_stmts(lowered, _cx, &mut inner, 1);
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

/// Render a closure for a Prelude generic callback parameter.
///
/// The Prelude owns any storage or synchronization wrapper it needs. Passing
/// `Rc`/`Arc` here changes the generic argument from the closure type to the
/// wrapper type and loses the callback's `Fn` contract at the boundary.
pub(crate) fn render_lambda_str_unboxed(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> String {
    let mut tl = lower_lambda(lam, cx, env);
    tl.boxed = false;
    tl.rc = false;
    tl.arc = false;
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

/// c109 Phase 25: render the router handler (arg 1) at lowering. A bare top-level fn
/// name (not a local)
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

#[cfg(test)]
mod tests {
    #[test]
    fn lambda_native_names_use_reserved_prefix() {
        assert_eq!(super::lambda_jit_name(12, 34), "__jet___lambda_12_34");
    }
}
