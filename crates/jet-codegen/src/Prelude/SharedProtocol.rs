// D-SHAREDGUARD1=A: the one lock and condition protocol shared by native
// Prelude values and evaluator adapters. Payload storage is deliberately
// outside this module; engines may marshal values, but not redefine policy.

#[allow(dead_code)]
pub const JET_SHARED_GUARD_EDIT_REQUIRED: &str = "a condition wait needs an edit guard";
#[allow(dead_code)]
pub const JET_SHARED_GUARD_WAIT_CANCELLED: &str = "condition wait cancelled";
#[allow(dead_code)]
pub const JET_SHARED_GUARD_INVALID: &str = "SharedGuard is invalid or released";
#[allow(dead_code)]
pub const JET_SHARED_GUARD_VALUE_STORAGE_FAILED: &str = "SharedGuard value storage failed";
#[allow(dead_code)]
pub const JET_SHARED_GUARD_CHARACTER_STORAGE_FAILED: &str =
    "SharedGuard character storage failed";
#[allow(dead_code)]
pub const JET_SHARED_TRANSACTION_VALUE_STORAGE_FAILED: &str =
    "Shared transaction record payload became invalid";

pub fn jet_shared_guard_validate_char(value: i32) -> Result<char, &'static str> {
    char::from_u32(value as u32).ok_or(JET_SHARED_GUARD_CHARACTER_STORAGE_FAILED)
}
/// Runtime scalar kinds with a lossless representation in one atomic word.
///
/// This metadata is shared by generated Prelude code and execution adapters;
/// it deliberately contains only scalar representations that can be read or
/// written without entering the blocking Shared protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetSharedScalarKind {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    F32,
    F64,
    Bool,
    Char,
}

/// The canonical wait-free carrier for a scalar Shared payload.
///
/// The payload bits use Acquire loads and Release stores.  The carrier has no
/// mutex, condition variable, allocation, or retry loop; structured Shared
/// values continue to use `JetSharedProtocol` separately.
pub struct JetSharedAtomic {
    kind: JetSharedScalarKind,
    bits: std::sync::atomic::AtomicU64,
}

impl JetSharedAtomic {
    pub fn new(kind: JetSharedScalarKind, bits: u64) -> Self {
        Self {
            kind,
            bits: std::sync::atomic::AtomicU64::new(bits),
        }
    }

    pub fn kind(&self) -> JetSharedScalarKind {
        self.kind
    }

    #[inline(always)]
    pub fn load(&self) -> u64 {
        self.bits
            .load(std::sync::atomic::Ordering::Acquire)
    }

    #[inline(always)]
    pub fn store(&self, bits: u64) {
        self.bits
            .store(bits, std::sync::atomic::Ordering::Release);
    }

    #[inline(always)]
    pub fn swap(&self, bits: u64) -> u64 {
        self.bits
            .swap(bits, std::sync::atomic::Ordering::AcqRel)
    }
}

/// Return the scalar carrier kind for a native Prelude payload.
///
/// Type identity is resolved once when the Shared cell is constructed.  A
/// non-scalar payload returns `None` and keeps the protocol-backed path.
pub fn jet_shared_scalar_kind<T: 'static>() -> Option<JetSharedScalarKind> {
    let ty = std::any::TypeId::of::<T>();
    Some(if ty == std::any::TypeId::of::<i8>() {
        JetSharedScalarKind::I8
    } else if ty == std::any::TypeId::of::<u8>() {
        JetSharedScalarKind::U8
    } else if ty == std::any::TypeId::of::<i16>() {
        JetSharedScalarKind::I16
    } else if ty == std::any::TypeId::of::<u16>() {
        JetSharedScalarKind::U16
    } else if ty == std::any::TypeId::of::<i32>() {
        JetSharedScalarKind::I32
    } else if ty == std::any::TypeId::of::<u32>() {
        JetSharedScalarKind::U32
    } else if ty == std::any::TypeId::of::<i64>() {
        JetSharedScalarKind::I64
    } else if ty == std::any::TypeId::of::<u64>() {
        JetSharedScalarKind::U64
    } else if ty == std::any::TypeId::of::<f32>() {
        JetSharedScalarKind::F32
    } else if ty == std::any::TypeId::of::<f64>() {
        JetSharedScalarKind::F64
    } else if ty == std::any::TypeId::of::<bool>() {
        JetSharedScalarKind::Bool
    } else if ty == std::any::TypeId::of::<char>() {
        JetSharedScalarKind::Char
    } else {
        return None;
    })
}

#[derive(Default)]
struct JetSharedLockState {
    readers: usize,
    writer: bool,
    writers_waiting: usize,
}

pub struct JetSharedProtocol {
    state: std::sync::Mutex<JetSharedLockState>,
    wake: std::sync::Condvar,
}

/// One physical Shared owner that can participate in the canonical ordered
/// lock protocol. `owner_identity` deduplicates aliases; `protocol_order_key`
/// orders distinct owner permits.
pub trait JetSharedCanonicalOwner: Send + Sync + 'static {
    fn owner_identity(&self) -> usize;
    fn protocol_order_key(&self) -> usize;
    fn acquire_permit(
        self: std::sync::Arc<Self>,
        editable: bool,
    ) -> Result<std::sync::Arc<dyn JetSharedCanonicalPermit>, String>;
}
/// The one-shot, owner-neutral completion of a managed payload finalizer.
pub type JetSharedPhysicalCompletion = Box<dyn std::any::Any + Send + 'static>;

/// Result of a physical Shared operation. Body status and a ready linear
/// finalizer completion are independent, so an operation error cannot lose a
/// completion already produced while releasing its last borrow.
#[must_use = "consume both the operation result and any linear finalizer completion"]
pub struct JetSharedPhysicalOperationOutcome<T> {
    pub result: Result<T, String>,
    pub completion: Option<JetSharedPhysicalCompletion>,
}

impl<T> JetSharedPhysicalOperationOutcome<T> {
    pub fn new(
        result: Result<T, String>,
        completion: Option<JetSharedPhysicalCompletion>,
    ) -> Self {
        Self { result, completion }
    }
}

pub type JetSharedPhysicalPayloadFinalizer<T> =
    Box<dyn FnOnce(T) -> JetSharedPhysicalCompletion + Send + 'static>;
pub type JetSharedPhysicalCompletionDelivery = std::sync::Arc<
    dyn Fn(JetSharedPhysicalCompletion) + Send + Sync + 'static,
>;

/// One managed physical-payload finalizer and its real receiver for implicit
/// Rust Drop paths. Explicit physical releases return the completion; callers
/// forward it through this same receiver when it is not returned farther.
#[must_use = "install or invoke the finalizer binding with its completion receiver"]
pub struct JetSharedPhysicalFinalizerBinding<T> {
    finalize: JetSharedPhysicalPayloadFinalizer<T>,
    deliver: JetSharedPhysicalCompletionDelivery,
}

impl<T> JetSharedPhysicalFinalizerBinding<T> {
    pub fn new(
        finalize: JetSharedPhysicalPayloadFinalizer<T>,
        deliver: JetSharedPhysicalCompletionDelivery,
    ) -> Self {
        Self { finalize, deliver }
    }

    pub fn completion_delivery(&self) -> JetSharedPhysicalCompletionDelivery {
        self.deliver.clone()
    }

    pub fn deliver(&self, completion: JetSharedPhysicalCompletion) {
        (self.deliver)(completion);
    }

    pub fn finish(self, value: T) -> JetSharedPhysicalCompletion {
        (self.finalize)(value)
    }

    pub fn finish_and_deliver(self, value: T) {
        (self.deliver)((self.finalize)(value));
    }
}

