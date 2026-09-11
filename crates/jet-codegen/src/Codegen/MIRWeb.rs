//! MIR-only WebAssembly/JavaScript emission.
//!
//! This adapter consumes the optimized semantic MIR and checked Web facts
//! carried by that model. It never accepts a bundle, reads AST/TIR/checker
//! state, or reconstructs route/partition/Core policy.

use jet_foundation::SHA256::sha256_hex;
use jet_foundation::Layout::TargetLayout;
use jet_foundation::MIR::{
    MirAllocatorKind, MirAccess, MirArtifactKind, MirArtifactPlan, MirArtifactTarget, MirBasicBlock, MirBinaryDispatch,
    MirBinaryOp, MirBinaryPatternPart, MirCallee, MirCaptureOperand, MirConstant, MirConstKey,
    MirConstReport, MirCoreClosureKind, MirEntrySpec, MirFunction, MirFunctionForm, MirFunctionKind, MirGcEditKind,
    MirConversion, MirJob, MirLayoutCompareOp, MirOperation, MirPlaceBase, MirPreludeAbi, MirPreludeCallId,
    MirPreludeFamily, MirProgram, MirProjection, MirRequireKind, MirSemanticOp, MirSelectKind,
    MirStringPart, MirSymbol, MirTaskGroupKind, MirTerminator, MirTextHoleKind, MirTextPatternPart,
    MirTraitMethodId, MirTraitRef, MirType, MirTypeDef, MirTypeDefKind, MirTypeKind, MirUnaryOp,
    MirVariantPayload, MirVisibility, MirForeignAbi, MirSerdeCodec,
    MirHttpMethod, MirOwnershipMode,
};
use jet_foundation::TestingHistory::{HistoryProvenance, HISTORY_ENGINE};
use jet_foundation::WebPartition::{WebBucket, WebPartitionMarker};
use jet_foundation::Names::mangle_path;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use super::MIRRust::{
    emit_mir_program, MirRustConfig, MirRustExecutionConfig, MirRustTarget,
};
use super::Web::{dom_runtime_source, shared_js_prelude};

fn foreign_abi_name(abi: &MirForeignAbi) -> String {
    match abi {
        MirForeignAbi::C => "C".to_string(),
        MirForeignAbi::CUnwind => "C-unwind".to_string(),
        MirForeignAbi::System => "system".to_string(),
        MirForeignAbi::Stdcall => "stdcall".to_string(),
        MirForeignAbi::Fastcall => "fastcall".to_string(),
        MirForeignAbi::Vectorcall => "vectorcall".to_string(),
        MirForeignAbi::Rust => "Rust".to_string(),
        MirForeignAbi::Platform(name) => name.clone(),
    }
}

/// Files emitted by the Web backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebArtifacts {
    pub manifest_json: String,
    pub wasm_rust: String,
    pub js_app: String,
    pub js_source_map: String,
    pub source_names: Vec<String>,
    pub source_contents: Vec<String>,
    pub dom_runtime: String,
    /// Browser model transport module.  It is emitted only when a checked
    /// model output reaches this Web artifact.
    pub onnx_runtime_js: String,
    /// Worker module loaded by the browser model transport module.
    pub onnx_runtime_worker_js: String,
    pub index_html: String,
    pub explicit_html_path: Option<String>,
    pub command_record: Vec<u8>,
}

/// Explicit publication assets supplied to Web emission.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MirWebAssets {
    pub dom_runtime: String,
    pub index_html: String,
    pub explicit_html_path: Option<String>,
    pub source_names: Vec<String>,
    pub source_contents: Vec<String>,
}

impl MirWebAssets {
    /// Build the standard browser shell without reading a bundle or source
    /// files. Callers may replace any asset before emission.
    pub fn with_default_shell() -> Self {
        Self {
            dom_runtime: dom_runtime_source().to_string(),
            index_html: default_index_html(),
            explicit_html_path: None,
            source_names: Vec::new(),
            source_contents: Vec::new(),
        }
    }
}

/// Target facts needed by Web representation.
///
/// Applicability and partition are checked facts on MIR functions. This target
/// also carries the one folded release-devtools policy used by the canonical
/// MIR Rust printer; Web emission never reconstructs it from source or host
/// environment.
pub struct MirWebTarget {
    pub layout: TargetLayout,
    pub assets: MirWebAssets,
    pub semantic_digest: [u8; 32],
    /// The checked artifact plan selected by the caller. Web emission never
    /// guesses an application from the program's artifact table.
    pub artifact: jet_foundation::MIR::MirArtifactId,
    /// D-DX-PROD1: the same typed policy used by native MIR Rust emission.
    pub release_devtools_policy: jet_pkg_model::Package::ReleaseDevtoolsPolicy,
}


/// Errors reserved for malformed/inconsistent MIR or publication assembly.
/// Web emission never re-decides target applicability; leaked target-specific
/// facts are malformed MIR.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirWebError {
    InvalidMir { message: String },
    InvalidLayout { ty: String },
    InvalidAssets { message: String },
    MissingCoreCall { call: u64 },
    MissingPreludeCall { call: u64 },
    MissingUserFunction { function: u64 },
    MissingEntryFunction { function: u64 },
}

impl std::fmt::Display for MirWebError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMir { message } => write!(f, "invalid MIR for Web emission: {message}"),
            Self::InvalidLayout { ty } => write!(f, "MIR type has an invalid layout: {ty}"),
            Self::InvalidAssets { message } => write!(f, "invalid Web publication assets: {message}"),
            Self::MissingCoreCall { call } => write!(f, "MIR CoreCallId {call} is not in the Core table"),
            Self::MissingPreludeCall { call } => write!(f, "MIR PreludeCallId {call} is not in the Prelude table"),
            Self::MissingUserFunction { function } => write!(f, "MIR function id {function} is not in the function table"),
            Self::MissingEntryFunction { function } => write!(f, "MIR entry function {function} is not Web-applicable"),
        }
    }
}

impl std::error::Error for MirWebError {}

/// Emit WebAssembly Rust, browser JavaScript, a manifest, and publication
pub fn emit_web(program: &MirProgram, target: &MirWebTarget) -> Result<WebArtifacts, MirWebError> {
    program
        .cffi
        .validate_boundaries()
        .map_err(|error| MirWebError::InvalidMir {
            message: format!("invalid foreign boundary facts: {error}"),
        })?;
    jet_foundation::MIROptimization::require_canonical_mir_optimization(program)
        .map_err(|error| MirWebError::InvalidMir { message: error.to_string() })?;
    validate_assets(&target.assets)?;
    validate_layouts(program)?;
    validate_web_facts(program)?;

    let artifact = select_web_artifact(program, target.artifact)?;
    let artifact_identity = program
        .artifact_identity(artifact.id)
        .map_err(|error| MirWebError::InvalidMir {
            message: format!("MIR Web artifact identity unavailable: {error}"),
        })?;
    let history_provenance = HistoryProvenance {
        source: artifact_identity.identity_digest(),
        tool: format!("{}:{}", HISTORY_ENGINE, program.facts.target_dossier.compiler_identity),
        target: sha256_hex(
            &program
                .facts
                .target_dossier
                .cache_bytes(&target.layout.triple),
        ),
    };
    let artifact_identity_text = artifact_identity.canonical_text();
    let entry = selected_web_entry(artifact)?;
    let functions = program
        .functions
        .iter()
        .filter(|function| {
            function.target_applicability.web
                && artifact.modules.iter().any(|module| *module == function.module_id)
        })
        .collect::<Vec<_>>();
    let Some(function) = functions.iter().find(|function| function.id == entry) else {
        return Err(MirWebError::MissingEntryFunction { function: entry.0 });
    };
    if !function_in_bucket(function, WebBucket::JS)
        && !function_in_bucket(function, WebBucket::Wasm)
    {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "MIR WebApplication entry {} has no checked Web partition",
                entry.0
            ),
        });
    }
    let wasm_rust = emit_wasm_rust(program, target, &functions)?;
    let js_app = emit_js_app(
        program,
        target,
        artifact,
        &functions,
        Some(entry),
        &artifact_identity_text,
        &history_provenance,
    )?;
    let js_source_map = emit_source_map(&target.assets);
    let manifest_json = emit_manifest(program, target, &functions, artifact, entry, &artifact_identity_text)?;
    let command_record = emit_command_record(program, functions.as_slice(), artifact, entry)?;

    Ok(WebArtifacts {
        manifest_json,
        wasm_rust,
        js_app,
        js_source_map,
        onnx_runtime_js: if program.facts.model_outputs.is_empty() {
            String::new()
        } else {
            jet_rt::model::provider::browser::HOST_JAVASCRIPT.to_string()
        },
        onnx_runtime_worker_js: if program.facts.model_outputs.is_empty() {
            String::new()
        } else {
            jet_rt::model::provider::browser::WORKER_JAVASCRIPT.to_string()
        },
        source_names: target.assets.source_names.clone(),
        source_contents: target.assets.source_contents.clone(),
        dom_runtime: target.assets.dom_runtime.clone(),
        index_html: target.assets.index_html.clone(),
        explicit_html_path: target.assets.explicit_html_path.clone(),
        command_record,
    })
}

fn select_web_artifact(
    program: &MirProgram,
    artifact_id: jet_foundation::MIR::MirArtifactId,
) -> Result<&MirArtifactPlan, MirWebError> {
    let Some(artifact) = program.artifacts.iter().find(|artifact| artifact.id == artifact_id) else {
        return Err(MirWebError::InvalidMir {
            message: format!("MIR artifact {} is missing", artifact_id.0),
        });
    };
    if artifact.kind != MirArtifactKind::WebApplication {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "MIR artifact {} has kind {:?}, expected WebApplication",
                artifact_id.0, artifact.kind
            ),
        });
    }
    if artifact.target != MirArtifactTarget::Web {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "MIR WebApplication artifact {} targets {:?}, not Web",
                artifact.id.0, artifact.target
            ),
        });
    }
    if artifact.modules.is_empty() {
        return Err(MirWebError::InvalidMir {
            message: format!("MIR WebApplication artifact {} has no selected modules", artifact.id.0),
        });
    }
    for module in &artifact.modules {
        if !program.modules.iter().any(|row| row.id == *module) {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "MIR WebApplication artifact {} references missing module {}",
                    artifact.id.0, module.0
                ),
            });
        }
    }
    for link in &artifact.links {
        if !program.links.iter().any(|row| row.id == *link) {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "MIR WebApplication artifact {} references missing link unit {}",
                    artifact.id.0, link.0
                ),
            });
        }
    }
    for job in &artifact.jobs {
        if !program.jobs.iter().any(|row| row.id == *job) {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "MIR WebApplication artifact {} references missing job {}",
                    artifact.id.0, job.0
                ),
            });
        }
    }
    let Some(entry) = &artifact.entry else {
        return Err(MirWebError::InvalidMir {
            message: format!("MIR WebApplication artifact {} has no entry", artifact.id.0),
        });
    };
    if let Some(function) = entry.function {
        if !program.functions.iter().any(|row| row.id == function) {
            return Err(MirWebError::MissingEntryFunction { function: function.0 });
        }
    } else {
        return Err(MirWebError::InvalidMir {
            message: format!("MIR WebApplication artifact {} has no entry function", artifact.id.0),
        });
    }
    if let Some(harness) = artifact.harness {
        if !program.harnesses.iter().any(|row| row.id == harness) {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "MIR WebApplication artifact {} references missing harness {}",
                    artifact.id.0, harness.0
                ),
            });
        }
    }
    for export in &artifact.exports {
        if export.symbol.is_empty() {
            return Err(MirWebError::InvalidMir {
                message: format!("MIR WebApplication artifact {} has an unnamed export", artifact.id.0),
            });
        }
        let Some(function) = program.functions.iter().find(|row| row.id == export.function) else {
            return Err(MirWebError::MissingUserFunction { function: export.function.0 });
        };
        if !artifact.modules.iter().any(|module| *module == function.module_id) {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "MIR WebApplication artifact {} export {} belongs to unselected module {}",
                    artifact.id.0, export.function.0, function.module_id.0
                ),
            });
        }
        if !function.target_applicability.web {
            return Err(MirWebError::MissingUserFunction { function: export.function.0 });
        }
    }
    Ok(artifact)
}

fn selected_web_entry(artifact: &MirArtifactPlan) -> Result<jet_foundation::MIR::MirFunctionId, MirWebError> {
    artifact
        .entry
        .as_ref()
        .and_then(|entry| entry.function)
        .ok_or_else(|| MirWebError::InvalidMir {
            message: format!("MIR WebApplication artifact {} has no entry function", artifact.id.0),
        })
}

fn validate_assets(assets: &MirWebAssets) -> Result<(), MirWebError> {
    if assets.source_names.len() != assets.source_contents.len() {
        return Err(MirWebError::InvalidAssets {
            message: format!(
                "source_names has {} rows but source_contents has {} rows",
                assets.source_names.len(),
                assets.source_contents.len()
            ),
        });
    }
    if assets.dom_runtime.is_empty() {
        return Err(MirWebError::InvalidAssets { message: "dom_runtime is empty".to_string() });
    }
    if assets.index_html.is_empty() {
        return Err(MirWebError::InvalidAssets { message: "index_html is empty".to_string() });
    }
    Ok(())
}

fn validate_layouts(program: &MirProgram) -> Result<(), MirWebError> {
    let check = |ty: &MirType| {
        if ty.has_valid_layout() {
            Ok(())
        } else {
            Err(MirWebError::InvalidLayout { ty: ty.name() })
        }
    };
    for type_def in &program.types {
        check_type_def(type_def, &check)?;
    }
    for function in &program.functions {
        if let Some(generator) = &function.generator {
            check(&generator.item)?;
        }
        for parameter in &function.params {
            check(&parameter.ty)?;
        }
        if let Some(return_type) = &function.declared_return {
            check(return_type)?;
        }
        check(&function.return_type)?;
        for local in &function.locals {
            check(&local.ty)?;
        }
        for (_, ty, _, _) in &function.values {
            check(ty)?;
        }
        for place in &function.places {
            check(&place.ty)?;
        }
        for reconstruction in &function.web_param_reconstructions {
            check(&reconstruction.ty)?;
            for field in &reconstruction.fields {
                check(&field.ty)?;
            }
        }
        for block in &function.blocks {
            for instruction in &block.instructions {
                if let Some(ty) = &instruction.ty {
                    check(ty)?;
                }
                check_operation_types(&instruction.operation, &check)?;
            }
        }
    }
    for foreign in &program.foreign {
        for parameter in &foreign.params {
            check(&parameter.ty)?;
        }
        if let Some(return_type) = &foreign.return_type {
            check(return_type)?;
        }
    }
    Ok(())
}

fn check_type_def(
    type_def: &MirTypeDef,
    check: &impl Fn(&MirType) -> Result<(), MirWebError>,
) -> Result<(), MirWebError> {
    match &type_def.kind {
        MirTypeDefKind::Struct { fields, .. } => {
            for field in fields {
                check(&field.ty)?;
            }
        }
        MirTypeDefKind::Enum { variants, .. } => {
            for variant in variants {
                match &variant.payload {
                    MirVariantPayload::Unit => {}
                    MirVariantPayload::Single(ty) => check(ty)?,
                    MirVariantPayload::Named(fields) => {
                        for field in fields {
                            check(&field.ty)?;
                        }
                    }
                }
            }
        }
        MirTypeDefKind::Distinct { base, .. } => check(base)?,
        MirTypeDefKind::Alias { target } => check(target)?,
        MirTypeDefKind::UnitFamily { .. } => {}
    }
    Ok(())
}

fn check_operation_types(
    operation: &MirOperation,
    check: &impl Fn(&MirType) -> Result<(), MirWebError>,
) -> Result<(), MirWebError> {
    match operation {
        MirOperation::Convert { target, .. }
        | MirOperation::PtrFromAddr { element: target, .. } => check(target)?,
        MirOperation::Present { .. } | MirOperation::ResultOk { .. } => {}
        MirOperation::Semantic(semantic) => match semantic {
            MirSemanticOp::MathBuiltin { .. }
            | MirSemanticOp::PreciseBuiltin { .. }
            | MirSemanticOp::Print { .. }
            | MirSemanticOp::AmbientInput { .. }
            | MirSemanticOp::RequireStop { .. }
            | MirSemanticOp::LayoutCompare { .. }
            | MirSemanticOp::LayoutLiteral { .. }
            | MirSemanticOp::StructLiteral { .. }
            | MirSemanticOp::CellGuardProject { .. }
            | MirSemanticOp::SharedGuardMap { .. }
            | MirSemanticOp::SharedGuardSplit { .. }
            | MirSemanticOp::SharedGuardWait { .. }
            | MirSemanticOp::ConditionNotify { .. }
            | MirSemanticOp::AllocNew { .. }
            | MirSemanticOp::ColumnarRead { .. }
            | MirSemanticOp::StaticPreludeCall { .. }
            | MirSemanticOp::DecodeUnder { .. }
            | MirSemanticOp::HardwareCall { .. }
            | MirSemanticOp::BuiltinMethod { .. }
            | MirSemanticOp::OptionLift2 { .. }
            | MirSemanticOp::ClosureMethod { .. }
            | MirSemanticOp::HostBorrowCallback { .. }
            | MirSemanticOp::NumericMethod { .. }
            | MirSemanticOp::NumericBinaryMethod { .. }
            | MirSemanticOp::OverflowOption { .. }
            | MirSemanticOp::HandleMethod { .. }
            | MirSemanticOp::PluginInvoke { .. }
            | MirSemanticOp::CoreClosureCall { .. }
            | MirSemanticOp::TaskGroup { .. }
            | MirSemanticOp::Select { .. }
            | MirSemanticOp::PolicyFunction { .. }
            | MirSemanticOp::InterruptFunction { .. }
            | MirSemanticOp::CarrierFact { .. }
            | MirSemanticOp::GcEdit { .. }
            | MirSemanticOp::TypedTextInterp { .. }
            | MirSemanticOp::HostCall { .. }
            | MirSemanticOp::DataEntriesToMap { .. }
            | MirSemanticOp::TextPatternMatch { .. }
            | MirSemanticOp::BinaryPatternMatch { .. }
            | MirSemanticOp::HttpRouterRegister { .. } => {}
            MirSemanticOp::CCallback { .. } => {}
        },
        MirOperation::Parameter { .. }
        | MirOperation::Capture { .. }
        | MirOperation::Global { .. }
        | MirOperation::Phi { .. }
        | MirOperation::ReadPlace(_)
        | MirOperation::MovePlace { .. }
        | MirOperation::WritePlace { .. }
        | MirOperation::InitializeUninit { .. }
        | MirOperation::Copy { .. }
        | MirOperation::Move { .. }
        | MirOperation::Constant(_)
        | MirOperation::Unary { .. }
        | MirOperation::Binary { .. }
        | MirOperation::BuildString { .. }
        | MirOperation::BuildList { .. }
        | MirOperation::BuildMap { .. }
        | MirOperation::EnumIs { .. }
        | MirOperation::EnumPayload { .. }
        | MirOperation::OptionIsSome { .. }
        | MirOperation::OptionValue { .. }
        | MirOperation::ResultIsOk { .. }
        | MirOperation::ResultValue { .. }
        | MirOperation::PatternCapture { .. }
        | MirOperation::PatternMatched { .. }
        | MirOperation::ProjectMembers { .. }
        | MirOperation::Index { .. }
        | MirOperation::Slice { .. }
        | MirOperation::Range { .. }
        | MirOperation::Field { .. }
        | MirOperation::Struct { .. }
        | MirOperation::Enum { .. }
        | MirOperation::Tuple { .. }
        | MirOperation::Absent
        | MirOperation::ResultErr { .. }
        | MirOperation::Call { .. }
        | MirOperation::CoreCall { .. }
        | MirOperation::IndirectCall { .. }
        | MirOperation::Closure { .. }
        | MirOperation::Deref { .. }
        | MirOperation::RawAddressOf { .. }
        | MirOperation::AddressOf { .. }
        | MirOperation::AttachTag { .. }
        | MirOperation::Todo { .. }
        | MirOperation::Never { .. }
        | MirOperation::LoopRangeInit { .. }
        | MirOperation::LoopRangeHasNext { .. }
        | MirOperation::LoopRangeValue { .. }
        | MirOperation::LoopRangeAdvance { .. }
        | MirOperation::LoopIterInit { .. }
        | MirOperation::LoopIterHasNext { .. }
        | MirOperation::LoopIterValue { .. }
        | MirOperation::LoopIterAdvance { .. }
        | MirOperation::ScopeEnter { .. }
        | MirOperation::ScopeExit { .. }
        | MirOperation::Drop { .. } => {}
    }
    Ok(())
}
/// `Target.Hardware` is the canonical capability spelling in the target
/// validation diagnostic. Target validation owns user-facing E3311; Web only
/// reports malformed MIR if a checked hardware fact leaks past that gate.
const WEB_TARGET_HARDWARE_CAPABILITY: &str = "Target.Hardware";

fn web_hardware_leak(detail: &str) -> MirWebError {
    MirWebError::InvalidMir {
        message: format!(
            "E3311: Target `Web` lacks the required `{WEB_TARGET_HARDWARE_CAPABILITY}` \
             capability; target validation must reject this hardware use before Web emission ({detail})"
        ),
    }
}

fn reject_leaked_web_hardware(program: &MirProgram) -> Result<(), MirWebError> {
    if program.facts.hardware_setups.first().is_some() {
        return Err(web_hardware_leak(
            "a package hardware setup reached the Web adapter",
        ));
    }
    for function in &program.functions {
        for block in &function.blocks {
            for instruction in &block.instructions {
                if !matches!(
                    &instruction.operation,
                    MirOperation::Semantic(MirSemanticOp::HardwareCall { .. })
                ) {
                    continue;
                }
                return Err(web_hardware_leak(&format!(
                    "function {} at {:?} reached the Web adapter",
                    function.id.0, instruction.span
                )));
            }
        }
    }
    Ok(())
}

/// Validate checked Web partition facts. Target applicability is owned by
/// sema; the hardware check above is only a defensive leak detector.
fn validate_web_facts(program: &MirProgram) -> Result<(), MirWebError> {
    reject_leaked_web_hardware(program)?;
    for function in &program.functions {
        if function.target_applicability.web && function.web_bucket.is_none() {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "function {} is Web-applicable but has no checked Web bucket",
                    function.id.0
                ),
            });
        }
        match (function.web_bucket, function.web_marker) {
            (Some(WebBucket::JS), Some(WebPartitionMarker::Wasm | WebPartitionMarker::WasmExport))
            | (Some(WebBucket::Wasm), Some(WebPartitionMarker::JS)) => {
                return Err(MirWebError::InvalidMir {
                    message: format!("function {} has contradictory Web bucket and marker", function.id.0),
                });
            }
            (Some(WebBucket::JS), Some(WebPartitionMarker::JS))
            | (Some(WebBucket::Wasm), Some(WebPartitionMarker::Wasm))
            | (Some(WebBucket::Wasm), Some(WebPartitionMarker::WasmExport))
            | (Some(WebBucket::JS), None)
            | (Some(WebBucket::Wasm), None)
            | (None, Some(WebPartitionMarker::JS))
            | (None, Some(WebPartitionMarker::Wasm))
            | (None, Some(WebPartitionMarker::WasmExport))
            | (None, None) => {}
        }
        if function.target_applicability.web && function_in_bucket(function, WebBucket::JS) {
            for block in &function.blocks {
                for instruction in &block.instructions {
                    let MirOperation::Call {
                        callee: MirCallee::Foreign(foreign_id),
                        ..
                    } = &instruction.operation
                    else {
                        continue;
                    };
                    let Some(foreign) = program.foreign.iter().find(|foreign| foreign.id == *foreign_id) else {
                        return Err(MirWebError::InvalidMir {
                            message: format!(
                                "function {} at {:?} references missing foreign row {}",
                                function.id.0, instruction.span, foreign_id.0
                            ),
                        });
                    };
                    let abi = foreign_abi_name(&foreign.foreign_abi);
                    return Err(MirWebError::InvalidMir {
                        message: format!(
                            "function {} at {:?} contains JS-partition foreign row {} declared at {:?} \
                             (symbol `{}` path `{}` ABI `{}` language {:?} applicability \
                             rust_aot={} cranelift={} interpreter={} web={}); no checked JavaScript linkage exists",
                            function.id.0,
                            instruction.span,
                            foreign.id.0,
                            foreign.span,
                            foreign.symbol,
                            foreign.path,
                            abi,
                            foreign.foreign_language,
                            foreign.target_applicability.rust_aot,
                            foreign.target_applicability.cranelift,
                            foreign.target_applicability.interpreter,
                            foreign.target_applicability.web,
                        ),
                    });
                }
            }
        }
        for reconstruction in &function.web_param_reconstructions {
            for field in &reconstruction.fields {
                if field.parameter.is_empty() || field.field.is_empty() {
                    return Err(MirWebError::InvalidMir {
                        message: format!("function {} has an empty Web reconstruction name", function.id.0),
                    });
                }
            }
        }
    }
    Ok(())
}

fn emit_wasm_rust(
    program: &MirProgram,
    target: &MirWebTarget,
    _functions: &[&MirFunction],
) -> Result<String, MirWebError> {
    // Rust emission is the canonical MIR Rust printer.  This adapter supplies
    // only the Web/Wasm target facts; it does not lower a second copy of CFG,
    // ownership, failure, or Core/Prelude semantics.
    let config = MirRustConfig {
        target: target.layout.clone(),
        target_kind: MirRustTarget::WebWasm,
        root_prefix: String::new(),
        execution: MirRustExecutionConfig {
            artifact: target.artifact,
            ffi: None,
            emit_types: true,
            emit_foreign: true,
            emit_metadata: true,
            emit_debug_linemap: false,
            emit_runtime: true,
            semantic_digest: Some(target.semantic_digest),
            release_devtools_policy: target.release_devtools_policy.clone(),
        },
    };
    let mut out = String::from("// Generated by jet from optimized MIR; Web/Wasm target.\n");
    out.push_str(&emit_mir_program(program, &config));
    Ok(out)
}
fn model_trait_for_type_args<'a>(
    program: &'a MirProgram,
    type_args: &[MirType],
) -> Option<&'a jet_foundation::MIR::MirTraitDef> {
    let Some(first) = type_args.first() else {
        return None;
    };
    let names = match &first.kind {
        MirTypeKind::TraitObject(rows) => rows.iter().map(|row| row.name.as_str()).collect::<Vec<_>>(),
        MirTypeKind::Apply { name, .. } => vec![name.name.as_str()],
        _ => Vec::new(),
    };
    program.traits.iter().find(|definition| {
        names.iter().any(|name| *name == definition.name || *name == definition.key)
            && program.facts.model_outputs.iter().any(|fact| {
                fact.signature_name.as_deref() == Some(definition.name.as_str())
            })
    })
}

fn model_trait_for_function<'a>(
    program: &'a MirProgram,
    id: jet_foundation::MIR::MirFunctionId,
) -> Option<&'a jet_foundation::MIR::MirTraitDef> {
    let function = program.functions.iter().find(|function| function.id == id)?;
    let MirFunctionForm::TraitMethod { trait_ref, .. } = &function.form else {
        return None;
    };
    let definition = program.traits.iter().find(|definition| definition.id == trait_ref.id)?;
    (function.name == "embed"
        && program.facts.model_outputs.iter().any(|fact| {
            fact.signature_name.as_deref() == Some(definition.name.as_str())
        }))
    .then_some(definition)
}

fn model_bridge_prefix(definition: &jet_foundation::MIR::MirTraitDef) -> String {
    mangle_path(&definition.key)
}

fn emit_model_web_bridges(
    out: &mut String,
    program: &MirProgram,
) -> Result<(), MirWebError> {
    if program.facts.model_outputs.is_empty() {
        return Ok(());
    }
    out.push_str(
        "const __jetModelUtf8 = new TextEncoder();\n\
         const __jetModelUtf8Decode = new TextDecoder(\"utf-8\", { fatal: true });\n\
         const __jetModelWait = () => new Promise((resolve) => setTimeout(resolve, 0));\n\
         function __jetModelInput(prefix, value) {\n\
           const bytes = value instanceof Uint8Array ? value : __jetModelUtf8.encode(value);\n\
           const pointer = Number(__jetPreludeWasm[`__jet_model_web_input_alloc_${prefix}`](bytes.length));\n\
           if (!Number.isSafeInteger(pointer) || (bytes.length !== 0 && pointer === 0)) throw new Error(\"model Web input allocation failed\");\n\
           if (bytes.length !== 0) new Uint8Array(__jetPreludeWasm.memory.buffer, pointer, bytes.length).set(bytes);\n\
           return [pointer, bytes.length];\n\
         }\n\
         function __jetModelDocuments(documents) {\n\
           if (!Array.isArray(documents)) throw new Error(\"model Web documents must be a list\");\n\
           const values = documents.map((document) => __jetModelUtf8.encode(String(document)));\n\
           const size = 4 + values.reduce((total, value) => total + 4 + value.length, 0);\n\
           const bytes = new Uint8Array(size);\n\
           const view = new DataView(bytes.buffer);\n\
           view.setUint32(0, values.length, true);\n\
           let offset = 4;\n\
           for (const value of values) { view.setUint32(offset, value.length, true); offset += 4; bytes.set(value, offset); offset += value.length; }\n\
           return bytes;\n\
         }\n\
         function __jetModelBatch(bytes) {\n\
           const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);\n\
           let offset = 0;\n\
           const u32 = () => { if (offset + 4 > view.byteLength) throw new Error(\"truncated model Web batch\"); const value = view.getUint32(offset, true); offset += 4; return value; };\n\
           const u64 = () => { const low = u32(); const high = u32(); const value = high * 4294967296 + low; if (!Number.isSafeInteger(value)) throw new Error(\"model Web batch dimension is too large\"); return value; };\n\
           const text = () => { const length = u32(); if (offset + length > view.byteLength) throw new Error(\"truncated model Web batch text\"); const value = __jetModelUtf8Decode.decode(new Uint8Array(view.buffer, view.byteOffset + offset, length)); offset += length; return value; };\n\
           const rows = u32();\n\
           const values = [];\n\
           for (let row = 0; row < rows; row += 1) { const columns = u32(); const value = []; for (let column = 0; column < columns; column += 1) value.push(f32()); values.push(value); }\n\
           const space = { model_digest: text(), dimension: u64(), metric: text(), normalization: text() };\n\
           return { values, space };\n\
         }\n\
         async function __jetModelPoll(prefix, operation, handle) {\n\
           const wasm = __jetPreludeWasm;\n\
           const poll = wasm[`__jet_model_web_${operation}_poll_${prefix}`];\n\
           const pointer = wasm[`__jet_model_web_${operation}_result_ptr_${prefix}`];\n\
           const length = wasm[`__jet_model_web_${operation}_result_len_${prefix}`];\n\
           const release = wasm[`__jet_model_web_${operation}_result_free_${prefix}`];\n\
           if (typeof poll !== \"function\" || typeof pointer !== \"function\" || typeof length !== \"function\" || typeof release !== \"function\") throw new Error(\"model Web async bridge is unavailable\");\n\
           for (;;) {\n\
             const status = Number(poll(handle));\n\
             if (status === 0) { await __jetModelWait(); continue; }\n\
             const size = Number(length(handle));\n\
             const address = Number(pointer(handle));\n\
             if (!Number.isSafeInteger(address) || !Number.isSafeInteger(size) || address < 0 || size < 0 || address + size > wasm.memory.buffer.byteLength) {\n\
               release(handle);\n\
               throw new Error(\"model Web async bridge returned an invalid result buffer\");\n\
             }\n\
             const bytes = new Uint8Array(wasm.memory.buffer, address, size).slice();\n\
             release(handle);\n\
             if (status === 2) throw new Error(__jetModelUtf8Decode.decode(bytes) || \"model Web provider failed\");\n\
             if (status !== 1) throw new Error(\"model Web async bridge returned an unknown status\");\n\
             return bytes;\n\
           }\n\
         }\n\
",
    );
    for definition in &program.traits {
        let Some(fact) = program
            .facts
            .model_outputs
            .iter()
            .find(|fact| fact.signature_name.as_deref() == Some(definition.name.as_str()))
        else {
            continue;
        };
        let _ = fact;
        let prefix = model_bridge_prefix(definition);
        writeln!(
            out,

            "async function __jet_model_open_{prefix}(output) {{
  const [pointer, length] = __jetModelInput({prefix:?}, String(output));
  let handle;
  try {{
    handle = Number(__jetPreludeWasm[`__jet_model_web_open_start_{prefix}`](pointer, length));
  }} finally {{
    __jetPreludeWasm[`__jet_model_web_input_free_{prefix}`](pointer);
  }}
  if (!Number.isSafeInteger(handle) || handle === 0) throw new Error(\"model Web open failed to start\");
  const packet = await __jetModelPoll({prefix:?}, \"open\", handle);
  if (packet.byteLength !== 4) throw new Error(\"model Web open returned an invalid session handle\");
  return {{ __jet_model_trait: {prefix:?}, handle: new DataView(packet.buffer, packet.byteOffset, 4).getUint32(0, true) }};
}}
async function __jet_model_embed_{prefix}(receiver, documents) {{
  if (!receiver || receiver.__jet_model_trait !== {prefix:?}) throw new Error(\"model Web session belongs to a different trait\");
  const [pointer, length] = __jetModelInput({prefix:?}, __jetModelDocuments(documents));
  let handle;
  try {{
    handle = Number(__jetPreludeWasm[`__jet_model_web_embed_start_{prefix}`](receiver.handle, pointer, length));
  }} finally {{
    __jetPreludeWasm[`__jet_model_web_input_free_{prefix}`](pointer);
  }}
  if (!Number.isSafeInteger(handle) || handle === 0) throw new Error(\"model Web embed failed to start\");
  return __jetModelBatch(await __jetModelPoll({prefix:?}, \"embed\", handle));
}}
"
        )
        .map_err(|_| MirWebError::InvalidMir {
            message: format!("model Web bridge for `{}` could not be rendered", definition.name),
        })?;
    }
    Ok(())
}

fn web_model_core_call(program: &MirProgram, id: jet_foundation::MIR::MirCoreCallId) -> bool {
    program
        .core_calls
        .iter()
        .find(|call| call.id == id)
        .is_some_and(|call| call.module == "core.models" && call.member == "open")
}
fn web_task_join_call(program: &MirProgram, id: MirPreludeCallId) -> bool {
    program.prelude_calls.iter().find(|call| call.id == id).is_some_and(|call| {
        call.family == MirPreludeFamily::HandleMethod
            && call.module == "core.tasks"
            && call.member == "join"
    })
}
fn web_channel_handle_call(program: &MirProgram, id: MirPreludeCallId) -> bool {
    program.prelude_calls.iter().find(|call| call.id == id).is_some_and(|call| {
        let expected_symbol = match call.member.as_str() {
            "receiver.receive" => "jet_std::JetReceiver::receive",
            "receiver.close" => "jet_std::JetReceiver::close",
            "sender.send" => "jet_std::JetSender::send",
            "sender.close" => "jet_std::JetSender::close",
            _ => return false,
        };
        call.family == MirPreludeFamily::HandleMethod
            && call.module == "core.channels"
            && call.abi == MirPreludeAbi::Value
            && call.symbol.name() == expected_symbol
    })
}

fn web_channel_select_call(program: &MirProgram, id: MirPreludeCallId) -> bool {
    program.prelude_calls.iter().find(|call| call.id == id).is_some_and(|call| {
        let expected_symbol = match call.member.as_str() {
            "select_wait_tagged" => "jet_std::jet_select_wait_tagged",
            "select_try_wait_tagged" => "jet_std::jet_select_try_wait_tagged",
            _ => return false,
        };
        call.family == MirPreludeFamily::StaticPrelude
            && call.module == "core.tasks"
            && call.abi == MirPreludeAbi::Value
            && call.symbol.name() == expected_symbol
    })
}

fn web_channel_async_call(program: &MirProgram, id: MirPreludeCallId) -> bool {
    if web_channel_select_call(program, id) {
        return program
            .prelude_calls
            .iter()
            .find(|call| call.id == id)
            .is_some_and(|call| call.member == "select_wait_tagged");
    }
    program.prelude_calls.iter().find(|call| call.id == id).is_some_and(|call| {
        web_channel_handle_call(program, id)
            && matches!(call.member.as_str(), "receiver.receive" | "sender.send")
    })
}

fn web_channel_select_wait_call(program: &MirProgram, id: MirPreludeCallId) -> bool {
    program
        .prelude_calls
        .iter()
        .find(|call| call.id == id)
        .is_some_and(|call| {
            web_channel_select_call(program, id) && call.member == "select_wait_tagged"
        })
}


fn web_async_function_ids(program: &MirProgram) -> BTreeSet<jet_foundation::MIR::MirFunctionId> {
    let mut async_functions = BTreeSet::new();
    loop {
        let mut changed = false;
        for function in &program.functions {
            if function.generator.is_some() || async_functions.contains(&function.id) {
                continue;
            }
            let needs_async = function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| match &instruction.operation {
                    MirOperation::CoreCall { call, .. } => web_model_core_call(program, *call),
                    MirOperation::Call { callee, .. } => match callee {
                        MirCallee::Core(id) => web_model_core_call(program, *id),
                        MirCallee::Prelude(id) => {
                            web_task_join_call(program, *id) || web_channel_async_call(program, *id)
                        }
                        MirCallee::Foreign(_) | MirCallee::Indirect(_) => false,
                        MirCallee::User(id)
                        | MirCallee::Associated { function: id, .. }
                        | MirCallee::Method { function: id, .. } => {
                            let wasm_export = program
                                .functions
                                .iter()
                                .find(|function| function.id == *id)
                                .is_some_and(|function| is_wasm_export(function));
                            model_trait_for_function(program, *id).is_some()
                                || wasm_export
                                || async_functions.contains(id)
                        }
                        MirCallee::TraitMethod { method, trait_ref, .. } => {
                            match web_trait_method_targets(program, *method, trait_ref) {
                                Ok((targets, default)) => {
                                    targets.iter().any(|(_, id)| {
                                        program
                                            .functions
                                            .iter()
                                            .find(|function| function.id == *id)
                                            .is_some_and(|function| {
                                                function.generator.is_none()
                                                    && (web_function_is_async(program, *id)
                                                        || async_functions.contains(id))
                                            })
                                    }) || default.is_some_and(|id| {
                                        program
                                            .functions
                                            .iter()
                                            .find(|function| function.id == id)
                                            .is_some_and(|function| {
                                                function.generator.is_none()
                                                    && (web_function_is_async(program, id)
                                                        || async_functions.contains(&id))
                                            })
                                    })
                                }
                                Err(_) => false,
                            }
                        }
                    },
                    MirOperation::Semantic(MirSemanticOp::HandleMethod { call, .. }) => {
                        web_task_join_call(program, *call) || web_channel_async_call(program, *call)
                    }
                    MirOperation::Semantic(MirSemanticOp::Select { call, .. }) => {
                        web_channel_async_call(program, *call)
                    }
                    _ => false,
                })
            });
            if needs_async {
                async_functions.insert(function.id);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    async_functions
}

fn web_function_is_async(program: &MirProgram, id: jet_foundation::MIR::MirFunctionId) -> bool {
    web_async_function_ids(program).contains(&id)
}

fn wasm_export_symbol(function: &MirFunction) -> String {
    format!("jet_export_{}", function.name)
}

fn wasm_bridge_name(function: &MirFunction) -> String {
    format!("bridge_{}", function.name)
}

fn wasm_abi_list_kind(inner: &MirType) -> Option<&'static str> {
    match &inner.kind {
        MirTypeKind::Int => Some("list-int"),
        MirTypeKind::String => Some("list-string"),
        MirTypeKind::IntN {
            signed: true,
            bits: 64,
        } => Some("list-i64"),
        MirTypeKind::InlineRange { base, .. }
        | MirTypeKind::Tagged { inner: base, .. } => wasm_abi_list_kind(base),
        _ => None,
    }
}

fn wasm_abi_kind(ty: &MirType) -> Result<Option<&'static str>, MirWebError> {
    if ty.is_unit() {
        return Ok(None);
    }
    match &ty.kind {
        MirTypeKind::Int => Ok(Some("int")),
        MirTypeKind::String => Ok(Some("string")),
        MirTypeKind::List(inner) => wasm_abi_list_kind(inner)
            .map(Some)
            .ok_or_else(|| MirWebError::InvalidMir {
                message: format!(
                    "unsupported Web Wasm export collection type `{}`",
                    ty.display_name()
                ),
            }),
        MirTypeKind::Map { key, value }
            if matches!(&key.kind, MirTypeKind::String)
                && matches!(&value.kind, MirTypeKind::Int) =>
        {
            Ok(Some("map-string-int"))
        }
        MirTypeKind::InlineRange { base, .. }
        | MirTypeKind::Tagged { inner: base, .. }
        | MirTypeKind::Quantity { base, .. } => wasm_abi_kind(base),
        MirTypeKind::Float
        | MirTypeKind::Bool
        | MirTypeKind::Char
        | MirTypeKind::IntN { .. }
        | MirTypeKind::Float32 => Ok(None),
        MirTypeKind::Apply { .. }
        | MirTypeKind::Map { .. }
        | MirTypeKind::FixedList { .. }
        | MirTypeKind::Shared(_)
        | MirTypeKind::Option(_)
        | MirTypeKind::Result { .. }
        | MirTypeKind::Fn(_)
        | MirTypeKind::SendFn { .. }
        | MirTypeKind::TraitObject(_)
        | MirTypeKind::Tuple(_)
        | MirTypeKind::Union(_)
        | MirTypeKind::Measure(_) => Err(MirWebError::InvalidMir {
            message: format!(
                "unsupported Web Wasm export boundary type `{}`",
                ty.display_name()
            ),
        }),
    }
}

fn wasm_direct_argument(value: &str, ty: &MirType) -> Result<String, MirWebError> {
    match &ty.kind {
        MirTypeKind::Float | MirTypeKind::Float32 => Ok(format!("Number({value})")),
        MirTypeKind::Bool => Ok(format!("Number(Boolean({value}))")),
        MirTypeKind::Char => Ok(format!("Number(String({value}).codePointAt(0))")),
        MirTypeKind::IntN { bits, .. } => {
            if *bits <= 32 {
                Ok(format!("Number({value})"))
            } else {
                Ok(format!("BigInt({value})"))
            }
        }
        MirTypeKind::InlineRange { base, .. }
        | MirTypeKind::Tagged { inner: base, .. }
        | MirTypeKind::Quantity { base, .. } => wasm_direct_argument(value, base),
        _ => Err(MirWebError::InvalidMir {
            message: format!(
                "unsupported direct Web Wasm export argument type `{}`",
                ty.display_name()
            ),
        }),
    }
}

fn wasm_direct_return(raw: &str, ty: &MirType) -> Result<String, MirWebError> {
    match &ty.kind {
        MirTypeKind::Float | MirTypeKind::Float32 => Ok(format!("Number({raw})")),
        MirTypeKind::Bool => Ok(format!("Boolean(Number({raw}))")),
        MirTypeKind::Char => Ok(format!("String.fromCodePoint(Number({raw}))")),
        MirTypeKind::IntN { signed, bits } => Ok(format!(
            "BigInt.as{}N({bits}, BigInt({raw}))",
            if *signed { "Int" } else { "Uint" }
        )),
        MirTypeKind::InlineRange { base, .. }
        | MirTypeKind::Tagged { inner: base, .. }
        | MirTypeKind::Quantity { base, .. } => wasm_direct_return(raw, base),
        _ => Err(MirWebError::InvalidMir {
            message: format!(
                "unsupported direct Web Wasm export return type `{}`",
                ty.display_name()
            ),
        }),
    }
}

fn wasm_abi_argument(value: &str, ty: &MirType) -> Result<String, MirWebError> {
    match wasm_abi_kind(ty)? {
        Some(kind) => Ok(format!(
            "jetDom.marshalAbi({}, {}, wasm)",
            value,
            js_string(kind)
        )),
        None => wasm_direct_argument(value, ty),
    }
}

fn wasm_abi_return(raw: &str, ty: &MirType) -> Result<String, MirWebError> {
    match wasm_abi_kind(ty)? {
        Some(kind) => Ok(format!(
            "jetDom.unmarshalAbi({}, {}, wasm)",
            raw,
            js_string(kind)
        )),
        None if ty.is_unit() => Ok("undefined".to_string()),
        None => wasm_direct_return(raw, ty),
    }
}

fn wasm_bridge_arguments(
    function: &MirFunction,
    values: &[String],
) -> Result<Vec<String>, MirWebError> {
    if values.len() != function.params.len() {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "Web Wasm export `{}` received {} arguments but declares {}",
                function.name,
                values.len(),
                function.params.len()
            ),
        });
    }
    let mut rendered = Vec::new();
    for (param, value) in function.params.iter().zip(values) {
        if let Some(reconstruction) = function
            .web_param_reconstructions
            .iter()
            .find(|row| row.local == param.name)
        {
            for field in &reconstruction.fields {
                let field_value = format!("({value})[{}]", js_string(&field.field));
                rendered.push(wasm_abi_argument(&field_value, &field.ty)?);
            }
        } else {
            rendered.push(wasm_abi_argument(value, &param.ty)?);
        }
    }
    Ok(rendered)
}

