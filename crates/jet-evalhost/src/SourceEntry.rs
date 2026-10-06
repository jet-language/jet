//! Source evaluator entry: hands one checked MIR artifact entry to the
//! Source evaluator callback and owns the completion and retirement receipts
//! around it.
//!
//! The evaluator owns execution semantics. This module validates the entry
//! identity, activates the invocation's physical Source resource arena, scopes
//! the callback, and retires the logical session before physical resources.

use crate::SourceResources::{SourceResourceLease, SourceResourceRetireError, SourceResourceSession};
use jet_foundation::JitBackend::RunOutcome;
use jet_foundation::MIR::{
    MirArtifactId, MirExecutionIdentity, MirFunctionId, MirParam, MirProgram, MirRuntimeValue,
    MirType,
};
use std::cell::RefCell;

/// Failure in the Source entry handoff itself. These are compiler/host
/// invariant failures, not Source program diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceEntryError {
    InvalidRequest(String),
    Callback(String),
    CallbackJob(jet_foundation::Outcome::JetTaskFailure),
    Resource(String),
    NativeInterface(crate::SourceInterfaces::NativeInterfaceError),
    /// The callback contract is still invalid even when its owned guard
    /// retires; retain that cleanup evidence without treating it as success.
    MissingOutcome { cleanup: Option<String> },
}

