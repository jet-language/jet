use jet_foundation::{
    JitBackend::{JitBackend, RunOutcome},
    MIR::{
        MirAbi, MirAccess, MirArtifactId, MirExecutionIdentity, MirFunction, MirFunctionId,
        MirParam, MirProgram, MirRuntimeValue, MirType, MirTypeKind,
    },
};
use crate::SourceResources::{
    activate_source_resource_arena, SourceResourceArena, SourceResourceLease,
    SourceResourceRetireError, SourceResourceSession,
};
use jet_pkg_model::Package::ReleaseDevtoolsPolicy;

use super::api_debug::{
    cranelift_host_supported, try_resident, try_resident_hot_swap, try_resident_restart,
    try_resident_with_function_values_and_result, try_resident_with_values_and_result,
};
use super::tiers::{record_trace, MirTierPlan};
use super::trace::note_deopt_invoked_for_test;
use super::types_meta::clif_ty_from_mir;

/// Failure in the Source entry handoff itself. These are compiler/host
/// invariant failures, not Source program diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceDeoptError {
    InvalidRequest(String),
    Callback(String),
    CallbackJob(jet_foundation::Outcome::JetTaskFailure),
    Backend(String),
    Resource(String),
    NativeInterface(crate::SourceInterfaces::NativeInterfaceError),
    /// The callback contract is still invalid even when its owned guard
    /// retires; retain that cleanup evidence without treating it as success.
    MissingOutcome { cleanup: Option<String> },
}

impl std::fmt::Display for SourceDeoptError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidRequest(message) => write!(formatter, "invalid Source entry request: {message}"),
            Self::Callback(message) => write!(formatter, "Source entry callback failed: {message}"),
            Self::CallbackJob(failure) => write!(formatter, "Source callback owner job failed: {failure:?}"),
            Self::Backend(message) => write!(formatter, "Source native backend failed: {message}"),
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

impl std::error::Error for SourceDeoptError {}

/// One linear completion emitted by a helper invocation. Not-invoked calls
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
            SourceExecutionCompletionRetirement::Completed(
                super::deopt::SourceExecutionRetirement {
                    outcome: None,
                    value: None,
                    soft_stop: false,
                    stdout: String::new(),
                    stderr: String::new(),
                    completions: Vec::new(),
                },
            )
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
        cause: SourceDeoptError,
        entry_values: Vec<MirRuntimeValue>,
        write_borrow_indices: Vec<usize>,
        completions: Vec<SourceExecutionCompletion>,
    },
    Invoked {
        cause: Option<SourceDeoptError>,
        tier: Option<SourceExecutionTier>,
        retirement: Option<SourceExecutionCompletionRetirement>,
        writebacks: Vec<SourceExecutionCompletionWriteback>,
    },
}

#[derive(Debug)]
pub enum SourceExecutionCompletionRetirement {
    Completed(super::deopt::SourceExecutionRetirement),
    Failed(SourceExecutionRetirementError),
}

#[derive(Debug)]
pub struct SourceExecutionCompletionWriteback {
    pub parameter_index: usize,
    pub checked_type: MirType,
}
pub type SourceExecutionCompletionBox = Box<dyn std::any::Any + Send>;

pub fn source_execution_completion_into_box(
    completion: SourceExecutionCompletion,
) -> SourceExecutionCompletionBox {
    Box::new(completion)
}

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
    static SOURCE_EXECUTION_COMPLETION_SCOPES: std::cell::RefCell<
        Vec<std::sync::Arc<std::sync::Mutex<Vec<SourceExecutionCompletion>>>>
    > = const { std::cell::RefCell::new(Vec::new()) };
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

/// One helper argument. Checked `MirAccess::Write` parameters must use
/// `WriteBorrow`, so the canonical executor can write the complete updated
/// MIR value back to the caller's live slot without cloning it.
pub enum SourceHelperArgument<'a> {
    Owned(MirRuntimeValue),
    WriteBorrow(&'a mut MirRuntimeValue),
}

/// A checked write-argument slot after the helper has restored it in place.
pub struct SourceHelperArgumentWriteback<'a> {
    pub parameter_index: usize,
    pub checked_type: MirType,
    pub value: &'a MirRuntimeValue,
}

struct SourceHelperWriteBorrow<'a> {
    parameter_index: usize,
    checked_type: MirType,
    slot: &'a mut MirRuntimeValue,
}

struct SourceHelperArgumentBuffer<'a> {
    values: Option<Vec<MirRuntimeValue>>,
    write_borrows: Vec<SourceHelperWriteBorrow<'a>>,
    restored: bool,
}

impl<'a> SourceHelperArgumentBuffer<'a> {
    fn validate(
        arguments: &[SourceHelperArgument<'a>],
        parameters: &[MirParam],
    ) -> Result<(), String> {
        if arguments.len() != parameters.len() {
            return Err(format!(
                "Source helper expected {} checked parameters, got {} arguments",
                parameters.len(),
                arguments.len()
            ));
        }
        for (index, (argument, parameter)) in arguments.iter().zip(parameters).enumerate() {
            let (value, access_matches) = match argument {
                SourceHelperArgument::Owned(value) => {
                    (value, parameter.access != MirAccess::Write)
                }
                SourceHelperArgument::WriteBorrow(value) => {
                    (&**value, parameter.access == MirAccess::Write)
                }
            };
            if !access_matches {
                return Err(format!(
                    "Source helper argument {index} does not use the checked {:?} access mode",
                    parameter.access
                ));
            }
            if !crate::SourceInterfaces::runtime_value_matches_type(value, &parameter.ty) {
                return Err(format!(
                    "Source helper argument {index} does not match its checked MIR type"
                ));
            }
        }
        Ok(())
    }

    fn new(arguments: Vec<SourceHelperArgument<'a>>, parameters: &[MirParam]) -> Self {
        let write_borrow_count = arguments
            .iter()
            .filter(|argument| matches!(argument, SourceHelperArgument::WriteBorrow(_)))
            .count();
        let mut values = Vec::with_capacity(arguments.len());
        let mut write_borrows = Vec::with_capacity(write_borrow_count);
        for (parameter_index, (argument, parameter)) in
            arguments.into_iter().zip(parameters).enumerate()
        {
            match argument {
                SourceHelperArgument::Owned(value) => values.push(value),
                SourceHelperArgument::WriteBorrow(slot) => {
                    values.push(std::mem::replace(slot, MirRuntimeValue::Moved));
                    write_borrows.push(SourceHelperWriteBorrow {
                        parameter_index,
                        checked_type: parameter.ty.clone(),
                        slot,
                    });
                }
            }
        }
        Self {
            values: Some(values),
            write_borrows,
            restored: false,
        }
    }

    fn values_mut(&mut self) -> &mut Option<Vec<MirRuntimeValue>> {
        &mut self.values
    }

    fn recover_not_invoked(mut self) -> (Vec<MirRuntimeValue>, Vec<usize>) {
        self.restore_write_borrows();
        let values = self
            .values
            .take()
            .expect("not-invoked helper retains its checked input values");
        let mut entry_values =
            Vec::with_capacity(values.len().saturating_sub(self.write_borrows.len()));
        let mut borrowed = Vec::with_capacity(self.write_borrows.len());
        let mut write_borrow = self.write_borrows.iter().peekable();
        for (index, value) in values.into_iter().enumerate() {
            if write_borrow
                .peek()
                .is_some_and(|borrow| borrow.parameter_index == index)
            {
                borrowed.push(index);
                write_borrow.next();
            } else {
                entry_values.push(value);
            }
        }
        borrowed.extend(
            write_borrow.map(|write_borrow| write_borrow.parameter_index),
        );
        (entry_values, borrowed)
    }

    fn complete(
        mut self,
        writeback_indices: &[usize],
    ) -> Vec<SourceHelperArgumentWriteback<'a>> {
        self.restore_write_borrows();
        let write_borrows = std::mem::take(&mut self.write_borrows);
        self.restored = true;
        write_borrows
            .into_iter()
            .filter(|slot| writeback_indices.contains(&slot.parameter_index))
            .map(|slot| SourceHelperArgumentWriteback {
                parameter_index: slot.parameter_index,
                checked_type: slot.checked_type,
                value: &*slot.slot,
            })
            .collect()
    }

    fn restore_write_borrows(&mut self) {
        if self.restored {
            return;
        }
        let Some(values) = self.values.as_mut() else {
            return;
        };
        for write_borrow in &mut self.write_borrows {
            let Some(value) = values.get_mut(write_borrow.parameter_index) else {
                continue;
            };
            *write_borrow.slot = std::mem::replace(value, MirRuntimeValue::Moved);
        }
        self.restored = true;
    }
}

impl Drop for SourceHelperArgumentBuffer<'_> {
    fn drop(&mut self) {
        self.restore_write_borrows();
    }
}

/// Ownership returned when a Source-helper attempt fails.
///
/// `NotInvoked` retains exact owned inputs and identifies unchanged borrowed
/// slots. `Invoked` never carries retry inputs; it exposes only checked
/// writebacks from the entered invocation.
pub enum SourceHelperInvocationFailure<'a> {
    NotInvoked {
        cause: SourceDeoptError,
        entry_values: Vec<MirRuntimeValue>,
        write_borrow_indices: Vec<usize>,
    },
    Invoked {
        cause: SourceDeoptError,
        tier: Option<SourceExecutionTier>,
        retirement: Option<super::deopt::SourceExecutionRetirement>,
        session: Option<Box<dyn super::deopt::SourceExecutionGuard>>,
        writebacks: Vec<SourceHelperArgumentWriteback<'a>>,
    },
}

impl std::fmt::Debug for SourceHelperInvocationFailure<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInvoked {
                cause,
                entry_values,
                write_borrow_indices,
            } => f
                .debug_struct("NotInvoked")
                .field("cause", cause)
                .field("entry_values", entry_values)
                .field("write_borrow_indices", write_borrow_indices)
                .finish(),
            Self::Invoked {
                cause,
                tier,
                retirement,
                session,
                writebacks,
            } => f
                .debug_struct("Invoked")
                .field("cause", cause)
                .field("tier", tier)
                .field("retirement", retirement)
                .field("session", &session.is_some())
                .field("writebacks", &writebacks.len())
                .finish(),
        }
    }
}

/// Failed helper invocation plus the counted root that keeps all carried
pub struct SourceHelperInvocationError<'a> {
    failure: SourceHelperInvocationFailure<'a>,
    lease: SourceResourceLease,
    completion_scope: SourceExecutionCompletionScope,
    native_interface_scope: Option<crate::SourceInterfaces::NativeInterfaceCheckedScope>,
    native_interface_context: Option<
        std::rc::Rc<crate::SourceInterfaces::NativeInterfacePhysicalBorrowContext>,
    >,
    execution: Option<MirExecutionIdentity>,
    function: Option<MirFunctionId>,
}