fn emit_wasm_export_bridges(
    out: &mut String,
    functions: &[&MirFunction],
) -> Result<(), MirWebError> {
    let mut exports = functions
        .iter()
        .copied()
        .filter(|function| {
            is_wasm_export(function) && function_in_bucket(function, WebBucket::Wasm)
        })
        .collect::<Vec<_>>();
    exports.sort_by_key(|function| function.id);
    let mut names = BTreeSet::new();
    for function in exports {
        if !matches!(function.form, MirFunctionForm::TopLevel)
            || !function.capture_params.is_empty()
        {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "Web Wasm export `{}` is not a top-level capture-free function",
                    function.name
                ),
            });
        }
        if function.generator.is_some() {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "Web Wasm export `{}` cannot be a generator",
                    function.name
                ),
            });
        }
        if !names.insert(function.name.clone()) {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "Web Wasm export `{}` has a duplicate JavaScript bridge name",
                    function.name
                ),
            });
        }
        let values = (0..function.params.len())
            .map(|index| format!("args[{index}]"))
            .collect::<Vec<_>>();
        let arguments = wasm_bridge_arguments(function, &values)?;
        let success = match &function.failure {
            jet_foundation::MIR::MirFailureCarrier::Result { success, .. } => success,
            jet_foundation::MIR::MirFailureCarrier::Optional { value } => value,
            jet_foundation::MIR::MirFailureCarrier::Infallible
            | jet_foundation::MIR::MirFailureCarrier::Diverges { .. } => &function.return_type,
        };
        let result = wasm_abi_return("raw", success)?;
        let bridge = wasm_bridge_name(function);
        let symbol = wasm_export_symbol(function);
        writeln!(out, "async function {bridge}(...args) {{").unwrap();
        writeln!(
            out,
            "  if (args.length !== {}) throw new TypeError({});",
            function.params.len(),
            js_string(&format!(
                "Web Wasm export `{}` expects {} arguments",
                function.name,
                function.params.len()
            ))
        )
        .unwrap();
        out.push_str("  const wasm = __jetPreludeWasm;\n");
        writeln!(out, "  let raw = wasm.{symbol}({});", arguments.join(", ")).unwrap();
        out.push_str("  const outcome = jetDom.takeWasmError(wasm);\n");
        out.push_str(
            "  if (outcome?.tag === \"Host\") throw jet_web_wasm_host_error(outcome, null, Number(outcome.status ?? 101));\n",
        );
        match &function.failure {
            jet_foundation::MIR::MirFailureCarrier::Result { .. }
            | jet_foundation::MIR::MirFailureCarrier::Optional { .. } => {
                out.push_str("  if (outcome?.tag === \"Err\") return { tag: \"Err\", values: [outcome.error] };\n");
                writeln!(out, "  return {{ tag: \"Ok\", values: [{result}] }};").unwrap();
            }
            jet_foundation::MIR::MirFailureCarrier::Infallible
            | jet_foundation::MIR::MirFailureCarrier::Diverges { .. } => {
                out.push_str("  if (outcome) return jet_web_edge_result(outcome);\n");
                writeln!(out, "  return {result};").unwrap();
            }
        }
        out.push_str("}\n\n");
    }
    Ok(())
}

fn js_wasm_export_call(
    function: &MirFunction,
    caller: &MirFunction,
    args: &[jet_foundation::MIR::MirCallArg],
    values: &[String],
) -> Result<String, MirWebError> {
    if !function.target_applicability.web || !function_in_bucket(function, WebBucket::Wasm) {
        return Err(MirWebError::MissingUserFunction { function: function.id.0 });
    }
    if !matches!(function.form, MirFunctionForm::TopLevel)
        || !function.capture_params.is_empty()
    {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "Web Wasm export `{}` is not a top-level capture-free function",
                function.name
            ),
        });
    }
    if caller.generator.is_some() {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "generator function {} cannot await Web Wasm export `{}`",
                caller.id.0, function.name
            ),
        });
    }
    if args.iter().any(|arg| arg.spread) {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "Web Wasm export `{}` cannot receive a spread argument",
                function.name
            ),
        });
    }
    Ok(format!(
        "await {}({})",
        wasm_bridge_name(function),
        values.join(", ")
    ))
}

fn web_source_name(name: &str) -> &str {
    name.strip_prefix(jet_foundation::Syntax::GENERATED_NAME_PREFIX)
        .unwrap_or(name)
}

fn emit_js_print_type_facts(out: &mut String, program: &MirProgram) {
    out.push_str("const __jet_print_type_facts = Object.freeze({\n");
    for type_def in &program.types {
        write!(
            out,
            "  {}: {{ name: {}, auto_printable: {}, ",
            js_string(&type_def.id.0.to_string()),
            js_string(web_source_name(&type_def.name)),
            type_def.auto_printable,
        )
        .unwrap();
        match &type_def.kind {
            MirTypeDefKind::Struct { fields, .. } => {
                out.push_str("kind: \"struct\", fields: [");
                for (index, field) in fields.iter().enumerate() {
                    if index != 0 {
                        out.push_str(", ");
                    }
                    write!(
                        out,
                        "{{ key: {}, name: {}, computed: {} }}",
                        js_string(&field.name),
                        js_string(web_source_name(&field.name)),
                        field.computed,
                    )
                    .unwrap();
                }
                out.push_str("]");
            }
            MirTypeDefKind::Enum { variants, .. } => {
                out.push_str("kind: \"enum\", variants: [");
                for (index, variant) in variants.iter().enumerate() {
                    if index != 0 {
                        out.push_str(", ");
                    }
                    write!(
                        out,
                        "{{ tag: {}, name: {}, ",
                        js_string(&variant.name),
                        js_string(web_source_name(&variant.name)),
                    )
                    .unwrap();
                    match &variant.payload {
                        MirVariantPayload::Unit => out.push_str("kind: \"unit\", fields: []"),
                        MirVariantPayload::Single(_) => {
                            out.push_str("kind: \"single\", fields: []")
                        }
                        MirVariantPayload::Named(fields) => {
                            out.push_str("kind: \"named\", fields: [");
                            for (field_index, field) in fields.iter().enumerate() {
                                if field_index != 0 {
                                    out.push_str(", ");
                                }
                                write!(
                                    out,
                                    "{{ key: {}, name: {}, computed: {} }}",
                                    js_string(&field.name),
                                    js_string(web_source_name(&field.name)),
                                    field.computed,
                                )
                                .unwrap();
                            }
                            out.push(']');
                        }
                    }
                    out.push('}');
                }
                out.push(']');
            }
            MirTypeDefKind::Distinct { .. } => out.push_str("kind: \"transparent\""),
            MirTypeDefKind::Alias { .. } => out.push_str("kind: \"transparent\""),
            MirTypeDefKind::UnitFamily { members } => {
                out.push_str("kind: \"unit_family\", members: [");
                for (index, member) in members.iter().enumerate() {
                    if index != 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&js_string(web_source_name(member)));
                }
                out.push(']');
            }
        }
        out.push_str("},\n");
    }
    out.push_str("});\n");
}

fn emit_js_app(
    program: &MirProgram,
    target: &MirWebTarget,
    artifact: &MirArtifactPlan,
    functions: &[&MirFunction],
    entry: Option<jet_foundation::MIR::MirFunctionId>,
    artifact_identity: &str,
    history_provenance: &HistoryProvenance,
) -> Result<String, MirWebError> {
    let has_game_runtime = artifact
        .runtime_parts
        .iter()
        .any(|part| matches!(part, jet_foundation::MIR::MirRuntimePartId::Game));
    let has_model_runtime = !program.facts.model_outputs.is_empty();
    let has_ui_runtime = artifact
        .runtime_parts
        .contains(&jet_foundation::MIR::MirRuntimePartId::Ui);
    let mut out = String::new();
    out.push_str("import * as jetDom from \"./jet_dom_runtime.js\";\n");
    if has_model_runtime {
        out.push_str("import { createOnnxRuntimeWebHost } from \"./jet_onnx_runtime.js\";\n");
    }
    let shared_prelude = shared_js_prelude().map_err(|error| MirWebError::InvalidAssets {
        message: error.to_string(),
    })?;
    out.push_str(&shared_prelude);
    out.push('\n');
    emit_js_print_type_facts(&mut out, program);
    emit_js_copy_type_facts(&mut out, program);
    out.push('\n');
    out.push_str(if has_ui_runtime {
        "const __jetFontImports = await jet_ui_web_harfbuzz_imports();\n"
    } else {
        "const __jetFontImports = {};\n"
    });
    if has_model_runtime {
        out.push_str(
            "const __jetModelHost = createOnnxRuntimeWebHost(globalThis.__JET_ONNX_RUNTIME_ARCHIVE ?? null);\n\
             const __jetDataImports = jet_data_web_imports();\n\
             const __jetTestingHistoryImports = jet_testing_history_web_imports();\n\
             const __jetPreludeWasm = (await jetDom.instantiateWasm(new URL(\"./app.wasm\", import.meta.url).href, { ...__jetFontImports, ...__jetDataImports, ...__jetTestingHistoryImports, ...__jetModelHost.imports })).exports;\n\
             __jetModelHost.bind(__jetPreludeWasm);\n\
             const __jetModelTransport = __jetModelHost.transport;\n\
             const __jetModelTransportAwait = (session, operation, packet, signal) => {\
               const job = __jetModelTransport.start(session, operation, packet, signal ? { signal } : {});\
               return __jetModelTransport.resume(job);\
             };\n\
             jet_data_web_bind_wasm(__jetPreludeWasm);\n\
             jet_testing_history_web_bind_wasm(__jetPreludeWasm);\n\
             if (typeof __jetPreludeWasm.jet_data_web_register_types === \"function\") __jetPreludeWasm.jet_data_web_register_types();\n",
        );
    } else {
        out.push_str(
            "const __jetDataImports = jet_data_web_imports();\n\
             const __jetTestingHistoryImports = jet_testing_history_web_imports();\n\
             const __jetPreludeWasm = (await jetDom.instantiateWasm(new URL(\"./app.wasm\", import.meta.url).href, { ...__jetFontImports, ...__jetDataImports, ...__jetTestingHistoryImports })).exports;\n\
             jet_data_web_bind_wasm(__jetPreludeWasm);\n\
             jet_testing_history_web_bind_wasm(__jetPreludeWasm);\n\
             if (typeof __jetPreludeWasm.jet_data_web_register_types === \"function\") __jetPreludeWasm.jet_data_web_register_types();\n",
        );
    }
    if has_ui_runtime {
        out.push_str("jet_ui_web_harfbuzz_bind_app(__jetPreludeWasm);\n");
    }
    if has_game_runtime {
        out.push_str(
            "  if (typeof fetch === \"function\") {\n\
             \
             try {\n\
             \
             const __jetProjection = await fetch(\"/__jet_devtools/state\").then((response) => response.ok ? response.json() : null);\n\
             \
             if (__jetProjection?.session_id) {\n\
             \
             globalThis.__jetDevtoolsGameIdentity = {\n\
             \
             session_id: __jetProjection.session_id,\n\
             \
             source_id: __jetProjection.source_id || \"game\",\n\
             \
             build_id: __jetProjection.build_id || \"runtime\",\n\
             \
             revision: __jetProjection.revision || \"runtime\",\n\
             \
             };\n\
             \
             }\n\
             \
             } catch (_) {}\n\
             \
             }\n",
        );
    }
    emit_model_web_bridges(&mut out, program)?;
    emit_wasm_export_bridges(&mut out, functions)?;
    emit_web_module_registry(&mut out, program, artifact, artifact_identity)?;
    let mut js_functions = functions
        .iter()
        .copied()
        .filter(|function| {
            function_in_bucket(function, WebBucket::JS)
                && artifact.modules.iter().any(|module| *module == function.module_id)
        })
        .collect::<Vec<_>>();
    js_functions.sort_by_key(|function| function.id);
    for function in js_functions {
        emit_js_function(
            &mut out,
            program,
            function,
            target.release_devtools_policy.local_rail,
            history_provenance,
        )?;
    }
    let entry_row = if let Some(entry) = entry {
        Some(
            functions
                .iter()
                .find(|function| function.id == entry)
                .copied()
                .ok_or(MirWebError::MissingEntryFunction { function: entry.0 })?,
        )
    } else {
        None
    };
    let entry_function = entry_row.filter(|function| function_in_bucket(function, WebBucket::JS));
    let entry_wasm = entry_row.filter(|function| function_in_bucket(function, WebBucket::Wasm));
    let main_keyword = if entry_function.is_some_and(|function| function.generator.is_some()) {
        "export function*"
    } else {
        "export async function"
    };
    writeln!(out, "\n{} jet_main(...args) {{", main_keyword).unwrap();
    out.push_str("  try {\n");
    if let Some(function) = entry_function {
        if function.generator.is_some() {
            writeln!(
                out,
                "    const __jet_edge_result = yield* jet_fn_{}([], args);",
                function.id.0
            )
            .unwrap();
        } else {
            writeln!(
                out,
                "    const __jet_edge_result = await jet_fn_{}([], args);",
                function.id.0
            )
            .unwrap();
        }
        out.push_str("    return jet_web_edge_result(__jet_edge_result);\n");
    } else if let Some(function) = entry_wasm {
        if function.generator.is_some() {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "Web Wasm entry {} cannot be a generator",
                    function.id.0
                ),
            });
        }
        writeln!(
            out,
            "    __jetPreludeWasm.jet_entry_{}();",
            function.id.0
        )
        .unwrap();
        out.push_str("    const outcome = jetDom.takeWasmError(__jetPreludeWasm);\n");
        out.push_str(
            "    if (outcome?.tag === \"Host\") throw jet_web_wasm_host_error(outcome, null, Number(outcome.status ?? 101));\n    if (outcome) return jet_web_edge_result(outcome);\n",
        );
        out.push_str("    return undefined;\n");
    } else {
        out.push_str("    return undefined;\n");
    }
    out.push_str(
        "  } catch (__jet_error) {\n\
         \
         if (__jet_error instanceof JetWebPropagation) {\n\
         \
         return jet_web_edge_result({ tag: \"Err\", error: __jet_error.wire });\n\
         \
         }\n\
         \
         throw __jet_error;\n\
         \
         }\n",
    );
    out.push_str("}\n\n");
    emit_js_routes(&mut out, program);
    let _ = target;
    Ok(out)
}

fn emit_web_module_registry(
    out: &mut String,
    program: &MirProgram,
    artifact: &MirArtifactPlan,
    artifact_identity: &str,
) -> Result<(), MirWebError> {
    let mut module_ids = artifact.modules.clone();
    module_ids.sort_by_key(|id| id.0);
    for module_id in module_ids {
        let module = program
            .modules
            .iter()
            .find(|module| module.id == module_id)
            .ok_or_else(|| MirWebError::InvalidMir {
                message: format!(
                    "Web artifact {} references missing module {}",
                    artifact.id.0, module_id.0
                ),
            })?;
        if module.key.is_empty() || module.path.is_empty() {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "Web artifact {} module {} has no stable key/path",
                    artifact.id.0, module_id.0
                ),
            });
        }
        let imports = module
            .imports
            .iter()
            .map(|id| id.0.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let item_order = module
            .item_order
            .iter()
            .map(|item| item_ref_descriptor(*item))
            .collect::<Vec<_>>()
            .join("\0");
        let interface_seed = format!(
            "{}\0{}\0{}\0{}",
            module.key, module.path, imports, item_order
        );
        let body_seed = format!(
            "{}\0{}\0{}",
            artifact_identity, module_id.0, module.path
        );
        let interface_fingerprint = format!("sha256-{}", sha256_hex(interface_seed.as_bytes()));
        let body_fingerprint = format!("sha256-{}", sha256_hex(body_seed.as_bytes()));
        writeln!(

            out,
            "jetDom.registerWebModule({}, {}, {});",
            json_string(&module.key),
            json_string(&interface_fingerprint),
            json_string(&body_fingerprint),
        )
        .unwrap();
    }
    out.push('\n');
    Ok(())
}

fn validate_function_blocks(function: &MirFunction) -> Result<(), MirWebError> {
    let has_block = |id: jet_foundation::MIR::MirBlockId| {
        function.blocks.iter().any(|block| block.id == id)
    };
    if !has_block(function.entry) {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "Web MIR function {} has a missing entry block {}",
                function.id.0, function.entry.0
            ),
        });
    }
    for block in &function.blocks {
        for target in block.terminator.targets() {
            if !has_block(target) {
                return Err(MirWebError::InvalidMir {
                    message: format!(
                        "Web MIR function {} block {} targets missing block {}",
                        function.id.0, block.id.0, target.0
                    ),
                });
            }
        }
    }
    Ok(())
}

fn validate_capture_params(
    program: &MirProgram,
    function: &MirFunction,
) -> Result<(), MirWebError> {
    for (slot, capture) in function.capture_params.iter().enumerate() {
        if capture.slot != slot {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "MIR function {} capture slot {} is out of order",
                    function.id.0, capture.slot
                ),
            });
        }
        if let Some(type_id) = capture.ty.identity {
            ensure_type_instance(program, type_id)?;
        }
    }
    Ok(())
}
fn emit_js_function(
    out: &mut String,
    program: &MirProgram,
    function: &MirFunction,
    live_values_enabled: bool,
    history_provenance: &HistoryProvenance,
) -> Result<(), MirWebError> {
    validate_function_blocks(function)?;
    validate_capture_params(program, function)?;
    if let Some(generator) = &function.generator {
        if let Some(type_id) = generator.item.identity {
            ensure_type_instance(program, type_id)?;
        }
    }
    let (source_file, source_line, source_text) = js_function_stack_context(program, function)?;
    let function_name = js_string(&function.name);
    let function_keyword = if function.generator.is_some() {
        "function*"
    } else if web_function_is_async(program, function.id) {
        "async function"
    } else {
        "function"
    };
    writeln!(out, "{} jet_fn_{}(__jet_env, __jet_args) {{", function_keyword, function.id.0).unwrap();
    writeln!(
        out,
        "  const __jet_stack_frame = jet_stack_enter({}, {}, {}, {});",
        source_file, source_line, function_name, source_text
    )
    .unwrap();
    out.push_str("  try {\n");
    out.push_str("  const __jet_values = new Map();\n  const __jet_cells = new Map();\n  const __jet_places = new Map();\n  const __jet_moved = Symbol.for(\"jet.moved\");\n  let __jet_break_value;\n");
    for place in &function.places {
        if matches!(&place.base, MirPlaceBase::Local(_)) {
            writeln!(out, "  __jet_places.set({}, {{ value: undefined }});", place.id.0).unwrap();
        }
    }
    writeln!(out, "  let __jet_block = {};\n  let __jet_pred = 0;", function.entry.0).unwrap();
    out.push_str("  for (;;) {\n    switch (__jet_block) {\n");
    let mut blocks = function.blocks.iter().collect::<Vec<_>>();
    blocks.sort_by_key(|block| block.id);
    for block in blocks {
        writeln!(out, "      case {}: {{", block.id.0).unwrap();
        for instruction in &block.instructions {
            let expression = js_operation_expression(
                program,
                function,
                &instruction.operation,
                instruction.ty.as_ref(),
                history_provenance,
            )?;
            if let Some(result) = instruction.result {
                writeln!(out, "        __jet_values.set({}, {});", result.0, expression).unwrap();
                if let MirOperation::Capture { slot } = &instruction.operation {
                    let cell = match function.capture_params[*slot].access {
                        MirAccess::Read | MirAccess::Write => format!("__jet_env[{slot}]"),
                        MirAccess::Move => format!(
                            "{{ get value() {{ return __jet_env[{slot}]; }}, set value(value) {{ __jet_env[{slot}] = value; }} }}"
                        ),
                    };
                    writeln!(out, "        __jet_cells.set({}, {});", result.0, cell).unwrap();
                } else if let MirOperation::Parameter { index, .. } = &instruction.operation {
                    if function.places.iter().any(|place| match &place.base {
                        MirPlaceBase::Parameter(value)
                        | MirPlaceBase::Capture(value)
                        | MirPlaceBase::Temporary(value) => value == &result,
                        MirPlaceBase::Local(_) | MirPlaceBase::Static(_) => false,
                    }) {
                        let param = function
                            .params
                            .iter()
                            .find(|param| param.index == *index)
                            .ok_or_else(|| MirWebError::InvalidMir {
                                message: format!(
                                    "MIR parameter index {} is missing from function {}",
                                    index, function.id.0
                                ),
                            })?;
                        let cell = if param.access == MirAccess::Write {
                            format!(
                                "{{ get value() {{ return __jet_args[{index}]; }}, set value(value) {{ __jet_args[{index}] = value; }} }}"
                            )
                        } else {
                            format!("{{ value: __jet_values.get({}) }}", result.0)
                        };
                        writeln!(out, "        __jet_cells.set({}, {});", result.0, cell).unwrap();
                    }
                } else if function.places.iter().any(|place| match &place.base {
                    MirPlaceBase::Parameter(value)
                    | MirPlaceBase::Capture(value)
                    | MirPlaceBase::Temporary(value) => value == &result,
                    MirPlaceBase::Local(_) | MirPlaceBase::Static(_) => false,
                }) {
                    writeln!(out, "        __jet_cells.set({}, {{ value: __jet_values.get({}) }});", result.0, result.0).unwrap();
                }
            } else {
                writeln!(out, "        void ({});", expression).unwrap();
            }
            if let Some(update) =
                js_persist_live_value_update(program, function, &instruction.operation, live_values_enabled)?
            {
                writeln!(out, "        {update}").unwrap();
            }
        }
        emit_js_terminator(out, function, block)?;

        out.push_str("      }\n");
    }
    out.push_str(
        "      default: throw new Error(\"invalid MIR block\");\n\
         \
         }\n\
         \
         }\n\
         \
         } finally {\n\
         \
         jetDom.exitRenderScope();\n\
         \
         jet_stack_leave(__jet_stack_frame);\n\
         \
         }\n\
         \
         }\n",
    );
    Ok(())
}
fn js_persist_type_renderable(ty: &MirType) -> bool {
    match ty.kind() {
        MirTypeKind::Int
        | MirTypeKind::Float
        | MirTypeKind::Bool
        | MirTypeKind::String
        | MirTypeKind::Char
        | MirTypeKind::IntN { .. }
        | MirTypeKind::Float32
        | MirTypeKind::Measure(_) => true,
        MirTypeKind::InlineRange { base, .. }
        | MirTypeKind::Tagged { inner: base, .. }
        | MirTypeKind::Quantity { base, .. } => js_persist_type_renderable(base),
        _ => false,
    }
}

fn js_persist_live_value_update(
    program: &MirProgram,
    function: &MirFunction,
    operation: &MirOperation,
    live_values_enabled: bool,
) -> Result<Option<String>, MirWebError> {
    if !live_values_enabled {
        return Ok(None);
    }
    let MirOperation::WritePlace { place, .. } = operation else {
        return Ok(None);
    };
    let place = function
        .places
        .iter()
        .find(|candidate| candidate.id == *place)
        .ok_or_else(|| MirWebError::InvalidMir {
            message: format!("Web MIR place {} is missing", place.0),
        })?;
    let Some(key) = place.persist_key.as_deref() else {
        return Ok(None);
    };
    let rendered = if js_persist_type_renderable(&place.ty) {
        format!(
            "String({})",
            js_read_place_expression(program, function, place.id)?
        )
    } else {
        "null".to_string()
    };
    Ok(Some(format!(
        "jetDom.publishLiveValueUpdate({}, {}, {});",
        js_string(key),
        js_string(&place.ty.canonical_key()),
        rendered,
    )))
}

fn js_move_place_expression(
    program: &MirProgram,
    function: &MirFunction,
    place_id: jet_foundation::MIR::MirPlaceId,
) -> Result<String, MirWebError> {
    let place = function
        .places
        .iter()
        .find(|candidate| candidate.id == place_id)
        .ok_or_else(|| MirWebError::InvalidMir {
            message: format!("Web MIR place {} is missing", place_id.0),
        })?;
    if place.access != MirAccess::Move {
        return Err(MirWebError::InvalidMir {
            message: format!("Web MIR MovePlace {} does not have move access", place_id.0),
        });
    }
    let cell = js_place_cell_expression(program, function, place_id)?;
    Ok(format!(
        "(() => {{ const cell = {cell}; const moved = cell.value; cell.value = __jet_moved; return moved; }})()"
    ))
}

fn js_move_value_expression(value: jet_foundation::MIR::MirValueId) -> String {
    format!(
        "(() => {{ const moved = __jet_values.get({}); __jet_values.set({}, __jet_moved); const cell = __jet_cells.get({}); if (cell) cell.value = __jet_moved; return moved; }})()",
        value.0, value.0, value.0,
    )
}

fn js_value_transfer_expression(
    program: &MirProgram,
    function: &MirFunction,
    value: jet_foundation::MIR::MirValueId,
) -> Result<String, MirWebError> {
    let ownership = function
        .values
        .iter()
        .find(|(id, _, _, _)| *id == value)
        .map(|(_, _, _, ownership)| ownership.mode)
        .ok_or_else(|| MirWebError::InvalidMir {
            message: format!(
                "MIR value {} is missing from function {}",
                value.0, function.id.0
            ),
        })?;
    Ok(match ownership {
        MirOwnershipMode::Owned | MirOwnershipMode::Move => js_move_value_expression(value),
        MirOwnershipMode::Copy
        | MirOwnershipMode::Shared
        | MirOwnershipMode::ReadBorrow
        | MirOwnershipMode::WriteBorrow => {
            let ty = mir_function_value_type(function, value)?;
            js_copy_expression(program, ty, format!("__jet_values.get({})", value.0))
        }
    })
}

fn js_local_place_id(
    function: &MirFunction,
    local: jet_foundation::MIR::MirLocalId,
) -> Result<jet_foundation::MIR::MirPlaceId, MirWebError> {
    function
        .locals
        .iter()
        .find(|candidate| candidate.id == local)
        .map(|candidate| candidate.place)
        .ok_or_else(|| MirWebError::InvalidMir {
            message: format!("Web MIR local {} has no place", local.0),
        })
}

fn js_place_source(base: &MirPlaceBase) -> Option<jet_foundation::MIR::MirValueId> {
    match base {
        MirPlaceBase::Parameter(value)
        | MirPlaceBase::Capture(value)
        | MirPlaceBase::Temporary(value) => Some(*value),
        MirPlaceBase::Local(_) | MirPlaceBase::Static(_) => None,
    }
}

fn js_place_root_value_expression(
    program: &MirProgram,
    function: &MirFunction,
    place: &jet_foundation::MIR::MirPlace,
) -> Result<String, MirWebError> {
    match &place.base {
        MirPlaceBase::Parameter(value)
        | MirPlaceBase::Capture(value)
        | MirPlaceBase::Temporary(value) => Ok(format!("__jet_cells.get({})?.value", value.0)),
        MirPlaceBase::Local(local) => {
            let local_place = js_local_place_id(function, *local)?;
            Ok(format!("__jet_places.get({})?.value", local_place.0))
        }
        MirPlaceBase::Static(name) => {
            let _ = program;
            Ok(format!("globalThis[{}]", js_string(name)))
        }
    }
}

fn js_place_cell_expression(
    program: &MirProgram,
    function: &MirFunction,
    place_id: jet_foundation::MIR::MirPlaceId,
) -> Result<String, MirWebError> {
    let Some(place) = function.places.iter().find(|place| place.id == place_id) else {
        return Err(MirWebError::InvalidMir {
            message: format!("Web MIR place {} is missing", place_id.0),
        });
    };
    if place.projections.is_empty() {
        if let Some(source) = js_place_source(&place.base) {
            return Ok(format!("__jet_cells.get({})", source.0));
        }
        if let MirPlaceBase::Local(local) = &place.base {
            let local_place = js_local_place_id(function, *local)?;
            return Ok(format!("__jet_places.get({})", local_place.0));
        }
    }
    let read = js_read_place_storage_expression(program, function, place_id)?;
    let write = js_write_place_expression(program, function, place_id, "value")?;
    Ok(format!(
        "(() => {{ const cell = {{}}; Object.defineProperty(cell, \"value\", {{ get: () => {}, set: value => {{ void {}; }} }}); return cell; }})()",
        read, write
    ))
}

fn js_read_place_expression(
    program: &MirProgram,
    function: &MirFunction,
    place_id: jet_foundation::MIR::MirPlaceId,
) -> Result<String, MirWebError> {
    let place = function
        .places
        .iter()
        .find(|place| place.id == place_id)
        .ok_or_else(|| MirWebError::InvalidMir {
            message: format!("Web MIR place {} is missing", place_id.0),
        })?;
    let storage = js_read_place_storage_expression(program, function, place_id)?;
    Ok(js_copy_expression(program, &place.ty, storage))
}

fn js_read_place_storage_expression(
    program: &MirProgram,
    function: &MirFunction,
    place_id: jet_foundation::MIR::MirPlaceId,
) -> Result<String, MirWebError> {
    let Some(place) = function.places.iter().find(|place| place.id == place_id) else {
        return Err(MirWebError::InvalidMir {
            message: format!("Web MIR place {} is missing", place_id.0),
        });
    };
    let mut expression = js_place_root_value_expression(program, function, place)?;
    for projection in &place.projections {
        match projection {
            MirProjection::Field { field, .. } => {
                expression = format!("{}[{}]", expression, js_string(&js_field_name(program, *field)?));
            }
            MirProjection::Index {
                call,
                index,
                location,
                context,
                ..
            } => {
                expression = js_index_expression(
                    program,
                    function,
                    *call,
                    expression,
                    format!("__jet_values.get({})", index.0),
                    *location,
                    context.as_ref(),
                )?;
            }
            MirProjection::Deref { .. } => {
                expression = format!("({expression}).value");
            }
        }
    }
    if place.projections.is_empty() {
        Ok(expression)
    } else {
        Ok(format!(
            "__jet_places.has({}) ? __jet_places.get({})?.value : {}",
            place_id.0, place_id.0, expression
        ))
    }
}