impl std::fmt::Display for SourceEntryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(formatter, "invalid Source entry request: {message}"),
            Self::Callback(message) => write!(formatter, "Source entry callback failed: {message}"),
            Self::CallbackJob(failure) => write!(formatter, "Source callback owner job failed: {failure:?}"),
            Self::NativeInterface(error) => {
                write!(formatter, "Source native interface scope failed: {error}")
            }
            Self::Resource(message) => write!(formatter, "Source resource activation failed: {message}"),
            Self::MissingOutcome { cleanup } => {
                write!(formatter, "Source entry callback returned no terminal outcome")?;
                if let Some(cleanup) = cleanup {
                    write!(formatter, "; cleanup evidence: {cleanup}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for SourceEntryError {}

/// Typed request delivered to the Source evaluator for one checked entry.
/// Its identity comes from the checked MIR artifact; `entry_values` carry the
/// recursive canonical MIR values without flattening them into ABI words.
#[derive(Debug, Clone)]
pub struct SourceEntryRequest {
    pub execution: MirExecutionIdentity,
    pub function: MirFunctionId,
    /// Checked parameter facts copied from the artifact. The Source adapter
    /// uses these to validate recursive value shape and access/ownership mode
    /// before adopting values into its first frame.
    pub entry_params: Vec<MirParam>,
    /// Exact checked entry arguments, including recursive aggregates,
    /// closures, and typed resource-owned values.
    pub entry_values: Vec<MirRuntimeValue>,
}

/// Terminal result supplied by the Source evaluator.
pub struct SourceEntryReply {
    /// Typed terminal result. Diagnostics remain Foundation values and are
    /// never stringified by this seam.
    pub outcome: Option<RunOutcome>,
    /// Exact recursive terminal value returned by Source.
    pub value: Option<MirRuntimeValue>,
    /// Explicit logical Source/Eval session and callback/resource root.
    /// The Runner retains this through value adoption and calls `retire`.
    pub session: Option<Box<dyn SourceExecutionGuard>>,
    /// Source evaluator status retained alongside `RunOutcome`.
    pub soft_stop: bool,
    /// Cumulative output snapshots retained by Source; consumers replace,
    /// never append, these values to output derived from `RunOutcome`.
    pub stdout: String,
    pub stderr: String,
}

/// Final Source/Eval state after its owner has decoded/adopted the terminal
/// value and explicitly retired the retained machine, callback roots, and
/// resource-transfer ledger.
#[derive(Debug)]
pub struct SourceExecutionRetirement {
    pub outcome: Option<RunOutcome>,
    pub value: Option<MirRuntimeValue>,
    pub soft_stop: bool,
    /// Cumulative Source output snapshots, kept separate because Problems
    /// outcomes have no output fields.
    pub stdout: String,
    /// Matching cumulative stderr snapshot.
    pub stderr: String,
    /// Owned nested Source/shared-payload completions discharged while this
    /// invocation, projection, and retirement scope remained active.
    pub completions: Vec<SourceExecutionCompletion>,
}

/// Owned, transport-neutral logical Source/Eval session guard.
///
/// The generated compiler implements this trait around its retained
/// `JetEvalResult.session`, callback queue/root lease, and physical arena
/// owner. `retire` is explicit: cleanup may update output/error state.
pub trait SourceExecutionGuard {
    fn retire(self: Box<Self>) -> Result<SourceExecutionRetirement, SourceExecutionRetirementError>;
}

/// One linear completion emitted by a Source invocation. Not-invoked calls
/// own their exact checked MIR arguments; invoked calls own one retirement
/// result or the complete failure that retains it.
pub struct SourceExecutionCompletion {
    pub execution: Option<MirExecutionIdentity>,
    pub function: Option<MirFunctionId>,
    /// Counted physical root retained until the owned completion is consumed.
    pub lease: Option<SourceResourceLease>,
    pub disposition: SourceExecutionCompletionDisposition,
}

impl SourceExecutionCompletion {
    /// Return a projected result to its original linear completion after the
    /// consumer has inspected it. A conflicting owner is returned unchanged.
    pub fn restore_projected_value(
        &mut self,
        value: MirRuntimeValue,
    ) -> Result<(), MirRuntimeValue> {
        let SourceExecutionCompletionDisposition::Invoked { retirement, .. } =
            &mut self.disposition
        else {
            return Err(value);
        };
        let result = match retirement.get_or_insert_with(|| {
            SourceExecutionCompletionRetirement::Completed(SourceExecutionRetirement {
                outcome: None,
                value: None,
                soft_stop: false,
                stdout: String::new(),
                stderr: String::new(),
                completions: Vec::new(),
            })
        }) {
            SourceExecutionCompletionRetirement::Completed(result) => result,
            SourceExecutionCompletionRetirement::Failed(error) => &mut error.retirement,
        };
        if result.value.is_some() {
            return Err(value);
        }
        result.value = Some(value);
        Ok(())
    }
}

impl std::fmt::Debug for SourceExecutionCompletion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SourceExecutionCompletion")
            .field("execution", &self.execution)
            .field("function", &self.function)
            .field("retains_lease", &self.lease.is_some())
            .field("disposition", &self.disposition)
            .finish()
    }
}

#[derive(Debug)]
pub enum SourceExecutionCompletionDisposition {
    NotInvoked {
        cause: SourceEntryError,
        entry_values: Vec<MirRuntimeValue>,
        write_borrow_indices: Vec<usize>,
        completions: Vec<SourceExecutionCompletion>,
    },
    Invoked {
        cause: Option<SourceEntryError>,
        retirement: Option<SourceExecutionCompletionRetirement>,
        writebacks: Vec<SourceExecutionCompletionWriteback>,
    },
}

#[derive(Debug)]
pub enum SourceExecutionCompletionRetirement {
    Completed(SourceExecutionRetirement),
    Failed(SourceExecutionRetirementError),
}

#[derive(Debug)]
pub struct SourceExecutionCompletionWriteback {
    pub parameter_index: usize,
    pub checked_type: MirType,
}
pub type SourceExecutionCompletionBox = Box<dyn std::any::Any + Send>;

pub fn source_execution_completion_take_from_box(
    completion: SourceExecutionCompletionBox,
) -> Result<SourceExecutionCompletion, SourceExecutionCompletionBox> {
    completion
        .downcast::<SourceExecutionCompletion>()
        .map(|completion| *completion)
}

#[derive(Clone, Debug, Default)]
pub struct SourceExecutionCompletionScope {
    inner: std::sync::Arc<std::sync::Mutex<Vec<SourceExecutionCompletion>>>,
}
#[derive(Clone, Debug)]
pub struct SourceExecutionCompletionScopeWeak {
    inner: std::sync::Weak<std::sync::Mutex<Vec<SourceExecutionCompletion>>>,
}

fn record_source_execution_completion(
    target: &std::sync::Mutex<Vec<SourceExecutionCompletion>>,
    completion: SourceExecutionCompletion,
) {
    target
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(completion);
}

fn record_source_execution_completion_box(
    target: &std::sync::Arc<std::sync::Mutex<Vec<SourceExecutionCompletion>>>,
    completion: SourceExecutionCompletionBox,
) -> Result<(), SourceExecutionCompletionBox> {
    let completion = match source_execution_completion_take_from_box(completion) {
        Ok(completion) => completion,
        Err(completion) => return Err(completion),
    };
    record_source_execution_completion(target, completion);
    Ok(())
}

impl SourceExecutionCompletionScopeWeak {
    pub fn upgrade(&self) -> Option<SourceExecutionCompletionScope> {
        self.inner
            .upgrade()
            .map(|inner| SourceExecutionCompletionScope { inner })
    }

    /// Record only to this explicit weak target. If its owner has gone away,
    /// return the exact opaque packet for its caller to propagate.
    pub fn record_box(
        &self,
        completion: SourceExecutionCompletionBox,
    ) -> Result<(), SourceExecutionCompletionBox> {
        let Some(target) = self.inner.upgrade() else {
            return Err(completion);
        };
        record_source_execution_completion_box(&target, completion)
    }
}

thread_local! {
    static SOURCE_EXECUTION_COMPLETION_SCOPES: RefCell<
        Vec<std::sync::Arc<std::sync::Mutex<Vec<SourceExecutionCompletion>>>>
    > = const { RefCell::new(Vec::new()) };
}

struct CurrentSourceExecutionCompletionScope {
    inner: std::sync::Arc<std::sync::Mutex<Vec<SourceExecutionCompletion>>>,
}

impl Drop for CurrentSourceExecutionCompletionScope {
    fn drop(&mut self) {
        let _ = SOURCE_EXECUTION_COMPLETION_SCOPES.try_with(|scopes| {
            let mut scopes = scopes.borrow_mut();
            let Some(current) = scopes.pop() else {
                return;
            };
            if !std::sync::Arc::ptr_eq(&current, &self.inner) {
                scopes.push(current);
            }
        });
    }
}

impl SourceExecutionCompletionScope {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn downgrade(&self) -> SourceExecutionCompletionScopeWeak {
        SourceExecutionCompletionScopeWeak {
            inner: std::sync::Arc::downgrade(&self.inner),
        }
    }
    /// Consume a typed completion into this explicit owner. This does not
    /// depend on activation or a type-erased downcast across a library boundary.
    pub fn record(&self, completion: SourceExecutionCompletion) {
        record_source_execution_completion(&self.inner, completion);
    }

    /// Make this owned invocation scope current only for the synchronous
    /// callback body. The guard never escapes the call or binds the scope to a
    /// thread after return.
    pub fn with_current<R>(&self, body: impl FnOnce() -> R) -> R {
        SOURCE_EXECUTION_COMPLETION_SCOPES.with(|scopes| {
            scopes.borrow_mut().push(self.inner.clone());
        });
        let _current = CurrentSourceExecutionCompletionScope {
            inner: self.inner.clone(),
        };
        body()
    }

    fn current_inner() -> Option<
        std::sync::Arc<std::sync::Mutex<Vec<SourceExecutionCompletion>>>,
    > {
        SOURCE_EXECUTION_COMPLETION_SCOPES
            .try_with(|scopes| scopes.borrow().last().cloned())
            .ok()
            .flatten()
    }

    /// Clone the exact active owner so a deferred finalizer can keep recording
    /// to its parent after this synchronous activation has returned.
    pub fn current() -> Option<Self> {
        Some(Self {
            inner: Self::current_inner()?,
        })
    }
    /// Record to this explicit strong target, independent of thread-local
    /// activation. A bad boxed type is returned untouched.
    pub fn record_box(
        &self,
        completion: SourceExecutionCompletionBox,
    ) -> Result<(), SourceExecutionCompletionBox> {
        record_source_execution_completion_box(&self.inner, completion)
    }

    /// Record on the current invocation. If there is no active owner, return
    /// the exact packet to the caller for explicit propagation.
    pub fn record_current(
        completion: SourceExecutionCompletion,
    ) -> Result<(), SourceExecutionCompletion> {
        let Some(current) = Self::current_inner() else {
            return Err(completion);
        };
        record_source_execution_completion(&current, completion);
        Ok(())
    }

    /// Take an opaque owned completion. A missing active scope returns the
    /// exact `Box` untouched; a failed checked downcast does the same.
    pub fn record_current_box(
        completion: SourceExecutionCompletionBox,
    ) -> Result<(), SourceExecutionCompletionBox> {
        let Some(current) = Self::current_inner() else {
            return Err(completion);
        };
        record_source_execution_completion_box(&current, completion)
    }

    pub fn drain(&self) -> Vec<SourceExecutionCompletion> {
        std::mem::take(
            &mut *self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

/// One checked Source entry execution, including the physical invocation
/// root and the logical session returned by the Source callback.
pub struct SourceEntryExecution {
    pub outcome: RunOutcome,
    pub value: Option<MirRuntimeValue>,
    pub soft_stop: bool,
    /// Cumulative Source output snapshot; never append this to `RunOutcome`.
    pub stdout: String,
    pub stderr: String,
    pub resources: SourceResourceSession,
    pub session: Option<Box<dyn SourceExecutionGuard>>,
}

fn merge_source_execution_retirement(
    fallback: SourceExecutionRetirement,
    retirement: SourceExecutionRetirement,
) -> SourceExecutionRetirement {
    let mut completions = fallback.completions;
    completions.extend(retirement.completions);
    SourceExecutionRetirement {
        outcome: retirement.outcome.or(fallback.outcome),
        value: retirement.value.or(fallback.value),
        soft_stop: retirement.soft_stop,
        stdout: retirement.stdout,
        stderr: retirement.stderr,
        completions,
    }
}

/// Which cleanup phase failed after the Source execution produced its result.
#[derive(Debug)]
pub enum SourceExecutionRetirementFailureKind {
    Session(String),
    Resource(SourceResourceRetireError),
}

/// A retirement failure owns the pending execution result and whichever
/// physical resource owner is required to keep it valid until projection.
pub struct SourceExecutionRetirementError {
    failure: SourceExecutionRetirementFailureKind,
    retirement: SourceExecutionRetirement,
    resources: Option<SourceResourceSession>,
    lease: Option<SourceResourceLease>,
}

impl SourceExecutionRetirementError {
    pub fn new(
        failure: SourceExecutionRetirementFailureKind,
        retirement: SourceExecutionRetirement,
        resources: Option<SourceResourceSession>,
        lease: Option<SourceResourceLease>,
    ) -> Self {
        Self {
            failure,
            retirement,
            resources,
            lease,
        }
    }

    pub fn failure(&self) -> &SourceExecutionRetirementFailureKind {
        &self.failure
    }

    pub fn retirement(&self) -> &SourceExecutionRetirement {
        &self.retirement
    }

    pub fn into_parts(
        self,
    ) -> (
        SourceExecutionRetirementFailureKind,
        SourceExecutionRetirement,
        Option<SourceResourceSession>,
        Option<SourceResourceLease>,
    ) {
        (self.failure, self.retirement, self.resources, self.lease)
    }
}

impl std::fmt::Debug for SourceExecutionRetirementError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SourceExecutionRetirementError")
            .field("failure", &self.failure)
            .field("retirement", &self.retirement)
            .field("retains_resources", &self.resources.is_some())
            .field("retains_lease", &self.lease.is_some())
            .finish()
    }
}

impl std::fmt::Display for SourceExecutionRetirementError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.failure {
            SourceExecutionRetirementFailureKind::Session(message) => {
                write!(formatter, "Source session retirement failed: {message}")
            }
            SourceExecutionRetirementFailureKind::Resource(error) => {
                write!(formatter, "Source resource retirement failed: {}", error.error)
            }
        }
    }
}

