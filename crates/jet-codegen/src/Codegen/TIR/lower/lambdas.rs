use crate::Codegen::mangle;
use crate::Codegen::mangle_generated;
use crate::Codegen::Cx;
use crate::Codegen::TIR::fork_panic;
use crate::Codegen::TIR::lambda_body_ty_expecting;
use crate::Codegen::TIR::spawn_body_carrier_ty;
use crate::Codegen::TIR::lower::lambda_block_tail;
use crate::Codegen::TIR::lower::{
    lower_value_block, prepare_interrupt_callback_local_expr, prepare_interrupt_callback_locals,
    return_type_has_value, with_lambda_body_expr_cache,
};
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower_owned_expr;
use crate::Codegen::TIR::lower_stmts;
use crate::Codegen::TIR::unit_type;
use crate::Codegen::TIR::view_copy_owned_type;
use crate::Codegen::TIR::view_copy_symbol;
use crate::Codegen::TIR::JitSpawnCapture;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TJitSpawnBody;
use crate::Codegen::TIR::TJitSpawnLambda;
use crate::Codegen::TIR::TLambda;
use crate::Codegen::TIR::TLambdaBody;
use crate::Codegen::TIR::TLocal;
use crate::Codegen::TIR::TStmt;
use crate::AST::{Expr, Lambda, LambdaBody, Stmt, Type};
use std::collections::HashSet;
use std::sync::Arc;

fn lambda_jit_name(start: usize, end: usize) -> String {
    mangle_generated(&format!("lambda_{start}_{end}"))
}
/// Return the statically known identity for a task body, when it is a direct
/// named function call or function reference. Arbitrary expressions stay
/// unlabeled so the runtime owns the single `task@<site>` fallback.
pub(crate) fn spawn_label(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> Option<String> {
    let body = match &lam.body {
        LambdaBody::Expr(expr) => expr.as_ref(),
        LambdaBody::Block(stmts) => {
            let [stmt] = stmts.as_slice() else {
                return None;
            };
            match stmt {
                Stmt::Expr(expr) | Stmt::Return(Some(expr), _) => expr,
                _ => return None,
            }
        }
    };
    let mut expr = body;
    while let Expr::Paren(inner, _) | Expr::Try(inner, _, _, _) = expr {
        expr = inner;
    }
    match expr {
        Expr::Call(call) if matches!(cx.fn_types.get(&call.name), Some(Type::Fn { .. })) => {
            Some(call.name.clone())
        }
        Expr::Ident(name, _)
            if !env.locals.contains_key(name)
                && matches!(cx.fn_types.get(name), Some(Type::Fn { .. })) =>
        {
            Some(name.clone())
        }
        _ => None,
    }
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
/// captures (rebound to `__jet___cap_<n>`, place = that name, and their checked
/// capture type) and the params (place = mangled name, type from the annotation).
/// The rendered closure body string is produced now so emit is a pure wrapper.
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
    lower_lambda_expecting_with_host_borrow(lam, cx, env, expected_params, None, false, None, None)
}
/// Lower a lambda for a checked function-value or host callback slot. The
/// slot's return is the ABI fact: effective `Result`/`Option` slots retain
/// their carrier, while raw host returns stay infallible.
pub(crate) fn lower_lambda_expecting_callable(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected: &Type,
) -> TLambda {
    let Type::Fn {
        params,
        ret,
        ..
    } = expected
    else {
        return lower_lambda_expecting(lam, cx, env, None);
    };
    // A fn slot with no written return type is still a Unit-returning Rust
    // callable. Preserve that slot fact so inferred callback carriers cannot
    // widen an infallible callback behind the adapter's back.
    let slot_return = ret.as_deref().cloned().unwrap_or_else(unit_type);
    lower_lambda_expecting_with_host_borrow(
        lam,
        cx,
        env,
        Some(params.as_slice()),
        None,
        false,
        None,
        Some(&slot_return),
    )
}

/// Lower a callback for a native helper whose Rust contract consumes each
/// payload as `Fn(T)`, rather than applying Jet's ordinary read convention.
pub(crate) fn lower_lambda_expecting_value(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: &[Type],
) -> TLambda {
    lower_lambda_expecting_with_host_borrow(
        lam,
        cx,
        env,
        Some(expected_params),
        None,
        true,
        None,
        None,
    )
}
/// Lower an owning native callback while preserving its checked return slot.
/// `expected_return` is the host `Fn(T) -> R` ABI, not the enclosing Jet
/// function's failure carrier.
pub(crate) fn lower_lambda_expecting_value_with_return(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: &[Type],
    expected_return: &Type,
) -> TLambda {
    lower_lambda_expecting_with_host_borrow(
        lam,
        cx,
        env,
        Some(expected_params),
        None,
        true,
        None,
        Some(expected_return),
    )
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
        None,
    )
}
/// Lower a collection comparator whose host callback has an explicit raw return.
/// The comparator is lent `&T` inputs like the ordinary collection callback, but
/// its return slot is part of the helper ABI and must not inherit Jet's ambient
/// failure carrier.
pub(crate) fn lower_lambda_expecting_host_borrow_with_return(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: &[Type],
    write: bool,
    expected_return: &Type,
) -> TLambda {
    lower_lambda_expecting_with_host_borrow(
        lam,
        cx,
        env,
        Some(expected_params),
        Some(write),
        false,
        None,
        Some(expected_return),
    )
}

