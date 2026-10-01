//! #2517 stage S3R: item reuse inside a checked package.
//!
//! In a package-record session (`PackageSeal`), every function and method
//! body of a checked module has an item check key: the module's reads (the
//! check environment, and the declarations and templates of every module
//! its package and its imports reach), the item's identity, and its
//! span-free syntax. A body whose key has a stored record is not checked
//! again. The record's outputs (effect summaries, address-taken names, name
//! references, import-alias uses, exact-`Int` use) are seeded where the
//! check would have produced them. Callers read signatures, not effect
//! rows, so a body edit that changes a summary keeps its callers green; the
//! effect, failure and post phases solve again over the seeded summaries
//! (item-level early cutoff).
//!
//! A body is recorded only when its check produced no diagnostic, pending
//! diagnostic, publication, discovered input, or structure fact. The record
//! carries the checked-body delta (`BodyDelta`): the parts of the function
//! the checker rewrote, as they were after the check. A reuse installs them
//! on the parsed function, so the bundle after a reuse is the bundle a cold
//! check builds and every later phase that walks bodies sees the same input.
//! A delta is recorded only when installing it on the parsed function
//! rebuilds the checked function exactly; the key covers the item's source
//! text, so its spans are exact, and the record's text and delta digests are
//! verified again when it is replayed.
//!
//! Spans in a record are relative to the item, so an edit that moves an
//! item does not invalidate it. A reference into another declaration is
//! anchored to the nearest callable of the defining module and must name
//! the same source text when replayed; otherwise the body is checked again.

use super::BodyProducts;
use super::PackageSeal::{
    decode_summary, encode_summary, map_edges, to_alias, to_identity, PackageRecordStore,
};
use crate::Diagnostics::{Diagnostic, Span};
use crate::Sema::Effects::EffectSummary;
use crate::AST::{Func, Item, LoadedModule, ProgramBundle};
use jet_foundation::Names::{NameLedger, NameReference};
use jet_foundation::PackageIdentity::KeyHasher;
use jet_foundation::RecordCodec::{
    encode_record, record_map, RecordReader, RecordSection, RecordValue,
};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};

/// Schema of one item check record (an action record under the item key).
const ITEM_CHECK_SCHEMA: &str = "jet.item-check/v1";
const SECTION_ITEM: u64 = 1;

/// A checked module whose bodies may be reused.
struct ModuleItems {
    /// Digest of everything a body of this module reads besides itself.
    reads: String,
    /// Loader display name, the prefix of this module's body cache keys.
    display: String,
}

/// One module's text and callable spans, for anchoring references.
struct Anchors {
    source: String,
    /// Spans of every function and method, ordered by start.
    callables: Vec<(usize, usize)>,
}

#[derive(Default)]
struct Tally {
    reused: usize,
    checked: Vec<String>,
    /// Why checked bodies could not be recorded, with counts.
    unrecordable: std::collections::BTreeMap<&'static str, usize>,
}

struct ItemState {
    store: Arc<dyn PackageRecordStore>,
    modules: HashMap<usize, ModuleItems>,
    anchors: Vec<Anchors>,
    /// The ledger path of every module (the key references use).
    paths: Vec<Option<String>>,
    by_path: HashMap<String, usize>,
    identity: Vec<Option<String>>,
    by_identity: HashMap<String, usize>,
    /// Loader alias to module identity, and back.
    identities: HashMap<String, String>,
    aliases: HashMap<String, String>,
    tally: Mutex<HashMap<usize, Tally>>,
    /// Store work of this check, done in one transaction at `end`: the keys
    /// of reused records and the records to publish. One store call per
    /// item costs a lock and a synced journal write each.
    pending: Mutex<Pending>,
}

#[derive(Default)]
struct Pending {
    touched: Vec<String>,
    records: Vec<(String, Vec<u8>)>,
}

static STATE: Mutex<Option<Arc<ItemState>>> = Mutex::new(None);

