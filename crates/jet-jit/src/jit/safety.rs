use jet_codegen::Codegen::TIR::{
    self, JitProgram, JitSpawnCapture, TBuiltinOp, TCallArg, TCoreClosureKind, TEnumPayload, TExpr,
    TExprKind, TFunc, TFuncKind, THandleOp, THostCall, TIfCond, TJitSpawnBody, TJitSpawnLambda, TModuleCallForm, TOrFallback,
    ListSpreadPart, TStmt, TStrPart,
    TNumericOp,
};
use jet_foundation::AST::{BinOp, Pattern, Type, UnOp};
use std::collections::HashSet;

pub(crate) fn flatten_string(parts: &[TStrPart]) -> Option<String> {
    let mut out = String::new();
    for p in parts {
        match p {
            TStrPart::Lit(s) => out.push_str(s),
            TStrPart::Interp(_, _) => return None,
        }
    }
    Some(out)
}

fn resident_safe_string_parts(parts: &[TStrPart], callees: &HashSet<String>) -> bool {
    parts.iter().all(|p| match p {
        TStrPart::Lit(_) => true,
        TStrPart::Interp(e, _) => resident_safe_expr(e, callees),
    })
}

fn jit_scalar_type(ty: &Type) -> bool {
    jit_value_type(ty)
}

fn erase_runtime_qualifiers(mut ty: &Type) -> &Type {
    while let Type::Tagged { inner, .. } = ty {
        ty = inner;
    }
    ty
}

fn intish_ty(ty: &Type) -> bool {
    matches!(erase_runtime_qualifiers(ty), Type::Int | Type::IntN { .. })
}

/// `Signal/Derived/Computed.get()` — TIR often erases the Apply payload to
/// `Named("Unit")` inside closures. Overflow arithmetic still uses the Int
/// host path for Int signals (`n.get() * 2`).
fn reactive_get_intish(expr: &TExpr) -> bool {
    matches!(
        &expr.kind,
        TExprKind::HandleMethod {
            op: THandleOp::ReactiveGet,
            ..
        }
    )
}

fn jit_bag_raw_key_candidate(ty: &Type) -> bool {
    match ty {
        Type::Tagged { inner, .. } => jit_bag_raw_key_candidate(inner),
        Type::Int | Type::IntN { .. } | Type::Bool | Type::Char | Type::Named(_) => true,
        _ => false,
    }
}

pub(crate) fn jit_list_int_type(ty: &Type) -> bool {
    // IntN (U8/…) shares the i64 list ABI — bytes / write_at / random.bytes.
    matches!(
        ty,
        Type::List(inner) if matches!(inner.as_ref(), Type::Int | Type::IntN { .. })
    ) || matches!(
        ty,
        Type::FixedList { elem, .. } if matches!(elem.as_ref(), Type::Int | Type::IntN { .. })
    )
}

pub(crate) fn jit_list_float_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::List(inner) if matches!(inner.as_ref(), Type::Float | Type::Float32)
    ) || matches!(
        ty,
        Type::FixedList { elem, .. } if matches!(elem.as_ref(), Type::Float | Type::Float32)
    )
}

pub(crate) fn jit_list_string_type(ty: &Type) -> bool {
    matches!(ty, Type::List(inner) if matches!(inner.as_ref(), Type::String))
        || matches!(ty, Type::FixedList { elem, .. } if matches!(elem.as_ref(), Type::String))
}

pub(crate) fn jit_list_native_type(ty: &Type) -> bool {
    jit_list_int_type(ty)
        || jit_list_float_type(ty)
        || jit_list_string_type(ty)
        || jit_list_option_type(ty)
}

/// `[T?]` for optional scalars (series missing counts, etc.).
pub(crate) fn jit_list_option_type(ty: &Type) -> bool {
    matches!(ty, Type::List(inner) if jit_optional_scalar_type(inner))
}

fn jit_list_intn_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::List(inner) | Type::FixedList { elem: inner, .. }
            if matches!(inner.as_ref(), Type::IntN { .. })
    )
}

/// `[[Int]]` / `Iter<[Int]>` — flat_map identity receivers in iter_adapters.
pub(crate) fn jit_list_of_int_list_type(ty: &Type) -> bool {
    matches!(ty, Type::List(inner) if jit_list_int_type(inner))
        || matches!(
            ty,
            Type::Apply { name, args }
                if name == jet_foundation::Syntax::TYPE_ITER
                    && args.len() == 1
                    && jit_list_int_type(&args[0])
        )
}

pub(crate) fn jit_list_iter_elem_type(ty: &Type) -> Option<Type> {
    match ty {
        Type::List(inner) | Type::FixedList { elem: inner, .. }
            if matches!(
                inner.as_ref(),
                Type::Int
                    | Type::IntN { .. }
                    | Type::Float
                    | Type::Float32
                    | Type::String
                    | Type::Char
            ) =>
        {
            Some(inner.as_ref().clone())
        }
        // Nested list rows (`[[String]]` CSV, etc.) — outer iterates i64 handles.
        Type::List(inner) | Type::FixedList { elem: inner, .. }
            if jit_list_native_type(inner) =>
        {
            Some(inner.as_ref().clone())
        }
        // `List<Map<String, V>>` (db query rows) — map handles are i64.
        Type::List(inner) | Type::FixedList { elem: inner, .. }
            if matches!(
                inner.as_ref(),
                Type::Map { key, .. } if matches!(key.as_ref(), Type::String)
            ) =>
        {
            Some(inner.as_ref().clone())
        }
        // JIT ABI: `Iter<T>` / `View<T>` / `ViewMut<T>` producers materialize list
        // handles — scalar elems share list for-in / join / len; record elems too.
        Type::Apply { name, args }
            if (name == jet_foundation::Syntax::TYPE_ITER
                || matches!(name.as_str(), "View" | "ViewMut"))
                && args.len() == 1
                && (matches!(
                    &args[0],
                    Type::Int | Type::Float | Type::String | Type::Char
                ) || record_type_key(&args[0]).is_some()) =>
        {
            Some(args[0].clone())
        }
        _ => None,
    }
}

/// Closure adapter receivers: scalar list/Iter/View elems, or user struct handles.
pub(crate) fn jit_closure_elem_type(ty: &Type) -> Option<Type> {
    if let Some(elem) = jit_list_iter_elem_type(ty) {
        return Some(elem);
    }
    match ty {
        Type::List(inner) | Type::FixedList { elem: inner, .. }
            if record_type_key(inner).is_some() =>
        {
            Some(inner.as_ref().clone())
        }
        Type::Apply { name, args }
            if (name == jet_foundation::Syntax::TYPE_ITER
                || matches!(name.as_str(), "View" | "ViewMut"))
                && args.len() == 1
                && record_type_key(&args[0]).is_some() =>
        {
            Some(args[0].clone())
        }
        _ => None,
    }
}

/// `[T?E]` / `Iter<T?E>` with JIT-representable payloads.
pub(crate) fn jit_result_list_elem(ty: &Type) -> Option<(Type, Type)> {
    let elem = match ty {
        Type::List(inner) | Type::FixedList { elem: inner, .. } => inner.as_ref(),
        Type::Apply { name, args }
            if name == jet_foundation::Syntax::TYPE_ITER && args.len() == 1 =>
        {
            &args[0]
        }
        _ => return None,
    };
    match elem {
        Type::Result { ok, err }
            if jit_result_payload_type(ok) && jit_result_payload_type(err) =>
        {
            Some((ok.as_ref().clone(), err.as_ref().clone()))
        }
        _ => None,
    }
}

pub(crate) fn jit_map_string_type(ty: &Type) -> bool {
    matches!(ty, Type::Map { key, .. } if matches!(key.as_ref(), Type::String))
}

/// `Map<Int, V>` with scalar/handle values — Int keys share the i64 map heap ABI.
pub(crate) fn jit_map_int_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Map { key, value, .. }
            if matches!(key.as_ref(), Type::Int)
                && (jit_value_type(value) || matches!(value.as_ref(), Type::String))
    )
}

fn jit_map_resident_type(ty: &Type) -> bool {
    (jit_map_string_type(ty) || jit_map_int_type(ty)) && !jit_map_intn_value_type(ty)
}

fn jit_map_intn_value_type(ty: &Type) -> bool {
    matches!(ty, Type::Map { value, .. } if matches!(value.as_ref(), Type::IntN { .. }))
}

pub(crate) fn jit_list_task_int_type(ty: &Type) -> bool {
    if let Type::List(inner) = ty {
        if let Type::Apply { name, args } = inner.as_ref() {
            return name == "Task" && args.len() == 1 && matches!(&args[0], Type::Int);
        }
    }
    false
}

pub(crate) fn jit_optional_scalar_type(ty: &Type) -> bool {
    // Option carrier ABI: IntN uses result-arena handles; other scalars and
    // named enums use the legacy packed carrier. Nested Option stays out to avoid
    // recursive jit_value_type → compound loops.
    matches!(
        ty,
        Type::Option(inner)
            if jit_scalar_type(inner)
                || matches!(
                    inner.as_ref(),
                    Type::Named(_) | Type::String | Type::Tuple(_) | Type::List(_)
                )
    )
}

pub(crate) fn user_type_name(ty: &Type) -> Option<&str> {
    match ty {
        Type::Named(n) if n != "Unit" => Some(n.as_str()),
        Type::Apply { name, .. } => Some(name.as_str()),
        Type::Tagged { inner, .. } => user_type_name(inner),
        _ => None,
    }
}

pub(crate) fn record_type_key(ty: &Type) -> Option<String> {
    match ty {
        Type::Tuple(_) => Some(ty.name()),
        _ => user_type_name(ty).map(str::to_string),
    }
}

pub(crate) fn jit_struct_type(ty: &Type) -> bool {
    user_type_name(ty).is_some()
}

pub(crate) fn jit_list_record_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::List(elem) | Type::FixedList { elem, .. }
            if record_type_key(elem).is_some() || matches!(elem.as_ref(), Type::TraitObject(_))
    )
}

pub(crate) fn jit_tuple_type(ty: &Type) -> bool {
    // Named tuples lower to record handles (i64), same as structs — any field
    // shape that fits the record ABI (scalars, Option, String, nested records).
    matches!(
        ty,
        Type::Tuple(fields)
            if !fields.is_empty()
                && fields.iter().all(|(_, t)| {
                    matches!(
                        t.as_ref(),
                        Type::Int
                            | Type::IntN { .. }
                            | Type::Float
                            | Type::Float32
                            | Type::Bool
                            | Type::Char
                            | Type::String
                            | Type::Option(_)
                            | Type::Named(_)
                            | Type::Tuple(_)
                            | Type::List(_)
                    )
                })
    )
}

pub(crate) fn jit_enum_type(ty: &Type) -> bool {
    user_type_name(ty).is_some() || matches!(ty, Type::Union(_))
}

fn jit_compound_type(ty: &Type) -> bool {
    jit_list_native_type(ty)
        || jit_list_of_int_list_type(ty)
        || jit_list_task_int_type(ty)
        || jit_list_record_type(ty)
        || jit_map_string_type(ty)
        || jit_struct_type(ty)
        || jit_tuple_type(ty)
        || jit_enum_type(ty)
        || jit_optional_scalar_type(ty)
        || matches!(
            ty,
            Type::Apply { name, args }
                if args.len() == 1
                    && (matches!(
                        name.as_str(),
                        "Set"
                            | "Deque"
                            | "Pool"
                            | "Id"
                            | "Stream"
                            | "ExpiringValue"
                            | "ExpiringSecret"
                            | "SortedSet"
                            | "PriorityQueue"
                            | "Lru"
                            | "Ptr"
                    ) || (name == "Bag" && jit_bag_raw_key_candidate(&args[0])))
        )
        || matches!(ty, Type::Apply { name, args }
            if name == "View" && args.len() == 1 && jit_value_type(&args[0]))
        || matches!(ty, Type::Named(name) if matches!(name.as_str(), "BitSet" | "ByteBuffer"))
        || matches!(ty, Type::Shared(_))
}

fn jit_concurrency_elem(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if n == "Unit") || jit_scalar_type(ty)
}

pub(crate) fn jit_concurrency_type(ty: &Type) -> bool {
    let Type::Apply { name, args } = ty else {
        return false;
    };
    matches!(name.as_str(), "Task" | "Receiver" | "Sender")
        && args.len() == 1
        && jit_concurrency_elem(&args[0])
}

pub(crate) fn jit_value_type(ty: &Type) -> bool {
    if let Some((base, _)) = ty.quantity_parts() {
        return jit_value_type(base);
    }
    match ty {
        Type::Tagged { inner, .. } => jit_value_type(inner),
        Type::Named(n)
            if n.chars()
                .next()
                .is_some_and(|c| c.is_ascii_uppercase()) =>
        {
            // Opaque i64 handles: std/user structs and enums (Date, Cmd,
            // FileReader, WatchEvent, GameScene, …).
            true
        }
        Type::Apply { name, args }
            if matches!(
                name.as_str(),
                "Signal"
                    | "Derived"
                    | "Computed"
                    | "Effect"
                    | "Loadable"
                    | "Event"
                    | "AsyncEvent"
                    | "Hook"
                    | "DecisionHook"
                    | "Watch"
            ) && args.iter().all(jit_value_type) =>
        {
            true
        }
        Type::Int
        | Type::IntN { .. }
        | Type::Float
        | Type::Float32
        | Type::Bool
        | Type::String
        | Type::Char => true,
        Type::Tuple(fields) if fields.is_empty() => true,
        Type::Fn { params, ret, .. } => {
            params.iter().all(jit_value_type)
                && ret.as_ref().is_none_or(|ret| jit_value_type(ret))
        }
        Type::Union(members) => members.iter().all(jit_value_type),
        Type::TraitObject(traits) => !traits.is_empty(),
        Type::Result { ok, err } => {
            jit_result_payload_type(ok.as_ref()) && jit_result_payload_type(err.as_ref())
        }
        other if jit_concurrency_type(other) => true,
        other if jit_compound_type(other) => true,
        _ => false,
    }
}

pub(crate) fn jit_result_payload_type(ty: &Type) -> bool {
    matches!(ty, Type::Named(n) if n == "Unit" || n == "Void" || n == "Error")
        || jit_value_type(ty)
}

