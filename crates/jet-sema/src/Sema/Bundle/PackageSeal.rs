//! #2517 stage S2: dependency packages checked once, reused from records.
//!
//! A `jet check` runs inside a package-record session (`with_package_records`).
//! After registration and the failure-union probe, every package of the
//! bundle gets its check key: compiler identity, the target facts and check
//! environment sema reads, the package's source digest, and the interface
//! digests its direct dependencies produced in this run. A dependency package
//! (Core, or a package under a dependency root) whose key has a stored record
//! is **sealed**: its bodies are erased, it is not body-checked, and its item
//! records seed the facts its bodies would have produced. Every other
//! package is checked, and each clean dependency package publishes its
//! record for the next run.
//!
//! The record is a bundle of item records (design section 3.5): one entry
//! per effect-graph node the package owns, with its summary and summary
//! digest, plus the per-module facts that are not keyed by item yet
//! (address-taken names, the #3708 inferred-infallible set, exact-int use).
//! Call edges are stored with stable module identities, never loader
//! aliases, and are mapped back to the current bundle's aliases when seeded.
//! Spans are module-absolute; a sealed package has the recorded bytes, so
//! they stay valid. Item reuse inside a checked package (stage S3R,
//! `ItemReuse`) stores item-relative spans in its own item records.
//!
//! A package is not published when the codec cannot carry one of its facts,
//! when its bodies evaluate at compile time or discovered inputs, or when its
//! check reported a diagnostic that reaches the importer's output. Such a
//! package is checked on every run.

use crate::Diagnostics::Span;
use crate::Sema::Effects::{
    AuthorityDelegation, AutodiffObligation, CallbackObligation, ComputeCallFact,
    DiscardedResultFact, EffectSummary, RegionSummary,
};
use crate::Sema::MemoryFacts::{
    MemoryCall, MemoryEvent, MemoryEventKind, MemorySummary, OpenMemoryDispatch,
};
use crate::AST::{Func, Item, ProgramBundle};
use jet_foundation::PackageIdentity::{self as identity, PackageGraph, PackageKind};
use jet_foundation::RecordCodec::{
    encode_record, record_digest, record_map, RecordError, RecordReader, RecordSection,
    RecordValue,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// Schema of the package check record (an action record under the key).
pub const PKG_CHECK_SCHEMA: &str = "jet.pkg-check/v1";
/// Schema of the item bundle the check record points to (content addressed).
pub const ITEMS_SCHEMA: &str = "jet.items/v1";
const ITEM_SUMMARY_SCHEMA: &str = "jet.item-summary/v1";

const SECTION_HEADER: u64 = 1;
const SECTION_MODULES: u64 = 2;
const SECTION_ITEMS: u64 = 3;

/// The record store a package-record session reads and writes. The CLI
/// implements it over the shared `jet-store` cache.
pub trait PackageRecordStore: Send + Sync {
    /// Compiler and Core identity; part of every key.
    fn compiler_identity(&self) -> String;
    /// A stored record of `schema` under `key`, or `None` on a miss.
    fn load(&self, schema: &str, key: &str) -> Option<Vec<u8>>;
    /// Publish a record. Content-addressed schemas use the SHA-256 hex of
    /// `bytes` as `key`. Failing to publish only costs speed.
    fn publish(&self, schema: &str, key: &str, bytes: &[u8]);
    /// A stored action record without recording its use. A check that reads
    /// one record per item uses this and records the uses in `publish_batch`.
    fn load_untouched(&self, schema: &str, key: &str) -> Option<Vec<u8>>;
    /// Record the uses of the `touched` action records and publish `records`
    /// (action records under their keys) in one store transaction. Failing
    /// only costs speed.
    fn publish_batch(&self, schema: &str, touched: &[String], records: Vec<(String, Vec<u8>)>);
    /// The manifest bytes of a package directory.
    fn manifest(&self, root: &std::path::Path) -> Option<Vec<u8>>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageReuse {
    /// Sealed: checked from its stored record, no body checked.
    Reused,
    /// Checked, and its record published for the next run.
    CheckedPublished,
    /// Checked; not recordable (the root package, or a reason in `detail`).
    Checked,
}

impl PackageReuse {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reused => "reused",
            Self::CheckedPublished => "checked+published",
            Self::Checked => "checked",
        }
    }
}

/// One package of one sealed-aware check.
#[derive(Clone, Debug)]
pub struct PackageReuseRow {
    pub identity: String,
    pub key: String,
    pub reuse: PackageReuse,
    pub modules: usize,
    pub detail: String,
}

struct Session {
    store: Arc<dyn PackageRecordStore>,
    rows: Vec<PackageReuseRow>,
}

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

fn session() -> std::sync::MutexGuard<'static, Option<Session>> {
    SESSION.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct SessionGuard;