/// Backend-neutral physical Shared ownership hooks. Pins preserve the cell
/// allocation but do not count as owners; alias reservations atomically
/// increment the canonical logical-owner count and release exactly once.
pub(crate) trait JetSharedPhysicalOwnerApi: Clone + Send + Sync + 'static {
    type Root: Send + Sync + 'static;
    type Pin: std::any::Any + Send + Sync + 'static;
    type Borrow: std::any::Any + Send + Sync + 'static;
    type Alias: std::any::Any + Send + Sync + 'static;
    type PhysicalGuard: 'static;

    fn owner_identity(&self) -> usize;
    fn protocol_order_key(&self) -> usize;
    fn logical_owner_count(&self) -> Result<usize, String>;
    fn pin_live_owner(&self) -> Result<Option<Self::Pin>, String>;
    fn reserve_logical_alias(&self) -> Result<Option<Self::Alias>, String>;
    fn alias_token_id(&self, alias: &Self::Alias) -> i64;
    fn install_payload_finalizer(
        &self,
        finalizer: &mut Option<JetSharedPhysicalFinalizerBinding<Self::Root>>,
    ) -> Result<(), String>;
    fn release_owner_alias(
        &self,
        alias: Self::Alias,
    ) -> JetSharedPhysicalOperationOutcome<()>;
    fn release_physical_borrow(
        &self,
        borrow: Self::Borrow,
    ) -> JetSharedPhysicalOperationOutcome<()>;
    fn release_physical_guard(
        &self,
        guard: Self::PhysicalGuard,
    ) -> JetSharedPhysicalOperationOutcome<()>;
    fn with_live_root<R>(
        &self,
        callback: impl FnOnce(&Self::Root, u64) -> R,
    ) -> JetSharedPhysicalOperationOutcome<Option<R>>;
    fn with_live_edit<R>(
        &self,
        callback: impl FnOnce(&mut Self::Root) -> R,
    ) -> JetSharedPhysicalOperationOutcome<Option<R>>;
    fn replace_if_revision(
        &self,
        expected_revision: u64,
        replacement: Self::Root,
    ) -> JetSharedPhysicalOperationOutcome<Option<(bool, u64)>>
    where
        Self::Root: Clone;
    fn acquire_physical_guard(
        &self,
        editable: bool,
    ) -> JetSharedPhysicalOperationOutcome<Option<Self::PhysicalGuard>>;
}


/// Invocation-local lease on one canonical physical owner permit.
///
/// Implementations may hold non-Send runtime guards; the lease is never
/// retained in a shared owner carrier. External participants use `stage_value`
/// to prepare publication through this already-held permit. If any participant
/// fails to stage, `discard_staged` must leave the owner unchanged before any
/// permit is released.
pub trait JetSharedCanonicalPermit: std::any::Any {
    fn editable(&self) -> bool;
    fn held(&self) -> bool;
    fn release(&self) -> JetSharedPhysicalOperationOutcome<()>;
    fn release_during_drop(&self);
    fn reacquire(&self, cancelled: &mut dyn FnMut() -> bool) -> bool;
    fn stage_value(&self, _value: Box<dyn std::any::Any>) -> Result<(), String> {
        Err("canonical Shared owner does not support typed transaction staging".to_string())
    }
    fn discard_staged(&self) -> Result<(), String> {
        Ok(())
    }
    fn into_any(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn std::any::Any>;
}

impl JetSharedProtocol {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            state: std::sync::Mutex::new(JetSharedLockState::default()),
            wake: std::sync::Condvar::new(),
        })
    }

    pub fn acquire(
        self: &std::sync::Arc<Self>,
        editable: bool,
        mut cancelled: impl FnMut() -> bool,
    ) -> Option<std::sync::Arc<JetSharedPermit>> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if editable {
            state.writers_waiting += 1;
        }
        loop {
            if cancelled() {
                if editable {
                    state.writers_waiting -= 1;
                    self.wake.notify_all();
                }
                return None;
            }
            let available = if editable {
                !state.writer && state.readers == 0
            } else {
                !state.writer && state.writers_waiting == 0
            };
            if available {
                if editable {
                    state.writers_waiting -= 1;
                    state.writer = true;
                } else {
                    state.readers += 1;
                }
                return Some(std::sync::Arc::new(JetSharedPermit {
                    protocol: self.clone(),
                    editable,
                    held: std::sync::atomic::AtomicBool::new(true),
                }));
            }
            let (next, _) = self
                .wake
                .wait_timeout(state, std::time::Duration::from_millis(10))
                .unwrap_or_else(|error| error.into_inner());
            state = next;
        }
    }
}

impl JetSharedCanonicalOwner for JetSharedProtocol {
    fn owner_identity(&self) -> usize {
        self as *const Self as usize
    }

    fn protocol_order_key(&self) -> usize {
        self as *const Self as usize
    }

    fn acquire_permit(
        self: std::sync::Arc<Self>,
        editable: bool,
    ) -> Result<std::sync::Arc<dyn JetSharedCanonicalPermit>, String> {
        self.acquire(editable, || false)
            .map(|permit| permit as std::sync::Arc<dyn JetSharedCanonicalPermit>)
            .ok_or_else(|| "canonical Shared permit acquisition was cancelled".to_string())
    }
}

pub fn jet_shared_acquire(
    protocol: &std::sync::Arc<JetSharedProtocol>,
    editable: bool,
    cancelled: impl FnMut() -> bool,
) -> Option<std::sync::Arc<JetSharedPermit>> {
    protocol.acquire(editable, cancelled)
}

/// Acquire one physical owner permit per root in canonical protocol order.
/// Duplicate aliases share the same lease while each row preserves its
/// original participant index and requested editable capability.
pub fn jet_shared_acquire_ordered_owners(
    mut participants: Vec<(
        usize,
        std::sync::Arc<dyn JetSharedCanonicalOwner>,
        bool,
    )>,
) -> Result<
    Vec<(
        usize,
        bool,
        std::sync::Arc<dyn JetSharedCanonicalPermit>,
    )>,
    String,
> {
    let mut ordered = participants
        .drain(..)
        .map(|(index, owner, editable)| {
            (
                index,
                owner.owner_identity(),
                owner.protocol_order_key(),
                editable,
                owner,
            )
        })
        .collect::<Vec<_>>();
    ordered.sort_unstable_by_key(|(index, identity, order_key, _, _)| {
        (*order_key, *identity, *index)
    });

    let mut acquired = Vec::with_capacity(ordered.len());
    let mut start = 0;
    while start < ordered.len() {
        let (_, owner_identity, order_key, first_editable, owner) = &ordered[start];
        let mut end = start + 1;
        let mut editable = *first_editable;
        while end < ordered.len() && ordered[end].1 == *owner_identity {
            if ordered[end].2 != *order_key {
                return Err(
                    "one physical Shared owner reported inconsistent protocol ordering keys"
                        .to_string(),
                );
            }
            editable |= ordered[end].3;
            end += 1;
        }
        let permit = owner.clone().acquire_permit(editable)?;
        for (index, _, _, requested_editable, _) in &ordered[start..end] {
            acquired.push((*index, *requested_editable, std::sync::Arc::clone(&permit)));
        }
        start = end;
    }
    Ok(acquired)
}

pub fn jet_shared_acquire_ordered(
    protocols: Vec<std::sync::Arc<JetSharedProtocol>>,
) -> Vec<std::sync::Arc<JetSharedPermit>> {
    let participants = protocols
        .into_iter()
        .enumerate()
        .map(|(index, protocol)| {
            (
                index,
                protocol as std::sync::Arc<dyn JetSharedCanonicalOwner>,
                true,
            )
        })
        .collect();
    let rows = jet_shared_acquire_ordered_owners(participants)
        .expect("uncancelled transaction lock acquisition succeeds");
    let mut acquired: Vec<std::sync::Arc<dyn JetSharedCanonicalPermit>> =
        Vec::with_capacity(rows.len());
    let mut permits = Vec::new();
    for (_, _, permit) in rows {
        if acquired
            .last()
            .is_some_and(|previous| std::sync::Arc::ptr_eq(previous, &permit))
        {
            continue;
        }
        let erased = permit.clone().into_any();
        let permit = match std::sync::Arc::downcast::<JetSharedPermit>(erased) {
            Ok(permit) => permit,
            Err(_) => panic!("local Shared protocol returned a foreign canonical permit"),
        };
        acquired.push(permit.clone());
        permits.push(permit);
    }
    permits
}

/// The Shared side of a `#Transact` block.
///
/// Engines and generated adapters only supply type-erased payload closures.
/// Participant identity, canonical lock ordering, commit, rollback, and the
/// transaction-local view live here so every execution tier uses one protocol.
///
/// A transaction stages each edited payload in a private working value.  The
/// edit callback runs once while the transaction body is executing; commit
/// only publishes that working value.  Nested transactions clone the parent's
/// working value, merge a committed child back into the parent, and never
/// publish to a Shared cell until the outermost transaction commits.
trait JetSharedTxnValue {
    fn as_any(&self) -> &dyn std::any::Any;

    fn merge_into(&self, parent: &dyn JetSharedTxnValue);
}

struct JetSharedTxnValueImpl<T: 'static> {
    value: std::rc::Rc<std::cell::RefCell<T>>,
}

impl<T: Clone + 'static> JetSharedTxnValue for JetSharedTxnValueImpl<T> {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn merge_into(&self, parent: &dyn JetSharedTxnValue) {
        let parent = parent
            .as_any()
            .downcast_ref::<Self>()
            .expect("Shared transaction participant type changed");
        let value = self.value.borrow().clone();
        *parent.value.borrow_mut() = value;
    }
}

