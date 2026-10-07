// Private orchestration for the source-coupled compiler artifact.
//
// The generated Jet factory returns one checked target result. Native artifacts
// receive the canonical Host splice, Web artifacts retain their exact payloads,
// and user-program Source Runtime artifacts enter the checked whole-entry JIT.
// This is separate from executing the compiler factory itself; the runner never
// parses Jet source, re-runs sema, or infers route policy.

use crate::Codegen::MIRRust::{MirRustAotMetadata, MirRustConfig};
use crate::{append_bootstrap_host_glue, BootstrapBindingDescriptor, BootstrapEntryCodec, BootstrapHostCodecError};
use crate::compiler_bootstrap_host::{
    invoke_with_authority, AuthorizedSourceLease, AuthorizedSourceSnapshot,
};
use crate::Authority::AuthorityError;
use jet_foundation::JitBackend::RunOutcome;
use jet_foundation::MIR::{
    MirArtifactId, MirArtifactTarget, MirFunctionId, MirProgram, MirRuntimeValue,
};
use jet_jit::SourceResources::{
    SourceResourceLease, SourceResourceRetireError, SourceResourceSession,
};
use std::path::Path;
use std::fmt::Write as _;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum BootstrapFactoryTier {
    Aot,
    CraneliftJit,
    SourceInterpreterDeopt,
}

/// One private compiler artifact after the canonical Host adapter has been
/// spliced. `source` is the exact source passed to the permitted backend;
/// `bindings` is the same descriptor used to validate and emit the splice.
/// `compiler_image` is the embedded compiler MIR image: the source names it as
/// `include_bytes!("compiler.image")`, so the backend writes these bytes to
/// `compiler.image` beside the crate source file that holds the item.
#[derive(Debug)]
pub(crate) struct BootstrapArtifact {
    pub(crate) source: String,
    pub(crate) compiler_image: Option<Vec<u8>>,
    pub(crate) bindings: BootstrapBindingDescriptor,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BootstrapWebArtifacts {
    pub(crate) manifest_json: String,
    pub(crate) wasm_rust: String,
    pub(crate) rustc_incremental_identity: String,
    pub(crate) js_app: String,
    pub(crate) js_source_map: String,
    pub(crate) source_names: Vec<String>,
    pub(crate) source_contents: Vec<String>,
    pub(crate) dom_runtime: String,
    pub(crate) index_html: String,
    pub(crate) explicit_html_path: Option<String>,
    pub(crate) command_record: Vec<u8>,
}
#[derive(Debug)]
pub(crate) enum BootstrapBackendArtifact<'a> {
    /// `compiler_image` is present only for compiler artifacts; the source
    /// then embeds it from `compiler.image` beside the crate source file.
    NativeRust {
        source: String,
        compiler_image: Option<Vec<u8>>,
        bindings: BootstrapBindingDescriptor,
    },
    Web {
        artifact: MirArtifactId,
        artifacts: &'a BootstrapWebArtifacts,
    },
}

/// Result decoded from the generated Jet compiler. Incomplete user compiles
/// keep their diagnostics but intentionally carry no backend source or rows.
pub(crate) struct BootstrapJetCompileResult<SourceProgram = (), RuntimeConfig = ()> {
    pub(crate) selected_factory_tier: BootstrapFactoryTier,
    pub(crate) actual_factory_tier: BootstrapFactoryTier,
    pub(crate) complete: bool,
    pub(crate) emitted_source: Option<String>,
    pub(crate) bindings: Option<BootstrapBindingDescriptor>,
    pub(crate) source_program: Option<SourceProgram>,
    pub(crate) runtime_config: Option<RuntimeConfig>,
    pub(crate) mir: Option<MirProgram>,
    pub(crate) entry_function: Option<MirFunctionId>,
    pub(crate) runtime_artifact: Option<MirArtifactId>,
    pub(crate) web_artifact: Option<MirArtifactId>,
    pub(crate) web_artifacts: Option<BootstrapWebArtifacts>,
    pub(crate) comptime_stdout: String,
    pub(crate) comptime_stderr: String,
    pub(crate) soft_stop: bool,
    pub(crate) exit_code: Option<i64>,
    pub(crate) internal_problem: Option<String>,
    pub(crate) reports: Vec<jet_foundation::Report::ReportEnvelope>,
    pub(crate) resources: Option<SourceResourceSession>,
}

pub(crate) type BootstrapSourceResume =
    Box<dyn Fn(&mut jet_jit::SourceDeoptRequest) -> Result<jet_jit::SourceDeoptReply, String> + 'static>;
pub(crate) type BootstrapSourceResumeFactory<'a, SourceProgram, RuntimeConfig> =
    Box<dyn FnOnce(SourceProgram, RuntimeConfig, SourceResourceLease) -> BootstrapSourceResume + 'a>;

pub(crate) struct BootstrapRunOutput<BackendOutput, SourceProgram = (), RuntimeConfig = ()> {
    pub(crate) selected_factory_tier: BootstrapFactoryTier,
    pub(crate) actual_factory_tier: BootstrapFactoryTier,
    pub(crate) complete: bool,
    pub(crate) backend: Option<BackendOutput>,
    pub(crate) web_artifacts: Option<BootstrapWebArtifacts>,
    pub(crate) outcome: Option<RunOutcome>,
    pub(crate) value: Option<MirRuntimeValue>,
    pub(crate) completion_owner: BootstrapRunCompletionOwner,
    pub(crate) soft_stop: bool,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
    pub(crate) exit_code: Option<i64>,
    pub(crate) comptime_stdout: String,
    pub(crate) comptime_stderr: String,
    pub(crate) comptime_soft_stop: bool,
    pub(crate) comptime_exit_code: Option<i64>,
    pub(crate) reports: Vec<jet_foundation::Report::ReportEnvelope>,
    pub(crate) source_program: Option<SourceProgram>,
    pub(crate) runtime_config: Option<RuntimeConfig>,
    pub(crate) mir: Option<MirProgram>,
    pub(crate) entry_function: Option<MirFunctionId>,
    pub(crate) artifact: Option<MirArtifactId>,
}

/// Result of finishing a Runner-owned completion receiver. Ready completions
/// must be consumed/dropped before calling again; Pending reports explicit
/// Source-root liveness and never treats a strong-count snapshot as retirement.
#[derive(Debug)]
pub(crate) enum BootstrapRunCompletionFinish {
    Ready(Vec<jet_jit::SourceExecutionCompletion>),
    Pending {
        retirement_requested: bool,
        retained_roots: usize,
        open_callback_sessions: usize,
        pending_jobs: usize,
    },
    Complete {
        failures: Vec<jet_jit::JetTaskFailure>,
    },
}

#[derive(Debug)]
pub(crate) enum BootstrapRunCompletionFinishError {
    Retirement {
        error: SourceResourceRetireError,
        completions: Vec<jet_jit::SourceExecutionCompletion>,
        callback_failures: Vec<jet_jit::JetTaskFailure>,
    },
    State(String),
    Unrouted(jet_jit::SourceExecutionCompletion),
}

#[derive(Default)]
struct BootstrapRunCallbackJobState {
    complete: bool,
    failures: Vec<jet_jit::JetTaskFailure>,
    failures_delivered: bool,
}

