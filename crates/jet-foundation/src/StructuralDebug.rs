/// Assemble the canonical Jet record shape from already-rendered fields.
pub fn jet_debug_record(
    type_name: &str,
    fields: impl IntoIterator<Item = (String, String)>,
) -> String {
    let fields = fields
        .into_iter()
        .map(|(name, value)| format!("{name}: {value}"))
        .collect::<Vec<_>>();
    if fields.is_empty() {
        format!("{type_name} {{}}")
    } else {
        format!("{type_name} {{ {} }}", fields.join(", "))
    }
}

/// Assemble the canonical Jet map shape from already-rendered entries.
pub fn jet_debug_map(entries: impl IntoIterator<Item = (String, String)>) -> String {
    let entries = entries
        .into_iter()
        .map(|(key, value)| format!("{key}: {value}"))
        .collect::<Vec<_>>();
    format!("[:{}]", entries.join(", "))
}

/// A structural union renders as its active payload, without a host tag.
pub fn jet_debug_union(payload: String) -> String {
    payload
}
