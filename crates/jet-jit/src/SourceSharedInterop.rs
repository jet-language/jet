//! Private physical transport for compiler-implementation `Shared<T>` values.
//!
//! This is deliberately separate from the Source evaluator's logical
//! `Shared(index)` carrier.  A value crossing the compiler entry/callback seam
//! retains one physical root and performs typed payload conversion while the
//! root's canonical read or edit permit is held.  The JIT and the generated
//! NativeAdapter use this seam; no raw pointer, heap record sentinel, or
//! evaluator session ID is a language value.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use jet_foundation::MIR::{MirNativeOwned, MirRuntimeValue};

pub type SourceSharedInteropPayloadFinalizer =
    crate::Memory::shared_protocol::JetSharedPhysicalFinalizerBinding<MirRuntimeValue>;
pub type SourceSharedInteropCompletionDelivery =
    crate::Memory::shared_protocol::JetSharedPhysicalCompletionDelivery;
type SourceSharedInteropPayloadFinalizerInstaller = Arc<
    dyn Fn(&mut Option<SourceSharedInteropPayloadFinalizer>) -> Result<(), String>
        + Send
        + Sync
        + 'static,
>;

/// Clone the exact typed view for a currently live native Source Shared guard
/// token in this invocation. The token is only a lookup key in its origin
/// runtime, never portable permit authority.
pub fn native_shared_guard_entry_view(
    token: i64,
) -> Option<Result<SourceSharedInteropGuardState, String>> {
    crate::runtime_host::native_shared_guard_entry_view(token)
}
fn finish_physical_operation<T>(
    completion_delivery: &Arc<Mutex<Option<SourceSharedInteropCompletionDelivery>>>,
    mut outcome: crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<T>,
) -> Result<T, String> {
    if let Some(completion) = outcome.completion.take() {
        let delivery = completion_delivery
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .cloned()
            .expect("physical Shared completion requires its installed delivery binding");
        delivery(completion);
    }
    outcome.result
}
/// Compute the exact JIT descriptor identity for a checked MIR type. Nominal
/// types use their checked identity; structural types (including
/// `Shared<scalar>`) use the same synthetic canonical-key identity as the
/// resident runtime descriptor table.
pub fn checked_type_id(ty: &jet_foundation::MIR::MirType) -> u64 {
    ty.identity
        .map(|identity| identity.0)
        .unwrap_or_else(|| {
            jet_foundation::MIR::stable_id("jit-runtime-type", &ty.kind.canonical_key())
                | (1_u64 << 63)
        })
}
/// Compute the descriptor identity in a checked program, including restored
/// type-instance and nominal-definition identities.
pub fn checked_type_id_for_program(
    program: &jet_foundation::MIR::MirProgram,
    ty: &jet_foundation::MIR::MirType,
) -> u64 {
    if let Some(identity) = ty.identity {
        return identity.0;
    }
    if let Some(instance) = program
        .type_instances
        .iter()
        .find(|instance| instance.canonical_key() == ty.canonical_key())
    {
        return checked_type_id(instance);
    }
    if let jet_foundation::MIR::MirTypeKind::Apply { name, args } = &ty.kind {
        if args.is_empty() {
            if let Some(definition) = program.types.iter().find(|definition| {
                definition.id == name.id
                    || definition.key == name.name
                    || definition.name == name.name
            }) {
                return definition.id.0;
            }
        }
    }
    checked_type_id(ty)
}


