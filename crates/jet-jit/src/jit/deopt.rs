//! MIR frame snapshots for a Cranelift-to-interpreter handoff.
//!
//! The interpreter owns execution semantics. This module only carries the
//! versioned foundation MIR identities and the packed ABI frame values across
//! a host boundary.

use jet_foundation::JitBackend::RunOutcome;
use jet_foundation::MIR::{
    MirArtifactId, MirBlockId, MirExecutionIdentity, MirFrameIdentity, MirFunctionId, MirParam,
    MirPlaceId, MirProgram, MirRuntimeValue, MirValueId,
};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone)]
pub struct MirFrameValue {
    pub value: MirValueId,
    pub bits: u64,
}

#[derive(Debug, Clone)]
pub struct MirFramePlace {
    pub place: MirPlaceId,
    pub bits: u64,
}

#[derive(Debug, Clone)]
pub struct MirFrameSnapshot {
    pub identity: MirFrameIdentity,
    pub values: Vec<MirFrameValue>,
    pub places: Vec<MirFramePlace>,
}

/// Typed request delivered when native code hands control to the Source
/// evaluator. The request is invocation-local: its identity comes from the
/// checked MIR artifact. Existing-frame requests use exact scalar ABI carriers;
/// entry requests use `entry_values`, the recursive canonical MIR value codec.
/// The snapshot is an ABI-boundary view; Source resume must use its retained
/// machine state for values, aliases, and resource ownership not carried here.
#[derive(Debug, Clone)]
pub struct SourceDeoptRequest {
    pub snapshot: MirFrameSnapshot,
    pub function: MirFunctionId,
    pub argc: usize,
    /// Checked parameter facts copied from the artifact for an entry handoff.
    /// The Source adapter may use these to validate recursive value shape and
    /// access/ownership mode before adopting values into its first frame.
    pub entry_params: Vec<MirParam>,
    /// Fixed physical ABI carriers for an existing Source frame; only
    /// `[..argc]` are live. Entry handoffs leave this array zeroed when a
    /// parameter is not representable by a scalar ABI word.
    pub args: [u64; 8],
    /// Exact checked entry arguments. This is populated only for an entry
    /// handoff and preserves recursive aggregates, closures, and typed
    /// resource-owned values without flattening them into ABI words.
    pub entry_values: Vec<MirRuntimeValue>,
    /// `true` only after the Source adapter installed the exact checked entry frame.
    /// False keeps the original borrowed write arguments untouched.
    pub entry_frame_installed: bool,
    /// `true` means this is an entry handoff: Source must allocate the
    /// checked function frame from these exact parameters. `false` means the
    /// snapshot belongs to an existing Source frame and resume must preserve
    /// its retained locals, aliases, and resource ledger.
    pub entry: bool,
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
    /// outcomes have no output fields. Native output remains in `outcome`.
    pub stdout: String,
    /// Matching cumulative stderr snapshot.
    pub stderr: String,
    /// Owned nested Source/shared-payload completions discharged while this
    /// invocation, projection, and retirement scope remained active.
    pub completions: Vec<super::backend::SourceExecutionCompletion>,
}

/// Owned, transport-neutral logical Source/Eval session guard.
///
/// The generated compiler implements this trait around its retained
/// `JetEvalResult.session`, callback queue/root lease, and physical arena
/// owner. `retire` is explicit: cleanup may update output/error state and its
pub trait SourceExecutionGuard {
    fn retire(
        self: Box<Self>,
    ) -> Result<SourceExecutionRetirement, super::backend::SourceExecutionRetirementError>;
}

/// ABI result supplied by the Source evaluator after it resumes a deopted
/// frame. `bits` is interpreted by the compiled function's checked return
/// carrier; no type or semantic reconstruction happens in this module.
pub struct SourceDeoptReply {
    /// The physical Cranelift return carrier. This is not a semantic
    /// projection of Source values.
    pub bits: i64,
    /// Typed terminal result, when Source completion terminates the native
    /// invocation. Diagnostics remain Foundation values and are never
    /// stringified by this seam.
    pub outcome: Option<RunOutcome>,
    /// Exact recursive terminal value returned by Source. This remains
    /// separate from the physical Cranelift word so aggregate compiler
    /// results cannot be silently reduced to an integer status.
    pub value: Option<MirRuntimeValue>,
    /// Explicit logical Source/Eval session and callback/resource root.
    /// The Runner retains this through value adoption and calls `retire`.
    pub session: Option<Box<dyn SourceExecutionGuard>>,
    /// Source evaluator status retained alongside `RunOutcome`; the public
    /// backend outcome has no separate soft-stop bit.
    pub soft_stop: bool,
    /// Cumulative output snapshots retained by Source; consumers replace,
    /// never append, these values to output derived from `RunOutcome`.
    pub stdout: String,
    pub stderr: String,
}
/// Callback errors are compiler/host invariant failures. Typed Source runtime
/// diagnostics and control outcomes belong in `SourceDeoptReply::outcome`.