impl std::error::Error for SourceExecutionRetirementError {}

/// Retire the logical Source session before physical resources. Failed cleanup
/// returns the pending output together with the owner that keeps it usable.
pub fn retire_source_entry(
    resources: SourceResourceSession,
    session: Option<Box<dyn SourceExecutionGuard>>,
    fallback: SourceExecutionRetirement,
) -> Result<SourceExecutionRetirement, SourceExecutionRetirementError> {
    let Some(session) = session else {
        return match resources.retire() {
            Ok(completions) => {
                let mut fallback = fallback;
                fallback.completions.extend(completions);
                Ok(fallback)
            }
            Err(error) => Err(SourceExecutionRetirementError::new(
                SourceExecutionRetirementFailureKind::Resource(error),
                fallback,
                Some(resources),
                None,
            )),
        };
    };
    let _activation = resources.activate();
    let retirement = match session.retire() {
        Ok(retirement) => retirement,
        Err(mut error) => {
            error.retirement = merge_source_execution_retirement(fallback, error.retirement);
            if error.resources.is_none() {
                error.resources = Some(resources);
            }
            return Err(error);
        }
    };
    let mut retirement = merge_source_execution_retirement(fallback, retirement);
    match resources.retire() {
        Ok(completions) => {
            retirement.completions.extend(completions);
            Ok(retirement)
        }
        Err(error) => Err(SourceExecutionRetirementError::new(
            SourceExecutionRetirementFailureKind::Resource(error),
            retirement,
            Some(resources),
            None,
        )),
    }
}

