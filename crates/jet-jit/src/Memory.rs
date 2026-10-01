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
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

pub(crate) mod shared_protocol {
    include!("../../jet-codegen/src/Prelude/SharedProtocol.rs");
}

thread_local! {
    static SHARED_TRANSACTIONS: std::cell::RefCell<Vec<SharedTransaction>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static SHARED_ACTIVE_PERMITS:
        std::cell::RefCell<Vec<(i64, Arc<SharedStatePermit>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static SHARED_ACTIVE_NATIVE_GUARDS: std::cell::RefCell<Vec<(i64, i64)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

struct SharedTransaction {
    transaction: shared_protocol::JetSharedTransaction,
}

pub(crate) struct SharedSnapshot {
    owner: Arc<SharedState>,
    _physical_owner: Option<crate::SourceSharedInterop::SourceSharedInterop>,
    _owner_alias: crate::SourceSharedInterop::SourceSharedInteropOwnerAlias,
    type_id: u64,
    root_identity: usize,
    revision: u64,
    value: i64,
    valid: Arc<std::sync::atomic::AtomicBool>,
    consumed: Arc<std::sync::atomic::AtomicBool>,
}

pub(crate) struct SharedWeakOwner {
    pub(crate) type_id: u64,
    pub(crate) owner: crate::SourceSharedInterop::SourceSharedInteropWeak,
}

static NEXT_SHARED_SNAPSHOT_TICKET: AtomicI64 = AtomicI64::new(1);
static NEXT_SHARED_WEAK_TICKET: AtomicI64 = AtomicI64::new(1);

pub(crate) struct JitCallableEnvLifetime {
    pub(crate) env: i64,
    pub(crate) capture_type_ids: Vec<u64>,
    pub(crate) capture_owned: Vec<bool>,
    pub(crate) references: usize,
    pub(crate) portable_captures:
        Option<Arc<Mutex<Vec<jet_foundation::MIR::MirRuntimeValue>>>>,
}

fn next_shared_ticket(counter: &AtomicI64) -> Result<i64, String> {
    let next = counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |next| {
            (next > 0).then(|| next.checked_add(1)).flatten()
        })
        .map_err(|_| "Shared owner ticket table exhausted".to_string())?;
    i64::MIN
        .checked_add(next)
        .filter(|ticket| *ticket < 0)
        .ok_or_else(|| "Shared owner ticket table exhausted".to_string())
}

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
    value: Option<AtomicI64>,
    portable_value: Option<Mutex<Option<jet_foundation::MIR::MirRuntimeValue>>>,
    type_id: Option<u64>,
    revision: Option<AtomicU64>,
    owner_lifetime: Option<Mutex<SharedOwnerLifetime>>,
    payload_finalizer:
        Mutex<Option<crate::SourceSharedInterop::SourceSharedInteropPayloadFinalizer>>,
    next_owner_alias_token: AtomicI64,
}

struct SharedOwnerLifetime {
    logical_owners: usize,
    active_borrows: usize,
}

pub(crate) struct SharedStatePermit {
    state: Arc<SharedState>,
    protocol: Arc<shared_protocol::JetSharedPermit>,
    active: AtomicBool,
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
    /// D-TTL-ZEROIZE1=A: the value is a Core secret record whose `bytes`
    /// list is wiped in place when the entry expires.
    secret: bool,
}

/// A Core secret (`Secret`, `SigningKey`, `X25519SecretKey`) is a record whose
/// only field is its `bytes` list.
fn expiring_secret_bytes(record: i64) -> Option<i64> {
    Concurrency::with_runtime_mut(|rt| match rt.heap.record_get(record, 0)? {
        jet_rt::JetVal::Int(list) | jet_rt::JetVal::RecordRef(list)
            if rt.heap.list_len(*list).is_some() =>
        {
            Some(*list)
        }
        _ => None,
    })
}