struct JetSharedTransactionPart {
    protocol: std::sync::Arc<dyn JetSharedCanonicalOwner>,
    staged: Option<Box<dyn JetSharedTxnValue>>,
    writes: bool,
    snapshots: Vec<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    deltas: Vec<Box<dyn FnOnce()>>,
    stage_hooks: Vec<
        Box<dyn FnOnce(&dyn JetSharedCanonicalPermit) -> Result<(), String>>,
    >,
    commit_hooks: Vec<Box<dyn FnOnce(&dyn JetSharedCanonicalPermit)>>,
    has_commit_hook: bool,
}
impl JetSharedTransactionPart {
    fn new(protocol: std::sync::Arc<dyn JetSharedCanonicalOwner>) -> Self {
        Self {
            protocol,
            staged: None,
            writes: false,
            snapshots: Vec::new(),
            deltas: Vec::new(),
            stage_hooks: Vec::new(),
            commit_hooks: Vec::new(),
            has_commit_hook: false,
        }
    }
}

struct JetSharedTransactionState {
    parts: Option<Vec<JetSharedTransactionPart>>,
    rollback_hooks: Option<Vec<Box<dyn FnOnce()>>>,
    parent: Option<std::rc::Weak<std::cell::RefCell<JetSharedTransactionState>>>,
}

thread_local! {
    static JET_SHARED_TRANSACTION_STACK:
        std::cell::RefCell<Vec<std::rc::Weak<std::cell::RefCell<JetSharedTransactionState>>>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

pub struct JetSharedTransaction {
    state: std::rc::Rc<std::cell::RefCell<JetSharedTransactionState>>,
}

fn jet_shared_transaction_part_mut<'a>(
    parts: &'a mut Vec<JetSharedTransactionPart>,
    protocol: &std::sync::Arc<dyn JetSharedCanonicalOwner>,
) -> &'a mut JetSharedTransactionPart {
    if let Some(index) = parts
        .iter()
        .position(|part| part.protocol.owner_identity() == protocol.owner_identity())
    {
        assert_eq!(
            parts[index].protocol.protocol_order_key(),
            protocol.protocol_order_key(),
            "one physical Shared owner reported inconsistent protocol ordering keys",
        );
        return &mut parts[index];
    }
    parts.push(JetSharedTransactionPart::new(protocol.clone()));
    parts.last_mut().expect("new Shared transaction participant")
}

fn jet_shared_transaction_part<'a>(
    parts: &'a [JetSharedTransactionPart],
    protocol: &std::sync::Arc<dyn JetSharedCanonicalOwner>,
) -> Option<&'a JetSharedTransactionPart> {
    parts
        .iter()
        .find(|part| part.protocol.owner_identity() == protocol.owner_identity())
}

fn jet_shared_transaction_staged_from<T: 'static>(
    state: &std::rc::Rc<std::cell::RefCell<JetSharedTransactionState>>,
    protocol: &std::sync::Arc<dyn JetSharedCanonicalOwner>,
) -> Option<std::rc::Rc<std::cell::RefCell<T>>> {
    let (staged, parent) = {
        let state = state.borrow();
        let staged = state
            .parts
            .as_deref()
            .and_then(|parts| jet_shared_transaction_part(parts, protocol))
            .and_then(|part| part.staged.as_ref())
            .and_then(|staged| {
                staged
                    .as_any()
                    .downcast_ref::<JetSharedTxnValueImpl<T>>()
                    .map(|staged| staged.value.clone())
            });
        (staged, state.parent.clone())
    };
    staged.or_else(|| {
        parent
            .and_then(|parent| parent.upgrade())
            .and_then(|parent| jet_shared_transaction_staged_from(&parent, protocol))
    })
}

fn jet_shared_transaction_has_writes(
    state: &std::rc::Rc<std::cell::RefCell<JetSharedTransactionState>>,
    protocol: &std::sync::Arc<dyn JetSharedCanonicalOwner>,
) -> bool {
    let (writes, parent) = {
        let state = state.borrow();
        let writes = state
            .parts
            .as_deref()
            .and_then(|parts| jet_shared_transaction_part(parts, protocol))
            .is_some_and(|part| part.writes);
        (writes, state.parent.clone())
    };
    writes
        || parent.is_some_and(|parent| {
            parent
                .upgrade()
                .is_some_and(|parent| jet_shared_transaction_has_writes(&parent, protocol))
        })
}

fn jet_shared_transaction_pop(
    state: &std::rc::Rc<std::cell::RefCell<JetSharedTransactionState>>,
) {
    JET_SHARED_TRANSACTION_STACK.with(|stack| {
        let mut stack = stack.borrow_mut();
        if let Some(index) = stack.iter().rposition(|entry| {
            entry
                .upgrade()
                .is_some_and(|candidate| std::rc::Rc::ptr_eq(&candidate, state))
        }) {
            stack.remove(index);
        }
    });
}

fn jet_shared_transaction_merge_nested(
    parent: &std::rc::Rc<std::cell::RefCell<JetSharedTransactionState>>,
    mut child_parts: Vec<JetSharedTransactionPart>,
    mut child_rollbacks: Vec<Box<dyn FnOnce()>>,
) {
    let mut parent = parent.borrow_mut();
    let parts = parent
        .parts
        .as_mut()
        .expect("nested Shared transaction parent already committed");
    for mut child in child_parts.drain(..) {
        let parent_part = jet_shared_transaction_part_mut(parts, &child.protocol);
        if child.writes {
            for snapshot in &parent_part.snapshots {
                snapshot.store(false, std::sync::atomic::Ordering::Release);
            }
        }
        if let Some(staged) = child.staged.take() {
            if let Some(parent_staged) = parent_part.staged.as_ref() {
                staged.merge_into(parent_staged.as_ref());
            } else {
                parent_part.staged = Some(staged);
            }
        }
        parent_part.writes |= child.writes;
        parent_part.deltas.append(&mut child.deltas);
        if !parent_part.has_commit_hook {
            parent_part.stage_hooks.append(&mut child.stage_hooks);
            parent_part.commit_hooks.append(&mut child.commit_hooks);
            parent_part.has_commit_hook = child.has_commit_hook;
        }
        parent_part.snapshots.append(&mut child.snapshots);
    }
    parent
        .rollback_hooks
        .as_mut()
        .expect("nested Shared transaction parent has no rollback hooks")
        .append(&mut child_rollbacks);
}

pub fn jet_shared_transaction_begin() -> JetSharedTransaction {
    let parent = JET_SHARED_TRANSACTION_STACK.with(|stack| {
        stack
            .borrow()
            .last()
            .and_then(|parent| parent.upgrade())
            .map(|parent| std::rc::Rc::downgrade(&parent))
    });
    let state = std::rc::Rc::new(std::cell::RefCell::new(JetSharedTransactionState {
        parts: Some(Vec::new()),
        rollback_hooks: Some(Vec::new()),
        parent,
    }));
    JET_SHARED_TRANSACTION_STACK.with(|stack| {
        stack.borrow_mut().push(std::rc::Rc::downgrade(&state));
    });
    JetSharedTransaction { state }
}

impl JetSharedTransaction {
    pub fn touch(&mut self, protocol: std::sync::Arc<dyn JetSharedCanonicalOwner>) {
        let mut state = self.state.borrow_mut();
        let parts = state
            .parts
            .as_mut()
            .expect("Shared transaction touch after commit");
        let _ = jet_shared_transaction_part_mut(parts, &protocol);
    }

