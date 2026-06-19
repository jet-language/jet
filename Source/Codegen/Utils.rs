use super::*;
use crate::Diagnostics::Span;

pub(crate) fn enum_type_prefix(cx: &Cx, variant: &str) -> String {
    cx.variant_owner
        .get(variant)
        .map(|t| {
            if let Some(rust_mod) = cx.foreign_types.get(t.as_str()) {
                format!("{}{}::user_{}", cx.root_prefix, rust_mod, t)
            } else {
                format!("user_{}", t)
            }
        })
        .unwrap_or_else(|| {
            if is_json_variant(variant) {
                format!("{}jet_std::Json", cx.root_prefix)
            } else {
                "user_TYPE".to_string()
            }
        })
}

pub(crate) fn is_json_variant(variant: &str) -> bool {
    matches!(
        variant,
        "Null" | "Boolean" | "Number" | "Text" | "Array" | "Object"
    )
}

pub(crate) fn variant_rust_name(variant: &str) -> String {
    if is_json_variant(variant) {
        variant.to_string()
    } else {
        mangle(variant)
    }
}

pub(crate) fn escape_rust_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// D-LABEL1: render the Rust loop-label prefix for a `@name` loop label, e.g.
/// `'jet_outer: `, or empty when the loop is unlabeled. The `jet_` namespace
/// keeps user labels clear of Rust's own lifetime/label names.
pub(crate) fn loop_label_prefix(label: &Option<(String, Span)>) -> String {
    match label {
        Some((n, _)) => format!("'jet_{}: ", n),
        None => String::new(),
    }
}
