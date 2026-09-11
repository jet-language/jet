#![allow(dead_code)]

//! Target-facing projections of canonical MIR facts.
//!
//! This module intentionally contains no lowering or semantic inference.  The
//! only source of user type/layout information is `MirProgram`; the Prelude
//! enum table is generated from the same Prelude declarations used by every
//! execution tier.

use cranelift_codegen::ir::{types, AbiParam, Signature};
use cranelift_module::Module;
use jet_foundation::MIR::{
    MirAbi, MirFunction, MirFunctionId, MirProgram, MirType, MirTypeDefKind,
};
use std::collections::HashMap;
use std::sync::LazyLock;

mod prelude_enum_meta {
    include!(concat!(env!("OUT_DIR"), "/prelude_enum_meta.rs"));
}

pub(crate) use prelude_enum_meta::{
    PRELUDE_DATATREE_ARRAY, PRELUDE_DATATREE_BOOL, PRELUDE_DATATREE_BYTES,
    PRELUDE_DATATREE_FLOAT, PRELUDE_DATATREE_INT, PRELUDE_DATATREE_NULL,
    PRELUDE_DATATREE_OBJECT, PRELUDE_DATATREE_TEXT,
};

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

fn core_prelude_key(name: &str) -> &str {
    name.rsplit("::")
        .next()
        .unwrap_or(name)
        .strip_prefix("__jet_")
        .unwrap_or_else(|| name.rsplit("::").next().unwrap_or(name))
}

fn enum_variant_position(variants: &[String], variant: &str) -> Option<i64> {
    let mangled = jet_foundation::Names::mangle_path(variant);
    let flat = jet_foundation::Syntax::generated_suffix(&mangled);
    variants
        .iter()
        .position(|candidate| {
            candidate == variant
                || candidate == &mangled
                || jet_foundation::Syntax::generated_suffix(candidate) == variant
                || jet_foundation::Syntax::generated_suffix(candidate) == flat
        })
        .map(|index| index as i64)
}

pub(crate) fn prelude_enum_variant_index(enum_name: &str, variant: &str) -> Option<i64> {
    PRELUDE_ENUM_VARIANTS
        .get(core_prelude_key(enum_name))
        .and_then(|variants| enum_variant_position(variants, variant))
}

pub(crate) fn prelude_enum_variant_at(enum_name: &str, index: i64) -> Option<&'static str> {
    let key = core_prelude_key(enum_name);
    let (_, variants) = prelude_enum_meta::all().iter().find(|(name, _)| *name == key)?;
    variants.get(usize::try_from(index).ok()?).copied()
}

/// A stable name shared by resident compilation, callbacks, and hot reload.
pub(crate) fn jit_fn_name(name: &str) -> String {
    let suffix = jet_foundation::Syntax::generated_suffix(name);
    jet_foundation::Names::mangle(&format!("jit_fn_{}", suffix.replace("::", "__")))
}

pub(crate) fn mir_fn_name(id: MirFunctionId) -> String {
    format!("__jet_mir_fn_{}", id.0)
}

/// Convert one canonical MIR layout to its Cranelift carrier.
///
/// MIR deliberately keeps arbitrary-precision integers and dynamic/nominal
/// values as runtime handles.  Those carriers are represented by i64 here;
/// the host registry performs the checked marshal at each boundary.
pub(crate) fn clif_ty_from_mir(ty: &MirType) -> Option<types::Type> {
    match ty.layout.abi {
        MirAbi::Scalar(kind) => match kind {
            jet_foundation::MIR::MirScalarKind::Int
            | jet_foundation::MIR::MirScalarKind::Pointer => Some(types::I64),
            jet_foundation::MIR::MirScalarKind::Float => Some(types::F64),
            jet_foundation::MIR::MirScalarKind::Float32 => Some(types::F32),
            jet_foundation::MIR::MirScalarKind::Bool => Some(types::I8),
            jet_foundation::MIR::MirScalarKind::Char => Some(types::I32),
        },
        MirAbi::Aggregate
        | MirAbi::Sequence
        | MirAbi::Function
        | MirAbi::Nominal
        | MirAbi::Dynamic => Some(types::I64),
        MirAbi::Never => None,
    }
}


pub(crate) fn struct_field_redacted(_type_name: &str, _index: usize) -> Option<bool> {
    None
}

/// MIR-backed type metadata used by Cell schema conversion and Cranelift
/// lowering.  Maps own their copies so returned slices remain valid for the
/// lifetime of this view and cannot accidentally observe a later pass.
pub(crate) struct JitMeta<'a> {
    pub(crate) program: &'a MirProgram,
    struct_fields: HashMap<String, (Vec<String>, Vec<MirType>)>,
    struct_type_params: HashMap<String, Vec<String>>,
    distinct_bases: HashMap<String, MirType>,
    enum_variants: HashMap<String, Vec<String>>,
}

