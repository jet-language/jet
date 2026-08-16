use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::JITModule;
use cranelift_module::Module;
use jet_codegen::Codegen::TIR::{
    JitProgram, SerdeCodec, TExpr, TExprKind, TFnValueKind, TFunc, TFuncKind, THandleOp,
    TMethodRef, TNumericOp, TPlace, TStmt,
};
use jet_foundation::AST::{Item, ProgramBundle, Type};
use jet_foundation::Names::{mangle, mangle_path};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

mod prelude_enum_meta {
    include!(concat!(env!("OUT_DIR"), "/prelude_enum_meta.rs"));
}

pub(crate) use prelude_enum_meta::{
    PRELUDE_DATATREE_ARRAY, PRELUDE_DATATREE_BOOL, PRELUDE_DATATREE_BYTES,
    PRELUDE_DATATREE_FLOAT, PRELUDE_DATATREE_INT, PRELUDE_DATATREE_NULL,
    PRELUDE_DATATREE_OBJECT, PRELUDE_DATATREE_TEXT,
};

static HOOK_INT_PAYLOAD: LazyLock<Vec<Type>> = LazyLock::new(|| vec![Type::Int]);
static HOOK_STR_PAYLOAD: LazyLock<Vec<Type>> = LazyLock::new(|| vec![Type::String]);
static EMPTY_PAYLOAD: LazyLock<Vec<Type>> = LazyLock::new(Vec::new);
// D-SERVICE-AUTHORITY1=A: `ServiceReceipt.Retained(id, until)` — the one
// two-field variant; every other variant carries a single String id.
static SERVICE_RECEIPT_RETAINED_PAYLOAD: LazyLock<Vec<Type>> =
    LazyLock::new(|| vec![Type::String, Type::Int]);
static IO_CONTEXT_PAYLOAD: LazyLock<Vec<Type>> =
    LazyLock::new(|| vec![Type::Named("IOContext".into())]);
static TLS_ROOT_CERTIFICATES_PAYLOAD: LazyLock<Vec<Type>> =
    LazyLock::new(|| vec![Type::Named("TLSRootCertificates".into())]);
static PRELUDE_ENUM_VARIANTS: LazyLock<HashMap<String, Vec<String>>> = LazyLock::new(|| {
    prelude_enum_meta::all()
        .iter()
        .map(|(name, variants)| {
            (
                (*name).to_string(),
                variants.iter().map(|variant| (*variant).to_string()).collect(),
            )
        })
        .collect()
});

fn enum_variant_position<'a>(
    mut variants: impl Iterator<Item = &'a String>,
    variant: &str,
) -> Option<i64> {
    let mangled = mangle_path(variant);
    let flat = jet_foundation::Syntax::generated_suffix(&mangled);
    variants
        .position(|candidate| {
            candidate == variant
                || candidate == &mangled
                || jet_foundation::Syntax::generated_suffix(candidate) == variant
                || jet_foundation::Syntax::generated_suffix(candidate) == flat
        })
        .map(|index| index as i64)
}

pub(crate) fn prelude_enum_variant_index(enum_name: &str, variant: &str) -> Option<i64> {
    let variants = PRELUDE_ENUM_VARIANTS.get(enum_name)?;
    enum_variant_position(variants.iter(), variant)
}

pub(crate) fn prelude_enum_variant_at(enum_name: &str, index: i64) -> Option<&'static str> {
    let (_, variants) = prelude_enum_meta::all()
        .iter()
        .find(|(name, _)| *name == enum_name)?;
    variants.get(usize::try_from(index).ok()?).copied()
}
// D-AUTH-TOKENPOLICY1=A: every AuthError variant uses one tagged heap record
// with typed fields in declaration order, matching the JIT auth bridge carrier.
static AUTH_ERROR_WRONG_AUDIENCE_PAYLOAD: LazyLock<Vec<Type>> =
    LazyLock::new(|| vec![Type::String, Type::String]);
static AUTH_ERROR_WRONG_ISSUER_PAYLOAD: LazyLock<Vec<Type>> =
    LazyLock::new(|| vec![Type::String, Type::Option(Box::new(Type::String))]);

/// `TypeName → per-field #[Redact]` flags in declaration order (parallel to
/// `JitProgram.struct_fields`). Populated from the ProgramBundle before compile
/// so JetDebug can redact without extending `JitProgram` (#729 display_debug).
thread_local! {
    static STRUCT_REDACT: RefCell<HashMap<String, Vec<bool>>> = RefCell::new(HashMap::new());
}

pub(crate) fn install_struct_redact(bundle: &ProgramBundle) {
    let mut map = HashMap::new();
    for module in &bundle.modules {
        for item in &module.items {
            if let Item::Struct(s) = item {
                map.insert(
                    s.name.clone(),
                    s.fields.iter().map(|f| f.redact).collect(),
                );
            }
        }
    }
    STRUCT_REDACT.with(|slot| *slot.borrow_mut() = map);
}

/// Whether field `idx` of `type_name` is redacted. `None` = no metadata available.
pub(crate) fn struct_field_redacted(type_name: &str, idx: usize) -> Option<bool> {
    let user_metadata = STRUCT_REDACT.with(|slot| {
        slot.borrow()
            .get(type_name)
            .and_then(|flags| flags.get(idx).copied())
    });
    user_metadata.or_else(|| {
        jet_foundation::StructuralDebug::jet_debug_field_metadata(type_name)
            .and_then(|fields| fields.get(idx).map(|(_, redacted)| *redacted))
    })
}

use super::safety::{
    jit_concurrency_type, jit_enum_type, jit_list_iter_elem_type, jit_list_native_type,
    jit_list_of_int_list_type, jit_list_record_type, jit_list_task_type,
    jit_optional_scalar_type, jit_result_payload_type, jit_struct_type, jit_tuple_type,
};

pub(crate) fn init_clif_ty(init: &TExpr, meta: &JitMeta<'_>) -> Result<types::Type, String> {
    if let TExprKind::DistinctCtor { base, .. } = &init.kind {
        return meta
            .clif_ty(base)
            .ok_or_else(|| format!("jit distinct base unsupported: {base:?}"));
    }
    // TIR may stamp Unit on Rng handle methods; recover ABI from the op.
    if let TExprKind::HandleMethod { op, .. } = &init.kind {
        match op {
            THandleOp::RngFloat
            | THandleOp::RngFloatRange
            | THandleOp::RngNormal
            | THandleOp::RngExponential => return Ok(types::F64),
            THandleOp::RngBool | THandleOp::RngBoolP => return Ok(types::I8),
            THandleOp::RngInt
            | THandleOp::RngBytes
            | THandleOp::RngSplit
            | THandleOp::RngSample
            | THandleOp::RngWeightedPick
            | THandleOp::RngPick
            | THandleOp::RngShuffle => return Ok(types::I64),
            THandleOp::SketchMethod { method, .. } if method == "quantile" => {
                return Ok(types::F64);
            }
            THandleOp::SketchMethod { method, .. }
                if method == "add" || method == "count" || method == "sample" =>
            {
                return Ok(types::I64);
            }
            _ => {}
        }
    }
    if let TExprKind::CoreCall { module, method, args, .. } = &init.kind {
        if module == "core.math.random" {
            match method.as_str() {
                "float_range" | "normal" | "exponential" => return Ok(types::F64),
                "bool" => return Ok(types::I8),
                "bytes" | "sample" | "rng" | "weighted_pick" => return Ok(types::I64),
                "seed" => return Ok(types::I8),
                _ => {}
            }
        }
        if module == "core.text" {
            match method.as_str() {
                "caseless_eq" | "is_alphabetic" | "is_numeric" | "starts_any" => {
                    return Ok(types::I8);
                }
                "byte_count" | "scalar_count" | "display_width" | "graphemes" | "words"
                | "sentences" | "inspect" | "char_indices" | "lower" | "upper" | "nfc" | "nfkc" | "nfd"
                | "nfkd" | "pad_start" | "center" | "trim" => {
                    // Int / String / List / Result handles all use I64 ABI.
                    let _ = args;
                    return Ok(types::I64);
                }
                _ => {}
            }
        }
    }
    if let TExprKind::DistinctConvert {
        arg,
        op,
        range,
        fallible,
        ..
    } = &init.kind
    {
        if range.is_some() && *fallible {
            return Ok(types::I64);
        }
        return match op {
            TNumericOp::CastAs { dst_rust }
                if matches!(dst_rust.as_str(), "f32" | "f64") =>
            {
                Ok(types::F64)
            }
            TNumericOp::CastAs { .. } => init_clif_ty(arg, meta),
            TNumericOp::InlineRange { .. }
            | TNumericOp::TryFrom { .. }
            | TNumericOp::FloatToInt { .. }
            | TNumericOp::FloatNarrow { .. } => Ok(types::I64),
            _ => Err("jit distinct conversion operation unsupported".to_string()),
        };
    }
    if let Some(t) = clif_ty_with_distinct(&init.ty, meta.distinct_bases) {
        return Ok(t);
    }
    if matches!(&init.ty, Type::List(_)) {
        return Ok(types::I64);
    }
    if let Type::Named(name) = &init.ty {
        return Ok(meta
            .distinct_base(name)
            .and_then(|base| meta.clif_ty(base))
            .unwrap_or(types::I64));
    }
    Err(format!("jit let type unsupported: {:?}", init.ty))
}

