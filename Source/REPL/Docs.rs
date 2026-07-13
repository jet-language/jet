//! D-FE-REPL-DOCS1=B (ratified 2026-07-08, option B): `?name` inline docs.
//!
//! `?name` is a REPL-only query form — it is parsed here, in the REPL's own
//! input classifier, and never reaches the Jet lexer/parser as source (no new
//! user-typeable syntax; see `Source/REPL/mod.rs` where `?name` is peeled off
//! before `classify()` sees the rest of the line).
//!
//! It resolves a name exactly the way completion/LSP hover would: builtin
//! collection/string methods come from the same tables `jet-foundation`'s
//! `Collections` module (and hence sema/codegen) use; user-defined session
//! items (`fn`, `struct`, …) go through `jet_semindex::build_symbol_db` — the
//! same shared semantic index `Source/LSP/Features.rs::compute_hover` builds
//! from, so the REPL never forks a second docs text.

use super::{unique_temp_name, Session};

/// Builtin method names on `ty` (e.g. `"List"`) whose name starts with
/// `partial` — the same table `?Type.method` reads from, so `Interactive`'s
/// Tab-completion menu and `?name` docs never drift apart (I8).
pub(crate) fn method_candidates(ty: &str, partial: &str) -> Vec<String> {
    crate::SemanticSymbols::members(ty, partial)
        .map(|symbol| symbol.member.to_string())
        .collect()
}

fn builtin_lookup(name: &str) -> Option<String> {
    let symbol = crate::SemanticSymbols::lookup(name)?;
    Some(format!(
        "{}\n{}\nExample: {}\nSource: {} ({})\n",
        symbol.signature, symbol.summary, symbol.example, symbol.module, symbol.provenance
    ))
}

/// Collect `///` doc-comment lines immediately preceding `def_start` in the
/// raw token stream. Mirrors `Source/LSP/Features.rs::collect_doc_comment` —
/// duplicated rather than exported across the (private) `LSP` submodule
/// boundary; both read the same token shape from the same lexer.
fn collect_doc_comment(tokens: &[crate::Lexer::Token], def_start: usize) -> Option<String> {
    use crate::Lexer::TokKind;
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
                lines.push(text.trim_start_matches('/').trim().to_string());
            }
            _ => break,
        }
    }
    if lines.is_empty() {
        return None;
    }
    lines.reverse();
    Some(lines.join("\n"))
}

fn signature_for(name: &str, kind: &jet_semindex::SymKind) -> String {
    use jet_semindex::SymKind;
    match kind {
        SymKind::Function { params, ret } => {
            let ps: Vec<String> = params
                .iter()
                .map(|(n, t)| format!("{}: {}", n, t.name()))
                .collect();
            let r = ret.as_ref().map(|t| format!(" -> {}", t.name())).unwrap_or_default();
            format!("{}({}){}", name, ps.join(", "), r)
        }
        SymKind::Struct { fields } => format!(
            "struct {} {{ {} }}",
            name,
            fields
                .iter()
                .map(|(n, t)| format!("{}: {}", n, t.name()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        SymKind::Enum { variants } => format!("enum {} {{ {} }}", name, variants.join(", ")),
        SymKind::Trait => format!("trait {}", name),
        SymKind::Tag => format!("tag {}", name),
        SymKind::Const => format!("const {}", name),
        SymKind::Module => format!("module {}", name),
        SymKind::EnumVariant { parent } => format!("{}.{}", parent, name),
        SymKind::Field { ty, parent } => format!("{}.{}: {}", parent, name, ty.name()),
        SymKind::Local { mutable, ty } => {
            let sigil = if *mutable { ":=" } else { "::" };
            match ty {
                Some(t) => format!("{} {} <{}>", name, sigil, t.name()),
                None => format!("{} {} <value>", name, sigil),
            }
        }
        SymKind::Param { ty } => format!("{}: {}", name, ty.name()),
    }
}

/// Session-defined name (a `fn`/`struct`/`enum`/… the user typed this
/// session): materializes the accumulated items exactly like `:run`/`?name`
/// need, checks it through the same bundle path the LSP uses, and builds the
/// shared `jet_semindex` symbol index over it.
fn session_item_lookup(session: &Session, name: &str) -> Option<String> {
    let src = format!("{}{}\n", session.import_src(), session.accumulated_src());
    if src.trim().is_empty() {
        return None;
    }
    let tmp_path = std::env::temp_dir().join(unique_temp_name("docs"));
    if std::fs::write(&tmp_path, &src).is_err() {
        return None;
    }
    let path_str = tmp_path.to_string_lossy().to_string();
    let (_, bundle, facts) = crate::LSP::check_document_with_bundle(&path_str, &src);
    let result = (|| {
        let bundle = bundle?;
        let db = jet_semindex::build_symbol_db(&bundle, &facts);
        let def = db.defs.iter().find(|d| d.name == name)?;
        let (toks, _) = crate::Lexer::lex(&src);
        let doc = collect_doc_comment(&toks, def.def_span.start);
        let mut out = String::new();
        out.push_str(&signature_for(name, &def.kind));
        out.push('\n');
        if let Some(doc) = doc {
            out.push_str(&doc);
            out.push('\n');
        }
        out.push_str("Source: this session\n");
        Some(out)
    })();
    let _ = std::fs::remove_file(&tmp_path);
    result
}

/// Resolve `?name`. Lookup order: builtin collection/string methods (exact
/// `Type.method` match) → a live session binding (`name : Type = value`) →
/// a session-defined item (`fn`/`struct`/…) via the shared semantic index.
/// `None` means "nothing named `name` in this session" (same wording as
/// `:type`'s existing not-found note, so the two stay consistent).
pub fn lookup(session: &Session, name: &str) -> Option<String> {
    if name.contains('.') {
        if let Some(doc) = builtin_lookup(name) {
            return Some(doc);
        }
    }
    if let Some(v) = session.scope.get(name) {
        return Some(format!(
            "{}\n",
            super::Render::format_binding(name, v, session.mutable_names.contains(name))
        ));
    }
    session_item_lookup(session, name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_list_filter_matches_ratified_shape() {
        let doc = builtin_lookup("List.filter").expect("List.filter should have docs");
        assert!(doc.starts_with("List.filter(f: fn(T) -> Bool) -> List<T>\n"));
        assert!(doc.contains("Keeps items where f(item) is true."));
        assert!(doc.contains("Source: core.collections"));
    }

    #[test]
    fn unknown_builtin_is_none() {
        assert!(builtin_lookup("List.nonexistent").is_none());
    }

    #[test]
    fn session_binding_lookup_shows_live_value() {
        let mut session = Session::new();
        session
            .scope
            .insert("answer".to_string(), crate::AST::CtValue::Int(42));
        let doc = lookup(&session, "answer").expect("binding should resolve");
        assert_eq!(doc, "answer: Int :: 42\n");
    }
}