#[derive(Clone)]
pub(crate) struct BootstrapRunCompletionOwner {
    scope: jet_jit::SourceExecutionCompletionScope,
    resources: Option<SourceResourceSession>,
    callback_jobs: Option<std::sync::Arc<dyn jet_jit::SourceCallbacks::SourceCallbackJobOwner>>,
    callback_job_state: std::sync::Arc<std::sync::Mutex<BootstrapRunCallbackJobState>>,
    allow_resource_retirement: bool,
}

impl std::fmt::Debug for BootstrapRunCompletionOwner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BootstrapRunCompletionOwner")
            .field("resources_present", &self.resources.is_some())
            .field("callback_jobs_present", &self.callback_jobs.is_some())
            .field("allow_resource_retirement", &self.allow_resource_retirement)
            .finish_non_exhaustive()
    }
}

impl BootstrapRunCompletionOwner {
    pub(crate) fn new(scope: jet_jit::SourceExecutionCompletionScope) -> Self {
        Self {
            scope,
            resources: None,
            callback_jobs: None,
            callback_job_state: std::sync::Arc::default(),
            allow_resource_retirement: false,
        }
    }

    pub(crate) fn set_resources(&mut self, resources: SourceResourceSession) {
        self.resources = Some(resources);
    }
    pub(crate) fn allow_resource_retirement(&mut self) {
        self.allow_resource_retirement = true;
    }

    pub(crate) fn register_callback_jobs(
        &mut self,
        callback_jobs: std::sync::Arc<
            dyn jet_jit::SourceCallbacks::SourceCallbackJobOwner,
        >,
    ) -> Result<(), std::sync::Arc<dyn jet_jit::SourceCallbacks::SourceCallbackJobOwner>> {
        if self.callback_jobs.is_some() {
            return Err(callback_jobs);
        }
        self.callback_jobs = Some(callback_jobs);
        Ok(())
    }

    pub(crate) fn completion_scope(&self) -> jet_jit::SourceExecutionCompletionScope {
        self.scope.clone()
    }

    /// Call after consuming returned runtime values and retiring escaped
    /// callback/task owners. A Ready batch is an exact typed delivery obligation;
    /// call again after consuming it to prove all jobs and physical resources retired.
    pub(crate) fn finish(
        &self,
    ) -> Result<BootstrapRunCompletionFinish, BootstrapRunCompletionFinishError> {
        let completions = self.scope.drain();
        if !completions.is_empty() {
            return Ok(BootstrapRunCompletionFinish::Ready(completions));
        }
        let (open_callback_sessions, pending_jobs, callback_jobs_pending) = {
            let mut state = self
                .callback_job_state
                .lock()
                .map_err(|error| BootstrapRunCompletionFinishError::State(error.to_string()))?;
            if state.complete {
                (0, 0, false)
            } else {
                match &self.callback_jobs {
                    Some(callback_jobs) => {
                        callback_jobs.close_admission();
                        match callback_jobs.drain() {
                        jet_jit::SourceCallbacks::SourceCallbackJobDrainOutcome::Pending {
                            open_callback_sessions,
                            pending_jobs,
                        } => (open_callback_sessions, pending_jobs, true),
                        jet_jit::SourceCallbacks::SourceCallbackJobDrainOutcome::Complete {
                            failures,
                        } => {
                            state.complete = true;
                            state.failures = failures;
                            (0, 0, false)
                        }
                        }
                    }
                    None => {
                        state.complete = true;
                        (0, 0, false)
                    }
                }
            }
        };
        let completions = self.scope.drain();
        if !completions.is_empty() {
            return Ok(BootstrapRunCompletionFinish::Ready(completions));
        }
        if callback_jobs_pending {
            let (retirement_requested, retained_roots) = self.resource_liveness()?;
            return Ok(BootstrapRunCompletionFinish::Pending {
                retirement_requested,
                retained_roots,
                open_callback_sessions,
                pending_jobs,
            });
        }
        let Some(resources) = &self.resources else {
            return Ok(BootstrapRunCompletionFinish::Complete {
                failures: self.take_callback_failures()?,
            });
        };
        let (retirement_requested, retained_roots) = self.resource_liveness()?;
        if retained_roots != 0 || !retirement_requested {
            return Ok(BootstrapRunCompletionFinish::Pending {
                retirement_requested,
                retained_roots,
                open_callback_sessions,
                pending_jobs,
            });
        }
        let retired = resources
            .arena()
            .is_retired()
            .map_err(BootstrapRunCompletionFinishError::State)?;
        if retired {
            return Ok(BootstrapRunCompletionFinish::Complete {
                failures: self.take_callback_failures()?,
            });
        }
        if !self.allow_resource_retirement {
            return Ok(BootstrapRunCompletionFinish::Pending {
                retirement_requested,
                retained_roots,
                open_callback_sessions,
                pending_jobs,
            });
        }
        let newly_retired = match self.scope.with_current(|| resources.retire()) {
            Ok(completions) => completions,
            Err(error) => {
                return Err(BootstrapRunCompletionFinishError::Retirement {
                    error,
                    completions: self.scope.drain(),
                    callback_failures: self.take_callback_failures()?,
                });
            }
        };
        for completion in newly_retired {
            self.scope.record(completion);
        }
        let completions = self.scope.drain();
        if !completions.is_empty() {
            return Ok(BootstrapRunCompletionFinish::Ready(completions));
        }
        let (retirement_requested, retained_roots) = self.resource_liveness()?;
        let retired = resources
            .arena()
            .is_retired()
            .map_err(BootstrapRunCompletionFinishError::State)?;
        if retired {
            Ok(BootstrapRunCompletionFinish::Complete {
                failures: self.take_callback_failures()?,
            })
        } else {
            Ok(BootstrapRunCompletionFinish::Pending {
                retirement_requested,
                retained_roots,
                open_callback_sessions,
                pending_jobs,
            })
        }
    }
    /// Whether physical resource retirement was requested and how many roots
    /// still hold the arena open. No resource session means neither.
    fn resource_liveness(&self) -> Result<(bool, usize), BootstrapRunCompletionFinishError> {
        let Some(resources) = &self.resources else {
            return Ok((false, 0));
        };
        let arena = resources.arena();
        let retirement_requested = arena
            .is_retirement_requested()
            .map_err(BootstrapRunCompletionFinishError::State)?;
        let retained_roots = arena
            .retained_root_count()
            .map_err(BootstrapRunCompletionFinishError::State)?;
        Ok((retirement_requested, retained_roots))
    }
    fn take_callback_failures(
        &self,
    ) -> Result<Vec<jet_jit::JetTaskFailure>, BootstrapRunCompletionFinishError> {
        let mut state = self
            .callback_job_state
            .lock()
            .map_err(|error| BootstrapRunCompletionFinishError::State(error.to_string()))?;
        if !state.complete {
            return Err(BootstrapRunCompletionFinishError::State(
                "callback job failures requested before the job owner reached Complete".to_string(),
            ));
        }
        if state.failures_delivered {
            return Ok(Vec::new());
        }
        state.failures_delivered = true;
        Ok(std::mem::take(&mut state.failures))
    }
}

impl<BackendOutput, SourceProgram, RuntimeConfig>
    BootstrapRunOutput<BackendOutput, SourceProgram, RuntimeConfig>
{
    pub(crate) fn finish_source_completions(
        &self,
    ) -> Result<BootstrapRunCompletionFinish, BootstrapRunCompletionFinishError> {
        self.completion_owner.finish()
    }
}