pub(crate) fn clif_ty(ty: &Type) -> Option<types::Type> {
    clif_ty_with_distinct(ty, &HashMap::new())
}

pub(crate) fn clif_ty_with_distinct(
    ty: &Type,
    distinct_bases: &HashMap<String, Type>,
) -> Option<types::Type> {
    let ty = ty.erased_inline_ranges();
    let ty = ty
        .quantity_parts()
        .map_or_else(|| ty.clone(), |(base, _)| base.clone());
    if let Type::Named(name) = &ty {
        if let Some(base) = distinct_bases.get(name) {
            return clif_ty_with_distinct(base, distinct_bases);
        }
    }
    if let Type::Tagged { inner, .. } = &ty {
        return clif_ty_with_distinct(inner, distinct_bases);
    }
    if matches!(&ty, Type::Named(n) if n == "Unit") {
        return None;
    }
    if matches!(&ty, Type::Apply { name, .. } if name == jet_foundation::Syntax::TYPE_CHECKED_TEXT) {
        return Some(types::I64);
    }
    // D-RANGE-VALUE1: Range uses a lossless three-value resident ABI
    // (`start: I64`, `end: I64`, `exclusive: I8`). It is deliberately not an
    // I64 arena handle, so callers must use the explicit Range ABI helpers.
    if matches!(&ty, Type::Named(n) if n == jet_foundation::Syntax::TYPE_RANGE) {
        return None;
    }
    if matches!(&ty, Type::Named(n)
        if matches!(
            n.as_str(),
            "Arena" | "Bump" | "Pool" | "Fixed" | "Solver" | jet_foundation::Syntax::TYPE_BITS | jet_foundation::Syntax::TYPE_BYTES | "Mod" | "ModGrant" | "Hasher" | "TestSuite" | "BenchSuite"
        ))
    {
        return Some(types::I64);
    }
    if matches!(&ty, Type::Named(n) if matches!(n.as_str(), "IOContext" | "IOOperation" | "IOError")) {
        return Some(types::I64);
    }
    if matches!(&ty, Type::Shared(_))
        || matches!(&ty, Type::Apply { name, .. }
            if matches!(
                name.as_str(),
                "Pool"
                    | "Id"
                    | "Ptr"
                    | "Stream"
                    | "ExpiringValue"
                    | "ExpiringSecret"
                    | jet_foundation::Syntax::TYPE_RANK
                    | "PriorityQueue"
                    | "Cache"
            ) || name == jet_foundation::Syntax::TYPE_SHARED_GUARD)
    {
        return Some(types::I64);
    }
    if matches!(&ty, Type::Named(n) if matches!(n.as_str(), "Duration" | "DurationUnit" | "RangeError" | "ParseError")) {
        return Some(types::I64);
    }
    if jit_concurrency_type(&ty) {
        return Some(types::I64);
    }
    // List/FixedList/Iter/View and structural-union values share the I64 arena
    // ABI. Function values use an opaque I64 callable-handle ABI.
    // Nested lists (`[[String]]` CSV rows, etc.) are also arena handles.
    if jit_list_native_type(&ty)
        || jit_list_of_int_list_type(&ty)
        || matches!(&ty, Type::List(inner) if jit_list_native_type(inner))
        || matches!(
            &ty,
            Type::List(inner)
                if matches!(
                    inner.as_ref(),
                    Type::Map { key, .. } if matches!(key.as_ref(), Type::String | Type::Int)
                )
        )
        || jit_list_task_type(&ty)
        || jit_list_record_type(&ty)
        || jit_list_iter_elem_type(&ty).is_some()
        || (jit_struct_type(&ty) && !distinct_bases.contains_key(ty.name().as_str()))
        || jit_tuple_type(&ty)
        || jit_enum_type(&ty)
        || matches!(
            &ty,
            Type::Union(_)
                | Type::Fn { .. }
                | Type::TraitObject(_)
        )
    {
        return Some(types::I64);
    }
    if matches!(&ty, Type::Map { key, .. } if matches!(key.as_ref(), Type::String | Type::Int)) {
        return Some(types::I64);
    }
    // Option of Map / list / named handle — packed Option ABI (0 / bits+1).
    if matches!(
        &ty,
        Type::Option(inner)
            if matches!(
                inner.as_ref(),
                Type::Map { key, .. } if matches!(key.as_ref(), Type::String | Type::Int)
            ) || matches!(
                inner.as_ref(),
                Type::List(_) | Type::FixedList { .. } | Type::Named(_)
            )
    ) {
        return Some(types::I64);
    }
    if jit_optional_scalar_type(&ty) {
        return Some(types::I64);
    }
    if matches!(&ty, Type::Result { ok, err }
        if jit_result_payload_type(ok.as_ref()) && jit_result_payload_type(err.as_ref()))
    {
        return Some(types::I64);
    }
    match &ty {
        Type::Int | Type::IntN { .. } | Type::InlineRange { .. } | Type::String => Some(types::I64),
        Type::Float | Type::Float32 => Some(types::F64),
        Type::Bool => Some(types::I8),
        Type::Char => Some(types::I32),
        _ => None,
    }
}

pub(crate) fn func_signature(
    module: &JITModule,
    tir: &TFunc,
    meta: &JitMeta<'_>,
) -> Result<Signature, String> {
    let cc = module.target_config().default_call_conv;
    let mut sig = Signature::new(cc);
    if func_has_receiver(tir) {
        sig.params
            .push(AbiParam::new(receiver_clif_ty(tir, meta)));
    }
    for (_, ty, convention) in &tir.params {
        if matches!(ty, Type::Named(n) if n == jet_foundation::Syntax::TYPE_RANGE) {
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I64));
            sig.params.push(AbiParam::new(types::I8));
            continue;
        }
        if matches!(convention, jet_foundation::AST::AccessConvention::Write)
            && matches!(
                ty,
                Type::Int
                    | Type::IntN { .. }
                    | Type::InlineRange { .. }
                    | Type::Float
                    | Type::Float32
                    | Type::Bool
                    | Type::Char
            )
        {
            sig.params.push(AbiParam::new(types::I64));
        } else {
            sig.params.push(AbiParam::new(
                meta.clif_ty(ty)
                    .ok_or_else(|| format!("jit param type unsupported: {ty:?}"))?,
            ));
        }
    }
    if let Some(ret) = &tir.ret {
        if matches!(ret, Type::Named(n) if n == jet_foundation::Syntax::TYPE_RANGE) {
            sig.returns.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I64));
            sig.returns.push(AbiParam::new(types::I8));
        } else if let Some(clif) = meta.clif_ty(ret) {
            sig.returns.push(AbiParam::new(clif));
        }
    }
    Ok(sig)
}