    /// Return a transaction-local working value, creating it from `initial`
    /// only when neither this transaction nor its parent has staged a value.
    pub fn stage_value<T: Clone + 'static>(
        &mut self,
        protocol: std::sync::Arc<dyn JetSharedCanonicalOwner>,
        initial: impl FnOnce() -> T,
    ) -> std::rc::Rc<std::cell::RefCell<T>> {
        {
            let state = self.state.borrow();
            if let Some(staged) = state
                .parts
                .as_deref()
                .and_then(|parts| jet_shared_transaction_part(parts, &protocol))
                .and_then(|part| part.staged.as_ref())
                .and_then(|staged| {
                    staged
                        .as_any()
                        .downcast_ref::<JetSharedTxnValueImpl<T>>()
                        .map(|staged| staged.value.clone())
                })
            {
                return staged;
            }
        }
        let value = self
            .staged_value::<T>(&protocol)
            .map(|staged| staged.borrow().clone())
            .unwrap_or_else(initial);
        let staged = std::rc::Rc::new(std::cell::RefCell::new(value));
        let mut state = self.state.borrow_mut();
        let parts = state
            .parts
            .as_mut()
            .expect("Shared transaction stage after commit");
        let part = jet_shared_transaction_part_mut(parts, &protocol);
        part.staged = Some(Box::new(JetSharedTxnValueImpl {
            value: staged.clone(),
        }));
        staged
    }

    pub fn staged_value<T: 'static>(
        &self,
        protocol: &std::sync::Arc<dyn JetSharedCanonicalOwner>,
    ) -> Option<std::rc::Rc<std::cell::RefCell<T>>> {
        jet_shared_transaction_staged_from::<T>(&self.state, protocol)
    }

    /// Mark a participant as written and invalidate tickets captured before
    /// this local write. The outermost commit still advances one revision.
    pub fn mark_write(&mut self, protocol: std::sync::Arc<dyn JetSharedCanonicalOwner>) {
        let mut state = self.state.borrow_mut();
        let parts = state
            .parts
            .as_mut()
            .expect("Shared transaction write after commit");
        let part = jet_shared_transaction_part_mut(parts, &protocol);
        part.writes = true;
        for snapshot in &part.snapshots {
            snapshot.store(false, std::sync::atomic::Ordering::Release);
        }
    }

    pub fn snapshot_revision(
        &self,
        protocol: &std::sync::Arc<dyn JetSharedCanonicalOwner>,
        committed: u64,
    ) -> Option<u64> {
        if jet_shared_transaction_has_writes(&self.state, protocol) {
            committed.checked_add(1)
        } else {
            Some(committed)
        }
    }

    /// Register a snapshot with the participant so abort invalidates it and
    /// nested commit can carry its lifecycle into the parent transaction.
    pub fn record_snapshot(
        &mut self,
        protocol: std::sync::Arc<dyn JetSharedCanonicalOwner>,
        valid: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        self.touch(protocol.clone());
        {
            let mut state = self.state.borrow_mut();
            let parts = state
                .parts
                .as_mut()
                .expect("Shared transaction snapshot after commit");
            jet_shared_transaction_part_mut(parts, &protocol)
                .snapshots
                .push(valid.clone());
        }
        self.record_rollback(Box::new(move || {
            valid.store(false, std::sync::atomic::Ordering::Release);
        }));
    }

    pub fn record_edit(
        &mut self,
        protocol: std::sync::Arc<dyn JetSharedCanonicalOwner>,
        delta: Box<dyn FnOnce()>,
    ) {
        let mut state = self.state.borrow_mut();
        let parts = state
            .parts
            .as_mut()
            .expect("Shared transaction edit after commit");
        jet_shared_transaction_part_mut(parts, &protocol)
            .deltas
            .push(delta);
    }

    /// Register the prepare and publish hooks for a participant. Preparation
    /// runs for every owner under its canonical permit before any commit hook
    /// publishes. The first pair wins because a participant publishes once,
    /// while its transaction-local deltas may accumulate.
    pub fn record_edit_with_commit(
        &mut self,
        protocol: std::sync::Arc<dyn JetSharedCanonicalOwner>,
        delta: Box<dyn FnOnce()>,
        commit: Box<dyn FnOnce(&dyn JetSharedCanonicalPermit)>,
    ) {
        self.record_edit_with_staged_commit(protocol, delta, Box::new(|_| Ok(())), commit);
    }

    /// Register one fallible staging hook and one infallible publication hook.
    /// If staging fails, every acquired permit is asked to discard its staged
    /// value before any owner is allowed to publish.
    pub fn record_edit_with_staged_commit(
        &mut self,
        protocol: std::sync::Arc<dyn JetSharedCanonicalOwner>,
        delta: Box<dyn FnOnce()>,
        stage: Box<
            dyn FnOnce(&dyn JetSharedCanonicalPermit) -> Result<(), String>,
        >,
        commit: Box<dyn FnOnce(&dyn JetSharedCanonicalPermit)>,
    ) {
        let mut state = self.state.borrow_mut();
        let parts = state
            .parts
            .as_mut()
            .expect("Shared transaction edit after commit");
        let part = jet_shared_transaction_part_mut(parts, &protocol);
        part.deltas.push(delta);
        if !part.has_commit_hook {
            part.stage_hooks.push(stage);
            part.commit_hooks.push(commit);
            part.has_commit_hook = true;
        }
    }

    pub fn record_rollback(&mut self, hook: Box<dyn FnOnce()>) {
        self.state
            .borrow_mut()
            .rollback_hooks
            .as_mut()
            .expect("Shared transaction rollback after commit")
            .push(hook);
    }

    pub fn commit(self) {
        self.commit_with(|| ());
    }

    pub fn commit_with<R>(self, apply: impl FnOnce() -> R) -> R {
        self.try_commit_with(apply)
            .unwrap_or_else(|error| panic!("Shared transaction commit failed: {error}"))
    }

    /// Fallible counterpart to `commit_with`, used by adapters whose physical
    /// owner can reject a prepared value. No participant publishes unless all
    /// staging hooks succeed under the complete canonical permit set.
    pub fn try_commit_with<R>(
        self,
        apply: impl FnOnce() -> R,
    ) -> Result<R, String> {
        let state = self.state.clone();
        let (parts, rollback_hooks, parent) = {
            let mut state = state.borrow_mut();
            (
                state.parts.take(),
                state.rollback_hooks.take(),
                state.parent.clone(),
            )
        };
        jet_shared_transaction_pop(&state);
        let Some(mut parts) = parts else {
            return Ok(apply());
        };
        let mut rollback_hooks = rollback_hooks.unwrap_or_default();
        if let Some(parent) = parent.and_then(|parent| parent.upgrade()) {
            jet_shared_transaction_merge_nested(&parent, parts, rollback_hooks);
            return Ok(apply());
        }
        let owners = parts
            .iter()
            .enumerate()
            .map(|(index, part)| (index, part.protocol.clone(), true))
            .collect();
        let leases = match jet_shared_acquire_ordered_owners(owners) {
            Ok(leases) => leases,
            Err(error) => {
                for hook in rollback_hooks.drain(..) {
                    hook();
                }
                return Err(format!(
                    "canonical Shared transaction owner acquisition failed: {error}"
                ));
            }
        };
        let mut permits = vec![None; parts.len()];
        for (index, _, permit) in leases {
            permits[index] = Some(permit);
        }
        for part in &mut parts {
            for delta in part.deltas.drain(..) {
                delta();
            }
        }
        let stage_result = (|| {
            for (index, part) in parts.iter_mut().enumerate() {
                let permit = permits[index]
                    .as_ref()
                    .expect("ordered Shared acquisition omitted a participant");
                for stage in part.stage_hooks.drain(..) {
                    stage(permit.as_ref())?;
                }
            }
            Ok::<(), String>(())
        })();
        if let Err(error) = stage_result {
            let mut discard_errors = Vec::new();
            for permit in permits.iter().flatten() {
                if let Err(discard_error) = permit.discard_staged() {
                    discard_errors.push(discard_error);
                }
            }
            drop(permits);
            for hook in rollback_hooks.drain(..) {
                hook();
            }
            if !discard_errors.is_empty() {
                return Err(format!(
                    "{error}; failed to discard staged Shared values: {}",
                    discard_errors.join("; ")
                ));
            }
            return Err(error);
        }
        for (index, part) in parts.iter_mut().enumerate() {
            let permit = permits[index]
                .as_ref()
                .expect("ordered Shared acquisition omitted a participant");
            for commit in part.commit_hooks.drain(..) {
                commit(permit.as_ref());
            }
        }
        drop(permits);
        Ok(apply())
    }
}


impl Drop for JetSharedTransaction {
    fn drop(&mut self) {
        let state = self.state.clone();
        let rollback_hooks = {
            let mut state = state.borrow_mut();
            if state.parts.take().is_some() {
                state.rollback_hooks.take()
            } else {
                None
            }
        };
        if let Some(mut hooks) = rollback_hooks {
            jet_shared_transaction_pop(&state);
            for hook in hooks.drain(..) {
                hook();
            }
        }
    }
}

pub struct JetSharedPermit {
    protocol: std::sync::Arc<JetSharedProtocol>,
    editable: bool,
    held: std::sync::atomic::AtomicBool,
}

impl JetSharedPermit {
    pub fn editable(&self) -> bool {
        self.editable
    }

    pub fn held(&self) -> bool {
        self.held.load(std::sync::atomic::Ordering::Acquire)
    }

    pub fn release(&self) {
        if !self
            .held
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            return;
        }
        let mut state = self
            .protocol
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.editable {
            state.writer = false;
        } else {
            state.readers = state.readers.saturating_sub(1);
        }
        self.protocol.wake.notify_all();
    }

    pub fn reacquire(&self, mut cancelled: impl FnMut() -> bool) -> bool {
        if self.held() {
            return true;
        }
        let mut state = self
            .protocol
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if self.editable {
            state.writers_waiting += 1;
        }
        loop {
            if cancelled() {
                if self.editable {
                    state.writers_waiting -= 1;
                    self.protocol.wake.notify_all();
                }
                return false;
            }
            let available = if self.editable {
                !state.writer && state.readers == 0
            } else {
                !state.writer && state.writers_waiting == 0
            };
            if available {
                if self.editable {
                    state.writers_waiting -= 1;
                    state.writer = true;
                } else {
                    state.readers += 1;
                }
                self.held
                    .store(true, std::sync::atomic::Ordering::Release);
                return true;
            }
            let (next, _) = self
                .protocol
                .wake
                .wait_timeout(state, std::time::Duration::from_millis(10))
                .unwrap_or_else(|error| error.into_inner());
            state = next;
        }
    }
}
impl JetSharedCanonicalPermit for JetSharedPermit {
    fn editable(&self) -> bool {
        JetSharedPermit::editable(self)
    }

