//! Checked, invocation-scoped dispatch for native implementations of private MIR traits.
//!
//! A binding associates one checked trait method with a non-owning registered carrier identity;
//! its physical root remains in an external carrier. The set is activated around a specific
//! artifact execution; nested calls restore the previous activation, and task workers must
//! explicitly activate their own set. Calls own every argument until a completion (including
//! failures and handler unwinds) returns the exact argument rows and transfer receipts. Move
//! payloads remain in call-owned escrow until an explicit disposition commits; handlers borrow
//! a recoverable ownership guard, and rejected offers remain in the completion unless taken.

use jet_foundation::Diagnostics::Span;
use jet_foundation::MIR::{
    MirAccess, MirArtifactId, MirExecutionIdentity, MirFailureCarrier, MirFieldId, MirNativeOwned,
    MirNominalRef, MirParam, MirPreludeCallId, MirProgram, MirRuntimeValue, MirTraitId,
    MirTraitMethodId, MirTraitRef, MirType, MirTypeId, MirTypeKind,
};
use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::fmt;
use std::marker::PhantomData;
use std::panic::AssertUnwindSafe;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

/// Exact artifact-local identity of one native trait-method binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeInterfaceIdentity {
    pub execution: MirExecutionIdentity,
    pub artifact: MirArtifactId,
    pub trait_ref: MirTraitRef,
    pub method_id: MirTraitMethodId,
    pub method_name: String,
    /// The exact trait-object MIR type at the call site, including its checked bounds.
    pub receiver_type: MirType,
}

/// The full checked MIR signature used to validate every call to a binding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeInterfaceSignature {
    pub receiver_access: MirAccess,
    pub parameters: Vec<MirParam>,
    pub return_type: MirType,
    pub failure: MirFailureCarrier,
}

/// Checked identity for a native function value. `key` is an explicit host
/// registration name, never a synthetic MIR function ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeCallableIdentity {
    pub execution: MirExecutionIdentity,
    pub artifact: MirArtifactId,
    pub key: String,
    pub callable_type: MirType,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeCallableParameter {
    pub ty: MirType,
    pub access: MirAccess,
}

/// Exact invocation shape projected from a checked MIR `Fn`/`SendFn` value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeCallableSignature {
    pub parameters: Vec<NativeCallableParameter>,
    pub return_type: Option<MirType>,
}

impl NativeCallableSignature {
    pub fn checked(callable_type: &MirType) -> Result<Self, NativeInterfaceError> {
        if !callable_type.has_valid_layout() {
            return Err(NativeInterfaceError::InvalidMetadata(
                "native callable type has an invalid MIR layout".to_string(),
            ));
        }
        match callable_type.kind() {
            MirTypeKind::Fn(signature) => Ok(Self {
                parameters: signature
                    .params
                    .iter()
                    .enumerate()
                    .map(|(index, ty)| NativeCallableParameter {
                        ty: ty.clone(),
                        access: signature
                            .call_metadata
                            .as_ref()
                            .and_then(|metadata| metadata.conventions.get(index))
                            .copied()
                            .unwrap_or(MirAccess::Read),
                    })
                    .collect(),
                return_type: signature.ret.as_deref().cloned(),
            }),
            MirTypeKind::SendFn {
                params,
                ret,
                conventions,
            } if params.len() == conventions.len() => Ok(Self {
                parameters: params
                    .iter()
                    .zip(conventions)
                    .map(|(ty, access)| NativeCallableParameter {
                        ty: ty.clone(),
                        access: *access,
                    })
                    .collect(),
                return_type: ret.as_deref().cloned(),
            }),
            MirTypeKind::SendFn { .. } => Err(NativeInterfaceError::InvalidMetadata(
                "native SendFn conventions do not cover every checked parameter".to_string(),
            )),
            _ => Err(NativeInterfaceError::InvalidMetadata(
                "native callable binding requires a checked Fn or SendFn type".to_string(),
            )),
        }
    }
}

impl NativeCallableIdentity {
    pub fn checked(
        program: &MirProgram,
        artifact: MirArtifactId,
        key: impl Into<String>,
        callable_type: MirType,
    ) -> Result<Self, NativeInterfaceError> {
        Self::checked_with(artifact, key, callable_type, || {
            program
                .execution_identity(Some(artifact))
                .map_err(|error| NativeInterfaceError::InvalidMetadata(error.to_string()))
        })
    }

    /// [`Self::checked`] for a caller that already holds the artifact's
    /// execution identity (a sealed compiler image), so the whole program is
    /// not re-digested per identity.
    pub fn checked_for_execution(
        execution: MirExecutionIdentity,
        artifact: MirArtifactId,
        key: impl Into<String>,
        callable_type: MirType,
    ) -> Result<Self, NativeInterfaceError> {
        Self::checked_with(artifact, key, callable_type, || Ok(execution))
    }

    fn checked_with(
        artifact: MirArtifactId,
        key: impl Into<String>,
        callable_type: MirType,
        execution: impl FnOnce() -> Result<MirExecutionIdentity, NativeInterfaceError>,
    ) -> Result<Self, NativeInterfaceError> {
        let key = key.into();
        if key.is_empty() || key.trim() != key {
            return Err(NativeInterfaceError::InvalidMetadata(
                "native callable key must be a non-empty canonical string".to_string(),
            ));
        }
        NativeCallableSignature::checked(&callable_type)?;
        let execution = execution()?;
        Ok(Self {
            execution,
            artifact,
            key,
            callable_type,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeCallableBindingDescriptor {
    pub identity: NativeCallableIdentity,
    pub signature: NativeCallableSignature,
    pub object: NativeInterfaceObjectId,
}

/// One exact binding row exposed to backend installation. The object ID is
/// set-local and is meaningful only with the `NativeInterfaceBindings` that
/// produced this descriptor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeInterfaceBindingDescriptor {
    pub identity: NativeInterfaceIdentity,
    pub signature: NativeInterfaceSignature,
    pub object: NativeInterfaceObjectId,
}

/// One owned, checked argument at the native boundary. Writeback is permitted
/// only when canonical MIR declares `MirAccess::Write`.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeInterfaceArgument {
    pub index: usize,
    pub ty: MirType,
    pub access: MirAccess,
    pub value: MirRuntimeValue,
    pub writeback: bool,
}

/// Identity of a registered native interface object. It is local to one binding
/// set; the service root and invocation carrier remain opaque Source values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NativeInterfaceObjectId(u64);

#[derive(Clone)]
struct NativeInterfaceInstanceMethod {
    identity: NativeInterfaceIdentity,
    signature: NativeInterfaceSignature,
    handler: NativeInterfaceHandler,
}

/// A callback-scope instance contains only its own service/context and cloned
/// immutable handler templates. It deliberately has no strong binding-set link.
struct NativeInterfaceInstance {
    id: NativeInterfaceObjectId,
    set_identity: Arc<()>,
    root: MirNativeOwned,
    methods: Vec<NativeInterfaceInstanceMethod>,
}

struct NativeInterfaceInstanceCarrier {
    instance: Arc<NativeInterfaceInstance>,
}

/// Static carriers identify a registered object while retaining its service
/// root outside the binding metadata that indexes it.
struct NativeInterfaceStaticCarrier {
    id: NativeInterfaceObjectId,
    set_identity: Arc<()>,
    root: MirNativeOwned,
}

/// Non-owning binding metadata. Physical carriers are supplied by their
/// external owner and reconstructed only for one checked call.
#[derive(Clone)]
struct NativeInterfaceObjectKey {
    id: NativeInterfaceObjectId,
    set_identity: Arc<()>,
    carrier: Weak<NativeInterfaceStaticCarrier>,
}

impl NativeInterfaceObjectKey {
    fn of(object: &NativeInterfaceObject) -> Option<Self> {
        let physical = object
            .carrier
            .clone()
            .downcast::<NativeInterfaceStaticCarrier>()
            .ok()?;
        if physical.id != object.id || !Arc::ptr_eq(&physical.set_identity, &object.set_identity) {
            return None;
        }
        Some(Self {
            id: object.id,
            set_identity: object.set_identity.clone(),
            carrier: Arc::downgrade(&physical),
        })
    }

    fn matches_object(&self, object: &NativeInterfaceObject) -> bool {
        self.id == object.id && Arc::ptr_eq(&self.set_identity, &object.set_identity)
    }

    fn matches_static_carrier(&self, carrier: &NativeInterfaceStaticCarrier) -> bool {
        self.id == carrier.id && Arc::ptr_eq(&self.set_identity, &carrier.set_identity)
    }

    fn object_for_static_carrier(
        &self,
        carrier: &MirNativeOwned,
    ) -> Option<NativeInterfaceObject> {
        let physical = carrier.downcast_ref::<NativeInterfaceStaticCarrier>()?;
        self.matches_static_carrier(physical).then(|| NativeInterfaceObject {
            id: self.id,
            set_identity: self.set_identity.clone(),
            root: physical.root.clone(),
            carrier: carrier.clone(),
            instance: None,
        })
    }

    fn object_from_external_carrier(&self) -> Option<NativeInterfaceObject> {
        let physical = self.carrier.upgrade()?;
        let carrier = MirNativeOwned::new(NativeInterfaceStaticCarrier {
            id: self.id,
            set_identity: self.set_identity.clone(),
            root: physical.root.clone(),
        });
        Some(NativeInterfaceObject {
            id: self.id,
            set_identity: self.set_identity.clone(),
            root: physical.root.clone(),
            carrier,
            instance: None,
        })
    }
}
/// One native interface object and its retained physical service root.
#[derive(Clone)]
pub struct NativeInterfaceObject {
    id: NativeInterfaceObjectId,
    set_identity: Arc<()>,
    root: MirNativeOwned,
    carrier: MirNativeOwned,
    instance: Option<Arc<NativeInterfaceInstance>>,
}

impl NativeInterfaceObject {
    /// Session-local object identity used by checked dispatch.
    pub fn id(&self) -> NativeInterfaceObjectId {
        self.id
    }

    /// Borrow the canonical native service/context root without converting it
    /// to a raw handle.
    pub fn root<T: Any>(&self) -> Option<&T> {
        self.root.downcast_ref::<T>()
    }

    /// A runtime trait-object carrier aliases this exact registered instance.
    /// Dynamic carriers keep their instance alive; static carriers retain their
    /// service root directly.
    pub fn as_runtime_value(&self) -> MirRuntimeValue {
        MirRuntimeValue::NativeOwned(self.carrier.clone())
    }

    pub fn matches_root(&self, carrier: &MirNativeOwned) -> bool {
        if self.carrier == *carrier {
            return true;
        }
        matches!(
            (
                self.carrier.downcast_ref::<NativeInterfaceStaticCarrier>(),
                carrier.downcast_ref::<NativeInterfaceStaticCarrier>(),
            ),
            (Some(left), Some(right))
                if left.id == right.id
                    && Arc::ptr_eq(&left.set_identity, &right.set_identity)
        )
    }

    /// Retain the exact carrier for an explicit child-task binding.
    pub fn clone_root(&self) -> MirNativeOwned {
        self.carrier.clone()
    }
}


impl fmt::Debug for NativeInterfaceObject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeInterfaceObject")
            .field("id", &self.id)
            .field("dynamic", &self.instance.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum NativeInterfaceTransferDisposition {
    /// Ownership returned to the original argument slot; its exact value is in
    /// the completed invocation's argument row.
    Returned,
    /// Ownership was consumed by a physical leaf. The typed receipt remains
    /// owned by the completion so failure cannot strand a resource transfer.
    Consumed(MirRuntimeValue),
}

/// Receipt for one explicitly moved argument. Receipts are produced only by
/// `return_owned_argument` or `consume_owned_argument` on the owning call.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeInterfaceTransferReceipt {
    pub argument: usize,
    pub disposition: NativeInterfaceTransferDisposition,
}

/// Payload and ownership state shared by a pending transfer row and its handler
/// token. The guard temporarily leases the MIR value; dropping an uncommitted
/// guard restores it to this cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OwnedValueState {
    Pending,
    Committed,
    Aborted,
}

struct TransferPayloadState {
    value: Option<MirRuntimeValue>,
    state: OwnedValueState,
}

type TransferPayload = Arc<Mutex<TransferPayloadState>>;

/// An owned Move argument detached from its call row. Its identity is the
/// shared payload cell; the token is valid only for the call that issued it.
pub struct NativeInterfaceOwnedArgument {
    argument: usize,
    payload: TransferPayload,
}

impl NativeInterfaceOwnedArgument {
    pub fn argument(&self) -> usize {
        self.argument
    }

    /// Lease the payload for a native attempt. The exact value remains under an
    /// unwind-safe guard until the call commits a disposition.
    pub fn take_value(&self) -> Result<NativeInterfaceOwnedValue, NativeInterfaceError> {
        let mut payload = lock_payload(&self.payload);
        if payload.state == OwnedValueState::Committed {
            return Err(NativeInterfaceError::InvalidCall(format!(
                "move argument {} transfer is already committed",
                self.argument
            )));
        }
        let value = payload.value.take().ok_or_else(|| {
            NativeInterfaceError::InvalidCall(format!(
                "move argument {} payload is already leased or resolved",
                self.argument
            ))
        })?;
        payload.state = OwnedValueState::Pending;
        drop(payload);
        Ok(NativeInterfaceOwnedValue {
            argument: self.argument,
            payload: self.payload.clone(),
            value: Some(value),
        })
    }

    fn is_untaken(&self) -> bool {
        lock_payload(&self.payload).value.is_some()
    }
}
/// Recoverable handler lease for one detached Source value. It may be staged
/// privately during acceptance, but a consumer may extract it only after commit.
/// Dropping an uncommitted guard restores the exact MIR payload to its call.
pub struct NativeInterfaceOwnedValue {
    argument: usize,
    payload: TransferPayload,
    value: Option<MirRuntimeValue>,
}

impl NativeInterfaceOwnedValue {
    pub fn argument(&self) -> usize {
        self.argument
    }

    pub fn with_value<T>(
        &self,
        inspect: impl FnOnce(&MirRuntimeValue) -> T,
    ) -> Result<T, NativeInterfaceError> {
        self.value.as_ref().map(inspect).ok_or_else(|| {
            NativeInterfaceError::InvalidCall(format!(
                "move argument {} value has already been resolved",
                self.argument
            ))
        })
    }

    /// Extract the value only after an acceptance receipt has committed. An
    /// early extraction aborts the transfer and restores the value before the
    /// sender can record a consumed receipt.
    pub fn into_committed_value(mut self) -> Result<MirRuntimeValue, NativeInterfaceError> {
        let mut payload = lock_payload(&self.payload);
        if payload.state == OwnedValueState::Committed {
            return self.value.take().ok_or_else(|| {
                NativeInterfaceError::InvalidCall(format!(
                    "move argument {} value was already extracted",
                    self.argument
                ))
            });
        }

        payload.state = OwnedValueState::Aborted;
        if let Some(value) = self.value.take() {
            if payload.value.is_none() {
                payload.value = Some(value);
            }
        }
        Err(NativeInterfaceError::InvalidCall(format!(
            "move argument {} cannot leave its recovery guard before acceptance",
            self.argument
        )))
    }
    /// Commit the exact mirrored receipt before extracting the value. This is
    /// the prepared-send handoff: a successful commit makes extraction
    /// infallible for the owning call, and the original receipt is returned
    /// alongside the value for publication.
    pub fn commit_and_extract(
        self,
        receipt: MirRuntimeValue,
        commit: &mut dyn FnMut(MirRuntimeValue) -> Result<(), NativeInterfaceError>,
    ) -> Result<(MirRuntimeValue, MirRuntimeValue), NativeInterfaceError> {
        commit(receipt.clone())?;
        let value = self.into_committed_value()?;
        Ok((value, receipt))
    }

}

impl Drop for NativeInterfaceOwnedValue {
    fn drop(&mut self) {
        let mut payload = lock_payload(&self.payload);
        if payload.state == OwnedValueState::Committed {
            return;
        }
        payload.state = OwnedValueState::Aborted;
        let Some(value) = self.value.take() else {
            return;
        };
        if payload.value.is_none() {
            payload.value = Some(value);
        }
    }
}

impl fmt::Debug for NativeInterfaceOwnedValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeInterfaceOwnedValue")
            .field("argument", &self.argument)
            .field("has_value", &self.value.is_some())
            .field("state", &lock_payload(&self.payload).state)
            .finish()
    }
}


impl fmt::Debug for NativeInterfaceOwnedArgument {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeInterfaceOwnedArgument")
            .field("argument", &self.argument)
            .field("taken", &!self.is_untaken())
            .finish()
    }
}

#[derive(Clone)]
pub struct NativeInterfaceTransferOffer(Arc<NativeInterfaceTransferOfferState>);

struct NativeInterfaceTransferOfferState {
    argument: usize,
    error: NativeInterfaceError,
    value: Mutex<Option<MirRuntimeValue>>,
}

struct TransferOfferInspection<'a> {
    slot: &'a Mutex<Option<MirRuntimeValue>>,
    value: Option<MirRuntimeValue>,
}

impl Drop for TransferOfferInspection<'_> {
    fn drop(&mut self) {
        let Some(value) = self.value.take() else {
            return;
        };
        let mut slot = self.slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if slot.is_none() {
            *slot = Some(value);
        }
    }
}

impl NativeInterfaceTransferOffer {
    pub fn argument(&self) -> usize {
        self.0.argument
    }

    pub fn error(&self) -> &NativeInterfaceError {
        &self.0.error
    }

    /// Inspect a rejected value without taking ownership away from the call.
    /// During the callback the offer is reserved; reentrant or concurrent access
    /// returns `None` rather than blocking on the same value.
    pub fn with_value<T>(&self, inspect: impl FnOnce(&MirRuntimeValue) -> T) -> Option<T> {
        let value = self
            .0
            .value
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()?;
        let inspection = TransferOfferInspection {
            slot: &self.0.value,
            value: Some(value),
        };
        Some(inspect(inspection.value.as_ref()?))
    }

    /// Explicitly take the offered value out of the completion's recovery
    /// ledger. Dropping the rejection itself does not perform this disposition.
    pub fn take_value(&self) -> Option<MirRuntimeValue> {
        self.0
            .value
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
    }
}

impl fmt::Debug for NativeInterfaceTransferOffer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeInterfaceTransferOffer")
            .field("argument", &self.argument())
            .field("error", self.error())
            .field("has_value", &self.with_value(|_| true).unwrap_or(false))
            .finish()
    }
}

/// A resolution the call refused. The original input remains protected by its
/// guard and the rejected offer is also retained by the call until explicitly
/// taken, so converting this rejection to an error cannot discard ownership.
#[derive(Debug)]
pub struct NativeInterfaceTransferRejection {
    pub transfer: NativeInterfaceOwnedArgument,
    pub value: NativeInterfaceOwnedValue,
    pub offer: NativeInterfaceTransferOffer,
    pub error: NativeInterfaceError,
}

impl NativeInterfaceTransferRejection {
    /// Convert to the handler's error rail. The call still retains the rejected
    /// offer and the transfer ledger still retains the original payload.
    pub fn into_error(self) -> NativeInterfaceError {
        self.error
    }
}

fn lock_payload(payload: &TransferPayload) -> std::sync::MutexGuard<'_, TransferPayloadState> {
    payload.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

struct PendingTransfer {
    argument: usize,
    payload: TransferPayload,
}

/// Move-transfer bookkeeping shared by interface and callable calls. Argument
/// rows stay owned by the call until a guard commits a disposition.
#[derive(Default)]
struct TransferLedger {
    pending: Vec<PendingTransfer>,
    receipts: Vec<NativeInterfaceTransferReceipt>,
    rejected: Vec<NativeInterfaceTransferOffer>,
}

