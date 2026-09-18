use crate::jet_generated_format as jet_format;
use crate::Codegen::is_db_value_type_name;
use crate::Codegen::is_db_value_variant;
use crate::Codegen::is_json_type_name;
use crate::Codegen::is_json_variant;
use crate::Codegen::is_key_variant;
use crate::Codegen::mangle;
use crate::Codegen::Cx;
use crate::Codegen::TIR::call_return_type_with_args;
use crate::Codegen::TIR::clone_env;
use crate::Codegen::TIR::core_enum_equal_type;
use crate::Codegen::TIR::data_plan_for_core_call;
use crate::Codegen::TIR::duration_new_unit;
use crate::Codegen::TIR::fn_field_call_ty;
use crate::Codegen::TIR::game_static_type;
use crate::Codegen::TIR::handle_method_op;
use crate::Codegen::TIR::handle_method_return_ty;
use crate::Codegen::TIR::imported_module_call_target_return;
use crate::Codegen::TIR::is_app_method_name;
use crate::Codegen::TIR::is_civil_time_method_name;
use crate::Codegen::TIR::is_concurrency_method_name;
use crate::Codegen::TIR::is_devserver_method_name;
use crate::Codegen::TIR::is_event_handle_type;
use crate::Codegen::TIR::is_event_method_name;
use crate::Codegen::TIR::is_http_method_name;
use crate::Codegen::TIR::is_http_type;
use crate::Codegen::TIR::is_loadable_method_name;
use crate::Codegen::TIR::is_measurement_method_name;
use crate::Codegen::TIR::is_reactive_effect_method_name;
use crate::Codegen::TIR::is_reactive_method_name;
use crate::Codegen::TIR::is_sketch_method_name;
use crate::Codegen::TIR::is_sketch_type;
use crate::Codegen::TIR::is_ui_backend_method_name;
use crate::Codegen::TIR::lower_debug_text;
use crate::Codegen::TIR::lower_extern_call_arg;
use crate::Codegen::TIR::module_call_source_return_type;
use crate::Codegen::TIR::module_call_target_return;
use crate::Codegen::TIR::preserve_typed_list_shape;
use crate::Codegen::TIR::TFailureCarrier;
use crate::Codegen::TIR::THardwareCall;
use crate::Codegen::TIR::TStmt;
use crate::Codegen::TIR::{allocator_constructor_owner, TAllocCtor};
use crate::AST::{
    AccessConvention, BinOp, EnumLitArg, Expr, Lambda, LambdaBody, LambdaMeta, LambdaParam, Stmt,
    StrPart, Type,
};
use crate::Syntax::{CoreCallInterpreterRoute, CoreCallProjectionError, CoreCallRecord};

const TIR_CORE_CALL_RECORDS: &[CoreCallRecord] = &[
    CoreCallRecord::new("core.reflect", "of", "jet_reflect_of", true, &[true]),
    CoreCallRecord::new(
        "core.http.server",
        "cors_policy",
        "jet_http_cors_policy",
        false,
        &[true, true, true, true, true, true, true],
    )
    .with_max_arity(7)
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient)
    .with_jit_symbol("jet_jit_http_cors_policy")
    .without_direct_aot(),
    CoreCallRecord::new(
        "core.archive",
        "zip_decompress",
        "jet_foundation::CoreArchive::jet_archive_zip_decompress",
        false,
        &[true],
    )
    .with_jit_symbol("jet_jit_zip_decompress"),
    CoreCallRecord::new(
        "core.archive",
        "deflate",
        "jet_foundation::CoreArchive::jet_archive_deflate",
        false,
        &[true],
    )
    .with_jit_symbol("jet_jit_archive_deflate"),
    CoreCallRecord::new(
        "core.archive",
        "crc32",
        "jet_foundation::CoreArchive::jet_archive_crc32",
        false,
        &[true],
    )
    .with_jit_symbol("jet_jit_archive_crc32"),
    CoreCallRecord::new(
        "core.archive",
        "adler32",
        "jet_foundation::CoreArchive::jet_archive_adler32",
        false,
        &[true],
    )
    .with_jit_symbol("jet_jit_archive_adler32"),
    CoreCallRecord::new(
        "core.encoding.json", "canonical", "jet_enc_json_canonical", true, &[true],
    ).with_max_arity(2).with_jit_symbol("jet_jit_json_canonical"),
    CoreCallRecord::new(
        "core.encoding.xml", "expanded_name", "jet_std_xml_expanded_name", true, &[true],
    ).with_jit_symbol("jet_jit_xml_expanded_name"),
    CoreCallRecord::new(
        "core.email",
        "smtp",
        "jet_email::smtp",
        true,
        &[true],
    )
    .with_jit_symbol("jet_jit_email_smtp")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.email",
        "smtp_from_env",
        "jet_email::smtp_from_env",
        true,
        &[],
    )
    .with_jit_symbol("jet_jit_email_smtp_from_env")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.encoding.toml", "decode", "jet_enc_toml_decode", true, &[true],
    ).without_direct_jit(),
    CoreCallRecord::new(
        "core.encoding.yaml", "decode", "jet_enc_yaml_decode", true, &[true],
    ).without_direct_jit(),
    CoreCallRecord::new(
        "core.data", "bar_svg", "jet_data_bar_svg_checked", true, &[true],
    ).with_jit_symbol("jet_jit_data_bar_svg"),
    CoreCallRecord::new(
        "core.data", "mean", "jet_data_mean_checked", true, &[true],
    ).without_direct_jit().with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.sync", "text_new", "jet_sync_text_new", true, &[false, false],
    ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.auth", "register_user", "jet_auth_register_user", true, &[false, false],
    ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.auth", "password_login", "jet_auth_password_login", true,
        &[false, false, false, false],
    ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.auth", "magic_link_issue", "jet_auth_magic_link_issue", true,
        &[false, false, false],
    ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.auth", "magic_link_consume", "jet_auth_magic_link_consume", true,
        &[false, false, false],
    ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.auth", "oauth_begin", "jet_auth_oauth_begin", true, &[false],
    ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.auth", "oauth_finish", "jet_auth_oauth_finish", true,
        &[false, false, false, false],
    ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto", "constant_time_equal_bytes", "jet_crypto_constant_time_equal_bytes_impl",
        false, &[true, true],
    ).with_jit_symbol("jet_jit_crypto_constant_time_equal_bytes")
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto", "constant_time_equal", "jet_crypto_constant_time_secret_impl",
        false, &[true, true],
    ).with_jit_symbol("jet_jit_crypto_constant_time_equal")
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto", "hkdf_sha256", "jet_crypto_hkdf_sha256_impl", false,
        &[true, true, true, false],
    ).with_jit_symbol("jet_jit_crypto_hkdf_sha256")
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto", "x25519_public", "jet_crypto_x25519_public_impl", false, &[true],
    ).with_jit_symbol("jet_jit_crypto_x25519_public_from_bytes")
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto", "__x25519_public", "jet_crypto_x25519_public_typed_impl", false,
        &[true],
    )
    .with_jit_symbol("jet_jit_crypto_x25519_public")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto",
        "__x25519_public_text",
        "jet_crypto_x25519_public_text_impl",
        false,
        &[true],
    )
    .with_jit_symbol("jet_jit_crypto_x25519_public_text")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto", "__signing_public", "jet_crypto_signing_public_impl", false, &[true],
    )
    .with_jit_symbol("jet_jit_crypto_signing_public")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto", "x25519_shared", "jet_crypto_x25519_shared_impl", false, &[true, true],
    ).with_jit_symbol("jet_jit_crypto_x25519_shared")
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto", "x25519", "jet_crypto_x25519_typed_impl", false, &[true, false],
    ).with_jit_symbol("jet_jit_crypto_x25519")
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto.expert", "x25519_raw", "jet_crypto_expert_x25519_impl", false, &[true, true],
    ).with_jit_symbol("jet_jit_crypto_expert_x25519_raw")
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto", "wrap", "jet_crypto_wrap_typed_impl", false, &[true, false],
    ).with_jit_symbol("jet_jit_crypto_wrap")
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto", "unwrap", "jet_crypto_unwrap_typed_impl", false, &[true, false],
    ).with_jit_symbol("jet_jit_crypto_unwrap")
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto", "sign", "jet_crypto_sign_typed_impl", false, &[true, true],
    ).with_jit_symbol("jet_jit_crypto_sign")
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto", "verify", "jet_crypto_verify_typed_impl", false, &[true, true, true],
    )
    .with_jit_symbol("jet_jit_crypto_verify")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto", "seal", "jet_crypto_seal_typed_impl", false, &[false, true, true],
    ).with_jit_symbol("jet_jit_crypto_seal")
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto", "open", "jet_crypto_open_typed_impl", false, &[true, false, true],
    )
    .with_jit_symbol("jet_jit_crypto_open")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.sync", "counter_new", "jet_sync_counter_new", true, &[false, false],
    ).with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.sync",
        "list_push",
        "jet_sync_list_push",
        true,
        &[false, false, false],
    )
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.sync", "counter_inc", "jet_sync_counter_inc", true, &[true, false, false],
    )
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.sync", "map_set", "jet_sync_map_set", true, &[false, false, false],
    )
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.web.storage.local", "set", "jet_web_storage_set", true, &[true, true],
    ).with_interpreter_route(CoreCallInterpreterRoute::Ambient)
        .without_direct_aot().without_direct_jit(),
    CoreCallRecord::new(
        "core.net", "dns_ptr", "jet_net_dns_ptr", true, &[true, false],
    )
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient)
    .with_jit_symbol("jet_jit_net_dns_ptr"),
    CoreCallRecord::new(
        "core.net", "dns_txt", "jet_net_dns_txt", true, &[true, false],
    )
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient)
    .with_jit_symbol("jet_jit_net_dns_txt"),
    CoreCallRecord::new(
        "core.net", "dns_srv", "jet_net_dns_srv", true, &[true, false],
    )
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient)
    .with_jit_symbol("jet_jit_net_dns_srv"),
    CoreCallRecord::new(
        "core.crypto", "__secret_from_bytes", "jet_crypto_secret_from_bytes_impl", false, &[true],
    )
    .with_jit_symbol("jet_jit_crypto_secret_from_bytes")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto.expert", "open_v1", "jet_crypto_expert_open_v1_impl", false, &[true, true],
    )
    .with_jit_symbol("jet_jit_crypto_expert_open_v1")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto.vault", "get", "jet_vault_get_impl", false, &[true],
    )
    .with_jit_symbol("jet_jit_vault_get")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient)
    .without_direct_aot(),
    CoreCallRecord::new(
        "app", "auth", "jet_app_auth", true, &[true],
    )
    .with_jit_symbol("jet_jit_app_auth")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "app", "auth_oauth", "jet_app_auth_oauth", true, &[true, true],
    )
    .with_jit_symbol("jet_jit_app_auth_oauth")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.encoding.json", "writer", "jet_jit_json_writer", true, &[false, false],
    )
    .with_max_arity(3)
    .with_jit_symbol("jet_jit_json_writer")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.encoding.json", "reader", "jet_jit_json_reader", true, &[false],
    )
    .with_max_arity(2)
    .with_jit_symbol("jet_jit_json_reader")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new("core.compiler", "lex", "jet_compiler_lex", true, &[true])
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.compiler", "parse", "jet_compiler_parse", true, &[true])
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.compiler", "check", "jet_compiler_check", true, &[true])
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.compiler", "source_map", "jet_compiler_source_map", true, &[true])
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.compiler", "manifest", "jet_compiler_manifest", true, &[])
        .with_max_arity(1)
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.compiler", "package", "jet_compiler_package", true, &[])
        .with_max_arity(1)
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.compiler", "lock", "jet_compiler_lock", true, &[])
        .with_max_arity(1)
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new("core.compiler", "profiles", "jet_compiler_profiles", true, &[])
        .with_interpreter_route(CoreCallInterpreterRoute::Ambient)
        .without_direct_aot()
        .without_direct_jit(),
    CoreCallRecord::new(
        "core.crypto.expert", "migrate_v1", "jet_crypto_expert_migrate_v1_impl", false, &[true, true, true, true],
    )
    .with_jit_symbol("jet_jit_crypto_expert_migrate_v1")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto", "__password_text", "jet_crypto_password_text_impl", false, &[true],
    )
    .with_jit_symbol("jet_jit_crypto_password_text")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto", "file_open", "jet_crypto_file_open_impl", false, &[true, true, true],
    )
    .with_jit_symbol("jet_jit_crypto_file_open")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
    CoreCallRecord::new(
        "core.crypto", "file_seal", "jet_crypto_file_seal_impl", false, &[true, true, true],
    )
    .with_jit_symbol("jet_jit_crypto_file_seal")
    .with_interpreter_route(CoreCallInterpreterRoute::Ambient),
];

pub(crate) fn tir_core_call_records() -> &'static [CoreCallRecord] {
    TIR_CORE_CALL_RECORDS
}


/// Preserve checked Core type arguments, including inferred encoding row types.
fn checked_core_type_args(
    module: &str,
    method: &str,
    type_args: &[Type],
    args: &[TExpr],
) -> Vec<Type> {
    if module == "core.models" && method == "open" {
        type_args
            .iter()
            .map(|ty| match ty {
                Type::Named(name) => Type::TraitObject(vec![name.clone()]),
                _ => ty.clone(),
            })
            .collect()
    } else if module == "core.encoding.json" && matches!(method, "to_string" | "to_string_pretty") {
        // JSON rendering lowers every value to the dynamic Data carrier before
        // the direct renderer call; source type arguments must not describe
        // the pre-lowering value.
        Vec::new()
    } else if module == "core.encoding.csv" && method == "to_string" && type_args.is_empty() {
        match args.first().map(|arg| arg.ty.without_user_tags()) {
            Some(Type::List(inner) | Type::FixedList { elem: inner, .. }) if !matches!(inner.without_user_tags(), Type::List(cell) if **cell == Type::String) =>
            {
                vec![(**inner).clone()]
            }
            _ => Vec::new(),
        }
    } else {
        type_args.to_vec()
    }
}

fn retag_empty_collection_args(args: &mut [TCallArg], sig: &[(AccessConvention, Type)]) {
    for (arg, (_, expected)) in args.iter_mut().zip(sig.iter()) {
        let empty = matches!(&arg.value.kind, TExprKind::MapLit(entries) if entries.is_empty())
            || matches!(&arg.value.kind, TExprKind::ListLit(items) if items.is_empty());
        if empty
            && matches!(
                expected.without_user_tags(),
                Type::Map { .. } | Type::List(_) | Type::FixedList { .. }
            )
        {
            arg.value.ty = expected.clone();
        }
    }
}
fn is_fixed_float_add_type(ty: &Type) -> bool {
    matches!(ty, Type::Float | Type::Float32)
}

fn is_pure_add_lambda(lambda: &Lambda) -> bool {
    if !lambda.take_names.is_empty()
        || lambda.params.len() != 2
        || lambda.meta.needs_fn_mut
        || lambda.meta.effect_maximal
        || !lambda.meta.effect_direct.is_empty()
        || !lambda.meta.effect_solved.is_empty()
        || !lambda.meta.effect_call_edges.is_empty()
        || !lambda.meta.mut_captures.is_empty()
        || !lambda.meta.cloned_captures.is_empty()
        || !lambda.meta.frozen_captures.is_empty()
        || !lambda.meta.materialized_captures.is_empty()
        || !lambda.meta.moved_captures.is_empty()
    {
        return false;
    }
    let [left_param, right_param] = lambda.params.as_slice() else {
        return false;
    };
    let LambdaBody::Expr(body) = &lambda.body else {
        return false;
    };
    matches!(
        body.as_ref(),
        Expr::Binary(
            BinOp::Add,
            lhs,
            rhs,
            _
        ) if matches!(lhs.as_ref(), Expr::Ident(name, _) if name == &left_param.name)
            && matches!(rhs.as_ref(), Expr::Ident(name, _) if name == &right_param.name)
    )
}

fn is_pure_zero_arg_lambda(lambda: &Lambda) -> Option<&Expr> {
    if !lambda.take_names.is_empty()
        || !lambda.params.is_empty()
        || lambda.meta.needs_fn_mut
        || lambda.meta.effect_maximal
        || !lambda.meta.effect_direct.is_empty()
        || !lambda.meta.effect_solved.is_empty()
        || !lambda.meta.effect_call_edges.is_empty()
        || !lambda.meta.mut_captures.is_empty()
        || !lambda.meta.cloned_captures.is_empty()
        || !lambda.meta.frozen_captures.is_empty()
        || !lambda.meta.materialized_captures.is_empty()
        || !lambda.meta.moved_captures.is_empty()
    {
        return None;
    }
    match &lambda.body {
        LambdaBody::Expr(body) => Some(body.as_ref()),
        LambdaBody::Block(_) => None,
    }
}

fn fixed_float_add_closure_op(
    method: &str,
    recv_ty: &Type,
    result_ty: &Type,
    args: &[crate::AST::CallArg],
) -> Option<TClosureOp> {
    let item_ty = match recv_ty {
        Type::List(elem) | Type::FixedList { elem, .. } => elem.as_ref(),
        _ => return None,
    };
    if !is_fixed_float_add_type(item_ty) || !is_fixed_float_add_type(result_ty) {
        return None;
    }
    let f32 = matches!(item_ty, Type::Float32);
    if f32 != matches!(result_ty, Type::Float32) {
        return None;
    }
    match method {
        "fold" if args.len() == 2 => {
            let Expr::Lambda(lambda) = &args[1].expr else {
                return None;
            };
            is_pure_add_lambda(lambda).then_some(TClosureOp::FloatAddFold { f32 })
        }
        "para_fold" if args.len() == 3 && matches!(recv_ty, Type::List(_)) => {
            let Expr::Lambda(seed) = &args[0].expr else {
                return None;
            };
            let Expr::Lambda(step) = &args[1].expr else {
                return None;
            };
            let Expr::Lambda(merge) = &args[2].expr else {
                return None;
            };
            let _ = is_pure_zero_arg_lambda(seed)?;
            if !is_pure_add_lambda(step) || !is_pure_add_lambda(merge) {
                return None;
            }
            Some(TClosureOp::FloatAddParaFold { f32 })
        }
        _ => None,
    }
}
fn unit_ratio_as_f64(value: &crate::AST::UnitRatio) -> Option<f64> {
    let numerator = value.num.to_string().parse::<f64>().ok()?;
    let denominator = value.den.to_string().parse::<f64>().ok()?;
    Some(numerator / denominator)
}
fn plugin_component_type(
    cx: &Cx,
    ty: &Type,
    seen: &mut HashSet<String>,
) -> Option<jet_foundation::MIR::ComponentTypeDescriptor> {
    use jet_foundation::MIR::ComponentTypeDescriptor;
    match ty {
        Type::Int => Some(ComponentTypeDescriptor::Int),
        Type::Float => Some(ComponentTypeDescriptor::Float),
        Type::Bool => Some(ComponentTypeDescriptor::Bool),
        Type::String => Some(ComponentTypeDescriptor::String),
        Type::List(inner) | Type::FixedList { elem: inner, .. } => Some(
            ComponentTypeDescriptor::List(Box::new(plugin_component_type(cx, inner, seen)?)),
        ),
        Type::InlineRange { base: inner, .. } | Type::Tagged { inner, .. } => {
            plugin_component_type(cx, inner, seen)
        }
        Type::Option(inner) => Some(ComponentTypeDescriptor::Option(Box::new(
            plugin_component_type(cx, inner, seen)?,
        ))),
        Type::Result { ok, err } => Some(ComponentTypeDescriptor::Result {
            ok: Box::new(plugin_component_type(cx, ok, seen)?),
            err: Box::new(plugin_component_type(cx, err, seen)?),
        }),
        Type::Named(name) | Type::Apply { name, .. } => {
            if !seen.insert(name.clone()) {
                return None;
            }
            let result = cx.struct_fields.get(name).and_then(|fields| {
                fields
                    .iter()
                    .enumerate()
                    .map(|(index, (field, field_ty))| {
                        Some((
                            index,
                            field.clone(),
                            plugin_component_type(cx, field_ty, seen)?,
                        ))
                    })
                    .collect::<Option<Vec<_>>>()
                    .map(|fields| ComponentTypeDescriptor::Record {
                        name: name.clone(),
                        fields,
                    })
            });
            seen.remove(name);
            result
        }
        _ => None,
    }
}

fn plugin_signature_descriptor(
    cx: &Cx,
    params: &[TExpr],
    result: &Type,
) -> Option<jet_foundation::MIR::ComponentSignatureDescriptor> {
    let mut seen = HashSet::new();
    Some(jet_foundation::MIR::ComponentSignatureDescriptor {
        params: params
            .iter()
            .map(|param| plugin_component_type(cx, &param.ty, &mut seen))
            .collect::<Option<Vec<_>>>()?,
        result: plugin_component_type(cx, result, &mut seen)?,
    })
}

// D-TYPE2-DEFAULT1: rational math crosses into the approximate world through
// the existing precise Fraction Prelude bridge before the libm call.
fn exact_rational_math_approx(method: &str) -> bool {
    matches!(
        method,
        "sqrt"
            | "sin"
            | "cos"
            | "tan"
            | "asin"
            | "acos"
            | "atan"
            | "sinh"
            | "cosh"
            | "tanh"
            | "exp"
            | "ln"
            | "log2"
            | "log10"
            | "acosh"
            | "asinh"
            | "atanh"
            | "cbrt"
            | "exp2"
            | "exp_m1"
            | "ln_1p"
            | "degrees"
            | "radians"
    )
}

/// Imported foreign functions preserve their source-declared bridge ABI.
///
/// C wrappers stop through the runtime boundary for conversion failures; they
/// do not return the callable's implicit `Result<_, JetErr>` carrier.  A
/// declared `Result` remains a carrier because it is part of the bridge ABI.
fn imported_extern_call_type(_c_abi: bool, declared: Type) -> Type {
    declared
}
use crate::Codegen::TIR::fixed_list_elem_compatible;
use crate::Codegen::TIR::http_client_static_op;
use crate::Codegen::TIR::is_watch_handle_type;
use crate::Codegen::TIR::is_watch_method_name;
use crate::Codegen::TIR::jit_spawn_site;
use crate::Codegen::TIR::jit_spawn_site_with;
use crate::Codegen::TIR::lambda_body_ty_expecting;
use crate::Codegen::TIR::lower::core_module_path_from_receiver;
use crate::Codegen::TIR::lower::in_own_frame;
use crate::Codegen::TIR::lower::consume_plain_helper_route;
use crate::Codegen::TIR::lower::lower_cursor_take_pattern;
use crate::Codegen::TIR::lower::lower_reader_take_pattern;
use crate::Codegen::TIR::lower::static_call_type_name_lower;
use crate::Codegen::TIR::lower_core_closure_call;
use crate::Codegen::TIR::lower_enum_arg;
use crate::Codegen::TIR::lower_expr;
use crate::Codegen::TIR::lower_expr_as_mut_place;
use crate::Codegen::TIR::lower_fn_value_call;
use crate::Codegen::TIR::lower_lambda;
use crate::Codegen::TIR::lower_lambda_expecting;
use crate::Codegen::TIR::lower_lambda_expecting_host_borrow;
use crate::Codegen::TIR::lower_lambda_expecting_value;
use crate::Codegen::TIR::lower_lambda_expecting_value_with_return;
use crate::Codegen::TIR::lower_method_args;
use crate::Codegen::TIR::lower_module_args;
use crate::Codegen::TIR::lower_named_collection_callback;
use crate::Codegen::TIR::lower_one_call_arg;
use crate::Codegen::TIR::lower_owned_expr;
use crate::Codegen::TIR::lower_spawn_lambda_for_jit;
use crate::Codegen::TIR::lower_spawn_lambda_for_jit_expecting;
use crate::Codegen::TIR::pool_field_ty_hint;
use crate::Codegen::TIR::preserve_source_arg_order;
use crate::Codegen::TIR::resolve_builtin_op;
use crate::Codegen::TIR::resolve_closure_op;
use crate::Codegen::TIR::resolve_numeric_conversion_op;
use crate::Codegen::TIR::resolve_numeric_op;
use crate::Codegen::TIR::resolve_self_ty;
use crate::Codegen::TIR::solve_new_type;
use crate::Codegen::TIR::source_arg_order;
use crate::Codegen::TIR::spawn_body_carrier_ty;
use crate::Codegen::TIR::spawn_label;
use crate::Codegen::TIR::struct_field_type;
use crate::Codegen::TIR::tir_recv_jet_ty;
use crate::Codegen::TIR::tls_static_op;
use crate::Codegen::TIR::unit_type;
use crate::Codegen::TIR::wrap_foreign_undo;
use crate::Codegen::TIR::LowerEnv;
use crate::Codegen::TIR::TBuiltinOp;
use crate::Codegen::TIR::TCallArg;
use crate::Codegen::TIR::TClosureOp;
use crate::Codegen::TIR::TCoreClosureKind;
use crate::Codegen::TIR::TEnumArg;
use crate::Codegen::TIR::TEnumPayload;
use crate::Codegen::TIR::TExpr;
use crate::Codegen::TIR::TExprKind;
use crate::Codegen::TIR::TFnValueKind;
use crate::Codegen::TIR::THandleOp;
use crate::Codegen::TIR::THostCall;
use crate::Codegen::TIR::TLocal;
use crate::Codegen::TIR::TMethodRef;
use crate::Codegen::TIR::TModuleCallForm;
use crate::Codegen::TIR::TNumericOp;
use crate::Codegen::TIR::TPreludeArg;
use crate::Codegen::TIR::TStaticOwner;
use crate::Codegen::TIR::TStrPart;
use crate::Diagnostics::Span;
use crate::Syntax;
use std::collections::HashSet;
fn operator_trait_for_method(method: &str) -> Option<&'static str> {
    match method {
        "add" => Some(Syntax::TRAIT_ADD),
        "sub" => Some(Syntax::TRAIT_SUB),
        "mul" => Some(Syntax::TRAIT_MUL),
        "div" => Some(Syntax::TRAIT_DIV),
        "equal" => Some(Syntax::TRAIT_EQUATABLE),
        "compare" => Some(Syntax::TRAIT_COMPARABLE),
        _ => None,
    }
}

fn lower_builtin_binary_method(
    method: &str,
    mut recv: TExpr,
    mut rhs: TExpr,
    ret_ty: Type,
    span: Span,
    cx: &Cx,
) -> TExpr {
    let op = match method {
        "add" => crate::AST::BinOp::Add,
        "sub" => crate::AST::BinOp::Sub,
        "mul" => crate::AST::BinOp::Mul,
        "div" => crate::AST::BinOp::Div,
        "equal" => crate::AST::BinOp::Eq,
        "compare" => crate::AST::BinOp::Compare,
        _ => unreachable!("checked builtin binary method"),
    };
    let distinct = match recv.ty.without_user_tags() {
        Type::Named(name) => cx
            .distinct_types
            .get(name)
            .map(|(base, _)| (name.clone(), base.clone())),
        _ => None,
    };
    // Bundle arithmetic has no Jet method body. Project to the checked base,
    // use its ordinary numeric route, then restore the nominal result.
    if let Some((_, base)) = &distinct {
        recv = TExpr {
            ty: base.clone(),
            kind: TExprKind::DistinctRaw(Box::new(recv)),
        };
        rhs = TExpr {
            ty: base.clone(),
            kind: TExprKind::DistinctRaw(Box::new(rhs)),
        };
    }
    let overflow = matches!(
        op,
        crate::AST::BinOp::Add
            | crate::AST::BinOp::Sub
            | crate::AST::BinOp::Mul
            | crate::AST::BinOp::Div
    ) && (recv.ty.is_integer() || rhs.ty.is_integer());
    let binary = TExpr {
        ty: distinct
            .as_ref()
            .map_or_else(|| ret_ty.clone(), |(_, base)| base.clone()),
        kind: TExprKind::Binary {
            op,
            overflow,
            line: crate::Diagnostics::span_line_col(&cx.src, span.start).0 as u32,
            lhs: Box::new(recv),
            rhs: Box::new(rhs),
        },
    };
    match distinct {
        Some((name, base)) => TExpr {
            ty: ret_ty,
            kind: TExprKind::DistinctCtor {
                name,
                arg: Box::new(binary),
                base,
            },
        },
        None => binary,
    }
}

/// Keep a selected lowering total without inventing a type or delegating an
/// impossible shape to a backend-specific expression.
fn invariant_method_expr(span: Span, construct: impl Into<String>) -> TExpr {
    TExpr {
        ty: Type::Named(Syntax::TYPE_NEVER.to_string()),
        kind: TExprKind::InvariantViolation {
            construct: construct.into(),
            span,
        },
    }
}

/// One projection for both TIR admission and lowering of checked web receivers.
pub(crate) fn web_receiver_projection<'a>(
    receiver: Option<&str>,
    method: &'a str,
    arity: usize,
) -> Option<(&'static str, &'a str)> {
    match (receiver, method, arity) {
        (Some("WebTable"), "with_column", 1) => Some(("core.web.table", "with_column")),
        (Some("WebTable"), "paginate", 2) => Some(("core.web.table", "paginate")),
        (Some("WebTable"), "page", 0) => Some(("core.web.table", "page_state")),
        (Some("WebStore"), "set", 1) => Some(("core.web.store", "set")),
        (Some("WebStore"), "signal", 0) => Some(("core.web.store", "signal")),
        (Some("WebStore"), "state_signal", 0) => Some(("core.web.store", "state_signal")),
        (Some("WebStore"), "facts_json", 0) => Some(("core.web.store", "facts_json")),
        (
            Some("WebQuery"),
            "state" | "state_signal" | "mutation_state" | "mutation_signal" | "invalidate" | "get"
            | "show" | "facts" | "cancel" | "refresh",
            0,
        ) => Some(("core.web.query", method)),
        (Some("WebFormTyped"), "set_async_validator", 4) => {
            Some(("core.web.forms", "typed_set_async_validator"))
        }
        (Some("WebFormTyped"), "set_action", 1) => Some(("core.web.forms", "typed_set_action")),
        (Some("WebFormTyped"), "set", 2) => Some(("core.web.forms", "typed_set")),
        (Some("WebFormTyped"), "blur", 1) => Some(("core.web.forms", "typed_blur")),
        (Some("WebFormTyped"), "focus", 1) => Some(("core.web.forms", "typed_focus")),
        (Some("WebFormTyped"), "post", 1) => Some(("core.web.forms", "typed_post")),
        (Some("WebFormTyped"), "validate", 0) => Some(("core.web.forms", "typed_validate")),
        (Some("WebFormTyped"), "validate", 3) => Some(("core.web.forms", "typed_validate_field")),
        (Some("WebFormValidationChain"), "render", 0) => {
            Some(("core.web.forms", "typed_validation_render"))
        }
        (Some("WebFormTyped"), "cancel", 0) => Some(("core.web.forms", "typed_cancel")),
        (Some("WebFormTyped"), "submit", 0) => Some(("core.web.forms", "typed_submit")),
        (Some("WebFormTyped"), "no_script", 0) => Some(("core.web.forms", "typed_no_script")),
        (Some("WebFormTyped"), "state", 0) => Some(("core.web.forms", "typed_state")),
        (Some("WebFormTyped"), "lifecycle", 0) => Some(("core.web.forms", "typed_lifecycle")),
        (Some("WebFormTyped"), "errors", 0) => Some(("core.web.forms", "typed_errors")),
        (Some("WebFormTyped"), "render", 0) => Some(("core.web.forms", "typed_html")),
        _ => None,
    }
}

fn checked_core_record(
    module: &str,
    method: &str,
    arity: usize,
    span: Span,
) -> Result<&'static crate::Syntax::CoreCallRecord, TExpr> {
    // `process.run(argv, authority)` is the checked authority-boundary form
    // of the one public call. Its binary ABI has a distinct registry row so
    // AOT/JIT never call the unary symbol with an extra argument.
    let (lookup_module, lookup_method) =
        if module == "core.process" && method == "run" && arity == 2 {
            ("core.process", "run_with_authority")
        } else {
            (module, method)
        };
    crate::Syntax::core_call_projection(
        lookup_module,
        lookup_method,
        crate::Syntax::CoreCallCoverage::TIR_SUBSET,
        arity,
    )
    .or_else(|error| match error {
        CoreCallProjectionError::Unknown => crate::Syntax::core_call_projection_in(
            TIR_CORE_CALL_RECORDS,
            lookup_module,
            lookup_method,
            crate::Syntax::CoreCallCoverage::TIR_SUBSET,
            arity,
        ),
        error => Err(error),
    })
    .map_err(|error| {
        invariant_method_expr(
            span,
            format!(
                "checked Core call `{module}.{method}` has no canonical TIR record ({error:?})",
            ),
        )
    })
}
/// Lower the sema-resolved polymorphic `core.math.abs` call into the typed
/// MathBuiltin family. The type name selects the shared Prelude route; the
/// resolved argument and return types keep backend dispatch from guessing.
fn lower_core_math_abs(
    args: &[crate::AST::CallArg],
    resolved_ret: Option<&Type>,
    span: Span,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TExpr {
    if args.len() != 1 {
        return invariant_method_expr(
            span,
            format!(
                "checked Core call `core.math.abs` has wrong arity {}",
                args.len()
            ),
        );
    }
    let Some(ret) = resolved_ret.cloned() else {
        return invariant_method_expr(
            span,
            "checked Core call `core.math.abs` has no resolved return type",
        );
    };
    let arg = lower_expr(&args[0].expr, cx, env);
    let type_name = match (&arg.ty, &ret) {
        (Type::Int, Type::Int) => "Int",
        (Type::Float, Type::Float) => "Float",
        (Type::Float32, Type::Float32) => "F32",
        (Type::Named(name), Type::Float) if name == Syntax::TYPE_COMPLEX => "Complex",
        _ => {
            return invariant_method_expr(
                span,
                format!(
                    "checked Core call `core.math.abs` has unsupported type pair {} -> {}",
                    arg.ty.name(),
                    ret.name()
                ),
            );
        }
    };
    TExpr {
        ty: ret,
        kind: TExprKind::MathBuiltin {
            type_name: type_name.to_string(),
            func: "abs".to_string(),
            args: vec![arg],
        },
    }
}
/// Lower sema-resolved polymorphic `core.math` numeric predicates through the
/// typed numeric predicate family used by receiver methods. These calls do not
/// share the fixed CoreCall ABI because sema resolves their argument type.
fn lower_core_math_predicate(
    method: &str,
    args: &[crate::AST::CallArg],
    resolved_ret: Option<&Type>,
    span: Span,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TExpr {
    let predicate = match method {
        "is_nan" => "is_nan",
        "is_inf" => "is_infinite",
        "is_finite" => "is_finite",
        _ => {
            return invariant_method_expr(
                span,
                format!("checked Core call `core.math.{method}` has no predicate route"),
            );
        }
    };
    if args.len() != 1 {
        return invariant_method_expr(
            span,
            format!(
                "checked Core call `core.math.{method}` has wrong arity {}",
                args.len()
            ),
        );
    }
    let Some(ret) = resolved_ret.cloned() else {
        return invariant_method_expr(
            span,
            format!("checked Core call `core.math.{method}` has no resolved return type"),
        );
    };
    let arg = lower_expr(&args[0].expr, cx, env);
    if !matches!(
        (&arg.ty, &ret),
        (Type::Float | Type::Float32, Type::Bool)
    ) {
        return invariant_method_expr(
            span,
            format!(
                "checked Core call `core.math.{method}` has unsupported type pair {} -> {}",
                arg.ty.name(),
                ret.name()
            ),
        );
    }
    let source_name = arg.ty.name();
    let line = crate::Diagnostics::span_line_col(&cx.src, span.start).0 as u32;
    let Some(op) = resolve_numeric_op(predicate, &source_name, line) else {
        return invariant_method_expr(
            span,
            format!("checked Core call `core.math.{method}` has no numeric predicate route"),
        );
    };
    TExpr {
        ty: ret,
        kind: TExprKind::NumericMethod {
            recv: Box::new(arg),
            op,
        },
    }
}
fn plot_selector_name(expr: &Expr) -> Option<&str> {
    match expr {
        Expr::EnumLit {
            type_name,
            variant,
            args,
            leading_dot: true,
            ..
        } if type_name.is_empty() && args.is_empty() => Some(variant.as_str()),
        Expr::Paren(inner, _) => plot_selector_name(inner),
        _ => None,
    }
}

fn plot_value_variant(ty: &Type) -> Option<&'static str> {
    match ty.without_user_tags() {
        Type::InlineRange { base, .. } => plot_value_variant(base),
        Type::Int | Type::IntN { .. } => Some("Integer"),
        Type::Float | Type::Float32 => Some("Number"),
        Type::Bool => Some("Boolean"),
        Type::String => Some("Text"),
        _ => None,
    }
}

fn plot_call_arg(value: TExpr) -> TCallArg {
    TCallArg {
        value,
        template_items: None,
        borrow: false,
        mut_borrow: false,
        clone: false,
        arc_clone: false,
        fn_coerce: None,
        widen_to_vec: false,
        widen_to_union: None,
        box_as_trait: None,
    }
}

fn lower_plot_column(
    field_name: &str,
    row_ty: &Type,
    span: Span,
    cx: &Cx,
    env: &mut LowerEnv,
) -> Result<TExpr, TExpr> {
    let Some(field_ty) = struct_field_type(cx, row_ty, field_name) else {
        return Err(invariant_method_expr(
            span,
            format!("checked plot selector has no field `{field_name}`"),
        ));
    };
    let field_type_name = field_ty.name();
    let field = TExpr {
        ty: Type::Named("JetDataPlotField".to_string()),
        kind: TExprKind::StructLit {
            fields: vec![
                (
                    "id".to_string(),
                    devtools_text(cx.data_plot_field_id(field_name, &field_ty)),
                    false,
                ),
                (
                    "name".to_string(),
                    devtools_text(field_name.to_string()),
                    false,
                ),
                (
                    "type_name".to_string(),
                    devtools_text(field_type_name),
                    false,
                ),
            ],
            extra: None,
            as_trait: None,
        },
    };
    let value_type = Type::Named("JetDataPlotValue".to_string());
    let field_expr = Expr::Field(
        Box::new(Expr::Ident("__row".to_string(), span)),
        field_name.to_string(),
        span,
    );
    let Some(variant) = plot_value_variant(&field_ty) else {
        return Err(invariant_method_expr(
            span,
            format!(
                "checked plot selector field `{field_name}` has no typed plot value conversion"
            ),
        ));
    };
    let variant = variant.to_string();
    let args = vec![EnumLitArg::Positional(field_expr)];
    let lambda = Lambda {
        take_names: Vec::new(),
        params: vec![LambdaParam {
            name: "__row".to_string(),
            name_span: span,
            ty: Some(row_ty.clone()),
            ty_span: None,
        }],
        result_type: Some(value_type.clone()),
        error_type: None,
        effects: None,
        body: LambdaBody::Expr(Box::new(Expr::EnumLit {
            type_name: value_type.name(),
            variant,
            variant_span: None,
            args,
            leading_dot: false,
            span,
        })),
        span,
        meta: LambdaMeta {
            escapes: true,
            ..LambdaMeta::default()
        },
    };
    let lowered =
        lower_lambda_expecting_host_borrow(&lambda, cx, env, std::slice::from_ref(row_ty), false);
    let callback = TExpr {
        ty: Type::Fn {
            params: lowered.param_types.clone(),
            ret: lowered.ret.clone().map(Box::new),
            effect_bound: None,
            param_contract: None,
            call_metadata: None,
            return_view_provenance: None,
        },
        kind: TExprKind::Lambda(Box::new(lowered)),
    };
    let column_ty = Type::Apply {
        name: "JetDataPlotColumn".to_string(),
        args: vec![row_ty.clone()],
    };
    Ok(TExpr {
        ty: column_ty,
        kind: TExprKind::StaticCall {
            owner: TStaticOwner::Prelude {
                rooted: false,
                path: "core.data.plot".to_string(),
                generics: vec![TPreludeArg::Jet(row_ty.clone())],
            },
            owner_type: None,
            method: TMethodRef::bare("column"),
            type_args: Vec::new(),
            args: vec![plot_call_arg(field), plot_call_arg(callback)],
        },
    })
}

fn devtools_field_name(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Field(base, field, _) if matches!(base.as_ref(), Expr::Ident(name, _) if name.is_empty()) => {
            Some(field.clone())
        }
        _ => None,
    }
}

fn devtools_text(value: String) -> TExpr {
    TExpr {
        ty: Type::String,
        kind: TExprKind::StrLit(vec![TStrPart::Lit(value)]),
    }
}

fn lower_devtools_publish(
    method_span: Span,
    args: &[crate::AST::CallArg],
    resolved_ret: Option<&Type>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TExpr {
    if args.len() != 2 {
        return invariant_method_expr(
            method_span,
            "checked core.devtools.publish has an invalid argument count",
        );
    }
    let Some(field) = devtools_field_name(&args[0].expr) else {
        return invariant_method_expr(
            method_span,
            "checked core.devtools.publish has no leading-dot field selector",
        );
    };
    let Some(panel) = cx
        .devtools_registry
        .panel_for_field(&cx.devtools_package, &cx.devtools_module, &field)
        .or_else(|| {
            cx.devtools_registry
                .unique_panel_for_field(&cx.devtools_package, &field)
        })
    else {
        return invariant_method_expr(
            method_span,
            "checked core.devtools.publish has no canonical panel fact",
        );
    };
    let Some(ty) = resolved_ret.cloned() else {
        return invariant_method_expr(
            method_span,
            "checked core.devtools.publish has no resolved return type",
        );
    };
    let mut call_args = Vec::with_capacity(5);
    call_args.push(devtools_text(panel.package.clone()));
    call_args.push(devtools_text(panel.module.clone()));
    call_args.push(devtools_text(panel.function.clone()));
    call_args.push(devtools_text(field));
    call_args.push(lower_expr(&args[1].expr, cx, env));
    let record = match checked_core_record("core.devtools", "publish", call_args.len(), method_span)
    {
        Ok(record) => record,
        Err(expr) => return expr,
    };
    TExpr {
        ty: ty.clone(),
        kind: TExprKind::CoreCall {
            record,
            args: call_args,
            source_span: method_span,
            type_args: Vec::new(),
            widen_to_vec: vec![false; 5],
            data_plan: None,
            fallibility: TFailureCarrier::from_checked_type(&ty),
        },
    }
}

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
fn db_static_sql_text(expr: &TExpr) -> Option<String> {
    match &expr.kind {
        TExprKind::HostCall(host) => match host.as_ref() {
            THostCall::TypedTextInterp {
                kind: crate::Codegen::TIR::TTypedTextInterpKind::SQL,
                literals,
                holes,
                trusted_html: _,
            } => {
                let mut sql = String::new();
                for (index, literal) in literals.iter().enumerate() {
                    sql.push_str(literal);
                    if index < holes.len() {
                        sql.push('?');
                    }
                }
                Some(sql)
            }
            THostCall::TypedText {
                kind: crate::Codegen::TIR::TTypedTextForm::SQLRaw,
                arg,
            } => db_static_sql_text(arg),
            _ => None,
        },
        TExprKind::StrLit(parts) => {
            let mut sql = String::new();
            for part in parts {
                match part {
                    TStrPart::Lit(text) => sql.push_str(text),
                    TStrPart::Interp(_, _) => return None,
                }
            }
            Some(sql)
        }
        _ => None,
    }
}

fn db_static_sql_words(sql: &str) -> Vec<String> {
    let bytes = sql.as_bytes();
    let mut words = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\'' {
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\'' {
                    if bytes.get(index + 1) == Some(&b'\'') {
                        index += 2;
                    } else {
                        index += 1;
                        break;
                    }
                } else {
                    index += 1;
                }
            }
            continue;
        }
        if bytes[index] == b'"' {
            index += 1;
            let start = index;
            while index < bytes.len() && bytes[index] != b'"' {
                index += 1;
            }
            if start < index {
                words.push(sql[start..index].to_ascii_lowercase());
            }
            index = index.saturating_add(1);
            continue;
        }
        if bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_' {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
            words.push(sql[start..index].to_ascii_lowercase());
        } else {
            index += 1;
        }
    }
    words
}

fn db_static_table_facts(sql: &str, method: &str) -> Vec<crate::Codegen::TIR::TDbTableFact> {
    let words = db_static_sql_words(sql);
    let read = matches!(method, "query" | "query_one");
    let mut tables = Vec::new();
    for (index, word) in words.iter().enumerate() {
        let relation = match word.as_str() {
            "from" | "join" if read => words.get(index + 1),
            "into" | "update" | "table" if !read => words.get(index + 1),
            _ => None,
        };
        let Some(relation) = relation else {
            continue;
        };
        if relation.is_empty()
            || matches!(
                relation.as_str(),
                "select" | "where" | "values" | "set" | "returning" | "on"
            )
        {
            continue;
        }
        tables.push(relation.clone());
    }
    tables.sort();
    tables.dedup();
    tables
        .into_iter()
        .map(|table_id| crate::Codegen::TIR::TDbTableFact {
            table_id,
            read,
            write: !read,
        })
        .collect()
}

fn db_query_metadata(
    method: &str,
    method_span: Span,
    cx: &Cx,
    sql: Option<&TExpr>,
) -> crate::Codegen::TIR::TDbQueryMetadata {
    let statement_identity = format!(
        "dbstmt-{:016x}",
        jet_foundation::MIR::stable_id(
            "db-statement",
            &format!(
                "{}:{}:{}:{}",
                cx.file, method_span.start, method_span.end, method
            ),
        )
    );
    let table_facts = sql
        .and_then(db_static_sql_text)
        .map(|sql| db_static_table_facts(&sql, method))
        .unwrap_or_default();
    crate::Codegen::TIR::TDbQueryMetadata {
        source_file: cx.file.clone(),
        source_span: method_span,
        statement_identity,
        table_facts,
    }
}

pub(crate) fn service_method_route(handle: &str, method: &str) -> Option<(&'static str, bool)> {
    let route = match (handle, method) {
        ("ServiceTree", "worker") => ("worker", true),
        ("ServiceTree", "group") => ("group", true),
        ("ServiceTree", "set_restart") => ("set_restart", true),
        ("ServiceTree", "set_delivery") => ("set_delivery", true),
        ("ServiceTree", "start") => ("start", true),
        ("ServiceTree", "stop") => ("stop", true),
        ("ServiceTree", "send") => ("send", true),
        ("ServiceTree", "send_durable") => ("send_durable", true),
        ("ServiceTree", "receive") => ("receive", true),
        ("ServiceTree", "mailbox_depth") => ("mailbox_depth", false),
        ("ServiceTree", "restarts") => ("restarts", false),
        ("ServiceTree", "fail_worker") => ("fail_worker", true),
        ("ServiceTree", "drain_worker") => ("drain_worker", true),
        ("ServiceTree", "partition_worker") => ("partition_worker", true),
        ("ServiceTree", "reconcile_worker") => ("reconcile_worker", true),
        ("ServiceTree", "dead_letter_count") => ("dead_letter_count", false),
        ("ServiceTree", "drain_dead_letters") => ("drain_dead_letters", true),
        ("ServiceTree", "event_count") => ("event_count", false),
        ("ServiceTree", "directory_generation") => ("directory_generation", false),
        ("ServiceTree", "set_state_empty") => ("set_state_empty", true),
        ("ServiceTree", "set_state_snapshot") => ("set_state_snapshot", true),
        ("ServiceTree", "set_state_event_log") => ("set_state_event_log", true),
        ("ServiceTree", "commit_snapshot") => ("commit_snapshot", true),
        ("ServiceTree", "restore_snapshot") => ("restore_snapshot", false),
        ("ServiceTree", "append_event") => ("append_event", true),
        ("ServiceTree", "replay_events") => ("replay_events", false),
        ("ServiceTree", "workflow_start") => ("workflow_start", true),
        ("ServiceTree", "workflow_step") => ("workflow_step", true),
        ("ServiceTree", "workflow_activity") => ("workflow_activity", true),
        ("ServiceTree", "workflow_activity_retry") => ("workflow_activity_retry", true),
        ("ServiceTree", "workflow_activity_complete") => ("workflow_activity_complete", true),
        ("ServiceTree", "workflow_history") => ("workflow_history", false),
        ("ServiceTree", "workflow_outcome") => ("workflow_outcome", false),
        ("ServiceWorkflow", "sleep") => ("workflow_sleep", true),
        ("ServiceWorkflow", "activity") => ("workflow_activity_wait", true),
        ("ServiceWorkflow", "all") => ("workflow_all", true),
        ("Delivery", "wait") => ("delivery_wait", false),
        ("Delivery", "status") => ("delivery_status", false),
        ("Delivery", "retry") => ("delivery_retry", false),
        ("Delivery", "cancel") => ("delivery_cancel", false),
        ("Delivery", "receipt") => ("delivery_receipt", false),
        ("Delivery", "events") => ("delivery_events", false),
        ("ServiceTree", "directory_register") => ("directory_register", true),
        ("ServiceTree", "directory_resolve") => ("directory_resolve", false),
        ("ServiceTree", "handoff_generation") => ("handoff_generation", true),
        ("ServiceTree", "rollback_generation") => ("rollback_generation", true),
        ("ServiceTree", "upgrade_receipt") => ("upgrade_receipt", false),
        ("ServiceTree", "chaos_fail") => ("chaos_fail", true),
        ("ServiceTree", "observe") => ("observe", false),
        ("ServiceTree", "show") => ("tree_show", false),
        ("ServiceEndpoint", "send") => ("endpoint_send", false),
        ("ServiceEndpoint", "receive") => ("endpoint_receive", false),
        ("ServiceEndpoint", "show") => ("endpoint_show", false),
        ("DeliveryState", "show") => ("delivery_state_show", false),
        _ => return None,
    };
    Some(route)
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

fn compute_function_type_names(ty: &Type) -> Option<Vec<String>> {
    let Type::Fn {
        params,
        param_contract,
        call_metadata,
        ..
    } = ty
    else {
        return None;
    };
    // Declaration-local names survive on the callable metadata even when
    // `param_contract` is absent (or intentionally omits implicit labels).
    // Higher-order compute transforms need those names to resolve `wrt`
    // targets without asking a backend to reconstruct sema facts.
    if let Some(names) = call_metadata
        .as_ref()
        .map(|metadata| &metadata.names)
        .filter(|names| names.len() == params.len())
    {
        return Some(names.clone());
    }
    param_contract.as_ref().and_then(|contract| {
        (contract.len() == params.len())
            .then(|| contract.iter().map(|(name, _)| name.clone()).collect())
    })
}

fn compute_transform_parameter_names(expr: &Expr, cx: &Cx) -> Option<Vec<String>> {
    match expr {
        Expr::Paren(inner, _) => compute_transform_parameter_names(inner, cx),
        Expr::Ident(name, _) => cx
            .fn_param_names
            .get(name)
            .cloned()
            .or_else(|| cx.fn_types.get(name).and_then(compute_function_type_names)),
        Expr::Lambda(lambda) => Some(
            lambda
                .params
                .iter()
                .map(|param| param.name.clone())
                .collect(),
        ),
        Expr::MethodCall { method, args, .. }
            if matches!(
                method.as_str(),
                "gradient" | "value_and_gradient" | "vjp" | "jvp"
            ) =>
        {
            args.first()
                .and_then(|arg| compute_transform_parameter_names(&arg.expr, cx))
        }
        Expr::Call(call)
            if matches!(
                call.name.as_str(),
                "gradient" | "value_and_gradient" | "vjp" | "jvp"
            ) =>
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
        if arg.label.as_ref().is_some_and(|(label, _)| label == "wrt") {
            wrt = compute_transform_wrt(&arg.expr);
        } else {
            value_args.push(arg);
            lowered_args.push(lower_expr(&arg.expr, cx, env));
        }
    }
    let Some(parameter_names) = compute_transform_parameter_names(&function.expr, cx)
        .or_else(|| compute_function_type_names(&function_ty))
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
    let Some(ty) = resolved_ret.cloned() else {
        return Some(invariant_method_expr(
            method_span,
            "compute call without a resolved return type",
        ));
    };
    let record = match checked_core_record(module, method, lowered_args.len(), method_span) {
        Ok(record) => record,
        Err(expr) => return Some(expr),
    };
    let arg_count = lowered_args.len();
    Some(TExpr {
        ty: ty.clone(),
        kind: TExprKind::CoreCall {
            record,
            args: lowered_args,
            source_span: method_span,
            type_args: vec![function_ty],
            widen_to_vec: vec![false; arg_count],
            data_plan: None,
            fallibility: TFailureCarrier::from_checked_type(&ty),
        },
    })
}

/// Route the public archive surface through the loaded source package. The
/// package module itself is the only caller that may lower the internal ABI
/// calls below; all other callers use the ordinary file-module TIR path.
fn lower_archive_source_call(
    method_span: Span,
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
    if !cx.core_archive_source || cx.module_alias == "core_archive" {
        return None;
    }
    let (sig, fixed_ret) = crate::Sema::core_fixed_sig("core.archive", method)?;
    let targs = lower_module_args(args, Some(sig.as_slice()), env, cx);
    let Some(ty) = resolved_ret.cloned().or(fixed_ret) else {
        return Some(invariant_method_expr(
            method_span,
            format!("archive call `core.archive.{method}` has no resolved return type"),
        ));
    };
    let target_return = match module_call_target_return(cx, Some(&ty)) {
        Ok(target_return) => Some(target_return),
        Err(error) => return Some(invariant_method_expr(method_span, error)),
    };
    Some(TExpr {
        ty,
        kind: TExprKind::ModuleCall {
            form: TModuleCallForm::Qualified {
                // The source package is emitted as `mod __jet_core_archive`.
                // `mangle_generated` would add a second generated prefix and
                // produce the nonexistent `__jet___core_archive` path.
                rust_mod: crate::Codegen::mangle("core_archive"),
                rust_fn: mangle(method).to_string(),
            },
            target_return,
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

/// The same, with resolved generic arguments the emitter spells.
fn host_generic_owner(path: impl Into<String>, generics: Vec<TPreludeArg>) -> TStaticOwner {
    TStaticOwner::Prelude {
        rooted: false,
        path: path.into(),
        generics,
    }
}

/// A String rvalue with no view provenance can transfer its buffer directly to
/// `Vec<u8>`. Places stay on the borrowing helper so the source remains usable.
fn string_bytes_receiver_is_owned(receiver: &TExpr, cx: &Cx) -> bool {
    if !matches!(&receiver.ty, Type::String) {
        return false;
    }
    match &receiver.kind {
        TExprKind::StrLit(_)
        | TExprKind::Clone(_)
        | TExprKind::ExplicitCopy(_)
        | TExprKind::MaterializeView(_) => true,
        TExprKind::Call { name, .. } => {
            let Some(Type::Fn {
                return_view_provenance,
                ..
            }) = cx.fn_types.get(name)
            else {
                return false;
            };
            !return_view_provenance
                .as_ref()
                .is_some_and(|provenance| !provenance.is_empty())
        }
        TExprKind::BuiltinMethod { op, .. } => !matches!(
            op,
            TBuiltinOp::TrimView | TBuiltinOp::AfterView | TBuiltinOp::BeforeView
        ),
        _ => false,
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
fn builtin_arg_is_place(expr: &Expr) -> bool {
    matches!(
        expr.without_parens(),
        Expr::Ident(..) | Expr::Field(..) | Expr::Index { .. }
    )
}

fn lower_builtin_arg(
    arg: &crate::AST::CallArg,
    expected: Option<&Type>,
    owns_value: bool,
    cx: &Cx,
    env: &mut LowerEnv,
) -> TExpr {
    let mut value = if owns_value {
        lower_owned_expr(&arg.expr, cx, env)
    } else {
        lower_expr(&arg.expr, cx, env)
    };
    if let Some(expected) = expected {
        if matches!(expected, Type::List(_) | Type::FixedList { .. })
            && crate::Generics::free_type_params(expected).is_empty()
        {
            value = preserve_typed_list_shape(value, expected, cx);
        }
    }

    // Builtins store values directly in Rust collections, unlike ordinary call
    // arguments whose `TCallArg` carries the implicit-clone bit to emission.
    // Preserve Jet's read-by-value argument semantics at this plain-argument
    // boundary: places are copied before a storing builtin consumes them.
    let resource_take = matches!(&value.kind, TExprKind::ResourceTake(_));
    let already_owned = matches!(
        &value.kind,
        TExprKind::Clone(_)
            | TExprKind::ExplicitCopy(_)
            | TExprKind::MaterializeView(_)
            | TExprKind::ResourceTake(_)
    );
    let place_copy = owns_value && builtin_arg_is_place(&arg.expr);
    if !resource_take
        && !already_owned
        && (arg.flags.implicit_clone || place_copy)
        && !value.ty.is_scalar()
    {
        let ty = value.ty.clone();
        env.note_clone(&ty);
        value = TExpr {
            ty,
            kind: TExprKind::Clone(Box::new(value)),
        };
    }
    value
}

fn core_widen_to_vec(module: &str, method: &str, args: &[TExpr]) -> Vec<bool> {
    if module == "core.term"
        && method == "progress"
        && matches!(
            args.first().map(|arg| &arg.ty),
            Some(Type::FixedList { .. })
        )
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
fn normalize_http_cors_policy_args(args: &mut Vec<TExpr>) {
    if args.first().is_none() {
        return;
    }
    let list_ty = Type::List(Box::new(Type::String));
    let empty_list = || TExpr {
        ty: list_ty.clone(),
        kind: TExprKind::ListLit(Vec::new()),
    };
    let origins = args[0].clone();
    let (origins_mode, origins_value) = match &origins.kind {
        TExprKind::EnumLit {
            variant,
            payload: TEnumPayload::Unit,
            ..
        } if variant == "Any" => (
            TExpr {
                ty: Type::Int,
                kind: TExprKind::IntLit(0, None),
            },
            empty_list(),
        ),
        TExprKind::EnumLit {
            variant,
            payload: TEnumPayload::Positional(values),
            ..
        } if variant == "List" && values.len() == 1 => (
            TExpr {
                ty: Type::Int,
                kind: TExprKind::IntLit(1, None),
            },
            values[0].value.clone(),
        ),
        _ => (
            TExpr {
                ty: Type::Int,
                kind: TExprKind::IntLit(1, None),
            },
            origins,
        ),
    };
    let methods = args.get(1).cloned().unwrap_or_else(|| empty_list());
    let headers = args.get(2).cloned().unwrap_or_else(|| empty_list());
    let credentials = args.get(3).cloned().unwrap_or(TExpr {
        ty: Type::Bool,
        kind: TExprKind::BoolLit(false),
    });
    let max_age = args.get(4).cloned().unwrap_or(TExpr {
        ty: Type::Int,
        kind: TExprKind::IntLit(86_400, None),
    });
    *args = vec![
        origins_mode,
        origins_value,
        methods,
        headers,
        credentials,
        TExpr {
            ty: Type::Int,
            kind: TExprKind::IntLit(1, None),
        },
        max_age,
    ];
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

fn lower_data_schema_call(
    receiver: &Expr,
    method: &str,
    method_span: Span,
    args: &[crate::AST::CallArg],
    resolved_ret: Option<&Type>,
    cx: &Cx,
    env: &mut LowerEnv,
) -> Option<TExpr> {
    let Some(Type::List(column)) = resolved_ret else {
        return None;
    };
    if column.base_name() != Some("DataColumn") {
        return None;
    }
    let target = core_module_path_from_receiver(receiver, cx, env)
        .map(|module| (module, method.to_string()))
        .or_else(|| match receiver {
            Expr::Ident(alias, _) if !env.locals.contains_key(alias) => cx
                .inline_reexport_core
                .get(&(alias.clone(), method.to_string()))
                .cloned(),
            _ => None,
        })?;
    if target.0 != "core.data" || target.1 != "schema" {
        return None;
    }
    let [arg] = args else {
        return Some(invariant_method_expr(
            method_span,
            "data.schema without one checked list argument",
        ));
    };
    let input = lower_expr(&arg.expr, cx, env);
    let elem = match &input.ty {
        Type::List(elem) | Type::FixedList { elem, .. } => elem.as_ref(),
        _ => {
            return Some(invariant_method_expr(
                method_span,
                "data.schema without a checked row type",
            ));
        }
    };
    let row = |name: &str| {
        Some((
            cx.struct_type_param_order
                .get(name)
                .cloned()
                .unwrap_or_default(),
            cx.struct_fields.get(name)?.clone(),
        ))
    };
    let Some(columns) = crate::Comptime::DataPipeline::schema_columns_for_type(elem, true, &row)
    else {
        return Some(invariant_method_expr(
            method_span,
            "data.schema without resolved row type arguments",
        ));
    };
    // Schema is checked type metadata, including when the list is empty.
    // Keep argument effects, but do not copy a stored list just to inspect its type.
    let input = if matches!(
        &input.kind,
        TExprKind::Local(_) | TExprKind::Field { .. } | TExprKind::Index { .. }
    ) {
        TExpr {
            ty: input.ty.clone(),
            kind: TExprKind::Borrow {
                place: Box::new(input),
                mutable: false,
            },
        }
    } else {
        input
    };
    let ty = Type::List(column.clone());
    Some(TExpr {
        ty: ty.clone(),
        kind: TExprKind::InlineBlock(vec![
            TStmt::ExprStmt(input),
            TStmt::ExprStmt(TExpr {
                ty,
                kind: TExprKind::CtLit(crate::AST::CtValue::List(columns)),
            }),
        ]),
    })
}

/// Keep the generic `core.crypto` call off the large method-dispatch frame.
/// This is the same resolved CoreCall shape as the full dispatcher below; the
/// sema fixed-signature fact makes the narrow route total.
fn lower_core_crypto_alias_fast(
    receiver: &Expr,
    method: &str,
    method_span: Span,
    args: &[crate::AST::CallArg],
    resolved_ret: Option<&Type>,
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
    if !matches!(module.as_str(), "core.crypto" | "core.crypto.expert") {
        return None;
    }
    let Some((params, _)) = crate::Sema::core_fixed_sig(&module, &core_method) else {
        return Some(invariant_method_expr(
            method_span,
            "crypto alias without a checked core signature",
        ));
    };
    let Some(ty) = resolved_ret.cloned() else {
        return Some(invariant_method_expr(
            method_span,
            "crypto alias without a resolved return type",
        ));
    };
    let raw_args: Vec<TExpr> = args
        .iter()
        .map(|arg| lower_expr(&arg.expr, cx, env))
        .collect();
    let widen_to_vec = core_widen_to_vec(&module, &core_method, &raw_args);
    let mut targs = Vec::with_capacity(raw_args.len());
    for (index, value) in raw_args.into_iter().enumerate() {
        let Some(widen) = widen_to_vec.get(index).copied() else {
            return Some(invariant_method_expr(
                method_span,
                "crypto alias without a widening fact for every argument",
            ));
        };
        if widen {
            targs.push(value);
        } else {
            let Some((_, expected)) = params.get(index) else {
                return Some(invariant_method_expr(
                    method_span,
                    "crypto alias argument exceeds its checked core signature",
                ));
            };
            targs.push(preserve_typed_list_shape(value, expected, cx));
        }
    }
    let record = match checked_core_record(&module, &core_method, targs.len(), method_span) {
        Ok(record) => record,
        Err(expr) => return Some(expr),
    };
    prepare_generic_serde_codec(cx, &env.fn_name, &module, &core_method, &mut targs, &ty);
    Some(TExpr {
        ty: ty.clone(),
        kind: TExprKind::CoreCall {
            record,
            args: targs,
            source_span: method_span,
            type_args: Vec::new(),
            widen_to_vec,
            data_plan: None,
            fallibility: TFailureCarrier::from_checked_type(&ty),
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
    let kind = recv_type.as_deref()?.rsplit('.').next()?;
    let helper = crypto_instance_helper(kind, method)?;
    let recv = lowered_receiver
        .take()
        .unwrap_or_else(|| lower_expr(receiver, cx, env));
    let mut args = vec![recv];
    if kind == "Hasher" && method == "update" {
        args.extend(call_args.iter().map(|arg| lower_expr(&arg.expr, cx, env)));
    }
    let widen_to_vec = core_widen_to_vec("core.crypto", helper, &args);
    let ty = match resolved_ret.cloned() {
        Some(ty) => ty,
        None if kind == "Hasher" && method == "update" => {
            Type::Named(crate::Syntax::INTERNAL_UNIT_TYPE.to_string())
        }
        None => {
            return Some(invariant_method_expr(
                method_span,
                "crypto instance call without a resolved return type",
            ));
        }
    };
    let record = match checked_core_record("core.crypto", helper, args.len(), method_span) {
        Ok(record) => record,
        Err(expr) => return Some(expr),
    };
    Some(TExpr {
        ty: ty.clone(),
        kind: TExprKind::CoreCall {
            record,
            args,
            source_span: method_span,
            type_args: Vec::new(),
            widen_to_vec,
            data_plan: None,
            fallibility: TFailureCarrier::from_checked_type(&ty),
        },
    })
}

/// D-SERDE2=A / I9: project a codec owner onto its canonical bundle identity.
///
/// A codec instance is keyed by the OWNER TYPE — `cx.rust_type` for AOT, the JIT
/// program's function table for the resident tier, and `serde_codec` for the TIR
/// evaluator all spell it `Owner::encode` / `Owner::decode`. Every imported
/// nominal in TIR is spelled by its canonical identity: `register_imported_struct_shapes`
/// registers it that way, and the struct-literal head resolves `alias.Type` through
/// `foreign_type_identity` (it ICEs rather than keep the alias spelling). A LOCAL
/// struct's declared field type is the one place the source spelling survives —
/// `struct ImportedEnvelope { badge: library.Badge }` stores `library.Badge`
/// verbatim — so a field read reaches this node under a spelling no codec is
/// keyed by, and the evaluator refuses it with E0956 while AOT emits fine.
/// Resolve it here, through the one resolver that splits an alias off a qualified
/// name, so one type demands exactly one codec in every tier. `cx.rust_type`
/// renders both spellings to the same Rust path (`Context.rs` foreign-identity arm
/// and dotted-alias arm), so AOT emit is unchanged.
///
/// A bare name is left alone unless the declaring context has already recorded
/// its canonical module identity. Imported generated codecs are lowered under
/// that identity, while their checked method bodies still carry source-local
/// field spellings (for example `Expr` inside `Dual`). Resolve that local
/// spelling before emitting a codec demand; core codec owners have no local
/// identity and therefore keep the existing builtin path.
fn canonical_codec_owner(ty: &Type, cx: &Cx) -> Type {
    let Type::Named(name) = ty else {
        return ty.clone();
    };
    if let Some(identity) = cx.local_type_identities.get(name) {
        return Type::Named(identity.clone());
    }
    if !name.contains('.') {
        return ty.clone();
    }
    match cx.imported_type_metadata_name(name) {
        Some(identity) if identity != *name => Type::Named(identity),
        _ => ty.clone(),
    }
}

/// Return the checked Prelude codec owner for the closed builtin set. Qualified
/// imports are canonicalized before this helper runs; arbitrary dotted user
/// names must never become builtin codecs by leaf-name coincidence.
pub(crate) fn builtin_codec_name(ty: &Type) -> Option<&str> {
    match ty.without_user_tags() {
        Type::Named(name)
            if matches!(
                name.as_str(),
                "Date" | "LocalDate" | "LocalTime" | "DateTime" | "Duration" | "Decimal"
            ) =>
        {
            Some(name.as_str())
        }
        _ => None,
    }
}

fn generic_method_instance_leaf(owner: &Type, method: &str, type_args: &[Type]) -> String {
    let key = crate::Codegen::TIR::generic_method_instance_key(owner, method, type_args);
    key.rsplit_once("::")
        .expect("generic method key includes an owner separator")
        .1
        .to_string()
}

fn lower_serde_encode_node(mut recv: TExpr, cx: &Cx) -> TExpr {
    recv.ty = canonical_codec_owner(&recv.ty, cx);
    if matches!(&recv.ty, Type::Apply { .. }) {
        cx.jit_method_calls.borrow_mut().insert(
            crate::Codegen::TIR::generic_method_instance_key(&recv.ty, "encode", &[]),
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
    _method_span: Span,
    cx: &Cx,
) -> TExpr {
    let target = canonical_codec_owner(&target, cx);
    if matches!(&target, Type::Apply { .. }) {
        cx.jit_method_calls.borrow_mut().insert(
            crate::Codegen::TIR::generic_method_instance_key(&target, "decode", &[]),
            (target.clone(), "decode".to_string(), Vec::new()),
        );
    }
    // Compiler-generated Codable bodies are lowered before sema writes
    // `resolved_ret`. The explicit target type argument is the same checked
    // fact; synthesize the Result carrier so fragment eval does not refuse.
    let ty = match resolved_ret.cloned() {
        Some(ty) => ty,
        None => Type::Result {
            ok: Box::new(target.clone()),
            err: Box::new(Type::Named(Syntax::TYPE_ENCODING_ERROR.to_string())),
        },
    };
    TExpr {
        ty,
        kind: TExprKind::HandleMethod {
            recv: Box::new(recv),
            op: THandleOp::DataTreeDecode(target),
            args: Vec::new(),
        },
    }
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
        Type::Apply { name, args } if name == Syntax::TYPE_ITER && args.len() == 1 => &args[0],
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
        Type::Apply { name, args } if name == Syntax::TYPE_ITER && args.len() == 1 => true,
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
    method_span: Span,
    resolved_ret: Option<&Type>,
) -> TExpr {
    let Some(ret) = resolved_ret.cloned() else {
        return invariant_method_expr(method_span, "zip call without a resolved return type");
    };
    let input_count = inputs.len() + 1;
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
    let Some(tuple_fields) = zip_tuple_fields(&ret) else {
        return invariant_method_expr(method_span, "zip call without a checked tuple return shape");
    };
    let field_types = tuple_fields.iter().map(|(_, ty)| ty.clone()).collect();
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
                tuple_struct: crate::Codegen::Tuples::tuple_struct_name(&tuple_fields),
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

pub(crate) fn lower_empty_zip_family(
    resolved_ret: &Type,
    _method: &str,
    _method_span: Span,
) -> TExpr {
    TExpr {
        ty: resolved_ret.clone(),
        kind: TExprKind::BuiltinMethod {
            recv: Box::new(TExpr {
                ty: Type::List(Box::new(unit_type())),
                kind: TExprKind::ListLit(Vec::new()),
            }),
            op: TBuiltinOp::ListLazy,
            args: Vec::new(),
        },
    }
}

fn wrap_list_as_iter(receiver: TExpr) -> TExpr {
    let element = match receiver.ty.without_user_tags() {
        Type::List(element) | Type::FixedList { elem: element, .. } => (**element).clone(),
        _ => return receiver,
    };
    TExpr {
        ty: crate::Collections::iter_ty(element),
        kind: TExprKind::BuiltinMethod {
            recv: Box::new(receiver),
            op: TBuiltinOp::ListLazy,
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
    operator_rhs: Option<&Type>,
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
        operator_rhs,
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
    operator_rhs: Option<&Type>,
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
                kind: TExprKind::HostCall(Box::new(THostCall::MemoStats {
                    name: name.clone(),
                })),
            };
        }
    }
    if let Some(lowered) =
        lower_data_schema_call(receiver, method, method_span, args, resolved_ret, cx, env)
    {
        return lowered;
    }
    if let Some(lowered) =
        lower_core_crypto_alias_fast(receiver, method, method_span, args, resolved_ret, cx, env)
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
        operator_rhs,
        resolved_ret,
        checked_widen,
        cx,
        env,
        lowered_receiver,
        instantiated_sig,
    )
}

/// D-TYPE2-SPELL1: sema marks `Int(lo..hi)`'s descriptor call with the
/// structural range it resolved. The descriptor has no runtime value; only
/// the destination-owned conversion argument reaches TIR.
fn inline_range_receiver(receiver: &Expr) -> Option<(i64, i64)> {
    let Expr::Call(call) = receiver else {
        return None;
    };
    match call.resolved_ret.as_ref() {
        Some(Type::InlineRange { lo, hi, .. }) => Some((*lo, *hi)),
        Some(Type::Result { ok, .. }) => match ok.as_ref() {
            Type::InlineRange { lo, hi, .. } => Some((*lo, *hi)),
            _ => None,
        },
        _ => None,
    }
}

fn route_strip_parens(mut expr: &Expr) -> &Expr {
    while let Expr::Paren(inner, _) = expr {
        expr = inner;
    }
    expr
}
fn route_handler_param_names(handler_expr: &Expr, cx: &Cx) -> Option<Vec<String>> {
    match route_strip_parens(handler_expr) {
        Expr::Lambda(lambda) => Some(
            lambda
                .params
                .iter()
                .map(|param| param.name.clone())
                .collect(),
        ),
        Expr::Ident(name, _) => cx
            .fn_param_names
            .get(name)
            .map(|params| params.iter().map(|param_name| param_name.clone()).collect()),
        _ => None,
    }
}

fn route_static_string(expr: &Expr, cx: &Cx) -> Option<String> {
    match route_strip_parens(expr) {
        Expr::Str(parts, _) => {
            let mut value = String::new();
            for part in parts {
                let StrPart::Lit(text) = part else {
                    return None;
                };
                value.push_str(text);
            }
            Some(value)
        }
        Expr::Ident(name, _) => match cx.const_values.get(name) {
            Some(crate::AST::CtValue::Str(value)) => Some(value.clone()),
            _ => None,
        },
        Expr::ComptimeName {
            value: Some(crate::AST::CtValue::Str(value)),
            ..
        } => Some(value.clone()),
        _ => None,
    }
}

fn route_schema_type(
    ty: &Type,
    cx: &Cx,
    active: &mut HashSet<String>,
) -> Option<jet_foundation::MIR::MirHttpSchema> {
    use jet_foundation::MIR::MirHttpSchema;
    match ty {
        Type::Int | Type::IntN { .. } => Some(MirHttpSchema::Integer),
        Type::Float | Type::Float32 => Some(MirHttpSchema::Number),
        Type::Bool => Some(MirHttpSchema::Boolean),
        Type::String | Type::Char => Some(MirHttpSchema::String),
        Type::List(item) | Type::FixedList { elem: item, .. } => Some(MirHttpSchema::Array(
            Box::new(route_schema_type(item, cx, active)?),
        )),
        Type::Map { .. } => Some(MirHttpSchema::Object {
            properties: Vec::new(),
            additional_properties: true,
        }),
        Type::Shared(inner)
        | Type::InlineRange { base: inner, .. }
        | Type::Quantity { base: inner, .. }
        | Type::Tagged { inner, .. } => route_schema_type(inner, cx, active),
        Type::Option(inner) => Some(MirHttpSchema::Nullable(Box::new(route_schema_type(
            inner, cx, active,
        )?))),
        Type::Result { ok, .. } => route_schema_type(ok, cx, active),
        Type::Tuple(fields) => {
            let mut properties = Vec::with_capacity(fields.len());
            for (name, field_ty) in fields {
                properties.push(jet_foundation::MIR::MirHttpPropertyFact {
                    name: name.clone(),
                    schema: route_schema_type(field_ty, cx, active)?,
                    required: true,
                });
            }
            Some(MirHttpSchema::Object {
                properties,
                additional_properties: false,
            })
        }
        Type::Union(items) => Some(MirHttpSchema::OneOf(
            items
                .iter()
                .map(|item| route_schema_type(item, cx, active))
                .collect::<Option<Vec<_>>>()?,
        )),
        Type::Named(name) | Type::Apply { name, .. } => match name.as_str() {
            "Any" | "JSON" => Some(MirHttpSchema::Any),
            "Null" | "Unit" => Some(MirHttpSchema::Null),
            "String" => Some(MirHttpSchema::String),
            "Bool" => Some(MirHttpSchema::Boolean),
            "Int" | "I8" | "I16" | "I32" | "I64" | "I128" | "U8" | "U16" | "U32" | "U64"
            | "U128" => Some(MirHttpSchema::Integer),
            "Float" | "F32" | "F64" => Some(MirHttpSchema::Number),
            _ if !cx.codable_types.contains(name) => None,
            _ if !active.insert(name.clone()) => None,
            _ => {
                let fields = cx.reflection_fields.get(name)?;
                let mut properties = Vec::with_capacity(fields.len());
                for field in fields.iter().filter(|field| field.is_pub) {
                    properties.push(jet_foundation::MIR::MirHttpPropertyFact {
                        name: field.name.clone(),
                        schema: route_schema_type(&field.ty, cx, active)?,
                        required: !matches!(field.ty, Type::Option(_)),
                    });
                }
                active.remove(name);
                Some(MirHttpSchema::Object {
                    properties,
                    additional_properties: false,
                })
            }
        },
        Type::TraitObject(_) | Type::Fn { .. } | Type::Measure(_) => None,
    }
}

fn route_schema_expr(
    expr: &Expr,
    cx: &Cx,
    locals: &std::collections::HashMap<String, Type>,
) -> Option<jet_foundation::MIR::MirHttpSchema> {
    use jet_foundation::MIR::MirHttpSchema;
    match route_strip_parens(expr) {
        Expr::Str(_, _) | Expr::Char(_, _) => Some(MirHttpSchema::String),
        Expr::Int(..) => Some(MirHttpSchema::Integer),
        Expr::Float(..) => Some(MirHttpSchema::Number),
        Expr::Bool(_, _) => Some(MirHttpSchema::Boolean),
        Expr::Unit(_) => Some(MirHttpSchema::Null),
        Expr::ListLit(items, _) => {
            let first = items.first()?;
            Some(MirHttpSchema::Array(Box::new(route_schema_expr(
                first, cx, locals,
            )?)))
        }
        Expr::StructLit { type_name, .. } => {
            let mut active = HashSet::new();
            route_schema_type(&Type::Named(type_name.clone()), cx, &mut active)
        }
        Expr::TupleLit(_, _, Some(ty)) => {
            let mut active = HashSet::new();
            route_schema_type(ty, cx, &mut active)
        }
        Expr::TypedLit { head: Some(ty), .. } => {
            let mut active = HashSet::new();
            route_schema_type(ty, cx, &mut active)
        }
        Expr::Ident(name, _) => {
            let ty = locals.get(name)?;
            let mut active = HashSet::new();
            route_schema_type(ty, cx, &mut active)
        }
        Expr::Call(call) => {
            let ty = call.resolved_ret.as_ref()?;
            let mut active = HashSet::new();
            route_schema_type(ty, cx, &mut active)
        }
        Expr::MethodCall {
            resolved_ret: Some(ty),
            ..
        } => {
            let mut active = HashSet::new();
            route_schema_type(ty, cx, &mut active)
        }
        Expr::Ok(value, _) | Expr::Try(value, _, _, _) => route_schema_expr(value, cx, locals),
        _ => None,
    }
}

fn route_status(expr: &Expr, cx: &Cx) -> Option<i64> {
    match route_strip_parens(expr) {
        Expr::Int(value, ..) => Some(*value),
        Expr::Ident(name, _) => match cx.const_values.get(name) {
            Some(crate::AST::CtValue::Int(value)) => Some(*value),
            _ => None,
        },
        Expr::ComptimeName {
            value: Some(crate::AST::CtValue::Int(value)),
            ..
        } => Some(*value),
        _ => None,
    }
}

fn route_response_constructor(
    name: &str,
    args: &[crate::AST::CallArg],
    cx: &Cx,
    locals: &std::collections::HashMap<String, Type>,
) -> Option<Vec<jet_foundation::MIR::MirHttpResponseFact>> {
    if !matches!(name, "response" | "json" | "empty_response") || args.is_empty() {
        return None;
    }
    let status = route_status(&args[0].expr, cx)?;
    if !(100..=599).contains(&status) {
        return None;
    }
    let (schema, content_type) = match name {
        "response" if args.len() == 2 => {
            (Some(route_schema_expr(&args[1].expr, cx, locals)?), None)
        }
        "json" if args.len() == 2 => (
            Some(route_schema_expr(&args[1].expr, cx, locals)?),
            Some("application/json".to_string()),
        ),
        "empty_response" if args.len() == 1 => (None, None),
        _ => return None,
    };
    Some(vec![jet_foundation::MIR::MirHttpResponseFact {
        status,
        description: "HTTP response".to_string(),
        schema,
        content_type,
    }])
}

fn route_response_expr(
    expr: &Expr,
    cx: &Cx,
    locals: &std::collections::HashMap<String, Type>,
    response_aliases: &std::collections::HashMap<
        String,
        Vec<jet_foundation::MIR::MirHttpResponseFact>,
    >,
) -> Option<Vec<jet_foundation::MIR::MirHttpResponseFact>> {
    match route_strip_parens(expr) {
        Expr::Ok(value, _) | Expr::Try(value, _, _, _) => {
            route_response_expr(value, cx, locals, response_aliases)
        }
        Expr::If {
            then_value,
            else_value,
            ..
        } => {
            let mut responses = route_response_expr(then_value, cx, locals, response_aliases)?;
            responses.extend(route_response_expr(
                else_value,
                cx,
                locals,
                response_aliases,
            )?);
            Some(responses)
        }
        Expr::Ident(name, _) => response_aliases.get(name).cloned(),
        Expr::Call(call) => {
            let name = call.name.rsplit('.').next().unwrap_or(call.name.as_str());
            route_response_constructor(name, &call.args, cx, locals)
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
            resolved_ret,
            ..
        } => {
            // Core aliases remain MethodCall nodes after sema.  Their checked
            // return fact is the authority that this is an endpoint response;
            // only then recover the constructor payload from its checked args.
            let returns_http_response = matches!(
                resolved_ret.as_ref(),
                Some(Type::Named(name)) if name == "HTTPResponse"
            );
            if returns_http_response
                && matches!(method.as_str(), "response" | "json" | "empty_response")
            {
                return route_response_constructor(method, args, cx, locals);
            }
            // Checked response adapters (for example `.header(...)`) preserve
            // the response contract carried by their receiver.
            if returns_http_response {
                return route_response_expr(receiver, cx, locals, response_aliases);
            }
            None
        }
        _ => None,
    }
}

fn route_collect_returns(
    body: &[Stmt],
    cx: &Cx,
    locals: &mut std::collections::HashMap<String, Type>,
    response_aliases: &mut std::collections::HashMap<
        String,
        Vec<jet_foundation::MIR::MirHttpResponseFact>,
    >,
    responses: &mut Vec<jet_foundation::MIR::MirHttpResponseFact>,
    allow_opaque_responses: bool,
) -> bool {
    let mut valid = true;
    for stmt in body {
        match stmt {
            Stmt::Val(binding) => {
                if !binding.name.is_empty() {
                    // A later binding shadows any response alias with the same
                    // name, even when its initializer is not a response.
                    response_aliases.remove(&binding.name);
                    if let Some(found) =
                        route_response_expr(&binding.init, cx, locals, response_aliases)
                    {
                        response_aliases.insert(binding.name.clone(), found);
                    }
                }
                if let Some(ty) = &binding.ty {
                    // Sema already checked local expressions. Keep their types
                    // available for payload aliases, but do not reject unrelated
                    // locals while reconstructing the endpoint contract.
                    if !binding.name.is_empty() {
                        locals.insert(binding.name.clone(), ty.clone());
                    }
                }
            }
            Stmt::Return(Some(value), _) => {
                let Some(found) = route_response_expr(value, cx, locals, response_aliases) else {
                    if !allow_opaque_responses {
                        valid = false;
                    }
                    continue;
                };
                responses.extend(found);
            }
            Stmt::Return(None, _) => valid = false,
            Stmt::While { body, .. }
            | Stmt::For { body, .. }
            | Stmt::Loop { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Impure { body, .. }
            | Stmt::Reactive { body, .. }
            | Stmt::Shield { body, .. }
            | Stmt::Switched { body, .. }
            | Stmt::Region { body, .. }
            | Stmt::Policy { body, .. }
            | Stmt::TaskGroup { body, .. }
            | Stmt::Layout { body, .. }
            | Stmt::AuthorityScope { body, .. }
            | Stmt::ComptimeBlock { body, .. }
            | Stmt::ContextBlock { body, .. }
            | Stmt::Live { body, .. }
            | Stmt::AssumeDet { body, .. }
            | Stmt::Transact { body, .. }
            | Stmt::ScopeMember { body, .. } => {
                valid &= route_collect_returns(
                    body,
                    cx,
                    locals,
                    response_aliases,
                    responses,
                    allow_opaque_responses,
                );
            }
            Stmt::Switch {
                arms, else_body, ..
            }
            | Stmt::ComptimeSwitch {
                arms, else_body, ..
            } => {
                for arm in arms {
                    valid &= route_collect_returns(
                        &arm.body,
                        cx,
                        locals,
                        response_aliases,
                        responses,
                        allow_opaque_responses,
                    );
                }
                if let Some(body) = else_body {
                    valid &= route_collect_returns(
                        body,
                        cx,
                        locals,
                        response_aliases,
                        responses,
                        allow_opaque_responses,
                    );
                }
            }
            Stmt::CountedLoop { body, step, .. } => {
                valid &= route_collect_returns(
                    body,
                    cx,
                    locals,
                    response_aliases,
                    responses,
                    allow_opaque_responses,
                );
                if let Some(step) = step {
                    valid &= route_collect_returns(
                        std::slice::from_ref(step.as_ref()),
                        cx,
                        locals,
                        response_aliases,
                        responses,
                        allow_opaque_responses,
                    );
                }
            }
            Stmt::ComptimeIf {
                then_body,
                else_body,
                ..
            } => {
                valid &= route_collect_returns(
                    then_body,
                    cx,
                    locals,
                    response_aliases,
                    responses,
                    allow_opaque_responses,
                );
                if let Some(body) = else_body {
                    valid &= route_collect_returns(
                        body,
                        cx,
                        locals,
                        response_aliases,
                        responses,
                        allow_opaque_responses,
                    );
                }
            }
            _ => {}
        }
    }
    valid
}

fn route_json_type_from_body(body: &LambdaBody) -> Option<Type> {
    let mut found = None;
    let mut visit = |expr: &Expr| {
        if found.is_some() {
            return;
        }
        if let Expr::MethodCall {
            method,
            recv_type,
            type_args,
            ..
        } = expr
        {
            if method == "json"
                && recv_type.as_deref() == Some("HTTPRequest")
                && type_args.len() == 1
            {
                found = Some(type_args[0].clone());
            }
        }
    };
    match body {
        LambdaBody::Expr(expr) => expr.for_each_expr(&mut visit),
        LambdaBody::Block(stmts) => {
            for stmt in stmts {
                stmt.for_each_expr(&mut visit);
            }
        }
    }
    found
}

fn route_handler_contract(
    method: jet_foundation::MIR::MirHttpMethod,
    path_expr: &Expr,
    handler_expr: &Expr,
    cx: &Cx,
    line: usize,
    is_mux: bool,
) -> Option<String> {
    let pattern = route_static_string(path_expr, cx)?;
    Syntax::validate_http_route_pattern(&pattern).ok()?;
    let mut path = String::new();
    let mut parameters = Vec::new();
    if pattern == "/" {
        path.push('/');
    } else {
        for segment in pattern.split('/').skip(1) {
            path.push('/');
            if let Some(name) = segment.strip_prefix(':') {
                path.push('{');
                path.push_str(name);
                path.push('}');
                parameters.push(jet_foundation::MIR::MirHttpParameterFact {
                    name: name.to_string(),
                    location: jet_foundation::MIR::MirHttpParameterLocation::Path,
                    required: true,
                    catch_all: false,
                    schema: jet_foundation::MIR::MirHttpSchema::String,
                });
            } else if let Some(name) = segment.strip_prefix('*') {
                path.push('{');
                path.push_str(name);
                path.push('}');
                parameters.push(jet_foundation::MIR::MirHttpParameterFact {
                    name: name.to_string(),
                    location: jet_foundation::MIR::MirHttpParameterLocation::Path,
                    required: true,
                    catch_all: true,
                    schema: jet_foundation::MIR::MirHttpSchema::String,
                });
            } else {
                path.push_str(segment);
            }
        }
    }
    let handler = route_strip_parens(handler_expr);
    let (body, param_types) = match handler {
        Expr::Lambda(lambda) => {
            let params = lambda
                .params
                .iter()
                .map(|param| param.ty.clone())
                .collect::<Option<Vec<_>>>()?;
            (
                match &lambda.body {
                    LambdaBody::Expr(expr) => LambdaBody::Expr(expr.clone()),
                    LambdaBody::Block(stmts) => LambdaBody::Block(stmts.clone()),
                },
                params,
            )
        }
        Expr::Ident(name, _) => {
            let body = LambdaBody::Block(cx.fn_bodies.get(name)?.clone());
            let params = cx
                .sigs
                .get(name)?
                .iter()
                .map(|(_, ty)| ty.clone())
                .collect::<Vec<_>>();
            (body, params)
        }
        _ => return None,
    };
    let mut locals = std::collections::HashMap::new();
    let mut response_aliases = std::collections::HashMap::new();
    let mut handler_param_names = Vec::with_capacity(param_types.len());
    if let Expr::Ident(name, _) = handler {
        if let Some(param_names) = cx.fn_param_names.get(name) {
            for (param_name, ty) in param_names.iter().zip(param_types.iter()) {
                handler_param_names.push((param_name.clone(), ty.clone()));
                locals.insert(param_name.clone(), ty.clone());
            }
        }
    } else if let Expr::Lambda(lambda) = handler {
        for (param, ty) in lambda.params.iter().zip(param_types.iter()) {
            handler_param_names.push((param.name.clone(), ty.clone()));
            locals.insert(param.name.clone(), ty.clone());
        }
    }
    if handler_param_names.len() != param_types.len()
        || handler_param_names.iter().any(|(name, _)| name.is_empty())
        || handler_param_names
            .iter()
            .map(|(name, _)| name)
            .collect::<HashSet<_>>()
            .len()
            != handler_param_names.len()
    {
        return None;
    }
    if handler_param_names
        .iter()
        .filter(|(_, ty)| matches!(ty, Type::Named(name) if name == "HTTPRequest"))
        .count()
        > 1
    {
        return None;
    }
    if !is_mux {
        for parameter in &mut parameters {
            if parameter.location != jet_foundation::MIR::MirHttpParameterLocation::Path {
                continue;
            }
            let matches = handler_param_names
                .iter()
                .filter(|(name, ty)| {
                    name == &parameter.name
                        && !matches!(ty, Type::Named(request) if request == "HTTPRequest")
                })
                .collect::<Vec<_>>();
            if matches.len() != 1 {
                return None;
            }
            let mut active = HashSet::new();
            parameter.schema = route_schema_type(&matches[0].1, cx, &mut active)?;
        }
    }
    let path_names = parameters
        .iter()
        .filter(|parameter| {
            parameter.location == jet_foundation::MIR::MirHttpParameterLocation::Path
        })
        .map(|parameter| parameter.name.clone())
        .collect::<HashSet<_>>();
    if matches!(
        method,
        jet_foundation::MIR::MirHttpMethod::Get | jet_foundation::MIR::MirHttpMethod::Head
    ) {
        for (name, ty) in &handler_param_names {
            if path_names.contains(name)
                || matches!(ty, Type::Named(request) if request == "HTTPRequest")
            {
                continue;
            }
            let mut active = HashSet::new();
            parameters.push(jet_foundation::MIR::MirHttpParameterFact {
                name: name.clone(),
                location: jet_foundation::MIR::MirHttpParameterLocation::Query,
                required: !matches!(ty, Type::Option(_)),
                catch_all: false,
                schema: route_schema_type(ty, cx, &mut active)?,
            });
        }
    } else if handler_param_names.iter().any(|(name, ty)| {
        !path_names.contains(name)
            && !matches!(ty, Type::Named(request) if request == "HTTPRequest")
    }) {
        return None;
    }
    let body_schema = match route_json_type_from_body(&body) {
        Some(ty) => {
            let mut active = HashSet::new();
            Some(route_schema_type(&ty, cx, &mut active)?)
        }
        None => None,
    };
    if let Some(jet_foundation::MIR::MirHttpSchema::Object { properties, .. }) =
        body_schema.as_ref()
    {
        if properties.iter().any(|property| {
            parameters
                .iter()
                .any(|parameter| parameter.name == property.name)
        }) {
            return None;
        }
    }
    let request_body = body_schema.map(|schema| jet_foundation::MIR::MirHttpRequestBodyFact {
        required: true,
        content_type: "application/json".to_string(),
        schema,
    });
    let mut responses = Vec::new();
    let valid = match &body {
        LambdaBody::Expr(expr) => match route_response_expr(expr, cx, &locals, &response_aliases) {
            Some(found) => {
                responses.extend(found);
                true
            }
            None => is_mux,
        },
        LambdaBody::Block(stmts) => route_collect_returns(
            stmts,
            cx,
            &mut locals,
            &mut response_aliases,
            &mut responses,
            is_mux,
        ),
    };
    if !valid || (!is_mux && responses.is_empty()) {
        return None;
    }
    responses.sort_by_key(|response| response.status);
    responses.dedup();
    let operation_id = jet_foundation::MIR::MirHttpRouteFacts::operation_id(method, &path);
    Some(
        jet_foundation::MIR::MirHttpRouteFacts {
            method,
            pattern,
            path,
            operation_id,
            summary: None,
            parameters,
            request_body,
            responses,
            security: Vec::new(),
            provenance: format!("{}:{}", cx.file, line),
        }
        .to_wire(),
    )
}

fn hardware_method_op(
    receiver: &Expr,
    method: &str,
    args: &[crate::AST::CallArg],
    recv_type: &Option<String>,
    cx: &Cx,
    env: &LowerEnv,
) -> Result<Option<THardwareCall>, String> {
    let Some(profile_id) = cx.hardware_profile_id.clone().filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    let Some(profile) = cx.hardware_profile.as_ref() else {
        return Err("checked hardware profile facts are missing".to_string());
    };
    let path = hardware_expr_path(receiver);
    let is_board_receiver = path.first().is_some_and(|root| {
        cx.core_imports
            .get(root)
            .is_some_and(|profile| profile.starts_with("board."))
    });
    if is_board_receiver && path.len() >= 3 {
        let block = profile
            .register_blocks
            .iter()
            .find(|block| block.name.eq_ignore_ascii_case(&path[1]))
            .ok_or_else(|| {
                format!(
                    "checked hardware register block fact is missing for `{}`",
                    path[1]
                )
            })?;
        let register = block
            .registers
            .iter()
            .find(|register| register.name.eq_ignore_ascii_case(&path[2]))
            .ok_or_else(|| {
                format!(
                    "checked hardware register fact is missing for `{}.{}`",
                    block.name, path[2]
                )
            })?;
        let block_name = block.name.clone();
        let register_name = register.name.clone();
        return Ok(match method {
            "read" => Some(THardwareCall::RegisterRead {
                profile_id,
                block: block_name,
                register: register_name,
                width: register.width,
            }),
            "write" | "set" | "clear" => Some(THardwareCall::RegisterWrite {
                profile_id,
                block: block_name,
                register: register_name,
                width: register.width,
            }),
            _ => None,
        });
    }
    if method == "start"
        && matches!(receiver, Expr::Ident(name, _) if name == "dma")
        && args.len() >= 2
    {
        let channel_path = hardware_expr_path(&args[0].expr);
        if channel_path.len() >= 3
            && channel_path.first().is_some_and(|root| {
                cx.core_imports
                    .get(root)
                    .is_some_and(|profile| profile.starts_with("board."))
            })
        {
            let channel_key = format!(
                "{}_{}",
                channel_path[1].to_ascii_uppercase(),
                channel_path[2].to_ascii_uppercase()
            );
            let channel = profile
                .dma_channels
                .iter()
                .find(|fact| fact.name.eq_ignore_ascii_case(&channel_key))
                .map(|fact| fact.name.clone())
                .ok_or_else(|| {
                    format!("checked hardware DMA channel fact is missing for `{channel_key}`")
                })?;
            return Ok(Some(THardwareCall::DmaStart {
                profile_id,
                channel,
                buffer_ty: Type::Named("__JetDmaBuffer".to_string()),
            }));
        }
    }
    if method == "wait" && recv_type.as_deref() == Some("__JetDmaTransfer") {
        let receiver_name = match receiver {
            Expr::Ident(name, _) => name,
            _ => {
                return Err(
                    "checked hardware DMA wait receiver has no transfer binding".to_string()
                );
            }
        };
        let channel = env.dma_transfer_channel(receiver_name).ok_or_else(|| {
            "checked hardware DMA wait has no channel fact for its transfer binding".to_string()
        })?;
        let channel = profile
            .dma_channels
            .iter()
            .find(|fact| fact.name.eq_ignore_ascii_case(channel))
            .map(|fact| fact.name.clone())
            .ok_or_else(|| {
                format!("checked hardware DMA channel fact is missing for `{channel}`")
            })?;
        return Ok(Some(THardwareCall::DmaWait {
            profile_id,
            channel,
            buffer_ty: Type::Named("__JetDmaBuffer".to_string()),
        }));
    }
    Ok(None)
}

fn hardware_expr_path(expr: &Expr) -> Vec<String> {
    match expr {
        Expr::Ident(name, _) => vec![name.clone()],
        Expr::Field(base, member, _) => {
            let mut path = hardware_expr_path(base);
            path.push(member.clone());
            path
        }
        Expr::Paren(inner, _) => hardware_expr_path(inner),
        _ => Vec::new(),
    }
}

fn lower_app_callback(expr: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    let value = lower_expr(expr, cx, env);
    if matches!(
        &value.kind,
        TExprKind::FnValue {
            kind: TFnValueKind::Send { .. },
        }
    ) {
        return value;
    }
    let ty = value.ty.clone();
    TExpr {
        ty,
        kind: TExprKind::FnValue {
            kind: TFnValueKind::Send {
                value: Box::new(value),
            },
        },
    }
}

/// Lower a checked App route/boundary/loader/server-function handler.  Named
/// handlers become the send-safe borrow callback (`Fn(&A, &B) -> R`) so the
/// one runtime adapter per arity decodes each typed input; sema has already
/// rejected every non-named handler (E2810).
fn lower_app_route_callback(expr: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
    let params = match expr {
        Expr::Ident(name, _) => cx.fn_types.get(name).and_then(|ty| match ty {
            Type::Fn { params, .. } => Some(params.clone()),
            _ => None,
        }),
        _ => None,
    };
    if let Some(params) = params {
        if let Some(callback) =
            lower_named_collection_callback(expr, cx, env, true, Some(params.as_slice()))
        {
            let ty = callback.ty.clone();
            return TExpr {
                ty,
                kind: TExprKind::FnValue {
                    kind: TFnValueKind::Send {
                        value: Box::new(callback),
                    },
                },
            };
        }
    }
    lower_app_callback(expr, cx, env)
}

/// The wire name of one route input type.  Scalars keep their Jet spelling;
/// every other type is projected as JSON by the router and decoded by the
/// handler's checked decoder.
fn app_route_type_name(ty: &Type) -> String {
    match ty {
        Type::Int => "Int".to_string(),
        Type::Float => "Float".to_string(),
        Type::Bool => "Bool".to_string(),
        Type::String => "String".to_string(),
        Type::Named(name) => name.clone(),
        other => other.show(),
    }
}

/// Serialize the sema-checked input binding of a route or loader handler.
///
/// Grammar (one entry per handler parameter, in declaration order):
/// `p:<name>=<Type>` binds a dynamic path segment by name; `s=json:<f>=<T>,…`
/// binds the Codable search record; `s=query:<name>=<T>` binds one scalar
/// search field; `d` binds the route's loader data.  `Prelude/App.rs`
/// parses exactly this grammar.
fn app_route_binding(method: &str, handler: Option<&Expr>, cx: &Cx) -> String {
    let Some(Expr::Ident(name, _)) = handler else {
        return String::new();
    };
    let (Some(names), Some(Type::Fn { params, .. })) =
        (cx.fn_param_names.get(name), cx.fn_source_types.get(name))
    else {
        return String::new();
    };
    names
        .iter()
        .zip(params.iter())
        .map(|(param, ty)| {
            let (ty, optional) = match ty {
                Type::Option(inner) => (inner.as_ref(), true),
                ty => (ty, false),
            };
            if param == "data" && method != "loader" {
                return "d".to_string();
            }
            if let Type::Named(record) = ty {
                if let Some(fields) = cx.struct_fields.get(record) {
                    let fields = fields
                        .iter()
                        .map(|(field, field_ty)| match field_ty {
                            Type::Option(inner) => {
                                format!("{field}={}?", app_route_type_name(inner))
                            }
                            field_ty => format!("{field}={}", app_route_type_name(field_ty)),
                        })
                        .collect::<Vec<_>>()
                        .join(",");
                    return format!("s=json:{fields}");
                }
            }
            let ty = app_route_type_name(ty);
            let ty = if optional { format!("{ty}?") } else { ty };
            if param == "search" || param == "query" {
                format!("s=query:{param}={ty}")
            } else {
                format!("p:{param}={ty}")
            }
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn lower_game_handle_receiver(
    receiver: &Expr,
    cx: &Cx,
    env: &mut LowerEnv,
) -> Option<TExpr> {
    let Expr::Field(base, field, _) = receiver else {
        return None;
    };
    let owner = tir_recv_jet_ty(base, env)?;
    let Type::Named(owner) = owner else {
        return None;
    };
    let target = match (owner.as_str(), field.as_str()) {
        ("GameScene", "assets") => "GameAssets",
        ("GameScene", "input") => "GameInputMap",
        ("GameFrame", "input") => "GameInputSnapshot",
        _ => return None,
    };
    let base = crate::Codegen::TIR::lower_expr(base, cx, env);
    Some(TExpr {
        ty: Type::Named(target.to_string()),
        kind: TExprKind::Field {
            recv: Box::new(base),
            field: field.clone(),
            boxed: false,
        },
    })
}

fn infer_comptime_host_owner(receiver: &Expr, method: &str) -> Option<&'static str> {
    let comptime_recv = match receiver {
        Expr::ComptimeName { .. } => true,
        Expr::Ident(name, _) => crate::Syntax::is_comptime_name(name),
        _ => false,
    };
    match method {
        "tokens" => Some("CompilerLexed"),
        "items" => Some("CompilerSyntaxTree"),
        "functions" | "effects" | "semantic_index" | "syntax" => Some("CompilerChecked"),
        "generated_lines" | "sources" => Some("CompilerSourceMap"),
        "outputs" | "build_profiles" | "dependencies" if comptime_recv => Some("CompilerManifest"),
        "version" | "schema_version" | "root_dependencies" if comptime_recv => Some("CompilerLock"),
        "packages" if comptime_recv => Some("CompilerLock"),
        "profiles" if comptime_recv => Some("CompilerProfileSet"),
        _ => None,
    }
}

fn host_named_type(ty: &Type) -> Option<String> {
    match ty {
        Type::Named(name) | Type::Apply { name, .. } => Some(name.clone()),
        Type::Option(inner) | Type::Tagged { inner, .. } => host_named_type(inner),
        Type::Result { ok, .. } => host_named_type(ok),
        _ => None,
    }
}

fn lower_build_generate(
    receiver: &Expr,
    method: &str,
    args: &[crate::AST::CallArg],
    resolved_ret: Option<&Type>,
    cx: &Cx,
    env: &mut LowerEnv,
    lowered_receiver: Option<TExpr>,
) -> Option<TExpr> {
    if method != "generate" {
        return None;
    }
    let template = args
        .iter()
        .find_map(|arg| arg.flags.template_items.as_deref())?;
    let source = match crate::Comptime::format_template_body(
        template,
        &std::collections::HashMap::new(),
        &std::collections::HashMap::new(),
        std::path::Path::new("."),
    ) {
        Ok(source) => source,
        Err(diag) => {
            return Some(invariant_method_expr(
                args.last()
                    .map(|arg| arg.span)
                    .unwrap_or_else(|| receiver.span()),
                diag.what,
            ));
        }
    };
    let name_arg = args.iter().find(|arg| arg.flags.template_items.is_none())?;
    Some(in_own_frame(|| {
        let recv = lowered_receiver.unwrap_or_else(|| lower_expr(receiver, cx, env));
        let name = lower_expr(&name_arg.expr, cx, env);
        let source_expr = TExpr {
            ty: Type::String,
            kind: TExprKind::StrLit(vec![TStrPart::Lit(source)]),
        };
        let ty = match resolved_ret {
            Some(ty @ Type::Result { .. }) => ty.clone(),
            _ => crate::Collections::builtin_method_return(
                &Type::Named(Syntax::TYPE_BUILD_CONTEXT.to_string()),
                "generate",
                2,
                false,
            )
            .flatten()
            .unwrap_or_else(unit_type),
        };
        TExpr {
            ty,
            kind: TExprKind::HostCall(Box::new(THostCall::Method {
                recv: Box::new(recv),
                method: "generate".to_string(),
                args: vec![name, source_expr],
            })),
        }
    }))
}

fn lower_comptime_host_method(
    receiver: &Expr,
    method: &str,
    args: &[crate::AST::CallArg],
    recv_type: &Option<String>,
    resolved_ret: Option<&Type>,
    cx: &Cx,
    env: &mut LowerEnv,
    lowered_receiver: Option<TExpr>,
) -> Option<TExpr> {
    if method == "generate" {
        return None;
    }
    let ty_name = recv_type
        .clone()
        .or_else(|| match tir_recv_jet_ty(receiver, env) {
            Some(ty) => host_named_type(&ty),
            None => None,
        })
        .or_else(|| infer_comptime_host_owner(receiver, method).map(str::to_string))?;
    if comptime_host_type_leaf(&ty_name) == Syntax::TYPE_BUILD_CONTEXT {
        return None;
    }
    if !is_comptime_host_method(&ty_name, method) {
        return None;
    }
    let recv = lowered_receiver.unwrap_or_else(|| lower_expr(receiver, cx, env));
    if let Some(actual) = host_named_type(&recv.ty) {
        if !is_comptime_host_method(&actual, method) {
            return None;
        }
    }
    let owner = host_named_type(&recv.ty)
        .filter(|name| is_comptime_host_method(name, method))
        .unwrap_or(ty_name);
    Some(in_own_frame(|| {
        let targs = args
            .iter()
            .map(|arg| lower_expr(&arg.expr, cx, env))
            .collect();
        let ty = resolved_ret
            .cloned()
            .or_else(|| {
                crate::Collections::builtin_method_return(
                    &Type::Named(comptime_host_type_leaf(&owner).to_string()),
                    method,
                    args.len(),
                    false,
                )
                .flatten()
            })
            .unwrap_or_else(unit_type);
        TExpr {
            ty,
            kind: TExprKind::HostCall(Box::new(THostCall::Method {
                recv: Box::new(recv),
                method: method.to_string(),
                args: targs,
            })),
        }
    }))
}

fn lower_method_call_impl(
    receiver: &Expr,
    method: &str,
    method_span: Span,
    owner_type_args: &[Type],
    type_args: &[Type],
    args: &[crate::AST::CallArg],
    recv_type: &Option<String>,
    operator_rhs: Option<&Type>,
    resolved_ret: Option<&Type>,
    checked_widen: bool,
    cx: &Cx,
    env: &mut LowerEnv,
    lowered_receiver: Option<TExpr>,
    instantiated_sig: Option<&[(AccessConvention, Type)]>,
) -> TExpr {
    if let Some(expr) = lower_build_generate(
        receiver,
        method,
        args,
        resolved_ret,
        cx,
        env,
        lowered_receiver.clone(),
    ) {
        return expr;
    }
    if let Some(expr) = lower_comptime_host_method(
        receiver,
        method,
        args,
        recv_type,
        resolved_ret,
        cx,
        env,
        lowered_receiver.clone(),
    ) {
        return expr;
    }
    match hardware_method_op(receiver, method, args, recv_type, cx, env) {
        Ok(Some(op)) => {
            return in_own_frame(|| {
                let unit_receiver = matches!(
                    &op,
                    THardwareCall::RegisterRead { .. }
                        | THardwareCall::RegisterWrite { .. }
                        | THardwareCall::DmaStart { .. }
                );
                let recv = if unit_receiver {
                    TExpr {
                        ty: Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string()),
                        kind: TExprKind::Unit,
                    }
                } else {
                    lowered_receiver
                        .unwrap_or_else(|| crate::Codegen::TIR::lower_expr(receiver, cx, env))
                };
                let lowered_args = args
                    .iter()
                    .map(|arg| crate::Codegen::TIR::lower_expr(&arg.expr, cx, env))
                    .collect::<Vec<_>>();
                let (op, ty) = match op {
                    THardwareCall::DmaStart {
                        profile_id,
                        channel,
                        ..
                    } => {
                        let buffer_ty = lowered_args[1].ty.clone();
                        (
                            THardwareCall::DmaStart {
                                profile_id,
                                channel,
                                buffer_ty: buffer_ty.clone(),
                            },
                            Type::Apply {
                                name: "__JetDmaTransfer".to_string(),
                                args: vec![buffer_ty],
                            },
                        )
                    }
                    THardwareCall::DmaWait {
                        profile_id,
                        channel,
                        ..
                    } => {
                        let Type::Apply { name, args } = &recv.ty else {
                            panic!("checked DMA wait receiver is not a typed transfer");
                        };
                        if name != "__JetDmaTransfer" || args.len() != 1 {
                            panic!("checked DMA wait receiver has an invalid transfer type");
                        }
                        let buffer_ty = args[0].clone();
                        (
                            THardwareCall::DmaWait {
                                profile_id,
                                channel,
                                buffer_ty: buffer_ty.clone(),
                            },
                            buffer_ty,
                        )
                    }
                    other => {
                        let ty = resolved_ret.cloned().unwrap_or_else(|| match &other {
                            THardwareCall::RegisterRead { .. } => Type::Int,
                            THardwareCall::RegisterWrite { .. } => {
                                Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string())
                            }
                            _ => Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string()),
                        });
                        (other, ty)
                    }
                };
                TExpr {
                    ty,
                    kind: TExprKind::HandleMethod {
                        recv: Box::new(recv),
                        op: THandleOp::Hardware(op),
                        args: lowered_args,
                    },
                }
            })
        }
        Ok(None) => {}
        Err(error) => return invariant_method_expr(method_span, error),
    }

    // D-CALLVALUE1=B: sema proved this `.call(...)` receiver is a function
    // value. Lower it through the same TIR function-value node as `Expr::CallValue`.
    // `bind_arg_temporaries` keys temps by `site`; the receiver span is the
    // nested callee's start and would rebind `.call`'s args onto that call.
    if recv_type.as_deref() == Some(Syntax::INTERNAL_CALL_VALUE) {
        return in_own_frame(|| {
            let callee = lowered_receiver
                .unwrap_or_else(|| crate::Codegen::TIR::lower_expr(receiver, cx, env));
            return lower_fn_value_call(
                Some(receiver),
                callee,
                args,
                method_span.start as u32,
                cx,
                env,
            );
        });
    }
    // D-VALIDATE1: sema has already materialized `Validate.over(value)` as
    // the compiler-owned builder literal. Erase only this surface head; the
    // literal itself is the ordinary TIR value consumed by `.check`/`.finish`.
    if recv_type.as_deref() == Some(Syntax::INTERNAL_VALIDATE_OVER)
        && method == "over"
        && args.is_empty()
    {
        return lowered_receiver
            .unwrap_or_else(|| crate::Codegen::TIR::lower_expr(receiver, cx, env));
    }
    // D-FOUND-RECEIPT1: the ambient handle has no runtime receiver. TIR keeps
    // an explicit typed handle op, while MIR lowers it to the existing CoreCall
    // row and appends these declaration-owned metadata operands.
    if recv_type.as_deref() == Some(Syntax::INTERNAL_RECEIPT_HANDLE)
        && method == Syntax::METHOD_RECEIPT_ATTACH
    {
        return in_own_frame(|| {
            let Some(ret) = resolved_ret.cloned() else {
                return invariant_method_expr(
                    method_span,
                    "receipt.attach without a resolved return type",
                );
            };
            if args.len() != 1 {
                return invariant_method_expr(
                    method_span,
                    "receipt.attach without exactly one payload",
                );
            }
            let payload = lower_expr(&args[0].expr, cx, env);
            let type_name = match &payload.ty {
                Type::Named(name) | Type::Apply { name, .. } => name.clone(),
                _ => {
                    return invariant_method_expr(
                        method_span,
                        "receipt.attach payload has no named declaration",
                    );
                }
            };
            let Some(fact) = cx.receipt_sections.get(&type_name) else {
                return invariant_method_expr(
                    method_span,
                    "receipt.attach payload has no checked receipt metadata",
                );
            };
            let metadata = [
                fact.name.clone(),
                fact.type_name.clone(),
                fact.schema_digest.clone(),
            ]
            .into_iter()
            .map(|value| TExpr {
                ty: Type::String,
                kind: TExprKind::StrLit(vec![TStrPart::Lit(value)]),
            })
            .collect::<Vec<_>>();
            let mut targs = Vec::with_capacity(4);
            targs.push(payload);
            targs.extend(metadata);
            TExpr {
                ty: ret,
                kind: TExprKind::HandleMethod {
                    recv: Box::new(TExpr {
                        ty: Type::Named(Syntax::INTERNAL_UNIT_TYPE.to_string()),
                        kind: TExprKind::Unit,
                    }),
                    op: THandleOp::ReceiptAttach,
                    args: targs,
                },
            }
        });
    }
    if method == Syntax::conversion_method_for_source("Int")
        && args.len() == 1
        && inline_range_receiver(receiver).is_some()
    {
        return in_own_frame(|| {
            let Some((lo, hi)) = inline_range_receiver(receiver) else {
                return invariant_method_expr(
                    method_span,
                    "inline-range conversion without a resolved range",
                );
            };
            let Some(ty) = resolved_ret.cloned() else {
                return invariant_method_expr(
                    method_span,
                    "inline-range conversion without a resolved return type",
                );
            };
            let input = lower_expr(&args[0].expr, cx, env);
            let fallible = matches!(ty, Type::Result { .. });
            TExpr {
                ty,
                kind: TExprKind::NumericMethod {
                    recv: Box::new(input),
                    op: TNumericOp::InlineRange { lo, hi, fallible },
                },
            }
        });
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
    let root_call = root_call.and_then(|(root_alias, root_core, root_name)| {
        (root_core.is_none()
            || (root_core.as_deref() == Some("core.term") && root_name == Syntax::BUILTIN_PRINT))
            .then_some((root_alias, root_core, root_name))
    });
    if let Some((root_alias, root_core, root_name)) = root_call {
        return in_own_frame(|| {
            let Some(ret) = resolved_ret.cloned() else {
                return invariant_method_expr(
                    method_span,
                    "root call without a resolved return type",
                );
            };
            if root_core.as_deref() == Some("core.term") && root_name == Syntax::BUILTIN_PRINT {
                return in_own_frame(|| {
                    // Core print is the first prelude `#Root` function. Lower
                    // receiver and arguments as ordered one-value Print nodes;
                    // each node retains its checked Printable/Display contract.
                    let receiver = lowered_receiver
                        .unwrap_or_else(|| crate::Codegen::TIR::lower_expr(receiver, cx, env));
                    crate::Codegen::TIR::lower::print_values(
                        std::iter::once(receiver).chain(
                            args.iter()
                                .map(|arg| crate::Codegen::TIR::lower_expr(&arg.expr, cx, env)),
                        ),
                        cx,
                        method_span.start,
                    )
                });
            }
            let sig = root_alias.as_ref().and_then(|alias| {
                cx.import_sigs
                    .get(&(alias.clone(), root_name.clone()))
                    .cloned()
            });
            let Some(sig) = sig.or_else(|| cx.sigs.get(&root_name).cloned()) else {
                return invariant_method_expr(
                    method_span,
                    "root call without a resolved signature",
                );
            };
            let receiver = lowered_receiver
                .unwrap_or_else(|| crate::Codegen::TIR::lower_expr(receiver, cx, env));
            let receiver_conv = sig.first().cloned();
            let receiver_arg = TCallArg {
                borrow: receiver_conv
                    .as_ref()
                    .is_some_and(|(conv, ty)| *conv == AccessConvention::Read && !ty.is_scalar()),
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
                box_as_trait: None,
            };
            let mut lowered_args = Vec::with_capacity(args.len() + 1);
            lowered_args.push(receiver_arg);
            lowered_args.extend(args.iter().enumerate().map(|(index, arg)| {
                let conv = sig.get(index + 1).cloned();
                lower_one_call_arg(arg, conv, env, cx)
            }));
            if let Some(alias) = root_alias {
                return in_own_frame(|| {
                    let Some((rust_mod, rust_fn)) = cx
                        .reexport_calls
                        .get(&(alias.clone(), root_name.clone()))
                        .cloned()
                        .or_else(|| {
                            cx.import_mods
                                .get(&alias)
                                .cloned()
                                .map(|module| (module, root_name.clone()))
                        })
                    else {
                        return invariant_method_expr(
                            method_span,
                            "root import without a codegen target",
                        );
                    };
                    let target_return = match imported_module_call_target_return(
                        cx,
                        &alias,
                        &root_name,
                        resolved_ret,
                    ) {
                        Ok(target_return) => target_return,
                        Err(error) => return invariant_method_expr(method_span, error),
                    };
                    return TExpr {
                        ty: ret,
                        kind: TExprKind::ModuleCall {
                            form: TModuleCallForm::Qualified {
                                rust_mod,
                                rust_fn: mangle(&rust_fn).to_string(),
                            },
                            target_return,
                            type_args: type_args.to_vec(),
                            args: lowered_args,
                        },
                    };
                });
            }
            let call_ty =
                call_return_type_with_args(cx, &root_name, type_args, &lowered_args);
            let lowered = TExpr {
                ty: call_ty,
                kind: TExprKind::Call {
                    name: cx.jit_local_call_prefix.as_ref().map_or_else(
                        || root_name.clone(),
                        |prefix| format!("{prefix}{}", mangle(&root_name)),
                    ),
                    type_args: type_args.to_vec(),
                    args: lowered_args,
                },
            };
            let lowered = match source_arg_order(args) {
                Some(order) => preserve_source_arg_order(
                    lowered,
                    &order,
                    args.len(),
                    method_span.start as u32,
                ),
                None => lowered,
            };
            return consume_plain_helper_route(resolved_ret, method_span, lowered, cx, env);
        });
    }
    // D-FAIL-CARRIER1=A: `.or_err("why")` lifts a clean absence into a failure.
    // One carrier, so the payload rides through untouched and only the report
    // changes. The prelude's `JetOptionalView::or_err` holds that one meaning;
    // this is a plain marshalling call onto it.
    if method == Syntax::METHOD_OUTCOME_OR_ERR
        && args.len() == 1
        && matches!(tir_recv_jet_ty(receiver, env), Some(Type::Option(_)))
    {
        return in_own_frame(|| {
            let Some(ty) = resolved_ret.cloned() else {
                return invariant_method_expr(
                    method_span,
                    "or_err call without a resolved return type",
                );
            };
            // Map.get (and similar) keep source `fn(...) T` in sema; stored
            // named functions already return the Result carrier. Carry that
            // ABI through `.or_err` so the binding is callable without a
            // second Ok wrap.
            let ty = ty.with_effective_fn_returns();
            let recv = lower_expr(receiver, cx, env);
            let why = lower_one_call_arg(&args[0], None, env, cx);
            let receiver = TCallArg {
                value: recv,
                template_items: None,
                borrow: false,
                mut_borrow: false,
                clone: false,
                arc_clone: false,
                fn_coerce: None,
                widen_to_vec: false,
                widen_to_union: None,
                box_as_trait: None,
            };
            TExpr {
                ty,
                kind: TExprKind::StaticCall {
                    owner: rooted_owner("JetOptionalView"),
                    owner_type: None,
                    method: TMethodRef::bare(method),
                    type_args: Vec::new(),
                    args: vec![receiver, why],
                },
            }
        });
    }
    // D-FAIL-CARRIER1=A: the carrier's middle states. `.partial` reads the
    // payload a failure kept and `.notes` reads what it had to say. Both live
    // on the outcome value; the prelude's `jet_partial`/`jet_notes` hold that
    // one meaning, and one node carries both to every engine.
    if recv_type.as_deref() == Some("__Carrier__") {
        return in_own_frame(|| {
            let Some(ty) = resolved_ret.cloned() else {
                return invariant_method_expr(
                    method_span,
                    "carrier fact without a resolved return type",
                );
            };
            let notes = method == Syntax::METHOD_OUTCOME_NOTES;
            let recv = lower_expr(receiver, cx, env);
            TExpr {
                ty,
                kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::CarrierFact {
                    recv: Box::new(recv),
                    field: if notes {
                        Syntax::FIELD_OUTCOME_NOTES.to_string()
                    } else {
                        Syntax::FIELD_OUTCOME_PARTIAL.to_string()
                    },
                    notes,
                })),
            }
        });
    }
    let guard_receiver = tir_recv_jet_ty(receiver, env).and_then(|ty| match ty {
        Type::Tagged { marker, inner } => match inner.as_ref() {
            Type::Apply { name, args } if name == Syntax::TYPE_SHARED_GUARD && args.len() == 1 => {
                Some((
                    args[0].clone(),
                    matches!(
                        marker,
                        crate::AST::TagMarker::Internal(crate::AST::InternalTag::SharedGuardEdit)
                    ),
                ))
            }
            _ => None,
        },
        _ => None,
    });
    if let Some((inner, editable)) = guard_receiver {
        match (method, args) {
            ("map", [arg]) => {
                return in_own_frame(|| {
                    let Some(ty) = resolved_ret.cloned() else {
                        return invariant_method_expr(
                            method_span,
                            "SharedGuard.map without a resolved return type",
                        );
                    };
                    let Expr::Lambda(lambda) = &arg.expr else {
                        return invariant_method_expr(
                            method_span,
                            "SharedGuard.map without a projection lambda",
                        );
                    };
                    let Some(path) = lambda.meta.guard_projection.clone() else {
                        return invariant_method_expr(
                            method_span,
                            "SharedGuard.map without a resolved projection",
                        );
                    };
                    TExpr {
                        ty,
                        kind: TExprKind::SharedGuardMap {
                            guard: Box::new(lower_expr(receiver, cx, env)),
                            path,
                            editable,
                        },
                    }
                });
            }
            ("split", [first, second]) => {
                return in_own_frame(|| {
                    let Some(ty) = resolved_ret.cloned() else {
                        return invariant_method_expr(
                            method_span,
                            "SharedGuard.split without a resolved return type",
                        );
                    };
                    if !matches!(ty, Type::Tuple(_)) {
                        return invariant_method_expr(
                            method_span,
                            "SharedGuard.split without a tuple return type",
                        );
                    }
                    let (Expr::Lambda(first), Expr::Lambda(second)) = (&first.expr, &second.expr)
                    else {
                        return invariant_method_expr(
                            method_span,
                            "SharedGuard.split without projection lambdas",
                        );
                    };
                    let Some(first_path) = first.meta.guard_projection.clone() else {
                        return invariant_method_expr(
                            method_span,
                            "SharedGuard.split without its first projection",
                        );
                    };
                    let Some(second_path) = second.meta.guard_projection.clone() else {
                        return invariant_method_expr(
                            method_span,
                            "SharedGuard.split without its second projection",
                        );
                    };
                    TExpr {
                        ty,
                        kind: TExprKind::SharedGuardSplit {
                            guard: Box::new(lower_expr(receiver, cx, env)),
                            first: first_path,
                            second: second_path,
                            editable,
                        },
                    }
                });
            }
            ("wait", [condition, predicate]) => {
                return in_own_frame(|| {
                    let Some(ty) = resolved_ret.cloned() else {
                        return invariant_method_expr(
                            method_span,
                            "SharedGuard.wait without a resolved return type",
                        );
                    };
                    let Expr::Lambda(predicate) = &predicate.expr else {
                        return invariant_method_expr(
                            method_span,
                            "SharedGuard.wait without a predicate lambda",
                        );
                    };
                    let expected = std::slice::from_ref(&inner);
                    TExpr {
                        ty,
                        kind: TExprKind::SharedGuardWait {
                            guard: Box::new(lower_expr(receiver, cx, env)),
                            condition: Box::new(lower_expr(&condition.expr, cx, env)),
                            predicate: Box::new(lower_lambda_expecting_host_borrow(
                                predicate, cx, env, expected, false,
                            )),
                        },
                    }
                });
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
        return in_own_frame(|| {
            return TExpr {
                ty: unit_type(),
                kind: TExprKind::ConditionNotify {
                    condition: Box::new(lower_expr(receiver, cx, env)),
                    all: method == "notify_all",
                },
            };
        });
    }

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
            if crate::Sema::core_call_signature(module, method)
                .as_ref()
                .and_then(|(params, _)| params.get(index))
                .is_some_and(|(access, _)| *access == AccessConvention::Write)
            {
                let value = lower_expr(expr, cx, env);
                if matches!(value.kind, TExprKind::Borrow { .. }) {
                    return value;
                }
                return TExpr {
                    ty: value.ty.clone(),
                    kind: TExprKind::Borrow {
                        place: Box::new(value),
                        mutable: true,
                    },
                };
            }
            // Comptime/MirBridge lowers core-call arguments before sema elaborates
            // inferred typed literals. At a regex one-shot's first parameter, the
            // expected type is unambiguously Regex; lower the same checked literal
            // node that normal sema produces.
            if module == "core.regex"
                && index == 0
                && matches!(
                    method,
                    "is_match"
                        | "full_match"
                        | "match"
                        | "find"
                        | "find_all"
                        | "matches"
                        | "split"
                        | "split_limit"
                        | "replace"
                        | "replace_first"
                )
            {
                if let Expr::TypedLit {
                    head: None,
                    body: crate::AST::TypedLitBody::Value(pattern),
                    span,
                } = expr
                {
                    let record = match checked_core_record("core.regex", "literal", 1, *span) {
                        Ok(record) => record,
                        Err(expr) => return expr,
                    };
                    let regex_ty = Type::Named(Syntax::TYPE_REGEX.to_string());
                    return TExpr {
                        ty: regex_ty.clone(),
                        kind: TExprKind::CoreCall {
                            record,
                            args: vec![lower_expr(pattern, cx, env)],
                            source_span: *span,
                            type_args: Vec::new(),
                            widen_to_vec: vec![false],
                            data_plan: None,
                            fallibility: TFailureCarrier::from_checked_type(&regex_ty),
                        },
                    };
                }
            }

            if module == "core.text.fmt" && method == "pretty" && index == 0 {
                return lower_debug_text(lower_expr(expr, cx, env));
            }

            // Both exact carriers cross here, matching the checker: it admits
            // Fraction and Decimal wherever a math call leaves the exact world,
            // so lowering must convert both or the tiers disagree with the type
            // that was already accepted. The multi-argument family crosses at
            // every argument, not only the first.
            let crosses = module == "core.math"
                && ((index == 0 && exact_rational_math_approx(method))
                    || matches!(
                        method,
                        "atan2" | "hypot" | "lerp" | "copysign" | "log" | "fma"
                    ));
            if crosses {
                let value = lower_expr(expr, cx, env);
                let exact = match &value.ty {
                    Type::Named(type_name) if type_name == Syntax::TYPE_FRACTION => {
                        Some(Syntax::TYPE_FRACTION)
                    }
                    Type::Named(type_name) if type_name == Syntax::TYPE_DECIMAL => {
                        Some(Syntax::TYPE_DECIMAL)
                    }
                    _ => None,
                };
                if let Some(type_name) = exact {
                    return TExpr {
                        ty: Type::Float,
                        kind: TExprKind::PreciseBuiltin {
                            type_name: type_name.to_string(),
                            func: "to_float".to_string(),
                            args: vec![value],
                        },
                    };
                }
                return value;
            }
            // Core signatures carry optional argument slots explicitly. Source
            // callers provide either the payload value or the binder's Absent;
            // normalize both forms before MIR sees the carrier.
            let core_arg_ty = crate::Sema::core_fixed_sig(module, method)
                .and_then(|(params, _)| params.get(index).map(|(_, ty)| ty.clone()));
            if let Some(Type::Option(inner)) = core_arg_ty {
                let option_ty = Type::Option(inner.clone());
                if matches!(expr, Expr::Absent(_)) {
                    return TExpr {
                        ty: option_ty,
                        kind: TExprKind::Absent,
                    };
                }
                let value = lower_expr(expr, cx, env);
                if value.ty == option_ty {
                    return value;
                }
                let value = preserve_typed_list_shape(value, &inner, cx);
                return TExpr {
                    ty: option_ty,
                    kind: TExprKind::Present(Box::new(value)),
                };
            }

            let value = lower_expr(expr, cx, env);
            // D-WEBQUERY1: A live query rerun can execute through the shared
            // registry. Carry its zero-argument callback as the checked
            // SendFn carrier so AOT emits Arc<dyn Fn + Send + Sync>, matching
            // the runtime's cross-thread query kernel.
            if module == "core.web.query"
                && method == "live"
                && index == 3
                && !matches!(
                    &value.kind,
                    TExprKind::FnValue {
                        kind: TFnValueKind::Send { .. }
                    }
                )
            {
                let ty = value.ty.clone();
                return TExpr {
                    ty,
                    kind: TExprKind::FnValue {
                        kind: TFnValueKind::Send {
                            value: Box::new(value),
                        },
                    },
                };
            }
            // D-FOUND-LIFECYCLE1: `core.http.server.bind/serve` have one
            // canonical four-slot ABI. The binder supplies `Absent` for
            // omitted `tls:`/`deadline:` controls; present values must become
            // typed options before MIR marshals them to AOT or JIT.
            if module == "core.http.server"
                && matches!(method, "bind" | "serve")
                && matches!(index, 2 | 3)
            {
                let inner_ty = if index == 2 {
                    Type::Named("HTTPServerTls".to_string())
                } else {
                    Type::Named("Duration".to_string())
                };
                let option_ty = Type::Option(Box::new(inner_ty));
                if matches!(expr, Expr::Absent(_)) {
                    return TExpr {
                        ty: option_ty,
                        kind: TExprKind::Absent,
                    };
                }
                return TExpr {
                    ty: option_ty,
                    kind: TExprKind::Present(Box::new(value)),
                };
            }
            // D-FOUND-COREAPI1=A: `game.run` has one canonical four-slot ABI.
            // The binder inserts `Absent` for replay/backend/frames; present
            // values cross as typed options so every tier sees the same plan.
            if module == "core.game" && method == "run" && matches!(index, 1 | 2 | 3) {
                let inner_ty = match index {
                    1 => Type::Named("GameReplay".to_string()),
                    2 => Type::Named("GameBackend".to_string()),
                    3 => Type::Int,
                    _ => unreachable!("game.run optional slot is outside 1..=3"),
                };
                let option_ty = Type::Option(Box::new(inner_ty));
                if matches!(expr, Expr::Absent(_)) {
                    return TExpr {
                        ty: option_ty,
                        kind: TExprKind::Absent,
                    };
                }
                return TExpr {
                    ty: option_ty,
                    kind: TExprKind::Present(Box::new(value)),
                };
            }
            if let Some((params, _)) = crate::Sema::core_fixed_sig(module, method) {
                if let Some((_, ty)) = params.get(index) {
                    return preserve_typed_list_shape(value, ty, cx);
                }
            }
            value
        };

    // D-ZIPPAD1: lower the complete list/iterator zip family as one TIR
    // contract. The shared node keeps free and method spellings identical;
    // the Prelude emitter supplies the lazy binary composition.
    if recv_type.is_none() && matches!(method, "zip" | "zip_short" | "zip_pad") {
        let lowered_recv = lower_expr(receiver, cx, env);
        if zip_sequence_type(&lowered_recv.ty) {
            return in_own_frame(|| {
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
                            fields.push(zip_field_name(
                                fields.len(),
                                arg.label.as_ref().map(|(name, _)| name.as_str()),
                            ));
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
                    method_span,
                    resolved_ret,
                );
            });
        }
        *lowered_receiver.borrow_mut() = Some(lowered_recv);
    }

    if let Expr::Ident(name, _) = receiver {
        if env.is_gc(name) {
            return in_own_frame(|| {
                let root = env.local_of(name);
                let edge_args = match method {
                    "insert" if args.len() > 1 => &args[1..],
                    "remove" => &args[0..0],
                    _ => args,
                };
                let mut edge_names = edge_args
                    .iter()
                    .flat_map(|arg| env.gc_edges_for_expr(&arg.expr, Some(name)))
                    .collect::<Vec<_>>();
                edge_names.sort();
                edge_names.dedup();
                let edges = edge_names
                    .iter()
                    .map(|edge| env.local_of(edge))
                    .collect::<Vec<_>>();
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
                    operator_rhs,
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
            });
        }
    }
    if recv_type.as_ref().is_some_and(|name| {
        cx.current_type_params.borrow().contains(name.as_str())
            || matches!(
                name.as_str(),
                crate::Generics::IO_READER | crate::Generics::IO_WRITER
            )
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
            | "live"
            | "begin"
            | "commit"
            | "rollback"
    ) {
        let Some(recv_name) = recv_type.clone() else {
            return invariant_method_expr(
                method_span,
                "generic receiver method without a resolved receiver type",
            );
        };
        return in_own_frame(|| {
            let Some(ret) = resolved_ret.cloned() else {
                return invariant_method_expr(
                    method_span,
                    "generic receiver method without a resolved return type",
                );
            };
            let recv = lower_expr(receiver, cx, env);
            let targs: Vec<_> = in_own_frame(|| match method {
                "query" | "query_one" | "execute" | "live" => {
                    let sql_ty = Type::Named("SQL".to_string());
                    args.iter()
                        .map(|arg| {
                            lower_one_call_arg(arg, Some((arg.convention, sql_ty.clone())), env, cx)
                        })
                        .collect()
                }
                "begin" | "commit" | "rollback" => args
                    .iter()
                    .map(|arg| lower_one_call_arg(arg, None, env, cx))
                    .collect(),
                _ => {
                    let arg_ty = match method {
                        "read" => Type::Int,
                        "write" | "write_all" => Type::List(Box::new(Type::IntN {
                            signed: false,
                            bits: 8,
                        })),
                        _ => Type::Named(recv_name.clone()),
                    };
                    args.iter()
                        .map(|arg| {
                            lower_one_call_arg(arg, Some((arg.convention, arg_ty.clone())), env, cx)
                        })
                        .collect()
                }
            });
            TExpr {
                ty: ret,
                kind: TExprKind::MethodCall {
                    recv: Box::new(recv),
                    method: TMethodRef::bare(method),
                    type_args: Vec::new(),
                    args: targs,
                    source_first_string_literal: first_string_literal_arg(args),
                    operator_line: matches!(method, "add" | "sub" | "mul" | "div").then(|| {
                        crate::Diagnostics::span_line_col(&cx.src, method_span.start).0 as u32
                    }),
                },
            }
        });
    }
    // D-NURSERY1/A: from `[Task<T>]` list type extract `T` (the joined result
    // type). A normalized fallible child is internally `Task<Result<T, E>>`,
    // but the task-group surface still returns the successful `T`; the
    // `TaskFailure` rail is the combinator's only visible error.

    /// D-CONC-ALLNAMED1=A: `task.all` carries its named shape in the existing
    /// tuple carrier. Each tuple field is a `Task<T>` at the input and becomes
    /// the corresponding `T` in the fallible output tuple. Normalized
    /// `Task<Result<T, E>>` fields expose only `T` at this boundary.

    /// Lower a named `task.all` carrier, then restore its authored field order.
    /// Ordinary tuple literals stay canonical in `lower_expr`; only this
    /// task-group boundary is allowed to reorder the already-lowered fields.
    fn lower_taskgroup_all_carrier(expr: &Expr, cx: &Cx, env: &mut LowerEnv) -> TExpr {
        let mut tasks = crate::Codegen::TIR::lower_expr(expr, cx, env);
        let Expr::TupleLit(lit_fields, _, _) = expr else {
            return tasks;
        };
        let fields: &mut Vec<(String, TExpr)> = match &mut tasks.kind {
            TExprKind::TupleLit { fields, .. } => fields,
            _ => return tasks,
        };
        let field_count = fields.len();
        let canonical_names = fields
            .iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>();
        let mut lowered_by_name = std::collections::HashMap::with_capacity(field_count);
        for (name, value) in fields.drain(..) {
            lowered_by_name.insert(name, value);
        }
        let authored = lit_fields
            .iter()
            .filter_map(|(name, _)| {
                lowered_by_name
                    .remove(name)
                    .map(|value| (name.clone(), value))
            })
            .collect::<Vec<_>>();
        if authored.len() == field_count && lowered_by_name.is_empty() {
            fields.extend(authored);
        } else {
            let mut authored_by_name = authored
                .into_iter()
                .collect::<std::collections::HashMap<_, _>>();
            fields.extend(canonical_names.into_iter().filter_map(|name| {
                authored_by_name
                    .remove(&name)
                    .or_else(|| lowered_by_name.remove(&name))
                    .map(|value| (name, value))
            }));
        }
        tasks
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
        return in_own_frame(|| {
            let module = "core.crypto";
            let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
            let widen_to_vec = core_widen_to_vec(module, helper, &targs);
            let Some(ty) = resolved_ret.cloned() else {
                return invariant_method_expr(
                    method_span,
                    "crypto static call without a resolved return type",
                );
            };
            let record = match checked_core_record(module, helper, targs.len(), method_span) {
                Ok(record) => record,
                Err(expr) => return expr,
            };
            TExpr {
                ty: ty.clone(),
                kind: TExprKind::CoreCall {
                    record,
                    args: targs,
                    source_span: method_span,
                    type_args: Vec::new(),
                    widen_to_vec,
                    data_plan: None,
                    fallibility: TFailureCarrier::from_checked_type(&ty),
                },
            }
        });
    }
    if let Some(kind) = recv_type
        .as_deref()
        .map(|name| name.rsplit('.').next().unwrap_or(name))
    {
        if let Some(helper) = crypto_instance_helper(kind, method) {
            return in_own_frame(|| {
                let recv = lower_expr(receiver, cx, env);
                let mut targs = vec![recv];
                if kind == "Hasher" && method == "update" {
                    targs.extend(args.iter().map(|arg| lower_expr(&arg.expr, cx, env)));
                }
                let widen_to_vec = core_widen_to_vec("core.crypto", helper, &targs);
                let ty = match resolved_ret.cloned() {
                    Some(ty) => ty,
                    None if kind == "Hasher" && method == "update" => {
                        Type::Named(crate::Syntax::INTERNAL_UNIT_TYPE.to_string())
                    }
                    None => {
                        return invariant_method_expr(
                            method_span,
                            "crypto instance call without a resolved return type",
                        );
                    }
                };
                let record =
                    match checked_core_record("core.crypto", helper, targs.len(), method_span) {
                        Ok(record) => record,
                        Err(expr) => return expr,
                    };
                TExpr {
                    ty: ty.clone(),
                    kind: TExprKind::CoreCall {
                        record,
                        args: targs,
                        source_span: method_span,
                        type_args: Vec::new(),
                        widen_to_vec,
                        data_plan: None,
                        fallibility: TFailureCarrier::from_checked_type(&ty),
                    },
                }
            });
        }
    }
    // D-TOOL4: `expect(x).snapshot()` — render the harness snapshot call. Test
    // bodies are `Result<(), String>`, so the trailing `?` propagates mismatch.
    if method == Syntax::BUILTIN_SNAPSHOT {
        if let Expr::Call(call) = receiver {
            if call.name == Syntax::BUILTIN_EXPECT && call.args.len() == 1 {
                return in_own_frame(|| {
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
                });
            }
        }
    }

    // D-TYPEDTEXT1=D: `SQL.raw("…")` / `HTML.raw("…")` — the audited escape.
    // `SQL`/`HTML` name the type here (sema already confirmed no local shadows
    // it), so `recv_type` was never set for this call; check the receiver
    // shape directly instead.
    if method == "raw" {
        if let Expr::Ident(n, _) = receiver {
            let is_builtin = Syntax::typed_head_kind(n).is_some_and(|kind| kind.is_typed_text());
            if is_builtin {
                return in_own_frame(|| {
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
                        kind: TExprKind::HostCall(Box::new(
                            crate::Codegen::TIR::THostCall::TypedText {
                                kind,
                                arg: Box::new(arg),
                            },
                        )),
                    };
                });
            }
        }
        if let Some(type_name) = static_call_type_name_lower(receiver, env) {
            if cx.string_distinct_has_trait_method(&type_name, "check")
                && cx.string_distinct_has_trait_method(&type_name, "encode_hole")
            {
                return in_own_frame(|| {
                    let Some(ty) = resolved_ret.cloned() else {
                        return invariant_method_expr(
                            method_span,
                            "typed-text raw without a resolved return type",
                        );
                    };
                    TExpr {
                        ty,
                        kind: TExprKind::StaticCall {
                            owner: TStaticOwner::User(type_name.clone()),
                            owner_type: Some(Type::Named(type_name)),
                            method: TMethodRef::bare("raw"),
                            type_args: Vec::new(),
                            args: args
                                .iter()
                                .map(|argument| lower_one_call_arg(argument, None, env, cx))
                                .collect(),
                        },
                    }
                });
            }
        }
    }
    // D-TYPEDSQL-SINK1=A: `.template()`/`.params()` project the checked `SQL`
    // value; `.params()` exposes its ordered `DBValue` bindings.
    if recv_type.as_deref() == Some("SQL") && matches!(method, "template" | "params") {
        return in_own_frame(|| {
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
                    Type::List(Box::new(Type::Named(Syntax::TYPE_DB_VALUE.to_string())))
                },
                kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::TypedText {
                    kind,
                    arg: Box::new(recv),
                })),
            };
        });
    }
    if recv_type.as_deref() == Some("HTML") && method == "text" {
        return in_own_frame(|| {
            let recv = lower_expr(receiver, cx, env);
            return TExpr {
                ty: Type::String,
                kind: TExprKind::Clone(Box::new(recv)),
            };
        });
    }
    // D-COLLBREADTH1=A: lower type-owned set constructors into the same
    // receiver-first builtin used by instance algebra, preserving the
    // concrete set type for chained dispatch.
    if recv_type.is_none() {
        if let Expr::Ident(type_name, _) = receiver {
            if !env.locals.contains_key(type_name)
                && matches!(type_name.as_str(), Syntax::TYPE_SET | Syntax::TYPE_RANK)
                && method == "from"
                && args.len() == 1
            {
                return in_own_frame(|| {
                    let lowered_list = lower_expr(&args[0].expr, cx, env);
                    let set_ty = match resolved_ret {
                        Some(set_ty) => {
                            if !matches!(
                                set_ty,
                                Type::Apply { name, args } if name == type_name && args.len() == 1
                            ) {
                                return invariant_method_expr(
                                    method_span,
                                    "set constructor without its checked element type",
                                );
                            }
                            set_ty.clone()
                        }
                        None => {
                            let elem = match lowered_list.ty.without_user_tags() {
                                Type::List(inner) | Type::FixedList { elem: inner, .. } => {
                                    (**inner).clone()
                                }
                                _ => {
                                    return invariant_method_expr(
                                        method_span,
                                        "set constructor without its checked element type",
                                    );
                                }
                            };
                            Type::Apply {
                                name: type_name.clone(),
                                args: vec![elem],
                            }
                        }
                    };
                    TExpr {
                        ty: set_ty,
                        kind: TExprKind::BuiltinMethod {
                            recv: Box::new(lowered_list),
                            op: if type_name == Syntax::TYPE_RANK {
                                TBuiltinOp::SortedSetFrom
                            } else {
                                TBuiltinOp::SetFrom
                            },
                            args: Vec::new(),
                        },
                    }
                });
            }
        }
    }
    // D-CONC-SPAWN1=D: parser-created `task` nodes outside a lexical group
    // still lower through the existing spawn/select TIR nodes. The receiver is
    // compiler-private and is never emitted or looked up as a Rust value.
    if recv_type.as_deref() == Some(Syntax::INTERNAL_TASK_SURFACE_TYPE) {
        if method == Syntax::INTERNAL_TASK_TIMEOUT_METHOD && args.len() == 1 {
            return in_own_frame(|| {
                let Some(ty) = resolved_ret.cloned() else {
                    return invariant_method_expr(
                        method_span,
                        "task timeout without a resolved return type",
                    );
                };
                let duration = lower_expr(&args[0].expr, cx, env);
                let record = match checked_core_record("core.tasks", "timeout", 1, method_span) {
                    Ok(record) => record,
                    Err(expr) => return expr,
                };
                TExpr {
                    ty: ty.clone(),
                    kind: TExprKind::CoreCall {
                        record,
                        args: vec![duration],
                        source_span: method_span,
                        type_args: Vec::new(),
                        widen_to_vec: vec![false],
                        data_plan: None,
                        fallibility: TFailureCarrier::from_checked_type(&ty),
                    },
                }
            });
        }
        if method == Syntax::INTERNAL_TASK_SPAWN_METHOD {
            if let Some(Expr::Lambda(lam)) = args.first().map(|a| &a.expr) {
                let Some(_) = resolved_ret else {
                    return invariant_method_expr(
                        method_span,
                        "task spawn without a resolved return type",
                    );
                };
                return in_own_frame(|| {
                    let carrier_ty = spawn_body_carrier_ty(lam, cx, env);
                    // Sema's Task<T> is the source metadata; TIR retains the
                    // closure carrier so join lowering can flatten its own E
                    // into the enclosing function's carrier.
                    let mut spawn_env = clone_env(env);
                    spawn_env.ret_ty = Some(carrier_ty.clone());
                    let site = jit_spawn_site(lam, cx, env);
                    let label = spawn_label(lam, cx, env);
                    let executable = Box::new(lower_lambda_expecting_value_with_return(
                        lam,
                        cx,
                        &spawn_env,
                        &[],
                        &carrier_ty,
                    ));
                    return TExpr {
                        ty: Type::Apply {
                            name: "Task".to_string(),
                            args: vec![carrier_ty],
                        },
                        kind: TExprKind::CoreClosureCall {
                            kind: TCoreClosureKind::Spawn {
                                group: None,
                                site,
                                label,
                                executable,
                            },
                        },
                    };
                });
            }
            return invariant_method_expr(
                method_span,
                "task spawn without a checked lambda argument",
            );
        }
        if args.len() == 1 {
            return in_own_frame(|| {
                let tasks = if method == Syntax::INTERNAL_TASK_ALL_METHOD {
                    lower_taskgroup_all_carrier(&args[0].expr, cx, env)
                } else {
                    lower_expr(&args[0].expr, cx, env)
                };
                let Some(ty) = resolved_ret.cloned() else {
                    return invariant_method_expr(
                        method_span,
                        "task group without a resolved return type",
                    );
                };
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
                    _ => {
                        return invariant_method_expr(method_span, "unknown task group operation");
                    }
                };
                return TExpr { ty, kind };
            });
        }
    }

    // D-CONC-SPAWN1=D: compiler-private methods behind canonical `task.group`.
    if matches!(
        recv_type.as_deref(),
        Some(Syntax::TYPE_TASKGROUP) | Some(Syntax::INTERNAL_TASK_GROUP_SURFACE_TYPE)
    ) && method == Syntax::INTERNAL_TASK_SPAWN_METHOD
    {
        let Some(Expr::Lambda(lam)) = args.first().map(|a| &a.expr) else {
            return invariant_method_expr(
                method_span,
                "task-group spawn without a checked lambda argument",
            );
        };
        let Some(_) = resolved_ret else {
            return invariant_method_expr(
                method_span,
                "task-group spawn without a resolved return type",
            );
        };
        return in_own_frame(|| {
            let carrier_ty = spawn_body_carrier_ty(lam, cx, env);
            // Group handles expose source Task<T> metadata through sema;
            // TIR retains the closure carrier for join's flatten adapter.
            let mut spawn_env = clone_env(env);
            spawn_env.ret_ty = Some(carrier_ty.clone());
            let site = jit_spawn_site(lam, cx, env);
            let label = spawn_label(lam, cx, env);
            let executable = Box::new(lower_lambda_expecting_value_with_return(
                lam,
                cx,
                &spawn_env,
                &[],
                &carrier_ty,
            ));
            let group = lower_expr(receiver, cx, env);
            TExpr {
                ty: Type::Apply {
                    name: "Task".to_string(),
                    args: vec![carrier_ty],
                },
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::Spawn {
                        group: Some(Box::new(group)),
                        site,
                        label,
                        executable,
                    },
                },
            }
        });
    }
    if matches!(
        recv_type.as_deref(),
        Some(Syntax::TYPE_TASKGROUP) | Some(Syntax::INTERNAL_TASK_GROUP_SURFACE_TYPE)
    ) && matches!(
        method,
        Syntax::INTERNAL_TASK_ALL_METHOD
            | Syntax::INTERNAL_TASK_RACE_METHOD
            | Syntax::INTERNAL_TASK_ANY_METHOD
    ) {
        if args.len() != 1 {
            return invariant_method_expr(
                method_span,
                "task group operation without its checked task argument",
            );
        }
        return in_own_frame(|| {
            let Some(ty) = resolved_ret.cloned() else {
                return invariant_method_expr(
                    method_span,
                    "task group operation without a resolved return type",
                );
            };
            let tasks = if method == Syntax::INTERNAL_TASK_ALL_METHOD {
                lower_taskgroup_all_carrier(&args[0].expr, cx, env)
            } else {
                lower_expr(&args[0].expr, cx, env)
            };
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
                _ => {
                    return invariant_method_expr(method_span, "unknown task group operation");
                }
            };
            TExpr { ty, kind }
        });
    }
    // D-TXN3/D-TXN4: `<handle>.on_commit(() => { … })` on a `#Transact` handle.
    // The gate proved `recv_type == Some("Transaction")` and a single literal
    // zero-param lambda arg. Lower to `<handle>.on_commit(Box::new(move || { … }))`;
    // the Drop-backed LIFO-on-commit semantics live in the `JetTransaction` prelude
    // type. The receiver is the bound handle ident → its mangled Rust place.
    if method == Syntax::TXN_ON_COMMIT && recv_type.as_deref() == Some(Syntax::TXN_HANDLE_TYPE) {
        let Some(crate::AST::Expr::Lambda(lam)) = args.first().map(|a| &a.expr) else {
            return invariant_method_expr(
                method_span,
                "transaction on_commit without a checked lambda",
            );
        };
        return in_own_frame(|| {
            let Some(ty) = resolved_ret.cloned() else {
                return invariant_method_expr(
                    method_span,
                    "transaction on_commit without a resolved return type",
                );
            };
            let tl = lower_lambda(lam, cx, env);
            TExpr {
                ty,
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::OnCommit {
                        handle_name: match receiver {
                            Expr::Ident(name, _) => name.clone(),
                            _ => unreachable!("validated transaction handle binding"),
                        },
                        executable: Box::new(tl),
                    },
                },
            }
        });
    }
    // D-TXN-ROLLBACK (layer 3): `<handle>.on_rollback(() => { … })` on a `#Transact`
    // handle — the exact mirror of `on_commit`. Lower to
    // `<handle>.on_rollback(Box::new(move || { … }))`; the Drop-backed run-on-rollback
    // semantics live in the `JetTransaction` prelude type.
    if method == Syntax::TXN_ON_ROLLBACK && recv_type.as_deref() == Some(Syntax::TXN_HANDLE_TYPE) {
        let Some(crate::AST::Expr::Lambda(lam)) = args.first().map(|a| &a.expr) else {
            return invariant_method_expr(
                method_span,
                "transaction on_rollback without a checked lambda",
            );
        };
        return in_own_frame(|| {
            let Some(ty) = resolved_ret.cloned() else {
                return invariant_method_expr(
                    method_span,
                    "transaction on_rollback without a resolved return type",
                );
            };
            let tl = lower_lambda(lam, cx, env);
            TExpr {
                ty,
                kind: TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::OnRollback {
                        handle_name: match receiver {
                            Expr::Ident(name, _) => name.clone(),
                            _ => unreachable!("validated transaction handle binding"),
                        },
                        executable: Box::new(tl),
                    },
                },
            }
        });
    }
    // D-CAP2 (D-MEM1/S4): a user-written `.clone()` MethodCall no longer reaches
    // here — sema never constructs one (unrecognized method, E0102/E0311), and
    // the compiler's own duplication rewrites build `Expr::Copy` instead (its
    // own `lower_expr` arm), which lowers straight to `TExprKind::Clone`.
    // c109 Phase 23: `.raw()` on a distinct type → `({recv}).0`. The receiver's resolved
    // type names the distinct; its base type (from `cx.distinct_types`) is the total
    // result type. Mirrors `emit_method_call`'s `METHOD_DISTINCT_RAW` early return.
    if method == Syntax::METHOD_DISTINCT_RAW {
        return in_own_frame(|| {
            let recv = lower_expr(receiver, cx, env);
            let Type::Named(name) = &recv.ty else {
                return invariant_method_expr(
                    method_span,
                    "distinct raw without a named distinct receiver",
                );
            };
            let Some((_, _)) = cx.distinct_types.get(name) else {
                return invariant_method_expr(
                    method_span,
                    "distinct raw without its checked base type",
                );
            };
            let Some(ty) = resolved_ret.cloned() else {
                return invariant_method_expr(
                    method_span,
                    "distinct raw without a resolved return type",
                );
            };
            TExpr {
                ty,
                kind: TExprKind::DistinctRaw(Box::new(recv)),
            }
        });
    }
    // Calling a checked function field uses the same argument and failure
    // carrier boundary as every other function-value invocation.
    if let Some(fn_ty @ Type::Fn { .. }) = fn_field_call_ty(method, recv_type, cx) {
        return in_own_frame(|| {
            let recv = lower_expr(receiver, cx, env);
            let boxed = match &recv.ty {
                Type::Named(name) => cx.boxed_edges.contains(&(name.clone(), method.to_string())),
                _ => false,
            };
            let callee = TExpr {
                ty: fn_ty.clone(),
                kind: TExprKind::Field {
                    recv: Box::new(recv),
                    field: method.to_string(),
                    boxed,
                },
            };
            lower_fn_value_call(None, callee, args, method_span.start as u32, cx, env)
        });
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
            return in_own_frame(|| {
                let ty = resolved_ret
                    .cloned()
                    .unwrap_or_else(|| Type::Named(Syntax::TYPE_DATA.to_string()));
                let arg = args.first().map(|a| {
                    let mut value = lower_expr(&a.expr, cx, env);
                    if method == "Object" && matches!(&value.kind, TExprKind::MapLit(_)) {
                        value.ty = crate::Sema::core_json_pattern_types(method)
                            .and_then(|types| types.into_iter().next())
                            .expect("checked DataTree.Object payload type");
                    }
                    let value = if method == "Array" {
                        let payload =
                            Type::List(Box::new(Type::Named(Syntax::TYPE_DATA.to_string())));
                        preserve_typed_list_shape(value, &payload, cx)
                    } else {
                        value
                    };
                    Box::new((value, a.flags.implicit_clone))
                });
                TExpr {
                    ty,
                    kind: TExprKind::JSONLit {
                        variant: method.to_string(),
                        arg,
                    },
                }
            });
        }
    }
    // D-DBDRIVER1: a `DBValue` construction `DBValue.Int(n)` / `.Float(f)` /
    // `.Text(s)` / `.Bool(b)` / `.Blob(bytes)` (the gate proved the receiver
    // is `DBValue` and `method` a `DBValue` variant). Same shape as the `Data`
    // construction above.
    if let Expr::Ident(type_name, _) = receiver {
        if !env.locals.contains_key(type_name)
            && is_db_value_type_name(type_name)
            && is_db_value_variant(method)
        {
            return in_own_frame(|| {
                let Some(ty) = resolved_ret.cloned() else {
                    return invariant_method_expr(
                        method_span,
                        "DBValue constructor without a resolved return type",
                    );
                };
                let arg = args.first().map(|a| {
                    Box::new((lower_owned_expr(&a.expr, cx, env), a.flags.implicit_clone))
                });
                TExpr {
                    ty,
                    kind: TExprKind::DBValueLit {
                        variant: method.to_string(),
                        arg,
                    },
                }
            });
        }
    }
    if matches!(receiver, Expr::Ident(name, _) if name == Syntax::CLOCK_TYPE)
        && !env.locals.contains_key(Syntax::CLOCK_TYPE)
    {
        if method == "new" && args.len() == 1 {
            return in_own_frame(|| {
                let Some(ty) = resolved_ret.cloned() else {
                    return invariant_method_expr(
                        method_span,
                        "Clock.new without a resolved return type",
                    );
                };
                let seed = lower_expr(&args[0].expr, cx, env);
                TExpr {
                    ty,
                    kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::Helper {
                        helper: format!("{}jet_std_clock_new", cx.root_prefix),
                        kind: crate::Codegen::TIR::THelperKind::ClockNew,
                        args: vec![crate::Codegen::TIR::THostArg::Expr(seed)],
                    })),
                }
            });
        }
        if method == "system" && args.is_empty() {
            return in_own_frame(|| {
                let Some(ty) = resolved_ret.cloned() else {
                    return invariant_method_expr(
                        method_span,
                        "Clock.system without a resolved return type",
                    );
                };
                return TExpr {
                    ty,
                    kind: TExprKind::HostCall(Box::new(crate::Codegen::TIR::THostCall::Helper {
                        helper: format!("{}jet_std_clock_system", cx.root_prefix),
                        kind: crate::Codegen::TIR::THelperKind::ClockSystem,
                        args: vec![],
                    })),
                };
            });
        }
        if method == "now" && args.is_empty() {
            return in_own_frame(|| {
                let Some(ty) = resolved_ret.cloned() else {
                    return invariant_method_expr(
                        method_span,
                        "Clock.now without a resolved return type",
                    );
                };
                let record = match checked_core_record("core.time", "instant", 0, method_span) {
                    Ok(record) => record,
                    Err(expr) => return expr,
                };
                TExpr {
                    ty: ty.clone(),
                    kind: TExprKind::CoreCall {
                        record,
                        args: Vec::new(),
                        source_span: method_span,
                        type_args: Vec::new(),
                        widen_to_vec: Vec::new(),
                        data_plan: None,
                        fallibility: TFailureCarrier::from_checked_type(&ty),
                    },
                }
            });
        }
    }
    // D-SHAPE-DURATION1=A: a bare `Duration.unit(value)` is a type-owned
    // checked constructor, not an instance/static user method.
    {
        let locals: HashSet<String> = env.locals.keys().cloned().collect();
        if let Some(unit) = duration_new_unit(receiver, method, &locals) {
            return in_own_frame(|| {
                let Some(ty) = resolved_ret.cloned() else {
                    return invariant_method_expr(
                        method_span,
                        "Duration constructor without a resolved return type",
                    );
                };
                let value = lower_expr(&args[0].expr, cx, env);
                let float = matches!(value.ty, Type::Float | Type::Float32);
                TExpr {
                    ty,
                    kind: TExprKind::HandleMethod {
                        recv: Box::new(value),
                        op: THandleOp::DurationNew { unit, float },
                        args: vec![],
                    },
                }
            });
        }
    }
    // c109 Phase 19: carry the checked allocator constructor as a closed
    // target-neutral fact. Runtime allocation is lowered from this node by the
    // shared MIR adapter; no Rust constructor text is produced here.
    {
        if let Some(alloc_type) = allocator_constructor_owner(recv_type.as_deref(), method) {
            return in_own_frame(|| {
                let Some(ty) = resolved_ret.cloned() else {
                    return invariant_method_expr(
                        method_span,
                        "allocator constructor without a resolved return type",
                    );
                };
                let ctor = match (alloc_type, method) {
                    ("Arena", "new") => TAllocCtor::Arena,
                    ("Bump", "new") => TAllocCtor::Bump,
                    ("Pool", "new") => TAllocCtor::Pool,
                    ("Fixed", "new") => {
                        let Some(size) = args
                            .first()
                            .and_then(|arg| route_status(&arg.expr, cx))
                            .and_then(|value| usize::try_from(value).ok())
                            .filter(|value| *value > 0)
                        else {
                            // Sema owns the user diagnostic. Reaching this
                            // path means checked metadata was lost before TIR.
                            return invariant_method_expr(
                                method_span,
                                "validated Fixed.new is missing its positive comptime size",
                            );
                        };
                        TAllocCtor::Fixed { size }
                    }
                    ("Fixed", "over") => TAllocCtor::FixedOver,
                    _ => {
                        return invariant_method_expr(
                            method_span,
                            "allocator constructor route was not recognized after sema",
                        )
                    }
                };
                let mut ctor_args = args
                    .iter()
                    .map(|arg| lower_one_call_arg(arg, None, env, cx))
                    .collect::<Vec<_>>();
                if matches!(ctor, TAllocCtor::FixedOver) {
                    for arg in &mut ctor_args {
                        arg.borrow = false;
                        arg.mut_borrow = true;
                    }
                }
                TExpr {
                    ty,
                    kind: TExprKind::AllocNew {
                        ctor,
                        args: ctor_args,
                    },
                }
            });
        }
    }
    // D-SOLVER-LIB1=A: `solve.Solver.new(seed)` constructor. The receiver is a
    // core module sentinel (`solve.Solver`), so the seed arg becomes the lowered recv.
    {
        let locals: HashSet<String> = env.locals.keys().cloned().collect();
        if solve_new_type(receiver, method, cx, &locals).is_some() {
            return in_own_frame(|| {
                let Some(ty) = resolved_ret.cloned() else {
                    return invariant_method_expr(
                        method_span,
                        "Solver.new without a resolved return type",
                    );
                };
                let Some(seed_arg) = args.first() else {
                    return invariant_method_expr(
                        method_span,
                        "Solver.new without its checked seed argument",
                    );
                };
                let seed = lower_expr(&seed_arg.expr, cx, env);
                TExpr {
                    ty,
                    kind: TExprKind::HandleMethod {
                        recv: Box::new(seed),
                        op: THandleOp::SolverNew,
                        args: vec![],
                    },
                }
            });
        }
        if let Some(static_type) = game_static_type(receiver, method, cx, &locals) {
            return in_own_frame(|| {
                let op = match (static_type, method) {
                    ("Scene", "new") => THandleOp::GameSceneNew,
                    ("Replay", "record") => THandleOp::GameReplayRecord,
                    ("Backend", "headless") => THandleOp::GameBackendHeadless,
                    _ => {
                        return invariant_method_expr(
                            method_span,
                            "unknown game static constructor",
                        );
                    }
                };
                let ty = resolved_ret.cloned().or_else(|| match (static_type, method) {
                    ("Scene", "new") => Some(Type::Named("GameScene".to_string())),
                    ("Replay", "record") => Some(Type::Named("GameReplay".to_string())),
                    ("Backend", "headless") => Some(Type::Named("GameBackend".to_string())),
                    _ => None,
                });
                let Some(ty) = ty else {
                    return invariant_method_expr(
                        method_span,
                        "game static constructor without a resolved return type",
                    );
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
                TExpr {
                    ty,
                    kind: TExprKind::HandleMethod {
                        recv: Box::new(recv),
                        op,
                        args: rest,
                    },
                }
            });
        }
        if let Some(op) = tls_static_op(receiver, method, cx, &locals) {
            return in_own_frame(|| {
                let ty = resolved_ret.cloned().or_else(|| match &op {
                    THandleOp::TLSClientConfigDefault => {
                        Some(Type::Named("TLSClientConfig".to_string()))
                    }
                    THandleOp::TLSRootCertificatesFromPem => Some(Type::Result {
                        ok: Box::new(Type::Named("TLSRootCertificates".to_string())),
                        err: Box::new(Type::Named(Syntax::TYPE_IO_ERROR.to_string())),
                    }),
                    THandleOp::TLSClientIdentityFromPem => Some(Type::Result {
                        ok: Box::new(Type::Named("TLSClientIdentity".to_string())),
                        err: Box::new(Type::Named(Syntax::TYPE_IO_ERROR.to_string())),
                    }),
                    THandleOp::TLSClientConfigWithAlpn
                    | THandleOp::TLSClientConfigWithTrust
                    | THandleOp::TLSClientConfigWithIdentity
                    | THandleOp::TLSClientConfigWithVersionBounds => {
                        Some(Type::Named("TLSClientConfig".to_string()))
                    }
                    _ => match (method, args.len()) {
                        ("default", 0) => Some(Type::Named("TLSClientConfig".to_string())),
                        ("from_pem", 1) => Some(Type::Result {
                            ok: Box::new(Type::Named("TLSRootCertificates".to_string())),
                            err: Box::new(Type::Named(Syntax::TYPE_IO_ERROR.to_string())),
                        }),
                        ("from_pem", 2) => Some(Type::Result {
                            ok: Box::new(Type::Named("TLSClientIdentity".to_string())),
                            err: Box::new(Type::Named(Syntax::TYPE_IO_ERROR.to_string())),
                        }),
                        _ => None,
                    },
                });
                let Some(ty) = ty else {
                    return invariant_method_expr(
                        method_span,
                        "TLS constructor without a resolved return type",
                    );
                };
                TExpr {
                    ty,
                    kind: TExprKind::HandleMethod {
                        recv: Box::new(TExpr {
                            ty: unit_type(),
                            kind: TExprKind::Unit,
                        }),
                        op,
                        args: args
                            .iter()
                            .map(|arg| lower_expr(&arg.expr, cx, env))
                            .collect(),
                    },
                }
            });
        }
        if let Some(op) = http_client_static_op(receiver, method, cx, &locals) {
            return in_own_frame(|| {
                let Some(ty) = resolved_ret.cloned() else {
                    return invariant_method_expr(
                        method_span,
                        "HTTP client constructor without a resolved return type",
                    );
                };
                TExpr {
                    ty,
                    kind: TExprKind::HandleMethod {
                        recv: Box::new(TExpr {
                            ty: unit_type(),
                            kind: TExprKind::Unit,
                        }),
                        op,
                        args: Vec::new(),
                    },
                }
            });
        }
    }
    // (`emit_boxed_enum_arg` byte-for-byte). This is the construction half of the
    // string/struct/collection-payload + recursive (boxed) enum coverage.
    if recv_type.is_none() {
        if let Expr::Ident(type_name, _) = receiver {
            if !env.locals.contains_key(type_name) {
                // D-TERM1 (ratified 2026-06-22): `Key` is a prelude enum not in
                // `cx.enum_variants`; lower `Key.Variant(args)` to a TIR enum lit.
                let key_type = crate::Syntax::TYPE_KEY;
                if type_name == key_type && is_key_variant(method) {
                    return in_own_frame(|| {
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
                    });
                }
                if type_name == "DataEvent"
                    && matches!(method, "Bool" | "Int" | "Float" | "Text" | "Bytes" | "Key")
                {
                    return in_own_frame(|| {
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
                    });
                }
                let enum_owner = crate::Codegen::TIR::canonical_enum_owner(cx, type_name);
                if let Some(variants) = cx.enum_variants.get(&enum_owner) {
                    if variants.iter().any(|(v, _)| v == method) {
                        return in_own_frame(|| {
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
                            let ty = match env.ret_ty.as_ref() {
                                Some(Type::Apply { name, args })
                                    if name == type_name || name == &enum_owner =>
                                {
                                    Type::Apply {
                                        name: name.clone(),
                                        args: args.clone(),
                                    }
                                }
                                _ => Type::Named(type_name.clone()),
                            };
                            return TExpr {
                                ty,
                                kind: TExprKind::EnumLit {
                                    enum_type: type_name.clone(),
                                    variant: method.to_string(),
                                    payload,
                                },
                            };
                        });
                    }
                }
            }
        }
    }
    // D-ENV-MUTATE1=A: current editions retain `env.set -> ()`, but invalid
    // runtime strings must produce existing E3001 at the Jet call span. Lower
    // this compatibility wrapper with all panic facts resolved before emit.
    if method == "set" && args.len() == 2 {
        if let Expr::Ident(alias, _) = receiver {
            if !env.locals.contains_key(alias)
                && cx
                    .core_import_module_for_function(&env.fn_name, alias)
                    .is_some_and(|module| module == "core.sys")
            {
                return in_own_frame(|| {
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
                });
            }
        }
    }

    // D-SQL-SURFACE1=C / D-QUERY-RETAIN1=A: `[T].query(SQL)`, `Query<T>` and
    // grouped-query receiver methods are projections onto the receiver Core
    // rows (`core_calls::lower_query_receiver_call`).
    if recv_type.is_none()
        || matches!(
            recv_type.as_deref(),
            Some("Query" | "DataGroupedQuery" | "DataTracked" | "DataWatch")
        )
    {
        if let Some(expr) = in_own_frame(|| {
            super::core_calls::lower_query_receiver_call(
                receiver,
                method,
                method_span,
                args,
                recv_type,
                resolved_ret,
                cx,
                env,
                lowered_receiver.borrow().as_ref(),
            )
        }) {
            return expr;
        }
    }

    // c109 Phase 10: a core/stdlib module call `alias.method(args)`.
    // Mirror TIR core-call emission: resolve the module here (total), lower args
    // PLAINLY (no clone/borrow wrappers —
    // `arg(i)` is a raw `emit_expr`), and carry the return type from the authoritative
    // `core_fixed_sig` table. Tried BEFORE the builtin shape (a core method named
    // `get`/`split`/… must not be claimed by the receiver-keyed builtin op).
    //
    // #777 / MirBridge: prefer the core-import alias even when `recv_type` is Some —
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
                return in_own_frame(|| {
                    let method = core_method.as_str();
                    if module == "core.devtools" && method == "publish" {
                        return lower_devtools_publish(method_span, args, resolved_ret, cx, env);
                    }
                    if module == "core.archive" {
                        if let Some(source_call) = lower_archive_source_call(
                            method_span,
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
                    // D-VERDICT-1321-1: `core.term.print` is the qualified
                    // twin of ambient print. Emit one typed Print per argument
                    // so Printable values do not acquire an implicit Display.
                    if module == "core.term" && method == "print" {
                        return crate::Codegen::TIR::lower::print_values(
                            args.iter()
                                .map(|arg| crate::Codegen::TIR::lower_expr(&arg.expr, cx, env)),
                            cx,
                            method_span.start,
                        );
                    }
                    // `core.term.eprint` keeps its stderr CoreCall adapter;
                    // preserve its existing newline join until that adapter
                    // grows the same typed Print operation.
                    if module == "core.term" && method == "eprint" && !args.is_empty() {
                        let joined = crate::Codegen::TIR::lower::join_print_args(args, cx, env);
                        let record =
                            match checked_core_record(module.as_str(), method, 1, method_span) {
                                Ok(record) => record,
                                Err(expr) => return expr,
                            };
                        let ty = resolved_ret.cloned().unwrap_or_else(unit_type);
                        return TExpr {
                            ty: ty.clone(),
                            kind: TExprKind::CoreCall {
                                record,
                                args: vec![joined],
                                source_span: method_span,
                                type_args: Vec::new(),
                                widen_to_vec: vec![false],
                                data_plan: None,
                                fallibility: TFailureCarrier::from_checked_type(&ty),
                            },
                        };
                    }
                    if module == "core.ui" && method == "mount" {
                        if !(2..=3).contains(&args.len()) {
                            return invariant_method_expr(
                                method_span,
                                "checked Core call `core.ui.mount` has the wrong arity",
                            );
                        }
                        let receiver = lower_expr(&args[0].expr, cx, env);
                        let call_args = args[1..]
                            .iter()
                            .map(|arg| lower_expr(&arg.expr, cx, env))
                            .collect::<Vec<_>>();
                        let route_method = if call_args.len() == 1 {
                            "mount_default"
                        } else {
                            "mount"
                        };
                        return TExpr {
                            ty: resolved_ret.cloned().unwrap_or_else(unit_type),
                            kind: TExprKind::HandleMethod {
                                recv: Box::new(receiver),
                                op: THandleOp::UiBackendMethod {
                                    method: route_method.to_string(),
                                },
                                args: call_args,
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
                    if module == "core.math"
                        && matches!(method, "is_nan" | "is_inf" | "is_finite")
                    {
                        return lower_core_math_predicate(
                            method,
                            args,
                            resolved_ret,
                            method_span,
                            cx,
                            env,
                        );
                    }
                    if module == "core.math" && method == "abs" {
                        return lower_core_math_abs(args, resolved_ret, method_span, cx, env);
                    }
                    if module == "core.math"
                        && method == "sqrt"
                        && args.len() == 1
                        && matches!(
                            resolved_ret,
                            Some(Type::Apply { name, args })
                                if name == Syntax::TYPE_MEASUREMENT
                                    && args == &[Type::Float]
                        )
                    {
                        return in_own_frame(|| {
                            let recv = lower_expr(&args[0].expr, cx, env);
                            return TExpr {
                                ty: recv.ty.clone(),
                                kind: TExprKind::HandleMethod {
                                    recv: Box::new(recv),
                                    op: THandleOp::MeasurementMethod {
                                        method: "sqrt".to_string(),
                                    },
                                    args: Vec::new(),
                                },
                            };
                        });
                    }
                    // D-PIN1=A: `mem.pin(&place)` IS the exclusive borrow of
                    // `place`. Sema proved the no-move contract before lowering
                    // (I3), so every tier emits exactly what `&place` emits and
                    // none of them re-encode the promise (I9).
                    if module == Syntax::CORE_MEM_MODULE && method == Syntax::MEM_PIN {
                        if let Some(arg) = args.first() {
                            return in_own_frame(|| {
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
                                    return TExpr {
                                        ty,
                                        kind: lowered.kind,
                                    };
                                }
                                return TExpr {
                                    ty,
                                    kind: TExprKind::Borrow {
                                        place: Box::new(lowered),
                                        mutable: true,
                                    },
                                };
                            });
                        }
                    }
                    if let Some(t) =
                        lower_core_closure_call(&module, method, method_span, args, cx, env)
                    {
                        return t;
                    }
                    if module == "core.mem" && method == "address_of" {
                        if args.len() != 1 {
                            return invariant_method_expr(
                                method_span,
                                "checked Core call `core.mem.address_of` has the wrong arity",
                            );
                        }
                        let value = lower_expr(&args[0].expr, cx, env);
                        if matches!(
                            crate::Codegen::TIR::tir_address_lifetime(&value),
                            crate::Codegen::TIR::TAddressLifetime::Stack
                        ) {
                            env.note_stack_address();
                        }
                        return TExpr {
                            ty: resolved_ret.cloned().unwrap_or(Type::Int),
                            kind: TExprKind::RawOf(Box::new(value)),
                        };
                    }
                    let mut targs: Vec<TExpr> = args
                        .iter()
                        .enumerate()
                        .map(|(index, arg)| {
                            lower_core_arg(&module, method, index, &arg.expr, cx, env)
                        })
                        .collect();
                    if module == "core.http.server" && method == "cors_policy" {
                        normalize_http_cors_policy_args(&mut targs);
                    }
                    if module == "core.mem"
                        && method == "address_of"
                        && targs.first().is_some_and(|arg| {
                            matches!(
                                crate::Codegen::TIR::tir_address_lifetime(arg),
                                crate::Codegen::TIR::TAddressLifetime::Stack
                            )
                        })
                    {
                        env.note_stack_address();
                    }
                    let widen_to_vec = core_widen_to_vec(&module, method, &targs);
                    let ty = match resolved_ret.cloned().or_else(|| {
                        crate::Sema::core_call_semantic_signature(&module, method)
                            .map(|(_, ret)| ret)
                    }) {
                        Some(ty) => ty,
                        None if module == "core.term" && method == "eprint" => unit_type(),
                        None => {
                            return invariant_method_expr(
                                method_span,
                                format!("Core call `{module}.{method}` has no resolved return type"),
                            )
                        }
                    };
                    let record =
                        match checked_core_record(&module, method, targs.len(), method_span) {
                            Ok(record) => record,
                            Err(expr) => return expr,
                        };
                    prepare_generic_serde_codec(cx, &env.fn_name, &module, method, &mut targs, &ty);
                    let data_plan = match data_plan_for_core_call(record, &targs, &ty, method_span)
                    {
                        Ok(plan) => plan,
                        Err(error) => return invariant_method_expr(method_span, error),
                    };
                    TExpr {
                        ty: ty.clone(),
                        kind: TExprKind::CoreCall {
                            record,
                            type_args: checked_core_type_args(&module, method, type_args, &targs),
                            args: targs,
                            source_span: method_span,
                            widen_to_vec,
                            data_plan,
                            fallibility: TFailureCarrier::from_checked_type(&ty),
                        },
                    }
                });
            }
        }
    }
    if recv_type.is_none() {
        if matches!(receiver, Expr::Field(..)) {
            if let Some(submodule) = core_module_path_from_receiver(receiver, cx, env) {
                return in_own_frame(|| {
                    if submodule == "core.devtools" && method == "publish" {
                        return lower_devtools_publish(method_span, args, resolved_ret, cx, env);
                    }
                    if submodule == "core.archive" {
                        if let Some(source_call) = lower_archive_source_call(
                            method_span,
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
                    if submodule == "core.mem" && method == "address_of" {
                        if args.len() != 1 {
                            return invariant_method_expr(
                                method_span,
                                "checked Core call `core.mem.address_of` has the wrong arity",
                            );
                        }
                        let value = lower_expr(&args[0].expr, cx, env);
                        if matches!(
                            crate::Codegen::TIR::tir_address_lifetime(&value),
                            crate::Codegen::TIR::TAddressLifetime::Stack
                        ) {
                            env.note_stack_address();
                        }
                        return TExpr {
                            ty: resolved_ret.cloned().unwrap_or(Type::Int),
                            kind: TExprKind::RawOf(Box::new(value)),
                        };
                    }
                    if submodule == "core.math"
                        && matches!(method, "is_nan" | "is_inf" | "is_finite")
                    {
                        return lower_core_math_predicate(
                            method,
                            args,
                            resolved_ret,
                            method_span,
                            cx,
                            env,
                        );
                    }
                    if submodule == "core.math" && method == "abs" {
                        return lower_core_math_abs(args, resolved_ret, method_span, cx, env);
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
                    let mut targs: Vec<TExpr> = args
                        .iter()
                        .enumerate()
                        .map(|(index, arg)| {
                            lower_core_arg(&submodule, method, index, &arg.expr, cx, env)
                        })
                        .collect();
                    if submodule == "core.http.server" && method == "cors_policy" {
                        normalize_http_cors_policy_args(&mut targs);
                    }
                    if submodule == "core.mem"
                        && method == "address_of"
                        && targs.first().is_some_and(|arg| {
                            matches!(
                                crate::Codegen::TIR::tir_address_lifetime(arg),
                                crate::Codegen::TIR::TAddressLifetime::Stack
                            )
                        })
                    {
                        env.note_stack_address();
                    }
                    let widen_to_vec = core_widen_to_vec(&submodule, method, &targs);
                    let Some(ty) = resolved_ret.cloned().or_else(|| {
                        crate::Sema::core_call_semantic_signature(&submodule, method)
                            .map(|(_, ret)| ret)
                    }) else {
                        return invariant_method_expr(
                            method_span,
                            format!("Core call `{submodule}.{method}` has no resolved return type",),
                        );
                    };
                    let record =
                        match checked_core_record(&submodule, method, targs.len(), method_span) {
                            Ok(record) => record,
                            Err(expr) => return expr,
                        };
                    prepare_generic_serde_codec(
                        cx,
                        &env.fn_name,
                        &submodule,
                        method,
                        &mut targs,
                        &ty,
                    );
                    let data_plan = match data_plan_for_core_call(record, &targs, &ty, method_span)
                    {
                        Ok(plan) => plan,
                        Err(error) => return invariant_method_expr(method_span, error),
                    };
                    TExpr {
                        ty: ty.clone(),
                        kind: TExprKind::CoreCall {
                            record,
                            type_args: checked_core_type_args(
                                &submodule, method, type_args, &targs,
                            ),
                            args: targs,
                            source_span: method_span,
                            widen_to_vec,
                            data_plan,
                            fallibility: TFailureCarrier::from_checked_type(&ty),
                        },
                    }
                });
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
                        return in_own_frame(|| {
                            let sig = cx
                                .inline_foreign_reexport_sigs
                                .get(&(owner.clone(), leaf.clone(), method.to_string()))
                                .cloned()
                                .or_else(|| {
                                    cx.import_signature_for_function(&env.fn_name, leaf, method)
                                });
                            let declared_ret = cx
                                .inline_foreign_reexport_rets
                                .get(&(owner.clone(), leaf.clone(), method.to_string()))
                                .cloned()
                                .or_else(|| {
                                    cx.import_return_for_function(&env.fn_name, leaf, method)
                                })
                                .flatten();
                            let target_return = match declared_ret.as_ref() {
                                Some(declared) => match module_call_target_return(cx, Some(declared)) {
                                    Ok(target_return) => Some(target_return),
                                    Err(error) => {
                                        return invariant_method_expr(method_span, error);
                                    }
                                },
                                None => None,
                            };
                            let ret = declared_ret.clone().unwrap_or_else(unit_type);
                            // C foreign namespaces mounted through an inline module
                            // still have a generated wrapper. Keep this branch on the
                            // same ExternCall path as a direct alias call so #Undo is
                            // registered before the wrapper runs. Cached JS/other
                            // foreign modules have no wrapper and remain ModuleCall
                            // values below.
                            let wrapper_key = format!("{rust_mod}::{method}");
                            if let Some(extern_fn) = cx.extern_funcs.get(&wrapper_key).cloned() {
                                let wrapper = extern_fn.wrapper;
                                let c_abi = extern_fn.c_abi;
                                return in_own_frame(|| {
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
                                        ty: imported_extern_call_type(
                                            c_abi,
                                            declared_ret.clone().unwrap_or_else(unit_type),
                                        ),
                                        kind: TExprKind::ExternCall {
                                            symbol: wrapper,
                                            c_abi,
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
                                });
                            }
                            let undo = cx.foreign_undos.get(&wrapper_key).map(String::as_str);
                            let targs = lower_module_args(args, sig.as_deref(), env, cx);
                            let lowered = TExpr {
                                ty: ret,
                                kind: TExprKind::ModuleCall {
                                    form: TModuleCallForm::Qualified {
                                        rust_mod,
                                        rust_fn: mangle(method).to_string(),
                                    },
                                    target_return,
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
                        });
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
                    return in_own_frame(|| {
                        let sig = cx.sigs.get(&mangled_key).cloned();
                        let targs = lower_module_args(args, sig.as_deref(), env, cx);
                        let target_return =
                            call_return_type_with_args(cx, &mangled_key, type_args, &targs);
                        let ret = match module_call_source_return_type(
                            cx,
                            &mangled_key,
                            resolved_ret,
                        ) {
                            Ok(ret) => ret,
                            Err(error) => return invariant_method_expr(method_span, error),
                        };
                        return TExpr {
                            ty: ret,
                            kind: TExprKind::ModuleCall {
                                form: TModuleCallForm::InlineMangled {
                                    mangled: crate::Codegen::TIR::demand_generic_free_function(
                                        cx,
                                        &mangled_key,
                                        &targs,
                                        type_args,
                                    )
                                    .unwrap_or(mangled_key),
                                },
                                target_return: Some(target_return),
                                type_args: type_args.to_vec(),
                                args: targs,
                            },
                        };
                    });
                }
                if let Some((real_mod, real_fn)) = cx
                    .reexport_calls
                    .get(&(alias.clone(), method.to_string()))
                    .cloned()
                {
                    return in_own_frame(|| {
                        let undo = cx
                            .foreign_undos
                            .get(&format!("{real_mod}::{real_fn}"))
                            .map(String::as_str);
                        let sig = cx
                            .import_sigs
                            .get(&(alias.clone(), method.to_string()))
                            .cloned();
                        let targs = lower_module_args(args, sig.as_deref(), env, cx);
                        let target_return = match imported_module_call_target_return(
                            cx,
                            alias,
                            method,
                            resolved_ret,
                        ) {
                            Ok(target_return) => target_return,
                            Err(error) => return invariant_method_expr(method_span, error),
                        };
                        let ret = match module_call_source_return_type(
                            cx,
                            &format!("{alias}.{method}"),
                            resolved_ret,
                        ) {
                            Ok(ret) => ret,
                            Err(error) => return invariant_method_expr(method_span, error),
                        };
                        let lowered = TExpr {
                            ty: ret,
                            kind: TExprKind::ModuleCall {
                                form: TModuleCallForm::Qualified {
                                    rust_mod: real_mod,
                                    rust_fn: mangle(&real_fn).to_string(),
                                },
                                target_return,
                                type_args: type_args.to_vec(),
                                args: targs,
                            },
                        };
                        return wrap_foreign_undo(lowered, undo, method_span.start as u32, cx, env);
                    });
                }
                if let Some(mod_name) = cx.import_mods.get(alias).cloned() {
                    return in_own_frame(|| {
                        let undo = cx
                            .foreign_undos
                            .get(&format!("{mod_name}::{method}"))
                            .map(String::as_str);
                        let sig = cx
                            .import_sigs
                            .get(&(alias.clone(), method.to_string()))
                            .cloned();
                        if let Some(extern_fn) = cx
                            .extern_funcs
                            .get(&format!("{mod_name}::{method}"))
                            .cloned()
                        {
                            let wrapper = extern_fn.wrapper;
                            let c_abi = extern_fn.c_abi;
                            return in_own_frame(|| {
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
                                let ty = imported_extern_call_type(
                                    c_abi,
                                    cx.import_rets
                                        .get(&(alias.clone(), method.to_string()))
                                        .cloned()
                                        .flatten()
                                        .unwrap_or_else(unit_type),
                                );
                                let lowered = TExpr {
                                    ty,
                                    kind: TExprKind::ExternCall {
                                        symbol: wrapper,
                                        c_abi,
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
                            });
                        }
                        let targs = lower_module_args(args, sig.as_deref(), env, cx);
                        let target_return = match imported_module_call_target_return(
                            cx,
                            alias,
                            method,
                            resolved_ret,
                        ) {
                            Ok(target_return) => target_return,
                            Err(error) => return invariant_method_expr(method_span, error),
                        };
                        let ret = match module_call_source_return_type(
                            cx,
                            &format!("{alias}.{method}"),
                            resolved_ret,
                        ) {
                            Ok(ret) => ret,
                            Err(error) => return invariant_method_expr(method_span, error),
                        };
                        let lowered = TExpr {
                            ty: ret,
                            kind: TExprKind::ModuleCall {
                                form: TModuleCallForm::Qualified {
                                    rust_mod: mod_name,
                                    rust_fn: mangle(method).to_string(),
                                },
                                target_return,
                                type_args: type_args.to_vec(),
                                args: targs,
                            },
                        };
                        return wrap_foreign_undo(lowered, undo, method_span.start as u32, cx, env);
                    });
                }
                if let Some(mod_name) = cx
                    .import_module_for_function(&env.fn_name, alias)
                    .map(str::to_owned)
                {
                    return in_own_frame(|| {
                        let undo = cx
                            .foreign_undos
                            .get(&format!("{mod_name}::{method}"))
                            .map(String::as_str);
                        let sig = cx.import_signature_for_function(&env.fn_name, alias, method);
                        if let Some(extern_fn) = cx
                            .extern_funcs
                            .get(&format!("{mod_name}::{method}"))
                            .cloned()
                        {
                            let wrapper = extern_fn.wrapper;
                            let c_abi = extern_fn.c_abi;
                            return in_own_frame(|| {
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
                                let ty = imported_extern_call_type(
                                    c_abi,
                                    cx.import_return_for_function(&env.fn_name, alias, method)
                                        .flatten()
                                        .unwrap_or_else(unit_type),
                                );
                                let lowered = TExpr {
                                    ty,
                                    kind: TExprKind::ExternCall {
                                        symbol: wrapper,
                                        c_abi,
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
                            });
                        }
                        let targs = lower_module_args(args, sig.as_deref(), env, cx);
                        let declared_ret =
                            cx.import_return_for_function(&env.fn_name, alias, method);
                        let direct_c_function = cx
                            .direct_c_functions
                            .contains(&format!("{mod_name}::{method}"));
                        let target_return = if direct_c_function {
                            declared_ret.clone().map(|declared| declared.unwrap_or_else(unit_type))
                        } else {
                            match module_call_target_return(cx, resolved_ret) {
                                Ok(target_return) => Some(target_return),
                                Err(error) => return invariant_method_expr(method_span, error),
                            }
                        };
                        let ret = match module_call_source_return_type(
                            cx,
                            &format!("{alias}.{method}"),
                            resolved_ret,
                        ) {
                            Ok(ret) => ret,
                            Err(error) => return invariant_method_expr(method_span, error),
                        };
                        let lowered = TExpr {
                            ty: ret,
                            kind: TExprKind::ModuleCall {
                                form: TModuleCallForm::Qualified {
                                    rust_mod: mod_name,
                                    rust_fn: mangle(method).to_string(),
                                },
                                target_return,
                                type_args: type_args.to_vec(),
                                args: targs,
                            },
                        };
                        return wrap_foreign_undo(lowered, undo, method_span.start as u32, cx, env);
                    });
                }
                if cx.code_modules.contains(alias.as_str()) {
                    return in_own_frame(|| {
                        let mangled_key = jet_foundation::Names::member_name(alias, method);
                        let undo = cx
                            .foreign_undos
                            .get(&format!("{alias}::{method}"))
                            .map(String::as_str);
                        let sig = cx.sigs.get(&mangled_key).cloned();
                        let targs = lower_module_args(args, sig.as_deref(), env, cx);
                        let target_return =
                            call_return_type_with_args(cx, &mangled_key, type_args, &targs);
                        // Inline-module signatures carry the instance's lifted
                        // nominal identity in `target_return`, but the expression
                        // keeps sema's source-facing return so MIR can consume the
                        // hidden Result carrier exactly once.
                        let ret = match module_call_source_return_type(
                            cx,
                            &mangled_key,
                            resolved_ret,
                        ) {
                            Ok(ret) => ret,
                            Err(error) => return invariant_method_expr(method_span, error),
                        };
                        let lowered = TExpr {
                            ty: ret,
                            kind: TExprKind::ModuleCall {
                                form: TModuleCallForm::InlineMangled {
                                    mangled: crate::Codegen::TIR::demand_generic_free_function(
                                        cx,
                                        &mangled_key,
                                        &targs,
                                        type_args,
                                    )
                                    .unwrap_or(mangled_key),
                                },
                                target_return: Some(target_return),
                                type_args: type_args.to_vec(),
                                args: targs,
                            },
                        };
                        return wrap_foreign_undo(lowered, undo, method_span.start as u32, cx, env);
                    });
                }
            }
        }
    }
    // D-PLACE1: Atomic<T> methods use the same BuiltinMethod MIR seam as
    // collection builtins, but their route is a closed, symbol-keyed adapter
    // family rather than a Rust method call.
    if recv_type.as_deref() == Some("Atomic") {
        return in_own_frame(|| {
            let recv_t = lowered_receiver.borrow_mut().take();
            let recv_t = recv_t.unwrap_or_else(|| lower_expr(receiver, cx, env));
            let op = TBuiltinOp::AtomicMethod {
                method: method.to_string(),
            };
            let expected_arg_types =
                crate::Collections::builtin_method_arg_types(&recv_t.ty, method);
            let targs = args
                .iter()
                .enumerate()
                .map(|(index, arg)| {
                    let expected = expected_arg_types
                        .as_ref()
                        .and_then(|types| types.get(index));
                    lower_builtin_arg(
                        arg,
                        expected,
                        builtin_arg_takes_ownership(&op, index),
                        cx,
                        env,
                    )
                })
                .collect();
            TExpr {
                ty: resolved_ret.cloned().unwrap_or_else(unit_type),
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(recv_t),
                    op,
                    args: targs,
                },
            }
        });
    }
    // c109 Phase 9: a built-in collection/string method (`emit_builtin_method`). The
    // gate proved `recv_type == None` + a covered builtin name + an in-subset value
    // receiver. Resolve the Map-vs-List-vs-String emit branch HERE from the
    // receiver's type (reproducing `expr_jet_ty`, incl. its `None` partiality), so
    // emit makes no type decision (I3). The result type comes from the builtin's
    // sema return (`Collections::builtin_method_return`) for totality.
    if recv_type.is_none()
        // Parenthesized/fallible String receivers retain their nominal sema
        // name. They are still the same builtin String surface; keep `.len()`
        // and its siblings on the shared Prelude path instead of Rust's byte
        // length method.
        || matches!(recv_type.as_deref(), Some("String" | "Option" | "SQL"))
        || matches!(
            recv_type.as_deref(),
            Some("Set") | Some(crate::Syntax::TYPE_RANK)
        )
        || recv_type.as_deref().is_some_and(|name| {
            let leaf = name.rsplit('.').next().unwrap_or(name);
            matches!(
                leaf,
                crate::Syntax::TYPE_BYTES | crate::Syntax::TYPE_LIST | "Equatable" | "FixedList"
            ) || leaf.starts_with('[')
        })
        || (matches!(method, "equal" | "compare") && args.len() == 1)
    {
        let mut builtin_receiver_ty = tir_recv_jet_ty(receiver, env);
        if builtin_receiver_ty.is_none() {
            let lowered_receiver_ty = {
                let lowered_receiver = lowered_receiver.borrow();
                lowered_receiver.as_ref().map(|lowered| lowered.ty.clone())
            };
            if let Some(ty) = lowered_receiver_ty {
                builtin_receiver_ty = Some(ty);
            } else {
                let lowered = lower_expr(receiver, cx, env);
                builtin_receiver_ty = Some(lowered.ty.clone());
                *lowered_receiver.borrow_mut() = Some(lowered);
            }
        }
        if let Some(op) = resolve_builtin_op(
            receiver,
            method,
            method_span,
            args,
            resolved_ret,
            builtin_receiver_ty.as_ref(),
            env,
            cx,
        ) {
            return in_own_frame(|| {
                // D-MEM1 S6: a mutating builtin (`.push()` etc.) on an indexed
                // place needs the same genuine-mutable-place treatment as
                // `LValue::Field`/`LValue::Index`. The resolved op carries this
                // fact even when the AST receiver type is an index or field, so
                // the ordinary value-clone path cannot discard the mutation.
                let recv_ast_ty = builtin_receiver_ty.clone();
                let recv_mut_ty_hint = recv_ast_ty
                    .clone()
                    .or_else(|| pool_field_ty_hint(receiver, cx, env));
                let needs_mut = op.needs_mut_receiver_place()
                    || recv_mut_ty_hint.as_ref().is_some_and(|ty| {
                        crate::Collections::builtin_needs_mut_receiver(ty, method)
                    });
                let recv_t = if needs_mut {
                    lower_expr_as_mut_place(receiver, cx, env)
                } else {
                    let recv_t = lowered_receiver.borrow_mut().take();
                    recv_t.unwrap_or_else(|| lower_expr(receiver, cx, env))
                };
                // #1478: Set.min() reuses the generic List reducer — route
                // through the same `.to_list()` a user would write so AOT and
                // JIT never see a raw `HashSet` where they expect a `Vec`.
                let recv_t = if matches!(op, TBuiltinOp::Min { .. }) {
                    crate::Codegen::TIR::wrap_set_receiver_as_list(recv_t, method_span)
                } else {
                    recv_t
                };
                // D-ITERTOOLS1=A: `tir_recv_jet_ty` is None for list literals and
                // fallible String call receivers, so a chain like
                // `[…].flatten().to_list()` or `text()?.split(",")` can
                // mis-resolve its builtin from the partial AST type. Prefer the
                // lowered receiver type.
                let op = match (&op, method) {
                    (TBuiltinOp::IterSplit { .. }, "split")
                        if matches!(&recv_t.ty, Type::String) =>
                    {
                        TBuiltinOp::Split
                    }
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
                // An rvalue String owns its buffer at this call boundary. Carry
                // that fact to AOT so `bytes()` can consume it without copying;
                // local/field/index places and tracked string views keep Read.
                let op = match op {
                    TBuiltinOp::Bytes { .. } if string_bytes_receiver_is_owned(&recv_t, cx) => {
                        TBuiltinOp::Bytes { owned: true }
                    }
                    op => op,
                };
                let Some(result_ty) = resolved_ret.cloned().or_else(|| {
                    crate::Collections::builtin_method_return(&recv_t.ty, method, args.len(), false)
                        .map(|ty| ty.unwrap_or_else(unit_type))
                }) else {
                    return invariant_method_expr(
                        method_span,
                        "builtin method without a resolved return type",
                    );
                };
                let result_ty = result_ty.with_effective_fn_returns();
                if matches!(
                    op,
                    TBuiltinOp::ListCopy | TBuiltinOp::MapCopy | TBuiltinOp::SetCopy
                ) {
                    return TExpr {
                        ty: result_ty,
                        kind: TExprKind::ExplicitCopy(Box::new(recv_t)),
                    };
                }
                // Reuse sema's canonical builtin signature so contextual empty
                // collections retain their element type. Builtins store values
                // as plain Rust arguments, so `lower_builtin_arg` also carries
                // the implicit-clone/read-by-value boundary ordinary `TCallArg`
                // emission normally owns.
                let expected_arg_types =
                    crate::Collections::builtin_method_arg_types(&recv_t.ty, method);
                let mut targs: Vec<TExpr> = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        let expected = expected_arg_types.as_ref().and_then(|types| types.get(i));
                        lower_builtin_arg(a, expected, builtin_arg_takes_ownership(&op, i), cx, env)
                    })
                    .collect();
                let recv_t = if matches!(
                    op,
                    TBuiltinOp::StepBy
                        | TBuiltinOp::Take
                        | TBuiltinOp::Skip
                        | TBuiltinOp::Dedup
                        | TBuiltinOp::Chunks
                        | TBuiltinOp::Windows
                        | TBuiltinOp::Intersperse
                        | TBuiltinOp::IterRepeat
                        | TBuiltinOp::IterCycle
                        | TBuiltinOp::IterDropLast
                        | TBuiltinOp::IterShuffle
                        | TBuiltinOp::IterIsSorted
                        | TBuiltinOp::IterLastIndexOf
                        | TBuiltinOp::IterAverage { .. }
                        | TBuiltinOp::IterCompare
                ) {
                    if matches!(op, TBuiltinOp::IterCompare) {
                        targs = targs.into_iter().map(wrap_list_as_iter).collect();
                    }
                    wrap_list_as_iter(recv_t)
                } else {
                    recv_t
                };
                return TExpr {
                    ty: result_ty,
                    kind: TExprKind::BuiltinMethod {
                        recv: Box::new(recv_t),
                        op,
                        args: targs,
                    },
                };
            });
        }
    }
    // c109 Phase 19: `Stopwatch.elapsed_millis()` (gate shape d2). The gate proved
    // `recv_type == None` + the `elapsed_millis` name + an in-subset value receiver.
    // Lower to the existing `THandleOp::StopwatchElapsedMillis` (`{root}jet_stopwatch_
    // elapsed_millis(&(recv))`), the same node the Phase-13 handle shape uses — emit is
    // byte-identical to `emit_builtin_method`'s name-keyed `elapsed_millis` arm. The
    // result type is `Int` (`stopwatch_method_return`), kept total per the design.
    if recv_type.is_none() && method == "elapsed_millis" && args.is_empty() {
        return in_own_frame(|| {
            let recv_t = lower_expr(receiver, cx, env);
            return TExpr {
                ty: Type::Int,
                kind: TExprKind::HandleMethod {
                    recv: Box::new(recv_t),
                    op: THandleOp::StopwatchElapsedMillis,
                    args: Vec::new(),
                },
            };
        });
    }
    // c109 Phase 24: `Match.group(n)` (gate shape d4). The gate proved `recv_type ==
    // Some("Match")` + `group`/1 + an in-subset value receiver. Lower to `BuiltinMethod`/
    // `MatchGroup`, byte-for-byte `emit_builtin_method`'s `("Match", "group")` arm. The
    // result type is `String?`. Placed BEFORE the user-instance shape (also `recv_type ==
    // Some`) — `Match` is never a covered user struct/enum, so the two never collide.
    if recv_type.as_deref() == Some("Match") && method == "group" && args.len() == 1 {
        return in_own_frame(|| {
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
        });
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
        return in_own_frame(|| {
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
        });
    }
    if recv_type.as_deref() == Some(crate::Syntax::TYPE_EFFECT)
        && is_reactive_effect_method_name(method, args.len())
    {
        return in_own_frame(|| {
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
        });
    }
    if recv_type.as_deref() == Some("FfiCallbackEvent") && method == "stop" && args.is_empty() {
        return in_own_frame(|| {
            let recv_t = lower_expr(receiver, cx, env);
            TExpr {
                ty: unit_type(),
                kind: TExprKind::HandleMethod {
                    recv: Box::new(recv_t),
                    op: THandleOp::FfiCallbackEventStop,
                    args: Vec::new(),
                },
            }
        });
    }
    // D-EVENT1=D: Event/Hook/Subscription/EventScope/EventTrace methods.
    if is_event_handle_type(recv_type.as_deref()) && is_event_method_name(method, args.len()) {
        return in_own_frame(|| {
            let recv_t = lower_expr(receiver, cx, env);
            let unit = unit_type();
            let result_ty = in_own_frame(|| match (recv_type.as_deref(), method) {
                (Some("Event"), "emit") => Type::Named("EventTrace".to_string()),
                (Some("AsyncEvent"), "emit_async") => match &recv_t.ty {
                    Type::Apply { args, .. } if args.len() >= 2 => Type::Apply {
                        name: "Task".to_string(),
                        args: vec![Type::Apply {
                            name: "DispatchReport".to_string(),
                            args: vec![args[1].clone()],
                        }],
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
            });
            let expected_payload = match &recv_t.ty {
                Type::Apply { args, .. } => args.first().cloned(),
                _ => None,
            };
            let expected_hook_result = match &recv_t.ty {
                Type::Apply { name, args } if name == "AsyncEvent" && args.len() >= 2 => {
                    Some(Type::Result {
                        ok: Box::new(Type::Named("Unit".to_string())),
                        err: Box::new(args[1].clone()),
                    })
                }
                Type::Apply { name, args } if name == "DecisionHook" && args.len() >= 2 => {
                    Some(Type::Apply {
                        name: "HookDecision".to_string(),
                        args: vec![args[0].clone(), args[1].clone()],
                    })
                }
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
                            let payload_ty = expected_payload.clone();
                            jit_spawn_site_with(
                                lam,
                                cx,
                                env,
                                |lam: &crate::AST::Lambda, cx: &Cx, env: &LowerEnv| {
                                    let mut jit_lambda = lower_spawn_lambda_for_jit(lam, cx, env);
                                    if let Some(ty) = payload_ty {
                                        if jit_lambda.params.is_empty() {
                                            jit_lambda.params.push(("__payload".into(), ty));
                                        } else {
                                            for (_, pty) in &mut jit_lambda.params {
                                                *pty = ty.clone();
                                            }
                                        }
                                    }
                                    jit_lambda
                                },
                            );
                            let tl = expected_hook_result.as_ref().map_or_else(
                                || lower_lambda_expecting_value(lam, cx, env, params.as_slice()),
                                |expected| {
                                    let mut lowered =
                                        lower_lambda_expecting_value_with_return(
                                            lam,
                                            cx,
                                            env,
                                            params.as_slice(),
                                            expected,
                                        );
                                    // The helper preserves the callback's carrier
                                    // body, while this slot's concrete generic
                                    // return is the ABI fact used by MIR emit.
                                    lowered.ret = Some(expected.clone());
                                    lowered
                                },
                            );
                            let callback_arity = params.len();
                            return TExpr {
                                ty: Type::Fn {
                                    params,
                                    ret: expected_hook_result.clone().map(Box::new),
                                    effect_bound: None,
                                    param_contract: None,
                                    // Event-family host helpers invoke handlers as `Fn(T)`,
                                    // not the ordinary Jet read callback `Fn(&T)`.
                                    call_metadata: Some(crate::AST::FunctionCallMetadata {
                                        conventions: vec![AccessConvention::Move; callback_arity],
                                        ..Default::default()
                                    }),
                                    return_view_provenance: None,
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
        });
    }
    // D-WATCH-SCOPE1: WatchHandle/WatchSet methods. Callback lambdas receive
    // a WatchEvent payload, matching the shared Core event/callback model.
    if is_watch_handle_type(recv_type.as_deref()) && is_watch_method_name(method, args.len()) {
        return in_own_frame(|| {
            let recv_t = lower_expr(receiver, cx, env);
            let result_ty = match (recv_type.as_deref(), method) {
                (_, "poll" | "events") => {
                    Type::List(Box::new(Type::Named("WatchEvent".to_string())))
                }
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
                            let idx = jit_spawn_site_with(
                                lam,
                                cx,
                                env,
                                |lam: &crate::AST::Lambda, cx: &Cx, env: &LowerEnv| {
                                    lower_spawn_lambda_for_jit_expecting(
                                        lam,
                                        cx,
                                        env,
                                        &[Type::Named("WatchEvent".to_string())],
                                    )
                                },
                            );
                            callback_index = Some(idx);
                            return TExpr {
                                ty: Type::Fn {
                                    params,
                                    ret: None,
                                    effect_bound: None,
                                    return_view_provenance: None,
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
        });
    }
    // D-PROCESS1: ProcessSpec/ProcessChild methods lower to explicit prelude
    // helpers; sema already proved arity and argument types.
    if matches!(
        recv_type.as_deref(),
        Some("ProcessSpec") | Some("ProcessChild") | Some("TerminalSession")
    ) {
        return in_own_frame(|| {
            let recv_t = lower_expr(receiver, cx, env);
            let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
            let result_ty = in_own_frame(|| match (recv_type.as_deref(), method) {
                (Some("ProcessSpec"), "run" | "run_checked") => Type::Result {
                    ok: Box::new(Type::Named("ProcessReceipt".to_string())),
                    err: Box::new(Type::Named("IOError".to_string())),
                },
                (Some("ProcessSpec"), "plan") => Type::Result {
                    ok: Box::new(Type::Named("ProcessPlan".to_string())),
                    err: Box::new(Type::Named("IOError".to_string())),
                },
                (Some("ProcessSpec"), "spawn") => Type::Result {
                    ok: Box::new(Type::Named("ProcessChild".to_string())),
                    err: Box::new(Type::Named("IOError".to_string())),
                },
                (Some("ProcessSpec"), "abilities") => Type::Apply {
                    name: "Set".to_string(),
                    args: vec![Type::String],
                },
                (Some("ProcessSpec"), _) => Type::Named("ProcessSpec".to_string()),
                (Some("ProcessChild"), "id") => Type::Int,
                (Some("ProcessChild"), "wait") => Type::Result {
                    ok: Box::new(Type::Named("ProcessReceipt".to_string())),
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
            });
            let op = if recv_type.as_deref() == Some("ProcessSpec") {
                THandleOp::ProcessSpecMethod {
                    method: method.to_string(),
                    args_len: args.len(),
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
        });
    }
    // D-PROCESS1=A / D-FOUND-LIFECYCLE1=A: `.close()` on `child.stdin`
    // drops the writer through the shared Prelude, making the child's next
    // read observe EOF.
    if recv_type.as_deref() == Some("ProcessStdin") && method == "close" {
        return in_own_frame(|| {
            let recv_t = lower_expr(receiver, cx, env);
            TExpr {
                ty: unit_type(),
                kind: TExprKind::HandleMethod {
                    recv: Box::new(recv_t),
                    op: THandleOp::ProcessStdinClose,
                    args: Vec::new(),
                },
            }
        });
    }
    // D-PROCESS1=A: `.write(text)` on `child.stdin` — the receiver LOWERS to the
    // real `ProcessChild.stdin` Rust field (a writer handle), and the write goes
    // through the generic `jet_process_stdin_write` prelude helper.
    if recv_type.as_deref() == Some("ProcessStdin") && method == "write" {
        return in_own_frame(|| {
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
        });
    }
    // D-HONESTNUM1=A: a `Measurement<Float>` method (gate shape d6).
    if recv_type.as_deref() == Some("Measurement") && is_measurement_method_name(method, args.len())
    {
        return in_own_frame(|| {
            let recv_t = lower_expr(receiver, cx, env);
            let result_ty = match method {
                "value" | "uncertainty" => Type::Float,
                _ => recv_t.ty.clone(),
            };
            let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
            let lowered = TExpr {
                ty: result_ty,
                kind: TExprKind::HandleMethod {
                    recv: Box::new(recv_t),
                    op: THandleOp::MeasurementMethod {
                        method: method.to_string(),
                    },
                    args: targs,
                },
            };
            return match source_arg_order(args) {
                Some(order) => {
                    preserve_source_arg_order(lowered, &order, args.len(), method_span.start as u32)
                }
                None => lowered,
            };
        });
    }
    // D-PENDING1=B: a `Loadable<T,E>` method (gate shape d7).
    if recv_type.as_deref() == Some("Loadable") && is_loadable_method_name(method, args.len()) {
        return in_own_frame(|| {
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
        });
    }
    // D-SHAPE-CTORVERB1=C: generic `ExpiringValue<T>` uses type-owned construction.
    if recv_type.as_deref() == Some(Syntax::EXPIRING_VALUE_TYPE)
        && matches!(method, "get" | "is_valid" | "force")
    {
        return in_own_frame(|| {
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
            let targs: Vec<TExpr> = args
                .iter()
                .map(|arg| lower_expr(&arg.expr, cx, env))
                .collect();
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
        });
    }
    // D-RENDERTGT2=A (c133 M1/M2): a UI backend method (gate shape d7b).
    if matches!(
        recv_type.as_deref(),
        Some("NullBackend" | "TuiBackend" | "GtkBackend")
    ) && is_ui_backend_method_name(recv_type.as_deref(), method, args.len())
    {
        return in_own_frame(|| {
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
                    jit_spawn_site(lam, cx, env);
                }
            }
            let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
            let route_method = if method == "mount" && args.len() == 1 {
                "mount_default"
            } else {
                method
            };
            return TExpr {
                ty: result_ty,
                kind: TExprKind::HandleMethod {
                    recv: Box::new(recv_t),
                    op: THandleOp::UiBackendMethod {
                        method: route_method.to_string(),
                    },
                    args: targs,
                },
            };
        });
    }
    // D-DATA-PLOT1=A: JetDataPlot receiver methods are projections onto the
    // checked receiver Core rows. Leading-dot fields become the private typed
    // column constructor marker, which MIR lowers to the canonical ClosureMethod
    // route consumed by AOT and JIT.
    if recv_type.as_deref() == Some("JetDataPlot") {
        let core_method = if method == "text" {
            "text_channel"
        } else {
            method
        };
        if let Some(record) = Syntax::core_receiver_method("JetDataPlot", core_method) {
            return in_own_frame(|| {
                let lowered_receiver = lowered_receiver.borrow_mut().take();
                let receiver = lowered_receiver.unwrap_or_else(|| lower_expr(receiver, cx, env));
                let row_ty = match receiver.ty.without_user_tags() {
                    Type::Apply { name, args } if name == "JetDataPlot" => args.first().cloned(),
                    _ => None,
                };
                let Some(row_ty) = row_ty else {
                    return invariant_method_expr(
                        method_span,
                        "checked plot receiver has no row type",
                    );
                };
                let selector_method = matches!(
                    core_method,
                    "x" | "y" | "color" | "size" | "text_channel" | "detail" | "facet"
                );
                let mut lowered_args = Vec::with_capacity(args.len() + 1);
                lowered_args.push(receiver);
                for (index, arg) in args.iter().enumerate() {
                    if selector_method && index == 0 {
                        let Some(field_name) = plot_selector_name(&arg.expr) else {
                            return invariant_method_expr(
                                method_span,
                                format!(
                                    "checked plot `{method}` argument is not a leading-dot field"
                                ),
                            );
                        };
                        match lower_plot_column(field_name, &row_ty, method_span, cx, env) {
                            Ok(column) => lowered_args.push(column),
                            Err(expr) => return expr,
                        }
                    } else {
                        lowered_args.push(lower_expr(&arg.expr, cx, env));
                    }
                }
                if !record.accepts_arity(lowered_args.len()) {
                    return invariant_method_expr(
                        method_span,
                        format!(
                            "checked plot Core row `{core_method}` has wrong arity {}",
                            lowered_args.len()
                        ),
                    );
                }
                let Some(ty) = resolved_ret.cloned() else {
                    return invariant_method_expr(
                        method_span,
                        format!("checked plot `{method}` has no resolved return type"),
                    );
                };
                TExpr {
                    ty: ty.clone(),
                    kind: TExprKind::CoreCall {
                        record,
                        args: lowered_args,
                        source_span: method_span,
                        type_args: Vec::new(),
                        widen_to_vec: vec![false; args.len() + 1],
                        data_plan: None,
                        fallibility: TFailureCarrier::from_checked_type(&ty),
                    },
                }
            });
        }
    }

    // D-FLAGSHIP-WEBAPI1=A: instance web operations are projections onto the
    // checked Core rows, never a second receiver dispatch table.  The source
    // receiver is the first Core argument; sema has already supplied the
    // generic element type and result carrier.
    if recv_type.as_deref() == Some("WebVirtualWindow") && method == "facts_json" && args.is_empty()
    {
        return in_own_frame(|| {
            let Some(result_ty) = resolved_ret.cloned() else {
                return invariant_method_expr(
                    method_span,
                    "virtual window facts has no resolved return type".to_string(),
                );
            };
            TExpr {
                ty: result_ty,
                kind: TExprKind::HandleMethod {
                    recv: Box::new(lower_expr(receiver, cx, env)),
                    op: THandleOp::WebVirtualWindowFacts,
                    args: Vec::new(),
                },
            }
        });
    }
    let web_projection = web_receiver_projection(recv_type.as_deref(), method, args.len());
    if let Some((module, core_method)) = web_projection {
        return in_own_frame(|| {
            let receiver = lower_expr(receiver, cx, env);
            let mut lowered_args = Vec::with_capacity(args.len() + 1);
            lowered_args.push(receiver);
            lowered_args.extend(args.iter().map(|arg| {
                let lowered = lower_expr(&arg.expr, cx, env);
                if matches!(lowered.ty, Type::Fn { .. })
                    && !matches!(
                        lowered.kind,
                        TExprKind::FnValue {
                            kind: TFnValueKind::Send { .. }
                        }
                    )
                {
                    // Callback rows take the send-safe carrier, exactly as
                    // the module-call spelling of the same Core row.
                    let ty = lowered.ty.clone();
                    TExpr {
                        ty,
                        kind: TExprKind::FnValue {
                            kind: TFnValueKind::Send {
                                value: Box::new(lowered),
                            },
                        },
                    }
                } else {
                    lowered
                }
            }));
            let Some(ty) = resolved_ret.cloned() else {
                return invariant_method_expr(
                    method_span,
                    format!("web receiver `{method}` has no resolved return type"),
                );
            };
            let record =
                match checked_core_record(module, core_method, lowered_args.len(), method_span) {
                    Ok(record) => record,
                    Err(expr) => return expr,
                };
            TExpr {
                ty: ty.clone(),
                kind: TExprKind::CoreCall {
                    record,
                    widen_to_vec: vec![false; lowered_args.len()],
                    type_args: Vec::new(),
                    fallibility: TFailureCarrier::from_checked_type(&ty),
                    args: lowered_args,
                    source_span: method_span,
                    data_plan: None,
                },
            }
        });
    }

    // c-devserver (owner-directed 2026-07-01): a DevServer builder method.
    if recv_type.as_deref() == Some("DevServer") && is_devserver_method_name(method, args.len()) {
        return in_own_frame(|| {
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
        });
    }
    // D-WEBAPP1=D / D-DX-ROUTER1=A: App builder methods remain one typed handle
    // operation.  Route, boundary, loader, and server-function handlers are
    // handed over as send-safe borrow callbacks; route and loader handlers
    // also carry the sema-checked input binding so the runtime decodes each
    // parameter from the navigation instead of guessing positions.
    if recv_type.as_deref() == Some("App") && is_app_method_name(method, args.len()) {
        return in_own_frame(|| {
            let recv_t = lower_expr(receiver, cx, env);
            let result_ty = match method {
                "serve" | "serve_on" => unit_type(),
                "facts_json" => Type::String,
                _ => Type::Named("App".to_string()),
            };
            let mut targs: Vec<TExpr> = args
                .iter()
                .enumerate()
                .map(|(index, arg)| {
                    if matches!(
                        (method, index),
                        ("route" | "page" | "layout", 1)
                            | ("loader", 1)
                            | ("pending" | "not_found" | "error", 0)
                            | ("action" | "form" | "data", 1)
                    ) {
                        lower_app_route_callback(&arg.expr, cx, env)
                    } else if matches!(
                        (method, index),
                        ("mount" | "mount_with_effect" | "mount_with_policy", 1)
                    ) {
                        lower_app_callback(&arg.expr, cx, env)
                    } else {
                        lower_expr(&arg.expr, cx, env)
                    }
                })
                .collect();
            if matches!(method, "route" | "page" | "layout" | "loader") {
                let handler = args.get(1).map(|arg| &arg.expr);
                targs.push(devtools_text(app_route_binding(method, handler, cx)));
            }
            TExpr {
                ty: result_ty,
                kind: TExprKind::HandleMethod {
                    recv: Box::new(recv_t),
                    op: THandleOp::AppMethod {
                        method: method.to_string(),
                        args_len: args.len(),
                    },
                    args: targs,
                },
            }
        });
    }
    // D-NETDEP1=A / D-HTTPLIB1=A: an HTTP type method (gate shape d10).
    let http_route_registration =
        matches!(recv_type.as_deref(), Some("HTTPRouter") | Some("HTTPMux"))
            && matches!(
                method,
                "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
            )
            && args.len() == 2;
    if is_http_type(recv_type.as_deref())
        && is_http_method_name(recv_type.as_deref(), method)
        && !http_route_registration
    {
        return in_own_frame(|| {
            let kind = recv_type.as_deref().unwrap_or("HTTPRequest").to_string();
            let recv_t = lower_expr(receiver, cx, env);
            let result_ty = in_own_frame(|| match (kind.as_str(), method) {
                (
                    "HTTPRequest",
                    "header" | "body" | "timeout" | "connect_timeout" | "read_timeout"
                    | "total_timeout" | "dns_timeout" | "tls_timeout" | "write_timeout"
                    | "first_byte_timeout" | "redirects" | "proxy" | "cookie" | "form"
                    | "multipart_text",
                ) if !args.is_empty() && !(method == "header" && args.len() == 1) => {
                    Type::Named("HTTPRequest".to_string())
                }
                ("HTTPRequest", "json") if args.len() == 1 => {
                    Type::Named("HTTPRequest".to_string())
                }
                ("HTTPRequest", "send") => Type::Result {
                    ok: Box::new(Type::Named("HTTPResponse".to_string())),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                },
                ("HTTPRequest", "method" | "path") => Type::String,
                ("HTTPRequest", "text") => Type::Result {
                    ok: Box::new(Type::String),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                },
                ("HTTPRequest", "body") => Type::Named("HTTPBody".to_string()),
                ("HTTPRequest", "trailers") => Type::Result {
                    ok: Box::new(Type::Named("HTTPHeaders".to_string())),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                },
                ("HTTPRequest", "body_len") => Type::Int,
                ("HTTPRequest", "under_limit") => Type::Bool,
                ("HTTPRequest", "param" | "header") => Type::Option(Box::new(Type::String)),
                (
                    "HTTPClient",
                    "cookies"
                    | "redirects"
                    | "protocols"
                    | "timeouts"
                    | "raw_encoding"
                    | "proxy"
                    | "tls"
                    | "allow_http_downgrade"
                    | "retries",
                ) => Type::Named("HTTPClient".to_string()),
                ("HTTPClient", "send") => Type::Result {
                    ok: Box::new(Type::Named("HTTPResponse".to_string())),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                },
                ("HTTPResponse", "header") if args.len() == 2 => {
                    Type::Named("HTTPResponse".to_string())
                }
                ("HTTPResponse", "status") => Type::Int,
                ("HTTPResponse", "text") => Type::Result {
                    ok: Box::new(Type::String),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                },
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
                    || {
                        resolved_ret.cloned().unwrap_or_else(|| Type::Result {
                            ok: Box::new(Type::Named("Unknown".to_string())),
                            err: Box::new(Type::Named("HTTPError".to_string())),
                        })
                    },
                    |target| Type::Result {
                        ok: Box::new(target.clone()),
                        err: Box::new(Type::Named("HTTPError".to_string())),
                    },
                ),
                // D-HTTP-JSON1=A: `req.json<T>()` / `resp.json<T>(limit)`.
                ("HTTPRequest", "json") | ("HTTPResponse", "json") => {
                    type_args.first().map_or_else(
                        || {
                            resolved_ret.cloned().unwrap_or_else(|| Type::Result {
                                ok: Box::new(Type::Named("Unknown".to_string())),
                                err: Box::new(Type::Named("HTTPError".to_string())),
                            })
                        },
                        |target| Type::Result {
                            ok: Box::new(target.clone()),
                            err: Box::new(Type::Named("HTTPError".to_string())),
                        },
                    )
                }
                ("HTTPBody", "chunks") => Type::Named("HTTPBodyChunks".to_string()),
                ("HTTPBody", "copy_to") => Type::Result {
                    ok: Box::new(Type::Int),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                },
                ("HTTPServer", "local_addr") => Type::Result {
                    ok: Box::new(Type::String),
                    err: Box::new(Type::Named("HTTPError".to_string())),
                },
                ("HTTPServer", "serve" | "shutdown" | "wait") => Type::Result {
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
                ("Browser", "abilities") => Type::Named("BrowserAbilities".to_string()),
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
                (
                    "Browser",
                    "continue_request" | "fail_request" | "fulfill_request" | "allow_downloads",
                ) => Type::Result {
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
                | (
                    "BrowserPage",
                    "goto" | "close" | "clear_cookies" | "set_cookie" | "storage_set"
                    | "storage_clear",
                )
                | ("BrowserFrame", "close")
                | ("BrowserIntercept", "remove")
                | (
                    "BrowserLocator",
                    "wait" | "wait_gone" | "click" | "hover" | "fill" | "press" | "set_files",
                ) => Type::Result {
                    ok: Box::new(unit_type()),
                    err: Box::new(Type::Named("BrowserError".to_string())),
                },
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
                    ok: Box::new(Type::List(Box::new(Type::Named(
                        "BrowserFrame".to_string(),
                    )))),
                    err: Box::new(Type::Named("BrowserError".to_string())),
                },
                (
                    "BrowserPage",
                    "get_by_role" | "get_by_text" | "get_by_label" | "get_by_placeholder"
                    | "get_by_test_id" | "get_by_css",
                ) => Type::Named("BrowserLocator".to_string()),
                (
                    "BrowserEvent",
                    "kind"
                    | "request_id"
                    | "request_method"
                    | "url_hash"
                    | "download_id"
                    | "suggested_filename_hash",
                )
                | ("BrowserAbilities", "profile")
                | ("BrowserTrace", "summary")
                | ("BrowserReceipt", "summary")
                | ("BrowserLocked", "engine" | "version" | "binary" | "protocol") => Type::String,
                ("BrowserEvent", "status_code") => Type::Int,
                ("BrowserProtocol", "send") => Type::Result {
                    ok: Box::new(Type::String),
                    err: Box::new(Type::Named("BrowserError".to_string())),
                },
                ("BrowserEvent", "is_blocked")
                | ("BrowserAbilities", "bidi" | "cdp")
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
            });
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
                            let params = if lam.params.is_empty() {
                                Vec::new()
                            } else {
                                vec![Type::Named("HTTPRequest".to_string())]
                            };
                            let ret = Type::Result {
                                ok: Box::new(Type::Named("HTTPResponse".to_string())),
                                err: Box::new(Type::Named("HTTPError".to_string())),
                            };
                            return TExpr {
                                ty: Type::Fn {
                                    params: params.clone(),
                                    ret: Some(Box::new(ret)),
                                    effect_bound: None,
                                    return_view_provenance: None,
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
                (
                    "HTTPRequest",
                    "method" | "path" | "param" | "body_len" | "under_limit",
                    _
                ) | ("HTTPRequest", "header", 1)
                    | ("HTTPRequest", "body", 0)
                    | ("HTTPRequest", "text", 0 | 1)
                    | ("HTTPRequest", "json", 0)
                    | ("HTTPRequest", "trailers", 0)
                    | ("HTTPResponse", "header", 2)
                    | ("HTTPResponse", "trailers", 1)
            );
            let op = match (kind.as_str(), method, args.len()) {
                // These request/response operations already have typed handle
                // rows in the canonical registry. Keep them as those rows
                // instead of routing through the broad HTTPServerMethod bucket,
                // whose only Prelude projection is the HTTPServer receiver.
                ("HTTPRequest", "header", 1) => THandleOp::HTTPReqHeader,
                ("HTTPRequest", "param", 1) => THandleOp::HTTPReqParam,
                ("HTTPRequest", "trailers", 0) => THandleOp::HTTPReqTrailers,
                ("HTTPResponse", "trailers", 1) => THandleOp::HTTPRespTrailers,
                ("HTTPResponse", "header", 1) => THandleOp::HTTPRespHeader,
                _ if server_message_method
                    || kind.starts_with("HTTPServer")
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
                            | "BrowserAbilities"
                            | "BrowserProtocol"
                            | "BrowserLocked"
                    ) =>
                {
                    THandleOp::HTTPServerMethod {
                        kind,
                        method: method.to_string(),
                    }
                }
                _ => THandleOp::HTTPClientMethod {
                    kind,
                    method: method.to_string(),
                },
            };
            return TExpr {
                ty: result_ty,
                kind: TExprKind::HandleMethod {
                    recv: Box::new(recv_t),
                    op,
                    args: targs,
                },
            };
        });
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
        return in_own_frame(|| {
            let kind = recv_type.as_deref().unwrap_or("Date").to_string();
            let recv_t = lower_expr(receiver, cx, env);
            let result_ty = in_own_frame(|| match (kind.as_str(), method) {
                ("Date" | "LocalDate", "year")
                | ("Date" | "LocalDate", "month")
                | ("Date" | "LocalDate", "day")
                | ("Date" | "LocalDate", "diff_days")
                | ("Date" | "LocalDate", "weekday")
                | ("Date" | "LocalDate", "iso_weekday")
                | ("Date" | "LocalDate", "day_of_year")
                | ("Date" | "LocalDate", "iso_week")
                | ("Date" | "LocalDate", "iso_week_year")
                | ("Date" | "LocalDate", "quarter_of_year")
                | ("Date" | "LocalDate", "days_in_month") => Type::Int,
                ("Date" | "LocalDate", "is_leap_year") => Type::Bool,
                (
                    "Date" | "LocalDate",
                    "add_days" | "add_months" | "add_period" | "subtract_period" | "truncate"
                    | "replace",
                ) => Type::Named("LocalDate".to_string()),
                ("Date" | "LocalDate", "with") => Type::Result {
                    ok: Box::new(Type::Named("LocalDate".to_string())),
                    err: Box::new(Type::String),
                },
                ("Date" | "LocalDate", "to_string" | "format") => Type::String,
                ("Date" | "LocalDate", "format_checked") => Type::Result {
                    ok: Box::new(Type::String),
                    err: Box::new(Type::Named("TextError".to_string())),
                },
                ("Date" | "LocalDate", "until" | "since") => Type::Named("Duration".to_string()),
                (
                    "LocalTime",
                    "hour" | "minute" | "second" | "millisecond" | "microsecond" | "nanosecond",
                ) => Type::Int,
                (
                    "LocalTime",
                    "add_duration" | "subtract_duration" | "round" | "truncate" | "floor" | "ceil",
                ) => Type::Named("LocalTime".to_string()),
                ("LocalTime", "until" | "since") => Type::Named("Duration".to_string()),
                ("LocalTime", "to_string" | "format") => Type::String,
                ("LocalTime", "format_checked") => Type::Result {
                    ok: Box::new(Type::String),
                    err: Box::new(Type::Named("TextError".to_string())),
                },
                (
                    "Date" | "LocalDate" | "LocalTime" | "DateTime" | "Instant" | "ZonedDateTime",
                    "equal",
                ) => Type::Bool,
                (
                    "Date" | "LocalDate" | "LocalTime" | "DateTime" | "Instant" | "ZonedDateTime",
                    "compare",
                ) => Type::Named(crate::Syntax::TYPE_ORDERING.to_string()),
                ("DateTime", "hour")
                | ("DateTime", "minute")
                | ("DateTime", "second")
                | ("DateTime", "millisecond")
                | ("DateTime", "microsecond")
                | ("DateTime", "nanosecond")
                | ("DateTime", "to_timestamp")
                | ("DateTime", "to_unix_ms")
                | ("DateTime", "to_unix_s") => Type::Int,
                ("DateTime", "to_unix_us" | "to_unix_ns") => Type::Result {
                    ok: Box::new(Type::Int),
                    err: Box::new(Type::Named("RangeError".to_string())),
                },
                ("DateTime", "date") => Type::Named("LocalDate".to_string()),
                ("DateTime", "time") => Type::Named("LocalTime".to_string()),
                (
                    "DateTime",
                    "plus_duration" | "subtract_duration" | "add_nanoseconds" | "add_period"
                    | "subtract_period" | "truncate" | "round" | "floor" | "ceil" | "replace",
                ) => Type::Named("DateTime".to_string()),
                ("DateTime", "with") => Type::Result {
                    ok: Box::new(Type::Named("DateTime".to_string())),
                    err: Box::new(Type::String),
                },
                ("DateTime", "difference" | "until" | "since") => {
                    Type::Named("Duration".to_string())
                }
                ("DateTime", "in_zone") => Type::Named("ZonedDateTime".to_string()),
                ("DateTime", "to_string" | "format_rfc3339" | "format") => Type::String,
                ("DateTime", "format_checked") => Type::Result {
                    ok: Box::new(Type::String),
                    err: Box::new(Type::Named("TextError".to_string())),
                },
                ("Instant", "elapsed_millis") => Type::Int,
                ("Instant", "elapsed") => Type::Named("Duration".to_string()),
                ("Period", "years" | "months" | "days" | "sign") => Type::Int,
                ("Period", "is_zero") => Type::Bool,
                ("Period", "abs" | "negated" | "add" | "sub") => Type::Named("Period".to_string()),
                ("Period", "total_in") => Type::Float,
                ("Period", "to_string") => Type::String,
                ("Zone", "name") => Type::String,
                ("Zone", "next_transition" | "previous_transition") => {
                    Type::Option(Box::new(Type::Int))
                }
                ("Zone", "start_of_day") => Type::Named("ZonedDateTime".to_string()),
                ("Zone", "hours_in_day") => Type::Int,
                ("ZonedDateTime", "date") => Type::Named("LocalDate".to_string()),
                ("ZonedDateTime", "time") => Type::Named("LocalTime".to_string()),
                ("ZonedDateTime", "offset_seconds") => Type::Int,
                ("ZonedDateTime", "is_dst") => Type::Bool,
                ("ZonedDateTime", "to_datetime") => Type::Named("DateTime".to_string()),
                ("ZonedDateTime", "zone") => Type::Named("Zone".to_string()),
                (
                    "ZonedDateTime",
                    "add_duration" | "subtract_duration" | "add_period" | "subtract_period",
                ) => Type::Named("ZonedDateTime".to_string()),
                ("ZonedDateTime", "with_zone" | "start_of_day") => {
                    Type::Named("ZonedDateTime".to_string())
                }
                ("ZonedDateTime", "with_time") => Type::Result {
                    ok: Box::new(Type::Named("ZonedDateTime".to_string())),
                    err: Box::new(Type::String),
                },
                ("ZonedDateTime", "until" | "since") => Type::Named("Duration".to_string()),
                ("ZonedDateTime", "next_transition" | "previous_transition") => {
                    Type::Option(Box::new(Type::Int))
                }
                ("ZonedDateTime", "hours_in_day") => Type::Int,
                ("ZonedDateTime", "to_string" | "format" | "format_rfc9557") => Type::String,
                ("ZonedDateTime", "format_checked") => Type::Result {
                    ok: Box::new(Type::String),
                    err: Box::new(Type::Named("TextError".to_string())),
                },
                _ => unit_type(),
            });
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
        });
    }
    // D-TIMEDEPTH1=A: Duration is represented as raw nanoseconds in TIR/JIT,
    // so its comparison hook reuses the typed binary node. The node's Compare
    // form emits the same Prelude Comparable symbol as the release hook, while
    // evaluator and JIT already marshal Duration comparisons there.
    if recv_type.as_deref() == Some(crate::Syntax::DURATION_TYPE)
        && matches!(method, "equal" | "compare")
        && args.len() == 1
    {
        let op = if method == "equal" {
            crate::AST::BinOp::Eq
        } else {
            crate::AST::BinOp::Compare
        };
        let operator = Expr::Binary(
            op,
            Box::new(receiver.clone()),
            Box::new(args[0].expr.clone()),
            method_span,
        );
        return lower_expr(&operator, cx, env);
    }
    // D-APPROX1=A: a sketch method (gate shape d8). Sema normally persists the
    // nominal receiver in `recv_type`; comptime fragments can retain the same
    // concrete receiver only in the environment. Derive that name from the
    // checked receiver type as the fallback so all engines select one sketch
    // operation rather than the generic method/math fallback.
    let sketch = recv_type.clone().or_else(|| {
        let Type::Named(name) = tir_recv_jet_ty(receiver, env)? else {
            return None;
        };
        is_sketch_type(Some(name.as_str())).then_some(name)
    });
    if is_sketch_type(sketch.as_deref()) && is_sketch_method_name(sketch.as_deref(), method) {
        return in_own_frame(|| {
            let sketch = sketch.as_deref().unwrap_or("").to_string();
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
            let targs: Vec<TExpr> = args
                .iter()
                .map(|a| {
                    let value = lower_expr(&a.expr, cx, env);
                    if sketch == "TDigest" && matches!(method, "add" | "quantile") {
                        let source_name = value.ty.without_user_tags().name();
                        if let Some((type_name, func)) =
                            crate::Numeric::precise_conversion_route("Float", &source_name)
                        {
                            return TExpr {
                                ty: Type::Float,
                                kind: TExprKind::PreciseBuiltin {
                                    type_name: type_name.to_string(),
                                    func: func.to_string(),
                                    args: vec![value],
                                },
                            };
                        }
                        if source_name != "Float"
                            && crate::AST::numeric_type_from_name(&source_name).is_some()
                        {
                            if let Some(op) = resolve_numeric_conversion_op("Float", &source_name)
                            {
                                return TExpr {
                                    ty: Type::Float,
                                    kind: TExprKind::NumericMethod {
                                        recv: Box::new(value),
                                        op,
                                    },
                                };
                            }
                        }
                    }
                    value
                })
                .collect();
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
        });
    }
    // c109 Phase 21 / D-TUPLE-DESTRUCT1: a Task/Receiver/Sender concurrency method
    // (gate shape d3). The gate proved `recv_type == None` + a disjoint concurrency
    // name+arity. Resolve the op + result type HERE (totality). The result type comes
    // from `Collections::builtin_method_return`'s `Type::Apply` arms
    // (Source/Collections.rs), read off the receiver's already-resolved type
    // `Task<T>`/`Receiver<T>`/`Sender<T>` (the LOWERED receiver's `.ty`, total from the
    // binding's annotated/inferred slot — never re-inferred in emit, I3): `join`
    // → `T !TaskFailure`; `detach`/`pause`/`resume`/`cancel`/`send` → Unit;
    // `receive` → `Result<T, Closed>`. Args lowered PLAINLY (the AST
    // `emit_builtin_method`'s `arg(i)` is a raw `emit_expr`).
    if recv_type.is_none() && is_concurrency_method_name(method, args.len()) {
        return in_own_frame(|| {
            let recv_t = lower_expr(receiver, cx, env);
            // `Task<T>` is source-shaped in sema, while a spawned closure may
            // store its effective `Result<T, E>` carrier in the handle.  `join`
            // exposes the source success value, so project one private carrier
            // layer before constructing its public `T !TaskFailure` result.
            let elem = match &recv_t.ty {
                Type::Apply { name, args } if name == "Task" => match args.first() {
                    Some(Type::Result { ok, .. }) | Some(Type::Option(ok)) => Some((**ok).clone()),
                    Some(other) => Some(other.clone()),
                    None => None,
                },
                Type::Apply { args, .. } => args.first().cloned(),
                _ => None,
            };
            let elem = elem.unwrap_or_else(unit_type);
            let (op, ty) = in_own_frame(|| match method {
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
            });
            let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
            return TExpr {
                ty,
                kind: TExprKind::HandleMethod {
                    recv: Box::new(recv_t),
                    op,
                    args: targs,
                },
            };
        });
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
        let is_shared =
            recv_type.as_deref() == Some("Shared") || matches!(&recv_peek, Some(Type::Shared(_)));
        let is_shared_snapshot = recv_type.as_deref() == Some(Syntax::TYPE_SHARED_SNAPSHOT)
            || matches!(
                &recv_peek,
                Some(Type::Apply { name, .. }) if name == Syntax::TYPE_SHARED_SNAPSHOT
            );
        if is_shared_snapshot && method == "value" && args.is_empty() {
            return in_own_frame(|| {
                let ty = resolved_ret.cloned().unwrap_or_else(|| match &recv_peek {
                    Some(Type::Apply { args, .. }) if args.len() == 2 => args[1].clone(),
                    _ => Type::Int,
                });
                return TExpr {
                    ty,
                    kind: TExprKind::HostCall(Box::new(THostCall::Method {
                        recv: Box::new(lower_expr(receiver, cx, env)),
                        method: method.to_string(),
                        args: Vec::new(),
                    })),
                };
            });
        }
        let cell_receiver = recv_type
            .as_deref()
            .filter(|name| matches!(*name, "Cell" | "CellReadGuard" | "CellEditGuard"));
        let is_expiring_secret = recv_type.as_deref() == Some("ExpiringSecret");
        if is_pool && matches!(method, "add" | "remove" | "ids") && args.len() <= 1 {
            return in_own_frame(|| {
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
                let targs: Vec<TExpr> = args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
                return TExpr {
                    ty,
                    kind: TExprKind::HostCall(Box::new(THostCall::Method {
                        recv: Box::new(recv_t),
                        method: method.to_string(),
                        args: targs,
                    })),
                };
            });
        }
        if is_shared && method == "capture" && args.len() <= 1 {
            return in_own_frame(|| {
                let inner = match &recv_peek {
                    Some(Type::Shared(inner)) => (**inner).clone(),
                    _ => Type::Int,
                };
                let recv_t = lower_expr(receiver, cx, env);
                let (targs, projection_ty) = if let Some(arg) = args.first() {
                    let Expr::Lambda(lam) = &arg.expr else {
                        unreachable!("sema's finish_shared_capture requires a lambda argument");
                    };
                    let expected = std::slice::from_ref(&inner);
                    let lowered = lower_lambda_expecting_host_borrow(lam, cx, env, expected, false);
                    let projection_ty = lambda_body_ty_expecting(lam, cx, env, Some(expected));
                    (
                        vec![TExpr {
                            ty: Type::Fn {
                                params: vec![inner.clone()],
                                ret: Some(Box::new(projection_ty.clone())),
                                effect_bound: None,
                                param_contract: None,
                                call_metadata: None,
                                return_view_provenance: None,
                            },
                            kind: TExprKind::Lambda(Box::new(lowered)),
                        }],
                        projection_ty,
                    )
                } else {
                    (Vec::new(), inner.clone())
                };
                let ty = resolved_ret.cloned().unwrap_or_else(|| Type::Apply {
                    name: Syntax::TYPE_SHARED_SNAPSHOT.to_string(),
                    args: vec![inner, projection_ty],
                });
                let method_out = if cx.in_stm_transact.get() {
                    cx.stm_touched.set(true);
                    "capture_txn"
                } else {
                    "capture"
                };
                return TExpr {
                    ty,
                    kind: TExprKind::HostCall(Box::new(THostCall::Method {
                        recv: Box::new(recv_t),
                        method: method_out.to_string(),
                        args: targs,
                    })),
                };
            });
        }
        if is_shared && method == "try_replace" && args.len() == 2 {
            return in_own_frame(|| {
                let recv_t = lower_expr(receiver, cx, env);
                let targs = args
                    .iter()
                    .map(|arg| lower_expr(&arg.expr, cx, env))
                    .collect();
                let ty = resolved_ret.cloned().unwrap_or_else(|| Type::Result {
                    ok: Box::new(Type::Bool),
                    err: Box::new(Type::Named(Syntax::TYPE_SHARED_REVISION_ERROR.to_string())),
                });
                return TExpr {
                    ty,
                    kind: TExprKind::HostCall(Box::new(THostCall::Method {
                        recv: Box::new(recv_t),
                        method: method.to_string(),
                        args: targs,
                    })),
                };
            });
        }
        if is_shared && matches!(method, "read" | "edit") && args.len() == 1 {
            return in_own_frame(|| {
                let inner = match &recv_peek {
                    Some(Type::Shared(inner)) => (**inner).clone(),
                    _ => Type::Int,
                };
                let recv_t = lower_expr(receiver, cx, env);
                let Expr::Lambda(lam) = &args[0].expr else {
                    unreachable!(
                        "sema's finish_shared_read/finish_shared_edit require a lambda arg"
                    );
                };
                let expected = std::slice::from_ref(&inner);
                // `JetShared::read`/`edit` lend `&T`/`&mut T` directly. This host
                // borrow is not an unmarked function-value parameter and must not
                // receive another D-MEM-PARAM1 Read borrow.
                let mut tl =
                    lower_lambda_expecting_host_borrow(lam, cx, env, expected, method == "edit");
                // D-CONC-STM1=A: a Shared read inside a transaction registers
                // its participant before doing the ordinary immediate read.
                // A write still defers to commit. The Prelude then acquires
                // every registered participant in one stable address order.
                let (method_out, ty) = if cx.in_stm_transact.get() {
                    cx.stm_touched.set(true);
                    if method == "edit" {
                        // The closure is stored past the call, so it must move
                        // its captures. A transactional edit yields Unit.
                        tl.is_move = true;
                        ("edit_txn", Type::Tuple(vec![]))
                    } else {
                        (
                            "read_txn",
                            lambda_body_ty_expecting(lam, cx, env, Some(expected)),
                        )
                    }
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
                                effect_bound: None,
                                return_view_provenance: None,
                                param_contract: None,
                                call_metadata: None,
                            },
                            kind: TExprKind::Lambda(Box::new(tl)),
                        }],
                    })),
                };
            });
        }
        if is_shared && matches!(method, "guard_read" | "guard_edit") && args.is_empty() {
            return in_own_frame(|| {
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
            });
        }
        // D-SHARED-CYCLE1=C: Shared.downgrade / strong_count → inherent JetShared methods.
        if is_shared && matches!(method, "downgrade" | "strong_count") && args.is_empty() {
            return in_own_frame(|| {
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
            });
        }
        let is_shared_weak = recv_type.as_deref() == Some(Syntax::TYPE_SHARED_WEAK)
            || matches!(
                &recv_peek,
                Some(Type::Apply { name, .. }) if name == Syntax::TYPE_SHARED_WEAK
            );
        if is_shared_weak && method == "upgrade" && args.is_empty() {
            return in_own_frame(|| {
                let inner = match &recv_peek {
                    Some(Type::Apply { args, .. }) if !args.is_empty() => args[0].clone(),
                    _ => Type::Int,
                };
                let ty = resolved_ret
                    .cloned()
                    .unwrap_or_else(|| Type::Option(Box::new(Type::Shared(Box::new(inner)))));
                return TExpr {
                    ty,
                    kind: TExprKind::HostCall(Box::new(THostCall::Method {
                        recv: Box::new(lower_expr(receiver, cx, env)),
                        method: method.to_string(),
                        args: Vec::new(),
                    })),
                };
            });
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
                return in_own_frame(|| {
                    let mut paths = Vec::with_capacity(args.len());
                    for arg in args {
                        let Expr::Lambda(lambda) = &arg.expr else {
                            unreachable!(
                                "sema requires projection lambdas for Cell guard map/split"
                            );
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
                            edit_paths_disjoint: cell_receiver == "CellEditGuard"
                                && method == "split",
                        })),
                    };
                });
            }
            if matches!(
                (cell_receiver, method, args.len()),
                ("Cell", "get" | "guard_read" | "guard_edit", 0)
                    | ("Cell", "set" | "replace", 1)
                    | ("CellReadGuard" | "CellEditGuard", "get", 0)
                    | ("CellEditGuard", "set", 1)
            ) {
                return in_own_frame(|| {
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
                });
            }
            if matches!(
                (cell_receiver, method),
                ("Cell", "read" | "edit")
                    | ("CellReadGuard", "read")
                    | ("CellEditGuard", "read" | "edit")
            ) && args.len() == 1
            {
                return in_own_frame(|| {
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
                });
            }
            if cell_receiver == "Cell" && method == "get_or_set" && args.len() == 1 {
                return in_own_frame(|| {
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
                });
            }
        }
        if is_expiring_secret && method == "with" && args.len() == 1 {
            return in_own_frame(|| {
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
                                effect_bound: None,
                                return_view_provenance: None,
                                param_contract: None,
                                call_metadata: None,
                            },
                            kind: TExprKind::Lambda(Box::new(tl)),
                        }],
                    })),
                };
            });
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
        let recv_t = lowered_receiver.borrow_mut().take();
        let recv_t = recv_t.unwrap_or_else(|| lower_expr(receiver, cx, env));
        let recv_ast_ty = tir_recv_jet_ty(receiver, env);
        let recv_ty = recv_ast_ty.unwrap_or_else(|| recv_t.ty.clone());
        let fixed_float_add_receiver = !matches!(
            &recv_ty,
            Type::Apply { name, .. } if name == "Set" || name == crate::Syntax::TYPE_RANK
        );
        // #1478: Set/Rank closures (filter/map/each/all/fold/flat_map)
        // route through the same to_list()-then-List path every other
        // container's closures already use (I9 — AOT and JIT both need a
        // real `Vec`-backed list, not a raw `HashSet`/`BTreeSet`).
        let (recv_t, recv_ty) = if matches!(
            &recv_ty,
            Type::Apply { name, .. } if name == "Set" || name == crate::Syntax::TYPE_RANK
        ) {
            let wrapped = crate::Codegen::TIR::wrap_set_receiver_as_list(recv_t, method_span);
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
            || matches!(
                &recv_ty,
                Type::Named(name)
                    if crate::Sema::is_math_type(name) && !cx.type_names.contains(name)
            );
        if skip_closure {
            if let Type::Named(handle) = &recv_ty {
                let is_reduce =
                    method == "reduce" && (crate::Sema::is_simd_lane_type(handle) || reduce_value);
                if is_reduce
                    || crate::Sema::math_method_return(handle, method, args.len()).is_some()
                {
                    return in_own_frame(|| {
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
                    });
                }
            }
        }
        if !skip_closure {
            return in_own_frame(|| {
                let Some(source_result_ty) = resolved_ret.cloned().or_else(|| {
                    crate::Collections::builtin_method_return(&recv_ty, method, args.len(), false)
                        .map(|ty| ty.unwrap_or_else(unit_type))
                }) else {
                    return invariant_method_expr(
                        method_span,
                        "closure method without a checked return type",
                    );
                };
                if fixed_float_add_receiver {
                    if let Some(op) =
                        fixed_float_add_closure_op(method, &recv_ty, &source_result_ty, args)
                    {
                        let seed = if matches!(&op, TClosureOp::FloatAddParaFold { .. }) {
                            match args.first().and_then(|arg| match &arg.expr {
                                Expr::Lambda(lambda) => is_pure_zero_arg_lambda(lambda),
                                _ => None,
                            }) {
                                Some(seed) => lower_expr(seed, cx, env),
                                None => lower_expr(&args[0].expr, cx, env),
                            }
                        } else {
                            lower_expr(&args[0].expr, cx, env)
                        };
                        if seed.ty == source_result_ty {
                            return TExpr {
                                ty: source_result_ty,
                                kind: TExprKind::ClosureMethod {
                                    recv: Box::new(recv_t),
                                    op,
                                    args: vec![seed],
                                },
                            };
                        }
                    }
                }
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
                    if let Type::List(inner) | Type::FixedList { elem: inner, .. } =
                        &callback_recv_ty
                    {
                        callback_params = Some(vec![(**inner).clone(), (**inner).clone()]);
                    }
                }
                if matches!(method, "reduce" | "fold" | "scan") {
                    if let Some(seed_ty) = args.first().map(|arg| lower_expr(&arg.expr, cx, env).ty)
                    {
                        if let Some(first) = callback_params
                            .as_mut()
                            .and_then(|params| params.first_mut())
                        {
                            *first = seed_ty;
                        }
                    }
                }
                let para_fold_params = if method == "para_fold" {
                    let Some(acc) = resolved_ret.cloned() else {
                        return invariant_method_expr(
                            method_span,
                            "para_fold without a resolved accumulator type",
                        );
                    };
                    let item = match &recv_ty {
                        Type::List(inner) | Type::FixedList { elem: inner, .. } => {
                            (**inner).clone()
                        }
                        _ => {
                            return invariant_method_expr(
                                method_span,
                                "para_fold without a checked list receiver",
                            );
                        }
                    };
                    Some(vec![
                        vec![],
                        vec![acc.clone(), item],
                        vec![acc.clone(), acc],
                    ])
                } else {
                    None
                };
                let mut targs: Vec<TExpr> = args
                    .iter()
                    .enumerate()
                    .map(|(index, a)| {
                        let params = para_fold_params
                            .as_ref()
                            .and_then(|all| all.get(index))
                            .or(callback_params.as_ref());
                        if params.is_some() {
                            if let Some(callback) = lower_named_collection_callback(
                                &a.expr,
                                cx,
                                env,
                                false,
                                params.map(|types| types.as_slice()),
                            ) {
                                return callback;
                            }
                        }
                        if let (Expr::Lambda(lam), Some(params)) = (&a.expr, params) {
                            let mut tl = if method == "edit_disjoint" {
                                crate::Codegen::TIR::lower_lambda_expecting_value(
                                    lam, cx, env, params,
                                )
                            } else {
                                lower_lambda_expecting_host_borrow(lam, cx, env, params, false)
                            };
                            // D-FAILURE-FOUNDATION1=A / #2266: sema records
                            // propagation in the callback when a nested callee
                            // needs the enclosing function's carrier. The
                            // expression lowerer has already emitted its
                            // success-side `Ok` wrapper, but its probe can only
                            // see the unwrapped value type. Project that value
                            // back onto the callback's Result return before the
                            // closure op is resolved; otherwise `?` lands in a
                            // raw-return Rust closure and rustc reports E0277.
                            if lam.meta.fallible_propagation
                                && !matches!(
                                    tl.ret.as_ref(),
                                    Some(Type::Result { .. }) | Some(Type::Option(_))
                                )
                            {
                                let callback_ok = tl.ret.clone().unwrap_or_else(unit_type);
                                if let Some(error) = env.ret_ty.as_ref().and_then(|ret| match ret {
                                    Type::Result { err, .. } => Some(Type::Result {
                                        ok: Box::new(callback_ok.clone()),
                                        err: err.clone(),
                                    }),
                                    Type::Option(_) => {
                                        Some(Type::Option(Box::new(callback_ok.clone())))
                                    }
                                    _ => None,
                                }) {
                                    tl.ret = Some(error);
                                }
                            }
                            return TExpr {
                                ty: Type::Fn {
                                    params: params.clone(),
                                    ret: tl.ret.clone().map(Box::new),
                                    effect_bound: None,
                                    return_view_provenance: None,
                                    param_contract: None,
                                    call_metadata: None,
                                },
                                kind: TExprKind::Lambda(Box::new(tl)),
                            };
                        }
                        // A function parameter is already a checked Jet callable,
                        // but collection helpers lend each item as `&T`. Adapt
                        // that local callable to the helper's host-borrow shape,
                        // just as the literal-lambda path does above.
                        if method == "map"
                            && matches!(&a.expr, Expr::Ident(_, _))
                            && params.is_some()
                        {
                            let callable = lower_expr(&a.expr, cx, env);
                            return TExpr {
                                ty: callable.ty.clone(),
                                kind: TExprKind::HostBorrowCallback {
                                    callable: Box::new(callable),
                                    params: params.cloned().unwrap_or_default(),
                                },
                            };
                        }
                        if method.starts_with("para_") && !(method == "para_map" && index != 0) {
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
                let callback_arg = if matches!(method, "reduce" | "fold" | "scan" | "para_fold") {
                    args.last()
                } else {
                    args.first()
                };
                let callback_targ = if matches!(method, "reduce" | "fold" | "scan" | "para_fold") {
                    targs.last()
                } else {
                    targs.first()
                };
                let callback_needs_fn_mut = match callback_arg.map(|arg| &arg.expr) {
                    Some(Expr::Lambda(lambda)) => Some(lambda.meta.needs_fn_mut),
                    Some(_)
                        if callback_targ.is_some_and(|arg| matches!(&arg.ty, Type::Fn { .. })) =>
                    {
                        Some(false)
                    }
                    _ => None,
                };
                let callback_param_count = callback_targ
                    .and_then(|arg| match &arg.ty {
                        Type::Fn { params, .. } => Some(params.len()),
                        _ => None,
                    })
                    .or_else(|| callback_params.as_ref().map(Vec::len));
                let fallible_callback = callback_targ.is_some_and(|arg| {
                    matches!(
                        &arg.ty,
                        Type::Fn {
                            ret: Some(ret), ..
                        } if matches!(ret.as_ref(), Type::Result { .. })
                    )
                }) || (matches!(method, "map" | "filter")
                    && matches!(&source_result_ty, Type::Result { .. }));
                let Some(op) = resolve_closure_op(
                    &recv_ty,
                    method,
                    args,
                    cx,
                    fallible_callback,
                    callback_needs_fn_mut,
                    callback_param_count,
                ) else {
                    return invariant_method_expr(
                        method_span,
                        "closure method without a checked operation",
                    );
                };
                let callback_uses_effective_carrier = matches!(
                    &op,
                    TClosureOp::TryMap
                        | TClosureOp::TryFilter
                        | TClosureOp::TrySortBy
                        | TClosureOp::TrySortByDesc
                        | TClosureOp::EachRef
                );
                if callback_uses_effective_carrier {
                    if let Some(callback) = lower_named_collection_callback(
                        &args[0].expr,
                        cx,
                        env,
                        true,
                        callback_params.as_deref(),
                    ) {
                        if let Some(first) = targs.first_mut() {
                            *first = callback;
                        }
                    }
                }
                let callback_error = targs.first().and_then(|callback| match &callback.ty {
                    Type::Fn { ret: Some(ret), .. } => match ret.as_ref() {
                        Type::Result { err, .. } => Some((**err).clone()),
                        _ => None,
                    },
                    _ => None,
                });
                // A fallible callback selects a `Try*` collection operation.
                // Its host returns the enclosing Result carrier even when sema
                // keeps the source-visible success type on the method call.
                // Preserve that ABI in TIR; otherwise a generic `map` binds
                // the Result handle as if it were the returned list.
                let result_ty = if matches!(
                    &op,
                    TClosureOp::TryMap
                        | TClosureOp::TryFilter
                        | TClosureOp::TrySortBy
                        | TClosureOp::TrySortByDesc
                        | TClosureOp::EachRef
                ) && !matches!(&source_result_ty, Type::Result { .. })
                {
                    callback_error
                        .map(|err| Type::Result {
                            ok: Box::new(source_result_ty.clone()),
                            err: Box::new(err),
                        })
                        .unwrap_or(source_result_ty)
                } else {
                    source_result_ty
                };
                let lazy_or_view_receiver = matches!(
                    &recv_ty,
                    Type::Apply { name, .. }
                        if name == crate::Syntax::TYPE_ITER
                            || matches!(
                                name.as_str(),
                                "View" | "ViewMut" | "ComputeViewMut"
                            )
                );
                if matches!(op, TClosureOp::Map | TClosureOp::MapMut) && !lazy_or_view_receiver {
                    // The eager list helper owns a cloned receiver. Publish the
                    // resulting generic `T: Clone` requirement with the TIR
                    // function instead of letting rustc discover it downstream.
                    env.note_clone(&recv_ty);
                }
                return TExpr {
                    ty: result_ty,
                    kind: TExprKind::ClosureMethod {
                        recv: Box::new(recv_t),
                        op,
                        args: targs,
                    },
                };
            });
        }
    }
    // D-NUMWIDEN-CROSS1=E / card #1662: sema owns the checked-crossing
    // decision and records it in `Expr::MethodCall::checked_widen` (replaces
    // the retired `\0numeric.checked_widen` fake-`recv_type` marker).
    // Lowering only records adapter facts.
    if checked_widen && args.len() == 1 {
        return in_own_frame(|| {
            let source = lower_expr(&args[0].expr, cx, env);
            let source_signed = !matches!(source.ty, Type::IntN { signed: false, .. });
            let target = resolved_ret.cloned().unwrap_or(Type::Float);
            if matches!(&target, Type::IntN { .. }) && matches!(&source.ty, Type::Int) {
                let conversion = resolve_numeric_conversion_op(&target.name(), "Int")
                    .expect("sema admitted a fixed-width Int conversion");
                let TNumericOp::TryFrom {
                    host_kind,
                    dst_rust,
                    dst_spelling,
                } = conversion
                else {
                    unreachable!("fixed-width Int conversion must be checked");
                };
                return TExpr {
                    ty: target,
                    kind: TExprKind::NumericMethod {
                        recv: Box::new(source),
                        op: TNumericOp::CheckedIntToFixed {
                            host_kind,
                            dst_rust,
                            dst_spelling,
                            line: crate::Diagnostics::span_line_col(&cx.src, method_span.start).0
                                as u32,
                        },
                    },
                };
            }
            return TExpr {
                ty: target.clone(),
                kind: TExprKind::NumericMethod {
                    recv: Box::new(source),
                    op: TNumericOp::CheckedIntToFloat {
                        source_signed,
                        target_f32: target == Type::Float32,
                        line: crate::Diagnostics::span_line_col(&cx.src, method_span.start).0
                            as u32,
                    },
                },
            };
        });
    }

    if recv_type.as_deref() == Some("Int") && method == "to_radix" && args.len() == 1 {
        return in_own_frame(|| {
            let Some(result_ty) = resolved_ret.cloned() else {
                return invariant_method_expr(
                    method_span,
                    "integer radix conversion without a resolved return type",
                );
            };
            let line = crate::Diagnostics::span_line_col(&cx.src, method_span.start).0 as u32;
            TExpr {
                ty: result_ty,
                kind: TExprKind::BuiltinMethod {
                    recv: Box::new(lower_expr(receiver, cx, env)),
                    op: TBuiltinOp::IntToRadix { line },
                    args: vec![lower_expr(&args[0].expr, cx, env)],
                },
            }
        });
    }

    // c109 Phase 12 / D-GO127-STDLIB1=A: numeric queries and exact-Int
    // Euclidean methods. The gate proved `recv_type == Some(<numeric name>)`
    // plus a covered operation. Resolve the operation HERE into a total
    // `TNumericOp`, so emit makes no decision (I3).
    // The result type comes from
    // `numeric_method_return` (the sema table), keyed on the receiver type recovered
    // from `recv_type` (the total width source — `src = recv_type.or_else(rty.name())`
    // on the AST side, where `recv_type` is always `Some` for these).
    if let Some(numeric_name) = recv_type {
        if let Some(recv_ty) = crate::AST::numeric_type_from_name(numeric_name) {
            if matches!(&recv_ty, Type::Int | Type::IntN { .. }) {
                if let Some((prefix, op, _)) =
                    crate::Collections::numeric_overflow_method(method, args.len())
                {
                    return in_own_frame(|| {
                        let lhs = lower_expr(receiver, cx, env);
                        let rhs = lower_expr(&args[0].expr, cx, env);
                        let Some(result_ty) = resolved_ret.cloned() else {
                            return invariant_method_expr(
                                method_span,
                                "numeric overflow operation without a resolved return type",
                            );
                        };
                        let line =
                            crate::Diagnostics::span_line_col(&cx.src, method_span.start).0 as u32;
                        return TExpr {
                            ty: result_ty,
                            kind: TExprKind::OverflowOpt {
                                prefix: prefix.to_string(),
                                op,
                                line,
                                policy: None,
                                lhs: Box::new(lhs),
                                rhs: Box::new(rhs),
                            },
                        };
                    });
                }
            }
            let line = crate::Diagnostics::span_line_col(&cx.src, method_span.start).0 as u32;
            let resolved_op = resolve_numeric_op(method, numeric_name, line);
            if let Some(op) = resolved_op {
                return in_own_frame(|| {
                    let recv_t = lowered_receiver
                        .borrow_mut()
                        .take();
                    let mut recv_t = recv_t.unwrap_or_else(|| lower_expr(receiver, cx, env));
                    // Sema's width is authoritative — Call/OrFallback lowering can
                    // fall back to Unit/Int and would silently widen bit queries.
                    recv_t.ty = recv_ty.clone();
                    let Some(result_ty) = resolved_ret.cloned() else {
                        return invariant_method_expr(
                            method_span,
                            "numeric method without a resolved return type",
                        );
                    };
                    if matches!(
                        op,
                        TNumericOp::EuclideanDiv { .. } | TNumericOp::EuclideanRem { .. }
                    ) {
                        let arg = lower_expr(&args[0].expr, cx, env);
                        return TExpr {
                            ty: result_ty,
                            kind: TExprKind::NumericBinaryMethod {
                                recv: Box::new(recv_t),
                                op,
                                arg: Box::new(arg),
                            },
                        };
                    }
                    TExpr {
                        ty: result_ty,
                        kind: TExprKind::NumericMethod {
                            recv: Box::new(recv_t),
                            op,
                        },
                    }
                });
            }
        }
    }
    // Core enum equality is represented by the shared Prelude's native
    // `PartialEq` value operation. Sema has already proved the Equatable
    // contract; preserve it as the existing typed equality node so emitters do
    // not rediscover the representation.
    if method == "equal"
        && args.len() == 1
        && recv_type
            .as_deref()
            .is_some_and(|name| core_enum_equal_type(name.rsplit('.').next().unwrap_or(name)))
    {
        return in_own_frame(|| {
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
        });
    }
    // c109 Phase 25: HTTPRouter/HTTPMux route registration
    // `get/post/put/delete/patch/head/options(path, handler)` (D-ROUTE1=A).
    // Sema has already checked the endpoint contract; preserve the handler's
    // source parameter names so MIR can build the typed request adapter.
    if matches!(recv_type.as_deref(), Some("HTTPRouter") | Some("HTTPMux"))
        && matches!(
            method,
            "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
        )
        && args.len() == 2
    {
        return in_own_frame(|| {
            let verb = match method {
                "get" => jet_foundation::MIR::MirHttpMethod::Get,
                "post" => jet_foundation::MIR::MirHttpMethod::Post,
                "put" => jet_foundation::MIR::MirHttpMethod::Put,
                "delete" => jet_foundation::MIR::MirHttpMethod::Delete,
                "patch" => jet_foundation::MIR::MirHttpMethod::Patch,
                "head" => jet_foundation::MIR::MirHttpMethod::Head,
                "options" => jet_foundation::MIR::MirHttpMethod::Options,
                _ => unreachable!(),
            };
            let Some(handler_param_names) = route_handler_param_names(&args[1].expr, cx) else {
                return invariant_method_expr(
                    method_span,
                    "HTTP route registration without handler parameter metadata",
                );
            };
            let recv_t = lower_expr(receiver, cx, env);
            let path_t = lower_expr(&args[0].expr, cx, env);
            // Sema has checked this lambda at the HTTP Send + Sync boundary;
            // lower the route callback with owned parameters explicitly rather
            // than inferring an HTTP representation from its syntax.
            let lowered_handler = match route_strip_parens(&args[1].expr) {
                Expr::Lambda(lam) => {
                    let lowered = lower_lambda_expecting_value(lam, cx, env, &[]);
                    TExpr {
                        ty: Type::Fn {
                            params: lowered.param_types.clone(),
                            ret: lowered.ret.clone().map(Box::new),
                            effect_bound: None,
                            return_view_provenance: None,
                            param_contract: None,
                            call_metadata: None,
                        },
                        kind: TExprKind::Lambda(Box::new(lowered)),
                    }
                }
                _ => lower_expr(&args[1].expr, cx, env),
            };
            let handler = Box::new(TExpr {
                ty: lowered_handler.ty.clone(),
                kind: TExprKind::FnValue {
                    kind: TFnValueKind::Send {
                        value: Box::new(lowered_handler),
                    },
                },
            });
            let line = crate::Diagnostics::span_line_col(&cx.src, method_span.start).0;
            let is_mux = recv_type.as_deref() == Some("HTTPMux");
            let Some(contract_json) =
                route_handler_contract(verb, &args[0].expr, &args[1].expr, cx, line, is_mux)
            else {
                return invariant_method_expr(
                    method_span,
                    "HTTP route registration without a checked endpoint contract",
                );
            };
            return TExpr {
                ty: unit_type(),
                kind: TExprKind::HandleMethod {
                    recv: Box::new(recv_t),
                    op: THandleOp::HTTPRouterRegister {
                        verb,
                        handler,
                        file: cx.file.clone(),
                        line,
                        handler_param_names,
                        contract_json,
                    },
                    args: vec![path_t],
                },
            };
        });
    }
    // D-DECIMAL1 / D-NUMTYPE1: instance methods on precise numerics → the same
    // `PreciseBuiltin` nodes as constructors/binops. Fragment lowers (empty Cx)
    // never see method_sigs, so this
    // must not fall through to the user-method Todo path.
    if let Some(handle) = recv_type {
        if (handle == Syntax::TYPE_DECIMAL || handle == Syntax::TYPE_FRACTION)
            && !cx.type_names.contains(handle)
        {
            let known = matches!(
                (handle.as_str(), method, args.len()),
                ("Decimal", "add" | "sub" | "mul" | "div" | "equal", 1)
                    | ("Decimal", "round" | "floor" | "ceil", 0)
                    | ("Decimal", "to_string", 0)
                    | ("Fraction", "add" | "sub" | "mul" | "div" | "equal", 1)
                    | (
                        "Fraction",
                        "numerator" | "denominator" | "to_string" | "to_float" | "is_zero",
                        0
                    )
            );
            if known {
                return in_own_frame(|| {
                    let recv_t = lower_expr(receiver, cx, env);
                    let mut value_args = vec![recv_t];
                    value_args.extend(args.iter().map(|a| lower_expr(&a.expr, cx, env)));
                    let ty = match method {
                        "to_string" => Type::String,
                        "div" => Type::Named(Syntax::TYPE_FRACTION.to_string()),
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
                });
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
        if crate::Sema::is_geometry_type(handle)
            && !cx.type_names.contains(handle)
            && matches!(
                (method, args.len()),
                ("add" | "sub" | "then" | "ray", 1)
                    | ("inverse", 0)
                    | ("point", 1)
                    | ("point_at_depth", 2)
            )
        {
            return in_own_frame(|| TExpr {
                ty: resolved_ret.cloned().unwrap_or_else(unit_type),
                kind: TExprKind::HandleMethod {
                    recv: Box::new(lower_expr(receiver, cx, env)),
                    op: THandleOp::MathMethod {
                        type_name: handle.to_string(),
                        method: method.to_string(),
                        reduce_op: None,
                    },
                    args: args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect(),
                },
            });
        }
    }

    if let Some(handle) = recv_type {
        if crate::Sema::is_math_type(handle) && !cx.type_names.contains(handle) {
            let is_reduce = method == "reduce" && crate::Sema::is_simd_lane_type(handle);
            if is_reduce || crate::Sema::math_method_return(handle, method, args.len()).is_some() {
                return in_own_frame(|| {
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
                });
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
                    return lower_cursor_take_pattern(
                        receiver,
                        parts,
                        cx,
                        env,
                        lowered_receiver.borrow_mut().take(),
                    );
                }
            }
        }
    }
    // D-BINPAT1 (card #506 follow-up): `reader.take_pattern([U8]{"…"})` — the
    // byte-mode sibling, same reasoning. The parser already committed to
    // `Expr::BinMatchLit` for this argument (sema rejects any other shape).
    if let Some(handle) = recv_type {
        if handle == "Reader" && method == "take_pattern" {
            if let Some(arg) = args.first() {
                if let Expr::BinMatchLit(parts, _) = &arg.expr {
                    return lower_reader_take_pattern(
                        receiver,
                        parts,
                        cx,
                        env,
                        lowered_receiver.borrow_mut().take(),
                    );
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
                return in_own_frame(|| {
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
                });
            }
        }
    }
    if let Some(handle) = recv_type {
        if handle == "__SerdeEncode__" && method == "encode" && args.is_empty() {
            let recv = lower_expr(receiver, cx, env);
            return lower_serde_encode_node(recv, cx);
        }
        if handle == "__Debug__" && method == "debug" && args.is_empty() {
            return lower_debug_text(lower_expr(receiver, cx, env));
        }
        if handle == Syntax::TYPE_DATA
            && method == Syntax::METHOD_DATATREE_DECODE
            && args.is_empty()
        {
            // Compiler-generated Codable bodies are lowered before a sema pass
            // writes `resolved_ret` onto each synthetic call. Their explicit
            // target type argument is the same checked fact, so do not fall
            // through to the user-method emitter (`__jet_decode`).
            let target = resolved_ret
                .and_then(|ret| match ret {
                    Type::Result { ok, .. } => Some((**ok).clone()),
                    _ => None,
                })
                .or_else(|| type_args.first().cloned());
            if let Some(target) = target {
                return in_own_frame(|| {
                    lower_datatree_decode_node(
                        lower_expr(receiver, cx, env),
                        target,
                        resolved_ret,
                        method_span,
                        cx,
                    )
                });
            }
        }
        if handle == "JobQueue"
            && matches!(
                method,
                "enqueue"
                    | "delay"
                    | "receipt"
                    | "inspect"
                    | "events"
                    | "claim"
                    | "heartbeat"
                    | "acknowledge"
                    | "fail"
                    | "cancel"
                    | "dead_letter"
                    | "recover_expired"
                    | "status"
                    | "pause"
                    | "resume"
                    | "wait"
                    | "prune"
            )
        {
            return in_own_frame(|| {
                let Some(ty) = resolved_ret.cloned() else {
                    return invariant_method_expr(
                        method_span,
                        format!("JobQueue.{method} has no resolved return type"),
                    );
                };
                let receiver = lower_expr_as_mut_place(receiver, cx, env);
                let receiver_ty = receiver.ty.clone();
                let mut lowered_args = Vec::with_capacity(args.len() + 1);
                lowered_args.push(TExpr {
                    ty: receiver_ty,
                    kind: TExprKind::Borrow {
                        place: Box::new(receiver),
                        mutable: true,
                    },
                });
                match method {
                    "enqueue" | "delay" => {
                        let Some(job_arg) = args.first() else {
                            return invariant_method_expr(
                                method_span,
                                format!("JobQueue.{method} has no checked job identity"),
                            );
                        };
                        let Some(job_name) = route_static_string(&job_arg.expr, cx) else {
                            return invariant_method_expr(
                                job_arg.expr.span(),
                                format!("JobQueue.{method} job identity is not a static #Job name"),
                            );
                        };
                        lowered_args.push(TExpr {
                            ty: Type::String,
                            kind: TExprKind::StrLit(vec![TStrPart::Lit(job_name)]),
                        });
                        let Some(payload_arg) = args.get(1) else {
                            return invariant_method_expr(
                                method_span,
                                format!("JobQueue.{method} has no checked payload argument"),
                            );
                        };
                        lowered_args.push(lower_expr(&payload_arg.expr, cx, env));
                        if method == "enqueue" {
                            if let Some(key_arg) = args.get(2) {
                                lowered_args.push(lower_expr(&key_arg.expr, cx, env));
                            } else {
                                lowered_args.push(TExpr {
                                    ty: Type::Option(Box::new(Type::String)),
                                    kind: TExprKind::Absent,
                                });
                            }
                        } else if let Some(duration_arg) = args.get(2) {
                            lowered_args.push(lower_expr(&duration_arg.expr, cx, env));
                        } else {
                            return invariant_method_expr(
                                method_span,
                                "JobQueue.delay has no checked duration argument",
                            );
                        }
                    }
                    "cancel" | "dead_letter" if args.len() == 2 => {
                        lowered_args.extend(args.iter().map(|arg| lower_expr(&arg.expr, cx, env)));
                        lowered_args.push(TExpr {
                            ty: Type::Option(Box::new(Type::String)),
                            kind: TExprKind::Absent,
                        });
                    }
                    _ => {
                        lowered_args.extend(args.iter().map(|arg| lower_expr(&arg.expr, cx, env)));
                    }
                }
                let widen_to_vec = vec![false; lowered_args.len()];
                let record =
                    match checked_core_record("core.jobs", method, lowered_args.len(), method_span)
                    {
                        Ok(record) => record,
                        Err(expr) => return expr,
                    };
                TExpr {
                    ty: ty.clone(),
                    kind: TExprKind::CoreCall {
                        record,
                        args: lowered_args,
                        source_span: method_span,
                        type_args: Vec::new(),
                        widen_to_vec,
                        data_plan: None,
                        fallibility: TFailureCarrier::from_checked_type(&ty),
                    },
                }
            });
        }
        if let Some((core_method, _mutates)) = service_method_route(handle, method) {
            return in_own_frame(|| {
                let mut lowered_args = Vec::with_capacity(args.len() + 2);
                lowered_args.push(lower_expr(receiver, cx, env));
                if handle == "ServiceTree" && method == "worker" && args.len() == 3 {
                    lowered_args.push(lower_expr(&args[0].expr, cx, env));
                    let handler = match &args[1].expr {
                        Expr::Ident(name, _) => name.clone(),
                        _ => String::new(),
                    };
                    lowered_args.push(lower_expr(&args[1].expr, cx, env));
                    lowered_args.push(TExpr {
                        ty: Type::String,
                        kind: TExprKind::StrLit(vec![TStrPart::Lit(handler)]),
                    });
                    lowered_args.push(lower_expr(&args[2].expr, cx, env));
                } else {
                    lowered_args.extend(args.iter().map(|arg| lower_expr(&arg.expr, cx, env)));
                }
                let widen_to_vec = vec![false; lowered_args.len()];
                let Some(ty) = resolved_ret.cloned() else {
                    return invariant_method_expr(
                        method_span,
                        format!(
                            "service Core call `core.service.{core_method}` has no resolved return type",
                        ),
                    );
                };
                let record = match checked_core_record(
                    "core.service",
                    core_method,
                    lowered_args.len(),
                    method_span,
                ) {
                    Ok(record) => record,
                    Err(expr) => return expr,
                };
                TExpr {
                    ty: ty.clone(),
                    kind: TExprKind::CoreCall {
                        record,
                        args: lowered_args,
                        source_span: method_span,
                        type_args: Vec::new(),
                        widen_to_vec,
                        data_plan: None,
                        fallibility: TFailureCarrier::from_checked_type(&ty),
                    },
                }
            });
        }
        if handle == "Plugin" && !matches!(method, "call" | "call_int" | "call_bool" | "call_text")
        {
            return in_own_frame(|| {
                let recv_t = lowered_receiver.borrow_mut().take();
                let recv_t =
                    recv_t.unwrap_or_else(|| crate::Codegen::TIR::lower_expr(receiver, cx, env));
                let targs = args
                    .iter()
                    .map(|arg| lower_expr(&arg.expr, cx, env))
                    .collect::<Vec<_>>();
                let ty = resolved_ret.cloned().unwrap_or_else(unit_type);
                let result_ty = match &ty {
                    Type::Result { ok, .. } => ok.as_ref().clone(),
                    other => other.clone(),
                };
                let Some(signature) = plugin_signature_descriptor(cx, &targs, &result_ty) else {
                    return invariant_method_expr(
                        method_span,
                        format!("plugin export `{method}` has no Component descriptor"),
                    );
                };
                TExpr {
                    ty,
                    kind: TExprKind::HandleMethod {
                        recv: Box::new(recv_t),
                        op: THandleOp::PluginInvoke {
                            export_name: method.to_string(),
                            signature,
                        },
                        args: targs,
                    },
                }
            });
        }
        if let Some(mut op) = handle_method_op(handle, method, args.len()) {
            return in_own_frame(|| {
                if let THandleOp::DurationIn { unit } = &mut op {
                    *unit = args.first().and_then(|arg| match &arg.expr {
                        Expr::EnumLit { variant, .. } => Syntax::DURATION_UNITS
                            .iter()
                            .copied()
                            .find(|candidate| *candidate == variant),
                        _ => None,
                    });
                }
                let recv_t = if let Some(game) = lower_game_handle_receiver(receiver, cx, env) {
                    game
                } else if matches!(
                    op,
                    THandleOp::FileReaderReadLine
                        | THandleOp::FileWriterWriteLine
                        | THandleOp::FileWriterFlush
                ) {
                    lower_expr_as_mut_place(receiver, cx, env)
                } else {
                    lower_expr(receiver, cx, env)
                };
                // D-GAME*: stash spawn-lambda for resident JIT `game.run` callbacks.
                // Keep the lowered lambda arg so AOT emit still receives it.
                if matches!(op, THandleOp::GameSceneOnFrame { .. }) {
                    if let Some(Expr::Lambda(lam)) = args.first().map(|a| &a.expr) {
                        if let THandleOp::GameSceneOnFrame {
                            schedule,
                            derivation,
                        } = &mut op
                        {
                            *schedule = lam.meta.frame_schedule.clone();
                            *derivation = lam.meta.frame_schedule_derivation.clone();
                        }
                        jit_spawn_site_with(
                            lam,
                            cx,
                            env,
                            |lam: &crate::AST::Lambda, cx: &Cx, env: &LowerEnv| {
                                let mut jit_lambda = lower_spawn_lambda_for_jit_expecting(
                                    lam,
                                    cx,
                                    env,
                                    &[Type::Named("GameFrame".to_string())],
                                );
                                for (_, ty) in &mut jit_lambda.params {
                                    *ty = Type::Named("GameFrame".to_string());
                                }
                                jit_lambda
                            },
                        );
                    }
                }
                let targs: Vec<TExpr> = args
                    .iter()
                    .enumerate()
                    .map(|(i, a)| {
                        if matches!(op, THandleOp::StreamWithEventTime | THandleOp::StreamKeyBy)
                            && i == 0
                        {
                            let params = match &recv_t.ty {
                                Type::Apply { name, args }
                                    if (name == "Stream" || name == "StreamEventTime")
                                        && args.len() == 1 =>
                                {
                                    vec![args[0].clone()]
                                }
                                _ => Vec::new(),
                            };
                            if !params.is_empty() {
                                let wrap_send = |callback: TExpr| {
                                    let ty = callback.ty.clone();
                                    TExpr {
                                        ty,
                                        kind: TExprKind::FnValue {
                                            kind: TFnValueKind::Send {
                                                value: Box::new(callback),
                                            },
                                        },
                                    }
                                };
                                if let Some(callback) = lower_named_collection_callback(
                                    &a.expr,
                                    cx,
                                    env,
                                    false,
                                    Some(params.as_slice()),
                                ) {
                                    return wrap_send(callback);
                                }
                                if let Expr::Lambda(lam) = &a.expr {
                                    let mut lowered = lower_lambda_expecting_host_borrow(
                                        lam,
                                        cx,
                                        env,
                                        params.as_slice(),
                                        false,
                                    );
                                    lowered.boxed = true;
                                    lowered.rc = false;
                                    lowered.arc = false;
                                    let callable_ty = Type::Fn {
                                        params: params.clone(),
                                        ret: lowered.ret.clone().map(Box::new),
                                        effect_bound: None,
                                        return_view_provenance: None,
                                        param_contract: None,
                                        call_metadata: None,
                                    };
                                    let callable = TExpr {
                                        ty: callable_ty.clone(),
                                        kind: TExprKind::Lambda(Box::new(lowered)),
                                    };
                                    return wrap_send(TExpr {
                                        ty: callable_ty,
                                        kind: TExprKind::HostBorrowCallback {
                                            callable: Box::new(callable),
                                            params,
                                        },
                                    });
                                }
                            }
                        }
                        if handle == "Regex" && method == "replace_all_with" && i == 1 {
                            if let Expr::Lambda(lam) = &a.expr {
                                let params = vec![Type::Named("RegexMatch".to_string())];
                                return TExpr {
                                    ty: Type::Fn {
                                        params: params.clone(),
                                        ret: Some(Box::new(Type::String)),
                                        effect_bound: None,
                                        return_view_provenance: None,
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
                        if matches!(op, THandleOp::GameSceneOnFrame { .. }) && i == 0 {
                            if let Expr::Lambda(lam) = &a.expr {
                                let params = vec![Type::Named("GameFrame".to_string())];
                                let mut lowered =
                                    lower_lambda_expecting_value(lam, cx, env, &params);
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
                        // `Rng.shuffle(&list)` must keep a writable place for MirBridge
                        // write-back (CallArg Write + Ident is not Expr::Borrow).
                        if handle == "Rng" && method == "shuffle" && i == 0 {
                            return lower_expr_as_mut_place(&a.expr, cx, env);
                        }
                        lower_expr(&a.expr, cx, env)
                    })
                    .collect();
                if matches!(op, THandleOp::StreamWithEventTime)
                    && targs.first().is_some_and(|arg| {
                        matches!(
                            &arg.ty,
                            Type::Fn {
                                ret: Some(ret), ..
                            } if matches!(
                                ret.as_ref(),
                                Type::Named(name) if name == "DateTime"
                            )
                        )
                    })
                {
                    op = THandleOp::StreamWithEventTimeNs;
                }
                if matches!(
                    &op,
                    THandleOp::DBQuery { .. }
                        | THandleOp::DBQueryOne { .. }
                        | THandleOp::DBExecute { .. }
                ) {
                    let metadata = db_query_metadata(method, method_span, cx, targs.first());
                    match &mut op {
                        THandleOp::DBQuery { metadata: slot }
                        | THandleOp::DBQueryOne { metadata: slot }
                        | THandleOp::DBExecute { metadata: slot } => *slot = Some(metadata),
                        _ => unreachable!("database metadata only applies to database queries"),
                    }
                }
                // c109 Phase 19: allocator result types are checked semantic facts.
                // Never rebuild an allocator view/result from lowered arguments when
                // sema did not attach the return row.
                let ty = if matches!(
                    &op,
                    THandleOp::AllocAlloc | THandleOp::AllocTryAlloc | THandleOp::AllocReset
                ) {
                    let Some(ty) = resolved_ret.cloned() else {
                        return invariant_method_expr(
                            method_span,
                            "allocator method without a resolved return type",
                        );
                    };
                    ty
                } else {
                    in_own_frame(|| match &op {
                        THandleOp::DataStreamNext => {
                            resolved_ret.cloned().unwrap_or_else(|| Type::Result {
                                ok: Box::new(Type::Option(Box::new(Type::Named(
                                    "Unknown".to_string(),
                                )))),
                                err: Box::new(Type::Named("DataError".to_string())),
                            })
                        }
                        _ => handle_method_return_ty(
                            handle,
                            method,
                            args.len(),
                            &recv_t.ty,
                            resolved_ret,
                        ),
                    })
                };
                return TExpr {
                    ty,
                    kind: TExprKind::HandleMethod {
                        recv: Box::new(recv_t),
                        op,
                        args: targs,
                    },
                };
            });
        }
    }
    // D-ENCSTREAM-SURFACE1=A: qualified shared type constructor.
    if method == "safe" && args.is_empty() {
        // D-APILABEL1=A: a Core parameter default is synthesized as a bare
        // `EncodingLimits.safe()`, because the caller that skipped the argument
        // need not have imported `core.encoding` to name an alias for it.
        if let Expr::Ident(type_name, _) = receiver {
            if type_name == "EncodingLimits" && !cx.struct_fields.contains_key(type_name) {
                return in_own_frame(|| {
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
                });
            }
        }
        if let Expr::Ident(type_name, _) = receiver {
            if type_name == "Limits"
                && cx
                    .core_imports
                    .values()
                    .any(|module| module == "core.email")
            {
                return in_own_frame(|| TExpr {
                    ty: Type::Named("Limits".to_string()),
                    kind: TExprKind::StaticCall {
                        owner: TStaticOwner::Prelude {
                            rooted: false,
                            path: "core.email".to_string(),
                            generics: Vec::new(),
                        },
                        owner_type: None,
                        method: TMethodRef::bare("limits_safe"),
                        type_args: Vec::new(),
                        args: vec![],
                    },
                });
            }
        }
        if let Expr::Field(base, leaf, _) = receiver {
            if leaf == "EncodingLimits"
                && core_module_path_from_receiver(base, cx, env).as_deref() == Some("core.encoding")
            {
                return in_own_frame(|| {
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
                });
            }
            if leaf == "DataLimits"
                && core_module_path_from_receiver(base, cx, env).as_deref() == Some("core.data")
            {
                return in_own_frame(|| {
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
                });
            }
            if leaf == "DataLimits"
                && core_module_path_from_receiver(base, cx, env).as_deref() == Some("core.data")
            {
                return in_own_frame(|| {
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
                });
            }
            if leaf == "CBOROptions"
                && core_module_path_from_receiver(base, cx, env).as_deref()
                    == Some("core.encoding.cbor")
            {
                return in_own_frame(|| {
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
                });
            }
            if matches!(
                leaf.as_str(),
                "XMLLimits" | "XMLParseOptions" | "XMLRenderOptions"
            ) && core_module_path_from_receiver(base, cx, env).as_deref()
                == Some("core.encoding.xml")
            {
                return in_own_frame(|| {
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
                });
            }
            if leaf == "Limits"
                && core_module_path_from_receiver(base, cx, env).as_deref() == Some("core.email")
            {
                return in_own_frame(|| {
                    return TExpr {
                        ty: Type::Named("Limits".to_string()),
                        kind: TExprKind::StaticCall {
                            owner: TStaticOwner::Prelude {
                                rooted: false,
                                path: "core.email".to_string(),
                                generics: Vec::new(),
                            },
                            owner_type: None,
                            method: TMethodRef::bare("limits_safe"),
                            type_args: Vec::new(),
                            args: vec![],
                        },
                    };
                });
            }
        }
    }
    // D-AUTHORITY-NAME1=A / D-AUTHORITY-WORD2=E: keep the two authority
    // operations on the shared Prelude route. This is ordinary data, not a
    // user method: the implementation has no `Authority::with` MIR function.
    // Comptime constants have no local slot, so recover the same fact from
    // their evaluated carrier before the static-call fallback mistakes a
    // value name for a type name.
    let comptime_authority_name = match receiver {
        Expr::Ident(name, _) | Expr::ComptimeName { name, .. } => {
            let marked_name = if Syntax::is_comptime_name(name) {
                name.clone()
            } else {
                format!("{}{}", Syntax::COMPTIME_MARK, name)
            };
            let is_authority = |candidate: &str| {
                matches!(
                    cx.const_values.get(candidate),
                    Some(crate::Comptime::CtValue::Struct { type_name, .. })
                        if type_name == Syntax::TYPE_AUTHORITY
                )
            };
            if is_authority(name) {
                Some(name.clone())
            } else if is_authority(&marked_name) {
                Some(marked_name)
            } else {
                None
            }
        }
        _ => None,
    };
    if (recv_type.as_deref() == Some(Syntax::TYPE_AUTHORITY) || comptime_authority_name.is_some())
        && matches!(method, "with" | "without")
        && args.len() == 1
    {
        return in_own_frame(|| {
            let recv = if let Some(name) = comptime_authority_name.as_ref() {
                let comptime_receiver = Expr::ComptimeName {
                    name: name.clone(),
                    span: receiver.span(),
                    value: None,
                };
                lower_expr(&comptime_receiver, cx, env)
            } else {
                lower_expr(receiver, cx, env)
            };
            let mut targs = args
                .iter()
                .map(|arg| lower_one_call_arg(arg, None, env, cx))
                .collect::<Vec<_>>();
            let receiver = TCallArg {
                value: recv,
                template_items: None,
                borrow: true,
                mut_borrow: false,
                clone: false,
                arc_clone: false,
                fn_coerce: None,
                widen_to_vec: false,
                widen_to_union: None,
                box_as_trait: None,
            };
            for arg in &mut targs {
                arg.borrow = true;
                arg.mut_borrow = false;
            }
            let mut call_args = Vec::with_capacity(targs.len() + 1);
            call_args.push(receiver);
            call_args.extend(targs);
            TExpr {
                ty: resolved_ret
                    .cloned()
                    .unwrap_or_else(|| Type::Named(Syntax::TYPE_AUTHORITY.to_string())),
                kind: TExprKind::StaticCall {
                    owner: rooted_owner("JetAuthority"),
                    owner_type: None,
                    method: TMethodRef::bare(method),
                    type_args: type_args.to_vec(),
                    args: call_args,
                },
            }
        });
    }
    // c109 Phase 7: a STATIC method call `Type.make(args)`. The gate
    // (`static_method_call_in_subset`) proved the receiver is a covered type-name
    // ident and `method` is a registered static method. Mirror the AST path
    // (Expression.rs ~L1644): `__jet_<Type>::__jet_<method>(args)`.
    if recv_type.is_none() {
        if let Some(type_name) = static_call_type_name_lower(receiver, env) {
                if type_name == "Limits" && method == "safe" && args.is_empty() {
                    return TExpr {
                        ty: resolved_ret
                            .cloned()
                            .unwrap_or_else(|| Type::Named("Limits".to_string())),
                        kind: TExprKind::StaticCall {
                            owner: TStaticOwner::Prelude {
                                rooted: false,
                                path: "core.email".to_string(),
                                generics: Vec::new(),
                            },
                            owner_type: None,
                            method: TMethodRef::bare("limits_safe"),
                            type_args: Vec::new(),
                            args: Vec::new(),
                        },
                    };
                }
            return in_own_frame(|| {
                if type_name == Syntax::TYPE_ATOMIC
                    && owner_type_args.len() == 1
                    && args.len() == 1
                    && matches!(method, "new" | "try_new")
                {
                    let inner = owner_type_args[0].clone();
                    return TExpr {
                        ty: resolved_ret.cloned().unwrap_or_else(|| Type::Apply {
                            name: Syntax::TYPE_ATOMIC.to_string(),
                            args: vec![inner.clone()],
                        }),
                        kind: TExprKind::StaticCall {
                            owner: rooted_generic_owner("JetAtomic", vec![TPreludeArg::Jet(inner)]),
                            owner_type: None,
                            method: TMethodRef::bare(method),
                            type_args: Vec::new(),
                            args: vec![lower_one_call_arg(&args[0], None, env, cx)],
                        },
                    };
                }
                // D-SHAPE-PROJECT1=A: `T.merge(flags, settings)` on a `#CLI`
                // struct is the precedence combinator; the row carries `T` so
                // every engine builds the same builder spec from T's rows.
                if method == "merge"
                    && args.len() == 2
                    && cx.type_names.contains(&type_name)
                    && matches!(resolved_ret, Some(Type::Result { ok, .. }) if **ok == Type::Named(type_name.clone()))
                {
                    let Some(ty) = resolved_ret.cloned() else {
                        return invariant_method_expr(
                            method_span,
                            "T.merge has no resolved return type",
                        );
                    };
                    let lowered_args = args
                        .iter()
                        .map(|arg| lower_expr(&arg.expr, cx, env))
                        .collect::<Vec<_>>();
                    let record = match checked_core_record("core.args", "merge", 2, method_span) {
                        Ok(record) => record,
                        Err(expr) => return expr,
                    };
                    return TExpr {
                        ty: ty.clone(),
                        kind: TExprKind::CoreCall {
                            record,
                            args: lowered_args,
                            source_span: method_span,
                            type_args: Vec::new(),
                            widen_to_vec: vec![false, false],
                            data_plan: None,
                            fallibility: TFailureCarrier::from_checked_type(&ty),
                        },
                    };
                }
                // D-FOUND-PLATFORM1=A: contextual `.cmd("key")` is the
                // checked UiShortcut constructor. It lowers through the same
                // shared Prelude type method on every resident/AOT tier.
                if type_name == "UiShortcut" && method == "cmd" && args.len() == 1 {
                    return TExpr {
                        ty: resolved_ret
                            .cloned()
                            .unwrap_or_else(|| Type::Named(type_name.clone())),
                        kind: TExprKind::StaticCall {
                            owner: rooted_owner("JetUiShortcut"),
                            owner_type: None,
                            method: TMethodRef::bare("cmd"),
                            type_args: Vec::new(),
                            args: vec![lower_one_call_arg(&args[0], None, env, cx)],
                        },
                    };
                }
                // D-TEXTHEAD-TYPE1=A: the source-facing constructors are ordinary
                // inherent facades over the ordinary CheckedText implementation.
                // Their Rust names stay bare; only actual Jet methods use the
                // `__jet_` mangle.
                if cx.string_distinct_has_trait_method(&type_name, "check")
                    && cx.string_distinct_has_trait_method(&type_name, "encode_hole")
                    && matches!(method, "from" | "encode_hole")
                {
                    let lowered_args: Vec<_> = args
                        .iter()
                        .map(|argument| lower_one_call_arg(argument, None, env, cx))
                        .collect();
                    let resolved_type_args = if method == "encode_hole" {
                        let sig = cx
                            .method_sigs
                            .get(&(type_name.clone(), method.to_string()))
                            .cloned()
                            .unwrap_or_default();
                        resolved_method_type_args(
                            cx,
                            &type_name,
                            method,
                            &sig,
                            &[],
                            &lowered_args,
                            type_args,
                            resolved_ret,
                        )
                    } else {
                        type_args.to_vec()
                    };
                    let owner = Type::Named(type_name.clone());
                    if !resolved_type_args.is_empty() {
                        cx.jit_method_calls.borrow_mut().insert(
                            crate::Codegen::TIR::generic_method_instance_key(
                                &owner,
                                method,
                                &resolved_type_args,
                            ),
                            (owner.clone(), method.to_string(), resolved_type_args.clone()),
                        );
                    }
                    let method_name = if resolved_type_args.is_empty() {
                        method.to_string()
                    } else {
                        generic_method_instance_leaf(&owner, method, &resolved_type_args)
                    };
                    return TExpr {
                        ty: resolved_ret.cloned().unwrap_or_else(|| {
                            if method == "from" {
                                Type::Result {
                                    ok: Box::new(Type::Named(type_name.clone())),
                                    err: Box::new(Type::String),
                                }
                            } else {
                                Type::String
                            }
                        }),
                        kind: TExprKind::StaticCall {
                            owner: TStaticOwner::User(type_name.clone()),
                            owner_type: Some(Type::Named(type_name)),
                            method: TMethodRef::bare(method_name),
                            type_args: resolved_type_args,
                            args: lowered_args,
                        },
                    };
                }
                // D-AUTHORITY-NAME1=A: the source-facing constructor is an
                // inherent facade over the one rooted Prelude carrier. There
                // is no user `Authority::from_rights` MIR function.
                if type_name == Syntax::TYPE_AUTHORITY
                    && method == "from_rights"
                    && args.len() == 1
                {
                    return TExpr {
                        ty: resolved_ret
                            .cloned()
                            .unwrap_or_else(|| Type::Named(Syntax::TYPE_AUTHORITY.to_string())),
                        kind: TExprKind::StaticCall {
                            owner: rooted_owner("JetAuthority"),
                            owner_type: None,
                            method: TMethodRef::bare("from_rights"),
                            type_args: Vec::new(),
                            args: args
                                .iter()
                                .map(|arg| lower_one_call_arg(arg, None, env, cx))
                                .collect(),
                        },
                    };
                }
                // D-AUTHORITY-NAME1=A: keep construction as a Prelude static call
                // so every engine receives the same named rights carrier.
                if type_name == Syntax::TYPE_AUTHORITY && method == "workspace" && args.is_empty() {
                    return TExpr {
                        ty: resolved_ret
                            .cloned()
                            .unwrap_or_else(|| Type::Named(Syntax::TYPE_AUTHORITY.to_string())),
                        kind: TExprKind::StaticCall {
                            owner: rooted_owner("JetAuthority"),
                            owner_type: None,
                            method: TMethodRef::bare("workspace"),
                            type_args: Vec::new(),
                            args: Vec::new(),
                        },
                    };
                }
                if type_name == "Date" && method == "today" && args.is_empty() {
                    return in_own_frame(|| {
                        let Some(ty) = resolved_ret.cloned() else {
                            return invariant_method_expr(
                                method_span,
                                "Date.today without a resolved return type",
                            );
                        };
                        let record = match checked_core_record("core.time", "today", 0, method_span)
                        {
                            Ok(record) => record,
                            Err(expr) => return expr,
                        };
                        TExpr {
                            ty: ty.clone(),
                            kind: TExprKind::CoreCall {
                                record,
                                args: Vec::new(),
                                source_span: method_span,
                                type_args: Vec::new(),
                                widen_to_vec: Vec::new(),
                                data_plan: None,
                                fallibility: TFailureCarrier::from_checked_type(&ty),
                            },
                        }
                    });
                }
                if type_name == "Path"
                    && method == "home"
                    && args.is_empty()
                    && !cx.type_names.contains("Path")
                {
                    return in_own_frame(|| {
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
                    });
                }
                // D-VALIDATE-DECODE1=B: generated codecs frame a child Result at the
                // one field/index boundary before applying `?`. Keep this as a TIR
                // node so AOT, JIT, and interpreter use one implementation.
                if type_name == "FieldError" && method == "under" && args.len() == 2 {
                    return in_own_frame(|| {
                        let segment = lower_one_call_arg(&args[0], None, env, cx).value;
                        let inner = lower_expr(&args[1].expr, cx, env);
                        return TExpr {
                            ty: resolved_ret.cloned().unwrap_or_else(|| inner.ty.clone()),
                            kind: TExprKind::DecodeUnder {
                                segment: Box::new(segment),
                                inner: Box::new(inner),
                            },
                        };
                    });
                }
                if matches!(
                    type_name.as_str(),
                    "HTTPMethod"
                        | "HTTPStatus"
                        | "HTTPVersion"
                        | "HTTPHeaderName"
                        | "HTTPHeaderValue"
                        | "HTTPHeaders"
                        | "HTTPBody"
                ) {
                    return in_own_frame(|| {
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
                            ty: resolved_ret
                                .cloned()
                                .unwrap_or_else(|| Type::Named(type_name.clone())),
                            kind: TExprKind::StaticCall {
                                owner: rooted_owner(format!("Jet{type_name}")),
                                owner_type: None,
                                method: TMethodRef::bare(method_rust),
                                type_args: Vec::new(),
                                args: args
                                    .iter()
                                    .map(|argument| lower_one_call_arg(argument, None, env, cx))
                                    .collect(),
                            },
                        };
                    });
                }
                // D-STRPARSE1: text interpretation stays `Type.parse(text)`. Carry the
                // text as the builtin receiver so the existing builtin TIR seam owns emit.
                if let ("Int" | "Float", "parse", Some(arg)) =
                    (type_name.as_str(), method, args.first())
                {
                    if args.len() == 1 {
                        return in_own_frame(|| {
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
                        });
                    }
                }
                if type_name == "Int" && method == "from_radix" && args.len() == 2 {
                    return in_own_frame(|| {
                        let line =
                            crate::Diagnostics::span_line_col(&cx.src, method_span.start).0 as u32;
                        return TExpr {
                            ty: resolved_ret.cloned().unwrap_or(Type::Int),
                            kind: TExprKind::BuiltinMethod {
                                recv: Box::new(lower_expr(&args[0].expr, cx, env)),
                                op: TBuiltinOp::IntFromRadix { line },
                                args: vec![lower_expr(&args[1].expr, cx, env)],
                            },
                        };
                    });
                }

                // D-BYTESDECODE1: UTF-8 decoding is a String static method, but
                // its value argument is the builtin receiver so every engine uses
                // the same Prelude-backed TIR operation.
                if let ("String", method @ ("from_bytes" | "from_bytes_lossy"), Some(arg)) =
                    (type_name.as_str(), method, args.first())
                {
                    if args.len() == 1 {
                        let (op, ty) = if method == "from_bytes" {
                            (
                                TBuiltinOp::StringFromBytes,
                                resolved_ret.cloned().unwrap_or_else(|| Type::Result {
                                    ok: Box::new(Type::String),
                                    err: Box::new(Type::Named(
                                        crate::Syntax::TYPE_UTF8_ERROR.to_string(),
                                    )),
                                }),
                            )
                        } else {
                            (
                                TBuiltinOp::StringFromBytesLossy,
                                resolved_ret.cloned().unwrap_or(Type::String),
                            )
                        };
                        return in_own_frame(|| TExpr {
                            ty,
                            kind: TExprKind::BuiltinMethod {
                                recv: Box::new(lower_expr(&arg.expr, cx, env)),
                                op,
                                args: vec![],
                            },
                        });
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
                        return in_own_frame(|| {
                            let input = lower_expr(&arg.expr, cx, env);
                            // Exact carriers are opaque Prelude values, not Rust
                            // primitives. Cross to Float through their shared
                            // `to_float` operation instead of emitting `as f64`.
                            if target == Type::Float
                                && matches!(source_name, "Fraction" | "Decimal")
                            {
                                return TExpr {
                                    ty: resolved_ret.cloned().unwrap_or(Type::Float),
                                    kind: TExprKind::PreciseBuiltin {
                                        type_name: source_name.to_string(),
                                        func: "to_float".to_string(),
                                        args: vec![input],
                                    },
                                };
                            }
                            let op = resolve_numeric_conversion_op(&type_name, source_name)
                                .expect("sema admitted a numeric destination conversion");
                            let ty =
                                crate::Collections::builtin_method_return(&target, method, 1, true)
                                    .flatten()
                                    .unwrap_or(target);
                            return TExpr {
                                ty,
                                kind: TExprKind::NumericMethod {
                                    recv: Box::new(input),
                                    op,
                                },
                            };
                        });
                    }
                }
                if let (Some((base, _)), Some(source), Some(arg)) = (
                    cx.distinct_types.get(&type_name),
                    Syntax::numeric_conversion_source(method),
                    args.first(),
                ) {
                    if args.len() == 1 {
                        return in_own_frame(|| {
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
                        });
                    }
                }
                if let (Some((base, _)), Some(arg)) =
                    (cx.distinct_types.get(&type_name), args.first())
                {
                    if args.len() == 1
                        && !base.is_numeric()
                        && Syntax::conversion_method_for_source(&base.name()) == method
                    {
                        return in_own_frame(|| {
                            return TExpr {
                                ty: Type::Named(type_name.clone()),
                                kind: TExprKind::Call {
                                    name: type_name,
                                    type_args: Vec::new(),
                                    args: vec![lower_one_call_arg(arg, None, env, cx)],
                                },
                            };
                        });
                    }
                }
                if let (Some(destination), Some(arg)) =
                    (cx.unit_facts.get(&type_name), args.first())
                {
                    let exact_method = method.strip_suffix("_rounded").unwrap_or(method);
                    let lowered = lower_expr(&arg.expr, cx, env);
                    let rounding = args.get(1).and_then(|mode| match &mode.expr {
                        Expr::EnumLit {
                            type_name,
                            variant,
                            args,
                            ..
                        } if type_name.is_empty() && args.is_empty() => {
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
                            return in_own_frame(|| {
                                let Ok(scale) = source.scale.div(&destination.scale) else {
                                    return invariant_method_expr(
                                        method_span,
                                        "unit conversion without a valid checked scale",
                                    );
                                };
                                let Ok(offset) = source
                                    .offset
                                    .sub(&destination.offset)
                                    .and_then(|value| value.div(&destination.scale))
                                else {
                                    return invariant_method_expr(
                                        method_span,
                                        "unit conversion without a valid checked offset",
                                    );
                                };
                                let measured_scale_uncertainty =
                                    |fact: &crate::Codegen::UnitFact| {
                                        let crate::AST::UnitScaleProvenance::Measured {
                                            standard_uncertainty,
                                            ..
                                        } = &fact.scale_provenance
                                        else {
                                            return None;
                                        };
                                        let standard_uncertainty =
                                            standard_uncertainty.parse::<f64>().ok()?;
                                        let scale = unit_ratio_as_f64(&fact.scale)?.abs();
                                        (scale.is_finite() && scale > 0.0)
                                            .then_some(standard_uncertainty.abs() / scale)
                                    };
                                let source_relative = measured_scale_uncertainty(source);
                                let destination_relative = measured_scale_uncertainty(destination);
                                let relative_uncertainty = if source_relative.is_some()
                                    || destination_relative.is_some()
                                {
                                    Some(
                                        source_relative
                                            .unwrap_or(0.0)
                                            .hypot(destination_relative.unwrap_or(0.0)),
                                    )
                                } else {
                                    None
                                };
                                let Some(result_ty) = resolved_ret.cloned() else {
                                    return invariant_method_expr(
                                        method_span,
                                        "unit conversion without a resolved return type",
                                    );
                                };
                                let fallible = matches!(&result_ty, Type::Result { .. });
                                let rounding = rounding.map(|mode| {
                                    (mode, Box::new(lower_expr(&args[2].expr, cx, env)))
                                });
                                return TExpr {
                                    ty: result_ty,
                                    kind: TExprKind::UnitConvert {
                                        destination: type_name,
                                        arg: Box::new(lowered),
                                        scale,
                                        offset,
                                        rounding,
                                        fallible,
                                        relative_uncertainty,
                                        file: cx.file.clone(),
                                        line: crate::Diagnostics::span_line_col(
                                            &cx.src,
                                            method_span.start,
                                        )
                                        .0 as u32,
                                    },
                                };
                            });
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
                    return in_own_frame(|| {
                        let str_arg = lower_expr(&args[0].expr, cx, env);
                        return TExpr {
                            ty: Type::Named("Path".to_string()),
                            kind: TExprKind::HandleMethod {
                                recv: Box::new(str_arg),
                                op: THandleOp::PathFrom,
                                args: vec![],
                            },
                        };
                    });
                }
                // D-SHIFT1 (c7shift): `Reader.over(bytes)` keeps the borrowing
                // clone helper unless sema proved a generic owned last use.
                // Same "arg becomes the recv slot" shape as `Path.from` above.
                if type_name == "Reader"
                    && method == "over"
                    && args.len() == 1
                    && !cx.type_names.contains("Reader")
                {
                    let owned = args[0].flags.owned_last_use;
                    return in_own_frame(|| {
                        let bytes_arg = lower_expr(&args[0].expr, cx, env);
                        return TExpr {
                            ty: Type::Named("Reader".to_string()),
                            kind: TExprKind::HandleMethod {
                                recv: Box::new(bytes_arg),
                                op: THandleOp::ReaderOver { owned },
                                args: vec![],
                            },
                        };
                    });
                }
                // D-SHIFT1: `Cursor.over(s)` → `jet_cursor_over(&(s_arg))`.
                if type_name == "Cursor"
                    && method == "over"
                    && args.len() == 1
                    && !cx.type_names.contains("Cursor")
                {
                    return in_own_frame(|| {
                        let s_arg = lower_expr(&args[0].expr, cx, env);
                        return TExpr {
                            ty: Type::Named("Cursor".to_string()),
                            kind: TExprKind::HandleMethod {
                                recv: Box::new(s_arg),
                                op: THandleOp::CursorOver,
                                args: vec![],
                            },
                        };
                    });
                }
                // D-FIDELITY-API1=A: `Perf.fidelity()` / `Perf.override_fidelity(v)?`
                // lower to the same core call shape as `use core.perf as perf`.
                if type_name == "Perf" && !cx.type_names.contains("Perf") {
                    return in_own_frame(|| {
                        let targs: Vec<TExpr> =
                            args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect();
                        let widen_to_vec = core_widen_to_vec("core.perf", method, &targs);
                        let Some(ty) = resolved_ret.cloned() else {
                            return invariant_method_expr(
                                method_span,
                                format!("Perf.{method} without a resolved return type",),
                            );
                        };
                        let record = match checked_core_record(
                            "core.perf",
                            method,
                            targs.len(),
                            method_span,
                        ) {
                            Ok(record) => record,
                            Err(expr) => return expr,
                        };
                        TExpr {
                            ty: ty.clone(),
                            kind: TExprKind::CoreCall {
                                record,
                                args: targs,
                                source_span: method_span,
                                type_args: Vec::new(),
                                widen_to_vec,
                                data_plan: None,
                                fallibility: TFailureCarrier::from_checked_type(&ty),
                            },
                        }
                    });
                }
                // D-COLLBREADTH1=A: `Set.from([...])` → collect list into HashSet.
                // Lower the list arg as the recv of a SetFrom BuiltinMethod.
                if type_name == "Set" && method == "from" && args.len() == 1 {
                    return in_own_frame(|| {
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
                    });
                }
                // #1478: `Set.new()` → empty HashSet with elem type from sema.
                if type_name == "Set" && method == "new" && args.is_empty() {
                    return in_own_frame(|| {
                        let elem_ty = match resolved_ret {
                            Some(Type::Apply { args: targs, .. }) if !targs.is_empty() => {
                                targs[0].clone()
                            }
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
                    });
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
                    return in_own_frame(|| {
                        let elem_ty = match resolved_ret {
                            Some(Type::Result { ok, .. }) => match ok.as_ref() {
                                Type::List(inner) => (**inner).clone(),
                                _ => Type::Int,
                            },
                            _ => Type::Int,
                        };
                        let ty = resolved_ret.cloned().unwrap_or_else(|| Type::Result {
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
                    });
                }
                // #1477: `Map.new()` → empty JetMap; `Map.from_keys(keys, default)`.
                if type_name == Syntax::TYPE_MAP && method == "new" && args.is_empty() {
                    return in_own_frame(|| {
                        let (key, value) = match resolved_ret {
                            Some(Type::Map { key, value, .. }) => {
                                ((**key).clone(), (**value).clone())
                            }
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
                    });
                }
                if type_name == Syntax::TYPE_MAP && method == "from_keys" && args.len() == 2 {
                    return in_own_frame(|| {
                        let keys = lower_expr(&args[0].expr, cx, env);
                        let default = lower_expr(&args[1].expr, cx, env);
                        let (key, value) = match resolved_ret {
                            Some(Type::Map { key, value, .. }) => {
                                ((**key).clone(), (**value).clone())
                            }
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
                    });
                }
                if type_name == crate::Syntax::TYPE_RANK && method == "from" && args.len() == 1 {
                    return in_own_frame(|| {
                        let list_arg = lower_expr(&args[0].expr, cx, env);
                        let elem_ty = match &list_arg.ty {
                            Type::List(inner) => *inner.clone(),
                            _ => Type::Int,
                        };
                        return TExpr {
                            ty: Type::Apply {
                                name: crate::Syntax::TYPE_RANK.to_string(),
                                args: vec![elem_ty],
                            },
                            kind: TExprKind::BuiltinMethod {
                                recv: Box::new(list_arg),
                                op: TBuiltinOp::SortedSetFrom,
                                args: vec![],
                            },
                        };
                    });
                }
                if type_name == crate::Syntax::TYPE_RANK && method == "new" && args.is_empty() {
                    return in_own_frame(|| {
                        let elem_ty = match resolved_ret {
                            Some(Type::Apply { args: targs, .. }) if !targs.is_empty() => {
                                targs[0].clone()
                            }
                            _ => Type::Int,
                        };
                        return TExpr {
                            ty: Type::Apply {
                                name: crate::Syntax::TYPE_RANK.to_string(),
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
                    });
                }
                if type_name == crate::Syntax::TYPE_PRIORITY_QUEUE
                    && method == "from"
                    && args.len() == 1
                {
                    return in_own_frame(|| {
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
                    });
                }
                if type_name == crate::Syntax::TYPE_PRIORITY_QUEUE
                    && method == "new"
                    && args.is_empty()
                {
                    return in_own_frame(|| {
                        let elem_ty = match resolved_ret {
                            Some(Type::Apply { args: targs, .. }) if !targs.is_empty() => {
                                targs[0].clone()
                            }
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
                    });
                }
                if type_name == crate::Syntax::TYPE_LRU && method == "new" && args.len() == 1 {
                    return in_own_frame(|| {
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
                                owner: rooted_owner("JetCache"),
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
                                    box_as_trait: None,
                                }],
                            },
                        };
                    });
                }
                if type_name == crate::Syntax::TYPE_BITS && method == "new" && args.is_empty() {
                    return in_own_frame(|| {
                        return TExpr {
                            ty: Type::Named(crate::Syntax::TYPE_BITS.to_string()),
                            kind: TExprKind::StaticCall {
                                owner: rooted_owner("JetBitSet"),
                                owner_type: None,
                                method: TMethodRef::bare("new"),
                                type_args: Vec::new(),
                                args: vec![],
                            },
                        };
                    });
                }
                if type_name == crate::Syntax::TYPE_BYTES && method == "new" && args.is_empty() {
                    return in_own_frame(|| {
                        return TExpr {
                            ty: Type::Named(crate::Syntax::TYPE_BYTES.to_string()),
                            kind: TExprKind::StaticCall {
                                owner: rooted_owner("JetByteBuffer"),
                                owner_type: None,
                                method: TMethodRef::bare("new"),
                                type_args: Vec::new(),
                                args: vec![],
                            },
                        };
                    });
                }
                if type_name == crate::Syntax::TYPE_BYTES
                    && method == "with_capacity"
                    && args.len() == 1
                {
                    return in_own_frame(|| {
                        let n = lower_expr(&args[0].expr, cx, env);
                        return TExpr {
                            ty: Type::Named(crate::Syntax::TYPE_BYTES.to_string()),
                            kind: TExprKind::BuiltinMethod {
                                recv: Box::new(n),
                                op: TBuiltinOp::ByteBufferWithCapacity,
                                args: vec![],
                            },
                        };
                    });
                }
                if type_name == crate::Syntax::TYPE_BYTES && method == "from" && args.len() == 1 {
                    return in_own_frame(|| {
                        let bytes_arg = lower_expr(&args[0].expr, cx, env);
                        return TExpr {
                            ty: Type::Named(crate::Syntax::TYPE_BYTES.to_string()),
                            kind: TExprKind::BuiltinMethod {
                                recv: Box::new(bytes_arg),
                                op: TBuiltinOp::ByteBufferFrom,
                                args: vec![],
                            },
                        };
                    });
                }
                // D-COLLBREADTH1=A: `Queue.init([...])` → collect list into VecDeque.
                if type_name == crate::Syntax::TYPE_QUEUE && method == "init" && args.len() == 1 {
                    return in_own_frame(|| {
                        let list_arg = lower_expr(&args[0].expr, cx, env);
                        let elem_ty = match &list_arg.ty {
                            Type::List(inner) => *inner.clone(),
                            _ => Type::Int,
                        };
                        return TExpr {
                            ty: Type::Apply {
                                name: crate::Syntax::TYPE_QUEUE.to_string(),
                                args: vec![elem_ty],
                            },
                            kind: TExprKind::BuiltinMethod {
                                recv: Box::new(list_arg),
                                op: TBuiltinOp::DequeFrom,
                                args: vec![],
                            },
                        };
                    });
                }
                // D-COLLBREADTH1=A: `Queue.new()` → empty VecDeque with elem type from sema.
                // The element type comes from `resolved_ret` (sema filled it from the annotation).
                if type_name == crate::Syntax::TYPE_QUEUE && method == "new" && args.is_empty() {
                    return in_own_frame(|| {
                        let elem_ty = match resolved_ret {
                            Some(Type::Apply { args: targs, .. }) if !targs.is_empty() => {
                                targs[0].clone()
                            }
                            _ => Type::Int,
                        };
                        let deque_ty = Type::Apply {
                            name: crate::Syntax::TYPE_QUEUE.to_string(),
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
                    });
                }
                // D-TAG1: `Tally.new()` → empty HashMap with elem type from sema.
                if type_name == crate::Syntax::TYPE_TALLY && method == "new" && args.is_empty() {
                    return in_own_frame(|| {
                        let elem_ty = match resolved_ret {
                            Some(Type::Apply { args: targs, .. }) if !targs.is_empty() => {
                                targs[0].clone()
                            }
                            _ => Type::Int,
                        };
                        let bag_ty = Type::Apply {
                            name: crate::Syntax::TYPE_TALLY.to_string(),
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
                    });
                }
                // D-MEM1 S6 (D-POOLID-API1=A): `Pool<T>.new()` → an empty `JetPool<T>`.
                // The element type comes from `resolved_ret` (sema filled it from the
                // call-site turbofish or the binding's annotation).
                if type_name == "Pool" && method == "new" && args.is_empty() {
                    return in_own_frame(|| {
                        let elem_ty = match resolved_ret {
                            Some(Type::Apply { args: targs, .. }) if !targs.is_empty() => {
                                targs[0].clone()
                            }
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
                    });
                }
                // D-MEM1 S6 (D-SHARED-API1=A): `Shared.new(x)` → a `JetShared<T>`
                // wrapping `x`; `T` is `x`'s own lowered type (no turbofish, no
                // annotation needed — the argument alone fixes it).
                if type_name == "Shared" && method == "new" && args.len() == 1 {
                    return in_own_frame(|| {
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
                                    box_as_trait: None,
                                }],
                            },
                        };
                    });
                }
                if type_name == Syntax::TYPE_CONDITION && method == "new" && args.is_empty() {
                    return in_own_frame(|| {
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
                    });
                }
                if type_name == "Cell" && method == "new" && args.len() == 1 {
                    return in_own_frame(|| {
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
                                    box_as_trait: None,
                                }],
                            },
                        };
                    });
                }
                if type_name == "ExpiringSecret" && method == "new" && args.len() == 3 {
                    return in_own_frame(|| {
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
                    });
                }
                if type_name == Syntax::EXPIRING_VALUE_TYPE && method == "new" && args.len() == 3 {
                    return in_own_frame(|| {
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
                    });
                }
                // D-HOLE1: `Option.lift2(f, a, b)` → apply `f` to both payloads only when
                // both are present. `a`/`b` lower plainly as values; `R` comes from sema's
                // `resolved_ret` (the arg-dependent return type, same mechanism the
                // polymorphic core specials use — see the AST `MethodCall.resolved_ret` doc).
                if type_name == "Option" && method == "lift2" && !cx.type_names.contains("Option") {
                    return in_own_frame(|| {
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
                        let f_t = in_own_frame(|| match &args[0].expr {
                            Expr::Lambda(lam) => super::take_scheduled_expr(&args[0].expr, cx)
                                .unwrap_or_else(|| {
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
                                            effect_bound: None,
                                            return_view_provenance: None,
                                            param_contract: None,
                                            call_metadata: None,
                                        },
                                        kind: TExprKind::Lambda(Box::new(tl)),
                                    }
                                }),
                            _ => lower_expr(&args[0].expr, cx, env),
                        });
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
                    });
                }
                // D-SPACE-GEOMETRY1=A: stock coordinate constructors lower
                // through the same typed MathBuiltin family as linalg values.
                if crate::Sema::is_geometry_type(&type_name) && !cx.type_names.contains(&type_name)
                {
                    if let Some(ret) = crate::Sema::geometry_static_return_with_owner(
                        &type_name,
                        method,
                        args.len(),
                        owner_type_args,
                    ) {
                        return in_own_frame(|| TExpr {
                            ty: ret,
                            kind: TExprKind::MathBuiltin {
                                type_name: type_name.clone(),
                                func: method.to_string(),
                                args: args.iter().map(|a| lower_expr(&a.expr, cx, env)).collect(),
                            },
                        });
                    }
                }

                // D-SIMD2 / D-LINALG1: a static method on a built-in math type → the prelude
                // free function `{root}jet_math_<T>_<method>(args)`.
                if crate::Sema::is_math_type(&type_name) && !cx.type_names.contains(&type_name) {
                    if let Some(ret) =
                        crate::Sema::math_static_return(&type_name, method, args.len())
                    {
                        return in_own_frame(|| {
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
                        });
                    }
                }
                // A module-qualified receiver (`mymod.Thing.new()`) registers its
                // methods under the bare leaf, so the leaf is the right key there.
                // But D-PROTO1/D-PROTO2 declare `Payment.Client` — the dot is part
                // of the type's own name, and `register_method` keyed its surface by
                // that full spelling. Stripping unconditionally missed both
                // `method_sigs` and `method_rets`, so `instantiate_method_ret` fell
                // through to `unit_type()` and every dotted-handle static call was
                // stamped `Unit`. Engines that need an ABI then read the wrong
                // carrier: the resident tier stored `Payment.Client.client()` in an
                // i8 slot and refused the next `^handle` argument, which wants the
                // i64 handle. Ask the one method table which spelling it knows.
                let dotted_key = (type_name.clone(), method.to_string());
                let lookup_type_name = if cx.method_sigs.contains_key(&dotted_key)
                    || cx.method_rets.contains_key(&dotted_key)
                {
                    type_name.as_str()
                } else {
                    type_name
                        .rsplit_once('.')
                        .map_or(type_name.as_str(), |(_, leaf)| leaf)
                };
                let operator_method = operator_rhs.and_then(|rhs| {
                    let trait_name = operator_trait_for_method(method)?;
                    let key = crate::Traits::operator_method_identity(
                        lookup_type_name,
                        trait_name,
                        method,
                        rhs,
                    );
                    cx.operator_methods.get(&key).cloned()
                });
                let sig = if operator_rhs.is_some() {
                    operator_method
                        .as_ref()
                        .map(|facts| facts.sig.clone())
                        .unwrap_or_default()
                } else {
                    let sig = cx
                        .method_sigs
                        .get(&(lookup_type_name.to_string(), method.to_string()))
                        .cloned()
                        .unwrap_or_default();
                    instantiated_sig.map(|sig| sig.to_vec()).unwrap_or_else(|| {
                        instantiate_method_sig(
                            cx,
                            &type_name,
                            method,
                            &sig,
                            owner_type_args,
                            type_args,
                        )
                    })
                };
                let mut targs = lower_method_args(args, &sig, env, cx);
                retag_empty_collection_args(&mut targs, &sig);
                let resolved_type_args = if operator_rhs.is_some() {
                    type_args.to_vec()
                } else {
                    resolved_method_type_args(
                        cx,
                        lookup_type_name,
                        method,
                        &sig,
                        owner_type_args,
                        &targs,
                        type_args,
                        resolved_ret,
                    )
                };
                let source_ret_ty = if operator_rhs.is_some() {
                    resolved_ret
                        .cloned()
                        .or_else(|| operator_method.as_ref().and_then(|facts| facts.ret.clone()))
                } else {
                    instantiate_method_ret(
                        cx,
                        lookup_type_name,
                        method,
                        owner_type_args,
                        &resolved_type_args,
                        resolved_ret,
                    )
                }
                .map(|ty| resolve_self_ty(&ty, &type_name))
                .unwrap_or_else(unit_type);
                let ret_ty = if env.fallback_subject && resolved_ret.is_some() {
                    jet_foundation::AST::FailureContract::from_return_type(Some(&source_ret_ty))
                        .effective_type()
                } else {
                    source_ret_ty
                };
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
                        (
                            owner_type.clone(),
                            method.to_string(),
                            resolved_type_args.clone(),
                        ),
                    );
                }
                let trait_name = operator_method
                    .as_ref()
                    .map(|facts| facts.trait_name.clone())
                    .or_else(|| {
                        cx.trait_method_traits
                            .get(&(lookup_type_name.to_string(), method.to_string()))
                            .cloned()
                    });
                let generic_method = (matches!(&owner_type, Type::Apply { .. })
                    || !resolved_type_args.is_empty())
                    && operator_rhs.is_none()
                    && trait_name.is_none();
                let method_ref = if let Some(rhs) = operator_rhs {
                    let trait_name = trait_name
                        .as_deref()
                        .or_else(|| operator_trait_for_method(method))
                        .expect("checked operator method has a canonical trait");
                    TMethodRef::operator(lookup_type_name, trait_name, method, rhs)
                } else {
                    trait_name.as_deref().map_or_else(
                        || {
                            let method_name = if generic_method {
                                generic_method_instance_leaf(
                                    &owner_type,
                                    method,
                                    &resolved_type_args,
                                )
                            } else {
                                method.to_string()
                            };
                            TMethodRef::inherent(method_name)
                        },
                        |trait_name| TMethodRef::trait_method(trait_name, method),
                    )
                };
                return TExpr {
                    ty: ret_ty,
                    kind: TExprKind::StaticCall {
                        owner: TStaticOwner::User(if generic_method {
                            owner_type.name()
                        } else {
                            type_name.clone()
                        }),
                        owner_type: Some(owner_type),
                        method: method_ref,
                        type_args: resolved_type_args,
                        args: targs,
                    },
                };
            });
        }
    }
    // c109 Phase 30: DYNAMIC dispatch on a TRAIT-OBJECT receiver (`s.name()`/`s.area()`,
    // `s: Box<dyn __jet_Shape>`). The gate proved `recv_type == Some(<trait>)` with the
    // trait in `cx.trait_names`. The AST `emit_method_call` (Expression.rs ~L1657) emits
    // `({recv}).{method}({args})` — the BARE (unmangled) method name (vtable dispatch),
    // node (`({recv}).{method_rust}({args})`) with the bare method name. Trait declarations
    // retain their full parameter/return facts in `Cx`, so dynamic calls use the same
    // convention-aware argument lowering as static calls.
    if let Some(ty) = recv_type {
        if cx.trait_names.contains(ty) || crate::Generics::is_builtin_trait(ty) {
            return in_own_frame(|| {
                let mut recv = lower_expr(receiver, cx, env);
                let concrete_owner = match &recv.ty {
                    Type::Named(name) => Some(name.clone()),
                    Type::Apply { name, .. } => Some(name.clone()),
                    _ => None,
                };
                let derived_operator_rhs = if operator_rhs.is_none() {
                    let trait_name = operator_trait_for_method(method);
                    let rhs = match recv.ty.without_user_tags() {
                        Type::Named(_) | Type::Apply { .. } => Some(recv.ty.clone()),
                        _ => None,
                    };
                    trait_name.and(rhs)
                } else {
                    None
                };
                let effective_operator_rhs = operator_rhs.or(derived_operator_rhs.as_ref());
                let operator_method = effective_operator_rhs.and_then(|rhs| {
                    let owner = concrete_owner.as_deref()?;
                    let trait_name = operator_trait_for_method(method)?;
                    let owner_leaf = owner.rsplit("::").next().unwrap_or(owner);
                    let lookup_owner = cx
                        .local_type_identities
                        .get(owner)
                        .map(String::as_str)
                        .unwrap_or(owner);
                    let rhs_name = rhs.name();
                    let rhs_leaf = rhs_name.rsplit("::").next().unwrap_or(rhs_name.as_str());
                    let rhs_source = Type::Named(rhs_leaf.to_string());
                    [owner, lookup_owner, owner_leaf]
                        .into_iter()
                        .find_map(|candidate| {
                            [&rhs, &rhs_source].into_iter().find_map(|candidate_rhs| {
                                let key = crate::Traits::operator_method_identity(
                                    candidate,
                                    trait_name,
                                    method,
                                    candidate_rhs,
                                );
                                cx.operator_methods.get(&key).cloned()
                            })
                        })
                });
                let key = (ty.clone(), method.to_string());
                let sig = if effective_operator_rhs.is_some() {
                    operator_method
                        .as_ref()
                        .map(|facts| facts.sig.clone())
                        .unwrap_or_else(|| {
                            vec![(
                                crate::AST::AccessConvention::Read,
                                    (*effective_operator_rhs
                                        .as_ref()
                                        .expect("effective operator RHS"))
                                        .clone(),
                            )]
                        })
                } else {
                    cx.method_sigs.get(&key).cloned().unwrap_or_default()
                };
                let sig = if effective_operator_rhs.is_some() {
                    sig
                } else {
                    instantiated_sig.map(|sig| sig.to_vec()).unwrap_or_else(|| {
                        instantiate_method_sig(cx, ty, method, &sig, &[], type_args)
                    })
                };
                let ret_ty = if effective_operator_rhs.is_some() {
                    resolved_ret.cloned().or_else(|| {
                        operator_method
                            .as_ref()
                            .and_then(|facts| facts.ret.clone())
                            .or_else(|| match operator_trait_for_method(method) {
                                Some(Syntax::TRAIT_EQUATABLE) => Some(Type::Bool),
                                Some(Syntax::TRAIT_COMPARABLE) => {
                                    Some(Type::Named(Syntax::TYPE_ORDERING.to_string()))
                                }
                                _ => None,
                            })
                    })
                } else {
                    resolved_ret
                        .cloned()
                        .or_else(|| cx.method_rets.get(&key).cloned().flatten())
                }
                .unwrap_or_else(unit_type);
                let builtin_operator = crate::Generics::is_builtin_trait(ty)
                    && matches!(method, "add" | "sub" | "mul" | "div" | "equal" | "compare");
                let distinct_numeric_operator = concrete_owner.as_ref().is_some_and(|owner| {
                    cx.distinct_types
                        .get(owner)
                        .is_some_and(|(_, numeric)| *numeric)
                        && !cx.distinct_ranges.contains_key(owner)
                        && !cx.method_sigs.contains_key(&(owner.clone(), method.to_string()))
                }) && operator_method.is_none()
                    && matches!(method, "add" | "sub" | "mul" | "div");
                if builtin_operator
                    && args.len() == 1
                    && (recv.ty.is_numeric()
                        || matches!(recv.ty, Type::Bool | Type::Char | Type::String)
                        || distinct_numeric_operator)
                {
                    // Specialized scalars and numeric bundles use the same base
                    // operation; synthetic traits have no user MIR function body.
                    let rhs = lower_expr(&args[0].expr, cx, env);
                    return lower_builtin_binary_method(method, recv, rhs, ret_ty, method_span, cx);
                }
                let mut targs = lower_method_args(args, &sig, env, cx);
                retag_empty_collection_args(&mut targs, &sig);
                if builtin_operator {
                    if let Some(rhs) = targs.first_mut() {
                        rhs.borrow = true;
                    }
                }
                // D-NUMOPS1/D-FAIL-ARITH1: an arithmetic operator that reached its
                // bound generically (`fn f<T: Add>(l: T, r: T) => T { return l + r }`)
                // traps in the same Prelude kernel as the direct operator, so it must
                // report the same Jet location. Record the operator's source line
                // here; emit then spells the trait's `_at` entry point, whose
                // fixed-width impls thread `(file, line)` into `jet_add`/`jet_div`.
                // Without it the call lands on the impl's location-free plain method,
                // whose only spellable placeholder is `<built-in Add>:0`.
                // `equal`/`compare` never trap, so they carry no line.
                let operator_line = (builtin_operator
                    && matches!(method, "add" | "sub" | "mul" | "div"))
                .then(|| crate::Diagnostics::span_line_col(&cx.src, method_span.start).0 as u32);
                // A specialized generic/variadic parameter can retain the trait as
                // `recv_type` while its lowered TIR type is the exact concrete
                // implementation. Keep that concrete ABI and dispatch directly;
                // only genuine trait-object values use the boxed dynamic ABI.
                let direct_bound_dispatch = match &recv.ty {
                    Type::Named(concrete) => {
                        !cx.trait_names.contains(concrete)
                            && !crate::Generics::is_builtin_trait(concrete)
                    }
                    Type::TraitObject(_) => false,
                    _ => true,
                };
                let method_ref = if let Some(rhs) = effective_operator_rhs {
                    let owner = concrete_owner
                        .as_deref()
                        .expect("checked operator call has a concrete receiver");
                    let trait_name = operator_method
                        .as_ref()
                        .map(|facts| facts.trait_name.as_str())
                        .or_else(|| operator_trait_for_method(method))
                        .expect("checked operator method has a canonical trait");
                    TMethodRef::operator(owner, trait_name, method, rhs)
                } else {
                    TMethodRef::trait_method(ty, method)
                };
                if direct_bound_dispatch {
                    return in_own_frame(|| {
                        return TExpr {
                            ty: ret_ty,
                            kind: TExprKind::MethodCall {
                                recv: Box::new(recv),
                                method: method_ref,
                                type_args: type_args.to_vec(),
                                args: targs,
                                source_first_string_literal: first_string_literal_arg(args),
                                operator_line,
                            },
                        };
                    });
                }
                recv.ty = Type::TraitObject(vec![ty.clone()]);
                return TExpr {
                    ty: ret_ty,
                    kind: TExprKind::MethodCall {
                        recv: Box::new(recv),
                        method: method_ref,
                        type_args: type_args.to_vec(),
                        args: targs,
                        source_first_string_literal: first_string_literal_arg(args),
                        operator_line,
                    },
                };
            });
        }
    }
    return in_own_frame(|| {
        // D-BUILDENTRY1 / D-METAREFLECT1: BuildContext, ProgramInfo, and TypeInfo
        // methods are compiler-host operations. Lower them as HostCall so MIR
        // eval can reach `eval_program_build_method` / `Builtins::apply_method`
        // without synthesizing missing user methods or field IDs.
        let ty_name = recv_type.clone().or_else(|| match tir_recv_jet_ty(receiver, env) {
            Some(Type::Named(name) | Type::Apply { name, .. }) => Some(name),
            _ => None,
        });
        if let Some(ty_name) = ty_name.as_deref() {
            if method != "generate" && is_comptime_host_method(ty_name, method) {
                let recv = lower_expr(receiver, cx, env);
                let targs = args
                    .iter()
                    .map(|arg| lower_expr(&arg.expr, cx, env))
                    .collect();
                let ty = resolved_ret
                    .cloned()
                    .or_else(|| {
                        crate::Collections::builtin_method_return(
                            &Type::Named(ty_name.to_string()),
                            method,
                            args.len(),
                            false,
                        )
                        .flatten()
                    })
                    .unwrap_or_else(unit_type);
                return TExpr {
                    ty,
                    kind: TExprKind::HostCall(Box::new(THostCall::Method {
                        recv: Box::new(recv),
                        method: method.to_string(),
                        args: targs,
                    })),
                };
            }
        }
        let Some(ty_name) = ty_name else {
            return invariant_method_expr(method_span, format!("method `{method}` receiver type"));
        };
        // Imported method metadata is keyed by the declaration's canonical nominal
        // identity. Resolve the source-facing leaf once while retaining `ty_name`
        // on the lowered receiver.
        let lookup_ty_name = cx
            .imported_type_metadata_name(&ty_name)
            .unwrap_or_else(|| ty_name.clone());
        let operator_method = operator_rhs.and_then(|rhs| {
            let trait_name = operator_trait_for_method(method)?;
            let key =
                crate::Traits::operator_method_identity(&lookup_ty_name, trait_name, method, rhs);
            cx.operator_methods.get(&key).cloned()
        });
        // A checked operator carries its complete identity from sema. Never
        // collapse that call onto the ordinary owner/method table.
        let sig = if operator_rhs.is_some() {
            operator_method
                .as_ref()
                .map(|facts| facts.sig.clone())
                .unwrap_or_default()
        } else {
            cx.method_sigs
                .get(&(lookup_ty_name.to_string(), method.to_string()))
                .cloned()
                .unwrap_or_default()
        };
        let recv = if matches!(
            cx.method_self_convs
                .get(&(lookup_ty_name.to_string(), method.to_string())),
            Some(AccessConvention::Move)
        ) && matches!(receiver, Expr::Ident(name, _) if env.is_resource(name))
        {
            lower_owned_expr(receiver, cx, env)
        } else {
            lower_expr(receiver, cx, env)
        };
        let owner_type_args = match &recv.ty {
            Type::Apply { name, args } if name == &ty_name || name == &lookup_ty_name => {
                args.as_slice()
            }
            _ => &[][..],
        };
        let sig = if operator_rhs.is_some() {
            sig
        } else {
            instantiated_sig.map(|sig| sig.to_vec()).unwrap_or_else(|| {
                instantiate_method_sig(
                    cx,
                    &lookup_ty_name,
                    method,
                    &sig,
                    owner_type_args,
                    type_args,
                )
            })
        };
        if matches!(&recv.ty, Type::Named(name) if cx.trait_names.contains(name)) {
            return in_own_frame(|| {
                let trait_name = recv.ty.name();
                let mut recv = recv;
                recv.ty = Type::TraitObject(vec![trait_name.clone()]);
                let mut targs = lower_method_args(args, &sig, env, cx);
                retag_empty_collection_args(&mut targs, &sig);
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
            });
        }
        let distinct_numeric_operator = cx
            .distinct_types
            .get(&lookup_ty_name)
            .is_some_and(|(_, numeric)| *numeric)
            && !cx.distinct_ranges.contains_key(&lookup_ty_name)
            && !cx
                .method_sigs
                .contains_key(&(lookup_ty_name.clone(), method.to_string()))
            && operator_method.is_none()
            && matches!(method, "add" | "sub" | "mul" | "div")
            && args.len() == 1;
        if distinct_numeric_operator {
            let rhs = lower_expr(&args[0].expr, cx, env);
            let ret_ty = resolved_ret
                .cloned()
                .or_else(|| operator_method.as_ref().and_then(|facts| facts.ret.clone()))
                .unwrap_or_else(unit_type);
            return lower_builtin_binary_method(
                method,
                recv,
                rhs,
                ret_ty,
                method_span,
                cx,
            );
        }
        let mut targs = lower_method_args(args, &sig, env, cx);
        retag_empty_collection_args(&mut targs, &sig);
        let resolved_type_args = if operator_rhs.is_some() {
            type_args.to_vec()
        } else {
            resolved_method_type_args(
                cx,
                &lookup_ty_name,
                method,
                &sig,
                owner_type_args,
                &targs,
                type_args,
                resolved_ret,
            )
        };
        let generic_method =
            matches!(&recv.ty, Type::Apply { .. }) || !resolved_type_args.is_empty();
        if generic_method {
            cx.jit_method_calls.borrow_mut().insert(
                crate::Codegen::TIR::generic_method_instance_key(
                    &recv.ty,
                    method,
                    &resolved_type_args,
                ),
                (
                    recv.ty.clone(),
                    method.to_string(),
                    resolved_type_args.clone(),
                ),
            );
        }
        let method_ref = if let Some(rhs) = operator_rhs {
            let trait_name = operator_method
                .as_ref()
                .map(|facts| facts.trait_name.as_str())
                .or_else(|| operator_trait_for_method(method))
                .expect("checked operator method has a canonical trait");
            TMethodRef::operator(&lookup_ty_name, trait_name, method, rhs)
        } else if cx
            .trait_methods
            .contains(&(lookup_ty_name.to_string(), method.to_string()))
        {
            TMethodRef::bare(method)
        } else if generic_method {
            let method_name = crate::Codegen::TIR::generic_method_instance_key(
                &recv.ty,
                method,
                &resolved_type_args,
            );
            TMethodRef::inherent(method_name)
        } else {
            TMethodRef::inherent(method)
        };
        // The result type, read from the resolved method return (total fact). It is
        // rarely load-bearing in emit (a binding carries sema's `b.ty`; arithmetic on a
        // method result doesn't trap — matching the AST `expr_jet_ty`/`operand_is_integer`),
        // but the TIR keeps it total per the design principle.
        let ret_ty = if operator_rhs.is_some() {
            resolved_ret
                .cloned()
                .or_else(|| operator_method.as_ref().and_then(|facts| facts.ret.clone()))
        } else {
            instantiate_method_ret(
                cx,
                &lookup_ty_name,
                method,
                owner_type_args,
                &resolved_type_args,
                resolved_ret,
            )
        }
        .unwrap_or_else(unit_type);
        TExpr {
            ty: ret_ty,
            kind: TExprKind::MethodCall {
                recv: Box::new(recv),
                method: method_ref,
                type_args: resolved_type_args,
                args: targs,
                source_first_string_literal: first_string_literal_arg(args),
                operator_line: None,
            },
        }
    });
}

fn comptime_host_type_leaf(name: &str) -> &str {
    name.rsplit("::").next().unwrap_or(name).rsplit('.').next().unwrap_or(name)
}

fn is_comptime_host_method(ty_name: &str, method: &str) -> bool {
    match comptime_host_type_leaf(ty_name) {
        Syntax::TYPE_BUILD_CONTEXT => matches!(
            method,
            "generate"
                | "find"
                | "embed"
                | "fetch"
                | "plugin"
                | "action"
                | "legacy"
                | "add_executable"
                | "add_library"
                | "add_test"
                | "add_asset_bundle"
                | "add_doc"
                | "add_install"
                | "add_package"
                | "add_publish"
                | "toolchain"
                | "signing"
                | "probe"
                | "error"
                | "plan"
                | "contribute"
        ),
        Syntax::TYPE_PROGRAM_INFO => matches!(
            method,
            "types"
                | "functions"
                | "packages"
                | "definitions"
                | "references"
                | "call_edges"
                | "structural_nodes"
        ),
        Syntax::TYPE_TYPE_INFO | "FieldInfo" | "MethodInfo" => {
            matches!(method, "has_marker" | "implements" | "has_method")
        }
        "CompilerLexed" | "CompilerSyntaxTree" | "CompilerChecked" | "CompilerSourceMap"
        | "CompilerPackageError" | "CompilerDependency" | "CompilerPackageTarget"
        | "CompilerPackageOutput" | "CompilerBuildProfile" | "CompilerManifest"
        | "CompilerPackage" | "CompilerLockedPackage" | "CompilerLock"
        | "CompilerKeyValue" | "CompilerProfile" | "CompilerProfileSet" => true,
        _ => false,
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
    if let Some(method_params) = cx
        .method_type_params
        .get(&(type_name.to_string(), method.to_string()))
    {
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
    let template = resolved_ret.cloned().or_else(|| {
        cx.method_rets
            .get(&(type_name.to_string(), method.to_string()))
            .cloned()
            .flatten()
    })?;
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
    let names: std::collections::HashSet<String> = method_params
        .iter()
        .map(|param| param.name.clone())
        .collect();
    let mut subst = std::collections::HashMap::new();
    for ((_, template), arg) in sig.iter().zip(args) {
        if !crate::Codegen::TIR::bind_generic_type(template, &arg.value.ty, &names, &mut subst) {
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
        let effective_template =
            jet_foundation::AST::FailureContract::from_return_type(Some(&template))
                .effective_type();
        let mut effective_subst = subst.clone();
        if crate::Codegen::TIR::bind_generic_type(
            &effective_template,
            actual,
            &names,
            &mut effective_subst,
        ) {
            subst = effective_subst;
        } else if !crate::Codegen::TIR::bind_generic_type(&template, actual, &names, &mut subst) {
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

/// Demand generic codec instances and lower JSON values through `Encode`.
fn prepare_generic_serde_codec(
    cx: &Cx,
    fn_name: &str,
    module: &str,
    method: &str,
    args: &mut [TExpr],
    ret_ty: &Type,
) {
    let encoding = matches!(
        module,
        "core.sys"
            | "core.encoding.json"
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
        "to_string" | "to_string_pretty" | "to_bytes" | "to_bytes_canonical" | "query"
    ) {
        if let Some(arg) = args.first() {
            if matches!(&arg.ty, Type::Apply { .. }) {
                cx.jit_method_calls.borrow_mut().insert(
                    crate::Codegen::TIR::generic_method_instance_key(&arg.ty, "encode", &[]),
                    (arg.ty.clone(), "encode".to_string(), Vec::new()),
                );
            }
        }
    }
    if method == "decode" || method == "query" {
        if let Type::Result { ok, .. } = ret_ty {
            if cx.migrations.contains_key(&ok.name()) {
                cx.jit_canonical_deopt
                    .borrow_mut()
                    .insert(fn_name.to_string());
            }
            let decode_ty = match ok.as_ref() {
                Type::List(inner) if method == "query" => inner.as_ref(),
                other => other,
            };
            if matches!(decode_ty, Type::Apply { .. }) {
                cx.jit_method_calls.borrow_mut().insert(
                    crate::Codegen::TIR::generic_method_instance_key(decode_ty, "decode", &[]),
                    (decode_ty.clone(), "decode".to_string(), Vec::new()),
                );
            }
        }
    }
    if module == "core.encoding.json" && matches!(method, "to_string" | "to_string_pretty") {
        if let Some(arg) = args.first_mut() {
            let value = std::mem::replace(
                arg,
                TExpr {
                    ty: unit_type(),
                    kind: TExprKind::Unit,
                },
            );
            *arg = lower_serde_encode_node(value, cx);
        }
    }
}
