use super::*;

/// The Rust type path of the enum that OWNS `variant`. Reads the one enum-path
/// table (`TIR::tir_enum_rust_path`), so an if-let head cannot drift from a
/// match-arm head or an enum literal for the same enum.
pub(crate) fn enum_type_prefix(cx: &Cx, variant: &str) -> String {
    match cx.variant_owner.get(variant) {
        Some(owner) => crate::Codegen::TIR::tir_enum_rust_path(cx, owner).0,
        // No owner registered: the variant name itself names the prelude enum.
        None if is_json_variant(variant) => format!("{}jet_std::DataTree", cx.root_prefix),
        // D-TERM1: `Key` variants are in the top-level prelude as `JetKey`.
        None if is_key_variant(variant) => format!("{}JetKey", cx.root_prefix),
        None => mangle("TYPE"),
    }
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

/// The Rust spelling of `variant` under `enum_type_prefix`'s head — raw for a
/// Rust-defined (Prelude/host) enum, mangled for a Jet-declared one. Same table,
/// same answer as the head, so the two halves of a pattern cannot disagree.
pub(crate) fn variant_rust_name(cx: &Cx, variant: &str) -> String {
    let raw = match cx.variant_owner.get(variant) {
        Some(owner) => crate::Codegen::TIR::tir_enum_rust_path(cx, owner).1,
        None => is_json_variant(variant) || is_key_variant(variant),
    };
    crate::Codegen::TIR::tir_enum_variant_rust_name(variant, raw)
}

pub(crate) fn escape_rust_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
