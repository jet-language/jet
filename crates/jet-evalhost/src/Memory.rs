//! Native memory carriers for the resident Cranelift runtime.

// This module includes shared Prelude source that several hosts compile,
// each using a different subset, so dead-code reports here are about the
// other hosts' usage, not about this one. Scoped to the module, never the crate.

use std::cell::{Cell, RefCell};

thread_local! {
    static JIT_SENTRY_GUARDS:
        RefCell<Vec<jet_foundation::MemSentry::JetSentryGuard>> =
        const { RefCell::new(Vec::new()) };
    static JIT_SENTRY_FRAMES:
        RefCell<Vec<jet_foundation::MemSentry::JetSentryFrame>> =
        const { RefCell::new(Vec::new()) };
    static JIT_SENTRY_RUN: Cell<(usize, u64)> = const { Cell::new((0, 0)) };
}

use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub(crate) mod shared_protocol {
    include!("../../jet-codegen/src/Prelude/SharedProtocol.rs");
}

thread_local! {
    static SHARED_ACTIVE_PERMITS:
        std::cell::RefCell<Vec<(i64, Arc<SharedStatePermit>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
    static SHARED_ACTIVE_NATIVE_GUARDS: std::cell::RefCell<Vec<(i64, i64)>> =
        const { std::cell::RefCell::new(Vec::new()) };
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
    consumed: Arc<std::sync::atomic::AtomicBool>
}

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
    _code: &'static str,
    _file: &str,
    _line: u32,
    _gate: &str,
    _operation: &str,
    _obligation: &str,
    _obligation_status: &str,
    _foreign_component: Option<&str>,
    _foreign_fenced: Option<bool>,
    detail: &str,
) -> ! {
    panic!("{detail}");
}

mod canonical_mem {
    mod jet_sentry {
        pub use jet_foundation::MemSentry::{
            jet_memory_ledger_record, jet_sentry_check,
            jet_sentry_check_foreign_strict, jet_sentry_check_foreign_with_component,
            jet_sentry_current_frame, jet_sentry_fenced_scope, jet_sentry_frame,
            jet_sentry_policy_scope, jet_sentry_quarantine, jet_sentry_quarantine_owner,
            jet_sentry_register_allocation, jet_sentry_register_owned_allocation,
            jet_sentry_register_stack_allocation, jet_sentry_reset, jet_sentry_scope,
            jet_sentry_set_hardened, JetSentryFault, JetSentryFrame, JetSentryGuard,
            MemoryLedgerWitness
        };
    }

    // Mem.rs supplies both arena ownership and the audited foreign-byte leaves.
    // Do not include ForeignBytes.rs beside it: those symbols share one owner.
    include!("../../jet-codegen/src/Prelude/Mem.rs");
}

pub use jet_foundation::Outcome::{jet_alloc_error, jet_try_alloc_value, AllocError};

type CanonicalPool = jet_codegen::Comptime::PoolRuntime::jet_std::JetPool<i64>;

pub(crate) struct PoolState {
    pool: CanonicalPool
}

impl Default for PoolState {
    fn default() -> Self {
        Self {
            pool: CanonicalPool::new()
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
    next_owner_alias_token: AtomicI64
}

struct SharedOwnerLifetime {
    logical_owners: usize,
    active_borrows: usize
}

pub(crate) struct SharedStatePermit {
    state: Arc<SharedState>,
    protocol: Arc<shared_protocol::JetSharedPermit>,
    active: AtomicBool
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
                active_borrows: 0
            })),
            payload_finalizer: Mutex::new(None),
            next_owner_alias_token: AtomicI64::new(1)
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
            next_owner_alias_token: AtomicI64::new(1)
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
            active: AtomicBool::new(true)
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
            None => shared_protocol::JetSharedPhysicalOperationOutcome::new(Ok(()), None)
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
            active: true
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
    active: bool
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