fn current() -> Option<Arc<ItemState>> {
    STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Plan item reuse for the modules of one sealed-aware check. `checked`
/// are the modules this check body-checks; `environment` is the compiler
/// identity and check environment every key starts from.
#[allow(clippy::too_many_arguments)]
pub(super) fn begin(
    store: Arc<dyn PackageRecordStore>,
    bundle: &ProgramBundle,
    ledger: &NameLedger,
    environment: &str,
    checked: &[usize],
    package_mates: &[Vec<usize>],
    identity: &[Option<String>],
    aliases: HashMap<String, String>,
) {
    let displays = bundle
        .modules
        .iter()
        .enumerate()
        .map(|(index, module)| (module.display.clone(), index))
        .collect::<HashMap<_, _>>();
    let dependencies = super::incremental_module_dependencies_with_ledger(bundle, ledger);
    let declarations = bundle
        .modules
        .iter()
        .map(|module| {
            let mut hasher = KeyHasher::new(ITEM_CHECK_SCHEMA);
            hasher
                .field(&jet_foundation::SHA256::sha256_hex(
                    &super::incremental_module_interface(module),
                ))
                .field(&template_digest(module));
            hasher.finish()
        })
        .collect::<Vec<_>>();
    let mut modules = HashMap::new();
    for &module in checked {
        // A body reads its own package's namespace (D-MOD-CYCLE1) and every
        // module its package imports, transitively.
        let mut reads = BTreeSet::new();
        let mut pending = package_mates[module].clone();
        pending.push(module);
        while let Some(next) = pending.pop() {
            if !reads.insert(next) {
                continue;
            }
            if let Some(imported) = dependencies.get(&bundle.modules[next].display) {
                pending.extend(imported.iter().filter_map(|display| displays.get(display).copied()));
            }
        }
        let mut hasher = KeyHasher::new(ITEM_CHECK_SCHEMA);
        hasher.field(environment);
        for &read in &reads {
            hasher
                .field(
                    identity[read]
                        .as_deref()
                        .unwrap_or(bundle.modules[read].display.as_str()),
                )
                .field(&declarations[read]);
        }
        modules.insert(
            module,
            ModuleItems {
                reads: hasher.finish(),
                display: bundle.modules[module].display.clone(),
            },
        );
    }
    let paths = (0..bundle.modules.len())
        .map(|module| ledger.module_path(module).map(str::to_string))
        .collect::<Vec<_>>();
    let by_path = paths
        .iter()
        .enumerate()
        .filter_map(|(module, path)| path.clone().map(|path| (path, module)))
        .collect();
    let by_identity = identity
        .iter()
        .enumerate()
        .filter_map(|(module, identity)| identity.clone().map(|identity| (identity, module)))
        .collect();
    let anchors = bundle
        .modules
        .iter()
        .map(|module| Anchors {
            source: module.source.clone(),
            callables: callable_spans(&module.items),
        })
        .collect();
    let identities = aliases
        .iter()
        .map(|(identity, alias)| (alias.clone(), identity.clone()))
        .collect();
    *STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::new(ItemState {
        store,
        modules,
        anchors,
        paths,
        by_path,
        identity: identity.to_vec(),
        by_identity,
        identities,
        aliases,
        tally: Mutex::new(HashMap::new()),
        pending: Mutex::new(Pending::default()),
    }));
}

/// End the item plan of the current check and flush its store work.
pub(super) fn end() {
    let state = STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).take();
    if let Some(state) = state {
        let pending = std::mem::take(
            &mut *state.pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
        );
        if !pending.touched.is_empty() || !pending.records.is_empty() {
            state.store.publish_batch(ITEM_CHECK_SCHEMA, &pending.touched, pending.records);
        }
    }
}

/// Whether bodies of `module` take part in item reuse.
pub(super) fn tracks(module: usize) -> bool {
    current().is_some_and(|state| state.modules.contains_key(&module))
}

/// The store-log summary of one package's items, or `None` when none of its
/// modules took part.
pub(super) fn package_detail(modules: &[usize]) -> Option<String> {
    let state = current()?;
    let tally = state.tally.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut seen = false;
    let mut reused = 0;
    let mut unrecordable = std::collections::BTreeMap::<&'static str, usize>::new();
    let mut checked = Vec::new();
    for module in modules {
        if let Some(entry) = tally.get(module) {
            seen = true;
            reused += entry.reused;
            for (reason, count) in &entry.unrecordable {
                *unrecordable.entry(*reason).or_default() += count;
            }
            checked.extend(entry.checked.iter().cloned());
        }
    }
    if !seen {
        return None;
    }
    checked.sort();
    let names = if !checked.is_empty() && checked.len() <= 8 {
        format!(" [{}]", checked.join(", "))
    } else {
        String::new()
    };
    let total = unrecordable.values().sum::<usize>();
    let reasons = if unrecordable.is_empty() {
        String::new()
    } else {
        let reasons = unrecordable
            .iter()
            .map(|(reason, count)| format!("{reason}: {count}"))
            .collect::<Vec<_>>();
        format!(" ({})", reasons.join(", "))
    };
    Some(format!(
        "items: {reused} reused, {} checked{names}, {total} not recordable{reasons}",
        checked.len()
    ))
}