impl<'a> SourceHelperInvocationError<'a> {
    pub fn not_invoked(
        cause: SourceDeoptError,
        entry_values: Vec<MirRuntimeValue>,
        lease: SourceResourceLease,
    ) -> Self {
        Self::not_invoked_with_write_borrows(
            cause,
            entry_values,
            Vec::new(),
            lease,
            SourceExecutionCompletionScope::new(),
            None,
            None,
            None,
            None,
        )
    }

    fn not_invoked_with_write_borrows(
        cause: SourceDeoptError,
        entry_values: Vec<MirRuntimeValue>,
        write_borrow_indices: Vec<usize>,
        lease: SourceResourceLease,
        completion_scope: SourceExecutionCompletionScope,
        native_interface_scope: Option<
            crate::SourceInterfaces::NativeInterfaceCheckedScope,
        >,
        native_interface_context: Option<
            std::rc::Rc<crate::SourceInterfaces::NativeInterfacePhysicalBorrowContext>,
        >,
        execution: Option<MirExecutionIdentity>,
        function: Option<MirFunctionId>,
    ) -> Self {
        Self {
            native_interface_scope,
            failure: SourceHelperInvocationFailure::NotInvoked {
                cause,
                entry_values,
                write_borrow_indices,
            },
            lease,
            completion_scope,
            native_interface_context,
            execution,
            function,
        }
    }


    pub fn failure(&self) -> &SourceHelperInvocationFailure<'a> {
        &self.failure
    }

    pub fn entry_values(&self) -> Option<&[MirRuntimeValue]> {
        match &self.failure {
            SourceHelperInvocationFailure::NotInvoked { entry_values, .. } => Some(entry_values),
            SourceHelperInvocationFailure::Invoked { .. } => None,
        }
    }

    pub fn writebacks(&self) -> &[SourceHelperArgumentWriteback<'a>] {
        match &self.failure {
            SourceHelperInvocationFailure::NotInvoked { .. } => &[],
            SourceHelperInvocationFailure::Invoked { writebacks, .. } => writebacks,
        }
    }


    pub fn into_completion(self) -> SourceExecutionCompletion {
        let SourceHelperInvocationError {
            failure,
            lease,
            completion_scope,
            native_interface_scope,
            native_interface_context,
            execution,
            function,
        } = self;
        let (cause, tier, retirement, writebacks, lease) = match failure {
            SourceHelperInvocationFailure::NotInvoked {
                cause,
                entry_values,
                write_borrow_indices,
            } => {
                return SourceExecutionCompletion {
                    execution,
                    function,
                    lease: Some(lease),
                    disposition: SourceExecutionCompletionDisposition::NotInvoked {
                        cause,
                        entry_values,
                        write_borrow_indices,
                        completions: completion_scope.drain(),
                    },
                };
            }
            SourceHelperInvocationFailure::Invoked {
                cause,
                tier,
                retirement,
                session,
                writebacks,
            } => {
                let retirement = match session {
                    Some(session) => {
                        let arena = lease.arena();
                        let _native_interface_activation = native_interface_scope.as_ref().map(
                            |scope| {
                                crate::SourceInterfaces::NativeInterfaceScope::activate_checked_scope(
                                    scope,
                                    native_interface_context.clone(),
                                )
                            },
                        );
                        let _activation = activate_source_resource_arena(&arena);
                        match completion_scope.with_current(|| session.retire()) {
                            Ok(completed) => {
                                let retirement = match retirement {
                                    Some(fallback) => {
                                        merge_source_execution_retirement(fallback, completed)
                                    }
                                    None => completed,
                                };
                                Some(SourceExecutionCompletionRetirement::Completed(retirement))
                            }
                            Err(mut error) => {
                                if let Some(fallback) = retirement {
                                    error.retirement =
                                        merge_source_execution_retirement(fallback, error.retirement);
                                }
                                Some(SourceExecutionCompletionRetirement::Failed(error))
                            }
                        }
                    }
                    None => retirement.map(SourceExecutionCompletionRetirement::Completed),
                };
                let mut nested_completions = completion_scope.drain();
                let retirement =
                    append_nested_completions(retirement, &mut nested_completions);
                let writebacks = writebacks
                    .into_iter()
                    .map(|writeback| SourceExecutionCompletionWriteback {
                        parameter_index: writeback.parameter_index,
                        checked_type: writeback.checked_type,
                    })
                    .collect();
                (Some(cause), tier, retirement, writebacks, Some(lease))
            }
        };
        SourceExecutionCompletion {
            execution,
            function,
            lease,
            disposition: SourceExecutionCompletionDisposition::Invoked {
                cause,
                tier,
                retirement,
                writebacks,
            },
        }
    }

    pub fn record_completion(self) -> Result<(), SourceExecutionCompletion> {
        SourceExecutionCompletionScope::record_current(self.into_completion())
    }

    pub fn completion_scope(&self) -> &SourceExecutionCompletionScope {
        &self.completion_scope
    }

    pub fn into_parts(
        self,
    ) -> (
        SourceHelperInvocationFailure<'a>,
        SourceResourceLease,
        SourceExecutionCompletionScope,
        Option<crate::SourceInterfaces::NativeInterfaceCheckedScope>,
        Option<
            std::rc::Rc<crate::SourceInterfaces::NativeInterfacePhysicalBorrowContext>,
        >,
    ) {
        (
            self.failure,
            self.lease,
            self.completion_scope,
            self.native_interface_scope,
            self.native_interface_context,
        )
    }
}

impl std::fmt::Debug for SourceHelperInvocationError<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SourceHelperInvocationError")
            .field("failure", &self.failure)
            .field("retains_lease", &true)
            .finish()
    }
}

impl std::fmt::Display for SourceHelperInvocationError<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.failure {
            SourceHelperInvocationFailure::NotInvoked { cause, .. }
            | SourceHelperInvocationFailure::Invoked { cause, .. } => {
                std::fmt::Display::fmt(cause, formatter)
            }
        }
    }
}

impl std::error::Error for SourceHelperInvocationError<'_> {}

enum SourceHelperRunFailure {
    NotInvoked {
        cause: SourceDeoptError,
    },
    Invoked {
        cause: SourceDeoptError,
        retirement: Option<super::deopt::SourceExecutionRetirement>,
        tier: Option<SourceExecutionTier>,
        session: Option<Box<dyn super::deopt::SourceExecutionGuard>>,
        writeback_indices: Vec<usize>,
    },
}

impl std::fmt::Debug for SourceHelperRunFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotInvoked { cause } => {
                f.debug_struct("NotInvoked").field("cause", cause).finish()
            }
            Self::Invoked {
                cause,
                retirement,
                tier,
                session,
                writeback_indices,
            } => f
                .debug_struct("Invoked")
                .field("cause", cause)
                .field("retirement", retirement)
                .field("tier", tier)
                .field("session", &session.is_some())
                .field("writeback_indices", writeback_indices)
                .finish(),
        }
    }
}

fn source_helper_failure_for_input(
    cause: SourceDeoptError,
    entry_values: &mut Option<Vec<MirRuntimeValue>>,
) -> SourceHelperRunFailure {
    match entry_values {
        Some(_) => SourceHelperRunFailure::NotInvoked { cause },
        None => SourceHelperRunFailure::Invoked {
            cause,
            retirement: None,
            tier: None,
            session: None,
            writeback_indices: Vec::new(),
        },
    }
}

fn native_helper_attempt_result(
    attempt: super::resident::ResidentHelperAttempt,
    function: &MirFunction,
    entry_values: &mut Option<Vec<MirRuntimeValue>>,
) -> Result<Option<SourceDeoptRun>, SourceHelperRunFailure> {
    let super::resident::ResidentHelperAttempt::Invoked {
        outcome,
        value,
        failure,
        writebacks,
    } = attempt
    else {
        return Ok(None);
    };
    let (writeback_indices, projection_failure) =
        apply_source_helper_writebacks(function, entry_values, writebacks);
    let failure = match (failure, projection_failure) {
        (Some(failure), Some(projection)) => {
            Some(format!("{failure}; writable output projection failed: {projection}"))
        }
        (Some(failure), None) => Some(failure),
        (None, Some(projection)) => {
            Some(format!("writable output projection failed: {projection}"))
        }
        (None, None) => None,
    };
    if failure.is_none() {
        if let Some(value) = value {
            return Ok(Some(SourceDeoptRun {
                outcome,
                value: Some(value),
                session: None,
                tier: SourceExecutionTier::Native,
                writeback_indices,
            }));
        }
    }
    let message = failure
        .unwrap_or_else(|| "native Source function returned no typed value".to_string());
    Err(SourceHelperRunFailure::Invoked {
        cause: SourceDeoptError::Backend(format!(
            "native Source function {:?} failed after invocation: {message}",
            function.id
        )),
        retirement: Some(super::deopt::SourceExecutionRetirement {
            outcome: Some(outcome),
            value,
            soft_stop: false,
            stdout: String::new(),
            stderr: String::new(),
            completions: Vec::new(),
        }),
        session: None,
        tier: Some(SourceExecutionTier::Native),
        writeback_indices,
    })
}

fn apply_source_helper_writebacks(
    function: &MirFunction,
    entry_values: &mut Option<Vec<MirRuntimeValue>>,
    writebacks: Vec<super::resident::ResidentHelperWriteback>,
) -> (Vec<usize>, Option<String>) {
    let Some(values) = entry_values.as_mut() else {
        return (
            Vec::new(),
            Some("typed writable outputs have no retained entry argument vector".to_string()),
        );
    };
    let mut failure = None;
    let mut valid = Vec::with_capacity(writebacks.len());
    for writeback in writebacks {
        let Some(parameter) = function.params.get(writeback.parameter_index) else {
            if failure.is_none() {
                failure = Some(format!(
                    "writeback index {} is outside the checked parameter list",
                    writeback.parameter_index
                ));
            }
            continue;
        };
        if parameter.access != MirAccess::Write
            || !crate::SourceInterfaces::runtime_value_matches_type(&writeback.value, &parameter.ty)
        {
            if failure.is_none() {
                failure = Some(format!(
                    "writeback parameter {} does not match its checked writable type",
                    writeback.parameter_index
                ));
            }
            continue;
        }
        if writeback.parameter_index >= values.len() {
            if failure.is_none() {
                failure = Some(format!(
                    "writeback index {} is outside the retained entry arguments",
                    writeback.parameter_index
                ));
            }
            continue;
        }
        valid.push((writeback.parameter_index, writeback.value));
    }
    let indices = valid.iter().map(|(index, _)| *index).collect();
    for (index, value) in valid {
        values[index] = value;
    }
    (indices, failure)
}