type SourceDeoptCallback =
    Rc<dyn Fn(&mut SourceDeoptRequest) -> Result<SourceDeoptReply, String>>;

/// Owns one nested Source-deopt callback activation.  Activations are
/// thread-local and strictly stack-scoped; there is no process-wide session
/// or session-ID registry.
pub struct SourceDeoptScope {
    callback: Option<SourceDeoptCallback>,
}

thread_local! {
    static SOURCE_DEOPT_CALLBACKS: RefCell<Vec<SourceDeoptCallback>> =
        const { RefCell::new(Vec::new()) };
}

/// Install an owned Source resume callback for the duration of `body`.
///
/// The callback is intentionally `Fn`, not `FnMut`: nested native calls can
/// re-enter the same Source evaluator without borrowing a callback out of
/// its activation stack.  Mutable evaluator state belongs in the callback's
/// own invocation-scoped owner (normally an `Arc<Mutex<_>>` or equivalent
/// Source session handle), never in a global registry.
pub fn with_source_deopt_scope<R>(
    callback: impl Fn(&mut SourceDeoptRequest) -> Result<SourceDeoptReply, String> + 'static,
    body: impl FnOnce() -> R,
) -> R {
    let _scope = push_source_deopt_scope(callback);
    body()
}

/// Push one callback activation and return its lifetime guard.
///
/// This lower-level form is used by generated private Runner glue when the
/// Cranelift call and Source evaluator session have separate lexical scopes.
pub fn push_source_deopt_scope(
    callback: impl Fn(&mut SourceDeoptRequest) -> Result<SourceDeoptReply, String> + 'static,
) -> SourceDeoptScope {
    let callback: SourceDeoptCallback = Rc::new(callback);
    SOURCE_DEOPT_CALLBACKS.with(|stack| stack.borrow_mut().push(callback.clone()));
    SourceDeoptScope {
        callback: Some(callback),
    }
}

impl Drop for SourceDeoptScope {
    fn drop(&mut self) {
        let Some(callback) = self.callback.take() else {
            return;
        };
        SOURCE_DEOPT_CALLBACKS.with(|stack| {
            let mut stack = stack.borrow_mut();
            // Scopes are lexical and therefore normally pop the last entry.
            // Retain the defensive identity search so an unwind through an
            // explicitly retained nested guard cannot leak an outer callback.
            if stack
                .last()
                .is_some_and(|active| Rc::ptr_eq(active, &callback))
            {
                stack.pop();
            } else if let Some(index) = stack.iter().rposition(|active| Rc::ptr_eq(active, &callback)) {
                stack.remove(index);
            }
        });
    }
}

/// Preserves invocation-local frame schemas while a nested native invocation
/// compiles or runs. Resident compilation legitimately replaces the
/// thread-local schema, so nested Source/JIT calls restore the outer schema
/// before the suspended callback can deopt again.
pub struct SourceDeoptFrameScope {
    previous: Option<DeoptState>,
}

/// Isolate frame schemas and one-shot deopt completion for one native
/// invocation. The callback scope and this frame scope are separate because a
/// Source callback may re-enter native code while retaining the outer machine.
pub fn push_source_deopt_frame_scope() -> SourceDeoptFrameScope {
    let previous = DEOPT_STATE.with(|slot| std::mem::take(&mut *slot.borrow_mut()));
    SourceDeoptFrameScope {
        previous: Some(previous),
    }
}

impl Drop for SourceDeoptFrameScope {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            DEOPT_STATE.with(|slot| *slot.borrow_mut() = previous);
        }
    }
}

