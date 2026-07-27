//! LSP language features: hover, go-to-definition, references, rename,
//! refactors, semantic tokens, inlay hints.

use crate::Diagnostics::{Diagnostic, Span, TextEdit};
use crate::Lexer::{TokKind, Token};
use crate::Syntax;

use super::Completion::{use_statement_for_module, JET_KEYWORDS, JET_TYPES};
use super::Position::byte_offset_to_lsp;
use super::SymbolDB::{InlayHint, SymKind, SymbolDB};
use jet_foundation::JSON::json_escape;

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
    if let Some(symbol) = db.symbols.resolve_visible_in(name, Some(path)) {
        return Some(semantic_hover(symbol));
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
    _tokens: &[Token],
    path: &str,
    offset: usize,
    include_declaration: bool,
) -> Vec<(String, Span)> {
    let anchor_identity = |anchor: &jet_semindex::DefinitionAnchor| {
        anchor.semantic_identity.clone().or_else(|| db.defs.iter().find(|definition| {
            definition.module_path == anchor.module_path
                && definition.def_span.start == anchor.def_span.start
                && definition.def_span.end == anchor.def_span.end
        }).map(|definition| definition.identity.clone()))
    };
    let identity = db.index.instances().iter().flat_map(|instance| &instance.applications)
        .find(|application| application.module_path == path
            && application.span.start <= offset && offset <= application.span.end)
        .map(|application| application.semantic_identity.clone())
        .or_else(|| db.defs.iter().find(|definition| definition.module_path == path
            && definition.def_span.start <= offset && offset <= definition.def_span.end)
            .map(|definition| definition.identity.clone()))
        .or_else(|| db.refs.iter().find(|reference| reference.module_path == path
            && reference.span.start <= offset && offset <= reference.span.end)
            .and_then(|reference| reference.target.as_ref())
            .and_then(anchor_identity));
    let Some(identity) = identity else { return Vec::new() };
    let mut result: Vec<(String, Span)> = db
        .refs
        .iter()
        .filter(|reference| reference.target.as_ref()
            .and_then(anchor_identity)
            .is_some_and(|candidate| candidate == identity))
        .map(|r| (r.module_path.clone(), r.span))
        .collect();
    if include_declaration {
        for def in db.defs.iter().filter(|definition| definition.identity == identity) {
            result.push((def.module_path.clone(), def.def_span));
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.start.cmp(&b.1.start)).then(a.1.end.cmp(&b.1.end)));
    result.dedup();
    result
}

#[cfg(test)]
mod generic_instance_tests {
    use super::compute_references;

