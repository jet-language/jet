use super::*;
use crate::AST::{Pattern, Type, VariantPayload};
use crate::Syntax;

pub(crate) fn emit_match_pattern(cx: &Cx, pattern: &Pattern, enum_type: Option<&str>) -> String {
    use crate::AST::PatSlot;
    // Resolve the enum type prefix from the pattern if not provided.
    let resolved_type = enum_type.map(|t| t.to_string()).or_else(|| {
        if let Pattern::Variant { variant, .. } = pattern {
            cx.variant_owner.get(variant).cloned()
        } else if let Pattern::Or(alts, _) = pattern {
            alts.iter().find_map(|a| {
                if let Pattern::Variant { variant, .. } = a { cx.variant_owner.get(variant).cloned() } else { None }
            })
        } else {
            None
        }
    });
    let etype = resolved_type.as_deref().or(enum_type);
    let is_json = etype.map(is_json_type_name).unwrap_or(false);
    let prefix = etype
        .map(|t| {
            if is_json_type_name(t) {
                format!("{}jet_std::Json", cx.root_prefix)
            } else if let Some(rust_mod) = cx.foreign_types.get(t) {
                format!("{}{}::user_{}", cx.root_prefix, rust_mod, t)
            } else {
                format!("user_{}", t)
            }
        })
        .unwrap_or_else(|| "user_TYPE".to_string());
    // Variant names are mangled for user enums, but JSON variants keep their
    // original Rust name (they are defined as plain Rust identifiers in std.rs).
    let vname = |v: &str| -> String {
        if is_json { v.to_string() } else { mangle(v) }
    };
    match pattern {
        Pattern::Variant {
            variant, bindings, ..
        } => {
            // Check if any slot is a Range (needs a guard — handled in emit_pattern_match_switch).
            if bindings.is_empty() {
                format!("{}::{}", prefix, vname(variant))
            } else {
                // Build per-slot Rust binding: Bind(n) → mangle(n), Wildcard → `_`, Range → temp name.
                let slot_pats: Vec<String> = bindings.iter().enumerate().map(|(i, s)| match s {
                    PatSlot::Bind(n) => mangle(n),
                    PatSlot::Wildcard => "_".to_string(),
                    PatSlot::Range { .. } => format!("__jet_range_{}", i),
                }).collect();
                if slot_pats.len() == 1 {
                    format!("{}::{}({})", prefix, vname(variant), slot_pats[0])
                } else {
                    // Named-field struct variant: use positional f0, f1 ... names.
                    let fields: Vec<String> = slot_pats.iter().enumerate()
                        .map(|(i, p)| format!("f{i}: {p}"))
                        .collect();
                    format!("{}::{} {{ {} }}", prefix, vname(variant), fields.join(", "))
                }
            }
        }
        Pattern::Present { binding, .. } => format!("Some({})", mangle(binding)),
        Pattern::Absent(_) => "None".to_string(),
        Pattern::Ok { binding, .. } => format!("Ok({})", mangle(binding)),
        Pattern::Err { binding, .. } => format!("Err({})", mangle(binding)),
        // D-PATR arm-head: range patterns go through mixed-switch; shouldn't appear here.
        Pattern::Range { lo, hi, .. } => format!("_ if __jet_subject >= {} && __jet_subject <= {}", lo, hi),
        // D-PATO: or-pattern in exhaustive match → `A(x) | B(x)`.
        Pattern::Or(alts, _) => {
            let pats: Vec<String> = alts.iter()
                .map(|a| emit_match_pattern(cx, a, etype))
                .collect();
            pats.join(" | ")
        }
    }
}

