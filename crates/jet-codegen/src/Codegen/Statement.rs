use super::*;
use crate::Syntax;
use crate::AST::{Type, VariantPayload};


/// The payload types a variant binds, so destructured names carry their type
/// into the body (needed so e.g. `.get` on a `Map` bound from `Object(root)`
/// lowers to a map lookup, not list indexing — B3). Mirrors sema's
/// `core_json_pattern_types` for the core `JSON` enum and reads `cx` for user
/// enums. Returns `None` when the types aren't known (binding stays untyped).
pub(crate) fn variant_binding_types(cx: &Cx, variant: &str) -> Option<Vec<Type>> {
    if is_json_variant(variant) {
        let data = Type::Named(Syntax::TYPE_DATA.to_string());
        return match variant {
            "Null" => Some(Vec::new()),
            "Bool" => Some(vec![Type::Bool]),
            "Int" => Some(vec![Type::Int]),
            "Float" => Some(vec![Type::Float]),
            "Text" => Some(vec![Type::String]),
            "Array" => Some(vec![Type::List(Box::new(data.clone()))]),
            "Object" => Some(vec![Type::Map {
                key: Box::new(Type::String),
                key_span: None,
                value: Box::new(data),
            }]),
            "Number" => Some(vec![Type::String]),
            _ => None,
        };
    }
    if let Some(owner) = cx.variant_owner.get(variant) {
        if let Some(types) = variant_binding_types_for_enum(cx, owner, variant) {
            return Some(types);
        }
    }
    // D-TERM1 (ratified 2026-06-22): `Key` variant payload types for codegen.
    if is_key_variant(variant) {
        return match variant {
            "Char" | "Ctrl" => Some(vec![Type::Char]),
            "F" => Some(vec![Type::Int]),
            _ => Some(Vec::new()), // unit variants
        };
    }
    None
}

pub(crate) fn variant_binding_types_for_enum(
    cx: &Cx,
    enum_name: &str,
    variant: &str,
) -> Option<Vec<Type>> {
    if enum_name == Syntax::TYPE_KEY {
        return match variant {
            "Char" | "Ctrl" => Some(vec![Type::Char]),
            "F" => Some(vec![Type::Int]),
            _ => Some(Vec::new()),
        };
    }
    if enum_name == "DataEvent" {
        return match variant {
            "Bool" => Some(vec![Type::Bool]),
            "Int" => Some(vec![Type::Int]),
            "Float" => Some(vec![Type::Float]),
            "Text" | "Key" => Some(vec![Type::String]),
            "Bytes" => Some(vec![Type::List(Box::new(Type::Int))]),
            "Null" | "ArrayStart" | "ArrayEnd" | "ObjectStart" | "ObjectEnd" => Some(Vec::new()),
            _ => None,
        };
    }
    let resolved = crate::Codegen::TIR::canonical_enum_owner(cx, enum_name);
    let variants = cx.enum_variants.get(&resolved)?;
    let (_, payload) = variants.iter().find(|(n, _)| n == variant)?;
    match payload {
        VariantPayload::Unit => Some(Vec::new()),
        VariantPayload::Single(t, _) => Some(vec![t.clone()]),
        VariantPayload::Named(fields) => Some(fields.iter().map(|f| f.ty.clone()).collect()),
    }
}


/// D-TAG1: the ordered leaves under a group path of `enum_type`, with payloads.
/// Empty when `variant` isn't a group (or the enum is unknown) — callers fall
/// back to the plain single-variant pattern.
pub(crate) fn group_leaves<'c>(
    cx: &'c Cx,
    enum_type: Option<&str>,
    variant: &str,
) -> Vec<(&'c String, &'c VariantPayload)> {
    let Some(et) = enum_type else {
        return Vec::new();
    };
    let Some(vars) = cx.enum_variants.get(et) else {
        return Vec::new();
    };
    let prefix = format!("{variant}.");
    vars.iter()
        .filter(|(n, _)| n.starts_with(&prefix))
        .map(|(n, p)| (n, p))
        .collect()
}
