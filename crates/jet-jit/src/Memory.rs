//! Native memory carriers for the resident Cranelift runtime.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.
#![allow(dead_code)]

use super::Concurrency;
use std::cell::{Cell, RefCell};

struct JitSentryFunctionMark {
    guard_depth: usize,
    frame_depth: usize,
}

thread_local! {
    static JIT_SENTRY_GUARDS:
        RefCell<Vec<jet_foundation::MemSentry::JetSentryGuard>> =
        const { RefCell::new(Vec::new()) };
    static JIT_SENTRY_FRAMES:
        RefCell<Vec<jet_foundation::MemSentry::JetSentryFrame>> =
        const { RefCell::new(Vec::new()) };
    static JIT_SENTRY_FUNCTIONS: RefCell<Vec<JitSentryFunctionMark>> =
        const { RefCell::new(Vec::new()) };
    static JIT_SENTRY_RUN: Cell<(usize, u64)> = const { Cell::new((0, 0)) };
}

static ACTIVE_JIT_SENTRY_RUN: LazyLock<Mutex<Option<(usize, u64)>>> =
    LazyLock::new(|| Mutex::new(None));

fn clear_jit_sentry_local() {
    JIT_SENTRY_GUARDS.with(|guards| guards.borrow_mut().clear());
    JIT_SENTRY_FRAMES.with(|frames| frames.borrow_mut().clear());
    JIT_SENTRY_FUNCTIONS.with(|functions| functions.borrow_mut().clear());
}

pub(crate) fn reset_jit_sentry_state() {
    clear_jit_sentry_local();
    jet_foundation::MemSentry::jet_sentry_reset();
    JIT_SENTRY_RUN.with(|run| run.set((0, 0)));
    *ACTIVE_JIT_SENTRY_RUN
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
}

fn prepare_jit_sentry_run(rt: &crate::JitRuntime) {
    let identity = (rt as *const crate::JitRuntime as usize, rt.invocations);
    let local_changed = JIT_SENTRY_RUN.with(|run| run.get() != identity);
    if !local_changed {
        return;
    }
    clear_jit_sentry_local();
    let should_reset_foundation = {
        let mut active = ACTIVE_JIT_SENTRY_RUN
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *active == Some(identity) {
            false
        } else {
            *active = Some(identity);
            true
        }
    };
    if should_reset_foundation {
        jet_foundation::MemSentry::jet_sentry_reset();
    }
    JIT_SENTRY_RUN.with(|run| run.set(identity));
}

fn sentry_address(address: i64) -> usize {
    usize::try_from(address).unwrap_or(0)
}

fn sentry_bytes(bytes: i64) -> usize {
    usize::try_from(bytes).unwrap_or(0).max(1)
}

fn sentry_scope_text(rt: &crate::JitRuntime, handle: i64) -> String {
    rt.heap.clone_string(handle).unwrap_or_default()
}

fn jet_jit_sentry_scope_enter(
    kind: i64,
    enabled: i64,
    file: i64,
    line: i64,
    reason: i64,
) {
    Concurrency::with_runtime_mut(|rt| {
        prepare_jit_sentry_run(rt);
        let file = sentry_scope_text(rt, file);
        let reason = sentry_scope_text(rt, reason);
        let guard = match kind {
            2 => jet_foundation::MemSentry::jet_sentry_policy_scope(enabled != 0),
            3 => jet_foundation::MemSentry::jet_sentry_fenced_scope(
                enabled != 0,
                &file,
                line.max(0) as u32,
                &reason,
            ),
            _ => jet_foundation::MemSentry::jet_sentry_scope(
                enabled != 0,
                &file,
                line.max(0) as u32,
                &reason,
            ),
        };
        JIT_SENTRY_GUARDS.with(|guards| guards.borrow_mut().push(guard));
    });
}

fn jet_jit_sentry_scope_exit() {
    JIT_SENTRY_GUARDS.with(|guards| {
        let _ = guards.borrow_mut().pop();
    });
}

fn jet_jit_sentry_frame_enter() {
    Concurrency::with_runtime_mut(|rt| {
        prepare_jit_sentry_run(rt);
        let frame = jet_foundation::MemSentry::jet_sentry_frame();
        JIT_SENTRY_FRAMES.with(|frames| frames.borrow_mut().push(frame));
    });
}

fn jet_jit_sentry_frame_exit() {
    JIT_SENTRY_FRAMES.with(|frames| {
        let _ = frames.borrow_mut().pop();
    });
}

fn jet_jit_sentry_function_mark() {
    JIT_SENTRY_FUNCTIONS.with(|functions| {
        let guard_depth = JIT_SENTRY_GUARDS.with(|guards| guards.borrow().len());
        let frame_depth = JIT_SENTRY_FRAMES.with(|frames| frames.borrow().len());
        functions
            .borrow_mut()
            .push(JitSentryFunctionMark { guard_depth, frame_depth });
    });
}

fn jet_jit_sentry_function_exit() {
    let Some(mark) = JIT_SENTRY_FUNCTIONS.with(|functions| functions.borrow_mut().pop()) else {
        return;
    };
    JIT_SENTRY_FRAMES.with(|frames| frames.borrow_mut().truncate(mark.frame_depth));
    JIT_SENTRY_GUARDS.with(|guards| guards.borrow_mut().truncate(mark.guard_depth));
}

fn jet_jit_sentry_register_stack(address: i64, bytes: i64) {
    Concurrency::with_runtime_mut(|rt| {
        prepare_jit_sentry_run(rt);
        jet_foundation::MemSentry::jet_sentry_register_stack_allocation(
            sentry_address(address),
            sentry_bytes(bytes),
        );
    });
}

fn jet_jit_sentry_check(
    address: i64,
    bytes: i64,
    alignment: i64,
    operation: i64,
    obligation: i64,
) {
    Concurrency::with_runtime_mut(|rt| {
        prepare_jit_sentry_run(rt);
        let operation = sentry_scope_text(rt, operation);
        let obligation = sentry_scope_text(rt, obligation);
        let fault = jet_foundation::MemSentry::jet_sentry_check(
            sentry_address(address),
            sentry_bytes(bytes),
            sentry_bytes(alignment),
            &operation,
            &obligation,
        );
        if let Some(fault) = fault {
            crate::runtime_host::set_sentry_fault(rt, fault);
        }
    });
}