/// Replay the stored check of one body when its key has a record: install
/// the stored checked-body delta on `function` and seed its outputs. Returns
/// `false` (nothing installed or seeded) on a miss.
#[allow(clippy::too_many_arguments)]
pub(super) fn reuse(
    module: usize,
    cache_key: &str,
    function: &mut Func,
    owner: Option<&str>,
    raw_protocol_return: bool,
    ledger: &mut NameLedger,
    products: &mut BodyProducts,
) -> bool {
    let Some(state) = current() else {
        return false;
    };
    let Some(items) = state.modules.get(&module) else {
        return false;
    };
    let Some(text) = state.text_digest(module, function.span) else {
        return false;
    };
    let key = item_key(&items.reads, cache_key, function, &text, owner, raw_protocol_return);
    let Some(seed) = state
        .store
        .load_untouched(ITEM_CHECK_SCHEMA, &key)
        .and_then(|bytes| state.decode(module, function.span, &text, &bytes))
    else {
        return false;
    };
    if super::BodyDelta::apply(function, &seed.body).is_none() {
        return false;
    }
    products.summaries.extend(seed.summaries);
    products.addr_taken.extend(seed.addr_taken);
    products.uses_exact_int |= seed.exact_int;
    for (start, end, reference) in seed.references {
        if let Some(path) = &state.paths[module] {
            ledger.record_reference(path.clone(), start, end, reference);
        }
    }
    for (alias_module, span) in seed.alias_uses {
        ledger.record_alias_use(alias_module, span);
    }
    state.defer(|pending| pending.touched.push(key));
    state.count(module, |tally| tally.reused += 1);
    true
}

/// Record one body's check for the next run. `pristine` is the function as
/// it was before `checked` went through the checker.
#[allow(clippy::too_many_arguments)]
pub(super) fn record(
    module: usize,
    cache_key: &str,
    pristine: Func,
    checked: &Func,
    owner: Option<&str>,
    raw_protocol_return: bool,
    diagnostics: &[Diagnostic],
    products: &BodyProducts,
    ledger: &NameLedger,
) {
    let Some(state) = current() else {
        return;
    };
    let Some(items) = state.modules.get(&module) else {
        return;
    };
    let name = cache_key
        .strip_prefix(&format!("{}::", items.display))
        .unwrap_or(cache_key)
        .to_string();
    let reason = if !diagnostics.is_empty() {
        Some("diagnostics")
    } else if !products.pending_diagnostics.is_empty() {
        Some("pending diagnostics")
    } else if !products.devtools_publications.is_empty() {
        Some("publications")
    } else if !products.embed_inputs.is_empty() || products.ran_ct_evaluator {
        Some("compile-time evaluation")
    } else if !ledger.structure_facts().is_empty() {
        Some("structure facts")
    } else {
        None
    };
    let span = pristine.span;
    let recorded = match reason {
        Some(reason) => Err(reason),
        None => state
            .text_digest(module, span)
            .ok_or("item span outside its module")
            .and_then(|text| {
                let key =
                    item_key(&items.reads, cache_key, &pristine, &text, owner, raw_protocol_return);
                let delta = verified_delta(pristine, checked)?;
                let bytes = state.encode(module, cache_key, span, &text, &delta, products, ledger)?;
                Ok((key, bytes))
            }),
    };
    match recorded {
        Ok((key, bytes)) => {
            state.defer(|pending| pending.records.push((key, bytes)));
            state.count(module, |tally| tally.checked.push(name));
        }
        Err(reason) => state.count(module, |tally| {
            tally.checked.push(name);
            *tally.unrecordable.entry(reason).or_default() += 1;
        }),
    }
}

