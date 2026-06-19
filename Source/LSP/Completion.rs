//! Completion: keyword/type tables + completion assembly.

use crate::AST;

use super::JSON::json_escape;
use super::SymbolDB::{SymKind, SymbolDB};

/// LSP completion item kinds (standard integers).
#[allow(dead_code)]
mod ck {
    pub const TEXT: u8 = 1;
    pub const METHOD: u8 = 2;
    pub const FUNCTION: u8 = 3;
    pub const CONSTRUCTOR: u8 = 4;
    pub const FIELD: u8 = 5;
    pub const VARIABLE: u8 = 6;
    pub const CLASS: u8 = 7;
    pub const INTERFACE: u8 = 8;
    pub const MODULE: u8 = 9;
    pub const PROPERTY: u8 = 10;
    pub const UNIT: u8 = 11;
    pub const VALUE: u8 = 12;
    pub const ENUM: u8 = 13;
    pub const KEYWORD: u8 = 14;
    pub const SNIPPET: u8 = 15;
    pub const COLOR: u8 = 16;
    pub const FILE: u8 = 17;
    pub const REFERENCE: u8 = 18;
    pub const FOLDER: u8 = 19;
    pub const ENUM_MEMBER: u8 = 20;
    pub const CONSTANT: u8 = 21;
    pub const STRUCT: u8 = 22;
    pub const EVENT: u8 = 23;
    pub const OPERATOR: u8 = 24;
    pub const TYPE_PARAMETER: u8 = 25;
}

pub(crate) struct CompletionItem {
    pub(crate) label: String,
    kind: u8,
    detail: Option<String>,
    insert_text: Option<String>,
    insert_text_format: u8, // 1=plain, 2=snippet
    /// D-LSP5: import statement to insert at top of file (auto-import).
    auto_import: Option<String>,
}

impl CompletionItem {
    pub(crate) fn to_json(&self) -> String {
        let detail = match &self.detail {
            Some(d) => format!(r#","detail":"{}""#, json_escape(d)),
            None => String::new(),
        };
        let insert = match &self.insert_text {
            Some(t) => format!(
                r#","insertText":"{}","insertTextFormat":{}"#,
                json_escape(t),
                self.insert_text_format
            ),
            None => String::new(),
        };
        let additional = match &self.auto_import {
            Some(stmt) => format!(
                r#","additionalTextEdits":[{{"range":{{"start":{{"line":0,"character":0}},"end":{{"line":0,"character":0}}}},"newText":"{}"}}]"#,
                json_escape(stmt)
            ),
            None => String::new(),
        };
        format!(
            r#"{{"label":"{}","kind":{}{}{}{}}}"#,
            json_escape(&self.label),
            self.kind,
            detail,
            insert,
            additional
        )
    }
}

/// Jet keywords for completion.
pub(crate) const JET_KEYWORDS: &[&str] = &[
    "fn", "pub", "val", "var", "if", "else", "in", "when", "break", "continue",
    "return", "struct", "enum", "impl", "trait", "const", "comptime", "import", "extern", "test",
    "derive", "mut", "take", "view", "ref", "self", "loop", "unsafe", "or", "true", "false",
    "null", "ok", "err", "value", "it", "pure", "module", "todo", "use",
];

/// Built-in type names for completion.
pub(crate) const JET_TYPES: &[&str] = &[
    "Int", "Float", "Bool", "String", "Char", "List", "Map", "Shared", "Result",
];

/// Is the character sequence before `offset` indicative of member access (`.`)?
fn context_is_member_access(src: &str, offset: usize) -> Option<String> {
    let before = &src[..offset.min(src.len())];
    // Walk backward over the current identifier, then check for `.`
    let bytes = before.as_bytes();
    let mut i = bytes.len();
    // skip current word
    while i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
        i -= 1;
    }
    if i > 0 && bytes[i - 1] == b'.' {
        // Find the word before the `.`
        i -= 1;
        let end = i;
        while i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
            i -= 1;
        }
        if i < end {
            return Some(
                std::str::from_utf8(&bytes[i..end])
                    .unwrap_or("")
                    .to_string(),
            );
        }
    }
    None
}

