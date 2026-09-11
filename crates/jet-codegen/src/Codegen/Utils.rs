

// D-ENC-DYN1=A+: the dynamic `Data` value's public variants plus the
// compiler-only typed-JSON numeric carrier.
pub(crate) fn is_json_variant(variant: &str) -> bool {
    crate::Syntax::is_data_variant(variant) || variant == "Number"
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

pub(crate) fn escape_rust_str(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}