impl Drop for SessionGuard {
    fn drop(&mut self) {
        session().take();
        super::ItemReuse::end();
    }
}

/// Run `check` with package records enabled for `jet check` bundles, and
/// return its rows. The session is process-wide so it follows the check onto
/// the compiler stack thread; one `jet check` process runs one session.
pub fn with_package_records<R>(
    store: Arc<dyn PackageRecordStore>,
    check: impl FnOnce() -> R,
) -> (R, Vec<PackageReuseRow>) {
    *session() = Some(Session {
        store,
        rows: Vec::new(),
    });
    let guard = SessionGuard;
    let result = check();
    let rows = session().as_mut().map(|state| std::mem::take(&mut state.rows)).unwrap_or_default();
    drop(guard);
    (result, rows)
}

/// Packages sealed so far in the active session.
pub fn sealed_package_count() -> usize {
    session().as_ref().map_or(0, |state| {
        state
            .rows
            .iter()
            .filter(|row| row.reuse == PackageReuse::Reused)
            .count()
    })
}

/// The record store of the active session, if any.
pub(super) fn session_store() -> Option<Arc<dyn PackageRecordStore>> {
    session().as_ref().map(|state| state.store.clone())
}

/// Stored facts of one sealed module.
pub(super) struct SealedModule {
    /// Canonical effect-graph nodes as `(local key, summary)`, with edges
    /// already mapped to this bundle's aliases.
    pub summaries: Vec<(String, EffectSummary)>,
    pub address_taken: Vec<String>,
    pub inferred_infallible: HashSet<String>,
    pub exact_int: bool,
}

/// Per-module facts a checked module contributes to its package record.
#[derive(Default)]
pub(super) struct ModuleOutputs {
    pub address_taken: HashSet<String>,
    pub discovered_inputs: bool,
}

/// One bundle's package plan: keys, sealed packages, and what to publish.
pub(super) struct SealPlan {
    store: Arc<dyn PackageRecordStore>,
    graph: PackageGraph,
    keys: Vec<String>,
    eligible: Vec<Option<String>>,
    sealed: Vec<bool>,
    module_identity: Vec<Option<String>>,
    comptime: Vec<bool>,
    pub modules: HashMap<usize, SealedModule>,
    pub outputs: HashMap<usize, ModuleOutputs>,
}

fn sealed_body(item: &Item) -> bool {
    !matches!(item, Item::Const(_)) && super::Comptime::item_has_comptime_evaluation(item)
}

fn erase_function(function: &mut Func) {
    function.body.clear();
}

/// Bodies of a sealed module are never checked; erase them so no later phase
/// walks an unchecked body.
fn erase_bodies(items: &mut [Item]) {
    for item in items {
        match item {
            Item::Func(function) => erase_function(function),
            Item::Struct(definition) => {
                definition.methods.iter_mut().for_each(erase_function);
                for implementation in &mut definition.trait_impls {
                    implementation.methods.iter_mut().for_each(erase_function);
                }
            }
            Item::Enum(definition) => {
                definition.methods.iter_mut().for_each(erase_function);
                for implementation in &mut definition.trait_impls {
                    implementation.methods.iter_mut().for_each(erase_function);
                }
            }
            Item::Impl(implementation) => implementation.methods.iter_mut().for_each(erase_function),
            Item::Test(test) => test.body.clear(),
            _ => {}
        }
    }
}