    fn held(&self) -> bool {
        JetSharedPermit::held(self)
    }

    fn release(&self) -> JetSharedPhysicalOperationOutcome<()> {
        JetSharedPermit::release(self);
        JetSharedPhysicalOperationOutcome::new(Ok(()), None)
    }

    fn release_during_drop(&self) {
        JetSharedPermit::release(self);
    }

    fn reacquire(&self, cancelled: &mut dyn FnMut() -> bool) -> bool {
        JetSharedPermit::reacquire(self, || cancelled())
    }

    fn into_any(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn std::any::Any> {
        self
    }
}

pub struct JetSharedGuardState {
    permit: std::sync::Arc<dyn JetSharedCanonicalPermit>,
    path: Vec<i64>,
    editable: bool,
    active: std::sync::atomic::AtomicBool,
}

impl JetSharedGuardState {
    pub fn permit_arc(&self) -> std::sync::Arc<dyn JetSharedCanonicalPermit> {
        self.permit.clone()
    }

    pub fn permit(&self) -> &dyn JetSharedCanonicalPermit {
        self.permit.as_ref()
    }

    pub fn editable(&self) -> bool {
        self.editable
    }

    pub fn held(&self) -> bool {
        self.active.load(std::sync::atomic::Ordering::Acquire) && self.permit.held()
    }

    pub fn path(&self) -> &[i64] {
        &self.path
    }
}

/// Build the checked path-tracking guard state around an already-acquired
/// canonical physical owner permit. This does not acquire a shell protocol.
pub fn jet_shared_guard_state_from_permit(
    permit: std::sync::Arc<dyn JetSharedCanonicalPermit>,
    editable: bool,
) -> Result<std::sync::Arc<JetSharedGuardState>, &'static str> {
    if !permit.held() {
        return Err(JET_SHARED_GUARD_INVALID);
    }
    if editable && !permit.editable() {
        return Err(JET_SHARED_GUARD_EDIT_REQUIRED);
    }
    Ok(std::sync::Arc::new(JetSharedGuardState {
        permit,
        path: Vec::new(),
        editable,
        active: std::sync::atomic::AtomicBool::new(true),
    }))
}

impl Drop for JetSharedPermit {
    fn drop(&mut self) {
        self.release();
    }
}


pub fn jet_shared_guard_acquire(
    protocol: &std::sync::Arc<JetSharedProtocol>,
    editable: bool,
    cancelled: impl FnMut() -> bool,
) -> Option<std::sync::Arc<JetSharedGuardState>> {
    jet_shared_acquire(protocol, editable, cancelled).map(|permit| {
        let permit: std::sync::Arc<dyn JetSharedCanonicalPermit> = permit;
        jet_shared_guard_state_from_permit(permit, editable)
            .expect("new Shared guard permit is held with matching capability")
    })
}

/// Acquire owner permits through the same canonical owner ordering used by
/// transactions, then return one checked path state per original participant.
pub fn jet_shared_guard_acquire_ordered(
    participants: Vec<(usize, std::sync::Arc<JetSharedProtocol>, bool)>,
) -> Vec<(usize, std::sync::Arc<JetSharedGuardState>)> {
    let owners = participants
        .into_iter()
        .map(|(index, protocol, editable)| {
            (
                index,
                protocol as std::sync::Arc<dyn JetSharedCanonicalOwner>,
                editable,
            )
        })
        .collect();
    jet_shared_acquire_ordered_owners(owners)
        .expect("uncancelled ordered Shared owner acquisition succeeds")
        .into_iter()
        .map(|(index, editable, permit)| {
            (
                index,
                jet_shared_guard_state_from_permit(permit, editable)
                    .expect("ordered Shared permit is held with matching capability"),
            )
        })
        .collect()
}

pub fn jet_shared_guard_map(
    guard: &JetSharedGuardState,
    field: i64,
    editable: bool,
) -> Result<std::sync::Arc<JetSharedGuardState>, &'static str> {
    jet_shared_guard_map_path(guard, &[field], editable)
}

/// Map a checked guard through a complete stored-field suffix while retaining
/// its canonical permit and consuming the parent only after child creation.
pub fn jet_shared_guard_map_path(
    guard: &JetSharedGuardState,
    fields: &[i64],
    editable: bool,
) -> Result<std::sync::Arc<JetSharedGuardState>, &'static str> {
    if !guard.held() {
        return Err(JET_SHARED_GUARD_INVALID);
    }
    if editable {
        jet_shared_guard_require_edit_capability(guard.editable(), guard.permit())?;
    }

    let mapped = std::sync::Arc::new(JetSharedGuardState {
        permit: std::sync::Arc::clone(&guard.permit),
        path: jet_shared_guard_path_with_suffix(guard, fields),
        editable,
        active: std::sync::atomic::AtomicBool::new(true),
    });
    guard
        .active
        .store(false, std::sync::atomic::Ordering::Release);
    Ok(mapped)
}

/// Split one checked guard into two disjoint projections while retaining the
/// original permit. Sema proves both field identities are stored and disjoint;
/// this protocol helper only records paths and consumes the parent after both
/// child states have been created.
pub fn jet_shared_guard_split(
    guard: &JetSharedGuardState,
    first: i64,
    second: i64,
    editable: bool,
) -> Result<
    (
        std::sync::Arc<JetSharedGuardState>,
        std::sync::Arc<JetSharedGuardState>,
    ),
    &'static str,
> {
    jet_shared_guard_split_path(guard, &[first], &[second], editable)
}

/// Split a checked guard through two complete stored-field suffixes. Both
/// children retain the same permit and inherit the source path.
pub fn jet_shared_guard_split_path(
    guard: &JetSharedGuardState,
    first_fields: &[i64],
    second_fields: &[i64],
    editable: bool,
) -> Result<
    (
        std::sync::Arc<JetSharedGuardState>,
        std::sync::Arc<JetSharedGuardState>,
    ),
    &'static str,
> {
    if !guard.held() {
        return Err(JET_SHARED_GUARD_INVALID);
    }
    if editable {
        jet_shared_guard_require_edit_capability(guard.editable(), guard.permit())?;
    }

    let first = std::sync::Arc::new(JetSharedGuardState {
        permit: std::sync::Arc::clone(&guard.permit),
        path: jet_shared_guard_path_with_suffix(guard, first_fields),
        editable,
        active: std::sync::atomic::AtomicBool::new(true),
    });
    let second = std::sync::Arc::new(JetSharedGuardState {
        permit: std::sync::Arc::clone(&guard.permit),
        path: jet_shared_guard_path_with_suffix(guard, second_fields),
        editable,
        active: std::sync::atomic::AtomicBool::new(true),
    });
    guard
        .active
        .store(false, std::sync::atomic::Ordering::Release);
    Ok((first, second))
}

fn jet_shared_guard_path_with_suffix(
    guard: &JetSharedGuardState,
    suffix: &[i64],
) -> Vec<i64> {
    let mut path = Vec::with_capacity(guard.path.len() + suffix.len());
    path.extend_from_slice(&guard.path);
    path.extend_from_slice(suffix);
    path
}

pub fn jet_shared_guard_clone(
    guard: &JetSharedGuardState,
    editable: bool,
) -> Result<std::sync::Arc<JetSharedGuardState>, &'static str> {
    if !guard.held() {
        return Err(JET_SHARED_GUARD_INVALID);
    }
    if editable {
        jet_shared_guard_require_edit_capability(guard.editable(), guard.permit())?;
    }

    Ok(std::sync::Arc::new(JetSharedGuardState {
        permit: std::sync::Arc::clone(&guard.permit),
        path: guard.path.clone(),
        editable,
        active: std::sync::atomic::AtomicBool::new(true),
    }))
}

pub fn jet_shared_guard_require_edit(
    guard: &JetSharedGuardState,
) -> Result<(), &'static str> {
    if !guard.held() {
        return Err(JET_SHARED_GUARD_INVALID);
    }
    jet_shared_guard_require_edit_capability(guard.editable(), guard.permit())
}

pub fn jet_shared_guard_require_edit_capability(
    editable: bool,
    permit: &dyn JetSharedCanonicalPermit,
) -> Result<(), &'static str> {
    if !permit.held() {
        return Err(JET_SHARED_GUARD_INVALID);
    }
    if !editable || !permit.editable() {
        return Err(JET_SHARED_GUARD_EDIT_REQUIRED);
    }
    Ok(())
}

