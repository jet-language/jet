// Private Source-MIR -> Cranelift bridge for the generated compiler Runner.
//
// This file is included only in the generated private compiler artifact.  The
// generated Host supplies the typed source-carrier decoder; this module owns
// the backend boundary, artifact/entry identity checks, Source resource
// activation, and nested Source deopt callback lifetime.  It never parses
// source text, re-lowers TIR, or invokes the Rust MIR evaluator.

use jet_foundation::JitBackend::RunOutcome;
use jet_foundation::MIR::{
    MirArtifactId, MirExecutionIdentity, MirFunctionId, MirProgram, MirRuntimeValue,
};
use jet_pkg_model::Package::ReleaseDevtoolsPolicy;
use jet_jit::SourceResources::SourceResourceSession;
use jet_jit::{SourceDeoptReply, SourceDeoptRequest};

/// The decoded MIR and physical resource lease are retained so a Source
/// deopt callback can keep the exact program identity, schema, and owner arena
/// alive until the logical evaluator session has retired it.
// The resource session intentionally has no Debug projection: its physical
// owners are opaque and remain inside the invocation arena.
pub(crate) struct SourceMirExecution {
    /// Typed native or Source-terminal outcome; this seam never renders or
    /// concatenates output from the two tiers.
    pub(crate) outcome: RunOutcome,
    /// Exact recursive terminal value returned by Source or a supported native
    /// scalar/Unit entry. Unsupported native return metadata is routed Source.
    pub(crate) value: Option<MirRuntimeValue>,
    pub(crate) soft_stop: bool,
    /// Cumulative output snapshot; callers must consume this path once.
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    /// Receipt of the tier that actually consumed the checked entry.
    pub(crate) tier: jet_jit::SourceExecutionTier,
    pub(crate) program: MirProgram,
    pub(crate) identity: MirExecutionIdentity,
    pub(crate) artifact: MirArtifactId,
    pub(crate) entry_function: MirFunctionId,
    /// Keeps the physical Source arena root alive until the generated Runner
    /// explicitly retires this execution's logical evaluator state.
    pub(crate) resources: SourceResourceSession,
    /// Logical Source/Eval session guard retained through value adoption and
    /// explicit cleanup/queue drain by the generated Runner.
    pub(crate) source_session: Option<Box<dyn jet_jit::SourceExecutionGuard>>,
}
/// Failure before a backend result exists. A Cranelift runtime diagnostic is
/// deliberately kept in `SourceMirExecution::outcome`, not collapsed into
/// this carrier/identity error rail.
#[derive(Debug)]
pub(crate) enum SourceMirExecutionError {
    Carrier(String),
    Identity(String),
    Resource(String),
    Backend(String),
    Retirement(jet_jit::SourceExecutionRetirementError),
}

impl SourceMirExecution {
    /// Retire the logical Source/Eval graph only after the Runner has decoded
    /// and adopted the returned value. Physical resource lookup stays active
    /// while cleanup drains callback roots and transfers; cleanup's final
    /// outcome/value replace the pre-retirement projections when present.
    pub(crate) fn retire_source_session(
        self,
    ) -> Result<jet_jit::SourceExecutionRetirement, SourceMirExecutionError> {
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
        jet_jit::retire_source_entry(
            resources,
            source_session,
            jet_jit::SourceExecutionRetirement {
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
            Self::Backend(message) => write!(formatter, "Source MIR backend handoff failed: {message}"),
            Self::Retirement(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for SourceMirExecutionError {}

/// Invoke the already-decoded canonical MIR through the permitted Cranelift
/// backend.  The caller must provide the physical invocation session retained
/// by generated Native/Runner glue; this function activates it explicitly and
/// restores any outer activation on both return and unwind.
/// Generic tier execution and explicit resource/session retirement live in the
/// transport-neutral `jet_jit` seam; this wrapper retains checked identity and
/// decoded-program metadata for generated Runner glue.
///
/// `resume` is Source-owned typed evaluator logic. Entry requests carry the
/// checked identity, parameter facts, and recursively typed canonical values;
/// existing-frame requests carry only exact ABI words and must join them to
/// their invocation-local Source machine state. It returns one checked ABI
/// word and may publish a typed terminal `RunOutcome`. It is never a Rust
/// MIREval callback. The callback is owned by the nested JIT scope, so nested
/// Source/JIT invocations select the innermost handler without a session-ID
/// registry.
pub(crate) fn execute_decoded_source_mir<Resume>(
    program: MirProgram,
    artifact: MirArtifactId,
    entry_function: MirFunctionId,
    resources: &SourceResourceSession,
    entry_values: Vec<MirRuntimeValue>,
    policy: ReleaseDevtoolsPolicy,
    resume: Resume,
) -> Result<SourceMirExecution, SourceMirExecutionError>
where
    Resume: Fn(SourceDeoptRequest) -> Result<SourceDeoptReply, String> + 'static,
{
    let identity = program
        .execution_identity(Some(artifact))
        .map_err(|error| SourceMirExecutionError::Identity(error.to_string()))?;
    let native_run = jet_jit::execute_source_entry(
        &program,
        artifact,
        entry_function,
        resources.clone(),
        entry_values,
        &policy,
        resume,
    )
    .map_err(|error| match error {
        jet_jit::SourceDeoptError::Resource(message) => SourceMirExecutionError::Resource(message),
        error => SourceMirExecutionError::Backend(error.to_string()),
    })?;
    let jet_jit::SourceEntryExecution {
        outcome: native_outcome,
        value: native_value,
        soft_stop: native_soft_stop,
        stdout: native_stdout,
        stderr: native_stderr,
        tier: native_tier,
        resources: execution_resources,
        session: source_session,
    } = native_run;
    Ok(SourceMirExecution {
        outcome: native_outcome,
        value: native_value,
        soft_stop: native_soft_stop,
        stdout: native_stdout,
        stderr: native_stderr,
        tier: native_tier,
        program,
        identity,
        artifact,
        entry_function,
        resources: execution_resources,
        source_session,
    })
}

/// Decode the generated Source MIR carrier with its exact binding metadata,
/// then execute the decoded program through the backend. The decoder is the
/// generated typed MIR codec; no textual or legacy reader is accepted. The
/// recursive entry values are moved into the invocation/deopt request.
pub(crate) fn execute_source_mir<SourceProgram, Decode, Resume>(
    source_program: &SourceProgram,
    artifact: MirArtifactId,
    entry_function: MirFunctionId,
    bindings: &crate::BootstrapBindingDescriptor,
    resources: &SourceResourceSession,
    entry_values: Vec<MirRuntimeValue>,
    policy: ReleaseDevtoolsPolicy,
    decode: Decode,
    resume: Resume,
) -> Result<SourceMirExecution, SourceMirExecutionError>
where
    Decode: FnOnce(
        &SourceProgram,
        &crate::BootstrapBindingDescriptor,
    ) -> Result<MirProgram, String>,
    Resume: Fn(SourceDeoptRequest) -> Result<SourceDeoptReply, String> + 'static,
{
    let program = decode(source_program, bindings).map_err(SourceMirExecutionError::Carrier)?;
    execute_decoded_source_mir(
        program,
        artifact,
        entry_function,
        resources,
        entry_values,
        policy,
        resume,
    )
}