/// The checked-body delta of `checked`, when installing it on `pristine`
/// rebuilds `checked` exactly.
fn verified_delta(mut pristine: Func, checked: &Func) -> Result<Vec<u8>, &'static str> {
    let delta = super::BodyDelta::encode(&pristine, checked)?;
    super::BodyDelta::apply(&mut pristine, &delta).ok_or("delta does not decode")?;
    if !same_debug(&pristine, checked) {
        return Err("delta does not rebuild the checked body");
    }
    Ok(delta)
}

/// Whether two values render the same `Debug` text. `expected` is rendered
/// once into a per-thread buffer and `actual` is streamed against it, stopping
/// at the first difference: no rendering is allocated per call.
pub(super) fn same_debug<T: std::fmt::Debug>(actual: &T, expected: &T) -> bool {
    use std::fmt::Write;
    struct Against<'a> {
        rest: &'a str,
    }
    impl Write for Against<'_> {
        fn write_str(&mut self, text: &str) -> std::fmt::Result {
            let rest = self.rest.strip_prefix(text).ok_or(std::fmt::Error)?;
            self.rest = rest;
            Ok(())
        }
    }
    thread_local! {
        static RENDERED: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    }
    RENDERED.with(|rendered| {
        let mut rendered = rendered.borrow_mut();
        rendered.clear();
        if write!(rendered, "{expected:?}").is_err() {
            return false;
        }
        let mut against = Against { rest: &rendered };
        write!(against, "{actual:?}").is_ok() && against.rest.is_empty()
    })
}

/// `text` is the digest of the item's source text: two parses of the same
/// text carry the same item-relative spans.
fn item_key(
    reads: &str,
    cache_key: &str,
    function: &Func,
    text: &str,
    owner: Option<&str>,
    raw_protocol_return: bool,
) -> String {
    let mut hasher = KeyHasher::new(ITEM_CHECK_SCHEMA);
    hasher
        .field(reads)
        .field(cache_key)
        .field(owner.unwrap_or(""))
        .field(if raw_protocol_return { "raw" } else { "plain" })
        .field(text)
        .field(&jet_foundation::SHA256::sha256_hex(
            &crate::CanonicalAST::canonical_fragment(function),
        ));
    hasher.finish()
}

/// Bodies that importers and callers may instantiate: generic and
/// `#Inline(Always)` functions, methods of generic types, and traits.
fn template_digest(module: &LoadedModule) -> String {
    fn add(bytes: &mut Vec<u8>, function: &Func, generic_owner: bool) {
        if generic_owner || !function.type_params.is_empty() || function.is_inline_always {
            bytes.extend(crate::CanonicalAST::canonical_fragment(function));
        }
    }
    let generic_types = module
        .items
        .iter()
        .filter_map(|item| match item {
            Item::Struct(definition) if !definition.type_params.is_empty() => {
                Some(definition.name.as_str())
            }
            Item::Enum(definition) if !definition.type_params.is_empty() => {
                Some(definition.name.as_str())
            }
            _ => None,
        })
        .collect::<HashSet<_>>();
    let mut bytes = Vec::new();
    for item in &module.items {
        match item {
            Item::Func(function) => add(&mut bytes, function, false),
            Item::Struct(definition) => {
                let generic = !definition.type_params.is_empty();
                for function in &definition.methods {
                    add(&mut bytes, function, generic);
                }
                for block in &definition.trait_impls {
                    for function in &block.methods {
                        add(&mut bytes, function, generic);
                    }
                }
            }
            Item::Enum(definition) => {
                let generic = !definition.type_params.is_empty();
                for function in &definition.methods {
                    add(&mut bytes, function, generic);
                }
                for block in &definition.trait_impls {
                    for function in &block.methods {
                        add(&mut bytes, function, generic);
                    }
                }
            }
            Item::Impl(implementation) => {
                let generic = generic_types.contains(implementation.type_name.as_str());
                for function in &implementation.methods {
                    add(&mut bytes, function, generic);
                }
            }
            Item::Trait(definition) => {
                bytes.extend(crate::CanonicalAST::canonical_fragment(definition))
            }
            _ => {}
        }
    }
    jet_foundation::SHA256::sha256_hex(&bytes)
}

