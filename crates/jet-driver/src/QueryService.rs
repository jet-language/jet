//! Canonical incremental front-end queries shared by compiler clients.

use jet_queries::{FileKey, InputKey, QueryEngine, QueryKey};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
pub struct CheckedQuery {
    pub diagnostics: Arc<Vec<crate::Diagnostics::Diagnostic>>,
    pub bundle: Option<Arc<crate::AST::ProgramBundle>>,
    pub effect_facts: Arc<crate::Sema::SemIndexEffectFacts>,
    dependencies: Arc<Vec<PathBuf>>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
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
    pub recomputed_items: Vec<String>,
}

/// One measured front-end re-verdict. The named items are the semantic
/// definitions actually rechecked after the caller's edit.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReverdictReceipt {
    pub source_path: String,
    pub source_bytes: usize,
    pub edit_bytes: usize,
    pub program_items: usize,
    pub reverified_items: Vec<String>,
    pub elapsed_us: u128,
    pub query_recomputes: u64,
    pub item_hits: u64,
    pub item_recomputes: u64,
    /// Effective callable failure contracts observed in the checked bundle.
    /// This keeps the incremental proof tied to the same sema facts exposed
    /// by hover, semindex, and inspect.
    pub callable_contracts: Vec<CallableContractReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CallableContractReceipt {
    pub identity: String,
    pub failure_contract: String,
    pub failure_source: String,
}

impl ReverdictReceipt {
    pub fn blast_radius(&self) -> usize {
        self.reverified_items.len()
    }

    /// Stable receipt envelope. Timing remains evidence in the content, while
    /// the id binds the exact measured claim for later readers.
    pub fn to_json(&self) -> String {
        use jet_foundation::PerformanceBudget::CanonicalJson;

        let items = CanonicalJson::Array(
            self.reverified_items
                .iter()
                .cloned()
                .map(CanonicalJson::String)
                .collect(),
        );
        let callable_contracts = CanonicalJson::Array(
            self.callable_contracts
                .iter()
                .map(|contract| {
                    CanonicalJson::object([
                        (
                            "identity".into(),
                            CanonicalJson::String(contract.identity.clone()),
                        ),
                        (
                            "failure_contract".into(),
                            CanonicalJson::String(contract.failure_contract.clone()),
                        ),
                        (
                            "failure_source".into(),
                            CanonicalJson::String(contract.failure_source.clone()),
                        ),
                    ])
                    .expect("fixed receipt callable contract fields")
                })
                .collect(),
        );
        let content = CanonicalJson::object([
            (
                "claim".into(),
                CanonicalJson::String("D-DEVR-CONE1=A".into()),
            ),
            (
                "edit_bytes".into(),
                CanonicalJson::Integer(self.edit_bytes.to_string()),
            ),
            (
                "elapsed_us".into(),
                CanonicalJson::Integer(self.elapsed_us.to_string()),
            ),
            ("evidence".into(), CanonicalJson::String("observed".into())),
            (
                "item_hits".into(),
                CanonicalJson::Integer(self.item_hits.to_string()),
            ),
            (
                "item_recomputes".into(),
                CanonicalJson::Integer(self.item_recomputes.to_string()),
            ),
            (
                "program_items".into(),
                CanonicalJson::Integer(self.program_items.to_string()),
            ),
            ("reverified_items".into(), items.clone()),
            ("callable_contracts".into(), callable_contracts),
            (
                "blast_radius".into(),
                CanonicalJson::object([
                    (
                        "count".into(),
                        CanonicalJson::Integer(self.blast_radius().to_string()),
                    ),
                    ("items".into(), items),
                ])
                .expect("fixed receipt fields"),
            ),
            (
                "source_bytes".into(),
                CanonicalJson::Integer(self.source_bytes.to_string()),
            ),
            (
                "source_path".into(),
                CanonicalJson::String(self.source_path.clone()),
            ),
            (
                "query_recomputes".into(),
                CanonicalJson::Integer(self.query_recomputes.to_string()),
            ),
        ])
        .expect("fixed receipt fields");
        let receipt_id = content.sha256();
        let receipt = CanonicalJson::object([
            ("content".into(), content),
            ("receipt_id".into(), CanonicalJson::String(receipt_id)),
            (
                "schema".into(),
                CanonicalJson::String("jet.reverdict-receipt".into()),
            ),
                ("version".into(), CanonicalJson::Integer("2".into())),
        ])
        .expect("fixed receipt envelope");
        String::from_utf8(receipt.bytes()).expect("canonical JSON is UTF-8")
    }
}

