//! Invocation-scoped typed owners for Source/Core resource iteration.
//!
//! This is deliberately the only JIT-side table for Source resource leases.  A
//! slot is keyed by an arena-issued raw capability plus the checked MIR handle
//! identity and exact resource kind.  Raw values are opaque capabilities: they
//! are never cast to pointers, passed to a Rust semantic evaluator, or treated
//! as an item value.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

use jet_codegen::Codegen::NativeLoopCursor::{
    NativeIter, NativeLoopResourceFactory, NativeLoopResourceKey, NativeLoopResourceOutcome,
};
use jet_foundation::MIR::{
    stable_id, MirCoreOwner, MirHandleId, MirLoopSourceKind, MirNativeCursor,
    MirNativeCursorError, MirNativeOwned, MirOwnershipMode, MirRuntimeValue, MirType, MirTypeKind,
};
use crate::backend::{SourceExecutionCompletion, SourceExecutionCompletionScope};

/// The checked handle identity emitted for the private Prelude cursor carrier.
pub fn loop_cursor_handle_id() -> MirHandleId {
    MirHandleId(stable_id("mir-handle", "core.prelude::loop_iter_cursor"))
}

/// Checked Prelude handle identity shared by physical native interface and
/// callable value roots. It is a capability namespace, never a function ID.
pub fn native_binding_handle_id() -> MirHandleId {
    MirHandleId(stable_id("mir-handle", "core.prelude::native_binding"))
}
/// Checked handle identity for a private physical Source Shared root.
pub fn shared_interop_root_handle_id() -> MirHandleId {
    MirHandleId(stable_id("mir-handle", "jet.internal::source_shared_interop_root"))
}
/// Checked handle identity for a private physical Source Shared weak root.
pub fn shared_interop_weak_root_handle_id() -> MirHandleId {
    MirHandleId(stable_id(
        "mir-handle",
        "jet.internal::source_shared_interop_weak_root",
    ))
}


/// Exact kind of a Source-owned resource slot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceResourceKind {
    PlainStream,
    LinesFile,
    /// A typed writer lease is releasable by the Source arena but is not a
    /// loop producer.  It exists so Core file creation never falls back to a
    /// raw legacy slot or a reopened path.
    FileWriter,
    LinesStdin,
    LinesProcessStream,
    ChannelReceiver,
    /// A canonical sender endpoint paired with exactly one receiver owner.
    ChannelSender,
    NativeBinding,
    SharedInteropRoot { type_identity: u64 },
    SharedInteropWeakRoot { type_identity: u64 },
    EncodingReader { reader_type: String },
    EncodingWriter { writer_type: String },
    NativeCursor,
}
impl SourceResourceKind {
    pub fn from_loop_source(source: &MirLoopSourceKind) -> Result<Self, String> {
        match source {
            MirLoopSourceKind::Plain => Ok(Self::PlainStream),
            MirLoopSourceKind::LinesFile => Ok(Self::LinesFile),
            MirLoopSourceKind::LinesStdin => Ok(Self::LinesStdin),
            MirLoopSourceKind::LinesProcessStream => Ok(Self::LinesProcessStream),
            MirLoopSourceKind::ChannelReceiver => Ok(Self::ChannelReceiver),
            MirLoopSourceKind::EncodingReader { reader_type } => {
                Ok(Self::EncodingReader {
                    reader_type: reader_type.clone(),
                })
            }
            MirLoopSourceKind::Chars => Err("character iteration is not a resource slot".to_string()),
            MirLoopSourceKind::Iterable { .. } => {
                Err("UserIterable is not a native resource slot".to_string())
            }
        }
    }

    fn matches_loop_source(&self, source: &MirLoopSourceKind) -> bool {
        match (self, source) {
            (Self::PlainStream, MirLoopSourceKind::Plain)
            | (Self::LinesFile, MirLoopSourceKind::LinesFile)
            | (Self::LinesStdin, MirLoopSourceKind::LinesStdin)
            | (Self::LinesProcessStream, MirLoopSourceKind::LinesProcessStream)
            | (Self::ChannelReceiver, MirLoopSourceKind::ChannelReceiver) => true,
            (
                Self::EncodingReader { reader_type: left },
                MirLoopSourceKind::EncodingReader { reader_type: right },
            ) => left == right,
            _ => false,
        }
    }
    pub fn as_loop_source(&self) -> Result<MirLoopSourceKind, String> {
        match self {
            Self::PlainStream => Ok(MirLoopSourceKind::Plain),
            Self::LinesFile => Ok(MirLoopSourceKind::LinesFile),
            Self::FileWriter => Err("file writer is not a loop producer kind".to_string()),
            Self::LinesStdin => Ok(MirLoopSourceKind::LinesStdin),
            Self::LinesProcessStream => Ok(MirLoopSourceKind::LinesProcessStream),
            Self::ChannelReceiver => Ok(MirLoopSourceKind::ChannelReceiver),
            Self::ChannelSender => Err("channel sender is not a loop producer kind".to_string()),
            Self::EncodingReader { reader_type } => Ok(MirLoopSourceKind::EncodingReader {
                reader_type: reader_type.clone(),
            }),
            Self::EncodingWriter { .. } => {
                Err("encoding writer is not a loop producer kind".to_string())
            }
            Self::SharedInteropRoot { .. } | Self::SharedInteropWeakRoot { .. } => {
                Err("Source Shared owners are not loop producer kinds".to_string())
            }
            Self::NativeBinding => Err("native binding is not a loop producer kind".to_string()),
            Self::NativeCursor => Err("native cursor is not a loop producer kind".to_string()),
        }
    }

    /// Map one checked Core owner registration and its checked MIR instance to
    /// the physical Source owner kind.
    ///
    /// The registration is the canonical authority for the owner name/key and
    /// ownership mode.  Its nominal identity must match the Apply spelling;
    /// an optional MIR instance identity may differ from the registration id.
    /// The live `(handle, raw, kind)` capability is validated separately by
    /// `take_owned_with_value`.
    pub fn from_core_owner_type(
        registration: &MirCoreOwner,
        owner_type: &MirType,
    ) -> Result<Self, String> {
        if registration.id.0 == 0 || registration.nominal_id.0 == 0 {
            return Err("checked Core owner registration has a zero identity".to_string());
        }
        if registration.key.is_empty() || registration.name.is_empty() {
            return Err("checked Core owner registration has an empty key or name".to_string());
        }
        if !matches!(
            registration.ownership,
            MirOwnershipMode::Owned | MirOwnershipMode::Move
        ) {
            return Err("checked Core owner registration is not owned".to_string());
        }
        let MirTypeKind::Apply { name, args } = &owner_type.kind else {
            return Err("checked Core resource owner is not a nominal Apply".to_string());
        };
        if name.id != registration.nominal_id {
            return Err(format!(
                "checked Core owner `{}` identity disagrees with its nominal registration",
                registration.name
            ));
        }
        if name.name != registration.name {
            return Err(format!(
                "checked Core owner registration `{}` disagrees with Apply name `{}`",
                registration.name, name.name
            ));
        }

        let (kind, expected_arity): (Self, usize) = match registration.name.as_str() {
            "Stream" => (Self::PlainStream, 1),
            "FileReader" => (Self::LinesFile, 0),
            "FileWriter" => (Self::FileWriter, 0),
            "StdinHandle" => (Self::LinesStdin, 0),
            "ProcessStdoutStream" | "ProcessStderrStream" => {
                (Self::LinesProcessStream, 0)
            }
            "Receiver" => (Self::ChannelReceiver, 1),
            "Sender" => (Self::ChannelSender, 1),
            "JSONReader" => (
                Self::EncodingReader {
                    reader_type: "JSONReader".to_string(),
                },
                0,
            ),
            "JSONWriter" => (
                Self::EncodingWriter {
                    writer_type: "JSONWriter".to_string(),
                },
                0,
            ),
            "JSONLReader" => (
                Self::EncodingReader {
                    reader_type: "JSONLReader".to_string(),
                },
                0,
            ),
            "JSONLWriter" => (
                Self::EncodingWriter {
                    writer_type: "JSONLWriter".to_string(),
                },
                0,
            ),
            "CSVReader" => (
                Self::EncodingReader {
                    reader_type: "CSVReader".to_string(),
                },
                0,
            ),
            "CSVWriter" => (
                Self::EncodingWriter {
                    writer_type: "CSVWriter".to_string(),
                },
                0,
            ),
            "XMLReader" => (
                Self::EncodingReader {
                    reader_type: "XMLReader".to_string(),
                },
                0,
            ),
            "XMLWriter" => (
                Self::EncodingWriter {
                    writer_type: "XMLWriter".to_string(),
                },
                0,
            ),
            "CBORReader" => (
                Self::EncodingReader {
                    reader_type: "CBORReader".to_string(),
                },
                0,
            ),
            "CBORWriter" => (
                Self::EncodingWriter {
                    writer_type: "CBORWriter".to_string(),
                },
                0,
            ),
            _ => {
                return Err(format!(
                    "unsupported checked Core resource owner `{}`",
                    registration.name
                ))
            }
        };
        if args.len() != expected_arity {
            return Err(format!(
                "checked Core resource owner `{}` has {} type arguments; expected {}",
                registration.name,
                args.len(),
                expected_arity
            ));
        }
        Ok(kind)
    }

}

impl SourceResourceHandle {
    pub fn native_loop_key(&self) -> Result<NativeLoopResourceKey, String> {
        Ok(NativeLoopResourceKey {
            handle: self.handle,
            raw: self.raw,
            source_kind: self.kind.as_loop_source()?,
        })
    }
}

/// An arena-issued, invocation-scoped capability.  `generation` is retained
/// for diagnostics and stale-lease checks; the raw slot is never recycled.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceResourceHandle {
    pub handle: MirHandleId,
    pub raw: i64,
    pub kind: SourceResourceKind,
    pub generation: u32,
}

/// Result of releasing one Source capability. A final physical channel
/// receiver returns every queued runtime value in an owned envelope; callers
/// must hand those values to Source cleanup before dropping this result.
#[derive(Debug)]
pub enum SourceResourceRelease {
    Released,
    DrainedChannel(Vec<MirRuntimeValue>),
}

impl SourceResourceRelease {
    pub fn drained_values(mut self) -> Vec<MirRuntimeValue> {
        match &mut self {
            Self::Released => Vec::new(),
            Self::DrainedChannel(values) => std::mem::take(values),
        }
    }
}

/// Failure from an origin cleanup callback. `remaining` owns the carrier only
/// when cleanup did not consume it; already-consumed failures use `None`.
#[derive(Debug)]
pub struct SourceResourceCleanupError {
    pub error: String,
    pub remaining: Option<MirRuntimeValue>,
}


/// Retirement could not hand every queued Source value to its origin owner.
/// `values` contains only carriers that were never passed to Source cleanup;
/// `arena` keeps their physical decode/resource context live for retry.
#[derive(Debug)]
pub struct SourceResourceRetireError {
    pub error: String,
    pub arena: SourceResourceArena,
    pub values: Vec<MirRuntimeValue>,
    pub completions: Vec<SourceExecutionCompletion>,
    retryable: bool,
}

#[derive(Debug)]
pub enum SourceResourceFinalizationOutcome {
    Retired {
        completions: Vec<SourceExecutionCompletion>,
    },
    Failed(SourceResourceRetireError),
}

/// Active physical resource context and exact MIR carriers handed to the
/// origin Source evaluator for terminal cleanup.
pub struct SourceResourceFinalization {
    arena: SourceResourceArena,
    values: Vec<MirRuntimeValue>,
    lease: SourceResourceLease,
}



impl SourceResourceFinalization {
    pub fn arena(&self) -> &SourceResourceArena {
        &self.arena
    }

    /// The counted capability that keeps physical slots live through this
    /// synchronous finalization invocation.
    pub fn lease(&self) -> &SourceResourceLease {
        &self.lease
    }

    /// Run cleanup within the live arena/lease scope. Successful cleanup must
    /// consume every pending carrier; completions are returned exactly to the
    /// active execution scope or the explicit retirement caller.
    pub fn with_live_scope(
        mut self,
        scope: impl FnOnce(
            &SourceResourceArena,
            &SourceResourceLease,
            &mut Vec<MirRuntimeValue>,
            &mut Vec<SourceExecutionCompletion>,
        ) -> Result<(), String>,
    ) -> Result<Vec<SourceExecutionCompletion>, SourceResourceFinalizationError> {
        let mut completions = Vec::new();
        let phase = match self.lease.activate_finalization_phase() {
            Ok(phase) => phase,
            Err(error) => {
                return Err(SourceResourceFinalizationError {
                    error,
                    values: self.values,
                    completions,
                });
            }
        };
        let result = {
            let _phase = phase;
            let _activation = self.lease.activate();
            scope(&self.arena, &self.lease, &mut self.values, &mut completions)
        };
        match result {
            Ok(()) if self.values.is_empty() => Ok(completions),
            Ok(()) => Err(SourceResourceFinalizationError {
                error: "Source finalization left carriers unconsumed".to_string(),
                values: self.values,
                completions,
            }),
            Err(error) => Err(SourceResourceFinalizationError {
                error,
                values: self.values,
                completions,
            }),
        }
    }


}

impl fmt::Debug for SourceResourceFinalization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceResourceFinalization")
            .field("arena", &self.arena)
            .field("values", &self.values)
            .finish()
    }
}

/// Finalizer failure owns only carriers that were not yet handed to Source
/// cleanup. `completions` retains every exact execution receipt observed
/// before failure, whether or not a Source carrier remains.
#[derive(Debug)]
pub struct SourceResourceFinalizationError {
    pub error: String,
    pub values: Vec<MirRuntimeValue>,
    pub completions: Vec<SourceExecutionCompletion>,
}

impl SourceResourceFinalizationError {
    pub fn into_parts(
        self,
    ) -> (
        String,
        Vec<MirRuntimeValue>,
        Vec<SourceExecutionCompletion>,
    ) {
        (self.error, self.values, self.completions)
    }
}

/// Consumer for automatic last-root retirement. It receives successful
/// unscoped completions as well as full retryable retirement failures.
pub type SourceResourceFinalizationHandler =
    Arc<dyn Fn(SourceResourceFinalizationOutcome) + Send + Sync + 'static>;

/// Explicit origin-owner handoff used for queued Source channel values. The
/// consumer converts carriers with the checked origin codec while `arena` is
/// live and performs Source cleanup exactly once.
pub type SourceResourceFinalizer = Arc<
    dyn Fn(SourceResourceFinalization)
            -> Result<Vec<SourceExecutionCompletion>, SourceResourceFinalizationError>
        + Send
        + Sync
        + 'static,
>;

fn route_completions(
    completions: Vec<SourceExecutionCompletion>,
) -> Vec<SourceExecutionCompletion> {
    let mut unrecorded = Vec::new();
    for completion in completions {
        if let Err(completion) = SourceExecutionCompletionScope::record_current(completion) {
            unrecorded.push(completion);
        }
    }
    unrecorded
}

fn route_retire_result(
    result: Result<Vec<SourceExecutionCompletion>, SourceResourceRetireError>,
) -> Result<Vec<SourceExecutionCompletion>, SourceResourceRetireError> {
    match result {
        Ok(completions) => Ok(route_completions(completions)),
        Err(mut error) => {
            error.completions = route_completions(error.completions);
            Err(error)
        }
    }
}



/// Checked identity of a physical native value root. Interface method identity
/// is intentionally absent: SourceInterfaces performs that method check after
/// it resolves the exact retained interface carrier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceNativeBindingIdentity {
    Callable(crate::SourceInterfaces::NativeCallableIdentity),
    Interface {
        execution: jet_foundation::MIR::MirExecutionIdentity,
        artifact: jet_foundation::MIR::MirArtifactId,
        receiver_type: MirType,
    },
}

impl SourceNativeBindingIdentity {
    pub fn value_type(&self) -> &MirType {
        match self {
            Self::Callable(identity) => &identity.callable_type,
            Self::Interface { receiver_type, .. } => receiver_type,
        }
    }

    fn same_scope_and_type(&self, other: &Self) -> bool {
        self == other
    }
}

/// Checked physical root for one native interface or native Fn value. The
/// retained `MirNativeOwned` carrier and object identity are physical metadata;
/// dispatch and binding-set scope validation stay with NativeAdapter and
/// SourceInterfaces.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceNativeBinding {
    carrier: MirNativeOwned,
    object: crate::SourceInterfaces::NativeInterfaceObjectId,
    identity: SourceNativeBindingIdentity,
}

impl SourceNativeBinding {
    pub fn interface(
        object: &crate::SourceInterfaces::NativeInterfaceObject,
        execution: jet_foundation::MIR::MirExecutionIdentity,
        artifact: jet_foundation::MIR::MirArtifactId,
        receiver_type: MirType,
    ) -> Result<Self, String> {
        if execution.artifact.artifact != artifact {
            return Err("native interface binding execution/artifact mismatch".to_string());
        }
        if !receiver_type.has_valid_layout() {
            return Err("native interface receiver type has invalid MIR layout".to_string());
        }
        Ok(Self {
            carrier: object.clone_root(),
            object: object.id(),
            identity: SourceNativeBindingIdentity::Interface {
                execution,
                artifact,
                receiver_type,
            },
        })
    }

    pub fn callable(
        object: &crate::SourceInterfaces::NativeInterfaceObject,
        identity: crate::SourceInterfaces::NativeCallableIdentity,
    ) -> Result<Self, String> {
        if identity.execution.artifact.artifact != identity.artifact {
            return Err("native callable binding execution/artifact mismatch".to_string());
        }
        if !identity.callable_type.has_valid_layout() {
            return Err("native callable type has invalid MIR layout".to_string());
        }
        crate::SourceInterfaces::NativeCallableSignature::checked(&identity.callable_type)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            carrier: object.clone_root(),
            object: object.id(),
            identity: SourceNativeBindingIdentity::Callable(identity),
        })
    }

    pub fn carrier(&self) -> &MirNativeOwned {
        &self.carrier
    }

    pub fn object_id(&self) -> crate::SourceInterfaces::NativeInterfaceObjectId {
        self.object
    }

    pub fn identity(&self) -> &SourceNativeBindingIdentity {
        &self.identity
    }

    pub fn value_type(&self) -> &MirType {
        self.identity.value_type()
    }

    fn matches_expected(&self, expected: &SourceNativeBindingIdentity) -> bool {
        self.identity.same_scope_and_type(expected)
    }
}

/// Rejected Source channel send. The logical value remains owned by this
/// envelope until Source handles cleanup or retries it.
#[derive(Debug)]
pub struct SourceChannelSendError {
    pub error: MirNativeCursorError,
    pub value: MirRuntimeValue,
}

impl SourceChannelSendError {
    pub fn into_parts(self) -> (MirNativeCursorError, MirRuntimeValue) {
        (self.error, self.value)
    }
}


/// Result of consuming one Source capability into another typed owner.
///
/// `consumed` is `None` when the source capability failed pre-take
/// validation and remains releasable. It is `Some` once the physical owner
/// has been taken; that source token is stale thereafter, including when
/// codec construction or destination registration returns an error.
#[derive(Debug, PartialEq)]
pub struct SourceResourceTransferOutcome {
    pub consumed: Option<SourceResourceHandle>,
    pub result: Result<SourceResourceHandle, MirNativeCursorError>,
}

/// A physical owner detached from one checked Source capability.  The lease
/// carries the consumed receipt privately; no raw capability bits are placed
/// in the queued runtime carrier.
pub struct SourceOwnedLease {
    owner: Arc<Mutex<BackendOwner>>,
    consumed: SourceResourceHandle,
    kind: SourceResourceKind,
}

impl SourceOwnedLease {
    /// Attach the checked runtime payload before moving this lease into a
    /// scheduler value.
    pub fn with_value(self, value: MirRuntimeValue) -> SourceOwnedValue {
        SourceOwnedValue {
            value,
            owner: self.owner,
            consumed: self.consumed,
            kind: self.kind,
        }
    }
    fn into_owner(self) -> Arc<Mutex<BackendOwner>> {
        self.owner
    }

    pub fn kind(&self) -> &SourceResourceKind {
        &self.kind
    }
}

/// A checked runtime payload plus the physical Source owner that was moved
/// with it.  This is the only payload accepted by `adopt_owned`; callers
/// cannot construct one without first taking a live arena capability.
pub struct SourceOwnedValue {
    owner: Arc<Mutex<BackendOwner>>,
    value: MirRuntimeValue,
    consumed: SourceResourceHandle,
    kind: SourceResourceKind,
}

/// Typed payload object stored inside `MirNativeOwned`.  It is public only so
/// native boundary code can use the MIR carrier's typed downcast; its fields
/// remain private and it can only be built from a real Source-owned value.
pub struct SourceOwnedPayload {
    value: SourceOwnedValue,
}

impl SourceOwnedPayload {
    pub fn new(value: SourceOwnedValue) -> Self {
        Self { value }
    }

    pub fn value(&self) -> &SourceOwnedValue {
        &self.value
    }

    pub fn into_value(self) -> SourceOwnedValue {
        self.value
    }
}

impl SourceOwnedValue {
    pub fn value(&self) -> &MirRuntimeValue {
        &self.value
    }


    pub fn kind(&self) -> &SourceResourceKind {
        &self.kind
    }

    /// Wrap the payload and lease in the genuine non-serializable MIR carrier.
    pub fn into_runtime_value(self) -> MirRuntimeValue {
        MirRuntimeValue::NativeOwned(MirNativeOwned::new(SourceOwnedPayload::new(self)))
    }
}

/// Recover one uniquely-owned Source payload from the MIR native-owned
/// carrier.  Aliased carriers are rejected instead of cloning a physical
/// owner or inventing a second logical capability.
pub fn source_owned_from_runtime_value(
    value: MirRuntimeValue,
) -> Result<SourceOwnedValue, MirNativeCursorError> {
    let MirRuntimeValue::NativeOwned(carrier) = value else {
        return Err(MirNativeCursorError::internal(
            "expected a native-owned Source payload",
        ));
    };
    let carrier = carrier
        .downcast::<SourceOwnedPayload>()
        .map_err(|_| MirNativeCursorError::internal("native-owned payload type mismatch"))?;
    std::sync::Arc::try_unwrap(carrier)
        .map(SourceOwnedPayload::into_value)
        .map_err(|_| {
            MirNativeCursorError::internal(
                "native-owned Source payload is aliased and cannot be adopted",
            )
        })
}

/// Failure while attaching a logical runtime value to a consumed Source
/// owner. The value remains owned by this envelope so Source can retry or
/// perform semantic cleanup; the consumed receipt remains stale.
#[derive(Debug)]
pub struct SourceOwnedTakeError {
    pub error: MirNativeCursorError,
    pub value: MirRuntimeValue,
}

impl SourceOwnedTakeError {
    pub fn into_parts(self) -> (MirNativeCursorError, MirRuntimeValue) {
        (self.error, self.value)
    }
}

/// Failure while committing an already-detached Source owner into a
/// destination arena. Unlike a plain error, this envelope retains the
/// complete physical/logical owner for Source rollback or cleanup.
pub struct SourceOwnedTransferFailure {
    pub error: MirNativeCursorError,
    pub owned: SourceOwnedValue,
}

impl SourceOwnedTransferFailure {
    pub fn into_parts(self) -> (MirNativeCursorError, SourceOwnedValue) {
        (self.error, self.owned)
    }
}
impl std::fmt::Debug for SourceOwnedTransferFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SourceOwnedTransferFailure")
            .field("error", &self.error)
            .field("owned", &"<SourceOwnedValue>")
            .finish()
    }
}

/// Receipt returned after a detached payload is adopted by a destination
/// handle. `result` preserves the complete owned value on every failure.
pub struct SourceOwnedTransferOutcome {
    pub consumed: SourceResourceHandle,
    pub result: Result<(SourceResourceHandle, MirRuntimeValue), SourceOwnedTransferFailure>,
}

/// Cursor acquisition outcome with the source receipt preserved after a
/// by-value owner is detached.  `consumed` is stale once present; it remains
/// available when iterator construction or cursor-slot installation fails.
#[derive(Debug, PartialEq)]
pub struct SourceCursorInitOutcome {
    pub consumed: Option<SourceResourceHandle>,
    pub result: Result<(SourceResourceHandle, MirRuntimeValue), MirNativeCursorError>,
}

/// A typed file owner adopted from the canonical FileStream Prelude leaf.
pub struct SourceFileReader(crate::enc_stream::runtime::JetFileReader);

impl SourceFileReader {
    pub fn open(path: &str) -> Result<Self, MirNativeCursorError> {
        crate::enc_stream::source_file_open(path)
            .map(Self)
            .map_err(file_error)
    }

    /// Consume a legacy JIT FileReader slot at the typed owner boundary.
    pub(crate) fn from_handle(handle: i64) -> Result<Self, String> {
        crate::enc_stream::source_take_file_reader(handle).map(Self)
    }
}

/// A typed writer owner.  Source keeps this owner in the same invocation
/// arena as readers so create/append never reopen a path or expose a slot ID.
pub struct SourceFileWriter(crate::enc_stream::runtime::JetFileWriter);

impl SourceFileWriter {
    pub fn create(path: &str) -> Result<Self, MirNativeCursorError> {
        crate::enc_stream::source_file_create(path)
            .map(Self)
            .map_err(file_error)
    }

    pub fn append(path: &str) -> Result<Self, MirNativeCursorError> {
        crate::enc_stream::source_file_append(path)
            .map(Self)
            .map_err(file_error)
    }

    /// Consume a legacy JIT FileWriter slot at the typed owner boundary.
    pub(crate) fn from_handle(handle: i64) -> Result<Self, String> {
        crate::enc_stream::source_take_file_writer(handle).map(Self)
    }
}

/// A process stdout/stderr owner whose reader lease is safe to move into a
/// native cursor task.  Construction from a checked ProcessChild stays in the
/// ProcessPrelude owner module.
pub struct SourceProcessStream(crate::ProcessPrelude::process_prelude::SourceProcessReader);

impl SourceProcessStream {
    pub(crate) fn from_child(
        child: &crate::ProcessPrelude::process_prelude::ProcessChild,
        stream: crate::ProcessPrelude::process_prelude::SourceProcessStreamKind,
    ) -> Result<Self, String> {
        crate::ProcessPrelude::process_prelude::source_take_process_stream(child, stream).map(Self)
    }
}
/// Public projection of the canonical encoding limits used when an owner is
/// acquired.  The runtime reader keeps the typed `JetOutcome` carrier
/// internally; Source callers only supply the checked scalar facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceEncodingLimits {
    pub buffer_bytes: i64,
    pub max_depth: i64,
    pub max_item_bytes: i64,
    pub max_total_bytes: Option<i64>,
    pub max_expansion_depth: i64,
    pub max_expansion_bytes: i64,
}

impl SourceEncodingLimits {
    pub fn safe() -> Self {
        let limits = crate::enc_stream::runtime::jet_std::EncodingLimits::safe();
        Self {
            buffer_bytes: limits.buffer_bytes,
            max_depth: limits.max_depth,
            max_item_bytes: limits.max_item_bytes,
            max_total_bytes: match limits.max_total_bytes {
                Ok(value) => Some(value),
                Err(_) => None,
            },
            max_expansion_depth: limits.max_expansion_depth,
            max_expansion_bytes: limits.max_expansion_bytes,
        }
    }

    fn into_runtime(self) -> crate::enc_stream::runtime::jet_std::EncodingLimits {
        crate::enc_stream::runtime::jet_std::EncodingLimits {
            buffer_bytes: self.buffer_bytes,
            max_depth: self.max_depth,
            max_item_bytes: self.max_item_bytes,
            max_total_bytes: self
                .max_total_bytes
                .map(Ok)
                .unwrap_or(Err(jet_foundation::Outcome::JetAbsent)),
            max_expansion_depth: self.max_expansion_depth,
            max_expansion_bytes: self.max_expansion_bytes,
        }
    }
}

/// Checked XML parser limits projected from the shared Prelude carrier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceXmlLimits {
    pub max_depth: i64,
    pub max_nodes: i64,
    pub max_attributes_per_element: i64,
    pub max_name_bytes: i64,
    pub max_text_bytes: i64,
    pub max_entity_declarations: i64,
    pub max_entity_depth: i64,
    pub max_entity_replacement_bytes: i64,
}

impl SourceXmlLimits {
    fn from_runtime(
        limits: crate::enc_stream::runtime::jet_std::XMLLimits,
    ) -> Self {
        Self {
            max_depth: limits.max_depth,
            max_nodes: limits.max_nodes,
            max_attributes_per_element: limits.max_attributes_per_element,
            max_name_bytes: limits.max_name_bytes,
            max_text_bytes: limits.max_text_bytes,
            max_entity_declarations: limits.max_entity_declarations,
            max_entity_depth: limits.max_entity_depth,
            max_entity_replacement_bytes: limits.max_entity_replacement_bytes,
        }
    }

    pub fn safe() -> Self {
        Self::from_runtime(
            crate::enc_stream::runtime::jet_std::XMLLimits::safe(),
        )
    }