impl TransferLedger {
    fn pending_arguments(&self) -> impl Iterator<Item = usize> + '_ {
        self.pending.iter().map(|transfer| transfer.argument)
    }

    fn has_receipt(&self, index: usize) -> bool {
        self.receipts.iter().any(|receipt| receipt.argument == index)
    }

    fn rejected(&self) -> &[NativeInterfaceTransferOffer] {
        &self.rejected
    }

    fn take(
        &mut self,
        arguments: &mut [NativeInterfaceArgument],
        index: usize,
    ) -> Result<NativeInterfaceOwnedArgument, NativeInterfaceError> {
        let argument = arguments
            .get_mut(index)
            .ok_or_else(|| NativeInterfaceError::InvalidCall(format!("move argument {index} is outside the checked signature")))?;
        if argument.access != MirAccess::Move {
            return Err(NativeInterfaceError::InvalidCall(format!(
                "argument {index} is not a checked Move parameter"
            )));
        }
        if self.pending_arguments().any(|pending| pending == index) || self.has_receipt(index) {
            return Err(NativeInterfaceError::InvalidCall(format!(
                "move argument {index} was transferred more than once"
            )));
        }
        if argument.value == MirRuntimeValue::Moved {
            return Err(NativeInterfaceError::InvalidCall(format!(
                "move argument {index} was already moved"
            )));
        }
        let value = std::mem::replace(&mut argument.value, MirRuntimeValue::Moved);
        let payload: TransferPayload = Arc::new(Mutex::new(TransferPayloadState {
            value: Some(value),
            state: OwnedValueState::Pending,
        }));
        self.pending.push(PendingTransfer {
            argument: index,
            payload: payload.clone(),
        });
        Ok(NativeInterfaceOwnedArgument {
            argument: index,
            payload,
        })
    }

    fn validate_owned(
        &self,
        transfer: &NativeInterfaceOwnedArgument,
        value: &NativeInterfaceOwnedValue,
    ) -> Result<usize, NativeInterfaceError> {
        let Some(position) = self
            .pending
            .iter()
            .position(|pending| Arc::ptr_eq(&pending.payload, &transfer.payload))
        else {
            return Err(NativeInterfaceError::InvalidCall(
                "owned argument transfer is not pending on this call".to_string(),
            ));
        };
        if self.pending[position].argument != transfer.argument
            || transfer.argument != value.argument
            || !Arc::ptr_eq(&transfer.payload, &value.payload)
            || value.value.is_none()
            || lock_payload(&value.payload).state != OwnedValueState::Pending
        {
            return Err(NativeInterfaceError::InvalidCall(
                "owned argument guard does not match its pending transfer".to_string(),
            ));
        }
        Ok(position)
    }

    fn store_rejected(
        &mut self,
        argument: usize,
        error: NativeInterfaceError,
        value: MirRuntimeValue,
    ) -> NativeInterfaceTransferOffer {
        let offer = NativeInterfaceTransferOffer(Arc::new(NativeInterfaceTransferOfferState {
            argument,
            error,
            value: Mutex::new(Some(value)),
        }));
        self.rejected.push(offer.clone());
        offer
    }

    fn reject_resolution(
        &mut self,
        error: NativeInterfaceError,
        transfer: NativeInterfaceOwnedArgument,
        value: NativeInterfaceOwnedValue,
        offered: MirRuntimeValue,
    ) -> Result<(), NativeInterfaceTransferRejection> {
        let offer = self.store_rejected(transfer.argument, error.clone(), offered);
        Err(NativeInterfaceTransferRejection {
            transfer,
            value,
            offer,
            error,
        })
    }

    /// Return a leased input to its original argument slot, or retain a
    /// rejected offered value in the call's recovery ledger.
    fn return_owned(
        &mut self,
        arguments: &mut [NativeInterfaceArgument],
        transfer: NativeInterfaceOwnedArgument,
        value: NativeInterfaceOwnedValue,
        offered: MirRuntimeValue,
    ) -> Result<(), NativeInterfaceTransferRejection> {
        let position = match self.validate_owned(&transfer, &value) {
            Ok(position) => position,
            Err(error) => return self.reject_resolution(error, transfer, value, offered),
        };
        let Some(argument) = arguments.get(transfer.argument) else {
            return self.reject_resolution(
                NativeInterfaceError::InvalidCall("owned argument transfer index disappeared".to_string()),
                transfer,
                value,
                offered,
            );
        };
        if offered == MirRuntimeValue::Moved || !runtime_value_matches_type(&offered, &argument.ty) {
            return self.reject_resolution(
                NativeInterfaceError::InvalidCall(format!(
                    "returned move argument {} does not match its checked MIR type",
                    transfer.argument
                )),
                transfer,
                value,
                offered,
            );
        }
        let index = transfer.argument;
        let is_pending = lock_payload(&value.payload).state == OwnedValueState::Pending;
        if !is_pending {
            return self.reject_resolution(
                NativeInterfaceError::InvalidCall(
                    "owned argument transfer was aborted before return".to_string(),
                ),
                transfer,
                value,
                offered,
            );
        }
        let argument = &mut arguments[index];
        argument.value = offered;
        self.pending.remove(position);
        self.receipts.push(NativeInterfaceTransferReceipt {
            argument: index,
            disposition: NativeInterfaceTransferDisposition::Returned,
        });
        lock_payload(&value.payload).state = OwnedValueState::Committed;
        Ok(())

    }

    fn commit_consumed(
        &mut self,
        position: usize,
        transfer: &NativeInterfaceOwnedArgument,
        value_argument: usize,
        value_payload: &TransferPayload,
        receipt: MirRuntimeValue,
    ) -> Result<(), (NativeInterfaceError, MirRuntimeValue)> {
        let mut payload = lock_payload(value_payload);
        if self.pending.get(position).is_none_or(|pending| {
            pending.argument != transfer.argument
                || !Arc::ptr_eq(&pending.payload, &transfer.payload)
        }) || transfer.argument != value_argument
            || !Arc::ptr_eq(&transfer.payload, value_payload)
            || payload.state != OwnedValueState::Pending
        {
            return Err((
                NativeInterfaceError::InvalidCall(
                    "owned argument transfer changed before physical acceptance was committed".to_string(),
                ),
                receipt,
            ));
        }
        self.receipts.push(NativeInterfaceTransferReceipt {
            argument: transfer.argument,
            disposition: NativeInterfaceTransferDisposition::Consumed(receipt),
        });
        self.pending.remove(position);
        payload.state = OwnedValueState::Committed;
        Ok(())
    }


    /// After the handler has returned or unwound, reclaim every payload left
    /// behind by a dropped or uncommitted guard. The transfer stays pending so
    /// the completion reports the unresolved obligation with its exact value.
    fn reclaim_untaken(&self, arguments: &mut [NativeInterfaceArgument]) {
        for pending in &self.pending {
            let Some(value) = lock_payload(&pending.payload).value.take() else {
                continue;
            };
            if let Some(argument) = arguments.get_mut(pending.argument) {
                argument.value = value;
            }
        }
    }

    fn validate(&self, arguments: &[NativeInterfaceArgument]) -> Result<(), NativeInterfaceError> {
        if !self.pending.is_empty() {
            return Err(NativeInterfaceError::UnresolvedTransfers(
                self.pending_arguments().collect(),
            ));
        }
        for argument in arguments {
            if argument.access == MirAccess::Move && !self.has_receipt(argument.index) {
                return Err(NativeInterfaceError::UnresolvedTransfers(vec![argument.index]));
            }
            if argument.writeback && argument.value == MirRuntimeValue::Moved {
                return Err(NativeInterfaceError::InvalidCall(format!(
                    "writable argument {} has no writeback value",
                    argument.index
                )));
            }
            if argument.writeback && !runtime_value_matches_type(&argument.value, &argument.ty) {
                return Err(NativeInterfaceError::InvalidCall(format!(
                    "writeback argument {} does not match its checked MIR type",
                    argument.index
                )));
            }
        }
        Ok(())
    }
}

fn consume_owned_argument(
    transfers: &mut TransferLedger,
    transfer: NativeInterfaceOwnedArgument,
    value: NativeInterfaceOwnedValue,
    consume: impl FnOnce(
        NativeInterfaceOwnedValue,
        &mut dyn FnMut(MirRuntimeValue) -> Result<(), NativeInterfaceError>,
    ) -> Result<(), NativeInterfaceError>,
) -> Result<(), NativeInterfaceError> {
    let position = transfers.validate_owned(&transfer, &value)?;
    let value_argument = value.argument;
    let value_payload = value.payload.clone();
    let commit_called = Cell::new(false);
    let mut commit = |receipt| {
        if commit_called.get() {
            let error = NativeInterfaceError::InvalidCall(
                "native leaf committed one owned argument more than once".to_string(),
            );
            transfers.store_rejected(transfer.argument, error.clone(), receipt);
            return Err(error);
        }
        match transfers.commit_consumed(
            position,
            &transfer,
            value_argument,
            &value_payload,
            receipt,
        ) {
            Ok(()) => {
                commit_called.set(true);
                Ok(())
            }
            Err((error, receipt)) => {
                transfers.store_rejected(transfer.argument, error.clone(), receipt);
                Err(error)
            }
        }
    };
    consume(value, &mut commit)?;
    if !commit_called.get() {
        return Err(NativeInterfaceError::InvalidCall(
            "native leaf returned without committing physical ownership acceptance".to_string(),
        ));
    }
    Ok(())
}

fn writeback_argument(
    arguments: &mut [NativeInterfaceArgument],
    index: usize,
    value: MirRuntimeValue,
) -> Result<(), NativeInterfaceError> {
    let argument = arguments
        .get_mut(index)
        .ok_or_else(|| NativeInterfaceError::InvalidCall(format!("writeback argument {index} is outside the checked signature")))?;
    if !argument.writeback {
        return Err(NativeInterfaceError::InvalidCall(format!(
            "argument {index} is not declared writable by this native call"
        )));
    }
    if value == MirRuntimeValue::Moved {
        return Err(NativeInterfaceError::InvalidCall(format!(
            "writeback argument {index} cannot be returned in moved state"
        )));
    }
    if !runtime_value_matches_type(&value, &argument.ty) {
        return Err(NativeInterfaceError::InvalidCall(format!(
            "writeback argument {index} does not match its checked MIR type"
        )));
    }
    argument.value = value;
    Ok(())
}

fn with_checked_writeback_argument<R>(
    arguments: &mut [NativeInterfaceArgument],
    index: usize,
    checked_type: &MirType,
    checked_access: MirAccess,
    reborrow: impl FnOnce(&mut MirRuntimeValue) -> R,
) -> Result<R, NativeInterfaceError> {
    let argument = arguments.get_mut(index).ok_or_else(|| {
        NativeInterfaceError::InvalidCall(format!(
            "writeback argument {index} is outside the checked signature"
        ))
    })?;
    if argument.index != index
        || &argument.ty != checked_type
        || argument.access != checked_access
    {
        return Err(NativeInterfaceError::InvalidCall(format!(
            "writeback argument {index} does not match its checked parameter"
        )));
    }
    if checked_access != MirAccess::Write || !argument.writeback {
        return Err(NativeInterfaceError::InvalidCall(format!(
            "argument {index} has no checked Write/writeback authority"
        )));
    }
    if argument.value == MirRuntimeValue::Moved
        || !runtime_value_matches_type(&argument.value, checked_type)
    {
        return Err(NativeInterfaceError::InvalidCall(format!(
            "writeback argument {index} does not contain its checked MIR value"
        )));
    }

    let result = reborrow(&mut argument.value);
    if argument.value == MirRuntimeValue::Moved
        || !runtime_value_matches_type(&argument.value, checked_type)
    {
        return Err(NativeInterfaceError::InvalidCall(format!(
            "writeback argument {index} did not retain its checked MIR value"
        )));
    }
    Ok(result)
}

/// Owned, checked call to one bound native trait method. The dispatcher keeps
/// this value alive across the handler and returns it even if the handler
/// reports failure or unwinds, preserving every argument, writeback slot, and
/// committed transfer receipt.
pub struct NativeInterfaceCall {
    identity: NativeInterfaceIdentity,
    signature: NativeInterfaceSignature,
    object: Option<NativeInterfaceObject>,
    receiver_access: MirAccess,
    receiver: MirRuntimeValue,
    arguments: Vec<NativeInterfaceArgument>,
    span: Span,
    transfers: TransferLedger,
    preflight_error: Option<NativeInterfaceError>,
    physical_borrow_context: Option<Rc<NativeInterfacePhysicalBorrowContext>>,
}

impl NativeInterfaceCall {
    pub fn new(
        identity: NativeInterfaceIdentity,
        signature: NativeInterfaceSignature,
        object: Option<NativeInterfaceObject>,
        receiver_access: MirAccess,
        receiver: MirRuntimeValue,
        arguments: Vec<NativeInterfaceArgument>,
        span: Span,
    ) -> Self {
        let mut call = Self {
            identity,
            signature,
            object,
            receiver_access,
            receiver,
            arguments,
            span,
            transfers: TransferLedger::default(),
            preflight_error: None,
            physical_borrow_context: NativeInterfaceScope::current_physical_borrow_context(),
        };
        call.preflight_error = validate_call_shape(
            &call.identity,
            &call.signature,
            call.object.as_ref(),
            call.receiver_access,
            &call.receiver,
            &call.arguments,
        )
        .err();
        call
    }

    pub fn identity(&self) -> &NativeInterfaceIdentity {
        &self.identity
    }

    pub fn signature(&self) -> &NativeInterfaceSignature {
        &self.signature
    }

    pub fn object(&self) -> Option<&NativeInterfaceObject> {
        self.object.as_ref()
    }

    pub fn receiver_access(&self) -> MirAccess {
        self.receiver_access
    }

    pub fn receiver(&self) -> &MirRuntimeValue {
        &self.receiver
    }

    pub fn arguments(&self) -> &[NativeInterfaceArgument] {
        &self.arguments
    }

    /// Committed transfer receipts, in resolution order.
    pub fn transfers(&self) -> &[NativeInterfaceTransferReceipt] {
        &self.transfers.receipts
    }

    /// Move arguments that remain unresolved after the handler.
    pub fn pending_transfers(&self) -> impl Iterator<Item = usize> + '_ {
        self.transfers.pending_arguments()
    }

    /// Rejected return values or physical receipts retained for explicit
    /// recovery. Dropping a rejection-to-error wrapper never drops these values.
    pub fn rejected_transfers(&self) -> &[NativeInterfaceTransferOffer] {
        self.transfers.rejected()
    }

    pub fn span(&self) -> Span {
        self.span
    }

    /// Read a checked argument without relinquishing its ownership.
    pub fn argument(&self, index: usize) -> Option<&NativeInterfaceArgument> {
        self.arguments.get(index)
    }

    /// Reborrow a checked call-owned physical guard view only for this closure.
    /// The completion retains the non-Send sidecar until its caller releases it.
    pub fn with_physical_borrow<R>(
        &self,
        metadata: &NativeInterfacePhysicalBorrowMetadata,
        reborrow: impl FnOnce(&crate::SourceSharedInterop::SourceSharedInteropGuardState) -> R,
    ) -> Result<R, NativeInterfaceError> {
        let context = self
            .physical_borrow_context
            .as_ref()
            .ok_or(NativeInterfaceError::MissingBinding)?;
        context.with_physical_borrow(metadata, reborrow)
    }

    /// Replace one explicitly writable argument value. This remains available
    /// on the error rail so callers can write back partially completed Source
    /// state before propagating the typed failure.
    pub fn writeback_argument(
        &mut self,
        index: usize,
        value: MirRuntimeValue,
    ) -> Result<(), NativeInterfaceError> {
        writeback_argument(&mut self.arguments, index, value)
    }

    /// Borrow an existing checked Write argument for the duration of `reborrow`.
    /// Its in-place mutation remains in this call if the closure returns an
    /// error or unwinds through the native handler boundary.
    pub fn with_writeback_argument<R>(
        &mut self,
        index: usize,
        reborrow: impl FnOnce(&mut MirRuntimeValue) -> R,
    ) -> Result<R, NativeInterfaceError> {
        if let Some(error) = &self.preflight_error {
            return Err(error.clone());
        }
        let parameter = self.signature.parameters.get(index).ok_or_else(|| {
            NativeInterfaceError::InvalidCall(format!(
                "writeback argument {index} is outside the checked signature"
            ))
        })?;
        if parameter.index != index {
            return Err(NativeInterfaceError::InvalidCall(format!(
                "writeback argument {index} does not match its checked parameter index"
            )));
        }
        with_checked_writeback_argument(
            &mut self.arguments,
            index,
            &parameter.ty,
            parameter.access,
            reborrow,
        )
    }

    /// Detach an owned Move argument behind an invocation-local token. Its
    /// exact payload stays in a recoverable guard until resolution commits.
    pub fn take_owned_argument(
        &mut self,
        index: usize,
    ) -> Result<NativeInterfaceOwnedArgument, NativeInterfaceError> {
        self.transfers.take(&mut self.arguments, index)
    }

    /// Return the leased value after a native attempt. A rejected offered value
    /// is retained in `rejected_transfers()` even if the rejection is converted
    /// to a handler error.
    pub fn return_owned_argument(
        &mut self,
        transfer: NativeInterfaceOwnedArgument,
        value: NativeInterfaceOwnedValue,
        returned: MirRuntimeValue,
    ) -> Result<(), NativeInterfaceTransferRejection> {
        self.transfers
            .return_owned(&mut self.arguments, transfer, value, returned)
    }

    /// Run the physical consumer while its input remains protected by a
    /// recoverable guard. `commit` must be called exactly at the leaf's
    /// irreversible acceptance point with its receipt; after that point the
    /// completion owns the receipt even if the handler later fails or unwinds.
    pub fn consume_owned_argument(
        &mut self,
        transfer: NativeInterfaceOwnedArgument,
        value: NativeInterfaceOwnedValue,
        consume: impl FnOnce(
            NativeInterfaceOwnedValue,
            &mut dyn FnMut(MirRuntimeValue) -> Result<(), NativeInterfaceError>,
        ) -> Result<(), NativeInterfaceError>,
    ) -> Result<(), NativeInterfaceError> {
        consume_owned_argument(&mut self.transfers, transfer, value, consume)
    }

    fn finish(
        &mut self,
        outcome: Result<MirRuntimeValue, NativeInterfaceError>,
    ) -> Result<MirRuntimeValue, NativeInterfaceError> {
        self.transfers.reclaim_untaken(&mut self.arguments);
        self.transfers.validate(&self.arguments)?;
        let value = outcome?;
        if !runtime_value_matches_type(&value, &self.signature.return_type) {
            return Err(NativeInterfaceError::InvalidCall(
                "native result does not match its checked MIR return type".to_string(),
            ));
        }
        Ok(value)
    }
}

/// Owned call to a bound native free function. Its identity is the explicit
/// registration key and checked Fn type, not a fabricated MIR function ID.
pub struct NativeCallableCall {
    identity: Option<NativeCallableIdentity>,
    signature: NativeCallableSignature,
    object: Option<NativeInterfaceObject>,
    carrier: Option<MirNativeOwned>,
    arguments: Vec<NativeInterfaceArgument>,
    span: Span,
    transfers: TransferLedger,
    preflight_error: Option<NativeInterfaceError>,
    physical_borrow_context: Option<Rc<NativeInterfacePhysicalBorrowContext>>,
}

impl NativeCallableCall {
    fn new(
        identity: Option<NativeCallableIdentity>,
        signature: NativeCallableSignature,
        object: Option<NativeInterfaceObject>,
        carrier: Option<MirNativeOwned>,
        arguments: Vec<NativeInterfaceArgument>,
        span: Span,
    ) -> Self {
        let mut call = Self {
            identity,
            signature,
            object,
            carrier,
            arguments,
            span,
            transfers: TransferLedger::default(),
            preflight_error: None,
            physical_borrow_context: NativeInterfaceScope::current_physical_borrow_context(),
        };
        call.preflight_error = validate_native_callable_call(
            call.identity.as_ref(),
            &call.signature,
            call.object.as_ref(),
            call.carrier.as_ref(),
            &call.arguments,
        )
        .err();
        call
    }

    pub fn identity(&self) -> Option<&NativeCallableIdentity> {
        self.identity.as_ref()
    }

    pub fn signature(&self) -> &NativeCallableSignature {
        &self.signature
    }

    pub fn transfers(&self) -> &[NativeInterfaceTransferReceipt] {
        &self.transfers.receipts
    }

    pub fn pending_transfers(&self) -> impl Iterator<Item = usize> + '_ {
        self.transfers.pending_arguments()
    }

    pub fn rejected_transfers(&self) -> &[NativeInterfaceTransferOffer] {
        self.transfers.rejected()
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn argument(&self, index: usize) -> Option<&NativeInterfaceArgument> {
        self.arguments.get(index)
    }

    /// Reborrow a checked call-owned physical guard view only for this closure.
    /// The completion retains the non-Send sidecar until its caller releases it.
    pub fn with_physical_borrow<R>(
        &self,
        metadata: &NativeInterfacePhysicalBorrowMetadata,
        reborrow: impl FnOnce(&crate::SourceSharedInterop::SourceSharedInteropGuardState) -> R,
    ) -> Result<R, NativeInterfaceError> {
        let context = self
            .physical_borrow_context
            .as_ref()
            .ok_or(NativeInterfaceError::MissingBinding)?;
        context.with_physical_borrow(metadata, reborrow)
    }

    pub fn writeback_argument(
        &mut self,
        index: usize,
        value: MirRuntimeValue,
    ) -> Result<(), NativeInterfaceError> {
        writeback_argument(&mut self.arguments, index, value)
    }

    /// Borrow an existing checked Write argument for the duration of `reborrow`.
    /// Its in-place mutation remains in this call if the closure returns an
    /// error or unwinds through the native handler boundary.
    pub fn with_writeback_argument<R>(
        &mut self,
        index: usize,
        reborrow: impl FnOnce(&mut MirRuntimeValue) -> R,
    ) -> Result<R, NativeInterfaceError> {
        if let Some(error) = &self.preflight_error {
            return Err(error.clone());
        }
        let parameter = self.signature.parameters.get(index).ok_or_else(|| {
            NativeInterfaceError::InvalidCall(format!(
                "writeback argument {index} is outside the checked signature"
            ))
        })?;
        with_checked_writeback_argument(
            &mut self.arguments,
            index,
            &parameter.ty,
            parameter.access,
            reborrow,
        )
    }

    pub fn take_owned_argument(
        &mut self,
        index: usize,
    ) -> Result<NativeInterfaceOwnedArgument, NativeInterfaceError> {
        self.transfers.take(&mut self.arguments, index)
    }

    pub fn return_owned_argument(
        &mut self,
        transfer: NativeInterfaceOwnedArgument,
        value: NativeInterfaceOwnedValue,
        returned: MirRuntimeValue,
    ) -> Result<(), NativeInterfaceTransferRejection> {
        self.transfers
            .return_owned(&mut self.arguments, transfer, value, returned)
    }

    pub fn consume_owned_argument(
        &mut self,
        transfer: NativeInterfaceOwnedArgument,
        value: NativeInterfaceOwnedValue,
        consume: impl FnOnce(
            NativeInterfaceOwnedValue,
            &mut dyn FnMut(MirRuntimeValue) -> Result<(), NativeInterfaceError>,
        ) -> Result<(), NativeInterfaceError>,
    ) -> Result<(), NativeInterfaceError> {
        consume_owned_argument(&mut self.transfers, transfer, value, consume)
    }

    fn finish(
        &mut self,
        outcome: Result<MirRuntimeValue, NativeInterfaceError>,
    ) -> Result<MirRuntimeValue, NativeInterfaceError> {
        self.transfers.reclaim_untaken(&mut self.arguments);
        self.transfers.validate(&self.arguments)?;
        let value = outcome?;
        let matches = match &self.signature.return_type {
            Some(ty) => runtime_value_matches_type(&value, ty),
            None => matches!(value, MirRuntimeValue::Unit),
        };
        if !matches {
            return Err(NativeInterfaceError::InvalidCall(
                "native callable result does not match its checked Fn return type".to_string(),
            ));
        }
        Ok(value)
    }
}

