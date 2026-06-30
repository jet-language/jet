//! LSP v2 (M13): full language server — completion, hover, go-to-definition,
//! references, rename, semantic tokens, inlay hints, quick-fixes, formatting.
//!
//! Hand-rolled JSON-RPC over stdio (invariant I6 — no serde in the compiler).
//! Panics inside handlers are caught (LSP-I2) — process death is a P0 bug.
//! All file reads go through the overlay (LSP-I4) — unsaved buffers are correct.

mod Check;
mod Completion;
mod Features;
mod JSON;
mod Position;
mod Server;
mod SymbolDB;

// Public entry points (preserve `jet::LSP::<item>` paths).
pub use Check::{
    apply_all, apply_edit, check_document, check_document_with_bundle, collect_fixes, run_bench,
    run_doctor, Fix,
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
    fn teaching_edit_from_let() {
        // D-BIND1: `let x = 1` migrates to `x #= 1`, which moves tokens — it is
        // no longer a single-keyword swap, so no auto-edit is synthesized (the
        // `replace `X` with `Y`` shape). `jet fmt` performs the migration. The
        // teaching diagnostic still fires and points at the sigil form.
        let src = "fn main() {\n    let x = 1\n}\n";
        let diags = check_document("test.jet", src);
        let e0009 = diags.iter().find(|d| d.code == "E0009").expect("E0009");
        assert!(
            e0009.edit.is_none(),
            "E0009 fix moves tokens; no trivial edit"
        );
        assert!(e0009.fix.contains(crate::Syntax::SIGIL_BIND_IMMUT));
    }

    #[test]
    fn lsp_pos_round_trip() {
        let src = "fn main() {\n    val x = 1;\n}\n";
        let offset = 18; // somewhere in 'val'
        let pos = byte_offset_to_lsp(src, offset);
        let back = lsp_pos_to_offset(src, pos);
        assert_eq!(back, offset);
    }

    #[test]
    fn symbol_db_finds_function() {
        let src = "fn greet(name: String) {\n    print(name);\n}\nfn main() {\n    greet(\"world\");\n}\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        assert!(db.defs.iter().any(|d| d.name == "greet"));
        assert!(db.defs.iter().any(|d| d.name == "main"));
        assert!(db.refs.iter().any(|r| r.name == "greet"));
    }

    #[test]
    fn hover_returns_function_signature() {
        let src =
            "fn add(a: Int, b: Int) -> Int { return a + b; }\nfn main() { val r = add(1, 2); }\n";
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
        let src = "fn greet() {}\nfn main() { greet(); }\n";
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
        let src = "fn greet() {}\nfn main() { greet(); }\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let (toks, _) = crate::Lexer::lex(src);
        assert!(compute_rename(&db, &toks, "test.jet", 3, "fn").is_err());
    }

    #[test]
    fn semantic_tokens_non_empty() {
        let src = "fn main() { val x: Int = 1; }\n";
        let (toks, _) = crate::Lexer::lex(src);
        let data = encode_semantic_tokens(&toks, src);
        // Should emit at least one token (5 u32s per token)
        assert!(data.len() >= 5, "expected at least one semantic token");
    }

    #[test]
    fn inlay_hints_for_int_literal() {
        let src = "fn main() {\n    x #= 42\n    count := 0\n}\n";
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
        let src = "fn main() {\n    \n}\n";
        let (_, bundle, facts) = check_document_with_bundle("test.jet", src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let items = compute_completions(&db, src, 14, "test.jet");
        // `val` and `var` are retired (FOREIGN_VAL/FOREIGN_VAR); they must not appear.
        assert!(
            !items.iter().any(|i| i.label == "val"),
            "val is retired (D-BIND1); must not appear in completions"
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
}