    fn into_runtime(self) -> crate::enc_stream::runtime::jet_std::XMLLimits {
        crate::enc_stream::runtime::jet_std::XMLLimits {
            max_depth: self.max_depth,
            max_nodes: self.max_nodes,
            max_attributes_per_element: self.max_attributes_per_element,
            max_name_bytes: self.max_name_bytes,
            max_text_bytes: self.max_text_bytes,
            max_entity_declarations: self.max_entity_declarations,
            max_entity_depth: self.max_entity_depth,
            max_entity_replacement_bytes: self.max_entity_replacement_bytes,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceXmlEntityPolicy {
    Preserve,
    Reject,
    Resolve(std::collections::BTreeMap<String, String>),
}

impl SourceXmlEntityPolicy {
    fn from_runtime(
        policy: crate::enc_stream::runtime::jet_std::XMLEntityPolicy,
    ) -> Self {
        match policy {
            crate::enc_stream::runtime::jet_std::XMLEntityPolicy::Preserve => Self::Preserve,
            crate::enc_stream::runtime::jet_std::XMLEntityPolicy::Reject => Self::Reject,
            crate::enc_stream::runtime::jet_std::XMLEntityPolicy::Resolve(values) => {
                Self::Resolve(values)
            }
        }
    }

    fn into_runtime(self) -> crate::enc_stream::runtime::jet_std::XMLEntityPolicy {
        match self {
            Self::Preserve => crate::enc_stream::runtime::jet_std::XMLEntityPolicy::Preserve,
            Self::Reject => crate::enc_stream::runtime::jet_std::XMLEntityPolicy::Reject,
            Self::Resolve(values) => {
                crate::enc_stream::runtime::jet_std::XMLEntityPolicy::Resolve(values)
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceXmlParseOptions {
    pub entities: SourceXmlEntityPolicy,
    pub limits: SourceXmlLimits,
}

impl SourceXmlParseOptions {
    pub fn safe() -> Self {
        let options = crate::enc_stream::runtime::jet_std::XMLParseOptions::safe();
        Self {
            entities: SourceXmlEntityPolicy::from_runtime(options.entities),
            limits: SourceXmlLimits::from_runtime(options.limits),
        }
    }

    fn into_runtime(self) -> crate::enc_stream::runtime::jet_std::XMLParseOptions {
        crate::enc_stream::runtime::jet_std::XMLParseOptions {
            entities: self.entities.into_runtime(),
            limits: self.limits.into_runtime(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceXmlEncoding {
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
}

impl SourceXmlEncoding {
    fn from_runtime(
        encoding: crate::enc_stream::runtime::jet_std::XMLEncoding,
    ) -> Self {
        match encoding {
            crate::enc_stream::runtime::jet_std::XMLEncoding::UTF8 => Self::Utf8,
            crate::enc_stream::runtime::jet_std::XMLEncoding::UTF8BOM => Self::Utf8Bom,
            crate::enc_stream::runtime::jet_std::XMLEncoding::UTF16LE => Self::Utf16Le,
            crate::enc_stream::runtime::jet_std::XMLEncoding::UTF16BE => Self::Utf16Be,
        }
    }

    fn into_runtime(self) -> crate::enc_stream::runtime::jet_std::XMLEncoding {
        match self {
            Self::Utf8 => crate::enc_stream::runtime::jet_std::XMLEncoding::UTF8,
            Self::Utf8Bom => crate::enc_stream::runtime::jet_std::XMLEncoding::UTF8BOM,
            Self::Utf16Le => crate::enc_stream::runtime::jet_std::XMLEncoding::UTF16LE,
            Self::Utf16Be => crate::enc_stream::runtime::jet_std::XMLEncoding::UTF16BE,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceXmlLexicalPolicy {
    PreserveValid,
    Deterministic,
}

impl SourceXmlLexicalPolicy {
    fn from_runtime(
        policy: crate::enc_stream::runtime::jet_std::XMLLexicalPolicy,
    ) -> Self {
        match policy {
            crate::enc_stream::runtime::jet_std::XMLLexicalPolicy::PreserveValid => {
                Self::PreserveValid
            }
            crate::enc_stream::runtime::jet_std::XMLLexicalPolicy::Deterministic => {
                Self::Deterministic
            }
        }
    }

    fn into_runtime(self) -> crate::enc_stream::runtime::jet_std::XMLLexicalPolicy {
        match self {
            Self::PreserveValid => {
                crate::enc_stream::runtime::jet_std::XMLLexicalPolicy::PreserveValid
            }
            Self::Deterministic => {
                crate::enc_stream::runtime::jet_std::XMLLexicalPolicy::Deterministic
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceXmlRenderOptions {
    pub encoding: SourceXmlEncoding,
    pub lexical: SourceXmlLexicalPolicy,
}

impl SourceXmlRenderOptions {
    pub fn safe() -> Self {
        let options = crate::enc_stream::runtime::jet_std::XMLRenderOptions::safe();
        Self {
            encoding: SourceXmlEncoding::from_runtime(options.encoding),
            lexical: SourceXmlLexicalPolicy::from_runtime(options.lexical),
        }
    }

    fn into_runtime(self) -> crate::enc_stream::runtime::jet_std::XMLRenderOptions {
        crate::enc_stream::runtime::jet_std::XMLRenderOptions {
            encoding: self.encoding.into_runtime(),
            lexical: self.lexical.into_runtime(),
        }
    }
}


/// An encoding reader owner.  The concrete reader remains private so callers
/// can only obtain it through the canonical typed constructors below.
pub struct SourceEncodingReader(EncodingReader);

enum EncodingReader {
    Json(crate::enc_stream::runtime::jet_std::JSONReader),
    Jsonl(crate::enc_stream::runtime::jet_std::JSONLReader),
    Csv(crate::enc_stream::runtime::jet_std::CSVReader),
    Xml(crate::enc_stream::runtime::jet_std::XMLReader),
    Cbor(crate::enc_stream::runtime::jet_std::CBORReader),
}

impl SourceEncodingReader {
    fn validate_type(reader_type: &str) -> Result<(), MirNativeCursorError> {
        if matches!(
            reader_type,
            "JSONReader" | "JSONLReader" | "CSVReader" | "XMLReader" | "CBORReader"
        ) {
            Ok(())
        } else {
            Err(MirNativeCursorError::internal(format!(
                "unknown checked encoding reader type `{reader_type}`"
            )))
        }
    }

    pub fn from_file(
        file: SourceFileReader,
        reader_type: &str,
    ) -> Result<Self, MirNativeCursorError> {
        Self::from_file_with_limits(file, reader_type, SourceEncodingLimits::safe())
    }

    pub fn from_file_with_limits(
        file: SourceFileReader,
        reader_type: &str,
        limits: SourceEncodingLimits,
    ) -> Result<Self, MirNativeCursorError> {
        Self::from_file_with_options(file, reader_type, limits, ",", false, false)
    }

    pub fn from_file_with_options(
        file: SourceFileReader,
        reader_type: &str,
        limits: SourceEncodingLimits,
        delimiter: &str,
        header: bool,
        skip_blank: bool,
    ) -> Result<Self, MirNativeCursorError> {
        Self::from_file_with_xml_options(
            file,
            reader_type,
            limits,
            delimiter,
            header,
            skip_blank,
            SourceXmlParseOptions::safe(),
        )
    }

    pub fn from_file_with_xml_options(
        file: SourceFileReader,
        reader_type: &str,
        limits: SourceEncodingLimits,
        delimiter: &str,
        header: bool,
        skip_blank: bool,
        xml_options: SourceXmlParseOptions,
    ) -> Result<Self, MirNativeCursorError> {
        let limits = limits.into_runtime();
        let xml_options = xml_options.into_runtime();
        let reader = match reader_type {
            "JSONReader" => crate::enc_stream::runtime::enc_json_reader(file.0, limits.clone())
                .map(EncodingReader::Json),
            "JSONLReader" => crate::enc_stream::runtime::enc_jsonl_reader(file.0, limits.clone())
                .map(EncodingReader::Jsonl),
            "CSVReader" => crate::enc_stream::runtime::enc_csv_reader(
                file.0,
                limits.clone(),
                delimiter.to_string(),
                header,
                skip_blank,
            )
            .map(EncodingReader::Csv),
            "XMLReader" => crate::enc_stream::runtime::enc_xml_reader(
                file.0,
                limits.clone(),
                xml_options,
            )
            .map(EncodingReader::Xml),
            "CBORReader" => crate::enc_stream::runtime::enc_cbor_reader(file.0, limits)
                .map(EncodingReader::Cbor),
            _ => {
                return Err(MirNativeCursorError::internal(format!(
                    "unknown checked encoding reader type `{reader_type}`"
                )))
            }
        };
        reader
            .map(Self)
            .map_err(|error| encoding_error(&error))
    }

    /// Consume a resident typed codec reader without reopening its input
    /// file.  The slot is moved exactly once by the enc_stream boundary.
    pub(crate) fn from_handle(handle: i64, reader_type: &str) -> Result<Self, String> {
        let reader = match reader_type {
            "JSONReader" => crate::enc_stream::source_take_json_reader(handle)
                .map(EncodingReader::Json),
            "JSONLReader" => crate::enc_stream::source_take_jsonl_reader(handle)
                .map(EncodingReader::Jsonl),
            "CSVReader" => crate::enc_stream::source_take_csv_reader(handle)
                .map(EncodingReader::Csv),
            "XMLReader" => crate::enc_stream::source_take_xml_reader(handle)
                .map(EncodingReader::Xml),
            "CBORReader" => crate::enc_stream::source_take_cbor_reader(handle)
                .map(EncodingReader::Cbor),
            _ => return Err(format!("unknown checked encoding reader type `{reader_type}`")),
        }?;
        Ok(Self(reader))
    }

    fn reader_type(&self) -> &'static str {
        match &self.0 {
            EncodingReader::Json(_) => "JSONReader",
            EncodingReader::Jsonl(_) => "JSONLReader",
            EncodingReader::Csv(_) => "CSVReader",
            EncodingReader::Xml(_) => "XMLReader",
            EncodingReader::Cbor(_) => "CBORReader",
        }
    }
}

/// An encoding writer owner. The output file lease is moved into the codec
/// exactly once; no path is reopened and no legacy integer slot is retained.
pub struct SourceEncodingWriter(EncodingWriter);

enum EncodingWriter {
    Json(crate::enc_stream::runtime::jet_std::JSONWriter),
    Jsonl(crate::enc_stream::runtime::jet_std::JSONLWriter),
    Csv(crate::enc_stream::runtime::jet_std::CSVWriter),
    Xml(crate::enc_stream::runtime::jet_std::XMLWriter),
    Cbor(crate::enc_stream::runtime::jet_std::CBORWriter),
}

impl SourceEncodingWriter {
    fn validate_type(writer_type: &str) -> Result<(), MirNativeCursorError> {
        if matches!(
            writer_type,
            "JSONWriter" | "JSONLWriter" | "CSVWriter" | "XMLWriter" | "CBORWriter"
        ) {
            Ok(())
        } else {
            Err(MirNativeCursorError::internal(format!(
                "unknown checked encoding writer type `{writer_type}`"
            )))
        }
    }

    pub fn from_file(
        file: SourceFileWriter,
        writer_type: &str,
    ) -> Result<Self, MirNativeCursorError> {
        Self::from_file_with_options(file, writer_type, SourceEncodingLimits::safe(), false)
    }

    pub fn from_file_with_limits(
        file: SourceFileWriter,
        writer_type: &str,
        limits: SourceEncodingLimits,
    ) -> Result<Self, MirNativeCursorError> {
        Self::from_file_with_options(file, writer_type, limits, false)
    }

    pub fn from_file_with_options(
        file: SourceFileWriter,
        writer_type: &str,
        limits: SourceEncodingLimits,
        canonical: bool,
    ) -> Result<Self, MirNativeCursorError> {
        Self::from_file_with_xml_options(
            file,
            writer_type,
            limits,
            canonical,
            SourceXmlRenderOptions::safe(),
        )
    }

    pub fn from_file_with_xml_options(
        file: SourceFileWriter,
        writer_type: &str,
        limits: SourceEncodingLimits,
        canonical: bool,
        xml_options: SourceXmlRenderOptions,
    ) -> Result<Self, MirNativeCursorError> {
        let limits = limits.into_runtime();
        let xml_options = xml_options.into_runtime();
        let writer = match writer_type {
            "JSONWriter" => crate::enc_stream::runtime::enc_json_writer(file.0, limits.clone(), canonical)
                .map(EncodingWriter::Json),
            "JSONLWriter" => crate::enc_stream::runtime::enc_jsonl_writer(file.0, limits.clone())
                .map(EncodingWriter::Jsonl),
            "CSVWriter" => crate::enc_stream::runtime::enc_csv_writer(file.0, limits.clone())
                .map(EncodingWriter::Csv),
            "XMLWriter" => crate::enc_stream::runtime::enc_xml_writer(
                file.0,
                limits.clone(),
                xml_options,
            )
            .map(EncodingWriter::Xml),
            "CBORWriter" => crate::enc_stream::runtime::enc_cbor_writer(file.0, limits)
                .map(EncodingWriter::Cbor),
            _ => {
                return Err(MirNativeCursorError::internal(format!(
                    "unknown checked encoding writer type `{writer_type}`"
                )))
            }
        };
        writer
            .map(Self)
            .map_err(|error| encoding_error(&error))
    }

    fn writer_type(&self) -> &'static str {
        match &self.0 {
            EncodingWriter::Json(_) => "JSONWriter",
            EncodingWriter::Jsonl(_) => "JSONLWriter",
            EncodingWriter::Csv(_) => "CSVWriter",
            EncodingWriter::Xml(_) => "XMLWriter",
            EncodingWriter::Cbor(_) => "CBORWriter",
        }
    }
}

/// One owner admitted to the invocation-scoped arena.
enum BackendOwner {
    PlainStream(jet_codegen::scheduler::JetStream<MirRuntimeValue>),
    File(SourceFileReader),
    FileWriter(SourceFileWriter),
    Stdin(crate::enc_stream::SourceStdinReader),
    Process(SourceProcessStream),
    ChannelReceiver(jet_codegen::scheduler::JetSchedulerChannel<MirRuntimeValue>),
    ChannelSender(jet_codegen::scheduler::JetSchedulerSender<MirRuntimeValue>),
    SharedInteropRoot(crate::SourceSharedInterop::SourceSharedInterop),
    SharedInteropWeakRoot {
        type_identity: u64,
        root: Arc<crate::SourceSharedInterop::SourceSharedInteropWeak>,
    },
    NativeBinding(SourceNativeBinding),
    Encoding(SourceEncodingReader),
    EncodingWriter(SourceEncodingWriter),
}
impl BackendOwner {
    fn kind(&self) -> SourceResourceKind {
        match self {
            Self::PlainStream(_) => SourceResourceKind::PlainStream,
            Self::File(_) => SourceResourceKind::LinesFile,
            Self::FileWriter(_) => SourceResourceKind::FileWriter,
            Self::Stdin(_) => SourceResourceKind::LinesStdin,
            Self::Process(_) => SourceResourceKind::LinesProcessStream,
            Self::ChannelReceiver(_) => SourceResourceKind::ChannelReceiver,
            Self::ChannelSender(_) => SourceResourceKind::ChannelSender,
            Self::NativeBinding(_) => SourceResourceKind::NativeBinding,
            Self::SharedInteropRoot(root) => SourceResourceKind::SharedInteropRoot {
                type_identity: root.type_id(),
            },
            Self::SharedInteropWeakRoot {
                type_identity,
                ..
            } => SourceResourceKind::SharedInteropWeakRoot {
                type_identity: *type_identity,
            },
            Self::Encoding(reader) => SourceResourceKind::EncodingReader {
                reader_type: reader.reader_type().to_string(),
            },
            Self::EncodingWriter(writer) => SourceResourceKind::EncodingWriter {
                writer_type: writer.writer_type().to_string(),
            },
        }
    }
}

trait FromBackendOwner: Sized {
    fn from_backend_owner(owner: BackendOwner) -> Result<Self, BackendOwner>;
}

impl FromBackendOwner for SourceFileReader {
    fn from_backend_owner(owner: BackendOwner) -> Result<Self, BackendOwner> {
        match owner {
            BackendOwner::File(reader) => Ok(reader),
            owner => Err(owner),
        }
    }
}

impl FromBackendOwner for SourceFileWriter {
    fn from_backend_owner(owner: BackendOwner) -> Result<Self, BackendOwner> {
        match owner {
            BackendOwner::FileWriter(writer) => Ok(writer),
            owner => Err(owner),
        }
    }
}

enum SlotEntry {
    Backend(Arc<Mutex<BackendOwner>>),
    Cursor(MirNativeCursor),
}

struct Slot {
    handle: MirHandleId,
    kind: SourceResourceKind,
    generation: u32,
    entry: SlotEntry,
}

// Capability generations are globally monotonic, but this atomic is only a
// nonce allocator: owners remain in the invocation-local arena below.
static NEXT_CAPABILITY_GENERATION: AtomicU32 = AtomicU32::new(0);

struct ArenaState {
    next_slot: u32,
    slots: HashMap<u32, Slot>,
    /// `None` marks an alias whose owner token is being dropped outside this
    /// mutex; retirement must wait until the release guard removes that entry.
    shared_interop_aliases:
        HashMap<(i64, i64), Option<crate::SourceSharedInterop::SourceSharedInteropOwnerAlias>>,
    /// New roots are rejected once retirement has been requested.
    retirement_requested: bool,
    /// Explicit Source callback/task roots, never inferred from Arc clones.
    retained_roots: usize,
    finalizing: bool,
    retired: bool,
    finalizer: Option<SourceResourceFinalizer>,
    finalization_handler: Option<SourceResourceFinalizationHandler>,
}


thread_local! {
    static ACTIVE_SOURCE_ARENA: RefCell<Option<SourceResourceArena>> =
        const { RefCell::new(None) };
    static ACTIVE_SOURCE_RESOURCE_LEASE: RefCell<Option<SourceResourceLease>> =
        const { RefCell::new(None) };
}

/// Sole invocation-scoped source resource authority. Clones share the slot
/// table and therefore retain the same concrete owners until explicit
/// capability release. An explicit `SourceResourceLease`, not an arena
/// `Arc` clone, keeps the table alive across session retirement.
#[derive(Clone)]
pub struct SourceResourceArena {
    state: Arc<Mutex<ArenaState>>,
}

impl fmt::Debug for SourceResourceArena {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SourceResourceArena").finish_non_exhaustive()
    }
}

/// Session ownership for callbacks that return live Source capabilities.
/// Individual outcome handles are released through `resource_release` or
/// `release_capability`.  `retire` requests logical retirement; an explicit
/// `SourceResourceLease` keeps the physical arena usable until its last
/// release.
#[derive(Clone)]
pub struct SourceResourceSession {
    arena: SourceResourceArena,

}

struct SourceResourceLeaseInner {
    arena: SourceResourceArena,
    finalization_root: bool,
    finalization_phase: Option<Arc<AtomicBool>>,
}

struct SourceResourceFinalizationPhase<'a>(&'a AtomicBool);

impl Drop for SourceResourceFinalizationPhase<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
/// An explicit retained physical-root lease for one Source callback/task
/// graph. Arena `Arc` clones are lookup/activation handles only and do not
/// keep an arena alive after retirement. Lease aliases share one explicit
/// retained-root count; only the final alias releases it.
pub struct SourceResourceLease {
    inner: Arc<SourceResourceLeaseInner>,
}

/// Non-owning handle to a counted Source resource lease. It cannot keep the
/// arena alive; upgrading only aliases a lease that still has a strong owner.
#[derive(Clone)]
pub struct SourceResourceLeaseWeak {
    inner: Weak<SourceResourceLeaseInner>,
}

/// Checked permission for narrowly-scoped resource creation/adoption while
/// Source finalization is active. It can be created only from the finalizer's
/// counted lease and is invalid as soon as that finalization ends.
#[derive(Clone)]
pub struct SourceResourceCleanupLease {
    inner: Arc<SourceResourceLeaseInner>,
}

struct SharedInteropAliasReleaseGuard<'a> {
    arena: &'a SourceResourceArena,
    key: (i64, i64),
}

impl Drop for SharedInteropAliasReleaseGuard<'_> {
    fn drop(&mut self) {
        let mut state = self
            .arena
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state
            .shared_interop_aliases
            .get(&self.key)
            .is_some_and(Option::is_none)
        {
            state.shared_interop_aliases.remove(&self.key);
        }
    }
}

/// Thread-local activation selects the arena and, when entered through a
/// counted lease, exposes that same lease to synchronous checked calls. The
/// guard restores prior state; a lease alias does not add another root count.
pub struct SourceResourceActivation {
    _current: SourceResourceArena,
    _current_lease: Option<SourceResourceLease>,
    previous: Option<SourceResourceArena>,
    previous_lease: Option<SourceResourceLease>,
}

impl Drop for SourceResourceActivation {
    fn drop(&mut self) {
        ACTIVE_SOURCE_ARENA.with(|slot| {
            *slot.borrow_mut() = self.previous.take();
        });
        let active_lease = ACTIVE_SOURCE_RESOURCE_LEASE.with(|slot| {
            std::mem::replace(&mut *slot.borrow_mut(), self.previous_lease.take())
        });
        drop(active_lease);
    }
}

impl Default for SourceResourceArena {
    fn default() -> Self {
        Self::new()
    }
}

impl SourceResourceSession {
    pub fn new() -> Self {
        Self::new_without_finalizer()
    }

    /// Configure Source cleanup and a separate live origin outcome consumer.
    /// The consumer receives successful completions as well as retryable
    /// retirement failures, without owning the arena through the finalizer.
    pub fn new_with_finalizer(
        finalizer: SourceResourceFinalizer,
        finalization_handler: SourceResourceFinalizationHandler,
    ) -> Self {
        Self {
            arena: SourceResourceArena::new_with_finalizer(finalizer, finalization_handler),
        }
    }

    fn new_without_finalizer() -> Self {
        Self {
            arena: SourceResourceArena::new(),
        }
    }

    pub fn arena(&self) -> SourceResourceArena {
        self.arena.clone()
    }


    pub fn set_finalizer(
        &self,
        finalizer: SourceResourceFinalizer,
        finalization_handler: SourceResourceFinalizationHandler,
    ) -> Result<(), String> {
        self.arena.set_finalizer(finalizer, finalization_handler)
    }
    /// Retain one explicit physical-root lease for an escaping callback/task
    /// graph before retirement is requested. The lease must be released by
    /// Source semantic cleanup before the arena can finish retirement.
    pub fn retain_root(&self) -> Result<SourceResourceLease, String> {
        self.arena.retain_root()
    }

    pub fn activate(&self) -> SourceResourceActivation {
        activate_source_resource_arena(&self.arena)
    }

    pub fn retire(&self) -> Result<Vec<SourceExecutionCompletion>, SourceResourceRetireError> {
        self.arena.retire()
    }

    pub fn retire_with_cleanup<F>(
        &self,
        cleanup: F,
    ) -> Result<Vec<SourceExecutionCompletion>, SourceResourceRetireError>
    where
        F: FnMut(MirRuntimeValue) -> Result<(), SourceResourceCleanupError>,
    {
        self.arena.retire_with_cleanup(cleanup)
    }

    pub fn retry_retire(
        &self,
        error: SourceResourceRetireError,
    ) -> Result<Vec<SourceExecutionCompletion>, SourceResourceRetireError> {
        self.arena.retry_retire(error)
    }

    pub fn retry_retire_with_cleanup<F>(
        &self,
        error: SourceResourceRetireError,
        cleanup: F,
    ) -> Result<Vec<SourceExecutionCompletion>, SourceResourceRetireError>
    where
        F: FnMut(MirRuntimeValue) -> Result<(), SourceResourceCleanupError>,
    {
        self.arena.retry_retire_with_cleanup(error, cleanup)
    }

    pub fn is_retirement_requested(&self) -> Result<bool, String> {
        self.arena.is_retirement_requested()
    }

    pub fn retained_root_count(&self) -> Result<usize, String> {
        self.arena.retained_root_count()
    }
}

impl SourceResourceLease {
    fn activate_finalization_phase(
        &self,
    ) -> Result<SourceResourceFinalizationPhase<'_>, String> {
        let phase = self
            .inner
            .finalization_phase
            .as_deref()
            .ok_or_else(|| "Source lease is not a finalization authority".to_string())?;
        phase
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "Source finalization phase is already active".to_string())?;
        Ok(SourceResourceFinalizationPhase(phase))
    }

    /// Derive cleanup-only authority from the active finalizer lease. It does
    /// not add another retained root; all operations revalidate that this
    /// exact finalization is still active.
    pub fn cleanup_lease(&self) -> Result<SourceResourceCleanupLease, String> {
        self.inner.arena.validate_cleanup_lease(self)?;
        Ok(SourceResourceCleanupLease {
            inner: self.inner.clone(),
        })
    }
}

impl SourceResourceLeaseWeak {
    /// Recover an alias to the existing counted lease while any strong alias
    /// remains. This does not create another retained root.
    pub fn upgrade(&self) -> Option<SourceResourceLease> {
        self.inner
            .upgrade()
            .map(|inner| SourceResourceLease { inner })
    }
}

impl SourceResourceCleanupLease {
    pub fn arena(&self) -> SourceResourceArena {
        self.inner.arena.clone()
    }

    /// Register one fresh native binding for a logical helper scope created
    /// while cleaning a Source-owned value.
    pub fn register_native_binding_root(
        &self,
        binding: SourceNativeBinding,
    ) -> Result<SourceResourceHandle, String> {
        self.inner
            .arena
            .register_cleanup_native_binding_root(self, binding)
    }

    /// Re-adopt only a physical owner already detached from a checked Source
    /// capability. The logical value and consumed receipt stay in the outcome.
    pub fn adopt_owned(
        &self,
        target_handle: MirHandleId,
        expected_kind: &SourceResourceKind,
        owned: SourceOwnedValue,
    ) -> SourceOwnedTransferOutcome {
        self.inner
            .arena
            .adopt_owned_with_cleanup(self, target_handle, expected_kind, owned)
    }
}

impl SourceResourceLease {
    /// Obtain the shared arena retained by this physical-root lease.
    pub fn arena(&self) -> SourceResourceArena {
        self.inner.arena.clone()
    }

    /// Create a non-owning reference for callbacks that must not form an
    /// arena-to-owner-to-callback-to-lease cycle.
    pub fn downgrade(&self) -> SourceResourceLeaseWeak {
        SourceResourceLeaseWeak {
            inner: Arc::downgrade(&self.inner),
        }
    }

    /// Activate the retained arena and counted lease for one owner-pump callback.
    pub fn activate(&self) -> SourceResourceActivation {
        activate_source_resource_context(&self.inner.arena, Some(self.clone()))
    }

    pub fn is_retired(&self) -> Result<bool, String> {
        self.inner.arena.is_retired()
    }

    pub fn retire_with_cleanup<F>(
        &self,
        cleanup: F,
    ) -> Result<Vec<SourceExecutionCompletion>, SourceResourceRetireError>
    where
        F: FnMut(MirRuntimeValue) -> Result<(), SourceResourceCleanupError>,
    {
        self.inner.arena.retire_with_cleanup(cleanup)
    }
}

impl Clone for SourceResourceLease {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Drop for SourceResourceLeaseInner {
    fn drop(&mut self) {
        self.arena.release_root();
    }
}

impl Default for SourceResourceSession {
    fn default() -> Self {
        Self::new()
    }
}

/// Activate an existing session-owned arena and restore the prior activation
/// on normal return or unwind. Arena-only activation has no counted lease.
pub fn activate_source_resource_arena(
    arena: &SourceResourceArena,
) -> SourceResourceActivation {
    activate_source_resource_context(arena, None)
}

fn activate_source_resource_context(
    arena: &SourceResourceArena,
    lease: Option<SourceResourceLease>,
) -> SourceResourceActivation {
    let previous = ACTIVE_SOURCE_ARENA.with(|slot| slot.borrow_mut().replace(arena.clone()));
    let previous_lease = ACTIVE_SOURCE_RESOURCE_LEASE.with(|slot| {
        std::mem::replace(&mut *slot.borrow_mut(), lease.clone())
    });
    SourceResourceActivation {
        _current: arena.clone(),
        _current_lease: lease,
        previous,
        previous_lease,
    }
}

/// Establish a fresh, callback-local arena. Callers returning live
/// capabilities must instead retain a `SourceResourceSession`, obtain an
/// explicit `SourceResourceLease`, and activate that lease around each callback.
pub fn with_source_resource_arena<R>(
    body: impl FnOnce(&SourceResourceArena) -> R,
) -> R {
    let arena = SourceResourceArena::new();
    let _activation = activate_source_resource_arena(&arena);
    body(&arena)
}

/// Obtain the active invocation arena for a plain bootstrap function pointer.
/// The clone shares the scoped table; it does not create a second registry.
pub fn active_source_resource_arena() -> Option<SourceResourceArena> {
    ACTIVE_SOURCE_ARENA.with(|slot| slot.borrow().clone())
}

/// Access the active invocation arena for a plain bootstrap function pointer.
/// The local snapshot keeps only a lookup handle alive; it adds no root count.
pub fn with_active_source_resource_arena<R>(
    body: impl FnOnce(Option<&SourceResourceArena>) -> R,
) -> R {
    let active = ACTIVE_SOURCE_ARENA.with(|slot| slot.borrow().clone());
    body(active.as_ref())
}
/// Borrow the counted lease for the current callback activation, if it was
/// entered through `SourceResourceLease::activate`. A local alias keeps the
/// same counted root alive only for this call; do not retain it in an instance.
pub fn with_active_source_resource_lease<R>(
    body: impl FnOnce(Option<&SourceResourceLease>) -> R,
) -> R {
    let active = ACTIVE_SOURCE_RESOURCE_LEASE.with(|slot| slot.borrow().clone());
    body(active.as_ref())
}

fn active_source_resource_lease_snapshot() -> Option<SourceResourceLease> {
    ACTIVE_SOURCE_RESOURCE_LEASE.with(|slot| slot.borrow().clone())
}
impl SourceResourceArena {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(ArenaState {
                next_slot: 0,
                slots: HashMap::new(),
                shared_interop_aliases: HashMap::new(),
                retirement_requested: false,
                retained_roots: 0,
                finalizing: false,
                retired: false,
                finalizer: None,
                finalization_handler: None,
            })),
        }
    }

    pub fn new_with_finalizer(
        finalizer: SourceResourceFinalizer,
        finalization_handler: SourceResourceFinalizationHandler,
    ) -> Self {
        let arena = Self::new();
        let mut state = arena
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.finalizer = Some(finalizer);
        state.finalization_handler = Some(finalization_handler);
        drop(state);
        arena
    }

    pub fn set_finalizer(
        &self,
        finalizer: SourceResourceFinalizer,
        finalization_handler: SourceResourceFinalizationHandler,
    ) -> Result<(), String> {
        let mut state = self.lock()?;
        if state.retired || state.finalizing || state.retirement_requested {
            return Err("Source resource arena retirement has already started".to_string());
        }
        if state.finalizer.is_some() || state.finalization_handler.is_some() {
            return Err("Source resource arena already has an origin finalizer".to_string());
        }
        state.finalizer = Some(finalizer);
        state.finalization_handler = Some(finalization_handler);
        Ok(())
    }

    fn validate_cleanup_authority(
        &self,
        state: &ArenaState,
        inner: &Arc<SourceResourceLeaseInner>,
    ) -> Result<(), String> {
        if !inner.finalization_root || !Arc::ptr_eq(&inner.arena.state, &self.state) {
            return Err("Source cleanup lease is not the active finalizer authority".to_string());
        }
        let phase_is_active = inner
            .finalization_phase
            .as_ref()
            .is_some_and(|phase| phase.load(Ordering::Acquire));
        let active_lease_matches = ACTIVE_SOURCE_RESOURCE_LEASE.with(|slot| {
            slot.borrow()
                .as_ref()
                .is_some_and(|active| Arc::ptr_eq(&active.inner, inner))
        });
        if !phase_is_active || !active_lease_matches {
            return Err("Source finalizer authority is outside its live finalization phase".to_string());
        }
        if !state.retirement_requested
            || !state.finalizing
            || state.retired
            || state.retained_roots != 1
        {
            return Err("Source cleanup lease is no longer active".to_string());
        }
        Ok(())
    }

    fn validate_cleanup_lease(&self, lease: &SourceResourceLease) -> Result<(), String> {
        let state = self.lock()?;
        self.validate_cleanup_authority(&state, &lease.inner)
    }

    fn validate_deferred_execution_authority(
        &self,
        state: &ArenaState,
        lease: Option<&SourceResourceLease>,
    ) -> Result<bool, String> {
        if state.retired || state.finalizing {
            return Err("Source resource arena is finalizing or retired".to_string());
        }
        if !state.retirement_requested {
            return Ok(false);
        }
        let lease = lease.ok_or_else(|| {
            "Source resource arena retirement requires an active normal callback lease".to_string()
        })?;
        if lease.inner.finalization_root {
            return Err("Finalizer lease cannot authorize ordinary resource creation".to_string());
        }
        if !Arc::ptr_eq(&lease.inner.arena.state, &self.state) {
            return Err("Active Source callback lease belongs to a different arena".to_string());
        }
        if state.retained_roots == 0 {
            return Err("Source resource arena has no live callback roots".to_string());
        }
        Ok(true)
    }

    fn validate_active_execution_authority(&self) -> Result<bool, String> {
        let lease = active_source_resource_lease_snapshot();
        let state = self.lock()?;
        self.validate_deferred_execution_authority(&state, lease.as_ref())
    }

    fn retain_callback_lease(&self) -> Result<SourceResourceLease, String> {
        let lease = active_source_resource_lease_snapshot();
        let state = self.lock()?;
        if state.finalizing {
            let lease = lease.as_ref().ok_or_else(|| {
                "Source finalizer operation requires its active counted lease".to_string()
            })?;
            self.validate_cleanup_authority(&state, &lease.inner)?;
            return Ok(lease.clone());
        }
        let deferred = self.validate_deferred_execution_authority(&state, lease.as_ref())?;
        if deferred {
            return Ok(lease.expect("validated deferred execution lease is present"));
        }
        drop(state);
        self.retain_root()
    }

    /// Retain one explicit physical-root lease. Existing arena clones do not
    /// count as roots and cannot keep a pending retirement alive.
    fn retain_root(&self) -> Result<SourceResourceLease, String> {
        let mut state = self.lock()?;
        if state.retired || state.finalizing || state.retirement_requested {
            return Err("Source resource arena is retired, finalizing, or retiring".to_string());
        }
        if Self::has_channel_receiver(&state)
            && (state.finalizer.is_none() || state.finalization_handler.is_none())
        {
            return Err(
                "Source channel receiver requires an origin-owner finalizer and completion handler"
                    .to_string(),
            );
        }
        state.retained_roots = state
            .retained_roots
            .checked_add(1)
            .ok_or_else(|| "Source resource retained-root count exhausted".to_string())?;
        Ok(SourceResourceLease {
            inner: Arc::new(SourceResourceLeaseInner {
                arena: self.clone(),
                finalization_root: false,
                finalization_phase: None,
            }),
        })
    }

    /// The in-flight finalizer is itself a real retained root. This lease is
    /// the only new lease admitted after retirement has entered finalization.
    fn retain_finalization_root(&self) -> Result<SourceResourceLease, String> {
        let mut state = self.lock()?;
        if !state.retirement_requested || !state.finalizing || state.retired {
            return Err("Source resource finalization capability is not active".to_string());
        }
        if state.finalizer.is_none() {
            return Err("Source resource arena has no origin-owner finalizer".to_string());
        }
        state.retained_roots = state
            .retained_roots
            .checked_add(1)
            .ok_or_else(|| "Source resource retained-root count exhausted".to_string())?;
        Ok(SourceResourceLease {
            inner: Arc::new(SourceResourceLeaseInner {
                arena: self.clone(),
                finalization_root: true,
                finalization_phase: Some(Arc::new(AtomicBool::new(false))),
            }),
        })
    }

    /// Request logical retirement. Physical owner clearing is deferred while
    /// explicit callback/task roots remain live.
    pub fn retire(&self) -> Result<Vec<SourceExecutionCompletion>, SourceResourceRetireError> {
        route_retire_result(self.retire_inner())
    }

    fn retire_inner(&self) -> Result<Vec<SourceExecutionCompletion>, SourceResourceRetireError> {
        let Some((receivers, finalizer)) = self.begin_retirement(true)? else {
            return Ok(Vec::new());
        };
        if receivers.is_empty() {
            return self.complete_retirement(Vec::new());
        }
        let Some(finalizer) = finalizer else {
            self.leave_finalizing();
            return Err(self.retirement_error(
                "Source resource arena has no origin-owner finalizer".to_string(),
                Vec::new(),
            ));
        };
        let completions = match self.retire_receivers(receivers, |value| {
            let lease = match self.retain_finalization_root() {
                Ok(lease) => lease,
                Err(error) => return Err((error, vec![value], Vec::new())),
            };
            finalizer(SourceResourceFinalization {
                arena: self.clone(),
                values: vec![value],
                lease,
            })
            .map_err(|error| (error.error, error.values, error.completions))
        }) {
            Ok(completions) => completions,
            Err((error, values, completions)) => {
                self.leave_finalizing();
                return Err(self.retryable_retirement_error_with_completions(error, values, completions));
            }
        };
        self.complete_retirement(completions)
    }

    /// Retire while handing queued Source values to the supplied origin
    /// cleanup. Receivers are finalized one at a time, with every remaining
    /// capability live while that receiver's values are cleaned.
    pub fn retire_with_cleanup<F>(
        &self,
        cleanup: F,
    ) -> Result<Vec<SourceExecutionCompletion>, SourceResourceRetireError>
    where
        F: FnMut(MirRuntimeValue) -> Result<(), SourceResourceCleanupError>,
    {
        route_retire_result(self.retire_with_cleanup_inner(cleanup))
    }

    fn retire_with_cleanup_inner<F>(
        &self,
        cleanup: F,
    ) -> Result<Vec<SourceExecutionCompletion>, SourceResourceRetireError>
    where
        F: FnMut(MirRuntimeValue) -> Result<(), SourceResourceCleanupError>,
    {
        let Some((receivers, _finalizer)) = self.begin_retirement(false)? else {
            return Ok(Vec::new());
        };
        let mut cleanup = cleanup;
        let completions = match self.retire_receivers(receivers, |value| {
            cleanup(value)
                .map(|()| Vec::new())
                .map_err(|error| {
                    let mut values = Vec::new();
                    if let Some(value) = error.remaining {
                        values.push(value);
                    }
                    (error.error, values, Vec::new())
                })
        }) {
            Ok(completions) => completions,
            Err((error, values, completions)) => {
                self.leave_finalizing();
                return Err(self.retryable_retirement_error_with_completions(error, values, completions));
            }
        };
        self.complete_retirement(completions)
    }

    /// Retry Source cleanup using the exact retryable retirement receipt. This
    /// preserves its pending carriers and unowned completion receipts.
    pub fn retry_retire(
        &self,
        error: SourceResourceRetireError,
    ) -> Result<Vec<SourceExecutionCompletion>, SourceResourceRetireError> {
        route_retire_result(self.retry_retire_inner(error))
    }

    fn retry_retire_inner(
        &self,
        error: SourceResourceRetireError,
    ) -> Result<Vec<SourceExecutionCompletion>, SourceResourceRetireError> {
        let (values, mut completions, receivers, finalizer) = self.begin_retry(error)?;
        let Some(finalizer) = finalizer else {
            if values.is_empty() && receivers.is_empty() {
                return self.complete_retirement(completions);
            }
            self.leave_finalizing();
            return Err(self.retryable_retirement_error_with_completions(
                "Source resource arena has no origin-owner finalizer".to_string(),
                values,
                completions,
            ));
        };
        let mut process = |value| {
            let lease = match self.retain_finalization_root() {
                Ok(lease) => lease,
                Err(error) => return Err((error, vec![value], Vec::new())),
            };
            finalizer(SourceResourceFinalization {
                arena: self.clone(),
                values: vec![value],
                lease,
            })
            .map_err(|error| (error.error, error.values, error.completions))
        };
        let _activation = activate_source_resource_arena(self);
        match Self::process_values(values, &mut process) {
            Ok(processed) => completions.extend(processed),
            Err((error, values, processed)) => {
                completions.extend(processed);
                self.leave_finalizing();
                return Err(self.retryable_retirement_error_with_completions(error, values, completions));
            }
        }
        match self.retire_receivers_with(receivers, &mut process) {
            Ok(processed) => completions.extend(processed),
            Err((error, values, processed)) => {
                completions.extend(processed);
                self.leave_finalizing();
                return Err(self.retryable_retirement_error_with_completions(error, values, completions));
            }
        }
        self.complete_retirement(completions)
    }

    pub fn retry_retire_with_cleanup<F>(
        &self,
        error: SourceResourceRetireError,
        cleanup: F,
    ) -> Result<Vec<SourceExecutionCompletion>, SourceResourceRetireError>
    where
        F: FnMut(MirRuntimeValue) -> Result<(), SourceResourceCleanupError>,
    {
        route_retire_result(self.retry_retire_with_cleanup_inner(error, cleanup))
    }

    fn retry_retire_with_cleanup_inner<F>(
        &self,
        error: SourceResourceRetireError,
        cleanup: F,
    ) -> Result<Vec<SourceExecutionCompletion>, SourceResourceRetireError>
    where
        F: FnMut(MirRuntimeValue) -> Result<(), SourceResourceCleanupError>,
    {
        let (values, mut completions, receivers, _finalizer) = self.begin_retry(error)?;
        let mut cleanup = cleanup;
        let mut process = |value| {
            cleanup(value)
                .map(|()| Vec::new())
                .map_err(|error| {
                    let mut values = Vec::new();
                    if let Some(value) = error.remaining {
                        values.push(value);
                    }
                    (error.error, values, Vec::new())
                })
        };
        let _activation = activate_source_resource_arena(self);
        match Self::process_values(values, &mut process) {
            Ok(processed) => completions.extend(processed),
            Err((error, values, processed)) => {
                completions.extend(processed);
                self.leave_finalizing();
                return Err(self.retryable_retirement_error_with_completions(error, values, completions));
            }
        }
        match self.retire_receivers_with(receivers, &mut process) {
            Ok(processed) => completions.extend(processed),
            Err((error, values, processed)) => {
                completions.extend(processed);
                self.leave_finalizing();
                return Err(self.retryable_retirement_error_with_completions(error, values, completions));
            }
        }
        self.complete_retirement(completions)
    }

    fn collect_channel_receivers(state: &ArenaState) -> Vec<SourceResourceHandle> {
        let mut receivers = state
            .slots
            .iter()
            .filter_map(|(slot_id, slot)| {
                (slot.kind == SourceResourceKind::ChannelReceiver).then(|| {
                    let raw_bits =
                        (u64::from(slot.generation) << 32) | u64::from(*slot_id);
                    SourceResourceHandle {
                        handle: slot.handle,
                        raw: i64::from_ne_bytes(raw_bits.to_ne_bytes()),
                        kind: slot.kind.clone(),
                        generation: slot.generation,
                    }
                })
            })
            .collect::<Vec<_>>();
        receivers.sort_by_key(|receiver| receiver.raw as u32);
        receivers
    }

    fn begin_retirement(
        &self,
        defer_for_roots: bool,
    ) -> Result<Option<(Vec<SourceResourceHandle>, Option<SourceResourceFinalizer>)>, SourceResourceRetireError>
    {
        let mut state = self
            .lock()
            .map_err(|error| self.retirement_error(error, Vec::new()))?;
        if state.retired {
            return Ok(None);
        }
        if state.finalizing {
            return Err(self.retirement_error(
                "Source resource arena finalization is already in progress".to_string(),
                Vec::new(),
            ));
        }
        let receivers = Self::collect_channel_receivers(&state);
        let finalizer = state.finalizer.clone();
        if !receivers.is_empty()
            && (finalizer.is_none() || state.finalization_handler.is_none())
        {
            return Err(self.retirement_error(
                "Source channel receiver requires an origin-owner finalizer and completion handler"
                    .to_string(),
                Vec::new(),
            ));
        }
        if state.finalization_handler.is_none() && !state.shared_interop_aliases.is_empty() {
            return Err(self.retirement_error(
                "Source Shared aliases must be released before retirement without an origin outcome handler"
                    .to_string(),
                Vec::new(),
            ));
        }
        state.retirement_requested = true;
        if state.retained_roots != 0 {
            if defer_for_roots {
                return Ok(None);
            }
            return Err(self.retirement_error(
                "Source resource arena still has retained roots".to_string(),
                Vec::new(),
            ));
        }
        state.finalizing = true;
        Ok(Some((receivers, finalizer)))
    }

    fn begin_retry(
        &self,
        error: SourceResourceRetireError,
    ) -> Result<
        (
            Vec<MirRuntimeValue>,
            Vec<SourceExecutionCompletion>,
            Vec<SourceResourceHandle>,
            Option<SourceResourceFinalizer>,
        ),
        SourceResourceRetireError,
    > {
        let SourceResourceRetireError {
            error: previous_error,
            arena,
            values,
            completions,
            retryable,
        } = error;
        if !retryable || !Arc::ptr_eq(&arena.state, &self.state) {
            return Err(SourceResourceRetireError {
                error: previous_error,
                arena,
                values,
                completions,
                retryable,
            });
        }
        drop(arena);
        let mut state = match self.lock() {
            Ok(state) => state,
            Err(error) => {
                return Err(self.retryable_retirement_error_with_completions(
                    error,
                    values,
                    completions,
                ));
            }
        };
        if state.retired || !state.retirement_requested {
            return Err(self.retryable_retirement_error_with_completions(
                "Source resource arena has no pending retirement to retry".to_string(),
                values,
                completions,
            ));
        }
        if state.finalizing || state.retained_roots != 0 {
            return Err(self.retryable_retirement_error_with_completions(
                "Source resource arena is still active".to_string(),
                values,
                completions,
            ));
        }
        let receivers = Self::collect_channel_receivers(&state);
        let finalizer = state.finalizer.clone();
        if !receivers.is_empty()
            && (finalizer.is_none() || state.finalization_handler.is_none())
        {
            return Err(self.retryable_retirement_error_with_completions(
                "Source channel receiver requires an origin-owner finalizer and completion handler"
                    .to_string(),
                values,
                completions,
            ));
        }
        state.finalizing = true;
        Ok((values, completions, receivers, finalizer))
    }
    fn complete_retirement(
        &self,
        completions: Vec<SourceExecutionCompletion>,
    ) -> Result<Vec<SourceExecutionCompletion>, SourceResourceRetireError> {
        let mut state = match self.lock() {
            Ok(state) => state,
            Err(error) => {
                return Err(self.retryable_retirement_error_with_completions(
                    error,
                    Vec::new(),
                    completions,
                ));
            }
        };
        if state.retired {
            state.finalizing = false;
            return Ok(completions);
        }
        if state.retained_roots != 0 {
            state.finalizing = false;
            return Err(self.retryable_retirement_error_with_completions(
                "Source resource finalization returned with retained roots".to_string(),
                Vec::new(),
                completions,
            ));
        }
        if !state.shared_interop_aliases.is_empty() {
            state.finalizing = false;
            return Err(self.retryable_retirement_error_with_completions(
                "Source resource finalization returned with pending Shared owner aliases"
                    .to_string(),
                Vec::new(),
                completions,
            ));
        }
        let slots = std::mem::take(&mut state.slots);
        state.retired = true;
        state.finalizing = false;
        drop(state);
        drop(slots);
        Ok(completions)
    }

    fn leave_finalizing(&self) {
        if let Ok(mut state) = self.lock() {
            state.finalizing = false;
        }
    }

    fn retirement_error(
        &self,
        error: String,
        values: Vec<MirRuntimeValue>,
    ) -> SourceResourceRetireError {
        self.retirement_error_with_completions(error, values, Vec::new())
    }

    fn retirement_error_with_completions(
        &self,
        error: String,
        values: Vec<MirRuntimeValue>,
        completions: Vec<SourceExecutionCompletion>,
    ) -> SourceResourceRetireError {
        SourceResourceRetireError {
            error,
            arena: self.clone(),
            values,
            completions,
            retryable: false,
        }
    }

    fn retryable_retirement_error_with_completions(
        &self,
        error: String,
        values: Vec<MirRuntimeValue>,
        completions: Vec<SourceExecutionCompletion>,
    ) -> SourceResourceRetireError {
        SourceResourceRetireError {
            error,
            arena: self.clone(),
            values,
            completions,
            retryable: true,
        }
    }

    fn retire_receivers<F>(
        &self,
        receivers: Vec<SourceResourceHandle>,
        process: F,
    ) -> Result<Vec<SourceExecutionCompletion>, (String, Vec<MirRuntimeValue>, Vec<SourceExecutionCompletion>)>
    where
        F: FnMut(
            MirRuntimeValue,
        ) -> Result<Vec<SourceExecutionCompletion>, (String, Vec<MirRuntimeValue>, Vec<SourceExecutionCompletion>)>,
    {
        let mut process = process;
        self.retire_receivers_with(receivers, &mut process)
    }

    fn retire_receivers_with<F>(
        &self,
        receivers: Vec<SourceResourceHandle>,
        process: &mut F,
    ) -> Result<Vec<SourceExecutionCompletion>, (String, Vec<MirRuntimeValue>, Vec<SourceExecutionCompletion>)>
    where
        F: FnMut(
            MirRuntimeValue,
        ) -> Result<Vec<SourceExecutionCompletion>, (String, Vec<MirRuntimeValue>, Vec<SourceExecutionCompletion>)>,
    {
        let _activation = activate_source_resource_arena(self);
        let mut completions = Vec::new();
        for receiver in receivers {
            loop {
                match self.poll_retirement_receiver(&receiver) {
                    Ok(Some(Some(value))) => match Self::process_values(vec![value], process) {
                        Ok(done) => completions.extend(done),
                        Err((error, values, done)) => {
                            completions.extend(done);
                            return Err((error, values, completions));
                        }
                    },
                    Ok(Some(None)) | Ok(None) => break,
                    Err(error) => return Err((error, Vec::new(), completions)),
                }
            }
            let drained = match self.close_retirement_receiver(&receiver) {
                Ok(Some(values)) => values,
                Ok(None) => Vec::new(),
                Err(error) => return Err((error, Vec::new(), completions)),
            };
            match Self::process_values(drained, process) {
                Ok(done) => completions.extend(done),
                Err((error, values, done)) => {
                    completions.extend(done);
                    return Err((error, values, completions));
                }
            }
            if let Err(error) = self.remove_retirement_receiver(&receiver) {
                return Err((error, Vec::new(), completions));
            }
        }
        Ok(completions)
    }

    fn process_values<F>(
        mut values: Vec<MirRuntimeValue>,
        process: &mut F,
    ) -> Result<Vec<SourceExecutionCompletion>, (String, Vec<MirRuntimeValue>, Vec<SourceExecutionCompletion>)>
    where
        F: FnMut(
            MirRuntimeValue,
        ) -> Result<Vec<SourceExecutionCompletion>, (String, Vec<MirRuntimeValue>, Vec<SourceExecutionCompletion>)>,
    {
        let mut unprocessed = values.drain(..);
        let mut completions = Vec::new();
        while let Some(value) = unprocessed.next() {
            match process(value) {
                Ok(done) => completions.extend(done),
                Err((error, mut untouched, done)) => {
                    completions.extend(done);
                    untouched.extend(unprocessed);
                    return Err((error, untouched, completions));
                }
            }
        }
        Ok(completions)
    }

    fn poll_retirement_receiver(
        &self,
        capability: &SourceResourceHandle,
    ) -> Result<Option<Option<MirRuntimeValue>>, String> {
        let owner = {
            let state = self.lock()?;
            let (slot_id, generation) = Self::decode_raw(capability.raw)?;
            let Some(slot) = state.slots.get(&slot_id) else {
                return Ok(None);
            };
            if slot.generation != generation
                || slot.handle != capability.handle
                || slot.kind != SourceResourceKind::ChannelReceiver
            {
                return Err("Source receiver capability changed during retirement".to_string());
            }
            match &slot.entry {
                SlotEntry::Backend(owner) => Arc::clone(owner),
                SlotEntry::Cursor(_) => {
                    return Err("Source receiver capability payload mismatch".to_string())
                }
            }
        };
        let result = {
            let owner = owner
                .lock()
                .map_err(|_| "Source resource owner lock is poisoned".to_string())?;
            let BackendOwner::ChannelReceiver(receiver) = &*owner else {
                return Err("Source receiver capability payload mismatch".to_string());
            };
            receiver.try_receive()
        };
        Ok(Some(result))
    }

    fn close_retirement_receiver(
        &self,
        capability: &SourceResourceHandle,
    ) -> Result<Option<Vec<MirRuntimeValue>>, String> {
        let owner = {
            let state = self.lock()?;
            let (slot_id, generation) = Self::decode_raw(capability.raw)?;
            let Some(slot) = state.slots.get(&slot_id) else {
                return Ok(None);
            };
            if slot.generation != generation
                || slot.handle != capability.handle
                || slot.kind != SourceResourceKind::ChannelReceiver
            {
                return Err("Source receiver capability changed during retirement".to_string());
            }
            match &slot.entry {
                SlotEntry::Backend(owner) => Arc::clone(owner),
                SlotEntry::Cursor(_) => {
                    return Err("Source receiver capability payload mismatch".to_string())
                }
            }
        };
        let values = {
            let owner = owner
                .lock()
                .map_err(|_| "Source resource owner lock is poisoned".to_string())?;
            let BackendOwner::ChannelReceiver(receiver) = &*owner else {
                return Err("Source receiver capability payload mismatch".to_string());
            };
            receiver.close_receiver_and_drain()
        };
        Ok(Some(values))
    }

    fn remove_retirement_receiver(
        &self,
        capability: &SourceResourceHandle,
    ) -> Result<(), String> {
        let removed = {
            let mut state = self.lock()?;
            let (slot_id, generation) = Self::decode_raw(capability.raw)?;
            let Some(slot) = state.slots.get(&slot_id) else {
                return Ok(());
            };
            if slot.generation != generation
                || slot.handle != capability.handle
                || slot.kind != SourceResourceKind::ChannelReceiver
            {
                return Err("Source receiver capability changed during retirement".to_string());
            }
            let removed = state.slots.remove(&slot_id);
            removed
        };
        drop(removed);
        Ok(())
    }

    pub fn is_retired(&self) -> Result<bool, String> {
        Ok(self.lock()?.retired)
    }

    pub fn is_retirement_requested(&self) -> Result<bool, String> {
        Ok(self.lock()?.retirement_requested)
    }

    pub fn retained_root_count(&self) -> Result<usize, String> {
        Ok(self.lock()?.retained_roots)
    }

    fn release_root(&self) {
        let (should_retire, finalization_handler) = {
            let Ok(mut state) = self.lock() else {
                return;
            };
            if state.retained_roots == 0 {
                return;
            }
            state.retained_roots -= 1;
            (
                state.retained_roots == 0
                    && state.retirement_requested
                    && !state.retired
                    && !state.finalizing,
                state.finalization_handler.clone(),
            )
        };
        if !should_retire {
            return;
        }
        let Some(finalization_handler) = finalization_handler else {
            self.retire_without_source_work();
            return;
        };
        let outcome = match self.retire() {
            Ok(completions) if !completions.is_empty() => {
                Some(SourceResourceFinalizationOutcome::Retired { completions })
            }
            Ok(_) => None,
            Err(error) => Some(SourceResourceFinalizationOutcome::Failed(error)),
        };
        if let Some(outcome) = outcome {
            finalization_handler(outcome);
        }
    }

    fn retire_without_source_work(&self) {
        let slots = {
            let Ok(mut state) = self.lock() else {
                return;
            };
            if state.retired
                || !state.retirement_requested
                || state.retained_roots != 0
                || state.finalizing
                || state.finalization_handler.is_some()
                || !state.shared_interop_aliases.is_empty()
                || Self::has_channel_receiver(&state)
            {
                return;
            }
            state.retired = true;
            std::mem::take(&mut state.slots)
        };
        drop(slots);
    }


    fn lock(&self) -> Result<MutexGuard<'_, ArenaState>, String> {
        self.state
            .lock()
            .map_err(|_| "Source resource arena lock is poisoned".to_string())
    }
    fn has_channel_receiver(state: &ArenaState) -> bool {
        state.slots.values().any(|slot| match &slot.entry {
            SlotEntry::Cursor(_) => false,
            SlotEntry::Backend(owner) => {
                let owner = owner
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                matches!(&*owner, BackendOwner::ChannelReceiver(_))
            }
        })
    }


    fn reserve_slot_handle(
        state: &mut ArenaState,
        handle: MirHandleId,
        kind: SourceResourceKind,
        allow_deferred_retirement: bool,
    ) -> Result<SourceResourceHandle, String> {
        if state.retired
            || state.finalizing
            || (state.retirement_requested && !allow_deferred_retirement)
        {
            return Err("Source resource arena is retired, finalizing, or retiring".to_string());
        }
        state.next_slot = state
            .next_slot
            .checked_add(1)
            .ok_or_else(|| "Source resource slot space exhausted".to_string())?;
        let generation = Self::allocate_generation()?;
        let raw_bits = (u64::from(generation) << 32) | u64::from(state.next_slot);
        Ok(SourceResourceHandle {
            handle,
            raw: i64::from_ne_bytes(raw_bits.to_ne_bytes()),
            kind,
            generation,
        })
    }

    fn insert_reserved_slot(
        state: &mut ArenaState,
        capability: SourceResourceHandle,
        entry: SlotEntry,
    ) {
        state.slots.insert(
            capability.raw as u32,
            Slot {
                handle: capability.handle,
                kind: capability.kind,
                generation: capability.generation,
                entry,
            },
        );
    }

    fn allocate_slot(
        state: &mut ArenaState,
        handle: MirHandleId,
        kind: SourceResourceKind,
        entry: SlotEntry,
        allow_deferred_retirement: bool,
    ) -> Result<SourceResourceHandle, String> {
        let capability =
            Self::reserve_slot_handle(state, handle, kind, allow_deferred_retirement)?;
        Self::insert_reserved_slot(state, capability.clone(), entry);
        Ok(capability)
    }

    fn allocate_cleanup_slot(
        state: &mut ArenaState,
        handle: MirHandleId,
        kind: SourceResourceKind,
        entry: SlotEntry,
    ) -> Result<SourceResourceHandle, String> {
        if state.retired || !state.finalizing || !state.retirement_requested {
            return Err("Source cleanup lease is no longer active".to_string());
        }
        state.next_slot = state
            .next_slot
            .checked_add(1)
            .ok_or_else(|| "Source resource slot space exhausted".to_string())?;
        let generation = Self::allocate_generation()?;
        let raw_bits = (u64::from(generation) << 32) | u64::from(state.next_slot);
        let capability = SourceResourceHandle {
            handle,
            raw: i64::from_ne_bytes(raw_bits.to_ne_bytes()),
            kind,
            generation,
        };
        Self::insert_reserved_slot(state, capability.clone(), entry);
        Ok(capability)
    }

    fn decode_raw(raw: i64) -> Result<(u32, u32), String> {
        let bits = u64::from_ne_bytes(raw.to_ne_bytes());
        let slot_id = bits as u32;
        let generation = (bits >> 32) as u32;
        if slot_id == 0 || generation == 0 {
            return Err("invalid or stale Source resource capability".to_string());
        }
        Ok((slot_id, generation))
    }
    fn allocate_generation() -> Result<u32, String> {
        let mut current = NEXT_CAPABILITY_GENERATION.load(Ordering::Relaxed);
        loop {
            let next = current
                .checked_add(1)
                .ok_or_else(|| "Source resource generation space exhausted".to_string())?;
            match NEXT_CAPABILITY_GENERATION.compare_exchange_weak(
                current,
                next,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(next),
                Err(observed) => current = observed,
            }
        }
    }

    fn register_backend(
        &self,
        handle: MirHandleId,
        owner: BackendOwner,
    ) -> Result<SourceResourceHandle, String> {
        let kind = owner.kind();
        self.register_backend_arc(handle, kind, Arc::new(Mutex::new(owner)))
    }


    fn register_backend_arc(
        &self,
        handle: MirHandleId,
        kind: SourceResourceKind,
        owner: Arc<Mutex<BackendOwner>>,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend_arc_owned(handle, kind, owner)
            .map_err(|(error, _owner)| error)
    }

    fn register_backend_arc_owned(
        &self,
        handle: MirHandleId,
        kind: SourceResourceKind,
        owner: Arc<Mutex<BackendOwner>>,
    ) -> Result<SourceResourceHandle, (String, Arc<Mutex<BackendOwner>>)> {
        let active_lease = active_source_resource_lease_snapshot();
        let result = {
            let mut state = self
                .lock()
                .map_err(|error| (error, Arc::clone(&owner)))?;
            let allow_deferred_retirement = self
                .validate_deferred_execution_authority(&state, active_lease.as_ref())
                .map_err(|error| (error, Arc::clone(&owner)))?;
            if matches!(&kind, SourceResourceKind::ChannelReceiver)
                && (state.finalizer.is_none() || state.finalization_handler.is_none())
            {
                Err((
                    "Source channel receiver requires an origin-owner finalizer and completion handler"
                        .to_string(),
                    Arc::clone(&owner),
                ))
            } else {
                Self::allocate_slot(
                    &mut state,
                    handle,
                    kind,
                    SlotEntry::Backend(Arc::clone(&owner)),
                    allow_deferred_retirement,
                )
                .map_err(|error| (error, Arc::clone(&owner)))
            }
        };
        match result {
            Ok(capability) => {
                drop(owner);
                Ok(capability)
            }
            Err((error, _)) => Err((error, owner)),
        }
    }

    fn register_cleanup_backend_arc_owned(
        &self,
        cleanup: &SourceResourceCleanupLease,
        handle: MirHandleId,
        kind: SourceResourceKind,
        owner: Arc<Mutex<BackendOwner>>,
    ) -> Result<SourceResourceHandle, (String, Arc<Mutex<BackendOwner>>)> {
        let result = {
            let mut state = self
                .lock()
                .map_err(|error| (error, Arc::clone(&owner)))?;
            if let Err(error) = self.validate_cleanup_authority(&state, &cleanup.inner) {
                Err((error, Arc::clone(&owner)))
            } else {
                Self::allocate_cleanup_slot(
                    &mut state,
                    handle,
                    kind,
                    SlotEntry::Backend(Arc::clone(&owner)),
                )
                .map_err(|error| (error, Arc::clone(&owner)))
            }
        };
        match result {
            Ok(capability) => {
                drop(owner);
                Ok(capability)
            }
            Err((error, _)) => Err((error, owner)),
        }
    }

    fn register_cleanup_native_binding_root(
        &self,
        cleanup: &SourceResourceCleanupLease,
        binding: SourceNativeBinding,
    ) -> Result<SourceResourceHandle, String> {
        self.register_cleanup_backend_arc_owned(
            cleanup,
            native_binding_handle_id(),
            SourceResourceKind::NativeBinding,
            Arc::new(Mutex::new(BackendOwner::NativeBinding(binding))),
        )
        .map_err(|(error, _owner)| error)
    }

    /// Register a canonical file reader owner and receive its scoped
    /// capability.
    pub fn register_file(
        &self,
        handle: MirHandleId,
        reader: SourceFileReader,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(handle, BackendOwner::File(reader))
    }

    /// Adopt a typed reader from the legacy JIT table without retaining the
    /// integer slot as a second owner.
    pub fn register_file_handle(
        &self,
        handle: MirHandleId,
        reader_handle: i64,
    ) -> Result<SourceResourceHandle, String> {
        self.register_file(handle, SourceFileReader::from_handle(reader_handle)?)
    }

    /// Open and register a file through the canonical FileStream leaf.
    pub fn open_file(
        &self,
        handle: MirHandleId,
        path: &str,
    ) -> Result<SourceResourceHandle, MirNativeCursorError> {
        self.register_file(handle, SourceFileReader::open(path)?)
            .map_err(MirNativeCursorError::internal)
    }

    /// Register a canonical file writer owner.  Writers are release-only
    /// leases and intentionally cannot be projected as loop producers.
    pub fn register_file_writer(
        &self,
        handle: MirHandleId,
        writer: SourceFileWriter,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(handle, BackendOwner::FileWriter(writer))
    }

    /// Adopt a typed writer from the legacy JIT table.
    pub fn register_file_writer_handle(
        &self,
        handle: MirHandleId,
        writer_handle: i64,
    ) -> Result<SourceResourceHandle, String> {
        self.register_file_writer(handle, SourceFileWriter::from_handle(writer_handle)?)
    }

    pub fn open_file_writer(
        &self,
        handle: MirHandleId,
        path: &str,
    ) -> Result<SourceResourceHandle, MirNativeCursorError> {
        self.register_file_writer(handle, SourceFileWriter::create(path)?)
            .map_err(MirNativeCursorError::internal)
    }

    pub fn append_file_writer(
        &self,
        handle: MirHandleId,
        path: &str,
    ) -> Result<SourceResourceHandle, MirNativeCursorError> {
        self.register_file_writer(handle, SourceFileWriter::append(path)?)
            .map_err(MirNativeCursorError::internal)
    }

    /// Register the typed canonical stdin line producer.
    pub fn register_stdin(
        &self,
        handle: MirHandleId,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(
            handle,
            BackendOwner::Stdin(crate::enc_stream::source_stdin_reader()),
        )
    }

    /// Register an already-adopted process stdout/stderr reader.
    pub fn register_process_stream(
        &self,
        handle: MirHandleId,
        reader: SourceProcessStream,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(handle, BackendOwner::Process(reader))
    }
    /// Adopt a process child output reader from the canonical Process owner
    /// table, then issue the Source arena capability for that stream.
    pub fn register_process_stream_handle(
        &self,
        handle: MirHandleId,
        child_raw: i64,
        stdout: bool,
    ) -> Result<SourceResourceHandle, String> {
        let stream = if stdout {
            crate::ProcessPrelude::process_prelude::SourceProcessStreamKind::Stdout
        } else {
            crate::ProcessPrelude::process_prelude::SourceProcessStreamKind::Stderr
        };
        let reader = crate::Process::source_take_process_stream(child_raw, stream)?;
        self.register_process_stream(handle, SourceProcessStream(reader))
    }


    /// Register a scheduler channel receiver carrying Source runtime values.
    pub fn register_channel_receiver(
        &self,
        handle: MirHandleId,
        receiver: &jet_codegen::scheduler::JetSchedulerChannel<MirRuntimeValue>,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(
            handle,
            BackendOwner::ChannelReceiver(receiver.clone()),
        )
    }

    /// Register the matching canonical sender endpoint.
    pub fn register_channel_sender(
        &self,
        handle: MirHandleId,
        sender: jet_codegen::scheduler::JetSchedulerSender<MirRuntimeValue>,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(handle, BackendOwner::ChannelSender(sender))
    }

    /// Register an already-checked native binding root. The arena retains
    /// only the exact carrier and metadata; NativeAdapter owns dispatch and
    /// binding-set scope.
    pub fn register_native_binding(
        &self,
        handle: MirHandleId,
        binding: SourceNativeBinding,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(handle, BackendOwner::NativeBinding(binding))
    }

    /// Build and register an interface value root. Method identity is checked
    /// later by SourceInterfaces, not encoded as this value's identity.
    pub fn register_native_interface_binding(
        &self,
        handle: MirHandleId,
        object: &crate::SourceInterfaces::NativeInterfaceObject,
        execution: jet_foundation::MIR::MirExecutionIdentity,
        artifact: jet_foundation::MIR::MirArtifactId,
        receiver_type: MirType,
    ) -> Result<SourceResourceHandle, String> {
        let binding = SourceNativeBinding::interface(object, execution, artifact, receiver_type)?;
        self.register_native_binding(handle, binding)
    }

    /// Build and register a checked native Fn value root.
    pub fn register_native_callable_binding(
        &self,
        handle: MirHandleId,
        object: &crate::SourceInterfaces::NativeInterfaceObject,
        identity: crate::SourceInterfaces::NativeCallableIdentity,
    ) -> Result<SourceResourceHandle, String> {
        let binding = SourceNativeBinding::callable(object, identity)?;
        self.register_native_binding(handle, binding)
    }

    pub fn register_native_binding_root(
        &self,
        binding: SourceNativeBinding,
    ) -> Result<SourceResourceHandle, String> {
        self.register_native_binding(native_binding_handle_id(), binding)
    }

    pub fn register_native_interface_binding_root(
        &self,
        object: &crate::SourceInterfaces::NativeInterfaceObject,
        execution: jet_foundation::MIR::MirExecutionIdentity,
        artifact: jet_foundation::MIR::MirArtifactId,
        receiver_type: MirType,
    ) -> Result<SourceResourceHandle, String> {
        self.register_native_interface_binding(
            native_binding_handle_id(),
            object,
            execution,
            artifact,
            receiver_type,
        )
    }

    pub fn register_native_callable_binding_root(
        &self,
        object: &crate::SourceInterfaces::NativeInterfaceObject,
        identity: crate::SourceInterfaces::NativeCallableIdentity,
    ) -> Result<SourceResourceHandle, String> {
        self.register_native_callable_binding(native_binding_handle_id(), object, identity)
    }
    fn shared_interop_root_type_identity(
        capability: &SourceResourceHandle,
    ) -> Result<u64, String> {
        if capability.handle != shared_interop_root_handle_id() {
            return Err("Source capability is not a SharedInteropRoot".to_string());
        }
        let SourceResourceKind::SharedInteropRoot { type_identity } = &capability.kind else {
            return Err("Source capability is not a SharedInteropRoot".to_string());
        };
        let (_, generation) = Self::decode_raw(capability.raw)?;
        if generation != capability.generation {
            return Err("Shared interop root generation mismatch".to_string());
        }
        Ok(*type_identity)
    }

    fn validate_shared_interop_root_locked(
        state: &ArenaState,
        capability: &SourceResourceHandle,
    ) -> Result<(), String> {
        let (slot_id, generation) = Self::decode_raw(capability.raw)?;
        if generation != capability.generation {
            return Err("Shared interop root generation mismatch".to_string());
        }
        let slot = state
            .slots
            .get(&slot_id)
            .ok_or_else(|| "stale or unknown Shared interop root capability".to_string())?;
        if slot.generation != generation
            || slot.handle != capability.handle
            || slot.kind != capability.kind
        {
            return Err("Shared interop root capability generation or kind mismatch".to_string());
        }
        Ok(())
    }

    fn ensure_no_shared_interop_aliases_locked(
        state: &ArenaState,
        raw: i64,
    ) -> Result<(), String> {
        if state
            .shared_interop_aliases
            .keys()
            .any(|(alias_raw, _)| *alias_raw == raw)
        {
            return Err("Shared interop root still has pending owner aliases".to_string());
        }
        Ok(())
    }

    /// Register one checked physical Source Shared root under a fresh arena
    /// capability, independent of Source alias indexes. A root without an
    /// attached Source alias is admissible without cleanup; an attached alias
    /// requires the origin outcome handler before this arena clones the root.
    pub fn register_shared_interop_root(
        &self,
        type_identity: u64,
        root: &crate::SourceSharedInterop::SourceSharedInterop,
    ) -> Result<SourceResourceHandle, String> {
        if root.type_id() != type_identity {
            return Err("Shared interop root checked type identity mismatch".to_string());
        }
        if root.owner_alias_token_id().is_some()
            && self.lock()?.finalization_handler.is_none()
        {
            return Err(
                "Source Shared aliases require an origin outcome handler before root registration"
                    .to_string(),
            );
        }
        self.register_backend(
            shared_interop_root_handle_id(),
            BackendOwner::SharedInteropRoot(root.clone()),
        )
    }

    /// Resolve the already-registered physical root for one Source Shared
    /// carrier. This returns its existing generation-bearing capability and
    /// never creates a second arena slot.
    pub fn resolve_shared_interop_root(
        &self,
        root: &crate::SourceSharedInterop::SourceSharedInterop,
    ) -> Result<SourceResourceHandle, String> {
        let type_identity = root.type_id();
        let state = self.lock()?;
        let mut resolved = None;
        for (slot_id, slot) in &state.slots {
            if slot.handle != shared_interop_root_handle_id()
                || slot.kind != (SourceResourceKind::SharedInteropRoot { type_identity })
            {
                continue;
            }
            let SlotEntry::Backend(owner) = &slot.entry else {
                return Err("Shared interop root capability payload mismatch".to_string());
            };
            let owner = owner
                .lock()
                .map_err(|_| "Source resource owner lock is poisoned".to_string())?;
            let BackendOwner::SharedInteropRoot(registered) = &*owner else {
                return Err("Shared interop root capability payload mismatch".to_string());
            };
            if registered.identity() != root.identity() {
                continue;
            }
            if resolved.is_some() {
                return Err("multiple Source Shared roots have the same physical identity".to_string());
            }
            let raw_bits = (u64::from(slot.generation) << 32) | u64::from(*slot_id);
            resolved = Some(SourceResourceHandle {
                handle: slot.handle,
                raw: i64::from_ne_bytes(raw_bits.to_ne_bytes()),
                kind: slot.kind.clone(),
                generation: slot.generation,
            });
        }
        resolved.ok_or_else(|| "Source Shared root is not registered in this arena".to_string())
    }

    /// Borrow the exact physical root and a counted lease alias. During a live
    /// callback this may retain a new root; during deferred retirement or its
    /// finalizer it reuses only that exact active lease.
    pub fn borrow_shared_interop_root(
        &self,
        capability: &SourceResourceHandle,
        type_identity: u64,
    ) -> Result<
        (
            crate::SourceSharedInterop::SourceSharedInterop,
            SourceResourceLease,
        ),
        MirNativeCursorError,
    > {
        if capability.handle != shared_interop_root_handle_id()
            || capability.kind != (SourceResourceKind::SharedInteropRoot { type_identity })
        {
            return Err(MirNativeCursorError::internal(
                "Shared interop root handle or checked type identity mismatch",
            ));
        }
        let (_, generation) =
            Self::decode_raw(capability.raw).map_err(MirNativeCursorError::internal)?;
        if generation != capability.generation {
            return Err(MirNativeCursorError::internal(
                "Shared interop root generation mismatch",
            ));
        }
        let lease = self
            .retain_callback_lease()
            .map_err(MirNativeCursorError::internal)?;
        let owner = self.owner_for_raw(
            capability.handle,
            capability.raw,
            &capability.kind,
        )?;
        let root = {
            let owner = owner
                .lock()
                .map_err(|_| MirNativeCursorError::internal("Shared interop root lock is poisoned"))?;
            let BackendOwner::SharedInteropRoot(root) = &*owner else {
                return Err(MirNativeCursorError::internal(
                    "Shared interop root capability payload mismatch",
                ));
            };
            if root.type_id() != type_identity {
                return Err(MirNativeCursorError::internal(
                    "Shared interop root checked type identity mismatch",
                ));
            }
            root.clone()
        };
        Ok((root, lease))
    }

    /// Release the arena's strong root reference. Borrowed roots remain usable;
    /// their separate leases keep nested Source owners live until the final
    /// native alias finishes cleanup and drops.
    pub fn release_shared_interop_root(
        &self,
        capability: &SourceResourceHandle,
    ) -> Result<SourceResourceRelease, String> {
        let _ = Self::shared_interop_root_type_identity(capability)?;
        let removed = {
            let mut state = self.lock()?;
            Self::validate_shared_interop_root_locked(&state, capability)?;
            if state
                .shared_interop_aliases
                .keys()
                .any(|(raw, _)| *raw == capability.raw)
            {
                return Err("Shared interop root still has pending owner aliases".to_string());
            }
            let (slot_id, _) = Self::decode_raw(capability.raw)?;
            state
                .slots
                .remove(&slot_id)
                .ok_or_else(|| "Shared interop root capability disappeared".to_string())?
        };
        Ok(Self::finalize_removed_slot(removed))
    }

    fn store_shared_interop_alias(
        &self,
        capability: &SourceResourceHandle,
        alias: crate::SourceSharedInterop::SourceSharedInteropOwnerAlias,
    ) -> Result<(), String> {
        let _ = Self::shared_interop_root_type_identity(capability)?;
        let token_id = alias.token_id();
        let active_lease = active_source_resource_lease_snapshot();
        let mut state = self.lock()?;
        Self::validate_shared_interop_root_locked(&state, capability)?;
        self.validate_deferred_execution_authority(&state, active_lease.as_ref())?;
        let key = (capability.raw, token_id);
        if state.shared_interop_aliases.contains_key(&key) {
            return Err("Shared interop owner alias token is already registered".to_string());
        }
        state.shared_interop_aliases.insert(key, Some(alias));
        Ok(())
    }

    /// Reserve one Source-owned logical alias for this exact physical root.
    /// The returned token ID is owner-issued; arena pins and metadata do not
    /// participate in its count. An origin outcome handler must be configured
    /// before reservation because final alias release may produce a receipt.
    pub fn retain_shared_interop_alias(
        &self,
        capability: &SourceResourceHandle,
    ) -> Result<i64, String> {
        let type_identity = Self::shared_interop_root_type_identity(capability)?;
        self.validate_active_execution_authority()?;
        let (root, lease) = self
            .borrow_shared_interop_root(capability, type_identity)
            .map_err(|error| {
                error
                    .internal_message()
                    .unwrap_or("Shared interop root borrow failed")
                    .to_string()
            })?;
        if self.lock()?.finalization_handler.is_none() {
            return Err("Source Shared aliases require an origin outcome handler".to_string());
        }
        let alias = root.retain_owner_alias()?;
        let token_id = alias.token_id();
        self.store_shared_interop_alias(capability, alias)?;
        drop(lease);
        Ok(token_id)
    }

    /// Consume one pending logical alias into a physical Shared carrier. The
    /// root clone and arena lease remain alive until the owner token is
    /// attached, so concurrent retirement cannot invalidate the transfer.
    pub fn adopt_shared_interop_alias(
        &self,
        capability: &SourceResourceHandle,
        token_id: i64,
    ) -> Result<crate::SourceSharedInterop::SourceSharedInterop, String> {
        let type_identity = Self::shared_interop_root_type_identity(capability)?;
        let (root, lease) = self
            .borrow_shared_interop_root(capability, type_identity)
            .map_err(|error| {
                error
                    .internal_message()
                    .unwrap_or("Shared interop root borrow failed")
                    .to_string()
            })?;
        let key = (capability.raw, token_id);
        let alias = {
            let active_lease = active_source_resource_lease_snapshot();
            let mut state = self.lock()?;
            Self::validate_shared_interop_root_locked(&state, capability)?;
            self.validate_deferred_execution_authority(&state, active_lease.as_ref())?;
            match state.shared_interop_aliases.get(&key) {
                Some(Some(_)) => {}
                Some(None) => {
                    return Err("Shared interop owner alias release is in progress".to_string())
                }
                None => return Err("Unknown Shared interop owner alias token".to_string()),
            }
            state
                .shared_interop_aliases
                .remove(&key)
                .and_then(|alias| alias)
                .expect("validated Shared interop owner alias remains registered")
        };
        let carrier = root.with_owner_alias_lease(alias)?;
        drop(lease);
        Ok(carrier)
    }

    /// Release a pending Source-owned logical alias. A release-in-progress
    /// marker keeps retirement and root release from overtaking the owner
    /// operation, which runs outside the arena mutex. The owner result and any
    /// one-shot physical completion are returned separately and unchanged.
    pub fn release_shared_interop_alias(
        &self,
        capability: &SourceResourceHandle,
        token_id: i64,
    ) -> Result<
        crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()>,
        String,
    > {
        let _ = Self::shared_interop_root_type_identity(capability)?;
        let key = (capability.raw, token_id);
        let alias = {
            let mut state = self.lock()?;
            Self::validate_shared_interop_root_locked(&state, capability)?;
            match state.shared_interop_aliases.get_mut(&key) {
                Some(entry) if entry.is_some() => entry.take().expect("alias entry is present"),
                Some(_) => {
                    return Err("Shared interop owner alias release is already in progress".to_string())
                }
                None => return Err("Unknown Shared interop owner alias token".to_string()),
            }
        };
        let release = SharedInteropAliasReleaseGuard { arena: self, key };
        let outcome = alias.release();
        drop(alias);
        drop(release);
        Ok(outcome)
    }

    /// Create an arena capability containing only the physical weak owner.
    /// Weak capability aliases retain the arena separately through leases.
    pub fn register_shared_interop_weak_root(
        &self,
        type_identity: u64,
        root: crate::SourceSharedInterop::SourceSharedInteropWeak,
    ) -> Result<SourceResourceHandle, String> {
        if root.type_id() != type_identity {
            return Err("Shared interop weak root checked type identity mismatch".to_string());
        }
        self.register_backend(
            shared_interop_weak_root_handle_id(),
            BackendOwner::SharedInteropWeakRoot {
                type_identity,
                root: Arc::new(root),
            },
        )
    }

    /// Downgrade an exact registered strong root without retaining it.
    pub fn downgrade_shared_interop_root(
        &self,
        capability: &SourceResourceHandle,
    ) -> Result<SourceResourceHandle, MirNativeCursorError> {
        let SourceResourceKind::SharedInteropRoot { type_identity } = &capability.kind else {
            return Err(MirNativeCursorError::internal(
                "Source capability is not a SharedInteropRoot",
            ));
        };
        let type_identity = *type_identity;
        if capability.handle != shared_interop_root_handle_id() {
            return Err(MirNativeCursorError::internal(
                "Shared interop root handle identity mismatch",
            ));
        }
        let (_, generation) =
            Self::decode_raw(capability.raw).map_err(MirNativeCursorError::internal)?;
        if generation != capability.generation {
            return Err(MirNativeCursorError::internal(
                "Shared interop root generation mismatch",
            ));
        }
        let owner = self.owner_for_raw(capability.handle, capability.raw, &capability.kind)?;
        let root = {
            let owner = owner
                .lock()
                .map_err(|_| MirNativeCursorError::internal("Shared interop root lock is poisoned"))?;
            let BackendOwner::SharedInteropRoot(root) = &*owner else {
                return Err(MirNativeCursorError::internal(
                    "Shared interop root capability payload mismatch",
                ));
            };
            if root.type_id() != type_identity {
                return Err(MirNativeCursorError::internal(
                    "Shared interop root checked type identity mismatch",
                ));
            }
            root.clone()
        };
        let weak = root.downgrade_owner().ok_or_else(|| {
            MirNativeCursorError::internal("Shared interop root has no physical weak-owner hook")
        })?;
        self.register_shared_interop_weak_root(type_identity, weak)
            .map_err(MirNativeCursorError::internal)
    }

    /// Borrow one weak owner and a counted lease alias. The weak pointer never
    /// retains the Shared value itself; deferred/finalizer scopes reuse their
    /// exact active lease instead of creating a new root.
    pub fn borrow_shared_interop_weak_root(
        &self,
        capability: &SourceResourceHandle,
        type_identity: u64,
    ) -> Result<
        (
            Arc<crate::SourceSharedInterop::SourceSharedInteropWeak>,
            SourceResourceLease,
        ),
        MirNativeCursorError,
    > {
        let SourceResourceKind::SharedInteropWeakRoot {
            type_identity: stored_type_identity,
        } = &capability.kind
        else {
            return Err(MirNativeCursorError::internal(
                "Source capability is not a SharedInteropWeakRoot",
            ));
        };
        if *stored_type_identity != type_identity {
            return Err(MirNativeCursorError::internal(
                "Shared interop weak root checked type identity mismatch",
            ));
        }
        if capability.handle != shared_interop_weak_root_handle_id() {
            return Err(MirNativeCursorError::internal(
                "Shared interop weak root handle identity mismatch",
            ));
        }
        let (_, generation) =
            Self::decode_raw(capability.raw).map_err(MirNativeCursorError::internal)?;
        if generation != capability.generation {
            return Err(MirNativeCursorError::internal(
                "Shared interop weak root generation mismatch",
            ));
        }
        let lease = self
            .retain_callback_lease()
            .map_err(MirNativeCursorError::internal)?;
        let owner = self.owner_for_raw(
            capability.handle,
            capability.raw,
            &capability.kind,
        )?;
        let weak = {
            let owner = owner.lock().map_err(|_| {
                MirNativeCursorError::internal("Shared interop weak root lock is poisoned")
            })?;
            let BackendOwner::SharedInteropWeakRoot {
                type_identity: stored_type_identity,
                root,
            } = &*owner
            else {
                return Err(MirNativeCursorError::internal(
                    "Shared interop weak root capability payload mismatch",
                ));
            };
            if *stored_type_identity != type_identity {
                return Err(MirNativeCursorError::internal(
                    "Shared interop weak root checked type identity mismatch",
                ));
            }
            Arc::clone(root)
        };
        if weak.type_id() != type_identity {
            return Err(MirNativeCursorError::internal(
                "Shared interop weak root checked type identity mismatch",
            ));
        }
        Ok((weak, lease))
    }

    /// Upgrade through the physical weak owner and register a fresh strong
    /// capability with the atomically reserved Source logical alias.
    pub fn upgrade_shared_interop_weak_root(
        &self,
        capability: &SourceResourceHandle,
    ) -> Result<
        Option<(
            crate::SourceSharedInterop::SourceSharedInterop,
            SourceResourceHandle,
            SourceResourceLease,
            i64,
        )>,
        MirNativeCursorError,
    > {
        let SourceResourceKind::SharedInteropWeakRoot { type_identity } = &capability.kind else {
            return Err(MirNativeCursorError::internal(
                "Source capability is not a SharedInteropWeakRoot",
            ));
        };
        let type_identity = *type_identity;
        if self
            .lock()
            .map_err(MirNativeCursorError::internal)?
            .finalization_handler
            .is_none()
        {
            return Err(MirNativeCursorError::internal(
                "Shared interop weak upgrades require an origin outcome handler",
            ));
        }
        self.validate_active_execution_authority()
            .map_err(MirNativeCursorError::internal)?;
        let (weak, _weak_lease) =
            self.borrow_shared_interop_weak_root(capability, type_identity)?;
        let Some(upgrade) = weak
            .upgrade()
            .map_err(MirNativeCursorError::internal)?
        else {
            return Ok(None);
        };
        let (root, alias) = upgrade.into_parts();
        if root.type_id() != type_identity {
            return Err(MirNativeCursorError::internal(
                "Shared interop weak upgrade checked type identity mismatch",
            ));
        }
        let token_id = alias.token_id();
        let strong_capability = self
            .register_shared_interop_root(type_identity, &root)
            .map_err(MirNativeCursorError::internal)?;
        let lease = match self.retain_callback_lease() {
            Ok(lease) => lease,
            Err(error) => {
                let _ = self.release_shared_interop_root(&strong_capability);
                return Err(MirNativeCursorError::internal(error));
            }
        };
        if let Err(error) = self.store_shared_interop_alias(&strong_capability, alias) {
            drop(lease);
            let _ = self.release_shared_interop_root(&strong_capability);
            return Err(MirNativeCursorError::internal(error));
        }
        Ok(Some((root, strong_capability, lease, token_id)))
    }

    /// Release one checked weak-root capability.
    pub fn release_shared_interop_weak_root(
        &self,
        capability: &SourceResourceHandle,
    ) -> Result<SourceResourceRelease, String> {
        if capability.handle != shared_interop_weak_root_handle_id()
            || !matches!(
                &capability.kind,
                SourceResourceKind::SharedInteropWeakRoot { .. }
            )
        {
            return Err("Source capability is not a SharedInteropWeakRoot".to_string());
        }
        self.release_capability(capability)
    }




    /// Clone a receiver endpoint into a fresh logical arena capability while
    /// retaining the scheduler's physical receiver count.
    pub fn clone_channel_receiver(
        &self,
        source_handle: MirHandleId,
        source_raw: i64,
        target_handle: MirHandleId,
    ) -> Result<SourceResourceHandle, MirNativeCursorError> {
        let owner = self.owner_for_raw(
            source_handle,
            source_raw,
            &SourceResourceKind::ChannelReceiver,
        )?;
        let receiver = {
            let owner = owner.lock().map_err(|_| {
                MirNativeCursorError::internal("Source resource owner lock is poisoned")
            })?;
            let BackendOwner::ChannelReceiver(receiver) = &*owner else {
                return Err(MirNativeCursorError::internal(
                    "Source channel receiver capability payload mismatch",
                ));
            };
            receiver.clone()
        };
        self.register_channel_receiver(target_handle, &receiver)
            .map_err(MirNativeCursorError::internal)
    }

    /// Clone a sender endpoint into a fresh logical arena capability while
    /// retaining the scheduler's physical sender count.
    pub fn clone_channel_sender(
        &self,
        source_handle: MirHandleId,
        source_raw: i64,
        target_handle: MirHandleId,
    ) -> Result<SourceResourceHandle, MirNativeCursorError> {
        let owner = self.owner_for_raw(
            source_handle,
            source_raw,
            &SourceResourceKind::ChannelSender,
        )?;
        let sender = {
            let owner = owner.lock().map_err(|_| {
                MirNativeCursorError::internal("Source resource owner lock is poisoned")
            })?;
            let BackendOwner::ChannelSender(sender) = &*owner else {
                return Err(MirNativeCursorError::internal(
                    "Source channel sender capability payload mismatch",
                ));
            };
            sender.clone()
        };
        self.register_channel_sender(target_handle, sender)
            .map_err(MirNativeCursorError::internal)
    }

    /// Create one canonical sender/receiver endpoint pair. Each endpoint gets
    /// its own arena capability and scheduler reference count.
    pub fn register_new_channel(
        &self,
        sender_handle: MirHandleId,
        receiver_handle: MirHandleId,
        capacity: Option<i64>,
    ) -> Result<(SourceResourceHandle, SourceResourceHandle), String> {
        let receiver = match capacity {
            Some(capacity) => jet_codegen::scheduler::JetSchedulerChannel::bounded(capacity),
            None => jet_codegen::scheduler::JetSchedulerChannel::new(),
        };
        let sender = receiver.sender();
        let sender_capability = self.register_channel_sender(sender_handle, sender)?;
        match self.register_channel_receiver(receiver_handle, &receiver) {
            Ok(receiver_capability) => Ok((sender_capability, receiver_capability)),
            Err(error) => {
                let _ = self.release_capability(&sender_capability);
                Err(error)
            }
        }
    }

    pub fn register_timer_channel(
        &self,
        handle: MirHandleId,
        delay_ms: i64,
    ) -> Result<SourceResourceHandle, String> {
        self.register_channel_receiver(
            handle,
            &jet_codegen::scheduler::JetSchedulerChannel::timer(delay_ms),
        )
    }

    pub fn register_interval_channel(
        &self,
        handle: MirHandleId,
        delay_ms: i64,
    ) -> Result<SourceResourceHandle, String> {
        self.register_channel_receiver(
            handle,
            &jet_codegen::scheduler::JetSchedulerChannel::interval(delay_ms),
        )
    }


    /// Register a scheduler stream carrying Source runtime values.
    pub fn register_plain_stream(
        &self,
        handle: MirHandleId,
        stream: jet_codegen::scheduler::JetStream<MirRuntimeValue>,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(handle, BackendOwner::PlainStream(stream))
    }
    /// Create a canonical pull-gated stream and register its consumer owner.
    pub fn register_new_plain_stream(
        &self,
        handle: MirHandleId,
    ) -> Result<
        (
            SourceResourceHandle,
            jet_codegen::scheduler::JetStreamSender<MirRuntimeValue>,
        ),
        String,
    > {
        let (sender, stream) = jet_codegen::scheduler::jet_stream();
        let capability = self.register_plain_stream(handle, stream)?;
        Ok((capability, sender))
    }

    /// Register an already-constructed canonical encoding reader.
    pub fn register_encoding_reader(
        &self,
        handle: MirHandleId,
        reader: SourceEncodingReader,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(handle, BackendOwner::Encoding(reader))
    }

    /// Adopt a resident typed codec reader without reopening the underlying
    /// file or retaining a second integer slot owner.
    pub fn register_encoding_reader_handle(
        &self,
        handle: MirHandleId,
        reader_handle: i64,
        reader_type: &str,
    ) -> Result<SourceResourceHandle, String> {
        self.register_encoding_reader(
            handle,
            SourceEncodingReader::from_handle(reader_handle, reader_type)?,
        )
    }

    /// Register an already-constructed canonical encoding writer.
    pub fn register_encoding_writer(
        &self,
        handle: MirHandleId,
        writer: SourceEncodingWriter,
    ) -> Result<SourceResourceHandle, String> {
        self.register_backend(handle, BackendOwner::EncodingWriter(writer))
    }

    /// Consume a live file-writer capability into a typed codec writer and
    /// register the resulting lease under a caller-selected capability.
    /// The file capability is moved before codec construction; constructor
    /// failure therefore drops the physical writer instead of reopening it.
    pub fn register_encoding_writer_from_file(
        &self,
        output_handle: MirHandleId,
        file_handle: MirHandleId,
        file_raw: i64,
        writer_type: &str,
    ) -> SourceResourceTransferOutcome {
        self.register_encoding_writer_from_file_with_options(
            output_handle,
            file_handle,
            file_raw,
            writer_type,
            SourceEncodingLimits::safe(),
            false,
        )
    }

    /// Consume a live file-reader capability into a typed codec reader and
    /// register the resulting lease under a caller-selected capability.
    pub fn register_encoding_reader_from_file(
        &self,
        output_handle: MirHandleId,
        file_handle: MirHandleId,
        file_raw: i64,
        reader_type: &str,
    ) -> SourceResourceTransferOutcome {
        self.register_encoding_reader_from_file_with_options(
            output_handle,
            file_handle,
            file_raw,
            reader_type,
            SourceEncodingLimits::safe(),
            ",",
            false,
            false,
        )
    }

    pub fn register_encoding_reader_from_file_with_options(
        &self,
        output_handle: MirHandleId,
        file_handle: MirHandleId,
        file_raw: i64,
        reader_type: &str,
        limits: SourceEncodingLimits,
        delimiter: &str,
        header: bool,
        skip_blank: bool,
    ) -> SourceResourceTransferOutcome {
        self.register_encoding_reader_from_file_with_xml_options(
            output_handle,
            file_handle,
            file_raw,
            reader_type,
            limits,
            delimiter,
            header,
            skip_blank,
            SourceXmlParseOptions::safe(),
        )
    }

    pub fn register_encoding_reader_from_file_with_xml_options(
        &self,
        output_handle: MirHandleId,
        file_handle: MirHandleId,
        file_raw: i64,
        reader_type: &str,
        limits: SourceEncodingLimits,
        delimiter: &str,
        header: bool,
        skip_blank: bool,
        xml_options: SourceXmlParseOptions,
    ) -> SourceResourceTransferOutcome {
        if let Err(error) = SourceEncodingReader::validate_type(reader_type) {
            return SourceResourceTransferOutcome {
                consumed: None,
                result: Err(error),
            };
        }
        let (source, file) = match self
            .take_file_owner::<SourceFileReader>(
                file_handle,
                file_raw,
                SourceResourceKind::LinesFile,
            ) {
            Ok(owners) => owners,
            Err(error) => {
                return SourceResourceTransferOutcome {
                    consumed: None,
                    result: Err(error),
                }
            }
        };
        let reader = match SourceEncodingReader::from_file_with_xml_options(
            file,
            reader_type,
            limits,
            delimiter,
            header,
            skip_blank,
            xml_options,
        ) {
            Ok(reader) => reader,
            Err(error) => {
                return SourceResourceTransferOutcome {
                    consumed: Some(source),
                    result: Err(error),
                }
            }
        };
        let result = self
            .register_encoding_reader(output_handle, reader)
            .map_err(MirNativeCursorError::internal);
        SourceResourceTransferOutcome {
            consumed: Some(source),
            result,
        }
    }

    pub fn register_encoding_writer_from_file_with_options(
        &self,
        output_handle: MirHandleId,
        file_handle: MirHandleId,
        file_raw: i64,
        writer_type: &str,
        limits: SourceEncodingLimits,
        canonical: bool,
    ) -> SourceResourceTransferOutcome {
        self.register_encoding_writer_from_file_with_xml_options(
            output_handle,
            file_handle,
            file_raw,
            writer_type,
            limits,
            canonical,
            SourceXmlRenderOptions::safe(),
        )
    }

    pub fn register_encoding_writer_from_file_with_xml_options(
        &self,
        output_handle: MirHandleId,
        file_handle: MirHandleId,
        file_raw: i64,
        writer_type: &str,
        limits: SourceEncodingLimits,
        canonical: bool,
        xml_options: SourceXmlRenderOptions,
    ) -> SourceResourceTransferOutcome {
        if let Err(error) = SourceEncodingWriter::validate_type(writer_type) {
            return SourceResourceTransferOutcome {
                consumed: None,
                result: Err(error),
            };
        }
        let (source, file) = match self
            .take_file_owner::<SourceFileWriter>(
                file_handle,
                file_raw,
                SourceResourceKind::FileWriter,
            ) {
            Ok(owners) => owners,
            Err(error) => {
                return SourceResourceTransferOutcome {
                    consumed: None,
                    result: Err(error),
                }
            }
        };
        let writer = match SourceEncodingWriter::from_file_with_xml_options(
            file,
            writer_type,
            limits,
            canonical,
            xml_options,
        ) {
            Ok(writer) => writer,
            Err(error) => {
                return SourceResourceTransferOutcome {
                    consumed: Some(source),
                    result: Err(error),
                }
            }
        };
        let result = self
            .register_encoding_writer(output_handle, writer)
            .map_err(MirNativeCursorError::internal);
        SourceResourceTransferOutcome {
            consumed: Some(source),
            result,
        }
    }

    /// Open a canonical file and construct one of the checked encoding reader
    /// owners before registering it in this invocation's table.
    pub fn open_encoding_reader(
        &self,
        handle: MirHandleId,
        path: &str,
        reader_type: &str,
    ) -> Result<SourceResourceHandle, MirNativeCursorError> {
        let file = SourceFileReader::open(path)?;
        let reader = SourceEncodingReader::from_file(file, reader_type)?;
        self.register_encoding_reader(handle, reader)
            .map_err(MirNativeCursorError::internal)
    }

    pub fn open_encoding_reader_with_limits(
        &self,
        handle: MirHandleId,
        path: &str,
        reader_type: &str,
        limits: SourceEncodingLimits,
    ) -> Result<SourceResourceHandle, MirNativeCursorError> {
        let file = SourceFileReader::open(path)?;
        let reader = SourceEncodingReader::from_file_with_limits(file, reader_type, limits)?;
        self.register_encoding_reader(handle, reader)
            .map_err(MirNativeCursorError::internal)
    }

    pub fn open_encoding_reader_with_options(
        &self,
        handle: MirHandleId,
        path: &str,
        reader_type: &str,
        limits: SourceEncodingLimits,
        delimiter: &str,
        header: bool,
        skip_blank: bool,
    ) -> Result<SourceResourceHandle, MirNativeCursorError> {
        let file = SourceFileReader::open(path)?;
        let reader = SourceEncodingReader::from_file_with_options(
            file,
            reader_type,
            limits,
            delimiter,
            header,
            skip_blank,
        )?;
        self.register_encoding_reader(handle, reader)
            .map_err(MirNativeCursorError::internal)
    }



    /// Insert a native cursor carrier under the private checked cursor handle.
    /// The raw slot and generation are allocated by this arena, not supplied by
    /// a caller or copied from a backend integer handle.
    pub fn insert_cursor(
        &self,
        handle: MirHandleId,
        cursor: MirNativeCursor,
    ) -> Result<SourceResourceHandle, String> {
        if handle != loop_cursor_handle_id() {
            return Err("native cursor uses the checked Prelude cursor handle identity".to_string());
        }
        let active_lease = active_source_resource_lease_snapshot();
        let result = {
            let mut state = self.lock()?;
            let allow_deferred_retirement =
                self.validate_deferred_execution_authority(&state, active_lease.as_ref())?;
            Self::allocate_slot(
                &mut state,
                handle,
                SourceResourceKind::NativeCursor,
                SlotEntry::Cursor(cursor.clone()),
                allow_deferred_retirement,
            )
        };
        drop(cursor);
        result
    }
    /// Adopt an already-created native cursor value without exposing a
    /// serializable integer representation of its payload.
    pub fn insert_cursor_value(
        &self,
        handle: MirHandleId,
        value: &MirRuntimeValue,
    ) -> Result<SourceResourceHandle, String> {
        let MirRuntimeValue::NativeCursor(cursor) = value else {
            return Err("cursor slot requires the private NativeCursor carrier".to_string());
        };
        self.insert_cursor(handle, cursor.clone())
    }

    /// Resolve a cursor carrier only through the exact private cursor handle
    /// and the arena-issued raw slot.
    pub fn lookup_cursor(
        &self,

        handle: MirHandleId,
        raw: i64,
    ) -> Result<MirNativeCursor, String> {
        if handle != loop_cursor_handle_id() {
            return Err("native cursor handle identity mismatch".to_string());
        }
        let state = self.lock()?;
        let (slot_id, generation) = Self::decode_raw(raw)?;
        let slot = state
            .slots
            .get(&slot_id)
            .ok_or_else(|| "stale or unknown native cursor capability".to_string())?;
        if slot.generation != generation
            || slot.handle != handle
            || slot.kind != SourceResourceKind::NativeCursor
        {
            return Err("native cursor capability generation, handle, or kind mismatch".to_string());
        }
        match &slot.entry {
            SlotEntry::Cursor(cursor) => Ok(cursor.clone()),
            SlotEntry::Backend(_) => Err("native cursor capability payload mismatch".to_string()),
        }
    }

    /// Release a cursor slot after validating its exact identity.
    pub fn release_cursor(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<SourceResourceRelease, String> {
        self.release(handle, raw, &SourceResourceKind::NativeCursor)
    }

    fn finalize_removed_slot(removed: Slot) -> SourceResourceRelease {
        let values = match removed.entry {
            SlotEntry::Cursor(_) => Vec::new(),
            SlotEntry::Backend(owner) => {
                let owner = owner
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                match &*owner {
                    BackendOwner::ChannelReceiver(receiver) => {
                        receiver.close_receiver_and_drain()
                    }
                    BackendOwner::ChannelSender(_)
                    | BackendOwner::NativeBinding(_)
                    | BackendOwner::PlainStream(_)
                    | BackendOwner::File(_)
                    | BackendOwner::FileWriter(_)
                    | BackendOwner::Stdin(_)
                    | BackendOwner::Process(_)
                    | BackendOwner::Encoding(_)
                    | BackendOwner::EncodingWriter(_)
                    | BackendOwner::SharedInteropRoot(_)
                    | BackendOwner::SharedInteropWeakRoot { .. } => Vec::new(),
                }
            }
        };
        if values.is_empty() {
            SourceResourceRelease::Released
        } else {
            SourceResourceRelease::DrainedChannel(values)
        }
    }

    fn drain_removed_slots(slots: HashMap<u32, Slot>) -> Vec<MirRuntimeValue> {
        let mut values = Vec::new();
        for slot in slots.into_values() {
            values.extend(Self::finalize_removed_slot(slot).drained_values());
        }
        values
    }

    pub fn release_resource(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<SourceResourceRelease, String> {
        let removed = {
            let mut state = self.lock()?;
            let (slot_id, generation) = Self::decode_raw(raw)?;
            let slot = state
                .slots
                .get(&slot_id)
                .ok_or_else(|| "stale or unknown Source resource capability".to_string())?;
            if slot.generation != generation || slot.handle != handle {
                return Err("Source resource generation or handle mismatch".to_string());
            }
            Self::ensure_no_shared_interop_aliases_locked(&state, raw)?;
            let removed = state.slots.remove(&slot_id).ok_or_else(|| {
                "Source resource capability disappeared".to_string()
            })?;
            removed
        };
        Ok(Self::finalize_removed_slot(removed))
    }

    /// Native callback spelling for the shared Core/Cursor release route.
    pub fn resource_release(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<SourceResourceRelease, MirNativeCursorError> {
        self.release_resource(handle, raw)
            .map_err(MirNativeCursorError::internal)
    }
    /// Release any backend owner through the exact checked triple.
    pub fn release(
        &self,
        handle: MirHandleId,
        raw: i64,
        expected_kind: &SourceResourceKind,
    ) -> Result<SourceResourceRelease, String> {
        let removed = {
            let mut state = self.lock()?;
            let (slot_id, generation) = Self::decode_raw(raw)?;
            let slot = state
                .slots
                .get(&slot_id)
                .ok_or_else(|| "stale or unknown Source resource capability".to_string())?;
            if slot.generation != generation
                || slot.handle != handle
                || &slot.kind != expected_kind
            {
                return Err("Source resource generation, handle, or kind mismatch".to_string());
            }
            Self::ensure_no_shared_interop_aliases_locked(&state, raw)?;
            let removed = state.slots.remove(&slot_id).ok_or_else(|| {
                "Source resource capability disappeared".to_string()
            })?;
            removed
        };
        Ok(Self::finalize_removed_slot(removed))
    }

    /// Release using the generation-bearing capability returned by an insert.
    pub fn release_capability(
        &self,
        capability: &SourceResourceHandle,
    ) -> Result<SourceResourceRelease, String> {
        let removed = {
            let mut state = self.lock()?;
            let (slot_id, generation) = Self::decode_raw(capability.raw)?;
            if generation != capability.generation {
                return Err("stale Source resource generation mismatch".to_string());
            }
            let slot = state
                .slots
                .get(&slot_id)
                .ok_or_else(|| "stale or unknown Source resource capability".to_string())?;
            if slot.generation != generation
                || slot.handle != capability.handle
                || slot.kind != capability.kind
            {
                return Err("stale Source resource generation or kind mismatch".to_string());
            }
            Self::ensure_no_shared_interop_aliases_locked(&state, capability.raw)?;
            let removed = state.slots.remove(&slot_id).ok_or_else(|| {
                "Source resource capability disappeared".to_string()
            })?;
            removed
        };
        Ok(Self::finalize_removed_slot(removed))
    }

    /// Validate an opaque `(handle, raw)` packet against this arena and return
    /// its generation-bearing capability.  Native hosts must use this lookup
    /// rather than decoding the raw value or manufacturing a resource kind.
    pub fn lookup_capability(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<SourceResourceHandle, String> {
        let state = self.lock()?;
        let (slot_id, generation) = Self::decode_raw(raw)?;
        let slot = state
            .slots
            .get(&slot_id)
            .ok_or_else(|| "stale or unknown Source resource capability".to_string())?;
        if slot.generation != generation || slot.handle != handle {
            return Err("Source resource generation or handle mismatch".to_string());
        }
        Ok(SourceResourceHandle {
            handle: slot.handle,
            raw,
            kind: slot.kind.clone(),
            generation: slot.generation,
        })
    }

    /// Borrow a registered binding after validating the exact value kind,
    /// execution/artifact scope, and checked MIR type. The physical carrier
    /// and object identity are returned only after this metadata check; no
    /// dispatch or Source-level alias policy is implemented here.
    pub fn borrow_native_binding(
        &self,
        handle: MirHandleId,
        raw: i64,
        expected: &SourceNativeBindingIdentity,
    ) -> Result<SourceNativeBinding, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::NativeBinding)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("native binding owner lock is poisoned"))?;
        let BackendOwner::NativeBinding(binding) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "native binding capability payload mismatch",
            ));
        };
        if !binding.matches_expected(expected) {
            return Err(MirNativeCursorError::internal(
                "native binding value kind, type, or scope mismatch",
            ));
        }
        Ok(binding.clone())
    }

    /// Release a native binding capability through the exact checked kind.
    pub fn release_native_binding(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<SourceResourceRelease, String> {
        self.release(handle, raw, &SourceResourceKind::NativeBinding)
    }

    pub fn release_native_binding_capability(
        &self,
        capability: &SourceResourceHandle,
    ) -> Result<SourceResourceRelease, String> {
        if capability.kind != SourceResourceKind::NativeBinding {
            return Err("Source capability is not a NativeBinding".to_string());
        }
        self.release_capability(capability)
    }

    fn owner_for_raw(
        &self,
        handle: MirHandleId,
        raw: i64,
        expected_kind: &SourceResourceKind,
    ) -> Result<Arc<Mutex<BackendOwner>>, MirNativeCursorError> {
        let state = self.lock().map_err(MirNativeCursorError::internal)?;
        let (slot_id, generation) = Self::decode_raw(raw)
            .map_err(MirNativeCursorError::internal)?;
        let slot = state
            .slots
            .get(&slot_id)
            .ok_or_else(|| {
                MirNativeCursorError::internal("stale or unknown Source resource capability")
            })?;
        if slot.generation != generation
            || slot.handle != handle
            || &slot.kind != expected_kind
        {
            return Err(MirNativeCursorError::internal(
                "Source resource generation, handle, or kind mismatch",
            ));
        }
        match &slot.entry {
            SlotEntry::Backend(owner) => Ok(Arc::clone(owner)),
            SlotEntry::Cursor(_) => Err(MirNativeCursorError::internal(
                "Source resource capability payload mismatch",
            )),
        }
    }

    /// Atomically consume one sole-owner backend slot into a physical lease.
    /// The returned capability is committed stale; the lease is the only
    /// remaining owner and carries no serializable raw capability payload.
    pub fn take_owned(
        &self,
        handle: MirHandleId,
        raw: i64,
        expected_kind: &SourceResourceKind,
    ) -> Result<(SourceResourceHandle, SourceOwnedLease), MirNativeCursorError> {
        let mut state = self.lock().map_err(MirNativeCursorError::internal)?;
        let (slot_id, generation) =
            Self::decode_raw(raw).map_err(MirNativeCursorError::internal)?;
        let slot = state
            .slots
            .get(&slot_id)
            .ok_or_else(|| {
                MirNativeCursorError::internal("stale or unknown Source resource capability")
            })?;
        if slot.generation != generation
            || slot.handle != handle
            || &slot.kind != expected_kind
        {
            return Err(MirNativeCursorError::internal(
                "Source resource generation, handle, or kind mismatch",
            ));
        }
        Self::ensure_no_shared_interop_aliases_locked(&state, raw)
            .map_err(MirNativeCursorError::internal)?;
        let consumed = SourceResourceHandle {
            handle: slot.handle,
            raw,
            kind: slot.kind.clone(),
            generation: slot.generation,
        };
        let slot = state
            .slots
            .remove(&slot_id)
            .ok_or_else(|| {
                MirNativeCursorError::internal("Source resource capability disappeared")
            })?;
        let Slot {
            handle: slot_handle,
            kind: slot_kind,
            generation: slot_generation,
            entry,
        } = slot;
        let owner = match entry {
            SlotEntry::Backend(owner) => match Arc::try_unwrap(owner) {
                Ok(owner) => owner,
                Err(owner) => {
                    state.slots.insert(
                        slot_id,
                        Slot {
                            handle: slot_handle,
                            kind: slot_kind,
                            generation: slot_generation,
                            entry: SlotEntry::Backend(owner),
                        },
                    );
                    return Err(MirNativeCursorError::internal(
                        "Source resource owner is still aliased",
                    ));
                }
            },
            SlotEntry::Cursor(cursor) => {
                state.slots.insert(
                    slot_id,
                    Slot {
                        handle: slot_handle,
                        kind: slot_kind,
                        generation: slot_generation,
                        entry: SlotEntry::Cursor(cursor),
                    },
                );
                return Err(MirNativeCursorError::internal(
                    "Source resource capability payload mismatch",
                ));
            }
        };
        let owner = match owner.into_inner() {
            Ok(owner) => owner,
            Err(poison) => {
                let owner = poison.into_inner();
                state.slots.insert(
                    slot_id,
                    Slot {
                        handle: slot_handle,
                        kind: slot_kind,
                        generation: slot_generation,
                        entry: SlotEntry::Backend(Arc::new(Mutex::new(owner))),
                    },
                );
                return Err(MirNativeCursorError::internal(
                    "Source resource owner lock is poisoned",
                ));
            }
        };
        let lease = SourceOwnedLease {
            owner: Arc::new(Mutex::new(owner)),
            consumed: consumed.clone(),
            kind: slot_kind,
        };
        Ok((consumed, lease))
    }

    /// Convenience form for a checked transfer whose logical payload is
    /// already available at the native boundary. Failure returns that payload
    /// in an owned envelope rather than Rust-dropping it.
    pub fn take_owned_with_value(
        &self,
        handle: MirHandleId,
        raw: i64,
        expected_kind: &SourceResourceKind,
        value: MirRuntimeValue,
    ) -> Result<(SourceResourceHandle, SourceOwnedValue), SourceOwnedTakeError> {
        match self.take_owned(handle, raw, expected_kind) {
            Ok((consumed, lease)) => Ok((consumed, lease.with_value(value))),
            Err(error) => Err(SourceOwnedTakeError { error, value }),
        }
    }

    /// Consume a checked Core owner after deriving its Source kind from the
    /// canonical owner registration and nominal type. Kind validation happens
    /// before the capability is taken, while `take_owned_with_value` validates
    /// the live `(handle, raw, kind)` triple and sole-owner state.
    pub fn take_owned_with_core_owner_type(
        &self,
        handle: MirHandleId,
        raw: i64,
        registration: &MirCoreOwner,
        owner_type: &MirType,
        value: MirRuntimeValue,
    ) -> Result<(SourceResourceHandle, SourceOwnedValue), SourceOwnedTakeError> {
        let expected_kind = match SourceResourceKind::from_core_owner_type(registration, owner_type)
        {
            Ok(kind) => kind,
            Err(error) => {
                return Err(SourceOwnedTakeError {
                    error: MirNativeCursorError::internal(error),
                    value,
                })
            }
        };
        self.take_owned_with_value(handle, raw, &expected_kind, value)
    }
    /// Commit one physical lease into a fresh destination slot. The source
    /// receipt is stale regardless of destination success. On any failure,
    /// the complete detached `SourceOwnedValue` remains in the error envelope.
    pub fn adopt_owned(
        &self,
        target_handle: MirHandleId,
        expected_kind: &SourceResourceKind,
        owned: SourceOwnedValue,
    ) -> SourceOwnedTransferOutcome {
        let SourceOwnedValue {
            value,
            owner,
            consumed,
            kind,
        } = owned;
        if &kind != expected_kind {
            return SourceOwnedTransferOutcome {
                consumed: consumed.clone(),
                result: Err(SourceOwnedTransferFailure {
                    error: MirNativeCursorError::internal(
                        "Source owned payload kind mismatch",
                    ),
                    owned: SourceOwnedValue {
                        value,
                        owner,
                        consumed,
                        kind,
                    },
                }),
            };
        }
        let result = match self.register_backend_arc_owned(target_handle, kind, owner) {
            Ok(capability) => Ok((capability, value)),
            Err((error, owner)) => Err(SourceOwnedTransferFailure {
                error: MirNativeCursorError::internal(error),
                owned: SourceOwnedValue {
                    value,
                    owner,
                    consumed: consumed.clone(),
                    kind: expected_kind.clone(),
                },
            }),
        };
        SourceOwnedTransferOutcome { consumed, result }
    }

    fn adopt_owned_with_cleanup(
        &self,
        cleanup: &SourceResourceCleanupLease,
        target_handle: MirHandleId,
        expected_kind: &SourceResourceKind,
        owned: SourceOwnedValue,
    ) -> SourceOwnedTransferOutcome {
        let SourceOwnedValue {
            value,
            owner,
            consumed,
            kind,
        } = owned;
        if &kind != expected_kind {
            return SourceOwnedTransferOutcome {
                consumed: consumed.clone(),
                result: Err(SourceOwnedTransferFailure {
                    error: MirNativeCursorError::internal(
                        "Source owned payload kind mismatch",
                    ),
                    owned: SourceOwnedValue {
                        value,
                        owner,
                        consumed,
                        kind,
                    },
                }),
            };
        }
        let result = match self.register_cleanup_backend_arc_owned(
            cleanup,
            target_handle,
            kind,
            owner,
        ) {
            Ok(capability) => Ok((capability, value)),
            Err((error, owner)) => Err(SourceOwnedTransferFailure {
                error: MirNativeCursorError::internal(error),
                owned: SourceOwnedValue {
                    value,
                    owner,
                    consumed: consumed.clone(),
                    kind: expected_kind.clone(),
                },
            }),
        };
        SourceOwnedTransferOutcome { consumed, result }
    }

    /// Consume a live file-reader capability into its typed Rust owner.
    /// Aliased owners are rejected and the slot remains installed.
    pub fn take_file_reader(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<SourceFileReader, MirNativeCursorError> {
        self.take_file_owner(handle, raw, SourceResourceKind::LinesFile)
            .map(|(_, reader)| reader)
    }

    /// Consume a live file-writer capability into its typed Rust owner.
    /// Aliased owners are rejected and the slot remains installed.
    pub fn take_file_writer(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<SourceFileWriter, MirNativeCursorError> {
        self.take_file_owner(handle, raw, SourceResourceKind::FileWriter)
            .map(|(_, writer)| writer)
    }

    fn take_file_owner<T>(
        &self,
        handle: MirHandleId,
        raw: i64,
        expected_kind: SourceResourceKind,
    ) -> Result<(SourceResourceHandle, T), MirNativeCursorError>
    where
        T: FromBackendOwner,
    {
        let mut state = self.lock().map_err(MirNativeCursorError::internal)?;
        let (slot_id, generation) = Self::decode_raw(raw)
            .map_err(MirNativeCursorError::internal)?;
        let slot = state
            .slots
            .get(&slot_id)
            .ok_or_else(|| {
                MirNativeCursorError::internal("stale or unknown Source resource capability")
            })?;
        if slot.generation != generation
            || slot.handle != handle
            || slot.kind != expected_kind
        {
            return Err(MirNativeCursorError::internal(
                "Source resource generation, handle, or kind mismatch",
            ));
        }
        let source = SourceResourceHandle {
            handle: slot.handle,
            raw,
            kind: slot.kind.clone(),
            generation: slot.generation,
        };
        let slot = state
            .slots
            .remove(&slot_id)
            .ok_or_else(|| {
                MirNativeCursorError::internal("Source resource capability disappeared")
            })?;
        let Slot {
            handle: slot_handle,
            kind: slot_kind,
            generation: slot_generation,
            entry,
        } = slot;
        let owner = match entry {
            SlotEntry::Backend(owner) => match Arc::try_unwrap(owner) {
                Ok(owner) => owner,
                Err(owner) => {
                    state.slots.insert(
                        slot_id,
                        Slot {
                            handle: slot_handle,
                            kind: slot_kind,
                            generation: slot_generation,
                            entry: SlotEntry::Backend(owner),
                        },
                    );
                    return Err(MirNativeCursorError::internal(
                        "Source resource owner is still aliased",
                    ));
                }
            },
            SlotEntry::Cursor(cursor) => {
                state.slots.insert(
                    slot_id,
                    Slot {
                        handle: slot_handle,
                        kind: slot_kind,
                        generation: slot_generation,
                        entry: SlotEntry::Cursor(cursor),
                    },
                );
                return Err(MirNativeCursorError::internal(
                    "Source resource capability payload mismatch",
                ));
            }
        };
        let owner = match owner.into_inner() {
            Ok(owner) => owner,
            Err(poison) => {
                let owner = poison.into_inner();
                state.slots.insert(
                    slot_id,
                    Slot {
                        handle: slot_handle,
                        kind: slot_kind,
                        generation: slot_generation,
                        entry: SlotEntry::Backend(Arc::new(Mutex::new(owner))),
                    },
                );
                return Err(MirNativeCursorError::internal(
                    "Source resource owner lock is poisoned",
                ));
            }
        };
        match T::from_backend_owner(owner) {
            Ok(owner) => Ok((source, owner)),
            Err(owner) => {
                state.slots.insert(
                    slot_id,
                    Slot {
                        handle: slot_handle,
                        kind: slot_kind,
                        generation: slot_generation,
                        entry: SlotEntry::Backend(Arc::new(Mutex::new(owner))),
                    },
                );
                Err(MirNativeCursorError::internal(
                    "Source resource capability payload mismatch",
                ))
            }
        }
    }

    pub fn file_writer_write_line(
        &self,
        handle: MirHandleId,
        raw: i64,
        line: &String,
    ) -> Result<(), MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::FileWriter)?;
        let mut owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::FileWriter(writer) = &mut *owner else {
            return Err(MirNativeCursorError::internal(
                "Source file writer capability payload mismatch",
            ));
        };
        crate::enc_stream::source_file_writer_write_line(&mut writer.0, line)
            .map_err(file_error)
    }

    pub fn file_writer_flush(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<(), MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::FileWriter)?;
        let mut owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::FileWriter(writer) = &mut *owner else {
            return Err(MirNativeCursorError::internal(
                "Source file writer capability payload mismatch",
            ));
        };
        crate::enc_stream::source_file_writer_flush(&mut writer.0).map_err(file_error)
    }

    pub fn file_writer_path(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<String, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::FileWriter)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::FileWriter(writer) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source file writer capability payload mismatch",
            ));
        };
        Ok(crate::enc_stream::source_file_writer_path(&writer.0))
    }

    pub fn file_reader_read_line(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<Option<String>, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::LinesFile)?;
        let mut owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::File(reader) = &mut *owner else {
            return Err(MirNativeCursorError::internal(
                "Source file reader capability payload mismatch",
            ));
        };
        crate::enc_stream::source_file_next_line(&mut reader.0)
            .map_err(file_error)
    }