fn callable_spans(items: &[Item]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    for item in items {
        let mut push = |function: &Func| spans.push((function.span.start, function.span.end));
        match item {
            Item::Func(function) => push(function),
            Item::Struct(definition) => {
                definition.methods.iter().for_each(&mut push);
                for block in &definition.trait_impls {
                    block.methods.iter().for_each(&mut push);
                }
            }
            Item::Enum(definition) => {
                definition.methods.iter().for_each(&mut push);
                for block in &definition.trait_impls {
                    block.methods.iter().for_each(&mut push);
                }
            }
            Item::Impl(implementation) => implementation.methods.iter().for_each(&mut push),
            _ => {}
        }
    }
    spans.sort_unstable();
    spans
}

/// Visit every span a summary carries; `false` when any visit refused.
fn each_span(summary: &mut EffectSummary, visit: &mut dyn FnMut(&mut Span) -> bool) -> bool {
    let mut ok = true;
    let mut go = |span: &mut Span| ok &= visit(span);
    summary.direct_spans.values_mut().for_each(&mut go);
    if let Some(span) = summary.maximal_span.as_mut() {
        go(span);
    }
    for region in &mut summary.regions {
        go(&mut region.caps_span);
    }
    for delegation in &mut summary.authority_delegations {
        go(&mut delegation.scope_span);
        go(&mut delegation.span);
    }
    for obligation in &mut summary.callback_obligations {
        go(&mut obligation.span);
    }
    for obligation in &mut summary.autodiff_obligations {
        go(&mut obligation.span);
    }
    for call in &mut summary.compute_calls {
        go(&mut call.span);
    }
    for event in &mut summary.memory.events {
        go(&mut event.span);
    }
    for dispatch in &mut summary.memory.open_dispatches {
        go(&mut dispatch.span);
    }
    summary.memory.unbounded_control.iter_mut().for_each(&mut go);
    for call in &mut summary.memory.calls {
        go(&mut call.span);
    }
    for fact in &mut summary.discarded_results {
        go(&mut fact.span);
    }
    ok
}

/// Where a referenced declaration sits, relative to something that moves
/// with it.
enum Anchor {
    /// Inside the referencing item; offset from its start.
    Own,
    /// Inside callable `n` of the defining module; offset from its start.
    In(usize),
    /// After callable `n` of the defining module; offset from its end.
    After(usize),
    /// Before every callable; offset from the start of the file.
    File,
}

/// Replayed outputs of one body.
struct Seed {
    summaries: Vec<(String, EffectSummary)>,
    addr_taken: Vec<String>,
    exact_int: bool,
    references: Vec<(usize, usize, NameReference)>,
    alias_uses: Vec<(usize, Span)>,
    /// The checked-body delta to install (`BodyDelta`).
    body: Vec<u8>,
}

fn int(value: usize) -> RecordValue {
    RecordValue::Int(value as i64)
}

impl ItemState {
    fn count(&self, module: usize, update: impl FnOnce(&mut Tally)) {
        let mut tally = self.tally.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        update(tally.entry(module).or_default());
    }