fn jet_jit_sentry_check_fixed(
    rt: &mut crate::JitRuntime,
    address: i64,
    operation: &str,
) -> bool {
    prepare_jit_sentry_run(rt);
    let fault = jet_foundation::MemSentry::jet_sentry_check(
        sentry_address(address),
        std::mem::size_of::<i64>(),
        std::mem::align_of::<i64>(),
        operation,
        "valid_ptr",
    );
    if let Some(fault) = fault {
        crate::runtime_host::set_sentry_fault(rt, fault);
        false
    } else {
        true
    }
}
use std::sync::atomic::{compiler_fence, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

pub(crate) mod shared_protocol {
    include!("../../jet-codegen/src/Prelude/SharedProtocol.rs");
}

thread_local! {
    static SHARED_TRANSACTIONS: std::cell::RefCell<Vec<SharedTransaction>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static SHARED_ACTIVE_PERMITS:
        std::cell::RefCell<Vec<(i64, Arc<shared_protocol::JetSharedPermit>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

struct SharedTransaction {
    transaction: shared_protocol::JetSharedTransaction,
}

struct SharedSnapshot {
    owner: Arc<SharedState>,
    revision: u64,
    value: i64,
    valid: Arc<std::sync::atomic::AtomicBool>,
    consumed: std::sync::atomic::AtomicBool,
}

static SHARED_SNAPSHOTS: LazyLock<Mutex<Vec<Arc<SharedSnapshot>>>> =
    LazyLock::new(|| Mutex::new(Vec::new()));

type SharedTransactionCallback = unsafe extern "C" fn(i64, i64) -> i64;

mod jet_uninit_semantics {
    include!("../../jet-codegen/src/Prelude/Uninit.rs");
}

mod jet_fixed_kernel {
    include!("../../jet-codegen/src/Prelude/Core/FixedAllocator.rs");
}

// The resident host has no generated Observe module; allocator accounting is
// still owned by the canonical storage runtime, while these hooks preserve the
// shared Prelude's observation seam without introducing a second policy.
fn jet_observe_arena_open() {}
fn jet_observe_arena_alloc(_bytes: usize) {}
fn jet_observe_arena_retain(_bytes: usize) {}
fn jet_observe_arena_release(_bytes: usize) {}
fn jet_observe_arena_reset(_allocations: usize, _bytes: usize) {}
fn jet_observe_arena_close() {}

fn jet_fault_should_fail_allocation() -> bool {
    crate::fault_injection::jet_fault_should_fail_allocation()
}

fn jet_sentry_runtime_stop(
    code: &'static str,
    file: &str,
    line: u32,
    _gate: &str,
    _operation: &str,
    _obligation: &str,
    _obligation_status: &str,
    _foreign_component: Option<&str>,
    _foreign_fenced: Option<bool>,
    detail: &str,
) -> ! {
    crate::runtime_host::runtime_stop_unwind_at(code, file, line, detail)
}

mod canonical_mem {
    mod jet_sentry {
        pub use jet_foundation::MemSentry::{
            jet_memory_ledger_record, jet_sentry_check, jet_sentry_check_foreign,
            jet_sentry_check_foreign_strict, jet_sentry_check_foreign_with_component,
            jet_sentry_current_frame, jet_sentry_fenced_scope, jet_sentry_frame,
            jet_sentry_policy_scope, jet_sentry_quarantine, jet_sentry_quarantine_owner,
            jet_sentry_register_allocation, jet_sentry_register_owned_allocation,
            jet_sentry_register_stack_allocation, jet_sentry_reset, jet_sentry_scope,
            jet_sentry_set_hardened, JetSentryFault, JetSentryFrame, JetSentryGuard,
            MemoryLedgerWitness,
        };
    }

    include!("../../jet-codegen/src/Prelude/Mem.rs");
}

pub use jet_foundation::Outcome::{jet_alloc_error, jet_try_alloc_value, AllocError};

enum CanonicalAllocator {
    Arena(canonical_mem::JetArena),
    Bump(canonical_mem::JetBump),
    Pool(canonical_mem::JetPool),
    Fixed(canonical_mem::JetFixed),
}

impl CanonicalAllocator {
    fn named(allocator: &'static str) -> (Self, Option<Vec<u8>>) {
        match allocator {
            "Bump" => (Self::Bump(canonical_mem::JetBump::new()), None),
            "Pool" => (Self::Pool(canonical_mem::JetPool::new()), None),
            "Fixed" => Self::with_capacity("Fixed", 1),
            _ => (Self::Arena(canonical_mem::JetArena::new()), None),
        }
    }

    fn with_capacity(
        allocator: &'static str,
        capacity: usize,
    ) -> (Self, Option<Vec<u8>>) {
        let capacity = capacity.max(1);
        match allocator {
            "Bump" => (
                Self::Bump(canonical_mem::JetBump::with_capacity(capacity)),
                None,
            ),
            "Pool" => (
                Self::Pool(canonical_mem::JetPool::with_slots(capacity)),
                None,
            ),
            "Fixed" => {
                let mut backing = vec![0_u8; capacity];
                let runtime = canonical_mem::JetFixed::over(backing.as_mut_slice());
                (Self::Fixed(runtime), Some(backing))
            }
            _ => (
                Self::Arena(canonical_mem::JetArena::with_capacity(capacity)),
                None,
            ),
        }
    }

    fn try_alloc(&self, value: i64) -> Result<*mut i64, ()> {
        let result = match self {
            Self::Arena(allocator) => allocator.try_alloc(value),
            Self::Bump(allocator) => allocator.try_alloc(value),
            Self::Pool(allocator) => allocator.try_alloc(value),
            Self::Fixed(allocator) => allocator.try_alloc(value),
        };
        result
            .map(|slot| slot as *mut i64)
            .map_err(|_| ())
    }

    fn reset(&mut self) {
        match self {
            Self::Arena(allocator) => allocator.reset(),
            Self::Bump(allocator) => allocator.reset(),
            Self::Pool(allocator) => allocator.reset(),
            Self::Fixed(allocator) => allocator.reset(),
        }
    }
}

pub(crate) struct AllocatorState {
    generation: u32,
    allocator: &'static str,
    runtime: CanonicalAllocator,
    // Declared after `runtime` so Fixed drops its values before this backing.
    fixed_backing: Option<Vec<u8>>,
    slots: Vec<AllocatorSlot>,
    closed: bool,
}

#[derive(Clone, Copy)]
struct AllocatorSlot {
    generation: u32,
    ptr: *mut i64,
}

#[derive(Clone, Copy)]
pub(crate) struct AllocatorView {
    pub(crate) allocator: i64,
    slot: i64,
}

impl AllocatorState {
    fn from_parts(
        allocator: &'static str,
        runtime: CanonicalAllocator,
        fixed_backing: Option<Vec<u8>>,
    ) -> Self {
        Self {
            generation: 0,
            allocator,
            runtime,
            fixed_backing,
            slots: Vec::new(),
            closed: false,
        }
    }

    fn named(allocator: &'static str) -> Self {
        let (runtime, fixed_backing) = CanonicalAllocator::named(allocator);
        Self::from_parts(allocator, runtime, fixed_backing)
    }

    fn with_capacity(allocator: &'static str, capacity: usize) -> Self {
        let (runtime, fixed_backing) = CanonicalAllocator::with_capacity(allocator, capacity);
        Self::from_parts(allocator, runtime, fixed_backing)
    }
}

impl Default for AllocatorState {
    fn default() -> Self {
        Self::named("Arena")
    }
}

type CanonicalPool = jet_codegen::Comptime::PoolRuntime::jet_std::JetPool<i64>;
type CanonicalPoolId = jet_codegen::Comptime::PoolRuntime::jet_std::JetId<i64>;

pub(crate) struct PoolState {
    pool: CanonicalPool,
}

impl Default for PoolState {
    fn default() -> Self {
        Self {
            pool: CanonicalPool::new(),
        }
    }
}

pub(crate) struct SharedState {
    pub(crate) protocol: Arc<shared_protocol::JetSharedProtocol>,
    value: AtomicI64,
    revision: AtomicU64,
}

pub(crate) struct ConditionState {
    pub(crate) protocol: Arc<shared_protocol::JetConditionProtocol>,
}

impl ConditionState {
    fn new() -> Self {
        Self {
            protocol: shared_protocol::JetConditionProtocol::new(),
        }
    }

    fn notify_one(&self) {
        shared_protocol::jet_shared_condition_notify_one(&self.protocol);
    }

    fn notify_all(&self) {
        shared_protocol::jet_shared_condition_notify_all(&self.protocol);
    }
}

struct JitConditionWaiter {
    slot: Arc<jet_codegen::scheduler::ParkSlot>,
}

impl JitConditionWaiter {
    fn new() -> Self {
        Self {
            slot: jet_codegen::scheduler::ParkSlot::new(),
        }
    }
}

impl shared_protocol::JetConditionWaiter for JitConditionWaiter {
    fn park(&self) -> Result<(), ()> {
        jet_codegen::scheduler::jet_scheduler_yield("Shared condition", &self.slot, None);
        Ok(())
    }

    fn wake(&self) {
        jet_codegen::scheduler::jet_scheduler_wake(&self.slot);
    }

    fn interrupted(&self) -> bool {
        jet_codegen::scheduler::jet_scheduler_wait_point_interrupted()
    }
}

pub(crate) struct ExpiringState {
    value: i64,
    expires_at: i64,
    clock: i64,
    secret: Option<SecretState>,
}

pub(crate) struct SecretState {
    handle: i64,
    bytes: Vec<u8>,
}

impl SecretState {
    pub(crate) fn from_material(handle: i64, bytes: Vec<u8>) -> Self {
        Self { handle, bytes }
    }

    fn zeroize(&mut self) {
        for byte in self.bytes.iter_mut() {
            // SAFETY: the pointer refers to this live, uniquely borrowed byte.
            unsafe { std::ptr::write_volatile(byte, 0) };
        }
        compiler_fence(Ordering::SeqCst);
    }
}

impl Drop for SecretState {
    fn drop(&mut self) {
        self.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::SecretState;

    #[test]
    fn secret_storage_zeroizes_the_owned_bytes() {
        let mut secret = SecretState::from_material(1, vec![0x5a; 32]);
        secret.zeroize();
        assert_eq!(secret.bytes.as_slice(), &[0; 32]);
    }
}

impl SharedState {
    fn new(value: i64) -> Self {
        Self {
            protocol: shared_protocol::JetSharedProtocol::new(),
            value: AtomicI64::new(value),
            revision: AtomicU64::new(0),
        }
    }
}
fn shared_next_revision(shared: &SharedState) -> Result<u64, ()> {
    shared
        .revision
        .load(Ordering::Acquire)
        .checked_add(1)
        .ok_or(())
}

fn shared_capture_parts(shared: &Arc<SharedState>) -> Option<(u64, i64)> {
    let permit = shared_protocol::jet_shared_acquire(&shared.protocol, false, || false)?;
    let revision = shared.revision.load(Ordering::Acquire);
    let value = shared.value.load(Ordering::Acquire);
    drop(permit);
    Some((revision, value))
}

fn shared_snapshot_store(owner: Arc<SharedState>, revision: u64, value: i64) -> i64 {
    let snapshot = Arc::new(SharedSnapshot {
        owner,
        revision,
        value,
        valid: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        consumed: std::sync::atomic::AtomicBool::new(false),
    });
    let mut snapshots = SHARED_SNAPSHOTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(index) = snapshots.len().checked_add(1) else {
        return 0;
    };
    let Ok(index) = i64::try_from(index) else {
        return 0;
    };
    snapshots.push(snapshot);
    -index
}

fn shared_snapshot_load(handle: i64) -> Option<Arc<SharedSnapshot>> {
    let index = handle.checked_neg()?.checked_sub(1)?;
    let index = usize::try_from(index).ok()?;
    SHARED_SNAPSHOTS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(index)
        .cloned()
}

fn shared_revision_error_result(rt: &mut crate::JitRuntime, discriminant: i64) -> i64 {
    let error = rt.heap.alloc_record(1);
    let _ = rt.heap.record_set_int(error, 0, discriminant);
    crate::runtime_host::alloc_jit_result(rt, false, error as u64)
}

fn shared_transaction_touch(shared: &Arc<SharedState>) -> bool {
    SHARED_TRANSACTIONS.with(|transactions| {
        let mut transactions = transactions.borrow_mut();
        let Some(transaction) = transactions.last_mut() else {
            return false;
        };
        transaction
            .transaction
            .touch(Arc::clone(&shared.protocol));
        true
    })
}
fn shared_transaction_staged(shared: &Arc<SharedState>) -> Option<std::rc::Rc<std::cell::RefCell<i64>>> {
    SHARED_TRANSACTIONS.with(|transactions| {
        transactions
            .borrow()
            .last()
            .and_then(|transaction| {
                transaction
                    .transaction
                    .staged_value::<i64>(&shared.protocol)
            })
    })
}

fn shared_transaction_snapshot_revision(shared: &Arc<SharedState>) -> Option<u64> {
    SHARED_TRANSACTIONS.with(|transactions| {
        transactions
            .borrow()
            .last()
            .and_then(|transaction| {
                transaction
                    .transaction
                    .snapshot_revision(
                        &shared.protocol,
                        shared.revision.load(Ordering::Acquire),
                    )
            })
    })
}

fn shared_transaction_stage_for_write(
    shared: &Arc<SharedState>,
) -> Option<std::rc::Rc<std::cell::RefCell<i64>>> {
    let protocol = Arc::clone(&shared.protocol);
    SHARED_TRANSACTIONS.with(|transactions| {
        let mut transactions = transactions.borrow_mut();
        let transaction = transactions.last_mut()?;
        let staged = transaction
            .transaction
            .stage_value(protocol.clone(), || shared.value.load(Ordering::Acquire));
        transaction.transaction.mark_write(protocol.clone());
        let commit_shared = Arc::clone(shared);
        let commit_staged = staged.clone();
        transaction.transaction.record_edit_with_commit(
            protocol,
            Box::new(|| {}),
            Box::new(move || {
                let value = *commit_staged.borrow();
                let Some(next) = shared_next_revision(&commit_shared).ok() else {
                    Concurrency::with_runtime_mut(|rt| {
                        rt.set_trap("SharedRevisionError.GenerationExhausted");
                    });
                    return;
                };
                commit_shared.value.store(value, Ordering::Release);
                commit_shared.revision.store(next, Ordering::Release);
            }),
        );
        Some(staged)
    })
}

fn pool(rt: &crate::JitRuntime, handle: i64) -> Option<Arc<Mutex<PoolState>>> {
    rt.pools.get((handle as usize).wrapping_sub(1)).cloned()
}

fn shared(rt: &crate::JitRuntime, handle: i64) -> Option<Arc<SharedState>> {
    rt.shareds.get((handle as usize).wrapping_sub(1)).cloned()
}

/// Read the current value behind a checked `Shared<T>` handle for a host
/// marshaller. The protocol permit is intentionally not acquired here:
/// receipt attachment snapshots the ambient value and does not expose a
/// mutable borrow to user code.
pub(crate) fn shared_value(rt: &crate::JitRuntime, handle: i64) -> Option<i64> {
    shared(rt, handle).map(|state| state.value.load(Ordering::Acquire))
}

fn condition(rt: &crate::JitRuntime, handle: i64) -> Option<Arc<ConditionState>> {
    rt.conditions
        .get((handle as usize).wrapping_sub(1))
        .cloned()
}

const GUARD_SHARED: i64 = 0;
const GUARD_VALUE: i64 = 1;

fn guard_shared_handle(rt: &crate::JitRuntime, guard: i64) -> Option<i64> {
    let handle = rt.heap.record_get_int(guard, GUARD_SHARED)?;
    (handle != 0).then_some(handle)
}

fn guard_state(
    rt: &crate::JitRuntime,
    guard: i64,
) -> Option<Arc<shared_protocol::JetSharedGuardState>> {
    rt.shared_guard_states.get(&guard).cloned()
}

fn guard_projection_slot(rt: &crate::JitRuntime, guard: i64, path: &[i64]) -> Option<(i64, i64)> {
    if path.is_empty() {
        return Some((guard, GUARD_VALUE));
    }
    let mut record = rt.heap.record_get_int(guard, GUARD_VALUE)?;
    for field in path.iter().take(path.len().saturating_sub(1)) {
        record = rt.heap.record_get_int(record, *field)?;
    }
    Some((record, *path.last()?))
}

fn pack_shared_guard(
    rt: &mut crate::JitRuntime,
    shared_handle: i64,
    value: i64,
    state: Arc<shared_protocol::JetSharedGuardState>,
) -> i64 {
    let guard = rt.heap.alloc_record(2);
    let _ = rt.heap.record_set_int(guard, GUARD_SHARED, shared_handle);
    let _ = rt.heap.record_set_int(guard, GUARD_VALUE, value);
    rt.shared_guard_states.insert(guard, state);
    guard
}

fn take_active_shared_permit(handle: i64) -> Option<Arc<shared_protocol::JetSharedPermit>> {
    SHARED_ACTIVE_PERMITS.with(|permits| {
        let mut permits = permits.borrow_mut();
        permits
            .iter()
            .rposition(|(active_handle, _)| *active_handle == handle)
            .map(|index| permits.swap_remove(index).1)
    })
}

fn pack_id(index: usize, generation: u32) -> i64 {
    (i64::from(generation) << 32) | (index as i64 + 1)
}

fn unpack_id(id: i64) -> Option<(usize, u32)> {
    let low = (id as u64 & 0xffff_ffff) as u32;
    low.checked_sub(1).map(|index| (index as usize, (id as u64 >> 32) as u32))
}

const ALLOCATOR_VIEW_TAG: i64 = i64::MIN;

fn pack_allocator_view(index: usize) -> Option<i64> {
    (index <= i64::MAX as usize).then_some(ALLOCATOR_VIEW_TAG | index as i64)
}

fn unpack_allocator_view(value: i64) -> Option<usize> {
    (value & ALLOCATOR_VIEW_TAG != 0).then_some((value & i64::MAX) as usize)
}

fn allocator_state_mut(
    rt: &mut crate::JitRuntime,
    handle: i64,
) -> Result<&mut AllocatorState, String> {
    let state = rt
        .allocators
        .get_mut((handle as usize).wrapping_sub(1))
        .ok_or_else(|| "allocator handle is closed or invalid".to_string())?;
    if state.closed {
        return Err("allocator handle is closed or invalid".to_string());
    }
    Ok(state)
}

/// Store one allocator value through the shared canonical allocator runtime.
///
/// The resident tier keeps only the erased pointer and generation table; the
/// Prelude allocator owns placement, capacity, destruction, and reset.
fn allocator_try_store(
    state: &mut AllocatorState,
    value: i64,
    requested: usize,
    fail: bool,
) -> Result<usize, AllocError> {
    if fail {
        return Err(jet_foundation::Outcome::jet_alloc_error(
            requested,
            state.allocator,
        ));
    }
    let ptr = state
        .runtime
        .try_alloc(value)
        .map_err(|_| jet_foundation::Outcome::jet_alloc_error(requested, state.allocator))?;
    let index = state.slots.len();
    state.slots.push(AllocatorSlot {
        generation: state.generation,
        ptr,
    });
    Ok(index)
}

fn allocator_store(
    rt: &mut crate::JitRuntime,
    handle: i64,
    value: i64,
    requested_bytes: i64,
) -> Result<i64, String> {
    let state = allocator_state_mut(rt, handle)?;
    let requested = usize::try_from(requested_bytes.max(1)).unwrap_or(usize::MAX);
    let index = allocator_try_store(state, value, requested, false).map_err(|error| {
        format!(
            "{} allocation failed for {} bytes",
            error.allocator, error.requested_bytes
        )
    })?;
    Ok(pack_id(index, state.generation))
}

fn allocator_view(rt: &crate::JitRuntime, view: i64) -> Result<AllocatorView, String> {
    let index = unpack_allocator_view(view)
        .ok_or_else(|| "allocator view is invalid or no longer live".to_string())?;
    rt.allocator_views
        .get(index)
        .copied()
        .ok_or_else(|| "allocator view is invalid or no longer live".to_string())
}

fn allocator_slot(rt: &crate::JitRuntime, view: i64) -> Result<(i64, AllocatorSlot), String> {
    let view = allocator_view(rt, view)?;
    let state = rt
        .allocators
        .get((view.allocator as usize).wrapping_sub(1))
        .ok_or_else(|| "allocator view is invalid or no longer live".to_string())?;
    let (index, generation) = unpack_id(view.slot)
        .ok_or_else(|| "allocator view is invalid or no longer live".to_string())?;
    let slot = state
        .slots
        .get(index)
        .copied()
        .filter(|slot| slot.generation == generation && generation == state.generation)
        .ok_or_else(|| "allocator view is invalid or no longer live".to_string())?;
    Ok((view.allocator, slot))
}

fn jet_jit_allocator_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.allocators.push(AllocatorState::default());
        rt.allocators.len() as i64
    })
}

fn jet_jit_allocator_new_named(kind: i64) -> i64 {
    let allocator = match kind {
        1 => "Bump",
        2 => "Pool",
        _ => "Arena",
    };
    Concurrency::with_runtime_mut(|rt| {
        rt.allocators.push(AllocatorState::named(allocator));
        rt.allocators.len() as i64
    })
}

fn jet_jit_allocator_new_capacity(capacity: i64, fixed: i64) -> i64 {
    let allocator = match fixed {
        1 => "Fixed",
        2 => "Bump",
        3 => "Pool",
        _ => "Arena",
    };
    let capacity = usize::try_from(capacity.max(1)).unwrap_or(usize::MAX);
    Concurrency::with_runtime_mut(|rt| {
        rt.allocators
            .push(AllocatorState::with_capacity(allocator, capacity));
        rt.allocators.len() as i64
    })
}

fn jet_jit_allocator_alloc(handle: i64, value: i64, requested_bytes: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let slot = match allocator_store(rt, handle, value, requested_bytes) {
            Ok(slot) => slot,
            Err(message) => {
                rt.set_trap(&message);
                return 0;
            }
        };
        let Some(view) = pack_allocator_view(rt.allocator_views.len()) else {
            rt.set_trap("allocator view table exhausted");
            return 0;
        };
        rt.allocator_views.push(AllocatorView {
            allocator: handle,
            slot,
        });
        view
    })
}

