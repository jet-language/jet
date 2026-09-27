//! Checked, invocation-scoped dispatch for native implementations of private MIR traits.
//!
//! A binding is a native service root plus one checked trait-method implementation.  The
//! binding set is activated around a specific artifact execution; nested calls restore the
//! previous activation, and task workers must explicitly activate their own set.  Calls own
//! every argument until a completion (including failures) returns the exact argument rows and
//! transfer receipts to the caller.

use jet_foundation::Diagnostics::Span;
use jet_foundation::MIR::{
    MirAccess, MirArtifactId, MirExecutionIdentity, MirFailureCarrier, MirNativeOwned, MirParam,
    MirProgram, MirRuntimeValue, MirTraitId, MirTraitMethodId, MirTraitRef, MirType, MirTypeId,
    MirTypeKind,
};
use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

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
        let key = key.into();
        if key.is_empty() || key.trim() != key {
            return Err(NativeInterfaceError::InvalidMetadata(
                "native callable key must be a non-empty canonical string".to_string(),
            ));
        }
        NativeCallableSignature::checked(&callable_type)?;
        let execution = program
            .execution_identity(Some(artifact))
            .map_err(|error| NativeInterfaceError::InvalidMetadata(error.to_string()))?;
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
/// set; the Arc-owned service root is retained alongside it for the whole binding
/// lifetime. It is never interpreted as an address or a language value.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NativeInterfaceObjectId(u64);

/// One native interface object and its retained physical service root.
#[derive(Clone)]
pub struct NativeInterfaceObject {
    id: NativeInterfaceObjectId,
    set_identity: Arc<()>,
    root: MirNativeOwned,
}

impl NativeInterfaceObject {
    /// Session-local object identity used by checked dispatch.
    pub fn id(&self) -> NativeInterfaceObjectId {
        self.id
    }

    /// Borrow the canonical native service root without converting it to a raw handle.
    pub fn root<T: Any>(&self) -> Option<&T> {
        self.root.downcast_ref::<T>()
    }

    /// A runtime trait-object carrier aliases this exact physical root. The JIT
    /// heap codec must preserve the `MirNativeOwned` Arc identity, never a raw
    /// pointer or a caller-chosen object number.
    pub fn as_runtime_value(&self) -> MirRuntimeValue {
        MirRuntimeValue::NativeOwned(self.root.clone())
    }

    pub fn matches_root(&self, carrier: &MirNativeOwned) -> bool {
        self.root == *carrier
    }

    /// Retain the same physical root for an explicit child-task binding.
    pub fn clone_root(&self) -> MirNativeOwned {
        self.root.clone()
    }
}

impl NativeInterfaceObject {
    fn same_identity(&self, other: &Self) -> bool {
        self.id == other.id
            && Arc::ptr_eq(&self.set_identity, &other.set_identity)
            && self.root == other.root
    }
}

impl fmt::Debug for NativeInterfaceObject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeInterfaceObject")
            .field("id", &self.id)
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
/// `NativeInterfaceCall::return_owned_argument` or
/// `NativeInterfaceCall::consume_owned_argument`.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeInterfaceTransferReceipt {
    pub argument: usize,
    pub disposition: NativeInterfaceTransferDisposition,
}

/// An owned Move argument temporarily detached from its call row.
pub struct NativeInterfaceOwnedArgument {
    argument: usize,
    call_identity: Arc<()>,
    value: Option<MirRuntimeValue>,
}

impl NativeInterfaceOwnedArgument {
    pub fn argument(&self) -> usize {
        self.argument
    }

    /// Take the payload for the native leaf. The matching transfer token must
    /// later be resolved through the call, on success or on failure.
    pub fn take_value(&mut self) -> MirRuntimeValue {
        self.value
            .take()
            .expect("native interface transfer payload was taken once")
    }
}