/// Wipe an expired secret's bytes through the same resident wipe
/// `core.crypto.__zeroize` uses when the secret closes.
fn wipe_expired_secret(record: i64) {
    if let Some(bytes) = expiring_secret_bytes(record) {
        crate::Crypto::jet_jit_crypto_zeroize(bytes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::shared_protocol::JetSharedWaitHandoff;
    use crate::resident::fresh_runtime;
    use jet_pkg_model::Package::ReleaseDevtoolsPolicy;
    fn shared_int_descriptors() -> [crate::runtime_host::RuntimeTypeDescriptor; 2] {
        use crate::runtime_host::{RuntimeTypeDescriptor, RuntimeValueAbi, RuntimeValueKind};

        let int = RuntimeTypeDescriptor {
            id: 0x501,
            name: "Int".to_string(),
            canonical: "Int".to_string(),
            kind: RuntimeValueKind::Int,
            abi: RuntimeValueAbi::Int,
            integer_width: None,
            integer_range: None,
            element: None,
            key: None,
            value: None,
            ok: None,
            err: None,
            serde_tag: None,
            serde_untagged: false,
            serde_deny_unknown: false,
            cli: None,
            fields: Vec::new(),
            migration: None,
            variants: Vec::new(),
        };
        let shared = RuntimeTypeDescriptor {
            id: 0x502,
            name: "Shared<Int>".to_string(),
            canonical: "Shared<Int>".to_string(),
            kind: RuntimeValueKind::Shared,
            abi: RuntimeValueAbi::Handle,
            integer_width: None,
            integer_range: None,
            element: Some(0x501),
            key: None,
            value: None,
            ok: None,
            err: None,
            serde_tag: None,
            serde_untagged: false,
            serde_deny_unknown: false,
            cli: None,
            fields: Vec::new(),
            migration: None,
            variants: Vec::new(),
        };
        [int, shared]
    }

    #[test]
    fn native_shared_guard_views_track_map_split_and_clone_paths() {
        use jet_foundation::MIR::MirRuntimeValue;

        let mut runtime = fresh_runtime(ReleaseDevtoolsPolicy::default());
        runtime.install_type_descriptors(shared_int_descriptors());
        let owner_marker = Arc::new(());
        let owner_identity = Arc::as_ptr(&owner_marker) as usize;
        let interop = crate::SourceSharedInterop::SourceSharedInterop::from_value(
            0x502,
            MirRuntimeValue::Int(7),
        )
        .with_owner_identity(owner_identity)
        .with_protocol_order_key(owner_identity);
        Concurrency::set_active_runtime(Some(&mut runtime as *mut crate::JitRuntime));

        let handle = crate::runtime_host::native_shared_export_owner(interop)
            .expect("physical owner should export to the JIT runtime");
        let root_guard = jet_jit_shared_guard_begin(handle, 1);
        let root_view = crate::runtime_host::native_shared_guard_entry_view(root_guard)
            .expect("JIT guard should keep its typed physical view")
            .expect("root view should remain live");
        assert!(root_view.path().is_empty());
        assert!(root_view.editable());
        let physical_identity = root_view.physical_identity();
        let protocol_order_key = root_view.protocol_order_key();
        let lease = std::rc::Rc::clone(&root_view.lease);
        drop(root_view);

        let mapped_guard = jet_jit_shared_guard_map(root_guard, 41, 0);
        let mapped_view = crate::runtime_host::native_shared_guard_entry_view(mapped_guard)
            .expect("mapped guard should keep its typed physical view")
            .expect("mapped view should remain live");
        assert_eq!(mapped_view.path(), &[41]);
        assert!(!mapped_view.editable());
        assert_eq!(mapped_view.physical_identity(), physical_identity);
        assert_eq!(mapped_view.protocol_order_key(), protocol_order_key);
        assert!(std::rc::Rc::ptr_eq(&mapped_view.lease, &lease));

        let cloned_guard = jet_jit_shared_guard_clone(mapped_guard, 0);
        let cloned_view = crate::runtime_host::native_shared_guard_entry_view(cloned_guard)
            .expect("cloned guard should keep its typed physical view")
            .expect("cloned view should remain live");
        assert_eq!(cloned_view.path(), &[41]);
        assert!(!cloned_view.editable());
        assert!(std::rc::Rc::ptr_eq(&cloned_view.lease, &lease));

        let split_pair = jet_jit_shared_guard_split(cloned_guard, 7, 8, 0);
        let (first_guard, second_guard) = Concurrency::with_runtime_mut(|rt| {
            (
                rt.heap
                    .record_get_int(split_pair, 0)
                    .expect("split should contain its first guard"),
                rt.heap
                    .record_get_int(split_pair, 1)
                    .expect("split should contain its second guard"),
            )
        });
        let first_view = crate::runtime_host::native_shared_guard_entry_view(first_guard)
            .expect("first split guard should keep its typed physical view")
            .expect("first split view should remain live");
        let second_view = crate::runtime_host::native_shared_guard_entry_view(second_guard)
            .expect("second split guard should keep its typed physical view")
            .expect("second split view should remain live");
        assert_eq!(first_view.path(), &[41, 7]);
        assert_eq!(second_view.path(), &[41, 8]);
        assert!(!first_view.editable());
        assert!(!second_view.editable());
        assert!(std::rc::Rc::ptr_eq(&first_view.lease, &lease));
        assert!(std::rc::Rc::ptr_eq(&second_view.lease, &lease));

        drop(mapped_view);
        drop(cloned_view);
        drop(first_view);
        drop(second_view);
        drop(lease);
        jet_jit_shared_guard_end(mapped_guard);
        jet_jit_shared_guard_end(first_guard);
        jet_jit_shared_guard_end(second_guard);
        Concurrency::set_active_runtime(None);
        Concurrency::clear_http_shared_runtime();
    }


    #[test]
    fn resident_shared_wait_handoff_publishes_once_and_refreshes_after_reacquire() {
        let mut runtime = fresh_runtime(ReleaseDevtoolsPolicy::default());
        runtime.install_type_descriptors(shared_int_descriptors());
        let initial = runtime.heap.int_from_i64(1);
        let handle = shared_alloc_for_persist(&mut runtime, initial, 0x502)
            .expect("typed Shared allocation should decode its checked payload");
        let shared = shared(&runtime, handle).expect("Shared state should resolve");
        Concurrency::set_active_runtime(Some(&mut runtime as *mut crate::JitRuntime));

        let guard = jet_jit_shared_guard_begin(handle, 1);
        let staged = Concurrency::with_runtime_mut(|rt| rt.heap.int_from_i64(2));
        jet_jit_shared_guard_set_value(guard, staged);
        let state = Concurrency::with_runtime_mut(|rt| guard_state(rt, guard))
            .expect("guard state should remain live");
        let mut handoff = JitSourceGuardWaitHandoff::new(
            guard,
            handle,
            staged,
            Some(Arc::clone(&shared)),
        );
        handoff
            .suspend()
            .expect("dirty resident guard should publish before wait");
        assert_eq!(shared_state_raw(&shared), Some(staged));
        assert_eq!(shared_state_revision(&shared), Some(1));

        let released = state.permit().release();
        assert!(released.result.is_ok());
        assert!(released.completion.is_none());
        let other = shared
            .acquire_state_permit(true)
            .expect("another owner should acquire the released permit");
        let latest = Concurrency::with_runtime_mut(|rt| rt.heap.int_from_i64(3));
        shared_state_commit(&shared, latest).expect("other owner should commit");
        drop(other);

        assert!(handoff
            .resume(false)
            .expect("resident wait should restore the logical permit"));
        assert!(state.permit().reacquire(&mut || false));
        let latest = shared_state_raw(&shared).expect("owner value should remain live");
        Concurrency::with_runtime_mut(|rt| {
            assert!(rt.heap.record_set_int(guard, GUARD_VALUE, latest).is_some());
            assert!(rt.heap.record_set_int(guard, GUARD_DIRTY, 0).is_some());
        });
        drop(state);
        drop(handoff);
        jet_jit_shared_guard_end(guard);
        Concurrency::set_active_runtime(None);
        Concurrency::clear_http_shared_runtime();

        assert_eq!(shared_state_raw(&shared), Some(latest));
        assert_eq!(shared_state_revision(&shared), Some(2));
    }
    #[test]
    fn jit_shared_payload_finalizer_returns_and_delivers_linear_completion() {
        use crate::SourceSharedInterop::SourceSharedInteropPayloadFinalizer;
        use jet_foundation::MIR::MirRuntimeValue;

        let state = Arc::new(SharedState::new(
            1,
            0x901,
            MirRuntimeValue::String("owned".to_string()),
        ));
        let weak_state = Arc::downgrade(&state);
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let finalizer_calls = Arc::clone(&calls);
        let delivered = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let delivery_count = Arc::clone(&delivered);
        let completion_delivery: shared_protocol::JetSharedPhysicalCompletionDelivery =
            Arc::new(move |completion| {
                let units = completion
                    .downcast::<usize>()
                    .expect("finalizer receipt must retain its typed completion");
                delivery_count.fetch_add(*units, Ordering::SeqCst);
            });
        let finalizer: SourceSharedInteropPayloadFinalizer =
            shared_protocol::JetSharedPhysicalFinalizerBinding::new(
                Box::new(move |payload| {
                    assert_eq!(payload, MirRuntimeValue::String("owned".to_string()));
                    let state = weak_state
                        .upgrade()
                        .expect("physical root remains available during finalization");
                    assert_eq!(state.owner_strong_count(), Ok(0));
                    let permit = shared_protocol::jet_shared_acquire(
                        &state.protocol,
                        false,
                        || false,
                    )
                    .expect("finalizer must run after the physical permit is released");
                    drop(permit);
                    finalizer_calls.fetch_add(1, Ordering::SeqCst);
                    Box::new(1usize)
                }),
                Arc::clone(&completion_delivery),
            );
        let mut finalizer = Some(finalizer);
        state
            .install_payload_finalizer(&mut finalizer)
            .expect("live physical root should accept one finalizer");
        assert!(finalizer.is_none(), "successful installation transfers the callback");

        let duplicate_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let duplicate_counter = Arc::clone(&duplicate_calls);
        let mut duplicate: Option<SourceSharedInteropPayloadFinalizer> =
            Some(shared_protocol::JetSharedPhysicalFinalizerBinding::new(
                Box::new(move |_| {
                    duplicate_counter.fetch_add(1, Ordering::SeqCst);
                    Box::new(10usize)
                }),
                Arc::clone(&completion_delivery),
            ));
        assert!(state.install_payload_finalizer(&mut duplicate).is_err());
        assert!(duplicate.is_some(), "duplicate rejection must retain the candidate");
        duplicate
            .take()
            .expect("duplicate candidate remains caller-owned")
            .finish_and_deliver(MirRuntimeValue::Unit);
        assert_eq!(duplicate_calls.load(Ordering::SeqCst), 1);

        let permit = state
            .acquire_state_permit(false)
            .expect("live owner should acquire a physical borrow");
        let owner_release = state.release_owner_alias();
        assert!(owner_release.result.is_ok());
        assert!(owner_release.completion.is_none());
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            state.owner_strong_count(),
            Ok(0),
            "logical death is immediate while active borrows defer payload retirement"
        );
        let release = permit.release();
        assert!(release.result.is_ok());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        completion_delivery(
            release
                .completion
                .expect("explicit last-borrow release returns the finalizer completion"),
        );
        assert_eq!(delivered.load(Ordering::SeqCst), 11);
        drop(permit);
        assert!(state
            .portable_value
            .as_ref()
            .expect("local root retains its payload slot")
            .lock()
            .expect("payload lock remains available")
            .is_none());

        let retired_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let retired_counter = Arc::clone(&retired_calls);
        let mut retired: Option<SourceSharedInteropPayloadFinalizer> =
            Some(shared_protocol::JetSharedPhysicalFinalizerBinding::new(
                Box::new(move |_| {
                    retired_counter.fetch_add(1, Ordering::SeqCst);
                    Box::new(100usize)
                }),
                Arc::clone(&completion_delivery),
            ));
        assert!(state.install_payload_finalizer(&mut retired).is_err());
        assert!(retired.is_some(), "retired rejection must retain the candidate");
        retired
            .take()
            .expect("retired candidate remains caller-owned")
            .finish_and_deliver(MirRuntimeValue::Unit);
        assert_eq!(retired_calls.load(Ordering::SeqCst), 1);
        assert_eq!(delivered.load(Ordering::SeqCst), 111);

        let drop_state = Arc::new(SharedState::new(
            2,
            0x902,
            MirRuntimeValue::String("drop".to_string()),
        ));
        let drop_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let drop_finalizer_calls = Arc::clone(&drop_calls);
        let drop_delivery_count = Arc::clone(&delivered);
        let drop_delivery: shared_protocol::JetSharedPhysicalCompletionDelivery =
            Arc::new(move |completion| {
                let units = completion
                    .downcast::<usize>()
                    .expect("drop finalizer receipt must retain its typed completion");
                drop_delivery_count.fetch_add(*units, Ordering::SeqCst);
            });
        let mut drop_finalizer = Some(
            shared_protocol::JetSharedPhysicalFinalizerBinding::new(
                Box::new(move |payload| {
                    assert_eq!(payload, MirRuntimeValue::String("drop".to_string()));
                    drop_finalizer_calls.fetch_add(1, Ordering::SeqCst);
                    Box::new(7usize)
                }),
                drop_delivery,
            ),
        );
        drop_state
            .install_payload_finalizer(&mut drop_finalizer)
            .expect("live root should accept a drop-delivery finalizer");
        let drop_permit = drop_state
            .acquire_state_permit(false)
            .expect("live drop root should acquire a borrow");
        let owner_release = drop_state.release_owner_alias();
        assert!(owner_release.result.is_ok());
        assert!(owner_release.completion.is_none());
        drop(drop_permit);
        assert_eq!(drop_calls.load(Ordering::SeqCst), 1);
        assert_eq!(delivered.load(Ordering::SeqCst), 118);
    }

}

impl SharedState {
    fn new(
        value: i64,
        type_id: u64,
        portable_value: jet_foundation::MIR::MirRuntimeValue,
    ) -> Self {
        Self {
            protocol: shared_protocol::JetSharedProtocol::new(),
            value: Some(AtomicI64::new(value)),
            portable_value: Some(Mutex::new(Some(portable_value))),
            type_id: Some(type_id),
            revision: Some(AtomicU64::new(0)),
            owner_lifetime: Some(Mutex::new(SharedOwnerLifetime {
                logical_owners: 1,
                active_borrows: 0,
            })),
            payload_finalizer: Mutex::new(None),
            next_owner_alias_token: AtomicI64::new(1),
        }
    }

    fn external(type_id: u64) -> Self {
        Self {
            protocol: shared_protocol::JetSharedProtocol::new(),
            value: None,
            portable_value: None,
            type_id: Some(type_id),
            revision: None,
            owner_lifetime: None,
            payload_finalizer: Mutex::new(None),
            next_owner_alias_token: AtomicI64::new(1),
        }
    }

    pub(crate) fn owner_strong_count(&self) -> Result<usize, String> {
        self.owner_lifetime
            .as_ref()
            .map(|lifetime| {
                lifetime
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .logical_owners
            })
            .ok_or_else(|| "JIT Shared root has no local owner-count hook".to_string())
    }

    fn has_live_logical_owner(&self) -> bool {
        self.owner_lifetime.as_ref().is_none_or(|lifetime| {
            lifetime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .logical_owners
                > 0
        })
    }

    fn has_live_owner_or_borrow(&self) -> bool {
        self.owner_lifetime.as_ref().is_none_or(|lifetime| {
            let lifetime = lifetime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            lifetime.logical_owners > 0 || lifetime.active_borrows > 0
        })
    }

    pub(crate) fn install_payload_finalizer(
        &self,
        finalizer: &mut Option<
            crate::SourceSharedInterop::SourceSharedInteropPayloadFinalizer,
        >,
    ) -> Result<(), String> {
        let lifetime = self
            .owner_lifetime
            .as_ref()
            .ok_or_else(|| "external Shared payload finalizer belongs to its owner".to_string())?;
        let lifetime = lifetime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifetime.logical_owners == 0 {
            return Err("cannot install a finalizer for a retired JIT Shared payload".to_string());
        }
        if finalizer.is_none() {
            return Err("JIT Shared payload finalizer candidate is empty".to_string());
        }
        let mut installed = self
            .payload_finalizer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if installed.is_some() {
            return Err("JIT Shared payload finalizer is already installed".to_string());
        }
        *installed = finalizer.take();
        Ok(())
    }

    pub(crate) fn acquire_state_permit(
        self: &Arc<Self>,
        editable: bool,
    ) -> Option<Arc<SharedStatePermit>> {
        if !self.begin_borrow() {
            return None;
        }
        let Some(protocol) =
            shared_protocol::jet_shared_acquire(&self.protocol, editable, || false)
        else {
            self.end_borrow_during_drop();
            return None;
        };
        Some(Arc::new(SharedStatePermit {
            state: Arc::clone(self),
            protocol,
            active: AtomicBool::new(true),
        }))
    }

    fn begin_borrow(&self) -> bool {
        let Some(lifetime) = &self.owner_lifetime else {
            return false;
        };
        let mut lifetime = lifetime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifetime.logical_owners == 0 {
            return false;
        }
        lifetime.active_borrows = lifetime
            .active_borrows
            .checked_add(1)
            .expect("JIT Shared active borrow count exhausted");
        true
    }

    fn end_borrow(&self) -> shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        Self::finish_retired_payload(self.take_ended_borrow_payload())
    }

    fn end_borrow_during_drop(&self) {
        Self::finish_retired_payload_during_drop(self.take_ended_borrow_payload());
    }

    fn take_ended_borrow_payload(
        &self,
    ) -> Option<(
        jet_foundation::MIR::MirRuntimeValue,
        Option<crate::SourceSharedInterop::SourceSharedInteropPayloadFinalizer>,
    )> {
        let lifetime = self.owner_lifetime.as_ref()?;
        let mut lifetime = lifetime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifetime.active_borrows == 0 {
            return None;
        }
        lifetime.active_borrows -= 1;
        self.take_retired_payload(&lifetime)
    }

    fn take_retired_payload(
        &self,
        lifetime: &SharedOwnerLifetime,
    ) -> Option<(
        jet_foundation::MIR::MirRuntimeValue,
        Option<crate::SourceSharedInterop::SourceSharedInteropPayloadFinalizer>,
    )> {
        if lifetime.logical_owners != 0 || lifetime.active_borrows != 0 {
            return None;
        }
        let value = self
            .portable_value
            .as_ref()?
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()?;
        let finalizer = self
            .payload_finalizer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        Some((value, finalizer))
    }

    fn finish_retired_payload(
        retired: Option<(
            jet_foundation::MIR::MirRuntimeValue,
            Option<crate::SourceSharedInterop::SourceSharedInteropPayloadFinalizer>,
        )>,
    ) -> shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        match retired {
            Some((value, Some(finalizer))) => {
                shared_protocol::JetSharedPhysicalOperationOutcome::new(
                    Ok(()),
                    Some(finalizer.finish(value)),
                )
            }
            Some((value, None)) => {
                drop(value);
                shared_protocol::JetSharedPhysicalOperationOutcome::new(Ok(()), None)
            }
            None => shared_protocol::JetSharedPhysicalOperationOutcome::new(Ok(()), None),
        }
    }

    fn finish_retired_payload_during_drop(
        retired: Option<(
            jet_foundation::MIR::MirRuntimeValue,
            Option<crate::SourceSharedInterop::SourceSharedInteropPayloadFinalizer>,
        )>,
    ) {
        match retired {
            Some((value, Some(finalizer))) => finalizer.finish_and_deliver(value),
            Some((value, None)) => drop(value),
            None => {}
        }
    }

    pub(crate) fn retain_owner_alias(
        self: &Arc<Self>,
    ) -> Result<Box<dyn crate::SourceSharedInterop::SourceSharedInteropOwnerAliasLease>, String>
    {
        self.try_retain_owner_alias()?
            .ok_or_else(|| "JIT Shared owner has no live strong aliases".to_string())
    }

    pub(crate) fn try_retain_owner_alias(
        self: &Arc<Self>,
    ) -> Result<
        Option<Box<dyn crate::SourceSharedInterop::SourceSharedInteropOwnerAliasLease>>,
        String,
    > {
        let lifetime = self
            .owner_lifetime
            .as_ref()
            .ok_or_else(|| "external Shared alias must be retained through its owner".to_string())?;
        let token_id = self
            .next_owner_alias_token
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |next| {
                (next > 0).then(|| next.checked_add(1)).flatten()
            })
            .map_err(|_| "JIT Shared owner alias tokens exhausted".to_string())?;
        let mut lifetime = lifetime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifetime.logical_owners == 0 {
            return Ok(None);
        }
        lifetime.logical_owners = lifetime
            .logical_owners
            .checked_add(1)
            .ok_or_else(|| "JIT Shared owner alias count exhausted".to_string())?;
        drop(lifetime);
        Ok(Some(Box::new(ResidentSharedOwnerAliasLease {
            state: Arc::clone(self),
            token_id,
            active: true,
        })))
    }

    pub(crate) fn release_owner_alias(
        &self,
    ) -> shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        Self::finish_retired_payload(self.take_released_owner_payload())
    }

    pub(crate) fn release_owner_alias_during_drop(&self) {
        Self::finish_retired_payload_during_drop(self.take_released_owner_payload());
    }

    fn take_released_owner_payload(
        &self,
    ) -> Option<(
        jet_foundation::MIR::MirRuntimeValue,
        Option<crate::SourceSharedInterop::SourceSharedInteropPayloadFinalizer>,
    )> {
        let lifetime = self.owner_lifetime.as_ref()?;
        let mut lifetime = lifetime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if lifetime.logical_owners == 0 {
            return None;
        }
        lifetime.logical_owners -= 1;
        self.take_retired_payload(&lifetime)
    }
}

impl SharedStatePermit {
    pub(crate) fn held(&self) -> bool {
        self.active.load(Ordering::Acquire) && self.protocol.held()
    }