#[derive(Debug, Clone)]
pub struct MirFrameSchema {
    pub execution: MirExecutionIdentity,
    pub function: MirFunctionId,
    /// The ABI parameters are the first value rows in a checked MIR function.
    /// Keep this separate from `values`: the latter also contains locals and
    /// temporaries, while Source resume validation compares only parameters.
    pub params: Vec<MirValueId>,
    pub values: Vec<MirValueId>,
    pub places: Vec<MirPlaceId>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DeoptState {
    schemas: Vec<MirFrameSchema>,
    last_snapshot: Option<MirFrameSnapshot>,
    last_outcome: Option<RunOutcome>,
    last_value: Option<MirRuntimeValue>,
    last_stdout: String,
    last_stderr: String,
    soft_stop: bool,
    sequence: u64,
}

pub(crate) struct DeoptStateGuard {
    previous: Option<DeoptState>,
}

thread_local! {
    static DEOPT_STATE: RefCell<DeoptState> = const { RefCell::new(DeoptState {
        schemas: Vec::new(),
        last_snapshot: None,
        last_outcome: None,
        last_value: None,
        last_stdout: String::new(),
        last_stderr: String::new(),
        soft_stop: false,
        sequence: 0,
    }) };
}

pub(crate) fn install_frame_schemas(
    program: &MirProgram,
    artifact: Option<MirArtifactId>,
) -> Result<(), String> {
    let execution = program
        .execution_identity(artifact)
        .map_err(|error| format!("MIR deopt execution identity unavailable: {error}"))?;
    DEOPT_STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state.schemas.clear();
        state.last_snapshot = None;
        state.last_outcome = None;
        state.last_value = None;
        state.last_stdout.clear();
        state.last_stderr.clear();
        state.soft_stop = false;
        state.sequence = 0;
        state.schemas.extend(program.functions.iter().map(|function| MirFrameSchema {
            execution: execution.clone(),
            function: function.id,
            params: function
                .values
                .iter()
                .take(function.params.len())
                .map(|(id, _, _, _)| *id)
                .collect(),
            values: function.values.iter().map(|(id, _, _, _)| *id).collect(),
            places: function.places.iter().map(|place| place.id).collect(),
        }));
    });
    Ok(())
}

pub(crate) fn capture_deopt_state() -> DeoptState {
    DEOPT_STATE.with(|slot| slot.borrow().clone())
}

pub(crate) fn install_deopt_state(state: DeoptState) -> DeoptStateGuard {
    let previous = DEOPT_STATE.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), state));
    DeoptStateGuard { previous: Some(previous) }
}

impl Drop for DeoptStateGuard {
    fn drop(&mut self) {
        if let Some(previous) = self.previous.take() {
            DEOPT_STATE.with(|slot| *slot.borrow_mut() = previous);
        }
    }
}

pub fn frame_schema(function: MirFunctionId) -> Option<MirFrameSchema> {
    DEOPT_STATE.with(|slot| {
        slot.borrow()
            .schemas
            .iter()
            .find(|schema| schema.function == function)
            .cloned()
    })
}

pub fn last_snapshot() -> Option<MirFrameSnapshot> {
    DEOPT_STATE.with(|slot| slot.borrow().last_snapshot.clone())
}

pub(crate) fn record_frame_snapshot(
    program: &MirProgram,
    artifact: MirArtifactId,
    function: MirFunctionId,
    block: MirBlockId,
    values: Vec<MirFrameValue>,
    places: Vec<MirFramePlace>,
) -> Result<(), String> {
    DEOPT_STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state.last_outcome = None;
        state.last_value = None;
        state.last_stdout.clear();
        state.last_stderr.clear();
        state.soft_stop = false;
        let sequence = state.sequence;
        state.sequence = state.sequence.wrapping_add(1);
        let identity = program
            .frame_identity(Some(artifact), function, Some(block), sequence)
            .map_err(|error| format!("MIR deopt frame identity unavailable: {error}"))?;
        state.last_snapshot = Some(MirFrameSnapshot { identity, values, places });
        Ok(())
    })
}