fn jet_jit_allocator_view_read(view: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match allocator_slot(rt, view) {
        Ok((_, slot)) => {
            // SAFETY: the slot is live under its allocator generation.
            unsafe { slot.ptr.read() }
        }
        Err(message) => {
            rt.set_trap(&message);
            0
        }
    })
}
fn jet_jit_view_string(view: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| match crate::runtime_host::view_string(rt, view) {
        Some(value) => rt.heap.alloc_string(value),
        None => {
            rt.set_trap("string view is invalid or not a text view");
            0
        }
    })
}

fn jet_jit_allocator_view_write(view: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| match allocator_slot(rt, view) {
        Ok((_, slot)) => {
            // SAFETY: the slot is live under its allocator generation.
            unsafe { slot.ptr.write(value) };
        }
        Err(message) => rt.set_trap(&message),
    });
}

fn jet_jit_allocator_try_alloc(handle: i64, value: i64, requested_bytes: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let requested = usize::try_from(requested_bytes.max(1)).unwrap_or(usize::MAX);
        let (allocator, generation, result) = {
            let Some(state) = rt.allocators.get_mut((handle as usize).wrapping_sub(1)) else {
                rt.set_trap("allocator handle is closed or invalid");
                return 0;
            };
            if state.closed {
                rt.set_trap("allocator handle is closed or invalid");
                return 0;
            }
            let result = allocator_try_store(
                state,
                value,
                requested,
                crate::fault_injection::jet_fault_should_fail_allocation(),
            );
            (state.allocator, state.generation, result)
        };
        match result {
            // D-ALLOCFAIL1=A: a successful try result carries the live
            // allocator view, not a copied scalar payload.
            Ok(index) => {
                let slot = pack_id(index, generation);
                let Some(view) = pack_allocator_view(rt.allocator_views.len()) else {
                    rt.set_trap("allocator view table exhausted");
                    return 0;
                };
                rt.allocator_views.push(AllocatorView {
                    allocator: handle,
                    slot,
                });
                crate::runtime_host::alloc_jit_result(rt, true, view as u64)
            }
            Err(error) => {
                let record = rt.heap.alloc_record(2);
                let allocator = rt.heap.alloc_string(error.allocator);
                let _ = rt.heap.record_set_int(record, 0, error.requested_bytes);
                let _ = rt.heap.record_set_string(record, 1, allocator);
                crate::runtime_host::alloc_jit_result(rt, false, record as u64)
            }
        }
    })
}

fn jet_jit_allocator_reset(handle: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let Some(state) = rt.allocators.get_mut((handle as usize).wrapping_sub(1)) else {
            rt.set_trap("allocator handle is closed or invalid");
            return;
        };
        if state.closed {
            rt.set_trap("allocator handle is closed or invalid");
            return;
        }
        state.runtime.reset();
        state.generation = state.generation.wrapping_add(1);
        state.slots.clear();
    });
}

fn jet_jit_allocator_close(handle: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let Some(state) = rt.allocators.get_mut((handle as usize).wrapping_sub(1)) else {
            rt.set_trap("allocator handle is closed or invalid");
            return;
        };
        if state.closed {
            rt.set_trap("allocator handle is closed or invalid");
            return;
        }
        state.runtime.reset();
        state.closed = true;
        state.slots.clear();
    });
}

const JIT_GC_SITE: jet_rt::__gc::PromotionSite = jet_rt::__gc::PromotionSite {
    source: "<jit>",
    span_start: 0,
    span_end: 0,
    scope: "#Policy(gc)",
    policy_provenance: "hosted",
    reason: "automatic promotion",
    type_name: "<jit-value>",
    bytes: 0,
};

fn jet_jit_gc_promote(value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let root = match jet_rt::__gc::AutomaticRoot::promote(value, JIT_GC_SITE) {
            Ok(root) => root,
            Err(fault) => {
                rt.set_trap(&fault.to_string());
                return 0;
            }
        };
        rt.gc_roots.push(root);
        rt.gc_edges.push(Vec::new());
        rt.gc_roots.len() as i64
    })
}

fn jet_jit_gc_read(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let result = match rt.gc_roots.get((handle as usize).wrapping_sub(1)) {
            Some(root) => root.read(|value| *value).map_err(|fault| fault.to_string()),
            None => Err("automatic GC root is invalid or closed".to_string()),
        };
        match result {
            Ok(value) => value,
            Err(message) => {
                rt.set_trap(&message);
                0
            }
        }
    })
}

fn jet_jit_gc_edit(handle: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let result = match rt.gc_roots.get((handle as usize).wrapping_sub(1)) {
            Some(root) => root
                .edit(|slot| *slot = value)
                .map(|_| ())
                .map_err(|fault| fault.to_string()),
            None => Err("automatic GC root is invalid or closed".to_string()),
        };
        if let Err(message) = result {
            rt.set_trap(&message);
        }
    });
}
fn gc_callback_value(slot: crate::runtime_host::JitCallableSlot, value: i64) -> i64 {
    unsafe {
        if slot.has_env {
            let callback: unsafe extern "C" fn(i64, i64) -> i64 =
                std::mem::transmute(slot.fn_ptr as usize);
            callback(slot.env, value)
        } else {
            let callback: unsafe extern "C" fn(i64) -> i64 =
                std::mem::transmute(slot.fn_ptr as usize);
            callback(value)
        }
    }
}

enum GcEditAction {
    Clear,
    Pop,
    RemoveIndex,
    InsertIndex,
    Prepend,
    Additive,
    Plain,
    EdgeSlot,
}