fn callable_contracts(bundle: &crate::AST::ProgramBundle) -> Vec<CallableContractReceipt> {
    let mut contracts = Vec::new();
    for module in &bundle.modules {
        collect_callable_contracts(&module.items, &module.display, "", &mut contracts);
    }
    contracts.sort_by(|left, right| left.identity.cmp(&right.identity));
    contracts
}

fn collect_callable_contracts(
    items: &[crate::AST::Item],
    module: &str,
    parent: &str,
    output: &mut Vec<CallableContractReceipt>,
) {
    for item in items {
        match item {
            crate::AST::Item::Func(function) => {
                push_callable_contract(function, join_identity(module, parent, &function.name), output);
            }
            crate::AST::Item::Struct(definition) => {
                let owner = join_identity(module, parent, &definition.name);
                for method in &definition.methods {
                    push_callable_contract(method, format!("{owner}.{}", method.name), output);
                }
                for implementation in &definition.trait_impls {
                    for method in &implementation.methods {
                        push_callable_contract(method, format!("{owner}.{}", method.name), output);
                    }
                }
            }
            crate::AST::Item::Enum(definition) => {
                let owner = join_identity(module, parent, &definition.name);
                for method in &definition.methods {
                    push_callable_contract(method, format!("{owner}.{}", method.name), output);
                }
                for implementation in &definition.trait_impls {
                    for method in &implementation.methods {
                        push_callable_contract(method, format!("{owner}.{}", method.name), output);
                    }
                }
            }
            crate::AST::Item::Impl(implementation) => {
                let owner = join_identity(module, parent, &implementation.type_name);
                for method in &implementation.methods {
                    push_callable_contract(method, format!("{owner}.{}", method.name), output);
                }
            }
            crate::AST::Item::Trait(definition) => {
                let owner = join_identity(module, parent, &definition.name);
                for method in &definition.methods {
                    let failure = method.failure_contract();
                    output.push(CallableContractReceipt {
                        identity: format!("{owner}.{}", method.name),
                        failure_contract: failure.effective_type().name(),
                        failure_source: failure.source(),
                    });
                }
            }
            crate::AST::Item::CodeModule(definition) => {
                if let Some(body) = &definition.body {
                    collect_callable_contracts(
                        body,
                        module,
                        &join_identity(parent, "", &definition.name),
                        output,
                    );
                }
            }
            crate::AST::Item::GenericModule(definition) => {
                collect_callable_contracts(
                    &definition.body,
                    module,
                    &join_identity(parent, "", &definition.name),
                    output,
                );
            }
            _ => {}
        }
    }
}

fn push_callable_contract(
    function: &crate::AST::Func,
    identity: String,
    output: &mut Vec<CallableContractReceipt>,
) {
    let failure = function.failure_contract();
    output.push(CallableContractReceipt {
        identity,
        failure_contract: failure.effective_type().name(),
        failure_source: failure.source(),
    });
}