    pub(crate) fn release(
        &self,
    ) -> shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        if self.active.swap(false, Ordering::AcqRel) {
            self.protocol.release();
            self.state.end_borrow()
        } else {
            shared_protocol::JetSharedPhysicalOperationOutcome::new(Ok(()), None)
        }
    }

    pub(crate) fn release_during_drop(&self) {
        if self.active.swap(false, Ordering::AcqRel) {
            shared_protocol::JetSharedCanonicalPermit::release_during_drop(
                self.protocol.as_ref(),
            );
            self.state.end_borrow_during_drop();
        }
    }

    pub(crate) fn reacquire(&self, cancelled: &mut dyn FnMut() -> bool) -> bool {
        if self.active.load(Ordering::Acquire) {
            return self.protocol.held();
        }
        if !self.state.begin_borrow() {
            return false;
        }
        if self.protocol.reacquire(|| cancelled()) {
            self.active.store(true, Ordering::Release);
            true
        } else {
            self.state.end_borrow_during_drop();
            false
        }
    }
}

impl Drop for SharedStatePermit {
    fn drop(&mut self) {
        self.release_during_drop();
    }
}

impl shared_protocol::JetSharedCanonicalPermit for SharedStatePermit {
    fn editable(&self) -> bool {
        self.protocol.editable()
    }

    fn held(&self) -> bool {
        SharedStatePermit::held(self)
    }

    fn release(&self) -> shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        SharedStatePermit::release(self)
    }

    fn release_during_drop(&self) {
        SharedStatePermit::release_during_drop(self);
    }

    fn reacquire(&self, cancelled: &mut dyn FnMut() -> bool) -> bool {
        SharedStatePermit::reacquire(self, cancelled)
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn std::any::Any> {
        self
    }
}

impl shared_protocol::JetSharedCanonicalOwner for SharedState {
    fn owner_identity(&self) -> usize {
        self as *const Self as usize
    }

    fn protocol_order_key(&self) -> usize {
        Arc::as_ptr(&self.protocol) as usize
    }

    fn acquire_permit(
        self: Arc<Self>,
        editable: bool,
    ) -> Result<Arc<dyn shared_protocol::JetSharedCanonicalPermit>, String> {
        self.acquire_state_permit(editable)
            .map(|permit| permit as Arc<dyn shared_protocol::JetSharedCanonicalPermit>)
            .ok_or_else(|| "JIT Shared owner has no live logical aliases".to_string())
    }
}

struct ResidentSharedOwnerAliasLease {
    state: Arc<SharedState>,
    token_id: i64,
    active: bool,
}

impl crate::SourceSharedInterop::SourceSharedInteropOwnerAliasLease
    for ResidentSharedOwnerAliasLease
{
    fn token_id(&self) -> i64 {
        self.token_id
    }

    fn release(
        mut self: Box<Self>,
    ) -> shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        self.active = false;
        self.state.release_owner_alias()
    }
}

impl Drop for ResidentSharedOwnerAliasLease {
    fn drop(&mut self) {
        if self.active {
            self.state.release_owner_alias_during_drop();
        }
    }
}

fn shared_next_revision(shared: &SharedState) -> Result<u64, ()> {
    shared
        .revision
        .as_ref()
        .ok_or(())?
        .load(Ordering::Acquire)
        .checked_add(1)
        .ok_or(())
}
struct SourceSharedCanonicalOwner {
    interop: crate::SourceSharedInterop::SourceSharedInterop,
    identity: usize,
    protocol_order_key: usize,
}

struct SourceSharedCanonicalPermit {
    owner: Arc<SourceSharedCanonicalOwner>,
    guard: std::rc::Rc<crate::SourceSharedInterop::SourceSharedInteropPhysicalLease>,
    editable: bool,
}

impl shared_protocol::JetSharedCanonicalOwner for SourceSharedCanonicalOwner {
    fn owner_identity(&self) -> usize {
        self.identity
    }

    fn protocol_order_key(&self) -> usize {
        self.protocol_order_key
    }

    fn acquire_permit(
        self: Arc<Self>,
        editable: bool,
    ) -> Result<Arc<dyn shared_protocol::JetSharedCanonicalPermit>, String> {
        let guard = self.interop.acquire_physical_lease(editable)?;
        Ok(Arc::new(SourceSharedCanonicalPermit {
            owner: self,
            guard,
            editable,
        }))
    }
}

impl shared_protocol::JetSharedCanonicalPermit for SourceSharedCanonicalPermit {
    fn editable(&self) -> bool {
        self.editable
    }

    fn held(&self) -> bool {
        self.guard.held()
    }

    fn release(&self) -> shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        if std::rc::Rc::strong_count(&self.guard) == 1 {
            self.guard.release()
        } else {
            shared_protocol::JetSharedPhysicalOperationOutcome::new(Ok(()), None)
        }
    }

    fn release_during_drop(&self) {
        if std::rc::Rc::strong_count(&self.guard) == 1 {
            self.guard.release_during_drop();
        }
    }

    fn reacquire(&self, _cancelled: &mut dyn FnMut() -> bool) -> bool {
        self.held()
    }

    fn stage_value(&self, value: Box<dyn std::any::Any>) -> Result<(), String> {
        let value = match value.downcast::<jet_foundation::MIR::MirRuntimeValue>() {
            Ok(value) => *value,
            Err(value) => match value.downcast::<i64>() {
                Ok(raw) => crate::runtime_host::native_shared_value_from_raw(
                    &self.owner.interop,
                    *raw,
                )?,
                Err(_) => {
                    return Err("Source Shared transaction staged an unsupported payload type"
                        .to_string());
                }
            },
        };
        self.guard.stage_value(value)
    }

    fn discard_staged(&self) -> Result<(), String> {
        self.guard.discard_staged()
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn std::any::Any> {
        self
    }
}

fn source_shared_canonical_owner_record(
    interop: crate::SourceSharedInterop::SourceSharedInterop,
) -> Result<Arc<SourceSharedCanonicalOwner>, String> {
    let identity = interop
        .physical_identity()
        .ok_or_else(|| "Source Shared root has no physical owner identity".to_string())?;
    let protocol_order_key = interop
        .protocol_order_key()
        .ok_or_else(|| "Source Shared root has no physical protocol-order key".to_string())?;
    Ok(Arc::new(SourceSharedCanonicalOwner {
        interop,
        identity,
        protocol_order_key,
    }))
}

pub(crate) fn source_shared_canonical_owner(
    interop: crate::SourceSharedInterop::SourceSharedInterop,
) -> Result<Arc<dyn shared_protocol::JetSharedCanonicalOwner>, String> {
    Ok(source_shared_canonical_owner_record(interop)?)
}

pub(crate) fn source_shared_canonical_permit(
    interop: crate::SourceSharedInterop::SourceSharedInterop,
    editable: bool,
) -> Result<Arc<dyn shared_protocol::JetSharedCanonicalPermit>, String> {
    shared_protocol::JetSharedCanonicalOwner::acquire_permit(
        source_shared_canonical_owner_record(interop)?,
        editable,
    )
}

pub(crate) fn source_shared_canonical_permit_with_entry_view(
    interop: crate::SourceSharedInterop::SourceSharedInterop,
    editable: bool,
) -> Result<
    (
        Arc<dyn shared_protocol::JetSharedCanonicalPermit>,
        crate::SourceSharedInterop::SourceSharedInteropGuardState,
    ),
    String,
> {
    let owner = source_shared_canonical_owner_record(interop)?;
    let guard = owner.interop.acquire_physical_lease(editable)?;
    let concrete = Arc::new(SourceSharedCanonicalPermit {
        owner: Arc::clone(&owner),
        guard: guard.clone(),
        editable,
    });
    let view_permit: Arc<dyn shared_protocol::JetSharedCanonicalPermit> = concrete.clone();
    let state = shared_protocol::jet_shared_guard_state_from_permit(view_permit, editable)
        .map_err(str::to_owned)?;
    let view = crate::SourceSharedInterop::SourceSharedInteropGuardState {
        state,
        lease: guard,
        physical_identity: owner.identity,
        protocol_order_key: owner.protocol_order_key,
    };
    let permit: Arc<dyn shared_protocol::JetSharedCanonicalPermit> = concrete;
    Ok((permit, view))
}

/// Borrow the concrete Source permit behind a canonical permit. The permit
/// holds a thread-affine lease, so it is inspected in place, never re-owned.
fn source_shared_concrete_permit(
    permit: &Arc<dyn shared_protocol::JetSharedCanonicalPermit>,
) -> Result<&SourceSharedCanonicalPermit, String> {
    let any: &dyn std::any::Any = permit.as_ref();
    any.downcast_ref::<SourceSharedCanonicalPermit>()
        .ok_or_else(|| "Source Shared canonical permit type changed".to_string())
}

pub(crate) fn source_shared_canonical_read(
    permit: &Arc<dyn shared_protocol::JetSharedCanonicalPermit>,
) -> Result<(jet_foundation::MIR::MirRuntimeValue, u64), String> {
    let permit = source_shared_concrete_permit(permit)?;
    Ok((permit.guard.read_value()?, permit.guard.revision()?))
}


pub(crate) fn source_shared_canonical_wait_suspend(
    permit: &Arc<dyn shared_protocol::JetSharedCanonicalPermit>,
    value: jet_foundation::MIR::MirRuntimeValue,
) -> Result<(), String> {
    let permit = source_shared_concrete_permit(permit)?;
    permit.guard.wait_suspend(value)
}

