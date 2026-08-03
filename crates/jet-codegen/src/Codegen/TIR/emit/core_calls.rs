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
use crate::Codegen::TIR::TExpr;

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
    let normalized_module =
        crate::Syntax::normalize_core_module(module).unwrap_or_else(|| module.to_string());
    let module = normalized_module.as_str();
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
    match (module, method) {
        ("jet.unit", "magnitude") => format!("({}).to_string()", arg(0)),
        // c109 Phase 18 (S58, E2-M13): low-level pointer ops, byte-for-byte
        // `emit_core_call`. `address_of` is an inert address cast (no `unsafe`);
        // `volatile_read`/`volatile_write` access through a `Ptr<T>` — the volatile ops are
        // valid because the call only reaches codegen inside an `#Unsafe` region/fn (sema
        // E3101), already lowered to a Rust `unsafe` context.
        ("core.mem", "address_of") => format!("(&({}) as *const _ as usize as i64)", arg(0)),
        ("core.mem", "volatile_read") => format!("std::ptr::read_volatile({})", arg(0)),
        ("core.mem", "volatile_write") => {
            format!("std::ptr::write_volatile({}, {})", arg(0), arg(1))
        }
        // D-TUPLE-DESTRUCT1: the `tasks.channel<T>()` producer — returns the
        // `(Sender<T>, Receiver<T>)` pair as the same `JetTup_<hash>` named-tuple
        // struct every other `Type::Tuple` value uses (`enumerate`/`zip`/`partition`'s
        // convention — `Tuples::collect_tuple_shapes` already walks this call's
        // `resolved_ret` and declares the struct). `T` and the struct shape both come
        // from the call node's own resolved `ret_ty`, not a binding annotation
        // (there's no combined "Channel" value left to annotate).
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
        ("core.tasks", "interval") => format!("{}jet_std::interval({})", cx.root_prefix, arg(0)),
        // D-REACT1=B: `reactive.signal(initial)` producer → a `JetSignal<T>`.
        ("jet.reactive", "signal") => {
            format!("{}jet_std::JetSignal::new({})", cx.root_prefix, arg(0))
        }
        // D-EVENT1=D: first-party typed Event/Hook constructors.
        ("core.event", "scope") => format!("{}jet_std::JetEventScope::new()", cx.root_prefix),
        ("core.event", "policy_sync") => {
            format!("{}jet_std::JetEventPolicy::sync()", cx.root_prefix)
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
        ("core.science.measurement", "from") => {
            format!(
                "{}jet_std::JetMeasurement::new({}, {})",
                cx.root_prefix,
                arg(0),
                arg(1)
            )
        }
        // D-CORE-NUMERIC1=A: `core.math.decimal(s)` → exact parse.
        ("core.math", "decimal") => {
            format!("{}jet_decimal_from_str(&({}))", cx.root_prefix, arg(0))
        }
        // D-PENDING1=B: Loadable<T,E> constructors.
        // idle/loading/loaded/failed need concrete type params for E: Clone bound satisfaction.
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
        ("core.files", "read") => format!("{}(&({}))", helper("jet_std_fs_read"), arg(0)),
        ("core.files", "read_bytes") => {
            format!("{}(&({}))", helper("jet_std_fs_read_bytes"), arg(0))
        }
        ("core.files", "write") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_write"),
            arg(0),
            arg(1)
        ),
        ("core.files", "append_all") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_append"),
            arg(0),
            arg(1)
        ),
        ("core.files", "exists") => format!("{}(&({}))", helper("jet_std_fs_exists"), arg(0)),
        ("core.files", "remove") => format!("{}(&({}))", helper("jet_std_fs_remove"), arg(0)),
        ("core.files", "remove_dir") => {
            format!("{}(&({}))", helper("jet_std_fs_remove_dir"), arg(0))
        }
        ("core.files", "remove_all") => {
            format!("{}(&({}))", helper("jet_std_fs_remove_all"), arg(0))
        }
        ("core.files", "list_dir") => format!("{}(&({}))", helper("jet_std_fs_list_dir"), arg(0)),
        ("core.files", "create_dir") => {
            format!("{}(&({}))", helper("jet_std_fs_create_dir"), arg(0))
        }
        ("core.files", "create_dir_all") => {
            format!("{}(&({}))", helper("jet_std_fs_create_dir_all"), arg(0))
        }
        ("core.files", "is_dir") => format!("{}(&({}))", helper("jet_std_fs_is_dir"), arg(0)),
        ("core.files", "copy") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_copy"),
            arg(0),
            arg(1)
        ),
        ("core.files", "copy_dir") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_copy_dir"),
            arg(0),
            arg(1)
        ),
        ("core.files", "rename") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_rename"),
            arg(0),
            arg(1)
        ),
        ("core.files", "symlink") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_symlink"),
            arg(0),
            arg(1)
        ),
        ("core.files", "read_link") => {
            format!("{}(&({}))", helper("jet_std_fs_read_link"), arg(0))
        }
        ("core.files", "hard_link") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_hard_link"),
            arg(0),
            arg(1)
        ),
        ("core.files", "stat") => format!("{}(&({}))", helper("jet_std_fs_stat"), arg(0)),
        ("core.files", "canonicalize") => {
            format!("{}(&({}))", helper("jet_std_fs_canonicalize"), arg(0))
        }
        ("core.files", "absolute") => {
            format!("{}(&({}))", helper("jet_std_fs_absolute"), arg(0))
        }
        ("core.files", "walk") => format!("{}(&({}))", helper("jet_std_fs_walk"), arg(0)),
        ("core.files", "glob") => format!("{}(&({}))", helper("jet_std_fs_glob"), arg(0)),
        ("core.files", "read_at") => format!(
            "{}(&({}), {}, {})",
            helper("jet_std_fs_read_at"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.files", "write_at") => format!(
            "{}(&({}), {}, &({}))",
            helper("jet_std_fs_write_at"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.files", "fsync") => format!("{}(&({}))", helper("jet_std_fs_fsync"), arg(0)),
        ("core.files", "write_atomic") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_fs_write_atomic"),
            arg(0),
            arg(1)
        ),
        ("core.files", "temp_dir") => {
            format!("{}(&({}))", helper("jet_std_fs_temp_dir"), arg(0))
        }
        ("core.files", "temp_file") => {
            format!("{}(&({}))", helper("jet_std_fs_temp_file"), arg(0))
        }
        ("core.files", "lock") => format!("{}(&({}))", helper("jet_std_fs_lock"), arg(0)),
        ("core.watcher", "files") => format!("{}(&({}))", helper("jet_watcher_files"), arg(0)),
        ("core.watcher", "process_pid") => {
            format!("{}({})", helper("jet_watcher_process_pid"), arg(0))
        }
        ("core.watcher", "port") => {
            format!("{}(&({}), {})", helper("jet_watcher_port"), arg(0), arg(1))
        }
        ("core.watcher", "set") => format!("{}()", helper("jet_watcher_set")),
        ("core.io", "args") => format!("{}()", helper("jet_std_io_args")),
        // D-ARGS1: `args.spec()` → empty builder.
        ("core.args", "spec") => format!("{}()", helper("jet_args_spec")),
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
        // c109 Phase 29: qualified `io.input(prompt)`, byte-for-byte `emit_core_call`
        // (Expression.rs ~L1294): no arg → `jet_std_io_input(None)`; a prompt arg →
        // `jet_std_io_input(Some(&(prompt)))`. Same emitted helper as the ambient bare
        // `input(...)` (Phase 25), the only difference being the source node shape.
        ("core.io", "input") => {
            if args.is_empty() {
                format!("{}(None)", helper("jet_std_io_input"))
            } else {
                format!("{}(Some(&({})))", helper("jet_std_io_input"), arg(0))
            }
        }
        ("core.io", "confirm") => {
            format!("{}(&({}))", helper("jet_std_io_confirm"), arg(0))
        }
        ("core.io", "choose") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_io_choose"),
            arg(0),
            arg(1)
        ),
        ("core.io", "input_secret") => {
            format!("{}(&({}))", helper("jet_std_io_input_secret"), arg(0))
        }
        ("core.io", "read_all_input") => format!("{}()", helper("jet_std_io_read_all_input")),
        // D-STDIN1=A: io.stdin() → JetStdinReader handle.
        ("core.io", "stdin") => format!("{}()", helper("jet_std_io_stdin")),
        ("core.io", "stdout") => format!("{}()", helper("jet_std_io_stdout")),
        ("core.io", "stderr") => format!("{}()", helper("jet_std_io_stderr")),
        ("core.io", "terminal_width") => format!("{}()", helper("jet_std_io_terminal_width")),
        ("core.io", "terminal_height") => format!("{}()", helper("jet_std_io_terminal_height")),
        ("core.io", "style") => {
            format!(
                "{}(&({}), &({}))",
                helper("jet_std_io_style"),
                arg(0),
                arg(1)
            )
        }
        ("core.io", "style_force") => {
            format!(
                "{}(&({}), &({}))",
                helper("jet_std_io_style_force"),
                arg(0),
                arg(1)
            )
        }
        ("core.io", "progress") => {
            format!("{}(&({}))", helper("jet_std_io_progress"), arg(0))
        }
        ("core.env", "get") => format!("{}(&({}))", helper("jet_std_env_get"), arg(0)),
        ("core.env", "set") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_env_set"),
            arg(0),
            arg(1)
        ),
        ("core.env", "unset") => format!("{}(&({}))", helper("jet_std_env_unset"), arg(0)),
        ("core.env", "vars") => format!("{}()", helper("jet_std_env_vars")),
        ("core.env", "current_dir") => format!("{}()", helper("jet_std_env_current_dir")),
        ("core.env", "home_dir") => format!("{}()", helper("jet_std_env_home_dir")),
        ("core.os", "name") => format!("{}()", helper("jet_std_os_name")),
        ("core.os", "family") => format!("{}()", helper("jet_std_os_family")),
        ("core.os", "arch") => format!("{}()", helper("jet_std_os_arch")),
        ("core.os", "cpu_count") => format!("{}()", helper("jet_std_os_cpu_count")),
        ("core.os", "temp_dir") => format!("{}()", helper("jet_std_os_temp_dir")),
        ("core.os", "executable") => format!("{}()", helper("jet_std_os_executable")),
        ("core.os", "pid") => format!("{}()", helper("jet_std_os_pid")),
        ("core.os", "hostname") => format!("{}()", helper("jet_std_os_hostname")),
        ("core.os", "username") => format!("{}()", helper("jet_std_os_username")),
        ("core.os", "set_current_dir") => {
            format!("{}(&({}))", helper("jet_std_os_set_current_dir"), arg(0))
        }
        ("core.os", "on_interrupt") => {
            format!("{}({})", helper("jet_std_os_on_interrupt"), arg(0))
        }
        ("core.process", "exit") => format!("{}({})", helper("jet_std_process_exit"), arg(0)),
        ("core.process", "run") => format!("{}(&({}))", helper("jet_std_process_run"), arg(0)),
        ("core.process", "cmd") => format!("{}(&({}))", helper("jet_std_process_cmd"), arg(0)),
        ("core.process", "pipeline") => {
            format!("{}(&({}))", helper("jet_std_process_pipeline"), arg(0))
        }
        ("core.testing", "snap") => {
            format!("{}(&({}), &({}))", helper("jet_testing_snap"), arg(0), arg(1))
        }
        ("core.testing", "golden") => {
            format!("{}(&({}), &({}))", helper("jet_testing_golden"), arg(0), arg(1))
        }
        ("core.testing", "fixture") => format!("{}(&({}))", helper("jet_testing_fixture"), arg(0)),
        ("core.testing", "temp_dir") => format!("{}(&({}))", helper("jet_testing_temp_dir"), arg(0)),
        ("core.testing", "corpus") => format!("{}(&({}))", helper("jet_testing_corpus"), arg(0)),
        ("core.testing", "fake_clock") => format!("{}({})", helper("jet_std_clock_new"), arg(0)),
        ("core.testing", "fake_rng") => format!("{}({})", helper("jet_std_rng_new"), arg(0)),
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
        ("core.math", "round") => format!("{}({})", helper("jet_std_math_round"), arg(0)),
        (
            "core.math",
            "sin" | "cos" | "tan" | "asin" | "acos" | "atan" | "sinh" | "cosh" | "tanh"
            | "exp" | "ln" | "log2" | "log10" | "trunc" | "fract",
        ) => format!("({}).{}()", arg(0), method),
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
        ("core.math", "checked_pow") => {
            format!("{}({}, {})", helper("jet_std_math_checked_pow"), arg(0), arg(1))
        }
        ("core.math", "saturating_add") => format!("({}).saturating_add({})", arg(0), arg(1)),
        ("core.math", "saturating_sub") => format!("({}).saturating_sub({})", arg(0), arg(1)),
        ("core.math", "saturating_mul") => format!("({}).saturating_mul({})", arg(0), arg(1)),
        ("core.math", "wrapping_add") => format!("({}).wrapping_add({})", arg(0), arg(1)),
        ("core.math", "wrapping_sub") => format!("({}).wrapping_sub({})", arg(0), arg(1)),
        ("core.math", "wrapping_mul") => format!("({}).wrapping_mul({})", arg(0), arg(1)),
        ("core.math", "int_pow") => format!("{}({}, {})", helper("jet_std_math_int_pow"), arg(0), arg(1)),
        ("core.math", "gcd") => format!("{}({}, {})", helper("jet_std_math_gcd"), arg(0), arg(1)),
        ("core.math", "lcm") => format!("{}({}, {})", helper("jet_std_math_lcm"), arg(0), arg(1)),
        ("core.random", "int") => {
            format!("{}({}, {})", helper("jet_std_random_int"), arg(0), arg(1))
        }
        ("core.random", "float") => format!("{}()", helper("jet_std_random_float")),
        ("core.random", "float_range") => {
            format!("{}({}, {})", helper("jet_std_random_float_range"), arg(0), arg(1))
        }
        ("core.random", "bool") => format!("{}({})", helper("jet_std_random_bool"), arg(0)),
        ("core.random", "normal") => {
            format!("{}({}, {})", helper("jet_std_random_normal"), arg(0), arg(1))
        }
        ("core.random", "exponential") => {
            format!("{}({})", helper("jet_std_random_exponential"), arg(0))
        }
        ("core.random", "seed") => format!("{}({})", helper("jet_std_random_seed"), arg(0)),
        // D-RANDSPLIT1=A: PRNG bytes — fast, NOT crypto-safe.
        ("core.random", "bytes") => format!("{}({})", helper("jet_std_random_bytes"), arg(0)),
        // D-CRYPTO-RNG1=A: shared fail-closed OS CSPRNG provider.
        ("core.crypto.random", "bytes") => {
            format!("{}({})", helper("jet_std_crypto_random_bytes"), arg(0))
        }
        // D-DET1: deterministic injected RNG capability constructor.
        ("core.random", "rng") => format!("{}({})", helper("jet_std_rng_new"), arg(0)),
        ("core.random", "split") => format!("{}({})", helper("jet_std_random_split"), arg(0)),
        ("core.time", "now") => format!("{}()", helper("jet_std_time_now")),
        ("core.time", "sleep") => format!("{}({})", helper("jet_std_time_sleep"), arg(0)),
        ("core.time", "start") => format!("{}()", helper("jet_std_time_start")),
        ("core.time", "milliseconds") => format!(
            "{}({}, jet_std::DurationUnit::Milliseconds)",
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
        ("core.time", "instant") => format!("{}()", helper("jet_time_instant_now")),
        ("core.time", "now_utc") => format!("{}()", helper("jet_time_now_utc")),
        ("core.time", "from_unix_ms") => format!("JetDateTime::from_unix_ms({})", arg(0)),
        ("core.time", "today") => format!("{}()", helper("jet_time_today")),
        ("core.time", "parse_rfc3339") => {
            format!("{}(&({}))", helper("jet_time_parse_rfc3339"), arg(0))
        }
        ("core.time", "local_time") => {
            format!("JetLocalTime::new({}, {}, {})", arg(0), arg(1), arg(2))
        }
        ("core.time", "parse_time") => {
            format!("JetLocalTime::parse(&({})).map_err(|e| e)", arg(0))
        }
        ("core.time", "period") => format!(
            "{}({}, {}, {})",
            helper("jet_time_period"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.time", "period_days") => format!("{}({})", helper("jet_time_period_days"), arg(0)),
        ("core.time", "period_months") => {
            format!("{}({})", helper("jet_time_period_months"), arg(0))
        }
        ("core.time", "period_years") => {
            format!("{}({})", helper("jet_time_period_years"), arg(0))
        }
        ("core.time", "zone") => format!("{}(&({}))", helper("jet_time_zone_named"), arg(0)),
        ("core.time", "utc") => format!("{}()", helper("jet_time_zone_utc")),
        ("core.time", "zoned") => {
            format!("{}(&({}), &({}))", helper("jet_time_zoned"), arg(0), arg(1))
        }
        ("core.time", "zoned_local") => format!(
            "{}(&({}), &({}), &({}))",
            helper("jet_time_zoned_local"),
            arg(0),
            arg(1),
            arg(2)
        ),
        // D-DET1: deterministic injected Clock capability constructor.
        ("core.time", "clock") => format!("{}({})", helper("jet_std_clock_new"), arg(0)),
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
        ("core.encoding.json", "parse") => {
            format!("{}(&({}))", helper("jet_std_json_parse"), arg(0))
        }
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
            format!("{}(&({}))", helper("jet_std_json_render_canonical"), arg(0))
        }
        ("core.encoding.json", "events") => {
            format!("{}(&({}))", helper("jet_std_json_events"), arg(0))
        }
        ("core.encoding.jsonl", "parse") => {
            format!("{}(&({}))", helper("jet_std_jsonl_parse"), arg(0))
        }
        ("core.encoding.jsonl", "to_string") => {
            format!("{}(&({}))", helper("jet_std_jsonl_render"), arg(0))
        }
        ("core.encoding.csv", "parse") => {
            format!("{}(&({}))", helper("jet_ring_csv_parse"), arg(0))
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
        ("core.data", "count") => format!("{}(&({}))", helper("jet_data_count"), arg(0)),
        // D-COMPUTE1=D (#443): Tensor CPU oracle — one Prelude symbol per call.
        ("core.compute", "zeros") => format!("{}(&({}))", helper("jet_compute_zeros"), arg(0)),
        ("core.compute", "ones") => format!("{}(&({}))", helper("jet_compute_ones"), arg(0)),
        ("core.compute", "full") => {
            format!("{}(&({}), {})", helper("jet_compute_full"), arg(0), arg(1))
        }
        ("core.compute", "from_list") => {
            format!("{}(&({}))", helper("jet_compute_from_list"), arg(0))
        }
        ("core.compute", "matrix") => format!(
            "{}({}, {}, {})",
            helper("jet_compute_matrix"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.compute", "vec") => {
            format!("{}({}, {})", helper("jet_compute_vec"), arg(0), arg(1))
        }
        ("core.compute", "add") => {
            format!("{}(&({}), &({}))", helper("jet_compute_add"), arg(0), arg(1))
        }
        ("core.compute", "mul") => {
            format!("{}(&({}), &({}))", helper("jet_compute_mul"), arg(0), arg(1))
        }
        ("core.compute", "matmul") => {
            format!("{}(&({}), &({}))", helper("jet_compute_matmul"), arg(0), arg(1))
        }
        ("core.compute", "reshape") => {
            format!("{}(&({}), &({}))", helper("jet_compute_reshape"), arg(0), arg(1))
        }
        ("core.compute", "get") => {
            format!("{}(&({}), &({}))", helper("jet_compute_get"), arg(0), arg(1))
        }
        ("core.compute", "set") => format!(
            "{}(&mut ({}), &({}), {})",
            helper("jet_compute_set"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.compute", "shape") => {
            format!("{}(&({}))", helper("jet_compute_tensor_shape"), arg(0))
        }
        ("core.compute", "rank") => {
            format!("{}(&({}))", helper("jet_compute_tensor_rank"), arg(0))
        }
        ("core.compute", "numel") => {
            format!("{}(&({}))", helper("jet_compute_tensor_numel"), arg(0))
        }
        ("core.compute", "to_list") => {
            format!("{}(&({}))", helper("jet_compute_tensor_to_list"), arg(0))
        }
        ("core.compute", "device") => {
            format!("{}(&({}))", helper("jet_compute_tensor_device"), arg(0))
        }
        ("core.compute", "placement") => {
            format!("{}(&({}))", helper("jet_compute_tensor_placement"), arg(0))
        }
        ("core.compute", "device_cpu") => format!("{}()", helper("jet_compute_device_cpu")),
        ("core.compute", "device_auto") => format!("{}()", helper("jet_compute_device_auto")),
        ("core.compute", "on_device") => format!(
            "{}(&({}), {})",
            helper("jet_compute_on_device"),
            arg(0),
            arg(1)
        ),
        ("core.compute", "broadcast_to") => format!(
            "{}(&({}), &({}))",
            helper("jet_compute_broadcast_to"),
            arg(0),
            arg(1)
        ),
        ("core.compute", "transpose") => {
            format!("{}(&({}))", helper("jet_compute_transpose"), arg(0))
        }
        ("core.compute", "sum_axis") => format!(
            "{}(&({}), {})",
            helper("jet_compute_sum_axis"),
            arg(0),
            arg(1)
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
        ("core.compute", "eye") => format!("{}({})", helper("jet_compute_eye"), arg(0)),
        ("core.compute", "det") => format!("{}(&({}))", helper("jet_compute_det"), arg(0)),
        ("core.compute", "inv") => format!("{}(&({}))", helper("jet_compute_inv"), arg(0)),
        ("core.compute", "fft") => format!("{}(&({}))", helper("jet_compute_fft"), arg(0)),
        ("core.compute", "solve") => format!(
            "{}(&({}), &({}))",
            helper("jet_compute_solve"),
            arg(0),
            arg(1)
        ),
        ("core.compute", "stream_new") => format!("{}()", helper("jet_compute_stream_new")),
        ("core.compute", "stream_sync") => {
            format!("{}(&({}))", helper("jet_compute_stream_sync"), arg(0))
        }
        ("core.compute", "stream_show") => {
            format!("{}(&({}))", helper("jet_compute_stream_show"), arg(0))
        }
        ("core.compute", "transfer") => format!(
            "{}(&({}), {})",
            helper("jet_compute_transfer"),
            arg(0),
            arg(1)
        ),
        ("core.compute", "transfer_show") => {
            format!("{}(&({}))", helper("jet_compute_transfer_show"), arg(0))
        }
        ("core.compute", "kernel_bounds_ok") => format!(
            "{}(&({}), &({}))",
            helper("jet_compute_kernel_bounds_ok"),
            arg(0),
            arg(1)
        ),
        ("core.compute", "raw_kernel_contract") => format!(
            "{}(({}).clone(), {})",
            helper("jet_compute_raw_kernel_contract"),
            arg(0),
            arg(1)
        ),
        ("core.compute", "raw_kernel_contract_show") => format!(
            "{}(&({}))",
            helper("jet_compute_raw_kernel_contract_show"),
            arg(0)
        ),
        ("core.compute", "jvp_add" | "jvp_mul" | "jvp_matmul") => format!(
            "{}(&({}), &({}), &({}), &({}))",
            helper(&format!("jet_compute_{method}")),
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        ("core.compute", "vjp_add" | "vjp_mul" | "vjp_matmul") => format!(
            "{}(&({}), &({}), &({}))",
            helper(&format!("jet_compute_{method}_value")),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.compute", "value_and_grad_mul") => format!(
            "{}(&({}), &({}))",
            helper("jet_compute_value_and_grad_mul"),
            arg(0),
            arg(1)
        ),
        ("core.compute", "grad_value") => {
            format!("{}(&({}))", helper("jet_compute_grad_value"), arg(0))
        }
        ("core.compute", "grad_a") => format!("{}(&({}))", helper("jet_compute_grad_a"), arg(0)),
        ("core.compute", "grad_b") => format!("{}(&({}))", helper("jet_compute_grad_b"), arg(0)),
        ("core.compute", "grad_show") => {
            format!("{}(&({}))", helper("jet_compute_grad_show"), arg(0))
        }
        ("core.compute", "mse_loss") => format!(
            "{}(&({}), &({}))",
            helper("jet_compute_mse_loss"),
            arg(0),
            arg(1)
        ),
        ("core.compute", "sgd_step") => format!(
            "{}(&({}), &({}), {})",
            helper("jet_compute_sgd_step"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.compute", "serialize") => {
            format!("{}(&({}))", helper("jet_compute_serialize"), arg(0))
        }
        ("core.compute", "deserialize") => {
            format!("{}(&({}))", helper("jet_compute_deserialize"), arg(0))
        }
        ("core.compute", "to_sparse") => {
            format!("{}(&({}))", helper("jet_compute_to_sparse"), arg(0))
        }
        ("core.compute", "sparse_nnz") => {
            format!("{}(&({}))", helper("jet_compute_sparse_nnz"), arg(0))
        }
        ("core.compute", "sparse_mv") => format!(
            "{}(&({}), &({}))",
            helper("jet_compute_sparse_mv"),
            arg(0),
            arg(1)
        ),
        ("core.compute", "sparse_show") => {
            format!("{}(&({}))", helper("jet_compute_sparse_show"), arg(0))
        }
        ("core.compute", "matmul_f32_tile") => format!(
            "{}(&({}), &({}))",
            helper("jet_compute_matmul_f32_tile"),
            arg(0),
            arg(1)
        ),
        ("core.compute", "profile_f32_strict") => {
            format!("{}()", helper("jet_compute_profile_f32_strict"))
        }
        ("core.compute", "profile_show") => format!("{}()", helper("jet_compute_profile_show")),
        ("core.services", "runtime") => format!(
            "{}(({}).clone(), ({}).ms)",
            helper("jet_services_runtime"),
            arg(0),
            arg(1)
        ),
        ("core.services", "state_authority") => format!(
            "{}(({}).clone(), ({}).clone(), {})",
            helper("jet_services_state_authority"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.services", "tree") => format!("{}(({}).clone())", helper("jet_services_tree"), arg(0)),
        ("core.services", "restart_one_for_one") => {
            format!("{}()", helper("jet_services_restart_one_for_one"))
        }
        ("core.services", "restart_one_for_all") => {
            format!("{}()", helper("jet_services_restart_one_for_all"))
        }
        ("core.services", "restart_rest_for_one") => {
            format!("{}()", helper("jet_services_restart_rest_for_one"))
        }
        ("core.services", "delivery_at_most_once") => {
            format!("{}()", helper("jet_services_delivery_at_most_once"))
        }
        ("core.services", "delivery_durable") => {
            format!("{}()", helper("jet_services_delivery_durable"))
        }
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
        ("core.services", "mailbox_depth") => format!(
            "{}(&({}), &({}))",
            helper("jet_services_mailbox_depth"),
            arg(0),
            arg(1)
        ),
        ("core.services", "fail_worker") => format!(
            "{}(&mut ({}), &({}))",
            helper("jet_services_fail_worker"),
            arg(0),
            arg(1)
        ),
        ("core.services", "restarts") => format!(
            "{}(&({}), &({}))",
            helper("jet_services_restarts"),
            arg(0),
            arg(1)
        ),
        ("core.services", "dead_letter_count") => {
            format!("{}(&({}))", helper("jet_services_dead_letter_count"), arg(0))
        }
        ("core.services", "drain_dead_letters") => format!(
            "{}(&mut ({}))",
            helper("jet_services_drain_dead_letters"),
            arg(0)
        ),
        ("core.services", "set_state_empty") => {
            format!("{}(&mut ({}))", helper("jet_services_set_state_empty"), arg(0))
        }
        ("core.services", "set_state_snapshot") => format!(
            "{}(&mut ({}), ({}).clone())",
            helper("jet_services_set_state_snapshot"),
            arg(0),
            arg(1)
        ),
        ("core.services", "set_state_event_log") => format!(
            "{}(&mut ({}), ({}).clone())",
            helper("jet_services_set_state_event_log"),
            arg(0),
            arg(1)
        ),
        ("core.services", "commit_snapshot") => format!(
            "{}(&mut ({}), ({}).clone())",
            helper("jet_services_commit_snapshot"),
            arg(0),
            arg(1)
        ),
        ("core.services", "restore_snapshot") => {
            format!("{}(&({}))", helper("jet_services_restore_snapshot"), arg(0))
        }
        ("core.services", "append_event") => format!(
            "{}(&mut ({}), ({}).clone())",
            helper("jet_services_append_event"),
            arg(0),
            arg(1)
        ),
        ("core.services", "event_count") => {
            format!("{}(&({}))", helper("jet_services_event_count"), arg(0))
        }
        ("core.services", "replay_events") => {
            format!("{}(&({}))", helper("jet_services_replay_events"), arg(0))
        }
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
        ("core.services", "workflow_history") => format!(
            "{}(&({}), {})",
            helper("jet_services_workflow_history"),
            arg(0),
            arg(1)
        ),
        ("core.services", "directory_register") => format!(
            "{}(&mut ({}), ({}).clone(), ({}).clone())",
            helper("jet_services_directory_register"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.services", "directory_resolve") => format!(
            "{}(&({}), &({}))",
            helper("jet_services_directory_resolve"),
            arg(0),
            arg(1)
        ),
        ("core.services", "directory_generation") => format!(
            "{}(&({}))",
            helper("jet_services_directory_generation"),
            arg(0)
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
        ("core.services", "observe") => {
            format!("{}(&({}))", helper("jet_services_observe"), arg(0))
        }
        ("core.services", "endpoint_show") => {
            format!("{}(&({}))", helper("jet_services_endpoint_show"), arg(0))
        }
        ("core.services", "tree_show") => {
            format!("{}(&({}))", helper("jet_services_tree_show"), arg(0))
        }
        ("core.data", "table") => format!("{}(&({}))", helper("jet_data_table"), arg(0)),
        ("core.data", "rows") => format!("{}(&({}))", helper("jet_data_rows"), arg(0)),
        ("core.data", "series") => format!("{}(&({}))", helper("jet_data_series"), arg(0)),
        ("core.data", "values") => format!("{}(&({}))", helper("jet_data_series_values"), arg(0)),
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
        ("core.data", "missing_count") => {
            format!("{}(&({}))", helper("jet_data_missing_count"), arg(0))
        }
        ("core.data", "lazy") => format!("{}(&({}))", helper("jet_data_lazy"), arg(0)),
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
        ("core.data", "plan") => format!("{}(&({}))", helper("jet_data_plan"), arg(0)),
        ("core.data", "filter") => {
            format!("{}(&({}), {})", helper("jet_data_filter"), arg(0), arg(1))
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
        ("core.data", "lazy_filter") => {
            format!("{}(&({}), {})", helper("jet_data_lazy_filter"), arg(0), arg(1))
        }
        ("core.data", "lazy_sort_by") => {
            format!("{}(&({}), {})", helper("jet_data_lazy_sort_by"), arg(0), arg(1))
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
        ("core.data", "status") => format!("{}()", helper("jet_data_status")),
        ("core.data", "require_bridge") => {
            format!("{}(&({}))", helper("jet_data_require_bridge"), arg(0))
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
        ("core.data", "csv_reader") => {
            format!(
                "{}({}, {})",
                helper("jet_data_csv_reader"),
                arg(0),
                arg(1)
            )
        }
        ("core.data", "json_reader") => {
            format!(
                "{}({}, {})",
                helper("jet_data_json_reader"),
                arg(0),
                arg(1)
            )
        }
        ("core.fmt", "number") => format!("{}({})", helper("jet_fmt_number"), arg(0)),
        ("core.fmt", "decimal") => {
            format!("{}({}, {})", helper("jet_fmt_decimal"), arg(0), arg(1))
        }
        ("core.fmt", "percent") => {
            format!("{}({}, {})", helper("jet_fmt_percent"), arg(0), arg(1))
        }
        ("core.fmt", "bytes") => format!("{}({})", helper("jet_fmt_bytes"), arg(0)),
        ("core.fmt", "duration") => format!("{}({})", helper("jet_fmt_duration"), arg(0)),
        ("core.fmt", "ordinal") => format!("{}({})", helper("jet_fmt_ordinal"), arg(0)),
        ("core.fmt", "plural") => format!(
            "{}({}, &({}), &({}))",
            helper("jet_fmt_plural"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.fmt", "pad_left") => format!(
            "{}(&({}), {}, &({}))",
            helper("jet_fmt_pad_left"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.fmt", "pad_right") => format!(
            "{}(&({}), {}, &({}))",
            helper("jet_fmt_pad_right"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.fmt", "pad_center") => format!(
            "{}(&({}), {}, &({}))",
            helper("jet_fmt_pad_center"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.encoding.toml", "parse") => {
            format!("{}(&({}))", helper("jet_std_toml_parse"), arg(0))
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
        ("core.encoding.yaml", "parse") => {
            format!("{}(&({}))", helper("jet_std_yaml_parse"), arg(0))
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
        ("core.encoding.xml", "parse") => {
            format!("{}(&({}))", helper("jet_std_xml_parse"), arg(0))
        }
        ("core.encoding.xml", "parse_with") => {
            format!("{}(&({}), &({}))", helper("jet_std_xml_parse_with"), arg(0), arg(1))
        }
        ("core.encoding.xml", "parse_bytes") => {
            let options = if args.len() > 1 {
                arg(1)
            } else {
                format!("{}jet_std::XMLParseOptions::safe()", cx.root_prefix)
            };
            format!("{}(&({}), {})", helper("jet_std_xml_parse_bytes"), arg(0), options)
        }
        ("core.encoding.xml", "to_string") => {
            format!("{}(&({}))", helper("jet_std_xml_render"), arg(0))
        }
        ("core.encoding.xml", "to_bytes") => {
            let options = if args.len() > 1 {
                arg(1)
            } else {
                format!("{}jet_std::XMLRenderOptions::safe()", cx.root_prefix)
            };
            format!("{}(&({}), {})", helper("jet_std_xml_to_bytes"), arg(0), options)
        }
        ("core.encoding.xml", "canonical") => {
            format!("{}(&({}), &({}))", helper("jet_std_xml_canonical"), arg(0), arg(1))
        }
        ("core.encoding.xml", "root") => {
            format!("{}(&({}))", helper("jet_std_xml_root"), arg(0))
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
        ("core.encoding.xml", "attribute") => {
            format!("{}(&({}), &({}))", helper("jet_std_xml_attribute"), arg(0), arg(1))
        }
        ("core.encoding.xml", "content") => {
            format!("{}(&({}))", helper("jet_std_xml_content"), arg(0))
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
        ("core.encoding.cbor", "to_bytes") => {
            format!("{}(&({}))", helper("jet_enc_cbor_to_bytes"), arg(0))
        }
        ("core.encoding.cbor", "to_bytes_canonical") => {
            format!("{}(&({}))", helper("jet_enc_cbor_to_bytes_canonical"), arg(0))
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
        ("core.encoding.hex", "encode") => {
            format!("{}(&({}))", helper("jet_std_hex_encode"), arg(0))
        }
        ("core.encoding.hex", "decode") => {
            format!("{}(&({}))", helper("jet_std_hex_decode"), arg(0))
        }
        ("core.encoding.base64", "encode") => {
            format!("{}(&({}))", helper("jet_std_b64_encode"), arg(0))
        }
        ("core.encoding.base64", "decode") => {
            format!("{}(&({}))", helper("jet_std_b64_decode"), arg(0))
        }
        ("core.encoding.base64", "encode_url") => {
            format!("{}(&({}))", helper("jet_std_b64url_encode"), arg(0))
        }
        ("core.encoding.base64", "decode_url") => {
            format!("{}(&({}))", helper("jet_std_b64url_decode"), arg(0))
        }
        ("core.encoding.base32", "encode") => {
            format!("{}(&({}))", helper("jet_std_base32_encode"), arg(0))
        }
        ("core.encoding.base32", "decode") => {
            format!("{}(&({}))", helper("jet_std_base32_decode"), arg(0))
        }
        // D-UUIDENC1=A: UUID v4 (CSPRNG) and v7 (injectable Clock).
        ("core.uuid", "v4") => format!("{}()", helper("jet_std_uuid_v4")),
        ("core.uuid", "v7") => format!("{}(&({}))", helper("jet_std_uuid_v7"), arg(0)),
        ("core.files", "open") => format!("{}(&({}))", helper("jet_std_files_open"), arg(0)),
        ("core.files", "create") => format!("{}(&({}))", helper("jet_std_files_create"), arg(0)),
        ("core.files", "append") => format!("{}(&({}))", helper("jet_std_files_append"), arg(0)),
        // E2-M7: std.path helpers (D-IO1).
        ("core.path", "join") => format!(
            "{}(&({}), &({}))",
            helper("jet_std_path_join"),
            arg(0),
            arg(1)
        ),
        ("core.path", "parent") => format!("{}(&({}))", helper("jet_std_path_parent"), arg(0)),
        ("core.path", "extension") => {
            format!("{}(&({}))", helper("jet_std_path_extension"), arg(0))
        }
        ("core.path", "normalize") => {
            format!("{}(&({}))", helper("jet_std_path_normalize"), arg(0))
        }
        ("core.url", "parse") => format!("{}(&({}))", helper("jet_url_parse"), arg(0)),
        ("core.url", "from_parts") => format!(
            "{}(&({}), &({}), &({}), &({}), &({}))",
            helper("jet_url_from_parts"),
            arg(0),
            arg(1),
            arg(2),
            arg(3),
            arg(4)
        ),
        ("core.url", "file") => format!("{}(&({}))", helper("jet_url_file"), arg(0)),
        ("core.url", "data") => {
            format!("{}(&({}), &({}))", helper("jet_url_data"), arg(0), arg(1))
        }
        ("core.url", "query") => format!("{}(&({}))", helper("jet_url_query"), arg(0)),
        ("core.url", "percent_encode") => {
            format!(
                "{}(&({}))",
                helper("jet_url_percent_encode_component"),
                arg(0)
            )
        }
        ("core.url", "percent_decode") => {
            format!(
                "{}(&({}))",
                helper("jet_url_percent_decode_component"),
                arg(0)
            )
        }
        ("core.mime", "parse") => format!("{}(&({}))", helper("jet_mime_parse"), arg(0)),
        ("core.mime", "from_extension") => {
            format!("{}(&({}))", helper("jet_mime_from_extension"), arg(0))
        }
        ("core.mime", "extension") => format!("{}(&({}))", helper("jet_mime_extension"), arg(0)),
        ("core.email", "address") => format!("{}jet_email::address(&({}))", cx.root_prefix, arg(0)),
        ("core.email", "attachment") => format!(
            "{}jet_email::attachment(&({}), &({}), &({}))",
            cx.root_prefix, arg(0), arg(1), arg(2)
        ),
        ("core.email", "message") => format!(
            "{}jet_email::message(&({}), &({}), &({}), &({}), &({}), &({}), &({}))",
            cx.root_prefix, arg(0), arg(1), arg(2), arg(3), arg(4), arg(5), arg(6)
        ),
        ("core.email", "envelope") => format!(
            "{}jet_email::envelope(&({}), &({}))", cx.root_prefix, arg(0), arg(1)
        ),
        ("core.email", "serialize") => format!("{}jet_email::serialize(&({}))", cx.root_prefix, arg(0)),
        ("core.email", "smtp") => format!(
            "{}jet_email::smtp({}, {}, {})",
            cx.root_prefix, format!("&({})", arg(0)), regex_fn("jet_crypto_secret_copy_for_smtp_impl"), email_runtime(),
        ),
        ("core.email", "smtp_from_env") => format!(
            "{}jet_email::smtp_from_env({})", cx.root_prefix, email_runtime(),
        ),
        // D-TEXTUNICODE1: std-only Unicode scalar helpers.
        ("core.text.unicode", "scalar_count") => {
            format!("{}(&({}))", helper("jet_text_unicode_scalar_count"), arg(0))
        }
        ("core.text.unicode", "byte_count") => {
            format!("{}(&({}))", helper("jet_text_unicode_byte_count"), arg(0))
        }
        ("core.text.unicode", "is_ascii") => {
            format!("{}(&({}))", helper("jet_text_unicode_is_ascii"), arg(0))
        }
        ("core.text.unicode", "lower") => {
            format!("{}(&({}))", helper("jet_text_unicode_lower"), arg(0))
        }
        ("core.text.unicode", "upper") => {
            format!("{}(&({}))", helper("jet_text_unicode_upper"), arg(0))
        }
        ("core.text.unicode", "scalars") => {
            format!("{}(&({}))", helper("jet_text_unicode_scalars"), arg(0))
        }
        ("core.text", "nfc") => format!("{}(&({}))", helper("jet_text_nfc"), arg(0)),
        ("core.text", "nfd") => format!("{}(&({}))", helper("jet_text_nfd"), arg(0)),
        ("core.text", "nfkc") => format!("{}(&({}))", helper("jet_text_nfkc"), arg(0)),
        ("core.text", "nfkd") => format!("{}(&({}))", helper("jet_text_nfkd"), arg(0)),
        ("core.text", "casefold") => format!("{}(&({}))", helper("jet_text_casefold"), arg(0)),
        ("core.text", "caseless_eq") => {
            format!("{}(&({}), &({}))", helper("jet_text_caseless_eq"), arg(0), arg(1))
        }
        ("core.text", "lower") => format!("{}(&({}))", helper("jet_text_lower"), arg(0)),
        ("core.text", "upper") => format!("{}(&({}))", helper("jet_text_upper"), arg(0)),
        ("core.text", "graphemes") => format!("{}(&({}))", helper("jet_text_graphemes"), arg(0)),
        ("core.text", "words") => format!("{}(&({}))", helper("jet_text_words"), arg(0)),
        ("core.text", "sentences") => format!("{}(&({}))", helper("jet_text_sentences"), arg(0)),
        // D-TEXTWIDTH1=B: 1-arg = portable default (`Int`); 2-arg (`policy:`)
        // routes through the `TextWidth`-taking helper (`Int ? TextError`).
        ("core.text", "display_width") if args.len() >= 2 => format!(
            "{}(&({}), &({}))",
            helper("jet_text_display_width"),
            arg(0),
            arg(1)
        ),
        ("core.text", "display_width") => format!("{}(&({}))", helper("jet_text_display_width_default"), arg(0)),
        ("core.text", "scalar_count") => format!("{}(&({}))", helper("jet_text_unicode_scalar_count"), arg(0)),
        ("core.text", "byte_count") => format!("{}(&({}))", helper("jet_text_unicode_byte_count"), arg(0)),
        ("core.text", "is_alphabetic") => format!("{}(&({}))", helper("jet_text_is_alphabetic"), arg(0)),
        ("core.text", "is_numeric") => format!("{}(&({}))", helper("jet_text_is_numeric"), arg(0)),
        ("core.text", "is_whitespace") => format!("{}(&({}))", helper("jet_text_is_whitespace"), arg(0)),
        ("core.text", "is_ascii") => format!("{}(&({}))", helper("jet_text_unicode_is_ascii"), arg(0)),
        ("core.text", "scalars") => format!("{}(&({}))", helper("jet_text_unicode_scalars"), arg(0)),
        ("core.text", "splitn") => {
            format!("{}(&({}), &({}), {})", helper("jet_text_splitn"), arg(0), arg(1), arg(2))
        }
        ("core.text", "rsplitn") => {
            format!("{}(&({}), &({}), {})", helper("jet_text_rsplitn"), arg(0), arg(1), arg(2))
        }
        ("core.text", "trim") => format!("{}(&({}))", helper("jet_text_trim"), arg(0)),
        ("core.text", "trim_start") => format!("{}(&({}))", helper("jet_text_trim_start"), arg(0)),
        ("core.text", "trim_end") => format!("{}(&({}))", helper("jet_text_trim_end"), arg(0)),
        ("core.text", "pad_start") => {
            format!("{}(&({}), {}, &({}))", helper("jet_text_pad_start"), arg(0), arg(1), arg(2))
        }
        ("core.text", "pad_end") => {
            format!("{}(&({}), {}, &({}))", helper("jet_text_pad_end"), arg(0), arg(1), arg(2))
        }
        ("core.text", "center") => {
            format!("{}(&({}), {}, &({}))", helper("jet_text_center"), arg(0), arg(1), arg(2))
        }
        ("core.text", "starts_any") => {
            format!("{}(&({}), &({}))", helper("jet_text_starts_any"), arg(0), arg(1))
        }
        ("core.text", "ends_any") => {
            format!("{}(&({}), &({}))", helper("jet_text_ends_any"), arg(0), arg(1))
        }
        ("core.text", "char_indices") => format!("{}(&({}))", helper("jet_text_char_indices"), arg(0)),
        // E2-M9: first-party ring packages.
        ("jet.log", "info") => format!("{}(&({}))", helper("jet_ring_log_info"), arg(0)),
        ("jet.log", "warn") => format!("{}(&({}))", helper("jet_ring_log_warn"), arg(0)),
        ("jet.log", "error") => format!("{}(&({}))", helper("jet_ring_log_error"), arg(0)),
        ("jet.log", "debug") => format!("{}(&({}))", helper("jet_ring_log_debug"), arg(0)),
        ("jet.log", "field") => {
            format!("{}(&({}), &({}))", helper("jet_ring_log_field"), arg(0), arg(1))
        }
        ("jet.log", "int") => {
            format!("{}(&({}), {})", helper("jet_ring_log_int"), arg(0), arg(1))
        }
        ("jet.log", "float") => {
            format!("{}(&({}), {})", helper("jet_ring_log_float"), arg(0), arg(1))
        }
        ("jet.log", "bool") => {
            format!("{}(&({}), {})", helper("jet_ring_log_bool"), arg(0), arg(1))
        }
        ("jet.log", "redact") => format!("{}(&({}))", helper("jet_ring_log_redact"), arg(0)),
        ("jet.log", "info_fields") => {
            format!("{}(&({}), &({}))", helper("jet_ring_log_info_fields"), arg(0), arg(1))
        }
        ("jet.log", "warn_fields") => {
            format!("{}(&({}), &({}))", helper("jet_ring_log_warn_fields"), arg(0), arg(1))
        }
        ("jet.log", "error_fields") => {
            format!("{}(&({}), &({}))", helper("jet_ring_log_error_fields"), arg(0), arg(1))
        }
        ("jet.log", "debug_fields") => {
            format!("{}(&({}), &({}))", helper("jet_ring_log_debug_fields"), arg(0), arg(1))
        }
        ("jet.log", "span") => format!("{}(&({}))", helper("jet_ring_log_span"), arg(0)),
        ("jet.log", "enter") => format!("{}(&({}))", helper("jet_ring_log_enter"), arg(0)),
        ("jet.log", "close") => format!("{}(&({}))", helper("jet_ring_log_close"), arg(0)),
        ("jet.log", "set_sink") => {
            format!("{}(&({}), &({}))", helper("jet_ring_log_set_sink"), arg(0), arg(1))
        }
        ("jet.log", "sample_every") => format!("{}({})", helper("jet_ring_log_sample_every"), arg(0)),
        ("jet.log", "counter") => {
            format!("{}(&({}), {})", helper("jet_ring_log_counter"), arg(0), arg(1))
        }
        ("jet.log", "otlp_file") => format!("{}(&({}))", helper("jet_ring_log_otlp_file"), arg(0)),
        ("jet.log", "set_level") => format!("{}(&({}))", helper("jet_ring_log_set_level"), arg(0)),
        // E2-M12 D-OBS3: trace context for structured log records.
        ("jet.log", "set_trace_id") => {
            format!("{}(&({}))", helper("jet_ring_log_set_trace_id"), arg(0))
        }
        // D-LOGFMT1=A: explicit log format override.
        ("jet.log", "setup") => format!("{}(&({}))", helper("jet_ring_log_setup"), arg(0)),
        ("jet.time", "now") => format!("{}()", helper("jet_std_time_now")),
        ("jet.time", "format") => format!(
            "{}({}, &({}))",
            helper("jet_ring_time_format"),
            arg(0),
            arg(1)
        ),
        ("jet.crypto", "sha256") => format!("{}(&({}))", regex_fn("jet_crypto_sha256_typed_impl"), arg(0)),
        ("jet.crypto", "sha256_bytes") => {
            format!("{}(&({}))", helper("jet_ring_crypto_sha256_bytes"), arg(0))
        }
        ("jet.crypto", "sha512_bytes") => {
            format!("{}(&({}))", regex_fn("jet_crypto_sha512_impl"), arg(0))
        }
        ("jet.crypto", "blake3_bytes") => {
            format!("{}(&({}))", regex_fn("jet_crypto_blake3_impl"), arg(0))
        }
        ("jet.crypto", "constant_time_equal_bytes") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_constant_time_equal_bytes_impl"),
            arg(0),
            arg(1)
        ),
        ("jet.crypto", "hkdf_sha256") => format!(
            "{}(&({}), &({}), &({}), {})",
            regex_fn("jet_crypto_hkdf_typed_impl"),
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        ("jet.crypto", "x25519_public") => {
            format!(
                "{}(&({}))",
                regex_fn("jet_crypto_x25519_public_impl"),
                arg(0)
            )
        }
        ("jet.crypto", "x25519_shared") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_x25519_shared_impl"),
            arg(0),
            arg(1)
        ),
        ("jet.crypto", "password_hash") => {
            format!(
                "{}(&({}), jet_scheduler_wait_point_cancelled, jet_task_deliver_cancel, jet_scheduler_blocking_wait_enter, jet_scheduler_blocking_wait_leave)",
                regex_fn("jet_crypto_password_hash_typed_cancel_impl"),
                arg(0)
            )
        }
        ("jet.crypto", "password_hash_with_salt") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_password_hash_with_salt_impl"),
            arg(0),
            arg(1)
        ),
        ("jet.crypto", "password_verify") => format!(
            "{}(&({}), &({}), jet_scheduler_wait_point_cancelled, jet_task_deliver_cancel, jet_scheduler_blocking_wait_enter, jet_scheduler_blocking_wait_leave)",
            regex_fn("jet_crypto_password_verify_typed_cancel_impl"),
            arg(0),
            arg(1)
        ),
        // D-CRYPTOENV1=A: misuse-resistant envelope (RustCrypto FFI bridge).
        ("jet.crypto", "seal") => format!(
            "{}({}, &({}), &({}))",
            regex_fn("jet_crypto_seal_typed_impl"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("jet.crypto", "file_seal") => format!(
            "{}({}, &({}.inner.to_string_lossy().into_owned()), &({}.inner.to_string_lossy().into_owned()), {}jet_scheduler_task_cancelled)",
            regex_fn("jet_crypto_file_seal_impl"),
            arg(0),
            arg(1),
            arg(2),
            cx.root_prefix,
        ),
        ("jet.crypto", "open") => format!(
            "{}(&({}), {}, &({}))",
            regex_fn("jet_crypto_open_typed_impl"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("jet.crypto", "file_open") => format!(
            "{}(&({}), &({}.inner.to_string_lossy().into_owned()), &({}.inner.to_string_lossy().into_owned()), {}jet_scheduler_task_cancelled)",
            regex_fn("jet_crypto_file_open_impl"),
            arg(0),
            arg(1),
            arg(2),
            cx.root_prefix,
        ),
        ("jet.crypto", "sign") => format!(
            "{}(&({}), &({}))",
            regex_fn("jet_crypto_sign_typed_impl"),
            arg(0),
            arg(1)
        ),
        ("jet.crypto", "verify") => format!(
            "{}({}, &({}), {})",
            regex_fn("jet_crypto_verify_typed_impl"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("jet.crypto", "wrap") => format!("{}(&({}), {})", regex_fn("jet_crypto_wrap_typed_impl"), arg(0), arg(1)),
        ("jet.crypto", "unwrap") => format!("{}(&({}), {})", regex_fn("jet_crypto_unwrap_typed_impl"), arg(0), arg(1)),
        ("jet.crypto", "x25519") => format!("{}(&({}), {})", regex_fn("jet_crypto_x25519_typed_impl"), arg(0), arg(1)),
        ("jet.crypto", "constant_time_equal") => format!("{}(&({}), &({}))", regex_fn("jet_crypto_constant_time_secret_impl"), arg(0), arg(1)),
        ("jet.crypto", "blake3") => format!("{}(&({}))", regex_fn("jet_crypto_blake3_typed_impl"), arg(0)),
        ("jet.crypto", "sha512") => format!("{}(&({}))", regex_fn("jet_crypto_sha512_typed_impl"), arg(0)),
        ("jet.crypto", "__secret_from_text") => format!("{}({})", regex_fn("jet_crypto_secret_from_text_impl"), arg(0)),
        ("jet.crypto", "__secret_from_bytes") => format!("{}({})", regex_fn("jet_crypto_secret_from_bytes_impl"), arg(0)),
        ("jet.crypto", "__signing_generate") => format!("{}()", regex_fn("jet_crypto_signing_generate_impl")),
        ("jet.crypto", "__x25519_generate") => format!("{}()", regex_fn("jet_crypto_x25519_generate_impl")),
        ("jet.crypto", "__verify_key_from_bytes") => format!("{}({})", regex_fn("jet_crypto_verify_key_from_bytes_impl"), arg(0)),
        ("jet.crypto", "__x25519_public_from_bytes") => format!("{}({})", regex_fn("jet_crypto_x25519_public_from_bytes_impl"), arg(0)),
        ("jet.crypto", "__x25519_public_from_text") => format!("{}({})", regex_fn("jet_crypto_x25519_public_from_text_impl"), arg(0)),
        ("jet.crypto", "__signature_from_bytes") => format!("{}({})", regex_fn("jet_crypto_signature_from_bytes_impl"), arg(0)),
        ("jet.crypto", "__sealed_from_bytes") => format!("{}({})", regex_fn("jet_crypto_sealed_from_bytes_impl"), arg(0)),
        ("jet.crypto", "__wrapped_from_bytes") => format!("{}({})", regex_fn("jet_crypto_wrapped_from_bytes_impl"), arg(0)),
        ("jet.crypto", "__password_parse") => format!("{}({})", regex_fn("jet_crypto_password_parse_impl"), arg(0)),
        ("jet.crypto", "__signing_public") => format!("{}(&({}))", regex_fn("jet_crypto_signing_public_impl"), arg(0)),
        ("jet.crypto", "__x25519_public") => format!("{}(&({}))", regex_fn("jet_crypto_x25519_public_typed_impl"), arg(0)),
        ("jet.crypto", "__verify_key_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_verify_key_bytes_impl"), arg(0)),
        ("jet.crypto", "__x25519_public_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_x25519_public_bytes_impl"), arg(0)),
        ("jet.crypto", "__x25519_public_text") => format!("{}(&({}))", regex_fn("jet_crypto_x25519_public_text_impl"), arg(0)),
        ("jet.crypto", "__signature_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_signature_bytes_impl"), arg(0)),
        ("jet.crypto", "__sealed_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_sealed_bytes_impl"), arg(0)),
        ("jet.crypto", "__wrapped_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_wrapped_bytes_impl"), arg(0)),
        ("jet.crypto", "__digest256_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_digest256_bytes_impl"), arg(0)),
        ("jet.crypto", "__digest512_bytes") => format!("{}(&({}))", regex_fn("jet_crypto_digest512_bytes_impl"), arg(0)),
        ("jet.crypto", "__digest256_hex") => format!("{}(&({}))", regex_fn("jet_crypto_digest256_hex_impl"), arg(0)),
        ("jet.crypto", "__digest512_hex") => format!("{}(&({}))", regex_fn("jet_crypto_digest512_hex_impl"), arg(0)),
        ("jet.crypto", "__password_text") => format!("{}(&({}))", regex_fn("jet_crypto_password_text_impl"), arg(0)),
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
        ("core.crypto.expert", "x25519") => {
            let reject_all_zero = if args.len() == 3 { arg(2) } else { "true".to_string() };
            format!("{}(&({}), &({}), {})", regex_fn("jet_crypto_expert_x25519_impl"), arg(0), arg(1), reject_all_zero)
        }
        ("core.crypto.expert", "hkdf_sha256") => format!("{}(&({}), &({}), &({}), {})", regex_fn("jet_crypto_expert_hkdf_sha256_impl"), arg(0), arg(1), arg(2), arg(3)),
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
        ("core.auth", "session_validate") => format!(
            "{}(&({}), {})",
            helper("jet_auth_session_validate"),
            arg(0),
            arg(1)
        ),
        ("core.auth", "session_show") => {
            format!("{}(&({}))", helper("jet_auth_session_show"), arg(0))
        }
        ("core.auth", "session_user") => {
            format!("{}(&({}))", helper("jet_auth_session_user"), arg(0))
        }
        ("core.auth", "session_cookie") => {
            format!("{}(&({}))", helper("jet_auth_session_cookie"), arg(0))
        }
        ("core.auth", "session_id") => {
            format!("{}(&({}))", helper("jet_auth_session_id"), arg(0))
        }
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
            "{}(({}), ({}).clone(), ({}).clone())",
            helper("jet_sync_text_set"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.sync", "text_merge") => format!(
            "{}(&({}), &({}))",
            helper("jet_sync_text_merge"),
            arg(0),
            arg(1)
        ),
        ("core.sync", "text_show") => {
            format!("{}(&({}))", helper("jet_sync_text_show"), arg(0))
        }
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
        ("core.sync", "counter_merge") => format!(
            "{}(&({}), &({}))",
            helper("jet_sync_counter_merge"),
            arg(0),
            arg(1)
        ),
        ("core.sync", "counter_value") => {
            format!("{}(&({}))", helper("jet_sync_counter_value"), arg(0))
        }
        ("core.sync", "map_new") => format!("{}()", helper("jet_sync_map_new")),
        ("core.sync", "map_set") => format!(
            "{}(({}), ({}).clone(), ({}).clone())",
            helper("jet_sync_map_set"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.sync", "map_get") => format!(
            "{}(&({}), &({}))",
            helper("jet_sync_map_get"),
            arg(0),
            arg(1)
        ),
        ("core.sync", "map_merge") => format!(
            "{}(&({}), &({}))",
            helper("jet_sync_map_merge"),
            arg(0),
            arg(1)
        ),
        ("core.sync", "map_show") => {
            format!("{}(&({}))", helper("jet_sync_map_show"), arg(0))
        }
        ("core.sync", "list_new") => format!("{}()", helper("jet_sync_list_new")),
        ("core.sync", "list_push") => format!(
            "{}(({}), ({}).clone(), ({}).clone())",
            helper("jet_sync_list_push"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.sync", "list_merge") => format!(
            "{}(&({}), &({}))",
            helper("jet_sync_list_merge"),
            arg(0),
            arg(1)
        ),
        ("core.sync", "list_show") => {
            format!("{}(&({}))", helper("jet_sync_list_show"), arg(0))
        }
        ("core.sync", "policy_new") => format!(
            "{}(({}).clone(), ({}).clone())",
            helper("jet_db_policy_new"),
            arg(0),
            arg(1)
        ),
        ("core.sync", "policy_allows") => format!(
            "{}(&({}), &({}), &({}))",
            helper("jet_db_policy_allows"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.sync", "policy_show") => {
            format!("{}(&({}))", helper("jet_db_policy_show"), arg(0))
        }
        // U13 (D-JPK-SECRETCRYPTO1): `core.vault.get` — reads `.jet/secrets.age`
        // (project-relative) and decrypts with the local identity, via the
        // age-style crypto FFI bridge. Already the exact `Option<String>` shape
        // (`None` on any failure — missing file, missing entry, bad identity).
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
        ("jet.crypto", "__vault_wrapped_from_bytes") =>
            format!("{}({})", regex_fn("jet_vault_wrapped_from_bytes_impl"), arg(0)),
        ("jet.crypto", "__vault_wrapped_bytes") =>
            format!("{}(&({}))", regex_fn("jet_vault_wrapped_bytes_impl"), arg(0)),
        ("jet.crypto", "__vault_unlock_recipient") =>
            format!("{}(&({}))", regex_fn("jet_vault_unlock_recipient_impl"), arg(0)),
        ("jet.crypto", "__vault_unlock_passphrase") =>
            format!("{}(&({}))", regex_fn("jet_vault_unlock_passphrase_impl"), arg(0)),
        // D-NETSOCKET1=A: core.net — typed addresses, TCP/UDP/Unix/DNS, TLS handle.
        ("core.net", "ip_addr") => format!("{}(&({}))", helper("jet_net_ip_addr"), arg(0)),
        ("core.net", "ip_to_string") => {
            format!("{}(&({}))", helper("jet_net_ip_to_string"), arg(0))
        }
        ("core.net", "ip_is_ipv4") => format!("{}(&({}))", helper("jet_net_ip_is_ipv4"), arg(0)),
        ("core.net", "socket_addr") => {
            format!(
                "{}(&({}), {})",
                helper("jet_net_socket_addr"),
                arg(0),
                arg(1)
            )
        }
        ("core.net", "socket_addr_parse") => {
            format!("{}(&({}))", helper("jet_net_socket_addr_parse"), arg(0))
        }
        ("core.net", "socket_host") => format!("{}(&({}))", helper("jet_net_socket_host"), arg(0)),
        ("core.net", "socket_port") => format!("{}(&({}))", helper("jet_net_socket_port"), arg(0)),
        ("core.net", "socket_to_string") => {
            format!("{}(&({}))", helper("jet_net_socket_to_string"), arg(0))
        }
        ("core.net", "tcp_listen") => format!("{}(&({}))", helper("jet_net_tcp_listen"), arg(0)),
        ("core.net", "tcp_listen_addr") => {
            format!("{}(&({}))", helper("jet_net_tcp_listen_addr"), arg(0))
        }
        ("core.net", "tcp_accept") => format!("{}(&({}))", helper("jet_net_tcp_accept"), arg(0)),
        ("core.net", "tcp_connect") => format!("{}(&({}))", helper("jet_net_tcp_connect"), arg(0)),
        ("core.net", "tcp_connect_addr") => {
            format!("{}(&({}))", helper("jet_net_tcp_connect_addr"), arg(0))
        }
        ("core.net", "tcp_connect_timeout") => format!(
            "{}(&({}), {})",
            helper("jet_net_tcp_connect_timeout"),
            arg(0),
            arg(1)
        ),
        ("core.net", "tcp_connect_happy") => format!(
            "{}(&({}), {}, {})",
            helper("jet_net_tcp_connect_happy"),
            arg(0),
            arg(1),
            arg(2)
        ),
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
        ("core.net", "ready_readable") => {
            format!("{}(&({}))", helper("jet_net_ready_readable"), arg(0))
        }
        ("core.net", "ready_writable") => {
            format!("{}(&({}))", helper("jet_net_ready_writable"), arg(0))
        }
        ("core.net", "error_operation") => format!("{}(&({}))", helper("jet_net_error_operation"), arg(0)),
        ("core.net", "error_address") => format!("{}(&({}))", helper("jet_net_error_address"), arg(0)),
        ("core.net", "error_name") => format!("{}(&({}))", helper("jet_net_error_name"), arg(0)),
        ("core.net", "error_message") => format!("{}(&({}))", helper("jet_net_error_message"), arg(0)),
        ("core.net", "error_os_code") => format!("{}(&({}))", helper("jet_net_error_os_code"), arg(0)),
        ("core.net", "tcp_local_addr") => {
            format!("{}(&({}))", helper("jet_net_tcp_local_addr"), arg(0))
        }
        ("core.net", "tcp_peer_addr") => {
            format!("{}(&({}))", helper("jet_net_tcp_peer_addr"), arg(0))
        }
        ("core.net", "tcp_local_socket_addr") => {
            format!("{}(&({}))", helper("jet_net_tcp_local_socket_addr"), arg(0))
        }
        ("core.net", "tcp_peer_socket_addr") => {
            format!("{}(&({}))", helper("jet_net_tcp_peer_socket_addr"), arg(0))
        }
        ("core.net", "listener_local_socket_addr") => format!(
            "{}(&({}))",
            helper("jet_net_listener_local_socket_addr"),
            arg(0)
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
        ("core.net", "tcp_reply") => format!(
            "{}({}, &({}), &({}))",
            helper("jet_net_tcp_reply"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.net", "udp_bind") => format!("{}(&({}))", helper("jet_net_udp_bind"), arg(0)),
        ("core.net", "udp_bind_addr") => {
            format!("{}(&({}))", helper("jet_net_udp_bind_addr"), arg(0))
        }
        ("core.net", "udp_local_addr") => {
            format!("{}(&({}))", helper("jet_net_udp_local_addr"), arg(0))
        }
        ("core.net", "udp_set_timeout") => format!(
            "{}(&({}), {})",
            helper("jet_net_udp_set_timeout"),
            arg(0),
            arg(1)
        ),
        ("core.net", "udp_send_to") => format!(
            "{}(&({}), &({}), &({}))",
            helper("jet_net_udp_send_to"),
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.net", "udp_recv_from") => format!(
            "{}(&({}), {})",
            helper("jet_net_udp_recv_from"),
            arg(0),
            arg(1)
        ),
        ("core.net", "udp_send_bytes_to") => format!(
            "{}(&({}), &({}), &({}))", helper("jet_net_udp_send_bytes_to"), arg(0), arg(1), arg(2)
        ),
        ("core.net", "udp_receive") => format!(
            "{}(&({}), {})", helper("jet_net_udp_receive"), arg(0), arg(1)
        ),
        ("core.net", "udp_packet_data") => {
            format!("{}(&({}))", helper("jet_net_udp_packet_data"), arg(0))
        }
        ("core.net", "udp_packet_addr") => {
            format!("{}(&({}))", helper("jet_net_udp_packet_addr"), arg(0))
        }
        ("core.net", "udp_packet_bytes") => {
            format!("{}(&({}))", helper("jet_net_udp_packet_bytes"), arg(0))
        }
        ("core.net", "udp_packet_original_len") => format!(
            "{}(&({}))", helper("jet_net_udp_packet_original_len"), arg(0)
        ),
        ("core.net", "udp_packet_truncated") => format!(
            "{}(&({}))", helper("jet_net_udp_packet_truncated"), arg(0)
        ),
        ("core.net", "unix_listen") => format!("{}(&({}))", helper("jet_net_unix_listen"), arg(0)),
        ("core.net", "unix_accept") => format!("{}(&({}))", helper("jet_net_unix_accept"), arg(0)),
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
        ("core.net", "dns_srv_target") => {
            format!("{}(&({}))", helper("jet_net_dns_srv_target"), arg(0))
        }
        ("core.net", "dns_srv_port") => {
            format!("{}(&({}))", helper("jet_net_dns_srv_port"), arg(0))
        }
        ("core.net", "dns_srv_priority") => {
            format!("{}(&({}))", helper("jet_net_dns_srv_priority"), arg(0))
        }
        ("core.net", "dns_srv_weight") => {
            format!("{}(&({}))", helper("jet_net_dns_srv_weight"), arg(0))
        }
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
        // E2-M10: jet.http — HTTP client.
        ("jet.http", "get") => {
            let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
            emit_http_response_from_bridge(
                format!("{ffi}::JetHTTPAmbientDeadline::push(jet_deadline_remaining_ms()).and_then(|_ambient| {ffi}::jet_http_client_get_impl(&({})))", arg(0)),
                ffi,
            )
        }
        ("jet.http", "post") => {
            let ffi = cx.ffi_crate.as_deref().unwrap_or("jet_ffi");
            emit_http_response_from_bridge(
                format!("{ffi}::JetHTTPAmbientDeadline::push(jet_deadline_remaining_ms()).and_then(|_ambient| {ffi}::jet_http_client_post_impl(&({}), &({})))", arg(0), arg(1)),
                ffi,
            )
        }
        // c109 Phase 25: HTTPRouter producer + parse/dispatch (D-ROUTE1=A), byte-for-byte
        // `emit_core_call` (Source/Codegen/Expression.rs ~L1411). `router()` is arg-free;
        // `parse(raw)` borrows the raw string; `dispatch(router, req)` borrows the router
        // and passes the request by value.
        ("jet.http", "router") => format!("{}()", helper("jet_http_router_new")),
        ("jet.http", "parse") => format!("{}(&({}))", helper("jet_http_parse_request"), arg(0)),
        ("jet.http", "dispatch") => format!(
            "{}(&({}), {})",
            helper("jet_http_router_dispatch"),
            arg(0),
            arg(1)
        ),
        // D-REGEXENGINE1=A: core.regex — std-only runtime in jet_std, no bridge dep.
        ("jet.regex", "flags") => {
            format!(
                "{}jet_std::jet_regex_flags({}, {}, {})",
                cx.root_prefix,
                arg(0),
                arg(1),
                arg(2)
            )
        }
        ("jet.regex", "compile") => {
            format!(
                "{}jet_std::jet_regex_compile(&({}))",
                cx.root_prefix,
                arg(0)
            )
        }
        ("jet.regex", "compile_with") => {
            format!(
                "{}jet_std::jet_regex_compile_with(&({}), &({}))",
                cx.root_prefix,
                arg(0),
                arg(1)
            )
        }
        ("jet.regex", "literal") => {
            format!(
                "{}jet_std::jet_regex_literal(&({}))",
                cx.root_prefix,
                arg(0)
            )
        }
        ("jet.regex", "is_match") => {
            format!(
                "{}jet_std::jet_regex_is_match(&({}), &({}))",
                cx.root_prefix,
                arg(0),
                arg(1)
            )
        }
        ("jet.regex", "match") => {
            format!(
                "{}jet_std::jet_regex_match(&({}), &({}))",
                cx.root_prefix,
                arg(0),
                arg(1)
            )
        }
        ("jet.regex", "find") => {
            format!(
                "{}jet_std::jet_regex_find(&({}), &({}))",
                cx.root_prefix,
                arg(0),
                arg(1)
            )
        }
        ("jet.regex", "find_all") => {
            format!(
                "{}jet_std::jet_regex_find_all(&({}), &({}))",
                cx.root_prefix,
                arg(0),
                arg(1)
            )
        }
        ("jet.regex", "matches") => {
            format!(
                "{}jet_std::jet_regex_matches(&({}), &({}))",
                cx.root_prefix,
                arg(0),
                arg(1)
            )
        }
        ("jet.regex", "split") => {
            format!(
                "{}jet_std::jet_regex_split(&({}), &({}))",
                cx.root_prefix,
                arg(0),
                arg(1)
            )
        }
        ("jet.regex", "split_limit") => {
            format!(
                "{}jet_std::jet_regex_split_limit(&({}), &({}), {})",
                cx.root_prefix,
                arg(0),
                arg(1),
                arg(2)
            )
        }
        ("jet.regex", "replace") => format!(
            "{}jet_std::jet_regex_replace(&({}), &({}), &({}))",
            cx.root_prefix,
            arg(0),
            arg(1),
            arg(2)
        ),
        ("jet.regex", "replace_all") => format!(
            "{}jet_std::jet_regex_replace_all(&({}), &({}), &({}))",
            cx.root_prefix,
            arg(0),
            arg(1),
            arg(2)
        ),
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
        ("core.raylib", "window_open") => {
            format!(
                "{}jet_raylib_window_open({}, {}, &({}))",
                cx.root_prefix,
                arg(0),
                arg(1),
                arg(2)
            )
        }
        ("core.raylib", "window_should_close") => {
            format!(
                "{}jet_raylib_window_should_close(&({}))",
                cx.root_prefix,
                arg(0)
            )
        }
        ("core.raylib", "window_ready") => {
            format!("{}jet_raylib_window_ready(&({}))", cx.root_prefix, arg(0))
        }
        ("core.raylib", "begin_drawing") => {
            format!("{}jet_raylib_begin_drawing(&({}))", cx.root_prefix, arg(0))
        }
        ("core.raylib", "clear_background") => {
            format!(
                "{}jet_raylib_clear_background(&({}))",
                cx.root_prefix,
                arg(0)
            )
        }
        ("core.raylib", "draw_text") => {
            format!(
                "{}jet_raylib_draw_text(&({}), {}, {}, {}, &({}))",
                cx.root_prefix,
                arg(0),
                arg(1),
                arg(2),
                arg(3),
                arg(4)
            )
        }
        ("core.raylib", "draw_rectangle") => {
            format!(
                "{}jet_raylib_draw_rectangle({}, {}, {}, {}, &({}))",
                cx.root_prefix,
                arg(0),
                arg(1),
                arg(2),
                arg(3),
                arg(4)
            )
        }
        ("core.raylib", "end_drawing") => {
            format!("{}jet_raylib_end_drawing()", cx.root_prefix)
        }
        ("core.raylib", "close_window") => {
            format!("{}jet_raylib_close_window(&({}))", cx.root_prefix, arg(0))
        }
        ("core.raylib", "key_down") => {
            format!("{}jet_raylib_key_down(&({}))", cx.root_prefix, arg(0))
        }
        ("core.raylib", "set_target_fps") => {
            format!("{}jet_raylib_set_target_fps({})", cx.root_prefix, arg(0))
        }
        ("core.raylib", "load_sound") => {
            format!("{}jet_raylib_load_sound(&({}))", cx.root_prefix, arg(0))
        }
        ("core.raylib", "play_sound") => {
            format!("{}jet_raylib_play_sound(&({}))", cx.root_prefix, arg(0))
        }
        ("core.raylib", "color") => {
            format!(
                "{}jet_raylib_color({}, {}, {}, {})",
                cx.root_prefix,
                arg(0),
                arg(1),
                arg(2),
                arg(3)
            )
        }
        // D-CORE-COMPRESS1=A / D-CODECS1: canonical gzip/zstd stream codecs
        // via the FFI bridge crate. `compress` is
        // infallible; `decompress` returns a Rust `Result<Vec<u8>, String>` which is
        // already the runtime shape of the Jet `Result<[U8], String>` value — no
        // extra wrapping needed (same pattern as `jet.crypto`'s seal/open).
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
        // D-DBDRIVER1: jet.db — SQLite via the FFI bridge crate. `open`/`open_memory`
        // are the only module-level entry points; they wrap the bridge's raw u64
        // handle in the Jet-visible `DBConnection` handle (`JetDbConnection`), so
        // every other operation dispatches by receiver TYPE as an instance method
        // (`THandleOp::DBQuery`/… in the `HandleMethod` arm below), not a second
        // module-call surface.
        ("jet.db", "open") => {
            format!(
                "{}JetDbConnection {{ handle: {}(&({})) }}",
                cx.root_prefix,
                regex_fn("jet_db_open"),
                arg(0)
            )
        }
        ("jet.db", "open_memory") => {
            format!(
                "{}JetDbConnection {{ handle: {}() }}",
                cx.root_prefix,
                regex_fn("jet_db_open_memory")
            )
        }
        ("jet.db", "policy") => format!(
            "{}jet_db_policy_new(({}).clone(), ({}).clone())",
            cx.root_prefix,
            arg(0),
            arg(1)
        ),
        ("jet.db", "params") => {
            format!("{}jet_std::jet_db_params_from_sql(&({}))", cx.root_prefix, arg(0))
        }
        ("jet.db", "row_value") => {
            format!("{}jet_std::jet_db_row_value(&({}), &({}))", cx.root_prefix, arg(0), arg(1))
        }
        ("jet.db", "row_int") => {
            format!("{}jet_std::jet_db_row_int(&({}), &({}))", cx.root_prefix, arg(0), arg(1))
        }
        ("jet.db", "row_float") => {
            format!("{}jet_std::jet_db_row_float(&({}), &({}))", cx.root_prefix, arg(0), arg(1))
        }
        ("jet.db", "row_text") => {
            format!("{}jet_std::jet_db_row_text(&({}), &({}))", cx.root_prefix, arg(0), arg(1))
        }
        ("jet.db", "row_bool") => {
            format!("{}jet_std::jet_db_row_bool(&({}), &({}))", cx.root_prefix, arg(0), arg(1))
        }
        ("jet.db", "transaction") => {
            format!(
                "jet_db_scope_transaction(&({}), &({}), &({}))",
                arg(0),
                arg(1),
                arg(2),
            )
        }
        ("jet.db", "migrate") => {
            format!(
                "jet_db_scope_migrate(&({}), &({}), &({}))",
                arg(0),
                arg(1),
                arg(2),
            )
        }
        // D-DEP-WASM1=A / D-PLUGIN1=B (c81): core.plugin — sandboxed WASM
        // Component Model loader via the FFI bridge crate (wasmtime,
        // runtime-side only, I6). `load` is the only module-level entry
        // point; it wraps the bridge's wire-encoded handle in the Jet-visible
        // `Plugin` handle (`JetPlugin`), so `.call`/`.call_int` dispatch by
        // receiver TYPE as instance methods (`THandleOp::PluginCall{,Int}`
        // in the `HandleMethod` arm below), not a second module-call surface.
        ("jet.plugin", "load") => {
            format!(
                "{root}JetPlugin {{ handle: {root}jet_std::jet_plugin_load_handle(&{}(&({}))) }}",
                regex_fn("jet_plugin_load"),
                arg(0),
                root = cx.root_prefix,
            )
        }
        // c109 Phase 20: the polymorphic core specials — byte-for-byte `emit_core_call`.
        // Their return type is arg-type dependent (resolved by sema's bespoke
        // `infer_core_call` and written onto the node's `resolved_ret`, read at
        // lowering), but the EMITTED form is a fixed per-`(module, method)` string —
        // no type decision here (I3). Args are emitted PLAINLY, exactly `emit_core_call`.
        ("core.math", "abs") => format!("({}).abs()", arg(0)),
        ("core.math", "min") => format!("({}).min({})", arg(0), arg(1)),
        ("core.math", "max") => format!("({}).max({})", arg(0), arg(1)),
        ("core.math", "clamp") => format!("({}).clamp({}, {})", arg(0), arg(1), arg(2)),
        ("core.random", "pick") => format!("{}(&({}))", helper("jet_std_random_pick"), arg(0)),
        ("core.random", "weighted_pick") => {
            format!("{}(&({}), &({}))", helper("jet_std_random_weighted_pick"), arg(0), arg(1))
        }
        ("core.random", "sample") => {
            format!("{}(&({}), {})", helper("jet_std_random_sample"), arg(0), arg(1))
        }
        ("core.random", "shuffle") => {
            format!("{}(&mut ({}))", helper("jet_std_random_shuffle"), arg(0))
        }
        ("core.io", "eprint") => format!("eprintln!(\"{{}}\", ({}).jet_show())", arg(0)),
        ("core.io", "print") => format!("println!(\"{{}}\", ({}).jet_show())", arg(0)),
        // D-TERM1 (ratified 2026-06-22): terminal direct-input.
        ("core.term", "read_key") => format!("{}()", helper("jet_term_read_key")),
        // D-FIDELITY-API1=A: runtime-global fidelity signal.
        ("core.perf", "fidelity") => format!("jet_perf_fidelity()"),
        ("core.perf", "default_fidelity") => format!("jet_perf_default_fidelity()"),
        ("core.perf", "override_fidelity") => {
            format!("jet_perf_override_fidelity({})", arg(0))
        }
        ("core.perf", "reset_fidelity") => format!("jet_perf_reset_fidelity()"),
        // D-RENDERTGT2=A (c133 M1): UI backend seam constructors.
        ("core.ui", "null_backend") => format!("{}jet_ui_null()", cx.root_prefix),
        ("core.ui", "tui_backend") => format!("{}jet_ui_tui()", cx.root_prefix),
        // D-UIDEVSHELL1=A (c134 Phase 8): native Linux GTK4 backend constructor.
        ("core.ui", "gtk_backend") => format!("{}jet_ui_gtk()", cx.root_prefix),
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
        ("core.ui", "point") => format!("{}jet_ui_point({}, {})", cx.root_prefix, arg(0), arg(1)),
        ("core.ui", "size") => format!("{}jet_ui_size({}, {})", cx.root_prefix, arg(0), arg(1)),
        ("core.ui", "rect") => format!(
            "{}jet_ui_rect({}, {}, {}, {})",
            cx.root_prefix,
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        ("core.ui", "constraint") => format!(
            "{}jet_ui_constraint({}, {}, {}, {})",
            cx.root_prefix,
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        ("core.ui", "node") => format!(
            "{}jet_ui_node(&({}), {}, {})",
            cx.root_prefix,
            arg(0),
            arg(1),
            arg(2)
        ),
        ("core.ui", "text") => {
            format!("{}jet_ui_text(&({}))", cx.root_prefix, arg(0))
        }
        ("core.ui", "button") => {
            format!("{}jet_ui_button(&({}))", cx.root_prefix, arg(0))
        }
        ("core.ui", "box") => {
            format!("{}jet_ui_box(({}).clone())", cx.root_prefix, arg(0))
        }
        ("core.ui", "key_event") => format!("{}jet_ui_key_event(&({}))", cx.root_prefix, arg(0)),
        ("core.ui", "resize_event") => format!(
            "{}jet_ui_resize_event({}, {})",
            cx.root_prefix,
            arg(0),
            arg(1)
        ),
        // D-A11YGATE1=B (c134 Phase 6): accessible-role node + role constants.
        ("core.ui", "node_role") => format!(
            "{}jet_ui_node_role(&({}), {}, {}, {})",
            cx.root_prefix,
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        // D-STYLESHAPE1=A wiring: a node carrying an explicit fill color.
        ("core.ui", "node_color") => format!(
            "{}jet_ui_node_color(&({}), {}, {}, &({}))",
            cx.root_prefix,
            arg(0),
            arg(1),
            arg(2),
            arg(3)
        ),
        ("core.ui", "aria_role_button") => {
            format!("{}jet_ui_aria_role_button()", cx.root_prefix)
        }
        ("core.ui", "aria_role_text_input") => {
            format!("{}jet_ui_aria_role_text_input()", cx.root_prefix)
        }
        ("core.ui", "aria_role_label") => {
            format!("{}jet_ui_aria_role_label()", cx.root_prefix)
        }
        ("core.ui", "aria_role_container") => {
            format!("{}jet_ui_aria_role_container()", cx.root_prefix)
        }
        // D-FLAGSHIP-WEBAPI1=A: browser-only helpers. Native TIR emission stays
        // inert so web TIR validation can lower checked JS bodies without making
        // rustc the browser API checker.
        ("core.web", "on") => "{ let _ = || (); () }".to_string(),
        ("core.web", "value") => "String::new()".to_string(),
        // D-WEBAPP1=D: application builder + page helper.
        ("core.web", "app") => format!("{}jet_web_app()", cx.root_prefix),
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
        ("app" | "core.web", "live_get") => {
            format!("{}jet_app_live_get(&({}))", cx.root_prefix, arg(0))
        }
        ("app" | "core.web", "live_show") => {
            format!("{}jet_app_live_show(&({}))", cx.root_prefix, arg(0))
        }
        ("app" | "core.web", "live_stats") => {
            format!("{}jet_app_live_stats()", cx.root_prefix)
        }
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
        ("app" | "core.web", "auth_routes") => {
            format!("{}jet_app_auth_routes(&({}))", cx.root_prefix, arg(0))
        }
        ("app" | "core.web", "auth_show") => {
            format!("{}jet_app_auth_show(&({}))", cx.root_prefix, arg(0))
        }
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
        ("core.web.devserver", "for_app") => {
            format!("{}jet_devserver_for_app(&({}))", cx.root_prefix, arg(0))
        }
        ("core.web.devserver", "app") => {
            format!("{}jet_devserver_app()", cx.root_prefix)
        }
        // D-APPROX1=A: sketch constructors.
        ("core.sketch.hll", "new") => format!("JetHyperLogLog::new()"),
        ("core.sketch.tdigest", "new") => format!("JetTDigest::new()"),
        ("core.sketch.cms", "new") => format!("JetCountMinSketch::new()"),
        ("core.sketch.reservoir", "new") => format!("JetReservoirSampler::new({})", arg(0)),
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
        ("core.browser", "profile") => {
            format!("jet_browser_profile(&({}))", arg(0))
        }
        ("core.browser", "timeout") => format!("jet_browser_timeout({})", arg(0)),
        ("core.browser", "locked") => format!("jet_browser_locked(&({}))", arg(0)),
        ("core.browser", "connect") => {
            format!("jet_browser_connect(&({}))", arg(0))
        }
        ("core.browser", "connect_profile") => format!(
            "jet_browser_connect_profile(&({}), &({}), {})",
            arg(0),
            arg(1),
            arg(2)
        ),
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
        ("core.http.server", "mux") => format!("jet_http_mux_new()"),
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
        ("core.http.server", "response") => {
            format!("jet_http_srv_response({}, &({}))", arg(0), arg(1))
        }
        ("core.http.server", "tls") => format!("jet_http_srv_tls(&({}), &({}))", arg(0), arg(1)),
        ("core.http.server", "sse") => format!("jet_http_srv_sse(&({}))", arg(0)),
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
        ("core.http.server", "json") => {
            format!("jet_http_srv_json({}, &({}))", arg(0), arg(1))
        }
        // D-HTTP-STATIC-FILES1=A: mount a directory. The trailing options keep
        // the safe defaults when the program leaves them off.
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
        ("core.http.server", "cors") => {
            format!("jet_http_srv_install_cors(&({}), &({}))", arg(0), arg(1))
        }
        ("core.http.server", "access_log") => {
            format!("jet_http_srv_access_log(&({}), {})", arg(0), arg(1))
        }
        ("core.http.server", "request_id") => {
            format!("jet_http_srv_install_request_id(&({}))", arg(0))
        }
        // D-WS1=B: cleartext WebSocket client/server.
        ("core.ws", "connect") => format!("jet_ws_connect(&({}))", arg(0)),
        ("core.ws", "upgrade") => format!("jet_ws_upgrade(&({}))", arg(0)),
        // D-TIMEDEPTH1=A: civil-time constructors.
        ("core.time.date", "new") => format!("JetDate::new({}, {}, {})", arg(0), arg(1), arg(2)),
        ("core.time.date", "today") => format!("JetDate::today_utc()"),
        ("core.time.date", "parse") => format!("JetDate::parse(&({})).map_err(|e| e)", arg(0)),
        ("core.time.datetime", "from_timestamp") => {
            format!("JetDateTime::from_timestamp({})", arg(0))
        }
        ("core.time.datetime", "now") => format!("JetDateTime::now()"),
        _ => "/* unknown std call */".to_string(),
    }
}
