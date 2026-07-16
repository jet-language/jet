// D-DEP-GC1=A: private, dependency-free tracing collector substrate.
//
// Frontend policy and automatic promotion live elsewhere. This module owns
// only stable identities, roots, traced edges, safepoints, and reclamation.

use std::any::Any;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::marker::PhantomData;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, TryLockError, Weak};

const MAX_OBJECTS: usize = 1_000_000;
const MAX_EDGES_PER_OBJECT: usize = 65_536;
static NEXT_OBJECT_ID: AtomicU64 = AtomicU64::new(1);

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
}

impl fmt::Display for Fault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "collector invariant failed: {self:?}")
    }
}

impl std::error::Error for Fault {}

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
            }),
        }
    }

    pub fn allocate<T>(&self, value: T) -> Result<Root<T>, Fault>
    where
        T: Any + Send,
    {
        self.allocate_erased(value, None)
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
        Ok(result)
    }

    pub fn collect(&self) -> Result<Collection, Fault> {
        self.safepoint()
    }
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
