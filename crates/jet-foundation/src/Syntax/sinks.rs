//! D-TAG-SURFACE1 / card #1710: one registration table for untrusted-text sinks.
//!
//! Call rows use `(module, method)` lookup. Checked text heads are nominal
//! library declarations, not entries in this credential-sink registry.

/// The policy a sink applies to untrusted text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkClass {
    /// A credential may not reach this call.
    Credential,
}

/// One registered call sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkRow {
    pub module: &'static str,
    pub method: &'static str,
    pub class: SinkClass,
}

impl SinkRow {
    pub const fn call(module: &'static str, method: &'static str, class: SinkClass) -> Self {
        Self {
            module,
            method,
            class,
        }
    }
}

/// One home for credential sinks.
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
];

/// Find the row for a Core call. `*` declares every member in a module.
pub fn sink_row(module: &str, method: &str) -> Option<&'static SinkRow> {
    SINK_ROWS.iter().find(|row| {
        row.module == module && (row.method == method || row.method == "*")
    })
}

/// Return whether a Core call is a credential sink.
pub fn credential_sink(module: &str, method: &str) -> bool {
    matches!(sink_row(module, method).map(|row| row.class), Some(SinkClass::Credential))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_share_call_lookup() {
        assert!(credential_sink("core.encoding.jsonl", "to_string"));
        assert!(!credential_sink("core.encoding.jsonl", "parse"));
    }
}