    fn defer(&self, update: impl FnOnce(&mut Pending)) {
        update(&mut self.pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));
    }

    fn text(&self, module: usize, start: usize, end: usize) -> Option<&str> {
        self.anchors.get(module)?.source.get(start..end)
    }

    /// Digest of the source text of the item at `span`.
    fn text_digest(&self, module: usize, span: Span) -> Option<String> {
        let text = self.text(module, span.start, span.end)?;
        Some(jet_foundation::SHA256::sha256_hex(text.as_bytes()))
    }

    /// The record of one clean body, or why an output cannot be carried
    /// faithfully. `item_text` is the item's text digest and `delta` its
    /// verified checked-body delta.
    #[allow(clippy::too_many_arguments)]
    fn encode(
        &self,
        module: usize,
        cache_key: &str,
        span: Span,
        item_text: &str,
        delta: &[u8],
        products: &BodyProducts,
        ledger: &NameLedger,
    ) -> Result<Vec<u8>, &'static str> {
        let (start, end) = (span.start, span.end);
        let own_path = self.paths[module].as_deref().ok_or("module has no ledger path")?;
        let mut keys = products.summaries.keys().collect::<Vec<_>>();
        keys.sort();
        let mut summaries = Vec::with_capacity(keys.len());
        for key in keys {
            let mut summary = products.summaries[key].clone();
            map_edges(&mut summary, &|edge| to_identity(edge, &self.identities));
            let inside = each_span(&mut summary, &mut |span| {
                if span.start < start || span.end > end {
                    return false;
                }
                *span = Span::new(span.start - start, span.end - start);
                true
            });
            if !inside {
                return Err("summary span outside the item");
            }
            summaries.push(record_map([
                ("key", RecordValue::str(key.as_str())),
                ("summary", encode_summary(&summary).map_err(|_| "summary not encodable")?),
            ]));
        }
        let mut sites = ledger.references().iter().collect::<Vec<_>>();
        sites.sort_by(|left, right| (left.0 .1, left.0 .2, &left.0 .0).cmp(&(right.0 .1, right.0 .2, &right.0 .0)));
        let mut references = Vec::with_capacity(sites.len());
        for ((source, site_start, site_end), reference) in sites {
            if source != own_path || *site_start < start || *site_end > end {
                return Err("reference site outside the item");
            }
            let def = reference.def_span;
            let def_module = *self
                .by_path
                .get(&reference.module_path)
                .ok_or("reference into an unknown module")?;
            let callables = &self.anchors[def_module].callables;
            let (anchor, offset) = if def_module == module && def.start >= start && def.end <= end {
                (Anchor::Own, def.start - start)
            } else {
                match callables.iter().rposition(|&(callable, _)| callable <= def.start) {
                    Some(index) if def.end <= callables[index].1 => {
                        (Anchor::In(index), def.start - callables[index].0)
                    }
                    Some(index) if def.start >= callables[index].1 => {
                        (Anchor::After(index), def.start - callables[index].1)
                    }
                    Some(_) => return Err("reference straddles a callable"),
                    None => (Anchor::File, def.start),
                }
            };
            let text = match anchor {
                Anchor::Own => String::new(),
                _ => {
                    let text = self
                        .text(def_module, def.start, def.end)
                        .ok_or("reference span outside its module")?;
                    if text.is_empty() {
                        return Err("empty reference span");
                    }
                    text.to_string()
                }
            };
            let (mode, index) = match anchor {
                Anchor::Own => ("own", 0),
                Anchor::In(index) => ("in", index),
                Anchor::After(index) => ("after", index),
                Anchor::File => ("file", 0),
            };
            references.push(record_map([
                ("start", int(site_start - start)),
                ("end", int(site_end - start)),
                ("kind", RecordValue::str(reference.kind.as_str())),
                (
                    "identity",
                    reference
                        .semantic_identity
                        .as_deref()
                        .map_or(RecordValue::Null, |identity| RecordValue::str(identity)),
                ),
                (
                    "module",
                    RecordValue::str(
                        self.identity[def_module].as_deref().ok_or("reference module has no identity")?,
                    ),
                ),
                ("mode", RecordValue::str(mode)),
                ("index", int(index)),
                ("offset", int(offset)),
                ("len", int(def.end - def.start)),
                ("text", RecordValue::Str(text)),
            ]));
        }
        let mut uses = ledger
            .alias_uses()
            .iter()
            .filter(|(alias_module, span)| !ledger.loader_alias_use(*alias_module, *span))
            .collect::<Vec<_>>();
        uses.sort_by_key(|(alias_module, span)| (*alias_module, span.start, span.end));
        let mut alias_uses = Vec::with_capacity(uses.len());
        for &(alias_module, span) in uses {
            let text = self
                .text(alias_module, span.start, span.end)
                .ok_or("alias span outside its module")?;
            if text.is_empty() {
                return Err("empty alias span");
            }
            alias_uses.push(record_map([
                (
                    "module",
                    RecordValue::str(
                        self.identity[alias_module].as_deref().ok_or("alias module has no identity")?,
                    ),
                ),
                ("start", int(span.start)),
                ("end", int(span.end)),
                ("text", RecordValue::str(text)),
            ]));
        }
        let mut addr_taken = products.addr_taken.iter().cloned().collect::<Vec<_>>();
        addr_taken.sort();
        let item = record_map([
            ("item", RecordValue::str(cache_key)),
            ("summaries", RecordValue::List(summaries)),
            ("addr_taken", RecordValue::list_of_str(&addr_taken)),
            ("exact_int", RecordValue::Bool(products.uses_exact_int)),
            ("references", RecordValue::List(references)),
            ("alias_uses", RecordValue::List(alias_uses)),
            ("text", RecordValue::str(item_text)),
            ("body", RecordValue::Bytes(delta.to_vec())),
            ("checked", RecordValue::str(jet_foundation::SHA256::sha256_hex(delta))),
        ]);
        encode_record(ITEM_CHECK_SCHEMA, &[RecordSection::new(SECTION_ITEM, vec![item])])
            .map_err(|_| "record not encodable")
    }

    /// The replayable outputs of a stored record for the item at `span`, or
    /// `None` when the record does not decode, was made from another item
    /// text (`item_text` digest), carries a delta that fails its digest, or
    /// a reference no longer names the declaration it named.
    fn decode(&self, module: usize, span: Span, item_text: &str, bytes: &[u8]) -> Option<Seed> {
        let start = span.start;
        let reader = RecordReader::open_schema(bytes, ITEM_CHECK_SCHEMA).ok()?;
        let item = reader.element(SECTION_ITEM, 0).ok()?;
        if item.field("text").ok()?.as_str().ok()? != item_text {
            return None;
        }
        let RecordValue::Bytes(body) = item.field("body").ok()? else {
            return None;
        };
        if jet_foundation::SHA256::sha256_hex(body) != item.field("checked").ok()?.as_str().ok()? {
            return None;
        }
        let mut summaries = Vec::new();
        for entry in item.field("summaries").ok()?.as_list().ok()? {
            let key = entry.field("key").ok()?.as_str().ok()?.to_string();
            let mut summary = decode_summary(entry.field("summary").ok()?).ok()?;
            map_edges(&mut summary, &|edge| to_alias(edge, &self.aliases));
            each_span(&mut summary, &mut |span| {
                *span = Span::new(span.start + start, span.end + start);
                true
            });
            summaries.push((key, summary));
        }
        let mut references = Vec::new();
        for entry in item.field("references").ok()?.as_list().ok()? {
            let number = |name: &str| entry.field(name).ok()?.as_usize().ok();
            let def_module = *self
                .by_identity
                .get(entry.field("module").ok()?.as_str().ok()?)?;
            let offset = number("offset")?;
            let index = number("index")?;
            let callables = &self.anchors.get(def_module)?.callables;
            let def_start = match entry.field("mode").ok()?.as_str().ok()? {
                "own" if def_module == module => start + offset,
                "in" => callables.get(index)?.0 + offset,
                "after" => callables.get(index)?.1 + offset,
                "file" => offset,
                _ => return None,
            };
            let def_end = def_start + number("len")?;
            let text = entry.field("text").ok()?.as_str().ok()?;
            if !text.is_empty() && self.text(def_module, def_start, def_end)? != text {
                return None;
            }
            let identity = entry.field("identity").ok()?;
            references.push((
                start + number("start")?,
                start + number("end")?,
                NameReference {
                    module_path: self.paths[def_module].clone()?,
                    kind: entry.field("kind").ok()?.as_str().ok()?.to_string(),
                    def_span: Span::new(def_start, def_end),
                    semantic_identity: if identity.is_null() {
                        None
                    } else {
                        Some(identity.as_str().ok()?.to_string())
                    },
                },
            ));
        }
        let mut alias_uses = Vec::new();
        for entry in item.field("alias_uses").ok()?.as_list().ok()? {
            let alias_module = *self
                .by_identity
                .get(entry.field("module").ok()?.as_str().ok()?)?;
            let span = Span::new(
                entry.field("start").ok()?.as_usize().ok()?,
                entry.field("end").ok()?.as_usize().ok()?,
            );
            if self.text(alias_module, span.start, span.end)? != entry.field("text").ok()?.as_str().ok()? {
                return None;
            }
            alias_uses.push((alias_module, span));
        }
        Some(Seed {
            summaries,
            addr_taken: item.field("addr_taken").ok()?.str_list().ok()?,
            exact_int: item.field("exact_int").ok()?.as_bool().ok()?,
            references,
            alias_uses,
            body: body.clone(),
        })
    }
}
