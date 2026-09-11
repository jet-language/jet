// D-DEP-GC1=A: private, dependency-free tracing collector substrate.
//
// Frontend policy and automatic promotion live elsewhere. This module owns
// only stable identities, roots, traced edges, safepoints, and reclamation.
// Platform synchronization, identity allocation, tracing, panic transport,
// and termination are supplied by the one adapter assembled beside this file.

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, BTreeSet};
use alloc::string::String;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use core::any::Any;
use core::fmt;
use core::marker::PhantomData;

const MAX_OBJECTS: usize = 1_000_000;
const MAX_EDGES_PER_OBJECT: usize = 65_536;

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

/// Panic payload transport stays in the adapter so the core remains usable by
/// a no-std target whose panic policy aborts instead of unwinding.
pub(crate) type GcPanic = Box<dyn Any + Send>;

/// Start a fresh durable trace even when an opted GC scope performs no
/// promotions. Generated startup calls this once when GC tracing is enabled.
pub fn initialize_trace() -> Result<(), Fault> {
    gc_initialize_trace()
}

/// Render every compiler-inserted collector failure as a stable Jet runtime
/// diagnostic. Generated code uses this single boundary instead of panicking.
pub fn runtime_or_exit<T>(result: Result<T, Fault>) -> T {
    match result {
        Ok(value) => value,
        Err(fault) => gc_runtime_or_exit(fault),
    }
}
fn gc_write_failure(out: &mut dyn fmt::Write, fault: &Fault) -> fmt::Result {
    fmt::Write::write_fmt(
        out,
        format_args!("Error [E2110]: Automatic memory management failed\n"),
    )?;
    fmt::Write::write_fmt(
        out,
        format_args!(
            " Why: The private garbage collector could not complete an operation: {fault}\n"
        ),
    )?;
    fmt::Write::write_fmt(
        out,
        format_args!(" Fix: Check the GC trace path and retry with a smaller workload\n"),
    )
}

fn trace_promotion(id: ObjectId, site: PromotionSite) -> Result<(), Fault> {
    gc_record_memory_ledger(site);
    gc_trace_promotion(id, site)
}

fn trace_collection(reclaimed: &[ObjectId]) -> Result<(), Fault> {
    gc_trace_collection(reclaimed)
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
    value: GcMutex<Box<dyn Any + Send>>,
    finalizer: GcMutex<Option<ErasedFinalizer>>,
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
    state: GcMutex<State>,
    collect_on_root_drop: bool,
}

impl Drop for Heap {
    fn drop(&mut self) {
        let entries = match self.state.get_mut() {
            Ok(state) => core::mem::take(&mut state.entries),
            Err(poisoned) => core::mem::take(&mut poisoned.into_inner().entries),
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
                state: GcMutex::new(State::default()),
                collect_on_root_drop: false,
            }),
        }
    }

    fn automatic() -> Self {
        Self {
            heap: Arc::new(Heap {
                state: GcMutex::new(State::default()),
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
        let id = ObjectId(gc_next_object_id()?);
        state.entries.insert(
            id,
            Entry {
                roots: 1,
                pins: 0,
                version: 0,
                reserved: false,
                edges: Vec::new(),
                object: Arc::new(Object {
                    value: GcMutex::new(Box::new(value)),
                    finalizer: GcMutex::new(finalizer),
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
                (entry.roots != 0 || entry.pins != 0 || Arc::strong_count(&entry.object) != 1)
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
    static COLLECTOR: GcOnce<Collector> = GcOnce::new();
    COLLECTOR.get_or_init(Collector::automatic)
}

/// Compiler-private storage for a source-level bare value promoted by
/// D-OPTGC1. All payload access remains serialized through the collector.
pub struct AutomaticRoot<T: Any + Send> {
    root: Root<T>,
    edge_slots: Arc<GcMutex<BTreeMap<String, Vec<Vec<ObjectId>>>>>,
}

impl<T: Any + Send> AutomaticRoot<T> {
    pub fn promote(value: T, site: PromotionSite) -> Result<Self, Fault> {
        automatic_collector()
            .allocate_traced(value, site)
            .map(|root| Self {
                root,
                edge_slots: Arc::new(GcMutex::new(BTreeMap::new())),
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

    pub fn edit_clearing_edges<R>(&self, edit: impl FnOnce(&mut T) -> R) -> Result<R, Fault> {
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
        next.entry(slot.to_string())
            .or_default()
            .push(edges.to_vec());
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
            Err(GcTryLockError::WouldBlock) => return Err(Fault::BorrowConflict(self.id)),
            Err(GcTryLockError::Poisoned(_)) => return Err(Fault::PayloadPoisoned(self.id)),
        };
        let value = guard
            .downcast_mut::<T>()
            .ok_or(Fault::TypeMismatch(self.id))?;
        match gc_catch_unwind(|| edit(value)) {
            Ok(result) => {
                drop(guard);
                drop(object);
                reservation.commit()?;
                Ok(result)
            }
            Err(payload) => gc_resume_unwind(payload),
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
            runtime_or_exit(
                Collector {
                    heap: Arc::clone(&self.heap),
                }
                .safepoint(),
            );
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
    let source = state.entries.get(&from).ok_or(Fault::UnknownObject(from))?;
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
        source.edges = core::mem::take(&mut self.edges);
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
    let source = state.entries.get(&from).ok_or(Fault::UnknownObject(from))?;
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
        state
            .entries
            .get_mut(id)
            .expect("collector pin validated")
            .pins += 1;
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

fn access<T, R>(heap: &Arc<Heap>, id: ObjectId, read: impl FnOnce(&T) -> R) -> Result<R, Fault>
where
    T: Any + Send,
{
    let object = lookup(heap, id)?;
    let value = match object.value.try_lock() {
        Ok(value) => value,
        Err(GcTryLockError::WouldBlock) => return Err(Fault::BorrowConflict(id)),
        Err(GcTryLockError::Poisoned(_)) => return Err(Fault::PayloadPoisoned(id)),
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
        Err(GcTryLockError::WouldBlock) => return Err(Fault::BorrowConflict(id)),
        Err(GcTryLockError::Poisoned(_)) => return Err(Fault::PayloadPoisoned(id)),
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
                if gc_catch_unwind(|| drop(value)).is_err() {
                    result.drop_panics.push(id);
                }
                result.reclaimed.push(id);
                continue;
            }
        };

        if let Some(finalizer) = finalizer {
            if gc_catch_unwind(|| finalizer(value.as_mut())).is_err() {
                result.finalizer_panics.push(id);
            }
        }
        if gc_catch_unwind(|| drop(value)).is_err() {
            result.drop_panics.push(id);
        }
        result.reclaimed.push(id);
    }
    result.reclaimed.sort_unstable();
    result.deferred.sort_unstable();
}