    #[test]
    fn references_join_applicative_generic_module_aliases() {
        let root = std::env::temp_dir().join(format!("jet_lsp_genmod_identity_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("main.jet");
        let source = "module value<n: Int> { pub fn get() => Int { return n } }\nmodule a = value<3>\nmodule b = value<3>\nfn run() { print(a.get()); print(b.get()) }\n";
        std::fs::write(&path, source).unwrap();
        let shown = path.to_string_lossy().into_owned();
        let mut bundle = crate::Loader::load_entry(&shown).unwrap();
        let (diagnostics, facts) = crate::Sema::check_bundle_with_effect_facts(&mut bundle, crate::Sema::CompileMode::Check);
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.severity != crate::Diagnostics::Severity::Error), "{diagnostics:#?}");
        let db = jet_semindex::build_symbol_db(&bundle, &facts);
        let (tokens, lex_diagnostics) = crate::Lexer::lex(source);
        assert!(lex_diagnostics.is_empty());
        let references = compute_references(&db, &tokens, &shown, source.find("a.get").unwrap(), true);
        let spellings: Vec<_> = references.iter().map(|(_, span)| &source[span.start..span.end]).collect();
        assert!(spellings.iter().any(|spelling| *spelling == "a"), "{spellings:?}");
        assert!(spellings.iter().any(|spelling| *spelling == "b"), "{spellings:?}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn references_never_join_same_alias_spelling_across_distinct_instances() {
        let root = std::env::temp_dir().join(format!("jet_lsp_genmod_hostile_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let main = root.join("main.jet");
        let left = root.join("left.jet");
        let right = root.join("right.jet");
        let left_source = "module value<n: Int> { pub fn get() => Int { return n } }\nmodule same = value<3>\nfn left_value() => Int { return same.get() }\n";
        let right_source = "module value<n: Int> { pub fn get() => Int { return n } }\nmodule same = value<4>\nfn right_value() => Int { return same.get() }\n";
        std::fs::write(&main, "module left\nmodule right\nfn run() {}\n").unwrap();
        std::fs::write(&left, left_source).unwrap();
        std::fs::write(&right, right_source).unwrap();
        let shown_main = main.to_string_lossy().into_owned();
        let shown_left = "left.jet".to_string();
        let shown_right = "right.jet".to_string();
        let mut bundle = crate::Loader::load_entry(&shown_main).unwrap();
        let (diagnostics, facts) = crate::Sema::check_bundle_with_effect_facts(&mut bundle, crate::Sema::CompileMode::Check);
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.severity != crate::Diagnostics::Severity::Error), "{diagnostics:#?}");
        let db = jet_semindex::build_symbol_db(&bundle, &facts);
        let (tokens, lex_diagnostics) = crate::Lexer::lex(left_source);
        assert!(lex_diagnostics.is_empty());
        let offset = left_source.find("same =").unwrap();
        let references = compute_references(&db, &tokens, &shown_left, offset, true);
        assert!(references.iter().any(|(path, span)| path == &shown_left && &left_source[span.start..span.end] == "same"), "refs={references:?} defs={:#?} instances={:#?}", db.defs, db.index.instances());
        assert!(!references.iter().any(|(path, _)| path == &shown_right), "distinct value<4> instance joined by alias spelling: {references:?}");
        let _ = std::fs::remove_dir_all(root);
    }
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
    if crate::Syntax::classify_identifier(new_name) == crate::Syntax::IdentifierClass::Reserved {
        return Err(format!("`{new_name}` is reserved for Jet and cannot be used as a name"));
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
    let indexed_case = db.defs.iter().find(|d| d.name == name).map(|def| {
        match &def.kind {
            SymKind::Struct { .. } | SymKind::Enum { .. } | SymKind::Trait | SymKind::Tag | SymKind::Type
            | SymKind::EnumVariant { .. } => ("type-like name", Syntax::NameCase::Pascal),
            SymKind::Module | SymKind::Function { .. } | SymKind::Const | SymKind::Field { .. }
            | SymKind::Local { .. } | SymKind::Param { .. } =>
                ("value-like name", Syntax::NameCase::Snake),
        }
    });
    // Some declaration families are expanded before SemIndex sees the checked
    // bundle (protocols, derives, state/unit sugar). Their already-validated
    // source spelling still determines the strict two-tier category exactly.
    let source_case = match (
        Syntax::name_has_case(name, Syntax::NameCase::Pascal),
        Syntax::name_has_case(name, Syntax::NameCase::Snake),
    ) {
        (true, false) => Some(("type-like name", Syntax::NameCase::Pascal)),
        (false, true) => Some(("value-like name", Syntax::NameCase::Snake)),
        _ => None,
    };
    if let Some((category, case)) = source_case.or(indexed_case) {
        if !Syntax::name_has_case(new_name, case) {
            return Err(format!(
                "`{new_name}` is not a valid {category}; use `{}`",
                Syntax::canonical_name_case(new_name, case)
            ));
        }
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
        spans.extend(tokens.iter().filter_map(|token| match &token.kind {
            TokKind::Ident(candidate) if candidate == name => Some((path.to_string(), token.span)),
            _ => None,
        }));
    }
    if spans.is_empty() {
        return Err(format!("no occurrences of `{}` found", name));
    }
    Ok(spans)
}

// ── Code actions ──────────────────────────────────────────────────────────────

pub(crate) struct RefactorAction {
    pub title: String,
    pub kind: &'static str,
    pub edits: Vec<TextEdit>,
}

pub(crate) fn compute_refactor_actions(
    db: &SymbolDB,
    tokens: &[Token],
    diagnostics: &[Diagnostic],
    src: &str,
    path: &str,
    workspace_root: Option<&str>,
    import_sources: &std::collections::HashMap<String, String>,
    excluded_import_paths: &std::collections::HashSet<String>,
    requested: Span,
) -> Vec<RefactorAction> {
    let mut actions = import_actions(
        db,
        diagnostics,
        src,
        path,
        workspace_root,
        import_sources,
        excluded_import_paths,
        requested,
    );
    let Some(selected) = trim_span(src, requested) else {
        actions.extend(inline_actions(db, tokens, src, path, requested));
        return actions;
    };
    let mut extracted = false;
    if let Some(selected) = extractable_expr(db, src, path, selected)
        .filter(|span| !is_trivial_extract(tokens, *span))
        .filter(|span| is_total_pure_expr(db, tokens, path, *span))
    {
        if let Some(insert_at) = extract_insert_point(db, src, path, selected) {
            if let Some(expression) = src.get(selected.start..selected.end) {
                let indent_len = src[insert_at..]
                    .chars()
                    .take_while(|c| *c == ' ' || *c == '\t')
                    .map(char::len_utf8)
                    .sum::<usize>();
                let indent = &src[insert_at..insert_at + indent_len];
                let name = fresh_name(db, "extracted_value");
                actions.push(RefactorAction {
                    title: "Extract binding".to_string(),
                    kind: "refactor.extract",
                    edits: vec![
                        TextEdit {
                            span: Span::new(insert_at, insert_at),
                            new_text: format!("{indent}{name} :: {expression}\n"),
                        },
                        TextEdit {
                            span: selected,
                            new_text: name,
                        },
                    ],
                });
                if let Some(action) =
                    extract_function_action(db, tokens, src, path, selected, expression)
                {
                    actions.push(action);
                }
                extracted = true;
            }
        }
    }
    if !extracted {
        actions.extend(inline_actions(db, tokens, src, path, selected));
    }
    actions
}

fn import_actions(
    db: &SymbolDB,
    diagnostics: &[Diagnostic],
    src: &str,
    path: &str,
    workspace_root: Option<&str>,
    import_sources: &std::collections::HashMap<String, String>,
    excluded_import_paths: &std::collections::HashSet<String>,
    requested: Span,
) -> Vec<RefactorAction> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for diagnostic in diagnostics
        .iter()
        .filter(|diagnostic| matches!(diagnostic.code.as_str(), "E0102" | "E0107"))
    {
        let Some(span) = diagnostic.span.filter(|span| spans_touch(*span, requested)) else {
            continue;
        };
        let Some(name) = src.get(span.start..span.end) else {
            continue;
        };
        let mut modules = db
            .symbols
            .lookup(name)
            .into_iter()
            .filter(|symbol| {
                symbol.module_path != path
                    && !excluded_import_paths.contains(&symbol.module_path)
                    && matches!(
                        symbol.provenance,
                        jet_semindex::SemanticProvenance::Source { .. }
                    )
                    && source_symbol_is_exported(symbol, import_sources)
                    && !matches!(
                        &symbol.kind,
                        jet_semindex::SemanticSymbolKind::Local
                            | jet_semindex::SemanticSymbolKind::Parameter
                            | jet_semindex::SemanticSymbolKind::Member
                    )
            })
            .filter_map(|symbol| {
                use_statement_for_module(path, workspace_root, &symbol.module_path)
            })
            .collect::<Vec<_>>();
        modules.sort();
        modules.dedup();
        let [statement] = modules.as_slice() else {
            continue;
        };
        if src.lines().any(|line| line.trim() == statement.trim()) || !seen.insert(statement.clone())
        {
            continue;
        }
        let module = statement
            .trim()
            .strip_prefix("use ")
            .unwrap_or(statement.trim());
        out.push(RefactorAction {
            title: format!("Import `{module}`"),
            kind: "quickfix",
            edits: vec![TextEdit {
                span: Span::new(0, 0),
                new_text: statement.clone(),
            }],
        });
    }
    out
}

fn extract_function_action(
    db: &SymbolDB,
    tokens: &[Token],
    src: &str,
    path: &str,
    selected: Span,
    expression: &str,
) -> Option<RefactorAction> {
    if !is_total_pure_expr(db, tokens, path, selected) {
        return None;
    }
    let return_type = infer_total_pure_return_type(db, tokens, path, selected)?;
    if !is_scalar_name(&return_type) {
        return None;
    }
    let function = db
        .index
        .definition_facts()
        .iter()
        .filter(|definition| {
            definition.module_path == path
                && definition.kind == "function"
                && definition.span.start <= selected.start
                && selected.end <= definition.span.end
        })
        .min_by_key(|definition| definition.span.end - definition.span.start)?;

    let mut inputs: Vec<(&jet_semindex::SymDef, usize)> = Vec::new();
    for reference in db.refs.iter().filter(|reference| {
        reference.module_path == path
            && selected.start <= reference.span.start
            && reference.span.end <= selected.end
    }) {
        let Some(target) = reference.target.as_ref() else {
            return None;
        };
        let Some(definition) = definition_for_anchor(db, target) else {
            return None;
        };
        if selected.start <= definition.def_span.start && definition.def_span.end <= selected.end {
            continue;
        }
        match &definition.kind {
            SymKind::Param { ty } if is_scalar_type(ty) => {}
            SymKind::Local {
                mutable: false,
                ty: Some(ty),
            } if is_scalar_type(ty) => {}
            SymKind::Local { .. } | SymKind::Param { .. } => return None,
            _ => continue,
        }
        if let Some((_, first)) = inputs
            .iter_mut()
            .find(|(existing, _)| existing.identity == definition.identity)
        {
            *first = (*first).min(reference.span.start);
        } else {
            inputs.push((definition, reference.span.start));
        }
    }
    inputs.sort_by_key(|(_, first)| *first);
    let function_name = fresh_name(db, "extracted_fn");
    let params = inputs
        .iter()
        .map(|(definition, _)| {
            let ty = match &definition.kind {
                SymKind::Param { ty } | SymKind::Local { ty: Some(ty), .. } => ty,
                _ => unreachable!(),
            };
            format!("{}: {}", definition.name, ty.name())
        })
        .collect::<Vec<_>>()
        .join(", ");
    let args = inputs
        .iter()
        .map(|(definition, _)| definition.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let insert = line_start(src, function.span.start);
    Some(RefactorAction {
        title: "Extract function".to_string(),
        kind: "refactor.extract",
        edits: vec![
            TextEdit {
                span: Span::new(insert, insert),
                new_text: format!(
                    "fn {function_name}({params}) => {return_type} {{ return {expression} }}\n\n"
                ),
            },
            TextEdit {
                span: selected,
                new_text: format!("{function_name}({args})"),
            },
        ],
    })
}

fn inline_actions(
    db: &SymbolDB,
    tokens: &[Token],
    src: &str,
    path: &str,
    requested: Span,
) -> Vec<RefactorAction> {
    let Some(reference) = db.refs.iter().find(|reference| {
        reference.module_path == path && spans_touch(reference.span, requested)
    }) else {
        return Vec::new();
    };
    let Some(target) = reference.target.as_ref() else {
        return Vec::new();
    };
    let Some(binding) = definition_for_anchor(db, target) else {
        return Vec::new();
    };
    if !matches!(
        binding.kind,
        SymKind::Local {
            mutable: false,
            ..
        }
    ) {
        return Vec::new();
    }
    let uses: Vec<_> = db
        .refs
        .iter()
        .filter(|candidate| {
            candidate.module_path == path
                && candidate
                    .target
                    .as_ref()
                    .is_some_and(|anchor| same_anchor(anchor, target))
        })
        .collect();
    if uses.is_empty() {
        return Vec::new();
    }
    let Some(initializer) = initializer_for_binding(db, src, path, binding) else {
        return Vec::new();
    };
    let initializer_span = Span::new(initializer.span.start, initializer.span.end);
    if !is_total_pure_expr(db, tokens, path, initializer_span)
        || !initializer_refs_are_stable(db, path, initializer_span)
    {
        return Vec::new();
    }
    let start = line_start(src, binding.def_span.start);
    let end = line_end_including_newline(src, initializer_span.end);
    let prefix = &src[start..binding.def_span.start];
    let suffix = src
        .get(initializer_span.end..end)
        .unwrap_or("")
        .trim_end_matches(['\r', '\n'])
        .trim();
    if !prefix.chars().all(char::is_whitespace)
        || !suffix.trim_end_matches(';').trim().is_empty()
        || uses.iter().any(|use_site| end > use_site.span.start)
    {
        return Vec::new();
    }
    let Some(expression) = src.get(initializer_span.start..initializer_span.end) else {
        return Vec::new();
    };
    let mut edits: Vec<TextEdit> = uses
        .iter()
        .map(|use_site| TextEdit {
            span: use_site.span,
            new_text: format!("({expression})"),
        })
        .collect();
    edits.push(TextEdit {
        span: Span::new(start, end),
        new_text: String::new(),
    });
    vec![RefactorAction {
        title: format!("Inline `{}`", binding.name),
        kind: "refactor.inline",
        edits,
    }]
}

fn initializer_refs_are_stable(db: &SymbolDB, path: &str, span: Span) -> bool {
    db.refs
        .iter()
        .filter(|reference| {
            reference.module_path == path
                && span.start <= reference.span.start
                && reference.span.end <= span.end
        })
        .all(|reference| {
            reference
                .target
                .as_ref()
                .and_then(|target| definition_for_anchor(db, target))
                .is_some_and(|definition| {
                    !matches!(
                        &definition.kind,
                        SymKind::Local { mutable: true, .. }
                    )
                })
        })
}

fn source_symbol_is_exported(
    symbol: &jet_semindex::SemanticSymbol,
    sources: &std::collections::HashMap<String, String>,
) -> bool {
    let Some(span) = symbol.span else {
        return false;
    };
    let Some(source) = sources.get(&symbol.module_path) else {
        return false;
    };
    let start = line_start(source, span.start);
    source
        .get(start..span.start)
        .is_some_and(|prefix| {
            prefix
                .split_whitespace()
                .any(|word| word == "pub" || word.starts_with("pub("))
        })
}

fn exact_expr<'a>(
    db: &'a SymbolDB,
    path: &str,
    selected: Span,
) -> Option<&'a jet_semindex::StructuralNode> {
    db.nodes.iter().find(|node| {
        node.module_path == path
            && node.class == "expr"
            && node.span.start == selected.start
            && node.span.end == selected.end
    })
}

