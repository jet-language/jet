use crate::AST::{AccessConvention, Expr, Lambda, LambdaBody, Stmt, Type};
use crate::Codegen::Cx;
use crate::Codegen::TIR::clone_env;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower_lambda_expecting;
use crate::Codegen::TIR::TCallArg;
use crate::Codegen::TIR::TEnumArg;
use crate::Codegen::TIR::TEnumPayload;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TExternArg;
use crate::Codegen::TIR::TFnCoerce;
use crate::Codegen::TIR::TLocal;
use crate::Codegen::TIR::unit_type;

/// D-UNIONTYPE1=A: wrap a member value into the compiler-generated union enum.
pub(crate) fn maybe_widen_expr_to_union(value: TExpr, want: &Type) -> TExpr {
    match want {
        Type::Union(members)
            if members.iter().any(|m| m == &value.ty) && !matches!(&value.ty, Type::Union(_)) =>
        {
            let enum_type = crate::AST::union_enum_name(members);
            let variant = crate::AST::union_member_tag(&value.ty);
            TExpr {
                ty: want.clone(),
                kind: TExprKind::EnumLit {
                    enum_type,
                    variant,
                    payload: TEnumPayload::Positional(vec![TEnumArg {
                        value,
                        clone: false,
                        boxed: false,
                    }]),
                },
            }
        }
        _ => value,
    }
}

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

fn callback_fn_type(ty: &Type) -> Option<&Type> {
    match ty {
        Type::Fn { .. } => Some(ty),
        Type::Tagged { marker, inner }
            if matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::CppCallbackAbi))
                && matches!(inner.as_ref(), Type::Fn { .. }) =>
        {
            Some(inner)
        }
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
            lam_env
                .locals
                .insert(p.name.clone(), (TLocal::user(&p.name), ty));
        }
        lam_env
    }
    // Type probing lowers the expression to recover its total type, but the
    // probe is not the executable TIR pass. Do not publish spawn callbacks it
    // discovers into the shared JIT lambda table; the real lowering pass that
    // follows owns those entries and their site indexes.
    let saved_spawn_lambdas = std::mem::take(&mut *cx.jit_spawn_lambdas.borrow_mut());
    let body_ty = match &lam.body {
        LambdaBody::Expr(e) => {
            let mut lam_env = bind_params(lam, env, expected_params);
            lower_expr(e, cx, &mut lam_env).ty
        }
        LambdaBody::Block(stmts) => {
            if let Some((_, tail)) = lambda_block_tail(stmts) {
                let mut lam_env = bind_params(lam, env, expected_params);
                match tail {
                    Stmt::Return(Some(e), _) | Stmt::Expr(e) => {
                        lower_expr(e, cx, &mut lam_env).ty
                    }
                    _ => unit_type(),
                }
            } else {
                unit_type()
            }
        }
    };
    *cx.jit_spawn_lambdas.borrow_mut() = saved_spawn_lambdas;
    body_ty
}

/// c109 Phase 6/13: lower method-call arguments. The clone/Arc, borrow, and mut-borrow
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