/// D-FAILURE-FOUNDATION1: mirror sema's implicit `Error` carrier for a lambda
/// that writes a success/error annotation. The AST keeps those two source
/// slots separate, while the lowered callable must expose one Rust carrier.
fn lambda_explicit_failure_carrier(lam: &Lambda) -> Option<Type> {
    if lam.result_type.is_none() && lam.error_type.is_none() {
        return None;
    }
    Some(Type::Result {
        ok: Box::new(lam.result_type.clone().unwrap_or_else(unit_type)),
        err: Box::new(
            lam.error_type
                .clone()
                .unwrap_or_else(|| Type::Named(crate::Syntax::TYPE_ERR.to_string())),
        ),
    })
}

fn is_default_error_type(ty: &Type) -> bool {
    matches!(ty, Type::Named(name) if name == crate::Syntax::TYPE_ERR)
}

fn lambda_carrier_return_type(body_ty: &Type, carrier: &Type) -> Type {
    match (body_ty, carrier) {
        (
            Type::Result {
                err: body_error, ..
            },
            Type::Result { err, .. },
        ) if body_error == err => body_ty.clone(),
        (
            Type::Result {
                ok: body_ok,
                err: body_error,
            },
            Type::Result {
                ok: carrier_ok,
                err,
            },
        ) if is_default_error_type(body_error) && body_ok == carrier_ok => Type::Result {
            ok: carrier_ok.clone(),
            err: err.clone(),
        },
        (Type::Option(_), Type::Option(_)) => body_ty.clone(),
        (
            _,
            Type::Result {
                err,
                ..
            },
        ) => Type::Result {
            ok: Box::new(body_ty.clone()),
            err: err.clone(),
        },
        (body_ty, Type::Option(_)) => Type::Option(Box::new(body_ty.clone())),
        (_, other) => other.clone(),
    }
}

fn rehome_default_result_carrier(mut value: TExpr, carrier: Option<&Type>) -> TExpr {
    let Some(Type::Result {
        ok: carrier_ok,
        err: carrier_err,
    }) = carrier
    else {
        return value;
    };
    let Type::Result {
        ok: value_ok,
        err: value_err,
    } = &value.ty
    else {
        return value;
    };
    if is_default_error_type(value_err) && value_ok == carrier_ok {
        value.ty = Type::Result {
            ok: carrier_ok.clone(),
            err: carrier_err.clone(),
        };
    }
    value
}