/// Run one handler inside the native boundary. A panic is converted into a
/// failed outcome so the owned call, its committed receipts, and its writeback
/// rows survive for the completion instead of unwinding through the host.
fn invoke_handler(
    handler: impl FnOnce() -> Result<MirRuntimeValue, String>,
) -> Result<MirRuntimeValue, NativeInterfaceError> {
    match std::panic::catch_unwind(AssertUnwindSafe(handler)) {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(detail)) => Err(NativeInterfaceError::Handler(detail)),
        Err(payload) => Err(NativeInterfaceError::HandlerPanicked(panic_detail(payload.as_ref()))),
    }
}

fn panic_detail(payload: &(dyn Any + Send)) -> String {
    if let Some(detail) = payload.downcast_ref::<&str>() {
        (*detail).to_string()
    } else if let Some(detail) = payload.downcast_ref::<String>() {
        detail.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

/// Full result of one native callable invocation. The owned call remains
/// available on failure with the same guarantees as `NativeInterfaceCompletion`.
pub struct NativeCallableCompletion {
    outcome: Result<MirRuntimeValue, NativeInterfaceError>,
    call: NativeCallableCall,
}

impl NativeCallableCompletion {
    pub fn outcome(&self) -> Result<&MirRuntimeValue, &NativeInterfaceError> {
        self.outcome.as_ref()
    }

    pub fn call(&self) -> &NativeCallableCall {
        &self.call
    }

    pub fn into_parts(
        self,
    ) -> (
        Result<MirRuntimeValue, NativeInterfaceError>,
        NativeCallableCall,
    ) {
        (self.outcome, self.call)
    }
}

/// The handler receives the canonical retained object and the mutable owned
/// call. It returns a typed result or error; dispatch packages the entire call
/// and all receipts in the completion in either case, including on unwind.
pub type NativeInterfaceHandler =
    Arc<dyn Fn(&NativeInterfaceObject, &mut NativeInterfaceCall) -> Result<MirRuntimeValue, String> + Send + Sync + 'static>;

pub type NativeCallableHandler =
    Arc<dyn Fn(&NativeInterfaceObject, &mut NativeCallableCall) -> Result<MirRuntimeValue, String> + Send + Sync + 'static>;

/// Per-activation binding set. It owns checked handler templates and
/// non-owning static-object identities; physical roots live in external
/// carriers. Callback-scope instances are weakly indexed so their returned
/// carrier, not the set, controls their lifetime.
pub struct NativeInterfaceBindings {
    set_identity: Arc<()>,
    next_object: AtomicU64,
    bindings: std::sync::RwLock<Vec<NativeInterfaceBinding>>,
    callable_bindings: std::sync::RwLock<Vec<NativeCallableBinding>>,
    instances: std::sync::RwLock<Vec<Weak<NativeInterfaceInstance>>>,
}

struct NativeInterfaceBinding {
    object: NativeInterfaceObjectKey,
    identity: NativeInterfaceIdentity,
    signature: NativeInterfaceSignature,
    handler: NativeInterfaceHandler,
}

struct NativeCallableBinding {
    object: NativeInterfaceObjectKey,
    identity: NativeCallableIdentity,
    signature: NativeCallableSignature,
    handler: NativeCallableHandler,
}

struct NativeCallableRcAssociation {
    identity: NativeCallableIdentity,
    carrier: MirNativeOwned,
    bindings: Arc<NativeInterfaceBindings>,
}

/// A registration lease captured by one local `Rc` callable wrapper. Its
/// carrier remains owned by the wrapper, not by the thread-local lookup table.
pub struct NativeCallableRcRegistration {
    association: Arc<NativeCallableRcAssociation>,
}

impl NativeCallableRcRegistration {
    pub fn identity(&self) -> &NativeCallableIdentity {
        &self.association.identity
    }

    pub fn carrier(&self) -> &MirNativeOwned {
        &self.association.carrier
    }
}

struct NativeCallableRcAssociationEntry {
    wrapper_type: TypeId,
    wrapper: std::rc::Weak<dyn Any>,
    association: Weak<NativeCallableRcAssociation>,
}

thread_local! {
    static NATIVE_CALLABLE_RC_ASSOCIATIONS: RefCell<Vec<NativeCallableRcAssociationEntry>> =
        RefCell::new(Vec::new());
}

/// Find the checked native root attached to this exact local wrapper. `Rc`
/// values never cross threads; decoding on another thread creates and registers
/// a fresh wrapper for the same portable MIR carrier.
pub fn native_callable_rc_association<W: 'static>(
    wrapper: &Rc<W>,
) -> Result<(NativeCallableIdentity, MirNativeOwned), NativeInterfaceError> {
    let erased_wrapper: Rc<dyn Any> = wrapper.clone();
    let association = NATIVE_CALLABLE_RC_ASSOCIATIONS.with(|registry| {
        let mut registry = registry.borrow_mut();
        registry.retain(|entry| {
            entry.wrapper.strong_count() != 0 && entry.association.strong_count() != 0
        });
        registry.iter().find_map(|entry| {
            if entry.wrapper_type != TypeId::of::<W>() {
                return None;
            }
            let known = entry.wrapper.upgrade()?;
            Rc::ptr_eq(&known, &erased_wrapper)
                .then(|| entry.association.upgrade())
                .flatten()
        })
    })
    .ok_or(NativeInterfaceError::MissingBinding)?;

    validate_native_callable_rc_binding(
        &association.bindings,
        &association.identity,
        &association.carrier,
    )?;
    Ok((association.identity.clone(), association.carrier.clone()))
}

fn validate_native_callable_rc_binding(
    bindings: &NativeInterfaceBindings,
    identity: &NativeCallableIdentity,
    carrier: &MirNativeOwned,
) -> Result<(), NativeInterfaceError> {
    if identity.execution.artifact.artifact != identity.artifact {
        return Err(NativeInterfaceError::ExecutionMismatch);
    }
    let signature = NativeCallableSignature::checked(&identity.callable_type)?;
    bindings.object_for_callable_carrier(identity, &signature, carrier)?;
    Ok(())
}

impl NativeInterfaceBindings {
    pub fn new() -> Self {
        Self {
            set_identity: Arc::new(()),
            next_object: AtomicU64::new(1),
            bindings: std::sync::RwLock::new(Vec::new()),
            callable_bindings: std::sync::RwLock::new(Vec::new()),
            instances: std::sync::RwLock::new(Vec::new()),
        }
    }

    /// Build a local `Rc` wrapper whose closure retains this exact checked
    /// callable registration. The lookup table stores only weak typed wrapper
    /// and registration references; `W` need not be `Send`.
    pub fn create_native_callable_rc_wrapper<W: 'static>(
        self: &Arc<Self>,
        identity: NativeCallableIdentity,
        carrier: MirNativeOwned,
        build: impl FnOnce(NativeCallableRcRegistration) -> Rc<W>,
    ) -> Result<Rc<W>, NativeInterfaceError> {
        validate_native_callable_rc_binding(self, &identity, &carrier)?;
        let association = Arc::new(NativeCallableRcAssociation {
            identity,
            carrier,
            bindings: self.clone(),
        });
        let wrapper = build(NativeCallableRcRegistration {
            association: association.clone(),
        });
        if Arc::strong_count(&association) == 1 {
            return Err(NativeInterfaceError::InvalidCall(
                "native callable Rc wrapper did not retain its registration".to_string(),
            ));
        }

        let erased_wrapper: Rc<dyn Any> = wrapper.clone();
        let attached = NATIVE_CALLABLE_RC_ASSOCIATIONS.with(|registry| {
            let mut registry = registry.borrow_mut();
            registry.retain(|entry| {
                entry.wrapper.strong_count() != 0 && entry.association.strong_count() != 0
            });
            if registry.iter().any(|entry| {
                entry.wrapper_type == TypeId::of::<W>()
                    && entry
                        .wrapper
                        .upgrade()
                        .is_some_and(|known| Rc::ptr_eq(&known, &erased_wrapper))
            }) {
                return false;
            }
            registry.push(NativeCallableRcAssociationEntry {
                wrapper_type: TypeId::of::<W>(),
                wrapper: Rc::downgrade(&erased_wrapper),
                association: Arc::downgrade(&association),
            });
            true
        });
        if !attached {
            return Err(NativeInterfaceError::InvalidCall(
                "native callable Rc wrapper already has a root association".to_string(),
            ));
        }
        Ok(wrapper)
    }

    /// Create a static adapter object with a carrier that owns its physical
    /// service root, independently of the non-owning binding metadata.
    pub fn create_object<T: Any + Send + Sync>(
        &self,
        root: T,
    ) -> Result<NativeInterfaceObject, NativeInterfaceError> {
        let id = self
            .next_object
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |next| next.checked_add(1))
            .map_err(|_| NativeInterfaceError::IdentityExhausted)?;
        let id = NativeInterfaceObjectId(id);
        let set_identity = self.set_identity.clone();
        let root = MirNativeOwned::new(root);
        let carrier = MirNativeOwned::new(NativeInterfaceStaticCarrier {
            id,
            set_identity: set_identity.clone(),
            root: root.clone(),
        });
        Ok(NativeInterfaceObject {
            id,
            set_identity,
            root,
            carrier,
            instance: None,
        })
    }

    /// Create a callback-scope instance from all checked interface handlers
    /// registered on the exact template carrier/type. The binding set retains
    /// only a Weak instance record; the returned object's carrier owns the
    /// instance and its fresh context.
    pub fn instantiate_interface_for_carrier<T: Any + Send + Sync>(
        self: &Arc<Self>,
        template_carrier: &MirNativeOwned,
        receiver_type: &MirType,
        root: T,
    ) -> Result<NativeInterfaceObject, NativeInterfaceError> {
        if !receiver_type.has_valid_layout()
            || !matches!(receiver_type.kind(), MirTypeKind::TraitObject(_))
        {
            return Err(NativeInterfaceError::InvalidMetadata(
                "native interface instance requires a checked trait-object receiver type".to_string(),
            ));
        }
        let template = template_carrier
            .downcast_ref::<NativeInterfaceStaticCarrier>()
            .filter(|template| Arc::ptr_eq(&template.set_identity, &self.set_identity))
            .ok_or(NativeInterfaceError::MissingBinding)?;
        let methods = {
            let bindings = self
                .bindings
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            bindings
                .iter()
                .filter(|binding| {
                    binding.object.matches_static_carrier(template)
                        && binding.identity.receiver_type == *receiver_type
                })
                .map(|binding| NativeInterfaceInstanceMethod {
                    identity: binding.identity.clone(),
                    signature: binding.signature.clone(),
                    handler: binding.handler.clone(),
                })
                .collect::<Vec<_>>()
        };
        if methods.is_empty() {
            return Err(NativeInterfaceError::MissingBinding);
        }
        let id = self
            .next_object
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |next| next.checked_add(1))
            .map_err(|_| NativeInterfaceError::IdentityExhausted)?;
        let root = MirNativeOwned::new(root);
        let instance = Arc::new(NativeInterfaceInstance {
            id: NativeInterfaceObjectId(id),
            set_identity: self.set_identity.clone(),
            root: root.clone(),
            methods,
        });
        let carrier = MirNativeOwned::new(NativeInterfaceInstanceCarrier {
            instance: instance.clone(),
        });
        let object = NativeInterfaceObject {
            id: instance.id,
            set_identity: self.set_identity.clone(),
            root,
            carrier,
            instance: Some(instance.clone()),
        };
        let mut instances = self
            .instances
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        instances.retain(|record| record.strong_count() != 0);
        instances.push(Arc::downgrade(&instance));
        Ok(object)
    }

    /// Snapshot checked identity/signature rows for backend installation.
    /// Live callback-scope instances appear only while their carriers retain them.
    pub fn method_descriptors(&self) -> Vec<NativeInterfaceBindingDescriptor> {
        let mut descriptors = {
            let bindings = self
                .bindings
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            bindings
                .iter()
                .filter(|binding| binding.object.carrier.strong_count() != 0)
                .map(|binding| NativeInterfaceBindingDescriptor {
                    identity: binding.identity.clone(),
                    signature: binding.signature.clone(),
                    object: binding.object.id,
                })
                .collect::<Vec<_>>()
        };
        let live_instances = {
            let instances = self
                .instances
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            instances.iter().filter_map(Weak::upgrade).collect::<Vec<_>>()
        };
        for instance in live_instances {
            descriptors.extend(instance.methods.iter().map(|binding| {
                NativeInterfaceBindingDescriptor {
                    identity: binding.identity.clone(),
                    signature: binding.signature.clone(),
                    object: instance.id,
                }
            }));
        }
        descriptors
    }

    pub fn callable_descriptors(&self) -> Vec<NativeCallableBindingDescriptor> {
        let bindings = self
            .callable_bindings
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        bindings
            .iter()
            .filter(|binding| binding.object.carrier.strong_count() != 0)
            .map(|binding| NativeCallableBindingDescriptor {
                identity: binding.identity.clone(),
                signature: binding.signature.clone(),
                object: binding.object.id,
            })
            .collect()
    }



    /// Bind one exact checked method implementation. The method descriptor is
    /// built from the same canonical MIR used by the JIT call site.
    pub fn bind(
        &self,
        object: NativeInterfaceObject,
        identity: NativeInterfaceIdentity,
        signature: NativeInterfaceSignature,
        handler: NativeInterfaceHandler,
    ) -> Result<(), NativeInterfaceError> {
        if !Arc::ptr_eq(&object.set_identity, &self.set_identity) {
            return Err(NativeInterfaceError::InvalidCall(
                "native interface object belongs to a different binding set".to_string(),
            ));
        }
        if object.instance.is_some() {
            return Err(NativeInterfaceError::InvalidCall(
                "callback-scope instances inherit their checked interface templates and cannot be rebound".to_string(),
            ));
        }
        let object_key = NativeInterfaceObjectKey::of(&object).ok_or_else(|| {
            NativeInterfaceError::InvalidCall(
                "static native interface object has no external physical carrier".to_string(),
            )
        })?;
        let mut bindings = self
            .bindings
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        bindings.retain(|binding| binding.object.carrier.strong_count() != 0);
        if bindings.iter().any(|binding| {
            binding.object.id == object.id
                && same_method(&binding.identity, &identity)
        }) {
            return Err(NativeInterfaceError::DuplicateBinding);
        }
        bindings.push(NativeInterfaceBinding {
            object: object_key,
            identity,
            signature,
            handler,
        });
        Ok(())
    }

    /// Register one explicit native function value under its checked Fn type.
    ///
    /// Two uniqueness rules make every later lookup exact instead of ambiguous:
    /// a callable identity (execution, artifact, key, checked type) is bound at
    /// most once in the set, and one external carrier exposes at most one
    /// function value per checked signature. Hosts that share one service behind
    /// several same-shaped functions must give each function its own carrier
    /// identity (for example a small per-function newtype over the shared `Arc`).
    pub fn bind_callable(
        &self,
        object: NativeInterfaceObject,
        identity: NativeCallableIdentity,
        signature: NativeCallableSignature,
        handler: NativeCallableHandler,
    ) -> Result<(), NativeInterfaceError> {
        if !Arc::ptr_eq(&object.set_identity, &self.set_identity) {
            return Err(NativeInterfaceError::InvalidCall(
                "native callable object belongs to a different binding set".to_string(),
            ));
        }
        if object.instance.is_some() {
            return Err(NativeInterfaceError::InvalidCall(
                "callback-scope instances cannot register new callable bindings".to_string(),
            ));
        }
        if identity.execution.artifact.artifact != identity.artifact
            || NativeCallableSignature::checked(&identity.callable_type)? != signature
        {
            return Err(NativeInterfaceError::SignatureMismatch);
        }
        let object_key = NativeInterfaceObjectKey::of(&object).ok_or_else(|| {
            NativeInterfaceError::InvalidCall(
                "static native callable object has no external physical carrier".to_string(),
            )
        })?;
        let mut bindings = self
            .callable_bindings
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        bindings.retain(|binding| binding.object.carrier.strong_count() != 0);
        if bindings
            .iter()
            .any(|binding| same_callable(&binding.identity, &identity))
        {
            return Err(NativeInterfaceError::DuplicateBinding);
        }
        if bindings.iter().any(|binding| {
            binding.signature == signature && binding.object.matches_object(&object)
        }) {
            return Err(NativeInterfaceError::CallableRootCollision {
                key: identity.key,
                object: object.id,
            });
        }
        bindings.push(NativeCallableBinding {
            object: object_key,
            identity,
            signature,
            handler,
        });
        Ok(())
    }
    /// Invoke by key only through this retained binding set. The adapter must
    /// retain the same `Arc` across Source tasks/transfers; the active scope is
    /// checked by pointer identity so a matching key in another root is never
    /// substituted. Its externally retained carrier keeps the physical root alive.
    pub fn dispatch_callable_for_scope(
        self: &Arc<Self>,
        identity: NativeCallableIdentity,
        signature: NativeCallableSignature,
        arguments: Vec<NativeInterfaceArgument>,
        span: Span,
    ) -> NativeCallableCompletion {
        let active = ACTIVE_NATIVE_INTERFACES.with(|stack| {
            stack.borrow().last().map(|active| {
                (
                    active.bindings.clone(),
                    active.execution.clone(),
                    active.artifact,
                )
            })
        });
        let Some((active_bindings, execution, artifact)) = active else {
            let mut call = NativeCallableCall::new(
                Some(identity),
                signature,
                None,
                None,
                arguments,
                span,
            );
            call.preflight_error = Some(NativeInterfaceError::MissingBinding);
            return NativeCallableCompletion {
                outcome: Err(NativeInterfaceError::MissingBinding),
                call,
            };
        };
        if !Arc::ptr_eq(&active_bindings, self)
            || execution != identity.execution
            || artifact != identity.artifact
        {
            let mut call = NativeCallableCall::new(
                Some(identity),
                signature,
                None,
                None,
                arguments,
                span,
            );
            call.preflight_error = Some(NativeInterfaceError::ExecutionMismatch);
            return NativeCallableCompletion {
                outcome: Err(NativeInterfaceError::ExecutionMismatch),
                call,
            };
        }
        let binding = self.object_for_callable_identity(&identity, &signature);
        let (object, preflight_error) = match binding {
            Ok(object) => (Some(object), None),
            Err(error) => (None, Some(error)),
        };
        let carrier = object.as_ref().map(NativeInterfaceObject::clone_root);
        let mut call = NativeCallableCall::new(
            Some(identity),
            signature,
            object,
            carrier,
            arguments,
            span,
        );
        // A failed lookup is the root cause; it must not be masked by the
        // shape error that a missing object implies.
        if let Some(error) = preflight_error {
            call.preflight_error = Some(error);
        }
        self.dispatch_callable(&execution, artifact, call)
    }


    /// Validate all registered identities and signatures against this exact
    /// checked artifact and capture the validated C binding scope.
    pub fn checked_scope(
        self: &Arc<Self>,
        program: &MirProgram,
        artifact: MirArtifactId,
    ) -> Result<NativeInterfaceCheckedScope, NativeInterfaceError> {
        let execution = program
            .execution_identity(Some(artifact))
            .map_err(|error| NativeInterfaceError::InvalidMetadata(error.to_string()))?;
        self.checked_scope_for_execution(program, artifact, execution)
    }

    /// [`Self::checked_scope`] for a caller that already holds the artifact's
    /// execution identity (a sealed compiler image): every binding is checked
    /// against `program`'s rows and that identity, and the whole program is
    /// not re-digested.
    pub fn checked_scope_for_execution(
        self: &Arc<Self>,
        program: &MirProgram,
        artifact: MirArtifactId,
        execution: MirExecutionIdentity,
    ) -> Result<NativeInterfaceCheckedScope, NativeInterfaceError> {
        let bindings = self
            .bindings
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for binding in bindings.iter() {
            let checked = NativeInterfaceMethod::checked_for_execution(
                program,
                execution.clone(),
                artifact,
                binding.identity.trait_ref.id,
                binding.identity.method_id,
                binding.identity.receiver_type.clone(),
            )?;
            if checked.identity != binding.identity
                || checked.signature != binding.signature
                || binding.identity.execution != execution
            {
                return Err(NativeInterfaceError::SignatureMismatch);
            }
        }
        drop(bindings);
        let callables = self
            .callable_bindings
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for binding in callables.iter() {
            let checked = NativeCallableIdentity::checked_for_execution(
                execution.clone(),
                artifact,
                binding.identity.key.clone(),
                binding.identity.callable_type.clone(),
            )?;
            let checked_signature =
                NativeCallableSignature::checked(&binding.identity.callable_type)?;
            if checked != binding.identity
                || checked_signature != binding.signature
                || binding.identity.execution != execution
            {
                return Err(NativeInterfaceError::SignatureMismatch);
            }
        }
        drop(callables);
        Ok(NativeInterfaceCheckedScope {
            bindings: self.clone(),
            execution,
            artifact,
        })
    }

    /// Validate this set for one artifact, then push a nested activation.
    pub fn activate(
        self: &Arc<Self>,
        program: &MirProgram,
        artifact: MirArtifactId,
    ) -> Result<NativeInterfaceScope, NativeInterfaceError> {
        let checked_scope = self.checked_scope(program, artifact)?;
        Ok(NativeInterfaceScope::activate_checked_scope(
            &checked_scope,
            None,
        ))
    }

    /// [`Self::activate`] for a caller that already holds the artifact's
    /// execution identity (a sealed compiler image).
    pub fn activate_for_execution(
        self: &Arc<Self>,
        program: &MirProgram,
        artifact: MirArtifactId,
        execution: MirExecutionIdentity,
    ) -> Result<NativeInterfaceScope, NativeInterfaceError> {
        let checked_scope = self.checked_scope_for_execution(program, artifact, execution)?;
        Ok(NativeInterfaceScope::activate_checked_scope(
            &checked_scope,
            None,
        ))
    }

    fn object_for_carrier(
        &self,
        identity: &NativeInterfaceIdentity,
        signature: &NativeInterfaceSignature,
        carrier: &MirNativeOwned,
    ) -> Result<NativeInterfaceObject, NativeInterfaceError> {
        if let Some(physical) = carrier.downcast_ref::<NativeInterfaceStaticCarrier>() {
            let key = {
                let bindings = self
                    .bindings
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let mut matches = bindings.iter().filter(|binding| {
                    binding.object.matches_static_carrier(physical)
                        && same_method(&binding.identity, identity)
                });
                let Some(binding) = matches.next() else {
                    return Err(NativeInterfaceError::MissingBinding);
                };
                if matches.next().is_some() {
                    return Err(NativeInterfaceError::AmbiguousBinding);
                }
                if binding.signature != *signature {
                    return Err(NativeInterfaceError::SignatureMismatch);
                }
                binding.object.clone()
            };
            return key
                .object_for_static_carrier(carrier)
                .ok_or(NativeInterfaceError::MissingBinding);
        }

        let carrier_instance = carrier
            .downcast_ref::<NativeInterfaceInstanceCarrier>()
            .ok_or(NativeInterfaceError::MissingBinding)?;
        let instance = carrier_instance.instance.clone();
        if !Arc::ptr_eq(&instance.set_identity, &self.set_identity) {
            return Err(NativeInterfaceError::MissingBinding);
        }
        let Some(method) = instance
            .methods
            .iter()
            .find(|method| same_method(&method.identity, identity))
        else {
            return Err(NativeInterfaceError::MissingBinding);
        };
        if method.signature != *signature {
            return Err(NativeInterfaceError::SignatureMismatch);
        }
        let instances = self
            .instances
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !instances
            .iter()
            .filter_map(Weak::upgrade)
            .any(|registered| Arc::ptr_eq(&registered, &instance))
        {
            return Err(NativeInterfaceError::MissingBinding);
        }
        drop(instances);
        Ok(NativeInterfaceObject {
            id: instance.id,
            set_identity: self.set_identity.clone(),
            root: instance.root.clone(),
            carrier: carrier.clone(),
            instance: Some(instance),
        })
    }

    fn dispatch(
        &self,
        execution: &MirExecutionIdentity,
        artifact: MirArtifactId,
        mut call: NativeInterfaceCall,
    ) -> NativeInterfaceCompletion {
        if let Some(error) = call.preflight_error.take() {
            return NativeInterfaceCompletion {
                outcome: Err(error),
                call,
            };
        }
        if call.identity.execution != *execution || call.identity.artifact != artifact {
            return NativeInterfaceCompletion {
                outcome: Err(NativeInterfaceError::ExecutionMismatch),
                call,
            };
        }
        let Some(object) = call.object.as_ref() else {
            return NativeInterfaceCompletion {
                outcome: Err(NativeInterfaceError::ReceiverMismatch),
                call,
            };
        };
        let binding = match &object.instance {
            Some(instance) => {
                let receiver_matches = matches!(
                    &call.receiver,
                    MirRuntimeValue::NativeOwned(carrier)
                        if object.matches_root(carrier)
                            && Arc::ptr_eq(&object.set_identity, &self.set_identity)
                            && Arc::ptr_eq(&instance.set_identity, &self.set_identity)
                );
                if !receiver_matches {
                    Some(Err(NativeInterfaceError::ReceiverMismatch))
                } else {
                    instance
                        .methods
                        .iter()
                        .find(|method| same_method(&method.identity, &call.identity))
                        .map(|method| {
                            if method.signature == call.signature {
                                Ok((object.clone(), method.handler.clone()))
                            } else {
                                Err(NativeInterfaceError::SignatureMismatch)
                            }
                        })
                }
            }
            None => {
                let carrier = match &call.receiver {
                    MirRuntimeValue::NativeOwned(carrier) => carrier,
                    _ => return NativeInterfaceCompletion {
                        outcome: Err(NativeInterfaceError::ReceiverMismatch),
                        call,
                    },
                };
                let Some(physical) = carrier.downcast_ref::<NativeInterfaceStaticCarrier>() else {
                    return NativeInterfaceCompletion {
                        outcome: Err(NativeInterfaceError::ReceiverMismatch),
                        call,
                    };
                };
                let bindings = self
                    .bindings
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let mut matches = bindings.iter().filter(|binding| {
                    binding.object.matches_object(object)
                        && binding.object.matches_static_carrier(physical)
                        && same_method(&binding.identity, &call.identity)
                });
                matches.next().map(|binding| {
                    if binding.signature == call.signature {
                        Ok((object.clone(), binding.handler.clone()))
                    } else {
                        Err(NativeInterfaceError::SignatureMismatch)
                    }
                })
            }
        };
        let outcome = match binding {
            None => Err(NativeInterfaceError::MissingBinding),
            Some(Err(error)) => Err(error),
            Some(Ok((object, handler))) => invoke_handler(|| handler(&object, &mut call)),
        };
        let outcome = call.finish(outcome);
        NativeInterfaceCompletion { outcome, call }
    }
    // Static callable metadata retains only a weak carrier reference; the
    // adapter's external physical root determines whether that value is live.
    fn object_for_callable_carrier(
        &self,
        identity: &NativeCallableIdentity,
        signature: &NativeCallableSignature,
        carrier: &MirNativeOwned,
    ) -> Result<NativeInterfaceObject, NativeInterfaceError> {
        let physical = carrier
            .downcast_ref::<NativeInterfaceStaticCarrier>()
            .ok_or(NativeInterfaceError::MissingBinding)?;
        let key = {
            let bindings = self
                .callable_bindings
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            bindings
                .iter()
                .find(|binding| {
                    same_callable(&binding.identity, identity)
                        && binding.signature == *signature
                        && binding.object.matches_static_carrier(physical)
                })
                .map(|binding| binding.object.clone())
                .ok_or(NativeInterfaceError::MissingBinding)?
        };
        key.object_for_static_carrier(carrier)
            .ok_or(NativeInterfaceError::MissingBinding)
    }

    fn object_for_callable_identity(
        &self,
        identity: &NativeCallableIdentity,
        signature: &NativeCallableSignature,
    ) -> Result<NativeInterfaceObject, NativeInterfaceError> {
        let key = {
            let bindings = self
                .callable_bindings
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            bindings
                .iter()
                .find(|binding| {
                    same_callable(&binding.identity, identity) && binding.signature == *signature
                })
                .map(|binding| binding.object.clone())
                .ok_or(NativeInterfaceError::MissingBinding)?
        };
        key.object_from_external_carrier()
            .ok_or(NativeInterfaceError::MissingBinding)
    }

    fn binding_for_callable_carrier(
        &self,
        signature: &NativeCallableSignature,
        carrier: &MirNativeOwned,
    ) -> Result<(NativeCallableIdentity, NativeInterfaceObject), NativeInterfaceError> {
        let physical = carrier
            .downcast_ref::<NativeInterfaceStaticCarrier>()
            .ok_or(NativeInterfaceError::MissingBinding)?;
        let (identity, key) = {
            let bindings = self
                .callable_bindings
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            bindings
                .iter()
                .find(|binding| {
                    binding.signature == *signature
                        && binding.object.matches_static_carrier(physical)
                })
                .map(|binding| (binding.identity.clone(), binding.object.clone()))
                .ok_or(NativeInterfaceError::MissingBinding)?
        };
        let object = key
            .object_for_static_carrier(carrier)
            .ok_or(NativeInterfaceError::MissingBinding)?;
        Ok((identity, object))
    }

    fn dispatch_callable(
        &self,
        execution: &MirExecutionIdentity,
        artifact: MirArtifactId,
        mut call: NativeCallableCall,
    ) -> NativeCallableCompletion {
        if let Some(error) = call.preflight_error.take() {
            return NativeCallableCompletion {
                outcome: Err(error),
                call,
            };
        }
        // Preflight already proved identity, object, and carrier are present and
        // that the carrier identifies this exact registered physical root.
        let (Some(identity), Some(object)) = (call.identity.as_ref(), call.object.as_ref()) else {
            return NativeCallableCompletion {
                outcome: Err(NativeInterfaceError::ReceiverMismatch),
                call,
            };
        };
        if identity.execution != *execution || identity.artifact != artifact {
            return NativeCallableCompletion {
                outcome: Err(NativeInterfaceError::ExecutionMismatch),
                call,
            };
        }
        let handler = {
            let bindings = self
                .callable_bindings
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            bindings
                .iter()
                .find(|binding| {
                    binding.object.matches_object(object)
                        && same_callable(&binding.identity, identity)
                })
                .map(|binding| {
                    if binding.signature == call.signature {
                        Ok((object.clone(), binding.handler.clone()))
                    } else {
                        Err(NativeInterfaceError::SignatureMismatch)
                    }
                })
        };
        let outcome = match handler {
            None => Err(NativeInterfaceError::MissingBinding),
            Some(Err(error)) => Err(error),
            Some(Ok((object, handler))) => invoke_handler(|| handler(&object, &mut call)),
        };
        let outcome = call.finish(outcome);
        NativeCallableCompletion { outcome, call }
    }
}