impl SealPlan {
    /// Plan one `jet check` bundle after registration and the failure-union
    /// probe (so inferred contracts are part of every interface digest).
    /// `None` outside a package-record session.
    pub(super) fn begin(
        bundle: &mut ProgramBundle,
        ledger: &jet_foundation::Names::NameLedger,
        plugin_digest: &str,
    ) -> Option<Self> {
        let store = session().as_ref()?.store.clone();
        let graph = PackageGraph::from_ledger(bundle, ledger);
        if !graph.is_acyclic() {
            return None;
        }
        // A runnable root lowers the whole program after checking (the cost
        // projection), which needs every body. Until per-package lowering
        // records exist (design stage S4), only library roots seal.
        let runnable = bundle.modules[bundle.entry].items.iter().any(|item| match item {
            Item::Func(function) => function.name == "run",
            Item::Const(constant) => constant.resolved_output.is_some(),
            _ => false,
        });
        if runnable {
            return None;
        }
        let mut environment = identity::KeyHasher::new(identity::TARGET_FACTS_SCHEMA);
        environment
            .field(&identity::target_facts_digest(&identity::target_fact_pairs(bundle)))
            .field(&jet_foundation::SHA256::sha256_hex(
                &super::incremental_global_environment(bundle, plugin_digest),
            ));
        let target = environment.finish();
        let compiler = store.compiler_identity();
        let count = graph.packages.len();
        let mut interfaces = vec![String::new(); count];
        let mut keys = vec![String::new(); count];
        for component in graph.components() {
            for &index in &component {
                interfaces[index] = graph.interface_digest(bundle, index, &BTreeMap::new());
            }
            for &index in &component {
                let package = &graph.packages[index];
                let manifest = package.root.as_deref().and_then(|root| store.manifest(root));
                let source = graph.source_digest(bundle, index, manifest.as_deref());
                let dependencies = package
                    .dependencies
                    .iter()
                    .map(|&dependency| {
                        (
                            graph.packages[dependency].identity.clone(),
                            interfaces[dependency].clone(),
                        )
                    })
                    .collect::<Vec<_>>();
                keys[index] =
                    identity::package_check_key(&compiler, &target, &package.identity, &source, &dependencies);
            }
        }
        let module_identity = (0..bundle.modules.len())
            .map(|module| ledger.module_identity(module))
            .collect::<Vec<_>>();
        let comptime = bundle
            .modules
            .iter()
            .map(|module| module.items.iter().any(sealed_body))
            .collect::<Vec<_>>();
        // Only a dependency package's reports are summarized away from the
        // importer's output (the rule `summarize_dependency_reports` applies:
        // a module under a dependency root), so only dependencies and Core
        // are sealed.
        let eligible = graph
            .packages
            .iter()
            .map(|package| {
                if package.modules.contains(&bundle.entry) {
                    return Some("the checked package itself".to_string());
                }
                let dependency = package.kind == PackageKind::Core
                    || package.modules.iter().all(|&module| {
                        let path = &bundle.modules[module].path;
                        bundle.dep_roots.values().any(|root| path.starts_with(root))
                    });
                if !dependency {
                    return Some("not a dependency package".to_string());
                }
                if package.modules.iter().any(|&module| module_identity[module].is_none()) {
                    return Some("a module has no stable identity".to_string());
                }
                None
            })
            .collect::<Vec<_>>();
        let mut plan = Self {
            store,
            graph,
            keys,
            eligible,
            sealed: vec![false; count],
            module_identity,
            comptime,
            modules: HashMap::new(),
            outputs: HashMap::new(),
        };
        let aliases = plan.identity_aliases(ledger, bundle);
        for index in 0..count {
            if plan.eligible[index].is_some() {
                continue;
            }
            if let Some(modules) = plan.load(index, &aliases) {
                plan.sealed[index] = true;
                plan.modules.extend(modules);
            }
        }
        // S3R: every module that is checked here may reuse its items, except
        // Core's: Core is sealed whole once checked, and its keys change only
        // with the compiler. Keys read the declarations as loaded, so they are
        // computed before the sealed bodies are erased.
        let checked = (0..bundle.modules.len())
            .filter(|module| !plan.modules.contains_key(module))
            .filter(|&module| !plan.comptime[module] && plan.module_identity[module].is_some())
            .filter(|&module| {
                plan.graph.package_of(module).map_or(true, |package| {
                    plan.graph.packages[package].kind != PackageKind::Core
                })
            })
            .collect::<Vec<_>>();
        let package_mates = (0..bundle.modules.len())
            .map(|module| {
                plan.graph
                    .package_of(module)
                    .map(|package| plan.graph.packages[package].modules.clone())
                    .unwrap_or_else(|| vec![module])
            })
            .collect::<Vec<_>>();
        super::ItemReuse::begin(
            plan.store.clone(),
            bundle,
            ledger,
            &format!("{compiler}\u{0}{target}"),
            &checked,
            &package_mates,
            &plan.module_identity,
            aliases,
        );
        for &module in plan.modules.keys() {
            erase_bodies(&mut bundle.modules[module].items);
        }
        Some(plan)
    }

    /// Module identity to this bundle's loader alias.
    fn identity_aliases(
        &self,
        ledger: &jet_foundation::Names::NameLedger,
        bundle: &ProgramBundle,
    ) -> HashMap<String, String> {
        self.module_identity
            .iter()
            .enumerate()
            .filter_map(|(module, identity)| {
                let alias = ledger.module_alias(module).unwrap_or(&bundle.modules[module].alias);
                identity.clone().map(|identity| (identity, alias.to_string()))
            })
            .collect()
    }