    pub fn file_reader_path(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<String, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::LinesFile)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::File(reader) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source file reader capability payload mismatch",
            ));
        };
        Ok(reader.0.path.clone())
    }

    pub fn stdin_read_line(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<Option<String>, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::LinesStdin)?;
        let mut owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::Stdin(reader) = &mut *owner else {
            return Err(MirNativeCursorError::internal(
                "Source stdin capability payload mismatch",
            ));
        };
        crate::enc_stream::source_stdin_next_line(reader)
            .map_err(process_error)
    }

    pub fn channel_sender_reserve(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<
        jet_codegen::scheduler::JetSchedulerSendReservation<MirRuntimeValue>,
        MirNativeCursorError,
    > {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::ChannelSender)?;
        let sender = {
            let owner = owner
                .lock()
                .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
            let BackendOwner::ChannelSender(sender) = &*owner else {
                return Err(MirNativeCursorError::internal(
                    "Source channel sender capability payload mismatch",
                ));
            };
            sender.clone()
        };
        sender.reserve_send().map_err(|error| {
            let message = match error {
                jet_codegen::scheduler::JetSchedulerSendReserveError::Closed => {
                    "Source channel sender is closed"
                }
                jet_codegen::scheduler::JetSchedulerSendReserveError::Cancelled => {
                    "Source channel send was cancelled"
                }
                jet_codegen::scheduler::JetSchedulerSendReserveError::ReservationCountExhausted => {
                    "Source channel send reservation count exhausted"
                }
                jet_codegen::scheduler::JetSchedulerSendReserveError::QueueAllocationFailed => {
                    "Source channel send queue reservation failed"
                }
            };
            MirNativeCursorError::internal(message)
        })
    }

    pub fn channel_sender_send(
        &self,
        handle: MirHandleId,
        raw: i64,
        value: MirRuntimeValue,
    ) -> Result<(), SourceChannelSendError> {
        let reservation = match self.channel_sender_reserve(handle, raw) {
            Ok(reservation) => reservation,
            Err(error) => return Err(SourceChannelSendError { error, value }),
        };
        reservation
            .commit(value)
            .map_err(|value| SourceChannelSendError {
                error: MirNativeCursorError::internal("Source channel sender is closed"),
                value,
            })
    }

    pub fn channel_sender_close(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<(), MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::ChannelSender)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::ChannelSender(sender) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source channel sender capability payload mismatch",
            ));
        };
        sender.close();
        Ok(())
    }

    pub fn channel_receiver_receive(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<Option<MirRuntimeValue>, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::ChannelReceiver)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::ChannelReceiver(receiver) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source channel receiver capability payload mismatch",
            ));
        };
        Ok(receiver.receive())
    }

    pub fn channel_receiver_try_receive(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<Option<MirRuntimeValue>, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::ChannelReceiver)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::ChannelReceiver(receiver) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source channel receiver capability payload mismatch",
            ));
        };
        Ok(receiver.try_receive())
    }

    pub fn channel_receiver_is_timer(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<bool, MirNativeCursorError> {
        self.channel_receiver_flag(handle, raw, |receiver| receiver.is_timer())
    }

    pub fn channel_receiver_is_interval(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<bool, MirNativeCursorError> {
        self.channel_receiver_flag(handle, raw, |receiver| receiver.is_interval())
    }

    pub fn channel_receiver_is_cancelled(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<bool, MirNativeCursorError> {
        self.channel_receiver_flag(handle, raw, |receiver| receiver.is_cancelled())
    }

    pub fn channel_receiver_is_ready(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<bool, MirNativeCursorError> {
        self.channel_receiver_flag(handle, raw, |receiver| receiver.is_ready())
    }

    pub fn channel_receiver_delay_ms(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<i64, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::ChannelReceiver)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::ChannelReceiver(receiver) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source channel receiver capability payload mismatch",
            ));
        };
        Ok(receiver.delay_ms())
    }

    pub fn channel_receiver_close(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<(), MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::ChannelReceiver)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::ChannelReceiver(receiver) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source channel receiver capability payload mismatch",
            ));
        };
        receiver.close();
        Ok(())
    }
    /// Select among receiver capabilities while validating and snapshotting
    /// physical queue views in deterministic owner-lock order. Owner locks and
    /// endpoint Arcs are released before the scheduler blocks.
    pub fn channel_receiver_select(
        &self,
        receivers: &[(MirHandleId, i64)],
        after_ns: Vec<i64>,
    ) -> Result<(i64, Option<MirRuntimeValue>), MirNativeCursorError> {
        if receivers.is_empty() && after_ns.is_empty() {
            return Err(MirNativeCursorError::internal(
                "Source channel select has no arms",
            ));
        }
        let mut arm_owners = Vec::with_capacity(receivers.len());
        for (arm, (handle, raw)) in receivers.iter().enumerate() {
            arm_owners.push((
                arm,
                self.owner_for_raw(*handle, *raw, &SourceResourceKind::ChannelReceiver)?,
            ));
        }
        let mut lock_order = arm_owners
            .iter()
            .map(|(_, owner)| Arc::clone(owner))
            .collect::<Vec<_>>();
        lock_order.sort_unstable_by_key(|owner| Arc::as_ptr(owner) as usize);
        lock_order.dedup_by(|left, right| Arc::ptr_eq(left, right));
        let guards = lock_order
            .iter()
            .map(|owner| {
                owner.lock().map_err(|_| {
                    MirNativeCursorError::internal("Source resource owner lock is poisoned")
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut views = Vec::with_capacity(receivers.len());
        for (_, owner) in &arm_owners {
            let address = Arc::as_ptr(owner) as usize;
            let guard_index = lock_order
                .binary_search_by_key(&address, |owner| Arc::as_ptr(owner) as usize)
                .map_err(|_| {
                    MirNativeCursorError::internal("Source channel select owner disappeared")
                })?;
            let owner = guards.get(guard_index).ok_or_else(|| {
                MirNativeCursorError::internal("Source channel select lock disappeared")
            })?;
            let BackendOwner::ChannelReceiver(receiver) = &**owner else {
                return Err(MirNativeCursorError::internal(
                    "Source channel receiver capability payload mismatch",
                ));
            };
            views.push(receiver.select_view());
        }
        drop(guards);
        drop(lock_order);
        drop(arm_owners);
        let selected = jet_codegen::scheduler::jet_scheduler_select_channel_views_tagged(
            views, after_ns,
        );
        Ok(selected)
    }

    fn channel_receiver_flag(
        &self,
        handle: MirHandleId,
        raw: i64,
        flag: impl FnOnce(&jet_codegen::scheduler::JetSchedulerChannel<MirRuntimeValue>) -> bool,
    ) -> Result<bool, MirNativeCursorError> {
        let owner = self.owner_for_raw(handle, raw, &SourceResourceKind::ChannelReceiver)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::ChannelReceiver(receiver) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source channel receiver capability payload mismatch",
            ));
        };
        Ok(flag(receiver))
    }

    pub fn encoding_writer_write(
        &self,
        handle: MirHandleId,
        raw: i64,
        value: &MirRuntimeValue,
    ) -> Result<(), MirNativeCursorError> {
        let capability = self
            .lookup_capability(handle, raw)
            .map_err(MirNativeCursorError::internal)?;
        let owner = self.owner_for_raw(handle, raw, &capability.kind)?;
        let mut owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::EncodingWriter(writer) = &mut *owner else {
            return Err(MirNativeCursorError::internal(
                "Source encoding writer capability payload mismatch",
            ));
        };
        match &mut writer.0 {
            EncodingWriter::Json(writer) => {
                let event = data_event_from_value(value)?;
                crate::enc_stream::runtime::enc_json_writer_write(writer, event)
                    .map_err(|error| encoding_error(&error))
            }
            EncodingWriter::Jsonl(writer) => {
                let tree = data_tree_from_value(value)?;
                crate::enc_stream::runtime::enc_jsonl_writer_write(writer, tree)
                    .map_err(|error| encoding_error(&error))
            }
            EncodingWriter::Csv(writer) => {
                let row = csv_row_from_value(value)?;
                crate::enc_stream::runtime::enc_csv_writer_write(writer, row)
                    .map_err(|error| encoding_error(&error))
            }
            EncodingWriter::Xml(writer) => {
                let tree = data_tree_from_value(value)?;
                crate::enc_stream::runtime::enc_xml_writer_write(writer, tree)
                    .map_err(|error| encoding_error(&error))
            }
            EncodingWriter::Cbor(writer) => {
                let event = data_event_from_value(value)?;
                crate::enc_stream::runtime::enc_cbor_writer_write(writer, event)
                    .map_err(|error| encoding_error(&error))
            }
        }
    }

    pub fn encoding_writer_flush(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<(), MirNativeCursorError> {
        let capability = self
            .lookup_capability(handle, raw)
            .map_err(MirNativeCursorError::internal)?;
        let owner = self.owner_for_raw(handle, raw, &capability.kind)?;
        let mut owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::EncodingWriter(writer) = &mut *owner else {
            return Err(MirNativeCursorError::internal(
                "Source encoding writer capability payload mismatch",
            ));
        };
        match &mut writer.0 {
            EncodingWriter::Json(writer) => crate::enc_stream::runtime::enc_json_writer_flush(writer)
                .map_err(|error| encoding_error(&error)),
            EncodingWriter::Jsonl(writer) => crate::enc_stream::runtime::enc_jsonl_writer_flush(writer)
                .map_err(|error| encoding_error(&error)),
            EncodingWriter::Csv(writer) => crate::enc_stream::runtime::enc_csv_writer_flush(writer)
                .map_err(|error| encoding_error(&error)),
            EncodingWriter::Xml(writer) => crate::enc_stream::runtime::enc_xml_writer_flush(writer)
                .map_err(|error| encoding_error(&error)),
            EncodingWriter::Cbor(writer) => crate::enc_stream::runtime::enc_cbor_writer_flush(writer)
                .map_err(|error| encoding_error(&error)),
        }
    }

    pub fn encoding_writer_finish(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<(), MirNativeCursorError> {
        let capability = self
            .lookup_capability(handle, raw)
            .map_err(MirNativeCursorError::internal)?;
        let owner = self.owner_for_raw(handle, raw, &capability.kind)?;
        let mut owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::EncodingWriter(writer) = &mut *owner else {
            return Err(MirNativeCursorError::internal(
                "Source encoding writer capability payload mismatch",
            ));
        };
        match &mut writer.0 {
            EncodingWriter::Json(writer) => crate::enc_stream::runtime::enc_json_writer_finish(writer)
                .map_err(|error| encoding_error(&error)),
            EncodingWriter::Jsonl(writer) => crate::enc_stream::runtime::enc_jsonl_writer_finish(writer)
                .map_err(|error| encoding_error(&error)),
            EncodingWriter::Csv(writer) => crate::enc_stream::runtime::enc_csv_writer_finish(writer)
                .map_err(|error| encoding_error(&error)),
            EncodingWriter::Xml(writer) => crate::enc_stream::runtime::enc_xml_writer_finish(writer)
                .map_err(|error| encoding_error(&error)),
            EncodingWriter::Cbor(writer) => crate::enc_stream::runtime::enc_cbor_writer_finish(writer)
                .map_err(|error| encoding_error(&error)),
        }
    }

    pub fn encoding_writer_path(
        &self,
        handle: MirHandleId,
        raw: i64,
    ) -> Result<String, MirNativeCursorError> {
        let capability = self
            .lookup_capability(handle, raw)
            .map_err(MirNativeCursorError::internal)?;
        let owner = self.owner_for_raw(handle, raw, &capability.kind)?;
        let owner = owner
            .lock()
            .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?;
        let BackendOwner::EncodingWriter(writer) = &*owner else {
            return Err(MirNativeCursorError::internal(
                "Source encoding writer capability payload mismatch",
            ));
        };
        let path = match &writer.0 {
            EncodingWriter::Json(writer) => &writer.output.path,
            EncodingWriter::Jsonl(writer) => &writer.json.output.path,
            EncodingWriter::Csv(writer) => &writer.output.path,
            EncodingWriter::Xml(writer) => &writer.output.path,
            EncodingWriter::Cbor(writer) => &writer.output.path,
        };
        Ok(path.clone())
    }

    /// Build a checked cursor from a registered backend owner and retain the
    /// resulting non-serializable cursor in this same slot authority.
    ///
    /// A by-value source returns its stale source receipt even when iterator
    /// construction or cursor-slot installation fails.  Callers must carry
    /// that receipt through the native transfer outcome; they must not probe
    /// this arena after an acquisition failure.
    pub fn init_cursor(
        &self,
        key: &NativeLoopResourceKey,
        step_value: i64,
        has_step: bool,
        by_value: bool,
    ) -> SourceCursorInitOutcome {
        let acquisition = jet_codegen::Codegen::NativeLoopCursor::init_from_resource_factory(
            self, key, step_value, has_step, by_value,
        );
        let consumed = acquisition
            .consumed
            .as_ref()
            .map(Self::source_receipt_from_loop_key);
        let value = match acquisition.result {
            Ok(value) => value,
            Err(error) => {
                return SourceCursorInitOutcome {
                    consumed,
                    result: Err(error),
                }
            }
        };
        let MirRuntimeValue::NativeCursor(cursor) = value.clone() else {
            return SourceCursorInitOutcome {
                consumed,
                result: Err(MirNativeCursorError::internal(
                    "canonical resource cursor did not produce a native cursor",
                )),
            };
        };
        let capability = match self.insert_cursor(loop_cursor_handle_id(), cursor) {
            Ok(capability) => capability,
            Err(error) => {
                return SourceCursorInitOutcome {
                    consumed,
                    result: Err(MirNativeCursorError::internal(error)),
                }
            }
        };
        SourceCursorInitOutcome {
            consumed,
            result: Ok((capability, value)),
        }
    }

    fn source_receipt_from_loop_key(key: &NativeLoopResourceKey) -> SourceResourceHandle {
        let (_, generation) = Self::decode_raw(key.raw)
            .expect("committed native loop receipt must contain a valid capability");
        let kind = SourceResourceKind::from_loop_source(&key.source_kind)
            .expect("committed native loop receipt must contain a resource kind");
        SourceResourceHandle {
            handle: key.handle,
            raw: key.raw,
            kind,
            generation,
        }
    }

    fn validate_slot<'a>(
        state: &'a ArenaState,
        key: &NativeLoopResourceKey,
        expected: &SourceResourceKind,
    ) -> Result<&'a Slot, String> {
        let (slot_id, generation) = Self::decode_raw(key.raw)?;
        let slot = state
            .slots
            .get(&slot_id)
            .ok_or_else(|| "stale or unknown Source resource capability".to_string())?;
        if slot.generation != generation || slot.handle != key.handle || &slot.kind != expected {
            return Err("Source resource handle, generation, or kind mismatch".to_string());
        }
        if !expected.matches_loop_source(&key.source_kind) {
            return Err("Source resource kind does not match checked loop source".to_string());
        }
        Ok(slot)
    }

    fn take_owner(
        &self,
        key: &NativeLoopResourceKey,
        expected: SourceResourceKind,
    ) -> NativeLoopResourceOutcome<Arc<Mutex<BackendOwner>>> {
        if !expected.matches_loop_source(&key.source_kind) {
            return NativeLoopResourceOutcome {
                consumed: None,
                result: Err(MirNativeCursorError::internal(
                    "Source resource kind does not match checked loop source",
                )),
            };
        }
        match self.take_owned(key.handle, key.raw, &expected) {
            Ok((consumed, lease)) => NativeLoopResourceOutcome {
                consumed: Some(NativeLoopResourceKey {
                    handle: consumed.handle,
                    raw: consumed.raw,
                    source_kind: key.source_kind.clone(),
                }),
                result: Ok(lease.into_owner()),
            },
            Err(error) => NativeLoopResourceOutcome {
                consumed: None,
                result: Err(error),
            },
        }
    }

    fn borrow_owner(
        &self,
        key: &NativeLoopResourceKey,
        expected: SourceResourceKind,
    ) -> Result<Arc<Mutex<BackendOwner>>, MirNativeCursorError> {
        let state = self.lock().map_err(MirNativeCursorError::internal)?;
        let slot = Self::validate_slot(&state, key, &expected)
            .map_err(MirNativeCursorError::internal)?;
        match &slot.entry {
            SlotEntry::Backend(owner) => Ok(Arc::clone(owner)),
            SlotEntry::Cursor(_) => Err(MirNativeCursorError::internal(
                "native cursor is not a backend producer",
            )),
        }
    }

    fn iterator_for(
        &self,
        key: &NativeLoopResourceKey,
        expected: SourceResourceKind,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter> {
        let acquisition = if by_value {
            self.take_owner(key, expected)
        } else {
            NativeLoopResourceOutcome {
                consumed: None,
                result: self.borrow_owner(key, expected),
            }
        };
        let NativeLoopResourceOutcome { consumed, result } = acquisition;
        NativeLoopResourceOutcome {
            consumed,
            result: result.and_then(|owner| resource_iterator(owner, &key.source_kind)),
        }
    }
}

impl NativeLoopResourceFactory for SourceResourceArena {
    fn plain_stream(
        &self,
        key: &NativeLoopResourceKey,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter> {
        self.iterator_for(key, SourceResourceKind::PlainStream, by_value)
    }

    fn lines_file(
        &self,
        key: &NativeLoopResourceKey,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter> {
        self.iterator_for(key, SourceResourceKind::LinesFile, by_value)
    }

    fn lines_stdin(
        &self,
        key: &NativeLoopResourceKey,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter> {
        self.iterator_for(key, SourceResourceKind::LinesStdin, by_value)
    }

    fn lines_process_stream(
        &self,
        key: &NativeLoopResourceKey,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter> {
        self.iterator_for(key, SourceResourceKind::LinesProcessStream, by_value)
    }

    fn channel_receiver(
        &self,
        key: &NativeLoopResourceKey,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter> {
        self.iterator_for(key, SourceResourceKind::ChannelReceiver, by_value)
    }

    fn encoding_reader(
        &self,
        key: &NativeLoopResourceKey,
        reader_type: &str,
        by_value: bool,
    ) -> NativeLoopResourceOutcome<NativeIter> {
        self.iterator_for(
            key,
            SourceResourceKind::EncodingReader {
                reader_type: reader_type.to_string(),
            },
            by_value,
        )
    }
}

/// Retain one typed owner lease in every cursor.  A by-value source removes
/// the arena slot before this function, while a borrow leaves the slot in
/// place; both paths share this same `Arc` lease and therefore stay lazy
/// without a second owner or a materialized fallback.
fn resource_iterator(
    owner: Arc<Mutex<BackendOwner>>,
    source: &MirLoopSourceKind,
) -> Result<NativeIter, MirNativeCursorError> {
    let valid = owner
        .lock()
        .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))?
        .kind()
        .as_loop_source()
        .ok()
        .is_some_and(|kind| &kind == source);
    if !valid {
        return Err(MirNativeCursorError::internal(
            "Source resource owner does not match checked loop source",
        ));
    }
    match source {
        MirLoopSourceKind::Plain => Ok(Box::new(PlainIter { owner })),
        MirLoopSourceKind::LinesFile => Ok(Box::new(FileIter { owner })),
        MirLoopSourceKind::LinesStdin => Ok(Box::new(StdinIter { owner })),
        MirLoopSourceKind::LinesProcessStream => Ok(Box::new(ProcessIter { owner })),
        MirLoopSourceKind::ChannelReceiver => Ok(Box::new(ChannelIter { owner })),
        MirLoopSourceKind::EncodingReader { .. } => Ok(Box::new(EncodingIter { owner })),
        MirLoopSourceKind::Chars | MirLoopSourceKind::Iterable { .. } => Err(MirNativeCursorError::internal(
            "Source resource owner does not match checked loop source",
        )),
    }
}
fn runtime_error(
    type_name: impl Into<String>,
    fields: Vec<(String, MirRuntimeValue)>,
) -> MirNativeCursorError {
    MirNativeCursorError::from_runtime_value(MirRuntimeValue::Struct {
        type_name: type_name.into(),
        fields,
    })
}

fn mir_named(name: &str) -> jet_foundation::MIR::MirType {
    jet_foundation::MIR::MirType::from_kind(
        jet_foundation::MIR::MirTypeKind::Apply {
            name: jet_foundation::MIR::MirNominalRef::from_name(name),
            args: Vec::new(),
        },
    )
}

fn mir_absent_nominal(name: &str) -> MirRuntimeValue {
    MirRuntimeValue::Absent {
        element: mir_named(name),
    }
}

fn mir_absent_int() -> MirRuntimeValue {
    MirRuntimeValue::Absent {
        element: jet_foundation::MIR::MirType::from_kind(
            jet_foundation::MIR::MirTypeKind::Int,
        ),
    }
}

fn optional_int<E>(value: &Result<i64, E>) -> MirRuntimeValue {
    match value {
        Ok(value) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::Int(*value))),
        Err(_) => mir_absent_int(),
    }
}


fn mir_enum(type_name: &str, variant: &str) -> MirRuntimeValue {
    MirRuntimeValue::Enum {
        type_name: type_name.to_string(),
        variant: variant.to_string(),
        args: Vec::new(),
    }
}

fn encoding_format_value(
    format: crate::enc_stream::runtime::jet_std::EncodingFormat,
) -> MirRuntimeValue {
    mir_enum("EncodingFormat", format.as_str())
}

fn encoding_kind_value(
    kind: &crate::enc_stream::runtime::jet_std::EncodingErrorKind,
) -> MirRuntimeValue {
    use crate::enc_stream::runtime::jet_std::EncodingErrorKind;
    let variant = match kind {
        EncodingErrorKind::Syntax => "Syntax",
        EncodingErrorKind::Truncated => "Truncated",
        EncodingErrorKind::Unsupported => "Unsupported",
        EncodingErrorKind::Limit => "Limit",
        EncodingErrorKind::IO => "IO",
        EncodingErrorKind::State => "State",
    };
    mir_enum("EncodingErrorKind", variant)
}

fn encoding_error(
    error: &crate::enc_stream::runtime::jet_std::EncodingError,
) -> MirNativeCursorError {
    let cause = match &error.cause {
        Ok(cause) => MirRuntimeValue::Present(Box::new(MirRuntimeValue::Struct {
            type_name: "EncodingCause".to_string(),
            fields: vec![
                ("kind".to_string(), MirRuntimeValue::String(cause.kind.clone())),
                ("os_code".to_string(), optional_int(&cause.os_code)),
                (
                    "message".to_string(),
                    MirRuntimeValue::String(cause.message.clone()),
                ),
            ],
        })),
        Err(_) => mir_absent_nominal("EncodingCause"),
    };
    runtime_error(
        "EncodingError",
        vec![
            ("format".to_string(), encoding_format_value(error.format)),
            ("kind".to_string(), encoding_kind_value(&error.kind)),
            ("byte_offset".to_string(), MirRuntimeValue::Int(error.byte_offset)),
            ("line".to_string(), optional_int(&error.line)),
            ("column".to_string(), optional_int(&error.column)),
            ("path".to_string(), MirRuntimeValue::String(error.path.clone())),
            (
                "reason".to_string(),
                MirRuntimeValue::String(error.reason.clone()),
            ),
            ("cause".to_string(), cause),
        ],
    )
}


fn process_error(
    error: crate::ProcessPrelude::process_prelude::jet_std::IOError,
) -> MirNativeCursorError {
    MirNativeCursorError::from_runtime_value(crate::Process::mir_io_error(error))
}

fn file_error(error: crate::enc_stream::SourceIoError) -> MirNativeCursorError {
    MirNativeCursorError::from_runtime_value(crate::Process::mir_io_error(error))
}



type OwnerLease = Arc<Mutex<BackendOwner>>;

fn owner_lock(
    owner: &OwnerLease,
) -> Result<MutexGuard<'_, BackendOwner>, MirNativeCursorError> {
    owner
        .lock()
        .map_err(|_| MirNativeCursorError::internal("Source resource owner lock is poisoned"))
}

struct PlainIter {
    owner: OwnerLease,
}

impl Iterator for PlainIter {
    type Item = Result<MirRuntimeValue, MirNativeCursorError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut owner = match owner_lock(&self.owner) {
            Ok(owner) => owner,
            Err(error) => return Some(Err(error)),
        };
        match &mut *owner {
            BackendOwner::PlainStream(stream) => stream.pull_checked().map(Ok),
            _ => Some(Err(MirNativeCursorError::internal("plain stream owner payload mismatch"))),
        }
    }
}

