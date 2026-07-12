//! D-FE-REPL-DOCS1=B: `?name` docs over shared semantic symbol facts.
//!
//! `?name` remains REPL-only input classification. Identity, signatures,
//! summaries, examples, provenance, and completion names come exclusively
//! from `jet_semindex::SemanticSymbolIndex`.

use super::{unique_temp_name, Session};

pub(crate) fn symbol_index(session: &Session) -> jet_semindex::SemanticSymbolIndex {
    let mut index = jet_semindex::SemanticSymbolIndex::language();
    let src = format!("{}{}\n", session.import_src(), session.accumulated_src());
    if !src.trim().is_empty() {
        let tmp_path = std::env::temp_dir().join(unique_temp_name("docs"));
        if std::fs::write(&tmp_path, &src).is_ok() {
            let path_str = tmp_path.to_string_lossy().to_string();
            let (_, bundle, facts) = crate::LSP::check_document_with_bundle(&path_str, &src);
            if let Some(bundle) = bundle {
                let db = jet_semindex::build_symbol_db(&bundle, &facts);
                index.extend(db.symbols.symbols().iter().cloned().map(|mut symbol| {
                    if matches!(symbol.provenance, jet_semindex::SemanticProvenance::Source { .. }) {
                        symbol.provenance = jet_semindex::SemanticProvenance::Session;
                    }
                    symbol
                }));
            }
            let _ = std::fs::remove_file(&tmp_path);
        }
    }
    for (name, value) in &session.scope {
        index.push(jet_semindex::SemanticSymbol {
            identity: format!("session:binding:{name}"),
            name: name.clone(),
            qualified_name: name.clone(),
            owner: None,
            module_path: "this session".to_string(),
            kind: jet_semindex::SemanticSymbolKind::Local,
            signature: super::Render::format_binding(
                name,
                value,
                session.mutable_names.contains(name),
            ),
            summary: String::new(),
            examples: Vec::new(),
            provenance: jet_semindex::SemanticProvenance::Session,
            span: None,
            lexical_scope: None,
        });
    }
    index
}

pub(crate) fn completion_candidates(
    session: &Session,
    prefix: &str,
    owner: Option<&str>,
) -> Vec<jet_semindex::SemanticSymbol> {
    symbol_index(session)
        .complete_visible_at(
            prefix,
            owner,
            jet_semindex::SemanticVisibilityAnchor {
                module_path: "this session",
                offset: None,
                session_top_level: true,
            },
        )
        .into_iter()
        .filter(|symbol| {
            owner.is_some()
                || !matches!(
                    symbol.kind,
                    jet_semindex::SemanticSymbolKind::Keyword
                        | jet_semindex::SemanticSymbolKind::Parameter
                )
        })
        .cloned()
        .collect()
}

fn render(symbol: &jet_semindex::SemanticSymbol) -> String {
    let mut out = format!("{}\n", symbol.signature);
    if !symbol.summary.is_empty() {
        out.push_str(&symbol.summary);
        out.push('\n');
    }
    for example in &symbol.examples {
        out.push_str("Example: ");
        out.push_str(example);
        out.push('\n');
    }
    let source = match &symbol.provenance {
        jet_semindex::SemanticProvenance::Source { module_path } => module_path.as_str(),
        jet_semindex::SemanticProvenance::Builtin { module } => module.as_str(),
        jet_semindex::SemanticProvenance::CommandRegistry => "command registry",
        jet_semindex::SemanticProvenance::Session => "this session",
    };
    out.push_str(&format!("Source: {source}\n"));
    out
}

pub fn lookup(session: &Session, name: &str) -> Option<String> {
    let index = symbol_index(session);
    index
        .resolve_visible_at(
            name,
            jet_semindex::SemanticVisibilityAnchor {
                module_path: "this session",
                offset: None,
                session_top_level: true,
            },
        )
        .map(render)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_list_filter_uses_shared_fact() {
        let session = Session::new();
        let doc = lookup(&session, "List.filter").expect("List.filter docs");
        assert!(doc.starts_with("List.filter(f: fn(T) -> Bool) -> List<T>\n"));
        assert!(doc.contains("Keeps items where f(item) is true."));
        assert!(doc.contains("Source: core.collections"));
    }

    #[test]
    fn session_binding_lookup_shows_live_value() {
        let mut session = Session::new();
        session
            .scope
            .insert("answer".to_string(), crate::AST::CtValue::Int(42));
        let doc = lookup(&session, "answer").expect("binding should resolve");
        assert!(doc.starts_with("answer: Int :: 42\n"));
        assert!(doc.contains("Source: this session"));
    }

    #[test]
    fn completion_dedups_shadowed_session_item() {
        let mut session = Session::new();
        session
            .item_srcs
            .push("fn answer() -> Int { return 1 }".to_string());
        session
            .scope
            .insert("answer".to_string(), crate::AST::CtValue::Int(42));
        let candidates = completion_candidates(&session, "ans", None);
        assert_eq!(candidates.len(), 1, "{candidates:?}");
        assert_eq!(candidates[0].identity, "session:binding:answer");
    }

    #[test]
    fn completion_excludes_accumulated_function_locals() {
        let mut session = Session::new();
        session
            .item_srcs
            .push("fn f() { hidden :: 1 }".to_string());
        assert!(completion_candidates(&session, "hid", None).is_empty());
    }
}