    fn load(&self, index: usize, aliases: &HashMap<String, String>) -> Option<HashMap<usize, SealedModule>> {
        let package = &self.graph.packages[index];
        let check = self.store.load(PKG_CHECK_SCHEMA, &self.keys[index])?;
        let check = RecordReader::open_schema(&check, PKG_CHECK_SCHEMA).ok()?;
        let header = check.element(SECTION_HEADER, 0).ok()?;
        if header.field("package").ok()?.as_str().ok()? != package.identity
            || header.field("key").ok()?.as_str().ok()? != self.keys[index]
        {
            return None;
        }
        let items_key = header.field("items").ok()?.as_str().ok()?.to_string();
        let items = self.store.load(ITEMS_SCHEMA, &items_key)?;
        let items = RecordReader::open_schema(&items, ITEMS_SCHEMA).ok()?;
        let by_identity = package
            .modules
            .iter()
            .map(|&module| (self.module_identity[module].clone().unwrap_or_default(), module))
            .collect::<HashMap<_, _>>();
        let mut modules = HashMap::new();
        for value in items.elements(SECTION_MODULES).ok()? {
            let identity = value.field("module").ok()?.as_str().ok()?;
            let module = *by_identity.get(identity)?;
            modules.insert(
                module,
                SealedModule {
                    summaries: Vec::new(),
                    address_taken: value.field("address_taken").ok()?.str_list().ok()?,
                    inferred_infallible: value
                        .field("inferred_infallible")
                        .ok()?
                        .str_list()
                        .ok()?
                        .into_iter()
                        .collect(),
                    exact_int: value.field("exact_int").ok()?.as_bool().ok()?,
                },
            );
        }
        if modules.len() != package.modules.len() {
            return None;
        }
        for value in items.elements(SECTION_ITEMS).ok()? {
            let identity = value.field("module").ok()?.as_str().ok()?;
            let module = *by_identity.get(identity)?;
            let key = value.field("key").ok()?.as_str().ok()?.to_string();
            let mut summary = decode_summary(value.field("summary").ok()?).ok()?;
            map_edges(&mut summary, &|edge| to_alias(edge, aliases));
            modules.get_mut(&module)?.summaries.push((key, summary));
        }
        Some(modules)
    }

    /// Module paths of sealed modules, for dropping their reports.
    pub(super) fn sealed_paths(&self, bundle: &ProgramBundle) -> HashSet<String> {
        self.modules
            .keys()
            .map(|&module| bundle.modules[module].path.to_string_lossy().into_owned())
            .collect()
    }

