use crate::jet_generated_format as jet_format;
use crate::AST::{AccessConvention, Expr, StrPart, Type};
use crate::Codegen::alloc_handle_rust_type;
use crate::Codegen::Cx;
use crate::Codegen::is_db_value_type_name;
use crate::Codegen::is_db_value_variant;
use crate::Codegen::is_json_type_name;
use crate::Codegen::is_json_variant;
use crate::Codegen::is_key_variant;
use crate::Codegen::mangle;
use crate::Codegen::TIR::alloc_new_type;
use crate::Codegen::TIR::builtin_result_ty;
use crate::Codegen::TIR::call_return_type_with_args;
use crate::Codegen::TIR::core_call_return_ty;
use crate::Codegen::TIR::core_enum_equal_type;
use crate::Codegen::TIR::duration_new_unit;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::fn_field_call_ty;
use crate::Codegen::TIR::game_static_type;
use crate::Codegen::TIR::handle_method_op;
use crate::Codegen::TIR::handle_method_return_ty;
use crate::Codegen::TIR::is_civil_time_method_name;
use crate::Codegen::TIR::is_concurrency_method_name;
use crate::Codegen::TIR::is_devserver_method_name;
use crate::Codegen::TIR::is_app_method_name;
use crate::Codegen::TIR::lower_extern_call_arg;
use crate::Codegen::TIR::is_event_handle_type;
use crate::Codegen::TIR::is_event_method_name;
use crate::Codegen::TIR::is_http_method_name;
use crate::Codegen::TIR::is_http_type;
use crate::Codegen::TIR::is_loadable_method_name;
use crate::Codegen::TIR::is_measurement_method_name;
use crate::Codegen::TIR::is_reactive_method_name;
use crate::Codegen::TIR::is_reactive_effect_method_name;
use crate::Codegen::TIR::is_sketch_method_name;
use crate::Codegen::TIR::is_sketch_type;
use crate::Codegen::TIR::is_ui_backend_method_name;

fn progress_return_ty(args: &[TExpr]) -> Type {
    if matches!(args.first().map(|arg| &arg.ty), Some(Type::String)) {
        return Type::Result {
            ok: Box::new(unit_type()),
            err: Box::new(Type::Named(crate::Syntax::TYPE_IO_ERROR.to_string())),
        };
    }
    match args.first().map(|arg| &arg.ty) {
        Some(Type::List(elem) | Type::FixedList { elem, .. }) => crate::Collections::iter_ty((**elem).clone()),
        Some(Type::Apply { name, args })
            if name == crate::Syntax::TYPE_ITER && args.len() == 1 => {
                crate::Collections::iter_ty(args[0].clone())
            }
        _ => unit_type(),
    }
}
use crate::Codegen::TIR::is_watch_handle_type;
use crate::Codegen::TIR::is_watch_method_name;
use crate::Codegen::TIR::lambda_body_ty;
use crate::Codegen::TIR::lambda_body_ty_expecting;
use crate::Codegen::TIR::lower_core_closure_call;
use crate::Codegen::TIR::lower::core_module_path_from_receiver;
use crate::Codegen::TIR::lower_enum_arg;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::lower_expr_as_mut_place;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower_owned_expr;
use crate::Codegen::TIR::lower_lambda;
use crate::Codegen::TIR::lower_lambda_expecting;
use crate::Codegen::TIR::lower_lambda_expecting_host_borrow;
use crate::Codegen::TIR::lower_lambda_expecting_value;
use crate::Codegen::TIR::lower::lower_cursor_take_pattern;
use crate::Codegen::TIR::lower::lower_reader_take_pattern;
use crate::Codegen::TIR::lower_method_args;
use crate::Codegen::TIR::lower_module_args;
use crate::Codegen::TIR::lower_one_call_arg;
use crate::Codegen::TIR::lower_fn_value_call;
use crate::Codegen::TIR::fixed_list_elem_compatible;
use crate::Codegen::TIR::lower_spawn_lambda_for_jit;
use crate::Codegen::TIR::lower_spawn_lambda_for_jit_expecting;
use crate::Codegen::TIR::lower::static_call_type_name_lower;
use crate::Codegen::TIR::pool_field_ty_hint;
use crate::Codegen::TIR::render_router_handler;
use crate::Codegen::TIR::render_spawn_lambda;
use crate::Codegen::TIR::preserve_source_arg_order;
use crate::Codegen::TIR::resolve_builtin_op;
use crate::Codegen::TIR::resolve_closure_op;
use crate::Codegen::TIR::resolve_numeric_conversion_op;
use crate::Codegen::TIR::resolve_numeric_op;
use crate::Codegen::TIR::resolve_self_ty;
use crate::Codegen::TIR::solve_new_type;
use crate::Codegen::TIR::source_arg_order;
use crate::Codegen::TIR::wrap_foreign_undo;
use crate::Codegen::TIR::TBuiltinOp;
use crate::Codegen::TIR::TCallArg;
use crate::Codegen::TIR::TCoreClosureKind;
use crate::Codegen::TIR::TEnumArg;
use crate::Codegen::TIR::TEnumPayload;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::THandleOp;
use crate::Codegen::TIR::THostCall;
use crate::Codegen::TIR::TLocal;
use crate::Codegen::TIR::TMethodRef;
use crate::Codegen::TIR::TNumericOp;
use crate::Codegen::TIR::TPreludeArg;
use crate::Codegen::TIR::TStaticOwner;
use crate::Codegen::TIR::tir_recv_jet_ty;
use crate::Codegen::TIR::tls_static_op;
use crate::Codegen::TIR::http_client_static_op;
use crate::Codegen::TIR::TModuleCallForm;
use crate::Codegen::TIR::unit_type;
use crate::Diagnostics::Span;
use crate::Syntax;
use std::collections::HashSet;

fn first_string_literal_arg(args: &[crate::AST::CallArg]) -> Option<String> {
    let first = args.first()?;
    let Expr::Str(parts, _) = &first.expr else {
        return None;
    };
    match parts.as_slice() {
        [StrPart::Lit(value)] => Some(value.clone()),
        _ => None,
    }
}

pub(crate) fn is_checked_text_head_static(
    receiver: &Expr,
    resolved_ret: Option<&Type>,
    cx: &Cx,
) -> bool {
    let Some(Type::Apply { name, args }) = resolved_ret else {
        return false;
    };
    if name != Syntax::TYPE_CHECKED_TEXT {
        return false;
    }
    let Some(Type::Named(head)) = args.first() else {
        return false;
    };
    let source_name = match receiver {
        Expr::Ident(name, _) => name.clone(),
        Expr::Field(base, member, _) => {
            let Expr::Ident(prefix, _) = base.as_ref() else {
                return false;
            };
            format!("{prefix}.{member}")
        }
        _ => return false,
    };
    let canonical = match receiver {
        Expr::Ident(name, _) => cx.foreign_type_identity("", name),
        Expr::Field(base, member, _) => {
            let Expr::Ident(prefix, _) = base.as_ref() else {
                return false;
            };
            cx.foreign_type_identity(prefix, member)
        }
        _ => None,
    };
    source_name == *head
        || canonical.as_deref() == Some(head.as_str())
        || cx
            .reflect_paths
            .get(&source_name)
            .is_some_and(|path| path == head)
}

fn compute_transform_wrt(expr: &Expr) -> Option<Vec<String>> {
    match expr {
        Expr::ListLit(items, _) => items
            .iter()
            .map(|item| match item {
                Expr::Ident(name, _) => Some(name.clone()),
                Expr::Paren(inner, _) => match inner.as_ref() {
                    Expr::Ident(name, _) => Some(name.clone()),
                    _ => None,
                },
                _ => None,
            })
            .collect(),
        Expr::Paren(inner, _) => compute_transform_wrt(inner),
        _ => None,
    }
}

fn compute_transform_parameter_names(expr: &Expr, cx: &Cx) -> Option<Vec<String>> {
    match expr {
        Expr::Paren(inner, _) => compute_transform_parameter_names(inner, cx),
        Expr::Ident(name, _) => cx.fn_param_names.get(name).cloned(),
        Expr::Lambda(lambda) => Some(
            lambda
                .params
                .iter()
                .map(|param| param.name.clone())
                .collect(),
        ),
        Expr::MethodCall { method, args, .. }
            if matches!(method.as_str(), "gradient" | "value_and_gradient" | "vjp" | "jvp") =>
        {
            args.first()
                .and_then(|arg| compute_transform_parameter_names(&arg.expr, cx))
        }
        Expr::Call(call)
            if matches!(call.name.as_str(), "gradient" | "value_and_gradient" | "vjp" | "jvp") =>
        {
            call.args
                .first()
                .and_then(|arg| compute_transform_parameter_names(&arg.expr, cx))
        }
        _ => None,
    }
}

fn lower_compute_transform_call(
    module: &str,
    method: &str,
    method_span: Span,
    args: &[crate::AST::CallArg],
    resolved_ret: Option<&Type>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> Option<TExpr> {
    if module != "core.compute"
        || !matches!(method, "gradient" | "value_and_gradient" | "vjp" | "jvp")
    {
        return None;
    }
    let function = args.first()?;
    let lowered_function = lower_expr(&function.expr, cx, env);
    let function_ty = lowered_function.ty.clone();
    let mut lowered_args = vec![lowered_function];
    let mut value_args = Vec::new();
    let mut wrt = None;
    for arg in args.iter().skip(1) {
        if arg
            .label
            .as_ref()
            .is_some_and(|(label, _)| label == "wrt")
        {
            wrt = compute_transform_wrt(&arg.expr);
        } else {
            value_args.push(arg);
            lowered_args.push(lower_expr(&arg.expr, cx, env));
        }
    }
    let Some(parameter_names) = compute_transform_parameter_names(&function.expr, cx)
        .or_else(|| match &function_ty {
            Type::Fn {
                param_contract: Some(contract),
                ..
            } => Some(contract.iter().map(|(name, _)| name.clone()).collect()),
            _ => None,
        })
    else {
        return None;
    };
    let primal_count = if method == "jvp" {
        value_args.len() / 2
    } else {
        value_args.len()
    };
    let target_count = if value_args.is_empty() {
        parameter_names.len()
    } else {
        primal_count
    };
    let targets = wrt
        .map(|names| {
            names
                .into_iter()
                .filter_map(|name| parameter_names.iter().position(|param| param == &name))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|| (0..target_count).collect());
    lowered_args.push(TExpr {
        ty: Type::List(Box::new(Type::Int)),
        kind: TExprKind::ListLit(
            targets
                .into_iter()
                .map(|index| TExpr {
                    ty: Type::Int,
                    kind: TExprKind::IntLit(index as i64, None),
                })
                .collect(),
        ),
    });
    let ty = resolved_ret
        .cloned()
        .unwrap_or_else(|| core_call_return_ty(module, method));
    Some(TExpr {
        ty,
        kind: TExprKind::CoreCall {
            module: module.to_string(),
            method: method.to_string(),
            widen_to_vec: vec![false; lowered_args.len()],
            args: lowered_args,
            source_span: method_span,
        },
    })
}

/// Route the public archive surface through the loaded source package. The
/// package module itself is the only caller that may lower the internal ABI
/// calls below; all other callers use the ordinary file-module TIR path.
fn lower_archive_source_call(
    method: &str,
    type_args: &[Type],
    resolved_ret: Option<&Type>,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    env: &mut LowerEnv,
) -> Option<TExpr> {
    // Comptime evaluates a standalone fragment, not the emitted module graph.
    // Keep its existing Core evaluator path; runtime bundles use the source
    // module above and therefore remain subject to the normal frontend/TIR
    // route.
    if !cx.core_archive_source || cx.module_alias == "core_archive" || super::is_eval_fragment() {
        return None;
    }
    let (sig, fixed_ret) = crate::Sema::core_fixed_sig("core.archive", method)?;
    let targs = lower_module_args(args, Some(sig.as_slice()), env, cx);
    Some(TExpr {
        ty: resolved_ret
            .cloned()
            .or(fixed_ret)
            .unwrap_or_else(unit_type),
        kind: TExprKind::ModuleCall {
            form: TModuleCallForm::Qualified {
                // The source package is emitted as `mod __jet_core_archive`.
                // `mangle_generated` would add a second generated prefix and
                // produce the nonexistent `__jet___core_archive` path.
                rust_mod: crate::Codegen::mangle("core_archive"),
                rust_fn: mangle(method).to_string(),
            },
            type_args: type_args.to_vec(),
            args: targs,
        },
    })
}

/// A prelude/host static-call owner whose path is prefixed by the generated
/// crate root (`{root}jet_std::…`).
fn reduce_op_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::ReduceMarker(name, _) => Some(name.clone()),
        Expr::EnumLit {
            type_name,
            variant,
            args,
            ..
        } if (type_name.is_empty() || type_name == crate::Syntax::TYPE_REDUCE_OP)
            && args.is_empty() =>
        {
            Some(variant.clone())
        }
        _ => None,
    }
}

fn is_fragment_build_context(receiver: &Expr, cx: &Cx) -> bool {
    if !super::is_eval_fragment() {
        return false;
    }
    let Expr::Ident(name, _) = receiver else {
        return false;
    };
    matches!(
        cx.const_values.get(name),
        Some(crate::Comptime::CtValue::Struct { type_name, .. })
            if type_name == crate::Syntax::TYPE_BUILD_CONTEXT
    )
}

fn rooted_owner(path: impl Into<String>) -> TStaticOwner {
    TStaticOwner::Prelude {
        rooted: true,
        path: path.into(),
        generics: Vec::new(),
    }
}

/// The same, with resolved generic arguments the emitter spells.
fn rooted_generic_owner(path: impl Into<String>, generics: Vec<TPreludeArg>) -> TStaticOwner {
    TStaticOwner::Prelude {
        rooted: true,
        path: path.into(),
        generics,
    }
}

/// A host static-call owner spelled without the crate root (a `std::` path or a
/// prelude type the generated crate has already imported).
fn host_owner(path: impl Into<String>) -> TStaticOwner {
    TStaticOwner::Prelude {
        rooted: false,
        path: path.into(),
        generics: Vec::new(),
    }
}

/// The same, with resolved generic arguments the emitter spells.
fn host_generic_owner(path: impl Into<String>, generics: Vec<TPreludeArg>) -> TStaticOwner {
    TStaticOwner::Prelude {
        rooted: false,
        path: path.into(),
        generics,
    }
}

fn builtin_arg_takes_ownership(op: &TBuiltinOp, index: usize) -> bool {
    match op {
        TBuiltinOp::Push
        | TBuiltinOp::TryPush
        | TBuiltinOp::TryStringPush
        | TBuiltinOp::Intersperse
        | TBuiltinOp::SetInsert
        | TBuiltinOp::SortedSetInsert
        | TBuiltinOp::BagAdd
        | TBuiltinOp::DequePushFront
        | TBuiltinOp::DequePushBack => index == 0,
        TBuiltinOp::InsertMap
        | TBuiltinOp::TryInsertMap
        | TBuiltinOp::AddNewMap
        | TBuiltinOp::InsertList => index == 1,
        TBuiltinOp::LruPut | TBuiltinOp::LruAddNew => index < 2,
        _ => false,
    }
}

fn core_widen_to_vec(module: &str, method: &str, args: &[TExpr]) -> Vec<bool> {
    if module == "core.io"
        && method == "progress"
        && matches!(args.first().map(|arg| &arg.ty), Some(Type::FixedList { .. }))
    {
        return std::iter::once(true)
            .chain(std::iter::repeat(false).take(args.len().saturating_sub(1)))
            .collect();
    }
    let params = crate::Sema::core_fixed_sig(module, method)
        .map(|(params, _)| params)
        .unwrap_or_default();
    args.iter()
        .enumerate()
        .map(|(index, arg)| {
            matches!(
                (&arg.ty, params.get(index).map(|(_, ty)| ty)),
                (Type::FixedList { elem: actual, .. }, Some(Type::List(want)))
                    if fixed_list_elem_compatible(actual, want)
            )
        })
        .collect()
}

fn crypto_helper_return_ty(helper: &str) -> Type {
    let u8_list = Type::List(Box::new(Type::IntN {
        signed: false,
        bits: 8,
    }));
    match helper {
        "__digest256_hex" | "__digest512_hex" | "__x25519_public_text" | "__password_text" | "__hasher_digest" => {
            Type::String
        }
        "__hasher_new" => Type::Named("Hasher".into()),
        "__signing_public" => Type::Named("VerifyKey".into()),
        "__x25519_public" => Type::Named("X25519PublicKey".into()),
        "__signing_generate" => Type::Result {
            ok: Box::new(Type::Named("SigningKey".into())),
            err: Box::new(Type::Named("CryptoError".into())),
        },
        "__x25519_generate" => Type::Result {
            ok: Box::new(Type::Named("X25519SecretKey".into())),
            err: Box::new(Type::Named("CryptoError".into())),
        },
        "__secret_from_text" | "__secret_from_bytes" => Type::Result {
            ok: Box::new(Type::Named("Secret".into())),
            err: Box::new(Type::Named("CryptoError".into())),
        },
        "__verify_key_from_bytes" => Type::Result {
            ok: Box::new(Type::Named("VerifyKey".into())),
            err: Box::new(Type::Named("CryptoError".into())),
        },
        "__x25519_public_from_bytes" | "__x25519_public_from_text" => Type::Result {
            ok: Box::new(Type::Named("X25519PublicKey".into())),
            err: Box::new(Type::Named("CryptoError".into())),
        },
        "__signature_from_bytes" => Type::Result {
            ok: Box::new(Type::Named("Signature".into())),
            err: Box::new(Type::Named("CryptoError".into())),
        },
        "__sealed_from_bytes" => Type::Result {
            ok: Box::new(Type::Named("Sealed".into())),
            err: Box::new(Type::Named("CryptoError".into())),
        },
        "__wrapped_from_bytes" => Type::Result {
            ok: Box::new(Type::Named("WrappedKey".into())),
            err: Box::new(Type::Named("CryptoError".into())),
        },
        "__vault_wrapped_from_bytes" => Type::Result {
            ok: Box::new(Type::Named("WrappedVaultKey".into())),
            err: Box::new(Type::Named("CryptoError".into())),
        },
        "__password_parse" => Type::Result {
            ok: Box::new(Type::Named("PasswordHash".into())),
            err: Box::new(Type::Named("CryptoError".into())),
        },
        "__vault_unlock_recipient" | "__vault_unlock_passphrase" => {
            Type::Named("KeyUnlock".into())
        }
        "__verify_key_bytes"
        | "__x25519_public_bytes"
        | "__signature_bytes"
        | "__sealed_bytes"
        | "__wrapped_bytes"
        | "__vault_wrapped_bytes"
        | "__digest256_bytes"
        | "__digest512_bytes" => u8_list,
        _ => unit_type(),
    }
}

fn crypto_instance_helper(kind: &str, method: &str) -> Option<&'static str> {
    match (kind, method) {
        ("SigningKey", "public_key") => Some("__signing_public"),
        ("X25519SecretKey", "public_key") => Some("__x25519_public"),
        ("VerifyKey", "bytes") => Some("__verify_key_bytes"),
        ("X25519PublicKey", "bytes") => Some("__x25519_public_bytes"),
        ("X25519PublicKey", "text") => Some("__x25519_public_text"),
        ("Signature", "bytes") => Some("__signature_bytes"),
        ("Sealed", "bytes") => Some("__sealed_bytes"),
        ("WrappedKey", "bytes") => Some("__wrapped_bytes"),
        ("WrappedVaultKey", "bytes") => Some("__vault_wrapped_bytes"),
        ("Digest256", "bytes") => Some("__digest256_bytes"),
        ("Digest512", "bytes") => Some("__digest512_bytes"),
        ("Digest256", "hex") => Some("__digest256_hex"),
        ("Digest512", "hex") => Some("__digest512_hex"),
        ("PasswordHash", "text") => Some("__password_text"),
        ("Hasher", "update") => Some("__hasher_update"),
        ("Hasher", "digest") => Some("__hasher_digest"),
        _ => None,
    }
}

/// Keep the generic `core.crypto` call off the large method-dispatch frame.
/// This is the same resolved CoreCall shape as the full dispatcher below; the
/// sema fixed-signature fact makes the narrow route total.
fn lower_core_crypto_alias_fast(
    receiver: &Expr,
    method: &str,
    method_span: Span,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    env: &mut LowerEnv,
) -> Option<TExpr> {
    let Expr::Ident(alias, _) = receiver else {
        return None;
    };
    if env.locals.contains_key(alias) {
        return None;
    }
    let target = cx
        .core_import_module_for_function(&env.fn_name, alias)
        .map(|module| (module.to_owned(), method.to_owned()))
        .or_else(|| {
            cx.inline_reexport_core
                .get(&(alias.clone(), method.to_owned()))
                .cloned()
        });
    let Some((module, core_method)) = target else {
        return None;
    };
    if module != "core.crypto"
        || crate::Sema::core_fixed_sig(&module, &core_method).is_none()
    {
        return None;
    }
    let targs: Vec<TExpr> = args
        .iter()
        .map(|arg| lower_expr(&arg.expr, cx, env))
        .collect();
    let widen_to_vec = core_widen_to_vec(&module, &core_method, &targs);
    let ty = core_call_return_ty(&module, &core_method);
    demand_generic_serde_codec(cx, &env.fn_name, &module, &core_method, &targs, &ty);
    Some(TExpr {
        ty,
        kind: TExprKind::CoreCall {
            module,
            method: core_method,
            args: targs,
            source_span: method_span,
            widen_to_vec,
        },
    })
}

