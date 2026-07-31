//! Canonical incremental front-end queries shared by compiler clients.

use jet_queries::{FileKey, InputKey, QueryEngine, QueryKey};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
pub struct CheckedQuery {
    pub diagnostics: Arc<Vec<crate::Diagnostics::Diagnostic>>,
    pub bundle: Option<Arc<crate::AST::ProgramBundle>>,
    pub effect_facts: Arc<crate::Sema::SemIndexEffectFacts>,
    dependencies: Arc<Vec<PathBuf>>,
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
    sema: HashMap<PathBuf, crate::Sema::IncrementalSemaCache>,
    external_files: HashMap<PathBuf, Vec<PathBuf>>,
    overlays: HashMap<PathBuf, String>,
    volatile_roots: HashSet<PathBuf>,
}

impl CompilerQueries {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn check_text(&mut self, path: &str, text: &str, is_lsp: bool) -> CheckedQuery {
        let root = canonical_path(Path::new(path));
        self.set_document_path(root.clone(), text);
        let path = root.to_string_lossy().into_owned();
        let file = FileKey::new(path.clone());
        let external = external_input(&root);
        self.external_files
            .entry(root.clone())
            .or_insert_with(|| default_external_files(&root));
        self.engine.set_input(
            external.clone(),
            external_fingerprint(
                &root,
                self.external_files.get(&root),
                &self.overlays,
            ),
        );
        let mut overlays = self
            .overlays
            .iter()
            .map(|(path, text)| (path.clone(), text.clone()))
            .collect::<Vec<_>>();
        overlays.sort_by(|left, right| left.0.cmp(&right.0));
        let query = QueryKey::for_file(
            if is_lsp { "checked.lsp" } else { "checked" },
            file.clone(),
        );
        if self.volatile_roots.contains(&root) {
            self.engine.invalidate(&query);
        }
        let checked = {
            let engine = &mut self.engine;
            let sema = self.sema.entry(root.clone()).or_default();
            engine.query(query, |queries| {
                let text = queries
                    .input_text(&InputKey::file(file.clone()))
                    .unwrap_or_default();
                let _ = queries.input_text(&external);
                if let Some((_, source)) = overlays.iter_mut().find(|(path, _)| path == &root) {
                    *source = text;
                }
                let overlay_refs = overlays
                    .iter()
                    .map(|(path, text)| (path.as_path(), text.as_str()))
                    .collect::<Vec<_>>();
                let (diagnostics, bundle, facts, dependencies) =
                    crate::Driver::check_file_with_effect_facts_incremental_overlays(
                        &path,
                        &overlay_refs,
                        is_lsp,
                        sema,
                    );
                CheckedQuery {
                    diagnostics: Arc::new(diagnostics),
                    bundle: bundle.map(Arc::new),
                    effect_facts: Arc::new(facts),
                    dependencies: Arc::new(dependencies),
                }
            })
        };
        self.update_external_state(&root, &checked);
        checked
    }

    pub fn check_disk(&mut self, path: &str, is_lsp: bool) -> CheckedQuery {
        let root = canonical_path(Path::new(path));
        let file = FileKey::new(root.to_string_lossy());
        self.invalidate_file(&file);
        match std::fs::read_to_string(path) {
            Ok(text) => self.check_text(path, &text, is_lsp),
            Err(_) => {
                let (diagnostics, bundle, facts) =
                    crate::Driver::check_file_with_effect_facts(path, None, is_lsp);
                CheckedQuery {
                    diagnostics: Arc::new(diagnostics),
                    bundle: bundle.map(Arc::new),
                    effect_facts: Arc::new(facts),
                    dependencies: Arc::new(Vec::new()),
                }
            }
        }
    }

    pub fn lex_text(&mut self, path: &str, text: &str) -> Arc<Vec<crate::Lexer::Token>> {
        let root = canonical_path(Path::new(path));
        self.set_document_path(root.clone(), text);
        let file = FileKey::new(root.to_string_lossy());
        self.engine
            .query(QueryKey::for_file("tokens", file.clone()), |queries| {
                let text = queries
                    .input_text(&InputKey::file(file))
                    .unwrap_or_default();
                Arc::new(crate::Lexer::lex(&text).0)
            })
    }

