//! LSP language features: hover, go-to-definition, references, rename,
//! semantic tokens, inlay hints.

use crate::Lexer::{TokKind, Token};
use crate::Diagnostics::Span;

use super::Completion::{JET_KEYWORDS, JET_TYPES};
use super::JSON::json_escape;
use super::Position::byte_offset_to_lsp;
use super::SymbolDB::{InlayHint, SymKind, SymbolDB};

// ── Hover ─────────────────────────────────────────────────────────────────────

/// B7 (D-LSP6): Collect adjacent `///` doc-comment lines immediately preceding
/// `def_start` in the raw token stream (which includes LineComment tokens).
fn collect_doc_comment(tokens: &[Token], def_start: usize) -> Option<String> {
    // Find the first token at or after def_start.
    let idx = tokens.partition_point(|t| t.span.end <= def_start);
    let mut lines: Vec<String> = Vec::new();
    let mut j = idx;
    loop {
        if j == 0 {
            break;
        }
        j -= 1;
        match &tokens[j].kind {
            TokKind::LineComment(text) if text.starts_with("///") => {
                let doc = text.trim_start_matches('/').trim().to_string();
                lines.push(doc);
            }
            // A regular `//` comment or any non-comment token stops the search.
            _ => break,
        }
    }
    if lines.is_empty() {
        return None;
    }
    lines.reverse();
    Some(lines.join("\n"))
}

