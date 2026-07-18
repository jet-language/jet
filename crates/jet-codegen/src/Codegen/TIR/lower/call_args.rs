use crate::AST::{AccessConvention, Expr, Lambda, LambdaBody, Stmt, Type};
use crate::Codegen::Cx;
use crate::Codegen::mangle;
use crate::Codegen::TIR::clone_env;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower_lambda_expecting;
use crate::Codegen::TIR::TCallArg;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TExternArg;
use crate::Codegen::TIR::TFnCoerce;
use crate::Codegen::TIR::unit_type;

/// Last expression-producing statement in a lambda block (mirrors sema tail rules).
/// Only the **final** statement may be a tail; an earlier `send()`/`call()` followed
/// by a loop is not a tail expression.
pub(super) fn lambda_block_tail<'a>(stmts: &'a [Stmt]) -> Option<(&'a [Stmt], &'a Stmt)> {
    let last_idx = stmts.len().checked_sub(1)?;
    let last = &stmts[last_idx];
    match last {
        Stmt::Return(Some(_), _) | Stmt::Expr(_) => Some((&stmts[..last_idx], last)),
        _ => None,
    }
}

/// c109 Phase 13: the type of a lambda's body (its return), used for a `spawn`ed
/// closure's `Task<T>` element type. Block bodies use the tail expression/return
/// (same rule as sema), not `Unit`.
pub(crate) fn lambda_body_ty(lam: &Lambda, cx: &Cx, env: &LowerEnv) -> Type {
    lambda_body_ty_expecting(lam, cx, env, None)
}

/// `lambda_body_ty`, but seeding a bare (unannotated) param from `expected_params`
/// at the same position — same fallback `lower_lambda_expecting` uses (D-MEM1 S6:
/// `Shared<T>.read(s => …)`'s bare `s` needs its real type here too, or the
/// closure's OWN return type comes back wrong for a chained field/method read).
pub(crate) fn lambda_body_ty_expecting(
    lam: &Lambda,
    cx: &Cx,
    env: &LowerEnv,
    expected_params: Option<&[Type]>,
) -> Type {
    fn bind_params(lam: &Lambda, env: &LowerEnv, expected_params: Option<&[Type]>) -> LowerEnv {
        let mut lam_env = clone_env(env);
        for (i, p) in lam.params.iter().enumerate() {
            let ty =
                p.ty.clone()
                    .or_else(|| expected_params.and_then(|ps| ps.get(i)).cloned());
            lam_env.locals.insert(p.name.clone(), (mangle(&p.name), ty));
        }
        lam_env
    }
    match &lam.body {
        LambdaBody::Expr(e) => {
            let mut lam_env = bind_params(lam, env, expected_params);
            lower_expr(e, cx, &mut lam_env).ty
        }
        LambdaBody::Block(stmts) => {
            let Some((_, tail)) = lambda_block_tail(stmts) else {
                return unit_type();
            };
            let mut lam_env = bind_params(lam, env, expected_params);
            match tail {
                Stmt::Return(Some(e), _) | Stmt::Expr(e) => lower_expr(e, cx, &mut lam_env).ty,
                _ => unit_type(),
            }
        }
    }
}

/// c109 Phase 6/13: lower method-call arguments, mirroring `emit_call_args`
/// (Source/Codegen/Expression.rs). The clone/Arc wrappers, the borrow/mut-borrow
/// wrappers, and the Fn-typed Box-coercion are all decided here from total facts
/// (`CallArg.flags` + the resolved param convention/type), never re-derived in emit.
pub(crate) fn lower_method_args(
    args: &[crate::AST::CallArg],
    sig: &[(AccessConvention, Type)],
    env: &mut LowerEnv,
    cx: &Cx,
) -> Vec<TCallArg> {
    args.iter()
        .enumerate()
        .map(|(i, a)| {
            let conv = sig.get(i).map(|(c, t)| (*c, t.clone()));
            lower_one_call_arg(a, conv, env, cx)
        })
        .collect()
}

