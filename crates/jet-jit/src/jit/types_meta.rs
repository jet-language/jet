use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::JITModule;
use cranelift_module::Module;
use jet_codegen::Codegen::TIR::{
    JitProgram, SerdeCodec, TExpr, TExprKind, TFunc, TFuncKind, THandleOp, TNumericOp,
};
use jet_foundation::AST::{Item, ProgramBundle, Type};
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::LazyLock;

static HOOK_INT_PAYLOAD: LazyLock<Vec<Type>> = LazyLock::new(|| vec![Type::Int]);
static HOOK_STR_PAYLOAD: LazyLock<Vec<Type>> = LazyLock::new(|| vec![Type::String]);
static EMPTY_PAYLOAD: LazyLock<Vec<Type>> = LazyLock::new(Vec::new);

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

/// Whether field `idx` of `type_name` is `#[Redact]`. `None` = no metadata installed.
pub(crate) fn struct_field_redacted(type_name: &str, idx: usize) -> Option<bool> {
    STRUCT_REDACT.with(|slot| {
        slot.borrow()
            .get(type_name)
            .and_then(|flags| flags.get(idx).copied())
    })
}

use super::safety::{
    jit_concurrency_type, jit_enum_type, jit_list_iter_elem_type, jit_list_native_type,
    jit_list_of_int_list_type, jit_list_record_type, jit_list_task_int_type,
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
        if module == "core.random" {
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
                | "sentences" | "char_indices" | "lower" | "upper" | "nfc" | "nfkc" | "nfd"
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
            TNumericOp::TryFrom { .. }
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
    if matches!(&ty, Type::Named(n)
        if matches!(
            n.as_str(),
            "Arena" | "Bump" | "Pool" | "Fixed" | "Solver" | "BitSet" | "ByteBuffer"
        ))
    {
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
                    | "SortedSet"
                    | "PriorityQueue"
                    | "Lru"
            ))
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
    // ABI. Noncapturing function values use an I64 code-pointer ABI.
    // Nested lists (`[[String]]` CSV rows, etc.) are also arena handles.
    if jit_list_native_type(&ty)
        || jit_list_of_int_list_type(&ty)
        || matches!(&ty, Type::List(inner) if jit_list_native_type(inner))
        || matches!(
            &ty,
            Type::List(inner)
                if matches!(
                    inner.as_ref(),
                    Type::Map { key, .. } if matches!(key.as_ref(), Type::String)
                )
        )
        || jit_list_task_int_type(&ty)
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
    if matches!(&ty, Type::Map { key, .. } if matches!(key.as_ref(), Type::String)) {
        return Some(types::I64);
    }
    // Option of Map / list / named handle — packed Option ABI (0 / bits+1).
    if matches!(
        &ty,
        Type::Option(inner)
            if matches!(
                inner.as_ref(),
                Type::Map { key, .. } if matches!(key.as_ref(), Type::String)
            ) || matches!(inner.as_ref(), Type::List(_) | Type::Named(_))
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
        Type::Int | Type::IntN { .. } | Type::String => Some(types::I64),
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
        sig.params.push(AbiParam::new(types::I64));
    }
    for (_, ty, convention) in &tir.params {
        if matches!(convention, jet_foundation::AST::AccessConvention::Write)
            && matches!(
                ty,
                Type::Int
                    | Type::IntN { .. }
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
        if let Some(clif) = meta.clif_ty(ret) {
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
        if let Some(clif) = meta.clif_ty(ret) {
            sig.returns.push(AbiParam::new(clif));
        }
    }
    Ok(sig)
}

pub(crate) fn func_has_receiver(tir: &TFunc) -> bool {
    match &tir.kind {
        TFuncKind::Method { self_conv, .. } => self_conv.is_some(),
        TFuncKind::TraitMethod { serde, .. } => *serde != Some(SerdeCodec::Decode),
        _ => false,
    }
}

pub(crate) fn jit_fn_name(name: &str) -> String {
    format!("jet_jit_fn_{}", name.replace("::", "__"))
}

pub(crate) struct JitMeta<'a> {
    trait_method_owners:
        &'a HashMap<(String, String), Vec<String>>,
    iterable_item_types:
        &'a HashMap<(String, String), Type>,
    struct_fields: &'a HashMap<String, Vec<String>>,
    struct_field_types: &'a HashMap<String, Vec<Type>>,
    enum_variants: &'a HashMap<String, Vec<String>>,
    enum_variant_payload_types: &'a HashMap<String, Vec<Type>>,
    int_constants: &'a HashMap<String, i64>,
    constants: &'a HashMap<String, jet_foundation::AST::CtValue>,
    has_generic_instances: bool,
    distinct_bases: &'a HashMap<String, Type>,
    distinct_ranges: &'a HashMap<String, (i64, i64)>,
}

impl<'a> JitMeta<'a> {
    pub(crate) fn from_program(program: &'a JitProgram) -> Self {
        JitMeta {
            trait_method_owners: &program.trait_method_owners,
            iterable_item_types: &program.iterable_item_types,
            struct_fields: &program.struct_fields,
            struct_field_types: &program.struct_field_types,
            enum_variants: &program.enum_variants,
            enum_variant_payload_types: &program.enum_variant_payload_types,
            int_constants: &program.int_constants,
            constants: &program.constants,
            has_generic_instances: !program.instance_provenance.is_empty(),
            distinct_bases: &program.distinct_bases,
            distinct_ranges: &program.distinct_ranges,
        }
    }

    pub(crate) fn clif_ty(&self, ty: &Type) -> Option<types::Type> {
        clif_ty_with_distinct(ty, self.distinct_bases)
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
        let key = format!("user_{enum_name}::user_{variant}");
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
            Type::Int | Type::IntN { .. } | Type::Bool | Type::Char => true,
            Type::Named(name) => {
                if let Some(base) = self.distinct_base(name) {
                    return self.raw_bag_key_type(base);
                }
                let Some(variants) = self.enum_variant_names(name) else {
                    return false;
                };
                variants.iter().all(|variant| {
                    let variant = variant.strip_prefix("user_").unwrap_or(variant);
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
        self.int_constants.get(rust_name.strip_prefix("user_").unwrap_or(rust_name)).copied()
    }
    pub(crate) fn constant(
        &self,
        rust_name: &str,
    ) -> Option<&jet_foundation::AST::CtValue> {
        self.constants
            .get(rust_name.strip_prefix("user_").unwrap_or(rust_name))
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
            let mangled = format!("user_{field}");
            if let Some(i) = fields.iter().position(|f| {
                f == field || f == &mangled || f.strip_prefix("user_") == Some(field)
            }) {
                return Some(i);
            }
        }
        // Core prelude structs are not user `Item::Struct`s, so TIR Program
        // omits them — fall back to declaration order from jet_std CommonTypes.
        core_struct_field_index(type_name, field)
    }

    /// Mangled field names + parallel types for `user_Type { user_f: … }` Debug show.
    pub(crate) fn struct_layout(&self, type_name: &str) -> Option<(&[String], &[Type])> {
        let names = self.struct_fields.get(type_name)?;
        let tys = self.struct_field_types.get(type_name)?;
        if names.len() != tys.len() {
            return None;
        }
        Some((names.as_slice(), tys.as_slice()))
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

    /// Discriminant index from structured enum + variant Jet names.
    pub(crate) fn enum_variant_index(&self, enum_name: &str, variant: &str) -> Option<i64> {
        // Core ProcessStreamMode is not registered on JitProgram; fixed order
        // matches jet_std::ProcessStreamMode { Stream, Inherit, Capture }.
        if enum_name == "ProcessStreamMode" {
            return match variant {
                "Stream" => Some(0),
                "Inherit" => Some(1),
                "Capture" => Some(2),
                _ => None,
            };
        }
        if enum_name == jet_foundation::Syntax::TYPE_TERMINAL_MODE {
            return match variant {
                "Raw" => Some(0),
                "Cooked" => Some(1),
                _ => None,
            };
        }
        // D-TERM1: prelude `Key` / `JetKey` variant order.
        if enum_name == "Key" {
            return match variant {
                "Char" => Some(0),
                "Enter" => Some(1),
                "Escape" => Some(2),
                "Backspace" => Some(3),
                "Tab" => Some(4),
                "Delete" => Some(5),
                "Up" => Some(6),
                "Down" => Some(7),
                "Left" => Some(8),
                "Right" => Some(9),
                "F" => Some(10),
                "Ctrl" => Some(11),
                "Unknown" => Some(12),
                _ => None,
            };
        }
        // D-ENCSTREAM-SURFACE1 core enums (not always on JitProgram).
        if enum_name == "EncodingFormat" {
            return match variant {
                "JSON" => Some(0),
                "JSONL" => Some(1),
                "CSV" => Some(2),
                "XML" => Some(3),
                "CBOR" => Some(4),
                _ => None,
            };
        }
        if enum_name == "EncodingErrorKind" {
            return match variant {
                "Syntax" => Some(0),
                "Truncated" => Some(1),
                "Unsupported" => Some(2),
                "Limit" => Some(3),
                "IO" => Some(4),
                "State" => Some(5),
                _ => None,
            };
        }
        if enum_name == "DataEvent" {
            // Unit variants first (sema registration order), then payload ones.
            return match variant {
                "Null" => Some(0),
                "ArrayStart" => Some(1),
                "ArrayEnd" => Some(2),
                "ObjectStart" => Some(3),
                "ObjectEnd" => Some(4),
                "Bool" => Some(5),
                "Int" => Some(6),
                "Float" => Some(7),
                "Text" => Some(8),
                "Bytes" => Some(9),
                "Key" => Some(10),
                _ => None,
            };
        }
        // D-EVENT1 / D-PENDING1: compiler-builtin enums not always on JitProgram.
        if enum_name == "HookOutcome" {
            return match variant {
                "Continue" => Some(0),
                "Cancel" => Some(1),
                "Fail" => Some(2),
                _ => None,
            };
        }
        if enum_name == "HookDecision" {
            return match variant {
                "Continue" => Some(0),
                "Transform" => Some(1),
                "Cancel" => Some(2),
                "Fail" => Some(3),
                _ => None,
            };
        }
        if enum_name == "Loadable" {
            return match variant {
                "Idle" => Some(0),
                "Loading" => Some(1),
                "Loaded" => Some(2),
                "Failed" => Some(3),
                _ => None,
            };
        }
        if enum_name == "Overflow" {
            return match variant {
                "Block" => Some(0),
                "DropNewest" => Some(1),
                "DropOldest" => Some(2),
                _ => None,
            };
        }
        if enum_name == "FailurePolicy" {
            return match variant {
                "StopFirst" => Some(0),
                "Collect" => Some(1),
                "Log" => Some(2),
                "Ignore" => Some(3),
                _ => None,
            };
        }
        if enum_name == "EventResult" {
            return match variant {
                "Handled" => Some(0),
                "Ignored" => Some(1),
                _ => None,
            };
        }
        if enum_name == "DispatchState" {
            return match variant {
                "Delivered" => Some(0),
                "HandlerFailed" => Some(1),
                "DroppedNewest" => Some(2),
                "DroppedOldest" => Some(3),
                "Closed" => Some(4),
                "Cancelled" => Some(5),
                "DeadlineExceeded" => Some(6),
                _ => None,
            };
        }
        // DataTree (+ format aliases): Null/Bool/Int/Float/Text/Array/Object.
        if matches!(enum_name, "DataTree" | "JSON" | "TOML" | "YAML" | "CSV") {
            return match variant {
                "Null" => Some(0),
                "Bool" => Some(1),
                "Int" => Some(2),
                "Float" => Some(3),
                "Text" => Some(4),
                "Array" => Some(5),
                "Object" => Some(6),
                _ => None,
            };
        }
        let variants = self.enum_variants.get(enum_name)?;
        let mangled = format!("user_{variant}");
        variants
            .iter()
            .position(|v| v == variant || v == &mangled || v.strip_prefix("user_") == Some(variant))
            .map(|i| i as i64)
    }

    pub(crate) fn enum_variant_indices(&self, enum_name: &str, variant: &str) -> Vec<i64> {
        if let Some(index) = self.enum_variant_index(enum_name, variant) {
            return vec![index];
        }
        let prefix = format!("{variant}.");
        self.enum_variants
            .get(enum_name)
            .into_iter()
            .flatten()
            .enumerate()
            .filter_map(|(index, candidate)| {
                candidate
                    .strip_prefix("user_")
                    .unwrap_or(candidate)
                    .starts_with(&prefix)
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
        ) || self.enum_variants.contains_key(name)
    }

    /// Packed i64 enum ABI (disc in low byte, payload >> 8): every variant is
    /// unit, `Int`, `String` handle, or another packed enum. Excludes F64-heap /
    /// multi-payload.
    pub(crate) fn enum_packed_showable(&self, name: &str) -> bool {
        let Some(variants) = self.enum_variants.get(name) else {
            return false;
        };
        for variant in variants {
            let vname = variant.strip_prefix("user_").unwrap_or(variant.as_str());
            let payloads = self.enum_variant_payload_types(name, vname).unwrap_or(&[]);
            match payloads {
                [] => {}
                [Type::Int] => {}
                [Type::String] => {}
                [Type::Named(inner)] if self.is_enum(inner) => {}
                _ => return false,
            }
        }
        true
    }

    pub(crate) fn enum_variant_names(&self, name: &str) -> Option<&[String]> {
        self.enum_variants.get(name).map(|v| v.as_slice())
    }

    pub(crate) fn enum_names(&self) -> impl Iterator<Item = &String> {
        self.enum_variants.keys()
    }

}

/// Field order mirrors `jet_std` CommonTypes / sema `core_struct_field`.
fn core_struct_field_index(type_name: &str, field: &str) -> Option<usize> {
    let fields: &[&str] = match type_name {
        // D-RENDERTGT*: UI geometry (Prelude). Do NOT register `Point` — many
        // examples define user `Point { x: Int, … }` and core Float would win.
        "Size" => &["width", "height"],
        "Rect" => &["x", "y", "width", "height"],
        "SizeConstraint" => &["min_width", "min_height", "max_width", "max_height"],
        "UiNode" => &["kind", "role", "label", "width", "height", "color", "children"],
        "DirEntry" => &["name", "path", "is_dir"],
        "Stat" => &[
            "size",
            "modified_ms",
            "created_ms",
            "readonly",
            "is_file",
            "is_dir",
            "is_symlink",
            "kind",
        ],
        "WalkEntry" => &["path", "relative", "is_dir", "depth"],
        "TempDir" | "TempFile" | "FileLock" => &["path"],
        "LogField" => &["key", "value", "kind", "redacted"],
        "LogSpan" => &["id", "name"],
        "Rng" => &["state"],
        // Mirrors jet_std::ProcessResult field order (Open.rs).
        "ProcessResult" => &["code", "output", "errors", "success", "signal", "timed_out"],
        "TerminalSize" => &["cols", "rows"],
        "TerminalPolicy" => &["size", "mode"],
        // D-ENCSTREAM-SURFACE1 / jet_std::EncodingLimits.
        "EncodingLimits" => &[
            "buffer_bytes",
            "max_depth",
            "max_item_bytes",
            "max_total_bytes",
            "max_expansion_depth",
            "max_expansion_bytes",
        ],
        "DataLimits" => &[
            "encoding",
            "max_groups",
            "max_sort_rows",
            "max_join_rows",
            "max_output_rows",
        ],
        "DataStatus" => &[
            "step",
            "path",
            "copy",
            "ownership",
            "trust",
            "fallback",
            "replacement",
        ],
        "DataGroup" => &["key", "count", "sum", "mean"],
        "DataError" => &[
            "kind",
            "operation",
            "row",
            "column",
            "index",
            "reason",
            "cause",
        ],
        "DataSummary" => &[
            "count",
            "sum",
            "mean",
            "min",
            "max",
            "median",
            "variance",
            "stddev",
        ],
        "DataTable" | "Table" | "LazyFrame" => &["rows", "missing", "plan"],
        "Series" | "DataSeries" => &["values", "missing"],
        "DataColumn" => &["name", "type_name"],
        "DataJoin" | "Join" => &["left", "right"],
        "DataPivotCell" => &["row_key", "column_key", "count", "sum", "mean"],
        "EncodingCause" => &["kind", "os_code", "message"],
        "EncodingError" => &[
            "format",
            "kind",
            "byte_offset",
            "line",
            "column",
            "path",
            "reason",
            "cause",
        ],
        "CBORError" => &["kind", "byte_offset", "path", "reason"],
        // D-VALIDATE1 / D-SERDE2 — path+reason records.
        "FieldError" | "DecodeError" => &["path", "reason"],
        // D-MIGRATE3=A.
        "MigrationStatus" => &["migrated", "from", "steps"],
        "DecodeResult" => &["value", "migration"],
        "TextWidth" => &["ambiguous", "controls"],
        // D-AUTH-TOKENPOLICY1=A — matches JetAuthClaims / JIT verify_jwt record.
        "Claims" => &["subject", "audience", "issuer", "expires_at", "issued_at"],
        "Rotation" => &["previous", "current"],
        "WatchEvent" => &["domain", "kind", "path", "detail", "pid", "port"],
        _ => return None,
    };
    fields.iter().position(|f| *f == field)
}

/// Sema-known CORE struct field types. TIR `struct_field_type` falls back to
/// `Int` when `cx.struct_fields` lacks CORE entries (ProcessResult is not a
/// user struct); recover the real type so JIT field/print/ABI stay total.
pub(crate) fn core_struct_field_type(type_name: &str, field: &str) -> Option<Type> {
    match type_name {
        // No `Point` — collides with user Int Point examples (library, etc.).
        "Size" => match field {
            "width" | "height" => Some(Type::Float),
            _ => None,
        },
        "Rect" => match field {
            "x" | "y" | "width" | "height" => Some(Type::Float),
            _ => None,
        },
        "SizeConstraint" => match field {
            "min_width" | "min_height" | "max_width" | "max_height" => Some(Type::Float),
            _ => None,
        },
        "UiNode" => match field {
            "kind" | "role" | "label" | "color" => Some(Type::String),
            "width" | "height" => Some(Type::Float),
            "children" => Some(Type::List(Box::new(Type::Named("UiNode".into())))),
            _ => None,
        },
        "Claims" => match field {
            "subject" | "issuer" => Some(Type::Option(Box::new(Type::String))),
            "audience" => Some(Type::String),
            "expires_at" => Some(Type::Int),
            "issued_at" => Some(Type::Option(Box::new(Type::Int))),
            _ => None,
        },
        "Rotation" => match field {
            "previous" | "current" => Some(Type::Named("KeyRef".into())),
            _ => None,
        },
        "ProcessResult" => match field {
            "code" => Some(Type::Int),
            "output" | "errors" => Some(Type::String),
            "success" | "timed_out" => Some(Type::Bool),
            "signal" => Some(Type::Option(Box::new(Type::Int))),
            _ => None,
        },
        "TerminalSize" => match field {
            "cols" | "rows" => Some(Type::Int),
            _ => None,
        },
        "TerminalPolicy" => match field {
            "size" => Some(Type::Named("TerminalSize".into())),
            "mode" => Some(Type::Named("TerminalMode".into())),
            _ => None,
        },
        // D-LSDIR1 / D-FSOPS1 — CORE FS records omitted from TIR ProgramBundle.
        "DirEntry" => match field {
            "name" | "path" => Some(Type::String),
            "is_dir" => Some(Type::Bool),
            _ => None,
        },
        "Stat" => match field {
            "size" | "modified_ms" | "created_ms" => Some(Type::Int),
            "readonly" | "is_file" | "is_dir" | "is_symlink" => Some(Type::Bool),
            "kind" => Some(Type::String),
            _ => None,
        },
        "WalkEntry" => match field {
            "path" | "relative" => Some(Type::String),
            "is_dir" => Some(Type::Bool),
            "depth" => Some(Type::Int),
            _ => None,
        },
        "TempDir" | "TempFile" | "FileLock" => match field {
            "path" => Some(Type::String),
            _ => None,
        },
        "EncodingLimits" => match field {
            "buffer_bytes" | "max_depth" | "max_item_bytes" | "max_expansion_depth"
            | "max_expansion_bytes" => Some(Type::Int),
            "max_total_bytes" => Some(Type::Option(Box::new(Type::Int))),
            _ => None,
        },
        "DataLimits" => match field {
            "encoding" => Some(Type::Named("EncodingLimits".into())),
            "max_groups" | "max_sort_rows" | "max_join_rows" | "max_output_rows" => Some(Type::Int),
            _ => None,
        },
        "DataStatus" => match field {
            "step" | "path" | "copy" | "ownership" | "trust" | "fallback" | "replacement" => {
                Some(Type::String)
            }
            _ => None,
        },
        "DataGroup" => match field {
            "key" => Some(Type::String),
            "count" => Some(Type::Int),
            "sum" | "mean" => Some(Type::Float),
            _ => None,
        },
        "DataError" => match field {
            "kind" | "operation" | "reason" => Some(Type::String),
            "row" | "column" | "index" | "cause" => Some(Type::Int),
            _ => None,
        },
        "DataSummary" => match field {
            "count" => Some(Type::Int),
            "sum" | "mean" | "min" | "max" | "median" | "variance" | "stddev" => {
                Some(Type::Float)
            }
            _ => None,
        },
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
        "DataColumn" => match field {
            "name" | "type_name" => Some(Type::String),
            _ => None,
        },
        "DataJoin" | "Join" => match field {
            "left" | "right" => Some(Type::Int),
            _ => None,
        },
        "DataPivotCell" => match field {
            "row_key" | "column_key" => Some(Type::String),
            "count" => Some(Type::Int),
            "sum" | "mean" => Some(Type::Float),
            _ => None,
        },
        "EncodingCause" => match field {
            "kind" | "message" => Some(Type::String),
            "os_code" => Some(Type::Option(Box::new(Type::Int))),
            _ => None,
        },
        "EncodingError" => match field {
            "format" => Some(Type::Named("EncodingFormat".into())),
            "kind" => Some(Type::Named("EncodingErrorKind".into())),
            "byte_offset" => Some(Type::Int),
            "line" | "column" => Some(Type::Option(Box::new(Type::Int))),
            "path" | "reason" => Some(Type::String),
            "cause" => Some(Type::Option(Box::new(Type::Named("EncodingCause".into())))),
            _ => None,
        },
        "CBORError" => match field {
            "kind" => Some(Type::Named("CBORErrorKind".into())),
            "byte_offset" => Some(Type::Int),
            "path" | "reason" => Some(Type::String),
            _ => None,
        },
        "FieldError" | "DecodeError" => match field {
            "path" | "reason" => Some(Type::String),
            _ => None,
        },
        "HTTPShutdownReport" => match field {
            "accepted" | "overloaded" | "completed" | "cancelled" => Some(Type::Int),
            _ => None,
        },
        "MigrationStatus" => match field {
            "migrated" => Some(Type::Bool),
            "from" => Some(Type::String),
            "steps" => Some(Type::List(Box::new(Type::String))),
            _ => None,
        },
        "DecodeResult" => match field {
            // `.value` type is the Apply arg; callers pass expr.ty.
            "value" => None,
            "migration" => Some(Type::Named("MigrationStatus".into())),
            _ => None,
        },
        "WatchEvent" => match field {
            "domain" | "kind" | "path" | "detail" => Some(Type::String),
            "pid" | "port" => Some(Type::Int),
            _ => None,
        },
        _ => None,
    }
}

fn datatree_payload(variant: &str) -> &'static [Type] {
    use std::sync::LazyLock;
    static BOOL: LazyLock<[Type; 1]> = LazyLock::new(|| [Type::Bool]);
    static INT: LazyLock<[Type; 1]> = LazyLock::new(|| [Type::Int]);
    static FLOAT: LazyLock<[Type; 1]> = LazyLock::new(|| [Type::Float]);
    static TEXT: LazyLock<[Type; 1]> = LazyLock::new(|| [Type::String]);
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
        "Array" => ARRAY.as_slice(),
        "Object" => OBJECT.as_slice(),
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