pub(crate) fn fn_value_signature(
    module: &JITModule,
    ty: &Type,
    meta: &JitMeta<'_>,
) -> Result<Signature, String> {
    let Type::Fn { params, ret, .. } = ty else {
        return Err(format!("jit callable type unsupported: {ty:?}"));
    };
    let mut sig = Signature::new(module.target_config().default_call_conv);
    for param in params {
        sig.params.push(AbiParam::new(
            meta.clif_ty(param)
                .ok_or_else(|| format!("jit callable param unsupported: {param:?}"))?,
        ));
    }
    if let Some(ret) = ret {
        if matches!(ret.as_ref(), Type::Named(n) if n == jet_foundation::Syntax::TYPE_RANGE) {
            return Err("jit callable Range return unsupported".to_string());
        }
        if let Some(clif) = meta.clif_ty(ret) {
            sig.returns.push(AbiParam::new(clif));
        }
    }
    Ok(sig)
}

pub(crate) fn interrupt_callback_signature(
    module: &JITModule,
    ty: &Type,
    meta: &JitMeta<'_>,
) -> Result<Signature, String> {
    let mut sig = fn_value_signature(module, ty, meta)?;
    sig.params.insert(0, AbiParam::new(types::I64));
    Ok(sig)
}

pub(crate) fn func_has_receiver(tir: &TFunc) -> bool {
    match &tir.kind {
        TFuncKind::Method { self_conv, .. } => self_conv.is_some(),
        TFuncKind::TraitMethod { serde, .. } => *serde != Some(SerdeCodec::Decode),
        _ => false,
    }
}

/// Return the resident ABI for an instance receiver. Structs and opaque
/// handles stay I64; numeric distinct receivers use their erased scalar ABI.
/// The AOT receiver is a Rust reference, but the JIT passes the same value
/// representation used by the rest of the TIR scalar operations.
pub(crate) fn receiver_clif_ty(tir: &TFunc, meta: &JitMeta<'_>) -> types::Type {
    let owner = match &tir.kind {
        TFuncKind::Method { owner_type, .. } => Some(owner_type.clone()),
        TFuncKind::TraitMethod { .. } => tir
            .name
            .split_once("::")
            .map(|(owner, _)| Type::Named(owner.to_string())),
        _ => None,
    };
    owner
        .as_ref()
        .and_then(|ty| meta.clif_ty(ty))
        .unwrap_or(types::I64)
}

fn is_i64_option(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Option(inner) if matches!(inner.as_ref(), Type::Int | Type::InlineRange { .. })
    )
}

pub(crate) fn core_call_uses_result_option_abi(module: &str, method: &str) -> bool {
    module == "core.math"
        && matches!(
            method,
            "isqrt"
                | "factorial"
                | "binomial"
                | "checked_abs"
                | "checked_neg"
                | "checked_add"
                | "checked_sub"
                | "checked_mul"
                | "checked_div"
                | "checked_rem"
                | "checked_pow"
        )
}

fn nominal_type_name(ty: &Type) -> Option<&str> {
    match ty {
        Type::Named(name) | Type::Apply { name, .. } => Some(name.as_str()),
        Type::Tagged { inner, .. } => nominal_type_name(inner),
        _ => None,
    }
}

fn method_target_key(recv_ty: &Type, method: &TMethodRef, type_args: &[Type]) -> Option<String> {
    let base = nominal_type_name(recv_ty)?;
    if matches!(recv_ty, Type::Apply { .. }) || !type_args.is_empty() {
        Some(jet_codegen::Codegen::TIR::generic_method_instance_key(
            recv_ty,
            &method.name,
            type_args,
        ))
    } else {
        Some(format!("{base}::{}", method.name))
    }
}

fn result_option_field(expr: &TExpr) -> bool {
    let TExprKind::Field { recv, field, .. } = &expr.kind else {
        return false;
    };
    nominal_type_name(&recv.ty).is_some_and(|name| {
        core_struct_field_uses_result_option_abi(name, field)
    })
}

fn named_fn_target(expr: &TExpr) -> Option<&str> {
    let TExprKind::FnValue { kind } = &expr.kind else {
        return None;
    };
    match kind {
        TFnValueKind::NamedFn { name: Some(name), .. } => Some(name),
        _ => None,
    }
}

fn result_option_expr(
    expr: &TExpr,
    locals: &HashSet<String>,
    targets: &HashSet<String>,
) -> bool {
    if !is_i64_option(&expr.ty) {
        return false;
    }
    match &expr.kind {
        TExprKind::Local(local) => locals.contains(&local.rust_name()),
        TExprKind::Field { .. } => result_option_field(expr),
        TExprKind::Borrow { place, .. }
        | TExprKind::DistinctCtor { arg: place, .. }
        | TExprKind::Clone(place) => result_option_expr(place, locals, targets),
        TExprKind::Call { name, .. } => targets.contains(name),
        TExprKind::CoreCall { module, method, .. } => {
            core_call_uses_result_option_abi(module, method)
        }
        TExprKind::MethodCall {
            recv,
            method,
            type_args,
            ..
        } => method_target_key(&recv.ty, method, type_args)
            .is_some_and(|key| targets.contains(&key)),
        TExprKind::FnValue {
            kind: TFnValueKind::Call { callee, .. },
        } => named_fn_target(callee).is_some_and(|name| targets.contains(name)),
        TExprKind::IfExpr {
            then_value,
            else_value,
            ..
        } => {
            result_option_expr(then_value, locals, targets)
                || result_option_expr(else_value, locals, targets)
        }
        _ => false,
    }
}

fn collect_result_option_calls(
    expr: &TExpr,
    locals: &HashSet<String>,
    targets: &HashSet<String>,
    params: &mut HashSet<(String, usize)>,
) {
    match &expr.kind {
        TExprKind::Call { name, args, .. } => {
            for (index, arg) in args.iter().enumerate() {
                if result_option_expr(&arg.value, locals, targets) {
                    params.insert((name.clone(), index));
                }
                collect_result_option_calls(&arg.value, locals, targets, params);
            }
        }
        TExprKind::MethodCall {
            recv,
            method,
            type_args,
            args,
            ..
        } => {
            if let Some(key) = method_target_key(&recv.ty, method, type_args) {
                for (index, arg) in args.iter().enumerate() {
                    if result_option_expr(&arg.value, locals, targets) {
                        params.insert((key.clone(), index));
                    }
                    collect_result_option_calls(&arg.value, locals, targets, params);
                }
            }
            collect_result_option_calls(recv, locals, targets, params);
        }
        TExprKind::FnValue {
            kind: TFnValueKind::Call { callee, args },
        } => {
            if let Some(name) = named_fn_target(callee) {
                for (index, arg) in args.iter().enumerate() {
                    if result_option_expr(&arg.value, locals, targets) {
                        params.insert((name.to_string(), index));
                    }
                    collect_result_option_calls(&arg.value, locals, targets, params);
                }
            }
            collect_result_option_calls(callee, locals, targets, params);
        }
        TExprKind::Field { recv, .. }
        | TExprKind::Borrow { place: recv, .. }
        | TExprKind::DistinctCtor { arg: recv, .. }
        | TExprKind::Clone(recv) => collect_result_option_calls(recv, locals, targets, params),
        TExprKind::IfExpr {
            then_value,
            else_value,
            ..
        } => {
            collect_result_option_calls(then_value, locals, targets, params);
            collect_result_option_calls(else_value, locals, targets, params);
        }
        _ => {}
    }
}

