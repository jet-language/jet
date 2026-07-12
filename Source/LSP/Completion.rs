//! Completion: keyword/type tables + completion assembly.

use crate::Jetpack::Discovery::Index as DiscoveryIndex;
use crate::Syntax;
use crate::AST;

use super::SymbolDB::{SymKind, SymbolDB};
use super::JSON::json_escape;

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
    pub(crate) detail: Option<String>,
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

/// Jet keywords for completion and rename validation.
///
/// Derives directly from `Syntax::JET_KEYWORD_LIST` (c44 consolidation).
/// Do NOT duplicate this list here — add keywords to Syntax.rs instead.
///
/// Paused or live teaching words must NOT appear here. Drift from Syntax.rs is
/// impossible: this is just an alias. Guarded by tests::c44_keyword_drift.
pub(crate) const JET_KEYWORDS: &[&str] = Syntax::JET_KEYWORD_LIST;

/// Built-in type names for completion and rename guard.
///
/// Derives directly from `Syntax::JET_TYPE_LIST` (c44 consolidation).
/// Do NOT duplicate this list here — add types to Syntax.rs instead.
pub(crate) const JET_TYPES: &[&str] = Syntax::JET_TYPE_LIST;

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

/// True when the cursor is at the start of a statement (indentation after `{` or blank line).
fn context_is_binding_start(src: &str, offset: usize) -> bool {
    let before = &src[..offset.min(src.len())];
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let prefix = before[line_start..].trim();
    if !prefix.is_empty() {
        return false;
    }
    let prior = before[..line_start].trim_end();
    prior.is_empty() || prior.ends_with('{')
}

fn context_allows_keyword(src: &str, offset: usize, kw: &str) -> bool {
    if context_is_binding_start(src, offset) {
        return true;
    }
    matches!(
        kw,
        "return" | "if" | "else" | "when" | "true" | "false" | "break" | "continue"
    )
}

fn semantic_owner(ty: &AST::Type) -> Option<String> {
    match ty {
        AST::Type::Named(name) => Some(name.clone()),
        AST::Type::List(_) | AST::Type::FixedList { .. } => Some("List".to_string()),
        AST::Type::Map { .. } => Some("Map".to_string()),
        AST::Type::String => Some("String".to_string()),
        _ => None,
    }
}

fn semantic_completion_kind(symbol: &jet_semindex::SemanticSymbol) -> u8 {
    use jet_semindex::SemanticSymbolKind;
    match symbol.kind {
        SemanticSymbolKind::Module => ck::MODULE,
        SemanticSymbolKind::Function if symbol.owner.is_some() => ck::METHOD,
        SemanticSymbolKind::Function => ck::FUNCTION,
        SemanticSymbolKind::Type => ck::CLASS,
        SemanticSymbolKind::Member => ck::FIELD,
        SemanticSymbolKind::Constant => ck::CONSTANT,
        SemanticSymbolKind::Local | SemanticSymbolKind::Parameter => ck::VARIABLE,
        SemanticSymbolKind::Keyword => ck::KEYWORD,
        SemanticSymbolKind::Command => ck::VALUE,
    }
}