/// Exact expr node, or a single outer `(…)` group around one.
fn extractable_expr(
    db: &SymbolDB,
    src: &str,
    path: &str,
    selected: Span,
) -> Option<Span> {
    if exact_expr(db, path, selected).is_some() {
        return Some(selected);
    }
    let text = src.get(selected.start..selected.end)?;
    let start_ws = text.len() - text.trim_start().len();
    let end_ws = text.len() - text.trim_end().len();
    let core = text.trim();
    if core.len() < 2 || !core.starts_with('(') || !core.ends_with(')') {
        return None;
    }
    let inner = Span::new(
        selected.start + start_ws + 1,
        selected.end - end_ws - 1,
    );
    let inner = trim_span(src, inner)?;
    exact_expr(db, path, inner).map(|_| selected)
}

fn extract_insert_point(db: &SymbolDB, src: &str, path: &str, selected: Span) -> Option<usize> {
    if let Some(initializer) = db.nodes.iter().find(|node| {
        node.module_path == path
            && node.class == "expr"
            && node.slot == "initializer"
            && node.span.start == selected.start
            && node.span.end == selected.end
    }) {
        if let Some(binding) = binding_for_initializer(db, src, path, initializer) {
            return Some(line_start(src, binding.def_span.start));
        }
    }
    // Call expr-stmts currently record a narrow callee-name stmt span, so
    // enclosure checks against stmt nodes miss argument subexpressions.
    // Insert on the line that holds the selection.
    Some(line_start(src, selected.start))
}