fn analyze_result_option_stmts(
    stmts: &[TStmt],
    locals: &mut HashSet<String>,
    targets: &HashSet<String>,
    params: &mut HashSet<(String, usize)>,
) -> bool {
    let mut returns_result_option = false;
    for stmt in stmts {
        match stmt {
            TStmt::Let { name, init, .. } => {
                collect_result_option_calls(init, locals, targets, params);
                let place = jet_codegen::Codegen::TIR::local_place(name);
                if result_option_expr(init, locals, targets) {
                    locals.insert(place);
                } else {
                    locals.remove(&place);
                }
            }
            TStmt::Assign {
                place: TPlace::Local(local),
                op: None,
                value,
                ..
            } => {
                collect_result_option_calls(value, locals, targets, params);
                let key = local.rust_name();
                if result_option_expr(value, locals, targets) {
                    locals.insert(key);
                } else {
                    locals.remove(&key);
                }
            }
            TStmt::Return(Some(value)) => {
                collect_result_option_calls(value, locals, targets, params);
                returns_result_option |= result_option_expr(value, locals, targets);
            }
            TStmt::ExprStmt(value) => {
                collect_result_option_calls(value, locals, targets, params);
            }
            TStmt::If {
                then_body,
                else_body,
                ..
            } => {
                let mut then_locals = locals.clone();
                let mut else_locals = locals.clone();
                returns_result_option |= analyze_result_option_stmts(
                    then_body,
                    &mut then_locals,
                    targets,
                    params,
                );
                if let Some(else_body) = else_body {
                    returns_result_option |= analyze_result_option_stmts(
                        else_body,
                        &mut else_locals,
                        targets,
                        params,
                    );
                }
            }
            TStmt::Inline(body)
            | TStmt::DebugOnly(body)
            | TStmt::Unsafe { body, .. }
            | TStmt::SentryPolicy { body, .. }
            | TStmt::Impure(body)
            | TStmt::Region(body) => {
                returns_result_option |=
                    analyze_result_option_stmts(body, locals, targets, params);
            }
            _ => {}
        }
    }
    returns_result_option
}

fn result_option_facts(
    program: &JitProgram,
) -> (HashSet<String>, HashSet<(String, usize)>) {
    let mut targets = HashSet::new();
    let mut params = HashSet::new();
    loop {
        let before_targets = targets.len();
        let before_params = params.len();
        for function in &program.funcs {
            let mut locals = HashSet::new();
            for (index, (name, ty, _)) in function.params.iter().enumerate() {
                if is_i64_option(ty) && params.contains(&(function.name.clone(), index)) {
                    locals.insert(name.clone());
                }
            }
            let returns_result_option = analyze_result_option_stmts(
                &function.body,
                &mut locals,
                &targets,
                &mut params,
            );
            if function.ret.as_ref().is_some_and(is_i64_option) && returns_result_option {
                targets.insert(function.name.clone());
            }
        }
        if targets.len() == before_targets && params.len() == before_params {
            break;
        }
    }
    (targets, params)
}

pub(crate) fn jit_fn_name(name: &str) -> String {
    let suffix = jet_foundation::Syntax::generated_suffix(name);
    jet_foundation::Names::mangle(&format!("jit_fn_{}", suffix.replace("::", "__")))
}

pub(crate) struct JitMeta<'a> {
    trait_method_owners:
        &'a HashMap<(String, String), Vec<String>>,
    iterable_item_types:
        &'a HashMap<(String, String), Type>,
    struct_fields: &'a HashMap<String, Vec<String>>,
    struct_field_types: &'a HashMap<String, Vec<Type>>,
    memo_dependencies:
        &'a HashMap<String, HashMap<String, Vec<String>>>,
    reflection_fields:
        &'a HashMap<String, Vec<jet_foundation::Reflection::ReflectionField>>,
    struct_type_params: &'a HashMap<String, Vec<String>>,
    enum_variants: &'a HashMap<String, Vec<String>>,
    enum_variant_payload_types: &'a HashMap<String, Vec<Type>>,
    int_constants: &'a HashMap<String, i64>,
    constants: &'a HashMap<String, jet_foundation::AST::CtValue>,
    has_generic_instances: bool,
    distinct_bases: &'a HashMap<String, Type>,
    distinct_ranges: &'a HashMap<String, (i64, i64)>,
    result_option_targets: HashSet<String>,
    result_option_params: HashSet<(String, usize)>,
    reflect_paths: &'a HashMap<String, String>,
    /// D-MEMO1=A: the ratified cache bound of each `#Memo fn`, so `f.cache()`
    /// can hand the one Prelude memo store the same bound AOT's
    /// `JetMemo::with_bound` gets. An untouched function has no entry in that
    /// store yet, and reporting a bound it never declared would be an
    /// invention.
    memo_bounds: HashMap<String, Option<usize>>,
}

impl<'a> JitMeta<'a> {
    pub(crate) fn from_program(program: &'a JitProgram) -> Self {
        let (result_option_targets, result_option_params) = result_option_facts(program);
        JitMeta {
            trait_method_owners: &program.trait_method_owners,
            iterable_item_types: &program.iterable_item_types,
            struct_fields: &program.struct_fields,
            struct_field_types: &program.struct_field_types,
            memo_dependencies: &program.memo_dependencies,
            reflection_fields: &program.reflection_fields,
            struct_type_params: &program.struct_type_params,
            enum_variants: &program.enum_variants,
            enum_variant_payload_types: &program.enum_variant_payload_types,
            int_constants: &program.int_constants,
            constants: &program.constants,
            has_generic_instances: !program.instance_provenance.is_empty(),
            distinct_bases: &program.distinct_bases,
            distinct_ranges: &program.distinct_ranges,
            result_option_targets,
            result_option_params,
            reflect_paths: &program.reflect_paths,
            memo_bounds: program
                .funcs
                .iter()
                .filter_map(|func| {
                    func.memo_bound.map(|bound| (func.name.clone(), bound))
                })
                .collect(),
        }
    }

    pub(crate) fn result_option_target(&self, name: &str) -> bool {
        self.result_option_targets.contains(name)
    }

    pub(crate) fn result_option_param(&self, function: &str, index: usize) -> bool {
        self.result_option_params
            .contains(&(function.to_string(), index))
    }

    pub(crate) fn memo_dependents(&self, owner: &str, source: &str) -> &[String] {
        self.memo_dependencies
            .get(owner)
            .and_then(|sources| sources.get(source))
            .map(|values| values.as_slice())
            .unwrap_or(&[])
    }

    /// D-MEMO1=A: `Some(bound)` for a memoized function (`None` inside spells
    /// `bound: none`); `None` when the function is not memoized at all.
    pub(crate) fn memo_bound(&self, function: &str) -> Option<Option<usize>> {
        self.memo_bounds.get(function).copied()
    }

    pub(crate) fn clif_ty(&self, ty: &Type) -> Option<types::Type> {
        clif_ty_with_distinct(ty, self.distinct_bases)
    }

    pub(crate) fn reflect_path(&self, ty: &Type) -> String {
        match ty {
            Type::Named(name) | Type::Apply { name, .. } => self
                .reflect_paths
                .get(name)
                .cloned()
                .unwrap_or_else(|| ty.leaf_name()),
            _ => ty.leaf_name(),
        }
    }

    pub(crate) fn trait_method_owners(
        &self,
        trait_name: &str,
        method_name: &str,
    ) -> Vec<&str> {
        self.trait_method_owners
            .get(&(trait_name.to_string(), method_name.to_string()))
            .into_iter()
            .flatten()
            .map(String::as_str)
            .collect()
    }

    pub(crate) fn iterable_item_type(
        &self,
        collection: &str,
        iterator: &str,
    ) -> Option<&Type> {
        self.iterable_item_types
            .get(&(collection.to_string(), iterator.to_string()))
    }