fn js_write_place_expression(
    program: &MirProgram,
    function: &MirFunction,
    place_id: jet_foundation::MIR::MirPlaceId,
    expression: &str,
) -> Result<String, MirWebError> {
    let Some(place) = function.places.iter().find(|place| place.id == place_id) else {
        return Err(MirWebError::InvalidMir {
            message: format!("Web MIR place {} is missing", place_id.0),
        });
    };
    if place.projections.is_empty() {
        if let Some(source) = js_place_source(&place.base) {
            return Ok(format!(
                "(__jet_cells.get({}).value = {}, __jet_values.set({}, {}), undefined)",
                source.0, expression, source.0, expression
            ));
        }
        if let MirPlaceBase::Local(local) = &place.base {
            let local_place = js_local_place_id(function, *local)?;
            return Ok(format!(
                "(__jet_places.get({}).value = {}, undefined)",
                local_place.0, expression
            ));
        }
        if let MirPlaceBase::Static(name) = &place.base {
            return Ok(format!(
                "(globalThis[{}] = {}, undefined)",
                js_string(name), expression
            ));
        }
    }
    let target = js_place_root_value_expression(program, function, place)?;
    js_write_projected_place_expression(program, function, target, &place.projections, expression)
}

fn js_write_projected_place_expression(
    program: &MirProgram,
    function: &MirFunction,
    target: String,
    projections: &[MirProjection],
    expression: &str,
) -> Result<String, MirWebError> {
    let Some((projection, rest)) = projections.split_first() else {
        return Ok(format!("({}, undefined)", format!("{target} = {expression}")));
    };
    match projection {
        MirProjection::Field { field, .. } => js_write_projected_place_expression(
            program,
            function,
            format!("{}[{}]", target, js_string(&js_field_name(program, *field)?)),
            rest,
            expression,
        ),
        MirProjection::Index {
            index,
            call,
            write_call,
            location,
            context,
            ..
        } => {
            let Some(write_call) = write_call else {
                return Err(MirWebError::InvalidMir {
                    message: "Web write index projection has no exact setter Prelude row".to_string(),
                });
            };
            let index = format!("__jet_values.get({})", index.0);
            if rest.is_empty() {
                return js_index_write_expression(
                    program,
                    *write_call,
                    target,
                    index,
                    expression.to_string(),
                    *location,
                    context.as_ref(),
                );
            }
            let read = js_index_expression(
                program,
                function,
                *call,
                "__jet_index_base".to_string(),
                index.clone(),
                *location,
                context.as_ref(),
            )?;
            let nested = js_write_projected_place_expression(
                program,
                function,
                "__jet_index_value".to_string(),
                rest,
                expression,
            )?;
            let setter = js_index_write_expression(
                program,
                *write_call,
                "__jet_index_base".to_string(),
                index,
                "__jet_index_value".to_string(),
                *location,
                context.as_ref(),
            )?;
            Ok(format!(
                "(() => {{ const __jet_index_base = {target}; const __jet_index_value = {read}; void ({nested}); return {setter}; }})()"
            ))
        }
        MirProjection::Deref { .. } => js_write_projected_place_expression(
            program,
            function,
            format!("({target}).value"),
            rest,
            expression,
        ),
    }
}

fn js_index_write_expression(
    program: &MirProgram,
    call: MirPreludeCallId,
    base: String,
    index: String,
    written: String,
    location: jet_foundation::MIR::MirPanicLoc,
    context: Option<&jet_foundation::MIR::MirPanicContext>,
) -> Result<String, MirWebError> {
    let Some(route) = program.prelude_calls.iter().find(|route| route.id == call) else {
        return Err(MirWebError::MissingPreludeCall { call: call.0 });
    };
    if route.signature.max_arity != route.signature.arity {
        return Err(MirWebError::InvalidMir {
            message: format!("MIR indexed setter Prelude call {} does not have an exact arity", call.0),
        });
    }
    let mut args = vec![base, index, written];
    match route.signature.arity {
        3 => {}
        5 => {
            args.push(js_source_file_path(program, location.file)?);
            args.push(location.line.to_string());
        }
        7 => {
            let Some(context) = context else {
                return Err(MirWebError::InvalidMir {
                    message: format!(
                        "MIR indexed setter Prelude call {} requires checked context",
                        call.0
                    ),
                });
            };
            args.push(js_source_file_path(program, location.file)?);
            args.push(location.line.to_string());
            args.push(js_string(&context.function));
            args.push(js_string(&context.source_line));
        }
        arity => {
            return Err(MirWebError::InvalidMir {
                message: format!("MIR indexed setter Prelude call {} has unsupported arity {}", call.0, arity),
            });
        }
    }
    js_prelude_call_expression(program, call, &args)
}
fn js_index_context(
    _program: &MirProgram,
    _function: &MirFunction,
    _location: jet_foundation::MIR::MirPanicLoc,
    context: Option<&jet_foundation::MIR::MirPanicContext>,
) -> Result<(String, String), MirWebError> {
    let Some(context) = context else {
        return Err(MirWebError::InvalidMir {
            message: "MIR index Prelude call requires checked context".to_string(),
        });
    };
    Ok((
        js_string(&context.function),
        js_string(&context.source_line),
    ))
}

fn js_index_expression(
    program: &MirProgram,
    function: &MirFunction,
    call: MirPreludeCallId,
    base: String,
    index: String,
    location: jet_foundation::MIR::MirPanicLoc,
    context: Option<&jet_foundation::MIR::MirPanicContext>,
) -> Result<String, MirWebError> {
    let Some(route) = program.prelude_calls.iter().find(|route| route.id == call) else {
        return Err(MirWebError::MissingPreludeCall { call: call.0 });
    };
    if route.signature.max_arity != route.signature.arity {
        return Err(MirWebError::InvalidMir {
            message: format!("MIR index Prelude call {} does not have an exact arity", call.0),
        });
    }
    let mut args = vec![
        base,
        index,
        js_source_file_path(program, location.file)?,
        location.line.to_string(),
    ];
    match route.signature.arity {
        4 => {}
        6 => {
            let (function_name, source_line) = js_index_context(program, function, location, context)?;
            args.push(function_name);
            args.push(source_line);
        }
        8 => {
            let (function_name, source_line) = js_index_context(program, function, location, context)?;
            let Some(context) = context else {
                return Err(MirWebError::InvalidMir {
                    message: format!("MIR index Prelude call {} requires checked context", call.0),
                });
            };
            args.push(function_name);
            args.push(source_line);
            args.push(location.column.to_string());
            args.push(context.caret.to_string());
        }
        arity => {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "MIR index Prelude call {} has unsupported arity {arity}",
                    call.0
                ),
            });
        }
    }
    js_prelude_call_expression(program, call, &args)
}


fn js_capture_rvalue_expression(
    function: &MirFunction,
    slot: usize,
) -> Result<String, MirWebError> {
    let Some(capture) = function.capture_params.get(slot) else {
        return Err(MirWebError::InvalidMir {
            message: format!("MIR capture slot {} is missing", slot),
        });
    };
    Ok(match capture.access {
        MirAccess::Read | MirAccess::Write => format!("__jet_env[{}].value", slot),
        MirAccess::Move => format!("__jet_env[{}]", slot),
    })
}

fn js_operation_expression(
    program: &MirProgram,
    function: &MirFunction,
    operation: &MirOperation,
    result_type: Option<&MirType>,
    history_provenance: &HistoryProvenance,
) -> Result<String, MirWebError> {
    let value = |id: jet_foundation::MIR::MirValueId| format!("__jet_values.get({})", id.0);
    let expression = match operation {
        MirOperation::Parameter { index, .. } => {
            let param = function
                .params
                .iter()
                .find(|param| param.index == *index)
                .ok_or_else(|| MirWebError::InvalidMir {
                    message: format!(
                        "MIR parameter index {} is missing from function {}",
                        index, function.id.0
                    ),
                })?;
            let expression = format!("__jet_args[{}]", index);
            match param.access {
                MirAccess::Move => expression,
                MirAccess::Read | MirAccess::Write => result_type
                    .map(|ty| js_copy_expression(program, ty, expression.clone()))
                    .unwrap_or(expression),
            }
        }
        MirOperation::Capture { slot } => {
            let capture = function
                .capture_params
                .get(*slot)
                .ok_or_else(|| MirWebError::InvalidMir {
                    message: format!("MIR capture slot {} is missing", slot),
                })?;
            let expression = js_capture_rvalue_expression(function, *slot)?;
            match capture.access {
                MirAccess::Move => expression,
                MirAccess::Read | MirAccess::Write => result_type
                    .map(|ty| js_copy_expression(program, ty, expression.clone()))
                    .unwrap_or(expression),
            }
        }
        MirOperation::Global { name } => format!("globalThis[{}]", js_string(name)),
        MirOperation::Phi { incoming } => {
            js_phi_expression(program, function, incoming)?
        }
        MirOperation::ReadPlace(place) => js_read_place_expression(program, function, *place)?,
        MirOperation::MovePlace { place } => js_move_place_expression(program, function, *place)?,
        MirOperation::InitializeUninit { place } => {
            let place_row = function
                .places
                .iter()
                .find(|candidate| candidate.id == *place)
                .ok_or_else(|| MirWebError::InvalidMir {
                    message: format!("Web MIR place {} is missing", place.0),
                })?;
            let expression = match place_row.ty.kind() {
                MirTypeKind::FixedList { len, .. } => {
                    let len = len.literal_value().ok_or_else(|| MirWebError::InvalidMir {
                        message: format!(
                            "Web MIR uninitialized fixed-list place {} has no static length",
                            place.0
                        ),
                    })?;
                    format!("Array({len}).fill(undefined)")
                }
                _ => "undefined".to_string(),
            };
            js_write_place_expression(program, function, *place, &expression)?
        }
        MirOperation::WritePlace { place, value: id } => {
            let transferred = js_value_transfer_expression(program, function, *id)?;
            js_write_place_expression(program, function, *place, &transferred)?
        }
        MirOperation::Copy { value: id } => {
            let ty = mir_function_value_type(function, *id)?;
            js_copy_expression(program, ty, value(*id))
        }
        MirOperation::Move { value: id } => js_move_value_expression(*id),
        MirOperation::Constant(constant) => js_constant_expression(program, constant)?,
        MirOperation::Unary { op, value: id } => js_unary_expression(*op, &value(*id)),
        MirOperation::Binary { op, dispatch, left, right } => {
            let mut args = vec![value(*left), value(*right)];
            if let MirBinaryDispatch::Prelude { location: Some(location), .. } = dispatch {
                args.push(js_source_file_path(program, location.file)?);
                args.push(location.line.to_string());
            }
            match dispatch {
                MirBinaryDispatch::Primitive => js_binary_expression(*op, &value(*left), &value(*right)),
                MirBinaryDispatch::Prelude { call, .. } => {
                    js_prelude_call_expression(program, *call, &args)?
                }
            }
        }
        MirOperation::BuildString { parts } => format!(
            "[{}].join(\"\")",
            parts.iter().map(js_string_part).collect::<Vec<_>>().join(", ")
        ),
        MirOperation::BuildList { values: ids } => format!(
            "[{}]",
            ids.iter()
                .map(|id| js_move_value_expression(*id))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        MirOperation::BuildMap { entries } => format!(
            "new Map([{}])",
            entries
                .iter()
                .map(|(key, id)| format!(
                    "[{}, {}]",
                    js_move_value_expression(*key),
                    js_move_value_expression(*id)
                ))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        MirOperation::EnumIs { subject, owner, variant } => {
            ensure_enum_variant(program, *owner, variant, None)?;
            format!("{}?.tag === {}", value(*subject), js_string(variant))
        }
        MirOperation::EnumPayload { subject, owner, variant, index } => {
            ensure_enum_variant(program, *owner, variant, Some(*index))?;
            let expression = format!(
                "{}?.tag === {} ? {}?.values[{}] : undefined",
                value(*subject),
                js_string(variant),
                value(*subject),
                index
            );
            if let Some(ty) = result_type {
                js_copy_expression(program, ty, expression)
            } else {
                expression
            }
        }
        MirOperation::OptionIsSome { subject } => {
            format!("{}?.tag === \"Ok\"", value(*subject))
        }
        MirOperation::OptionValue { subject } => {
            format!(
                "(() => {{ const carrier = {}; if (carrier?.tag !== \"Ok\") throw new Error(\"MIR option payload missing\"); return carrier.values[0]; }})()",
                js_move_value_expression(*subject)
            )
        }
        MirOperation::ResultIsOk { subject } => {
            format!("{}?.tag === \"Ok\"", value(*subject))
        }
        MirOperation::ResultValue { subject, ok } => format!(
            "(() => {{ const carrier = {}; if (carrier?.tag !== {}) throw new Error({}); return carrier.values[0]; }})()",
            js_move_value_expression(*subject),
            js_string(if *ok { "Ok" } else { "Err" }),
            js_string(if *ok {
                "MIR result success payload missing"
            } else {
                "MIR result error payload missing"
            }),
        ),
        MirOperation::PatternCapture { matched, index } => {
            let expression = format!(
                "{}?.tag === \"Ok\" ? {}?.values?.[0]?.[{}] : undefined",
                value(*matched),
                value(*matched),
                index
            );
            if let Some(ty) = result_type {
                js_copy_expression(program, ty, expression)
            } else {
                expression
            }
        }
        MirOperation::PatternMatched { matched } => {
            format!("{}?.tag === \"Ok\"", value(*matched))
        }
        MirOperation::ProjectMembers { base, members } => {
            let mut expression = value(*base);
            for field in members {
                expression = format!("{}[{}]", expression, js_string(&js_field_name(program, *field)?));
            }
            if let Some(ty) = result_type {
                js_copy_expression(program, ty, expression)
            } else {
                expression
            }
        }
        MirOperation::Index { call, base, index, location, context, .. } => js_index_expression(
            program,
            function,
            *call,
            value(*base),
            value(*index),
            *location,
            Some(context),
        )?,
        MirOperation::Slice { call, base, start, end, range, location, .. } => {
            let Some(route) = program.prelude_calls.iter().find(|route| route.id == *call) else {
                return Err(MirWebError::MissingPreludeCall { call: call.0 });
            };
            if route.signature.max_arity != route.signature.arity {
                return Err(MirWebError::InvalidMir {
                    message: format!("MIR slice Prelude call {} does not have an exact arity", call.0),
                });
            }
            let args = match (route.signature.arity, range) {
                (4, Some(range)) => vec![
                    value(*base),
                    value(*range),
                    js_source_file_path(program, location.file)?,
                    location.line.to_string(),
                ],
                (5, None) => vec![
                    value(*base),
                    value(*start),
                    value(*end),
                    js_source_file_path(program, location.file)?,
                    location.line.to_string(),
                ],
                (arity, range) => {
                    return Err(MirWebError::InvalidMir {
                        message: format!(
                            "MIR slice Prelude call {} has arity {} for {} range form",
                            call.0,
                            arity,
                            if range.is_some() { "a" } else { "a direct" }
                        ),
                    });
                }
            };
            js_prelude_call_expression(program, *call, &args)?
        }
        MirOperation::Range { start, end, exclusive } => format!(
            "{{ start: {}, end: {}, exclusive: {} }}",
            value(*start),
            value(*end),
            exclusive
        ),
        MirOperation::Field { base, field } => {
            let expression = format!(
                "{}[{}]",
                value(*base),
                js_string(&js_field_name(program, *field)?)
            );
            if let Some(ty) = result_type {
                js_copy_expression(program, ty, expression)
            } else {
                expression
            }
        }
        MirOperation::Struct { type_id, fields } | MirOperation::Tuple { type_id, fields } => {
            ensure_type_instance(program, *type_id)?;
            let fields = fields
                .iter()
                .map(|(field, id)| {
                    Ok(format!(
                        "{}: {}",
                        js_string(&js_field_name(program, *field)?),
                        js_move_value_expression(*id)
                    ))
                })
                .collect::<Result<Vec<_>, MirWebError>>()?
                .join(", ");
            format!(
                "Object.defineProperty({{ {} }}, \"__jet_type\", {{ value: {}, enumerable: false }})",
                fields,
                js_string(&type_id.0.to_string())
            )
        }
        MirOperation::Enum { type_id, variant, args } => {
            ensure_enum_variant(program, *type_id, variant, None)?;
            let Some(type_def) = program.types.iter().find(|type_def| type_def.id == *type_id) else {
                return Err(MirWebError::InvalidMir {
                    message: format!("MIR enum type definition {} is missing", type_id.0),
                });
            };
            let MirTypeDefKind::Enum { variants, .. } = &type_def.kind else {
                return Err(MirWebError::InvalidMir {
                    message: format!("MIR enum operation owner {} is not an enum", type_id.0),
                });
            };
            let Some(declared) = variants.iter().find(|candidate| candidate.name == *variant) else {
                return Err(MirWebError::InvalidMir {
                    message: format!("MIR enum {} has no variant {}", type_id.0, variant),
                });
            };
            match &declared.payload {
                MirVariantPayload::Unit if !args.is_empty() => {
                    return Err(MirWebError::InvalidMir {
                        message: format!("MIR unit enum variant {} has payload arguments", variant),
                    });
                }
                MirVariantPayload::Single(_) if args.len() != 1 || args[0].field.is_some() => {
                    return Err(MirWebError::InvalidMir {
                        message: format!("MIR single-payload enum variant {} has invalid arguments", variant),
                    });
                }
                MirVariantPayload::Named(fields) => {
                    if args.len() != fields.len()
                        || args
                            .iter()
                            .zip(fields)
                            .any(|(arg, field)| arg.field != Some(field.id))
                    {
                        return Err(MirWebError::InvalidMir {
                            message: format!(
                                "MIR named enum variant {} arguments are not in canonical field order",
                                variant
                            ),
                        });
                    }
                }
                MirVariantPayload::Unit | MirVariantPayload::Single(_) => {}
            }
            format!(
                "Object.defineProperty({{ tag: {}, values: [{}] }}, \"__jet_type\", {{ value: {}, enumerable: false }})",
                js_string(variant),
                args.iter()
                    .map(|arg| js_move_value_expression(arg.value))
                    .collect::<Vec<_>>()
                    .join(", "),
                js_string(&type_id.0.to_string())
            )
        }
        MirOperation::Present { value: id } => {
            format!("jet_option_some({})", js_move_value_expression(*id))
        }
        MirOperation::Absent => "jet_absent()".to_string(),
        MirOperation::ResultOk { value: id } => {
            format!("jet_outcome_ok({})", js_move_value_expression(*id))
        }
        MirOperation::ResultErr { value: id } => {
            format!("jet_outcome_err({})", js_move_value_expression(*id))
        }
        MirOperation::Call { callee, args, type_args } => {
            js_call_expression(program, function, callee, args, type_args, history_provenance)?
        }
        MirOperation::CoreCall { call, route, args, type_args, data_plan: _, .. } => {
            let expression = js_core_call_expression(
                program,
                function,
                *call,
                Some(*route),
                args,
                type_args,
                history_provenance,
            )?;
            if function.generator.is_none() && web_model_core_call(program, *call) {
                format!("await ({expression})")
            } else {
                expression
            }
        }
        MirOperation::IndirectCall { callee, args, type_args } => {
            validate_type_args(program, type_args)?;
            format!(
                "{}({})",
                value(*callee),
                js_call_values(program, function, args, true)?.join(", ")
            )
        }
        MirOperation::Closure { function: function_id, captures, .. } => {
            js_closure_expression(program, function, *function_id, captures, history_provenance)?
        }
        MirOperation::PtrFromAddr { addr, .. } => {
            let expression = value(*addr);
            result_type
                .map(|ty| js_copy_expression(program, ty, expression.clone()))
                .unwrap_or(expression)
        }
        MirOperation::Deref { value: id } => {
            let expression = format!("({}).value", value(*id));
            result_type
                .map(|ty| js_copy_expression(program, ty, expression.clone()))
                .unwrap_or(expression)
        }
        MirOperation::RawAddressOf { place } => {
            js_place_cell_expression(program, function, *place)?
        }
        MirOperation::AddressOf { place, access } => match access {
            MirAccess::Read => js_read_place_expression(program, function, *place)?,
            MirAccess::Write | MirAccess::Move => js_place_cell_expression(program, function, *place)?,
        },
        MirOperation::Convert { value: id, parameters, target, conversion } => {
            js_conversion_expression(program, *id, parameters, target, conversion)?
        }
        MirOperation::AttachTag { value: id, tag: _ } => js_move_value_expression(*id),
        MirOperation::Todo { call, location, expected_type } => {
            if let Some(ty) = expected_type {
                if let Some(type_id) = ty.identity {
                    ensure_type_instance(program, type_id)?;
                }
            }
            let expected = expected_type
                .as_ref()
                .map(type_descriptor)
                .unwrap_or_else(|| "(unknown)".to_string());
            js_prelude_call_expression(
                program,
                *call,
                &[
                    js_source_file_path(program, location.file)?,
                    location.line.to_string(),
                    js_string(&expected),
                ],
            )?
        }
        MirOperation::Never { reason } => {
            format!("(() => {{ throw new Error({}); }})()", js_string(reason))
        }
        MirOperation::Semantic(semantic) => {
            js_semantic_expression(program, function, semantic, result_type)?
        }
        MirOperation::LoopRangeInit { call, start, end, step, exclusive, .. } => js_prelude_call_expression(
            program,
            *call,
            &[
                value(*start),
                value(*end),
                step.map(value).unwrap_or_else(|| "1".to_string()),
                exclusive.to_string(),
            ],
        )?,
        MirOperation::LoopRangeHasNext { call, cursor } => {
            js_prelude_call_expression(program, *call, &[value(*cursor)])?
        }
        MirOperation::LoopRangeValue { call, cursor } => {
            js_prelude_call_expression(program, *call, &[value(*cursor)])?
        }
        MirOperation::LoopRangeAdvance { call, cursor } => {
            js_prelude_call_expression(program, *call, &[value(*cursor)])?
        }
        MirOperation::LoopIterInit { call, collection, step, .. } => js_prelude_call_expression(
            program,
            *call,
            &[
                value(*collection),
                step.map(value).unwrap_or_else(|| "1".to_string()),
            ],
        )?,
        MirOperation::LoopIterHasNext { call, cursor } => {
            js_prelude_call_expression(program, *call, &[value(*cursor)])?
        }
        MirOperation::LoopIterValue { call, cursor } => {
            js_prelude_call_expression(program, *call, &[value(*cursor)])?
        }
        MirOperation::LoopIterAdvance { call, cursor } => {
            js_prelude_call_expression(program, *call, &[value(*cursor)])?
        }
        MirOperation::ScopeEnter { .. } | MirOperation::ScopeExit { .. } => {
            "undefined".to_string()
        }
        MirOperation::Drop { value: id, .. } => format!("void (__jet_values.get({}))", id.0),
    };
    if let Some(result_type) = result_type.filter(|ty| ty.is_integer()) {
        let (signed, bits) = if matches!(operation, MirOperation::Parameter { .. }) {
            result_type.fixed_int().unwrap_or((false, 0))
        } else {
            (false, 0)
        };
        Ok(format!(
            "jet_numeric_web_int_value({}, {}, {})",
            expression, signed, bits
        ))
    } else {
        Ok(expression)
    }
}
fn js_phi_expression(
    program: &MirProgram,
    function: &MirFunction,
    incoming: &[(jet_foundation::MIR::MirBlockId, jet_foundation::MIR::MirValueId)],
) -> Result<String, MirWebError> {
    let mut expression = "undefined".to_string();
    for (block, value) in incoming.iter().rev() {
        let transferred = js_value_transfer_expression(program, function, *value)?;
        expression = format!(
            "(__jet_pred === {} ? {} : {})",
            block.0, transferred, expression
        );
    }
    Ok(expression)
}

fn js_integer_constant(value: i64, width: Option<(bool, u8)>) -> String {
    let literal = match width {
        Some((false, _)) => (value as u64).to_string(),
        _ => value.to_string(),
    };
    let base = format!("BigInt({})", js_string(&literal));
    match width {
        Some((true, bits)) => format!("BigInt.asIntN({bits}, {base})"),
        Some((false, bits)) => format!("BigInt.asUintN({bits}, {base})"),
        None => base,
    }
}

fn js_constant_type_id(program: &MirProgram, type_name: &str) -> Option<jet_foundation::MIR::MirTypeId> {
    program
        .types
        .iter()
        .find(|type_def| type_def.name == type_name || type_def.key == type_name)
        .map(|type_def| type_def.id)
}

fn js_constant_expression(
    program: &MirProgram,
    constant: &MirConstant,
) -> Result<String, MirWebError> {
    match constant {
        MirConstant::Int { value, width, .. } => Ok(js_integer_constant(*value, *width)),
        MirConstant::Float { value, f32: is_f32, .. } => {
            if *is_f32 {
                Ok(format!("{:?}", *value as f32))
            } else {
                Ok(format!("{value:?}"))
            }
        }
        MirConstant::Bool(value) => Ok(value.to_string()),
        MirConstant::Char(value) => Ok(js_string(&value.to_string())),
        MirConstant::String(value) => Ok(js_string(value)),
        MirConstant::Bytes(values) => Ok(format!(
            "new Uint8Array([{}])",
            values.iter().map(|value| value.to_string()).collect::<Vec<_>>().join(", ")
        )),
        MirConstant::Unit => Ok("undefined".to_string()),
        MirConstant::BigInt(value) => Ok(format!("BigInt({})", js_string(value))),
        MirConstant::List(values) => Ok(format!(
            "[{}]",
            values
                .iter()
                .map(|value| js_constant_expression(program, value))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ")
        )),
        MirConstant::Map(values) => Ok(format!(
            "new Map([{}])",
            values
                .iter()
                .map(|(key, value)| Ok(format!(
                    "[{}, {}]",
                    js_const_key_expression(key)?,
                    js_constant_expression(program, value)?
                )))
                .collect::<Result<Vec<_>, MirWebError>>()?
                .join(", ")
        )),
        MirConstant::Struct { type_name, fields } => {
            let object = format!(
                "{{ {} }}",
                fields
                    .iter()
                    .map(|(name, value)| Ok(format!(
                        "{}: {}",
                        js_string(name),
                        js_constant_expression(program, value)?
                    )))
                    .collect::<Result<Vec<_>, MirWebError>>()?
                    .join(", ")
            );
            Ok(js_constant_type_id(program, type_name).map_or(object.clone(), |type_id| {
                format!(
                    "Object.defineProperty({}, \"__jet_type\", {{ value: {}, enumerable: false }})",
                    object,
                    js_string(&type_id.0.to_string())
                )
            }))
        }
        MirConstant::Enum {
            type_name,
            variant,
            args,
        } => {
            let object = format!(
                "{{ tag: {}, values: [{}] }}",
                js_string(variant),
                args.iter()
                    .map(|(_, value)| js_constant_expression(program, value))
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", ")
            );
            Ok(js_constant_type_id(program, type_name).map_or(object.clone(), |type_id| {
                format!(
                    "Object.defineProperty({}, \"__jet_type\", {{ value: {}, enumerable: false }})",
                    object,
                    js_string(&type_id.0.to_string())
                )
            }))
        }
        MirConstant::Present(value) => Ok(format!(
            "jet_option_some({})",
            js_constant_expression(program, value)?
        )),
        MirConstant::Failed(MirConstReport::Clean(_)) => {
            Ok("jet_absent()".to_string())
        }
        MirConstant::Failed(MirConstReport::Told(value)) => Ok(format!(
            "{{ tag: \"Err\", values: [{}] }}",
            js_constant_expression(program, value)?
        )),
    }
}

fn js_const_key_expression(key: &MirConstKey) -> Result<String, MirWebError> {
    match key {
        MirConstKey::Int(value) => Ok(format!("BigInt({})", js_string(&value.to_string()))),
        MirConstKey::String(value) => Ok(js_string(value)),
        MirConstKey::Bool(value) => Ok(value.to_string()),
        MirConstKey::Char(value) => Ok(js_string(&value.to_string())),
        MirConstKey::Tuple(values) => Ok(format!(
            "[{}]",
            values
                .iter()
                .map(|(_, key)| js_const_key_expression(key))
                .collect::<Result<Vec<_>, _>>()?
                .join(", ")
        )),
        MirConstKey::Struct { fields, .. } => Ok(format!(
            "{{ {} }}",
            fields
                .iter()
                .map(|(name, key)| Ok(format!(
                    "{}: {}",
                    js_string(name),
                    js_const_key_expression(key)?
                )))
                .collect::<Result<Vec<_>, MirWebError>>()?
                .join(", ")
        )),
        MirConstKey::Enum { variant, .. } => Ok(format!(
            "{{ tag: {}, values: [] }}",
            js_string(variant)
        )),
    }
}

fn js_unary_expression(op: MirUnaryOp, value: &str) -> String {
    match op {
        MirUnaryOp::Neg => format!("(-({value}))"),
        MirUnaryOp::Not => format!("(!({value}))"),
    }
}

fn js_binary_expression(op: MirBinaryOp, left: &str, right: &str) -> String {
    match op {
        MirBinaryOp::Add => format!("(({left}) + ({right}))"),
        MirBinaryOp::Sub => format!("(({left}) - ({right}))"),
        MirBinaryOp::Mul => format!("(({left}) * ({right}))"),
        MirBinaryOp::Div => format!("(({left}) / ({right}))"),
        MirBinaryOp::FloorDiv => format!("Math.floor(({left}) / ({right}))"),
        MirBinaryOp::Mod => format!("(((({left}) % ({right})) + ({right})) % ({right}))"),
        MirBinaryOp::Rem => format!("(({left}) % ({right}))"),
        MirBinaryOp::Pow => format!("(({left}) ** ({right}))"),
        MirBinaryOp::BitAnd => format!("(({left}) & ({right}))"),
        MirBinaryOp::BitOr => format!("(({left}) | ({right}))"),
        MirBinaryOp::BitXor => format!("(({left}) ^ ({right}))"),
        MirBinaryOp::Shl => format!("(({left}) << ({right}))"),
        MirBinaryOp::Shr => format!("(({left}) >> ({right}))"),
        MirBinaryOp::Eq => format!("(({left}) === ({right}))"),
        MirBinaryOp::Ne => format!("(({left}) !== ({right}))"),
        MirBinaryOp::Lt => format!("(({left}) < ({right}))"),
        MirBinaryOp::Gt => format!("(({left}) > ({right}))"),
        MirBinaryOp::Le => format!("(({left}) <= ({right}))"),
        MirBinaryOp::Ge => format!("(({left}) >= ({right}))"),
        MirBinaryOp::Compare => {
            format!("(({left}) < ({right}) ? -1 : (({left}) > ({right}) ? 1 : 0))")
        }
        MirBinaryOp::And => format!("(({left}) && ({right}))"),
        MirBinaryOp::Or => format!("(({left}) || ({right}))"),
    }
}
fn js_panic_locals(
    program: &MirProgram,
    function: &MirFunction,
    context: &jet_foundation::MIR::MirPanicContext,
) -> Result<String, MirWebError> {
    let rendered = context
        .locals
        .iter()
        .map(|(name, local)| {
            let place = js_local_place_id(function, *local)?;
            let value = js_read_place_expression(program, function, place)?;
            Ok(format!("{} + \" = \" + jet_debug({value})", js_string(name)))
        })
        .collect::<Result<Vec<_>, MirWebError>>()?;
    if rendered.is_empty() {
        Ok(js_string(""))
    } else {
        Ok(format!("[{}].join(\", \")", rendered.join(", ")))
    }
}


fn js_core_closure_call_expression(
    program: &MirProgram,
    call: MirPreludeCallId,
    kind: MirCoreClosureKind,
    values: &[jet_foundation::MIR::MirValueId],
    closure: Option<jet_foundation::MIR::MirValueId>,
    site: jet_foundation::MIR::MirSiteId,
    label: &str,
) -> Result<String, MirWebError> {
    let preview_source = match &kind {
        MirCoreClosureKind::UiPreview {
            source_file,
            source_start_line,
            source_start_column,
            source_end_line,
            source_end_column,
            build_id,
            revision,
            ..
        } => Some((
            source_file.clone(),
            *source_start_line,
            *source_start_column,
            *source_end_line,
            *source_end_column,
            build_id.clone(),
            revision.clone(),
        )),
        _ => None,
    };
    let value = |id: jet_foundation::MIR::MirValueId| format!("__jet_values.get({})", id.0);
    let required_closure = || {
        closure
            .map(value)
            .ok_or_else(|| MirWebError::InvalidMir {
                message: format!("MIR {kind:?} CoreClosureCall has no closure operand"),
            })
    };
    let no_closure = || {
        if closure.is_some() {
            Err(MirWebError::InvalidMir {
                message: format!("MIR {kind:?} CoreClosureCall has an unexpected closure operand"),
            })
        } else {
            Ok(())
        }
    };
    let wrong_arity = |expected: &str| {
        Err(MirWebError::InvalidMir {
            message: format!(
                "MIR {kind:?} CoreClosureCall has {} value operands; expected {expected}",
                values.len()
            ),
        })
    };
    let args = match &kind {
        MirCoreClosureKind::Spawn => {
            let closure = required_closure()?;
            match values {
                [] => vec![site.0.to_string(), js_string(label), closure],
                [group] => vec![value(*group), site.0.to_string(), js_string(label), closure],
                _ => return wrong_arity("zero or one (grouped)"),
            }
        }
        MirCoreClosureKind::Realtime => {
            let [rate, frames] = values else {
                return wrong_arity("two realtime parameters");
            };
            vec![value(*rate), value(*frames), required_closure()?]
        }
        MirCoreClosureKind::Serve => {
            let [address] = values else {
                return wrong_arity("one address");
            };
            vec![value(*address), required_closure()?]
        }
        MirCoreClosureKind::OnInterrupt => {
            no_closure()?;
            let [callback] = values else {
                return wrong_arity("one callback");
            };
            vec![value(*callback)]
        }
        MirCoreClosureKind::Guard => {
            if !values.is_empty() {
                return wrong_arity("no value operands");
            }
            vec![required_closure()?]
        }
        MirCoreClosureKind::OnCommit | MirCoreClosureKind::OnRollback => {
            let [handle] = values else {
                return wrong_arity("one transaction handle");
            };
            vec![value(*handle), required_closure()?]
        }
        MirCoreClosureKind::ReactiveDerived
        | MirCoreClosureKind::ReactiveEffect
        | MirCoreClosureKind::UiMount => {
            if !values.is_empty() {
                return wrong_arity("no value operands");
            }
            vec![required_closure()?]
        }
        MirCoreClosureKind::UiPreview { .. } => {
            let [name, viewport] = values else {
                return wrong_arity("name and viewport");
            };
            vec![value(*name), value(*viewport), required_closure()?]
        }
        MirCoreClosureKind::UiAction => {
            let [display, shortcut, accessible_label] = values else {
                return wrong_arity("display, shortcut, and accessible label");
            };
            vec![
                value(*display),
                value(*shortcut),
                value(*accessible_label),
                required_closure()?,
            ]
        }
        MirCoreClosureKind::UiTextInputOnDrop => {
            let [state, ime] = values else {
                return wrong_arity("state and IME mode");
            };
            vec![value(*state), value(*ime), required_closure()?]
        }
    };
    let emitted = js_prelude_call_expression(program, call, &args)?;
    if let Some((
        source_file,
        source_start_line,
        source_start_column,
        source_end_line,
        source_end_column,
        build_id,
        revision,
    )) = preview_source
    {
        let source_id = source_file.clone();
        Ok(format!(
            "jet_ui_preview_attach_compiler_source({emitted}, {}, {}, {}, {}, {}, {}, {}, {})",
            js_string(&source_id),
            js_string(&source_file),
            js_string(&build_id),
            js_string(&revision),
            source_start_line,
            source_start_column,
            source_end_line,
            source_end_column,
        ))
    } else {
        Ok(emitted)
    }
}



fn mir_function_value_type<'a>(
    function: &'a MirFunction,
    value: jet_foundation::MIR::MirValueId,
) -> Result<&'a MirType, MirWebError> {
    function
        .values
        .iter()
        .find(|(id, _, _, _)| *id == value)
        .map(|(_, ty, _, _)| ty)
        .ok_or_else(|| MirWebError::InvalidMir {
            message: format!(
                "MIR value {} is missing from function {}",
                value.0, function.id.0
            ),
        })
}

fn js_carrier_fact_expression(
    program: &MirProgram,
    call: MirPreludeCallId,
    receiver: jet_foundation::MIR::MirValueId,
    field: jet_foundation::MIR::MirFieldId,
    _notes: bool,
) -> Result<String, MirWebError> {
    if !program.fields.iter().any(|row| row.id == field) {
        return Err(MirWebError::InvalidMir {
            message: format!("MIR carrier fact field {} is missing", field.0),
        });
    }
    js_prelude_call_expression(
        program,
        call,
        &[format!("__jet_values.get({})", receiver.0)],
    )
}