    pub fn remove_document(&mut self, path: &str) {
        let root = canonical_path(Path::new(path));
        self.overlays.remove(&root);
        let file = FileKey::new(root.to_string_lossy());
        if self
            .engine
            .remove_input(&InputKey::file(file.clone()))
        {
            self.invalidate_file(&file);
            self.engine.remove_input(&external_input(&root));
            self.sema.remove(&root);
            self.external_files.remove(&root);
            self.volatile_roots.remove(&root);
        }
    }

    pub fn stats(&self) -> CompilerQueryStats {
        let query = self.engine.stats();
        let item = self.sema.values().map(|cache| cache.stats()).fold(
            crate::Sema::IncrementalSemaStats::default(),
            |mut total, stats| {
                total.hits += stats.hits;
                total.recomputes += stats.recomputes;
                total.live_items += stats.live_items;
                total.live_item_bytes += stats.live_item_bytes;
                total
            },
        );
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

    pub fn set_document(&mut self, path: &str, text: &str) {
        self.set_document_path(canonical_path(Path::new(path)), text);
    }

    fn set_document_path(&mut self, path: PathBuf, text: &str) {
        self.overlays.insert(path.clone(), text.to_string());
        self.engine
            .set_input(InputKey::file(FileKey::new(path.to_string_lossy())), text.to_string());
    }

    fn invalidate_file(&mut self, file: &FileKey) {
        self.engine
            .invalidate(&QueryKey::for_file("checked", file.clone()));
        self.engine
            .invalidate(&QueryKey::for_file("checked.lsp", file.clone()));
    }

    fn update_external_state(&mut self, root: &Path, checked: &CheckedQuery) {
        let mut files = checked
            .dependencies
            .iter()
            .map(|path| canonical_path(path))
            .collect::<Vec<_>>();
        if let Some(bundle) = checked.bundle.as_deref() {
            files.extend(
                bundle
                    .modules
                    .iter()
                    .map(|module| canonical_path(&module.path)),
            );
            files.extend(
                bundle
                    .comptime_inputs
                    .iter()
                    .map(|input| canonical_path(&bundle.project_root.join(&input.path))),
            );
            files.extend([
                bundle.project_root.join("pkg.jet"),
                bundle.project_root.join(".jet/lock"),
            ]);
            if crate::Sema::bundle_has_comptime_evaluation(bundle)
                || !bundle.comptime_inputs.is_empty()
            {
                self.volatile_roots.insert(root.to_path_buf());
            } else {
                self.volatile_roots.remove(root);
            }
        } else {
            files.extend(
                self.external_files
                    .get(root)
                    .into_iter()
                    .flatten()
                    .cloned(),
            );
        }
        files.retain(|path| path != root);
        files.extend(default_external_files(root));
        files.sort();
        files.dedup();
        self.external_files.insert(root.to_path_buf(), files);
        let fingerprint = external_fingerprint(root, self.external_files.get(root), &self.overlays);
        self.engine.set_input(external_input(root), fingerprint);
    }
}

fn external_input(root: &Path) -> InputKey {
    InputKey::new(format!("checked-external:{}", root.display()))
}

fn default_external_files(root: &Path) -> Vec<PathBuf> {
    let project = root.parent().unwrap_or_else(|| Path::new("."));
    let mut files = vec![project.join("pkg.jet"), project.join(".jet/lock")];
    files.sort();
    files
}

fn external_fingerprint(
    root: &Path,
    files: Option<&Vec<PathBuf>>,
    overlays: &HashMap<PathBuf, String>,
) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    root.hash(&mut hasher);
    for path in files.into_iter().flatten() {
        path.hash(&mut hasher);
        if let Some(text) = overlays.get(path) {
            text.hash(&mut hasher);
        } else {
            match std::fs::read(path) {
                Ok(bytes) => bytes.hash(&mut hasher),
                Err(error) => error.kind().hash(&mut hasher),
            }
        }
    }
    format!("{:016x}", hasher.finish())
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

    fn checked_key(path: impl AsRef<Path>) -> QueryKey {
        QueryKey::for_file(
            "checked.lsp",
            FileKey::new(canonical_path(path.as_ref()).to_string_lossy()),
        )
    }

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
        let source = "fn beta() => Int { return 1 }\nfn alpha() => String { return beta() }\n";