    pub(crate) fn enum_variant_payload_types(
        &self,
        enum_name: &str,
        variant: &str,
    ) -> Option<&[Type]> {
        if matches!(enum_name, "DataTree" | "JSON" | "TOML" | "YAML" | "CSV") {
            return Some(datatree_payload(variant));
        }
        if enum_name == "DataEvent" {
            return Some(data_event_payload(variant));
        }
        if enum_name == "AuthError" {
            return Some(match variant {
                "MalformedToken" | "UnsupportedToken" | "MissingClaim" | "DecodeError" => {
                    HOOK_STR_PAYLOAD.as_slice()
                }
                "WrongAudience" => AUTH_ERROR_WRONG_AUDIENCE_PAYLOAD.as_slice(),
                "WrongIssuer" => AUTH_ERROR_WRONG_ISSUER_PAYLOAD.as_slice(),
                _ => EMPTY_PAYLOAD.as_slice(),
            });
        }
        if enum_name == "ServiceReceipt" {
            return Some(match variant {
                "Retained" => SERVICE_RECEIPT_RETAINED_PAYLOAD.as_slice(),
                "Enqueued" | "Executed" | "DeadLettered" | "Rejected" | "Unavailable" => {
                    HOOK_STR_PAYLOAD.as_slice()
                }
                _ => EMPTY_PAYLOAD.as_slice(),
            });
        }
        if matches!(enum_name, "SMTPSecurity" | "RecipientPolicy" | "SMTPAuth" | "TLSTrust" | "EmailError") {
            return Some(email_payload(variant));
        }
        if enum_name == "IOError" {
            return Some(match variant {
                "InvalidInput" | "NotFound" | "PermissionDenied" | "TimedOut" | "Cancelled"
                | "Closed" | "Protocol" | "Other" => IO_CONTEXT_PAYLOAD.as_slice(),
                _ => EMPTY_PAYLOAD.as_slice(),
            });
        }
        if enum_name == "IOOperation" {
            return Some(EMPTY_PAYLOAD.as_slice());
        }
        if enum_name == "TLSVersion" {
            return Some(EMPTY_PAYLOAD.as_slice());
        }
        if enum_name == "TLSClientTrust" {
            return Some(match variant {
                "System" => EMPTY_PAYLOAD.as_slice(),
                "SystemPlus" | "CustomOnly" => TLS_ROOT_CERTIFICATES_PAYLOAD.as_slice(),
                _ => EMPTY_PAYLOAD.as_slice(),
            });
        }
        if enum_name == "Key" {
            return Some(key_payload(variant));
        }
        // Builtin generic enums used by UI/event stems — payload shapes for the
        // examples under proof (Int/String). Instantiations match events/loadable.
        if enum_name == "HookOutcome" {
            return Some(match variant {
                "Continue" => HOOK_INT_PAYLOAD.as_slice(),
                "Fail" => HOOK_STR_PAYLOAD.as_slice(),
                _ => EMPTY_PAYLOAD.as_slice(),
            });
        }
        if enum_name == "HookDecision" {
            return Some(match variant {
                "Transform" => HOOK_INT_PAYLOAD.as_slice(),
                "Fail" => HOOK_STR_PAYLOAD.as_slice(),
                _ => EMPTY_PAYLOAD.as_slice(),
            });
        }
        if enum_name == "Loadable" {
            return Some(match variant {
                "Loaded" | "Failed" => HOOK_STR_PAYLOAD.as_slice(),
                _ => EMPTY_PAYLOAD.as_slice(),
            });
        }
        let key = format!("{}::{}", mangle_path(enum_name), mangle_path(variant));
        self.enum_variant_payload_types
            .get(&key)
            .map(|types| types.as_slice())
            .or_else(|| {
                let alt = format!("{enum_name}::{variant}");
                self.enum_variant_payload_types
                    .get(&alt)
                    .map(|types| types.as_slice())
            })
    }

    pub(crate) fn raw_bag_key_type(&self, ty: &Type) -> bool {
        match ty {
            Type::Tagged { inner, .. } => self.raw_bag_key_type(inner),
            Type::Int | Type::IntN { .. } | Type::InlineRange { .. } | Type::Bool | Type::Char => true,
            Type::Named(name) => {
                if let Some(base) = self.distinct_base(name) {
                    return self.raw_bag_key_type(base);
                }
                let Some(variants) = self.enum_variant_names(name) else {
                    return false;
                };
                variants.iter().all(|variant| {
                    let variant = variant
                        .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                        .unwrap_or(variant);
                    self.enum_variant_payload_types(name, variant)
                        .is_some_and(|payloads| {
                            payloads.is_empty()
                                || payloads.iter().all(|payload| self.raw_bag_key_type(payload))
                        })
                })
            }
            _ => false,
        }
    }

    pub(crate) fn int_constant(&self, rust_name: &str) -> Option<i64> {
        self.int_constants
            .get(
                rust_name
                    .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                    .unwrap_or(rust_name),
            )
            .copied()
    }
    pub(crate) fn constant(
        &self,
        rust_name: &str,
    ) -> Option<&jet_foundation::AST::CtValue> {
        self.constants
            .get(
                rust_name
                    .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                    .unwrap_or(rust_name),
            )
    }
    pub(crate) fn has_generic_instances(&self) -> bool { self.has_generic_instances }
    pub(crate) fn distinct_base(&self, name: &str) -> Option<&Type> {
        self.distinct_bases.get(name)
    }
    pub(crate) fn distinct_range(&self, name: &str) -> Option<(i64, i64)> {
        self.distinct_ranges.get(name).copied()
    }

    pub(crate) fn struct_field_index(&self, type_name: &str, field: &str) -> Option<usize> {
        if let Some(fields) = self.struct_fields.get(type_name) {
            let mangled = mangle(field);
            if let Some(i) = fields.iter().position(|f| {
                f == field
                    || f == &mangled
                    || f.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                        == Some(field)
            }) {
                return Some(i);
            }
        }
        // Core prelude structs are not user `Item::Struct`s, so TIR Program
        // omits them — fall back to declaration order from jet_std CommonTypes.
        core_struct_field_index(type_name, field)
    }

    /// How many storage slots one record of this type has.
    ///
    /// A struct literal only needs the slot count, so this view stays total
    /// over the Core table even for the few types whose field types are not
    /// all known (`struct_layout` still declines those).
    pub(crate) fn struct_field_count(&self, type_name: &str) -> Option<usize> {
        if let Some(names) = self.struct_fields.get(type_name) {
            return Some(names.len());
        }
        core_struct_field_names(type_name).map(|names| names.len())
    }

    /// Mangled field names + parallel types for `__jet_Type { __jet_f: … }` Debug show.
    pub(crate) fn struct_layout(&self, type_name: &str) -> Option<(&[String], &[Type])> {
        if let Some(names) = self.struct_fields.get(type_name) {
            let tys = self.struct_field_types.get(type_name)?;
            if names.len() != tys.len() {
                return None;
            }
            return Some((names.as_slice(), tys.as_slice()));
        }
        core_struct_layout(type_name)
    }

    pub(crate) fn reflection_fields(
        &self,
        type_name: &str,
    ) -> Option<&[jet_foundation::Reflection::ReflectionField]> {
        self.reflection_fields.get(type_name).map(Vec::as_slice)
    }

    pub(crate) fn struct_type_params(&self, type_name: &str) -> Option<&[String]> {
        self.struct_type_params.get(type_name).map(Vec::as_slice)
    }

    pub(crate) fn struct_type_id(&self, type_name: &str) -> Option<i64> {
        let mut names: Vec<&str> = self.struct_fields.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
            .iter()
            .position(|name| *name == type_name)
            .map(|index| index as i64 + 1)
    }

    /// Field type by Jet or mangled Rust field name.
    pub(crate) fn struct_field_ty(&self, type_name: &str, field: &str) -> Option<Type> {
        let idx = self.struct_field_index(type_name, field)?;
        self.struct_field_types
            .get(type_name)
            .and_then(|tys| tys.get(idx).cloned())
            .or_else(|| core_struct_field_type(type_name, field))
    }

    /// Discriminant index from the Prelude declaration order or a user enum table.
    pub(crate) fn enum_variant_index(&self, enum_name: &str, variant: &str) -> Option<i64> {
        if let Some(index) = prelude_enum_variant_index(enum_name, variant) {
            return Some(index);
        }
        let variants = self.enum_variants.get(enum_name)?;
        enum_variant_position(variants.iter(), variant)
    }