pub(crate) fn source_shared_canonical_wait_resume(
    permit: &Arc<dyn shared_protocol::JetSharedCanonicalPermit>,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Option<jet_foundation::MIR::MirRuntimeValue>, String> {
    let permit = source_shared_concrete_permit(permit)?;
    permit.guard.wait_resume(cancelled)
}

pub(crate) fn source_shared_canonical_wait_abort(
    permit: &Arc<dyn shared_protocol::JetSharedCanonicalPermit>,
) -> Result<(), String> {
    let permit = source_shared_concrete_permit(permit)?;
    permit.guard.wait_abort()
}

pub(crate) fn source_shared_canonical_finish(
    permit: &Arc<dyn shared_protocol::JetSharedCanonicalPermit>,
    raw: i64,
) -> Result<(), String> {
    let permit = source_shared_concrete_permit(permit)?;
    let value =
        crate::runtime_host::native_shared_value_from_raw(&permit.owner.interop, raw)?;
    permit.guard.finish_value(value)
}

fn shared_capture_parts(shared: &Arc<SharedState>) -> Option<(u64, i64)> {
    let permit = shared.acquire_state_permit(false)?;
    let revision = shared.revision.as_ref()?.load(Ordering::Acquire);
    let value = shared.value.as_ref()?.load(Ordering::Acquire);
    drop(permit);
    Some((revision, value))
}

fn shared_snapshot_store(
    rt: &mut crate::JitRuntime,
    owner: Arc<SharedState>,
    type_id: u64,
    revision: u64,
    value: i64,
    physical_owner: Option<crate::SourceSharedInterop::SourceSharedInterop>,
) -> Result<i64, String> {
    let root_identity = physical_owner
        .as_ref()
        .map(crate::SourceSharedInterop::SourceSharedInterop::identity)
        .unwrap_or_else(|| Arc::as_ptr(&owner) as usize);
    let owner_alias = if let Some(interop) = physical_owner.as_ref() {
        interop.retain_owner_alias()?
    } else {
        crate::SourceSharedInterop::SourceSharedInteropOwnerAlias::new(
            root_identity,
            owner.retain_owner_alias()?,
        )
    };
    let snapshot = Arc::new(SharedSnapshot {
        owner,
        _physical_owner: physical_owner.map(|interop| interop.without_owner_alias_lease()),
        _owner_alias: owner_alias,
        type_id,
        root_identity,
        revision,
        value,
        valid: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        consumed: Arc::new(std::sync::atomic::AtomicBool::new(false)),
    });
    let ticket = next_shared_ticket(&NEXT_SHARED_SNAPSHOT_TICKET)?;
    rt.shared_snapshots.insert(ticket, snapshot);
    Ok(ticket)
}

fn shared_snapshot_load(
    rt: &crate::JitRuntime,
    handle: i64,
    type_id: u64,
) -> Result<Arc<SharedSnapshot>, String> {
    let snapshot = rt
        .shared_snapshots
        .get(&handle)
        .cloned()
        .ok_or_else(|| "Shared snapshot ticket is invalid or released".to_string())?;
    if snapshot.type_id != type_id {
        return Err("Shared snapshot ticket type disagrees with its checked descriptor".to_string());
    }
    Ok(snapshot)
}
fn shared_snapshot_store_for_handle(
    handle: i64,
    owner: Arc<SharedState>,
    type_id: u64,
    revision: u64,
    value: i64,
) -> Result<i64, String> {
    let interop = crate::runtime_host::native_shared_interop_for_type(handle, type_id)?;
    Concurrency::with_runtime_string(|rt| {
        shared_snapshot_store(rt, owner, type_id, revision, value, Some(interop))
    })
}

fn shared_snapshot_clone_ticket(handle: i64, type_id: u64) -> Result<i64, String> {
    Concurrency::with_runtime_string(|rt| shared_snapshot_clone_in_runtime(rt, handle, type_id))
}

pub(crate) fn shared_snapshot_clone_in_runtime(
    rt: &mut crate::JitRuntime,
    handle: i64,
    type_id: u64,
) -> Result<i64, String> {
    let snapshot = shared_snapshot_load(rt, handle, type_id)?;
    let physical_owner = snapshot
        ._physical_owner
        .as_ref()
        .ok_or_else(|| "Shared snapshot has no physical owner metadata".to_string())?;
    let owner_alias = physical_owner.retain_owner_alias()?;
    let cloned = Arc::new(SharedSnapshot {
        owner: Arc::clone(&snapshot.owner),
        _physical_owner: Some(physical_owner.without_owner_alias_lease()),
        _owner_alias: owner_alias,
        type_id,
        root_identity: snapshot.root_identity,
        revision: snapshot.revision,
        value: snapshot.value,
        valid: Arc::clone(&snapshot.valid),
        consumed: Arc::clone(&snapshot.consumed),
    });
    let ticket = next_shared_ticket(&NEXT_SHARED_SNAPSHOT_TICKET)?;
    rt.shared_snapshots.insert(ticket, cloned);
    Ok(ticket)
}

fn shared_snapshot_release_ticket(handle: i64, type_id: u64) -> Result<(), String> {
    Concurrency::with_runtime_string(|rt| shared_snapshot_release_in_runtime(rt, handle, type_id))
}

pub(crate) fn shared_snapshot_release_in_runtime(
    rt: &mut crate::JitRuntime,
    handle: i64,
    type_id: u64,
) -> Result<(), String> {
    shared_snapshot_load(rt, handle, type_id)?;
    drop(rt.shared_snapshots.remove(&handle));
    Ok(())
}

fn shared_weak_clone_ticket(handle: i64, type_id: u64) -> Result<i64, String> {
    Concurrency::with_runtime_string(|rt| shared_weak_clone_in_runtime(rt, handle, type_id))
}

pub(crate) fn shared_weak_clone_in_runtime(
    rt: &mut crate::JitRuntime,
    handle: i64,
    type_id: u64,
) -> Result<i64, String> {
    let owner = shared_weak_owner_load(rt, handle, type_id)?;
    shared_weak_owner_store(rt, type_id, owner)
}

fn shared_weak_release_ticket(handle: i64, type_id: u64) -> Result<(), String> {
    Concurrency::with_runtime_string(|rt| shared_weak_release_in_runtime(rt, handle, type_id))
}

pub(crate) fn shared_weak_release_in_runtime(
    rt: &mut crate::JitRuntime,
    handle: i64,
    type_id: u64,
) -> Result<(), String> {
    shared_weak_owner_load(rt, handle, type_id)?;
    rt.shared_weak_owners.remove(&handle);
    Ok(())
}


fn shared_weak_owner_store(
    rt: &mut crate::JitRuntime,
    type_id: u64,
    owner: crate::SourceSharedInterop::SourceSharedInteropWeak,
) -> Result<i64, String> {
    if owner.type_id() != type_id {
        return Err("Shared weak owner type disagrees with its checked descriptor".to_string());
    }
    let ticket = next_shared_ticket(&NEXT_SHARED_WEAK_TICKET)?;
    rt.shared_weak_owners
        .insert(ticket, SharedWeakOwner { type_id, owner });
    Ok(ticket)
}

fn shared_weak_owner_load(
    rt: &crate::JitRuntime,
    ticket: i64,
    type_id: u64,
) -> Result<crate::SourceSharedInterop::SourceSharedInteropWeak, String> {
    let weak = rt
        .shared_weak_owners
        .get(&ticket)
        .ok_or_else(|| "Shared weak ticket is invalid or released".to_string())?;
    if weak.type_id != type_id || weak.owner.type_id() != type_id {
        return Err("Shared weak ticket type disagrees with its checked descriptor".to_string());
    }
    Ok(weak.owner.clone())
}

fn shared_revision_error_result(rt: &mut crate::JitRuntime, discriminant: i64) -> i64 {
    let error = rt.heap.alloc_record(1);
    let _ = rt.heap.record_set_int(error, 0, discriminant);
    crate::runtime_host::alloc_jit_result(rt, false, error as u64)
}

fn shared_transaction_active() -> bool {
    SHARED_TRANSACTIONS.with(|transactions| !transactions.borrow().is_empty())
}

fn shared_transaction_owner(
    shared: &Arc<SharedState>,
    handle: i64,
) -> Result<Arc<dyn shared_protocol::JetSharedCanonicalOwner>, String> {
    if let Some(interop) = crate::runtime_host::native_shared_interop(handle) {
        if !interop.is_resident_root() || shared.portable_value.is_none() {
            return source_shared_canonical_owner(interop);
        }
    }
    Ok(Arc::clone(shared) as Arc<dyn shared_protocol::JetSharedCanonicalOwner>)
}

fn shared_transaction_capture_raw(
    shared: &Arc<SharedState>,
    handle: i64,
) -> Result<(i64, u64), String> {
    match crate::runtime_host::native_shared_capture(handle) {
        Some(result) => result,
        None => {
            let revision = shared_state_revision(shared)
                .ok_or_else(|| "Shared owner has no physical revision".to_string())?;
            let value = shared_state_raw(shared)
                .ok_or_else(|| "Shared owner has no physical value".to_string())?;
            Ok((value, revision))
        }
    }
}

fn shared_transaction_touch(shared: &Arc<SharedState>, handle: i64) -> Result<bool, String> {
    if !shared_transaction_active() {
        return Ok(false);
    }
    let owner = shared_transaction_owner(shared, handle)?;
    Ok(SHARED_TRANSACTIONS.with(|transactions| {
        let mut transactions = transactions.borrow_mut();
        let Some(transaction) = transactions.last_mut() else {
            return false;
        };
        transaction.transaction.touch(owner);
        true
    }))
}

fn shared_transaction_staged(
    shared: &Arc<SharedState>,
    handle: i64,
) -> Result<Option<std::rc::Rc<std::cell::RefCell<i64>>>, String> {
    if !shared_transaction_active() {
        return Ok(None);
    }
    let owner = shared_transaction_owner(shared, handle)?;
    Ok(SHARED_TRANSACTIONS.with(|transactions| {
        transactions
            .borrow()
            .last()
            .and_then(|transaction| transaction.transaction.staged_value::<i64>(&owner))
    }))
}

fn shared_transaction_snapshot_revision(
    shared: &Arc<SharedState>,
    handle: i64,
) -> Result<Option<u64>, String> {
    if !shared_transaction_active() {
        return Ok(None);
    }
    let owner = shared_transaction_owner(shared, handle)?;
    let (_, revision) = shared_transaction_capture_raw(shared, handle)?;
    Ok(SHARED_TRANSACTIONS.with(|transactions| {
        transactions
            .borrow()
            .last()
            .and_then(|transaction| transaction.transaction.snapshot_revision(&owner, revision))
    }))
}

fn shared_transaction_stage_for_write(
    shared: &Arc<SharedState>,
    handle: i64,
) -> Result<Option<std::rc::Rc<std::cell::RefCell<i64>>>, String> {
    if !shared_transaction_active() {
        return Ok(None);
    }
    let owner = shared_transaction_owner(shared, handle)?;
    let (initial, _) = shared_transaction_capture_raw(shared, handle)?;
    let source_owned = crate::runtime_host::native_shared_interop(handle).is_some_and(|interop| {
        !interop.is_resident_root() || shared.portable_value.is_none()
    });
    Ok(SHARED_TRANSACTIONS.with(|transactions| {
        let mut transactions = transactions.borrow_mut();
        let Some(transaction) = transactions.last_mut() else {
            return None;
        };
        let staged = transaction
            .transaction
            .stage_value(owner.clone(), || initial);
        transaction.transaction.mark_write(owner.clone());
        let commit_shared = Arc::clone(shared);
        let stage_staged = staged.clone();
        let commit_staged = staged.clone();
        transaction.transaction.record_edit_with_staged_commit(
            owner,
            Box::new(|| {}),
            Box::new(move |permit| {
                if source_owned {
                    permit.stage_value(Box::new(*stage_staged.borrow()))
                } else {
                    Ok(())
                }
            }),
            Box::new(move |_| {
                if source_owned {
                    return;
                }
                let value = *commit_staged.borrow();
                if let Err(error) = shared_state_commit(&commit_shared, value) {
                    shared_commit_fault(&error);
                }
            }),
        );
        Some(staged)
    }))
}
pub(crate) fn shared_state_has_live_portable_value(state: &SharedState) -> bool {
    state.has_live_logical_owner()
        && state
            .portable_value
            .as_ref()
            .and_then(|value| value.lock().ok())
            .is_some_and(|value| value.is_some())
}


fn pool(rt: &crate::JitRuntime, handle: i64) -> Option<Arc<Mutex<PoolState>>> {
    rt.pools.get((handle as usize).wrapping_sub(1)).cloned()
}

fn shared(rt: &crate::JitRuntime, handle: i64) -> Option<Arc<SharedState>> {
    rt.shareds
        .get((handle as usize).wrapping_sub(1))?
        .as_ref()
        .cloned()
}
pub(crate) fn shared_take_slot(
    rt: &mut crate::JitRuntime,
    handle: i64,
) -> Result<Arc<SharedState>, String> {
    let index = usize::try_from(handle)
        .ok()
        .and_then(|handle| handle.checked_sub(1))
        .ok_or_else(|| "JIT Shared alias handle is invalid".to_string())?;
    rt.shareds
        .get_mut(index)
        .and_then(Option::take)
        .ok_or_else(|| "JIT Shared alias handle is no longer live".to_string())
}


pub(crate) fn shared_state(
    rt: &crate::JitRuntime,
    handle: i64,
) -> Option<Arc<SharedState>> {
    shared(rt, handle)
}

pub(crate) fn shared_state_portable_value(
    state: &SharedState,
) -> Result<jet_foundation::MIR::MirRuntimeValue, String> {
    if !state.has_live_owner_or_borrow() {
        return Err("JIT Shared root has no live owner aliases or physical borrows".to_string());
    }
    state
        .portable_value
        .as_ref()
        .ok_or_else(|| "external Shared root has no local portable payload".to_string())?
        .lock()
        .map_err(|_| "JIT Shared portable payload lock was poisoned".to_string())?
        .as_ref()
        .cloned()
        .ok_or_else(|| "JIT Shared portable payload was released".to_string())
}

pub(crate) fn shared_state_type_id(state: &SharedState) -> Option<u64> {
    state.type_id
}

fn shared_state_publish(
    state: &SharedState,
    portable_value: jet_foundation::MIR::MirRuntimeValue,
    projection: Option<i64>,
) -> Result<(), String> {
    if !state.has_live_owner_or_borrow() {
        return Err("JIT Shared root has no live owner aliases or physical borrows".to_string());
    }
    let value = state
        .portable_value
        .as_ref()
        .ok_or_else(|| "external Shared payload must be updated through its owner".to_string())?;
    let next = shared_next_revision(state)
        .map_err(|_| "SharedRevisionError.GenerationExhausted".to_string())?;
    let mut value = value
        .lock()
        .map_err(|_| "JIT Shared portable payload lock was poisoned".to_string())?;
    let current = value
        .as_mut()
        .ok_or_else(|| "JIT Shared portable payload was released".to_string())?;
    *current = portable_value;
    if let (Some(raw), Some(slot)) = (projection, state.value.as_ref()) {
        slot.store(raw, Ordering::Release);
    }
    shared_store_revision(state, next)
        .then_some(())
        .ok_or_else(|| "external Shared revision belongs to its owner".to_string())
}

pub(crate) fn shared_state_commit_portable(
    state: &SharedState,
    portable_value: jet_foundation::MIR::MirRuntimeValue,
) -> Result<(), String> {
    shared_state_publish(state, portable_value, None)
}

pub(crate) fn shared_state_raw(state: &SharedState) -> Option<i64> {
    state
        .value
        .as_ref()
        .map(|value| value.load(Ordering::Acquire))
}
pub(crate) fn shared_state_revision(state: &SharedState) -> Option<u64> {
    state
        .revision
        .as_ref()
        .map(|revision| revision.load(Ordering::Acquire))
}

fn shared_store_revision(state: &SharedState, revision: u64) -> bool {
    if let Some(slot) = state.revision.as_ref() {
        slot.store(revision, Ordering::Release);
        true
    } else {
        false
    }
}

pub(crate) fn shared_state_commit(
    state: &SharedState,
    value: i64,
) -> Result<(), String> {
    let type_id = state
        .type_id
        .ok_or_else(|| "external Shared payload must be updated through its owner".to_string())?;
    let portable_value = Concurrency::with_runtime_string(|rt| {
        crate::runtime_host::decode_jit_shared_payload_raw(rt, value, type_id)
    })?;
    shared_state_publish(state, portable_value, Some(value))
}


/// Read the current value behind a checked `Shared<T>` handle for a host
/// marshaller. External owner shells intentionally have no local payload.
pub(crate) fn shared_value(rt: &crate::JitRuntime, handle: i64) -> Option<i64> {
    shared(rt, handle).and_then(|state| shared_state_raw(&state))
}

fn condition(rt: &crate::JitRuntime, handle: i64) -> Option<Arc<ConditionState>> {
    rt.conditions
        .get((handle as usize).wrapping_sub(1))
        .cloned()
}

const GUARD_SHARED: i64 = 0;
const GUARD_VALUE: i64 = 1;
const GUARD_DIRTY: i64 = 2;
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
    let guard = rt.heap.alloc_record(3);
    let _ = rt.heap.record_set_int(guard, GUARD_SHARED, shared_handle);
    let _ = rt.heap.record_set_int(guard, GUARD_VALUE, value);
    let _ = rt.heap.record_set_int(guard, GUARD_DIRTY, 0);
    rt.shared_guard_states.insert(guard, state);
    guard
}
fn mark_shared_guard_dirty(rt: &mut crate::JitRuntime, guard: i64) {
    if rt.heap.record_set_int(guard, GUARD_DIRTY, 1).is_none() {
        rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
    }
}