fn js_gc_edit_expression(
    program: &MirProgram,
    call: MirPreludeCallId,
    root: jet_foundation::MIR::MirValueId,
    edges: &[jet_foundation::MIR::MirValueId],
    edit: jet_foundation::MIR::MirValueId,
    index: Option<jet_foundation::MIR::MirValueId>,
    kind: MirGcEditKind,
    site: jet_foundation::MIR::MirGcEditSiteId,
) -> Result<String, MirWebError> {
    let value = |id: jet_foundation::MIR::MirValueId| format!("__jet_values.get({})", id.0);
    let edge_values = format!(
        "[{}]",
        edges.iter().map(|id| value(*id)).collect::<Vec<_>>().join(", ")
    );
    let args = match kind {
        MirGcEditKind::Clear => {
            if index.is_some() || !edges.is_empty() {
                return Err(MirWebError::InvalidMir {
                    message: "MIR GC clear edit has unexpected index or edge operands".to_string(),
                });
            }
            vec![value(root), value(edit)]
        }
        MirGcEditKind::Pop => {
            if index.is_some() || !edges.is_empty() {
                return Err(MirWebError::InvalidMir {
                    message: "MIR GC pop edit has unexpected index or edge operands".to_string(),
                });
            }
            vec![value(root), value(edit)]
        }
        MirGcEditKind::RemoveIndex => {
            let Some(index) = index else {
                return Err(MirWebError::InvalidMir {
                    message: "MIR GC remove-index edit has no index operand".to_string(),
                });
            };
            if !edges.is_empty() {
                return Err(MirWebError::InvalidMir {
                    message: "MIR GC remove-index edit has unexpected edge operands".to_string(),
                });
            }
            vec![value(root), value(edit), value(index)]
        }
        MirGcEditKind::InsertIndex => {
            let Some(index) = index else {
                return Err(MirWebError::InvalidMir {
                    message: "MIR GC insert-index edit has no index operand".to_string(),
                });
            };
            vec![value(root), value(edit), value(index), edge_values]
        }
        MirGcEditKind::Prepend => {
            if index.is_some() {
                return Err(MirWebError::InvalidMir {
                    message: "MIR GC prepend edit has an unexpected index operand".to_string(),
                });
            }
            vec![value(root), edge_values, value(edit)]
        }
        MirGcEditKind::Additive => {
            if index.is_some() {
                return Err(MirWebError::InvalidMir {
                    message: "MIR GC additive edit has an unexpected index operand".to_string(),
                });
            }
            vec![value(root), edge_values, value(edit)]
        }
        MirGcEditKind::Plain => {
            if index.is_some() || !edges.is_empty() {
                return Err(MirWebError::InvalidMir {
                    message: "MIR plain GC edit has unexpected index or edge operands".to_string(),
                });
            }
            vec![value(root), value(edit)]
        }
        MirGcEditKind::EdgeSlot => {
            if index.is_some() {
                return Err(MirWebError::InvalidMir {
                    message: "MIR edge-slot edit has an unexpected index operand".to_string(),
                });
            }
            vec![value(root), edge_values, value(edit), site.0.to_string()]
        }
    };
    js_prelude_call_expression(program, call, &args)
}

fn js_typed_text_interp_expression(
    program: &MirProgram,
    function: &MirFunction,
    call: MirPreludeCallId,
    kind: jet_foundation::Syntax::TypedHeadKind,
    literals: &[String],
    holes: &[jet_foundation::MIR::MirValueId],
) -> Result<String, MirWebError> {
    if literals.len() != holes.len().saturating_add(1) {
        return Err(MirWebError::InvalidMir {
            message: "MIR typed text interpolation literal/hole arity is inconsistent".to_string(),
        });
    }
    let literal_values = format!(
        "[{}]",
        literals.iter().map(|literal| js_string(literal)).collect::<Vec<_>>().join(", ")
    );
    let hole_values = format!(
        "[{}]",
        holes
            .iter()
            .map(|id| format!("__jet_values.get({})", id.0))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let args = if matches!(kind, jet_foundation::Syntax::TypedHeadKind::HTML) {
        let trusted = holes
            .iter()
            .map(|id| {
                let ty = mir_function_value_type(function, *id)?;
                Ok((ty.nominal_name() == Some(jet_foundation::Syntax::TYPE_HTML)).to_string())
            })
            .collect::<Result<Vec<_>, MirWebError>>()?;
        vec![literal_values, hole_values, format!("[{}]", trusted.join(", "))]
    } else {
        vec![literal_values, hole_values]
    };
    js_prelude_call_expression(program, call, &args)
}
fn js_http_router_register_expression(
    program: &MirProgram,
    function: &MirFunction,
    call: MirPreludeCallId,
    receiver: jet_foundation::MIR::MirValueId,
    path: jet_foundation::MIR::MirValueId,
    handler: jet_foundation::MIR::MirValueId,
    method: MirHttpMethod,
    handler_param_names: &[String],
    contract_json: &str,
    location: jet_foundation::MIR::MirPanicLoc,
) -> Result<String, MirWebError> {
    let handler_type = mir_function_value_type(function, handler)?;
    let parameter_count = match handler_type.kind() {
        MirTypeKind::Fn(signature) => signature.params.len(),
        MirTypeKind::SendFn { params, .. } => params.len(),
        other => {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "MIR HTTP route handler has non-callable type {}",
                    other.display_name()
                ),
            })
        }
    };
    if parameter_count != handler_param_names.len() {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "MIR HTTP route handler metadata has {} names for {} parameters",
                handler_param_names.len(),
                parameter_count
            ),
        });
    }
    if handler_param_names.iter().any(String::is_empty) {
        return Err(MirWebError::InvalidMir {
            message: "MIR HTTP route handler metadata contains an empty parameter name".to_string(),
        });
    }
    // The web router stores the callable and checked contract for the server
    // boundary. Parameter names are validated above against the callable's
    // checked shape; the contract remains the transport binding authority.
    if location.column != 0 {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "MIR HTTP router registration location has unsupported column {}",
                location.column
            ),
        });
    }
    let Some(route) = program.prelude_calls.iter().find(|route| route.id == call) else {
        return Err(MirWebError::MissingPreludeCall { call: call.0 });
    };
    if route.signature.arity != 7 || route.signature.max_arity != 7 {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "MIR HTTP router registration Prelude call {} must have exact arity 7",
                call.0
            ),
        });
    }
    let handler_names = format!(
        "[{}]",
        handler_param_names
            .iter()
            .map(|name| js_string(name))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let adapted_handler = format!(
        "jet_http_router_handler_with_names(__jet_values.get({}), {})",
        handler.0, handler_names
    );
    let args = vec![
        format!("__jet_values.get({})", receiver.0),
        js_string(method.as_str()),
        format!("__jet_values.get({})", path.0),
        adapted_handler,
        js_source_file_path(program, location.file)?,
        location.line.to_string(),
        js_string(contract_json),
    ];
    js_prelude_call_expression(program, call, &args)
}


fn js_ccallback_unsupported(
    program: &MirProgram,
    function: &MirFunction,
    call: MirPreludeCallId,
    callback: jet_foundation::MIR::MirCallbackId,
    lambda: jet_foundation::MIR::MirValueId,
) -> Result<String, MirWebError> {
    let Some(adapter) = program.callbacks.iter().find(|adapter| adapter.id == callback) else {
        return Err(MirWebError::InvalidMir {
            message: format!("MIR callback adapter {} is missing", callback.0),
        });
    };
    if adapter.symbol.is_empty() {
        return Err(MirWebError::InvalidMir {
            message: format!("MIR callback adapter {} has no symbol", callback.0),
        });
    }
    if !program.functions.iter().any(|candidate| candidate.id == adapter.function) {
        return Err(MirWebError::MissingUserFunction {
            function: adapter.function.0,
        });
    }
    let _ = mir_function_value_type(function, lambda)?;
    let _ = js_prelude_call_expression(
        program,
        call,
        &[format!("__jet_values.get({})", lambda.0)],
    )?;
    Err(MirWebError::InvalidMir {
        message: format!(
            "MIR C callback adapter {} is not applicable to the Web target",
            callback.0
        ),
    })
}
/// Browser publication has no Component Model host boundary.  Keep this
/// defensive MIR arm even when sema already emits E3306: emitting an empty
/// object or a fake promise would silently turn a checked plugin call into
/// success.
fn js_plugin_invoke_unsupported(
    program: &MirProgram,
    call: MirPreludeCallId,
    export_name: &str,
    signature: &jet_foundation::MIR::ComponentSignatureDescriptor,
) -> Result<String, MirWebError> {
    let Some(route) = program.prelude_calls.iter().find(|row| row.id == call) else {
        return Err(MirWebError::MissingPreludeCall { call: call.0 });
    };
    if route.module != "core.plugin" || route.member != "invoke" {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "MIR plugin invocation {} uses non-plugin route {}.{}",
                call.0, route.module, route.member
            ),
        });
    }
    Err(MirWebError::InvalidMir {
        message: format!(
            "E3306: selected web target does not provide the Component Model host boundary \
             for reachable Core plugin import `core.plugin::{export_name}` (signature `{}`)",
            signature.wire()
        ),
    })
}

fn js_semantic_expression(
    program: &MirProgram,
    function: &MirFunction,
    semantic: &MirSemanticOp,
    result_type: Option<&MirType>,
) -> Result<String, MirWebError> {

    let value = |id: jet_foundation::MIR::MirValueId| format!("__jet_values.get({})", id.0);
    let values = |ids: &[jet_foundation::MIR::MirValueId]| {
        ids.iter().map(|id| value(*id)).collect::<Vec<_>>()
    };
    let prelude = |id: MirPreludeCallId, args: Vec<String>| {
        js_prelude_call_expression(program, id, &args)
    };
    let expression = match semantic {
        MirSemanticOp::DataEntriesToMap { call, local } => {
            let place = function
                .locals
                .iter()
                .find(|candidate| candidate.id == *local)
                .map(|candidate| candidate.place)
                .ok_or_else(|| MirWebError::InvalidMir {
                    message: format!("MIR data-entry local {} is missing", local.0),
                })?;
            prelude(*call, vec![js_read_place_expression(program, function, place)?])?
        }
        MirSemanticOp::MathBuiltin { type_id, call, args }
        | MirSemanticOp::PreciseBuiltin { type_id, call, args } => {
            ensure_type_instance(program, *type_id)?;
            prelude(*call, values(args))?
        }
        MirSemanticOp::Print { call, value: id } => {
            let value_type = mir_function_value_type(function, *id)?;
            let rendered = if matches!(value_type.kind(), MirTypeKind::String) {
                value(*id)
            } else {
                format!("jet_show({})", value(*id))
            };
            prelude(*call, vec![rendered])?
        }
        MirSemanticOp::AmbientInput { call, prompt } => {
            prelude(*call, prompt.map(value).into_iter().collect())?
        }
        MirSemanticOp::RequireStop {
            call,
            kind,
            condition,
            location,
            context,
            values: ids,
            always_stops: _,
        } => {
            let location_args = vec![
                js_source_file_path(program, location.file)?,
                location.line.to_string(),
                js_string(&context.function),
                js_string(&context.source_line),
                location.column.to_string(),
                context.caret.to_string(),
            ];
            let locals = js_panic_locals(program, function, context)?;
            match kind {
                MirRequireKind::Require => {
                    let Some(condition) = *condition else {
                        return Err(MirWebError::InvalidMir {
                            message: "MIR require stop has no checked condition".to_string(),
                        });
                    };
                    let message = match ids.as_slice() {
                        [] => js_string("condition failed"),
                        [message] => value(*message),
                        _ => {
                            return Err(MirWebError::InvalidMir {
                                message:
                                    "MIR require stop expects one checked condition and an optional message"
                                        .to_string(),
                            });
                        }
                    };
                    let mut args = vec![value(condition), message];
                    args.extend(location_args);
                    args.push(locals);
                    prelude(*call, args)?
                }
                MirRequireKind::RequireEq => {
                    let Some(condition) = *condition else {
                        return Err(MirWebError::InvalidMir {
                            message: "MIR require_eq stop has no checked condition".to_string(),
                        });
                    };
                    let [left, right] = ids.as_slice() else {
                        return Err(MirWebError::InvalidMir {
                            message: format!(
                                "MIR require_eq stop has {} values, expected two",
                                ids.len()
                            ),
                        });
                    };
                    let mut args = vec![value(condition), value(*left), value(*right)];
                    args.extend(location_args);
                    args.push(locals);
                    prelude(*call, args)?
                }
                MirRequireKind::Panic => {
                    if condition.is_some() {
                        return Err(MirWebError::InvalidMir {
                            message: "MIR panic stop carries an unexpected condition".to_string(),
                        });
                    }
                    let [message] = ids.as_slice() else {
                        return Err(MirWebError::InvalidMir {
                            message: format!(
                                "MIR panic stop has {} values, expected one",
                                ids.len()
                            ),
                        });
                    };
                    let mut args = location_args;
                    args.push(value(*message));
                    args.push(locals);
                    prelude(*call, args)?
                }
            }
        }
        MirSemanticOp::LayoutCompare { call, op, left, right } => {
            prelude(*call, vec![layout_compare_code(*op).to_string(), value(*left), value(*right)])?
        }
        MirSemanticOp::LayoutLiteral { inner } => value(*inner),
        MirSemanticOp::StructLiteral { type_id, fields, .. } => {
            ensure_type_instance(program, *type_id)?;
            let entries = fields
                .iter()
                .map(|(field, id)| {
                    Ok(format!(
                        "{}: {}",
                        js_string(&js_field_name(program, *field)?),
                        value(*id)
                    ))
                })
                .collect::<Result<Vec<_>, MirWebError>>()?
                .join(", ");
            format!(
                "Object.defineProperty({{ {} }}, \"__jet_type\", {{ value: {}, enumerable: false }})",
                entries,
                js_string(&type_id.0.to_string())
            )
        }
        MirSemanticOp::CellGuardProject {
            map_call,
            split_call,
            guard,
            paths,
            editable,
            edit_paths_disjoint,
        } => js_cell_guard_project_expression(
            program,
            *map_call,
            *split_call,
            value(*guard),
            paths,
            *editable,
            *edit_paths_disjoint,
            result_type,
        )?,
        MirSemanticOp::SharedGuardMap { call, guard, path, editable } => {
            if path.is_empty() {
                return Err(MirWebError::InvalidMir {
                    message: "MIR shared guard map has no checked field path".to_string(),
                });
            }
            js_shared_guard_map_expression(
                program,
                *call,
                value(*guard),
                path,
                *editable,
            )?
        }
        MirSemanticOp::SharedGuardSplit {
            call,
            map_call,
            guard,
            first,
            second,
            editable,
        } => js_shared_guard_split_expression(
            program,
            *map_call,
            *call,
            value(*guard),
            first,
            second,
            *editable,
            result_type,
        )?,
        MirSemanticOp::SharedGuardWait { call, guard, condition, predicate } => {
            prelude(*call, vec![value(*guard), value(*condition), value(*predicate)])?
        }
        MirSemanticOp::ConditionNotify { call, condition, all } => {
            prelude(*call, vec![value(*condition), all.to_string()])?
        }
        MirSemanticOp::AllocNew { call, kind } => {
            prelude(*call, vec![allocator_code(*kind).to_string()])?
        }
        MirSemanticOp::ColumnarRead { base, index, column, column_index, accessor } => {
            let _ = js_field_name(program, *column)?;
            prelude(
                *accessor,
                vec![value(*base), column_index.to_string(), value(*index)],
            )?
        }
        MirSemanticOp::StaticPreludeCall {
            call,
            args,
            owner_type_args,
            type_args,
        } => {
            validate_type_args(program, type_args)?;
            validate_owner_type_args(program, owner_type_args)?;
            let Some(route) = program.prelude_calls.iter().find(|route| route.id == *call) else {
                return Err(MirWebError::MissingPreludeCall { call: call.0 });
            };
            if route.module == "core.encoding.codec"
                && matches!(route.member.as_str(), "encode" | "decode" | "decode_typed")
            {
                js_codec_prelude_expression(program, function, &route.member, args, type_args)?
            } else {
                let mut rendered = js_call_values(program, function, args, false)?;
                if route.module == "::jet_std::JetShared" && route.member == "new" {
                    if args.len() != 1 {
                        return Err(MirWebError::InvalidMir {
                            message: format!(
                                "MIR Shared.new has {} arguments; expected one payload",
                                args.len()
                            ),
                        });
                    }
                    let [jet_foundation::MIR::MirPreludeTypeArg::Type(payload)] =
                        owner_type_args.as_slice()
                    else {
                        return Err(MirWebError::InvalidMir {
                            message: "MIR Shared.new has no checked payload type argument"
                                .to_string(),
                        });
                    };
                    rendered.push(js_string(&js_copy_type_key(program, payload)));
                }
                prelude(*call, rendered)?
            }
        }

        MirSemanticOp::HostCall { call, args } => {
            prelude(*call, js_call_values(program, function, args, false)?)?
        }
        MirSemanticOp::HardwareCall { .. } => {
            return Err(web_hardware_leak(&format!(
                "function {} reached the Web adapter",
                function.id.0
            )));
        }
        MirSemanticOp::DecodeUnder { call, segment, inner } => {
            prelude(*call, vec![value(*segment), value(*inner)])?
        }
        MirSemanticOp::BuiltinMethod {
            call,
            receiver,
            receiver_place,
            args,
        } => {
            let receiver = match receiver_place {
                Some(place) => {
                    let cell = js_place_cell_expression(program, function, *place)?;
                    format!("({cell}).value")
                }
                None => value(*receiver),
            };
            let mut rendered = vec![receiver];
            rendered.extend(values(args));
            prelude(*call, rendered)?
        }
        MirSemanticOp::OptionLift2 { call, function, left, right } => {
            prelude(*call, vec![value(*left), value(*right), value(*function)])?
        }
        MirSemanticOp::ClosureMethod { receiver, args, call } => {
            let Some(route) = program.prelude_calls.iter().find(|route| route.id == *call) else {
                return Err(MirWebError::MissingPreludeCall { call: call.0 });
            };
            let mut rendered = vec![value(*receiver)];
            for (index, arg) in args.iter().enumerate() {
                let rendered_arg = js_call_arg_value(program, function, arg, false)?;
                if route.member == "zip_pad" && matches!(index, 1 | 2) {
                    let ty = mir_function_value_type(function, arg.value)?;
                    let mut schemas = BTreeMap::new();
                    let key = js_history_capture_schema(
                        program,
                        ty,
                        &BTreeMap::new(),
                        &mut schemas,
                    );
                    rendered.push(format!(
                        "jet_copy_value_register({}, {}, __jet_copy_type_facts)",
                        rendered_arg,
                        js_string(&key),
                    ));
                } else {
                    rendered.push(rendered_arg);
                }
            }
            prelude(*call, rendered)?
        }
        MirSemanticOp::HostBorrowCallback { callable, params } => {
            validate_type_args(program, params)?;
            format!("(...__jet_callback_args) => {}(...__jet_callback_args)", value(*callable))
        }
        MirSemanticOp::TextPatternMatch { call, subject, parts } => prelude(
            *call,
            vec![value(*subject), js_text_pattern_parts(program, parts)?],
        )?,
        MirSemanticOp::BinaryPatternMatch { call, subject, parts } => prelude(
            *call,
            vec![value(*subject), js_binary_pattern_parts(program, parts)?],
        )?,
        MirSemanticOp::NumericMethod { call, receiver } => {
            prelude(*call, vec![value(*receiver)])?
        }
        MirSemanticOp::NumericBinaryMethod { call, receiver, argument } => {
            prelude(*call, vec![value(*receiver), value(*argument)])?
        }
        MirSemanticOp::OverflowOption { call, left, right, location } => {
            let Some(route) = program.prelude_calls.iter().find(|route| route.id == *call) else {
                return Err(MirWebError::MissingPreludeCall { call: call.0 });
            };
            let mut rendered = vec![value(*left), value(*right)];
            match (route.signature.arity, location) {
                (2, None) => {}
                (4, Some(location)) => {
                    rendered.push(js_source_file_path(program, location.file)?);
                    rendered.push(location.line.to_string());
                }
                (arity, location) => {
                    return Err(MirWebError::InvalidMir {
                        message: format!(
                            "MIR OverflowOption Prelude call {} has arity {} with {} location",
                            call.0,
                            arity,
                            if location.is_some() { "a" } else { "no" }
                        ),
                    });
                }
            }
            prelude(*call, rendered)?
        }
        MirSemanticOp::HandleMethod {
            call,
            receiver,
            args,
            frame_schedule,
            frame_schedule_derivation,
        } => {
            let Some(route) = program.prelude_calls.iter().find(|row| row.id == *call) else {
                return Err(MirWebError::MissingPreludeCall { call: call.0 });
            };
            if web_task_join_call(program, *call) {
                if !args.is_empty() {
                    return Err(MirWebError::InvalidMir {
                        message: format!(
                            "MIR task join route has {} arguments; expected none",
                            args.len()
                        ),
                    });
                }
                let joined = prelude(*call, vec![value(*receiver)])?;
                format!("await ({joined})")
            } else if web_channel_handle_call(program, *call) {
                let mut rendered = vec![value(*receiver)];
                rendered.extend(values(args));
                let expression = prelude(*call, rendered)?;
                if web_channel_async_call(program, *call) {
                    format!("await ({expression})")
                } else {
                    expression
                }
            } else if route.family == MirPreludeFamily::HandleMethod
                && route.module == "core.encoding.datatree"
            {
                let mut rendered = vec![value(*receiver)];
                rendered.extend(values(args));
                js_datatree_handle_expression(route, &rendered)?
            } else if route.family == MirPreludeFamily::HandleMethod
                && route.module == "core.time"
                && route
                    .member
                    .split_once('.')
                    .is_some_and(|(kind, method)| {
                        crate::Codegen::TIR::is_civil_time_method_name(Some(kind), method)
                    })
            {
                let (kind, method) = route.member.split_once('.').ok_or_else(|| {
                    MirWebError::InvalidMir {
                        message: format!(
                            "MIR civil-time route member `{}` is malformed",
                            route.member
                        ),
                    }
                })?;
                let rendered_args = values(args).join(", ");
                format!(
                    "jet_time_method({}, {}, {}, [{}])",
                    value(*receiver),
                    js_string(kind),
                    js_string(method),
                    rendered_args
                )
            } else {
                let mut rendered = vec![value(*receiver)];
                rendered.extend(values(args));
                if let Some(metadata) = route.db_metadata.as_ref() {
                    rendered.push(js_string(&metadata.to_wire()));
                }
                if route.member == "game.scene_on_frame" {
                    rendered.push(
                        frame_schedule
                            .as_ref()
                            .map(|schedule| js_string(&schedule.canonical_json()))
                            .unwrap_or_else(|| "null".to_string()),
                    );
                    rendered.push(
                        frame_schedule_derivation
                            .as_ref()
                            .map(|reference| js_string(&reference.id))
                            .unwrap_or_else(|| "null".to_string()),
                    );
                }
                prelude(*call, rendered)?
            }
        }
        MirSemanticOp::PluginInvoke {
            call,
            export_name,
            signature,
            ..
        } => js_plugin_invoke_unsupported(program, *call, export_name, signature)?,
        MirSemanticOp::CoreClosureCall {
            call,
            kind,
            values: ids,
            closure,
            site,
            label,
        } => js_core_closure_call_expression(
            program,
            *call,
            kind.clone(),
            ids,
            *closure,
            *site,
            label,
        )?,
        MirSemanticOp::TaskGroup { call, kind, tasks } => {
            let mut rendered = vec![task_group_code(*kind).to_string()];
            rendered.extend(values(tasks));
            prelude(*call, rendered)?
        }
        MirSemanticOp::Select { call, kind, values: ids } => {
            if web_channel_select_call(program, *call) {
                let [receivers, timers] = ids.as_slice() else {
                    return Err(MirWebError::InvalidMir {
                        message: format!(
                            "MIR channel select has {} values; expected receiver and timer lists",
                            ids.len()
                        ),
                    });
                };
                let expression = prelude(*call, vec![value(*receivers), value(*timers)])?;
                if web_channel_select_wait_call(program, *call) {
                    format!("await ({expression})")
                } else {
                    expression
                }
            } else {
                let mut rendered = vec![select_code(*kind).to_string()];
                rendered.extend(values(ids));
                prelude(*call, rendered)?
            }
        }
        MirSemanticOp::PolicyFunction { policy, values: ids } => {
            format!("{}({})", value(*policy), values(ids).join(", "))
        }
        MirSemanticOp::InterruptFunction { interrupt, values: ids } => {
            format!("{}({})", value(*interrupt), values(ids).join(", "))
        }
        MirSemanticOp::CarrierFact {
            call,
            receiver,
            field,
            notes,
        } => js_carrier_fact_expression(program, *call, *receiver, *field, *notes)?,
        MirSemanticOp::GcEdit {
            call,
            root,
            edges,
            edit,
            index,
            kind,
            site,
        } => js_gc_edit_expression(
            program,
            *call,
            *root,
            edges,
            *edit,
            *index,
            *kind,
            *site,
        )?,
        MirSemanticOp::TypedTextInterp {
            call,
            kind,
            literals,
            holes,
        } => js_typed_text_interp_expression(program, function, *call, *kind, literals, holes)?,
        MirSemanticOp::HttpRouterRegister {
            call,
            receiver,
            path,
            handler,
            method,
            handler_param_names,
            contract_json,
            location,
        } => js_http_router_register_expression(
            program,
            function,
            *call,
            *receiver,
            *path,
            *handler,
            *method,
            handler_param_names,
            contract_json,
            *location,
        )?,
        MirSemanticOp::CCallback {
            call,
            callback,
            lambda,
        } => js_ccallback_unsupported(program, function, *call, *callback, *lambda)?,
    };
    Ok(expression)
}
fn js_codec_prelude_expression(
    program: &MirProgram,
    function: &MirFunction,
    member: &str,
    args: &[jet_foundation::MIR::MirCallArg],
    type_args: &[MirType],
) -> Result<String, MirWebError> {
    let rendered = js_call_values(program, function, args, false)?;
    if member == "decode_typed" {
        let [tree] = rendered.as_slice() else {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "MIR typed codec Prelude call has {} arguments, expected one tree",
                    rendered.len()
                ),
            });
        };
        let [ty] = type_args else {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "MIR typed codec Prelude call has {} type arguments, expected one target",
                    type_args.len()
                ),
            });
        };
        let descriptor = js_codec_type_descriptor(program, ty)?;
        return Ok(format!("jet_codec_decode_typed({tree}, {descriptor})"));
    }
    let [_kind, value] = rendered.as_slice() else {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "MIR codec Prelude call {member:?} has {} arguments, expected kind and value",
                rendered.len()
            ),
        });
    };
    let [ty] = type_args else {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "MIR codec Prelude call {member:?} has {} type arguments, expected one codec type",
                type_args.len()
            ),
        });
    };
    if member == "encode" {
        let descriptor = js_codec_type_descriptor(program, ty)?;
        return Ok(format!("jet_codec_encode_typed({value}, {descriptor})"));
    }
    let codec = ty.nominal_name().ok_or_else(|| MirWebError::InvalidMir {
        message: format!(
            "MIR codec Prelude call {member:?} has a non-nominal codec type {}",
            ty.display_name()
        ),
    })?;
    if !matches!(
        codec,
        "Date" | "LocalDate" | "LocalTime" | "DateTime" | "Duration" | "Decimal"
    ) {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "MIR codec Prelude call {member:?} has unsupported codec type {codec:?}"
            ),
        });
    }
    match member {
        "decode" => {
            let decoded = match codec {
                "Date" | "LocalDate" => {
                    let text = js_codec_tree_text_expression(value);
                    js_codec_result_expression(
                        format!("jet_time_date_parse({text})"),
                        &format!("expected {codec}: "),
                    )
                }
                "LocalTime" => {
                    let text = js_codec_tree_text_expression(value);
                    js_codec_result_expression(
                        format!("jet_time_parse_time({text})"),
                        "expected LocalTime: ",
                    )
                }
                "DateTime" => {
                    let text = js_codec_tree_text_expression(value);
                    js_codec_result_expression(
                        format!("jet_time_parse_rfc3339({text})"),
                        "expected DateTime: ",
                    )
                }
                "Duration" => {
                    let integer = js_codec_tree_integer_expression(value);
                    let body = format!(
                        "(() => {{ const __jet_duration = {integer}; \
                         if (__jet_duration < JET_I64_MIN || __jet_duration > JET_I64_MAX) \
                         throw new Error(\"expected Duration, found out-of-range Int\"); \
                         return jet_time_clamp_i64(__jet_duration); }})()"
                    );
                    js_codec_result_expression(body, "")
                }
                "Decimal" => {
                    let decimal = js_codec_tree_decimal_expression(value);
                    js_codec_result_expression(decimal, "expected Decimal: ")
                }
                _ => unreachable!(),
            };
            Ok(decoded)
        }
        _ => Err(MirWebError::InvalidMir {
            message: format!("unsupported MIR codec Prelude member {member:?}"),
        }),
    }
}

fn js_datatree_handle_expression(
    route: &jet_foundation::MIR::MirPreludeCall,
    rendered: &[String],
) -> Result<String, MirWebError> {
    let (symbol, arity, borrow_mask) = match route.member.as_str() {
        "field" => ("jet_datatree_field", 2, &[true, true][..]),
        "at" => ("jet_datatree_at", 2, &[true, false][..]),
        "int" => ("jet_datatree_int", 1, &[true][..]),
        "text" => ("jet_datatree_text", 1, &[true][..]),
        "bool" => ("jet_datatree_bool", 1, &[true][..]),
        "float" => ("jet_datatree_float", 1, &[true][..]),
        "to_text" => ("jet_datatree_to_text", 1, &[true][..]),
        "equal_unordered" => ("jet_datatree_equal_unordered", 2, &[true, true][..]),
        other => {
            return Err(MirWebError::InvalidMir {
                message: format!("unsupported DataTree Prelude member {other:?}"),
            });
        }
    };
    if route.signature.arity != arity
        || route.signature.max_arity != arity
        || route.signature.borrow_mask.as_slice() != borrow_mask
    {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "MIR DataTree Prelude call {:?} has an invalid signature",
                route.member
            ),
        });
    }
    match &route.symbol {
        MirSymbol::Prelude(actual) if actual == symbol => {}
        _ => {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "MIR DataTree Prelude member {:?} has symbol {:?}, expected {:?}",
                    route.member, route.symbol, symbol
                ),
            });
        }
    }
    if rendered.len() != arity {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "MIR DataTree Prelude call {:?} has {} arguments, expected {}",
                route.member,
                rendered.len(),
                arity
            ),
        });
    }
    Ok(format!("{symbol}({})", rendered.join(", ")))
}

fn js_codec_user_decode_function(
    program: &MirProgram,
    target: &MirType,
) -> Result<jet_foundation::MIR::MirFunctionId, MirWebError> {
    let mut candidate = program
        .impls
        .iter()
        .filter(|implementation| {
            implementation.serde == Some(MirSerdeCodec::Decode)
                && implementation.self_type.same_checked_type(target)
        })
        .flat_map(|implementation| implementation.methods.iter().copied())
        .find(|id| {
            program
                .functions
                .iter()
                .find(|function| function.id == *id)
                .is_some_and(|function| function.name == "decode")
        });
    if candidate.is_none() {
        candidate = target.nominal_id().and_then(|type_id| {
            program
                .types
                .iter()
                .find(|definition| definition.id == type_id)
                .and_then(|definition| match &definition.kind {
                    MirTypeDefKind::Struct { methods, .. }
                    | MirTypeDefKind::Enum { methods, .. } => methods.iter().copied().find(|id| {
                        program
                            .functions
                            .iter()
                            .find(|function| function.id == *id)
                            .is_some_and(|function| function.name == "decode")
                    }),
                    _ => None,
                })
        });
    }
    let Some(id) = candidate else {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "typed codec target `{}` has no checked Decode function",
                target.display_name()
            ),
        });
    };
    let Some(function) = program.functions.iter().find(|function| function.id == id) else {
        return Err(MirWebError::MissingUserFunction { function: id.0 });
    };
    if !function.target_applicability.web || !function_in_bucket(function, WebBucket::JS) {
        return Err(MirWebError::MissingUserFunction { function: id.0 });
    }
    if function.generator.is_some() || web_function_is_async(program, id) {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "typed codec Decode function {} is async or generator",
                id.0
            ),
        });
    }
    Ok(id)
}

fn js_codec_type_descriptor(
    program: &MirProgram,
    ty: &MirType,
) -> Result<String, MirWebError> {
    match &ty.kind {
        MirTypeKind::Int => Ok("{ kind: \"int\" }".to_string()),
        MirTypeKind::Float => Ok("{ kind: \"float\" }".to_string()),
        MirTypeKind::Bool => Ok("{ kind: \"bool\" }".to_string()),
        MirTypeKind::String => Ok("{ kind: \"string\" }".to_string()),
        MirTypeKind::Char => Ok("{ kind: \"char\" }".to_string()),
        MirTypeKind::Float32 => Ok("{ kind: \"float32\" }".to_string()),
        MirTypeKind::IntN { signed, bits } => Ok(format!(
            "{{ kind: \"intn\", signed: {}, bits: {}, name: {}, byte: {} }}",
            signed,
            bits,
            js_string(&ty.display_name()),
            !signed && *bits == 8
        )),
        MirTypeKind::InlineRange { base, lo, hi } => Ok(format!(
            "{{ kind: \"range\", base: {}, lo: BigInt({lo}), hi: BigInt({hi}) }}",
            js_codec_type_descriptor(program, base)?
        )),
        MirTypeKind::Option(inner) => Ok(format!(
            "{{ kind: \"option\", inner: {} }}",
            js_codec_type_descriptor(program, inner)?
        )),
        MirTypeKind::List(inner) => Ok(format!(
            "{{ kind: \"list\", element: {} }}",
            js_codec_type_descriptor(program, inner)?
        )),
        MirTypeKind::FixedList { elem, len } => {
            let Some(length) = len.literal_value() else {
                return Err(MirWebError::InvalidMir {
                    message: format!(
                        "typed codec fixed-list target `{}` has a non-literal length",
                        ty.display_name()
                    ),
                });
            };
            Ok(format!(
                "{{ kind: \"fixed_list\", element: {}, length: BigInt({length}) }}",
                js_codec_type_descriptor(program, elem)?
            ))
        }
        MirTypeKind::Map { key, value } => {
            if !matches!(&key.kind, MirTypeKind::String) {
                return Err(MirWebError::InvalidMir {
                    message: "comptime maps require String keys".to_string(),
                });
            }
            Ok(format!(
                "{{ kind: \"map\", key: {}, value: {} }}",
                js_codec_type_descriptor(program, key)?,
                js_codec_type_descriptor(program, value)?
            ))
        }
        MirTypeKind::Shared(inner) | MirTypeKind::Tagged { inner, .. } => {
            js_codec_type_descriptor(program, inner)
        }
        MirTypeKind::Apply { name, .. } => match name.name.as_str() {
            "Data" | "DataTree" => Ok("{ kind: \"datatree\" }".to_string()),
            "Date" => Ok("{ kind: \"date\" }".to_string()),
            "LocalDate" => Ok("{ kind: \"local_date\" }".to_string()),
            "LocalTime" => Ok("{ kind: \"local_time\" }".to_string()),
            "DateTime" => Ok("{ kind: \"datetime\" }".to_string()),
            "Duration" => Ok("{ kind: \"duration\" }".to_string()),
            "Decimal" => Ok("{ kind: \"decimal\" }".to_string()),
            name @ ("I8" | "I16" | "I32" | "I64" | "I128" | "U8" | "U16" | "U32" | "U64" | "U128") => {
                let signed = name.starts_with('I');
                let bits = &name[1..];
                Ok(format!(
                    "{{ kind: \"intn\", signed: {}, bits: {}, name: {}, byte: {} }}",
                    signed,
                    bits,
                    js_string(name),
                    !signed && name == "U8"
                ))
            }
            _ => {
                let id = js_codec_user_decode_function(program, ty)?;
                Ok(format!(
                    "{{ kind: \"user\", name: {}, decode: (tree) => jet_fn_{}([], [tree]) }}",
                    js_string(&ty.display_name()),
                    id.0
                ))
            }
        },
        _ => Err(MirWebError::InvalidMir {
            message: format!(
                "unsupported typed codec target `{}` in Web adapter",
                ty.display_name()
            ),
        }),
    }
}

fn js_codec_tree_text_expression(tree: &str) -> String {
    format!(
        "(() => {{ const __jet_tree = {tree}; switch (__jet_tree?.tag) {{ \
         case \"Text\": case \"TypedText\": return String(__jet_tree.values?.[0]); \
         case \"Int\": return BigInt(__jet_tree.values?.[0]).toString(); \
         case \"Float\": return String(__jet_tree.values?.[0]); \
         case \"Bool\": return __jet_tree.values?.[0] ? \"true\" : \"false\"; \
         case \"Number\": throw new Error(\"expected Text, found number \" + String(__jet_tree.values?.[0])); \
         default: throw new Error(\"expected Text, found \" + String(__jet_tree?.tag ?? \"value\")); \
         }} }})()"
    )
}

fn js_codec_tree_integer_expression(tree: &str) -> String {
    format!(
        "(() => {{ const __jet_tree = {tree}; switch (__jet_tree?.tag) {{ \
         case \"Int\": return BigInt(__jet_tree.values?.[0]); \
         case \"Number\": case \"Text\": {{ \
           const __jet_text = String(__jet_tree.values?.[0]).trim(); \
           try {{ return BigInt(__jet_text); }} \
           catch (_) {{ throw new Error(\"expected Int, found text \" + JSON.stringify(__jet_text)); }} \
         }} \
         case \"Float\": {{ \
           const __jet_number = Number(__jet_tree.values?.[0]); \
           if (!Number.isFinite(__jet_number) || !Number.isInteger(__jet_number) \
             || __jet_number < Number(JET_I64_MIN) || __jet_number >= Number(JET_I64_MAX)) \
             throw new Error(\"expected Int, found out-of-range Float\"); \
           return BigInt(__jet_number); \
         }} \
         default: throw new Error(\"expected Int, found \" + String(__jet_tree?.tag ?? \"value\")); \
         }} }})()"
    )
}

