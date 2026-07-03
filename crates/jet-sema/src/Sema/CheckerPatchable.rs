//! D-PATCH1 (card #181): `@[Patchable]` derive — synthetic `T.Patch` type + apply/diff/merge.

use crate::AST::{
    AccessConvention, Field, Item, StructDef, Type,
};
use crate::Diagnostics::Diagnostic;
use crate::Syntax;

use super::{MethodSig, TypeDef, TypeRegistry};

pub(crate) fn patch_type_name(base: &str) -> String {
    format!("{base}.Patch")
}

fn has_patchable(s: &StructDef) -> bool {
    s.derives
        .iter()
        .any(|(t, _)| t == Syntax::CONTRACT_PATCHABLE)
}

/// Append synthetic `T.Patch` struct items (Codable via Encode+Decode) before registration.
pub(crate) fn inject_patchable_types(items: &mut Vec<Item>, diags: &mut Vec<Diagnostic>) {
    let mut to_add = Vec::new();
    for item in items.iter() {
        let Item::Struct(s) = item else { continue };
        if !has_patchable(s) {
            continue;
        }
        if let Some(d) = validate_patchable_struct(s) {
            diags.push(d);
            continue;
        }
        let patch_name = patch_type_name(&s.name);
        let fields: Vec<Field> = s
            .fields
            .iter()
            .map(|f| Field {
                name: f.name.clone(),
                name_span: f.name_span,
                ty: Type::Option(Box::new(f.ty.clone())),
                ty_span: f.ty_span,
                is_stored_ref: false,
                stored_ref_label: None,
                is_pub: f.is_pub,
                is_package_pub: f.is_package_pub,
                serde_markers: Vec::new(),
                redact: false,
            })
            .collect();
        to_add.push(Item::Struct(StructDef {
            is_pub: s.is_pub,
            is_package_pub: s.is_package_pub,
            name: patch_name,
            name_span: s.name_span,
            type_params: Vec::new(),
            fields,
            methods: Vec::new(),
            trait_impls: Vec::new(),
            derives: Vec::new(),
            is_published_schema: false,
            published_schema_span: None,
            is_single_use: false,
            single_use_span: None,
            is_must_use: false,
            must_use_span: None,
            layout: None,
            layout_span: None,
            serde_markers: Vec::new(),
            type_markers: Vec::new(),
        }));
    }
    items.extend(to_add);
}

fn validate_patchable_struct(s: &StructDef) -> Option<Diagnostic> {
    if !s.type_params.is_empty() {
        return Some(Diagnostic::error(
            "E0336",
            format!("`@[Patchable]` on generic struct `{}` isn't supported yet", s.name),
            "`T.Patch` codegen needs a concrete field list — generic patches are a follow-on"
                .to_string(),
            "remove the type parameters, or drop `@[Patchable]` for now".to_string(),
            Some(s.name_span),
        ));
    }
    for f in &s.fields {
        if f.is_stored_ref {
            return Some(Diagnostic::error(
                "E0337",
                format!(
                    "`@[Patchable]` struct `{}` field `{}` is a stored reference",
                    s.name, f.name
                ),
                "patches hold optional owned values — a patch can't express a borrowed field"
                    .to_string(),
                "use an owned field type, or drop `@[Patchable]`".to_string(),
                Some(f.name_span),
            ));
        }
        if matches!(f.ty, Type::Fn { .. }) {
            return Some(Diagnostic::error(
                "E0337",
                format!(
                    "`@[Patchable]` struct `{}` field `{}` has function type",
                    s.name, f.name
                ),
                "patches are data values — function-typed fields can't be patched"
                    .to_string(),
                "store a callable handle type instead, or drop `@[Patchable]`".to_string(),
                Some(f.name_span),
            ));
        }
    }
    None
}

/// Register apply / diff / merge after base + Patch types exist in the registry.
pub(crate) fn register_patchable_methods(items: &[Item], registry: &mut TypeRegistry) {
    for item in items {
        let Item::Struct(s) = item else { continue };
        if !has_patchable(s) {
            continue;
        }
        let patch = patch_type_name(&s.name);
        if !registry.contains(&patch) {
            continue;
        }
        let base = s.name.clone();
        let base_ty = Type::Named(base.clone());
        let patch_ty = Type::Named(patch.clone());

        if let Some(TypeDef::Struct { methods, .. }) = registry.types.get_mut(&base) {
            methods.insert(
                "apply".to_string(),
                MethodSig {
                    params: vec![
                        (AccessConvention::Read, base_ty.clone()),
                        (AccessConvention::Move, patch_ty.clone()),
                    ],
                    return_type: Some(base_ty.clone()),
                    is_view_return: false,
                    is_static: false,
                    self_conv: Some(AccessConvention::Read),
                    param_info: vec![("patch".to_string(), false)],
                    defaults: vec![None],
                    must_use: false,
                },
            );
            methods.insert(
                "diff".to_string(),
                MethodSig {
                    params: vec![
                        (AccessConvention::Move, base_ty.clone()),
                        (AccessConvention::Move, base_ty),
                    ],
                    return_type: Some(patch_ty.clone()),
                    is_view_return: false,
                    is_static: true,
                    self_conv: None,
                    param_info: vec![
                        ("new".to_string(), false),
                        ("old".to_string(), false),
                    ],
                    defaults: vec![None, None],
                    must_use: false,
                },
            );
        }
        if let Some(TypeDef::Struct { methods, .. }) = registry.types.get_mut(&patch) {
            methods.insert(
                "merge".to_string(),
                MethodSig {
                    params: vec![
                        (AccessConvention::Read, patch_ty.clone()),
                        (AccessConvention::Move, patch_ty),
                    ],
                    return_type: Some(Type::Named(patch)),
                    is_view_return: false,
                    is_static: false,
                    self_conv: Some(AccessConvention::Read),
                    param_info: vec![("other".to_string(), false)],
                    defaults: vec![None],
                    must_use: false,
                },
            );
        }
    }
}