struct FileIter {
    owner: OwnerLease,
}

impl Iterator for FileIter {
    type Item = Result<MirRuntimeValue, MirNativeCursorError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut owner = match owner_lock(&self.owner) {
            Ok(owner) => owner,
            Err(error) => return Some(Err(error)),
        };
        let BackendOwner::File(reader) = &mut *owner else {
            return Some(Err(MirNativeCursorError::internal("file owner payload mismatch")));
        };
        match crate::enc_stream::source_file_next_line(&mut reader.0) {
            Ok(Some(value)) => Some(Ok(MirRuntimeValue::String(value))),
            Ok(None) => None,
            Err(error) => Some(Err(file_error(error))),
        }
    }
}

struct StdinIter {
    owner: OwnerLease,
}

impl Iterator for StdinIter {
    type Item = Result<MirRuntimeValue, MirNativeCursorError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut owner = match owner_lock(&self.owner) {
            Ok(owner) => owner,
            Err(error) => return Some(Err(error)),
        };
        let BackendOwner::Stdin(reader) = &mut *owner else {
            return Some(Err(MirNativeCursorError::internal("stdin owner payload mismatch")));
        };
        match crate::enc_stream::source_stdin_next_line(reader) {
            Ok(Some(value)) => Some(Ok(MirRuntimeValue::String(value))),
            Ok(None) => None,
            Err(error) => Some(Err(process_error(error))),
        }
    }
}