/// Failure while moving one authority-pinned generated result into the
/// permitted backend. Authority failures remain distinct from carrier/codec
/// failures so callers cannot mistake an input change for a compiler result.
#[derive(Debug)]
pub(crate) enum BootstrapRunError {
    Authority(AuthorityError),
    Codec(BootstrapHostCodecError),
    CompilerInternal {
        error: BootstrapHostCodecError,
        reports: Vec<jet_foundation::Report::ReportEnvelope>,
    },
    CompilerInternalResourceRetirement {
        error: BootstrapHostCodecError,
        reports: Vec<jet_foundation::Report::ReportEnvelope>,
        retirement: SourceResourceRetireError,
    },
    SourceRuntime(String),
    SourceRuntimeResourceRetirement {
        error: String,
        retirement: SourceResourceRetireError,
    },
    ResourceRetirement(SourceResourceRetireError),
    SourceExecution(jet_jit::SourceDeoptError),
    SourceExecutionResourceRetirement {
        execution: jet_jit::SourceDeoptError,
        retirement: SourceResourceRetireError,
    },
    SourceExecutionRetirement(jet_jit::SourceExecutionRetirementError),
    UnroutedCompletion(jet_jit::SourceExecutionCompletion),
    WithCompletionOwner {
        error: Box<BootstrapRunError>,
        completion_owner: BootstrapRunCompletionOwner,
    },
}

impl BootstrapRunError {
    pub(crate) fn finish_source_completions(
        &self,
    ) -> Result<BootstrapRunCompletionFinish, BootstrapRunCompletionFinishError> {
        match self {
            Self::WithCompletionOwner {
                completion_owner, ..
            } => completion_owner.finish(),
            _ => Ok(BootstrapRunCompletionFinish::Complete { failures: Vec::new() }),
        }
    }

    fn preserves_pending_retirement_error(&self) -> bool {
        match self {
            Self::CompilerInternalResourceRetirement { .. }
            | Self::ResourceRetirement(_)
            | Self::SourceRuntimeResourceRetirement { .. }
            | Self::SourceExecutionResourceRetirement { .. }
            | Self::SourceExecutionRetirement(_) => true,
            Self::WithCompletionOwner { error, .. } => {
                error.preserves_pending_retirement_error()
            }
            _ => false,
        }
    }
}

impl From<AuthorityError> for BootstrapRunError {
    fn from(error: AuthorityError) -> Self {
        Self::Authority(error)
    }
}

impl From<BootstrapHostCodecError> for BootstrapRunError {
    fn from(error: BootstrapHostCodecError) -> Self {
        Self::Codec(error)
    }
}

/// Append the one canonical Host adapter to a source artifact using the
/// mechanically supplied binding rows. The caller owns backend compilation.
#[allow(clippy::needless_pass_by_value)]
pub(crate) fn prepare_bootstrap_artifact(
    mut source: String,
    program: &MirProgram,
    config: &MirRustConfig<'_>,
    bindings: BootstrapBindingDescriptor,
) -> Result<BootstrapArtifact, BootstrapHostCodecError> {
    append_bootstrap_host_glue(&mut source, config, &bindings, program)?;
    source = package_bootstrap_artifact(source)?;
    Ok(BootstrapArtifact {
        source,
        compiler_image: None,
        bindings,
    })
}