impl SourceEntryExecution {
    /// Retire this execution after the caller has adopted its returned value.
    pub fn retire(self) -> Result<SourceExecutionRetirement, SourceExecutionRetirementError> {
        retire_source_entry(
            self.resources,
            self.session,
            SourceExecutionRetirement {
                outcome: Some(self.outcome),
                value: self.value,
                soft_stop: self.soft_stop,
                stdout: self.stdout,
                stderr: self.stderr,
                completions: Vec::new(),
            },
        )
    }
}

fn source_callback_panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|message| (*message).to_string()))
        .unwrap_or_else(|| "unknown panic payload".to_string())
}

/// Execute one checked whole-entry request through the Source evaluator
/// callback `resume`. This is the shared entry executor used by both
/// generated bootstrap glue and the RuntimeMir adapter; it owns callback
/// activation and the physical Source arena lease.
pub fn execute_source_entry<Resume>(
    program: &MirProgram,
    artifact: MirArtifactId,
    entry_function: MirFunctionId,
    resources: SourceResourceSession,
    entry_values: Vec<MirRuntimeValue>,
    resume: Resume,
) -> Result<SourceEntryExecution, SourceEntryError>
where
    Resume: Fn(&mut SourceEntryRequest) -> Result<SourceEntryReply, String>,
{
    program.validate().map_err(|error| {
        SourceEntryError::InvalidRequest(format!("checked Source MIR is invalid: {error}"))
    })?;
    let artifact_entry = program
        .artifacts
        .iter()
        .find(|candidate| candidate.id == artifact)
        .and_then(|candidate| candidate.entry.as_ref())
        .and_then(|entry| entry.function)
        .ok_or_else(|| {
            SourceEntryError::InvalidRequest(format!(
                "artifact {artifact:?} has no checked entry function"
            ))
        })?;
    if artifact_entry != entry_function {
        return Err(SourceEntryError::InvalidRequest(format!(
            "Source entry function {entry_function:?} does not match artifact {artifact:?} entry {artifact_entry:?}"
        )));
    }
    let function = program
        .functions
        .iter()
        .find(|candidate| candidate.id == entry_function)
        .ok_or_else(|| {
            SourceEntryError::InvalidRequest(format!(
                "Source entry function {entry_function:?} is missing"
            ))
        })?;
    if !function.target_applicability.interpreter {
        return Err(SourceEntryError::InvalidRequest(format!(
            "Source entry function {entry_function:?} is not executable by the Source evaluator"
        )));
    }
    if function.params.len() != entry_values.len() {
        return Err(SourceEntryError::InvalidRequest(format!(
            "Source entry expected {} checked parameters, got {} typed values",
            function.params.len(),
            entry_values.len()
        )));
    }
    let execution = program.execution_identity(Some(artifact)).map_err(|error| {
        SourceEntryError::InvalidRequest(format!("Source entry identity unavailable: {error}"))
    })?;
    if resources
        .arena()
        .is_retired()
        .map_err(SourceEntryError::Resource)?
    {
        return Err(SourceEntryError::Resource(
            "Source resource session is retired".to_string(),
        ));
    }
    let mut request = SourceEntryRequest {
        execution,
        function: entry_function,
        entry_params: function.params.clone(),
        entry_values,
    };
    let _activation = resources.activate();
    let reply = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        resume(&mut request)
    })) {
        Ok(Ok(reply)) => reply,
        Ok(Err(message)) => return Err(SourceEntryError::Callback(message)),
        Err(payload) => {
            return Err(SourceEntryError::Callback(format!(
                "Source entry callback panicked: {}",
                source_callback_panic_message(payload.as_ref())
            )))
        }
    };
    let SourceEntryReply {
        outcome,
        value,
        session,
        soft_stop,
        stdout,
        stderr,
    } = reply;
    let Some(outcome) = outcome else {
        return Err(SourceEntryError::MissingOutcome { cleanup: None });
    };
    Ok(SourceEntryExecution {
        outcome,
        value,
        soft_stop,
        stdout,
        stderr,
        resources,
        session,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weak_completion_target_records_explicitly_and_returns_box_when_expired() {
        let target = SourceExecutionCompletionScope::new();
        let weak = target.downgrade();
        let completion = SourceExecutionCompletion {
            execution: None,
            function: Some(MirFunctionId(7)),
            lease: None,
            disposition: SourceExecutionCompletionDisposition::NotInvoked {
                cause: SourceEntryError::InvalidRequest("preflight".to_string()),
                entry_values: Vec::new(),
                write_borrow_indices: Vec::new(),
                completions: Vec::new(),
            },
        };
        let boxed: SourceExecutionCompletionBox = Box::new(completion);
        assert!(weak.record_box(boxed).is_ok());
        let mut recorded = target.drain();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded.pop().and_then(|packet| packet.function), Some(MirFunctionId(7)));

        let expired = SourceExecutionCompletionScope::new().downgrade();
        assert!(expired.upgrade().is_none());
        let opaque: SourceExecutionCompletionBox = Box::new(String::from("owned"));
        let payload = opaque.downcast_ref::<String>().expect("boxed payload");
        let payload_pointer = payload.as_ptr();
        let returned = expired.record_box(opaque).unwrap_err();
        let returned_payload = returned.downcast_ref::<String>().expect("returned payload");
        assert_eq!(returned_payload.as_ptr(), payload_pointer);
    }

    struct CountedSourceValue {
        marker: i64,
        drops: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    }

    impl Drop for CountedSourceValue {
        fn drop(&mut self) {
            self.drops
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn counted_source_value(
        marker: i64,
        drops: &std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> MirRuntimeValue {
        MirRuntimeValue::NativeOwned(jet_foundation::MIR::MirNativeOwned::new(
            CountedSourceValue {
                marker,
                drops: drops.clone(),
            },
        ))
    }

    struct FailedSourceGuard;

    impl SourceExecutionGuard for FailedSourceGuard {
        fn retire(
            self: Box<Self>,
        ) -> Result<SourceExecutionRetirement, SourceExecutionRetirementError> {
            Err(SourceExecutionRetirementError::new(
                SourceExecutionRetirementFailureKind::Session(
                    "test session cleanup failure".to_string(),
                ),
                SourceExecutionRetirement {
                    outcome: None,
                    value: None,
                    soft_stop: false,
                    stdout: String::new(),
                    stderr: String::new(),
                    completions: Vec::new(),
                },
                None,
                None,
            ))
        }
    }

    #[test]
    fn session_retirement_failure_keeps_pending_result_and_resource_owner() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let drops = Arc::new(AtomicUsize::new(0));
        let resources = SourceResourceSession::new();
        let arena = resources.arena();
        let fallback = SourceExecutionRetirement {
            outcome: None,
            value: Some(counted_source_value(123, &drops)),
            soft_stop: false,
            stdout: "pending".to_string(),
            stderr: String::new(),
            completions: Vec::new(),
        };
        let error = retire_source_entry(resources, Some(Box::new(FailedSourceGuard)), fallback)
            .expect_err("failed session retirement must preserve result");
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert!(!arena.is_retired().expect("error owns the resource session"));

        let (failure, pending, retained_resources, retained_lease) = error.into_parts();
        assert!(matches!(
            failure,
            SourceExecutionRetirementFailureKind::Session(ref message)
                if message == "test session cleanup failure"
        ));
        assert!(retained_resources.is_some());
        assert!(retained_lease.is_none());
        assert_eq!(pending.stdout, "pending");
        let MirRuntimeValue::NativeOwned(value) =
            pending.value.as_ref().expect("pending updated value")
        else {
            panic!("pending output root must remain native-owned");
        };
        assert_eq!(
            value
                .downcast_ref::<CountedSourceValue>()
                .expect("pending source value type")
                .marker,
            123
        );
        drop(pending);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        drop(retained_resources);
        drop(retained_lease);
    }
}
