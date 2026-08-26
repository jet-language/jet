//! Completion: keyword/type tables + completion assembly.

use crate::Syntax;
use crate::AST;
use jetpack::Discovery::Index as DiscoveryIndex;

use super::SymbolDB::{SymKind, SymbolDB};
use jet_foundation::JSON::json_escape;

/// LSP completion item kinds (standard integers).
mod ck {
    pub const METHOD: u8 = 2;
    pub const FUNCTION: u8 = 3;
    pub const FIELD: u8 = 5;
    pub const VARIABLE: u8 = 6;
    pub const CLASS: u8 = 7;
    pub const MODULE: u8 = 9;
    pub const PROPERTY: u8 = 10;
    pub const VALUE: u8 = 12;
    pub const KEYWORD: u8 = 14;
    pub const SNIPPET: u8 = 15;
    pub const ENUM_MEMBER: u8 = 20;
    pub const CONSTANT: u8 = 21;
}

pub(crate) struct CompletionItem {
    pub(crate) label: String,
    kind: u8,
    pub(crate) detail: Option<String>,
    documentation: Option<String>,
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
        let documentation = match &self.documentation {
            Some(value) => format!(r#","documentation":"{}""#, json_escape(value)),
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
            r#"{{"label":"{}","kind":{}{}{}{}{}}}"#,
            json_escape(&self.label),
            self.kind,
            detail,
            documentation,
            insert,
            additional
        )
    }
}