        let check = service.check_text("shared.jet", source, true);
        let lsp = service.check_text("shared.jet", source, true);
        assert_eq!(
            diagnostic_summary(&check.diagnostics),
            diagnostic_summary(&lsp.diagnostics)
        );
        assert!(!check.diagnostics.is_empty());
        assert_eq!(
            service.recompute_count(&checked_key("shared.jet")),
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
        let main_source = "module b;\nfn run() => Int { return b.value() }\n";
        let first_dependency = "pub fn value() => Int { return 1 }\n";
        let second_dependency = "pub fn value() => String { return \"changed\" }\n";
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
            service.recompute_count(&checked_key(&main)),
            2
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn local_body_edit_rechecks_only_changed_item() {
        let mut service = CompilerQueries::new();
        let before = "fn alpha() => Int { return 1 }\nfn beta() => Int { return 2 }\n";
        let after = "fn alpha() => Int { return 1 }\nfn beta() => Int { return 3 }\n";

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
        let before = "fn alpha() =[]=> Int { return beta() }\nfn beta() => Int { return 2 }\n";
        let after = "fn alpha() =[]=> Int { return beta() }\nfn beta() => Int { print(\"x\"); return 2 }\n";
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
    fn cached_nonce_diagnostic_tracks_callee_effect_precedence() {
        let source = |next_body: &str, marker: u8| {
            format!(
                r#"use core.crypto.expert as expert

fn protect() =[]=> {{
    #Unsafe("fixed interop vector") {{
        _ :: expert.xchacha20poly1305_seal(
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [next_byte()],
            [],
            []
        )
    }}
}}

fn next_byte() => U8 {{
    {next_body}
    return 0
}}

fn marker() => Int {{ return {marker} }}
fn run() {{}}
"#
            )
        };
        let pure_body = "               ";
        let codes = |checked: &CheckedQuery| {
            checked
                .diagnostics
                .iter()
                .map(|diagnostic| diagnostic.code.clone())
                .collect::<Vec<_>>()
        };
        let mut incremental = CompilerQueries::new();

        let cold_source = source(pure_body, 0);
        let cold = incremental.check_text("crypto-cache.jet", &cold_source, true);
        assert_eq!(codes(&cold), ["E2702".to_string()]);
        let cold_stats = incremental.stats();

        let warm_source = source(pure_body, 1);
        let warm = incremental.check_text("crypto-cache.jet", &warm_source, true);
        let warm_fresh = CompilerQueries::new().check_text("crypto-cache.jet", &warm_source, true);
        assert_eq!(
            diagnostic_summary(&warm.diagnostics),
            diagnostic_summary(&warm_fresh.diagnostics)
        );
        assert_eq!(codes(&warm), ["E2702".to_string()]);
        let warm_stats = incremental.stats();
        assert_eq!(warm_stats.item_hits - cold_stats.item_hits, 3);

        let impure_source = source("print(\"effect\")", 1);
        let impure = incremental.check_text("crypto-cache.jet", &impure_source, true);
        let impure_fresh =
            CompilerQueries::new().check_text("crypto-cache.jet", &impure_source, true);
        assert_eq!(
            diagnostic_summary(&impure.diagnostics),
            diagnostic_summary(&impure_fresh.diagnostics)
        );
        assert_eq!(codes(&impure), ["E3401".to_string()]);
        let impure_stats = incremental.stats();
        assert_eq!(impure_stats.item_hits - warm_stats.item_hits, 3);

        let restored_source = source(pure_body, 2);
        let restored = incremental.check_text("crypto-cache.jet", &restored_source, true);
        let restored_fresh =
            CompilerQueries::new().check_text("crypto-cache.jet", &restored_source, true);
        assert_eq!(
            diagnostic_summary(&restored.diagnostics),
            diagnostic_summary(&restored_fresh.diagnostics)
        );
        assert_eq!(codes(&restored), ["E2702".to_string()]);
        let restored_stats = incremental.stats();
        assert_eq!(restored_stats.item_hits - impure_stats.item_hits, 2);
        assert_eq!(restored_stats.live_items, 4);
    }