/// The payload types a variant binds, so destructured names carry their type
/// into the body (needed so e.g. `.get` on a `Map` bound from `Object(root)`
/// lowers to a map lookup, not list indexing — B3). Mirrors sema's
/// `core_json_pattern_types` for the core `JSON` enum and reads `cx` for user
/// enums. Returns `None` when the types aren't known (binding stays untyped).
pub(crate) fn variant_binding_types(cx: &Cx, variant: &str) -> Option<Vec<Type>> {
    if is_json_variant(variant) {
        let json = Type::Named(Syntax::TYPE_JSON.to_string());
        return match variant {
            "Null" => Some(Vec::new()),
            "Boolean" => Some(vec![Type::Bool]),
            "Number" => Some(vec![Type::Float]),
            "Text" => Some(vec![Type::String]),
            "Array" => Some(vec![Type::List(Box::new(json))]),
            "Object" => Some(vec![Type::Map {
                key: Box::new(Type::String),
                value: Box::new(json),
            }]),
            _ => None,
        };
    }
    let owner = cx.variant_owner.get(variant)?;
    let variants = cx.enum_variants.get(owner)?;
    let (_, payload) = variants.iter().find(|(n, _)| n == variant)?;
    match payload {
        VariantPayload::Unit => Some(Vec::new()),
        VariantPayload::Single(t, _) => Some(vec![t.clone()]),
        VariantPayload::Named(fields) => {
            Some(fields.iter().map(|f| f.ty.clone()).collect())
        }
    }
}

pub(crate) fn emit_if_let_pattern(cx: &Cx, pattern: &Pattern) -> String {
    use crate::AST::PatSlot;
    match pattern {
        Pattern::Variant {
            variant, bindings, ..
        } => {
            let prefix = enum_type_prefix(cx, variant);
            let rv = variant_rust_name(variant);
            if bindings.is_empty() {
                format!("{}::{}", prefix, rv)
            } else {
                let slot_pats: Vec<String> = bindings.iter().enumerate().map(|(i, s)| match s {
                    PatSlot::Bind(n) => mangle(n),
                    PatSlot::Wildcard => "_".to_string(),
                    PatSlot::Range { .. } => format!("__jet_range_{}", i),
                }).collect();
                if slot_pats.len() == 1 {
                    format!("{}::{}({})", prefix, rv, slot_pats[0])
                } else {
                    let fields: Vec<String> = slot_pats.iter().enumerate()
                        .map(|(i, p)| format!("f{i}: {p}"))
                        .collect();
                    format!("{}::{} {{ {} }}", prefix, rv, fields.join(", "))
                }
            }
        }
        Pattern::Present { binding, .. } => format!("Some({})", mangle(binding)),
        Pattern::Absent(_) => "None".to_string(),
        Pattern::Ok { binding, .. } => format!("Ok({})", mangle(binding)),
        Pattern::Err { binding, .. } => format!("Err({})", mangle(binding)),
        // Or/Range in if-let position: fall back to a safe default.
        Pattern::Or(alts, _) => alts.first().map(|a| emit_if_let_pattern(cx, a)).unwrap_or_default(),
        Pattern::Range { .. } => "_".to_string(),
    }
}

pub(crate) fn emit_named_fn_value(cx: &Cx, name: &str, ft: &Type) -> String {
    let rust_name = mangle(name);
    let Type::Fn { params, ret } = ft else {
        return rust_name;
    };
    let arg_decls: Vec<String> = params
        .iter()
        .enumerate()
        .map(|(i, p)| format!("__jet_a{}: {}", i, cx.rust_type(p)))
        .collect();
    // Non-scalar Named types are passed by reference in Jet Rust functions.
    let arg_calls: Vec<String> = params.iter().enumerate().map(|(i, p)| {
        if matches!(p, Type::Named(_) | Type::String | Type::List(_) | Type::Map { .. }) {
            format!("&__jet_a{i}")
        } else {
            format!("__jet_a{i}")
        }
    }).collect();
    let _ = ret;
    format!(
        "Box::new(move |{}| {}({})) as {}",
        arg_decls.join(", "),
        rust_name,
        arg_calls.join(", "),
        cx.rust_type(ft)
    )
}