fn js_codec_tree_decimal_expression(tree: &str) -> String {
    format!(
        "(() => {{ const __jet_tree = {tree}; switch (__jet_tree?.tag) {{ \
         case \"Number\": case \"Text\": \
           return jet_decimal_from_str(String(__jet_tree.values?.[0])); \
         case \"Int\": \
           return jet_decimal_from_str(BigInt(__jet_tree.values?.[0]).toString()); \
         case \"TypedText\": \
           throw new Error(\"expected Decimal, found text \" + JSON.stringify(__jet_tree.values?.[0])); \
         default: throw new Error(\"expected Decimal, found \" + String(__jet_tree?.tag ?? \"value\")); \
         }} }})()"
    )
}

fn js_codec_result_expression(body: String, prefix: &str) -> String {
    format!(
        "(() => {{ const __jet_codec_result = jet_time_result(() => {body}); \
         if (__jet_codec_result.tag === \"Ok\") return __jet_codec_result; \
         return {{ tag: \"Err\", values: [[{{ path: \"\", reason: {} + \
         String(__jet_codec_result.values?.[0] ?? \"\") }}]] }}; }})()",
        js_string(prefix)
    )
}
fn js_conversion_expression(
    program: &MirProgram,
    value_id: jet_foundation::MIR::MirValueId,
    parameters: &[jet_foundation::MIR::MirValueId],
    target: &MirType,
    conversion: &MirConversion,
) -> Result<String, MirWebError> {
    if let Some(type_id) = target.identity {
        ensure_type_instance(program, type_id)?;
    }
    let value = format!("__jet_values.get({})", value_id.0);
    match conversion {
        MirConversion::Transparent => Ok(value),
        MirConversion::NumericCast => {
            if !parameters.is_empty() {
                return Err(MirWebError::InvalidMir {
                    message: "numeric cast has parameters".to_string(),
                });
            }
            Ok(if target.is_integer() {
                format!("BigInt({value})")
            } else if matches!(target.kind(), MirTypeKind::Float32) {
                format!("Math.fround(Number({value}))")
            } else {
                format!("Number({value})")
            })
        }
        MirConversion::SendFn => {
            if !parameters.is_empty() {
                return Err(MirWebError::InvalidMir {
                    message: "SendFn MIR conversion has parameters".to_string(),
                });
            }
            Ok(value)
        }
        MirConversion::Prelude { call, location, .. } => {
            let Some(route) = program.prelude_calls.iter().find(|route| route.id == *call) else {
                return Err(MirWebError::MissingPreludeCall { call: call.0 });
            };
            if route.signature.max_arity != route.signature.arity {
                return Err(MirWebError::InvalidMir {
                    message: format!(
                        "MIR conversion Prelude call {} does not have an exact arity",
                        call.0
                    ),
                });
            }
            let mut args = Vec::with_capacity(parameters.len() + 3);
            args.push(value);
            args.extend(
                parameters
                    .iter()
                    .map(|id| format!("__jet_values.get({})", id.0)),
            );
            match route.signature.arity.checked_sub(args.len()) {
                Some(0) => {}
                Some(2) => {
                    args.push(js_source_file_path(program, location.file)?);
                    args.push(location.line.to_string());
                }
                Some(additional) => {
                    return Err(MirWebError::InvalidMir {
                        message: format!(
                            "MIR conversion Prelude call {} requires unsupported context arity {additional}",
                            call.0
                        ),
                    });
                }
                None => {
                    return Err(MirWebError::InvalidMir {
                        message: format!(
                            "MIR conversion Prelude call {} has fewer arguments than its value/parameter prefix",
                            call.0
                        ),
                    });
                }
            }
            js_prelude_call_expression(program, *call, &args)
        }
    }
}

fn validate_owner_type_args(
    program: &MirProgram,
    type_args: &[jet_foundation::MIR::MirPreludeTypeArg],
) -> Result<(), MirWebError> {
    for type_arg in type_args {
        if let jet_foundation::MIR::MirPreludeTypeArg::Type(ty) = type_arg {
            validate_type_args(program, std::slice::from_ref(ty))?;
        }
    }
    Ok(())
}

fn ensure_type_instance(
    program: &MirProgram,
    id: jet_foundation::MIR::MirTypeId,
) -> Result<(), MirWebError> {
    if program
        .type_instances
        .iter()
        .any(|ty| ty.identity == Some(id))
    {
        Ok(())
    } else {
        Err(MirWebError::InvalidMir {
            message: format!("MIR type instance {} is missing", id.0),
        })
    }
}

fn validate_type_args(
    program: &MirProgram,
    types: &[MirType],
) -> Result<(), MirWebError> {
    for ty in types {
        if let Some(type_id) = ty.identity {
            ensure_type_instance(program, type_id)?;
        }
    }
    Ok(())
}

fn ensure_enum_variant(
    program: &MirProgram,
    owner: jet_foundation::MIR::MirTypeId,
    variant: &str,
    payload_index: Option<usize>,
) -> Result<(), MirWebError> {
    ensure_type_instance(program, owner)?;
    let Some(type_def) = program.types.iter().find(|type_def| type_def.id == owner) else {
        return Err(MirWebError::InvalidMir {
            message: format!("MIR enum type definition {} is missing", owner.0),
        });
    };
    let MirTypeDefKind::Enum { variants, .. } = &type_def.kind else {
        return Err(MirWebError::InvalidMir {
            message: format!("MIR enum operation owner {} is not an enum", owner.0),
        });
    };
    let Some(declared) = variants.iter().find(|candidate| candidate.name == variant) else {
        return Err(MirWebError::InvalidMir {
            message: format!("MIR enum {} has no variant {}", owner.0, variant),
        });
    };
    if let Some(index) = payload_index {
        let arity = match &declared.payload {
            MirVariantPayload::Unit => 0,
            MirVariantPayload::Single(_) => 1,
            MirVariantPayload::Named(fields) => fields.len(),
        };
        if index >= arity {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "MIR enum {} variant {} has no payload index {}",
                    owner.0, variant, index
                ),
            });
        }
    }
    Ok(())
}

fn js_field_name(
    program: &MirProgram,
    id: jet_foundation::MIR::MirFieldId,
) -> Result<String, MirWebError> {
    program
        .fields
        .iter()
        .find(|row| row.id == id)
        .map(|row| row.field.name.clone())
        .ok_or_else(|| MirWebError::InvalidMir {
            message: format!("MIR field {} is missing", id.0),
        })
}

fn js_shared_guard_map_expression(
    program: &MirProgram,
    call: MirPreludeCallId,
    guard: String,
    path: &[jet_foundation::MIR::MirFieldId],
    editable: bool,
) -> Result<String, MirWebError> {
    let mut expression = guard;
    for field in path {
        let field_name = js_field_name(program, *field)?;
        expression = js_prelude_call_expression(
            program,
            call,
            &[
                expression,
                js_string(&field_name),
                editable.to_string(),
            ],
        )?;
    }
    Ok(expression)
}

fn js_shared_guard_split_expression(
    program: &MirProgram,
    map_call: MirPreludeCallId,
    split_call: MirPreludeCallId,
    guard: String,
    first: &[jet_foundation::MIR::MirFieldId],
    second: &[jet_foundation::MIR::MirFieldId],
    editable: bool,
    result_type: Option<&MirType>,
) -> Result<String, MirWebError> {
    if first.is_empty() || second.is_empty() {
        return Err(MirWebError::InvalidMir {
            message: "MIR shared guard split has an empty checked field path".to_string(),
        });
    }
    let common = first
        .iter()
        .zip(second)
        .take_while(|(left, right)| left == right)
        .count();
    if common == first.len() || common == second.len() {
        return Err(MirWebError::InvalidMir {
            message: "MIR shared guard split paths are identical or prefix-related".to_string(),
        });
    }
    let mapped = js_shared_guard_map_expression(program, map_call, guard, &first[..common], editable)?;
    let first_name = js_field_name(program, first[common])?;
    let second_name = js_field_name(program, second[common])?;
    let split = js_prelude_call_expression(
        program,
        split_call,
        &[
            mapped,
            js_string(&first_name),
            js_string(&second_name),
            editable.to_string(),
        ],
    )?;
    let first_guard = js_shared_guard_map_expression(
        program,
        map_call,
        "__jet_shared_split[0]".to_string(),
        &first[common + 1..],
        editable,
    )?;
    let second_guard = js_shared_guard_map_expression(
        program,
        map_call,
        "__jet_shared_split[1]".to_string(),
        &second[common + 1..],
        editable,
    )?;
    js_guard_pair_expression(
        program,
        result_type,
        split,
        first_guard,
        second_guard,
        "__jet_shared_split",
    )
}

fn js_guard_pair_expression(
    program: &MirProgram,
    result_type: Option<&MirType>,
    split: String,
    first_guard: String,
    second_guard: String,
    split_binding: &str,
) -> Result<String, MirWebError> {
    let Some(result_type) = result_type else {
        return Err(MirWebError::InvalidMir {
            message: "MIR guard split has no result type".to_string(),
        });
    };
    let Some(type_id) = result_type.identity else {
        return Err(MirWebError::InvalidMir {
            message: "MIR guard split result has no type identity".to_string(),
        });
    };
    ensure_type_instance(program, type_id)?;
    let Some(type_def) = program.types.iter().find(|type_def| type_def.id == type_id) else {
        return Err(MirWebError::InvalidMir {
            message: format!("MIR guard split result type {} is missing", type_id.0),
        });
    };
    let MirTypeDefKind::Struct { fields, .. } = &type_def.kind else {
        return Err(MirWebError::InvalidMir {
            message: format!("MIR guard split result type {} is not a two-field struct", type_id.0),
        });
    };
    let [first_field, second_field] = fields.as_slice() else {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "MIR guard split result type {} does not have two fields",
                type_id.0
            ),
        });
    };
    let first_key = js_string(&js_field_name(program, first_field.id)?);
    let second_key = js_string(&js_field_name(program, second_field.id)?);
    Ok(format!(
        "(() => {{ const {split_binding} = {split}; return {{ {first_key}: {first_guard}, {second_key}: {second_guard} }}; }})()"
    ))
}

fn js_cell_guard_map_expression(
    program: &MirProgram,
    call: MirPreludeCallId,
    guard: String,
    path: &[jet_foundation::MIR::MirFieldId],
    _editable: bool,
) -> Result<String, MirWebError> {
    let mut expression = guard;
    for field in path {
        let field_name = js_field_name(program, *field)?;
        expression = js_prelude_call_expression(
            program,
            call,
            &[expression, js_string(&field_name)],
        )?;
    }
    Ok(expression)
}

fn js_cell_guard_project_expression(
    program: &MirProgram,
    map_call: MirPreludeCallId,
    split_call: Option<MirPreludeCallId>,
    guard: String,
    paths: &[Vec<jet_foundation::MIR::MirFieldId>],
    editable: bool,
    edit_paths_disjoint: bool,
    result_type: Option<&MirType>,
) -> Result<String, MirWebError> {
    if !(1..=2).contains(&paths.len()) || paths.iter().any(|path| path.is_empty()) {
        return Err(MirWebError::InvalidMir {
            message: "MIR Cell guard projection has an invalid checked path shape".to_string(),
        });
    }
    let is_split = paths.len() == 2;
    if split_call.is_some() != is_split {
        return Err(MirWebError::InvalidMir {
            message: "MIR Cell guard projection path arity disagrees with its split row".to_string(),
        });
    }
    if edit_paths_disjoint && (!editable || !is_split) {
        return Err(MirWebError::InvalidMir {
            message: "MIR Cell guard edit disjointness proof has an invalid shape".to_string(),
        });
    }
    if editable && is_split && !edit_paths_disjoint {
        return Err(MirWebError::InvalidMir {
            message: "MIR editable Cell guard split is missing disjoint paths proof".to_string(),
        });
    }
    match paths {
        [path] => js_cell_guard_map_expression(program, map_call, guard, path, editable),
        [first, second] => {
            let common = first
                .iter()
                .zip(second)
                .take_while(|(left, right)| left == right)
                .count();
            if common == first.len() || common == second.len() {
                return Err(MirWebError::InvalidMir {
                    message: "MIR Cell guard split paths are identical or prefix-related".to_string(),
                });
            }
            let mapped =
                js_cell_guard_map_expression(program, map_call, guard, &first[..common], editable)?;
            let first_name = js_field_name(program, first[common])?;
            let second_name = js_field_name(program, second[common])?;
            let Some(split_call) = split_call else {
                return Err(MirWebError::InvalidMir {
                    message: "MIR Cell guard split has no exact split row".to_string(),
                });
            };
            let split = js_prelude_call_expression(
                program,
                split_call,
                &[
                    mapped,
                    js_string(&first_name),
                    js_string(&second_name),
                ],
            )?;
            let first_guard = js_cell_guard_map_expression(
                program,
                map_call,
                "__jet_cell_split[0]".to_string(),
                &first[common + 1..],
                editable,
            )?;
            let second_guard = js_cell_guard_map_expression(
                program,
                map_call,
                "__jet_cell_split[1]".to_string(),
                &second[common + 1..],
                editable,
            )?;
            js_guard_pair_expression(
                program,
                result_type,
                split,
                first_guard,
                second_guard,
                "__jet_cell_split",
            )
        }
        _ => Err(MirWebError::InvalidMir {
            message: "MIR Cell guard projection has an invalid checked path shape".to_string(),
        }),
    }
}

fn js_source_file_path(
    program: &MirProgram,
    id: jet_foundation::MIR::MirSourceFileId,
) -> Result<String, MirWebError> {
    let Some(file) = program.source_files.iter().find(|file| file.id == id) else {
        return Err(MirWebError::InvalidMir {
            message: format!("MIR source file {} is missing", id.0),
        });
    };
    Ok(js_string(&file.path))
}
fn js_function_stack_context(
    program: &MirProgram,
    function: &MirFunction,
) -> Result<(String, u32, String), MirWebError> {
    let module = program
        .modules
        .iter()
        .find(|module| module.id == function.module_id)
        .ok_or_else(|| MirWebError::InvalidMir {
            message: format!(
                "MIR function {} references missing module {}",
                function.id.0, function.module_id.0
            ),
        })?;
    let file = program
        .source_files
        .iter()
        .find(|file| file.id == module.source_file)
        .ok_or_else(|| MirWebError::InvalidMir {
            message: format!("MIR source file {} is missing", module.source_file.0),
        })?;
    let (line, _) = jet_foundation::Diagnostics::span_line_col(&file.source, function.span.start);
    let source_line = file
        .source
        .lines()
        .nth(line.saturating_sub(1))
        .unwrap_or_default()
        .trim_end();
    Ok((js_string(&file.path), line as u32, js_string(source_line)))
}


fn js_text_hole_kind(kind: MirTextHoleKind) -> String {
    match kind {
        MirTextHoleKind::Text => "{ kind: \"text\" }".to_string(),
        MirTextHoleKind::Int => "{ kind: \"int\" }".to_string(),
        MirTextHoleKind::Float => "{ kind: \"float\" }".to_string(),
        MirTextHoleKind::Bool => "{ kind: \"bool\" }".to_string(),
        MirTextHoleKind::InlineRange { lo, hi } => {
            format!("{{ kind: \"inline_range\", lo: {lo}, hi: {hi} }}")
        }
    }
}

fn js_text_pattern_parts(
    program: &MirProgram,
    parts: &[MirTextPatternPart],
) -> Result<String, MirWebError> {
    let rendered = parts
        .iter()
        .map(|part| match part {
            MirTextPatternPart::Literal(value) => {
                Ok(format!("{{ kind: \"literal\", value: {} }}", js_string(value)))
            }
            MirTextPatternPart::Hole { kind, ty, .. } => {
                ensure_type_instance(program, *ty)?;
                Ok(format!(
                    "{{ kind: \"hole\", hole_kind: {} }}",
                    js_text_hole_kind(*kind)
                ))
            }
        })
        .collect::<Result<Vec<_>, MirWebError>>()?;
    Ok(format!("[{}]", rendered.join(", ")))
}

fn js_binary_pattern_parts(
    program: &MirProgram,
    parts: &[MirBinaryPatternPart],
) -> Result<String, MirWebError> {
    let rendered = parts
        .iter()
        .map(|part| match part {
            MirBinaryPatternPart::Literal(values) => Ok(format!(
                "{{ kind: \"literal\", value: new Uint8Array([{}]) }}",
                values
                    .iter()
                    .map(|value| value.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            MirBinaryPatternPart::Bits { width, ty, little, .. } => {
                ensure_type_instance(program, *ty)?;
                Ok(format!("{{ kind: \"bits\", width: {}, little: {} }}", width, little))
            }
            MirBinaryPatternPart::Rest { ty, .. } => {
                ensure_type_instance(program, *ty)?;
                Ok("{ kind: \"rest\" }".to_string())
            }
        })
        .collect::<Result<Vec<_>, MirWebError>>()?;
    Ok(format!("[{}]", rendered.join(", ")))
}

fn web_trait_method_targets(
    program: &MirProgram,
    method_id: MirTraitMethodId,
    trait_ref: &MirTraitRef,
) -> Result<
    (
        Vec<(jet_foundation::MIR::MirTypeId, jet_foundation::MIR::MirFunctionId)>,
        Option<jet_foundation::MIR::MirFunctionId>,
    ),
    MirWebError,
> {
    let trait_def = program
        .traits
        .iter()
        .find(|definition| definition.id == trait_ref.id)
        .ok_or_else(|| MirWebError::InvalidMir {
            message: format!("MIR trait {} is missing", trait_ref.id.0),
        })?;
    let method = trait_def
        .methods
        .iter()
        .find(|method| method.id == method_id)
        .ok_or_else(|| MirWebError::InvalidMir {
            message: format!("MIR trait method {} is missing", method_id.0),
        })?;
    let mut targets = Vec::new();
    for impl_row in &program.impls {
        let Some(impl_trait) = impl_row.trait_ref.as_ref() else {
            continue;
        };
        if impl_trait.id != trait_ref.id {
            continue;
        }
        let type_id = impl_row
            .self_type
            .identity
            .ok_or_else(|| MirWebError::InvalidMir {
                message: format!("MIR impl {} has no self type identity", impl_row.id.0),
            })?;
        for function_id in &impl_row.methods {
            let function = program
                .functions
                .iter()
                .find(|function| function.id == *function_id)
                .ok_or_else(|| MirWebError::InvalidMir {
                    message: format!("MIR impl {} references a missing function", impl_row.id.0),
                })?;
            if function.name == method.name
                && matches!(
                    &function.form,
                    MirFunctionForm::TraitMethod {
                        trait_ref: function_trait,
                        ..
                    } if function_trait.id == trait_ref.id
                )
            {
                if targets.iter().any(|(existing, _)| *existing == type_id) {
                    return Err(MirWebError::InvalidMir {
                        message: format!(
                            "MIR trait method {} has ambiguous implementations for type {}",
                            method_id.0, type_id.0
                        ),
                    });
                }
                targets.push((type_id, *function_id));
            }
        }
    }
    Ok((targets, method.default))
}

fn js_call_expression(
    program: &MirProgram,
    caller: &MirFunction,
    callee: &MirCallee,
    args: &[jet_foundation::MIR::MirCallArg],
    type_args: &[MirType],
    history_provenance: &HistoryProvenance,
) -> Result<String, MirWebError> {
    validate_type_args(program, type_args)?;
    match callee {
        MirCallee::User(id) => {
            let Some(function) = program.functions.iter().find(|function| function.id == *id) else {
                return Err(MirWebError::MissingUserFunction { function: id.0 });
            };
            let values = js_call_values(program, caller, args, !is_wasm_export(function))?;
            if is_wasm_export(function) {
                return js_wasm_export_call(function, caller, args, &values);
            }
            if !function.target_applicability.web || !function_in_bucket(function, WebBucket::JS) {
                return Err(MirWebError::MissingUserFunction { function: id.0 });
            }
            let call = format!("jet_fn_{}([], [{}])", id.0, values.join(", "));
            Ok(if function.generator.is_none() && web_function_is_async(program, *id) {
                format!("await {call}")
            } else {
                call
            })
        }
        MirCallee::Associated { function: id, owner } => {
            validate_type_args(program, std::slice::from_ref(owner))?;
            let Some(function) = program.functions.iter().find(|function| function.id == *id) else {
                return Err(MirWebError::MissingUserFunction { function: id.0 });
            };
            let values = js_call_values(program, caller, args, !is_wasm_export(function))?;
            if is_wasm_export(function) {
                return js_wasm_export_call(function, caller, args, &values);
            }
            if !function.target_applicability.web || !function_in_bucket(function, WebBucket::JS) {
                return Err(MirWebError::MissingUserFunction { function: id.0 });
            }
            match &function.form {
                MirFunctionForm::Method {
                    owner: declared_owner,
                    self_access: None,
                } if declared_owner.same_checked_type(owner) => {}
                MirFunctionForm::TraitMethod {
                    owner: declared_owner,
                    self_access: None,
                    trait_ref,
                    ..
                } if declared_owner.same_checked_type(owner)
                    && program.traits.iter().any(|row| row.id == trait_ref.id) => {}
                MirFunctionForm::Method { .. } | MirFunctionForm::TraitMethod { .. } => {
                    return Err(MirWebError::InvalidMir {
                        message: format!(
                            "MIR associated call {} does not target a declared static method",
                            id.0
                        ),
                    });
                }
                MirFunctionForm::TopLevel => {
                    return Err(MirWebError::InvalidMir {
                        message: format!(
                            "MIR associated call {} does not target an inherent or trait method",
                            id.0
                        ),
                    });
                }
            }
            let call = format!("jet_fn_{}([], [{}])", id.0, values.join(", "));
            Ok(if function.generator.is_none() && web_function_is_async(program, *id) {
                format!("await {call}")
            } else {
                call
            })
        }
        MirCallee::Method { function: id, owner } => {
            validate_type_args(program, std::slice::from_ref(owner))?;
            let Some(function) = program.functions.iter().find(|function| function.id == *id) else {
                return Err(MirWebError::MissingUserFunction { function: id.0 });
            };
            let values = js_call_values(program, caller, args, !is_wasm_export(function))?;
            if is_wasm_export(function) {
                return js_wasm_export_call(function, caller, args, &values);
            }
            if !function.target_applicability.web || !function_in_bucket(function, WebBucket::JS) {
                return Err(MirWebError::MissingUserFunction { function: id.0 });
            }
            let expected_access = match &function.form {
                MirFunctionForm::Method {
                    owner: declared_owner,
                    self_access: Some(access),
                } if declared_owner.same_checked_type(owner) => *access,
                MirFunctionForm::TraitMethod {
                    owner: declared_owner,
                    self_access: Some(access),
                    trait_ref,
                    ..
                } if declared_owner.same_checked_type(owner)
                    && program.traits.iter().any(|row| row.id == trait_ref.id) => *access,
                MirFunctionForm::Method { .. } | MirFunctionForm::TraitMethod { .. } => {
                    return Err(MirWebError::InvalidMir {
                        message: format!(
                            "MIR method call {} owner/access disagrees with its declaration",
                            id.0
                        ),
                    });
                }
                MirFunctionForm::TopLevel => {
                    return Err(MirWebError::InvalidMir {
                        message: format!(
                            "MIR method call {} does not target an inherent or trait method",
                            id.0
                        ),
                    });
                }
            };
            let Some(receiver) = args.first() else {
                return Err(MirWebError::InvalidMir {
                    message: format!("MIR method call {} has no receiver argument", id.0),
                });
            };
            if receiver.access != expected_access {
                return Err(MirWebError::InvalidMir {
                    message: format!(
                        "MIR method call {} receiver access {:?} disagrees with declaration {:?}",
                        id.0, receiver.access, expected_access
                    ),
                });
            }
            if let Some(definition) = model_trait_for_function(program, *id) {
                if args.len() != 2 {
                    return Err(MirWebError::InvalidMir {
                        message: format!("model embed method {} has {} arguments; expected receiver and documents", id.0, args.len()),
                    });
                }
                let prefix = model_bridge_prefix(definition);
                let call = format!(
                    "__jet_model_embed_{prefix}({}, {})",
                    values[0], values[1]
                );
                return Ok(if caller.generator.is_none() {
                    format!("await {call}")
                } else {
                    call
                });
            }
            let call = format!("jet_fn_{}([], [{}])", id.0, values.join(", "));
            Ok(if function.generator.is_none() && web_function_is_async(program, *id) {
                format!("await {call}")
            } else {
                call
            })
        }
        MirCallee::TraitMethod { method, trait_ref, receiver: _receiver } => {
            let values = js_call_values(program, caller, args, true)?;
            let Some(receiver_value) = values.first() else {
                return Err(MirWebError::InvalidMir {
                    message: format!("MIR trait method call {} has no receiver argument", method.0),
                });
            };
            let (targets, default) = web_trait_method_targets(program, *method, trait_ref)?;
            let mut receiver_args = values.clone();
            receiver_args[0] = "__jet_receiver".to_string();
            let rendered_args = receiver_args.join(", ");
            let mut dispatch = if let Some(default_id) = default {
                let Some(function) = program.functions.iter().find(|function| function.id == default_id) else {
                    return Err(MirWebError::MissingUserFunction { function: default_id.0 });
                };
                if !function.target_applicability.web || !function_in_bucket(function, WebBucket::JS) {
                    return Err(MirWebError::MissingUserFunction { function: default_id.0 });
                }
                format!("jet_fn_{}([], [{}])", default_id.0, rendered_args)
            } else {
                format!(
                    "(() => {{ throw new Error({}) }})()",
                    js_string(&format!("no implementation for trait method {}", method.0))
                )
            };
            let mut is_async = default
                .and_then(|id| program.functions.iter().find(|function| function.id == id))
                .is_some_and(|function| {
                    function.generator.is_none() && web_function_is_async(program, function.id)
                });
            for (type_id, function_id) in targets.iter().rev() {
                let Some(function) = program.functions.iter().find(|function| function.id == *function_id) else {
                    return Err(MirWebError::MissingUserFunction { function: function_id.0 });
                };
                if !function.target_applicability.web || !function_in_bucket(function, WebBucket::JS) {
                    return Err(MirWebError::MissingUserFunction { function: function_id.0 });
                }
                is_async |= function.generator.is_none() && web_function_is_async(program, *function_id);
                let call = format!("jet_fn_{}([], [{}])", function_id.0, rendered_args);
                dispatch = format!(
                    "__jet_receiver?.__jet_type === {} ? {} : {}",
                    js_string(&type_id.0.to_string()),
                    call,
                    dispatch
                );
            }
            let expression = format!("((__jet_receiver) => ({}))({})", dispatch, receiver_value);
            Ok(if caller.generator.is_none() && is_async {
                format!("await ({expression})")
            } else {
                expression
            })
        }

        MirCallee::Core(id) => {
            let expression = js_core_call_expression(
                program,
                caller,
                *id,
                None,
                args,
                type_args,
                history_provenance,
            )?;
            Ok(if caller.generator.is_none() && web_model_core_call(program, *id) {
                format!("await ({expression})")
            } else {
                expression
            })
        }
        MirCallee::Prelude(id) => {
            let values = js_call_values(program, caller, args, false)?;
            js_prelude_call_expression(program, *id, &values)
        }
        MirCallee::Foreign(id) => {
            let Some(foreign) = program.foreign.iter().find(|foreign| foreign.id == *id) else {
                return Err(MirWebError::InvalidMir {
                    message: format!("MIR foreign function {} is missing", id.0),
                });
            };
            let abi = foreign_abi_name(&foreign.foreign_abi);
            Err(MirWebError::InvalidMir {
                message: format!(
                    "MIR Web JS-partition call references foreign row {} at {:?} \
                     (symbol `{}` path `{}` ABI `{}` language {:?} applicability \
                     rust_aot={} cranelift={} interpreter={} web={}); no checked JavaScript linkage exists",
                    foreign.id.0,
                    foreign.span,
                    foreign.symbol,
                    foreign.path,
                    abi,
                    foreign.foreign_language,
                    foreign.target_applicability.rust_aot,
                    foreign.target_applicability.cranelift,
                    foreign.target_applicability.interpreter,
                    foreign.target_applicability.web,
                ),
            })
        }
        MirCallee::Indirect(id) => Ok(format!(
            "__jet_values.get({})({})",
            id.0,
            js_call_values(program, caller, args, true)?.join(", ")
        )),
    }
}

fn js_closure_expression(
    program: &MirProgram,
    enclosing: &MirFunction,
    function_id: jet_foundation::MIR::MirFunctionId,
    captures: &[MirCaptureOperand],
    history_provenance: &HistoryProvenance,
) -> Result<String, MirWebError> {
    let Some(function) = program.functions.iter().find(|function| function.id == function_id) else {
        return Err(MirWebError::MissingUserFunction { function: function_id.0 });
    };
    if !function.target_applicability.web || !function_in_bucket(function, WebBucket::JS) {
        return Err(MirWebError::MissingUserFunction { function: function_id.0 });
    }
    if function.capture_params.len() != captures.len() {
        return Err(MirWebError::InvalidMir {
            message: format!(
                "MIR closure for function {} has {} captures, expected {}",
                function_id.0,
                captures.len(),
                function.capture_params.len()
            ),
        });
    }
    validate_capture_params(program, function)?;
    let values = captures
        .iter()
        .zip(&function.capture_params)
        .enumerate()
        .map(|(slot, (operand, capture))| {
            match (operand, capture.access) {
                (MirCaptureOperand::Value(value), MirAccess::Move) => {
                    Ok(js_move_value_expression(*value))
                }
                (MirCaptureOperand::Value(value), MirAccess::Read | MirAccess::Write)
                    if matches!(capture.ownership.mode, MirOwnershipMode::Owned) =>
                {
                    Ok(format!("{{ value: {} }}", js_move_value_expression(*value)))
                }
                (MirCaptureOperand::Place(place), MirAccess::Read | MirAccess::Write) => {
                    js_place_cell_expression(program, enclosing, *place)
                }
                (MirCaptureOperand::Value(_), access) => Err(MirWebError::InvalidMir {
                    message: format!(
                        "MIR closure capture slot {} passes a value for {:?} access",
                        slot, access
                    ),
                }),
                (MirCaptureOperand::Place(_), MirAccess::Move) => Err(MirWebError::InvalidMir {
                    message: format!(
                        "MIR closure capture slot {} passes a place for Move access",
                        slot
                    ),
                }),
            }
        })
        .collect::<Result<Vec<_>, MirWebError>>()?
        .join(", ");
    let mut schemas = BTreeMap::new();
    let mut capture_values = Vec::new();
    for capture in &function.capture_params {
        let schema = js_history_capture_schema(program, &capture.ty, &BTreeMap::new(), &mut schemas);
        let slot = capture.slot;
        let value = match capture.access {
            MirAccess::Read | MirAccess::Write => format!("captured[{slot}].value"),
            MirAccess::Move => format!("captured[{slot}]"),
        };
        capture_values.push(format!(
            "jet_testing_history_web_capture({value}, {}, types, active)",
            js_string(&schema),
        ));
    }
    let schemas = schemas.iter().map(|(key, schema)| {
        format!("[{}, {schema}]", js_string(key))
    }).collect::<Vec<_>>().join(", ");
    // The selected artifact binds the checked function table. This is the
    // function actually instantiated here, never a guessed factory return.
    let identity = sha256_hex(format!(
        "{}:function:{}", history_provenance.source, function_id.0
    ).as_bytes());
    Ok(format!(
        "((captured, types) => jet_testing_history_web_callable((...args) => jet_fn_{}(captured, args), {}, active => [{}]))([{}], new Map([{}]))",
        function_id.0, js_string(&identity), capture_values.join(", "), values, schemas
    ))
}

fn js_prelude_call_expression(
    program: &MirProgram,
    id: MirPreludeCallId,
    args: &[String],
) -> Result<String, MirWebError> {
    let Some(row) = program.prelude_calls.iter().find(|call| call.id == id) else {
        return Err(MirWebError::MissingPreludeCall { call: id.0 });
    };
    let symbol = js_symbol_expression(&row.symbol)?;
    Ok(format!("{symbol}({})", args.join(", ")))
}

fn web_data_symbol_name(name: &str) -> bool {
    matches!(
        name,
        "jet_data_count"
            | "jet_data_query"
            | "jet_data_track"
            | "jet_data_query_tracked"
            | "jet_data_track_insert"
            | "jet_data_track_replace"
            | "jet_data_track_remove"
            | "jet_data_load"
            | "jet_data_loader_load"
            | "jet_data_loader_load_default"
            | "jet_data_loader_file"
            | "jet_data_loader_file_member"
            | "jet_data_loader_url"
            | "jet_data_loader_database"
            | "jet_data_loader_value"
            | "jet_data_loader_snapshot"
            | "jet_data_loader_bind"
            | "jet_data_loader_bind_text"
            | "jet_data_loader_cancel"
            | "jet_data_loader_offline"
            | "jet_data_loader_invalidate"
            | "jet_data_loader_needs_refresh"
            | "jet_data_loader_ready"
            | "jet_data_loader_status"
            | "jet_data_loader_source_identity"
            | "jet_data_loader_authority_of"
            | "jet_data_snapshot_reusable"
            | "jet_data_loader_stream"
            | "jet_data_stream_next"
            | "jet_data_stream_collect"
            | "jet_data_stream_cancel"
            | "jet_data_query_plan"
            | "jet_data_query_collect"
            | "jet_data_query_sort_by"
            | "jet_data_left_join_checked_default"
            | "jet_data_inner_join_checked_default"
            | "jet_data_query_filter"
            | "jet_data_query_map"
            | "jet_data_query_min"
            | "jet_data_query_max"
            | "jet_data_query_inner_join"
            | "jet_data_query_left_join"
            | "jet_data_query_group_by"
            | "jet_data_query_watch"
            | "jet_data_watch_get"
            | "jet_data_watch_status"
            | "jet_data_watch_cancel"
            | "jet_data_group_count_query"
            | "jet_data_group_sum_query"
            | "jet_data_group_mean_query"
            | "jet_data_status"
            | "jet_data_require_bridge"
            | "jet_data_csv_reader"
            | "jet_data_json_reader"
    ) || name.starts_with("jet_data.loader.")
}

fn js_data_call_expression(
    program: &MirProgram,
    symbol: &MirSymbol,
    values: &[String],
    type_args: &[MirType],
) -> Result<String, MirWebError> {
    let name = match symbol {
        MirSymbol::Prelude(name) => name,
        MirSymbol::Runtime(_) => {
            return Err(MirWebError::InvalidMir {
                message: "Web data bridge requires a Prelude symbol".to_string(),
            })
        }
    };
    if !web_data_symbol_name(name) {
        return Err(MirWebError::InvalidMir {
            message: format!("MIR Web data symbol {name:?} has no data bridge binding"),
        });
    }
    let local = js_symbol_expression(symbol)?;
    let descriptors = type_args
        .iter()
        .map(|ty| js_string(&type_descriptor(ty)))
        .collect::<Vec<_>>()
        .join(", ");
    let mut args = values.join(", ");
    if !args.is_empty() {
        args.push_str(", ");
    }
    args.push('[');
    args.push_str(&descriptors);
    args.push(']');
    let _ = program;
    Ok(format!("{local}({args})"))
}


fn js_world_effect_guard(
    core: &jet_foundation::MIR::MirCoreCall,
) -> Option<String> {
    let effect = core.effect?;
    let controlled = match effect {
        jet_foundation::Authority::Effect::Time => {
            core.module == "core.time" && core.member != "start"
        }
        jet_foundation::Authority::Effect::Rand => core.module != "core.crypto.random",
        _ => false,
    };
    if controlled {
        None
    } else {
        Some(format!(
            "jet_world_reject_uncontrolled_effect({})",
            js_string(effect.name())
        ))
    }
}

fn js_guard_core_expression(
    core: &jet_foundation::MIR::MirCoreCall,
    expression: String,
) -> String {
    match js_world_effect_guard(core) {
        Some(guard) => format!("({guard}, {expression})"),
        None => expression,
    }
}

fn js_history_absent_schema(schemas: &mut BTreeMap<String, String>) -> String {
    let key = "jet.absent".to_string();
    schemas
        .entry(key.clone())
        .or_insert_with(|| format!("{{ type: {}, kind: \"absent\" }}", js_string(&key)));
    key
}

// Schemas describe checked types only. Values are read from the selected
// closure's live environment by TestingHistory.js at campaign invocation.
fn js_history_capture_schema(
    program: &MirProgram,
    ty: &MirType,
    bindings: &BTreeMap<String, String>,
    schemas: &mut BTreeMap<String, String>,
) -> String {
    if let MirTypeKind::Apply { name, args } = ty.kind() {
        if args.is_empty() {
            if let Some(schema) = bindings.get(&name.name) {
                return schema.clone();
            }
        }
    }
    let mut key = type_descriptor(ty);
    if let MirTypeKind::Tagged { marker, .. } = ty.kind() {
        key.push_str(&format!(";tag={marker}"));
    }
    if !bindings.is_empty() {
        key.push_str(&format!(";bindings={}", bindings.iter().map(|(name, value)| {
            format!("{}:{name}{}:{value}", name.len(), value.len())
        }).collect::<String>()));
    }
    if schemas.contains_key(&key) {
        return key;
    }
    // Reserve recursive nominal types before following their field types.
    schemas.insert(key.clone(), "{ kind: \"opaque\" }".to_string());
    let child = |ty: &MirType, schemas: &mut BTreeMap<String, String>| {
        js_string(&js_history_capture_schema(program, ty, bindings, schemas))
    };
    let shape = if program.handles.iter().any(|handle| {
        handle.ty.same_checked_type(ty)
            || handle.ty.nominal_name() == ty.nominal_name() && ty.nominal_name().is_some()
    }) {
        "kind: \"opaque\"".to_string()
    } else if ty.is_unit() {
        "kind: \"unit\"".to_string()
    } else {
        match ty.kind() {
            MirTypeKind::Int | MirTypeKind::IntN { .. } => "kind: \"integer\"".to_string(),
            MirTypeKind::Float | MirTypeKind::Float32 => "kind: \"float\"".to_string(),
            MirTypeKind::Bool => "kind: \"boolean\"".to_string(),
            MirTypeKind::String | MirTypeKind::Char => "kind: \"text\"".to_string(),
            MirTypeKind::Fn(_) | MirTypeKind::SendFn { .. } => "kind: \"callable\"".to_string(),
            MirTypeKind::List(inner) | MirTypeKind::FixedList { elem: inner, .. } => {
                format!("kind: \"list\", item: {}", child(inner, schemas))
            }
            MirTypeKind::Map { key, value } => format!(
                "kind: \"map\", key: {}, value: {}", child(key, schemas), child(value, schemas)
            ),
            MirTypeKind::Option(inner) => {
                let absent = js_history_absent_schema(schemas);
                format!(
                    "kind: \"enum\", clean: true, variants: [[\"Err\", [{}]], [\"Ok\", [{}]]]",
                    js_string(&absent),
                    child(inner, schemas),
                )
            }
            MirTypeKind::Result { ok, err } => format!(
                "kind: \"enum\", variants: [[\"Ok\", [{}]], [\"Err\", [{}]]]",
                child(ok, schemas), child(err, schemas)
            ),
            MirTypeKind::Shared(inner) => format!("kind: \"shared\", inner: {}", child(inner, schemas)),
            MirTypeKind::InlineRange { base, .. } | MirTypeKind::Quantity { base, .. } => {
                format!("kind: \"transparent\", inner: {}", child(base, schemas))
            }
            MirTypeKind::Tagged { marker, inner } => {
                let marker = marker.to_string();
                if marker == "expiring_secret.loan" || marker == "core.crypto"
                    || marker.rsplit("::").next() == Some("Secret")
                {
                    "kind: \"opaque\"".to_string()
                } else {
                    format!("kind: \"transparent\", inner: {}", child(inner, schemas))
                }
            }
            MirTypeKind::Tuple(fields) => format!(
                "kind: \"record\", fields: [{}]",
                fields.iter().map(|(name, ty)| {
                    format!("[{}, {}]", js_string(name), child(ty, schemas))
                }).collect::<Vec<_>>().join(", ")
            ),
            MirTypeKind::Apply { .. } | MirTypeKind::Union(_) => {
                let (nominal, args) = match ty.kind() {
                    MirTypeKind::Apply { name, args } => (Some(name), args.as_slice()),
                    _ => (None, &[][..]),
                };
                let leaf = nominal.map(|name| name.name.rsplit("::").next().unwrap_or(&name.name))
                    .unwrap_or("");
                match leaf {
                    "Secret" | "ExpiringSecret" | "HistoryRng" | "Ptr" => "kind: \"opaque\"".to_string(),
                    "Bytes" => "kind: \"bytes\"".to_string(),
                    "BigInt" => "kind: \"integer\"".to_string(),
                    "DataTree" => "kind: \"data\"".to_string(),
                    _ => {
                        let definition = program.types.iter().find(|row| Some(row.id) == ty.identity)
                            .or_else(|| nominal.and_then(|name| program.types.iter().find(|row| row.id == name.id)));
                        if let Some(definition) = definition {
                            let mut field_bindings = bindings.clone();
                            for (parameter, argument) in definition.generic_params.iter().zip(args) {
                                field_bindings.insert(parameter.name.clone(),
                                    js_history_capture_schema(program, argument, bindings, schemas));
                            }
                            let field_type = |ty: &MirType, schemas: &mut BTreeMap<String, String>| {
                                js_string(&js_history_capture_schema(program, ty, &field_bindings, schemas))
                            };
                            match &definition.kind {
                                MirTypeDefKind::Struct { fields, .. } if !fields.iter().any(|field| field.skip) => format!(
                                    "kind: \"record\", fields: [{}]",
                                    fields.iter().filter(|field| !field.computed).map(|field| {
                                        format!("[{}, {}]", js_string(&field.name), field_type(&field.ty, schemas))
                                    }).collect::<Vec<_>>().join(", ")
                                ),
                                MirTypeDefKind::Enum { variants, .. } => format!(
                                    "kind: \"enum\", variants: [{}]",
                                    variants.iter().map(|variant| {
                                        let fields = match &variant.payload {
                                            MirVariantPayload::Unit => Vec::new(),
                                            MirVariantPayload::Single(ty) => vec![field_type(ty, schemas)],
                                            MirVariantPayload::Named(fields) => fields.iter().map(|field| {
                                                if field.skip { "null".to_string() } else { field_type(&field.ty, schemas) }
                                            }).collect(),
                                        };
                                        format!("[{}, [{}]]", js_string(&variant.name), fields.join(", "))
                                    }).collect::<Vec<_>>().join(", ")
                                ),
                                MirTypeDefKind::Distinct { base, .. } | MirTypeDefKind::Alias { target: base } => {
                                    format!("kind: \"transparent\", inner: {}", field_type(base, schemas))
                                }
                                _ => "kind: \"opaque\"".to_string(),
                            }
                        } else {
                            "kind: \"opaque\"".to_string()
                        }
                    }
                }
            }
            _ => "kind: \"opaque\"".to_string(),
        }
    };
    schemas.insert(key.clone(), format!("{{ type: {}, {shape} }}", js_string(&key)));
    key
}

fn emit_js_copy_type_facts(out: &mut String, program: &MirProgram) {
    let mut schemas = BTreeMap::new();
    for ty in &program.type_instances {
        js_history_capture_schema(program, ty, &BTreeMap::new(), &mut schemas);
    }
    for function in &program.functions {
        js_history_capture_schema(program, &function.return_type, &BTreeMap::new(), &mut schemas);
        for param in &function.params {
            js_history_capture_schema(program, &param.ty, &BTreeMap::new(), &mut schemas);
        }
        for capture in &function.capture_params {
            js_history_capture_schema(program, &capture.ty, &BTreeMap::new(), &mut schemas);
        }
        for (_, ty, _, _) in &function.values {
            js_history_capture_schema(program, ty, &BTreeMap::new(), &mut schemas);
        }
        for place in &function.places {
            js_history_capture_schema(program, &place.ty, &BTreeMap::new(), &mut schemas);
        }
    }
    for handle in &program.handles {
        js_history_capture_schema(program, &handle.ty, &BTreeMap::new(), &mut schemas);
    }
    out.push_str("const __jet_copy_type_facts = new Map([\n");
    for (key, schema) in schemas {
        writeln!(out, "  [{}, {}],", js_string(&key), schema).unwrap();
    }
    out.push_str("]);\n");
}

fn js_copy_expression(program: &MirProgram, ty: &MirType, expression: String) -> String {
    let key = js_copy_type_key(program, ty);
    format!(
        "jet_copy_value({}, {}, __jet_copy_type_facts)",
        expression,
        js_string(&key),
    )
}

fn js_copy_type_key(program: &MirProgram, ty: &MirType) -> String {
    let mut schemas = BTreeMap::new();
    js_history_capture_schema(program, ty, &BTreeMap::new(), &mut schemas)
}

fn js_core_call_expression(
    program: &MirProgram,
    caller: &MirFunction,
    id: jet_foundation::MIR::MirCoreCallId,
    route: Option<MirPreludeCallId>,
    args: &[jet_foundation::MIR::MirCallArg],
    type_args: &[MirType],
    history_provenance: &HistoryProvenance,
) -> Result<String, MirWebError> {
    let Some(core) = program.core_calls.iter().find(|call| call.id == id) else {
        return Err(MirWebError::MissingCoreCall { call: id.0 });
    };
    validate_type_args(program, type_args)?;
    let values = js_call_values(program, caller, args, false)?;
    if core.module == "core.models" && core.member == "open" {
        if values.len() != 1 {
            return Err(MirWebError::InvalidMir {
                message: format!("MIR models.open has {} runtime arguments; expected one output name", values.len()),
            });
        }
        let Some(definition) = model_trait_for_type_args(program, type_args) else {
            return Err(MirWebError::InvalidMir {
                message: "MIR models.open has no checked model trait binding".to_string(),
            });
        };
        let expression = format!(
            "__jet_model_open_{}({})",
            model_bridge_prefix(definition),
            values[0]
        );
        return Ok(js_guard_core_expression(core, expression));
    }
    if core.module == "core.testing" && core.member == "histories" {
        if type_args.len() != 1 {
            return Err(MirWebError::InvalidMir {
                message: "MIR testing.histories requires exactly one checked Command type argument"
                    .to_string(),
            });
        }
        if values.len() != 6 {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "MIR testing.histories has {} runtime arguments; expected six",
                    values.len()
                ),
            });
        }
        let symbol = match &core.symbol {
            jet_foundation::Syntax::CoreCallSymbol::Prelude(name) => {
                js_symbol_expression(&MirSymbol::Prelude((*name).to_string()))?
            }
            jet_foundation::Syntax::CoreCallSymbol::Rust(name) => {
                js_symbol_expression(&MirSymbol::Runtime((*name).to_string()))?
            }
        };
        let command_descriptor = js_string(&type_descriptor(&type_args[0]));
        let provenance_descriptor = format!(
            "{{ source: {}, tool: {}, target: {} }}",
            js_string(&history_provenance.source),
            js_string(&history_provenance.tool),
            js_string(&history_provenance.target),
        );
        let descriptor = format!(
            "{{ command_type: {command_descriptor}, provenance: {provenance_descriptor} }}"
        );
        let expression = format!(
            "{symbol}({}, {descriptor})",
            values.join(", ")
        );
        return Ok(js_guard_core_expression(core, expression));
    }
    if let Some(route) = route {
        if let Some(row) = program.prelude_calls.iter().find(|call| call.id == route) {
            if let MirSymbol::Prelude(name) = &row.symbol {
                if web_data_symbol_name(name) {
                    return js_data_call_expression(program, &row.symbol, &values, type_args);
                }
            }
        }
        let expression = js_prelude_call_expression(program, route, &values)?;
        return Ok(js_guard_core_expression(core, expression));
    }
    if let jet_foundation::Syntax::CoreCallSymbol::Prelude(name) = &core.symbol {
        let symbol = MirSymbol::Prelude((*name).to_string());
        if web_data_symbol_name(name) {
            return js_data_call_expression(program, &symbol, &values, type_args);
        }
    }
    let symbol = match &core.symbol {
        jet_foundation::Syntax::CoreCallSymbol::Prelude(name) => {
            js_symbol_expression(&MirSymbol::Prelude((*name).to_string()))?
        }
        jet_foundation::Syntax::CoreCallSymbol::Rust(name) => {
            js_symbol_expression(&MirSymbol::Runtime((*name).to_string()))?
        }
    };
    Ok(js_guard_core_expression(
        core,
        format!("{symbol}({})", values.join(", ")),
    ))
}

