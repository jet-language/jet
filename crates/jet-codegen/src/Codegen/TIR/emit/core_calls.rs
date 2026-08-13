use crate::jet_generated_format as jet_format;
use crate::AST::{AccessConvention, Type};
use crate::Codegen::escape_rust_str;
use crate::Codegen::Cx;
use crate::Codegen::mangle;
use crate::Codegen::mangle_generated;
use crate::Codegen::rust_param_type;
use crate::Codegen::TIR::emit_tir_expr;
use crate::Codegen::TIR::enc_arg_is_json;
use crate::Codegen::TIR::enc_arg_is_string_rows;
use crate::Codegen::TIR::enc_ok_is_json;
use crate::Codegen::TIR::enc_row_target_rust;
use crate::Codegen::TIR::enc_row_target_rust_traced;
use crate::Codegen::TIR::enc_target_rust;
use crate::Codegen::TIR::enc_target_rust_traced;
use crate::Codegen::TIR::struct_field_type;
use crate::Codegen::TIR::emit::emit_symbol_call;
use crate::Codegen::TIR::{TExpr, TExprKind};

fn reflect_field_type(cx: &Cx, owner_ty: &Type, declared: &Type) -> Type {
    let Type::Apply { name, args } = owner_ty else {
        return declared.clone();
    };
    let Some(params) = cx.struct_type_param_order.get(name) else {
        return declared.clone();
    };
    let subst = params
        .iter()
        .zip(args)
        .map(|(param, arg)| (param.clone(), arg.clone()))
        .collect();
    crate::Generics::substitute_type(declared, &subst)
}