/// Lower crypto nominal receiver methods without retaining the full dispatcher
/// frame while the receiver itself is lowered.
fn lower_crypto_instance_fast(
    receiver: &Expr,
    method: &str,
    method_span: Span,
    call_args: &[crate::AST::CallArg],
    recv_type: &Option<String>,
    resolved_ret: Option<&Type>,
    lowered_receiver: &mut Option<TExpr>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> Option<TExpr> {
    if static_call_type_name_lower(receiver, env).is_some() {
        return None;
    }
    if matches!(receiver, Expr::Ident(name, _) if env.is_gc(name)) {
        return None;
    }
    let kind = recv_type
        .as_deref()?
        .rsplit('.')
        .next()
        .unwrap_or_default();
    let helper = crypto_instance_helper(kind, method)?;
    let recv = lowered_receiver
        .take()
        .unwrap_or_else(|| lower_expr(receiver, cx, env));
    let mut args = vec![recv];
    if kind == "Hasher" && method == "update" {
        args.extend(call_args.iter().map(|arg| lower_expr(&arg.expr, cx, env)));
    }
    let widen_to_vec = core_widen_to_vec("core.crypto", helper, &args);
    let ty = resolved_ret
        .cloned()
        .unwrap_or_else(|| crypto_helper_return_ty(helper));
    Some(TExpr {
        ty,
        kind: TExprKind::CoreCall {
            module: "core.crypto".to_string(),
            method: helper.to_string(),
            args,
            source_span: method_span,
            widen_to_vec,
        },
    })
}

fn lower_serde_encode_node(recv: TExpr, cx: &Cx) -> TExpr {
    if matches!(&recv.ty, Type::Apply { .. }) {
        cx.jit_method_calls.borrow_mut().insert(
            format!("{}::encode", recv.ty.name()),
            (recv.ty.clone(), "encode".to_string(), Vec::new()),
        );
    }
    TExpr {
        ty: Type::Named(Syntax::TYPE_DATA.to_string()),
        kind: TExprKind::HandleMethod {
            recv: Box::new(recv),
            op: THandleOp::SerdeEncode,
            args: Vec::new(),
        },
    }
}

fn lower_datatree_decode_node(
    recv: TExpr,
    target: Type,
    resolved_ret: Option<&Type>,
    cx: &Cx,
) -> TExpr {
    if matches!(&target, Type::Apply { .. }) {
        cx.jit_method_calls.borrow_mut().insert(
            format!("{}::decode", target.name()),
            (target.clone(), "decode".to_string(), Vec::new()),
        );
    }
    TExpr {
        ty: resolved_ret.cloned().unwrap_or_else(|| Type::Result {
            ok: Box::new(target.clone()),
            err: Box::new(Type::List(Box::new(Type::Named("FieldError".to_string())))),
        }),
        kind: TExprKind::HandleMethod {
            recv: Box::new(recv),
            op: THandleOp::DataTreeDecode(target),
            args: Vec::new(),
        },
    }
}

fn fragment_serde_encode_type(ty: &Type, cx: &Cx) -> bool {
    matches!(
        ty,
        Type::Int
            | Type::IntN { .. }
            | Type::Float
            | Type::Float32
            | Type::Bool
            | Type::String
            | Type::Char
            | Type::List(_)
            | Type::FixedList { .. }
            | Type::Option(_)
            | Type::Map { .. }
    ) || matches!(ty, Type::Named(name) if is_json_type_name(name))
        || cx.sigs.contains_key(&format!("{}::encode", ty.name()))
        || matches!(ty, Type::Apply { name, .. }
            if cx.sigs.contains_key(&format!("{name}::encode")))
}

fn zip_family_mode(method: &str) -> crate::Codegen::TIR::TZipMode {
    match method {
        "zip_short" => crate::Codegen::TIR::TZipMode::Short,
        "zip_pad" => crate::Codegen::TIR::TZipMode::Pad,
        _ => crate::Codegen::TIR::TZipMode::Strict,
    }
}

fn zip_tuple_fields(ty: &Type) -> Option<Vec<(String, Type)>> {
    let inner = match ty {
        Type::List(inner) => inner.as_ref(),
        Type::Apply { name, args }
            if name == Syntax::TYPE_ITER && args.len() == 1 => &args[0],
        _ => return None,
    };
    match inner {
        Type::Tuple(fields) => Some(
            fields
                .iter()
                .map(|(name, ty)| (name.clone(), (**ty).clone()))
                .collect(),
        ),
        _ => None,
    }
}

fn zip_sequence_type(ty: &Type) -> bool {
    match ty {
        Type::Tagged { inner, .. } => zip_sequence_type(inner),
        Type::List(_) | Type::FixedList { .. } => true,
        Type::Apply { name, args }
            if name == Syntax::TYPE_ITER && args.len() == 1 => true,
        _ => false,
    }
}

fn zip_field_name(index: usize, label: Option<&str>) -> String {
    label.map_or_else(
        || {
            ["a", "b", "c", "d", "e", "f"]
                .get(index)
                .map_or_else(|| format!("column_{index}"), |name| (*name).to_string())
        },
        str::to_string,
    )
}

/// Build one typed zip-family TIR node. The emitter composes this node's
/// inputs through the shared binary Prelude primitives; the node itself still
/// carries the complete variadic contract.
pub(crate) fn lower_zip_family(
    receiver: TExpr,
    inputs: Vec<TExpr>,
    fills: Vec<TExpr>,
    fields: Vec<String>,
    method: &str,
    resolved_ret: Option<&Type>,
) -> TExpr {
    let input_count = inputs.len() + 1;
    let ret = resolved_ret
        .cloned()
        .unwrap_or_else(|| crate::Collections::iter_ty(Type::Int));
    if input_count == 1 {
        if crate::Collections::is_iter_type(&receiver.ty) {
            return receiver;
        }
        return TExpr {
            ty: ret,
            kind: TExprKind::BuiltinMethod {
                recv: Box::new(receiver),
                op: TBuiltinOp::ListLazy,
                args: Vec::new(),
            },
        };
    }
    let tuple_fields = resolved_ret.and_then(zip_tuple_fields);
    let field_types = tuple_fields
        .as_ref()
        .map(|fields| fields.iter().map(|(_, ty)| ty.clone()).collect())
        .unwrap_or_else(|| fields.iter().map(|_| Type::Int).collect());
    let fill_mode = if method != "zip_pad" {
        crate::Codegen::TIR::TZipFillMode::DefaultNone
    } else if fills.len() == 1 {
        if matches!(fills[0].ty, Type::Tuple(_)) {
            crate::Codegen::TIR::TZipFillMode::Columns
        } else {
            crate::Codegen::TIR::TZipFillMode::Common
        }
    } else {
        crate::Codegen::TIR::TZipFillMode::DefaultNone
    };
    TExpr {
        ty: ret,
        kind: TExprKind::BuiltinMethod {
            recv: Box::new(receiver),
            op: TBuiltinOp::Zip {
                tuple_struct: tuple_fields
                    .as_ref()
                    .map(|fields| crate::Codegen::Tuples::tuple_struct_name(&fields))
                    .unwrap_or_default(),
                mode: zip_family_mode(method),
                fields,
                flatten: false,
                input_count,
                fill_mode,
                field_types,
            },
            args: inputs.into_iter().chain(fills).collect(),
        },
    }
}

pub(crate) fn lower_empty_zip_family(resolved_ret: &Type, method: &str) -> TExpr {
    let tuple_fields = zip_tuple_fields(resolved_ret);
    let fields = tuple_fields
        .as_ref()
        .map(|fields| fields.iter().map(|(name, _)| name.clone()).collect())
        .unwrap_or_default();
    let field_types = tuple_fields
        .as_ref()
        .map(|fields| fields.iter().map(|(_, ty)| ty.clone()).collect())
        .unwrap_or_default();
    TExpr {
        ty: resolved_ret.clone(),
        kind: TExprKind::BuiltinMethod {
            recv: Box::new(TExpr {
                ty: unit_type(),
                kind: TExprKind::Unit,
            }),
            op: TBuiltinOp::Zip {
                tuple_struct: String::new(),
                mode: zip_family_mode(method),
                fields,
                flatten: false,
                input_count: 0,
                fill_mode: crate::Codegen::TIR::TZipFillMode::DefaultNone,
                field_types,
            },
            args: Vec::new(),
        },
    }
}

/// c109 Phase 6: lower a method call. The gate proved it is the synthetic `.clone()`
/// or a user instance method on a covered type; resolve every dispatch fact here.
pub(crate) fn lower_method_call(
    receiver: &Expr,
    method: &str,
    method_span: Span,
    owner_type_args: &[Type],
    type_args: &[Type],
    args: &[crate::AST::CallArg],
    recv_type: &Option<String>,
    resolved_ret: Option<&Type>,
    checked_widen: bool,
    cx: &Cx,
    env: &mut LowerEnv,
    lowered_receiver: Option<TExpr>,
) -> TExpr {
    lower_method_call_with_sig(
        receiver,
        method,
        method_span,
        owner_type_args,
        type_args,
        args,
        recv_type,
        resolved_ret,
        checked_widen,
        cx,
        env,
        lowered_receiver,
        None,
    )
}

pub(crate) fn lower_method_call_with_sig(
    receiver: &Expr,
    method: &str,
    method_span: Span,
    owner_type_args: &[Type],
    type_args: &[Type],
    args: &[crate::AST::CallArg],
    recv_type: &Option<String>,
    resolved_ret: Option<&Type>,
    checked_widen: bool,
    cx: &Cx,
    env: &mut LowerEnv,
    lowered_receiver: Option<TExpr>,
    instantiated_sig: Option<&[(AccessConvention, Type)]>,
) -> TExpr {
    // D-MEMO1=A: sema has already proved `name.cache()` is the memoized
    // function's statistics projection. Keep it as a named TIR call so AOT,
    // JIT, and interpreter adapters all enter the same Prelude-backed store.
    if method == Syntax::METHOD_MEMO_CACHE
        && args.is_empty()
        && matches!(resolved_ret, Some(Type::Named(name)) if name == Syntax::TYPE_MEMO_STATS)
    {
        if let Expr::Ident(name, _) = receiver {
            return TExpr {
                ty: Type::Named(Syntax::TYPE_MEMO_STATS.to_string()),
                kind: TExprKind::Call {
                    name: Syntax::memo_stats_call(name),
                    type_args: Vec::new(),
                    args: Vec::new(),
                },
            };
        }
    }
    if let Some(lowered) =
        lower_core_crypto_alias_fast(receiver, method, method_span, args, cx, env)
    {
        return lowered;
    }
    let mut lowered_receiver = lowered_receiver;
    if let Some(lowered) = lower_crypto_instance_fast(
        receiver,
        method,
        method_span,
        args,
        recv_type,
        resolved_ret,
        &mut lowered_receiver,
        cx,
        env,
    ) {
        return lowered;
    }
    lower_method_call_impl(
        receiver,
        method,
        method_span,
        owner_type_args,
        type_args,
        args,
        recv_type,
        resolved_ret,
        checked_widen,
        cx,
        env,
        lowered_receiver,
        instantiated_sig,
    )
}

fn lower_method_call_impl(
    receiver: &Expr,
    method: &str,
    method_span: Span,
    owner_type_args: &[Type],
    type_args: &[Type],
    args: &[crate::AST::CallArg],
    recv_type: &Option<String>,
    resolved_ret: Option<&Type>,
    checked_widen: bool,
    cx: &Cx,
    env: &mut LowerEnv,
    lowered_receiver: Option<TExpr>,
    instantiated_sig: Option<&[(AccessConvention, Type)]>,
) -> TExpr {
    // D-CALLVALUE1=B: sema proved this `.call(...)` receiver is a function
    // value. Lower it through the same TIR function-value node as `Expr::CallValue`.
    if recv_type.as_deref() == Some(Syntax::INTERNAL_CALL_VALUE) {
        let callee = lowered_receiver
            .unwrap_or_else(|| crate::Codegen::TIR::lower_expr(receiver, cx, env));
        return lower_fn_value_call(
            receiver,
            callee,
            args,
            receiver.span().start as u32,
            cx,
            env,
        );
    }
    // D-CALLDUAL1=E: sema has already selected one `#Root` function. Lower
    // the receiver as argument zero and keep the callee on the ordinary
    // direct/module-call TIR path so AOT, JIT, interpreter, and web share it.
    let root_call = recv_type.as_deref().and_then(|name| {
        if name == Syntax::INTERNAL_ROOT_CALL_LOCAL {
            Some((None, None, method.to_string()))
        } else if let Some(alias) = name.strip_prefix(Syntax::INTERNAL_ROOT_CALL_IMPORT_PREFIX) {
            Some((Some(alias.to_string()), None, method.to_string()))
        } else {
            name.strip_prefix(Syntax::INTERNAL_ROOT_CALL_CORE_PREFIX)
                .map(|module| (None, Some(module.to_string()), method.to_string()))
        }
    });
    if let Some((root_alias, root_core, root_name)) = root_call {
        if root_core.is_some() {
            // Core print is the first prelude `#Root` function. Preserve its
            // variadic newline semantics by lowering the receiver as argument
            // zero and joining it with the ordinary call arguments.
            let receiver = lowered_receiver
                .unwrap_or_else(|| crate::Codegen::TIR::lower_expr(receiver, cx, env));
            let joined = crate::Codegen::TIR::lower::join_print_values(
                std::iter::once(receiver).chain(
                    args.iter()
                        .map(|arg| crate::Codegen::TIR::lower_expr(&arg.expr, cx, env)),
                ),
                cx,
            );
            return TExpr {
                ty: unit_type(),
                kind: TExprKind::Print(Box::new(joined)),
            };
        }
        let sig = root_alias.as_ref().and_then(|alias| {
            cx.import_sigs
                .get(&(alias.clone(), root_name.clone()))
                .cloned()
        });
        let sig = sig.or_else(|| cx.sigs.get(&root_name).cloned());
        let receiver = lowered_receiver
            .unwrap_or_else(|| crate::Codegen::TIR::lower_expr(receiver, cx, env));
        let receiver_conv = sig.as_ref().and_then(|params| params.first()).cloned();
        let receiver_arg = TCallArg {
            borrow: receiver_conv.as_ref().is_some_and(|(conv, ty)| {
                *conv == AccessConvention::Read && !ty.is_scalar()
            }),
            mut_borrow: receiver_conv
                .as_ref()
                .is_some_and(|(conv, _)| *conv == AccessConvention::Write),
            value: receiver,
            template_items: None,
            clone: false,
            arc_clone: false,
            fn_coerce: None,
            widen_to_vec: false,
            widen_to_union: None,
        };
        let mut lowered_args = Vec::with_capacity(args.len() + 1);
        lowered_args.push(receiver_arg);
        lowered_args.extend(args.iter().enumerate().map(|(index, arg)| {
            let conv = sig
                .as_ref()
                .and_then(|params| params.get(index + 1))
                .cloned();
            lower_one_call_arg(arg, conv, env, cx)
        }));
        if let Some(alias) = root_alias {
            let ret = cx
                .import_rets
                .get(&(alias.clone(), root_name.clone()))
                .cloned()
                .flatten()
                .unwrap_or_else(unit_type);
            let (rust_mod, rust_fn) = cx
                .reexport_calls
                .get(&(alias.clone(), root_name.clone()))
                .cloned()
                .or_else(|| {
                    cx.import_mods
                        .get(&alias)
                        .cloned()
                        .map(|module| (module, root_name.clone()))
                })
                .expect("sema root import must have a codegen import target");
            return TExpr {
                ty: ret,
                kind: TExprKind::ModuleCall {
                    form: TModuleCallForm::Qualified {
                        rust_mod,
                        rust_fn: mangle(&rust_fn).to_string(),
                    },
                    type_args: type_args.to_vec(),
                    args: lowered_args,
                },
            };
        }
        let ret = call_return_type_with_args(cx, &root_name, type_args, &lowered_args);
        return TExpr {
            ty: ret,
            kind: TExprKind::Call {
                name: cx.jit_local_call_prefix.as_ref().map_or_else(
                    || root_name.clone(),
                    |prefix| format!("{prefix}{}", mangle(&root_name)),
                ),
                type_args: type_args.to_vec(),
                args: lowered_args,
            },
        };
    }
    // D-FAIL-CARRIER1=A: `.or_err("why")` lifts a clean absence into a failure.
    // One carrier, so the payload rides through untouched and only the report
    // changes. The prelude's `JetOptionalView::or_err` holds that one meaning;
    // this is a plain marshalling call onto it.
    if method == Syntax::METHOD_OUTCOME_OR_ERR
        && args.len() == 1
        && matches!(tir_recv_jet_ty(receiver, env), Some(Type::Option(_)))
    {
        let recv = lower_expr(receiver, cx, env);
        let why = lower_one_call_arg(&args[0], None, env, cx);
        return TExpr {
            ty: resolved_ret.cloned().unwrap_or_else(|| Type::Result {
                ok: Box::new(recv.ty.clone()),
                err: Box::new(Type::Named(Syntax::TYPE_ERR.to_string())),
            }),
            kind: TExprKind::MethodCall {
                recv: Box::new(recv),
                method: TMethodRef::bare(method),
                type_args: Vec::new(),
                args: vec![why],
                source_first_string_literal: first_string_literal_arg(args),
                operator_line: None,
            },
        };
    }
    // D-FAIL-CARRIER1=A: the carrier's middle states. `.partial` reads the
    // payload a failure kept and `.notes` reads what it had to say. Both live
    // on the outcome value; the prelude's `jet_partial`/`jet_notes` hold the
    // one meaning, and one node carries both to every engine.
    if recv_type.as_deref() == Some("__Carrier__") {
        let notes = method == Syntax::METHOD_OUTCOME_NOTES;
        let recv = lower_expr(receiver, cx, env);
        return TExpr {
            ty: resolved_ret
                .cloned()
                .unwrap_or_else(|| Type::List(Box::new(Type::String))),
            kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::CarrierFact {
                recv: Box::new(recv),
                field: if notes {
                    Syntax::FIELD_OUTCOME_NOTES.to_string()
                } else {
                    Syntax::FIELD_OUTCOME_PARTIAL.to_string()
                },
                notes,
            })),
        };
    }
    let guard_receiver = tir_recv_jet_ty(receiver, env).and_then(|ty| match ty {
        Type::Tagged { marker, inner } => match inner.as_ref() {
            Type::Apply { name, args }
                if name == Syntax::TYPE_SHARED_GUARD && args.len() == 1 =>
            {
                Some((
                    args[0].clone(),
                    matches!(marker, crate::AST::TagMarker::Internal(crate::AST::InternalTag::SharedGuardEdit)),
                ))
            }
            _ => None,
        },
        _ => None,
    });
    if let Some((inner, editable)) = guard_receiver {
        match (method, args) {
            ("map", [arg]) => {
                let Expr::Lambda(lambda) = &arg.expr else {
                    unreachable!("sema requires a SharedGuard.map projection lambda");
                };
                let path = lambda
                    .meta
                    .guard_projection
                    .clone()
                    .expect("sema supplies a SharedGuard.map projection");
                return TExpr {
                    ty: resolved_ret.cloned().unwrap_or_else(unit_type),
                    kind: TExprKind::SharedGuardMap {
                        guard: Box::new(lower_expr(receiver, cx, env)),
                        path,
                        editable,
                    },
                };
            }
            ("split", [first, second]) => {
                let (Expr::Lambda(first), Expr::Lambda(second)) =
                    (&first.expr, &second.expr)
                else {
                    unreachable!("sema requires SharedGuard.split projection lambdas");
                };
                return TExpr {
                    ty: resolved_ret.cloned().unwrap_or_else(unit_type),
                    kind: TExprKind::SharedGuardSplit {
                        guard: Box::new(lower_expr(receiver, cx, env)),
                        first: first
                            .meta
                            .guard_projection
                            .clone()
                            .expect("sema supplies the first SharedGuard.split projection"),
                        second: second
                            .meta
                            .guard_projection
                            .clone()
                            .expect("sema supplies the second SharedGuard.split projection"),
                        editable,
                    },
                };
            }
            ("wait", [condition, predicate]) => {
                let Expr::Lambda(predicate) = &predicate.expr else {
                    unreachable!("sema requires a SharedGuard.wait predicate lambda");
                };
                let expected = std::slice::from_ref(&inner);
                return TExpr {
                    ty: resolved_ret.cloned().unwrap_or_else(unit_type),
                    kind: TExprKind::SharedGuardWait {
                        guard: Box::new(lower_expr(receiver, cx, env)),
                        condition: Box::new(lower_expr(&condition.expr, cx, env)),
                        predicate: Box::new(lower_lambda_expecting_host_borrow(
                            predicate,
                            cx,
                            env,
                            expected,
                            false,
                        )),
                    },
                };
            }
            _ => {}
        }
    }
    if matches!(
        tir_recv_jet_ty(receiver, env),
        Some(Type::Named(name)) if name == Syntax::TYPE_CONDITION
    ) && args.is_empty()
        && matches!(method, "notify_one" | "notify_all")
    {
        return TExpr {
            ty: unit_type(),
            kind: TExprKind::ConditionNotify {
                condition: Box::new(lower_expr(receiver, cx, env)),
                all: method == "notify_all",
            },
        };
    }

    // Comptime fragment globals carry values but no local type slot. Recover
    // the two opaque regex receiver types from that canonical value instead
    // of misclassifying `binding.method()` as a static call.
    let fragment_recv_type = if recv_type.is_none() && super::is_eval_fragment() {
        let recv_name = match receiver {
            Expr::Ident(name, _) => Some(name),
            // D-META-STAGE1=B: a marked name is an ordinary name for dispatch.
            Expr::ComptimeName { name, .. } => Some(name),
            _ => None,
        };
        recv_name
            .and_then(|name| cx.const_values.get(name))
            .and_then(|value| match value {
                crate::Comptime::CtValue::Struct { type_name, .. } => match type_name.as_str() {
                    "__JetRegex" => Some(Syntax::TYPE_REGEX.to_string()),
                    "Match" => Some("Match".to_string()),
                    // Any handle the shared op table knows (Reader, Cursor,
                    // FileReader, …) recovers its receiver type the same way.
                    other if handle_method_op(other, method, args.len()).is_some() => {
                        Some(other.to_string())
                    }
                    _ => None,
                },
                _ => None,
            })
    } else {
        None
    };
    let recv_type = if recv_type.is_some() {
        recv_type
    } else {
        &fragment_recv_type
    };
    let lowered_receiver = std::cell::RefCell::new(lowered_receiver);
    let lower_expr = |expr: &Expr, cx: &Cx, env: &mut LowerEnv| {
        if std::ptr::eq(expr, receiver) {
            if let Some(lowered) = lowered_receiver.borrow_mut().take() {
                return lowered;
            }
        }
        crate::Codegen::TIR::lower_expr(expr, cx, env)
    };
    let lower_core_arg =
        |module: &str, method: &str, index: usize, expr: &Expr, cx: &Cx, env: &mut LowerEnv| {
            // Comptime/TirBridge lowers core-call arguments before sema elaborates
            // inferred typed literals. At a regex one-shot's first parameter, the
            // expected type is unambiguously Regex; lower the same checked literal
            // node that normal sema produces.
            if module == "core.regex"
                && index == 0
                && matches!(
                    method,
                    "is_match"
                        | "match"
                        | "find"
                        | "find_all"
                        | "matches"
                        | "split"
                        | "split_limit"
                        | "replace"
                        | "replace_all"
                )
            {
                if let Expr::TypedLit {
                    head: None,
                    body: crate::AST::TypedLitBody::Value(pattern),
                    span,
                } = expr
                {
                    return TExpr {
                        ty: Type::Named(Syntax::TYPE_REGEX.to_string()),
                        kind: TExprKind::CoreCall {
                            module: "core.regex".to_string(),
                            method: "literal".to_string(),
                            args: vec![lower_expr(pattern, cx, env)],
                            source_span: *span,
                            widen_to_vec: vec![false],
                        },
                    };
                }
            }

            lower_expr(expr, cx, env)
        };

    // D-ZIPPAD1: lower the complete list/iterator zip family as one TIR
    // contract. The shared node keeps free and method spellings identical;
    // the Prelude emitter supplies the lazy binary composition.
    if recv_type.is_none() && matches!(method, "zip" | "zip_short" | "zip_pad") {
        let lowered_recv = lower_expr(receiver, cx, env);
        if zip_sequence_type(&lowered_recv.ty) {
            let is_pad = method == "zip_pad";
            let mut inputs = Vec::new();
            let mut fields = vec![zip_field_name(0, None)];
            let mut fills = Vec::new();
            for arg in args {
                match (is_pad, arg.label.as_ref().map(|(name, _)| name.as_str())) {
                    (true, Some("fill")) | (true, Some("fills")) => {
                        fills.push(lower_expr(&arg.expr, cx, env));
                    }
                    _ => {
                        fields.push(zip_field_name(fields.len(), arg.label.as_ref().map(|(name, _)| name.as_str())));
                        inputs.push(lower_expr(&arg.expr, cx, env));
                    }
                }
            }
            return lower_zip_family(
                lowered_recv,
                inputs,
                fills,
                fields,
                method,
                resolved_ret,
            );
        }
        *lowered_receiver.borrow_mut() = Some(lowered_recv);
    }

    if let Expr::Ident(name, _) = receiver {
        if env.is_gc(name) {
            let root = env.place_of(name);
            let edge_args = match method {
                "insert" if args.len() > 1 => &args[1..],
                "remove" => &args[0..0],
                _ => args,
            };
            let mut edges = edge_args
                .iter()
                .flat_map(|arg| env.gc_edges_for_expr(&arg.expr, Some(name)))
                .collect::<Vec<_>>();
            edges.sort();
            edges.dedup();
            let mut lowered_args = args.to_vec();
            let index_temp = if matches!(method, "insert" | "remove") {
                args.first().and_then(|arg| {
                    let lowered = lower_expr(&arg.expr, cx, env);
                    if !lowered.ty.is_integer() {
                        return None;
                    }
                    let source_name = jet_format!("{jet_prefix}gc_index_{}", method_span.start);
                    lowered_args[0].expr = Expr::Ident(source_name.clone(), arg.span);
                    env.bind(
                        &source_name,
                        TLocal::generated(&source_name),
                        Some(lowered.ty.clone()),
                    );
                    Some((source_name, lowered))
                })
            } else {
                None
            };
            let saved = env.locals.get(name).cloned();
            env.gc_locals.remove(name);
            env.bind(
                name,
                TLocal::generated("value").through_ref(),
                saved.as_ref().and_then(|(_, ty)| ty.clone()),
            );
            let inner = lower_method_call(
                receiver,
                method,
                method_span,
                owner_type_args,
                type_args,
                &lowered_args,
                recv_type,
                resolved_ret,
                checked_widen,
                cx,
                env,
                None,
            );
            if let Some((place, ty)) = saved {
                env.bind(name, place, ty);
            }
            env.mark_gc(name);
            if let Some((temp, _)) = &index_temp {
                env.locals.remove(temp);
            }
            let ty = inner.ty.clone();
            use crate::Codegen::TIR::TGcEditKind;
            let kind = if method == "clear" {
                TGcEditKind::Clear
            } else if method == "pop" {
                TGcEditKind::Pop
            } else if method == "remove" && index_temp.is_some() {
                TGcEditKind::RemoveIndex
            } else if method == "insert" && index_temp.is_some() {
                TGcEditKind::InsertIndex
            } else if method == "prepend" {
                TGcEditKind::Prepend
            } else if matches!(method, "push" | "append") {
                TGcEditKind::Additive
            } else if edges.is_empty() {
                TGcEditKind::Plain
            } else {
                TGcEditKind::EdgeSlot
            };
            return TExpr {
                ty,
                kind: TExprKind::HostCall(Box::new(THostCall::GcEdit {
                    root,
                    method_span_start: method_span.start,
                    edges,
                    edit: Box::new(inner),
                    index_temp,
                    kind,
                })),
            };
        }
    }
    if recv_type.as_ref().is_some_and(|name| {
        cx.current_type_params.borrow().contains(name.as_str())
    }) && matches!(
        method,
        "read"
            | "write"
            | "write_all"
            | "add"
            | "sub"
            | "mul"
            | "div"
            | "equal"
            | "compare"
            | "query"
            | "query_one"
            | "execute"
            | "begin"
            | "commit"
            | "rollback"
    ) {
        let db_params_ty = Type::List(Box::new(Type::Named(Syntax::TYPE_DB_VALUE.to_string())));
        let recv = lower_expr(receiver, cx, env);
        let targs: Vec<_> = match method {
            "query" | "query_one" | "execute" => args
                .iter()
                .enumerate()
                .map(|(i, arg)| {
                    let arg_ty = if i == 0 {
                        Type::String
                    } else {
                        db_params_ty.clone()
                    };
                    lower_one_call_arg(arg, Some((arg.convention, arg_ty)), env, cx)
                })
                .collect(),
            "begin" | "commit" | "rollback" => args
                .iter()
                .map(|arg| lower_one_call_arg(arg, None, env, cx))
                .collect(),
            _ => {
                let arg_ty = match method {
                    "read" => Type::Int,
                    "write" | "write_all" => {
                        Type::List(Box::new(Type::IntN { signed: false, bits: 8 }))
                    }
                    _ => Type::Named(recv_type.clone().unwrap_or_default()),
                };
                args.iter()
                    .map(|arg| {
                        lower_one_call_arg(arg, Some((arg.convention, arg_ty.clone())), env, cx)
                    })
                    .collect()
            }
        };
        return TExpr {
            ty: resolved_ret.cloned().unwrap_or_else(unit_type),
            kind: TExprKind::MethodCall {
                recv: Box::new(recv),
                method: TMethodRef::bare(method),
                type_args: Vec::new(),
                args: targs,
                source_first_string_literal: first_string_literal_arg(args),
                operator_line: matches!(method, "add" | "sub" | "mul" | "div")
                    .then(|| crate::Diagnostics::span_line_col(&cx.src, method_span.start).0 as u32),
            },
        };
    }
    // D-NURSERY1/A: from `[Task<T>]` list type extract `T` (the joined result type).
    fn taskgroup_result_elem(tasks: &TExpr) -> Type {
        match &tasks.ty {
            Type::List(inner) => match inner.as_ref() {
                Type::Apply { name, args, .. } if name == "Task" && args.len() == 1 => {
                    args[0].clone()
                }
                other => (*other).clone(),
            },
            _ => unit_type(),
        }
    }
    let crypto_static = static_call_type_name_lower(receiver, env).and_then(|ty| {
        let helper = match (ty.as_str(), method) {
            ("Secret", "from_text") => "__secret_from_text",
            ("Secret", "from_bytes") => "__secret_from_bytes",
            ("SigningKey", "new_random") => "__signing_generate",
            ("X25519SecretKey", "new_random") => "__x25519_generate",
            ("VerifyKey", "from_bytes") => "__verify_key_from_bytes",
            ("X25519PublicKey", "from_bytes") => "__x25519_public_from_bytes",
            ("X25519PublicKey", "from_text") => "__x25519_public_from_text",
            ("Signature", "from_bytes") => "__signature_from_bytes",
            ("Sealed", "from_bytes") => "__sealed_from_bytes",
            ("WrappedKey", "from_bytes") => "__wrapped_from_bytes",
            ("WrappedVaultKey", "from_bytes") => "__vault_wrapped_from_bytes",
            ("KeyUnlock", "Recipient") => "__vault_unlock_recipient",
            ("KeyUnlock", "Passphrase") => "__vault_unlock_passphrase",
            ("PasswordHash", "parse") => "__password_parse",
            ("Hasher", "new") => "__hasher_new",
            _ => return None,
        };
        Some(helper)
    });
    if let Some(helper) = crypto_static {
        let module = "core.crypto";
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        let widen_to_vec = core_widen_to_vec(module, helper, &targs);
        let ty = resolved_ret.cloned().unwrap_or_else(|| crypto_helper_return_ty(helper));
        return TExpr { ty, kind: TExprKind::CoreCall {
            module: module.to_string(), method: helper.to_string(), args: targs, source_span: method_span, widen_to_vec,
        }};
    }
    if let Some(kind) = recv_type
        .as_deref()
        .map(|name| name.rsplit('.').next().unwrap_or(name))
    {
        let helper = crypto_instance_helper(kind, method);
        if let Some(helper) = helper {
            let recv = lower_expr(receiver, cx, env);
            let mut targs = vec![recv];
            if kind == "Hasher" && method == "update" {
                targs.extend(args.iter().map(|arg| lower_expr(&arg.expr, cx, env)));
            }
            let widen_to_vec = core_widen_to_vec("core.crypto", helper, &targs);
            let ty = resolved_ret.cloned().unwrap_or_else(|| crypto_helper_return_ty(helper));
            return TExpr { ty, kind: TExprKind::CoreCall {
                module: "core.crypto".to_string(), method: helper.to_string(), args: targs, source_span: method_span, widen_to_vec,
            }};
        }
    }
    // D-TOOL4: `expect(x).snapshot()` — render the harness snapshot call. Test
    // bodies are `Result<(), String>`, so the trailing `?` propagates mismatch.
    if method == Syntax::BUILTIN_SNAPSHOT {
        if let Expr::Call(call) = receiver {
            if call.name == Syntax::BUILTIN_EXPECT && call.args.len() == 1 {
                let val = lower_expr(&call.args[0].expr, cx, env);
                let line = crate::Diagnostics::span_line_col(&cx.src, method_span.start).0;
                let snap_path = format!(
                    "snapshots/{}_{}.snap",
                    cx.file.replace(['/', '\\', '.'], "_"),
                    line
                );
                return TExpr {
                    ty: unit_type(),
                    kind: TExprKind::HostCall(Box::new(THostCall::ExpectSnapshot {
                        value: Box::new(val),
                        snap_path,
                    })),
                };
            }
        }
    }

    // D-TYPEDTEXT1=D: `SQL.raw("…")` / `HTML.raw("…")` — the audited escape.
    // `SQL`/`HTML` name the type here (sema already confirmed no local shadows
    // it), so `recv_type` was never set for this call; check the receiver
    // shape directly instead.
    if method == "raw" {
        if let Expr::Ident(n, _) = receiver {
            let is_builtin = Syntax::typed_head_kind(n)
                .is_some_and(|kind| kind.is_typed_text());
            if is_builtin {
                let arg = lower_expr(&args[0].expr, cx, env);
                let kind = if n == "SQL" {
                    crate::Codegen::TIR::TTypedTextForm::SQLRaw
                } else if n == Syntax::TYPE_SH {
                    crate::Codegen::TIR::TTypedTextForm::ShRaw
                } else {
                    crate::Codegen::TIR::TTypedTextForm::HTMLRaw
                };
                return TExpr {
                    ty: Type::Named(n.clone()),
                    kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::TypedText {
                        kind,
                        arg: Box::new(arg),
                    })),
                };
            }
        }
    }
    // D-TYPEDTEXT1=D: `.template()`/`.params()` split a checked `SQL` value;
    // `.text()` reads the escaped `HTML` string.
    if recv_type.as_deref() == Some("SQL") && matches!(method, "template" | "params") {
        let recv = lower_expr(receiver, cx, env);
        let kind = if method == "template" {
            crate::Codegen::TIR::TTypedTextForm::SQLTemplate
        } else {
            crate::Codegen::TIR::TTypedTextForm::SQLParams
        };
        return TExpr {
            ty: if method == "template" {
                Type::String
            } else {
                Type::List(Box::new(Type::String))
            },
            kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::TypedText {
                kind,
                arg: Box::new(recv),
            })),
        };
    }
    if recv_type.as_deref() == Some("HTML") && method == "text" {
        let recv = lower_expr(receiver, cx, env);
        return TExpr {
            ty: Type::String,
            kind: TExprKind::Clone(Box::new(recv)),
        };
    }
    // D-COLLBREADTH1=A: lower type-owned set constructors into the same
    // receiver-first builtin used by instance algebra, preserving the
    // concrete set type for chained dispatch.
    if recv_type.is_none() {
        // D-BOUND-SINK1=A: sema has proved this is a declared head's
        // qualified or local static `.raw(String)` escape. The result already
        // carries the canonical checked-text carrier; pass the String through.
        if method == "raw"
            && args.len() == 1
            && is_checked_text_head_static(receiver, resolved_ret, cx)
        {
            let arg = lower_expr(&args[0].expr, cx, env);
            return TExpr {
                ty: Type::String,
                kind: arg.kind,
            };
        }
        if let Expr::Ident(type_name, _) = receiver {
            if !env.locals.contains_key(type_name)
                && matches!(type_name.as_str(), Syntax::TYPE_SET | Syntax::TYPE_SORTED_SET)
                && method == "from"
                && args.len() == 1
            {
                let lowered_list = lower_expr(&args[0].expr, cx, env);
                let elem = match &lowered_list.ty {
                    Type::List(inner) | Type::FixedList { elem: inner, .. } => (**inner).clone(),
                    _ => Type::Int,
                };
                let set_ty = Type::Apply {
                    name: type_name.clone(),
                    args: vec![elem],
                };
                return TExpr {
                    ty: resolved_ret.cloned().unwrap_or(set_ty),
                    kind: TExprKind::BuiltinMethod {
                        recv: Box::new(lowered_list),
                        op: if type_name == Syntax::TYPE_SORTED_SET {
                            TBuiltinOp::SortedSetFrom
                        } else {
                            TBuiltinOp::SetFrom
                        },
                        args: Vec::new(),
                    },
                };
            }
        }
    }
    // D-CONC-SPAWN1=D: parser-created `task` nodes outside a lexical group
    // still lower through the existing spawn/select TIR nodes. The receiver is
    // compiler-private and is never emitted or looked up as a Rust value.
    if recv_type.as_deref() == Some(Syntax::INTERNAL_TASK_SURFACE_TYPE) {
        if method == Syntax::INTERNAL_TASK_SPAWN_METHOD {
            if let Some(Expr::Lambda(lam)) = args.first().map(|a| &a.expr) {
                let body_ty = lambda_body_ty(lam, cx, env);
                let jit_lambda = lower_spawn_lambda_for_jit(lam, cx, env);
                let key = (env.fn_name.clone(), lam.span.start, lam.span.end);
                let existing = cx.jit_spawn_sites.borrow().get(&key).copied();
                let site = existing.unwrap_or_else(|| {
                    let site = cx.jit_spawn_site_base + cx.jit_spawn_lambdas.borrow().len();
                    cx.jit_spawn_lambdas.borrow_mut().push(jit_lambda);
                    cx.jit_spawn_sites.borrow_mut().insert(key, site);
                    site
                });
                let spawn_closure = render_spawn_lambda(lam, cx, env);
                return TExpr {
                    ty: Type::Apply {
                        name: "Task".to_string(),
                        args: vec![body_ty],
                    },
                    kind: TExprKind::CoreClosureCall {
                        kind: TCoreClosureKind::Spawn {
                            group: None,
                            site,
                            spawn_closure,
                        },
                    },
                };
            }
        }
        if args.len() == 1 {
            let tasks = lower_expr(&args[0].expr, cx, env);
            let elem = taskgroup_result_elem(&tasks);
            let ty = resolved_ret.cloned().unwrap_or_else(|| match method {
                Syntax::INTERNAL_TASK_ALL_METHOD => Type::Result {
                    ok: Box::new(Type::List(Box::new(elem.clone()))),
                    err: Box::new(Type::Named(crate::Syntax::TYPE_TASK_FAILURE.to_string())),
                },
                _ => Type::Result {
                    ok: Box::new(elem),
                    err: Box::new(Type::Named(crate::Syntax::TYPE_TASK_FAILURE.to_string())),
                },
            });
            let kind = match method {
                Syntax::INTERNAL_TASK_ALL_METHOD => TExprKind::TaskGroupAll {
                    tasks: Box::new(tasks),
                },
                Syntax::INTERNAL_TASK_RACE_METHOD => TExprKind::TaskGroupRace {
                    tasks: Box::new(tasks),
                },
                Syntax::INTERNAL_TASK_ANY_METHOD => TExprKind::TaskGroupAny {
                    tasks: Box::new(tasks),
                },
                _ => return TExpr { ty, kind: TExprKind::Unit },
            };
            return TExpr { ty, kind };
        }
    }

    // D-CONC-SPAWN1=D: compiler-private methods behind canonical `task.group`.
    if matches!(
        recv_type.as_deref(),
        Some(Syntax::TYPE_TASKGROUP) | Some(Syntax::INTERNAL_TASK_GROUP_SURFACE_TYPE)
    )
        && method == Syntax::INTERNAL_TASK_SPAWN_METHOD
    {
        if let Some(Expr::Lambda(lam)) = args.first().map(|a| &a.expr) {
            let body_ty = lambda_body_ty(lam, cx, env);
            let jit_lambda = lower_spawn_lambda_for_jit(lam, cx, env);
            let key = (env.fn_name.clone(), lam.span.start, lam.span.end);
            let existing = cx.jit_spawn_sites.borrow().get(&key).copied();
            let site = existing.unwrap_or_else(|| {
                let site = cx.jit_spawn_site_base + cx.jit_spawn_lambdas.borrow().len();
                cx.jit_spawn_lambdas.borrow_mut().push(jit_lambda);
                cx.jit_spawn_sites.borrow_mut().insert(key, site);
                site
            });
            let spawn_closure = render_spawn_lambda(lam, cx, env);
            let group = lower_expr(receiver, cx, env);
            return TExpr {
                ty: Type::Apply {
                    name: "Task".to_string(),
                    args: vec![body_ty],
                },
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::Spawn {
                        group: Some(Box::new(group)),
                        site,
                        spawn_closure,
                    },
                },
            };
        }
    }
    if matches!(
        recv_type.as_deref(),
        Some(Syntax::TYPE_TASKGROUP) | Some(Syntax::INTERNAL_TASK_GROUP_SURFACE_TYPE)
    )
        && method == Syntax::INTERNAL_TASK_ALL_METHOD
        && args.len() == 1
    {
        let tasks = lower_expr(&args[0].expr, cx, env);
        let elem = taskgroup_result_elem(&tasks);
        return TExpr {
            ty: resolved_ret
                .cloned()
                .unwrap_or_else(|| Type::Result {
                    ok: Box::new(Type::List(Box::new(elem))),
                    err: Box::new(Type::Named(crate::Syntax::TYPE_TASK_FAILURE.to_string())),
                }),
            kind: TExprKind::TaskGroupAll {
                tasks: Box::new(tasks),
            },
        };
    }
    if matches!(
        recv_type.as_deref(),
        Some(Syntax::TYPE_TASKGROUP) | Some(Syntax::INTERNAL_TASK_GROUP_SURFACE_TYPE)
    )
        && method == Syntax::INTERNAL_TASK_RACE_METHOD
        && args.len() == 1
    {
        let tasks = lower_expr(&args[0].expr, cx, env);
        let elem = taskgroup_result_elem(&tasks);
        return TExpr {
            ty: resolved_ret.cloned().unwrap_or_else(|| Type::Result {
                ok: Box::new(elem),
                err: Box::new(Type::Named(crate::Syntax::TYPE_TASK_FAILURE.to_string())),
            }),
            kind: TExprKind::TaskGroupRace {
                tasks: Box::new(tasks),
            },
        };
    }
    if matches!(
        recv_type.as_deref(),
        Some(Syntax::TYPE_TASKGROUP) | Some(Syntax::INTERNAL_TASK_GROUP_SURFACE_TYPE)
    )
        && method == Syntax::INTERNAL_TASK_ANY_METHOD
        && args.len() == 1
    {
        let tasks = lower_expr(&args[0].expr, cx, env);
        let elem = taskgroup_result_elem(&tasks);
        return TExpr {
            ty: resolved_ret.cloned().unwrap_or_else(|| Type::Result {
                ok: Box::new(elem),
                err: Box::new(Type::Named(crate::Syntax::TYPE_TASK_FAILURE.to_string())),
            }),
            kind: TExprKind::TaskGroupAny {
                tasks: Box::new(tasks),
            },
        };
    }
    if recv_type.as_deref() == Some(Syntax::TYPE_TASKGROUP)
        && method == Syntax::TASKGROUP_SELECT_METHOD
        && args.is_empty()
    {
        return TExpr {
            ty: Type::Apply {
                name: Syntax::TYPE_SELECT_BUILDER.to_string(),
                args: vec![],
            },
            kind: TExprKind::SelectStart,
        };
    }
    if recv_type
        .as_deref()
        .is_some_and(|rt| rt == Syntax::TYPE_SELECT_BUILDER || rt.starts_with("SelectBuilder<"))
    {
        let builder = lower_expr(receiver, cx, env);
        match method {
            Syntax::SELECT_RECV_METHOD if args.len() == 1 => {
                let channel = lower_expr(&args[0].expr, cx, env);
                let elem = match &builder.ty {
                    Type::Apply { name, args, .. }
                        if name == Syntax::TYPE_SELECT_BUILDER && args.len() == 1 =>
                    {
                        args[0].clone()
                    }
                    _ => unit_type(),
                };
                return TExpr {
                    ty: Type::Apply {
                        name: Syntax::TYPE_SELECT_BUILDER.to_string(),
                        args: vec![elem],
                    },
                    kind: TExprKind::SelectRecv {
                        builder: Box::new(builder),
                        channel: Box::new(channel),
                    },
                };
            }
            Syntax::SELECT_AFTER_METHOD if args.len() == 1 || args.len() == 2 => {
                let millis = lower_expr(&args[0].expr, cx, env);
                let value = args
                    .get(1)
                    .map(|arg| Box::new(lower_expr(&arg.expr, cx, env)));
                let ty = if let Some(value) = value.as_ref() {
                    match &builder.ty {
                        Type::Apply { name, args, .. }
                            if name == Syntax::TYPE_SELECT_BUILDER && args.len() == 1 =>
                        {
                            builder.ty.clone()
                        }
                        _ => Type::Apply {
                            name: Syntax::TYPE_SELECT_BUILDER.to_string(),
                            args: vec![value.ty.clone()],
                        },
                    }
                } else {
                    builder.ty.clone()
                };
                return TExpr {
                    ty,
                    kind: TExprKind::SelectAfter {
                        builder: Box::new(builder),
                        millis: Box::new(millis),
                        value,
                    },
                };
            }
            Syntax::SELECT_READ_METHOD if args.len() == 1 => {
                let stream = lower_expr(&args[0].expr, cx, env);
                return TExpr {
                    ty: builder.ty.clone(),
                    kind: TExprKind::SelectRead {
                        builder: Box::new(builder),
                        stream: Box::new(stream),
                    },
                };
            }
            Syntax::SELECT_WAIT_METHOD if args.is_empty() => {
                let ret = match &builder.ty {
                    Type::Apply { name, args, .. }
                        if name == Syntax::TYPE_SELECT_BUILDER && args.len() == 1 =>
                    {
                        args[0].clone()
                    }
                    _ => unit_type(),
                };
                return TExpr {
                    ty: ret,
                    kind: TExprKind::SelectWait {
                        builder: Box::new(builder),
                    },
                };
            }
            _ => {}
        }
    }
    // D-TXN3/D-TXN4: `<handle>.on_commit(() => { … })` on a `#Transact` handle.
    // The gate proved `recv_type == Some("Transaction")` and a single literal
    // zero-param lambda arg. Lower to `<handle>.on_commit(Box::new(move || { … }))`;
    // the Drop-backed LIFO-on-commit semantics live in the `JetTransaction` prelude
    // type. The receiver is the bound handle ident → its mangled Rust place.
    if method == Syntax::TXN_ON_COMMIT && recv_type.as_deref() == Some(Syntax::TXN_HANDLE_TYPE) {
        // The handle is always a bound ident (sema typed it `Transaction` from a
        // `#Transact(name)` binding); its mangled place is `__jet_<name>`.
        let handle = match receiver {
            Expr::Ident(name, _) => mangle(name),
            // Defensive: a non-ident receiver can't be a transaction handle, but
            // lowering it keeps the place well-formed if one ever appears.
            other => emit_tir_expr(&lower_expr(other, cx, env), cx),
        };
        if let Some(crate::AST::Expr::Lambda(lam)) = args.first().map(|a| &a.expr) {
            // Build the closure directly (not via `render_lambda_str`, which may add
            // its own `Box::new(…)` wrapper). The hook is stored in the transaction's
            // `Vec<Box<dyn FnOnce()>>`, so it must be a `move` closure boxed exactly
            // once by the `TCoreClosureKind::OnCommit` emit (no double-box).
            let tl = lower_lambda(lam, cx, env);
            let inner = format!("move |{}| {}", tl.params.join(", "), tl.body);
            let closure = if tl.prep.is_empty() {
                inner
            } else {
                format!("{{ {} {} }}", tl.prep, inner)
            };
            return TExpr {
                ty: Type::Named("TransactionGuard".to_string()),
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::OnCommit {
                        handle,
                        closure,
                        executable: Box::new(tl),
                    },
                },
            };
        }
    }
    // D-TXN-ROLLBACK (layer 3): `<handle>.on_rollback(() => { … })` on a `#Transact`
    // handle — the exact mirror of `on_commit`. Lower to
    // `<handle>.on_rollback(Box::new(move || { … }))`; the Drop-backed run-on-rollback
    // semantics live in the `JetTransaction` prelude type.
    if method == Syntax::TXN_ON_ROLLBACK && recv_type.as_deref() == Some(Syntax::TXN_HANDLE_TYPE) {
        let handle = match receiver {
            Expr::Ident(name, _) => mangle(name),
            other => emit_tir_expr(&lower_expr(other, cx, env), cx),
        };
        if let Some(crate::AST::Expr::Lambda(lam)) = args.first().map(|a| &a.expr) {
            let tl = lower_lambda(lam, cx, env);
            let inner = format!("move |{}| {}", tl.params.join(", "), tl.body);
            let closure = if tl.prep.is_empty() {
                inner
            } else {
                format!("{{ {} {} }}", tl.prep, inner)
            };
            return TExpr {
                ty: Type::Named("TransactionGuard".to_string()),
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::OnRollback {
                        handle,
                        closure,
                        executable: Box::new(tl),
                    },
                },
            };
        }
    }
    // D-CAP2 (D-MEM1/S4): a user-written `.clone()` MethodCall no longer reaches
    // here — sema never constructs one (unrecognized method, E0102/E0311), and
    // the compiler's own duplication rewrites build `Expr::Copy` instead (its
    // own `lower_expr` arm), which lowers straight to `TExprKind::Clone`.
    // c109 Phase 23: `.raw()` on a distinct type → `({recv}).0`. The receiver's resolved
    // type names the distinct; its base type (from `cx.distinct_types`) is the total
    // result type. Mirrors `emit_method_call`'s `METHOD_DISTINCT_RAW` early return.
    if method == Syntax::METHOD_DISTINCT_RAW {
        let recv = lower_expr(receiver, cx, env);
        let base = match &recv.ty {
            Type::Named(n) => cx
                .distinct_types
                .get(n)
                .map(|(b, _)| b.clone())
                .unwrap_or_else(unit_type),
            _ => unit_type(),
        };
        return TExpr {
            ty: base,
            kind: TExprKind::DistinctRaw(Box::new(recv)),
        };
    }
    // c109 Phase 27: a CALL THROUGH a fn-typed struct field — `w.step(4)`. The gate
    // proved `recv_type == Some(<CoveredStruct>)` and the named field is `Type::Fn`. The
    // AST `emit_method_call` (Expression.rs ~L1573) emits `(({recv}).{__jet_<field>})({args})`
    // with PLAIN args. Resolve the field's Rust name + the call's result type (the Fn's
    // return) here; emit just splices. (Tried before the JSON/core/user shapes, mirroring
    // the AST dispatch order — a fn-field check fires before user-method dispatch.)
    if let Some(fn_ty @ Type::Fn { params, ret, .. }) = fn_field_call_ty(method, recv_type, cx) {
        let ret_ty = ret.as_deref().cloned().unwrap_or_else(unit_type);
        let recv = lower_expr(receiver, cx, env);
        let conventions = match &fn_ty {
            Type::Fn {
                call_metadata: Some(metadata),
                ..
            } => Some(metadata.conventions.as_slice()),
            _ => None,
        };
        let targs: Vec<TCallArg> = args
            .iter()
            .enumerate()
            .map(|(index, a)| {
                let conv = params.get(index).cloned().map(|ty| {
                    (
                        conventions
                            .and_then(|row| row.get(index))
                            .copied()
                            .unwrap_or(AccessConvention::Read),
                        ty,
                    )
                });
                lower_one_call_arg(a, conv, env, cx)
            })
            .collect();
        let lowered = TExpr {
            ty: ret_ty,
            kind: TExprKind::FnFieldCall {
                recv: Box::new(recv),
                field: method.to_string(),
                args: targs,
            },
        };
        return match source_arg_order(args) {
            Some(order) => preserve_source_arg_order(
                lowered,
                &order,
                args.len(),
                method_span.start as u32,
            ),
            None => lowered,
        };
    }
    // D-ENC-DYN1=A+: a dynamic `Data` construction `Data.<Variant>(arg)` (the gate
    // proved the receiver is a `Data`/`JSON`/… type-name ident and `method` is a `Data`
    // variant). Lower to `TExprKind::JSONLit`, carrying the payload's `implicit_clone`
    // flag as a total fact. The result type is `Data`.
    if let Expr::Ident(type_name, _) = receiver {
        if !env.locals.contains_key(type_name)
            && is_json_type_name(type_name)
            && is_json_variant(method)
        {
            let arg = args
                .first()
                .map(|a| Box::new((lower_expr(&a.expr, cx, env), a.flags.implicit_clone)));
            return TExpr {
                ty: Type::Named(Syntax::TYPE_DATA.to_string()),
                kind: TExprKind::JSONLit {
                    variant: method.to_string(),
                    arg,
                },
            };
        }
    }
    // D-DBDRIVER1: a `DBValue` construction `DBValue.Int(n)` / `.Float(f)` /
    // `.Text(s)` / `.Bool(b)` (the gate proved the receiver is `DBValue` and
    // `method` a `DBValue` variant). Same shape as the `Data` construction above.
    if let Expr::Ident(type_name, _) = receiver {
        if !env.locals.contains_key(type_name)
            && is_db_value_type_name(type_name)
            && is_db_value_variant(method)
        {
            let arg = args
                .first()
                .map(|a| Box::new((lower_expr(&a.expr, cx, env), a.flags.implicit_clone)));
            return TExpr {
                ty: Type::Named(Syntax::TYPE_DB_VALUE.to_string()),
                kind: TExprKind::DBValueLit {
                    variant: method.to_string(),
                    arg,
                },
            };
        }
    }
    if matches!(receiver, Expr::Ident(name, _) if name == Syntax::CLOCK_TYPE)
        && !env.locals.contains_key(Syntax::CLOCK_TYPE)
    {
        if method == "new" && args.len() == 1 {
            let seed = lower_expr(&args[0].expr, cx, env);
            return TExpr {
                ty: Type::Named(Syntax::CLOCK_TYPE.to_string()),
                kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::Helper {
                    helper: format!("{}jet_std_clock_new", cx.root_prefix),
                    args: vec![crate::Codegen::TIR::THostArg::Expr(seed)],
                })),
            };
        }
        if method == "system" && args.is_empty() {
            return TExpr {
                ty: Type::Named(Syntax::CLOCK_TYPE.to_string()),
                kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::Helper {
                    helper: format!("{}jet_std_clock_system", cx.root_prefix),
                    args: vec![],
                })),
            };
        }
        if method == "now" && args.is_empty() {
            return TExpr {
                ty: Type::Named("Instant".to_string()),
                kind: TExprKind::CoreCall {
                    module: "core.time".to_string(),
                    method: "instant".to_string(),
                    args: Vec::new(),
                    source_span: method_span,
                    widen_to_vec: Vec::new(),
                },
            };
        }
    }
    // D-SHAPE-DURATION1=A: a bare `Duration.unit(value)` is a type-owned
    // checked constructor, not an instance/static user method.
    {
        let locals: HashSet<String> = env.locals.keys().cloned().collect();
        if let Some(unit) = duration_new_unit(receiver, method, &locals) {
            let value = lower_expr(&args[0].expr, cx, env);
            let float = matches!(value.ty, Type::Float | Type::Float32);
            return TExpr {
                ty: Type::Result {
                    ok: Box::new(Type::Named(Syntax::DURATION_TYPE.to_string())),
                    err: Box::new(Type::Named(
                        Syntax::DURATION_RANGE_ERROR_TYPE.to_string(),
                    )),
                },
                kind: TExprKind::HandleMethod {
                    recv: Box::new(value),
                    op: THandleOp::DurationNew { unit, float },
                    args: vec![],
                },
            };
        }
    }
    // c109 Phase 19: the arena allocator constructor `mem.Arena.new(…)` (D-ALLOC1). The
    // gate proved the receiver is `Field(Ident(mem-alias), <AllocType>)` + method `new`.
    // Render the whole ctor call HERE. Ordinary families call their runtime
    // constructors; Fixed.new carries an internal byte-count marker to the let
    // emitter so it can synthesize frame storage, and Fixed.over borrows its array.
    // result type is the allocator handle `Named(<AllocType>)` (`alloc_method_return`'s
    // `new` arm). The allocator's only `unsafe` lives in the vetted `jet_mem` prelude (I1).
    {
        let locals: HashSet<String> = env.locals.keys().cloned().collect();
        {
            if let Some(alloc_type) = alloc_new_type(receiver, method, cx, &locals) {
                let rust_type = alloc_handle_rust_type(alloc_type).unwrap_or("jet_mem::JetArena");
                let ctor = if alloc_type == "Fixed" && method == "new" {
                    let Some(Expr::Int(size, _, _, _)) = args.first().map(|arg| &arg.expr) else {
                        // Invalid source can still reach the lowering seam while the
                        // front end is assembling all diagnostics. Keep codegen total;
                        // the sema E0103 remains the user-facing result.
                        return TExpr {
                            ty: Type::Named(alloc_type.to_string()),
                            kind: TExprKind::Uninit,
                        };
                    };
                    format!("__JET_FIXED_INLINE:{size}")
                } else if alloc_type == "Fixed" && method == "over" {
                    let backing = match &args[0].expr {
                        Expr::Ident(name, _) if env.is_uninit_fixed(name) => env.place_of(name),
                        _ => emit_tir_expr(&lower_expr(&args[0].expr, cx, env), cx),
                    };
                    if matches!(&args[0].expr, Expr::Ident(name, _) if env.is_uninit_fixed(name)) {
                        format!("{rust_type}::over_uninit_fixed(&mut {backing})")
                    } else {
                        format!("{rust_type}::over(&mut {backing})")
                    }
                } else if args.is_empty() {
                    format!("{}::new()", rust_type)
                } else {
                    let ctor_fn = match alloc_type {
                        "Pool" => "with_slots",
                        _ => "with_capacity",
                    };
                    let a0 = emit_tir_expr(&lower_expr(&args[0].expr, cx, env), cx);
                    format!("{}::{}({} as usize)", rust_type, ctor_fn, a0)
                };
                return TExpr {
                    ty: Type::Named(alloc_type.to_string()),
                    kind: TExprKind::AllocNew { ctor },
                };
            }
        }
    }
    // D-SOLVER-LIB1=A: `solve.Solver.new(seed)` constructor. The receiver is a
    // core module sentinel (`solve.Solver`), so the seed arg becomes the lowered recv.
    {
        let locals: HashSet<String> = env.locals.keys().cloned().collect();
        if solve_new_type(receiver, method, cx, &locals).is_some() {
            let seed = lower_expr(&args[0].expr, cx, env);
            return TExpr {
                ty: Type::Named(Syntax::SOLVER_TYPE.to_string()),
                kind: TExprKind::HandleMethod {
                    recv: Box::new(seed),
                    op: THandleOp::SolverNew,
                    args: vec![],
                },
            };
        }
        if let Some(static_type) = game_static_type(receiver, method, cx, &locals) {
            let op = match (static_type, method) {
                ("Scene", "new") => THandleOp::GameSceneNew,
                ("Replay", "record") => THandleOp::GameReplayRecord,
                ("Backend", "headless") => THandleOp::GameBackendHeadless,
                _ => unreachable!("game_static_type admitted only stable game constructors"),
            };
            let ty = match static_type {
                "Scene" => Type::Named("GameScene".to_string()),
                "Replay" => Type::Named("GameReplay".to_string()),
                "Backend" => Type::Named("GameBackend".to_string()),
                _ => unit_type(),
            };
            let recv = if args.is_empty() {
                TExpr {
                    ty: unit_type(),
                    kind: TExprKind::Unit,
                }
            } else {
                lower_expr(&args[0].expr, cx, env)
            };
            let rest = args
                .iter()
                .skip(1)
                .map(|a| lower_expr(&a.expr, cx, env))
                .collect();
            return TExpr {
                ty,
                kind: TExprKind::HandleMethod {
                    recv: Box::new(recv),
                    op,
                    args: rest,
                },
            };
        }
        if let Some(op) = tls_static_op(receiver, method, cx, &locals) {
            let ty = match op {
                THandleOp::TLSClientConfigDefault => Type::Named("TLSClientConfig".to_string()),
                THandleOp::TLSRootCertificatesFromPem => Type::Result {
                    ok: Box::new(Type::Named("TLSRootCertificates".to_string())),
                    err: Box::new(Type::Named(Syntax::TYPE_IO_ERROR.to_string())),
                },
                THandleOp::TLSClientIdentityFromPem => Type::Result {
                    ok: Box::new(Type::Named("TLSClientIdentity".to_string())),
                    err: Box::new(Type::Named(Syntax::TYPE_IO_ERROR.to_string())),
                },
                _ => unit_type(),
            };
            return TExpr {
                ty,
                kind: TExprKind::HandleMethod {
                    recv: Box::new(TExpr {
                        ty: unit_type(),
                        kind: TExprKind::Unit,
                    }),
                    op,
                    args: args.iter().map(|arg| lower_expr(&arg.expr, cx, env)).collect(),
                },
            };
        }
        if let Some(op) = http_client_static_op(receiver, method, cx, &locals) {
            return TExpr {
                ty: Type::Named("HTTPClient".to_string()),
                kind: TExprKind::HandleMethod {
                    recv: Box::new(TExpr {
                        ty: unit_type(),
                        kind: TExprKind::Unit,
                    }),
                    op,
                    args: Vec::new(),
                },
            };
        }
    }
    // c109 Phase 16: an enum-variant CONSTRUCTION `Enum.Variant(args)` reaching codegen
    // as a `MethodCall` (sema never rewrites a payload variant to `Expr::EnumLit`). The
    // AST `emit_method_call` routes it to `emit_enum_lit` with all-positional args; we
    // reproduce that, resolving each arg's `clone`/`boxed` decisions via `lower_enum_arg`
    // (`emit_boxed_enum_arg` byte-for-byte). This is the construction half of the
    // string/struct/collection-payload + recursive (boxed) enum coverage.
    if recv_type.is_none() {
        if let Expr::Ident(type_name, _) = receiver {
            if !env.locals.contains_key(type_name) {
                // D-TERM1 (ratified 2026-06-22): `Key` is a prelude enum not in
                // `cx.enum_variants`; lower `Key.Variant(args)` to a TIR enum lit.
                let key_type = crate::Syntax::TYPE_KEY;
                if type_name == key_type && is_key_variant(method) {
                    let payload = if args.is_empty() {
                        TEnumPayload::Unit
                    } else {
                        let pos = args
                            .iter()
                            .map(|a| {
                                // Key payload args are always scalar/Char — no clone/box needed.
                                TEnumArg {
                                    value: lower_expr(&a.expr, cx, env),
                                    clone: false,
                                    boxed: false,
                                }
                            })
                            .collect();
                        TEnumPayload::Positional(pos)
                    };
                    return TExpr {
                        ty: Type::Named(type_name.clone()),
                        kind: TExprKind::EnumLit {
                            enum_type: type_name.clone(),
                            variant: method.to_string(),
                            payload,
                        },
                    };
                }
                if type_name == "DataEvent"
                    && matches!(method, "Bool" | "Int" | "Float" | "Text" | "Bytes" | "Key")
                {
                    let payload = TEnumPayload::Positional(
                        args.iter()
                            .map(|a| TEnumArg {
                                value: lower_expr(&a.expr, cx, env),
                                clone: false,
                                boxed: false,
                            })
                            .collect(),
                    );
                    return TExpr {
                        ty: Type::Named("DataEvent".to_string()),
                        kind: TExprKind::EnumLit {
                            enum_type: "DataEvent".to_string(),
                            variant: method.to_string(),
                            payload,
                        },
                    };
                }
                if let Some(variants) = cx.enum_variants.get(type_name) {
                    if variants.iter().any(|(v, _)| v == method) {
                        let payload = if args.is_empty() {
                            TEnumPayload::Unit
                        } else {
                            let pos = args
                                .iter()
                                .map(|a| {
                                    lower_enum_arg(type_name, method, method, &a.expr, cx, env)
                                })
                                .collect();
                            TEnumPayload::Positional(pos)
                        };
                        return TExpr {
                            ty: Type::Named(type_name.clone()),
                            kind: TExprKind::EnumLit {
                                enum_type: type_name.clone(),
                                variant: method.to_string(),
                                payload,
                            },
                        };
                    }
                }
            }
        }
    }
    // D-ENV-MUTATE1=A: current editions retain `env.set -> ()`, but invalid
    // runtime strings must produce existing E3001 at the Jet call span. Lower
    // this compatibility wrapper with all panic facts resolved before emit.
    if method == "set" && args.len() == 2 && !super::is_eval_fragment() {
        if let Expr::Ident(alias, _) = receiver {
            if !env.locals.contains_key(alias)
                && cx
                    .core_import_module_for_function(&env.fn_name, alias)
                    .is_some_and(|module| module == "core.env")
            {
                let name = lower_expr(&args[0].expr, cx, env);
                let value = lower_expr(&args[1].expr, cx, env);
                let loc = crate::Codegen::TIR::capture_panic_loc(&method_span, cx, env);
                return TExpr {
                    ty: unit_type(),
                    kind: TExprKind::HostCall(Box::new(THostCall::EnvSet {
                        name: Box::new(name),
                        value: Box::new(value),
                        loc,
                    })),
                };
            }
        }
    }

    // c109 Phase 10: a core/stdlib module call `alias.method(args)`.
    // Mirror TIR core-call emission: resolve the module here (total), lower args
    // PLAINLY (no clone/borrow wrappers —
    // `arg(i)` is a raw `emit_expr`), and carry the return type from the authoritative
    // `core_fixed_sig` table. Tried BEFORE the builtin shape (a core method named
    // `get`/`split`/… must not be claimed by the receiver-keyed builtin op).
    //
    // #777 / TirBridge: prefer the core-import alias even when `recv_type` is Some —
    // REPL/comptime fragments often mark the alias as a type-shaped receiver, which
    // would otherwise fall through to `StaticCall { User(alias) }` and E0956.
    if let Expr::Ident(alias, _) = receiver {
        if !env.locals.contains_key(alias) {
            let core_target = cx
                .core_import_module_for_function(&env.fn_name, alias)
                .map(|module| (module.to_owned(), method.to_owned()))
                .or_else(|| {
                    cx.inline_reexport_core
                        .get(&(alias.clone(), method.to_owned()))
                        .cloned()
                });
            if let Some((module, core_method)) = core_target {
                let method = core_method.as_str();
                if module == "core.archive" {
                    if let Some(source_call) = lower_archive_source_call(
                        method,
                        type_args,
                        resolved_ret,
                        args,
                        cx,
                        env,
                    ) {
                        return source_call;
                    }
                }
                // D-VERDICT-1321-1: variadic io.print/io.eprint — join the
                // arguments with newlines so the engines keep one-value calls.
                if module == "core.io"
                    && matches!(method, "print" | "eprint")
                    && args.len() > 1
                {                    let joined = crate::Codegen::TIR::lower::join_print_args(args, cx, env);
                    return TExpr {
                        ty: unit_type(),
                        kind: TExprKind::CoreCall {
                            module,
                            method: method.to_string(),
                            args: vec![joined],
                            source_span: method_span,
                            widen_to_vec: vec![false],
                        },
                    };
                }
                if let Some(transform) = lower_compute_transform_call(
                    &module,
                    method,
                    method_span,
                    args,
                    resolved_ret,
                    cx,
                    env,
                ) {
                    return transform;
                }
                // D-PIN1=A: `mem.pin(&place)` IS the exclusive borrow of
                // `place`. Sema proved the no-move contract before lowering
                // (I3), so every tier emits exactly what `&place` emits and
                // none of them re-encode the promise (I9).
                if module == Syntax::CORE_MEM_MODULE && method == Syntax::MEM_PIN {
                    if let Some(arg) = args.first() {
                        // `&place` reaches lowering either as `Expr::Place` (a
                        // written window) or as a plain place with the call
                        // argument carrying the write convention; both mean the
                        // same exclusive borrow.
                        let lowered = lower_expr(&arg.expr, cx, env);
                        let ty = Type::Apply {
                            name: Syntax::TYPE_PIN.to_string(),
                            args: vec![lowered.ty.clone()],
                        };
                        if matches!(lowered.kind, TExprKind::Borrow { .. }) {
                            return TExpr { ty, kind: lowered.kind };
                        }
                        return TExpr {
                            ty,
                            kind: TExprKind::Borrow {
                                place: Box::new(lowered),
                                mutable: true,
                            },
                        };
                    }
                }
                if let Some(t) =
                    lower_core_closure_call(&module, method, method_span, args, cx, env)
                {
                    return t;
                }
                let targs: Vec<TExpr> = args
                    .iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        lower_core_arg(&module, method, index, &arg.expr, cx, env)
                    })
                    .collect();
                let widen_to_vec = core_widen_to_vec(&module, method, &targs);
                let ty = if module == "core.mem" {
                    match method {
                        "address_of" => Type::Int,
                        "volatile_read" => targs
                            .first()
                            .and_then(|a| crate::Sema::ptr_elem(&a.ty))
                            .unwrap_or_else(unit_type),
                        "volatile_write" => unit_type(),
                        _ => core_call_return_ty(&module, method),
                    }
                } else if module == "core.encoding.cbor"
                    && method == "decode"
                    && !type_args.is_empty()
                {
                    resolved_ret.cloned().unwrap_or_else(|| Type::Result {
                        ok: Box::new(type_args[0].clone()),
                        err: Box::new(Type::List(Box::new(Type::Named(
                            "FieldError".to_string(),
                        )))),
                    })
                } else if crate::Sema::is_polymorphic_core_special(&module, method) {
                    resolved_ret.cloned().unwrap_or_else(|| {
                        if module == "core.io" && method == "progress" {
                            progress_return_ty(&targs)
                        } else {
                            unit_type()
                        }
                    })
                } else if module == "core.event"
                    && matches!(method, "new" | "with_policy" | "hook" | "async_result")
                {
                    resolved_ret
                        .cloned()
                        .unwrap_or_else(|| core_call_return_ty(&module, method))
                } else {
                    core_call_return_ty(&module, method)
                };
                demand_generic_serde_codec(
                    cx,
                    &env.fn_name,
                    &module,
                    method,
                    &targs,
                    &ty,
                );
                return TExpr {
                    ty,
                    kind: TExprKind::CoreCall {
                        module,
                        method: method.to_string(),
                        args: targs,
                        source_span: method_span,
                        widen_to_vec,
                    },
                };
            }
        }
    }
    if recv_type.is_none() {
        if matches!(receiver, Expr::Field(..)) {
            if let Some(submodule) = core_module_path_from_receiver(receiver, cx, env)
            {
                if submodule == "core.archive" {
                    if let Some(source_call) = lower_archive_source_call(
                        method,
                        type_args,
                        resolved_ret,
                        args,
                        cx,
                        env,
                    ) {
                        return source_call;
                    }
                }
                if let Some(transform) = lower_compute_transform_call(
                    &submodule,
                    method,
                    method_span,
                    args,
                    resolved_ret,
                    cx,
                    env,
                ) {
                    return transform;
                }
                let targs: Vec<TExpr> = args
                    .iter()
                    .enumerate()
                    .map(|(index, arg)| {
                        lower_core_arg(&submodule, method, index, &arg.expr, cx, env)
                    })
                    .collect();
                let widen_to_vec = core_widen_to_vec(&submodule, method, &targs);
                let ty = if crate::Sema::is_polymorphic_core_special(&submodule, method) {
                    resolved_ret.cloned().unwrap_or_else(|| {
                        if submodule == "core.io" && method == "progress" {
                            progress_return_ty(&targs)
                        } else {
                            core_call_return_ty(&submodule, method)
                        }
                    })
                } else {
                    core_call_return_ty(&submodule, method)
                };
                demand_generic_serde_codec(
                    cx,
                    &env.fn_name,
                    &submodule,
                    method,
                    &targs,
                    &ty,
                );
                return TExpr {
                    ty,
                    kind: TExprKind::CoreCall {
                        module: submodule,
                        method: method.to_string(),
                        args: targs,
                        source_span: method_span,
                        widen_to_vec,
                    },
                };
            }
        }
        // D-VERDICT-1867-1: lower `inline.foreign_alias.method(...)` after
        // sema has resolved the exported namespace. Its signatures use the
        // same per-function foreign scope as a direct inline import.
        if let Expr::Field(base, leaf, _) = receiver {
            if let Expr::Ident(owner, _) = base.as_ref() {
                if !env.locals.contains_key(owner) {
                    if let Some(rust_mod) = cx
                        .inline_reexport_foreign
                        .get(&(owner.clone(), leaf.clone()))
                        .cloned()
                    {
                        let sig = cx
                            .inline_foreign_reexport_sigs
                            .get(&(owner.clone(), leaf.clone(), method.to_string()))
                            .cloned()
                            .or_else(|| {
                                cx.import_signature_for_function(&env.fn_name, leaf, method)
                            });
                        let ret = cx
                            .inline_foreign_reexport_rets
                            .get(&(owner.clone(), leaf.clone(), method.to_string()))
                            .cloned()
                            .or_else(|| {
                                cx.import_return_for_function(&env.fn_name, leaf, method)
                            })
                            .flatten()
                            .unwrap_or_else(unit_type);
                        // C foreign namespaces mounted through an inline module
                        // still have a generated wrapper. Keep this branch on the
                        // same ExternCall path as a direct alias call so #Undo is
                        // registered before the wrapper runs. Cached JS/other
                        // foreign modules have no wrapper and remain ModuleCall
                        // values below.
                        let wrapper_key = format!("{rust_mod}::{method}");
                        if let Some(wrapper) = cx.extern_funcs.get(&wrapper_key).cloned() {
                            let eargs = args
                                .iter()
                                .enumerate()
                                .map(|(index, arg)| {
                                    let conv = sig
                                        .as_ref()
                                        .and_then(|params| params.get(index))
                                        .map(|(convention, ty)| (*convention, ty.clone()));
                                    lower_extern_call_arg(arg, conv, env, cx)
                                })
                                .collect();
                            let lowered = TExpr {
                                ty: ret.clone(),
                                kind: TExprKind::ExternCall {
                                    wrapper,
                                    args: eargs,
                                },
                            };
                            let undo = cx
                                .foreign_undos
                                .get(&wrapper_key)
                                .or_else(|| cx.foreign_undos.get(method))
                                .map(String::as_str);
                            return wrap_foreign_undo(
                                lowered,
                                undo,
                                method_span.start as u32,
                                cx,
                                env,
                            );
                        }
                        let undo = cx
                            .foreign_undos
                            .get(&wrapper_key)
                            .map(String::as_str);
                        let targs = lower_module_args(args, sig.as_deref(), env, cx);
                        let lowered = TExpr {
                            ty: ret,
                            kind: TExprKind::ModuleCall {
                                form: TModuleCallForm::Qualified {
                                    rust_mod,
                                    rust_fn: mangle(method).to_string(),
                                },
                                type_args: type_args.to_vec(),
                                args: targs,
                            },
                        };
                        return wrap_foreign_undo(
                            lowered,
                            undo,
                            method_span.start as u32,
                            cx,
                            env,
                        );
                    }
                }
            }
        }
        if let Expr::Ident(alias, _) = receiver {
            if !env.locals.contains_key(alias) {
                // c109 Phase 14: a qualified cross-module call `alias.method(args)`.
                // The gate proved the alias is a re-export / import_mod / code_module.
                // Mirror `emit_method_call`'s arms IN ORDER (reexport, import_mods,
                // code_modules) — resolving the path pieces here so emit decides nothing.
                if let Some(mangled_key) = cx
                    .inline_reexport_inline
                    .get(&(alias.clone(), method.to_string()))
                    .cloned()
                {
                    let sig = cx.sigs.get(&mangled_key).cloned();
                    let targs = lower_module_args(args, sig.as_deref(), env, cx);
                    return TExpr {
                        ty: call_return_type_with_args(cx, &mangled_key, type_args, &targs),
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::InlineMangled {
                                mangled: mangled_key,
                            },
                            type_args: type_args.to_vec(),
                            args: targs,
                        },
                    };
                }
                if let Some((real_mod, real_fn)) = cx
                    .reexport_calls
                    .get(&(alias.clone(), method.to_string()))
                    .cloned()
                {
                    let undo = cx
                        .foreign_undos
                        .get(&format!("{real_mod}::{real_fn}"))
                        .map(String::as_str);
                    let sig = cx
                        .import_sigs
                        .get(&(alias.clone(), method.to_string()))
                        .cloned();
                    let targs = lower_module_args(args, sig.as_deref(), env, cx);
                    let ret = cx
                        .import_rets
                        .get(&(alias.clone(), method.to_string()))
                        .cloned()
                        .flatten()
                        .unwrap_or_else(unit_type);
                    let lowered = TExpr {
                        ty: ret,
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::Qualified {
                                rust_mod: real_mod,
                                rust_fn: mangle(&real_fn).to_string(),
                            },
                            type_args: type_args.to_vec(),
                            args: targs,
                        },
                    };
                    return wrap_foreign_undo(
                        lowered,
                        undo,
                        method_span.start as u32,
                        cx,
                        env,
                    );
                }
                if let Some(mod_name) = cx.import_mods.get(alias).cloned() {
                    let undo = cx
                        .foreign_undos
                        .get(&format!("{mod_name}::{method}"))
                        .map(String::as_str);
                    let sig = cx
                        .import_sigs
                        .get(&(alias.clone(), method.to_string()))
                        .cloned();
                    if let Some(wrapper) = cx
                        .extern_funcs
                        .get(&format!("{mod_name}::{method}"))
                        .cloned()
                    {
                        let eargs = args
                            .iter()
                            .enumerate()
                            .map(|(index, arg)| {
                                let conv = sig
                                    .as_ref()
                                    .and_then(|params| params.get(index))
                                    .map(|(convention, ty)| (*convention, ty.clone()));
                                lower_extern_call_arg(arg, conv, env, cx)
                            })
                            .collect();
                        let ty = cx
                            .import_rets
                            .get(&(alias.clone(), method.to_string()))
                            .cloned()
                            .flatten()
                            .unwrap_or_else(unit_type);
                        let lowered = TExpr {
                            ty,
                            kind: TExprKind::ExternCall {
                                wrapper,
                                args: eargs,
                            },
                        };
                        let undo = cx
                            .foreign_undos
                            .get(&format!("{mod_name}::{method}"))
                            .map(String::as_str);
                        return wrap_foreign_undo(
                            lowered,
                            undo,
                            method_span.start as u32,
                            cx,
                            env,
                        );
                    }
                    let targs = lower_module_args(args, sig.as_deref(), env, cx);
                    let ret = cx
                        .import_rets
                        .get(&(alias.clone(), method.to_string()))
                        .cloned()
                        .flatten()
                        .unwrap_or_else(unit_type);
                    let lowered = TExpr {
                        ty: ret,
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::Qualified {
                                rust_mod: mod_name,
                                rust_fn: mangle(method).to_string(),
                            },
                            type_args: type_args.to_vec(),
                            args: targs,
                        },
                    };
                    return wrap_foreign_undo(
                        lowered,
                        undo,
                        method_span.start as u32,
                        cx,
                        env,
                    );
                }
                if let Some(mod_name) = cx
                    .import_module_for_function(&env.fn_name, alias)
                    .map(str::to_owned)
                {
                    let undo = cx
                        .foreign_undos
                        .get(&format!("{mod_name}::{method}"))
                        .map(String::as_str);
                    let sig = cx.import_signature_for_function(&env.fn_name, alias, method);
                    if let Some(wrapper) = cx
                        .extern_funcs
                        .get(&format!("{mod_name}::{method}"))
                        .cloned()
                    {
                        let eargs = args
                            .iter()
                            .enumerate()
                            .map(|(index, arg)| {
                                let conv = sig
                                    .as_ref()
                                    .and_then(|params| params.get(index))
                                    .map(|(convention, ty)| (*convention, ty.clone()));
                                lower_extern_call_arg(arg, conv, env, cx)
                            })
                            .collect();
                        let ty = cx
                            .import_return_for_function(&env.fn_name, alias, method)
                            .flatten()
                            .unwrap_or_else(unit_type);
                        let lowered = TExpr {
                            ty,
                            kind: TExprKind::ExternCall {
                                wrapper,
                                args: eargs,
                            },
                        };
                        let undo = cx
                            .foreign_undos
                            .get(&format!("{mod_name}::{method}"))
                            .map(String::as_str);
                        return wrap_foreign_undo(
                            lowered,
                            undo,
                            method_span.start as u32,
                            cx,
                            env,
                        );
                    }
                    let targs = lower_module_args(args, sig.as_deref(), env, cx);
                    let ret = cx
                        .import_return_for_function(&env.fn_name, alias, method)
                        .flatten()
                        .unwrap_or_else(unit_type);
                    let lowered = TExpr {
                        ty: ret,
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::Qualified {
                                rust_mod: mod_name,
                                rust_fn: mangle(method).to_string(),
                            },
                            type_args: type_args.to_vec(),
                            args: targs,
                        },
                    };
                    return wrap_foreign_undo(
                        lowered,
                        undo,
                        method_span.start as u32,
                        cx,
                        env,
                    );
                }
                if cx.code_modules.contains(alias.as_str()) {
                    let mangled_key = jet_foundation::Names::member_name(alias, method);
                    let undo = cx
                        .foreign_undos
                        .get(&format!("{alias}::{method}"))
                        .map(String::as_str);
                    let sig = cx.sigs.get(&mangled_key).cloned();
                    let targs = lower_module_args(args, sig.as_deref(), env, cx);
                    let lowered = TExpr {
                        ty: call_return_type_with_args(
                            cx,
                            &mangled_key,
                            type_args,
                            &targs,
                        ),
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::InlineMangled {
                                mangled: mangled_key,
                            },
                            type_args: type_args.to_vec(),
                            args: targs,
                        },
                    };
                    return wrap_foreign_undo(
                        lowered,
                        undo,
                        method_span.start as u32,
                        cx,
                        env,
                    );
                }
            }
        }
    }
    // c109 Phase 9: a built-in collection/string method (`emit_builtin_method`). The
    // gate proved `recv_type == None` + a covered builtin name + an in-subset value
    // receiver. Resolve the Map-vs-List-vs-String emit branch HERE from the
    // receiver's type (reproducing `expr_jet_ty`, incl. its `None` partiality), so
    // emit makes no type decision (I3). The result type comes from the builtin's
    // sema return (`Collections::builtin_method_return`) for totality.
    if recv_type.is_none()
        || matches!(recv_type.as_deref(), Some("Set") | Some("SortedSet"))
    {
        if let Some(op) =
            resolve_builtin_op(receiver, method, method_span, args, resolved_ret, env, cx)
        {
            // D-MEM1 S6: a mutating builtin (`.push()` etc.) on a place rooted in
            // a `Pool` index (`tree[root].children.push(child)`) needs the SAME
            // genuine-mutable-place treatment `LValue::Field`/`LValue::Index`
            // get above — the ordinary receiver lowering reads `pool[id]` as an
            // owned CLONE (`jet_pool_get`), so mutating it would silently edit a
            // throwaway copy instead of the real slot.
            let recv_ast_ty = tir_recv_jet_ty(receiver, env);
            let recv_mut_ty_hint = recv_ast_ty
                .clone()
                .or_else(|| pool_field_ty_hint(receiver, cx, env));
            let recv_t = if crate::Collections::builtin_needs_mut_receiver(
                recv_mut_ty_hint.as_ref().unwrap_or(&Type::Int),
                method,
            ) {
                lower_expr_as_mut_place(receiver, cx, env)
            } else {
                lower_expr(receiver, cx, env)
            };
            // #1478: `Set.min()`/`Set.max()` reuse the generic List reducer —
            // route through the same `.to_list()` a user would write so AOT
            // and JIT never see a raw `HashSet` where they expect a `Vec`.
            let recv_t = if matches!(op, TBuiltinOp::Min { .. } | TBuiltinOp::Max { .. }) {
                crate::Codegen::TIR::wrap_set_receiver_as_list(recv_t)
            } else {
                recv_t
            };
            // D-ITERTOOLS1=A: `tir_recv_jet_ty` is None for list literals, so a
            // chain like `[…].flatten().to_list()` can mis-resolve `to_list` as
            // SetToList. Prefer the lowered receiver type.
            let op = match (&op, method) {
                (
                    TBuiltinOp::SetToList
                    | TBuiltinOp::SortedSetToList
                    | TBuiltinOp::BitSetToList,
                    "to_list",
                ) if crate::Collections::is_iter_type(&recv_t.ty) => TBuiltinOp::IterToList,
                (_, "collect")
                    if matches!(
                        op,
                        TBuiltinOp::SetToList
                            | TBuiltinOp::SortedSetToList
                            | TBuiltinOp::BitSetToList
                    ) && crate::Collections::is_iter_type(&recv_t.ty) =>
                {
                    TBuiltinOp::IterCollect
                }
                _ => op,
            };
            // Prefer lowered Iter type when AST peek missed the chain.
            let recv_for_result = recv_ast_ty.as_ref().unwrap_or(&recv_t.ty);
            // D-HOLE1: `Option.zip`'s `b` type is heterogeneous (arg-dependent), so
            // the generic single-receiver-type table (`builtin_result_ty`) can't
            // resolve it; `resolve_builtin_op` already worked it out for the tuple
            // struct name above — reuse it here instead of guessing a placeholder.
            let result_ty = match &op {
                TBuiltinOp::OptionZip { elem_ty, .. } => Type::Option(Box::new(elem_ty.clone())),
                TBuiltinOp::IterToList | TBuiltinOp::IterCollect => {
                    // Sema's refined return wins over the iterator carrier's
                    // placeholder element type. The latter is only a fallback
                    // for defensive lowering without a resolved method fact.
                    resolved_ret
                        .cloned()
                        .or_else(|| {
                            crate::Collections::iter_elem(&recv_t.ty)
                                .map(|e| Type::List(Box::new(e.clone())))
                        })
                        .unwrap_or_else(unit_type)
                }
                _ if resolved_ret.is_some() => resolved_ret.cloned().unwrap_or_else(unit_type),
                // D-ITERTOOLS1=A: list-literal receivers leave `tir_recv` None;
                // use the lowered receiver type so adapters still type as `Iter`.
                _ => builtin_result_ty(method, args.len(), Some(recv_for_result)),
            };
            // Builtins that store an argument need an owned value. A borrowed
            // generic parameter is a dereferenced Rust place, so materialize a
            // clone here instead of leaking rustc E0507. Lookup/compare args stay
            // borrowed and avoid needless copies.
            let targs = args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    if builtin_arg_takes_ownership(&op, i) {
                        lower_owned_expr(&a.expr, cx, env)
                    } else {
                        lower_expr(&a.expr, cx, env)
                    }
                })
                .collect();
            return TExpr {
                ty: result_ty,
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(recv_t),
                    op,
                    args: targs,
                },
            };
        }
    }
    // c109 Phase 19: `Stopwatch.elapsed_millis()` (gate shape d2). The gate proved
    // `recv_type == None` + the `elapsed_millis` name + an in-subset value receiver.
    // Lower to the existing `THandleOp::StopwatchElapsedMillis` (`{root}jet_stopwatch_
    // elapsed_millis(&(recv))`), the same node the Phase-13 handle shape uses — emit is
    // byte-identical to `emit_builtin_method`'s name-keyed `elapsed_millis` arm. The
    // result type is `Int` (`stopwatch_method_return`), kept total per the design.
    if recv_type.is_none() && method == "elapsed_millis" && args.is_empty() {
        let recv_t = lower_expr(receiver, cx, env);
        return TExpr {
            ty: Type::Int,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::StopwatchElapsedMillis,
                args: Vec::new(),
            },
        };
    }
    // c109 Phase 24: `Match.group(n)` (gate shape d4). The gate proved `recv_type ==
    // Some("Match")` + `group`/1 + an in-subset value receiver. Lower to `BuiltinMethod`/
    // `MatchGroup`, byte-for-byte `emit_builtin_method`'s `("Match", "group")` arm. The
    // result type is `String?`. Placed BEFORE the user-instance shape (also `recv_type ==
    // Some`) — `Match` is never a covered user struct/enum, so the two never collide.
    if recv_type.as_deref() == Some("Match") && method == "group" && args.len() == 1 {
        let recv_t = lower_expr(receiver, cx, env);
        let arg0 = lower_expr(&args[0].expr, cx, env);
        return TExpr {
            ty: Type::Option(Box::new(Type::String)),
            kind: TExprKind::BuiltinMethod {
                recv: Box::new(recv_t),
                op: TBuiltinOp::MatchGroup,
                args: vec![arg0],
            },
        };
    }
    // D-REACT1=B: a reactive `Signal`/`Derived` method (gate shape d5). The gate proved
    // `recv_type == Some("Signal"|"Derived")` + `get`/0 or `set`/1. Resolve the op +
    // result type HERE from the receiver's already-resolved `Apply<T>` slot (I3):
    // `Signal.get()`/`Derived.get()` → `T`; `Signal.set(v)` → Unit.
    if matches!(
        recv_type.as_deref(),
        Some("Signal") | Some("Derived") | Some("Computed")
    ) && is_reactive_method_name(method, args.len())
    {
        let recv_t = lower_expr(receiver, cx, env);
        let elem = match &recv_t.ty {
            Type::Apply { args, .. } => args.first().cloned(),
            _ => None,
        }
        .unwrap_or_else(unit_type);
        let (op, ty) = match method {
            "get" => (THandleOp::ReactiveGet, elem),
            "set" => (THandleOp::ReactiveSet, unit_type()),
            _ => unreachable!("is_reactive_method_name admitted only get/set"),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op,
                args: targs,
            },
        };
    }
    if recv_type.as_deref() == Some(crate::Syntax::TYPE_EFFECT)
        && is_reactive_effect_method_name(method, args.len())
    {
        let recv_t = lower_expr(receiver, cx, env);
        let ty = if method == "is_active" {
            Type::Bool
        } else {
            unit_type()
        };
        return TExpr {
            ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::ReactiveEffectMethod {
                    method: method.to_string(),
                },
                args: Vec::new(),
            },
        };
    }
    // D-EVENT1=D: Event/Hook/Subscription/EventScope/EventTrace methods.
    if is_event_handle_type(recv_type.as_deref()) && is_event_method_name(method, args.len()) {
        let recv_t = lower_expr(receiver, cx, env);
        let unit = unit_type();
        let result_ty = match (recv_type.as_deref(), method) {
            (Some("Event"), "emit") => Type::Named("EventTrace".to_string()),
            (Some("AsyncEvent"), "emit_async") => match &recv_t.ty {
                Type::Apply { args, .. } if args.len() >= 2 => Type::Apply {
                    name: "Task".to_string(),
                    args: vec![Type::Apply { name: "DispatchReport".to_string(), args: vec![args[1].clone()] }],
                },
                _ => Type::Named("Unknown".to_string()),
            },
            (Some("Event"), "on" | "once" | "on_priority")
            | (Some("AsyncEvent"), "on" | "once" | "on_priority")
            | (Some("Hook"), "on" | "once" | "on_priority")
            | (Some("DecisionHook"), "on" | "once" | "on_priority") => {
                Type::Named("Subscription".to_string())
            }
            (Some("Hook"), "run") => match &recv_t.ty {
                Type::Apply { args, .. } if args.len() >= 2 => args[1].clone(),
                _ => Type::Named("Unknown".to_string()),
            },
            (Some("DecisionHook"), "run") => match &recv_t.ty {
                Type::Apply { args, .. } if args.len() >= 2 => Type::Apply {
                    name: "HookOutcome".to_string(),
                    args: vec![args[0].clone(), args[1].clone()],
                },
                _ => Type::Named("Unknown".to_string()),
            },
            (Some("DispatchReport"), "trace") => Type::Named("EventTrace".to_string()),
            (_, "trace" | "summary") => Type::String,
            (
                _,
                "listener_count" | "queued_count" | "active_count" | "delivered" | "queued"
                | "dropped" | "running_count" | "blocked_count" | "delivered_handlers",
            ) => Type::Int,
            (_, "is_active" | "accepted") => Type::Bool,
            (Some("DispatchReport"), "state") => Type::Named("DispatchState".to_string()),
            _ => unit,
        };
        let expected_payload = match &recv_t.ty {
            Type::Apply { args, .. } => args.first().cloned(),
            _ => None,
        };
        let expected_hook_result = match &recv_t.ty {
            Type::Apply { name, args } if name == "AsyncEvent" && args.len() >= 2 => Some(Type::Result {
                ok: Box::new(Type::Named("Unit".to_string())),
                err: Box::new(args[1].clone()),
            }),
            Type::Apply { name, args } if name == "DecisionHook" && args.len() >= 2 => Some(Type::Apply {
                name: "HookDecision".to_string(),
                args: vec![args[0].clone(), args[1].clone()],
            }),
            Type::Apply { args, .. } if args.len() >= 2 => args.get(1).cloned(),
            _ => None,
        };
        let targs: Vec<TExpr> = args
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let handler_idx = matches!(
                    (method, args.len(), i),
                    ("on" | "once", 2, 1) | ("on_priority", 3, 2)
                );
                if handler_idx {
                    if let Expr::Lambda(lam) = &a.expr {
                        let params = expected_payload.clone().into_iter().collect::<Vec<_>>();
                        let mut jit_lambda = lower_spawn_lambda_for_jit(lam, cx, env);
                        if let Some(ty) = expected_payload.clone() {
                            if jit_lambda.params.is_empty() {
                                jit_lambda.params.push(("__payload".into(), ty));
                            } else {
                                for (_, pty) in &mut jit_lambda.params {
                                    *pty = ty.clone();
                                }
                            }
                        }
                        cx.jit_spawn_lambdas.borrow_mut().push(jit_lambda);
                        let tl = lower_lambda_expecting_value(lam, cx, env, params.as_slice());
                        return TExpr {
                            ty: Type::Fn {
                                params,
                                ret: expected_hook_result.clone().map(Box::new),
                                effect_bound: None, return_view_provenance: None,
                                param_contract: None,
                call_metadata: None,
                            },
                            kind: TExprKind::Lambda(Box::new(tl)),
                        };
                    }
                }
                lower_expr(&a.expr, cx, env)
            })
            .collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::EventMethod {
                    method: method.to_string(),
                },
                args: targs,
            },
        };
    }
    // D-WATCH-SCOPE1: WatchHandle/WatchSet methods. Callback lambdas receive
    // a WatchEvent payload, matching the shared Core event/callback model.
    if is_watch_handle_type(recv_type.as_deref()) && is_watch_method_name(method, args.len()) {
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match (recv_type.as_deref(), method) {
            (_, "poll" | "events") => Type::List(Box::new(Type::Named("WatchEvent".to_string()))),
            (Some("WatchHandle"), "on" | "once") => Type::Named("Subscription".to_string()),
            (_, "summary") => Type::String,
            (_, "is_active") => Type::Bool,
            _ => unit_type(),
        };
        let mut callback_index = None;
        let targs: Vec<TExpr> = args
            .iter()
            .enumerate()
            .map(|(i, a)| {
                if matches!(
                    (recv_type.as_deref(), method, args.len(), i),
                    (Some("WatchHandle"), "on" | "once", 2, 1)
                ) {
                    if let Expr::Lambda(lam) = &a.expr {
                        let params = vec![Type::Named("WatchEvent".to_string())];
                        let tl = lower_lambda_expecting_value(lam, cx, env, params.as_slice());
                        let idx = cx.jit_spawn_lambdas.borrow().len();
                        let jit_lambda = lower_spawn_lambda_for_jit_expecting(
                            lam,
                            cx,
                            env,
                            &[Type::Named("WatchEvent".to_string())],
                        );
                        cx.jit_spawn_lambdas.borrow_mut().push(jit_lambda);
                        callback_index = Some(idx);
                        return TExpr {
                            ty: Type::Fn {
                                params,
                                ret: None,
                                effect_bound: None, return_view_provenance: None,
                                param_contract: None,
                call_metadata: None,
                            },
                            kind: TExprKind::Lambda(Box::new(tl)),
                        };
                    }
                }
                lower_expr(&a.expr, cx, env)
            })
            .collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::WatchMethod {
                    method: method.to_string(),
                    callback_index,
                },
                args: targs,
            },
        };
    }
    // D-PROCESS1: ProcessSpec/ProcessChild methods lower to explicit prelude
    // helpers; sema already proved arity and argument types.
    if matches!(
        recv_type.as_deref(),
        Some("ProcessSpec") | Some("ProcessChild") | Some("TerminalSession")
    ) {
        let recv_t = lower_expr(receiver, cx, env);
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        let result_ty = match (recv_type.as_deref(), method) {
            (Some("ProcessSpec"), "run" | "run_checked") => Type::Result {
                ok: Box::new(Type::Named("ProcessResult".to_string())),
                err: Box::new(Type::Named("IOError".to_string())),
            },
            (Some("ProcessSpec"), "spawn") => Type::Result {
                ok: Box::new(Type::Named("ProcessChild".to_string())),
                err: Box::new(Type::Named("IOError".to_string())),
            },
            (Some("ProcessSpec"), "capabilities") => Type::Apply {
                name: "Set".to_string(),
                args: vec![Type::String],
            },
            (Some("ProcessSpec"), _) => Type::Named("ProcessSpec".to_string()),
            (Some("ProcessChild"), "id") => Type::Int,
            (Some("ProcessChild"), "wait") => Type::Result {
                ok: Box::new(Type::Named("ProcessResult".to_string())),
                err: Box::new(Type::Named("IOError".to_string())),
            },
            (Some("ProcessChild"), "exited") => Type::Result {
                ok: Box::new(Type::Bool),
                err: Box::new(Type::Named("IOError".to_string())),
            },
            (Some("ProcessChild"), _) => Type::Result {
                ok: Box::new(unit_type()),
                err: Box::new(Type::Named("IOError".to_string())),
            },
            (Some("TerminalSession"), "resize") => Type::Result {
                ok: Box::new(unit_type()),
                err: Box::new(Type::Named("IOError".to_string())),
            },
            _ => unit_type(),
        };
        let op = if recv_type.as_deref() == Some("ProcessSpec") {
            THandleOp::ProcessSpecMethod {
                method: method.to_string(),
            }
        } else if recv_type.as_deref() == Some("ProcessChild") {
            THandleOp::ProcessChildMethod {
                method: method.to_string(),
            }
        } else {
            THandleOp::TerminalSessionResize
        };
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op,
                args: targs,
            },
        };
    }
    // D-PROCESS1=A: `.write(text)` on `child.stdin` — the receiver LOWERS to the
    // real `ProcessChild.stdin` Rust field (a writer handle), and the write goes
    // through the generic `jet_process_stdin_write` prelude helper.
    if recv_type.as_deref() == Some("ProcessStdin") && method == "write" {
        let recv_t = lower_expr(receiver, cx, env);
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty: Type::Result {
                ok: Box::new(unit_type()),
                err: Box::new(Type::Named("IOError".to_string())),
            },
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::ProcessStdinWrite,
                args: targs,
            },
        };
    }
    // D-HONESTNUM1=A: a `Measurement<Float>` method (gate shape d6).
    if recv_type.as_deref() == Some("Measurement") && is_measurement_method_name(method, args.len())
    {
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match method {
            "value" | "uncertainty" => Type::Float,
            _ => recv_t.ty.clone(),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::MeasurementMethod {
                    method: method.to_string(),
                },
                args: targs,
            },
        };
    }
    // D-PENDING1=B: a `Loadable<T,E>` method (gate shape d7).
    if recv_type.as_deref() == Some("Loadable") && is_loadable_method_name(method, args.len()) {
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match method {
            "is_loading" | "is_loaded" | "is_failed" | "is_idle" => Type::Bool,
            // .loaded() → Option<T> (extract T from Loadable<T,E>)
            "loaded" => match &recv_t.ty {
                Type::Apply { args: targs, .. } if !targs.is_empty() => {
                    Type::Option(Box::new(targs[0].clone()))
                }
                _ => Type::Option(Box::new(Type::Named("Unknown".to_string()))),
            },
            // .or_else(default) → T
            "or_else" => match &recv_t.ty {
                Type::Apply { args: targs, .. } if !targs.is_empty() => targs[0].clone(),
                _ => Type::Named("Unknown".to_string()),
            },
            _ => recv_t.ty.clone(),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::LoadableMethod {
                    method: method.to_string(),
                },
                args: targs,
            },
        };
    }
    // D-SHAPE-CTORVERB1=C: generic `ExpiringValue<T>` uses type-owned construction.
    if recv_type.as_deref() == Some(Syntax::EXPIRING_VALUE_TYPE)
        && matches!(method, "get" | "is_valid" | "force")
    {
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match method {
            "get" => Type::Result {
                ok: Box::new(match &recv_t.ty {
                    Type::Apply { args, .. } if !args.is_empty() => args[0].clone(),
                    _ => Type::Named("Unknown".to_string()),
                }),
                err: Box::new(Type::Named("Expired".to_string())),
            },
            "is_valid" => Type::Bool,
            _ => recv_t.ty.clone(),
        };
        let targs: Vec<TExpr> = args.iter().map(|arg| lower_expr(&arg.expr, cx, env)).collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::ExpiringMethod {
                    method: method.to_string(),
                },
                args: targs,
            },
        };
    }
    // D-RENDERTGT2=A (c133 M1/M2): a UI backend method (gate shape d7b).
    if matches!(
        recv_type.as_deref(),
        Some("NullBackend" | "TuiBackend" | "GtkBackend")
    ) && is_ui_backend_method_name(recv_type.as_deref(), method, args.len())
    {
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match method {
            "measure" => Type::Named("Size".to_string()),
            "on_event" => Type::Named("EventResult".to_string()),
            "commands" | "frame_lines" => Type::List(Box::new(Type::String)),
            "render_count" => Type::Int,
            // D-A11YGATE1=B (c134 Phase 6): keyboard focus routing.
            "focused_label" => Type::String,
            // D-UIDEVSHELL1=A (c134 Phase 8): native GTK4 widget handles.
            "label" | "button" => Type::Int,
            _ => unit_type(),
        };
        // Resident JIT registers `on_click` via spawn-site (Game on_frame pattern).
        if method == "on_click" {
            if let Some(Expr::Lambda(lam)) = args.get(1).map(|a| &a.expr) {
                let jit_lambda = lower_spawn_lambda_for_jit(lam, cx, env);
                cx.jit_spawn_lambdas.borrow_mut().push(jit_lambda);
            }
        }
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::UiBackendMethod {
                    method: method.to_string(),
                },
                args: targs,
            },
        };
    }
    // c-devserver (owner-directed 2026-07-01): a DevServer builder method.
    if recv_type.as_deref() == Some("DevServer") && is_devserver_method_name(method, args.len()) {
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match method {
            "serve" => unit_type(),
            _ => Type::Named("DevServer".to_string()),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::DevServerMethod {
                    method: method.to_string(),
                },
                args: targs,
            },
        };
    }
    // D-WEBAPP1=D: an App builder method.
    if recv_type.as_deref() == Some("App") && is_app_method_name(method, args.len()) {
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match method {
            "serve" | "serve_on" => unit_type(),
            "facts_json" => Type::String,
            _ => Type::Named("App".to_string()),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::AppMethod {
                    method: method.to_string(),
                },
                args: targs,
            },
        };
    }
    // D-NETDEP1=A / D-HTTPLIB1=A: an HTTP type method (gate shape d10).
    if is_http_type(recv_type.as_deref()) && is_http_method_name(recv_type.as_deref(), method) {
        let kind = recv_type.as_deref().unwrap_or("HTTPRequest").to_string();
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match (kind.as_str(), method) {
            ("HTTPRequest", "header" | "body" | "timeout" | "connect_timeout" | "read_timeout"
                | "total_timeout" | "dns_timeout" | "tls_timeout" | "write_timeout"
                | "first_byte_timeout" | "redirects" | "proxy" | "cookie" | "form" | "multipart_text")
                if !args.is_empty() && !(method == "header" && args.len() == 1) => {
                    Type::Named("HTTPRequest".to_string())
                }
            ("HTTPRequest", "send") => Type::Result {
                ok: Box::new(Type::Named("HTTPResponse".to_string())),
                err: Box::new(Type::Named("HTTPError".to_string())),
            },
            ("HTTPRequest", "method" | "path") => Type::String,
            ("HTTPRequest", "body") => Type::Named("HTTPBody".to_string()),
            ("HTTPRequest", "trailers") => Type::Result {
                ok: Box::new(Type::Named("HTTPHeaders".to_string())),
                err: Box::new(Type::Named("HTTPError".to_string())),
            },
            ("HTTPRequest", "body_len") => Type::Int,
            ("HTTPRequest", "under_limit") => Type::Bool,
            ("HTTPRequest", "param" | "header") => Type::Option(Box::new(Type::String)),
            ("HTTPClient", "cookies" | "redirects" | "protocols" | "timeouts" | "raw_encoding" | "proxy" | "tls" | "allow_http_downgrade" | "retries") => Type::Named("HTTPClient".to_string()),
            ("HTTPClient", "send") => Type::Result {
                ok: Box::new(Type::Named("HTTPResponse".to_string())),
                err: Box::new(Type::Named("HTTPError".to_string())),
            },
            ("HTTPResponse", "header") if args.len() == 2 => Type::Named("HTTPResponse".to_string()),
            ("HTTPResponse", "status") => Type::Int,
            ("HTTPResponse", "body") => Type::Named("HTTPBody".to_string()),
            ("HTTPResponse", "header") => Type::Option(Box::new(Type::String)),
            ("HTTPResponse", "cookies") => Type::List(Box::new(Type::String)),
            ("HTTPResponse", "trailers") => Type::Result {
                ok: Box::new(Type::Named("HTTPResponse".to_string())),
                err: Box::new(Type::Named("HTTPError".to_string())),
            },
            ("HTTPResponse", "protocol" | "remote_address") => Type::String,
            ("HTTPResponse", "redirect_history") => Type::List(Box::new(Type::String)),
            ("HTTPResponse", "timings") => Type::List(Box::new(Type::Int)),
            ("HTTPResponse", "reused_connection") => Type::Bool,
            ("HTTPResponse", "raw_content_encoding") => Type::Option(Box::new(Type::String)),
            ("HTTPHeaders", "first") => Type::Option(Box::new(Type::String)),
            ("HTTPHeaders", "all") => Type::List(Box::new(Type::String)),
            ("HTTPHeaders", "append" | "set") => Type::Result {
                ok: Box::new(Type::Named("HTTPHeaders".to_string())),
                err: Box::new(Type::Named("HTTPError".to_string())),
            },
            ("HTTPHeaders", "remove") => Type::Named("HTTPHeaders".to_string()),
            ("HTTPMux", _) => unit_type(),
            ("HTTPHandler", "handle") => Type::Result {
                ok: Box::new(Type::Named("HTTPResponse".to_string())),
                err: Box::new(Type::Named("HTTPError".to_string())),
            },
            ("HTTPBody", "bytes") => Type::Result {
                ok: Box::new(Type::List(Box::new(Type::Named("U8".to_string())))),
                err: Box::new(Type::Named("HTTPError".to_string())),
            },
            ("HTTPBody", "text") => Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::Named("HTTPError".to_string())),
            },
            ("HTTPBody", "json") => type_args.first().map_or_else(
                || resolved_ret.cloned().unwrap_or_else(|| Type::Result {
                    ok: Box::new(Type::Named("Unknown".to_string())),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                }),
                |target| Type::Result {
                    ok: Box::new(target.clone()),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                },
            ),
            // D-HTTP-JSON1=A: `req.json<T>()` / `resp.json<T>(limit)`.
            ("HTTPRequest", "json") | ("HTTPResponse", "json") => type_args.first().map_or_else(
                || resolved_ret.cloned().unwrap_or_else(|| Type::Result {
                    ok: Box::new(Type::Named("Unknown".to_string())),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                }),
                |target| Type::Result {
                    ok: Box::new(target.clone()),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                },
            ),
            ("HTTPBody", "chunks") => Type::Named("HTTPBodyChunks".to_string()),
            ("HTTPBody", "copy_to") => Type::Result {
                ok: Box::new(Type::Int),
                err: Box::new(Type::Named("HTTPError".to_string())),
            },
            ("HTTPServer", "local_addr") => Type::Result { ok: Box::new(Type::String), err: Box::new(Type::Named("HTTPError".to_string())) },
            ("HTTPServer", "serve" | "shutdown") => Type::Result {
                ok: Box::new(Type::Named("HTTPShutdownReport".to_string())),
                err: Box::new(Type::Named("HTTPError".to_string())),
            },
            ("WsConn", "send_text" | "send_bytes" | "close") => Type::Result {
                ok: Box::new(unit_type()),
                err: Box::new(Type::Named("WsError".to_string())),
            },
            ("WsConn", "recv") => Type::Result {
                ok: Box::new(Type::Named("WsMessage".to_string())),
                err: Box::new(Type::Named("WsError".to_string())),
            },
            ("WsMessage", "is_text" | "is_binary" | "is_close") => Type::Bool,
            ("WsMessage", "text") => Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::Named("WsError".to_string())),
            },
            ("WsMessage", "bytes") => Type::Result {
                ok: Box::new(Type::List(Box::new(Type::Named("U8".to_string())))),
                err: Box::new(Type::Named("WsError".to_string())),
            },
            ("Browser", "capabilities") => Type::Named("BrowserCapabilities".to_string()),
            ("Browser", "context") => Type::Result {
                ok: Box::new(Type::Named("BrowserContext".to_string())),
                err: Box::new(Type::Named("BrowserError".to_string())),
            },
            ("Browser", "subscribe" | "close") => Type::Result {
                ok: Box::new(unit_type()),
                err: Box::new(Type::Named("BrowserError".to_string())),
            },
            ("Browser", "next_event") => Type::Result {
                ok: Box::new(Type::Named("BrowserEvent".to_string())),
                err: Box::new(Type::Named("BrowserError".to_string())),
            },
            ("Browser", "add_intercept" | "add_intercept_url") => Type::Result {
                ok: Box::new(Type::Named("BrowserIntercept".to_string())),
                err: Box::new(Type::Named("BrowserError".to_string())),
            },
            ("Browser", "continue_request" | "fail_request" | "fulfill_request"
                | "allow_downloads") => Type::Result {
                ok: Box::new(unit_type()),
                err: Box::new(Type::Named("BrowserError".to_string())),
            },
            ("Browser", "protocol") => Type::Result {
                ok: Box::new(Type::Named("BrowserProtocol".to_string())),
                err: Box::new(Type::Named("BrowserError".to_string())),
            },
            ("Browser", "trace") => Type::Named("BrowserTrace".to_string()),
            ("Browser", "privacy") => Type::Named("BrowserPrivacy".to_string()),
            ("Browser", "receipt") => Type::Named("BrowserReceipt".to_string()),
            ("BrowserContext", "page" | "tab") => Type::Result {
                ok: Box::new(Type::Named("BrowserPage".to_string())),
                err: Box::new(Type::Named("BrowserError".to_string())),
            },
            ("BrowserContext", "isolated") => Type::Bool,
            ("BrowserContext", "user_hash") => Type::String,
            ("BrowserContext", "close")
            | ("BrowserPage", "goto" | "close" | "clear_cookies" | "set_cookie" | "storage_set"
                | "storage_clear")
            | ("BrowserFrame", "close")
            | ("BrowserIntercept", "remove")
            | ("BrowserLocator", "wait" | "wait_gone" | "click" | "hover" | "fill" | "press"
                | "set_files") => {
                Type::Result {
                    ok: Box::new(unit_type()),
                    err: Box::new(Type::Named("BrowserError".to_string())),
                }
            }
            ("BrowserPage", "screenshot" | "pdf") => Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::Named("BrowserError".to_string())),
            },
            ("BrowserPage", "cookie" | "storage_get") => Type::Result {
                ok: Box::new(Type::Option(Box::new(Type::String))),
                err: Box::new(Type::Named("BrowserError".to_string())),
            },
            ("BrowserPage", "main_frame") => Type::Result {
                ok: Box::new(Type::Named("BrowserFrame".to_string())),
                err: Box::new(Type::Named("BrowserError".to_string())),
            },
            ("BrowserPage", "frames") => Type::Result {
                ok: Box::new(Type::List(Box::new(Type::Named("BrowserFrame".to_string())))),
                err: Box::new(Type::Named("BrowserError".to_string())),
            },
            ("BrowserPage", "get_by_role" | "get_by_text" | "get_by_label" | "get_by_placeholder"
                | "get_by_test_id" | "get_by_css") => Type::Named("BrowserLocator".to_string()),
            ("BrowserEvent", "kind" | "request_id" | "request_method" | "url_hash"
                | "download_id" | "suggested_filename_hash")
            | ("BrowserCapabilities", "profile")
            | ("BrowserTrace", "summary")
            | ("BrowserReceipt", "summary")
            | ("BrowserLocked", "engine" | "version" | "binary" | "protocol") => Type::String,
            ("BrowserEvent", "status_code") => Type::Int,
            ("BrowserProtocol", "send") => Type::Result {
                ok: Box::new(Type::String),
                err: Box::new(Type::Named("BrowserError".to_string())),
            },
            ("BrowserEvent", "is_blocked")
            | ("BrowserCapabilities", "bidi" | "cdp")
            | ("BrowserTrace", "redacted")
            | ("BrowserReceipt", "redacted" | "isolated" | "cleaned")
            | ("BrowserPrivacy", "isolated_profiles" | "redact_receipts" | "shared_profiles") => {
                Type::Bool
            }
            ("BrowserTrace", "entry_count") | ("BrowserReceipt", "entry_count") => Type::Int,
            ("BrowserLocked", "verify") => Type::Result {
                ok: Box::new(unit_type()),
                err: Box::new(Type::Named("BrowserError".to_string())),
            },
            _ => unit_type(),
        };
        let targs: Vec<TExpr> = args
            .iter()
            .enumerate()
            .map(|(i, a)| {
                if kind == "HTTPMux"
                    && matches!(
                        method,
                        "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
                    )
                    && i == 1
                {
                    if let Expr::Lambda(lam) = &a.expr {
                        let params = vec![Type::Named("HTTPRequest".to_string())];
                        let ret = Type::Result {
                            ok: Box::new(Type::Named("HTTPResponse".to_string())),
                            err: Box::new(Type::Named("HTTPError".to_string())),
                        };
                        return TExpr {
                            ty: Type::Fn {
                                params: params.clone(),
                                ret: Some(Box::new(ret)),
                                effect_bound: None, return_view_provenance: None,
                                param_contract: None,
                call_metadata: None,
                            },
                            kind: TExprKind::Lambda(Box::new(lower_lambda_expecting_value(
                                lam, cx, env, &params,
                            ))),
                        };
                    }
                }
                lower_expr(&a.expr, cx, env)
            })
            .collect();
        let server_message_method = matches!(
            (kind.as_str(), method, args.len()),
            ("HTTPRequest", "method" | "path" | "param" | "body_len" | "under_limit", _)
                | ("HTTPRequest", "header", 1)
                | ("HTTPRequest", "body", 0)
                | ("HTTPRequest", "json", 0)
                | ("HTTPRequest", "trailers", 0)
                | ("HTTPResponse", "header", 2)
                | ("HTTPResponse", "trailers", 1)
        );
        let op = if kind.starts_with("HTTPServer")
            || kind == "HTTPMux"
            || kind == "HTTPHandler"
            || kind == "WsConn"
            || kind == "WsMessage"
            || matches!(
                kind.as_str(),
                "Browser"
                    | "BrowserContext"
                    | "BrowserPage"
                    | "BrowserFrame"
                    | "BrowserLocator"
                    | "BrowserIntercept"
                    | "BrowserEvent"
                    | "BrowserTrace"
                    | "BrowserReceipt"
                    | "BrowserPrivacy"
                    | "BrowserCapabilities"
                    | "BrowserProtocol"
                    | "BrowserLocked"
            )
            || server_message_method
        {
            THandleOp::HTTPServerMethod {
                kind,
                method: method.to_string(),
            }
        } else {
            THandleOp::HTTPClientMethod {
                kind,
                method: method.to_string(),
            }
        };
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op,
                args: targs,
            },
        };
    }
    // D-TIMEDEPTH1=A: a civil-time method (gate shape d9).
    if matches!(
        recv_type.as_deref(),
        Some(
            "Date"
                | "LocalDate"
                | "LocalTime"
                | "DateTime"
                | "Instant"
                | "Period"
                | "Zone"
                | "ZonedDateTime"
        )
    ) && is_civil_time_method_name(recv_type.as_deref(), method)
    {
        let kind = recv_type.as_deref().unwrap_or("Date").to_string();
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match (kind.as_str(), method) {
            ("Date" | "LocalDate", "year")
            | ("Date" | "LocalDate", "month")
            | ("Date" | "LocalDate", "day")
            | ("Date" | "LocalDate", "diff_days")
            | ("Date" | "LocalDate", "weekday")
            | ("Date" | "LocalDate", "iso_weekday")
            | ("Date" | "LocalDate", "day_of_year")
            | ("Date" | "LocalDate", "iso_week")
            | ("Date" | "LocalDate", "quarter_of_year")
            | ("Date" | "LocalDate", "days_in_month") => Type::Int,
            ("Date" | "LocalDate", "is_leap_year") => Type::Bool,
            ("Date" | "LocalDate", "add_days" | "add_months" | "add_period" | "truncate" | "replace") => {
                Type::Named("LocalDate".to_string())
            }
            ("Date" | "LocalDate", "to_string" | "format") => Type::String,
            ("LocalTime", "hour" | "minute" | "second") => Type::Int,
            ("LocalTime", "to_string") => Type::String,
            ("DateTime", "hour")
            | ("DateTime", "minute")
            | ("DateTime", "second")
            | ("DateTime", "millisecond")
            | ("DateTime", "microsecond")
            | ("DateTime", "nanosecond")
            | ("DateTime", "to_timestamp")
            | ("DateTime", "to_unix_ms") => Type::Int,
            ("DateTime", "date") => Type::Named("LocalDate".to_string()),
            ("DateTime", "time") => Type::Named("LocalTime".to_string()),
            ("DateTime", "plus_duration" | "truncate" | "round" | "floor" | "ceil" | "replace") => {
                Type::Named("DateTime".to_string())
            }
            ("DateTime", "difference") => Type::Named("Duration".to_string()),
            ("DateTime", "in_zone") => Type::Named("ZonedDateTime".to_string()),
            ("DateTime", "to_string" | "format_rfc3339" | "format") => Type::String,
            ("Instant", "elapsed_millis") => Type::Int,
            ("Instant", "elapsed") => Type::Named("Duration".to_string()),
            ("Period", "to_string") => Type::String,
            ("Zone", "name") => Type::String,
            ("ZonedDateTime", "date") => Type::Named("LocalDate".to_string()),
            ("ZonedDateTime", "time") => Type::Named("LocalTime".to_string()),
            ("ZonedDateTime", "offset_seconds") => Type::Int,
            ("ZonedDateTime", "is_dst") => Type::Bool,
            ("ZonedDateTime", "to_datetime") => Type::Named("DateTime".to_string()),
            ("ZonedDateTime", "zone") => Type::Named("Zone".to_string()),
            ("ZonedDateTime", "add_duration" | "add_period") => {
                Type::Named("ZonedDateTime".to_string())
            }
            ("ZonedDateTime", "to_string" | "format") => Type::String,
            _ => unit_type(),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::CivilTimeMethod {
                    kind,
                    method: method.to_string(),
                },
                args: targs,
            },
        };
    }
    // D-APPROX1=A: a sketch method (gate shape d8).
    if is_sketch_type(recv_type.as_deref()) && is_sketch_method_name(recv_type.as_deref(), method) {
        let sketch = recv_type.as_deref().unwrap_or("").to_string();
        let recv_t = lower_expr(receiver, cx, env);
        let result_ty = match (sketch.as_str(), method) {
            ("HyperLogLog", "add")
            | ("TDigest", "add")
            | ("CountMinSketch", "add")
            | ("ReservoirSampler", "add") => unit_type(),
            ("HyperLogLog", "count") | ("CountMinSketch", "count") => Type::Int,
            ("TDigest", "quantile") => Type::Float,
            ("ReservoirSampler", "sample") => Type::List(Box::new(Type::String)),
            _ => unit_type(),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::SketchMethod {
                    sketch,
                    method: method.to_string(),
                },
                args: targs,
            },
        };
    }
    // c109 Phase 21 / D-TUPLE-DESTRUCT1: a Task/Receiver/Sender concurrency method
    // (gate shape d3). The gate proved `recv_type == None` + a disjoint concurrency
    // name+arity. Resolve the op + result type HERE (totality). The result type comes
    // from `Collections::builtin_method_return`'s `Type::Apply` arms
    // (Source/Collections.rs), read off the receiver's already-resolved type
    // `Task<T>`/`Receiver<T>`/`Sender<T>` (the LOWERED receiver's `.ty`, total from the
    // binding's annotated/inferred slot — never re-inferred in emit, I3): `join`
    // → `T ? TaskFailure`; `detach`/`pause`/`resume`/`cancel`/`send` → Unit;
    // `receive` → `Result<T, Closed>`. Args lowered PLAINLY (the AST
    // `emit_builtin_method`'s `arg(i)` is a raw `emit_expr`).
    if recv_type.is_none() && is_concurrency_method_name(method, args.len()) {
        let recv_t = lower_expr(receiver, cx, env);
        // The element type `T` from the receiver's `Apply<T>` (the first type arg).
        let elem = match &recv_t.ty {
            Type::Apply { args, .. } => args.first().cloned(),
            _ => None,
        };
        let elem = elem.unwrap_or_else(unit_type);
        let (op, ty) = match method {
            "join" => (
                THandleOp::TaskJoin,
                resolved_ret.cloned().unwrap_or_else(|| Type::Result {
                    ok: Box::new(elem),
                    err: Box::new(Type::Named(crate::Syntax::TYPE_TASK_FAILURE.to_string())),
                }),
            ),
            "detach" => (THandleOp::TaskDetach, unit_type()),
            "pause" => (THandleOp::TaskPause, unit_type()),
            "resume" => (THandleOp::TaskResume, unit_type()),
            "cancel" => (THandleOp::TaskCancel, unit_type()),
            "receive" => (
                THandleOp::ChannelReceive,
                Type::Result {
                    ok: Box::new(elem),
                    err: Box::new(Type::Named("Closed".to_string())),
                },
            ),
            "close" => (THandleOp::ChannelClose, unit_type()),
            "send" => (THandleOp::SenderSend, unit_type()),
            _ => unreachable!("is_concurrency_method_name admitted only these names"),
        };
        let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
        return TExpr {
            ty,
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op,
                args: targs,
            },
        };
    }
    // D-MEM1 S6 (D-POOLID-API1=A / D-SHARED-API1=A): `Pool<T>.add/remove/ids`
    // and `Shared<T>.read/edit`. Both lower to a PLAIN Rust method call on the
    // receiver (`(recv).add(val)`, …) — `add`/`remove`/`ids` are genuine
    // inherent methods on `JetPool<T>` and `read`/`edit` on `JetShared<T>`
    // (Prelude/CoreLib.rs), so there's no free-function indirection to model
    // (unlike `Pool`/`Shared` INDEXING, which needs a real mutable-place
    // helper — see the `Expr::Index`/`LValue::Field` arms). `ConstInline` is
    // the pragmatic vehicle, same as the concurrency/SQL/HTML escapes nearby.
    {
        let recv_peek = tir_recv_jet_ty(receiver, env);
        // Sema normally sets `recv_type` to `"Pool"`/`"Shared"` explicitly for
        // these calls. Comptime fragments can omit that name while retaining the
        // resolved receiver type, so recognize `Pool<T>` from either source.
        let is_pool = recv_type.as_deref() == Some("Pool")
            || matches!(
                &recv_peek,
                Some(Type::Apply { name, .. }) if name == "Pool"
            );
        let is_shared = recv_type.as_deref() == Some("Shared");
        let cell_receiver = recv_type
            .as_deref()
            .filter(|name| {
                matches!(
                    *name,
                    "Cell" | "CellReadGuard" | "CellEditGuard"
                )
            });
        let is_expiring_secret = recv_type.as_deref() == Some("ExpiringSecret");
        if is_pool && matches!(method, "add" | "remove" | "ids") && args.len() <= 1 {
            let recv_t = lower_expr(receiver, cx, env);
            let elem = match &recv_t.ty {
                Type::Apply { args, .. } if !args.is_empty() => args[0].clone(),
                _ => Type::Int,
            };
            let id_ty = Type::Apply {
                name: "Id".to_string(),
                args: vec![elem.clone()],
            };
            let ty = match method {
                "add" => id_ty,
                "remove" => Type::Option(Box::new(elem)),
                "ids" => Type::List(Box::new(id_ty)),
                _ => unreachable!("matches! above admitted only these"),
            };
            let targs: Vec<TExpr> = args
                .iter()
                .map(|a| lower_expr(&a.expr, cx, env))
                .collect();
            return TExpr {
                ty,
                kind: TExprKind::HostCall(Box::new(THostCall::Method {
                    recv: Box::new(recv_t),
                    method: method.to_string(),
                    args: targs,
                })),
            };
        }
        if is_shared && matches!(method, "read" | "edit") && args.len() == 1 {
            let inner = match &recv_peek {
                Some(Type::Shared(inner)) => (**inner).clone(),
                _ => Type::Int,
            };
            let recv_t = lower_expr(receiver, cx, env);
            let Expr::Lambda(lam) = &args[0].expr else {
                unreachable!("sema's finish_shared_read/finish_shared_edit require a lambda arg");
            };
            let expected = std::slice::from_ref(&inner);
            // `JetShared::read`/`edit` lend `&T`/`&mut T` directly. This host
            // borrow is not an unmarked function-value parameter and must not
            // receive another D-MEM-PARAM1 Read borrow.
            let mut tl = lower_lambda_expecting_host_borrow(
                lam,
                cx,
                env,
                expected,
                method == "edit",
            );
            // D-STM1=A (card #506): a `Shared<T>.edit(f)` inside a `#Transact` block
            // routes to the deferred `edit_txn` — the write is buffered and applied
            // atomically at the block's commit, so the call yields nothing (Unit; E0750
            // rejects a value-producing edit here). The closure is stored past the call,
            // so it must `move` its captures. A `.read` (or an `.edit` outside a
            // transaction) is unchanged.
            let (method_out, ty) =
                if method == "edit" && cx.in_stm_transact.get() {
                    cx.stm_touched.set(true);
                    tl.is_move = true;
                    ("edit_txn", Type::Tuple(vec![]))
                } else {
                    (
                        method,
                        lambda_body_ty_expecting(lam, cx, env, Some(expected)),
                    )
                };
            return TExpr {
                ty,
                kind: TExprKind::HostCall(Box::new(THostCall::Method {
                    recv: Box::new(recv_t),
                    method: method_out.to_string(),
                    args: vec![TExpr {
                        ty: Type::Fn {
                            params: vec![inner],
                            ret: None,
                            effect_bound: None, return_view_provenance: None,
                            param_contract: None,
                call_metadata: None,
                        },
                        kind: TExprKind::Lambda(Box::new(tl)),
                    }],
                })),
            };
        }
        if is_shared && matches!(method, "guard_read" | "guard_edit") && args.is_empty() {
            let inner = match &recv_peek {
                Some(Type::Shared(inner)) => (**inner).clone(),
                _ => Type::Int,
            };
            let marker = if method == "guard_edit" {
                crate::AST::InternalTag::SharedGuardEdit
            } else {
                crate::AST::InternalTag::SharedGuardRead
            };
            let ty = resolved_ret.cloned().unwrap_or_else(|| Type::Tagged {
                marker: crate::AST::TagMarker::Internal(marker),
                inner: Box::new(Type::Apply {
                    name: Syntax::TYPE_SHARED_GUARD.to_string(),
                    args: vec![inner],
                }),
            });
            return TExpr {
                ty,
                kind: TExprKind::HostCall(Box::new(THostCall::Method {
                    recv: Box::new(lower_expr(receiver, cx, env)),
                    method: method.to_string(),
                    args: Vec::new(),
                })),
            };
        }
        // D-SHARED-CYCLE1=C: Shared.downgrade / strong_count → inherent JetShared methods.
        if is_shared && matches!(method, "downgrade" | "strong_count") && args.is_empty() {
            let inner = match &recv_peek {
                Some(Type::Shared(inner)) => (**inner).clone(),
                _ => Type::Int,
            };
            let ty = resolved_ret.cloned().unwrap_or_else(|| {
                if method == "strong_count" {
                    Type::Int
                } else {
                    Type::Apply {
                        name: Syntax::TYPE_SHARED_WEAK.to_string(),
                        args: vec![inner],
                    }
                }
            });
            return TExpr {
                ty,
                kind: TExprKind::HostCall(Box::new(THostCall::Method {
                    recv: Box::new(lower_expr(receiver, cx, env)),
                    method: method.to_string(),
                    args: Vec::new(),
                })),
            };
        }
        let is_shared_weak = recv_type.as_deref() == Some(Syntax::TYPE_SHARED_WEAK)
            || matches!(
                &recv_peek,
                Some(Type::Apply { name, .. }) if name == Syntax::TYPE_SHARED_WEAK
            );
        if is_shared_weak && method == "upgrade" && args.is_empty() {
            let inner = match &recv_peek {
                Some(Type::Apply { args, .. }) if !args.is_empty() => args[0].clone(),
                _ => Type::Int,
            };
            let ty = resolved_ret.cloned().unwrap_or_else(|| {
                Type::Option(Box::new(Type::Shared(Box::new(inner))))
            });
            return TExpr {
                ty,
                kind: TExprKind::HostCall(Box::new(THostCall::Method {
                    recv: Box::new(lower_expr(receiver, cx, env)),
                    method: method.to_string(),
                    args: Vec::new(),
                })),
            };
        }
        if let Some(cell_receiver) = cell_receiver {
            let recv_t = lower_expr(receiver, cx, env);
            let inner = match &recv_t.ty {
                Type::Apply { args, .. } if !args.is_empty() => args[0].clone(),
                _ => Type::Int,
            };
            if matches!(cell_receiver, "CellReadGuard" | "CellEditGuard")
                && matches!((method, args.len()), ("map", 1) | ("split", 2))
            {
                let mut paths = Vec::with_capacity(args.len());
                for arg in args {
                    let Expr::Lambda(lambda) = &arg.expr else {
                        unreachable!("sema requires projection lambdas for Cell guard map/split");
                    };
                    let path = lambda
                        .meta
                        .cell_projection_path
                        .as_ref()
                        .expect("sema records every Cell guard projection path");
                    paths.push(path.clone());
                }
                let ty = resolved_ret
                    .cloned()
                    .expect("sema persists exact Cell guard projection return type");
                debug_assert!(method != "split" || matches!(ty, Type::Tuple(_)));
                return TExpr {
                    ty,
                    kind: TExprKind::HostCall(Box::new(THostCall::CellGuardProject {
                        recv: Box::new(recv_t),
                        paths,
                        result_ty: resolved_ret
                            .cloned()
                            .expect("sema persists exact Cell guard projection return type"),
                        editable: cell_receiver == "CellEditGuard",
                        edit_paths_disjoint: cell_receiver == "CellEditGuard" && method == "split",
                    })),
                };
            }
            if matches!(
                (cell_receiver, method, args.len()),
                ("Cell", "get" | "guard_read" | "guard_edit", 0)
                    | ("Cell", "set" | "replace", 1)
                    | ("CellReadGuard" | "CellEditGuard", "get", 0)
                    | ("CellEditGuard", "set", 1)
            ) {
                let ty = resolved_ret.cloned().unwrap_or_else(unit_type);
                let targs = args
                    .iter()
                    .map(|arg| lower_expr(&arg.expr, cx, env))
                    .collect();
                return TExpr {
                    ty,
                    kind: TExprKind::HostCall(Box::new(THostCall::Method {
                        recv: Box::new(recv_t),
                        method: method.to_string(),
                        args: targs,
                    })),
                };
            }
            if matches!(
                (cell_receiver, method),
                ("Cell", "read" | "edit")
                    | ("CellReadGuard", "read")
                    | ("CellEditGuard", "read" | "edit")
            ) && args.len() == 1
            {
                let Expr::Lambda(lambda) = &args[0].expr else {
                    unreachable!("sema requires a lambda for Cell read/edit");
                };
                let expected = std::slice::from_ref(&inner);
                let write = method == "edit";
                let lowered =
                    lower_lambda_expecting_host_borrow(lambda, cx, env, expected, write);
                let ty = resolved_ret.cloned().unwrap_or_else(|| {
                    lambda_body_ty_expecting(lambda, cx, env, Some(expected))
                });
                return TExpr {
                    ty: ty.clone(),
                    kind: TExprKind::HostCall(Box::new(THostCall::Method {
                        recv: Box::new(recv_t),
                        method: method.to_string(),
                        args: vec![TExpr {
                            ty: Type::Fn {
                                params: vec![inner],
                                ret: Some(Box::new(ty)),
                                effect_bound: None,
                                param_contract: None,
                call_metadata: None,
                                return_view_provenance: None,
                            },
                            kind: TExprKind::Lambda(Box::new(lowered)),
                        }],
                    })),
                };
            }
            if cell_receiver == "Cell" && method == "get_or_set" && args.len() == 1 {
                let Expr::Lambda(lambda) = &args[0].expr else {
                    unreachable!("sema requires a lambda for Cell.get_or_set");
                };
                let value_ty = resolved_ret.cloned().unwrap_or_else(|| match inner {
                    Type::Option(value) => *value,
                    other => other,
                });
                let lowered = lower_lambda(lambda, cx, env);
                return TExpr {
                    ty: value_ty.clone(),
                    kind: TExprKind::HostCall(Box::new(THostCall::Method {
                        recv: Box::new(recv_t),
                        method: method.to_string(),
                        args: vec![TExpr {
                            ty: Type::Fn {
                                params: vec![],
                                ret: Some(Box::new(value_ty.clone())),
                                effect_bound: None,
                                param_contract: None,
                call_metadata: None,
                                return_view_provenance: None,
                            },
                            kind: TExprKind::Lambda(Box::new(lowered)),
                        }],
                    })),
                };
            }
        }
        if is_expiring_secret && method == "with" && args.len() == 1 {
            let mut recv_shape = recv_peek.as_ref();
            while let Some(Type::Tagged { inner, .. }) = recv_shape {
                recv_shape = Some(inner.as_ref());
            }
            let inner = match recv_shape {
                Some(Type::Apply { name, args })
                    if name == "ExpiringSecret" && !args.is_empty() =>
                {
                    args[0].clone()
                }
                _ => Type::Int,
            };
            let recv_t = lower_expr(receiver, cx, env);
            let Expr::Lambda(lam) = &args[0].expr else {
                unreachable!("sema requires a lambda for ExpiringSecret.with");
            };
            let expected = std::slice::from_ref(&inner);
            let tl = lower_lambda_expecting_host_borrow(lam, cx, env, expected, false);
            let ty = resolved_ret.cloned().unwrap_or_else(|| Type::Result {
                ok: Box::new(lambda_body_ty_expecting(lam, cx, env, Some(expected))),
                err: Box::new(Type::Named("Expired".to_string())),
            });
            return TExpr {
                ty,
                kind: TExprKind::HostCall(Box::new(THostCall::Method {
                    recv: Box::new(recv_t),
                    method: "with".to_string(),
                    args: vec![TExpr {
                        ty: Type::Fn {
                            params: vec![inner],
                            ret: None,
                            effect_bound: None, return_view_provenance: None,
                            param_contract: None,
                call_metadata: None,
                        },
                        kind: TExprKind::Lambda(Box::new(tl)),
                    }],
                })),
            };
        }
    }
    // c109 Phase 11: a closure-taking collection method (`map`/`filter`/`each`/…).
    // The gate proved `recv_type == None` + a closure-method name + a literal lambda
    // arg. Resolve the receiver-type + Fn-vs-FnMut dispatch HERE into a total
    // `TClosureOp` (reproducing `emit_builtin_method`'s closure arms, incl. its
    // `expr_jet_ty(receiver)` Map/trait-object branches), so emit makes no decision.
    // SIMD/linalg `.reduce(.Op)` is MathMethod (below), not a collection fold —
    // skip when the lowered receiver is a math value type.
    if recv_type.is_none() && crate::Collections::is_closure_method(method) {
        let recv_t = lower_expr(receiver, cx, env);
        let recv_ast_ty = tir_recv_jet_ty(receiver, env);
        let recv_ty = recv_ast_ty.unwrap_or_else(|| recv_t.ty.clone());
        // #1478: Set/SortedSet closures (filter/map/each/all/fold/flat_map)
        // route through the same to_list()-then-List path every other
        // container's closures already use (I9 — AOT and JIT both need a
        // real `Vec`-backed list, not a raw `HashSet`/`BTreeSet`).
        let (recv_t, recv_ty) = if matches!(
            &recv_ty,
            Type::Apply { name, .. } if name == "Set" || name == crate::Syntax::TYPE_SORTED_SET
        ) {
            let wrapped = crate::Codegen::TIR::wrap_set_receiver_as_list(recv_t);
            let ty = wrapped.ty.clone();
            (wrapped, ty)
        } else {
            (recv_t, recv_ty)
        };
        // A `ReduceOp` value on SIMD is MathMethod, never collection fold.
        let reduce_value = method == "reduce"
            && args
                .first()
                .and_then(|argument| reduce_op_name(&argument.expr))
                .is_some();
        let skip_closure = reduce_value
            || (method == "find" && is_fragment_build_context(receiver, cx))
            || (super::is_eval_fragment()
                && matches!(
                    &recv_ty,
                    Type::Named(name) if name == crate::Syntax::TYPE_BUILD_CONTEXT
                ))
            || matches!(
                &recv_ty,
                Type::Named(name)
                    if crate::Sema::is_math_type(name) && !cx.type_names.contains(name)
            );
        if skip_closure {
            if let Type::Named(handle) = &recv_ty {
                let is_reduce = method == "reduce"
                    && (crate::Sema::is_simd_lane_type(handle) || reduce_value);
                if is_reduce
                    || crate::Sema::math_method_return(handle, method, args.len()).is_some()
                {
                    let (reduce_op, value_args): (Option<String>, Vec<TExpr>) = if is_reduce {
                        let op = args
                            .first()
                            .and_then(|argument| reduce_op_name(&argument.expr))
                            .unwrap_or_else(|| "Add".to_string());
                        (Some(op), Vec::new())
                    } else {
                        (
                            None,
                            args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect(),
                        )
                    };
                    let ty = if is_reduce {
                        crate::Sema::math_scalar_ty(handle)
                    } else {
                        crate::Sema::math_method_return(handle, method, args.len())
                            .unwrap_or_else(unit_type)
                    };
                    return TExpr {
                        ty,
                        kind: TExprKind::HandleMethod {
                            recv: Box::new(recv_t),
                            op: THandleOp::MathMethod {
                                type_name: handle.clone(),
                                method: method.to_string(),
                                reduce_op,
                            },
                            args: value_args,
                        },
                    };
                }
            }
        }
        if !skip_closure {
        let op = resolve_closure_op(&recv_ty, method, args, cx);
        let result_ty = resolved_ret
            .cloned()
            .unwrap_or_else(|| builtin_result_ty(method, args.len(), Some(&recv_ty)));
        // Collection helpers lend callback inputs (`&T`, or `&U, &T` for
        // folds). Lower that host borrow exactly once, including scalar
        // payloads. `Option.map` emits through `.as_ref()` for the same law.
        // `tir_recv_jet_ty` intentionally returns `None` for literals, while the
        // lowered receiver still carries their resolved type. Use that total type
        // for the helper's borrowed callback convention as well.
        let callback_recv_ty = match &recv_ty {
            Type::FixedList { elem, .. } => Type::List(elem.clone()),
            _ => recv_ty.clone(),
        };
        let mut callback_params =
            crate::Collections::builtin_method_arg_types(&callback_recv_ty, method)
            .and_then(|types| {
                types.into_iter().find_map(|ty| match ty {
                    Type::Fn { params, .. } => Some(params),
                    _ => None,
                })
            });
        if method == "sort_by"
            && matches!(
                args.first().map(|arg| &arg.expr),
                Some(Expr::Lambda(lambda)) if lambda.params.len() == 2
            )
        {
            if let Type::List(inner) | Type::FixedList { elem: inner, .. } = &callback_recv_ty {
                callback_params = Some(vec![(**inner).clone(), (**inner).clone()]);
            }
        }
        if matches!(method, "reduce" | "fold" | "scan") {
            if let Some(seed_ty) = args.first().map(|arg| lower_expr(&arg.expr, cx, env).ty) {
                if let Some(first) = callback_params
                    .as_mut()
                    .and_then(|params| params.first_mut())
                {
                    *first = seed_ty;
                }
            }
        }
        let para_fold_params = if method == "para_fold" {
            let acc = resolved_ret.cloned().unwrap_or(Type::Int);
            let item = match &recv_ty {
                Type::List(inner) | Type::FixedList { elem: inner, .. } => (**inner).clone(),
                _ => Type::Int,
            };
            Some(vec![vec![], vec![acc.clone(), item], vec![acc.clone(), acc]])
        } else {
            None
        };
        let targs = args
            .iter()
            .enumerate()
            .map(|(index, a)| {
                let params = para_fold_params
                    .as_ref()
                    .and_then(|all| all.get(index))
                    .or(callback_params.as_ref());
                if let (Expr::Lambda(lam), Some(params)) = (&a.expr, params) {
                    let tl = if method == "edit_disjoint" {
                        crate::Codegen::TIR::lower_lambda_expecting_value(lam, cx, env, params)
                    } else {
                        lower_lambda_expecting_host_borrow(lam, cx, env, params, false)
                    };
                    return TExpr {
                        ty: Type::Fn {
                            params: params.clone(),
                            ret: None,
                            effect_bound: None, return_view_provenance: None,
                            param_contract: None,
                call_metadata: None,
                        },
                        kind: TExprKind::Lambda(Box::new(tl)),
                    };
                }
                if method.starts_with("para_") {
                    if let Some(params) = params {
                        let callable = lower_expr(&a.expr, cx, env);
                        return TExpr {
                            ty: callable.ty.clone(),
                            kind: TExprKind::HostBorrowCallback {
                                callable: Box::new(callable),
                                params: params.clone(),
                            },
                        };
                    }
                }
                lower_expr(&a.expr, cx, env)
            })
            .collect();
        return TExpr {
            ty: result_ty,
            kind: TExprKind::ClosureMethod {
                recv: Box::new(recv_t),
                op,
                args: targs,
            },
        };
        }
    }
    // D-NUMWIDEN-CROSS1=E / card #1662: sema owns the checked-crossing
    // decision and records it in `Expr::MethodCall::checked_widen` (replaces
    // the retired `\0numeric.checked_widen` fake-`recv_type` marker).
    // Lowering only records adapter facts.
    if checked_widen && args.len() == 1 {
        let source = lower_expr(&args[0].expr, cx, env);
        let source_signed = !matches!(source.ty, Type::IntN { signed: false, .. });
        let target = resolved_ret.cloned().unwrap_or(Type::Float);
        return TExpr {
            ty: target.clone(),
            kind: TExprKind::NumericMethod {
                recv: Box::new(source),
                op: TNumericOp::CheckedIntToFloat {
                    source_signed,
                    target_f32: target == Type::Float32,
                    line: crate::Diagnostics::span_line_col(&cx.src, method_span.start).0 as u32,
                },
            },
        };
    }

    // c109 Phase 12: a numeric predicate / bit-pop query
    // (`is_nan`/`count_ones`/…). The gate proved `recv_type ==
    // Some(<numeric name>)` + a covered nullary numeric op. Resolve the receiver
    // operation HERE into a total `TNumericOp`, so emit makes no decision (I3).
    // The result type comes from
    // `numeric_method_return` (the sema table), keyed on the receiver type recovered
    // from `recv_type` (the total width source — `src = recv_type.or_else(rty.name())`
    // on the AST side, where `recv_type` is always `Some` for these).
    if let Some(numeric_name) = recv_type {
        if let Some(recv_ty) = crate::AST::numeric_type_from_name(numeric_name) {
            // `origin` needs the lowered receiver's binding metadata, so complete
            // that payload here instead of letting a tier rediscover it.
            let resolved_op = resolve_numeric_op(method, numeric_name);
            if method == "origin" || resolved_op.is_some() {
                let mut recv_t = lower_expr(receiver, cx, env);
                // Sema's width is authoritative — Call/OrFallback lowering can
                // fall back to Unit/Int and would silently widen bit queries.
                recv_t.ty = recv_ty.clone();
                let op = match resolved_op {
                    Some(op) => op,
                    None => TNumericOp::Origin {
                        origin: recv_t
                            .binding_origin()
                            .map(|origin| {
                                crate::Codegen::TIR::lower::render_tracked_float_origin(
                                    origin, cx,
                                )
                            })
                            .unwrap_or_else(|| "untracked".to_string()),
                    },
                };
                let result_ty = builtin_result_ty(method, args.len(), Some(&recv_ty));
                return TExpr {
                    ty: result_ty,
                    kind: TExprKind::NumericMethod {
                        recv: Box::new(recv_t),
                        op,
                    },
                };
            }
        }
    }
    // Core enum equality is represented by the shared Prelude's native
    // `PartialEq` value operation. Sema has already proved the Equatable
    // contract; preserve it as the existing typed equality node so emitters do
    // not rediscover the representation.
    if method == "equal"
        && args.len() == 1
        && recv_type.as_deref().is_some_and(|name| {
            core_enum_equal_type(name.rsplit('.').next().unwrap_or(name))
        })
    {
        let lhs = lower_expr(receiver, cx, env);
        let rhs = lower_expr(&args[0].expr, cx, env);
        return TExpr {
            ty: resolved_ret.cloned().unwrap_or(Type::Bool),
            kind: TExprKind::Binary {
                op: crate::AST::BinOp::Eq,
                overflow: false,
                line: crate::Diagnostics::span_line_col(&cx.src, method_span.start).0 as u32,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            },
        };
    }
    // c109 Phase 25: HTTPRouter route registration `router.get/post/put/delete(path,
    // handler)` (D-ROUTE1=A). The gate (`router_register_in_subset`) proved the receiver
    // + path in-subset and the handler a named-fn/lambda. Render the handler closure HERE
    // (the `emit_router_handler` reproduction); emit assembles the register call. Result
    // is Unit (the registration is a statement effect).
    if recv_type.as_deref() == Some("HTTPRouter")
        && matches!(method, "get" | "post" | "put" | "delete")
        && args.len() == 2
    {
        let verb = match method {
            "get" => "GET",
            "post" => "POST",
            "put" => "PUT",
            "delete" => "DELETE",
            _ => unreachable!(),
        };
        let recv_t = lower_expr(receiver, cx, env);
        let path_t = lower_expr(&args[0].expr, cx, env);
        let handler = render_router_handler(args, cx, env);
        let line = crate::Diagnostics::span_line_col(&cx.src, method_span.start).0;
        return TExpr {
            ty: unit_type(),
            kind: TExprKind::HandleMethod {
                recv: Box::new(recv_t),
                op: THandleOp::HTTPRouterRegister {
                    verb,
                    handler,
                    file: cx.file.clone(),
                    line,
                },
                args: vec![path_t],
            },
        };
    }
    // D-BIGINT1 / D-DECIMAL1: instance methods on precise numerics → the same
    // `PreciseBuiltin` nodes as constructors/binops (emit uses jet_bigint_* /
    // jet_decimal_*). Fragment lowers (empty Cx) never see method_sigs, so this
    // must not fall through to the user-method Todo path.
    if let Some(handle) = recv_type {
        if (handle == Syntax::TYPE_BIGINT
            || handle == Syntax::TYPE_DECIMAL
            || handle == Syntax::TYPE_FRACTION)
            && !cx.type_names.contains(handle)
        {
            let known = matches!(
                (handle.as_str(), method, args.len()),
                ("BigInt", "add" | "sub" | "mul", 1)
                    | ("BigInt", "neg" | "to_string", 0)
                    | ("Decimal", "add" | "sub" | "mul", 1)
                    | ("Decimal", "to_string", 0)
                    | ("Fraction", "add" | "sub" | "mul" | "div" | "equal", 1)
                    | ("Fraction", "numerator" | "denominator" | "to_string" | "to_float" | "is_zero", 0)
            );
            if known {
                let recv_t = lower_expr(receiver, cx, env);
                let mut value_args = vec![recv_t];
                value_args.extend(args.iter().map(|a| lower_expr(&a.expr, cx, env)));
                let ty = match method {
                    "to_string" => Type::String,
                    "numerator" | "denominator" => Type::Int,
                    "to_float" => Type::Float,
                    "is_zero" | "equal" => Type::Bool,
                    _ => Type::Named(handle.clone()),
                };
                return TExpr {
                    ty: resolved_ret.cloned().unwrap_or(ty),
                    kind: TExprKind::PreciseBuiltin {
                        type_name: handle.clone(),
                        func: method.to_string(),
                        args: value_args,
                    },
                };
            }
        }
    }
    // c109 Phase 13: a method ON a handle. The gate proved `recv_type ==
    // Some(<handle>)` + a covered handle op. Resolve the handle-receiver branch HERE
    // into a total `THandleOp` (reproducing the handle arms of `emit_builtin_method`),
    // so emit makes no type decision (I3). Args lowered PLAINLY (`arg(i)` = raw
    // `emit_expr`). The return type is the total sema handle-table fact.
    // D-SIMD2 / D-LINALG1: a method on a built-in math value type. Resolve the
    // reduce-op marker (which is NOT a lowerable expression) here so emit makes no
    // decision (I3). The return type is the total sema math-method fact.
    if let Some(handle) = recv_type {
        if crate::Sema::is_math_type(handle) && !cx.type_names.contains(handle) {
            let is_reduce = method == "reduce" && crate::Sema::is_simd_lane_type(handle);
            if is_reduce || crate::Sema::math_method_return(handle, method, args.len()).is_some() {
                let recv_t = lower_expr(receiver, cx, env);
                let (reduce_op, value_args): (Option<String>, Vec<TExpr>) = if is_reduce {
                    let op = args
                        .first()
                        .and_then(|argument| reduce_op_name(&argument.expr))
                        .unwrap_or_else(|| "Add".to_string());
                    (Some(op), Vec::new())
                } else {
                    (
                        None,
                        args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect(),
                    )
                };
                let ty = if is_reduce {
                    crate::Sema::math_scalar_ty(handle)
                } else {
                    crate::Sema::math_method_return(handle, method, args.len())
                        .unwrap_or_else(unit_type)
                };
                return TExpr {
                    ty,
                    kind: TExprKind::HandleMethod {
                        recv: Box::new(recv_t),
                        op: THandleOp::MathMethod {
                            type_name: handle.to_string(),
                            method: method.to_string(),
                            reduce_op,
                        },
                        args: value_args,
                    },
                };
            }
        }
    }
    // D-SHIFT1 (c7shift): `cursor.take_pattern("…")` — argument-dependent
    // (the return shape comes from the pattern's holes), so it is resolved
    // directly instead of through a generic method-return table. The parser
    // already committed to `Expr::StrMatchLit`
    // for this argument (sema rejects any other shape), so it's always
    // present when this method name/receiver-type pair is reached.
    if let Some(handle) = recv_type {
        if handle == "Cursor" && method == "take_pattern" {
            if let Some(arg) = args.first() {
                if let Expr::StrMatchLit(parts, _) = &arg.expr {
                    return lower_cursor_take_pattern(receiver, parts, cx, env);
                }
            }
        }
    }
    // D-BINPAT1 (card #506 follow-up): `reader.take_pattern(b"…")` — the
    // byte-mode sibling, same reasoning. The parser already committed to
    // `Expr::BinMatchLit` for this argument (sema rejects any other shape).
    if let Some(handle) = recv_type {
        if handle == "Reader" && method == "take_pattern" {
            if let Some(arg) = args.first() {
                if let Expr::BinMatchLit(parts, _) = &arg.expr {
                    return lower_reader_take_pattern(receiver, parts, cx, env);
                }
            }
        }
    }
    // D-LAYOUT1 / D-LAYOUT-GATES1: a method on `LayoutHandle`/`Constraint`.
    // Every Jet method name IS the `jet_layout` Rust method name (pure
    // passthrough, no reduce-marker-style special casing needed).
    if let Some(handle) = recv_type {
        if crate::Sema::is_layout_type(handle) {
            if let Some(ret) = crate::Sema::layout_method_return(handle, method, args.len()) {
                let recv_t = lower_expr(receiver, cx, env);
                let value_args: Vec<TExpr> =
                    args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
                return TExpr {
                    ty: ret,
                    kind: TExprKind::HandleMethod {
                        recv: Box::new(recv_t),
                        op: THandleOp::LayoutMethod {
                            method: method.to_string(),
                        },
                        args: value_args,
                    },
                };
            }
        }
    }
    if super::is_eval_fragment() && recv_type.is_none() && args.is_empty() {
        let recv = lower_expr(receiver, cx, env);
        if method == "encode" && fragment_serde_encode_type(&recv.ty, cx) {
            return lower_serde_encode_node(recv, cx);
        }
        if method == Syntax::METHOD_DATATREE_DECODE
            && matches!(&recv.ty, Type::Named(name) if is_json_type_name(name))
        {
            if let Some(target) = type_args.first() {
                return lower_datatree_decode_node(recv, target.clone(), resolved_ret, cx);
            }
        }
        *lowered_receiver.borrow_mut() = Some(recv);
    }
    if let Some(handle) = recv_type {
        if handle == "__SerdeEncode__" && method == "encode" && args.is_empty() {
            let recv = lower_expr(receiver, cx, env);
            return lower_serde_encode_node(recv, cx);
        }
        if handle == Syntax::TYPE_DATA
            && method == Syntax::METHOD_DATATREE_DECODE
            && args.is_empty()
        {
            if let Some(Type::Result { ok, .. }) = resolved_ret {
                return lower_datatree_decode_node(
                    lower_expr(receiver, cx, env),
                    (**ok).clone(),
                    resolved_ret,
                    cx,
                );
            }
        }
        if let Some(mut op) = handle_method_op(handle, method, args.len()) {
            if let THandleOp::DurationIn { unit } = &mut op {
                *unit = args.first().and_then(|arg| match &arg.expr {
                    Expr::EnumLit { variant, .. } => Syntax::DURATION_UNITS
                        .iter()
                        .copied()
                        .find(|candidate| *candidate == variant),
                    _ => None,
                });
            }
            let recv_t = lower_expr(receiver, cx, env);
            // D-GAME*: stash spawn-lambda for resident JIT `game.run` callbacks.
            // Keep the lowered lambda arg so AOT emit still receives it.
            if matches!(op, THandleOp::GameSceneOnFrame) {
                if let Some(Expr::Lambda(lam)) = args.first().map(|a| &a.expr) {
                    let mut jit_lambda = lower_spawn_lambda_for_jit_expecting(
                        lam,
                        cx,
                        env,
                        &[Type::Named("GameFrame".to_string())],
                    );
                    for (_, ty) in &mut jit_lambda.params {
                        *ty = Type::Named("GameFrame".to_string());
                    }
                    cx.jit_spawn_lambdas.borrow_mut().push(jit_lambda);
                }
            }
            let targs: Vec<TExpr> = args
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    if handle == "Regex" && method == "replace_all_with" && i == 1 {
                        if let Expr::Lambda(lam) = &a.expr {
                            let params = vec![Type::Named("RegexMatch".to_string())];
                            return TExpr {
                                ty: Type::Fn {
                                    params: params.clone(),
                                    ret: Some(Box::new(Type::String)),
                                    effect_bound: None, return_view_provenance: None,
                                    param_contract: None,
                call_metadata: None,
                                },
                                kind: TExprKind::Lambda(Box::new(
                                    lower_lambda_expecting_value(lam, cx, env, &params),
                                )),
                            };
                        }
                    }
                    // D-GAME*: `on_frame` is `Box<dyn FnMut(GameFrame)>` — typed
                    // by-value param + Box wrap (not Rc), or rustc E0282 / type mismatch (I2).
                    if matches!(op, THandleOp::GameSceneOnFrame) && i == 0 {
                        if let Expr::Lambda(lam) = &a.expr {
                            let params = vec![Type::Named("GameFrame".to_string())];
                            let mut lowered = lower_lambda_expecting_value(lam, cx, env, &params);
                            lowered.boxed = true;
                            lowered.rc = false;
                            lowered.arc = false;
                            return TExpr {
                                ty: Type::Fn {
                                    params: params.clone(),
                                    ret: Some(Box::new(Type::Named("Unit".to_string()))),
                                    effect_bound: None,
                                    param_contract: None,
                call_metadata: None,
                                    return_view_provenance: None,
                                },
                                kind: TExprKind::Lambda(Box::new(lowered)),
                            };
                        }
                    }
                    // `Rng.shuffle(&list)` must keep a writable place for TirBridge
                    // write-back (CallArg Write + Ident is not Expr::Borrow).
                    if handle == "Rng" && method == "shuffle" && i == 0 {
                        return lower_expr_as_mut_place(&a.expr, cx, env);
                    }
                    lower_expr(&a.expr, cx, env)
                })
                .collect();
            // c109 Phase 19: an arena `alloc(v)` returns a `&mut T` view whose VALUE type is
            // the arg's type (sema's `alloc_method_return` returns a `__alloc_infer__`
            // sentinel, resolved from the arg). The result `ty` is rarely load-bearing (an
            // `arena_view` binding emits no type annotation), but kept total per the design —
            // recovered from the LOWERED arg's total `ty`, never re-inferred (I3).
            let ty = match &op {
                THandleOp::AllocAlloc => targs
                    .first()
                    .map(|a| a.ty.clone())
                    .unwrap_or_else(unit_type),
                THandleOp::AllocTryAlloc => resolved_ret.cloned().unwrap_or_else(|| {
                    Type::Result {
                        ok: Box::new(
                            targs
                                .first()
                                .map(|a| a.ty.clone())
                                .unwrap_or_else(unit_type),
                        ),
                        err: Box::new(Type::Named(Syntax::TYPE_ALLOC_ERROR.to_string())),
                    }
                }),
                THandleOp::AllocReset => unit_type(),
                THandleOp::DataStreamNext => resolved_ret.cloned().unwrap_or_else(|| {
                    Type::Result {
                        ok: Box::new(Type::Option(Box::new(Type::Named("Unknown".to_string())))),
                        err: Box::new(Type::Named("DataError".to_string())),
                    }
                }),
                _ => handle_method_return_ty(
                    handle,
                    method,
                    args.len(),
                    &recv_t.ty,
                    resolved_ret,
                ),
            };
            return TExpr {
                ty,
                kind: TExprKind::HandleMethod {
                    recv: Box::new(recv_t),
                    op,
                    args: targs,
                },
            };
        }
    }
    // D-ENCSTREAM-SURFACE1=A: qualified shared type constructor.
    if method == "safe" && args.is_empty() {
        // D-APILABEL1=A: a Core parameter default is synthesized as a bare
        // `EncodingLimits.safe()`, because the caller that skipped the argument
        // need not have imported `core.encoding` to name an alias for it.
        if let Expr::Ident(type_name, _) = receiver {
            if type_name == "EncodingLimits" && !cx.struct_fields.contains_key(type_name) {
                return TExpr {
                    ty: Type::Named("EncodingLimits".to_string()),
                    kind: TExprKind::StaticCall {
                        owner: rooted_owner("jet_std::EncodingLimits"),
                        owner_type: None,
                        method: TMethodRef::bare("safe"),
                        type_args: Vec::new(),
                        args: vec![],
                    },
                };
            }
        }
        if let Expr::Field(base, leaf, _) = receiver {
            if leaf == "EncodingLimits"
                && core_module_path_from_receiver(base, cx, env).as_deref() == Some("core.encoding")
            {
                return TExpr {
                    ty: Type::Named("EncodingLimits".to_string()),
                    kind: TExprKind::StaticCall {
                        owner: rooted_owner("jet_std::EncodingLimits"),
                        owner_type: None,
                        method: TMethodRef::bare("safe"),
                        type_args: Vec::new(),
                        args: vec![],
                    },
                };
            }
            if leaf == "DataLimits"
                && core_module_path_from_receiver(base, cx, env).as_deref() == Some("core.data")
            {
                return TExpr {
                    ty: Type::Named("DataLimits".to_string()),
                    kind: TExprKind::StaticCall {
                        owner: rooted_owner("jet_std::DataLimits"),
                        owner_type: None,
                        method: TMethodRef::bare("safe"),
                        type_args: Vec::new(),
                        args: vec![],
                    },
                };
            }
            if leaf == "DataLimits"
                && core_module_path_from_receiver(base, cx, env).as_deref() == Some("core.data")
            {
                return TExpr {
                    ty: Type::Named("DataLimits".to_string()),
                    kind: TExprKind::StaticCall {
                        owner: rooted_owner("jet_std::DataLimits"),
                        owner_type: None,
                        method: TMethodRef::bare("safe"),
                        type_args: Vec::new(),
                        args: vec![],
                    },
                };
            }
            if leaf == "CBOROptions"
                && core_module_path_from_receiver(base, cx, env).as_deref() == Some("core.encoding.cbor")
            {
                return TExpr {
                    ty: Type::Named("CBOROptions".to_string()),
                    kind: TExprKind::StaticCall {
                        owner: rooted_owner("jet_std::CBOROptions"),
                        owner_type: None,
                        method: TMethodRef::bare("safe"),
                        type_args: Vec::new(),
                        args: vec![],
                    },
                };
            }
            if matches!(leaf.as_str(), "XMLLimits" | "XMLParseOptions" | "XMLRenderOptions")
                && core_module_path_from_receiver(base, cx, env).as_deref() == Some("core.encoding.xml")
            {
                return TExpr {
                    ty: Type::Named(leaf.clone()),
                    kind: TExprKind::StaticCall {
                        owner: rooted_owner(format!("jet_std::{leaf}")),
                        owner_type: None,
                        method: TMethodRef::bare("safe"),
                        type_args: Vec::new(),
                        args: vec![],
                    },
                };
            }
            if leaf == "Limits"
                && core_module_path_from_receiver(base, cx, env).as_deref() == Some("core.email")
            {
                return TExpr {
                    ty: Type::Named("Limits".to_string()),
                    kind: TExprKind::StaticCall {
                        owner: rooted_owner("jet_email::Limits"),
                        owner_type: None,
                        method: TMethodRef::bare("safe"),
                        type_args: Vec::new(),
                        args: vec![],
                    },
                };
            }
        }
    }
    // c109 Phase 7: a STATIC method call `Type.make(args)`. The gate
    // (`static_method_call_in_subset`) proved the receiver is a covered type-name
    // ident and `method` is a registered static method. Mirror the AST path
    // (Expression.rs ~L1644): `__jet_<Type>::__jet_<method>(args)`.
    if let Some(type_name) = static_call_type_name_lower(receiver, env) {
        if type_name == "Date" && method == "today" && args.is_empty() {
            return TExpr {
                ty: Type::Named("Date".to_string()),
                kind: TExprKind::CoreCall {
                    module: "core.time.date".to_string(),
                    method: "today".to_string(),
                    args: Vec::new(),
                    source_span: method_span,
                    widen_to_vec: Vec::new(),
                },
            };
        }
        if type_name == "Path"
            && method == "home"
            && args.is_empty()
            && !cx.type_names.contains("Path")
        {
            return TExpr {
                ty: Type::Named("Path".to_string()),
                kind: TExprKind::HandleMethod {
                    recv: Box::new(TExpr {
                        ty: unit_type(),
                        kind: TExprKind::Unit,
                    }),
                    op: THandleOp::PathHome,
                    args: Vec::new(),
                },
            };
        }
        // D-VALIDATE-DECODE1=B: generated codecs frame a child Result at the
        // one field/index boundary before applying `?`. Keep this as a TIR
        // node so AOT, JIT, and interpreter use one implementation.
        if type_name == "FieldError" && method == "under" && args.len() == 2 {
            let segment = lower_one_call_arg(&args[0], None, env, cx).value;
            let inner = lower_expr(&args[1].expr, cx, env);
            return TExpr {
                ty: resolved_ret.cloned().unwrap_or_else(|| inner.ty.clone()),
                kind: TExprKind::DecodeUnder {
                    segment: Box::new(segment),
                    inner: Box::new(inner),
                },
            };
        }
        if matches!(type_name.as_str(),
            "HTTPMethod" | "HTTPStatus" | "HTTPVersion" | "HTTPHeaderName"
            | "HTTPHeaderValue" | "HTTPHeaders" | "HTTPBody")
        {
            let method_rust = match (type_name.as_str(), method, args.len()) {
                ("HTTPBody", "bytes", 1) => "from_bytes",
                ("HTTPBody", "text", 1) => "from_text",
                ("HTTPBody", "text", 2) => "from_text_with_mime",
                ("HTTPBody", "json", 1) => "from_json",
                ("HTTPBody", "form", 1) => "from_form",
                ("HTTPBody", "multipart", 1) => "from_multipart",
                ("HTTPBody", "reader", 1) => "from_reader",
                ("HTTPBody", "reader", 2) => "from_reader_with_length",
                _ => method,
            };
            return TExpr {
                ty: resolved_ret.cloned().unwrap_or_else(|| Type::Named(type_name.clone())),
                kind: TExprKind::StaticCall {
                    owner: rooted_owner(format!("Jet{type_name}")),
                    owner_type: None,
                    method: TMethodRef::bare(method_rust),
                    type_args: Vec::new(),
                    args: args.iter().map(|argument| lower_one_call_arg(argument, None, env, cx)).collect(),
                },
            };
        }
        // D-STRPARSE1: text interpretation stays `Type.parse(text)`. Carry the
        // text as the builtin receiver so the existing builtin TIR seam owns emit.
        if let ("Int" | "Float", "parse", Some(arg)) =
            (type_name.as_str(), method, args.first())
        {
            if args.len() == 1 {
                return TExpr {
                    ty: resolved_ret.cloned().unwrap_or_else(|| Type::Result {
                        ok: Box::new(if type_name == "Int" {
                            Type::Int
                        } else {
                            Type::Float
                        }),
                        err: Box::new(Type::Named("ParseError".to_string())),
                    }),
                    kind: TExprKind::BuiltinMethod {
                        recv: Box::new(lower_expr(&arg.expr, cx, env)),
                        op: if type_name == "Int" {
                            TBuiltinOp::ParseInt
                        } else {
                            TBuiltinOp::ParseFloat
                        },
                        args: vec![],
                    },
                };
            }
        }
        // D-SHAPE-CONVERT1=A: numeric conversions are static on the destination
        // type. Reuse NumericMethod by treating the sole value argument as its
        // input; only the source-level call direction changed.
        if let (Some(target), Some(source_name), Some(arg)) = (
            crate::AST::numeric_type_from_name(&type_name),
            Syntax::numeric_conversion_source(method),
            args.first(),
        ) {
            if args.len() == 1 {
                let input = lower_expr(&arg.expr, cx, env);
                let op = resolve_numeric_conversion_op(&type_name, source_name)
                    .expect("sema admitted a numeric destination conversion");
                let ty = crate::Collections::builtin_method_return(&target, method, 1, true)
                    .flatten()
                    .unwrap_or(target);
                return TExpr {
                    ty,
                    kind: TExprKind::NumericMethod {
                        recv: Box::new(input),
                        op,
                    },
                };
            }
        }
        if let (Some((base, _)), Some(source), Some(arg)) = (
            cx.distinct_types.get(&type_name),
            Syntax::numeric_conversion_source(method),
            args.first(),
        ) {
            if args.len() == 1 {
                let op = resolve_numeric_conversion_op(&base.name(), source)
                    .expect("sema admitted a numeric distinct conversion");
                let range = cx.distinct_ranges.get(&type_name).copied();
                return TExpr {
                    ty: resolved_ret
                        .cloned()
                        .unwrap_or_else(|| Type::Named(type_name.clone())),
                    kind: TExprKind::DistinctConvert {
                        name: type_name,
                        arg: Box::new(lower_expr(&arg.expr, cx, env)),
                        op,
                        range,
                        fallible: matches!(resolved_ret, Some(Type::Result { .. })),
                    },
                };
            }
        }
        if let (Some((base, _)), Some(arg)) = (cx.distinct_types.get(&type_name), args.first()) {
            if args.len() == 1
                && !base.is_numeric()
                && Syntax::conversion_method_for_source(&base.name()) == method
            {
                return TExpr {
                    ty: Type::Named(type_name.clone()),
                    kind: TExprKind::Call {
                        name: type_name,
                        type_args: Vec::new(),
                        args: vec![lower_one_call_arg(arg, None, env, cx)],
                    },
                };
            }
        }
        if let (Some(destination), Some(arg)) = (cx.unit_facts.get(&type_name), args.first()) {
            let exact_method = method.strip_suffix("_rounded").unwrap_or(method);
            let lowered = lower_expr(&arg.expr, cx, env);
            let rounding = args.get(1).and_then(|mode| match &mode.expr {
                Expr::EnumLit { type_name, variant, args, .. }
                    if type_name.is_empty() && args.is_empty() =>
                {
                    Syntax::unit_rounding_mode(variant)
                }
                _ => None,
            });
            let source = match &lowered.ty {
                Type::Named(name) => cx.unit_facts.get(name),
                _ => None,
            }
            .or_else(|| {
                let destination_scope = type_name.rsplit_once('.').map(|(scope, _)| scope);
                cx.unit_facts.iter().find_map(|(name, fact)| {
                    let leaf = name.rsplit('.').next().unwrap_or(name);
                    let source_scope = name.rsplit_once('.').map(|(scope, _)| scope);
                    (source_scope == destination_scope
                        && fact.family == destination.family
                        && fact.kind == destination.kind
                        && Syntax::conversion_method_for_source(leaf) == exact_method)
                        .then_some(fact)
                })
            });
            if args.len() == 1 || (args.len() == 3 && rounding.is_some()) {
                if let Some(source) = source {
                    let scale = source.scale.div(&destination.scale)
                        .expect("sema validated unit scale");
                    let offset = source.offset.sub(&destination.offset)
                        .and_then(|value| value.div(&destination.scale))
                        .expect("sema validated unit offset");
                    let fallible = matches!(resolved_ret, Some(Type::Result { .. }));
                    let rounding = rounding.map(|mode| {
                        (mode, Box::new(lower_expr(&args[2].expr, cx, env)))
                    });
                    return TExpr {
                        ty: if fallible {
                            Type::Result {
                                ok: Box::new(Type::Float),
                                err: Box::new(Type::String),
                            }
                        } else {
                            Type::Float
                        },
                        kind: TExprKind::UnitConvert {
                            destination: type_name,
                            arg: Box::new(lowered),
                            scale,
                            offset,
                            rounding,
                            fallible,
                            file: cx.file.clone(),
                            line: crate::Diagnostics::span_line_col(&cx.src, method_span.start).0
                                as u32,
                        },
                    };
                }
            }
        }
        // D-PATHFS1: `Path.from(str)` → `jet_path_from(&(str_arg))`.
        // The string arg becomes the "receiver" slot of the PathFrom HandleMethod;
        // `Path` itself (a type-name ident) has no value.
        if type_name == "Path"
            && method == "from"
            && args.len() == 1
            && !cx.type_names.contains("Path")
        {
            let str_arg = lower_expr(&args[0].expr, cx, env);
            return TExpr {
                ty: Type::Named("Path".to_string()),
                kind: TExprKind::HandleMethod {
                    recv: Box::new(str_arg),
                    op: THandleOp::PathFrom,
                    args: vec![],
                },
            };
        }
        // D-SHIFT1 (c7shift): `Reader.over(bytes)` → `jet_reader_over(&(bytes_arg))`.
        // Same "arg becomes the recv slot" shape as `Path.from` above.
        if type_name == "Reader"
            && method == "over"
            && args.len() == 1
            && !cx.type_names.contains("Reader")
        {
            let bytes_arg = lower_expr(&args[0].expr, cx, env);
            return TExpr {
                ty: Type::Named("Reader".to_string()),
                kind: TExprKind::HandleMethod {
                    recv: Box::new(bytes_arg),
                    op: THandleOp::ReaderOver,
                    args: vec![],
                },
            };
        }
        // D-SHIFT1: `Cursor.over(s)` → `jet_cursor_over(&(s_arg))`.
        if type_name == "Cursor"
            && method == "over"
            && args.len() == 1
            && !cx.type_names.contains("Cursor")
        {
            let s_arg = lower_expr(&args[0].expr, cx, env);
            return TExpr {
                ty: Type::Named("Cursor".to_string()),
                kind: TExprKind::HandleMethod {
                    recv: Box::new(s_arg),
                    op: THandleOp::CursorOver,
                    args: vec![],
                },
            };
        }
        // D-FIDELITY-API1=A: `Perf.fidelity()` / `Perf.override_fidelity(v)?`
        // lower to the same core call shape as `use core.perf as perf`.
        if type_name == "Perf" && !cx.type_names.contains("Perf") {
            let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
            let widen_to_vec = core_widen_to_vec("core.perf", method, &targs);
            return TExpr {
                ty: core_call_return_ty("core.perf", method),
                kind: TExprKind::CoreCall {
                    module: "core.perf".to_string(),
                    method: method.to_string(),
                    args: targs,
                    source_span: method_span,
                    widen_to_vec,
                },
            };
        }
        // D-COLLBREADTH1=A: `Set.from([...])` → collect list into HashSet.
        // Lower the list arg as the recv of a SetFrom BuiltinMethod.
        if type_name == "Set" && method == "from" && args.len() == 1 {
            let list_arg = lower_expr(&args[0].expr, cx, env);
            let elem_ty = match &list_arg.ty {
                Type::List(inner) => *inner.clone(),
                _ => Type::Int,
            };
            return TExpr {
                ty: Type::Apply {
                    name: "Set".to_string(),
                    args: vec![elem_ty],
                },
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(list_arg),
                    op: TBuiltinOp::SetFrom,
                    args: vec![],
                },
            };
        }
        // #1478: `Set.new()` → empty HashSet with elem type from sema.
        if type_name == "Set" && method == "new" && args.is_empty() {
            let elem_ty = match resolved_ret {
                Some(Type::Apply { args: targs, .. }) if !targs.is_empty() => targs[0].clone(),
                _ => Type::Int,
            };
            return TExpr {
                ty: Type::Apply {
                    name: "Set".to_string(),
                    args: vec![elem_ty.clone()],
                },
                kind: TExprKind::StaticCall {
                    owner: host_generic_owner(
                        "std::collections::HashSet",
                        vec![TPreludeArg::Jet(elem_ty)],
                    ),
                    owner_type: None,
                    method: TMethodRef::bare("new"),
                    type_args: Vec::new(),
                    args: vec![],
                },
            };
        }
        // D-ALLOCFAIL1=A: `List.try_new()` and `.try_with_capacity(n)` use
        // the ordinary BuiltinMethod carrier; the inert Unit receiver keeps
        // static lowering inside the existing TIR seam.
        if type_name == Syntax::TYPE_LIST
            && !cx.type_names.contains(Syntax::TYPE_LIST)
            && matches!(method, "try_new" | "try_with_capacity")
            && ((method == "try_new" && args.is_empty())
                || (method == "try_with_capacity" && args.len() == 1))
        {
            let elem_ty = match resolved_ret {
                Some(Type::Result { ok, .. }) => match ok.as_ref() {
                    Type::List(inner) => (**inner).clone(),
                    _ => Type::Int,
                },
                _ => Type::Int,
            };
            let ty = resolved_ret.clone().unwrap_or_else(|| Type::Result {
                ok: Box::new(Type::List(Box::new(elem_ty.clone()))),
                err: Box::new(Type::Named(Syntax::TYPE_ALLOC_ERROR.to_string())),
            });
            let value_args = if method == "try_with_capacity" {
                vec![lower_expr(&args[0].expr, cx, env)]
            } else {
                Vec::new()
            };
            return TExpr {
                ty,
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(TExpr {
                        ty: unit_type(),
                        kind: TExprKind::Unit,
                    }),
                    op: if method == "try_new" {
                        TBuiltinOp::ListTryNew
                    } else {
                        TBuiltinOp::ListTryWithCapacity
                    },
                    args: value_args,
                },
            };
        }
        // #1477: `Map.new()` → empty JetMap; `Map.from_keys(keys, default)`.
        if type_name == Syntax::TYPE_MAP && method == "new" && args.is_empty() {
            let (key, value) = match resolved_ret {
                Some(Type::Map { key, value, .. }) => ((**key).clone(), (**value).clone()),
                _ => (Type::Int, Type::Int),
            };
            return TExpr {
                ty: Type::Map {
                    key: Box::new(key),
                    key_span: None,
                    value: Box::new(value),
                },
                kind: TExprKind::MapLit(Vec::new()),
            };
        }
        if type_name == Syntax::TYPE_MAP && method == "from_keys" && args.len() == 2 {
            let keys = lower_expr(&args[0].expr, cx, env);
            let default = lower_expr(&args[1].expr, cx, env);
            let (key, value) = match resolved_ret {
                Some(Type::Map { key, value, .. }) => ((**key).clone(), (**value).clone()),
                _ => (
                    match &keys.ty {
                        Type::List(inner) => *inner.clone(),
                        _ => Type::Int,
                    },
                    default.ty.clone(),
                ),
            };
            return TExpr {
                ty: Type::Map {
                    key: Box::new(key),
                    key_span: None,
                    value: Box::new(value),
                },
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(keys),
                    op: TBuiltinOp::MapFromKeys,
                    args: vec![default],
                },
            };
        }
        if type_name == crate::Syntax::TYPE_SORTED_SET && method == "from" && args.len() == 1 {
            let list_arg = lower_expr(&args[0].expr, cx, env);
            let elem_ty = match &list_arg.ty {
                Type::List(inner) => *inner.clone(),
                _ => Type::Int,
            };
            return TExpr {
                ty: Type::Apply {
                    name: crate::Syntax::TYPE_SORTED_SET.to_string(),
                    args: vec![elem_ty],
                },
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(list_arg),
                    op: TBuiltinOp::SortedSetFrom,
                    args: vec![],
                },
            };
        }
        if type_name == crate::Syntax::TYPE_SORTED_SET && method == "new" && args.is_empty() {
            let elem_ty = match resolved_ret {
                Some(Type::Apply { args: targs, .. }) if !targs.is_empty() => targs[0].clone(),
                _ => Type::Int,
            };
            return TExpr {
                ty: Type::Apply {
                    name: crate::Syntax::TYPE_SORTED_SET.to_string(),
                    args: vec![elem_ty.clone()],
                },
                kind: TExprKind::StaticCall {
                    owner: host_generic_owner(
                        "std::collections::BTreeSet",
                        vec![TPreludeArg::Jet(elem_ty)],
                    ),
                    owner_type: None,
                    method: TMethodRef::bare("new"),
                    type_args: Vec::new(),
                    args: vec![],
                },
            };
        }
        if type_name == crate::Syntax::TYPE_PRIORITY_QUEUE && method == "from" && args.len() == 1 {
            let list_arg = lower_expr(&args[0].expr, cx, env);
            let elem_ty = match &list_arg.ty {
                Type::List(inner) => *inner.clone(),
                _ => Type::Int,
            };
            return TExpr {
                ty: Type::Apply {
                    name: crate::Syntax::TYPE_PRIORITY_QUEUE.to_string(),
                    args: vec![elem_ty],
                },
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(list_arg),
                    op: TBuiltinOp::PriorityQueueFrom,
                    args: vec![],
                },
            };
        }
        if type_name == crate::Syntax::TYPE_PRIORITY_QUEUE && method == "new" && args.is_empty() {
            let elem_ty = match resolved_ret {
                Some(Type::Apply { args: targs, .. }) if !targs.is_empty() => targs[0].clone(),
                _ => Type::Int,
            };
            return TExpr {
                ty: Type::Apply {
                    name: crate::Syntax::TYPE_PRIORITY_QUEUE.to_string(),
                    args: vec![elem_ty.clone()],
                },
                kind: TExprKind::StaticCall {
                    owner: host_generic_owner(
                        "std::collections::BinaryHeap",
                        vec![TPreludeArg::Jet(elem_ty)],
                    ),
                    owner_type: None,
                    method: TMethodRef::bare("new"),
                    type_args: Vec::new(),
                    args: vec![],
                },
            };
        }
        if type_name == crate::Syntax::TYPE_LRU && method == "new" && args.len() == 1 {
            let cap_arg = lower_expr(&args[0].expr, cx, env);
            let (key_ty, value_ty) = match resolved_ret {
                Some(Type::Apply { args: targs, .. }) if targs.len() >= 2 => {
                    (targs[0].clone(), targs[1].clone())
                }
                _ => (Type::String, Type::Int),
            };
            return TExpr {
                ty: Type::Apply {
                    name: crate::Syntax::TYPE_LRU.to_string(),
                    args: vec![key_ty, value_ty],
                },
                kind: TExprKind::StaticCall {
                    owner: host_owner("JetCache"),
                    owner_type: None,
                    method: TMethodRef::bare("new"),
                    type_args: Vec::new(),
                    args: vec![TCallArg {
                        value: cap_arg,
                        template_items: None,
                        borrow: false,
                        mut_borrow: false,
                        clone: false,
                        arc_clone: false,
                        fn_coerce: None,
                        widen_to_vec: false,
                        widen_to_union: None,
                    }],
                },
            };
        }
        if type_name == crate::Syntax::TYPE_BIT_SET && method == "new" && args.is_empty() {
            return TExpr {
                ty: Type::Named(crate::Syntax::TYPE_BIT_SET.to_string()),
                kind: TExprKind::StaticCall {
                    owner: host_owner("JetBitSet"),
                    owner_type: None,
                    method: TMethodRef::bare("new"),
                    type_args: Vec::new(),
                    args: vec![],
                },
            };
        }
        if type_name == crate::Syntax::TYPE_BYTE_BUFFER && method == "new" && args.is_empty() {
            return TExpr {
                ty: Type::Named(crate::Syntax::TYPE_BYTE_BUFFER.to_string()),
                kind: TExprKind::StaticCall {
                    owner: host_owner("JetByteBuffer"),
                    owner_type: None,
                    method: TMethodRef::bare("new"),
                    type_args: Vec::new(),
                    args: vec![],
                },
            };
        }
        if type_name == crate::Syntax::TYPE_BYTE_BUFFER
            && method == "with_capacity"
            && args.len() == 1
        {
            let n = lower_expr(&args[0].expr, cx, env);
            return TExpr {
                ty: Type::Named(crate::Syntax::TYPE_BYTE_BUFFER.to_string()),
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(n),
                    op: TBuiltinOp::ByteBufferWithCapacity,
                    args: vec![],
                },
            };
        }
        if type_name == crate::Syntax::TYPE_BYTE_BUFFER && method == "from" && args.len() == 1 {
            let bytes_arg = lower_expr(&args[0].expr, cx, env);
            return TExpr {
                ty: Type::Named(crate::Syntax::TYPE_BYTE_BUFFER.to_string()),
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(bytes_arg),
                    op: TBuiltinOp::ByteBufferFrom,
                    args: vec![],
                },
            };
        }
        // D-COLLBREADTH1=A: `Deque.init([...])` → collect list into VecDeque.
        if type_name == "Deque" && method == "init" && args.len() == 1 {
            let list_arg = lower_expr(&args[0].expr, cx, env);
            let elem_ty = match &list_arg.ty {
                Type::List(inner) => *inner.clone(),
                _ => Type::Int,
            };
            return TExpr {
                ty: Type::Apply {
                    name: "Deque".to_string(),
                    args: vec![elem_ty],
                },
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(list_arg),
                    op: TBuiltinOp::DequeFrom,
                    args: vec![],
                },
            };
        }
        // D-COLLBREADTH1=A: `Deque.new()` → empty VecDeque with elem type from sema.
        // The element type comes from `resolved_ret` (sema filled it from the annotation).
        if type_name == "Deque" && method == "new" && args.is_empty() {
            let elem_ty = match resolved_ret {
                Some(Type::Apply { args: targs, .. }) if !targs.is_empty() => targs[0].clone(),
                _ => Type::Int,
            };
            let deque_ty = Type::Apply {
                name: "Deque".to_string(),
                args: vec![elem_ty.clone()],
            };
            return TExpr {
                ty: deque_ty,
                kind: TExprKind::StaticCall {
                    owner: host_generic_owner(
                        "std::collections::VecDeque",
                        vec![TPreludeArg::Jet(elem_ty)],
                    ),
                    owner_type: None,
                    method: TMethodRef::bare("new"),
                    type_args: Vec::new(),
                    args: vec![],
                },
            };
        }
        // D-TAG1: `Bag.new()` → empty HashMap with elem type from sema.
        if type_name == "Bag" && method == "new" && args.is_empty() {
            let elem_ty = match resolved_ret {
                Some(Type::Apply { args: targs, .. }) if !targs.is_empty() => targs[0].clone(),
                _ => Type::Int,
            };
            let bag_ty = Type::Apply {
                name: "Bag".to_string(),
                args: vec![elem_ty.clone()],
            };
            return TExpr {
                ty: bag_ty,
                kind: TExprKind::StaticCall {
                    owner: host_generic_owner(
                        "std::collections::HashMap",
                        vec![TPreludeArg::Jet(elem_ty), TPreludeArg::HostUsize],
                    ),
                    owner_type: None,
                    method: TMethodRef::bare("new"),
                    type_args: Vec::new(),
                    args: vec![],
                },
            };
        }
        // D-MEM1 S6 (D-POOLID-API1=A): `Pool<T>.new()` → an empty `JetPool<T>`.
        // The element type comes from `resolved_ret` (sema filled it from the
        // call-site turbofish or the binding's annotation).
        if type_name == "Pool" && method == "new" && args.is_empty() {
            let elem_ty = match resolved_ret {
                Some(Type::Apply { args: targs, .. }) if !targs.is_empty() => targs[0].clone(),
                _ => Type::Int,
            };
            let pool_ty = Type::Apply {
                name: "Pool".to_string(),
                args: vec![elem_ty.clone()],
            };
            return TExpr {
                ty: pool_ty,
                kind: TExprKind::StaticCall {
                    owner: rooted_generic_owner(
                        "jet_std::JetPool",
                        vec![TPreludeArg::Jet(elem_ty)],
                    ),
                    owner_type: None,
                    method: TMethodRef::bare("new"),
                    type_args: Vec::new(),
                    args: vec![],
                },
            };
        }
        // D-MEM1 S6 (D-SHARED-API1=A): `Shared.new(x)` → a `JetShared<T>`
        // wrapping `x`; `T` is `x`'s own lowered type (no turbofish, no
        // annotation needed — the argument alone fixes it).
        if type_name == "Shared" && method == "new" && args.len() == 1 {
            let arg_t = lower_expr(&args[0].expr, cx, env);
            let elem_ty = arg_t.ty.clone();
            return TExpr {
                ty: Type::Shared(Box::new(elem_ty.clone())),
                kind: TExprKind::StaticCall {
                    owner: rooted_generic_owner(
                        "jet_std::JetShared",
                        vec![TPreludeArg::Jet(elem_ty)],
                    ),
                    owner_type: None,
                    method: TMethodRef::bare("new"),
                    type_args: Vec::new(),
                    args: vec![TCallArg {
                        value: arg_t,
                        template_items: None,
                        borrow: false,
                        mut_borrow: false,
                        clone: false,
                        arc_clone: false,
                        fn_coerce: None,
                        widen_to_vec: false,
                        widen_to_union: None,
                    }],
                },
            };
        }
        if type_name == Syntax::TYPE_CONDITION && method == "new" && args.is_empty() {
            return TExpr {
                ty: Type::Named(Syntax::TYPE_CONDITION.to_string()),
                kind: TExprKind::StaticCall {
                    owner: rooted_owner("jet_std::JetCondition"),
                    owner_type: None,
                    method: TMethodRef::bare("new"),
                    type_args: Vec::new(),
                    args: Vec::new(),
                },
            };
        }
        if type_name == "Cell" && method == "new" && args.len() == 1 {
            let value = lower_expr(&args[0].expr, cx, env);
            let cell_ty = resolved_ret.cloned().unwrap_or_else(|| Type::Apply {
                name: "Cell".to_string(),
                args: vec![value.ty.clone()],
            });
            let elem_ty = match &cell_ty {
                Type::Apply { args, .. } if !args.is_empty() => args[0].clone(),
                _ => value.ty.clone(),
            };
            return TExpr {
                ty: cell_ty,
                kind: TExprKind::StaticCall {
                    owner: rooted_generic_owner(
                        "jet_std::JetCell",
                        vec![TPreludeArg::Jet(elem_ty)],
                    ),
                    owner_type: None,
                    method: TMethodRef::bare("new"),
                    type_args: Vec::new(),
                    args: vec![TCallArg {
                        value,
                        template_items: None,
                        borrow: false,
                        mut_borrow: false,
                        clone: false,
                        arc_clone: false,
                        fn_coerce: None,
                        widen_to_vec: false,
                        widen_to_union: None,
                    }],
                },
            };
        }
        if type_name == "ExpiringSecret" && method == "new" && args.len() == 3 {
            let value = lower_expr(&args[0].expr, cx, env);
            let duration = lower_expr(&args[1].expr, cx, env);
            let clock = lower_expr(&args[2].expr, cx, env);
            let elem_ty = match resolved_ret {
                Some(Type::Apply { name, args })
                    if name == "ExpiringSecret" && !args.is_empty() =>
                {
                    args[0].clone()
                }
                _ => value.ty.clone(),
            };
            return TExpr {
                ty: Type::Apply {
                    name: "ExpiringSecret".to_string(),
                    args: vec![elem_ty.clone()],
                },
                kind: TExprKind::HostCall(Box::new(THostCall::ExpiringSecretNew {
                    value: Box::new(value),
                    duration: Box::new(duration),
                    clock: Box::new(clock),
                    elem: elem_ty,
                })),
            };
        }
        if type_name == Syntax::EXPIRING_VALUE_TYPE && method == "new" && args.len() == 3 {
            let value = lower_expr(&args[0].expr, cx, env);
            let duration = lower_expr(&args[1].expr, cx, env);
            let clock = lower_expr(&args[2].expr, cx, env);
            let elem_ty = match resolved_ret {
                Some(Type::Apply { name, args })
                    if name == Syntax::EXPIRING_VALUE_TYPE && !args.is_empty() =>
                {
                    args[0].clone()
                }
                _ => value.ty.clone(),
            };
            return TExpr {
                ty: Type::Apply {
                    name: Syntax::EXPIRING_VALUE_TYPE.to_string(),
                    args: vec![elem_ty],
                },
                kind: TExprKind::HostCall(Box::new(THostCall::ExpiringValueNew {
                    value: Box::new(value),
                    duration: Box::new(duration),
                    clock: Box::new(clock),
                })),
            };
        }
        // D-HOLE1: `Option.lift2(f, a, b)` → apply `f` to both payloads only when
        // both are present. `a`/`b` lower plainly as values; `R` comes from sema's
        // `resolved_ret` (the arg-dependent return type, same mechanism the
        // polymorphic core specials use — see the AST `MethodCall.resolved_ret` doc).
        if type_name == "Option" && method == "lift2" && !cx.type_names.contains("Option") {
            let a_ty = match tir_recv_jet_ty(&args[1].expr, env) {
                Some(Type::Option(inner)) => (*inner).clone(),
                _ => Type::Int,
            };
            let b_ty = match tir_recv_jet_ty(&args[2].expr, env) {
                Some(Type::Option(inner)) => (*inner).clone(),
                _ => Type::Int,
            };
            // c142: a bare `f` callable is invoked only by the shared Prelude's
            // present branch, not stored — but rustc
            // still needs its param types written out whenever the body resolves a
            // trait method on a param (e.g. interpolation's `.jet_display()`), the same reason
            // `lower_one_call_arg` annotates a lambda flowing into a fn-typed
            // parameter. Reuse that mechanism with `a`/`b`'s payload types as the
            // expected params.
            let ret_ty = match resolved_ret {
                Some(Type::Option(inner)) => (**inner).clone(),
                _ => Type::Int,
            };
            let f_t = match &args[0].expr {
                Expr::Lambda(lam) => {
                    super::take_scheduled_expr(&args[0].expr, cx).unwrap_or_else(|| {
                        let tl = lower_lambda_expecting(
                            lam,
                            cx,
                            env,
                            Some(&[a_ty.clone(), b_ty.clone()]),
                        );
                        TExpr {
                            ty: Type::Fn {
                                params: vec![a_ty.clone(), b_ty.clone()],
                                ret: Some(Box::new(ret_ty.clone())),
                                effect_bound: None, return_view_provenance: None,
                                param_contract: None,
                                call_metadata: None,
                            },
                            kind: TExprKind::Lambda(Box::new(tl)),
                        }
                    })
                }
                _ => lower_expr(&args[0].expr, cx, env),
            };
            let a_t = lower_expr(&args[1].expr, cx, env);
            let b_t = lower_expr(&args[2].expr, cx, env);
            return TExpr {
                ty: Type::Option(Box::new(ret_ty)),
                kind: TExprKind::OptionLift2 {
                    f: Box::new(f_t),
                    a: Box::new(a_t),
                    b: Box::new(b_t),
                },
            };
        }
        // D-SIMD2 / D-LINALG1: a static method on a built-in math type → the prelude
        // free function `{root}jet_math_<T>_<method>(args)`.
        if crate::Sema::is_math_type(&type_name) && !cx.type_names.contains(&type_name) {
            if let Some(ret) = crate::Sema::math_static_return(&type_name, method, args.len()) {
                let bridge = crate::Sema::math_static_arg_ty(&type_name, method);
                let targs: Vec<TExpr> = args
                    .iter()
                    .map(|a| {
                        let mut t = lower_expr(&a.expr, cx, env);
                        // D-FIXARR1 bridge: a `[..]` literal arg to `from_array` lowered
                        // as a growable list; re-tag it to the `[T#N]` fixed array so emit
                        // produces `[e1, …]` (a Rust stack array), not `vec![…]`.
                        if let Some(fl @ Type::FixedList { .. }) = &bridge {
                            if matches!(t.ty, Type::List(_))
                                && matches!(t.kind, TExprKind::ListLit(_))
                            {
                                t.ty = fl.clone();
                            }
                        }
                        t
                    })
                    .collect();
                return TExpr {
                    ty: ret,
                    kind: TExprKind::MathBuiltin {
                        type_name: type_name.clone(),
                        func: method.to_string(),
                        args: targs,
                    },
                };
            }
        }
        let lookup_type_name = type_name
            .rsplit_once('.')
            .map_or(type_name.as_str(), |(_, leaf)| leaf);
        let sig = cx
            .method_sigs
            .get(&(lookup_type_name.to_string(), method.to_string()))
            .cloned()
            .unwrap_or_default();
        let sig = instantiated_sig
            .map(|sig| sig.to_vec())
            .unwrap_or_else(|| {
                instantiate_method_sig(
                    cx,
                    &type_name,
                    method,
                    &sig,
                    owner_type_args,
                    type_args,
                )
            });
        let targs = lower_method_args(args, &sig, env, cx);
        let resolved_type_args = resolved_method_type_args(
            cx,
            lookup_type_name,
            method,
            &sig,
            owner_type_args,
            &targs,
            type_args,
            resolved_ret,
        );
        let ret_ty = instantiate_method_ret(
            cx,
            lookup_type_name,
            method,
            owner_type_args,
            &resolved_type_args,
            resolved_ret,
        )
        .map(|ty| resolve_self_ty(&ty, &type_name))
        .unwrap_or_else(unit_type);
        let owner_type = if owner_type_args.is_empty() {
            Type::Named(type_name.clone())
        } else {
            Type::Apply {
                name: type_name.clone(),
                args: owner_type_args.to_vec(),
            }
        };
        if matches!(&owner_type, Type::Apply { .. }) || !resolved_type_args.is_empty() {
            cx.jit_method_calls.borrow_mut().insert(
                crate::Codegen::TIR::generic_method_instance_key(
                    &owner_type,
                    method,
                    &resolved_type_args,
                ),
                (owner_type.clone(), method.to_string(), resolved_type_args.clone()),
            );
        }
        return TExpr {
            ty: ret_ty,
            kind: TExprKind::StaticCall {
                owner: TStaticOwner::User(type_name.clone()),
                owner_type: Some(owner_type),
                method: TMethodRef::inherent(method),
                type_args: resolved_type_args,
                args: targs,
            },
        };
    }
    // c109 Phase 30: DYNAMIC dispatch on a TRAIT-OBJECT receiver (`s.name()`/`s.area()`,
    // `s: Box<dyn __jet_Shape>`). The gate proved `recv_type == Some(<trait>)` with the
    // trait in `cx.trait_names`. The AST `emit_method_call` (Expression.rs ~L1657) emits
    // `({recv}).{method}({args})` — the BARE (unmangled) method name (vtable dispatch),
    // node (`({recv}).{method_rust}({args})`) with the bare method name. Trait declarations
    // retain their full parameter/return facts in `Cx`, so dynamic calls use the same
    // convention-aware argument lowering as static calls.
    if let Some(ty) = recv_type {
        if cx.trait_names.contains(ty) {
            let key = (ty.clone(), method.to_string());
            let sig = cx.method_sigs.get(&key).cloned().unwrap_or_default();
            let sig = instantiated_sig
                .map(|sig| sig.to_vec())
                .unwrap_or_else(|| instantiate_method_sig(cx, ty, method, &sig, &[], type_args));
            let ret_ty = cx
                .method_rets
                .get(&key)
                .cloned()
                .flatten()
                .unwrap_or_else(unit_type);
            let mut recv = lower_expr(receiver, cx, env);
            // A specialized generic/variadic parameter can retain the trait as
            // `recv_type` while its lowered TIR type is the exact concrete
            // implementation. Keep that concrete ABI and dispatch directly;
            // only genuine trait-object values use the boxed dynamic ABI.
            if let Type::Named(concrete) = &recv.ty {
                if cx.type_names.contains(concrete) {
                    let targs = lower_method_args(args, &sig, env, cx);
                    return TExpr {
                        ty: ret_ty,
                        kind: TExprKind::MethodCall {
                            recv: Box::new(recv),
                            method: TMethodRef::bare(method),
                            type_args: type_args.to_vec(),
                            args: targs,
                            source_first_string_literal: first_string_literal_arg(args),
                            operator_line: None,
                        },
                    };
                }
            }
            recv.ty = Type::TraitObject(vec![ty.clone()]);
            let targs = lower_method_args(args, &sig, env, cx);
            return TExpr {
                ty: ret_ty,
                kind: TExprKind::MethodCall {
                    recv: Box::new(recv),
                    method: TMethodRef::trait_method(ty, method),
                    type_args: type_args.to_vec(),
                    args: targs,
                    source_first_string_literal: first_string_literal_arg(args),
                    operator_line: None,
                },
            };
        }
    }
    // A user instance method on a covered type. `recv_type` is total (gate proved
    // `Some`). Resolve the param conventions from `method_sigs` and the Rust method
    // name (trait-impl methods keep their bare name; others get the `__jet_` mangle).
    let Some(ty_name) = recv_type.clone() else {
        // Comptime may evaluate before sema writes `recv_type`; recover precise
        // numeric methods from the lowered receiver type.
        let recv_lowered = lower_expr(receiver, cx, env);
        if let Type::Named(n) = &recv_lowered.ty {
            if (n == Syntax::TYPE_BIGINT
                || n == Syntax::TYPE_DECIMAL
                || n == Syntax::TYPE_FRACTION)
                && !cx.type_names.contains(n)
            {
                let known = matches!(
                    (n.as_str(), method, args.len()),
                    ("BigInt", "add" | "sub" | "mul", 1)
                        | ("BigInt", "neg" | "to_string", 0)
                        | ("Decimal", "add" | "sub" | "mul", 1)
                        | ("Decimal", "to_string", 0)
                        | ("Fraction", "add" | "sub" | "mul" | "div" | "equal", 1)
                        | (
                            "Fraction",
                            "numerator" | "denominator" | "to_string" | "to_float" | "is_zero",
                            0
                        )
                );
                if known {
                    let type_name = n.clone();
                    let mut value_args = vec![recv_lowered];
                    value_args.extend(args.iter().map(|a| lower_expr(&a.expr, cx, env)));
                    let ty = match method {
                        "to_string" => Type::String,
                        "numerator" | "denominator" => Type::Int,
                        "to_float" => Type::Float,
                        "is_zero" | "equal" => Type::Bool,
                        _ => Type::Named(type_name.clone()),
                    };
                    return TExpr {
                        ty: resolved_ret.cloned().unwrap_or(ty),
                        kind: TExprKind::PreciseBuiltin {
                            type_name,
                            func: method.to_string(),
                            args: value_args,
                        },
                    };
                }
            }
        }
        // Comptime/REPL fragment eval (#777): keep MethodCall so the TIR
        // evaluator can dispatch via Builtins/host surface without sema facts.
        if super::is_eval_fragment() {
            let targs = args
                .iter()
                .map(|a| lower_one_call_arg(a, None, env, cx))
                .collect();
            return TExpr {
                ty: resolved_ret.cloned().unwrap_or(recv_lowered.ty.clone()),
                kind: TExprKind::MethodCall {
                    recv: Box::new(recv_lowered),
                    method: TMethodRef::bare(method),
                    type_args: type_args.to_vec(),
                    args: targs,
                    source_first_string_literal: first_string_literal_arg(args),
                    operator_line: None,
                },
            };
        }
        return TExpr {
            ty: resolved_ret.cloned().unwrap_or(Type::Int),
            kind: TExprKind::Todo {
                line: method_span.start,
                expected_type: format!("method `{method}` receiver type"),
            },
        };
    };
    // Sema preserves a qualified nominal name for imported receivers so type
    // identity and Rust lowering stay collision-safe.  Imported method facts
    // are registered by their declaration's bare owner name; use that key only
    // for metadata lookup while retaining `ty_name` in the lowered receiver.
    let lookup_ty_name = ty_name.rsplit_once('.').map_or(ty_name.as_str(), |(_, leaf)| leaf);
    let sig = cx
        .method_sigs
        .get(&(lookup_ty_name.to_string(), method.to_string()))
        .cloned()
        .unwrap_or_default();
    let recv = if matches!(
        cx.method_self_convs
            .get(&(lookup_ty_name.to_string(), method.to_string())),
        Some(AccessConvention::Move)
    )
        && matches!(receiver, Expr::Ident(name, _) if env.is_resource(name))
    {
        lower_owned_expr(receiver, cx, env)
    } else {
        lower_expr(receiver, cx, env)
    };
    let owner_type_args = match &recv.ty {
        Type::Apply { name, args }
            if name == &ty_name || name.as_str() == lookup_ty_name => args.as_slice(),
        _ => &[][..],
    };
    let sig = instantiated_sig
        .map(|sig| sig.to_vec())
        .unwrap_or_else(|| {
            instantiate_method_sig(cx, &ty_name, method, &sig, owner_type_args, type_args)
        });
    if matches!(&recv.ty, Type::Named(name) if cx.trait_names.contains(name)) {
        let trait_name = recv.ty.name();
        let mut recv = recv;
        recv.ty = Type::TraitObject(vec![trait_name.clone()]);
        let targs = lower_method_args(args, &sig, env, cx);
        let ret_ty = resolved_ret
            .cloned()
            .or_else(|| {
                cx.method_rets
                    .get(&(trait_name.clone(), method.to_string()))
                    .cloned()
                    .flatten()
            })
            .unwrap_or_else(unit_type);
        return TExpr {
            ty: ret_ty,
            kind: TExprKind::MethodCall {
                recv: Box::new(recv),
                method: TMethodRef::trait_method(&trait_name, method),
                type_args: type_args.to_vec(),
                args: targs,
                source_first_string_literal: first_string_literal_arg(args),
                operator_line: None,
            },
        };
    }
    let mut targs = lower_method_args(args, &sig, env, cx);
    let resolved_type_args = resolved_method_type_args(
        cx,
        lookup_ty_name,
        method,
        &sig,
        owner_type_args,
        &targs,
        type_args,
        resolved_ret,
    );
    if matches!(&recv.ty, Type::Apply { .. }) || !resolved_type_args.is_empty() {
        cx.jit_method_calls.borrow_mut().insert(
            crate::Codegen::TIR::generic_method_instance_key(
                &recv.ty,
                method,
                &resolved_type_args,
            ),
            (recv.ty.clone(), method.to_string(), resolved_type_args.clone()),
        );
    }
    let distinct_numeric_operator = cx
        .distinct_types
        .get(lookup_ty_name)
        .is_some_and(|(_, numeric)| *numeric)
        && !cx.distinct_ranges.contains_key(lookup_ty_name)
        && matches!(method, "add" | "sub" | "mul" | "div")
        && args.len() == 1;
    if distinct_numeric_operator {
        // The synthetic numeric traits use `fn op(&self, rhs: &Self)`. There is
        // no ordinary Jet method signature for this compiler-owned path, so the
        // borrow convention must be recorded on the TIR argument explicitly.
        targs[0].borrow = true;
    }
    // S62: a trait-impl method is called by its bare name (the trait impl owns it);
    // a plain user method is `__jet_<method>`. Numeric distinct operators are also
    // emitted through the bare synthetic operator trait, even though sema does not
    // register them as ordinary Jet methods.
    let method_ref = if cx
        .trait_methods
        .contains(&(lookup_ty_name.to_string(), method.to_string()))
        || distinct_numeric_operator
    {
        TMethodRef::bare(method)
    } else {
        TMethodRef::inherent(method)
    };
    // The result type, read from the resolved method return (total fact). It is
    // rarely load-bearing in emit (a binding carries sema's `b.ty`; arithmetic on a
    // method result doesn't trap — matching the AST `expr_jet_ty`/`operand_is_integer`),
    // but the TIR keeps it total per the design principle.
    let ret_ty = instantiate_method_ret(
        cx,
        lookup_ty_name,
        method,
        owner_type_args,
        &resolved_type_args,
        resolved_ret,
    )
    .unwrap_or_else(unit_type);
    TExpr {
        ty: ret_ty,
        kind: TExprKind::MethodCall {
            recv: Box::new(recv),
            method: method_ref,
            type_args: resolved_type_args,
            args: targs,
            source_first_string_literal: first_string_literal_arg(args),
            operator_line: distinct_numeric_operator.then(|| {
                crate::Diagnostics::span_line_col(&cx.src, method_span.start).0 as u32
            }),
        },
    }
}