/// Owned, checked call to one bound native trait method. The dispatcher keeps
/// this value alive across the handler and returns it even if the handler
/// reports failure, preserving every argument and writeback slot.
pub struct NativeInterfaceCall {
    identity: NativeInterfaceIdentity,
    signature: NativeInterfaceSignature,
    object: Option<NativeInterfaceObject>,
    receiver_access: MirAccess,
    receiver: MirRuntimeValue,
    arguments: Vec<NativeInterfaceArgument>,
    span: Span,
    transfers: Vec<NativeInterfaceTransferReceipt>,
    pending_transfers: Vec<usize>,
    transfer_identity: Arc<()>,
    preflight_error: Option<NativeInterfaceError>,
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
            transfers: Vec::new(),
            pending_transfers: Vec::new(),
            transfer_identity: Arc::new(()),
            preflight_error: None,
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

    pub fn transfers(&self) -> &[NativeInterfaceTransferReceipt] {
        &self.transfers
    }

    pub fn pending_transfers(&self) -> &[usize] {
        &self.pending_transfers
    }

    pub fn span(&self) -> Span {
        self.span
    }

    /// Read a checked argument without relinquishing its ownership.
    pub fn argument(&self, index: usize) -> Option<&NativeInterfaceArgument> {
        self.arguments.get(index)
    }

