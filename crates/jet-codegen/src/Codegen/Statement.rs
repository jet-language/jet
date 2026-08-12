use super::*;
use crate::Syntax;
use crate::AST::{AccessConvention, Pattern, Type, VariantPayload};

pub(crate) fn emit_match_pattern(cx: &Cx, pattern: &Pattern, enum_type: Option<&str>) -> String {
    use crate::AST::PatSlot;
    // Resolve the enum type prefix from the pattern if not provided.
    let resolved_type = enum_type.map(|t| t.to_string()).or_else(|| {
        if let Pattern::Variant { variant, .. } = pattern {
            cx.variant_owner.get(variant).cloned()
        } else if let Pattern::Or(alts, _) = pattern {
            alts.iter().find_map(|a| {
                if let Pattern::Variant { variant, .. } = a {
                    cx.variant_owner.get(variant).cloned()
                } else {
                    None
                }
            })
        } else {
            None
        }
    });
    let etype = resolved_type.as_deref().or(enum_type);
    let is_json = etype.map(is_json_type_name).unwrap_or(false);
    let is_io = etype.map(|t| matches!(t, "IOError" | "IOOperation")).unwrap_or(false);
    let is_http = etype
        .map(|t| matches!(t, "HTTPError" | "HTTPOperation"))
        .unwrap_or(false);
    let is_email = etype.is_some_and(|t| matches!(t, "SMTPSecurity" | "RecipientPolicy" | "EmailError"));
    let is_auth = etype == Some("AuthError");
    let is_service_receipt = etype == Some("ServiceReceipt");
    let is_service_error = etype == Some("ServiceError");
    let is_hook_outcome = etype == Some("HookOutcome");
    let is_data_event = etype == Some("DataEvent");
    // D-UNIONTYPE1=A: anonymous unions lower to `__JetUnion_*` with bare tags.
    let is_anon_union = etype.is_some_and(|t| t.starts_with("__JetUnion_"));
    // D-TERM1: detect `Key` from the variant name when the type isn't resolved in etype.
    let is_key = {
        let from_etype = etype.map(|t| t == crate::Syntax::TYPE_KEY).unwrap_or(false);
        let from_variant = if etype.is_none() {
            if let Pattern::Variant { variant, .. } = pattern {
                is_key_variant(variant)
            } else {
                false
            }
        } else {
            false
        };
        from_etype || from_variant
    };
    let prefix = etype
        .map(|t| {
            if is_json_type_name(t) {
                format!("{}jet_std::DataTree", cx.root_prefix)
            } else if t.starts_with("__JetUnion_") {
                mangle_path(t)
            } else if t == crate::Syntax::TYPE_KEY {
                format!("{}JetKey", cx.root_prefix)
            } else if t == "DataEvent" {
                format!("{}jet_std::DataEvent", cx.root_prefix)
            } else if t == crate::Syntax::TYPE_IO_ERROR {
                format!("{}jet_std::IOError", cx.root_prefix)
            } else if t == crate::Syntax::TYPE_IO_OPERATION {
                format!("{}jet_std::IOOperation", cx.root_prefix)
            } else if t == "HTTPError" {
                format!("{}JetHTTPError", cx.root_prefix)
            } else if t == "HTTPOperation" {
                format!("{}JetHTTPOperation", cx.root_prefix)
            } else if matches!(t, "SMTPSecurity" | "RecipientPolicy" | "EmailError") {
                let rust = if t == "EmailError" { "Error" } else { t };
                format!("{}jet_email::{rust}", cx.root_prefix)
            } else if t == "AuthError" {
                format!("{}JetAuthError", cx.root_prefix)
            } else if t == "ServiceReceipt" {
                format!("{}JetServiceReceipt", cx.root_prefix)
            } else if t == "ServiceError" {
                format!("{}JetServiceError", cx.root_prefix)
            } else if t == "HookOutcome" {
                format!("{}jet_std::JetHookOutcome", cx.root_prefix)
            } else if let Some(rust_mod) = cx.foreign_types.get(t) {
                format!("{}{}::{}", cx.root_prefix, rust_mod, mangle_path(t))
            } else {
                mangle_path(t)
            }
        })
        .unwrap_or_else(|| {
            // No etype: infer from the variant name.
            if let Pattern::Variant { variant, .. } = pattern {
                if is_key_variant(variant) {
                    return format!("{}JetKey", cx.root_prefix);
                }
            }
            mangle("TYPE")
        });
    // Variant names are mangled for user enums, but JSON/Key/union tags keep
    // their original Rust name (defined as plain Rust identifiers in the prelude).
    let vname = |v: &str| -> String {
        if is_json || is_key || is_data_event || is_io || is_http || is_email || is_auth || is_service_receipt || is_service_error || is_hook_outcome || is_anon_union {
            v.to_string()
        } else {
            mangle_path(v)
        }
    };
    match pattern {
        Pattern::Variant {
            variant, bindings, ..
        } => {
            // Check if any slot is a Range (needs a guard — handled in emit_pattern_match_switch).
            if bindings.is_empty() {
                // D-TAG1: a group name matches its whole subtree — expand to an
                // or-pattern over its leaves, payloads wildcarded.
                if !is_json && !is_key {
                    let leaves = group_leaves(cx, etype, variant);
                    if !leaves.is_empty() {
                        return leaves
                            .iter()
                            .map(|(n, p)| {
                                let head = format!("{}::{}", prefix, vname(n));
                                match p {
                                    VariantPayload::Unit => head,
                                    VariantPayload::Single(..) => format!("{head}(_)"),
                                    VariantPayload::Named(_) => format!("{head} {{ .. }}"),
                                }
                            })
                            .collect::<Vec<_>>()
                            .join(" | ");
                    }
                }
                format!("{}::{}", prefix, vname(variant))
            } else {
                // Build per-slot Rust binding: Bind(n) → mangle(n), Wildcard → `_`, Range → temp name.
                let slot_pats: Vec<String> = bindings
                    .iter()
                    .enumerate()
                    .map(|(i, s)| match s {
                        PatSlot::Bind { name, .. } => mangle(name),
                        PatSlot::Wildcard => "_".to_string(),
                        PatSlot::Range { .. } => format!("__jet_range_{}", i),
                    })
                    .collect();
                // Named-field struct variant (S30, incl. a single named field):
                // positional pattern slots bind by declaration order, so map slot
                // index -> the real Rust field name (mangle(&f.name), matching enum
                // definition codegen in Items.rs) rather than always assuming a
                // tuple variant. `VariantPayload::Single` is the only real tuple case.
                let real_names = variant_field_names(cx, variant).map(|names| {
                    if is_email || is_auth || is_service_receipt || is_service_error {
                        names
                            .into_iter()
                            .map(|name| {
                                name.strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX)
                                    .unwrap_or(&name)
                                    .to_string()
                            })
                            .collect()
                    } else {
                        names
                    }
                });
                if let Some(real_names) = real_names {
                    let fields: Vec<String> = slot_pats
                        .iter()
                        .enumerate()
                        .map(|(i, p)| {
                            let name = real_names
                                .get(i)
                                .cloned()
                                .unwrap_or_else(|| format!("f{i}"));
                            format!("{name}: {p}")
                        })
                        .collect();
                    format!("{}::{} {{ {} }}", prefix, vname(variant), fields.join(", "))
                } else if slot_pats.len() == 1 {
                    format!("{}::{}({})", prefix, vname(variant), slot_pats[0])
                } else {
                    let fields: Vec<String> = slot_pats
                        .iter()
                        .enumerate()
                        .map(|(i, p)| format!("f{i}: {p}"))
                        .collect();
                    format!("{}::{} {{ {} }}", prefix, vname(variant), fields.join(", "))
                }
            }
        }
        Pattern::Present { binding, .. } => format!("Ok({})", mangle(binding)),
        Pattern::Absent(_) => "Err(_)".to_string(),
        Pattern::Ok { binding, .. } => format!("Ok({})", mangle(binding)),
        Pattern::Err { binding, .. } => format!("Err({})", mangle(binding)),
        // D-PATR arm-head: range patterns go through mixed-switch; shouldn't appear here.
        Pattern::Range { lo, hi, .. } => {
            format!("_ if __jet_subject >= {} && __jet_subject <= {}", lo, hi)
        }
        // D-PATO: or-pattern in exhaustive match → `A(x) | B(x)`.
        Pattern::Or(alts, _) => {
            let pats: Vec<String> = alts
                .iter()
                .map(|a| emit_match_pattern(cx, a, etype))
                .collect();
            pats.join(" | ")
        }
        // D-PARSESTR1: str-match patterns are lowered as their own scan/split/parse
        // control flow (TIR/lower.rs), never as a single Rust match pattern.
        Pattern::Struct { .. } | Pattern::StrMatch { .. } | Pattern::BinMatch { .. } => {
            "_".to_string()
        }
    }
}