impl Default for NativeInterfaceBindings {
    fn default() -> Self {
        Self::new()
    }
}

/// Checked native interface method descriptor derived from canonical MIR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeInterfaceMethod {
    pub identity: NativeInterfaceIdentity,
    pub signature: NativeInterfaceSignature,
}

impl NativeInterfaceMethod {
    pub fn checked(
        program: &MirProgram,
        artifact: MirArtifactId,
        trait_id: MirTraitId,
        method_id: MirTraitMethodId,
        receiver_type: MirType,
    ) -> Result<Self, NativeInterfaceError> {
        Self::checked_with(program, artifact, trait_id, method_id, receiver_type, || {
            program
                .execution_identity(Some(artifact))
                .map_err(|error| NativeInterfaceError::InvalidMetadata(error.to_string()))
        })
    }

    /// [`Self::checked`] for a caller that already holds the artifact's
    /// execution identity (a sealed compiler image): the method is checked
    /// against `program`'s rows and the whole program is not re-digested.
    pub fn checked_for_execution(
        program: &MirProgram,
        execution: MirExecutionIdentity,
        artifact: MirArtifactId,
        trait_id: MirTraitId,
        method_id: MirTraitMethodId,
        receiver_type: MirType,
    ) -> Result<Self, NativeInterfaceError> {
        Self::checked_with(program, artifact, trait_id, method_id, receiver_type, || Ok(execution))
    }

    fn checked_with(
        program: &MirProgram,
        artifact: MirArtifactId,
        trait_id: MirTraitId,
        method_id: MirTraitMethodId,
        receiver_type: MirType,
        execution: impl FnOnce() -> Result<MirExecutionIdentity, NativeInterfaceError>,
    ) -> Result<Self, NativeInterfaceError> {
        let artifact_row = program
            .artifacts
            .iter()
            .find(|row| row.id == artifact)
            .ok_or_else(|| NativeInterfaceError::InvalidMetadata(format!("MIR artifact {artifact:?} is missing")))?;
        let mut traits = program.traits.iter().filter(|row| row.id == trait_id);
        let trait_row = traits
            .next()
            .ok_or_else(|| NativeInterfaceError::InvalidMetadata(format!("MIR trait {trait_id:?} is missing")))?;
        if traits.next().is_some() || !artifact_row.modules.contains(&trait_row.module) {
            return Err(NativeInterfaceError::InvalidMetadata(
                "native interface trait identity is ambiguous or outside the selected artifact".to_string(),
            ));
        }
        let mut methods = trait_row.methods.iter().filter(|row| row.id == method_id);
        let method = methods
            .next()
            .ok_or_else(|| NativeInterfaceError::InvalidMetadata(format!("MIR trait method {method_id:?} is missing")))?;
        if methods.next().is_some() {
            return Err(NativeInterfaceError::InvalidMetadata(
                "native interface method identity is ambiguous".to_string(),
            ));
        }
        let receiver_access = method.self_access.ok_or_else(|| {
            NativeInterfaceError::InvalidMetadata("native interface method has no checked receiver access".to_string())
        })?;
        let MirTypeKind::TraitObject(bounds) = receiver_type.kind() else {
            return Err(NativeInterfaceError::InvalidMetadata(
                "native interface receiver type is not a checked trait object".to_string(),
            ));
        };
        if !bounds
            .iter()
            .any(|bound| trait_bound_names(bound, trait_id, &trait_row.name))
        {
            return Err(NativeInterfaceError::InvalidMetadata(
                "native interface receiver type does not carry the exact checked trait bound".to_string(),
            ));
        }
        if method.params.iter().any(|parameter| !parameter.ty.has_valid_layout())
            || !method.return_type.has_valid_layout()
        {
            return Err(NativeInterfaceError::InvalidMetadata(
                "native interface signature contains an invalid MIR layout".to_string(),
            ));
        }
        let execution = execution()?;
        Ok(Self {
            identity: NativeInterfaceIdentity {
                execution,
                artifact,
                trait_ref: MirTraitRef {
                    id: trait_id,
                    name: trait_row.name.clone(),
                },
                method_id,
                method_name: method.name.clone(),
                receiver_type,
            },
            signature: NativeInterfaceSignature {
                receiver_access,
                parameters: method.params.clone(),
                return_type: method.return_type.clone(),
                failure: method.failure.clone(),
            },
        })
    }
}

/// Full result of one native interface invocation. The owned call remains
/// available on failure so the caller can write back every mutable argument
/// and discharge every physical transfer receipt before returning the error.
pub struct NativeInterfaceCompletion {
    outcome: Result<MirRuntimeValue, NativeInterfaceError>,
    call: NativeInterfaceCall,
}

impl NativeInterfaceCompletion {
    pub fn outcome(&self) -> Result<&MirRuntimeValue, &NativeInterfaceError> {
        self.outcome.as_ref()
    }

    pub fn call(&self) -> &NativeInterfaceCall {
        &self.call
    }

    pub fn into_parts(
        self,
    ) -> (
        Result<MirRuntimeValue, NativeInterfaceError>,
        NativeInterfaceCall,
    ) {
        (self.outcome, self.call)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeInterfaceError {
    InvalidMetadata(String),
    InvalidCall(String),
    MissingBinding,
    DuplicateBinding,
    AmbiguousBinding,
    /// A second function value with the same checked signature was bound on a
    /// carrier root that already exposes one; JIT function values would be
    /// indistinguishable, so the registration is refused up front.
    CallableRootCollision {
        key: String,
        object: NativeInterfaceObjectId,
    },
    ReceiverMismatch,
    SignatureMismatch,
    ExecutionMismatch,
    IdentityExhausted,
    UnresolvedTransfers(Vec<usize>),
    Handler(String),
    /// The handler unwound. The completion still carries the owned call with
    /// every argument row, committed receipt, and pending transfer intact.
    HandlerPanicked(String),
}

impl fmt::Display for NativeInterfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMetadata(detail) => write!(formatter, "invalid native interface metadata: {detail}"),
            Self::InvalidCall(detail) => write!(formatter, "invalid native interface call: {detail}"),
            Self::MissingBinding => formatter.write_str("no native interface binding is active for this checked receiver"),
            Self::DuplicateBinding => formatter.write_str("native interface method binding is duplicated"),
            Self::AmbiguousBinding => formatter.write_str("native interface receiver matches more than one checked binding"),
            Self::CallableRootCollision { key, object } => write!(
                formatter,
                "native callable `{key}` shares a carrier root ({object:?}) and checked signature with an existing function value; bind it on its own root"
            ),
            Self::ReceiverMismatch => formatter.write_str("native interface receiver does not carry the retained binding root"),
            Self::SignatureMismatch => formatter.write_str("native interface signature does not match its checked MIR method"),
            Self::ExecutionMismatch => formatter.write_str("native interface binding belongs to a different checked execution or artifact"),
            Self::IdentityExhausted => formatter.write_str("native interface object identity space is exhausted"),
            Self::UnresolvedTransfers(arguments) => write!(formatter, "native interface call has unresolved owned transfers for arguments {arguments:?}"),
            Self::Handler(detail) => write!(formatter, "native interface handler failed: {detail}"),
            Self::HandlerPanicked(detail) => write!(formatter, "native interface handler panicked: {detail}"),
        }
    }
}

impl std::error::Error for NativeInterfaceError {}

/// Caller-validated Source root metadata paired with one invocation-local
/// physical SharedGuard view. The selector indexes only the owning call context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeInterfacePhysicalBorrowMetadata {
    pub selector: i64,
    pub owner_row: MirPreludeCallId,
    pub capability: crate::SourceResources::SourceResourceHandle,
    pub physical_identity: usize,
    pub editable: bool,
    pub path: Vec<MirFieldId>,
}

/// One typed physical view already obtained from its Source/JIT owner.
pub struct NativeInterfacePhysicalBorrowEntry {
    pub metadata: NativeInterfacePhysicalBorrowMetadata,
    pub view: crate::SourceSharedInterop::SourceSharedInteropGuardState,
}

/// Validated native binding scope for one compiler execution/artifact. This
/// checked C-side identity is independent of any guest physical guard views.
#[derive(Clone)]
pub struct NativeInterfaceCheckedScope {
    bindings: Arc<NativeInterfaceBindings>,
    execution: MirExecutionIdentity,
    artifact: MirArtifactId,
}

impl NativeInterfaceCheckedScope {
    pub fn validate_execution(
        &self,
        program: &MirProgram,
        artifact: MirArtifactId,
    ) -> Result<(), NativeInterfaceError> {
        let execution = program
            .execution_identity(Some(artifact))
            .map_err(|error| NativeInterfaceError::InvalidMetadata(error.to_string()))?;
        if self.execution != execution || self.artifact != artifact {
            return Err(NativeInterfaceError::ExecutionMismatch);
        }
        Ok(())
    }
}

/// Non-Send, call-owned set of guest physical guard views. Checked C binding
/// scopes carry this context independently and only expose it through a weak
/// activation-local association.
pub struct NativeInterfacePhysicalBorrowContext {
    entries: Vec<NativeInterfacePhysicalBorrowEntry>,
    thread: std::thread::ThreadId,
}

impl NativeInterfacePhysicalBorrowContext {
    /// Construct one coherent per-call sidecar from arena-validated root facts
    /// and already-typed views. This does not acquire, clone, or re-resolve a
    /// permit; owner-specific code supplies each existing view.
    pub fn new(
        entries: &mut Vec<NativeInterfacePhysicalBorrowEntry>,
    ) -> Result<Rc<Self>, NativeInterfaceError> {
        for (index, entry) in entries.iter().enumerate() {
            if entries[..index]
                .iter()
                .any(|prior| prior.metadata.selector == entry.metadata.selector)
            {
                return Err(NativeInterfaceError::InvalidCall(format!(
                    "physical borrow selector {} is duplicated in one call context",
                    entry.metadata.selector
                )));
            }
            if !matches!(
                &entry.metadata.capability.kind,
                crate::SourceResources::SourceResourceKind::SharedInteropRoot { .. }
            ) {
                return Err(NativeInterfaceError::InvalidCall(format!(
                    "physical borrow selector {} does not carry a checked Shared root capability",
                    entry.metadata.selector
                )));
            }
            validate_native_interface_physical_borrow_entry(entry)?;
        }
        Ok(Rc::new(Self {
            entries: std::mem::take(entries),
            thread: std::thread::current().id(),
        }))
    }