fn binding_for_initializer<'a>(
    db: &'a SymbolDB,
    src: &str,
    path: &str,
    initializer: &jet_semindex::StructuralNode,
) -> Option<&'a jet_semindex::SymDef> {
    let start = line_start(src, initializer.span.start);
    db.defs
        .iter()
        .filter(|definition| {
            definition.module_path == path
                && matches!(&definition.kind, SymKind::Local { .. })
                && start <= definition.def_span.start
                && definition.def_span.end <= initializer.span.start
        })
        .max_by_key(|definition| definition.def_span.start)
}

fn initializer_for_binding<'a>(
    db: &'a SymbolDB,
    src: &str,
    path: &str,
    binding: &jet_semindex::SymDef,
) -> Option<&'a jet_semindex::StructuralNode> {
    let end = src[binding.def_span.start..]
        .find(['\r', '\n'])
        .map_or(src.len(), |offset| binding.def_span.start + offset);
    db.nodes
        .iter()
        .filter(|node| {
            node.module_path == path
                && node.class == "expr"
                && node.slot == "initializer"
                && binding.def_span.end <= node.span.start
                && node.span.end <= end
        })
        .min_by_key(|node| node.span.start)
}

fn definition_for_anchor<'a>(
    db: &'a SymbolDB,
    anchor: &jet_semindex::DefinitionAnchor,
) -> Option<&'a jet_semindex::SymDef> {
    db.defs.iter().find(|definition| {
        anchor
            .semantic_identity
            .as_ref()
            .is_some_and(|identity| identity == &definition.identity)
            || (definition.module_path == anchor.module_path
                && definition.def_span.start == anchor.def_span.start
                && definition.def_span.end == anchor.def_span.end)
    })
}