// Runtime symbols are not JavaScript property lookups.  This closed table is
// the Web linkage boundary: every accepted target is a local identifier or an
// explicit imported expression (such as `jetDom.webStreamCommit`), while a new
// or target-inapplicable symbol fails emission instead of becoming ambient
// runtime dispatch.
// Namespaced Prelude symbols are admitted only when the canonical Web kernel
// has an explicit local alias.  The App builder uses the same rule: its
// checked chain is recorded by the browser adapter, while `jet_app_graph`
// remains the compiler-owned route authority.
const WEB_PRELUDE_LINKS: &[(&str, &str)] = &[
    ("jet_data_track", "jet_data_track"),
    ("jet_data_query_tracked", "jet_data_query_tracked"),
    ("jet_data_track_insert", "jet_data_track_insert"),
    ("jet_data_track_replace", "jet_data_track_replace"),
    ("jet_data_track_remove", "jet_data_track_remove"),
    ("jet_data_count", "jet_data_count"),
    ("jet_data_query", "jet_data_query"),
    ("jet_data_load", "jet_data_loader_load"),
    ("jet_data_loader_load", "jet_data_loader_load"),
    ("jet_data_loader_load_default", "jet_data_loader_load_default"),
    ("jet_data_loader_file", "jet_data_loader_file"),
    ("jet_data_loader_file_member", "jet_data_loader_file_member"),
    ("jet_data_loader_url", "jet_data_loader_url"),
    ("jet_data_loader_database", "jet_data_loader_database"),
    ("jet_data_loader_value", "jet_data_loader_value"),
    ("jet_data_loader_snapshot", "jet_data_loader_snapshot"),
    ("jet_data_loader_bind", "jet_data_loader_bind"),
    ("jet_data_loader_bind_text", "jet_data_loader_bind_text"),
    ("jet_data_loader_cancel", "jet_data_loader_cancel"),
    ("jet_data_loader_offline", "jet_data_loader_offline"),
    ("jet_data_loader_invalidate", "jet_data_loader_invalidate"),
    ("jet_data_loader_needs_refresh", "jet_data_loader_needs_refresh"),
    ("jet_data_loader_ready", "jet_data_loader_ready"),
    ("jet_data_loader_status", "jet_data_loader_status"),
    ("jet_data_loader_source_identity", "jet_data_loader_source_identity"),
    ("jet_data_loader_authority_of", "jet_data_loader_authority_of"),
    ("jet_data_snapshot_reusable", "jet_data_snapshot_reusable"),
    ("jet_data_loader_stream", "jet_data_loader_stream"),
    ("jet_data_stream_next", "jet_data_stream_next"),
    ("jet_data_stream_collect", "jet_data_stream_collect"),
    ("jet_data_stream_cancel", "jet_data_stream_cancel"),
    ("jet_data_query_plan", "jet_data_query_plan"),
    ("jet_data_query_collect", "jet_data_query_collect"),
    ("jet_data_query_sort_by", "jet_data_query_sort_by"),
    ("jet_data_left_join_checked_default", "jet_data_left_join"),
    ("jet_data_inner_join_checked_default", "jet_data_inner_join"),
    ("jet_data_query_filter", "jet_data_query_filter"),
    ("jet_data_query_map", "jet_data_query_map"),
    ("jet_data_query_min", "jet_data_query_min"),
    ("jet_data_query_max", "jet_data_query_max"),
    ("jet_data_query_inner_join", "jet_data_query_inner_join"),
    ("jet_data_query_left_join", "jet_data_query_left_join"),
    ("jet_data_query_group_by", "jet_data_query_group_by"),
    ("jet_data_query_watch", "jet_data_query_watch"),
    ("jet_data_watch_get", "jet_data_watch_get"),
    ("jet_data_watch_status", "jet_data_watch_status"),
    ("jet_data_watch_cancel", "jet_data_watch_cancel"),
    ("jet_data_group_count_query", "jet_data_group_count_query"),
    ("jet_data_group_sum_query", "jet_data_group_sum_query"),
    ("jet_data_group_mean_query", "jet_data_group_mean_query"),
    ("jet_data_status", "jet_data_status"),
    ("jet_data_require_bridge", "jet_data_require_bridge"),
    ("jet_data_csv_reader", "jet_data_csv_reader"),
    ("jet_data_json_reader", "jet_data_json_reader"),
    ("jet_fmt_display", "jet_display"),
    ("jet_fmt_debug", "jet_debug"),
    ("jet_term_write_stdout_line", "jetDom.print"),
    ("jet_std::jet_typed_url_literal", "jet_typed_url_literal"),
    ("jet_std::jet_int_abs", "jet_int_abs"),
    ("jet_std::jet_int_add", "jet_int_add"),
    ("jet_std::jet_int_sub", "jet_int_sub"),
    ("jet_std::jet_int_mul", "jet_int_mul"),
    ("jet_std_math_abs_f64", "jet_std_math_abs_f64"),
    ("jet_std_math_abs_f32", "jet_std_math_abs_f32"),
    ("jet_std::jet_int_checked_widen", "jet_int_checked_widen"),
    ("jet_std::jet_int_try_from", "jet_int_try_from"),
    ("jet_std::jet_int_try_from_checked", "jet_int_try_from_checked"),
    ("jet_std::jet_int_checked_fixed", "jet_int_checked_fixed"),
    ("jet_std::channel", "jet_channel_new"),
    ("jet_std::channel_bounded", "jet_channel_bounded"),
    ("jet_std::JetReceiver::receive", "jet_channel_receive"),
    ("jet_std::JetReceiver::close", "jet_channel_close"),
    ("jet_std::JetSender::send", "jet_channel_send"),
    ("jet_std::JetSender::close", "jet_channel_close"),
    ("jet_std::jet_select_wait_tagged", "jet_scheduler_select"),
    ("jet_std::jet_select_try_wait_tagged", "jet_scheduler_try_select"),
    ("jet_std::JetShared::new", "jet_shared_new"),
    ("::jet_std::JetShared::new", "jet_shared_new"),
    ("JetUiShortcut::cmd", "jet_ui_shortcut_cmd"),
    ("::JetUiShortcut::cmd", "jet_ui_shortcut_cmd"),
    ("jet_std::JetUiShortcut::cmd", "jet_ui_shortcut_cmd"),
    ("jet_app", "jetDom.jetApp"),
    ("jet_app_route", "jetDom.jetAppRoute"),
    ("jet_app_page", "jetDom.jetAppPage"),
    ("jet_app_layout", "jetDom.jetAppLayout"),
    ("jet_app_loader", "jetDom.jetAppLoader"),
    ("jet_app_loader_preload", "jetDom.jetAppLoaderPreload"),
    ("jet_app_pending", "jetDom.jetAppPending"),
    ("jet_app_not_found", "jetDom.jetAppNotFound"),
    ("jet_app_error", "jetDom.jetAppError"),
    ("jet_app_action", "jetDom.jetAppAction"),
    ("jet_app_form", "jetDom.jetAppForm"),
    ("jet_app_data", "jetDom.jetAppData"),
    ("jet_app_mount", "jetDom.jetAppMount"),
    ("jet_app_mount_with_effect", "jetDom.jetAppMountWithEffect"),
    ("jet_app_mount_with_policy", "jetDom.jetAppMountWithPolicy"),
    ("jet_app_routes", "jetDom.jetAppRoutes"),
    ("jet_app_security", "jetDom.jetAppSecurity"),
    ("jet_app_assets", "jetDom.jetAppAssets"),
    ("jet_app_split", "jetDom.jetAppSplit"),
    ("jet_app_code_split", "jetDom.jetAppCodeSplit"),
    ("jet_app_cache", "jetDom.jetAppCache"),
    ("jet_app_a11y", "jetDom.jetAppA11y"),
    ("jet_app_adapter", "jetDom.jetAppAdapter"),
    ("jet_app_csr", "jetDom.jetAppCsr"),
    ("jet_app_ssr", "jetDom.jetAppSsr"),
    ("jet_app_ssg", "jetDom.jetAppSsg"),
    ("jet_app_stream", "jetDom.jetAppStream"),
    ("jet_app_streaming", "jetDom.jetAppStreaming"),
    ("jet_app_island", "jetDom.jetAppIsland"),
    ("jet_app_hydration_dev", "jetDom.jetAppHydrationDev"),
    ("jet_app_hydration_release", "jetDom.jetAppHydrationRelease"),
    ("jet_app_facts_json", "jetDom.jetAppFactsJson"),
    ("jet_app_serve", "jetDom.jetAppServe"),
    ("jet_app_serve_with_port", "jetDom.jetAppServeWithPort"),
    ("jet_app_serve_on", "jetDom.jetAppServeOn"),
    ("jet_testing_histories", "jet_testing_histories"),
    ("jet_testing_status", "jet_testing_status"),
    ("jet_testing_assert_equal", "jet_testing_assert_equal"),
    ("jet_testing_world", "jet_testing_world"),
    ("jet_world_now", "jet_world_now"),
    ("jet_world_advance", "jet_world_advance"),
    ("jet_world_wait_idle", "jet_world_wait_idle"),
    ("jet_world_history", "jet_world_history"),
];

const WEB_RUNTIME_LINKS: &[(&str, &str)] = &[
    ("jet_web_error_wire", "jet_web_error_wire"),
    ("jet_journey_reset", "jet_journey_reset"),
    ("jet_journey_frame_text", "jet_journey_frame_text"),
    ("jet_err_from_message", "jet_err_from_message"),
    ("jet_err_with_context_frame", "jet_err_with_context_frame"),
    ("jet_entry_error_exit_jet", "jet_entry_error_exit_jet"),
    ("jet_web_result_value", "jet_web_result_value"),
    ("jet_web_edge_error", "jet_web_edge_error"),
    ("jet_web_edge_result", "jet_web_edge_result"),
    ("jet_web_default_error", "jet_web_default_error"),
    ("jet_web_error_from_conversion", "jet_web_error_from_conversion"),
    ("jet_web_try", "jet_web_try"),
    ("jet_list_bounds_message", "jet_list_bounds_message"),
    ("jet_missing_map_key_message", "jet_missing_map_key_message"),
    ("jet_list_get", "jet_list_get"),
    ("jet_map_get", "jet_map_get"),
    ("jet_web_runtime_context", "jet_web_runtime_context"),
    ("jet_web_stream_register", "jetDom.webStreamRegister"),
    ("jet_web_stream_begin", "jetDom.webStreamBegin"),
    ("jet_web_stream_chunk", "jetDom.webStreamChunk"),
    ("jet_web_stream_commit", "jetDom.webStreamCommit"),
    ("jet_web_stream_fail", "jetDom.webStreamFail"),
    ("jet_web_stream_cancel", "jetDom.webStreamCancel"),
    ("jet_web_stream_rollback", "jetDom.webStreamRollback"),
    (
        "jet_web_stream_hydration_start",
        "jetDom.webStreamHydrationStart",
    ),
    (
        "jet_web_stream_hydration_complete",
        "jetDom.webStreamHydrationComplete",
    ),
    ("jet_web_stream_receipt", "jetDom.webStreamReceipt"),
    ("jet_web_stream_projection", "jetDom.webStreamProjection"),
    ("jet_web_runtime_context_frame", "jet_web_runtime_context_frame"),
    ("jet_runtime_stop_report", "jet_runtime_stop_report"),
    ("jet_runtime_stop", "jet_runtime_stop"),
    ("jet_web_wasm_host_error", "jet_web_wasm_host_error"),
    ("jet_stack_overflow_message", "jet_stack_overflow_message"),
    ("jet_stack_enter", "jet_stack_enter"),
    ("jet_stack_leave", "jet_stack_leave"),
    ("jet_todo_stop", "jet_todo_stop"),
    ("jet_contract_check", "jet_contract_check"),
    ("jet_contract_fail", "jet_contract_fail"),
    ("jet_keep", "jet_keep"),
    ("jet_task_spawn", "jet_task_spawn"),
    ("jet_task_join", "jet_task_join"),
    ("jet_task_detach", "jet_task_detach"),
    ("jet_task_group_body_failed", "jet_task_group_body_failed"),
    ("jet_task_all", "jet_task_all"),
    ("jet_task_all_named", "jet_task_all_named"),
    ("jet_std::JetTask::join", "jet_task_join"),
    ("jet_std::jet_task_join_result", "jet_task_join_result"),
    ("jet_std::JetEventScope::new", "jet_event_scope_new"),
    ("jet_std::JetHook::new", "jet_event_hook_new"),
    ("jet_std::JetDecisionHook::new", "jet_event_decision_hook_new"),
    ("jet_std::JetEventPolicy::sync", "jet_event_policy_sync"),
    ("jet_std::JetEvent::new", "jet_event_new"),
    ("jet_std::JetEvent::with_policy", "jet_event_with_policy"),
    ("jet_std::JetAsyncEvent::new", "jet_async_event_new"),
    ("jet_std::JetEvent::on", "jet_event_on"),
    ("jet_std::JetEvent::once", "jet_event_once"),
    ("jet_std::JetEvent::on_priority", "jet_event_on_priority"),
    ("jet_std::JetEvent::emit", "jet_event_emit"),
    ("jet_std::JetEvent::listener_count", "jet_event_listener_count"),
    ("jet_std::JetEvent::trace", "jet_event_trace"),
    ("jet_std::JetAsyncEvent::on", "jet_async_event_on"),
    ("jet_std::JetAsyncEvent::once", "jet_async_event_once"),
    ("jet_std::JetAsyncEvent::on_priority", "jet_async_event_on_priority"),
    ("jet_std::JetAsyncEvent::emit_async", "jet_async_event_emit"),
    ("jet_std::JetAsyncEvent::close", "jet_async_event_close"),
    ("jet_std::JetAsyncEvent::listener_count", "jet_async_event_listener_count"),
    ("jet_std::JetAsyncEvent::queued_count", "jet_async_event_queued_count"),
    ("jet_std::JetAsyncEvent::running_count", "jet_async_event_running_count"),
    ("jet_std::JetAsyncEvent::blocked_count", "jet_async_event_blocked_count"),
    ("jet_std::JetHook::on", "jet_hook_on"),
    ("jet_std::JetHook::once", "jet_hook_once"),
    ("jet_std::JetHook::on_priority", "jet_hook_on_priority"),
    ("jet_std::JetHook::run", "jet_hook_run"),
    ("jet_std::JetHook::listener_count", "jet_hook_listener_count"),
    ("jet_std::JetHook::trace", "jet_hook_trace"),
    ("jet_std::JetDecisionHook::on", "jet_decision_hook_on"),
    ("jet_std::JetDecisionHook::once", "jet_decision_hook_once"),
    ("jet_std::JetDecisionHook::on_priority", "jet_decision_hook_on_priority"),
    ("jet_std::JetDecisionHook::run", "jet_decision_hook_run"),
    ("jet_std::JetDecisionHook::listener_count", "jet_decision_hook_listener_count"),
    ("jet_std::JetSubscription::unsubscribe", "jet_subscription_unsubscribe"),
    ("jet_std::JetSubscription::active", "jet_subscription_active"),
    ("jet_std::JetEventScope::cancel", "jet_event_scope_cancel"),
    ("jet_std::JetEventScope::active_count", "jet_event_scope_active_count"),
    ("jet_std::JetEventTrace::summary", "jet_event_trace_summary"),
    ("jet_std::JetEventTrace::delivered", "jet_event_trace_delivered"),
    ("jet_std::JetEventTrace::queued", "jet_event_trace_queued"),
    ("jet_std::JetEventTrace::dropped", "jet_event_trace_dropped"),
    ("jet_std::JetDispatchReport::state", "jet_dispatch_report_state"),
    ("jet_std::JetDispatchReport::accepted", "jet_dispatch_report_accepted"),
    ("jet_std::JetDispatchReport::delivered_handlers", "jet_dispatch_report_delivered_handlers"),
    ("jet_std::JetDispatchReport::failures", "jet_dispatch_report_failures"),
    ("jet_std::JetDispatchReport::trace", "jet_dispatch_report_trace"),
    ("jet_channel_send", "jet_channel_send"),
    ("jet_channel_receive", "jet_channel_receive"),
    ("jet_channel_close", "jet_channel_close"),
    ("jet_scheduler_select", "jet_scheduler_select"),
    ("jet_scheduler_try_select", "jet_scheduler_try_select"),
    ("jet_option_lift2", "jet_option_lift2"),
    ("jet_fixed_list_index", "jet_fixed_list_index"),
    ("jet_list_map", "jet_list_map"),
    ("jet_list_filter", "jet_list_filter"),
    ("jet_iter_map", "jet_iter_map"),
    ("jet_iter_filter", "jet_iter_filter"),
    ("jet_list_try_map", "jet_list_try_map"),
    ("jet_list_try_filter", "jet_list_try_filter"),
    ("jet_list_pop_kernel", "jet_list_pop_kernel"),
    ("jet_list_remove_value", "jet_list_remove_value"),
    ("jet_list_remove_slot", "jet_list_remove_slot"),
    ("jet_map_pop_kernel", "jet_map_pop_kernel"),
    ("jet_map_pop_first", "jet_map_pop_first"),
    ("jet_list_take", "jet_list_take"),
    ("jet_list_skip", "jet_list_skip"),
    ("jet_iter_lazy", "jet_iter_lazy"),
    ("jet_iter_from_vec", "jet_iter_from_vec"),
    ("jet_iter_to_list", "jet_iter_to_list"),
    ("jet_iter_collect", "jet_iter_collect"),
    ("jet_iter_take", "jet_iter_take"),
    ("jet_iter_skip", "jet_iter_skip"),
    ("jet_iter_empty", "jet_iter_empty"),
    ("jet_iter_some", "jet_iter_some"),
    ("jet_iter_indexes", "jet_iter_indexes"),
    ("jet_iter_zip", "jet_iter_zip"),
    ("jet_iter_zip_strict", "jet_iter_zip_strict"),
    ("jet_iter_zip_pad", "jet_iter_zip_pad"),
    ("jet_iter_step_by", "jet_iter_step_by"),
    ("jet_iter_dedup", "jet_iter_dedup"),
    ("jet_iter_chunks", "jet_iter_chunks"),
    ("jet_iter_windows", "jet_iter_windows"),
    ("jet_iter_flatten", "jet_iter_flatten"),
    ("jet_list_flatten", "jet_list_flatten"),
    ("jet_iter_intersperse", "jet_iter_intersperse"),
    ("jet_iter_repeat", "jet_iter_repeat"),
    ("jet_iter_cycle", "jet_iter_cycle"),
    ("jet_iter_drop_last", "jet_iter_drop_last"),
    ("jet_iter_shuffle", "jet_iter_shuffle"),
    ("jet_iter_is_sorted", "jet_iter_is_sorted"),
    ("jet_iter_last_index_of", "jet_iter_last_index_of"),
    ("jet_iter_average_int", "jet_iter_average_int"),
    ("jet_iter_average_float", "jet_iter_average_float"),
    ("jet_iter_compare", "jet_iter_compare"),
    ("jet_iter_split", "jet_iter_split"),
    ("jet_iter_zip_family", "jet_iter_zip_family"),
    ("jet_list_try_collect", "jet_list_try_collect"),
    ("jet_iter_first", "jet_iter_first"),
    ("jet_string_concat", "jet_string_concat"),
    ("jet_view_copy", "jet_view_copy"),
    ("jet_string_view_copy", "jet_string_view_copy"),
    ("jet_float_display", "jet_float_display"),
    ("jet_show", "jet_show"),
    ("jet_fmt_decimal", "jet_fmt_decimal"),
    ("jet_fmt_fixed_even", "jet_fmt_fixed_even"),
    ("jet_fmt_grouped", "jet_fmt_grouped"),
    ("jet_fmt_decimal_int", "jet_fmt_decimal_int"),
    ("jet_fmt_grouped_int", "jet_fmt_grouped_int"),
    ("jet_group_decimal", "jet_group_decimal"),
    ("jet_display", "jet_display"),
    ("jet_debug", "jet_debug"),
    ("jet_fmt_pretty", "jet_fmt_pretty"),
    ("jet_gc_root", "jet_gc_root"),
    ("jet_gc_apply", "jet_gc_apply"),
    ("jet_gc_read", "jet_gc_read"),
    ("jet_gc_edit_clear", "jet_gc_edit_clear"),
    ("jet_gc_edit_pop", "jet_gc_edit_pop"),
    ("jet_gc_edit_remove_index", "jet_gc_edit_remove_index"),
    ("jet_gc_edit_insert_index", "jet_gc_edit_insert_index"),
    ("jet_gc_edit_prepend", "jet_gc_edit_prepend"),
    ("jet_gc_edit_additive", "jet_gc_edit_additive"),
    ("jet_gc_edit_plain", "jet_gc_edit_plain"),
    ("jet_gc_edit_edge_slot", "jet_gc_edit_edge_slot"),
    ("jet_typed_sql_raw", "jet_typed_sql_raw"),
    ("jet_typed_sql_interpolate", "jet_typed_sql_interpolate"),
    ("jet_typed_sql_template", "jet_typed_sql_template"),
    ("jet_typed_sql_params", "jet_typed_sql_params"),
    ("jet_typed_html_raw", "jet_typed_html_raw"),
    ("jet_typed_html_text", "jet_typed_html_text"),
    ("jet_typed_html_escape", "jet_typed_html_escape"),
    ("jet_typed_html_interpolate", "jet_typed_html_interpolate"),
    ("jet_typed_sh_raw", "jet_typed_sh_raw"),
    ("jet_typed_sh_interpolate", "jet_typed_sh_interpolate"),
    ("jet_typed_path_component", "jet_typed_path_component"),
    ("jet_typed_path_interpolate", "jet_typed_path_interpolate"),
    ("jet_typed_path_literal", "jet_typed_path_literal"),
    ("jet_typed_datetime_interpolate", "jet_typed_datetime_interpolate"),
    ("jet_typed_url_literal", "jet_typed_url_literal"),
    ("jet_expiring_now", "jet_expiring_now"),
    ("jet_expiring_new", "jet_expiring_new"),
    ("jet_expiring_secret_new", "jet_expiring_secret_new"),
    ("jet_expiring_get", "jet_expiring_get"),
    ("jet_expiring_secret_with", "jet_expiring_secret_with"),
    ("jet_fraction_value", "jet_fraction_value"),
    ("jet_fraction_from_parts", "jet_fraction_from_parts"),
    ("jet_fraction_add", "jet_fraction_add"),
    ("jet_fraction_sub", "jet_fraction_sub"),
    ("jet_fraction_mul", "jet_fraction_mul"),
    ("jet_fraction_div", "jet_fraction_div"),
    ("jet_fraction_equal", "jet_fraction_equal"),
    ("jet_fraction_numerator", "jet_fraction_numerator"),
    ("jet_fraction_denominator", "jet_fraction_denominator"),
    ("jet_fraction_to_float", "jet_fraction_to_float"),
    ("jet_fraction_is_zero", "jet_fraction_is_zero"),
    ("jet_fraction_to_string", "jet_fraction_to_string"),
    ("jet_complex_value", "jet_complex_value"),
    ("jet_complex_from_parts", "jet_complex_from_parts"),
    ("jet_complex_add", "jet_complex_add"),
    ("jet_complex_sub", "jet_complex_sub"),
    ("jet_complex_mul", "jet_complex_mul"),
    ("jet_complex_div", "jet_complex_div"),
    ("jet_complex_abs", "jet_complex_abs"),
    ("jet_measurement_kernel_new", "jet_measurement_kernel_new"),
    ("jet_measurement_kernel_from_relative", "jet_measurement_kernel_from_relative"),
    ("jet_measurement_kernel_add", "jet_measurement_kernel_add"),
    ("jet_measurement_kernel_sub", "jet_measurement_kernel_sub"),
    ("jet_measurement_kernel_mul", "jet_measurement_kernel_mul"),
    ("jet_measurement_kernel_div", "jet_measurement_kernel_div"),
    ("jet_measurement_kernel_sqrt", "jet_measurement_kernel_sqrt"),
    ("jet_measurement_kernel_show", "jet_measurement_kernel_show"),
    ("jet_measurement_new", "jet_measurement_new"),
    ("jet_measurement_value", "jet_measurement_value"),
    ("jet_measurement_uncertainty", "jet_measurement_uncertainty"),
    ("jet_measurement_add", "jet_measurement_add"),
    ("jet_measurement_sub", "jet_measurement_sub"),
    ("jet_measurement_mul", "jet_measurement_mul"),
    ("jet_measurement_div", "jet_measurement_div"),
    ("jet_measurement_sqrt", "jet_measurement_sqrt"),
    ("jet_measurement_show", "jet_measurement_show"),
    ("jet_shared_new", "jet_shared_new"),
    ("jet_shared_get", "jet_shared_get"),
    ("jet_shared_set", "jet_shared_set"),
    ("jet_shared_replace", "jet_shared_replace"),
    ("jet_shared_read", "jet_shared_read"),
    ("jet_shared_edit", "jet_shared_edit"),
    ("jet_shared_capture", "jet_shared_capture"),
    ("jet_shared_capture_with", "jet_shared_capture_with"),
    ("jet_shared_capture_txn_plain", "jet_shared_capture_txn_plain"),
    ("jet_shared_capture_txn", "jet_shared_capture_txn"),
    ("jet_shared_try_replace", "jet_shared_try_replace"),
    ("jet_shared_snapshot_value", "jet_shared_snapshot_value"),
    ("jet_shared_guard_read", "jet_shared_guard_read"),
    ("jet_shared_guard_edit", "jet_shared_guard_edit"),
    ("jet_shared_guard_map", "jet_shared_guard_map"),
    ("jet_shared_guard_split", "jet_shared_guard_split"),
    ("jet_stm_begin", "jet_stm_begin"),
    ("jet_shared_read_txn", "jet_shared_read_txn"),
    ("jet_shared_edit_txn", "jet_shared_edit_txn"),
    ("jet_stm_commit", "jet_stm_commit"),
    ("jet_shared_strong_count", "jet_shared_strong_count"),
    ("jet_shared_downgrade", "jet_shared_downgrade"),
    ("jet_shared_weak_upgrade", "jet_shared_weak_upgrade"),
    ("jet_cell_new", "jet_cell_new"),
    ("jet_cell_get", "jet_cell_get"),
    ("jet_cell_set", "jet_cell_set"),
    ("jet_cell_replace", "jet_cell_replace"),
    ("jet_cell_read", "jet_cell_read"),
    ("jet_cell_edit", "jet_cell_edit"),
    ("jet_cell_get_or_set", "jet_cell_get_or_set"),
    ("jet_cell_guard_read", "jet_cell_guard_read"),
    ("jet_cell_guard_edit", "jet_cell_guard_edit"),
    ("jet_cell_guard_map", "jet_cell_guard_map"),
    ("jet_cell_guard_split", "jet_cell_guard_split"),
    ("jet_cell_read_guard_get", "jet_cell_read_guard_get"),
    ("jet_cell_read_guard_read", "jet_cell_read_guard_read"),
    ("jet_cell_edit_guard_get", "jet_cell_edit_guard_get"),
    ("jet_cell_edit_guard_set", "jet_cell_edit_guard_set"),
    ("jet_cell_edit_guard_read", "jet_cell_edit_guard_read"),
    ("jet_cell_edit_guard_edit", "jet_cell_edit_guard_edit"),
    ("jet_std::JetPool::new", "jet_pool_new"),
    ("jet_std::JetPool::add", "jet_pool_add"),
    ("jet_std::JetPool::remove", "jet_pool_remove"),
    ("jet_std::JetPool::ids", "jet_pool_ids"),
    ("jet_pool_get", "jet_pool_get"),
    ("jet_pool_get_mut", "jet_pool_get_mut"),
    ("jet_pool_set", "jet_pool_set"),
    ("jet_authority_covers", "jet_authority_covers"),
    ("jet_authority_workspace", "jet_authority_workspace"),
    ("jet_authority_from_rights", "jet_authority_from_rights"),
    ("jet_authority_with", "jet_authority_with"),
    ("jet_authority_without", "jet_authority_without"),
    ("jet_inline_range_from_int", "jet_inline_range_from_int"),
    ("jet_compute_web_ok", "jet_compute_web_ok"),
    ("jet_compute_web_err", "jet_compute_web_err"),
    ("jet_compute_web_fail", "jet_compute_web_fail"),
    ("jet_compute_web_num", "jet_compute_web_num"),
    ("jet_compute_web_float", "jet_compute_web_float"),
    ("jet_compute_web_shape", "jet_compute_web_shape"),
    ("jet_compute_web_numel", "jet_compute_web_numel"),
    ("jet_compute_web_placement", "jet_compute_web_placement"),
    ("jet_compute_web_tensor", "jet_compute_web_tensor"),
    ("jet_compute_web_device_show", "jet_compute_web_device_show"),
    ("jet_compute_web_device", "jet_compute_web_device"),
    ("jet_compute_web_same_device", "jet_compute_web_same_device"),
    ("jet_compute_webgpu_device", "jet_compute_webgpu_device"),
    ("jet_compute_webgpu_buffer", "jet_compute_webgpu_buffer"),
    ("jet_compute_webgpu_params", "jet_compute_webgpu_params"),
    ("jet_compute_webgpu_pipeline", "jet_compute_webgpu_pipeline"),
    ("jet_compute_webgpu_dispatch", "jet_compute_webgpu_dispatch"),
    ("jet_compute_webgpu_read", "jet_compute_webgpu_read"),
    ("jet_compute_web_values", "jet_compute_web_values"),
    ("jet_compute_web_buffer", "jet_compute_web_buffer"),
    ("jet_compute_web_unary_shader", "jet_compute_web_unary_shader"),
    ("jet_compute_web_binary_shader", "jet_compute_web_binary_shader"),
    ("jet_compute_web_f32_values", "jet_compute_web_f32_values"),
    ("jet_compute_web_gpu_unary", "jet_compute_web_gpu_unary"),
    ("jet_compute_web_gpu_binary", "jet_compute_web_gpu_binary"),
    ("jet_compute_web_gpu_matmul", "jet_compute_web_gpu_matmul"),
    ("jet_compute_web_broadcast_shape", "jet_compute_web_broadcast_shape"),
    ("jet_compute_web_broadcast_values", "jet_compute_web_broadcast_values"),
    ("jet_compute_web_binary", "jet_compute_web_binary"),
    ("jet_compute_web_unary", "jet_compute_web_unary"),
    ("jet_compute_web_matmul", "jet_compute_web_matmul"),
    ("jet_compute_web_upload", "jet_compute_web_upload"),
    ("jet_compute_web_transfer", "jet_compute_web_transfer"),
    ("jet_std_time_sleep_duration_ns", "jet_std_time_sleep_duration_ns"),
    ("jet_time_sleep_until", "jet_time_sleep_until"),
    ("jet_rt_callback", "jet_rt_callback"),
    ("jet_rt_next_deadline", "jet_rt_next_deadline"),
    ("jet_rt_receipt", "jet_rt_receipt"),
    ("jet_rt_cancel", "jet_rt_cancel"),
    ("jet_rt_is_cancelled", "jet_rt_is_cancelled"),
    ("jet_compute_web_sum", "jet_compute_web_sum"),
    ("jet_compute_web_mse", "jet_compute_web_mse"),
    ("jet_compute_web_sgd", "jet_compute_web_sgd"),
    ("jet_compute_web_set", "jet_compute_web_set"),
    ("jet_compute_web_shape_result", "jet_compute_web_shape_result"),
    ("jet_compute_web_call", "jet_compute_web_call"),
    ("jet_time_monotonic_now_ns", "jet_time_monotonic_now_ns"),
    ("jet_time_floor_div", "jet_time_floor_div"),
    ("jet_time_mod", "jet_time_mod"),
    ("jet_time_clamp_i64", "jet_time_clamp_i64"),
    ("jet_time_checked_i64", "jet_time_checked_i64"),
    ("jet_time_saturating_add_i64", "jet_time_saturating_add_i64"),
    ("jet_time_saturating_mul_i64", "jet_time_saturating_mul_i64"),
    ("jet_time_saturating_negated_i64", "jet_time_saturating_negated_i64"),
    ("jet_time_saturating_abs_i64", "jet_time_saturating_abs_i64"),
    ("jet_time_pad", "jet_time_pad"),
    ("jet_time_year_string", "jet_time_year_string"),
    ("jet_time_is_leap", "jet_time_is_leap"),
    ("jet_time_days_in_month", "jet_time_days_in_month"),
    ("jet_time_date", "jet_time_date"),
    ("jet_time_day_number", "jet_time_day_number"),
    ("jet_time_date_from_day_number", "jet_time_date_from_day_number"),
    ("jet_time_add_days", "jet_time_add_days"),
    ("jet_time_add_months", "jet_time_add_months"),
    ("jet_time_date_parse", "jet_time_date_parse"),
    ("jet_time_weekday", "jet_time_weekday"),
    ("jet_time_iso_weekday", "jet_time_iso_weekday"),
    ("jet_time_iso_week_year", "jet_time_iso_week_year"),
    ("jet_time_iso_week", "jet_time_iso_week"),
    ("jet_time_from_iso_week", "jet_time_from_iso_week"),
    ("jet_time_parse_iso_week_date", "jet_time_parse_iso_week_date"),
    ("jet_time_time", "jet_time_time"),
    ("jet_time_parse_time", "jet_time_parse_time"),
    ("jet_time_time_seconds", "jet_time_time_seconds"),
    ("jet_time_time_nanoseconds", "jet_time_time_nanoseconds"),
    ("jet_time_time_from_nanoseconds", "jet_time_time_from_nanoseconds"),
    ("jet_time_date_time_from_ns", "jet_time_date_time_from_ns"),
    ("jet_time_date_time_from_ms", "jet_time_date_time_from_ms"),
    ("jet_time_date_time_from_seconds", "jet_time_date_time_from_seconds"),
    ("jet_time_date_time_from_microseconds", "jet_time_date_time_from_microseconds"),
    ("jet_time_date_time_from_nanoseconds", "jet_time_date_time_from_nanoseconds"),
    ("jet_time_date_time_total_ns", "jet_time_date_time_total_ns"),
    ("jet_time_date_time_date", "jet_time_date_time_date"),
    ("jet_time_date_time_time", "jet_time_date_time_time"),
    ("jet_time_date_time_add_duration", "jet_time_date_time_add_duration"),
    ("jet_time_local_epoch_seconds", "jet_time_local_epoch_seconds"),
    ("jet_time_parse_offset", "jet_time_parse_offset"),
    ("jet_time_offset_string", "jet_time_offset_string"),
    ("jet_time_parse_rfc3339", "jet_time_parse_rfc3339"),
    ("jet_time_date_string", "jet_time_date_string"),
    ("jet_time_time_string", "jet_time_time_string"),
    ("jet_time_date_time_format_rfc3339", "jet_time_date_time_format_rfc3339"),
    ("jet_time_period", "jet_time_period"),
    ("jet_time_period_add", "jet_time_period_add"),
    ("jet_time_period_negated", "jet_time_period_negated"),
    ("jet_time_period_abs", "jet_time_period_abs"),
    ("jet_time_period_month_delta", "jet_time_period_month_delta"),
    ("jet_time_period_add_to_date", "jet_time_period_add_to_date"),
    ("jet_time_period_add_to_datetime", "jet_time_period_add_to_datetime"),
    ("jet_time_period_total_in", "jet_time_period_total_in"),
    ("jet_time_unit_ns", "jet_time_unit_ns"),
    ("jet_time_round_quotient", "jet_time_round_quotient"),
    ("jet_time_round_ns", "jet_time_round_ns"),
    ("jet_time_duration_abs", "jet_time_duration_abs"),
    ("jet_time_duration_negated", "jet_time_duration_negated"),
    ("jet_time_duration_sign", "jet_time_duration_sign"),
    ("jet_time_duration_total_in", "jet_time_duration_total_in"),
    ("jet_time_duration_round", "jet_time_duration_round"),
    ("jet_time_ok", "jet_time_ok"),
    ("jet_time_err", "jet_time_err"),
    ("jet_time_result", "jet_time_result"),
    ("jet_time_text_result", "jet_time_text_result"),
    ("jet_time_range_result", "jet_time_range_result"),
    ("jet_time_some", "jet_time_some"),
    ("jet_time_none", "jet_time_none"),
    ("jet_time_tzif_u32", "jet_time_tzif_u32"),
    ("jet_time_tzif_i64", "jet_time_tzif_i64"),
    ("jet_time_tzif_counts", "jet_time_tzif_counts"),
    ("jet_time_tzif_block_size", "jet_time_tzif_block_size"),
    ("jet_time_parse_tzif", "jet_time_parse_tzif"),
    ("jet_time_zone", "jet_time_zone"),
    ("jet_time_zone_info_at_utc", "jet_time_zone_info_at_utc"),
    ("jet_time_zone_local_parts", "jet_time_zone_local_parts"),
    ("jet_time_zone_local_to_utc", "jet_time_zone_local_to_utc"),
    ("jet_time_zone_local_to_utc_offset", "jet_time_zone_local_to_utc_offset"),
    ("jet_time_zone_start_of_day", "jet_time_zone_start_of_day"),
    ("jet_time_zone_hours_in_day", "jet_time_zone_hours_in_day"),
    ("jet_time_zone_transition", "jet_time_zone_transition"),
    ("jet_time_zoned_from_local", "jet_time_zoned_from_local"),
    ("jet_time_zoned_date", "jet_time_zoned_date"),
    ("jet_time_zoned_time", "jet_time_zoned_time"),
    ("jet_time_replace", "jet_time_replace"),
    ("jet_time_format", "jet_time_format"),
    ("jet_time_format_checked", "jet_time_format_checked"),
    ("jet_time_method", "jet_time_method"),
    ("jet_time_core", "jet_time_core"),
    ("jet_time_add", "jet_time_add"),
    ("jet_time_sub", "jet_time_sub"),
];
fn web_runtime_link(name: &str) -> Option<&'static str> {
    WEB_RUNTIME_LINKS
        .iter()
        .find_map(|(source, local)| (*source == name).then_some(*local))
}
fn js_symbol_expression(symbol: &MirSymbol) -> Result<String, MirWebError> {
    match symbol {
        MirSymbol::Prelude(name) => {
            if let Some(local) = WEB_PRELUDE_LINKS
                .iter()
                .find_map(|(source, local)| (*source == name).then_some(*local))
            {
                return Ok(local.to_string());
            }
            if name.bytes().enumerate().all(|(index, byte)| {
                if index == 0 {
                    byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$'
                } else {
                    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
                }
            }) {
                return Ok(name.clone());
            }
            Err(MirWebError::InvalidMir {
                message: format!(
                    "MIR Prelude symbol {name:?} has no direct local Web binding"
                ),
            })
        }
        MirSymbol::Runtime(name) => web_runtime_link(name)
            .map(str::to_string)
            .ok_or_else(|| MirWebError::InvalidMir {
                message: format!(
                    "MIR Web runtime symbol {name:?} has no binding in the Web JS runtime module"
                ),
            }),
    }
}
fn emit_js_terminator(
    out: &mut String,
    function: &MirFunction,
    block: &MirBasicBlock,
) -> Result<(), MirWebError> {
    match &block.terminator {
        MirTerminator::Jump { target } => writeln!(
            out,
            "        __jet_pred = __jet_block; __jet_block = {}; continue;",
            target.0
        )
        .unwrap(),
        MirTerminator::Branch { condition, then_target, else_target } => writeln!(
            out,
            "        __jet_pred = __jet_block; __jet_block = Boolean(__jet_values.get({})) ? {} : {}; continue;",
            condition.0,
            then_target.0,
            else_target.0
        )
        .unwrap(),
        MirTerminator::Switch { subject, arms, otherwise } => {
            write!(
                out,
                "        __jet_pred = __jet_block; const __jet_subject = __jet_values.get({}); __jet_block = ",
                subject.0
            )
            .unwrap();
            for arm in arms {
                write!(
                    out,
                    "__jet_subject === __jet_values.get({}) ? {} : ",
                    arm.condition.0, arm.target.0
                )
                .unwrap();
            }
            writeln!(out, "{}; continue;", otherwise.0).unwrap();
        }
        MirTerminator::Return { value } => match value {
            Some(value) => {
                let returned = js_return_expression(function, *value);
                writeln!(out, "        return {returned};").unwrap();
            }
            None => out.push_str("        return undefined;\n"),
        },
        MirTerminator::Yield { value, resume } => {
            let Some(generator) = &function.generator else {
                return Err(MirWebError::InvalidMir {
                    message: format!(
                        "MIR function {} has a Yield terminator without generator facts",
                        function.id.0
                    ),
                });
            };
            let Some((_, value_ty, _, _)) = function.values.iter().find(|(id, _, _, _)| id == value) else {
                return Err(MirWebError::InvalidMir {
                    message: format!("MIR yield value {} is missing from value declarations", value.0),
                });
            };
            if value_ty != &generator.item {
                return Err(MirWebError::InvalidMir {
                    message: format!("MIR generator {} yields a value with the wrong type", function.id.0),
                });
            }
            writeln!(
                out,
                "        yield __jet_values.get({}); __jet_pred = __jet_block; __jet_block = {}; continue;",
                value.0, resume.0
            )
            .unwrap();
        }
        MirTerminator::Break { target, value } => {
            if let Some(value) = value {
                writeln!(out, "        __jet_break_value = __jet_values.get({});", value.0).unwrap();
            }
            writeln!(
                out,
                "        __jet_pred = __jet_block; __jet_block = {}; continue;",
                target.0
            )
            .unwrap();
        }
        MirTerminator::Continue { target } => writeln!(
            out,
            "        __jet_pred = __jet_block; __jet_block = {}; continue;",
            target.0
        )
        .unwrap(),
        MirTerminator::Unreachable { reason } => {
            writeln!(out, "        throw new Error({});", js_string(reason)).unwrap();
        }
    }
    Ok(())
}