    pub(crate) fn enum_variant_indices(&self, enum_name: &str, variant: &str) -> Vec<i64> {
        if let Some(index) = self.enum_variant_index(enum_name, variant) {
            return vec![index];
        }
        let source_prefix = format!("{variant}.");
        let generated_prefix = jet_foundation::Names::mangle_path(&source_prefix);
        self.enum_variants
            .get(enum_name)
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(index, candidate)| {
                (candidate.starts_with(&generated_prefix)
                    || candidate
                        .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                        .unwrap_or(candidate)
                        .starts_with(&source_prefix))
                    .then_some(index as i64)
            })
            .collect()
    }

    pub(crate) fn is_enum(&self, name: &str) -> bool {
        matches!(
            name,
            "DataTree"
                | "JSON"
                | "TOML"
                | "YAML"
                | "CSV"
                | "ProcessStreamMode"
                | "TerminalMode"
                | "EncodingFormat"
                | "EncodingErrorKind"
                | "DataEvent"
                | "Key"
                | "HookOutcome"
                | "HookDecision"
                | "Loadable"
                | "Overflow"
                | "FailurePolicy"
                | "EventResult"
                | "DispatchState"
                | "ServiceReceipt"
                | "SMTPSecurity"
                | "RecipientPolicy"
                | "SMTPAuth"
                | "TLSTrust"
                | "EmailError"
                | "IOOperation"
                | "IOError"
                | "TLSVersion"
                | "TLSClientTrust"
                | jet_foundation::Syntax::TYPE_TASK_FAILURE
        ) || self.enum_variants.contains_key(name)
    }

    /// Packed i64 enum ABI (disc in low byte, payload >> 8): every variant is
    /// unit, `Int`, `String` handle, or another packed enum. Excludes F64-heap /
    /// multi-payload.
    pub(crate) fn enum_packed_showable(&self, name: &str) -> bool {
        let Some(variants) = self.enum_variant_names(name) else {
            return false;
        };
        for variant in variants {
            let vname = variant
                .strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
                .unwrap_or(variant.as_str());
            let payloads = self.enum_variant_payload_types(name, vname).unwrap_or(&[]);
            match payloads {
                [] => {}
                [Type::Int] => {}
                [Type::String] => {}
                [Type::Named(inner)]
                    if self.is_enum(inner)
                        || jet_foundation::StructuralDebug::jet_debug_field_metadata(inner)
                            .is_some() => {}
                _ => return false,
            }
        }
        true
    }

    /// The record carrier: the value is a host struct handle laid out as
    /// `[disc:i64, payload…]` instead of one i64 with the payload packed beside
    /// the disc byte. `pack_enum_record` writes it and
    /// `unpack_enum_heap_payload_at` reads it back.
    ///
    /// For a SOURCE enum this is one fact about the WHOLE enum, never about one
    /// variant. A discriminant test cannot know which variant the value holds,
    /// so every value of that type has to be read the same way. Answering per
    /// variant made `Shape.Empty` a bare integer while `Shape.Circle(1.0)` was a
    /// record handle, so the `.Empty` arm read the low byte of a handle and the
    /// `.Circle` arm read field 0 of the integer 2 — a wrong answer with no
    /// diagnostic.
    ///
    /// Prelude, foreign and generated `__JetUnion_*` enums keep the per-variant
    /// answer: the host (or `pack_enum_scalar` at the union sites) fixes each
    /// variant's shape and the JIT only marshals what it is handed.
    pub(crate) fn enum_uses_heap(&self, enum_name: &str, variant: &str) -> bool {
        if matches!(
            enum_name,
            "AuthError" | "DataTree" | "JSON" | "TOML" | "YAML" | "CSV" | "EmailError"
        ) {
            return true;
        }
        let declared = self
            .enum_variants
            .get(enum_name)
            .filter(|_| !enum_name.starts_with("__JetUnion_"))
            .filter(|_| !PRELUDE_ENUM_VARIANTS.contains_key(enum_name));
        let Some(declared) = declared else {
            return self.variant_uses_record(enum_name, variant);
        };
        declared.iter().any(|candidate| {
            self.variant_uses_record(enum_name, jet_foundation::Syntax::generated_suffix(candidate))
        })
    }

    /// One variant's payload needs the record carrier: a float cannot share an
    /// i64 with the disc byte (`shl 8` drops its sign/exponent byte), and a
    /// second slot has nowhere to live beside the first.
    fn variant_uses_record(&self, enum_name: &str, variant: &str) -> bool {
        self.enum_variant_payload_types(enum_name, variant)
            .is_some_and(|types| {
                types.len() > 1 || matches!(types.first(), Some(Type::Float | Type::Float32))
            })
    }

    pub(crate) fn enum_variant_names(&self, name: &str) -> Option<&[String]> {
        PRELUDE_ENUM_VARIANTS
            .get(name)
            .map(|variants| variants.as_slice())
            .or_else(|| self.enum_variants.get(name).map(|variants| variants.as_slice()))
    }

    pub(crate) fn enum_names(&self) -> impl Iterator<Item = &String> {
        self.enum_variants
            .keys()
            .chain(PRELUDE_ENUM_VARIANTS.keys().filter(|name| !self.enum_variants.contains_key(*name)))
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jit_discriminants_follow_every_prelude_enum() {
        for (name, source_variants) in prelude_enum_meta::all() {
            let jit_variants = PRELUDE_ENUM_VARIANTS
                .get(*name)
                .expect("generated Prelude enum metadata");
            assert_eq!(jit_variants.len(), source_variants.len(), "{name}");
            for (index, variant) in source_variants.iter().enumerate() {
                assert_eq!(
                    prelude_enum_variant_index(name, variant),
                    Some(index as i64),
                    "{name}.{variant}"
                );
                assert_eq!(
                    prelude_enum_variant_at(name, index as i64),
                    Some(*variant),
                    "{name}[{index}]"
                );
            }
        }
    }

    /// Card 2021: no engine may answer a Core field's type for itself.
    ///
    /// Whenever sema declares a field, the JIT MUST hand back sema's answer
    /// byte for byte — a private row that merely looks right is how
    /// `DataError.row` came to be `Int` against sema's `Option<Int>`, and how
    /// AOT came to read `ProcessResult.output` (a `String`) through the integer
    /// accessor. The residual rows below the delegation are allowed to exist
    /// only where sema answers nothing.
    #[test]
    fn every_core_field_type_is_the_one_sema_declared() {
        let mut checked = 0usize;
        for (spellings, fields) in CORE_STRUCT_FIELDS {
            for name in *spellings {
                for field in *fields {
                    let Some(declared) =
                        jet_codegen::Sema::core_struct_field_type(name, field, &[])
                    else {
                        continue;
                    };
                    assert_eq!(
                        core_struct_field_type(name, field),
                        Some(declared),
                        "{name}.{field} disagrees with the sema declaration"
                    );
                    checked += 1;
                }
            }
        }
        // A delegation that resolved nothing would pass the loop vacuously.
        assert!(checked > 100, "only {checked} Core fields resolved");
    }

    /// The AOT-visible failure of card 2021 in table form: sema declares
    /// `ProcessResult.output` a `String`, so no tier may read it as an `Int`.
    #[test]
    fn process_result_fields_keep_their_declared_types() {
        for (field, declared) in [
            ("code", Type::Int),
            ("output", Type::String),
            ("errors", Type::String),
            ("success", Type::Bool),
            ("timed_out", Type::Bool),
            ("signal", Type::Option(Box::new(Type::Int))),
        ] {
            assert_eq!(
                core_struct_field_type("ProcessResult", field),
                Some(declared.clone()),
                "ProcessResult.{field}"
            );
            assert_eq!(
                jet_codegen::Sema::core_struct_field_type("ProcessResult", field, &[]),
                Some(declared),
                "sema ProcessResult.{field}"
            );
        }
    }
}