    fn with_physical_borrow<R>(
        &self,
        metadata: &NativeInterfacePhysicalBorrowMetadata,
        reborrow: impl FnOnce(&crate::SourceSharedInterop::SourceSharedInteropGuardState) -> R,
    ) -> Result<R, NativeInterfaceError> {
        if self.thread != std::thread::current().id() {
            return Err(NativeInterfaceError::ExecutionMismatch);
        }
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.metadata.selector == metadata.selector)
            .ok_or(NativeInterfaceError::MissingBinding)?;
        if entry.metadata != *metadata {
            return Err(NativeInterfaceError::ExecutionMismatch);
        }
        validate_native_interface_physical_borrow_entry(entry)?;
        Ok(reborrow(&entry.view))
    }
}
fn validate_native_interface_physical_borrow_entry(
    entry: &NativeInterfacePhysicalBorrowEntry,
) -> Result<(), NativeInterfaceError> {
    let metadata = &entry.metadata;
    let path_matches = entry.view.path().len() == metadata.path.len()
        && entry
            .view
            .path()
            .iter()
            .zip(&metadata.path)
            .all(|(actual, checked)| i64::try_from(checked.0).ok() == Some(*actual));
    if !entry.view.held()
        || entry.view.editable() != metadata.editable
        || entry.view.physical_identity() != metadata.physical_identity
        || !path_matches
    {
        return Err(NativeInterfaceError::InvalidCall(format!(
            "physical borrow selector {} no longer matches its checked guard view",
            metadata.selector
        )));
    }
    Ok(())
}

/// Invocation-local activation guard. It is deliberately !Send: every worker
/// must push its own scope instead of assuming thread-local state follows tasks.
pub struct NativeInterfaceScope {
    token: Option<Arc<()>>,
    physical_borrow_context: Option<Rc<NativeInterfacePhysicalBorrowContext>>,
    _not_send: PhantomData<Rc<()>>,
}

impl NativeInterfaceScope {
    /// Push an already-validated C compiler scope with an optional, independent
    /// guest physical context. The scope owns that context until after pop.
    pub fn activate_checked_scope(
        checked_scope: &NativeInterfaceCheckedScope,
        physical_borrow_context: Option<Rc<NativeInterfacePhysicalBorrowContext>>,
    ) -> Self {
        push_scope_with_physical_borrow(
            checked_scope.bindings.clone(),
            checked_scope.execution.clone(),
            checked_scope.artifact,
            physical_borrow_context,
        )
    }

    /// Capture the exact checked scope at the top of the activation stack.
    pub fn current_checked_scope() -> Option<NativeInterfaceCheckedScope> {
        ACTIVE_NATIVE_INTERFACES.with(|stack| {
            stack.borrow().last().map(|active| NativeInterfaceCheckedScope {
                bindings: active.bindings.clone(),
                execution: active.execution.clone(),
                artifact: active.artifact,
            })
        })
    }

    /// Current activation's guest physical context, if its weak association is
    /// live on this same thread.
    pub fn current_physical_borrow_context(
    ) -> Option<Rc<NativeInterfacePhysicalBorrowContext>> {
        let thread = std::thread::current().id();
        ACTIVE_NATIVE_INTERFACES.with(|stack| {
            stack
                .borrow()
                .last()
                .and_then(|active| active.physical_borrow_context.as_ref()?.upgrade())
                .filter(|context| context.thread == thread)
        })
    }

    pub fn with_active_physical_borrow<R>(
        metadata: &NativeInterfacePhysicalBorrowMetadata,
        reborrow: impl FnOnce(&crate::SourceSharedInterop::SourceSharedInteropGuardState) -> R,
    ) -> Result<R, NativeInterfaceError> {
        let context =
            Self::current_physical_borrow_context().ok_or(NativeInterfaceError::MissingBinding)?;
        context.with_physical_borrow(metadata, reborrow)
    }
}

struct ActiveNativeInterface {
    token: Arc<()>,
    bindings: Arc<NativeInterfaceBindings>,
    execution: MirExecutionIdentity,
    artifact: MirArtifactId,
    physical_borrow_context:
        Option<std::rc::Weak<NativeInterfacePhysicalBorrowContext>>,
}

thread_local! {
    static ACTIVE_NATIVE_INTERFACES: RefCell<Vec<ActiveNativeInterface>> = const { RefCell::new(Vec::new()) };
}
/// Snapshot native free-callable registrations from the active checked
/// artifact. Callable keys and signatures remain explicit; no MIR function ID
/// is synthesized for a native function value.
pub fn active_callable_descriptors(
) -> Result<Vec<NativeCallableBindingDescriptor>, NativeInterfaceError> {
    let active = ACTIVE_NATIVE_INTERFACES.with(|stack| {
        stack.borrow().last().map(|active| {
            (
                active.bindings.clone(),
                active.execution.clone(),
                active.artifact,
            )
        })
    });
    let Some((bindings, execution, artifact)) = active else {
        return Ok(Vec::new());
    };
    let descriptors = bindings.callable_descriptors();
    if descriptors.iter().any(|descriptor| {
        descriptor.identity.execution != execution || descriptor.identity.artifact != artifact
    }) {
        return Err(NativeInterfaceError::ExecutionMismatch);
    }
    Ok(descriptors)
}



/// Snapshot the descriptors belonging to the currently active checked
/// artifact. This is the backend installation surface; callers never infer a
/// private trait name or search MIR for unrelated host bindings.
pub fn active_binding_descriptors(
    program: &MirProgram,
    artifact: MirArtifactId,
) -> Result<Vec<NativeInterfaceBindingDescriptor>, NativeInterfaceError> {
    let execution = program
        .execution_identity(Some(artifact))
        .map_err(|error| NativeInterfaceError::InvalidMetadata(error.to_string()))?;
    let active = ACTIVE_NATIVE_INTERFACES.with(|stack| {
        stack.borrow().last().map(|active| {
            (
                active.bindings.clone(),
                active.execution.clone(),
                active.artifact,
            )
        })
    });
    let Some((bindings, active_execution, active_artifact)) = active else {
        return Err(NativeInterfaceError::MissingBinding);
    };
    if active_execution != execution || active_artifact != artifact {
        return Err(NativeInterfaceError::ExecutionMismatch);
    }
    let descriptors = bindings.method_descriptors();
    if descriptors.iter().any(|descriptor| {
        descriptor.identity.execution != execution || descriptor.identity.artifact != artifact
    }) {
        return Err(NativeInterfaceError::ExecutionMismatch);
    }
    Ok(descriptors)
}

#[cfg(test)]
fn push_scope(
    bindings: Arc<NativeInterfaceBindings>,
    execution: MirExecutionIdentity,
    artifact: MirArtifactId,
) -> NativeInterfaceScope {
    push_scope_with_physical_borrow(bindings, execution, artifact, None)
}

fn push_scope_with_physical_borrow(
    bindings: Arc<NativeInterfaceBindings>,
    execution: MirExecutionIdentity,
    artifact: MirArtifactId,
    physical_borrow_context: Option<Rc<NativeInterfacePhysicalBorrowContext>>,
) -> NativeInterfaceScope {
    let token = Arc::new(());
    let weak_context = physical_borrow_context.as_ref().map(Rc::downgrade);
    ACTIVE_NATIVE_INTERFACES.with(|stack| {
        stack.borrow_mut().push(ActiveNativeInterface {
            token: token.clone(),
            bindings,
            execution,
            artifact,
            physical_borrow_context: weak_context,
        });
    });
    NativeInterfaceScope {
        token: Some(token),
        physical_borrow_context,
        _not_send: PhantomData,
    }
}

impl Drop for NativeInterfaceScope {
    fn drop(&mut self) {
        if let Some(token) = self.token.take() {
            ACTIVE_NATIVE_INTERFACES.with(|stack| {
                let mut stack = stack.borrow_mut();
                if stack
                    .last()
                    .is_some_and(|active| Arc::ptr_eq(&active.token, &token))
                {
                    stack.pop();
                } else if let Some(index) = stack
                    .iter()
                    .rposition(|active| Arc::ptr_eq(&active.token, &token))
                {
                    stack.remove(index);
                }
            });
        }
        drop(self.physical_borrow_context.take());
    }
}

/// Dispatch through the currently active per-artifact native interface set.
/// There is no Prelude fallback: absent or mismatched bindings are errors and
/// the owned call is returned intact in the completion.
pub fn dispatch_active(call: NativeInterfaceCall) -> NativeInterfaceCompletion {
    let active = ACTIVE_NATIVE_INTERFACES.with(|stack| {
        stack.borrow().last().map(|active| {
            (
                active.bindings.clone(),
                active.execution.clone(),
                active.artifact,
            )
        })
    });
    match active {
        Some((bindings, execution, artifact)) => bindings.dispatch(&execution, artifact, call),
        None => NativeInterfaceCompletion {
            outcome: Err(NativeInterfaceError::MissingBinding),
            call,
        },
    }
}
/// Resolve a Source trait-object carrier through the active, artifact-scoped
/// native bindings. `carrier` must be the same retained `MirNativeOwned` Arc
/// carried by `receiver`; raw JIT integers are never interpreted as identity.
pub fn dispatch_active_for_carrier(
    identity: NativeInterfaceIdentity,
    signature: NativeInterfaceSignature,
    receiver_access: MirAccess,
    receiver: MirRuntimeValue,
    carrier: MirNativeOwned,
    arguments: Vec<NativeInterfaceArgument>,
    span: Span,
) -> NativeInterfaceCompletion {
    let active = ACTIVE_NATIVE_INTERFACES.with(|stack| {
        stack.borrow().last().map(|active| {
            (
                active.bindings.clone(),
                active.execution.clone(),
                active.artifact,
            )
        })
    });
    let Some((bindings, execution, artifact)) = active else {
        let mut call = NativeInterfaceCall::new(
            identity,
            signature,
            None,
            receiver_access,
            receiver,
            arguments,
            span,
        );
        call.preflight_error = Some(NativeInterfaceError::MissingBinding);
        return NativeInterfaceCompletion {
            outcome: Err(NativeInterfaceError::MissingBinding),
            call,
        };
    };
    let object = match &receiver {
        MirRuntimeValue::NativeOwned(actual) if actual == &carrier => {
            bindings.object_for_carrier(&identity, &signature, &carrier)
        }
        _ => Err(NativeInterfaceError::ReceiverMismatch),
    };
    let (object, preflight_error) = match object {
        Ok(object) => (Some(object), None),
        Err(error) => (None, Some(error)),
    };
    let mut call = NativeInterfaceCall::new(
        identity,
        signature,
        object,
        receiver_access,
        receiver,
        arguments,
        span,
    );
    if let Some(error) = preflight_error {
        call.preflight_error = Some(error);
    }
    bindings.dispatch(&execution, artifact, call)
}
/// Dispatch a native Fn value by its checked retained carrier in the active
/// artifact scope. A carrier must identify exactly one registered callable.
pub fn dispatch_active_callable_for_carrier(
    signature: NativeCallableSignature,
    carrier: MirNativeOwned,
    arguments: Vec<NativeInterfaceArgument>,
    span: Span,
) -> NativeCallableCompletion {
    let active = ACTIVE_NATIVE_INTERFACES.with(|stack| {
        stack.borrow().last().map(|active| {
            (
                active.bindings.clone(),
                active.execution.clone(),
                active.artifact,
            )
        })
    });
    let Some((bindings, execution, artifact)) = active else {
        let mut call = NativeCallableCall::new(
            None,
            signature,
            None,
            Some(carrier),
            arguments,
            span,
        );
        call.preflight_error = Some(NativeInterfaceError::MissingBinding);
        return NativeCallableCompletion {
            outcome: Err(NativeInterfaceError::MissingBinding),
            call,
        };
    };
    let binding = bindings.binding_for_callable_carrier(&signature, &carrier);
    let (identity, object, preflight_error) = match binding {
        Ok((identity, object)) => (Some(identity), Some(object), None),
        Err(error) => (None, None, Some(error)),
    };
    let mut call = NativeCallableCall::new(
        identity,
        signature,
        object,
        Some(carrier),
        arguments,
        span,
    );
    if let Some(error) = preflight_error {
        call.preflight_error = Some(error);
    }
    bindings.dispatch_callable(&execution, artifact, call)
}

/// Dispatch when the caller already has the exact callable identity.
pub fn dispatch_active_callable(
    identity: NativeCallableIdentity,
    signature: NativeCallableSignature,
    carrier: MirNativeOwned,
    arguments: Vec<NativeInterfaceArgument>,
    span: Span,
) -> NativeCallableCompletion {
    let active = ACTIVE_NATIVE_INTERFACES.with(|stack| {
        stack.borrow().last().map(|active| {
            (
                active.bindings.clone(),
                active.execution.clone(),
                active.artifact,
            )
        })
    });
    let Some((bindings, execution, artifact)) = active else {
        let mut call = NativeCallableCall::new(
            Some(identity),
            signature,
            None,
            Some(carrier),
            arguments,
            span,
        );
        call.preflight_error = Some(NativeInterfaceError::MissingBinding);
        return NativeCallableCompletion {
            outcome: Err(NativeInterfaceError::MissingBinding),
            call,
        };
    };
    let binding = bindings.object_for_callable_carrier(&identity, &signature, &carrier);
    let (object, preflight_error) = match binding {
        Ok(object) => (Some(object), None),
        Err(error) => (None, Some(error)),
    };
    let mut call = NativeCallableCall::new(
        Some(identity),
        signature,
        object,
        Some(carrier),
        arguments,
        span,
    );
    if let Some(error) = preflight_error {
        call.preflight_error = Some(error);
    }
    bindings.dispatch_callable(&execution, artifact, call)
}

/// Resolve the canonical carrier from the typed receiver and dispatch it
/// through the same checked path as callers that already hold the root token.
pub fn dispatch_active_for_identity(
    identity: NativeInterfaceIdentity,
    signature: NativeInterfaceSignature,
    receiver_access: MirAccess,
    receiver: MirRuntimeValue,
    arguments: Vec<NativeInterfaceArgument>,
    span: Span,
) -> NativeInterfaceCompletion {
    let carrier = match &receiver {
        MirRuntimeValue::NativeOwned(carrier) => carrier.clone(),
        _ => {
            let mut call = NativeInterfaceCall::new(
                identity,
                signature,
                None,
                receiver_access,
                receiver,
                arguments,
                span,
            );
            call.preflight_error = Some(NativeInterfaceError::ReceiverMismatch);
            return dispatch_active(call);
        }
    };
    dispatch_active_for_carrier(
        identity,
        signature,
        receiver_access,
        receiver,
        carrier,
        arguments,
        span,
    )
}

/// Whether a trait-object bound names the checked trait `trait_id`. Rust MIR
/// keeps unqualified trait keys, so its bound carries the declared name and
/// that name's stable id; Jet MIR names the bound by the trait's canonical
/// (module-qualified) key, whose stable id is the trait row's own identity.
fn trait_bound_names(bound: &MirNominalRef, trait_id: MirTraitId, name: &str) -> bool {
    bound.id.0 == trait_id.0
        || (bound.name == name
            && bound.id == MirTypeId(jet_foundation::MIR::stable_id("mir-trait", name)))
}

fn validate_call_shape(
    identity: &NativeInterfaceIdentity,
    signature: &NativeInterfaceSignature,
    object: Option<&NativeInterfaceObject>,
    receiver_access: MirAccess,
    receiver: &MirRuntimeValue,
    arguments: &[NativeInterfaceArgument],
) -> Result<(), NativeInterfaceError> {
    if receiver_access != signature.receiver_access {
        return Err(NativeInterfaceError::InvalidCall(
            "trait receiver access disagrees with checked method metadata".to_string(),
        ));
    }
    let object = object.ok_or(NativeInterfaceError::ReceiverMismatch)?;
    let MirRuntimeValue::NativeOwned(carrier) = receiver else {
        return Err(NativeInterfaceError::ReceiverMismatch);
    };
    if !object.matches_root(carrier) {
        return Err(NativeInterfaceError::ReceiverMismatch);
    }
    if !matches!(identity.receiver_type.kind(), MirTypeKind::TraitObject(bounds)
        if bounds.iter().any(|bound| {
            trait_bound_names(bound, identity.trait_ref.id, &identity.trait_ref.name)
        }))
    {
        return Err(NativeInterfaceError::InvalidCall(
            "native receiver type does not carry the exact checked trait bound".to_string(),
        ));
    }
    if arguments.len() != signature.parameters.len() {
        return Err(NativeInterfaceError::InvalidCall(
            "native interface argument count disagrees with checked method metadata".to_string(),
        ));
    }
    for (index, (argument, parameter)) in arguments.iter().zip(&signature.parameters).enumerate() {
        if argument.index != index
            || argument.access != parameter.access
            || argument.ty != parameter.ty
        {
            return Err(NativeInterfaceError::InvalidCall(format!(
                "argument {index} identity, access, or type disagrees with the checked method"
            )));
        }
        validate_writeback_access(parameter.access, argument.writeback).map_err(|_| {
            NativeInterfaceError::InvalidCall(format!(
                "argument {index} writeback contradicts its checked access"
            ))
        })?;
        if argument.value == MirRuntimeValue::Moved {
            return Err(NativeInterfaceError::InvalidCall(format!(
                "native interface argument {index} is already moved"
            )));
        }
        if !runtime_value_matches_type(&argument.value, &argument.ty) {
            return Err(NativeInterfaceError::InvalidCall(format!(
                "native interface argument {index} does not match its checked MIR type"
            )));
        }
    }
    if identity.trait_ref.id.0 == 0 || identity.method_id.0 == 0 {
        return Err(NativeInterfaceError::InvalidCall(
            "native interface call carries a zero checked identity".to_string(),
        ));
    }
    Ok(())
}

fn validate_writeback_access(access: MirAccess, writeback: bool) -> Result<(), ()> {
    if writeback == (access == MirAccess::Write) {
        Ok(())
    } else {
        Err(())
    }
}

fn validate_native_callable_call(
    identity: Option<&NativeCallableIdentity>,
    signature: &NativeCallableSignature,
    object: Option<&NativeInterfaceObject>,
    carrier: Option<&MirNativeOwned>,
    arguments: &[NativeInterfaceArgument],
) -> Result<(), NativeInterfaceError> {
    let identity = identity.ok_or(NativeInterfaceError::MissingBinding)?;
    let object = object.ok_or(NativeInterfaceError::ReceiverMismatch)?;
    let carrier = carrier.ok_or(NativeInterfaceError::ReceiverMismatch)?;
    if !object.matches_root(carrier) {
        return Err(NativeInterfaceError::ReceiverMismatch);
    }
    if identity.key.is_empty() || identity.artifact.0 == 0 {
        return Err(NativeInterfaceError::InvalidCall(
            "native callable carries an empty or zero checked identity".to_string(),
        ));
    }
    let checked = NativeCallableSignature::checked(&identity.callable_type)?;
    if checked != *signature {
        return Err(NativeInterfaceError::SignatureMismatch);
    }
    if arguments.len() != signature.parameters.len() {
        return Err(NativeInterfaceError::InvalidCall(format!(
            "native callable received {} arguments for {} checked parameters",
            arguments.len(),
            signature.parameters.len()
        )));
    }
    for (index, (argument, parameter)) in arguments.iter().zip(&signature.parameters).enumerate() {
        if argument.index != index
            || argument.ty != parameter.ty
            || argument.access != parameter.access
        {
            return Err(NativeInterfaceError::InvalidCall(format!(
                "callable argument {index} identity, access, or type disagrees with its checked Fn parameter"
            )));
        }
        validate_writeback_access(parameter.access, argument.writeback).map_err(|_| {
            NativeInterfaceError::InvalidCall(format!(
                "callable argument {index} writeback contradicts its checked access"
            ))
        })?;
        if argument.value == MirRuntimeValue::Moved
            || !runtime_value_matches_type(&argument.value, &argument.ty)
        {
            return Err(NativeInterfaceError::InvalidCall(format!(
                "callable argument {index} does not contain its checked MIR value"
            )));
        }
    }
    Ok(())
}

