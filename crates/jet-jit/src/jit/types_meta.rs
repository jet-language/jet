use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::JITModule;
use cranelift_module::Module;
use jet_codegen::Codegen::TIR::{
    JitProgram, SerdeCodec, TExpr, TExprKind, TFunc, TFuncKind, THandleOp, TNumericOp,
};
use jet_foundation::AST::Type;
use std::collections::HashMap;

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
            _ => {}
        }
    }
    if let TExprKind::CoreCall { module, method, .. } = &init.kind {
        if module == "core.random" {
            match method.as_str() {
                "float_range" | "normal" | "exponential" => return Ok(types::F64),
                "bool" => return Ok(types::I8),
                "bytes" | "sample" | "rng" | "weighted_pick" => return Ok(types::I64),
                "seed" => return Ok(types::I8),
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
    if matches!(&ty, Type::Named(n) if n == "Unit") {
        return None;
    }
    if matches!(&ty, Type::Named(n) if matches!(n.as_str(), "Duration" | "DurationUnit" | "RangeError" | "ParseError")) {
        return Some(types::I64);
    }
    if jit_concurrency_type(&ty) {
        return Some(types::I64);
    }
    // List/FixedList/Iter/View handles share the I64 arena ABI (string elems are
    // string-handle ints). Fn values stay unsupported until call/closure ABI lands.
    if jit_list_native_type(&ty)
        || jit_list_of_int_list_type(&ty)
        || jit_list_task_int_type(&ty)
        || jit_list_record_type(&ty)
        || jit_list_iter_elem_type(&ty).is_some()
        || (jit_struct_type(&ty) && !distinct_bases.contains_key(ty.name().as_str()))
        || jit_tuple_type(&ty)
        || jit_enum_type(&ty)
    {
        return Some(types::I64);
    }
    if matches!(&ty, Type::Map { key, .. } if matches!(key.as_ref(), Type::String)) {
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
    for (_, ty, _) in &tir.params {
        sig.params.push(AbiParam::new(
            meta.clif_ty(ty)
                .ok_or_else(|| format!("jit param type unsupported: {ty:?}"))?,
        ));
    }
    if let Some(ret) = &tir.ret {
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
    struct_fields: &'a HashMap<String, Vec<String>>,
    struct_field_types: &'a HashMap<String, Vec<Type>>,
    enum_variants: &'a HashMap<String, Vec<String>>,
    enum_variant_payload_types: &'a HashMap<String, Vec<Type>>,
    int_constants: &'a HashMap<String, i64>,
    has_generic_instances: bool,
    distinct_bases: &'a HashMap<String, Type>,
}

impl<'a> JitMeta<'a> {
    pub(crate) fn from_program(program: &'a JitProgram) -> Self {
        JitMeta {
            struct_fields: &program.struct_fields,
            struct_field_types: &program.struct_field_types,
            enum_variants: &program.enum_variants,
            enum_variant_payload_types: &program.enum_variant_payload_types,
            int_constants: &program.int_constants,
            has_generic_instances: !program.instance_provenance.is_empty(),
            distinct_bases: &program.distinct_bases,
        }
    }

    pub(crate) fn clif_ty(&self, ty: &Type) -> Option<types::Type> {
        clif_ty_with_distinct(ty, self.distinct_bases)
    }

    pub(crate) fn enum_variant_payload_types(
        &self,
        enum_name: &str,
        variant: &str,
    ) -> Option<&[Type]> {
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

    pub(crate) fn int_constant(&self, rust_name: &str) -> Option<i64> {
        self.int_constants.get(rust_name.strip_prefix("user_").unwrap_or(rust_name)).copied()
    }
    pub(crate) fn has_generic_instances(&self) -> bool { self.has_generic_instances }
    pub(crate) fn distinct_base(&self, name: &str) -> Option<&Type> {
        self.distinct_bases.get(name)
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

    /// Discriminant index from structured enum + variant Jet names.
    pub(crate) fn enum_variant_index(&self, enum_name: &str, variant: &str) -> Option<i64> {
        let variants = self.enum_variants.get(enum_name)?;
        let mangled = format!("user_{variant}");
        variants
            .iter()
            .position(|v| v == variant || v == &mangled || v.strip_prefix("user_") == Some(variant))
            .map(|i| i as i64)
    }

    pub(crate) fn is_enum(&self, name: &str) -> bool {
        self.enum_variants.contains_key(name)
    }

    /// Packed i64 enum ABI (disc in low byte, payload >> 8): every variant is
    /// unit, `Int`, or another packed enum. Excludes F64-heap / multi-payload.
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
        _ => return None,
    };
    fields.iter().position(|f| *f == field)
}