fn take_active_shared_permit(handle: i64) -> Option<Arc<SharedStatePermit>> {
    SHARED_ACTIVE_PERMITS.with(|permits| {
        let mut permits = permits.borrow_mut();
        permits
            .iter()
            .rposition(|(active_handle, _)| *active_handle == handle)
            .map(|index| permits.swap_remove(index).1)
    })
}
fn take_active_native_shared_guard(handle: i64) -> Option<i64> {
    SHARED_ACTIVE_NATIVE_GUARDS.with(|guards| {
        let mut guards = guards.borrow_mut();
        guards
            .iter()
            .rposition(|(active_handle, _)| *active_handle == handle)
            .map(|index| guards.swap_remove(index).1)
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

fn jet_jit_shared_new(value: i64, type_id: i64) -> i64 {
    Concurrency::with_runtime_mut(|rt| {
        // Checked type IDs are u64 stable hashes carried bit-for-bit in an
        // i64 word; a set high bit is a valid identity, not a negative value.
        match shared_alloc_for_persist(rt, value, type_id as u64) {
            Ok(handle) => handle,
            Err(error) => {
                rt.set_host_fault(&error);
                0
            }
        }
    })
}

/// Allocate a Shared root from one checked payload descriptor. The raw word is
/// decoded immediately into a portable MIR value; it is only a runtime-local
/// projection and is never retained as the physical payload.
pub(crate) fn shared_alloc_for_persist(
    rt: &mut crate::JitRuntime,
    value: i64,
    type_id: u64,
) -> Result<i64, String> {
    let portable_value = crate::runtime_host::decode_jit_shared_payload_raw(rt, value, type_id)?;
    let handle = i64::try_from(
        rt.shareds
            .len()
            .checked_add(1)
            .ok_or_else(|| "JIT Shared handle table exhausted".to_string())?,
    )
    .map_err(|_| "JIT Shared handle table exhausted".to_string())?;
    rt.shareds.push(Some(Arc::new(SharedState::new(
        value,
        type_id,
        portable_value,
    ))));
    Ok(handle)
}

pub(crate) fn shared_alloc_alias_state(
    rt: &mut crate::JitRuntime,
    state: Arc<SharedState>,
) -> Result<i64, String> {
    let handle = i64::try_from(
        rt.shareds
            .len()
            .checked_add(1)
            .ok_or_else(|| "JIT Shared handle table exhausted".to_string())?,
    )
    .map_err(|_| "JIT Shared handle table exhausted".to_string())?;
    rt.shareds.push(Some(state));
    Ok(handle)
}

pub(crate) fn shared_alloc_external_for_persist(
    rt: &mut crate::JitRuntime,
    type_id: u64,
) -> Result<i64, String> {
    shared_alloc_alias_state(rt, Arc::new(SharedState::external(type_id)))
}


pub(crate) fn shared_cache_value(handle: i64, value: i64) {
    let _ = Concurrency::with_runtime_mut(|rt| {
        if let Some(shared) = shared(rt, handle) {
            if let Some(slot) = shared.value.as_ref() {
                slot.store(value, Ordering::Release);
            }
        }
    });
}
pub(crate) fn shared_cache_capture(handle: i64, value: i64, revision: u64) {
    let _ = Concurrency::with_runtime_mut(|rt| {
        if let Some(shared) = shared(rt, handle) {
            if let Some(slot) = shared.value.as_ref() {
                slot.store(value, Ordering::Release);
            }
            let _ = shared_store_revision(&shared, revision);
        }
    });
}
pub(crate) fn shared_cache_revision(handle: i64, revision: u64) {
    let _ = Concurrency::with_runtime_mut(|rt| {
        if let Some(shared) = shared(rt, handle) {
            let _ = shared_store_revision(&shared, revision);
        }
    });
}



/// Scalar Shared compatibility operations stay on the canonical atomic rail.
/// They must not enter the blocking guard protocol: callback-safe scalar reads
/// and writes are direct Acquire/Release operations, while guard/aggregate
/// paths below retain the protocol permits.
fn jet_jit_shared_get(handle: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return 0;
    };
    match shared_transaction_staged(&shared, handle) {
        Ok(Some(staged)) => return *staged.borrow(),
        Ok(None) => {}
        Err(error) => return shared_callback_fault(&error),
    }
    if let Some(result) = crate::runtime_host::native_shared_get_value(handle) {
        return result.unwrap_or_else(|error| shared_callback_fault(&error));
    }
    shared_state_raw(&shared)
        .unwrap_or_else(|| shared_callback_fault("external Shared read is missing its physical owner"))
}

fn jet_jit_shared_set(handle: i64, value: i64) {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return;
    };
    match shared_transaction_stage_for_write(&shared, handle) {
        Ok(Some(staged)) => {
            *staged.borrow_mut() = value;
            return;
        }
        Ok(None) => {}
        Err(error) => {
            shared_callback_fault(&error);
            return;
        }
    }
    if let Some(result) = crate::runtime_host::native_shared_set_value(handle, value) {
        if let Err(error) = result {
            shared_callback_fault(&error);
        }
        return;
    }
    if shared.value.is_none() {
        shared_callback_fault("external Shared write is missing its physical owner");
        return;
    }
    let Some(permit) = shared.acquire_state_permit(true) else {
        return;
    };
    if let Err(error) = shared_state_commit(&shared, value) {
        shared_commit_fault(&error);
    }
}

fn jet_jit_shared_replace(handle: i64, value: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return 0;
    };
    match shared_transaction_stage_for_write(&shared, handle) {
        Ok(Some(staged)) => {
            let previous = *staged.borrow();
            *staged.borrow_mut() = value;
            return previous;
        }
        Ok(None) => {}
        Err(error) => return shared_callback_fault(&error),
    }
    if let Some(result) = crate::runtime_host::native_shared_replace_value(handle, value) {
        return result.unwrap_or_else(|error| shared_callback_fault(&error));
    }
    let Some(value_slot) = shared.value.as_ref() else {
        return shared_callback_fault("external Shared replace is missing its physical owner");
    };
    let Some(permit) = shared.acquire_state_permit(true) else {
        return 0;
    };
    let previous = value_slot.load(Ordering::Acquire);
    if let Err(error) = shared_state_commit(&shared, value) {
        shared_commit_fault(&error);
        drop(permit);
        return 0;
    }
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
fn shared_commit_fault(message: &str) {
    Concurrency::with_runtime_mut(|rt| {
        if message == "SharedRevisionError.GenerationExhausted" {
            rt.set_trap(message);
        } else {
            rt.set_host_fault(message);
        }
    });
}
fn jet_jit_shared_read(handle: i64, callback: i64) -> i64 {
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return shared_callback_fault("Shared.read received an invalid shared handle");
    };
    match shared_transaction_staged(&shared, handle) {
        Ok(Some(staged)) => {
            let Some(slot) = shared_callback_slot(callback) else {
                return shared_callback_fault("Shared.read callback handle is invalid");
            };
            return crate::runtime_host::invoke_universal_unary(slot, *staged.borrow())
                .unwrap_or_else(|| {
                    shared_callback_fault("Shared.read callback has no unary universal thunk")
                });
        }
        Ok(None) => {}
        Err(error) => return shared_callback_fault(&error),
    }
    if let Some(result) = crate::runtime_host::native_shared_read_call(handle, callback) {
        return result.unwrap_or_else(|error| shared_callback_fault(&error));
    }
    let Some(permit) = shared.acquire_state_permit(false) else {
        return 0;
    };
    let Some(value) = shared_state_raw(&shared) else {
        drop(permit);
        return shared_callback_fault("external Shared read is missing its physical owner");
    };
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
    match shared_transaction_stage_for_write(&shared, handle) {
        Ok(Some(staged)) => {
            let Some(slot) = shared_callback_slot(callback) else {
                return shared_callback_fault("Shared.edit callback handle is invalid");
            };
            let mut value = staged.borrow_mut();
            let address = (&mut *value as *mut i64) as i64;
            return crate::runtime_host::invoke_universal_unary(slot, address).unwrap_or_else(
                || shared_callback_fault("Shared.edit callback has no unary universal thunk"),
            );
        }
        Ok(None) => {}
        Err(error) => return shared_callback_fault(&error),
    }
    if let Some(result) = crate::runtime_host::native_shared_edit_call(handle, callback) {
        return result.unwrap_or_else(|error| shared_callback_fault(&error));
    }
    let Some(permit) = shared.acquire_state_permit(true) else {
        return 0;
    };
    let Some(mut value) = shared_state_raw(&shared) else {
        drop(permit);
        return shared_callback_fault("external Shared edit is missing its physical owner");
    };
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
    if let Err(error) = shared_state_commit(&shared, value) {
        drop(permit);
        shared_commit_fault(&error);
        return 0;
    }
    drop(permit);
    result
}


fn jet_jit_shared_capture(handle: i64, type_id: i64) -> i64 {
    let type_id = type_id as u64;
    let Some(shared) = Concurrency::with_runtime_mut(|rt| {
        shared(rt, handle).filter(|state| shared_state_type_id(state) == Some(type_id))
    }) else {
        return shared_callback_fault("Shared.capture received an invalid shared handle or type");
    };
    let captured = match crate::runtime_host::native_shared_capture(handle) {
        Some(Ok((value, revision))) => Some((revision, value)),
        Some(Err(error)) => return shared_callback_fault(&error),
        None => shared_capture_parts(&shared),
    };
    let Some((revision, value)) = captured else {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            0
        });
    };
    match shared_snapshot_store_for_handle(handle, shared, type_id, revision, value) {
        Ok(ticket) => ticket,
        Err(error) => shared_callback_fault(&error),
    }
}
fn shared_projected_capture_value(
    revision: u64,
    value: i64,
    callback: i64,
) -> Option<(u64, i64)> {
    let slot = Concurrency::with_runtime_mut(|rt| {
        crate::runtime_host::jit_callable_parts(rt, callback)
    })?;
    let projected = crate::runtime_host::invoke_universal_unary(slot, value)?;
    Some((revision, projected))
}

fn shared_projected_capture(shared: &Arc<SharedState>, callback: i64) -> Option<(u64, i64)> {
    let permit = shared.acquire_state_permit(false)?;
    let revision = shared_state_revision(shared)?;
    let value = shared_state_raw(shared)?;
    let captured = shared_projected_capture_value(revision, value, callback);
    drop(permit);
    captured
}
fn shared_projected_capture_staged(
    shared: &Arc<SharedState>,
    handle: i64,
    callback: i64,
    staged: &std::rc::Rc<std::cell::RefCell<i64>>,
) -> Result<Option<(u64, i64)>, String> {
    let Some(revision) = shared_transaction_snapshot_revision(shared, handle)? else {
        return Ok(None);
    };
    let slot = Concurrency::with_runtime_mut(|rt| {
        crate::runtime_host::jit_callable_parts(rt, callback)
    });
    let Some(slot) = slot else {
        return Ok(None);
    };
    let projected =
        crate::runtime_host::invoke_universal_unary(slot, *staged.borrow());
    Ok(projected.map(|projected| (revision, projected)))
}

fn jet_jit_shared_capture_with(handle: i64, callback: i64, type_id: i64) -> i64 {
    let type_id = type_id as u64;
    let Some(shared) = Concurrency::with_runtime_mut(|rt| {
        shared(rt, handle).filter(|state| shared_state_type_id(state) == Some(type_id))
    }) else {
        return shared_callback_fault("Shared.capture received an invalid shared handle or type");
    };
    let captured = match crate::runtime_host::native_shared_capture(handle) {
        Some(Ok((value, revision))) => shared_projected_capture_value(revision, value, callback),
        Some(Err(error)) => return shared_callback_fault(&error),
        None => shared_projected_capture(&shared, callback),
    };
    let Some((revision, value)) = captured else {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("Shared.capture projection callback is invalid");
            0
        });
    };
    match shared_snapshot_store_for_handle(handle, shared, type_id, revision, value) {
        Ok(ticket) => ticket,
        Err(error) => shared_callback_fault(&error),
    }
}

fn shared_transaction_register_snapshot(
    snapshot: Arc<SharedSnapshot>,
    owner: Arc<dyn shared_protocol::JetSharedCanonicalOwner>,
) -> bool {
    SHARED_TRANSACTIONS.with(|transactions| {
        let mut transactions = transactions.borrow_mut();
        let Some(transaction) = transactions.last_mut() else {
            return false;
        };
        transaction
            .transaction
            .record_snapshot(owner, snapshot.valid.clone());
        true
    })
}