/// c109 Phase 13: lower ONE call argument, reproducing `emit_call_args`
/// (Source/Codegen/Expression.rs) byte-for-byte — the single source of truth for
/// the clone/Arc, Fn-coercion, and borrow/mut-borrow wrapper order. `conv` is the
/// resolved param `(convention, type)` for this position (`None` when the callee has
/// no known signature, e.g. a `CallValue`). The emit order is exactly the AST path's:
///   1. the implicit-clone / Arc-clone wrapper (`(…).clone()` / `Arc::clone(&…)`);
///   2. the Fn-typed Box-coercion (`Box::new(…) as <fn-type>`, or just ` as <fn-type>`
///      when already boxed);
///   3. the borrow wrapper (`&(…)` for a `Read` non-scalar non-Fn, `&mut (…)` for a
///      `Mutate`).
pub(crate) fn lower_one_call_arg(
    a: &crate::AST::CallArg,
    conv: Option<(AccessConvention, Type)>,
    env: &mut LowerEnv,
    cx: &Cx,
) -> TCallArg {
    let resource_move = matches!(
        (&a.expr, &conv),
        (Expr::Ident(name, _), Some((AccessConvention::Move, _))) if env.is_resource(name)
    );
    // A bare lambda flowing into a user fn-typed parameter takes its param
    // types from that fn-type so codegen emits the Rust closure-param types
    // rustc needs (c142). Other args lower normally.
    let value = match (&a.expr, &conv) {
        (Expr::Ident(name, _), Some((AccessConvention::Move, ty))) if env.is_resource(name) => {
            TExpr {
                ty: ty.clone(),
                kind: TExprKind::ResourceTake(env.rust_name_of(name)),
            }
        }
        (Expr::Ident(name, _), Some((_, Type::Fn { .. }))) if a.flags.c_callback_symbol => TExpr {
            ty: conv.as_ref().map(|(_, t)| t.clone()).unwrap(),
            kind: TExprKind::ConstInline(cx.mangle_name(name)),
        },
        (Expr::Lambda(lam), Some((_, Type::Fn { params, ret, .. })))
            if a.flags.c_callback_symbol =>
        {
            let tl = lower_lambda_expecting(lam, cx, env, Some(params.as_slice()));
            let name = format!("__jet_c_callback_{}_{}", lam.span.start, lam.span.end);
            let ret = ret
                .as_deref()
                .map(|ty| format!(" -> {}", cx.rust_type(ty)))
                .unwrap_or_default();
            let body = if tl.body.starts_with('{') {
                tl.body
            } else {
                format!("{{ {} }}", tl.body)
            };
            TExpr {
                ty: conv.as_ref().map(|(_, t)| t.clone()).unwrap(),
                kind: TExprKind::ConstInline(format!(
                    "{{ extern \"C\" fn {}({}){} {} {} }}",
                    name,
                    tl.params.join(", "),
                    ret,
                    body,
                    name
                )),
            }
        }
        (Expr::Lambda(lam), Some((_, Type::Fn { params, .. }))) => {
            let tl = lower_lambda_expecting(lam, cx, env, Some(params.as_slice()));
            TExpr {
                ty: Type::Fn {
                    params: Vec::new(),
                    ret: None,
                    effect_bound: None,
                },
                kind: TExprKind::Lambda(Box::new(tl)),
            }
        }
        _ => lower_expr(&a.expr, cx, env),
    };
    let clone = !resource_move
        && (a.flags.implicit_clone
        || matches!(
            (&a.expr, conv.as_ref()),
            (Expr::Ident(name, _), None | Some((AccessConvention::Move, _)))
                if env.is_borrowed(name)
                    && env.ty_of(name).is_some_and(|ty| !ty.is_scalar())
        ));
    let arc_clone = a.flags.shared_auto_clone;
    // The Fn-typed Box-coercion (`emit_call_args`' `if let Some((_, Type::Fn …))`).
    let fn_coerce = match &conv {
        Some((_, Type::Fn { .. })) if a.flags.c_callback_symbol => None,
        Some((_, Type::Fn { .. })) => {
            // `already_boxed`: the value already produces a `Box::new(…)`. The AST
            // checks two cases — the emitted string starts with `Box::new(` (only a
            // bare fn-name value does, in subset — `emit_named_fn_value`), OR the
            // value is a fn-typed local ident. Resolve both at lowering.
            let already_boxed = ast_arg_is_named_fn_value(&a.expr, cx, env)
                || matches!(
                    &a.expr,
                    Expr::Ident(name, _)
                        if env.ty_of(name).is_some_and(|t| matches!(t, Type::Fn { .. }))
                );
            let (_, ty) = conv.as_ref().expect("matched Some above");
            Some(TFnCoerce {
                fn_type_rust: cx.rust_type(ty),
                already_boxed,
            })
        }
        _ => None,
    };
    // D-FIXARR1: when a [T#N] (Rust [T; N]) is passed where a [T] (Vec<T>) is expected,
    // widen by copying into a growable list (`.to_vec()`).
    let widen_to_vec = matches!(
        (&value.ty, conv.as_ref().map(|(_, t)| t)),
        (Type::FixedList { elem: arg_elem, .. }, Some(Type::List(param_elem)))
            if arg_elem == param_elem
    );
    // Borrow wrappers (applied after the clone + fn-coerce wrappers). A `Read`
    // non-scalar is `&(…)`; a `Mutate` is `&mut (…)`.
    // When widening to Vec, the borrow wrapper applies to the widened Vec (not the array).
    let (borrow, mut_borrow) = match &conv {
        Some((AccessConvention::Read, t))
            if !t.is_scalar()
                && !(a.flags.c_callback_symbol && matches!(t, Type::Fn { .. })) =>
        {
            (true, false)
        }
        Some((AccessConvention::Write, _)) => (false, true),
        _ => (false, false),
    };
    TCallArg {
        value,
        borrow,
        mut_borrow,
        clone,
        arc_clone,
        fn_coerce,
        widen_to_vec,
    }
}

