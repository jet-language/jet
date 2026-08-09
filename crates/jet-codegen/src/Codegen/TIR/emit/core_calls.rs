use crate::AST::{Type};
use crate::Codegen::escape_rust_str;
use crate::Codegen::Cx;
use crate::Codegen::mangle;
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
use crate::Codegen::TIR::TExpr;

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
            format!(
                "let __jet_state_{index} = jet_compute_vjp_begin((({output}).{}).clone(), ({tape}).clone());",
                mangle(field)
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let state_names = output_fields
        .iter()
        .enumerate()
        .map(|(index, _)| format!("__jet_state_{index}"))
        .collect::<Vec<_>>();
    let gradient_defs = format!(
        "let __jet_nested_gradients = jet_compute_nested_gradient_or_panic(&[{}], &{}, \"compute.gradient\");",
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
                    format!(
                        "(__jet_nested_gradients[{component_index}])[{target_index}].clone()"
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
            .map(|index| format!("({inputs})[{index}].clone()"))
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
                Some(format!(
                    "{{ let __jet_result = jet_compute_transform_or_panic(\"gradient\", &{state}, &[], &{target_expr}, \"compute.gradient\"); let JetComputeTransformResult::Gradient(__jet_gradients) = __jet_result else {{ jet_panic(\"Compute.rs\", line!(), \"compute.gradient returned the wrong result\") }}; {} }}",
                    compute_gradient_tuple(gradient_ty, "__jet_gradients")
                ))
            }
            "value_and_gradient" => {
                let gradient_ty = gradient_ty.as_ref()?;
                Some(format!(
                    "{{ let __jet_result = jet_compute_transform_or_panic(\"value_and_gradient\", &{state}, &[], &{target_expr}, \"compute.value_and_gradient\"); let JetComputeTransformResult::ValueAndGradient {{ value: __jet_value, gradients: __jet_gradients }} = __jet_result else {{ jet_panic(\"Compute.rs\", line!(), \"compute.value_and_gradient returned the wrong result\") }}; {} }}",
                    compute_tuple_value(result_ty, &["__jet_value".to_string(), compute_gradient_tuple(gradient_ty, "__jet_gradients")])
                ))
            }
            "vjp" => {
                let gradient_ty = gradient_ty.as_ref()?;
                Some(format!(
                    "{{ let __jet_result = jet_compute_transform_or_panic(\"vjp\", &{state}, &[], &{target_expr}, \"compute.vjp\"); let JetComputeTransformResult::Vjp {{ value: __jet_vjp_value, state: __jet_vjp_state }} = __jet_result else {{ jet_panic(\"Compute.rs\", line!(), \"compute.vjp returned the wrong result\") }}; let __jet_pull_state = __jet_vjp_state.clone(); let __jet_grads_state = __jet_vjp_state; let __jet_pull_targets = {target_expr}.clone(); let __jet_grads_targets = {target_expr}.clone(); JetComputeVjpRun {{ value: __jet_vjp_value, pull: std::rc::Rc::new(move |__jet_seed: JetTensor| {{ let __jet_gradients = jet_compute_vjp_pull_or_panic(&__jet_pull_state, &__jet_seed, &__jet_pull_targets, \"compute.vjp.pull\"); {} }}), grads: std::rc::Rc::new(move || {{ let __jet_gradients = jet_compute_vjp_unit_grads_or_panic(&__jet_grads_state, &__jet_grads_targets, \"compute.vjp.grads\"); {} }}) }} }}",
                    compute_gradient_tuple(gradient_ty, "__jet_gradients"),
                    compute_gradient_tuple(gradient_ty, "__jet_gradients")
                ))
            }
            "jvp" => {
                let tangents = if transform {
                    (0..primal_count)
                        .map(|index| format!("__jet_arg{}", index + primal_count))
                        .collect::<Vec<_>>()
                } else {
                    (0..primal_count)
                        .map(|index| emit_tir_expr(&args[index + 1 + primal_count], cx))
                        .collect::<Vec<_>>()
                };
                Some(format!(
                    "{{ let __jet_result = jet_compute_transform_or_panic(\"jvp\", &{state}, &[{}], &{target_expr}, \"compute.jvp\"); let JetComputeTransformResult::Jvp {{ value: __jet_value, tangent: __jet_tangent }} = __jet_result else {{ jet_panic(\"Compute.rs\", line!(), \"compute.jvp returned the wrong result\") }}; {} }}",
                    tangents.join(", "),
                    compute_tuple_value(
                        result_ty,
                        &[
                            "__jet_value".to_string(),
                            "__jet_tangent".to_string(),
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
                .map(|(index, ty)| format!("__jet_arg{index}: {}", cx.rust_type(ty)))
                .collect::<Vec<_>>()
        } else {
            base_params
                .iter()
                .enumerate()
                .map(|(index, ty)| format!("__jet_arg{index}: {}", cx.rust_type(ty)))
                .collect::<Vec<_>>()
        };
        let target = format!("__jet_targets");
        let state_setup = if nested_gradient {
            String::new()
        } else {
            "let __jet_state = jet_compute_vjp_begin(__jet_value.clone(), __jet_tape.clone());".to_string()
        };
        let result = result_body("__jet_value", "__jet_state", "__jet_tape", &target)?;
        Some(format!(
            "{{ let __jet_base = ({f}).clone(); std::rc::Rc::new(move |{}| {{ let (__jet_tape, __jet_inputs) = jet_compute_trace_inputs(vec![{}]); let __jet_value = (__jet_base)({}); {state_setup} let __jet_targets = {}; {} }}) as {} }}",
            params.join(", "),
            (0..primal_count)
                .map(|index| format!("__jet_arg{index}.clone()"))
                .collect::<Vec<_>>()
                .join(", "),
            (0..primal_count)
                .map(|index| format!("(__jet_inputs)[{index}].clone()"))
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
        let call = base_call(&f, "__jet_inputs");
        let state_setup = if nested_gradient {
            String::new()
        } else {
            "let __jet_state = jet_compute_vjp_begin(__jet_value.clone(), __jet_tape.clone());".to_string()
        };
        let result = result_body("__jet_value", "__jet_state", "__jet_tape", "__jet_targets")?;
        Some(format!(
            "{{ let (__jet_tape, __jet_inputs) = jet_compute_trace_inputs(vec![{trace_inputs}]); let __jet_value = {call}; {state_setup} let __jet_targets = {targets}; {result} }}"
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

/// #1635: plain core calls that carry nothing but a Prelude symbol name and a
/// borrow mask -- (module, method, symbol, prefixed_with_root, arg_borrow_mask).
/// `prefixed_with_root` means `{cx.root_prefix}{symbol}`; otherwise `symbol` is
/// emitted verbatim. Looked up by `emit_plain_core_call` before the bespoke match.
const PLAIN_CORE_CALLS: &[(&str, &str, &str, bool, &[bool])] = &[
    ("core.mem", "volatile_read", "std::ptr::read_volatile", false, &[false]),
    ("core.mem", "volatile_write", "std::ptr::write_volatile", false, &[false, false]),
    ("core.tasks", "interval", "jet_std::interval", true, &[false]),
    ("core.tasks", "yield_now", "jet_std::jet_task_yield", true, &[]),
    ("core.tasks", "current_task", "jet_std::jet_task_current_trace", true, &[]),
    ("core.reactive", "signal", "jet_std::JetSignal::new", true, &[false]), // D-REACT1=B: `reactive.signal(initial)` producer → a `JetSignal<T>`.
    ("core.event", "scope", "jet_std::JetEventScope::new", true, &[]), // D-EVENT1=D: first-party typed Event/Hook constructors.
    ("core.event", "policy_sync", "jet_std::JetEventPolicy::sync", true, &[]),
    ("core.science.measurement", "from", "jet_std::JetMeasurement::new", true, &[false, false]), // D-HONESTNUM1=A: `M.from(value, uncertainty)` → a `JetMeasurement<f64>`.
    ("core.math", "fraction", "jet_fraction_new", true, &[false, false]), // D-CORE-NUMERIC1=A: `core.math.decimal(s)` → exact parse.
    ("core.math", "decimal", "jet_decimal_from_str", true, &[true]),
    ("core.files", "read", "jet_std_fs_read", true, &[true]), // D-FILES-WRITE1 (merge, was `core.fs`): whole-file convenience helpers now // live in `core.files` alongside the streaming handle constructors below. // D-FILES-APPEND1=A: whole-file one-shot is `append_all`, not `append` — // that name stays reserved for the streaming handle's `.append(text)`.
    ("core.files", "read_bytes", "jet_std_fs_read_bytes", true, &[true]),
    ("core.files", "write", "jet_std_fs_write", true, &[true, true]),
    ("core.files", "append_all", "jet_std_fs_append", true, &[true, true]),
    ("core.files", "exists", "jet_std_fs_exists", true, &[true]),
    ("core.files", "remove", "jet_std_fs_remove", true, &[true]),
    ("core.files", "remove_dir", "jet_std_fs_remove_dir", true, &[true]),
    ("core.files", "remove_all", "jet_std_fs_remove_all", true, &[true]),
    ("core.files", "list_dir", "jet_std_fs_list_dir", true, &[true]),
    ("core.files", "create_dir", "jet_std_fs_create_dir", true, &[true]),
    ("core.files", "create_dir_all", "jet_std_fs_create_dir_all", true, &[true]),
    ("core.files", "is_dir", "jet_std_fs_is_dir", true, &[true]),
    ("core.files", "copy", "jet_std_fs_copy", true, &[true, true]),
    ("core.files", "copy_dir", "jet_std_fs_copy_dir", true, &[true, true]),
    ("core.files", "rename", "jet_std_fs_rename", true, &[true, true]),
    ("core.files", "symlink", "jet_std_fs_symlink", true, &[true, true]),
    ("core.files", "read_link", "jet_std_fs_read_link", true, &[true]),
    ("core.files", "hard_link", "jet_std_fs_hard_link", true, &[true, true]),
    ("core.files", "stat", "jet_std_fs_stat", true, &[true]),
    ("core.files", "canonicalize", "jet_std_fs_canonicalize", true, &[true]),
    ("core.files", "absolute", "jet_std_fs_absolute", true, &[true]),
    ("core.files", "walk", "jet_std_fs_walk", true, &[true]),
    ("core.files", "glob", "jet_std_fs_glob", true, &[true]),
    ("core.files", "read_at", "jet_std_fs_read_at", true, &[true, false, false]),
    ("core.files", "write_at", "jet_std_fs_write_at", true, &[true, false, true]),
    ("core.files", "fsync", "jet_std_fs_fsync", true, &[true]),
    ("core.files", "write_atomic", "jet_std_fs_write_atomic", true, &[true, true]),
    ("core.files", "temp_dir", "jet_std_fs_temp_dir", true, &[true]),
    ("core.files", "temp_file", "jet_std_fs_temp_file", true, &[true]),
    ("core.files", "lock", "jet_std_fs_lock", true, &[true]),
    ("core.watcher", "files", "jet_watcher_files", true, &[true]),
    ("core.watcher", "process_pid", "jet_watcher_process_pid", true, &[false]),
    ("core.watcher", "port", "jet_watcher_port", true, &[true, false]),
    ("core.watcher", "set", "jet_watcher_set", true, &[]),
    ("core.io", "args", "jet_std_io_args", true, &[]),
    ("core.args", "spec", "jet_args_spec", true, &[]), // D-ARGS1: `args.spec()` → empty builder.
    ("core.io", "confirm", "jet_std_io_confirm", true, &[true]),
    ("core.io", "choose", "jet_std_io_choose", true, &[true, true]),
    ("core.io", "input_secret", "jet_std_io_input_secret", true, &[true]),
    ("core.io", "read_all_input", "jet_std_io_read_all_input", true, &[]),
    ("core.io", "readline", "jet_std_io_readline", true, &[]),
    ("core.io", "read_until", "jet_std_io_read_until", true, &[true]),
    ("core.io", "take", "jet_std_io_take", true, &[false]),
    ("core.io", "buffered", "jet_std_io_buffered", true, &[]),
    ("core.io", "sprint", "jet_std_io_sprint", true, &[true]),
    ("core.io", "repr", "jet_std_io_repr", true, &[true]),
    ("core.io", "binread", "jet_std_io_binread", true, &[true]),
    ("core.io", "binwrite", "jet_std_io_binwrite", true, &[true, true]),
    ("core.io", "stdin", "jet_std_io_stdin", true, &[]), // D-STDIN1=A: io.stdin() → JetStdinReader handle.
    ("core.io", "stdout", "jet_std_io_stdout", true, &[]),
    ("core.io", "stderr", "jet_std_io_stderr", true, &[]),
    ("core.io", "terminal_width", "jet_std_io_terminal_width", true, &[]),
    ("core.io", "terminal_height", "jet_std_io_terminal_height", true, &[]),
    ("core.io", "style", "jet_std_io_style", true, &[true, true]),
    ("core.io", "style_force", "jet_std_io_style_force", true, &[true, true]),
    ("core.env", "get", "jet_std_env_get", true, &[true]),
    ("core.env", "set", "jet_std_env_set", true, &[true, true]),
    ("core.env", "unset", "jet_std_env_unset", true, &[true]),
    ("core.env", "vars", "jet_std_env_vars", true, &[]),
    ("core.env", "current_dir", "jet_std_env_current_dir", true, &[]),
    ("core.env", "home_dir", "jet_std_env_home_dir", true, &[]),
    ("core.os", "name", "jet_std_os_name", true, &[]),
    ("core.os", "family", "jet_std_os_family", true, &[]),
    ("core.os", "arch", "jet_std_os_arch", true, &[]),
    ("core.os", "cpu_count", "jet_std_os_cpu_count", true, &[]),
    ("core.os", "temp_dir", "jet_std_os_temp_dir", true, &[]),
    ("core.os", "executable", "jet_std_os_executable", true, &[]),
    ("core.os", "pid", "jet_std_os_pid", true, &[]),
    ("core.os", "getpid", "jet_std_os_pid", true, &[]),
    ("core.os", "hostname", "jet_std_os_hostname", true, &[]),
    ("core.os", "username", "jet_std_os_username", true, &[]),
    ("core.os", "release", "jet_std_os_release", true, &[]),
    ("core.os", "version", "jet_std_os_version", true, &[]),
    ("core.os", "expand", "jet_std_os_expand", true, &[true]),
    ("core.os", "getppid", "jet_std_os_getppid", true, &[]),
    ("core.os", "getuid", "jet_std_os_getuid", true, &[]),
    ("core.os", "geteuid", "jet_std_os_geteuid", true, &[]),
    ("core.os", "getgid", "jet_std_os_getgid", true, &[]),
    ("core.os", "getegid", "jet_std_os_getegid", true, &[]),
    ("core.os", "getgroups", "jet_std_os_getgroups", true, &[]),
    ("core.os", "getpgrp", "jet_std_os_getpgrp", true, &[]),
    ("core.os", "uptime", "jet_std_os_uptime", true, &[]),
    ("core.os", "loadavg", "jet_std_os_loadavg", true, &[]),
    ("core.os", "times", "jet_std_os_times", true, &[]),
    ("core.os", "sync", "jet_std_os_sync", true, &[]),
    ("core.os", "getpgid", "jet_std_os_getpgid", true, &[false]),
    ("core.os", "getsid", "jet_std_os_getsid", true, &[false]),
    ("core.os", "exitcode", "jet_std_os_exitcode", true, &[false]),
    ("core.os", "success", "jet_std_os_success", true, &[false]),
    ("core.os", "umask", "jet_std_os_umask", true, &[false]),
    ("core.os", "getpriority", "jet_std_os_getpriority", true, &[false]),
    ("core.os", "setpriority", "jet_std_os_setpriority", true, &[false, false]),
    ("core.os", "utime", "jet_std_os_utime", true, &[true, false, false]),
    ("core.os", "stop", "jet_std_os_stop", true, &[false]),
    ("core.os", "set_current_dir", "jet_std_os_set_current_dir", true, &[true]),
    ("core.os", "on_interrupt", "jet_std_os_on_interrupt", true, &[false]),
    ("core.os", "atexit", "jet_std_os_atexit", true, &[false]),
    ("core.os", "fork", "jet_std_os_fork", true, &[]),
    ("core.os", "setuid", "jet_std_os_setuid", true, &[false]),
    ("core.os", "setgid", "jet_std_os_setgid", true, &[false]),
    ("core.os", "setpgid", "jet_std_os_setpgid", true, &[false, false]),
    ("core.os", "setpgrp", "jet_std_os_setpgrp", true, &[]),
    ("core.os", "setsid", "jet_std_os_setsid", true, &[]),
    ("core.os", "initgroups", "jet_std_os_initgroups", true, &[true, false]),
    ("core.os", "kill", "jet_std_os_kill", true, &[false, false]),
    ("core.os", "wait", "jet_std_os_wait", true, &[]),
    ("core.os", "waitpid", "jet_std_os_waitpid", true, &[false, false]),
    ("core.os", "pipe", "jet_std_os_pipe", true, &[]),
    ("core.os", "close_fd", "jet_std_os_close_fd", true, &[false]),
    ("core.os", "mkfifo", "jet_std_os_mkfifo", true, &[true, false]),
    ("core.process", "exit", "jet_std_process_exit", true, &[false]),
    ("core.process", "run", "jet_std_process_run", true, &[true]),
    ("core.process", "cmd", "jet_std_process_cmd", true, &[true]),
    ("core.process", "pipeline", "jet_std_process_pipeline", true, &[true]),
    ("core.testing", "snap", "jet_testing_snap", true, &[true, true]),
    ("core.testing", "golden", "jet_testing_golden", true, &[true, true]),
    ("core.testing", "fixture", "jet_testing_fixture", true, &[true]),
    ("core.testing", "temp_dir", "jet_testing_temp_dir", true, &[true]),
    ("core.testing", "corpus", "jet_testing_corpus", true, &[true]),
    ("core.testing", "fake_clock", "jet_std_clock_new", true, &[false]),
    ("core.testing", "fake_rng", "jet_std_rng_new", true, &[false]),
    ("core.math", "round", "jet_std_math_round", true, &[false]),
    ("core.math", "isqrt", "jet_std_math_isqrt", true, &[false]),
    ("core.math", "factorial", "jet_std_math_factorial", true, &[false]),
    ("core.math", "erf", "jet_std_math_erf", true, &[false]),
    ("core.math", "erfc", "jet_std_math_erfc", true, &[false]),
    ("core.math", "gamma", "jet_std_math_gamma", true, &[false]),
    ("core.math", "lgamma", "jet_std_math_lgamma", true, &[false]),
    ("core.math", "logb", "jet_std_math_logb", true, &[false]),
    ("core.math", "significand", "jet_std_math_significand", true, &[false]),
    ("core.math", "ulp", "jet_std_math_ulp", true, &[false]),
    ("core.math", "cmp", "jet_std_math_cmp", true, &[false, false]),
    ("core.math", "next_after", "jet_std_math_next_after", true, &[false, false]),
    ("core.math", "ldexp", "jet_std_math_ldexp", true, &[false, false]),
    ("core.math", "scaleb", "jet_std_math_ldexp", true, &[false, false]),
    ("core.math", "ilogb", "jet_std_math_ilogb", true, &[false]),
    ("core.math", "leading_ones", "jet_std_math_leading_ones", true, &[false]),
    ("core.math", "trailing_ones", "jet_std_math_trailing_ones", true, &[false]),
    ("core.math", "digits", "jet_std_math_digits", true, &[false]),
    ("core.math", "binomial", "jet_std_math_binomial", true, &[false, false]),
    ("core.math", "checked_pow", "jet_std_math_checked_pow", true, &[false, false]),
    ("core.math", "int_pow", "jet_std_math_int_pow", true, &[false, false]),
    ("core.math", "gcd", "jet_std_math_gcd", true, &[false, false]),
    ("core.math", "lcm", "jet_std_math_lcm", true, &[false, false]),
    ("core.random", "int", "jet_std_random_int", true, &[false, false]),
    ("core.random", "float", "jet_std_random_float", true, &[]),
    ("core.random", "float_range", "jet_std_random_float_range", true, &[false, false]),
    ("core.random", "bool", "jet_std_random_bool", true, &[false]),
    ("core.random", "normal", "jet_std_random_normal", true, &[false, false]),
    ("core.random", "exponential", "jet_std_random_exponential", true, &[false]),
    ("core.random", "seed", "jet_std_random_seed", true, &[false]),
    ("core.random", "bytes", "jet_std_random_bytes", true, &[false]), // D-RANDSPLIT1=A: PRNG bytes — fast, NOT crypto-safe.
    ("core.crypto.random", "bytes", "jet_std_crypto_random_bytes", true, &[false]), // D-CRYPTO-RNG1=A: shared fail-closed OS CSPRNG provider.
    ("core.random", "rng", "jet_std_rng_new", true, &[false]), // D-DET1: deterministic injected RNG capability constructor.
    ("core.random", "split", "jet_std_random_split", true, &[false]),
    ("core.time", "now", "jet_std_time_now", true, &[]),
    ("core.time", "sleep", "jet_std_time_sleep", true, &[false]),
    ("core.time", "start", "jet_std_time_start", true, &[]),
    ("core.time", "instant", "jet_time_instant_now", true, &[]),
    ("core.time", "now_utc", "jet_time_now_utc", true, &[]),
    ("core.time", "from_unix_ms", "JetDateTime::from_unix_ms", false, &[false]),
    ("core.time", "today", "jet_time_today", true, &[]),
    ("core.time", "parse_rfc3339", "jet_time_parse_rfc3339", true, &[true]),
    ("core.time", "datetime", "jet_time_datetime", true, &[false, false, false, false, false, false]),
    ("core.time", "time", "JetLocalTime::new", false, &[false, false, false]),
    ("core.time", "local_time", "JetLocalTime::new", false, &[false, false, false]),
    ("core.time", "days_in_month", "jet_time_days_in_month", true, &[false, false]),
    ("core.time", "is_leap_year", "jet_time_is_leap_year", true, &[false]),
    ("core.time", "period", "jet_time_period", true, &[false, false, false]),
    ("core.time", "period_days", "jet_time_period_days", true, &[false]),
    ("core.time", "period_months", "jet_time_period_months", true, &[false]),
    ("core.time", "period_years", "jet_time_period_years", true, &[false]),
    ("core.time", "zone", "jet_time_zone_named", true, &[true]),
    ("core.time", "utc", "jet_time_zone_utc", true, &[]),
    ("core.time", "zoned", "jet_time_zoned", true, &[true, true]),
    ("core.time", "zoned_local", "jet_time_zoned_local", true, &[true, true, true]),
    ("core.time", "clock", "jet_std_clock_new", true, &[false]), // D-DET1: deterministic injected Clock capability constructor.
    ("core.encoding.json", "parse", "jet_std_json_parse", true, &[true]), // D-ENC1 + D-JSONVERB1 + D-SERDE6: unified `core.encoding.*`. The dynamic forms // (`JSON` tree / `[[String]]` / `Map`) keep their existing helpers; the typed // forms route through the Encode/Decode model, distinguished by the lowered arg // type (encode) or the resolved return type (decode). `is_json_value` etc. read // those total facts — codegen never re-infers (I3).
    ("core.encoding.json", "events", "jet_std_json_events", true, &[true]),
    ("core.encoding.jsonl", "parse", "jet_std_jsonl_parse", true, &[true]),
    ("core.encoding.jsonl", "to_string", "jet_std_jsonl_render", true, &[true]),
    ("core.encoding.csv", "parse", "jet_ring_csv_parse", true, &[true]),
    ("core.data", "count", "jet_data_count", true, &[true]),
    ("core.compute", "zeros", "jet_compute_zeros", true, &[true]), // D-COMPUTE1=D (#443): Tensor CPU oracle — one Prelude symbol per call.
    ("core.compute", "ones", "jet_compute_ones", true, &[true]),
    ("core.compute", "full", "jet_compute_full", true, &[true, false]),
    ("core.compute", "from_list", "jet_compute_from_list", true, &[true]),
    ("core.compute", "matrix", "jet_compute_matrix", true, &[false, false, false]),
    ("core.compute", "vec", "jet_compute_vec", true, &[false, false]),
    ("core.compute", "add", "jet_compute_add", true, &[true, true]),
    ("core.compute", "mul", "jet_compute_mul", true, &[true, true]),
    ("core.compute", "matmul", "jet_compute_matmul", true, &[true, true]),
    ("core.compute", "reshape", "jet_compute_reshape", true, &[true, true]),
    ("core.compute", "get", "jet_compute_get", true, &[true, true]),
    ("core.compute", "shape", "jet_compute_tensor_shape", true, &[true]),
    ("core.compute", "rank", "jet_compute_tensor_rank", true, &[true]),
    ("core.compute", "numel", "jet_compute_tensor_numel", true, &[true]),
    ("core.compute", "to_list", "jet_compute_tensor_to_list", true, &[true]),
    ("core.compute", "device", "jet_compute_tensor_device", true, &[true]),
    ("core.compute", "placement", "jet_compute_tensor_placement", true, &[true]),
    ("core.compute", "device_cpu", "jet_compute_device_cpu", true, &[]),
    ("core.compute", "device_auto", "jet_compute_device_auto", true, &[]),
    ("core.compute", "on_device", "jet_compute_on_device", true, &[true, false]),
    ("core.compute", "broadcast_to", "jet_compute_broadcast_to", true, &[true, true]),
    ("core.compute", "transpose", "jet_compute_transpose", true, &[true]),
    ("core.compute", "sum_axis", "jet_compute_sum_axis", true, &[true, false]),
    ("core.compute", "eye", "jet_compute_eye", true, &[false]),
    ("core.compute", "det", "jet_compute_det", true, &[true]),
    ("core.compute", "inv", "jet_compute_inv", true, &[true]),
    ("core.compute", "fft", "jet_compute_fft", true, &[true]),
    ("core.compute", "solve", "jet_compute_solve", true, &[true, true]),
    ("core.compute", "stream_new", "jet_compute_stream_new", true, &[]),
    ("core.compute", "stream_sync", "jet_compute_stream_sync", true, &[true]),
    ("core.compute", "stream_show", "jet_compute_stream_show", true, &[true]),
    ("core.compute", "transfer", "jet_compute_transfer", true, &[true, false]),
    ("core.compute", "transfer_show", "jet_compute_transfer_show", true, &[true]),
    ("core.compute", "kernel_bounds_ok", "jet_compute_kernel_bounds_ok", true, &[true, true]),
    ("core.compute", "mse_loss", "jet_compute_mse_loss", true, &[true, true]),
    ("core.compute", "sgd_step", "jet_compute_sgd_step", true, &[true, true, false]),
    ("core.compute", "serialize", "jet_compute_serialize", true, &[true]),
    ("core.compute", "deserialize", "jet_compute_deserialize", true, &[true]),
    ("core.compute", "to_sparse", "jet_compute_to_sparse", true, &[true]),
    ("core.compute", "sparse_nnz", "jet_compute_sparse_nnz", true, &[true]),
    ("core.compute", "sparse_mv", "jet_compute_sparse_mv", true, &[true, true]),
    ("core.compute", "sparse_show", "jet_compute_sparse_show", true, &[true]),
    ("core.compute", "matmul_f32_tile", "jet_compute_matmul_f32_tile", true, &[true, true]),
    ("core.compute", "profile_f32_strict", "jet_compute_profile_f32_strict", true, &[]),
    ("core.compute", "profile_show", "jet_compute_profile_show", true, &[]),
    ("core.services", "restart_one_for_one", "jet_services_restart_one_for_one", true, &[]),
    ("core.services", "restart_one_for_all", "jet_services_restart_one_for_all", true, &[]),
    ("core.services", "restart_rest_for_one", "jet_services_restart_rest_for_one", true, &[]),
    ("core.services", "delivery_at_most_once", "jet_services_delivery_at_most_once", true, &[]),
    ("core.services", "delivery_durable", "jet_services_delivery_durable", true, &[]),
    ("core.services", "mailbox_depth", "jet_services_mailbox_depth", true, &[true, true]),
    ("core.services", "restarts", "jet_services_restarts", true, &[true, true]),
    ("core.services", "dead_letter_count", "jet_services_dead_letter_count", true, &[true]),
    ("core.services", "restore_snapshot", "jet_services_restore_snapshot", true, &[true]),
    ("core.services", "event_count", "jet_services_event_count", true, &[true]),
    ("core.services", "replay_events", "jet_services_replay_events", true, &[true]),
    ("core.services", "workflow_history", "jet_services_workflow_history", true, &[true, false]),
    ("core.services", "directory_resolve", "jet_services_directory_resolve", true, &[true, true]),
    ("core.services", "directory_generation", "jet_services_directory_generation", true, &[true]),
    ("core.services", "upgrade_receipt", "jet_services_upgrade_receipt", true, &[true]),
    ("core.services", "observe", "jet_services_observe", true, &[true]),
    ("core.services", "endpoint_show", "jet_services_endpoint_show", true, &[true]),
    ("core.services", "tree_show", "jet_services_tree_show", true, &[true]),
    ("core.data", "table", "jet_data_table", true, &[true]),
    ("core.data", "rows", "jet_data_rows", true, &[true]),
    ("core.data", "series", "jet_data_series", true, &[true]),
    ("core.data", "values", "jet_data_series_values", true, &[true]),
    ("core.data", "missing_count", "jet_data_missing_count", true, &[true]),
    ("core.data", "lazy", "jet_data_lazy", true, &[true]),
    ("core.data", "plan", "jet_data_plan", true, &[true]),
    ("core.data", "filter", "jet_data_filter", true, &[true, false]),
    ("core.data", "lazy_filter", "jet_data_lazy_filter", true, &[true, false]),
    ("core.data", "lazy_sort_by", "jet_data_lazy_sort_by", true, &[true, false]),
    ("core.data", "status", "jet_data_status", true, &[]),
    ("core.data", "require_bridge", "jet_data_require_bridge", true, &[true]),
    ("core.data", "csv_reader", "jet_data_csv_reader", true, &[false, false]),
    ("core.data", "json_reader", "jet_data_json_reader", true, &[false, false]),
    ("core.fmt", "number", "jet_fmt_number", true, &[false]),
    ("core.fmt", "decimal", "jet_fmt_decimal", true, &[false, false]),
    ("core.fmt", "percent", "jet_fmt_percent", true, &[false, false]),
    ("core.fmt", "bytes", "jet_fmt_bytes", true, &[false]),
    ("core.fmt", "duration", "jet_fmt_duration", true, &[false]),
    ("core.fmt", "ordinal", "jet_fmt_ordinal", true, &[false]),
    ("core.fmt", "plural", "jet_fmt_plural", true, &[false, true, true]),
    ("core.fmt", "pad_left", "jet_fmt_pad_left", true, &[true, false, true]),
    ("core.fmt", "pad_right", "jet_fmt_pad_right", true, &[true, false, true]),
    ("core.fmt", "pad_center", "jet_fmt_pad_center", true, &[true, false, true]),
    ("core.encoding.toml", "parse", "jet_std_toml_parse", true, &[true]),
    ("core.encoding.yaml", "parse", "jet_std_yaml_parse", true, &[true]),
    ("core.encoding.xml", "parse", "jet_std_xml_parse", true, &[true]),
    ("core.encoding.xml", "parse_with", "jet_std_xml_parse_with", true, &[true, true]),
    ("core.encoding.xml", "to_string", "jet_std_xml_render", true, &[true]),
    ("core.encoding.xml", "canonical", "jet_std_xml_canonical", true, &[true, true]),
    ("core.encoding.xml", "root", "jet_std_xml_root", true, &[true]),
    ("core.encoding.xml", "attribute", "jet_std_xml_attribute", true, &[true, true]),
    ("core.encoding.xml", "content", "jet_std_xml_content", true, &[true]),
    ("core.encoding.cbor", "to_bytes", "jet_enc_cbor_to_bytes", true, &[true]),
    ("core.encoding.cbor", "to_bytes_canonical", "jet_enc_cbor_to_bytes_canonical", true, &[true]),
    ("core.encoding.hex", "encode", "jet_std_hex_encode", true, &[true]), // D-UUIDENC1=A: hex and base64 encode/decode.
    ("core.encoding.hex", "decode", "jet_std_hex_decode", true, &[true]),
    ("core.encoding.base64", "encode", "jet_std_b64_encode", true, &[true]),
    ("core.encoding.base64", "decode", "jet_std_b64_decode", true, &[true]),
    ("core.encoding.base64", "encode_url", "jet_std_b64url_encode", true, &[true]),
    ("core.encoding.base64", "decode_url", "jet_std_b64url_decode", true, &[true]),
    ("core.encoding.base32", "encode", "jet_std_base32_encode", true, &[true]),
    ("core.encoding.base32", "decode", "jet_std_base32_decode", true, &[true]),
    ("core.uuid", "v4", "jet_std_uuid_v4", true, &[]), // D-UUIDENC1=A: UUID v4 (CSPRNG) and v7 (injectable Clock).
    ("core.uuid", "v7", "jet_std_uuid_v7", true, &[true]),
    ("core.uuid", "v5", "jet_std_uuid_v5", true, &[true, true]), // #1481: `v5` (namespace+name, deterministic) and `parse` (validate // + normalize) — pure std, same UUID-as-String shape as v4/v7.
    ("core.uuid", "parse", "jet_std_uuid_parse", true, &[true]),
    ("core.files", "open", "jet_std_files_open", true, &[true]),
    ("core.files", "create", "jet_std_files_create", true, &[true]),
    ("core.files", "append", "jet_std_files_append", true, &[true]),
    ("core.path", "join", "jet_std_path_join", true, &[true, true]), // E2-M7: std.path helpers (D-IO1).
    ("core.path", "parent", "jet_std_path_parent", true, &[true]),
    ("core.path", "extension", "jet_std_path_extension", true, &[true]),
    ("core.path", "normalize", "jet_std_path_normalize", true, &[true]),
    ("core.url", "parse", "jet_url_parse", true, &[true]),
    ("core.url", "from_parts", "jet_url_from_parts", true, &[true, true, true, true, true]),
    ("core.url", "file", "jet_url_file", true, &[true]),
    ("core.url", "data", "jet_url_data", true, &[true, true]),
    ("core.url", "query", "jet_url_query", true, &[true]),
    ("core.url", "percent_encode", "jet_url_percent_encode_component", true, &[true]),
    ("core.url", "percent_decode", "jet_url_percent_decode_component", true, &[true]),
    ("core.mime", "parse", "jet_mime_parse", true, &[true]),
    ("core.mime", "from_extension", "jet_mime_from_extension", true, &[true]),
    ("core.mime", "extension", "jet_mime_extension", true, &[true]),
    ("core.email", "address", "jet_email::address", true, &[true]),
    ("core.email", "attachment", "jet_email::attachment", true, &[true, true, true]),
    ("core.email", "message", "jet_email::message", true, &[true, true, true, true, true, true, true]),
    ("core.email", "envelope", "jet_email::envelope", true, &[true, true]),
    ("core.email", "serialize", "jet_email::serialize", true, &[true]),
    ("core.text.unicode", "scalar_count", "jet_text_unicode_scalar_count", true, &[true]), // D-TEXTUNICODE1: std-only Unicode scalar helpers.
    ("core.text.unicode", "byte_count", "jet_text_unicode_byte_count", true, &[true]),
    ("core.text.unicode", "is_ascii", "jet_text_unicode_is_ascii", true, &[true]),
    ("core.text.unicode", "lower", "jet_text_unicode_lower", true, &[true]),
    ("core.text.unicode", "upper", "jet_text_unicode_upper", true, &[true]),
    ("core.text.unicode", "scalars", "jet_text_unicode_scalars", true, &[true]),
    ("core.text", "nfc", "jet_text_nfc", true, &[true]),
    ("core.text", "nfd", "jet_text_nfd", true, &[true]),
    ("core.text", "nfkc", "jet_text_nfkc", true, &[true]),
    ("core.text", "nfkd", "jet_text_nfkd", true, &[true]),
    ("core.text", "casefold", "jet_text_casefold", true, &[true]),
    ("core.text", "caseless_eq", "jet_text_caseless_eq", true, &[true, true]),
    ("core.text", "lower", "jet_text_lower", true, &[true]),
    ("core.text", "upper", "jet_text_upper", true, &[true]),
    ("core.text", "graphemes", "jet_text_graphemes", true, &[true]),
    ("core.text", "words", "jet_text_words", true, &[true]),
    ("core.text", "sentences", "jet_text_sentences", true, &[true]),
    ("core.text", "scalar_count", "jet_text_unicode_scalar_count", true, &[true]),
    ("core.text", "byte_count", "jet_text_unicode_byte_count", true, &[true]),
    ("core.text", "is_alphabetic", "jet_text_is_alphabetic", true, &[true]),
    ("core.text", "is_numeric", "jet_text_is_numeric", true, &[true]),
    ("core.text", "is_whitespace", "jet_text_is_whitespace", true, &[true]),
    ("core.text", "is_ascii", "jet_text_unicode_is_ascii", true, &[true]),
    ("core.text", "scalars", "jet_text_unicode_scalars", true, &[true]),
    ("core.text", "splitn", "jet_text_splitn", true, &[true, true, false]),
    ("core.text", "rsplitn", "jet_text_rsplitn", true, &[true, true, false]),
    ("core.text", "trim", "jet_text_trim", true, &[true]),
    ("core.text", "trim_start", "jet_text_trim_start", true, &[true]),
    ("core.text", "trim_end", "jet_text_trim_end", true, &[true]),
    ("core.text", "pad_start", "jet_text_pad_start", true, &[true, false, true]),
    ("core.text", "pad_end", "jet_text_pad_end", true, &[true, false, true]),
    ("core.text", "center", "jet_text_center", true, &[true, false, true]),
    ("core.text", "starts_any", "jet_text_starts_any", true, &[true, true]),
    ("core.text", "ends_any", "jet_text_ends_any", true, &[true, true]),
    ("core.text", "char_indices", "jet_text_char_indices", true, &[true]),
    ("core.log", "info", "jet_ring_log_info", true, &[true]), // E2-M9: first-party ring packages.
    ("core.log", "warn", "jet_ring_log_warn", true, &[true]),
    ("core.log", "error", "jet_ring_log_error", true, &[true]),
    ("core.log", "debug", "jet_ring_log_debug", true, &[true]),
    ("core.log", "critical", "jet_ring_log_critical", true, &[true]),
    ("core.log", "fatal", "jet_ring_log_fatal", true, &[true]),
    ("core.log", "disable", "jet_ring_log_disable", true, &[]),
    ("core.log", "flush", "jet_ring_log_flush", true, &[]),
    ("core.log", "enabled", "jet_ring_log_enabled", true, &[true]),
    ("core.log", "field", "jet_ring_log_field", true, &[true, true]),
    ("core.log", "int", "jet_ring_log_int", true, &[true, false]),
    ("core.log", "float", "jet_ring_log_float", true, &[true, false]),
    ("core.log", "bool", "jet_ring_log_bool", true, &[true, false]),
    ("core.log", "redact", "jet_ring_log_redact", true, &[true]),
    ("core.log", "info_fields", "jet_ring_log_info_fields", true, &[true, true]),
    ("core.log", "warn_fields", "jet_ring_log_warn_fields", true, &[true, true]),
    ("core.log", "error_fields", "jet_ring_log_error_fields", true, &[true, true]),
    ("core.log", "debug_fields", "jet_ring_log_debug_fields", true, &[true, true]),
    ("core.log", "span", "jet_ring_log_span", true, &[true]),
    ("core.log", "enter", "jet_ring_log_enter", true, &[true]),
    ("core.log", "close", "jet_ring_log_close", true, &[true]),
    ("core.log", "set_sink", "jet_ring_log_set_sink", true, &[true, true]),
    ("core.log", "sample_every", "jet_ring_log_sample_every", true, &[false]),
    ("core.log", "counter", "jet_ring_log_counter", true, &[true, false]),
    ("core.log", "otlp_file", "jet_ring_log_otlp_file", true, &[true]),
    ("core.log", "set_level", "jet_ring_log_set_level", true, &[true]),
    ("core.log", "set_trace_id", "jet_ring_log_set_trace_id", true, &[true]), // E2-M12 D-OBS3: trace context for structured log records.
    ("core.log", "setup", "jet_ring_log_setup", true, &[true]), // D-LOGFMT1=A: explicit log format override.
    ("core.crypto", "sha256_bytes", "jet_ring_crypto_sha256_bytes", true, &[true]),
    ("core.auth", "session_validate", "jet_auth_session_validate", true, &[true, false]),
    ("core.auth", "session_show", "jet_auth_session_show", true, &[true]),
    ("core.auth", "session_user", "jet_auth_session_user", true, &[true]),
    ("core.auth", "session_cookie", "jet_auth_session_cookie", true, &[true]),
    ("core.auth", "session_id", "jet_auth_session_id", true, &[true]),
    ("core.sync", "text_merge", "jet_sync_text_merge", true, &[true, true]),
    ("core.sync", "text_show", "jet_sync_text_show", true, &[true]),
    ("core.sync", "text_metadata", "jet_sync_text_metadata", true, &[true]),
    ("core.sync", "counter_merge", "jet_sync_counter_merge", true, &[true, true]),
    ("core.sync", "counter_value", "jet_sync_counter_value", true, &[true]),
    ("core.sync", "map_new", "jet_sync_map_new", true, &[]),
    ("core.sync", "map_get", "jet_sync_map_get", true, &[true, true]),
    ("core.sync", "map_merge", "jet_sync_map_merge", true, &[true, true]),
    ("core.sync", "map_show", "jet_sync_map_show", true, &[true]),
    ("core.sync", "list_new", "jet_sync_list_new", true, &[]),
    ("core.sync", "list_merge", "jet_sync_list_merge", true, &[true, true]),
    ("core.sync", "list_show", "jet_sync_list_show", true, &[true]),
    ("core.sync", "policy_allows", "jet_db_policy_allows", true, &[true, true, true]),
    ("core.sync", "policy_show", "jet_db_policy_show", true, &[true]),
    ("core.net", "ip_addr", "jet_net_ip_addr", true, &[true]), // D-NETSOCKET1=A: core.net — typed addresses, TCP/UDP/Unix/DNS, TLS handle.
    ("core.net", "ip_to_string", "jet_net_ip_to_string", true, &[true]),
    ("core.net", "ip_is_ipv4", "jet_net_ip_is_ipv4", true, &[true]),
    ("core.net", "socket_addr", "jet_net_socket_addr", true, &[true, false]),
    ("core.net", "socket_addr_parse", "jet_net_socket_addr_parse", true, &[true]),
    ("core.net", "socket_host", "jet_net_socket_host", true, &[true]),
    ("core.net", "socket_port", "jet_net_socket_port", true, &[true]),
    ("core.net", "socket_to_string", "jet_net_socket_to_string", true, &[true]),
    ("core.net", "tcp_listen", "jet_net_tcp_listen", true, &[true]),
    ("core.net", "tcp_listen_addr", "jet_net_tcp_listen_addr", true, &[true]),
    ("core.net", "tcp_accept", "jet_net_tcp_accept", true, &[true]),
    ("core.net", "tcp_connect", "jet_net_tcp_connect", true, &[true]),
    ("core.net", "tcp_connect_addr", "jet_net_tcp_connect_addr", true, &[true]),
    ("core.net", "tcp_connect_timeout", "jet_net_tcp_connect_timeout", true, &[true, false]),
    ("core.net", "tcp_connect_happy", "jet_net_tcp_connect_happy", true, &[true, false, false]),
    ("core.net", "ready_readable", "jet_net_ready_readable", true, &[true]),
    ("core.net", "ready_writable", "jet_net_ready_writable", true, &[true]),
    ("core.net", "error_operation", "jet_net_error_operation", true, &[true]),
    ("core.net", "error_address", "jet_net_error_address", true, &[true]),
    ("core.net", "error_name", "jet_net_error_name", true, &[true]),
    ("core.net", "error_message", "jet_net_error_message", true, &[true]),
    ("core.net", "error_os_code", "jet_net_error_os_code", true, &[true]),
    ("core.net", "tcp_local_addr", "jet_net_tcp_local_addr", true, &[true]),
    ("core.net", "tcp_peer_addr", "jet_net_tcp_peer_addr", true, &[true]),
    ("core.net", "tcp_local_socket_addr", "jet_net_tcp_local_socket_addr", true, &[true]),
    ("core.net", "tcp_peer_socket_addr", "jet_net_tcp_peer_socket_addr", true, &[true]),
    ("core.net", "listener_local_socket_addr", "jet_net_listener_local_socket_addr", true, &[true]),
    ("core.net", "nodelay", "jet_net_nodelay", true, &[true]),
    ("core.net", "set_nodelay", "jet_net_set_nodelay", true, &[true, false]),
    ("core.net", "ttl", "jet_net_ttl", true, &[true]),
    ("core.net", "set_ttl", "jet_net_set_ttl", true, &[true, false]),
    ("core.net", "socket_type", "jet_net_socket_type", true, &[true]),
    ("core.net", "tcp_reply", "jet_net_tcp_reply", true, &[false, true, true]),
    ("core.net", "udp_bind", "jet_net_udp_bind", true, &[true]),
    ("core.net", "udp_bind_addr", "jet_net_udp_bind_addr", true, &[true]),
    ("core.net", "udp_local_addr", "jet_net_udp_local_addr", true, &[true]),
    ("core.net", "udp_set_timeout", "jet_net_udp_set_timeout", true, &[true, false]),
    ("core.net", "udp_send_to", "jet_net_udp_send_to", true, &[true, true, true]),
    ("core.net", "udp_recv_from", "jet_net_udp_recv_from", true, &[true, false]),
    ("core.net", "udp_send_bytes_to", "jet_net_udp_send_bytes_to", true, &[true, true, true]),
    ("core.net", "udp_receive", "jet_net_udp_receive", true, &[true, false]),
    ("core.net", "udp_packet_data", "jet_net_udp_packet_data", true, &[true]),
    ("core.net", "udp_packet_addr", "jet_net_udp_packet_addr", true, &[true]),
    ("core.net", "udp_packet_bytes", "jet_net_udp_packet_bytes", true, &[true]),
    ("core.net", "udp_packet_original_len", "jet_net_udp_packet_original_len", true, &[true]),
    ("core.net", "udp_packet_truncated", "jet_net_udp_packet_truncated", true, &[true]),
    ("core.net", "unix_listen", "jet_net_unix_listen", true, &[true]),
    ("core.net", "unix_accept", "jet_net_unix_accept", true, &[true]),
    ("core.net", "getservbyname", "jet_net_getservbyname", true, &[true]),
    ("core.net", "getservbyport", "jet_net_getservbyport", true, &[false]),
    ("core.net", "dns_srv_target", "jet_net_dns_srv_target", true, &[true]),
    ("core.net", "dns_srv_port", "jet_net_dns_srv_port", true, &[true]),
    ("core.net", "dns_srv_priority", "jet_net_dns_srv_priority", true, &[true]),
    ("core.net", "dns_srv_weight", "jet_net_dns_srv_weight", true, &[true]),
    ("core.http", "router", "jet_http_router_new", true, &[]), // c109 Phase 25: HTTPRouter producer + parse/dispatch (D-ROUTE1=A). `router()` is arg-free; `parse(raw)` borrows the raw string; `dispatch(router, req)` borrows the router and passes the request by value.
    ("core.http", "parse", "jet_http_parse_request", true, &[true]),
    ("core.http", "dispatch", "jet_http_router_dispatch", true, &[true, false]),
    ("core.regex", "flags", "jet_std::jet_regex_flags", true, &[false, false, false]), // D-REGEXENGINE1=A: core.regex — std-only runtime in jet_std, no bridge dep.
    ("core.regex", "escape", "jet_std::jet_regex_escape", true, &[true]),
    ("core.regex", "compile", "jet_std::jet_regex_compile", true, &[true]),
    ("core.regex", "compile_with", "jet_std::jet_regex_compile_with", true, &[true, true]),
    ("core.regex", "literal", "jet_std::jet_regex_literal", true, &[true]),
    ("core.regex", "is_match", "jet_std::jet_regex_is_match", true, &[true, true]),
    ("core.regex", "match", "jet_std::jet_regex_match", true, &[true, true]),
    ("core.regex", "find", "jet_std::jet_regex_find", true, &[true, true]),
    ("core.regex", "find_all", "jet_std::jet_regex_find_all", true, &[true, true]),
    ("core.regex", "matches", "jet_std::jet_regex_matches", true, &[true, true]),
    ("core.regex", "split", "jet_std::jet_regex_split", true, &[true, true]),
    ("core.regex", "split_limit", "jet_std::jet_regex_split_limit", true, &[true, true, false]),
    ("core.regex", "replace", "jet_std::jet_regex_replace", true, &[true, true, true]),
    ("core.regex", "replace_all", "jet_std::jet_regex_replace_all", true, &[true, true, true]),
    ("core.raylib", "window_open", "jet_raylib_window_open", true, &[false, false, true]), // D-RAYLIB1=A / D-FLAGSHIP-RAYLIB1=A: typed graphics bridge.
    ("core.raylib", "window_should_close", "jet_raylib_window_should_close", true, &[true]),
    ("core.raylib", "window_ready", "jet_raylib_window_ready", true, &[true]),
    ("core.raylib", "begin_drawing", "jet_raylib_begin_drawing", true, &[true]),
    ("core.raylib", "clear_background", "jet_raylib_clear_background", true, &[true]),
    ("core.raylib", "draw_text", "jet_raylib_draw_text", true, &[true, false, false, false, true]),
    ("core.raylib", "draw_rectangle", "jet_raylib_draw_rectangle", true, &[false, false, false, false, true]),
    ("core.raylib", "end_drawing", "jet_raylib_end_drawing", true, &[]),
    ("core.raylib", "close_window", "jet_raylib_close_window", true, &[true]),
    ("core.raylib", "key_down", "jet_raylib_key_down", true, &[true]),
    ("core.raylib", "set_target_fps", "jet_raylib_set_target_fps", true, &[false]),
    ("core.raylib", "load_sound", "jet_raylib_load_sound", true, &[true]),
    ("core.raylib", "play_sound", "jet_raylib_play_sound", true, &[true]),
    ("core.raylib", "color", "jet_raylib_color", true, &[false, false, false, false]),
    ("core.db", "params", "jet_std::jet_db_params_from_sql", true, &[true]),
    ("core.db", "row_value", "jet_std::jet_db_row_value", true, &[true, true]),
    ("core.db", "row_int", "jet_std::jet_db_row_int", true, &[true, true]),
    ("core.db", "row_float", "jet_std::jet_db_row_float", true, &[true, true]),
    ("core.db", "row_text", "jet_std::jet_db_row_text", true, &[true, true]),
    ("core.db", "row_bool", "jet_std::jet_db_row_bool", true, &[true, true]),
    ("core.db", "transaction", "jet_db_scope_transaction", false, &[true, true, true]),
    ("core.db", "migrate", "jet_db_scope_migrate", false, &[true, true, true]),
    ("core.random", "pick", "jet_std_random_pick", true, &[true]),
    ("core.random", "weighted_pick", "jet_std_random_weighted_pick", true, &[true, true]),
    ("core.random", "sample", "jet_std_random_sample", true, &[true, false]),
    ("core.term", "read_key", "jet_term_read_key", true, &[]), // D-TERM1 (ratified 2026-06-22): terminal direct-input.
    ("core.perf", "fidelity", "jet_perf_fidelity", false, &[]), // D-FIDELITY-API1=A: runtime-global fidelity signal.
    ("core.perf", "default_fidelity", "jet_perf_default_fidelity", false, &[]),
    ("core.perf", "override_fidelity", "jet_perf_override_fidelity", false, &[false]),
    ("core.perf", "reset_fidelity", "jet_perf_reset_fidelity", false, &[]),
    ("core.ui", "null_backend", "jet_ui_null", true, &[]), // D-RENDERTGT2=A (c133 M1): UI backend seam constructors.
    ("core.ui", "tui_backend", "jet_ui_tui", true, &[]),
    ("core.ui", "gtk_backend", "jet_ui_gtk", true, &[]), // D-UIDEVSHELL1=A (c134 Phase 8): native Linux GTK4 backend constructor.
    ("core.ui", "point", "jet_ui_point", true, &[false, false]),
    ("core.ui", "size", "jet_ui_size", true, &[false, false]),
    ("core.ui", "rect", "jet_ui_rect", true, &[false, false, false, false]),
    ("core.ui", "constraint", "jet_ui_constraint", true, &[false, false, false, false]),
    ("core.ui", "node", "jet_ui_node", true, &[true, false, false]),
    ("core.ui", "text", "jet_ui_text", true, &[true]),
    ("core.ui", "button", "jet_ui_button", true, &[true]),
    ("core.ui", "key_event", "jet_ui_key_event", true, &[true]),
    ("core.ui", "resize_event", "jet_ui_resize_event", true, &[false, false]),
    ("core.ui", "node_role", "jet_ui_node_role", true, &[true, false, false, false]), // D-A11YGATE1=B (c134 Phase 6): accessible-role node + role constants.
    ("core.ui", "node_color", "jet_ui_node_color", true, &[true, false, false, true]), // D-STYLESHAPE1=A wiring: a node carrying an explicit fill color.
    ("core.ui", "aria_role_button", "jet_ui_aria_role_button", true, &[]),
    ("core.ui", "aria_role_text_input", "jet_ui_aria_role_text_input", true, &[]),
    ("core.ui", "aria_role_label", "jet_ui_aria_role_label", true, &[]),
    ("core.ui", "aria_role_container", "jet_ui_aria_role_container", true, &[]),
    ("core.web", "app", "jet_web_app", true, &[]), // D-WEBAPP1=D: application builder + page helper.
    ("app", "live_get", "jet_app_live_get", true, &[true]),
    ("core.web", "live_get", "jet_app_live_get", true, &[true]),
    ("app", "live_show", "jet_app_live_show", true, &[true]),
    ("core.web", "live_show", "jet_app_live_show", true, &[true]),
    ("app", "live_stats", "jet_app_live_stats", true, &[]),
    ("core.web", "live_stats", "jet_app_live_stats", true, &[]),
    ("app", "auth_routes", "jet_app_auth_routes", true, &[true]),
    ("core.web", "auth_routes", "jet_app_auth_routes", true, &[true]),
    ("app", "auth_show", "jet_app_auth_show", true, &[true]),
    ("core.web", "auth_show", "jet_app_auth_show", true, &[true]),
    ("core.web.devserver", "for_app", "jet_devserver_for_app", true, &[true]), // c-devserver (owner-directed 2026-07-01): `devserver.for_app(file)` // constructor — the builder methods dispatch through // `THandleOp::DevServerMethod` above, not here.
    ("core.web.devserver", "app", "jet_devserver_app", true, &[]),
    ("core.sketch.hll", "new", "JetHyperLogLog::new", false, &[]), // D-APPROX1=A: sketch constructors.
    ("core.sketch.tdigest", "new", "JetTDigest::new", false, &[]),
    ("core.sketch.cms", "new", "JetCountMinSketch::new", false, &[]),
    ("core.sketch.reservoir", "new", "JetReservoirSampler::new", false, &[false]),
    ("core.browser", "profile", "jet_browser_profile", false, &[true]), // D-BROWSER-AUTO1=A: native versioned BiDi entry points.
    ("core.browser", "timeout", "jet_browser_timeout", false, &[false]),
    ("core.browser", "locked", "jet_browser_locked", false, &[true]),
    ("core.browser", "connect", "jet_browser_connect", false, &[true]),
    ("core.browser", "connect_profile", "jet_browser_connect_profile", false, &[true, true, false]),
    ("core.http.server", "mux", "jet_http_mux_new", false, &[]),
    ("core.http.server", "response", "jet_http_srv_response", false, &[false, true]),
    ("core.http.server", "tls", "jet_http_srv_tls", false, &[true, true]),
    ("core.http.server", "sse", "jet_http_srv_sse", false, &[true]),
    ("core.http.server", "json", "jet_http_srv_json", false, &[false, true]), // D-HTTP-JSON1=A: one JSON response with its content type set.
    ("core.http.server", "cors", "jet_http_srv_install_cors", false, &[true, true]),
    ("core.http.server", "access_log", "jet_http_srv_access_log", false, &[true, false]),
    ("core.http.server", "request_id", "jet_http_srv_install_request_id", false, &[true]),
    ("core.ws", "connect", "jet_ws_connect", false, &[true]), // D-WS1=B: cleartext WebSocket client/server.
    ("core.ws", "upgrade", "jet_ws_upgrade", false, &[true]),
    ("core.time.date", "new", "JetDate::new", false, &[false, false, false]), // D-TIMEDEPTH1=A: civil-time constructors.
    ("core.time.date", "today", "JetDate::today_utc", false, &[]),
    ("core.time.datetime", "from_timestamp", "JetDateTime::from_timestamp", false, &[false]),
    ("core.time.datetime", "now", "JetDateTime::now", false, &[]),
];

/// #1635: dispatch a plain call straight off `PLAIN_CORE_CALLS` before the
/// bespoke match below has to run. `arg`/`helper` are the same closures
/// `emit_tir_core_call` already built (widen-to-vec included).
fn emit_plain_core_call(
    module: &str,
    method: &str,
    arg: &dyn Fn(usize) -> String,
    helper: &dyn Fn(&str) -> String,
) -> Option<String> {
    let &(_, _, symbol, prefixed, mask) = PLAIN_CORE_CALLS
        .iter()
        .find(|&&(m, me, ..)| m == module && me == method)?;
    let rendered: Vec<String> = mask
        .iter()
        .enumerate()
        .map(|(idx, borrow)| {
            let a = arg(idx);
            if *borrow { format!("&({a})") } else { a }
        })
        .collect();
    let sym = if prefixed { helper(symbol) } else { symbol.to_string() };
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
    if module == "core.compute" {
        if let Some(rendered) = emit_compute_transform_call(method, args, ret_ty, cx) {
            return rendered;
        }
    }
    if let Some(rendered) = emit_plain_core_call(module, method, &arg, &helper) {
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
    // `PLAIN_CORE_CALLS` can't express -- a match guard or duplicate
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
        ("core.mem", "address_of") => format!("(&({}) as *const _ as usize as i64)", arg(0)),
        
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
            format!(
                "{{ let __jet_ch = {}; {} {{ {}: __jet_ch.0, {}: __jet_ch.1 }} }}",
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
        // values use `jet_show()` (universal — every type has it, primitives
        // included, so this never needs its own displayability check) and are
        // populated only when the arg's resolved sema type is a known user
        // struct (`cx.struct_fields`); every other shape (primitives, enums,
        // tuples, lists) gets an empty list, never a guess.
        ("core.reflect", "of") => {
            let arg_ty = args.first().map(|a| &a.ty);
            let type_name = arg_ty.map(|t| t.name()).unwrap_or_default();
            let fields_code = match arg_ty {
                Some(Type::Named(struct_name)) => match cx.struct_fields.get(struct_name) {
                    Some(fields) if !fields.is_empty() => {
                        let items: Vec<String> = fields
                            .iter()
                            .map(|(fname, _)| {
                                format!(
                                    "{root}JetReflectField {{ name: \"{fname}\".to_string(), value: (__reflect_v.{mangled}).jet_show() }}",
                                    root = cx.root_prefix,
                                    fname = fname,
                                    mangled = mangle(fname)
                                )
                            })
                            .collect();
                        format!("vec![{}]", items.join(", "))
                    }
                    _ => "Vec::new()".to_string(),
                },
                _ => "Vec::new()".to_string(),
            };
            format!(
                "{{ let __reflect_v = &({arg0}); {root}JetReflectValue {{ type_name: \"{type_name}\".to_string(), display: __reflect_v.jet_display(), fields: {fields_code} }} }}",
                arg0 = arg(0),
                root = cx.root_prefix,
                type_name = type_name,
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
            format!(
                "{{ let __jet_sc = ({}).sin_cos(); {} {{ {}: __jet_sc.0, {}: __jet_sc.1 }} }}",
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
            format!(
                "{{ let __jet_x = ({0}); {1} {{ {2}: __jet_x.fract(), {3}: __jet_x.trunc() }} }}",
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
            format!(
                "{{ let __jet_x = ({0}); let __jet_e = {1}jet_std_math_ilogb(__jet_x).unwrap_or(0); let __jet_f = if __jet_x == 0.0 || !__jet_x.is_finite() {{ __jet_x }} else {{ {1}jet_std_math_ldexp(__jet_x, -__jet_e) }}; {2} {{ {3}: __jet_f, {4}: __jet_e }} }}",
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
                format!(
                    "let __jet_a = ({0}); let __jet_b = ({1}); let __jet_q = __jet_a.div_euclid(__jet_b); let __jet_r = __jet_a.rem_euclid(__jet_b);",
                    arg(0),
                    arg(1)
                )
            } else {
                // Truncating division + remainder (Rust /, %).
                format!(
                    "let __jet_a = ({0}); let __jet_b = ({1}); let __jet_q = __jet_a / __jet_b; let __jet_r = __jet_a % __jet_b;",
                    arg(0),
                    arg(1)
                )
            };
            format!(
                "{{ {op} {struct_name} {{ {}: __jet_q, {}: __jet_r }} }}",
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
            let replay = if args.len() >= 2
                && matches!(args[1].ty, Type::Named(ref n) if n == "GameReplay")
            {
                format!("Some(&({}))", arg(1))
            } else {
                "None".to_string()
            };
            let backend_idx = if args.len() >= 2
                && matches!(args[1].ty, Type::Named(ref n) if n == "GameBackend")
            {
                Some(1)
            } else if args.len() >= 3 {
                Some(2)
            } else {
                None
            };
            let backend = backend_idx
                .map(|i| format!("Some(&({}))", arg(i)))
                .unwrap_or_else(|| "None".to_string());
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
            format!(
                "{helper}(&({arg})).map(|(__jet_raw, __jet_prefix, __jet_local, __jet_uri)| {struct_name} {{ {raw}: __jet_raw, {prefix}: __jet_prefix, {local}: __jet_local, {uri}: __jet_uri }})",
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
        // The 1-arg default stays here, not in `PLAIN_CORE_CALLS`: the table is
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
        // D-AUTH-TOKENPOLICY1=A: fixed HS256 with required audience. Optional
        // controls are positional named arguments, so omitted suffixes lower to
        // their safe defaults here.
        ("core.auth", "verify_jwt") => {
            let issuer = if args.len() >= 4 { format!("Some(&({}))", arg(3)) } else { "None".to_string() };
            let skew = if args.len() >= 5 { format!("{}jet_duration_ms_value(&({}))", cx.root_prefix, arg(4)) } else { "0".to_string() };
            format!(
                "{}(&({}), &({}), &({}), {}, {})",
                helper("jet_auth_verify_jwt_impl"),
                arg(0),
                arg(1),
                arg(2),
                issuer,
                skew,
            )
        }
        ("core.auth", "verify_paseto") => {
            let issuer = if args.len() >= 4 { format!("Some(&({}))", arg(3)) } else { "None".to_string() };
            let skew = if args.len() >= 5 { format!("{}jet_duration_ms_value(&({}))", cx.root_prefix, arg(4)) } else { "0".to_string() };
            let footer = if args.len() >= 6 { arg(5) } else { "Vec::<u8>::new()".to_string() };
            let implicit = if args.len() >= 7 { arg(6) } else { "Vec::<u8>::new()".to_string() };
            format!(
                "{}(&({}), &({}), &({}), {}, {}, &({}), &({}), {})",
                helper("jet_auth_verify_paseto_impl"), arg(0), arg(1), arg(2), issuer, skew,
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
        ("core.vault", "current" | "versions" | "prepare_generate" | "prepare_rotate") =>
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
        ("core.io", "print" | "println") => format!("println!(\"{{}}\", ({}).jet_show())", arg(0)),
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
        ("core.http.server", "bind") if args.len() == 3 => {
            let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
            format!(
                "jet_http_server_bind_tls(&({}), {}, {}, |cert, key| {ffi}::jet_http_server_tls_validate_impl(cert, key), |cert, key, stream, on_request, on_h2, should_stop| {ffi}::jet_http_server_tls_session_impl(cert, key, stream, on_request, on_h2, should_stop)).map_err(|e| JetHTTPError::IO {{ operation: e }})",
                arg(0),
                arg(1),
                arg(2)
            )
        }
        ("core.http.server", "bind") => format!("jet_http_server_bind(&({}), {}).map_err(|_| JetHTTPError::IO {{ operation: \"bind\".to_string() }})", arg(0), arg(1)),
        
        ("core.http.server", "serve") if args.len() == 3 => {
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
        
        
        ("core.time.date", "parse") => format!("JetDate::parse(&({})).map_err(|e| e)", arg(0)),
        
        _ => "/* unknown std call */".to_string(),
    }
}

/// D-ONCE-LAW1=A: the guard for the `AotCoreCalls` truth row.
///
/// `emit_plain_core_call` reads `PLAIN_CORE_CALLS` before the bespoke match
/// runs, so a bespoke arm keyed on a pair the table already holds is a second
/// copy of the same call that nothing can reach. It compiles, it never runs,
/// and it drifts. This counts the definition sites for every pair and refuses
/// a second one.
#[cfg(test)]
mod tests {
    /// The `(module, method)` pairs the table defines.
    fn table_keys(source: &str) -> Vec<(String, String)> {
        let start = source
            .find("const PLAIN_CORE_CALLS")
            .expect("the table is the one home for a plain core call");
        let end = source[start..]
            .find("\n];")
            .expect("the table ends")
            + start;
        pairs(&source[start..end])
    }

    /// The `(module, method)` pairs the bespoke match arms below the table
    /// define. Both readers use the same shape: `("a", "b"` at a line start.
    fn pairs(text: &str) -> Vec<(String, String)> {
        let mut found = Vec::new();
        for line in text.lines() {
            let line = line.trim_start();
            let Some(rest) = line.strip_prefix("(\"") else {
                continue;
            };
            let Some((module, rest)) = rest.split_once("\", \"") else {
                continue;
            };
            let Some((method, _)) = rest.split_once('"') else {
                continue;
            };
            found.push((module.to_string(), method.to_string()));
        }
        found
    }

    #[test]
    fn no_bespoke_arm_repeats_a_table_row() {
        let source = include_str!("core_calls.rs");
        let table_end = source
            .find("const PLAIN_CORE_CALLS")
            .and_then(|start| source[start..].find("\n];").map(|end| start + end))
            .expect("the table is the one home for a plain core call");
        let keys = table_keys(source);
        assert!(keys.len() > 500, "the table lost its rows: {}", keys.len());

        let shadowed: Vec<String> = pairs(&source[table_end..])
            .into_iter()
            .filter(|pair| keys.contains(pair))
            .map(|(module, method)| format!("(\"{module}\", \"{method}\")"))
            .collect();
        assert!(
            shadowed.is_empty(),
            "these calls are defined twice — once in PLAIN_CORE_CALLS and again \
             in a bespoke arm the table already answers, so the arm is dead and \
             free to drift ({}):\n{}",
            shadowed.len(),
            shadowed.join("\n")
        );
    }

    /// The table itself says each call once.
    #[test]
    fn the_table_defines_each_call_once() {
        let mut keys = table_keys(include_str!("core_calls.rs"));
        let before = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(before, keys.len(), "PLAIN_CORE_CALLS holds a repeated key");
    }
}
