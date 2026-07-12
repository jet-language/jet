//! LSP language features: hover, go-to-definition, references, rename,
//! semantic tokens, inlay hints.

use crate::Diagnostics::Span;
use crate::Lexer::{TokKind, Token};

use super::Completion::{JET_KEYWORDS, JET_TYPES};
use super::Position::byte_offset_to_lsp;
use super::SymbolDB::{InlayHint, SymbolDB};
use super::JSON::json_escape;

// ── Hover ─────────────────────────────────────────────────────────────────────

fn semantic_hover(symbol: &jet_semindex::SemanticSymbol) -> String {
    let mut out = String::new();
    if !symbol.summary.is_empty() {
        out.push_str(&symbol.summary);
        out.push_str("\n\n---\n\n");
    }
    out.push_str(&symbol.signature);
    for example in &symbol.examples {
        out.push_str("\n\nExample: `");
        out.push_str(example);
        out.push('`');
    }
    out
}

pub(crate) fn compute_hover(
    db: &SymbolDB,
    tokens: &[Token],
    _src: &str,
    path: &str,
    offset: usize,
) -> Option<String> {
    if let Some(symbol) = db.symbols.at(path, offset) {
        return Some(semantic_hover(symbol));
    }
    if let Some(reference) = db.refs.iter().find(|reference| {
        reference.module_path == path
            && reference.span.start <= offset
            && offset <= reference.span.end
    }) {
        if let Some(target) = &reference.target {
            if let Some(symbol) = db.symbols.symbols().iter().find(|symbol| {
                symbol.module_path == target.module_path
                    && symbol.span == Some(target.def_span)
            }) {
                return Some(semantic_hover(symbol));
            }
        }
    }
    let name = find_ident_at(tokens, offset)?;
    let symbols = db.symbols.lookup(name);
    if symbols.len() == 1 {
        return Some(semantic_hover(symbols[0]));
    }
    db.hover_at(path, offset).map(str::to_string)
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

/// Resolve a call into source owned by the canonical build graph. Generated
/// modules are not a second symbol database: their source and path come from
/// BuildPlan, and the lexer identifies the exact declaration span.
pub(crate) fn compute_generated_definition(
    plan: &crate::Comptime::Build::BuildPlan,
    tokens: &[Token],
    offset: usize,
) -> Option<(String, String, Span)> {
    let name = find_ident_at(tokens, offset)?;
    for module in plan.generated_modules() {
        let (generated_tokens, errors) = crate::Lexer::lex(&module.source);
        if !errors.is_empty() {
            continue;
        }
        for pair in generated_tokens.windows(2) {
            if matches!(pair[0].kind, TokKind::KwFn)
                && matches!(&pair[1].kind, TokKind::Ident(candidate) if candidate == name)
            {
                return Some((
                    module.path.as_str().to_string(),
                    module.source.clone(),
                    pair[1].span,
                ));
            }
        }
    }
    None
}

// ── Go-to-definition ──────────────────────────────────────────────────────────

pub(crate) fn compute_definition(
    db: &SymbolDB,
    tokens: &[Token],
    _src: &str,
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
    _path: &str,
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
    _path: &str,
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
    pub const OWNERSHIP: u32 = 12;
    pub const DECORATOR: u32 = 13;
}

// Modifier bitmasks
#[allow(dead_code)] // wired in c41 (semantic token highlighting)
mod sm {
    pub const DECLARATION: u32 = 1 << 0;
    pub const READONLY: u32 = 1 << 1;
    pub const MOVE: u32 = 1 << 2;
    pub const WRITE_BORROW: u32 = 1 << 3;
    pub const COPY: u32 = 1 << 4;
    pub const DIRECTIVE: u32 = 1 << 5;
    pub const CONTRACT: u32 = 1 << 6;
}

fn semantic_token_type_for(tokens: &[Token], idx: usize, src: &str) -> Option<(u32, u32)> {
    let tok = &tokens[idx];
    if let Some(marker_mod) = marker_modifier(tokens, idx) {
        return Some((st::DECORATOR, marker_mod));
    }

    match &tok.kind {
        TokKind::KwFn
        | TokKind::KwPub
        | TokKind::KwIf
        | TokKind::KwElse
        | TokKind::KwIn
        | TokKind::KwBreak
        | TokKind::KwContinue
        | TokKind::KwReturn
        | TokKind::KwStruct
        | TokKind::KwEnum
        | TokKind::KwImpl
        | TokKind::KwTrait
        | TokKind::KwTag
        | TokKind::KwDerive
        | TokKind::KwConst
        | TokKind::KwComptime
        | TokKind::KwUse
        | TokKind::KwExtern
        | TokKind::KwLoop
        | TokKind::KwUnsafe
        | TokKind::KwSelf
        | TokKind::KwNull
        | TokKind::KwOk
        | TokKind::KwErr
        | TokKind::KwIt
        | TokKind::KwModule => Some((st::KEYWORD, 0)),

        TokKind::KwTrue | TokKind::KwFalse => Some((st::KEYWORD, sm::READONLY)),

        TokKind::KwCopy => Some((st::OWNERSHIP, sm::COPY)),

        TokKind::KwWhile
        | TokKind::KwFor
        | TokKind::KwSwitch
        | TokKind::KwMutate
        | TokKind::KwMove
        | TokKind::KwView => None,

        TokKind::Ident(name) => {
            if is_live_teaching_semantic_word(name) {
                return None;
            }
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
        | TokKind::Pipe
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

        TokKind::Amp if token_text(src, tok) == crate::Syntax::SIGIL_WRITE => {
            Some((st::OWNERSHIP, sm::WRITE_BORROW))
        }

        TokKind::Caret if token_text(src, tok) == crate::Syntax::SIGIL_MOVE => {
            Some((st::OWNERSHIP, sm::MOVE))
        }

        _ => None,
    }
}

fn marker_modifier(tokens: &[Token], idx: usize) -> Option<u32> {
    let tok = &tokens[idx];
    match tok.kind {
        TokKind::Hash => marker_kind_after(tokens, idx).map(|kind| kind.modifier()),
        TokKind::At => marker_kind_after(tokens, idx).map(|kind| kind.modifier()),
        _ => {
            let prev = previous_significant(tokens, idx)?;
            match tokens[prev].kind {
                TokKind::Hash | TokKind::At if marker_name(tokens, idx).is_some() => {
                    marker_kind_for(&tokens[prev], tokens, idx).map(|kind| kind.modifier())
                }
                _ => None,
            }
        }
    }
}

#[derive(Clone, Copy)]
enum MarkerKind {
    Directive,
    Contract,
}

impl MarkerKind {
    fn modifier(self) -> u32 {
        match self {
            MarkerKind::Directive => sm::DIRECTIVE,
            MarkerKind::Contract => sm::CONTRACT,
        }
    }
}

fn marker_kind_after(tokens: &[Token], idx: usize) -> Option<MarkerKind> {
    let next = next_significant(tokens, idx)?;
    marker_kind_for(&tokens[idx], tokens, next)
}

fn marker_kind_for(prefix: &Token, tokens: &[Token], name_idx: usize) -> Option<MarkerKind> {
    let name = marker_name(tokens, name_idx)?;
    match prefix.kind {
        TokKind::Hash if crate::Syntax::DIRECTIVE_MARKERS.contains(&name) => {
            Some(MarkerKind::Directive)
        }
        TokKind::At if crate::Syntax::CONTRACT_MARKERS.contains(&name) => {
            Some(MarkerKind::Contract)
        }
        _ => None,
    }
}

fn marker_name(tokens: &[Token], idx: usize) -> Option<&str> {
    match &tokens[idx].kind {
        TokKind::Ident(name) => Some(name.as_str()),
        TokKind::KwUnsafe => Some(crate::Syntax::KW_UNSAFE),
        _ => None,
    }
}

fn previous_significant(tokens: &[Token], idx: usize) -> Option<usize> {
    tokens[..idx]
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, tok)| (!is_trivia(tok)).then_some(i))
}