pub trait JetConditionWaiter: Send + Sync {
    fn park(&self) -> Result<(), ()>;
    fn wake(&self);
    fn interrupted(&self) -> bool {
        false
    }
}

pub struct JetConditionProtocol {
    waiters: std::sync::Mutex<
        std::collections::VecDeque<(u64, std::sync::Arc<dyn JetConditionWaiter>)>,
    >,
    next_id: std::sync::atomic::AtomicU64,
    epoch: std::sync::atomic::AtomicU64,
    pending: std::sync::atomic::AtomicU64,
}

impl JetConditionProtocol {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self {
            waiters: std::sync::Mutex::new(std::collections::VecDeque::new()),
            next_id: std::sync::atomic::AtomicU64::new(1),
            epoch: std::sync::atomic::AtomicU64::new(0),
            pending: std::sync::atomic::AtomicU64::new(0),
        })
    }

    pub fn register(
        self: &std::sync::Arc<Self>,
        waiter: std::sync::Arc<dyn JetConditionWaiter>,
    ) -> JetConditionRegistration {
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut waiters = self
            .waiters
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let epoch = self
            .epoch
            .load(std::sync::atomic::Ordering::Relaxed);
        let stale = self
            .pending
            .fetch_update(
                std::sync::atomic::Ordering::Relaxed,
                std::sync::atomic::Ordering::Relaxed,
                |pending| {
                    if pending > 0 {
                        Some(pending - 1)
                    } else {
                        None
                    }
                },
            )
            .is_ok();
        waiters.push_back((id, waiter));
        JetConditionRegistration {
            condition: self.clone(),
            id,
            epoch,
            stale,
        }
    }

    pub fn notify_one(&self) {
        let waiter = {
            let mut waiters = self
                .waiters
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            self.epoch
                .fetch_add(1, std::sync::atomic::Ordering::Release);
            let waiter = waiters.pop_front().map(|(_, waiter)| waiter);
            if waiter.is_none() {
                self.pending.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            waiter
        };
        if let Some(waiter) = waiter {
            waiter.wake();
        }
    }

    pub fn notify_all(&self) {
        let waiters = {
            let mut registered = self
                .waiters
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            self.epoch
                .fetch_add(1, std::sync::atomic::Ordering::Release);
            let waiters = registered
                .drain(..)
                .map(|(_, waiter)| waiter)
                .collect::<Vec<_>>();
            if waiters.is_empty() {
                self.pending.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            waiters
        };
        for waiter in waiters {
            waiter.wake();
        }
    }

    fn unregister(&self, id: u64) {
        self.waiters
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|(candidate, _)| *candidate != id);
    }
}

pub fn jet_shared_condition_notify_one(
    condition: &std::sync::Arc<JetConditionProtocol>,
) {
    condition.notify_one();
}

pub fn jet_shared_condition_notify_all(
    condition: &std::sync::Arc<JetConditionProtocol>,
) {
    condition.notify_all();
}

pub struct JetConditionRegistration {
    condition: std::sync::Arc<JetConditionProtocol>,
    id: u64,
    epoch: u64,
    stale: bool,
}

impl JetConditionRegistration {
    fn saw_notification(&self) -> bool {
        self.stale
            || self
                .condition
                .epoch
                .load(std::sync::atomic::Ordering::Acquire)
                != self.epoch
    }
}

impl Drop for JetConditionRegistration {
    fn drop(&mut self) {
        self.condition.unregister(self.id);
    }
}

pub enum JetConditionWaitError<E> {
    Predicate(E),
    Cancelled,
}

/// Handoff coordinated with a condition wait while its notification is
/// registered. The physical owner publishes/releases before the local permit
/// is released, and reacquires before local permit restoration.
pub trait JetSharedWaitHandoff {
    fn suspend(&mut self) -> Result<(), ()>;
    fn resume(&mut self, cancelled: bool) -> Result<bool, ()>;
    fn abort(&mut self) -> Result<(), ()>;
}

struct JetConditionWaitCleanup<'a> {
    registration: Option<JetConditionRegistration>,
    permit: &'a dyn JetSharedCanonicalPermit,
    waiter: &'a dyn JetConditionWaiter,
    released: bool,
}

impl JetConditionWaitCleanup<'_> {
    fn release(&mut self) {
        self.permit.release_during_drop();
        self.released = true;
    }

    fn abandon(&mut self) {
        self.registration.take();
        self.released = false;
    }

    fn finish(&mut self) -> Result<(), ()> {
        self.registration.take();
        if self.released {
            self.released = false;
            let mut cancelled = || self.waiter.interrupted();
            if !self.permit.reacquire(&mut cancelled) {
                return Err(());
            }
        }
        Ok(())
    }
}

impl Drop for JetConditionWaitCleanup<'_> {
    fn drop(&mut self) {
        self.registration.take();
        if self.released {
            let mut cancelled = || self.waiter.interrupted();
            let _ = self.permit.reacquire(&mut cancelled);
        }
    }
}

fn jet_shared_condition_wait_registered(
    permit: &dyn JetSharedCanonicalPermit,
    registration: JetConditionRegistration,
    waiter: std::sync::Arc<dyn JetConditionWaiter>,
) -> Result<(), ()> {
    jet_shared_condition_wait_registered_with_handoff(permit, registration, waiter, None)
}

fn jet_shared_condition_wait_registered_with_handoff(
    permit: &dyn JetSharedCanonicalPermit,
    registration: JetConditionRegistration,
    waiter: std::sync::Arc<dyn JetConditionWaiter>,
    handoff: Option<&mut dyn JetSharedWaitHandoff>,
) -> Result<(), ()> {
    let mut cleanup = JetConditionWaitCleanup {
        registration: Some(registration),
        permit,
        waiter: waiter.as_ref(),
        released: false,
    };
    let mut handoff = handoff;
    if let Some(owner) = handoff.as_deref_mut() {
        if owner.suspend().is_err() {
            let _ = owner.abort();
            return Err(());
        }
    }
    let saw_notification = cleanup
        .registration
        .as_ref()
        .is_some_and(JetConditionRegistration::saw_notification);
    cleanup.release();
    let parked = if saw_notification {
        Ok(())
    } else {
        waiter.park()
    };
    let resumed = match handoff.as_deref_mut() {
        Some(owner) => owner.resume(parked.is_err()),
        None => Ok(true),
    };
    if !matches!(resumed, Ok(true)) {
        if let Some(owner) = handoff.as_deref_mut() {
            let _ = owner.abort();
        }
        cleanup.abandon();
        return Err(());
    }
    let reacquired = cleanup.finish();
    if reacquired.is_err() {
        if let Some(owner) = handoff.as_deref_mut() {
            let _ = owner.abort();
        }
        cleanup.abandon();
        return Err(());
    }
    drop(cleanup);
    parked.and(reacquired)
}

/// Park one condition-wait iteration after the caller checked its predicate.
/// Registration precedes physical handoff; notification is rechecked after
/// publication and before the local permit is released.
pub fn jet_shared_condition_wait_once(
    permit: &dyn JetSharedCanonicalPermit,
    condition: &std::sync::Arc<JetConditionProtocol>,
    waiter: std::sync::Arc<dyn JetConditionWaiter>,
) -> Result<(), ()> {
    jet_shared_condition_wait_once_with_handoff(permit, condition, waiter, None)
}

pub fn jet_shared_condition_wait_once_with_handoff(
    permit: &dyn JetSharedCanonicalPermit,
    condition: &std::sync::Arc<JetConditionProtocol>,
    waiter: std::sync::Arc<dyn JetConditionWaiter>,
    handoff: Option<&mut dyn JetSharedWaitHandoff>,
) -> Result<(), ()> {
    let registration = condition.register(waiter.clone());
    jet_shared_condition_wait_registered_with_handoff(permit, registration, waiter, handoff)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JetSharedGuardWaitError {
    Invalid,
    EditRequired,
    Cancelled,
}

impl JetSharedGuardWaitError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Invalid => JET_SHARED_GUARD_INVALID,
            Self::EditRequired => JET_SHARED_GUARD_EDIT_REQUIRED,
            Self::Cancelled => JET_SHARED_GUARD_WAIT_CANCELLED,
        }
    }

    pub fn traps(self) -> bool {
        matches!(self, Self::Invalid)
    }
}

pub fn jet_shared_guard_wait_once(
    guard: Option<&JetSharedGuardState>,
    condition: Option<&std::sync::Arc<JetConditionProtocol>>,
    waiter: std::sync::Arc<dyn JetConditionWaiter>,
) -> Result<(), JetSharedGuardWaitError> {
    jet_shared_guard_wait_once_with_handoff(guard, condition, waiter, None)
}