/// c109 Phase 14: lower a cross-module call's arguments against the callee's import
/// signature, reproducing `emit_call_args`. Each arg's borrow/clone/fn-coercion is
/// resolved from the sig param convention (the same `lower_one_call_arg` used by the
/// plain-call path).
pub(crate) fn lower_module_args(
    args: &[crate::AST::CallArg],
    sig: Option<&[(AccessConvention, Type)]>,
    env: &mut LowerEnv,
    cx: &Cx,
) -> Vec<TCallArg> {
    args.iter()
        .enumerate()
        .map(|(i, a)| {
            let conv = sig.and_then(|ps| ps.get(i)).map(|(c, t)| (*c, t.clone()));
            lower_one_call_arg(a, conv, env, cx)
        })
        .collect()
}

/// c109 Phase 14: lower one FFI extern-call argument, reproducing
/// `emit_extern_call_args` (Source/Codegen/Expression.rs). The value is wrapped in
/// `(…).clone()` when the arg carries `implicit_clone`, OR when its param is a
/// non-scalar `Read`-convention type and `implicit_clone` is NOT already set (the AST
/// `if a.flags.implicit_clone { … } else if … } if let Some((_, ty)) = sig … if
/// !ty.is_scalar() && !implicit_clone`). The Arc (`shared_auto_clone`) form is excluded
/// from the subset, so it never reaches here.
pub(crate) fn lower_extern_call_arg(
    a: &crate::AST::CallArg,
    conv: Option<(AccessConvention, Type)>,
    env: &mut LowerEnv,
    cx: &Cx,
) -> TExternArg {
    let value = lower_expr(&a.expr, cx, env);
    let non_scalar_param = conv
        .as_ref()
        .map(|(_, ty)| !ty.is_scalar())
        .unwrap_or(false);
    // `(…).clone()` is emitted once: either the explicit implicit_clone flag, or the
    // non-scalar-param clone (when implicit_clone is false). The two never stack — the
    // AST applies the param clone only `&& !a.flags.implicit_clone`.
    let clone = a.flags.implicit_clone || (non_scalar_param && !a.flags.implicit_clone);
    TExternArg { value, clone }
}

/// c109 Phase 13: does this AST arg expression emit as a `Box::new(…)` (a bare
/// fn-name value via `emit_named_fn_value`)? That is exactly an `Expr::Ident` which
/// is NOT a local and resolves to a `Type::Fn` in `cx.fn_types` (a top-level fn used
/// as a value). Mirrors `emit_expr`'s `Expr::Ident` arm + `emit_call_args`'
/// `s.starts_with("Box::new(")` check, resolved at lowering.
pub(crate) fn ast_arg_is_named_fn_value(e: &Expr, cx: &Cx, env: &LowerEnv) -> bool {
    if let Expr::Ident(name, _) = e {
        if !env.locals.contains_key(name) && !cx.consts.contains_key(name) {
            return matches!(cx.fn_types.get(name), Some(Type::Fn { .. }));
        }
    }
    false
}

/// c109 Phase 9: reproduce codegen's `expr_jet_ty(receiver, env)`
/// (Source/Codegen/Expression.rs) for a built-in method receiver, using the TIR
/// lowering env's slot types. This MUST match `expr_jet_ty` bit-for-bit (incl. its
/// `None` results) because the Map-vs-List-vs-String emit branch in
/// `emit_builtin_method` is keyed on it: a divergence here flips a branch and breaks
/// byte-parity. Only `Ident` (via its slot type), `Str`/`Char`, and chained
/// `chars`/`split`/other method calls resolve; everything else (notably a struct
/// `Field` read) is `None` — exactly as `expr_jet_ty` does, so a `None`-typed
/// receiver lands on the AST's default branch (the list/else arm).
pub(crate) fn tir_recv_jet_ty(e: &Expr, env: &LowerEnv) -> Option<Type> {
    match e {
        Expr::Ident(name, _) => env.ty_of(name),
        Expr::Str(_, _) => Some(Type::String),
        Expr::Char(_, _) => Some(Type::Char),
        Expr::TupleLit(_, _, Some(ty)) => Some(ty.clone()),
        Expr::MethodCall {
            receiver,
            method,
            resolved_ret,
            ..
        } => {
            if matches!(method.as_str(), "zip" | "map") {
                if let Some(ty) = resolved_ret {
                    return Some(ty.clone());
                }
            }
            if method == "chars" {
                return Some(Type::List(Box::new(Type::Char)));
            }
            if method == "split" {
                return Some(Type::List(Box::new(Type::String)));
            }
            // D-DYNARRAY1: `xs.view(a..b).fold(...)` chained with no intermediate
            // binding — resolve the constructed `View<T>`'s element type from the
            // list receiver so the chained call still dispatches correctly.
            if method == crate::Syntax::METHOD_VIEW {
                if let Some(list_ty) = tir_recv_jet_ty(receiver, env) {
                    let elem = match list_ty {
                        Type::List(e) => Some(*e),
                        Type::FixedList { elem, .. } => Some(*elem),
                        _ => None,
                    };
                    if let Some(elem) = elem {
                        return Some(Type::Apply {
                            name: "View".to_string(),
                            args: vec![elem],
                        });
                    }
                }
                return None;
            }
            tir_recv_jet_ty(receiver, env)
        }
        _ => None,
    }
}