fn next_significant(tokens: &[Token], idx: usize) -> Option<usize> {
    tokens
        .iter()
        .enumerate()
        .skip(idx + 1)
        .find_map(|(i, tok)| (!is_trivia(tok)).then_some(i))
}

fn is_trivia(tok: &Token) -> bool {
    matches!(tok.kind, TokKind::LineComment(_) | TokKind::BlockComment(_))
}

fn token_text<'a>(src: &'a str, tok: &Token) -> &'a str {
    src.get(tok.span.start..tok.span.end.min(src.len()))
        .unwrap_or("")
}

pub(crate) fn is_live_teaching_semantic_word(name: &str) -> bool {
    matches!(
        name,
        crate::Syntax::FOREIGN_PRIVATE
            | crate::Syntax::FOREIGN_UNSAFE
            | crate::Syntax::FOREIGN_NAMESPACE
            | crate::Syntax::FOREIGN_OWNED
            | crate::Syntax::FOREIGN_SANITIZER
            | crate::Syntax::FOREIGN_VEC
            | crate::Syntax::FOREIGN_DICT
            | crate::Syntax::FOREIGN_EPRINTLN
            | crate::Syntax::FOREIGN_OPEN
            | crate::Syntax::FOREIGN_GETENV
            | crate::Syntax::FOREIGN_OS
            | crate::Syntax::FOREIGN_ASYNC
            | crate::Syntax::FOREIGN_AWAIT
            | crate::Syntax::FOREIGN_MUTEX
            | crate::Syntax::FOREIGN_LOCK
    )
}

/// Encode semantic tokens for a token stream into the LSP delta-encoded u32 array.
pub(crate) fn encode_semantic_tokens(tokens: &[Token], src: &str) -> Vec<u32> {
    encode_semantic_tokens_where(tokens, src, |_| true)
}

/// Encode only tokens whose start byte lies inside `span`.
pub(crate) fn encode_semantic_tokens_in_span(tokens: &[Token], src: &str, span: Span) -> Vec<u32> {
    encode_semantic_tokens_where(tokens, src, |tok| {
        tok.span.start >= span.start && tok.span.start < span.end
    })
}

fn encode_semantic_tokens_where(
    tokens: &[Token],
    src: &str,
    include: impl Fn(&Token) -> bool,
) -> Vec<u32> {
    let mut data: Vec<u32> = Vec::new();
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for (idx, tok) in tokens.iter().enumerate() {
        if matches!(tok.kind, TokKind::Eof) {
            break;
        }
        if !include(tok) {
            continue;
        }
        let (tok_type, tok_mods) = match semantic_token_type_for(tokens, idx, src) {
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