fn same_anchor(
    left: &jet_semindex::DefinitionAnchor,
    right: &jet_semindex::DefinitionAnchor,
) -> bool {
    match (&left.semantic_identity, &right.semantic_identity) {
        (Some(left), Some(right)) => left == right,
        _ => {
            left.module_path == right.module_path
                && left.def_span.start == right.def_span.start
                && left.def_span.end == right.def_span.end
        }
    }
}

fn is_scalar_type(ty: &crate::AST::Type) -> bool {
    is_scalar_name(&ty.name())
}

fn is_scalar_name(name: &str) -> bool {
    matches!(name, "Bool" | "Char" | "Int" | "Float")
}

fn is_trivial_extract(tokens: &[Token], span: Span) -> bool {
    let significant: Vec<_> = tokens
        .iter()
        .filter(|token| {
            span.start <= token.span.start
                && token.span.end <= span.end
                && token.span.start < token.span.end
                && !matches!(
                    token.kind,
                    TokKind::Eof | TokKind::LParen | TokKind::RParen
                )
        })
        .collect();
    matches!(
        significant.as_slice(),
        [token]
            if matches!(
                token.kind,
                TokKind::Ident(_)
                    | TokKind::Int(_, _)
                    | TokKind::Float(_)
                    | TokKind::Char(_)
                    | TokKind::KwTrue
                    | TokKind::KwFalse
                    | TokKind::Str(_)
            )
    )
}