fn gc_edit(
    root_handle: i64,
    edge_list: i64,
    callback_handle: i64,
    index: i64,
    site: i64,
    action: GcEditAction,
) -> i64 {
    let snapshot = Concurrency::with_runtime_string(|rt| {
        let root_index = usize::try_from(root_handle)
            .ok()
            .and_then(|handle| handle.checked_sub(1))
            .ok_or_else(|| "automatic GC root handle is invalid".to_string())?;
        let root = rt
            .gc_roots
            .get(root_index)
            .ok_or_else(|| "automatic GC root handle is invalid".to_string())?
            .try_clone_root()
            .map_err(|fault| fault.to_string())?;
        let callback = crate::runtime_host::jit_callable_parts(rt, callback_handle)
            .ok_or_else(|| "automatic GC edit callback handle is invalid".to_string())?;
        let mut edges = Vec::new();
        if edge_list != 0 {
            let len = rt
                .heap
                .list_len(edge_list)
                .ok_or_else(|| "automatic GC edge list handle is invalid".to_string())?;
            for position in 0..len {
                let handle = rt
                    .heap
                    .list_get_int(edge_list, position)
                    .ok_or_else(|| "automatic GC edge list contains an invalid handle".to_string())?;
                let edge_index = usize::try_from(handle)
                    .ok()
                    .and_then(|value| value.checked_sub(1))
                    .ok_or_else(|| "automatic GC edge handle is invalid".to_string())?;
                let edge = rt
                    .gc_roots
                    .get(edge_index)
                    .ok_or_else(|| "automatic GC edge handle is invalid".to_string())?
                    .id();
                edges.push(edge);
            }
        }
        Ok::<_, String>((root, edges, callback))
    });
    let (root, edges, callback) = match snapshot {
        Ok(snapshot) => snapshot,
        Err(message) => {
            Concurrency::with_runtime_mut(|rt| rt.set_host_fault(&message));
            return 0;
        }
    };
    let mut edit = |slot: &mut i64| {
        let updated = gc_callback_value(callback, *slot);
        *slot = updated;
        updated
    };
    let result = match action {
        GcEditAction::Clear => root.edit_clearing_edges(&mut edit),
        GcEditAction::Pop => root.edit_edge_slot_pop("collection", &mut edit),
        GcEditAction::RemoveIndex => usize::try_from(index)
            .map_err(|_| jet_rt::__gc::Fault::UnknownObject(root.id()))
            .and_then(|index| root.edit_edge_slot_remove("collection", index, &mut edit)),
        GcEditAction::InsertIndex => usize::try_from(index)
            .map_err(|_| jet_rt::__gc::Fault::UnknownObject(root.id()))
            .and_then(|index| root.edit_edge_slot_insert("collection", index, &edges, &mut edit)),
        GcEditAction::Prepend => root.edit_edge_slot_prepend("collection", &edges, &mut edit),
        GcEditAction::Additive => root.edit_edge_slot_additive("collection", &edges, &mut edit),
        GcEditAction::Plain => root.edit(&mut edit),
        GcEditAction::EdgeSlot => {
            let slot = format!("method:{site}");
            root.edit_edge_slot(&slot, &edges, &mut edit)
        }
    };
    match result {
        Ok(value) => value,
        Err(fault) => {
            Concurrency::with_runtime_mut(|rt| rt.set_trap(&fault.to_string()));
            0
        }
    }
}

fn jet_jit_gc_edit_clear(root: i64, callback: i64) -> i64 {
    gc_edit(root, 0, callback, 0, 0, GcEditAction::Clear)
}

fn jet_jit_gc_edit_pop(root: i64, callback: i64) -> i64 {
    gc_edit(root, 0, callback, 0, 0, GcEditAction::Pop)
}

fn jet_jit_gc_edit_remove_index(root: i64, index: i64, callback: i64) -> i64 {
    gc_edit(root, 0, callback, index, 0, GcEditAction::RemoveIndex)
}

fn jet_jit_gc_edit_insert_index(root: i64, index: i64, edges: i64, callback: i64) -> i64 {
    gc_edit(root, edges, callback, index, 0, GcEditAction::InsertIndex)
}

fn jet_jit_gc_edit_prepend(root: i64, edges: i64, callback: i64) -> i64 {
    gc_edit(root, edges, callback, 0, 0, GcEditAction::Prepend)
}

fn jet_jit_gc_edit_additive(root: i64, edges: i64, callback: i64) -> i64 {
    gc_edit(root, edges, callback, 0, 0, GcEditAction::Additive)
}

fn jet_jit_gc_edit_plain(root: i64, callback: i64) -> i64 {
    gc_edit(root, 0, callback, 0, 0, GcEditAction::Plain)
}

fn jet_jit_gc_edit_edge_slot(root: i64, edges: i64, callback: i64, site: i64) -> i64 {
    gc_edit(root, edges, callback, 0, site, GcEditAction::EdgeSlot)
}
fn jet_jit_gc_clear_edges(handle: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let index = (handle as usize).wrapping_sub(1);
        let result = match (rt.gc_edges.get_mut(index), rt.gc_roots.get(index)) {
            (Some(edges), Some(root)) => {
                edges.clear();
                root.replace_edge_slot("jit", &[])
                    .map_err(|fault| fault.to_string())
            }
            _ => Err("automatic GC root is invalid or closed".to_string()),
        };
        if let Err(message) = result {
            rt.set_trap(&message);
        }
    });
}

fn jet_jit_gc_add_edge(handle: i64, child: i64, edge_slot: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let root_index = (handle as usize).wrapping_sub(1);
        let child_id = rt
            .gc_roots
            .get((child as usize).wrapping_sub(1))
            .map(|root| root.id());
        let result = match (child_id, rt.gc_edges.get_mut(root_index)) {
            (Some(child_id), Some(edges)) if edge_slot >= 0 => {
                let slot = edge_slot as usize;
                let edge_result = if slot == edges.len() {
                    edges.push(child_id);
                    Ok(())
                } else if slot < edges.len() {
                    edges[slot] = child_id;
                    Ok(())
                } else {
                    Err("automatic GC edge slots must be contiguous".to_string())
                };
                match edge_result {
                    Ok(()) => match rt.gc_roots.get(root_index) {
                        Some(root) => root
                            .replace_edge_slot("jit", edges)
                            .map_err(|fault| fault.to_string()),
                        None => Err("automatic GC root is invalid or closed".to_string()),
                    },
                    Err(message) => Err(message),
                }
            }
            (Some(_), Some(_)) => Err("automatic GC edge slot is invalid".to_string()),
            _ => Err("automatic GC root is invalid or closed".to_string()),
        };
        if let Err(message) = result {
            rt.set_trap(&message);
        }
    });
}

fn jet_jit_pool_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.pools.push(Arc::new(Mutex::new(PoolState::default())));
        rt.pools.len() as i64
    })
}

fn jet_jit_pool_add(handle: i64, value: i64) -> i64 {
    let Some(pool) = Concurrency::with_runtime_mut(|rt| pool(rt, handle)) else {
        return 0;
    };
    let mut pool = pool.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    pool.pool.add(value).to_word()
}

fn pool_value(handle: i64, id: i64) -> Option<i64> {
    let pool = Concurrency::with_runtime_mut(|rt| pool(rt, handle))?;
    let pool = pool.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let id = CanonicalPoolId::from_word(id)?;
    pool.pool.checked_get(id).copied()
}

/// `src_line` is the source line captured in the shared PoolSlot TIR node; it
/// avoids falling back to the enclosing function's prologue when this host's
/// run-level source text is unavailable.
fn jet_jit_pool_get(handle: i64, id: i64, line: i64, src_line: i64) -> i64 {
    match pool_value(handle, id) {
        Some(value) => value,
        None => {
            Concurrency::with_runtime_mut(|rt| {
                let source_line = rt.heap.clone_string(src_line).unwrap_or_default();
                rt.set_runtime_stop_with_source_line(
                    "E3001",
                    line.max(0) as u32,
                    Some(source_line.as_str()),
                    jet_foundation::Outcome::jet_pool_stale_message(),
                )
            });
            0
        }
    }
}
/// Checked MIR Pool-index reads carry file/function handles for diagnostics;
/// this adapter keeps the existing source-line-aware JIT kernel and its
/// six-word ABI.
fn jet_jit_pool_get_checked(
    handle: i64,
    id: i64,
    _file: i64,
    line: i64,
    _function: i64,
    source_line: i64,
) -> i64 {
    jet_jit_pool_get(handle, id, line, source_line)
}


fn jet_jit_pool_set(handle: i64, id: i64, value: i64, line: i64, src_line: i64) {
    let updated = Concurrency::with_runtime_mut(|rt| {
        let Some(pool) = pool(rt, handle) else {
            return false;
        };
        let mut pool = pool.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(id) = CanonicalPoolId::from_word(id) else {
            return false;
        };
        let Some(slot) = pool.pool.checked_get_mut(id) else {
            return false;
        };
        *slot = value;
        true
    });
    if !updated {
        Concurrency::with_runtime_mut(|rt| {
            let source_line = rt.heap.clone_string(src_line).unwrap_or_default();
            rt.set_runtime_stop_with_source_line(
                "E3001",
                line.max(0) as u32,
                Some(source_line.as_str()),
                jet_foundation::Outcome::jet_pool_stale_message(),
            )
        });
    }
}

/// Exact checked MIR pool-index setter route.  The route carries the source
/// file and function as diagnostic metadata; the pool kernel consumes the
/// source line and source text just like its getter counterpart.
fn jet_jit_index_pool_set(
    handle: i64,
    id: i64,
    value: i64,
    _file: i64,
    line: i64,
    _function: i64,
    source_line: i64,
) {
    jet_jit_pool_set(handle, id, value, line, source_line);
}

/// `JetPool::remove` uses the packed optional ABI: zero is absent and a
/// present payload is offset by one so that zero remains unambiguous.
fn jet_jit_pool_absent() -> i64 {
    0
}

fn jet_jit_pool_remove(handle: i64, id: i64) -> i64 {
    let Some(pool) = Concurrency::with_runtime_mut(|rt| pool(rt, handle)) else {
        return jet_jit_pool_absent();
    };
    let mut pool = pool.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(id) = CanonicalPoolId::from_word(id) else {
        return jet_jit_pool_absent();
    };
    match pool.pool.remove(id) {
        Ok(value) => value.wrapping_add(1),
        Err(_) => jet_jit_pool_absent(),
    }
}

fn jet_jit_pool_ids(handle: i64) -> i64 {
    let Some(pool) = Concurrency::with_runtime_mut(|rt| pool(rt, handle)) else {
        return 0;
    };
    let ids = {
        let pool = pool.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        pool.pool
            .ids()
            .into_iter()
            .map(|id| id.to_word())
            .collect::<Vec<_>>()
    };
    Concurrency::with_runtime_mut(|rt| rt.heap.alloc_int_list(ids))
}

fn jet_jit_shared_new(value: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.shareds.push(Arc::new(SharedState::new(value)));
        rt.shareds.len() as i64
    })
}

/// Allocate a fresh Shared carrier for descriptor-guided persistent restore.
/// The caller has already converted the element into a new run-heap word.
pub(crate) fn shared_alloc_for_persist(rt: &mut crate::JitRuntime, value: i64) -> i64 {
    rt.shareds.push(Arc::new(SharedState::new(value)));
    rt.shareds.len() as i64
}