pub(crate) fn instantiate_method_sig(
    cx: &Cx,
    type_name: &str,
    method: &str,
    sig: &[(AccessConvention, Type)],
    owner_type_args: &[Type],
    method_type_args: &[Type],
) -> Vec<(AccessConvention, Type)> {
    let mut subst = std::collections::HashMap::new();
    if let Some(owner_params) = cx.struct_type_param_order.get(type_name) {
        for (param, actual) in owner_params.iter().zip(owner_type_args) {
            subst.insert(param.clone(), actual.clone());
        }
    }
    if let Some(method_params) = cx.method_type_params.get(&(type_name.to_string(), method.to_string())) {
        for (param, actual) in method_params.iter().zip(method_type_args) {
            subst.insert(param.name.clone(), actual.clone());
        }
    }
    sig.iter()
        .map(|(conv, ty)| (*conv, crate::Generics::substitute_type(ty, &subst)))
        .collect()
}

fn instantiate_method_ret(
    cx: &Cx,
    type_name: &str,
    method: &str,
    owner_type_args: &[Type],
    method_type_args: &[Type],
    resolved_ret: Option<&Type>,
) -> Option<Type> {
    let template = cx
        .method_rets
        .get(&(type_name.to_string(), method.to_string()))
        .cloned()
        .flatten()
        .or_else(|| resolved_ret.cloned())?;
    let mut subst = owner_type_subst(cx, type_name, owner_type_args);
    if let Some(method_params) = cx
        .method_type_params
        .get(&(type_name.to_string(), method.to_string()))
    {
        for (param, actual) in method_params.iter().zip(method_type_args) {
            subst.insert(param.name.clone(), actual.clone());
        }
    }
    Some(crate::Generics::substitute_type(&template, &subst))
}