struct ProcessIter {
    owner: OwnerLease,
}

impl Iterator for ProcessIter {
    type Item = Result<MirRuntimeValue, MirNativeCursorError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut owner = match owner_lock(&self.owner) {
            Ok(owner) => owner,
            Err(error) => return Some(Err(error)),
        };
        let BackendOwner::Process(reader) = &mut *owner else {
            return Some(Err(MirNativeCursorError::internal("process stream owner payload mismatch")));
        };
        match crate::ProcessPrelude::process_prelude::source_process_stream_next_line(&reader.0) {
            Ok(Some(value)) => Some(Ok(MirRuntimeValue::String(value))),
            Ok(None) => None,
            Err(error) => Some(Err(process_error(error))),
        }
    }
}

struct ChannelIter {
    owner: OwnerLease,
}

impl Iterator for ChannelIter {
    type Item = Result<MirRuntimeValue, MirNativeCursorError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut owner = match owner_lock(&self.owner) {
            Ok(owner) => owner,
            Err(error) => return Some(Err(error)),
        };
        let BackendOwner::ChannelReceiver(receiver) = &mut *owner else {
            return Some(Err(MirNativeCursorError::internal("channel owner payload mismatch")));
        };
        receiver.receive().map(Ok)
    }
}

struct EncodingIter {
    owner: OwnerLease,
}

