use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_jit::JITModule;
use cranelift_module::Module;
use jet_codegen::Codegen::TIR::{JitProgram, SerdeCodec, TExpr, TFunc, TFuncKind};
use jet_foundation::AST::Type;
use std::collections::HashMap;

use super::safety::{
    jit_concurrency_type, jit_enum_type, jit_list_int_type, jit_list_task_int_type,
    jit_optional_scalar_type, jit_result_payload_type, jit_struct_type, jit_tuple_type,
};

pub(crate) fn init_clif_ty(init: &TExpr) -> Result<types::Type, String> {
    if let Some(t) = clif_ty(&init.ty) {
        return Ok(t);
    }
    if matches!(&init.ty, Type::List(_)) {
        return Ok(types::I64);
    }
    if matches!(&init.ty, Type::Named(_)) {
        return Ok(types::I64);
    }
    Err(format!("jit let type unsupported: {:?}", init.ty))
}

pub(crate) fn clif_ty(ty: &Type) -> Option<types::Type> {
    if matches!(ty, Type::Named(n) if n == "Unit") {
        return None;
    }
    if matches!(ty, Type::Named(n) if matches!(n.as_str(), "Duration" | "DurationUnit" | "RangeError")) {
        return Some(types::I64);
    }
    if jit_concurrency_type(ty) {
        return Some(types::I64);
    }
    if jit_list_int_type(ty)
        || jit_list_task_int_type(ty)
        || jit_struct_type(ty)
        || jit_tuple_type(ty)
        || jit_enum_type(ty)
    {
        return Some(types::I64);
    }
    if jit_optional_scalar_type(ty) {
        return Some(types::I64);
    }
    if matches!(ty, Type::Result { ok, err }
        if jit_result_payload_type(ok.as_ref()) && jit_result_payload_type(err.as_ref()))
    {
        return Some(types::I64);
    }
    match ty {
        Type::Int | Type::String => Some(types::I64),
        Type::Float => Some(types::F64),
        Type::Bool => Some(types::I8),
        Type::Char => Some(types::I32),
        _ => None,
    }
}

pub(crate) fn func_signature(module: &JITModule, tir: &TFunc) -> Result<Signature, String> {
    let cc = module.target_config().default_call_conv;
    let mut sig = Signature::new(cc);
    if func_has_receiver(tir) {
        sig.params.push(AbiParam::new(types::I64));
    }
    for (_, ty, _) in &tir.params {
        sig.params.push(AbiParam::new(
            clif_ty(ty).ok_or_else(|| format!("jit param type unsupported: {ty:?}"))?,
        ));
    }
    if let Some(ret) = &tir.ret {
        if let Some(clif) = clif_ty(ret) {
            sig.returns.push(AbiParam::new(clif));
        }
    }
    Ok(sig)
}

pub(crate) fn func_has_receiver(tir: &TFunc) -> bool {
    match &tir.kind {
        TFuncKind::Method { self_conv } => self_conv.is_some(),
        TFuncKind::TraitMethod { serde, .. } => *serde != Some(SerdeCodec::Decode),
        _ => false,
    }
}

pub(crate) fn jit_fn_name(name: &str) -> String {
    if name == "run" {
        "jet_jit_main".to_string()
    } else {
        format!("jet_jit_fn_{}", name.replace("::", "__"))
    }
}

pub(crate) struct JitMeta<'a> {
    struct_fields: &'a HashMap<String, Vec<String>>,
    struct_field_types: &'a HashMap<String, Vec<Type>>,
    enum_variants: &'a HashMap<String, Vec<String>>,
    int_constants: &'a HashMap<String, i64>,
    has_generic_instances: bool,
}

impl<'a> JitMeta<'a> {
    pub(crate) fn from_program(program: &'a JitProgram) -> Self {
        JitMeta {
            struct_fields: &program.struct_fields,
            struct_field_types: &program.struct_field_types,
            enum_variants: &program.enum_variants,
            int_constants: &program.int_constants,
            has_generic_instances: !program.instance_provenance.is_empty(),
        }
    }

    pub(crate) fn int_constant(&self, rust_name: &str) -> Option<i64> {
        self.int_constants.get(rust_name.strip_prefix("user_").unwrap_or(rust_name)).copied()
    }
    pub(crate) fn has_generic_instances(&self) -> bool { self.has_generic_instances }

    pub(crate) fn struct_field_index(&self, type_name: &str, field_rust: &str) -> Option<usize> {
        self.struct_fields
            .get(type_name)?
            .iter()
            .position(|f| f == field_rust)
    }

    pub(crate) fn struct_field_type(&self, type_name: &str, field_rust: &str) -> Option<Type> {
        let idx = self.struct_field_index(type_name, field_rust)?;
        self.struct_field_types.get(type_name)?.get(idx).cloned()
    }

    pub(crate) fn enum_variant_disc(&self, prefix: &str) -> Option<i64> {
        let (enum_part, variant) = prefix.rsplit_once("::")?;
        let enum_name = enum_part.strip_prefix("user_").unwrap_or(enum_part);
        let variants = self.enum_variants.get(enum_name)?;
        let variant = if enum_name.contains("__") {
            variant.split_once('(').map_or(variant, |(head, _)| head)
        } else {
            variant
        };
        let variant_key = variant.strip_prefix("user_").unwrap_or(variant);
        variants
            .iter()
            .position(|v| v == variant || v.strip_prefix("user_").unwrap_or(v) == variant_key)
            .map(|i| i as i64)
    }

}