pub(crate) fn compute_hover(
    db: &SymbolDB,
    tokens: &[Token],
    _src: &str,
    path: &str,
    offset: usize,
) -> Option<String> {
    // Collect the base hover text (type signature / ownership annotation).
    let base = if let Some(text) = db.hover_at(path, offset) {
        text.to_string()
    } else {
        // Fall back: find the token at offset and look up the name.
        let name = find_ident_at(tokens, offset)?;
        if let Some(def) = db.defs.iter().find(|d| d.name == name) {
            match &def.kind {
                SymKind::Function { params, ret } => {
                    let ps: Vec<String> = params
                        .iter()
                        .map(|(n, t)| format!("{}: {}", n, t.name()))
                        .collect();
                    let r = match ret {
                        Some(t) => format!(" -> {}", t.name()),
                        None => String::new(),
                    };
                    format!("fn {}({}){}", name, ps.join(", "), r)
                }
                SymKind::Struct { fields } => {
                    format!(
                        "struct `{}`\n\nFields: {}",
                        name,
                        fields
                            .iter()
                            .map(|(n, t)| format!("{}: {}", n, t.name()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }
                SymKind::Enum { variants } => {
                    format!("enum `{}`\n\nVariants: {}", name, variants.join(", "))
                }
                SymKind::Trait => format!("trait `{}`", name),
                SymKind::Const => format!("const `{}`", name),
                SymKind::EnumVariant { parent } => format!("`{}` — variant of `{}`", name, parent),
                SymKind::Field { ty, parent } => {
                    format!("`{}`: {} (field of `{}`)", name, ty.name(), parent)
                }
                SymKind::Local { mutable, ty } => {
                    let kw = if *mutable { "var" } else { "val" };
                    match ty {
                        Some(t) => format!("`{}`: {} ({})", name, t.name(), kw),
                        None => format!("`{}` ({})", name, kw),
                    }
                }
                SymKind::Param { ty } => format!("`{}`: {} (parameter)", name, ty.name()),
            }
        } else {
            return None;
        }
    };

    // B7: prepend any `///` doc comment lines found before the definition.
    let name = find_ident_at(tokens, offset);
    if let Some(name) = name {
        if let Some(def) = db
            .defs
            .iter()
            .find(|d| d.name == name && d.module_path == path)
        {
            if let Some(doc) = collect_doc_comment(tokens, def.def_span.start) {
                return Some(format!("{}\n\n---\n\n{}", doc, base));
            }
        }
    }
    Some(base)
}

fn find_ident_at<'a>(tokens: &'a [Token], offset: usize) -> Option<&'a str> {
    for tok in tokens {
        if tok.span.start <= offset && offset <= tok.span.end {
            if let TokKind::Ident(name) = &tok.kind {
                return Some(name.as_str());
            }
        }
    }
    None
}

// ── Go-to-definition ──────────────────────────────────────────────────────────

pub(crate) fn compute_definition(
    db: &SymbolDB,
    tokens: &[Token],
    src: &str,
    path: &str,
    offset: usize,
) -> Option<(String, Span)> {
    let name = find_ident_at(tokens, offset)?;
    // Look for a top-level or local def with this name
    // Prefer defs in same module, then other modules
    if let Some(def) = db
        .defs
        .iter()
        .find(|d| d.name == name && d.module_path == path)
    {
        return Some((def.module_path.clone(), def.def_span));
    }
    if let Some(def) = db.defs.iter().find(|d| d.name == name) {
        return Some((def.module_path.clone(), def.def_span));
    }
    None
}

// ── References ────────────────────────────────────────────────────────────────

pub(crate) fn compute_references(
    db: &SymbolDB,
    tokens: &[Token],
    path: &str,
    offset: usize,
    include_declaration: bool,
) -> Vec<(String, Span)> {
    let name = match find_ident_at(tokens, offset) {
        Some(n) => n,
        None => return Vec::new(),
    };
    let mut result: Vec<(String, Span)> = db
        .refs
        .iter()
        .filter(|r| r.name == name)
        .map(|r| (r.module_path.clone(), r.span))
        .collect();
    if include_declaration {
        for def in db.defs.iter().filter(|d| d.name == name) {
            result.push((def.module_path.clone(), def.def_span));
        }
    }
    result
}

// ── Rename ────────────────────────────────────────────────────────────────────

fn is_keyword(name: &str) -> bool {
    JET_KEYWORDS.contains(&name) || JET_TYPES.contains(&name)
}

fn is_valid_ident(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !first.is_alphabetic() && first != '_' {
        return false;
    }
    chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// Compute a workspace edit for renaming the symbol at `offset` to `new_name`.
/// Returns `Err(msg)` if the rename is invalid.
pub(crate) fn compute_rename(
    db: &SymbolDB,
    tokens: &[Token],
    path: &str,
    offset: usize,
    new_name: &str,
) -> Result<Vec<(String, Span)>, String> {
    if !is_valid_ident(new_name) {
        return Err(format!("`{}` is not a valid identifier", new_name));
    }
    if is_keyword(new_name) {
        return Err(format!(
            "`{}` is a keyword and cannot be used as a name",
            new_name
        ));
    }
    let name = match find_ident_at(tokens, offset) {
        Some(n) => n,
        None => return Err("no identifier at cursor".to_string()),
    };
    if is_keyword(name) {
        return Err(format!("`{}` is a keyword and cannot be renamed", name));
    }
    let mut spans: Vec<(String, Span)> = Vec::new();
    // Include definition spans
    for def in db.defs.iter().filter(|d| d.name == name) {
        spans.push((def.module_path.clone(), def.def_span));
    }
    // Include all reference spans
    for r in db.refs.iter().filter(|r| r.name == name) {
        spans.push((r.module_path.clone(), r.span));
    }
    if spans.is_empty() {
        return Err(format!("no occurrences of `{}` found", name));
    }
    Ok(spans)
}

// ── Semantic tokens ───────────────────────────────────────────────────────────
//
// Token type indices (must match the legend in initialize_response).
#[allow(dead_code)] // wired in c41 (semantic token highlighting)
mod st {
    pub const KEYWORD: u32 = 0;
    pub const TYPE: u32 = 1;
    pub const FUNCTION: u32 = 2;
    pub const VARIABLE: u32 = 3;
    pub const PARAMETER: u32 = 4;
    pub const PROPERTY: u32 = 5;
    pub const ENUM_MEMBER: u32 = 6;
    pub const STRING: u32 = 7;
    pub const NUMBER: u32 = 8;
    pub const COMMENT: u32 = 9;
    pub const OPERATOR: u32 = 10;
    pub const NAMESPACE: u32 = 11;
}

// Modifier bitmasks
#[allow(dead_code)] // wired in c41 (semantic token highlighting)
mod sm {
    pub const DECLARATION: u32 = 1 << 0;
    pub const READONLY: u32 = 1 << 1;
}

fn semantic_token_type_for(tok: &Token) -> Option<(u32, u32)> {
    match &tok.kind {
        TokKind::KwFn
        | TokKind::KwPub
        | TokKind::KwIf
        | TokKind::KwElse
        | TokKind::KwWhile
        | TokKind::KwFor
        | TokKind::KwIn
        | TokKind::KwSwitch
        | TokKind::KwBreak
        | TokKind::KwContinue
        | TokKind::KwReturn
        | TokKind::KwStruct
        | TokKind::KwEnum
        | TokKind::KwImpl
        | TokKind::KwTrait
        | TokKind::KwDerive
        | TokKind::KwConst
        | TokKind::KwComptime
        | TokKind::KwUse
        | TokKind::KwExtern
        | TokKind::KwLoop
        | TokKind::KwUnsafe
        | TokKind::KwMutate
        | TokKind::KwMove
        | TokKind::KwView
        | TokKind::KwStored
        | TokKind::KwSelf
        | TokKind::KwNull
        | TokKind::KwOk
        | TokKind::KwErr
        | TokKind::KwIt
        | TokKind::KwModule => Some((st::KEYWORD, 0)),

        TokKind::KwTrue | TokKind::KwFalse => Some((st::KEYWORD, sm::READONLY)),

        TokKind::Ident(name) => {
            // Classify identifiers by name convention:
            // PascalCase → type, everything else → variable
            if name.starts_with(|c: char| c.is_uppercase()) {
                Some((st::TYPE, 0))
            } else {
                Some((st::VARIABLE, 0))
            }
        }

        TokKind::Str(_) => Some((st::STRING, 0)),

        TokKind::Int(_) | TokKind::Float(_) | TokKind::Char(_) => Some((st::NUMBER, 0)),

        TokKind::LineComment(_) | TokKind::BlockComment(_) => Some((st::COMMENT, 0)),

        TokKind::Plus
        | TokKind::Minus
        | TokKind::Star
        | TokKind::Slash
        | TokKind::Percent
        | TokKind::Amp
        | TokKind::Pipe
        | TokKind::Caret
        | TokKind::Shl
        | TokKind::Shr
        | TokKind::AndAnd
        | TokKind::OrOr
        | TokKind::Bang
        | TokKind::EqEq
        | TokKind::NotEq
        | TokKind::Lt
        | TokKind::Gt
        | TokKind::Le
        | TokKind::Ge
        | TokKind::Arrow
        | TokKind::LambdaArrow
        | TokKind::Question
        | TokKind::DotDot => Some((st::OPERATOR, 0)),

        _ => None,
    }
}

/// Encode semantic tokens for a token stream into the LSP delta-encoded u32 array.
pub(crate) fn encode_semantic_tokens(tokens: &[Token], src: &str) -> Vec<u32> {
    let mut data: Vec<u32> = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for tok in tokens {
        if matches!(tok.kind, TokKind::Eof) {
            break;
        }
        let (tok_type, tok_mods) = match semantic_token_type_for(tok) {
            Some(t) => t,
            None => continue,
        };
        let lsp_start = byte_offset_to_lsp(src, tok.span.start);
        let line = lsp_start.line;
        let start = lsp_start.character;

        // Compute length in UTF-16 code units
        let text = src
            .get(tok.span.start..tok.span.end.min(src.len()))
            .unwrap_or("");
        // For multi-line tokens (strings with newlines) just use first line
        let first_line_text: &str = text.split('\n').next().unwrap_or(text);
        let length = first_line_text.encode_utf16().count() as u32;
        if length == 0 {
            continue;
        }

        let delta_line = line - prev_line;
        let delta_start = if delta_line == 0 {
            start - prev_start
        } else {
            start
        };

        data.push(delta_line);
        data.push(delta_start);
        data.push(length);
        data.push(tok_type);
        data.push(tok_mods);

        prev_line = line;
        prev_start = start;
    }
    data
}

// ── Inlay hints ───────────────────────────────────────────────────────────────

pub(crate) fn format_inlay_hints(hints: &[&InlayHint], src: &str) -> String {
    let mut items = String::new();
    for (i, h) in hints.iter().enumerate() {
        if i > 0 {
            items.push(',');
        }
        // Position: just after the name span
        let pos = byte_offset_to_lsp(src, h.span.end);
        items.push_str(&format!(
            r#"{{"position":{{"line":{},"character":{}}},"label":"{}","kind":1}}"#,
            pos.line,
            pos.character,
            json_escape(&h.label)
        ));
    }
    format!("[{}]", items)
}
