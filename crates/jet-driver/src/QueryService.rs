//! Canonical incremental front-end queries shared by compiler clients.

use jet_queries::{FileKey, InputKey, QueryEngine, QueryKey, QueryStats};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct CheckedQuery {
    pub diagnostics: Arc<Vec<crate::Diagnostics::Diagnostic>>,
    pub bundle: Option<Arc<crate::AST::ProgramBundle>>,
    pub effect_facts: Arc<crate::Sema::SemIndexEffectFacts>,
}

#[derive(Default)]
pub struct CompilerQueries {
    engine: QueryEngine,
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
        self.engine.query(
            QueryKey::for_file(if is_lsp { "checked.lsp" } else { "checked" }, file.clone()),
            |queries| {
                let text = queries
                    .input_text(&InputKey::file(file))
                    .unwrap_or_default();
                let (diagnostics, bundle, facts) = crate::Driver::check_file_with_effect_facts(
                    &path,
                    Some((&abs, &text)),
                    is_lsp,
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
        }
    }

    pub fn stats(&self) -> QueryStats {
        self.engine.stats()
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
}