/// Scalar Shared compatibility operations stay on the canonical atomic rail.
/// They must not enter the blocking guard protocol: callback-safe scalar reads
/// and writes are direct Acquire/Release operations, while guard/aggregate
/// paths below retain the protocol permits.
fn jet_jit_shared_get(handle: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return 0;
    };
    shared_transaction_staged(&shared)
        .map_or_else(|| shared.value.load(Ordering::Acquire), |staged| *staged.borrow())
}

fn jet_jit_shared_set(handle: i64, value: i64) {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return;
    };
    if let Some(staged) = shared_transaction_stage_for_write(&shared) {
        *staged.borrow_mut() = value;
        return;
    }
    let Some(permit) =
        shared_protocol::jet_shared_acquire(&shared.protocol, true, || false)
    else {
        return;
    };
    let Ok(next) = shared_next_revision(&shared) else {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("SharedRevisionError.GenerationExhausted");
        });
        drop(permit);
        return;
    };
    shared.value.store(value, Ordering::Release);
    shared.revision.store(next, Ordering::Release);
    drop(permit);
}

fn jet_jit_shared_replace(handle: i64, value: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return 0;
    };
    if let Some(staged) = shared_transaction_stage_for_write(&shared) {
        let previous = *staged.borrow();
        *staged.borrow_mut() = value;
        return previous;
    }
    let Some(permit) =
        shared_protocol::jet_shared_acquire(&shared.protocol, true, || false)
    else {
        return 0;
    };
    let Ok(next) = shared_next_revision(&shared) else {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("SharedRevisionError.GenerationExhausted");
        });
        drop(permit);
        return 0;
    };
    let previous = shared.value.swap(value, Ordering::AcqRel);
    shared.revision.store(next, Ordering::Release);
    drop(permit);
    previous
}

fn shared_callback_slot(callback: i64) -> Option<crate::runtime_host::JitCallableSlot> {
    Concurrency::with_runtime_mut(|rt| crate::runtime_host::jit_callable_parts(rt, callback))
}

fn shared_callback_fault(message: &str) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.set_host_fault(message);
        0
    })
}

fn jet_jit_shared_read(handle: i64, callback: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return shared_callback_fault("Shared.read received an invalid shared handle");
    };
    if let Some(staged) = shared_transaction_staged(&shared) {
        let value = *staged.borrow();
        let Some(slot) = shared_callback_slot(callback) else {
            return shared_callback_fault("Shared.read callback handle is invalid");
        };
        let Some(result) = crate::runtime_host::invoke_universal_unary(slot, value) else {
            return shared_callback_fault("Shared.read callback has no unary universal thunk");
        };
        return result;
    }
    let Some(permit) = shared_protocol::jet_shared_acquire(&shared.protocol, false, || false)
    else {
        return 0;
    };
    let value = shared.value.load(Ordering::Acquire);
    let result = shared_callback_slot(callback)
        .and_then(|slot| crate::runtime_host::invoke_universal_unary(slot, value));
    let stopped = Concurrency::with_runtime_mut(|rt| {
        crate::runtime_host::runtime_stop_pending(rt)
    });
    drop(permit);
    if stopped {
        return 0;
    }
    result.unwrap_or_else(|| shared_callback_fault("Shared.read callback is invalid"))
}

fn jet_jit_shared_edit(handle: i64, callback: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return shared_callback_fault("Shared.edit received an invalid shared handle");
    };
    if let Some(staged) = shared_transaction_stage_for_write(&shared) {
        let Some(slot) = shared_callback_slot(callback) else {
            return shared_callback_fault("Shared.edit callback handle is invalid");
        };
        let mut value = staged.borrow_mut();
        let address = (&mut *value as *mut i64) as i64;
        let Some(result) = crate::runtime_host::invoke_universal_unary(slot, address) else {
            return shared_callback_fault("Shared.edit callback has no unary universal thunk");
        };
        return result;
    }
    let Some(permit) = shared_protocol::jet_shared_acquire(&shared.protocol, true, || false)
    else {
        return 0;
    };
    let mut value = shared.value.load(Ordering::Acquire);
    let result = shared_callback_slot(callback).and_then(|slot| {
        let address = (&mut value as *mut i64) as i64;
        crate::runtime_host::invoke_universal_unary(slot, address)
    });
    let stopped = Concurrency::with_runtime_mut(|rt| {
        crate::runtime_host::runtime_stop_pending(rt)
    });
    if stopped {
        drop(permit);
        return 0;
    }
    let Some(result) = result else {
        drop(permit);
        return shared_callback_fault("Shared.edit callback is invalid");
    };
    let Ok(next) = shared_next_revision(&shared) else {
        drop(permit);
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("SharedRevisionError.GenerationExhausted");
            0
        });
    };
    shared.value.store(value, Ordering::Release);
    shared.revision.store(next, Ordering::Release);
    drop(permit);
    result
}


fn jet_jit_shared_capture(handle: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return 0;
    };
    let Some((revision, value)) = shared_capture_parts(&shared) else {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            0
        });
    };
    shared_snapshot_store(shared, revision, value)
}

fn shared_projected_capture(shared: &Arc<SharedState>, callback: i64) -> Option<(u64, i64)> {
    let permit = shared_protocol::jet_shared_acquire(&shared.protocol, false, || false)?;
    let revision = shared.revision.load(Ordering::Acquire);
    let value = shared.value.load(Ordering::Acquire);
    let slot = Concurrency::with_runtime_mut(|rt| {
        crate::runtime_host::jit_callable_parts(rt, callback)
    })?;
    let projected = crate::runtime_host::invoke_universal_unary(slot, value)?;
    drop(permit);
    Some((revision, projected))
}
fn shared_projected_capture_staged(
    shared: &Arc<SharedState>,
    callback: i64,
    staged: &std::rc::Rc<std::cell::RefCell<i64>>,
) -> Option<(u64, i64)> {
    let revision = shared_transaction_snapshot_revision(shared)?;
    let slot = Concurrency::with_runtime_mut(|rt| {
        crate::runtime_host::jit_callable_parts(rt, callback)
    })?;
    let projected =
        crate::runtime_host::invoke_universal_unary(slot, *staged.borrow())?;
    Some((revision, projected))
}

fn jet_jit_shared_capture_with(handle: i64, callback: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return 0;
    };
    let Some((revision, value)) = shared_projected_capture(&shared, callback) else {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("Shared.capture projection callback is invalid");
            0
        });
    };
    shared_snapshot_store(shared, revision, value)
}

fn shared_transaction_register_snapshot(
    snapshot: Arc<SharedSnapshot>,
    protocol: Arc<shared_protocol::JetSharedProtocol>,
) -> bool {
    SHARED_TRANSACTIONS.with(|transactions| {
        let mut transactions = transactions.borrow_mut();
        let Some(transaction) = transactions.last_mut() else {
            return false;
        };
        transaction
            .transaction
            .record_snapshot(protocol, snapshot.valid.clone());
        true
    })
}

fn jet_jit_shared_capture_txn_plain(handle: i64, _stm: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return 0;
    };
    if !shared_transaction_touch(&shared) {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("Shared.capture_txn requires an active transaction");
            0
        });
    }
    let captured = if let Some(staged) = shared_transaction_staged(&shared) {
        let Some(revision) = shared_transaction_snapshot_revision(&shared) else {
            return 0;
        };
        Some((revision, *staged.borrow()))
    } else {
        shared_capture_parts(&shared)
    };
    let Some((revision, value)) = captured else {
        return 0;
    };
    let protocol = Arc::clone(&shared.protocol);
    let token = shared_snapshot_store(shared, revision, value);
    let Some(snapshot) = shared_snapshot_load(token) else {
        return 0;
    };
    if !shared_transaction_register_snapshot(snapshot, protocol) {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("Shared.capture_txn requires an active transaction");
            0
        });
    }
    token
}

fn jet_jit_shared_capture_txn(handle: i64, _stm: i64, callback: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return 0;
    };
    if !shared_transaction_touch(&shared) {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("Shared.capture_txn requires an active transaction");
            0
        });
    }
    let captured = if let Some(staged) = shared_transaction_staged(&shared) {
        shared_projected_capture_staged(&shared, callback, &staged)
    } else {
        shared_projected_capture(&shared, callback)
    };
    let Some((revision, value)) = captured else {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("Shared.capture projection callback is invalid");
            0
        });
    };
    let protocol = Arc::clone(&shared.protocol);
    let token = shared_snapshot_store(shared, revision, value);
    let Some(snapshot) = shared_snapshot_load(token) else {
        return 0;
    };
    if !shared_transaction_register_snapshot(snapshot, protocol) {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("Shared.capture_txn requires an active transaction");
            0
        });
    }
    token
}

fn jet_jit_shared_try_replace(handle: i64, snapshot_handle: i64, value: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return 0;
    };
    let Some(permit) =
        shared_protocol::jet_shared_acquire(&shared.protocol, true, || false)
    else {
        return 0;
    };
    let Some(snapshot) = shared_snapshot_load(snapshot_handle) else {
        let result = Concurrency::with_runtime_mut(|rt| shared_revision_error_result(rt, 0));
        drop(permit);
        return result;
    };
    if !Arc::ptr_eq(&shared, &snapshot.owner) {
        let result = Concurrency::with_runtime_mut(|rt| shared_revision_error_result(rt, 0));
        drop(permit);
        return result;
    }
    if !snapshot
        .valid
        .load(std::sync::atomic::Ordering::Acquire)
        || snapshot
            .consumed
            .load(std::sync::atomic::Ordering::Acquire)
    {
        let result = Concurrency::with_runtime_mut(|rt| {
            crate::runtime_host::alloc_jit_result(rt, true, 0)
        });
        drop(permit);
        return result;
    }
    if shared.revision.load(Ordering::Acquire) != snapshot.revision {
        let result = Concurrency::with_runtime_mut(|rt| {
            crate::runtime_host::alloc_jit_result(rt, true, 0)
        });
        drop(permit);
        return result;
    }
    let Ok(next) = shared_next_revision(&shared) else {
        let result = Concurrency::with_runtime_mut(|rt| shared_revision_error_result(rt, 1));
        drop(permit);
        return result;
    };
    if snapshot
        .consumed
        .swap(true, std::sync::atomic::Ordering::AcqRel)
    {
        let result = Concurrency::with_runtime_mut(|rt| {
            crate::runtime_host::alloc_jit_result(rt, true, 0)
        });
        drop(permit);
        return result;
    }
    snapshot
        .valid
        .store(false, std::sync::atomic::Ordering::Release);
    shared.value.store(value, Ordering::Release);
    shared.revision.store(next, Ordering::Release);
    let result = Concurrency::with_runtime_mut(|rt| {
        crate::runtime_host::alloc_jit_result(rt, true, 1)
    });
    drop(permit);
    result
}

fn jet_jit_shared_snapshot_value(snapshot_handle: i64) -> i64 {
    let Some(snapshot) = shared_snapshot_load(snapshot_handle) else {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("SharedSnapshot.value received an invalid snapshot");
            0
        });
    };
    snapshot.value
}
fn jet_jit_shared_begin(handle: i64, editable: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return 0;
    };
    let Some(permit) =
        shared_protocol::jet_shared_acquire(&shared.protocol, editable != 0, || false)
    else {
        return 0;
    };
    let value = shared.value.load(Ordering::Acquire);
    SHARED_ACTIVE_PERMITS.with(|permits| permits.borrow_mut().push((handle, permit)));
    value
}

fn jet_jit_shared_end_read(handle: i64) {
    drop(take_active_shared_permit(handle));
}