pub fn jet_shared_guard_wait_once_with_handoff(
    guard: Option<&JetSharedGuardState>,
    condition: Option<&std::sync::Arc<JetConditionProtocol>>,
    waiter: std::sync::Arc<dyn JetConditionWaiter>,
    handoff: Option<&mut dyn JetSharedWaitHandoff>,
) -> Result<(), JetSharedGuardWaitError> {
    let guard = guard.ok_or(JetSharedGuardWaitError::Invalid)?;
    let condition = condition.ok_or(JetSharedGuardWaitError::Invalid)?;
    jet_shared_guard_require_edit(guard).map_err(|message| {
        if message == JET_SHARED_GUARD_INVALID {
            JetSharedGuardWaitError::Invalid
        } else {
            JetSharedGuardWaitError::EditRequired
        }
    })?;
    jet_shared_condition_wait_once_with_handoff(guard.permit(), condition, waiter, handoff)
        .map_err(|_| JetSharedGuardWaitError::Cancelled)
}

pub fn jet_shared_condition_wait<E>(
    permit: &dyn JetSharedCanonicalPermit,
    condition: &std::sync::Arc<JetConditionProtocol>,
    mut ready: impl FnMut() -> Result<bool, E>,
    mut waiter: impl FnMut() -> std::sync::Arc<dyn JetConditionWaiter>,
) -> Result<(), JetConditionWaitError<E>> {
    loop {
        if ready().map_err(JetConditionWaitError::Predicate)? {
            return Ok(());
        }
        let waiter = waiter();
        let registration = condition.register(waiter.clone());
        if ready().map_err(JetConditionWaitError::Predicate)? {
            drop(registration);
            return Ok(());
        }
        let parked = jet_shared_condition_wait_registered(permit, registration, waiter);
        if parked.is_err() {
            return Err(JetConditionWaitError::Cancelled);
        }
    }
}

#[cfg(test)]
mod shared_protocol_tests {
    use super::*;

    struct CountingWaiter(std::sync::atomic::AtomicUsize);
    struct PanicWaiter;
    struct BlockingWaiter {
        notified: std::sync::atomic::AtomicBool,
        lock: std::sync::Mutex<()>,
        wake: std::sync::Condvar,
    }

    impl CountingWaiter {
        fn new() -> Self {
            Self(std::sync::atomic::AtomicUsize::new(0))
        }
    }

    impl JetConditionWaiter for CountingWaiter {
        fn park(&self) -> Result<(), ()> {
            Ok(())
        }

        fn wake(&self) {
            self.0
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    impl JetConditionWaiter for PanicWaiter {
        fn park(&self) -> Result<(), ()> {
            panic!("waiter panic")
        }

        fn wake(&self) {}
    }

    impl BlockingWaiter {
        fn new() -> Self {
            Self {
                notified: std::sync::atomic::AtomicBool::new(false),
                lock: std::sync::Mutex::new(()),
                wake: std::sync::Condvar::new(),
            }
        }
    }

    impl JetConditionWaiter for BlockingWaiter {
        fn park(&self) -> Result<(), ()> {
            if self
                .notified
                .swap(false, std::sync::atomic::Ordering::Acquire)
            {
                return Ok(());
            }
            let mut lock = self.lock.lock().unwrap();
            while !self
                .notified
                .swap(false, std::sync::atomic::Ordering::Acquire)
            {
                lock = self.wake.wait(lock).unwrap();
            }
            Ok(())
        }

        fn wake(&self) {
            let _lock = self.lock.lock().unwrap();
            self.notified
                .store(true, std::sync::atomic::Ordering::Release);
            self.wake.notify_one();
        }
    }
    #[derive(Default)]
    struct TransactionTestOwnerState {
        staged: bool,
        published: bool,
        discards: usize,
    }

    struct TransactionTestOwner {
        order: usize,
        state: std::sync::Arc<std::sync::Mutex<TransactionTestOwnerState>>,
    }

    struct TransactionTestPermit {
        state: std::sync::Arc<std::sync::Mutex<TransactionTestOwnerState>>,
        held: std::sync::atomic::AtomicBool,
    }

    impl JetSharedCanonicalOwner for TransactionTestOwner {
        fn owner_identity(&self) -> usize {
            self as *const Self as usize
        }

        fn protocol_order_key(&self) -> usize {
            self.order
        }

        fn acquire_permit(
            self: std::sync::Arc<Self>,
            _editable: bool,
        ) -> Result<std::sync::Arc<dyn JetSharedCanonicalPermit>, String> {
            Ok(std::sync::Arc::new(TransactionTestPermit {
                state: self.state.clone(),
                held: std::sync::atomic::AtomicBool::new(true),
            }))
        }
    }

    impl JetSharedCanonicalPermit for TransactionTestPermit {
        fn editable(&self) -> bool {
            true
        }

        fn held(&self) -> bool {
            self.held.load(std::sync::atomic::Ordering::Acquire)
        }

        fn release(&self) -> JetSharedPhysicalOperationOutcome<()> {
            self.held.store(false, std::sync::atomic::Ordering::Release);
            JetSharedPhysicalOperationOutcome::new(Ok(()), None)
        }

        fn release_during_drop(&self) {
            self.held.store(false, std::sync::atomic::Ordering::Release);
        }

        fn reacquire(&self, cancelled: &mut dyn FnMut() -> bool) -> bool {
            if cancelled() {
                return false;
            }
            self.held.store(true, std::sync::atomic::Ordering::Release);
            true
        }

        fn discard_staged(&self) -> Result<(), String> {
            let mut state = self.state.lock().unwrap();
            state.staged = false;
            state.discards += 1;
            Ok(())
        }

        fn into_any(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn std::any::Any> {
            self
        }
    }

    #[test]
    fn transaction_stage_failure_discards_all_participants_before_rollback() {
        let first = std::sync::Arc::new(TransactionTestOwner {
            order: 1,
            state: std::sync::Arc::new(std::sync::Mutex::new(
                TransactionTestOwnerState::default(),
            )),
        });
        let second = std::sync::Arc::new(TransactionTestOwner {
            order: 2,
            state: std::sync::Arc::new(std::sync::Mutex::new(
                TransactionTestOwnerState::default(),
            )),
        });
        let first_owner: std::sync::Arc<dyn JetSharedCanonicalOwner> = first.clone();
        let second_owner: std::sync::Arc<dyn JetSharedCanonicalOwner> = second.clone();
        let applied = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let rolled_back = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let mut transaction = jet_shared_transaction_begin();
        for (owner, state, fail) in [
            (first_owner, first.state.clone(), false),
            (second_owner, second.state.clone(), true),
        ] {
            transaction.record_edit_with_staged_commit(
                owner,
                Box::new(|| {}),
                Box::new(move |_| {
                    state.lock().unwrap().staged = true;
                    if fail {
                        Err("participant staging rejected".to_string())
                    } else {
                        Ok(())
                    }
                }),
                Box::new(move |_| {
                    state.lock().unwrap().published = true;
                }),
            );
        }
        let rollback_flag = rolled_back.clone();
        transaction.record_rollback(Box::new(move || {
            rollback_flag.store(true, std::sync::atomic::Ordering::Release);
        }));

        let apply_flag = applied.clone();
        let result = transaction.try_commit_with(move || {
            apply_flag.store(true, std::sync::atomic::Ordering::Release);
        });

        assert_eq!(result.as_deref(), Err("participant staging rejected"));
        assert!(!applied.load(std::sync::atomic::Ordering::Acquire));
        assert!(rolled_back.load(std::sync::atomic::Ordering::Acquire));
        for owner in [first, second] {
            let state = owner.state.lock().unwrap();
            assert!(!state.staged);
            assert!(!state.published);
            assert_eq!(state.discards, 1);
        }
    }


    #[test]
    fn full_path_map_and_split_preserve_parent_prefix_and_one_permit() {
        let protocol = JetSharedProtocol::new();
        let root = jet_shared_guard_acquire(&protocol, true, || false).unwrap();
        let mapped = jet_shared_guard_map_path(&root, &[4, 7], true).unwrap();
        assert!(!root.held());
        assert_eq!(mapped.path(), &[4, 7]);

        let (first, second) =
            jet_shared_guard_split_path(&mapped, &[10, 11], &[20, 21, 22], true).unwrap();
        assert!(!mapped.held());
        assert_eq!(first.path(), &[4, 7, 10, 11]);
        assert_eq!(second.path(), &[4, 7, 20, 21, 22]);
        assert!(first.held());
        assert!(second.held());
        assert!(std::sync::Arc::ptr_eq(&first.permit, &second.permit));
    }

    #[test]
    fn failed_full_path_projection_keeps_parent_live() {
        let protocol = JetSharedProtocol::new();
        let root = jet_shared_guard_acquire(&protocol, false, || false).unwrap();

        assert!(matches!(
            jet_shared_guard_map_path(&root, &[3, 5], true),
            Err(message) if message == JET_SHARED_GUARD_EDIT_REQUIRED,
        ));
        assert!(root.held());
        assert!(root.path().is_empty());
    }
    #[test]
    fn ordered_acquisition_deduplicates_one_protocol() {
        let protocol = JetSharedProtocol::new();
        let permits =
            jet_shared_acquire_ordered(vec![protocol.clone(), protocol.clone()]);
        assert_eq!(permits.len(), 1);
        assert!(permits[0].editable());
    }

    #[test]
    fn ordered_acquisition_uses_protocol_address_order() {
        let first = JetSharedProtocol::new();
        let second = JetSharedProtocol::new();
        let expected = std::cmp::min(
            std::sync::Arc::as_ptr(&first) as usize,
            std::sync::Arc::as_ptr(&second) as usize,
        );
        let permits = jet_shared_acquire_ordered(vec![second, first]);
        assert_eq!(
            std::sync::Arc::as_ptr(&permits[0].protocol) as usize,
            expected,
            "multi-cell commit locks must use stable address order"
        );
    }

    #[test]
    fn contended_transaction_waits_and_applies_its_body_once() {
        let protocol = JetSharedProtocol::new();
        let held = protocol.acquire(true, || false).unwrap();
        let log_lines = std::sync::Arc::new(std::sync::Mutex::new(Vec::<&'static str>::new()));
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let worker_protocol = protocol.clone();
        let worker_log_lines = log_lines.clone();
        let worker = std::thread::spawn(move || {
            let mut transaction = jet_shared_transaction_begin();
            transaction.record_edit(
                worker_protocol,
                Box::new(move || {
                    worker_log_lines.lock().unwrap().push("transaction-body");
                }),
            );
            ready_tx.send(()).unwrap();
            transaction.commit();
        });

        ready_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("transaction worker reached the lock wait");
        assert!(
            log_lines.lock().unwrap().is_empty(),
            "a blocked commit must not log"
        );
        drop(held);
        worker.join().unwrap();
        assert_eq!(
            *log_lines.lock().unwrap(),
            vec!["transaction-body"],
            "the committed transaction emits its body log exactly once"
        );
    }

    #[test]
    fn notify_one_claims_each_waiter_once() {
        let condition = JetConditionProtocol::new();
        let first = std::sync::Arc::new(CountingWaiter::new());
        let second = std::sync::Arc::new(CountingWaiter::new());
        let _first_registration = condition.register(first.clone());
        let _second_registration = condition.register(second.clone());

        condition.notify_one();
        condition.notify_one();

        assert_eq!(
            first.0.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
        assert_eq!(
            second.0.load(std::sync::atomic::Ordering::SeqCst),
            1
        );
    }

    #[test]
    fn panic_during_park_unregisters_and_reacquires() {
        let protocol = JetSharedProtocol::new();
        let condition = JetConditionProtocol::new();
        let permit = protocol.acquire(true, || false).unwrap();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = jet_shared_condition_wait(
                &permit,
                &condition,
                || Ok::<bool, ()>(false),
                || std::sync::Arc::new(PanicWaiter),
            );
        }));

        assert!(result.is_err());
        assert!(permit.held());
        drop(permit);
        assert!(protocol.acquire(true, || false).is_some());
    }

    #[test]
    fn notify_before_registration_becomes_a_spurious_wake() {
        let protocol = JetSharedProtocol::new();
        let condition = JetConditionProtocol::new();
        let permit = protocol.acquire(true, || false).unwrap();
        condition.notify_one();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            jet_shared_condition_wait_once(
                &permit,
                &condition,
                std::sync::Arc::new(PanicWaiter),
            )
        }));