fn record_abi_frame(function: i64, argc: i64, args: &[i64; 8]) -> Option<MirFrameSnapshot> {
    let function = u64::try_from(function).ok()?;
    let argc = usize::try_from(argc).ok()?;
    if argc > args.len() {
        return None;
    }
    DEOPT_STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state.last_outcome = None;
        state.last_value = None;
        state.last_stdout.clear();
        state.last_stderr.clear();
        state.soft_stop = false;
        let (execution, schema_function, schema_params) = state
            .schemas
            .iter()
            .find(|schema| schema.function == MirFunctionId(function))
            .map(|schema| {
                (
                    schema.execution.clone(),
                    schema.function,
                    schema.params.iter().copied().collect::<Vec<_>>(),
                )
            })?;
        if argc != schema_params.len() {
            return None;
        }
        let sequence = state.sequence;
        state.sequence = state.sequence.wrapping_add(1);
        let identity = MirFrameIdentity {
            schema_version: execution.schema_version,
            execution,
            function: schema_function,
            block: None,
            sequence,
        };
        let values = schema_params
            .into_iter()
            .zip(args.iter().copied())
            .map(|(value, bits)| MirFrameValue {
                value,
                bits: bits as u64,
            })
            .collect();
        let snapshot = MirFrameSnapshot {
            identity,
            values,
            places: Vec::new(),
        };
        state.last_snapshot = Some(snapshot.clone());
        Some(snapshot)
    })
}

/// The innermost active Source-deopt callback, if any scope is open.
fn current_source_deopt_callback() -> Option<SourceDeoptCallback> {
    SOURCE_DEOPT_CALLBACKS.with(|stack| stack.borrow().last().cloned())
}

/// Dispatch one checked Source handoff through the innermost active callback.
/// Planner-level entry handoffs use this callback rail. The fixed-word ABI
/// helper below remains a strict boundary for any producer that emits it; the
/// current Cranelift lowering does not synthesize a mid-function deopt call.
pub fn dispatch_source_deopt(
    request: &mut SourceDeoptRequest,
) -> Result<SourceDeoptReply, String> {
    current_source_deopt_callback()
        .ok_or_else(|| "Source MIR deopt requested without an active resume callback".to_string())?
        (request)
}

pub(crate) fn source_deopt_callback_available() -> bool {
    current_source_deopt_callback().is_some()
}

/// Construct the exact entry request from the canonical recursive MIR value
/// carrier. Entry values remain in the caller's slot until every preflight
/// check succeeds, then move into the request without cloning.
pub fn source_entry_deopt_request(
    program: &MirProgram,
    artifact: MirArtifactId,
    function: MirFunctionId,
    entry_values: &mut Option<Vec<MirRuntimeValue>>,
) -> Result<SourceDeoptRequest, String> {
    let values = entry_values
        .as_ref()
        .ok_or_else(|| "Source entry values were already consumed".to_string())?;
    let execution = program
        .execution_identity(Some(artifact))
        .map_err(|error| format!("Source entry deopt identity unavailable: {error}"))?;
    let function_row = program
        .functions
        .iter()
        .find(|row| row.id == function)
        .ok_or_else(|| format!("Source entry deopt function {function:?} is missing"))?;
    if function_row.params.len() != values.len() {
        return Err(format!(
            "Source entry deopt expected {} checked parameters, got {} typed values",
            function_row.params.len(),
            values.len()
        ));
    }
    let argc = values.len();
    let identity = MirFrameIdentity {
        schema_version: execution.schema_version,
        execution,
        function,
        block: None,
        sequence: 0,
    };
    let entry_values = entry_values
        .take()
        .expect("checked Source entry values remain owned by their caller");
    Ok(SourceDeoptRequest {
        snapshot: MirFrameSnapshot {
            identity,
            // No native frame exists at an entry handoff. Typed values live in
            // `entry_values`; fabricating ABI bits would lose aggregate and
            // owned carriers.
            values: Vec::new(),
            places: Vec::new(),
        },
        function,
        entry_params: function_row.params.clone(),
        argc,
        args: [0_u64; 8],
        entry_values,
        entry_frame_installed: false,
        entry: true,
    })
}

pub(crate) fn publish_source_deopt_reply(reply: &SourceDeoptReply, stop_native: bool) {
    let Some(outcome) = reply.outcome.clone() else {
        return;
    };
    DEOPT_STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state.last_outcome = Some(outcome);
        state.last_value = reply.value.clone();
        state.last_stdout = reply.stdout.clone();
        state.last_stderr = reply.stderr.clone();
        state.soft_stop = reply.soft_stop;
    });
    if stop_native {
        // A terminal Source result is a control transfer, not metadata that
        // the native caller may ignore. The next generated trap poll leaves
        // through Cranelift control flow before another native instruction.
        crate::Concurrency::with_runtime_mut(|runtime| {
            runtime.set_trap_message("__jet_source_deopt__".to_string());
        });
    }
}