fn jet_jit_shared_end_write(handle: i64, value: i64) {
    let permit = take_active_shared_permit(handle);
    if let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) {
        if let Ok(next) = shared_next_revision(&shared) {
            shared.value.store(value, Ordering::Release);
            shared.revision.store(next, Ordering::Release);
        } else {
            Concurrency::with_runtime_mut(|rt| {
                rt.set_trap("SharedRevisionError.GenerationExhausted");
            });
        }
    }
    drop(permit);
}

/// D-SHARED-CYCLE1=C: weak handle is the same slot index; upgrade packs
/// Option as `0` (None) or `handle + 1` (Some).
fn jet_jit_shared_downgrade(handle: i64) -> i64 {
    handle
}

fn jet_jit_shared_strong_count(handle: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        shared(rt, handle)
            // `shared()` clones the Arc out of `rt.shareds` to hand back an
            // owned handle, so `state` itself holds one strong ref the Jet
            // program never asked for; subtract it back out (D-SHARED-CYCLE1=C).
            .map(|state| (Arc::strong_count(&state) - 1) as i64)
            .unwrap_or(0)
    })
}

fn jet_jit_shared_weak_upgrade(weak: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if shared(rt, weak).is_some() {
            weak.wrapping_add(1)
        } else {
            0
        }
    })
}

fn jet_jit_condition_new() -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        rt.conditions.push(Arc::new(ConditionState::new()));
        rt.conditions.len() as i64
    })
}

fn jet_jit_condition_notify_one(handle: i64) {
    if let Some(condition) = Concurrency::with_runtime_mut(|rt| condition(rt, handle)) {
        shared_protocol::jet_shared_condition_notify_one(&condition.protocol);
    }
}

fn jet_jit_condition_notify_all(handle: i64) {
    if let Some(condition) = Concurrency::with_runtime_mut(|rt| condition(rt, handle)) {
        shared_protocol::jet_shared_condition_notify_all(&condition.protocol);
    }
}

fn jet_jit_shared_guard_begin(handle: i64, editable: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            0
        });
    };
    let Some(state) =
        shared_protocol::jet_shared_guard_acquire(&shared.protocol, editable != 0, || false)
    else {
        return 0;
    };
    let value = shared.value.load(Ordering::Acquire);
    Concurrency::with_runtime_mut(|rt| pack_shared_guard(rt, handle, value, state))
}

fn jet_jit_shared_guard_read(handle: i64) -> i64 {
    jet_jit_shared_guard_begin(handle, 0)
}

fn jet_jit_shared_guard_edit(handle: i64) -> i64 {
    jet_jit_shared_guard_begin(handle, 1)
}

fn jet_jit_shared_guard_map(guard: i64, field: i64, editable: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if guard_shared_handle(rt, guard).is_none() {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            return 0;
        }
        let Some(state) = guard_state(rt, guard) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            return 0;
        };
        match shared_protocol::jet_shared_guard_map(&state, field, editable != 0) {
            Ok(mapped) => {
                let shared_handle = guard_shared_handle(rt, guard)
                    .expect("validated SharedGuard carrier lost its shared handle");
                let Some(value) = rt.heap.record_get_int(guard, GUARD_VALUE) else {
                    rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                    return 0;
                };
                // Mapping consumes the source guard. Keep the source carrier
                // as an inert move marker and give the mapped projection its
                // own identity; the shared permit stays alive through the new
                // state, so lexical cleanup releases it exactly once.
                rt.shared_guard_states.remove(&guard);
                let _ = rt.heap.record_set_int(guard, GUARD_SHARED, 0);
                pack_shared_guard(rt, shared_handle, value, mapped)
            }
            Err(message) => {
                rt.set_trap(message);
                0
            }
        }
    })
}

fn jet_jit_shared_guard_split(guard: i64, first: i64, second: i64, editable: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        if guard_shared_handle(rt, guard).is_none() {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            return 0;
        }
        let Some(state) = guard_state(rt, guard) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            return 0;
        };
        match shared_protocol::jet_shared_guard_split(&state, first, second, editable != 0) {
            Ok((first_state, second_state)) => {
                let shared_handle = guard_shared_handle(rt, guard)
                    .expect("validated SharedGuard carrier lost its shared handle");
                let Some(value) = rt.heap.record_get_int(guard, GUARD_VALUE) else {
                    rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                    return 0;
                };
                rt.shared_guard_states.remove(&guard);
                let _ = rt.heap.record_set_int(guard, GUARD_SHARED, 0);
                let first = pack_shared_guard(rt, shared_handle, value, first_state);
                let second = pack_shared_guard(rt, shared_handle, value, second_state);
                let pair = rt.heap.alloc_record(2);
                let _ = rt.heap.record_set_int(pair, 0, first);
                let _ = rt.heap.record_set_int(pair, 1, second);
                pair
            }
            Err(message) => {
                rt.set_trap(message);
                0
            }
        }
    })
}

fn jet_jit_shared_guard_clone(guard: i64, editable: i64) -> i64 {
    let Some((shared, value, state)) = Concurrency::with_runtime_mut(|rt| {
        let shared = guard_shared_handle(rt, guard)?;
        let value = rt.heap.record_get_int(guard, GUARD_VALUE)?;
        let state = rt.shared_guard_states.get(&guard)?.clone();
        Some((shared, value, state))
    }) else {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
        });
        return 0;
    };
    let state = match shared_protocol::jet_shared_guard_clone(&state, editable != 0) {
        Ok(state) => state,
        Err(message) => {
            return Concurrency::with_runtime_mut(|rt| {
                rt.set_trap(message);
                0
            });
        }
    };
    Concurrency::with_runtime_mut(|rt| pack_shared_guard(rt, shared, value, state))
}

fn jet_jit_shared_guard_value(guard: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field)) = readable_guard_slot(rt, guard) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            return 0;
        };
        let Some(value) = rt.heap.record_get_int(record, field) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
            return 0;
        };
        value
    })
}

fn jet_jit_shared_guard_value_f64(guard: i64) -> f64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field)) = readable_guard_slot(rt, guard) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            return 0.0;
        };
        if field == GUARD_VALUE && record == guard {
            let Some(value) = rt.heap.record_get_int(record, field) else {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                return 0.0;
            };
            return f64::from_bits(value as u64);
        }
        let Some(value) = rt.heap.record_get_float(record, field) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
            return 0.0;
        };
        value
    })
}

fn jet_jit_shared_guard_value_bool(guard: i64) -> i8 {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field)) = readable_guard_slot(rt, guard) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            return 0;
        };
        if field == GUARD_VALUE && record == guard {
            let Some(value) = rt.heap.record_get_int(record, field) else {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                return 0;
            };
            return i8::from(value != 0);
        }
        let Some(value) = rt.heap.record_get_bool(record, field) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
            return 0;
        };
        i8::from(value)
    })
}

fn jet_jit_shared_guard_value_char(guard: i64) -> i32 {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field)) = readable_guard_slot(rt, guard) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            return 0;
        };
        let value = if field == GUARD_VALUE && record == guard {
            let Some(value) = rt.heap.record_get_int(record, field) else {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                return 0;
            };
            value as i32
        } else {
            let Some(value) = rt.heap.record_get_char(record, field) else {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                return 0;
            };
            value as i32
        };
        if shared_protocol::jet_shared_guard_validate_char(value).is_err() {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_CHARACTER_STORAGE_FAILED);
            return 0;
        }
        value
    })
}

fn jet_jit_shared_guard_value_string(guard: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field)) = readable_guard_slot(rt, guard) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            return 0;
        };
        if field == GUARD_VALUE && record == guard {
            let Some(value) = rt.heap.record_get_int(record, field) else {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                return 0;
            };
            return value;
        }
        let Some(value) = rt.heap.record_get_string(record, field) else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
            return 0;
        };
        value
    })
}

fn readable_guard_slot(rt: &crate::JitRuntime, guard: i64) -> Option<(i64, i64)> {
    guard_shared_handle(rt, guard)?;
    let state = guard_state(rt, guard)?;
    if !state.held() {
        return None;
    }
    guard_projection_slot(rt, guard, state.path())
}

fn editable_guard_slot(
    rt: &crate::JitRuntime,
    guard: i64,
) -> Result<(i64, i64, bool), &'static str> {
    guard_shared_handle(rt, guard).ok_or(shared_protocol::JET_SHARED_GUARD_INVALID)?;
    let state = guard_state(rt, guard).ok_or(shared_protocol::JET_SHARED_GUARD_INVALID)?;
    shared_protocol::jet_shared_guard_require_edit(&state)?;
    let (record, field) = guard_projection_slot(rt, guard, state.path())
        .ok_or(shared_protocol::JET_SHARED_GUARD_INVALID)?;
    Ok((record, field, state.path().is_empty()))
}

fn editable_guard_slot_or_trap(rt: &mut crate::JitRuntime, guard: i64) -> Option<(i64, i64, bool)> {
    match editable_guard_slot(rt, guard) {
        Ok(slot) => Some(slot),
        Err(message) => {
            rt.set_trap(message);
            None
        }
    }
}

fn store_root_guard_value(
    rt: &mut crate::JitRuntime,
    guard: i64,
    value: i64,
) -> Result<(), &'static str> {
    let shared_handle =
        guard_shared_handle(rt, guard).ok_or(shared_protocol::JET_SHARED_GUARD_INVALID)?;
    let shared = shared(rt, shared_handle).ok_or(shared_protocol::JET_SHARED_GUARD_INVALID)?;
    shared.value.store(value, Ordering::Release);
    Ok(())
}

fn jet_jit_shared_guard_set_value(guard: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field, _root)) = editable_guard_slot_or_trap(rt, guard) else {
            return;
        };
        if rt.heap.record_set_int(record, field, value).is_none() {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
        }
    });
}

fn jet_jit_shared_guard_set_value_f64(guard: i64, value: f64) {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field, root)) = editable_guard_slot_or_trap(rt, guard) else {
            return;
        };
        if root {
            if rt
                .heap
                .record_set_int(record, field, value.to_bits() as i64)
                .is_none()
            {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
            }
        } else if rt.heap.record_set_float(record, field, value).is_none() {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
        }
    });
}

fn jet_jit_shared_guard_set_value_bool(guard: i64, value: i8) {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field, root)) = editable_guard_slot_or_trap(rt, guard) else {
            return;
        };
        let value = value != 0;
        if root {
            if rt
                .heap
                .record_set_int(record, field, i64::from(value))
                .is_none()
            {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
            }
        } else if rt.heap.record_set_bool(record, field, value).is_none() {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
        }
    });
}

fn jet_jit_shared_guard_set_value_char(guard: i64, value: i32) {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field, root)) = editable_guard_slot_or_trap(rt, guard) else {
            return;
        };
        let value = match shared_protocol::jet_shared_guard_validate_char(value) {
            Ok(value) => value,
            Err(message) => {
                rt.set_trap(message);
                return;
            }
        };
        if root {
            if rt
                .heap
                .record_set_int(record, field, i64::from(value as u32))
                .is_none()
            {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
            }
        } else if rt.heap.record_set_char(record, field, value).is_none() {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
        }
    });
}

fn jet_jit_shared_guard_set_value_string(guard: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field, root)) = editable_guard_slot_or_trap(rt, guard) else {
            return;
        };
        if root {
            if rt.heap.record_set_int(record, field, value).is_none() {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
            }
        } else if rt.heap.record_set_string(record, field, value).is_none() {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
        }
    });
}