impl Iterator for EncodingIter {
    type Item = Result<MirRuntimeValue, MirNativeCursorError>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut owner = match owner_lock(&self.owner) {
            Ok(owner) => owner,
            Err(error) => return Some(Err(error)),
        };
        let BackendOwner::Encoding(reader) = &mut *owner else {
            return Some(Err(MirNativeCursorError::internal("encoding owner payload mismatch")));
        };
        match encoding_next(&mut reader.0) {
            Ok(Some(value)) => Some(Ok(value)),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        }
    }
}

fn encoding_next(
    reader: &mut EncodingReader,
) -> Result<Option<MirRuntimeValue>, MirNativeCursorError> {
    match reader {
        EncodingReader::Json(reader) => crate::enc_stream::runtime::enc_json_reader_next(reader)
            .map(|value| value.map(data_event_value))
            .map_err(|error| encoding_error(&error)),
        EncodingReader::Jsonl(reader) => crate::enc_stream::runtime::enc_jsonl_reader_next(reader)
            .map(|value| value.map(data_tree_value))
            .map_err(|error| encoding_error(&error)),
        EncodingReader::Csv(reader) => crate::enc_stream::runtime::enc_csv_reader_next(reader)
            .map(|value| value.map(csv_row_value))
            .map_err(|error| encoding_error(&error)),
        EncodingReader::Xml(reader) => crate::enc_stream::runtime::enc_xml_reader_next(reader)
            .map(|value| value.map(data_tree_value))
            .map_err(|error| encoding_error(&error)),
        EncodingReader::Cbor(reader) => crate::enc_stream::runtime::enc_cbor_reader_next(reader)
            .map(|value| value.map(data_event_value))
            .map_err(|error| encoding_error(&error)),
    }
}

fn data_event_value(event: crate::enc_stream::runtime::jet_std::DataEvent) -> MirRuntimeValue {
    use crate::enc_stream::runtime::jet_std::DataEvent;
    let (variant, args) = match event {
        DataEvent::Null => ("Null", Vec::new()),
        DataEvent::Bool(value) => ("Bool", vec![MirRuntimeValue::Bool(value)]),
        DataEvent::Int(value) => ("Int", vec![MirRuntimeValue::Int(value)]),
        DataEvent::Float(value) => (
            "Float",
            vec![MirRuntimeValue::Float { value, f32: false }],
        ),
        DataEvent::Number(value) => ("Number", vec![MirRuntimeValue::String(value)]),
        DataEvent::Text(value) => ("Text", vec![MirRuntimeValue::String(value)]),
        DataEvent::Bytes(value) => ("Bytes", vec![MirRuntimeValue::Bytes(value)]),
        DataEvent::ArrayStart => ("ArrayStart", Vec::new()),
        DataEvent::ArrayEnd => ("ArrayEnd", Vec::new()),
        DataEvent::ObjectStart => ("ObjectStart", Vec::new()),
        DataEvent::Key(value) => ("Key", vec![MirRuntimeValue::String(value)]),
        DataEvent::ObjectEnd => ("ObjectEnd", Vec::new()),
    };
    MirRuntimeValue::Enum {
        type_name: "DataEvent".to_string(),
        variant: variant.to_string(),
        args: args.into_iter().map(|value| (None, value)).collect(),
    }
}

fn data_tree_value(tree: crate::enc_stream::runtime::jet_std::DataTree) -> MirRuntimeValue {
    use crate::enc_stream::runtime::jet_std::DataTree;
    let (variant, args) = match tree {
        DataTree::Null => ("Null", Vec::new()),
        DataTree::Bool(value) => ("Bool", vec![MirRuntimeValue::Bool(value)]),
        DataTree::Int(value) => ("Int", vec![MirRuntimeValue::Int(value)]),
        DataTree::Float(value) => (
            "Float",
            vec![MirRuntimeValue::Float { value, f32: false }],
        ),
        DataTree::Number(value) => ("Number", vec![MirRuntimeValue::String(value)]),
        DataTree::TypedText(value) => ("TypedText", vec![MirRuntimeValue::String(value)]),
        DataTree::Text(value) => ("Text", vec![MirRuntimeValue::String(value)]),
        DataTree::Bytes(value) => ("Bytes", vec![MirRuntimeValue::Bytes(value)]),
        DataTree::Array(values) => (
            "Array",
            vec![MirRuntimeValue::List(
                values.into_iter().map(data_tree_value).collect(),
            )],
        ),
        DataTree::Object(values) => (
            "Object",
            vec![MirRuntimeValue::List(
                values
                    .into_iter()
                    .map(|(key, value)| MirRuntimeValue::Struct {
                        type_name: "Tuple".to_string(),
                        fields: vec![
                            ("key".to_string(), MirRuntimeValue::String(key)),
                            ("value".to_string(), data_tree_value(value)),
                        ],
                    })
                    .collect(),
            )],
        ),
    };
    MirRuntimeValue::Enum {
        type_name: "DataTree".to_string(),
        variant: variant.to_string(),
        args: args.into_iter().map(|value| (None, value)).collect(),
    }
}

fn csv_row_value(row: crate::enc_stream::runtime::jet_std::CSVRow) -> MirRuntimeValue {
    MirRuntimeValue::Struct {
        type_name: "CSVRow".to_string(),
        fields: vec![
            (
                "fields".to_string(),
                MirRuntimeValue::List(
                    row.fields
                        .into_iter()
                        .map(MirRuntimeValue::String)
                        .collect(),
                ),
            ),
            ("line".to_string(), MirRuntimeValue::Int(row.line)),
        ],
    }
}

fn data_event_from_value(
    value: &MirRuntimeValue,
) -> Result<crate::enc_stream::runtime::jet_std::DataEvent, MirNativeCursorError> {
    use crate::enc_stream::runtime::jet_std::DataEvent;
    let MirRuntimeValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return Err(MirNativeCursorError::internal(
            "encoding writer expects a DataEvent carrier",
        ));
    };
    if type_name != "DataEvent" {
        return Err(MirNativeCursorError::internal(format!(
            "encoding writer expects DataEvent, got `{type_name}`"
        )));
    }
    let arg = |index: usize| {
        args.get(index)
            .map(|(_, value)| value)
            .ok_or_else(|| MirNativeCursorError::internal("DataEvent carrier is missing an argument"))
    };
    let unit = || {
        if args.is_empty() {
            Ok(())
        } else {
            Err(MirNativeCursorError::internal(
                "unit DataEvent variant has unexpected arguments",
            ))
        }
    };
    match variant.as_str() {
        "Null" => {
            unit()?;
            Ok(DataEvent::Null)
        }
        "ArrayStart" => {
            unit()?;
            Ok(DataEvent::ArrayStart)
        }
        "ArrayEnd" => {
            unit()?;
            Ok(DataEvent::ArrayEnd)
        }
        "ObjectStart" => {
            unit()?;
            Ok(DataEvent::ObjectStart)
        }
        "ObjectEnd" => {
            unit()?;
            Ok(DataEvent::ObjectEnd)
        }
        "Bool" => match arg(0)? {
            MirRuntimeValue::Bool(value) if args.len() == 1 => Ok(DataEvent::Bool(*value)),
            _ => Err(MirNativeCursorError::internal(
                "DataEvent.Bool carrier has the wrong payload",
            )),
        },
        "Int" => match arg(0)? {
            MirRuntimeValue::Int(value) if args.len() == 1 => Ok(DataEvent::Int(*value)),
            _ => Err(MirNativeCursorError::internal(
                "DataEvent.Int carrier has the wrong payload",
            )),
        },
        "Float" => match arg(0)? {
            MirRuntimeValue::Float { value, .. } if args.len() == 1 => {
                Ok(DataEvent::Float(*value))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataEvent.Float carrier has the wrong payload",
            )),
        },
        "Number" => match arg(0)? {
            MirRuntimeValue::String(value) if args.len() == 1 => {
                Ok(DataEvent::Number(value.clone()))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataEvent.Number carrier has the wrong payload",
            )),
        },
        "Text" => match arg(0)? {
            MirRuntimeValue::String(value) if args.len() == 1 => {
                Ok(DataEvent::Text(value.clone()))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataEvent.Text carrier has the wrong payload",
            )),
        },
        "Key" => match arg(0)? {
            MirRuntimeValue::String(value) if args.len() == 1 => {
                Ok(DataEvent::Key(value.clone()))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataEvent.Key carrier has the wrong payload",
            )),
        },
        "Bytes" => match arg(0)? {
            MirRuntimeValue::Bytes(value) if args.len() == 1 => {
                Ok(DataEvent::Bytes(value.clone()))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataEvent.Bytes carrier has the wrong payload",
            )),
        },
        _ => Err(MirNativeCursorError::internal(format!(
            "unknown DataEvent variant `{variant}`"
        ))),
    }
}

fn data_tree_from_value(
    value: &MirRuntimeValue,
) -> Result<crate::enc_stream::runtime::jet_std::DataTree, MirNativeCursorError> {
    use crate::enc_stream::runtime::jet_std::DataTree;
    let MirRuntimeValue::Enum {
        type_name,
        variant,
        args,
    } = value
    else {
        return Err(MirNativeCursorError::internal(
            "encoding writer expects a DataTree carrier",
        ));
    };
    if type_name != "DataTree" {
        return Err(MirNativeCursorError::internal(format!(
            "encoding writer expects DataTree, got `{type_name}`"
        )));
    }
    let arg = |index: usize| {
        args.get(index)
            .map(|(_, value)| value)
            .ok_or_else(|| MirNativeCursorError::internal("DataTree carrier is missing an argument"))
    };
    let unit = || {
        if args.is_empty() {
            Ok(())
        } else {
            Err(MirNativeCursorError::internal(
                "unit DataTree variant has unexpected arguments",
            ))
        }
    };
    match variant.as_str() {
        "Null" => {
            unit()?;
            Ok(DataTree::Null)
        }
        "Bool" => match arg(0)? {
            MirRuntimeValue::Bool(value) if args.len() == 1 => Ok(DataTree::Bool(*value)),
            _ => Err(MirNativeCursorError::internal(
                "DataTree.Bool carrier has the wrong payload",
            )),
        },
        "Int" => match arg(0)? {
            MirRuntimeValue::Int(value) if args.len() == 1 => Ok(DataTree::Int(*value)),
            _ => Err(MirNativeCursorError::internal(
                "DataTree.Int carrier has the wrong payload",
            )),
        },
        "Float" => match arg(0)? {
            MirRuntimeValue::Float { value, .. } if args.len() == 1 => {
                Ok(DataTree::Float(*value))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataTree.Float carrier has the wrong payload",
            )),
        },
        "Number" => match arg(0)? {
            MirRuntimeValue::String(value) if args.len() == 1 => {
                Ok(DataTree::Number(value.clone()))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataTree.Number carrier has the wrong payload",
            )),
        },
        "TypedText" => match arg(0)? {
            MirRuntimeValue::String(value) if args.len() == 1 => {
                Ok(DataTree::TypedText(value.clone()))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataTree.TypedText carrier has the wrong payload",
            )),
        },
        "Text" => match arg(0)? {
            MirRuntimeValue::String(value) if args.len() == 1 => {
                Ok(DataTree::Text(value.clone()))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataTree.Text carrier has the wrong payload",
            )),
        },
        "Bytes" => match arg(0)? {
            MirRuntimeValue::Bytes(value) if args.len() == 1 => {
                Ok(DataTree::Bytes(value.clone()))
            }
            _ => Err(MirNativeCursorError::internal(
                "DataTree.Bytes carrier has the wrong payload",
            )),
        },
        "Array" => {
            let MirRuntimeValue::List(values) = arg(0)? else {
                return Err(MirNativeCursorError::internal(
                    "DataTree.Array carrier has the wrong payload",
                ));
            };
            if args.len() != 1 {
                return Err(MirNativeCursorError::internal(
                    "DataTree.Array carrier has unexpected arguments",
                ));
            }
            values
                .iter()
                .map(data_tree_from_value)
                .collect::<Result<Vec<_>, _>>()
                .map(DataTree::Array)
        }
        "Object" => {
            let MirRuntimeValue::List(values) = arg(0)? else {
                return Err(MirNativeCursorError::internal(
                    "DataTree.Object carrier has the wrong payload",
                ));
            };
            if args.len() != 1 {
                return Err(MirNativeCursorError::internal(
                    "DataTree.Object carrier has unexpected arguments",
                ));
            }
            let mut entries = Vec::with_capacity(values.len());
            for value in values {
                let MirRuntimeValue::Struct { type_name, fields } = value else {
                    return Err(MirNativeCursorError::internal(
                        "DataTree.Object carrier contains a non-tuple entry",
                    ));
                };
                if type_name != "Tuple" {
                    return Err(MirNativeCursorError::internal(
                        "DataTree.Object carrier entry is not a Tuple",
                    ));
                }
                let key = named_field(fields, "key")?;
                let value = named_field(fields, "value")?;
                let MirRuntimeValue::String(key) = key else {
                    return Err(MirNativeCursorError::internal(
                        "DataTree.Object carrier key is not a String",
                    ));
                };
                entries.push((key.clone(), data_tree_from_value(value)?));
            }
            Ok(DataTree::Object(entries))
        }
        _ => Err(MirNativeCursorError::internal(format!(
            "unknown DataTree variant `{variant}`"
        ))),
    }
}

fn named_field<'a>(
    fields: &'a [(String, MirRuntimeValue)],
    name: &str,
) -> Result<&'a MirRuntimeValue, MirNativeCursorError> {
    fields
        .iter()
        .find_map(|(field, value)| (field == name).then_some(value))
        .ok_or_else(|| {
            MirNativeCursorError::internal(format!("carrier is missing `{name}` field"))
        })
}

