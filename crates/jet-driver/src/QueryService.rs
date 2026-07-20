//! Canonical incremental front-end queries shared by compiler clients.

use jet_queries::{FileKey, InputKey, QueryEngine, QueryKey};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct CheckedQuery {
    pub diagnostics: Arc<Vec<crate::Diagnostics::Diagnostic>>,
    pub bundle: Option<Arc<crate::AST::ProgramBundle>>,
    pub effect_facts: Arc<crate::Sema::SemIndexEffectFacts>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CompilerQueryStats {
    pub hits: u64,
    pub recomputes: u64,
    pub live_inputs: usize,
    pub live_input_bytes: usize,
    pub live_memos: usize,
    pub live_query_counters: usize,
    pub item_hits: u64,
    pub item_recomputes: u64,
    pub live_items: usize,
    pub live_item_bytes: usize,
}

#[derive(Default)]
pub struct CompilerQueries {
    engine: QueryEngine,
    sema: crate::Sema::IncrementalSemaCache,
}

impl CompilerQueries {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn check_text(&mut self, path: &str, text: &str, is_lsp: bool) -> CheckedQuery {
        let file = FileKey::new(path);
        self.set_file_input(file.clone(), text);
        let path = path.to_string();
        let abs = canonical_path(Path::new(&path));
        let engine = &mut self.engine;
        let sema = &mut self.sema;
        engine.query(
            QueryKey::for_file(if is_lsp { "checked.lsp" } else { "checked" }, file.clone()),
            |queries| {
                let text = queries
                    .input_text(&InputKey::file(file))
                    .unwrap_or_default();
                let (diagnostics, bundle, facts) =
                    crate::Driver::check_file_with_effect_facts_incremental(
                        &path,
                        Some((&abs, &text)),
                        is_lsp,
                        sema,
                    );
                CheckedQuery {
                    diagnostics: Arc::new(diagnostics),
                    bundle: bundle.map(Arc::new),
                    effect_facts: Arc::new(facts),
                }
            },
        )
    }

    pub fn check_disk(&mut self, path: &str, is_lsp: bool) -> CheckedQuery {
        // Imported modules are loaded from disk below this query boundary. A
        // disk check must therefore revalidate its full module closure.
        self.invalidate_checked();
        match std::fs::read_to_string(path) {
            Ok(text) => self.check_text(path, &text, is_lsp),
            Err(_) => {
                let (diagnostics, bundle, facts) =
                    crate::Driver::check_file_with_effect_facts(path, None, is_lsp);
                CheckedQuery {
                    diagnostics: Arc::new(diagnostics),
                    bundle: bundle.map(Arc::new),
                    effect_facts: Arc::new(facts),
                }
            }
        }
    }

    pub fn lex_text(&mut self, path: &str, text: &str) -> Arc<Vec<crate::Lexer::Token>> {
        let file = FileKey::new(path);
        self.set_file_input(file.clone(), text);
        self.engine
            .query(QueryKey::for_file("tokens", file.clone()), |queries| {
                let text = queries
                    .input_text(&InputKey::file(file))
                    .unwrap_or_default();
                Arc::new(crate::Lexer::lex(&text).0)
            })
    }

    pub fn remove_document(&mut self, path: &str) {
        if self
            .engine
            .remove_input(&InputKey::file(FileKey::new(path)))
        {
            self.invalidate_checked();
            self.sema.clear();
        }
    }

    pub fn stats(&self) -> CompilerQueryStats {
        let query = self.engine.stats();
        let item = self.sema.stats();
        CompilerQueryStats {
            hits: query.hits,
            recomputes: query.recomputes,
            live_inputs: query.live_inputs,
            live_input_bytes: query.live_input_bytes,
            live_memos: query.live_memos,
            live_query_counters: query.live_query_counters,
            item_hits: item.hits,
            item_recomputes: item.recomputes,
            live_items: item.live_items,
            live_item_bytes: item.live_item_bytes,
        }
    }

    pub fn recompute_count(&self, key: &QueryKey) -> u64 {
        self.engine.recompute_count(key)
    }

    fn set_file_input(&mut self, file: FileKey, text: &str) {
        if self
            .engine
            .set_input(InputKey::file(file), text.to_string())
        {
            // The loader may read this file while checking any root. Until
            // item-level loader dependencies land, invalidate all checked roots
            // conservatively; token queries remain precisely file-local.
            self.invalidate_checked();
        }
    }

    fn invalidate_checked(&mut self) {
        self.engine.invalidate_kind("checked");
        self.engine.invalidate_kind("checked.lsp");
    }
}

fn canonical_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diagnostic_summary(
        diagnostics: &[crate::Diagnostics::Diagnostic],
    ) -> Vec<(String, String, String, String, Option<crate::Diagnostics::Span>)> {
        diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.code.clone(),
                    diagnostic.what.clone(),
                    diagnostic.why.clone(),
                    diagnostic.fix.clone(),
                    diagnostic.span,
                )
            })
            .collect()
    }

    #[test]
    fn check_and_lsp_share_one_authoritative_query() {
        let mut service = CompilerQueries::new();
        let source = "fn beta() -> Int { return 1 }\nfn alpha() -> String { return beta() }\n";

        let check = service.check_text("shared.jet", source, true);
        let lsp = service.check_text("shared.jet", source, true);
        assert_eq!(
            diagnostic_summary(&check.diagnostics),
            diagnostic_summary(&lsp.diagnostics)
        );
        assert!(!check.diagnostics.is_empty());
        assert_eq!(
            service.recompute_count(&QueryKey::for_file(
                "checked.lsp",
                FileKey::new("shared.jet")
            )),
            1
        );
        assert_eq!(service.stats().hits, 1);
    }

    #[test]
    fn changed_import_invalidates_cached_importer() {
        let root = std::env::temp_dir().join(format!(
            "jet-query-import-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let main = root.join("main.jet");
        let dependency = root.join("b.jet");
        let main_source = "module b;\nfn run() -> Int { return b.value() }\n";
        let first_dependency = "pub fn value() -> Int { return 1 }\n";
        let second_dependency = "pub fn value() -> String { return \"changed\" }\n";
        std::fs::write(&main, main_source).unwrap();
        std::fs::write(&dependency, first_dependency).unwrap();

        let mut service = CompilerQueries::new();
        assert!(service
            .check_disk(&main.to_string_lossy(), true)
            .diagnostics
            .is_empty());
        std::fs::write(&dependency, second_dependency).unwrap();
        let changed = service.check_disk(&main.to_string_lossy(), true);
        assert!(
            !changed.diagnostics.is_empty(),
            "changed dependency must not leave importer diagnostics cached"
        );
        assert_eq!(
            service.recompute_count(&QueryKey::for_file(
                "checked.lsp",
                FileKey::new(main.to_string_lossy())
            )),
            2
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn local_body_edit_rechecks_only_changed_item() {
        let mut service = CompilerQueries::new();
        let before = "fn alpha() -> Int { return 1 }\nfn beta() -> Int { return 2 }\n";
        let after = "fn alpha() -> Int { return 1 }\nfn beta() -> Int { return 3 }\n";

        assert!(service.check_text("items.jet", before, true).diagnostics.is_empty());
        let cold = service.stats();
        assert_eq!(cold.item_hits, 0);
        assert_eq!(cold.item_recomputes, 2);
        assert_eq!(cold.live_items, 2);

        assert!(service.check_text("items.jet", after, true).diagnostics.is_empty());
        let warm = service.stats();
        assert_eq!(warm.item_hits, 1, "unchanged alpha must reuse checked body");
        assert_eq!(warm.item_recomputes, 3, "only changed beta may recheck");
        assert_eq!(warm.live_items, 2);
        assert!(warm.live_item_bytes > before.len());
    }

    #[test]
    fn cached_caller_observes_changed_callee_effects() {
        let before = "fn alpha() --[]-> Int { return beta() }\nfn beta() -> Int { return 2 }\n";
        let after = "fn alpha() --[]-> Int { return beta() }\nfn beta() -> Int { print(\"x\"); return 2 }\n";
        let mut incremental = CompilerQueries::new();
        assert!(incremental
            .check_text("effects.jet", before, true)
            .diagnostics
            .is_empty());

        let changed = incremental.check_text("effects.jet", after, true);
        let fresh = CompilerQueries::new().check_text("effects.jet", after, true);
        assert_eq!(
            diagnostic_summary(&changed.diagnostics),
            diagnostic_summary(&fresh.diagnostics),
            "incremental diagnostics must be byte-for-byte equivalent to a fresh check"
        );
        assert!(changed.diagnostics.iter().any(|diagnostic| diagnostic.code == "E3401"));
        let stats = incremental.stats();
        assert_eq!(stats.item_hits, 1, "unchanged alpha must reuse its checked body");
        assert_eq!(stats.item_recomputes, 3, "changed beta alone must recheck");
    }

    #[test]
    fn whitespace_edit_recomputes_span_bearing_diagnostics() {
        let before = "fn beta() -> Int { return \"x\" }\n";
        let after = "fn beta() -> Int {  return \"x\" }\n";
        let mut incremental = CompilerQueries::new();
        let _ = incremental.check_text("spans.jet", before, true);

        let changed = incremental.check_text("spans.jet", after, true);
        let fresh = CompilerQueries::new().check_text("spans.jet", after, true);
        assert_eq!(
            diagnostic_summary(&changed.diagnostics),
            diagnostic_summary(&fresh.diagnostics),
            "cached diagnostics must never retain pre-edit source spans"
        );
        assert_eq!(incremental.stats().item_hits, 0);
        assert_eq!(incremental.stats().item_recomputes, 2);
    }
}