fn lower_lambda_expecting_with_host_borrow(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: Option<&[Type]>,
    host_borrow: Option<bool>,
    by_value: bool,
    shared_body: Option<Arc<[TStmt]>>,
    expected_return: Option<&Type>,
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
    let mut body_ty = shared_body
        .as_ref()
        .map(|body| lowered_block_return_ty(body))
        .unwrap_or_else(|| lambda_body_ty_expecting(lam, cx, env, expected_params));
    // `emit_lambda` clones the env (`lam_env = env.clone()`), so a `??` panic inside the
    // lambda body dumps the lambda's lexical env (outer locals + captures + params) and
    // does not leak its own bindings into the enclosing function. The lambda's return
    // carrier is its own body result plus the callback slot's failure type; inheriting
    // the enclosing function's success type would erase a callback tail such as `true`
    // to `Unit`.
    let explicit_failure_carrier = lambda_explicit_failure_carrier(lam);
    let lambda_ret_ty = expected_return
        .or(lam.meta.fallible_carrier.as_ref())
        .or(explicit_failure_carrier.as_ref())
        .or_else(|| {
            lam.meta
                .fallible_propagation
                .then(|| env.ret_ty.as_ref())
                .flatten()
        })
        .map(|ret| lambda_carrier_return_type(&body_ty, ret))
        .unwrap_or_else(|| body_ty.clone());
    // A lambda called immediately in a fallible outer expression still needs the
    // outer error carrier while lowering `??`, but it never escapes that call.
    let direct_fallible = expected_return.is_none()
        && lam.meta.fallible_carrier.is_none()
        && explicit_failure_carrier.is_none()
        && env
            .ret_ty
            .as_ref()
            .is_some_and(|ty| matches!(ty, Type::Result { .. } | Type::Option(_)))
        && !by_value
        && host_borrow.is_none();
    let mut lam_env = fork_panic(env);
    // A fallback marker belongs to the enclosing subject, never to a deferred
    // callback body or its type probe.
    lam_env.fallback_subject = false;
    lam_env.ret_ty = Some(lambda_ret_ty.clone());
    lam_env.raw_protocol_return = false;
    // Sema suspends transaction checks inside deferred lambdas. Do not attach a
    // foreign call in a closure to the outer transaction at codegen time.
    lam_env.txn_handle = None;
    lam_env.txn_undo_needed = None;
    // `move ` keyword: the AST emits it UNLESS the lambda is FnMut and does not escape.
    // Computed before the clone-capture prelude so a moving escape can also clone
    // borrowed/Fn captures rustc would otherwise reject (E0521).
    let is_move = !(lam.meta.needs_fn_mut && !lam.meta.escapes);
    // The clone/materialization capture prelude: `let __jet___cap_<n> =
    // (<outer place>).clone();` or its shared Prelude copy equivalent. The
    // outer place comes from the *outer* env (the capture is an outer local).
    // The cap rebinds the name with place `__jet___cap_<n>` and its checked
    // capture type.
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
            LambdaBody::Expr(e) => crate::Sema::expr_free_reads_and_calls(e).0,
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
            let needs_clone =
                env.is_borrowed(&name) || matches!(env.ty_of(&name), Some(Type::Fn { .. }));
            if needs_clone {
                extra_cloned.push(name);
            }
        }
    }
    for name in lam.meta.cloned_captures.iter().chain(extra_cloned.iter()) {
        let cap = reactive_capture_name(name);
        // Clone temps must be `mut` when the closure body assigns through them
        // (FnMut / captured `:=` locals). Always emit `let mut` for cloned
        // captures — over-mutability is safe; missing mut is rustc E0594 (I2).
        let materialized = lam
            .meta
            .materialized_captures
            .iter()
            .any(|capture| capture == name);
        let copied_window = materialized
            .then(|| materialized_capture_kind(name, env))
            .flatten();
        let cap_ty = if let Some((_, ty)) = &copied_window {
            ty.clone()
        } else {
            env.ty_of(name)
                .expect("checked capture local has no resolved type")
        };
        captures.push((name.clone(), cap.clone(), cap_ty.clone()));
        let slot = TLocal::generated(&cap);
        lam_env.bind(name, slot, Some(cap_ty));
        // D-MEM-COPYSEM1=A: the capture slot now OWNS its bytes. Inside the
        // body the name is no longer a window, so an owning read of it clones
        // the owned value instead of calling the window kernel a second time
        // with an owned argument (rustc E0308 — I2).
        if copied_window.is_some() {
            lam_env.clear_view_marks(name);
        }
    }
    // Taken resources (`owned :: ~next`) are neither cloned nor moved-captured in
    // sema — AOT relies on Rust lexical capture. Cranelift needs an explicit pack.
    {
        let param_names: HashSet<&str> = lam.params.iter().map(|p| p.name.as_str()).collect();
        let (mut reads, called) = match &lam.body {
            LambdaBody::Block(stmts) => crate::Sema::block_free_reads_and_calls(stmts),
            LambdaBody::Expr(e) => crate::Sema::expr_free_reads_and_calls(e),
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
                .expect("checked capture local has no resolved type");
            // Capture rows use TLocal's canonical slot name, not its rendered
            // Rust spelling.  Feeding `__jet_offset` back through
            // `TLocal::generated` turns it into the distinct generated lane
            // `__jet___offset`, leaving the checked local table with a name
            // that the body never bound.  A user slot therefore remains
            // `offset`; an already-generated slot keeps its generated name.
            captures.push((name.clone(), env.local_of(&name).name, cap_ty));
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
    // Body lowering uses executable TIR. AOT rendering is owned by MIR.
    let prev_in_stm = cx.in_stm_transact.replace(false);
    // The clone pack rebound each capture above, so this body resolves names
    // against `lam_env`, not the caller's env. Lower it on its own memo.
    let executable = with_lambda_body_expr_cache(|| match &lam.body {
        LambdaBody::Expr(e) => {
            prepare_interrupt_callback_local_expr(e, cx, &mut lam_env);
            let lowered = lower_owned_expr(e, cx, &mut lam_env);
            let lowered = fallible_lambda_value(lowered, lam, env, expected_return);
            body_ty = lowered.ty.clone();
            TLambdaBody::Expr(Box::new(lowered))
        }
        LambdaBody::Block(stmts) => {
            if let Some(shared) = shared_body.as_ref() {
                TLambdaBody::SharedBlock(shared.clone())
            } else {
                prepare_interrupt_callback_locals(stmts, cx, &mut lam_env);
                let mut lowered = if matches!(stmts.last(), Some(Stmt::Expr(_))) {
                    lower_value_block(stmts, cx, &mut lam_env)
                } else if return_type_has_value(&body_ty) {
                    lower_value_block(stmts, cx, &mut lam_env)
                } else {
                    lower_stmts(stmts, cx, &mut lam_env)
                };
                if lam.meta.fallible_propagation
                    || expected_return.is_some_and(|ty| {
                        matches!(ty, Type::Result { .. } | Type::Option(_))
                    })
                {
                    lift_fallible_lambda_returns(&mut lowered, lam, env, expected_return);
                }
                body_ty = lowered_block_return_ty(&lowered);
                TLambdaBody::Block(lowered)
            }
        }
    });
    cx.in_stm_transact.set(prev_in_stm);
    let uses_stack_sentry = lam_env.stack_sentry_needed();
    let mut capture_facts = crate::Codegen::TIR::lambda_capture_facts(lam);
    // `extra_cloned` is a producer-owned lifetime fact: unlike sema's
    // explicit clone list, these slots are inserted here to make a moving
    // closure own borrowed/Fn captures.  Carry the fact onto the target MIR
    // row so its capture parameter and the enclosing Closure operand agree.
    capture_facts.cloned.extend(extra_cloned.iter().cloned());
    TLambda {
        executable,
        source_span: lam.span,
        frame_schedule: lam.meta.frame_schedule.clone(),
        frame_schedule_derivation: lam.meta.frame_schedule_derivation.clone(),
        capture_facts,
        effects: crate::Codegen::TIR::lambda_effect_facts(lam),
        host_param_conventions: (by_value && host_borrow.is_none())
            .then(|| vec![crate::AST::AccessConvention::Move; param_types.len()]),
        source_params: lam.params.iter().map(|p| p.name.clone()).collect(),
        jit_name: lambda_jit_name(lam.span.start, lam.span.end),
        param_types,
        failure_carrier: if expected_return.is_some() {
            // A checked callable slot owns the host ABI.  Its raw return is
            // already the boundary decision: `Unit`/a value is infallible,
            // while `Result`/`Option` carries its own failure rail.  Do not
            // reapply the lambda's implicit default `Result` here; that
            // default is for standalone Jet callables and would leave a
            // host `Fn() -> Unit` with a Result carrier and no return slot.
            crate::Codegen::TIR::TFailureCarrier::from_checked_type(&lambda_ret_ty)
        } else {
            match crate::Codegen::TIR::lambda_failure_carrier(lam) {
                crate::Codegen::TIR::TFailureCarrier::Infallible => {
                    crate::Codegen::TIR::TFailureCarrier::from_checked_type(&body_ty)
                }
                carrier => carrier,
            }
        },
        ret: (!matches!(&body_ty, Type::Named(name) if name == "Unit")).then_some(body_ty),
        is_move,
        boxed: lam.meta.escapes && !direct_fallible,
        rc: lam.meta.escapes
            && !lam.meta.needs_fn_mut
            && !by_value
            && host_borrow.is_none()
            && !direct_fallible,
        arc: false,
        captures,
        materialized_captures: lam.meta.materialized_captures.clone(),
        frozen_captures: lam.meta.frozen_captures.clone(),
        uses_stack_sentry,
    }
}

/// Lift the successful result of a callback that propagates into its enclosing
/// failure carrier. The `?` nodes already carry failures out of the callback;
/// this supplies the matching `Ok`/`Some` on the success path.
fn fallible_lambda_value(
    value: TExpr,
    lam: &Lambda,
    env: &LowerEnv,
    expected_return: Option<&Type>,
) -> TExpr {
    let explicit_failure_carrier = lambda_explicit_failure_carrier(lam);
    let carrier = expected_return
        .or(lam.meta.fallible_carrier.as_ref())
        .or(explicit_failure_carrier.as_ref())
        .or_else(|| {
            lam.meta
                .fallible_propagation
                .then(|| env.ret_ty.as_ref())
                .flatten()
        });
    if expected_return.is_none()
        && lam.meta.fallible_carrier.is_none()
        && !lam.meta.fallible_propagation
        && explicit_failure_carrier.is_none()
    {
        return value;
    }
    let value = rehome_default_result_carrier(value, carrier);
    match carrier {
        Some(Type::Result { err, .. }) if !matches!(&value.ty, Type::Result { .. }) => TExpr {
            ty: Type::Result {
                ok: Box::new(value.ty.clone()),
                err: err.clone(),
            },
            kind: TExprKind::Ok(Box::new(value)),
        },
        Some(Type::Option(_)) if !matches!(&value.ty, Type::Option(_)) => TExpr {
            ty: Type::Option(Box::new(value.ty.clone())),
            kind: TExprKind::Present(Box::new(value)),
        },
        _ => value,
    }
}

/// A block callback may return through a tail, an explicit return, or a
/// nested control-flow arm. Every such route belongs to the callback, so a
/// propagated failure must leave the callback through its own carrier rather
/// than a raw success value.
fn lift_fallible_lambda_returns(
    stmts: &mut Vec<TStmt>,
    lam: &Lambda,
    env: &LowerEnv,
    expected_return: Option<&Type>,
) {
    lift_fallible_lambda_return_block(stmts, lam, env, expected_return);
    if matches!(stmts.last(), Some(TStmt::Return(_))) {
        return;
    }
    if let Some(TStmt::ExprStmt(value)) = stmts.last_mut() {
        let return_value = std::mem::replace(
            value,
            TExpr {
                ty: unit_type(),
                kind: TExprKind::Unit,
            },
        );
        *value = fallible_lambda_value(return_value, lam, env, expected_return);
        return;
    }
    stmts.push(TStmt::Return(Some(fallible_lambda_value(
        TExpr {
            ty: unit_type(),
            kind: TExprKind::Unit,
        },
        lam,
        env,
        expected_return,
    ))));
}

fn lift_fallible_lambda_return_block(
    stmts: &mut Vec<TStmt>,
    lam: &Lambda,
    env: &LowerEnv,
    expected_return: Option<&Type>,
) {
    for stmt in stmts {
        lift_fallible_lambda_return_stmt(stmt, lam, env, expected_return);
    }
}

fn lift_fallible_lambda_return_stmt(
    stmt: &mut TStmt,
    lam: &Lambda,
    env: &LowerEnv,
    expected_return: Option<&Type>,
) {
    match stmt {
        TStmt::Return(value) => {
            let return_value = value.take().unwrap_or_else(|| TExpr {
                ty: unit_type(),
                kind: TExprKind::Unit,
            });
            *value = Some(fallible_lambda_value(return_value, lam, env, expected_return));
        }
        TStmt::ContractScope { body, .. }
        | TStmt::TaskGroup { body, .. }
        | TStmt::Loop { body, .. }
        | TStmt::While { body, .. }
        | TStmt::Range { body, .. }
        | TStmt::ForIn { body, .. }
        | TStmt::Inline(body)
        | TStmt::DebugOnly(body)
        | TStmt::Unsafe { body, .. }
        | TStmt::SentryPolicy { body, .. }
        | TStmt::Impure(body)
        | TStmt::Region(body)
        | TStmt::Layout { body, .. }
        | TStmt::ContextBlock { body, .. }
        | TStmt::Live { body }
        | TStmt::Shield { body }
        | TStmt::ScopeMember { body, .. }
        | TStmt::Transact { body, .. } => {
            lift_fallible_lambda_return_block(body, lam, env, expected_return)
        }
        TStmt::RefutableBind { fallback, .. } => {
            lift_fallible_lambda_return_block(fallback, lam, env, expected_return)
        }
        TStmt::GcEdit { stmt, .. } => {
            lift_fallible_lambda_return_stmt(stmt, lam, env, expected_return)
        }
        TStmt::CountedLoop {
            init, step, body, ..
        } => {
            lift_fallible_lambda_return_stmt(init, lam, env, expected_return);
            if let Some(step) = step {
                lift_fallible_lambda_return_stmt(step, lam, env, expected_return);
            }
            lift_fallible_lambda_return_block(body, lam, env, expected_return);
        }
        TStmt::If {
            then_body,
            else_body,
            ..
        } => {
            lift_fallible_lambda_return_block(then_body, lam, env, expected_return);
            if let Some(else_body) = else_body {
                lift_fallible_lambda_return_block(else_body, lam, env, expected_return);
            }
        }
        TStmt::EnumMatch {
            arms, else_body, ..
        } => {
            for arm in arms {
                lift_fallible_lambda_return_block(&mut arm.body, lam, env, expected_return);
            }
            if let Some(else_body) = else_body {
                lift_fallible_lambda_return_block(else_body, lam, env, expected_return);
            }
        }
        TStmt::RangeSwitch {
            arms, else_body, ..
        } => {
            for (_, _, body) in arms {
                lift_fallible_lambda_return_block(body, lam, env, expected_return);
            }
            lift_fallible_lambda_return_block(else_body, lam, env, expected_return);
        }
        TStmt::MixedSwitch {
            arms, else_body, ..
        } => {
            for (_, body) in arms {
                lift_fallible_lambda_return_block(body, lam, env, expected_return);
            }
            if let Some(else_body) = else_body {
                lift_fallible_lambda_return_block(else_body, lam, env, expected_return);
            }
        }
        // A reactive body owns a separate closure and therefore has its
        // own return carrier metadata.
        TStmt::Reactive { .. }
        | TStmt::Let { .. }
        | TStmt::ExprStmt(_)
        | TStmt::Contract { .. }
        | TStmt::SplitViews { .. }
        | TStmt::TupleDestructure { .. }
        | TStmt::StructDestructure { .. }
        | TStmt::ListDestructure { .. }
        | TStmt::Assign { .. }
        | TStmt::DeferClose { .. }
        | TStmt::Break(_)
        | TStmt::BreakValue { .. }
        | TStmt::Continue(_)
        | TStmt::IndexAssign { .. }
        | TStmt::IndexFieldAssign(_)
        | TStmt::IndexHookAssign { .. }
        | TStmt::MathSwizzleAssign { .. }
        | TStmt::LineMarker(_)
        | TStmt::SourceSpan(_)
        | TStmt::Erased { .. }
        | TStmt::InvariantViolation { .. } => {}
    }
}

fn lowered_block_return_ty(body: &[TStmt]) -> Type {
    match body.last() {
        Some(TStmt::ExprStmt(expr)) => expr.ty.clone(),
        Some(TStmt::Return(Some(expr))) => expr.ty.clone(),
        // `lower_return_value` keeps the value alive across a lexical temp
        // before emitting `return`, so a tail expression commonly finishes as
        // an inline `[let; return]` block rather than a bare `Return`.
        Some(TStmt::Inline(body)) | Some(TStmt::DebugOnly(body)) => lowered_block_return_ty(body),
        _ => unit_type(),
    }
}

pub(crate) fn lower_lambda_with_shared_block(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    body: Arc<[TStmt]>,
) -> TLambda {
    lower_lambda_expecting_with_host_borrow(lam, cx, env, None, None, false, Some(body), None)
}

/// c139 M4: lower a spawn lambda to compilable TIR for the Cranelift JIT.
pub(crate) fn lower_spawn_lambda_for_jit(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> TJitSpawnLambda {
    lower_spawn_lambda_for_jit_expecting(lam, cx, env, &[])
}

pub(crate) fn lower_spawn_lambda_for_jit_unit(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
) -> TJitSpawnLambda {
    lower_spawn_lambda_for_jit(lam, cx, env)
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
    let moved: HashSet<&str> = lam
        .meta
        .moved_captures
        .iter()
        .map(|s| s.as_str())
        .collect();
    let (reads, called) = match &lam.body {
        LambdaBody::Block(stmts) => crate::Sema::block_free_reads_and_calls(stmts),
        LambdaBody::Expr(e) => crate::Sema::expr_free_reads_and_calls(e),
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
                || (shared_body.is_some() && materialized_capture_kind(&source, env).is_some());
            let source_ty = env
                .split_view_handle(&source)
                .or_else(|| env.ty_of(&source))
                .expect("checked capture local has no resolved type");
            let ty = if materialize_at_spawn {
                match materialized_capture_kind(&source, env) {
                    Some((_, ty)) => ty,
                    None => source_ty.clone(),
                }
            } else {
                source_ty
            };
            JitSpawnCapture {
                materialize_at_spawn,
                // Clone once when the closure owns a retained environment;
                // an explicit consuming capture takes precedence.
                clone_at_spawn: cloned.contains(source.as_str()) && !moved.contains(source.as_str()),
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
        let slot = TLocal::user(&cap.name);
        lam_env.bind(&cap.name, slot, Some(cap.ty.clone()));
    }
    for (i, p) in lam.params.iter().enumerate() {
        let ty =
            p.ty.clone()
                .or_else(|| expected_params.get(i).cloned())
                .or_else(|| Some(Type::Int));
        lam_env.bind(&p.name, TLocal::user(&p.name), ty);
    }

    let ret = shared_body
        .as_ref()
        .map(|body| {
            let body_ty = lowered_block_return_ty(body);
            if let Some(Type::Result { err, .. }) = lam.meta.fallible_carrier.as_ref() {
                Type::Result {
                    ok: Box::new(body_ty),
                    err: err.clone(),
                }
            } else if lam.meta.fallible_propagation {
                match env.ret_ty.as_ref() {
                    Some(Type::Result { err, .. }) => Type::Result {
                        ok: Box::new(body_ty),
                        err: err.clone(),
                    },
                    Some(Type::Option(_)) => Type::Option(Box::new(body_ty)),
                    _ => body_ty,
                }
            } else {
                body_ty
            }
        })
        .unwrap_or_else(|| spawn_body_carrier_ty(lam, cx, env));
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
    let uses_stack_sentry = lam_env.stack_sentry_needed();

    TJitSpawnLambda {
        params: lam
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                (
                    p.name.clone(),
                    p.ty.clone()
                        .or_else(|| expected_params.get(i).cloned())
                        .unwrap_or_else(|| Type::Int),
                )
            })
            .collect(),
        captures,
        frozen_captures: lam.meta.frozen_captures.clone(),
        body,
        ret,
        uses_stack_sentry,
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

/// D-CONC-SPAWN1: every AOT task closure returns the uniform `Result<T, Err>`
/// carrier. The source-level task still exposes `T`; `join` maps this private
/// error value onto the public `TaskFailure` rail.
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
        let copied_window = materialized_capture_kind(name, outer_env);
        let (cap_ty, init) = match &copied_window {
            Some((helper, ty)) => (
                Some(ty.clone()),
                format!("{helper}(({}))", outer_env.place_of(name)),
            ),
            None => (
                outer_env.ty_of(name),
                format!("({}).clone()", outer_env.place_of(name)),
            ),
        };
        prep.push_str(&format!("let mut {cap} = {init};\n    "));
        let slot = TLocal::generated(&cap);
        lam_env.bind(name, slot, cap_ty);
        // Same owning-slot fact as the stored-lambda prelude above: once the
        // window has been copied out, the body's name is an owned value.
        if copied_window.is_some() {
            lam_env.clear_view_marks(name);
        }
    }
    (prep, lam_env)
}

pub(super) fn reactive_block_env(stmts: &[Stmt], cx: &Cx, outer_env: &LowerEnv) -> LowerEnv {
    let (_, mut lam_env) = reactive_capture_setup(stmts, outer_env);
    prepare_interrupt_callback_locals(stmts, cx, &mut lam_env);
    lam_env
}

#[cfg(test)]
mod tests {
    #[test]
    fn lambda_native_names_use_reserved_prefix() {
        assert_eq!(super::lambda_jit_name(12, 34), "__jet___lambda_12_34");
    }
}