/// c109 Phase 13: lower ONE call argument — the single source of truth for
/// the clone/Arc, Fn-coercion, and borrow/mut-borrow wrapper order. `conv` is the
/// resolved param `(convention, type)` for this position (`None` when the callee has
/// no known signature, e.g. a `CallValue`). The emit order is exactly the AST path's:
///   1. the implicit-clone / Arc-clone wrapper (`(…).clone()` / `Arc::clone(&…)`);
///   2. the Fn-typed coercion (`Rc`/`Arc`/`Box::new(…) as <fn-type>`, or just
///      ` as <fn-type>` when already wrapped);
///   3. the borrow wrapper (`&(…)` for a `Read` non-scalar non-Fn, `&mut (…)` for a
///      `Mutate`).
pub(crate) fn lower_call_arg_value(
    a: &crate::AST::CallArg,
    conv: Option<(AccessConvention, Type)>,
    env: &mut LowerEnv,
    cx: &Cx,
) -> TExpr {
    let saved_binder_refs = env.binder_refs.clone();
    let site = a.flags.binder_site.unwrap_or(a.span.start as u32);
    for (name, slot, ty) in &a.flags.binder_refs {
        env.binder_refs.insert(
            name.clone(),
            (format!("__jet_arg{site}_{slot}"), ty.clone()),
        );
    }
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
        (Expr::Ident(name, _), Some((_, ty)))
            if a.flags.c_callback_symbol && callback_fn_type(ty).is_some() => TExpr {
            ty: conv.as_ref().map(|(_, t)| t.clone()).unwrap(),
            kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::FnName(name.clone()))),
        },
        (Expr::Lambda(lam), Some((_, ty)))
            if a.flags.c_callback_symbol && callback_fn_type(ty).is_some() =>
        {
            let Type::Fn { params, ret, .. } = callback_fn_type(ty).unwrap() else {
                unreachable!()
            };
            let tl = lower_lambda_expecting(lam, cx, env, Some(params.as_slice()));
            let name = format!("__jet_c_callback_{}_{}", lam.span.start, lam.span.end);
            TExpr {
                ty: conv.as_ref().map(|(_, t)| t.clone()).unwrap(),
                kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::CCallback {
                    symbol: name,
                    lambda: tl,
                    ret: ret.as_deref().cloned(),
                })),
            }
        }
        (Expr::Lambda(lam), Some((_, ty @ Type::Fn { params, .. }))) => {
            let tl = lower_lambda_expecting(lam, cx, env, Some(params.as_slice()));
            TExpr {
                ty: ty.clone(),
                kind: TExprKind::Lambda(Box::new(tl)),
            }
        }
        _ => lower_expr(&a.expr, cx, env),
    };
    env.binder_refs = saved_binder_refs;
    value
}

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
    let value = super::take_scheduled_expr(&a.expr)
        .unwrap_or_else(|| lower_call_arg_value(a, conv.clone(), env, cx));
    // D-SG9: call-site `[U8].{…}` / contextual list args need IntN suffixes.
    let value = match (&conv, value) {
        (Some((_, want @ (Type::List(_) | Type::FixedList { .. }))), v) => {
            super::preserve_typed_list_shape(v, want, cx)
        }
        (_, v) => v,
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
        Some((_, ty)) if a.flags.c_callback_symbol && callback_fn_type(ty).is_some() => None,
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
                ty: ty.clone(),
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
    // D-UNIONTYPE1=A: member → union inject at the call boundary.
    let widen_to_union = match (&value.ty, conv.as_ref().map(|(_, t)| t)) {
        (got, Some(want @ Type::Union(members))) if members.iter().any(|m| m == got) => {
            Some(want.clone())
        }
        _ => None,
    };
    // Borrow wrappers (applied after the clone + fn-coerce wrappers). A `Read`
    // non-scalar is `&(…)`; a `Mutate` is `&mut (…)`.
    // When widening to Vec, the borrow wrapper applies to the widened Vec (not the array).
    let (borrow, mut_borrow) = match &conv {
        Some((AccessConvention::Read, t))
            if !t.is_scalar()
                && !(a.flags.c_callback_symbol && callback_fn_type(t).is_some()) =>
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
        widen_to_union,
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

/// c109 Phase 14: lower one FFI extern-call argument. The value is wrapped in
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
    // D-CABI-CALLBACK1: the C bridge is still an extern call, but its callback
    // argument needs the same stable function item / lambda wrapper as a direct
    // C call. Preserve sema's fact instead of re-boxing it as `dyn Fn`.
    let c_callback = conv.as_ref().is_some_and(|(_, ty)| {
        a.flags.c_callback_symbol && callback_fn_type(ty).is_some()
    });
    let value = if c_callback {
        lower_one_call_arg(a, conv.clone(), env, cx).value
    } else {
        lower_expr(&a.expr, cx, env)
    };
    let non_scalar_param = conv
        .as_ref()
        .map(|(_, ty)| !ty.is_scalar() && !c_callback)
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

/// c109 Phase 9: reproduce codegen's `expr_jet_ty(receiver, env)` for a built-in
/// method receiver, using the TIR
/// lowering env's slot types. This MUST match `expr_jet_ty` bit-for-bit (incl. its
/// `None` results) because the Map-vs-List-vs-String emit branch in
/// `emit_builtin_method` is keyed on it: a divergence here flips a branch and breaks
/// byte-parity. Only `Ident` (via its slot type), `Str`/`Char`, and chained
/// `chars`/`split`/other method calls resolve; everything else (notably a struct
/// `Field` read) is `None` — exactly as `expr_jet_ty` does, so a `None`-typed
/// receiver lands on the AST's default branch (the list/else arm).
pub(crate) fn tir_recv_jet_ty(e: &Expr, env: &LowerEnv) -> Option<Type> {
    fn dispatch_ty(ty: Type) -> Type {
        // D-TAINT1/D-TAG-SURFACE1: most `Type::Tagged` markers (the terminal
        // capability report's own D-PROCESS-SESSION2 tag, `#Input`/other user
        // taint facts, …) are compile-time-only dataflow facts with no runtime
        // representation — the same erasure `Expr::Tainted` gets in
        // `expr_in_subset` (TIR/subset/expressions.rs). Builtin-method
        // dispatch here must see the real underlying type (String/List/Map/…)
        // or a tagged receiver's `.split()`/`.get()`/etc. misses the curated
        // `TBuiltinOp` fast path and falls back to a generic call the JIT
        // declines to native-compile.
        //
        // The compiler-owned exceptions mirror sema's OWN rule at
        // `infer_method_call` (Sema/CheckerInfer/calls/method_calls.rs,
        // "Most fact tags are type-transparent"): SharedGuard read/edit and
        // the crypto nominal tag carry method POLICY, not just a dataflow
        // fact — `SharedGuard.wait()` dispatches through the tagged type's
        // own handle-method table, not a generic `TypeName::method` lookup.
        // Stripping those here misroutes the call and regresses an
        // already-JIT-covered stem (memory/shared_guard_queue).
        match ty {
            Type::Tagged { marker, inner }
                if matches!(
                    marker,
                    crate::AST::TagMarker::Internal(
                        crate::AST::InternalTag::SharedGuardRead
                            | crate::AST::InternalTag::SharedGuardEdit
                            | crate::AST::InternalTag::CoreCryptoNominal
                    )
                ) =>
            {
                Type::Tagged { marker, inner }
            }
            Type::Tagged { inner, .. } => dispatch_ty(*inner),
            other => other,
        }
    }

    fn literal_ty(expr: &Expr) -> Option<Type> {
        match expr {
            Expr::Int(..) => Some(Type::Int),
            Expr::Float(_, _, _) => Some(Type::Float),
            Expr::Bool(_, _) => Some(Type::Bool),
            Expr::Char(_, _) => Some(Type::Char),
            Expr::Str(_, _) => Some(Type::String),
            _ => None,
        }
    }

    fn set_constructor_elem(expr: &Expr, env: &LowerEnv) -> Option<Type> {
        match expr {
            Expr::ListLit(items, _) => items.first().and_then(literal_ty),
            Expr::Ident(name, _) => match env.ty_of(name) {
                Some(Type::List(elem)) | Some(Type::FixedList { elem, .. }) => Some(*elem),
                _ => None,
            },
            _ => None,
        }
    }

    match e {
        Expr::Ident(name, _) => env.ty_of(name).map(dispatch_ty),
        Expr::Str(_, _) => Some(Type::String),
        Expr::Char(_, _) => Some(Type::Char),
        Expr::TupleLit(_, _, Some(ty)) => Some(dispatch_ty(ty.clone())),
        Expr::MethodCall {
            receiver,
            method,
            args,
            resolved_ret,
            ..
        } => {
            // `resolved_ret` exists only when sema persisted a result more exact
            // than the generic method table (or another required exact shape).
            if let Some(ty) = resolved_ret {
                return Some(dispatch_ty(ty.clone()));
            }
            if let Expr::Ident(name, _) = receiver.as_ref() {
                if !env.locals.contains_key(name)
                    && method == "from"
                    && matches!(
                        name.as_str(),
                        crate::Syntax::TYPE_SET | crate::Syntax::TYPE_SORTED_SET
                    )
                    && args.len() == 1
                {
                    let elem = set_constructor_elem(&args[0].expr, env).unwrap_or(Type::Int);
                    return Some(Type::Apply {
                        name: name.clone(),
                        args: vec![elem],
                    });
                }
                // ByteBuffer static constructors used as chain receivers
                // (`ByteBuffer.from([…]).to_lower()`).
                if !env.locals.contains_key(name)
                    && name == crate::Syntax::TYPE_BYTE_BUFFER
                    && matches!(
                        (method.as_str(), args.len()),
                        ("new", 0) | ("from", 1) | ("with_capacity", 1)
                    )
                {
                    return Some(Type::Named(crate::Syntax::TYPE_BYTE_BUFFER.to_string()));
                }
            }
            // D-ITERTOOLS1=A: chained adapters (`nums.take(3).to_list()`) must
            // resolve as `Iter`, not fall through to the list receiver — otherwise
            // `to_list` lowers as SetToList and rustc sees `.iter()` on JetIter.
            if let Some(recv_ty) = tir_recv_jet_ty(receiver, env) {
                if let Some(Some(ret)) =
                    crate::Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                {
                    return Some(dispatch_ty(ret));
                }
            }
            if method == "chars" {
                return Some(Type::List(Box::new(Type::Char)));
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