fn is_total_pure_expr(db: &SymbolDB, tokens: &[Token], path: &str, span: Span) -> bool {
    let enclosed: Vec<&Token> = tokens
        .iter()
        .filter(|token| {
            span.start <= token.span.start
                && token.span.end <= span.end
                && token.span.start < token.span.end
                && !matches!(token.kind, TokKind::Eof)
        })
        .collect();
    if enclosed.is_empty() {
        return false;
    }
    // `name()` / `name(args)` are effect-unknown calls even when parens are otherwise
    // allowed for grouping. Reject any Ident immediately followed by `(`.
    for pair in enclosed.windows(2) {
        if matches!(pair[0].kind, TokKind::Ident(_)) && matches!(pair[1].kind, TokKind::LParen) {
            return false;
        }
    }
    let mut has_comparison = false;
    for token in &enclosed {
        let safe = match &token.kind {
            TokKind::Ident(_)
            | TokKind::Int(_, _)
            | TokKind::Float(_)
            | TokKind::Char(_)
            | TokKind::KwTrue
            | TokKind::KwFalse
            | TokKind::AndAnd
            | TokKind::OrOr
            | TokKind::Bang
            | TokKind::LParen
            | TokKind::RParen => true,
            TokKind::EqEq | TokKind::NotEq => {
                has_comparison = true;
                true
            }
            TokKind::Str(parts) => parts
                .iter()
                .all(|part| matches!(part, crate::Lexer::StrTokPart::Lit(_))),
            _ => false,
        };
        if !safe {
            return false;
        }
    }
    if has_comparison {
        return expr_comparison_operands_are_scalar(db, tokens, path, span);
    }
    true
}

/// `==`/`!=` are total for matching scalars. Bool ops (`&&`/`||`/`!`) may only
/// combine Bool leaves. Pure scalar comparisons (no bool ops) allow any one
/// matching scalar type: Bool, Char, Int, or Float.
fn expr_comparison_operands_are_scalar(
    db: &SymbolDB,
    tokens: &[Token],
    path: &str,
    span: Span,
) -> bool {
    let enclosed: Vec<&Token> = tokens
        .iter()
        .filter(|token| {
            span.start <= token.span.start
                && token.span.end <= span.end
                && token.span.start < token.span.end
        })
        .collect();
    let has_bool_op = enclosed.iter().any(|token| {
        matches!(
            token.kind,
            TokKind::AndAnd | TokKind::OrOr | TokKind::Bang
        )
    });
    let mut leaf: Option<String> = None;
    for token in &enclosed {
        match &token.kind {
            TokKind::KwTrue | TokKind::KwFalse => {
                if !agree_scalar_leaf(&mut leaf, "Bool") {
                    return false;
                }
            }
            TokKind::Int(_, _) => {
                if has_bool_op || !agree_scalar_leaf(&mut leaf, "Int") {
                    return false;
                }
            }
            TokKind::Float(_) => {
                if has_bool_op || !agree_scalar_leaf(&mut leaf, "Float") {
                    return false;
                }
            }
            TokKind::Char(_) => {
                if has_bool_op || !agree_scalar_leaf(&mut leaf, "Char") {
                    return false;
                }
            }
            TokKind::Ident(_) => {
                let Some(name) = resolved_type_name(db, path, token.span) else {
                    return false;
                };
                if !is_scalar_name(&name) {
                    return false;
                }
                if has_bool_op && name != "Bool" {
                    return false;
                }
                if !agree_scalar_leaf(&mut leaf, &name) {
                    return false;
                }
            }
            TokKind::AndAnd
            | TokKind::OrOr
            | TokKind::Bang
            | TokKind::EqEq
            | TokKind::NotEq
            | TokKind::LParen
            | TokKind::RParen => {}
            _ => return false,
        }
    }
    leaf.as_deref().is_some_and(is_scalar_name)
}

