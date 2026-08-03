use super::*;

pub(crate) fn enum_type_prefix(cx: &Cx, variant: &str) -> String {
    cx.variant_owner
        .get(variant)
        .map(|t| {
            if t == crate::Syntax::TYPE_IO_ERROR {
                format!("{}jet_std::IOError", cx.root_prefix)
            } else if t == crate::Syntax::TYPE_IO_OPERATION {
                format!("{}jet_std::IOOperation", cx.root_prefix)
            } else if t == "HTTPError" {
                format!("{}JetHTTPError", cx.root_prefix)
            } else if t == "HTTPOperation" {
                format!("{}JetHTTPOperation", cx.root_prefix)
            } else if t == "AuthError" {
                format!("{}JetAuthError", cx.root_prefix)
            } else if t == "ServiceReceipt" {
                format!("{}JetServiceReceipt", cx.root_prefix)
            } else if t == "ServiceError" {
                format!("{}JetServiceError", cx.root_prefix)
            } else if t == "HookOutcome" {
                format!("{}jet_std::JetHookOutcome", cx.root_prefix)
            } else if let Some(rust_mod) = cx.foreign_types.get(t.as_str()) {
                format!("{}{}::user_{}", cx.root_prefix, rust_mod, t)
            } else {
                format!("user_{}", t)
            }
        })
        .unwrap_or_else(|| {
            if is_json_variant(variant) {
                format!("{}jet_std::DataTree", cx.root_prefix)
            } else if is_key_variant(variant) {
                // D-TERM1: `Key` variants are in the top-level prelude as `JetKey`.
                format!("{}JetKey", cx.root_prefix)
            } else {
                "user_TYPE".to_string()
            }
        })
}

// D-ENC-DYN1=A+: the dynamic `Data` value's variants (face of `jet_std::DataTree`).
pub(crate) fn is_json_variant(variant: &str) -> bool {
    crate::Syntax::is_data_variant(variant)
}

// D-DBDRIVER1: the `DBValue` dynamic tagged SQL value's variants.
pub(crate) fn is_db_value_variant(variant: &str) -> bool {
    crate::Syntax::is_db_value_variant(variant)
}

/// D-TERM1 (ratified 2026-06-22): is this variant name a `Key` enum variant?
pub(crate) fn is_key_variant(variant: &str) -> bool {
    matches!(
        variant,
        "Char"
            | "Enter"
            | "Escape"
            | "Backspace"
            | "Tab"
            | "Delete"
            | "Up"
            | "Down"
            | "Left"
            | "Right"
            | "F"
            | "Ctrl"
            | "Unknown"
    )
}

pub(crate) fn variant_rust_name(cx: &Cx, variant: &str) -> String {
    if is_json_variant(variant)
        || (is_key_variant(variant)
            && cx
                .variant_owner
                .get(variant)
                .is_none_or(|owner| owner == crate::Syntax::TYPE_KEY))
        || cx
            .variant_owner
            .get(variant)
            .is_some_and(|owner| matches!(owner.as_str(), "HTTPError" | "HTTPOperation"))
        || cx.variant_owner.get(variant).is_some_and(|owner| owner == "HookOutcome")
        || cx.variant_owner.get(variant).is_some_and(|owner| owner == "AuthError")
        || cx
            .variant_owner
            .get(variant)
            .is_some_and(|owner| matches!(owner.as_str(), "ServiceReceipt" | "ServiceError"))
    {
        variant.to_string()
    } else {
        mangle_variant(variant)
    }
}

pub(crate) fn escape_rust_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