impl<'a> JitMeta<'a> {
    pub(crate) fn from_program(program: &'a MirProgram) -> Self {
        let mut struct_fields = HashMap::new();
        let mut struct_type_params = HashMap::new();
        let mut distinct_bases = HashMap::new();
        let mut enum_variants = HashMap::new();
        for def in &program.types {
            match &def.kind {
                MirTypeDefKind::Struct { fields, .. } => {
                    struct_fields.insert(
                        def.name.clone(),
                        (
                            fields.iter().map(|field| field.name.clone()).collect(),
                            fields.iter().map(|field| field.ty.clone()).collect(),
                        ),
                    );
                    struct_type_params.entry(def.name.clone()).or_default();
                }
                MirTypeDefKind::Enum { variants, .. } => {
                    enum_variants.insert(
                        def.name.clone(),
                        variants.iter().map(|variant| variant.name.clone()).collect(),
                    );
                }
                MirTypeDefKind::Distinct { base, .. } => {
                    distinct_bases.insert(def.name.clone(), base.clone());
                }
                MirTypeDefKind::Alias { .. } | MirTypeDefKind::UnitFamily { .. } => {}
            }
        }
        Self {
            program,
            struct_fields,
            struct_type_params,
            distinct_bases,
            enum_variants,
        }
    }

    pub(crate) fn clif_ty(&self, ty: &MirType) -> Option<types::Type> {
        if let Some(name) = ty.nominal_name() {
            if let Some(base) = self.distinct_bases.get(core_prelude_key(name)) {
                return self.clif_ty(base);
            }
        }
        clif_ty_from_mir(ty)
    }

    pub(crate) fn distinct_base(&self, name: &str) -> Option<&MirType> {
        self.distinct_bases.get(core_prelude_key(name))
    }

    pub(crate) fn user_record_or_enum(&self, name: &str) -> bool {
        let key = core_prelude_key(name);
        self.struct_fields.contains_key(key) || self.enum_variants.contains_key(key)
    }

    pub(crate) fn struct_layout(&self, type_name: &str) -> Option<(&[String], &[MirType])> {
        let key = core_prelude_key(type_name);
        self.struct_fields
            .get(key)
            .map(|(names, types)| (names.as_slice(), types.as_slice()))
    }

    pub(crate) fn struct_type_params(&self, type_name: &str) -> Option<&[String]> {
        self.struct_type_params
            .get(core_prelude_key(type_name))
            .map(Vec::as_slice)
    }

    pub(crate) fn enum_names(&self) -> impl Iterator<Item = &String> {
        self.enum_variants.keys()
    }

    pub(crate) fn enum_variants(&self, type_name: &str) -> Option<&[String]> {
        self.enum_variants
            .get(core_prelude_key(type_name))
            .map(Vec::as_slice)
    }

    pub(crate) fn function(&self, id: MirFunctionId) -> Option<&MirFunction> {
        self.program.functions.iter().find(|function| function.id == id)
    }
}

pub(crate) fn func_signature<M: Module>(
    module: &M,
    function: &MirFunction,
) -> Result<Signature, String> {
    let mut signature = Signature::new(module.target_config().default_call_conv);
    for parameter in &function.params {
        let ty = clif_ty_from_mir(&parameter.ty)
            .ok_or_else(|| format!("MIR function `{}` has a non-value parameter", function.key))?;
        signature.params.push(AbiParam::new(ty));
    }
    if let Some(ty) = clif_ty_from_mir(&function.return_type) {
        signature.returns.push(AbiParam::new(ty));
    }
    Ok(signature)
}

pub(crate) fn fn_value_signature<M: Module>(
    module: &M,
    ty: &MirType,
    _meta: &JitMeta<'_>,
) -> Result<Signature, String> {
    let (params, ret) = ty
        .function_signature()
        .map(|facts| (facts.params.as_slice(), facts.ret.as_deref()))
        .or_else(|| ty.send_fn_signature())
        .ok_or_else(|| {
            format!("MIR function value has non-function type `{}`", ty.display_name())
        })?;
    let mut signature = Signature::new(module.target_config().default_call_conv);
    for parameter in params {
        signature.params.push(AbiParam::new(
            clif_ty_from_mir(parameter)
                .ok_or_else(|| format!("function parameter has no ABI: {}", parameter.display_name()))?,
        ));
    }
    if let Some(ret) = ret {
        if let Some(ty) = clif_ty_from_mir(ret) {
            signature.returns.push(AbiParam::new(ty));
        }
    }
    Ok(signature)
}

pub(crate) fn interrupt_callback_signature<M: Module>(module: &M) -> Signature {
    Signature::new(module.target_config().default_call_conv)
}

pub(crate) fn core_alias_leaf(type_name: &str) -> Option<&str> {
    let leaf = type_name.rsplit('.').next().unwrap_or(type_name);
    PRELUDE_ENUM_VARIANTS
        .contains_key(core_prelude_key(leaf))
        .then_some(core_prelude_key(leaf))
}

pub(crate) fn core_struct_field_type(_type_name: &str, _field: &str) -> Option<MirType> {
    None
}

pub(crate) fn core_struct_field_uses_result_option_abi(
    _type_name: &str,
    _field: &str,
) -> bool {
    false
}

pub(crate) fn core_call_uses_result_option_abi(_module: &str, _member: &str) -> bool {
    false
}

pub(crate) fn install_struct_redact(_program: &MirProgram) {}
