// Private Source-MIR -> Source evaluator bridge for the generated compiler
// Runner.
//
// This file is included only in the generated private compiler artifact.  The
// generated Host supplies the typed source-carrier decoder; this module owns
// the evaluator handoff boundary, artifact/entry identity checks, Source
// resource activation, and the Source callback lifetime.  It never parses
// source text, re-lowers TIR, or invokes the Rust MIR evaluator.

use jet_foundation::JitBackend::RunOutcome;
use jet_foundation::MIR::{
    MirArtifactId, MirExecutionIdentity, MirFunctionId, MirProgram, MirRuntimeValue,
};
use jet_evalhost::SourceResources::SourceResourceSession;
use jet_evalhost::{SourceEntryReply, SourceEntryRequest};

/// The decoded MIR and physical resource lease are retained so a Source
/// callback can keep the exact program identity, schema, and owner arena
/// alive until the logical evaluator session has retired it.
// The resource session intentionally has no Debug projection: its physical
// owners are opaque and remain inside the invocation arena.
pub(crate) struct SourceMirExecution {
    /// Typed Source-terminal outcome; this seam never renders or
    /// concatenates output.
    pub(crate) outcome: RunOutcome,
    /// Exact recursive terminal value returned by Source.
    pub(crate) value: Option<MirRuntimeValue>,
    pub(crate) soft_stop: bool,
    /// Cumulative output snapshot; callers must consume this path once.
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) program: MirProgram,
    pub(crate) identity: MirExecutionIdentity,
    pub(crate) artifact: MirArtifactId,
    pub(crate) entry_function: MirFunctionId,
    /// Keeps the physical Source arena root alive until the generated Runner
    /// explicitly retires this execution's logical evaluator state.
    pub(crate) resources: SourceResourceSession,
    /// Logical Source/Eval session guard retained through value adoption and
    /// explicit cleanup/queue drain by the generated Runner.
    pub(crate) source_session: Option<Box<dyn jet_evalhost::SourceExecutionGuard>>,
}
/// Failure before an evaluator result exists. A Source runtime diagnostic is
/// deliberately kept in `SourceMirExecution::outcome`, not collapsed into
/// this carrier/identity error rail.
#[derive(Debug)]
pub(crate) enum SourceMirExecutionError {
    Carrier(String),
    Identity(String),
    Resource(String),
    Backend(String),
    Retirement(jet_evalhost::SourceExecutionRetirementError),
}

impl SourceMirExecution {
    /// Retire the logical Source/Eval graph only after the Runner has decoded
    /// and adopted the returned value. Physical resource lookup stays active
    /// while cleanup drains callback roots and transfers; cleanup's final
    /// outcome/value replace the pre-retirement projections when present.
    pub(crate) fn retire_source_session(
        self,
    ) -> Result<jet_evalhost::SourceExecutionRetirement, SourceMirExecutionError> {
        let SourceMirExecution {
            outcome,
            value,
            soft_stop,
            stdout,
            stderr,
            resources,
            source_session,
            ..
        } = self;
        jet_evalhost::retire_source_entry(
            resources,
            source_session,
            jet_evalhost::SourceExecutionRetirement {
                outcome: Some(outcome),
                value,
                soft_stop,
                stdout,
                stderr,
                completions: Vec::new(),
            },
        )
        .map_err(SourceMirExecutionError::Retirement)
    }
}


impl std::fmt::Display for SourceMirExecutionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Carrier(message) => write!(formatter, "Source MIR carrier conversion failed: {message}"),
            Self::Identity(message) => write!(formatter, "Source MIR execution identity failed: {message}"),
            Self::Resource(message) => write!(formatter, "Source resource activation failed: {message}"),
            Self::Backend(message) => write!(formatter, "Source MIR evaluator handoff failed: {message}"),
            Self::Retirement(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for SourceMirExecutionError {}

/// Hand the already-decoded canonical MIR to the Source evaluator. The caller
/// must provide the physical invocation session retained by generated
/// Native/Runner glue; this function activates it explicitly and restores any
/// outer activation on both return and unwind. Explicit resource/session
/// retirement lives in the transport-neutral `jet_evalhost` seam; this
/// wrapper retains checked identity and decoded-program metadata for
/// generated Runner glue.
///
/// `resume` is Source-owned typed evaluator logic. Entry requests carry the
/// checked identity, parameter facts, and recursively typed canonical values.
/// It may publish a typed terminal `RunOutcome`. It is never a Rust MIREval
/// callback. The callback is scoped to this invocation, so nested Source
/// invocations select the innermost handler without a session-ID registry.
pub(crate) fn execute_decoded_source_mir<Resume>(
    program: MirProgram,
    artifact: MirArtifactId,
    entry_function: MirFunctionId,
    resources: &SourceResourceSession,
    entry_values: Vec<MirRuntimeValue>,
    resume: Resume,
) -> Result<SourceMirExecution, SourceMirExecutionError>
where
    Resume: Fn(&mut SourceEntryRequest) -> Result<SourceEntryReply, String> + 'static,
{
    let identity = program
        .execution_identity(Some(artifact))
        .map_err(|error| SourceMirExecutionError::Identity(error.to_string()))?;
    let execution = jet_evalhost::execute_source_entry(
        &program,
        artifact,
        entry_function,
        resources.clone(),
        entry_values,
        resume,
    )
    .map_err(|error| match error {
        jet_evalhost::SourceEntryError::Resource(message) => SourceMirExecutionError::Resource(message),
        error => SourceMirExecutionError::Backend(error.to_string()),
    })?;
    let jet_evalhost::SourceEntryExecution {
        outcome,
        value,
        soft_stop,
        stdout,
        stderr,
        resources: execution_resources,
        session: source_session,
    } = execution;
    Ok(SourceMirExecution {
        outcome,
        value,
        soft_stop,
        stdout,
        stderr,
        program,
        identity,
        artifact,
        entry_function,
        resources: execution_resources,
        source_session,
    })
}

/// Decode the generated Source MIR carrier with its exact binding metadata,
/// then hand the decoded program to the Source evaluator. The decoder is the
/// generated typed MIR codec; no textual or legacy reader is accepted. The
/// recursive entry values are moved into the Source entry request.
pub(crate) fn execute_source_mir<SourceProgram, Decode, Resume>(
    source_program: &SourceProgram,
    artifact: MirArtifactId,
    entry_function: MirFunctionId,
    bindings: &crate::BootstrapBindingDescriptor,
    resources: &SourceResourceSession,
    entry_values: Vec<MirRuntimeValue>,
    decode: Decode,
    resume: Resume,
) -> Result<SourceMirExecution, SourceMirExecutionError>
where
    Decode: FnOnce(
        &SourceProgram,
        &crate::BootstrapBindingDescriptor,
    ) -> Result<MirProgram, String>,
    Resume: Fn(&mut SourceEntryRequest) -> Result<SourceEntryReply, String> + 'static,
{
    let program = decode(source_program, bindings).map_err(SourceMirExecutionError::Carrier)?;
    execute_decoded_source_mir(
        program,
        artifact,
        entry_function,
        resources,
        entry_values,
        resume,
    )
}