fn semantic_documentation(symbol: &jet_semindex::SemanticSymbol) -> String {
    let provenance = match &symbol.provenance {
        jet_semindex::SemanticProvenance::Source { module_path } => module_path,
        jet_semindex::SemanticProvenance::Builtin { module } => module,
        jet_semindex::SemanticProvenance::CommandRegistry => "command registry",
        jet_semindex::SemanticProvenance::Session => "session",
    };
    format!("{}\n\n{}", symbol.summary, provenance)
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
pub(crate) fn context_is_member_access(src: &str, offset: usize) -> Option<String> {
    let before = &src[..offset.min(src.len())];
    // Walk backward over the current identifier, then check for `.`
    let bytes = before.as_bytes();
    let mut i = bytes.len();
    // skip current word
    while i > 0 && (bytes[i - 1].is_ascii_alphanumeric() || bytes[i - 1] == b'_') {
        i -= 1;
    }
    if i > 0 && bytes[i - 1] == b'.' {
        // Find the dotted receiver before the `.`. This keeps `Door.State`
        // intact so the nested typestate owner is a real completion scope.
        i -= 1;
        let end = i;
        while i > 0
            && (bytes[i - 1].is_ascii_alphanumeric()
                || bytes[i - 1] == b'_'
                || bytes[i - 1] == b'.')
        {
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

/// D-LAYOUT-FACTS1=B: completion context for the compiler-owned `@` member.
/// It is kept separate from ordinary member completion because the marked
/// fact name is a contextual identifier after `.`.
fn context_is_compiler_fact_access(src: &str, offset: usize) -> Option<(String, String)> {
    let before = &src[..offset.min(src.len())];
    let mark = before.rfind('@')?;
    let dot = mark.checked_sub(1)?;
    if before.as_bytes().get(dot) != Some(&b'.') {
        return None;
    }
    let receiver_end = dot;
    let mut receiver_start = receiver_end;
    while receiver_start > 0
        && (before.as_bytes()[receiver_start - 1].is_ascii_alphanumeric()
            || before.as_bytes()[receiver_start - 1] == b'_')
    {
        receiver_start -= 1;
    }
    if receiver_start == receiver_end {
        return None;
    }
    let suffix = &before[mark..];
    if !suffix[1..]
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return None;
    }
    Some((
        before[receiver_start..receiver_end].to_string(),
        suffix.to_string(),
    ))
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
    matches!(kw, "return" | "if" | "else" | "true" | "false" | "break")
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

/// The `State` segment is an erased fact plane, not a runtime member. Keep it
/// available as the first completion step of the canonical `Type.State.Name`
/// path without inventing a second type symbol. `qualified_name` may carry a
/// module alias while `MemberFact.owner` remains the local nominal owner, so
/// return that local owner for the second completion step.
fn state_plane_owner(db: &SymbolDB, type_name: &str) -> Option<String> {
    let local_owner = format!("{type_name}.State");
    if db
        .members
        .iter()
        .any(|member| member.owner == local_owner.as_str())
    {
        return Some(local_owner);
    }
    let qualified_prefix = format!("{type_name}.State.");
    db.symbols
        .symbols()
        .iter()
        .find(|symbol| symbol.qualified_name.starts_with(&qualified_prefix))
        .and_then(|symbol| symbol.owner.clone())
}

fn has_state_plane(db: &SymbolDB, type_name: &str) -> bool {
    state_plane_owner(db, type_name).is_some()
}

fn semantic_member_owner(db: &SymbolDB, receiver_name: &str) -> Option<String> {
    if let Some(definition) = db.defs.iter().find(|def| def.name == receiver_name) {
        match &definition.kind {
            SymKind::Struct { .. } | SymKind::Type { .. } => {
                return Some(definition.name.clone());
            }
            SymKind::Local { ty: Some(ty), .. } | SymKind::Param { ty } => {
                return semantic_owner(ty);
            }
            _ => {}
        }
    }
    if let Some(owner) = state_plane_owner(db, receiver_name) {
        return Some(
            owner
                .strip_suffix(".State")
                .unwrap_or(owner.as_str())
                .to_string(),
        );
    }
    let qualified_prefix = format!("{receiver_name}.");
    db.symbols
        .symbols()
        .iter()
        .find(|symbol| symbol.qualified_name.starts_with(&qualified_prefix))
        .and_then(|symbol| symbol.owner.clone())
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

/// D-CALLVALUE1=B: a function-typed local or parameter exposes the builtin
/// method-shaped invocation member. Struct members still come from the
/// semantic index and therefore keep their ordinary shadowing behavior.
fn function_value_call_completion(prefix: &str) -> Option<CompletionItem> {
    if !Syntax::METHOD_CALL.starts_with(prefix) {
        return None;
    }
    Some(CompletionItem {
        label: Syntax::METHOD_CALL.to_string(),
        kind: ck::METHOD,
        detail: Some("call(...)".to_string()),
        documentation: Some("Invoke this function value.".to_string()),
        insert_text: None,
        insert_text_format: 1,
        auto_import: None,
    })
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
            if index.packages.iter().any(|record| record.source == source) {
                for name in index.package_completions(&source, &prefix) {
                    if seen.insert(format!("pkg:{source}.{name}")) {
                        items.push(CompletionItem {
                            label: name,
                            kind: ck::MODULE,
                            detail: Some(format!("package from {source}")),
                            documentation: None,
                            insert_text: None,
                            insert_text_format: 1,
                            auto_import: None,
                        });
                    }
                }
                return items;
            }
        }

        if let Some(prefix) = context_is_option_field(src, offset) {
            let mut fields = index
                .packages
                .iter()
                .flat_map(|record| record.options.iter())
                .filter(|field| field.name.starts_with(&prefix))
                .collect::<Vec<_>>();
            fields.sort_by(|a, b| a.name.cmp(&b.name));
            if !fields.is_empty() {
                for field in fields {
                    if seen.insert(format!("opt:{}", field.name)) {
                        items.push(CompletionItem {
                            label: field.name.clone(),
                            kind: ck::PROPERTY,
                            detail: Some(format!("default: {}", field.default)),
                            documentation: Some(field.docs.clone()),
                            insert_text: Some(format!("{}: ", field.name)),
                            insert_text_format: 1,
                            auto_import: None,
                        });
                    }
                }
                return items;
            }
        }
    }

    // Member completion: `expr.`
    if let Some((_receiver_name, prefix)) = context_is_compiler_fact_access(src, offset) {
        for fact in Syntax::fact_read_members().filter(|fact| fact.starts_with(&prefix)) {
            let fact_type = Syntax::fact_read_kind(&fact)
                .and_then(|read| read.public_read_type());
            let detail = match fact.as_str() {
                Syntax::COMPILER_FACT_LAYOUT => {
                    format!("compiler fact: {}", Syntax::TYPE_LAYOUT_INFO)
                }
                _ => {
                    fact_type.map_or_else(
                        || {
                            let kind = Syntax::fact_read_kind(&fact)
                                .and_then(|read| read.reflection_kind())
                                .unwrap_or("typed");
                            format!("compiler fact: {kind}")
                        },
                        |type_name| format!("compiler fact: {type_name}"),
                    )
                }
            };
            let documentation = match fact.as_str() {
                Syntax::COMPILER_FACT_LAYOUT => {
                    "Focused layout facts; byte values remain unknown when the target layout is not guaranteed."
                        .to_string()
                }
                Syntax::COMPILER_FACT_ORIGIN => {
                    "Optional tracked origin, derived from sema flow; movement and ambiguity are never reconstructed at runtime."
                        .to_string()
                }
                _ => format!("Registered compiler fact {}.", fact),
            };
            items.push(CompletionItem {
                label: fact,
                kind: ck::PROPERTY,
                detail: Some(detail),
                documentation: Some(documentation),
                insert_text: None,
                insert_text_format: 1,
                auto_import: None,
            });
        }
        return items;
    }

    if let Some(receiver_name) = context_is_member_access(src, offset) {
        let prefix = current_identifier_prefix(src, offset);
        let function_value = db
            .defs
            .iter()
            .find(|def| def.name == receiver_name)
            .is_some_and(|def| match &def.kind {
                SymKind::Local { ty: Some(ty), .. } | SymKind::Param { ty } => {
                    matches!(ty, AST::Type::Fn { .. })
                }
                _ => false,
            });
        if function_value {
            if let Some(item) = function_value_call_completion(&prefix) {
                items.push(item);
            }
            return items;
        }
        if has_state_plane(db, &receiver_name) && "State".starts_with(&prefix) {
            items.push(CompletionItem {
                label: "State".to_string(),
                kind: ck::PROPERTY,
                detail: Some(format!("erased state facts owned by `{receiver_name}`")),
                documentation: Some(format!(
                    "The nested typestate fact plane for `{receiver_name}`."
                )),
                insert_text: None,
                insert_text_format: 1,
                auto_import: None,
            });
        }
        let owner = if receiver_name == crate::Syntax::DURATION_TYPE
            || crate::AST::numeric_type_from_name(&receiver_name).is_some()
        {
            Some(receiver_name.clone())
        } else {
            semantic_member_owner(db, &receiver_name).or_else(|| {
                let anchor = jet_semindex::SemanticVisibilityAnchor {
                    module_path: current_path,
                    offset: Some(offset),
                    session_top_level: false,
                };
                (!db
                    .symbols
                    .complete_visible_at("", Some(&receiver_name), anchor)
                    .is_empty())
                    .then_some(receiver_name.clone())
            })
        };
        if let Some(owner) = owner {
            for symbol in db.symbols.complete_visible_at(
                &prefix,
                Some(&owner),
                jet_semindex::SemanticVisibilityAnchor {
                    module_path: current_path,
                    offset: Some(offset),
                    session_top_level: false,
                },
            ) {
                if seen.insert(symbol.identity.clone()) {
                    items.push(CompletionItem {
                        label: symbol.name.clone(),
                        kind: semantic_completion_kind(symbol),
                        detail: Some(symbol.signature.clone()),
                        documentation: Some(semantic_documentation(symbol)),
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
                if let SymKind::Enum { variants, .. } = &def.kind {
                    for v in variants {
                        let label = format!("{}.{}", enum_type, v);
                        if seen.insert(label.clone()) {
                            items.push(CompletionItem {
                                label: label.clone(),
                                kind: ck::ENUM_MEMBER,
                                detail: Some(format!("variant of {}", enum_type)),
                                documentation: None,
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

    for symbol in db.symbols.complete_visible_at(
        "",
        None,
        jet_semindex::SemanticVisibilityAnchor {
            module_path: current_path,
            offset: Some(offset),
            session_top_level: false,
        },
    ) {
        if symbol.kind == jet_semindex::SemanticSymbolKind::Keyword
            && !context_allows_keyword(src, offset, &symbol.name)
        {
            continue;
        }
        if seen.insert(symbol.identity.clone()) {
            items.push(CompletionItem {
                label: symbol.name.clone(),
                kind: semantic_completion_kind(symbol),
                detail: Some(symbol.signature.clone()),
                documentation: Some(semantic_documentation(symbol)),
                insert_text: None,
                insert_text_format: 1,
                auto_import: match symbol.provenance {
                    jet_semindex::SemanticProvenance::Source { .. }
                        if !matches!(
                            symbol.kind,
                            jet_semindex::SemanticSymbolKind::Local
                                | jet_semindex::SemanticSymbolKind::Parameter
                        ) =>
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
        if seen.insert("contextual:next".to_string()) {
            items.push(CompletionItem {
                label: Syntax::KW_NEXT.to_string(),
                kind: ck::KEYWORD,
                detail: Some("advance the current loop".to_string()),
                documentation: None,
                insert_text: None,
                insert_text_format: 1,
                auto_import: None,
            });
        }
        for (label, detail, insert) in [
            ("bind immut", "name :: value", "${1:name} :: ${2:value}"),
            ("bind mut", "name := value", "${1:name} := ${2:value}"),
        ] {
            if seen.insert(format!("bind:{}", label)) {
                items.push(CompletionItem {
                    label: label.to_string(),
                    kind: ck::SNIPPET,
                    detail: Some(detail.to_string()),
                    documentation: None,
                    insert_text: Some(insert.to_string()),
                    insert_text_format: 2,
                    auto_import: None,
                });
            }
        }
    }

    items
}

pub(crate) fn use_statement_for_module(
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

pub(crate) fn context_is_option_field(src: &str, offset: usize) -> Option<String> {
    let before = &src[..offset.min(src.len())];
    let service = before.rfind("Service{");
    let env = before.rfind("Env{");
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
