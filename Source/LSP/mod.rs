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
    collect_fixes, measure_bench, run_bench, run_doctor, BenchReport, Fix,
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
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_TEST_PROJECT: AtomicU64 = AtomicU64::new(0);

    /// Give unit overlays their own manifest boundary. A relative `test.jet`
    /// can discover and scan the full workspace.
    struct TestProject {
        root: std::path::PathBuf,
        entry: String,
    }

    impl TestProject {
        fn new() -> Self {
            let root = loop {
                let candidate = std::env::temp_dir().join(format!(
                    "jet-lsp-unit-{}-{}",
                    std::process::id(),
                    NEXT_TEST_PROJECT.fetch_add(1, Ordering::Relaxed)
                ));
                match std::fs::create_dir(&candidate) {
                    Ok(()) => break candidate,
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create isolated LSP test project: {error}"),
                }
            };
            let entry = root.join("test.jet").to_string_lossy().into_owned();
            let project = Self { root, entry };
            std::fs::write(
                project.root.join(crate::Syntax::PACKAGE_FILE),
                "name: \"lsp_unit\"\nversion: \"0.1.0\"\n",
            )
            .expect("write isolated LSP test manifest");
            project
        }

        fn entry(&self) -> &str {
            &self.entry
        }
    }

    impl Drop for TestProject {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn check_test_document(
        text: &str,
    ) -> (
        TestProject,
        Vec<crate::Diagnostics::Diagnostic>,
        Option<crate::AST::ProgramBundle>,
        jet_semindex::SemIndexEffectFacts,
    ) {
        let project = TestProject::new();
        let (diagnostics, bundle, facts) = check_document_with_bundle(project.entry(), text);
        (project, diagnostics, bundle, facts)
    }

    #[test]
    fn old_binding_keyword_has_no_teaching_edit() {
        let src = "fn run() {\n    let x = 1\n}\n";
        let (_, diags, _, _) = check_test_document(src);
        assert!(
            !diags.iter().any(|d| d.code == "E0009" || d.code == "E0985"),
            "old binding words should not produce migration diagnostics: {diags:?}"
        );
        assert!(!diags.is_empty(), "old binding words should still fail");
    }

    #[test]
    fn retired_interpolation_selector_has_one_shared_quick_fix() {
        let project = TestProject::new();
        let source = format!(
            "fn run() {{\n    value :: 1\n    print(\"{{value{}Debug}}\")\n}}\n",
            crate::Syntax::RETIRED_INTERPOLATION_SELECTOR_RAIL
        );
        let fixes = collect_fixes(project.entry(), &source);
        assert_eq!(
            fixes
                .iter()
                .filter(|fix| fix.title.contains("D-ONCE-HASH1"))
                .count(),
            1
        );
        assert_eq!(
            apply_all(&source, &fixes),
            format!(
                "fn run() {{\n    value :: 1\n    print(\"{{value{}Debug}}\")\n}}\n",
                crate::Syntax::INTERPOLATION_SELECTOR_RAIL
            )
        );
    }

    #[test]
    fn test_document_context_does_not_discover_the_workspace() {
        let (project, diagnostics, bundle, _) = check_test_document("fn run() {}\n");
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
        let bundle = bundle.expect("bundle");
        assert!(project.root.is_absolute());
        assert_eq!(
            crate::Loader::find_manifest_root(&project.root),
            Some(project.root.clone())
        );
        assert_eq!(bundle.project_root, project.root);
        assert_eq!(bundle.modules.len(), 1);
        assert_eq!(bundle.modules[0].display, project.entry());
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
        let (_project, _, bundle, facts) = check_test_document(src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        assert!(db.defs.iter().any(|d| d.name == "greet"));
        assert!(db.defs.iter().any(|d| d.name == "run"));
        assert!(db.refs.iter().any(|r| r.name == "greet"));
    }

    #[test]
    fn hover_returns_function_signature() {
        let src = "fn add(a: Int, b: Int) => Int { return a + b; }\nfn run() { r :: add(1, 2) }\n";
        let (project, _, bundle, facts) = check_test_document(src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let (toks, _) = crate::Lexer::lex(src);
        // Hover over 'add' at offset 3 (the name span)
        let hover = compute_hover(&db, &toks, src, project.entry(), 3);
        assert!(hover.is_some(), "expected hover for 'add'");
        let h = hover.unwrap();
        assert!(h.contains("add"), "hover should mention the function name");
    }

    #[test]
    fn hover_shows_callable_access_defaults_and_policies() {
        let src = "#Policy(trace(\"users.load\"))\nfn load(value: &Int, label: String = \"user\") => Int { return 1 }\nfn run() {}\n";
        let (project, diagnostics, bundle, facts) = check_test_document(src);
        assert!(diagnostics.iter().all(|diagnostic| diagnostic.severity != crate::Diagnostics::Severity::Error));
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let (toks, _) = crate::Lexer::lex(src);
        let offset = src.find("fn load").expect("load declaration") + 3;
        let hover = compute_hover(&db, &toks, src, project.entry(), offset).expect("load hover");
        assert!(hover.contains("&value: Int"), "{hover}");
        assert!(hover.contains("label: String = \"user\""), "{hover}");
        assert!(hover.contains("policies=[trace(\"users.load\")]"), "{hover}");
    }

    #[test]
    fn unique_type_uses_leaf_in_hover_and_json() {
        let src = "struct Point { x: Int }\nfn run() {}\n";
        let (project, diagnostics, bundle, facts) = check_test_document(src);
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.severity != crate::Diagnostics::Severity::Error),
            "unique type fixture should check: {diagnostics:#?}"
        );
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let point = db
            .symbols
            .symbols()
            .iter()
            .find(|symbol| {
                symbol.name == "Point"
                    && symbol.kind == jet_semindex::SemanticSymbolKind::Type
            })
            .expect("Point semantic symbol");
        assert_eq!(point.qualified_name, "Point");

        let (tokens, lex_diagnostics) = crate::Lexer::lex(src);
        assert!(lex_diagnostics.is_empty(), "{lex_diagnostics:#?}");
        let hover = compute_hover(
            &db,
            &tokens,
            src,
            project.entry(),
            src.find("Point").expect("Point declaration"),
        )
        .expect("Point hover");
        assert!(hover.contains("Point"), "{hover}");

        let json = db.index.to_json();
        assert!(
            json.contains("\"name\":\"Point\",\"leaf_name\":\"Point\""),
            "{json}"
        );
    }

    #[test]
    fn hover_shows_source_module_for_imported_root_call() {
        let project = TestProject::new();
        std::fs::write(
            project.root.join("library.jet"),
            "pub fn render(#Root value: Int) => Int { return value }\n",
        )
        .expect("write imported root-call library");
        let src = "use \"./library\" as one\nfn run() { value :: 1\n    value.render()\n}\n";
        let (diagnostics, bundle, facts) = check_document_with_bundle(project.entry(), src);
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.severity != crate::Diagnostics::Severity::Error),
            "imported root-call fixture should check: {diagnostics:#?}"
        );
        let bundle = bundle.expect("imported root-call bundle");
        let db = build_symbol_db(&bundle, &facts);
        let (tokens, lex_diagnostics) = crate::Lexer::lex(src);
        assert!(lex_diagnostics.is_empty(), "{lex_diagnostics:#?}");
        let hover = compute_hover(
            &db,
            &tokens,
            src,
            project.entry(),
            src.find("render").expect("root-call method name"),
        )
        .expect("imported root-call hover");
        assert!(hover.contains("from module `"), "{hover}");
        assert!(hover.contains("library.jet"), "{hover}");
    }

    #[test]
    fn compiler_layout_fact_lsp_surface_is_visible_and_fixed() {
        let src = "struct Packet { count: Int }\nderive T.LayoutFacts { info :: T.$layout }\nfn run() {}\n";
        let (project, diagnostics, bundle, facts) = check_test_document(src);
        assert!(
            diagnostics.iter().all(|diagnostic| diagnostic.severity != crate::Diagnostics::Severity::Error),
            "layout fact LSP fixture should check: {diagnostics:#?}"
        );
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let layout_offset = src.find("$layout").expect("layout fact") + 2;
        let completions = compute_completions(
            &db,
            src,
            layout_offset + 4,
            project.entry(),
            None,
            None,
        );
        assert!(
            completions.iter().any(|item| {
                item.label == "$layout"
                    && item.detail.as_deref() == Some("compiler fact: LayoutInfo")
            }),
            "compiler fact completion missing: {}",
            completions
                .iter()
                .map(|item| item.label.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );

        let (tokens, lex_diagnostics) = crate::Lexer::lex(src);
        assert!(lex_diagnostics.is_empty(), "{lex_diagnostics:#?}");
        let hover = compute_hover(&db, &tokens, src, project.entry(), layout_offset)
            .expect("layout fact hover");
        assert!(hover.contains("$layout"), "{hover}");

        // Definition lookup is driven by the receiver type. The compiler
        // accepts the focused fact on the derive type parameter `T`; use the
        // same resolver with a concrete receiver to prove navigation lands on
        // that type declaration.
        let navigation_src = "struct Packet { count: Int }\nfn run() { info :: Packet.$layout }\n";
        let (navigation_tokens, navigation_diagnostics) = crate::Lexer::lex(navigation_src);
        assert!(navigation_diagnostics.is_empty(), "{navigation_diagnostics:#?}");
        let navigation_offset = navigation_src.find("$layout").expect("layout fact") + 2;
        let (definition_path, definition_span) = compute_definition(
            &db,
            &navigation_tokens,
            navigation_src,
            project.entry(),
            navigation_offset,
        )
        .expect("layout fact definition");
        assert_eq!(definition_path, project.entry());
        assert_eq!(&src[definition_span.start..definition_span.end], "Packet");

        let rename_error = compute_rename(
            &db,
            &tokens,
            project.entry(),
            layout_offset,
            "Other",
        )
        .expect_err("compiler facts are not renameable");
        assert!(rename_error.contains("compiler-owned $layout"), "{rename_error}");
    }

    #[test]
    fn rename_basic_function() {
        let src = "fn greet() {}\nfn run() { greet(); }\n";
        let (project, _, bundle, facts) = check_test_document(src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let (toks, _) = crate::Lexer::lex(src);
        let spans = compute_rename(&db, &toks, project.entry(), 3, "hello").expect("rename ok");
        assert!(!spans.is_empty());
        assert!(spans.iter().any(|(_, sp)| sp.start <= 3 && 3 <= sp.end));
    }

    #[test]
    fn rename_rejects_keyword() {
        let src = "fn greet() {}\nfn run() { greet(); }\n";
        let (project, _, bundle, facts) = check_test_document(src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let (toks, _) = crate::Lexer::lex(src);
        assert!(compute_rename(&db, &toks, project.entry(), 3, "fn").is_err());
    }

    #[test]
    fn rename_rejects_reserved_double_underscore_name() {
        let src = "fn greet() {}\nfn run() { greet(); }\n";
        let (project, _, bundle, facts) = check_test_document(src);
        let db = build_symbol_db(&bundle.expect("bundle"), &facts);
        let (tokens, _) = crate::Lexer::lex(src);
        assert!(compute_rename(&db, &tokens, project.entry(), 3, "__generated").is_err());
    }

    #[test]
    fn rename_preserves_identifier_case_category() {
        let src = "fn greet() {}\nfn run() { greet(); }\n";
        let (project, _, bundle, facts) = check_test_document(src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let (toks, _) = crate::Lexer::lex(src);
        let err = compute_rename(&db, &toks, project.entry(), 3, "BadName").unwrap_err();
        assert!(err.contains("bad_name"), "{err}");
    }

    #[test]
    fn rename_uses_semantic_category_for_uncased_unicode_names() {
        let src = "fn 日本語() {}\nfn run() { 日本語(); }\n";
        let (project, _, bundle, facts) = check_test_document(src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let (tokens, _) = crate::Lexer::lex(src);
        assert!(compute_rename(&db, &tokens, project.entry(), 3, "new_name").is_ok());
        let err = compute_rename(&db, &tokens, project.entry(), 3, "BadName").unwrap_err();
        assert!(err.contains("bad_name"), "{err}");
    }

    #[test]
    fn rename_preserves_case_for_all_declaration_families() {
        let src = r#"UserId :: distinct Int
alias Count = Int
#UnitFamily(Length) { meter }
state Door { Open }
protocol Wire { client: Send(value: Int) }
module holder<T> { pub struct Box { value: T } }
module cache = holder<Int>
fn run() {}
"#;
        let (project, _, bundle, facts) = check_test_document(src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let (tokens, _) = crate::Lexer::lex(src);
        for name in ["UserId", "Count", "Length", "Door", "Open", "Wire", "Send"] {
            let err = compute_rename(
                &db,
                &tokens,
                project.entry(),
                src.find(name).unwrap(),
                "bad_name",
            )
            .unwrap_err();
            assert!(err.contains("BadName"), "{name}: {err}");
        }
        for name in ["meter", "holder", "cache"] {
            let err = compute_rename(
                &db,
                &tokens,
                project.entry(),
                src.find(name).unwrap(),
                "BadName",
            )
            .unwrap_err();
            assert!(err.contains("bad_name"), "{name}: {err}");
        }
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
        let (project, _, bundle, facts) = check_test_document(src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let hints = db.inlay_hints_for(project.entry());
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
    fn inlay_and_hover_for_inferred_struct_lit_from_place() {
        let src = "\
struct Point { x: Int y: Int }
fn run() {
    p := Point.{ x: 1, y: 2 }
    p = .{ x: 3, y: 4 }
}
";
        let (project, _, bundle, facts) = check_test_document(src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let hints = db.inlay_hints_for(project.entry());
        assert!(
            hints.iter().any(|h| h.label.contains(": Point")),
            "expected : Point inlay on inferred `.{{…}}` after place assign: {hints:?}"
        );
        assert!(
            db.hover.iter().any(|h| h.text.contains("`Point`")),
            "expected hover naming Point on inferred struct lit"
        );
    }

    #[test]
    fn completion_includes_keywords() {
        let src = "fn run() {\n    \n}\n";
        let (project, _, bundle, facts) = check_test_document(src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let items = compute_completions(&db, src, 14, project.entry(), None, None);
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
        let (project, _, bundle, facts) = check_test_document(src);
        let db = build_symbol_db(&bundle.expect("bundle"), &facts);
        assert!(db.symbols.lookup("_helper").len() == 1);
        assert!(!db
            .symbols
            .complete_visible_in("", None, Some(project.entry()))
            .iter()
            .any(|symbol| symbol.name == "_helper"));
        assert!(db
            .symbols
            .complete_visible_in("_", None, Some(project.entry()))
            .iter()
            .any(|symbol| symbol.name == "_helper"));
    }

    #[test]
    fn hover_and_completion_use_same_semantic_fact() {
        let src = "/// Adds two values.\n/// Example: add(1, 2)\nfn add(a: Int, b: Int) => Int { return a + b }\nfn run() {\n    \n}\n";
        let (project, _, bundle, facts) = check_test_document(src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let symbol = db
            .symbols
            .lookup_identity(&format!("fn:module:{}::add", project.entry()))
            .expect("add fact");
        let (tokens, _) = crate::Lexer::lex(src);
        let hover_offset = src.find("\nfn add").unwrap() + 4;
        let hover =
            compute_hover(&db, &tokens, src, project.entry(), hover_offset).expect("hover");
        let offset = src.rfind("    \n").unwrap() + 4;
        let completion = compute_completions(&db, src, offset, project.entry(), None, None)
            .into_iter()
            .find(|item| item.label == "add")
            .expect("add completion");
        assert!(hover.contains(&symbol.signature));
        assert!(hover.contains(&symbol.summary));
        assert_eq!(completion.detail.as_deref(), Some(symbol.signature.as_str()));
    }

    #[test]
    fn completion_exposes_public_labels_and_parameter_zones() {
        let src =
            "fn connect(host: String, /, *, timeout seconds: Int = 30) => String {\n    return host\n}\nfn run() {\n    \n}\n";
        let (project, _, bundle, facts) = check_test_document(src);
        let db = build_symbol_db(&bundle.expect("bundle"), &facts);
        let offset = src.rfind("    \n").unwrap() + 4;
        let item = compute_completions(&db, src, offset, project.entry(), None, None)
            .into_iter()
            .find(|item| item.label == "connect")
            .expect("connect completion");
        assert_eq!(
            item.detail.as_deref(),
            Some("fn connect(host: String, /, *, timeout seconds: Int) =[]=> String")
        );
    }

    #[test]
    fn completion_uses_builtin_member_facts_for_list_local() {
        let src = "fn run() {\n    items :: [1, 2]\n    count :: items.len()\n}\n";
        let (project, _, bundle, facts) = check_test_document(src);
        let bundle = bundle.expect("bundle");
        let db = build_symbol_db(&bundle, &facts);
        let offset = src.find("items.len").unwrap() + "items.l".len();
        let items = compute_completions(&db, src, offset, project.entry(), None, None);
        let len = db.symbols.lookup_qualified("List.len").unwrap();
        assert!(items.iter().any(|item| {
            item.label == "len" && item.detail.as_deref() == Some(len.signature.as_str())
        }));
    }

    #[test]
    fn completion_catalogs_numeric_destination_methods() {
        let src = "fn run() {\n    value :: F32.from_float(1.0) ?? F32.from_int(0)\n}\n";
        let (project, _, bundle, facts) = check_test_document(src);
        let db = build_symbol_db(&bundle.expect("bundle"), &facts);
        let offset = src.find("F32.from_f").unwrap() + "F32.from_f".len();
        let items = compute_completions(&db, src, offset, project.entry(), None, None);
        assert!(items.iter().any(|item| {
            item.label == "from_float"
                && item.detail.as_deref()
                    == Some("F32.from_float(value: Float) -> F32 ? String")
        }));
    }

    #[test]
    fn completion_catalogs_source_distinct_and_unit_members() {
        let src = r#"
#Numeric Token :: distinct Int(0..10)
#UnitFamily(Reward) { credit }

fn run() {
    token :: Token.from_u8(1)
    money :: Credit.from_int(1)
}
"#;
        let (project, diagnostics, bundle, facts) = check_test_document(src);
        let db = build_symbol_db(
            &bundle.unwrap_or_else(|| panic!("bundle diagnostics: {diagnostics:#?}")),
            &facts,
        );

        let token_site = src.find("Token.from_u8").unwrap();
        let token_items = compute_completions(
            &db,
            src,
            token_site + "Token.from_u".len(),
            project.entry(),
            None,
            None,
        );
        assert!(token_items.iter().any(|item| {
            item.label == "from_u8"
                && item.detail.as_deref()
                    == Some("Token.from_u8(value: U8) -> Token ? String")
        }));
        for (unit_site, _) in src.match_indices("Credit.from_int") {
            let unit_items = compute_completions(
                &db,
                src,
                unit_site + "Credit.from_i".len(),
                project.entry(),
                None,
                None,
            );
            assert_eq!(unit_items.iter().filter(|item| item.label == "from_int").count(), 1);
        }
    }

    #[test]
    fn completion_excludes_locals_from_other_functions() {
        let src = "fn first() {\n    hidden :: 1\n}\nfn second() {\n    \n}\n";
        let (project, _, bundle, facts) = check_test_document(src);
        let db = build_symbol_db(&bundle.expect("bundle"), &facts);
        let offset = src.rfind("    \n").unwrap() + 4;
        assert!(!compute_completions(&db, src, offset, project.entry(), None, None)
            .iter()
            .any(|item| item.label == "hidden"));
    }

    #[test]
    fn completion_respects_then_else_slot_boundaries() {
        let src = "fn run(flag: Bool) {\n    if flag {\n        then_only :: 1\n        print(then_only)\n    } else {\n        else_only :: 2\n        print(else_only)\n    }\n}\n";
        let (project, _, bundle, facts) = check_test_document(src);
        let db = build_symbol_db(&bundle.expect("bundle"), &facts);
        let then_start = src.find("if flag {").unwrap() + "if flag {".len();
        let then_end = src.find("    } else {").unwrap() + 4;
        let else_start = src.find("else {").unwrap() + "else {".len();
        let else_end = src.rfind("    }\n}").unwrap() + 4;
        assert!(db.slot_boundaries.iter().any(|boundary| {
            boundary.slot == "body"
                && boundary.span.start == then_start
                && boundary.span.end == then_end
        }));
        assert!(db.slot_boundaries.iter().any(|boundary| {
            boundary.slot == "else_body"
                && boundary.span.start == else_start
                && boundary.span.end == else_end
        }));
        let then_offset = src.find("print(then_only)").unwrap();
        let then_items =
            compute_completions(&db, src, then_offset, project.entry(), None, None);
        assert!(then_items.iter().any(|item| item.label == "then_only"));
        assert!(!then_items.iter().any(|item| item.label == "else_only"));
        let else_offset = src.find("print(else_only)").unwrap();
        let else_items =
            compute_completions(&db, src, else_offset, project.entry(), None, None);
        assert!(else_items.iter().any(|item| item.label == "else_only"));
        assert!(!else_items.iter().any(|item| item.label == "then_only"));
    }

    #[test]
    fn completion_keeps_then_local_out_of_empty_else() {
        let src = "fn run(flag: Bool) {\n    if flag {\n        then_only :: 1\n    } else {\n        \n    }\n}\n";
        let (project, _, bundle, facts) = check_test_document(src);
        let db = build_symbol_db(&bundle.expect("bundle"), &facts);
        let offset = src.rfind("        \n").unwrap() + 8;
        assert!(!compute_completions(&db, src, offset, project.entry(), None, None)
            .iter()
            .any(|item| item.label == "then_only"));
    }

    #[test]
    fn completion_sees_parameter_inside_empty_body() {
        let src = "fn inspect(value: Int) {\n    \n}\n";
        let (project, _, bundle, facts) = check_test_document(src);
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
        assert!(compute_completions(&db, src, offset, project.entry(), None, None)
            .iter()
            .any(|item| item.label == "value"));
    }

    #[test]
    fn completion_sees_last_local_on_trailing_blank_line() {
        let src = "fn run() {\n    last_local :: 1\n    \n}\n";
        let (project, _, bundle, facts) = check_test_document(src);
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
        assert!(compute_completions(&db, src, offset, project.entry(), None, None)
            .iter()
            .any(|item| item.label == "last_local"));
    }

    #[test]
    fn slot_boundaries_ignore_comment_and_interpolation_braces() {
        let src = "fn run(flag: Bool) {\n    text :: \"flag={flag}\"\n    // comment braces: { }\n    \n}\n";
        let (project, _, bundle, facts) = check_test_document(src);
        let bundle = bundle.expect("bundle");
        let expected_start = src.find('{').unwrap() + 1;
        let expected_end = src.rfind('}').unwrap();
        assert_eq!(bundle.modules[0].block_spans, vec![crate::Diagnostics::Span::new(expected_start, expected_end)]);
        let db = build_symbol_db(&bundle, &facts);
        let offset = src.rfind("    \n").unwrap() + 4;
        assert!(compute_completions(&db, src, offset, project.entry(), None, None)
            .iter()
            .any(|item| item.label == "text"));
    }
}