fn agree_scalar_leaf(leaf: &mut Option<String>, name: &str) -> bool {
    match leaf {
        Some(existing) => existing == name,
        None => {
            *leaf = Some(name.to_string());
            true
        }
    }
}

fn resolved_type_name(db: &SymbolDB, path: &str, span: Span) -> Option<String> {
    let reference = db.refs.iter().find(|reference| {
        reference.module_path == path
            && reference.span.start == span.start
            && reference.span.end == span.end
    })?;
    let target = reference.target.as_ref()?;
    let definition = definition_for_anchor(db, target)?;
    match &definition.kind {
        SymKind::Param { ty } => Some(ty.name()),
        SymKind::Local { ty: Some(ty), .. } => Some(ty.name()),
        _ => None,
    }
}

fn infer_total_pure_return_type(
    db: &SymbolDB,
    tokens: &[Token],
    path: &str,
    span: Span,
) -> Option<String> {
    let mut saw_bool_op = false;
    let mut saw_comparison = false;
    let mut literal: Option<&str> = None;
    let mut from_ident: Option<String> = None;
    for token in tokens.iter().filter(|token| {
        span.start <= token.span.start
            && token.span.end <= span.end
            && token.span.start < token.span.end
            && !matches!(token.kind, TokKind::Eof)
    }) {
        match &token.kind {
            TokKind::AndAnd | TokKind::OrOr | TokKind::Bang | TokKind::KwTrue | TokKind::KwFalse => {
                saw_bool_op = true;
            }
            TokKind::EqEq | TokKind::NotEq => saw_comparison = true,
            TokKind::Int(_, _) => {
                if literal.is_some_and(|existing| existing != "Int") {
                    return None;
                }
                literal = Some("Int");
            }
            TokKind::Float(_) => {
                if literal.is_some_and(|existing| existing != "Float") {
                    return None;
                }
                literal = Some("Float");
            }
            TokKind::Char(_) => {
                if literal.is_some_and(|existing| existing != "Char") {
                    return None;
                }
                literal = Some("Char");
            }
            TokKind::Ident(_) => {
                let name = resolved_type_name(db, path, token.span)?;
                if !is_scalar_name(&name) {
                    return None;
                }
                match &from_ident {
                    Some(existing) if existing != &name => {
                        if !saw_bool_op && !saw_comparison {
                            return None;
                        }
                    }
                    None => from_ident = Some(name),
                    _ => {}
                }
            }
            TokKind::LParen | TokKind::RParen => {}
            TokKind::Str(_) => return None,
            _ => return None,
        }
    }
    if saw_bool_op || saw_comparison {
        return Some("Bool".to_string());
    }
    if let Some(name) = literal {
        return Some(name.to_string());
    }
    from_ident.filter(|name| is_scalar_name(name))
}

#[cfg(test)]
mod refactor_safety_tests {
    use super::{extract_function_action, infer_total_pure_return_type, is_total_pure_expr};
    use crate::Diagnostics::{Severity, Span};

    #[test]
    fn code_actions_reject_non_scalar_return_at_type_gate() {
        let root = std::env::temp_dir().join(format!(
            "jet-lsp-string-result-gate-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("main.jet");
        let source = "fn run() {\n    print(\"result\")\n}\n";
        std::fs::write(&path, source).unwrap();
        let shown = path.to_string_lossy().into_owned();
        let mut bundle = crate::Loader::load_entry(&shown).unwrap();
        let (diagnostics, facts) = crate::Sema::check_bundle_with_effect_facts(
            &mut bundle,
            crate::Sema::CompileMode::Check,
        );
        assert!(
            diagnostics
                .iter()
                .all(|diagnostic| diagnostic.severity != Severity::Error),
            "{diagnostics:#?}"
        );
        let db = jet_semindex::build_symbol_db(&bundle, &facts);
        let (tokens, lex_diagnostics) = crate::Lexer::lex(source);
        assert!(lex_diagnostics.is_empty(), "{lex_diagnostics:#?}");
        let start = source.find("\"result\"").unwrap();
        let selected = Span::new(start, start + "\"result\"".len());

        assert!(is_total_pure_expr(&db, &tokens, &shown, selected));
        assert_eq!(
            infer_total_pure_return_type(&db, &tokens, &shown, selected),
            None
        );
        assert!(
            extract_function_action(&db, &tokens, source, &shown, selected, "\"result\"")
                .is_none()
        );
        let _ = std::fs::remove_dir_all(root);
    }
}

fn trim_span(src: &str, span: Span) -> Option<Span> {
    let text = src.get(span.start..span.end)?;
    let start = text.len() - text.trim_start().len();
    let end = text.trim_end().len();
    (start < end).then(|| Span::new(span.start + start, span.start + end))
}

fn spans_touch(left: Span, right: Span) -> bool {
    left.start <= right.end && right.start <= left.end
}

fn line_start(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())]
        .rfind('\n')
        .map_or(0, |index| index + 1)
}