fn reflect_nested_value(cx: &Cx, ty: &Type, value: &str) -> String {
    let type_name = ty.leaf_name();
    let path = cx.reflect_path(ty);
    let fields = match ty {
        Type::Named(name) | Type::Apply { name, .. } => cx
            .struct_fields
            .get(name)
            .map(|fields| {
                fields
                    .iter()
                    .map(|(field_name, declared_ty)| {
                        let field_ty = reflect_field_type(cx, ty, declared_ty);
                        let field_value = format!("({value}).{}", mangle(field_name));
                        format!(
                            "{}JetReflectField {{ name: {}.to_string(), value: {} }}",
                            cx.root_prefix,
                            escape_rust_str(field_name),
                            reflect_nested_value(cx, &field_ty, &field_value)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .map(|fields| format!("vec![{fields}]"))
            .unwrap_or_else(|| "Vec::new()".to_string()),
        _ => "Vec::new()".to_string(),
    };
    format!(
        "{}JetReflectValue {{ type_name: {}.to_string(), path: {}.to_string(), display: ({}).jet_display(), fields: {} }}",
        cx.root_prefix,
        escape_rust_str(&type_name),
        escape_rust_str(&path),
        value,
        fields
    )
}

fn compute_tuple_value(ty: &Type, values: &[String]) -> String {
    let Type::Tuple(fields) = ty else {
        return "()".to_string();
    };
    let plain = crate::Codegen::Tuples::tuple_fields_plain(fields);
    let name = crate::Codegen::Tuples::tuple_struct_name(&plain);
    let fields = fields
        .iter()
        .zip(values.iter())
        .map(|((field, _), value)| format!("{}: {}", mangle(field), value))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{name} {{ {fields} }}")
}

fn compute_gradient_type(method: &str, ret_ty: &Type) -> Option<Type> {
    match method {
        "gradient" => Some(ret_ty.clone()),
        "value_and_gradient" => match ret_ty {
            Type::Tuple(fields) => fields
                .iter()
                .find(|(name, _)| name == "gradients")
                .map(|(_, ty)| (**ty).clone()),
            _ => None,
        },
        "vjp" => match ret_ty {
            Type::Apply { name, args } if name == "VjpRun" && args.len() == 1 => {
                Some(args[0].clone())
            }
            _ => None,
        },
        _ => None,
    }
}

fn compute_result_type<'a>(ret_ty: &'a Type, transform: bool) -> Option<&'a Type> {
    if !transform {
        return Some(ret_ty);
    }
    match ret_ty {
        Type::Fn {
            ret: Some(result),
            ..
        } => Some(result),
        _ => None,
    }
}

fn compute_gradient_tuple(gradient_ty: &Type, values: &str) -> String {
    let Type::Tuple(fields) = gradient_ty else {
        return "()".to_string();
    };
    let values = (0..fields.len())
        .map(|index| format!("({values})[{index}].clone()"))
        .collect::<Vec<_>>();
    compute_tuple_value(gradient_ty, &values)
}

fn compute_nested_gradient(
    gradient_ty: &Type,
    output_ty: &Type,
    output: &str,
    tape: &str,
    targets: &str,
) -> Option<String> {
    let Type::Tuple(output_fields) = output_ty else {
        return None;
    };
    let Type::Tuple(gradient_fields) = gradient_ty else {
        return None;
    };
    if gradient_fields.len() == 0
        || gradient_fields.iter().any(|(_, ty)| !matches!(ty.as_ref(), Type::Tuple(_)))
        || gradient_fields
            .iter()
            .any(|(_, ty)| matches!(ty.as_ref(), Type::Tuple(fields) if fields.len() != output_fields.len()))
    {
        return None;
    }
    let state_defs = output_fields
        .iter()
        .enumerate()
        .map(|(index, (field, _))| {
            jet_format!(
                "let {jet_prefix}state_{index} = jet_compute_vjp_begin((({output}).{}).clone(), ({tape}).clone());",
                mangle(field)
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let state_names = output_fields
        .iter()
        .enumerate()
        .map(|(index, _)| jet_format!("{jet_prefix}state_{index}"))
        .collect::<Vec<_>>();
    let gradient_defs = jet_format!(
        "let {jet_prefix}nested_gradients = jet_compute_nested_gradient_or_panic(&[{}], &{}, \"compute.gradient\");",
        state_names.join(", "),
        targets
    );
    let values = gradient_fields
        .iter()
        .enumerate()
        .map(|(target_index, (_, inner_ty))| {
            let Type::Tuple(inner_fields) = inner_ty.as_ref() else {
                return String::new();
            };
            let inner_values = inner_fields
                .iter()
                .enumerate()
                .map(|(component_index, _)| {
                    jet_format!(
                        "({jet_prefix}nested_gradients[{component_index}])[{target_index}].clone()"
                    )
                })
                .collect::<Vec<_>>();
            compute_tuple_value(inner_ty, &inner_values)
        })
        .collect::<Vec<_>>();
    Some(format!(
        "{{ {state_defs} {gradient_defs} {} }}",
        compute_tuple_value(gradient_ty, &values)
    ))
}

fn emit_compute_transform_call(
    method: &str,
    args: &[TExpr],
    ret_ty: &Type,
    cx: &Cx,
) -> Option<String> {
    if !matches!(method, "gradient" | "value_and_gradient" | "vjp" | "jvp")
        || args.len() < 2
    {
        return None;
    }
    let Type::Fn {
        params: base_params,
        ret: Some(base_ret),
        ..
    } = &args[0].ty
    else {
        return None;
    };
    let transform = args.len() == 2;
    let value_count = args.len().saturating_sub(2);
    let primal_count = if method == "jvp" {
        if !transform && value_count % 2 != 0 {
            return None;
        }
        base_params.len()
    } else {
        base_params.len()
    };
    if !transform && value_count != if method == "jvp" {
        primal_count.saturating_mul(2)
    } else {
        primal_count
    } {
        return None;
    }
    let f = emit_tir_expr(&args[0], cx);
    let targets = emit_tir_expr(args.last()?, cx);
    let base_call = |base: &str, inputs: &str| {
        let call_args = (0..primal_count)
            .map(|index| {
                let input = format!("({inputs})[{index}]");
                if base_params[index].is_scalar() {
                    format!("({input}).clone()")
                } else {
                    format!("&{input}")
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        format!("({base})({call_args})")
    };
    let result_ty = compute_result_type(ret_ty, transform)?;
    let gradient_ty = compute_gradient_type(method, result_ty);
    let nested_gradient = method == "gradient"
        && matches!(base_ret.as_ref(), Type::Tuple(fields) if fields.iter().all(|(_, ty)| matches!(ty.as_ref(), Type::Named(name) if name == "Tensor")));
    let result_body = |output: &str, state: &str, tape: &str, target_expr: &str| -> Option<String> {
        match method {
            "gradient" => {
                let gradient_ty = gradient_ty.as_ref()?;
                if nested_gradient {
                    return compute_nested_gradient(
                        gradient_ty,
                        base_ret,
                        output,
                        tape,
                        target_expr,
                    );
                }
                Some(jet_format!(
                    "{{ let {jet_prefix}result = jet_compute_transform_or_panic(\"gradient\", &{state}, &[], &{target_expr}, \"compute.gradient\"); let JetComputeTransformResult::Gradient({jet_prefix}gradients) = {jet_prefix}result else {{ jet_panic(\"Compute.rs\", line!(), \"compute.gradient returned the wrong result\") }}; {} }}",
                    compute_gradient_tuple(gradient_ty, &mangle_generated("gradients"))
                ))
            }
            "value_and_gradient" => {
                let gradient_ty = gradient_ty.as_ref()?;
                Some(jet_format!(
                    "{{ let {jet_prefix}result = jet_compute_transform_or_panic(\"value_and_gradient\", &{state}, &[], &{target_expr}, \"compute.value_and_gradient\"); let JetComputeTransformResult::ValueAndGradient {{ value: {jet_prefix}value, gradients: {jet_prefix}gradients }} = {jet_prefix}result else {{ jet_panic(\"Compute.rs\", line!(), \"compute.value_and_gradient returned the wrong result\") }}; {} }}",
                    compute_tuple_value(
                        result_ty,
                        &[
                            mangle_generated("value"),
                            compute_gradient_tuple(gradient_ty, &mangle_generated("gradients")),
                        ],
                    )
                ))
            }
            "vjp" => {
                let gradient_ty = gradient_ty.as_ref()?;
                Some(jet_format!(
                    "{{ let {jet_prefix}result = jet_compute_transform_or_panic(\"vjp\", &{state}, &[], &{target_expr}, \"compute.vjp\"); let JetComputeTransformResult::Vjp {{ value: {jet_prefix}vjp_value, state: {jet_prefix}vjp_state }} = {jet_prefix}result else {{ jet_panic(\"Compute.rs\", line!(), \"compute.vjp returned the wrong result\") }}; let {jet_prefix}pull_state = {jet_prefix}vjp_state.clone(); let {jet_prefix}grads_state = {jet_prefix}vjp_state; let {jet_prefix}pull_targets = {target_expr}.clone(); let {jet_prefix}grads_targets = {target_expr}.clone(); JetComputeVjpRun {{ value: {jet_prefix}vjp_value, pull: std::rc::Rc::new(move |{jet_prefix}seed: &JetTensor| {{ let {jet_prefix}gradients = jet_compute_vjp_pull_or_panic(&{jet_prefix}pull_state, {jet_prefix}seed, &{jet_prefix}pull_targets, \"compute.vjp.pull\"); {} }}), grads: std::rc::Rc::new(move || {{ let {jet_prefix}gradients = jet_compute_vjp_unit_grads_or_panic(&{jet_prefix}grads_state, &{jet_prefix}grads_targets, \"compute.vjp.grads\"); {} }}) }} }}",
                    compute_gradient_tuple(gradient_ty, &mangle_generated("gradients")),
                    compute_gradient_tuple(gradient_ty, &mangle_generated("gradients"))
                ))
            }
            "jvp" => {
                let tangents = if transform {
                    (0..primal_count)
                        .map(|index| {
                            let arg = index + primal_count;
                            if base_params[index].is_scalar() {
                                jet_format!("{jet_prefix}arg{arg}.clone()")
                            } else {
                                jet_format!("(*{jet_prefix}arg{arg}).clone()")
                            }
                        })
                        .collect::<Vec<_>>()
                } else {
                    (0..primal_count)
                        .map(|index| emit_tir_expr(&args[index + 1 + primal_count], cx))
                        .collect::<Vec<_>>()
                };
                Some(jet_format!(
                    "{{ let {jet_prefix}result = jet_compute_transform_or_panic(\"jvp\", &{state}, &[{}], &{target_expr}, \"compute.jvp\"); let JetComputeTransformResult::Jvp {{ value: {jet_prefix}value, tangent: {jet_prefix}tangent }} = {jet_prefix}result else {{ jet_panic(\"Compute.rs\", line!(), \"compute.jvp returned the wrong result\") }}; {} }}",
                    tangents.join(", "),
                    compute_tuple_value(
                        result_ty,
                        &[
                            mangle_generated("value"),
                            mangle_generated("tangent"),
                        ]
                    )
                ))
            }
            _ => None,
        }
    };
    let body = if transform {
        let params = if method == "jvp" {
            base_params
                .iter()
                .chain(base_params.iter())
                .enumerate()
                .map(|(index, ty)| {
                    jet_format!(
                        "{jet_prefix}arg{index}: {}",
                        rust_param_type(cx, AccessConvention::Read, ty)
                    )
                })
                .collect::<Vec<_>>()
        } else {
            base_params
                .iter()
                .enumerate()
                .map(|(index, ty)| {
                    jet_format!(
                        "{jet_prefix}arg{index}: {}",
                        rust_param_type(cx, AccessConvention::Read, ty)
                    )
                })
                .collect::<Vec<_>>()
        };
        let target = jet_format!("{jet_prefix}targets");
        let state_setup = if nested_gradient {
            String::new()
        } else {
            jet_format!(
                "let {jet_prefix}state = jet_compute_vjp_begin({jet_prefix}value.clone(), {jet_prefix}tape.clone());"
            )
        };
        let result = result_body(
            &mangle_generated("value"),
            &mangle_generated("state"),
            &mangle_generated("tape"),
            &target,
        )?;
        Some(jet_format!(
            "{{ let {jet_prefix}base = ({f}).clone(); std::rc::Rc::new(move |{}| {{ let ({jet_prefix}tape, {jet_prefix}inputs) = jet_compute_trace_inputs(vec![{}]); let {jet_prefix}value = ({jet_prefix}base)({}); {state_setup} let {jet_prefix}targets = {}; {} }}) as {} }}",
            params.join(", "),
            (0..primal_count)
                .map(|index| {
                    if base_params[index].is_scalar() {
                        jet_format!("{jet_prefix}arg{index}.clone()")
                    } else {
                        jet_format!("(*{jet_prefix}arg{index}).clone()")
                    }
                })
                .collect::<Vec<_>>()
                .join(", "),
            (0..primal_count)
                .map(|index| {
                    let input = jet_format!("({jet_prefix}inputs)[{index}]");
                    if base_params[index].is_scalar() {
                        format!("({input}).clone()")
                    } else {
                        format!("&{input}")
                    }
                })
                .collect::<Vec<_>>()
                .join(", "),
            targets,
            result,
            cx.rust_type(ret_ty)
        ))
    } else {
        let trace_inputs = (0..primal_count)
            .map(|index| format!("({}).clone()", emit_tir_expr(&args[index + 1], cx)))
            .collect::<Vec<_>>()
            .join(", ");
        let call = base_call(&f, &mangle_generated("inputs"));
        let state_setup = if nested_gradient {
            String::new()
        } else {
            jet_format!(
                "let {jet_prefix}state = jet_compute_vjp_begin({jet_prefix}value.clone(), {jet_prefix}tape.clone());"
            )
        };
        let result = result_body(
            &mangle_generated("value"),
            &mangle_generated("state"),
            &mangle_generated("tape"),
            &mangle_generated("targets"),
        )?;
        Some(jet_format!(
            "{{ let ({jet_prefix}tape, {jet_prefix}inputs) = jet_compute_trace_inputs(vec![{trace_inputs}]); let {jet_prefix}value = {call}; {state_setup} let {jet_prefix}targets = {targets}; {result} }}"
        ))
    };
    body
}

fn emit_data_schema_columns(elem_ty: &Type, expand_struct: bool, cx: &Cx) -> String {
    let column = |name: &str, type_name: &str| {
        format!(
            "{root}jet_std::DataColumn {{ name: {name}.to_string(), type_name: {ty}.to_string() }}",
            root = cx.root_prefix,
            name = escape_rust_str(name),
            ty = escape_rust_str(type_name),
        )
    };
    // Series schema is always one `value` column (docs / D-DATAFRAME1). Table /
    // LazyFrame / `[T]` expand a Named row struct into its fields.
    if !expand_struct {
        return format!("vec![{}]", column("value", &elem_ty.name()));
    }
    let Some(struct_name) = elem_ty.base_name() else {
        return format!("vec![{}]", column("value", &elem_ty.name()));
    };
    match cx.struct_fields.get(struct_name) {
        Some(fields) => {
            let items: Vec<String> = fields
                .iter()
                .map(|(fname, _)| {
                    let field_ty = struct_field_type(cx, elem_ty, fname)
                        .expect("registered struct field must have a concrete type");
                    column(fname, &field_ty.name())
                })
                .collect();
            if items.is_empty() {
                format!("Vec::<{}jet_std::DataColumn>::new()", cx.root_prefix)
            } else {
                format!("vec![{}]", items.join(", "))
            }
        }
        None => format!("vec![{}]", column("value", &elem_ty.name())),
    }
}

fn data_schema_elem_ty(arg_ty: &Type) -> Option<&Type> {
    match arg_ty {
        Type::List(inner) => Some(inner.as_ref()),
        Type::Apply { name, args }
            if matches!(name.as_str(), "Table" | "Series" | "LazyFrame") && args.len() == 1 =>
        {
            Some(&args[0])
        }
        _ => None,
    }
}

fn data_schema_expand_struct(arg_ty: &Type) -> bool {
    !matches!(arg_ty, Type::Apply { name, .. } if name == "Series")
}

pub(crate) fn emit_http_bridge_error(ffi: &str, error: &str) -> String {
    format!(
        "match {error} {{ \
         {ffi}::JetHTTPBridgeError::InvalidUrl => JetHTTPError::InvalidUrl, \
         {ffi}::JetHTTPBridgeError::InvalidHeader => JetHTTPError::InvalidHeader, \
         {ffi}::JetHTTPBridgeError::InvalidFraming => JetHTTPError::InvalidFraming, \
         {ffi}::JetHTTPBridgeError::UnsupportedEncoding => JetHTTPError::UnsupportedEncoding, \
         {ffi}::JetHTTPBridgeError::Resolve => JetHTTPError::Resolve {{ host: \"<redacted>\".to_string() }}, \
         {ffi}::JetHTTPBridgeError::Connect => JetHTTPError::Connect {{ address: \"<redacted>\".to_string() }}, \
         {ffi}::JetHTTPBridgeError::TLS => JetHTTPError::TLS {{ stage: \"handshake\".to_string() }}, \
         {ffi}::JetHTTPBridgeError::Timeout => JetHTTPError::Timeout {{ phase: \"transport\".to_string() }}, \
         {ffi}::JetHTTPBridgeError::Proxy => JetHTTPError::Proxy {{ stage: \"transport\".to_string() }}, \
         {ffi}::JetHTTPBridgeError::Redirect => JetHTTPError::Redirect {{ reason: \"limit\".to_string() }}, \
         {ffi}::JetHTTPBridgeError::Protocol => JetHTTPError::Protocol {{ version: \"unsupported\".to_string() }}, \
         {ffi}::JetHTTPBridgeError::IO => JetHTTPError::IO {{ operation: \"transport\".to_string() }}, \
         {ffi}::JetHTTPBridgeError::ResourceUnavailable => JetHTTPError::ResourceUnavailable {{ resource: \"transport\".to_string() }}, \
         {ffi}::JetHTTPBridgeError::Cancelled => JetHTTPError::Cancelled, \
         {ffi}::JetHTTPBridgeError::UnsupportedTarget => JetHTTPError::UnsupportedTarget {{ operation: JetHTTPOperation::ClientConnect }}, \
         {ffi}::JetHTTPBridgeError::Internal => JetHTTPError::Internal {{ incident_id: \"http-transport\".to_string() }} }}"
    )
}

pub(crate) fn emit_http_response_from_bridge(call: String, ffi: &str) -> String {
    let error = emit_http_bridge_error(ffi, "error");
    let read_error = emit_http_bridge_error(ffi, "error");
    format!(
        "({call}).map_err(|error| {error}).and_then(|(status, handle, length, headers)| {{ \
         let protocol = {ffi}::jet_http_client_response_protocol_impl(handle); \
         let remote_address = {ffi}::jet_http_client_response_remote_address_impl(handle); \
         let redirect_history = {ffi}::jet_http_client_response_redirect_history_impl(handle); \
         let timings = {ffi}::jet_http_client_response_timings_impl(handle); \
         let reused = {ffi}::jet_http_client_response_reused_impl(handle); \
         let raw_encoding = {ffi}::jet_http_client_response_raw_encoding_impl(handle); \
         {ffi}::jet_http_client_response_facts_drop_impl(handle); \
         jet_http_client_response_new(status, handle, length, headers, \
         |handle, max_chunk| {ffi}::jet_http_client_body_read_impl(handle, max_chunk).map_err(|error| {read_error}), \
         {ffi}::jet_http_client_body_close_impl, protocol, remote_address, redirect_history, timings, reused, raw_encoding) }})"
    )
}

/// Project a plain Core call from the foundation registry before bespoke emit.
fn emit_plain_core_call(
    module: &str,
    method: &str,
    args: &[TExpr],
    arg: &dyn Fn(usize) -> String,
    helper: &dyn Fn(&str) -> String,
) -> Option<String> {
    let row = crate::Syntax::core_call(module, method)?;
    if !row.aot_direct {
        return None;
    }
    let rendered: Vec<String> = row
        .signature
        .borrow_mask
        .iter()
        .enumerate()
        .map(|(idx, borrow)| {
            let a = arg(idx);
            let a = if row.path_arg(idx)
                && args.get(idx).is_some_and(|arg| {
                    matches!(&arg.ty, Type::Named(name) if name == "Path")
                })
            {
                format!("({a}).jet_show()")
            } else {
                a
            };
            if *borrow { format!("&({a})") } else { a }
        })
        .collect();
    let sym = match row.symbol {
        crate::Syntax::CoreCallSymbol::Prelude(symbol) => helper(symbol),
        crate::Syntax::CoreCallSymbol::Rust(symbol) => symbol.to_string(),
    };
    Some(emit_symbol_call(&sym, &rendered.join(", ")))
}

pub(crate) fn emit_tir_core_call(
    module: &str,
    method: &str,
    args: &[TExpr],
    widen_to_vec: &[bool],
    ret_ty: &Type,
    cx: &Cx,
) -> String {
    let arg = |i: usize| {
        let rendered = args.get(i)
            .map(|e| emit_tir_expr(e, cx))
            .unwrap_or_default();
        if widen_to_vec.get(i).copied().unwrap_or(false) {
            format!("({rendered}).to_vec()")
        } else {
            rendered
        }
    };
    let helper = |name: &str| format!("{}{}", cx.root_prefix, name);
    let regex_fn = |name: &str| {
        let crate_name = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
        format!("{}::{}", crate_name, name)
    };
    let email_runtime = || format!(
        "{}jet_email::RuntimeFns {{ tls_begin: {}, tls_begin_ca: {}, tls_handshake_step: {}, tls_set_poll_timeout: {}, tls_read: {}, tls_write_all: {}, tls_close: {}, wipe: {}, sha256: {}, ed25519_sign: {}, cancelled: jet_scheduler_task_cancelled, remaining_ms: jet_deadline_remaining_ms, accepted_at: {}jet_email::runtime_now }}",
        cx.root_prefix,
        regex_fn("jet_net_tls_begin_impl"),
        regex_fn("jet_net_tls_begin_with_ca_impl"),
        regex_fn("jet_net_tls_handshake_step_impl"),
        regex_fn("jet_net_tls_set_poll_timeout_impl"),
        regex_fn("jet_net_tls_read_bytes_impl"),
        regex_fn("jet_net_tls_write_all_bytes_impl"),
        regex_fn("jet_net_tls_close_impl"),
        regex_fn("jet_crypto_zeroize_email_impl"),
        regex_fn("jet_crypto_email_sha256_impl"),
        regex_fn("jet_crypto_email_ed25519_sign_impl"),
        cx.root_prefix,
    );
    if module == "core.encoding" && method == "__published_schema_empty" {
        return format!("{}jet_std::DataTree::Object(Vec::new())", cx.root_prefix);
    }
    if module == "core.encoding" && method == "__published_schema_merge" {
        return format!(
            "{}jet_std::jet_datatree_merge_wire_order(&({}), &({}))",
            cx.root_prefix,
            arg(0),
            arg(1)
        );
    }
    if module == "core.compute" {
        if let Some(rendered) = emit_compute_transform_call(method, args, ret_ty, cx) {
            return rendered;
        }
    }
    if let Some(rendered) = emit_plain_core_call(module, method, args, &arg, &helper) {
        return rendered;
    }
    fn vault_key_type(ty: &Type) -> Option<&str> {
        match ty {
            Type::Named(name) if matches!(name.as_str(), "SigningKey" | "X25519SecretKey") => Some(name),
            Type::Tagged { inner, .. } => vault_key_type(inner),
            Type::Apply { args, .. } => args.iter().find_map(vault_key_type),
            Type::Option(inner) | Type::List(inner) => vault_key_type(inner),
            Type::Result { ok, .. } => vault_key_type(ok),
            _ => None,
        }
    }
    let vault_rust = || match vault_key_type(ret_ty) {
        Some("SigningKey") => format!("{}::JetSigningKey", cx.ffi_crate.as_deref().unwrap_or("jet_ffi")),
        Some("X25519SecretKey") => format!("{}::JetX25519SecretKey", cx.ffi_crate.as_deref().unwrap_or("jet_ffi")),
        _ => "_".to_string(),
    };
    // #1635: every arm below stays bespoke because it needs at least one thing
    // the foundation Core-call row can't express -- a match guard or duplicate
    // (module, method) key (order-sensitive dispatch), a dynamic Rust type
    // parameter (`cx.rust_type(...)`), tuple/struct construction with
    // generated field names, conditional branching on `args`/`ret_ty` (arg
    // count, arg type, f32 vs f64 paths, ...), `Option`-wrapped or defaulted
    // args, a method-call or operator spelling instead of a bare symbol call
    // (`(x).foo()`, `x % 2`), or other call-site logic (HTTP/bridge/vault
    // error mapping, `method` used inside the template, multi-statement
    // setup). Each is exactly one of those; see the arm itself for which.
    match (module, method) {
        ("jet.unit", "magnitude") => format!("({}).to_string()", arg(0)),
        // c109 Phase 18 (S58, E2-M13): low-level pointer ops, byte-for-byte
        // TIR core-call emission. `address_of` is an inert address cast (no `unsafe`);
        // `volatile_read`/`volatile_write` access through a `Ptr<T>` — the volatile ops are
        // valid because the call only reaches codegen inside an `#Unsafe` region/fn (sema
        // E3101), already lowered to a Rust `unsafe` context.
        ("core.mem", "address_of") => {
            let place = arg(0);
            let mutable = args.first().is_some_and(|expr| {
                matches!(&expr.kind, TExprKind::Local(local) if local.mutable)
            });
            if mutable {
                format!("(&mut ({place}) as *mut _ as usize as i64)")
            } else {
                format!("(&({place}) as *const _ as usize as i64)")
            }
        }
        ("core.mem", "volatile_read") => {
            format!("std::ptr::read_volatile({})", arg(0))
        }
        ("core.mem", "volatile_write") => {
            format!("std::ptr::write_volatile({}, {})", arg(0), arg(1))
        }
        
        ("core.tasks", "channel") => {
            let fields = match ret_ty {
                Type::Tuple(fields) => crate::Codegen::Tuples::tuple_fields_plain(fields),
                _ => Vec::new(),
            };
            let elem = fields
                .first()
                .and_then(|(_, t)| match t {
                    Type::Apply { args, .. } => args.first().cloned(),
                    _ => None,
                })
                .unwrap_or(Type::Int);
            let struct_name = crate::Codegen::Tuples::tuple_struct_name(&fields);
            let ctor = if args.is_empty() {
                format!(
                    "{}jet_std::channel::<{}>()",
                    cx.root_prefix,
                    cx.rust_type(&elem)
                )
            } else {
                format!(
                    "{}jet_std::channel_bounded::<{}>({})",
                    cx.root_prefix,
                    cx.rust_type(&elem),
                    arg(0)
                )
            };
            jet_format!(
                "{{ let {jet_prefix}ch = {}; {} {{ {}: {jet_prefix}ch.0, {}: {jet_prefix}ch.1 }} }}",
                ctor,
                struct_name,
                mangle("sender"),
                mangle("receiver"),
            )
        }
        ("core.tasks", "after") => {
            if args.len() == 1 {
                format!("{}jet_std::after({})", cx.root_prefix, arg(0))
            } else {
                format!(
                    "{}jet_std::after_value({}, {})",
                    cx.root_prefix,
                    arg(0),
                    arg(1)
                )
            }
        }
        
        
        
        ("core.event", "new") => {
            let elem = match ret_ty {
                Type::Apply { args, .. } => args.first().cloned().unwrap_or(Type::Int),
                _ => Type::Int,
            };
            format!(
                "{}jet_std::JetEvent::<{}>::new()",
                cx.root_prefix,
                cx.rust_type(&elem)
            )
        }
        ("core.event", "with_policy") => {
            let elem = match ret_ty {
                Type::Apply { args, .. } => args.first().cloned().unwrap_or(Type::Int),
                _ => Type::Int,
            };
            format!(
                "{}jet_std::JetEvent::<{}>::with_policy({})",
                cx.root_prefix,
                cx.rust_type(&elem),
                arg(0)
            )
        }
        ("core.event", "async_result") => {
            let (payload, error) = match ret_ty {
                Type::Result { ok, .. } => match ok.as_ref() {
                    Type::Apply { args, .. } if args.len() >= 2 => (args[0].clone(), args[1].clone()),
                    _ => (Type::Int, Type::String),
                },
                _ => (Type::Int, Type::String),
            };
            format!(
                "{}jet_std::JetAsyncEvent::<{}, {}>::new({}, {})",
                cx.root_prefix,
                cx.rust_type(&payload),
                cx.rust_type(&error),
                arg(0),
                arg(1)
            )
        }
        ("core.event", "hook") => {
            let (payload, result) = match ret_ty {
                Type::Apply { args, .. } if args.len() >= 2 => (args[0].clone(), args[1].clone()),
                _ => (Type::Int, Type::Int),
            };
            format!(
                "{}jet_std::JetHook::<{}, {}>::new({})",
                cx.root_prefix,
                cx.rust_type(&payload),
                cx.rust_type(&result),
                arg(0)
            )
        }
        ("core.event", "decision_hook") => {
            let (payload, error) = match ret_ty {
                Type::Apply { args, .. } if args.len() >= 2 => (args[0].clone(), args[1].clone()),
                _ => (Type::Int, Type::String),
            };
            format!(
                "{}jet_std::JetDecisionHook::<{}, {}>::new({})",
                cx.root_prefix,
                cx.rust_type(&payload),
                cx.rust_type(&error),
                arg(0)
            )
        }
        // D-HONESTNUM1=A: `M.from(value, uncertainty)` → a `JetMeasurement<f64>`.
        
        ("core.reactive.loadable", "idle") => format!("JetLoadable::<(), ()>::Idle"),
        ("core.reactive.loadable", "loading") => format!("JetLoadable::<(), ()>::Loading"),
        ("core.reactive.loadable", "loaded") => {
            format!("JetLoadable::<_, ()>::Loaded({})", arg(0))
        }
        ("core.reactive.loadable", "failed") => {
            format!("JetLoadable::<(), _>::Failed({})", arg(0))
        }
        // D-FILES-WRITE1 (merge, was `core.fs`): whole-file convenience helpers now
        // live in `core.files` alongside the streaming handle constructors below.
        // D-FILES-APPEND1=A: whole-file one-shot is `append_all`, not `append` —
        // that name stays reserved for the streaming handle's `.append(text)`.
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        // D-ARGS1: `args.spec()` → empty builder.
        
        // D-ANY-JAI1 (c7jaiany §6): `reflect.of(x)` — built entirely at this call
        // site (no generic runtime trait needed, I3: sema already gated
        // legality via `is_displayable` in `CheckerCoreLib::infer_core_call`,
        // the SAME check `"{x}"` interpolation uses). `x` is bound once
        // (`__reflect_v`) so a side-effecting argument expression isn't
        // evaluated twice. `.display()` calls `jet_display()` (JetDisplay) —
        // never `jet_show()`/`{:?}` — so it shows exactly what `"{x}"` would,
        // never codegen's mangled Rust field names. `.fields()`'s per-field
        // values are nested `Value` handles, so the same statically registered
        // field rows project recursively instead of discarding their types as
        // strings.
        ("core.reflect", "of") => {
            let arg_ty = args.first().map(|a| &a.ty);
            let type_name = arg_ty.map(Type::leaf_name).unwrap_or_default();
            let path = arg_ty.map(|t| cx.reflect_path(t)).unwrap_or_default();
            let fields_code = match arg_ty {
                Some(owner_ty @ (Type::Named(struct_name) | Type::Apply { name: struct_name, .. })) => cx
                    .struct_fields
                    .get(struct_name)
                    .map(|fields| {
                        fields
                            .iter()
                            .map(|(field_name, declared_ty)| {
                                let field_ty = reflect_field_type(cx, owner_ty, declared_ty);
                                let field_value = format!("(__reflect_v).{}", mangle(field_name));
                                format!(
                                    "{}JetReflectField {{ name: {}.to_string(), value: {} }}",
                                    cx.root_prefix,
                                    escape_rust_str(field_name),
                                    reflect_nested_value(cx, &field_ty, &field_value)
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .map(|fields| format!("vec![{fields}]"))
                    .unwrap_or_else(|| "Vec::new()".to_string()),
                _ => "Vec::new()".to_string(),
            };
            format!(
                "{{ let __reflect_v = &({arg0}); {root}JetReflectValue {{ type_name: {type_name}.to_string(), path: {path}.to_string(), display: __reflect_v.jet_display(), fields: {fields_code} }} }}",
                arg0 = arg(0),
                root = cx.root_prefix,
                type_name = escape_rust_str(&type_name),
                path = escape_rust_str(&path),
                fields_code = fields_code
            )
        }
        // c109 Phase 29: qualified `io.input(prompt)`: no arg → `jet_std_io_input(None)`;
        // a prompt arg →
        // `jet_std_io_input(Some(&(prompt)))`. Same emitted helper as the ambient bare
        // `input(...)` (Phase 25), the only difference being the source node shape.
        ("core.io", "input") => {
            if args.is_empty() {
                format!("{}(None)", helper("jet_std_io_input"))
            } else {
                format!("{}(Some(&({})))", helper("jet_std_io_input"), arg(0))
            }
        }
        
        
        
        
        
        
        
        
        
        // D-STDIN1=A: io.stdin() → JetStdinReader handle.
        
        
        
        
        
        ("core.io", "progress") => {
            if matches!(args.first().map(|a| &a.ty), Some(Type::String)) {
                format!("{}(&({}))", helper("jet_std_io_progress"), arg(0))
            } else {
                let description = args
                    .get(1)
                    .map(|_| format!("&({})", arg(1)))
                    .unwrap_or_else(|| "&\"Progress\".to_string()".to_string());
                let format = args
                    .get(2)
                    .map(|_| format!("&({})", arg(2)))
                    .unwrap_or_else(|| "&String::new()".to_string());
                let helper_name = match args.first().map(|a| &a.ty) {
                    Some(Type::Apply { name, .. }) if name == crate::Syntax::TYPE_ITER => {
                        "jet_std_io_progress_iter"
                    }
                    _ => "jet_std_io_progress_list",
                };
                format!("{}({}, {}, {})", helper(helper_name), arg(0), description, format)
            }
        }
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        // D-FLOATW1: width-generic math — choose the f32 helper when the arg is F32.
        ("core.math", "sqrt") => {
            let f32_path = matches!(args.first().map(|a| &a.ty), Some(Type::Float32));
            if f32_path {
                format!("{}({})", helper("jet_std_math_sqrt_f32"), arg(0))
            } else {
                format!("{}({})", helper("jet_std_math_sqrt"), arg(0))
            }
        }
        ("core.math", "pow") => {
            let f32_path = matches!(args.first().map(|a| &a.ty), Some(Type::Float32));
            if f32_path {
                format!("{}({}, {})", helper("jet_std_math_pow_f32"), arg(0), arg(1))
            } else {
                format!("{}({}, {})", helper("jet_std_math_pow"), arg(0), arg(1))
            }
        }
        ("core.math", "floor") => {
            let f32_path = matches!(args.first().map(|a| &a.ty), Some(Type::Float32));
            if f32_path {
                format!("{}({})", helper("jet_std_math_floor_f32"), arg(0))
            } else {
                format!("{}({})", helper("jet_std_math_floor"), arg(0))
            }
        }
        ("core.math", "ceil") => {
            let f32_path = matches!(args.first().map(|a| &a.ty), Some(Type::Float32));
            if f32_path {
                format!("{}({})", helper("jet_std_math_ceil_f32"), arg(0))
            } else {
                format!("{}({})", helper("jet_std_math_ceil"), arg(0))
            }
        }
        
        (
            "core.math",
            "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh" | "cosh" | "tanh"
            | "exp" | "ln" | "log2" | "log10" | "trunc" | "fract"
            | "acosh" | "asinh" | "atanh" | "cbrt" | "exp2" | "signum",
        ) => format!("({}).{}()", arg(0), method),
        // Rust spells these exp_m1 and ln_1p; Jet keeps the same names, so the
        // call is the identity.
        ("core.math", "exp_m1" | "ln_1p") => format!("({}).{}()", arg(0), method),
        ("core.math", "copysign" | "log") => format!("({}).{}({})", arg(0), method, arg(1)),
        ("core.math", "fma") => {
            format!("({}).mul_add({}, {})", arg(0), arg(1), arg(2))
        }
        
        
        ("core.math", "checked_abs") => format!("({}).checked_abs()", arg(0)),
        ("core.math", "checked_neg") => format!("({}).checked_neg()", arg(0)),
        ("core.math", "checked_div") => format!("({}).checked_div({})", arg(0), arg(1)),
        ("core.math", "checked_rem") => format!("({}).checked_rem({})", arg(0), arg(1)),
        ("core.math", "is_even") => format!("(({}) % 2 == 0)", arg(0)),
        ("core.math", "is_odd") => format!("(({}) % 2 != 0)", arg(0)),
        ("core.math", "is_normal") => format!("({}).is_normal()", arg(0)),
        ("core.math", "is_subnormal") => format!("({}).is_subnormal()", arg(0)),
        ("core.math", "is_canonical") => {
            format!("(({}).is_finite() || ({}).is_nan())", arg(0), arg(0))
        }
        ("core.math", "is_signed" | "sign_bit") => format!("({}).is_sign_negative()", arg(0)),
        ("core.math", "is_zero") => format!("({} == 0.0)", arg(0)),
        ("core.math", "is_integer") => {
            format!("(({}).is_finite() && ({}).fract() == 0.0)", arg(0), arg(0))
        }
        ("core.math", "next_up") => format!("({}).next_up()", arg(0)),
        ("core.math", "next_down") => format!("({}).next_down()", arg(0)),
        ("core.math", "copy") => format!("({})", arg(0)),
        ("core.math", "cot") => format!("(1.0 / ({}).tan())", arg(0)),
        ("core.math", "inv") => format!("(1.0 / ({}))", arg(0)),
        ("core.math", "zero") => "0.0_f64".to_string(),
        ("core.math", "radix") => "2i64".to_string(),
        
        
        
        
        
        
        
        
        
        ("core.math", "sin_cos") => {
            let fields = match ret_ty {
                Type::Tuple(fields) => crate::Codegen::Tuples::tuple_fields_plain(fields),
                _ => vec![
                    ("sin".to_string(), Type::Float),
                    ("cos".to_string(), Type::Float),
                ],
            };
            let struct_name = crate::Codegen::Tuples::tuple_struct_name(&fields);
            jet_format!(
                "{{ let {jet_prefix}sc = ({}).sin_cos(); {} {{ {}: {jet_prefix}sc.0, {}: {jet_prefix}sc.1 }} }}",
                arg(0),
                struct_name,
                mangle("sin"),
                mangle("cos"),
            )
        }
        ("core.math", "modf") => {
            let fields = match ret_ty {
                Type::Tuple(fields) => crate::Codegen::Tuples::tuple_fields_plain(fields),
                _ => vec![
                    ("fract".to_string(), Type::Float),
                    ("whole".to_string(), Type::Float),
                ],
            };
            let struct_name = crate::Codegen::Tuples::tuple_struct_name(&fields);
            jet_format!(
                "{{ let {jet_prefix}x = ({0}); {1} {{ {2}: {jet_prefix}x.fract(), {3}: {jet_prefix}x.trunc() }} }}",
                arg(0),
                struct_name,
                mangle("fract"),
                mangle("whole"),
            )
        }
        ("core.math", "frexp") => {
            let fields = match ret_ty {
                Type::Tuple(fields) => crate::Codegen::Tuples::tuple_fields_plain(fields),
                _ => vec![
                    ("frac".to_string(), Type::Float),
                    ("exp".to_string(), Type::Int),
                ],
            };
            let struct_name = crate::Codegen::Tuples::tuple_struct_name(&fields);
            jet_format!(
                "{{ let {jet_prefix}x = ({0}); let {jet_prefix}e = {1}jet_std_math_ilogb({jet_prefix}x).unwrap_or(0); let {jet_prefix}f = if {jet_prefix}x == 0.0 || !{jet_prefix}x.is_finite() {{ {jet_prefix}x }} else {{ {1}jet_std_math_ldexp({jet_prefix}x, -{jet_prefix}e) }}; {2} {{ {3}: {jet_prefix}f, {4}: {jet_prefix}e }} }}",
                arg(0),
                cx.root_prefix,
                struct_name,
                mangle("frac"),
                mangle("exp"),
            )
        }
        ("core.math", "div_mod" | "div_rem") => {
            let fields = match ret_ty {
                Type::Tuple(fields) => crate::Codegen::Tuples::tuple_fields_plain(fields),
                _ => vec![
                    ("quot".to_string(), Type::Int),
                    ("rem".to_string(), Type::Int),
                ],
            };
            let struct_name = crate::Codegen::Tuples::tuple_struct_name(&fields);
            let op = if method == "div_mod" {
                // Floor division + matching remainder (Python divmod).
                jet_format!(
                    "let {jet_prefix}a = ({0}); let {jet_prefix}b = ({1}); let {jet_prefix}q = {jet_prefix}a.div_euclid({jet_prefix}b); let {jet_prefix}r = {jet_prefix}a.rem_euclid({jet_prefix}b);",
                    arg(0),
                    arg(1)
                )
            } else {
                // Truncating division + remainder (Rust /, %).
                jet_format!(
                    "let {jet_prefix}a = ({0}); let {jet_prefix}b = ({1}); let {jet_prefix}q = {jet_prefix}a / {jet_prefix}b; let {jet_prefix}r = {jet_prefix}a % {jet_prefix}b;",
                    arg(0),
                    arg(1)
                )
            };
            jet_format!(
                "{{ {op} {struct_name} {{ {}: {jet_prefix}q, {}: {jet_prefix}r }} }}",
                mangle("quot"),
                mangle("rem"),
            )
        }
        ("core.math", "degrees") => format!("({}).to_degrees()", arg(0)),
        ("core.math", "radians") => format!("({}).to_radians()", arg(0)),
        ("core.math", "atan2" | "hypot") => format!("({}).{}({})", arg(0), method, arg(1)),
        ("core.math", "lerp") => {
            format!("(({}) + (({}) - ({})) * ({}))", arg(0), arg(1), arg(0), arg(2))
        }
        ("core.math", "is_nan") => format!("({}).is_nan()", arg(0)),
        ("core.math", "is_inf") => format!("({}).is_infinite()", arg(0)),
        ("core.math", "is_finite") => format!("({}).is_finite()", arg(0)),
        ("core.math", "sign") => format!(
            "if ({0}) > 0.0 {{ 1i64 }} else if ({0}) < 0.0 {{ -1i64 }} else {{ 0i64 }}",
            arg(0)
        ),
        ("core.math", "to_bits") => format!("(({}).to_bits() as i64)", arg(0)),
        ("core.math", "from_bits") => format!("f64::from_bits(({}) as u64)", arg(0)),
        ("core.math", "checked_add") => format!("({}).checked_add({})", arg(0), arg(1)),
        ("core.math", "checked_sub") => format!("({}).checked_sub({})", arg(0), arg(1)),
        ("core.math", "checked_mul") => format!("({}).checked_mul({})", arg(0), arg(1)),
        ("core.math", "saturating_add") => format!("({}).saturating_add({})", arg(0), arg(1)),
        ("core.math", "saturating_sub") => format!("({}).saturating_sub({})", arg(0), arg(1)),
        ("core.math", "saturating_mul") => format!("({}).saturating_mul({})", arg(0), arg(1)),
        ("core.math", "wrapping_add") => format!("({}).wrapping_add({})", arg(0), arg(1)),
        ("core.math", "wrapping_sub") => format!("({}).wrapping_sub({})", arg(0), arg(1)),
        ("core.math", "wrapping_mul") => format!("({}).wrapping_mul({})", arg(0), arg(1)),
        
        
        
        
        
        
        // D-RANDSPLIT1=A: PRNG bytes — fast, NOT crypto-safe.
        
        // D-CRYPTO-RNG1=A: shared fail-closed OS CSPRNG provider.
        
        
        
        
        
        ("core.time", "milliseconds") => format!(
            "{}({}, jet_std::DurationUnit::Milliseconds)",
            helper("jet_duration_from_int"),
            arg(0)
        ),
        ("core.time", "nanoseconds") => format!(
            "{}({}, jet_std::DurationUnit::Nanoseconds)",
            helper("jet_duration_from_int"),
            arg(0)
        ),
        ("core.time", "microseconds") => format!(
            "{}({}, jet_std::DurationUnit::Microseconds)",
            helper("jet_duration_from_int"),
            arg(0)
        ),
        ("core.time", "seconds") => format!(
            "{}({}, jet_std::DurationUnit::Seconds)",
            helper("jet_duration_from_int"),
            arg(0)
        ),
        ("core.time", "minutes") => format!(
            "{}({}, jet_std::DurationUnit::Minutes)",
            helper("jet_duration_from_int"),
            arg(0)
        ),
        ("core.time", "hours") => format!(
            "{}({}, jet_std::DurationUnit::Hours)",
            helper("jet_duration_from_int"),
            arg(0)
        ),
        
        
        
        
        
        ("core.time", "parse_time") => {
            format!("JetLocalTime::parse(&({})).map_err(|e| e)", arg(0))
        }
        
        
        
        
        
        // D-DET1: deterministic injected Clock capability constructor.
        
        ("core.game", "run") => {
            let optional_ref = |index: usize| {
                if matches!(args.get(index).map(|arg| &arg.ty), Some(Type::Option(_))) {
                    format!("({}).as_ref().ok()", arg(index))
                } else {
                    format!("Some(&({}))", arg(index))
                }
            };
            let replay = optional_ref(1);
            let backend = optional_ref(2);
            format!(
                "{root}jet_game_run(&mut ({scene}), {replay}, {backend})",
                root = cx.root_prefix,
                scene = arg(0),
                replay = replay,
                backend = backend
            )
        }
        // D-ENC1 + D-JSONVERB1 + D-SERDE6: unified `core.encoding.*`. The dynamic forms
        // (`JSON` tree / `[[String]]` / `Map`) keep their existing helpers; the typed
        // forms route through the Encode/Decode model, distinguished by the lowered arg
        // type (encode) or the resolved return type (decode). `is_json_value` etc. read
        // those total facts — codegen never re-infers (I3).
        ("core.encoding.json", "reader") => {
            let limits = if args.len() > 1 { arg(1) } else { format!("{}jet_std::EncodingLimits::safe()", cx.root_prefix) };
            format!("{}({}, {})", helper("jet_enc_json_reader"), arg(0), limits)
        }
        ("core.encoding.json", "writer") => {
            let limits = if args.len() > 1 { arg(1) } else { format!("{}jet_std::EncodingLimits::safe()", cx.root_prefix) };
            let canonical = if args.len() > 2 { arg(2) } else { "false".to_string() };
            format!("{}({}, {}, {})", helper("jet_enc_json_writer"), arg(0), limits, canonical)
        }
        ("core.encoding.jsonl", "reader") => {
            let limits = if args.len() > 1 { arg(1) } else { format!("{}jet_std::EncodingLimits::safe()", cx.root_prefix) };
            format!("{}({}, {})", helper("jet_enc_jsonl_reader"), arg(0), limits)
        }
        ("core.encoding.jsonl", "writer") => {
            let limits = if args.len() > 1 { arg(1) } else { format!("{}jet_std::EncodingLimits::safe()", cx.root_prefix) };
            format!("{}({}, {})", helper("jet_enc_jsonl_writer"), arg(0), limits)
        }
        ("core.encoding.csv", "reader") => {
            let limits = if args.len() > 1 { arg(1) } else { format!("{}jet_std::EncodingLimits::safe()", cx.root_prefix) };
            format!("{}({}, {})", helper("jet_enc_csv_reader"), arg(0), limits)
        }
        ("core.encoding.csv", "writer") => {
            let limits = if args.len() > 1 { arg(1) } else { format!("{}jet_std::EncodingLimits::safe()", cx.root_prefix) };
            format!("{}({}, {})", helper("jet_enc_csv_writer"), arg(0), limits)
        }
        ("core.encoding.json", "decode") => {
            if enc_ok_is_json(ret_ty) {
                format!("{}(&({}))", helper("jet_std_json_decode_lenient"), arg(0))
            } else {
                format!(
                    "{}::<{}>(&({}))",
                    helper("jet_enc_json_decode"),
                    enc_target_rust(ret_ty, cx),
                    arg(0)
                )
            }
        }
        // D-MIGRATE3=A: `decode_traced<T>` — the traced sibling of `decode<T>`,
        // one wrapper deeper (`DecodeResult<T>`), same target-type plumbing.
        ("core.encoding.json", "decode_traced") => {
            format!(
                "{}::<{}>(&({}))",
                helper("jet_enc_json_decode_traced"),
                enc_target_rust_traced(ret_ty, cx),
                arg(0)
            )
        }
        ("core.encoding.json", "to_string") => {
            if enc_arg_is_json(args) {
                format!("{}(&({}))", helper("jet_std_json_render"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_enc_json_to_string"), arg(0))
            }
        }
        ("core.encoding.json", "to_string_pretty") => {
            if enc_arg_is_json(args) {
                format!("{}(&({}))", helper("jet_std_json_render_pretty"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_enc_json_to_string_pretty"), arg(0))
            }
        }
        ("core.encoding.json", "canonical") => {
            if jet_foundation::PackageEdition::package_edition_at_least("2027") {
                let limits = if args.len() > 1 {
                    arg(1)
                } else {
                    format!("{}jet_std::EncodingLimits::safe()", cx.root_prefix)
                };
                format!(
                    "{}(&({}), &({}))",
                    helper("jet_enc_json_canonical"),
                    arg(0),
                    limits
                )
            } else {
                format!("{}(&({}))", helper("jet_std_json_render_canonical"), arg(0))
            }
        }
        ("core.encoding.csv", "decode") => {
            format!(
                "{}::<{}>(&({}))",
                helper("jet_enc_csv_decode"),
                enc_row_target_rust(ret_ty, cx),
                arg(0)
            )
        }
        ("core.encoding.csv", "decode_traced") => {
            format!(
                "{}::<{}>(&({}))",
                helper("jet_enc_csv_decode_traced"),
                enc_row_target_rust_traced(ret_ty, cx),
                arg(0)
            )
        }
        ("core.encoding.csv", "to_string") => {
            if enc_arg_is_string_rows(args) {
                format!("{}(&({}))", helper("jet_ring_csv_render"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_enc_csv_to_string"), arg(0))
            }
        }
        ("core.data", "csv") => {
            format!(
                "{}::<{}>(&({}))",
                helper("jet_enc_csv_decode"),
                enc_row_target_rust(ret_ty, cx),
                arg(0)
            )
        }
        // Array-of-objects JSON → `[T]`, reusing encoding.json's Decode path (I8).
        ("core.data", "json") => {
            format!(
                "{}::<{}>(&({}))",
                helper("jet_enc_json_decode"),
                enc_target_rust(ret_ty, cx),
                arg(0)
            )
        }
        
        // D-COMPUTE1=D (#443): Tensor CPU oracle — one Prelude symbol per call.
        
        
        
        ("core.compute", "set") => format!(
            "{}(&mut ({}), &({}), {})",
            helper("jet_compute_set"),
            arg(0),
            arg(1),
            arg(2)
        ),
        
        
        
        
        
        ("core.compute", "negate" | "abs" | "exp" | "log" | "sqrt") => {
            format!("{}(\"{}\", &({}))", helper("jet_compute_unary"), method, arg(0))
        }
        ("core.compute", "sub" | "div" | "maximum" | "minimum") => format!(
            "{}(\"{}\", &({}), &({}))",
            helper("jet_compute_binary"),
            method,
            arg(0),
            arg(1)
        ),
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        ("core.services", "runtime") => format!(
            "{}(({}).clone(), ({}).as_millis())",
            helper("jet_services_runtime"),
            arg(0),
            arg(1)
        ),
        ("core.services", "state_store") => format!(
            "{}(({}).clone())",
            helper("jet_services_state_store"),
            arg(0)
        ),
        ("core.services", "tree") => format!("{}(({}).clone())", helper("jet_services_tree"), arg(0)),
        ("core.services", "set_restart") => format!(
            "{}(&mut ({}), {})",
            helper("jet_services_set_restart"),
            arg(0),
            arg(1)
        ),
        ("core.services", "set_delivery") => format!(
            "{}(&mut ({}), {})",
            helper("jet_services_set_delivery"),
            arg(0),
            arg(1)
        ),
        ("core.services", "worker") => format!(
            "{}(&mut ({}), ({}).clone(), {})",
            helper("jet_services_worker"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.services", "group") => format!(
            "{}(&mut ({}), ({}).clone(), ({}).clone())",
            helper("jet_services_group"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.services", "start") => {
            format!("{}(&mut ({}))", helper("jet_services_start"), arg(0))
        }
        ("core.services", "stop") => {
            format!("{}(&mut ({}))", helper("jet_services_stop"), arg(0))
        }
        ("core.services", "send") => format!(
            "{}(&mut ({}), &({}), ({}).clone())",
            helper("jet_services_send"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.services", "send_durable") => format!(
            "{}(&mut ({}), &({}), ({}).clone(), ({}).clone())",
            helper("jet_services_send_durable"),
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        ("core.services", "receive") => format!(
            "{}(&mut ({}), &({}))",
            helper("jet_services_receive"),
            arg(0),
            arg(1)
        ),
        
        ("core.services", "fail_worker") => format!(
            "{}(&mut ({}), &({}))",
            helper("jet_services_fail_worker"),
            arg(0),
            arg(1)
        ),
        
        ("core.services", "drain_dead_letters") => format!(
            "{}(&mut ({}))",
            helper("jet_services_drain_dead_letters"),
            arg(0)
        ),
        ("core.services", "set_state_empty") => {
            format!("{}(&mut ({}))", helper("jet_services_set_state_empty"), arg(0))
        }
        ("core.services", "set_state_snapshot") => format!(
            "{}(&mut ({}), ({}).clone(), ({}).clone(), {}, ({}).clone())",
            helper("jet_services_set_state_snapshot"),
            arg(0),
            arg(1),
            arg(2),
            arg(3),
            arg(4)
        ),
        ("core.services", "set_state_event_log") => format!(
            "{}(&mut ({}), ({}).clone(), ({}).clone(), {}, ({}).clone())",
            helper("jet_services_set_state_event_log"),
            arg(0),
            arg(1),
            arg(2),
            arg(3),
            arg(4)
        ),
        ("core.services", "commit_snapshot") => format!(
            "{}(&mut ({}), ({}).clone())",
            helper("jet_services_commit_snapshot"),
            arg(0),
            arg(1)
        ),
        ("core.services", "append_event") => format!(
            "{}(&mut ({}), ({}).clone())",
            helper("jet_services_append_event"),
            arg(0),
            arg(1)
        ),
        ("core.services", "workflow_start") => format!(
            "{}(&mut ({}), ({}).clone(), {})",
            helper("jet_services_workflow_start"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.services", "workflow_step") => format!(
            "{}(&mut ({}), {}, ({}).clone())",
            helper("jet_services_workflow_step"),
            arg(0),
            arg(1),
            arg(2)
        ),
        
        ("core.services", "directory_register") => format!(
            "{}(&mut ({}), ({}).clone(), ({}).clone())",
            helper("jet_services_directory_register"),
            arg(0),
            arg(1),
            arg(2)
        ),
        
        
        ("core.services", "drain_worker") => format!(
            "{}(&mut ({}), &({}))",
            helper("jet_services_drain_worker"),
            arg(0),
            arg(1)
        ),
        ("core.services", "handoff_generation") => format!(
            "{}(&mut ({}))",
            helper("jet_services_handoff_generation"),
            arg(0)
        ),
        
        ("core.services", "rollback_generation") => format!(
            "{}(&mut ({}))",
            helper("jet_services_rollback_generation"),
            arg(0)
        ),
        ("core.services", "chaos_fail") => {
            format!("{}(&mut ({}))", helper("jet_services_chaos_fail"), arg(0))
        }
        
        
        
        
        ("core.data", "schema") => {
            let arg_ty = args.first().map(|a| &a.ty);
            let elem = arg_ty
                .and_then(|ty| data_schema_elem_ty(ty))
                .cloned()
                .unwrap_or(Type::Int);
            let expand = arg_ty.map(|ty| data_schema_expand_struct(ty)).unwrap_or(true);
            // Argument is evaluated for effects, then discarded — schema is type-driven.
            format!(
                "{{ let _ = &({}); {} }}",
                arg(0),
                emit_data_schema_columns(&elem, expand, cx)
            )
        }
        
        ("core.data", "collect") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!(
                    "{}(&({}), &({}jet_std::DataLimits::safe()))",
                    helper("jet_data_collect_checked"),
                    arg(0),
                    cx.root_prefix
                )
            } else {
                format!("{}(&({}))", helper("jet_data_collect"), arg(0))
            }
        }
        
        ("core.data", "sort_by") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!(
                    "{}(&({}), {}, &({}jet_std::DataLimits::safe()))",
                    helper("jet_data_sort_by_checked"),
                    arg(0),
                    arg(1),
                    cx.root_prefix
                )
            } else {
                format!("{}(&({}), {})", helper("jet_data_sort_by"), arg(0), arg(1))
            }
        }
        ("core.data", "group_count") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!(
                    "{}(&({}), {}, &({}jet_std::DataLimits::safe()))",
                    helper("jet_data_group_count_checked"),
                    arg(0),
                    arg(1),
                    cx.root_prefix
                )
            } else {
                format!(
                    "{}(&({}), {})",
                    helper("jet_data_group_count"),
                    arg(0),
                    arg(1)
                )
            }
        }
        ("core.data", "group_sum") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!(
                    "{}(&({}), {}, {}, &({}jet_std::DataLimits::safe()))",
                    helper("jet_data_group_sum_checked"),
                    arg(0),
                    arg(1),
                    arg(2),
                    cx.root_prefix
                )
            } else {
                format!(
                    "{}(&({}), {}, {})",
                    helper("jet_data_group_sum"),
                    arg(0),
                    arg(1),
                    arg(2)
                )
            }
        }
        ("core.data", "group_mean") => {
            let stream = matches!(
                args.first().map(|a| &a.ty),
                Some(Type::Apply { name, .. }) if name == "DataStream"
            );
            if stream {
                let row = match args.first().map(|a| &a.ty) {
                    Some(Type::Apply { args: ta, .. }) => ta
                        .first()
                        .map(|t| cx.rust_type(t))
                        .unwrap_or_else(|| "()".to_string()),
                    _ => "()".to_string(),
                };
                format!(
                    "{}::<{}, _, _>(&mut ({}), {}, {})",
                    helper("jet_data_group_mean_stream"),
                    row,
                    arg(0),
                    arg(1),
                    arg(2)
                )
            } else if matches!(ret_ty, Type::Result { .. }) {
                format!(
                    "{}(&({}), {}, {}, &({}jet_std::DataLimits::safe()))",
                    helper("jet_data_group_mean_checked"),
                    arg(0),
                    arg(1),
                    arg(2),
                    cx.root_prefix
                )
            } else {
                format!(
                    "{}(&({}), {}, {})",
                    helper("jet_data_group_mean"),
                    arg(0),
                    arg(1),
                    arg(2)
                )
            }
        }
        ("core.data", "inner_join") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!(
                    "{}(&({}), &({}), {}, {}, &({}jet_std::DataLimits::safe()))",
                    helper("jet_data_inner_join_checked"),
                    arg(0),
                    arg(1),
                    arg(2),
                    arg(3),
                    cx.root_prefix
                )
            } else {
                format!(
                    "{}(&({}), &({}), {}, {})",
                    helper("jet_data_inner_join"),
                    arg(0),
                    arg(1),
                    arg(2),
                    arg(3)
                )
            }
        }
        ("core.data", "left_join") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!(
                    "{}(&({}), &({}), {}, {}, &({}jet_std::DataLimits::safe()))",
                    helper("jet_data_left_join_checked"),
                    arg(0),
                    arg(1),
                    arg(2),
                    arg(3),
                    cx.root_prefix
                )
            } else {
                format!(
                    "{}(&({}), &({}), {}, {})",
                    helper("jet_data_left_join"),
                    arg(0),
                    arg(1),
                    arg(2),
                    arg(3)
                )
            }
        }
        ("core.data", "pivot_sum") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!(
                    "{}(&({}), {}, {}, {}, &({}jet_std::DataLimits::safe()))",
                    helper("jet_data_pivot_sum_checked"),
                    arg(0),
                    arg(1),
                    arg(2),
                    arg(3),
                    cx.root_prefix
                )
            } else {
                format!(
                    "{}(&({}), {}, {}, {})",
                    helper("jet_data_pivot_sum"),
                    arg(0),
                    arg(1),
                    arg(2),
                    arg(3)
                )
            }
        }
        ("core.data", "sum") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!("{}(&({}))", helper("jet_data_sum_checked"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_data_sum"), arg(0))
            }
        }
        ("core.data", "mean") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!("{}(&({}))", helper("jet_data_mean_checked"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_data_mean"), arg(0))
            }
        }
        ("core.data", "min") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!("{}(&({}))", helper("jet_data_min_checked"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_data_min"), arg(0))
            }
        }
        ("core.data", "max") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!("{}(&({}))", helper("jet_data_max_checked"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_data_max"), arg(0))
            }
        }
        ("core.data", "median") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!("{}(&({}))", helper("jet_data_median_checked"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_data_median"), arg(0))
            }
        }
        ("core.data", "quantile") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!(
                    "{}(&({}), {})",
                    helper("jet_data_quantile_checked"),
                    arg(0),
                    arg(1)
                )
            } else {
                format!("{}(&({}), {})", helper("jet_data_quantile"), arg(0), arg(1))
            }
        }
        ("core.data", "variance") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!("{}(&({}))", helper("jet_data_variance_checked"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_data_variance"), arg(0))
            }
        }
        ("core.data", "stddev") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!("{}(&({}))", helper("jet_data_stddev_checked"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_data_stddev"), arg(0))
            }
        }
        ("core.data", "rolling_mean") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!(
                    "{}(&({}), {})",
                    helper("jet_data_rolling_mean_checked"),
                    arg(0),
                    arg(1)
                )
            } else {
                format!("{}(&({}), {})", helper("jet_data_rolling_mean"), arg(0), arg(1))
            }
        }
        ("core.data", "describe") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!("{}(&({}))", helper("jet_data_describe_checked"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_data_describe"), arg(0))
            }
        }
        
        ("core.data", "bar_text") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!("{}(&({}))", helper("jet_data_bar_text_checked"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_data_bar_text"), arg(0))
            }
        }
        ("core.data", "bar_svg") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!("{}(&({}))", helper("jet_data_bar_svg_checked"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_data_bar_svg"), arg(0))
            }
        }
        ("core.data", "line_text") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!(
                    "{}(&({}), &({}))",
                    helper("jet_data_line_text_checked"),
                    arg(0),
                    arg(1)
                )
            } else {
                format!("{}(&({}), &({}))", helper("jet_data_line_text"), arg(0), arg(1))
            }
        }
        ("core.data", "line_svg") => {
            if matches!(ret_ty, Type::Result { .. }) {
                format!(
                    "{}(&({}), &({}))",
                    helper("jet_data_line_svg_checked"),
                    arg(0),
                    arg(1)
                )
            } else {
                format!("{}(&({}), &({}))", helper("jet_data_line_svg"), arg(0), arg(1))
            }
        }
        
        
        
        
        
        
        
        
        ("core.encoding.toml", "decode") => {
            format!(
                "{}::<{}>(&({}))",
                helper("jet_enc_toml_decode"),
                enc_target_rust(ret_ty, cx),
                arg(0)
            )
        }
        ("core.encoding.toml", "decode_traced") => {
            format!(
                "{}::<{}>(&({}))",
                helper("jet_enc_toml_decode_traced"),
                enc_target_rust_traced(ret_ty, cx),
                arg(0)
            )
        }
        ("core.encoding.toml", "to_string") => {
            if enc_arg_is_json(args) {
                format!("{}(&({}))", helper("jet_std_toml_render"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_enc_toml_to_string"), arg(0))
            }
        }
        ("core.encoding.yaml", "decode") => {
            format!(
                "{}::<{}>(&({}))",
                helper("jet_enc_yaml_decode"),
                enc_target_rust(ret_ty, cx),
                arg(0)
            )
        }
        ("core.encoding.yaml", "decode_traced") => {
            format!(
                "{}::<{}>(&({}))",
                helper("jet_enc_yaml_decode_traced"),
                enc_target_rust_traced(ret_ty, cx),
                arg(0)
            )
        }
        ("core.encoding.yaml", "to_string") => {
            if enc_arg_is_json(args) {
                format!("{}(&({}))", helper("jet_std_yaml_render"), arg(0))
            } else {
                format!("{}(&({}))", helper("jet_enc_yaml_to_string"), arg(0))
            }
        }
        ("core.encoding.xml", "parse_bytes") => {
            let options = if args.len() > 1 {
                arg(1)
            } else {
                format!("{}jet_std::XMLParseOptions::safe()", cx.root_prefix)
            };
            format!("{}(&({}), {})", helper("jet_std_xml_parse_bytes"), arg(0), options)
        }
        ("core.encoding.xml", "to_bytes") => {
            let options = if args.len() > 1 {
                arg(1)
            } else {
                format!("{}jet_std::XMLRenderOptions::safe()", cx.root_prefix)
            };
            format!("{}(&({}), {})", helper("jet_std_xml_to_bytes"), arg(0), options)
        }
        ("core.encoding.xml", "expanded_name") => {
            let fields = match ret_ty {
                Type::Result { ok, .. } => match ok.as_ref() {
                    Type::Tuple(fields) => crate::Codegen::Tuples::tuple_fields_plain(fields),
                    _ => Vec::new(),
                },
                Type::Tuple(fields) => crate::Codegen::Tuples::tuple_fields_plain(fields),
                _ => Vec::new(),
            };
            let struct_name = crate::Codegen::Tuples::tuple_struct_name(&fields);
            jet_format!(
                "{helper}(&({arg})).map(|({jet_prefix}raw, {jet_prefix}prefix, {jet_prefix}local, {jet_prefix}uri)| {struct_name} {{ {raw}: {jet_prefix}raw, {prefix}: {jet_prefix}prefix, {local}: {jet_prefix}local, {uri}: {jet_prefix}uri }})",
                helper = helper("jet_std_xml_expanded_name"),
                arg = arg(0),
                struct_name = struct_name,
                raw = mangle("raw"),
                prefix = mangle("prefix"),
                local = mangle("local"),
                uri = mangle("namespace_uri"),
            )
        }
        ("core.encoding.xml", "decode") => {
            let options = if args.len() > 1 {
                arg(1)
            } else {
                format!("{}jet_std::XMLParseOptions::safe()", cx.root_prefix)
            };
            let target = match ret_ty {
                Type::Result { ok, .. } => cx.rust_type(ok),
                other => cx.rust_type(other),
            };
            format!(
                "{}::<{}>(&({}), {})",
                helper("jet_enc_xml_decode"),
                target,
                arg(0),
                options
            )
        }
        ("core.encoding.xml", "decode_bytes") => {
            let options = if args.len() > 1 {
                arg(1)
            } else {
                format!("{}jet_std::XMLParseOptions::safe()", cx.root_prefix)
            };
            let target = match ret_ty {
                Type::Result { ok, .. } => cx.rust_type(ok),
                other => cx.rust_type(other),
            };
            format!(
                "{}::<{}>(&({}), {})",
                helper("jet_enc_xml_decode_bytes"),
                target,
                arg(0),
                options
            )
        }
        ("core.encoding.cbor", "parse") => {
            let options = if args.len() > 1 { arg(1) } else { format!("{}jet_std::CBOROptions::safe()", cx.root_prefix) };
            format!("{}(&({}), {})", helper("jet_enc_cbor_parse"), arg(0), options)
        }
        ("core.encoding.cbor", "reader") => {
            let limits = if args.len() > 1 { arg(1) } else { format!("{}jet_std::EncodingLimits::safe()", cx.root_prefix) };
            format!("{}({}, {})", helper("jet_enc_cbor_reader"), arg(0), limits)
        }
        ("core.encoding.xml", "reader") => {
            let limits = if args.len() > 1 {
                arg(1)
            } else {
                format!("{}jet_std::EncodingLimits::safe()", cx.root_prefix)
            };
            let xml = if args.len() > 2 {
                arg(2)
            } else {
                format!("{}jet_std::XMLParseOptions::safe()", cx.root_prefix)
            };
            format!(
                "{}({}, {}, {})",
                helper("jet_enc_xml_reader"),
                arg(0),
                limits,
                xml
            )
        }
        ("core.encoding.xml", "writer") => {
            let limits = if args.len() > 1 {
                arg(1)
            } else {
                format!("{}jet_std::EncodingLimits::safe()", cx.root_prefix)
            };
            let xml = if args.len() > 2 {
                arg(2)
            } else {
                format!("{}jet_std::XMLRenderOptions::safe()", cx.root_prefix)
            };
            format!(
                "{}({}, {}, {})",
                helper("jet_enc_xml_writer"),
                arg(0),
                limits,
                xml
            )
        }
        ("core.encoding.cbor", "writer") => {
            let limits = if args.len() > 1 { arg(1) } else { format!("{}jet_std::EncodingLimits::safe()", cx.root_prefix) };
            format!("{}({}, {})", helper("jet_enc_cbor_writer"), arg(0), limits)
        }
        ("core.encoding.cbor", "decode") => {
            let options = if args.len() > 1 { arg(1) } else { format!("{}jet_std::CBOROptions::safe()", cx.root_prefix) };
            // CBOR decodes one whole Codable value. Unlike CSV, a list return is
            // the target itself, not a row wrapper whose element type is T.
            let target = match ret_ty {
                Type::Result { ok, .. } => cx.rust_type(ok),
                other => cx.rust_type(other),
            };
            format!("{}::<{}>(&({}), {})", helper("jet_enc_cbor_decode"), target, arg(0), options)
        }
        // D-UUIDENC1=A: hex and base64 encode/decode.
        
        
        // #1481: `v5` (namespace+name, deterministic) and `parse` (validate
        // + normalize) — pure std, same UUID-as-String shape as v4/v7.
        
        
        
        
        
        // E2-M7: std.path helpers (D-IO1).
        
        
        
        
        
        
        
        
        
        
        
        
        
        ("core.email", "smtp") => format!(
            "{}jet_email::smtp({}, {}, {})",
            cx.root_prefix, format!("&({})", arg(0)), regex_fn("jet_crypto_secret_copy_for_smtp_impl"), email_runtime(),
        ),
        ("core.email", "smtp_from_env") => format!(
            "{}jet_email::smtp_from_env({})", cx.root_prefix, email_runtime(),
        ),
        // D-TEXTUNICODE1: std-only Unicode scalar helpers.
        
        
        
        
        
        
        
        
        
        
        // D-TEXTWIDTH1=B: 1-arg = portable default (`Int`); 2-arg (`policy:`)
        // routes through the `TextWidth`-taking helper (`Int ? TextError`).
        ("core.text", "display_width") if args.len() >= 2 => format!(
            "{}(&({}), &({}))",
            helper("jet_text_display_width"),
            arg(0),
            arg(1)
        ),
        // The 1-arg default stays here, not in `Syntax::CORE_CALLS`: the row is
        // read before this match, so a table row would answer the 2-arg call
        // above and drop the policy argument.
        ("core.text", "display_width") => {
            format!("{}(&({}))", helper("jet_text_display_width_default"), arg(0))
        }
        
        
        
        
        
        
        
        
        
        
        
        
        // E2-M9: first-party ring packages.
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        
        // E2-M12 D-OBS3: trace context for structured log records.
        
        
        
        ("core.crypto", "sha256") => format!("{}(&({}))", regex_fn("jet_crypto_sha256_typed_impl"), arg(0)),
        ("core.crypto", "sha512_bytes") => {
            format!("{}(&({}))", regex_fn("jet_crypto_sha512_impl"), arg(0))
        }
        ("core.crypto", "blake3_bytes") => {
            format!("{}(&({}))", regex_fn("jet_crypto_blake3_impl"), arg(0))
        }
        ("core.crypto", "constant_time_equal_bytes") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_constant_time_equal_bytes_impl"),
            arg(0),
            arg(1)
        ),
        ("core.crypto", "hkdf_sha256") => format!(
            "{}(&({}), &({}), &({}), {})",
            regex_fn("jet_crypto_hkdf_typed_impl"),
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        ("core.crypto", "x25519_public") => {
            format!(
                "{}(&({}))",
                regex_fn("jet_crypto_x25519_public_impl"),
                arg(0)
            )
        }
        ("core.crypto", "x25519_shared") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_x25519_shared_impl"),
            arg(0),
            arg(1)
        ),
        ("core.crypto", "password_hash") => {
            format!(
                "{}(&({}), jet_scheduler_wait_point_cancelled, jet_task_deliver_cancel, jet_scheduler_blocking_wait_enter, jet_scheduler_blocking_wait_leave)",
                regex_fn("jet_crypto_password_hash_typed_cancel_impl"),
                arg(0)
            )
        }
        ("core.crypto", "password_hash_with_salt") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_password_hash_with_salt_impl"),
            arg(0),
            arg(1)
        ),
        ("core.crypto", "password_verify") => format!(
            "{}(&({}), &({}), jet_scheduler_wait_point_cancelled, jet_task_deliver_cancel, jet_scheduler_blocking_wait_enter, jet_scheduler_blocking_wait_leave)",
            regex_fn("jet_crypto_password_verify_typed_cancel_impl"),
            arg(0),
            arg(1)
        ),
        // D-CRYPTOENV1=A: misuse-resistant envelope (RustCrypto FFI bridge).
        ("core.crypto", "seal") => format!(
            "{}({}, &({}), &({}))",
            regex_fn("jet_crypto_seal_typed_impl"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.crypto", "file_seal") => format!(
            "{}({}, &({}.inner.to_string_lossy().into_owned()), &({}.inner.to_string_lossy().into_owned()), {}jet_scheduler_task_cancelled)",
            regex_fn("jet_crypto_file_seal_impl"),
            arg(0),
            arg(1),
            arg(2),
            cx.root_prefix,
        ),
        ("core.crypto", "open") => format!(
            "{}(&({}), {}, &({}))",
            regex_fn("jet_crypto_open_typed_impl"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.crypto", "file_open") => format!(
            "{}(&({}), &({}.inner.to_string_lossy().into_owned()), &({}.inner.to_string_lossy().into_owned()), {}jet_scheduler_task_cancelled)",
            regex_fn("jet_crypto_file_open_impl"),
            arg(0),
            arg(1),
            arg(2),
            cx.root_prefix,
        ),
        ("core.crypto", "sign") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_sign_typed_impl"),
            arg(0),
            arg(1)
        ),
        ("core.crypto", "verify") => format!(
            "{}({}, &({}), {})",
            regex_fn("jet_crypto_verify_typed_impl"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.crypto", "wrap") => format!("{}(&({}), {})", regex_fn("jet_crypto_wrap_typed_impl"), arg(0), arg(1)),
        ("core.crypto", "unwrap") => format!("{}(&({}), {})", regex_fn("jet_crypto_unwrap_typed_impl"), arg(0), arg(1)),
        ("core.crypto", "x25519") => format!("{}(&({}), {})", regex_fn("jet_crypto_x25519_typed_impl"), arg(0), arg(1)),
        ("core.crypto", "constant_time_equal") => format!("{}(&({}), &({}))", regex_fn("jet_crypto_constant_time_secret_impl"), arg(0), arg(1)),
        ("core.crypto", "blake3") => format!("{}(&({}))", regex_fn("jet_crypto_blake3_typed_impl"), arg(0)),
        ("core.crypto", "sha512") => format!("{}(&({}))", regex_fn("jet_crypto_sha512_typed_impl"), arg(0)),
        ("core.crypto", "__secret_from_text") => format!("{}({})", regex_fn("jet_crypto_secret_from_text_impl"), arg(0)),
        ("core.crypto", "__secret_from_bytes") => format!("{}({})", regex_fn("jet_crypto_secret_from_bytes_impl"), arg(0)),
        ("core.crypto", "__signing_generate") => format!("{}()", regex_fn("jet_crypto_signing_generate_impl")),
        ("core.crypto", "__x25519_generate") => format!("{}()", regex_fn("jet_crypto_x25519_generate_impl")),
        ("core.crypto", "__verify_key_from_bytes") => format!("{}({})", regex_fn("jet_crypto_verify_key_from_bytes_impl"), arg(0)),
        ("core.crypto", "__x25519_public_from_bytes") => format!("{}({})", regex_fn("jet_crypto_x25519_public_from_bytes_impl"), arg(0)),
        ("core.crypto", "__x25519_public_from_text") => format!("{}({})", regex_fn("jet_crypto_x25519_public_from_text_impl"), arg(0)),
        ("core.crypto", "__signature_from_bytes") => format!("{}({})", regex_fn("jet_crypto_signature_from_bytes_impl"), arg(0)),
        ("core.crypto", "__sealed_from_bytes") => format!("{}({})", regex_fn("jet_crypto_sealed_from_bytes_impl"), arg(0)),
        ("core.crypto", "__wrapped_from_bytes") => format!("{}({})", regex_fn("jet_crypto_wrapped_from_bytes_impl"), arg(0)),
        ("core.crypto", "__password_parse") => format!("{}({})", regex_fn("jet_crypto_password_parse_impl"), arg(0)),
        ("core.crypto", "__signing_public") => format!("{}(&({}))", regex_fn("jet_crypto_signing_public_impl"), arg(0)),
        ("core.crypto", "__x25519_public") => format!("{}(&({}))", regex_fn("jet_crypto_x25519_public_typed_impl"), arg(0)),
        ("core.crypto", "__verify_key_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_verify_key_bytes_impl"), arg(0)),
        ("core.crypto", "__x25519_public_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_x25519_public_bytes_impl"), arg(0)),
        ("core.crypto", "__x25519_public_text") => format!("{}(&({}))", regex_fn("jet_crypto_x25519_public_text_impl"), arg(0)),
        ("core.crypto", "__signature_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_signature_bytes_impl"), arg(0)),
        ("core.crypto", "__sealed_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_sealed_bytes_impl"), arg(0)),
        ("core.crypto", "__wrapped_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_wrapped_bytes_impl"), arg(0)),
        ("core.crypto", "__digest256_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_digest256_bytes_impl"), arg(0)),
        ("core.crypto", "__digest512_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_digest512_bytes_impl"), arg(0)),
        ("core.crypto", "__digest256_hex") => format!("{}(&({}))", regex_fn("jet_crypto_digest256_hex_impl"), arg(0)),
        ("core.crypto", "__digest512_hex") => format!("{}(&({}))", regex_fn("jet_crypto_digest512_hex_impl"), arg(0)),
        ("core.crypto", "__password_text") => format!("{}(&({}))", regex_fn("jet_crypto_password_text_impl"), arg(0)),
        ("core.crypto", "__hasher_new") => format!("{}()", helper("jet_crypto_hasher_new")),
        ("core.crypto", "__hasher_update") => format!(
            "{}(&mut ({}), &({}))",
            helper("jet_crypto_hasher_update"),
            arg(0),
            arg(1)
        ),
        ("core.crypto", "__hasher_digest") => format!("{}(&({}))", helper("jet_crypto_hasher_digest"), arg(0)),
        // D-CRYPTO-API1=A: exact expert primitives, all checked in one bridge.
        ("core.crypto.expert", "xchacha20poly1305_seal") => format!("{}(&({}), &({}), &({}), &({}))", regex_fn("jet_crypto_expert_xchacha20poly1305_seal_impl"), arg(0), arg(1), arg(2), arg(3)),
        ("core.crypto.expert", "xchacha20poly1305_open") => format!("{}(&({}), &({}), &({}), &({}))", regex_fn("jet_crypto_expert_xchacha20poly1305_open_impl"), arg(0), arg(1), arg(2), arg(3)),
        ("core.crypto.expert", "aes256gcm_seal") => format!("{}(&({}), &({}), &({}), &({}))", regex_fn("jet_crypto_expert_aes256gcm_seal_impl"), arg(0), arg(1), arg(2), arg(3)),
        ("core.crypto.expert", "aes256gcm_open") => format!("{}(&({}), &({}), &({}), &({}))", regex_fn("jet_crypto_expert_aes256gcm_open_impl"), arg(0), arg(1), arg(2), arg(3)),
        ("core.crypto.expert", "open_v1") => format!("{}(&({}), &({}))", regex_fn("jet_crypto_expert_open_v1_impl"), arg(0), arg(1)),
        ("core.crypto.expert", "migrate_v1") => format!(
            "{}(&({}), &({}.inner.to_string_lossy().into_owned()), {}, &({}.inner.to_string_lossy().into_owned()), {}jet_scheduler_task_cancelled)",
            regex_fn("jet_crypto_expert_migrate_v1_impl"),
            arg(0),
            arg(1),
            arg(2),
            arg(3),
            cx.root_prefix,
        ),
        ("core.crypto.expert", "ed25519_sign") => format!("{}(&({}), &({}))", regex_fn("jet_crypto_expert_ed25519_sign_impl"), arg(0), arg(1)),
        ("core.crypto.expert", "ed25519_verify_strict") => format!("{}(&({}), &({}), &({}))", regex_fn("jet_crypto_expert_ed25519_verify_strict_impl"), arg(0), arg(1), arg(2)),
        ("core.crypto.expert", "x25519_raw") => format!("{}(&({}), &({}), true)", regex_fn("jet_crypto_expert_x25519_impl"), arg(0), arg(1)),
        ("core.crypto.expert", "hkdf_sha256_raw") => format!("{}(&({}), &({}), &({}), {})", regex_fn("jet_crypto_expert_hkdf_sha256_impl"), arg(0), arg(1), arg(2), arg(3)),
        ("core.crypto.expert", "argon2id") => format!("{}(&({}), &({}), {}, {}, {}, {}, jet_scheduler_wait_point_cancelled, jet_task_deliver_cancel, jet_scheduler_blocking_wait_enter, jet_scheduler_blocking_wait_leave)", regex_fn("jet_crypto_expert_argon2id_cancel_impl"), arg(0), arg(1), arg(2), arg(3), arg(4), arg(5)),
        ("core.crypto.expert", "secret_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_expert_secret_bytes_impl"), arg(0)),
        ("core.crypto.expert", "signing_key_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_expert_signing_key_bytes_impl"), arg(0)),
        ("core.crypto.expert", "x25519_secret_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_expert_x25519_secret_bytes_impl"), arg(0)),
        ("core.crypto.expert", "shared_secret_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_expert_shared_secret_bytes_impl"), arg(0)),
        // D-AUTH-TOKENPOLICY1=A: fixed HS256 with required labelled key and
        // audience. This bridge only marshals optional suffixes; Auth.rs owns
        // the omitted-value policy for every execution tier.
        ("core.auth", "verify_jwt") => {
            let issuer = if args.len() >= 4 { format!("Some(&({}))", arg(3)) } else { "None".to_string() };
            let skew = if args.len() >= 5 { format!("Some({}jet_duration_ns_value(&({})))", cx.root_prefix, arg(4)) } else { "None".to_string() };
            format!(
                "{}(&({}), &({}), &({}), {}, {})",
                helper("jet_auth_verify_jwt_defaulted"),
                arg(0),
                arg(1),
                arg(2),
                issuer,
                skew,
            )
        }
        ("core.auth", "verify_paseto") => {
            let issuer = if args.len() >= 4 { format!("Some(&({}))", arg(3)) } else { "None".to_string() };
            let skew = if args.len() >= 5 { format!("Some({}jet_duration_ns_value(&({})))", cx.root_prefix, arg(4)) } else { "None".to_string() };
            let footer = if args.len() >= 6 { format!("Some(&({}))", arg(5)) } else { "None".to_string() };
            let implicit = if args.len() >= 7 { format!("Some(&({}))", arg(6)) } else { "None".to_string() };
            format!(
                "{}(&({}), &({}), &({}), {}, {}, {}, {}, {})",
                helper("jet_auth_verify_paseto_defaulted"), arg(0), arg(1), arg(2), issuer, skew,
                footer, implicit, regex_fn("jet_crypto_expert_ed25519_verify_strict_impl"),
            )
        }
        ("core.auth", "register_user") => format!(
            "{}(({}).clone(), ({}).clone())",
            helper("jet_auth_register_user"),
            arg(0),
            arg(1)
        ),
        ("core.auth", "password_login") => format!(
            "{}(({}).clone(), ({}).clone(), {}, {})",
            helper("jet_auth_password_login"),
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        
        ("core.auth", "magic_link_issue") => format!(
            "{}(({}).clone(), {}, {})",
            helper("jet_auth_magic_link_issue"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.auth", "magic_link_consume") => format!(
            "{}(({}).clone(), {}, {})",
            helper("jet_auth_magic_link_consume"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.auth", "oauth_begin") => format!(
            "{}(({}).clone())",
            helper("jet_auth_oauth_begin"),
            arg(0)
        ),
        ("core.auth", "oauth_finish") => format!(
            "{}(({}).clone(), ({}).clone(), {}, {})",
            helper("jet_auth_oauth_finish"),
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        ("core.sync", "text_new") => format!(
            "{}(({}).clone(), ({}).clone())",
            helper("jet_sync_text_new"),
            arg(0),
            arg(1)
        ),
        ("core.sync", "text_set") => format!(
            "{}(({}).clone(), ({}).clone(), ({}).clone())",
            helper("jet_sync_text_set"),
            arg(0),
            arg(1),
            arg(2)
        ),
        
        ("core.sync", "text_edit") => format!(
            "{}(({}).clone(), ({}).clone(), {}, {}, ({}).clone())",
            helper("jet_sync_text_edit"),
            arg(0),
            arg(1),
            arg(2),
            arg(3),
            arg(4)
        ),
        ("core.sync", "counter_new") => format!(
            "{}(({}).clone(), {})",
            helper("jet_sync_counter_new"),
            arg(0),
            arg(1)
        ),
        ("core.sync", "counter_inc") => format!(
            "{}(({}), ({}).clone(), {})",
            helper("jet_sync_counter_inc"),
            arg(0),
            arg(1),
            arg(2)
        ),
        
        
        ("core.sync", "map_set") => format!(
            "{}(({}), ({}).clone(), ({}).clone())",
            helper("jet_sync_map_set"),
            arg(0),
            arg(1),
            arg(2)
        ),
        
        
        
        ("core.sync", "list_push") => format!(
            "{}(({}), ({}).clone(), ({}).clone())",
            helper("jet_sync_list_push"),
            arg(0),
            arg(1),
            arg(2)
        ),
        
        ("core.sync", "policy_new") => format!(
            "{}(({}).clone(), ({}).clone())",
            helper("jet_db_policy_new"),
            arg(0),
            arg(1)
        ),
        
        ("core.vault", "get") => {
            format!("{}(&({}))", regex_fn("jet_vault_get_impl"), arg(0))
        }
        ("core.vault", "current") =>
            format!("{}::<{}>(&({})).map(jet_outcome_of)", regex_fn("jet_vault_current_impl"), vault_rust(), arg(0)),
        ("core.vault", "versions" | "prepare_generate" | "prepare_rotate") =>
            format!("{}::<{}>(&({}))", regex_fn(&format!("jet_vault_{method}_impl")), vault_rust(), arg(0)),
        ("core.vault", "load" | "status") =>
            format!("{}::<{}>(&({}))", regex_fn(&format!("jet_vault_{method}_impl")), vault_rust(), arg(0)),
        ("core.vault", "prepare_store") =>
            format!("{}::<{}>(&({}), {})", regex_fn("jet_vault_prepare_store_impl"), vault_rust(), arg(0), arg(1)),
        ("core.vault", "prepare_retire" | "prepare_revoke") =>
            format!("{}::<{}>(&({}), &({}))", regex_fn(&format!("jet_vault_{method}_impl")), vault_rust(), arg(0), arg(1)),
        ("core.vault", "authorize_write") =>
            format!("{}::<{}>(&({}), &({}))", regex_fn("jet_vault_authorize_write_impl"), vault_rust(), arg(0), arg(1)),
        ("core.vault", "export_to_recipients") =>
            format!("{}::<{}>(&({}), &({}))", regex_fn("jet_vault_export_to_recipients_impl"), vault_rust(), arg(0), arg(1)),
        ("core.vault", "export_to_passphrase") =>
            format!("{}::<{}>(&({}), &({}), jet_scheduler_wait_point_cancelled, jet_task_deliver_cancel, jet_scheduler_blocking_wait_enter, jet_scheduler_blocking_wait_leave)", regex_fn("jet_vault_export_to_passphrase_cancel_impl"), vault_rust(), arg(0), arg(1)),
        ("core.vault", "prepare_import_wrapped") =>
            format!("{}::<{}>(&({}), ({}).clone(), ({}).clone(), jet_scheduler_wait_point_cancelled, jet_task_deliver_cancel, jet_scheduler_blocking_wait_enter, jet_scheduler_blocking_wait_leave)", regex_fn("jet_vault_prepare_import_wrapped_cancel_impl"), vault_rust(), arg(0), arg(1), arg(2)),
        ("core.vault", "authorize_wrapped_import") =>
            format!("{}::<{}>(&({}), &({}))", regex_fn("jet_vault_authorize_wrapped_import_impl"), vault_rust(), arg(0), arg(1)),
        ("core.vault", "commit_import_wrapped") =>
            format!("{}::<{}>({}, {})", regex_fn("jet_vault_commit_import_wrapped_impl"), vault_rust(), arg(0), arg(1)),
        ("core.vault", "commit_generate" | "commit_store" | "commit_rotate" | "commit_retire" | "commit_revoke") =>
            format!("{}::<{}>({}, {})", regex_fn(&format!("jet_vault_{method}_impl")), vault_rust(), arg(0), arg(1)),
        ("core.vault.expert", "prepare_import_signing") =>
            format!("{}(&({}), {})", regex_fn("jet_vault_expert_prepare_import_signing_impl"), arg(0), arg(1)),
        ("core.vault.expert", "prepare_import_x25519") =>
            format!("{}(&({}), {})", regex_fn("jet_vault_expert_prepare_import_x25519_impl"), arg(0), arg(1)),
        ("core.vault.expert", "commit_import_signing") =>
            format!("{}({}, {})", regex_fn("jet_vault_expert_commit_import_signing_impl"), arg(0), arg(1)),
        ("core.vault.expert", "commit_import_x25519") =>
            format!("{}({}, {})", regex_fn("jet_vault_expert_commit_import_x25519_impl"), arg(0), arg(1)),
        ("core.crypto", "__vault_wrapped_from_bytes") =>
            format!("{}({})", regex_fn("jet_vault_wrapped_from_bytes_impl"), arg(0)),
        ("core.crypto", "__vault_wrapped_bytes") =>
            format!("{}(&({}))", regex_fn("jet_vault_wrapped_bytes_impl"), arg(0)),
        ("core.crypto", "__vault_unlock_recipient") =>
            format!("{}(&({}))", regex_fn("jet_vault_unlock_recipient_impl"), arg(0)),
        ("core.crypto", "__vault_unlock_passphrase") =>
            format!("{}(&({}))", regex_fn("jet_vault_unlock_passphrase_impl"), arg(0)),
        // D-NETSOCKET1=A: core.net — typed addresses, TCP/UDP/Unix/DNS, TLS handle.
        
        
        
        
        
        
        
        
        
        ("core.net", "tcp_read") => format!("{}(&mut ({}))", helper("jet_net_tcp_read"), arg(0)),
        ("core.net", "tcp_write") => format!(
            "{}(&mut ({}), &({}))",
            helper("jet_net_tcp_write"),
            arg(0),
            arg(1)
        ),
        ("core.net", "tcp_read_bytes") => format!(
            "{}(&mut ({}), {})", helper("jet_net_tcp_read_bytes"), arg(0), arg(1)
        ),
        ("core.net", "tcp_read_text") => format!(
            "{}(&mut ({}), {})", helper("jet_net_tcp_read_text"), arg(0), arg(1)
        ),
        ("core.net", "tcp_write_bytes") => format!(
            "{}(&mut ({}), &({}))", helper("jet_net_tcp_write_bytes"), arg(0), arg(1)
        ),
        ("core.net", "tcp_write_all_bytes") => format!(
            "{}(&mut ({}), &({}))", helper("jet_net_tcp_write_all_bytes"), arg(0), arg(1)
        ),
        ("core.net", "tcp_write_text") => format!(
            "{}(&mut ({}), &({}))", helper("jet_net_tcp_write_text"), arg(0), arg(1)
        ),
        ("core.net", "tcp_shutdown") => format!(
            "{}(&mut ({}), {})", helper("jet_net_tcp_shutdown"), arg(0), arg(1)
        ),
        ("core.net", "tcp_close") => {
            format!("{}(&mut ({}))", helper("jet_net_tcp_close"), arg(0))
        }
        ("core.net", "tcp_ready") => format!(
            "{}(&mut ({}), {}, {})", helper("jet_net_tcp_ready"), arg(0), arg(1), arg(2)
        ),
        
        
        
        
        
        
        ("core.net", "set_timeout") => format!(
            "{}(&mut ({}), {})",
            helper("jet_net_set_timeout"),
            arg(0),
            arg(1)
        ),
        ("core.net", "set_read_timeout") => format!(
            "{}(&mut ({}), {})",
            helper("jet_net_set_read_timeout"),
            arg(0),
            arg(1)
        ),
        ("core.net", "set_write_timeout") => format!(
            "{}(&mut ({}), {})",
            helper("jet_net_set_write_timeout"),
            arg(0),
            arg(1)
        ),
        
        
        
        ("core.net", "sendfile") => format!(
            "{}(&mut ({}), &({}))",
            helper("jet_net_sendfile"),
            arg(0),
            arg(1)
        ),
        
        
        
        
        
        
        
        
        
        
        
        ("core.net", "unix_connect") => {
            if args.len() == 2 {
                format!(
                    "{}(&({}), &({}))",
                    helper("jet_net_unix_connect_deadline"),
                    arg(0),
                    arg(1)
                )
            } else {
                format!("{}(&({}))", helper("jet_net_unix_connect"), arg(0))
            }
        }
        ("core.net", "unix_read") => format!("{}(&mut ({}))", helper("jet_net_unix_read"), arg(0)),
        ("core.net", "unix_write") => format!(
            "{}(&mut ({}), &({}))",
            helper("jet_net_unix_write"),
            arg(0),
            arg(1)
        ),
        ("core.net", "unix_read_bytes") => format!(
            "{}(&mut ({}), {})", helper("jet_net_unix_read_bytes"), arg(0), arg(1)
        ),
        ("core.net", "unix_write_all_bytes") => format!(
            "{}(&mut ({}), &({}))", helper("jet_net_unix_write_all_bytes"), arg(0), arg(1)
        ),
        ("core.net", "unix_shutdown") => format!(
            "{}(&mut ({}), {})", helper("jet_net_unix_shutdown"), arg(0), arg(1)
        ),
        ("core.net", "unix_close") => {
            format!("{}(&mut ({}))", helper("jet_net_unix_close"), arg(0))
        }
        ("core.net", "dns_a") => format!("{}({}(&({}), {}), &({}))", helper("jet_net_dns_result"), helper("jet_net_dns_a"), arg(0), arg(1), arg(0)),
        ("core.net", "dns_aaaa") => {
            format!("{}({}(&({}), {}), &({}))", helper("jet_net_dns_result"), helper("jet_net_dns_aaaa"), arg(0), arg(1), arg(0))
        }
        ("core.net", "dns_a_at") => format!(
            "{}({}(&({}), &({}), {}), &({}))",
            helper("jet_net_dns_result"), helper("jet_net_dns_a_at"),
            arg(0),
            arg(1),
            arg(2), arg(1)
        ),
        ("core.net", "dns_aaaa_at") => format!(
            "{}({}(&({}), &({}), {}), &({}))",
            helper("jet_net_dns_result"), helper("jet_net_dns_aaaa_at"),
            arg(0),
            arg(1),
            arg(2), arg(1)
        ),
        ("core.net", "dns_txt") => {
            format!("{}({}(&({}), {}), &({}))", helper("jet_net_dns_result"), helper("jet_net_dns_txt"), arg(0), arg(1), arg(0))
        }
        ("core.net", "dns_txt_at") => format!(
            "{}({}(&({}), &({}), {}), &({}))",
            helper("jet_net_dns_result"), helper("jet_net_dns_txt_at"),
            arg(0),
            arg(1),
            arg(2), arg(1)
        ),
        ("core.net", "dns_ptr") => {
            format!("{}({}(&({}), {}), &({}))", helper("jet_net_dns_result"), helper("jet_net_dns_ptr"), arg(0), arg(1), arg(0))
        }
        ("core.net", "dns_srv") => {
            format!("{}({}(&({}), {}), &({}))", helper("jet_net_dns_result"), helper("jet_net_dns_srv"), arg(0), arg(1), arg(0))
        }
        ("core.net", "dns_srv_at") => format!(
            "{}({}(&({}), &({}), {}), &({}))",
            helper("jet_net_dns_result"), helper("jet_net_dns_srv_at"),
            arg(0),
            arg(1),
            arg(2), arg(1)
        ),
        ("core.net", "tls_connect") => format!(
            "{}({}, &({}), {}, {}, {}, {}, {}, {}, {}, {}, {}, {})",
            helper("jet_net_tls_client_scheduler"),
            arg(0),
            arg(1),
            regex_fn("jet_net_tls_begin_impl"),
            regex_fn("jet_net_tls_handshake_step_impl"),
            regex_fn("jet_net_tls_abort_impl"),
            regex_fn("jet_net_tls_wants_impl"),
            regex_fn("jet_net_tls_read_ready_impl"),
            regex_fn("jet_net_tls_read_step_impl"),
            regex_fn("jet_net_tls_write_step_impl"),
            regex_fn("jet_net_tls_close_step_impl"),
            regex_fn("jet_net_tls_close_write_step_impl"),
            regex_fn("jet_net_tls_peer_identity_impl")
        ),
        ("core.net", "tls_read") => {
            format!("{}(&mut ({}))", helper("jet_net_tls_read_text"), arg(0))
        }
        ("core.net", "tls_write") => format!(
            "{}(&mut ({}), &({}))",
            helper("jet_net_tls_write_text"),
            arg(0),
            arg(1)
        ),
        ("core.net", "tls_close") => {
            format!("{}(&mut ({}))", helper("jet_net_tls_close"), arg(0))
        }
        ("core.tls", "client") => {
            let (helper_name, extra_args, begin_name) = match args.len() {
                4 => (
                    "jet_net_tls_client_scheduler_config_deadline",
                    format!(", &({}), &({})", arg(2), arg(3)),
                    "jet_net_tls_begin_config_impl",
                ),
                3 => (
                    "jet_net_tls_client_scheduler_deadline",
                    format!(", &({})", arg(2)),
                    "jet_net_tls_begin_impl",
                ),
                _ => (
                    "jet_net_tls_client_scheduler",
                    String::new(),
                    "jet_net_tls_begin_impl",
                ),
            };
            format!(
                "{}({}, &({}){}, {}, {}, {}, {}, {}, {}, {}, {}, {}, {})",
                helper(helper_name),
                arg(0),
                arg(1),
                extra_args,
                regex_fn(begin_name),
                regex_fn("jet_net_tls_handshake_step_impl"),
                regex_fn("jet_net_tls_abort_impl"),
                regex_fn("jet_net_tls_wants_impl"),
                regex_fn("jet_net_tls_read_ready_impl"),
                regex_fn("jet_net_tls_read_step_impl"),
                regex_fn("jet_net_tls_write_step_impl"),
                regex_fn("jet_net_tls_close_step_impl"),
                regex_fn("jet_net_tls_close_write_step_impl"),
                regex_fn("jet_net_tls_peer_identity_impl")
            )
        },
        ("core.tls", "read") => format!(
            "{}(&mut ({}), {})",
            helper("jet_net_tls_read_bytes"), arg(0), arg(1)
        ),
        ("core.tls", "read_text") => format!(
            "{}(&mut ({}))",
            helper("jet_net_tls_read_text"), arg(0)
        ),
        ("core.tls", "write") => format!(
            "{}(&mut ({}), &({}))",
            helper("jet_net_tls_write_bytes"), arg(0), arg(1)
        ),
        ("core.tls", "write_all") => format!(
            "{}(&mut ({}), &({}))",
            helper("jet_net_tls_write_all_bytes"), arg(0), arg(1)
        ),
        ("core.tls", "write_text") => format!(
            "{}(&mut ({}), &({}))",
            helper("jet_net_tls_write_text"), arg(0), arg(1)
        ),
        ("core.tls", "close") => format!(
            "{}(&mut ({}))",
            helper("jet_net_tls_close"), arg(0)
        ),
        // E2-M10: core.http — HTTP client.
        ("core.http", "get") => {
            let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
            emit_http_response_from_bridge(
                format!("{ffi}::JetHTTPAmbientDeadline::push(jet_deadline_remaining_ms()).and_then(|_ambient| {ffi}::jet_http_client_get_impl(&({})))", arg(0)),
                ffi,
            )
        }
        ("core.http", "post") => {
            let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
            emit_http_response_from_bridge(
                format!("{ffi}::JetHTTPAmbientDeadline::push(jet_deadline_remaining_ms()).and_then(|_ambient| {ffi}::jet_http_client_post_impl(&({}), &({})))", arg(0), arg(1)),
                ffi,
            )
        }
        // c109 Phase 25: HTTPRouter producer + parse/dispatch (D-ROUTE1=A).
        // `parse(raw)` borrows the raw string; `dispatch(router, req)` borrows the router
        // and passes the request by value.
        
        
        
        // D-REGEXENGINE1=A: core.regex — std-only runtime in jet_std, no bridge dep.
        
        
        // D-CORE-COMPRESS1=A / D-DEP-ARCHIVE1=A: core.archive owns only
        // zip/tar containers. Stream codecs lower through core.compress.
        // Archive operations use the canonical dependency-free ABI bridge.
        // zip_compress takes (&str, &[u8]); zip_decompress takes &[u8].
        ("core.archive", "zip_compress") => {
            format!(
                "{}(&({}), &({}))",
                regex_fn("jet_archive_zip_compress"),
                arg(0),
                arg(1)
            )
        }
        ("core.archive", "zip_decompress") => {
            format!("{}(&({}))", regex_fn("jet_archive_zip_decompress"), arg(0))
        }
        ("core.archive", "crc32") => {
            format!("{}(&({}))", regex_fn("jet_archive_crc32"), arg(0))
        }
        ("core.archive", "adler32") => {
            format!("{}(&({}))", regex_fn("jet_archive_adler32"), arg(0))
        }
        ("core.archive", "deflate") => {
            format!("{}(&({}))", regex_fn("jet_archive_deflate"), arg(0))
        }
        ("core.archive", "inflate") => {
            format!("{}(&({}))", regex_fn("jet_archive_inflate"), arg(0))
        }
        ("core.archive", "zip_names_json") => {
            format!("{}(&({}))", regex_fn("jet_archive_zip_names_json"), arg(0))
        }
        ("core.archive", "zip_open") => {
            format!("{}(&({}))", regex_fn("jet_archive_zip_open"), arg(0))
        }
        ("core.archive", "zip_next") => {
            format!("{}(&({}), {})", regex_fn("jet_archive_zip_next"), arg(0), arg(1))
        }
        ("core.archive", "zip_read") => {
            format!("{}(&({}), &({}))", regex_fn("jet_archive_zip_read"), arg(0), arg(1))
        }
        ("core.archive", "zip_write") => {
            format!(
                "{}(&({}), &({}), &({}))",
                regex_fn("jet_archive_zip_write"),
                arg(0),
                arg(1),
                arg(2)
            )
        }
        ("core.archive", "zip_close") => {
            format!("{}(&({}))", regex_fn("jet_archive_zip_close"), arg(0))
        }
        ("core.archive", "zip_extract") => {
            format!("{}(&({}), &({}))", regex_fn("jet_archive_zip_extract"), arg(0), arg(1))
        }
        ("core.archive", "unzip") => {
            format!("{}(&({}), &({}))", regex_fn("jet_archive_unzip"), arg(0), arg(1))
        }
        // D-DEP-ARCHIVE1=A: tar_add / tar_get / tar_names_json via the FFI bridge.
        // All three take &[u8] / &str args (non-scalar → borrow); none take scalars.
        ("core.archive", "tar_add") => {
            format!(
                "{}(&({}), &({}), &({}))",
                regex_fn("jet_archive_tar_add"),
                arg(0),
                arg(1),
                arg(2)
            )
        }
        ("core.archive", "tar_get") => {
            format!(
                "{}(&({}), &({}))",
                regex_fn("jet_archive_tar_get"),
                arg(0),
                arg(1)
            )
        }
        ("core.archive", "tar_names_json") => {
            format!("{}(&({}))", regex_fn("jet_archive_tar_names_json"), arg(0))
        }
        // D-RAYLIB1=A / D-FLAGSHIP-RAYLIB1=A: typed graphics bridge.
        ("core.compress.gzip", "compress") => {
            format!("{}(&({}))", regex_fn("jet_compress_gzip_compress"), arg(0))
        }
        ("core.compress.gzip", "decompress") => {
            format!(
                "{}(&({}))",
                regex_fn("jet_compress_gzip_decompress"),
                arg(0)
            )
        }
        ("core.compress.zstd", "compress") => {
            format!("{}(&({}))", regex_fn("jet_compress_zstd_compress"), arg(0))
        }
        ("core.compress.zstd", "decompress") => {
            format!(
                "{}(&({}))",
                regex_fn("jet_compress_zstd_decompress"),
                arg(0)
            )
        }
        // D-DBDRIVER1: core.db — SQLite via the FFI bridge crate. `open`/`open_memory`
        // are the only module-level entry points; they wrap the bridge's raw u64
        // handle in the Jet-visible `DBConnection` handle (`JetDbConnection`), so
        // every other operation dispatches by receiver TYPE as an instance method
        // (`THandleOp::DBQuery`/… in the `HandleMethod` arm below), not a second
        // module-call surface.
        ("core.db", "open") => {
            format!(
                "{}JetDbConnection {{ handle: {}(&({})) }}",
                cx.root_prefix,
                regex_fn("jet_db_open"),
                arg(0)
            )
        }
        ("core.db", "open_memory") => {
            format!(
                "{}JetDbConnection {{ handle: {}() }}",
                cx.root_prefix,
                regex_fn("jet_db_open_memory")
            )
        }
        ("core.db", "policy") => format!(
            "{}jet_db_policy_new(({}).clone(), ({}).clone())",
            cx.root_prefix,
            arg(0),
            arg(1)
        ),
        ("core.plugin", "load") => {
            format!(
                "{root}JetPlugin {{ handle: {root}jet_std::jet_plugin_load_handle(&{}(&({}))) }}",
                regex_fn("jet_plugin_load"),
                arg(0),
                root = cx.root_prefix,
            )
        }
        // c109 Phase 20: the polymorphic core specials.
        // Their return type is arg-type dependent (resolved by sema's bespoke
        // `infer_core_call` and written onto the node's `resolved_ret`, read at
        // lowering), but the EMITTED form is a fixed per-`(module, method)` string —
        // no type decision here (I3). Args are emitted PLAINLY.
        ("core.math", "abs") => format!("({}).abs()", arg(0)),
        ("core.math", "min") => format!("({}).min({})", arg(0), arg(1)),
        ("core.math", "max") => format!("({}).max({})", arg(0), arg(1)),
        ("core.math", "clamp") => format!("({}).clamp({}, {})", arg(0), arg(1), arg(2)),
        
        ("core.random", "shuffle") => {
            format!("{}(&mut ({}))", helper("jet_std_random_shuffle"), arg(0))
        }
        ("core.io", "eprint") => format!("eprintln!(\"{{}}\", ({}).jet_show())", arg(0)),
        ("core.io", "print") => format!("println!(\"{{}}\", ({}).jet_show())", arg(0)),
        // D-TERM1 (ratified 2026-06-22): terminal direct-input.
        
        // D-FIDELITY-API1=A: runtime-global fidelity signal.
        
        
        
        // D-RENDERTGT2=A (c133 M1): UI backend seam constructors.
        
        
        // D-UIDEVSHELL1=A (c134 Phase 8): native Linux GTK4 backend constructor.
        
        // D-UI-MOUNT1=A: free-fn spelling of the backend mount pipeline.
        ("core.ui", "mount") => {
            let backend = arg(0);
            let tree = arg(1);
            if args.len() >= 3 {
                format!(
                    "({}).mount_node(({}).clone(), ({}).clone())",
                    backend,
                    tree,
                    arg(2)
                )
            } else {
                format!("({}).mount_node_default(({}).clone())", backend, tree)
            }
        }
        
        
        
        
        
        ("core.ui", "box") => {
            format!("{}jet_ui_box(({}).clone())", cx.root_prefix, arg(0))
        }
        
        
        // D-A11YGATE1=B (c134 Phase 6): accessible-role node + role constants.
        
        // D-STYLESHAPE1=A wiring: a node carrying an explicit fill color.
        
        ("core.web", "on") => "{ let _ = || (); () }".to_string(),
        ("core.web", "value") => "String::new()".to_string(),
        // D-WEBAPP1=D: application builder + page helper.
        
        ("core.web", "page") => {
            format!(
                "{}jet_web_page(({}).clone(), ({}).clone())",
                cx.root_prefix,
                arg(0),
                arg(1)
            )
        },
        // D-LIVEQUERY1=A (#505): live query registry + invalidation.
        ("app" | "core.web", "live") => format!(
            "{}jet_app_live(({}).clone(), ({}).clone())",
            cx.root_prefix,
            arg(0),
            arg(1)
        ),
        ("app" | "core.web", "subscribe") => format!(
            "{}jet_app_subscribe(({}).clone())",
            cx.root_prefix,
            arg(0)
        ),
        ("app" | "core.web", "invalidate") => format!(
            "{}jet_app_invalidate(({}).clone())",
            cx.root_prefix,
            arg(0)
        ),
        ("app" | "core.web", "transact_invalidate") => format!(
            "{}jet_app_transact_invalidate(({}).clone())",
            cx.root_prefix,
            arg(0)
        ),
        ("app" | "core.web", "signal_push") => format!(
            "{}jet_app_signal_push(&({}), ({}).clone())",
            cx.root_prefix,
            arg(0),
            arg(1)
        ),
        ("app" | "core.web", "auth") => format!(
            "{}jet_app_auth(({}).clone())",
            cx.root_prefix,
            arg(0)
        ),
        ("app" | "core.web", "auth_oauth") => format!(
            "{}jet_app_auth_oauth(({}), ({}).clone())",
            cx.root_prefix,
            arg(0),
            arg(1)
        ),
        ("app" | "core.web", "sync_over") => format!(
            "{}jet_app_sync_over(({}).clone(), ({}).clone())",
            cx.root_prefix,
            arg(0),
            arg(1)
        ),
        ("app" | "core.web", "sync") => format!(
            "{}jet_app_sync(({}).clone(), ({}).clone())",
            cx.root_prefix,
            arg(0),
            arg(1)
        ),
        ("core.web.storage.local" | "core.web.storage.session", "get") => {
            "None::<String>".to_string()
        }
        ("core.web.storage.local" | "core.web.storage.session", "set" | "remove" | "clear") => {
            "()".to_string()
        }
        // c-devserver (owner-directed 2026-07-01): `devserver.for_app(file)`
        // constructor — the builder methods dispatch through
        // `THandleOp::DevServerMethod` above, not here.
        
        
        
        
        // D-NETDEP1=A / D-HTTPLIB1=A: HTTP client constructors.
        // Bridge returns primitives; CoreLib assembles the one shared response.
        ("core.http.client", "get") => {
            let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
            let u = if matches!(args.get(0).map(|e| &e.ty), Some(Type::Named(n)) if n == "Url") {
                format!("({}).to_string_value()", arg(0))
            } else {
                arg(0)
            };
            emit_http_response_from_bridge(
                format!("{ffi}::JetHTTPAmbientDeadline::push(jet_deadline_remaining_ms()).and_then(|_ambient| {ffi}::jet_http_client_get_impl(&({u})))"),
                ffi,
            )
        }
        ("core.http.client", "post") => {
            let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
            let u = if matches!(args.get(0).map(|e| &e.ty), Some(Type::Named(n)) if n == "Url") {
                format!("({}).to_string_value()", arg(0))
            } else {
                arg(0)
            };
            emit_http_response_from_bridge(
                format!("{ffi}::JetHTTPAmbientDeadline::push(jet_deadline_remaining_ms()).and_then(|_ambient| {ffi}::jet_http_client_post_impl(&({u}), &({})))", arg(1)),
                ffi,
            )
        }
        ("core.http.client", "request") => {
            let u = if matches!(args.get(1).map(|e| &e.ty), Some(Type::Named(n)) if n == "Url") {
                format!("({}).to_string_value()", arg(1))
            } else {
                arg(1)
            };
            format!("jet_http_client_request_new(&({}), &({}))", arg(0), u)
        }
        // D-BROWSER-AUTO1=A: native versioned BiDi entry points.
        
        
        
        // D-NETDEP1=A / D-HTTPLIB1=A: HTTP server constructors (CoreLib, no prefix needed).
        ("core.http.server", "bind")
            if args.len() == 3 && !matches!(&args[2].ty, Type::Option(_)) =>
        {
            let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
            format!(
                "jet_http_server_bind_tls(&({}), {}, {}, |cert, key| {ffi}::jet_http_server_tls_validate_impl(cert, key), |cert, key, stream, on_request, on_h2, should_stop| {ffi}::jet_http_server_tls_session_impl(cert, key, stream, on_request, on_h2, should_stop)).map_err(|e| JetHTTPError::IO {{ operation: e }})",
                arg(0),
                arg(1),
                arg(2)
            )
        }
        ("core.http.server", "bind") => format!("jet_http_server_bind(&({}), {}).map_err(|_| JetHTTPError::IO {{ operation: \"bind\".to_string() }})", arg(0), arg(1)),
        
        ("core.http.server", "serve")
            if args.len() == 3 && !matches!(&args[2].ty, Type::Option(_)) =>
        {
            let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
            format!(
                "jet_http_mux_serve_tls(&({}), {}, {}, |cert, key| {ffi}::jet_http_server_tls_validate_impl(cert, key), |cert, key, stream, on_request, on_h2, should_stop| {ffi}::jet_http_server_tls_session_impl(cert, key, stream, on_request, on_h2, should_stop)).map_err(|e| JetHTTPError::IO {{ operation: e }})",
                arg(0),
                arg(1),
                arg(2)
            )
        }
        ("core.http.server", "serve") => format!("jet_http_mux_serve(&({}), {}).map_err(|_| JetHTTPError::IO {{ operation: \"serve\".to_string() }})", arg(0), arg(1)),
        ("core.http.server", "serve_once") => {
            format!("jet_http_mux_serve_once(&({}), {}).map_err(|_| JetHTTPError::IO {{ operation: \"serve once\".to_string() }})", arg(0), arg(1))
        }
        ("core.http.server", "serve_once_listener") => {
            format!(
                "jet_http_mux_serve_once_listener(&({}), &({})).map_err(|_| JetHTTPError::IO {{ operation: \"serve listener\".to_string() }})",
                arg(0),
                arg(1)
            )
        }
        
        
        ("core.http.server", "static_file") => {
            format!("jet_http_srv_static_file(&({}), &({})).map_err(|_| JetHTTPError::IO {{ operation: \"read static file\".to_string() }})", arg(0), arg(1))
        }
        ("core.http.server", "static_file_range") => format!(
            "jet_http_srv_static_file_range(&({}), &({}), &({})).map_err(|_| JetHTTPError::IO {{ operation: \"read static file\".to_string() }})",
            arg(0),
            arg(1),
            arg(2)
        ),
        // D-HTTP-JSON1=A: one JSON response with its content type set.
        ("core.http.server", "static_files") => {
            let option = |index: usize| {
                args.get(index)
                    .map_or_else(|| "None".to_string(), |_| format!("Some({})", arg(index)))
            };
            format!(
                "jet_http_srv_static_files_mount_defaulted(&({}), &({}), &({}), {}, {}, {})",
                arg(0),
                arg(1),
                arg(2),
                option(3),
                option(4),
                option(5),
            )
        }
        // D-HTTP-CORS1=A: `origins` takes a plain list or the `.Any` case.
        ("core.http.server", "cors_policy") => {
            let origins = if matches!(args.first().map(|a| &a.ty), Some(Type::List(_))) {
                format!("JetHTTPCorsOrigins::List({})", arg(0))
            } else {
                arg(0)
            };
            let list = |index: usize| {
                args.get(index)
                    .map_or_else(|| "None".to_string(), |_| format!("Some(&({}))", arg(index)))
            };
            let value = |index: usize| {
                args.get(index)
                    .map_or_else(|| "None".to_string(), |_| format!("Some({})", arg(index)))
            };
            format!(
                "jet_http_cors_policy_defaulted(&({}), {}, {}, {}, {})",
                origins,
                list(1),
                list(2),
                value(3),
                value(4),
            )
        }
        
        
        // D-TIMEDEPTH1=A: civil-time constructors.
        ("core.time.date", "new") => {
            format!("JetDate::new({}, {}, {})", arg(0), arg(1), arg(2))
        }
        ("core.time.date", "today") => "JetDate::today_utc()".to_string(),
        ("core.time.date", "parse") => format!("JetDate::parse(&({})).map_err(|e| e)", arg(0)),
        ("core.time.datetime", "from_timestamp") => {
            format!("JetDateTime::from_timestamp({})", arg(0))
        }
        ("core.time.datetime", "now") => "JetDateTime::now()".to_string(),
        
        _ => "/* unknown std call */".to_string(),
    }
}