    /// Publish every clean dependency package that was checked, and report
    /// every package's row to the session.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn finish(
        self,
        bundle: &ProgramBundle,
        ledger: &jet_foundation::Names::NameLedger,
        public_summaries: &HashMap<String, EffectSummary>,
        inferred: &[HashSet<String>],
        exact_int: &[bool],
        diagnostics: &[crate::Diagnostics::Diagnostic],
    ) {
        let aliases = self.identity_aliases(ledger, bundle);
        let identities = aliases
            .iter()
            .map(|(identity, alias)| (alias.clone(), identity.clone()))
            .collect::<HashMap<_, _>>();
        let module_of_path = bundle
            .modules
            .iter()
            .enumerate()
            .map(|(module, loaded)| (loaded.path.to_string_lossy().into_owned(), module))
            .collect::<HashMap<_, _>>();
        let mut reported = vec![false; self.graph.packages.len()];
        let mut unattributed = false;
        for diagnostic in diagnostics {
            let core_lint = diagnostic.severity == crate::Diagnostics::Severity::Lint;
            let error = diagnostic.severity == crate::Diagnostics::Severity::Error;
            // A report without an origin renders against the entry file, so
            // it belongs to the checked package, which is never recorded.
            let Some(origin) = diagnostic.origin() else {
                continue;
            };
            match module_of_path
                .get(&origin.path)
                .and_then(|&module| self.graph.package_of(module))
            {
                Some(package) => {
                    let core = self.graph.packages[package].kind == PackageKind::Core;
                    if error || (core && !core_lint) {
                        reported[package] = true;
                    }
                }
                None => unattributed |= error,
            }
        }
        let mut rows = Vec::with_capacity(self.graph.packages.len());
        for (index, package) in self.graph.packages.iter().enumerate() {
            let (reuse, detail) = if self.sealed[index] {
                (PackageReuse::Reused, "green:key".to_string())
            } else if let Some(reason) = &self.eligible[index] {
                (PackageReuse::Checked, reason.clone())
            } else if reported[index] || unattributed {
                (PackageReuse::Checked, "its check reported diagnostics".to_string())
            } else if package.modules.iter().any(|&module| self.comptime[module]) {
                (PackageReuse::Checked, "a body evaluates at compile time".to_string())
            } else if package.modules.iter().any(|&module| {
                self.outputs.get(&module).is_some_and(|outputs| outputs.discovered_inputs)
            }) {
                (PackageReuse::Checked, "its check discovered inputs".to_string())
            } else {
                match self.publish(index, bundle, ledger, &identities, public_summaries, inferred, exact_int) {
                    Ok(()) => (PackageReuse::CheckedPublished, "red".to_string()),
                    Err(reason) => (PackageReuse::Checked, reason),
                }
            };
            // S3R: how the package's items fared inside the check.
            let detail = match super::ItemReuse::package_detail(&package.modules) {
                Some(items) => format!("{detail}; {items}"),
                None => detail,
            };
            rows.push(PackageReuseRow {
                identity: package.identity.clone(),
                key: self.keys[index].clone(),
                reuse,
                modules: package.modules.len(),
                detail,
            });
        }
        super::ItemReuse::end();
        if let Some(state) = session().as_mut() {
            state.rows.extend(rows);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn publish(
        &self,
        index: usize,
        bundle: &ProgramBundle,
        ledger: &jet_foundation::Names::NameLedger,
        identities: &HashMap<String, String>,
        public_summaries: &HashMap<String, EffectSummary>,
        inferred: &[HashSet<String>],
        exact_int: &[bool],
    ) -> Result<(), String> {
        let package = &self.graph.packages[index];
        let mut modules = Vec::new();
        let mut items = Vec::new();
        for &module in &package.modules {
            let identity = self.module_identity[module].clone().unwrap_or_default();
            let alias = ledger.module_alias(module).unwrap_or(&bundle.modules[module].alias);
            let prefix = format!("{alias}::");
            let mut nodes = public_summaries
                .iter()
                .filter_map(|(key, summary)| key.strip_prefix(&prefix).map(|local| (local, summary)))
                .collect::<Vec<_>>();
            nodes.sort_by(|left, right| left.0.cmp(right.0));
            for (local, summary) in nodes {
                let mut summary = summary.clone();
                map_edges(&mut summary, &|edge| to_identity(edge, identities));
                let encoded = encode_summary(&summary)?;
                let digest = record_digest(
                    ITEM_SUMMARY_SCHEMA,
                    &[RecordSection::new(1, vec![encoded.clone()])],
                )
                .map_err(|error| error.to_string())?;
                items.push(record_map([
                    ("module", RecordValue::str(identity.as_str())),
                    ("key", RecordValue::str(local)),
                    ("summary_digest", RecordValue::Digest(digest)),
                    ("summary", encoded),
                ]));
            }
            let mut address_taken = self
                .outputs
                .get(&module)
                .map(|outputs| outputs.address_taken.iter().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            address_taken.sort();
            let mut infallible = inferred
                .get(module)
                .map(|names| names.iter().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            infallible.sort();
            modules.push(record_map([
                ("module", RecordValue::str(identity)),
                ("address_taken", RecordValue::list_of_str(&address_taken)),
                ("inferred_infallible", RecordValue::list_of_str(&infallible)),
                ("exact_int", RecordValue::Bool(exact_int.get(module).copied().unwrap_or(false))),
            ]));
        }
        let header = record_map([("package", RecordValue::str(package.identity.as_str()))]);
        let items = encode_record(
            ITEMS_SCHEMA,
            &[
                RecordSection::new(SECTION_HEADER, vec![header]),
                RecordSection::new(SECTION_MODULES, modules),
                RecordSection::new(SECTION_ITEMS, items),
            ],
        )
        .map_err(|error| error.to_string())?;
        let items_key = jet_foundation::SHA256::sha256_hex(&items);
        self.store.publish(ITEMS_SCHEMA, &items_key, &items);
        let check = encode_record(
            PKG_CHECK_SCHEMA,
            &[RecordSection::new(
                SECTION_HEADER,
                vec![record_map([
                    ("package", RecordValue::str(package.identity.as_str())),
                    ("key", RecordValue::str(self.keys[index].as_str())),
                    ("items", RecordValue::Str(items_key)),
                ])],
            )],
        )
        .map_err(|error| error.to_string())?;
        self.store.publish(PKG_CHECK_SCHEMA, &self.keys[index], &check);
        Ok(())
    }
}

// ------------------------------------------------------------ edge identity

/// `alias::rest` becomes `{module identity}::rest`; other edges (bare names
/// the bundle could not resolve, the panic sentinel) are kept as written.
pub(super) fn to_identity(edge: &str, identities: &HashMap<String, String>) -> String {
    match edge.split_once("::") {
        Some((alias, rest)) => match identities.get(alias) {
            Some(identity) => format!("{{{identity}}}::{rest}"),
            None => edge.to_string(),
        },
        None => edge.to_string(),
    }
}

pub(super) fn to_alias(edge: &str, aliases: &HashMap<String, String>) -> String {
    let Some(stable) = edge.strip_prefix('{') else {
        return edge.to_string();
    };
    match stable.split_once("}::") {
        Some((identity, rest)) => match aliases.get(identity) {
            Some(alias) => format!("{alias}::{rest}"),
            None => edge.to_string(),
        },
        None => edge.to_string(),
    }
}

/// Every call-graph reference a summary carries (the same fields
/// `qualified_effect_facts` resolves).
pub(super) fn map_edges(summary: &mut EffectSummary, map: &dyn Fn(&str) -> String) {
    let set = |edges: &BTreeSet<String>| edges.iter().map(|edge| map(edge)).collect::<BTreeSet<_>>();
    summary.edges = set(&summary.edges);
    for region in &mut summary.regions {
        region.edges = set(&region.edges);
    }
    for obligation in &mut summary.callback_obligations {
        obligation.edges = set(&obligation.edges);
    }
    for obligation in &mut summary.autodiff_obligations {
        obligation.target = map(&obligation.target);
    }
    for fact in &mut summary.discarded_results {
        fact.callee = map(&fact.callee);
    }
    for call in &mut summary.memory.calls {
        call.callee = map(&call.callee);
    }
}

// ------------------------------------------------------------ summary codec

fn span_value(span: Span) -> RecordValue {
    RecordValue::List(vec![
        RecordValue::Int(span.start as i64),
        RecordValue::Int(span.end as i64),
    ])
}

fn span_of(value: &RecordValue) -> Result<Span, RecordError> {
    match value.as_list()? {
        [start, end] => Ok(Span::new(start.as_usize()?, end.as_usize()?)),
        _ => Err(RecordError::Malformed("span is not a pair".to_string())),
    }
}

fn strings(values: &BTreeSet<String>) -> RecordValue {
    RecordValue::list_of_str(values)
}

fn string_set(value: &RecordValue) -> Result<BTreeSet<String>, RecordError> {
    Ok(value.str_list()?.into_iter().collect())
}

fn count_value(value: Option<u64>) -> RecordValue {
    value.map_or(RecordValue::Null, |count| RecordValue::Int(count as i64))
}

fn count_of(value: &RecordValue) -> Result<Option<u64>, RecordError> {
    if value.is_null() {
        return Ok(None);
    }
    u64::try_from(value.as_int()?)
        .map(Some)
        .map_err(|_| RecordError::Malformed("negative count".to_string()))
}

fn text(value: &RecordValue, name: &str) -> Result<String, RecordError> {
    value.field(name)?.as_str().map(str::to_string)
}

fn list<T>(
    value: &RecordValue,
    name: &str,
    decode: impl Fn(&RecordValue) -> Result<T, RecordError>,
) -> Result<Vec<T>, RecordError> {
    value.field(name)?.as_list()?.iter().map(decode).collect()
}

/// Lossless encoding of one effect-graph node. Destructuring keeps the codec
/// exhaustive: a new summary field does not compile until it is carried.
pub(super) fn encode_summary(summary: &EffectSummary) -> Result<RecordValue, String> {
    let EffectSummary {
        direct,
        direct_spans,
        edges,
        maximal,
        maximal_span,
        unbounded_trait_dispatch,
        regions,
        authority_delegations,
        callback_obligations,
        autodiff_obligations,
        compute_calls,
        autodiff_safe_panic,
        autodiff_unsafe_panic,
        memory,
        discarded_results,
        failure_direct,
    } = summary;
    let MemorySummary {
        events,
        open_dispatches,
        regions: memory_regions,
        unbounded_control,
        calls,
    } = memory;
    if !memory_regions.is_empty() {
        return Err("a memory policy region is not recordable yet".to_string());
    }
    let direct_spans = direct_spans
        .iter()
        .collect::<BTreeMap<_, _>>()
        .into_iter()
        .map(|(effect, span)| {
            record_map([("effect", RecordValue::str(effect.as_str())), ("span", span_value(*span))])
        })
        .collect();
    Ok(record_map([
        ("direct", strings(direct)),
        ("direct_spans", RecordValue::List(direct_spans)),
        ("edges", strings(edges)),
        ("maximal", RecordValue::Bool(*maximal)),
        ("maximal_span", maximal_span.map_or(RecordValue::Null, span_value)),
        ("unbounded_trait_dispatch", RecordValue::Bool(*unbounded_trait_dispatch)),
        (
            "regions",
            RecordValue::List(
                regions
                    .iter()
                    .map(|region| {
                        record_map([
                            ("caps", strings(&region.caps)),
                            ("direct", strings(&region.direct)),
                            ("edges", strings(&region.edges)),
                            ("maximal", RecordValue::Bool(region.maximal)),
                            ("caps_span", span_value(region.caps_span)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "authority_delegations",
            RecordValue::List(
                authority_delegations
                    .iter()
                    .map(|delegation| {
                        record_map([
                            ("scope_span", span_value(delegation.scope_span)),
                            ("binding", RecordValue::str(delegation.binding.as_str())),
                            ("resource", RecordValue::str(delegation.resource.as_str())),
                            ("operation", RecordValue::str(delegation.operation.as_str())),
                            ("span", span_value(delegation.span)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "callback_obligations",
            RecordValue::List(
                callback_obligations
                    .iter()
                    .map(|obligation| {
                        record_map([
                            ("bound", strings(&obligation.bound)),
                            ("direct", strings(&obligation.direct)),
                            ("edges", strings(&obligation.edges)),
                            ("maximal", RecordValue::Bool(obligation.maximal)),
                            ("span", span_value(obligation.span)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "autodiff_obligations",
            RecordValue::List(
                autodiff_obligations
                    .iter()
                    .map(|obligation| {
                        record_map([
                            ("method", RecordValue::str(obligation.method.as_str())),
                            ("target", RecordValue::str(obligation.target.as_str())),
                            ("span", span_value(obligation.span)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "compute_calls",
            RecordValue::List(
                compute_calls
                    .iter()
                    .map(|call| {
                        record_map([
                            ("method", RecordValue::str(call.method.as_str())),
                            ("span", span_value(call.span)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("autodiff_safe_panic", RecordValue::Bool(*autodiff_safe_panic)),
        ("autodiff_unsafe_panic", RecordValue::Bool(*autodiff_unsafe_panic)),
        (
            "memory_events",
            RecordValue::List(
                events
                    .iter()
                    .map(|event| {
                        let (kind, bytes) = match event.kind {
                            MemoryEventKind::Allocation => ("alloc", RecordValue::Null),
                            MemoryEventKind::RetainRelease => ("rc", RecordValue::Null),
                            MemoryEventKind::ArenaBytes(bytes) => ("arena", count_value(bytes)),
                        };
                        record_map([
                            ("kind", RecordValue::str(kind)),
                            ("bytes", bytes),
                            ("span", span_value(event.span)),
                            ("source", RecordValue::str(event.source.as_str())),
                            ("operation", RecordValue::str(event.operation.as_str())),
                            ("provenance", RecordValue::str(event.provenance.as_str())),
                            ("executions", count_value(event.executions)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "memory_open_dispatches",
            RecordValue::List(
                open_dispatches
                    .iter()
                    .map(|dispatch| {
                        record_map([
                            ("span", span_value(dispatch.span)),
                            ("source", RecordValue::str(dispatch.source.as_str())),
                            ("reason", RecordValue::str(dispatch.reason.as_str())),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "memory_unbounded_control",
            RecordValue::List(unbounded_control.iter().copied().map(span_value).collect()),
        ),
        (
            "memory_calls",
            RecordValue::List(
                calls
                    .iter()
                    .map(|call| {
                        record_map([
                            ("callee", RecordValue::str(call.callee.as_str())),
                            ("span", span_value(call.span)),
                            ("source", RecordValue::str(call.source.as_str())),
                            ("executions", count_value(call.executions)),
                        ])
                    })
                    .collect(),
            ),
        ),
        (
            "discarded_results",
            RecordValue::List(
                discarded_results
                    .iter()
                    .map(|fact| {
                        record_map([
                            ("callee", RecordValue::str(fact.callee.as_str())),
                            ("callee_name", RecordValue::str(fact.callee_name.as_str())),
                            ("call_text", RecordValue::str(fact.call_text.as_str())),
                            ("span", span_value(fact.span)),
                            ("failure_open", RecordValue::Bool(fact.failure_open)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("failure_direct", RecordValue::Bool(*failure_direct)),
    ]))
}

pub(super) fn decode_summary(value: &RecordValue) -> Result<EffectSummary, RecordError> {
    let flag = |name: &str| value.field(name)?.as_bool();
    let maximal_span = value.field("maximal_span")?;
    Ok(EffectSummary {
        direct: string_set(value.field("direct")?)?,
        direct_spans: list(value, "direct_spans", |entry| {
            Ok((text(entry, "effect")?, span_of(entry.field("span")?)?))
        })?
        .into_iter()
        .collect(),
        edges: string_set(value.field("edges")?)?,
        maximal: flag("maximal")?,
        maximal_span: if maximal_span.is_null() {
            None
        } else {
            Some(span_of(maximal_span)?)
        },
        unbounded_trait_dispatch: flag("unbounded_trait_dispatch")?,
        regions: list(value, "regions", |entry| {
            Ok(RegionSummary {
                caps: string_set(entry.field("caps")?)?,
                direct: string_set(entry.field("direct")?)?,
                edges: string_set(entry.field("edges")?)?,
                maximal: entry.field("maximal")?.as_bool()?,
                caps_span: span_of(entry.field("caps_span")?)?,
            })
        })?,
        authority_delegations: list(value, "authority_delegations", |entry| {
            Ok(AuthorityDelegation {
                scope_span: span_of(entry.field("scope_span")?)?,
                binding: text(entry, "binding")?,
                resource: text(entry, "resource")?,
                operation: text(entry, "operation")?,
                span: span_of(entry.field("span")?)?,
            })
        })?,
        callback_obligations: list(value, "callback_obligations", |entry| {
            Ok(CallbackObligation {
                bound: string_set(entry.field("bound")?)?,
                direct: string_set(entry.field("direct")?)?,
                edges: string_set(entry.field("edges")?)?,
                maximal: entry.field("maximal")?.as_bool()?,
                span: span_of(entry.field("span")?)?,
            })
        })?,
        autodiff_obligations: list(value, "autodiff_obligations", |entry| {
            Ok(AutodiffObligation {
                method: text(entry, "method")?,
                target: text(entry, "target")?,
                span: span_of(entry.field("span")?)?,
            })
        })?,
        compute_calls: list(value, "compute_calls", |entry| {
            Ok(ComputeCallFact {
                method: text(entry, "method")?,
                span: span_of(entry.field("span")?)?,
            })
        })?,
        autodiff_safe_panic: flag("autodiff_safe_panic")?,
        autodiff_unsafe_panic: flag("autodiff_unsafe_panic")?,
        memory: MemorySummary {
            events: list(value, "memory_events", |entry| {
                let kind = match entry.field("kind")?.as_str()? {
                    "alloc" => MemoryEventKind::Allocation,
                    "rc" => MemoryEventKind::RetainRelease,
                    "arena" => MemoryEventKind::ArenaBytes(count_of(entry.field("bytes")?)?),
                    other => {
                        return Err(RecordError::Malformed(format!("memory event kind `{other}`")))
                    }
                };
                Ok(MemoryEvent {
                    kind,
                    span: span_of(entry.field("span")?)?,
                    source: text(entry, "source")?,
                    operation: text(entry, "operation")?,
                    provenance: text(entry, "provenance")?,
                    executions: count_of(entry.field("executions")?)?,
                })
            })?,
            open_dispatches: list(value, "memory_open_dispatches", |entry| {
                Ok(OpenMemoryDispatch {
                    span: span_of(entry.field("span")?)?,
                    source: text(entry, "source")?,
                    reason: text(entry, "reason")?,
                })
            })?,
            regions: Vec::new(),
            unbounded_control: list(value, "memory_unbounded_control", span_of)?,
            calls: list(value, "memory_calls", |entry| {
                Ok(MemoryCall {
                    callee: text(entry, "callee")?,
                    span: span_of(entry.field("span")?)?,
                    source: text(entry, "source")?,
                    executions: count_of(entry.field("executions")?)?,
                })
            })?,
        },
        discarded_results: list(value, "discarded_results", |entry| {
            Ok(DiscardedResultFact {
                callee: text(entry, "callee")?,
                callee_name: text(entry, "callee_name")?,
                call_text: text(entry, "call_text")?,
                span: span_of(entry.field("span")?)?,
                failure_open: entry.field("failure_open")?.as_bool()?,
            })
        })?,
        failure_direct: flag("failure_direct")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_round_trips_with_stable_edges() {
        let summary = EffectSummary {
            direct: ["IO".to_string()].into_iter().collect(),
            direct_spans: HashMap::from([("IO".to_string(), Span::new(3, 9))]),
            edges: ["util::helper".to_string(), "__jet_panic__".to_string(), "bare".to_string()]
                .into_iter()
                .collect(),
            maximal_span: Some(Span::new(1, 2)),
            memory: MemorySummary {
                events: vec![MemoryEvent::new(MemoryEventKind::ArenaBytes(Some(64)), Span::new(4, 5), "arena")],
                calls: vec![MemoryCall {
                    callee: "util::helper".to_string(),
                    span: Span::new(6, 7),
                    source: "a.jet".to_string(),
                    executions: None,
                }],
                ..Default::default()
            },
            failure_direct: true,
            ..Default::default()
        };
        let identities = HashMap::from([("util".to_string(), "pkg::Source/Util.jet".to_string())]);
        let mut stable = summary.clone();
        map_edges(&mut stable, &|edge| to_identity(edge, &identities));
        assert!(stable.edges.contains("{pkg::Source/Util.jet}::helper"));
        let decoded = decode_summary(&encode_summary(&stable).unwrap()).unwrap();
        // A later bundle names the same module with another loader alias.
        let aliases = HashMap::from([("pkg::Source/Util.jet".to_string(), "util_2".to_string())]);
        let mut seeded = decoded;
        map_edges(&mut seeded, &|edge| to_alias(edge, &aliases));
        assert_eq!(
            seeded.edges,
            ["util_2::helper", "__jet_panic__", "bare"].iter().map(|s| s.to_string()).collect()
        );
        assert_eq!(seeded.memory.calls[0].callee, "util_2::helper");
        assert_eq!(seeded.memory.events, summary.memory.events);
        assert_eq!(seeded.direct_spans, summary.direct_spans);
        assert_eq!(seeded.maximal_span, summary.maximal_span);
        assert!(seeded.failure_direct);
    }
}