pub(crate) fn resident_safe_expr(expr: &TExpr, callees: &HashSet<String>) -> bool {
    match &expr.kind {
        TExprKind::Print(inner) => resident_safe_expr(inner, callees),
        TExprKind::Call { name, args } => {
            if !callees.contains(name) {
                return false;
            }
            args.iter().all(|a| resident_safe_call_arg(a, callees))
        }
        TExprKind::ModuleCall { form, args } => {
            let target = match form {
                TModuleCallForm::Qualified { rust_mod, rust_fn } => {
                    format!("{rust_mod}::{rust_fn}")
                }
                TModuleCallForm::InlineMangled { mangled } => mangled.clone(),
            };
            callees.contains(&target)
                && args
                    .iter()
                    .all(|arg| resident_safe_call_arg(arg, callees))
        }
        TExprKind::CoreCall {
            module,
            method,
            args,
            ..
        } => {
            if module == "jet.crypto" {
                return match (method.as_str(), args.as_slice()) {
                    ("__signing_generate" | "__x25519_generate", []) => true,
                    (
                        "__signing_public"
                        | "__x25519_public"
                        | "sha256"
                        | "sha512_bytes"
                        | "blake3_bytes"
                        | "__digest256_hex"
                        | "__digest256_bytes"
                        | "__signature_bytes"
                        | "__sealed_bytes"
                        | "__x25519_public_bytes"
                        | "__x25519_public_text"
                        | "__x25519_public_from_text"
                        | "__secret_from_text"
                        | "__vault_wrapped_from_bytes"
                        | "__vault_wrapped_bytes"
                        | "__vault_unlock_recipient"
                        | "__vault_unlock_passphrase"
                        | "password_hash",
                        [value],
                    ) => resident_safe_expr(value, callees),
                    ("sign" | "password_verify", [a, b]) => {
                        resident_safe_expr(a, callees) && resident_safe_expr(b, callees)
                    }
                    ("verify" | "seal" | "open" | "file_open", [a, b, c]) => {
                        resident_safe_expr(a, callees)
                            && resident_safe_expr(b, callees)
                            && resident_safe_expr(c, callees)
                    }
                    _ => false,
                };
            }
            if module == "core.crypto.expert" {
                return match (method.as_str(), args.as_slice()) {
                    ("secret_bytes", [value]) => resident_safe_expr(value, callees),
                    ("open_v1" | "x25519", args) if (2..=3).contains(&args.len()) => {
                        args.iter().all(|arg| resident_safe_expr(arg, callees))
                    }
                    ("aes256gcm_seal" | "aes256gcm_open" | "migrate_v1", args)
                        if args.len() == 4 =>
                    {
                        args.iter().all(|arg| resident_safe_expr(arg, callees))
                    }
                    _ => false,
                };
            }
            if module == "core.crypto.random" && method == "bytes" {
                return matches!(args.as_slice(), [arg] if resident_safe_expr(arg, callees));
            }
            if module == "core.auth" && matches!(method.as_str(), "verify_jwt" | "verify_paseto") {
                return (3..=7).contains(&args.len())
                    && args.iter().all(|arg| resident_safe_expr(arg, callees));
            }
            if module == "core.vault" || module == "core.vault.expert" {
                return !args.is_empty()
                    && args.iter().all(|arg| resident_safe_expr(arg, callees));
            }
            if module == "core.text" {
                return match method.as_str() {
                    "lower" | "upper" | "trim" | "scalar_count" | "byte_count" | "graphemes"
                    | "words" | "sentences" | "nfc" | "nfkc" | "nfd" | "nfkd"
                    | "display_width" | "is_alphabetic" | "is_numeric" | "char_indices"
                        if args.len() == 1 =>
                    {
                        matches!(&args[0].ty, Type::String) && resident_safe_expr(&args[0], callees)
                    }
                    "display_width" if args.len() == 2 => {
                        matches!(&args[0].ty, Type::String)
                            && matches!(&args[1].ty, Type::Named(n) if n == "TextWidth")
                            && resident_safe_expr(&args[0], callees)
                            && resident_safe_expr(&args[1], callees)
                    }
                    "caseless_eq" | "starts_any" if args.len() == 2 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    "pad_start" | "center" if args.len() == 3 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    _ => false,
                };
            }
            if module.starts_with("core.sketch.") {
                return match (module.as_str(), method.as_str(), args.len()) {
                    ("core.sketch.hll" | "core.sketch.tdigest" | "core.sketch.cms", "new", 0) => {
                        true
                    }
                    ("core.sketch.reservoir", "new", 1) => {
                        resident_safe_expr(&args[0], callees)
                    }
                    _ => false,
                };
            }
            if module == "core.args" && method == "spec" {
                return args.is_empty();
            }
            if module == "core.text.unicode" {
                return matches!(args.as_slice(), [arg]
                    if matches!(
                        method.as_str(),
                        "scalar_count"
                            | "byte_count"
                            | "is_ascii"
                            | "lower"
                            | "upper"
                            | "scalars"
                    ) && matches!(&arg.ty, Type::String)
                        && resident_safe_expr(arg, callees));
            }
            if module == "core.io" {
                return match method.as_str() {
                    "args" => args.is_empty(),
                    "confirm" | "input_secret" => {
                        args.len() == 1 && resident_safe_expr(&args[0], callees)
                    }
                    "choose" => {
                        args.len() == 2
                            && args.iter().all(|arg| resident_safe_expr(arg, callees))
                    }
                    _ => false,
                };
            }
            if module == "core.reflect" && method == "of" {
                return args.len() == 1 && resident_safe_expr(&args[0], callees);
            }
            if module == "core.testing" {
                return match method.as_str() {
                    "temp_dir" | "fake_clock" | "fake_rng" if args.len() == 1 => {
                        resident_safe_expr(&args[0], callees)
                    }
                    "snap" if args.len() == 2 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    _ => false,
                };
            }
            if module == "core.data" {
                return match method.as_str() {
                    "status" if args.is_empty() => true,
                    "csv" | "json" | "count" | "mean" | "sum" | "min" | "max" | "median"
                    | "variance" | "stddev" | "describe" | "bar_text" | "bar_svg"
                    | "require_bridge" | "table" | "rows" | "schema" | "series"
                    | "missing_count" | "lazy" | "collect" | "plan" | "values"
                        if args.len() == 1 =>
                    {
                        resident_safe_expr(&args[0], callees)
                    }
                    "quantile" | "filter" | "sort_by" | "rolling_mean" if args.len() == 2 => {
                        // filter/sort_by carry lambdas — still resident-safe when
                        // the list + callable lower.
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    "group_count" if args.len() == 2 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    "group_sum" | "group_mean" if args.len() == 3 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    "inner_join" | "left_join" if args.len() == 4 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    "pivot_sum" if args.len() == 4 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    "lazy_filter" | "lazy_sort_by" if args.len() == 2 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    "csv_reader" if args.len() == 2 => {
                        args.iter().all(|a| resident_safe_expr(a, callees))
                    }
                    _ => false,
                };
            }
            if module == "core.tasks" && method == "channel" {
                return args.len() <= 1 && args.iter().all(|a| resident_safe_expr(a, callees));
            }
            if module == "core.tasks" && matches!(method.as_str(), "after" | "interval") {
                return !args.is_empty()
                    && args.len() <= 2
                    && args.iter().all(|a| {
                        matches!(&a.ty, Type::Int) && resident_safe_expr(a, callees)
                    });
            }
            if module == "core.time" && matches!(method.as_str(), "now" | "sleep") {
                return match (method.as_str(), args.len()) {
                    ("now", 0) => true,
                    ("sleep", 1) => {
                        matches!(&args[0].ty, Type::Int) && resident_safe_expr(&args[0], callees)
                    }
                    _ => false,
                };
            }
            if module == "core.game" && method == "run" {
                return !args.is_empty()
                    && args.len() <= 3
                    && args.iter().all(|a| resident_safe_expr(a, callees));
            }
            if module == "core.raylib" {
                return match method.as_str() {
                    "window_open" => args.len() == 3,
                    "color" => args.len() == 4,
                    "set_target_fps" | "key_down" | "begin_drawing" | "clear_background"
                    | "close_window" => args.len() == 1,
                    "draw_rectangle" | "draw_text" => args.len() == 5,
                    "end_drawing" => args.is_empty(),
                    _ => false,
                } && args.iter().all(|a| resident_safe_expr(a, callees));
            }
            if module == "core.random"
                && method == "weighted_pick"
                && args.first().is_some_and(|items| jit_list_intn_type(&items.ty))
            {
                return false;
            }
            // Other core modules retain their existing resident coverage. core.text
            // alone is exact above: unsupported methods cannot fail into fallback.
            resident_safe_expr_list(args, callees)
        }
        TExprKind::CoreClosureCall { kind } => match kind {
            TCoreClosureKind::Spawn { .. } => true,
            TCoreClosureKind::ReactiveDerived { executable, .. }
            | TCoreClosureKind::ReactiveEffect { executable, .. }
            | TCoreClosureKind::UiReactiveRender { executable, .. } => {
                match &executable.executable {
                    TIR::TLambdaBody::Expr(e) => {
                        if resident_safe_expr(e, callees) {
                            true
                        } else {
                            // Keep false; tag surfaces via Let init detail when needed.
                            let _ = expr_kind_tag(e);
                            false
                        }
                    }
                    TIR::TLambdaBody::Block(stmts) => {
                        stmts.iter().all(|s| resident_safe_stmt(s, callees))
                    }
                }
            }
            TCoreClosureKind::Guard { executable, .. }
            | TCoreClosureKind::OnCommit { executable, .. }
            | TCoreClosureKind::OnRollback { executable, .. } => {
                match &executable.executable {
                    TIR::TLambdaBody::Expr(e) => resident_safe_expr(e, callees),
                    TIR::TLambdaBody::Block(stmts) => {
                        stmts.iter().all(|s| resident_safe_stmt(s, callees))
                    }
                }
            }
            _ => false,
        },
        TExprKind::HandleMethod { recv, op, args } => {
            let args_ok = match op {
                // Frame / click callbacks are registered via JIT spawn-sites;
                // the TIR lambda arg is not the resident-lowered body.
                THandleOp::GameSceneOnFrame => args.len() <= 1,
                THandleOp::UiBackendMethod { method } if method == "on_click" => {
                    args.len() == 2
                        && resident_safe_expr(&args[0], callees)
                        && matches!(&args[1].kind, TExprKind::Lambda(_))
                }
                _ => args.iter().all(|a| resident_safe_expr(a, callees)),
            };
            resident_safe_expr(recv, callees)
                && args_ok
                && resident_safe_handle_op(op, recv, args)
        }
        TExprKind::OrFallback {
            value,
            fallback,
            is_option,
        } => {
            if *is_option {
                resident_safe_expr(value, callees)
                    && matches!(
                        fallback,
                        TOrFallback::Value(_)
                            | TOrFallback::Panic { .. }
                            | TOrFallback::Break
                            | TOrFallback::Continue
                            | TOrFallback::BreakLabel(_)
                            | TOrFallback::ContinueLabel(_)
                    )
            } else {
                resident_safe_expr(value, callees)
                    && match fallback {
                        TOrFallback::Value(e) => resident_safe_expr(e, callees),
                        TOrFallback::Return(None) => true,
                        TOrFallback::Return(Some(e)) => resident_safe_expr(e, callees),
                        TOrFallback::Panic { .. }
                        | TOrFallback::Break
                        | TOrFallback::Continue
                        | TOrFallback::BreakLabel(_)
                        | TOrFallback::ContinueLabel(_) => true,
                    }
            }
        }
        TExprKind::MapLit(entries) => {
            jit_map_resident_type(&expr.ty)
                && entries.iter().all(|(k, v)| {
                    let key_ok = if jit_map_int_type(&expr.ty) {
                        matches!(&k.ty, Type::Int)
                    } else {
                        matches!(&k.ty, Type::String)
                    };
                    key_ok
                        && jit_value_type(&v.ty)
                        && resident_safe_expr(k, callees)
                        && resident_safe_expr(v, callees)
                })
        }
        TExprKind::ListLit(elems) => {
            (jit_list_native_type(&expr.ty)
                && elems.iter().all(|e| {
                    (matches!(
                        &e.ty,
                        Type::Int
                            | Type::IntN { .. }
                            | Type::Float
                            | Type::String
                            | Type::Char
                    ) || jit_optional_scalar_type(&e.ty))
                        && resident_safe_expr(e, callees)
                }))
                || (jit_list_of_int_list_type(&expr.ty)
                    && elems.iter().all(|e| resident_safe_expr(e, callees)))
                || (matches!(
                    &expr.ty,
                    Type::List(inner) if jit_list_native_type(inner)
                        && matches!(inner.as_ref(), Type::List(elem) if matches!(elem.as_ref(), Type::String))
                ) && elems.iter().all(|e| resident_safe_expr(e, callees)))
                || (jit_list_task_int_type(&expr.ty)
                    && elems.iter().all(|e| resident_safe_expr(e, callees)))
                || (jit_list_record_type(&expr.ty)
                    && elems.iter().all(|e| resident_safe_expr(e, callees)))
                || (matches!(
                    &expr.ty,
                    Type::List(elem) | Type::FixedList { elem, .. }
                        if matches!(elem.as_ref(), Type::Named(_) | Type::Union(_))
                ) && elems.iter().all(|e| resident_safe_expr(e, callees)))
        }
        TExprKind::ListSpread { parts } => {
            matches!(&expr.ty, Type::List(_) | Type::FixedList { .. })
                && parts.iter().all(|part| match part {
                    ListSpreadPart::Elem(elem) => resident_safe_expr(elem, callees),
                    ListSpreadPart::Spread(list) => {
                        (jit_list_native_type(&list.ty) || jit_list_record_type(&list.ty))
                            && resident_safe_expr(list, callees)
                    }
                })
        }
        TExprKind::Index {
            base,
            index,
            is_map,
            ..
        } => {
            if *is_map {
                let key_ok = if jit_map_int_type(&base.ty) {
                    matches!(&index.ty, Type::Int)
                } else {
                    jit_map_string_type(&base.ty) && matches!(&index.ty, Type::String)
                };
                key_ok
                    && !jit_map_intn_value_type(&base.ty)
                    && resident_safe_expr(base, callees)
                    && resident_safe_expr(index, callees)
            } else {
                (jit_list_native_type(&base.ty)
                    || jit_list_record_type(&base.ty)
                    || jit_list_iter_elem_type(&base.ty).is_some()
                    || jit_closure_elem_type(&base.ty).is_some())
                    && matches!(&index.ty, Type::Int)
                    && resident_safe_expr(base, callees)
                    && resident_safe_expr(index, callees)
            }
        }
        TExprKind::Slice {
            base,
            start,
            end,
            range,
            ..
        } => {
            (jit_list_native_type(&base.ty) || jit_list_record_type(&base.ty))
                && resident_safe_expr(base, callees)
                && range.as_deref().map_or_else(
                    || {
                        matches!(&start.ty, Type::Int)
                            && matches!(&end.ty, Type::Int)
                            && resident_safe_expr(start, callees)
                            && resident_safe_expr(end, callees)
                    },
                    |range| {
                        matches!(&range.ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_RANGE)
                            && resident_safe_expr(range, callees)
                    },
                )
        }
        TExprKind::BuiltinMethod { recv, op, args } => {
            resident_safe_builtin_op(op, recv, args, callees)
        }
        TExprKind::NumericMethod { recv, op } => {
            resident_safe_expr(recv, callees)
                && match op {
                    TNumericOp::Predicate(name) => matches!(&recv.ty, Type::Float)
                        && matches!(name.as_str(), "is_nan" | "is_infinite" | "is_finite"),
                    TNumericOp::BitCount { method: name, width } => {
                        (matches!(&recv.ty, Type::IntN { bits, .. } if u32::from(*bits) == *width)
                            || (*width == 64 && matches!(&recv.ty, Type::Int)))
                            && matches!(
                                name.as_str(),
                                "count_ones"
                                    | "count_zeros"
                                    | "leading_zeros"
                                    | "trailing_zeros"
                            )
                    }
                    TNumericOp::ToShow => {
                        matches!(&recv.ty, Type::Int | Type::IntN { .. } | Type::Float)
                    },
                    TNumericOp::CastAs { dst_rust } => {
                        recv.ty.is_numeric()
                            && matches!(dst_rust.as_str(), "i8" | "i16" | "i32" | "i64" | "u8" | "u16" | "u32" | "u64" | "f32" | "f64")
                    }
                    TNumericOp::FloatToInt { .. } | TNumericOp::FloatNarrow { .. } => recv.ty.is_float(),
                    TNumericOp::TryFrom { .. } => recv.ty.is_integer(),
                    TNumericOp::Origin(_) => true,
                }
        }
        TExprKind::DistinctConvert { arg, .. } | TExprKind::DistinctRaw(arg) => {
            resident_safe_expr(arg, callees)
        }
        TExprKind::UnitConvert { arg, .. } => resident_safe_expr(arg, callees),
        TExprKind::PreciseBuiltin {
            type_name,
            func,
            args,
        } => {
            ((type_name == "BigInt"
                && matches!(
                    (func.as_str(), args.len()),
                    ("from_int" | "from_str" | "to_string", 1)
                        | ("add" | "sub" | "mul", 2)
                ))
                || (type_name == "Decimal"
                    && matches!(
                        (func.as_str(), args.len()),
                        ("from_str" | "to_string", 1) | ("add" | "sub" | "mul", 2)
                    )))
                && args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        TExprKind::StructLit { fields, .. } => {
            (jit_struct_type(&expr.ty)
                || matches!(&expr.ty, Type::TraitObject(_))
                || matches!(&expr.ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_RANGE))
                && fields
                    .iter()
                    .all(|(_, v, _)| resident_safe_expr(v, callees))
        }
        TExprKind::DataEntriesToMap(local) => {
            local.generated && !local.deref && local.name.starts_with("__jet_obj")
        }
        TExprKind::TupleLit { fields, .. } => {
            jit_tuple_type(&expr.ty) && resident_safe_tuple_fields(fields, callees)
        }
        TExprKind::Field { recv, .. } => match &recv.kind {
            TExprKind::Index {
                base,
                index,
                is_map,
                ..
            } => {
                !is_map
                    && (jit_list_record_type(&base.ty)
                        || matches!(
                            &base.ty,
                            Type::Apply { name, args }
                                if name == "View"
                                    && args.len() == 1
                                    && record_type_key(&args[0]).is_some()
                        ))
                    && matches!(&index.ty, Type::Int)
                    && resident_safe_expr(base, callees)
                    && resident_safe_expr(index, callees)
            }
            _ => resident_safe_expr(recv, callees),
        },
        TExprKind::MethodCall { recv, args, .. } => {
            resident_safe_expr(recv, callees)
                && args.iter().all(|a| resident_safe_call_arg(a, callees))
        }
        TExprKind::FnFieldCall { recv, args, .. } => {
            resident_safe_expr(recv, callees)
                && args.iter().all(|arg| resident_safe_call_arg(arg, callees))
        }
        TExprKind::StaticCall { args, .. } => {
            args.iter().all(|a| resident_safe_call_arg(a, callees))
        }
        TExprKind::AllocNew { .. } => true,
        TExprKind::PoolSlot { pool, id, .. } => {
            resident_safe_expr(pool, callees) && resident_safe_expr(id, callees)
        }
        TExprKind::IndexHook {
            base, index, ..
        } => {
            resident_safe_expr(base, callees) && resident_safe_expr(index, callees)
        }
        TExprKind::Drop(inner)
        | TExprKind::MaterializeView(inner)
        | TExprKind::Deref(inner)
        | TExprKind::RawOf(inner) => resident_safe_expr(inner, callees),
        TExprKind::EnumLit { payload, .. } => {
            jit_enum_type(&expr.ty) && resident_safe_enum_payload(payload, callees)
        }
        TExprKind::Present(inner) | TExprKind::Ok(inner) | TExprKind::Err(inner) => {
            resident_safe_expr(inner, callees)
        }
        TExprKind::Try { inner, convert, .. } => {
            matches!(
                convert,
                TIR::TTryConvert::None | TIR::TTryConvert::Typed(_)
            ) && resident_safe_expr(inner, callees)
        }
        TExprKind::Absent => true,
        TExprKind::DistinctCtor { arg, base, .. } => {
            jit_value_type(base) && resident_safe_expr(arg, callees)
        }
        // Moved file/codec handles (`output :: files.create(...)`, `^output` take).
        TExprKind::ResourceNew(inner) => resident_safe_expr(inner, callees),
        TExprKind::ResourceTake(_) => jit_value_type(&expr.ty),
        TExprKind::Close(_) => true,
        _ if !jit_value_type(&expr.ty) => false,
        TExprKind::IntLit(_, _)
        | TExprKind::FloatLit(_)
        | TExprKind::BoolLit(_)
        | TExprKind::CharLit(_) => true,
        TExprKind::CtLit(
            jet_foundation::AST::CtValue::Int(_)
            | jet_foundation::AST::CtValue::Bool(_)
            | jet_foundation::AST::CtValue::Char(_)
            | jet_foundation::AST::CtValue::Str(_)
            | jet_foundation::AST::CtValue::List(_),
        )
        | TExprKind::ConstRef(_) => true,
        TExprKind::StrLit(parts) => resident_safe_string_parts(parts, callees),
        TExprKind::Local(_) => true,
        TExprKind::Unary { op, operand } => {
            matches!(op, UnOp::Neg | UnOp::Not) && resident_safe_expr(operand, callees)
        }
        TExprKind::IncDec { ty, .. } => matches!(ty, Type::Int),
        TExprKind::Binary {
            op,
            overflow,
            lhs,
            rhs,
            ..
        } => {
            if matches!(op, BinOp::And | BinOp::Or) {
                return matches!(&lhs.ty, Type::Bool)
                    && matches!(&rhs.ty, Type::Bool)
                    && resident_safe_expr(lhs, callees)
                    && resident_safe_expr(rhs, callees);
            }
            if *overflow {
                let lhs_int = intish_ty(&lhs.ty) || reactive_get_intish(lhs);
                let rhs_int = intish_ty(&rhs.ty) || reactive_get_intish(rhs);
                if !lhs_int || !rhs_int {
                    return false;
                }
            }
            resident_safe_expr(lhs, callees) && resident_safe_expr(rhs, callees)
        }
        TExprKind::CompareChain { operands, ops, hooks } => {
            operands.len() == ops.len() + 1
                && hooks.len() == ops.len()
                && operands.iter().all(|e| resident_safe_expr(e, callees))
                && ops.iter().enumerate().all(|(i, _)| {
                    hooks[i]
                        || matches!(
                            &operands[i].ty,
                            Type::Int | Type::IntN { .. } | Type::Float
                        )
                })
                && ops
                    .iter()
                    .all(|op| matches!(op, BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge))
        }
        TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
        } => {
            matches!(cond.as_ref(), TIfCond::Plain(e) if matches!(&e.ty, Type::Bool) && resident_safe_expr(e, callees))
                && then_body.iter().all(|s| resident_safe_stmt(s, callees))
                && resident_safe_expr(then_value, callees)
                && else_body.iter().all(|s| resident_safe_stmt(s, callees))
                && resident_safe_expr(else_value, callees)
        }
        TExprKind::Clone(inner) => resident_safe_expr(inner, callees),
        TExprKind::Borrow { place, .. } => resident_safe_expr(place, callees),
        TExprKind::InlineBlock(stmts) => {
            (jit_value_type(&expr.ty)
                || jit_list_native_type(&expr.ty)
                || jit_struct_type(&expr.ty)
                || jit_tuple_type(&expr.ty))
                && stmts.iter().all(|stmt| resident_safe_stmt(stmt, callees))
        }
        TExprKind::Lambda(lam) => {
            lam.param_types.iter().all(jit_value_type)
                && lam.ret.as_ref().is_none_or(jit_value_type)
                && match &lam.executable {
                    TIR::TLambdaBody::Expr(expr) => resident_safe_expr(expr, callees),
                    TIR::TLambdaBody::Block(stmts) => {
                        stmts.iter().all(|stmt| resident_safe_stmt(stmt, callees))
                    }
                }
        }
        TExprKind::FnValue { kind } => match kind {
            TIR::TFnValueKind::NamedFn {
                name: Some(name), ..
            } => callees.contains(name),
            TIR::TFnValueKind::NamedFn { name: None, .. } => false,
            TIR::TFnValueKind::Call { callee, args } => {
                resident_safe_expr(callee, callees)
                    && args.iter().all(|arg| resident_safe_call_arg(arg, callees))
            }
        },
        TExprKind::PatternMatches { subj, .. } => resident_safe_expr(subj, callees),
        TExprKind::OptionLift2 { f, a, b } => {
            matches!(
                &f.kind,
                TExprKind::Lambda(lam)
                    if lam.prep.is_empty()
                        && lam.source_params.len() == 2
                        && matches!(
                            &lam.executable,
                            TIR::TLambdaBody::Expr(e) if resident_safe_expr(e, callees)
                        )
            ) && resident_safe_expr(a, callees)
                && resident_safe_expr(b, callees)
                && matches!(&expr.ty, Type::Option(inner) if jit_scalar_type(inner) || matches!(inner.as_ref(), Type::Named(_)|Type::Tuple(_)))
        }
        TExprKind::OverflowOpt {
            prefix,
            op,
            lhs,
            rhs,
        } => {
            matches!(prefix.as_str(), "wrapping" | "saturating" | "checked")
                && matches!(*op, "add" | "sub" | "mul" | "div")
                && matches!(&lhs.ty, Type::Int | Type::IntN { .. })
                && matches!(&rhs.ty, Type::Int | Type::IntN { .. })
                && resident_safe_expr(lhs, callees)
                && resident_safe_expr(rhs, callees)
        }
        TExprKind::ClosureMethod { recv, op, args } => {
            resident_safe_closure_method(recv, op, args, callees)
        }
        TExprKind::TaskGroupAll { tasks } => {
            jit_list_int_type(&expr.ty) && resident_safe_task_list_expr(tasks, callees)
        }
        TExprKind::TaskGroupRace { tasks } | TExprKind::TaskGroupAny { tasks } => {
            matches!(&expr.ty, Type::Int) && resident_safe_task_list_expr(tasks, callees)
        }
        TExprKind::SelectStart => true,
        TExprKind::SelectRecv { builder, channel } => {
            resident_safe_expr(builder, callees)
                && jit_concurrency_type(&channel.ty)
                && resident_safe_expr(channel, callees)
        }
        TExprKind::SelectAfter {
            builder,
            millis,
            value,
        } => {
            resident_safe_expr(builder, callees)
                && matches!(&millis.ty, Type::Int)
                && resident_safe_expr(millis, callees)
                && value.as_ref().map_or(true, |v| {
                    matches!(&v.ty, Type::Int) && resident_safe_expr(v, callees)
                })
        }
        TExprKind::SelectRead { builder, .. } => resident_safe_expr(builder, callees),
        TExprKind::SelectWait { builder } => {
            jit_value_type(&expr.ty) && resident_safe_select_wait(builder, callees)
        }
        TExprKind::AmbientInput { prompt } => {
            prompt.as_ref().is_none_or(|p| resident_safe_expr(p, callees))
        }
        // D-OPTGC1: GcRead/GcEdit lower to Variable load/store of the payload
        // handle — same value snapshots AOT clones out of AutomaticRoot.
        TExprKind::HostCall(host) => match host.as_ref() {
            THostCall::GcRead { .. } => true,
            THostCall::GcEdit {
                edit,
                index_temp,
                ..
            } => {
                resident_safe_expr(edit, callees)
                    && index_temp
                        .as_ref()
                        .is_none_or(|(_, e)| resident_safe_expr(e, callees))
            }
            THostCall::EnvSet { name, value, .. } => {
                resident_safe_expr(name, callees) && resident_safe_expr(value, callees)
            }
            THostCall::NumericBounds { ty, member } => {
                (jet_codegen::Comptime::MathLayout::integer_type_layout(ty).is_some()
                    && matches!(member.as_str(), "MIN" | "MAX"))
                    || (matches!(ty, Type::Float)
                        && matches!(member.as_str(), "INFINITY" | "NAN" | "EPSILON"))
            }
            THostCall::FixedListIndex { base, index } => {
                resident_safe_expr(base, callees)
                    && resident_safe_expr(index, callees)
                    && matches!(&index.ty, Type::Int | Type::IntN { .. } | Type::Named(_))
            }
            THostCall::TupleIndex { base, .. } => resident_safe_expr(base, callees),
            THostCall::SwitchSubjectField { .. } => true,
            THostCall::StrMatchScan { .. } | THostCall::BinMatchScan { .. } => true,
            THostCall::Method {
                recv,
                method,
                args,
            } => {
                !matches!(method.as_str(), "guard_read" | "guard_edit")
                    && resident_safe_expr(recv, callees)
                    && args.iter().all(|arg| resident_safe_expr(arg, callees))
            }
            THostCall::Helper { helper, args }
                if helper.ends_with("jet_std_clock_new") =>
            {
                args.len() == 1
                    && args.iter().all(|arg| match arg {
                        TIR::THostArg::Expr(expr) | TIR::THostArg::Borrow(expr) => {
                            resident_safe_expr(expr, callees)
                        }
                        TIR::THostArg::Lambda(_) => false,
                    })
            }
            THostCall::Helper { helper, args } if helper.ends_with("jet_context") => {
                args.len() == 2
                    && args.iter().all(|arg| match arg {
                        TIR::THostArg::Expr(expr) | TIR::THostArg::Borrow(expr) => {
                            resident_safe_expr(expr, callees)
                        }
                        TIR::THostArg::Lambda(_) => false,
                    })
            }
            THostCall::ExpiringValueNew {
                value,
                duration,
                clock,
            }
            | THostCall::ExpiringSecretNew {
                value,
                duration,
                clock,
                ..
            } => {
                resident_safe_expr(value, callees)
                    && resident_safe_expr(duration, callees)
                    && resident_safe_expr(clock, callees)
            }
            THostCall::YieldSend { value } => resident_safe_expr(value, callees),
            THostCall::TypedText { arg, .. } => resident_safe_expr(arg, callees),
            THostCall::TypedTextInterp { holes, .. } => {
                holes.iter().all(|h| resident_safe_expr(h, callees))
            }
            _ => false,
        },
        // Codable encode lowers to `JSONLit` (DataTree/JSON foreign enum).
        TExprKind::JSONLit { arg, .. } => match arg.as_ref() {
            None => true,
            Some(boxed) => {
                let (inner, _) = boxed.as_ref();
                enum_payload_value_type(&inner.ty) && resident_safe_expr(inner, callees)
            }
        },
        // D-DBDRIVER1: `DBValue.Int(n)` / `.Text(s)` / … — foreign prelude enum.
        TExprKind::DBValueLit { arg, .. } => match arg.as_ref() {
            None => true,
            Some(boxed) => {
                let (inner, _) = boxed.as_ref();
                enum_payload_value_type(&inner.ty) && resident_safe_expr(inner, callees)
            }
        },
        TExprKind::RequireStop { kind, .. } => match kind {
            TIR::TRequireKind::Require { cond, msg, .. } => {
                resident_safe_expr(cond, callees)
                    && msg
                        .as_ref()
                        .is_none_or(|m| resident_safe_expr(m, callees))
            }
            TIR::TRequireKind::RequireEq { left, right } => {
                resident_safe_expr(left, callees) && resident_safe_expr(right, callees)
            }
            TIR::TRequireKind::Panic { msg } => resident_safe_expr(msg, callees),
        },
        TExprKind::Todo { .. } => true,
        TExprKind::Uninit => matches!(&expr.ty, Type::FixedList { .. })
            && jit_list_native_type(&expr.ty),
        TExprKind::LayoutCompare { lhs, rhs, .. } => {
            resident_safe_expr(lhs, callees) && resident_safe_expr(rhs, callees)
        }
        TExprKind::LayoutLit { inner } => resident_safe_expr(inner, callees),
        TExprKind::MathBuiltin { args, .. } => {
            args.iter().all(|a| resident_safe_expr(a, callees))
        }
        TExprKind::MathLaneIndex { base, index, .. } => {
            resident_safe_expr(base, callees) && resident_safe_expr(index, callees)
        }
        TExprKind::MathSwizzleRead { recv, .. } => resident_safe_expr(recv, callees),
        TExprKind::PtrFromAddr { addr, .. } => resident_safe_expr(addr, callees),
        TExprKind::ExternCall { args, .. } => {
            args.iter().all(|a| resident_safe_expr(&a.value, callees))
        }

        _ => false,
    }
}

fn resident_safe_call_arg(arg: &TCallArg, callees: &HashSet<String>) -> bool {
    // Arc/fn-coerce still unsupported. borrow/mut_borrow pass heap handles;
    // clone lowers through `lower_clone` for the types below.
    if arg.arc_clone {
        return false;
    }
    let ty = &arg.value.ty;
    let handle_pass = jit_value_type(ty)
        || jit_struct_type(ty)
        || jit_tuple_type(ty)
        || matches!(
            ty,
            Type::String
                | Type::List(_)
                | Type::FixedList { .. }
                | Type::Option(_)
                | Type::Map { .. }
        );
    if (arg.borrow || arg.mut_borrow) && !handle_pass {
        return false;
    }
    if arg.clone {
        let clone_ok = matches!(
            ty,
            Type::Int
                | Type::Float
                | Type::Bool
                | Type::Char
                | Type::String
                | Type::Option(_)
                | Type::IntN { .. }
                | Type::Float32
        ) || jit_struct_type(ty)
            || jit_compound_type(ty)
            || jit_tuple_type(ty)
            || jit_list_native_type(ty)
            || jit_list_record_type(ty)
            || jit_map_string_type(ty);
        if !clone_ok {
            return false;
        }
    }
    if arg.widen_to_vec && !(jit_list_native_type(ty) || matches!(ty, Type::FixedList { .. })) {
        return false;
    }
    resident_safe_expr(&arg.value, callees)
}

fn resident_safe_unary_lambda(args: &[TExpr], callees: &HashSet<String>) -> bool {
    args.len() == 1
        && matches!(
            &args[0].kind,
            TExprKind::Lambda(lam)
                if lam.prep.is_empty()
                    && lam.source_params.len() == 1
                    && matches!(
                        &lam.executable,
                        TIR::TLambdaBody::Expr(e) if resident_safe_expr(e, callees)
                    )
        )
}

fn resident_safe_each_lambda(args: &[TExpr], callees: &HashSet<String>) -> bool {
    args.len() == 1
        && matches!(
            &args[0].kind,
            TExprKind::Lambda(lam)
                if lam.prep.is_empty()
                    && lam.source_params.len() == 1
                    && match &lam.executable {
                        TIR::TLambdaBody::Expr(e) => resident_safe_expr(e, callees),
                        TIR::TLambdaBody::Block(stmts) => stmts.iter().all(|stmt| {
                            matches!(stmt, TStmt::Let { .. } | TStmt::Assign { .. } | TStmt::ExprStmt(_))
                                && resident_safe_stmt(stmt, callees)
                        }),
                    }
        )
}

fn resident_safe_fold_lambda(args: &[TExpr], callees: &HashSet<String>) -> bool {
    args.len() == 2
        && matches!(&args[0].ty, Type::Int)
        && resident_safe_expr(&args[0], callees)
        && matches!(
            &args[1].kind,
            TExprKind::Lambda(lam)
                if lam.prep.is_empty()
                    && lam.source_params.len() == 2
                    && matches!(
                        &lam.executable,
                        TIR::TLambdaBody::Expr(e) if resident_safe_expr(e, callees)
                    )
        )
}

/// `para_fold(seed, step, merge)` — seed is `() => U`, step/merge are binary.
fn resident_safe_para_fold_lambdas(args: &[TExpr], callees: &HashSet<String>) -> bool {
    if args.len() != 3 {
        return false;
    }
    let seed_ok = matches!(
        &args[0].kind,
        TExprKind::Lambda(lam)
            if lam.prep.is_empty()
                && lam.source_params.is_empty()
                && matches!(
                    &lam.executable,
                    TIR::TLambdaBody::Expr(e)
                        if matches!(&e.ty, Type::Int) && resident_safe_expr(e, callees)
                )
    );
    let bin_ok = |arg: &TExpr| {
        matches!(
            &arg.kind,
            TExprKind::Lambda(lam)
                if lam.prep.is_empty()
                    && lam.source_params.len() == 2
                    && matches!(
                        &lam.executable,
                        TIR::TLambdaBody::Expr(e) if resident_safe_expr(e, callees)
                    )
        )
    };
    seed_ok && bin_ok(&args[1]) && bin_ok(&args[2])
}

fn resident_safe_closure_method(
    recv: &TExpr,
    op: &TIR::TClosureOp,
    args: &[TExpr],
    callees: &HashSet<String>,
) -> bool {
    if !resident_safe_expr(recv, callees) {
        return false;
    }
    match op {
        TIR::TClosureOp::Map | TIR::TClosureOp::MapMut | TIR::TClosureOp::ViewMap => {
            jit_closure_elem_type(&recv.ty).is_some_and(|elem| {
                matches!(elem, Type::Int | Type::String | Type::Named(_))
            }) && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::ParaMap => {
            jit_closure_elem_type(&recv.ty).is_some_and(|elem| {
                matches!(elem, Type::Int | Type::String | Type::Named(_))
            }) && resident_safe_unary_lambda(args, callees)
        }
        // D-HOLE1: Option.map — packed Option ABI; unary lambda over the payload.
        TIR::TClosureOp::OptionMap => {
            matches!(
                &recv.ty,
                Type::Option(inner)
                    if jit_scalar_type(inner)
                        || matches!(inner.as_ref(), Type::Named(_) | Type::Tuple(_))
            ) && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::Filter | TIR::TClosureOp::ParaFilter => {
            jit_closure_elem_type(&recv.ty).is_some_and(|elem| {
                matches!(elem, Type::Int | Type::String | Type::Named(_))
            }) && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::ParaPartition { .. } => {
            jit_closure_elem_type(&recv.ty).is_some_and(|elem| {
                matches!(elem, Type::Int | Type::String | Type::Named(_))
            }) && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::ParaFold => {
            jit_closure_elem_type(&recv.ty)
                .is_some_and(|elem| matches!(elem, Type::Int | Type::Named(_)))
                && args.len() == 3
                && resident_safe_para_fold_lambdas(args, callees)
        }
        TIR::TClosureOp::Each | TIR::TClosureOp::EachMut | TIR::TClosureOp::EachRef => {
            jit_closure_elem_type(&recv.ty).is_some_and(|elem| {
                matches!(elem, Type::Int | Type::String | Type::Named(_))
            }) && resident_safe_each_lambda(args, callees)
        }
        TIR::TClosureOp::FilterMap => {
            matches!(jit_list_iter_elem_type(&recv.ty), Some(Type::String))
                && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::SortBy => {
            jit_closure_elem_type(&recv.ty)
                .is_some_and(|elem| matches!(elem, Type::String | Type::Named(_)))
                && resident_safe_unary_lambda(args, callees)
                && matches!(
                    &args[0].kind,
                    TExprKind::Lambda(lam)
                        if matches!(&lam.executable, TIR::TLambdaBody::Expr(body) if matches!(body.ty, Type::Int))
                )
        }
        TIR::TClosureOp::TakeWhile
        | TIR::TClosureOp::SkipWhile
        | TIR::TClosureOp::Position => {
            matches!(jit_list_iter_elem_type(&recv.ty), Some(Type::Int))
                && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::Fold | TIR::TClosureOp::Reduce | TIR::TClosureOp::ViewFold => {
            jit_closure_elem_type(&recv.ty)
                .or_else(|| jit_list_iter_elem_type(&recv.ty))
                .is_some_and(|elem| matches!(elem, Type::Int | Type::Named(_)))
                && resident_safe_fold_lambda(args, callees)
        }
        TIR::TClosureOp::MinBy | TIR::TClosureOp::MaxBy => {
            matches!(jit_list_iter_elem_type(&recv.ty), Some(Type::String))
                && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::FlatMap => {
            jit_list_of_int_list_type(&recv.ty) && resident_safe_unary_lambda(args, callees)
        }
        TIR::TClosureOp::CountBy => {
            matches!(jit_list_iter_elem_type(&recv.ty), Some(Type::String))
                && resident_safe_unary_lambda(args, callees)
        }
        _ => false,
    }
}

fn resident_safe_enum_payload(payload: &TEnumPayload, callees: &HashSet<String>) -> bool {
    match payload {
        TEnumPayload::Unit => true,
        TEnumPayload::Positional(vals) => {
            vals.len() == 1
                && enum_payload_value_type(&vals[0].value.ty)
                && resident_safe_expr(&vals[0].value, callees)
        }
        TEnumPayload::Named(fields) => fields
            .iter()
            .all(|(_, a)| resident_safe_expr(&a.value, callees)),
    }
}

/// DataTree/JSON Object/Array payloads are Map/List handles; scalars stay as before.
fn enum_payload_value_type(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Int
            | Type::Float
            | Type::Float32
            | Type::Bool
            | Type::String
            | Type::Char
            | Type::Named(_)
    ) || jit_map_string_type(ty)
        || jit_list_native_type(ty)
        || jit_list_record_type(ty)
        || matches!(ty, Type::List(elem) if matches!(elem.as_ref(), Type::Named(_)))
}

fn resident_safe_tuple_fields(fields: &[(String, TExpr)], callees: &HashSet<String>) -> bool {
    fields.iter().all(|(_, value)| {
        matches!(&value.ty, Type::Int | Type::Float) && resident_safe_expr(value, callees)
    })
}

fn resident_safe_builtin_op(
    op: &TBuiltinOp,
    recv: &TExpr,
    args: &[TExpr],
    callees: &HashSet<String>,
) -> bool {
    if !resident_safe_expr(recv, callees) {
        return false;
    }
    match op {
        TBuiltinOp::LenString => {
            (matches!(&recv.ty, Type::String) || is_process_result_string_field(recv))
                && args.is_empty()
        }
        TBuiltinOp::Trim | TBuiltinOp::ToUpper | TBuiltinOp::ToLower => {
            (matches!(&recv.ty, Type::String) || is_process_result_string_field(recv))
                && args.is_empty()
        }
        TBuiltinOp::Replace => {
            matches!(&recv.ty, Type::String)
                && args.len() == 2
                && args
                    .iter()
                    .all(|a| matches!(&a.ty, Type::String) && resident_safe_expr(a, callees))
        }
        TBuiltinOp::Push => {
            (jit_list_native_type(&recv.ty)
                || matches!(&recv.ty, Type::List(elem) if jit_value_type(elem))
                || matches!(&recv.ty, Type::Apply { name, .. } if name == "PriorityQueue"))
                && args.len() == 1
                && (jit_value_type(&args[0].ty) || jit_compound_type(&args[0].ty))
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Keys | TBuiltinOp::Values => {
            jit_map_string_type(&recv.ty) && args.is_empty()
        }
        TBuiltinOp::Sort => {
            (jit_list_int_type(&recv.ty) || jit_list_string_type(&recv.ty)) && args.is_empty()
        }
        TBuiltinOp::LenList => {
            (matches!(&recv.ty, Type::String)
                || jit_list_native_type(&recv.ty)
                || jit_list_iter_elem_type(&recv.ty).is_some()
                || jit_closure_elem_type(&recv.ty).is_some()
                || matches!(&recv.ty, Type::Apply { name, .. } if matches!(name.as_str(), "Set" | "Deque"))
                || matches!(&recv.ty, Type::Named(name) if name == "BitSet"))
                && args.is_empty()
        }
        TBuiltinOp::GetList => {
            jit_list_native_type(&recv.ty)
                && !jit_list_intn_type(&recv.ty)
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::GetMap => {
            jit_map_resident_type(&recv.ty)
                && args.len() == 1
                && (if jit_map_int_type(&recv.ty) {
                    matches!(&args[0].ty, Type::Int)
                } else {
                    matches!(&args[0].ty, Type::String)
                })
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::InsertMap | TBuiltinOp::AddNewMap => {
            jit_map_resident_type(&recv.ty)
                && args.len() == 2
                && (if jit_map_int_type(&recv.ty) {
                    matches!(&args[0].ty, Type::Int)
                } else {
                    matches!(&args[0].ty, Type::String)
                })
                && jit_value_type(&args[1].ty)
                && resident_safe_expr(&args[0], callees)
                && resident_safe_expr(&args[1], callees)
        }
        TBuiltinOp::JoinSep => {
            (jit_list_native_type(&recv.ty) || jit_list_iter_elem_type(&recv.ty).is_some())
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::IsEmpty => {
            (jit_list_native_type(&recv.ty)
                || jit_list_iter_elem_type(&recv.ty).is_some()
                || matches!(&recv.ty, Type::Apply { name, .. } if matches!(name.as_str(), "Set" | "Deque")))
                && args.is_empty()
        }
        TBuiltinOp::ParseInt | TBuiltinOp::ParseFloat => {
            matches!(&recv.ty, Type::String) && args.is_empty()
        }
        TBuiltinOp::Slice { .. } => {
            (jit_list_int_type(&recv.ty) || matches!(&recv.ty, Type::String))
                && args.len() == 2
                && matches!(&args[0].ty, Type::Int)
                && matches!(&args[1].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
                && resident_safe_expr(&args[1], callees)
        }
        TBuiltinOp::Lines => matches!(&recv.ty, Type::String) && args.is_empty(),
        TBuiltinOp::Split => {
            matches!(&recv.ty, Type::String)
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Chars | TBuiltinOp::Bytes => matches!(&recv.ty, Type::String) && args.is_empty(),
        TBuiltinOp::After | TBuiltinOp::Before => {
            matches!(&recv.ty, Type::String)
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        // JIT ABI: Iter producers already materialize list handles.
        TBuiltinOp::IterToList | TBuiltinOp::IterCollect => {
            (jit_list_iter_elem_type(&recv.ty).is_some()
                || jit_closure_elem_type(&recv.ty).is_some())
                && args.is_empty()
        }
        TBuiltinOp::Take | TBuiltinOp::Skip | TBuiltinOp::StepBy | TBuiltinOp::Chunks
        | TBuiltinOp::Windows => {
            jit_list_iter_elem_type(&recv.ty).is_some()
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Dedup => jit_list_iter_elem_type(&recv.ty).is_some() && args.is_empty(),
        TBuiltinOp::Sum { float: false } => {
            matches!(jit_list_iter_elem_type(&recv.ty), Some(Type::Int)) && args.is_empty()
        }
        TBuiltinOp::Product { float: false }
        | TBuiltinOp::Min { float: false }
        | TBuiltinOp::Max { float: false } => {
            matches!(jit_list_iter_elem_type(&recv.ty), Some(Type::Int)) && args.is_empty()
        }
        TBuiltinOp::Flatten => jit_list_of_int_list_type(&recv.ty) && args.is_empty(),
        TBuiltinOp::Intersperse => {
            jit_list_iter_elem_type(&recv.ty).is_some()
                && args.len() == 1
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Zip { .. } => {
            jit_list_iter_elem_type(&recv.ty).is_some()
                && args.len() == 1
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Unzip { .. } => args.is_empty(),
        TBuiltinOp::TryCollect => {
            jit_result_list_elem(&recv.ty).is_some() && args.is_empty()
        }
        // D-HOLE1: Option.zip — both sides packed Option; builds a Present pair.
        TBuiltinOp::OptionZip { .. } => {
            matches!(&recv.ty, Type::Option(_))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Option(_))
                && resident_safe_expr(&args[0], callees)
        }
        // D-RANGE-EXCL1=C: `indexes()` → Iter<Int> of every valid index.
        TBuiltinOp::Indexes => {
            (jit_list_native_type(&recv.ty)
                || jit_list_iter_elem_type(&recv.ty).is_some()
                || jit_closure_elem_type(&recv.ty).is_some())
                && args.is_empty()
        }
        // JIT ABI: View/ViewMut materialize as owned list handles (inclusive slice).
        TBuiltinOp::ViewNew { .. } | TBuiltinOp::ViewMutNew { .. } => {
            (jit_list_native_type(&recv.ty) || jit_list_record_type(&recv.ty))
                && match args {
                    [range]
                        if matches!(&range.ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_RANGE) =>
                    {
                        resident_safe_expr(range, callees)
                    }
                    [start, end] => {
                        matches!(&start.ty, Type::Int)
                            && matches!(&end.ty, Type::Int)
                            && resident_safe_expr(start, callees)
                            && resident_safe_expr(end, callees)
                    }
                    _ => false,
                }
        }
        // D-COLLBREADTH1=A: Set / Deque / list.remove — Int elems only.
        TBuiltinOp::RemoveList { .. } => {
            jit_list_int_type(&recv.ty)
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::SetFrom => {
            jit_list_int_type(&recv.ty) && args.is_empty()
        }
        TBuiltinOp::SetInsert | TBuiltinOp::SetRemove => {
            matches!(&recv.ty, Type::Apply { name, args: targs }
                if name == "Set" && targs.len() == 1 && matches!(&targs[0], Type::Int))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::SetToList => {
            matches!(&recv.ty, Type::Apply { name, args: targs }
                if name == "Set" && targs.len() == 1 && matches!(&targs[0], Type::Int))
                && args.is_empty()
        }
        TBuiltinOp::SetUnion => {
            matches!(&recv.ty, Type::Apply { name, args: targs }
                if name == "Set" && targs.len() == 1 && matches!(&targs[0], Type::Int))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Apply { name, args: targs }
                    if name == "Set" && targs.len() == 1 && matches!(&targs[0], Type::Int))
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::SortedSetFrom | TBuiltinOp::PriorityQueueFrom => {
            jit_list_int_type(&recv.ty) && args.is_empty()
        }
        TBuiltinOp::SortedSetInsert | TBuiltinOp::SortedSetRemove => {
            matches!(&recv.ty, Type::Apply { name, .. } if name == "SortedSet")
                && args.len() == 1
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::SortedSetToList
        | TBuiltinOp::PriorityQueuePeek
        | TBuiltinOp::PriorityQueueToSortedList
        | TBuiltinOp::LruKeys
        | TBuiltinOp::BitSetCount
        | TBuiltinOp::BitSetToList
        | TBuiltinOp::ByteBufferToBytes => args.is_empty(),
        TBuiltinOp::First | TBuiltinOp::Last => {
            matches!(&recv.ty, Type::Apply { name, .. } if name == "SortedSet")
                && args.is_empty()
        }
        TBuiltinOp::Pop => {
            (matches!(&recv.ty, Type::Apply { name, .. } if name == "PriorityQueue")
                || jit_list_native_type(&recv.ty)
                || matches!(&recv.ty, Type::List(elem) if jit_value_type(elem) || matches!(elem.as_ref(), Type::Apply { name, .. } if name == "Task")))
                && args.is_empty()
        }
        TBuiltinOp::LruPut | TBuiltinOp::LruAddNew => {
            matches!(&recv.ty, Type::Apply { name, .. } if name == "Lru")
                && args.len() == 2
                && args.iter().all(|arg| resident_safe_expr(arg, callees))
        }
        TBuiltinOp::LruGet | TBuiltinOp::ContainsKey => {
            matches!(&recv.ty, Type::Apply { name, .. } if name == "Lru")
                && args.len() == 1
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::BitSetAdd | TBuiltinOp::BitSetRemove => {
            matches!(&recv.ty, Type::Named(name) if name == "BitSet")
                && args.len() == 1
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::BitSetNew | TBuiltinOp::ByteBufferNew => args.is_empty(),
        TBuiltinOp::ByteBufferWrite { .. } => {
            matches!(&recv.ty, Type::Named(name) if name == "ByteBuffer")
                && args.len() == 1
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::TrimView => matches!(&recv.ty, Type::String) && args.is_empty(),
        TBuiltinOp::AfterView | TBuiltinOp::BeforeView => {
            matches!(&recv.ty, Type::String)
                && args.len() == 1
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::BagAdd | TBuiltinOp::BagRemove | TBuiltinOp::BagHas | TBuiltinOp::BagCount => {
            matches!(&recv.ty, Type::Apply { name, args: targs }
                if name == "Bag" && targs.len() == 1 && jit_bag_raw_key_candidate(&targs[0]))
                && args.len() == 1
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::BagLen => {
            matches!(&recv.ty, Type::Apply { name, args: targs }
                if name == "Bag" && targs.len() == 1 && jit_bag_raw_key_candidate(&targs[0]))
                && args.is_empty()
        }
        TBuiltinOp::Contains
            if matches!(&recv.ty, Type::Named(name) if name == jet_foundation::Syntax::TYPE_RANGE) =>
        {
            args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Contains
            if matches!(&recv.ty, Type::Apply { name, args: targs }
                if name == "Set"
                    && targs.len() == 1
                    && matches!(&targs[0], Type::Int | Type::String)) =>
        {
            args.len() == 1
                && match (&recv.ty, &args[0].ty) {
                    (Type::Apply { args: targs, .. }, Type::Int) => {
                        matches!(targs.as_slice(), [Type::Int])
                    }
                    (Type::Apply { args: targs, .. }, Type::String) => {
                        matches!(targs.as_slice(), [Type::String])
                    }
                    _ => false,
                }
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Contains
            if matches!(&recv.ty, Type::List(inner) if **inner == Type::String) =>
        {
            args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::Contains => {
            matches!(&recv.ty, Type::String)
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::StartsWith | TBuiltinOp::EndsWith => {
            matches!(&recv.ty, Type::String)
                && args.len() == 1
                && matches!(&args[0].ty, Type::String)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::MatchGroup => {
            (matches!(&recv.ty, Type::Named(n) if n == "Match")
                || matches!(&recv.kind, TExprKind::Local(_)))
                && args.len() == 1
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::DequePushFront | TBuiltinOp::DequePushBack => {
            matches!(&recv.ty, Type::Apply { name, args: targs }
                if name == "Deque" && targs.len() == 1 && matches!(&targs[0], Type::Int))
                && args.len() == 1
                && matches!(&args[0].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
        }
        TBuiltinOp::DequePopFront
        | TBuiltinOp::DequePopBack
        | TBuiltinOp::DequePeekFront
        | TBuiltinOp::DequePeekBack => {
            matches!(&recv.ty, Type::Apply { name, args: targs }
                if name == "Deque" && targs.len() == 1 && matches!(&targs[0], Type::Int))
                && args.is_empty()
        }
        TBuiltinOp::InsertList => {
            jit_list_int_type(&recv.ty)
                && args.len() == 2
                && matches!(&args[0].ty, Type::Int)
                && matches!(&args[1].ty, Type::Int)
                && resident_safe_expr(&args[0], callees)
                && resident_safe_expr(&args[1], callees)
        }
        _ => false,
    }
}

pub(crate) fn resident_safe_stmt(stmt: &TStmt, callees: &HashSet<String>) -> bool {
    match stmt {
        TStmt::Let { init, gc_promotion: _, gc_transferred: _, .. } => {
            // Promotion/transfer only wraps the same payload handle for the
            // collector; JIT stores the finite snapshot directly (D-OPTGC1).
            // `gc_transferred` is a call result that is already a root handle.
            resident_safe_expr(init, callees)
        }
        // D-TUPLE-DESTRUCT1: `(tx, rx) := tasks.channel<T>()` — the one
        // tuple-destructure shape this tier covers (general `TupleDestructure` /
        // `StructDestructure` / `ListDestructure` are not covered at all otherwise;
        // that's unrelated pre-existing scope, not narrowed here). Mirrors the old
        // single-handle `let ch := tasks.channel()` + `ch.sender()` shape exactly:
        // one `channel_new` call for the receiver handle, one `channel_sender` call
        // on it for the sender handle — same two host calls, now both fired at the
        // producer site instead of at a later `.sender()` call.
        TStmt::TupleDestructure { init, binds, .. } => {
            (binds.len() == 2
                && matches!(
                    &init.kind,
                    TExprKind::CoreCall { module, method, args, .. }
                        if module == "core.tasks"
                            && method == "channel"
                            && args.len() <= 1
                            && args.iter().all(|a| resident_safe_expr(a, callees))
                ))
                || (jit_tuple_type(&init.ty)
                    && binds.len()
                        == match &init.ty {
                            Type::Tuple(fields) => fields.len(),
                            _ => 0,
                        }
                    && resident_safe_expr(init, callees))
        }
        TStmt::ListDestructure { init, elems, .. } => {
            jit_list_native_type(&init.ty)
                && !elems.is_empty()
                && resident_safe_expr(init, callees)
        }
        TStmt::StructDestructure { init, binds, .. } => {
            jit_struct_type(&init.ty) && !binds.is_empty() && resident_safe_expr(init, callees)
        }
        TStmt::Assign {
            place,
            op,
            value,
            clone_value,
        } => {
            let compound = op.as_ref().is_none_or(|op| match &value.ty {
                Type::Int | Type::IntN { .. } => matches!(
                    op,
                    BinOp::Add
                        | BinOp::Sub
                        | BinOp::Mul
                        | BinOp::Div
                        | BinOp::Rem
                        | BinOp::BitAnd
                        | BinOp::BitOr
                        | BinOp::BitXor
                        | BinOp::Shl
                        | BinOp::Shr
                ),
                Type::Float => {
                    matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div)
                }
                _ => false,
            });
            let local = place.as_local().is_some_and(|local| {
                local
                    .name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            });
            let field = structured_record_field_place(place)
                || matches!(
                    place,
                    TIR::TPlace::Expr(expr)
                        if matches!(
                            &expr.kind,
                            TExprKind::PoolSlot { field: Some(_), .. }
                        )
                );
            (!clone_value || jit_value_type(&value.ty))
                && compound
                && (local || field)
                && resident_safe_expr(value, callees)
        }
        TStmt::Return(ret) => ret.as_ref().is_none_or(|e| resident_safe_expr(e, callees)),
        TStmt::ExprStmt(e) => resident_safe_expr(e, callees),
        // Lowered by JIT (`Close` / file hosts); same expr gate as ExprStmt.
        TStmt::DeferClose { close, .. } => resident_safe_expr(close, callees),
        TStmt::If {
            cond,
            then_body,
            else_body,
            ..
        } => {
            let cond_ok = match cond {
                TIfCond::Plain(e) => matches!(&e.ty, Type::Bool) && resident_safe_expr(e, callees),
                TIfCond::IfLet { pattern, subj } => {
                    matches!(
                        &pattern.pattern,
                        Pattern::Ok { .. }
                            | Pattern::Err { .. }
                            | Pattern::Present { .. }
                            | Pattern::Variant { .. }
                    ) && resident_safe_expr(subj, callees)
                }
                TIfCond::Matches { subj, .. } => resident_safe_expr(subj, callees),
                TIfCond::And { left, right } => {
                    // `if a == .V(x) && …` and similar dual conditions.
                    matches!(
                        (left.as_ref(), right.as_ref()),
                        (
                            TIfCond::Plain(e)
                                | TIfCond::Matches { subj: e, .. }
                                | TIfCond::IfLet { subj: e, .. }
                                | TIfCond::IsNone { subj: e },
                            TIfCond::Plain(e2)
                                | TIfCond::Matches { subj: e2, .. }
                                | TIfCond::IfLet { subj: e2, .. }
                                | TIfCond::IsNone { subj: e2 },
                        ) if resident_safe_expr(e, callees) && resident_safe_expr(e2, callees)
                    ) || {
                        // Recurse for nested And / Plain bool.
                        let left_ok = match left.as_ref() {
                            TIfCond::Plain(e) => {
                                matches!(&e.ty, Type::Bool) && resident_safe_expr(e, callees)
                            }
                            TIfCond::IfLet { subj, .. }
                            | TIfCond::Matches { subj, .. }
                            | TIfCond::IsNone { subj } => resident_safe_expr(subj, callees),
                            TIfCond::And { .. } => false,
                        };
                        let right_ok = match right.as_ref() {
                            TIfCond::Plain(e) => {
                                matches!(&e.ty, Type::Bool) && resident_safe_expr(e, callees)
                            }
                            TIfCond::IfLet { subj, .. }
                            | TIfCond::Matches { subj, .. }
                            | TIfCond::IsNone { subj } => resident_safe_expr(subj, callees),
                            TIfCond::And { .. } => false,
                        };
                        left_ok && right_ok
                    }
                }
                // `if maybe == .None` — Option ABI already lowered in lower_ctx.
                TIfCond::IsNone { subj } => {
                    matches!(&subj.ty, Type::Option(_)) && resident_safe_expr(subj, callees)
                }
            };
            cond_ok
                && then_body.iter().all(|s| resident_safe_stmt(s, callees))
                && else_body
                    .as_ref()
                    .is_none_or(|b| b.iter().all(|s| resident_safe_stmt(s, callees)))
        }
        TStmt::Loop { body, .. } => body.iter().all(|s| resident_safe_stmt(s, callees)),
        TStmt::While { cond, body, .. } => {
            matches!(&cond.ty, Type::Bool)
                && resident_safe_expr(cond, callees)
                && body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::CountedLoop {
            init,
            cond,
            step,
            body,
            ..
        } => {
            resident_safe_stmt(init, callees)
                && matches!(&cond.ty, Type::Bool)
                && resident_safe_expr(cond, callees)
                && step.as_ref().is_none_or(|step| resident_safe_stmt(step, callees))
                && body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::Range {
            start,
            end,
            step,
            body,
            ..
        } => {
            matches!(&start.ty, Type::Int)
                && matches!(&end.ty, Type::Int)
                && step.is_none()
                && resident_safe_expr(start, callees)
                && resident_safe_expr(end, callees)
                && body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::Break(_) | TStmt::Continue(_) => true,
        TStmt::BreakValue { value, .. } => {
            (jit_value_type(&value.ty)
                || jit_list_native_type(&value.ty)
                || jit_struct_type(&value.ty)
                || jit_tuple_type(&value.ty))
                && resident_safe_expr(value, callees)
        }
        TStmt::IndexAssign {
            base,
            index,
            is_map,
            value,
            ..
        } => {
            if *is_map {
                let key_ok = if jit_map_int_type(&base.ty) {
                    matches!(&index.ty, Type::Int)
                } else {
                    jit_map_string_type(&base.ty) && matches!(&index.ty, Type::String)
                };
                key_ok
                    && !jit_map_intn_value_type(&base.ty)
                    && jit_value_type(&value.ty)
                    && resident_safe_expr(base, callees)
                    && resident_safe_expr(index, callees)
                    && resident_safe_expr(value, callees)
            } else {
                (jit_list_native_type(&base.ty)
                    || jit_list_iter_elem_type(&base.ty).is_some()
                    || jit_closure_elem_type(&base.ty).is_some())
                    && matches!(&index.ty, Type::Int)
                    && matches!(
                        &value.ty,
                        Type::Int
                            | Type::IntN { .. }
                            | Type::Float
                            | Type::Float32
                            | Type::String
                            | Type::Char
                            | Type::Bool
                    )
                    && resident_safe_expr(base, callees)
                    && resident_safe_expr(index, callees)
                    && resident_safe_expr(value, callees)
            }
        }
        TStmt::IndexFieldAssign(assign) => {
            let base_ok = jit_list_record_type(&assign.base.ty)
                || matches!(
                    &assign.base.ty,
                    Type::Apply { name, args }
                        if name == "ViewMut"
                            && args.len() == 1
                            && (record_type_key(&args[0]).is_some()
                                || matches!(&args[0], Type::TraitObject(_)))
                );
            !assign.is_map
                && !assign.clone_value
                && base_ok
                && matches!(&assign.index.ty, Type::Int)
                && assign
                    .op
                    .is_none_or(|_| matches!(&assign.field_ty, Type::Int | Type::Float))
                && resident_safe_expr(&assign.base, callees)
                && resident_safe_expr(&assign.index, callees)
                && resident_safe_expr(&assign.value, callees)
        }
        TStmt::ForIn {
            var2,
            source,
            collection,
            method_kind,
            columnar,
            by_value,
            step,
            body,
            ..
        } => {
            use jet_codegen::Codegen::TIR::TForInMethod;
            let chars_ok = matches!(method_kind, Some(TForInMethod::Chars))
                && matches!(&source.ty, Type::String);
            let iterable_ok = matches!(
                method_kind,
                Some(TForInMethod::Iterable { coll_type, iter_type })
                    if record_type_key(&source.ty).is_some_and(|name| name == *coll_type)
                        && callees.contains(&format!("{coll_type}::iter"))
                        && callees.contains(&format!("{iter_type}::next"))
            );
            let process_lines_ok = matches!(method_kind, Some(TForInMethod::LinesProcessStream))
                && var2.is_none()
                && matches!(
                    &source.kind,
                    TExprKind::Field { recv, field, boxed: false }
                        if matches!(&recv.ty, Type::Named(n) if n == "ProcessChild")
                            && (field == "stdout" || field == "stderr")
                );
            let file_lines_ok = matches!(method_kind, Some(TForInMethod::LinesFile)) && var2.is_none();
            let stdin_lines_ok =
                matches!(method_kind, Some(TForInMethod::LinesStdin)) && var2.is_none();
            // TIR leaves `.lines()` MethodCall as Unit and the loop var unbound;
            // lower hardcodes String elems. Don't demand collection/body types.
            if process_lines_ok || file_lines_ok || stdin_lines_ok {
                return !columnar
                    && resident_safe_expr(source, callees)
                    && step
                        .as_ref()
                        .is_none_or(|step| resident_safe_expr(step, callees))
                    && body.iter().all(|s| resident_safe_process_lines_body(s, callees));
            }
            let list_ok = method_kind.is_none()
                && var2.is_none()
                && (jit_list_iter_elem_type(&collection.ty).is_some()
                    || jit_closure_elem_type(&collection.ty).is_some()
                    || jit_list_record_type(&collection.ty));
            let stream_ok = method_kind.is_none()
                && var2.is_none()
                && step.is_none()
                && matches!(
                    &collection.ty,
                    Type::Apply { name, args }
                        if name == "Stream"
                            && args.len() == 1
                            && jit_value_type(&args[0])
                );
            // D-RANGE-EXCL1=C: sequence two-binding is index then item.
            let list_pair_ok = method_kind.is_none()
                && var2.is_some()
                && (jit_list_iter_elem_type(&collection.ty).is_some()
                    || jit_closure_elem_type(&collection.ty).is_some()
                    || jit_list_record_type(&collection.ty));
            let map_ok = method_kind.is_none()
                && var2.is_some()
                && jit_map_string_type(&collection.ty);
            // `by_value` marks Stream/Iter/HTTPBodyChunks. Only Iter<T> is list-
            // backed under the JIT host ABI (true lazy handles don't cross).
            let by_value_ok = !*by_value
                || jet_foundation::Collections::is_iter_type(&collection.ty)
                || matches!(&collection.ty, Type::Apply { name, .. } if name == "Stream");
            (chars_ok || iterable_ok || stream_ok || list_ok || list_pair_ok || map_ok)
                && !columnar
                && by_value_ok
                && resident_safe_expr(source, callees)
                && resident_safe_expr(collection, callees)
                && step.as_ref().is_none_or(|step| resident_safe_expr(step, callees))
                && body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::EnumMatch {
            arms, else_body, ..
        } => {
            arms.iter()
                .all(|a| a.body.iter().all(|s| resident_safe_stmt(s, callees)))
                && else_body
                    .as_ref()
                    .is_none_or(|b| b.iter().all(|s| resident_safe_stmt(s, callees)))
        }
        TStmt::MixedSwitch {
            subject,
            arms,
            else_body,
        } => {
            resident_safe_expr(subject, callees)
                && arms.iter()
                .all(|(_, b)| b.iter().all(|s| resident_safe_stmt(s, callees)))
                && else_body
                    .as_ref()
                    .is_none_or(|b| b.iter().all(|s| resident_safe_stmt(s, callees)))
        }
        TStmt::RangeSwitch {
            subject,
            arms,
            else_body,
        } => {
            resident_safe_expr(subject, callees)
                && arms
                    .iter()
                    .all(|(_, _, body)| body.iter().all(|s| resident_safe_stmt(s, callees)))
                && else_body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::TaskGroup { body, .. }
        | TStmt::Region(body)
        | TStmt::Impure(body)
        | TStmt::Inline(body)
        | TStmt::Unsafe(body)
        | TStmt::Shield { body }
        | TStmt::Transact { body, .. }
        | TStmt::Live { body } => {
            body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::ContextBlock { guards, body } => {
            guards
                .iter()
                .all(|(_, value)| resident_safe_expr(value, callees))
                && body.iter().all(|s| resident_safe_stmt(s, callees))
        }
        TStmt::Reactive { executable, .. } => match &executable.executable {
            TIR::TLambdaBody::Expr(e) => resident_safe_expr(e, callees),
            TIR::TLambdaBody::Block(stmts) => stmts.iter().all(|s| resident_safe_stmt(s, callees)),
        },
        TStmt::Layout { body, .. } => body.iter().all(|s| resident_safe_stmt(s, callees)),
        TStmt::IndexHookAssign {
            base,
            index,
            value,
            ..
        } => {
            resident_safe_expr(base, callees)
                && resident_safe_expr(index, callees)
                && resident_safe_expr(value, callees)
        }
        // JIT models singleton split views as checked element/window handles.
        // Range windows need relative end-bound enforcement before they can be
        // resident-safe.
        TStmt::SplitViews {
            owner,
            single,
            elem_ty,
            ..
        } => {
            *single
                && elem_ty.as_ref().is_some_and(|ty| {
                    matches!(ty, Type::Int | Type::Float | Type::String)
                        || record_type_key(ty).is_some()
                })
                && owner
                    .as_ref()
                    .is_none_or(|o| resident_safe_expr(o, callees))
        }
        // Whole-value GC assign (`replace_all`): nested stmt is a plain assign
        // of a finite payload snapshot into the root Variable.
        TStmt::GcEdit {
            replace_all,
            index_temp,
            stmt,
            ..
        } => {
            *replace_all
                && index_temp
                    .as_ref()
                    .is_none_or(|(_, e)| resident_safe_expr(e, callees))
                && resident_safe_stmt(stmt, callees)
        }
        TStmt::MathSwizzleAssign { base, value, .. } => {
            resident_safe_expr(base, callees) && resident_safe_expr(value, callees)
        }
        _ => false,
    }
}

fn structured_record_field_place(place: &TIR::TPlace) -> bool {
    let TIR::TPlace::Expr(expr) = place else {
        return false;
    };
    matches!(
        &expr.kind,
        TIR::TExprKind::Field {
            recv,
            boxed: false,
            ..
        } if matches!(
            &recv.kind,
            TIR::TExprKind::Local(_) | TIR::TExprKind::PoolSlot { .. }
        )
    )
}

pub(crate) fn resident_safe_func(tir: &TFunc, callees: &HashSet<String>) -> bool {
    resident_safe_func_detail(tir, callees).is_none()
}

pub(crate) fn resident_safe_func_detail(tir: &TFunc, callees: &HashSet<String>) -> Option<String> {
    if !matches!(
        tir.kind,
        TFuncKind::TopLevel | TFuncKind::Method { .. } | TFuncKind::TraitMethod { .. }
    ) {
        return Some("not top-level".into());
    }
    if !tir.generics.is_empty() || tir.is_unsafe || tir.is_reactive {
        return Some("func attrs unsupported".into());
    }
    if !tir.params.iter().all(|(_, ty, _)| jit_value_type(ty)) {
        return Some("param type unsupported".into());
    }
    if let Some(ret) = &tir.ret {
        if !jit_value_type(ret) {
            return Some("return type unsupported".into());
        }
    }
    for (i, s) in tir.body.iter().enumerate() {
        if !resident_safe_stmt(s, callees) {
            if let Some(drill) = first_unsafe_stmt_detail(std::slice::from_ref(s), callees) {
                return Some(format!("body stmt {i}: {drill}"));
            }
            let extra = match s {
                TStmt::Let { init, .. } | TStmt::ExprStmt(init) | TStmt::Assign { value: init, .. } => {
                    let mut tag = format!(" init={}", expr_kind_tag(init));
                    if let TExprKind::CoreClosureCall {
                        kind: TCoreClosureKind::ReactiveDerived { executable, .. }
                            | TCoreClosureKind::ReactiveEffect { executable, .. }
                            | TCoreClosureKind::UiReactiveRender { executable, .. },
                    } = &init.kind
                    {
                        match &executable.executable {
                            TIR::TLambdaBody::Expr(e) => {
                                tag.push_str(&format!(" body={}", expr_kind_tag(e)));
                                if let TExprKind::Binary {
                                    overflow,
                                    lhs,
                                    rhs,
                                    ..
                                } = &e.kind
                                {
                                    tag.push_str(&format!(
                                        " bin=({} / {} overflow={overflow} lhs_ok={} rhs_ok={})",
                                        expr_kind_tag(lhs),
                                        expr_kind_tag(rhs),
                                        intish_ty(&lhs.ty) || reactive_get_intish(lhs),
                                        intish_ty(&rhs.ty) || reactive_get_intish(rhs),
                                    ));
                                }
                                if let TExprKind::HandleMethod { recv, .. } = &e.kind {
                                    tag.push_str(&format!(" hm_recv={}", expr_kind_tag(recv)));
                                }
                            }
                            TIR::TLambdaBody::Block(stmts) => {
                                tag.push_str(&format!(" block_len={}", stmts.len()));
                            }
                        }
                    }
                    tag
                }
                _ => String::new(),
            };
            return Some(format!("body stmt {i}: {:?}{extra}", stmt_kind_tag(s)));
        }
    }
    None
}

fn expr_kind_tag(expr: &TExpr) -> &'static str {
    match &expr.kind {
        TExprKind::CoreCall { module, method, .. } => {
            // Leak short tag for diagnostics only.
            Box::leak(format!("CoreCall:{module}.{method}").into_boxed_str())
        }
        TExprKind::CoreClosureCall { kind } => match kind {
            TCoreClosureKind::ReactiveDerived { .. } => "CoreClosure:Derived",
            TCoreClosureKind::ReactiveEffect { .. } => "CoreClosure:Effect",
            TCoreClosureKind::UiReactiveRender { .. } => "CoreClosure:UiRender",
            TCoreClosureKind::Spawn { .. } => "CoreClosure:Spawn",
            _ => "CoreClosure:Other",
        },
        TExprKind::HandleMethod { op, .. } => "HandleMethod",
        TExprKind::Call { name, .. } => Box::leak(format!("Call:{name}").into_boxed_str()),
        TExprKind::Binary { .. } => "Binary",
        TExprKind::Local(_) => "Local",
        TExprKind::Clone(_) => "Clone",
        _ => "OtherExpr",
    }
}

fn stmt_kind_tag(stmt: &TStmt) -> &'static str {
    match stmt {
        TStmt::Let { .. } => "Let",
        TStmt::Assign { .. } => "Assign",
        TStmt::Return(_) => "Return",
        TStmt::ExprStmt(_) => "ExprStmt",
        TStmt::Reactive { .. } => "Reactive",
        TStmt::If { .. } => "If",
        TStmt::Loop { .. } => "Loop",
        TStmt::While { .. } => "While",
        TStmt::ForIn { .. } => "ForIn",
        TStmt::Inline(_) => "Inline",
        TStmt::Impure(_) => "Impure",
        TStmt::Region(_) => "Region",
        TStmt::TaskGroup { .. } => "TaskGroup",
        TStmt::Unsafe(_) => "Unsafe",
        _ => "Other",
    }
}

fn first_unsafe_stmt_detail(stmts: &[TStmt], callees: &HashSet<String>) -> Option<String> {
    for (i, s) in stmts.iter().enumerate() {
        if resident_safe_stmt(s, callees) {
            continue;
        }
        match s {
            TStmt::Inline(body)
            | TStmt::Impure(body)
            | TStmt::Region(body)
            | TStmt::TaskGroup { body, .. }
            | TStmt::Unsafe(body) => {
                if let Some(inner) = first_unsafe_stmt_detail(body, callees) {
                    return Some(format!("{}[{i}]>{inner}", stmt_kind_tag(s)));
                }
                return Some(format!("{}[{i}]", stmt_kind_tag(s)));
            }
            TStmt::Let { init, .. } | TStmt::ExprStmt(init) | TStmt::Assign { value: init, .. } => {
                let mut detail = format!("{}[{i}] init={}", stmt_kind_tag(s), expr_kind_tag(init));
                if let TExprKind::CoreClosureCall {
                    kind: TCoreClosureKind::ReactiveDerived { executable, .. }
                        | TCoreClosureKind::ReactiveEffect { executable, .. }
                        | TCoreClosureKind::UiReactiveRender { executable, .. },
                } = &init.kind
                {
                    match &executable.executable {
                        TIR::TLambdaBody::Expr(e) => {
                            detail.push_str(&format!(" body={}", expr_kind_tag(e)));
                            if let TExprKind::Binary {
                                overflow,
                                lhs,
                                rhs,
                                ..
                            } = &e.kind
                            {
                                let recv_ty = if let TExprKind::HandleMethod { recv, op, .. } =
                                    &lhs.kind
                                {
                                    format!("recv={:?} op_is_get={}", recv.ty, matches!(op, THandleOp::ReactiveGet))
                                } else {
                                    "recv=?".into()
                                };
                                detail.push_str(&format!(
                                    " bin=({}/{} overflow={overflow} lhs_ok={} rhs_ok={} lhs_ty={:?} rhs_ty={:?} {recv_ty})",
                                    expr_kind_tag(lhs),
                                    expr_kind_tag(rhs),
                                    intish_ty(&lhs.ty) || reactive_get_intish(lhs),
                                    intish_ty(&rhs.ty) || reactive_get_intish(rhs),
                                    lhs.ty,
                                    rhs.ty,
                                ));
                            }
                        }
                        TIR::TLambdaBody::Block(inner) => {
                            if let Some(b) = first_unsafe_stmt_detail(inner, callees) {
                                detail.push_str(&format!(" block>{b}"));
                            }
                        }
                    }
                }
                if let TExprKind::HandleMethod { op, .. } = &init.kind {
                    let op_tag = match op {
                        THandleOp::UiBackendMethod { method } => {
                            Box::leak(format!("UiBackend:{method}").into_boxed_str())
                        }
                        THandleOp::EventMethod { method } => {
                            Box::leak(format!("Event:{method}").into_boxed_str())
                        }
                        THandleOp::ReactiveGet => "ReactiveGet",
                        THandleOp::ReactiveSet => "ReactiveSet",
                        _ => "HandleOp",
                    };
                    detail.push_str(&format!(" op={op_tag}"));
                }
                return Some(detail);
            }
            _ => return Some(format!("{}[{i}]", stmt_kind_tag(s))),
        }
    }
    None
}

pub(crate) fn resident_safe_program(program: &JitProgram) -> bool {
    let names: HashSet<String> = program.funcs.iter().map(|f| f.name.clone()).collect();
    let main_ok = if program.entry == "__jet_cli_main" {
        program
            .funcs
            .iter()
            .any(|f| f.name == "run" && resident_safe_func(f, &names))
    } else {
        program.funcs.iter().any(|f| {
            f.name == program.entry
                && f.params.is_empty()
                && (f.ret.is_none()
                    || matches!(&f.ret, Some(Type::Result { ok, err })
                        if matches!(ok.as_ref(), Type::Named(n) if n == "Void" || n == "Unit")
                            && matches!(err.as_ref(), Type::String | Type::Named(_))))
                && resident_safe_func(f, &names)
        })
    };
    if !main_ok {
        return false;
    }
    if !program.funcs.iter().all(|f| resident_safe_func(f, &names)) {
        return false;
    }
    let spawn_sites = count_spawn_sites(program);
    if spawn_sites != program.spawn_lambdas.len() {
        return false;
    }
    program
        .spawn_lambdas
        .iter()
        .all(|lam| resident_safe_spawn_lambda(lam, &names))
}

pub(crate) fn count_spawn_sites(program: &JitProgram) -> usize {
    let mut n = 0usize;
    for f in &program.funcs {
        count_spawn_sites_stmts(&f.body, &mut n);
    }
    n
}

fn count_spawn_sites_stmts(stmts: &[TStmt], n: &mut usize) {
    for s in stmts {
        match s {
            TStmt::Let { init, .. }
            | TStmt::Assign { value: init, .. }
            | TStmt::Return(Some(init))
            | TStmt::ExprStmt(init) => count_spawn_sites_expr(init, n),
            TStmt::IndexFieldAssign(assign) => {
                count_spawn_sites_expr(&assign.base, n);
                count_spawn_sites_expr(&assign.index, n);
                count_spawn_sites_expr(&assign.value, n);
            }
            TStmt::If {
                then_body,
                else_body,
                ..
            } => {
                count_spawn_sites_stmts(then_body, n);
                if let Some(b) = else_body {
                    count_spawn_sites_stmts(b, n);
                }
            }
            TStmt::Loop { body, .. } | TStmt::While { body, .. } | TStmt::Range { body, .. } => {
                count_spawn_sites_stmts(body, n)
            }
            TStmt::CountedLoop {
                init, step, body, ..
            } => {
                count_spawn_sites_stmts(std::slice::from_ref(init), n);
                if let Some(step) = step {
                    count_spawn_sites_stmts(std::slice::from_ref(step), n);
                }
                count_spawn_sites_stmts(body, n);
            }
            TStmt::TaskGroup { body, .. }
            | TStmt::Region(body)
            | TStmt::Impure(body)
            | TStmt::Inline(body)
            | TStmt::Unsafe(body)
            | TStmt::Shield { body }
            | TStmt::DebugOnly(body) => count_spawn_sites_stmts(body, n),
            TStmt::Reactive { .. } => {
                *n += 1;
            }
            TStmt::Layout { body, .. } => count_spawn_sites_stmts(body, n),
            TStmt::ContextBlock { guards, body } => {
                for (_, value) in guards {
                    count_spawn_sites_expr(value, n);
                }
                count_spawn_sites_stmts(body, n);
            }
            TStmt::Transact { body, .. } => count_spawn_sites_stmts(body, n),
            TStmt::GcEdit { index_temp, stmt, .. } => {
                if let Some((_, idx)) = index_temp {
                    count_spawn_sites_expr(idx, n);
                }
                count_spawn_sites_stmts(std::slice::from_ref(stmt), n);
            }
            TStmt::ForIn {
                source,
                collection,
                step,
                body,
                ..
            } => {
                count_spawn_sites_expr(source, n);
                count_spawn_sites_expr(collection, n);
                if let Some(step) = step {
                    count_spawn_sites_expr(step, n);
                }
                count_spawn_sites_stmts(body, n);
            }
            TStmt::TupleDestructure { init, .. }
            | TStmt::StructDestructure { init, .. }
            | TStmt::ListDestructure { init, .. } => count_spawn_sites_expr(init, n),
            TStmt::EnumMatch {
                arms, else_body, ..
            } => {
                for arm in arms {
                    count_spawn_sites_stmts(&arm.body, n);
                }
                if let Some(b) = else_body {
                    count_spawn_sites_stmts(b, n);
                }
            }
            TStmt::MixedSwitch {
                arms, else_body, ..
            } => {
                for (_, b) in arms {
                    count_spawn_sites_stmts(b, n);
                }
                if let Some(b) = else_body {
                    count_spawn_sites_stmts(b, n);
                }
            }
            _ => {}
        }
    }
}

fn count_spawn_sites_expr(expr: &TExpr, n: &mut usize) {
    if matches!(
        expr.kind,
        TExprKind::CoreClosureCall {
            kind: TCoreClosureKind::Spawn { .. }
                | TCoreClosureKind::ReactiveDerived { .. }
                | TCoreClosureKind::ReactiveEffect { .. }
                | TCoreClosureKind::UiReactiveRender { .. }
        }
    ) {
        *n += 1;
    }
    if matches!(
        &expr.kind,
        TExprKind::HandleMethod {
            op: THandleOp::GameSceneOnFrame,
            ..
        }
    ) {
        *n += 1;
    }
    if let TExprKind::HandleMethod {
        op: THandleOp::WatchMethod {
            callback_index: Some(_),
            ..
        },
        ..
    } = &expr.kind
    {
        *n += 1;
    }
    if let TExprKind::HandleMethod {
        op: THandleOp::EventMethod { method },
        ..
    } = &expr.kind
    {
        if matches!(method.as_str(), "on" | "once" | "on_priority") {
            *n += 1;
        }
    }
    if let TExprKind::HandleMethod {
        op: THandleOp::UiBackendMethod { method },
        ..
    } = &expr.kind
    {
        if method == "on_click" {
            *n += 1;
        }
    }
    match &expr.kind {
        TExprKind::Print(inner)
        | TExprKind::Unary { operand: inner, .. }
        | TExprKind::Clone(inner)
        | TExprKind::Ok(inner)
        | TExprKind::Err(inner) => count_spawn_sites_expr(inner, n),
        TExprKind::Binary { lhs, rhs, .. } => {
            count_spawn_sites_expr(lhs, n);
            count_spawn_sites_expr(rhs, n);
        }
        TExprKind::Call { args, .. } => {
            for a in args {
                count_spawn_sites_expr(&a.value, n);
            }
        }
        TExprKind::IfExpr {
            cond,
            then_body,
            then_value,
            else_body,
            else_value,
            ..
        } => {
            count_spawn_sites_if_cond(cond, n);
            count_spawn_sites_stmts(then_body, n);
            count_spawn_sites_expr(then_value, n);
            count_spawn_sites_stmts(else_body, n);
            count_spawn_sites_expr(else_value, n);
        }
        TExprKind::HandleMethod { recv, args, .. } => {
            count_spawn_sites_expr(recv, n);
            for a in args {
                count_spawn_sites_expr(a, n);
            }
        }
        TExprKind::BuiltinMethod { recv, args, .. }
        | TExprKind::ClosureMethod { recv, args, .. } => {
            count_spawn_sites_expr(recv, n);
            for a in args {
                count_spawn_sites_expr(a, n);
            }
        }
        TExprKind::CoreCall { args, .. } => {
            for a in args {
                count_spawn_sites_expr(a, n);
            }
        }
        TExprKind::OrFallback { value, .. } => count_spawn_sites_expr(value, n),
        TExprKind::TaskGroupAll { tasks }
        | TExprKind::TaskGroupRace { tasks }
        | TExprKind::TaskGroupAny { tasks } => count_spawn_sites_expr(tasks, n),
        _ => {}
    }
}

fn count_spawn_sites_if_cond(cond: &TIfCond, n: &mut usize) {
    match cond {
        TIfCond::Plain(expr) => count_spawn_sites_expr(expr, n),
        TIfCond::And { left, right } => {
            count_spawn_sites_if_cond(left, n);
            count_spawn_sites_if_cond(right, n);
        }
        TIfCond::IfLet { subj, .. }
        | TIfCond::IsNone { subj }
        | TIfCond::Matches { subj, .. } => count_spawn_sites_expr(subj, n),
    }
}

fn resident_safe_expr_list(exprs: &[TExpr], callees: &HashSet<String>) -> bool {
    exprs.iter().all(|e| resident_safe_expr(e, callees))
}

fn resident_safe_task_list_expr(tasks: &TExpr, callees: &HashSet<String>) -> bool {
    jit_list_task_int_type(&tasks.ty) && resident_safe_expr(tasks, callees)
}

fn resident_safe_select_wait(builder: &TExpr, callees: &HashSet<String>) -> bool {
    let (recvs, afters) = collect_select_arms_jit(builder);
    (!recvs.is_empty() || !afters.is_empty())
        && recvs
            .iter()
            .all(|ch| jit_concurrency_type(&ch.ty) && resident_safe_expr(ch, callees))
        && afters.iter().all(|(ms, value)| {
            matches!(&ms.ty, Type::Int)
                && resident_safe_expr(ms, callees)
                && value
                    .map(|v| matches!(&v.ty, Type::Int) && resident_safe_expr(v, callees))
                    .unwrap_or(true)
        })
}

pub(crate) fn collect_select_arms_jit<'a>(
    builder: &'a TExpr,
) -> (Vec<&'a TExpr>, Vec<(&'a TExpr, Option<&'a TExpr>)>) {
    let mut recvs = Vec::new();
    let mut afters = Vec::new();
    let mut cur = builder;
    loop {
        match &cur.kind {
            TExprKind::SelectStart => break,
            TExprKind::SelectRecv {
                builder: inner,
                channel,
            } => {
                recvs.push(channel.as_ref());
                cur = inner;
            }
            TExprKind::SelectAfter {
                builder: inner,
                millis,
                value,
            } => {
                afters.push((millis.as_ref(), value.as_deref()));
                cur = inner;
            }
            TExprKind::SelectRead { builder: inner, .. } => {
                cur = inner;
            }
            _ => break,
        }
    }
    (recvs, afters)
}

pub(crate) fn resident_safe_spawn_lambda(lam: &TJitSpawnLambda, callees: &HashSet<String>) -> bool {
    if lam.captures.len() > 4 {
        return false;
    }
    if !lam
        .captures
        .iter()
        .all(|c| jit_value_type(&c.ty) && resident_safe_capture_policy(c))
    {
        return false;
    }
    if !lam.params.iter().all(|(_, ty)| jit_value_type(ty)) {
        return false;
    }
    if !jit_value_type(&lam.ret) {
        return false;
    }
    match &lam.body {
        TJitSpawnBody::Expr(e) => resident_safe_expr(e, callees),
        TJitSpawnBody::Block { prefix, tail } => {
            prefix.iter().all(|s| resident_safe_stmt(s, callees))
                && tail.as_ref().is_none_or(|t| resident_safe_expr(t, callees))
        }
    }
}

fn resident_safe_capture_policy(c: &JitSpawnCapture) -> bool {
    if c.clone_at_spawn {
        matches!(&c.ty, Type::Apply { name, .. } if name == "Sender")
            || matches!(&c.ty, Type::Shared(_))
            || matches!(
                &c.ty,
                Type::String
                    | Type::Int
                    | Type::IntN { .. }
                    | Type::Float
                    | Type::Float32
                    | Type::Bool
                    | Type::Char
            )
            || opaque_host_handle_ty(&c.ty)
    } else {
        true
    }
}

/// Resident JIT opaque handles: i64 slots, clone = copy.
pub(crate) fn opaque_host_handle_ty(ty: &Type) -> bool {
    match ty {
        Type::Named(n) => matches!(
            n.as_str(),
            "NullBackend"
                | "TuiBackend"
                | "GtkBackend"
                | "UiNode"
                | "EventResult"
                | "InputEvent"
                | "AriaRole"
                | "Point"
                | "Size"
                | "Rect"
                | "SizeConstraint"
                | "WebApp"
                | "WebPage"
                | "DevServer"
                | "AsyncPolicy"
                | "Overflow"
                | "FailurePolicy"
                | "DispatchState"
                | "HookPolicy"
                | "HookDecision"
                | "HookOutcome"
                | "EventConfigError"
                | "Layout"
                | "GameScene"
                | "GameFrame"
                | "GameBackend"
                | "RaylibWindow"
                | "TcpListener"
                | "TcpStream"
                | "SocketAddr"
                | "UdpSocket"
                | "UnixListener"
                | "UnixStream"
                | "HTTPMux"
                | "HTTPServer"
                | "HTTPHandler"
                | "HTTPRequest"
                | "HTTPResponse"
                | "HTTPBody"
                | "HTTPHeaders"
                | "HTTPCorsPolicy"
                | "WsConn"
                | "WsMessage"
        ),
        Type::Apply { name, .. } => matches!(
            name.as_str(),
            "Signal"
                | "Derived"
                | "Computed"
                | "Effect"
                | "Loadable"
                | "Event"
                | "AsyncEvent"
                | "Hook"
                | "DecisionHook"
                | "Watch"
        ),
        _ => false,
    }
}

fn is_process_result_string_field(expr: &TExpr) -> bool {
    matches!(
        &expr.kind,
        TExprKind::Field {
            recv,
            field,
            boxed: false
        } if matches!(&recv.ty, Type::Named(n) if n == "ProcessResult")
            && matches!(field.as_str(), "output" | "errors")
    )
}

/// Body of `child.stdout.lines()` — loop var type is erased to Unit in TIR.
fn resident_safe_process_lines_body(stmt: &TStmt, callees: &HashSet<String>) -> bool {
    match stmt {
        TStmt::ExprStmt(e) => match &e.kind {
            TExprKind::Print(inner) => match &inner.kind {
                TExprKind::BuiltinMethod { op, recv, args }
                    if matches!(
                        op,
                        TBuiltinOp::Trim | TBuiltinOp::ToUpper | TBuiltinOp::ToLower
                    ) && args.is_empty()
                        && matches!(&recv.kind, TExprKind::Local(_)) =>
                {
                    true
                }
                TExprKind::Local(_) => true,
                _ => resident_safe_expr(e, callees),
            },
            TExprKind::Try { inner, .. } => match &inner.kind {
                TExprKind::HandleMethod {
                    op: THandleOp::FileWriterWriteLine,
                    args,
                    ..
                } if args.len() == 1 => match &args[0].kind {
                    TExprKind::BuiltinMethod { op, recv, args: a }
                        if matches!(op, TBuiltinOp::ToUpper | TBuiltinOp::ToLower | TBuiltinOp::Trim)
                            && a.is_empty()
                            && matches!(&recv.kind, TExprKind::Local(_)) =>
                    {
                        true
                    }
                    TExprKind::Local(_) => true,
                    _ => resident_safe_expr(inner, callees),
                },
                _ => resident_safe_expr(e, callees),
            },
            TExprKind::HandleMethod {
                op: THandleOp::FileWriterWriteLine,
                args,
                ..
            } if args.len() == 1 => match &args[0].kind {
                TExprKind::BuiltinMethod { op, recv, args: a }
                    if matches!(op, TBuiltinOp::ToUpper | TBuiltinOp::ToLower | TBuiltinOp::Trim)
                        && a.is_empty()
                        && matches!(&recv.kind, TExprKind::Local(_)) =>
                {
                    true
                }
                TExprKind::Local(_) => true,
                _ => resident_safe_expr(e, callees),
            },
            _ => resident_safe_expr(e, callees),
        },
        TStmt::Assign { value, .. } => resident_safe_expr(value, callees),
        TStmt::If {
            cond,
            then_body,
            else_body,
            ..
        } => {
            let cond_ok = match cond {
                TIfCond::Plain(e) => match &e.kind {
                    TExprKind::BuiltinMethod { op, recv, args }
                        if matches!(op, TBuiltinOp::Contains)
                            && args.len() == 1
                            && matches!(&recv.kind, TExprKind::Local(_))
                            && matches!(&args[0].ty, Type::String)
                            && resident_safe_expr(&args[0], callees) =>
                    {
                        true
                    }
                    _ => matches!(&e.ty, Type::Bool) && resident_safe_expr(e, callees),
                },
                _ => false,
            };
            cond_ok
                && then_body
                    .iter()
                    .all(|s| resident_safe_process_lines_body(s, callees))
                && else_body.as_ref().is_none_or(|b| {
                    b.iter()
                        .all(|s| resident_safe_process_lines_body(s, callees))
                })
        }
        _ => resident_safe_stmt(stmt, callees),
    }
}

fn resident_safe_handle_op(op: &THandleOp, recv: &TExpr, args: &[TExpr]) -> bool {
    match op {
        THandleOp::TaskJoin
        | THandleOp::TaskCancel
        | THandleOp::TaskDetach
        | THandleOp::TaskPause
        | THandleOp::TaskResume
        | THandleOp::TaskTrace => {
            args.is_empty() && jit_concurrency_type(&recv.ty)
        }
        THandleOp::ChannelReceive => {
            args.is_empty() && matches!(&recv.ty, Type::Apply { name, .. } if name == "Receiver")
        }
        THandleOp::SenderSend => {
            args.len() == 1 && matches!(&recv.ty, Type::Apply { name, .. } if name == "Sender")
        }
        THandleOp::SolverNew => args.is_empty() && recv.ty == Type::Int,
        THandleOp::SolverRequire => args.len() == 1 && recv.ty == Type::Named("Solver".into()) && args[0].ty == Type::Bool,
        THandleOp::SolverFailureCount | THandleOp::SolverStatus => {
            args.is_empty() && recv.ty == Type::Named("Solver".into())
        }
        THandleOp::MeasurementMethod { method } => {
            matches!(&recv.ty, Type::Apply { name, args: targs }
                if name == "Measurement" && targs.as_slice() == [Type::Float])
                && matches!(
                    (method.as_str(), args.len()),
                    ("value" | "uncertainty", 0) | ("add" | "sub" | "mul" | "div", 1)
                )
        }
        THandleOp::PreciseMethod { type_name, method } => {
            ((type_name == "BigInt"
                && recv.ty == Type::Named("BigInt".into())
                && matches!(
                    (method.as_str(), args.len()),
                    ("add" | "sub" | "mul", 1) | ("neg" | "to_string", 0)
                ))
                || (type_name == "Decimal"
                    && recv.ty == Type::Named("Decimal".into())
                    && matches!(
                        (method.as_str(), args.len()),
                        ("add" | "sub" | "mul", 1) | ("to_string", 0)
                    )))
        }
        THandleOp::CivilTimeMethod { method, .. } => {
            // Civil date/time/zoned methods are host-backed; arity is checked in lower.
            matches!(args.len(), 0 | 1 | 2)
                && matches!(
                    method.as_str(),
                    "year"
                        | "month"
                        | "day"
                        | "hour"
                        | "minute"
                        | "second"
                        | "to_string"
                        | "weekday"
                        | "day_of_year"
                        | "timestamp"
                        | "to_timestamp"
                        | "to_unix_ms"
                        | "date"
                        | "time"
                        | "format"
                        | "format_rfc3339"
                        | "iso_weekday"
                        | "iso_week"
                        | "offset_seconds"
                        | "elapsed_millis"
                        | "add_days"
                        | "add_months"
                        | "add_years"
                        | "add_period"
                        | "diff_days"
                        | "with_time"
                )
        }
        THandleOp::RegexMethod { method, .. } => {
            matches!(
                (method.as_str(), args.len()),
                (
                    "matches"
                        | "match"
                        | "is_match"
                        | "find"
                        | "find_all"
                        | "split"
                        | "start"
                        | "end",
                    0..=1
                ) | ("group" | "name", 1)
                    | ("replace_all" | "split_limit" | "replace_all_with", 2)
            )
        }
        THandleOp::FileReaderReadLine if args.is_empty() => true,
        THandleOp::FileWriterWriteLine if args.len() == 1 => true,
        THandleOp::FileWriterFlush if args.is_empty() => true,
        THandleOp::StdinReadLine if args.is_empty() => true,
        THandleOp::StdoutWrite
        | THandleOp::StdoutWriteLine
        | THandleOp::StdoutWriteBytes
            if args.len() == 1 =>
        {
            true
        }
        THandleOp::StdoutFlush | THandleOp::StdoutIsTty if args.is_empty() => true,
        THandleOp::StderrWrite
        | THandleOp::StderrWriteLine
        | THandleOp::StderrWriteBytes
            if args.len() == 1 =>
        {
            true
        }
        THandleOp::StderrFlush | THandleOp::StderrIsTty if args.is_empty() => true,
        THandleOp::DBQuery | THandleOp::DBQueryOne | THandleOp::DBExecute if args.len() == 2 => true,
        THandleOp::DBBegin
        | THandleOp::DBCommit
        | THandleOp::DBRollback
        | THandleOp::DBClose
        | THandleOp::DBValueInt
        | THandleOp::DBValueFloat
        | THandleOp::DBValueText
        | THandleOp::DBValueBool
        | THandleOp::DBValueIsNull
            if args.is_empty() =>
        {
            true
        }
        THandleOp::DurationNew { .. } => args.is_empty(),
        THandleOp::DurationIn { .. } => args.len() <= 1,
        THandleOp::AllocAlloc => args.len() == 1,
        THandleOp::AllocReset | THandleOp::ClockNow | THandleOp::RngBool => args.is_empty(),
        THandleOp::RngInt => args.len() == 2,
        THandleOp::ClockTick
        | THandleOp::ClockAdvance
        | THandleOp::ClockWait
        | THandleOp::RngPick
        | THandleOp::RngShuffle => args.len() == 1,
        THandleOp::ExpiringMethod { method } => {
            matches!(method.as_str(), "get" | "is_valid") && args.len() == 1
        }
        THandleOp::ProcessSpecMethod { method } => {
            matches!(
                (method.as_str(), args.len()),
                (
                    "stdout" | "stderr" | "stdin" | "timeout" | "output_limit" | "cwd"
                        | "env_remove",
                    1,
                ) | ("env", 2)
                    | ("terminal", 0 | 1)
                    | (
                        "env_clear" | "detached" | "capabilities" | "run" | "run_checked"
                            | "spawn",
                        0,
                    )
            )
        }
        THandleOp::ProcessChildMethod { method } => {
            matches!(
                (method.as_str(), args.len()),
                ("wait" | "id" | "kill" | "terminate" | "interrupt", 0)
            )
        }
        THandleOp::TerminalSessionResize => args.len() == 1,
        THandleOp::EventMethod { method } => {
            matches!(
                (method.as_str(), args.len()),
                ("cancel" | "is_active", 0)
            )
        }
        THandleOp::WatchMethod { method, .. } => match method.as_str() {
            "poll" | "events" | "cancel" | "is_active" | "summary" => args.is_empty(),
            "add" => args.len() == 1,
            "on" | "once" => args.len() == 2,
            _ => false,
        },
        THandleOp::ArgsSpecFlag
        | THandleOp::ArgsSpecPositional
            if args.len() == 2 =>
        {
            true
        }
        THandleOp::ArgsSpecFlagShort
        | THandleOp::ArgsSpecOption
        | THandleOp::ArgsSpecOptionInt
        | THandleOp::ArgsSpecRepeat
        | THandleOp::ArgsSpecSubcommand
            if args.len() == 3 =>
        {
            true
        }
        THandleOp::ArgsSpecOptionDefault | THandleOp::ArgsSpecOptionChoice if args.len() == 4 => {
            true
        }
        THandleOp::ArgsSpecVersion
        | THandleOp::ArgsSpecCompletion
        | THandleOp::ArgsSpecParse
        | THandleOp::ArgsSpecParseOrExit
            if args.len() == 1 =>
        {
            true
        }
        THandleOp::ArgsSpecHelp if args.is_empty() => true,
        THandleOp::ParsedArgsFlag
        | THandleOp::ParsedArgsOption
        | THandleOp::ParsedArgsOptionInt
        | THandleOp::ParsedArgsOptionFloat
        | THandleOp::ParsedArgsOptions
        | THandleOp::ParsedArgsPositional
            if args.len() == 1 =>
        {
            true
        }
        THandleOp::ParsedArgsSubcommand if args.is_empty() => true,
        THandleOp::SketchMethod { sketch, method } => match method.as_str() {
            "add" => args.len() == 1,
            "count" if sketch == "HyperLogLog" => args.is_empty(),
            "count" if sketch == "CountMinSketch" => args.len() == 1,
            "quantile" => args.len() == 1,
            "sample" => args.is_empty(),
            _ => false,
        },
        THandleOp::DataTreeField
        | THandleOp::JSONField
        | THandleOp::DataTreeAt
        | THandleOp::JSONAt => args.len() == 1,
        THandleOp::DataTreeInt
        | THandleOp::JSONInt
        | THandleOp::DataTreeText
        | THandleOp::JSONText
        | THandleOp::DataTreeBool
        | THandleOp::JSONBool
        | THandleOp::DataTreeFloat
        | THandleOp::JSONFloat => args.is_empty(),
        THandleOp::SerdeEncode => args.is_empty(),
        THandleOp::DataTreeDecode(_) => args.is_empty(),
        THandleOp::JSONReaderNext
        | THandleOp::JSONLReaderNext
        | THandleOp::CSVReaderNext
        | THandleOp::DataStreamNext
        | THandleOp::XMLReaderNext
        | THandleOp::CBORReaderNext
        | THandleOp::JSONWriterFlush
        | THandleOp::JSONWriterFinish
        | THandleOp::JSONLWriterFlush
        | THandleOp::JSONLWriterFinish
        | THandleOp::CSVWriterFlush
        | THandleOp::CSVWriterFinish
        | THandleOp::XMLWriterFlush
        | THandleOp::XMLWriterFinish
        | THandleOp::CBORWriterFlush
        | THandleOp::CBORWriterFinish => args.is_empty(),
        THandleOp::JSONWriterWrite
        | THandleOp::JSONLWriterWrite
        | THandleOp::CSVWriterWrite
        | THandleOp::XMLWriterWrite
        | THandleOp::CBORWriterWrite => args.len() == 1,
        THandleOp::PathFrom => matches!(&recv.ty, Type::String) && args.is_empty(),
        THandleOp::PathJoin => args.len() == 1 && matches!(&args[0].ty, Type::String),
        THandleOp::PathParent
        | THandleOp::PathExtension
        | THandleOp::PathStem
        | THandleOp::PathToString
        |         THandleOp::PathWalk => args.is_empty(),
        THandleOp::PathWriteAtomic => args.len() == 1,
        THandleOp::GameSceneNew => args.is_empty() && matches!(&recv.ty, Type::String),
        THandleOp::GameReplayRecord => args.is_empty() && matches!(&recv.ty, Type::String),
        THandleOp::GameBackendHeadless => args.is_empty(),
        THandleOp::GameSceneOnFrame => {
            // AOT keeps the frame lambda in TIR args; JIT registers via spawn-site.
            args.len() <= 1
        }
        THandleOp::GameSceneComponent | THandleOp::GameSceneQuery => {
            args.len() == 1 && matches!(&args[0].ty, Type::String)
        }
        THandleOp::GameAssetsImage | THandleOp::GameAssetsSound => {
            args.len() == 1 && matches!(&args[0].ty, Type::String)
        }
        THandleOp::GameInputBind => {
            args.len() == 2
                && matches!(&args[0].ty, Type::String)
                && matches!(&args[1].ty, Type::String)
        }
        THandleOp::GameInputPressed => args.len() == 1 && matches!(&args[0].ty, Type::String),
        THandleOp::ReactiveGet => args.is_empty(),
        THandleOp::ReactiveSet => args.len() == 1,
        THandleOp::ReactiveEffectMethod { .. } => args.is_empty(),
        THandleOp::EventMethod { .. } => true,
        THandleOp::LayoutMethod { .. } => true,
        THandleOp::LoadableMethod { .. } => true,
        THandleOp::UiBackendMethod { .. } => true,
        THandleOp::DevServerMethod { .. } => true,
        THandleOp::WebAppMethod { .. } => true,
        THandleOp::ReaderOver
        | THandleOp::ReaderReadU8
        | THandleOp::ReaderReadU16Le
        | THandleOp::ReaderReadU16Be
        | THandleOp::ReaderReadU32Le
        | THandleOp::ReaderReadU32Be
        | THandleOp::ReaderReadU64Le
        | THandleOp::ReaderReadU64Be
        | THandleOp::ReaderRemaining
        | THandleOp::ReaderAtEnd
        | THandleOp::CursorOver
        | THandleOp::CursorSkipWs => args.is_empty(),
        THandleOp::ReaderTake | THandleOp::CursorTakeUntil => args.len() == 1,
        THandleOp::CursorTakePattern { .. } | THandleOp::ReaderTakePattern { .. } => {
            args.is_empty()
        }
        THandleOp::ReflectValueTypeName
        | THandleOp::ReflectValueDisplay
        | THandleOp::ReflectValueFields
        | THandleOp::ReflectFieldName
        | THandleOp::ReflectFieldValue => args.is_empty(),
        THandleOp::UrlMimeMethod { method, .. } => match method.as_str() {
            "to_string" | "host" | "path" | "query_pairs" | "path_segments" | "fragment"
            | "essence"
                if args.is_empty() =>
            {
                true
            }
            "join" | "param" if args.len() == 1 => true,
            _ => false,
        },
        THandleOp::TcpListenerAccept
        | THandleOp::TcpListenerLocalAddr
        | THandleOp::TcpStreamClose => args.is_empty(),
        THandleOp::TcpStreamReadText | THandleOp::TcpStreamWriteAllBytes if args.len() == 1 => true,
        THandleOp::HTTPClientMethod { kind, method } => match (kind.as_str(), method.as_str()) {
            ("HTTPResponse", "status" | "body" | "cookies") if args.is_empty() => true,
            ("HTTPResponse", "header") if args.len() == 1 => true,
            ("HTTPResponse", "json") if args.len() <= 1 => true,
            ("HTTPBody", "text" | "json") if args.len() == 1 => true,
            ("HTTPRequest", "body") if args.len() == 1 => true,
            ("HTTPRequest", "form" | "cookie" | "header") if args.len() == 2 => true,
            ("HTTPRequest", "redirects" | "connect_timeout" | "read_timeout")
                if args.len() == 1 =>
            {
                true
            }
            ("HTTPRequest", "send") if args.is_empty() => true,
            _ => false,
        },
        THandleOp::HTTPServerMethod { kind, method } => match (kind.as_str(), method.as_str()) {
            ("HTTPMux", m)
                if matches!(
                    m,
                    "get" | "post" | "put" | "delete" | "patch" | "head" | "options"
                ) && args.len() == 2 =>
            {
                true
            }
            ("HTTPMux", "middleware") if args.len() == 1 => true,
            ("HTTPHandler", "handle") if args.len() == 1 => true,
            ("HTTPRequest", "body" | "method" | "path" | "trailers" | "body_len" | "json")
                if args.is_empty() =>
            {
                true
            }
            ("HTTPRequest", "param" | "header" | "under_limit") if args.len() == 1 => true,
            ("HTTPBody", "text" | "json") if args.len() == 1 => true,
            ("HTTPResponse", "status" | "body") if args.is_empty() => true,
            ("HTTPResponse", "trailers") if args.len() == 1 => true,
            ("HTTPServer", "local_addr" | "serve") if args.is_empty() => true,
            ("HTTPServer", "shutdown") if args.len() == 1 => true,
            ("WsConn", "recv") if args.is_empty() => true,
            ("WsConn", "send_text") if args.len() == 1 => true,
            ("WsConn", "close") if args.len() == 2 => true,
            ("WsMessage", "is_text" | "text") if args.is_empty() => true,
            _ => false,
        },
        THandleOp::HTTPReqTrailers => args.is_empty(),
        THandleOp::HTTPRespTrailers => args.len() == 1,
        THandleOp::MathMethod { .. } => true,
        _ => false,
    }
}