/// Stage-0 adapter: reference Rust emission already provides the exact rows;
/// the self-emitted path uses `prepare_bootstrap_artifact` with rows decoded
/// from the Jet-produced manifest instead.
pub(crate) fn prepare_bootstrap_artifact_from_aot(
    source: String,
    program: &MirProgram,
    source_authority: &AuthorizedSourceSnapshot,
    config: &MirRustConfig<'_>,
    metadata: &MirRustAotMetadata,
) -> Result<BootstrapArtifact, BootstrapHostCodecError> {
    let bindings = BootstrapBindingDescriptor::from_aot(program, metadata)?;
    let mut entries = bindings
        .callables
        .iter()
        .filter(|callable| callable.source_name == "jet_bootstrap_compile");
    let compiler_entry_function = entries
        .next()
        .map(|callable| callable.metadata.function)
        .ok_or_else(|| {
            BootstrapHostCodecError::MissingEntry("jet_bootstrap_compile".to_string())
        })?;
    if entries.next().is_some() {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "stage-zero compiler MIR has multiple jet_bootstrap_compile bindings".to_string(),
        ));
    }
    let entry = program
        .functions
        .iter()
        .find(|function| function.id == compiler_entry_function)
        .ok_or_else(|| {
            BootstrapHostCodecError::InvalidMetadata(
                "stage-zero jet_bootstrap_compile binding is absent from MIR".to_string(),
            )
        })?;
    if !entry.capture_params.is_empty() {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "stage-zero jet_bootstrap_compile entry cannot capture values".to_string(),
        ));
    }
    if !entry.target_applicability.rust_aot {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "stage-zero jet_bootstrap_compile root is not Rust AOT-applicable".to_string(),
        ));
    }
    // Packaging keeps going past a failed check so one stage-zero run reports
    // every failure: entry ABI, artifact plan, image archive and host glue.
    let mut errors = Vec::new();
    if let Err(error) = BootstrapEntryCodec::new(program, &bindings, compiler_entry_function) {
        errors.push(error);
    }
    let compiler_artifact = config.execution.artifact;
    match program.artifacts.iter().find(|artifact| artifact.id == compiler_artifact) {
        None => errors.push(BootstrapHostCodecError::InvalidMetadata(format!(
            "stage-zero compiler artifact {compiler_artifact:?} is absent from MIR"
        ))),
        Some(artifact_plan)
            if artifact_plan.target != MirArtifactTarget::RustAot
                || !artifact_plan.modules.contains(&entry.module_id) =>
        {
            errors.push(BootstrapHostCodecError::InvalidMetadata(
                "stage-zero compiler artifact does not contain the private Rust AOT compiler factory root"
                    .to_string(),
            ));
        }
        Some(_) => {}
    }
    let image_bytes = crate::compiler_bootstrap_compiler_image::archive_compiler_image(
        program,
        program,
        compiler_artifact,
        compiler_entry_function,
        source_authority,
        |program| Ok(program.clone()),
    )
    .map_err(|error| {
        BootstrapHostCodecError::InvalidMetadata(format!(
            "cannot archive stage-zero compiler MIR: {error}"
        ))
    });
    let source_authority_digest =
        crate::compiler_bootstrap_compiler_image::compiler_image_source_authority_digest(
            source_authority,
        );
    let artifact = prepare_bootstrap_artifact(source, program, config, bindings);
    let (mut artifact, image_bytes) = match (artifact, image_bytes) {
        (Ok(artifact), Ok(image_bytes)) if errors.is_empty() => (artifact, image_bytes),
        (artifact, image_bytes) => {
            errors.extend(artifact.err());
            errors.extend(image_bytes.err());
            return Err(BootstrapHostCodecError::combine(errors));
        }
    };
    append_embedded_compiler_image(
        &mut artifact,
        image_bytes,
        source_authority_digest,
        compiler_artifact,
        compiler_entry_function,
    )?;
    Ok(artifact)
}
/// Embed the compiler image through `include_bytes!` from `compiler.image`
/// beside the crate source file: an inline byte-string literal of the image
/// would be a single multi-hundred-megabyte token for rustc to lex.
fn append_embedded_compiler_image(
    artifact: &mut BootstrapArtifact,
    image: Vec<u8>,
    source_authority_digest: [u8; 32],
    artifact_id: MirArtifactId,
    entry_function: MirFunctionId,
) -> Result<(), BootstrapHostCodecError> {
    let source = &mut artifact.source;
    source.push_str(
        "\n#[doc(hidden)]\n\
         const __JET_BOOTSTRAP_COMPILER_IMAGE_BYTES: &[u8] = include_bytes!(\"compiler.image\");\n\
         #[doc(hidden)]\n\
         const __JET_BOOTSTRAP_COMPILER_SOURCE_AUTHORITY_DIGEST: [u8; 32] = [",
    );
    for (index, byte) in source_authority_digest.iter().enumerate() {
        if index != 0 {
            source.push(',');
        }
        write!(source, "{byte}")
            .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    }
    source.push_str(
        "];\n\
         #[doc(hidden)]\n\
         pub(crate) fn __jet_bootstrap_restore_compiler_image() -> Result<\n\
             crate::compiler_bootstrap_compiler_image::RestoredCompilerImage<crate::__JetBootstrapSourceProgram>,\n\
             crate::compiler_bootstrap_compiler_image::CompilerImageError,\n\
         > {\n\
             crate::compiler_bootstrap_compiler_image::restore_compiler_image(\n\
                 __JET_BOOTSTRAP_COMPILER_IMAGE_BYTES,\n\
                 __JET_BOOTSTRAP_COMPILER_SOURCE_AUTHORITY_DIGEST,\n",
    );
    writeln!(
        source,
        "                 ::jet_foundation::MIR::MirArtifactId({}),",
        artifact_id.0
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    writeln!(
        source,
        "                 ::jet_foundation::MIR::MirFunctionId({}),",
        entry_function.0
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    source.push_str(
        "                 crate::__jet_bootstrap_mir_program_from_host,\n\
         crate::__jet_bootstrap_mir_program_to_host,\n\
             )\n\
         }\n",
    );
    source.push_str(
        "\n#[doc(hidden)]\n\
         struct __JetBootstrapCompilerImage {\n\
             header: crate::compiler_bootstrap_compiler_image::CompilerImageHeader,\n\
             source_program: ::std::sync::Arc<crate::__JetBootstrapSourceProgram>,\n\
             program: ::std::sync::Arc<::jet_foundation::MIR::MirProgram>,\n\
         }\n\
         #[doc(hidden)]\n\
         static __JET_BOOTSTRAP_COMPILER_IMAGE: ::std::sync::OnceLock<Result<::std::sync::Arc<__JetBootstrapCompilerImage>, String>> = ::std::sync::OnceLock::new();\n\
         #[doc(hidden)]\n\
         fn __jet_bootstrap_compiler_image() -> Result<::std::sync::Arc<__JetBootstrapCompilerImage>, String> {\n\
             __JET_BOOTSTRAP_COMPILER_IMAGE.get_or_init(|| {\n\
                 let restored = __jet_bootstrap_restore_compiler_image().map_err(|error| error.to_string())?;\n\
                 Ok(::std::sync::Arc::new(__JetBootstrapCompilerImage {\n\
                     header: restored.header,\n\
                     source_program: ::std::sync::Arc::new(restored.source_program),\n\
                     program: ::std::sync::Arc::new(restored.program),\n\
                 }))\n\
             }).clone()\n\
         }\n",
    );
    artifact.compiler_image = Some(image);
    Ok(())
}

/// Revalidate the authorized source lease, install the canonical Prelude
/// adapter, and invoke the generated Host factory. The factory owns the exact
/// emitted Jet carriers; this runner only supplies the pinned snapshot.
pub(crate) fn invoke_bootstrap_entry<Output>(
    lease: &AuthorizedSourceLease,
    entry: impl FnOnce(&AuthorizedSourceSnapshot) -> Output,
) -> Result<Output, AuthorityError> {
    invoke_with_authority(lease, entry)
}

/// Invoke the generated Host factory. Incomplete user compiles preserve their
/// diagnostics; Native artifacts reach the AOT backend, Web artifacts retain
/// their exact target payloads, and user-program Source runtime artifacts use
/// the checked whole-entry JIT seam, separate from the compiler factory entry.
// Runs only inside the generated compiler artifact: it restores the compiler
// image that `append_embedded_compiler_image` embeds at that crate's root.
#[cfg(jet_bootstrap_compiler_artifact)]
pub(crate) fn run_bootstrap_artifact<BackendOutput, SourceProgram, RuntimeConfig>(
    lease: &AuthorizedSourceLease,
    config: &MirRustConfig<'_>,
    factory_tier: BootstrapFactoryTier,
    factory: impl FnOnce(
        &AuthorizedSourceSnapshot,
        BootstrapFactoryTier,
        &crate::Codegen::MIRRust::MirRustExecutionConfig,
        &mut BootstrapRunCompletionOwner,
    ) -> Result<BootstrapJetCompileResult<SourceProgram, RuntimeConfig>, BootstrapHostCodecError>,
    backend: impl FnOnce(BootstrapBackendArtifact<'_>) -> BackendOutput,
    source_resume_factory: Option<
        BootstrapSourceResumeFactory<'_, SourceProgram, RuntimeConfig>,
    >,
    compiler_image_archiver: impl FnOnce(
        &SourceProgram,
        &MirProgram,
        MirArtifactId,
        MirFunctionId,
        &AuthorizedSourceSnapshot,
    ) -> Result<Vec<u8>, BootstrapHostCodecError>,
) -> Result<
    BootstrapRunOutput<BackendOutput, SourceProgram, RuntimeConfig>,
    BootstrapRunError,
> {
    let scope = jet_jit::SourceExecutionCompletionScope::new();
    let mut completion_owner = BootstrapRunCompletionOwner::new(scope.clone());
    let result = scope.with_current(|| {
        run_bootstrap_artifact_inner(
            lease,
            config,
            factory_tier,
            factory,
            backend,
            source_resume_factory,
            compiler_image_archiver,
            &scope,
            &mut completion_owner,
        )
    });
    match result {
        Ok(mut output) => {
            output.completion_owner.allow_resource_retirement = true;
            Ok(output)
        }
        Err(error) => {
            completion_owner.allow_resource_retirement =
                !error.preserves_pending_retirement_error();
            Err(BootstrapRunError::WithCompletionOwner {
                error: Box::new(error),
                completion_owner,
            })
        }
    }
}

#[cfg(jet_bootstrap_compiler_artifact)]
fn run_bootstrap_artifact_inner<BackendOutput, SourceProgram, RuntimeConfig>(
    lease: &AuthorizedSourceLease,
    config: &MirRustConfig<'_>,
    factory_tier: BootstrapFactoryTier,
    factory: impl FnOnce(
        &AuthorizedSourceSnapshot,
        BootstrapFactoryTier,
        &crate::Codegen::MIRRust::MirRustExecutionConfig,
        &mut BootstrapRunCompletionOwner,
    ) -> Result<BootstrapJetCompileResult<SourceProgram, RuntimeConfig>, BootstrapHostCodecError>,
    backend: impl FnOnce(BootstrapBackendArtifact<'_>) -> BackendOutput,
    source_resume_factory: Option<
        BootstrapSourceResumeFactory<'_, SourceProgram, RuntimeConfig>,
    >,
    compiler_image_archiver: impl FnOnce(
        &SourceProgram,
        &MirProgram,
        MirArtifactId,
        MirFunctionId,
        &AuthorizedSourceSnapshot,
    ) -> Result<Vec<u8>, BootstrapHostCodecError>,
    completion_scope: &jet_jit::SourceExecutionCompletionScope,
    completion_owner: &mut BootstrapRunCompletionOwner,
) -> Result<
    BootstrapRunOutput<BackendOutput, SourceProgram, RuntimeConfig>,
    BootstrapRunError,
> {
    let result = invoke_bootstrap_entry(lease, |snapshot| {
        factory(snapshot, factory_tier, &config.execution, completion_owner)
    })??;
    let BootstrapJetCompileResult {
        complete,
        emitted_source,
        bindings,
        source_program,
        runtime_config,
        mir,
        entry_function,
        runtime_artifact,
        web_artifact,
        web_artifacts,
        comptime_stdout,
        comptime_stderr,
        soft_stop: comptime_soft_stop,
        exit_code: comptime_exit_code,
        internal_problem,
        reports,
        resources,
        selected_factory_tier,
        actual_factory_tier,
    } = result;
    if let Some(resources) = resources.as_ref() {
        completion_owner.set_resources(resources.clone());
    }
    let actual_tier_is_valid = match (factory_tier, actual_factory_tier) {
        (BootstrapFactoryTier::Aot, BootstrapFactoryTier::Aot)
        | (
            BootstrapFactoryTier::CraneliftJit,
            BootstrapFactoryTier::CraneliftJit | BootstrapFactoryTier::SourceInterpreterDeopt,
        )
        | (
            BootstrapFactoryTier::SourceInterpreterDeopt,
            BootstrapFactoryTier::SourceInterpreterDeopt,
        ) => true,
        _ => false,
    };
    if selected_factory_tier != factory_tier || !actual_tier_is_valid {
        retire_bootstrap_resources(resources, completion_scope)?;
        return Err(BootstrapRunError::Codec(
            BootstrapHostCodecError::InvalidMetadata(
                "compiler factory selected/actual tier provenance disagrees with the invocation"
                    .to_string(),
            ),
        ));
    }
    if let Some(problem) = internal_problem {
        let error = BootstrapHostCodecError::InvalidMetadata(format!(
            "Jet compiler internal failure: {problem}"
        ));
        return match retire_bootstrap_resources(resources, completion_scope) {
            Ok(()) => Err(BootstrapRunError::CompilerInternal { error, reports }),
            Err(BootstrapRunError::ResourceRetirement(retirement)) => {
                Err(BootstrapRunError::CompilerInternalResourceRetirement {
                    error,
                    reports,
                    retirement,
                })
            }
            Err(error) => Err(error),
        };
    }
    if !complete {
        retire_bootstrap_resources(resources, completion_scope)?;
        return Ok(BootstrapRunOutput {
            selected_factory_tier,
            actual_factory_tier,
            complete: false,
            backend: None,
            web_artifacts: None,
            outcome: None,
            value: None,
            completion_owner: completion_owner.clone(),
            soft_stop: comptime_soft_stop,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            comptime_stdout,
            comptime_stderr,
            comptime_soft_stop,
            comptime_exit_code,
            reports,
            source_program,
            runtime_config: None,
            mir,
            entry_function,
            artifact: runtime_artifact.or(web_artifact),
        });
    }
    if runtime_artifact.is_some() && (web_artifact.is_some() || web_artifacts.is_some()) {
        retire_bootstrap_resources(resources, completion_scope)?;
        return Err(BootstrapRunError::Codec(
            BootstrapHostCodecError::InvalidMetadata(
                "complete compiler result selected both Source runtime and Web artifacts"
                    .to_string(),
            ),
        ));
    }
    if let Some(artifact) = runtime_artifact {
        let Some(program) = mir else {
            retire_bootstrap_resources(resources, completion_scope)?;
            return Err(BootstrapRunError::Codec(
                BootstrapHostCodecError::InvalidMetadata(
                    "complete Source runtime result has no typed MIR".to_string(),
                ),
            ));
        };
        let Some(entry_function) = entry_function else {
            retire_bootstrap_resources(resources, completion_scope)?;
            return Err(BootstrapRunError::Codec(
                BootstrapHostCodecError::InvalidMetadata(
                    "complete Source runtime result has no checked entry function".to_string(),
                ),
            ));
        };
        let Some(source_program) = source_program else {
            retire_bootstrap_resources(resources, completion_scope)?;
            return Err(BootstrapRunError::Codec(
                BootstrapHostCodecError::InvalidMetadata(
                    "complete Source runtime result has no retained Source program".to_string(),
                ),
            ));
        };
        let Some(runtime_config) = runtime_config else {
            retire_bootstrap_resources(resources, completion_scope)?;
            return Err(BootstrapRunError::Codec(
                BootstrapHostCodecError::InvalidMetadata(
                    "complete Source runtime result has no runtime configuration".to_string(),
                ),
            ));
        };
        let Some(resources) = resources else {
            return Err(BootstrapRunError::Codec(
                BootstrapHostCodecError::InvalidMetadata(
                    "complete Source runtime result has no physical resource session".to_string(),
                ),
            ));
        };
        let Some(entry) = program
            .functions
            .iter()
            .find(|function| function.id == entry_function)
        else {
            retire_bootstrap_resources(Some(resources), completion_scope)?;
            return Err(BootstrapRunError::Codec(
                BootstrapHostCodecError::InvalidMetadata(format!(
                    "Source runtime entry function {entry_function:?} is absent from checked MIR"
                )),
            ));
        };
        if !entry.params.is_empty() || !entry.capture_params.is_empty() {
            retire_bootstrap_resources(Some(resources), completion_scope)?;
            return Err(BootstrapRunError::Codec(
                BootstrapHostCodecError::InvalidMetadata(format!(
                    "Source runtime entry `{}` requires parameters or captures, but bootstrap supplies none",
                    entry.name
                )),
            ));
        }
        let root_lease = match resources.retain_root() {
            Ok(root_lease) => root_lease,
            Err(error) => {
                return match resources.retire() {
                    Ok(completions) => {
                        record_source_completions(completion_scope, completions)?;
                        Err(BootstrapRunError::SourceRuntime(format!(
                            "cannot retain Source resource root: {error}"
                        )))
                    }
                    Err(retirement) => Err(
                        BootstrapRunError::SourceRuntimeResourceRetirement {
                            error: format!("cannot retain Source resource root: {error}"),
                            retirement,
                        },
                    ),
                };
            }
        };
        let Some(source_resume_factory) = source_resume_factory else {
            return match resources.retire() {
                Ok(completions) => {
                    record_source_completions(completion_scope, completions)?;
                    Err(BootstrapRunError::SourceRuntime(
                        "Source runtime callback factory is unavailable".to_string(),
                    ))
                }
                Err(retirement) => Err(BootstrapRunError::SourceRuntimeResourceRetirement {
                    error: "Source runtime callback factory is unavailable".to_string(),
                    retirement,
                }),
            };
        };
        let resume = source_resume_factory(source_program, runtime_config, root_lease);
        let execution = match jet_jit::execute_source_entry(
            &program,
            artifact,
            entry_function,
            resources.clone(),
            Vec::new(),
            &config.execution.release_devtools_policy,
            resume,
        ) {
            Ok(execution) => execution,
            Err(error) => {
                return match resources.retire() {
                    Ok(completions) => {
                        record_source_completions(completion_scope, completions)?;
                        Err(BootstrapRunError::SourceExecution(error))
                    }
                    Err(retirement) => Err(
                        BootstrapRunError::SourceExecutionResourceRetirement {
                            execution: error,
                            retirement,
                        },
                    ),
                };
            }
        };
        let mut execution = execution;
        let adopted_value = execution.value.take();
        let retired = execution
            .retire()
            .map_err(BootstrapRunError::SourceExecutionRetirement)?;
        let exit_code = match &retired.outcome {
            Some(RunOutcome::Ran { exit_code, .. }) => Some(i64::from(*exit_code)),
            Some(RunOutcome::Problems(_)) | None => None,
        };
        record_source_completions(completion_scope, retired.completions)?;
        return Ok(BootstrapRunOutput {
            selected_factory_tier,
            actual_factory_tier,
            complete: true,
            backend: None,
            web_artifacts: None,
            outcome: retired.outcome,
            value: retired.value.or(adopted_value),
            completion_owner: completion_owner.clone(),
            stdout: retired.stdout,
            stderr: retired.stderr,
            exit_code,
            comptime_stdout,
            comptime_stderr,
            comptime_soft_stop,
            comptime_exit_code,
            reports,
            source_program: None,
            runtime_config: None,
            mir: Some(program),
            entry_function: Some(entry_function),
            artifact: Some(artifact),
        });
    }
    if web_artifact.is_some() || web_artifacts.is_some() {
        let (Some(artifact), Some(web_artifacts)) = (web_artifact, web_artifacts) else {
            retire_bootstrap_resources(resources, completion_scope)?;
            return Err(BootstrapRunError::Codec(
                BootstrapHostCodecError::InvalidMetadata(
                    "complete Web result must carry both its exact artifact and full artifact set"
                        .to_string(),
                ),
            ));
        };
        let backend_output = backend(BootstrapBackendArtifact::Web {
            artifact,
            artifacts: &web_artifacts,
        });
        retire_bootstrap_resources(resources, completion_scope)?;
        return Ok(BootstrapRunOutput {
            selected_factory_tier,
            actual_factory_tier,
            complete: true,
            backend: Some(backend_output),
            web_artifacts: Some(web_artifacts),
            outcome: None,
            value: None,
            completion_owner: completion_owner.clone(),
            soft_stop: comptime_soft_stop,
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            comptime_stdout,
            comptime_stderr,
            comptime_soft_stop,
            comptime_exit_code,
            reports,
            source_program,
            runtime_config: None,
            mir,
            entry_function,
            artifact: Some(artifact),
        });
    }
    let Some(source) = emitted_source else {
        retire_bootstrap_resources(resources, completion_scope)?;
        return Err(BootstrapRunError::Codec(
            BootstrapHostCodecError::InvalidMetadata(
                "complete native compiler result has no emitted Rust source".to_string(),
            ),
        ));
    };
    let Some(bindings) = bindings else {
        retire_bootstrap_resources(resources, completion_scope)?;
        return Err(BootstrapRunError::Codec(
            BootstrapHostCodecError::InvalidMetadata(
                "complete native compiler result has no Rust binding manifest".to_string(),
            ),
        ));
    };
    let Some(program) = mir.as_ref() else {
        retire_bootstrap_resources(resources, completion_scope)?;
        return Err(BootstrapRunError::Codec(
            BootstrapHostCodecError::InvalidMetadata(
                "complete native compiler result has no checked MIR program".to_string(),
            ),
        ));
    };
    let compiler_image = match crate::__jet_bootstrap_compiler_image() {
        Ok(image) => image,
        Err(error) => {
            retire_bootstrap_resources(resources, completion_scope)?;
            return Err(BootstrapRunError::Codec(
                BootstrapHostCodecError::InvalidMetadata(format!(
                    "cannot restore canonical compiler image before native emission: {error}"
                )),
            ));
        }
    };
    let source_authority_digest =
        crate::compiler_bootstrap_compiler_image::compiler_image_source_authority_digest(
            lease.snapshot(),
        );
    let compiling_canonical_source =
        source_authority_digest == compiler_image.header.source_authority_digest;
    let Some(source_program_ref) = source_program.as_ref() else {
        retire_bootstrap_resources(resources, completion_scope)?;
        return Err(BootstrapRunError::Codec(
            BootstrapHostCodecError::InvalidMetadata(
                "complete native compiler result has no typed Source MIR".to_string(),
            ),
        ));
    };
    // Only a compiler output is packaged with the Host/Runner splice, whose
    // readers restore the embedded compiler image. A user program has no reader
    // of either and carries neither.
    let compiler_output = bindings
        .callables
        .iter()
        .any(|callable| callable.source_name == "jet_bootstrap_compile");
    let (native_artifact, embedded_image) =
        if compiling_canonical_source {
            let compiler_root = match checked_compiler_factory_root(program, &bindings) {
                Ok(root) => root,
                Err(error) => {
                    retire_bootstrap_resources(resources, completion_scope)?;
                    return Err(BootstrapRunError::Codec(error));
                }
            };
            if compiler_image.header.entry_function != compiler_root {
                retire_bootstrap_resources(resources, completion_scope)?;
                return Err(BootstrapRunError::Codec(
                    BootstrapHostCodecError::InvalidMetadata(
                        "canonical compiler image private root differs from the checked source factory"
                            .to_string(),
                    ),
                ));
            }
            let Some(native_artifact) =
                artifact_for_function_target(program, compiler_root, MirArtifactTarget::RustAot)
            else {
                retire_bootstrap_resources(resources, completion_scope)?;
                return Err(BootstrapRunError::Codec(
                    BootstrapHostCodecError::InvalidMetadata(
                        "canonical compiler source has no exact Rust AOT artifact containing its private factory root"
                            .to_string(),
                    ),
                ));
            };
            let image_bytes = match compiler_image_archiver(
                source_program_ref,
                program,
                native_artifact,
                compiler_root,
                lease.snapshot(),
            ) {
                Ok(image) => image,
                Err(error) => {
                    retire_bootstrap_resources(resources, completion_scope)?;
                    return Err(BootstrapRunError::Codec(error));
                }
            };
            (
                native_artifact,
                Some((image_bytes, source_authority_digest, native_artifact, compiler_root)),
            )
        } else {
            let native_artifact = match entry_function {
                Some(entry_function) => {
                    entry_artifact_for_target(program, entry_function, MirArtifactTarget::RustAot)
                }
                None => single_artifact_for_target(program, MirArtifactTarget::RustAot),
            };
            let Some(native_artifact) = native_artifact else {
                retire_bootstrap_resources(resources, completion_scope)?;
                return Err(BootstrapRunError::Codec(
                    BootstrapHostCodecError::InvalidMetadata(
                        "complete native result has no unique exact Rust AOT output artifact"
                            .to_string(),
                    ),
                ));
            };
            let embedded_image = compiler_output.then(|| {
                (
                    crate::__JET_BOOTSTRAP_COMPILER_IMAGE_BYTES.to_vec(),
                    compiler_image.header.source_authority_digest,
                    compiler_image.header.artifact,
                    compiler_image.header.entry_function,
                )
            });
            (native_artifact, embedded_image)
        };
    let mut output_config = (*config).clone();
    output_config.execution.artifact = native_artifact;
    // The Jet emitter returns the user-item suffix only; the runtime/Core prefix
    // is the same build-fact text MIRRust emits, so `jet_store::runtime::prepare`
    // links both compilers' artifacts against one cached `jet_runtime` rlib.
    let mut assembled = crate::Codegen::MIRRust::emit_mir_runtime_text(program, &output_config);
    assembled.push_str(&source);
    let artifact = match embedded_image {
        Some((image_bytes, image_authority_digest, image_artifact, image_root)) => {
            let mut artifact =
                match prepare_bootstrap_artifact(assembled, program, &output_config, bindings) {
                    Ok(artifact) => artifact,
                    Err(error) => {
                        retire_bootstrap_resources(resources, completion_scope)?;
                        return Err(BootstrapRunError::Codec(error));
                    }
                };
            if let Err(error) = append_embedded_compiler_image(
                &mut artifact,
                image_bytes,
                image_authority_digest,
                image_artifact,
                image_root,
            ) {
                retire_bootstrap_resources(resources, completion_scope)?;
                return Err(BootstrapRunError::Codec(error));
            }
            artifact
        }
        None => BootstrapArtifact {
            source: assembled,
            compiler_image: None,
            bindings,
        },
    };
    let backend_output = backend(BootstrapBackendArtifact::NativeRust {
        source: artifact.source,
        compiler_image: artifact.compiler_image,
        bindings: artifact.bindings,
    });
    retire_bootstrap_resources(resources, completion_scope)?;
    Ok(BootstrapRunOutput {
        selected_factory_tier,
        actual_factory_tier,
        complete: true,
        backend: Some(backend_output),
        web_artifacts: None,
        outcome: None,
        value: None,
        completion_owner: completion_owner.clone(),
        soft_stop: comptime_soft_stop,
        stdout: String::new(),
        stderr: String::new(),
        exit_code: None,
        comptime_stdout,
        comptime_stderr,
        comptime_soft_stop,
        comptime_exit_code,
        reports,
        source_program,
        runtime_config: None,
        mir,
        entry_function,
        artifact: Some(native_artifact),
    })
}

fn record_source_completions(
    scope: &jet_jit::SourceExecutionCompletionScope,
    completions: Vec<jet_jit::SourceExecutionCompletion>,
) -> Result<(), BootstrapRunError> {
    for completion in completions {
        scope.record(completion);
    }
    Ok(())
}

fn retire_bootstrap_resources(
    resources: Option<SourceResourceSession>,
    completion_scope: &jet_jit::SourceExecutionCompletionScope,
) -> Result<(), BootstrapRunError> {
    if let Some(resources) = resources {
        let completions = resources
            .retire()
            .map_err(BootstrapRunError::ResourceRetirement)?;
        record_source_completions(completion_scope, completions)?;
    }
    Ok(())
}

fn entry_artifact_for_target(
    program: &MirProgram,
    entry_function: MirFunctionId,
    target: MirArtifactTarget,
) -> Option<MirArtifactId> {
    let mut matches = program.artifacts.iter().filter(|artifact| {
        artifact.target == target
            && artifact
                .entry
                .as_ref()
                .and_then(|entry| entry.function)
                == Some(entry_function)
    });
    let artifact = matches.next()?.id;
    matches.next().is_none().then_some(artifact)
}
fn checked_compiler_factory_root(
    program: &MirProgram,
    bindings: &BootstrapBindingDescriptor,
) -> Result<MirFunctionId, BootstrapHostCodecError> {
    let mut entries = bindings
        .callables
        .iter()
        .filter(|callable| callable.source_name == "jet_bootstrap_compile");
    let entry = entries.next().ok_or_else(|| {
        BootstrapHostCodecError::MissingEntry("jet_bootstrap_compile".to_string())
    })?;
    if entries.next().is_some() {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "compiler MIR has multiple private jet_bootstrap_compile roots".to_string(),
        ));
    }
    let function = program
        .functions
        .iter()
        .find(|function| function.id == entry.metadata.function)
        .ok_or_else(|| {
            BootstrapHostCodecError::InvalidMetadata(
                "private jet_bootstrap_compile root is absent from checked MIR".to_string(),
            )
        })?;
    if !function.capture_params.is_empty() || !function.target_applicability.rust_aot {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "private jet_bootstrap_compile root must be capture-free and Rust AOT-applicable"
                .to_string(),
        ));
    }
    BootstrapEntryCodec::new(program, bindings, function.id)?;
    Ok(function.id)
}