pub(crate) fn compute_completions(
    db: &SymbolDB,
    src: &str,
    offset: usize,
    current_path: &str,
    workspace_root: Option<&str>,
    discovery: Option<&DiscoveryIndex>,
) -> Vec<CompletionItem> {
    let mut items: Vec<CompletionItem> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    if let Some(index) = discovery {
        if let Some((source, prefix)) = context_is_package_ref(src, offset) {
            for name in index.package_completions(&source, &prefix) {
                if seen.insert(format!("pkg:{source}.{name}")) {
                    items.push(CompletionItem {
                        label: name,
                        kind: ck::MODULE,
                        detail: Some(format!("package from {source}")),
                        insert_text: None,
                        insert_text_format: 1,
                        auto_import: None,
                    });
                }
            }
            return items;
        }

        if let Some(prefix) = context_is_option_field(src, offset) {
            let mut fields = index
                .packages
                .iter()
                .flat_map(|record| record.options.iter())
                .filter(|field| field.name.starts_with(&prefix))
                .collect::<Vec<_>>();
            fields.sort_by(|a, b| a.name.cmp(&b.name));
            for field in fields {
                if seen.insert(format!("opt:{}", field.name)) {
                    items.push(CompletionItem {
                        label: field.name.clone(),
                        kind: ck::PROPERTY,
                        detail: Some(format!("default: {} - {}", field.default, field.docs)),
                        insert_text: Some(format!("{}: ", field.name)),
                        insert_text_format: 1,
                        auto_import: None,
                    });
                }
            }
            return items;
        }
    }

    // Member completion: `expr.`
    if let Some(receiver_name) = context_is_member_access(src, offset) {
        let owner = db.defs.iter().find(|def| def.name == receiver_name).and_then(|def| {
            match &def.kind {
                SymKind::Struct { .. } => Some(def.name.clone()),
                SymKind::Local { ty: Some(ty), .. } | SymKind::Param { ty } => semantic_owner(ty),
                _ => None,
            }
        });
        if let Some(owner) = owner {
            let prefix = current_identifier_prefix(src, offset);
            for symbol in db.symbols.complete(&prefix, Some(&owner)) {
                if seen.insert(symbol.identity.clone()) {
                    items.push(CompletionItem {
                        label: symbol.name.clone(),
                        kind: semantic_completion_kind(symbol),
                        detail: Some(symbol.signature.clone()),
                        insert_text: None,
                        insert_text_format: 1,
                        auto_import: None,
                    });
                }
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
        let statement = use_statement_for_module(current_path, workspace_root, mp)?;
        let already_imported = statement.trim().strip_prefix("use ").is_some_and(|module| {
            src.lines()
                .any(|line| line.trim() == format!("use {module}"))
        });
        if already_imported {
            return None; // already imported
        }
        Some(statement)
    };

    for symbol in db.symbols.symbols().iter().filter(|symbol| symbol.owner.is_none()) {
        if symbol.kind == jet_semindex::SemanticSymbolKind::Keyword
            && (!context_allows_keyword(src, offset, &symbol.name)
                || db.symbols.symbols().iter().any(|candidate| {
                    candidate.owner.is_none()
                        && candidate.name == symbol.name
                        && candidate.kind != jet_semindex::SemanticSymbolKind::Keyword
                }))
        {
            continue;
        }
        if seen.insert(symbol.identity.clone()) {
            items.push(CompletionItem {
                label: symbol.name.clone(),
                kind: semantic_completion_kind(symbol),
                detail: Some(symbol.signature.clone()),
                insert_text: None,
                insert_text_format: 1,
                auto_import: match symbol.provenance {
                    jet_semindex::SemanticProvenance::Source { .. }
                        if !matches!(symbol.kind, jet_semindex::SemanticSymbolKind::Local | jet_semindex::SemanticSymbolKind::Parameter) =>
                    {
                        auto_import_for(&symbol.module_path)
                    }
                    _ => None,
                },
            });
        }
    }

    // D-BINDEXPLICIT1: binding snippets with canonical sigils.
    if context_is_binding_start(src, offset) {
        for (label, detail, insert) in [
            (
                "bind immut (inferred)",
                "name :: value",
                "${1:name} :: ${2:value}",
            ),
            (
                "bind mut (inferred)",
                "name := value",
                "${1:name} := ${2:value}",
            ),
            (
                "bind immut (explicit)",
                "name: Type :: value",
                "${1:name}: ${2:Type} :: ${3:value}",
            ),
            (
                "bind mut (explicit)",
                "name: Type := value",
                "${1:name}: ${2:Type} := ${3:value}",
            ),
        ] {
            if seen.insert(format!("bind:{}", label)) {
                items.push(CompletionItem {
                    label: label.to_string(),
                    kind: ck::SNIPPET,
                    detail: Some(detail.to_string()),
                    insert_text: Some(insert.to_string()),
                    insert_text_format: 2,
                    auto_import: None,
                });
            }
        }
    }

    items
}

fn use_statement_for_module(
    current_path: &str,
    workspace_root: Option<&str>,
    module_path: &str,
) -> Option<String> {
    let current_dir = std::path::Path::new(current_path).parent()?;
    let base = workspace_root
        .map(std::path::Path::new)
        .unwrap_or(current_dir);
    let module = std::path::Path::new(module_path);
    let rel = module.strip_prefix(base).ok()?;
    let rel = rel.to_string_lossy();
    let rel = rel.trim_end_matches(".jet");
    let module_name = rel
        .split(std::path::MAIN_SEPARATOR)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(".");
    (!module_name.is_empty()).then(|| format!("use {}\n", module_name))
}

fn context_is_package_ref(src: &str, offset: usize) -> Option<(String, String)> {
    let source = context_is_member_access(src, offset)?;
    let prefix = current_identifier_prefix(src, offset);
    Some((source, prefix))
}

fn context_is_option_field(src: &str, offset: usize) -> Option<String> {
    let before = &src[..offset.min(src.len())];
    let service = before.rfind("Service.{");
    let env = before.rfind("Env.{");
    let marker = match (service, env) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(a), None) | (None, Some(a)) => Some(a),
        (None, None) => None,
    }?;
    let body = &before[marker..];
    if body
        .rfind('}')
        .is_some_and(|close| body.rfind('{').map(|open| close > open).unwrap_or(false))
    {
        return None;
    }
    Some(current_identifier_prefix(src, offset))
}

fn current_identifier_prefix(src: &str, offset: usize) -> String {
    let before = &src[..offset.min(src.len())];
    let start = before
        .rfind(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .map(|i| i + 1)
        .unwrap_or(0);
    before[start..].to_string()
}