        assert!(result.is_ok());
        assert!(permit.held());
    }

    #[test]
    fn notify_between_registration_and_park_is_not_lost() {
        for _ in 0..64 {
            let protocol = JetSharedProtocol::new();
            let condition = JetConditionProtocol::new();
            let waiter = std::sync::Arc::new(BlockingWaiter::new());
            let rescue = waiter.clone();
            let ready = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (registered_tx, registered_rx) = std::sync::mpsc::channel();
            let (park_tx, park_rx) = std::sync::mpsc::channel();
            let (done_tx, done_rx) = std::sync::mpsc::channel();
            let worker_condition = condition.clone();
            let worker_ready = ready.clone();
            let worker = std::thread::spawn(move || {
                let permit = protocol.acquire(true, || false).unwrap();
                let mut checks = 0usize;
                let result = jet_shared_condition_wait(
                    &permit,
                    &worker_condition,
                    || {
                        checks += 1;
                        if checks == 2 {
                            registered_tx.send(()).unwrap();
                            park_rx.recv().unwrap();
                            return Ok::<bool, ()>(false);
                        }
                        Ok(worker_ready.load(std::sync::atomic::Ordering::Acquire))
                    },
                    || waiter.clone(),
                );
                done_tx.send(result.is_ok()).unwrap();
            });

            registered_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .expect("waiter registers before the second predicate check");
            ready.store(true, std::sync::atomic::Ordering::Release);
            condition.notify_one();
            park_tx.send(()).unwrap();
            let done = done_rx.recv_timeout(std::time::Duration::from_secs(1));
            if done.is_err() {
                rescue.wake();
            }
            assert_eq!(done.ok(), Some(true));
            worker.join().unwrap();
        }
    }
    struct OrderedHandoffWaiter(std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>);

    impl JetConditionWaiter for OrderedHandoffWaiter {
        fn park(&self) -> Result<(), ()> {
            self.0.lock().unwrap().push("park");
            Ok(())
        }

        fn wake(&self) {
            self.0.lock().unwrap().push("wake");
        }
    }

    struct NotifyDuringHandoff {
        condition: std::sync::Arc<JetConditionProtocol>,
        events: std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>,
    }

    impl JetSharedWaitHandoff for NotifyDuringHandoff {
        fn suspend(&mut self) -> Result<(), ()> {
            self.events.lock().unwrap().push("suspend");
            self.condition.notify_one();
            Ok(())
        }

        fn resume(&mut self, cancelled: bool) -> Result<bool, ()> {
            self.events.lock().unwrap().push("resume");
            assert!(!cancelled);
            Ok(true)
        }

        fn abort(&mut self) -> Result<(), ()> {
            self.events.lock().unwrap().push("abort");
            Ok(())
        }
    }

    #[test]
    fn condition_notification_during_physical_handoff_is_not_lost() {
        let protocol = JetSharedProtocol::new();
        let permit = protocol.acquire(true, || false).unwrap();
        let condition = JetConditionProtocol::new();
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let waiter: std::sync::Arc<dyn JetConditionWaiter> =
            std::sync::Arc::new(OrderedHandoffWaiter(events.clone()));
        let mut handoff = NotifyDuringHandoff {
            condition: condition.clone(),
            events: events.clone(),
        };

        assert_eq!(
            jet_shared_condition_wait_once_with_handoff(
                &permit,
                &condition,
                waiter,
                Some(&mut handoff),
            ),
            Ok(()),
        );
        assert_eq!(*events.lock().unwrap(), vec!["suspend", "wake", "resume"]);
        assert!(permit.held());
    }

    #[test]
    fn ordered_guard_acquisition_preserves_indices_and_shares_duplicate_permits() {
        let first = std::sync::Arc::new(JetSharedProtocol::new());
        let second = std::sync::Arc::new(JetSharedProtocol::new());
        let guards = jet_shared_guard_acquire_ordered(vec![
            (11, second.clone(), false),
            (7, first.clone(), true),
            (12, first.clone(), false),
        ]);

        let mut indices = guards.iter().map(|(index, _)| *index).collect::<Vec<_>>();
        indices.sort_unstable();
        assert_eq!(indices, vec![7, 11, 12]);
        assert!(guards.iter().all(|(_, guard)| guard.held()));
        let mut editability = guards
            .iter()
            .map(|(index, guard)| (*index, guard.editable()))
            .collect::<Vec<_>>();
        editability.sort_unstable_by_key(|(index, _)| *index);
        assert_eq!(editability, vec![(7, true), (11, false), (12, false)]);
        assert!(guards.iter().all(|(_, guard)| guard.permit().editable()));
        let first_guard = guards.iter().find(|(index, _)| *index == 7).unwrap();
        let duplicate_guard = guards.iter().find(|(index, _)| *index == 12).unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &first_guard.1.permit_arc(),
            &duplicate_guard.1.permit_arc(),
        ));
        drop(guards);
        assert!(jet_shared_acquire(&first, true, || false).is_some());
        assert!(jet_shared_acquire(&second, true, || false).is_some());
    }
}
