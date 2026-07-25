use std::collections::HashSet;

use super::super::validation_json::json_str;

pub(in crate::Canvas) struct NodeDescriptor {
    pub(in crate::Canvas) id: &'static str,
    pub(in crate::Canvas) kind: &'static str,
    pub(in crate::Canvas) archetype: &'static str,
    projected: bool,
    presentation: PresentationFacts,
    palette: PaletteFacts,
    pub(super) transaction: Option<&'static str>,
    default_editor: &'static str,
}

struct PresentationFacts {
    label: &'static str,
    glyph: &'static str,
    hover: &'static str,
    accent: &'static str,
    header: &'static str,
    style_archetype: &'static str,
    layout_family: &'static str,
    shape: &'static str,
}

struct PaletteFacts {
    visible: bool,
    insertable: bool,
    category: &'static str,
    rank: i32,
    rank_terms: &'static [&'static str],
}

const fn presentation(
    label: &'static str,
    glyph: &'static str,
    hover: &'static str,
    accent: &'static str,
    header: &'static str,
    style_archetype: &'static str,
    layout_family: &'static str,
    shape: &'static str,
) -> PresentationFacts {
    PresentationFacts {
        label,
        glyph,
        hover,
        accent,
        header,
        style_archetype,
        layout_family,
        shape,
    }
}

const fn palette(
    visible: bool,
    insertable: bool,
    category: &'static str,
    rank: i32,
    rank_terms: &'static [&'static str],
) -> PaletteFacts {
    PaletteFacts {
        visible,
        insertable,
        category,
        rank,
        rank_terms,
    }
}