/// The one declaration-ordered field table for Prelude ("Core") structs.
///
/// TIR's `ProgramBundle` carries only user `Item::Struct`s, so every engine
/// has to get a Core record's shape from somewhere. Keeping a private copy per
/// lookup site is what let the struct-literal path drift behind the field path
/// (card 1979: `TerminalPolicy`, `EncodingCause`, `TextWidth`,
/// `DataLineOptions` and `AsyncPolicy` all resolved a field index while
/// `struct_layout` had never heard of them). `struct_field_index`,
/// `struct_field_count` and `struct_layout` are now three views of this table.
///
/// Row: (the type-name spellings that share one shape, declaration order).
/// Field order mirrors `jet_std` CommonTypes / sema `core_struct_field`.
/// Types whose rows `jet_foundation::StructuralDebug` already owns — it also
/// owns their redaction policy — are folded in below instead of repeated here.
static CORE_STRUCT_FIELDS: &[(&[&str], &[&str])] = &[
    (&["Err"], &["message", "code", "cause"]),
    // D-RENDERTGT*: UI geometry (Prelude). Do NOT register `Point` — many
    // examples define user `Point { x: Int, … }` and core Float would win.
    (&["Size"], &["width", "height"]),
    (&["Rect"], &["x", "y", "width", "height"]),
    (
        &["SizeConstraint"],
        &["min_width", "min_height", "max_width", "max_height"],
    ),
    (
        &["UiNode"],
        &["kind", "role", "label", "width", "height", "color", "children"],
    ),
    // `Path` is one slot holding its text, the shape `CoreHost::path_record`
    // allocates and `path_string_from_record` reads back, and the same single
    // `inner` field the AOT `JetPath` and the interpreter's `Path` CtValue
    // carry. Registering it here is what makes `~path` (Clone) a record copy
    // instead of a lowering refusal.
    (&["Path"], &["inner"]),
    (&["DirEntry"], &["name", "path", "is_dir"]),
    (
        &["Stat"],
        &[
            "size",
            "modified_ms",
            "created_ms",
            "readonly",
            "is_file",
            "is_dir",
            "is_symlink",
            "kind",
        ],
    ),
    (&["WalkEntry"], &["path", "relative", "is_dir", "depth"]),
    (&["TempDir", "TempFile", "FileLock"], &["path"]),
    (&["LogField"], &["key", "value", "kind", "redacted"]),
    (&["LogSpan"], &["id", "name"]),
    (&["Rng"], &["state"]),
    (&["TestSuite", "BenchSuite"], &["iteration", "result"]),
    (
        &["TLSCertificate"],
        &[
            "der",
            "sha256",
            "spki_sha256",
            "dns_names",
            "valid_from_unix_ms",
            "valid_until_unix_ms",
            "subject",
            "issuer",
        ],
    ),
    (
        &["TLSPeerIdentity"],
        &[
            "verified_server_name",
            "leaf",
            "certificate_chain",
            "cipher_suite",
            "tls_version",
        ],
    ),
    // Mirrors jet_std::ProcessResult field order (Open.rs).
    (
        &["ProcessResult"],
        &["code", "output", "errors", "success", "signal", "timed_out"],
    ),
    (&["TerminalSize"], &["cols", "rows"]),
    (&["TerminalPolicy"], &["size", "mode"]),
    // D-EVENT1: jet_std::JetAsyncPolicy (Prelude ReactiveEventWatch.rs).
    (&["AsyncPolicy"], &["capacity", "overflow"]),
    (&["ModGrant"], &["read"]),
    (&["Envelope"], &["from", "recipients"]),
    (
        &["RecipientReport"],
        &["address", "accepted", "code", "message"],
    ),
    (
        &["SendReport"],
        &[
            "server",
            "accepted",
            "rejected",
            "response_code",
            "response",
            "accepted_at",
        ],
    ),
    (
        &["Limits"],
        &[
            "max_reply_line_bytes",
            "max_reply_lines",
            "max_capabilities",
            "max_recipients",
            "max_message_bytes",
            "max_auth_challenge_bytes",
        ],
    ),
    (
        &["SMTPConfig"],
        &[
            "host",
            "port",
            "security",
            "auth",
            "recipient_policy",
            "trust",
            "limits",
            "dkim",
        ],
    ),
    (
        &["DkimConfig"],
        &["domain", "selector", "private_key", "signed_headers"],
    ),
    // D-ENCSTREAM-SURFACE1 / jet_std::EncodingLimits.
    (
        &["EncodingLimits"],
        &[
            "buffer_bytes",
            "max_depth",
            "max_item_bytes",
            "max_total_bytes",
            "max_expansion_depth",
            "max_expansion_bytes",
        ],
    ),
    (
        &["DataLimits"],
        &[
            "encoding",
            "max_groups",
            "max_sort_rows",
            "max_join_rows",
            "max_output_rows",
        ],
    ),
    (
        &["DataStatus"],
        &[
            "step",
            "path",
            "copy",
            "ownership",
            "trust",
            "fallback",
            "replacement",
        ],
    ),
    (&["DataGroup"], &["key", "count", "sum", "mean"]),
    (
        &["DataLineOptions"],
        &[
            "title", "x_label", "y_label", "markers", "reference", "style", "color", "legend",
        ],
    ),
    (
        &["DataError"],
        &[
            "kind",
            "operation",
            "row",
            "column",
            "index",
            "reason",
            "cause",
        ],
    ),
    (
        &["DataSummary"],
        &[
            "count",
            "sum",
            "mean",
            "min",
            "max",
            "median",
            "variance",
            "stddev",
        ],
    ),
    (
        &["DataTable", "Table", "LazyFrame"],
        &["rows", "missing", "plan"],
    ),
    (&["Series", "DataSeries"], &["values", "missing"]),
    (&["DataColumn"], &["name", "type_name"]),
    (&["DataJoin", "Join"], &["left", "right"]),
    (&["VjpRun"], &["value", "pull", "grads"]),
    (
        &["DataPivotCell"],
        &["row_key", "column_key", "count", "sum", "mean"],
    ),
    (&["EncodingCause"], &["kind", "os_code", "message"]),
    (
        &["EncodingError"],
        &[
            "format",
            "kind",
            "byte_offset",
            "line",
            "column",
            "path",
            "reason",
            "cause",
        ],
    ),
    // D-MIGRATE3=A.
    (&["MigrationStatus"], &["migrated", "from", "steps"]),
    (&["DecodeResult"], &["value", "migration"]),
    (&["TextWidth"], &["ambiguous", "controls"]),
    // D-AUTH-TOKENPOLICY1=A — matches JetAuthClaims / JIT verify_jwt record.
    (
        &["Claims"],
        &[
            "subject",
            "audience",
            "issuer",
            "expires_at",
            "not_before",
            "issued_at",
        ],
    ),
    (&["Rotation"], &["previous", "current"]),
    (
        &["WatchEvent"],
        &["domain", "kind", "path", "detail", "pid", "port"],
    ),
    // D-MEMO1=A: the read-only record `name.cache()` projects. Declaration
    // order matches the TIR evaluator's `memo_stats` and sema's
    // `core_struct_field`, so a `stats.hits` read resolves to the same slot on
    // every tier.
    (
        &[jet_foundation::Syntax::TYPE_MEMO_STATS],
        &["hits", "misses", "size", "bound"],
    ),
];

/// One Prelude struct's shape, resolved once from the table above.
///
/// `types` is `None` while some field's type has no `core_struct_field_type`
/// row (`DecodeResult.value` is the type argument, not a fixed type). Index
/// and count stay total; the layout consumers — Debug show, clone, patch —
/// keep failing the same way they did before rather than guessing an ABI.
struct CoreStructShape {
    names: Vec<String>,
    types: Option<Vec<Type>>,
}

static CORE_STRUCTS: LazyLock<HashMap<&'static str, CoreStructShape>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for (name, fields) in jet_foundation::StructuralDebug::jet_debug_field_metadata_rows() {
        let names = fields.iter().map(|(field, _)| *field).collect::<Vec<_>>();
        map.insert(*name, core_struct_shape(name, &names));
    }
    for (spellings, fields) in CORE_STRUCT_FIELDS {
        for name in *spellings {
            map.insert(*name, core_struct_shape(name, fields));
        }
    }
    map
});

fn core_struct_shape(type_name: &str, fields: &[&str]) -> CoreStructShape {
    CoreStructShape {
        names: fields.iter().map(|field| (*field).to_string()).collect(),
        types: fields
            .iter()
            .map(|field| core_struct_field_type(type_name, field))
            .collect::<Option<Vec<Type>>>(),
    }
}

/// Declaration-ordered field names of a Prelude struct.
fn core_struct_field_names(type_name: &str) -> Option<&'static [String]> {
    CORE_STRUCTS
        .get(type_name)
        .map(|shape| shape.names.as_slice())
}

fn core_struct_field_index(type_name: &str, field: &str) -> Option<usize> {
    core_struct_field_names(type_name)?
        .iter()
        .position(|name| name == field)
}

fn core_struct_layout(type_name: &str) -> Option<(&'static [String], &'static [Type])> {
    let shape = CORE_STRUCTS.get(type_name)?;
    Some((shape.names.as_slice(), shape.types.as_ref()?.as_slice()))
}