/// The real Rust field names (declaration order, mangled) for a named-payload
/// variant, used to map positional pattern slots onto the struct-variant shape
/// that Items.rs emits for `VariantPayload::Named`. `None` when `variant` isn't
/// a known user enum with a named payload (JSON/Key variants, single/unit).
fn variant_field_names(cx: &Cx, variant: &str) -> Option<Vec<String>> {
    let owner = cx.variant_owner.get(variant)?;
    let variants = cx.enum_variants.get(owner)?;
    let (_, payload) = variants.iter().find(|(n, _)| n == variant)?;
    match payload {
        VariantPayload::Named(fs) => Some(fs.iter().map(|f| mangle(&f.name)).collect()),
        _ => None,
    }
}

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
            "Array" => Some(vec![Type::List(Box::new(data))]),
            "Object" => Some(vec![Type::Map {
                key: Box::new(Type::String),
                key_span: None,
                value: Box::new(data),
            }]),
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
            "Null" | "ArrayStart" | "ArrayEnd" | "ObjectStart" | "ObjectEnd" => {
                Some(Vec::new())
            }
            _ => None,
        };
    }
    let resolved = cx
        .core_qualified_rust_type_name(enum_name)
        .unwrap_or(enum_name);
    let variants = cx.enum_variants.get(resolved)?;
    let (_, payload) = variants.iter().find(|(n, _)| n == variant)?;
    match payload {
        VariantPayload::Unit => Some(Vec::new()),
        VariantPayload::Single(t, _) => Some(vec![t.clone()]),
        VariantPayload::Named(fields) => Some(fields.iter().map(|f| f.ty.clone()).collect()),
    }
}