pub(crate) fn runtime_value_matches_type(value: &MirRuntimeValue, ty: &MirType) -> bool {
    use jet_foundation::MIR::MirTypeKind as Kind;
    match ty.kind() {
        Kind::Int => matches!(value, MirRuntimeValue::Int(_) | MirRuntimeValue::BigInt(_)),
        Kind::Float => matches!(value, MirRuntimeValue::Float { f32: false, .. }),
        Kind::Bool => matches!(value, MirRuntimeValue::Bool(_)),
        Kind::String => matches!(value, MirRuntimeValue::String(_)),
        Kind::Char => matches!(value, MirRuntimeValue::Char(_)),
        Kind::List(element) => {
            matches!(value, MirRuntimeValue::List(values) if values.iter().all(|value| runtime_value_matches_type(value, element)))
        }
        Kind::FixedList { elem, len } => {
            // A literal length is part of the checked type; the JIT list encoder
            // trusts it for bounds, so a short or long list must not cross here.
            matches!(value, MirRuntimeValue::List(values)
                if len.literal_value().is_none_or(|len| u64::try_from(values.len()) == Ok(len))
                    && values.iter().all(|value| runtime_value_matches_type(value, elem)))
        }
        Kind::Map { value: element, .. } => {
            matches!(value, MirRuntimeValue::Map(values) if values.iter().all(|(_, value)| runtime_value_matches_type(value, element)))
        }
        Kind::Shared(_) => matches!(value, MirRuntimeValue::NativeOwned(_)),
        Kind::Option(inner) => match value {
            MirRuntimeValue::Present(value) => runtime_value_matches_type(value, inner),
            MirRuntimeValue::Absent { element } => element == inner.as_ref(),
            _ => false,
        },
        Kind::Result { ok, err } => match value {
            MirRuntimeValue::Present(value) => runtime_value_matches_type(value, ok),
            MirRuntimeValue::FailedTold(value) => runtime_value_matches_type(value, err),
            _ => false,
        },
        Kind::Fn(_) | Kind::SendFn { .. } => {
            matches!(value, MirRuntimeValue::Closure(_) | MirRuntimeValue::NativeOwned(_))
        }
        Kind::Apply { name, args } => {
            if args.is_empty() && name.name == "Unit" {
                return matches!(value, MirRuntimeValue::Unit);
            }
            match value {
                MirRuntimeValue::Struct { type_name, .. }
                | MirRuntimeValue::Enum { type_name, .. } => type_name == &name.name,
                _ => false,
            }
        }
        Kind::TraitObject(_) => matches!(value, MirRuntimeValue::NativeOwned(_)),
        Kind::Tuple(fields) => {
            let MirRuntimeValue::Struct {
                fields: values, ..
            } = value
            else {
                return false;
            };
            values.len() == fields.len()
                && fields.iter().all(|(name, ty)| {
                    values
                        .iter()
                        .find(|(value_name, _)| value_name == name)
                        .is_some_and(|(_, value)| runtime_value_matches_type(value, ty))
                })
        }
        Kind::IntN { signed, bits } => {
            if *bits == 0 || *bits > 64 {
                false
            } else {
                let (lower, upper) = jet_foundation::AST::int_range(*signed, *bits);
                match value {
                    MirRuntimeValue::Int(number) => {
                        let number = i128::from(*number);
                        number >= lower && number <= upper
                    }
                    MirRuntimeValue::BigInt(number) => number
                        .parse::<i128>()
                        .is_ok_and(|number| number >= lower && number <= upper),
                    _ => false,
                }
            }
        }
        Kind::InlineRange { base, lo, hi } => {
            runtime_value_matches_type(value, base)
                && matches!(value, MirRuntimeValue::Int(number) if number >= lo && number <= hi)
        }
        Kind::Float32 => matches!(value, MirRuntimeValue::Float { f32: true, .. }),
        Kind::Tagged { inner, .. } | Kind::Quantity { base: inner, .. } => {
            runtime_value_matches_type(value, inner)
        }
        Kind::Union(members) => members
            .iter()
            .any(|member| runtime_value_matches_type(value, member)),
        Kind::Measure(_) => matches!(value, MirRuntimeValue::Int(_) | MirRuntimeValue::BigInt(_)),
    }
}

fn same_method(left: &NativeInterfaceIdentity, right: &NativeInterfaceIdentity) -> bool {
    left.execution == right.execution
        && left.artifact == right.artifact
        && left.trait_ref == right.trait_ref
        && left.method_id == right.method_id
        && left.method_name == right.method_name
        && left.receiver_type == right.receiver_type
}