fn line_end_including_newline(src: &str, offset: usize) -> usize {
    src[offset.min(src.len())..]
        .find('\n')
        .map_or(src.len(), |index| offset.min(src.len()) + index + 1)
}

fn fresh_name(db: &SymbolDB, base: &str) -> String {
    if !db.defs.iter().any(|definition| definition.name == base) {
        return base.to_string();
    }
    for suffix in 2.. {
        let candidate = format!("{base}_{suffix}");
        if !db
            .defs
            .iter()
            .any(|definition| definition.name == candidate)
        {
            return candidate;
        }
    }
    unreachable!()
}

// ── Semantic tokens ───────────────────────────────────────────────────────────
//
// Token type indices (must match the legend in initialize_response).
mod st {
    pub const KEYWORD: u32 = 0;
    pub const TYPE: u32 = 1;
    pub const VARIABLE: u32 = 3;
    pub const STRING: u32 = 7;
    pub const NUMBER: u32 = 8;
    pub const COMMENT: u32 = 9;
    pub const OPERATOR: u32 = 10;
    pub const OWNERSHIP: u32 = 12;
    pub const DECORATOR: u32 = 13;
}

// Modifier bitmasks
mod sm {
    pub const READONLY: u32 = 1 << 1;
    pub const MOVE: u32 = 1 << 2;
    pub const WRITE_BORROW: u32 = 1 << 3;
    pub const COPY: u32 = 1 << 4;
    pub const RULE: u32 = 1 << 5;
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
        | TokKind::KwBreak
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
        | TokKind::KwIt
        | TokKind::KwModule => Some((st::KEYWORD, 0)),

        TokKind::KwTrue | TokKind::KwFalse => Some((st::KEYWORD, sm::READONLY)),

        TokKind::KwCopy => Some((st::OWNERSHIP, sm::COPY)),

        TokKind::KwWhile
        | TokKind::KwFor
        | TokKind::KwSwitch
        | TokKind::KwMutate
        | TokKind::KwMove => None,

        TokKind::Ident(name) => {
            if name == Syntax::KW_NEXT && is_contextual_next(tokens, idx) {
                return Some((st::KEYWORD, 0));
            }
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

        TokKind::Int(..) | TokKind::Float(_) | TokKind::Char(_) => Some((st::NUMBER, 0)),

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
        | TokKind::DotDotLt => Some((st::OPERATOR, 0)),

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
        _ => {
            let prev = previous_significant(tokens, idx)?;
            match tokens[prev].kind {
                TokKind::Hash if marker_name(tokens, idx).is_some() => {
                    marker_kind_for(&tokens[prev], tokens, idx).map(|kind| kind.modifier())
                }
                _ => None,
            }
        }
    }
}

#[derive(Clone, Copy)]
enum MarkerKind {
    Rule,
}

impl MarkerKind {
    fn modifier(self) -> u32 {
        match self {
            MarkerKind::Rule => sm::RULE,
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
        TokKind::Hash if crate::Syntax::is_applied_rule(name) => {
            Some(MarkerKind::Rule)
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

fn is_contextual_next(tokens: &[Token], idx: usize) -> bool {
    let previous = previous_significant(tokens, idx).map(|idx| &tokens[idx].kind);
    let next_idx = next_significant(tokens, idx);
    let next = next_idx.map(|idx| &tokens[idx].kind);
    if matches!(previous, Some(TokKind::QuestionQuestion)) {
        return true;
    }
    if !matches!(previous, Some(TokKind::LBrace | TokKind::Semi)) {
        return false;
    }
    matches!(next, Some(TokKind::Semi | TokKind::RBrace))
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
        crate::Syntax::METHOD_VIEW
            | crate::Syntax::FOREIGN_PRIVATE
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