/// Declared type of a CORE ("Prelude") struct field, for JIT field reads, the
/// `CORE_STRUCTS` layout above, and print.
///
/// I9: this is a MARSHALLING ADAPTER over sema's table, not a second table.
/// Card 2021 — it used to restate about forty rows sema already declared, and
/// the copies had drifted: `DataError.row`/`.column`/`.index` answered `Int`
/// where sema declares `Option<Int>`, `DataError.cause` answered `Int` where
/// sema declares `Option<EncodingError>`, and `DkimConfig.private_key` dropped
/// the crypto-nominal tag that carries `Secret`'s redaction policy. Each of
/// those types still fits an i64 slot, so nothing rejected them — they simply
/// printed the wrong thing.
///
/// Only rows sema genuinely cannot answer for a bare type name survive here,
/// each because of a stated structural reason, never because a row was easier
/// to copy.
pub(crate) fn core_struct_field_type(type_name: &str, field: &str) -> Option<Type> {
    // The one declaring table (jet-sema CheckerCoreLib/core_types.rs). A user
    // struct never reaches this function: every caller resolves its own
    // `struct_field_ty` first.
    if let Some(ty) = jet_codegen::Sema::core_struct_field_type(type_name, field, &[]) {
        return Some(ty);
    }
    match type_name {
        // A reserved core GENERIC resolves these fields FROM ITS TYPE
        // ARGUMENTS (sema `core_generic_struct_field`), and a JIT field read
        // reaches this function with a bare type name, so the argument is gone.
        // `None` means "ask the expression's own type instead", which is what
        // the callers already do for `DecodeResult.value`.
        "DecodeResult" => match field {
            "value" => None,
            "migration" => Some(Type::Named("MigrationStatus".into())),
            _ => None,
        },
        // `DataJoin<L, R>.left`/`.right` are `L`/`R`. The JIT stores both sides
        // as arena handles, which is what this row describes — not the payload
        // type, which only the `Type::Apply` receiver knows.
        "DataJoin" | "Join" => match field {
            "left" | "right" => Some(Type::Int),
            _ => None,
        },
        "Rotation" => match field {
            "previous" | "current" => Some(Type::Named("KeyRef".into())),
            _ => None,
        },
        // `Path` is one slot holding its text: the shape `CoreHost::path_record`
        // allocates and `path_string_from_record` reads back, and the same single
        // field the AOT `JetPath` and the interpreter's `Path` CtValue carry.
        // Registering it here is what makes `~path` (Clone) a record copy instead
        // of a lowering refusal. It stays LOCAL rather than moving into the sema
        // declaring table on purpose: `Path` is opaque to users, so teaching sema
        // the field would make `path.inner` a legal user field read, which is a
        // surface change no decision ratifies. Same reason as the rows below.
        "Path" => match field {
            "inner" => Some(Type::String),
            _ => None,
        },
        // D-DATAFRAME1=A: `Table<T>`/`Series<T>`/`LazyFrame<T>` are opaque core
        // generics — sema types the VALUE and rejects a field read on it
        // (E0302), so these rows are the JIT handle's own shape, used by Debug
        // show and clone, never by a user field access.
        "DataTable" | "Table" | "LazyFrame" => match field {
            "rows" => Some(Type::List(Box::new(Type::Int))),
            "missing" => Some(Type::Int),
            "plan" => Some(Type::List(Box::new(Type::String))),
            _ => None,
        },
        "Series" | "DataSeries" => match field {
            "values" => Some(Type::List(Box::new(Type::Float))),
            "missing" => Some(Type::Int),
            _ => None,
        },
        // D-RENDERTGT*: sema types `UiNode.label`/`.width`/`.height` for a read
        // and stops there; the remaining declared fields are still part of the
        // record's layout, so Debug show and clone need them.
        "UiNode" => match field {
            "kind" | "role" | "color" => Some(Type::String),
            "children" => Some(Type::List(Box::new(Type::Named("UiNode".into())))),
            _ => None,
        },
        _ => None,
    }
}

/// Core optional fields whose JIT carrier is a one-based `JitResultValue`
/// handle: `ok` is presence and `bits` is the exact payload bits.
pub(crate) fn core_struct_field_uses_result_option_abi(type_name: &str, field: &str) -> bool {
    matches!((type_name, field), ("Claims", "not_before" | "issued_at"))
}

fn datatree_payload(variant: &str) -> &'static [Type] {
    use std::sync::LazyLock;
    static BOOL: LazyLock<[Type; 1]> = LazyLock::new(|| [Type::Bool]);
    static INT: LazyLock<[Type; 1]> = LazyLock::new(|| [Type::Int]);
    static FLOAT: LazyLock<[Type; 1]> = LazyLock::new(|| [Type::Float]);
    static TEXT: LazyLock<[Type; 1]> = LazyLock::new(|| [Type::String]);
    static BYTES: LazyLock<[Type; 1]> =
        LazyLock::new(|| [Type::List(Box::new(Type::IntN { signed: false, bits: 8 }))]);
    static ARRAY: LazyLock<[Type; 1]> =
        LazyLock::new(|| [Type::List(Box::new(Type::Named("DataTree".into())))]);
    static OBJECT: LazyLock<[Type; 1]> = LazyLock::new(|| {
        // Ordered entry list (pair records), matching AOT `Vec<(String, DataTree)>`.
        // `DataEntriesToMap` turns this into the user-facing Map for patterns.
        [Type::List(Box::new(Type::Int))]
    });
    match variant {
        "Null" => &[],
        "Bool" => BOOL.as_slice(),
        "Int" => INT.as_slice(),
        "Float" => FLOAT.as_slice(),
        "Text" => TEXT.as_slice(),
        jet_foundation::Syntax::TYPE_BYTES => BYTES.as_slice(),
        "Array" => ARRAY.as_slice(),
        "Object" => OBJECT.as_slice(),
        _ => &[],
    }
}

fn email_payload(variant: &str) -> &'static [Type] {
    static ERROR: LazyLock<Vec<Type>> = LazyLock::new(|| {
        vec![
            Type::String,
            Type::Option(Box::new(Type::String)),
            Type::Option(Box::new(Type::Int)),
            Type::String,
        ]
    });
    static PASSWORD: LazyLock<Vec<Type>> =
        LazyLock::new(|| vec![Type::String, Type::Named("Secret".into())]);
    static PEM: LazyLock<Vec<Type>> = LazyLock::new(|| {
        vec![Type::List(Box::new(Type::IntN {
            signed: false,
            bits: 8,
        }))]
    });
    match variant {
        "Password" => PASSWORD.as_slice(),
        "SystemPlusCa" => PEM.as_slice(),
        "Configuration"
        | "DNS"
        | "Connect"
        | "TLS"
        | "Auth"
        | "Protocol"
        | "Rejected"
        | "Transient"
        | "TimedOut"
        | "Cancelled"
        | "DeliveryUnknown" => ERROR.as_slice(),
        _ => &[],
    }
}

fn key_payload(variant: &str) -> &'static [Type] {
    use std::sync::LazyLock;
    static CHAR: LazyLock<[Type; 1]> = LazyLock::new(|| [Type::Char]);
    static INT: LazyLock<[Type; 1]> = LazyLock::new(|| [Type::Int]);
    match variant {
        "Char" | "Ctrl" => CHAR.as_slice(),
        "F" => INT.as_slice(),
        _ => &[],
    }
}

fn data_event_payload(variant: &str) -> &'static [Type] {
    use std::sync::LazyLock;
    static BOOL: LazyLock<[Type; 1]> = LazyLock::new(|| [Type::Bool]);
    static INT: LazyLock<[Type; 1]> = LazyLock::new(|| [Type::Int]);
    static FLOAT: LazyLock<[Type; 1]> = LazyLock::new(|| [Type::Float]);
    static STRING: LazyLock<[Type; 1]> = LazyLock::new(|| [Type::String]);
    static BYTES: LazyLock<[Type; 1]> = LazyLock::new(|| {
        [Type::List(Box::new(Type::Int))]
    });
    match variant {
        "Bool" => BOOL.as_slice(),
        "Int" => INT.as_slice(),
        "Float" => FLOAT.as_slice(),
        "Text" | "Key" => STRING.as_slice(),
        jet_foundation::Syntax::TYPE_BYTES => BYTES.as_slice(),
        _ => &[],
    }
}