/// Is the cursor inside a switch body for an enum type?
fn detect_switch_enum_type<'a>(src: &str, offset: usize, db: &'a SymbolDB) -> Option<&'a str> {
    // Look backward for `when <ident> {` pattern
    let before = &src[..offset.min(src.len())];
    if let Some(kw_pos) = before.rfind("when ") {
        let after_kw = before[kw_pos + 5..].trim_start();
        let ident_end = after_kw
            .find(|c: char| !c.is_alphanumeric() && c != '_')
            .unwrap_or(after_kw.len());
        let ident = &after_kw[..ident_end];
        if !ident.is_empty() {
            // Look up ident in DB to find its type
            for d in &db.defs {
                if d.name == ident {
                    if let SymKind::Local {
                        ty: Some(AST::Type::Named(type_name)),
                        ..
                    }
                    | SymKind::Param {
                        ty: AST::Type::Named(type_name),
                    } = &d.kind
                    {
                        // Check if that type is an enum
                        for ed in &db.defs {
                            if ed.name == *type_name {
                                if let SymKind::Enum { .. } = &ed.kind {
                                    return Some(&ed.name);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

pub(crate) fn compute_completions(
    db: &SymbolDB,
    src: &str,
    offset: usize,
    current_path: &str,
) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    // Member completion: `expr.`
    if let Some(receiver_name) = context_is_member_access(src, offset) {
        // Find the type of receiver_name from DB
        for def in &db.defs {
            if def.name == receiver_name {
                match &def.kind {
                    SymKind::Struct { fields } => {
                        for (fname, fty) in fields {
                            if seen.insert(fname.clone()) {
                                items.push(CompletionItem {
                                    label: fname.clone(),
                                    kind: ck::FIELD,
                                    detail: Some(fty.name()),
                                    insert_text: None,
                                    insert_text_format: 1,
                                    auto_import: None,
                                });
                            }
                        }
                    }
                    SymKind::Local {
                        ty: Some(AST::Type::Named(tn)),
                        ..
                    }
                    | SymKind::Param {
                        ty: AST::Type::Named(tn),
                    } => {
                        // look up tn's fields/methods
                        let tn = tn.clone();
                        for td in &db.defs {
                            if td.name == tn {
                                if let SymKind::Struct { fields } = &td.kind {
                                    for (fname, fty) in fields {
                                        if seen.insert(fname.clone()) {
                                            items.push(CompletionItem {
                                                label: fname.clone(),
                                                kind: ck::FIELD,
                                                detail: Some(fty.name()),
                                                insert_text: None,
                                                insert_text_format: 1,
                                                auto_import: None,
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
                // Also add methods
                for md in &db.defs {
                    if let SymKind::Function { params, .. } = &md.kind {
                        if params.first().map(|(n, _)| n.as_str()) == Some("self")
                            || md.module_path == def.module_path
                        {
                            // heuristic: include all methods in same module
                            if seen.insert(format!("m:{}", md.name)) {
                                let detail = format!(
                                    "fn {}({})",
                                    md.name,
                                    params
                                        .iter()
                                        .map(|(n, t)| format!("{}: {}", n, t.name()))
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                );
                                items.push(CompletionItem {
                                    label: md.name.clone(),
                                    kind: ck::METHOD,
                                    detail: Some(detail),
                                    insert_text: None,
                                    insert_text_format: 1,
                                    auto_import: None,
                                });
                            }
                        }
                    }
                }
                return items;
            }
        }
        return items;
    }

    // Switch-arm enum snippet completion
    if let Some(enum_type) = detect_switch_enum_type(src, offset, db) {
        for def in &db.defs {
            if def.name == enum_type {
                if let SymKind::Enum { variants } = &def.kind {
                    for v in variants {
                        let label = format!("{}.{}", enum_type, v);
                        if seen.insert(label.clone()) {
                            items.push(CompletionItem {
                                label: label.clone(),
                                kind: ck::ENUM_MEMBER,
                                detail: Some(format!("variant of {}", enum_type)),
                                insert_text: Some(format!("{}.{} {{}}", enum_type, v)),
                                insert_text_format: 2,
                                auto_import: None,
                            });
                        }
                    }
                    break;
                }
            }
        }
    }

    // D-LSP5: for symbols from other modules, generate an auto-import edit if
    // that module isn't already imported in the current source.
    let auto_import_for = |mp: &str| -> Option<String> {
        if mp == current_path || mp.is_empty() {
            return None;
        }
        if src.contains(&format!("\"{}\"", mp)) {
            return None; // already imported
        }
        Some(format!("import \"{}\";\n", mp))
    };

    // All top-level definitions
    for def in &db.defs {
        match &def.kind {
            SymKind::Function { params, ret: _ } => {
                if seen.insert(def.name.clone()) {
                    let detail = format!(
                        "fn {}({})",
                        def.name,
                        params
                            .iter()
                            .map(|(n, t)| format!("{}: {}", n, t.name()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    items.push(CompletionItem {
                        label: def.name.clone(),
                        kind: ck::FUNCTION,
                        detail: Some(detail),
                        insert_text: None,
                        insert_text_format: 1,
                        auto_import: auto_import_for(&def.module_path),
                    });
                }
            }
            SymKind::Struct { .. } => {
                if seen.insert(def.name.clone()) {
                    items.push(CompletionItem {
                        label: def.name.clone(),
                        kind: ck::STRUCT,
                        detail: Some(format!("struct {}", def.name)),
                        insert_text: None,
                        insert_text_format: 1,
                        auto_import: auto_import_for(&def.module_path),
                    });
                }
            }
            SymKind::Enum { variants } => {
                if seen.insert(def.name.clone()) {
                    items.push(CompletionItem {
                        label: def.name.clone(),
                        kind: ck::ENUM,
                        detail: Some(format!(
                            "enum {} — variants: {}",
                            def.name,
                            variants.join(", ")
                        )),
                        insert_text: None,
                        insert_text_format: 1,
                        auto_import: auto_import_for(&def.module_path),
                    });
                }
            }
            SymKind::Const => {
                if seen.insert(def.name.clone()) {
                    items.push(CompletionItem {
                        label: def.name.clone(),
                        kind: ck::CONSTANT,
                        detail: None,
                        insert_text: None,
                        insert_text_format: 1,
                        auto_import: auto_import_for(&def.module_path),
                    });
                }
            }
            SymKind::Trait => {
                if seen.insert(def.name.clone()) {
                    items.push(CompletionItem {
                        label: def.name.clone(),
                        kind: ck::INTERFACE,
                        detail: Some(format!("trait {}", def.name)),
                        insert_text: None,
                        insert_text_format: 1,
                        auto_import: auto_import_for(&def.module_path),
                    });
                }
            }
            SymKind::Local { mutable: _, ty } => {
                if seen.insert(def.name.clone()) {
                    let detail = ty.as_ref().map(|t| t.name());
                    items.push(CompletionItem {
                        label: def.name.clone(),
                        kind: ck::VARIABLE,
                        detail,
                        insert_text: None,
                        insert_text_format: 1,
                        auto_import: None,
                    });
                }
            }
            SymKind::Param { ty } => {
                if seen.insert(def.name.clone()) {
                    items.push(CompletionItem {
                        label: def.name.clone(),
                        kind: ck::VARIABLE,
                        detail: Some(ty.name()),
                        insert_text: None,
                        insert_text_format: 1,
                        auto_import: None,
                    });
                }
            }
            _ => {}
        }
    }

    // Keywords
    for kw in JET_KEYWORDS {
        if seen.insert(format!("kw:{}", kw)) {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: ck::KEYWORD,
                detail: None,
                insert_text: None,
                insert_text_format: 1,
                auto_import: None,
            });
        }
    }

    // Built-in types
    for ty in JET_TYPES {
        if seen.insert(format!("ty:{}", ty)) {
            items.push(CompletionItem {
                label: ty.to_string(),
                kind: ck::CLASS,
                detail: Some("built-in type".to_string()),
                insert_text: None,
                insert_text_format: 1,
                auto_import: None,
            });
        }
    }

    items
}