pub(super) static NODE_DESCRIPTORS: &[NodeDescriptor] = &[
    NodeDescriptor {
        id: "entry",
        kind: "entry",
        archetype: "entry",
        projected: true,
        presentation: presentation(
            "Entry",
            "ƒ",
            "Starts this function.",
            "archetype",
            "archetype",
            "entry",
            "entry",
            "node",
        ),
        palette: palette(false, false, "", 0, &[]),
        transaction: None,
        default_editor: "function_signature",
    },
    NodeDescriptor {
        id: "binding",
        kind: "binding",
        archetype: "function_exec",
        projected: true,
        presentation: presentation(
            "Set variable",
            "•",
            "Creates a local variable.",
            "type",
            "#2c333d",
            "control",
            "normal",
            "node",
        ),
        palette: palette(false, false, "Variables", 0, &["binding", "local"]),
        transaction: None,
        default_editor: "inline_expr",
    },
    NodeDescriptor {
        id: "assignment",
        kind: "assign",
        archetype: "function_exec",
        projected: true,
        presentation: presentation(
            "Set variable",
            "•",
            "Changes a variable.",
            "type",
            "#2c333d",
            "control",
            "normal",
            "node",
        ),
        palette: palette(false, false, "Variables", 0, &["assign", "set"]),
        transaction: None,
        default_editor: "inline_expr",
    },
    NodeDescriptor {
        id: "return",
        kind: "return",
        archetype: "control",
        projected: true,
        presentation: presentation(
            "Control",
            "⏎",
            "Returns a value from this function.",
            "#7dd3a6",
            "archetype",
            "control",
            "exit",
            "node",
        ),
        palette: palette(false, false, "Execution", 0, &["return", "exit"]),
        transaction: None,
        default_editor: "inline_expr",
    },
    NodeDescriptor {
        id: "branch",
        kind: "branch",
        archetype: "control",
        projected: true,
        presentation: presentation(
            "Control",
            "◇",
            "Chooses which path runs next.",
            "archetype",
            "archetype",
            "control",
            "control",
            "node",
        ),
        palette: palette(true, true, "Execution", 80, &["if", "else", "branch"]),
        transaction: Some("insert_branch"),
        default_editor: "inline_expr",
    },
    NodeDescriptor {
        id: "dispatch",
        kind: "function",
        archetype: "control",
        projected: true,
        presentation: presentation(
            "Control",
            "◇",
            "Chooses a path by matching a value.",
            "archetype",
            "archetype",
            "control",
            "control",
            "node",
        ),
        palette: palette(true, true, "Execution", 78, &["switch", "match", "dispatch"]),
        transaction: Some("insert_switch"),
        default_editor: "pattern_arm",
    },
    NodeDescriptor {
        id: "loop",
        kind: "loop",
        archetype: "control",
        projected: true,
        presentation: presentation(
            "Control",
            "↻",
            "Repeats work.",
            "archetype",
            "archetype",
            "control",
            "control",
            "node",
        ),
        palette: palette(true, true, "Execution", 76, &["loop", "repeat", "for", "while"]),
        transaction: Some("insert_loop"),
        default_editor: "inline_expr",
    },
    NodeDescriptor {
        id: "flow",
        kind: "flow",
        archetype: "control",
        projected: true,
        presentation: presentation(
            "Control",
            "◇",
            "Changes loop control flow.",
            "archetype",
            "archetype",
            "control",
            "control",
            "node",
        ),
        palette: palette(false, false, "Execution", 0, &["break", "continue"]),
        transaction: None,
        default_editor: "none",
    },
    NodeDescriptor {
        id: "yield",
        kind: "yield",
        archetype: "function_exec",
        projected: true,
        presentation: presentation(
            "Function",
            "ƒ",
            "Yields a stream value.",
            "archetype",
            "archetype",
            "function_exec",
            "normal",
            "node",
        ),
        palette: palette(false, false, "Execution", 0, &["yield", "stream"]),
        transaction: None,
        default_editor: "inline_expr",
    },
    NodeDescriptor {
        id: "variable_get",
        kind: "variable_get",
        archetype: "value",
        projected: true,
        presentation: presentation(
            "Value",
            "•",
            "Reads a variable.",
            "type",
            "type",
            "value",
            "value",
            "capsule",
        ),
        palette: palette(true, false, "Variables", 84, &["get", "read", "variable"]),
        transaction: None,
        default_editor: "inline_expr",
    },
    NodeDescriptor {
        id: "constant",
        kind: "constant",
        archetype: "value",
        projected: true,
        presentation: presentation(
            "Literal",
            "•",
            "Uses a fixed value.",
            "type",
            "type",
            "value",
            "value",
            "capsule",
        ),
        palette: palette(false, false, "Values", 0, &["literal", "constant", "value"]),
        transaction: None,
        default_editor: "inline_value",
    },
    NodeDescriptor {
        id: "function_exec",
        kind: "function",
        archetype: "function_exec",
        projected: true,
        presentation: presentation(
            "Function",
            "ƒ",
            "Calls a function.",
            "archetype",
            "archetype",
            "function_exec",
            "normal",
            "node",
        ),
        palette: palette(true, true, "Project", 72, &["call", "function"]),
        transaction: Some("insert_call"),
        default_editor: "function_pins",
    },
    NodeDescriptor {
        id: "function_pure",
        kind: "function",
        archetype: "function_pure",
        projected: true,
        presentation: presentation(
            "Pure function",
            "ƒ",
            "Calls a pure function.",
            "archetype",
            "archetype",
            "function_pure",
            "normal",
            "node",
        ),
        palette: palette(true, true, "Project", 74, &["call", "pure", "function"]),
        transaction: Some("insert_call"),
        default_editor: "function_pins",
    },
    NodeDescriptor {
        id: "variant",
        kind: "variant",
        archetype: "function_pure",
        projected: true,
        presentation: presentation(
            "Pure function",
            "ƒ",
            "Creates an enum variant.",
            "archetype",
            "archetype",
            "function_pure",
            "normal",
            "node",
        ),
        palette: palette(false, false, "Values", 0, &["enum", "variant"]),
        transaction: None,
        default_editor: "function_pins",
    },
    NodeDescriptor {
        id: "fallible",
        kind: "fallible",
        archetype: "control",
        projected: true,
        presentation: presentation(
            "Control",
            "◇",
            "Routes a fallible result.",
            "archetype",
            "archetype",
            "control",
            "control",
            "node",
        ),
        palette: palette(
            true,
            true,
            "Execution",
            70,
            &["fallible", "error", "result", "question"],
        ),
        transaction: Some("insert_fallible_rail"),
        default_editor: "fallback",
    },
    NodeDescriptor {
        id: "expression",
        kind: "expr",
        archetype: "function_pure",
        projected: true,
        presentation: presentation(
            "Pure function",
            "ƒ",
            "Computes a value.",
            "archetype",
            "archetype",
            "function_pure",
            "normal",
            "node",
        ),
        palette: palette(false, false, "Values", 0, &["expression", "value"]),
        transaction: None,
        default_editor: "inline_expr",
    },
];

pub(super) fn descriptor_for(kind: &str, archetype: &str) -> &'static NodeDescriptor {
    validate_catalog().unwrap_or_else(|message| panic!("invalid Canvas node catalog: {message}"));
    NODE_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.kind == kind && descriptor.archetype == archetype)
        .unwrap_or_else(|| panic!("missing Canvas node descriptor for {kind}/{archetype}"))
}