fn csv_row_from_value(value: &MirRuntimeValue) -> Result<Vec<String>, MirNativeCursorError> {
    let fields = match value {
        MirRuntimeValue::List(values) => values,
        MirRuntimeValue::Struct { type_name, fields } if type_name == "CSVRow" => {
            match named_field(fields, "fields")? {
                MirRuntimeValue::List(values) => values,
                _ => {
                    return Err(MirNativeCursorError::internal(
                        "CSVRow.fields is not a list",
                    ))
                }
            }
        }
        _ => {
            return Err(MirNativeCursorError::internal(
                "CSV writer expects a CSVRow or string list",
            ))
        }
    };
    fields
        .iter()
        .map(|value| match value {
            MirRuntimeValue::String(value) => Ok(value.clone()),
            _ => Err(MirNativeCursorError::internal(
                "CSV writer row contains a non-string field",
            )),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::{SourceDeoptError, SourceExecutionCompletionDisposition};
    use crate::enc_stream::runtime::jet_std::{
        EncodingCause, EncodingError, EncodingErrorKind, EncodingFormat,
    };
    use crate::ProcessPrelude::process_prelude::jet_std::{
        IOContext, IOError, IOOperation, ProcessResourceLimit,
    };
    use jet_foundation::Outcome::JetAbsent;
    use std::path::PathBuf;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::sync::{Arc, Mutex};
    use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

    fn test_handle(value: u64) -> MirHandleId {
        MirHandleId(value)
    }

    fn test_path(label: &str) -> PathBuf {
        static NEXT_PATH: AtomicU32 = AtomicU32::new(0);
        let suffix = NEXT_PATH.fetch_add(1, Ordering::Relaxed);
        let root = std::env::var_os("JET_SCRATCH")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .map(|home| home.join(".cache").join("jet-luna"))
            })
            .expect("JET_SCRATCH or HOME must select the Jet scratch directory");
        std::fs::create_dir_all(&root).expect("create Jet scratch directory");
        root.join(format!(
            "source-resources-{label}-{}-{suffix}.tmp",
            std::process::id()
        ))
    }

    fn test_channel_arena() -> SourceResourceArena {
        SourceResourceArena::new_with_finalizer(
            Arc::new(|finalization| {
                finalization.with_live_scope(|_, _, values, _completions| {
                    if values.is_empty() {
                        Ok(())
                    } else {
                        Err("channel test did not consume its queued values".to_string())
                    }
                })
            }),
            Arc::new(|error| panic!("channel test finalization failed: {error:?}")),
        )
    }

    fn test_shared_interop_session() -> SourceResourceSession {
        SourceResourceSession::new_with_finalizer(
            Arc::new(|finalization| {
                finalization.with_live_scope(|_, _, values, _completions| {
                    if values.is_empty() {
                        Ok(())
                    } else {
                        Err("Shared interop test did not consume queued values".to_string())
                    }
                })
            }),
            Arc::new(|outcome| panic!("unexpected automatic Shared finalization: {outcome:?}")),
        )
    }

    fn test_completion(id: i64) -> SourceExecutionCompletion {
        SourceExecutionCompletion {
            execution: None,
            function: None,
            lease: None,
            disposition: SourceExecutionCompletionDisposition::NotInvoked {
                cause: SourceDeoptError::Backend(format!("completion-{id}")),
                entry_values: vec![MirRuntimeValue::Int(id)],
                write_borrow_indices: vec![id as usize],
                completions: Vec::new(),
            },
        }
    }

    fn test_nested_completion() -> SourceExecutionCompletion {
        SourceExecutionCompletion {
            execution: None,
            function: None,
            lease: None,
            disposition: SourceExecutionCompletionDisposition::NotInvoked {
                cause: SourceDeoptError::Backend("completion-1".to_string()),
                entry_values: vec![MirRuntimeValue::Int(1)],
                write_borrow_indices: vec![1],
                completions: vec![test_completion(2)],
            },
        }
    }

    fn assert_completion_receipt(
        completion: &SourceExecutionCompletion,
        id: i64,
    ) -> &[SourceExecutionCompletion] {
        assert!(completion.execution.is_none());
        assert!(completion.function.is_none());
        assert!(completion.lease.is_none());
        match &completion.disposition {
            SourceExecutionCompletionDisposition::NotInvoked {
                cause,
                entry_values,
                write_borrow_indices,
                completions,
            } => {
                assert_eq!(
                    cause,
                    &SourceDeoptError::Backend(format!("completion-{id}"))
                );
                assert_eq!(entry_values, &[MirRuntimeValue::Int(id)]);
                assert_eq!(write_borrow_indices, &[id as usize]);
                completions
            }
            SourceExecutionCompletionDisposition::Invoked { .. } => {
                panic!("expected the exact not-invoked completion receipt")
            }
        }
    }

    fn assert_nested_completion(completion: &SourceExecutionCompletion) {
        let nested = assert_completion_receipt(completion, 1);
        assert_eq!(nested.len(), 1);
        assert!(assert_completion_receipt(&nested[0], 2).is_empty());
    }

    #[test]
    fn last_root_retires_empty_no_handler_session() {
        let session = SourceResourceSession::new();
        let arena = session.arena();
        let root = session.retain_root().expect("retain Source session root");

        session.retire().expect("defer retirement while root is live");
        assert!(!arena.is_retired().expect("root keeps arena live"));

        drop(root);
        assert!(arena.is_retired().expect("last root completes retirement"));
    }

    #[test]
    fn dropping_session_before_last_root_delivers_success_to_live_handler() {
        let outcomes = Arc::new(Mutex::new(Vec::new()));
        let handler_target = Arc::downgrade(&outcomes);
        let session = SourceResourceSession::new_with_finalizer(
            Arc::new(|finalization| {
                finalization.with_live_scope(|_, _, values, completions| {
                    if values.len() != 1 || values[0] != MirRuntimeValue::Int(81) {
                        return Err("success finalizer received the wrong Source value".to_string());
                    }
                    values.clear();
                    completions.push(test_nested_completion());
                    Ok(())
                })
            }),
            Arc::new(move |outcome| {
                if let Some(target) = handler_target.upgrade() {
                    target
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(outcome);
                }
            }),
        );
        let root = session.retain_root().expect("retain Source session root");
        let arena = root.arena();
        let (sender, _) = arena
            .register_new_channel(test_handle(1140), test_handle(1141), None)
            .expect("register Source cleanup receiver");
        arena
            .channel_sender_send(sender.handle, sender.raw, MirRuntimeValue::Int(81))
            .expect("queue Source cleanup value");
        assert!(session
            .retire()
            .expect("defer retirement while the root is live")
            .is_empty());

        drop(session);
        drop(root);

        assert!(arena.is_retired().expect("successful cleanup retires arena"));
        let mut outcomes = outcomes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(outcomes.len(), 1);
        match outcomes.pop().expect("one last-root outcome") {
            SourceResourceFinalizationOutcome::Retired { completions } => {
                assert_eq!(completions.len(), 1);
                assert_nested_completion(&completions[0]);
            }
            SourceResourceFinalizationOutcome::Failed(error) => {
                panic!("successful last-root cleanup failed: {error:?}");
            }
        }
    }

    #[test]
    fn dropping_session_before_last_root_delivers_failure_to_live_handler() {
        let outcomes = Arc::new(Mutex::new(Vec::new()));
        let handler_target = Arc::downgrade(&outcomes);
        let attempts = Arc::new(AtomicUsize::new(0));
        let finalizer_attempts = attempts.clone();
        let session = SourceResourceSession::new_with_finalizer(
            Arc::new(move |finalization| {
                let first_attempt = finalizer_attempts.fetch_add(1, Ordering::SeqCst) == 0;
                finalization.with_live_scope(move |_, _, values, completions| {
                    if values.len() != 1 || values[0] != MirRuntimeValue::Int(82) {
                        return Err("failure finalizer received the wrong Source value".to_string());
                    }
                    if first_attempt {
                        completions.push(test_nested_completion());
                        Err("Source cleanup retained its owned carrier".to_string())
                    } else {
                        values.clear();
                        Ok(())
                    }
                })
            }),
            Arc::new(move |outcome| {
                if let Some(target) = handler_target.upgrade() {
                    target
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(outcome);
                }
            }),
        );
        let root = session.retain_root().expect("retain Source session root");
        let arena = root.arena();
        let (sender, _) = arena
            .register_new_channel(test_handle(1150), test_handle(1151), None)
            .expect("register Source cleanup receiver");
        arena
            .channel_sender_send(sender.handle, sender.raw, MirRuntimeValue::Int(82))
            .expect("queue Source cleanup value");
        assert!(session
            .retire()
            .expect("defer retirement while the root is live")
            .is_empty());

        drop(session);
        drop(root);

        assert!(!arena.is_retired().expect("failed cleanup remains retryable"));
        let mut outcomes = outcomes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(outcomes.len(), 1);
        let failure = match outcomes.pop().expect("one last-root outcome") {
            SourceResourceFinalizationOutcome::Failed(error) => {
                assert_eq!(error.error, "Source cleanup retained its owned carrier");
                assert_eq!(error.values, vec![MirRuntimeValue::Int(82)]);
                assert_eq!(error.completions.len(), 1);
                assert_nested_completion(&error.completions[0]);
                assert!(error.retryable);
                error
            }
            SourceResourceFinalizationOutcome::Retired { .. } => {
                panic!("failed last-root cleanup was reported as retired");
            }
        };
        drop(outcomes);

        assert!(!arena.is_retired().expect("failed cleanup remains retryable"));
        let completions = arena
            .retry_retire(failure)
            .expect("retry exact full outcome delivered to handler");
        assert_eq!(completions.len(), 1);
        assert_nested_completion(&completions[0]);
        assert_eq!(attempts.load(Ordering::Acquire), 2);
        assert!(arena.is_retired().expect("retry consumes retained carrier"));
    }

    #[test]
    fn no_handler_admits_physical_shared_roots_but_rejects_source_alias_debt() {
        let session = SourceResourceSession::new();
        let arena = session.arena();
        let owner_count = Arc::new(AtomicUsize::new(1));
        let count_for_count = owner_count.clone();
        let count_for_retain = owner_count.clone();
        let root = crate::SourceSharedInterop::SourceSharedInterop::from_value(
            0xfeed_01,
            MirRuntimeValue::Int(1),
        )
        .with_owner_alias_lifecycle(
            move || Ok(count_for_count.load(Ordering::Acquire)),
            move || {
                reserve_test_shared_alias(&count_for_retain, 0x501)?
                    .ok_or_else(|| "test Shared owner has no live aliases".to_string())
            },
        );
        let physical_root = arena
            .register_shared_interop_root(0xfeed_01, &root)
            .expect("bare physical Shared root has no Source alias debt");
        assert_eq!(owner_count.load(Ordering::Acquire), 1);
        assert_eq!(
            arena.retain_shared_interop_alias(&physical_root),
            Err("Source Shared aliases require an origin outcome handler".to_string())
        );
        assert_eq!(owner_count.load(Ordering::Acquire), 1);

        let alias = root
            .retain_owner_alias()
            .expect("reserve test Source Shared alias");
        let alias_root = root
            .clone()
            .with_owner_alias_lease(alias)
            .expect("attach Source alias to physical root");
        let alias_token = alias_root
            .owner_alias_token_id()
            .expect("attached Source alias token");
        assert_eq!(owner_count.load(Ordering::Acquire), 2);
        assert_eq!(
            arena.register_shared_interop_root(0xfeed_01, &alias_root),
            Err("Source Shared aliases require an origin outcome handler before root registration"
                .to_string())
        );
        assert_eq!(alias_root.owner_alias_token_id(), Some(alias_token));
        assert_eq!(owner_count.load(Ordering::Acquire), 2);
        let released = alias_root.release_owner_alias();
        assert!(released.result.is_ok());
        assert!(released.completion.is_none());
        assert_eq!(owner_count.load(Ordering::Acquire), 1);

        let root_lease = session.retain_root().expect("retain no-handler session root");
        assert!(session
            .retire()
            .expect("defer no-work retirement while root is live")
            .is_empty());
        drop(root);
        drop(alias_root);
        drop(session);
        drop(root_lease);
        assert!(arena
            .is_retired()
            .expect("no-handler physical root finishes without Source work"));
    }

    #[test]
    fn no_handler_rejects_source_receiver_without_adopting_it() {
        let session = SourceResourceSession::new();
        let arena = session.arena();
        let receiver = jet_codegen::scheduler::JetSchedulerChannel::new();
        let error = arena
            .register_channel_receiver(test_handle(1160), &receiver)
            .expect_err("Source cleanup receiver requires an origin outcome owner");
        assert_eq!(
            error,
            "Source channel receiver requires an origin-owner finalizer and completion handler"
        );

        let sender = receiver.sender();
        assert!(sender.send(MirRuntimeValue::Int(83)));
        assert_eq!(receiver.try_receive(), Some(MirRuntimeValue::Int(83)));
        session.retire().expect("rejected receiver was not adopted");
        assert!(arena.is_retired().expect("empty no-handler arena retires"));
    }

    #[test]
    fn finalizer_returns_full_nested_receipt_without_active_scope() {
        let pending = Arc::new(Mutex::new(Some(test_nested_completion())));
        let completion = pending.clone();
        let session = SourceResourceSession::new_with_finalizer(
            Arc::new(move |finalization| {
                let packet = completion
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take()
                    .expect("one queued value owns one completion packet");
                finalization.with_live_scope(move |_, _, values, completions| {
                    values.clear();
                    completions.push(packet);
                    Ok(())
                })
            }),
            Arc::new(|outcome| panic!("unexpected automatic finalization: {outcome:?}")),
        );
        let arena = session.arena();
        let (sender, _) = arena
            .register_new_channel(test_handle(1100), test_handle(1101), None)
            .expect("register completion receiver");
        arena
            .channel_sender_send(sender.handle, sender.raw, MirRuntimeValue::Int(1))
            .expect("queue Source finalization value");

        let completions = session.retire().expect("retire and return exact receipt");
        assert_eq!(completions.len(), 1);
        assert_nested_completion(&completions[0]);
    }

    #[test]
    fn finalizer_routes_full_receipt_into_the_current_execution_scope() {
        let pending = Arc::new(Mutex::new(Some(test_nested_completion())));
        let completion = pending.clone();
        let session = SourceResourceSession::new_with_finalizer(
            Arc::new(move |finalization| {
                let packet = completion
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take()
                    .expect("one queued value owns one completion packet");
                finalization.with_live_scope(move |_, _, values, completions| {
                    values.clear();
                    completions.push(packet);
                    Ok(())
                })
            }),
            Arc::new(|outcome| panic!("unexpected automatic finalization: {outcome:?}")),
        );
        let arena = session.arena();
        let (sender, _) = arena
            .register_new_channel(test_handle(1110), test_handle(1111), None)
            .expect("register completion receiver");
        arena
            .channel_sender_send(sender.handle, sender.raw, MirRuntimeValue::Int(1))
            .expect("queue Source finalization value");

        let scope = SourceExecutionCompletionScope::new();
        let returned = scope.with_current(|| session.retire().expect("retire in current scope"));
        assert!(returned.is_empty());
        let completions = scope.drain();
        assert_eq!(completions.len(), 1);
        assert_nested_completion(&completions[0]);
    }

    #[test]
    fn failed_receiver_cleanup_preserves_completion_order() {
        let session = SourceResourceSession::new_with_finalizer(
            Arc::new(|finalization| {
                finalization.with_live_scope(|_, _, values, completions| {
                    let MirRuntimeValue::Int(id) =
                        values.first().expect("receiver cleanup owns one value")
                    else {
                        panic!("test receiver value is an integer")
                    };
                    let id = *id;
                    completions.push(test_completion(id));
                    if id == 1 {
                        values.clear();
                        Ok(())
                    } else {
                        Err("receiver cleanup failed after its receipt".to_string())
                    }
                })
            }),
            Arc::new(|outcome| panic!("unexpected automatic finalization: {outcome:?}")),
        );
        let arena = session.arena();
        let (sender, _) = arena
            .register_new_channel(test_handle(1130), test_handle(1131), None)
            .expect("register ordered-completion receiver");
        arena
            .channel_sender_send(sender.handle, sender.raw, MirRuntimeValue::Int(1))
            .expect("queue first receiver value");
        arena
            .channel_sender_send(sender.handle, sender.raw, MirRuntimeValue::Int(2))
            .expect("queue second receiver value");

        let failure = session
            .retire()
            .expect_err("second receiver cleanup fails");
        assert_eq!(failure.values, vec![MirRuntimeValue::Int(2)]);
        assert_eq!(failure.completions.len(), 2);
        assert!(assert_completion_receipt(&failure.completions[0], 1).is_empty());
        assert!(assert_completion_receipt(&failure.completions[1], 2).is_empty());
    }

    #[test]
    fn failed_finalization_preserves_partial_receipts_through_retry() {
        let attempts = Arc::new(AtomicUsize::new(0));
        let finalizer_attempts = attempts.clone();
        let session = SourceResourceSession::new_with_finalizer(
            Arc::new(move |finalization| {
                let first_attempt = finalizer_attempts.fetch_add(1, Ordering::SeqCst) == 0;
                finalization.with_live_scope(move |_, _, values, completions| {
                    completions.push(if first_attempt {
                        test_nested_completion()
                    } else {
                        test_completion(3)
                    });
                    if first_attempt {
                        Err("Source cleanup failed after producing a receipt".to_string())
                    } else {
                        values.clear();
                        Ok(())
                    }
                })
            }),
            Arc::new(|outcome| panic!("unexpected automatic finalization: {outcome:?}")),
        );
        let arena = session.arena();
        let (sender, _) = arena
            .register_new_channel(test_handle(1120), test_handle(1121), None)
            .expect("register retry receiver");
        arena
            .channel_sender_send(sender.handle, sender.raw, MirRuntimeValue::Int(77))
            .expect("queue retry value");

        let failure = session.retire().expect_err("first finalization fails");
        assert_eq!(
            failure.error,
            "Source cleanup failed after producing a receipt"
        );
        assert_eq!(failure.values, vec![MirRuntimeValue::Int(77)]);
        assert_eq!(failure.completions.len(), 1);
        assert_nested_completion(&failure.completions[0]);

        let completions = session
            .retry_retire(failure)
            .expect("retry full retirement packet");
        assert_eq!(completions.len(), 2);
        assert_nested_completion(&completions[0]);
        assert!(assert_completion_receipt(&completions[1], 3).is_empty());
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert!(arena.is_retired().expect("retry completes retirement"));
    }

    fn test_native_binding(artifact_id: u64, scope: &str) -> SourceNativeBinding {
        use jet_foundation::MIR::{
            MirArtifactBuildMode, MirArtifactIdentity, MirArtifactId, MirArtifactKind,
            MirArtifactTarget, MirExecutionIdentity, MirProgramIdentity, MIR_SCHEMA_VERSION,
        };

        let artifact = MirArtifactId(artifact_id);
        let execution = MirExecutionIdentity {
            schema_version: 1,
            artifact: MirArtifactIdentity {
                schema_version: 1,
                mir_schema_version: MIR_SCHEMA_VERSION,
                program_digest: [0; 32],
                package_identity: "source-resource-test".to_string(),
                artifact,
                name: scope.to_string(),
                kind: MirArtifactKind::NativeExecutable,
                target: MirArtifactTarget::Cranelift,
                mode: MirArtifactBuildMode::Dev,
                provider_identity: String::new(),
                closure_identity: String::new(),
                artifact_identity: String::new(),
                program_identity: MirProgramIdentity::unavailable(String::new(), Vec::new()),
            },
        };
        let bindings = crate::SourceInterfaces::NativeInterfaceBindings::new();
        let object = bindings
            .create_object(scope.to_string())
            .expect("create native test interface");
        SourceNativeBinding::interface(
            &object,
            execution,
            artifact,
            MirType::from_kind(MirTypeKind::Int),
        )
        .expect("construct checked native binding")
    }

    #[test]
    fn active_source_resource_lease_is_scoped_and_restored() {
        let session = SourceResourceSession::new();
        let lease = session.retain_root().expect("retain Source callback root");
        let arena = lease.arena();

        with_active_source_resource_lease(|active| assert!(active.is_none()));
        let activation = lease.activate();
        with_active_source_resource_lease(|active| {
            let active = active.expect("lease activation is visible");
            assert!(Arc::ptr_eq(&active.inner, &lease.inner));

            let nested = active.activate();
            with_active_source_resource_lease(|nested_active| {
                assert!(Arc::ptr_eq(
                    &nested_active.expect("nested activation is visible").inner,
                    &lease.inner
                ));
            });
            drop(nested);
        });

        let unwind = catch_unwind(AssertUnwindSafe(|| {
            with_active_source_resource_lease(|active| {
                let _nested = active
                    .expect("lease activation is visible while unwinding")
                    .activate();
                panic!("nested active lease accessor");
            });
        }));
        assert!(unwind.is_err());
        with_active_source_resource_lease(|active| {
            assert!(Arc::ptr_eq(
                &active.expect("outer lease activation is restored after unwind").inner,
                &lease.inner
            ));
        });
        with_active_source_resource_arena(|active_arena| {
            let active_arena = active_arena.expect("arena activation is visible");
            assert!(Arc::ptr_eq(&active_arena.state, &arena.state));

            let nested = activate_source_resource_arena(active_arena);
            with_active_source_resource_arena(|nested_arena| {
                assert!(Arc::ptr_eq(
                    &nested_arena.expect("nested arena activation is visible").state,
                    &arena.state
                ));
            });
            drop(nested);
        });

        let arena_unwind = catch_unwind(AssertUnwindSafe(|| {
            with_active_source_resource_arena(|active_arena| {
                let _nested = activate_source_resource_arena(
                    active_arena.expect("arena activation is visible while unwinding"),
                );
                panic!("nested active arena accessor");
            });
        }));
        assert!(arena_unwind.is_err());
        with_active_source_resource_lease(|active| {
            assert!(Arc::ptr_eq(
                &active.expect("lease activation is restored after arena unwind").inner,
                &lease.inner
            ));
        });

        let nested = activate_source_resource_arena(&arena);
        with_active_source_resource_lease(|active| assert!(active.is_none()));
        drop(nested);
        with_active_source_resource_lease(|active| {
            assert!(Arc::ptr_eq(
                &active.expect("outer lease activation is restored").inner,
                &lease.inner
            ));
        });

        drop(activation);
        with_active_source_resource_lease(|active| assert!(active.is_none()));
        drop(lease);
        session.retire().expect("retire empty Source session");
    }

    #[test]
    fn weak_source_resource_lease_alias_does_not_mint_a_root() {
        let session = SourceResourceSession::new();
        let lease = session.retain_root().expect("retain Source callback root");
        let weak = lease.downgrade();

        session.retire().expect("defer retirement while root is live");
        assert_eq!(session.retained_root_count().unwrap(), 1);
        let alias = weak.upgrade().expect("existing counted lease is live");
        assert!(Arc::ptr_eq(&alias.inner, &lease.inner));
        assert_eq!(session.retained_root_count().unwrap(), 1);
        drop(alias);

        drop(lease);
        assert_eq!(session.retained_root_count().unwrap(), 0);
        assert!(session.arena().is_retired().unwrap());
        assert!(weak.upgrade().is_none());
    }

    struct TestSharedInteropAliasLease {
        owner_count: Arc<AtomicUsize>,
        token_id: i64,
        completion: Mutex<Option<crate::Memory::shared_protocol::JetSharedPhysicalCompletion>>,
        result_error: Option<String>,
    }

    impl crate::SourceSharedInterop::SourceSharedInteropOwnerAliasLease
        for TestSharedInteropAliasLease
    {
        fn token_id(&self) -> i64 {
            self.token_id
        }

        fn release(
            mut self: Box<Self>,
        ) -> crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome<()> {
            let completion = self
                .completion
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take();
            let result = self.result_error.take().map_or(Ok(()), Err);
            crate::Memory::shared_protocol::JetSharedPhysicalOperationOutcome::new(
                result,
                completion,
            )
        }
    }

    impl Drop for TestSharedInteropAliasLease {
        fn drop(&mut self) {
            let previous = self.owner_count.fetch_sub(1, Ordering::AcqRel);
            assert!(previous > 0, "test Source Shared alias count underflowed");
        }
    }

    fn reserve_test_shared_alias(
        owner_count: &Arc<AtomicUsize>,
        token_id: i64,
    ) -> Result<Option<Box<dyn crate::SourceSharedInterop::SourceSharedInteropOwnerAliasLease>>, String>
    {
        reserve_test_shared_alias_with_completion(owner_count, token_id, None, None)
    }

    fn reserve_test_shared_alias_with_completion(
        owner_count: &Arc<AtomicUsize>,
        token_id: i64,
        completion: Option<crate::Memory::shared_protocol::JetSharedPhysicalCompletion>,
        result_error: Option<String>,
    ) -> Result<Option<Box<dyn crate::SourceSharedInterop::SourceSharedInteropOwnerAliasLease>>, String>
    {
        let mut current = owner_count.load(Ordering::Acquire);
        loop {
            if current == 0 {
                return Ok(None);
            }
            let next = current
                .checked_add(1)
                .ok_or_else(|| "test Source Shared alias count exhausted".to_string())?;
            match owner_count.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Ok(Some(Box::new(TestSharedInteropAliasLease {
                        owner_count: owner_count.clone(),
                        token_id,
                        completion: Mutex::new(completion),
                        result_error,
                    })))
                }
                Err(observed) => current = observed,
            }
        }
    }

    fn release_test_shared_alias(
        arena: &SourceResourceArena,
        capability: &SourceResourceHandle,
        token_id: i64,
    ) {
        let outcome = arena
            .release_shared_interop_alias(capability, token_id)
            .expect("release pending test Shared alias");
        assert!(outcome.result.is_ok());
        assert!(outcome.completion.is_none());
    }

    struct TestSharedInteropWeakOwner {
        owner_slot:
            std::sync::Weak<Mutex<Option<crate::SourceSharedInterop::SourceSharedInterop>>>,
        owner_count: Arc<AtomicUsize>,
        next_token_id: Arc<AtomicU32>,
    }

    impl crate::SourceSharedInterop::SourceSharedInteropWeakOwner for TestSharedInteropWeakOwner {
        fn upgrade(
            &self,
        ) -> Result<
            Option<(
                crate::SourceSharedInterop::SourceSharedInterop,
                Box<dyn crate::SourceSharedInterop::SourceSharedInteropOwnerAliasLease>,
            )>,
            String,
        > {
            let Some(owner) = self.owner_slot.upgrade() else {
                return Ok(None);
            };
            let root = owner
                .lock()
                .map_err(|_| "test Shared owner slot is poisoned".to_string())?
                .as_ref()
                .cloned();
            let Some(root) = root else {
                return Ok(None);
            };
            let token_id = i64::from(self.next_token_id.fetch_add(1, Ordering::AcqRel));
            let Some(alias) = reserve_test_shared_alias(&self.owner_count, token_id)? else {
                return Ok(None);
            };
            Ok(Some((root, alias)))
        }
    }

    struct ReentrantLeaseDrop {
        lease: Option<SourceResourceLease>,
        arena: SourceResourceArena,
        reentered_after_release: Arc<AtomicUsize>,
    }

    impl Drop for ReentrantLeaseDrop {
        fn drop(&mut self) {
            drop(self.lease.take());
            if self.arena.is_retired().unwrap_or(false) {
                self.reentered_after_release.fetch_add(1, Ordering::SeqCst);
            }
        }
    }

    fn value_of(error: MirNativeCursorError) -> MirRuntimeValue {
        match error {
            MirNativeCursorError::Value(value) => *value,
            MirNativeCursorError::Internal(message) => {
                panic!("expected a checked value error, got adapter failure: {message}")
            }
        }
    }

    #[test]
    fn source_file_iterator_is_lazy_and_owner_is_typed() {
        let path = test_path("lazy");
        std::fs::write(&path, b"").expect("create empty source file");
        let arena = SourceResourceArena::new();
        let capability = arena
            .open_file(test_handle(1), path.to_str().expect("utf8 scratch path"))
            .expect("admit typed reader");
        let key = capability.native_loop_key().expect("file loop key");
        let mut iterator = arena
            .lines_file(&key, false)
            .result
            .expect("borrow typed file owner");

        // No read occurs during cursor initialization.  Data written after
        // admission is visible to the first pull.
        std::fs::write(&path, b"late\n").expect("publish source data");
        assert_eq!(
            iterator.next(),
            Some(Ok(MirRuntimeValue::String("late".to_string())))
        );
        assert_eq!(iterator.next(), None);
        let _ = arena.release_capability(&capability);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn borrowed_file_iterator_survives_release_and_moved_close() {
        let path = test_path("lease");
        std::fs::write(&path, b"borrowed\nmoved\n").expect("create source file");
        let arena = SourceResourceArena::new();
        let capability = arena
            .open_file(test_handle(2), path.to_str().expect("utf8 scratch path"))
            .expect("admit typed reader");
        let key = capability.native_loop_key().expect("file loop key");
        let mut borrowed = arena
            .lines_file(&key, false)
            .result
            .expect("borrow typed file owner");
        arena
            .release_capability(&capability)
            .expect("release source slot while lease is live");
        assert_eq!(
            borrowed.next(),
            Some(Ok(MirRuntimeValue::String("borrowed".to_string())))
        );

        let moved_capability = arena
            .open_file(test_handle(3), path.to_str().expect("utf8 scratch path"))
            .expect("admit second typed reader");
        let moved_key = moved_capability.native_loop_key().expect("file loop key");
        let mut moved = arena
            .lines_file(&moved_key, true)
            .result
            .expect("move typed file owner");
        assert!(arena.release_capability(&moved_capability).is_err());
        assert_eq!(
            moved.next(),
            Some(Ok(MirRuntimeValue::String("borrowed".to_string())))
        );
        drop(borrowed);
        drop(moved);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn source_file_writer_is_a_typed_release_lease() {
        let path = test_path("writer");
        let arena = SourceResourceArena::new();
        let capability = arena
            .open_file_writer(test_handle(6), path.to_str().expect("utf8 scratch path"))
            .expect("admit typed writer");
        assert_eq!(capability.kind, SourceResourceKind::FileWriter);
        assert!(capability.native_loop_key().is_err());
        let line = "writer line".to_string();
        arena
            .file_writer_write_line(capability.handle, capability.raw, &line)
            .expect("write through typed writer lease");
        arena
            .file_writer_flush(capability.handle, capability.raw)
            .expect("flush through typed writer lease");
        assert_eq!(
            arena
                .file_writer_path(capability.handle, capability.raw)
                .expect("writer path"),
            path.to_str().expect("utf8 scratch path")
        );
        assert_eq!(
            std::fs::read_to_string(&path).expect("read writer output"),
            "writer line\n"
        );
        arena
            .release_resource(capability.handle, capability.raw)
            .expect("release typed writer");
        assert!(arena.release_resource(capability.handle, capability.raw).is_err());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn source_capability_lookup_returns_stored_generation_and_kind() {
        let arena = SourceResourceArena::new();
        let capability = arena.register_stdin(test_handle(11)).expect("stdin owner");
        let looked_up = arena
            .lookup_capability(capability.handle, capability.raw)
            .expect("lookup canonical source capability");
        assert_eq!(looked_up, capability);
        assert!(arena
            .lookup_capability(test_handle(12), capability.raw)
            .is_err());
        arena
            .release_resource(capability.handle, capability.raw)
            .expect("release stdin owner");
    }


    #[test]
    fn taking_file_reader_rolls_back_when_cursor_aliases_owner() {
        let path = test_path("take-reader");

        std::fs::write(&path, b"line\n").expect("create source file");
        let arena = SourceResourceArena::new();
        let capability = arena
            .open_file(test_handle(13), path.to_str().expect("utf8 scratch path"))
            .expect("admit typed reader");
        let key = capability.native_loop_key().expect("file loop key");
        let borrowed = arena
            .lines_file(&key, false)
            .result
            .expect("borrow file owner");
        assert!(arena
            .take_file_reader(capability.handle, capability.raw)
            .is_err());
        assert_eq!(
            arena
                .lookup_capability(capability.handle, capability.raw)
                .expect("rolled back file capability")
                .kind,
            SourceResourceKind::LinesFile
        );
        drop(borrowed);
        let reader = arena
            .take_file_reader(capability.handle, capability.raw)
            .expect("consume unaliased file owner");
        assert!(arena
            .lookup_capability(capability.handle, capability.raw)
            .is_err());
        drop(reader);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn taking_file_writer_rolls_back_when_owner_is_aliased() {
        let path = test_path("take-writer");
        let arena = SourceResourceArena::new();
        let capability = arena
            .open_file_writer(test_handle(20), path.to_str().expect("utf8 scratch path"))
            .expect("admit file writer");
        let alias = arena
            .owner_for_raw(
                capability.handle,
                capability.raw,
                &SourceResourceKind::FileWriter,
            )
            .expect("borrow writer owner");
        assert!(arena
            .take_file_writer(capability.handle, capability.raw)
            .is_err());
        assert_eq!(
            arena
                .lookup_capability(capability.handle, capability.raw)
                .expect("rolled back writer capability")
                .kind,
            SourceResourceKind::FileWriter
        );
        drop(alias);
        let writer = arena
            .take_file_writer(capability.handle, capability.raw)
            .expect("consume unaliased file writer");
        drop(writer);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn canonical_channel_endpoints_keep_sender_and_receiver_leases_distinct() {
        let arena = test_channel_arena();
        let (sender, receiver) = arena
            .register_new_channel(test_handle(16), test_handle(17), Some(1))
            .expect("admit channel endpoints");
        assert_eq!(sender.kind, SourceResourceKind::ChannelSender);
        assert_eq!(receiver.kind, SourceResourceKind::ChannelReceiver);
        arena
            .channel_sender_send(sender.handle, sender.raw, MirRuntimeValue::Int(9))
            .expect("send channel value");
        assert_eq!(
            arena
                .channel_receiver_try_receive(receiver.handle, receiver.raw)
                .expect("receive channel value"),
            Some(MirRuntimeValue::Int(9))
        );
        arena
            .channel_sender_close(sender.handle, sender.raw)
            .expect("close channel sender");
        assert_eq!(
            arena
                .channel_receiver_try_receive(receiver.handle, receiver.raw)
                .expect("observe closed receiver"),
            None
        );
        arena
            .release_resource(sender.handle, sender.raw)
            .expect("release sender endpoint");
        arena
            .release_resource(receiver.handle, receiver.raw)
            .expect("release receiver endpoint");
    }

    #[test]
    fn cloned_channel_endpoints_keep_physical_counts_until_last_drop() {
        let arena = test_channel_arena();
        let (sender, receiver) = arena
            .register_new_channel(test_handle(21), test_handle(22), None)
            .expect("admit channel endpoints");
        let sender_clone = arena
            .clone_channel_sender(sender.handle, sender.raw, test_handle(23))
            .expect("clone sender endpoint");
        let receiver_clone = arena
            .clone_channel_receiver(receiver.handle, receiver.raw, test_handle(24))
            .expect("clone receiver endpoint");
        arena
            .release_resource(sender.handle, sender.raw)
            .expect("release original sender endpoint");
        arena
            .release_resource(receiver.handle, receiver.raw)
            .expect("release original receiver endpoint");
        arena
            .channel_sender_send(sender_clone.handle, sender_clone.raw, MirRuntimeValue::Int(11))
            .expect("send with cloned sender");
        assert_eq!(
            arena
                .channel_receiver_try_receive(
                    receiver_clone.handle,
                    receiver_clone.raw,
                )
                .expect("receive with cloned receiver"),
            Some(MirRuntimeValue::Int(11))
        );
        arena
            .channel_sender_close(sender_clone.handle, sender_clone.raw)
            .expect("close final sender endpoint");
        assert_eq!(
            arena
                .channel_receiver_try_receive(receiver_clone.handle, receiver_clone.raw)
                .expect("observe final sender close"),
            None
        );
        arena
            .release_resource(sender_clone.handle, sender_clone.raw)
            .expect("release cloned sender endpoint");
        arena
            .release_resource(receiver_clone.handle, receiver_clone.raw)
            .expect("release cloned receiver endpoint");
    }

    #[test]
    fn source_sender_reservation_publishes_only_after_commit() {
        let arena = test_channel_arena();
        let (sender, receiver) = arena
            .register_new_channel(test_handle(24), test_handle(25), Some(1))
            .expect("admit bounded channel endpoints");
        let reservation = arena
            .channel_sender_reserve(sender.handle, sender.raw)
            .expect("reserve Source sender capacity");
        assert_eq!(
            arena
                .channel_receiver_try_receive(receiver.handle, receiver.raw)
                .expect("poll receiver before commit"),
            None
        );
        assert!(reservation.commit(MirRuntimeValue::Int(91)).is_ok());
        assert_eq!(
            arena
                .channel_receiver_try_receive(receiver.handle, receiver.raw)
                .expect("receive committed Source value"),
            Some(MirRuntimeValue::Int(91))
        );
        arena
            .release_capability(&sender)
            .expect("release bounded sender");
        arena
            .release_capability(&receiver)
            .expect("release bounded receiver");
    }

    #[test]
    fn rejected_channel_send_returns_owned_value() {
        let arena = test_channel_arena();
        let (sender, receiver) = arena
            .register_new_channel(test_handle(25), test_handle(26), None)
            .expect("admit channel endpoints");
        arena
            .release_resource(receiver.handle, receiver.raw)
            .expect("release final receiver endpoint");
        let error = arena
            .channel_sender_send(sender.handle, sender.raw, MirRuntimeValue::Int(12))
            .expect_err("closed channel must reject send");
        assert_eq!(error.value, MirRuntimeValue::Int(12));
        arena
            .release_resource(sender.handle, sender.raw)
            .expect("release sender endpoint");
    }

    #[test]
    fn final_receiver_release_returns_queued_values() {
        let arena = test_channel_arena();
        let (sender, receiver) = arena
            .register_new_channel(test_handle(27), test_handle(28), None)
            .expect("admit channel endpoints");
        arena
            .channel_sender_send(sender.handle, sender.raw, MirRuntimeValue::Int(13))
            .expect("queue channel value");
        let drained = arena
            .release_resource(receiver.handle, receiver.raw)
            .expect("release final receiver endpoint");
        assert_eq!(
            drained.drained_values(),
            vec![MirRuntimeValue::Int(13)]
        );
        arena
            .release_resource(sender.handle, sender.raw)
            .expect("release sender endpoint");
    }

    #[test]
    fn receiver_alias_keeps_queue_until_last_physical_receiver() {
        let arena = test_channel_arena();
        let (sender, receiver) = arena
            .register_new_channel(test_handle(29), test_handle(30), None)
            .expect("admit channel endpoints");
        let receiver_alias = arena
            .clone_channel_receiver(receiver.handle, receiver.raw, test_handle(31))
            .expect("clone receiver endpoint");
        arena
            .channel_sender_send(sender.handle, sender.raw, MirRuntimeValue::Int(14))
            .expect("queue channel value");
        assert!(matches!(
            arena
                .release_resource(receiver.handle, receiver.raw)
                .expect("release aliased receiver"),
            SourceResourceRelease::Released
        ));
        assert_eq!(
            arena
                .channel_receiver_try_receive(receiver_alias.handle, receiver_alias.raw)
                .expect("receive through surviving alias"),
            Some(MirRuntimeValue::Int(14))
        );
        arena
            .release_resource(sender.handle, sender.raw)
            .expect("release sender endpoint");
        arena
            .release_resource(receiver_alias.handle, receiver_alias.raw)
            .expect("release final receiver alias");
    }

    #[test]
    fn receiver_select_uses_canonical_arm_order_for_source_values() {
        let arena = test_channel_arena();
        let (first_sender, first_receiver) = arena
            .register_new_channel(test_handle(60), test_handle(61), None)
            .expect("register first select arm");
        let (second_sender, second_receiver) = arena
            .register_new_channel(test_handle(62), test_handle(63), None)
            .expect("register second select arm");
        arena
            .channel_sender_send(first_sender.handle, first_sender.raw, MirRuntimeValue::Int(61))
            .expect("queue first arm");
        arena
            .channel_sender_send(second_sender.handle, second_sender.raw, MirRuntimeValue::Int(63))
            .expect("queue second arm");

        assert_eq!(
            arena
                .channel_receiver_select(
                    &[
                        (second_receiver.handle, second_receiver.raw),
                        (first_receiver.handle, first_receiver.raw),
                    ],
                    Vec::new(),
                )
                .expect("select ready Source receiver"),
            (0, Some(MirRuntimeValue::Int(63)))
        );
        assert_eq!(
            arena
                .channel_receiver_select(
                    &[(first_receiver.handle, first_receiver.raw)],
                    Vec::new(),
                )
                .expect("select remaining Source receiver"),
            (0, Some(MirRuntimeValue::Int(61)))
        );
        assert_eq!(
            arena
                .channel_receiver_select(&[], vec![0])
                .expect("select immediate timer arm"),
            (0, None)
        );
        for capability in [
            first_sender,
            first_receiver,
            second_sender,
            second_receiver,
        ] {
            arena
                .release_capability(&capability)
                .expect("release selected endpoint");
        }
    }

    #[test]
    fn explicit_cleanup_retires_queued_source_values() {
        let arena = test_channel_arena();
        let (sender, _receiver) = arena
            .register_new_channel(test_handle(32), test_handle(33), None)
            .expect("admit channel endpoints");
        arena
            .channel_sender_send(sender.handle, sender.raw, MirRuntimeValue::Int(15))
            .expect("queue source value");
        let mut cleaned = Vec::new();
        arena
            .retire_with_cleanup(|value| {
                cleaned.push(value);
                Ok(())
            })
            .expect("explicit retirement cleanup");
        assert_eq!(cleaned, vec![MirRuntimeValue::Int(15)]);
        assert!(arena.is_retired().expect("retired after cleanup"));
    }

    #[test]
    fn retirement_keeps_nested_receivers_live_and_retries_exact_values() {
        let nested_receiver = Arc::new(Mutex::new(None::<SourceResourceHandle>));
        let observed_nested = Arc::new(AtomicUsize::new(0));
        let finalized = Arc::new(Mutex::new(Vec::new()));
        let attempts = Arc::new(AtomicUsize::new(0));
        let nested = nested_receiver.clone();
        let observed = observed_nested.clone();
        let sink = finalized.clone();
        let calls = attempts.clone();
        let session = SourceResourceSession::new_with_finalizer(
            Arc::new(move |finalization| {
                finalization.with_live_scope(|arena, _lease, values, _completions| {
                    if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                        let capability = nested
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .clone()
                            .expect("nested receiver registered before retirement");
                        assert!(arena
                            .lookup_capability(capability.handle, capability.raw)
                            .is_ok());
                        let nested_value = arena
                            .channel_receiver_try_receive(capability.handle, capability.raw)
                            .expect("nested receiver remains usable during cleanup")
                            .expect("nested receiver has queued Source value");
                        observed.fetch_add(1, Ordering::SeqCst);
                        values.push(nested_value);
                        Err("retry this Source cleanup".to_string())
                    } else {
                        sink.lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .extend(std::mem::take(values));
                        Ok(())
                    }
                })
            }),
            Arc::new(|error| panic!("unexpected automatic finalization failure: {error:?}")),
        );
        let arena = session.arena();
        let (first_sender, first_receiver) = arena
            .register_new_channel(test_handle(54), test_handle(55), None)
            .expect("register first receiver");
        let (second_sender, second_receiver) = arena
            .register_new_channel(test_handle(56), test_handle(57), None)
            .expect("register nested receiver");
        *nested_receiver
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(second_receiver.clone());
        arena
            .channel_sender_send(
                first_sender.handle,
                first_sender.raw,
                MirRuntimeValue::Int(15),
            )
            .expect("queue first Source value");
        arena
            .channel_sender_send(
                second_sender.handle,
                second_sender.raw,
                MirRuntimeValue::Int(16),
            )
            .expect("queue nested Source value");

        let failure = session
            .retire()
            .expect_err("first Source cleanup returns exact untouched values");
        assert_eq!(
            failure.values,
            vec![MirRuntimeValue::Int(15), MirRuntimeValue::Int(16)]
        );
        assert_eq!(observed_nested.load(Ordering::SeqCst), 1);
        assert!(arena
            .lookup_capability(first_receiver.handle, first_receiver.raw)
            .is_ok());
        assert!(arena
            .lookup_capability(second_receiver.handle, second_receiver.raw)
            .is_ok());
        session
            .retry_retire(failure)
            .expect("retry exact untouched Source values");
        assert_eq!(
            *finalized.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![MirRuntimeValue::Int(15), MirRuntimeValue::Int(16)]
        );
        assert!(arena.is_retired().expect("retirement completes after retry"));
    }

    #[test]
    fn final_root_uses_explicit_origin_finalizer_for_channel_values() {
        let cleaned = Arc::new(Mutex::new(Vec::new()));
        let sink = cleaned.clone();
        let session = SourceResourceSession::new_with_finalizer(
            Arc::new(move |finalization| {
                finalization.with_live_scope(|_, _lease, values, _completions| {
                    sink.lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .extend(std::mem::take(values));
                    Ok(())
                })
            }),
            Arc::new(|error| panic!("unexpected finalization failure: {error:?}")),
        );
        let root = session.retain_root().expect("retain source root");
        let arena = root.arena();
        let (sender, _receiver) = arena
            .register_new_channel(test_handle(34), test_handle(35), None)
            .expect("admit channel endpoints");
        arena
            .channel_sender_send(sender.handle, sender.raw, MirRuntimeValue::Int(16))
            .expect("queue source value");
        session.retire().expect("request source retirement");
        drop(root);
        assert!(arena.is_retired().expect("finalizer retires arena"));

        assert_eq!(
            *cleaned.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![MirRuntimeValue::Int(16)]
        );
    }
    #[test]
    fn finalizer_lease_keeps_nested_capabilities_live_until_helper_completion() {
        let nested_capability = Arc::new(Mutex::new(None::<SourceResourceHandle>));
        let escaped_lease = Arc::new(Mutex::new(None::<SourceResourceLease>));
        let shared_root_capability = Arc::new(Mutex::new(None::<SourceResourceHandle>));
        let shared_weak_capability = Arc::new(Mutex::new(None::<SourceResourceHandle>));
        let nested = nested_capability.clone();
        let escaped = escaped_lease.clone();
        let shared_root_for_finalizer = shared_root_capability.clone();
        let shared_weak_for_finalizer = shared_weak_capability.clone();
        let shared_type_identity = 0xfeed_beef_cafe_5680;
        let session = SourceResourceSession::new_with_finalizer(
            Arc::new(move |finalization| {
                let result =
                    finalization.with_live_scope(|arena, lease, values, _completions| {
                    assert_eq!(values.len(), 1);
                    assert_eq!(values[0], MirRuntimeValue::Int(16));
                    let nested = nested
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .clone()
                        .expect("nested capability registered before finalization");
                    assert!(arena
                        .lookup_capability(nested.handle, nested.raw)
                        .is_ok());

                    let shared_root = shared_root_for_finalizer
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .clone()
                        .expect("Shared root capability is registered");
                    let (_, shared_lease) = arena
                        .borrow_shared_interop_root(&shared_root, shared_type_identity)
                        .map_err(|error| {
                            error
                                .internal_message()
                                .unwrap_or("finalizer Shared root borrow failed")
                                .to_string()
                        })?;
                    assert!(Arc::ptr_eq(&shared_lease.inner, &lease.inner));
                    drop(shared_lease);

                    let shared_weak = shared_weak_for_finalizer
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .clone()
                        .expect("Shared weak-root capability is registered");
                    let (_, weak_lease) = arena
                        .borrow_shared_interop_weak_root(&shared_weak, shared_type_identity)
                        .map_err(|error| {
                            error
                                .internal_message()
                                .unwrap_or("finalizer Shared weak-root borrow failed")
                                .to_string()
                        })?;
                    assert!(Arc::ptr_eq(&weak_lease.inner, &lease.inner));
                    drop(weak_lease);

                    *escaped
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(lease.clone());
                    values.clear();
                    Ok(())
                });
                let escaped_lease = escaped
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .as_ref()
                    .expect("finalizer lease escaped for lifetime retention")
                    .clone();
                let activation = escaped_lease.activate();
                assert!(
                    escaped_lease.cleanup_lease().is_err(),
                    "closed finalizer phase cannot be reopened by an escaped lease"
                );
                drop(activation);
                result
            }),
            Arc::new(|error| panic!("unexpected automatic finalization failure: {error:?}")),
        );
        let arena = session.arena();
        let nested = arena
            .register_stdin(test_handle(59))
            .expect("register nested Source capability");
        *nested_capability
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(nested.clone());

        let owner_slot = Arc::new(Mutex::new(None));
        let weak_owner_slot = Arc::downgrade(&owner_slot);
        let owner_count = Arc::new(AtomicUsize::new(1));
        let next_token_id = Arc::new(AtomicU32::new(1));
        let weak_count = owner_count.clone();
        let weak_token_id = next_token_id.clone();
        let count_for_count = owner_count.clone();
        let count_for_retain = owner_count.clone();
        let token_for_retain = next_token_id.clone();
        let owner_identity = usize::MAX - 21;
        let shared_root = crate::SourceSharedInterop::SourceSharedInterop::from_value(
            shared_type_identity,
            MirRuntimeValue::Int(23),
        )
        .with_owner_identity(owner_identity)
        .with_owner_lifecycle(move || {
            Box::new(TestSharedInteropWeakOwner {
                owner_slot: weak_owner_slot.clone(),
                owner_count: weak_count.clone(),
                next_token_id: weak_token_id.clone(),
            })
        })
        .with_owner_alias_lifecycle(
            move || Ok(count_for_count.load(Ordering::Acquire)),
            move || {
                let token_id = i64::from(token_for_retain.fetch_add(1, Ordering::AcqRel));
                reserve_test_shared_alias(&count_for_retain, token_id)?.ok_or_else(|| {
                    "test Source Shared owner has no live logical aliases".to_string()
                })
            },
        );
        *owner_slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(shared_root.clone());
        let shared_root_cap = arena
            .register_shared_interop_root(shared_type_identity, &shared_root)
            .expect("register Shared root");
        let shared_weak_cap = arena
            .downgrade_shared_interop_root(&shared_root_cap)
            .expect("register Shared weak root");
        *shared_root_capability
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(shared_root_cap);
        *shared_weak_capability
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(shared_weak_cap);

        let (sender, _) = arena
            .register_new_channel(test_handle(60), test_handle(61), None)
            .expect("register receiver to finalize");
        arena
            .channel_sender_send(sender.handle, sender.raw, MirRuntimeValue::Int(16))
            .expect("queue Source value for finalizer");

        let failure = session
            .retire()
            .expect_err("finalizer lease must block premature slot removal");
        assert!(failure.values.is_empty());
        assert_eq!(session.retained_root_count().expect("finalizer root count"), 1);
        assert!(!arena.is_retired().expect("finalizer lease preserves arena"));
        assert!(arena.lookup_capability(nested.handle, nested.raw).is_ok());

        drop(
            escaped_lease
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .take()
                .expect("helper releases its counted lease"),
        );
        assert!(arena.is_retired().expect("helper completion permits retirement"));
        assert!(arena.lookup_capability(nested.handle, nested.raw).is_err());
    }

    #[test]
    fn finalizer_cleanup_lease_adopts_queued_owner_and_registers_fresh_binding() {
        use jet_foundation::MIR::{
            MirArtifactBuildMode, MirArtifactIdentity, MirArtifactKind, MirArtifactTarget,
            MirExecutionIdentity, MirProgramIdentity, MIR_SCHEMA_VERSION,
        };

        let logical_values = Arc::new(Mutex::new(Vec::new()));
        let adopted_values = logical_values.clone();
        let artifact = jet_foundation::MIR::MirArtifactId(9101);
        let execution = MirExecutionIdentity {
            schema_version: 1,
            artifact: MirArtifactIdentity {
                schema_version: 1,
                mir_schema_version: MIR_SCHEMA_VERSION,
                program_digest: [0; 32],
                package_identity: "source-finalizer-test".to_string(),
                artifact,
                name: "owned-root-cleanup".to_string(),
                kind: MirArtifactKind::NativeExecutable,
                target: MirArtifactTarget::Cranelift,
                mode: MirArtifactBuildMode::Dev,
                provider_identity: String::new(),
                closure_identity: String::new(),
                artifact_identity: String::new(),
                program_identity: MirProgramIdentity::unavailable(String::new(), Vec::new()),
            },
        };
        let session = SourceResourceSession::new_with_finalizer(
            Arc::new(move |finalization| {
                finalization.with_live_scope(|arena, lease, values, _completions| {
                    if values.len() != 1 {
                        return Err("finalizer expected one queued owned payload".to_string());
                    }
                    let carrier = values.pop().expect("queued carrier is present");
                    let owned = source_owned_from_runtime_value(carrier).map_err(|error| {
                        error
                            .internal_message()
                            .unwrap_or("queued owner carrier was invalid")
                            .to_string()
                    })?;
                    let cleanup = lease.cleanup_lease()?;
                    let outcome = cleanup.adopt_owned(
                        test_handle(92),
                        &SourceResourceKind::LinesStdin,
                        owned,
                    );
                    let (adopted, logical_value) = match outcome.result {
                        Ok(adopted) => adopted,
                        Err(failure) => {
                            let (error, owned) = failure.into_parts();
                            values.push(owned.into_runtime_value());
                            return Err(error
                                .internal_message()
                                .unwrap_or("queued Source owner adoption failed")
                                .to_string());
                        }
                    };
                    if logical_value != MirRuntimeValue::String("queued owner".to_string()) {
                        return Err("queued Source logical value changed during adoption".to_string());
                    }
                    if arena
                        .lookup_capability(adopted.handle, adopted.raw)
                        .is_err()
                    {
                        return Err("cleanup adoption did not publish its physical owner".to_string());
                    }
                    arena.release_capability(&adopted)?;

                    let bindings = crate::SourceInterfaces::NativeInterfaceBindings::new();
                    let object = bindings
                        .create_object("cleanup-scope")
                        .map_err(|error| error.to_string())?;
                    let binding = SourceNativeBinding::interface(
                        &object,
                        execution.clone(),
                        artifact,
                        MirType::from_kind(MirTypeKind::Int),
                    )?;
                    assert!(arena.register_native_binding_root(binding.clone()).is_err());
                    let fresh_scope = cleanup.register_native_binding_root(binding)?;
                    assert_eq!(fresh_scope.kind, SourceResourceKind::NativeBinding);
                    assert!(arena
                        .lookup_capability(fresh_scope.handle, fresh_scope.raw)
                        .is_ok());
                    arena.release_native_binding_capability(&fresh_scope)?;
                    adopted_values
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .push(logical_value);
                    Ok(())
                })
            }),
            Arc::new(|error| panic!("unexpected finalizer cleanup failure: {error:?}")),
        );
        let arena = session.arena();
        let source = arena
            .register_stdin(test_handle(91))
            .expect("register Source-owned stream");
        let (_, owned) = arena
            .take_owned_with_value(
                source.handle,
                source.raw,
                &SourceResourceKind::LinesStdin,
                MirRuntimeValue::String("queued owner".to_string()),
            )
            .expect("detach checked Source owner");
        let owned_carrier = owned.into_runtime_value();
        let (sender, _receiver) = arena
            .register_new_channel(test_handle(93), test_handle(94), None)
            .expect("register finalizer channel");
        arena
            .channel_sender_send(sender.handle, sender.raw, owned_carrier)
            .expect("queue detached owner for Source cleanup");

        session
            .retire()
            .expect("finalizer adopts queued owner and binds fresh cleanup scope");
        assert_eq!(
            *logical_values
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            [MirRuntimeValue::String("queued owner".to_string())]
        );
        assert!(arena.is_retired().expect("finalizer cleanup completes retirement"));
    }

    #[test]
    fn deferred_retirement_creation_requires_existing_normal_lease() {
        let finalizer_runs = Arc::new(AtomicUsize::new(0));
        let observed_runs = finalizer_runs.clone();
        let session = SourceResourceSession::new_with_finalizer(
            Arc::new(move |finalization| {
                observed_runs.fetch_add(1, Ordering::SeqCst);
                finalization.with_live_scope(|_, _, values, _completions| {
                    if values.len() != 1 || values[0] != MirRuntimeValue::Int(77) {
                        return Err("deferred channel finalizer received the wrong value".to_string());
                    }
                    values.clear();
                    Ok(())
                })
            }),
            Arc::new(|error| panic!("deferred retirement finalization failed: {error:?}")),
        );
        let lease = session.retain_root().expect("retain normal callback lease");
        let arena = lease.arena();

        session.retire().expect("request deferred retirement");
        assert!(session.is_retirement_requested().unwrap());
        assert!(!arena.is_retired().unwrap());
        assert_eq!(session.retained_root_count().unwrap(), 1);
        assert_eq!(finalizer_runs.load(Ordering::SeqCst), 0);
        assert!(session.retain_root().is_err());
        assert!(arena.register_stdin(test_handle(96)).is_err());

        let binding = test_native_binding(9302, "deferred-normal-callback");
        assert!(arena.register_native_binding_root(binding.clone()).is_err());
        let activation = lease.activate();
        let binding_capability = arena
            .register_native_binding_root(binding)
            .expect("existing normal lease authorizes a fresh binding");
        let resource_capability = arena
            .register_stdin(test_handle(97))
            .expect("existing normal lease authorizes a fresh resource");
        let (sender, receiver) = arena
            .register_new_channel(test_handle(98), test_handle(99), Some(1))
            .expect("existing normal lease authorizes fresh channel endpoints");
        arena
            .channel_sender_send(sender.handle, sender.raw, MirRuntimeValue::Int(77))
            .expect("queue one value for exactly-once finalization");
        assert_eq!(session.retained_root_count().unwrap(), 1);
        drop(activation);
        assert_eq!(session.retained_root_count().unwrap(), 1);

        drop(lease);
        assert!(arena.is_retired().unwrap());
        assert_eq!(session.retained_root_count().unwrap(), 0);
        assert_eq!(finalizer_runs.load(Ordering::SeqCst), 1);
        assert!(arena
            .lookup_capability(binding_capability.handle, binding_capability.raw)
            .is_err());
        assert!(arena
            .lookup_capability(resource_capability.handle, resource_capability.raw)
            .is_err());
        assert!(arena
            .lookup_capability(receiver.handle, receiver.raw)
            .is_err());

        session.retire().expect("retirement is idempotent");
        assert_eq!(finalizer_runs.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn shared_interop_root_capability_and_alias_leases_keep_nested_resources_live() {
        let session = test_shared_interop_session();
        let arena = session.arena();
        let nested = arena
            .register_stdin(test_handle(58))
            .expect("register nested Source resource");
        let type_identity = u64::MAX;
        let root = crate::SourceSharedInterop::SourceSharedInterop::from_value(
            type_identity,
            MirRuntimeValue::Int(17),
        );
        let capability = arena
            .register_shared_interop_root(type_identity, &root)
            .expect("register checked SharedInterop root");
        assert_eq!(
            arena
                .resolve_shared_interop_root(&root)
                .expect("resolve the existing SharedInterop capability"),
            capability
        );
        assert_eq!(capability.handle, shared_interop_root_handle_id());
        assert_eq!(
            capability.kind,
            SourceResourceKind::SharedInteropRoot { type_identity }
        );
        assert!(arena
            .borrow_shared_interop_root(&capability, type_identity - 1)
            .is_err());
        let (root, lease) = arena
            .borrow_shared_interop_root(&capability, type_identity)
            .expect("borrow exact SharedInterop root");
        let alias = root.clone();
        let alias_lease = lease.clone();

        session.retire().expect("defer while native aliases are live");
        arena
            .release_shared_interop_root(&capability)
            .expect("release Source registry root");
        assert!(arena.resolve_shared_interop_root(&root).is_err());
        assert!(!arena.is_retired().expect("alias lease retains arena"));
        assert!(arena.lookup_capability(nested.handle, nested.raw).is_ok());
        drop(root);
        drop(lease);
        assert!(!arena.is_retired().expect("second alias remains live"));
        assert_eq!(
            alias
                .with_read(|value| Ok(value.clone()))
                .expect("native alias remains usable"),
            MirRuntimeValue::Int(17)
        );
        drop(alias);
        drop(alias_lease);
        assert!(arena.is_retired().expect("last native alias releases arena"));
        assert!(arena.lookup_capability(nested.handle, nested.raw).is_err());
    }


    #[test]
    fn shared_interop_weak_root_upgrades_only_a_live_owner() {
        let session = test_shared_interop_session();
        let arena = session.arena();
        let owner_slot = Arc::new(Mutex::new(None));
        let weak_slot = Arc::downgrade(&owner_slot);
        let owner_count = Arc::new(AtomicUsize::new(1));
        let next_token_id = Arc::new(AtomicU32::new(1));
        let type_identity = 0xfeed_beef_cafe_1234;
        let owner_identity = usize::MAX - 9;
        let count_for_weak = owner_count.clone();
        let token_for_weak = next_token_id.clone();
        let count_for_count = owner_count.clone();
        let count_for_retain = owner_count.clone();
        let token_for_retain = next_token_id.clone();
        let root = crate::SourceSharedInterop::SourceSharedInterop::from_value(
            type_identity,
            MirRuntimeValue::Int(27),
        )
        .with_owner_identity(owner_identity)
        .with_owner_lifecycle(move || {
            Box::new(TestSharedInteropWeakOwner {
                owner_slot: weak_slot.clone(),
                owner_count: count_for_weak.clone(),
                next_token_id: token_for_weak.clone(),
            })
        })
        .with_owner_alias_lifecycle(
            move || Ok(count_for_count.load(Ordering::Acquire)),
            move || {
                let token_id = i64::from(token_for_retain.fetch_add(1, Ordering::AcqRel));
                reserve_test_shared_alias(&count_for_retain, token_id)?.ok_or_else(|| {
                    "test Source Shared owner has no live logical aliases".to_string()
                })
            },
        );
        *owner_slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) =
            Some(root.clone());
        let strong = arena
            .register_shared_interop_root(type_identity, &root)
            .expect("register strong SharedInterop root");
        let weak_capability = arena
            .downgrade_shared_interop_root(&strong)
            .expect("downgrade physical Shared owner");
        assert_eq!(owner_count.load(Ordering::Acquire), 1);
        let mut stale_strong = strong.clone();
        stale_strong.generation = stale_strong.generation.wrapping_add(1);
        assert!(arena.downgrade_shared_interop_root(&stale_strong).is_err());
        assert!(arena
            .borrow_shared_interop_weak_root(&weak_capability, type_identity + 1)
            .is_err());
        let mut stale_weak = weak_capability.clone();
        stale_weak.generation = stale_weak.generation.wrapping_add(1);
        assert!(arena
            .borrow_shared_interop_weak_root(&stale_weak, type_identity)
            .is_err());
        let (weak, weak_lease) = arena
            .borrow_shared_interop_weak_root(&weak_capability, type_identity)
            .expect("borrow weak root capability");
        assert_eq!(owner_count.load(Ordering::Acquire), 1);
        let weak_upgrade = weak
            .upgrade()
            .expect("upgrade live Shared owner")
            .expect("owner remains live");
        assert_eq!(weak_upgrade.interop().identity(), owner_identity);
        drop(weak_upgrade);
        assert_eq!(owner_count.load(Ordering::Acquire), 1);

        drop(root);
        arena
            .release_shared_interop_root(&strong)
            .expect("release original strong capability");
        let (upgraded, fresh_strong, fresh_lease, token_id) = arena
            .upgrade_shared_interop_weak_root(&weak_capability)
            .expect("upgrade through weak capability")
            .expect("logical strong owner keeps physical root alive");
        assert_eq!(upgraded.identity(), owner_identity);
        assert_eq!(
            fresh_strong.kind,
            SourceResourceKind::SharedInteropRoot { type_identity }
        );
        assert_eq!(owner_count.load(Ordering::Acquire), 2);
        let direct_token = arena
            .retain_shared_interop_alias(&fresh_strong)
            .expect("reserve a Source Shared logical alias");
        assert_ne!(direct_token, token_id);
        assert_eq!(owner_count.load(Ordering::Acquire), 3);
        assert!(arena
            .release_shared_interop_alias(&fresh_strong, direct_token.wrapping_add(1000))
            .is_err());
        release_test_shared_alias(&arena, &fresh_strong, direct_token);
        assert_eq!(owner_count.load(Ordering::Acquire), 2);
        assert!(arena.release_shared_interop_root(&fresh_strong).is_err());
        let mut stale_fresh = fresh_strong.clone();
        stale_fresh.generation = stale_fresh.generation.wrapping_add(1);
        assert!(arena
            .release_shared_interop_alias(&stale_fresh, token_id)
            .is_err());
        assert_eq!(owner_count.load(Ordering::Acquire), 2);
        let carrier = arena
            .adopt_shared_interop_alias(&fresh_strong, token_id)
            .expect("adopt the reserved owner alias");
        assert_eq!(carrier.owner_alias_token_id(), Some(token_id));
        assert!(arena
            .release_shared_interop_alias(&fresh_strong, token_id)
            .is_err());
        arena
            .release_shared_interop_root(&fresh_strong)
            .expect("release fresh physical root after alias adoption");
        drop(upgraded);
        drop(fresh_lease);
        drop(carrier);
        assert_eq!(owner_count.load(Ordering::Acquire), 1);

        owner_count.store(0, Ordering::Release);
        owner_slot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        assert!(weak
            .upgrade()
            .expect("observe expired Source owner")
            .is_none());
        assert!(arena
            .upgrade_shared_interop_weak_root(&weak_capability)
            .expect("weak upgrade of expired owner")
            .is_none());

        session.retire().expect("defer while weak alias lease is live");
        assert_eq!(session.retained_root_count().expect("weak alias root count"), 1);
        assert!(!arena.is_retired().expect("weak alias lease preserves arena"));
        arena
            .release_shared_interop_weak_root(&weak_capability)
            .expect("release weak registry capability");
        drop(weak);
        drop(weak_lease);
        assert!(arena.is_retired().expect("last weak alias releases arena"));
    }

    #[test]
    fn shared_alias_release_returns_completion_even_when_operation_fails() {
        let session = test_shared_interop_session();
        let arena = session.arena();
        let owner_count = Arc::new(AtomicUsize::new(1));
        let count_for_count = owner_count.clone();
        let count_for_retain = owner_count.clone();
        let completion: crate::Memory::shared_protocol::JetSharedPhysicalCompletion =
            Box::new(test_nested_completion());
        let completion_pointer = completion
            .as_ref()
            .downcast_ref::<SourceExecutionCompletion>()
            .expect("test completion is stored as its canonical receipt type")
            as *const SourceExecutionCompletion as usize;
        let completion_for_retain = Arc::new(Mutex::new(Some(completion)));
        let completion_packet = completion_for_retain.clone();
        let type_identity = 0xdead_beef_cafe_4321;
        let owner_identity = usize::MAX - 31;
        let root = crate::SourceSharedInterop::SourceSharedInterop::from_value(
            type_identity,
            MirRuntimeValue::Int(3),
        )
        .with_owner_identity(owner_identity)
        .with_owner_alias_lifecycle(
            move || Ok(count_for_count.load(Ordering::Acquire)),
            move || {
                let completion = completion_packet
                    .lock()
                    .map_err(|_| "test completion packet is poisoned".to_string())?
                    .take()
                    .ok_or_else(|| "test completion packet was already consumed".to_string())?;
                reserve_test_shared_alias_with_completion(
                    &count_for_retain,
                    0x117,
                    Some(completion),
                    Some("physical alias release failed".to_string()),
                )?
                .ok_or_else(|| "test Shared owner has no live logical aliases".to_string())
            },
        );
        let capability = arena
            .register_shared_interop_root(type_identity, &root)
            .expect("register Shared owner root");
        let token_id = arena
            .retain_shared_interop_alias(&capability)
            .expect("retain Source Shared logical alias");
        assert_eq!(token_id, 0x117);

        let outcome = arena
            .release_shared_interop_alias(&capability, token_id)
            .expect("release Source Shared logical alias");
        assert_eq!(
            outcome.result,
            Err("physical alias release failed".to_string())
        );
        let completion = outcome
            .completion
            .expect("physical alias release returns its completion packet");
        let completion = match completion.downcast::<SourceExecutionCompletion>() {
            Ok(completion) => completion,
            Err(_) => panic!("physical completion packet lost its typed Source receipt"),
        };
        assert_eq!(
            completion.as_ref() as *const SourceExecutionCompletion as usize,
            completion_pointer
        );
        assert_nested_completion(&completion);
        assert_eq!(owner_count.load(Ordering::Acquire), 1);

        arena
            .release_shared_interop_root(&capability)
            .expect("release Shared owner root");
        session.retire().expect("retire empty Source session");
    }

    #[test]
    fn pending_shared_alias_blocks_root_release_and_arena_retirement() {
        let session = test_shared_interop_session();
        let arena = session.arena();
        let owner_count = Arc::new(AtomicUsize::new(1));
        let next_token_id = Arc::new(AtomicU32::new(41));
        let count_for_count = owner_count.clone();
        let count_for_retain = owner_count.clone();
        let token_for_retain = next_token_id.clone();
        let root = crate::SourceSharedInterop::SourceSharedInterop::from_value(
            0x91,
            MirRuntimeValue::Int(1),
        )
        .with_owner_alias_lifecycle(
            move || Ok(count_for_count.load(Ordering::Acquire)),
            move || {
                let token_id = i64::from(token_for_retain.fetch_add(1, Ordering::AcqRel));
                reserve_test_shared_alias(&count_for_retain, token_id)?.ok_or_else(|| {
                    "test Source Shared owner has no live logical aliases".to_string()
                })
            },
        );
        assert_eq!(owner_count.load(Ordering::Acquire), 1);
        let root_capability = arena
            .register_shared_interop_root(0x91, &root)
            .expect("register physical Shared root");
        assert_eq!(owner_count.load(Ordering::Acquire), 1);
        let token_id = arena
            .retain_shared_interop_alias(&root_capability)
            .expect("retain Source logical alias");
        assert_eq!(owner_count.load(Ordering::Acquire), 2);
        assert!(arena
            .release_shared_interop_root(&root_capability)
            .is_err());

        let failure = session
            .retire()
            .expect_err("retirement cannot erase a pending owner alias");
        assert!(failure.error.contains("pending Shared owner aliases"));
        assert!(failure.values.is_empty());
        assert!(!arena.is_retired().expect("alias registry keeps arena live"));
        assert!(arena
            .lookup_capability(root_capability.handle, root_capability.raw)
            .is_ok());

        release_test_shared_alias(&arena, &root_capability, token_id);
        assert_eq!(owner_count.load(Ordering::Acquire), 1);
        arena
            .release_shared_interop_root(&root_capability)
            .expect("release physical root after alias cleanup");
        session
            .retire()
            .expect("retirement completes after pending alias release");
        assert!(arena.is_retired().expect("arena retires after alias release"));
    }

    #[test]
    fn deferred_retirement_shared_alias_uses_existing_lease() {
        let session = test_shared_interop_session();
        let arena = session.arena();
        let owner_count = Arc::new(AtomicUsize::new(1));
        let next_token_id = Arc::new(AtomicU32::new(81));
        let count_for_count = owner_count.clone();
        let count_for_retain = owner_count.clone();
        let token_for_retain = next_token_id.clone();
        let root = crate::SourceSharedInterop::SourceSharedInterop::from_value(
            0x92,
            MirRuntimeValue::Int(2),
        )
        .with_owner_alias_lifecycle(
            move || Ok(count_for_count.load(Ordering::Acquire)),
            move || {
                let token_id = i64::from(token_for_retain.fetch_add(1, Ordering::AcqRel));
                reserve_test_shared_alias(&count_for_retain, token_id)?.ok_or_else(|| {
                    "test Source Shared owner has no live logical aliases".to_string()
                })
            },
        );
        let capability = arena
            .register_shared_interop_root(0x92, &root)
            .expect("register physical Shared root");
        let lease = session.retain_root().expect("retain normal callback lease");

        session.retire().expect("request deferred retirement");
        assert!(arena.retain_shared_interop_alias(&capability).is_err());
        assert_eq!(owner_count.load(Ordering::Acquire), 1);
        let activation = lease.activate();
        let token = arena
            .retain_shared_interop_alias(&capability)
            .expect("existing normal lease authorizes a deferred alias");
        assert_eq!(owner_count.load(Ordering::Acquire), 2);
        drop(activation);

        release_test_shared_alias(&arena, &capability, token);
        assert_eq!(owner_count.load(Ordering::Acquire), 1);
        arena
            .release_shared_interop_root(&capability)
            .expect("release root after pending alias");
        drop(root);
        drop(lease);
        assert!(arena.is_retired().unwrap());
        assert_eq!(session.retained_root_count().unwrap(), 0);
    }

    #[test]
    fn encoding_writer_consumes_file_writer_without_reopening_path() {
        let path = test_path("encoding-writer");
        let arena = SourceResourceArena::new();
        let file = arena
            .open_file_writer(test_handle(14), path.to_str().expect("utf8 scratch path"))
            .expect("admit file writer");
        let transfer = arena.register_encoding_writer_from_file(
            test_handle(15),
            file.handle,
            file.raw,
            "JSONLWriter",
        );
        assert!(transfer.consumed.is_some());
        let writer = transfer.result.expect("admit encoding writer");
        assert_eq!(
            writer.kind,
            SourceResourceKind::EncodingWriter {
                writer_type: "JSONLWriter".to_string()
            }
        );
        let value = MirRuntimeValue::Enum {
            type_name: "DataTree".to_string(),
            variant: "Int".to_string(),
            args: vec![(None, MirRuntimeValue::Int(7))],
        };
        arena
            .encoding_writer_write(writer.handle, writer.raw, &value)
            .expect("write encoded value");
        arena
            .encoding_writer_finish(writer.handle, writer.raw)
            .expect("finish encoded writer");
        assert_eq!(
            std::fs::read_to_string(&path).expect("read encoded output"),
            "7\n"
        );
        arena
            .release_resource(writer.handle, writer.raw)
            .expect("release encoding writer");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn encoding_reader_consumes_existing_file_reader_lease() {
        let path = test_path("encoding-reader");
        std::fs::write(&path, b"7\n").expect("create encoded source");
        let arena = SourceResourceArena::new();
        let file = arena
            .open_file(test_handle(18), path.to_str().expect("utf8 scratch path"))
            .expect("admit file reader");
        let transfer = arena.register_encoding_reader_from_file(
            test_handle(19),
            file.handle,
            file.raw,
            "JSONLReader",
        );
        assert!(transfer.consumed.is_some());
        let reader = transfer.result.expect("admit encoding reader");
        assert!(arena.lookup_capability(file.handle, file.raw).is_err());
        let key = reader.native_loop_key().expect("encoding loop key");
        let mut iterator = arena
            .encoding_reader(&key, "JSONLReader", false)
            .result
            .expect("borrow encoding reader");
        assert_eq!(
            iterator.next(),
            Some(Ok(MirRuntimeValue::Enum {
                type_name: "DataTree".to_string(),
                variant: "Int".to_string(),
                args: vec![(None, MirRuntimeValue::Int(7))],
            }))
        );
        assert_eq!(iterator.next(), None);
        drop(iterator);
        arena
            .release_resource(reader.handle, reader.raw)
            .expect("release encoding reader");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn encoding_transfer_outcome_distinguishes_preflight_and_post_take_failures() {
        let preflight_path = test_path("encoding-transfer-preflight");
        std::fs::write(&preflight_path, b"7\n").expect("create preflight source");
        let arena = SourceResourceArena::new();
        let preflight_file = arena
            .open_file(
                test_handle(25),
                preflight_path.to_str().expect("utf8 scratch path"),
            )
            .expect("admit preflight reader");
        let preflight = arena.register_encoding_reader_from_file(
            test_handle(26),
            preflight_file.handle,
            preflight_file.raw,
            "UnknownReader",
        );
        assert!(preflight.consumed.is_none());
        assert!(preflight.result.is_err());
        assert!(arena
            .lookup_capability(preflight_file.handle, preflight_file.raw)
            .is_ok());
        arena
            .release_resource(preflight_file.handle, preflight_file.raw)
            .expect("release preflight reader");
        let _ = std::fs::remove_file(preflight_path);

        let post_take_path = test_path("encoding-transfer-post-take");
        std::fs::write(&post_take_path, b"a,b\n").expect("create post-take source");
        let post_take_file = arena
            .open_file(
                test_handle(27),
                post_take_path.to_str().expect("utf8 scratch path"),
            )
            .expect("admit post-take reader");
        let post_take = arena.register_encoding_reader_from_file_with_options(
            test_handle(28),
            post_take_file.handle,
            post_take_file.raw,
            "CSVReader",
            SourceEncodingLimits::safe(),
            ",,",
            false,
            false,
        );
        assert!(post_take.consumed.is_some());
        assert!(post_take.result.is_err());
        assert!(arena
            .lookup_capability(post_take_file.handle, post_take_file.raw)
            .is_err());
        let _ = std::fs::remove_file(post_take_path);
    }

    #[test]
    fn source_writer_refuses_live_mapped_path() {
        let path = test_path("mapped-writer");
        let path_text = path.to_str().expect("utf8 scratch path");
        std::fs::write(&path, b"mapped\n").expect("create mapped source");
        let mapped = match crate::CoreHost::os_rt::jet_std::jet_std_files_map(
            &path_text.to_string(),
        ) {
            Ok(mapped) => mapped,
            Err(_) => panic!("mapped source must be admitted"),
        };
        let result = SourceResourceArena::new().open_file_writer(test_handle(10), path_text);
        drop(mapped);
        let error = match result {
            Ok(_) => panic!("writer must refuse a path held by a mapped reader"),
            Err(error) => error,
        };
        let value = value_of(error);
        assert!(matches!(
            value,
            MirRuntimeValue::Enum { type_name, variant, args }
                if type_name == "IOError"
                    && variant == "Other"
                    && matches!(
                        &args[0].1,
                        MirRuntimeValue::Struct { type_name, fields }
                            if type_name == "IOContext"
                                && matches!(
                                    &fields[0].1,
                                    MirRuntimeValue::Enum { type_name, variant, .. }
                                        if type_name == "IOOperation" && variant == "Write"
                                )
                                && matches!(
                                    &fields[3].1,
                                    MirRuntimeValue::Present(value)
                                        if matches!(
                                            value.as_ref(),
                                            MirRuntimeValue::String(message)
                                                if message == "file is mapped read-only"
                                        )
                                )
                    )
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn source_resource_rejects_wrong_scope_stale_and_kind() {
        let arena = SourceResourceArena::new();
        let capability = arena.register_stdin(test_handle(4)).expect("stdin owner");
        let key = capability.native_loop_key().expect("stdin loop key");
        let wrong_kind = NativeLoopResourceKey {
            handle: key.handle,
            raw: key.raw,
            source_kind: MirLoopSourceKind::Plain,
        };
        assert!(arena.lines_file(&wrong_kind, false).result.is_err());

        let other_arena = SourceResourceArena::new();
        assert!(other_arena.lines_stdin(&key, false).result.is_err());
        arena
            .release_resource(capability.handle, capability.raw)
            .expect("release exact source slot");
        assert!(arena.lines_stdin(&key, false).result.is_err());
        assert!(arena.release_resource(capability.handle, capability.raw).is_err());
    }

    #[test]
    fn source_activation_restores_nested_and_unwind_without_implicit_retirement() {
        let outer_session = SourceResourceSession::new();
        let outer_arena = outer_session.arena();
        let outer_capability = outer_arena.register_stdin(test_handle(5)).unwrap();
        let retained_capability = outer_arena.register_stdin(test_handle(6)).unwrap();
        let inner_session = SourceResourceSession::new();
        let inner_arena = inner_session.arena();
        let inner_capability = inner_arena.register_stdin(test_handle(7)).unwrap();

        {
            let _outer_activation = outer_session.activate();
            let active = active_source_resource_arena().expect("outer activation");
            assert!(Arc::ptr_eq(&active.state, &outer_arena.state));
            let result = catch_unwind(AssertUnwindSafe(|| {
                let _inner_activation = inner_session.activate();
                let active = active_source_resource_arena().expect("inner activation");
                assert!(Arc::ptr_eq(&active.state, &inner_arena.state));
                panic!("abort nested source callback");
            }));
            assert!(result.is_err());
            let active = active_source_resource_arena().expect("outer restoration");
            assert!(Arc::ptr_eq(&active.state, &outer_arena.state));
        }
        assert!(active_source_resource_arena().is_none());
        assert!(outer_arena.release_capability(&outer_capability).is_ok());
        assert!(inner_arena.release_capability(&inner_capability).is_ok());
        let retained_key = retained_capability.native_loop_key().unwrap();
        outer_session.retire().expect("explicit source retirement");
        assert!(outer_arena.release_capability(&retained_capability).is_err());
        assert!(outer_arena.register_stdin(test_handle(9)).is_err());
        assert!(outer_arena.lines_stdin(&retained_key, false).result.is_err());

    }

    #[test]
    fn source_open_preserves_canonical_io_error_value() {
        let path = test_path("missing");
        let _ = std::fs::remove_file(&path);
        let error = SourceResourceArena::new()
            .open_file(test_handle(7), path.to_str().expect("utf8 scratch path"))
            .expect_err("missing source file must be a typed IO error");
        let value = value_of(error);
        assert!(matches!(
            value,
            MirRuntimeValue::Enum { type_name, variant, args }
                if type_name == "IOError"
                    && variant == "NotFound"
                    && matches!(
                        &args[0].1,
                        MirRuntimeValue::Struct { type_name, fields }
                            if type_name == "IOContext"
                                && matches!(
                                    &fields[0].1,
                                    MirRuntimeValue::Enum { type_name, variant, .. }
                                        if type_name == "IOOperation" && variant == "Read"
                                )
                    )
        ));
    }

    #[test]
    fn source_error_shapes_preserve_canonical_enums_and_context() {
        let context = IOContext::new(
            IOOperation::Read,
            Some("source.dat".to_string()),
            Some(2),
            Some("denied".to_string()),
        );
        let file_value = value_of(file_error(IOError::PermissionDenied(context.clone())));
        let MirRuntimeValue::Enum {
            type_name,
            variant,
            args,
        } = file_value
        else {
            panic!("file error was not an IOError enum");
        };
        assert_eq!(type_name, "IOError");
        assert_eq!(variant, "PermissionDenied");
        assert!(matches!(
            &args[0].1,
            MirRuntimeValue::Struct { type_name, fields }
                if type_name == "IOContext"
                    && matches!(
                        &fields[0].1,
                        MirRuntimeValue::Enum { type_name, variant, .. }
                            if type_name == "IOOperation" && variant == "Read"
                    )
        ));

        let limit_value = value_of(process_error(IOError::ResourceLimit(
            ProcessResourceLimit::Memory,
        )));
        assert!(matches!(
            limit_value,
            MirRuntimeValue::Enum { type_name, variant, args }
                if type_name == "IOError"
                    && variant == "ResourceLimit"
                    && matches!(
                        &args[0].1,
                        MirRuntimeValue::Enum { type_name, variant, .. }
                            if type_name == "ProcessResourceLimit" && variant == "Memory"
                    )
        ));

        let encoding = EncodingError::new(
            EncodingFormat::JSON,
            EncodingErrorKind::Syntax,
            9,
            Ok(3),
            Err(JetAbsent),
            "source.dat",
            "bad token",
        )
        .with_cause(EncodingCause {
            kind: "os".to_string(),
            os_code: Ok(2),
            message: "parse".to_string(),
        });
        let encoding_value = value_of(encoding_error(&encoding));
        assert!(matches!(
            encoding_value,
            MirRuntimeValue::Struct { type_name, fields }
                if type_name == "EncodingError"
                    && matches!(
                        &fields[0].1,
                        MirRuntimeValue::Enum { type_name, variant, .. }
                            if type_name == "EncodingFormat" && variant == "JSON"
                    )
                    && matches!(
                        &fields[1].1,
                        MirRuntimeValue::Enum { type_name, variant, .. }
                            if type_name == "EncodingErrorKind" && variant == "Syntax"
                    )
        ));
    }

    #[test]
    fn cursor_by_value_preserves_consumed_receipt_on_success() {
        let arena = SourceResourceArena::new();
        let source = arena.register_stdin(test_handle(43)).expect("admit stdin owner");
        let key = source.native_loop_key().expect("stdin loop key");
        let outcome = arena.init_cursor(&key, 1, false, true);
        assert_eq!(outcome.consumed, Some(source.clone()));
        let (cursor, value) = outcome.result.expect("install native cursor");
        assert_eq!(cursor.kind, SourceResourceKind::NativeCursor);
        assert!(matches!(value, MirRuntimeValue::NativeCursor(_)));
        assert!(arena.lookup_capability(source.handle, source.raw).is_err());
        arena
            .release_capability(&cursor)
            .expect("release native cursor");
    }

    #[test]
    fn cursor_by_value_preserves_consumed_receipt_on_slot_failure() {
        let arena = SourceResourceArena::new();
        let source = arena.register_stdin(test_handle(44)).expect("admit stdin owner");
        let key = source.native_loop_key().expect("stdin loop key");
        arena
            .state
            .lock()
            .expect("lock source arena")
            .next_slot = u32::MAX;
        let outcome = arena.init_cursor(&key, 1, false, true);
        assert_eq!(outcome.consumed, Some(source.clone()));
        assert!(outcome.result.is_err());
        assert!(arena.lookup_capability(source.handle, source.raw).is_err());
    }

    #[test]
    fn take_owned_with_value_returns_logical_value_on_validation_failure() {
        let arena = SourceResourceArena::new();
        let value = MirRuntimeValue::String("preserve".to_string());
        let error = match arena.take_owned_with_value(
            test_handle(45),
            1,
            &SourceResourceKind::LinesStdin,
            value.clone(),
        ) {
            Err(error) => error,
            Ok(_) => panic!("stale owner was accepted"),
        };
        assert_eq!(error.value, value);
    }

    #[test]
    fn failed_adoption_returns_complete_owned_value() {
        let arena = SourceResourceArena::new();
        let source = arena.register_stdin(test_handle(46)).expect("admit source owner");
        let (_, lease) = arena
            .take_owned(
                source.handle,
                source.raw,
                &SourceResourceKind::LinesStdin,
            )
            .expect("take source owner");
        let owned = lease.with_value(MirRuntimeValue::String("rollback".to_string()));
        arena
            .state
            .lock()
            .expect("lock source arena")
            .next_slot = u32::MAX;
        let outcome = arena.adopt_owned(
            test_handle(47),
            &SourceResourceKind::LinesStdin,
            owned,
        );
        let failure = outcome.result.expect_err("destination must reject");
        let (error, owned) = failure.into_parts();
        assert!(matches!(error, MirNativeCursorError::Internal(_)));
        assert_eq!(owned.value(), &MirRuntimeValue::String("rollback".to_string()));
        drop(owned);
    }

    #[test]
    fn owned_payload_take_carrier_adopt_commits_and_stales_source() {
        let arena = SourceResourceArena::new();
        let source = arena.register_stdin(test_handle(40)).expect("admit stdin owner");
        let (consumed, lease) = arena
            .take_owned(
                source.handle,
                source.raw,
                &SourceResourceKind::LinesStdin,
            )
            .expect("take sole source owner");
        assert_eq!(consumed, source);
        let carrier = lease
            .with_value(MirRuntimeValue::String("owned".to_string()))
            .into_runtime_value();
        let alias = carrier.clone();
        assert!(source_owned_from_runtime_value(alias).is_err());
        let owned = source_owned_from_runtime_value(carrier).expect("unwrap owned carrier");
        let outcome = arena.adopt_owned(
            test_handle(41),
            &SourceResourceKind::LinesStdin,
            owned,
        );
        assert_eq!(outcome.consumed, source);
        let (destination, value) = outcome.result.expect("commit destination owner");
        assert_eq!(value, MirRuntimeValue::String("owned".to_string()));
        assert!(arena
            .lookup_capability(source.handle, source.raw)
            .is_err());
        assert_eq!(
            arena
                .lookup_capability(destination.handle, destination.raw)
                .expect("lookup adopted owner")
                .kind,
            SourceResourceKind::LinesStdin
        );
        arena
            .release_capability(&destination)
            .expect("release adopted owner");
    }

    #[test]
    fn owned_payload_take_rejects_alias_and_preserves_source_slot() {
        let arena = SourceResourceArena::new();
        let source = arena.register_stdin(test_handle(42)).expect("admit stdin owner");
        let alias = arena
            .owner_for_raw(
                source.handle,
                source.raw,
                &SourceResourceKind::LinesStdin,
            )
            .expect("borrow source owner");
        assert!(arena
            .take_owned(
                source.handle,
                source.raw,
                &SourceResourceKind::LinesStdin,
            )
            .is_err());
        assert_eq!(
            arena
                .lookup_capability(source.handle, source.raw)
                .expect("source remains after alias rejection"),
            source
        );
        drop(alias);
        let (_, lease) = arena
            .take_owned(
                source.handle,
                source.raw,
                &SourceResourceKind::LinesStdin,
            )
            .expect("take after alias release");
        drop(lease);
    }
    #[test]
    fn retained_root_defers_retirement_until_last_lease_and_clears_slots() {
        let session = SourceResourceSession::new();
        let root = session.retain_root().expect("retain Source callback root");
        let arena = root.arena();
        let capability = arena
            .register_stdin(test_handle(51))
            .expect("register retained callback resource");

        session.retire().expect("retirement request");
        assert!(session.is_retirement_requested().expect("retirement state"));
        assert!(!arena.is_retired().expect("deferred retirement state"));
        assert!(arena.lookup_capability(capability.handle, capability.raw).is_ok());

        let alias = root.clone();
        drop(root);
        assert_eq!(session.retained_root_count().expect("root count"), 1);
        assert!(!arena.is_retired().expect("last alias still live"));
        drop(alias);

        assert_eq!(session.retained_root_count().expect("root count"), 0);
        assert!(arena.is_retired().expect("last root retires arena"));
        assert!(arena.lookup_capability(capability.handle, capability.raw).is_err());
    }

    #[test]
    fn retirement_request_rejects_new_roots_and_resources() {
        let session = SourceResourceSession::new();
        let root = session.retain_root().expect("retain Source callback root");
        let arena = root.arena();
        let existing = arena
            .register_stdin(test_handle(52))
            .expect("register existing callback resource");
        session.retire().expect("retirement request");

        assert!(session
            .retain_root()
            .is_err(), "retirement must reject unrelated new owners");
        assert!(arena
            .register_stdin(test_handle(53))
            .is_err(), "retirement must reject new resource slots");
        assert!(arena
            .lookup_capability(existing.handle, existing.raw)
            .is_ok(), "the existing root remains physically usable");
        assert!(!arena.is_retired().expect("retained root defers retirement"));

        drop(root);
        assert!(arena.is_retired().expect("last root retires arena"));
        assert!(arena
            .lookup_capability(existing.handle, existing.raw)
            .is_err());
    }

    #[test]
    fn last_root_release_allows_reentrant_arena_cleanup() {
        let session = SourceResourceSession::new();
        let root = session.retain_root().expect("retain Source callback root");
        let arena = root.arena();
        let capability = arena
            .register_stdin(test_handle(53))
            .expect("register reentrant cleanup resource");
        session.retire().expect("retirement request");

        let reentered_after_release = Arc::new(AtomicUsize::new(0));
        let cleanup = ReentrantLeaseDrop {
            lease: Some(root),
            arena: arena.clone(),
            reentered_after_release: reentered_after_release.clone(),
        };
        drop(cleanup);

        assert_eq!(
            reentered_after_release.load(Ordering::SeqCst),
            1,
            "last-root cleanup must unlock before reentrant arena access"
        );
        assert!(arena.is_retired().expect("retired arena"));
        assert!(arena.lookup_capability(capability.handle, capability.raw).is_err());
    }
}
