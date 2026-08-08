//! D-TAG-SURFACE1 / card #1710: one registration table for untrusted-text sinks.
//!
//! Call rows use `(module, method)` lookup. Typed-text rows have no call
//! address; they register the type name consumed by typed-text positions.

/// The policy a sink applies to untrusted text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkClass {
    /// A credential may not reach this call.
    Credential,
    /// A typed-text position owns this type name.
    TypedText(&'static str),
}

/// One registered sink or typed-text position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkRow {
    /// Core module for a call sink. `None` for a typed-text position.
    pub module: Option<&'static str>,
    /// Core member for a call sink. `None` for a typed-text position.
    pub method: Option<&'static str>,
    pub class: SinkClass,
}

impl SinkRow {
    pub const fn call(module: &'static str, method: &'static str, class: SinkClass) -> Self {
        Self {
            module: Some(module),
            method: Some(method),
            class,
        }
    }

    pub const fn typed_text(type_name: &'static str) -> Self {
        Self {
            module: None,
            method: None,
            class: SinkClass::TypedText(type_name),
        }
    }
}

/// One home for credential sinks and typed-text positions.
pub const SINK_ROWS: &[SinkRow] = &[
    SinkRow::call("core.io", "print", SinkClass::Credential),
    SinkRow::call("core.io", "eprint", SinkClass::Credential),
    SinkRow::call("core.log", "*", SinkClass::Credential),
    SinkRow::call("core.encoding.json", "to_string", SinkClass::Credential),
    SinkRow::call("core.encoding.json", "to_string_pretty", SinkClass::Credential),
    SinkRow::call("core.encoding.json", "to_bytes", SinkClass::Credential),
    SinkRow::call("core.encoding.json", "to_bytes_canonical", SinkClass::Credential),
    SinkRow::call("core.encoding.csv", "to_string", SinkClass::Credential),
    SinkRow::call("core.encoding.csv", "to_string_pretty", SinkClass::Credential),
    SinkRow::call("core.encoding.csv", "to_bytes", SinkClass::Credential),
    SinkRow::call("core.encoding.csv", "to_bytes_canonical", SinkClass::Credential),
    SinkRow::call("core.encoding.toml", "to_string", SinkClass::Credential),
    SinkRow::call("core.encoding.toml", "to_string_pretty", SinkClass::Credential),
    SinkRow::call("core.encoding.toml", "to_bytes", SinkClass::Credential),
    SinkRow::call("core.encoding.toml", "to_bytes_canonical", SinkClass::Credential),
    SinkRow::call("core.encoding.yaml", "to_string", SinkClass::Credential),
    SinkRow::call("core.encoding.yaml", "to_string_pretty", SinkClass::Credential),
    SinkRow::call("core.encoding.yaml", "to_bytes", SinkClass::Credential),
    SinkRow::call("core.encoding.yaml", "to_bytes_canonical", SinkClass::Credential),
    SinkRow::call("core.encoding.cbor", "to_string", SinkClass::Credential),
    SinkRow::call("core.encoding.cbor", "to_string_pretty", SinkClass::Credential),
    SinkRow::call("core.encoding.cbor", "to_bytes", SinkClass::Credential),
    SinkRow::call("core.encoding.cbor", "to_bytes_canonical", SinkClass::Credential),
    SinkRow::call("core.encoding.xml", "to_string", SinkClass::Credential),
    SinkRow::call("core.encoding.xml", "to_string_pretty", SinkClass::Credential),
    SinkRow::call("core.encoding.xml", "to_bytes", SinkClass::Credential),
    SinkRow::call("core.encoding.xml", "to_bytes_canonical", SinkClass::Credential),
    // Card #1710 regression row: JSONL was absent from the old match.
    SinkRow::call("core.encoding.jsonl", "to_string", SinkClass::Credential),
    SinkRow::typed_text("SQL"),
    SinkRow::typed_text("HTML"),
    SinkRow::typed_text(super::TYPE_SH),
];

/// Find the row for a Core call. `*` declares every member in a module.
pub fn sink_row(module: &str, method: &str) -> Option<&'static SinkRow> {
    SINK_ROWS.iter().find(|row| {
        row.module == Some(module)
            && matches!(row.method, Some(name) if name == method || name == "*")
    })
}

/// Return whether a Core call is a credential sink.
pub fn credential_sink(module: &str, method: &str) -> bool {
    matches!(sink_row(module, method).map(|row| row.class), Some(SinkClass::Credential))
}

/// Return registered typed-text type name, if any.
pub fn typed_text_name(type_name: &str) -> Option<&'static str> {
    SINK_ROWS.iter().find_map(|row| match row.class {
        SinkClass::TypedText(name) if name == type_name => Some(name),
        _ => None,
    })
}

/// Return whether a type is registered as a typed-text position.
pub fn is_typed_text_type(type_name: &str) -> bool {
    typed_text_name(type_name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_share_call_and_typed_text_lookup() {
        assert!(credential_sink("core.encoding.jsonl", "to_string"));
        assert!(!credential_sink("core.encoding.jsonl", "parse"));
        assert!(is_typed_text_type("SQL"));
        assert!(is_typed_text_type("HTML"));
        assert!(is_typed_text_type(super::super::TYPE_SH));
    }
}