fn jet_jit_shared_capture_txn_plain(handle: i64, _stm: i64, type_id: i64) -> i64 {
    let type_id = type_id as u64;
    let Some(shared) = Concurrency::with_runtime_mut(|rt| {
        shared(rt, handle).filter(|state| shared_state_type_id(state) == Some(type_id))
    }) else {
        return shared_callback_fault("Shared.capture_txn received an invalid shared handle or type");
    };
    match shared_transaction_touch(&shared, handle) {
        Ok(true) => {}
        Ok(false) => {
            return Concurrency::with_runtime_mut(|rt| {
                rt.set_trap("Shared.capture_txn requires an active transaction");
                0
            });
        }
        Err(error) => return shared_callback_fault(&error),
    }
    let staged = match shared_transaction_staged(&shared, handle) {
        Ok(staged) => staged,
        Err(error) => return shared_callback_fault(&error),
    };
    let captured = if let Some(staged) = staged {
        match shared_transaction_snapshot_revision(&shared, handle) {
            Ok(Some(revision)) => Some((revision, *staged.borrow())),
            Ok(None) => None,
            Err(error) => return shared_callback_fault(&error),
        }
    } else {
        match crate::runtime_host::native_shared_capture(handle) {
            Some(Ok((value, revision))) => Some((revision, value)),
            Some(Err(error)) => return shared_callback_fault(&error),
            None => shared_capture_parts(&shared),
        }
    };
    let Some((revision, value)) = captured else {
        return shared_callback_fault("Shared.capture_txn could not capture its physical owner");
    };
    let owner = match shared_transaction_owner(&shared, handle) {
        Ok(owner) => owner,
        Err(error) => return shared_callback_fault(&error),
    };
    let token = match shared_snapshot_store_for_handle(handle, shared, type_id, revision, value) {
        Ok(token) => token,
        Err(error) => return shared_callback_fault(&error),
    };
    let snapshot = match Concurrency::with_runtime_string(|rt| {
        shared_snapshot_load(rt, token, type_id)
    }) {
        Ok(snapshot) => snapshot,
        Err(error) => return shared_callback_fault(&error),
    };
    if !shared_transaction_register_snapshot(snapshot, owner) {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("Shared.capture_txn requires an active transaction");
            0
        });
    }
    token
}

fn jet_jit_shared_capture_txn(handle: i64, _stm: i64, callback: i64, type_id: i64) -> i64 {
    let type_id = type_id as u64;
    let Some(shared) = Concurrency::with_runtime_mut(|rt| {
        shared(rt, handle).filter(|state| shared_state_type_id(state) == Some(type_id))
    }) else {
        return shared_callback_fault("Shared.capture_txn received an invalid shared handle or type");
    };
    match shared_transaction_touch(&shared, handle) {
        Ok(true) => {}
        Ok(false) => {
            return Concurrency::with_runtime_mut(|rt| {
                rt.set_trap("Shared.capture_txn requires an active transaction");
                0
            });
        }
        Err(error) => return shared_callback_fault(&error),
    }
    let staged = match shared_transaction_staged(&shared, handle) {
        Ok(staged) => staged,
        Err(error) => return shared_callback_fault(&error),
    };
    let captured = if let Some(staged) = staged {
        match shared_projected_capture_staged(&shared, handle, callback, &staged) {
            Ok(captured) => captured,
            Err(error) => return shared_callback_fault(&error),
        }
    } else {
        match crate::runtime_host::native_shared_capture(handle) {
            Some(Ok((value, revision))) => {
                shared_projected_capture_value(revision, value, callback)
            }
            Some(Err(error)) => return shared_callback_fault(&error),
            None => shared_projected_capture(&shared, callback),
        }
    };
    let Some((revision, value)) = captured else {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("Shared.capture projection callback is invalid");
            0
        });
    };
    let owner = match shared_transaction_owner(&shared, handle) {
        Ok(owner) => owner,
        Err(error) => return shared_callback_fault(&error),
    };
    let token = match shared_snapshot_store_for_handle(handle, shared, type_id, revision, value) {
        Ok(token) => token,
        Err(error) => return shared_callback_fault(&error),
    };
    let snapshot = match Concurrency::with_runtime_string(|rt| {
        shared_snapshot_load(rt, token, type_id)
    }) {
        Ok(snapshot) => snapshot,
        Err(error) => return shared_callback_fault(&error),
    };
    if !shared_transaction_register_snapshot(snapshot, owner) {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("Shared.capture_txn requires an active transaction");
            0
        });
    }
    token
}

fn jet_jit_shared_try_replace(
    handle: i64,
    snapshot_handle: i64,
    value: i64,
    type_id: i64,
) -> i64 {
    let type_id = type_id as u64;
    let Some(shared) = Concurrency::with_runtime_mut(|rt| {
        shared(rt, handle).filter(|state| shared_state_type_id(state) == Some(type_id))
    }) else {
        return Concurrency::with_runtime_mut(|rt| shared_revision_error_result(rt, 0));
    };
    let interop = match crate::runtime_host::native_shared_interop_for_type(handle, type_id) {
        Ok(interop) => interop,
        Err(error) => return shared_callback_fault(&error),
    };
    let snapshot = match Concurrency::with_runtime_string(|rt| {
        shared_snapshot_load(rt, snapshot_handle, type_id)
    }) {
        Ok(snapshot) => snapshot,
        Err(_) => return Concurrency::with_runtime_mut(|rt| shared_revision_error_result(rt, 0)),
    };
    if snapshot.root_identity != interop.identity() {
        return Concurrency::with_runtime_mut(|rt| shared_revision_error_result(rt, 0));
    }
    if !snapshot
        .valid
        .load(std::sync::atomic::Ordering::Acquire)
        || snapshot
            .consumed
            .load(std::sync::atomic::Ordering::Acquire)
    {
        return Concurrency::with_runtime_mut(|rt| {
            crate::runtime_host::alloc_jit_result(rt, true, 0)
        });
    }


    if let Some(result) = crate::runtime_host::native_shared_replace_if_revision(
        handle,
        snapshot.revision,
        value,
    ) {
        return match result {
            Ok((false, _)) => Concurrency::with_runtime_mut(|rt| {
                crate::runtime_host::alloc_jit_result(rt, true, 0)
            }),
            Ok((true, _)) => {
                if snapshot
                    .consumed
                    .swap(true, std::sync::atomic::Ordering::AcqRel)
                {
                    return Concurrency::with_runtime_mut(|rt| {
                        rt.set_host_fault("Shared snapshot was consumed during physical replacement");
                        crate::runtime_host::alloc_jit_result(rt, false, 0)
                    });
                }
                snapshot
                    .valid
                    .store(false, std::sync::atomic::Ordering::Release);
                Concurrency::with_runtime_mut(|rt| {
                    crate::runtime_host::alloc_jit_result(rt, true, 1)
                })
            }
            Err(error) => Concurrency::with_runtime_mut(|rt| {
                rt.set_host_fault(&error);
                crate::runtime_host::alloc_jit_result(rt, false, 0)
            }),
        };
    }

    if shared.value.is_none() {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_host_fault("external Shared conditional replace is missing its physical owner");
            crate::runtime_host::alloc_jit_result(rt, false, 0)
        });
    }
    let Some(permit) = shared.acquire_state_permit(true) else {
        return 0;
    };
    if shared_state_revision(&shared) != Some(snapshot.revision) {
        let result = Concurrency::with_runtime_mut(|rt| {
            crate::runtime_host::alloc_jit_result(rt, true, 0)
        });
        drop(permit);
        return result;
    }
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
    if let Err(error) = shared_state_commit(&shared, value) {
        snapshot
            .consumed
            .store(false, std::sync::atomic::Ordering::Release);
        let result = Concurrency::with_runtime_mut(|rt| {
            if error == "SharedRevisionError.GenerationExhausted" {
                shared_revision_error_result(rt, 1)
            } else {
                rt.set_host_fault(&error);
                crate::runtime_host::alloc_jit_result(rt, false, 0)
            }
        });
        drop(permit);
        return result;
    }
    snapshot
        .valid
        .store(false, std::sync::atomic::Ordering::Release);
    let result = Concurrency::with_runtime_mut(|rt| {
        crate::runtime_host::alloc_jit_result(rt, true, 1)
    });
    drop(permit);
    result
}

fn jet_jit_shared_snapshot_value(snapshot_handle: i64, type_id: i64) -> i64 {
    let type_id = type_id as u64;
    match Concurrency::with_runtime_string(|rt| shared_snapshot_load(rt, snapshot_handle, type_id)) {
        Ok(snapshot) => snapshot.value,
        Err(error) => shared_callback_fault(&error),
    }
}
fn jet_jit_shared_snapshot_clone(ticket: i64, type_id: i64) -> i64 {
    let type_id = type_id as u64;
    match shared_snapshot_clone_ticket(ticket, type_id) {
        Ok(ticket) => ticket,
        Err(error) => shared_callback_fault(&error),
    }
}

fn jet_jit_shared_snapshot_release(ticket: i64, type_id: i64) {
    let type_id = type_id as u64;
    if let Err(error) = shared_snapshot_release_ticket(ticket, type_id) {
        shared_callback_fault(&error);
    }
}


fn jet_jit_shared_begin(handle: i64, editable: i64) -> i64 {
    if let Some(result) =
        crate::runtime_host::native_shared_guard_begin(handle, editable != 0)
    {
        let (token, value) = match result {
            Ok(result) => result,
            Err(error) => return shared_callback_fault(&error),
        };
        SHARED_ACTIVE_NATIVE_GUARDS.with(|guards| guards.borrow_mut().push((handle, token)));
        return value;
    }
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return 0;
    };
    let Some(permit) = shared.acquire_state_permit(editable != 0) else {
        return 0;
    };
    let Some(value) = shared_state_raw(&shared) else {
        drop(permit);
        return shared_callback_fault("external Shared guard is missing its physical owner");
    };
    SHARED_ACTIVE_PERMITS.with(|permits| permits.borrow_mut().push((handle, permit)));
    value
}

fn jet_jit_shared_end_read(handle: i64) {
    if let Some(token) = take_active_native_shared_guard(handle) {
        match crate::runtime_host::native_shared_guard_end(token, handle, 0, false) {
            Some(Ok(())) => {}
            Some(Err(error)) => {
                shared_callback_fault(&error);
            }
            None => {
                shared_callback_fault("Source Shared guard lease disappeared before read end");
            }
        }
        return;
    }
    drop(take_active_shared_permit(handle));
}

fn jet_jit_shared_end_write(handle: i64, value: i64) {
    if let Some(token) = take_active_native_shared_guard(handle) {
        match crate::runtime_host::native_shared_guard_end(token, handle, value, true) {
            Some(Ok(())) => {}
            Some(Err(error)) => {
                shared_callback_fault(&error);
            }
            None => {
                shared_callback_fault("Source Shared guard lease disappeared before write end");
            }
        }
        return;
    }
    let permit = take_active_shared_permit(handle);
    if let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) {
        if shared.value.is_none() {
            shared_callback_fault("external Shared guard is missing its physical owner");
            drop(permit);
            return;
        }
        if let Err(error) = shared_state_commit(&shared, value) {
            shared_commit_fault(&error);
        }
    }
    drop(permit);
}

fn jet_jit_shared_retain(handle: i64, type_id: i64) -> i64 {
    let type_id = type_id as u64;
    match crate::runtime_host::native_shared_alias_retain(handle, type_id) {
        Ok(handle) => handle,
        Err(error) => shared_callback_fault(&error),
    }
}

fn jet_jit_shared_release(handle: i64, type_id: i64) {
    let type_id = type_id as u64;
    if let Err(error) = crate::runtime_host::native_shared_alias_release(handle, type_id) {
        shared_callback_fault(&error);
    }
}

fn jet_jit_shared_downgrade(handle: i64, type_id: i64) -> i64 {
    let type_id = type_id as u64;
    let owner = match crate::runtime_host::native_shared_owner_downgrade(handle, type_id) {
        Ok(owner) => owner,
        Err(error) => return shared_callback_fault(&error),
    };
    match Concurrency::with_runtime_string(|rt| shared_weak_owner_store(rt, type_id, owner)) {
        Ok(ticket) => ticket,
        Err(error) => shared_callback_fault(&error),
    }
}

fn jet_jit_shared_strong_count(handle: i64, type_id: i64) -> i64 {
    let type_id = type_id as u64;
    match crate::runtime_host::native_shared_owner_strong_count(handle, type_id) {
        Ok(count) => i64::try_from(count)
            .unwrap_or_else(|_| shared_callback_fault("Shared strong count overflow")),
        Err(error) => shared_callback_fault(&error),
    }
}

/// Identity only, like `Rc::ptr_eq`: both handles name one physical cell.
fn jet_jit_shared_same(handle: i64, other: i64, type_id: i64) -> i64 {
    let type_id = type_id as u64;
    let identity = |handle| {
        crate::runtime_host::native_shared_interop_for_type(handle, type_id)
            .map(|interop| interop.identity())
    };
    match (identity(handle), identity(other)) {
        (Ok(left), Ok(right)) => i64::from(left == right),
        (Err(error), _) | (_, Err(error)) => shared_callback_fault(&error),
    }
}