fn artifact_for_function_target(
    program: &MirProgram,
    function: MirFunctionId,
    target: MirArtifactTarget,
) -> Option<MirArtifactId> {
    let module = program
        .functions
        .iter()
        .find(|row| row.id == function)?
        .module_id;
    let mut matches = program
        .artifacts
        .iter()
        .filter(|artifact| artifact.target == target && artifact.modules.contains(&module));
    let artifact = matches.next()?.id;
    matches.next().is_none().then_some(artifact)
}

fn single_artifact_for_target(
    program: &MirProgram,
    target: MirArtifactTarget,
) -> Option<MirArtifactId> {
    let mut matches = program
        .artifacts
        .iter()
        .filter(|artifact| artifact.target == target);
    let artifact = matches.next()?.id;
    matches.next().is_none().then_some(artifact)
}

/// Package the canonical Host and Runner modules into the private emitted
/// crate. The backend keeps the generated prefix untouched, including its
/// library/entry attributes; this suffix only links the real source modules
/// and aliases the workspace crates they already use.
fn rust_string_literal(path: &Path) -> String {
    format!("{:?}", path.to_string_lossy().as_ref())
}

fn package_bootstrap_artifact(
    mut source: String,
) -> Result<String, BootstrapHostCodecError> {
    let source_root = Path::new(crate::BOOTSTRAP_CANONICAL_SOURCE_ROOT);
    let host_path = source_root.join("Compiler/Bootstrap/Host/Native.rs");
    let native_adapter_path = source_root.join("Compiler/Bootstrap/Host/NativeAdapter.rs");
    let runtime_mir_codec_path = source_root.join("Compiler/Bootstrap/Host/RuntimeMirCodec.rs");
    let diagnostic_codec_path = source_root.join("Compiler/Bootstrap/Host/DiagnosticCodec.rs");
    let runtime_mir_path = source_root.join("Compiler/Bootstrap/Host/RuntimeMir.rs");
    let runner_path = source_root.join("Compiler/Bootstrap/Runner.rs");
    let compiler_image_path = source_root.join("Compiler/Bootstrap/Host/CompilerImage.rs");
    let entry_codec_path = source_root.join("Compiler/Bootstrap/Host/EntryCodec.rs");
    if !host_path.is_file()
        || !native_adapter_path.is_file()
        || !runner_path.is_file()
        || !runtime_mir_codec_path.is_file()
        || !diagnostic_codec_path.is_file()
        || !runtime_mir_path.is_file()
        || !compiler_image_path.is_file()
        || !entry_codec_path.is_file()
    {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "canonical bootstrap Host/NativeAdapter/Runner/RuntimeMir/RuntimeMirCodec/EntryCodec/DiagnosticCodec source paths are unavailable".to_string(),
        ));
    }
    source.push_str("\n\n#[doc(hidden)]\n");
    source.push_str("pub(crate) const BOOTSTRAP_CANONICAL_SOURCE_ROOT: &str = ");
    source.push_str(&rust_string_literal(source_root));
    source.push_str(
        ";\n\
         \n\
         // Private bootstrap dependency aliases. The backend supplies these\n\
         // workspace crates for the compiler artifact. `jet_foundation` keeps\n\
         // its own name: the generated runtime declares a `mod jet_foundation`\n\
         // facade at this root, and `::jet_foundation` names the crate.\n\
         extern crate jet as __jet_compiler;\n\
         extern crate jet_driver as __jet_driver;\n\
         use __jet_driver as jet_driver;\n\
         pub(crate) use __jet_compiler::{Authority, BootstrapBuildIdentity, Codegen, Comptime};\n\
         \n",
    );
    source.push_str("#[path = ");
    source.push_str(&rust_string_literal(&runtime_mir_codec_path));
    source.push_str("]\nmod compiler_bootstrap_runtime_mir_codec;\n\n");
    source.push_str("#[path = ");
    source.push_str(&rust_string_literal(&runtime_mir_path));
    source.push_str("]\nmod compiler_bootstrap_runtime_mir;\n\n");
    source.push_str("#[path = ");
    source.push_str(&rust_string_literal(&diagnostic_codec_path));
    source.push_str("]\nmod compiler_bootstrap_diagnostic_codec;\n\n");
    source.push_str("#[path = ");
    source.push_str(&rust_string_literal(&entry_codec_path));
    source.push_str("]\nmod compiler_bootstrap_entry_codec;\n\n");
    source.push_str("pub(crate) use compiler_bootstrap_entry_codec::{BootstrapEntryCodec, BootstrapEntryPhysicalBindings, BootstrapEntryValue};\n\n");
    source.push_str("#[path = ");
    source.push_str(&rust_string_literal(&compiler_image_path));
    source.push_str("]\nmod compiler_bootstrap_compiler_image;\n");
    source.push_str("pub(crate) use compiler_bootstrap_compiler_image::{\n\
        archive_compiler_image, restore_compiler_image, CompilerImageError,\n\
        CompilerImageHeader, RestoredCompilerImage,\n\
    };\n\n");
    source.push_str("pub(crate) use compiler_bootstrap_runtime_mir::{execute_source_mir, SourceMirExecution, SourceMirExecutionError};\n\n");
    source.push_str("#[path = ");
    source.push_str(&rust_string_literal(&host_path));
    source.push_str(
        "]\n\
         mod compiler_bootstrap_host;\n\
         pub(crate) use compiler_bootstrap_host::{\n\
             append_bootstrap_host_glue, AuthorizedSourceLease,\n\
             AuthorizedSourceSnapshot, BootstrapBindingDescriptor,\n\
             BootstrapHostCodecError,\n\
         };\n\
         \n\
         #[path = ",
    );
    source.push_str(&rust_string_literal(&runner_path));
    source.push_str(
        "]\n\
         mod compiler_bootstrap_runner;\n\
         pub(crate) use compiler_bootstrap_runner::{\n\
             invoke_bootstrap_entry, prepare_bootstrap_artifact,\n\
             bootstrap_artifact_build_id, prepare_bootstrap_artifact_from_aot, run_bootstrap_artifact,\n\
             BootstrapArtifact, BootstrapRunError,\n\
             BootstrapJetCompileResult, BootstrapRunOutput,\n\
             BootstrapFactoryTier,\n\
             BootstrapBackendArtifact, BootstrapSourceResumeFactory, BootstrapSourceResume,\n\
             BootstrapWebArtifacts,\n\
         };\n\
         \n\
         #[doc(hidden)]\n\
         pub(crate) fn __jet_bootstrap_run_from_host<BackendOutput, RuntimeConfig>(
             lease: &crate::compiler_bootstrap_host::AuthorizedSourceLease,
             config: &crate::Codegen::MIRRust::MirRustConfig<'_>,
             factory_tier: crate::BootstrapFactoryTier,
             factory: impl FnOnce(\n\
                 &crate::compiler_bootstrap_host::AuthorizedSourceSnapshot,\n\
                 crate::BootstrapFactoryTier,\n\
                 &crate::Codegen::MIRRust::MirRustExecutionConfig,\n\
                 &mut crate::compiler_bootstrap_runner::BootstrapRunCompletionOwner,\n\
             ) -> Result<crate::BootstrapJetCompileResult<crate::__JetBootstrapSourceProgram, RuntimeConfig>, crate::BootstrapHostCodecError>,\n\
             backend: impl FnOnce(crate::compiler_bootstrap_runner::BootstrapBackendArtifact<'_>) -> BackendOutput,
             source_resume_factory: Option<crate::compiler_bootstrap_runner::BootstrapSourceResumeFactory<'_, crate::__JetBootstrapSourceProgram, RuntimeConfig>>,
         ) -> Result<
             crate::compiler_bootstrap_runner::BootstrapRunOutput<BackendOutput, crate::__JetBootstrapSourceProgram, RuntimeConfig>,
             crate::compiler_bootstrap_runner::BootstrapRunError,
         >
         {
             crate::compiler_bootstrap_runner::run_bootstrap_artifact(
                 lease,
                 config,
                 factory_tier,
                 factory,
                 backend,
                 source_resume_factory,\n\
                 |source_program, program, artifact, entry, snapshot| {\n\
                     let compiler_image = crate::__jet_bootstrap_compiler_image()\n\
                         .map_err(crate::BootstrapHostCodecError::InvalidMetadata)?;\n\
                     let source_authority_digest =\n\
                         crate::compiler_bootstrap_compiler_image::compiler_image_source_authority_digest(snapshot);\n\
                     if source_authority_digest != compiler_image.header.source_authority_digest\n\
                         || entry != compiler_image.header.entry_function {\n\
                         return Err(crate::BootstrapHostCodecError::InvalidMetadata(\n\
                             \"private compiler-image archive root differs from canonical source authority\".to_string(),\n\
                         ));\n\
                     }\n\
                     crate::compiler_bootstrap_compiler_image::archive_compiler_image(\n\
                         source_program,\n\
                         program,\n\
                         artifact,\n\
                         entry,\n\
                         snapshot,\n\
                         crate::__jet_bootstrap_mir_program_to_host,\n\
                     )\n\
                     .map_err(|error| crate::BootstrapHostCodecError::InvalidMetadata(format!(\"cannot archive checked compiler MIR image: {error}\")))\n\
                 },\n\
             )\n\
         }\n",
    );
    Ok(source)
}
/// Compute the compiler identity for this exact emitted artifact with the
/// canonical build-fact/source framing. The private backend supplies the facts
/// from its actual rustc invocation, then exports the result as
/// `JET_COMPILER_BUILD_ID` while compiling this artifact; absent facts remain a
/// backend-level unavailable state rather than a package-version fallback.
pub(crate) fn bootstrap_artifact_build_id(
    source: &str,
    facts: &[(String, String)],
) -> Result<String, BootstrapHostCodecError> {
    let root = Path::new(crate::BOOTSTRAP_CANONICAL_SOURCE_ROOT);
    let extra = vec![(
        "Compiler/Bootstrap/__generated_compiler.rs".to_string(),
        source.as_bytes().to_vec(),
    )];
    crate::BootstrapBuildIdentity::semantic_id_with_extra(
        root,
        crate::BootstrapBuildIdentity::COMPILER_DOMAIN,
        crate::BootstrapBuildIdentity::COMPILER_SOURCES,
        facts,
        &extra,
    )
    .map_err(|error| {
        BootstrapHostCodecError::InvalidMetadata(format!(
            "cannot compute private compiler artifact identity: {error}"
        ))
    })
}