fn resolved_method_type_args(
    cx: &Cx,
    type_name: &str,
    method: &str,
    sig: &[(AccessConvention, Type)],
    owner_type_args: &[Type],
    args: &[TCallArg],
    explicit_type_args: &[Type],
    resolved_ret: Option<&Type>,
) -> Vec<Type> {
    if !explicit_type_args.is_empty() {
        return explicit_type_args.to_vec();
    }
    let Some(method_params) = cx
        .method_type_params
        .get(&(type_name.to_string(), method.to_string()))
    else {
        return Vec::new();
    };
    if method_params.is_empty() {
        return Vec::new();
    }
    let names: std::collections::HashSet<String> =
        method_params.iter().map(|param| param.name.clone()).collect();
    let mut subst = std::collections::HashMap::new();
    for ((_, template), arg) in sig.iter().zip(args) {
        if !crate::Codegen::TIR::bind_generic_type(
            template,
            &arg.value.ty,
            &names,
            &mut subst,
        ) {
            return Vec::new();
        }
    }
    if let (Some(template), Some(actual)) = (
        cx.method_rets
            .get(&(type_name.to_string(), method.to_string()))
            .and_then(Option::as_ref),
        resolved_ret,
    ) {
        let owner_subst = owner_type_subst(cx, type_name, owner_type_args);
        let template = crate::Generics::substitute_type(template, &owner_subst);
        if !crate::Codegen::TIR::bind_generic_type(&template, actual, &names, &mut subst) {
            return Vec::new();
        }
    }
    method_params
        .iter()
        .map(|param| subst.get(&param.name).cloned())
        .collect::<Option<Vec<_>>>()
        .unwrap_or_default()
}