fn js_return_expression(function: &MirFunction, value: jet_foundation::MIR::MirValueId) -> String {
    let value = format!("__jet_values.get({})", value.0);
    if matches!(function.return_type.kind(), MirTypeKind::Option(_)) {
        format!("jet_clean_option({value})")
    } else {
        value
    }
}


fn js_optional_string(value: Option<&str>) -> String {
    value.map(js_string).unwrap_or_else(|| "null".to_string())
}

fn js_route_field(field: &jet_foundation::App::AppRouteField) -> String {
    let default = field
        .default
        .as_deref()
        .map(js_string)
        .unwrap_or_else(|| "null".to_string());
    format!(
        "Object.freeze({{ name: {}, ty: {}, codec: {}, required: {}, default: {} }})",
        js_string(&field.name),
        js_string(&field.ty),
        js_string(&field.codec),
        field.required,
        default,
    )
}

fn js_route_fields(fields: &[jet_foundation::App::AppRouteField]) -> String {
    let fields = fields.iter().map(js_route_field).collect::<Vec<_>>().join(", ");
    format!("Object.freeze([{}])", fields)
}

fn js_route_loader(loader: Option<&jet_foundation::App::AppRouteLoader>) -> String {
    let Some(loader) = loader else {
        return "null".to_string();
    };
    format!(
        "Object.freeze({{ handler: {}, data_type: {}, dependency: {}, preload: {}, cache_identity: {} }})",
        js_string(&loader.handler),
        js_string(&loader.data_type),
        js_string(&loader.dependency),
        loader.preload,
        js_string(&loader.cache_identity),
    )
}

fn js_route_boundaries(boundaries: &jet_foundation::App::AppRouteBoundaries) -> String {
    format!(
        "Object.freeze({{ pending: {}, not_found: {}, error: {} }})",
        js_optional_string(boundaries.pending.as_deref()),
        js_optional_string(boundaries.not_found.as_deref()),
        js_optional_string(boundaries.error.as_deref()),
    )
}

fn js_route_island(island: Option<&jet_foundation::App::AppIslandFact>) -> String {
    let Some(island) = island else {
        return "null".to_string();
    };
    let captures = island
        .resume_payload
        .captures
        .iter()
        .map(|capture| {
            format!(
                "Object.freeze({{ name: {}, ty: {}, serializable: {} }})",
                js_string(&capture.name),
                js_string(&capture.ty),
                capture.serializable
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "Object.freeze({{ identity: {}, hydration_trigger: {}, resume_payload: Object.freeze({{ captures: Object.freeze([{}]), serializable: {} }}) }})",
        js_string(&island.identity),
        js_string(island.hydration_trigger.as_str()),
        captures,
        island.resume_payload.serializable
    )
}

fn js_route_render_facts(facts: &jet_foundation::App::AppRenderFacts) -> String {
    format!(
        "Object.freeze({{ mode: {}, reason: Object.freeze({}), effects: Object.freeze({}), interactive: {}, loader: {}, form: {}, cache: {}, override_mode: {}, pending_boundary_id: {}, island: {} }})",
        js_string(facts.mode.as_str()),
        json_string_list(&facts.reason),
        json_string_list(&facts.effects),
        facts.interactive,
        js_optional_string(facts.loader.as_deref()),
        js_optional_string(facts.form.as_deref()),
        js_string(facts.cache.as_str()),
        facts
            .override_mode
            .map(|mode| js_string(mode.as_str()))
            .unwrap_or_else(|| "null".to_string()),
        facts
            .pending_boundary_id
            .map(|id| id.get().to_string())
            .unwrap_or_else(|| "null".to_string()),
        js_route_island(facts.island.as_ref())
    )
}

fn js_route_projection(route: &jet_foundation::App::AppRoute) -> String {
    format!(
        "Object.freeze({{ path: {}, handler: {}, provenance: {}, route_identity: {}, path_params: {}, search_params: {}, search_codec: {}, loader: {}, boundaries: {}, precedence: {}, render_facts: {} }})",
        js_string(&route.path),
        js_string(&route.handler),
        js_string(&route.provenance),
        js_string(&route.route_identity),
        js_route_fields(&route.path_params),
        js_route_fields(&route.search_params),
        js_string(&route.search_codec),
        js_route_loader(route.loader.as_ref()),
        js_route_boundaries(&route.boundaries),
        js_string(&route.precedence),
        js_route_render_facts(&route.render_facts),
    )
}
fn js_action_projection(action: &jet_foundation::App::AppAction) -> String {
    format!(
        "Object.freeze({{ name: {}, handler: {}, kind: {}, preload: {}, input: {}, output: {}, error: {}, endpoint: {}, method: {}, csrf: {}, effects: {}, middleware: {}, provenance: {} }})",
        js_string(&action.name),
        js_string(&action.handler),
        js_string(&action.kind),
        action.preload,
        js_string(&action.input_type),
        js_string(&action.output_type),
        js_string(&action.error_type),
        js_string(&action.endpoint),
        js_string(&action.method),
        js_string(&action.csrf),
        json_string_list(&action.effects),
        json_string_list(&action.middleware),
        js_string(&action.provenance),
    )
}


fn emit_js_routes(out: &mut String, program: &MirProgram) {
    match &program.facts.web_app {
        Some(graph) => {
            // The emitted route/action projections are views of the one
            // sema-owned graph.  Keep the complete graph available to the
            // browser bootstrap so it can pass the checked contracts to the
            // canonical router/server-function kernels.
            out.push_str("export const jet_app_graph = Object.freeze(");
            out.push_str(&graph.to_json());
            out.push_str(");\n");
            out.push_str("export const jet_routes = Object.freeze({\n");
            for route in &graph.routes {
                writeln!(
                    out,
                    "  {}: {},",
                    js_string(&route.path),
                    js_route_projection(route),
                )
                .unwrap();
            }
            out.push_str("});\n");
            out.push_str("export const jet_actions = Object.freeze([\n");
            for action in &graph.actions {
                writeln!(out, "  {},", js_action_projection(action)).unwrap();
            }
            out.push_str("]);\n");
        }
        None => {
            out.push_str("export const jet_app_graph = null;\n");
            out.push_str("export const jet_routes = Object.freeze({});\n");
            out.push_str("export const jet_actions = Object.freeze([]);\n");
        }
    }
    out.push_str("globalThis.__jetAppGraph = jet_app_graph;\n");
    out.push_str(
        "\n// The browser adapter consumes this checked graph and delegates route\n\
         // matching/data production to the native server graph.\n\
         export const jet_web_app = jetDom.createWebApp({ graph: jet_app_graph, routes: jet_routes, actions: jet_actions });\n\
         export function jet_bootstrap_web(options = {}) {\n\
           return jetDom.bootstrapWebApp(jet_web_app, options);\n\
         }\n\
         export const jet_web_bootstrap = jet_bootstrap_web;\n\
         if (typeof document !== \"undefined\" && jet_app_graph !== null\n\
             && !globalThis.__jetWebDisableAutoBootstrap\n\
             && document.getElementById(\"jet-route-data\")) {\n\
           queueMicrotask(() => {\n\
             if (!globalThis.__jetWebApp) jet_bootstrap_web();\n\
           });\n\
         }\n",
    );
}


fn manifest_partitions(
    functions: &[&MirFunction],
) -> Result<BTreeMap<String, WebBucket>, MirWebError> {
    let mut partitions = BTreeMap::new();
    for function in functions {
        if !function.target_applicability.web {
            continue;
        }
        let Some(bucket) = function.web_bucket else {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "manifest function {} is Web-applicable but has no checked Web bucket",
                    function.id.0
                ),
            });
        };
        if partitions.insert(function.key.clone(), bucket).is_some() {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "manifest contains duplicate Web function key `{}`",
                    function.key
                ),
            });
        }
    }
    Ok(partitions)
}

fn emit_manifest(
    program: &MirProgram,
    target: &MirWebTarget,
    functions: &[&MirFunction],
    artifact: &MirArtifactPlan,
    entry: jet_foundation::MIR::MirFunctionId,
    artifact_identity: &str,
) -> Result<String, MirWebError> {
    let mut out = String::from("{\n");
    out.push_str("  \"schema\": \"mir-web-v1\",\n");
    let partitions = manifest_partitions(functions)?;
    out.push_str("  \"status\": \"m2\",\n");
    out.push_str("  \"partitions\": {\n");
    for (index, (key, bucket)) in partitions.iter().enumerate() {
        if index != 0 {
            out.push_str(",\n");
        }
        write!(
            out,
            "    {}: {}",
            json_string(key),
            optional_web_bucket(Some(*bucket))
        )
        .unwrap();
    }
    out.push_str("\n  },\n");
    writeln!(out, "  \"package\": {},", json_string(&program.package_identity)).unwrap();
    writeln!(
        out,
        "  \"target\": {{ \"triple\": {}, \"pointer_size\": {}, \"pointer_alignment\": {} }},",
        json_string(&target.layout.triple),
        target.layout.pointer_size,
        target.layout.pointer_alignment
    )
    .unwrap();
    writeln!(out, "  \"artifact_id\": {},", artifact.id.0).unwrap();
    writeln!(out, "  \"entry\": {},", entry.0).unwrap();
    out.push_str("  \"functions\": [\n");
    let mut sorted_functions = functions.to_vec();
    sorted_functions.sort_by_key(|function| function.id);
    for (index, function) in sorted_functions.iter().enumerate() {
        if index != 0 {
            out.push_str(",\n");
        }
        let generic_params = json_generic_params(&function.generic_params, program)?;
        write!(
            out,
            "    {{ \"id\": {}, \"key\": {}, \"module\": {}, \"name\": {}, \"kind\": {}, \"visibility\": {}, \"web_bucket\": {}, \"web_marker\": {}, \"target_applicability\": {{ \"web\": {} }}, \"wasm_export\": {}, \"generic_params\": {}, \"web_param_reconstructions\": {} }}",
            function.id.0,
            json_string(&function.key),
            json_string(&function.module),
            json_string(&function.name),
            json_string(function_kind_descriptor(function.kind)),
            json_string(visibility_descriptor(function.visibility)),
            optional_web_bucket(function.web_bucket),
            optional_web_marker(function.web_marker),
            function.target_applicability.web,
            is_wasm_export(function),
            generic_params,
            json_reconstructions(&function.web_param_reconstructions)
        )
        .unwrap();
    }
    out.push_str("\n  ],\n  \"core_calls\": [\n");
    let mut core_calls = program.core_calls.iter().collect::<Vec<_>>();
    core_calls.sort_by_key(|call| call.id);
    for (index, call) in core_calls.iter().enumerate() {
        if index != 0 {
            out.push_str(",\n");
        }
        write!(
            out,
            "    {{ \"id\": {}, \"key\": {}, \"symbol\": {}, \"arity\": {}, \"max_arity\": {}, \"fallibility\": {}, \"effect\": {}, \"pure_route\": {}, \"interpreter_route\": {} }}",
            call.id.0,
            json_string(&call.key),
            json_string(core_symbol(call.symbol)),
            call.arity,
            call.max_arity,
            json_string(core_call_fallibility_descriptor(call.fallibility)),
            optional_effect_descriptor(call.effect.as_ref()),
            json_string(core_call_pure_route_descriptor(call.pure_route)),
            json_string(&core_call_interpreter_route_descriptor(call.interpreter_route))
        )
        .unwrap();
    }
    out.push_str("\n  ],\n  \"prelude_calls\": [\n");
    let mut prelude_calls = program.prelude_calls.iter().collect::<Vec<_>>();
    prelude_calls.sort_by_key(|call| call.id);
    for (index, call) in prelude_calls.iter().enumerate() {
        if index != 0 {
            out.push_str(",\n");
        }
        write!(
            out,
            "    {{ \"id\": {}, \"module\": {}, \"member\": {}, \"family\": {}, \"symbol\": {}, \"abi\": {}, \"arity\": {}, \"max_arity\": {} }}",
            call.id.0,
            json_string(&call.module),
            json_string(&call.member),
            json_string(prelude_family_descriptor(call.family)),
            json_string(call.symbol.name()),
            json_string(prelude_abi_descriptor(call.abi)),
            call.signature.arity,
            call.signature.max_arity
        )
        .unwrap();
    }
    out.push_str("\n  ],\n  \"types\": [\n");
    for (index, type_def) in program.types.iter().enumerate() {
        if index != 0 {
            out.push_str(",\n");
        }
        let generic_params = json_generic_params(&type_def.generic_params, program)?;
        write!(
            out,
            "    {{ \"id\": {}, \"key\": {}, \"name\": {}, \"kind\": {}, \"generic_params\": {} }}",
            type_def.id.0,
            json_string(&type_def.key),
            json_string(&type_def.name),
            json_string(&type_def_kind_descriptor(&type_def.kind)),
            generic_params
        )
        .unwrap();
    }
    out.push_str("\n  ],\n  \"modules\": ");
    out.push_str(&json_modules(program));
    out.push_str(",\n  \"imports\": ");
    out.push_str(&json_imports(program));
    out.push_str(",\n  \"traits\": ");
    out.push_str(&json_traits(program));
    out.push_str(",\n  \"impls\": ");
    out.push_str(&json_impls(program));
    out.push_str(",\n  \"constants\": ");
    out.push_str(&json_constants(program));
    out.push_str(",\n  \"jobs\": ");
    out.push_str(&json_jobs(program));
    out.push_str(",\n  \"harnesses\": ");
    out.push_str(&json_harnesses(program));
    out.push_str(",\n  \"artifacts\": ");
    out.push_str(&json_artifacts(program, artifact.id, artifact_identity)?);
    out.push_str(",\n  \"artifact\": ");
    out.push_str(&json_artifact(artifact, program, artifact_identity)?);
    out.push_str(",\n  \"entry_spec\": ");
    out.push_str(&json_entry_spec(artifact.entry.as_ref(), program)?);
    out.push_str(",\n  \"app\": ");
    match &program.facts.web_app {
        Some(graph) => out.push_str(&graph.to_json()),
        None => out.push_str("null"),
    }
    out.push_str("\n}\n");
    Ok(out)
}

fn json_generic_params(
    params: &[jet_foundation::MIR::MirGenericParam],
    program: &MirProgram,
) -> Result<String, MirWebError> {
    let rows = params
        .iter()
        .map(|param| {
            let bounds = param
                .bounds
                .iter()
                .map(|bound| {
                    let Some(trait_def) = program.traits.iter().find(|trait_def| trait_def.id == bound.id) else {
                        return Err(MirWebError::InvalidMir {
                            message: format!(
                                "MIR generic parameter {} references missing trait {}",
                                param.name, bound.id.0
                            ),
                        });
                    };
                    Ok(format!(
                        "{{\"id\":{},\"name\":{}}}",
                        bound.id.0,
                        json_string(&trait_def.name)
                    ))
                })
                .collect::<Result<Vec<_>, MirWebError>>()?;
            Ok(format!(
                "{{\"name\":{},\"bounds\":[{}]}}",
                json_string(&param.name),
                bounds.join(",")
            ))
        })
        .collect::<Result<Vec<_>, MirWebError>>()?;
    Ok(format!("[{}]", rows.join(",")))
}

