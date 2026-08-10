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

/// Assemble the canonical Jet positional-variant shape from its payload.
pub fn jet_debug_variant(variant: &str, payload: String) -> String {
    format!("{variant}({payload})")
}

/// Assemble the canonical Jet optional Debug shape from a rendered payload.
pub fn jet_debug_optional(payload: Option<String>) -> String {
    match payload {
        Some(payload) => format!("Val({payload})"),
        None => "None".to_string(),
    }
}

/// D-TASK-PAUSE-TIER1: one formatter for Task `paused=` / `cancel=` trace text.
/// AOT `JetTask::trace` and the TIR evaluator both call this (I9).
pub fn jet_task_control_trace(paused: bool, cancel: bool) -> String {
    format!("paused={paused},cancel={cancel}")
}

#[cfg(test)]
mod tests {
    use super::jet_task_control_trace;

    #[test]
    fn task_control_trace_is_stable() {
        assert_eq!(
            jet_task_control_trace(true, false),
            "paused=true,cancel=false"
        );
        assert_eq!(
            jet_task_control_trace(false, true),
            "paused=false,cancel=true"
        );
    }
}

/// D-RANGE-VALUE1=A: the standard structural record form for `Range`.
pub fn jet_debug_range(start: i64, end: i64, exclusive: bool) -> String {
    jet_debug_record(
        "Range",
        [
            ("start".to_string(), start.to_string()),
            ("end".to_string(), end.to_string()),
            ("exclusive".to_string(), exclusive.to_string()),
        ],
    )
}