fn owner_type_subst(
    cx: &Cx,
    type_name: &str,
    owner_type_args: &[Type],
) -> std::collections::HashMap<String, Type> {
    cx.struct_type_param_order
        .get(type_name)
        .into_iter()
        .flat_map(|params| params.iter().zip(owner_type_args))
        .map(|(param, actual)| (param.clone(), actual.clone()))
        .collect()
}

/// Demand monomorphized `T::encode` / `T::decode` when encoding core calls touch
/// a generic Apply type (JIT looks up `Wrap<Int>::encode`).
fn demand_generic_serde_codec(
    cx: &Cx,
    fn_name: &str,
    module: &str,
    method: &str,
    args: &[TExpr],
    ret_ty: &Type,
) {
    let encoding = matches!(
        module,
        "core.encoding.json"
            | "core.encoding.toml"
            | "core.encoding.yaml"
            | "core.encoding.csv"
            | "core.encoding.cbor"
    );
    if !encoding {
        return;
    }
    cx.jit_canonical_calls
        .borrow_mut()
        .insert(fn_name.to_string());
    if matches!(
        method,
        "to_string" | "to_string_pretty" | "to_bytes" | "to_bytes_canonical"
    ) {
        if let Some(arg) = args.first() {
            if matches!(&arg.ty, Type::Apply { .. }) {
                cx.jit_method_calls.borrow_mut().insert(
                    format!("{}::encode", arg.ty.name()),
                    (arg.ty.clone(), "encode".to_string(), Vec::new()),
                );
            }
        }
    }
    if method == "decode" {
        if let Type::Result { ok, .. } = ret_ty {
            if cx.migrations.contains_key(&ok.name()) {
                cx.jit_canonical_deopt
                    .borrow_mut()
                    .insert(fn_name.to_string());
            }
            if matches!(ok.as_ref(), Type::Apply { .. }) {
                cx.jit_method_calls.borrow_mut().insert(
                    format!("{}::decode", ok.name()),
                    ((**ok).clone(), "decode".to_string(), Vec::new()),
                );
            }
        }
    }
}