fn json_modules(program: &MirProgram) -> String {
    let rows = program
        .modules
        .iter()
        .map(|module| {
            let imports = module
                .imports
                .iter()
                .map(|id| id.0.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let items = module
                .item_order
                .iter()
                .map(|item| json_string(&item_ref_descriptor(*item)))
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"id\":{},\"key\":{},\"name\":{},\"path\":{},\"source_file\":{},\"imports\":[{}],\"item_order\":[{}]}}",
                module.id.0,
                json_string(&module.key),
                json_string(&module.name),
                json_string(&module.path),
                module.source_file.0,
                imports,
                items
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", rows)
}

fn json_imports(program: &MirProgram) -> String {
    let rows = program
        .imports
        .iter()
        .map(|import| {
            format!(
                "{{\"id\":{},\"module\":{},\"visibility\":{},\"alias\":{},\"kind\":{}}}",
                import.id.0,
                import.module.0,
                json_string(visibility_descriptor(import.visibility)),
                json_string(&import.alias),
                json_string(&import_kind_descriptor(&import.kind))
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", rows)
}

fn json_traits(program: &MirProgram) -> String {
    let rows = program
        .traits
        .iter()
        .map(|trait_def| {
            let associated_types = trait_def
                .associated_types
                .iter()
                .map(|associated| json_string(&associated.name))
                .collect::<Vec<_>>()
                .join(",");
            let methods = trait_def
                .methods
                .iter()
                .map(|method| {
                    format!(
                        "{{\"id\":{},\"name\":{},\"self_access\":{},\"default\":{}}}",
                        method.id.0,
                        json_string(&method.name),
                        optional_access(method.self_access),
                        optional_function_id(method.default)
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"id\":{},\"module\":{},\"key\":{},\"name\":{},\"visibility\":{},\"associated_types\":[{}],\"methods\":[{}]}}",
                trait_def.id.0,
                trait_def.module.0,
                json_string(&trait_def.key),
                json_string(&trait_def.name),
                json_string(visibility_descriptor(trait_def.visibility)),
                associated_types,
                methods
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", rows)
}

fn json_impls(program: &MirProgram) -> String {
    let rows = program
        .impls
        .iter()
        .map(|impl_def| {
            let trait_ref = impl_def
                .trait_ref
                .as_ref()
                .map(|trait_ref| {
                    format!(
                        "{{\"id\":{},\"name\":{}}}",
                        trait_ref.id.0,
                        json_string(&trait_ref.name)
                    )
                })
                .unwrap_or_else(|| "null".to_string());
            let associated_types = impl_def
                .associated_types
                .iter()
                .map(|associated| {
                    format!(
                        "{{\"name\":{},\"type\":{}}}",
                        json_string(&associated.name),
                        json_string(&type_descriptor(&associated.ty))
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let methods = impl_def
                .methods
                .iter()
                .map(|id| id.0.to_string())
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"id\":{},\"module\":{},\"key\":{},\"self_type\":{},\"trait\":{},\"associated_types\":[{}],\"methods\":[{}],\"delegation\":{},\"compiler_generated\":{},\"serde\":{},\"operator_rhs\":{},\"target_applicability\":{{\"web\":{}}}}}",
                impl_def.id.0,
                impl_def.module.0,
                json_string(&impl_def.key),
                json_string(&type_descriptor(&impl_def.self_type)),
                trait_ref,
                associated_types,
                methods,
                impl_def.delegation.map(|id| id.0.to_string()).unwrap_or_else(|| "null".to_string()),
                impl_def.compiler_generated,
                optional_serde_codec_descriptor(impl_def.serde.as_ref()),
                impl_def.operator_rhs.as_ref().map(|ty| json_string(&type_descriptor(ty))).unwrap_or_else(|| "null".to_string()),
                impl_def.target_applicability.web
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", rows)
}

fn json_constants(program: &MirProgram) -> String {
    let rows = program
        .constants
        .iter()
        .map(|constant| {
            format!(
                "{{\"id\":{},\"module\":{},\"key\":{},\"name\":{},\"visibility\":{},\"type\":{},\"value\":{}}}",
                constant.id.0,
                constant.module.0,
                json_string(&constant.key),
                json_string(&constant.name),
                json_string(visibility_descriptor(constant.visibility)),
                json_string(&type_descriptor(&constant.ty)),
                json_string(&constant_descriptor(&constant.value))
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", rows)
}

fn json_jobs(program: &MirProgram) -> String {
    let rows = program.jobs.iter().map(json_job).collect::<Vec<_>>().join(",");
    format!("[{}]", rows)
}

fn json_job(job: &MirJob) -> String {
    format!(
        "{{\"id\":{},\"function\":{},\"name\":{},\"scope\":{},\"schedule\":{},\"inputs\":{},\"dispatch\":{},\"after\":{},\"parallel\":{},\"packages\":{},\"working_directory\":{},\"input_paths\":{},\"output_paths\":{},\"skip\":{},\"cache\":{},\"limits\":{}}}",
        job.id.0,
        job.function.0,
        json_string(&job.name),
        json_string(job_scope_descriptor(job.scope)),
        optional_job_schedule_descriptor(job.schedule.as_ref()),
        json_cli_inputs(&job.inputs),
        json_string(job_dispatch_descriptor(job.dispatch)),
        json_string_list(&job.after),
        job.parallel,
        json_string_list(&job.packages),
        job.working_directory
            .as_ref()
            .map(|value| json_string(value))
            .unwrap_or_else(|| "null".to_string()),
        json_string_list(&job.input_paths),
        json_string_list(&job.output_paths),
        optional_job_skip_descriptor(job.skip.as_ref()),
        json_string(job_cache_policy_descriptor(job.cache)),
        json_string(&job_limits_descriptor(&job.limits))
    )
}

fn json_harnesses(program: &MirProgram) -> String {
    let rows = program
        .harnesses
        .iter()
        .map(|harness| {
            let tests = harness
                .tests
                .iter()
                .map(|id| id.0.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let output_checks = harness
                .output_checks
                .iter()
                .map(|check| {
                    format!(
                        "{{\"id\":{},\"name\":{},\"function\":{}}}",
                        check.id.0,
                        json_string(&check.name),
                        check.function.0
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            let coverage_points = harness
                .coverage_points
                .iter()
                .map(|point| {
                    format!(
                        "{{\"id\":{},\"function\":{},\"block\":{}}}",
                        point.id.0, point.function.0, point.block.0
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            format!(
                "{{\"id\":{},\"kind\":{},\"tests\":[{}],\"output_checks\":[{}],\"selected_test\":{},\"coverage_points\":[{}],\"command_override\":{}}}",
                harness.id.0,
                json_string(harness_kind_descriptor(harness.kind)),
                tests,
                output_checks,
                harness
                    .selected_test
                    .map(|id| id.0.to_string())
                    .unwrap_or_else(|| "null".to_string()),
                coverage_points,
                harness.command_override
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", rows)
}

fn json_artifacts(
    program: &MirProgram,
    selected_artifact: jet_foundation::MIR::MirArtifactId,
    selected_identity: &str,
) -> Result<String, MirWebError> {
    let rows = program
        .artifacts
        .iter()
        .map(|artifact| {
            let identity = if artifact.id == selected_artifact {
                selected_identity
            } else {
                artifact.artifact_identity.as_str()
            };
            json_artifact(artifact, program, identity)
        })
        .collect::<Result<Vec<_>, MirWebError>>()?
        .join(",");
    Ok(format!("[{}]", rows))
}

fn json_artifact(
    artifact: &MirArtifactPlan,
    program: &MirProgram,
    artifact_identity: &str,
) -> Result<String, MirWebError> {
    let modules = artifact.modules.iter().map(|id| id.0.to_string()).collect::<Vec<_>>().join(",");
    let links = artifact.links.iter().map(|id| id.0.to_string()).collect::<Vec<_>>().join(",");
    let jobs = artifact.jobs.iter().map(|id| id.0.to_string()).collect::<Vec<_>>().join(",");
    let runtime_parts = artifact
        .runtime_parts
        .iter()
        .map(|part| json_string(runtime_part_descriptor(*part)))
        .collect::<Vec<_>>()
        .join(",");
    let exports = artifact
        .exports
        .iter()
        .map(|export| {
            format!(
                "{{\"symbol\":{},\"function\":{},\"abi\":{}}}",
                json_string(&export.symbol),
                export.function.0,
                json_string(&foreign_abi_descriptor(&export.abi))
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    Ok(format!(
        "{{\"id\":{},\"kind\":{},\"name\":{},\"target\":{},\"mode\":{},\"modules\":[{}],\"links\":[{}],\"jobs\":[{}],\"runtime_parts\":[{}],\"exports\":[{}],\"provider_identity\":{},\"closure_identity\":{},\"artifact_identity\":{},\"entry\":{},\"harness\":{}}}",
        artifact.id.0,
        json_string(artifact.kind.as_str()),
        json_string(&artifact.name),
        json_string(artifact.target.as_str()),
        json_string(artifact.mode.as_str()),
        modules,
        links,
        jobs,
        runtime_parts,
        exports,
        json_string(&artifact.provider_identity),
        json_string(&artifact.closure_identity),
        json_string(artifact_identity),
        json_entry_spec(artifact.entry.as_ref(), program)?,
        artifact.harness.map(|id| id.0.to_string()).unwrap_or_else(|| "null".to_string())
    ))
}

fn json_entry_spec(entry: Option<&MirEntrySpec>, program: &MirProgram) -> Result<String, MirWebError> {
    let Some(entry) = entry else {
        return Ok("null".to_string());
    };
    let cli = match &entry.cli {
        None => "null".to_string(),
        Some(cli) => {
            let commands = cli
                .commands
                .iter()
                .map(|command| {
                    if !program.functions.iter().any(|function| function.id == command.function) {
                        return Err(MirWebError::MissingUserFunction { function: command.function.0 });
                    }
                    Ok(format!(
                        "{{\"name\":{},\"description\":{},\"function\":{},\"receiver\":{},\"inputs\":{}}}",
                        json_string(&command.name),
                        command
                            .description
                            .as_ref()
                            .map(|value| json_string(value))
                            .unwrap_or_else(|| "null".to_string()),
                        command.function.0,
                        command.receiver.map(|id| id.0.to_string()).unwrap_or_else(|| "null".to_string()),
                        json_cli_inputs(&command.inputs)
                    ))
                })
                .collect::<Result<Vec<_>, MirWebError>>()?
                .join(",");
            format!(
                "{{\"description\":{},\"inputs\":{},\"commands\":[{}],\"standard\":{}}}",
                cli.description
                    .as_ref()
                    .map(|value| json_string(value))
                    .unwrap_or_else(|| "null".to_string()),
                json_cli_inputs(&cli.inputs),
                commands,
                cli.standard
            )
        }
    };
    Ok(format!(
        "{{\"kind\":{},\"function\":{},\"cli\":{},\"output\":{},\"initialize_environment\":{},\"initialize_gc\":{},\"serves_until_stopped\":{},\"package_version\":{}}}",
        json_string(entry_kind_descriptor(entry.kind)),
        entry.function.map(|id| id.0.to_string()).unwrap_or_else(|| "null".to_string()),
        cli,
        json_string(entry_output_descriptor(entry.output)),
        entry.initialize_environment,
        entry.initialize_gc,
        entry.serves_until_stopped,
        json_string(&entry.package_version)
    ))
}

fn json_cli_inputs(inputs: &[jet_foundation::MIR::MirCliInput]) -> String {
    let rows = inputs
        .iter()
        .map(|input| {
            format!(
                "{{\"parameter\":{},\"name\":{},\"label\":{},\"type\":{},\"zone\":{},\"short\":{},\"env\":{},\"help\":{},\"metavar\":{},\"shape\":{},\"positional\":{},\"variadic\":{}}}",
                input.parameter,
                json_string(&input.name),
                json_string(&input.label),
                json_string(&type_descriptor(&input.ty)),
                json_string(input.zone.as_str()),
                input
                    .short
                    .as_ref()
                    .map(|value| json_string(value))
                    .unwrap_or_else(|| "null".to_string()),
                input
                    .env
                    .as_ref()
                    .map(|value| json_string(value))
                    .unwrap_or_else(|| "null".to_string()),
                json_string(&input.help),
                input
                    .metavar
                    .as_ref()
                    .map(|value| json_string(value))
                    .unwrap_or_else(|| "null".to_string()),
                json_cli_shape(&input.shape),
                input
                    .positional
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "null".to_string()),
                input.variadic
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("[{}]", rows)
}

fn json_cli_shape(shape: &jet_foundation::MIR::MirCliInputShape) -> String {
    match shape {
        jet_foundation::MIR::MirCliInputShape::Flag => "{\"kind\":\"flag\"}".to_string(),
        jet_foundation::MIR::MirCliInputShape::Value {
            kind,
            optional,
            default,
        } => format!(
            "{{\"kind\":{},\"optional\":{},\"default\":{}}}",
            json_string(cli_value_kind_descriptor(*kind)),
            optional,
            json_cli_default(default.as_ref())
        ),
    }
}

fn json_cli_default(default: Option<&jet_foundation::MIR::MirCliDefault>) -> String {
    match default {
        None => "null".to_string(),
        Some(jet_foundation::MIR::MirCliDefault::TypeDefault) => "{\"kind\":\"type-default\"}".to_string(),
        Some(jet_foundation::MIR::MirCliDefault::Value(value)) => format!(
            "{{\"kind\":\"value\",\"value\":{}}}",
            json_string(&constant_descriptor(value))
        ),
    }
}

fn json_string_list(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|value| json_string(value))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn optional_function_id(id: Option<jet_foundation::MIR::MirFunctionId>) -> String {
    id.map(|id| id.0.to_string()).unwrap_or_else(|| "null".to_string())
}

fn optional_access(access: Option<MirAccess>) -> String {
    access
        .map(|access| json_string(access_descriptor(access)))
        .unwrap_or_else(|| "null".to_string())
}

fn emit_source_map(assets: &MirWebAssets) -> String {
    let sources = assets.source_names.iter().map(|name| json_string(name)).collect::<Vec<_>>().join(",");
    let contents = assets.source_contents.iter().map(|content| json_string(content)).collect::<Vec<_>>().join(",");
    format!("{{\"version\":3,\"file\":\"app.js\",\"sources\":[{sources}],\"sourcesContent\":[{contents}],\"names\":[],\"mappings\":\"\"}}\n")
}

fn emit_command_record(
    program: &MirProgram,
    functions: &[&MirFunction],
    artifact: &MirArtifactPlan,
    entry: jet_foundation::MIR::MirFunctionId,
) -> Result<Vec<u8>, MirWebError> {
    let entry_type = functions
        .iter()
        .find(|function| function.id == entry)
        .map(|function| function.name.as_str())
        .unwrap_or(artifact.name.as_str());
    let (description, standard, version, inputs, commands) = match artifact.entry.as_ref() {
        Some(entry) => match entry.cli.as_ref() {
            Some(cli) => (
                cli.description.as_deref(),
                cli.standard,
                Some(entry.package_version.as_str()),
                cli.inputs.as_slice(),
                cli.commands.as_slice(),
            ),
            None => (
                None,
                false,
                Some(entry.package_version.as_str()),
                &[][..],
                &[][..],
            ),
        },
        None => (None, false, None, &[][..], &[][..]),
    };
    let mut jobs = Vec::with_capacity(artifact.jobs.len());
    for job_id in &artifact.jobs {
        let Some(job) = program.jobs.iter().find(|job| job.id == *job_id) else {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "MIR WebApplication artifact {} references missing job {} while encoding its command record",
                    artifact.id.0, job_id.0
                ),
            });
        };
        jobs.push(job.clone());
    }
    Ok(jet_foundation::CLISchema::encode_mir_record(
        entry_type,
        description,
        standard,
        version,
        inputs,
        commands,
        &jobs,
    ))
}

fn function_in_bucket(function: &MirFunction, bucket: WebBucket) -> bool {
    function.web_bucket == Some(bucket)
}

fn is_wasm_export(function: &MirFunction) -> bool {
    match function.web_marker {
        Some(WebPartitionMarker::WasmExport) => true,
        Some(WebPartitionMarker::JS) | Some(WebPartitionMarker::Wasm) | None => false,
    }
}

fn optional_web_bucket(bucket: Option<WebBucket>) -> String {
    match bucket {
        Some(WebBucket::JS) => json_string("JS"),
        Some(WebBucket::Wasm) => json_string("Wasm"),
        None => "null".to_string(),
    }
}

fn optional_web_marker(marker: Option<WebPartitionMarker>) -> String {
    match marker {
        Some(WebPartitionMarker::JS) => json_string("JS"),
        Some(WebPartitionMarker::Wasm) => json_string("Wasm"),
        Some(WebPartitionMarker::WasmExport) => json_string("WasmExport"),
        None => "null".to_string(),
    }
}
fn js_call_values(
    program: &MirProgram,
    function: &MirFunction,
    args: &[jet_foundation::MIR::MirCallArg],
    write_alias: bool,
) -> Result<Vec<String>, MirWebError> {
    args.iter()
        .map(|arg| js_call_arg_value(program, function, arg, write_alias))
        .collect()
}

fn js_call_arg_value(
    program: &MirProgram,
    function: &MirFunction,
    arg: &jet_foundation::MIR::MirCallArg,
    write_alias: bool,
) -> Result<String, MirWebError> {
    let ty = mir_function_value_type(function, arg.value)?;
    let source = match arg.access {
        MirAccess::Move => js_move_value_expression(arg.value),
        MirAccess::Read => match arg.place {
            Some(place) => js_read_place_storage_expression(program, function, place)?,
            None => format!("__jet_values.get({})", arg.value.0),
        },
        MirAccess::Write => {
            let Some(place) = arg.place else {
                return Err(MirWebError::InvalidMir {
                    message: format!(
                        "MIR Web write call argument {} has no checked place",
                        arg.value.0
                    ),
                });
            };
            if write_alias {
                format!(
                    "jet_web_write_arg({})",
                    js_place_cell_expression(program, function, place)?
                )
            } else {
                js_read_place_storage_expression(program, function, place)?
            }
        }
    };
    let mut value = if matches!(arg.access, MirAccess::Read)
        && !arg.implicit_clone
        && !arg.shared_auto_clone
    {
        js_copy_expression(program, ty, source)
    } else {
        source
    };
    if arg.implicit_clone || arg.shared_auto_clone {
        let copy_source = if arg.access == MirAccess::Write && write_alias {
            format!("jet_web_call_arg_value({value})")
        } else {
            value
        };
        value = js_copy_expression(program, ty, copy_source);
    }
    if let Some(coercion) = &arg.fn_coercion {
        if let Some(type_id) = coercion.ty.identity {
            ensure_type_instance(program, type_id)?;
        }
        // JavaScript function values already use the canonical callable ABI;
        // native boxing/coercion does not require a second representation.
    }
    if arg.widen_fixed_to_list {
        value = format!("Array.from({})", value);
    }
    if let Some(coercion) = &arg.widen_to_union {
        ensure_type_instance(program, coercion.union)?;
        let Some(type_def) = program.types.iter().find(|type_def| type_def.id == coercion.union) else {
            return Err(MirWebError::InvalidMir {
                message: format!(
                    "MIR call argument {} widens to missing union type {}",
                    arg.value.0, coercion.union.0
                ),
            });
        };
        match &type_def.kind {
            MirTypeDefKind::Enum { variants, .. } if variants.iter().any(|variant| variant.name == coercion.variant) => {}
            MirTypeDefKind::Enum { .. } => {
                return Err(MirWebError::InvalidMir {
                    message: format!(
                        "MIR call argument {} widens to union {} with missing variant {}",
                        arg.value.0, coercion.union.0, coercion.variant
                    ),
                });
            }
            MirTypeDefKind::Struct { .. }
            | MirTypeDefKind::Distinct { .. }
            | MirTypeDefKind::Alias { .. }
            | MirTypeDefKind::UnitFamily { .. } => {
                return Err(MirWebError::InvalidMir {
                    message: format!(
                        "MIR call argument {} widens to non-enum union type {}",
                        arg.value.0, coercion.union.0
                    ),
                });
            }
        }
        value = format!(
            "{{ tag: {}, values: [{}] }}",
            js_string(&coercion.variant),
            value
        );
    }
    match arg.box_as_trait {
        Some(_) => {
            // Canonical Web trait values are already their concrete JS objects.
        }
        None => {}
    }
    if arg.spread {
        Ok(format!("...{}", value))
    } else {
        Ok(value)
    }
}

fn type_descriptor(ty: &MirType) -> String {
    format!(
        "{}:abi={},size={},align={}",
        ty.name(),
        mir_abi_descriptor(ty.layout.abi),
        mir_size_descriptor(ty.layout.size),
        mir_size_descriptor(ty.layout.align)
    )
}

fn mir_abi_descriptor(abi: jet_foundation::MIR::MirAbi) -> String {
    match abi {
        jet_foundation::MIR::MirAbi::Scalar(kind) => {
            format!("scalar-{}", mir_scalar_kind_descriptor(kind))
        }
        jet_foundation::MIR::MirAbi::Aggregate => "aggregate".to_string(),
        jet_foundation::MIR::MirAbi::Sequence => "sequence".to_string(),
        jet_foundation::MIR::MirAbi::Function => "function".to_string(),
        jet_foundation::MIR::MirAbi::Nominal => "nominal".to_string(),
        jet_foundation::MIR::MirAbi::Dynamic => "dynamic".to_string(),
        jet_foundation::MIR::MirAbi::Never => "never".to_string(),
    }
}

fn mir_scalar_kind_descriptor(kind: jet_foundation::MIR::MirScalarKind) -> &'static str {
    match kind {
        jet_foundation::MIR::MirScalarKind::Int => "int",
        jet_foundation::MIR::MirScalarKind::Float => "float",
        jet_foundation::MIR::MirScalarKind::Float32 => "float32",
        jet_foundation::MIR::MirScalarKind::Bool => "bool",
        jet_foundation::MIR::MirScalarKind::Char => "char",
        jet_foundation::MIR::MirScalarKind::Pointer => "pointer",
    }
}

fn mir_size_descriptor(size: jet_foundation::MIR::MirSize) -> String {
    match size {
        jet_foundation::MIR::MirSize::Static(bytes) => format!("static-{}", bytes),
        jet_foundation::MIR::MirSize::Dynamic => "dynamic".to_string(),
    }
}

fn mir_int_width_descriptor(width: Option<(bool, u8)>) -> String {
    match width {
        Some((signed, bits)) => format!("{}-{}", if signed { "signed" } else { "unsigned" }, bits),
        None => "unspecified".to_string(),
    }
}

fn mir_char_descriptor(value: char) -> String {
    value.escape_default().collect()
}

fn mir_bytes_descriptor(value: &[u8]) -> String {
    value.iter().map(|byte| format!("{byte:02x}")).collect::<Vec<_>>().join("")
}

fn mir_float_descriptor(value: f64, is_f32: bool) -> String {
    if is_f32 {
        format!("{}", value as f32)
    } else {
        value.to_string()
    }
}

fn js_string(value: &str) -> String {
    format!("\"{}\"", escape_string(value))
}

fn json_string(value: &str) -> String {
    format!("\"{}\"", escape_string(value))
}

fn escape_string(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if c.is_control() => write!(out, "\\u{:04x}", c as u32).unwrap(),
            c => out.push(c),
        }
    }
    out
}

fn default_index_html() -> String {
    "<!doctype html><html><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>Jet app</title></head><body><script type=\"module\" src=\"./app.js\"></script></body></html>\n".to_string()
}

fn constant_descriptor(constant: &MirConstant) -> String {
    match constant {
        MirConstant::Int { value, width, spelling } => {
            format!(
                "int:{}:{}:{}",
                value,
                mir_int_width_descriptor(*width),
                spelling.as_deref().unwrap_or("")
            )
        }
        MirConstant::Float { value, f32, spelling } => {
            format!(
                "float:{}:{}:{}",
                mir_float_descriptor(*value, *f32),
                f32,
                spelling.as_deref().unwrap_or("")
            )
        }
        MirConstant::Bool(value) => format!("bool:{value}"),
        MirConstant::Char(value) => format!("char:{}", mir_char_descriptor(*value)),
        MirConstant::String(value) => format!("string:{value}"),
        MirConstant::Bytes(value) => format!("bytes:{}", mir_bytes_descriptor(value)),
        MirConstant::Unit => "unit".to_string(),
        MirConstant::BigInt(value) => format!("bigint:{value}"),
        MirConstant::List(values) => format!(
            "list:{}",
            values.iter().map(constant_descriptor).collect::<Vec<_>>().join(",")
        ),
        MirConstant::Map(values) => format!(
            "map:{}",
            values
                .iter()
                .map(|(key, value)| format!("{}={}", const_key_descriptor(key), constant_descriptor(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        MirConstant::Struct { type_name, fields } => format!(
            "struct:{type_name}:{}",
            fields
                .iter()
                .map(|(name, value)| format!("{name}={}", constant_descriptor(value)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        MirConstant::Enum { type_name, variant, args } => format!(
            "enum:{type_name}:{variant}:{}",
            args.iter()
                .map(|(name, value)| format!(
                    "{}={}",
                    name.as_deref().unwrap_or(""),
                    constant_descriptor(value)
                ))
                .collect::<Vec<_>>()
                .join(",")
        ),
        MirConstant::Present(value) => format!("present:{}", constant_descriptor(value)),
        MirConstant::Failed(report) => format!("failed:{}", const_report_descriptor(report)),
    }
}

fn const_report_descriptor(report: &MirConstReport) -> String {
    match report {
        MirConstReport::Clean(ty) => format!("clean:{}", ty.identity_key()),
        MirConstReport::Told(value) => format!("told:{}", constant_descriptor(value)),
    }
}

fn const_key_descriptor(key: &MirConstKey) -> String {
    match key {
        MirConstKey::Int(value) => format!("int:{value}"),
        MirConstKey::String(value) => format!("string:{value}"),
        MirConstKey::Bool(value) => format!("bool:{value}"),
        MirConstKey::Char(value) => format!("char:{}", mir_char_descriptor(*value)),
        MirConstKey::Tuple(values) => format!(
            "tuple:{}",
            values
                .iter()
                .map(|(name, key)| format!("{name}={}", const_key_descriptor(key)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        MirConstKey::Struct { type_name, fields } => format!(
            "struct:{type_name}:{}",
            fields
                .iter()
                .map(|(name, key)| format!("{name}={}", const_key_descriptor(key)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        MirConstKey::Enum { type_name, variant } => format!("enum:{type_name}:{variant}"),
    }
}




fn layout_compare_code(op: MirLayoutCompareOp) -> u8 {
    match op { MirLayoutCompareOp::Equal => 0, MirLayoutCompareOp::LessEqual => 1, MirLayoutCompareOp::GreaterEqual => 2 }
}

fn allocator_code(kind: MirAllocatorKind) -> u8 {
    match kind { MirAllocatorKind::General => 0, MirAllocatorKind::Fixed => 1 }
}



fn task_group_code(kind: MirTaskGroupKind) -> u8 {
    match kind { MirTaskGroupKind::All => 0, MirTaskGroupKind::Any => 1, MirTaskGroupKind::Race => 2 }
}

fn select_code(kind: MirSelectKind) -> u8 {
    match kind { MirSelectKind::Start => 0, MirSelectKind::Receive => 1, MirSelectKind::After => 2, MirSelectKind::Wait => 3 }
}




fn js_string_part(part: &MirStringPart) -> String {
    match part {
        MirStringPart::Literal(value) => js_string(value),
        MirStringPart::Value(value) => format!("__jet_values.get({})", value.0),
    }
}




fn type_def_kind_descriptor(kind: &MirTypeDefKind) -> String {
    match kind {
        MirTypeDefKind::Struct { fields, methods } => {
            format!("struct:fields={}:methods={}", fields.len(), methods.len())
        }
        MirTypeDefKind::Enum { variants, methods } => {
            format!("enum:variants={}:methods={}", variants.len(), methods.len())
        }
        MirTypeDefKind::Distinct { base, range } => {
            format!("distinct:{}:range={}", type_descriptor(base), mir_range_descriptor(*range))
        }
        MirTypeDefKind::Alias { target } => format!("alias:{}", type_descriptor(target)),
        MirTypeDefKind::UnitFamily { members } => format!("unit-family:{}", members.join(",")),
    }
}

fn mir_range_descriptor(range: Option<(i64, i64)>) -> String {
    range
        .map(|(lo, hi)| format!("{}..{}", lo, hi))
        .unwrap_or_else(|| "none".to_string())
}

fn function_kind_descriptor(kind: MirFunctionKind) -> &'static str {
    match kind {
        MirFunctionKind::Jet => "jet",
        MirFunctionKind::Foreign => "foreign",
        MirFunctionKind::Test => "test",
    }
}

fn visibility_descriptor(visibility: MirVisibility) -> &'static str {
    match visibility {
        MirVisibility::Private => "private",
        MirVisibility::Package => "package",
        MirVisibility::Public => "public",
    }
}

fn core_symbol(symbol: jet_foundation::Syntax::CoreCallSymbol) -> &'static str {
    match symbol {
        jet_foundation::Syntax::CoreCallSymbol::Prelude(name) => name,
        jet_foundation::Syntax::CoreCallSymbol::Rust(name) => name,
    }
}

fn prelude_family_descriptor(family: MirPreludeFamily) -> &'static str {
    match family {
        MirPreludeFamily::MathBuiltin => "math-builtin",
        MirPreludeFamily::PreciseBuiltin => "precise-builtin",
        MirPreludeFamily::BuiltinMethod => "builtin-method",
        MirPreludeFamily::HostBorrowCallback => "host-borrow-callback",
        MirPreludeFamily::Overflow => "overflow",
        MirPreludeFamily::HandleMethod => "handle-method",
        MirPreludeFamily::ClosureMethod => "closure-method",
        MirPreludeFamily::ColumnarAccess => "columnar-access",
        MirPreludeFamily::StaticPrelude => "static-prelude",
        MirPreludeFamily::Host => "host",
    }
}

fn prelude_abi_descriptor(abi: MirPreludeAbi) -> &'static str {
    match abi {
        MirPreludeAbi::Value => "value",
        MirPreludeAbi::Aggregate => "aggregate",
        MirPreludeAbi::Control => "control",
        MirPreludeAbi::Effect => "effect",
    }
}

fn core_call_fallibility_descriptor(
    value: jet_foundation::Syntax::CoreCallFallibility,
) -> &'static str {
    match value {
        jet_foundation::Syntax::CoreCallFallibility::Sema => "sema",
    }
}

fn optional_effect_descriptor(
    value: Option<&jet_foundation::Authority::Effect>,
) -> String {
    value
        .map(|value| json_string(effect_descriptor(*value)))
        .unwrap_or_else(|| "null".to_string())
}

fn effect_descriptor(value: jet_foundation::Authority::Effect) -> &'static str {
    match value {
        jet_foundation::Authority::Effect::Net => "net",
        jet_foundation::Authority::Effect::FS => "fs",
        jet_foundation::Authority::Effect::IO => "io",
        jet_foundation::Authority::Effect::DB => "db",
        jet_foundation::Authority::Effect::Time => "time",
        jet_foundation::Authority::Effect::Rand => "rand",
        jet_foundation::Authority::Effect::Env => "env",
        jet_foundation::Authority::Effect::Exec => "exec",
        jet_foundation::Authority::Effect::Log => "log",
        jet_foundation::Authority::Effect::GPU => "gpu",
        jet_foundation::Authority::Effect::Panic => "panic",
        jet_foundation::Authority::Effect::FFI => "ffi",
        jet_foundation::Authority::Effect::Browser => "browser",
        jet_foundation::Authority::Effect::Secret => "secret",
        jet_foundation::Authority::Effect::Mem => "mem",
    }
}

fn core_call_pure_route_descriptor(
    value: jet_foundation::Syntax::CoreCallPureRoute,
) -> &'static str {
    match value {
        jet_foundation::Syntax::CoreCallPureRoute::None => "none",
        jet_foundation::Syntax::CoreCallPureRoute::Mime => "mime",
        jet_foundation::Syntax::CoreCallPureRoute::Email => "email",
        jet_foundation::Syntax::CoreCallPureRoute::EncodingXml => "encoding-xml",
        jet_foundation::Syntax::CoreCallPureRoute::Time => "time",
        jet_foundation::Syntax::CoreCallPureRoute::Math => "math",
        jet_foundation::Syntax::CoreCallPureRoute::Measurement => "measurement",
        jet_foundation::Syntax::CoreCallPureRoute::Date => "date",
        jet_foundation::Syntax::CoreCallPureRoute::DateTime => "date-time",
        jet_foundation::Syntax::CoreCallPureRoute::SketchHll => "sketch-hll",
        jet_foundation::Syntax::CoreCallPureRoute::SketchTDigest => "sketch-t-digest",
        jet_foundation::Syntax::CoreCallPureRoute::SketchCms => "sketch-cms",
        jet_foundation::Syntax::CoreCallPureRoute::SketchReservoir => "sketch-reservoir",
        jet_foundation::Syntax::CoreCallPureRoute::Ui => "ui",
        jet_foundation::Syntax::CoreCallPureRoute::Raylib => "raylib",
        jet_foundation::Syntax::CoreCallPureRoute::Io => "io",
        jet_foundation::Syntax::CoreCallPureRoute::Net => "net",
        jet_foundation::Syntax::CoreCallPureRoute::Crypto => "crypto",
    }
}

fn core_call_interpreter_route_descriptor(
    value: jet_foundation::Syntax::CoreCallInterpreterRoute,
) -> String {
    match value {
        jet_foundation::Syntax::CoreCallInterpreterRoute::None => "none".to_string(),
        jet_foundation::Syntax::CoreCallInterpreterRoute::Pure(route) => {
            format!("pure-{}", core_call_pure_route_descriptor(route))
        }
        jet_foundation::Syntax::CoreCallInterpreterRoute::Ambient => "ambient".to_string(),
        jet_foundation::Syntax::CoreCallInterpreterRoute::TypedIntrinsic => {
            "typed-intrinsic".to_string()
        }
    }
}

fn item_ref_descriptor(item: jet_foundation::MIR::MirItemRef) -> String {
    match item {
        jet_foundation::MIR::MirItemRef::Type(id) => format!("type:{}", id.0),
        jet_foundation::MIR::MirItemRef::Trait(id) => format!("trait:{}", id.0),
        jet_foundation::MIR::MirItemRef::Function(id) => format!("function:{}", id.0),
        jet_foundation::MIR::MirItemRef::Constant(id) => format!("constant:{}", id.0),
        jet_foundation::MIR::MirItemRef::Impl(id) => format!("impl:{}", id.0),
        jet_foundation::MIR::MirItemRef::Foreign(id) => format!("foreign:{}", id.0),
        jet_foundation::MIR::MirItemRef::Import(id) => format!("import:{}", id.0),
    }
}

fn import_kind_descriptor(kind: &jet_foundation::MIR::MirImportKind) -> String {
    match kind {
        jet_foundation::MIR::MirImportKind::File { path } => format!("file:{path}"),
        jet_foundation::MIR::MirImportKind::Module { path } => format!("module:{path}"),
        jet_foundation::MIR::MirImportKind::Unqualified { module, items } => format!(
            "unqualified:module={}:items={}",
            module.0,
            items
                .iter()
                .map(|item| format!(
                    "{}:{}:{}",
                    item.original,
                    item.local,
                    item_ref_descriptor(item.item)
                ))
                .collect::<Vec<_>>()
                .join(",")
        ),
    }
}

fn optional_serde_codec_descriptor(
    value: Option<&jet_foundation::MIR::MirSerdeCodec>,
) -> String {
    value
        .map(|value| json_string(serde_codec_descriptor(*value)))
        .unwrap_or_else(|| "null".to_string())
}

fn serde_codec_descriptor(value: jet_foundation::MIR::MirSerdeCodec) -> &'static str {
    match value {
        jet_foundation::MIR::MirSerdeCodec::Encode => "encode",
        jet_foundation::MIR::MirSerdeCodec::Decode => "decode",
    }
}

fn job_scope_descriptor(value: jet_foundation::MIR::MirJobScope) -> &'static str {
    match value {
        jet_foundation::MIR::MirJobScope::Dev => "dev",
        jet_foundation::MIR::MirJobScope::Ship => "ship",
        jet_foundation::MIR::MirJobScope::Internal => "internal",
    }
}

fn job_schedule_descriptor(value: &jet_foundation::MIR::MirJobSchedule) -> String {
    match value {
        jet_foundation::MIR::MirJobSchedule::Duration { nanos } => {
            format!("duration:{nanos}")
        }
        jet_foundation::MIR::MirJobSchedule::WallClockTime { hour, minute } => {
            format!("wall-clock-time:{hour:02}:{minute:02}")
        }
    }
}

fn optional_job_schedule_descriptor(
    value: Option<&jet_foundation::MIR::MirJobSchedule>,
) -> String {
    value
        .map(|value| json_string(&job_schedule_descriptor(value)))
        .unwrap_or_else(|| "null".to_string())
}

fn job_dispatch_descriptor(value: jet_foundation::MIR::MirJobDispatch) -> &'static str {
    match value {
        jet_foundation::MIR::MirJobDispatch::Direct => "direct",
        jet_foundation::MIR::MirJobDispatch::Spawn => "spawn",
        jet_foundation::MIR::MirJobDispatch::Scheduled => "scheduled",
    }
}

fn job_skip_descriptor(value: &jet_foundation::MIR::MirJobSkip) -> String {
    match value {
        jet_foundation::MIR::MirJobSkip::Always(condition) => format!("always:{condition}"),
        jet_foundation::MIR::MirJobSkip::UnlessPlatform(platform) => {
            format!("unless-platform:{platform}")
        }
    }
}

fn optional_job_skip_descriptor(value: Option<&jet_foundation::MIR::MirJobSkip>) -> String {
    value
        .map(|value| json_string(&job_skip_descriptor(value)))
        .unwrap_or_else(|| "null".to_string())
}

fn job_cache_policy_descriptor(value: jet_foundation::MIR::MirJobCachePolicy) -> &'static str {
    match value {
        jet_foundation::MIR::MirJobCachePolicy::Uncached => "uncached",
        jet_foundation::MIR::MirJobCachePolicy::Local => "local",
        jet_foundation::MIR::MirJobCachePolicy::Shared => "shared",
    }
}

fn job_limits_descriptor(limits: &BTreeMap<String, String>) -> String {
    limits
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(",")
}

fn harness_kind_descriptor(value: jet_foundation::MIR::MirHarnessKind) -> &'static str {
    match value {
        jet_foundation::MIR::MirHarnessKind::Test => "test",
        jet_foundation::MIR::MirHarnessKind::Fuzz => "fuzz",
        jet_foundation::MIR::MirHarnessKind::Coverage => "coverage",
    }
}

fn runtime_part_descriptor(value: jet_foundation::MIR::MirRuntimePartId) -> &'static str {
    match value {
        jet_foundation::MIR::MirRuntimePartId::Gc => "gc",
        jet_foundation::MIR::MirRuntimePartId::Event => "event",
        jet_foundation::MIR::MirRuntimePartId::Realtime => "realtime",
        jet_foundation::MIR::MirRuntimePartId::EmbeddedHardware => "embedded-hardware",
        jet_foundation::MIR::MirRuntimePartId::Ui => "ui",
        jet_foundation::MIR::MirRuntimePartId::Devtools => "devtools",
        jet_foundation::MIR::MirRuntimePartId::Gtk => "gtk",
        jet_foundation::MIR::MirRuntimePartId::Apps => "apps",
        jet_foundation::MIR::MirRuntimePartId::Email => "email",
        jet_foundation::MIR::MirRuntimePartId::Game => "game",
        jet_foundation::MIR::MirRuntimePartId::Files => "files",
        jet_foundation::MIR::MirRuntimePartId::Interrupt => "interrupt",
        jet_foundation::MIR::MirRuntimePartId::FsRuntime => "fs-runtime",
        jet_foundation::MIR::MirRuntimePartId::Process => "process",
        jet_foundation::MIR::MirRuntimePartId::Crypto => "crypto",
        jet_foundation::MIR::MirRuntimePartId::Math => "math",
        jet_foundation::MIR::MirRuntimePartId::Encoding => "encoding",
        jet_foundation::MIR::MirRuntimePartId::Data => "data",
        jet_foundation::MIR::MirRuntimePartId::Fmt => "fmt",
        jet_foundation::MIR::MirRuntimePartId::DataFmt => "data-fmt",
        jet_foundation::MIR::MirRuntimePartId::Compute => "compute",
        jet_foundation::MIR::MirRuntimePartId::Http => "http",
        jet_foundation::MIR::MirRuntimePartId::WebSocket => "web-socket",
        jet_foundation::MIR::MirRuntimePartId::Browser => "browser",
        jet_foundation::MIR::MirRuntimePartId::Args => "args",
        jet_foundation::MIR::MirRuntimePartId::Reflect => "reflect",
        jet_foundation::MIR::MirRuntimePartId::AuthTokens => "auth-tokens",
        jet_foundation::MIR::MirRuntimePartId::AuthSession => "auth-session",
        jet_foundation::MIR::MirRuntimePartId::Sync => "sync",
        jet_foundation::MIR::MirRuntimePartId::Services => "services",
        jet_foundation::MIR::MirRuntimePartId::Mod => "mod",
    }
}

fn foreign_abi_descriptor(abi: &MirForeignAbi) -> String {
    match abi {
        MirForeignAbi::C => "c".to_string(),
        MirForeignAbi::CUnwind => "c-unwind".to_string(),
        MirForeignAbi::System => "system".to_string(),
        MirForeignAbi::Stdcall => "stdcall".to_string(),
        MirForeignAbi::Fastcall => "fastcall".to_string(),
        MirForeignAbi::Vectorcall => "vectorcall".to_string(),
        MirForeignAbi::Rust => "rust".to_string(),
        MirForeignAbi::Platform(name) => format!("platform:{name}"),
    }
}

fn entry_kind_descriptor(value: jet_foundation::MIR::MirEntryKind) -> &'static str {
    match value {
        jet_foundation::MIR::MirEntryKind::Library => "library",
        jet_foundation::MIR::MirEntryKind::Command => "command",
        jet_foundation::MIR::MirEntryKind::App => "app",
        jet_foundation::MIR::MirEntryKind::Service => "service",
        jet_foundation::MIR::MirEntryKind::Test => "test",
    }
}

fn entry_output_descriptor(value: jet_foundation::MIR::MirEntryOutput) -> &'static str {
    match value {
        jet_foundation::MIR::MirEntryOutput::None => "none",
        jet_foundation::MIR::MirEntryOutput::ReturnValue => "return-value",
        jet_foundation::MIR::MirEntryOutput::StandardOutput => "standard-output",
        jet_foundation::MIR::MirEntryOutput::ExitStatus => "exit-status",
    }
}

fn cli_value_kind_descriptor(value: jet_foundation::MIR::MirCliValueKind) -> &'static str {
    match value {
        jet_foundation::MIR::MirCliValueKind::Bool => "bool",
        jet_foundation::MIR::MirCliValueKind::Int => "int",
        jet_foundation::MIR::MirCliValueKind::Float => "float",
        jet_foundation::MIR::MirCliValueKind::String => "string",
        jet_foundation::MIR::MirCliValueKind::Path => "path",
    }
}

fn access_descriptor(value: MirAccess) -> &'static str {
    match value {
        MirAccess::Read => "read",
        MirAccess::Write => "write",
        MirAccess::Move => "move",
    }
}


fn json_reconstructions(reconstructions: &[jet_foundation::MIR::MirWebParamReconstruction]) -> String {
    let rows = reconstructions.iter().map(|reconstruction| {
        let fields = reconstruction.fields.iter().map(|field| format!("{{\"field\":{},\"parameter\":{},\"type\":{}}}", json_string(&field.field), json_string(&field.parameter), json_string(&type_descriptor(&field.ty)))).collect::<Vec<_>>().join(",");
        format!("{{\"local\":{},\"type\":{},\"fields\":[{}]}}", json_string(&reconstruction.local), json_string(&type_descriptor(&reconstruction.ty)), fields)
    }).collect::<Vec<_>>().join(",");
    format!("[{rows}]")
}
