// D-DEP-GC1=A: private, dependency-free tracing collector substrate.
//
// Frontend policy and automatic promotion live elsewhere. This module owns
// only stable identities, roots, traced edges, safepoints, and reclamation.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Write as IoWrite;
use std::marker::PhantomData;
use std::path::PathBuf;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, TryLockError, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_OBJECTS: usize = 1_000_000;
const MAX_EDGES_PER_OBJECT: usize = 65_536;
const MAX_TRACE_BYTES: usize = 4 * 1024 * 1024;
const MAX_TRACE_FIELD_BYTES: usize = 4 * 1024;
const MAX_TRACE_IDENTITIES: usize = 65_536;
const MAX_TRACE_SITES: usize = 4_096;
const TRACE_SCHEMA: &str = "jet.gc.trace";
const TRACE_VERSION: u32 = 1;
static NEXT_OBJECT_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_TRACE_TEMP: AtomicU64 = AtomicU64::new(1);
static TRACE: OnceLock<Option<Mutex<TraceState>>> = OnceLock::new();

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ObjectId(u64);

impl ObjectId {
    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Fault {
    HeapPoisoned,
    IdExhausted,
    ObjectLimit,
    TooManyEdges { count: usize, limit: usize },
    UnknownObject(ObjectId),
    DanglingEdge { from: ObjectId, to: ObjectId },
    RootCountOverflow(ObjectId),
    PinCountOverflow(ObjectId),
    MutationConflict(ObjectId),
    VersionOverflow(ObjectId),
    PayloadPoisoned(ObjectId),
    BorrowConflict(ObjectId),
    TypeMismatch(ObjectId),
    HeapGone,
    TracePoisoned,
    TraceIo(String),
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "collector invariant failed: {self:?}")
    }
}

impl std::error::Error for Fault {}