fn source_entry_writeback_indices(
    function: &MirFunction,
    values: &[MirRuntimeValue],
) -> (Vec<usize>, Option<String>) {
    let mut indices = Vec::new();
    let mut failure = None;
    for (index, parameter) in function.params.iter().enumerate() {
        if parameter.access != MirAccess::Write {
            continue;
        }
        match values.get(index) {
            Some(value)
                if crate::SourceInterfaces::runtime_value_matches_type(value, &parameter.ty) =>
            {
                indices.push(index)
            }
            Some(_) => {
                if failure.is_none() {
                    failure = Some(format!(
                        "Source writeback parameter {index} does not match its checked MIR type"
                    ));
                }
            }
            None => {
                if failure.is_none() {
                    failure = Some(format!(
                        "Source writeback parameter {index} is missing from the entry values"
                    ));
                }
            }
        }
    }
    (indices, failure)
}

fn source_callback_panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|message| (*message).to_string()))
        .unwrap_or_else(|| "unknown panic payload".to_string())
}

/// Which tier consumed a typed Source entry invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceExecutionTier {
    Native,
    Source,
}

/// Policy fixed by the caller before a Source helper invocation. The actual
/// tier is still returned per invocation because native compilation may fail
/// and honestly deopt to Source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceExecutionPolicy {
    JitWithSourceFallback,
    SourceOnly,
}

impl SourceExecutionPolicy {
    fn allows_native(self) -> bool {
        matches!(self, Self::JitWithSourceFallback)
    }
}

fn source_native_attempt_allowed(
    host_supported: bool,
    helper_invocation: bool,
    policy: SourceExecutionPolicy,
    plan_allows_native: bool,
    native_values_supported: bool,
    native_return_supported: bool,
) -> bool {
    host_supported
        && (!helper_invocation || policy.allows_native())
        && plan_allows_native
        && native_values_supported
        && native_return_supported
}

pub struct SourceDeoptRun {
    pub outcome: RunOutcome,
    /// Exact recursive Source or native typed return. Native Cranelift
    /// supplies this for every supported scalar/Unit return; unsupported
    /// return metadata never enters the native path.
    pub value: Option<MirRuntimeValue>,
    /// Owned logical Source/Eval session guard. Native runs do not create one.
    pub session: Option<Box<dyn super::deopt::SourceExecutionGuard>>,
    pub tier: SourceExecutionTier,
    /// Checked writable parameter slots installed by this invocation.
    writeback_indices: Vec<usize>,
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
    pub tier: SourceExecutionTier,
    pub resources: SourceResourceSession,
    pub session: Option<Box<dyn super::deopt::SourceExecutionGuard>>,
}

/// One private helper invocation over a retained parent lease. The helper
/// owns a clone only long enough to keep the physical arena live; it never
/// retires the parent session.
pub struct SourceHelperExecution<'a> {
    pub outcome: Option<RunOutcome>,
    pub value: Option<MirRuntimeValue>,
    pub soft_stop: bool,
    pub stdout: String,
    pub stderr: String,
    pub tier: SourceExecutionTier,
    /// Exact checked write-argument values, already restored in their caller
    /// slots and borrowed here without cloning.
    pub writebacks: Vec<SourceHelperArgumentWriteback<'a>>,
    /// A retained clone keeps the physical arena live until this result is
    /// discharged.
    lease: SourceResourceLease,
    pub session: Option<Box<dyn super::deopt::SourceExecutionGuard>>,
    completion_scope: SourceExecutionCompletionScope,
    native_interface_scope: Option<crate::SourceInterfaces::NativeInterfaceCheckedScope>,
    native_interface_context: Option<
        std::rc::Rc<crate::SourceInterfaces::NativeInterfacePhysicalBorrowContext>,
    >,
    execution: Option<MirExecutionIdentity>,
    function: Option<MirFunctionId>,
}

struct SourceHelperRun {
    outcome: Option<RunOutcome>,
    value: Option<MirRuntimeValue>,
    soft_stop: bool,
    stdout: String,
    stderr: String,
    tier: SourceExecutionTier,
    session: Option<Box<dyn super::deopt::SourceExecutionGuard>>,
}

impl<'a> SourceHelperExecution<'a> {
    /// Keep projection callbacks inside this invocation's owned completion
    /// scope, without returning a thread-bound scope guard.
    pub fn with_completion_scope<R>(
        self,
        project: impl FnOnce(&mut Self) -> R,
    ) -> (Self, R) {
        let native_interface_activation = self.native_interface_scope.as_ref().map(|scope| {
            crate::SourceInterfaces::NativeInterfaceScope::activate_checked_scope(
                scope,
                self.native_interface_context.clone(),
            )
        });
        let completion_scope = self.completion_scope.clone();
        let mut execution = self;
        let projected = completion_scope.with_current(|| project(&mut execution));
        drop(native_interface_activation);
        (execution, projected)
    }

    /// Move the completed MIR output to the caller for projection. After this
    /// the retirement completion keeps cleanup state, not a second result copy.
    pub fn take_result(&mut self) -> Option<super::deopt::SourceExecutionRetirement> {
        Some(super::deopt::SourceExecutionRetirement {
            outcome: Some(self.outcome.take()?),
            value: self.value.take(),
            soft_stop: self.soft_stop,
            stdout: std::mem::take(&mut self.stdout),
            stderr: std::mem::take(&mut self.stderr),
            completions: Vec::new(),
        })
    }

    /// Retire only the logical Source/deopt state created by this helper.
    /// The returned completion is the single owner of result, nested receipts,
    /// and the retained counted lease (or the full cleanup error).
    pub fn retire(self) -> SourceExecutionCompletion {
        let SourceHelperExecution {
            outcome,
            value,
            soft_stop,
            stdout,
            stderr,
            tier,
            writebacks,
            lease,
            session,
            completion_scope,
            native_interface_scope,
            execution,
            native_interface_context,
            function,
        } = self;
        let writebacks = writebacks
            .into_iter()
            .map(|writeback| SourceExecutionCompletionWriteback {
                parameter_index: writeback.parameter_index,
                checked_type: writeback.checked_type,
            })
            .collect();
        let arena = lease.arena();
        let _native_interface_activation = native_interface_scope.as_ref().map(|scope| {
            crate::SourceInterfaces::NativeInterfaceScope::activate_checked_scope(
                scope,
                native_interface_context.clone(),
            )
        });
        let retired = completion_scope.with_current(|| {
            retire_source_helper_run(
                SourceHelperRun {
                    outcome,
                    value,
                    soft_stop,
                    stdout,
                    stderr,
                    tier,
                    session,
                },
                &arena,
            )
        });
        let mut nested_completions = completion_scope.drain();
        let (cause, retirement) = match retired {
            Ok(mut retirement) => {
                retirement.completions.append(&mut nested_completions);
                (
                    None,
                    Some(SourceExecutionCompletionRetirement::Completed(retirement)),
                )
            }
            Err(mut error) => {
                error.retirement.completions.append(&mut nested_completions);
                (
                    None,
                    Some(SourceExecutionCompletionRetirement::Failed(error)),
                )
            }
        };
        SourceExecutionCompletion {
            execution,
            function,
            lease: Some(lease),
            disposition: SourceExecutionCompletionDisposition::Invoked {
                cause,
                tier: Some(tier),
                retirement,
                writebacks,
            },
        }
    }
}

fn retire_source_helper_run(
    run: SourceHelperRun,
    arena: &SourceResourceArena,
) -> Result<super::deopt::SourceExecutionRetirement, SourceExecutionRetirementError> {
    let SourceHelperRun {
        outcome,
        value,
        tier: _,
        soft_stop,
        stdout,
        stderr,
        session,
    } = run;
    let fallback = super::deopt::SourceExecutionRetirement {
        outcome,
        value,
        soft_stop,
        stdout,
        stderr,
        completions: Vec::new(),
    };
    let Some(session) = session else {
        return Ok(fallback);
    };
    let _activation = activate_source_resource_arena(arena);
    match session.retire() {
        Ok(retirement) => Ok(merge_source_execution_retirement(fallback, retirement)),
        Err(mut error) => {
            error.retirement = merge_source_execution_retirement(fallback, error.retirement);
            Err(error)
        }
    }
}