/// The physical operations supplied by the owner of one typed Shared root.
///
/// Each operation must invoke its callback exactly once while the canonical
/// read/edit permit is held. An edit callback error is a transport or codec
/// failure and aborts the physical edit; a successful edit callback commits
/// the value supplied by the callback under that same permit. A semantic
/// Source callback error is an ordinary value in the callback's successful
/// result and therefore still commits. `acquire_guard` holds the same
/// physical permit until the returned guard is dropped.
pub trait SourceSharedInteropBackend: Send + Sync + 'static {
    fn with_read(
        &self,
        callback: &mut dyn FnMut(&MirRuntimeValue) -> Result<(), String>,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()>;
    fn with_read_revision(
        &self,
        callback: &mut dyn FnMut(&MirRuntimeValue, u64) -> Result<(), String>,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()>;

    fn with_edit(
        &self,
        callback: &mut dyn FnMut(&mut MirRuntimeValue) -> Result<(), String>,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()>;
    fn replace_if_revision(
        &self,
        expected_revision: u64,
        replacement: MirRuntimeValue,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<(bool, u64)>;

    fn acquire_guard(
        &self,
        editable: bool,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<
        Box<dyn SourceSharedInteropGuard>,
    >;
}


/// Physical owner weak handles preserve source Shared lifetime semantics
/// without retaining a wrapper or a copied payload.
pub trait SourceSharedInteropWeakOwner: Send + Sync + 'static {
    /// Atomically require a live logical owner and reserve one strong alias.
    /// The returned lease must represent that exact reservation.
    fn upgrade(
        &self,
    ) -> Result<
        Option<(
            SourceSharedInterop,
            Box<dyn SourceSharedInteropOwnerAliasLease>,
        )>,
        String,
    >;
}

struct OwnerLifecycle {
    downgrade: Arc<dyn Fn() -> Box<dyn SourceSharedInteropWeakOwner> + Send + Sync>,
}

/// Owner-provided logical alias reservation. Explicit release returns any
/// completion produced by retiring this exact physical owner.
pub trait SourceSharedInteropOwnerAliasLease: Send + Sync + 'static {
    fn token_id(&self) -> i64;
    fn release(
        self: Box<Self>,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()>;
}

struct OwnerAliasLifecycle {
    strong_count: Arc<dyn Fn() -> Result<usize, String> + Send + Sync>,
    retain: Arc<
        dyn Fn() -> Result<Box<dyn SourceSharedInteropOwnerAliasLease>, String> + Send + Sync,
    >,
}

#[must_use = "dropping the token releases one logical Source Shared owner alias"]
pub struct SourceSharedInteropOwnerAlias {
    owner_identity: usize,
    token_id: i64,
    lease: Mutex<Option<Box<dyn SourceSharedInteropOwnerAliasLease>>>,
}

impl SourceSharedInteropOwnerAlias {
    pub(crate) fn new(
        owner_identity: usize,
        lease: Box<dyn SourceSharedInteropOwnerAliasLease>,
    ) -> Self {
        let token_id = lease.token_id();
        Self {
            owner_identity,
            token_id,
            lease: Mutex::new(Some(lease)),
        }
    }

    pub fn token_id(&self) -> i64 {
        self.token_id
    }

    pub fn release(
        &self,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        let lease = self
            .lease
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        match lease {
            Some(lease) => lease.release(),
            None => crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(
                Err("Source Shared owner alias was already released".to_string()),
                None,
            ),
        }
    }
}

/// Linearizable weak-upgrade result: the root and its newly reserved logical
/// strong alias travel together so callers cannot separate upgrade from retain.
pub struct SourceSharedInteropOwnerAliasUpgrade {
    interop: SourceSharedInterop,
    owner_alias: SourceSharedInteropOwnerAlias,
}

impl SourceSharedInteropOwnerAliasUpgrade {
    pub fn interop(&self) -> &SourceSharedInterop {
        &self.interop
    }

    pub fn owner_alias(&self) -> &SourceSharedInteropOwnerAlias {
        &self.owner_alias
    }

    pub fn into_parts(
        self,
    ) -> (SourceSharedInterop, SourceSharedInteropOwnerAlias) {
        (self.interop, self.owner_alias)
    }

    /// Adopt the reserved alias into the carrier so every carrier clone
    /// shares exactly one logical owner lease.
    pub fn into_carrier(self) -> Result<SourceSharedInterop, String> {
        self.interop.with_owner_alias_lease(self.owner_alias)
    }
}

/// A non-Send physical Shared guard. The JIT keeps this object in its
/// invocation-local runtime, never in the Send+Sync root carrier.
pub trait SourceSharedInteropGuard: 'static {
    /// Invoke a read projection while this guard's canonical permit is held.
    fn with_read(
        &mut self,
        callback: &mut dyn FnMut(&MirRuntimeValue) -> Result<(), String>,
    ) -> Result<(), String> {
        let value = self.read_value()?;
        callback(&value)
    }

    /// Invoke an edit projection while this guard's canonical permit is held.
    /// Semantic callback outcomes belong in a successful callback result so
    /// the staged value still publishes.
    fn with_edit(
        &mut self,
        callback: &mut dyn FnMut(&mut MirRuntimeValue) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut value = self.read_value()?;
        callback(&mut value)?;
        self.stage_value(value)
    }

    /// Clone the current typed payload while this guard's canonical permit is
    /// held. The clone is only the JIT projection; the owner remains the root.
    fn read_value(&mut self) -> Result<MirRuntimeValue, String>;

    /// Stage a typed payload replacement while this guard's permit is held.
    /// The physical root publishes it when the final aliasing guard is dropped.
    fn stage_value(&mut self, value: MirRuntimeValue) -> Result<(), String>;
    /// Read this physical owner's revision under the already-held guard.
    fn revision(&self) -> Result<u64, String>;
    /// Whether the physical permit is currently held by this logical guard.
    fn held(&self) -> bool {
        true
    }

    /// Close this physical guard and return any typed owner completion.
    fn release(
        &mut self,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()>;
    /// Drop fallback for closing this physical guard. Implementations route
    /// any finalizer completion through their paired owner binding.
    fn release_during_drop(&mut self);

    /// Optional typed access-time projection owned by this guard (for example,
    /// a checked codec's stable Box<T> shadow). Never cast a dynamic MIR
    /// payload to T; dynamic owners should leave this unsupported.
    fn root_ptr(&mut self) -> Result<*mut (), String> {
        Err("Source Shared owner has no typed physical projection".to_string())
    }

    /// Mark the optional typed access-time projection dirty before yielding
    /// mutable access; the guard stages it back before releasing its permit.
    fn mark_dirty(&mut self) -> Result<(), String> {
        Err("Source Shared owner has no typed physical projection".to_string())
    }
    /// Publish the current JIT projection and release the physical permit
    /// while a condition wait parks. Implementations must retain the same
    /// logical guard lease so aliases and publication epochs remain intact.
    fn wait_suspend(
        &mut self,
        _value: MirRuntimeValue,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(
            Err("Source Shared owner does not support canonical guard wait handoff".to_string()),
            None,
        )
    }

    /// Reacquire the same physical permit after a wait and refresh the JIT
    /// projection. `Ok(None)` means cancellation released the logical lease
    /// without reacquiring it.
    fn wait_resume(
        &mut self,
        _cancelled: &mut dyn FnMut() -> bool,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<
        Option<MirRuntimeValue>,
    > {
        crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(
            Err("Source Shared owner does not support canonical guard wait handoff".to_string()),
            None,
        )
    }

    /// Abort a wait whose source permit was reacquired but whose local JIT
    /// permit could not be reacquired. The logical guard is then closed
    /// without publishing a second epoch.
    fn wait_abort(&mut self) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(
            Err("Source Shared owner cannot abort a canonical guard wait".to_string()),
            None,
        )
    }

    /// Finish the guard after its final JIT alias closes. A cancelled wait can
    /// leave the physical lease released; that state must discard rather than
    /// stage a second publication.
    fn finish_value(&mut self, value: MirRuntimeValue) -> Result<(), String> {
        self.stage_value(value)
    }
    /// Discard unpublished staging while retaining this physical permit.
    /// Used to roll back every participant when a canonical multi-owner stage
    /// fails before the transaction publishes any owner.
    fn discard_staged(&mut self) -> Result<(), String> {
        Err("Source Shared owner cannot discard staged values".to_string())
    }
}
pub(crate) struct SourceSharedInteropPhysicalLease {
    guard: RefCell<Box<dyn SourceSharedInteropGuard>>,
    completion_delivery: Arc<Mutex<Option<SourceSharedInteropCompletionDelivery>>>,
}

impl SourceSharedInteropPhysicalLease {
    pub(crate) fn with_read(
        &self,
        callback: &mut dyn FnMut(&MirRuntimeValue) -> Result<(), String>,
    ) -> Result<(), String> {
        self.guard.borrow_mut().with_read(callback)
    }

    pub(crate) fn with_edit(
        &self,
        callback: &mut dyn FnMut(&mut MirRuntimeValue) -> Result<(), String>,
    ) -> Result<(), String> {
        self.guard.borrow_mut().with_edit(callback)
    }

    pub(crate) fn read_value(&self) -> Result<MirRuntimeValue, String> {
        self.guard.borrow_mut().read_value()
    }

    pub(crate) fn stage_value(&self, value: MirRuntimeValue) -> Result<(), String> {
        self.guard.borrow_mut().stage_value(value)
    }

    pub(crate) fn revision(&self) -> Result<u64, String> {
        self.guard.borrow().revision()
    }

    pub(crate) fn held(&self) -> bool {
        self.guard.borrow().held()
    }

    pub(crate) fn wait_suspend(&self, value: MirRuntimeValue) -> Result<(), String> {
        let outcome = self.guard.borrow_mut().wait_suspend(value);
        finish_physical_operation(&self.completion_delivery, outcome)
    }

    pub(crate) fn wait_resume(
        &self,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Option<MirRuntimeValue>, String> {
        let outcome = self.guard.borrow_mut().wait_resume(cancelled);
        finish_physical_operation(&self.completion_delivery, outcome)
    }

    pub(crate) fn wait_abort(&self) -> Result<(), String> {
        let outcome = self.guard.borrow_mut().wait_abort();
        finish_physical_operation(&self.completion_delivery, outcome)
    }

    pub(crate) fn finish_value(&self, value: MirRuntimeValue) -> Result<(), String> {
        self.guard.borrow_mut().finish_value(value)
    }

    pub(crate) fn discard_staged(&self) -> Result<(), String> {
        self.guard.borrow_mut().discard_staged()
    }
    pub(crate) fn release(
        &self,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        self.guard.borrow_mut().release()
    }

    pub(crate) fn release_during_drop(&self) {
        self.guard.borrow_mut().release_during_drop();
    }
}

impl Drop for SourceSharedInteropPhysicalLease {
    fn drop(&mut self) {
        self.guard.get_mut().release_during_drop();
    }
}

struct SourceSharedInteropCanonicalPermit {
    lease: Rc<SourceSharedInteropPhysicalLease>,
    owner_alias: Option<SourceSharedInteropOwnerAlias>,
    editable: bool,
}

impl crate::Memory::shared_protocol::JetSharedCanonicalPermit
    for SourceSharedInteropCanonicalPermit
{
    fn editable(&self) -> bool {
        self.editable
    }

    fn held(&self) -> bool {
        self.lease.held()
    }

    fn release(&self) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(Ok(()), None)
    }

    fn release_during_drop(&self) {
        // The paired non-Send physical guard lease controls release.
    }

    fn reacquire(&self, _cancelled: &mut dyn FnMut() -> bool) -> bool {
        self.held()
    }

    fn stage_value(&self, value: Box<dyn Any>) -> Result<(), String> {
        let value = value.downcast::<MirRuntimeValue>().map_err(|_| {
            "Source Shared physical guard requires a checked MIR payload".to_string()
        })?;
        self.lease.stage_value(*value)
    }

    fn discard_staged(&self) -> Result<(), String> {
        self.lease.discard_staged()
    }

    fn into_any(self: Arc<Self>) -> Arc<dyn Any> {
        self
    }
}
pub struct SourceSharedInteropGuardState {
    pub(crate) state: Arc<crate::Memory::shared_protocol::JetSharedGuardState>,
    pub(crate) lease: Rc<SourceSharedInteropPhysicalLease>,
    pub(crate) physical_identity: usize,
    pub(crate) protocol_order_key: usize,
}

impl SourceSharedInteropGuardState {
    /// Clone this exact invocation-local logical view without acquiring a
    /// permit or changing its path or edit capability.
    pub fn entry_view(&self) -> Result<Self, String> {
        let state = crate::Memory::shared_protocol::jet_shared_guard_clone(
            &self.state,
            self.editable(),
        )
        .map_err(str::to_owned)?;
        Ok(Self {
            state,
            lease: self.lease.clone(),
            physical_identity: self.physical_identity,
            protocol_order_key: self.protocol_order_key,
        })
    }
    pub(crate) fn release(
        self,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        let Self {
            state,
            lease,
            physical_identity: _,
            protocol_order_key: _,
        } = self;
        drop(state);
        if Rc::strong_count(&lease) == 1 {
            lease.release()
        } else {
            crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(Ok(()), None)
        }
    }
    pub(crate) fn release_and_deliver(self) -> Result<(), String> {
        let completion_delivery = Arc::clone(&self.lease.completion_delivery);
        finish_physical_operation(&completion_delivery, self.release())
    }

    fn acquire(
        interop: &SourceSharedInterop,
        editable: bool,
        owner_alias: Option<SourceSharedInteropOwnerAlias>,
    ) -> Result<Self, String> {
        let physical_identity = interop
            .physical_identity()
            .ok_or_else(|| "Source Shared root has no physical owner identity".to_string())?;
        let protocol_order_key = interop
            .protocol_order_key()
            .ok_or_else(|| "Source Shared root has no physical protocol-order key".to_string())?;
        if owner_alias
            .as_ref()
            .is_some_and(|alias| alias.owner_identity != interop.identity())
        {
            return Err("Source Shared guard alias belongs to a different physical owner".to_string());
        }
        let lease = interop.acquire_physical_lease(editable)?;
        let permit: Arc<dyn crate::Memory::shared_protocol::JetSharedCanonicalPermit> =
            Arc::new(SourceSharedInteropCanonicalPermit {
                lease: lease.clone(),
                owner_alias,
                editable,
            });
        let state = crate::Memory::shared_protocol::jet_shared_guard_state_from_permit(
            permit, editable,
        )
        .map_err(str::to_owned)?;
        Ok(Self {
            state,
            lease,
            physical_identity,
            protocol_order_key,
        })
    }

    pub fn path(&self) -> &[i64] {
        self.state.path()
    }

    pub fn editable(&self) -> bool {
        self.state.editable()
    }

    pub fn held(&self) -> bool {
        self.state.held()
    }
    pub fn physical_identity(&self) -> usize {
        self.physical_identity
    }

    pub fn protocol_order_key(&self) -> usize {
        self.protocol_order_key
    }

    pub fn clone_guard(&self, editable: bool) -> Result<Self, String> {
        let state = crate::Memory::shared_protocol::jet_shared_guard_clone(
            &self.state,
            editable,
        )
        .map_err(str::to_owned)?;
        Ok(Self {
            state,
            lease: self.lease.clone(),
            physical_identity: self.physical_identity,
            protocol_order_key: self.protocol_order_key,
        })
    }

    pub(crate) fn canonical_state(
        &self,
    ) -> &Arc<crate::Memory::shared_protocol::JetSharedGuardState> {
        &self.state
    }

    pub(crate) fn wait_handoff(&self) -> SourceSharedInteropWaitHandoff {
        SourceSharedInteropWaitHandoff {
            lease: self.lease.clone(),
            error: None,
        }
    }

    pub fn map_path(
        self,
        fields: &[i64],
        editable: bool,
    ) -> Result<Self, String> {
        let state = crate::Memory::shared_protocol::jet_shared_guard_map_path(
            &self.state,
            fields,
            editable,
        )
        .map_err(str::to_owned)?;
        Ok(Self {
            state,
            lease: self.lease,
            physical_identity: self.physical_identity,
            protocol_order_key: self.protocol_order_key,
        })
    }

    pub fn split_path(
        self,
        first_fields: &[i64],
        second_fields: &[i64],
        editable: bool,
    ) -> Result<(Self, Self), String> {
        let (first, second) =
            crate::Memory::shared_protocol::jet_shared_guard_split_path(
                &self.state,
                first_fields,
                second_fields,
                editable,
            )
            .map_err(str::to_owned)?;
        Ok((
            Self {
                state: first,
                lease: self.lease.clone(),
                physical_identity: self.physical_identity,
                protocol_order_key: self.protocol_order_key,
            },
            Self {
                state: second,
                lease: self.lease,
                physical_identity: self.physical_identity,
                protocol_order_key: self.protocol_order_key,
            },
        ))
    }

    pub fn with_read<R>(
        &self,
        callback: impl FnOnce(&[i64], &MirRuntimeValue) -> Result<R, String>,
    ) -> Result<R, String> {
        if !self.held() {
            return Err(crate::Memory::shared_protocol::JET_SHARED_GUARD_INVALID.to_string());
        }
        let path = self.path();
        let mut callback = Some(callback);
        let mut result = None;
        self.lease.with_read(&mut |value| {
            let callback = callback
                .take()
                .ok_or_else(|| "Source Shared read callback was invoked more than once".to_string())?;
            result = Some(callback(path, value)?);
            Ok(())
        })?;
        result.ok_or_else(|| "Source Shared read callback did not run".to_string())
    }

    pub fn with_edit<R>(
        &self,
        callback: impl FnOnce(&[i64], &mut MirRuntimeValue) -> Result<R, String>,
    ) -> Result<R, String> {
        crate::Memory::shared_protocol::jet_shared_guard_require_edit(&self.state)
            .map_err(str::to_owned)?;
        let path = self.path();
        let mut callback = Some(callback);
        let mut result = None;
        self.lease.with_edit(&mut |value| {
            let callback = callback
                .take()
                .ok_or_else(|| "Source Shared edit callback was invoked more than once".to_string())?;
            result = Some(callback(path, value)?);
            Ok(())
        })?;
        result.ok_or_else(|| "Source Shared edit callback did not run".to_string())
    }

    pub fn read_value(&self) -> Result<MirRuntimeValue, String> {
        self.with_read(|_, value| Ok(value.clone()))
    }

    pub fn stage_value(&self, value: MirRuntimeValue) -> Result<(), String> {
        crate::Memory::shared_protocol::jet_shared_guard_require_edit(&self.state)
            .map_err(str::to_owned)?;
        self.lease.stage_value(value)
    }

    pub fn revision(&self) -> Result<u64, String> {
        if !self.held() {
            return Err(crate::Memory::shared_protocol::JET_SHARED_GUARD_INVALID.to_string());
        }
        self.lease.revision()
    }

    pub fn wait_suspend(&self, value: MirRuntimeValue) -> Result<(), String> {
        crate::Memory::shared_protocol::jet_shared_guard_require_edit(&self.state)
            .map_err(str::to_owned)?;
        self.lease.wait_suspend(value)
    }

    pub fn wait_resume(
        &self,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> Result<Option<MirRuntimeValue>, String> {
        self.lease.wait_resume(cancelled)
    }

    pub fn wait_abort(&self) -> Result<(), String> {
        self.lease.wait_abort()
    }

    pub fn finish_value(&self, value: MirRuntimeValue) -> Result<(), String> {
        self.lease.finish_value(value)
    }

    pub fn discard_staged(&self) -> Result<(), String> {
        crate::Memory::shared_protocol::jet_shared_guard_require_edit(&self.state)
            .map_err(str::to_owned)?;
        self.lease.discard_staged()
    }
}
pub(crate) struct SourceSharedInteropWaitHandoff {
    lease: Rc<SourceSharedInteropPhysicalLease>,
    error: Option<String>,
}

impl SourceSharedInteropWaitHandoff {
    pub(crate) fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    fn record_error(&mut self, error: String) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }
}

impl crate::Memory::shared_protocol::JetSharedWaitHandoff
    for SourceSharedInteropWaitHandoff
{
    fn suspend(&mut self) -> Result<(), ()> {
        let value = match self.lease.read_value() {
            Ok(value) => value,
            Err(error) => {
                self.record_error(error);
                return Err(());
            }
        };
        match self.lease.wait_suspend(value) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.record_error(error);
                Err(())
            }
        }
    }

    fn resume(&mut self, cancelled: bool) -> Result<bool, ()> {
        let mut should_cancel = || cancelled;
        match self.lease.wait_resume(&mut should_cancel) {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(error) => {
                self.record_error(error);
                Err(())
            }
        }
    }

    fn abort(&mut self) -> Result<(), ()> {
        match self.lease.wait_abort() {
            Ok(()) => Ok(()),
            Err(error) => {
                self.record_error(error);
                Err(())
            }
        }
    }
}


impl SourceSharedInterop {
    pub(crate) fn acquire_physical_lease(
        &self,
        editable: bool,
    ) -> Result<Rc<SourceSharedInteropPhysicalLease>, String> {
        let guard = self.acquire_guard(editable)?;
        Ok(Rc::new(SourceSharedInteropPhysicalLease {
            guard: RefCell::new(guard),
            completion_delivery: Arc::clone(&self.completion_delivery),
        }))
    }
    /// Acquire the physical Source owner guard once and attach canonical
    /// SharedProtocol path state for access-time map/split aliases.
    pub fn acquire_guard_state(
        &self,
        editable: bool,
    ) -> Result<SourceSharedInteropGuardState, String> {
        SourceSharedInteropGuardState::acquire(self, editable, None)
    }

    /// Transfer a guard-prepared logical owner alias into the single canonical
    /// permit; map/split/clone guards retain this same token until final close.
    pub fn acquire_guard_state_with_owner_alias(
        &self,
        editable: bool,
        owner_alias: SourceSharedInteropOwnerAlias,
    ) -> Result<SourceSharedInteropGuardState, String> {
        SourceSharedInteropGuardState::acquire(self, editable, Some(owner_alias))
    }
}

struct InMemoryState {
    value: Mutex<MirRuntimeValue>,
    revision: AtomicU64,
    protocol: Arc<crate::Memory::shared_protocol::JetSharedProtocol>,
}

impl InMemoryState {
    fn acquire(
        &self,
        editable: bool,
    ) -> Result<Arc<crate::Memory::shared_protocol::JetSharedPermit>, String> {
        crate::Memory::shared_protocol::jet_shared_acquire(
            &self.protocol,
            editable,
            || false,
        )
        .ok_or_else(|| "Source Shared interop permit acquisition was cancelled".to_string())
    }

    fn publish(&self, value: MirRuntimeValue) -> Result<(), String> {
        let mut current = self
            .value
            .lock()
            .map_err(|_| "Source Shared interop value lock is poisoned".to_string())?;
        let next = self
            .revision
            .load(Ordering::Acquire)
            .checked_add(1)
            .ok_or_else(|| "Source Shared interop revision exhausted".to_string())?;
        *current = value;
        self.revision.store(next, Ordering::Release);
        Ok(())
    }
}

struct InMemoryGuard {
    state: Arc<InMemoryState>,
    value: MirRuntimeValue,
    permit: Arc<crate::Memory::shared_protocol::JetSharedPermit>,
    editable: bool,
    suspended: bool,
    dirty: bool,
}

impl SourceSharedInteropGuard for InMemoryGuard {
    fn read_value(&mut self) -> Result<MirRuntimeValue, String> {
        if self.suspended || !self.permit.held() {
            return Err("Source Shared guard permit is not held".to_string());
        }
        Ok(self.value.clone())
    }
    fn revision(&self) -> Result<u64, String> {
        if self.suspended || !self.permit.held() {
            return Err("Source Shared guard permit is not held".to_string());
        }
        Ok(self.state.revision.load(Ordering::Acquire))
    }
    fn held(&self) -> bool {
        !self.suspended && self.permit.held()
    }
    fn release(
        &mut self,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        let commit_result = if self.permit.held() && self.dirty {
            self.state.publish(self.value.clone())
        } else {
            Ok(())
        };
        self.dirty = false;
        self.suspended = false;
        let released =
            crate::Memory::shared_protocol::JetSharedCanonicalPermit::release(self.permit.as_ref());
        let crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome {
            result: release_result,
            completion,
        } = released;
        crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(
            commit_result.and(release_result),
            completion,
        )
    }

    fn release_during_drop(&mut self) {
        if self.permit.held() && self.dirty {
            let _ = self.state.publish(self.value.clone());
        }
        self.dirty = false;
        self.suspended = false;
        crate::Memory::shared_protocol::JetSharedCanonicalPermit::release_during_drop(
            self.permit.as_ref(),
        );
    }

    fn stage_value(&mut self, value: MirRuntimeValue) -> Result<(), String> {
        if !self.editable {
            return Err("read-only Shared guard cannot stage a value".to_string());
        }
        if self.suspended || !self.permit.held() {
            return Err("Source Shared guard permit is not held".to_string());
        }
        self.value = value;
        self.dirty = true;
        Ok(())
    }

    fn wait_suspend(
        &mut self,
        value: MirRuntimeValue,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        if !self.editable {
            return crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(
                Err("read-only Shared guard cannot wait".to_string()),
                None,
            );
        }
        if self.suspended || !self.permit.held() {
            return crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(
                Err("Source Shared guard permit is not active".to_string()),
                None,
            );
        }
        self.value = value;
        if self.dirty {
            if let Err(error) = self.state.publish(self.value.clone()) {
                return crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(
                    Err(error),
                    None,
                );
            }
            self.dirty = false;
        }
        let released =
            crate::Memory::shared_protocol::JetSharedCanonicalPermit::release(self.permit.as_ref());
        self.suspended = true;
        released
    }

    fn wait_resume(
        &mut self,
        cancelled: &mut dyn FnMut() -> bool,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<
        Option<MirRuntimeValue>,
    > {
        if !self.suspended || self.permit.held() {
            return crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(
                Err("Source Shared guard wait lease is not suspended".to_string()),
                None,
            );
        }
        if !self.permit.reacquire(cancelled) {
            return crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(
                Ok(None),
                None,
            );
        }
        self.suspended = false;
        let result = self
            .state
            .value
            .lock()
            .map_err(|_| "Source Shared interop value lock is poisoned".to_string())
            .map(|value| {
                self.value = value.clone();
                Some(self.value.clone())
            });
        crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(result, None)
    }

    fn wait_abort(
        &mut self,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        self.suspended = false;
        self.dirty = false;
        crate::Memory::shared_protocol::JetSharedCanonicalPermit::release(self.permit.as_ref())
    }

    fn finish_value(&mut self, value: MirRuntimeValue) -> Result<(), String> {
        if !self.permit.held() {
            self.suspended = false;
            self.dirty = false;
            return Ok(());
        }
        self.stage_value(value)
    }

    fn discard_staged(&mut self) -> Result<(), String> {
        if self.suspended || !self.permit.held() {
            return Err("Source Shared guard permit is not held".to_string());
        }
        self.value = self
            .state
            .value
            .lock()
            .map_err(|_| "Source Shared interop value lock is poisoned".to_string())?
            .clone();
        self.dirty = false;
        Ok(())
    }
}

impl Drop for InMemoryGuard {
    fn drop(&mut self) {
        self.release_during_drop();
    }
}

struct InMemoryBackend {
    state: Arc<InMemoryState>,
}

impl InMemoryBackend {
    fn new(value: MirRuntimeValue) -> Self {
        Self {
            state: Arc::new(InMemoryState {
                value: Mutex::new(value),
                revision: AtomicU64::new(0),
                protocol: crate::Memory::shared_protocol::JetSharedProtocol::new(),
            }),
        }
    }
}

impl SourceSharedInteropBackend for InMemoryBackend {
    fn with_read(
        &self,
        callback: &mut dyn FnMut(&MirRuntimeValue) -> Result<(), String>,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        let result = (|| {
            let _permit = self.state.acquire(false)?;
            let value = self
                .state
                .value
                .lock()
                .map_err(|_| "Source Shared interop value lock is poisoned".to_string())?;
            callback(&value)
        })();
        crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(result, None)
    }

    fn with_read_revision(
        &self,
        callback: &mut dyn FnMut(&MirRuntimeValue, u64) -> Result<(), String>,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        let result = (|| {
            let _permit = self.state.acquire(false)?;
            let value = self
                .state
                .value
                .lock()
                .map_err(|_| "Source Shared interop value lock is poisoned".to_string())?;
            callback(&value, self.state.revision.load(Ordering::Acquire))
        })();
        crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(result, None)
    }

    fn with_edit(
        &self,
        callback: &mut dyn FnMut(&mut MirRuntimeValue) -> Result<(), String>,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        let result = (|| {
            let _permit = self.state.acquire(true)?;
            let mut value = self
                .state
                .value
                .lock()
                .map_err(|_| "Source Shared interop value lock is poisoned".to_string())?;
            let mut edited = value.clone();
            callback(&mut edited)?;
            drop(value);
            self.state.publish(edited)
        })();
        crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(result, None)
    }

    fn replace_if_revision(
        &self,
        expected_revision: u64,
        replacement: MirRuntimeValue,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<(bool, u64)> {
        let result = (|| {
            let _permit = self.state.acquire(true)?;
            let mut value = self
                .state
                .value
                .lock()
                .map_err(|_| "Source Shared interop value lock is poisoned".to_string())?;
            let current = self.state.revision.load(Ordering::Acquire);
            if current != expected_revision {
                return Ok((false, current));
            }
            let next = current
                .checked_add(1)
                .ok_or_else(|| "Source Shared interop revision exhausted".to_string())?;
            *value = replacement;
            self.state.revision.store(next, Ordering::Release);
            Ok((true, next))
        })();
        crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(result, None)
    }

    fn acquire_guard(
        &self,
        editable: bool,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<
        Box<dyn SourceSharedInteropGuard>,
    > {
        let result = (|| {
            let permit = self.state.acquire(editable)?;
            let value = self
                .state
                .value
                .lock()
                .map_err(|_| "Source Shared interop value lock is poisoned".to_string())?
                .clone();
            Ok(Box::new(InMemoryGuard {
                state: self.state.clone(),
                value,
                permit,
                editable,
                suspended: false,
                dirty: editable,
            }) as Box<dyn SourceSharedInteropGuard>)
        })();
        crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(result, None)
    }
}

type GuardFactory = dyn Fn(
        bool,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<
        Box<dyn SourceSharedInteropGuard>,
    > + Send
    + Sync
    + 'static;

struct CallbackBackend<Read, Edit, Capture, Replace> {
    read: Read,
    edit: Edit,
    capture: Capture,
    replace: Replace,
    guard: Option<Arc<GuardFactory>>,
}

impl<Read, Edit, Capture, Replace> SourceSharedInteropBackend
    for CallbackBackend<Read, Edit, Capture, Replace>
where
    Read: Fn(
            &mut dyn FnMut(&MirRuntimeValue) -> Result<(), String>,
        ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()>
        + Send
        + Sync
        + 'static,
    Edit: Fn(
            &mut dyn FnMut(&mut MirRuntimeValue) -> Result<(), String>,
        ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()>
        + Send
        + Sync
        + 'static,
    Capture: Fn(
            &mut dyn FnMut(&MirRuntimeValue, u64) -> Result<(), String>,
        ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()>
        + Send
        + Sync
        + 'static,
    Replace: Fn(
            u64,
            MirRuntimeValue,
        ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<(bool, u64)>
        + Send
        + Sync
        + 'static,
{
    fn with_read(
        &self,
        callback: &mut dyn FnMut(&MirRuntimeValue) -> Result<(), String>,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        (self.read)(callback)
    }

    fn with_read_revision(
        &self,
        callback: &mut dyn FnMut(&MirRuntimeValue, u64) -> Result<(), String>,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        (self.capture)(callback)
    }

    fn with_edit(
        &self,
        callback: &mut dyn FnMut(&mut MirRuntimeValue) -> Result<(), String>,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        (self.edit)(callback)
    }

    fn replace_if_revision(
        &self,
        expected_revision: u64,
        replacement: MirRuntimeValue,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<(bool, u64)> {
        (self.replace)(expected_revision, replacement)
    }

    fn acquire_guard(
        &self,
        editable: bool,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<
        Box<dyn SourceSharedInteropGuard>,
    > {
        match self.guard.as_ref() {
            Some(factory) => factory(editable),
            None => crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(
                Err("Source Shared owner has no guard factory".to_string()),
                None,
            ),
        }
    }
}

/// One checked, identity-preserving physical Shared root.
///
/// Cloning this value aliases the same backend and therefore the same physical
/// root.  `identity` is only an in-process equality key for sidecar lookup; it
/// is never emitted into MIR or used as a language-level handle.
#[derive(Clone)]
pub struct SourceSharedInterop {
    type_id: u64,
    backend: Arc<dyn SourceSharedInteropBackend>,
    owner_identity: Option<usize>,
    protocol_order_key: Option<usize>,
    owner_lifecycle: Option<Arc<OwnerLifecycle>>,
    owner_alias_lifecycle: Option<Arc<OwnerAliasLifecycle>>,
    owner_alias_lease: Option<Arc<SourceSharedInteropOwnerAlias>>,
    typed_owner: Option<Arc<dyn Any + Send + Sync>>,
    resource_lease_owner: Option<Arc<dyn Any + Send + Sync>>,
    resident_root: bool,
    completion_delivery: Arc<Mutex<Option<SourceSharedInteropCompletionDelivery>>>,
    payload_finalizer_installer: Option<SourceSharedInteropPayloadFinalizerInstaller>,
}

#[derive(Clone)]
pub struct SourceSharedInteropWeak {
    type_id: u64,
    identity: usize,
    physical_identity: Option<usize>,
    protocol_order_key: Option<usize>,
    owner_alias_count: Arc<dyn Fn() -> Result<usize, String> + Send + Sync>,
    owner: Arc<dyn SourceSharedInteropWeakOwner>,
    typed_owner: Option<Arc<dyn Any + Send + Sync>>,
    resource_lease_owner: Option<Arc<dyn Any + Send + Sync>>,
}


impl SourceSharedInterop {
    /// Build a root from owner-supplied permit callbacks. `capture` must
    /// report the value and physical revision from the same read permit;
    /// `replace` must compare and publish under one exclusive permit.
    pub fn from_callbacks<Read, Edit, Capture, Replace>(
        type_id: u64,
        read: Read,
        edit: Edit,
        capture: Capture,
        replace: Replace,
    ) -> Self
    where
        Read: Fn(
                &mut dyn FnMut(&MirRuntimeValue) -> Result<(), String>,
            ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()>
            + Send
            + Sync
            + 'static,
        Edit: Fn(
                &mut dyn FnMut(&mut MirRuntimeValue) -> Result<(), String>,
            ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()>
            + Send
            + Sync
            + 'static,
        Capture: Fn(
                &mut dyn FnMut(&MirRuntimeValue, u64) -> Result<(), String>,
            ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()>
            + Send
            + Sync
            + 'static,
        Replace: Fn(u64, MirRuntimeValue) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<(bool, u64)>
            + Send
            + Sync
            + 'static,
    {
        Self {
            type_id,
            backend: Arc::new(CallbackBackend {
                read,
                edit,
                capture,
                replace,
                guard: None,
            }),
            owner_identity: None,
            protocol_order_key: None,
            owner_lifecycle: None,
            owner_alias_lifecycle: None,
            owner_alias_lease: None,
            typed_owner: None,
            resource_lease_owner: None,
            resident_root: false,
            completion_delivery: Arc::new(Mutex::new(None)),
            payload_finalizer_installer: None,
        }
    }
    /// Build a root with the owner's physical non-Send guard and its atomic
    /// revisioned capture/conditional-replacement operations.
    pub fn from_callbacks_with_guard<Read, Edit, Guard, Capture, Replace>(
        type_id: u64,
        read: Read,
        edit: Edit,
        guard: Guard,
        capture: Capture,
        replace: Replace,
    ) -> Self
    where
        Read: Fn(
                &mut dyn FnMut(&MirRuntimeValue) -> Result<(), String>,
            ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()>
            + Send
            + Sync
            + 'static,
        Edit: Fn(
                &mut dyn FnMut(&mut MirRuntimeValue) -> Result<(), String>,
            ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()>
            + Send
            + Sync
            + 'static,
        Guard: Fn(bool) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<Box<dyn SourceSharedInteropGuard>>
            + Send
            + Sync
            + 'static,
        Capture: Fn(
                &mut dyn FnMut(&MirRuntimeValue, u64) -> Result<(), String>,
            ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()>
            + Send
            + Sync
            + 'static,
        Replace: Fn(u64, MirRuntimeValue) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<(bool, u64)>
            + Send
            + Sync
            + 'static,
    {
        Self {
            type_id,
            backend: Arc::new(CallbackBackend {
                read,
                edit,
                capture,
                replace,
                guard: Some(Arc::new(guard)),
            }),
            owner_identity: None,
            protocol_order_key: None,
            owner_lifecycle: None,
            owner_alias_lifecycle: None,
            owner_alias_lease: None,
            typed_owner: None,
            resource_lease_owner: None,
            resident_root: false,
            completion_delivery: Arc::new(Mutex::new(None)),
            payload_finalizer_installer: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_value(type_id: u64, value: MirRuntimeValue) -> Self {
        Self {
            type_id,
            backend: Arc::new(InMemoryBackend::new(value)),
            owner_identity: None,
            protocol_order_key: None,
            owner_lifecycle: None,
            owner_alias_lifecycle: None,
            owner_alias_lease: None,
            typed_owner: None,
            resource_lease_owner: None,
            resident_root: false,
            completion_delivery: Arc::new(Mutex::new(None)),
            payload_finalizer_installer: None,
        }
    }
    /// Bind this callback carrier to the physical owner's stable identity.
    /// Aliases represented by separate callback sets must supply the same key.
    pub fn with_owner_identity(mut self, identity: usize) -> Self {
        self.owner_identity = Some(identity);
        self
    }

    /// Bind the canonical ordering identity of the physical Shared permit.
    pub fn with_protocol_order_key(mut self, key: usize) -> Self {
        self.protocol_order_key = Some(key);
        self
    }

    /// Bind the owner's weak-upgrade operation. Logical alias counting and
    /// retention are attached separately by `with_owner_alias_lifecycle`.
    pub fn with_owner_lifecycle<Downgrade>(mut self, downgrade: Downgrade) -> Self
    where
        Downgrade: Fn() -> Box<dyn SourceSharedInteropWeakOwner> + Send + Sync + 'static,
    {
        self.owner_lifecycle = Some(Arc::new(OwnerLifecycle {
            downgrade: Arc::new(downgrade),
        }));
        self
    }

    /// Bind logical owner-alias counting and explicit per-alias retention.
    /// Counts exclude interop metadata, typed-root pins, and guard leases.
    pub fn with_owner_alias_lifecycle<Count, Retain>(
        mut self,
        strong_count: Count,
        retain: Retain,
    ) -> Self
    where
        Count: Fn() -> Result<usize, String> + Send + Sync + 'static,
        Retain: Fn() -> Result<Box<dyn SourceSharedInteropOwnerAliasLease>, String>
            + Send
            + Sync
            + 'static,
    {
        self.owner_alias_lifecycle = Some(Arc::new(OwnerAliasLifecycle {
            strong_count: Arc::new(strong_count),
            retain: Arc::new(retain),
        }));
        self
    }

    /// Bind the one-shot physical-cell finalizer installer. Its implementation
    /// must take the candidate only when the same cell has accepted it.
    pub fn with_payload_finalizer_installer<Install>(mut self, installer: Install) -> Self
    where
        Install: Fn(&mut Option<SourceSharedInteropPayloadFinalizer>) -> Result<(), String>
            + Send
            + Sync
            + 'static,
    {
        self.payload_finalizer_installer = Some(Arc::new(installer));
        self
    }

    /// Install a Source-aware logical drop on this exact physical root.
    ///
    /// On error, `finalizer` remains the caller's exact pending candidate.
    /// The owner invokes it only after its last logical alias and active borrow
    /// are gone, outside its state, protocol, and permit locks.
    pub fn install_payload_finalizer(
        &self,
        finalizer: &mut Option<SourceSharedInteropPayloadFinalizer>,
    ) -> Result<(), String> {
        let Some(finalizer_installer) = &self.payload_finalizer_installer else {
            return Err("Source Shared owner has no physical payload finalizer installer".to_string());
        };
        let Some(binding) = finalizer.as_ref() else {
            return Err("Source Shared payload finalizer candidate is empty".to_string());
        };
        let delivery = binding.completion_delivery();
        let mut installed_delivery = self
            .completion_delivery
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let previous_delivery = installed_delivery.replace(delivery);
        if let Err(error) = finalizer_installer(finalizer) {
            *installed_delivery = previous_delivery;
            return Err(error);
        }
        Ok(())
    }

    /// Retain one semantic strong alias without cloning the physical root.
    pub fn retain_owner_alias(&self) -> Result<SourceSharedInteropOwnerAlias, String> {
        let lifecycle = self
            .owner_alias_lifecycle
            .as_ref()
            .ok_or_else(|| "Source Shared owner has no logical alias lifecycle".to_string())?;
        Ok(SourceSharedInteropOwnerAlias::new(
            self.identity(),
            (lifecycle.retain)()?,
        ))
    }

    /// Number of semantic strong aliases, excluding carrier/guard pins.
    pub fn owner_strong_count(&self) -> Result<Option<usize>, String> {
        self.owner_alias_lifecycle
            .as_ref()
            .map(|owner| (owner.strong_count)())
            .transpose()
    }

    pub fn downgrade_owner(&self) -> Option<SourceSharedInteropWeak> {
        let owner = self.owner_lifecycle.as_ref()?;
        let aliases = self.owner_alias_lifecycle.as_ref()?;
        Some(SourceSharedInteropWeak {
            type_id: self.type_id,
            identity: self.identity(),
            physical_identity: self.physical_identity(),
            protocol_order_key: self.protocol_order_key,
            owner_alias_count: aliases.strong_count.clone(),
            owner: Arc::from((owner.downgrade)()),
            typed_owner: None,
            resource_lease_owner: None,
        })
    }

    pub fn protocol_order_key(&self) -> Option<usize> {
        self.protocol_order_key
    }

    pub fn physical_identity(&self) -> Option<usize> {
        self.owner_identity
    }
    /// Consume this carrier's exact logical owner alias and preserve any
    /// completion produced when it retires the physical payload.
    pub fn release_owner_alias(
        &self,
    ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
        match &self.owner_alias_lease {
            Some(owner_alias) => owner_alias.release(),
            None => crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(
                Err("Source Shared carrier has no logical owner alias lease".to_string()),
                None,
            ),
        }
    }

    pub fn has_owner_lifecycle(&self) -> bool {
        self.owner_lifecycle.is_some() && self.owner_alias_lifecycle.is_some()
    }

    /// Retain an owner-native typed handle without serializing or projecting
    /// its payload. Clones of this carrier share the same attachment.
    pub fn with_typed_owner<T: Any + Send + Sync>(mut self, owner: T) -> Self {
        self.typed_owner = Some(Arc::new(owner));
        self
    }
    /// Keep one owner-issued logical alias alive across clones of this
    /// physical carrier. This is separate from typed/resource metadata pins.
    pub fn with_owner_alias_lease(
        mut self,
        owner_alias: SourceSharedInteropOwnerAlias,
    ) -> Result<Self, String> {
        if owner_alias.owner_identity != self.identity() {
            return Err("Source Shared alias lease belongs to a different physical owner".to_string());
        }
        self.owner_alias_lease = Some(Arc::new(owner_alias));
        Ok(self)
    }
    pub(crate) fn without_owner_alias_lease(&self) -> Self {
        let mut interop = self.clone();
        interop.owner_alias_lease = None;
        interop
    }


    pub fn owner_alias_token_id(&self) -> Option<i64> {
        self.owner_alias_lease
            .as_ref()
            .map(|owner_alias| owner_alias.token_id())
    }


    /// Recover the owner-native handle when this carrier originated from the
    /// same typed Shared implementation.
    pub fn downcast_typed_owner<T: Any + Send + Sync + Clone>(&self) -> Option<T> {
        self.typed_owner.as_ref()?.downcast_ref::<T>().cloned()
    }

    /// Attach a non-counting Source resource lease without replacing the
    /// owner-native typed handle stored in `typed_owner`.
    pub fn with_resource_lease_owner<T: Any + Send + Sync>(mut self, lease: T) -> Self {
        self.resource_lease_owner = Some(Arc::new(lease));
        self
    }

    pub fn downcast_resource_lease_owner<T: Any + Send + Sync + Clone>(&self) -> Option<T> {
        self.resource_lease_owner
            .as_ref()?
            .downcast_ref::<T>()
            .cloned()
    }

    pub(crate) fn mark_resident_root(&mut self) {
        self.resident_root = true;
    }

    pub(crate) fn is_resident_root(&self) -> bool {
        self.resident_root
    }

    /// Checked outer `Shared<T>` type identity retained by the physical root.
    pub fn type_id(&self) -> u64 {
        self.type_id
    }

    /// Stable identity for aliases of this physical root during one process
    /// lifetime.
    pub fn identity(&self) -> usize {
        self.owner_identity
            .unwrap_or_else(|| Arc::as_ptr(&self.backend) as *const () as usize)
    }

    /// Project this exact root into the private MIR carrier.
    pub fn as_native_owned(&self) -> MirRuntimeValue {
        MirRuntimeValue::NativeOwned(MirNativeOwned::new(self.clone()))
    }

    /// Recover a physical Shared root from a private MIR carrier.
    pub fn from_native_owned(value: &MirRuntimeValue) -> Result<Self, String> {
        let MirRuntimeValue::NativeOwned(root) = value else {
            return Err("expected a native-owned Shared root".to_string());
        };
        root.downcast_ref::<Self>()
            .cloned()
            .ok_or_else(|| "native-owned value is not a SourceSharedInterop root".to_string())
    }

    pub(crate) fn finish_physical_operation<T>(
        &self,
        outcome: crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<T>,
    ) -> Result<T, String> {
        finish_physical_operation(&self.completion_delivery, outcome)
    }

    /// Hold the canonical read permit while invoking `callback`.
    pub fn with_read<R>(
        &self,
        callback: impl FnOnce(&MirRuntimeValue) -> Result<R, String>,
    ) -> Result<R, String> {
        let mut callback = Some(callback);
        let mut result = None;
        let outcome = self.backend.with_read(&mut |value| {
            let callback = callback
                .take()
                .ok_or_else(|| "Shared read callback was invoked more than once".to_string())?;
            result = Some(callback(value)?);
            Ok(())
        });
        self.finish_physical_operation(outcome)?;
        result.ok_or_else(|| "Shared read backend did not invoke its callback".to_string())
    }
    /// Capture the typed payload and owner revision under one canonical read
    /// permit. The returned revision belongs to this exact physical root.
    pub fn capture_with_revision(&self) -> Result<(MirRuntimeValue, u64), String> {
        let mut captured = None;
        let outcome = self.backend.with_read_revision(&mut |value, revision| {
            if captured.is_some() {
                return Err("Shared capture callback was invoked more than once".to_string());
            }
            captured = Some((value.clone(), revision));
            Ok(())
        });
        self.finish_physical_operation(outcome)?;
        captured.ok_or_else(|| "Shared capture backend did not invoke its callback".to_string())
    }
    /// Atomically replace the root only when its physical revision still
    /// matches the revision captured from this same root.
    pub fn replace_if_revision(
        &self,
        expected_revision: u64,
        replacement: MirRuntimeValue,
    ) -> Result<(bool, u64), String> {
        self.finish_physical_operation(
            self.backend
                .replace_if_revision(expected_revision, replacement),
        )
    }


    /// Hold the canonical edit permit while invoking `callback`; commit is
    /// owner-defined and occurs only when the callback returns `Ok`.
    pub fn with_edit<R>(
        &self,
        callback: impl FnOnce(&mut MirRuntimeValue) -> Result<R, String>,
    ) -> Result<R, String> {
        let mut callback = Some(callback);
        let mut result = None;
        let outcome = self.backend.with_edit(&mut |value| {
            let callback = callback
                .take()
                .ok_or_else(|| "Shared edit callback was invoked more than once".to_string())?;
            result = Some(callback(value)?);
            Ok(())
        });
        self.finish_physical_operation(outcome)?;
        result.ok_or_else(|| "Shared edit backend did not invoke its callback".to_string())
    }
    /// Acquire the owner's canonical guard permit and keep it alive until the
    /// returned non-Send object is dropped.
    pub fn acquire_guard(
        &self,
        editable: bool,
    ) -> Result<Box<dyn SourceSharedInteropGuard>, String> {
        self.finish_physical_operation(self.backend.acquire_guard(editable))
    }
}
impl SourceSharedInteropWeak {
    /// Checked outer `Shared<T>` identity without reserving a strong alias.
    pub fn type_id(&self) -> u64 {
        self.type_id
    }

    /// Stable carrier identity shared by physical aliases and their upgrades.
    pub fn identity(&self) -> usize {
        self.identity
    }

    /// Explicit identity of the physical Shared owner, when bound by its
    /// implementation rather than inferred from the callback backend.
    pub fn physical_identity(&self) -> Option<usize> {
        self.physical_identity
    }

    /// Attach a non-counting typed metadata pin, such as the lease which keeps
    /// this weak capability's Source resource arena live.
    pub fn with_typed_owner<T: Any + Send + Sync>(mut self, owner: T) -> Self {
        self.typed_owner = Some(Arc::new(owner));
        self
    }

    pub fn downcast_typed_owner<T: Any + Send + Sync + Clone>(&self) -> Option<T> {
        self.typed_owner.as_ref()?.downcast_ref::<T>().cloned()
    }

    /// Attach a non-counting Source resource lease without replacing the
    /// owner-native typed handle stored in `typed_owner`.
    pub fn with_resource_lease_owner<T: Any + Send + Sync>(mut self, lease: T) -> Self {
        self.resource_lease_owner = Some(Arc::new(lease));
        self
    }

    pub fn downcast_resource_lease_owner<T: Any + Send + Sync + Clone>(&self) -> Option<T> {
        self.resource_lease_owner
            .as_ref()?
            .downcast_ref::<T>()
            .cloned()
    }

    /// Atomically upgrade the physical owner and reserve the returned strong
    /// alias as one operation. Callers must adopt this token; do not retain a
    /// second time after extracting the root.
    pub fn upgrade(
        &self,
    ) -> Result<Option<SourceSharedInteropOwnerAliasUpgrade>, String> {
        let Some((owner, lease)) = self.owner.upgrade()? else {
            return Ok(None);
        };
        if owner.type_id() != self.type_id
            || owner.identity() != self.identity
            || owner.physical_identity() != self.physical_identity
            || owner.protocol_order_key() != self.protocol_order_key
        {
            return Err("Source Shared weak upgrade resolved a different physical owner".to_string());
        }
        let alias_count = owner
            .owner_strong_count()?
            .ok_or_else(|| "Source Shared weak upgrade lost its alias lifecycle".to_string())?;
        if alias_count == 0 {
            return Err("Source Shared weak upgrade did not reserve a live alias".to_string());
        }
        Ok(Some(SourceSharedInteropOwnerAliasUpgrade {
            interop: owner,
            owner_alias: SourceSharedInteropOwnerAlias::new(self.identity, lease),
        }))
    }
}


impl std::fmt::Debug for SourceSharedInterop {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SourceSharedInterop")
            .field("type_id", &self.type_id)
            .field("identity", &self.identity())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(value: i64) -> MirRuntimeValue {
        MirRuntimeValue::Struct {
            type_name: "State".to_string(),
            fields: vec![("value".to_string(), MirRuntimeValue::Int(value))],
        }
    }

    fn state_value(value: &MirRuntimeValue) -> Result<i64, String> {
        let MirRuntimeValue::Struct { type_name, fields } = value else {
            return Err("expected State record".to_string());
        };
        if type_name != "State" || fields.len() != 1 || fields[0].0 != "value" {
            return Err("State record shape changed".to_string());
        }
        let MirRuntimeValue::Int(value) = &fields[0].1 else {
            return Err("State.value is not an Int".to_string());
        };
        Ok(*value)
    }

    #[test]
    fn missing_payload_finalizer_installer_preserves_the_candidate() {
        let root = SourceSharedInterop::from_value(0x701, MirRuntimeValue::Int(1));
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let finalizer_calls = Arc::clone(&calls);
        let completion_delivery: SourceSharedInteropCompletionDelivery = Arc::new(|_| {});
        let mut finalizer: Option<SourceSharedInteropPayloadFinalizer> =
            Some(crate::Memory::shared_protocol::JetSharedPhysicalFinalizerBinding::new(
                Box::new(move |value| {
                    assert_eq!(value, MirRuntimeValue::String("retained".to_string()));
                    finalizer_calls.fetch_add(1, Ordering::SeqCst);
                    Box::new(())
                }),
                completion_delivery,
            ));

        let error = root
            .install_payload_finalizer(&mut finalizer)
            .expect_err("an owner without an installer must reject finalization");
        assert!(error.contains("no physical payload finalizer installer"));
        assert!(finalizer.is_some(), "installation failure must retain the exact candidate");
        finalizer
            .take()
            .expect("the original finalizer should remain available")
            .finish_and_deliver(MirRuntimeValue::String("retained".to_string()));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn source_shared_weak_upgrade_reserves_one_logical_alias() {
        struct TestAliasLease {
            token_id: i64,
            count: Arc<std::sync::atomic::AtomicUsize>,
        }

        impl SourceSharedInteropOwnerAliasLease for TestAliasLease {
            fn token_id(&self) -> i64 {
                self.token_id
            }

            fn release(
                self: Box<Self>,
            ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
                let token_id = self.token_id;
                drop(self);
                crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(
                    Ok(()),
                    Some(Box::new(token_id)),
                )
            }
        }

        impl Drop for TestAliasLease {
            fn drop(&mut self) {
                self.count.fetch_sub(1, Ordering::AcqRel);
            }
        }

        fn reserve_alias(
            count: Arc<std::sync::atomic::AtomicUsize>,
            token_id: i64,
        ) -> Result<Box<dyn SourceSharedInteropOwnerAliasLease>, String> {
            let mut current = count.load(Ordering::Acquire);
            loop {
                if current == 0 {
                    return Err("no logical Shared owner remains".to_string());
                }
                let next = current
                    .checked_add(1)
                    .ok_or_else(|| "logical Shared alias count exhausted".to_string())?;
                match count.compare_exchange_weak(
                    current,
                    next,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                ) {
                    Ok(_) => break,
                    Err(actual) => current = actual,
                }
            }
            Ok(Box::new(TestAliasLease { token_id, count }))
        }

        struct FixedSharedWeak {
            root: SourceSharedInterop,
            count: Arc<std::sync::atomic::AtomicUsize>,
            next_token: Arc<std::sync::atomic::AtomicU64>,
        }

        impl SourceSharedInteropWeakOwner for FixedSharedWeak {
            fn upgrade(
                &self,
            ) -> Result<
                Option<(
                    SourceSharedInterop,
                    Box<dyn SourceSharedInteropOwnerAliasLease>,
                )>,
                String,
            > {
                let mut current = self.count.load(Ordering::Acquire);
                loop {
                    if current == 0 {
                        return Ok(None);
                    }
                    let next = current
                        .checked_add(1)
                        .ok_or_else(|| "logical Shared alias count exhausted".to_string())?;
                    match self.count.compare_exchange_weak(
                        current,
                        next,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    ) {
                        Ok(_) => break,
                        Err(actual) => current = actual,
                    }
                }
                let token_id = i64::try_from(self.next_token.fetch_add(1, Ordering::AcqRel))
                    .map_err(|_| "logical Shared alias token IDs exhausted".to_string())?;
                Ok(Some((
                    self.root.clone(),
                    Box::new(TestAliasLease {
                        token_id,
                        count: self.count.clone(),
                    }),
                )))
            }
        }

        let alias_count = Arc::new(std::sync::atomic::AtomicUsize::new(1));
        let next_token = Arc::new(std::sync::atomic::AtomicU64::new(1));
        let count_for_count = alias_count.clone();
        let count_for_retain = alias_count.clone();
        let ids_for_retain = next_token.clone();
        let root = SourceSharedInterop::from_value(38, state(1))
            .with_owner_identity(0x381)
            .with_protocol_order_key(0x382)
            .with_owner_alias_lifecycle(
                move || Ok(count_for_count.load(Ordering::Acquire)),
                move || {
                    let token_id = i64::try_from(ids_for_retain.fetch_add(1, Ordering::AcqRel))
                        .map_err(|_| "logical Shared alias token IDs exhausted".to_string())?;
                    reserve_alias(count_for_retain.clone(), token_id)
                },
            );
        let weak_root = root.clone();
        let weak_count = alias_count.clone();
        let weak_ids = next_token.clone();
        let owner = root.with_owner_lifecycle(move || {
            Box::new(FixedSharedWeak {
                root: weak_root.clone(),
                count: weak_count.clone(),
                next_token: weak_ids.clone(),
            })
        });
        let weak = owner
            .downgrade_owner()
            .expect("owner lifecycle should expose a weak carrier");
        assert_eq!(weak.identity(), owner.identity());
        assert_eq!(weak.physical_identity(), Some(0x381));
        assert_eq!(owner.owner_strong_count().expect("logical count"), Some(1));

        let metadata_clone = owner.clone();
        let first_alias = owner
            .retain_owner_alias()
            .expect("first semantic alias should retain");
        let second_alias = owner
            .retain_owner_alias()
            .expect("second semantic alias should retain");
        assert_ne!(first_alias.token_id(), second_alias.token_id());
        assert_eq!(owner.owner_strong_count().expect("logical count"), Some(3));
        drop(metadata_clone);
        assert_eq!(owner.owner_strong_count().expect("logical count"), Some(3));
        drop(first_alias);
        drop(second_alias);
        assert_eq!(owner.owner_strong_count().expect("logical count"), Some(1));

        let upgraded = weak
            .upgrade()
            .expect("weak upgrade should reserve atomically")
            .expect("the original logical owner is still live");
        let token_id = upgraded.owner_alias().token_id();
        let upgraded_carrier = upgraded
            .into_carrier()
            .expect("the carrier should adopt its reserved alias");
        let carrier_clone = upgraded_carrier.clone();
        assert_eq!(upgraded_carrier.owner_alias_token_id(), Some(token_id));
        assert_eq!(carrier_clone.owner_alias_token_id(), Some(token_id));
        assert_eq!(upgraded_carrier.identity(), owner.identity());
        assert_eq!(upgraded_carrier.physical_identity(), Some(0x381));
        assert_eq!(upgraded_carrier.protocol_order_key(), Some(0x382));
        assert_eq!(owner.owner_strong_count().expect("logical count"), Some(2));
        let outcome = upgraded_carrier.release_owner_alias();
        assert!(outcome.result.is_ok());
        assert_eq!(
            *outcome
                .completion
                .expect("explicit release returns its exact completion")
                .downcast::<i64>()
                .expect("completion preserves the alias token"),
            token_id,
        );
        assert_eq!(owner.owner_strong_count().expect("released count"), Some(1));
        drop(upgraded_carrier);
        drop(carrier_clone);
        assert_eq!(owner.owner_strong_count().expect("released carrier is inert"), Some(1));
        alias_count.store(0, Ordering::Release);
        assert!(
            weak.upgrade()
                .expect("zero logical owners should be a non-error expiration")
                .is_none(),
            "metadata pins must not resurrect a logical owner"
        );
    }

    #[test]
    fn source_shared_interop_path_aliases_share_one_owner_permit_and_publication() {
        let owner_marker = Arc::new(0_u8);
        let owner_identity = Arc::as_ptr(&owner_marker) as usize;
        let root = SourceSharedInterop::from_value(40, state(1))
            .with_owner_identity(owner_identity)
            .with_protocol_order_key(owner_identity);
        let root_guard = root
            .acquire_guard_state(true)
            .expect("canonical physical guard should acquire");
        let mapped = root_guard
            .map_path(&[4, 7], true)
            .expect("full path should map through canonical Shared policy");
        let (first, second) = mapped
            .split_path(&[10, 11], &[20, 21], true)
            .expect("disjoint full paths should split through canonical Shared policy");
        assert_eq!(first.path(), &[4, 7, 10, 11]);
        assert_eq!(second.path(), &[4, 7, 20, 21]);
        assert!(first.held());
        assert!(second.held());
        let read_alias = first
            .clone_guard(false)
            .expect("read alias should retain the same physical permit");

        first
            .with_edit(|path, value| {
                assert_eq!(path, &[4, 7, 10, 11]);
                *value = state(9);
                Ok(())
            })
            .expect("editable mapped guard should stage through its shared permit");
        assert_eq!(
            second
                .with_read(|path, value| {
                    assert_eq!(path, &[4, 7, 20, 21]);
                    state_value(value)
                })
                .expect("split alias should read while the same permit is held"),
            9
        );
        drop(first);
        assert!(second.held());
        assert_eq!(
            read_alias
                .with_read(|_, value| state_value(value))
                .expect("clone alias should retain the original owner lease"),
            9
        );
        drop(second);
        assert!(read_alias.held());
        drop(read_alias);

        let (committed, revision) = root
            .capture_with_revision()
            .expect("last alias close should publish through the physical owner");
        assert_eq!(state_value(&committed).expect("State should decode"), 9);
        assert_eq!(revision, 1);
    }

    #[test]
    fn source_shared_interop_discard_staged_aborts_without_revision() {
        let root = SourceSharedInterop::from_value(39, state(1));
        let mut guard = root
            .acquire_guard(true)
            .expect("editable physical guard should acquire");
        guard
            .stage_value(state(99))
            .expect("editable physical guard should stage");
        guard
            .discard_staged()
            .expect("physical guard should discard unpublished staging");
        drop(guard);

        let (value, revision) = root
            .capture_with_revision()
            .expect("discarded owner should remain readable");
        assert_eq!(state_value(&value).expect("State should decode"), 1);
        assert_eq!(revision, 0);
    }

    #[test]
    fn source_shared_interop_aliases_preserve_identity_and_guard_commit() {
        let root = SourceSharedInterop::from_value(41, state(1));
        let alias = root.clone();
        assert_eq!(root.identity(), alias.identity());

        let native = root.as_native_owned();
        let restored = SourceSharedInterop::from_native_owned(&native)
            .expect("native-owned Source Shared root must downcast");
        assert_eq!(restored.identity(), root.identity());

        root.with_edit(|value| {
            *value = state(2);
            Ok(())
        })
        .expect("physical Shared edit should commit");
        assert_eq!(
            alias
                .with_read(state_value)
                .expect("alias should read the committed payload"),
            2
        );

        let mut guard = alias
            .acquire_guard(true)
            .expect("editable physical guard should acquire");
        guard
            .stage_value(state(3))
            .expect("editable guard should stage its payload");
        drop(guard);
        assert_eq!(
            root.with_read(state_value)
                .expect("guard drop should publish its staged payload"),
            3
        );
    }

    #[test]
    fn source_shared_interop_capture_and_conditional_replace_keep_owner_revision() {
        let root = SourceSharedInterop::from_value(44, state(1));
        let (initial, initial_revision) = root
            .capture_with_revision()
            .expect("capture should return a payload and its owner revision");
        assert_eq!(state_value(&initial).expect("initial State should decode"), 1);
        assert_eq!(initial_revision, 0);

        let (committed, committed_revision) = root
            .replace_if_revision(initial_revision, state(2))
            .expect("matching revision should be conditionally replaceable");
        assert!(committed);
        assert_eq!(committed_revision, initial_revision + 1);

        let (stale_commit, current_revision) = root
            .replace_if_revision(initial_revision, state(3))
            .expect("stale revision should be a normal non-commit");
        assert!(!stale_commit);
        assert_eq!(current_revision, committed_revision);
        let (current, current_revision) = root
            .capture_with_revision()
            .expect("capture should expose the committed value and revision");
        assert_eq!(state_value(&current).expect("State should decode"), 2);
        assert_eq!(current_revision, committed_revision);

        let semantic_result = root.with_edit(|value| {
            *value = state(4);
            Ok(Err::<(), _>("Source callback returned an error".to_string()))
        });
        assert_eq!(
            semantic_result,
            Ok(Err("Source callback returned an error".to_string()))
        );
        let (current, current_revision) = root
            .capture_with_revision()
            .expect("semantic callback errors remain published data");
        assert_eq!(state_value(&current).expect("State should decode"), 4);
        assert_eq!(current_revision, committed_revision + 1);
    }

    #[test]
    fn source_shared_interop_edit_error_does_not_publish_partial_value() {
        let root = SourceSharedInterop::from_value(42, state(7));
        let result = root.with_edit(|value| {
            *value = state(99);
            Err::<(), _>("codec rejected the replacement".to_string())
        });
        assert_eq!(result, Err("codec rejected the replacement".to_string()));
        assert_eq!(
            root.with_read(state_value)
                .expect("failed edit should leave the root readable"),
            7
        );
    }

    #[test]
    fn source_shared_interop_wait_reacquires_refreshes_and_cancellation_discards() {
        let root = SourceSharedInterop::from_value(43, state(1));
        let mut guard = root
            .acquire_guard(true)
            .expect("editable physical guard should acquire");
        guard
            .wait_suspend(state(2))
            .expect("wait handoff should publish and release");
        root.with_edit(|value| {
            *value = state(3);
            Ok(())
        })
        .expect("another owner should edit while the guard is suspended");
        assert_eq!(
            guard
                .wait_resume(&mut || false)
                .expect("wait handoff should reacquire"),
            Some(state(3))
        );
        guard
            .finish_value(state(4))
            .expect("resumed guard should finish its staged value");
        drop(guard);
        assert_eq!(
            root.with_read(state_value)
                .expect("resumed guard should publish on close"),
            4
        );

        let mut cancelled = root
            .acquire_guard(true)
            .expect("second editable physical guard should acquire");
        cancelled
            .wait_suspend(state(5))
            .expect("second wait handoff should publish and release");
        root.with_edit(|value| {
            *value = state(6);
            Ok(())
        })
        .expect("owner edit should be visible before cancellation");
        assert_eq!(
            cancelled
                .wait_resume(&mut || true)
                .expect("cancelled wait should return cleanly"),
            None
        );
        cancelled
            .finish_value(state(8))
            .expect("cancelled close should discard its stale projection");
        drop(cancelled);
        assert_eq!(
            root.with_read(state_value)
                .expect("cancelled guard should leave the latest owner value"),
            6
        );
    }
}