pub(crate) fn emit_if_let_pattern(cx: &Cx, pattern: &Pattern) -> String {
    use crate::AST::PatSlot;
    match pattern {
        Pattern::Variant {
            variant, bindings, ..
        } => {
            let prefix = enum_type_prefix(cx, variant);
            let rv = variant_rust_name(cx, variant);
            if bindings.is_empty() {
                // D-TAG1: a group name matches its whole subtree.
                let owner = cx.variant_owner.get(variant).map(String::as_str);
                let leaves = group_leaves(cx, owner, variant);
                if !leaves.is_empty() {
                    return leaves
                        .iter()
                        .map(|(n, p)| {
                            let head = format!("{}::{}", prefix, variant_rust_name(cx, n));
                            match p {
                                VariantPayload::Unit => head,
                                VariantPayload::Single(..) => format!("{head}(_)"),
                                VariantPayload::Named(_) => format!("{head} {{ .. }}"),
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" | ");
                }
                format!("{}::{}", prefix, rv)
            } else {
                let slot_pats: Vec<String> = bindings
                    .iter()
                    .enumerate()
                    .map(|(i, s)| match s {
                        PatSlot::Bind { name, .. } => mangle(name),
                        PatSlot::Wildcard => "_".to_string(),
                        PatSlot::Range { .. } => format!("__jet_range_{}", i),
                    })
                    .collect();
                if let Some(names) = variant_field_names(cx, variant) {
                    let plain = cx.variant_owner.get(variant).is_some_and(|owner| {
                        matches!(owner.as_str(), "EmailError" | "SMTPAuth" | "TLSTrust" | "AuthError" | "ServiceReceipt" | "ServiceError")
                    });
                    let fields = names
                        .iter()
                        .zip(&slot_pats)
                        .map(|(name, pattern)| {
                            let name = if plain {
                                name.strip_prefix(crate::Syntax::GENERATED_NAME_PREFIX)
                                    .unwrap_or(name)
                            } else {
                                name
                            };
                            format!("{name}: {pattern}")
                        })
                        .collect::<Vec<_>>();
                    format!("{}::{} {{ {} }}", prefix, rv, fields.join(", "))
                } else if slot_pats.len() == 1 {
                    format!("{}::{}({})", prefix, rv, slot_pats[0])
                } else {
                    let fields: Vec<String> = slot_pats
                        .iter()
                        .enumerate()
                        .map(|(i, p)| format!("f{i}: {p}"))
                        .collect();
                    format!("{}::{} {{ {} }}", prefix, rv, fields.join(", "))
                }
            }
        }
        Pattern::Present { binding, .. } => format!("Ok({})", mangle(binding)),
        Pattern::Absent(_) => "Err(_)".to_string(),
        Pattern::Ok { binding, .. } => format!("Ok({})", mangle(binding)),
        Pattern::Err { binding, .. } => format!("Err({})", mangle(binding)),
        // Or/Range in if-let position: fall back to a safe default.
        Pattern::Or(alts, _) => alts
            .first()
            .map(|a| emit_if_let_pattern(cx, a))
            .unwrap_or_default(),
        // D-PARSESTR1: str-match, like struct patterns, isn't reachable here —
        // it has its own dedicated lowering (TIR/lower.rs).
        Pattern::Range { .. }
        | Pattern::Struct { .. }
        | Pattern::StrMatch { .. }
        | Pattern::BinMatch { .. } => "_".to_string(),
    }
}

pub(crate) fn emit_named_fn_value(cx: &Cx, name: &str, ft: &Type) -> String {
    emit_named_fn_value_with_storage(cx, name, ft, false)
}

/// Emit a named function value for a host that stores callbacks behind a
/// `Send + Sync` boundary. Ordinary Jet function values stay `Rc`; only the
/// host boundary chooses `Arc`.
pub(crate) fn emit_named_fn_value_sync(cx: &Cx, name: &str, ft: &Type) -> String {
    emit_named_fn_value_with_storage(cx, name, ft, true)
}

fn emit_named_fn_value_with_storage(
    cx: &Cx,
    name: &str,
    ft: &Type,
    send_sync: bool,
) -> String {
    let rust_name = mangle(name);
    let Type::Fn { params, ret, .. } = ft else {
        return rust_name;
    };
    let middleware = params.len() == 1
        && matches!(&params[0], Type::Named(name) if name == "HTTPHandler")
        && matches!(ret.as_deref(), Some(Type::Named(name)) if name == "HTTPHandler");
    let wrap = if middleware || send_sync {
        "std::sync::Arc::new"
    } else {
        "std::rc::Rc::new"
    };
    let rust_type = if send_sync && !middleware {
        let ordinary = cx.rust_type(ft);
        ordinary
            .strip_prefix("std::rc::Rc<")
            .and_then(|inner| inner.strip_suffix('>'))
            .map(|inner| format!("std::sync::Arc<{inner} + Send + Sync>"))
            .unwrap_or(ordinary)
    } else {
        cx.rust_type(ft)
    };
    if !middleware
        && ret
            .as_deref()
            .is_some_and(|ret| cx.type_contains_view(ret))
    {
        return format!("{wrap}({rust_name}) as {rust_type}");
    }
    let arg_decls: Vec<String> = params
        .iter()
        .enumerate()
        .map(|(i, p)| {
            format!(
                "__jet_a{}: {}",
                i,
                if middleware {
                    cx.rust_type(p)
                } else {
                    rust_param_type(cx, AccessConvention::Read, p)
                }
            )
        })
        .collect();
    let arg_calls: Vec<String> = params
        .iter()
        .enumerate()
        .map(|(i, _)| {
            if middleware {
                format!("&__jet_a{i}")
            } else {
                format!("__jet_a{i}")
            }
        })
        .collect();
    let _ = ret;
    format!(
        "{wrap}(move |{}| {}({})) as {rust_type}",
        arg_decls.join(", "),
        rust_name,
        arg_calls.join(", "),
    )
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