fn same_callable(left: &NativeCallableIdentity, right: &NativeCallableIdentity) -> bool {
    left.execution == right.execution
        && left.artifact == right.artifact
        && left.key == right.key
        && left.callable_type == right.callable_type
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writeback_requires_checked_write_access() {
        assert!(validate_writeback_access(MirAccess::Read, true).is_err());
        assert!(validate_writeback_access(MirAccess::Read, false).is_ok());
        assert!(validate_writeback_access(MirAccess::Write, true).is_ok());
        assert!(validate_writeback_access(MirAccess::Write, false).is_err());
    }

    #[test]
    fn native_receiver_carrier_preserves_exact_root_identity() {
        let bindings = NativeInterfaceBindings::new();
        let object = bindings
            .create_object(String::from("adapter-root"))
            .expect("object identity is available");
        let MirRuntimeValue::NativeOwned(carrier) = object.as_runtime_value() else {
            panic!("native interface receiver was not a native-owned value");
        };
        assert!(object.matches_root(&carrier));
        assert!(object.matches_root(&carrier.clone()));
        assert!(!object.matches_root(&MirNativeOwned::new(String::from("adapter-root"))));
    }

    use jet_foundation::MIR::{
        MirArtifactBuildMode, MirArtifactIdentity, MirArtifactKind, MirArtifactTarget,
        MirCallMetadata, MirFunctionSignature, MirMeasure, MirNominalRef, MirOwnership,
        MirProgramIdentity, MIR_SCHEMA_VERSION,
    };
    use std::sync::atomic::{AtomicBool, Ordering};

    const ARTIFACT: MirArtifactId = MirArtifactId(7);

    fn execution() -> MirExecutionIdentity {
        MirExecutionIdentity {
            schema_version: 1,
            artifact: MirArtifactIdentity {
                schema_version: 1,
                mir_schema_version: MIR_SCHEMA_VERSION,
                program_digest: [0; 32],
                package_identity: "test-package".to_string(),
                artifact: ARTIFACT,
                name: "main".to_string(),
                kind: MirArtifactKind::NativeExecutable,
                target: MirArtifactTarget::Cranelift,
                mode: MirArtifactBuildMode::Dev,
                provider_identity: String::new(),
                closure_identity: String::new(),
                artifact_identity: String::new(),
                program_identity: std::sync::Arc::new(MirProgramIdentity::unavailable(
                    String::new(),
                    Vec::new(),
                )),
            },
        }
    }

    fn int() -> MirType {
        MirType::from_kind(MirTypeKind::Int)
    }

    fn unit() -> MirType {
        MirType::from_kind(MirTypeKind::Apply {
            name: MirNominalRef {
                id: MirTypeId(jet_foundation::MIR::stable_id("mir-type", "Unit")),
                name: "Unit".to_string(),
            },
            args: Vec::new(),
        })
    }
    fn interface_type() -> (MirTraitRef, MirType) {
        let name = "Sink".to_string();
        let id = jet_foundation::MIR::stable_id("mir-trait", &name);
        (
            MirTraitRef {
                id: MirTraitId(id),
                name: name.clone(),
            },
            MirType::from_kind(MirTypeKind::TraitObject(vec![MirNominalRef {
                id: MirTypeId(id),
                name,
            }])),
        )
    }


    fn callable_with_parameter_types(
        key: &str,
        parameter_types: &[MirType],
        conventions: &[MirAccess],
    ) -> (NativeCallableIdentity, NativeCallableSignature) {
        let callable_type = MirType::from_kind(MirTypeKind::Fn(MirFunctionSignature {
            params: parameter_types.to_vec(),
            ret: Some(Box::new(int())),
            effect_bound: None,
            param_contract: None,
            call_metadata: Some(MirCallMetadata {
                conventions: conventions.to_vec(),
                ..MirCallMetadata::default()
            }),
            return_view_provenance: None,
        }));
        let signature = NativeCallableSignature::checked(&callable_type).expect("checked Fn type");
        (
            NativeCallableIdentity {
                execution: execution(),
                artifact: ARTIFACT,
                key: key.to_string(),
                callable_type,
            },
            signature,
        )
    }

    fn callable(key: &str, conventions: &[MirAccess]) -> (NativeCallableIdentity, NativeCallableSignature) {
        let parameter_types = vec![int(); conventions.len()];
        callable_with_parameter_types(key, &parameter_types, conventions)
    }

    type TestNativeCallableWrapper = Rc<RefCell<Option<Box<dyn FnMut()>>>>;

    fn register_callable_root<T: Any + Send + Sync>(
        root: T,
        key: &str,
    ) -> (
        Arc<NativeInterfaceBindings>,
        NativeInterfaceObject,
        NativeCallableIdentity,
        NativeCallableSignature,
        MirNativeOwned,
    ) {
        let bindings = Arc::new(NativeInterfaceBindings::new());
        let object = bindings.create_object(root).expect("callable root object");
        let (identity, signature) = callable(key, &[MirAccess::Read]);
        bindings
            .bind_callable(
                object.clone(),
                identity.clone(),
                signature.clone(),
                Arc::new(|_object, _call| Ok(MirRuntimeValue::Int(1))),
            )
            .expect("checked callable binds");
        let carrier = object.clone_root();
        (bindings, object, identity, signature, carrier)
    }

    fn make_test_native_callable_wrapper(
        bindings: &Arc<NativeInterfaceBindings>,
        identity: NativeCallableIdentity,
        carrier: MirNativeOwned,
        invoked: Rc<Cell<usize>>,
    ) -> Result<TestNativeCallableWrapper, NativeInterfaceError> {
        bindings.create_native_callable_rc_wrapper(identity, carrier, move |registration| {
            let callback: Box<dyn FnMut()> = Box::new(move || {
                let _ = registration.identity();
                let _ = registration.carrier();
                invoked.set(invoked.get() + 1);
            });
            Rc::new(RefCell::new(Some(callback)))
        })
    }

    fn shared() -> MirType {
        MirType::from_kind(MirTypeKind::Shared(Box::new(int())))
    }

    fn argument(index: usize, access: MirAccess, value: MirRuntimeValue) -> NativeInterfaceArgument {
        NativeInterfaceArgument {
            index,
            ty: int(),
            access,
            value,
            writeback: access == MirAccess::Write,
        }
    }

    fn owned_argument(index: usize, carrier: MirNativeOwned) -> NativeInterfaceArgument {
        NativeInterfaceArgument {
            index,
            ty: shared(),
            access: MirAccess::Move,
            value: MirRuntimeValue::NativeOwned(carrier),
            writeback: false,
        }
    }

    fn callable_handler(
        body: impl Fn(&mut NativeCallableCall) -> Result<MirRuntimeValue, String> + Send + Sync + 'static,
    ) -> NativeCallableHandler {
        Arc::new(move |_object: &NativeInterfaceObject, call: &mut NativeCallableCall| body(call))
    }

    fn carrier_of(object: &NativeInterfaceObject) -> MirNativeOwned {
        object.clone_root()
    }

    #[test]
    fn native_callable_rc_wrapper_owns_registration_after_factory_scope_and_releases_root() {
        let root = Arc::new(());
        let weak_root = Arc::downgrade(&root);
        let invoked = Rc::new(Cell::new(0));
        let (wrapper, expected_identity, expected_object) = {
            let (bindings, object, identity, _signature, carrier) =
                register_callable_root(root.clone(), "rc.factory-scope");
            let expected_object = object.id();
            let wrapper = make_test_native_callable_wrapper(
                &bindings,
                identity.clone(),
                carrier,
                invoked.clone(),
            )
            .expect("checked callable wrapper associates");
            drop(object);
            drop(bindings);
            (wrapper, identity, expected_object)
        };
        drop(root);

        assert!(weak_root.upgrade().is_some());
        let (identity, carrier) =
            native_callable_rc_association(&wrapper).expect("wrapper retains registration");
        assert_eq!(identity, expected_identity);
        assert_eq!(
            carrier
                .downcast_ref::<NativeInterfaceStaticCarrier>()
                .expect("native callable has the exact registered carrier")
                .id,
            expected_object
        );
        wrapper
            .borrow_mut()
            .as_mut()
            .expect("local callable remains installed")();
        assert_eq!(invoked.get(), 1);

        drop(carrier);
        drop(wrapper);
        assert!(weak_root.upgrade().is_none());
    }

    #[test]
    fn native_callable_rc_lookup_separates_types_and_expires_with_wrapper() {
        let (bindings, object, identity, _signature, carrier) =
            register_callable_root(String::from("rc-type-root"), "rc.type-separation");
        let wrapper = make_test_native_callable_wrapper(
            &bindings,
            identity,
            carrier.clone(),
            Rc::new(Cell::new(0)),
        )
        .expect("checked callable wrapper associates");
        assert!(native_callable_rc_association(&wrapper).is_ok());

        let wrong_type = Rc::new(Cell::new(0));
        assert!(matches!(
            native_callable_rc_association(&wrong_type),
            Err(NativeInterfaceError::MissingBinding)
        ));

        let stale = Rc::downgrade(&wrapper);
        drop(wrapper);
        assert!(stale.upgrade().is_none());
        let replacement: TestNativeCallableWrapper = Rc::new(RefCell::new(None));
        assert!(matches!(
            native_callable_rc_association(&replacement),
            Err(NativeInterfaceError::MissingBinding)
        ));

        drop(replacement);
        drop(carrier);
        drop(object);
        drop(bindings);
    }

    #[test]
    fn native_callable_rc_association_rejects_interface_carriers() {
        let bindings = Arc::new(NativeInterfaceBindings::new());
        let interface_object = bindings
            .create_object(String::from("interface-only-root"))
            .expect("interface root");
        let (trait_ref, receiver_type) = interface_type();
        bindings
            .bind(
                interface_object.clone(),
                NativeInterfaceIdentity {
                    execution: execution(),
                    artifact: ARTIFACT,
                    trait_ref,
                    method_id: MirTraitMethodId(1),
                    method_name: "receive".to_string(),
                    receiver_type,
                },
                NativeInterfaceSignature {
                    receiver_access: MirAccess::Read,
                    parameters: Vec::new(),
                    return_type: unit(),
                    failure: MirFailureCarrier::Infallible,
                },
                Arc::new(|_object, _call| Ok(MirRuntimeValue::Unit)),
            )
            .expect("interface binding");
        let (identity, _signature) = callable("not-an-interface", &[MirAccess::Read]);

        assert!(matches!(
            bindings.create_native_callable_rc_wrapper(
                identity,
                interface_object.clone_root(),
                |_registration| Rc::new(Cell::new(0)),
            ),
            Err(NativeInterfaceError::MissingBinding)
        ));
    }

    #[test]
    fn native_callable_rc_decode_builds_fresh_thread_local_wrapper_for_same_root() {
        let (bindings, object, identity, _signature, carrier) =
            register_callable_root(String::from("rc-worker-root"), "rc.worker");
        let main_wrapper = make_test_native_callable_wrapper(
            &bindings,
            identity.clone(),
            carrier.clone(),
            Rc::new(Cell::new(0)),
        )
        .expect("main-thread callable wrapper");
        assert_eq!(
            native_callable_rc_association(&main_wrapper)
                .expect("main wrapper association")
                .0,
            identity
        );

        let worker_bindings = bindings.clone();
        let worker_identity = identity.clone();
        let worker_carrier = carrier.clone();
        let (decoded_identity, decoded_carrier) = std::thread::spawn(move || {
            let wrapper = make_test_native_callable_wrapper(
                &worker_bindings,
                worker_identity.clone(),
                worker_carrier,
                Rc::new(Cell::new(0)),
            )
            .expect("worker creates its own Rc wrapper");
            let association =
                native_callable_rc_association(&wrapper).expect("worker-local association");
            assert_eq!(association.0, worker_identity);
            drop(wrapper);
            association
        })
        .join()
        .expect("worker completes");

        assert_eq!(decoded_identity, identity);
        assert_eq!(decoded_carrier, carrier);
        drop(decoded_carrier);
        drop(main_wrapper);
        drop(carrier);
        drop(object);
        drop(bindings);
    }

    /// An interface call whose only purpose is to exercise the transfer ledger
    /// directly; its receiver and signature are internally consistent so the
    /// completion path (`finish`) can be driven without a MIR program.
    fn interface_call(bindings: &NativeInterfaceBindings, arguments: Vec<NativeInterfaceArgument>) -> NativeInterfaceCall {
        let (trait_ref, receiver_type) = interface_type();
        let object = bindings.create_object(String::from("sink-root")).expect("object identity");
        let receiver = object.as_runtime_value();
        let parameters = arguments
            .iter()
            .map(|argument| MirParam {
                index: argument.index,
                name: format!("p{}", argument.index),
                span: Span::new(0, 0),
                ty: argument.ty.clone(),
                access: argument.access,
                ownership: MirOwnership::copy(),
                public_label: String::new(),
                variadic: false,
                default_present: false,
            })
            .collect();
        NativeInterfaceCall::new(
            NativeInterfaceIdentity {
                execution: execution(),
                artifact: ARTIFACT,
                trait_ref,
                method_id: MirTraitMethodId(1),
                method_name: "put".to_string(),
                receiver_type,
            },
            NativeInterfaceSignature {
                receiver_access: MirAccess::Read,
                parameters,
                return_type: unit(),
                failure: MirFailureCarrier::Infallible,
            },
            Some(object),
            MirAccess::Read,
            receiver,
            arguments,
            Span::new(0, 0),
        )
    }

    #[test]
    fn callback_instances_share_checked_templates_without_sharing_session_roots() {
        let bindings = Arc::new(NativeInterfaceBindings::new());
        let template = bindings
            .create_object(String::from("template"))
            .expect("template object");
        let (trait_ref, receiver_type) = interface_type();
        let identity = NativeInterfaceIdentity {
            execution: execution(),
            artifact: ARTIFACT,
            trait_ref,
            method_id: MirTraitMethodId(1),
            method_name: "session".to_string(),
            receiver_type: receiver_type.clone(),
        };
        let signature = NativeInterfaceSignature {
            receiver_access: MirAccess::Read,
            parameters: Vec::new(),
            return_type: unit(),
            failure: MirFailureCarrier::Infallible,
        };
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_by_handler = seen.clone();
        let bindings_by_handler = Arc::downgrade(&bindings);
        bindings
            .bind(
                template.clone(),
                identity.clone(),
                signature.clone(),
                Arc::new(move |object, _call| {
                    let session = object
                        .root::<String>()
                        .cloned()
                        .ok_or_else(|| "callback context root has the wrong type".to_string())?;
                    let descriptors = bindings_by_handler
                        .upgrade()
                        .ok_or_else(|| "binding set was released during dispatch".to_string())?
                        .method_descriptors()
                        .len();
                    seen_by_handler
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push((session, descriptors));
                    Ok(MirRuntimeValue::Unit)
                }),
            )
            .expect("template method binds");

        let template_carrier = template.clone_root();
        let first = bindings
            .instantiate_interface_for_carrier(
                &template_carrier,
                &receiver_type,
                String::from("session-one"),
            )
            .expect("first callback instance");
        let second = bindings
            .instantiate_interface_for_carrier(
                &template_carrier,
                &receiver_type,
                String::from("session-two"),
            )
            .expect("second callback instance");
        assert_ne!(first.id(), second.id());
        assert_eq!(bindings.method_descriptors().len(), 3);
        let _scope = push_scope(bindings.clone(), execution(), ARTIFACT);

        for instance in [&first, &second] {
            let receiver = instance.as_runtime_value();
            let completion = dispatch_active_for_carrier(
                identity.clone(),
                signature.clone(),
                MirAccess::Read,
                receiver,
                instance.clone_root(),
                Vec::new(),
                Span::new(0, 0),
            );
            assert_eq!(completion.outcome(), Ok(&MirRuntimeValue::Unit));
            drop(completion);
        }
        assert_eq!(
            *seen.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![
                (String::from("session-one"), 3),
                (String::from("session-two"), 3),
            ]
        );
        drop(first);
        drop(second);
        assert_eq!(bindings.method_descriptors().len(), 1);
    }

    #[test]
    fn callable_preflight_failure_never_reaches_the_handler() {
        let bindings = Arc::new(NativeInterfaceBindings::new());
        let object = bindings.create_object(String::from("service")).expect("object identity");
        let (identity, signature) = callable("bump", &[MirAccess::Write, MirAccess::Read]);
        let invoked = Arc::new(AtomicBool::new(false));
        let seen = invoked.clone();
        bindings
            .bind_callable(
                object.clone(),
                identity.clone(),
                signature.clone(),
                callable_handler(move |_call| {
                    seen.store(true, Ordering::SeqCst);
                    Ok(MirRuntimeValue::Int(0))
                }),
            )
            .expect("callable binds");
        let _scope = push_scope(bindings.clone(), execution(), ARTIFACT);

        // Read access (and no writeback channel) where the checked Fn says Write.
        let completion = dispatch_active_callable(
            identity.clone(),
            signature.clone(),
            carrier_of(&object),
            vec![
                argument(0, MirAccess::Read, MirRuntimeValue::Int(3)),
                argument(1, MirAccess::Read, MirRuntimeValue::Int(4)),
            ],
            Span::new(0, 0),
        );
        assert!(matches!(completion.outcome(), Err(NativeInterfaceError::InvalidCall(_))));
        assert!(!invoked.load(Ordering::SeqCst));
        assert_eq!(completion.call().argument(0).expect("argument").value, MirRuntimeValue::Int(3));

        // A checked Read row cannot obtain a writeback channel by flag alone.
        let mut smuggled = argument(1, MirAccess::Read, MirRuntimeValue::Int(4));
        smuggled.writeback = true;
        let completion = dispatch_active_callable(
            identity.clone(),
            signature.clone(),
            carrier_of(&object),
            vec![argument(0, MirAccess::Write, MirRuntimeValue::Int(3)), smuggled],
            Span::new(0, 0),
        );
        assert!(matches!(completion.outcome(), Err(NativeInterfaceError::InvalidCall(_))));
        assert!(!invoked.load(Ordering::SeqCst));

        // Wrong checked value type.
        let completion = dispatch_active_callable(
            identity.clone(),
            signature.clone(),
            carrier_of(&object),
            vec![
                argument(0, MirAccess::Write, MirRuntimeValue::String("3".to_string())),
                argument(1, MirAccess::Read, MirRuntimeValue::Int(4)),
            ],
            Span::new(0, 0),
        );
        assert!(matches!(completion.outcome(), Err(NativeInterfaceError::InvalidCall(_))));
        assert!(!invoked.load(Ordering::SeqCst));

        // The well-formed call still runs.
        let completion = dispatch_active_callable(
            identity,
            signature,
            carrier_of(&object),
            vec![
                argument(0, MirAccess::Write, MirRuntimeValue::Int(3)),
                argument(1, MirAccess::Read, MirRuntimeValue::Int(4)),
            ],
            Span::new(0, 0),
        );
        assert_eq!(completion.outcome(), Ok(&MirRuntimeValue::Int(0)));
        assert!(invoked.load(Ordering::SeqCst));
    }

    #[test]
    fn callable_writeback_reborrow_persists_through_result_errors_and_panics() {
        let bindings = Arc::new(NativeInterfaceBindings::new());
        let object = bindings.create_object(String::from("writeback-service")).expect("object identity");
        let (identity, signature) = callable("writeback", &[MirAccess::Write]);
        let behavior = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let handler_behavior = behavior.clone();
        bindings
            .bind_callable(
                object.clone(),
                identity.clone(),
                signature.clone(),
                callable_handler(move |call| {
                    let behavior = handler_behavior.load(Ordering::SeqCst);
                    let borrow_result = call
                        .with_writeback_argument(0, |value| -> Result<(), &'static str> {
                            let MirRuntimeValue::Int(value) = value else {
                                panic!("checked writeback argument is not an integer");
                            };
                            *value += 1;
                            match behavior {
                                1 => Err("writeback callback failed"),
                                2 => panic!("writeback callback panicked"),
                                _ => Ok(()),
                            }
                        })
                        .map_err(|error| error.to_string())?;
                    if let Err(error) = borrow_result {
                        return Err(error.to_string());
                    }
                    Ok(MirRuntimeValue::Int(0))
                }),
            )
            .expect("callable binds");
        let _scope = push_scope(bindings.clone(), execution(), ARTIFACT);

        let dispatch = || {
            dispatch_active_callable(
                identity.clone(),
                signature.clone(),
                carrier_of(&object),
                vec![argument(0, MirAccess::Write, MirRuntimeValue::Int(10))],
                Span::new(0, 0),
            )
        };

        let success = dispatch();
        assert_eq!(success.outcome(), Ok(&MirRuntimeValue::Int(0)));
        assert_eq!(
            success.call().argument(0).expect("writeback argument").value,
            MirRuntimeValue::Int(11)
        );

        behavior.store(1, Ordering::SeqCst);
        let failed = dispatch();
        assert!(matches!(
            failed.outcome(),
            Err(NativeInterfaceError::Handler(_))
        ));
        assert_eq!(
            failed.call().argument(0).expect("writeback argument").value,
            MirRuntimeValue::Int(11)
        );

        behavior.store(2, Ordering::SeqCst);
        let panicked = dispatch();
        assert!(matches!(
            panicked.outcome(),
            Err(NativeInterfaceError::HandlerPanicked(_))
        ));
        assert_eq!(
            panicked.call().argument(0).expect("writeback argument").value,
            MirRuntimeValue::Int(11)
        );
    }

    #[test]
    fn interface_writeback_reborrow_is_scoped_and_rejects_read_and_move() {
        let bindings = NativeInterfaceBindings::new();
        let mut write_call = interface_call(
            &bindings,
            vec![argument(0, MirAccess::Write, MirRuntimeValue::Int(5))],
        );
        write_call
            .with_writeback_argument(0, |value| {
                let MirRuntimeValue::Int(value) = value else {
                    panic!("checked writeback argument is not an integer");
                };
                *value = 9;
            })
            .expect("checked Write argument is reborrowed");
        assert_eq!(
            write_call.argument(0).expect("writeback argument").value,
            MirRuntimeValue::Int(9)
        );
        assert_eq!(
            write_call.finish(Ok(MirRuntimeValue::Unit)),
            Ok(MirRuntimeValue::Unit)
        );

        let mut read_call = interface_call(
            &bindings,
            vec![argument(0, MirAccess::Read, MirRuntimeValue::Int(5))],
        );
        let read_closure_ran = Cell::new(false);
        assert!(matches!(
            read_call.with_writeback_argument(0, |_value| read_closure_ran.set(true)),
            Err(NativeInterfaceError::InvalidCall(_))
        ));
        assert!(!read_closure_ran.get());

        let move_carrier = MirNativeOwned::new(String::from("move-value"));
        let original_move = MirRuntimeValue::NativeOwned(move_carrier.clone());
        let mut move_call = interface_call(
            &bindings,
            vec![owned_argument(0, move_carrier)],
        );
        let move_closure_ran = Cell::new(false);
        assert!(matches!(
            move_call.with_writeback_argument(0, |_value| move_closure_ran.set(true)),
            Err(NativeInterfaceError::InvalidCall(_))
        ));
        assert!(!move_closure_ran.get());
        assert_eq!(move_call.argument(0).expect("Move argument").value, original_move);
        assert_eq!(
            move_call.finish(Ok(MirRuntimeValue::Unit)),
            Err(NativeInterfaceError::UnresolvedTransfers(vec![0]))
        );
    }

    #[test]
    fn callable_writeback_reborrow_rejects_read_and_move_without_detaching_values() {
        let bindings = NativeInterfaceBindings::new();
        let object = bindings.create_object(String::from("callable-writeback")).expect("object identity");

        let (read_identity, read_signature) = callable("readonly", &[MirAccess::Read]);
        let mut read_call = NativeCallableCall::new(
            Some(read_identity),
            read_signature,
            Some(object.clone()),
            Some(carrier_of(&object)),
            vec![argument(0, MirAccess::Read, MirRuntimeValue::Int(3))],
            Span::new(0, 0),
        );
        let read_closure_ran = Cell::new(false);
        assert!(matches!(
            read_call.with_writeback_argument(0, |_value| read_closure_ran.set(true)),
            Err(NativeInterfaceError::InvalidCall(_))
        ));
        assert!(!read_closure_ran.get());

        let (move_identity, move_signature) = callable("move", &[MirAccess::Move]);
        let mut move_call = NativeCallableCall::new(
            Some(move_identity),
            move_signature,
            Some(object.clone()),
            Some(carrier_of(&object)),
            vec![argument(0, MirAccess::Move, MirRuntimeValue::Int(7))],
            Span::new(0, 0),
        );
        let original = move_call.argument(0).expect("Move argument").value.clone();
        let move_closure_ran = Cell::new(false);
        assert!(matches!(
            move_call.with_writeback_argument(0, |_value| move_closure_ran.set(true)),
            Err(NativeInterfaceError::InvalidCall(_))
        ));
        assert!(!move_closure_ran.get());
        assert_eq!(move_call.argument(0).expect("Move argument").value, original);
        assert_eq!(
            move_call.finish(Ok(MirRuntimeValue::Int(0))),
            Err(NativeInterfaceError::UnresolvedTransfers(vec![0]))
        );
    }

    #[test]
    fn callable_writeback_reborrow_preserves_unresolved_move_obligation() {
        let bindings = NativeInterfaceBindings::new();
        let object = bindings.create_object(String::from("writeback-with-move")).expect("object identity");
        let (identity, signature) = callable_with_parameter_types(
            "writeback-with-move",
            &[int(), shared()],
            &[MirAccess::Write, MirAccess::Move],
        );
        let move_carrier = MirNativeOwned::new(String::from("unresolved-move"));
        let original_move = MirRuntimeValue::NativeOwned(move_carrier.clone());
        let mut call = NativeCallableCall::new(
            Some(identity),
            signature,
            Some(object.clone()),
            Some(carrier_of(&object)),
            vec![
                argument(0, MirAccess::Write, MirRuntimeValue::Int(4)),
                owned_argument(1, move_carrier),
            ],
            Span::new(0, 0),
        );

        call.with_writeback_argument(0, |value| {
            let MirRuntimeValue::Int(value) = value else {
                panic!("checked writeback argument is not an integer");
            };
            *value = 8;
        })
        .expect("checked Write argument is reborrowed");
        assert_eq!(
            call.finish(Ok(MirRuntimeValue::Int(0))),
            Err(NativeInterfaceError::UnresolvedTransfers(vec![1]))
        );
        assert_eq!(call.argument(0).expect("writeback argument").value, MirRuntimeValue::Int(8));
        assert_eq!(call.argument(1).expect("Move argument").value, original_move);
    }

    #[test]
    fn callable_root_and_signature_identify_exactly_one_function_value() {
        let bindings = Arc::new(NativeInterfaceBindings::new());
        let shared = bindings.create_object(String::from("service")).expect("object identity");
        let (alpha, signature) = callable("alpha", &[MirAccess::Read]);
        let (beta, _) = callable("beta", &[MirAccess::Read]);
        bindings
            .bind_callable(
                shared.clone(),
                alpha.clone(),
                signature.clone(),
                callable_handler(|_call| Ok(MirRuntimeValue::Int(1))),
            )
            .expect("first function binds");

        // Same key again is a duplicate regardless of root.
        let other_root = bindings.create_object(String::from("service")).expect("object identity");
        assert_eq!(
            bindings
                .bind_callable(
                    other_root.clone(),
                    alpha.clone(),
                    signature.clone(),
                    callable_handler(|_call| Ok(MirRuntimeValue::Int(1))),
                )
                .err(),
            Some(NativeInterfaceError::DuplicateBinding)
        );

        // A second same-shaped function on the same root would be unreachable by
        // carrier; it is refused before any value is exposed.
        assert_eq!(
            bindings
                .bind_callable(
                    shared.clone(),
                    beta.clone(),
                    signature.clone(),
                    callable_handler(|_call| Ok(MirRuntimeValue::Int(2))),
                )
                .err(),
            Some(NativeInterfaceError::CallableRootCollision {
                key: "beta".to_string(),
                object: shared.id(),
            })
        );
        assert_eq!(bindings.callable_descriptors().len(), 1);

        // The same function on its own root is fine, and each carrier reaches
        // exactly its own handler.
        bindings
            .bind_callable(
                other_root.clone(),
                beta,
                signature.clone(),
                callable_handler(|_call| Ok(MirRuntimeValue::Int(2))),
            )
            .expect("distinct root binds");
        let _scope = push_scope(bindings.clone(), execution(), ARTIFACT);
        let arguments = || vec![argument(0, MirAccess::Read, MirRuntimeValue::Int(0))];
        let first = dispatch_active_callable_for_carrier(signature.clone(), carrier_of(&shared), arguments(), Span::new(0, 0));
        assert_eq!(first.outcome(), Ok(&MirRuntimeValue::Int(1)));
        assert_eq!(first.call().identity().map(|identity| identity.key.as_str()), Some("alpha"));
        let second = dispatch_active_callable_for_carrier(signature.clone(), carrier_of(&other_root), arguments(), Span::new(0, 0));
        assert_eq!(second.outcome(), Ok(&MirRuntimeValue::Int(2)));
        assert_eq!(second.call().identity().map(|identity| identity.key.as_str()), Some("beta"));

        // An unbound root reports the missing binding itself, not the shape
        // error that follows from having no object.
        let unbound = bindings.create_object(String::from("service")).expect("object identity");
        let missing = dispatch_active_callable_for_carrier(signature, carrier_of(&unbound), arguments(), Span::new(0, 0));
        assert_eq!(missing.outcome(), Err(&NativeInterfaceError::MissingBinding));
        assert_eq!(missing.call().argument(0).expect("argument").value, MirRuntimeValue::Int(0));
    }

    #[test]
    fn static_binding_metadata_does_not_keep_a_physical_carrier_alive() {
        let bindings = Arc::new(NativeInterfaceBindings::new());
        let object = bindings
            .create_object(String::from("service"))
            .expect("object identity");
        let (identity, signature) = callable("temporary", &[MirAccess::Read]);
        bindings
            .bind_callable(
                object.clone(),
                identity.clone(),
                signature.clone(),
                callable_handler(|_call| Ok(MirRuntimeValue::Int(1))),
            )
            .expect("callable binds");
        drop(object);

        assert!(bindings.callable_descriptors().is_empty());
        let _scope = push_scope(bindings.clone(), execution(), ARTIFACT);
        let completion = bindings.dispatch_callable_for_scope(
            identity,
            signature,
            vec![argument(0, MirAccess::Read, MirRuntimeValue::Int(0))],
            Span::new(0, 0),
        );
        assert_eq!(
            completion.outcome(),
            Err(&NativeInterfaceError::MissingBinding)
        );
    }

    #[test]
    fn interface_template_metadata_does_not_retain_its_physical_carrier() {
        let bindings = NativeInterfaceBindings::new();
        let object = bindings
            .create_object(String::from("template"))
            .expect("template object");
        let (trait_ref, receiver_type) = interface_type();
        let identity = NativeInterfaceIdentity {
            execution: execution(),
            artifact: ARTIFACT,
            trait_ref,
            method_id: MirTraitMethodId(1),
            method_name: "session".to_string(),
            receiver_type,
        };
        let signature = NativeInterfaceSignature {
            receiver_access: MirAccess::Read,
            parameters: Vec::new(),
            return_type: unit(),
            failure: MirFailureCarrier::Infallible,
        };
        bindings
            .bind(
                object.clone(),
                identity,
                signature,
                Arc::new(|_object, _call| Ok(MirRuntimeValue::Unit)),
            )
            .expect("interface template binds");
        drop(object);

        assert!(bindings.method_descriptors().is_empty());
    }

    #[test]
    fn rejected_offer_survives_error_conversion_while_native_owned_source_is_reclaimed() {
        let bindings = NativeInterfaceBindings::new();
        let original_carrier = MirNativeOwned::new(String::from("source"));
        let original_value = MirRuntimeValue::NativeOwned(original_carrier.clone());
        let mut call = interface_call(&bindings, vec![owned_argument(0, original_carrier)]);
        let transfer = call.take_owned_argument(0).expect("move argument detaches");
        let value = transfer.take_value().expect("native-owned source is guarded");
        let error = call
            .return_owned_argument(transfer, value, MirRuntimeValue::Bool(true))
            .map_err(NativeInterfaceTransferRejection::into_error)
            .expect_err("a value of the wrong checked type is rejected");
        assert!(matches!(error, NativeInterfaceError::InvalidCall(_)));
        assert_eq!(call.rejected_transfers().len(), 1);
        let offer = call.rejected_transfers()[0].clone();
        let alias = offer.clone();
        assert_eq!(offer.with_value(|_| alias.take_value()), Some(None));
        assert_eq!(
            offer.with_value(|value| value.clone()),
            Some(MirRuntimeValue::Bool(true))
        );
        assert_eq!(
            call.rejected_transfers()[0].with_value(|value| value.clone()),
            Some(MirRuntimeValue::Bool(true))
        );
        assert!(call.transfers().is_empty());
        assert_eq!(call.pending_transfers().collect::<Vec<_>>(), vec![0]);

        let outcome = call.finish(Ok(MirRuntimeValue::Unit));
        assert_eq!(
            outcome,
            Err(NativeInterfaceError::UnresolvedTransfers(vec![0]))
        );
        assert_eq!(call.arguments()[0].value, original_value);
        assert_eq!(
            call.rejected_transfers()[0].take_value(),
            Some(MirRuntimeValue::Bool(true))
        );
        assert_eq!(call.rejected_transfers()[0].take_value(), None);
    }

    #[test]
    fn returning_a_native_owned_move_commits_to_its_argument_slot() {
        let bindings = NativeInterfaceBindings::new();
        let original_carrier = MirNativeOwned::new(String::from("source"));
        let returned_carrier = MirNativeOwned::new(String::from("returned"));
        let returned_value = MirRuntimeValue::NativeOwned(returned_carrier.clone());
        let mut call = interface_call(&bindings, vec![owned_argument(0, original_carrier)]);
        let transfer = call.take_owned_argument(0).expect("move argument detaches");
        let value = transfer.take_value().expect("native-owned source is guarded");
        call.return_owned_argument(transfer, value, returned_value.clone())
            .expect("checked returned value commits");

        assert_eq!(call.arguments()[0].value, returned_value);
        assert_eq!(
            call.transfers(),
            &[NativeInterfaceTransferReceipt {
                argument: 0,
                disposition: NativeInterfaceTransferDisposition::Returned,
            }]
        );
        assert!(call.pending_transfers().next().is_none());
        assert_eq!(call.finish(Ok(MirRuntimeValue::Unit)), Ok(MirRuntimeValue::Unit));
    }

    #[test]
    fn dropping_an_uncommitted_guard_restores_exact_native_owned_value() {
        let bindings = NativeInterfaceBindings::new();
        let first_carrier = MirNativeOwned::new(String::from("first"));
        let first_value = MirRuntimeValue::NativeOwned(first_carrier.clone());
        let second_carrier = MirNativeOwned::new(String::from("second"));
        let second_value = MirRuntimeValue::NativeOwned(second_carrier.clone());
        let mut call = interface_call(
            &bindings,
            vec![
                owned_argument(0, first_carrier),
                owned_argument(1, second_carrier),
            ],
        );
        drop(call.take_owned_argument(0).expect("first detaches"));
        let transfer = call.take_owned_argument(1).expect("second detaches");
        let value = transfer.take_value().expect("second is leased");
        assert_eq!(
            value.with_value(|value| value.clone()),
            Ok(second_value.clone())
        );
        drop(value);

        let outcome = call.finish(Ok(MirRuntimeValue::Unit));
        assert_eq!(
            outcome,
            Err(NativeInterfaceError::UnresolvedTransfers(vec![0, 1]))
        );
        assert_eq!(call.arguments()[0].value, first_value);
        assert_eq!(call.arguments()[1].value, second_value);
        assert_eq!(call.pending_transfers().collect::<Vec<_>>(), vec![0, 1]);
        assert!(call.transfers().is_empty());
    }

    #[test]
    fn taken_native_owned_move_is_recovered_when_consumer_errors_before_commit() {
        let bindings = NativeInterfaceBindings::new();
        let original_carrier = MirNativeOwned::new(String::from("source"));
        let original_value = MirRuntimeValue::NativeOwned(original_carrier.clone());
        let mut call = interface_call(&bindings, vec![owned_argument(0, original_carrier)]);
        let transfer = call.take_owned_argument(0).expect("move argument detaches");
        let value = transfer.take_value().expect("native-owned source is guarded");
        let attempt = call.consume_owned_argument(transfer, value, |guard, _commit| {
            assert_eq!(
                guard.with_value(|value| value.clone()),
                Ok(original_value.clone())
            );
            Err(NativeInterfaceError::Handler("leaf rejected".to_string()))
        });
        assert!(matches!(attempt, Err(NativeInterfaceError::Handler(_))));

        assert_eq!(
            call.finish(Ok(MirRuntimeValue::Unit)),
            Err(NativeInterfaceError::UnresolvedTransfers(vec![0]))
        );
        assert_eq!(call.arguments()[0].value, original_value);
        assert!(call.transfers().is_empty());
    }

    #[test]
    fn premature_cross_thread_extraction_aborts_before_consumption_commit() {
        use std::thread;

        let bindings = NativeInterfaceBindings::new();
        let original_carrier = MirNativeOwned::new(String::from("source"));
        let original_value = MirRuntimeValue::NativeOwned(original_carrier.clone());
        let mut call = interface_call(&bindings, vec![owned_argument(0, original_carrier)]);
        let transfer = call.take_owned_argument(0).expect("move argument detaches");
        let value = transfer.take_value().expect("native-owned source is guarded");
        let attempt = call.consume_owned_argument(transfer, value, move |guard, commit| {
            let extracted = thread::spawn(move || guard.into_committed_value().map_err(|_| ()))
                .join()
                .expect("consumer thread completes");
            assert!(extracted.is_err());
            commit(MirRuntimeValue::Int(7))
        });

        assert!(matches!(attempt, Err(NativeInterfaceError::InvalidCall(_))));
        assert_eq!(
            call.finish(Ok(MirRuntimeValue::Unit)),
            Err(NativeInterfaceError::UnresolvedTransfers(vec![0]))
        );
        assert_eq!(call.arguments()[0].value, original_value);
        assert_eq!(call.pending_transfers().collect::<Vec<_>>(), vec![0]);
        assert!(call.transfers().is_empty());
        assert_eq!(
            call.rejected_transfers()[0].with_value(|value| value.clone()),
            Some(MirRuntimeValue::Int(7))
        );
    }

    #[test]
    fn handler_panic_before_commit_restores_native_owned_move_and_preserves_writeback() {
        let bindings = Arc::new(NativeInterfaceBindings::new());
        let object = bindings.create_object(String::from("service")).expect("object identity");
        let (identity, signature) = callable_with_parameter_types(
            "sink",
            &[int(), shared()],
            &[MirAccess::Write, MirAccess::Move],
        );
        let original_carrier = MirNativeOwned::new(String::from("source"));
        let original_value = MirRuntimeValue::NativeOwned(original_carrier.clone());
        bindings
            .bind_callable(
                object.clone(),
                identity.clone(),
                signature.clone(),
                callable_handler(|call| {
                    call.writeback_argument(0, MirRuntimeValue::Int(41))
                        .map_err(|error| error.to_string())?;
                    let transfer = call
                        .take_owned_argument(1)
                        .map_err(|error| error.to_string())?;
                    let value = transfer
                        .take_value()
                        .map_err(|error| error.to_string())?;
                    call.consume_owned_argument(transfer, value, |guard, _commit| {
                        assert!(guard.with_value(|value| {
                            matches!(value, MirRuntimeValue::NativeOwned(_))
                        })?);
                        panic!("leaf failed before acceptance");
                    })
                    .map_err(|error| error.to_string())?;
                    Ok(MirRuntimeValue::Int(0))
                }),
            )
            .expect("callable binds");
        let _scope = push_scope(bindings.clone(), execution(), ARTIFACT);

        let completion = dispatch_active_callable(
            identity,
            signature,
            carrier_of(&object),
            vec![
                argument(0, MirAccess::Write, MirRuntimeValue::Int(1)),
                owned_argument(1, original_carrier),
            ],
            Span::new(0, 0),
        );
        let (outcome, call) = completion.into_parts();
        assert_eq!(
            outcome,
            Err(NativeInterfaceError::UnresolvedTransfers(vec![1]))
        );
        assert_eq!(call.argument(0).expect("argument").value, MirRuntimeValue::Int(41));
        assert_eq!(call.argument(1).expect("argument").value, original_value);
        assert!(call.transfers().is_empty());
        assert_eq!(call.pending_transfers().collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn committed_native_owned_receipt_and_writeback_survive_later_panic() {
        let bindings = Arc::new(NativeInterfaceBindings::new());
        let object = bindings.create_object(String::from("service")).expect("object identity");
        let (identity, signature) = callable_with_parameter_types(
            "sink",
            &[int(), shared()],
            &[MirAccess::Write, MirAccess::Move],
        );
        let original_carrier = MirNativeOwned::new(String::from("source"));
        let original_value = MirRuntimeValue::NativeOwned(original_carrier.clone());
        let expected_for_handler = original_value.clone();
        bindings
            .bind_callable(
                object.clone(),
                identity.clone(),
                signature.clone(),
                callable_handler(move |call| {
                    call.writeback_argument(0, MirRuntimeValue::Int(41))
                        .map_err(|error| error.to_string())?;
                    let transfer = call
                        .take_owned_argument(1)
                        .map_err(|error| error.to_string())?;
                    let value = transfer
                        .take_value()
                        .map_err(|error| error.to_string())?;
                    call.consume_owned_argument(transfer, value, |guard, commit| {
                        let (value, receipt) = guard
                            .commit_and_extract(MirRuntimeValue::Int(7), commit)?;
                        assert_eq!(value, expected_for_handler);
                        assert_eq!(receipt, MirRuntimeValue::Int(7));
                        Ok(())
                    })
                    .map_err(|error| error.to_string())?;
                    panic!("handler failed after acceptance");
                }),
            )
            .expect("callable binds");
        let _scope = push_scope(bindings.clone(), execution(), ARTIFACT);

        let completion = dispatch_active_callable(
            identity,
            signature,
            carrier_of(&object),
            vec![
                argument(0, MirAccess::Write, MirRuntimeValue::Int(1)),
                owned_argument(1, original_carrier),
            ],
            Span::new(0, 0),
        );
        let (outcome, call) = completion.into_parts();
        assert!(matches!(
            outcome,
            Err(NativeInterfaceError::HandlerPanicked(_))
        ));
        assert_eq!(call.argument(0).expect("argument").value, MirRuntimeValue::Int(41));
        assert_eq!(call.argument(1).expect("argument").value, MirRuntimeValue::Moved);
        assert_eq!(
            call.transfers(),
            &[NativeInterfaceTransferReceipt {
                argument: 1,
                disposition: NativeInterfaceTransferDisposition::Consumed(
                    MirRuntimeValue::Int(7)
                ),
            }]
        );
        assert!(call.pending_transfers().next().is_none());
    }


    #[test]
    fn fixed_list_literal_length_is_part_of_the_checked_type() {
        let fixed = MirType::from_kind(MirTypeKind::FixedList {
            elem: Box::new(int()),
            len: MirMeasure::Literal {
                kind: "len".to_string(),
                value: 3,
            },
        });
        let ints = |count: i64| MirRuntimeValue::List((0..count).map(MirRuntimeValue::Int).collect());
        assert!(runtime_value_matches_type(&ints(3), &fixed));
        assert!(!runtime_value_matches_type(&ints(0), &fixed));
        assert!(!runtime_value_matches_type(&ints(4), &fixed));
        assert!(!runtime_value_matches_type(
            &MirRuntimeValue::List(vec![MirRuntimeValue::Int(1), MirRuntimeValue::Int(2), MirRuntimeValue::Bool(true)]),
            &fixed
        ));

        let symbolic = MirType::from_kind(MirTypeKind::FixedList {
            elem: Box::new(int()),
            len: MirMeasure::Symbol {
                kind: "len".to_string(),
                name: "N".to_string(),
            },
        });
        assert!(runtime_value_matches_type(&ints(0), &symbolic));
        assert!(runtime_value_matches_type(&ints(5), &symbolic));
    }
    #[test]
    fn fixed_width_integer_bounds_cover_int_and_bigint_carriers() {
        let signed = MirType::from_kind(MirTypeKind::IntN {
            signed: true,
            bits: 8,
        });
        for value in [
            MirRuntimeValue::Int(-128),
            MirRuntimeValue::Int(127),
            MirRuntimeValue::BigInt("-128".to_string()),
            MirRuntimeValue::BigInt("127".to_string()),
        ] {
            assert!(runtime_value_matches_type(&value, &signed));
        }
        for value in [
            MirRuntimeValue::Int(-129),
            MirRuntimeValue::Int(128),
            MirRuntimeValue::BigInt("-129".to_string()),
            MirRuntimeValue::BigInt("128".to_string()),
            MirRuntimeValue::BigInt("not-an-integer".to_string()),
        ] {
            assert!(!runtime_value_matches_type(&value, &signed));
        }

        let unsigned = MirType::from_kind(MirTypeKind::IntN {
            signed: false,
            bits: 64,
        });
        assert!(runtime_value_matches_type(
            &MirRuntimeValue::Int(0),
            &unsigned
        ));
        assert!(runtime_value_matches_type(
            &MirRuntimeValue::BigInt("18446744073709551615".to_string()),
            &unsigned
        ));
        assert!(!runtime_value_matches_type(
            &MirRuntimeValue::Int(-1),
            &unsigned
        ));
        assert!(!runtime_value_matches_type(
            &MirRuntimeValue::BigInt("18446744073709551616".to_string()),
            &unsigned
        ));

        for bits in [0, 65] {
            let invalid_width =
                MirType::from_kind(MirTypeKind::IntN { signed: true, bits });
            assert!(!runtime_value_matches_type(
                &MirRuntimeValue::Int(0),
                &invalid_width
            ));
        }
    }

    fn physical_borrow_root(
        type_identity: u64,
        owner_identity: usize,
    ) -> crate::SourceSharedInterop::SourceSharedInterop {
        crate::SourceSharedInterop::SourceSharedInterop::from_value(
            type_identity,
            MirRuntimeValue::Int(11),
        )
        .with_owner_identity(owner_identity)
        .with_protocol_order_key(owner_identity)
    }

    fn physical_borrow_entry(
        capability: &crate::SourceResources::SourceResourceHandle,
        selector: i64,
        view: crate::SourceSharedInterop::SourceSharedInteropGuardState,
    ) -> NativeInterfacePhysicalBorrowEntry {
        let metadata = NativeInterfacePhysicalBorrowMetadata {
            selector,
            owner_row: MirPreludeCallId(selector as u64),
            capability: capability.clone(),
            physical_identity: view.physical_identity(),
            editable: view.editable(),
            path: view
                .path()
                .iter()
                .map(|field| MirFieldId(*field as u64))
                .collect(),
        };
        NativeInterfacePhysicalBorrowEntry { metadata, view }
    }

    #[test]
    fn physical_borrow_context_restores_nested_scopes_and_separates_selectors() {
        let bindings = Arc::new(NativeInterfaceBindings::new());
        let _base = push_scope(bindings, execution(), ARTIFACT);
        let arena = crate::SourceResources::SourceResourceArena::new();
        let root = physical_borrow_root(91, 0x9101);
        let capability = arena
            .register_shared_interop_root(91, &root)
            .expect("physical root capability");
        let first_view = root
            .acquire_guard_state(true)
            .expect("physical guard view");
        let second_view = first_view
            .clone_guard(false)
            .expect("read-only argument view shares the acquired guard");
        let mut entries = vec![
            physical_borrow_entry(&capability, 17, first_view),
            physical_borrow_entry(&capability, 18, second_view),
        ];
        let first_context = NativeInterfacePhysicalBorrowContext::new(&mut entries)
            .expect("one context owns both argument guard views");
        assert!(entries.is_empty());
        assert_eq!(
            first_context
                .entries
                .iter()
                .map(|entry| entry.metadata.selector)
                .collect::<Vec<_>>(),
            vec![17, 18]
        );
        let checked_scope = NativeInterfaceScope::current_checked_scope()
            .expect("base scope supplies checked C activation metadata");
        let first_activation = NativeInterfaceScope::activate_checked_scope(
            &checked_scope,
            Some(first_context.clone()),
        );
        assert!(NativeInterfaceScope::current_physical_borrow_context()
            .is_some_and(|active| Rc::ptr_eq(&active, &first_context)));
        assert_eq!(
            NativeInterfaceScope::with_active_physical_borrow(
                &first_context.entries[1].metadata,
                |view| view.physical_identity(),
            )
            .expect("active C scope reborrows the selected B view"),
            first_context.entries[1].metadata.physical_identity
        );
        let (_, call_signature) = callable("physical-borrow-arguments", &[]);
        let call = NativeCallableCall::new(
            None,
            call_signature,
            None,
            None,
            Vec::new(),
            Span::new(0, 0),
        );

        let nested_view = first_context.entries[0]
            .view
            .entry_view()
            .expect("nested context shares the existing physical guard");
        let mut nested_entries = vec![physical_borrow_entry(
            &capability,
            29,
            nested_view,
        )];
        let nested_context = NativeInterfacePhysicalBorrowContext::new(&mut nested_entries)
            .expect("nested context owns a physical view");
        let nested_activation = NativeInterfaceScope::activate_checked_scope(
            &checked_scope,
            Some(nested_context.clone()),
        );
        assert!(NativeInterfaceScope::current_physical_borrow_context()
            .is_some_and(|active| Rc::ptr_eq(&active, &nested_context)));

        drop(nested_activation);
        assert!(NativeInterfaceScope::current_physical_borrow_context()
            .is_some_and(|active| Rc::ptr_eq(&active, &first_context)));
        drop(first_activation);
        assert!(NativeInterfaceScope::current_physical_borrow_context().is_none());

        assert!(call
            .with_physical_borrow(&first_context.entries[0].metadata, |view| {
                assert!(view.held());
                view.editable()
            })
            .expect("first call selector resolves its writable guard view"));
        assert!(!call
            .with_physical_borrow(&first_context.entries[1].metadata, |view| {
                assert!(view.held());
                view.editable()
            })
            .expect("second call selector resolves its read-only guard view"));
    }


    #[test]
    fn physical_borrow_context_is_independent_of_checked_scope() {
        let arena = crate::SourceResources::SourceResourceArena::new();
        let root = physical_borrow_root(90, 0x9001);
        let capability = arena
            .register_shared_interop_root(90, &root)
            .expect("physical root capability");
        let view = root
            .acquire_guard_state(true)
            .expect("physical guard view");
        let mut entries = vec![physical_borrow_entry(&capability, 9, view)];

        let context = NativeInterfacePhysicalBorrowContext::new(&mut entries)
            .expect("guest views are independent of checked C metadata");
        assert!(entries.is_empty());
        assert!(NativeInterfaceScope::current_checked_scope().is_none());
        assert!(NativeInterfaceScope::current_physical_borrow_context().is_none());
        assert!(context
            .with_physical_borrow(&context.entries[0].metadata, |view| view.held())
            .expect("same-thread context remains directly owned"));
    }
    #[test]
    fn physical_borrow_rejects_metadata_mismatches_and_preserves_invalid_entries() {
        let bindings = Arc::new(NativeInterfaceBindings::new());
        let _base = push_scope(bindings, execution(), ARTIFACT);
        let arena = crate::SourceResources::SourceResourceArena::new();
        let root = physical_borrow_root(92, 0x9201);
        let capability = arena
            .register_shared_interop_root(92, &root)
            .expect("physical root capability");
        let view = root
            .acquire_guard_state(true)
            .expect("physical guard view");
        let entry = physical_borrow_entry(&capability, 41, view);
        let expected = entry.metadata.clone();
        let mut entries = vec![entry];
        let context = NativeInterfacePhysicalBorrowContext::new(&mut entries)
            .expect("validated view and capability");

        let mut wrong_owner = expected.clone();
        wrong_owner.owner_row = MirPreludeCallId(999);
        let mut wrong_generation = expected.clone();
        wrong_generation.capability.generation =
            wrong_generation.capability.generation.wrapping_add(1);
        let mut wrong_raw_capability = expected.clone();
        wrong_raw_capability.capability.raw =
            wrong_raw_capability.capability.raw.wrapping_add(1);
        let mut wrong_type = expected.clone();
        wrong_type.capability.kind =
            crate::SourceResources::SourceResourceKind::SharedInteropRoot {
                type_identity: 999,
            };
        let mut wrong_path = expected.clone();
        wrong_path.path.push(MirFieldId(3));
        let mut wrong_editability = expected.clone();
        wrong_editability.editable = !wrong_editability.editable;
        let mut wrong_physical_owner = expected.clone();
        wrong_physical_owner.physical_identity =
            wrong_physical_owner.physical_identity.wrapping_add(1);
        for mismatch in [
            wrong_owner,
            wrong_generation,
            wrong_raw_capability,
            wrong_type,
            wrong_path,
            wrong_editability,
            wrong_physical_owner,
        ] {
            assert!(context
                .with_physical_borrow(&mismatch, |_| ())
                .is_err());
        }
        let mut wrong_selector = expected.clone();
        wrong_selector.selector += 1;
        assert_eq!(
            context.with_physical_borrow(&wrong_selector, |_| ()),
            Err(NativeInterfaceError::MissingBinding)
        );

        let invalid_path_view = context.entries[0]
            .view
            .entry_view()
            .expect("retain view for rejected construction");
        let mut invalid_path_entries = vec![physical_borrow_entry(
            &capability,
            42,
            invalid_path_view,
        )];
        invalid_path_entries[0].metadata.path.push(MirFieldId(8));
        assert!(NativeInterfacePhysicalBorrowContext::new(&mut invalid_path_entries).is_err());
        assert_eq!(invalid_path_entries.len(), 1);
        assert!(invalid_path_entries[0].view.held());

        let invalid_edit_view = context.entries[0]
            .view
            .entry_view()
            .expect("retain view for rejected edit capability");
        let mut invalid_edit_entries = vec![physical_borrow_entry(
            &capability,
            43,
            invalid_edit_view,
        )];
        invalid_edit_entries[0].metadata.editable = false;
        assert!(NativeInterfacePhysicalBorrowContext::new(&mut invalid_edit_entries).is_err());
        assert_eq!(invalid_edit_entries.len(), 1);
        assert!(invalid_edit_entries[0].view.held());

        let mut wrong_thread_context =
            NativeInterfacePhysicalBorrowContext::new(&mut vec![physical_borrow_entry(
                &capability,
                44,
                context.entries[0]
                    .view
                    .entry_view()
                    .expect("thread check retains a typed view"),
            )])
            .expect("construct same-thread context");
        let other_thread = std::thread::spawn(|| std::thread::current().id())
            .join()
            .expect("worker thread id");
        Rc::get_mut(&mut wrong_thread_context)
            .expect("context has one strong owner")
            .thread = other_thread;
        assert_eq!(
            wrong_thread_context.with_physical_borrow(&expected, |_| ()),
            Err(NativeInterfaceError::ExecutionMismatch)
        );
    }

    type TestPhysicalOutcome<T> =
        crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<T>;

    struct ScopeObservingGuard {
        observed_restored_scope: Arc<AtomicBool>,
    }

    impl crate::SourceSharedInterop::SourceSharedInteropGuard for ScopeObservingGuard {
        fn read_value(&mut self) -> Result<MirRuntimeValue, String> {
            Ok(MirRuntimeValue::Int(11))
        }

        fn stage_value(&mut self, _value: MirRuntimeValue) -> Result<(), String> {
            Ok(())
        }

        fn revision(&self) -> Result<u64, String> {
            Ok(0)
        }

        fn release(&mut self) -> TestPhysicalOutcome<()> {
            TestPhysicalOutcome::new(Ok(()), None)
        }

        fn release_during_drop(&mut self) {
            let restored = NativeInterfaceScope::current_physical_borrow_context().is_none()
                && active_callable_descriptors()
                    .is_ok_and(|descriptors| descriptors.len() == 1);
            self.observed_restored_scope
                .store(restored, Ordering::SeqCst);
        }
    }

    fn observing_physical_borrow_root(
        type_identity: u64,
        owner_identity: usize,
        observed_restored_scope: Arc<AtomicBool>,
    ) -> crate::SourceSharedInterop::SourceSharedInterop {
        crate::SourceSharedInterop::SourceSharedInterop::from_callbacks_with_guard(
            type_identity,
            |callback| {
                let value = MirRuntimeValue::Int(11);
                TestPhysicalOutcome::new(callback(&value), None)
            },
            |callback| {
                let mut value = MirRuntimeValue::Int(11);
                TestPhysicalOutcome::new(callback(&mut value), None)
            },
            {
                let observed_restored_scope = observed_restored_scope.clone();
                move |_editable| {
                    let guard: Box<dyn crate::SourceSharedInterop::SourceSharedInteropGuard> =
                        Box::new(ScopeObservingGuard {
                            observed_restored_scope: observed_restored_scope.clone(),
                        });
                    TestPhysicalOutcome::new(Ok(guard), None)
                }
            },
            |callback| {
                let value = MirRuntimeValue::Int(11);
                TestPhysicalOutcome::new(callback(&value, 0), None)
            },
            |_revision, _value| TestPhysicalOutcome::new(Ok((true, 1)), None),
        )
        .with_owner_identity(owner_identity)
        .with_protocol_order_key(owner_identity)
    }

    #[test]
    fn physical_borrow_completion_retains_views_and_last_drop_sees_restored_scope() {
        let bindings = Arc::new(NativeInterfaceBindings::new());
        let object = bindings
            .create_object(String::from("physical-borrow-scope"))
            .expect("callable object");
        let (identity, signature) = callable("physical-borrow-scope", &[]);
        bindings
            .bind_callable(
                object.clone(),
                identity.clone(),
                signature.clone(),
                callable_handler(|_call| Ok(MirRuntimeValue::Int(1))),
            )
            .expect("checked scope marker binding");
        let carrier = object.clone_root();
        let _base = push_scope(bindings, execution(), ARTIFACT);

        let observed = Arc::new(AtomicBool::new(false));
        let root = observing_physical_borrow_root(93, 0x9301, observed.clone());
        let arena = crate::SourceResources::SourceResourceArena::new();
        let capability = arena
            .register_shared_interop_root(93, &root)
            .expect("physical root capability");
        let view = root
            .acquire_guard_state(true)
            .expect("physical guard view");
        let mut entries = vec![physical_borrow_entry(&capability, 51, view)];
        let context = NativeInterfacePhysicalBorrowContext::new(&mut entries)
            .expect("context retains guest physical view");
        let metadata = context.entries[0].metadata.clone();
        let checked_scope = NativeInterfaceScope::current_checked_scope()
            .expect("base scope supplies checked C activation metadata");
        let activation = NativeInterfaceScope::activate_checked_scope(
            &checked_scope,
            Some(context.clone()),
        );
        let call = NativeCallableCall::new(
            Some(identity),
            signature,
            Some(object),
            Some(carrier),
            Vec::new(),
            Span::new(0, 0),
        );
        let completion = NativeCallableCompletion {
            outcome: Ok(MirRuntimeValue::Int(1)),
            call,
        };
        let (trait_ref, receiver_type) = interface_type();
        let interface_call = NativeInterfaceCall::new(
            NativeInterfaceIdentity {
                execution: execution(),
                artifact: ARTIFACT,
                trait_ref,
                method_id: MirTraitMethodId(1),
                method_name: "physical_borrow".to_string(),
                receiver_type,
            },
            NativeInterfaceSignature {
                receiver_access: MirAccess::Read,
                parameters: Vec::new(),
                return_type: unit(),
                failure: MirFailureCarrier::Infallible,
            },
            None,
            MirAccess::Read,
            MirRuntimeValue::Unit,
            Vec::new(),
            Span::new(0, 0),
        );
        let interface_completion = NativeInterfaceCompletion {
            outcome: Ok(MirRuntimeValue::Unit),
            call: interface_call,
        };
        drop(activation);
        let alternate_view = context.entries[0]
            .view
            .entry_view()
            .expect("alternate C scope reuses the same physical permit view");
        let mut alternate_entries = vec![physical_borrow_entry(
            &capability,
            52,
            alternate_view,
        )];
        let alternate_context =
            NativeInterfacePhysicalBorrowContext::new(&mut alternate_entries)
                .expect("alternate context retains its own selector");
        let alternate_activation = NativeInterfaceScope::activate_checked_scope(
            &checked_scope,
            Some(alternate_context.clone()),
        );
        drop(context);
        assert!(NativeInterfaceScope::current_physical_borrow_context()
            .is_some_and(|active| Rc::ptr_eq(&active, &alternate_context)));
        assert!(completion
            .call()
            .with_physical_borrow(&metadata, |view| view.held())
            .expect("completion uses its own view under another active context"));
        assert!(interface_completion
            .call()
            .with_physical_borrow(&metadata, |view| view.held())
            .expect("interface completion uses its own view under another active context"));
        drop(alternate_activation);
        drop(alternate_context);
        assert!(NativeInterfaceScope::current_physical_borrow_context().is_none());

        drop(completion);
        drop(interface_completion);
        assert!(observed.load(Ordering::SeqCst));
        drop(_base);
    }
}