/// Compiler-authored provenance for one automatic GC promotion. Ordinary
/// collector allocations do not produce telemetry; codegen must choose this
/// API only for values promoted by an effective `gc` policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PromotionSite {
    pub source: &'static str,
    pub span_start: u64,
    pub span_end: u64,
    pub scope: &'static str,
    pub policy_provenance: &'static str,
    pub reason: &'static str,
    pub type_name: &'static str,
    pub bytes: u64,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct TraceKey {
    source: String,
    span_start: u64,
    span_end: u64,
    scope: String,
    policy_provenance: String,
    reason: String,
    type_name: String,
    bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct TraceIdentity {
    id: ObjectId,
    retained: bool,
}

struct TraceState {
    path: PathBuf,
    project: String,
    pid: u32,
    started_unix_ms: u128,
    updated_unix_ms: u128,
    complete: bool,
    dropped_promotions: u64,
    collections: u64,
    identity_count: usize,
    sites: BTreeMap<TraceKey, Vec<TraceIdentity>>,
}

impl TraceState {
    fn from_env() -> Option<Self> {
        let path = std::env::var_os("JET_GC_TRACE").filter(|value| !value.is_empty())?;
        let project = std::env::var("JET_GC_PROJECT").unwrap_or_else(|_| {
            std::env::current_dir()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
        let now = unix_ms();
        Some(Self {
            path: PathBuf::from(path),
            project,
            pid: std::process::id(),
            started_unix_ms: now,
            updated_unix_ms: now,
            complete: true,
            dropped_promotions: 0,
            collections: 0,
            identity_count: 0,
            sites: BTreeMap::new(),
        })
    }

    fn record(&mut self, id: ObjectId, site: PromotionSite) -> Result<(), Fault> {
        if site.span_start > site.span_end
            || [site.source, site.scope, site.policy_provenance, site.reason, site.type_name]
                .iter()
                .any(|value| {
                    value.is_empty()
                        || value.len() > MAX_TRACE_FIELD_BYTES
                        || value.chars().any(char::is_control)
                })
        {
            self.drop_promotion();
            return self.persist();
        }
        let key = TraceKey {
            source: site.source.to_string(),
            span_start: site.span_start,
            span_end: site.span_end,
            scope: site.scope.to_string(),
            policy_provenance: site.policy_provenance.to_string(),
            reason: site.reason.to_string(),
            type_name: site.type_name.to_string(),
            bytes: site.bytes,
        };
        if self.identity_count >= MAX_TRACE_IDENTITIES
            || (!self.sites.contains_key(&key) && self.sites.len() >= MAX_TRACE_SITES)
        {
            self.drop_promotion();
            return self.persist();
        }
        self.sites
            .entry(key.clone())
            .or_default()
            .push(TraceIdentity { id, retained: true });
        self.identity_count += 1;
        if self.render().len() > MAX_TRACE_BYTES {
            self.remove_last(&key);
            self.drop_promotion();
            return self.persist();
        }
        if let Err(fault) = self.persist() {
            self.remove_last(&key);
            return Err(fault);
        }
        Ok(())
    }

    fn remove_last(&mut self, key: &TraceKey) {
        let remove_site = if let Some(identities) = self.sites.get_mut(key) {
            identities.pop();
            identities.is_empty()
        } else {
            false
        };
        if remove_site {
            self.sites.remove(key);
        }
        self.identity_count = self.identity_count.saturating_sub(1);
    }

    fn record_collection(&mut self, reclaimed: &[ObjectId]) -> Result<(), Fault> {
        self.collections = self.collections.saturating_add(1);
        let reclaimed: BTreeSet<ObjectId> = reclaimed.iter().copied().collect();
        for identities in self.sites.values_mut() {
            for identity in identities {
                if identity.retained && reclaimed.contains(&identity.id) {
                    identity.retained = false;
                }
            }
        }
        self.persist()
    }

    fn drop_promotion(&mut self) {
        self.complete = false;
        self.dropped_promotions = self.dropped_promotions.saturating_add(1);
    }

    fn persist(&mut self) -> Result<(), Fault> {
        self.updated_unix_ms = unix_ms();
        let json = self.render();
        if json.len() > MAX_TRACE_BYTES {
            return Err(Fault::TraceIo("GC trace exceeds its 4 MiB safety limit".to_string()));
        }
        let parent = self
            .path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        {
            let _parent_existed = match std::fs::symlink_metadata(parent) {
                Ok(metadata) if !metadata.file_type().is_dir() => {
                    return Err(Fault::TraceIo(
                        "GC trace directory path is not a directory".to_string(),
                    ));
                }
                Ok(_) => true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => {
                    return Err(Fault::TraceIo(format!(
                        "cannot inspect GC trace directory: {error}"
                    )));
                }
            };
            std::fs::create_dir_all(parent)
                .map_err(|error| Fault::TraceIo(format!("cannot create GC trace directory: {error}")))?;
            #[cfg(unix)]
            if !_parent_existed {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700))
                    .map_err(|error| Fault::TraceIo(format!("cannot secure GC trace directory: {error}")))?;
            }
        }
        let _existing_metadata = match std::fs::symlink_metadata(&self.path) {
            Ok(metadata) if !metadata.file_type().is_file() => {
                return Err(Fault::TraceIo("GC trace path is not a regular file".to_string()));
            }
            Ok(metadata) => Some(metadata),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(Fault::TraceIo(format!("cannot inspect GC trace: {error}")));
            }
        };
        let file_name = self.path.file_name().ok_or_else(|| {
            Fault::TraceIo("GC trace path has no file name".to_string())
        })?;
        let mut temp = None;
        for _ in 0..64 {
            let sequence = NEXT_TRACE_TEMP.fetch_add(1, Ordering::Relaxed);
            let temp_name = format!(
                ".{}.tmp.{}.{}",
                file_name.to_string_lossy(),
                std::process::id(),
                sequence
            );
            let temp_path = parent.join(temp_name);
            let mut options = std::fs::OpenOptions::new();
            options.create_new(true).write(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            #[cfg(any(target_os = "linux", target_os = "android"))]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.custom_flags(0o400000);
            }
            match options.open(&temp_path) {
                Ok(file) => {
                    temp = Some((temp_path, file));
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(Fault::TraceIo(format!(
                        "cannot create GC trace temporary file: {error}"
                    )));
                }
            }
        }
        let (temp_path, mut file) = temp.ok_or_else(|| {
            Fault::TraceIo("cannot reserve a GC trace temporary file".to_string())
        })?;
        #[cfg(unix)]
        if let Some(existing) = &_existing_metadata {
            use std::os::unix::fs::MetadataExt;
            let created = match file.metadata() {
                Ok(metadata) => metadata,
                Err(error) => {
                    drop(file);
                    let _ = std::fs::remove_file(&temp_path);
                    return Err(Fault::TraceIo(format!(
                        "cannot inspect GC trace temporary file: {error}"
                    )));
                }
            };
            if existing.uid() != created.uid() {
                drop(file);
                let _ = std::fs::remove_file(&temp_path);
                return Err(Fault::TraceIo(
                    "GC trace path is owned by a different user".to_string(),
                ));
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Err(error) = file.set_permissions(std::fs::Permissions::from_mode(0o600)) {
                drop(file);
                let _ = std::fs::remove_file(&temp_path);
                return Err(Fault::TraceIo(format!("cannot secure GC trace: {error}")));
            }
        }
        let write = file
            .write_all(json.as_bytes())
            .and_then(|_| file.sync_all());
        drop(file);
        let persist = write
            .and_then(|_| std::fs::rename(&temp_path, &self.path))
            .and_then(|_| sync_trace_parent(parent));
        if let Err(error) = persist {
            let _ = std::fs::remove_file(&temp_path);
            return Err(Fault::TraceIo(format!("cannot persist GC trace: {error}")));
        }
        Ok(())
    }

    fn render(&self) -> String {
        let mut sites = String::new();
        for (index, (key, identities)) in self.sites.iter().enumerate() {
            if index != 0 {
                sites.push(',');
            }
            let retained = identities.iter().filter(|identity| identity.retained).count();
            let identity_json = identities
                .iter()
                .map(|identity| {
                    format!(
                        "{{\"identity\":{},\"retained\":{}}}",
                        identity.id.get(),
                        identity.retained
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            sites.push_str(&format!(
                "{{\"source\":\"{}\",\"span_start\":{},\"span_end\":{},\"scope\":\"{}\",\"policy_provenance\":\"{}\",\"reason\":\"{}\",\"type_name\":\"{}\",\"bytes\":{},\"allocations\":{},\"retained\":{},\"identities\":[{}]}}",
                trace_escape(&key.source), key.span_start, key.span_end,
                trace_escape(&key.scope), trace_escape(&key.policy_provenance),
                trace_escape(&key.reason), trace_escape(&key.type_name),
                key.bytes, identities.len(), retained, identity_json
            ));
        }
        format!(
            "{{\"schema\":\"{TRACE_SCHEMA}\",\"version\":{TRACE_VERSION},\"project\":\"{}\",\"pid\":{},\"started_unix_ms\":{},\"updated_unix_ms\":{},\"complete\":{},\"dropped_promotions\":{},\"collections\":{},\"sites\":[{}]}}\n",
            trace_escape(&self.project), self.pid, self.started_unix_ms,
            self.updated_unix_ms, self.complete, self.dropped_promotions,
            self.collections, sites
        )
    }
}

#[cfg(unix)]
fn sync_trace_parent(parent: &std::path::Path) -> std::io::Result<()> {
    std::fs::File::open(parent)?.sync_all()
}

#[cfg(not(unix))]
fn sync_trace_parent(_parent: &std::path::Path) -> std::io::Result<()> {
    Ok(())
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn trace_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out
}

/// Start a fresh durable trace even when an opted GC scope performs no
/// promotions. Generated startup calls this once when GC tracing is enabled.
pub fn initialize_trace() -> Result<(), Fault> {
    let Some(trace) = TRACE
        .get_or_init(|| TraceState::from_env().map(Mutex::new))
        .as_ref()
    else {
        return Ok(());
    };
    trace
        .lock()
        .map_err(|_| Fault::TracePoisoned)?
        .persist()
}

/// Render every compiler-inserted collector failure as a stable Jet runtime
/// diagnostic. Generated code uses this single boundary instead of panicking.
pub fn runtime_or_exit<T>(result: Result<T, Fault>) -> T {
    match result {
        Ok(value) => value,
        Err(fault) => {
            eprintln!("error[E2110]: automatic memory management failed");
            eprintln!(" what: the private garbage collector could not complete an operation");
            eprintln!(" why: {fault}");
            eprintln!(" fix: check the GC trace path and retry with a smaller workload");
            std::process::exit(1);
        }
    }
}

fn trace_promotion(id: ObjectId, site: PromotionSite) -> Result<(), Fault> {
    let Some(trace) = TRACE
        .get_or_init(|| TraceState::from_env().map(Mutex::new))
        .as_ref()
    else {
        return Ok(());
    };
    trace
        .lock()
        .map_err(|_| Fault::TracePoisoned)?
        .record(id, site)
}

fn trace_collection(reclaimed: &[ObjectId]) -> Result<(), Fault> {
    let Some(trace) = TRACE.get().and_then(Option::as_ref) else {
        return Ok(());
    };
    trace
        .lock()
        .map_err(|_| Fault::TracePoisoned)?
        .record_collection(reclaimed)
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Collection {
    pub reachable: usize,
    pub reclaimed: Vec<ObjectId>,
    pub deferred: Vec<ObjectId>,
    pub finalizer_panics: Vec<ObjectId>,
    pub poisoned_payloads: Vec<ObjectId>,
    pub drop_panics: Vec<ObjectId>,
}

type ErasedFinalizer = Box<dyn FnOnce(&mut dyn Any) + Send + 'static>;

struct Object {
    value: Mutex<Box<dyn Any + Send>>,
    finalizer: Mutex<Option<ErasedFinalizer>>,
}

struct Entry {
    roots: usize,
    pins: usize,
    version: u64,
    reserved: bool,
    edges: Vec<ObjectId>,
    object: Arc<Object>,
}

#[derive(Default)]
struct State {
    entries: BTreeMap<ObjectId, Entry>,
}

struct Heap {
    state: Mutex<State>,
    collect_on_root_drop: bool,
}

impl Drop for Heap {
    fn drop(&mut self) {
        let entries = match self.state.get_mut() {
            Ok(state) => std::mem::take(&mut state.entries),
            Err(poisoned) => std::mem::take(&mut poisoned.into_inner().entries),
        };
        let mut ignored = Collection::default();
        finalize(entries.into_iter().collect(), &mut ignored);
    }
}

#[derive(Clone)]
pub struct Collector {
    heap: Arc<Heap>,
}

impl Default for Collector {
    fn default() -> Self {
        Self::new()
    }
}

impl Collector {
    pub fn new() -> Self {
        Self {
            heap: Arc::new(Heap {
                state: Mutex::new(State::default()),
                collect_on_root_drop: false,
            }),
        }
    }

    fn automatic() -> Self {
        Self {
            heap: Arc::new(Heap {
                state: Mutex::new(State::default()),
                collect_on_root_drop: true,
            }),
        }
    }

    pub fn allocate<T>(&self, value: T) -> Result<Root<T>, Fault>
    where
        T: Any + Send,
    {
        self.allocate_erased(value, None)
    }

    /// Allocate one value whose ownership proof required automatic promotion.
    /// This remains allocation-equivalent to `allocate` when tracing is off.
    pub fn allocate_traced<T>(&self, value: T, site: PromotionSite) -> Result<Root<T>, Fault>
    where
        T: Any + Send,
    {
        let root = self.allocate_erased(value, None)?;
        if let Err(fault) = trace_promotion(root.id, site) {
            if let Ok(mut state) = self.heap.state.lock() {
                state.entries.remove(&root.id);
            }
            return Err(fault);
        }
        Ok(root)
    }

    pub fn allocate_with_finalizer<T, F>(&self, value: T, finalizer: F) -> Result<Root<T>, Fault>
    where
        T: Any + Send,
        F: FnOnce(&mut T) + Send + 'static,
    {
        let erased: ErasedFinalizer = Box::new(move |value| {
            let value = value
                .downcast_mut::<T>()
                .expect("collector payload type invariant");
            finalizer(value);
        });
        self.allocate_erased(value, Some(erased))
    }

    fn allocate_erased<T>(
        &self,
        value: T,
        finalizer: Option<ErasedFinalizer>,
    ) -> Result<Root<T>, Fault>
    where
        T: Any + Send,
    {
        let mut state = self.heap.state.lock().map_err(|_| Fault::HeapPoisoned)?;
        if state.entries.len() >= MAX_OBJECTS {
            return Err(Fault::ObjectLimit);
        }
        let raw = NEXT_OBJECT_ID
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .map_err(|_| Fault::IdExhausted)?;
        let id = ObjectId(raw);
        state.entries.insert(
            id,
            Entry {
                roots: 1,
                pins: 0,
                version: 0,
                reserved: false,
                edges: Vec::new(),
                object: Arc::new(Object {
                    value: Mutex::new(Box::new(value)),
                    finalizer: Mutex::new(finalizer),
                }),
            },
        );
        Ok(Root {
            id,
            heap: Arc::clone(&self.heap),
            marker: PhantomData,
        })
    }

    pub fn replace_edges(&self, from: ObjectId, edges: &[ObjectId]) -> Result<(), Fault> {
        replace_edges(&self.heap, from, edges)
    }

    pub fn live_count(&self) -> Result<usize, Fault> {
        Ok(self
            .heap
            .state
            .lock()
            .map_err(|_| Fault::HeapPoisoned)?
            .entries
            .len())
    }

    pub fn safepoint(&self) -> Result<Collection, Fault> {
        let mut state = self.heap.state.lock().map_err(|_| Fault::HeapPoisoned)?;
        let mut marked = BTreeSet::new();
        let mut stack: Vec<ObjectId> = state
            .entries
            .iter()
            .filter_map(|(id, entry)| {
                (entry.roots != 0
                    || entry.pins != 0
                    || Arc::strong_count(&entry.object) != 1)
                    .then_some(*id)
            })
            .rev()
            .collect();

        while let Some(id) = stack.pop() {
            if !marked.insert(id) {
                continue;
            }
            let entry = state.entries.get(&id).ok_or(Fault::UnknownObject(id))?;
            for child in entry.edges.iter().rev() {
                if !state.entries.contains_key(child) {
                    return Err(Fault::DanglingEdge {
                        from: id,
                        to: *child,
                    });
                }
                stack.push(*child);
            }
        }

        let candidates: Vec<ObjectId> = state
            .entries
            .keys()
            .filter(|id| !marked.contains(id))
            .copied()
            .collect();
        let mut removed = Vec::new();
        let mut deferred = Vec::new();
        for id in candidates {
            let busy = state
                .entries
                .get(&id)
                .is_some_and(|entry| Arc::strong_count(&entry.object) != 1);
            if busy {
                deferred.push(id);
            } else if let Some(entry) = state.entries.remove(&id) {
                removed.push((id, entry));
            }
        }
        drop(state);

        let mut result = Collection {
            reachable: marked.len(),
            deferred,
            ..Collection::default()
        };
        finalize(removed, &mut result);
        trace_collection(&result.reclaimed)?;
        Ok(result)
    }

    pub fn collect(&self) -> Result<Collection, Fault> {
        self.safepoint()
    }
}

/// One private collector for compiler-inserted promotions. Source code has no
/// constructor or handle for this object.
pub fn automatic_collector() -> &'static Collector {
    static COLLECTOR: OnceLock<Collector> = OnceLock::new();
    COLLECTOR.get_or_init(Collector::automatic)
}

/// Compiler-private storage for a source-level bare value promoted by
/// D-OPTGC1. All payload access remains serialized through the collector.
pub struct AutomaticRoot<T: Any + Send> {
    root: Root<T>,
    edge_slots: Arc<Mutex<BTreeMap<String, Vec<Vec<ObjectId>>>>>,
}

impl<T: Any + Send> AutomaticRoot<T> {
    pub fn promote(value: T, site: PromotionSite) -> Result<Self, Fault> {
        automatic_collector()
            .allocate_traced(value, site)
            .map(|root| Self {
                root,
                edge_slots: Arc::new(Mutex::new(BTreeMap::new())),
            })
    }

    pub fn id(&self) -> ObjectId {
        self.root.id()
    }

    pub fn read<R>(&self, read: impl FnOnce(&T) -> R) -> Result<R, Fault> {
        self.root.read(read)
    }

    pub fn edit<R>(&self, edit: impl FnOnce(&mut T) -> R) -> Result<R, Fault> {
        self.root.edit(edit)
    }

    pub fn replace_edges(&self, edges: &[ObjectId]) -> Result<(), Fault> {
        self.replace_edge_slot("initial", edges)
    }

    pub fn replace_edge_slots(
        &self,
        edges: &[(&str, usize, ObjectId)],
        collection_len: Option<usize>,
    ) -> Result<(), Fault> {
        let mut slots = self.edge_slots.lock().map_err(|_| Fault::HeapPoisoned)?;
        let mut next = BTreeMap::<String, Vec<Vec<ObjectId>>>::new();
        for (slot, group, id) in edges {
            let groups = next.entry((*slot).to_string()).or_default();
            while groups.len() <= *group {
                groups.push(Vec::new());
            }
            groups[*group].push(*id);
        }
        if let Some(len) = collection_len {
            let groups = next.entry("collection".to_string()).or_default();
            while groups.len() < len {
                groups.push(Vec::new());
            }
        }
        let flattened = flatten_edge_slots(&next);
        self.root.replace_edges(&flattened)?;
        *slots = next;
        Ok(())
    }

    pub fn replace_edge_slot(&self, slot: &str, edges: &[ObjectId]) -> Result<(), Fault> {
        let mut slots = self.edge_slots.lock().map_err(|_| Fault::HeapPoisoned)?;
        let mut next = slots.clone();
        if edges.is_empty() {
            next.remove(slot);
        } else {
            next.insert(slot.to_string(), vec![edges.to_vec()]);
        }
        let flattened = flatten_edge_slots(&next);
        self.root.replace_edges(&flattened)?;
        *slots = next;
        Ok(())
    }

    pub fn edit_edge_slot<R>(
        &self,
        slot: &str,
        edges: &[ObjectId],
        edit: impl FnOnce(&mut T) -> R,
    ) -> Result<R, Fault> {
        let mut slots = self.edge_slots.lock().map_err(|_| Fault::HeapPoisoned)?;
        let mut next = slots.clone();
        if edges.is_empty() {
            next.remove(slot);
        } else {
            next.insert(slot.to_string(), vec![edges.to_vec()]);
        }
        let flattened = flatten_edge_slots(&next);
        let result = self.root.edit_with_edges(&flattened, edit)?;
        *slots = next;
        Ok(result)
    }

    pub fn edit_edge_slot_index<R>(
        &self,
        slot: &str,
        index: usize,
        edges: &[ObjectId],
        edit: impl FnOnce(&mut T) -> R,
    ) -> Result<R, Fault> {
        let mut slots = self.edge_slots.lock().map_err(|_| Fault::HeapPoisoned)?;
        let mut next = slots.clone();
        let groups = next.entry(slot.to_string()).or_default();
        while groups.len() <= index {
            groups.push(Vec::new());
        }
        groups[index] = edges.to_vec();
        let flattened = flatten_edge_slots(&next);
        let result = self.root.edit_with_edges(&flattened, edit)?;
        *slots = next;
        Ok(result)
    }

    pub fn edit_replacing_all_edges<R>(
        &self,
        edges: &[ObjectId],
        edit: impl FnOnce(&mut T) -> R,
    ) -> Result<R, Fault> {
        let mut slots = self.edge_slots.lock().map_err(|_| Fault::HeapPoisoned)?;
        let result = self.root.edit_with_edges(edges, edit)?;
        slots.clear();
        if !edges.is_empty() {
            slots.insert("value".to_string(), vec![edges.to_vec()]);
        }
        Ok(result)
    }

    pub fn edit_clearing_edges<R>(
        &self,
        edit: impl FnOnce(&mut T) -> R,
    ) -> Result<R, Fault> {
        self.edit_replacing_all_edges(&[], edit)
    }

    pub fn edit_edge_slot_additive<R>(
        &self,
        slot: &str,
        edges: &[ObjectId],
        edit: impl FnOnce(&mut T) -> R,
    ) -> Result<R, Fault> {
        let mut slots = self.edge_slots.lock().map_err(|_| Fault::HeapPoisoned)?;
        let mut next = slots.clone();
        next.entry(slot.to_string()).or_default().push(edges.to_vec());
        let flattened = flatten_edge_slots(&next);
        let result = self.root.edit_with_edges(&flattened, edit)?;
        *slots = next;
        Ok(result)
    }

    pub fn edit_edge_slot_prepend<R>(
        &self,
        slot: &str,
        edges: &[ObjectId],
        edit: impl FnOnce(&mut T) -> R,
    ) -> Result<R, Fault> {
        self.edit_edge_slot_insert(slot, 0, edges, edit)
    }

    pub fn edit_edge_slot_insert<R>(
        &self,
        slot: &str,
        index: usize,
        edges: &[ObjectId],
        edit: impl FnOnce(&mut T) -> R,
    ) -> Result<R, Fault> {
        let mut slots = self.edge_slots.lock().map_err(|_| Fault::HeapPoisoned)?;
        let mut next = slots.clone();
        let groups = next.entry(slot.to_string()).or_default();
        groups.insert(index.min(groups.len()), edges.to_vec());
        let flattened = flatten_edge_slots(&next);
        let result = self.root.edit_with_edges(&flattened, edit)?;
        *slots = next;
        Ok(result)
    }

    pub fn edit_edge_slot_remove<R>(
        &self,
        slot: &str,
        index: usize,
        edit: impl FnOnce(&mut T) -> R,
    ) -> Result<R, Fault> {
        let mut slots = self.edge_slots.lock().map_err(|_| Fault::HeapPoisoned)?;
        let mut next = slots.clone();
        if let Some(groups) = next.get_mut(slot) {
            if index < groups.len() {
                groups.remove(index);
            }
            if groups.is_empty() {
                next.remove(slot);
            }
        }
        let flattened = flatten_edge_slots(&next);
        let result = self.root.edit_with_edges(&flattened, edit)?;
        *slots = next;
        Ok(result)
    }

    pub fn edit_edge_slot_pop<R>(
        &self,
        slot: &str,
        edit: impl FnOnce(&mut T) -> R,
    ) -> Result<R, Fault> {
        let mut slots = self.edge_slots.lock().map_err(|_| Fault::HeapPoisoned)?;
        let mut next = slots.clone();
        if let Some(groups) = next.get_mut(slot) {
            groups.pop();
            if groups.is_empty() {
                next.remove(slot);
            }
        }
        let flattened = flatten_edge_slots(&next);
        let result = self.root.edit_with_edges(&flattened, edit)?;
        *slots = next;
        Ok(result)
    }

    pub fn try_clone_root(&self) -> Result<Self, Fault> {
        self.root.try_clone().map(|root| Self {
            root,
            edge_slots: Arc::clone(&self.edge_slots),
        })
    }
}

fn flatten_edge_slots(slots: &BTreeMap<String, Vec<Vec<ObjectId>>>) -> Vec<ObjectId> {
    slots
        .values()
        .flatten()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub struct Root<T: Any + Send> {
    id: ObjectId,
    heap: Arc<Heap>,
    marker: PhantomData<fn() -> T>,
}

impl<T: Any + Send> Root<T> {
    pub fn id(&self) -> ObjectId {
        self.id
    }

    pub fn try_clone(&self) -> Result<Self, Fault> {
        let mut state = self.heap.state.lock().map_err(|_| Fault::HeapPoisoned)?;
        let entry = state
            .entries
            .get_mut(&self.id)
            .ok_or(Fault::UnknownObject(self.id))?;
        entry.roots = entry
            .roots
            .checked_add(1)
            .ok_or(Fault::RootCountOverflow(self.id))?;
        Ok(Self {
            id: self.id,
            heap: Arc::clone(&self.heap),
            marker: PhantomData,
        })
    }

    pub fn edge(&self) -> Edge<T> {
        Edge {
            id: self.id,
            heap: Arc::downgrade(&self.heap),
            marker: PhantomData,
        }
    }

    pub fn replace_edges(&self, edges: &[ObjectId]) -> Result<(), Fault> {
        replace_edges(&self.heap, self.id, edges)
    }

    pub fn read<R>(&self, read: impl FnOnce(&T) -> R) -> Result<R, Fault> {
        access(&self.heap, self.id, |value| read(value))
    }

    pub fn edit<R>(&self, edit: impl FnOnce(&mut T) -> R) -> Result<R, Fault> {
        access_mut(&self.heap, self.id, edit)
    }

    pub fn edit_with_edges<R>(
        &self,
        edges: &[ObjectId],
        edit: impl FnOnce(&mut T) -> R,
    ) -> Result<R, Fault> {
        let reservation = reserve_mutation(&self.heap, self.id, edges)?;
        let object = lookup(&self.heap, self.id)?;
        let mut guard = match object.value.try_lock() {
            Ok(value) => value,
            Err(TryLockError::WouldBlock) => return Err(Fault::BorrowConflict(self.id)),
            Err(TryLockError::Poisoned(_)) => return Err(Fault::PayloadPoisoned(self.id)),
        };
        let value = guard
            .downcast_mut::<T>()
            .ok_or(Fault::TypeMismatch(self.id))?;
        match catch_unwind(AssertUnwindSafe(|| edit(value))) {
            Ok(result) => {
                drop(guard);
                drop(object);
                reservation.commit()?;
                Ok(result)
            }
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }
}

impl<T: Any + Send> Drop for Root<T> {
    fn drop(&mut self) {
        let mut state = match self.heap.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(entry) = state.entries.get_mut(&self.id) {
            entry.roots = entry.roots.saturating_sub(1);
        }
        let collect = self.heap.collect_on_root_drop;
        drop(state);
        if collect {
            runtime_or_exit(Collector {
                heap: Arc::clone(&self.heap),
            }
            .safepoint());
        }
    }
}

pub struct Edge<T: Any + Send> {
    id: ObjectId,
    heap: Weak<Heap>,
    marker: PhantomData<fn() -> T>,
}

impl<T: Any + Send> Clone for Edge<T> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            heap: Weak::clone(&self.heap),
            marker: PhantomData,
        }
    }
}

impl<T: Any + Send> Edge<T> {
    pub fn id(&self) -> ObjectId {
        self.id
    }

    pub fn read<R>(&self, read: impl FnOnce(&T) -> R) -> Result<R, Fault> {
        let heap = self.heap.upgrade().ok_or(Fault::HeapGone)?;
        access(&heap, self.id, read)
    }

    pub fn edit<R>(&self, edit: impl FnOnce(&mut T) -> R) -> Result<R, Fault> {
        let heap = self.heap.upgrade().ok_or(Fault::HeapGone)?;
        access_mut(&heap, self.id, edit)
    }
}

fn replace_edges(heap: &Arc<Heap>, from: ObjectId, edges: &[ObjectId]) -> Result<(), Fault> {
    let normalized = normalize_edges(edges)?;
    let mut state = heap.state.lock().map_err(|_| Fault::HeapPoisoned)?;
    let source = state
        .entries
        .get(&from)
        .ok_or(Fault::UnknownObject(from))?;
    if source.reserved {
        return Err(Fault::MutationConflict(from));
    }
    let next_version = source
        .version
        .checked_add(1)
        .ok_or(Fault::VersionOverflow(from))?;
    for child in &normalized {
        if !state.entries.contains_key(child) {
            return Err(Fault::UnknownObject(*child));
        }
    }
    let source = state
        .entries
        .get_mut(&from)
        .expect("collector source validated");
    source.edges = normalized;
    source.version = next_version;
    Ok(())
}

fn normalize_edges(edges: &[ObjectId]) -> Result<Vec<ObjectId>, Fault> {
    if edges.len() > MAX_EDGES_PER_OBJECT {
        return Err(Fault::TooManyEdges {
            count: edges.len(),
            limit: MAX_EDGES_PER_OBJECT,
        });
    }
    let mut normalized = edges.to_vec();
    normalized.sort_unstable();
    normalized.dedup();
    Ok(normalized)
}

struct MutationReservation {
    heap: Arc<Heap>,
    from: ObjectId,
    edges: Vec<ObjectId>,
    pinned: Vec<ObjectId>,
    version: u64,
    active: bool,
}

impl MutationReservation {
    fn commit(mut self) -> Result<(), Fault> {
        let mut state = self.heap.state.lock().map_err(|_| Fault::HeapPoisoned)?;
        let source = state
            .entries
            .get(&self.from)
            .ok_or(Fault::UnknownObject(self.from))?;
        if !source.reserved || source.version != self.version {
            return Err(Fault::MutationConflict(self.from));
        }
        let next_version = source
            .version
            .checked_add(1)
            .ok_or(Fault::VersionOverflow(self.from))?;
        for child in &self.edges {
            if !state.entries.contains_key(child) {
                return Err(Fault::UnknownObject(*child));
            }
        }
        let source = state
            .entries
            .get_mut(&self.from)
            .expect("collector source pinned");
        source.edges = std::mem::take(&mut self.edges);
        source.version = next_version;
        source.reserved = false;
        for id in &self.pinned {
            let entry = state.entries.get_mut(id).expect("collector pin retained");
            entry.pins = entry.pins.saturating_sub(1);
        }
        drop(state);
        self.active = false;
        Ok(())
    }
}

impl Drop for MutationReservation {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        let mut state = match self.heap.state.lock() {
            Ok(state) => state,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(source) = state.entries.get_mut(&self.from) {
            if source.reserved && source.version == self.version {
                source.reserved = false;
            }
        }
        for id in &self.pinned {
            if let Some(entry) = state.entries.get_mut(id) {
                entry.pins = entry.pins.saturating_sub(1);
            }
        }
    }
}

fn reserve_mutation(
    heap: &Arc<Heap>,
    from: ObjectId,
    edges: &[ObjectId],
) -> Result<MutationReservation, Fault> {
    let edges = normalize_edges(edges)?;
    let mut state = heap.state.lock().map_err(|_| Fault::HeapPoisoned)?;
    let source = state
        .entries
        .get(&from)
        .ok_or(Fault::UnknownObject(from))?;
    if source.reserved {
        return Err(Fault::MutationConflict(from));
    }
    let version = source.version;
    let mut pinned = source.edges.clone();
    pinned.extend(edges.iter().copied());
    pinned.push(from);
    pinned.sort_unstable();
    pinned.dedup();
    for id in &pinned {
        let entry = state.entries.get(id).ok_or(Fault::UnknownObject(*id))?;
        if entry.pins == usize::MAX {
            return Err(Fault::PinCountOverflow(*id));
        }
    }
    for id in &pinned {
        state.entries.get_mut(id).expect("collector pin validated").pins += 1;
    }
    state
        .entries
        .get_mut(&from)
        .expect("collector source validated")
        .reserved = true;
    drop(state);
    Ok(MutationReservation {
        heap: Arc::clone(heap),
        from,
        edges,
        pinned,
        version,
        active: true,
    })
}

fn lookup(heap: &Arc<Heap>, id: ObjectId) -> Result<Arc<Object>, Fault> {
    let state = heap.state.lock().map_err(|_| Fault::HeapPoisoned)?;
    state
        .entries
        .get(&id)
        .map(|entry| Arc::clone(&entry.object))
        .ok_or(Fault::UnknownObject(id))
}

fn access<T, R>(
    heap: &Arc<Heap>,
    id: ObjectId,
    read: impl FnOnce(&T) -> R,
) -> Result<R, Fault>
where
    T: Any + Send,
{
    let object = lookup(heap, id)?;
    let value = match object.value.try_lock() {
        Ok(value) => value,
        Err(TryLockError::WouldBlock) => return Err(Fault::BorrowConflict(id)),
        Err(TryLockError::Poisoned(_)) => return Err(Fault::PayloadPoisoned(id)),
    };
    value
        .downcast_ref::<T>()
        .map(read)
        .ok_or(Fault::TypeMismatch(id))
}

fn access_mut<T, R>(
    heap: &Arc<Heap>,
    id: ObjectId,
    edit: impl FnOnce(&mut T) -> R,
) -> Result<R, Fault>
where
    T: Any + Send,
{
    let object = lookup(heap, id)?;
    let mut value = match object.value.try_lock() {
        Ok(value) => value,
        Err(TryLockError::WouldBlock) => return Err(Fault::BorrowConflict(id)),
        Err(TryLockError::Poisoned(_)) => return Err(Fault::PayloadPoisoned(id)),
    };
    value
        .downcast_mut::<T>()
        .map(edit)
        .ok_or(Fault::TypeMismatch(id))
}

fn finalize(entries: Vec<(ObjectId, Entry)>, result: &mut Collection) {
    for (id, entry) in entries {
        let Ok(object) = Arc::try_unwrap(entry.object) else {
            result.deferred.push(id);
            continue;
        };
        let finalizer = match object.finalizer.into_inner() {
            Ok(finalizer) => finalizer,
            Err(poisoned) => poisoned.into_inner(),
        };
        let mut value = match object.value.into_inner() {
            Ok(value) => value,
            Err(poisoned) => {
                result.poisoned_payloads.push(id);
                let value = poisoned.into_inner();
                if catch_unwind(AssertUnwindSafe(|| drop(value))).is_err() {
                    result.drop_panics.push(id);
                }
                result.reclaimed.push(id);
                continue;
            }
        };

        if let Some(finalizer) = finalizer {
            if catch_unwind(AssertUnwindSafe(|| finalizer(value.as_mut()))).is_err() {
                result.finalizer_panics.push(id);
            }
        }
        if catch_unwind(AssertUnwindSafe(|| drop(value))).is_err() {
            result.drop_panics.push(id);
        }
        result.reclaimed.push(id);
    }
    result.reclaimed.sort_unstable();
    result.deferred.sort_unstable();
}
