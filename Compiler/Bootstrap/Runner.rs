// Private orchestration for the source-coupled compiler artifact.
//
// The generated Jet factory returns one checked target result. Native artifacts
// receive the canonical Host splice, Web artifacts retain their exact payloads,
// and user-program Source Runtime artifacts enter the checked whole-entry JIT.
// This is separate from executing the compiler factory itself; the runner never
// parses Jet source, re-runs sema, or infers route policy.

use crate::Codegen::MIRRust::{MirRustAotMetadata, MirRustConfig};
use crate::{append_bootstrap_host_glue, BootstrapBindingDescriptor, BootstrapHostCodecError};
use crate::compiler_bootstrap_host::{
    invoke_with_authority, AuthorizedSourceLease, AuthorizedSourceSnapshot,
};
use crate::Authority::AuthorityError;
use jet_foundation::JitBackend::RunOutcome;
use jet_foundation::MIR::{
    MirArtifactId, MirArtifactTarget, MirFunctionId, MirProgram, MirRuntimeValue,
};
use jet_jit::SourceResources::{SourceResourceLease, SourceResourceSession};
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
#[derive(Debug)]
pub(crate) struct BootstrapArtifact {
    pub(crate) source: String,
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
    pub(crate) onnx_runtime_js: String,
    pub(crate) onnx_runtime_worker_js: String,
    pub(crate) index_html: String,
    pub(crate) explicit_html_path: Option<String>,
    pub(crate) command_record: Vec<u8>,
}
#[derive(Debug)]
pub(crate) enum BootstrapBackendArtifact<'a> {
    NativeRust {
        source: String,
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
    Box<dyn Fn(jet_jit::SourceDeoptRequest) -> Result<jet_jit::SourceDeoptReply, String> + 'static>;
pub(crate) type BootstrapSourceResumeFactory<'a, SourceProgram, RuntimeConfig> =
    Box<dyn FnOnce(SourceProgram, RuntimeConfig, SourceResourceLease) -> BootstrapSourceResume + 'a>;

pub(crate) struct BootstrapRunOutput<BackendOutput, SourceProgram = (), RuntimeConfig = ()> {
    pub(crate) complete: bool,
    pub(crate) backend: Option<BackendOutput>,
    pub(crate) web_artifacts: Option<BootstrapWebArtifacts>,
    pub(crate) outcome: Option<RunOutcome>,
    pub(crate) value: Option<MirRuntimeValue>,
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
    SourceRuntime(String),
    ResourceRetirement(String),

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
    let compiler_artifact = config.execution.artifact;
    let artifact_plan = program
        .artifacts
        .iter()
        .find(|artifact| artifact.id == compiler_artifact)
        .ok_or_else(|| {
            BootstrapHostCodecError::InvalidMetadata(format!(
                "stage-zero compiler artifact {compiler_artifact:?} is absent from MIR"
            ))
        })?;
    if artifact_plan.target != MirArtifactTarget::RustAot
        || artifact_plan
            .entry
            .as_ref()
            .and_then(|entry| entry.function)
            != Some(compiler_entry_function)
    {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "stage-zero compiler artifact is not the Rust AOT jet_bootstrap_compile entry"
                .to_string(),
        ));
    }
    let image_bytes = crate::compiler_bootstrap_compiler_image::archive_compiler_image(
        program,
        program,
        compiler_artifact,
        compiler_entry_function,
        source_authority,
        |program| Ok(program.clone()),
        jet_foundation::MIR::mir_program_image_bytes,
    )
    .map_err(|error| {
        BootstrapHostCodecError::InvalidMetadata(format!(
            "cannot archive stage-zero compiler MIR: {error}"
        ))
    })?;
    let source_authority_digest =
        crate::compiler_bootstrap_compiler_image::compiler_image_source_authority_digest(
            source_authority,
        );
    let mut artifact = prepare_bootstrap_artifact(source, program, config, bindings)?;
    append_embedded_compiler_image(
        &mut artifact.source,
        &image_bytes,
        source_authority_digest,
        compiler_artifact,
        compiler_entry_function,
    )?;
    Ok(artifact)
}
fn append_embedded_compiler_image(
    source: &mut String,
    image: &[u8],
    source_authority_digest: [u8; 32],
    artifact: MirArtifactId,
    entry_function: MirFunctionId,
) -> Result<(), BootstrapHostCodecError> {
    source.push_str(
        "\n#[doc(hidden)]\n\
         const __JET_BOOTSTRAP_COMPILER_IMAGE_BYTES: &[u8] = &[",
    );
    for (index, byte) in image.iter().enumerate() {
        if index != 0 {
            source.push(',');
        }
        write!(source, "{byte}")
            .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    }
    source.push_str(
        "];\n\
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
             crate::compiler_bootstrap_compiler_image::RestoredCompilerImage<::jet_foundation::MIR::MirProgram>,\n\
             crate::compiler_bootstrap_compiler_image::CompilerImageError,\n\
         > {\n\
             crate::compiler_bootstrap_compiler_image::restore_compiler_image_for_digest(\n\
                 __JET_BOOTSTRAP_COMPILER_IMAGE_BYTES,\n\
                 __JET_BOOTSTRAP_COMPILER_SOURCE_AUTHORITY_DIGEST,\n",
    );
    writeln!(
        source,
        "                 ::jet_foundation::MIR::MirArtifactId({}),",
        artifact.0
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    writeln!(
        source,
        "                 ::jet_foundation::MIR::MirFunctionId({}),",
        entry_function.0
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    source.push_str(
        "                 ::jet_foundation::MIR::mir_program_from_image_bytes,\n\
         |program| Ok(program.clone()),\n\
             )\n\
         }\n",
    );
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
pub(crate) fn run_bootstrap_artifact<BackendOutput, SourceProgram, RuntimeConfig>(
    lease: &AuthorizedSourceLease,
    config: &MirRustConfig<'_>,
    factory: impl FnOnce(
        &AuthorizedSourceSnapshot,
    ) -> Result<BootstrapJetCompileResult<SourceProgram, RuntimeConfig>, BootstrapHostCodecError>,
    backend: impl FnOnce(BootstrapBackendArtifact<'_>) -> BackendOutput,
    source_resume_factory: Option<
        BootstrapSourceResumeFactory<'_, SourceProgram, RuntimeConfig>,
    >,
) -> Result<
    BootstrapRunOutput<BackendOutput, SourceProgram, RuntimeConfig>,
    BootstrapRunError,
> {
    let result = invoke_bootstrap_entry(lease, factory)??;
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
    } = result;
    if let Some(problem) = internal_problem {
        if let Err(error) = retire_bootstrap_resources(resources) {
            return Err(BootstrapRunError::CompilerInternal {
                error: BootstrapHostCodecError::InvalidMetadata(format!(
                    "Jet compiler internal failure: {problem}; Source resource retirement also failed: {error}"
                )),
                reports,
            });
        }
        return Err(BootstrapRunError::CompilerInternal {
            error: BootstrapHostCodecError::InvalidMetadata(format!(
                "Jet compiler internal failure: {problem}"
            )),
            reports,
        });
    }
    if !complete {
        retire_bootstrap_resources(resources)?;
        return Ok(BootstrapRunOutput {
            complete: false,
            backend: None,
            web_artifacts: None,
            outcome: None,
            value: None,
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
            runtime_config,
            mir,
            entry_function,
            artifact: runtime_artifact.or(web_artifact),
        });
    }
    if runtime_artifact.is_some() && (web_artifact.is_some() || web_artifacts.is_some()) {
        retire_bootstrap_resources(resources)?;
        return Err(BootstrapRunError::Codec(
            BootstrapHostCodecError::InvalidMetadata(
                "complete compiler result selected both Source runtime and Web artifacts"
                    .to_string(),
            ),
        ));
    }
    if let Some(artifact) = runtime_artifact {
        let Some(program) = mir else {
            retire_bootstrap_resources(resources)?;
            return Err(BootstrapRunError::Codec(
                BootstrapHostCodecError::InvalidMetadata(
                    "complete Source runtime result has no typed MIR".to_string(),
                ),
            ));
        };
        let Some(entry_function) = entry_function else {
            retire_bootstrap_resources(resources)?;
            return Err(BootstrapRunError::Codec(
                BootstrapHostCodecError::InvalidMetadata(
                    "complete Source runtime result has no checked entry function".to_string(),
                ),
            ));
        };
        let Some(source_program) = source_program else {
            retire_bootstrap_resources(resources)?;
            return Err(BootstrapRunError::Codec(
                BootstrapHostCodecError::InvalidMetadata(
                    "complete Source runtime result has no retained Source program".to_string(),
                ),
            ));
        };
        let Some(runtime_config) = runtime_config else {
            retire_bootstrap_resources(resources)?;
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
            retire_bootstrap_resources(Some(resources))?;
            return Err(BootstrapRunError::Codec(
                BootstrapHostCodecError::InvalidMetadata(format!(
                    "Source runtime entry function {entry_function:?} is absent from checked MIR"
                )),
            ));
        };
        if !entry.params.is_empty() || !entry.capture_params.is_empty() {
            retire_bootstrap_resources(Some(resources))?;
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
                if let Err(retirement) = resources.retire() {
                    return Err(BootstrapRunError::SourceRuntime(format!(
                        "cannot retain Source resource root: {error}; physical retirement also failed: {retirement}"
                    )));
                }
                return Err(BootstrapRunError::SourceRuntime(format!(
                    "cannot retain Source resource root: {error}"
                )));
            }
        };
        let Some(source_resume_factory) = source_resume_factory else {
            if let Err(retirement) = resources.retire() {
                return Err(BootstrapRunError::SourceRuntime(format!(
                    "Source runtime callback factory is unavailable; physical resource retirement also failed: {retirement}"
                )));
            }
            return Err(BootstrapRunError::SourceRuntime(
                "Source runtime callback factory is unavailable".to_string(),
            ));
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
                if let Err(retire_error) = resources.retire() {
                    return Err(BootstrapRunError::SourceRuntime(format!(
                        "{error}; physical Source resource retirement also failed: {retire_error}"
                    )));
                }
                return Err(BootstrapRunError::SourceRuntime(error.to_string()));
            }
        };
        let mut execution = execution;
        let adopted_value = execution.value.take();
        let retired = execution
            .retire()
            .map_err(|error| BootstrapRunError::SourceRuntime(error.to_string()))?;
        let exit_code = match &retired.outcome {
            Some(RunOutcome::Ran { exit_code, .. }) => Some(i64::from(*exit_code)),
            Some(RunOutcome::Problems(_)) | None => None,
        };
        return Ok(BootstrapRunOutput {
            complete: true,
            backend: None,
            web_artifacts: None,
            outcome: retired.outcome,
            value: retired.value.or(adopted_value),
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
            retire_bootstrap_resources(resources)?;
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
        retire_bootstrap_resources(resources)?;
        return Ok(BootstrapRunOutput {
            complete: true,
            backend: Some(backend_output),
            web_artifacts: Some(web_artifacts),
            outcome: None,
            value: None,
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
            runtime_config,
            mir,
            entry_function,
            artifact: Some(artifact),
        });
    }
    let Some(source) = emitted_source else {
        retire_bootstrap_resources(resources)?;
        return Err(BootstrapRunError::Codec(
            BootstrapHostCodecError::InvalidMetadata(
                "complete native compiler result has no emitted Rust source".to_string(),
            ),
        ));
    };
    let Some(bindings) = bindings else {
        retire_bootstrap_resources(resources)?;
        return Err(BootstrapRunError::Codec(
            BootstrapHostCodecError::InvalidMetadata(
                "complete native compiler result has no Rust binding manifest".to_string(),
            ),
        ));
    };
    let Some(program) = mir.as_ref() else {
        retire_bootstrap_resources(resources)?;
        return Err(BootstrapRunError::Codec(
            BootstrapHostCodecError::InvalidMetadata(
                "complete native compiler result has no checked MIR program".to_string(),
            ),
        ));
    };
    let native_artifact = mir
        .as_ref()
        .zip(entry_function)
        .and_then(|(program, entry)| {
            entry_artifact_for_target(program, entry, MirArtifactTarget::RustAot)
        });
    let artifact = match prepare_bootstrap_artifact(source, program, config, bindings) {
        Ok(artifact) => artifact,
        Err(error) => {
            retire_bootstrap_resources(resources)?;
            return Err(BootstrapRunError::Codec(error));
        }
    };
    let backend_output = backend(BootstrapBackendArtifact::NativeRust {
        source: artifact.source,
        bindings: artifact.bindings,
    });
    retire_bootstrap_resources(resources)?;
    Ok(BootstrapRunOutput {
        complete: true,
        backend: Some(backend_output),
        web_artifacts: None,
        outcome: None,
        value: None,
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
        runtime_config,
        mir,
        entry_function,
        artifact: native_artifact,
    })
}

fn retire_bootstrap_resources(
    resources: Option<SourceResourceSession>,
) -> Result<(), BootstrapRunError> {
    if let Some(resources) = resources {
        resources
            .retire()
            .map_err(BootstrapRunError::ResourceRetirement)?;
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
         // workspace crates for the compiler artifact.\n\
         extern crate jet as __jet_compiler;\n\
         extern crate jet_driver as __jet_driver;\n\
         extern crate jet_foundation as __jet_foundation;\n\
         use __jet_driver as jet_driver;\n\
         use __jet_foundation as jet_foundation;\n\
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
         pub(crate) fn __jet_bootstrap_run_from_host<BackendOutput, SourceProgram, RuntimeConfig>(\n\
             lease: &crate::compiler_bootstrap_host::AuthorizedSourceLease,\n\
             config: &crate::Codegen::MIRRust::MirRustConfig<'_>,\n\
             factory: impl FnOnce(&crate::compiler_bootstrap_host::AuthorizedSourceSnapshot) -> Result<crate::BootstrapJetCompileResult<SourceProgram, RuntimeConfig>, crate::BootstrapHostCodecError>,\n\
             backend: impl FnOnce(crate::compiler_bootstrap_runner::BootstrapBackendArtifact<'_>) -> BackendOutput,\n\
             source_resume_factory: Option<crate::compiler_bootstrap_runner::BootstrapSourceResumeFactory<'_, SourceProgram, RuntimeConfig>>,\n\
         ) -> Result<\n\
             crate::compiler_bootstrap_runner::BootstrapRunOutput<BackendOutput, SourceProgram, RuntimeConfig>,\n\
             crate::compiler_bootstrap_runner::BootstrapRunError,\n\
         >\n\
         {\n\
             crate::compiler_bootstrap_runner::run_bootstrap_artifact(\n\
                 lease,\n\
                 config,\n\
                 factory,\n\
                 backend,\n\
                 source_resume_factory,\n\
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