    /// Replace one explicitly writable argument value. This remains available
    /// on the error rail so callers can write back partially completed Source
    /// state before propagating the typed failure.
    pub fn writeback_argument(
        &mut self,
        index: usize,
        value: MirRuntimeValue,
    ) -> Result<(), NativeInterfaceError> {
        let argument = self
            .arguments
            .get_mut(index)
            .ok_or_else(|| NativeInterfaceError::InvalidCall(format!("writeback argument {index} is outside the checked signature")))?;
        if !argument.writeback {
            return Err(NativeInterfaceError::InvalidCall(format!(
                "argument {index} is not declared writable by this native interface call"
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

    /// Take an owned Move argument while retaining an invocation-local receipt
    /// token. A handler must resolve every taken value using one of the methods
    /// below, including when its native leaf returns an error.
    pub fn take_owned_argument(
        &mut self,
        index: usize,
    ) -> Result<NativeInterfaceOwnedArgument, NativeInterfaceError> {
        let argument = self
            .arguments
            .get_mut(index)
            .ok_or_else(|| NativeInterfaceError::InvalidCall(format!("move argument {index} is outside the checked signature")))?;
        if argument.access != MirAccess::Move {
            return Err(NativeInterfaceError::InvalidCall(format!(
                "argument {index} is not a checked Move parameter"
            )));
        }
        if self.pending_transfers.contains(&index)
            || self.transfers.iter().any(|receipt| receipt.argument == index)
        {
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
        self.pending_transfers.push(index);
        Ok(NativeInterfaceOwnedArgument {
            argument: index,
            call_identity: self.transfer_identity.clone(),
            value: Some(value),
        })
    }

    /// Return a moved argument to its original slot after a failed or successful
    /// native attempt. The exact returned value remains in the completion.
    pub fn return_owned_argument(
        &mut self,
        transfer: NativeInterfaceOwnedArgument,
        value: MirRuntimeValue,
    ) -> Result<(), NativeInterfaceError> {
        self.resolve_transfer(transfer, NativeInterfaceTransferDisposition::Returned, Some(value))
    }

    /// Record an explicit physical-consumption receipt after a native leaf has
    /// accepted ownership. `receipt` is retained by the completion on both the
    /// success and error rails.
    pub fn consume_owned_argument(
        &mut self,
        transfer: NativeInterfaceOwnedArgument,
        receipt: MirRuntimeValue,
    ) -> Result<(), NativeInterfaceError> {
        self.resolve_transfer(
            transfer,
            NativeInterfaceTransferDisposition::Consumed(receipt),
            None,
        )
    }

    fn resolve_transfer(
        &mut self,
        mut transfer: NativeInterfaceOwnedArgument,
        disposition: NativeInterfaceTransferDisposition,
        returned: Option<MirRuntimeValue>,
    ) -> Result<(), NativeInterfaceError> {
        if !Arc::ptr_eq(&transfer.call_identity, &self.transfer_identity) {
            return Err(NativeInterfaceError::InvalidCall(
                "owned argument transfer belongs to another interface call".to_string(),
            ));
        }
        let position = self
            .pending_transfers
            .iter()
            .position(|index| *index == transfer.argument)
            .ok_or_else(|| NativeInterfaceError::InvalidCall("owned argument transfer is not pending".to_string()))?;
        if transfer.value.is_some() {
            return Err(NativeInterfaceError::InvalidCall(
                "owned argument transfer must be taken before resolution".to_string(),
            ));
        }
        if let Some(value) = returned.as_ref() {
            if value == &MirRuntimeValue::Moved {
                return Err(NativeInterfaceError::InvalidCall(
                    "a moved argument cannot be returned in moved state".to_string(),
                ));
            }
            let argument = self.arguments.get(transfer.argument).ok_or_else(|| {
                NativeInterfaceError::InvalidCall("owned argument transfer index disappeared".to_string())
            })?;
            if !runtime_value_matches_type(value, &argument.ty) {
                return Err(NativeInterfaceError::InvalidCall(format!(
                    "returned move argument {} does not match its checked MIR type",
                    transfer.argument
                )));
            }
        }
        self.pending_transfers.remove(position);
        if let Some(value) = returned {
            let argument = self.arguments.get_mut(transfer.argument).ok_or_else(|| {
                NativeInterfaceError::InvalidCall("owned argument transfer index disappeared".to_string())
            })?;
            argument.value = value;
        }
        self.transfers.push(NativeInterfaceTransferReceipt {
            argument: transfer.argument,
            disposition,
        });
        Ok(())
    }

    fn validate_completed(&self, outcome: &Result<MirRuntimeValue, NativeInterfaceError>) -> Result<(), NativeInterfaceError> {
        if !self.pending_transfers.is_empty() {
            return Err(NativeInterfaceError::UnresolvedTransfers(
                self.pending_transfers.clone(),
            ));
        }
        for argument in &self.arguments {
            if argument.access == MirAccess::Move
                && !self.transfers.iter().any(|receipt| receipt.argument == argument.index)
            {
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
        if let Ok(value) = outcome {
            if !runtime_value_matches_type(value, &self.signature.return_type) {
                return Err(NativeInterfaceError::InvalidCall(
                    "native result does not match its checked MIR return type".to_string(),
                ));
            }
        }
        Ok(())
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
    transfers: Vec<NativeInterfaceTransferReceipt>,
    pending_transfers: Vec<usize>,
    transfer_identity: Arc<()>,
    preflight_error: Option<NativeInterfaceError>,
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
            transfers: Vec::new(),
            pending_transfers: Vec::new(),
            transfer_identity: Arc::new(()),
            preflight_error: None,
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

    pub fn arguments(&self) -> &[NativeInterfaceArgument] {
        &self.arguments
    }

    pub fn transfers(&self) -> &[NativeInterfaceTransferReceipt] {
        &self.transfers
    }

    pub fn span(&self) -> Span {
        self.span
    }

    pub fn argument(&self, index: usize) -> Option<&NativeInterfaceArgument> {
        self.arguments.get(index)
    }

    pub fn writeback_argument(
        &mut self,
        index: usize,
        value: MirRuntimeValue,
    ) -> Result<(), NativeInterfaceError> {
        let argument = self
            .arguments
            .get_mut(index)
            .ok_or_else(|| NativeInterfaceError::InvalidCall(format!("callable writeback argument {index} is out of range")))?;
        if !argument.writeback || value == MirRuntimeValue::Moved {
            return Err(NativeInterfaceError::InvalidCall(format!(
                "callable argument {index} has no valid checked writeback channel"
            )));
        }
        if !runtime_value_matches_type(&value, &argument.ty) {
            return Err(NativeInterfaceError::InvalidCall(format!(
                "callable writeback argument {index} does not match its checked MIR type"
            )));
        }
        argument.value = value;
        Ok(())
    }

    pub fn take_owned_argument(
        &mut self,
        index: usize,
    ) -> Result<NativeInterfaceOwnedArgument, NativeInterfaceError> {
        let argument = self
            .arguments
            .get_mut(index)
            .ok_or_else(|| NativeInterfaceError::InvalidCall(format!("callable Move argument {index} is out of range")))?;
        if argument.access != MirAccess::Move {
            return Err(NativeInterfaceError::InvalidCall(format!(
                "callable argument {index} is not a checked Move parameter"
            )));
        }
        if self.pending_transfers.contains(&index)
            || self.transfers.iter().any(|receipt| receipt.argument == index)
        {
            return Err(NativeInterfaceError::InvalidCall(format!(
                "callable Move argument {index} was transferred more than once"
            )));
        }
        if argument.value == MirRuntimeValue::Moved {
            return Err(NativeInterfaceError::InvalidCall(format!(
                "callable Move argument {index} was already moved"
            )));
        }
        let value = std::mem::replace(&mut argument.value, MirRuntimeValue::Moved);
        self.pending_transfers.push(index);
        Ok(NativeInterfaceOwnedArgument {
            argument: index,
            call_identity: self.transfer_identity.clone(),
            value: Some(value),
        })
    }

    pub fn return_owned_argument(
        &mut self,
        transfer: NativeInterfaceOwnedArgument,
        value: MirRuntimeValue,
    ) -> Result<(), NativeInterfaceError> {
        self.resolve_transfer(transfer, NativeInterfaceTransferDisposition::Returned, Some(value))
    }

    pub fn consume_owned_argument(
        &mut self,
        transfer: NativeInterfaceOwnedArgument,
        receipt: MirRuntimeValue,
    ) -> Result<(), NativeInterfaceError> {
        self.resolve_transfer(
            transfer,
            NativeInterfaceTransferDisposition::Consumed(receipt),
            None,
        )
    }

    fn resolve_transfer(
        &mut self,
        mut transfer: NativeInterfaceOwnedArgument,
        disposition: NativeInterfaceTransferDisposition,
        returned: Option<MirRuntimeValue>,
    ) -> Result<(), NativeInterfaceError> {
        if !Arc::ptr_eq(&transfer.call_identity, &self.transfer_identity) {
            return Err(NativeInterfaceError::InvalidCall(
                "callable transfer belongs to a different invocation".to_string(),
            ));
        }
        let position = self
            .pending_transfers
            .iter()
            .position(|index| *index == transfer.argument)
            .ok_or_else(|| NativeInterfaceError::InvalidCall("callable transfer is not pending".to_string()))?;
        if transfer.value.is_some() {
            let value = transfer.value.take().expect("checked callable payload");
            if let Some(argument) = self.arguments.get_mut(transfer.argument) {
                argument.value = value;
            }
            self.pending_transfers.remove(position);
            self.transfers.push(NativeInterfaceTransferReceipt {
                argument: transfer.argument,
                disposition: NativeInterfaceTransferDisposition::Returned,
            });
            return Err(NativeInterfaceError::InvalidCall(
                "callable transfer must be taken before resolution".to_string(),
            ));
        }
        if let Some(value) = returned.as_ref() {
            if value == &MirRuntimeValue::Moved
                || !self
                    .arguments
                    .get(transfer.argument)
                    .is_some_and(|argument| runtime_value_matches_type(value, &argument.ty))
            {
                return Err(NativeInterfaceError::InvalidCall(
                    "returned callable argument does not match its checked MIR type".to_string(),
                ));
            }
        }
        self.pending_transfers.remove(position);
        if let Some(value) = returned {
            self.arguments[transfer.argument].value = value;
        }
        self.transfers.push(NativeInterfaceTransferReceipt {
            argument: transfer.argument,
            disposition,
        });
        Ok(())
    }

    fn validate_completed(
        &self,
        outcome: &Result<MirRuntimeValue, NativeInterfaceError>,
    ) -> Result<(), NativeInterfaceError> {
        if !self.pending_transfers.is_empty() {
            return Err(NativeInterfaceError::UnresolvedTransfers(
                self.pending_transfers.clone(),
            ));
        }
        for argument in &self.arguments {
            if argument.access == MirAccess::Move
                && !self.transfers.iter().any(|receipt| receipt.argument == argument.index)
            {
                return Err(NativeInterfaceError::UnresolvedTransfers(vec![argument.index]));
            }
            if argument.writeback && argument.value == MirRuntimeValue::Moved {
                return Err(NativeInterfaceError::InvalidCall(format!(
                    "callable argument {} has no writeback value",
                    argument.index
                )));
            }
            if argument.writeback && !runtime_value_matches_type(&argument.value, &argument.ty) {
                return Err(NativeInterfaceError::InvalidCall(format!(
                    "callable writeback argument {} has an invalid checked type",
                    argument.index
                )));
            }
        }
        if let Ok(value) = outcome {
            let matches = match &self.signature.return_type {
                Some(ty) => runtime_value_matches_type(value, ty),
                None => matches!(value, MirRuntimeValue::Unit),
            };
            if !matches {
                return Err(NativeInterfaceError::InvalidCall(
                    "native callable result does not match its checked Fn return type".to_string(),
                ));
            }
        }
        Ok(())
    }
}



/// The handler receives the canonical retained object and mutable owned call.

/// It must return a typed result or error; dispatch packages the entire call
/// and all receipts in `NativeInterfaceCompletion` in either case.
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

pub type NativeInterfaceHandler =
    Arc<dyn Fn(&NativeInterfaceObject, &mut NativeInterfaceCall) -> Result<MirRuntimeValue, String> + Send + Sync + 'static>;

pub type NativeCallableHandler =
    Arc<dyn Fn(&NativeInterfaceObject, &mut NativeCallableCall) -> Result<MirRuntimeValue, String> + Send + Sync + 'static>;

/// Per-activation binding set. It owns receiver roots and checked handler
/// schemas; the set is transferable to a task, but each task must activate it
/// locally on its own worker thread.
pub struct NativeInterfaceBindings {
    set_identity: Arc<()>,
    next_object: AtomicU64,
    bindings: std::sync::RwLock<Vec<NativeInterfaceBinding>>,
    callable_bindings: std::sync::RwLock<Vec<NativeCallableBinding>>,
}

struct NativeInterfaceBinding {
    object: NativeInterfaceObject,
    identity: NativeInterfaceIdentity,
    signature: NativeInterfaceSignature,
    handler: NativeInterfaceHandler,
}

struct NativeCallableBinding {
    object: NativeInterfaceObject,
    identity: NativeCallableIdentity,
    signature: NativeCallableSignature,
    handler: NativeCallableHandler,
}

impl NativeInterfaceBindings {
    pub fn new() -> Self {
        Self {
            set_identity: Arc::new(()),
            next_object: AtomicU64::new(1),
            bindings: std::sync::RwLock::new(Vec::new()),
            callable_bindings: std::sync::RwLock::new(Vec::new()),
        }
    }

    /// Create an invocation-local adapter object while retaining the canonical
    /// Send+Sync physical service root for the lifetime of all its bindings.
    pub fn create_object<T: Any + Send + Sync>(
        &self,
        root: T,
    ) -> Result<NativeInterfaceObject, NativeInterfaceError> {
        let id = self
            .next_object
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |next| next.checked_add(1))
            .map_err(|_| NativeInterfaceError::IdentityExhausted)?;
        Ok(NativeInterfaceObject {
            id: NativeInterfaceObjectId(id),
            set_identity: self.set_identity.clone(),
            root: MirNativeOwned::new(root),
        })
    }
    /// Snapshot only checked identity/signature rows for backend installation.
    /// Physical roots and handlers remain owned by this binding set.
    pub fn method_descriptors(&self) -> Vec<NativeInterfaceBindingDescriptor> {
        let bindings = self
            .bindings
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        bindings
            .iter()
            .map(|binding| NativeInterfaceBindingDescriptor {
                identity: binding.identity.clone(),
                signature: binding.signature.clone(),
                object: binding.object.id,
            })
            .collect()
    }
    pub fn callable_descriptors(&self) -> Vec<NativeCallableBindingDescriptor> {
        let bindings = self
            .callable_bindings
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        bindings
            .iter()
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
        let mut bindings = self
            .bindings
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if bindings.iter().any(|binding| {
            binding.object.id == object.id
                && same_method(&binding.identity, &identity)
        }) {
            return Err(NativeInterfaceError::DuplicateBinding);
        }
        bindings.push(NativeInterfaceBinding {
            object,
            identity,
            signature,
            handler,
        });
        Ok(())
    }
    /// Register one explicit native function value under its checked Fn type.
    /// The carrier root, key, and signature must resolve to exactly one target.
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
        if identity.execution.artifact.artifact != identity.artifact
            || NativeCallableSignature::checked(&identity.callable_type)? != signature
        {
            return Err(NativeInterfaceError::SignatureMismatch);
        }
        let mut bindings = self
            .callable_bindings
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if bindings.iter().any(|binding| {
            same_callable(&binding.identity, &identity)
                && binding.signature == signature
        }) {
            return Err(NativeInterfaceError::DuplicateBinding);
        }
        bindings.push(NativeCallableBinding {
            object,
            identity,
            signature,
            handler,
        });
        Ok(())
    }
    /// Invoke by key only through this retained binding set. The adapter must
    /// retain the same `Arc` across Source tasks/transfers; the active scope is
    /// checked by pointer identity so a matching key in another root is never
    /// substituted. The binding owns the physical callable root until completion.
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
        if call.preflight_error.is_none() {
            call.preflight_error = preflight_error;
        }
        self.dispatch_callable(&execution, artifact, call)
    }


    /// Validate all registered identities and signatures against this exact
    /// checked artifact, then push a nested per-thread activation scope.
    pub fn activate(
        self: &Arc<Self>,
        program: &MirProgram,
        artifact: MirArtifactId,
    ) -> Result<NativeInterfaceScope, NativeInterfaceError> {
        let execution = program
            .execution_identity(Some(artifact))
            .map_err(|error| NativeInterfaceError::InvalidMetadata(error.to_string()))?;
        let bindings = self
            .bindings
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for binding in bindings.iter() {
            let checked = NativeInterfaceMethod::checked(
                program,
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
            let checked = NativeCallableIdentity::checked(
                program,
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
        Ok(push_scope(self.clone(), execution, artifact))
    }

    fn object_for_carrier(
        &self,
        identity: &NativeInterfaceIdentity,
        signature: &NativeInterfaceSignature,
        carrier: &MirNativeOwned,
    ) -> Result<NativeInterfaceObject, NativeInterfaceError> {
        let bindings = self
            .bindings
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut objects = bindings.iter().filter(|binding| {
            same_method(&binding.identity, identity)
                && binding.signature == *signature
                && binding.object.matches_root(carrier)
        });
        let binding = objects.next().ok_or(NativeInterfaceError::MissingBinding)?;
        if objects.next().is_some() {
            return Err(NativeInterfaceError::AmbiguousBinding);
        }
        Ok(binding.object.clone())
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
        let binding = {
            let bindings = self
                .bindings
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut matches = bindings.iter().filter(|binding| {
                binding.object.same_identity(object)
                    && binding.object.matches_root(
                        match &call.receiver {
                            MirRuntimeValue::NativeOwned(carrier) => carrier,
                            _ => return false,
                        },
                    )
                    && same_method(&binding.identity, &call.identity)
            });
            matches.next().map(|binding| {
                if binding.signature == call.signature {
                    Ok((binding.object.clone(), binding.handler.clone()))
                } else {
                    Err(NativeInterfaceError::SignatureMismatch)
                }
            })
        };
        let outcome = match binding {
            None => Err(NativeInterfaceError::MissingBinding),
            Some(Err(error)) => Err(error),
            Some(Ok((object, handler))) => match handler(&object, &mut call) {
                Ok(value) => Ok(value),
                Err(detail) => Err(NativeInterfaceError::Handler(detail)),
            },
        };
        if let Err(error) = call.validate_completed(&outcome) {
            return NativeInterfaceCompletion {
                outcome: Err(error),
                call,
            };
        }
        NativeInterfaceCompletion { outcome, call }
    }
    fn object_for_callable_carrier(
        &self,
        identity: &NativeCallableIdentity,
        signature: &NativeCallableSignature,
        carrier: &MirNativeOwned,
    ) -> Result<NativeInterfaceObject, NativeInterfaceError> {
        let bindings = self
            .callable_bindings
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut objects = bindings.iter().filter(|binding| {
            same_callable(&binding.identity, identity)
                && binding.signature == *signature
                && binding.object.matches_root(carrier)
        });
        let binding = objects.next().ok_or(NativeInterfaceError::MissingBinding)?;
        if objects.next().is_some() {
            return Err(NativeInterfaceError::AmbiguousBinding);
        }
        Ok(binding.object.clone())
    }
    fn object_for_callable_identity(
        &self,
        identity: &NativeCallableIdentity,
        signature: &NativeCallableSignature,
    ) -> Result<NativeInterfaceObject, NativeInterfaceError> {
        let bindings = self
            .callable_bindings
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut matches = bindings.iter().filter(|binding| {
            same_callable(&binding.identity, identity) && binding.signature == *signature
        });
        let binding = matches.next().ok_or(NativeInterfaceError::MissingBinding)?;
        if matches.next().is_some() {
            return Err(NativeInterfaceError::AmbiguousBinding);
        }
        Ok(binding.object.clone())
    }

    fn binding_for_callable_carrier(
        &self,
        signature: &NativeCallableSignature,
        carrier: &MirNativeOwned,
    ) -> Result<(NativeCallableIdentity, NativeInterfaceObject), NativeInterfaceError> {
        let bindings = self
            .callable_bindings
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut matches = bindings.iter().filter(|binding| {
            binding.signature == *signature && binding.object.matches_root(carrier)
        });
        let binding = matches.next().ok_or(NativeInterfaceError::MissingBinding)?;
        if matches.next().is_some() {
            return Err(NativeInterfaceError::AmbiguousBinding);
        }
        Ok((binding.identity.clone(), binding.object.clone()))
    }

    fn dispatch_callable(
        &self,
        execution: &MirExecutionIdentity,
        artifact: MirArtifactId,
        mut call: NativeCallableCall,
    ) -> NativeCallableCompletion {
        let Some(identity) = call.identity.as_ref() else {
            return NativeCallableCompletion {
                outcome: Err(NativeInterfaceError::MissingBinding),
                call,
            };
        };
        if identity.execution != *execution || identity.artifact != artifact {
            return NativeCallableCompletion {
                outcome: Err(NativeInterfaceError::ExecutionMismatch),
                call,
            };
        }
        let Some(object) = call.object.as_ref() else {
            return NativeCallableCompletion {
                outcome: Err(NativeInterfaceError::ReceiverMismatch),
                call,
            };
        };
        let Some(carrier) = call.carrier.as_ref() else {
            return NativeCallableCompletion {
                outcome: Err(NativeInterfaceError::ReceiverMismatch),
                call,
            };
        };
        if !object.matches_root(carrier) {
            return NativeCallableCompletion {
                outcome: Err(NativeInterfaceError::ReceiverMismatch),
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
                    binding.object.same_identity(object)
                        && same_callable(&binding.identity, identity)
                })
                .map(|binding| {
                    if binding.signature == call.signature {
                        Ok((binding.object.clone(), binding.handler.clone()))
                    } else {
                        Err(NativeInterfaceError::SignatureMismatch)
                    }
                })
        };
        let outcome = match handler {
            None => Err(NativeInterfaceError::MissingBinding),
            Some(Err(error)) => Err(error),
            Some(Ok((object, handler))) => match handler(&object, &mut call) {
                Ok(value) => Ok(value),
                Err(detail) => Err(NativeInterfaceError::Handler(detail)),
            },
        };
        if let Err(error) = call.validate_completed(&outcome) {
            return NativeCallableCompletion {
                outcome: Err(error),
                call,
            };
        }
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
        let expected_bound_id =
            MirTypeId(jet_foundation::MIR::stable_id("mir-trait", &trait_row.name));
        if !bounds
            .iter()
            .any(|bound| bound.id == expected_bound_id && bound.name == trait_row.name)
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
        let execution = program
            .execution_identity(Some(artifact))
            .map_err(|error| NativeInterfaceError::InvalidMetadata(error.to_string()))?;
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
    ReceiverMismatch,
    SignatureMismatch,
    ExecutionMismatch,
    IdentityExhausted,
    UnresolvedTransfers(Vec<usize>),
    Handler(String),
}

impl fmt::Display for NativeInterfaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMetadata(detail) => write!(formatter, "invalid native interface metadata: {detail}"),
            Self::InvalidCall(detail) => write!(formatter, "invalid native interface call: {detail}"),
            Self::MissingBinding => formatter.write_str("no native interface binding is active for this checked receiver"),
            Self::DuplicateBinding => formatter.write_str("native interface method binding is duplicated"),
            Self::AmbiguousBinding => formatter.write_str("native interface receiver matches more than one checked binding"),
            Self::ReceiverMismatch => formatter.write_str("native interface receiver does not carry the retained binding root"),
            Self::SignatureMismatch => formatter.write_str("native interface signature does not match its checked MIR method"),
            Self::IdentityExhausted => formatter.write_str("native interface object identity space is exhausted"),
            Self::UnresolvedTransfers(arguments) => write!(formatter, "native interface call has unresolved owned transfers for arguments {arguments:?}"),
            Self::Handler(detail) => write!(formatter, "native interface handler failed: {detail}"),
        }
    }
}

impl std::error::Error for NativeInterfaceError {}

/// Invocation-local activation guard. It is deliberately !Send: every worker
/// must push its own scope instead of assuming thread-local state follows tasks.
pub struct NativeInterfaceScope {
    token: Option<Arc<()>>,
    _not_send: PhantomData<Rc<()>>,
}

struct ActiveNativeInterface {
    token: Arc<()>,
    bindings: Arc<NativeInterfaceBindings>,
    execution: MirExecutionIdentity,
    artifact: MirArtifactId,
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

fn push_scope(
    bindings: Arc<NativeInterfaceBindings>,
    execution: MirExecutionIdentity,
    artifact: MirArtifactId,
) -> NativeInterfaceScope {
    let token = Arc::new(());
    ACTIVE_NATIVE_INTERFACES.with(|stack| {
        stack.borrow_mut().push(ActiveNativeInterface {
            token: token.clone(),
            bindings,
            execution,
            artifact,
        });
    });
    NativeInterfaceScope {
        token: Some(token),
        _not_send: PhantomData,
    }
}

impl Drop for NativeInterfaceScope {
    fn drop(&mut self) {
        let Some(token) = self.token.take() else {
            return;
        };
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
    if call.preflight_error.is_none() {
        call.preflight_error = preflight_error;
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
    if call.preflight_error.is_none() {
        call.preflight_error = preflight_error;
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
    if call.preflight_error.is_none() {
        call.preflight_error = preflight_error;
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
    let expected_bound_id =
        MirTypeId(jet_foundation::MIR::stable_id("mir-trait", &identity.trait_ref.name));
    if !matches!(identity.receiver_type.kind(), MirTypeKind::TraitObject(bounds)
        if bounds.iter().any(|bound| {
            bound.id == expected_bound_id && bound.name == identity.trait_ref.name
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

fn runtime_value_matches_type(value: &MirRuntimeValue, ty: &MirType) -> bool {
    use jet_foundation::MIR::MirTypeKind as Kind;
    match ty.kind() {
        Kind::Int => matches!(value, MirRuntimeValue::Int(_) | MirRuntimeValue::BigInt(_)),
        Kind::Float => matches!(value, MirRuntimeValue::Float { f32: false, .. }),
        Kind::Bool => matches!(value, MirRuntimeValue::Bool(_)),
        Kind::String => matches!(value, MirRuntimeValue::String(_)),
        Kind::Char => matches!(value, MirRuntimeValue::Char(_)),
        Kind::List(element) | Kind::FixedList { elem: element, .. } => {
            matches!(value, MirRuntimeValue::List(values) if values.iter().all(|value| runtime_value_matches_type(value, element)))
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
        Kind::IntN { .. } => matches!(value, MirRuntimeValue::Int(_) | MirRuntimeValue::BigInt(_)),
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
}