fn jet_jit_shared_guard_end(guard: i64) {
    let Some((shared, value, editable, root, state)) = Concurrency::with_runtime_mut(|rt| {
        let Some(shared_handle) = guard_shared_handle(rt, guard) else {
            return None;
        };
        let state = rt.shared_guard_states.remove(&guard)?;
        let root = state.path().is_empty();
        let value = if root {
            let Some(value) = rt.heap.record_get_int(guard, GUARD_VALUE) else {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                return None;
            };
            value
        } else {
            0
        };
        let editable = state.editable();
        let _ = rt.heap.record_set_int(guard, GUARD_SHARED, 0);
        Some((shared(rt, shared_handle), value, editable, root, state))
    }) else {
        return;
    };
    if let Some(shared) = shared {
        if editable && root {
            if let Ok(next) = shared_next_revision(&shared) {
                shared.value.store(value, Ordering::Release);
                shared.revision.store(next, Ordering::Release);
            } else {
                Concurrency::with_runtime_mut(|rt| {
                    rt.set_trap("SharedRevisionError.GenerationExhausted");
                });
            }
        }
    }
    drop(state);
}

fn shared_guard_result(rt: &mut crate::JitRuntime, ok: bool, message: Option<&str>) -> i64 {
    let bits = message
        .map(|message| rt.heap.alloc_string(message.to_string()) as u64)
        .unwrap_or(0);
    crate::runtime_host::alloc_jit_result(rt, ok, bits)
}

fn jet_jit_shared_guard_wait_once(guard: i64, condition_handle: i64) -> i64 {
    let Some((shared_handle, state, condition)) = Concurrency::with_runtime_mut(|rt| {
        let shared_handle = guard_shared_handle(rt, guard)?;
        let state = guard_state(rt, guard)?;
        let condition = condition(rt, condition_handle)?;
        Some((shared_handle, state, condition))
    }) else {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap(shared_protocol::JetSharedGuardWaitError::Invalid.message());
            shared_guard_result(
                rt,
                false,
                Some(shared_protocol::JetSharedGuardWaitError::Invalid.message()),
            )
        });
    };
    let waiter = Arc::new(JitConditionWaiter::new());
    let waited = jet_codegen::scheduler::jet_scheduler_wait_without_unwind(|| {
        shared_protocol::jet_shared_guard_wait_once(
            Some(state.as_ref()),
            Some(&condition.protocol),
            waiter,
        )
    });
    match waited {
        jet_codegen::scheduler::JetSchedulerWait::Ready(Ok(())) => {
            let Some(fresh) = Concurrency::with_runtime_mut(|rt| {
                Some(shared(rt, shared_handle)?.value.load(Ordering::Acquire))
            }) else {
                return Concurrency::with_runtime_mut(|rt| {
                    rt.set_trap(shared_protocol::JetSharedGuardWaitError::Invalid.message());
                    shared_guard_result(
                        rt,
                        false,
                        Some(shared_protocol::JetSharedGuardWaitError::Invalid.message()),
                    )
                });
            };
            Concurrency::with_runtime_mut(|rt| {
                if rt.heap.record_set_int(guard, GUARD_VALUE, fresh).is_none() {
                    rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                    return shared_guard_result(
                        rt,
                        false,
                        Some(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED),
                    );
                }
                shared_guard_result(rt, true, None)
            })
        }
        jet_codegen::scheduler::JetSchedulerWait::Ready(Err(error)) => {
            Concurrency::with_runtime_mut(|rt| {
                if error.traps() {
                    rt.set_trap(error.message());
                }
                shared_guard_result(rt, false, Some(error.message()))
            })
        }
        jet_codegen::scheduler::JetSchedulerWait::Cancelled => {
            Concurrency::with_runtime_mut(|rt| {
                shared_guard_result(
                    rt,
                    false,
                    Some(shared_protocol::JetSharedGuardWaitError::Cancelled.message()),
                )
            })
        }
        jet_codegen::scheduler::JetSchedulerWait::Deadline(rendered) => {
            Concurrency::with_runtime_mut(|rt| {
                rt.set_deadline(rendered);
                shared_guard_result(
                    rt,
                    false,
                    Some(shared_protocol::JetSharedGuardWaitError::Cancelled.message()),
                )
            })
        }
        jet_codegen::scheduler::JetSchedulerWait::Panicked(message) => {
            Concurrency::with_runtime_mut(|rt| {
                rt.set_trap(&message);
                shared_guard_result(rt, false, Some(message.as_str()))
            })
        }
    }
}

fn jet_jit_shared_txn_begin() {
    SHARED_TRANSACTIONS.with(|transactions| {
        transactions.borrow_mut().push(SharedTransaction {
            transaction: shared_protocol::jet_shared_transaction_begin(),
        });
    });
}

fn jet_jit_shared_txn_touch(handle: i64) {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return;
    };
    let _ = shared_transaction_touch(&shared);
}

fn jet_jit_shared_txn_record(handle: i64, callback_ptr: i64, environment: i64, record: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return 0;
    };
    if callback_ptr == 0 {
        return 0;
    }
    // The callback address is produced by Cranelift `func_addr` for the fixed
    // `(environment, current) -> updated` ABI above. Run it once against the
    // transaction-local value; commit only publishes that value.
    let callback: SharedTransactionCallback = unsafe { std::mem::transmute(callback_ptr as usize) };
    let Some(staged) = shared_transaction_stage_for_write(&shared) else {
        return 0;
    };
    let current = *staged.borrow();
    let updated = unsafe { callback(environment, current) };
    if record != 0 {
        let delta = Box::new(move || {
            Concurrency::with_runtime_mut(|rt| {
                if rt.heap.record_assign_from(current, updated).is_none() {
                    rt.set_trap(shared_protocol::JET_SHARED_TRANSACTION_VALUE_STORAGE_FAILED);
                }
            });
        });
        let protocol = Arc::clone(&shared.protocol);
        SHARED_TRANSACTIONS.with(|transactions| {
            let mut transactions = transactions.borrow_mut();
            if let Some(transaction) = transactions.last_mut() {
                transaction.transaction.record_edit(protocol, delta);
            }
        });
    }
    *staged.borrow_mut() = updated;
    1
}

fn jet_jit_shared_txn_commit() {
    let Some(transaction) =
        SHARED_TRANSACTIONS.with(|transactions| transactions.borrow_mut().pop())
    else {
        return;
    };
    transaction.transaction.commit();
}

fn jet_jit_shared_txn_abort() {
    SHARED_TRANSACTIONS.with(|transactions| {
        transactions.borrow_mut().pop();
    });
}

fn jet_jit_expiring_new(value: i64, duration: i64, clock: i64, secret: i64) -> i64 {
    // SigningKey / X25519 / Secret live in crypto_values (#1222). Claim a
    // zeroize mirror here; keep the crypto handle live for `with` loans.
    let owned_secret = if secret != 0 {
        match crate::Crypto::claim_expiring_secret(value) {
            Some(state) => Some(state),
            None => {
                Concurrency::with_runtime_mut(|rt| {
                    rt.set_trap("secret key handle is invalid or already moved");
                });
                return 0;
            }
        }
    } else {
        None
    };
    Concurrency::with_runtime_mut(|rt| {
        let now = rt.clock_now(clock);
        rt.expirings.push(ExpiringState {
            value,
            expires_at: now.saturating_add(duration.max(0)),
            clock,
            secret: owned_secret,
        });
        rt.expirings.len() as i64
    })
}

fn jet_jit_expiring_get(handle: i64, clock: i64) -> i64 {
    let (status, drop_crypto) = Concurrency::with_runtime_mut(|rt| {
        let stored_clock = rt
            .expirings
            .get((handle as usize).wrapping_sub(1))
            .map(|value| value.clock)
            .unwrap_or(0);
        let clock = if clock == 0 { stored_clock } else { clock };
        let now = rt.clock_now(clock);
        let Some(value) = rt.expirings.get_mut((handle as usize).wrapping_sub(1)) else {
            return (0_i64, None);
        };
        if now > value.expires_at {
            let crypto_handle = value.value;
            value.secret.take();
            value.value = 0;
            return (0, Some(crypto_handle));
        }
        (value.value + 1, None)
    });
    if let Some(crypto_handle) = drop_crypto {
        crate::Crypto::drop_crypto_handle(crypto_handle);
    }
    status
}

fn jet_jit_expiring_is_valid(handle: i64, clock: i64) -> i8 {
    i8::from(jet_jit_expiring_get(handle, clock) != 0)
}

fn jet_jit_volatile_read(address: i64) -> i64 {
    let allowed = Concurrency::with_runtime_mut(|rt| {
        jet_jit_sentry_check_fixed(rt, address, "volatile_read")
    });
    if !allowed {
        return 0;
    }
    // SAFETY: the sentry check above owns validity of the typed pointer.
    unsafe { std::ptr::read_volatile(address as *const i64) }
}

fn jet_jit_volatile_write(address: i64, value: i64) {
    let allowed = Concurrency::with_runtime_mut(|rt| {
        jet_jit_sentry_check_fixed(rt, address, "volatile_write")
    });
    if !allowed {
        return;
    }
    // SAFETY: the sentry check above owns validity of the typed pointer.
    unsafe { std::ptr::write_volatile(address as *mut i64, value) };
}

fn jet_jit_shared_edit_txn(handle: i64, _stm: i64, callback: i64) -> i64 {
    jet_jit_shared_edit(handle, callback)
}

fn jet_jit_shared_read_txn(handle: i64, _stm: i64, callback: i64) -> i64 {
    jet_jit_shared_read(handle, callback)
}