pub(in crate::Canvas) fn descriptor_for_id(id: &str) -> &'static NodeDescriptor {
    validate_catalog().unwrap_or_else(|message| panic!("invalid Canvas node catalog: {message}"));
    NODE_DESCRIPTORS
        .iter()
        .find(|descriptor| descriptor.id == id)
        .unwrap_or_else(|| panic!("missing Canvas node descriptor `{id}`"))
}

pub(in crate::Canvas) fn insert_descriptor_id(transaction: &str, pure: bool) -> &'static str {
    let descriptor = if transaction == "insert_call" {
        descriptor_for_id(if pure {
            "function_pure"
        } else {
            "function_exec"
        })
    } else {
        NODE_DESCRIPTORS
            .iter()
            .find(|descriptor| descriptor.transaction == Some(transaction))
            .unwrap_or_else(|| {
                panic!("missing Canvas node descriptor transaction `{transaction}`")
            })
    };
    assert!(
        descriptor.palette.insertable,
        "Canvas action descriptor `{}` is not insertable",
        descriptor.id
    );
    descriptor.id
}

pub(in crate::Canvas) fn catalog_json() -> String {
    validate_catalog().unwrap_or_else(|message| panic!("invalid Canvas node catalog: {message}"));
    format!(
        "[{}]",
        NODE_DESCRIPTORS
            .iter()
            .map(descriptor_json)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn validate_catalog() -> Result<(), String> {
    let mut ids = HashSet::new();
    let mut identities = HashSet::new();
    for descriptor in NODE_DESCRIPTORS {
        if descriptor.id.is_empty() || !ids.insert(descriptor.id) {
            return Err(format!("missing or duplicate id `{}`", descriptor.id));
        }
        if !identities.insert((descriptor.kind, descriptor.archetype)) {
            return Err(format!(
                "duplicate identity `{}/{}`",
                descriptor.kind, descriptor.archetype
            ));
        }
        if !descriptor.projected && !descriptor.palette.insertable {
            return Err(format!("orphaned descriptor `{}`", descriptor.id));
        }
        if descriptor.palette.insertable != descriptor.transaction.is_some() {
            return Err(format!(
                "insertable/transaction mismatch for `{}`",
                descriptor.id
            ));
        }
        if descriptor.palette.insertable
            && (!descriptor.palette.visible || descriptor.palette.category.is_empty())
        {
            return Err(format!("non-visible insertable descriptor `{}`", descriptor.id));
        }
        if let Some(transaction) = descriptor.transaction {
            if !matches!(
                transaction,
                "edit_inline_expr"
                    | "insert_branch"
                    | "insert_switch"
                    | "insert_loop"
                    | "insert_fallible_rail"
                    | "insert_call"
            ) {
                return Err(format!(
                    "unsupported transaction `{transaction}` for `{}`",
                    descriptor.id
                ));
            }
        }
    }
    Ok(())
}

fn descriptor_json(descriptor: &NodeDescriptor) -> String {
    format!(
        "{{\"id\":{},\"kind\":{},\"archetype\":{},\"projected\":{},\"presentation\":{{\"label\":{},\"glyph\":{},\"hover\":{},\"accent\":{},\"header\":{},\"style_archetype\":{},\"layout_family\":{},\"shape\":{}}},\"palette\":{{\"visible\":{},\"insertable\":{},\"category\":{},\"rank\":{},\"rank_terms\":[{}]}},\"transaction\":{},\"default_editor\":{}}}",
        json_str(descriptor.id),
        json_str(descriptor.kind),
        json_str(descriptor.archetype),
        if descriptor.projected { "true" } else { "false" },
        json_str(descriptor.presentation.label),
        json_str(descriptor.presentation.glyph),
        json_str(descriptor.presentation.hover),
        json_str(descriptor.presentation.accent),
        json_str(descriptor.presentation.header),
        json_str(descriptor.presentation.style_archetype),
        json_str(descriptor.presentation.layout_family),
        json_str(descriptor.presentation.shape),
        if descriptor.palette.visible { "true" } else { "false" },
        if descriptor.palette.insertable {
            "true"
        } else {
            "false"
        },
        json_str(descriptor.palette.category),
        descriptor.palette.rank,
        descriptor
            .palette
            .rank_terms
            .iter()
            .map(|term| json_str(term))
            .collect::<Vec<_>>()
            .join(","),
        descriptor
            .transaction
            .map(json_str)
            .unwrap_or_else(|| "null".to_string()),
        json_str(descriptor.default_editor),
    )
}
