//! LSP v2 (M13): full language server — completion, hover, go-to-definition,
//! references, rename, semantic tokens, inlay hints, quick-fixes, formatting.
//!
//! Hand-rolled JSON-RPC over stdio (invariant I6 — no serde in the compiler).
//! Panics inside handlers are caught (LSP-I2) — process death is a P0 bug.
//! All file reads go through the overlay (LSP-I4) — unsaved buffers are correct.

mod Check;
mod Completion;
mod Features;
mod Position;
mod Server;
mod SymbolDB;

// Public entry points (preserve `jet::LSP::<item>` paths).
pub use Check::{
    apply_all, apply_edit, build_graph_json, check_document, check_document_with_bundle,
    collect_fixes, run_bench, run_doctor, Fix,
};
pub use Position::{byte_offset_to_lsp, lsp_pos_to_offset, LspPos};
pub use Server::run_stdio;

// Glob re-exports so the inline `#[cfg(test)] mod tests`' `use super::*` can
// resolve `build_symbol_db`, `compute_completions`, `compute_hover`,
// `compute_rename`, `encode_semantic_tokens` (etc.) by name through the parent.
// Test-only: these surface `pub(crate)` items for the test glob; gating to
// `cfg(test)` keeps the public API crate-private and avoids dead re-export
// warnings in normal builds.
#[cfg(test)]
pub(crate) use Completion::*;
#[cfg(test)]
pub(crate) use Features::*;
#[cfg(test)]
pub(crate) use SymbolDB::*;

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_binding_keyword_has_no_teaching_edit() {
        let src = "fn run() {\n    let x = 1\n}\n";
        let diags = check_document("test.jet", src);
        assert!(
            !diags.iter().any(|d| d.code == "E0009" || d.code == "E0985"),
            "old binding words should not produce migration diagnostics: {diags:?}"
        );
        assert!(!diags.is_empty(), "old binding words should still fail");
    }

    #[test]
    fn lsp_pos_round_trip() {
        let src = "fn run() {\n    x :: 1\n}\n";
        let offset = 18; // somewhere in 'x ::'
        let pos = byte_offset_to_lsp(src, offset);
        let back = lsp_pos_to_offset(src, pos);
        assert_eq!(back, offset);
    }

    #[test]
    fn symbol_db_finds_function() {
        let src =
            "fn greet(name: String) {\n    print(name);\n}\nfn run() {\n    greet(\"world\");\n}\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        assert!(db.defs.iter().any(|d| d.name == "greet"));
        assert!(db.defs.iter().any(|d| d.name == "run"));
        assert!(db.refs.iter().any(|r| r.name == "greet"));
    }

    #[test]
    fn hover_returns_function_signature() {
        let src = "fn add(a: Int, b: Int) -> Int { return a + b; }\nfn run() { r :: add(1, 2) }\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let (toks, _) = crate::Lexer::lex(src);
        // Hover over 'add' at offset 3 (the name span)
        let hover = compute_hover(&db, &toks, src, "test.jet", 3);
        assert!(hover.is_some(), "expected hover for 'add'");
        let h = hover.unwrap();
        assert!(h.contains("add"), "hover should mention the function name");
    }

    #[test]
    fn rename_basic_function() {
        let src = "fn greet() {}\nfn run() { greet(); }\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let (toks, _) = crate::Lexer::lex(src);
        let spans = compute_rename(&db, &toks, "test.jet", 3, "hello").expect("rename ok");
        assert!(!spans.is_empty());
        assert!(spans.iter().any(|(_, sp)| sp.start <= 3 && 3 <= sp.end));
    }

    #[test]
    fn rename_rejects_keyword() {
        let src = "fn greet() {}\nfn run() { greet(); }\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let (toks, _) = crate::Lexer::lex(src);
        assert!(compute_rename(&db, &toks, "test.jet", 3, "fn").is_err());
    }

    #[test]
    fn rename_rejects_reserved_double_underscore_name() {
        let src = "fn greet() {}\nfn run() { greet(); }\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let db = build_symbol_db(&bundle.expect("bundle"), &facts);
        let (tokens, _) = crate::Lexer::lex(src);
        assert!(compute_rename(&db, &tokens, "test.jet", 3, "__generated").is_err());
    }

    #[test]
    fn semantic_tokens_non_empty() {
        let src = "fn run() { x: Int :: 1 }\n";
        let (toks, _) = crate::Lexer::lex(src);
        let data = encode_semantic_tokens(&toks, src);
        // Should emit at least one token (5 u32s per token)
        assert!(data.len() >= 5, "expected at least one semantic token");
    }

    #[test]
    fn inlay_hints_for_int_literal() {
        let src = "fn run() {\n    x :: 42\n    count := 0\n}\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let hints = db.inlay_hints_for("test.jet");
        assert!(
            hints.iter().any(|h| h.label.contains(": Int")),
            "expected : Int inlay hint for immutable binding"
        );
        assert!(
            hints.iter().any(|h| h.label.contains(": Int")),
            "expected : Int inlay hint for mutable binding"
        );
    }

    #[test]
    fn completion_includes_keywords() {
        let src = "fn run() {\n    \n}\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let items = compute_completions(&db, src, 14, "test.jet", None, None);
        // Keyword completions expose only Jet syntax.
        assert!(
            !items.iter().any(|i| i.label == "val"),
            "old binding words must not appear in completions"
        );
        // Real keywords must appear:
        assert!(
            items.iter().any(|i| i.label == "fn"),
            "expected fn in completions"
        );
        assert!(
            items.iter().any(|i| i.label == "use"),
            "expected use (KW_USE) in completions"
        );
    }

    #[test]
    fn completion_hides_soft_public_names_until_explicitly_requested() {
        let src = "pub fn _helper() {}\nfn run() {}\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let db = build_symbol_db(&bundle.expect("bundle"), &facts);
        assert!(db.symbols.lookup("_helper").len() == 1);
        assert!(!db
            .symbols
            .complete_visible_in("", None, Some("test.jet"))
            .iter()
            .any(|symbol| symbol.name == "_helper"));
        assert!(db
            .symbols
            .complete_visible_in("_", None, Some("test.jet"))
            .iter()
            .any(|symbol| symbol.name == "_helper"));
    }

    #[test]
    fn hover_and_completion_use_same_semantic_fact() {
        let src = "/// Adds two values.\n/// Example: add(1, 2)\nfn add(a: Int, b: Int) -> Int { return a + b }\nfn run() {\n    \n}\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let symbol = db
            .symbols
            .lookup_identity("fn:module:test.jet::add")
            .expect("add fact");
        let (tokens, _) = crate::Lexer::lex(src);
        let hover_offset = src.find("\nfn add").unwrap() + 4;
        let hover = compute_hover(&db, &tokens, src, "test.jet", hover_offset).expect("hover");
        let offset = src.rfind("    \n").unwrap() + 4;
        let completion = compute_completions(&db, src, offset, "test.jet", None, None)
            .into_iter()
            .find(|item| item.label == "add")
            .expect("add completion");
        assert!(hover.contains(&symbol.signature));
        assert!(hover.contains(&symbol.summary));
        assert_eq!(completion.detail.as_deref(), Some(symbol.signature.as_str()));
    }

    #[test]
    fn completion_uses_builtin_member_facts_for_list_local() {
        let src = "fn run() {\n    items :: [1, 2]\n    count :: items.len()\n}\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let offset = src.find("items.len").unwrap() + "items.l".len();
        let items = compute_completions(&db, src, offset, "test.jet", None, None);
        let len = db.symbols.lookup_qualified("List.len").unwrap();
        assert!(items.iter().any(|item| {
            item.label == "len" && item.detail.as_deref() == Some(len.signature.as_str())
        }));
    }
    #[test]
    fn completion_excludes_locals_from_other_functions() {
        let src = "fn first() {\n    hidden :: 1\n}\nfn second() {\n    \n}\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let db = build_symbol_db(&bundle.expect("bundle"), &facts);
        let offset = src.rfind("    \n").unwrap() + 4;
        assert!(!compute_completions(&db, src, offset, "test.jet", None, None)
            .iter()
            .any(|item| item.label == "hidden"));
    }

    #[test]
    fn completion_respects_then_else_slot_boundaries() {
        let src = "fn run(flag: Bool) {\n    if flag {\n        then_only :: 1\n        print(then_only)\n    } else {\n        else_only :: 2\n        print(else_only)\n    }\n}\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let db = build_symbol_db(&bundle.expect("bundle"), &facts);
        let then_start = src.find("if flag {").unwrap() + "if flag {".len();
        let then_end = src.find("    } else {").unwrap() + 4;
        let else_start = src.find("else {").unwrap() + "else {".len();
        let else_end = src.rfind("    }\n}").unwrap() + 4;
        assert!(db.slot_boundaries.iter().any(|boundary| {
            boundary.slot == "then_body"
                && boundary.span.start == then_start
                && boundary.span.end == then_end
        }));
        assert!(db.slot_boundaries.iter().any(|boundary| {
            boundary.slot == "else_body"
                && boundary.span.start == else_start
                && boundary.span.end == else_end
        }));
        let then_offset = src.find("print(then_only)").unwrap();
        let then_items = compute_completions(&db, src, then_offset, "test.jet", None, None);
        assert!(then_items.iter().any(|item| item.label == "then_only"));
        assert!(!then_items.iter().any(|item| item.label == "else_only"));
        let else_offset = src.find("print(else_only)").unwrap();
        let else_items = compute_completions(&db, src, else_offset, "test.jet", None, None);
        assert!(else_items.iter().any(|item| item.label == "else_only"));
        assert!(!else_items.iter().any(|item| item.label == "then_only"));
    }

    #[test]
    fn completion_keeps_then_local_out_of_empty_else() {
        let src = "fn run(flag: Bool) {\n    if flag {\n        then_only :: 1\n    } else {\n        \n    }\n}\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let db = build_symbol_db(&bundle.expect("bundle"), &facts);
        let offset = src.rfind("        \n").unwrap() + 8;
        assert!(!compute_completions(&db, src, offset, "test.jet", None, None)
            .iter()
            .any(|item| item.label == "then_only"));
    }

    #[test]
    fn completion_sees_parameter_inside_empty_body() {
        let src = "fn inspect(value: Int) {\n    \n}\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let db = build_symbol_db(&bundle.expect("bundle"), &facts);
        let offset = src.find("    \n").unwrap() + 4;
        let scope = db
            .symbols
            .symbols()
            .iter()
            .find(|symbol| symbol.name == "value")
            .unwrap()
            .lexical_scope
            .as_ref()
            .unwrap();
        assert_eq!(scope.span.start, src.find('{').unwrap() + 1);
        assert_eq!(scope.span.end, src.rfind('}').unwrap());
        assert!(compute_completions(&db, src, offset, "test.jet", None, None)
            .iter()
            .any(|item| item.label == "value"));
    }

    #[test]
    fn completion_sees_last_local_on_trailing_blank_line() {
        let src = "fn run() {\n    last_local :: 1\n    \n}\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let db = build_symbol_db(&bundle.expect("bundle"), &facts);
        let offset = src.rfind("    \n").unwrap() + 4;
        let scope = db
            .symbols
            .symbols()
            .iter()
            .find(|symbol| symbol.name == "last_local")
            .unwrap()
            .lexical_scope
            .as_ref()
            .unwrap();
        assert_eq!(scope.span.end, src.rfind('}').unwrap());
        assert!(compute_completions(&db, src, offset, "test.jet", None, None)
            .iter()
            .any(|item| item.label == "last_local"));
    }

    #[test]
    fn slot_boundaries_ignore_comment_and_interpolation_braces() {
        let src = "fn run(flag: Bool) {\n    text :: \"flag={flag}\"\n    // comment braces: { }\n    \n}\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let expected_start = src.find('{').unwrap() + 1;
        let expected_end = src.rfind('}').unwrap();
        assert_eq!(bundle.modules[0].block_spans, vec![crate::Diagnostics::Span::new(expected_start, expected_end)]);
        let db = build_symbol_db(&bundle, &facts);
        let offset = src.rfind("    \n").unwrap() + 4;
        assert!(compute_completions(&db, src, offset, "test.jet", None, None)
            .iter()
            .any(|item| item.label == "text"));
    }
}