fn jet_jit_shared_weak_clone(ticket: i64, type_id: i64) -> i64 {
    let type_id = type_id as u64;
    match shared_weak_clone_ticket(ticket, type_id) {
        Ok(ticket) => ticket,
        Err(error) => shared_callback_fault(&error),
    }
}

fn jet_jit_shared_weak_release(ticket: i64, type_id: i64) {
    let type_id = type_id as u64;
    if let Err(error) = shared_weak_release_ticket(ticket, type_id) {
        shared_callback_fault(&error);
    }
}

fn jet_jit_shared_weak_upgrade(ticket: i64, type_id: i64) -> i64 {
    let type_id = type_id as u64;
    let owner = match Concurrency::with_runtime_string(|rt| {
        shared_weak_owner_load(rt, ticket, type_id)
    }) {
        Ok(owner) => owner,
        Err(error) => return shared_callback_fault(&error),
    };
    match owner.upgrade() {
        Ok(Some(upgrade)) => match upgrade.into_carrier() {
            Ok(carrier) => match crate::runtime_host::native_shared_export_owner(carrier) {
                Ok(handle) => handle
                    .checked_add(1)
                    .unwrap_or_else(|| shared_callback_fault("Shared handle overflow")),
                Err(error) => shared_callback_fault(&error),
            },
            Err(error) => shared_callback_fault(&error),
        },
        Ok(None) => 0,
        Err(error) => shared_callback_fault(&error),
    }
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
    if let Some(result) =
        crate::runtime_host::native_shared_guard_begin(handle, editable != 0)
    {
        let (token, value) = match result {
            Ok(result) => result,
            Err(error) => return shared_callback_fault(&error),
        };
        let view = match crate::runtime_host::native_shared_guard_entry_view(token) {
            Some(Ok(view)) => view,
            Some(Err(error)) => {
                crate::runtime_host::native_shared_guard_abort(token);
                return shared_callback_fault(&error);
            }
            None => {
                crate::runtime_host::native_shared_guard_abort(token);
                return shared_callback_fault(
                    "Source Shared guard view disappeared before binding",
                );
            }
        };
        let state = Arc::clone(&view.state);
        drop(view);
        let Some(guard) = Concurrency::with_runtime_mut(|rt| {
            shared(rt, handle)?;
            Some(pack_shared_guard(rt, handle, value, state))
        }) else {
            crate::runtime_host::native_shared_guard_abort(token);
            return shared_callback_fault("Source Shared guard could not bind a JIT guard");
        };
        if let Err(error) = crate::runtime_host::native_shared_guard_bind(guard, token) {
            Concurrency::with_runtime_mut(|rt| {
                rt.shared_guard_states.remove(&guard);
            });
            crate::runtime_host::native_shared_guard_abort(token);
            return shared_callback_fault(&error);
        }
        return guard;
    }
    let Some(shared) = Concurrency::with_runtime_mut(|rt| shared(rt, handle)) else {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_INVALID);
            0
        });
    };
    let Some(permit) = shared.acquire_state_permit(editable != 0) else {
        return 0;
    };
    let Some(state) =
        shared_protocol::jet_shared_guard_state_from_permit(permit, editable != 0).ok()
    else {
        return 0;
    };
    let Some(value) = shared_state_raw(&shared) else {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_host_fault("external Shared guard is missing its physical owner");
            0
        });
    };
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
            Ok(mapped_state) => {
                let shared_handle = guard_shared_handle(rt, guard)
                    .expect("validated SharedGuard carrier lost its shared handle");
                let Some(value) = rt.heap.record_get_int(guard, GUARD_VALUE) else {
                    rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                    return 0;
                };
                let native_entry = rt.native_shared_guards.get(&guard).map(|entry| {
                    let permit = Arc::clone(&entry.permit);
                    entry
                        .view
                        .entry_view()
                        .and_then(|view| view.map_path(&[field], editable != 0))
                        .map(|view| crate::runtime_host::NativeSharedGuardEntry { permit, view })
                });
                let native_entry = match native_entry {
                    Some(Ok(native_entry)) => Some(native_entry),
                    Some(Err(error)) => {
                        rt.set_trap(&error);
                        return 0;
                    }
                    None => None,
                };
                let _ = rt.native_shared_guards.remove(&guard);
                // Mapping consumes the source guard. Keep the source carrier
                // as an inert move marker and give the mapped projection its
                // own identity; both records retain the same physical source
                // guard lease until the final alias closes.
                rt.shared_guard_states.remove(&guard);
                let _ = rt.heap.record_set_int(guard, GUARD_SHARED, 0);
                let mapped = pack_shared_guard(rt, shared_handle, value, mapped_state);
                if let Some(native_entry) = native_entry {
                    rt.native_shared_guards.insert(mapped, native_entry);
                }
                mapped
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
                let native_entry = rt.native_shared_guards.get(&guard).map(|entry| {
                    let permit = Arc::clone(&entry.permit);
                    entry
                        .view
                        .entry_view()
                        .and_then(|view| {
                            view.split_path(&[first], &[second], editable != 0)
                        })
                        .map(|(first_view, second_view)| {
                            (permit, first_view, second_view)
                        })
                });
                let native_entry = match native_entry {
                    Some(Ok(native_entry)) => Some(native_entry),
                    Some(Err(error)) => {
                        rt.set_trap(&error);
                        return 0;
                    }
                    None => None,
                };
                let _ = rt.native_shared_guards.remove(&guard);
                rt.shared_guard_states.remove(&guard);
                let _ = rt.heap.record_set_int(guard, GUARD_SHARED, 0);
                let first = pack_shared_guard(rt, shared_handle, value, first_state);
                let second = pack_shared_guard(rt, shared_handle, value, second_state);
                if let Some((permit, first_view, second_view)) = native_entry {
                    rt.native_shared_guards.insert(
                        first,
                        crate::runtime_host::NativeSharedGuardEntry {
                            permit: Arc::clone(&permit),
                            view: first_view,
                        },
                    );
                    rt.native_shared_guards.insert(
                        second,
                        crate::runtime_host::NativeSharedGuardEntry {
                            permit,
                            view: second_view,
                        },
                    );
                }
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
    let native_entry = Concurrency::with_runtime_mut(|rt| {
        rt.native_shared_guards.get(&guard).map(|entry| {
            let permit = Arc::clone(&entry.permit);
            entry
                .view
                .clone_guard(editable != 0)
                .map(|view| crate::runtime_host::NativeSharedGuardEntry { permit, view })
        })
    });
    let native_entry = match native_entry {
        Some(Ok(native_entry)) => Some(native_entry),
        Some(Err(message)) => {
            return Concurrency::with_runtime_mut(|rt| {
                rt.set_trap(&message);
                0
            });
        }
        None => None,
    };
    Concurrency::with_runtime_mut(|rt| {
        let cloned = pack_shared_guard(rt, shared, value, state);
        if let Some(native_entry) = native_entry {
            rt.native_shared_guards.insert(cloned, native_entry);
        }
        cloned
    })
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
    if let Some(value_slot) = shared.value.as_ref() {
        value_slot.store(value, Ordering::Release);
    }
    Ok(())
}

fn jet_jit_shared_guard_set_value(guard: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field, _root)) = editable_guard_slot_or_trap(rt, guard) else {
            return;
        };
        if rt.heap.record_set_int(record, field, value).is_none() {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
        } else {
            mark_shared_guard_dirty(rt, guard);
        }
    });
}

fn jet_jit_shared_guard_set_value_f64(guard: i64, value: f64) {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field, root)) = editable_guard_slot_or_trap(rt, guard) else {
            return;
        };
        let stored = if root {
            rt.heap
                .record_set_int(record, field, value.to_bits() as i64)
                .is_some()
        } else {
            rt.heap.record_set_float(record, field, value).is_some()
        };
        if stored {
            mark_shared_guard_dirty(rt, guard);
        } else {
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
        let stored = if root {
            rt.heap
                .record_set_int(record, field, i64::from(value))
                .is_some()
        } else {
            rt.heap.record_set_bool(record, field, value).is_some()
        };
        if stored {
            mark_shared_guard_dirty(rt, guard);
        } else {
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
        let stored = if root {
            rt.heap
                .record_set_int(record, field, i64::from(value as u32))
                .is_some()
        } else {
            rt.heap.record_set_char(record, field, value).is_some()
        };
        if stored {
            mark_shared_guard_dirty(rt, guard);
        } else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
        }
    });
}

fn jet_jit_shared_guard_set_value_string(guard: i64, value: i64) {
    Concurrency::with_runtime_mut(|rt| {
        let Some((record, field, root)) = editable_guard_slot_or_trap(rt, guard) else {
            return;
        };
        let stored = if root {
            rt.heap.record_set_int(record, field, value).is_some()
        } else {
            rt.heap.record_set_string(record, field, value).is_some()
        };
        if stored {
            mark_shared_guard_dirty(rt, guard);
        } else {
            rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
        }
    });
}

