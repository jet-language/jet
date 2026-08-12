/// Debug-only field metadata. Storage layout stays declaration-ordered; this
/// order is the canonical developer-facing shape for `IOContext`.
static IO_CONTEXT_DEBUG_FIELDS: &[&str] = &["operation", "resource", "os_code", "cause"];

/// Core field metadata keeps declaration/storage order and redaction policy
/// separate from developer-facing Debug order.
static IO_CONTEXT_FIELD_METADATA: &[(&str, bool)] = &[
    ("operation", false),
    ("resource", false),
    ("os_code", false),
    ("cause", false),
];

/// Return shared field metadata for a core structural type.
pub fn jet_debug_field_metadata(type_name: &str) -> Option<&'static [(&'static str, bool)]> {
    match type_name {
        "IOContext" => Some(IO_CONTEXT_FIELD_METADATA),
        _ => None,
    }
}

/// Return canonical developer-facing field order for a structural type.
pub fn jet_debug_field_order(type_name: &str) -> Option<&'static [&'static str]> {
    match type_name {
        "IOContext" => Some(IO_CONTEXT_DEBUG_FIELDS),
        _ => None,
    }
}

/// One field marshalled into the shared structural Debug formatter.
/// `storage_index` stays separate from developer-facing order so a runtime
/// adapter can extract declaration-ordered storage without changing Debug.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JetDebugField {
    pub name: String,
    pub value: String,
    pub storage_index: usize,
    pub redacted: bool,
}

/// Assemble the canonical Jet record shape from marshalled fields.
pub fn jet_debug_record_fields(
    type_name: &str,
    fields: impl IntoIterator<Item = JetDebugField>,
) -> String {
    let mut fields = fields.into_iter().collect::<Vec<_>>();
    if let Some(order) = jet_debug_field_order(type_name) {
        fields.sort_by_key(|field| {
            order
                .iter()
                .position(|name| field.name.as_str() == *name)
                .map(|position| (0, position))
                .unwrap_or((1, field.storage_index))
        });
    }
    let fields = fields
        .into_iter()
        .map(|field| {
            let value = if field.redacted {
                "[redacted]"
            } else {
                field.value.as_str()
            };
            format!("{}: {value}", field.name)
        })
        .collect::<Vec<_>>();
    if fields.is_empty() {
        format!("{type_name} {{}}")
    } else {
        format!("{type_name} {{ {} }}", fields.join(", "))
    }
}

/// Assemble the canonical Jet record shape from already-rendered fields.
pub fn jet_debug_record(
    type_name: &str,
    fields: impl IntoIterator<Item = (String, String)>,
) -> String {
    jet_debug_record_fields(
        type_name,
        fields
            .into_iter()
            .enumerate()
            .map(|(storage_index, (name, value))| JetDebugField {
                name,
                value,
                storage_index,
                redacted: false,
            }),
    )
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

/// Assemble the canonical Jet positional-variant shape from an optional payload.
pub fn jet_debug_variant(variant: &str, payload: Option<String>) -> String {
    match payload {
        Some(payload) => format!("{variant}({payload})"),
        None => variant.to_string(),
    }
}

/// Assemble the canonical Jet optional Debug shape from a rendered payload.
pub fn jet_debug_optional(payload: Option<String>) -> String {
    match payload {
        Some(payload) => format!("Val({payload})"),
        None => "None".to_string(),
    }
}

/// Shared Prelude JetShow projection for the packed `IOError` carrier.
/// `variant` and `operation` use the declaration order of their core enums;
/// resident tiers marshal those discriminants here instead of reformatting
/// the error locally.
pub fn jet_show_io_error(
    variant: i64,
    operation: i64,
    resource: Option<&str>,
    cause: Option<&str>,
) -> String {
    let kind = match variant {
        0 => "invalid input",
        1 => "not found",
        2 => "permission denied",
        3 => "timed out",
        4 => "cancelled",
        5 => "closed",
        6 => "protocol error",
        _ => "I/O error",
    };
    let operation = match operation {
        0 => "read",
        1 => "write",
        2 => "flush",
        3 => "connect",
        4 => "accept",
        5 => "close",
        6 => "resolve",
        _ => "codec",
    };
    let mut text = format!("{kind} during {operation}");
    if let Some(resource) = resource {
        text.push_str(&format!(" `{resource}`"));
    }
    if let Some(cause) = cause {
        text.push_str(&format!(": {cause}"));
    }
    text
}

/// D-TASK-PAUSE-TIER1: one formatter for Task `paused=` / `cancel=` trace text.
/// AOT `JetTask::trace` and the TIR evaluator both call this (I9).
/// D-TASK-PAUSE-TIER1: one formatter for Task `paused=` / `cancel=` trace text.
/// AOT `JetTask::trace` and the TIR evaluator both call this (I9).
pub fn jet_task_control_trace(paused: bool, cancel: bool) -> String {
    format!("paused={paused},cancel={cancel}")
}

#[cfg(test)]
mod tests {
    use super::{
        jet_debug_field_metadata, jet_debug_record, jet_debug_record_fields, jet_debug_variant,
        JetDebugField,
        jet_task_control_trace,
    };

    #[test]
    fn io_context_debug_fields_use_canonical_order() {
        assert_eq!(
            jet_debug_field_metadata("IOContext")
                .unwrap()
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>(),
            vec!["operation", "resource", "os_code", "cause"]
        );
        assert_eq!(
            jet_debug_record(
                "IOContext",
                [
                    ("cause".to_string(), "None".to_string()),
                    ("os_code".to_string(), "None".to_string()),
                    ("resource".to_string(), "Val".to_string()),
                    ("operation".to_string(), "Read".to_string()),
                ],
            ),
            "IOContext { operation: Read, resource: Val, os_code: None, cause: None }"
        );
        assert_eq!(
            jet_debug_record_fields(
                "Point",
                [JetDebugField {
                    name: "secret".to_string(),
                    value: "42".to_string(),
                    storage_index: 0,
                    redacted: true,
                }],
            ),
            "Point { secret: [redacted] }"
        );
        assert_eq!(jet_debug_variant("Ready", None), "Ready");
    }

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