    #[test]
    fn whitespace_edit_recomputes_span_bearing_diagnostics() {
        let before = "fn beta() => Int { return \"x\" }\n";
        let after = "fn beta() => Int {  return \"x\" }\n";
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

    #[test]
    fn unrelated_roots_remain_warm() {
        let root = std::env::temp_dir().join(format!("jet-query-roots-{}", std::process::id()));
        let left = root.join("left/main.jet");
        let right = root.join("right/main.jet");
        std::fs::create_dir_all(left.parent().unwrap()).unwrap();
        std::fs::create_dir_all(right.parent().unwrap()).unwrap();
        let mut service = CompilerQueries::new();
        let left_before = "fn left() => Int { return 1 }\n";
        let left_after = "fn left() => Int { return 2 }\n";
        let right_source = "fn right() => Int { return 3 }\n";

        let _ = service.check_text(&left.to_string_lossy(), left_before, true);
        let _ = service.check_text(&right.to_string_lossy(), right_source, true);
        let _ = service.check_text(&left.to_string_lossy(), left_after, true);
        let hits = service.stats().hits;
        let _ = service.check_text(&right.to_string_lossy(), right_source, true);

        assert_eq!(service.recompute_count(&checked_key(&left)), 2);
        assert_eq!(service.recompute_count(&checked_key(&right)), 1);
        assert_eq!(service.stats().hits, hits + 1, "right root must stay warm");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn overlay_tracks_imported_disk_inputs() {
        let root = std::env::temp_dir().join(format!("jet-query-overlay-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let main = root.join("main.jet");
        let dependency = root.join("b.jet");
        let source = "module b;\nfn run() => Int { return b.value() }\n";
        std::fs::write(&dependency, "pub fn value() => Int { return 1 }\n").unwrap();
        let mut service = CompilerQueries::new();
        assert!(service
            .check_text(&main.to_string_lossy(), source, true)
            .diagnostics
            .is_empty());

        std::fs::write(
            &dependency,
            "pub fn value() => String { return \"changed\" }\n",
        )
        .unwrap();
        assert!(!service
            .check_text(&main.to_string_lossy(), source, true)
            .diagnostics
            .is_empty());
        assert_eq!(service.recompute_count(&checked_key(&main)), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn repaired_import_invalidates_cached_load_failure() {
        let root = std::env::temp_dir().join(format!("jet-query-repair-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let main = root.join("main.jet");
        let dependency = root.join("b.jet");
        let source = "module b;\nfn run() => Int { return b.value() }\n";
        let dependency_source = "pub fn value() => Int { return 1 }\n";
        std::fs::write(&dependency, dependency_source).unwrap();
        let mut service = CompilerQueries::new();
        assert!(service
            .check_text(&main.to_string_lossy(), source, true)
            .diagnostics
            .is_empty());

        std::fs::write(
            &dependency,
            "pub fn value() => Int { return 1 }\n::\n",
        )
        .unwrap();
        let broken = service.check_text(&main.to_string_lossy(), source, true);
        assert!(broken
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0003"));
        let recomputes = service.recompute_count(&checked_key(&main));
        let broken_again = service.check_text(&main.to_string_lossy(), source, true);
        assert_eq!(
            diagnostic_summary(&broken.diagnostics),
            diagnostic_summary(&broken_again.diagnostics)
        );
        assert_eq!(service.recompute_count(&checked_key(&main)), recomputes);

        std::fs::write(&dependency, dependency_source).unwrap();
        let repaired = service.check_text(&main.to_string_lossy(), source, true);
        let fresh = CompilerQueries::new().check_text(&main.to_string_lossy(), source, true);
        assert_eq!(
            diagnostic_summary(&repaired.diagnostics),
            diagnostic_summary(&fresh.diagnostics)
        );
        assert!(!repaired
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0003"));
        assert!(repaired.diagnostics.is_empty(), "{:#?}", repaired.diagnostics);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn first_load_failure_tracks_import_for_repair() {
        let root = std::env::temp_dir().join(format!(
            "jet-query-first-repair-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let main = root.join("main.jet");
        let dependency = root.join("b.jet");
        let source = "module b;\nfn run() => Int { return b.value() }\n";
        let dependency_source = "pub fn value() => Int { return 1 }\n";
        std::fs::write(
            &dependency,
            "pub fn value() => Int { return 1 }\n::\n",
        )
        .unwrap();
        let mut service = CompilerQueries::new();
        let broken = service.check_text(&main.to_string_lossy(), source, true);
        assert!(broken
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E0003"));

        std::fs::write(&dependency, dependency_source).unwrap();
        let repaired = service.check_text(&main.to_string_lossy(), source, true);
        let fresh = CompilerQueries::new().check_text(&main.to_string_lossy(), source, true);
        assert_eq!(
            diagnostic_summary(&repaired.diagnostics),
            diagnostic_summary(&fresh.diagnostics)
        );
        assert!(repaired.diagnostics.is_empty(), "{:#?}", repaired.diagnostics);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn missing_first_import_tracks_both_module_candidates() {
        for nested in [false, true] {
            let root = std::env::temp_dir().join(format!(
                "jet-query-missing-repair-{}-{nested}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            let main = root.join("main.jet");
            let source = "module b;\nfn run() => Int { return b.value() }\n";
            let mut service = CompilerQueries::new();
            let missing = service.check_text(&main.to_string_lossy(), source, true);
            assert!(missing
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "E0607"));

            let dependency = if nested {
                let directory = root.join("b");
                std::fs::create_dir(&directory).unwrap();
                directory.join("module.jet")
            } else {
                root.join("b.jet")
            };
            std::fs::write(&dependency, "pub fn value() => Int { return 1 }\n").unwrap();
            let repaired = service.check_text(&main.to_string_lossy(), source, true);
            let fresh = CompilerQueries::new().check_text(&main.to_string_lossy(), source, true);
            assert_eq!(
                diagnostic_summary(&repaired.diagnostics),
                diagnostic_summary(&fresh.diagnostics)
            );
            assert!(repaired.diagnostics.is_empty(), "{:#?}", repaired.diagnostics);
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn comptime_local_disables_replay() {
        let mut service = CompilerQueries::new();
        let source = "fn run() {\n    #Known value :: 1\n    print(\"{value}\")\n}\n";
        assert!(service.check_text("comptime.jet", source, true).diagnostics.is_empty());
        assert!(service.check_text("comptime.jet", source, true).diagnostics.is_empty());
        assert_eq!(service.recompute_count(&checked_key("comptime.jet")), 2);
        assert_eq!(service.stats().item_hits, 0);
        assert_eq!(service.stats().live_items, 0);
    }

    #[test]
    fn changed_embed_input_is_never_replayed() {
        let root = std::env::temp_dir().join(format!("jet-query-embed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let main = root.join("main.jet");
        let asset = root.join("message.txt");
        let source = concat!(
            "#Known message :: embed_file(\"message.txt\")\n",
            "fn read() => String { return message }\n"
        );
        std::fs::write(&asset, "first").unwrap();
        let mut service = CompilerQueries::new();
        let first = service.check_text(&main.to_string_lossy(), source, true);
        assert!(first.diagnostics.is_empty());
        assert!(format!("{:?}", first.bundle).contains("first"));

        std::fs::write(&asset, "second").unwrap();
        let second = service.check_text(&main.to_string_lossy(), source, true);
        assert!(second.diagnostics.is_empty());
        assert!(format!("{:?}", second.bundle).contains("second"));
        assert_eq!(service.recompute_count(&checked_key(&main)), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn retained_item_bytes_grow_with_retained_payloads() {
        let mut service = CompilerQueries::new();
        let one = "fn alpha() => Int { return 1 }\n";
        let two = concat!(
            "fn alpha() => Int { return 1 }\n",
            "fn beta() => Int { return \"a deliberately long wrong value\" }\n"
        );
        let _ = service.check_text("memory.jet", one, true);
        let before = service.stats();
        let _ = service.check_text("memory.jet", two, true);
        let after = service.stats();

        assert_eq!(after.live_items, 2);
        assert!(
            after.live_item_bytes >= before.live_item_bytes + (two.len() - one.len()),
            "retained-byte total must cover the added key, function, and diagnostic payload"
        );
    }
}