fn record_source_deopt_failure(message: impl Into<String>) {
    crate::Concurrency::with_runtime_mut(|runtime| {
        runtime.set_host_fault(message.into());
    });
}

pub(crate) fn source_deopt_scope_depth() -> usize {
    SOURCE_DEOPT_CALLBACKS.with(|stack| stack.borrow().len())
}

pub(crate) fn clear_deopt_state() {
    DEOPT_STATE.with(|slot| *slot.borrow_mut() = DeoptState::default());
}

/// Take the typed terminal Source result and its cumulative output snapshot.
/// The native ABI word remains separate and is never used to reconstruct this
/// recursive result or its output.
pub fn take_source_deopt_result_with_output(
) -> Option<(RunOutcome, bool, Option<MirRuntimeValue>, String, String)> {
    DEOPT_STATE.with(|slot| {
        let mut state = slot.borrow_mut();
        state.last_outcome.take().map(|outcome| {
            let soft_stop = state.soft_stop;
            let value = state.last_value.take();
            let stdout = std::mem::take(&mut state.last_stdout);
            let stderr = std::mem::take(&mut state.last_stderr);
            (outcome, soft_stop, value, stdout, stderr)
        })
    })
}


/// Host ABI entry for a producer that emits a mid-function deopt boundary.
/// It supplies packed ABI words only; the active Source callback owns typed
/// decoding and resume-state construction. Current Cranelift lowering does not
/// emit this call, so planner-level entry fallback uses the typed request
/// factory above instead.
pub(crate) fn jet_deopt_call(
    function: i64,
    argc: i64,
    a0: i64,
    a1: i64,
    a2: i64,
    a3: i64,
    a4: i64,
    a5: i64,
    a6: i64,
    a7: i64,
) -> i64 {
    crate::trace::note_deopt_invoked_for_test();
    let abi_args = [a0, a1, a2, a3, a4, a5, a6, a7];
    let Some(snapshot) = record_abi_frame(function, argc, &abi_args) else {
        record_source_deopt_failure(
            "MIR deopt requested with an invalid or unavailable typed interpreter frame",
        );
        return 0;
    };
    if let Ok(function) = u64::try_from(function) {
        super::resident::publish_runtime_deopt(
            MirFunctionId(function),
            "native frame requested Source evaluator deopt",
        );
    }
    let Ok(argc) = usize::try_from(argc) else {
        record_source_deopt_failure("MIR deopt argument count is outside usize");
        return 0;
    };
    let mut request = SourceDeoptRequest {
        function: snapshot.identity.function,
        snapshot,
        entry_params: Vec::new(),
        argc,
        args: abi_args.map(|bits| bits as u64),
        entry_values: Vec::new(),
        entry_frame_installed: false,
        entry: false,
    };
    match dispatch_source_deopt(&mut request) {
        Ok(reply) => {
            publish_source_deopt_reply(&reply, true);
            reply.bits
        }
        Err(error) => {
            record_source_deopt_failure(format!("Source MIR deopt resume failed: {error}"));
            0
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_source_deopt_publishes_native_stop_before_resume() {
        let mut runtime = crate::resident::fresh_runtime(
            jet_pkg_model::Package::ReleaseDevtoolsPolicy::default(),
        );
        let runtime_ptr = &mut runtime as *mut crate::JitRuntime;
        crate::Concurrency::set_active_runtime(Some(runtime_ptr));

        publish_source_deopt_reply(
            &SourceDeoptReply {
                bits: 0,
                outcome: Some(RunOutcome::Ran {
                    stdout: String::new(),
                    stderr: String::new(),
                    exit_code: 0,
                }),
                value: None,
                session: None,
                soft_stop: false,
                stdout: String::new(),
                stderr: String::new(),
            },
            true,
        );
        let trapped = crate::Concurrency::with_runtime_mut(|runtime| runtime.trap_pending());
        crate::Concurrency::set_active_runtime(None);

        assert!(trapped, "terminal Source deopt must stop subsequent native polls");
        clear_deopt_state();
    }
}