fn merge_source_execution_retirement(
    fallback: super::deopt::SourceExecutionRetirement,
    retirement: super::deopt::SourceExecutionRetirement,
) -> super::deopt::SourceExecutionRetirement {
    let mut completions = fallback.completions;
    completions.extend(retirement.completions);
    super::deopt::SourceExecutionRetirement {
        outcome: retirement.outcome.or(fallback.outcome),
        value: retirement.value.or(fallback.value),
        soft_stop: retirement.soft_stop,
        stdout: retirement.stdout,
        stderr: retirement.stderr,
        completions,
    }
}
fn append_nested_completions(
    retirement: Option<SourceExecutionCompletionRetirement>,
    nested: &mut Vec<SourceExecutionCompletion>,
) -> Option<SourceExecutionCompletionRetirement> {
    if nested.is_empty() {
        return retirement;
    }
    match retirement {
        Some(SourceExecutionCompletionRetirement::Completed(mut retirement)) => {
            retirement.completions.append(nested);
            Some(SourceExecutionCompletionRetirement::Completed(retirement))
        }
        Some(SourceExecutionCompletionRetirement::Failed(mut error)) => {
            error.retirement.completions.append(nested);
            Some(SourceExecutionCompletionRetirement::Failed(error))
        }
        None => Some(SourceExecutionCompletionRetirement::Completed(
            super::deopt::SourceExecutionRetirement {
                outcome: None,
                value: None,
                soft_stop: false,
                stdout: String::new(),
                stderr: String::new(),
                completions: std::mem::take(nested),
            },
        )),
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
    retirement: super::deopt::SourceExecutionRetirement,
    resources: Option<SourceResourceSession>,
    lease: Option<SourceResourceLease>,
}

impl SourceExecutionRetirementError {
    pub fn new(
        failure: SourceExecutionRetirementFailureKind,
        retirement: super::deopt::SourceExecutionRetirement,
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

    pub fn retirement(&self) -> &super::deopt::SourceExecutionRetirement {
        &self.retirement
    }

    pub fn into_parts(
        self,
    ) -> (
        SourceExecutionRetirementFailureKind,
        super::deopt::SourceExecutionRetirement,
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
    session: Option<Box<dyn super::deopt::SourceExecutionGuard>>,
    fallback: super::deopt::SourceExecutionRetirement,
) -> Result<super::deopt::SourceExecutionRetirement, SourceExecutionRetirementError> {
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
    pub fn retire(
        self,
    ) -> Result<super::deopt::SourceExecutionRetirement, SourceExecutionRetirementError> {
        retire_source_entry(
            self.resources,
            self.session,
            super::deopt::SourceExecutionRetirement {
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

/// Tier-1 backend for a checked canonical MIR package.
pub struct CraneliftBackend;

impl CraneliftBackend {
    pub fn new() -> Self {
        CraneliftBackend
    }
    /// Run the checked artifact entry through native Cranelift when its
    /// selected tier supports the typed ABI, otherwise use the active Source
    /// callback. The public entry wrapper remains strict about artifact.entry.
    pub fn run_with_source_deopt(
        &mut self,
        program: &MirProgram,
        artifact: MirArtifactId,
        entry_values: Vec<MirRuntimeValue>,
        try_anyway: bool,
        policy: &ReleaseDevtoolsPolicy,
    ) -> Result<SourceDeoptRun, SourceDeoptError> {
        let entry = program
            .artifacts
            .iter()
            .find(|candidate| candidate.id == artifact)
            .and_then(|candidate| candidate.entry.as_ref())
            .and_then(|entry| entry.function)
            .ok_or_else(|| {
                SourceDeoptError::InvalidRequest(format!(
                    "artifact {artifact:?} has no checked entry function"
                ))
            })?;
        let mut entry_values = Some(entry_values);
        self.run_with_source_deopt_function(
            program,
            artifact,
            &[],
            entry,
            &mut entry_values,
            try_anyway,
            SourceExecutionPolicy::JitWithSourceFallback,
            policy,
        )
        .map_err(|failure| match failure {
            SourceHelperRunFailure::NotInvoked { cause }
            | SourceHelperRunFailure::Invoked { cause, .. } => cause,
        })
    }

    /// Run one checked private helper root in the selected compiler artifact.
    /// The helper root list is validated separately from public artifact
    /// exports; no artifact entry or MIR row is retargeted.
    fn run_with_source_deopt_function(
        &mut self,
        program: &MirProgram,
        artifact: MirArtifactId,
        helper_roots: &[MirFunctionId],
        function_id: MirFunctionId,
        entry_values: &mut Option<Vec<MirRuntimeValue>>,
        _try_anyway: bool,
        execution_policy: SourceExecutionPolicy,
        policy: &ReleaseDevtoolsPolicy,
    ) -> Result<SourceDeoptRun, SourceHelperRunFailure> {
        let input = match entry_values.as_ref() {
            Some(input) => input,
            None => {
                return Err(SourceHelperRunFailure::Invoked {
                    cause: SourceDeoptError::InvalidRequest(
                        "Source helper arguments were already consumed".to_string(),
                    ),
                    retirement: None,
                    tier: None,
                    session: None,
                    writeback_indices: Vec::new(),
                });
            }
        };
        let function = match program
            .functions
            .iter()
            .find(|candidate| candidate.id == function_id)
        {
            Some(function) => function,
            None => {
                return Err(source_helper_failure_for_input(
                    SourceDeoptError::InvalidRequest(format!(
                        "Source helper function {function_id:?} is missing"
                    )),
                    entry_values,
                ));
            }
        };
        if helper_roots.is_empty() {
            let artifact_entry = program
                .artifacts
                .iter()
                .find(|candidate| candidate.id == artifact)
                .and_then(|candidate| candidate.entry.as_ref())
                .and_then(|entry| entry.function);
            if artifact_entry != Some(function_id) {
                return Err(source_helper_failure_for_input(
                    SourceDeoptError::InvalidRequest(format!(
                        "Source function {function_id:?} is not artifact {artifact:?} entry"
                    )),
                    entry_values,
                ));
            }
            if function.params.len() != input.len() {
                return Err(source_helper_failure_for_input(
                    SourceDeoptError::InvalidRequest(format!(
                        "Source entry expected {} checked parameters, got {} typed values",
                        function.params.len(),
                        input.len()
                    )),
                    entry_values,
                ));
            }
            if let Err(error) = program.validate() {
                return Err(source_helper_failure_for_input(
                    SourceDeoptError::InvalidRequest(format!(
                        "checked Source MIR is invalid: {error}"
                    )),
                    entry_values,
                ));
            }
        } else if let Err(error) = validate_source_helper_entry(
            program,
            artifact,
            helper_roots,
            function_id,
            input.len(),
        ) {
            return Err(source_helper_failure_for_input(error, entry_values));
        }

        let plan = if helper_roots.is_empty() {
            super::tiers::plan_mir_tiers(program, artifact)
        } else {
            super::tiers::plan_mir_tiers_with_roots(program, artifact, helper_roots)
        };
        if !helper_roots.is_empty()
            && !plan.native.contains(&function_id)
            && !function.target_applicability.interpreter
        {
            return Err(source_helper_failure_for_input(
                SourceDeoptError::InvalidRequest(format!(
                    "Source helper function {function_id:?} has no selected executable tier"
                )),
                entry_values,
            ));
        }
        let plan_allows_native = if helper_roots.is_empty() {
            plan.deopt.is_empty()
        } else {
            plan.native.contains(&function_id)
        };
        let native_values_supported = if helper_roots.is_empty() {
            native_entry_values_supported(function, input)
        } else {
            native_helper_entry_values_supported(function, input)
        };
        let native_return_supported = native_return_supported(&function.return_type);
        if source_native_attempt_allowed(
            cranelift_host_supported(),
            !helper_roots.is_empty(),
            execution_policy,
            plan_allows_native,
            native_values_supported,
            native_return_supported,
        ) {
            let attempt = if helper_roots.is_empty() {
                try_resident_with_values_and_result(
                    program,
                    artifact,
                    input,
                    &function.return_type,
                    policy,
                )
            } else {
                try_resident_with_function_values_and_result(
                    program,
                    artifact,
                    helper_roots,
                    function_id,
                    input,
                    &function.return_type,
                    policy,
                )
            };
            if let Some(run) =
                native_helper_attempt_result(attempt, function, entry_values)?
            {
                return Ok(run);
            }
        }

        if !function.target_applicability.interpreter {
            return Err(source_helper_failure_for_input(
                SourceDeoptError::InvalidRequest(format!(
                    "Source helper function {function_id:?} cannot fall back to the Source interpreter"
                )),
                entry_values,
            ));
        }
        if !super::deopt::source_deopt_callback_available() {
            return Err(source_helper_failure_for_input(
                SourceDeoptError::Callback(
                    "Source MIR deopt requested without an active resume callback".to_string(),
                ),
                entry_values,
            ));
        }
        let mut request = match super::deopt::source_entry_deopt_request(
            program,
            artifact,
            function_id,
            entry_values,
        ) {
            Ok(request) => request,
            Err(message) => {
                return Err(source_helper_failure_for_input(
                    SourceDeoptError::InvalidRequest(message),
                    entry_values,
                ));
            }
        };
        let dispatch = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::deopt::dispatch_source_deopt(&mut request)
        }));
        let (reply, callback_error) = match dispatch {
            Ok(Ok(reply)) => (Some(reply), None),
            Ok(Err(message)) => (None, Some(SourceDeoptError::Callback(message))),
            Err(payload) => (
                None,
                Some(SourceDeoptError::Callback(format!(
                    "Source entry callback panicked: {}",
                    source_callback_panic_message(payload.as_ref())
                ))),
            ),
        };
        let (writeback_indices, writeback_error) =
            if request.entry_frame_installed && !helper_roots.is_empty() {
                source_entry_writeback_indices(function, &request.entry_values)
            } else {
                (Vec::new(), None)
            };
        *entry_values = Some(std::mem::take(&mut request.entry_values));
        if callback_error.is_some() || writeback_error.is_some() {
            let cause = callback_error.unwrap_or_else(|| {
                SourceDeoptError::Backend(
                    writeback_error.expect("checked Source writeback error"),
                )
            });
            let (retirement, session) = reply.map_or((None, None), |reply| {
                let super::deopt::SourceDeoptReply {
                    outcome,
                    value,
                    session,
                    soft_stop,
                    stdout,
                    stderr,
                    ..
                } = reply;
                (
                    Some(super::deopt::SourceExecutionRetirement {
                        outcome,
                        value,
                        soft_stop,
                        stdout,
                        stderr,
                        completions: Vec::new(),
                    }),
                    session,
                )
            });
            return Err(SourceHelperRunFailure::Invoked {
                cause,
                retirement,
                tier: Some(SourceExecutionTier::Source),
                session,
                writeback_indices,
            });
        }
        let reply = reply.expect("successful Source callback retains its reply");
        if reply.outcome.is_some() {
            super::deopt::publish_source_deopt_reply(&reply, false);
            let super::deopt::SourceDeoptReply {
                outcome: Some(outcome),
                value,
                session,
                ..
            } = reply
            else {
                unreachable!("checked Source reply had a terminal outcome");
            };
            return Ok(SourceDeoptRun {
                outcome,
                value,
                session,
                tier: SourceExecutionTier::Source,
                writeback_indices,
            });
        }
        let super::deopt::SourceDeoptReply {
            value,
            session,
            soft_stop,
            stdout,
            stderr,
            ..
        } = reply;
        let retirement = super::deopt::SourceExecutionRetirement {
            outcome: None,
            value,
            soft_stop,
            stdout,
            stderr,
            completions: Vec::new(),
        };
        Err(SourceHelperRunFailure::Invoked {
            cause: SourceDeoptError::MissingOutcome { cleanup: None },
            tier: Some(SourceExecutionTier::Source),
            retirement: Some(retirement),
            session,
            writeback_indices,
        })
    }
    /// Prove that an HTTP/1.1 worker cannot outlive the resident JIT image.
    ///
    /// This deliberately goes through the resident compiler and the Prelude
    /// server adapters.  The hooks only provide barriers for the proof: the
    /// callback, shutdown, worker join, and runtime teardown are all real
    /// paths.
    pub fn http_worker_runtime_lifetime_proof_for_test(
        &self,
        program: &MirProgram,
        artifact: MirArtifactId,
        handler_name: &str,
    ) -> Result<(), String> {
        crate::on_compiler_stack(|| {
            use std::io::Write;
            use std::net::TcpStream;
            use std::sync::mpsc;
            use std::sync::{Arc, Mutex};
            use std::thread;
            use std::time::Duration;

            if !cranelift_host_supported() {
                return Err("Cranelift host path is unsupported on this architecture".to_string());
            }

            let function_id = program
                .functions
                .iter()
                .find(|function| {
                    function.name == handler_name || function.key == handler_name
                })
                .map(|function| function.id)
                .ok_or_else(|| format!("MIR handler `{handler_name}` is missing"))?;

            super::Concurrency::set_http_test_handler_hook(None);
            super::Concurrency::set_http_test_shutdown_hook(None);
            super::resident::resident_teardown();

            let policy = ReleaseDevtoolsPolicy::development();
            if let Err(error) =
                super::resident::ensure_resident_module(program, artifact, &policy)
            {
                super::resident::resident_teardown();
                return Err(error);
            }

            let handler_func = match super::RESIDENT_MODULE.with(|mod_slot| {
                let mut mod_guard = mod_slot.borrow_mut();
                let resident = mod_guard
                    .as_mut()
                    .ok_or_else(|| "resident module missing".to_string())?;
                super::RESIDENT_RUNTIME.with(|rt_slot| {
                    let mut rt_guard = rt_slot.borrow_mut();
                    let runtime = rt_guard
                        .as_mut()
                        .ok_or_else(|| "resident runtime missing".to_string())?;
                    super::functions_compile::compile_mir_function(
                        &mut resident.module,
                        &resident.host,
                        program,
                        function_id,
                        runtime,
                    )
                })
            }) {
                Ok(function) => function,
                Err(error) => {
                    super::resident::resident_teardown();
                    return Err(error);
                }
            };

            let handler_ptr = match super::RESIDENT_MODULE.with(|mod_slot| {
                let mut mod_guard = mod_slot.borrow_mut();
                let resident = mod_guard
                    .as_mut()
                    .ok_or_else(|| "resident module missing".to_string())?;
                resident
                    .module
                    .finalize_definitions()
                    .map_err(|error| error.to_string())?;
                let ptr = resident.module.get_finalized_function(handler_func);
                if ptr.is_null() {
                    Err("resident HTTP handler has no finalized function address".to_string())
                } else {
                    Ok(ptr as i64)
                }
            }) {
                Ok(ptr) => ptr,
                Err(error) => {
                    super::resident::resident_teardown();
                    return Err(error);
                }
            };

            let callable = match super::RESIDENT_RUNTIME.with(|rt_slot| {
                let mut rt_guard = rt_slot.borrow_mut();
                let runtime = rt_guard
                    .as_mut()
                    .ok_or_else(|| "resident runtime missing".to_string())?;
                let handle = super::runtime_host::bind_jit_callable_handle(
                    runtime,
                    handler_ptr,
                    0,
                    false,
                );
                let runtime_ptr = runtime as *mut super::JitRuntime;
                super::Concurrency::set_active_runtime(Some(runtime_ptr));
                if handle == 0 {
                    Err("resident HTTP handler callable binding failed".to_string())
                } else {
                    Ok(handle)
                }
            }) {
                Ok(handle) => handle,
                Err(error) => {
                    super::resident::resident_teardown();
                    return Err(error);
                }
            };

            let mux = super::net_http_rt::test_http_mux();
            let handler = super::net_http_rt::test_capture_http_handler(callable);
            if let Err(error) =
                super::net_http_rt::test_http_mux_add_handler(mux, "GET", "/", &handler)
            {
                super::resident::resident_teardown();
                return Err(error);
            }
            let server = match super::net_http_rt::test_http_server_bind(
                "127.0.0.1:0".to_string(),
                mux,
            ) {
                Ok(server) => server,
                Err(error) => {
                    super::resident::resident_teardown();
                    return Err(error);
                }
            };
            let address = match super::net_http_rt::test_http_server_local_addr(server) {
                Ok(address) => address,
                Err(error) => {
                    super::resident::resident_teardown();
                    return Err(error);
                }
            };

            let (entry_tx, entry_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let release_rx = Arc::new(Mutex::new(release_rx));
            super::Concurrency::set_http_test_handler_hook(Some(Arc::new(move || {
                let _ = entry_tx.send(());
                let _ = release_rx.lock().ok().and_then(|receiver| receiver.recv().ok());
            })));

            let (shutdown_tx, shutdown_rx) = mpsc::channel();
            super::Concurrency::set_http_test_shutdown_hook(Some(Arc::new(move || {
                let _ = shutdown_tx.send(());
            })));

            let serve_thread = thread::spawn(move || {
                super::net_http_rt::test_http_server_serve(server)
            });
            let cleanup_on_error = |release_tx: &mpsc::Sender<()>,
                                    serve_thread: thread::JoinHandle<Result<(), String>>| {
                let _ = release_tx.send(());
                super::Concurrency::set_http_test_handler_hook(None);
                super::Concurrency::set_http_test_shutdown_hook(None);
                super::resident::resident_teardown();
                let _ = serve_thread.join();
            };

            let mut client = match TcpStream::connect(&address) {
                Ok(client) => client,
                Err(error) => {
                    cleanup_on_error(&release_tx, serve_thread);
                    return Err(format!("HTTP lifetime proof client connect failed: {error}"));
                }
            };
            if let Err(error) = client.set_read_timeout(Some(Duration::from_secs(5))) {
                cleanup_on_error(&release_tx, serve_thread);
                return Err(format!("HTTP lifetime proof client setup failed: {error}"));
            }
            if let Err(error) = client.write_all(
                b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            ) {
                cleanup_on_error(&release_tx, serve_thread);
                return Err(format!("HTTP lifetime proof request failed: {error}"));
            }

            if entry_rx.recv_timeout(Duration::from_secs(5)).is_err() {
                cleanup_on_error(&release_tx, serve_thread);
                return Err("HTTP worker never entered the JIT handler".to_string());
            }
            if super::Concurrency::runtime_access_available_for_test() {
                cleanup_on_error(&release_tx, serve_thread);
                return Err(
                    "HTTP worker handler entry did not hold the runtime access guard".to_string(),
                );
            }

            let (lock_held_tx, lock_held_rx) = mpsc::channel();
            let release_thread = thread::spawn(move || {
                let lock_held = match shutdown_rx.recv_timeout(Duration::from_secs(5)) {
                    Ok(()) => !super::Concurrency::runtime_access_available_for_test(),
                    Err(_) => false,
                };
                let _ = lock_held_tx.send(lock_held);
                let _ = release_tx.send(());
            });

            super::resident::resident_teardown();
            super::Concurrency::set_http_test_handler_hook(None);
            super::Concurrency::set_http_test_shutdown_hook(None);

            let shutdown_saw_guard = lock_held_rx
                .recv()
                .map_err(|_| "HTTP shutdown proof coordinator failed".to_string())?;
            release_thread
                .join()
                .map_err(|_| "HTTP shutdown proof coordinator panicked".to_string())?;
            if !shutdown_saw_guard {
                let _ = serve_thread.join();
                return Err(
                    "HTTP shutdown did not observe the worker runtime access guard".to_string(),
                );
            }

            serve_thread
                .join()
                .map_err(|_| "HTTP server thread panicked".to_string())?
                .map_err(|error| format!("HTTP server failed during lifetime proof: {error}"))?;
            drop(client);
            Ok(())
        })
    }
    /// Exercise the Prelude HTTP/2 dispatch ownership and drain boundary.
    pub fn http2_dispatch_drain_proof_for_test(&self) -> Result<(), String> {
        super::net_http_rt::test_http2_dispatch_drain()
    }
}

/// Execute one checked whole-entry Source request through the native tier or
/// the active Source callback. This is the shared entry executor used by both
/// generated bootstrap glue and the RuntimeMir adapter; it owns callback
/// activation, frame-state isolation, and the physical Source arena lease.
pub fn execute_source_entry<Resume>(
    program: &MirProgram,
    artifact: MirArtifactId,
    entry_function: MirFunctionId,
    resources: SourceResourceSession,
    entry_values: Vec<MirRuntimeValue>,
    policy: &ReleaseDevtoolsPolicy,
    resume: Resume,
) -> Result<SourceEntryExecution, SourceDeoptError>
where
    Resume: Fn(&mut super::deopt::SourceDeoptRequest) -> Result<super::deopt::SourceDeoptReply, String>
        + 'static,
{
    program
        .validate()
        .map_err(|error| SourceDeoptError::InvalidRequest(format!("checked Source MIR is invalid: {error}")))?;
    let artifact_entry = program
        .artifacts
        .iter()
        .find(|candidate| candidate.id == artifact)
        .and_then(|candidate| candidate.entry.as_ref())
        .and_then(|entry| entry.function)
        .ok_or_else(|| {
            SourceDeoptError::InvalidRequest(format!(
                "artifact {artifact:?} has no checked entry function"
            ))
        })?;
    if artifact_entry != entry_function {
        return Err(SourceDeoptError::InvalidRequest(format!(
            "Source entry function {entry_function:?} does not match artifact {artifact:?} entry {artifact_entry:?}"
        )));
    }
    if resources
        .arena()
        .is_retired()
        .map_err(SourceDeoptError::Resource)?
    {
        return Err(SourceDeoptError::Resource(
            "Source resource session is retired".to_string(),
        ));
    }
    let _activation = resources.activate();
    let mut backend = CraneliftBackend::new();
    let native_run = super::deopt::with_source_deopt_scope(resume, || {
        backend.run_with_source_deopt(program, artifact, entry_values, false, policy)
    })?;
    let native_outcome = native_run.outcome;
    let native_value = native_run.value;
    let native_tier = native_run.tier;
    let session = native_run.session;
    let (outcome, soft_stop, value, stdout, stderr, tier) =
        match super::deopt::take_source_deopt_result_with_output() {
            Some((outcome, soft_stop, value, stdout, stderr)) => (
                outcome,
                soft_stop,
                value,
                stdout,
                stderr,
                SourceExecutionTier::Source,
            ),
            None => (
                native_outcome,
                false,
                native_value,
                String::new(),
                String::new(),
                native_tier,
            ),
        };
    Ok(SourceEntryExecution {
        outcome,
        value,
        soft_stop,
        stdout,
        stderr,
        tier,
        resources,
        session,
    })
}

fn validate_source_helper_entry(
    program: &MirProgram,
    artifact: MirArtifactId,
    helper_roots: &[MirFunctionId],
    function: MirFunctionId,
    argument_count: usize,
) -> Result<(), SourceDeoptError> {
    program.validate().map_err(|error| {
        SourceDeoptError::InvalidRequest(format!("checked Source MIR is invalid: {error}"))
    })?;
    let artifact_plan = program
        .artifacts
        .iter()
        .find(|candidate| candidate.id == artifact)
        .ok_or_else(|| {
            SourceDeoptError::InvalidRequest(format!(
                "Source helper artifact {artifact:?} is missing"
            ))
        })?;
    if !matches!(
        artifact_plan.target,
        jet_foundation::MIR::MirArtifactTarget::RustAot
            | jet_foundation::MIR::MirArtifactTarget::Cranelift
    ) {
        return Err(SourceDeoptError::InvalidRequest(format!(
            "Source helper artifact {artifact:?} targets {:?}, which is not executable by the Source helper native tier",
            artifact_plan.target
        )));
    }
    if helper_roots.is_empty() || !helper_roots.contains(&function) {
        return Err(SourceDeoptError::InvalidRequest(format!(
            "Source helper function {function:?} is not in the checked private root list"
        )));
    }
    for root in helper_roots {
        let root_function = program
            .functions
            .iter()
            .find(|candidate| candidate.id == *root)
            .ok_or_else(|| {
                SourceDeoptError::InvalidRequest(format!(
                    "private Source helper function {root:?} is missing"
                ))
            })?;
        if !artifact_plan.modules.contains(&root_function.module_id) {
            return Err(SourceDeoptError::InvalidRequest(format!(
                "private Source helper function {root:?} is not part of artifact {artifact:?}"
            )));
        }
        if !root_function.target_applicability.cranelift
            && !root_function.target_applicability.interpreter
        {
            return Err(SourceDeoptError::InvalidRequest(format!(
                "private Source helper function {root:?} has no executable target"
            )));
        }
        if !root_function.capture_params.is_empty() {
            return Err(SourceDeoptError::InvalidRequest(format!(
                "private Source helper function {root:?} requires captured parameters"
            )));
        }
    }
    let function_row = program
        .functions
        .iter()
        .find(|candidate| candidate.id == function)
        .ok_or_else(|| {
            SourceDeoptError::InvalidRequest(format!(
                "Source helper function {function:?} is missing"
            ))
        })?;
    if function_row.params.len() != argument_count {
        return Err(SourceDeoptError::InvalidRequest(format!(
            "Source helper function {function:?} expected {} checked parameters, got {argument_count}",
            function_row.params.len()
        )));
    }
    for parameter in &function_row.params {
        if parameter.variadic {
            return Err(SourceDeoptError::InvalidRequest(format!(
                "Source helper function {function:?} has a variadic parameter `{}`",
                parameter.name
            )));
        }
    }
    Ok(())
}

/// Determine whether this helper's reachable code can enter either checked
/// dynamic dispatch path. Unrelated declarations elsewhere in the compiler
/// image do not require a C bindings scope for a pure private helper.
fn source_helper_requires_native_interface_scope(
    program: &MirProgram,
    root: MirFunctionId,
) -> Result<bool, SourceDeoptError> {
    let mut pending = vec![root];
    let mut visited = std::collections::BTreeSet::new();
    while let Some(function_id) = pending.pop() {
        if !visited.insert(function_id) {
            continue;
        }
        let function = program
            .functions
            .iter()
            .find(|candidate| candidate.id == function_id)
            .ok_or_else(|| {
                SourceDeoptError::InvalidRequest(format!(
                    "reachable Source helper function {function_id:?} is missing"
                ))
            })?;
        for instruction in function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
        {
            match &instruction.operation {
                jet_foundation::MIR::MirOperation::Call { callee, .. } => match callee {
                    jet_foundation::MIR::MirCallee::TraitMethod { .. } => return Ok(true),
                    jet_foundation::MIR::MirCallee::Indirect(callee) => {
                        if function.values.iter().any(|(value, ty, _, _)| {
                            value == callee
                                && matches!(
                                    ty.kind(),
                                    MirTypeKind::Fn(_) | MirTypeKind::SendFn { .. }
                                )
                        }) {
                            return Ok(true);
                        }
                    }
                    jet_foundation::MIR::MirCallee::User(target)
                    | jet_foundation::MIR::MirCallee::Associated {
                        function: target, ..
                    }
                    | jet_foundation::MIR::MirCallee::Method {
                        function: target, ..
                    } => pending.push(*target),
                    _ => {}
                },
                jet_foundation::MIR::MirOperation::IndirectCall { callee, .. } => {
                    if function.values.iter().any(|(value, ty, _, _)| {
                        value == callee
                            && matches!(
                                ty.kind(),
                                MirTypeKind::Fn(_) | MirTypeKind::SendFn { .. }
                            )
                    }) {
                        return Ok(true);
                    }
                }
                jet_foundation::MIR::MirOperation::Closure {
                    function: target, ..
                } => pending.push(*target),
                _ => {}
            }
        }
    }
    Ok(false)
}

fn source_helper_arguments_not_invoked<'a>(
    arguments: Vec<SourceHelperArgument<'a>>,
) -> (Vec<MirRuntimeValue>, Vec<usize>) {
    let mut entry_values = Vec::with_capacity(arguments.len());
    let mut write_borrow_indices = Vec::new();
    for (index, argument) in arguments.into_iter().enumerate() {
        match argument {
            SourceHelperArgument::Owned(value) => entry_values.push(value),
            SourceHelperArgument::WriteBorrow(_) => write_borrow_indices.push(index),
        }
    }
    (entry_values, write_borrow_indices)
}

fn execute_source_helper_entry_with_root<'a, Resume>(
    program: &MirProgram,
    artifact: MirArtifactId,
    helper_roots: &[MirFunctionId],
    helper_function: MirFunctionId,
    arena: &SourceResourceArena,
    mut arguments: SourceHelperArgumentBuffer<'a>,
    execution_policy: SourceExecutionPolicy,
    policy: &ReleaseDevtoolsPolicy,
    resume: Resume,
) -> Result<
    (SourceHelperRun, Vec<SourceHelperArgumentWriteback<'a>>),
    SourceHelperInvocationFailure<'a>,
>
where
    Resume: Fn(&mut super::deopt::SourceDeoptRequest) -> Result<super::deopt::SourceDeoptReply, String>
        + 'static,
{
    match arena.is_retired() {
        Ok(false) => {}
        Ok(true) => {
            let (entry_values, write_borrow_indices) = arguments.recover_not_invoked();
            return Err(SourceHelperInvocationFailure::NotInvoked {
                cause: SourceDeoptError::Resource("Source resource arena is retired".to_string()),
                entry_values,
                write_borrow_indices,
            });
        }
        Err(error) => {
            let (entry_values, write_borrow_indices) = arguments.recover_not_invoked();
            return Err(SourceHelperInvocationFailure::NotInvoked {
                cause: SourceDeoptError::Resource(error),
                entry_values,
                write_borrow_indices,
            });
        }
    }
    super::deopt::clear_deopt_state();
    let _frame_scope = super::deopt::push_source_deopt_frame_scope();
    let _activation = activate_source_resource_arena(arena);
    let mut backend = CraneliftBackend::new();
    let helper_run = {
        let entry_values = arguments.values_mut();
        super::deopt::with_source_deopt_scope(resume, || {
            backend.run_with_source_deopt_function(
                program,
                artifact,
                helper_roots,
                helper_function,
                entry_values,
                false,
                execution_policy,
                policy,
            )
        })
    };
    let helper_run = match helper_run {
        Ok(run) => run,
        Err(SourceHelperRunFailure::NotInvoked { cause }) => {
            let (entry_values, write_borrow_indices) = arguments.recover_not_invoked();
            return Err(SourceHelperInvocationFailure::NotInvoked {
                cause,
                entry_values,
                write_borrow_indices,
            });
        }
        Err(SourceHelperRunFailure::Invoked {
            cause,
            retirement,
            tier,
            session,
            writeback_indices,
        }) => {
            let writebacks = arguments.complete(&writeback_indices);
            return Err(SourceHelperInvocationFailure::Invoked {
                cause,
                tier,
                retirement,
                session,
                writebacks,
            });
        }
    };
    let writebacks = arguments.complete(&helper_run.writeback_indices);
    let native_outcome = helper_run.outcome;
    let native_value = helper_run.value;
    let native_tier = helper_run.tier;
    let session = helper_run.session;
    let (outcome, soft_stop, value, stdout, stderr, tier) =
        match super::deopt::take_source_deopt_result_with_output() {
            Some((outcome, soft_stop, value, stdout, stderr)) => (
                outcome,
                soft_stop,
                value,
                stdout,
                stderr,
                SourceExecutionTier::Source,
            ),
            None => (
                native_outcome,
                false,
                native_value,
                String::new(),
                String::new(),
                native_tier,
            ),
        };
    Ok((
        SourceHelperRun {
            outcome: Some(outcome),
            value,
            soft_stop,
            stdout,
            stderr,
            tier,
            session,
        },
        writebacks,
    ))
}

/// Execute one checked private helper through its selected native or Source
/// tier. The helper root list is compiler-image authority, not a public MIR
/// export or an artifact-entry replacement. The retained lease is cloned into
/// the result so physical resources stay live until it is discharged.
pub fn execute_source_helper_entry<'a, Resume>(
    program: &MirProgram,
    artifact: MirArtifactId,
    helper_roots: &[MirFunctionId],
    helper_function: MirFunctionId,
    lease: &SourceResourceLease,
    entry_values: Vec<SourceHelperArgument<'a>>,
    execution_policy: SourceExecutionPolicy,
    policy: &ReleaseDevtoolsPolicy,
    resume: Resume,
) -> Result<SourceHelperExecution<'a>, SourceHelperInvocationError<'a>>
where
    Resume: Fn(&mut super::deopt::SourceDeoptRequest) -> Result<super::deopt::SourceDeoptReply, String>
        + 'static,
{
    let helper_lease = lease.clone();
    let completion_scope = SourceExecutionCompletionScope::new();
    let native_interface_scope =
        crate::SourceInterfaces::NativeInterfaceScope::current_checked_scope();
    let native_interface_context =
        crate::SourceInterfaces::NativeInterfaceScope::current_physical_borrow_context();
    if let Err(cause) = validate_source_helper_entry(
        program,
        artifact,
        helper_roots,
        helper_function,
        entry_values.len(),
    ) {
        let (entry_values, write_borrow_indices) =
            source_helper_arguments_not_invoked(entry_values);
        return Err(SourceHelperInvocationError::not_invoked_with_write_borrows(
            cause,
            entry_values,
            write_borrow_indices,
            helper_lease,
            completion_scope,
            native_interface_scope,
            native_interface_context.clone(),
            None,
            Some(helper_function),
        ));
    }
    let execution = match program.execution_identity(Some(artifact)) {
        Ok(execution) => execution,
        Err(error) => {
            let (entry_values, write_borrow_indices) =
                source_helper_arguments_not_invoked(entry_values);
            return Err(SourceHelperInvocationError::not_invoked_with_write_borrows(
                SourceDeoptError::InvalidRequest(format!(
                    "Source helper execution identity is unavailable: {error}"
                )),
                entry_values,
                write_borrow_indices,
                helper_lease,
                completion_scope,
                native_interface_scope,
                native_interface_context.clone(),
                None,
                Some(helper_function),
            ));
        }
    };
    let function = match program
        .functions
        .iter()
        .find(|candidate| candidate.id == helper_function)
    {
        Some(function) => function,
        None => {
            let (entry_values, write_borrow_indices) =
                source_helper_arguments_not_invoked(entry_values);
            return Err(SourceHelperInvocationError::not_invoked_with_write_borrows(
                SourceDeoptError::InvalidRequest(format!(
                    "Source helper function {helper_function:?} is missing"
                )),
                entry_values,
                write_borrow_indices,
                helper_lease,
                completion_scope,
                native_interface_scope,
                native_interface_context.clone(),
                Some(execution),
                Some(helper_function),
            ));
        }
    };
    if let Some(scope) = native_interface_scope.as_ref() {
        if let Err(error) = scope.validate_execution(program, artifact) {
            let (entry_values, write_borrow_indices) =
                source_helper_arguments_not_invoked(entry_values);
            return Err(SourceHelperInvocationError::not_invoked_with_write_borrows(
                SourceDeoptError::NativeInterface(error),
                entry_values,
                write_borrow_indices,
                helper_lease,
                completion_scope,
                native_interface_scope,
                native_interface_context.clone(),
                Some(execution),
                Some(helper_function),
            ));
        }
    }
    let requires_native_interface_scope =
        match source_helper_requires_native_interface_scope(program, helper_function) {
            Ok(required) => required,
            Err(cause) => {
                let (entry_values, write_borrow_indices) =
                    source_helper_arguments_not_invoked(entry_values);
                return Err(SourceHelperInvocationError::not_invoked_with_write_borrows(
                    cause,
                    entry_values,
                    write_borrow_indices,
                    helper_lease,
                    completion_scope,
                    native_interface_scope,
                    native_interface_context.clone(),
                    Some(execution),
                    Some(helper_function),
                ));
            }
        };
    if requires_native_interface_scope && native_interface_scope.is_none() {
        let (entry_values, write_borrow_indices) =
            source_helper_arguments_not_invoked(entry_values);
        return Err(SourceHelperInvocationError::not_invoked_with_write_borrows(
            SourceDeoptError::NativeInterface(
                crate::SourceInterfaces::NativeInterfaceError::MissingBinding,
            ),
            entry_values,
            write_borrow_indices,
            helper_lease,
            completion_scope,
            native_interface_scope,
            native_interface_context.clone(),
            Some(execution),
            Some(helper_function),
        ));
    }
    if let Err(message) = SourceHelperArgumentBuffer::validate(&entry_values, &function.params) {
        let (entry_values, write_borrow_indices) =
            source_helper_arguments_not_invoked(entry_values);
        return Err(SourceHelperInvocationError::not_invoked_with_write_borrows(
            SourceDeoptError::InvalidRequest(message),
            entry_values,
            write_borrow_indices,
            helper_lease,
            completion_scope,
            native_interface_scope,
            native_interface_context.clone(),
            Some(execution),
            Some(helper_function),
        ));
    }
    let arguments = SourceHelperArgumentBuffer::new(entry_values, &function.params);
    let native_interface_activation = native_interface_scope.as_ref().map(|scope| {
        crate::SourceInterfaces::NativeInterfaceScope::activate_checked_scope(
            scope,
            native_interface_context.clone(),
        )
    });
    let arena = lease.arena();
    let result = completion_scope.with_current(|| {
        execute_source_helper_entry_with_root(
            program,
            artifact,
            helper_roots,
            helper_function,
            &arena,
            arguments,
            execution_policy,
            policy,
            resume,
        )
    });
    drop(native_interface_activation);
    match result {
        Ok((run, writebacks)) => Ok(SourceHelperExecution {
            outcome: run.outcome,
            value: run.value,
            soft_stop: run.soft_stop,
            stdout: run.stdout,
            stderr: run.stderr,
            tier: run.tier,
            writebacks,
            lease: helper_lease,
            session: run.session,
            completion_scope,
            native_interface_scope,
            native_interface_context,
            execution: Some(execution),
            function: Some(helper_function),
        }),
        Err(failure) => Err(SourceHelperInvocationError {
            failure,
            lease: helper_lease,
            completion_scope,
            native_interface_scope,
            execution: Some(execution),
            native_interface_context,
            function: Some(helper_function),
        }),
    }
}



fn integer_fits(kind: &MirTypeKind, raw: i64) -> bool {
    match kind {
        MirTypeKind::IntN { signed, bits } => {
            if *bits == 0 || *bits > 64 {
                return false;
            }
            if *signed {
                *bits == 64
                    || {
                        let half = 1_i128 << (*bits - 1);
                        let raw = i128::from(raw);
                        (-half..half).contains(&raw)
                    }
            } else if *bits == 64 {
                raw >= 0
            } else {
                let limit = 1_i128 << *bits;
                (0..limit).contains(&i128::from(raw))
            }
        }
        MirTypeKind::InlineRange { base, lo, hi } => {
            integer_fits(&base.kind, raw) && (*lo..=*hi).contains(&raw)
        }
        MirTypeKind::Tagged { inner, .. } => integer_fits(&inner.kind, raw),
        MirTypeKind::Quantity { base, .. } => integer_fits(&base.kind, raw),
        _ => true,
    }
}
fn native_entry_values_supported(
    function: &jet_foundation::MIR::MirFunction,
    values: &[MirRuntimeValue],
) -> bool {
    if !function.capture_params.is_empty()
        || function.params.len() != values.len()
        || function.params.len() > 8
    {
        return false;
    }
    !function.params.iter().any(|parameter| {
        parameter.access == MirAccess::Write || clif_ty_from_mir(&parameter.ty).is_none()
    })
}

fn native_helper_entry_values_supported(
    function: &MirFunction,
    values: &[MirRuntimeValue],
) -> bool {
    function.capture_params.is_empty()
        && function.params.len() == values.len()
        && function.params.len() <= 8
        && function
            .params
            .iter()
            .all(|parameter| clif_ty_from_mir(&parameter.ty).is_some())
}

fn native_return_supported(ty: &MirType) -> bool {
    clif_ty_from_mir(ty).is_some() && !matches!(ty.layout.abi, MirAbi::Function | MirAbi::Never)
}

fn native_float_word(value: &MirRuntimeValue, f32_value: bool) -> Option<i64> {
    match (f32_value, value) {
        (
            false,
            MirRuntimeValue::Float {
                value,
                f32: false,
            },
        ) => Some(value.to_bits() as i64),
        (
            true,
            MirRuntimeValue::Float {
                value,
                f32: true,
            },
        ) => Some((*value as f32).to_bits() as i64),
        _ => None,
    }
}

fn plan_failure(plan: &MirTierPlan) -> RunOutcome {
    note_deopt_invoked_for_test();
    // Publish the planner's own rows before converting the tier failure to
    // the ordinary runtime diagnostic.  `record_trace` is the sole notice
    // channel and suppresses the concise notice when expert tracing is on.
    record_trace(plan.rows.clone());
    let detail = plan
        .gap
        .as_ref()
        .map(|gap| {
            format!(
                "Cranelift cannot execute MIR function `{}`: {}",
                gap.function_name, gap.reason
            )
        })
        .unwrap_or_else(|| "Cranelift cannot execute the checked MIR program".to_string());
    RunOutcome::Problems(vec![jet_foundation::Diagnostics::Diagnostic::runtime_host_fault(
        String::new(),
        detail,
    )])
}

fn plan_failure_diagnostics(plan: &MirTierPlan) -> Vec<jet_foundation::Diagnostics::Diagnostic> {
    match plan_failure(plan) {
        RunOutcome::Problems(diagnostics) => diagnostics,
        RunOutcome::Ran { .. } => Vec::new(),
    }
}

impl JitBackend for CraneliftBackend {
    type InvocationPolicy = ReleaseDevtoolsPolicy;

    fn run(
        &mut self,
        program: &MirProgram,
        artifact: MirArtifactId,
        _try_anyway: bool,
        policy: &Self::InvocationPolicy,
    ) -> RunOutcome {
        crate::on_compiler_stack(|| {
            crate::reset_one_shot_core_state();
            if !cranelift_host_supported() {
                return plan_failure(&super::tiers::plan_mir_tiers(program, artifact));
            }
            match try_resident(program, artifact, policy) {
                Ok(outcome) => outcome,
                Err(plan) => plan_failure(&plan),
            }
        })
    }

    fn hot_swap(
        &mut self,
        _module_name: &str,
        program: &MirProgram,
        artifact: MirArtifactId,
        _try_anyway: bool,
        policy: &Self::InvocationPolicy,
    ) -> Result<RunOutcome, Vec<jet_foundation::Diagnostics::Diagnostic>> {
        match try_resident_hot_swap(program, artifact, policy) {
            Ok(outcome) => Ok(outcome),
            Err(plan) => Err(plan_failure_diagnostics(&plan)),
        }
    }

    fn restart(
        &mut self,
        program: &MirProgram,
        artifact: MirArtifactId,
        _try_anyway: bool,
        policy: &Self::InvocationPolicy,
    ) -> RunOutcome {
        match try_resident_restart(program, artifact, policy) {
            Ok(outcome) => outcome,
            Err(plan) => plan_failure(&plan),
        }
    }
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
                cause: SourceDeoptError::InvalidRequest("preflight".to_string()),
                entry_values: Vec::new(),
                write_borrow_indices: Vec::new(),
                completions: Vec::new(),
            },
        };
        assert!(weak
            .record_box(source_execution_completion_into_box(completion))
            .is_ok());
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
    #[test]
    fn source_helper_write_borrow_restores_the_exact_caller_slot() {
        let parameter = MirParam {
            index: 0,
            name: "value".to_string(),
            span: jet_foundation::Diagnostics::Span::new(0, 0),
            ty: MirType::from_kind(MirTypeKind::Int),
            access: MirAccess::Write,
            ownership: jet_foundation::MIR::MirOwnership::copy(),
            public_label: String::new(),
            variadic: false,
            default_present: false,
        };
        let parameters = [parameter];
        assert!(SourceHelperArgumentBuffer::validate(
            &[SourceHelperArgument::Owned(MirRuntimeValue::Int(5))],
            &parameters,
        )
        .is_err());

        let mut caller_slot = MirRuntimeValue::Int(10);
        assert!(SourceHelperArgumentBuffer::validate(
            &[SourceHelperArgument::WriteBorrow(&mut caller_slot)],
            &parameters,
        )
        .is_ok());
        let mut arguments = SourceHelperArgumentBuffer::new(
            vec![SourceHelperArgument::WriteBorrow(&mut caller_slot)],
            &parameters,
        );
        arguments.values_mut().as_mut().expect("helper arguments")[0] =
            MirRuntimeValue::Int(42);
        let writebacks = arguments.complete(&[0]);
        assert_eq!(caller_slot, MirRuntimeValue::Int(42));
        assert_eq!(writebacks.len(), 1);
        assert_eq!(writebacks[0].parameter_index, 0);
        assert!(std::ptr::eq(writebacks[0].value, &caller_slot));
        assert!(writebacks[0]
            .checked_type
            .same_checked_type(&parameters[0].ty));
        drop(writebacks);

        let mut unchanged_slot = MirRuntimeValue::Int(17);
        let (entry_values, write_borrow_indices) = SourceHelperArgumentBuffer::new(
            vec![SourceHelperArgument::WriteBorrow(&mut unchanged_slot)],
            &parameters,
        )
        .recover_not_invoked();
        assert_eq!(unchanged_slot, MirRuntimeValue::Int(17));
        assert!(entry_values.is_empty());
        assert_eq!(write_borrow_indices, vec![0]);
        let mut rejected_slot = MirRuntimeValue::Int(23);
        let (entry_values, write_borrow_indices) = source_helper_arguments_not_invoked(vec![
            SourceHelperArgument::Owned(MirRuntimeValue::Int(11)),
            SourceHelperArgument::WriteBorrow(&mut rejected_slot),
            SourceHelperArgument::Owned(MirRuntimeValue::Int(29)),
        ]);
        assert_eq!(
            entry_values,
            vec![MirRuntimeValue::Int(11), MirRuntimeValue::Int(29)]
        );
        assert_eq!(write_borrow_indices, vec![1]);
        assert_eq!(rejected_slot, MirRuntimeValue::Int(23));
    }




    #[test]
    fn native_entry_float_words_preserve_declared_bit_width() {
        let f64_bits = 0x3ff8_0000_0000_0042_u64;
        assert_eq!(
            native_float_word(
                &MirRuntimeValue::Float {
                    value: f64::from_bits(f64_bits),
                    f32: false,
                },
                false,
            ),
            Some(f64_bits as i64)
        );
        let f32_bits = 0x3fc0_1234_u32;
        assert_eq!(
            native_float_word(
                &MirRuntimeValue::Float {
                    value: f32::from_bits(f32_bits) as f64,
                    f32: true,
                },
                true,
            ),
            Some(f32_bits as i64)
        );
    }

    #[test]
    fn native_entry_integer_words_reject_checked_range_overflow() {
        let signed = MirTypeKind::IntN {
            signed: true,
            bits: 8,
        };
        assert!(integer_fits(&signed, -128));
        assert!(integer_fits(&signed, 127));
        assert!(!integer_fits(&signed, 128));

        let unsigned = MirTypeKind::IntN {
            signed: false,
            bits: 8,
        };
        assert!(integer_fits(&unsigned, 255));
        assert!(!integer_fits(&unsigned, -1));
        assert!(!integer_fits(&unsigned, 256));
    }
    #[test]
    fn source_only_helper_policy_never_attempts_native() {
        assert!(!source_native_attempt_allowed(
            true,
            true,
            SourceExecutionPolicy::SourceOnly,
            true,
            true,
            true,
        ));
        assert!(source_native_attempt_allowed(
            true,
            true,
            SourceExecutionPolicy::JitWithSourceFallback,
            true,
            true,
            true,
        ));
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

    impl super::super::deopt::SourceExecutionGuard for FailedSourceGuard {
        fn retire(
            self: Box<Self>,
        ) -> Result<
            super::super::deopt::SourceExecutionRetirement,
            super::SourceExecutionRetirementError,
        > {
            Err(super::SourceExecutionRetirementError::new(
                super::SourceExecutionRetirementFailureKind::Session(
                    "test session cleanup failure".to_string(),
                ),
                super::super::deopt::SourceExecutionRetirement {
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
    fn not_invoked_helper_error_returns_exact_values_and_keeps_the_counted_root() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let drops = Arc::new(AtomicUsize::new(0));
        let session = SourceResourceSession::new();
        let lease = session.retain_root().expect("retain helper root");
        let arena = lease.arena();
        let mut entry_values = Some(vec![counted_source_value(71, &drops)]);
        let attempt = native_helper_attempt_result(
            super::resident::ResidentHelperAttempt::NotInvoked(
                "native preflight failed".to_string(),
            ),
            MirFunctionId(1),
            &mut entry_values,
        );
        assert!(matches!(attempt, Ok(None)));
        let error = SourceHelperInvocationError::not_invoked(
            SourceDeoptError::Backend("native preflight failed".to_string()),
            entry_values.take().expect("original input remains available"),
            lease.clone(),
        );

        drop(lease);
        session.retire().expect("request parent retirement");
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert_eq!(session.retained_root_count().expect("root count"), 1);
        assert!(!arena.is_retired().expect("error still owns the root"));

        let (
            failure,
            retained_lease,
            _completion_scope,
            _native_interface_scope,
            _native_interface_context,
        ) = error.into_parts();
        let SourceHelperInvocationFailure::NotInvoked { entry_values, .. } = failure else {
            panic!("pre-invocation error must return the original entry values");
        };
        let MirRuntimeValue::NativeOwned(value) = &entry_values[0] else {
            panic!("helper input root must remain native-owned");
        };
        assert_eq!(
            value
                .downcast_ref::<CountedSourceValue>()
                .expect("original source value type")
                .marker,
            71
        );

        drop(entry_values);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(!arena.is_retired().expect("caller still owns the counted root"));
        drop(retained_lease);
        assert!(arena.is_retired().expect("last root releases the arena"));
    }

    #[test]
    fn invoked_helper_error_has_no_retry_input_and_retains_updated_output() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let invocations = Arc::new(AtomicUsize::new(0));
        let input_drops = Arc::new(AtomicUsize::new(0));
        let output_drops = Arc::new(AtomicUsize::new(0));
        let session = SourceResourceSession::new();
        let lease = session.retain_root().expect("retain helper root");
        let arena = lease.arena();
        let mut entry_values = Some(vec![counted_source_value(71, &input_drops)]);
        invocations.fetch_add(1, Ordering::SeqCst);
        let attempt = super::resident::ResidentHelperAttempt::Invoked {
            outcome: RunOutcome::Ran {
                stdout: "native output".to_string(),
                stderr: "decode failed".to_string(),
                exit_code: 1,
            },
            value: Some(counted_source_value(99, &output_drops)),
            failure: Some("native return decode failed".to_string()),
        };
        let failure = match native_helper_attempt_result(
            attempt,
            MirFunctionId(1),
            &mut entry_values,
        ) {
            Err(failure) => failure,
            Ok(_) => panic!("post-invocation failure must not produce Source fallback"),
        };
        assert!(entry_values.is_none());
        assert_eq!(input_drops.load(Ordering::SeqCst), 1);
        let SourceHelperRunFailure::Invoked {
            cause,
            retirement: Some(retirement),
            tier,
            session: None,
            ..
        } = failure
        else {
            panic!("post-invocation failure must retain its output evidence");
        };
        let error = SourceHelperInvocationError {
            failure: SourceHelperInvocationFailure::Invoked {
                cause,
                tier,
                retirement: Some(retirement),
                session: None,
                writebacks: Vec::new(),
            },
            lease: lease.clone(),
            completion_scope: SourceExecutionCompletionScope::new(),
            native_interface_scope: None,
            native_interface_context: None,
            execution: None,
            function: Some(MirFunctionId(1)),
        };
        drop(lease);
        session.retire().expect("request parent retirement");

        assert_eq!(invocations.load(Ordering::SeqCst), 1);
        assert!(error.entry_values().is_none());
        assert_eq!(output_drops.load(Ordering::SeqCst), 0);
        assert!(!arena.is_retired().expect("error still owns the root"));

        let SourceExecutionCompletion {
            lease: Some(retained_lease),
            disposition:
                SourceExecutionCompletionDisposition::Invoked {
                    cause: Some(_),
                    tier: Some(SourceExecutionTier::Native),
                    retirement:
                        Some(SourceExecutionCompletionRetirement::Completed(retirement)),
                    ..
                },
            ..
        } = error.into_completion()
        else {
            panic!("invoked failure completion must retain its exact result and lease");
        };
        let RunOutcome::Ran {
            stdout,
            stderr,
            ..
        } = retirement.outcome.expect("native outcome remains owned")
        else {
            panic!("native error completion keeps the run outcome");
        };
        assert_eq!(stdout, "native output");
        assert_eq!(stderr, "decode failed");
        let MirRuntimeValue::NativeOwned(value) =
            retirement.value.as_ref().expect("updated return value")
        else {
            panic!("updated return root must remain native-owned");
        };
        assert_eq!(
            value
                .downcast_ref::<CountedSourceValue>()
                .expect("updated source value type")
                .marker,
            99
        );
        drop(retirement);
        assert_eq!(output_drops.load(Ordering::SeqCst), 1);
        assert_eq!(invocations.load(Ordering::SeqCst), 1);
        drop(retained_lease);
        assert!(arena.is_retired().expect("last root releases the arena"));
    }


    #[test]
    fn helper_session_retirement_failure_keeps_result_and_counted_lease() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let drops = Arc::new(AtomicUsize::new(0));
        let session = SourceResourceSession::new();
        let lease = session.retain_root().expect("retain helper result root");
        let arena = lease.arena();
        let execution = SourceHelperExecution {
            outcome: Some(RunOutcome::Ran {
                stdout: "helper output".to_string(),
                stderr: String::new(),
                exit_code: 0,
            }),
            value: Some(counted_source_value(144, &drops)),
            soft_stop: false,
            stdout: "helper output".to_string(),
            stderr: String::new(),
            tier: SourceExecutionTier::Source,
            writebacks: Vec::new(),
            lease: lease.clone(),
            session: Some(Box::new(FailedSourceGuard)),
            completion_scope: SourceExecutionCompletionScope::new(),
            native_interface_scope: None,
            native_interface_context: None,
            execution: None,
            function: Some(MirFunctionId(1)),
        };

        drop(lease);
        session.retire().expect("request parent retirement");
        let completion = execution.retire();
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert_eq!(session.retained_root_count().expect("root count"), 1);
        assert!(!arena.is_retired().expect("completion owns the counted lease"));

        let SourceExecutionCompletion {
            lease: Some(retained_lease),
            disposition:
                SourceExecutionCompletionDisposition::Invoked {
                    tier: Some(SourceExecutionTier::Source),
                    retirement:
                        Some(SourceExecutionCompletionRetirement::Failed(error)),
                    ..
                },
            ..
        } = completion
        else {
            panic!("helper retirement must preserve the full cleanup failure");
        };
        let (failure, pending, retained_resources, retained_error_lease) =
            error.into_parts();
        assert!(matches!(
            failure,
            SourceExecutionRetirementFailureKind::Session(ref message)
                if message == "test session cleanup failure"
        ));
        assert!(retained_resources.is_none());
        assert!(retained_error_lease.is_none());
        assert_eq!(pending.stdout, "helper output");
        let MirRuntimeValue::NativeOwned(value) =
            pending.value.as_ref().expect("pending helper value")
        else {
            panic!("pending helper output remains native-owned");
        };
        assert_eq!(
            value
                .downcast_ref::<CountedSourceValue>()
                .expect("pending helper value type")
                .marker,
            144
        );

        drop(pending);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(!arena.is_retired().expect("counted lease keeps arena live"));
        drop(retained_lease);
        assert!(arena.is_retired().expect("last helper lease retires arena"));
    }
    #[test]
    fn session_retirement_failure_keeps_pending_result_and_resource_owner() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;

        let drops = Arc::new(AtomicUsize::new(0));
        let resources = SourceResourceSession::new();
        let arena = resources.arena();
        let fallback = super::super::deopt::SourceExecutionRetirement {
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
    #[test]
    fn returned_helper_completion_retains_lease_until_consumed() {
        let session = SourceResourceSession::new();
        let lease = session.retain_root().expect("retain helper invocation root");
        let arena = lease.arena();
        let returned = SourceHelperExecution {
            outcome: Some(RunOutcome::Problems(Vec::new())),
            value: None,
            soft_stop: false,
            stdout: String::new(),
            stderr: String::new(),
            tier: SourceExecutionTier::Source,
            writebacks: Vec::new(),
            lease: lease.clone(),
            session: None,
            completion_scope: SourceExecutionCompletionScope::new(),
            native_interface_scope: None,
            native_interface_context: None,
            execution: None,
            function: None,
        };

        drop(lease);
        session.retire().expect("request parent retirement");
        assert!(!arena.is_retired().expect("returned helper retains the root"));
        let completion = returned.retire();
        assert!(!arena.is_retired().expect("completion retains the root"));
        drop(completion);
        assert!(arena.is_retired().expect("consuming completion releases the root"));
    }
}