fn join_identity(module: &str, parent: &str, name: &str) -> String {
    let local = if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}::{name}")
    };
    if module.is_empty() {
        local
    } else {
        format!("{module}::{local}")
    }
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

    /// Check one root through the shared query path. `is_lsp = false` is the
    /// batch frontend mode; both modes retain the same sema cache and module
    /// interface invalidation rules.
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
            external_fingerprint(&root, self.external_files.get(&root), &self.overlays),
        );
        let mut overlays = self
            .overlays
            .iter()
            .map(|(path, text)| (path.clone(), text.clone()))
            .collect::<Vec<_>>();
        overlays.sort_by(|left, right| left.0.cmp(&right.0));
        let frontend_sources = self.frontend_sources(&root);
        let query =
            QueryKey::for_file(if is_lsp { "checked.lsp" } else { "checked" }, file.clone());
        if self.volatile_roots.contains(&root) {
            self.engine.invalidate(&query);
        }
        let checked = {
            let engine = &mut self.engine;
            let sema = self.sema.entry(root.clone()).or_default();
            engine.query(query, |queries| {
                let mut prepared_frontend =
                    crate::Loader::prepare_frontend_sources(frontend_sources);
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
                    crate::Driver::check_file_with_effect_facts_incremental_overlays_prepared(
                        &path,
                        &overlay_refs,
                        is_lsp,
                        sema,
                        &mut prepared_frontend,
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

    /// Check one edit and return the measured semantic cone as a receipt.
    /// `check_text` remains the compatibility-free low-level operation used by
    /// existing callers; this wrapper is the explicit measurement seam.
    pub fn check_text_with_receipt(
        &mut self,
        path: &str,
        text: &str,
        is_lsp: bool,
    ) -> (CheckedQuery, ReverdictReceipt) {
        let root = canonical_path(Path::new(path));
        // Measure the edit before `check_text` takes `&mut self`; borrowing the
        // overlay text across that call would conflict, and cloning it here
        // would copy the whole previous source for one length comparison.
        let edit_bytes = edit_bytes(self.overlays.get(&root).map(String::as_str), text);
        let before = self.stats();
        for cache in self.sema.values_mut() {
            cache.clear_measurement();
        }
        let started = Instant::now();
        let checked = self.check_text(path, text, is_lsp);
        let elapsed_us = started.elapsed().as_micros();
        let after = self.stats();
        let mut reverified_items = after.recomputed_items.clone();
        reverified_items.sort();
        reverified_items.dedup();
        let receipt = ReverdictReceipt {
            source_path: root.to_string_lossy().into_owned(),
            source_bytes: text.len(),
            edit_bytes,
            program_items: after.live_items,
            reverified_items,
            elapsed_us,
            query_recomputes: after.recomputes.saturating_sub(before.recomputes),
            item_hits: after.item_hits.saturating_sub(before.item_hits),
            item_recomputes: after.item_recomputes.saturating_sub(before.item_recomputes),
            callable_contracts: checked
                .bundle
                .as_deref()
                .map(callable_contracts)
                .unwrap_or_default(),
        };
        (checked, receipt)
    }

    pub fn check_disk(&mut self, path: &str, is_lsp: bool) -> CheckedQuery {
        let root = canonical_path(Path::new(path));
        match std::fs::read_to_string(path) {
            Ok(text) => self.check_text(path, &text, is_lsp),
            Err(_) => {
                let file = FileKey::new(root.to_string_lossy());
                self.invalidate_file(&file);
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
        if self.engine.remove_input(&InputKey::file(file.clone())) {
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
                total.recomputed_items.extend(stats.recomputed_items);
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
            recomputed_items: item.recomputed_items,
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
        self.engine.set_input(
            InputKey::file(FileKey::new(path.to_string_lossy())),
            text.to_string(),
        );
    }

    fn frontend_sources(&self, root: &Path) -> Vec<(PathBuf, String)> {
        let mut sources = self.overlays.clone();
        if let Some(files) = self.external_files.get(root) {
            for path in files {
                if sources.contains_key(path)
                    || path.extension().and_then(|extension| extension.to_str()) != Some("jet")
                {
                    continue;
                }
                if let Ok(source) = std::fs::read_to_string(path) {
                    sources.insert(path.clone(), source);
                }
            }
        }
        let mut sources = sources.into_iter().collect::<Vec<_>>();
        sources.sort_by(|left, right| left.0.cmp(&right.0));
        sources
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
                crate::Manifest::manifest_path_in(&bundle.project_root),
                bundle.project_root.join(".jet/lock"),
            ]);
            if bundle.project_root.join("package.jet").is_file()
                && bundle.project_root.join("pkg.jet").is_file()
            {
                files.push(bundle.project_root.join("pkg.jet"));
            }
            if crate::Sema::bundle_has_comptime_evaluation(bundle)
                || !bundle.comptime_inputs.is_empty()
            {
                self.volatile_roots.insert(root.to_path_buf());
            } else {
                self.volatile_roots.remove(root);
            }
        } else {
            files.extend(self.external_files.get(root).into_iter().flatten().cloned());
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
    let mut files = vec![
        crate::Manifest::manifest_path_in(project),
        project.join(".jet/lock"),
    ];
    if project.join("package.jet").is_file() && project.join("pkg.jet").is_file() {
        files.push(project.join("pkg.jet"));
    }
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

fn edit_bytes(old: Option<&str>, new: &str) -> usize {
    let Some(old) = old else {
        return new.len();
    };
    if old == new {
        return 0;
    }
    let prefix = old
        .bytes()
        .zip(new.bytes())
        .take_while(|(left, right)| left == right)
        .count();
    let old_tail = old.len() - prefix;
    let new_tail = new.len() - prefix;
    let suffix = old[..prefix + old_tail]
        .bytes()
        .rev()
        .zip(new[..prefix + new_tail].bytes().rev())
        .take_while(|(left, right)| left == right)
        .count()
        .min(old_tail)
        .min(new_tail);
    old_tail - suffix + new_tail - suffix
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

    fn batch_checked_key(path: impl AsRef<Path>) -> QueryKey {
        QueryKey::for_file(
            "checked",
            FileKey::new(canonical_path(path.as_ref()).to_string_lossy()),
        )
    }

    fn diagnostic_summary(
        diagnostics: &[crate::Diagnostics::Diagnostic],
    ) -> Vec<(
        String,
        String,
        String,
        String,
        Option<crate::Diagnostics::Span>,
    )> {
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
        let source = "fn beta() Int -> { return 1 }\nfn alpha() String -> { return beta() }\n";

        let check = service.check_text("shared.jet", source, true);
        let lsp = service.check_text("shared.jet", source, true);
        assert_eq!(
            diagnostic_summary(&check.diagnostics),
            diagnostic_summary(&lsp.diagnostics)
        );
        assert!(!check.diagnostics.is_empty());
        assert_eq!(service.recompute_count(&checked_key("shared.jet")), 1);
        assert_eq!(service.stats().hits, 1);
    }

    #[test]
    fn changed_import_invalidates_cached_importer() {
        let root = std::env::temp_dir().join(format!("jet-query-import-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let main = root.join("main.jet");
        let dependency = root.join("b.jet");
        let main_source = "module b;\nfn run() Int -> { return b.value() }\n";
        let first_dependency = "pub fn value() Int -> { return 1 }\n";
        let second_dependency = "pub fn value() String -> { return \"changed\" }\n";
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
        assert_eq!(service.recompute_count(&checked_key(&main)), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn batch_disk_interface_change_keeps_unrelated_module_warm() {
        let root =
            std::env::temp_dir().join(format!("jet-query-batch-interface-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let main = root.join("main.jet");
        let dependency = root.join("b.jet");
        let unrelated = root.join("c.jet");
        let lock_dir = root.join(".jet");
        let main_source =
            "module b;\nmodule c;\nfn run() Int -> { return b.value() + c.other() }\n";
        let dependency_source = "pub fn value() Int -> { return 1 }\n";
        let changed_dependency = "pub fn value() String -> { return \"changed\" }\n";
        let unrelated_source = "pub fn other() Int -> { return 2 }\n";
        std::fs::write(&main, main_source).unwrap();
        std::fs::write(&dependency, dependency_source).unwrap();
        std::fs::write(&unrelated, unrelated_source).unwrap();
        std::fs::create_dir_all(&lock_dir).unwrap();
        std::fs::write(
            lock_dir.join("lock"),
            "version = 1\n\n[build.stamp]\ndirty = false\ntoolchain = \"test\"\nat = \"test\"\n",
        )
        .unwrap();

        let mut service = CompilerQueries::new();
        let first = service.check_disk(&main.to_string_lossy(), false);
        assert!(first.diagnostics.is_empty(), "{:#?}", first.diagnostics);
        assert_eq!(
            service.recompute_count(&batch_checked_key(&main)),
            1,
            "the first disk batch check must compute the root"
        );

        let discovered = service.check_disk(&main.to_string_lossy(), false);
        assert!(discovered.diagnostics.is_empty());
        assert_eq!(
            service.recompute_count(&batch_checked_key(&main)),
            2,
            "the first dependency discovery must settle the external input set"
        );
        let cold = service.stats();

        let unchanged = service.check_disk(&main.to_string_lossy(), false);
        assert!(unchanged.diagnostics.is_empty());
        assert_eq!(
            service.recompute_count(&batch_checked_key(&main)),
            2,
            "an unchanged disk batch must reuse the checked query"
        );
        assert!(
            service.stats().hits > cold.hits,
            "an unchanged disk batch must record a query hit"
        );

        std::fs::write(&dependency, changed_dependency).unwrap();
        let changed = service.check_disk(&main.to_string_lossy(), false);
        assert!(
            !changed.diagnostics.is_empty(),
            "a changed imported interface must recheck the real batch path"
        );
        let warm = service.stats();
        assert!(
            warm.item_hits > cold.item_hits,
            "an unrelated module must remain reusable after dependency invalidation: cold={cold:?}, warm={warm:?}"
        );

        let fresh = {
            let mut fresh = CompilerQueries::new();
            fresh.check_disk(&main.to_string_lossy(), false)
        };
        assert_eq!(
            diagnostic_summary(&changed.diagnostics),
            diagnostic_summary(&fresh.diagnostics),
            "batch invalidation must preserve fresh-check diagnostics"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn local_body_edit_rechecks_only_changed_item() {
        let mut service = CompilerQueries::new();
        let before = "fn alpha() Int -> { return 1 }\nfn beta() Int -> { return 2 }\n";
        let after = "fn alpha() Int -> { return 1 }\nfn beta() Int -> { return 3 }\n";

        assert!(service
            .check_text("items.jet", before, true)
            .diagnostics
            .is_empty());
        let cold = service.stats();
        assert_eq!(cold.item_hits, 0);
        assert_eq!(cold.item_recomputes, 2);
        assert_eq!(cold.live_items, 2);

        assert!(service
            .check_text("items.jet", after, true)
            .diagnostics
            .is_empty());
        let warm = service.stats();
        assert_eq!(warm.item_hits, 1, "unchanged alpha must reuse checked body");
        assert_eq!(warm.item_recomputes, 3, "only changed beta may recheck");
        assert_eq!(warm.live_items, 2);
        assert!(warm.live_item_bytes > before.len());
    }

    #[test]
    fn cached_caller_observes_changed_callee_effects() {
        let before = "fn alpha() Int -[]> { return beta() }\nfn beta() Int -> { return 2 }\n";
        let after =
            "fn alpha() Int -[]> { return beta() }\nfn beta() Int -> { print(\"x\"); return 2 }\n";
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
        assert!(changed
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "E3401"));
        let stats = incremental.stats();
        assert_eq!(
            stats.item_hits, 1,
            "unchanged alpha must reuse its checked body"
        );
        assert_eq!(stats.item_recomputes, 3, "changed beta alone must recheck");
    }

    #[test]
    fn cached_nonce_diagnostic_tracks_callee_effect_precedence() {
        let source = |next_body: &str, marker: u8| {
            format!(
                r#"use core.crypto.expert as expert

fn protect() -[]> {{
    #Unsafe("fixed interop vector") {{
        _ :: expert.xchacha20poly1305_seal(
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            [next_byte()],
            [],
            []
        )
    }}
}}

fn next_byte() U8 -> {{
    {next_body}
    return 0
}}

fn marker() Int -> {{ return {marker} }}
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
        let before = "fn beta() Int -> { return \"x\" }\n";
        let after = "fn beta() Int -> {  return \"x\" }\n";
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
        let left_before = "fn left() Int -> { return 1 }\n";
        let left_after = "fn left() Int -> { return 2 }\n";
        let right_source = "fn right() Int -> { return 3 }\n";

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
        let source = "module b;\nfn run() Int -> { return b.value() }\n";
        std::fs::write(&dependency, "pub fn value() Int -> { return 1 }\n").unwrap();
        let mut service = CompilerQueries::new();
        assert!(service
            .check_text(&main.to_string_lossy(), source, true)
            .diagnostics
            .is_empty());

        std::fs::write(
            &dependency,
            "pub fn value() String -> { return \"changed\" }\n",
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
        let source = "module b;\nfn run() Int -> { return b.value() }\n";
        let dependency_source = "pub fn value() Int -> { return 1 }\n";
        std::fs::write(&dependency, dependency_source).unwrap();
        let mut service = CompilerQueries::new();
        assert!(service
            .check_text(&main.to_string_lossy(), source, true)
            .diagnostics
            .is_empty());

        std::fs::write(&dependency, "pub fn value() Int -> { return 1 }\n::\n").unwrap();
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
        assert!(
            repaired.diagnostics.is_empty(),
            "{:#?}",
            repaired.diagnostics
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn first_load_failure_tracks_import_for_repair() {
        let root =
            std::env::temp_dir().join(format!("jet-query-first-repair-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let main = root.join("main.jet");
        let dependency = root.join("b.jet");
        let source = "module b;\nfn run() Int -> { return b.value() }\n";
        let dependency_source = "pub fn value() Int -> { return 1 }\n";
        std::fs::write(&dependency, "pub fn value() Int -> { return 1 }\n::\n").unwrap();
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
        assert!(
            repaired.diagnostics.is_empty(),
            "{:#?}",
            repaired.diagnostics
        );
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
            let source = "module b;\nfn run() Int -> { return b.value() }\n";
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
            std::fs::write(&dependency, "pub fn value() Int -> { return 1 }\n").unwrap();
            let repaired = service.check_text(&main.to_string_lossy(), source, true);
            let fresh = CompilerQueries::new().check_text(&main.to_string_lossy(), source, true);
            assert_eq!(
                diagnostic_summary(&repaired.diagnostics),
                diagnostic_summary(&fresh.diagnostics)
            );
            assert!(
                repaired.diagnostics.is_empty(),
                "{:#?}",
                repaired.diagnostics
            );
            let _ = std::fs::remove_dir_all(root);
        }
    }

    #[test]
    fn comptime_local_disables_replay() {
        let mut service = CompilerQueries::new();
        let source = "fn run() {\n    @value :: 1\n    print(\"{value}\")\n}\n";
        assert!(service
            .check_text("comptime.jet", source, true)
            .diagnostics
            .is_empty());
        assert!(service
            .check_text("comptime.jet", source, true)
            .diagnostics
            .is_empty());
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
            "@message :: embed_file(\"message.txt\")\n",
            "fn read() String -> { return message }\n"
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
        let one = "fn alpha() Int -> { return 1 }\n";
        let two = concat!(
            "fn alpha() Int -> { return 1 }\n",
            "fn beta() Int -> { return \"a deliberately long wrong value\" }\n"
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

    fn cone_source(functions: usize) -> String {
        let mut source = (0..functions)
            .map(|index| format!("fn helper_{index}() Int -> {{ return {index} }}\n"))
            .collect::<String>();
        source.push_str("fn target() Int -> { return 1 }\nfn run() Int -> { return target() }\n");
        source
    }

    #[test]
    fn reverdict_receipt_reports_the_edited_function_and_is_canonical() {
        use jet_foundation::PerformanceBudget::CanonicalJson;

        let before = cone_source(8);
        let after = before.replace(
            "fn target() Int -> { return 1 }",
            "fn target() Int -> { return 2 }",
        );
        let mut service = CompilerQueries::new();
        let (cold, _) = service.check_text_with_receipt("cone.jet", &before, true);
        assert!(cold.diagnostics.is_empty(), "{:#?}", cold.diagnostics);
        let (checked, receipt) = service.check_text_with_receipt("cone.jet", &after, true);
        assert!(checked.diagnostics.is_empty(), "{:#?}", checked.diagnostics);
        assert_eq!(receipt.program_items, 10);
        assert_eq!(receipt.item_recomputes, 1);
        assert_eq!(receipt.blast_radius(), 1);
        assert!(receipt.reverified_items[0].contains("fn:target"));
        assert!(receipt
            .callable_contracts
            .iter()
            .any(|contract| contract.failure_source == "implicit default !Err"));
        assert_eq!(receipt.edit_bytes, 2);

        let json = receipt.to_json();
        let CanonicalJson::Object(envelope) =
            CanonicalJson::parse_canonical(json.as_bytes()).unwrap()
        else {
            panic!("re-verdict receipt envelope")
        };
        assert_eq!(
            envelope["schema"],
            CanonicalJson::String("jet.reverdict-receipt".into())
        );
        assert!(matches!(envelope["receipt_id"], CanonicalJson::String(_)));
        let CanonicalJson::Object(content) = &envelope["content"] else {
            panic!("re-verdict receipt content")
        };
        assert!(matches!(
            &content["callable_contracts"],
            CanonicalJson::Array(values) if !values.is_empty()
        ));
        let CanonicalJson::Object(blast_radius) = &content["blast_radius"] else {
            panic!("re-verdict blast radius")
        };
        assert_eq!(blast_radius["count"], CanonicalJson::Integer("1".into()));
        assert_eq!(
            content["claim"],
            CanonicalJson::String("D-DEVR-CONE1=A".into())
        );
    }

    #[test]
    fn cone_benchmark_keeps_one_function_edit_flat_as_program_grows() {
        // `item_recomputes` is the deterministic work unit; the receipt also
        // records wall time without making this machine-sensitive assertion.
        let mut measurements = Vec::new();
        for functions in [16, 128] {
            let before = cone_source(functions);
            let after = before.replace(
                "fn target() Int -> { return 1 }",
                "fn target() Int -> { return 2 }",
            );
            let mut service = CompilerQueries::new();
            let (cold, _) = service.check_text_with_receipt("cone-bench.jet", &before, true);
            assert!(cold.diagnostics.is_empty(), "{:#?}", cold.diagnostics);
            let (checked, receipt) =
                service.check_text_with_receipt("cone-bench.jet", &after, true);
            assert!(checked.diagnostics.is_empty(), "{:#?}", checked.diagnostics);
            measurements.push(receipt);
        }

        let [small, large] = measurements.as_slice() else {
            panic!("cone benchmark measurements")
        };
        assert!(large.program_items > small.program_items);
        assert_eq!(small.item_recomputes, 1);
        assert_eq!(large.item_recomputes, 1);
        assert_eq!(small.blast_radius(), large.blast_radius());
        assert_eq!(small.reverified_items, large.reverified_items);
    }
}