host_fns! {
    struct MemoryHostFns;
    register: register_memory_symbols;
    declare: declare_memory_host_fns(module) {
        use cranelift_codegen::ir::{types, AbiParam, Signature};
        use cranelift_module::Module;
        let cc = module.target_config().default_call_conv;
        let mut noarg_i64 = Signature::new(cc);
        noarg_i64.returns.push(AbiParam::new(types::I64));
        let noarg_void = Signature::new(cc);
        let mut unary = Signature::new(cc);
        unary.params.push(AbiParam::new(types::I64));
        unary.returns.push(AbiParam::new(types::I64));
        let mut unary_f64 = Signature::new(cc);
        unary_f64.params.push(AbiParam::new(types::I64));
        unary_f64.returns.push(AbiParam::new(types::F64));
        let mut unary_i8 = Signature::new(cc);
        unary_i8.params.push(AbiParam::new(types::I64));
        unary_i8.returns.push(AbiParam::new(types::I8));
        let mut unary_i32 = Signature::new(cc);
        unary_i32.params.push(AbiParam::new(types::I64));
        unary_i32.returns.push(AbiParam::new(types::I32));
        let mut binary = unary.clone();
        binary.params.push(AbiParam::new(types::I64));
        let mut ternary = binary.clone();
        ternary.params.push(AbiParam::new(types::I64));
        let mut unary_void = Signature::new(cc);
        unary_void.params.push(AbiParam::new(types::I64));
        let mut binary_void = unary_void.clone();
        binary_void.params.push(AbiParam::new(types::I64));
        let mut ternary_void = binary_void.clone();
        ternary_void.params.push(AbiParam::new(types::I64));
        let mut binary_f64_void = Signature::new(cc);
        let mut sig_index_pool_get = Signature::new(cc);
        sig_index_pool_get
            .params
            .extend([AbiParam::new(types::I64); 6]);
        sig_index_pool_get
            .returns
            .push(AbiParam::new(types::I64));
        let mut sig_index_pool_set = Signature::new(cc);
        sig_index_pool_set
            .params
            .extend([AbiParam::new(types::I64); 7]);

        binary_f64_void.params.push(AbiParam::new(types::I64));
        binary_f64_void.params.push(AbiParam::new(types::F64));
        let mut binary_i8_void = Signature::new(cc);
        binary_i8_void.params.push(AbiParam::new(types::I64));
        let mut binary_i32_void = Signature::new(cc);
        binary_i32_void.params.push(AbiParam::new(types::I64));
        binary_i32_void.params.push(AbiParam::new(types::I32));
        let mut quaternary = Signature::new(cc);
        for _ in 0..4 {
            quaternary.params.push(AbiParam::new(types::I64));
        }
        quaternary.returns.push(AbiParam::new(types::I64));
        let mut quinary_void = Signature::new(cc);
        for _ in 0..5 {
            quinary_void.params.push(AbiParam::new(types::I64));
        }
        let mut binary_i8 = Signature::new(cc);
        binary_i8.params.push(AbiParam::new(types::I64));
        binary_i8.params.push(AbiParam::new(types::I64));
        binary_i8.returns.push(AbiParam::new(types::I8));


    }
    allocator_new: "jet_jit_allocator_new" => jet_jit_allocator_new: noarg_i64;
    allocator_new_named: "jet_jit_allocator_new_named" => jet_jit_allocator_new_named: unary;
    allocator_new_capacity: "jet_jit_allocator_new_capacity" => jet_jit_allocator_new_capacity: binary;
    allocator_alloc: "jet_jit_allocator_alloc" => jet_jit_allocator_alloc: ternary;
    allocator_view_read: "jet_jit_allocator_view_read" => jet_jit_allocator_view_read: unary;
    sentry_check: "jet_jit_sentry_check" => jet_jit_sentry_check: quinary_void;
    sentry_scope_enter: "jet_jit_sentry_scope_enter" => jet_jit_sentry_scope_enter: quinary_void;
    sentry_scope_exit: "jet_jit_sentry_scope_exit" => jet_jit_sentry_scope_exit: noarg_void;
    sentry_function_mark: "jet_jit_sentry_function_mark" => jet_jit_sentry_function_mark: noarg_void;
    sentry_function_exit: "jet_jit_sentry_function_exit" => jet_jit_sentry_function_exit: noarg_void;
    sentry_frame_enter: "jet_jit_sentry_frame_enter" => jet_jit_sentry_frame_enter: noarg_void;
    sentry_frame_exit: "jet_jit_sentry_frame_exit" => jet_jit_sentry_frame_exit: noarg_void;
    sentry_register_stack: "jet_jit_sentry_register_stack" => jet_jit_sentry_register_stack: binary_void;
    view_string: "jet_jit_view_string" => jet_jit_view_string: unary;
    index_pool_get: "jet_std::jet_pool_get" => jet_jit_pool_get_checked: sig_index_pool_get;
    index_pool_get_mut: "jet_std::jet_pool_get_mut" => jet_jit_pool_get_checked: sig_index_pool_get;
    index_pool_set: "jet_std::jet_pool_set" => jet_jit_index_pool_set: sig_index_pool_set;

    allocator_view_write: "jet_jit_allocator_view_write" => jet_jit_allocator_view_write: binary_void;
    allocator_try_alloc: "jet_jit_allocator_try_alloc" => jet_jit_allocator_try_alloc: ternary;
    allocator_reset: "jet_jit_allocator_reset" => jet_jit_allocator_reset: unary_void;
    allocator_close: "jet_jit_allocator_close" => jet_jit_allocator_close: unary_void;
    gc_read: "jet_jit_gc_read" => jet_jit_gc_read: unary;
    gc_edit: "jet_jit_gc_edit" => jet_jit_gc_edit: binary_void;
    gc_clear_edges: "jet_jit_gc_clear_edges" => jet_jit_gc_clear_edges: unary_void;
    gc_add_edge: "jet_jit_gc_add_edge" => jet_jit_gc_add_edge: ternary_void;
    gc_read_canonical: "jet_gc_read" => jet_jit_gc_read: unary;
    gc_edit_clear: "jet_gc_edit_clear" => jet_jit_gc_edit_clear: binary;
    gc_edit_pop: "jet_gc_edit_pop" => jet_jit_gc_edit_pop: binary;
    gc_edit_remove_index: "jet_gc_edit_remove_index" => jet_jit_gc_edit_remove_index: ternary;
    gc_edit_insert_index: "jet_gc_edit_insert_index" => jet_jit_gc_edit_insert_index: quaternary;
    gc_edit_prepend: "jet_gc_edit_prepend" => jet_jit_gc_edit_prepend: ternary;
    gc_edit_additive: "jet_gc_edit_additive" => jet_jit_gc_edit_additive: ternary;
    gc_edit_plain: "jet_gc_edit_plain" => jet_jit_gc_edit_plain: binary;
    gc_edit_edge_slot: "jet_gc_edit_edge_slot" => jet_jit_gc_edit_edge_slot: quaternary;
    pool_new: "jet_std::JetPool::new" => jet_jit_pool_new: noarg_i64;
    pool_add: "jet_std::JetPool::add" => jet_jit_pool_add: binary;
    pool_get: "jet_jit_pool_get" => jet_jit_pool_get: quaternary;

    shared_new_static: "jet_std::JetShared::new" => jet_jit_shared_new: unary;
    shared_new_rooted: "::jet_std::JetShared::new" => jet_jit_shared_new: unary;
    shared_get: "jet_shared_get" => jet_jit_shared_get: unary;
    shared_set: "jet_shared_set" => jet_jit_shared_set: binary_void;
    shared_replace: "jet_shared_replace" => jet_jit_shared_replace: binary;
    shared_read: "jet_shared_read" => jet_jit_shared_read: binary;
    shared_edit: "jet_shared_edit" => jet_jit_shared_edit: binary;
    shared_edit_txn: "jet_shared_edit_txn" => jet_jit_shared_edit_txn: ternary;
    shared_read_txn: "jet_shared_read_txn" => jet_jit_shared_read_txn: ternary;
    shared_capture: "jet_shared_capture" => jet_jit_shared_capture: unary;
    shared_capture_with: "jet_shared_capture_with" => jet_jit_shared_capture_with: binary;
    shared_capture_txn_plain: "jet_shared_capture_txn_plain" => jet_jit_shared_capture_txn_plain: binary;
    shared_capture_txn: "jet_shared_capture_txn" => jet_jit_shared_capture_txn: ternary;
    shared_try_replace: "jet_shared_try_replace" => jet_jit_shared_try_replace: ternary;
    shared_snapshot_value: "jet_shared_snapshot_value" => jet_jit_shared_snapshot_value: unary;
    pool_set: "jet_jit_pool_set" => jet_jit_pool_set: quinary_void;
    pool_remove: "jet_std::JetPool::remove" => jet_jit_pool_remove: binary;
    pool_ids: "jet_std::JetPool::ids" => jet_jit_pool_ids: unary;
    shared_new: "jet_jit_shared_new" => jet_jit_shared_new: unary;
    shared_begin: "jet_jit_shared_begin" => jet_jit_shared_begin: binary;
    shared_end_read: "jet_jit_shared_end_read" => jet_jit_shared_end_read: unary_void;
    shared_end_write: "jet_jit_shared_end_write" => jet_jit_shared_end_write: binary_void;
    shared_downgrade: "jet_jit_shared_downgrade" => jet_jit_shared_downgrade: unary;
    shared_strong_count: "jet_jit_shared_strong_count" => jet_jit_shared_strong_count: unary;
    shared_weak_upgrade: "jet_jit_shared_weak_upgrade" => jet_jit_shared_weak_upgrade: unary;
    condition_new: "jet_jit_condition_new" => jet_jit_condition_new: noarg_i64;
    condition_new_prelude: "jet_std::JetCondition::new" => jet_jit_condition_new: noarg_i64;
    condition_notify_one: "jet_jit_condition_notify_one" => jet_jit_condition_notify_one: unary_void;
    condition_notify_all: "jet_jit_condition_notify_all" => jet_jit_condition_notify_all: unary_void;
    shared_guard_read: "jet_shared_guard_read" => jet_jit_shared_guard_read: unary;
    shared_guard_edit: "jet_shared_guard_edit" => jet_jit_shared_guard_edit: unary;
    shared_guard_map: "jet_shared_guard_map" => jet_jit_shared_guard_map: ternary;
    shared_guard_split: "jet_shared_guard_split" => jet_jit_shared_guard_split: quaternary;
    shared_guard_clone: "jet_jit_shared_guard_clone" => jet_jit_shared_guard_clone: binary;
    shared_guard_value: "jet_jit_shared_guard_value" => jet_jit_shared_guard_value: unary;
    shared_guard_value_f64: "jet_jit_shared_guard_value_f64" => jet_jit_shared_guard_value_f64: unary_f64;
    shared_guard_value_bool: "jet_jit_shared_guard_value_bool" => jet_jit_shared_guard_value_bool: unary_i8;
    shared_guard_value_char: "jet_jit_shared_guard_value_char" => jet_jit_shared_guard_value_char: unary_i32;
    shared_guard_value_string: "jet_jit_shared_guard_value_string" => jet_jit_shared_guard_value_string: unary;
    shared_guard_set_value: "jet_jit_shared_guard_set_value" => jet_jit_shared_guard_set_value: binary_void;
    shared_guard_set_value_f64: "jet_jit_shared_guard_set_value_f64" => jet_jit_shared_guard_set_value_f64: binary_f64_void;
    shared_guard_set_value_bool: "jet_jit_shared_guard_set_value_bool" => jet_jit_shared_guard_set_value_bool: binary_i8_void;
    shared_guard_set_value_char: "jet_jit_shared_guard_set_value_char" => jet_jit_shared_guard_set_value_char: binary_i32_void;
    shared_guard_set_value_string: "jet_jit_shared_guard_set_value_string" => jet_jit_shared_guard_set_value_string: binary_void;
    shared_guard_end: "jet_jit_shared_guard_end" => jet_jit_shared_guard_end: unary_void;
    shared_guard_wait_once: "jet_jit_shared_guard_wait_once" => jet_jit_shared_guard_wait_once: binary;
    shared_txn_begin: "jet_jit_shared_txn_begin" => jet_jit_shared_txn_begin: Signature::new(cc);
    shared_txn_touch: "jet_jit_shared_txn_touch" => jet_jit_shared_txn_touch: unary_void;
    shared_txn_record: "jet_jit_shared_txn_record" => jet_jit_shared_txn_record: quaternary;
    shared_txn_commit: "jet_jit_shared_txn_commit" => jet_jit_shared_txn_commit: Signature::new(cc);
    shared_txn_abort: "jet_jit_shared_txn_abort" => jet_jit_shared_txn_abort: Signature::new(cc);
    expiring_new: "jet_jit_expiring_new" => jet_jit_expiring_new: quaternary;
    expiring_get: "jet_jit_expiring_get" => jet_jit_expiring_get: binary;
    expiring_is_valid: "jet_jit_expiring_is_valid" => jet_jit_expiring_is_valid: binary_i8;
    volatile_read: "std::ptr::read_volatile" => jet_jit_volatile_read: unary;
    volatile_write: "std::ptr::write_volatile" => jet_jit_volatile_write: binary_void;
}