fn jet_jit_shared_guard_end(guard: i64) {
    let Some((shared_handle, shared, value, editable, dirty, root, state)) =
        Concurrency::with_runtime_mut(|rt| {
            let Some(shared_handle) = guard_shared_handle(rt, guard) else {
                return None;
            };
            let state = rt.shared_guard_states.remove(&guard)?;
            let root = state.path().is_empty();
            let Some(value) = rt.heap.record_get_int(guard, GUARD_VALUE) else {
                rt.set_trap(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED);
                return None;
            };
            let dirty = rt.heap.record_get_int(guard, GUARD_DIRTY)? != 0;
            let editable = state.editable();
            let _ = rt.heap.record_set_int(guard, GUARD_SHARED, 0);
            Some((
                shared_handle,
                shared(rt, shared_handle),
                value,
                editable,
                dirty,
                root,
                state,
            ))
        })
    else {
        crate::runtime_host::native_shared_guard_abort(guard);
        return;
    };
    let mut state = Some(state);
    let state_held = state.as_ref().map(|state| state.held()).unwrap_or(false);
    let native_prebound = crate::runtime_host::native_shared_guard_is_bound(guard)
        .unwrap_or(false);
    if native_prebound {
        drop(state.take());
    }
    let native_result =
        crate::runtime_host::native_shared_guard_end(guard, shared_handle, value, editable);
    let native_bound = native_result.is_some();
    if let Some(Err(error)) = native_result {
        shared_callback_fault(&error);
        drop(state);
        return;
    }
    if let Some(shared) = shared {
        if editable && dirty {
            if native_bound {
                if let Some(value_slot) = shared.value.as_ref() {
                    value_slot.store(value, Ordering::Release);
                }
            } else if root && state_held {
                if let Err(error) = shared_state_commit(&shared, value) {
                    shared_commit_fault(&error);
                }
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

struct JitSourceGuardWaitHandoff {
    guard: i64,
    shared: i64,
    raw: i64,
    resumed: Option<i64>,
    error: Option<String>,
    source_active: bool,
    local_state: Option<Arc<SharedState>>,
}

impl JitSourceGuardWaitHandoff {
    fn new(
        guard: i64,
        shared: i64,
        raw: i64,
        local_state: Option<Arc<SharedState>>,
    ) -> Self {
        Self {
            guard,
            shared,
            raw,
            resumed: None,
            error: None,
            source_active: false,
            local_state,
        }
    }

    fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    fn record_error(&mut self, error: String) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }
}

impl shared_protocol::JetSharedWaitHandoff for JitSourceGuardWaitHandoff {
    fn suspend(&mut self) -> Result<(), ()> {
        if let Some(state) = &self.local_state {
            let dirty = Concurrency::with_runtime_mut(|rt| {
                rt.heap.record_get_int(self.guard, GUARD_DIRTY)
            });
            let Some(dirty) = dirty else {
                self.record_error(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED.to_string());
                return Err(());
            };
            if dirty != 0 {
                if let Err(error) = shared_state_commit(state, self.raw) {
                    self.record_error(error);
                    return Err(());
                }
            }
            let reset = Concurrency::with_runtime_mut(|rt| {
                rt.heap.record_set_int(self.guard, GUARD_DIRTY, 0).is_some()
            });
            if !reset {
                self.record_error(shared_protocol::JET_SHARED_GUARD_VALUE_STORAGE_FAILED.to_string());
                return Err(());
            }
            return Ok(());
        }
        match crate::runtime_host::native_shared_guard_wait_suspend(
            self.guard,
            self.shared,
            self.raw,
        ) {
            Some(Ok(())) => {
                self.source_active = true;
                Ok(())
            }
            Some(Err(error)) => {
                self.record_error(error);
                Err(())
            }
            None => {
                self.record_error("Source Shared guard lease disappeared before wait".to_string());
                Err(())
            }
        }
    }

    fn resume(&mut self, cancelled: bool) -> Result<bool, ()> {
        if self.local_state.is_some() {
            return Ok(true);
        }
        match crate::runtime_host::native_shared_guard_wait_resume(
            self.guard,
            self.shared,
            cancelled,
        ) {
            Some(Ok(Some(raw))) => {
                self.source_active = true;
                self.resumed = Some(raw);
                Ok(true)
            }
            Some(Ok(None)) => {
                self.source_active = false;
                Ok(false)
            }
            Some(Err(error)) => {
                self.record_error(error);
                Err(())
            }
            None => {
                self.record_error("Source Shared guard lease disappeared after wait".to_string());
                Err(())
            }
        }
    }

    fn abort(&mut self) -> Result<(), ()> {
        if self.local_state.is_some() {
            return Ok(());
        }
        match crate::runtime_host::native_shared_guard_wait_abort(self.guard) {
            Some(Ok(())) => {
                self.source_active = false;
                Ok(())
            }
            Some(Err(error)) => {
                self.record_error(error);
                Err(())
            }
            None => {
                self.record_error("Source Shared guard lease disappeared while aborting wait".to_string());
                Err(())
            }
        }
    }
}

impl Drop for JitSourceGuardWaitHandoff {
    fn drop(&mut self) {
        if self.source_active {
            let _ = crate::runtime_host::native_shared_guard_wait_abort(self.guard);
            self.source_active = false;
        }
    }
}

fn jet_jit_shared_guard_wait_once(guard: i64, condition_handle: i64) -> i64 {
    let Some((shared_handle, state, condition, raw)) = Concurrency::with_runtime_mut(|rt| {
        let shared_handle = guard_shared_handle(rt, guard)?;
        let state = guard_state(rt, guard)?;
        let condition = condition(rt, condition_handle)?;
        let raw = rt.heap.record_get_int(guard, GUARD_VALUE)?;
        Some((shared_handle, state, condition, raw))
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
    if let Err(error) = shared_protocol::jet_shared_guard_require_edit(&state) {
        return Concurrency::with_runtime_mut(|rt| {
            if error == shared_protocol::JET_SHARED_GUARD_INVALID {
                rt.set_trap(error);
            }
            shared_guard_result(rt, false, Some(error))
        });
    }
    let source_bound =
        crate::runtime_host::native_shared_guard_is_bound(guard).unwrap_or(false);
    let local_state = if !source_bound && state.path().is_empty() {
        Concurrency::with_runtime_mut(|rt| shared(rt, shared_handle))
    } else {
        None
    };
    let has_handoff = source_bound || local_state.is_some();
    let mut handoff = JitSourceGuardWaitHandoff::new(guard, shared_handle, raw, local_state);
    let waiter = Arc::new(JitConditionWaiter::new());
    let waited = jet_codegen::scheduler::jet_scheduler_wait_without_unwind(|| {
        shared_protocol::jet_shared_guard_wait_once_with_handoff(
            Some(state.as_ref()),
            Some(&condition.protocol),
            waiter,
            if has_handoff {
                Some(&mut handoff as &mut dyn shared_protocol::JetSharedWaitHandoff)
            } else {
                None
            },
        )
    });
    if source_bound
        && matches!(
            &waited,
            jet_codegen::scheduler::JetSchedulerWait::Panicked(_)
        )
    {
        let _ = shared_protocol::JetSharedWaitHandoff::abort(&mut handoff);
    }
    if let Some(error) = handoff.take_error() {
        return shared_callback_fault(&error);
    }
    handoff.source_active = false;
    let resumed = handoff.resumed.take();
    match waited {
        jet_codegen::scheduler::JetSchedulerWait::Ready(Ok(())) => {
            let Some(fresh) = resumed.or_else(|| {
                Concurrency::with_runtime_mut(|rt| {
                    Some(shared(rt, shared_handle)?.value.as_ref()?.load(Ordering::Acquire))
                })
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
                if rt.heap.record_set_int(guard, GUARD_VALUE, fresh).is_none()
                    || rt.heap.record_set_int(guard, GUARD_DIRTY, 0).is_none()
                {
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
    if let Err(error) = shared_transaction_touch(&shared, handle) {
        shared_callback_fault(&error);
    }
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
    let staged = match shared_transaction_stage_for_write(&shared, handle) {
        Ok(Some(staged)) => staged,
        Ok(None) => return 0,
        Err(error) => return shared_callback_fault(&error),
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
        let owner = match shared_transaction_owner(&shared, handle) {
            Ok(owner) => owner,
            Err(error) => return shared_callback_fault(&error),
        };
        SHARED_TRANSACTIONS.with(|transactions| {
            let mut transactions = transactions.borrow_mut();
            if let Some(transaction) = transactions.last_mut() {
                transaction.transaction.record_edit(owner, delta);
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
    if let Err(error) = transaction.transaction.try_commit_with(|| ()) {
        Concurrency::with_runtime_mut(|rt| rt.set_host_fault(&error));
    }
}

fn jet_jit_shared_txn_abort() {
    SHARED_TRANSACTIONS.with(|transactions| {
        transactions.borrow_mut().pop();
    });
}

fn jet_jit_expiring_new(value: i64, duration: i64, clock: i64, secret: i64) -> i64 {
    let secret = secret != 0;
    if secret && expiring_secret_bytes(value).is_none() {
        Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("secret key handle is invalid or already moved");
        });
        return 0;
    }
    Concurrency::with_runtime_mut(|rt| {
        let now = rt.clock_now(clock);
        // `Duration` is nanoseconds; resident clocks report milliseconds.
        // Preserve a positive sub-millisecond duration instead of silently
        // turning it into an already-expired entry.
        let duration_ms = duration
            .max(0)
            .saturating_add(999_999)
            .checked_div(1_000_000)
            .unwrap_or(i64::MAX);
        rt.expirings.push(ExpiringState {
            value,
            expires_at: now.saturating_add(duration_ms),
            clock,
            secret,
        });
        rt.expirings.len() as i64
    })
}

fn jet_jit_expiring_get(handle: i64, clock: i64) -> i64 {
    let (value, expired, wipe) = Concurrency::with_runtime_mut(|rt| {
        let stored_clock = rt
            .expirings
            .get((handle as usize).wrapping_sub(1))
            .map(|state| state.clock)
            .unwrap_or(0);
        let clock = if clock == 0 { stored_clock } else { clock };
        let now = rt.clock_now(clock);
        let Some(state) = rt.expirings.get_mut((handle as usize).wrapping_sub(1)) else {
            return (0_i64, true, None);
        };
        if now > state.expires_at {
            let expired = std::mem::take(&mut state.value);
            return (0, true, (state.secret && expired != 0).then_some(expired));
        }
        (state.value, false, None)
    });
    if let Some(record) = wipe {
        wipe_expired_secret(record);
    }
    Concurrency::with_runtime_mut(|rt| {
        crate::runtime_host::alloc_jit_result(rt, !expired, value as u64)
    })
}

fn jet_jit_expiring_is_valid(handle: i64, clock: i64) -> i8 {
    let result = jet_jit_expiring_get(handle, clock);
    Concurrency::with_runtime_mut(|rt| {
        i8::from(crate::runtime_host::jit_result_is_ok(rt, result).unwrap_or(false))
    })
}
fn jet_jit_expiring_secret_with(handle: i64, callback: i64) -> i64 {
    let (value, expired, wipe) = Concurrency::with_runtime_mut(|rt| {
        let index = (handle as usize).wrapping_sub(1);
        let stored_clock = rt.expirings.get(index).map(|state| state.clock).unwrap_or(0);
        let now = rt.clock_now(stored_clock);
        let Some(state) = rt.expirings.get_mut(index) else {
            return (0, true, None);
        };
        if state.value == 0 || now > state.expires_at {
            let expired = std::mem::take(&mut state.value);
            return (0, true, (state.secret && expired != 0).then_some(expired));
        }
        (state.value, false, None)
    });
    if let Some(record) = wipe {
        wipe_expired_secret(record);
    }
    if expired {
        return Concurrency::with_runtime_mut(|rt| {
            crate::runtime_host::alloc_jit_result(rt, false, 0)
        });
    }
    let Some(slot) = Concurrency::with_runtime_mut(|rt| {
        crate::runtime_host::jit_callable_parts(rt, callback)
    }) else {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("ExpiringSecret.with callback is invalid");
            crate::runtime_host::alloc_jit_result(rt, false, 0)
        });
    };
    let Some(result) = crate::runtime_host::invoke_universal_unary(slot, value) else {
        return Concurrency::with_runtime_mut(|rt| {
            rt.set_trap("ExpiringSecret.with callback has no unary thunk");
            crate::runtime_host::alloc_jit_result(rt, false, 0)
        });
    };
    Concurrency::with_runtime_mut(|rt| {
        crate::runtime_host::alloc_jit_result(rt, true, result as u64)
    })
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
    expiring_secret_with: "jet_expiring_secret_with" => jet_jit_expiring_secret_with: binary;
    expiring_secret_with_jit: "jet_jit_expiring_secret_with" => jet_jit_expiring_secret_with: binary;
    pool_new: "jet_std::JetPool::new" => jet_jit_pool_new: noarg_i64;
    pool_add: "jet_std::JetPool::add" => jet_jit_pool_add: binary;
    pool_get: "jet_jit_pool_get" => jet_jit_pool_get: quaternary;

    shared_new_static: "jet_std::JetShared::new" => jet_jit_shared_new: binary;
    shared_new_rooted: "::jet_std::JetShared::new" => jet_jit_shared_new: binary;
    shared_get: "jet_shared_get" => jet_jit_shared_get: unary;
    shared_set: "jet_shared_set" => jet_jit_shared_set: binary_void;
    shared_replace: "jet_shared_replace" => jet_jit_shared_replace: binary;
    shared_read: "jet_shared_read" => jet_jit_shared_read: binary;
    shared_edit: "jet_shared_edit" => jet_jit_shared_edit: binary;
    shared_edit_txn: "jet_shared_edit_txn" => jet_jit_shared_edit_txn: ternary;
    shared_read_txn: "jet_shared_read_txn" => jet_jit_shared_read_txn: ternary;
    shared_capture: "jet_shared_capture" => jet_jit_shared_capture: binary;
    shared_capture_with: "jet_shared_capture_with" => jet_jit_shared_capture_with: ternary;
    shared_capture_txn_plain: "jet_shared_capture_txn_plain" => jet_jit_shared_capture_txn_plain: ternary;
    shared_capture_txn: "jet_shared_capture_txn" => jet_jit_shared_capture_txn: quaternary;
    shared_try_replace: "jet_shared_try_replace" => jet_jit_shared_try_replace: quaternary;
    shared_snapshot_value: "jet_shared_snapshot_value" => jet_jit_shared_snapshot_value: binary;
    shared_snapshot_clone: "jet_jit_shared_snapshot_clone" => jet_jit_shared_snapshot_clone: binary;
    shared_snapshot_release: "jet_jit_shared_snapshot_release" => jet_jit_shared_snapshot_release: binary_void;
    pool_set: "jet_jit_pool_set" => jet_jit_pool_set: quinary_void;
    pool_remove: "jet_std::JetPool::remove" => jet_jit_pool_remove: binary;
    pool_ids: "jet_std::JetPool::ids" => jet_jit_pool_ids: unary;
    shared_new: "jet_jit_shared_new" => jet_jit_shared_new: binary;
    shared_retain: "jet_jit_shared_retain" => jet_jit_shared_retain: binary;
    shared_release: "jet_jit_shared_release" => jet_jit_shared_release: binary_void;
    shared_begin: "jet_jit_shared_begin" => jet_jit_shared_begin: binary;
    shared_end_read: "jet_jit_shared_end_read" => jet_jit_shared_end_read: unary_void;
    shared_end_write: "jet_jit_shared_end_write" => jet_jit_shared_end_write: binary_void;
    shared_downgrade: "jet_jit_shared_downgrade" => jet_jit_shared_downgrade: binary;
    shared_strong_count: "jet_jit_shared_strong_count" => jet_jit_shared_strong_count: binary;
    shared_same: "jet_jit_shared_same" => jet_jit_shared_same: ternary;
    shared_weak_clone: "jet_jit_shared_weak_clone" => jet_jit_shared_weak_clone: binary;
    shared_weak_upgrade: "jet_jit_shared_weak_upgrade" => jet_jit_shared_weak_upgrade: binary;
    shared_weak_release: "jet_jit_shared_weak_release" => jet_jit_shared_weak_release: binary_void;
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
    expiring_secret_new: "jet_jit_expiring_secret_new" => jet_jit_expiring_new: quaternary;
    expiring_get: "jet_jit_expiring_get" => jet_jit_expiring_get: binary;
    expiring_is_valid: "jet_jit_expiring_is_valid" => jet_jit_expiring_is_valid: binary_i8;
    expiring_new_prelude: "jet_expiring_new" => jet_jit_expiring_new: quaternary;
    expiring_secret_new_prelude: "jet_expiring_secret_new" => jet_jit_expiring_new: quaternary;
    expiring_get_prelude: "jet_expiring_get" => jet_jit_expiring_get: binary;
    volatile_read: "std::ptr::read_volatile" => jet_jit_volatile_read: unary;
    volatile_write: "std::ptr::write_volatile" => jet_jit_volatile_write: binary_void;
}
