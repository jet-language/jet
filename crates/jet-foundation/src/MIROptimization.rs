//! The one deterministic legality and optimization pipeline for canonical MIR.
//!
//! This module operates only on semantic MIR.  It does not know how a value is
//! represented by Rust, Cranelift, the interpreter, or Web.  Proof-backed facts
//! are written back to [`MirOptimizationFacts`] so every adapter consumes the
//! same rows.

use crate::CanonicalPass;
use crate::Diagnostics::Span;
use crate::Syntax::{CoreCallFallibility, CoreCallInterpreterRoute, CoreCallPureRoute};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::fmt;
use crate::MIR::{
    canonical_identity, canonical_payload, stable_id, MirAccess, MirAccelerationFact,
    MirArtifactPlan, MirBasicBlock, MirBinaryOp,
    MirBlockId, MirBoundsFact, MirCallbackAdapter, MirCallee, MirCallFallibility, MirCliDefault,
    MirCliInput, MirCliInputShape, MirCliValueKind, MirConstant, MirConstantDef, MirConstKey,
    MirConstReport, MirConversion, MirCopyCost, MirDropAction, MirDropEdge, MirDropKind,
    MirEntrySpec,
    MirEnumArg, MirFailureCarrier, MirFieldId, MirForeign, MirFunction, MirFunctionId,
    MirFusionFact, MirHandleLifecycle, MirHardwareOp, MirHardwareSetup, MirHarnessPlan,
    MirImplDef, MirImport, MirImportKind, MirIndexKind,
    MirInstruction, MirItemRef, MirJob, MirJobSchedule, MirJobSkip, MirLinkUnit, MirLocal,
    MirLocalId, MirLoopFact, MirLoopForm, MirLoopSourceKind, MirModule, MirOperation, MirOpId,
    MirOptimizationDecision, MirOptimizationRejection, MirOwnership, MirOwnershipMode,
    MirDecisionDisposition, MirDecisionIdentity, MirDecisionKind, MirDecisionLedger, MirDecisionRow,
    MirOptimizationFacts, MirOptimizationPassId, MirPlace, MirPlaceBase, MirPlaceId,
    MirPreludeCall, MirPreludeCallId, MirProgram, MirProjection, MirScope, MirScopeId,
    MirSemanticOp, MirSourceFileId, MirStringPart, MirStructLayout, MirSwitchArm,
    MirTestScopeMember,
    MirTerminator, MirTestCase,
    MirTraitDef, MirType, MirTypeDef, MirTypeDefKind, MirTypeId, MirTypeKind, MirUnaryOp,
    MirPanicContext, MirValidationError, MirValueId, MirVectorAccess, MirVectorAccessRoot,
    MirVectorFact, MirVectorLayout, MirVectorRule, MirFixedReductionFact,
};
/// Typed acceleration facts and release gate shared by MIR adapters.
pub mod Acceleration;
/// The pass order is part of the semantic MIR contract.  It is intentionally
/// target-neutral and stable across runs.
pub const MIR_OPTIMIZATION_PASS_ORDER: [MirOptimizationPassId; 7] = [
    MirOptimizationPassId::LegalityVerification,
    MirOptimizationPassId::UnreachableBlockElimination,
    MirOptimizationPassId::CfgSimplification,
    MirOptimizationPassId::ExactConstantFolding,
    MirOptimizationPassId::BoundsCheckElimination,
    MirOptimizationPassId::DeadPureValueElimination,
    MirOptimizationPassId::CanonicalLoopFacts,
];

/// The current pipeline is deliberately policy-free.  Keeping the argument
/// typed leaves the seam explicit without creating per-adapter switches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MirOptimizationPolicy;

impl MirOptimizationPolicy {
    pub const fn conservative() -> Self {
        Self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirLegalityError {
    Validation(MirValidationError),
    InvalidSpan { function: Option<MirFunctionId>, span: Span },
    DuplicateInstruction { function: MirFunctionId, id: u64 },
    MissingValueMetadata { function: MirFunctionId, value: MirValueId },
    UnexpectedValueMetadata { function: MirFunctionId, value: MirValueId },
    InvalidType { function: MirFunctionId, span: Span },
    InvalidLayout { function: MirFunctionId, span: Span },
    InvalidReference { function: MirFunctionId, subject: String, span: Span },
    InvalidPlaceAccess { function: MirFunctionId, place: u64, span: Span },
    InvalidBorrow { function: MirFunctionId, place: u64, span: Span },
    InvalidOwnership { function: MirFunctionId, value: MirValueId, span: Span },
    InvalidMove { function: MirFunctionId, value: MirValueId, span: Span },
    InvalidMovedPlace { function: MirFunctionId, place: u64, span: Span },
    InvalidDrop { function: MirFunctionId, place: u64, span: Span },
    InvalidFailureEdge { function: MirFunctionId, span: Span },
    InvalidEffectEdge { function: MirFunctionId, span: Span },
    InvalidCoreCall { call: u64, key: String },
    InvalidPreludeCall { call: u64, member: String },
    InvalidFact { function: MirFunctionId, span: Span },
}

impl fmt::Display for MirLegalityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Validation(error) => write!(f, "invalid MIR: {error}"),
            Self::InvalidSpan { span, .. } => write!(f, "invalid MIR source span {}..{}", span.start, span.end),
            Self::DuplicateInstruction { function, id } => write!(f, "function {function:?} has duplicate instruction ID {id}"),
            Self::MissingValueMetadata { function, value } => write!(f, "function {function:?} has no metadata for value {value:?}"),
            Self::UnexpectedValueMetadata { function, value } => write!(f, "function {function:?} has metadata for missing value {value:?}"),
            Self::InvalidType { function, span } => write!(f, "function {function:?} has an invalid type at {}..{}", span.start, span.end),
            Self::InvalidLayout { function, span } => write!(f, "function {function:?} has a type/layout mismatch at {}..{}", span.start, span.end),
            Self::InvalidReference { function, subject, span } => write!(f, "function {function:?} has invalid MIR reference {subject} at {}..{}", span.start, span.end),
            Self::InvalidPlaceAccess { function, place, span } => write!(f, "function {function:?} writes through invalid place {place} at {}..{}", span.start, span.end),
            Self::InvalidBorrow { function, place, span } => write!(f, "function {function:?} borrows unreadable place {place} at {}..{}", span.start, span.end),
            Self::InvalidOwnership { function, value, span } => write!(f, "function {function:?} has invalid ownership for {value:?} at {}..{}", span.start, span.end),
            Self::InvalidMove { function, value, span } => write!(f, "function {function:?} moves value {value:?} more than once at {}..{}", span.start, span.end),
            Self::InvalidMovedPlace { function, place, span } => write!(f, "function {function:?} uses moved place {place} at {}..{}", span.start, span.end),
            Self::InvalidDrop { function, place, span } => write!(f, "function {function:?} has an invalid drop for place {place} at {}..{}", span.start, span.end),
            Self::InvalidFailureEdge { function, span } => write!(f, "function {function:?} has an invalid failure edge at {}..{}", span.start, span.end),
            Self::InvalidEffectEdge { function, span } => write!(f, "function {function:?} has an invalid effect edge at {}..{}", span.start, span.end),
            Self::InvalidCoreCall { call, key } => write!(f, "Core call {key:?} has invalid metadata for {call}"),
            Self::InvalidPreludeCall { call, member } => write!(f, "Prelude call {member:?} has invalid metadata for {call}"),
            Self::InvalidFact { function, span } => write!(f, "function {function:?} has an invalid optimization fact at {}..{}", span.start, span.end),
        }
    }
}

impl std::error::Error for MirLegalityError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirOptimizationError {
    Legality {
        pass: MirOptimizationPassId,
        error: MirLegalityError,
    },
    PassPrecondition {
        pass: MirOptimizationPassId,
        function: Option<MirFunctionId>,
        reason: String,
    },
}

impl fmt::Display for MirOptimizationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Legality { pass, error } => write!(f, "MIR pass {} rejected the program: {error}", pass.as_str()),
            Self::PassPrecondition { pass, function, reason } => write!(f, "MIR pass {} precondition failed for {function:?}: {reason}", pass.as_str()),
        }
    }
}

impl std::error::Error for MirOptimizationError {}

/// Verify all semantic invariants needed by every optimization pass and every
/// later adapter.  The verifier is intentionally independent of optimization
/// or backend representation.
pub fn verify_mir_legality(program: &MirProgram) -> Result<(), MirLegalityError> {
    program.validate().map_err(MirLegalityError::Validation)?;
    verify_operator_markers(program)?;
    let function_ids: HashSet<_> = program.functions.iter().map(|function| function.id).collect();
    let type_ids: HashSet<_> = program
        .types
        .iter()
        .map(|ty| ty.id)
        .chain(program.type_instances.iter().filter_map(|ty| ty.identity))
        .collect();
    let field_ids: HashSet<_> = program.fields.iter().map(|row| row.id).collect();
    let source_file_ids: HashSet<_> = program.source_files.iter().map(|file| file.id).collect();
    let foreign_ids: HashSet<_> = program.foreign.iter().map(|foreign| foreign.id).collect();
    let callback_ids: HashSet<_> = program.callbacks.iter().map(|row| row.id).collect();
    let trait_ids: HashSet<_> = program.traits.iter().map(|row| row.id).collect();
    let core_ids: HashSet<_> = program.core_calls.iter().map(|call| call.id).collect();
    let core_calls: HashMap<_, _> = program.core_calls.iter().map(|call| (call.id, call)).collect();
    let prelude_calls: HashMap<_, _> = program.prelude_calls.iter().map(|call| (call.id, call)).collect();

    verify_program_rows(program, &function_ids, &type_ids, &field_ids, &source_file_ids)?;
    verify_hardware_setups(program)?;
    verify_prelude_rows(program, &type_ids, &field_ids)?;
    for call in &program.core_calls {
        verify_core_call(call)?;
    }
    for ty in &program.types {
        verify_type_def(ty, &function_ids, &field_ids)?;
    }
    for ty in &program.type_instances {
        verify_type(ty, Span { start: 0, end: 0 }, MirFunctionId(0))?;
    }
    let mut function_keys = HashSet::new();
    for function in &program.functions {
        if !function_keys.insert(function.key.as_str()) {
            return Err(MirLegalityError::Validation(MirValidationError::DuplicateKey {
                kind: "function",
                key: function.key.clone(),
            }));
        }
        verify_function(
            function,
            &function_ids,
            &type_ids,
            &trait_ids,
            &field_ids,
            &source_file_ids,
            &foreign_ids,
            &callback_ids,
            &core_ids,
            &core_calls,
            &prelude_calls,
        )?;
        verify_capture_operations(function)?;
        verify_transparent_conversions(function, &program.types)?;
    }
    Ok(())
}
fn invalid_program_row(subject: impl Into<String>, span: Span) -> MirLegalityError {
    MirLegalityError::InvalidReference {
        function: MirFunctionId(0),
        subject: subject.into(),

        span,
    }
}
/// D-FOUND-OPMIX1=A: keep the marker's checked shape intact at the canonical
/// optimizer boundary. Backends never infer or repair operator symmetry.
fn verify_operator_markers(program: &MirProgram) -> Result<(), MirLegalityError> {
    for implementation in &program.impls {
        let Some(crate::AST::OperatorMarker::Commutative) = implementation.operator_marker
        else {
            continue;
        };
        let trait_name = implementation
            .trait_ref
            .as_ref()
            .map(|trait_ref| trait_ref.name.rsplit("::").next().unwrap_or(&trait_ref.name));
        let valid_trait = trait_name.is_some_and(|name| {
            name == crate::Syntax::TRAIT_ADD || name == crate::Syntax::TRAIT_MUL
        });
        if !valid_trait || implementation.operator_rhs.is_none() {
            return Err(invalid_program_row(
                "invalid #Commutative operator marker",
                implementation.span,
            ));
        }
    }
    Ok(())
}

fn verify_row_id(
    id: u64,
    kind: &'static str,
    seen: &mut HashSet<u64>,
    span: Span,
) -> Result<(), MirLegalityError> {
    if id == 0 {
        return Err(invalid_program_row(format!("{kind} zero ID"), span));
    }
    if !seen.insert(id) {
        return Err(MirLegalityError::Validation(MirValidationError::DuplicateId { kind, id }));
    }
    Ok(())
}

fn verify_row_ref<T: Eq + std::hash::Hash>(
    ids: &HashSet<T>,
    id: &T,
    subject: impl Into<String>,
    span: Span,
) -> Result<(), MirLegalityError> {
    if ids.contains(id) {
        Ok(())
    } else {
        Err(invalid_program_row(subject, span))
    }
}

fn verify_generic_params(
    params: &[crate::MIR::MirGenericParam],
    trait_ids: &HashSet<crate::MIR::MirTraitId>,
    span: Span,
) -> Result<(), MirLegalityError> {
    for param in params {
        if param.name.is_empty() {
            return Err(invalid_program_row("generic parameter without a name", span));
        }
        for bound in &param.bounds {
            if bound.id.0 == 0 || bound.name.is_empty() || !trait_ids.contains(&bound.id) {
                return Err(invalid_program_row(format!("generic bound {:?}", bound.id), span));
            }
        }
    }
    Ok(())
}

fn verify_mir_param(
    param: &crate::MIR::MirParam,
    function: MirFunctionId,
) -> Result<(), MirLegalityError> {
    if param.name.is_empty() || param.public_label.is_empty() || param.span.start > param.span.end {
        return Err(invalid_program_row(format!("parameter {}", param.index), param.span));
    }
    verify_type(&param.ty, param.span, function)
}

fn verify_mir_constant(constant: &MirConstant, span: Span) -> Result<(), MirLegalityError> {
    match constant {
        MirConstant::Int { .. }
        | MirConstant::Float { .. }
        | MirConstant::Bool(_)
        | MirConstant::Char(_)
        | MirConstant::String(_)
        | MirConstant::Bytes(_)
        | MirConstant::Unit
        | MirConstant::BigInt(_) => Ok(()),
        MirConstant::List(values) => {
            for value in values {
                verify_mir_constant(value, span)?;
            }
            Ok(())
        }
        MirConstant::Map(values) => {
            for value in values.values() {
                verify_mir_constant(value, span)?;
            }
            Ok(())
        }
        MirConstant::Struct { fields, .. } => {
            for (_, value) in fields {
                verify_mir_constant(value, span)?;
            }
            Ok(())
        }
        MirConstant::Enum { args, .. } => {
            for (_, value) in args {
                verify_mir_constant(value, span)?;
            }
            Ok(())
        }
        MirConstant::Present(value) => verify_mir_constant(value, span),
        MirConstant::Failed(report) => match report {
            MirConstReport::Clean(ty) => verify_type(ty, span, MirFunctionId(0)),
            MirConstReport::Told(value) => verify_mir_constant(value, span),
        },
    }
}

fn verify_cli_input(input: &MirCliInput, span: Span) -> Result<(), MirLegalityError> {
    if input.name.is_empty() || input.label.is_empty() || input.help.is_empty() {
        return Err(invalid_program_row("CLI input without a name, label, or help", span));
    }
    if input.short.as_deref().is_some_and(str::is_empty)
        || input.env.as_deref().is_some_and(str::is_empty)
        || input.metavar.as_deref().is_some_and(str::is_empty)
    {
        return Err(invalid_program_row("CLI input contains an empty optional name", span));
    }
    verify_type(&input.ty, span, MirFunctionId(0))?;
    match &input.shape {
        MirCliInputShape::Flag => {
            if !input.ty.is_bool() || input.variadic || input.metavar.is_some() {
                return Err(invalid_program_row("CLI flag has invalid type or shape", span));
            }
        }
        MirCliInputShape::Value {
            kind,
            optional,
            default,
        } => {
            let value_type = if input.variadic {
                match input.ty.kind() {
                    MirTypeKind::List(element) => element.as_ref(),
                    _ => return Err(invalid_program_row("CLI variadic input is not a list", span)),
                }
            } else if *optional {
                match input.ty.kind() {
                    MirTypeKind::Option(value) => value.as_ref(),
                    _ => return Err(invalid_program_row("CLI optional input is not optional", span)),
                }
            } else {
                &input.ty
            };
            let kind_matches = match kind {
                MirCliValueKind::Bool => value_type.is_bool(),
                MirCliValueKind::Int => value_type.is_integer(),
                MirCliValueKind::Float => value_type.is_float(),
                MirCliValueKind::String => value_type.is_string(),
                MirCliValueKind::Path => {
                    value_type.is_string()
                        || value_type.nominal_name() == Some(crate::Syntax::TYPE_PATH)
                }
            };
            if !kind_matches {
                return Err(invalid_program_row("CLI value kind disagrees with its type", span));
            }
            if *optional && default.is_some() {
                return Err(invalid_program_row("CLI optional input cannot carry a default", span));
            }
            if let Some(MirCliDefault::Value(value)) = default {
                verify_mir_constant(value, span)?;
            }
        }
    }
    if input.variadic && input.positional.is_none() {
        return Err(invalid_program_row("CLI variadic input must be positional", span));
    }
    Ok(())
}

fn verify_entry_spec(
    entry: &MirEntrySpec,
    function_ids: &HashSet<MirFunctionId>,
    type_ids: &HashSet<MirTypeId>,
    span: Span,
) -> Result<(), MirLegalityError> {
    if let Some(function) = entry.function {
        verify_row_ref(function_ids, &function, "entry function", span)?;
    }
    if entry.package_version.is_empty() {
        return Err(invalid_program_row("entry package version", span));
    }
    if let Some(cli) = &entry.cli {
        for input in &cli.inputs {
            verify_cli_input(input, span)?;
        }
        let mut command_names = HashSet::new();
        for command in &cli.commands {
            if command.name.is_empty() || !command_names.insert(command.name.as_str()) {
                return Err(invalid_program_row(format!("CLI command {}", command.name), span));
            }
            verify_row_ref(function_ids, &command.function, format!("CLI command {}", command.name), span)?;
            if let Some(receiver) = command.receiver {
                verify_row_ref(type_ids, &receiver, format!("CLI receiver {}", command.name), span)?;
            }
            for input in &command.inputs {
                verify_cli_input(input, span)?;
            }
        }
    }
    Ok(())
}

fn verify_program_rows(
    program: &MirProgram,
    function_ids: &HashSet<MirFunctionId>,
    type_ids: &HashSet<MirTypeId>,
    field_ids: &HashSet<MirFieldId>,
    source_file_ids: &HashSet<MirSourceFileId>,
) -> Result<(), MirLegalityError> {
    let module_ids: HashSet<_> = program.modules.iter().map(|row| row.id).collect();
    let import_ids: HashSet<_> = program.imports.iter().map(|row| row.id).collect();
    let trait_ids: HashSet<_> = program.traits.iter().map(|row| row.id).collect();
    let impl_ids: HashSet<_> = program.impls.iter().map(|row| row.id).collect();
    let constant_ids: HashSet<_> = program.constants.iter().map(|row| row.id).collect();
    let foreign_ids: HashSet<_> = program.foreign.iter().map(|row| row.id).collect();
    let callback_ids: HashSet<_> = program.callbacks.iter().map(|row| row.id).collect();
    let link_ids: HashSet<_> = program.links.iter().map(|row| row.id).collect();
    let handle_ids: HashSet<_> = program.handles.iter().map(|row| row.id).collect();
    let job_ids: HashSet<_> = program.jobs.iter().map(|job| job.id).collect();
    let test_ids: HashSet<_> = program.tests.iter().map(|row| row.id).collect();
    let harness_ids: HashSet<_> = program.harnesses.iter().map(|row| row.id).collect();
    let mut seen_modules = HashSet::new();
    for module in &program.modules {
        verify_row_id(module.id.0, "module", &mut seen_modules, Span { start: 0, end: 0 })?;
        if module.key.is_empty() || module.name.is_empty() || module.path.is_empty() {
            return Err(invalid_program_row(format!("module {:?}", module.id), Span { start: 0, end: 0 }));
        }
        verify_row_ref(source_file_ids, &module.source_file, format!("module {} source", module.key), Span { start: 0, end: 0 })?;
        for import in &module.imports {
            verify_row_ref(&import_ids, import, format!("module {} import", module.key), Span { start: 0, end: 0 })?;
        }
        for item in &module.item_order {
            match item {
                crate::MIR::MirItemRef::Type(id) => verify_row_ref(type_ids, id, "module type item", Span { start: 0, end: 0 })?,
                crate::MIR::MirItemRef::Trait(id) => verify_row_ref(&trait_ids, id, "module trait item", Span { start: 0, end: 0 })?,
                crate::MIR::MirItemRef::Function(id) => verify_row_ref(function_ids, id, "module function item", Span { start: 0, end: 0 })?,
                crate::MIR::MirItemRef::Constant(id) => verify_row_ref(&constant_ids, id, "module constant item", Span { start: 0, end: 0 })?,
                crate::MIR::MirItemRef::Impl(id) => verify_row_ref(&impl_ids, id, "module impl item", Span { start: 0, end: 0 })?,
                crate::MIR::MirItemRef::Foreign(id) => verify_row_ref(&foreign_ids, id, "module foreign item", Span { start: 0, end: 0 })?,
                crate::MIR::MirItemRef::Import(id) => verify_row_ref(&import_ids, id, "module import item", Span { start: 0, end: 0 })?,
            }
        }
    }
    let mut seen_imports = HashSet::new();
    for import in &program.imports {
        verify_row_id(import.id.0, "import", &mut seen_imports, import.span)?;
        verify_row_ref(&module_ids, &import.module, "import owner module", import.span)?;
        match &import.kind {
            MirImportKind::File { path } | MirImportKind::Module { path } => {
                if path.is_empty() {
                    return Err(invalid_program_row(format!("import {:?}", import.id), import.span));
                }
            }
            MirImportKind::Unqualified { module, items } => {
                verify_row_ref(&module_ids, module, "unqualified import module", import.span)?;
                for item in items {
                    if item.original.is_empty() || item.local.is_empty() {
                        return Err(invalid_program_row(format!("import {:?}", import.id), import.span));
                    }
                    match item.item {
                        crate::MIR::MirItemRef::Type(id) => verify_row_ref(type_ids, &id, "import type", import.span)?,
                        crate::MIR::MirItemRef::Trait(id) => verify_row_ref(&trait_ids, &id, "import trait", import.span)?,
                        crate::MIR::MirItemRef::Function(id) => verify_row_ref(function_ids, &id, "import function", import.span)?,
                        crate::MIR::MirItemRef::Constant(id) => verify_row_ref(&constant_ids, &id, "import constant", import.span)?,
                        crate::MIR::MirItemRef::Impl(id) => verify_row_ref(&impl_ids, &id, "import impl", import.span)?,
                        crate::MIR::MirItemRef::Foreign(id) => verify_row_ref(&foreign_ids, &id, "import foreign", import.span)?,
                        crate::MIR::MirItemRef::Import(id) => verify_row_ref(&import_ids, &id, "import import", import.span)?,
                    }
                }
            }
        }
    }
    for type_def in &program.types {
        verify_row_ref(&module_ids, &type_def.module, format!("type {} module", type_def.key), type_def.span)?;
        if type_def.key.is_empty() || type_def.name.is_empty() {
            return Err(invalid_program_row(format!("type {:?}", type_def.id), type_def.span));
        }
        verify_generic_params(&type_def.generic_params, &trait_ids, type_def.span)?;
        for trait_id in &type_def.derives {
            verify_row_ref(&trait_ids, trait_id, format!("type {} derive", type_def.key), type_def.span)?;
        }
        for binding in &type_def.cli_bindings {
            if binding.name.is_empty() {
                return Err(invalid_program_row(format!("type {:?} CLI binding", type_def.id), type_def.span));
            }
            verify_row_ref(function_ids, &binding.function, "type CLI binding function", type_def.span)?;
        }
        if let Some(cli) = &type_def.cli {
            for input in &cli.inputs {
                verify_cli_input(input, type_def.span)?;
            }
            if !cli.commands.is_empty() {
                return Err(invalid_program_row(
                    format!("type {:?} CLI shape carries commands", type_def.id),
                    type_def.span,
                ));
            }
        }
    }
    for function in &program.functions {
        verify_row_ref(&module_ids, &function.module_id, format!("function {} module", function.key), function.span)?;
    }


    let mut seen_traits = HashSet::new();
    let mut seen_trait_methods = HashSet::new();
    for trait_def in &program.traits {
        verify_row_id(trait_def.id.0, "trait", &mut seen_traits, trait_def.span)?;
        verify_row_ref(&module_ids, &trait_def.module, "trait module", trait_def.span)?;
        if trait_def.key.is_empty() || trait_def.name.is_empty() {
            return Err(invalid_program_row(format!("trait {:?}", trait_def.id), trait_def.span));
        }
        for method in &trait_def.methods {
            verify_row_id(method.id.0, "trait method", &mut seen_trait_methods, method.span)?;
            if method.name.is_empty() {
                return Err(invalid_program_row(format!("trait {} method", trait_def.key), method.span));
            }
            if let Some(access) = method.self_access {
                match access {
                    MirAccess::Read | MirAccess::Write | MirAccess::Move => {}
                }
            }
            for param in &method.params {
                verify_mir_param(param, MirFunctionId(0))?;
            }
            if let Some(return_type) = &method.declared_return {
                verify_type(return_type, method.span, MirFunctionId(0))?;
            }
            verify_type(&method.return_type, method.span, MirFunctionId(0))?;
            verify_failure_carrier_at(&method.failure, MirFunctionId(0), method.span)?;
            if let Some(default) = method.default {
                verify_row_ref(function_ids, &default, format!("trait {} default", trait_def.key), method.span)?;
            }
        }
    }
    let mut seen_impls = HashSet::new();
    for impl_def in &program.impls {
        verify_row_id(impl_def.id.0, "impl", &mut seen_impls, impl_def.span)?;
        verify_row_ref(&module_ids, &impl_def.module, "impl module", impl_def.span)?;
        if impl_def.key.is_empty() {
            return Err(invalid_program_row(format!("impl {:?}", impl_def.id), impl_def.span));
        }
        verify_type(&impl_def.self_type, impl_def.span, MirFunctionId(0))?;
        if let Some(trait_ref) = &impl_def.trait_ref {
            verify_row_ref(&trait_ids, &trait_ref.id, "impl trait", impl_def.span)?;
            if trait_ref.name.is_empty() {
                return Err(invalid_program_row("impl trait name", impl_def.span));
            }
        }
        for associated in &impl_def.associated_types {
            if associated.name.is_empty() || associated.span.start > associated.span.end {
                return Err(invalid_program_row("impl associated type", associated.span));
            }
            verify_type(&associated.ty, associated.span, MirFunctionId(0))?;
        }
        for method in &impl_def.methods {
            verify_row_ref(function_ids, method, "impl method", impl_def.span)?;
        }
        if let Some(field) = impl_def.delegation {
            verify_row_ref(field_ids, &field, "impl delegation field", impl_def.span)?;
        }
        if let Some(rhs) = &impl_def.operator_rhs {
            verify_type(rhs, impl_def.span, MirFunctionId(0))?;
        }
    }
    let mut seen_constants = HashSet::new();
    for constant in &program.constants {
        verify_row_id(constant.id.0, "constant", &mut seen_constants, constant.span)?;
        verify_row_ref(&module_ids, &constant.module, "constant module", constant.span)?;
        if constant.key.is_empty() || constant.name.is_empty() {
            return Err(invalid_program_row(format!("constant {:?}", constant.id), constant.span));
        }
        verify_type(&constant.ty, constant.span, MirFunctionId(0))?;
        verify_mir_constant(&constant.value, constant.span)?;
    }

    let mut seen_links = HashSet::new();
    for link in &program.links {
        verify_row_id(link.id.0, "link unit", &mut seen_links, Span { start: 0, end: 0 })?;
        if link.crate_spec.is_empty() || link.cache_identity.is_empty() {
            return Err(invalid_program_row(format!("link {:?}", link.id), Span { start: 0, end: 0 }));
        }
        for artifact in &link.artifacts {
            if artifact.path.is_empty() {
                return Err(invalid_program_row(format!("link {:?} artifact", link.id), Span { start: 0, end: 0 }));
            }
        }
        for dependency in &link.dependency_dirs {
            if dependency.is_empty() {
                return Err(invalid_program_row(format!("link {:?} dependency", link.id), Span { start: 0, end: 0 }));
            }
        }
        for closure in &link.link_closure {
            verify_row_ref(&link_ids, closure, "link closure", Span { start: 0, end: 0 })?;
        }
    }
    let mut seen_foreign = HashSet::new();
    for foreign in &program.foreign {
        verify_row_id(foreign.id.0, "foreign", &mut seen_foreign, foreign.span)?;
        verify_row_ref(&module_ids, &foreign.module_id, "foreign module", foreign.span)?;
        if foreign.key.is_empty()
            || foreign.module.is_empty()
            || foreign.name.is_empty()
            || foreign.symbol.is_empty()
            || foreign.path.is_empty()
        {
            return Err(invalid_program_row(format!("foreign {:?}", foreign.id), foreign.span));
        }
        for param in &foreign.params {
            verify_mir_param(param, MirFunctionId(0))?;
        }
        if let Some(return_type) = &foreign.return_type {
            verify_type(return_type, foreign.span, MirFunctionId(0))?;
        }
        if let Some(link) = foreign.link {
            verify_row_ref(&link_ids, &link, "foreign link", foreign.span)?;
        }
        if let Some(callback) = foreign.callback {
            verify_row_ref(&callback_ids, &callback, "foreign callback", foreign.span)?;
        }
        if let Some(handle) = foreign.handle {
            verify_row_ref(&handle_ids, &handle, "foreign handle", foreign.span)?;
        }
        for function in [foreign.close_function, foreign.undo_function].into_iter().flatten() {
            verify_row_ref(function_ids, &function, "foreign lifecycle function", foreign.span)?;
        }
        if let Some(close_foreign) = foreign.close_foreign {
            verify_row_ref(&foreign_ids, &close_foreign, "foreign close", foreign.span)?;
        }
    }
    let mut seen_callbacks = HashSet::new();
    for callback in &program.callbacks {
        verify_row_id(callback.id.0, "callback", &mut seen_callbacks, Span { start: 0, end: 0 })?;
        if callback.symbol.is_empty() {
            return Err(invalid_program_row(format!("callback {:?}", callback.id), Span { start: 0, end: 0 }));
        }
        verify_row_ref(function_ids, &callback.function, "callback function", Span { start: 0, end: 0 })?;
        for param in &callback.params {
            verify_mir_param(param, callback.function)?;
        }
        if let Some(return_type) = &callback.return_type {
            verify_type(return_type, Span { start: 0, end: 0 }, callback.function)?;
        }
    }
    let mut seen_handles = HashSet::new();
    for handle in &program.handles {
        verify_row_id(handle.id.0, "handle", &mut seen_handles, Span { start: 0, end: 0 })?;
        verify_type(&handle.ty, Span { start: 0, end: 0 }, MirFunctionId(0))?;
        for function in [handle.close, handle.undo].into_iter().flatten() {
            verify_row_ref(function_ids, &function, "handle lifecycle function", Span { start: 0, end: 0 })?;
        }
        if let Some(foreign) = handle.close_foreign {
            verify_row_ref(&foreign_ids, &foreign, "handle close foreign", Span { start: 0, end: 0 })?;
        }
    }

    let mut seen_jobs = HashSet::new();
    for job in &program.jobs {
        verify_row_id(job.id.0, "job", &mut seen_jobs, Span { start: 0, end: 0 })?;
        verify_row_ref(
            function_ids,
            &job.function,
            format!("job {}", job.name),
            Span { start: 0, end: 0 },
        )?;
        if job.name.is_empty() {
            return Err(invalid_program_row("job without a name", Span { start: 0, end: 0 }));
        }
        if job.parallel == 0 || job.after.iter().any(String::is_empty) {
            return Err(invalid_program_row(
                format!("job {:?}", job.id),
                Span { start: 0, end: 0 },
            ));
        }
        if let Some(schedule) = job.schedule {
            match schedule {
                MirJobSchedule::Duration { nanos } if nanos < 0 => {
                    return Err(invalid_program_row(
                        "negative job duration",
                        Span { start: 0, end: 0 },
                    ))
                }
                MirJobSchedule::WallClockTime { hour, minute }
                    if hour >= 24 || minute >= 60 =>
                {
                    return Err(invalid_program_row(
                        "invalid job wall-clock time",
                        Span { start: 0, end: 0 },
                    ))
                }
                _ => {}
            }
        }
        for input in &job.inputs {
            verify_cli_input(input, Span { start: 0, end: 0 })?;
        }
        if job.packages.iter().any(String::is_empty)
            || job.input_paths.iter().any(String::is_empty)
            || job.output_paths.iter().any(String::is_empty)
            || job.limits.iter().any(|(key, value)| key.is_empty() || value.is_empty())
        {
            return Err(invalid_program_row(
                format!("job {:?}", job.id),
                Span { start: 0, end: 0 },
            ));
        }
        if let Some(directory) = &job.working_directory {
            if directory.is_empty() {
                return Err(invalid_program_row(
                    format!("job {:?}", job.id),
                    Span { start: 0, end: 0 },
                ));
            }
        }
        if let Some(skip) = &job.skip {
            match skip {
                MirJobSkip::Always(value) | MirJobSkip::UnlessPlatform(value)
                    if value.is_empty() =>
                {
                    return Err(invalid_program_row(
                        format!("job {:?}", job.id),
                        Span { start: 0, end: 0 },
                    ))
                }
                _ => {}
            }
        }
    }
    let mut seen_tests = HashSet::new();
    for test in &program.tests {
        verify_row_id(test.id.0, "test", &mut seen_tests, test.span)?;
        verify_row_ref(function_ids, &test.function, format!("test {}", test.name), test.span)?;
        if let Some(eligibility) = &test.eligibility {
            verify_row_ref(
                function_ids,
                eligibility,
                format!("test {} eligibility predicate", test.name),
                test.span,
            )?;
        }
        // #2502: exactly the sampled contract rows carry the pre-call
        // eligibility predicate; unavailable and ordinary rows never do.
        let sampled_contract =
            test.contract_generated && test.generation_unavailable_reason.is_none();
        if test.name.is_empty()
            || test.span.start > test.span.end
            || test
                .generation_unavailable_reason
                .as_deref()
                .is_some_and(str::is_empty)
            || test.eligibility.is_some() != sampled_contract
        {
            return Err(invalid_program_row(format!("test {:?}", test.id), test.span));
        }
        for param in &test.parameters {
            verify_mir_param(param, test.function)?;
        }
        if test.faults.iter().any(String::is_empty) {
            return Err(invalid_program_row(format!("test {:?}", test.id), test.span));
        }
    }
    let function_blocks: HashMap<_, HashSet<_>> = program
        .functions
        .iter()
        .map(|function| (function.id, function.blocks.iter().map(|block| block.id).collect()))
        .collect();
    let mut seen_harnesses = HashSet::new();
    let mut seen_output_checks = HashSet::new();
    let mut seen_coverage_points = HashSet::new();
    for harness in &program.harnesses {
        verify_row_id(harness.id.0, "harness", &mut seen_harnesses, Span { start: 0, end: 0 })?;
        for test in &harness.tests {
            verify_row_ref(&test_ids, test, "harness test", Span { start: 0, end: 0 })?;
        }
        if let Some(test) = harness.selected_test {
            verify_row_ref(&test_ids, &test, "harness selected test", Span { start: 0, end: 0 })?;
        }
        for check in &harness.output_checks {
            verify_row_id(check.id.0, "output check", &mut seen_output_checks, Span { start: 0, end: 0 })?;
            if check.name.is_empty() {
                return Err(invalid_program_row("output check without a name", Span { start: 0, end: 0 }));
            }
            verify_row_ref(function_ids, &check.function, "output check function", Span { start: 0, end: 0 })?;
        }
        for point in &harness.coverage_points {
            verify_row_id(point.id.0, "coverage point", &mut seen_coverage_points, point.span)?;
            verify_row_ref(function_ids, &point.function, "coverage point function", point.span)?;
            if !function_blocks.get(&point.function).is_some_and(|blocks| blocks.contains(&point.block)) {
                return Err(invalid_program_row("coverage point block", point.span));
            }
            if point.span.start > point.span.end {
                return Err(MirLegalityError::InvalidSpan { function: Some(point.function), span: point.span });
            }
        }
    }
    let mut seen_artifacts = HashSet::new();
    for artifact in &program.artifacts {
        verify_row_id(artifact.id.0, "artifact", &mut seen_artifacts, Span { start: 0, end: 0 })?;
        if artifact.name.is_empty()
            || artifact.provider_identity.is_empty()
            || artifact.closure_identity.is_empty()
            || artifact.artifact_identity.is_empty()
        {
            return Err(invalid_program_row(format!("artifact {:?}", artifact.id), Span { start: 0, end: 0 }));
        }
        for module in &artifact.modules {
            verify_row_ref(&module_ids, module, "artifact module", Span { start: 0, end: 0 })?;
        }
        for link in &artifact.links {
            verify_row_ref(&link_ids, link, "artifact link", Span { start: 0, end: 0 })?;
        }
        for job in &artifact.jobs {
            verify_row_ref(&job_ids, job, "artifact job", Span { start: 0, end: 0 })?;
        }
        if let Some(harness) = artifact.harness {
            verify_row_ref(&harness_ids, &harness, "artifact harness", Span { start: 0, end: 0 })?;
        }
        if let Some(entry) = &artifact.entry {
            verify_entry_spec(entry, function_ids, type_ids, Span { start: 0, end: 0 })?;
        }
        for export in &artifact.exports {
            if export.symbol.is_empty() {
                return Err(invalid_program_row(format!("artifact {:?} export", artifact.id), Span { start: 0, end: 0 }));
            }
            verify_row_ref(function_ids, &export.function, "artifact export function", Span { start: 0, end: 0 })?;
        }
    }
    Ok(())
}

fn verify_hardware_setups(program: &MirProgram) -> Result<(), MirLegalityError> {
    let span = Span { start: 0, end: 0 };
    let setups = &program.facts.hardware_setups;
    if setups.is_empty() {
        return Ok(());
    }
    let profile_id = &program.facts.hardware_profile_id;
    if profile_id.is_empty() {
        return Err(invalid_program_row("hardware setup has no profile ID", span));
    }
    let Some(profile) = program.facts.hardware_profile.as_ref() else {
        return Err(invalid_program_row("hardware setup has no target profile", span));
    };
    let mut dma_channels = HashSet::new();
    let mut interrupt_vectors = HashSet::new();
    for setup in setups {
        match setup {
            MirHardwareSetup::DmaConfigure {
                profile_id: setup_profile,
                channel,
                transfer_width,
                ownership,
            } => {
                if setup_profile.is_empty() || setup_profile != profile_id || channel.is_empty() {
                    return Err(invalid_program_row("invalid DMA hardware setup identity", span));
                }
                let Some(fact) = profile.dma_channel(channel) else {
                    return Err(invalid_program_row(format!("unknown DMA channel {channel}"), span));
                };
                if fact.transfer_width != *transfer_width || fact.ownership != *ownership {
                    return Err(invalid_program_row(format!("DMA setup disagrees with channel {channel} facts"), span));
                }
                if !dma_channels.insert(channel.as_str()) {
                    return Err(invalid_program_row(format!("duplicate DMA setup {channel}"), span));
                }
            }
            MirHardwareSetup::InterruptBind {
                profile_id: setup_profile,
                interrupt,
                vector,
                handler_symbol,
                forbidden_effects,
            } => {
                if setup_profile.is_empty()
                    || setup_profile != profile_id
                    || interrupt.is_empty()
                    || handler_symbol.is_empty()
                    || forbidden_effects.iter().any(String::is_empty)
                {
                    return Err(invalid_program_row("invalid interrupt hardware setup identity", span));
                }
                let Some(fact) = profile.interrupt(interrupt) else {
                    return Err(invalid_program_row(format!("unknown interrupt {interrupt}"), span));
                };
                if fact.vector != *vector {
                    return Err(invalid_program_row(format!("interrupt {interrupt} vector disagrees with profile"), span));
                }
                let actual_effects = forbidden_effects.iter().cloned().collect::<BTreeSet<_>>();
                if actual_effects != fact.forbidden_effects {
                    return Err(invalid_program_row(format!("interrupt {interrupt} effects disagree with profile"), span));
                }
                if !interrupt_vectors.insert(*vector) {
                    return Err(invalid_program_row(format!("duplicate interrupt vector {vector}"), span));
                }
            }
        }
    }
    Ok(())
}

fn valid_shape_field_names(names: &crate::Shape::ShapeFieldNames) -> bool {
    [
        Some(names.text.as_str()),
        names.json.as_deref(),
        names.cbor.as_deref(),
        names.csv.as_deref(),
        names.toml.as_deref(),
        names.yaml.as_deref(),
        names.xml.as_deref(),
        Some(names.args.as_str()),
        Some(names.env.as_str()),
        names.db.as_deref(),
        names.layout.as_deref(),
    ]
    .into_iter()
    .flatten()
    .all(|name| !name.trim().is_empty() && !name.contains('\0'))
}

fn verify_prelude_rows(
    program: &MirProgram,
    type_ids: &HashSet<MirTypeId>,
    field_ids: &HashSet<MirFieldId>,
) -> Result<(), MirLegalityError> {
    let mut seen_fields = HashSet::new();
    for row in &program.fields {
        if row.id.0 == 0
            || row.field.id != row.id
            || !seen_fields.insert(row.id)
            || !field_ids.contains(&row.id)
            || !type_ids.contains(&row.owner)
            || row.field.name.is_empty()
            || !valid_shape_field_names(&row.field.shape_names)
            || row.field.span.start > row.field.span.end
        {
            return Err(MirLegalityError::InvalidReference {
                function: MirFunctionId(0),
                subject: format!("field row {:?}", row.id),
                span: row.field.span,
            });
        }
        verify_type(&row.field.ty, row.field.span, MirFunctionId(0))?;
    }
    let mut seen_sources = HashSet::new();
    for source in &program.source_files {
        if source.id.0 == 0 || !seen_sources.insert(source.id) || source.path.is_empty() {
            return Err(MirLegalityError::InvalidReference {
                function: MirFunctionId(0),
                subject: format!("source file {:?}", source.id),
                span: Span { start: 0, end: 0 },
            });
        }
    }
    let mut seen_calls = HashSet::new();
    for call in &program.prelude_calls {
        if call.id.0 == 0
            || !seen_calls.insert(call.id)
            || call.module.is_empty()
            || call.member.is_empty()
            || call.symbol.name().is_empty()
            || call.signature.arity != call.signature.borrow_mask.len()
            || call.signature.arity > call.signature.max_arity
        {
            return Err(MirLegalityError::InvalidPreludeCall {
                call: call.id.0,
                member: call.member.clone(),
            });
        }
        verify_call_fallibility(&call.fallibility, MirFunctionId(0), Span { start: 0, end: 0 })?;
    }
    Ok(())
}


fn verify_core_call(call: &crate::MIR::MirCoreCall) -> Result<(), MirLegalityError> {
    if call.key.is_empty()
        || (call.module.is_empty() && !call.key.starts_with("receiver("))
        || call.member.is_empty()
        || call.arity != call.borrow_mask.len()
        || call.arity > call.max_arity
        || call.coverage_bits & !0x7f != 0
        // A symbol is required exactly when an executable backend claims the
        // row (AOT bit 3, interpreter bit 4, JIT bit 6); typed intrinsics are
        // interpreter-executable without a symbol, and sema-only rows
        // (`receiver_with_coverage`) legitimately have none until their
        // backend adapters land.
        || (call.symbol.name().is_empty()
            && call.coverage_bits & 0b101_1000 != 0
            && !matches!(call.interpreter_route, CoreCallInterpreterRoute::TypedIntrinsic))
        || call.jit_symbol.as_deref().is_some_and(str::is_empty)
    {
        return Err(MirLegalityError::InvalidCoreCall {
            call: call.id.0,
            key: call.key.clone(),
        });
    }
    match call.interpreter_route {
        CoreCallInterpreterRoute::None => {
            if call.coverage_bits & (1 << 4) != 0 {
                return Err(MirLegalityError::InvalidCoreCall { call: call.id.0, key: call.key.clone() });
            }
        }
        CoreCallInterpreterRoute::Pure(route) => {
            if route == CoreCallPureRoute::None || call.pure_route != route || call.coverage_bits & (1 << 4) == 0 {
                return Err(MirLegalityError::InvalidCoreCall { call: call.id.0, key: call.key.clone() });
            }
        }
        // Ambient rows route through the deterministic-world provider; the
        // purity classification stays orthogonal so comptime pure-parity and
        // web descriptors keep the row's pure route even when the interpreter
        // executes it ambiently.
        CoreCallInterpreterRoute::Ambient => {
            if call.coverage_bits & (1 << 4) == 0 {
                return Err(MirLegalityError::InvalidCoreCall { call: call.id.0, key: call.key.clone() });
            }
        }
        CoreCallInterpreterRoute::TypedIntrinsic => {
            if call.coverage_bits & (1 << 4) == 0 || call.pure_route != CoreCallPureRoute::None {
                return Err(MirLegalityError::InvalidCoreCall { call: call.id.0, key: call.key.clone() });
            }
        }
    }
    Ok(())
}

fn verify_type_def(
    ty: &crate::MIR::MirTypeDef,
    function_ids: &HashSet<MirFunctionId>,
    field_ids: &HashSet<MirFieldId>,
) -> Result<(), MirLegalityError> {
    match &ty.kind {
        MirTypeDefKind::Struct { fields, methods } => {
            for field in fields {
                if !field_ids.contains(&field.id)
                    || field.name.is_empty()
                    || !valid_shape_field_names(&field.shape_names)
                {
                    return Err(MirLegalityError::InvalidReference {
                        function: MirFunctionId(0),
                        subject: format!("type {} field {:?}", ty.key, field.id),
                        span: field.span,
                    });
                }
                verify_type(&field.ty, ty.span, MirFunctionId(0))?;
            }
            if methods.iter().any(|method| !function_ids.contains(method)) {
                return Err(MirLegalityError::InvalidReference {
                    function: MirFunctionId(0),
                    subject: format!("type {} method", ty.key),
                    span: ty.span,
                });
            }
        }
        MirTypeDefKind::Enum { variants, methods } => {
            for variant in variants {
                if variant.name.is_empty() || variant.wire_name.is_empty() {
                    return Err(MirLegalityError::InvalidReference {
                        function: MirFunctionId(0),
                        subject: format!("type {} variant", ty.key),
                        span: variant.span,
                    });
                }
                match &variant.payload {
                    crate::MIR::MirVariantPayload::Unit => {}
                    crate::MIR::MirVariantPayload::Single(value) => {
                        verify_type(value, variant.span, MirFunctionId(0))?
                    }
                    crate::MIR::MirVariantPayload::Named(fields) => {
                        for field in fields {
                            if !field_ids.contains(&field.id)
                                || field.name.is_empty()
                                || !valid_shape_field_names(&field.shape_names)
                            {
                                return Err(MirLegalityError::InvalidReference {
                                    function: MirFunctionId(0),
                                    subject: format!("type {} field {:?}", ty.key, field.id),
                                    span: field.span,
                                });
                            }
                            verify_type(&field.ty, field.span, MirFunctionId(0))?;
                        }
                    }
                }
            }
            if methods.iter().any(|method| !function_ids.contains(method)) {
                return Err(MirLegalityError::InvalidReference {
                    function: MirFunctionId(0),
                    subject: format!("type {} method", ty.key),
                    span: ty.span,
                });
            }
        }
        MirTypeDefKind::Distinct { base, .. } | MirTypeDefKind::Alias { target: base } => {
            verify_type(base, ty.span, MirFunctionId(0))?;
        }
        MirTypeDefKind::UnitFamily { .. } => {}
    }
    Ok(())
}

fn verify_type(ty: &MirType, span: Span, function: MirFunctionId) -> Result<(), MirLegalityError> {
    if span.start > span.end {
        return Err(MirLegalityError::InvalidSpan { function: Some(function), span });
    }
    if !ty.has_valid_layout() {
        return Err(MirLegalityError::InvalidLayout { function, span });
    }
    verify_type_kind(ty.kind(), span, function)
}

fn verify_type_kind(
    kind: &MirTypeKind,
    span: Span,
    function: MirFunctionId,
) -> Result<(), MirLegalityError> {
    let invalid = || Err(MirLegalityError::InvalidType { function, span });
    match kind {
        MirTypeKind::IntN { bits, .. } if *bits == 0 || *bits > 64 => invalid(),
        MirTypeKind::IntN { .. } => Ok(()),
        MirTypeKind::InlineRange { lo, hi, .. } if lo > hi => invalid(),
        MirTypeKind::List(inner)
        | MirTypeKind::Shared(inner)
        | MirTypeKind::Option(inner)
        | MirTypeKind::FixedList { elem: inner, .. }
        | MirTypeKind::Tagged { inner, .. }
        | MirTypeKind::Quantity { base: inner, .. } => verify_type(inner, span, function),
        MirTypeKind::Map { key, value } => {
            verify_type(key, span, function)?;
            verify_type(value, span, function)
        }
        MirTypeKind::Result { ok, err } => {
            verify_type(ok, span, function)?;
            verify_type(err, span, function)
        }
        MirTypeKind::Fn(signature) => {
            for param in &signature.params {
                verify_type(param, span, function)?;
            }
            if let Some(ret) = &signature.ret {
                verify_type(ret, span, function)?;
            }
            Ok(())
        }
        MirTypeKind::SendFn { params, ret } => {
            for param in params {
                verify_type(param, span, function)?;
            }
            if let Some(ret) = ret {
                verify_type(ret, span, function)?;
            }
            Ok(())
        }
        MirTypeKind::Apply { args, .. } | MirTypeKind::Union(args) => {
            for arg in args {
                verify_type(arg, span, function)?;
            }
            Ok(())
        }
        MirTypeKind::Tuple(fields) => {
            for (_, field) in fields {
                verify_type(field, span, function)?;
            }
            Ok(())
        }
        MirTypeKind::InlineRange { base, .. } => verify_type(base, span, function),
        MirTypeKind::Int
        | MirTypeKind::Float
        | MirTypeKind::Bool
        | MirTypeKind::String
        | MirTypeKind::Char
        | MirTypeKind::TraitObject(_)
        | MirTypeKind::Float32
        | MirTypeKind::Measure(_) => Ok(()),
    }
}
fn verify_function_form(
    form: &crate::MIR::MirFunctionForm,
    type_ids: &HashSet<MirTypeId>,
    trait_ids: &HashSet<crate::MIR::MirTraitId>,
    span: Span,
    function: MirFunctionId,
) -> Result<(), MirLegalityError> {
    let invalid = |subject: String| {
        Err(MirLegalityError::InvalidReference {
            function,
            subject,
            span,
        })
    };
    let check_owner = |owner: &MirType| -> Result<(), MirLegalityError> {
        verify_type(owner, span, function)?;
        let Some(owner_id) = owner.identity else {
            return invalid("function form owner has no type identity".to_string());
        };
        if !type_ids.contains(&owner_id) {
            return invalid(format!("function form owner type {owner_id:?}"));
        }
        Ok(())
    };
    match form {
        crate::MIR::MirFunctionForm::TopLevel => Ok(()),
        crate::MIR::MirFunctionForm::Method { owner, .. } => check_owner(owner),
        crate::MIR::MirFunctionForm::TraitMethod {
            owner,
            trait_ref,
            ..
        } => {
            check_owner(owner)?;
            if trait_ref.id.0 == 0 || !trait_ids.contains(&trait_ref.id) || trait_ref.name.is_empty() {
                return invalid(format!("function form trait {:?}", trait_ref.id));
            }
            Ok(())
        }
    }
}

fn verify_function(
    function: &MirFunction,
    function_ids: &HashSet<MirFunctionId>,
    type_ids: &HashSet<MirTypeId>,
    trait_ids: &HashSet<crate::MIR::MirTraitId>,
    field_ids: &HashSet<MirFieldId>,
    source_file_ids: &HashSet<MirSourceFileId>,
    foreign_ids: &HashSet<crate::MIR::MirForeignId>,
    callback_ids: &HashSet<crate::MIR::MirCallbackId>,
    core_ids: &HashSet<crate::MIR::MirCoreCallId>,
    core_calls: &HashMap<crate::MIR::MirCoreCallId, &crate::MIR::MirCoreCall>,
    prelude_calls: &HashMap<MirPreludeCallId, &MirPreludeCall>,
) -> Result<(), MirLegalityError> {
    let span = function.span;
    if span.start > span.end {
        return Err(MirLegalityError::InvalidSpan { function: Some(function.id), span });
    }
    verify_function_form(&function.form, type_ids, trait_ids, span, function.id)?;
    verify_generic_params(&function.generic_params, trait_ids, span)?;
    for (slot, capture) in function.capture_params.iter().enumerate() {
        if capture.slot != slot || capture.name.is_empty() {
            return Err(MirLegalityError::InvalidReference {
                function: function.id,
                subject: format!("capture parameter slot {}", capture.slot),
                span: capture.span,
            });
        }
        verify_type(&capture.ty, capture.span, function.id)?;
        verify_ownership(
            capture.ownership,
            capture.span,
            function.id,
            MirValueId(capture.slot as u64 + 1),
        )?;
    }
    for (index, param) in function.params.iter().enumerate() {
        if param.index != index {
            return Err(MirLegalityError::InvalidReference { function: function.id, subject: format!("parameter index {}", param.index), span: param.span });
        }
        verify_type(&param.ty, param.span, function.id)?;
        verify_ownership(param.ownership, param.span, function.id, MirValueId(param.index as u64 + 1))?;
    }
    verify_type(&function.return_type, function.span, function.id)?;
    if let Some(declared) = &function.declared_return {
        verify_type(declared, function.span, function.id)?;
    }
    verify_failure_carrier(&function.failure, function)?;
    if let Some(generator) = &function.generator {
        verify_type(&generator.item, function.span, function.id)?;
    }

    let block_ids: HashSet<MirBlockId> = function.blocks.iter().map(|block| block.id).collect();
    let scope_ids: HashSet<_> = function.scopes.iter().map(|scope| scope.id).collect();
    let local_ids: HashSet<_> = function.locals.iter().map(|local| local.id).collect();
    let place_map: HashMap<_, _> = function.places.iter().map(|place| (place.id, place)).collect();
    let mut instruction_ids = HashSet::new();
    let mut defs: HashMap<MirValueId, (MirBlockId, usize, MirType, MirOwnership)> = HashMap::new();
    for block in &function.blocks {
        if block.span.start > block.span.end {
            return Err(MirLegalityError::InvalidSpan { function: Some(function.id), span: block.span });
        }
        for (index, instruction) in block.instructions.iter().enumerate() {
            if !instruction_ids.insert(instruction.id) {
                return Err(MirLegalityError::DuplicateInstruction { function: function.id, id: instruction.id.0 });
            }
            if instruction.span.start > instruction.span.end {
                return Err(MirLegalityError::InvalidSpan { function: Some(function.id), span: instruction.span });
            }
            if let Some(ty) = &instruction.ty {
                verify_type(ty, instruction.span, function.id)?;
            }
            if let Some(value) = instruction.result {
                let Some(ty) = instruction.ty.clone() else {
                    return Err(MirLegalityError::InvalidType { function: function.id, span: instruction.span });
                };
                let ownership = function
                    .values
                    .iter()
                    .find(|(candidate, _, _, _)| *candidate == value)
                    .map(|(_, _, _, ownership)| *ownership)
                    .unwrap_or_else(MirOwnership::copy);
                if defs.insert(value, (block.id, index, ty, ownership)).is_some() {
                    return Err(MirLegalityError::Validation(MirValidationError::DuplicateId { kind: "value", id: value.0 }));
                }
            } else if instruction.ty.is_some() {
                return Err(MirLegalityError::InvalidType { function: function.id, span: instruction.span });
            }
        }
    }
    let mut metadata_ids = HashSet::new();
    for (value, ty, value_span, ownership) in &function.values {
        if !metadata_ids.insert(*value) {
            return Err(MirLegalityError::Validation(MirValidationError::DuplicateId { kind: "value metadata", id: value.0 }));
        }
        verify_type(ty, *value_span, function.id)?;
        verify_ownership(*ownership, *value_span, function.id, *value)?;
        if !defs.contains_key(value) {
            return Err(MirLegalityError::UnexpectedValueMetadata { function: function.id, value: *value });
        }
    }
    for value in defs.keys() {
        if !metadata_ids.contains(value) {
            return Err(MirLegalityError::MissingValueMetadata { function: function.id, value: *value });
        }
    }
    for local in &function.locals {
        verify_type(&local.ty, local.span, function.id)?;
        verify_ownership(local.ownership, local.span, function.id, MirValueId(local.id.0))?;
        if !place_map.contains_key(&local.place) {
            return Err(MirLegalityError::InvalidReference {
                function: function.id,
                subject: format!("local {} place", local.name),
                span: local.span,
            });
        }
    }
    for place in &function.places {
        verify_place(place, function, &local_ids, &defs, field_ids, source_file_ids, prelude_calls)?;
    }

    let reachable = reachable_blocks(function);
    let predecessors = predecessor_map(function);
    let dominators = dominator_map(function, &reachable, &predecessors);
    for block in &function.blocks {
        verify_block_references(
            block,
            function,
            &block_ids,
            &scope_ids,
            &defs,
            &dominators,
            &predecessors,
            &place_map,
            function_ids,
            type_ids,
            field_ids,
            source_file_ids,
            foreign_ids,
            callback_ids,
            core_ids,
            core_calls,
            prelude_calls,
        )?;
    }
    verify_host_borrow_callback_uses(function, &prelude_calls)?;
    verify_drops(function, &block_ids, &place_map)?;
    verify_moves_and_borrows(function, &defs, &dominators, &reachable, &place_map)?;
    verify_facts(function, &block_ids, &instruction_ids, &defs)?;
    Ok(())
}


fn verify_capture_operations(function: &MirFunction) -> Result<(), MirLegalityError> {
    for block in &function.blocks {
        for instruction in &block.instructions {
            let MirOperation::Capture { slot } = &instruction.operation else {
                continue;
            };
            let Some(capture) = function.capture_params.get(*slot) else {
                return Err(MirLegalityError::InvalidReference {
                    function: function.id,
                    subject: format!("capture slot {slot}"),
                    span: instruction.span,
                });
            };
            if instruction.ty.as_ref() != Some(&capture.ty) {
                return Err(MirLegalityError::InvalidType {
                    function: function.id,
                    span: instruction.span,
                });
            }
        }
    }
    Ok(())
}


fn verify_transparent_conversions(
    function: &MirFunction,
    type_defs: &[crate::MIR::MirTypeDef],
) -> Result<(), MirLegalityError> {
    let value_types: HashMap<MirValueId, &MirType> =
        function.values.iter().map(|value| (value.0, &value.1)).collect();
    for block in &function.blocks {
        for instruction in &block.instructions {
            let MirOperation::Convert {
                value,
                parameters,
                target,
                conversion: MirConversion::Transparent,
            } = &instruction.operation
            else {
                continue;
            };
            let Some(source) = value_types.get(value) else {
                continue;
            };
            if !parameters.is_empty() || !transparent_wrapper_pair(source, target, type_defs) {
                return Err(MirLegalityError::InvalidReference {
                    function: function.id,
                    subject: "transparent conversion is not a representation-transparent distinct pair"
                        .to_string(),
                    span: instruction.span,
                });
            }
        }
    }
    Ok(())
}

fn transparent_wrapper_pair(
    source: &MirType,
    target: &MirType,
    type_defs: &[crate::MIR::MirTypeDef],
) -> bool {
    let distinct_base = |identity: Option<MirTypeId>| {
        identity.and_then(|id| {
            type_defs.iter().find(|definition| definition.id == id).and_then(|definition| {
                match &definition.kind {
                    MirTypeDefKind::Distinct { base, .. } => Some(base),
                    _ => None,
                }
            })
        })
    };
    let source_base = distinct_base(source.identity);
    let target_base = distinct_base(target.identity);
    if source_base.is_none() && target_base.is_none() {
        return false;
    }
    let left = source_base.unwrap_or(source);
    let right = target_base.unwrap_or(target);
    left.kind == right.kind && left.layout == right.layout
}

fn verify_failure_carrier(carrier: &MirFailureCarrier, function: &MirFunction) -> Result<(), MirLegalityError> {
    verify_failure_carrier_at(carrier, function.id, function.span)
}

fn verify_failure_carrier_at(
    carrier: &MirFailureCarrier,
    function: MirFunctionId,
    span: Span,
) -> Result<(), MirLegalityError> {
    match carrier {
        MirFailureCarrier::Infallible => Ok(()),
        MirFailureCarrier::Result { success, error } => {
            verify_type(success, span, function)?;
            verify_type(error, span, function)
        }
        MirFailureCarrier::Optional { value } | MirFailureCarrier::Diverges { value } => {
            verify_type(value, span, function)
        }
    }
}

fn verify_ownership(
    ownership: MirOwnership,
    span: Span,
    function: MirFunctionId,
    value: MirValueId,
) -> Result<(), MirLegalityError> {
    let valid = match ownership.mode {
        MirOwnershipMode::Copy => ownership.drop == crate::MIR::MirDropKind::None && !ownership.moved,
        MirOwnershipMode::Owned => matches!(ownership.drop, crate::MIR::MirDropKind::Value | crate::MIR::MirDropKind::ForeignHandle),
        MirOwnershipMode::Shared => ownership.drop == crate::MIR::MirDropKind::Shared,
        MirOwnershipMode::ReadBorrow => matches!(ownership.drop, crate::MIR::MirDropKind::None | crate::MIR::MirDropKind::View),
        MirOwnershipMode::WriteBorrow => ownership.drop == crate::MIR::MirDropKind::View,
        MirOwnershipMode::Move => matches!(ownership.drop, crate::MIR::MirDropKind::Value | crate::MIR::MirDropKind::ForeignHandle),
    };
    if valid {
        Ok(())
    } else {
        Err(MirLegalityError::InvalidOwnership { function, value, span })
    }
}

fn index_route_arity(kind: MirIndexKind) -> Option<usize> {
    match kind {
        MirIndexKind::List | MirIndexKind::FixedListProof => Some(4),
        MirIndexKind::Map => Some(8),
        MirIndexKind::Pool => Some(6),
        MirIndexKind::Lane => None,
    }
}

fn verify_panic_context(
    context: &MirPanicContext,
    function: &MirFunction,
    span: Span,
) -> Result<(), MirLegalityError> {
    let invalid = |subject: String| {
        Err(MirLegalityError::InvalidReference {
            function: function.id,
            subject,
            span,
        })
    };
    if context.function.is_empty() {
        return invalid("panic context has no function".to_string());
    }
    let mut seen_locals = HashSet::new();
    for (name, local) in &context.locals {
        if name.is_empty() {
            return invalid("panic context has an unnamed local".to_string());
        }
        if !seen_locals.insert(*local) {
            return invalid(format!("duplicate panic context local {local:?}"));
        }
        if !function.locals.iter().any(|candidate| candidate.id == *local) {
            return invalid(format!("panic context local {local:?}"));
        }
    }
    Ok(())
}

fn guard_paths_are_distinct(first: &[MirFieldId], second: &[MirFieldId]) -> bool {
    first != second && !first.starts_with(second) && !second.starts_with(first)
}



fn verify_place(
    place: &MirPlace,
    function: &MirFunction,
    local_ids: &HashSet<crate::MIR::MirLocalId>,
    defs: &HashMap<MirValueId, (MirBlockId, usize, MirType, MirOwnership)>,
    field_ids: &HashSet<MirFieldId>,
    source_file_ids: &HashSet<MirSourceFileId>,
    prelude_calls: &HashMap<MirPreludeCallId, &MirPreludeCall>,
) -> Result<(), MirLegalityError> {
    verify_type(&place.ty, place.span, function.id)?;
    match &place.base {
        MirPlaceBase::Local(local) if !local_ids.contains(local) => {
            return Err(MirLegalityError::InvalidReference { function: function.id, subject: format!("local {local:?}"), span: place.span });
        }
        MirPlaceBase::Parameter(value)
        | MirPlaceBase::Capture(value)
        | MirPlaceBase::Temporary(value)
            if !defs.contains_key(value) =>
        {
            return Err(MirLegalityError::InvalidReference {
                function: function.id,
                subject: format!("place base {value:?}"),
                span: place.span,
            });
        }
        MirPlaceBase::Static(name) if name.is_empty() => {
            return Err(MirLegalityError::InvalidReference { function: function.id, subject: "empty static place".to_string(), span: place.span });
        }
        _ => {}
    }
    for projection in &place.projections {
        match projection {
            MirProjection::Field { field, span } => {
                if !field_ids.contains(field) {
                    return Err(MirLegalityError::InvalidReference {
                        function: function.id,
                        subject: format!("field projection {field:?}"),
                        span: *span,
                    });
                }
                if span.start > span.end {
                    return Err(MirLegalityError::InvalidSpan { function: Some(function.id), span: *span });
                }
            }
            MirProjection::Index {
                kind,
                index,
                call,
                write_call,
                location,
                context,
                span,
            } => {
                if !defs.contains_key(index) {
                    return Err(MirLegalityError::InvalidReference { function: function.id, subject: format!("index {index:?}"), span: *span });
                }
                let Some(route) = prelude_calls.get(call) else {
                    return Err(MirLegalityError::InvalidReference {
                        function: function.id,
                        subject: format!("Prelude call {call:?}"),
                        span: *span,
                    });
                };
                let Some(arity) = index_route_arity(*kind) else {
                    return Err(MirLegalityError::InvalidReference {
                        function: function.id,
                        subject: format!("index kind {kind:?} has no checked route"),
                        span: *span,
                    });
                };
                if route.signature.arity != arity
                    || route.signature.max_arity != arity
                    || route.abi != crate::MIR::MirPreludeAbi::Value
                {
                    return Err(MirLegalityError::InvalidReference {
                        function: function.id,
                        subject: format!("index projection route {call:?} must use Value ABI with arity {arity}"),
                        span: *span,
                    });
                }
                if matches!(place.access, MirAccess::Write) != write_call.is_some() {
                    return Err(MirLegalityError::InvalidReference {
                        function: function.id,
                        subject: "index write accessor disagrees with place access".to_string(),
                        span: *span,
                    });
                }
                if let Some(write_call) = write_call {
                    let Some(write_route) = prelude_calls.get(write_call) else {
                        return Err(MirLegalityError::InvalidReference {
                            function: function.id,
                            subject: format!("Prelude write call {write_call:?}"),
                            span: *span,
                        });
                    };
                    let setter_arity = match kind {
                        MirIndexKind::List | MirIndexKind::FixedListProof => 5,
                        MirIndexKind::Map => 3,
                        MirIndexKind::Pool => 7,
                        MirIndexKind::Lane => {
                            return Err(MirLegalityError::InvalidReference {
                                function: function.id,
                                subject: "lane index place has no checked setter route".to_string(),
                                span: *span,
                            });
                        }
                    };
                    if write_route.signature.arity != setter_arity
                        || write_route.signature.max_arity != setter_arity
                        || write_route.abi != crate::MIR::MirPreludeAbi::Value
                    {
                        return Err(MirLegalityError::InvalidReference {
                            function: function.id,
                            subject: format!("index setter route {write_call:?} must use Value ABI with arity {setter_arity}"),
                            span: *span,
                        });
                    }
                }
                if matches!(kind, MirIndexKind::Pool | MirIndexKind::Map) != context.is_some() {
                    return Err(MirLegalityError::InvalidReference {
                        function: function.id,
                        subject: "index projection context disagrees with index kind".to_string(),
                        span: *span,
                    });
                }
                if let Some(context) = context {
                    verify_panic_context(context, function, *span)?;
                }
                if !source_file_ids.contains(&location.file) {
                    return Err(MirLegalityError::InvalidReference { function: function.id, subject: format!("source file {:?}", location.file), span: *span });
                }
                if span.start > span.end {
                    return Err(MirLegalityError::InvalidSpan { function: Some(function.id), span: *span });
                }
            }
            MirProjection::Deref { span } if span.start > span.end => {
                return Err(MirLegalityError::InvalidSpan { function: Some(function.id), span: *span });
            }
            MirProjection::Deref { .. } => {}
        }
    }
    if matches!(place.access, MirAccess::Read) && place.projections.iter().any(|projection| matches!(projection, MirProjection::Deref { .. })) {
        // A read through a dereference is legal; this arm intentionally keeps
        // the access check local to the place rather than inferring pointers.
    }
    Ok(())
}
fn verify_block_references(
    block: &MirBasicBlock,
    function: &MirFunction,
    block_ids: &HashSet<MirBlockId>,
    scope_ids: &HashSet<crate::MIR::MirScopeId>,
    defs: &HashMap<MirValueId, (MirBlockId, usize, MirType, MirOwnership)>,
    dominators: &HashMap<MirBlockId, BTreeSet<MirBlockId>>,
    predecessors: &HashMap<MirBlockId, BTreeSet<MirBlockId>>,
    place_map: &HashMap<crate::MIR::MirPlaceId, &MirPlace>,
    function_ids: &HashSet<MirFunctionId>,
    type_ids: &HashSet<MirTypeId>,
    field_ids: &HashSet<MirFieldId>,
    source_file_ids: &HashSet<MirSourceFileId>,
    foreign_ids: &HashSet<crate::MIR::MirForeignId>,
    callback_ids: &HashSet<crate::MIR::MirCallbackId>,
    core_ids: &HashSet<crate::MIR::MirCoreCallId>,
    core_calls: &HashMap<crate::MIR::MirCoreCallId, &crate::MIR::MirCoreCall>,
    prelude_calls: &HashMap<MirPreludeCallId, &MirPreludeCall>,
) -> Result<(), MirLegalityError> {
    let reachable = reachable_blocks(function);
    let block_predecessors = predecessors.get(&block.id).cloned().unwrap_or_default();
    for (index, instruction) in block.instructions.iter().enumerate() {
        let operation = &instruction.operation;
        if reachable.contains(&block.id) {
            if let MirOperation::Phi { incoming } = operation {
                // A phi's operands are uses on the incoming edge, not at the
                // join: each must be defined by the end of its predecessor.
                // This is the same rule `MirProgram::validate` states; arm
                // values can never dominate the join itself.
                let mut seen = BTreeSet::new();
                for (predecessor, value) in incoming {
                    if !seen.insert(*predecessor)
                        || !block_ids.contains(predecessor)
                        || !block_predecessors.contains(predecessor)
                        || !reachable.contains(predecessor)
                    {
                        return Err(MirLegalityError::InvalidReference { function: function.id, subject: format!("phi predecessor {predecessor:?}"), span: instruction.span });
                    }
                    let predecessor_index = function
                        .blocks
                        .iter()
                        .find(|candidate| candidate.id == *predecessor)
                        .map_or(0, |candidate| candidate.instructions.len());
                    ensure_value_dominates(function, defs, dominators, *predecessor, predecessor_index, *value, instruction.span)?;
                }
                let expected: BTreeSet<_> = block_predecessors.intersection(&reachable).copied().collect();
                if seen != expected {
                    return Err(MirLegalityError::InvalidReference { function: function.id, subject: format!("phi edges for block {:?}", block.id), span: instruction.span });
                }
            } else {
                for value in operation.value_uses() {
                    ensure_value_dominates(function, defs, dominators, block.id, index, value, instruction.span)?;
                }
            }
            for place_id in operation_place_refs(operation) {
                let Some(place) = place_map.get(&place_id).copied() else {
                    return Err(MirLegalityError::InvalidReference { function: function.id, subject: format!("place {place_id:?}"), span: instruction.span });
                };
                for value in place_value_uses(place) {
                    ensure_value_dominates(function, defs, dominators, block.id, index, value, instruction.span)?;
                }
            }
        } else {
            for place_id in operation_place_refs(operation) {
                if !place_map.contains_key(&place_id) {
                    return Err(MirLegalityError::InvalidReference { function: function.id, subject: format!("place {place_id:?}"), span: instruction.span });
                }
            }
        }
        verify_operation_metadata(
            operation,
            instruction.span,
            function,
            block_ids,
            scope_ids,
            defs,
            function_ids,
            type_ids,
            field_ids,
            source_file_ids,
            foreign_ids,
            callback_ids,
            core_ids,
            core_calls,
            prelude_calls,
        )?;
    }
    if reachable.contains(&block.id) {
        for value in block.terminator.value_uses() {
            ensure_value_dominates(function, defs, dominators, block.id, block.instructions.len(), value, block.span)?;
        }
    }
    for target in block.terminator.targets() {
        if !block_ids.contains(&target) {
            return Err(MirLegalityError::InvalidReference { function: function.id, subject: format!("target {target:?}"), span: block.span });
        }
    }
    Ok(())
}

fn mir_type_is_copy(ty: &MirType) -> bool {
    ty.is_scalar()
        || ty.is_char()
        || matches!(
            &ty.kind,
            MirTypeKind::Apply { name, args }
                if args.is_empty()
                    && (name.name == crate::Syntax::TYPE_COMPLEX
                        || name.name == crate::Syntax::TYPE_RANGE)
            )
}

fn mir_move_place_has_unowned_projection(place: &MirPlace) -> bool {
    !mir_type_is_copy(&place.ty)
        && place.projections.iter().any(|projection| {
            matches!(
                projection,
                MirProjection::Index {
                    kind: MirIndexKind::Lane,
                    ..
                } | MirProjection::Deref { .. }
            )
        })
}

fn call_args_mutably_borrow_place(
    args: &[crate::MIR::MirCallArg],
    place: crate::MIR::MirPlaceId,
) -> bool {
    args.iter()
        .any(|arg| arg.place == Some(place) && arg.access == MirAccess::Write)
}

fn operation_mutably_borrows_place(operation: &MirOperation, place: crate::MIR::MirPlaceId) -> bool {
    match operation {
        MirOperation::AddressOf {
            place: candidate,
            access: MirAccess::Write,
        } => *candidate == place,
        MirOperation::Call { args, .. }
        | MirOperation::CoreCall { args, .. }
        | MirOperation::IndirectCall { args, .. } => call_args_mutably_borrow_place(args, place),
        MirOperation::Semantic(
            MirSemanticOp::StaticPreludeCall { args, .. }
            | MirSemanticOp::HardwareCall { args, .. }
            | MirSemanticOp::ClosureMethod { args, .. }
            | MirSemanticOp::HostCall { args, .. },
        ) => call_args_mutably_borrow_place(args, place),
        MirOperation::Semantic(MirSemanticOp::BuiltinMethod {
            receiver_place: Some(candidate),
            ..
        }) => *candidate == place,
        _ => false,
    }
}

fn verify_static_mutable_place(
    operation: &MirOperation,
    place: &MirPlace,
    function: MirFunctionId,
    span: Span,
) -> Result<(), MirLegalityError> {
    let invalid = |subject: String| {
        Err(MirLegalityError::InvalidReference {
            function,
            subject,
            span,
        })
    };
    if operation_mutably_borrows_place(operation, place.id)
        && matches!(&place.base, MirPlaceBase::Static(_))
    {
        return invalid(format!(
            "mutable borrow of static place {:?} cannot escape its storage cell",
            place.id
        ));
    }
    if matches!(operation, MirOperation::WritePlace { .. })
        && matches!(&place.base, MirPlaceBase::Static(_))
        && !place.projections.is_empty()
    {
        return invalid(format!(
            "static write place {:?} must assign the whole static root",
            place.id
        ));
    }
    Ok(())
}

fn verify_move_place_projection(
    operation: &MirOperation,
    function: MirFunctionId,
    span: Span,
    places: &[MirPlace],
) -> Result<(), MirLegalityError> {
    let MirOperation::MovePlace { place: place_id } = operation else {
        return Ok(());
    };
    let Some(place) = places.iter().find(|candidate| candidate.id == *place_id) else {
        return Ok(());
    };
    if mir_move_place_has_unowned_projection(place) {
        return Err(MirLegalityError::InvalidReference {
            function,
            subject: format!(
                "non-Copy MovePlace {:?} has a Lane or dereference projection without an owning representation",
                place.id
            ),
            span,
        });
    }
    Ok(())
}


fn send_fn_signature_matches(source: &MirType, target: &MirType) -> bool {
    let target = match &target.kind {
        MirTypeKind::SendFn { params, ret } => (params, ret),
        _ => return false,
    };
    let (params, ret) = match &source.kind {
        MirTypeKind::SendFn { params, ret } => (params, ret),
        _ => return false,
    };
    params.len() == target.0.len()
        && params
            .iter()
            .zip(target.0)
            .all(|(source, target)| source.same_checked_type(target))
        && match (ret.as_deref(), target.1.as_deref()) {
            (None, None) => true,
            (Some(source), Some(target)) => source.same_checked_type(target),
            _ => false,
        }
}


fn verify_operation_metadata(
    operation: &MirOperation,
    span: Span,
    function: &MirFunction,
    block_ids: &HashSet<MirBlockId>,
    scope_ids: &HashSet<crate::MIR::MirScopeId>,
    defs: &HashMap<MirValueId, (MirBlockId, usize, MirType, MirOwnership)>,
    function_ids: &HashSet<MirFunctionId>,
    type_ids: &HashSet<MirTypeId>,
    field_ids: &HashSet<MirFieldId>,
    source_file_ids: &HashSet<MirSourceFileId>,
    foreign_ids: &HashSet<crate::MIR::MirForeignId>,
    callback_ids: &HashSet<crate::MIR::MirCallbackId>,
    core_ids: &HashSet<crate::MIR::MirCoreCallId>,
    core_calls: &HashMap<crate::MIR::MirCoreCallId, &crate::MIR::MirCoreCall>,
    prelude_calls: &HashMap<MirPreludeCallId, &MirPreludeCall>,
) -> Result<(), MirLegalityError> {
    let invalid = |subject: String| Err(MirLegalityError::InvalidReference { function: function.id, subject, span });
    for place_id in operation_place_refs(operation) {
        if let Some(place) = function.places.iter().find(|candidate| candidate.id == place_id) {
            verify_static_mutable_place(operation, place, function.id, span)?;
        }
    }
    verify_move_place_projection(operation, function.id, span, &function.places)?;
    let check_type_id = |type_id: MirTypeId| {
        if type_ids.contains(&type_id) {
            Ok(())
        } else {
            invalid(format!("type {type_id:?}"))
        }
    };
    let check_field_id = |field_id: MirFieldId| {
        if field_ids.contains(&field_id) {
            Ok(())
        } else {
            invalid(format!("field {field_id:?}"))
        }
    };
    let check_source_file_id = |source_file: MirSourceFileId| {
        if source_file_ids.contains(&source_file) {
            Ok(())
        } else {
            invalid(format!("source file {source_file:?}"))
        }
    };
    let check_prelude_call_id = |call: MirPreludeCallId| {
        if prelude_calls.contains_key(&call) {
            Ok(())
        } else {
            invalid(format!("Prelude call {call:?}"))
        }
    };
    match operation {
        MirOperation::Binary {
            dispatch: crate::MIR::MirBinaryDispatch::Prelude { call, location },
            ..
        } => {
            check_prelude_call_id(*call)?;
            if let Some(location) = location {
                check_source_file_id(location.file)?;
            }
        }
        MirOperation::Parameter { index, name } => {
            let Some(parameter) = function.params.get(*index) else {
                return invalid(format!("parameter {index}"));
            };
            if name.is_empty() || name != &parameter.name {
                return invalid(format!("parameter name {name:?} at index {index}"));
            }
        }
        MirOperation::Capture { slot } => {
            if function.capture_params.get(*slot).is_none() {
                return invalid(format!("capture slot {slot}"));
            }
        }
        MirOperation::Call { callee, args, type_args } => {
            verify_call_callee(
                callee,
                block_ids,
                function_ids,
                foreign_ids,
                core_ids,
                prelude_calls,
                function,
                span,
            )?;
            if let MirCallee::Prelude(call) = callee {
                let Some(route) = prelude_calls.get(call) else {
                    return invalid(format!("Prelude callee {call:?}"));
                };
                if args.len() < route.signature.arity || args.len() > route.signature.max_arity {
                    return invalid(format!("Prelude call {call:?} arity {}", args.len()));
                }
            }
            if let MirCallee::Associated { function: target, owner }
            | MirCallee::Method { function: target, owner } = callee
            {
                if !function_ids.contains(target) {
                    return invalid(format!("associated callee {target:?}"));
                }
                let Some(owner_id) = owner.identity else {
                    return invalid("associated callee has no owner type identity".to_string());
                };
                if !type_ids.contains(&owner_id) {
                    return invalid(format!("associated owner type {owner_id:?}"));
                }
                verify_type(owner, span, function.id)?;
            }
            if let MirCallee::TraitMethod { receiver, .. } = callee {
                let Some(receiver_id) = receiver.identity else {
                    return invalid("trait method callee has no receiver type identity".to_string());
                };
                if !type_ids.contains(&receiver_id) {
                    return invalid(format!("trait method receiver type {receiver_id:?}"));
                }
                verify_type(receiver, span, function.id)?;
            }
            verify_call_args(args, type_ids, defs, function.id, &invalid)?;
            for ty in type_args {
                verify_type(ty, span, function.id)?;
            }
        }
        MirOperation::IndirectCall { args, type_args, .. } => {
            verify_call_args(args, type_ids, defs, function.id, &invalid)?;
            for ty in type_args {
                verify_type(ty, span, function.id)?;
            }
        }
        MirOperation::CoreCall {
            call,
            route,
            args,
            type_args,
            fallibility,
            data_plan,
        } => {
            let Some(record) = core_calls.get(call) else {
                return invalid(format!("Core call {call:?}"));
            };
            let Some(route_record) = prelude_calls.get(route) else {
                return invalid(format!("Prelude call {route:?}"));
            };
            if args.len() < record.arity
                || args.len() > record.max_arity
                || args.len() < route_record.signature.arity
                || args.len() > route_record.signature.max_arity
            {
                return invalid(format!("Core call {} arity {}", record.key, args.len()));
            }
            if record.arity != route_record.signature.arity
                || record.max_arity != route_record.signature.max_arity
                || record.borrow_mask != route_record.signature.borrow_mask
            {
                return invalid(format!(
                    "Core call {} signature disagrees with route {}",
                    record.key, route.0
                ));
            }
            match (&record.symbol, &route_record.symbol) {
                (
                    crate::Syntax::CoreCallSymbol::Prelude(core_symbol),
                    crate::MIR::MirSymbol::Prelude(route_symbol),
                ) if *core_symbol == route_symbol => {}
                (
                    crate::Syntax::CoreCallSymbol::Rust(core_symbol),
                    crate::MIR::MirSymbol::Runtime(route_symbol),
                ) if *core_symbol == route_symbol => {}
                _ => {
                    return invalid(format!(
                        "Core call {} symbol disagrees with route {}",
                        record.key, route.0
                    ))
                }
            }
            if &route_record.fallibility != fallibility {
                return invalid(format!("Core call {} fallibility disagrees with route {}", record.key, route.0));
            }
            verify_call_args(args, type_ids, defs, function.id, &invalid)?;
            for ty in type_args {
                verify_type(ty, span, function.id)?;
            }
            verify_call_fallibility(fallibility, function.id, span)?;
            if let Some(plan) = data_plan {
                if !plan.validate() {
                    return invalid(format!("Core call {} has an invalid data plan", record.key));
                }
                for node in &plan.logical {
                    verify_type(&node.row_type, node.span, function.id)?;
                    for column in &node.columns {
                        verify_type(&column.ty, node.span, function.id)?;
                    }
                    if let Some(callable) = &node.callable {
                        verify_type(&callable.parameter, callable.span, function.id)?;
                        verify_type(&callable.result, callable.span, function.id)?;
                    }
                }
            }
        }
        MirOperation::MovePlace { place } => {
            let Some(place_row) = function.places.iter().find(|candidate| candidate.id == *place) else {
                return invalid(format!("move place {place:?}"));
            };
            if place_row.access != MirAccess::Move {
                return invalid(format!("move place {place:?} is not a move-access place"));
            }
            if matches!(&place_row.base, MirPlaceBase::Static(_))
                && !place_row.projections.is_empty()
            {
                return invalid(format!(
                    "static move place {place:?} must target the whole static root"
                ));
            }
        }
        MirOperation::InitializeUninit { place } => {
            let Some(place_row) = function.places.iter().find(|candidate| candidate.id == *place) else {
                return invalid(format!("uninitialized place {place:?}"));
            };
            let MirPlaceBase::Local(local) = &place_row.base else {
                return invalid(format!("uninitialized place {place:?} is not a local root"));
            };
            let Some(local_row) = function.locals.iter().find(|candidate| candidate.id == *local) else {
                return invalid(format!("uninitialized place {place:?} has no local"));
            };
            if local_row.place != *place || !local_row.uninit || !place_row.projections.is_empty() {
                return invalid(format!("uninitialized place {place:?} is not an uninitialized local root"));
            }
        }
        MirOperation::ProjectMembers { members, .. } => {
            for member in members {
                check_field_id(*member)?;
            }
        }
        MirOperation::Field { field, .. } => {
            check_field_id(*field)?;
        }
        MirOperation::Index {
            call,
            location,
            kind,
            context,
            ..
        } => {
            check_prelude_call_id(*call)?;
            let Some(route) = prelude_calls.get(call) else {
                return invalid(format!("Prelude call {call:?}"));
            };
            let Some(arity) = index_route_arity(*kind) else {
                return invalid(format!("index kind {kind:?} has no checked route"));
            };
            if route.signature.arity != arity
                || route.signature.max_arity != arity
                || route.abi != crate::MIR::MirPreludeAbi::Value
            {
                return invalid(format!("index route {call:?} must use Value ABI with arity {arity}"));
            }
            check_source_file_id(location.file)?;
            verify_panic_context(context, function, span)?;
        }
        MirOperation::Slice { call, range, location, .. } => {
            check_prelude_call_id(*call)?;
            let Some(route) = prelude_calls.get(call) else {
                return invalid(format!("Prelude call {call:?}"));
            };
            let arity = if range.is_some() { 4 } else { 5 };
            if route.signature.arity != arity
                || route.signature.max_arity != arity
                || route.abi != crate::MIR::MirPreludeAbi::Value
            {
                return invalid(format!("slice route {call:?} must use Value ABI with arity {arity}"));
            }
            check_source_file_id(location.file)?;
        }
        MirOperation::Enum { type_id, variant, args } => {
            check_type_id(*type_id)?;
            if variant.is_empty() {
                return invalid("empty enum variant".to_string());
            }
            for arg in args {
                if let Some(field) = arg.field {
                    check_field_id(field)?;
                }
            }
        }
        MirOperation::EnumIs { owner, variant, .. } | MirOperation::EnumPayload { owner, variant, .. } => {
            check_type_id(*owner)?;
            if variant.is_empty() {
                return invalid("empty enum variant".to_string());
            }
        }
        MirOperation::LoopRangeInit { call, .. } => {
            check_prelude_call_id(*call)?;
            let Some(route) = prelude_calls.get(call) else {
                return invalid(format!("Prelude call {call:?}"));
            };
            if route.signature.arity != 5
                || route.signature.max_arity != 5
                || route.signature.borrow_mask != [false, false, false, false, false]
                || route.abi != crate::MIR::MirPreludeAbi::Value
            {
                return invalid(format!(
                    "loop range init route {call:?} must use Value ABI with arity 5"
                ));
            }
        }
        MirOperation::LoopRangeHasNext { call, .. }
        | MirOperation::LoopRangeValue { call, .. }
        | MirOperation::LoopRangeAdvance { call, .. }
        | MirOperation::LoopIterHasNext { call, .. }
        | MirOperation::LoopIterValue { call, .. }
        | MirOperation::LoopIterAdvance { call, .. } => check_prelude_call_id(*call)?,
        MirOperation::LoopIterInit { call, source_kind, .. } => {
            check_prelude_call_id(*call)?;
            let Some(route) = prelude_calls.get(call) else {
                return invalid(format!("Prelude call {call:?}"));
            };
            if route.signature.arity != 5
                || route.signature.max_arity != 5
                || route.signature.borrow_mask != [false, false, false, false, false]
                || route.abi != crate::MIR::MirPreludeAbi::Value
            {
                return invalid(format!(
                    "loop iterator init route {call:?} must use Value ABI with arity 5"
                ));
            }
            match source_kind {
                MirLoopSourceKind::Plain
                | MirLoopSourceKind::Chars
                | MirLoopSourceKind::LinesFile
                | MirLoopSourceKind::LinesStdin
                | MirLoopSourceKind::LinesProcessStream
                | MirLoopSourceKind::ChannelReceiver => {}
                MirLoopSourceKind::EncodingReader { reader_type } if reader_type.is_empty() => {
                    return invalid("loop iterator encoding reader type is empty".to_string());
                }
                MirLoopSourceKind::Iterable {
                    coll_type,
                    iter_type,
                    iter_symbol,
                    next_symbol,
                } if coll_type.is_empty()
                    || iter_type.is_empty()
                    || iter_symbol.is_empty()
                    || next_symbol.is_empty() => {
                    return invalid("loop iterator Iterable types or symbols are empty".to_string());
                }
                MirLoopSourceKind::EncodingReader { .. }
                | MirLoopSourceKind::Iterable { .. } => {}
            }
        }
        MirOperation::Todo { call, location, expected_type } => {
            check_prelude_call_id(*call)?;
            check_source_file_id(location.file)?;
            if let Some(expected_type) = expected_type {
                verify_type(expected_type, span, function.id)?;
            }
        }
        MirOperation::Closure { function: target, .. } if !function_ids.contains(target) => {
            return invalid(format!("closure function {target:?}"));
        }
        MirOperation::ScopeEnter { scope, test_member } => {
            if !scope_ids.contains(scope) {
                return invalid(format!("scope {scope:?}"));
            }
            if let Some(MirTestScopeMember::Timeout { duration }) = test_member {
                if !defs.contains_key(duration) {
                    return invalid(format!(
                        "timeout duration value {duration:?} has no definition"
                    ));
                }
            }
        }
        MirOperation::ScopeExit { scope } if !scope_ids.contains(scope) => {
            return invalid(format!("scope {scope:?}"));
        }
        MirOperation::Convert {
            value,
            parameters,
            target,
            conversion,
        } => {
            verify_type(target, span, function.id)?;
            match conversion {
                MirConversion::Transparent => {}
                MirConversion::NumericCast => {
                    let Some((_, _, source, _)) = defs.get(value) else {
                        return invalid(format!("numeric cast value {value:?} has no definition"));
                    };
                    if !parameters.is_empty() || !source.is_numeric() || !target.is_numeric() {
                        return invalid("numeric cast requires scalar numeric operands".to_string());
                    }
                }
                MirConversion::SendFn => {
                    if !parameters.is_empty() {
                        return invalid("SendFn conversion has parameters".to_string());
                    }
                    let Some((_, _, source, _)) = defs.get(value) else {
                        return invalid(format!("SendFn conversion value {value:?} has no definition"));
                    };
                    if !send_fn_signature_matches(source, target) {
                        return invalid(
                            "SendFn conversion source and target signatures disagree".to_string(),
                        );
                    }
                    if !matches!(&source.kind, MirTypeKind::SendFn { .. }) {
                        return invalid(
                            "SendFn conversion requires a source already typed as SendFn"
                                .to_string(),
                        );
                    }
                }
                MirConversion::Prelude {
                    call,
                    location,
                    fallibility,
                } => {
                    check_prelude_call_id(*call)?;
                    let Some(route) = prelude_calls.get(call) else {
                        return invalid(format!("Prelude call {call:?}"));
                    };
                    if &route.fallibility != fallibility {
                        return invalid(format!("conversion fallibility disagrees with route {}", call.0));
                    }
                    check_source_file_id(location.file)?;
                    verify_call_fallibility(fallibility, function.id, span)?;
                }
            }
        }
        MirOperation::Semantic(operation) => verify_semantic_operation(
            operation,
            span,
            function,
            type_ids,
            field_ids,
            source_file_ids,
            prelude_calls,
            callback_ids,
            defs,
        )?,
        _ => {}
    }
    Ok(())
}

fn verify_call_args(
    args: &[crate::MIR::MirCallArg],
    type_ids: &HashSet<MirTypeId>,
    defs: &HashMap<MirValueId, (MirBlockId, usize, MirType, MirOwnership)>,
    function: MirFunctionId,
    invalid: &impl Fn(String) -> Result<(), MirLegalityError>,
) -> Result<(), MirLegalityError> {
    for arg in args {
        if arg.span.start > arg.span.end {
            return Err(MirLegalityError::InvalidSpan { function: Some(function), span: arg.span });
        }
        if arg.access == MirAccess::Write && arg.place.is_none() {
            return invalid(format!(
                "write call argument {:?} has no checked place",
                arg.value
            ));
        }
        let Some((_, _, _, ownership)) = defs.get(&arg.value) else {
            return invalid(format!("call argument value {:?}", arg.value));
        };
        if arg.label.as_deref() == Some("") {
            return invalid("empty call label".to_string());
        }
        if let Some(coercion) = &arg.fn_coercion {
            verify_type(&coercion.ty, arg.span, function)?;
        }
        if let Some(coercion) = &arg.widen_to_union {
            if !type_ids.contains(&coercion.union) {
                return invalid(format!("union widening type {:?}", coercion.union));
            }
            if coercion.variant.is_empty() {
                return invalid("union widening has an empty variant".to_string());
            }
        }
        if let Some(type_id) = arg.box_as_trait {
            if !type_ids.contains(&type_id) {
                return invalid(format!("trait boxing type {type_id:?}"));
            }
        }
        let coercion_consumes = arg.fn_coercion.is_some() || arg.widen_to_union.is_some() || arg.box_as_trait.is_some();
        if coercion_consumes
            && ownership.mode != MirOwnershipMode::Copy
            && arg.access != MirAccess::Move
            && !arg.implicit_clone
            && !arg.shared_auto_clone
            && !arg.owned_last_use
        {
            return invalid(format!("coercion consumes non-Copy argument {:?}", arg.value));
        }
    }
    Ok(())
}
fn verify_call_fallibility(
    fallibility: &MirCallFallibility,
    function: MirFunctionId,
    span: Span,
) -> Result<(), MirLegalityError> {
    if let MirCallFallibility::Failure(carrier) = fallibility {
        verify_failure_carrier_at(carrier, function, span)?;
    }
    Ok(())
}

fn route_matches(
    route: &MirPreludeCall,
    family: crate::MIR::MirPreludeFamily,
    module: &str,
    member: &str,
    symbol: &str,
    arity: usize,
    borrow_mask: &[bool],
    abi: crate::MIR::MirPreludeAbi,
) -> bool {
    route.family == family
        && route.module == module
        && route.member == member
        && route.symbol.name() == symbol
        && route.signature.arity == arity
        && route.signature.max_arity == arity
        && route.signature.borrow_mask == borrow_mask
        && route.abi == abi
}

fn typed_text_interp_route_shape(
    kind: crate::Syntax::TypedHeadKind,
) -> (&'static str, &'static str, usize, &'static [bool]) {
    match kind {
        crate::Syntax::TypedHeadKind::SQL => ("sql_interpolate", "jet_typed_sql_interpolate", 2, &[true, false]),
        crate::Syntax::TypedHeadKind::HTML => ("html_interpolate", "jet_typed_html_interpolate", 3, &[true, false, true]),
        crate::Syntax::TypedHeadKind::Sh => ("sh_interpolate", "jet_typed_sh_interpolate", 2, &[true, false]),
        crate::Syntax::TypedHeadKind::URL => ("url_literal", "jet_std::jet_typed_url_literal", 2, &[true, false]),
        crate::Syntax::TypedHeadKind::Path => ("path_literal", "jet_typed_path_literal", 2, &[true, false]),
        crate::Syntax::TypedHeadKind::DateTime => ("datetime_literal", "jet_typed_datetime_literal", 2, &[true, false]),
    }
}

fn gc_edit_route_shape(
    kind: crate::MIR::MirGcEditKind,
) -> (&'static str, &'static str, usize, &'static [bool]) {
    match kind {
        crate::MIR::MirGcEditKind::Clear => ("edit_clear", "jet_gc_edit_clear", 2, &[true, false]),
        crate::MIR::MirGcEditKind::Pop => ("edit_pop", "jet_gc_edit_pop", 2, &[true, false]),
        crate::MIR::MirGcEditKind::RemoveIndex => ("edit_remove_index", "jet_gc_edit_remove_index", 3, &[true, false, false]),
        crate::MIR::MirGcEditKind::InsertIndex => ("edit_insert_index", "jet_gc_edit_insert_index", 4, &[true, false, true, false]),
        crate::MIR::MirGcEditKind::Prepend => ("edit_prepend", "jet_gc_edit_prepend", 3, &[true, true, false]),
        crate::MIR::MirGcEditKind::Additive => ("edit_additive", "jet_gc_edit_additive", 3, &[true, true, false]),
        crate::MIR::MirGcEditKind::Plain => ("edit_plain", "jet_gc_edit_plain", 2, &[true, false]),
        crate::MIR::MirGcEditKind::EdgeSlot => ("edit_edge_slot", "jet_gc_edit_edge_slot", 4, &[true, true, false, false]),
    }
}

fn verify_hardware_call(
    call: MirPreludeCallId,
    op: &MirHardwareOp,
    receiver: Option<MirValueId>,
    args: &[crate::MIR::MirCallArg],
    span: Span,
    function: &MirFunction,
    type_ids: &HashSet<MirTypeId>,
    prelude_calls: &HashMap<MirPreludeCallId, &MirPreludeCall>,
    defs: &HashMap<MirValueId, (MirBlockId, usize, MirType, MirOwnership)>,
) -> Result<(), MirLegalityError> {
    let invalid = |subject: String| Err(MirLegalityError::InvalidReference { function: function.id, subject, span });
    if let MirHardwareOp::DmaStart { buffer_ty, .. } | MirHardwareOp::DmaWait { buffer_ty, .. } = op {
        verify_type(buffer_ty, span, function.id)?;
    }
    let Some(route) = prelude_calls.get(&call) else {
        return invalid(format!("Prelude call {call:?}"));
    };
    let expected_receiver = matches!(op, MirHardwareOp::DmaWait { .. });
    if receiver.is_some() != expected_receiver {
        return invalid(format!("hardware operation {op:?} has an invalid receiver"));
    }
    if let Some(receiver) = receiver {
        if !defs.contains_key(&receiver) {
            return invalid(format!("hardware receiver {receiver:?}"));
        }
    }
    let expected_access = match op {
        MirHardwareOp::RegisterRead { .. } | MirHardwareOp::DmaWait { .. } => None,
        MirHardwareOp::RegisterWrite { .. } => Some(MirAccess::Read),
        MirHardwareOp::DmaStart { .. } => Some(MirAccess::Move),
    };
    if let Some(expected_access) = expected_access {
        let Some(arg) = args.first() else {
            return invalid(format!("hardware operation {op:?} is missing its value argument"));
        };
        if args.len() != 1 || arg.access != expected_access || arg.place.is_some() {
            return invalid(format!("hardware operation {op:?} has an invalid value argument"));
        }
    } else if !args.is_empty() {
        return invalid(format!("hardware operation {op:?} has unexpected arguments"));
    }
    match op {
        MirHardwareOp::DmaStart { buffer_ty, .. } => {
            let Some(arg) = args.first() else {
                return invalid("DMA start has no buffer argument".to_string());
            };
            let Some((_, _, arg_ty, _)) = defs.get(&arg.value) else {
                return invalid(format!("DMA buffer argument {:?} has no definition", arg.value));
            };
            if !arg_ty.same_checked_type(buffer_ty) {
                return invalid("DMA start buffer type disagrees with its moved argument".to_string());
            }
        }
        MirHardwareOp::DmaWait { buffer_ty, .. } => {
            let Some(receiver) = receiver else {
                return invalid("DMA wait has no transfer receiver".to_string());
            };
            let Some((_, _, receiver_ty, _)) = defs.get(&receiver) else {
                return invalid(format!("DMA transfer receiver {:?} has no definition", receiver));
            };
            let valid = matches!(
                &receiver_ty.kind,
                MirTypeKind::Apply { name, args }
                    if name.name == "__JetDmaTransfer"
                        && args.len() == 1
                        && args[0].same_checked_type(buffer_ty)
            );
            if !valid {
                return invalid("DMA wait receiver is not the checked typed transfer".to_string());
            }
        }
        _ => {}
    }
    verify_call_args(args, type_ids, defs, function.id, &invalid)?;
    let expected_mask = vec![false; args.len()];
    if route.signature.arity != args.len()
        || route.signature.max_arity != args.len()
        || route.signature.borrow_mask != expected_mask
        || route.abi != crate::MIR::MirPreludeAbi::Value
    {
        return invalid(format!("hardware route {call:?} has invalid argument metadata"));
    }
    let check_id = |kind: &str, value: &str| {
        if value.is_empty() {
            invalid(format!("hardware {kind} is empty"))
        } else {
            Ok(())
        }
    };
    match op {
        MirHardwareOp::RegisterRead {
            profile_id,
            block,
            register,
            ..
        }
        | MirHardwareOp::RegisterWrite {
            profile_id,
            block,
            register,
            ..
        } => {
            check_id("profile ID", profile_id)?;
            check_id("register block", block)?;
            check_id("register", register)?;
        }
        MirHardwareOp::DmaStart {
            profile_id,
            channel,
            ..
        }
        | MirHardwareOp::DmaWait {
            profile_id,
            channel,
            ..
        } => {
            check_id("profile ID", profile_id)?;
            check_id("DMA channel", channel)?;
        }
    }
    Ok(())
}

enum HostBorrowCallbackConsumer<'a> {
    CallArgs(&'a [crate::MIR::MirCallArg]),
    Values(&'a [MirValueId]),
}

fn host_borrow_callback_consumer<'a>(
    operation: &'a MirOperation,
    prelude_calls: &HashMap<MirPreludeCallId, &MirPreludeCall>,
) -> Option<HostBorrowCallbackConsumer<'a>> {
    match operation {
        MirOperation::Semantic(MirSemanticOp::HandleMethod { call, args, .. }) => {
            let route = prelude_calls.get(call)?;
            (route.family == crate::MIR::MirPreludeFamily::HandleMethod)
                .then_some(HostBorrowCallbackConsumer::Values(args.as_slice()))
        }
        MirOperation::Semantic(MirSemanticOp::PluginInvoke { call, args, .. }) => {
            let route = prelude_calls.get(call)?;
            (route.family == crate::MIR::MirPreludeFamily::HandleMethod)
                .then_some(HostBorrowCallbackConsumer::Values(args.as_slice()))
        }
        MirOperation::Semantic(MirSemanticOp::BuiltinMethod { call, args, .. }) => {
            let route = prelude_calls.get(call)?;
            (route.family == crate::MIR::MirPreludeFamily::BuiltinMethod)
                .then_some(HostBorrowCallbackConsumer::Values(args.as_slice()))
        }
        _ => {
            let (call, args, expected_family) = match operation {
                MirOperation::Call {
                    callee: MirCallee::Prelude(call),
                    args,
                    ..
                } => (*call, args.as_slice(), None),
                MirOperation::CoreCall { route, args, .. } => (
                    *route,
                    args.as_slice(),
                    Some(crate::MIR::MirPreludeFamily::StaticPrelude),
                ),
                MirOperation::Semantic(MirSemanticOp::StaticPreludeCall { call, args, .. }) => (
                    *call,
                    args.as_slice(),
                    Some(crate::MIR::MirPreludeFamily::StaticPrelude),
                ),
                MirOperation::Semantic(MirSemanticOp::ClosureMethod { call, args, .. }) => (
                    *call,
                    args.as_slice(),
                    Some(crate::MIR::MirPreludeFamily::ClosureMethod),
                ),
                MirOperation::Semantic(MirSemanticOp::HostCall { call, args }) => (
                    *call,
                    args.as_slice(),
                    Some(crate::MIR::MirPreludeFamily::Host),
                ),
                _ => return None,
            };
            let route = prelude_calls.get(&call)?;
            if expected_family.is_some_and(|family| route.family != family) {
                return None;
            }
            Some(HostBorrowCallbackConsumer::CallArgs(args))
        }
    }
}

fn verify_host_borrow_callback_uses(
    function: &MirFunction,
    prelude_calls: &HashMap<MirPreludeCallId, &MirPreludeCall>,
) -> Result<(), MirLegalityError> {
    let invalid = |subject: String, span: Span| {
        Err(MirLegalityError::InvalidReference {
            function: function.id,
            subject,
            span,
        })
    };
    let mut producers = Vec::new();
    for block in &function.blocks {
        for (index, instruction) in block.instructions.iter().enumerate() {
            if !matches!(
                &instruction.operation,
                MirOperation::Semantic(MirSemanticOp::HostBorrowCallback { .. })
            ) {
                continue;
            }
            let Some(result) = instruction.result else {
                return invalid(
                    "HostBorrowCallback has no representation result".to_string(),
                    instruction.span,
                );
            };
            let Some(ty) = instruction.ty.as_ref() else {
                return invalid(
                    "HostBorrowCallback has no representation type".to_string(),
                    instruction.span,
                );
            };
            if !ty.is_function() {
                return invalid(
                    "HostBorrowCallback result is not a checked callable".to_string(),
                    instruction.span,
                );
            }
            producers.push((result, block.id, index, instruction.span));
        }
    }

    for (result, producer_block, producer_index, producer_span) in producers {
        let mut consumed = false;
        for block in &function.blocks {
            for (index, instruction) in block.instructions.iter().enumerate() {
                let value_uses = instruction
                    .operation
                    .value_uses()
                    .into_iter()
                    .filter(|value| *value == result)
                    .count();
                let place_use = operation_place_refs(&instruction.operation)
                    .into_iter()
                    .any(|place_id| {
                        function
                            .places
                            .iter()
                            .find(|place| place.id == place_id)
                            .is_some_and(|place| {
                                place_value_uses(place)
                                    .into_iter()
                                    .any(|value| value == result)
                            })
                    });
                if value_uses == 0 && !place_use {
                    continue;
                }
                if block.id != producer_block || index != producer_index + 1 {
                    return invalid(
                        "HostBorrowCallback result must be consumed by the next host/Prelude call"
                            .to_string(),
                        instruction.span,
                    );
                }
                let Some(consumer) =
                    host_borrow_callback_consumer(&instruction.operation, prelude_calls)
                else {
                    return invalid(
                        "HostBorrowCallback result escaped its direct host/Prelude call argument"
                            .to_string(),
                        instruction.span,
                    );
                };
                let consumer_uses = match consumer {
                    HostBorrowCallbackConsumer::CallArgs(args) => args
                        .iter()
                        .filter(|arg| arg.value == result)
                        .count(),
                    HostBorrowCallbackConsumer::Values(values) => {
                        values.iter().filter(|value| **value == result).count()
                    }
                };
                if value_uses != 1 || consumer_uses != 1 {
                    return invalid(
                        "HostBorrowCallback result must appear exactly once as a call argument"
                            .to_string(),
                        instruction.span,
                    );
                }
                consumed = true;
            }
            if block
                .terminator
                .value_uses()
                .into_iter()
                .any(|value| value == result)
            {
                return invalid(
                    "HostBorrowCallback result escaped through a block terminator".to_string(),
                    block.span,
                );
            }
        }
        if !consumed {
            return invalid(
                "HostBorrowCallback result has no direct host/Prelude call consumer".to_string(),
                producer_span,
            );
        }
    }
    Ok(())
}

fn verify_semantic_operation(
    operation: &MirSemanticOp,
    span: Span,
    function: &MirFunction,
    type_ids: &HashSet<MirTypeId>,
    field_ids: &HashSet<MirFieldId>,
    source_file_ids: &HashSet<MirSourceFileId>,
    prelude_calls: &HashMap<MirPreludeCallId, &MirPreludeCall>,
    callback_ids: &HashSet<crate::MIR::MirCallbackId>,
    defs: &HashMap<MirValueId, (MirBlockId, usize, MirType, MirOwnership)>,
) -> Result<(), MirLegalityError> {
    let invalid = |subject: String| Err(MirLegalityError::InvalidReference { function: function.id, subject, span });
    for call in operation.prelude_calls() {
        if !prelude_calls.contains_key(&call) {
            return invalid(format!("Prelude call {call:?}"));
        }
    }
    for field in operation.field_uses() {
        if !field_ids.contains(&field) {
            return invalid(format!("field {field:?}"));
        }
    }
    for source_file in operation.source_file_uses() {
        if !source_file_ids.contains(&source_file) {
            return invalid(format!("source file {source_file:?}"));
        }
    }
    let check_type = |type_id: MirTypeId| {
        if type_ids.contains(&type_id) {
            Ok(())
        } else {
            invalid(format!("type {type_id:?}"))
        }
    };
    for type_id in operation.type_uses() {
        check_type(type_id)?;
    }
    let check_value = |value: MirValueId| {
        if defs.contains_key(&value) {
            Ok(())
        } else {
            invalid(format!("value {value:?}"))
        }
    };
    let check_guard_route = |call: MirPreludeCallId,
                             module: &str,
                             arity: usize,
                             borrow_mask: &[bool],
                             abi: crate::MIR::MirPreludeAbi,
                             member: &str,
                             symbol: &str| {
        let Some(route) = prelude_calls.get(&call) else {
            return invalid(format!("Prelude call {call:?}"));
        };
        if !route_matches(
            route,
            crate::MIR::MirPreludeFamily::StaticPrelude,
            module,
            member,
            symbol,
            arity,
            borrow_mask,
            abi,
        ) {
            return invalid(format!("guard route {call:?} does not match {symbol}/{member}/{arity}"));
        }
        Ok(())
    };
    match operation {
        MirSemanticOp::DataEntriesToMap { local, .. } => {
            if !function.locals.iter().any(|candidate| candidate.id == *local) {
                return invalid(format!("local {local:?}"));
            }
        }
        MirSemanticOp::HardwareCall {
            call,
            op,
            receiver,
            args,
        } => {
            verify_hardware_call(
                *call,
                op,
                *receiver,
                args,
                span,
                function,
                type_ids,
                prelude_calls,
                defs,
            )?;
        }
        MirSemanticOp::BuiltinMethod { receiver_place, .. } => {
            if let Some(place) = receiver_place {
                if !function.places.iter().any(|candidate| candidate.id == *place) {
                    return invalid(format!("receiver place {place:?}"));
                }
            }
        }
        MirSemanticOp::MathBuiltin { type_id, .. } | MirSemanticOp::PreciseBuiltin { type_id, .. } => {
            check_type(*type_id)?;
        }
        MirSemanticOp::StructLiteral {
            type_id,
            trait_coercion,
            ..
        } => {
            check_type(*type_id)?;
            if let Some(trait_coercion) = trait_coercion {
                check_type(*trait_coercion)?;
            }
        }
        MirSemanticOp::ColumnarRead {
            accessor,
            column_index,
            ..
        } => {
            let Some(route) = prelude_calls.get(accessor) else {
                return invalid(format!("Prelude call {accessor:?}"));
            };
            let suffix = format!("#{column_index}");
            if route.family != crate::MIR::MirPreludeFamily::ColumnarAccess
                || route.module != "core.columnar"
                || route.symbol.name() != "jet_columns_gather_cell"
                || route.signature.arity != 3
                || route.signature.max_arity != 3
                || route.signature.borrow_mask != [true, false, false]
                || route.abi != crate::MIR::MirPreludeAbi::Value
                || !route.member.ends_with(&suffix)
            {
                return invalid(format!("columnar read route {accessor:?} has the wrong metadata"));
            }
        }
        MirSemanticOp::CellGuardProject {
            map_call,
            split_call,
            paths,
            editable,
            edit_paths_disjoint,
            ..
        } => {
            let map_member = if *editable { "map_edit" } else { "map_read" };
            check_guard_route(
                *map_call,
                "core.cell_guard",
                2,
                &[true, false],
                crate::MIR::MirPreludeAbi::Value,
                map_member,
                "jet_cell_guard_map",
            )?;
            if !(1..=2).contains(&paths.len()) {
                return invalid("Cell guard projection must carry one or two paths".to_string());
            }
            if paths.iter().any(|path| path.is_empty()) {
                return invalid("Cell guard projection has an empty path".to_string());
            }
            let split = paths.len() == 2;
            if split_call.is_some() != split {
                return invalid("Cell guard projection path arity disagrees with its split row".to_string());
            }
            if let Some(split_call) = split_call {
                if *split_call == *map_call {
                    return invalid("Cell guard projection reuses its map row as its split row".to_string());
                }
                check_guard_route(
                    *split_call,
                    "core.cell_guard",
                    3,
                    &[true, false, false],
                    crate::MIR::MirPreludeAbi::Value,
                    if *editable { "split_edit" } else { "split_read" },
                    "jet_cell_guard_split",
                )?;
                if !guard_paths_are_distinct(&paths[0], &paths[1]) {
                    return invalid("Cell guard split paths must be distinct and non-prefix".to_string());
                }
            }
            if *edit_paths_disjoint && (!*editable || !split) {
                return invalid("Cell guard edit disjointness requires an editable split".to_string());
            }
            if *editable && split && !*edit_paths_disjoint {
                return invalid("editable Cell guard split is missing disjoint paths proof".to_string());
            }
        }
        MirSemanticOp::SharedGuardMap {
            call,
            editable,
            path,
            ..
        } => {
            if path.is_empty() {
                return invalid("shared guard map has an empty path".to_string());
            }
            check_guard_route(
                *call,
                "core.shared_guard",
                3,
                &[true, false, false],
                crate::MIR::MirPreludeAbi::Control,
                if *editable { "map_edit" } else { "map_read" },
                "jet_shared_guard_map",
            )?;
        }
        MirSemanticOp::SharedGuardSplit {
            call,
            map_call,
            first,
            second,
            editable,
            ..
        } => {
            if first.is_empty() || second.is_empty() {
                return invalid("shared guard split has an empty path".to_string());
            }
            if !guard_paths_are_distinct(first, second) {
                return invalid("shared guard split paths must be distinct and non-prefix".to_string());
            }
            if *call == *map_call {
                return invalid("shared guard split reuses its split row as its map row".to_string());
            }
            check_guard_route(
                *call,
                "core.shared_guard",
                4,
                &[true, false, false, false],
                crate::MIR::MirPreludeAbi::Control,
                if *editable { "split_edit" } else { "split_read" },
                "jet_shared_guard_split",
            )?;
            check_guard_route(
                *map_call,
                "core.shared_guard",
                3,
                &[true, false, false],
                crate::MIR::MirPreludeAbi::Control,
                if *editable { "map_edit" } else { "map_read" },
                "jet_shared_guard_map",
            )?;
        }
        MirSemanticOp::TextPatternMatch { call, parts, .. } => {
            let Some(route) = prelude_calls.get(call) else {
                return invalid(format!("Prelude call {call:?}"));
            };
            if !route_matches(
                route,
                crate::MIR::MirPreludeFamily::StaticPrelude,
                "core.pattern",
                "text_match",
                "jet_text_pattern_match",
                1,
                &[true],
                crate::MIR::MirPreludeAbi::Value,
            ) {
                return invalid(format!("text pattern route {call:?} has the wrong metadata"));
            }
            verify_text_pattern_parts(parts, type_ids, function.id)?;
        }
        MirSemanticOp::BinaryPatternMatch { call, parts, .. } => {
            let Some(route) = prelude_calls.get(call) else {
                return invalid(format!("Prelude call {call:?}"));
            };
            if !route_matches(
                route,
                crate::MIR::MirPreludeFamily::StaticPrelude,
                "core.pattern",
                "binary_match",
                "jet_binary_pattern_match",
                1,
                &[true],
                crate::MIR::MirPreludeAbi::Value,
            ) {
                return invalid(format!("binary pattern route {call:?} has the wrong metadata"));
            }
            verify_binary_pattern_parts(parts, type_ids, function.id)?;
        }
        MirSemanticOp::RequireStop {
            kind,
            condition,
            context,
            ..
        } => {
            let requires_condition = matches!(
                kind,
                crate::MIR::MirRequireKind::Require | crate::MIR::MirRequireKind::RequireEq
            );
            if condition.is_some() != requires_condition {
                return invalid(format!(
                    "require-stop {:?} condition presence is inconsistent",
                    kind
                ));
            }
            if let Some(condition) = condition {
                let Some((_, _, ty, _)) = defs.get(condition) else {
                    return invalid(format!("require-stop condition value {condition:?}"));
                };
                if !ty.is_bool() {
                    return invalid(format!(
                        "require-stop condition {condition:?} is not Bool"
                    ));
                }
            }
            if context.function.is_empty() {
                return invalid("require-stop context has no function".to_string());
            }
            let mut seen_locals = HashSet::new();
            for (name, local) in &context.locals {
                if name.is_empty() {
                    return invalid("require-stop context has an unnamed local".to_string());
                }
                if !seen_locals.insert(*local) {
                    return invalid(format!("duplicate require-stop local {local:?}"));
                }
                if !function.locals.iter().any(|candidate| candidate.id == *local) {
                    return invalid(format!("require-stop local {local:?}"));
                }
            }
        }
        MirSemanticOp::CarrierFact { receiver, .. } => {
            check_value(*receiver)?;
            let Some((_, _, receiver_ty, _)) = defs.get(receiver) else {
                return invalid(format!("carrier fact receiver {receiver:?}"));
            };
            if !receiver_ty.is_result() {
                return invalid(format!(
                    "carrier fact receiver {receiver:?} is not a Result"
                ));
            }
        }
        MirSemanticOp::GcEdit {
            call,
            root,
            edges,
            edit,
            index,
            kind,
            site,
            ..
        } => {
            check_value(*root)?;
            for edge in edges {
                check_value(*edge)?;
            }
            check_value(*edit)?;
            if let Some(index) = index {
                check_value(*index)?;
            }
            if site.0 == 0 {
                return invalid("gc edit has a zero typed site ID".to_string());
            }
            let (member, symbol, arity, borrow_mask) = gc_edit_route_shape(*kind);
            let Some(route) = prelude_calls.get(call) else {
                return invalid(format!("Prelude call {call:?}"));
            };
            if !route_matches(
                route,
                crate::MIR::MirPreludeFamily::StaticPrelude,
                "core.gc",
                member,
                symbol,
                arity,
                borrow_mask,
                crate::MIR::MirPreludeAbi::Value,
            ) {
                return invalid(format!("gc edit route {call:?} does not match {symbol}/{member}/{arity}"));
            }
            match kind {
                crate::MIR::MirGcEditKind::Clear
                | crate::MIR::MirGcEditKind::Pop
                | crate::MIR::MirGcEditKind::Plain => {
                    if index.is_some() || !edges.is_empty() {
                        return invalid(format!("gc edit {:?} carries unused operands", kind));
                    }
                }
                crate::MIR::MirGcEditKind::RemoveIndex => {
                    if index.is_none() || !edges.is_empty() {
                        return invalid("gc remove edit has the wrong operands".to_string());
                    }
                }
                crate::MIR::MirGcEditKind::InsertIndex => {
                    if index.is_none() {
                        return invalid("gc insert edit has no index".to_string());
                    }
                }
                crate::MIR::MirGcEditKind::Prepend | crate::MIR::MirGcEditKind::Additive => {
                    if index.is_some() {
                        return invalid(format!("gc edge edit {:?} carries an index", kind));
                    }
                }
                crate::MIR::MirGcEditKind::EdgeSlot => {
                    if index.is_some() || edges.is_empty() {
                        return invalid("gc edge-slot edit has the wrong operands".to_string());
                    }
                }
            }
        }
        MirSemanticOp::TypedTextInterp {
            call,
            kind,
            literals,
            holes,
        } => {
            let (member, symbol, arity, borrow_mask) = typed_text_interp_route_shape(*kind);
            let Some(route) = prelude_calls.get(call) else {
                return invalid(format!("Prelude call {call:?}"));
            };
            if !route_matches(
                route,
                crate::MIR::MirPreludeFamily::StaticPrelude,
                "core.typed_text",
                member,
                symbol,
                arity,
                borrow_mask,
                crate::MIR::MirPreludeAbi::Value,
            ) {
                return invalid(format!("typed-text route {call:?} does not match {symbol}/{member}/{arity}"));
            }
            for hole in holes {
                check_value(*hole)?;
            }
            if literals.len() != holes.len().saturating_add(1) {
                return invalid(
                    "typed-text interpolation must have one literal edge per hole".to_string(),
                );
            }
        }
        MirSemanticOp::CCallback {
            call,
            callback,
            lambda,
        } => {
            if !callback_ids.contains(callback) {
                return invalid(format!("callback {callback:?}"));
            }
            let Some(route) = prelude_calls.get(call) else {
                return invalid(format!("Prelude call {call:?}"));
            };
            if !route_matches(
                route,
                crate::MIR::MirPreludeFamily::StaticPrelude,
                "core.ffi",
                "callback_boundary",
                "jet_ffi_callback_boundary",
                1,
                &[false],
                crate::MIR::MirPreludeAbi::Value,
            ) {
                return invalid(format!("callback route {call:?} has the wrong symbol, ABI, or arity"));
            }
            check_value(*lambda)?;
            let Some((_, _, lambda_ty, _)) = defs.get(lambda) else {
                return invalid(format!("C callback lambda {lambda:?}"));
            };
            if lambda_ty.function_signature().is_none() {
                return invalid(format!("C callback lambda {lambda:?} is not a function"));
            }
        }
        MirSemanticOp::HttpRouterRegister {
            call,
            receiver,
            path,
            handler,
            handler_param_names,
            ..
        } => {
            check_value(*receiver)?;
            check_value(*path)?;
            check_value(*handler)?;
            let Some(route) = prelude_calls.get(call) else {
                return invalid(format!("Prelude call {call:?}"));
            };
            let (member, symbol, arity, borrow_mask) = match defs
                .get(receiver)
                .and_then(|(_, _, ty, _)| ty.nominal_name())
            {
                Some("HTTPMux") if handler_param_names.is_empty() => (
                    "mux_add_zero",
                    "jet_http_mux_add_zero_handler",
                    4,
                    &[true, false, false, false][..],
                ),
                Some("HTTPMux") => (
                    "mux_add",
                    "jet_http_mux_add_handler",
                    4,
                    &[true, false, false, false][..],
                ),
                Some("HTTPRouter") => (
                    "router_register",
                    "jet_http_router_register",
                    7,
                    &[true, false, false, false, true, false, false][..],
                ),
                _ => return invalid(format!("HTTP registration receiver {receiver:?} has the wrong type")),
            };
            if !route_matches(
                route,
                crate::MIR::MirPreludeFamily::StaticPrelude,
                "core.http",
                member,
                symbol,
                arity,
                borrow_mask,
                crate::MIR::MirPreludeAbi::Control,
            ) {
                return invalid(format!(
                    "HTTP route registration route {call:?} has the wrong metadata"
                ));
            }
        }
        MirSemanticOp::PluginInvoke {
            call,
            handle,
            args,
            ..
        } => {
            check_value(*handle)?;
            for arg in args {
                check_value(*arg)?;
            }
            let Some(route) = prelude_calls.get(call) else {
                return invalid(format!("Prelude call {call:?}"));
            };
            if !route_matches(
                route,
                crate::MIR::MirPreludeFamily::HandleMethod,
                "core.handle",
                "plugin.invoke",
                "jet_plugin_call",
                3,
                &[true, true, true],
                crate::MIR::MirPreludeAbi::Value,
            ) {
                return invalid(format!(
                    "plugin invocation route {call:?} has the wrong metadata"
                ));
            }
        }
        MirSemanticOp::StaticPreludeCall {
            args,
            owner_type_args,
            type_args,
            ..
        } => {
            verify_call_args(args, type_ids, defs, function.id, &invalid)?;
            for type_arg in owner_type_args {
                if let crate::MIR::MirPreludeTypeArg::Type(ty) = type_arg {
                    verify_type(ty, span, function.id)?;
                }
            }
            for ty in type_args {
                verify_type(ty, span, function.id)?;
            }
        }
        MirSemanticOp::ClosureMethod { args, .. } | MirSemanticOp::HostCall { args, .. } => {
            verify_call_args(args, type_ids, defs, function.id, &invalid)?;
        }
        MirSemanticOp::HostBorrowCallback { params, .. } => {
            for ty in params {
                verify_type(ty, span, function.id)?;
            }
        }
        MirSemanticOp::CoreClosureCall { site, .. } if site.0 == 0 => {
            return invalid("zero Core closure site".to_string());
        }
        _ => {}
    }
    Ok(())
}



fn verify_text_pattern_parts(
    parts: &[crate::MIR::MirTextPatternPart],
    type_ids: &HashSet<MirTypeId>,
    function: MirFunctionId,
) -> Result<(), MirLegalityError> {
    for part in parts {
        if let crate::MIR::MirTextPatternPart::Hole { kind, ty, span } = part {
            if !type_ids.contains(ty) {
                return Err(MirLegalityError::InvalidReference {
                    function,
                    subject: format!("text pattern type {ty:?}"),
                    span: *span,
                });
            }
            if span.start > span.end {
                return Err(MirLegalityError::InvalidSpan { function: Some(function), span: *span });
            }
            if let crate::MIR::MirTextHoleKind::InlineRange { lo, hi } = kind {
                if lo > hi {
                    return Err(MirLegalityError::InvalidReference {
                        function,
                        subject: "invalid text pattern range".to_string(),
                        span: *span,
                    });
                }
            }
        }
    }
    Ok(())
}

fn verify_binary_pattern_parts(
    parts: &[crate::MIR::MirBinaryPatternPart],
    type_ids: &HashSet<MirTypeId>,
    function: MirFunctionId,
) -> Result<(), MirLegalityError> {
    for part in parts {
        match part {
            crate::MIR::MirBinaryPatternPart::Literal(_) => {}
            crate::MIR::MirBinaryPatternPart::Bits { width, ty, span, .. } => {
                if !type_ids.contains(ty) {
                    return Err(MirLegalityError::InvalidReference {
                        function,
                        subject: format!("binary pattern type {ty:?}"),
                        span: *span,
                    });
                }
                if span.start > span.end {
                    return Err(MirLegalityError::InvalidSpan { function: Some(function), span: *span });
                }
                if *width == 0 {
                    return Err(MirLegalityError::InvalidReference {
                        function,
                        subject: "zero-width binary pattern".to_string(),
                        span: *span,
                    });
                }
            }
            crate::MIR::MirBinaryPatternPart::Rest { ty, span } => {
                if !type_ids.contains(ty) {
                    return Err(MirLegalityError::InvalidReference {
                        function,
                        subject: format!("binary pattern rest type {ty:?}"),
                        span: *span,
                    });
                }
                if span.start > span.end {
                    return Err(MirLegalityError::InvalidSpan { function: Some(function), span: *span });
                }
            }
        }
    }
    Ok(())
}



fn verify_call_callee(
    callee: &MirCallee,
    block_ids: &HashSet<MirBlockId>,
    function_ids: &HashSet<MirFunctionId>,
    foreign_ids: &HashSet<crate::MIR::MirForeignId>,
    core_ids: &HashSet<crate::MIR::MirCoreCallId>,
    prelude_calls: &HashMap<MirPreludeCallId, &MirPreludeCall>,
    function: &MirFunction,
    span: Span,
) -> Result<(), MirLegalityError> {
    match callee {
        MirCallee::Associated { function: target, .. }
        | MirCallee::Method { function: target, .. }
            if !function_ids.contains(target) =>
        {
            Err(MirLegalityError::InvalidReference {
                function: function.id,
                subject: format!("associated callee {target:?}"),
                span,
            })
        }
        MirCallee::User(target) if !function_ids.contains(target) => Err(MirLegalityError::InvalidReference {
            function: function.id,
            subject: format!("callee {target:?}"),
            span,
        }),
        MirCallee::Foreign(target) if !foreign_ids.contains(target) => Err(MirLegalityError::InvalidReference {
            function: function.id,
            subject: format!("foreign callee {target:?}"),
            span,
        }),
        MirCallee::Core(target) if !core_ids.contains(target) => Err(MirLegalityError::InvalidReference {
            function: function.id,
            subject: format!("Core callee {target:?}"),
            span,
        }),
        MirCallee::Prelude(target) if !prelude_calls.contains_key(target) => Err(MirLegalityError::InvalidReference {
            function: function.id,
            subject: format!("Prelude callee {target:?}"),
            span,
        }),
        MirCallee::Indirect(_) => {
            let _ = block_ids;
            Ok(())
        }
        _ => Ok(()),
    }
}


fn ensure_value_dominates(
    function: &MirFunction,
    defs: &HashMap<MirValueId, (MirBlockId, usize, MirType, MirOwnership)>,
    dominators: &HashMap<MirBlockId, BTreeSet<MirBlockId>>,
    use_block: MirBlockId,
    use_index: usize,
    value: MirValueId,
    span: Span,
) -> Result<(), MirLegalityError> {
    let Some((definition_block, definition_index, _, _)) = defs.get(&value) else {
        return Err(MirLegalityError::InvalidReference { function: function.id, subject: format!("value {value:?}"), span });
    };
    let valid = if *definition_block == use_block {
        *definition_index < use_index
    } else {
        dominators.get(&use_block).is_some_and(|set| set.contains(definition_block))
    };
    if valid {
        Ok(())
    } else {
        Err(MirLegalityError::InvalidReference { function: function.id, subject: format!("non-dominating value {value:?}"), span })
    }
}

fn reachable_blocks(function: &MirFunction) -> BTreeSet<MirBlockId> {
    let ids: HashSet<_> = function.blocks.iter().map(|block| block.id).collect();
    let mut seen = BTreeSet::new();
    let mut work = vec![function.entry];
    for drop in &function.drops {
        if let MirDropEdge::Failure(target) | MirDropEdge::Unwind(target) = drop.edge {
            work.push(target);
        }
    }
    while let Some(block) = work.pop() {
        if !ids.contains(&block) || !seen.insert(block) {
            continue;
        }
        if let Some(block) = function.blocks.iter().find(|candidate| candidate.id == block) {
            work.extend(block.terminator.targets());
        }
    }
    seen
}

fn predecessor_map(function: &MirFunction) -> HashMap<MirBlockId, BTreeSet<MirBlockId>> {
    let mut out: HashMap<MirBlockId, BTreeSet<MirBlockId>> = function
        .blocks
        .iter()
        .map(|block| (block.id, BTreeSet::new()))
        .collect();
    for block in &function.blocks {
        for target in block.terminator.targets() {
            out.entry(target).or_default().insert(block.id);
        }
    }
    out
}

fn dominator_map(
    function: &MirFunction,
    reachable: &BTreeSet<MirBlockId>,
    predecessors: &HashMap<MirBlockId, BTreeSet<MirBlockId>>,
) -> HashMap<MirBlockId, BTreeSet<MirBlockId>> {
    let roots: BTreeSet<_> = std::iter::once(function.entry)
        .chain(function.drops.iter().filter_map(|drop| match drop.edge {
            MirDropEdge::Failure(target) | MirDropEdge::Unwind(target) => Some(target),
            MirDropEdge::Normal | MirDropEdge::Return => None,
        }))
        .filter(|root| reachable.contains(root))
        .collect();
    let mut result: HashMap<_, _> = reachable
        .iter()
        .map(|block| (*block, reachable.clone()))
        .collect();
    for root in &roots {
        result.insert(*root, [*root].into_iter().collect());
    }
    let mut changed = true;
    while changed {
        changed = false;
        for block in reachable {
            if roots.contains(block) {
                continue;
            }
            let incoming: Vec<_> = predecessors
                .get(block)
                .into_iter()
                .flat_map(|set| set.iter())
                .filter(|predecessor| reachable.contains(predecessor))
                .collect();
            if incoming.is_empty() {
                continue;
            }
            let mut next = reachable.clone();
            for predecessor in incoming {
                if let Some(dom) = result.get(predecessor) {
                    next = next.intersection(dom).copied().collect();
                }
            }
            next.insert(*block);
            if result.get(block) != Some(&next) {
                result.insert(*block, next);
                changed = true;
            }
        }
    }
    let _ = function;
    result
}

fn operation_place_refs(operation: &MirOperation) -> Vec<crate::MIR::MirPlaceId> {
    let mut places = match operation {
        MirOperation::ReadPlace(place)
        | MirOperation::MovePlace { place }
        | MirOperation::InitializeUninit { place }
        | MirOperation::RawAddressOf { place }
        | MirOperation::AddressOf { place, .. }
        | MirOperation::WritePlace { place, .. } => vec![*place],
        MirOperation::Closure { captures, .. } => captures
            .iter()
            .filter_map(|capture| match capture {
                crate::MIR::MirCaptureOperand::Place(place) => Some(*place),
                crate::MIR::MirCaptureOperand::Value(_) => None,
            })
            .collect(),
        MirOperation::Semantic(MirSemanticOp::BuiltinMethod {
            receiver_place: Some(place),
            ..
        }) => vec![*place],
        _ => Vec::new(),
    };
    match operation {
        MirOperation::Call { args, .. }
        | MirOperation::CoreCall { args, .. }
        | MirOperation::IndirectCall { args, .. } => {
            places.extend(args.iter().filter_map(|arg| arg.place));
        }
        MirOperation::Semantic(
            MirSemanticOp::StaticPreludeCall { args, .. }
            | MirSemanticOp::HardwareCall { args, .. }
            | MirSemanticOp::ClosureMethod { args, .. }
            | MirSemanticOp::HostCall { args, .. },
        ) => {
            places.extend(args.iter().filter_map(|arg| arg.place));
        }
        _ => {}
    }
    places
}

fn place_value_uses(place: &MirPlace) -> Vec<MirValueId> {
    let mut values = Vec::new();
    match &place.base {
        MirPlaceBase::Parameter(value)
        | MirPlaceBase::Capture(value)
        | MirPlaceBase::Temporary(value) => values.push(*value),
        MirPlaceBase::Local(_) | MirPlaceBase::Static(_) => {}
    }
    for projection in &place.projections {
        if let MirProjection::Index { index, .. } = projection {
            values.push(*index);
        }
    }
    values
}

fn call_arg_consumes(arg: &crate::MIR::MirCallArg) -> bool {
    if arg.implicit_clone || arg.shared_auto_clone || arg.widen_fixed_to_list {
        return false;
    }
    arg.access == MirAccess::Move || arg.owned_last_use
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct MirPlacePath {
    root: String,
    projections: Vec<(u8, u64)>,
}

fn mir_place_path(place: &MirPlace) -> MirPlacePath {
    let root = match &place.base {
        MirPlaceBase::Local(local) => format!("local:{}", local.0),
        MirPlaceBase::Parameter(value) => format!("parameter:{}", value.0),
        MirPlaceBase::Capture(value) => format!("capture:{}", value.0),
        MirPlaceBase::Temporary(value) => format!("temporary:{}", value.0),
        MirPlaceBase::Static(name) => format!("static:{name}"),
    };
    let projections = place
        .projections
        .iter()
        .map(|projection| match projection {
            MirProjection::Field { field, .. } => (0, field.0),
            MirProjection::Index { kind, index, .. } => (mir_index_kind_key(*kind), index.0),
            MirProjection::Deref { .. } => (6, 0),
        })
        .collect();
    MirPlacePath { root, projections }
}

fn mir_index_kind_key(kind: MirIndexKind) -> u8 {
    match kind {
        MirIndexKind::List => 1,
        MirIndexKind::FixedListProof => 2,
        MirIndexKind::Map => 3,
        MirIndexKind::Lane => 4,
        MirIndexKind::Pool => 5,
    }
}

fn mir_place_path_is_prefix(prefix: &MirPlacePath, path: &MirPlacePath) -> bool {
    prefix.root == path.root
        && prefix.projections.len() <= path.projections.len()
        && prefix.projections == path.projections[..prefix.projections.len()]
}

fn mir_place_paths_overlap(left: &MirPlacePath, right: &MirPlacePath) -> bool {
    mir_place_path_is_prefix(left, right) || mir_place_path_is_prefix(right, left)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MirPlaceUse {
    Read,
    Borrow,
    Move,
    Reinitialize,
}

fn call_arg_place_use(arg: &crate::MIR::MirCallArg) -> MirPlaceUse {
    if call_arg_consumes(arg) {
        MirPlaceUse::Move
    } else if arg.access == MirAccess::Write {
        MirPlaceUse::Borrow
    } else {
        MirPlaceUse::Read
    }
}

fn operation_place_uses(operation: &MirOperation) -> Vec<(crate::MIR::MirPlaceId, MirPlaceUse)> {
    let mut uses = match operation {
        MirOperation::ReadPlace(place) => vec![(*place, MirPlaceUse::Read)],
        MirOperation::MovePlace { place } => vec![(*place, MirPlaceUse::Move)],
        MirOperation::InitializeUninit { place } => vec![(*place, MirPlaceUse::Reinitialize)],
        MirOperation::RawAddressOf { place }
        | MirOperation::AddressOf { place, .. } => vec![(*place, MirPlaceUse::Borrow)],
        MirOperation::WritePlace { place, .. } => vec![(*place, MirPlaceUse::Reinitialize)],
        MirOperation::Closure { captures, .. } => captures
            .iter()
            .filter_map(|capture| match capture {
                crate::MIR::MirCaptureOperand::Place(place) => {
                    Some((*place, MirPlaceUse::Borrow))
                }
                crate::MIR::MirCaptureOperand::Value(_) => None,
            })
            .collect(),
        MirOperation::Semantic(MirSemanticOp::BuiltinMethod {
            receiver_place: Some(place),
            ..
        }) => vec![(*place, MirPlaceUse::Borrow)],
        _ => Vec::new(),
    };
    match operation {
        MirOperation::Call { args, .. }
        | MirOperation::CoreCall { args, .. }
        | MirOperation::IndirectCall { args, .. } => {
            uses.extend(
                args.iter()
                    .filter_map(|arg| arg.place.map(|place| (place, call_arg_place_use(arg)))),
            );
        }
        MirOperation::Semantic(
            MirSemanticOp::StaticPreludeCall { args, .. }
            | MirSemanticOp::HardwareCall { args, .. }
            | MirSemanticOp::ClosureMethod { args, .. }
            | MirSemanticOp::HostCall { args, .. },
        ) => {
            uses.extend(
                args.iter()
                    .filter_map(|arg| arg.place.map(|place| (place, call_arg_place_use(arg)))),
            );
        }
        _ => {}
    }
    uses
}


fn verify_moves_and_borrows(
    function: &MirFunction,
    defs: &HashMap<MirValueId, (MirBlockId, usize, MirType, MirOwnership)>,
    dominators: &HashMap<MirBlockId, BTreeSet<MirBlockId>>,
    reachable: &BTreeSet<MirBlockId>,
    place_map: &HashMap<crate::MIR::MirPlaceId, &MirPlace>,
) -> Result<(), MirLegalityError> {
    let mut consumed: Vec<(MirValueId, MirBlockId, usize, Span)> = Vec::new();
    let mut moved_places: Vec<(MirPlacePath, MirBlockId, usize, Span)> = Vec::new();
    for block in &function.blocks {
        if !reachable.contains(&block.id) {
            continue;
        }
        for (index, instruction) in block.instructions.iter().enumerate() {
            for value in instruction.operation.value_uses() {
                if let Some((_, consumed_block, consumed_index, consumed_span)) =
                    consumed.iter().find(|(candidate, _, _, _)| *candidate == value)
                {
                    let dominates = *consumed_block == block.id && *consumed_index < index
                        || *consumed_block != block.id
                            && dominators
                                .get(&block.id)
                                .is_some_and(|set| set.contains(consumed_block));
                    if dominates {
                        return Err(MirLegalityError::InvalidMove {
                            function: function.id,
                            value,
                            span: *consumed_span,
                        });
                    }
                }
            }
            for (place_id, use_kind) in operation_place_uses(&instruction.operation) {
                let Some(place) = place_map.get(&place_id).copied() else {
                    continue;
                };
                if matches!(use_kind, MirPlaceUse::Reinitialize)
                    && matches!(place.access, MirAccess::Read)
                    && place.projections.iter().any(|projection| {
                        matches!(projection, MirProjection::Deref { .. })
                    })
                {
                    return Err(MirLegalityError::InvalidPlaceAccess {
                        function: function.id,
                        place: place_id.0,
                        span: instruction.span,
                    });
                }
                if let MirOperation::AddressOf {
                    access: MirAccess::Write,
                    ..
                } = &instruction.operation
                {
                    if matches!(place.access, MirAccess::Read) {
                        return Err(MirLegalityError::InvalidBorrow {
                            function: function.id,
                            place: place_id.0,
                            span: instruction.span,
                        });
                    }
                }

                if matches!(use_kind, MirPlaceUse::Move)
                    && matches!(&place.base, MirPlaceBase::Static(_))
                    && !place.projections.is_empty()
                {
                    return Err(MirLegalityError::InvalidReference {
                        function: function.id,
                        subject: format!(
                            "static move place {place_id:?} must target the whole static root"
                        ),
                        span: instruction.span,
                    });
                }
                let path = mir_place_path(place);
                let moved_span = moved_places
                    .iter()
                    .find(|(moved_path, moved_block, moved_index, _)| {
                        let dominates = *moved_block == block.id && *moved_index < index
                            || *moved_block != block.id
                                && dominators
                                    .get(&block.id)
                                    .is_some_and(|set| set.contains(moved_block));
                        dominates && mir_place_paths_overlap(moved_path, &path)
                    })
                    .map(|(_, _, _, span)| *span);

                match use_kind {
                    MirPlaceUse::Read | MirPlaceUse::Borrow | MirPlaceUse::Move => {
                        if moved_span.is_some() {
                            return Err(MirLegalityError::InvalidMovedPlace {
                                function: function.id,
                                place: place_id.0,
                                span: instruction.span,
                            });
                        }
                        if matches!(use_kind, MirPlaceUse::Move) {
                            moved_places.push((path, block.id, index, instruction.span));
                        }
                    }
                    MirPlaceUse::Reinitialize => {
                        if moved_places.iter().any(|(moved_path, moved_block, moved_index, _)| {
                            let dominates = *moved_block == block.id && *moved_index < index
                                || *moved_block != block.id
                                    && dominators
                                        .get(&block.id)
                                        .is_some_and(|set| set.contains(moved_block));
                            dominates
                                && mir_place_path_is_prefix(moved_path, &path)
                                && moved_path != &path
                        }) {
                            return Err(MirLegalityError::InvalidMovedPlace {
                                function: function.id,
                                place: place_id.0,
                                span: instruction.span,
                            });
                        }
                        moved_places.retain(|(moved_path, moved_block, moved_index, _)| {
                            let dominates = *moved_block == block.id && *moved_index < index
                                || *moved_block != block.id
                                    && dominators
                                        .get(&block.id)
                                        .is_some_and(|set| set.contains(moved_block));
                            !(dominates && mir_place_path_is_prefix(&path, moved_path))
                        });
                    }
                }
            }
            let mut moved = Vec::new();
            match &instruction.operation {
                MirOperation::Move { value } => moved.push(*value),
                MirOperation::Drop { value, kind } if *kind != crate::MIR::MirDropKind::None => {
                    moved.push(*value)
                }
                MirOperation::Closure { captures, .. } => {
                    moved.extend(captures.iter().filter_map(|capture| match capture {
                        crate::MIR::MirCaptureOperand::Value(value) => Some(*value),
                        crate::MIR::MirCaptureOperand::Place(_) => None,
                    }));
                }
                MirOperation::Call { args, .. }
                | MirOperation::CoreCall { args, .. }
                | MirOperation::IndirectCall { args, .. } => {
                    moved.extend(
                        args.iter()
                            .filter(|arg| call_arg_consumes(arg))
                            .map(|arg| arg.value),
                    );
                }
                MirOperation::Semantic(
                    MirSemanticOp::StaticPreludeCall { args, .. }
                    | MirSemanticOp::HardwareCall { args, .. }
                    | MirSemanticOp::ClosureMethod { args, .. }
                    | MirSemanticOp::HostCall { args, .. },
                ) => {
                    moved.extend(
                        args.iter()
                            .filter(|arg| call_arg_consumes(arg))
                            .map(|arg| arg.value),
                    );
                }
                _ => {}
            }
            for value in moved {
                let Some((_, _, _, ownership)) = defs.get(&value) else {
                    return Err(MirLegalityError::InvalidReference {
                        function: function.id,
                        subject: format!("move {value:?}"),
                        span: instruction.span,
                    });
                };
                if matches!(
                    ownership.mode,
                    MirOwnershipMode::Copy
                        | MirOwnershipMode::ReadBorrow
                        | MirOwnershipMode::WriteBorrow
                ) || consumed
                    .iter()
                    .any(|(candidate, _, _, _)| *candidate == value)
                {
                    return Err(MirLegalityError::InvalidMove {
                        function: function.id,
                        value,
                        span: instruction.span,
                    });
                }
                consumed.push((value, block.id, index, instruction.span));
            }
        }
    }
    Ok(())
}

fn verify_drops(
    function: &MirFunction,
    block_ids: &HashSet<MirBlockId>,
    place_map: &HashMap<crate::MIR::MirPlaceId, &MirPlace>,
) -> Result<(), MirLegalityError> {
    for drop in &function.drops {
        let Some(place) = place_map.get(&drop.place).copied() else {
            return Err(MirLegalityError::InvalidDrop { function: function.id, place: drop.place.0, span: drop.span });
        };
        if drop.span.start > drop.span.end {
            return Err(MirLegalityError::InvalidSpan { function: Some(function.id), span: drop.span });
        }
        if place.ty.is_bool() || place.ty.is_char() || place.ty.is_float() || place.ty.is_integer() {
            return Err(MirLegalityError::InvalidDrop { function: function.id, place: drop.place.0, span: drop.span });
        }
        if let MirDropEdge::Failure(target) | MirDropEdge::Unwind(target) = drop.edge {
            if !block_ids.contains(&target) {
                return Err(MirLegalityError::InvalidFailureEdge { function: function.id, span: drop.span });
            }
        }
    }
    Ok(())
}

fn verify_facts(
    function: &MirFunction,
    block_ids: &HashSet<MirBlockId>,
    instruction_ids: &HashSet<crate::MIR::MirOpId>,
    defs: &HashMap<MirValueId, (MirBlockId, usize, MirType, MirOwnership)>,
) -> Result<(), MirLegalityError> {
    let facts = &function.optimization;
    if facts.pass_ids.len() > MIR_OPTIMIZATION_PASS_ORDER.len()
        || facts.pass_ids.iter().enumerate().any(|(index, pass)| MIR_OPTIMIZATION_PASS_ORDER.get(index) != Some(pass))
    {
        return Err(MirLegalityError::InvalidFact { function: function.id, span: function.span });
    }
    for row in &facts.loop_facts {
        if !block_ids.contains(&row.header)
            || row.body.is_some_and(|block| !block_ids.contains(&block))
            || row.exit.is_some_and(|block| !block_ids.contains(&block))
            || row.span.start > row.span.end
        {
            return Err(MirLegalityError::InvalidFact { function: function.id, span: row.span });
        }
    }
    for row in &facts.bounds_facts {
        if !instruction_ids.contains(&row.operation)
            || row.place.is_some_and(|place| !function.places.iter().any(|candidate| candidate.id == place))
            || row.index.is_some_and(|value| !defs.contains_key(&value))
            || (row.elided && !row.proven)
            || !row.failure_preserved
            || row.span.start > row.span.end
        {
            return Err(MirLegalityError::InvalidFact { function: function.id, span: row.span });
        }
    }
    for row in &facts.vector_facts {
        let invalid_access = row.accesses.iter().any(|access| {
            let invalid_root = match access.root {
                MirVectorAccessRoot::Place(place) => {
                    !function.places.iter().any(|candidate| candidate.id == place)
                }
                MirVectorAccessRoot::Value(value) => !defs.contains_key(&value),
            };
            let invalid_column =
                access.column_index.is_some()
                    != (access.layout == MirVectorLayout::ColumnarDirect
                        && access.field.is_some());
            invalid_root || invalid_column
        });
        let invalid_fixed = row.fixed_reduction.as_ref().is_some_and(|reduction| {
            let valid_rule = matches!(
                row.rule,
                MirVectorRule::Reduction | MirVectorRule::ConditionalAccumulate
            );
            let valid_condition = match row.rule {
                MirVectorRule::Reduction => reduction.condition.is_none(),
                MirVectorRule::ConditionalAccumulate => reduction
                    .condition
                    .is_some_and(|condition| defs.get(&condition).is_some_and(|(_, _, ty, _)| ty.is_bool())),
                _ => reduction.condition.is_none(),
            };
            let seed_read = function.blocks.iter().any(|block| {
                block.instructions.iter().any(|instruction| {
                    instruction.result == Some(reduction.seed)
                        && matches!(
                            &instruction.operation,
                            MirOperation::ReadPlace(place) if *place == reduction.accumulator
                        )
                })
            });
            let source_operations_valid = !reduction.source_operations.is_empty()
                && reduction
                    .source_operations
                    .iter()
                    .enumerate()
                    .all(|(index, operation)| {
                        !reduction.source_operations[..index].contains(operation)
                            && function.blocks.iter().any(|block| {
                                row.body_blocks.contains(&block.id)
                                    && block.instructions.iter().any(|instruction| {
                                        instruction.id == *operation
                                    })
                            })
                    });
            !valid_rule
                || !reduction.order.is_canonical()
                || !valid_condition
                || !function
                    .places
                    .iter()
                    .any(|place| place.id == reduction.accumulator)
                || !defs.contains_key(&reduction.addend)
                || !defs.contains_key(&reduction.seed)
                || !seed_read
                || !block_ids.contains(&reduction.exit)
                || !source_operations_valid
        });
        if !block_ids.contains(&row.loop_header)
            || row.cursor.is_some_and(|value| !defs.contains_key(&value))
            || row.body_blocks.iter().any(|block| !block_ids.contains(block))
            || row
                .advance_block
                .is_some_and(|block| !block_ids.contains(&block))
            || row.body_blocks.contains(&row.loop_header)
            || row
                .advance_block
                .is_some_and(|block| row.body_blocks.contains(&block))
            || invalid_access
            || invalid_fixed
            || row.span.start > row.span.end
        {
            return Err(MirLegalityError::InvalidFact {
                function: function.id,
                span: row.span,
            });
        }
        if let Some(ty) = &row.element_type {
            verify_type(ty, row.span, function.id)?;
        }
        let valid_lane = matches!(row.lane_width, None | Some(2 | 4 | 8));
        let layout_matches = row
            .accesses
            .iter()
            .all(|access| access.layout == MirVectorLayout::Flat || access.layout == row.layout);
        let supported_rule = matches!(
            row.rule,
            MirVectorRule::Elementwise
                | MirVectorRule::FieldAccess
                | MirVectorRule::ConditionalAccumulate
                | MirVectorRule::EarlyExitSearch
                | MirVectorRule::Reduction
        );
        let fixed_reduction_proven = match row.rule {
            MirVectorRule::Reduction => row.fixed_reduction.as_ref().is_some_and(|reduction| {
                reduction.order.is_canonical() && reduction.condition.is_none()
            }),
            MirVectorRule::ConditionalAccumulate => {
                row.fixed_reduction.as_ref().is_some_and(|reduction| {
                    reduction.order.is_canonical() && reduction.condition.is_some()
                })
            }
            _ => row.fixed_reduction.is_none(),
        };
        let control_flow_proven = match row.rule {
            MirVectorRule::EarlyExitSearch => !row.no_early_exit,
            _ => row.no_early_exit,
        };
        let eligible_proof = row.cursor.is_some()
            && !row.body_blocks.is_empty()
            && row.advance_block.is_some()
            && !row.accesses.is_empty()
            && row.element_type.as_ref().is_some_and(|ty| is_packable_for_rule(row.rule, ty))
            && row.packed
            && valid_lane
            && layout_matches
            && row.no_aliasing
            && control_flow_proven
            && row.effect_free_body
            && row.no_cross_iteration_dependencies
            && supported_rule
            && fixed_reduction_proven;
        if row.decision.is_eligible() && !eligible_proof {
            return Err(MirLegalityError::InvalidFact {
                function: function.id,
                span: row.span,
            });
        }
    }
    let has_eligible = facts
        .vector_facts
        .iter()
        .any(|fact| fact.decision.is_eligible());
    let expected_no_aliasing = has_eligible
        && facts
            .vector_facts
            .iter()
            .filter(|fact| fact.decision.is_eligible())
            .all(|fact| fact.no_aliasing);
    let expected_no_early_exit = has_eligible
        && facts
            .vector_facts
            .iter()
            .filter(|fact| fact.decision.is_eligible())
            .all(|fact| fact.no_early_exit);
    let expected_no_cross_iteration_dependencies = has_eligible
        && facts
            .vector_facts
            .iter()
            .filter(|fact| fact.decision.is_eligible())
            .all(|fact| fact.no_cross_iteration_dependencies);
    if facts.auto_vectorizable != has_eligible
        || facts.no_aliasing != expected_no_aliasing
        || facts.no_early_exit != expected_no_early_exit
        || facts.no_cross_iteration_dependencies != expected_no_cross_iteration_dependencies
    {
        return Err(MirLegalityError::InvalidFact {
            function: function.id,
            span: function.span,
        });
    }
    for row in &facts.fusion_facts {
        if !block_ids.contains(&row.first_loop)
            || !block_ids.contains(&row.second_loop)
            || row.span.start > row.span.end
            || matches!(row.decision, MirOptimizationDecision::Eligible)
                && !(row.packed && row.same_iteration_domain && row.no_aliasing && row.effect_free && row.no_cross_iteration_dependencies)
        {
            return Err(MirLegalityError::InvalidFact { function: function.id, span: row.span });
        }
    }
    for row in &facts.acceleration_facts {
        if !block_ids.contains(&row.loop_header) || row.span.start > row.span.end {
            return Err(MirLegalityError::InvalidFact {
                function: function.id,
                span: row.span,
            });
        }
    }
    Ok(())
}

/// Reject MIR that has not completed the one canonical pass pipeline.
///
/// Adapters call this at their public boundary instead of duplicating pass
/// ordering or attempting target-local repair.
pub fn require_canonical_mir_optimization(
    program: &MirProgram,
) -> Result<(), MirOptimizationError> {
    verify_mir_legality(program).map_err(|error| MirOptimizationError::Legality {
        pass: MirOptimizationPassId::LegalityVerification,
        error,
    })?;
    let input_digest = mir_program_digest(program);
    if !optimized_pass_order_complete(program, &input_digest) {
        return Err(MirOptimizationError::PassPrecondition {
            pass: MirOptimizationPassId::CanonicalLoopFacts,
            function: program
                .functions
                .iter()
                .find(|function| {
                    function.optimization.pass_ids.as_slice()
                        != MIR_OPTIMIZATION_PASS_ORDER.as_slice()
                        || function.optimization.derived_from_digest.as_ref()
                            != Some(&input_digest)
                })
                .map(|function| function.id),
            reason: "canonical MIR optimization pipeline has not completed for this MIR".to_string(),
        });
    }
    record_canonical_check(
        "mir.require-canonical-optimization",
        "crates/jet-foundation/src/MIROptimization.rs",
        program,
    );
    Ok(())
}
fn run_canonical_pass<F>(
    operation_id: &str,
    source: &str,
    program: &mut MirProgram,
    apply: F,
) where
    F: FnOnce(&mut MirProgram),
{
    if !CanonicalPass::enabled() {
        apply(program);
        return;
    }
    let before_payload = crate::MIR::canonical_payload(program);
    let before_identity = crate::MIR::canonical_identity(program);
    apply(program);
    CanonicalPass::record(
        "optimization",
        operation_id,
        source,
        "mir",
        before_payload,
        before_identity,
        "mir",
        crate::MIR::canonical_payload(program),
        crate::MIR::canonical_identity(program),
        "preserve",
    );
}

fn record_canonical_check(operation_id: &str, source: &str, program: &MirProgram) {
    if !CanonicalPass::enabled() {
        return;
    }
    CanonicalPass::record(
        "optimization",
        operation_id,
        source,
        "mir",
        crate::MIR::canonical_payload(program),
        crate::MIR::canonical_identity(program),
        "mir",
        crate::MIR::canonical_payload(program),
        crate::MIR::canonical_identity(program),
        "checked",
    );
}

fn record_canonical_transition(
    operation_id: &str,
    source: &str,
    before_payload: Option<String>,
    before_identity: Option<String>,
    after: &MirProgram,
) {
    let (Some(before_payload), Some(before_identity)) = (before_payload, before_identity) else {
        return;
    };
    CanonicalPass::record(
        "optimization",
        operation_id,
        source,
        "mir",
        before_payload,
        before_identity,
        "mir",
        crate::MIR::canonical_payload(after),
        crate::MIR::canonical_identity(after),
        "preserve",
    );
}

pub fn optimize_mir_program(
    program: &MirProgram,
    policy: &MirOptimizationPolicy,
) -> Result<MirProgram, MirOptimizationError> {
    let _ = policy;
    let pipeline_before_payload = CanonicalPass::enabled()
        .then(|| crate::MIR::canonical_payload(program));
    let pipeline_before_identity = CanonicalPass::enabled()
        .then(|| crate::MIR::canonical_identity(program));
    verify_mir_legality(program).map_err(|error| MirOptimizationError::Legality {
        pass: MirOptimizationPassId::LegalityVerification,
        error,
    })?;
    record_canonical_check(
        "mir.legality-verification",
        "crates/jet-foundation/src/MIROptimization.rs",
        program,
    );
    let input_digest = mir_program_digest(program);
    if optimized_pass_order_complete(program, &input_digest) {
        let mut optimized = program.clone();
        run_canonical_pass(
            "mir.fixed-reduction-normalization",
            "crates/jet-foundation/src/MIROptimization.rs",
            &mut optimized,
            normalize_fixed_reduction_loops,
        );
        run_canonical_pass(
            "mir.acceleration-facts",
            "crates/jet-foundation/src/MIROptimization.rs",
            &mut optimized,
            refresh_acceleration_facts,
        );
        validate_after(&optimized, MirOptimizationPassId::CanonicalLoopFacts)?;
        seal_derived_facts(&mut optimized);
        record_canonical_transition(
            "mir.optimize-pipeline",
            "crates/jet-foundation/src/MIROptimization.rs",
            pipeline_before_payload,
            pipeline_before_identity,
            &optimized,
        );
        return Ok(optimized);
    }
    let mut optimized = program.clone();
    clear_derived_facts(&mut optimized);
    denormalize_fixed_reduction_loops(&mut optimized);
    // Checked #Inline(Always) is expanded here, before any target-neutral
    // proof pass. Keeping it in this one MIR pipeline means every execution
    // adapter consumes the same CFG, while the completed pass fingerprint
    // prevents a second expansion on already optimized input.
    run_canonical_pass(
        "mir.inline-always",
        "crates/jet-foundation/src/MIROptimization.rs",
        &mut optimized,
        expand_inline_always,
    );
    mark_pass(&mut optimized, MirOptimizationPassId::LegalityVerification);
    validate_after(&optimized, MirOptimizationPassId::LegalityVerification)?;

    run_canonical_pass(
        "mir.unreachable-block-elimination",
        "crates/jet-foundation/src/MIROptimization.rs",
        &mut optimized,
        eliminate_unreachable_blocks,
    );
    mark_pass(&mut optimized, MirOptimizationPassId::UnreachableBlockElimination);
    validate_after(&optimized, MirOptimizationPassId::UnreachableBlockElimination)?;

    run_canonical_pass(
        "mir.cfg-simplification",
        "crates/jet-foundation/src/MIROptimization.rs",
        &mut optimized,
        simplify_cfg,
    );
    mark_pass(&mut optimized, MirOptimizationPassId::CfgSimplification);
    validate_after(&optimized, MirOptimizationPassId::CfgSimplification)?;

    run_canonical_pass(
        "mir.exact-constant-folding",
        "crates/jet-foundation/src/MIROptimization.rs",
        &mut optimized,
        fold_exact_constants,
    );
    mark_pass(&mut optimized, MirOptimizationPassId::ExactConstantFolding);
    validate_after(&optimized, MirOptimizationPassId::ExactConstantFolding)?;

    run_canonical_pass(
        "mir.bounds-check-elimination",
        "crates/jet-foundation/src/MIROptimization.rs",
        &mut optimized,
        derive_bounds_facts,
    );
    mark_pass(&mut optimized, MirOptimizationPassId::BoundsCheckElimination);
    validate_after(&optimized, MirOptimizationPassId::BoundsCheckElimination)?;

    run_canonical_pass(
        "mir.dead-pure-value-elimination",
        "crates/jet-foundation/src/MIROptimization.rs",
        &mut optimized,
        eliminate_dead_pure_values,
    );
    mark_pass(&mut optimized, MirOptimizationPassId::DeadPureValueElimination);
    validate_after(&optimized, MirOptimizationPassId::DeadPureValueElimination)?;

    run_canonical_pass(
        "mir.canonical-loop-facts",
        "crates/jet-foundation/src/MIROptimization.rs",
        &mut optimized,
        derive_loop_and_vector_facts,
    );

    // Reduction candidates become final facts only after normalization.
    run_canonical_pass(
        "mir.fixed-reduction-normalization",
        "crates/jet-foundation/src/MIROptimization.rs",
        &mut optimized,
        normalize_fixed_reduction_loops,
    );
    // The reducer rewrites the CFG. Rebuild all loop-dependent facts from the
    // generated shape before sealing the pass, then restore the source
    // reduction row without applying the rewrite a second time.
    derive_loop_and_vector_facts(&mut optimized);
    normalize_fixed_reduction_loops(&mut optimized);
    mark_pass(&mut optimized, MirOptimizationPassId::CanonicalLoopFacts);
    validate_after(&optimized, MirOptimizationPassId::CanonicalLoopFacts)?;

    run_canonical_pass(
        "mir.acceleration-facts",
        "crates/jet-foundation/src/MIROptimization.rs",
        &mut optimized,
        refresh_acceleration_facts,
    );
    validate_after(&optimized, MirOptimizationPassId::CanonicalLoopFacts)?;
    run_canonical_pass(
        "mir.canonical-program-order",
        "crates/jet-foundation/src/MIROptimization.rs",
        &mut optimized,
        canonicalize_program_order,
    );
    validate_after(&optimized, MirOptimizationPassId::CanonicalLoopFacts)?;
    seal_derived_facts(&mut optimized);
    record_canonical_transition(
        "mir.optimize-pipeline",
        "crates/jet-foundation/src/MIROptimization.rs",
        pipeline_before_payload,
        pipeline_before_identity,
        &optimized,
    );
    Ok(optimized)
}
/// Project the compiler-owned MIR facts into the one source-facing decision
/// ledger.  This is a projection only: no proof or profitability analysis is
/// performed here, and an absent fact remains absent.
pub fn decision_ledger_rows(program: &MirProgram) -> Vec<MirDecisionRow> {
    let mut rows = Vec::new();
    for function in &program.functions {
        rows.extend(function.optimization.decision_rows.iter().cloned());

        if function.is_inline || function.is_inline_always {
            let rule = if function.is_inline_always {
                "#Inline(Always)"
            } else {
                "#Inline"
            };
            rows.push(MirDecisionRow::new(
                MirDecisionKind::Inline,
                MirDecisionDisposition::Selected,
                Some(function.id),
                function.key.clone(),
                function.span,
                rule,
                "the checked function carries this inline contract",
                "mir.inline",
                "checked-function-metadata",
            ));
        }

        if let Some(kernel) = function.kernel_proof {
            let proven = kernel.bounds
                && kernel.alias_free
                && kernel.captures
                && kernel.race_free
                && kernel.barriers_uniform
                && kernel.control_flow;
            rows.push(MirDecisionRow::new(
                MirDecisionKind::Parallel,
                if proven {
                    MirDecisionDisposition::Accepted
                } else {
                    MirDecisionDisposition::Rejected
                },
                Some(function.id),
                function.key.clone(),
                function.span,
                "kernel-parallel",
                if proven {
                    "the checked kernel satisfies the parallel safety proof"
                } else {
                    "the checked kernel does not satisfy every parallel safety obligation"
                },
                "sema.kernel-proof",
                "static-legality-proof",
            ));
        }

        for loop_fact in &function.optimization.loop_facts {
            let (disposition, reason) = match loop_fact.copy_cost {
                MirCopyCost::None => (
                    MirDecisionDisposition::NotAttempted,
                    "no loop copy was required by the checked ownership fact",
                ),
                MirCopyCost::PerIteration(count) => (
                    MirDecisionDisposition::Selected,
                    if count == 1 {
                        "the compiler inserted one copy per iteration"
                    } else {
                        "the compiler inserted copies per iteration"
                    },
                ),
                MirCopyCost::Unknown => (
                    MirDecisionDisposition::Unavailable,
                    "the compiler has no bounded copy-cost fact",
                ),
            };
            rows.push(MirDecisionRow::new(
                MirDecisionKind::Copy,
                disposition,
                Some(function.id),
                function.key.clone(),
                loop_fact.span,
                "loop-copy-cost",
                reason,
                "mir.loop-facts",
                "static-cost-fact",
            ));
        }

        for bounds in &function.optimization.bounds_facts {
            let (disposition, reason) = if bounds.proven && bounds.elided {
                (
                    MirDecisionDisposition::Accepted,
                    "the checked bounds proof permits elision",
                )
            } else if bounds.proven {
                (
                    MirDecisionDisposition::Rejected,
                    "the bounds proof is present but the failure check is retained",
                )
            } else {
                (
                    MirDecisionDisposition::NotAttempted,
                    "no checked bounds proof is available for this access",
                )
            };
            rows.push(MirDecisionRow::new(
                MirDecisionKind::Bounds,
                disposition,
                Some(function.id),
                function.key.clone(),
                bounds.span,
                "bounds-check-elimination",
                reason,
                "mir.bounds",
                "static-legality-proof",
            ));
        }

        for vector in &function.optimization.vector_facts {
            let (disposition, reason) = match &vector.decision {
                MirOptimizationDecision::Eligible => (
                    MirDecisionDisposition::Accepted,
                    format!("the {} vector proof was accepted", vector.rule.as_str()),
                ),
                MirOptimizationDecision::Rejected(rejection) => {
                    let reason = if vector.rule == MirVectorRule::Reduction
                        && vector.fixed_reduction.is_none()
                    {
                        "strict-order reduction proof was not accepted".to_string()
                    } else {
                        format!("vector proof rejected: {}", rejection.as_str())
                    };
                    (MirDecisionDisposition::Rejected, reason)
                }
            };
            rows.push(MirDecisionRow::new(
                MirDecisionKind::Vectorize,
                disposition,
                Some(function.id),
                function.key.clone(),
                vector.span,
                vector.rule.as_str(),
                reason,
                "mir.vector-proof",
                "static-legality-proof",
            ));
        }
    }

    for unreachable in &program.unreachable {
        rows.push(MirDecisionRow::new(
            MirDecisionKind::Unreachable,
            MirDecisionDisposition::Selected,
            None,
            String::new(),
            unreachable.span,
            unreachable.construct.clone(),
            format!("{:?}", unreachable.reason).to_lowercase(),
            "tir.erasure",
            "static-control-flow-fact",
        ));
    }

    rows.sort_by(|left, right| {
        left.span
            .start
            .cmp(&right.span.start)
            .then_with(|| left.span.end.cmp(&right.span.end))
            .then_with(|| left.kind.cmp(&right.kind))
            .then_with(|| left.function_name.cmp(&right.function_name))
            .then_with(|| left.id.cmp(&right.id))
    });
    rows
}

/// Materialize the canonical decision ledger for a checked MIR program.
/// Callers may append producer rows (for example sema lints or JIT tier
/// observations) before passing them here; all rows receive the same checked
/// derivation identity.
pub fn decision_ledger(
    program: &MirProgram,
    artifact: Option<crate::MIR::MirArtifactId>,
    profile: impl Into<String>,
    run: impl Into<String>,
    additional_rows: impl IntoIterator<Item = MirDecisionRow>,
) -> MirDecisionLedger {
    let identity = MirDecisionIdentity::from_program(program, artifact, profile, run);
    let mut rows = decision_ledger_rows(program);
    rows.extend(additional_rows);
    MirDecisionLedger::from_rows(identity, rows)
}

/// Expand checked `#Inline(Always)` calls into the caller's MIR CFG.
///
/// This is deliberately a MIR transform rather than a TIR expression rewrite:
/// the cloned body keeps every branch, cleanup operation, failure edge, and
/// early return visible to all later adapters.
fn expand_inline_always(program: &mut MirProgram) {
    loop {
        let mut changed = false;
        'search: for caller_index in 0..program.functions.len() {
            let caller_id = program.functions[caller_index].id;
            let mut site = None;
            for (block_index, block) in program.functions[caller_index].blocks.iter().enumerate() {
                for (instruction_index, instruction) in block.instructions.iter().enumerate() {
                    let Some(target_id) = inline_target_id(&instruction.operation) else {
                        continue;
                    };
                    let Some(callee) = program.functions.iter().find(|function| function.id == target_id)
                    else {
                        continue;
                    };
                    if !callee.is_inline_always
                        || callee.id == caller_id
                        || inline_path(program, callee.id, caller_id, &mut HashSet::new())
                        || inline_path(program, callee.id, callee.id, &mut HashSet::new())
                    {
                        continue;
                    }
                    site = Some((block_index, instruction_index, instruction.clone(), target_id));
                    break;
                }
                if site.is_some() {
                    break;
                }
            }
            let Some((block_index, instruction_index, instruction, target_id)) = site else {
                continue;
            };
            let Some(callee) = program.functions.iter().find(|function| function.id == target_id).cloned()
            else {
                continue;
            };
            if inline_call(
                &mut program.functions[caller_index],
                &callee,
                block_index,
                instruction_index,
                &instruction,
            ) {
                let caller = &mut program.functions[caller_index];
                caller.optimization.decision_rows.push(MirDecisionRow::new(
                    MirDecisionKind::Inline,
                    MirDecisionDisposition::Accepted,
                    Some(caller.id),
                    caller.key.clone(),
                    instruction.span,
                    "#Inline(Always)",
                    format!("expanded checked callee `{}` into this call site", callee.key),
                    "mir.inline",
                    "canonical-cfg-transform",
                ));
                changed = true;
                break 'search;
            }
        }
        if !changed {
            break;
        }
    }
}

fn inline_target_id(operation: &MirOperation) -> Option<MirFunctionId> {
    let MirOperation::Call { callee, .. } = operation else {
        return None;
    };
    match callee {
        MirCallee::User(function)
        | MirCallee::Associated { function, .. }
        | MirCallee::Method { function, .. } => Some(*function),
        _ => None,
    }
}

fn inline_path(
    program: &MirProgram,
    current: MirFunctionId,
    wanted: MirFunctionId,
    seen: &mut HashSet<MirFunctionId>,
) -> bool {
    if !seen.insert(current) {
        return false;
    }
    let Some(function) = program.functions.iter().find(|function| function.id == current) else {
        return false;
    };
    for target in function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .filter_map(|instruction| inline_target_id(&instruction.operation))
    {
        let Some(target_function) = program.functions.iter().find(|function| function.id == target)
        else {
            continue;
        };
        if !target_function.is_inline_always {
            continue;
        }
        if target == wanted || inline_path(program, target, wanted, seen) {
            return true;
        }
    }
    false
}

#[derive(Default)]
struct InlineIdMaps {
    values: HashMap<MirValueId, MirValueId>,
    blocks: HashMap<MirBlockId, MirBlockId>,
    locals: HashMap<MirLocalId, MirLocalId>,
    places: HashMap<MirPlaceId, MirPlaceId>,
    scopes: HashMap<MirScopeId, MirScopeId>,
}

fn inline_call(
    caller: &mut MirFunction,
    callee: &MirFunction,
    block_index: usize,
    instruction_index: usize,
    call_instruction: &MirInstruction,
) -> bool {
    let MirOperation::Call {
        callee: call_callee,
        args,
        type_args,
    } = &call_instruction.operation
    else {
        return false;
    };
    if inline_target_id(&call_instruction.operation) != Some(callee.id)
        || callee.capture_params.len() != 0
        || (caller.generator.is_none() && callee.generator.is_some())
        || block_index >= caller.blocks.len()
        || instruction_index >= caller.blocks[block_index].instructions.len()
    {
        return false;
    }
    let substitutions = if callee.generic_params.is_empty() {
        HashMap::new()
    } else {
        if callee.generic_params.len() != type_args.len() {
            return false;
        }
        callee
            .generic_params
            .iter()
            .zip(type_args)
            .map(|(parameter, ty)| (parameter.name.clone(), ty.clone()))
            .collect::<HashMap<_, _>>()
    };
    if args.len() != callee.params.len() {
        return false;
    }
    let return_type = inline_type(&callee.return_type, &substitutions);
    if let Some(call_type) = &call_instruction.ty {
        if !call_type.same_checked_type(&return_type) {
            return false;
        }
    }
    let caller_values = caller
        .values
        .iter()
        .map(|(id, ty, span, ownership)| (*id, (ty.clone(), *span, *ownership)))
        .collect::<HashMap<_, _>>();
    for (argument, parameter) in args.iter().zip(&callee.params) {
        let Some((argument_type, _, _)) = caller_values.get(&argument.value) else {
            return false;
        };
        if !argument_type.same_checked_type(&inline_type(&parameter.ty, &substitutions)) {
            return false;
        }
        if argument.fn_coercion.is_some()
            || argument.widen_fixed_to_list
            || argument.widen_to_union.is_some()
            || argument.box_as_trait.is_some()
            || argument.authority_boundary
        {
            // These are backend adaptation boundaries.  They need a checked
            // MIR conversion operation before their call can be erased.
            return false;
        }
    }
    let callee_reachable = reachable_blocks(callee);
    let reachable_returns = callee
        .blocks
        .iter()
        .filter(|block| {
            callee_reachable.contains(&block.id)
                && matches!(block.terminator, MirTerminator::Return { .. })
        })
        .count();
    if reachable_returns == 0 && call_instruction.result.is_some() {
        return false;
    }

    let mut used_values = caller.values.iter().map(|(id, ..)| id.0).collect::<HashSet<_>>();
    let mut used_operations = caller
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .map(|instruction| instruction.id.0)
        .collect::<HashSet<_>>();
    let mut used_blocks = caller.blocks.iter().map(|block| block.id.0).collect::<HashSet<_>>();
    let mut used_locals = caller.locals.iter().map(|local| local.id.0).collect::<HashSet<_>>();
    let mut used_places = caller.places.iter().map(|place| place.id.0).collect::<HashSet<_>>();
    let mut used_scopes = caller.scopes.iter().map(|scope| scope.id.0).collect::<HashSet<_>>();
    let identity = |kind: &str, source: u64, ordinal: usize| {
        format!(
            "caller={}:call={}:callee={}:kind={kind}:source={source}:ordinal={ordinal}",
            caller.id.0, call_instruction.id.0, callee.id.0
        )
    };

    let original_block = caller.blocks[block_index].clone();
    let mut adapted_values = Vec::new();
    let mut adapted_arguments = Vec::with_capacity(args.len());
    let mut adapter_instructions = Vec::new();
    for (ordinal, argument) in args.iter().enumerate() {
        let Some((argument_type, argument_span, argument_ownership)) = caller_values.get(&argument.value)
        else {
            return false;
        };
        let adapted = if argument.implicit_clone || argument.shared_auto_clone {
            let value = MirValueId(inline_fresh_id(
                &mut used_values,
                "mir-inline-value",
                &identity("clone", argument.value.0, ordinal),
            ));
            let operation_id = MirOpId(inline_fresh_id(
                &mut used_operations,
                "mir-inline-op",
                &identity("clone-op", argument.value.0, ordinal),
            ));
            let mut ownership = *argument_ownership;
            ownership.moved = false;
            ownership.last_use = false;
            adapted_values.push((value, argument_type.clone(), *argument_span, ownership));
            adapter_instructions.push(MirInstruction {
                id: operation_id,
                span: *argument_span,
                source_line: None,
                result: Some(value),
                ty: Some(argument_type.clone()),
                operation: MirOperation::Copy {
                    value: argument.value,
                },
            });
            value
        } else if call_arg_consumes(argument) {
            let value = MirValueId(inline_fresh_id(
                &mut used_values,
                "mir-inline-value",
                &identity("move", argument.value.0, ordinal),
            ));
            let operation_id = MirOpId(inline_fresh_id(
                &mut used_operations,
                "mir-inline-op",
                &identity("move-op", argument.value.0, ordinal),
            ));
            let mut ownership = *argument_ownership;
            ownership.moved = false;
            ownership.last_use = false;
            adapted_values.push((value, argument_type.clone(), *argument_span, ownership));
            adapter_instructions.push(MirInstruction {
                id: operation_id,
                span: *argument_span,
                source_line: None,
                result: Some(value),
                ty: Some(argument_type.clone()),
                operation: MirOperation::Move {
                    value: argument.value,
                },
            });
            value
        } else {
            argument.value
        };
        adapted_arguments.push(adapted);
    }

    let mut prefix = original_block.instructions[..instruction_index].to_vec();
    prefix.extend(adapter_instructions);
    let suffix = original_block.instructions[instruction_index + 1..].to_vec();
    let original_terminator = original_block.terminator.clone();

    let mut ids = InlineIdMaps::default();
    let mut parameter_values = HashMap::new();
    for block in &callee.blocks {
        for instruction in &block.instructions {
            if let MirOperation::Parameter { index, .. } = instruction.operation {
                let Some(value) = instruction.result else {
                    return false;
                };
                if parameter_values.insert(index, value).is_some() {
                    return false;
                }
            }
        }
    }
    if parameter_values.len() != callee.params.len() {
        return false;
    }
    for (index, adapted) in adapted_arguments.iter().enumerate() {
        let Some(parameter_value) = parameter_values.get(&index).copied() else {
            return false;
        };
        ids.values.insert(parameter_value, *adapted);
    }
    for (value, _, _, _) in &callee.values {
        if ids.values.contains_key(value) {
            continue;
        }
        ids.values.insert(
            *value,
            MirValueId(inline_fresh_id(
                &mut used_values,
                "mir-inline-value",
                &identity("value", value.0, 0),
            )),
        );
    }
    for block in &callee.blocks {
        ids.blocks.insert(
            block.id,
            MirBlockId(inline_fresh_id(
                &mut used_blocks,
                "mir-inline-block",
                &identity("block", block.id.0, 0),
            )),
        );
    }
    for local in &callee.locals {
        ids.locals.insert(
            local.id,
            MirLocalId(inline_fresh_id(
                &mut used_locals,
                "mir-inline-local",
                &identity("local", local.id.0, 0),
            )),
        );
    }
    for scope in &callee.scopes {
        ids.scopes.insert(
            scope.id,
            MirScopeId(inline_fresh_id(
                &mut used_scopes,
                "mir-inline-scope",
                &identity("scope", scope.id.0, 0),
            )),
        );
    }

    let parameter_indexes = parameter_values
        .iter()
        .map(|(index, value)| (*value, *index))
        .collect::<HashMap<_, _>>();
    let caller_places = caller
        .places
        .iter()
        .map(|place| (place.id, place.clone()))
        .collect::<HashMap<_, _>>();
    let mut cloned_places = Vec::new();
    for place in &callee.places {
        let mapped_place = match &place.base {
            MirPlaceBase::Parameter(value) => {
                let Some(index) = parameter_indexes.get(value).copied() else {
                    return false;
                };
                if let Some(source_place_id) = args[index].place {
                    let Some(source_place) = caller_places.get(&source_place_id) else {
                        return false;
                    };
                    if place.projections.is_empty() {
                        ids.places.insert(place.id, source_place.id);
                        continue;
                    }
                    let mapped_id = MirPlaceId(inline_fresh_id(
                        &mut used_places,
                        "mir-inline-place",
                        &identity("parameter-place", place.id.0, index),
                    ));
                    let mut projections = source_place.projections.clone();
                    projections.extend(
                        place
                            .projections
                            .iter()
                            .map(|projection| inline_projection(projection, &ids)),
                    );
                    let mapped = MirPlace {
                        id: mapped_id,
                        span: place.span,
                        ty: inline_type(&place.ty, &substitutions),
                        base: source_place.base.clone(),
                        projections,
                        access: place.access,
                        persist_key: place.persist_key.clone().or(source_place.persist_key.clone()),
                    };
                    ids.places.insert(place.id, mapped_id);
                    cloned_places.push(mapped);
                    continue;
                }
                MirPlaceBase::Parameter(inline_value(&ids, *value))
            }
            MirPlaceBase::Local(local) => MirPlaceBase::Local(inline_local(&ids, *local)),
            MirPlaceBase::Capture(value) => MirPlaceBase::Capture(inline_value(&ids, *value)),
            MirPlaceBase::Temporary(value) => MirPlaceBase::Temporary(inline_value(&ids, *value)),
            MirPlaceBase::Static(name) => MirPlaceBase::Static(name.clone()),
        };
        let mapped_id = MirPlaceId(inline_fresh_id(
            &mut used_places,
            "mir-inline-place",
            &identity("place", place.id.0, 0),
        ));
        ids.places.insert(place.id, mapped_id);
        cloned_places.push(MirPlace {
            id: mapped_id,
            span: place.span,
            ty: inline_type(&place.ty, &substitutions),
            base: mapped_place,
            projections: place
                .projections
                .iter()
                .map(|projection| inline_projection(projection, &ids))
                .collect(),
            access: place.access,
            persist_key: place.persist_key.clone(),
        });
    }
    let cloned_locals = callee
        .locals
        .iter()
        .map(|local| MirLocal {
            id: ids.locals[&local.id],
            name: local.name.clone(),
            span: local.span,
            ty: inline_type(&local.ty, &substitutions),
            place: ids.places[&local.place],
            mutable: local.mutable,
            ownership: local.ownership,
            comptime: local.comptime,
            uninit: local.uninit,
            arena_view: local.arena_view,
            string_view: local.string_view,
            gc_root: local.gc_root,
        })
        .collect::<Vec<_>>();
    let cloned_scopes = callee
        .scopes
        .iter()
        .map(|scope| MirScope {
            id: ids.scopes[&scope.id],
            kind: scope.kind.clone(),
            span: scope.span,
            name: scope.name.clone(),
            facts: scope.facts.clone(),
        })
        .collect::<Vec<_>>();
    let mut cloned_values = Vec::new();
    for (value, ty, span, ownership) in &callee.values {
        if parameter_values.values().any(|parameter| parameter == value) {
            continue;
        }
        cloned_values.push((
            ids.values[value],
            inline_type(ty, &substitutions),
            *span,
            *ownership,
        ));
    }

    let continuation = if reachable_returns > 0 {
        Some(MirBlockId(inline_fresh_id(
            &mut used_blocks,
            "mir-inline-block",
            &identity("continuation", call_instruction.id.0, 0),
        )))
    } else {
        None
    };
    let mut cloned_blocks = Vec::new();
    let mut return_sites = Vec::new();
    for block in &callee.blocks {
        let mapped_id = ids.blocks[&block.id];
        let mut instructions = Vec::new();
        for instruction in &block.instructions {
            if matches!(instruction.operation, MirOperation::Parameter { .. }) {
                continue;
            }
            let operation = inline_operation(&instruction.operation, &ids, &substitutions);
            instructions.push(MirInstruction {
                id: MirOpId(inline_fresh_id(
                    &mut used_operations,
                    "mir-inline-op",
                    &identity("instruction", instruction.id.0, 0),
                )),
                span: instruction.span,
                source_line: instruction.source_line,
                result: instruction.result.map(|value| inline_value(&ids, value)),
                ty: instruction.ty.as_ref().map(|ty| inline_type(ty, &substitutions)),
                operation,
            });
        }
        let terminator = match &block.terminator {
            MirTerminator::Return { value } if callee_reachable.contains(&block.id) => {
                let Some(continuation) = continuation else {
                    return false;
                };
                let source_return_value = *value;
                let mapped_value = value.map(|value| inline_value(&ids, value));
                let mapped_value = if mapped_value.is_none() && call_instruction.result.is_some() {
                    if !return_type.is_unit() {
                        return false;
                    }
                    let unit_value = MirValueId(inline_fresh_id(
                        &mut used_values,
                        "mir-inline-value",
                        &identity("unit-return", block.id.0, 0),
                    ));
                    let unit_operation = MirOpId(inline_fresh_id(
                        &mut used_operations,
                        "mir-inline-op",
                        &identity("unit-return-op", block.id.0, 0),
                    ));
                    instructions.push(MirInstruction {
                        id: unit_operation,
                        span: block.span,
                        source_line: None,
                        result: Some(unit_value),
                        ty: Some(return_type.clone()),
                        operation: MirOperation::Constant(MirConstant::Unit),
                    });
                    cloned_values.push((
                        unit_value,
                        return_type.clone(),
                        block.span,
                        MirOwnership::copy(),
                    ));
                    Some(unit_value)
                } else {
                    mapped_value
                };
                if call_instruction.result.is_none() {
                    if let Some(value) = mapped_value {
                        if let Some((_, _, _, ownership)) = callee
                            .values
                            .iter()
                            .find(|(id, ..)| Some(*id) == source_return_value)
                        {
                            if ownership.drop != MirDropKind::None {
                                instructions.push(MirInstruction {
                                    id: MirOpId(inline_fresh_id(
                                        &mut used_operations,
                                        "mir-inline-op",
                                        &identity("discard-return", block.id.0, 0),
                                    )),
                                    span: block.span,
                                    source_line: None,
                                    result: None,
                                    ty: None,
                                    operation: MirOperation::Drop {
                                        value,
                                        kind: ownership.drop,
                                    },
                                });
                            }
                        }
                    }
                }
                return_sites.push((mapped_id, mapped_value));
                MirTerminator::Jump { target: continuation }
            }
            other => inline_terminator(other, &ids),
        };
        cloned_blocks.push(MirBasicBlock {
            id: mapped_id,
            span: block.span,
            instructions,
            terminator,
        });
    }
    if let Some(continuation) = continuation {
        let mut continuation_instructions = Vec::new();
        if let Some(result) = call_instruction.result {
            let Some(call_type) = call_instruction.ty.clone() else {
                return false;
            };
            let incoming = return_sites
                .iter()
                .map(|(block, value)| {
                    let Some(value) = value else {
                        return None;
                    };
                    Some((*block, *value))
                })
                .collect::<Option<Vec<_>>>();
            let Some(incoming) = incoming else {
                return false;
            };
            continuation_instructions.push(MirInstruction {
                id: MirOpId(inline_fresh_id(
                    &mut used_operations,
                    "mir-inline-op",
                    &identity("return-phi", call_instruction.id.0, 0),
                )),
                span: call_instruction.span,
                source_line: call_instruction.source_line,
                result: Some(result),
                ty: Some(call_type),
                operation: MirOperation::Phi { incoming },
            });
        }
        cloned_blocks.push(MirBasicBlock {
            id: continuation,
            span: call_instruction.span,
            instructions: continuation_instructions
                .into_iter()
                .chain(suffix)
                .collect(),
            terminator: original_terminator,
        });
    } else {
        caller
            .values
            .retain(|(value, ..)| Some(*value) != call_instruction.result);
    }

    let mut drops = Vec::new();
    for drop in &callee.drops {
        let Some(source_place) = callee.places.iter().find(|place| place.id == drop.place) else {
            continue;
        };
        if matches!(&source_place.base, MirPlaceBase::Parameter(_)) {
            continue;
        }
        let edge = match &drop.edge {
            MirDropEdge::Normal => MirDropEdge::Normal,
            MirDropEdge::Return => MirDropEdge::Return,
            MirDropEdge::Failure(block) => MirDropEdge::Failure(ids.blocks[block]),
            MirDropEdge::Unwind(block) => MirDropEdge::Unwind(ids.blocks[block]),
        };
        drops.push(MirDropAction {
            place: ids.places[&drop.place],
            edge,
            span: drop.span,
        });
    }

    caller.blocks[block_index].instructions = prefix;
    caller.blocks[block_index].terminator = MirTerminator::Jump {
        target: ids.blocks[&callee.entry],
    };
    caller.blocks.extend(cloned_blocks);
    caller.values.extend(adapted_values);
    caller.values.extend(cloned_values);
    caller.locals.extend(cloned_locals);
    caller.places.extend(cloned_places);
    caller.scopes.extend(cloned_scopes);
    caller.drops.extend(drops);
    let _ = call_callee;
    true
}

fn inline_fresh_id(used: &mut HashSet<u64>, namespace: &str, identity: &str) -> u64 {
    let mut salt = 0u64;
    loop {
        let key = format!("{identity}:salt={salt}");
        let id = stable_id(namespace, &key);
        if used.insert(id) {
            return id;
        }
        salt = salt.saturating_add(1);
    }
}

fn inline_value(ids: &InlineIdMaps, value: MirValueId) -> MirValueId {
    ids.values.get(&value).copied().unwrap_or(value)
}

fn inline_block(ids: &InlineIdMaps, block: MirBlockId) -> MirBlockId {
    ids.blocks.get(&block).copied().unwrap_or(block)
}

fn inline_local(ids: &InlineIdMaps, local: MirLocalId) -> MirLocalId {
    ids.locals.get(&local).copied().unwrap_or(local)
}

fn inline_place(ids: &InlineIdMaps, place: MirPlaceId) -> MirPlaceId {
    ids.places.get(&place).copied().unwrap_or(place)
}

fn inline_scope(ids: &InlineIdMaps, scope: MirScopeId) -> MirScopeId {
    ids.scopes.get(&scope).copied().unwrap_or(scope)
}

fn inline_context(ids: &InlineIdMaps, context: MirPanicContext) -> MirPanicContext {
    MirPanicContext {
        function: context.function,
        source_line: context.source_line,
        caret: context.caret,
        locals: context
            .locals
            .into_iter()
            .map(|(name, local)| (name, inline_local(ids, local)))
            .collect(),
    }
}

fn inline_projection(projection: &MirProjection, ids: &InlineIdMaps) -> MirProjection {
    match projection {
        MirProjection::Field { field, span } => MirProjection::Field {
            field: *field,
            span: *span,
        },
        MirProjection::Index {
            kind,
            index,
            call,
            write_call,
            location,
            context,
            span,
        } => MirProjection::Index {
            kind: *kind,
            index: inline_value(ids, *index),
            call: *call,
            write_call: *write_call,
            location: *location,
            context: context.clone().map(|context| inline_context(ids, context)),
            span: *span,
        },
        MirProjection::Deref { span } => MirProjection::Deref { span: *span },
    }
}

fn inline_call_arg(
    argument: &crate::MIR::MirCallArg,
    ids: &InlineIdMaps,
    substitutions: &HashMap<String, MirType>,
) -> crate::MIR::MirCallArg {
    crate::MIR::MirCallArg {
        value: inline_value(ids, argument.value),
        place: argument.place.map(|place| inline_place(ids, place)),
        access: argument.access,
        span: argument.span,
        label: argument.label.clone(),
        source_index: argument.source_index,
        binder_slot: argument.binder_slot,
        spread: argument.spread,
        implicit_clone: argument.implicit_clone,
        shared_auto_clone: argument.shared_auto_clone,
        owned_last_use: argument.owned_last_use,
        authority_boundary: argument.authority_boundary,
        fn_coercion: argument.fn_coercion.clone().map(|coercion| crate::MIR::MirFnCoercion {
            ty: inline_type(&coercion.ty, substitutions),
            already_boxed: coercion.already_boxed,
        }),
        widen_fixed_to_list: argument.widen_fixed_to_list,
        widen_to_union: argument.widen_to_union.clone(),
        box_as_trait: argument.box_as_trait,
    }
}

fn inline_callee(
    callee: MirCallee,
    ids: &InlineIdMaps,
    substitutions: &HashMap<String, MirType>,
) -> MirCallee {
    match callee {
        MirCallee::User(function) => MirCallee::User(function),
        MirCallee::Associated { function, owner } => MirCallee::Associated {
            function,
            owner: inline_type(&owner, substitutions),
        },
        MirCallee::Method { function, owner } => MirCallee::Method {
            function,
            owner: inline_type(&owner, substitutions),
        },
        MirCallee::TraitMethod {
            method,
            trait_ref,
            receiver,
        } => MirCallee::TraitMethod {
            method,
            trait_ref,
            receiver: inline_type(&receiver, substitutions),
        },
        MirCallee::Core(call) => MirCallee::Core(call),
        MirCallee::Prelude(call) => MirCallee::Prelude(call),
        MirCallee::Foreign(foreign) => MirCallee::Foreign(foreign),
        MirCallee::Indirect(value) => MirCallee::Indirect(inline_value(ids, value)),
    }
}

fn inline_failure_carrier(
    carrier: MirFailureCarrier,
    substitutions: &HashMap<String, MirType>,
) -> MirFailureCarrier {
    match carrier {
        MirFailureCarrier::Infallible => MirFailureCarrier::Infallible,
        MirFailureCarrier::Result { success, error } => MirFailureCarrier::Result {
            success: inline_type(&success, substitutions),
            error: inline_type(&error, substitutions),
        },
        MirFailureCarrier::Optional { value } => MirFailureCarrier::Optional {
            value: inline_type(&value, substitutions),
        },
        MirFailureCarrier::Diverges { value } => MirFailureCarrier::Diverges {
            value: inline_type(&value, substitutions),
        },
    }
}

fn inline_fallibility(
    fallibility: MirCallFallibility,
    substitutions: &HashMap<String, MirType>,
) -> MirCallFallibility {
    match fallibility {
        MirCallFallibility::Infallible => MirCallFallibility::Infallible,
        MirCallFallibility::Failure(carrier) => {
            MirCallFallibility::Failure(inline_failure_carrier(carrier, substitutions))
        }
    }
}

fn inline_conversion(
    conversion: MirConversion,
    substitutions: &HashMap<String, MirType>,
) -> MirConversion {
    match conversion {
        MirConversion::Transparent => MirConversion::Transparent,
        MirConversion::NumericCast => MirConversion::NumericCast,
        MirConversion::SendFn => MirConversion::SendFn,
        MirConversion::Prelude {
            call,
            location,
            fallibility,
        } => MirConversion::Prelude {
            call,
            location,
            fallibility: inline_fallibility(fallibility, substitutions),
        },
    }
}

fn inline_operation(
    operation: &MirOperation,
    ids: &InlineIdMaps,
    substitutions: &HashMap<String, MirType>,
) -> MirOperation {
    match operation {
        MirOperation::Parameter { index, name } => MirOperation::Parameter {
            index: *index,
            name: name.clone(),
        },
        MirOperation::Capture { slot } => MirOperation::Capture { slot: *slot },
        MirOperation::Global { name } => MirOperation::Global { name: name.clone() },
        MirOperation::Phi { incoming } => MirOperation::Phi {
            incoming: incoming
                .iter()
                .map(|(block, value)| (inline_block(ids, *block), inline_value(ids, *value)))
                .collect(),
        },
        MirOperation::ReadPlace(place) => MirOperation::ReadPlace(inline_place(ids, *place)),
        MirOperation::MovePlace { place } => MirOperation::MovePlace {
            place: inline_place(ids, *place),
        },
        MirOperation::WritePlace { place, value } => MirOperation::WritePlace {
            place: inline_place(ids, *place),
            value: inline_value(ids, *value),
        },
        MirOperation::InitializeUninit { place } => MirOperation::InitializeUninit {
            place: inline_place(ids, *place),
        },
        MirOperation::Copy { value } => MirOperation::Copy {
            value: inline_value(ids, *value),
        },
        MirOperation::Move { value } => MirOperation::Move {
            value: inline_value(ids, *value),
        },
        MirOperation::Constant(constant) => MirOperation::Constant(constant.clone()),
        MirOperation::Unary { op, value } => MirOperation::Unary {
            op: *op,
            value: inline_value(ids, *value),
        },
        MirOperation::Binary {
            op,
            dispatch,
            left,
            right,
        } => MirOperation::Binary {
            op: *op,
            dispatch: dispatch.clone(),
            left: inline_value(ids, *left),
            right: inline_value(ids, *right),
        },
        MirOperation::BuildString { parts } => MirOperation::BuildString {
            parts: parts
                .iter()
                .map(|part| match part {
                    MirStringPart::Literal(value) => MirStringPart::Literal(value.clone()),
                    MirStringPart::Value(value) => MirStringPart::Value(inline_value(ids, *value)),
                })
                .collect(),
        },
        MirOperation::BuildList { values } => MirOperation::BuildList {
            values: values.iter().map(|value| inline_value(ids, *value)).collect(),
        },
        MirOperation::BuildMap { entries } => MirOperation::BuildMap {
            entries: entries
                .iter()
                .map(|(key, value)| (inline_value(ids, *key), inline_value(ids, *value)))
                .collect(),
        },
        MirOperation::EnumIs {
            subject,
            owner,
            variant,
        } => MirOperation::EnumIs {
            subject: inline_value(ids, *subject),
            owner: *owner,
            variant: variant.clone(),
        },
        MirOperation::EnumPayload {
            subject,
            owner,
            variant,
            index,
        } => MirOperation::EnumPayload {
            subject: inline_value(ids, *subject),
            owner: *owner,
            variant: variant.clone(),
            index: *index,
        },
        MirOperation::OptionIsSome { subject } => MirOperation::OptionIsSome {
            subject: inline_value(ids, *subject),
        },
        MirOperation::OptionValue { subject } => MirOperation::OptionValue {
            subject: inline_value(ids, *subject),
        },
        MirOperation::ResultIsOk { subject } => MirOperation::ResultIsOk {
            subject: inline_value(ids, *subject),
        },
        MirOperation::ResultValue { subject, ok } => MirOperation::ResultValue {
            subject: inline_value(ids, *subject),
            ok: *ok,
        },
        MirOperation::PatternCapture { matched, index } => MirOperation::PatternCapture {
            matched: inline_value(ids, *matched),
            index: *index,
        },
        MirOperation::PatternMatched { matched } => MirOperation::PatternMatched {
            matched: inline_value(ids, *matched),
        },
        MirOperation::ProjectMembers { base, members } => MirOperation::ProjectMembers {
            base: inline_value(ids, *base),
            members: members.clone(),
        },
        MirOperation::Index {
            call,
            base,
            index,
            kind,
            access,
            location,
            context,
        } => MirOperation::Index {
            call: *call,
            base: inline_value(ids, *base),
            index: inline_value(ids, *index),
            kind: *kind,
            access: *access,
            location: *location,
            context: inline_context(ids, context.clone()),
        },
        MirOperation::Slice {
            call,
            base,
            start,
            end,
            range,
            location,
        } => MirOperation::Slice {
            call: *call,
            base: inline_value(ids, *base),
            start: inline_value(ids, *start),
            end: inline_value(ids, *end),
            range: range.map(|value| inline_value(ids, value)),
            location: *location,
        },
        MirOperation::Range {
            start,
            end,
            exclusive,
        } => MirOperation::Range {
            start: inline_value(ids, *start),
            end: inline_value(ids, *end),
            exclusive: *exclusive,
        },
        MirOperation::Field { base, field } => MirOperation::Field {
            base: inline_value(ids, *base),
            field: *field,
        },
        MirOperation::Struct { type_id, fields } => MirOperation::Struct {
            type_id: *type_id,
            fields: fields
                .iter()
                .map(|(field, value)| (*field, inline_value(ids, *value)))
                .collect(),
        },
        MirOperation::Enum {
            type_id,
            variant,
            args,
        } => MirOperation::Enum {
            type_id: *type_id,
            variant: variant.clone(),
            args: args
                .iter()
                .map(|argument| MirEnumArg {
                    field: argument.field,
                    value: inline_value(ids, argument.value),
                    boxed: argument.boxed,
                })
                .collect(),
        },
        MirOperation::Tuple { type_id, fields } => MirOperation::Tuple {
            type_id: *type_id,
            fields: fields
                .iter()
                .map(|(field, value)| (*field, inline_value(ids, *value)))
                .collect(),
        },
        MirOperation::Present { value } => MirOperation::Present {
            value: inline_value(ids, *value),
        },
        MirOperation::Convert {
            value,
            parameters,
            target,
            conversion,
        } => MirOperation::Convert {
            value: inline_value(ids, *value),
            parameters: parameters
                .iter()
                .map(|parameter| inline_value(ids, *parameter))
                .collect(),
            target: inline_type(target, substitutions),
            conversion: inline_conversion(conversion.clone(), substitutions),
        },
        MirOperation::Absent => MirOperation::Absent,
        MirOperation::ResultOk { value } => MirOperation::ResultOk {
            value: inline_value(ids, *value),
        },
        MirOperation::ResultErr { value } => MirOperation::ResultErr {
            value: inline_value(ids, *value),
        },
        MirOperation::Call {
            callee,
            args,
            type_args,
        } => MirOperation::Call {
            callee: inline_callee(callee.clone(), ids, substitutions),
            args: args
                .iter()
                .map(|argument| inline_call_arg(argument, ids, substitutions))
                .collect(),
            type_args: type_args
                .iter()
                .map(|ty| inline_type(ty, substitutions))
                .collect(),
        },
        MirOperation::IndirectCall {
            callee,
            args,
            type_args,
        } => MirOperation::IndirectCall {
            callee: inline_value(ids, *callee),
            args: args
                .iter()
                .map(|argument| inline_call_arg(argument, ids, substitutions))
                .collect(),
            type_args: type_args
                .iter()
                .map(|ty| inline_type(ty, substitutions))
                .collect(),
        },
        MirOperation::Closure {
            function,
            captures,
            facts,
        } => MirOperation::Closure {
            function: *function,
            captures: captures
                .iter()
                .map(|capture| match capture {
                    crate::MIR::MirCaptureOperand::Value(value) => {
                        crate::MIR::MirCaptureOperand::Value(inline_value(ids, *value))
                    }
                    crate::MIR::MirCaptureOperand::Place(place) => {
                        crate::MIR::MirCaptureOperand::Place(inline_place(ids, *place))
                    }
                })
                .collect(),
            facts: facts.clone(),
        },
        MirOperation::PtrFromAddr { addr, element } => MirOperation::PtrFromAddr {
            addr: inline_value(ids, *addr),
            element: inline_type(element, substitutions),
        },
        MirOperation::Deref { value } => MirOperation::Deref {
            value: inline_value(ids, *value),
        },
        MirOperation::AddressOf { place, access } => MirOperation::AddressOf {
            place: inline_place(ids, *place),
            access: *access,
        },
        MirOperation::RawAddressOf { place } => MirOperation::RawAddressOf {
            place: inline_place(ids, *place),
        },
        MirOperation::CoreCall {
            call,
            route,
            args,
            type_args,
            fallibility,
            data_plan,
        } => MirOperation::CoreCall {
            call: *call,
            route: *route,
            args: args
                .iter()
                .map(|argument| inline_call_arg(argument, ids, substitutions))
                .collect(),
            type_args: type_args
                .iter()
                .map(|ty| inline_type(ty, substitutions))
                .collect(),
            fallibility: inline_fallibility(fallibility.clone(), substitutions),
            data_plan: data_plan.clone(),
        },
        MirOperation::AttachTag { value, tag } => MirOperation::AttachTag {
            value: inline_value(ids, *value),
            tag: tag.clone(),
        },
        MirOperation::Todo {
            call,
            location,
            expected_type,
        } => MirOperation::Todo {
            call: *call,
            location: *location,
            expected_type: expected_type
                .as_ref()
                .map(|ty| inline_type(ty, substitutions)),
        },
        MirOperation::Never { reason } => MirOperation::Never {
            reason: reason.clone(),
        },
        MirOperation::Semantic(semantic) => {
            MirOperation::Semantic(inline_semantic(semantic, ids, substitutions))
        }
        MirOperation::LoopRangeInit {
            call,
            start,
            end,
            step,
            exclusive,
        } => MirOperation::LoopRangeInit {
            call: *call,
            start: inline_value(ids, *start),
            end: inline_value(ids, *end),
            step: step.map(|value| inline_value(ids, value)),
            exclusive: *exclusive,
        },
        MirOperation::LoopRangeHasNext { call, cursor } => MirOperation::LoopRangeHasNext {
            call: *call,
            cursor: inline_value(ids, *cursor),
        },
        MirOperation::LoopRangeValue { call, cursor } => MirOperation::LoopRangeValue {
            call: *call,
            cursor: inline_value(ids, *cursor),
        },
        MirOperation::LoopRangeAdvance { call, cursor } => MirOperation::LoopRangeAdvance {
            call: *call,
            cursor: inline_value(ids, *cursor),
        },
        MirOperation::LoopIterInit {
            call,
            collection,
            step,
            by_value,
            source_kind,
        } => MirOperation::LoopIterInit {
            call: *call,
            collection: inline_value(ids, *collection),
            step: step.map(|value| inline_value(ids, value)),
            by_value: *by_value,
            source_kind: source_kind.clone(),
        },
        MirOperation::LoopIterHasNext { call, cursor } => MirOperation::LoopIterHasNext {
            call: *call,
            cursor: inline_value(ids, *cursor),
        },
        MirOperation::LoopIterValue { call, cursor } => MirOperation::LoopIterValue {
            call: *call,
            cursor: inline_value(ids, *cursor),
        },
        MirOperation::LoopIterAdvance { call, cursor } => MirOperation::LoopIterAdvance {
            call: *call,
            cursor: inline_value(ids, *cursor),
        },
        MirOperation::ScopeEnter { scope, test_member } => MirOperation::ScopeEnter {
            scope: inline_scope(ids, *scope),
            test_member: test_member.as_ref().map(|member| match member {
                MirTestScopeMember::ExpectFail { expected_code } => {
                    MirTestScopeMember::ExpectFail {
                        expected_code: expected_code.clone(),
                    }
                }
                MirTestScopeMember::Timeout { duration } => MirTestScopeMember::Timeout {
                    duration: inline_value(ids, *duration),
                },
                MirTestScopeMember::Skip { whole_test } => {
                    MirTestScopeMember::Skip {
                        whole_test: *whole_test,
                    }
                }
                MirTestScopeMember::Measure => MirTestScopeMember::Measure,
            }),
        },
        MirOperation::ScopeExit { scope } => MirOperation::ScopeExit {
            scope: inline_scope(ids, *scope),
        },
        MirOperation::Drop { value, kind } => MirOperation::Drop {
            value: inline_value(ids, *value),
            kind: *kind,
        },
    }
}

fn inline_semantic(
    semantic: &MirSemanticOp,
    ids: &InlineIdMaps,
    substitutions: &HashMap<String, MirType>,
) -> MirSemanticOp {
    match semantic {
        MirSemanticOp::DataEntriesToMap { call, local } => MirSemanticOp::DataEntriesToMap {
            call: *call,
            local: inline_local(ids, *local),
        },
        MirSemanticOp::MathBuiltin { type_id, call, args } => MirSemanticOp::MathBuiltin {
            type_id: *type_id,
            call: *call,
            args: args.iter().map(|value| inline_value(ids, *value)).collect(),
        },
        MirSemanticOp::PreciseBuiltin { type_id, call, args } => {
            MirSemanticOp::PreciseBuiltin {
                type_id: *type_id,
                call: *call,
                args: args.iter().map(|value| inline_value(ids, *value)).collect(),
            }
        }
        MirSemanticOp::Print { call, value } => MirSemanticOp::Print {
            call: *call,
            value: inline_value(ids, *value),
        },
        MirSemanticOp::AmbientInput { call, prompt } => MirSemanticOp::AmbientInput {
            call: *call,
            prompt: prompt.map(|value| inline_value(ids, value)),
        },
        MirSemanticOp::RequireStop {
            call,
            kind,
            condition,
            location,
            context,
            values,
            always_stops,
        } => MirSemanticOp::RequireStop {
            call: *call,
            kind: *kind,
            condition: condition.map(|value| inline_value(ids, value)),
            location: *location,
            context: inline_context(ids, context.clone()),
            values: values.iter().map(|value| inline_value(ids, *value)).collect(),
            always_stops: *always_stops,
        },
        MirSemanticOp::LayoutCompare {
            call,
            op,
            left,
            right,
        } => MirSemanticOp::LayoutCompare {
            call: *call,
            op: *op,
            left: inline_value(ids, *left),
            right: inline_value(ids, *right),
        },
        MirSemanticOp::LayoutLiteral { inner } => MirSemanticOp::LayoutLiteral {
            inner: inline_value(ids, *inner),
        },
        MirSemanticOp::StructLiteral {
            type_id,
            fields,
            extra,
            trait_coercion,
            boxed_fields,
        } => MirSemanticOp::StructLiteral {
            type_id: *type_id,
            fields: fields
                .iter()
                .map(|(field, value)| (*field, inline_value(ids, *value)))
                .collect(),
            extra: extra.clone(),
            trait_coercion: *trait_coercion,
            boxed_fields: boxed_fields.clone(),
        },
        MirSemanticOp::CellGuardProject {
            map_call,
            split_call,
            guard,
            paths,
            editable,
            edit_paths_disjoint,
        } => MirSemanticOp::CellGuardProject {
            map_call: *map_call,
            split_call: *split_call,
            guard: inline_value(ids, *guard),
            paths: paths.clone(),
            editable: *editable,
            edit_paths_disjoint: *edit_paths_disjoint,
        },
        MirSemanticOp::SharedGuardMap {
            call,
            guard,
            path,
            editable,
        } => MirSemanticOp::SharedGuardMap {
            call: *call,
            guard: inline_value(ids, *guard),
            path: path.clone(),
            editable: *editable,
        },
        MirSemanticOp::SharedGuardSplit {
            call,
            map_call,
            guard,
            first,
            second,
            editable,
        } => MirSemanticOp::SharedGuardSplit {
            call: *call,
            map_call: *map_call,
            guard: inline_value(ids, *guard),
            first: first.clone(),
            second: second.clone(),
            editable: *editable,
        },
        MirSemanticOp::SharedGuardWait {
            call,
            guard,
            condition,
            predicate,
        } => MirSemanticOp::SharedGuardWait {
            call: *call,
            guard: inline_value(ids, *guard),
            condition: inline_value(ids, *condition),
            predicate: inline_value(ids, *predicate),
        },
        MirSemanticOp::ConditionNotify { call, condition, all } => {
            MirSemanticOp::ConditionNotify {
                call: *call,
                condition: inline_value(ids, *condition),
                all: *all,
            }
        }
        MirSemanticOp::AllocNew { call, kind, args } => MirSemanticOp::AllocNew {
            call: *call,
            kind: *kind,
            args: args
                .iter()
                .map(|argument| inline_call_arg(argument, ids, substitutions))
                .collect(),
        },
        MirSemanticOp::ColumnarRead {
            base,
            index,
            column,
            column_index,
            accessor,
        } => MirSemanticOp::ColumnarRead {
            base: inline_value(ids, *base),
            index: inline_value(ids, *index),
            column: column.clone(),
            column_index: *column_index,
            accessor: *accessor,
        },
        MirSemanticOp::StaticPreludeCall {
            call,
            args,
            owner_type_args,
            type_args,
        } => MirSemanticOp::StaticPreludeCall {
            call: *call,
            args: args
                .iter()
                .map(|argument| inline_call_arg(argument, ids, substitutions))
                .collect(),
            owner_type_args: owner_type_args
                .iter()
                .map(|type_arg| match type_arg {
                    crate::MIR::MirPreludeTypeArg::Type(ty) => {
                        crate::MIR::MirPreludeTypeArg::Type(inline_type(ty, substitutions))
                    }
                    crate::MIR::MirPreludeTypeArg::HostUsize => {
                        crate::MIR::MirPreludeTypeArg::HostUsize
                    }
                })
                .collect(),
            type_args: type_args
                .iter()
                .map(|ty| inline_type(ty, substitutions))
                .collect(),
        },
        MirSemanticOp::DecodeUnder {
            call,
            segment,
            inner,
        } => MirSemanticOp::DecodeUnder {
            call: *call,
            segment: inline_value(ids, *segment),
            inner: inline_value(ids, *inner),
        },
        MirSemanticOp::HardwareCall {
            call,
            op,
            receiver,
            args,
        } => MirSemanticOp::HardwareCall {
            call: *call,
            op: inline_hardware_op(op, substitutions),
            receiver: receiver.map(|value| inline_value(ids, value)),
            args: args
                .iter()
                .map(|argument| inline_call_arg(argument, ids, substitutions))
                .collect(),
        },
        MirSemanticOp::BuiltinMethod {
            call,
            receiver,
            receiver_place,
            args,
            aggregate_fields,
        } => MirSemanticOp::BuiltinMethod {
            call: *call,
            receiver: inline_value(ids, *receiver),
            receiver_place: receiver_place.map(|place| inline_place(ids, place)),
            args: args.iter().map(|value| inline_value(ids, *value)).collect(),
            aggregate_fields: aggregate_fields.clone(),
        },
        MirSemanticOp::OptionLift2 {
            call,
            function,
            left,
            right,
        } => MirSemanticOp::OptionLift2 {
            call: *call,
            function: inline_value(ids, *function),
            left: inline_value(ids, *left),
            right: inline_value(ids, *right),
        },
        MirSemanticOp::ClosureMethod {
            receiver,
            args,
            call,
        } => MirSemanticOp::ClosureMethod {
            receiver: inline_value(ids, *receiver),
            args: args
                .iter()
                .map(|argument| inline_call_arg(argument, ids, substitutions))
                .collect(),
            call: *call,
        },
        MirSemanticOp::HostBorrowCallback { callable, params } => {
            MirSemanticOp::HostBorrowCallback {
                callable: inline_value(ids, *callable),
                params: params
                    .iter()
                    .map(|ty| inline_type(ty, substitutions))
                    .collect(),
            }
        }
        MirSemanticOp::TextPatternMatch {
            call,
            subject,
            parts,
        } => MirSemanticOp::TextPatternMatch {
            call: *call,
            subject: inline_value(ids, *subject),
            parts: parts.clone(),
        },
        MirSemanticOp::BinaryPatternMatch {
            call,
            subject,
            parts,
        } => MirSemanticOp::BinaryPatternMatch {
            call: *call,
            subject: inline_value(ids, *subject),
            parts: parts.clone(),
        },
        MirSemanticOp::NumericMethod { call, receiver } => MirSemanticOp::NumericMethod {
            call: *call,
            receiver: inline_value(ids, *receiver),
        },
        MirSemanticOp::NumericBinaryMethod {
            call,
            receiver,
            argument,
        } => MirSemanticOp::NumericBinaryMethod {
            call: *call,
            receiver: inline_value(ids, *receiver),
            argument: inline_value(ids, *argument),
        },
        MirSemanticOp::OverflowOption {
            call,
            left,
            right,
            location,
        } => MirSemanticOp::OverflowOption {
            call: *call,
            left: inline_value(ids, *left),
            right: inline_value(ids, *right),
            location: *location,
        },
        MirSemanticOp::HandleMethod {
            call,
            receiver,
            args,
            frame_schedule,
            frame_schedule_derivation,
        } => MirSemanticOp::HandleMethod {
            call: *call,
            receiver: inline_value(ids, *receiver),
            args: args.iter().map(|value| inline_value(ids, *value)).collect(),
            frame_schedule: frame_schedule.clone(),
            frame_schedule_derivation: frame_schedule_derivation.clone(),
        },
        MirSemanticOp::PluginInvoke {
            call,
            handle,
            export_name,
            signature,
            args,
        } => MirSemanticOp::PluginInvoke {
            call: *call,
            handle: inline_value(ids, *handle),
            export_name: export_name.clone(),
            signature: signature.clone(),
            args: args.iter().map(|value| inline_value(ids, *value)).collect(),
        },
        MirSemanticOp::HttpRouterRegister {
            call,
            receiver,
            path,
            handler,
            method,
            handler_param_names,
            contract_json,
            location,
        } => MirSemanticOp::HttpRouterRegister {
            call: *call,
            receiver: inline_value(ids, *receiver),
            path: inline_value(ids, *path),
            handler: inline_value(ids, *handler),
            method: method.clone(),
            handler_param_names: handler_param_names.clone(),
            contract_json: contract_json.clone(),
            location: *location,
        },
        MirSemanticOp::CoreClosureCall {
            call,
            kind,
            values,
            closure,
            site,
            label,
        } => MirSemanticOp::CoreClosureCall {
            call: *call,
            kind: kind.clone(),
            values: values.iter().map(|value| inline_value(ids, *value)).collect(),
            closure: closure.map(|value| inline_value(ids, value)),
            site: *site,
            label: label.clone(),
        },
        MirSemanticOp::TaskGroup { call, kind, tasks } => MirSemanticOp::TaskGroup {
            call: *call,
            kind: *kind,
            tasks: tasks.iter().map(|value| inline_value(ids, *value)).collect(),
        },
        MirSemanticOp::Select { call, kind, values } => MirSemanticOp::Select {
            call: *call,
            kind: *kind,
            values: values.iter().map(|value| inline_value(ids, *value)).collect(),
        },
        MirSemanticOp::PolicyFunction { policy, values } => MirSemanticOp::PolicyFunction {
            policy: *policy,
            values: values.iter().map(|value| inline_value(ids, *value)).collect(),
        },
        MirSemanticOp::InterruptFunction { interrupt, values } => {
            MirSemanticOp::InterruptFunction {
                interrupt: *interrupt,
                values: values.iter().map(|value| inline_value(ids, *value)).collect(),
            }
        }
        MirSemanticOp::CarrierFact {
            call,
            receiver,
            field,
            notes,
        } => MirSemanticOp::CarrierFact {
            call: *call,
            receiver: inline_value(ids, *receiver),
            field: *field,
            notes: *notes,
        },
        MirSemanticOp::GcEdit {
            call,
            root,
            edges,
            edit,
            index,
            kind,
            site,
        } => MirSemanticOp::GcEdit {
            call: *call,
            root: inline_value(ids, *root),
            edges: edges.iter().map(|value| inline_value(ids, *value)).collect(),
            edit: inline_value(ids, *edit),
            index: index.map(|value| inline_value(ids, value)),
            kind: *kind,
            site: *site,
        },
        MirSemanticOp::TypedTextInterp {
            call,
            kind,
            literals,
            holes,
        } => MirSemanticOp::TypedTextInterp {
            call: *call,
            kind: kind.clone(),
            literals: literals.clone(),
            holes: holes.iter().map(|value| inline_value(ids, *value)).collect(),
        },
        MirSemanticOp::CCallback {
            call,
            callback,
            lambda,
        } => MirSemanticOp::CCallback {
            call: *call,
            callback: *callback,
            lambda: inline_value(ids, *lambda),
        },
        MirSemanticOp::HostCall { call, args } => MirSemanticOp::HostCall {
            call: *call,
            args: args
                .iter()
                .map(|argument| inline_call_arg(argument, ids, substitutions))
                .collect(),
        },
    }
}

fn inline_hardware_op(
    operation: &crate::MIR::MirHardwareOp,
    substitutions: &HashMap<String, MirType>,
) -> crate::MIR::MirHardwareOp {
    match operation {
        crate::MIR::MirHardwareOp::RegisterRead {
            profile_id,
            block,
            register,
            width,
        } => crate::MIR::MirHardwareOp::RegisterRead {
            profile_id: profile_id.clone(),
            block: block.clone(),
            register: register.clone(),
            width: *width,
        },
        crate::MIR::MirHardwareOp::RegisterWrite {
            profile_id,
            block,
            register,
            width,
        } => crate::MIR::MirHardwareOp::RegisterWrite {
            profile_id: profile_id.clone(),
            block: block.clone(),
            register: register.clone(),
            width: *width,
        },
        crate::MIR::MirHardwareOp::DmaStart {
            profile_id,
            channel,
            buffer_ty,
        } => crate::MIR::MirHardwareOp::DmaStart {
            profile_id: profile_id.clone(),
            channel: channel.clone(),
            buffer_ty: inline_type(buffer_ty, substitutions),
        },
        crate::MIR::MirHardwareOp::DmaWait {
            profile_id,
            channel,
            buffer_ty,
        } => crate::MIR::MirHardwareOp::DmaWait {
            profile_id: profile_id.clone(),
            channel: channel.clone(),
            buffer_ty: inline_type(buffer_ty, substitutions),
        },
    }
}

fn inline_terminator(terminator: &MirTerminator, ids: &InlineIdMaps) -> MirTerminator {
    match terminator {
        MirTerminator::Jump { target } => MirTerminator::Jump {
            target: inline_block(ids, *target),
        },
        MirTerminator::Branch {
            condition,
            then_target,
            else_target,
        } => MirTerminator::Branch {
            condition: inline_value(ids, *condition),
            then_target: inline_block(ids, *then_target),
            else_target: inline_block(ids, *else_target),
        },
        MirTerminator::Switch {
            subject,
            arms,
            otherwise,
        } => MirTerminator::Switch {
            subject: inline_value(ids, *subject),
            arms: arms
                .iter()
                .map(|arm| MirSwitchArm {
                    condition: inline_value(ids, arm.condition),
                    target: inline_block(ids, arm.target),
                    span: arm.span,
                })
                .collect(),
            otherwise: inline_block(ids, *otherwise),
        },
        MirTerminator::Return { value } => MirTerminator::Return {
            value: value.map(|value| inline_value(ids, value)),
        },
        MirTerminator::Yield { value, resume } => MirTerminator::Yield {
            value: inline_value(ids, *value),
            resume: inline_block(ids, *resume),
        },
        MirTerminator::Break { target, value } => MirTerminator::Break {
            target: inline_block(ids, *target),
            value: value.map(|value| inline_value(ids, value)),
        },
        MirTerminator::Continue { target } => MirTerminator::Continue {
            target: inline_block(ids, *target),
        },
        MirTerminator::Unreachable { reason } => MirTerminator::Unreachable {
            reason: reason.clone(),
        },
    }
}

fn inline_type(ty: &MirType, substitutions: &HashMap<String, MirType>) -> MirType {
    if let MirTypeKind::Apply { name, args } = &ty.kind {
        if args.is_empty() {
            if let Some(replacement) = substitutions.get(&name.name) {
                return replacement.clone();
            }
        }
    }
    let kind = match &ty.kind {
        MirTypeKind::List(inner) => {
            MirTypeKind::List(Box::new(inline_type(inner, substitutions)))
        }
        MirTypeKind::Map { key, value } => MirTypeKind::Map {
            key: Box::new(inline_type(key, substitutions)),
            value: Box::new(inline_type(value, substitutions)),
        },
        MirTypeKind::Shared(inner) => {
            MirTypeKind::Shared(Box::new(inline_type(inner, substitutions)))
        }
        MirTypeKind::Option(inner) => {
            MirTypeKind::Option(Box::new(inline_type(inner, substitutions)))
        }
        MirTypeKind::Result { ok, err } => MirTypeKind::Result {
            ok: Box::new(inline_type(ok, substitutions)),
            err: Box::new(inline_type(err, substitutions)),
        },
        MirTypeKind::Fn(signature) => {
            let mut signature = signature.clone();
            signature.params = signature
                .params
                .iter()
                .map(|ty| inline_type(ty, substitutions))
                .collect();
            signature.ret = signature
                .ret
                .as_ref()
                .map(|ty| Box::new(inline_type(ty, substitutions)));
            MirTypeKind::Fn(signature)
        }
        MirTypeKind::SendFn { params, ret } => MirTypeKind::SendFn {

            params: params
                .iter()
                .map(|ty| inline_type(ty, substitutions))
                .collect(),
            ret: ret
                .as_ref()
                .map(|ty| Box::new(inline_type(ty, substitutions))),
        },
        MirTypeKind::Apply { name, args } => MirTypeKind::Apply {
            name: name.clone(),
            args: args
                .iter()
                .map(|ty| inline_type(ty, substitutions))
                .collect(),
        },
        MirTypeKind::Tuple(fields) => MirTypeKind::Tuple(
            fields
                .iter()
                .map(|(name, ty)| (name.clone(), inline_type(ty, substitutions)))
                .collect(),
        ),
        MirTypeKind::FixedList { elem, len } => MirTypeKind::FixedList {
            elem: Box::new(inline_type(elem, substitutions)),
            len: len.clone(),
        },
        MirTypeKind::InlineRange { base, lo, hi } => MirTypeKind::InlineRange {
            base: Box::new(inline_type(base, substitutions)),
            lo: *lo,
            hi: *hi,
        },
        MirTypeKind::Tagged { marker, inner } => MirTypeKind::Tagged {
            marker: marker.clone(),
            inner: Box::new(inline_type(inner, substitutions)),
        },
        MirTypeKind::Union(members) => MirTypeKind::Union(
            members
                .iter()
                .map(|ty| inline_type(ty, substitutions))
                .collect(),
        ),
        MirTypeKind::Quantity { base, dimension } => MirTypeKind::Quantity {
            base: Box::new(inline_type(base, substitutions)),
            dimension: dimension.clone(),
        },
        _ => ty.kind.clone(),
    };
    if kind.same_identity(&ty.kind) {
        ty.clone()
    } else {
        MirType::from_kind(kind.clone()).with_identity(MirTypeId(stable_id(
            "mir-type",
            &kind.canonical_key(),
        )))
    }
}
fn denormalize_fixed_reduction_loops(program: &mut MirProgram) {
    for function in &mut program.functions {
        let candidates = function
            .blocks
            .iter()
            .filter_map(|block| {
                let MirTerminator::Branch {
                    then_target,
                    else_target,
                    ..
                } = &block.terminator
                else {
                    return None;
                };
                normalized_fixed_reduction_shape(
                    function,
                    block.id,
                    *then_target,
                    *else_target,
                )
                .map(|shape| (block.id, shape))
            })
            .collect::<Vec<_>>();
        for (header, shape) in candidates {
            let Some(cursor) = shape.cursor else {
                continue;
            };
            let Some(advance) = shape.advance else {
                continue;
            };
            let Some(exit) = shape.exit else {
                continue;
            };
            let continuation =
                normalized_fixed_reduction_block_id(function, header, "continuation");
            let Some(continuation_block) = function
                .blocks
                .iter()
                .find(|block| block.id == continuation)
                .cloned()
            else {
                continue;
            };
            let Some(preheader) = function.blocks.iter().find(|block| {
                block.instructions.iter().any(|instruction| {
                    instruction.result == Some(cursor)
                        && matches!(&instruction.operation, MirOperation::LoopRangeInit { .. })
                })
            }) else {
                continue;
            };
            let preheader = preheader.id;
            let Some(generated_blocks) =
                normalized_fixed_reduction_generated_blocks(function, header)
            else {
                continue;
            };
            let order = crate::MIROptimization::Acceleration::D_FRED1_FIXED_ORDER;
            let mut generated_preheader_ops = HashSet::new();
            for role in [
                "constant-zero",
                "constant-seen-false",
                "constant-seen-true",
                "seed-read",
                "seed-seen",
            ] {
                generated_preheader_ops.insert(normalized_fixed_reduction_op_id(
                    function, header, role,
                ));
            }
            for lane in 0..order.lanes {
                generated_preheader_ops.insert(normalized_fixed_reduction_op_id(
                    function,
                    header,
                    &format!("seed-lane-{lane}"),
                ));
            }
            let mut generated_values = HashSet::new();
            for block in &function.blocks {
                if generated_blocks.contains(&block.id) {
                    generated_values.extend(
                        block
                            .instructions
                            .iter()
                            .filter_map(|instruction| instruction.result),
                    );
                } else if block.id == preheader {
                    generated_values.extend(
                        block
                            .instructions
                            .iter()
                            .filter(|instruction| generated_preheader_ops.contains(&instruction.id))
                            .filter_map(|instruction| instruction.result),
                    );
                }
            }
            let body_blocks = shape
                .blocks
                .iter()
                .copied()
                .filter(|block_id| *block_id != advance)
                .collect::<Vec<_>>();
            let post_entry =
                normalized_fixed_reduction_block_id(function, header, "post-entry");
            for block_id in &body_blocks {
                if let Some(block) = function.blocks.iter_mut().find(|block| block.id == *block_id) {
                    match &mut block.terminator {
                        MirTerminator::Jump { target } if *target == post_entry => {
                            *target = advance;
                        }
                        MirTerminator::Branch {
                            then_target,
                            else_target,
                            ..
                        } => {
                            if *then_target == post_entry {
                                *then_target = advance;
                            }
                            if *else_target == post_entry {
                                *else_target = advance;
                            }
                        }
                        _ => {}
                    }
                }
            }
            if let Some(block) = function.blocks.iter_mut().find(|block| block.id == header) {
                if let MirTerminator::Branch { then_target, .. } = &mut block.terminator {
                    *then_target = shape.body;
                }
            }
            if let Some(block) = function.blocks.iter_mut().find(|block| block.id == exit) {
                block.instructions = continuation_block.instructions.clone();
                block.terminator = continuation_block.terminator.clone();
            }
            if let Some(block) = function.blocks.iter_mut().find(|block| block.id == preheader) {
                block
                    .instructions
                    .retain(|instruction| !generated_preheader_ops.contains(&instruction.id));
            }
            function
                .blocks
                .retain(|block| !generated_blocks.contains(&block.id));
            let mut generated_locals = HashSet::new();
            let mut generated_places = HashSet::new();
            for lane in 0..order.lanes {
                generated_locals.insert(normalized_fixed_reduction_local_id(
                    function,
                    header,
                    &format!("lane-place-{lane}"),
                ));
                generated_places.insert(normalized_fixed_reduction_place_id(
                    function,
                    header,
                    &format!("lane-place-{lane}"),
                ));
            }
            generated_locals.insert(normalized_fixed_reduction_local_id(
                function,
                header,
                "seen-place",
            ));
            generated_places.insert(normalized_fixed_reduction_place_id(
                function,
                header,
                "seen-place",
            ));
            function
                .locals
                .retain(|local| !generated_locals.contains(&local.id));
            function
                .places
                .retain(|place| !generated_places.contains(&place.id));
            function
                .values
                .retain(|(value, ..)| !generated_values.contains(value));
            function.optimization.derived_from_digest = None;
        }
    }
}
/// may still consume the retained `fixed_reduction` source row for packing,
/// but it must not replace the semantic result with a second policy.
fn normalize_fixed_reduction_loops(program: &mut MirProgram) {
    for function in &mut program.functions {
        let mut changed = false;
        let candidates = function
            .optimization
            .vector_facts
            .iter()
            .filter(|fact| {
                matches!(&fact.decision, MirOptimizationDecision::Eligible)
                    && fact.rule == MirVectorRule::Reduction
                    && fact.fixed_reduction.is_none()
            })
            .cloned()
            .collect::<Vec<_>>();
        for fact in candidates {
            let Some(fixed) = normalize_fixed_reduction_loop(function, &fact) else {
                if let Some(row) = function
                    .optimization
                    .vector_facts
                    .iter_mut()
                    .find(|candidate| candidate.loop_header == fact.loop_header)
                {
                    row.packed = false;
                    row.decision = MirOptimizationDecision::Rejected(
                        MirOptimizationRejection::UnsupportedOperation,
                    );
                    changed = true;
                }
                continue;
            };
            if let Some(row) = function
                .optimization
                .vector_facts
                .iter_mut()
                .find(|candidate| candidate.loop_header == fact.loop_header)
            {
                row.fixed_reduction = Some(fixed);
                changed = true;
            }
        }
        refresh_loop_summaries(function);
        if changed {
            function.optimization.derived_from_digest = None;
        }
    }
}

fn normalize_fixed_reduction_loop(
    function: &mut MirFunction,
    fact: &MirVectorFact,
) -> Option<MirFixedReductionFact> {
    if fact.rule != MirVectorRule::Reduction {
        return None;
    }
    let order = crate::MIROptimization::Acceleration::D_FRED1_FIXED_ORDER;
    if !order.is_canonical() {
        return None;
    }
    if let Some(fixed) = normalized_fixed_reduction_fact(function, fact) {
        return Some(fixed);
    }
    let lanes = order.lanes;
    let cursor = fact.cursor?;
    let advance = fact.advance_block?;
    let row = function
        .optimization
        .loop_facts
        .iter()
        .find(|row| row.header == fact.loop_header)?;
    let exit = row.exit?;
    let body_entry = row.body?;
    let header = function.blocks.iter().find(|block| block.id == fact.loop_header)?;
    let (then_target, else_target) = match &header.terminator {
        MirTerminator::Branch {
            then_target,
            else_target,
            ..
        } => (*then_target, *else_target),
        _ => return None,
    };
    if then_target != body_entry || else_target != exit {
        return None;
    }
    let init = function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .find(|instruction| {
            instruction.result == Some(cursor)
                && matches!(&instruction.operation, MirOperation::LoopRangeInit { .. })
        })?;
    let MirOperation::LoopRangeInit {
        call: range_call,
        start,
        step,
        exclusive: true,
        ..
    } = init.operation.clone()
    else {
        return None;
    };
    if let Some(step) = step {
        let constants = collect_scalar_constants(function);
        if !matches!(constants.get(&step), Some(MirConstant::Int { value: 1, .. })) {
            return None;
        }
    }
    let preheader_id = function
        .blocks
        .iter()
        .find(|block| {
            block.instructions.iter().any(|instruction| {
                instruction.result == Some(cursor)
                    && matches!(&instruction.operation, MirOperation::LoopRangeInit { .. })
            })
        })
        .map(|block| block.id)?;
    let body_ids = fact.body_blocks.clone();
    if body_ids.is_empty() || body_ids.contains(&advance) || body_ids.contains(&exit) {
        return None;
    }
    let body_instructions = body_ids
        .iter()
        .filter_map(|id| function.blocks.iter().find(|block| block.id == *id))
        .flat_map(|block| block.instructions.iter())
        .collect::<Vec<_>>();
    if !vector_has_indexed_input(function, &body_instructions, cursor, None) {
        return None;
    }
    let (accumulator, addend, _) = fixed_reduction_plan(function, &body_instructions)?;
    let accumulator_ty = function
        .places
        .iter()
        .find(|place| place.id == accumulator)
        .map(|place| place.ty.clone())?;
    if !function
        .places
        .iter()
        .find(|place| place.id == accumulator)?
        .projections
        .is_empty()
        || !matches!(accumulator_ty.kind(), MirTypeKind::Float | MirTypeKind::Float32)
    {
        return None;
    }
    let condition = None;
    if !body_ids.iter().all(|id| {
        function
            .blocks
            .iter()
            .find(|block| block.id == *id)
            .is_some_and(|block| match &block.terminator {
                MirTerminator::Jump { target } => {
                    *target == advance || body_ids.contains(target)
                }
                MirTerminator::Branch {
                    then_target,
                    else_target,
                    ..
                } => [*then_target, *else_target]
                    .into_iter()
                    .all(|target| target == advance || body_ids.contains(&target)),
                _ => false,
            })
    }) {
        return None;
    }
    let source_operations = body_instructions
        .iter()
        .map(|instruction| instruction.id)
        .collect::<Vec<_>>();
    let span = header.span;
    let source_line = body_instructions.iter().find_map(|instruction| instruction.source_line);
    let value_type = |value: MirValueId| {
        function
            .values
            .iter()
            .find(|(candidate, _, _, _)| *candidate == value)
            .map(|(_, ty, _, _)| ty.clone())
    };
    let addend_ty = value_type(addend)?;
    if !matches!(
        (accumulator_ty.kind(), addend_ty.kind()),
        (MirTypeKind::Float, MirTypeKind::Float)
            | (MirTypeKind::Float32, MirTypeKind::Float32)
    ) {
        return None;
    }
    let cursor_ty = value_type(cursor)?;
    let int_ty = cursor_ty.clone();
    let bool_value = header.instructions.iter().find_map(|instruction| {
        if matches!(&instruction.operation, MirOperation::LoopRangeHasNext { .. }) {
            instruction.result
        } else {
            None
        }
    })?;
    let bool_ty = value_type(bool_value)?;
    let bool_type = MirType::from_kind(MirTypeKind::Bool);
    let base = format!("{}:{}:", function.id.0, fact.loop_header.0);
    macro_rules! ni {
        ($role:expr, $ty:expr, $operation:expr) => {
            normalized_instruction(
                function,
                &format!("{}{}", base, $role),
                span,
                source_line,
                $ty,
                $operation,
            )
        };
    }
    macro_rules! bid {
        ($role:expr) => {
            MirBlockId(stable_id(
                "mir-fixed-reduction-block",
                &format!("{}{}", base, $role),
            ))
        };
    }
    let lane_places = (0..lanes)
        .map(|lane| {
            normalized_local_place(
                function,
                &format!("{}lane-place-{lane}", base),
                span,
                accumulator_ty.clone(),
            )
            .1
        })
        .collect::<Vec<_>>();
    let seen_place = normalized_local_place(
        function,
        &format!("{}seen-place", base),
        span,
        bool_type.clone(),
    )
    .1;
    let zero = ni!(
        "constant-zero",
        Some(accumulator_ty.clone()),
        normalized_float_zero(&accumulator_ty)
    );
    let zero_value = zero.value?;
    let false_constant = ni!(
        "constant-seen-false",
        Some(bool_type.clone()),
        MirOperation::Constant(MirConstant::Bool(false))
    );
    let false_value = false_constant.value?;
    let true_constant = ni!(
        "constant-seen-true",
        Some(bool_type.clone()),
        MirOperation::Constant(MirConstant::Bool(true))
    );
    let true_value = true_constant.value?;
    let seed_read = ni!(
        "seed-read",
        Some(accumulator_ty.clone()),
        MirOperation::ReadPlace(accumulator)
    );
    let seed_value = seed_read.value?;
    let lane_index = ni!(
        "dispatch-index",
        Some(cursor_ty),
        MirOperation::LoopRangeValue {
            call: range_call,
            cursor,
        }
    );
    let lane_index_value = lane_index.value?;
    let mask = ni!(
        "dispatch-mask",
        Some(int_ty.clone()),
        MirOperation::Constant(MirConstant::Int {
            value: lanes as i64 - 1,
            width: None,
            spelling: None,
        })
    );
    let mask_value = mask.value?;
    let eight = ni!(
        "dispatch-eight",
        Some(int_ty.clone()),
        MirOperation::Constant(MirConstant::Int {
            value: lanes as i64,
            width: None,
            spelling: None,
        })
    );
    let eight_value = eight.value?;
    let index_low = ni!(
        "dispatch-index-low",
        Some(int_ty.clone()),
        MirOperation::Binary {
            op: MirBinaryOp::BitAnd,
            dispatch: crate::MIR::MirBinaryDispatch::Primitive,
            left: lane_index_value,
            right: mask_value,
        }
    );
    let index_low_value = index_low.value?;
    let start_low = ni!(
        "dispatch-start-low",
        Some(int_ty.clone()),
        MirOperation::Binary {
            op: MirBinaryOp::BitAnd,
            dispatch: crate::MIR::MirBinaryDispatch::Primitive,
            left: start,
            right: mask_value,
        }
    );
    let start_low_value = start_low.value?;
    let delta = ni!(
        "dispatch-delta",
        Some(int_ty.clone()),
        MirOperation::Binary {
            op: MirBinaryOp::Sub,
            dispatch: crate::MIR::MirBinaryDispatch::Primitive,
            left: index_low_value,
            right: start_low_value,
        }
    );
    let delta_value = delta.value?;
    let wrapped = ni!(
        "dispatch-wrapped",
        Some(int_ty.clone()),
        MirOperation::Binary {
            op: MirBinaryOp::Add,
            dispatch: crate::MIR::MirBinaryDispatch::Primitive,
            left: delta_value,
            right: eight_value,
        }
    );
    let wrapped_value = wrapped.value?;
    let ordinal = ni!(
        "dispatch-ordinal",
        Some(int_ty.clone()),
        MirOperation::Binary {
            op: MirBinaryOp::BitAnd,
            dispatch: crate::MIR::MirBinaryDispatch::Primitive,
            left: wrapped_value,
            right: mask_value,
        }
    );
    let ordinal_value = ordinal.value?;
    let mut lane_constants = Vec::new();
    for lane in 0..lanes {
        let constant = ni!(
            &format!("lane-constant-{lane}"),
            Some(int_ty.clone()),
            MirOperation::Constant(MirConstant::Int {
                value: lane as i64,
                width: None,
                spelling: None,
            })
        );
        lane_constants.push((constant.value?, constant.instruction));
    }
    let dispatch_entry = bid!("dispatch-entry");
    let post_entry = bid!("post-entry");
    let post_seen_entry = bid!("post-seen-entry");
    let post_unseen_entry = bid!("post-unseen-entry");
    let tree_block = bid!("tree");
    let continuation = bid!("continuation");
    let lane_pre = (0..lanes)
        .map(|lane| bid!(&format!("lane-pre-{lane}")))
        .collect::<Vec<_>>();
    let lane_seen_post = (0..lanes)
        .map(|lane| bid!(&format!("lane-seen-post-{lane}")))
        .collect::<Vec<_>>();
    let lane_unseen_post = (0..lanes)
        .map(|lane| bid!(&format!("lane-unseen-post-{lane}")))
        .collect::<Vec<_>>();
    let lane_dispatch = (0..lanes - 1)
        .map(|lane| bid!(&format!("lane-dispatch-{lane}")))
        .collect::<Vec<_>>();
    let lane_seen_dispatch = (0..lanes - 1)
        .map(|lane| bid!(&format!("lane-seen-dispatch-{lane}")))
        .collect::<Vec<_>>();
    let lane_unseen_dispatch = (0..lanes - 1)
        .map(|lane| bid!(&format!("lane-unseen-dispatch-{lane}")))
        .collect::<Vec<_>>();
    let preheader_index = function.blocks.iter().position(|block| block.id == preheader_id)?;
    let mut generated = Vec::new();
    let mut preheader_instructions = vec![
        zero.instruction,
        false_constant.instruction,
        true_constant.instruction,
        seed_read.instruction,
    ];
    for lane in 0..lanes {
        preheader_instructions.push(ni!(
            &format!("seed-lane-{lane}"),
            None,
            MirOperation::WritePlace {
                place: lane_places[lane],
                value: if lane == 0 { seed_value } else { zero_value },
            }
        ).instruction);
    }
    preheader_instructions.push(ni!(
        "seed-seen",
        None,
        MirOperation::WritePlace {
            place: seen_place,
            value: false_value,
        }
    ).instruction);
    function.blocks[preheader_index]
        .instructions
        .extend(preheader_instructions);
    let mut dispatch_instructions = vec![
        lane_index.instruction,
        mask.instruction,
        eight.instruction,
        index_low.instruction,
        start_low.instruction,
        delta.instruction,
        wrapped.instruction,
        ordinal.instruction,
    ];
    dispatch_instructions.extend(lane_constants.iter().map(|(_, instruction)| instruction.clone()));
    generated.push(MirBasicBlock {
        id: dispatch_entry,
        span,
        instructions: dispatch_instructions,
        terminator: MirTerminator::Jump {
            target: lane_dispatch[0],
        },
    });
    let mut make_dispatch = |dispatch_id: MirBlockId,
                         lane: usize,
                         next: MirBlockId,
                         target: MirBlockId|
     -> MirBasicBlock {
        let compare = ni!(
            &format!("dispatch-compare-{}-{lane}", dispatch_id.0),
            Some(bool_ty.clone()),
            MirOperation::Binary {
                op: MirBinaryOp::Eq,
                dispatch: crate::MIR::MirBinaryDispatch::Primitive,
                left: ordinal_value,
                right: lane_constants[lane].0,
            }
        );
        MirBasicBlock {
            id: dispatch_id,
            span,
            instructions: vec![compare.instruction],
            terminator: MirTerminator::Branch {
                condition: compare.value.expect("dispatch comparison has a result"),
                then_target: target,
                else_target: next,
            },
        }
    };
    for lane in 0..lanes.saturating_sub(1) {
        generated.push(make_dispatch(
            lane_dispatch[lane],
            lane,
            if lane + 1 == lanes - 1 {
                lane_pre[lanes - 1]
            } else {
                lane_dispatch[lane + 1]
            },
            lane_pre[lane],
        ));
        generated.push(make_dispatch(
            lane_seen_dispatch[lane],
            lane,
            if lane + 1 == lanes - 1 {
                lane_seen_post[lanes - 1]
            } else {
                lane_seen_dispatch[lane + 1]
            },
            lane_seen_post[lane],
        ));
        generated.push(make_dispatch(
            lane_unseen_dispatch[lane],
            lane,
            if lane + 1 == lanes - 1 {
                lane_unseen_post[lanes - 1]
            } else {
                lane_unseen_dispatch[lane + 1]
            },
            lane_unseen_post[lane],
        ));
    }
    drop(make_dispatch);
    for lane in 0..lanes {
        let read = ni!(
            &format!("pre-read-{lane}"),
            Some(accumulator_ty.clone()),
            MirOperation::ReadPlace(lane_places[lane])
        );
        let read_value = read.value.expect("lane pre-read has a result");
        generated.push(MirBasicBlock {
            id: lane_pre[lane],
            span,
            instructions: vec![
                read.instruction,
                ni!(
                    &format!("pre-write-{lane}"),
                    None,
                    MirOperation::WritePlace {
                        place: accumulator,
                        value: read_value,
                    }
                ).instruction,
            ],
            terminator: MirTerminator::Jump { target: body_entry },
        });
    }
    generated.push(MirBasicBlock {
        id: post_entry,
        span,
        instructions: Vec::new(),
        terminator: condition.map_or(
            MirTerminator::Jump {
                target: post_seen_entry,
            },
            |condition| MirTerminator::Branch {
                condition,
                then_target: post_seen_entry,
                else_target: post_unseen_entry,
            },
        ),
    });
    generated.push(MirBasicBlock {
        id: post_seen_entry,
        span,
        instructions: Vec::new(),
        terminator: MirTerminator::Jump {
            target: lane_seen_dispatch[0],
        },
    });
    generated.push(MirBasicBlock {
        id: post_unseen_entry,
        span,
        instructions: Vec::new(),
        terminator: MirTerminator::Jump {
            target: lane_unseen_dispatch[0],
        },
    });
    for lane in 0..lanes {
        let read = ni!(
            &format!("seen-post-read-{lane}"),
            Some(accumulator_ty.clone()),
            MirOperation::ReadPlace(accumulator)
        );
        let read_value = read.value.expect("seen post-read has a result");
        generated.push(MirBasicBlock {
            id: lane_seen_post[lane],
            span,
            instructions: vec![
                read.instruction,
                ni!(
                    &format!("seen-post-write-{lane}"),
                    None,
                    MirOperation::WritePlace {
                        place: lane_places[lane],
                        value: read_value,
                    }
                ).instruction,
                ni!(
                    &format!("seen-post-mark-{lane}"),
                    None,
                    MirOperation::WritePlace {
                        place: seen_place,
                        value: true_value,
                    }
                ).instruction,
            ],
            terminator: MirTerminator::Jump { target: advance },
        });
        let read = ni!(
            &format!("unseen-post-read-{lane}"),
            Some(accumulator_ty.clone()),
            MirOperation::ReadPlace(accumulator)
        );
        let read_value = read.value.expect("unseen post-read has a result");
        generated.push(MirBasicBlock {
            id: lane_unseen_post[lane],
            span,
            instructions: vec![
                read.instruction,
                ni!(
                    &format!("unseen-post-write-{lane}"),
                    None,
                    MirOperation::WritePlace {
                        place: lane_places[lane],
                        value: read_value,
                    }
                ).instruction,
            ],
            terminator: MirTerminator::Jump { target: advance },
        });
    }
    let mut tree_instructions = Vec::new();
    let mut lane_values = Vec::new();
    for lane in 0..lanes {
        let read = ni!(
            &format!("tree-read-{lane}"),
            Some(accumulator_ty.clone()),
            MirOperation::ReadPlace(lane_places[lane])
        );
        lane_values.push(read.value.expect("tree read has a result"));
        tree_instructions.push(read.instruction);
    }
    let mut tree_add = |role: &str, left: MirValueId, right: MirValueId| {
        let add = ni!(
            role,
            Some(accumulator_ty.clone()),
            MirOperation::Binary {
                op: MirBinaryOp::Add,
                dispatch: crate::MIR::MirBinaryDispatch::Primitive,
                left,
                right,
            }
        );
        let value = add.value.expect("tree add has a result");
        tree_instructions.push(add.instruction);
        value
    };
    let left01 = tree_add("tree-left-01", lane_values[0], lane_values[1]);
    let left23 = tree_add("tree-left-23", lane_values[2], lane_values[3]);
    let left = tree_add("tree-left", left01, left23);
    let right45 = tree_add("tree-right-45", lane_values[4], lane_values[5]);
    let right67 = tree_add("tree-right-67", lane_values[6], lane_values[7]);
    let right = tree_add("tree-right", right45, right67);
    let result = tree_add("tree-final", left, right);
    drop(tree_add);
    tree_instructions.push(ni!(
        "tree-write",
        None,
        MirOperation::WritePlace {
            place: accumulator,
            value: result,
        }
    ).instruction);
    generated.push(MirBasicBlock {
        id: tree_block,
        span,
        instructions: tree_instructions,
        terminator: MirTerminator::Jump { target: continuation },
    });
    let exit_index = function.blocks.iter().position(|block| block.id == exit)?;
    let original_exit_instructions = std::mem::take(&mut function.blocks[exit_index].instructions);
    let original_exit_terminator = std::mem::replace(
        &mut function.blocks[exit_index].terminator,
        MirTerminator::Unreachable {
            reason: "fixed reduction exit gate".to_string(),
        },
    );
    generated.push(MirBasicBlock {
        id: continuation,
        span,
        instructions: original_exit_instructions,
        terminator: original_exit_terminator,
    });
    let seen_read = ni!(
        "exit-seen-read",
        Some(bool_type),
        MirOperation::ReadPlace(seen_place)
    );
    let seen_value = seen_read.value?;
    function.blocks[exit_index].instructions = vec![seen_read.instruction];
    function.blocks[exit_index].terminator = MirTerminator::Branch {
        condition: seen_value,
        then_target: tree_block,
        else_target: continuation,
    };
    for id in &body_ids {
        let block = function.blocks.iter_mut().find(|block| block.id == *id)?;
        match &mut block.terminator {
            MirTerminator::Jump { target } if *target == advance => *target = post_entry,
            MirTerminator::Branch {
                then_target,
                else_target,
                ..
            } => {
                if *then_target == advance {
                    *then_target = post_entry;
                }
                if *else_target == advance {
                    *else_target = post_entry;
                }
            }
            _ => {}
        }
    }
    let header = function.blocks.iter_mut().find(|block| block.id == fact.loop_header)?;
    if let MirTerminator::Branch { then_target, .. } = &mut header.terminator {
        *then_target = dispatch_entry;
    } else {
        return None;
    }
    function.blocks.extend(generated);
    Some(MirFixedReductionFact {
        accumulator,
        addend,
        seed: seed_value,
        condition,
        source_operations,
        exit: continuation,
        order,
    })
}

fn fixed_reduction_plan(
    function: &MirFunction,
    instructions: &[&MirInstruction],
) -> Option<(MirPlaceId, MirValueId, MirValueId)> {
    let plans = instructions
        .iter()
        .filter_map(|instruction| {
            let MirOperation::WritePlace { place, value } = &instruction.operation else {
                return None;
            };
            let defining = instructions.iter().find(|candidate| candidate.result == Some(*value))?;
            let MirOperation::Binary {
                op: MirBinaryOp::Add,
                left,
                right,
                ..
            } = &defining.operation
            else {
                return None;
            };
            let is_read = |candidate: MirValueId| {
                instructions.iter().any(|instruction| {
                    instruction.result == Some(candidate)
                        && matches!(
                            &instruction.operation,
                            MirOperation::ReadPlace(read) if *read == *place
                        )
                })
            };
            let (seed, addend) = if is_read(*left) {
                (*left, *right)
            } else if is_read(*right) {
                (*right, *left)
            } else {
                return None;
            };
            Some((*place, addend, seed))
        })
        .collect::<Vec<_>>();
    let _ = function;
    if plans.len() == 1 {
        plans.into_iter().next()
    } else {
        None
    }
}

fn normalized_value(
    function: &mut MirFunction,
    identity: &str,
    span: Span,
    ty: MirType,
) -> MirValueId {
    let value = MirValueId(stable_id("mir-fixed-reduction-value", identity));
    if !function.values.iter().any(|(candidate, _, _, _)| *candidate == value) {
        function
            .values
            .push((value, ty, span, MirOwnership::copy()));
    }
    value
}

struct NormalizedInstruction {
    instruction: MirInstruction,
    value: Option<MirValueId>,
}


fn normalized_instruction(
    function: &mut MirFunction,
    identity: &str,
    span: Span,
    source_line: Option<u32>,
    ty: Option<MirType>,
    operation: MirOperation,
) -> NormalizedInstruction {
    let value = ty.clone().map(|ty| normalized_value(function, identity, span, ty));
    let op_id = MirOpId(stable_id("mir-fixed-reduction-op", identity));
    NormalizedInstruction {
        instruction: MirInstruction {
            id: op_id,
            span,
            source_line,
            result: value,
            ty,
            operation,
        },
        value,
    }
}

fn normalized_local_place(
    function: &mut MirFunction,
    identity: &str,
    span: Span,
    ty: MirType,
) -> (MirLocalId, MirPlaceId) {
    let local = MirLocalId(stable_id("mir-fixed-reduction-local", identity));
    let place = MirPlaceId(stable_id("mir-fixed-reduction-place", identity));
    function.locals.push(MirLocal {
        id: local,
        name: format!("__jet_dfred_{identity}"),
        span,
        ty: ty.clone(),
        place,
        mutable: true,
        ownership: MirOwnership::copy(),
        comptime: false,
        uninit: false,
        arena_view: false,
        string_view: false,
        gc_root: false,
    });
    function.places.push(MirPlace {
        id: place,
        span,
        ty,
        base: MirPlaceBase::Local(local),
        projections: Vec::new(),
        access: MirAccess::Write,
        persist_key: None,
    });
    (local, place)
}

fn normalized_float_zero(ty: &MirType) -> MirOperation {
    MirOperation::Constant(MirConstant::Float {
        value: 0.0,
        f32: matches!(ty.kind(), MirTypeKind::Float32),
        spelling: Some("0.0".to_string()),
    })
}


fn optimized_pass_order_complete(program: &MirProgram, input_digest: &[u8; 32]) -> bool {
    program
        .functions
        .iter()
        .all(|function| {
            function.optimization.pass_ids.as_slice() == MIR_OPTIMIZATION_PASS_ORDER.as_slice()
                && function.optimization.derived_from_digest.as_ref() == Some(input_digest)
        })
}

fn validate_after(program: &MirProgram, pass: MirOptimizationPassId) -> Result<(), MirOptimizationError> {
    verify_mir_legality(program).map_err(|error| MirOptimizationError::Legality { pass, error })
}


fn seal_derived_facts(program: &mut MirProgram) {
    let input_digest = mir_program_digest(program);
    for function in &mut program.functions {
        function.optimization.derived_from_digest = Some(input_digest);
    }
}

fn clear_derived_facts(program: &mut MirProgram) {
    for function in &mut program.functions {
        function.optimization.clear_derived();
    }
}

fn mark_pass(program: &mut MirProgram, pass: MirOptimizationPassId) {
    let before = CanonicalPass::enabled().then(|| canonical_payload(program));
    let before_identity = CanonicalPass::enabled().then(|| canonical_identity(program));
    for function in &mut program.functions {
        if function.optimization.pass_ids.last().copied() != Some(pass) {
            function.optimization.pass_ids.push(pass);
        }
    }
    if let (Some(before), Some(before_identity)) = (before, before_identity) {
        CanonicalPass::record(
            "optimization",
            "mir.pass-order-facts",
            "crates/jet-foundation/src/MIR.rs",
            "mir",
            before,
            before_identity,
            "mir",
            canonical_payload(program),
            canonical_identity(program),
            "checked",
        );
    }
}

fn eliminate_unreachable_blocks(program: &mut MirProgram) {
    for function in &mut program.functions {
        let reachable = reachable_blocks(function);
        let kept: HashSet<_> = reachable.iter().copied().collect();
        function.blocks.retain(|block| kept.contains(&block.id));
        let live_values: HashSet<_> = function
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter().filter_map(|instruction| instruction.result))
            .collect();
        function.values.retain(|(value, _, _, _)| live_values.contains(value));
        let predecessors = predecessor_map(function);
        for block in &mut function.blocks {
            let expected = predecessors.get(&block.id).cloned().unwrap_or_default();
            for instruction in &mut block.instructions {
                if let MirOperation::Phi { incoming } = &mut instruction.operation {
                    incoming.retain(|(predecessor, _)| expected.contains(predecessor));
                }
            }
        }
    }
}

fn simplify_cfg(program: &mut MirProgram) {
    for function in &mut program.functions {
        let constants = collect_scalar_constants(function);
        for block in &mut function.blocks {
            let terminator = block.terminator.clone();
            block.terminator = match terminator {
                crate::MIR::MirTerminator::Branch { condition: _, then_target, else_target } if then_target == else_target => {
                    crate::MIR::MirTerminator::Jump { target: then_target }
                }
                crate::MIR::MirTerminator::Branch { condition, then_target, else_target } => match constants.get(&condition) {
                    Some(MirConstant::Bool(true)) => crate::MIR::MirTerminator::Jump { target: then_target },
                    Some(MirConstant::Bool(false)) => crate::MIR::MirTerminator::Jump { target: else_target },
                    _ => crate::MIR::MirTerminator::Branch { condition, then_target, else_target },
                },
                other => other,
            };
        }
    }
    eliminate_unreachable_blocks(program);
}

fn fold_exact_constants(program: &mut MirProgram) {
    for function in &mut program.functions {
        let limit = function.blocks.iter().map(|block| block.instructions.len()).sum::<usize>().saturating_add(1);
        for _ in 0..limit {
            let mut constants = collect_scalar_constants(function);
            let mut changed = false;
            for block in &mut function.blocks {
                for instruction in &mut block.instructions {
                    let replacement = match &instruction.operation {
                        MirOperation::Phi { incoming } => fold_phi(incoming, &constants),
                        MirOperation::Unary { op, value } => constants.get(value).and_then(|constant| fold_unary(*op, constant, instruction.ty.as_ref()?)),
                        MirOperation::Binary {
                            op,
                            dispatch: crate::MIR::MirBinaryDispatch::Primitive,
                            left,
                            right,
                        } => constants
                            .get(left)
                            .zip(constants.get(right))
                            .and_then(|(left, right)| fold_binary(*op, left, right, instruction.ty.as_ref()?)),
                        MirOperation::Binary { .. } => None,
                        _ => None,
                    };
                    if let Some(constant) = replacement {
                        instruction.operation = MirOperation::Constant(constant.clone());
                        if let Some(result) = instruction.result {
                            constants.insert(result, constant);
                        }
                        changed = true;
                    } else if let (Some(result), MirOperation::Constant(constant)) = (instruction.result, &instruction.operation) {
                        if let Some(scalar) = scalar_constant(constant) {
                            constants.insert(result, scalar);
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }
    simplify_cfg(program);
}

fn collect_scalar_constants(function: &MirFunction) -> HashMap<MirValueId, MirConstant> {
    let mut constants = HashMap::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            if let Some(result) = instruction.result {
                if let MirOperation::Constant(constant) = &instruction.operation {
                    if let Some(scalar) = scalar_constant(constant) {
                        constants.insert(result, scalar);
                    }
                }
                if let MirOperation::Phi { incoming } = &instruction.operation {
                    if let Some(constant) = fold_phi(incoming, &constants) {
                        constants.insert(result, constant);
                    }
                }
            }
        }
    }
    constants
}

fn fold_phi(incoming: &[(MirBlockId, MirValueId)], constants: &HashMap<MirValueId, MirConstant>) -> Option<MirConstant> {
    // A join folds only when every edge carries the same constant.  Skipping
    // non-constant edges folded `if n < 0 -> 1 else if … -> 2 else -> 3` to
    // `1` because the inner join was not a constant and was ignored.
    let mut iter = incoming.iter().map(|(_, value)| constants.get(value));
    let first = iter.next()??.clone();
    if iter.all(|candidate| candidate.is_some_and(|candidate| same_scalar(&first, candidate))) {
        Some(first)
    } else {
        None
    }
}

fn scalar_constant(constant: &MirConstant) -> Option<MirConstant> {
    match constant {
        MirConstant::Int {
            value, width, ..
        } => Some(MirConstant::Int {
            value: *value,
            width: *width,
            spelling: None,
        }),
        MirConstant::Bool(value) => Some(MirConstant::Bool(*value)),
        MirConstant::Char(value) => Some(MirConstant::Char(*value)),
        _ => None,
    }
}

fn same_scalar(left: &MirConstant, right: &MirConstant) -> bool {
    match (left, right) {
        (MirConstant::Int { value: left, width: left_width, .. }, MirConstant::Int { value: right, width: right_width, .. }) => left == right && left_width == right_width,
        (MirConstant::Bool(left), MirConstant::Bool(right)) => left == right,
        (MirConstant::Char(left), MirConstant::Char(right)) => left == right,
        _ => false,
    }
}

fn fold_unary(op: MirUnaryOp, value: &MirConstant, ty: &MirType) -> Option<MirConstant> {
    match (op, value) {
        (MirUnaryOp::Not, MirConstant::Bool(value)) => Some(MirConstant::Bool(!value)),
        (MirUnaryOp::Neg, MirConstant::Int { value, width, .. }) => {
            let result = value.checked_neg()?;
            make_integer_constant(result, ty, *width)
        }
        _ => None,
    }
}

fn fold_binary(
    op: MirBinaryOp,
    left: &MirConstant,
    right: &MirConstant,
    ty: &MirType,
) -> Option<MirConstant> {
    if let (MirConstant::Bool(left), MirConstant::Bool(right)) = (left, right) {
        return match op {
            MirBinaryOp::And => Some(MirConstant::Bool(*left && *right)),
            MirBinaryOp::Or => Some(MirConstant::Bool(*left || *right)),
            MirBinaryOp::Eq => Some(MirConstant::Bool(left == right)),
            MirBinaryOp::Ne => Some(MirConstant::Bool(left != right)),
            _ => None,
        };
    }
    if let (MirConstant::Char(left), MirConstant::Char(right)) = (left, right) {
        return match op {
            MirBinaryOp::Eq => Some(MirConstant::Bool(left == right)),
            MirBinaryOp::Ne => Some(MirConstant::Bool(left != right)),
            MirBinaryOp::Lt => Some(MirConstant::Bool(left < right)),
            MirBinaryOp::Gt => Some(MirConstant::Bool(left > right)),
            MirBinaryOp::Le => Some(MirConstant::Bool(left <= right)),
            MirBinaryOp::Ge => Some(MirConstant::Bool(left >= right)),
            _ => None,
        };
    }
    let (
        MirConstant::Int {
            value: left,
            width: left_width,
            ..
        },
        MirConstant::Int { value: right, .. },
    ) = (left, right)
    else {
        return None;
    };
    let result = match op {
        MirBinaryOp::Add => left.checked_add(*right)?,
        MirBinaryOp::Sub => left.checked_sub(*right)?,
        MirBinaryOp::Mul => left.checked_mul(*right)?,
        MirBinaryOp::Div => {
            if *right == 0 || (*left == i64::MIN && *right == -1) {
                return None;
            }
            left / right
        }
        MirBinaryOp::FloorDiv => floor_div(*left, *right)?,
        MirBinaryOp::Mod => floor_mod(*left, *right)?,
        MirBinaryOp::Rem => {
            if *right == 0 {
                return None;
            }
            left % right
        }
        MirBinaryOp::Pow => {
            if *right < 0 {
                return None;
            }
            left.checked_pow(u32::try_from(*right).ok()?)?
        }
        MirBinaryOp::BitAnd => left & right,
        MirBinaryOp::BitOr => left | right,
        MirBinaryOp::BitXor => left ^ right,
        MirBinaryOp::Shl => left.checked_shl(u32::try_from(*right).ok()?)?,
        MirBinaryOp::Shr => left.checked_shr(u32::try_from(*right).ok()?)?,
        MirBinaryOp::Eq => return Some(MirConstant::Bool(left == right)),
        MirBinaryOp::Ne => return Some(MirConstant::Bool(left != right)),
        MirBinaryOp::Lt => return Some(MirConstant::Bool(left < right)),
        MirBinaryOp::Gt => return Some(MirConstant::Bool(left > right)),
        MirBinaryOp::Le => return Some(MirConstant::Bool(left <= right)),
        MirBinaryOp::Ge => return Some(MirConstant::Bool(left >= right)),
        MirBinaryOp::And | MirBinaryOp::Or | MirBinaryOp::Compare => return None,
    };
    make_integer_constant(result, ty, *left_width)
}
fn floor_div(left: i64, right: i64) -> Option<i64> {
    if right == 0 || (left == i64::MIN && right == -1) { return None; }
    let quotient = left / right;
    let remainder = left % right;
    if remainder != 0 && (remainder < 0) != (right < 0) {
        quotient.checked_sub(1)
    } else {
        Some(quotient)
    }
}

fn floor_mod(left: i64, right: i64) -> Option<i64> {
    if right == 0 || (left == i64::MIN && right == -1) { return None; }
    let remainder = left % right;
    if remainder != 0 && (remainder < 0) != (right < 0) {
        remainder.checked_add(right)
    } else {
        Some(remainder)
    }
}

fn make_integer_constant(value: i64, ty: &MirType, fallback: Option<(bool, u8)>) -> Option<MirConstant> {
    let width = ty.fixed_int().or(fallback);
    if let Some((signed, bits)) = width {
        if !fits_integer(value, signed, bits) {
            return None;
        }
    }
    Some(MirConstant::Int { value, width, spelling: None })
}

fn fits_integer(value: i64, signed: bool, bits: u8) -> bool {
    if bits == 0 || bits > 64 { return false; }
    if signed {
        let min = -(1i128 << (u32::from(bits) - 1));
        let max = (1i128 << (u32::from(bits) - 1)) - 1;
        i128::from(value) >= min && i128::from(value) <= max
    } else {
        value >= 0 && (bits == 64 || i128::from(value) <= (1i128 << u32::from(bits)) - 1)
    }
}

fn derive_bounds_facts(program: &mut MirProgram) {
    for function in &mut program.functions {
        function.optimization.bounds_facts.clear();
        for block in &function.blocks {
            for instruction in &block.instructions {
                if let MirOperation::Index { index, kind, .. } = &instruction.operation {
                    function.optimization.bounds_facts.push(MirBoundsFact {
                        operation: instruction.id,
                        place: None,
                        index: Some(*index),
                        kind: *kind,
                        proven: matches!(kind, MirIndexKind::FixedListProof),
                        elided: matches!(kind, MirIndexKind::FixedListProof),
                        failure_preserved: true,
                        span: instruction.span,
                    });
                }
                for place_id in operation_place_refs(&instruction.operation) {
                    let Some(place) = function.places.iter().find(|place| place.id == place_id) else { continue; };
                    for projection in &place.projections {
                        if let MirProjection::Index { kind, index, span, .. } = projection {
                            function.optimization.bounds_facts.push(MirBoundsFact {
                                operation: instruction.id,
                                place: Some(place_id),
                                index: Some(*index),
                                kind: *kind,
                                proven: matches!(kind, MirIndexKind::FixedListProof),
                                elided: matches!(kind, MirIndexKind::FixedListProof),
                                failure_preserved: true,
                                span: *span,
                            });
                        }
                    }
                }
            }
        }
        function.optimization.bounds_facts.sort_by_key(|fact| (fact.operation, fact.place, fact.index));
    }
}

fn eliminate_dead_pure_values(program: &mut MirProgram) {
    let prelude_calls: HashMap<_, _> = program.prelude_calls.iter().map(|call| (call.id, call)).collect();
    for function in &mut program.functions {
        loop {
            let uses = value_use_counts(function);
            let metadata: HashMap<_, _> = function.values.iter().map(|(value, _, _, ownership)| (*value, *ownership)).collect();
            let mut removed = HashSet::new();
            for block in &mut function.blocks {
                block.instructions.retain(|instruction| {
                    let Some(result) = instruction.result else { return true; };
                    if uses.get(&result).copied().unwrap_or(0) != 0 {
                        return true;
                    }
                    let Some(ownership) = metadata.get(&result).copied() else { return true; };
                    if !removable_dead_operation(&instruction.operation, instruction.ty.as_ref(), ownership, &prelude_calls) {
                        return true;
                    }
                    removed.insert(result);
                    false
                });
            }
            if removed.is_empty() {
                break;
            }
            function.values.retain(|(value, _, _, _)| !removed.contains(value));
        }
    }
}

fn value_use_counts(function: &MirFunction) -> HashMap<MirValueId, usize> {
    let mut uses = HashMap::new();
    let mut add = |value: MirValueId| {
        *uses.entry(value).or_insert(0) += 1;
    };
    for place in &function.places {
        for value in place_value_uses(place) { add(value); }
    }
    for block in &function.blocks {
        for instruction in &block.instructions {
            for value in instruction.operation.value_uses() { add(value); }
            for place_id in operation_place_refs(&instruction.operation) {
                if let Some(place) = function.places.iter().find(|place| place.id == place_id) {
                    for value in place_value_uses(place) { add(value); }
                }
            }
        }
        for value in block.terminator.value_uses() { add(value); }
    }
    uses
}
fn removable_dead_operation(
    operation: &MirOperation,
    ty: Option<&MirType>,
    ownership: MirOwnership,
    prelude_calls: &HashMap<MirPreludeCallId, &MirPreludeCall>,
) -> bool {
    if ownership.mode != MirOwnershipMode::Copy || ownership.drop != crate::MIR::MirDropKind::None {
        return false;
    }
    match operation {
        MirOperation::Constant(constant) => matches!(constant, MirConstant::Int { .. } | MirConstant::Float { .. } | MirConstant::Bool(_) | MirConstant::Char(_)),
        MirOperation::Unary { op: MirUnaryOp::Not, .. } => ty.is_some_and(MirType::is_bool),
        MirOperation::Binary { op, dispatch, .. } => {
            let pure_dispatch = match dispatch {
                crate::MIR::MirBinaryDispatch::Primitive => true,
                crate::MIR::MirBinaryDispatch::Prelude { call, .. } => prelude_call_is_pure(prelude_calls, *call),
            };
            pure_dispatch
                && matches!(
                    op,
                    MirBinaryOp::Eq
                        | MirBinaryOp::Ne
                        | MirBinaryOp::Lt
                        | MirBinaryOp::Gt
                        | MirBinaryOp::Le
                        | MirBinaryOp::Ge
                        | MirBinaryOp::BitAnd
                        | MirBinaryOp::BitOr
                        | MirBinaryOp::BitXor
                        | MirBinaryOp::And
                        | MirBinaryOp::Or
                )
        }
        MirOperation::Phi { .. } => true,
        MirOperation::Convert { conversion, .. } => conversion_is_pure(conversion, prelude_calls),
        MirOperation::Semantic(operation) => semantic_operation_is_pure(operation, prelude_calls),
        _ => false,
    }
}

fn prelude_call_is_pure(
    prelude_calls: &HashMap<MirPreludeCallId, &MirPreludeCall>,
    call: MirPreludeCallId,
) -> bool {
    prelude_calls.get(&call).is_some_and(|record| {
        record.effect.is_none() && matches!(&record.fallibility, MirCallFallibility::Infallible)
    })
}
fn conversion_is_pure(
    conversion: &MirConversion,
    prelude_calls: &HashMap<MirPreludeCallId, &MirPreludeCall>,
) -> bool {
    match conversion {
        MirConversion::Transparent | MirConversion::NumericCast | MirConversion::SendFn => true,
        MirConversion::Prelude { call, fallibility, .. } => {
            prelude_call_is_pure(prelude_calls, *call)
                && matches!(fallibility, MirCallFallibility::Infallible)
        }
    }
}
fn semantic_operation_is_pure(
    operation: &MirSemanticOp,
    prelude_calls: &HashMap<MirPreludeCallId, &MirPreludeCall>,
) -> bool {
    let calls = operation.prelude_calls();
    !calls.is_empty() && calls.into_iter().all(|call| prelude_call_is_pure(prelude_calls, call))
}

#[derive(Debug, Clone, Copy)]
struct CanonicalLoopRange {
    start: MirValueId,
    end: MirValueId,
    step: Option<MirValueId>,
    exclusive: bool,
}

#[derive(Debug, Clone)]
struct CanonicalLoopShape {
    form: MirLoopForm,
    cursor: Option<MirValueId>,
    cursor_place: Option<MirPlaceId>,
    range: Option<CanonicalLoopRange>,
    body: MirBlockId,
    exit: Option<MirBlockId>,
    advance: Option<MirBlockId>,
    blocks: Vec<MirBlockId>,
}

fn refresh_loop_summaries(function: &mut MirFunction) {
    let has_eligible = function
        .optimization
        .vector_facts
        .iter()
        .any(|fact| fact.decision.is_eligible());
    let no_aliasing = has_eligible
        && function
            .optimization
            .vector_facts
            .iter()
            .filter(|fact| fact.decision.is_eligible())
            .all(|fact| fact.no_aliasing);
    let no_early_exit = has_eligible
        && function
            .optimization
            .vector_facts
            .iter()
            .filter(|fact| fact.decision.is_eligible())
            .all(|fact| fact.no_early_exit);
    let no_cross_iteration_dependencies = has_eligible
        && function
            .optimization
            .vector_facts
            .iter()
            .filter(|fact| fact.decision.is_eligible())
            .all(|fact| fact.no_cross_iteration_dependencies);
    function.optimization.auto_vectorizable = has_eligible;
    function.optimization.no_aliasing = no_aliasing;
    function.optimization.no_early_exit = no_early_exit;
    function.optimization.no_cross_iteration_dependencies = no_cross_iteration_dependencies;
}


fn fusion_facts_for_rows(rows: &[MirLoopFact], vectors: &[MirVectorFact]) -> Vec<MirFusionFact> {
    let mut facts = Vec::new();
    for pair in rows.windows(2) {
        let first = &pair[0];
        let second = &pair[1];
        let first_vector = vectors.iter().find(|vector| vector.loop_header == first.header);
        let second_vector = vectors.iter().find(|vector| vector.loop_header == second.header);
        let same_domain = first.trip_count.is_some() && first.trip_count == second.trip_count;
        let adjacent = first.exit == Some(second.header);
        let packed = first_vector.is_some_and(|fact| fact.packed)
            && second_vector.is_some_and(|fact| fact.packed);
        let no_aliasing = first_vector.is_some_and(|fact| fact.no_aliasing)
            && second_vector.is_some_and(|fact| fact.no_aliasing);
        let effect_free = first_vector.is_some_and(|fact| fact.effect_free_body)
            && second_vector.is_some_and(|fact| fact.effect_free_body);
        let no_cross = first_vector
            .is_some_and(|fact| fact.no_cross_iteration_dependencies)
            && second_vector.is_some_and(|fact| fact.no_cross_iteration_dependencies);
        let decision = if !same_domain || !adjacent {
            MirOptimizationDecision::Rejected(MirOptimizationRejection::DynamicTripCount)
        } else if !packed {
            MirOptimizationDecision::Rejected(MirOptimizationRejection::ScalarBoundary)
        } else if !no_aliasing {
            MirOptimizationDecision::Rejected(MirOptimizationRejection::MayAlias)
        } else if !effect_free {
            MirOptimizationDecision::Rejected(MirOptimizationRejection::HasEffects)
        } else if !no_cross {
            MirOptimizationDecision::Rejected(
                MirOptimizationRejection::CrossIterationDependency,
            )
        } else {
            MirOptimizationDecision::Eligible
        };
        facts.push(MirFusionFact {
            first_loop: first.header,
            second_loop: second.header,
            packed,
            same_iteration_domain: same_domain,
            no_aliasing,
            effect_free,
            no_cross_iteration_dependencies: no_cross,
            span: first.span,
            decision,
        });
    }
    facts.sort_by_key(|fact| (fact.first_loop, fact.second_loop));
    facts
}

fn derive_loop_and_vector_facts(program: &mut MirProgram) {
    let prelude_calls: HashMap<_, _> = program
        .prelude_calls
        .iter()
        .map(|call| (call.id, call))
        .collect();
    let type_defs = program.types.as_slice();
    for function in &mut program.functions {
        function.optimization.loop_facts.clear();
        function.optimization.vector_facts.clear();
        function.optimization.fusion_facts.clear();
        function.optimization.acceleration_facts.clear();
        let mut shapes = Vec::new();
        for block in &function.blocks {
            if let crate::MIR::MirTerminator::Branch {
                then_target,
                else_target,
                ..
            } = &block.terminator
            {
                if let Some(shape) =
                    counted_shape(function, block.id, *then_target, *else_target)
                {
                    let trip_count = shape.range.and_then(|range| loop_trip_count(function, range));
                    let copy_cost = loop_copy_cost(function, &shape.blocks);
                    let row = MirLoopFact {
                        header: block.id,
                        body: Some(shape.body),
                        exit: shape.exit,
                        form: shape.form,
                        trip_count,
                        copy_cost,
                        canonical: true,
                        span: block.span,
                        decision: MirOptimizationDecision::Eligible,
                    };
                    shapes.push((row, shape));
                }
            } else if let crate::MIR::MirTerminator::Jump { target: body } = &block.terminator {
                if function.blocks.iter().any(|candidate| {
                    candidate.id == *body
                        && matches!(
                            &candidate.terminator,
                            crate::MIR::MirTerminator::Jump { target } if *target == block.id
                        )
                }) {
                    let shape = CanonicalLoopShape {
                        form: MirLoopForm::Unconditional,
                        cursor: None,
                        cursor_place: None,
                        range: None,
                        body: *body,
                        exit: None,
                        advance: None,
                        blocks: vec![*body],
                    };
                    shapes.push((
                        MirLoopFact {
                            header: block.id,
                            body: Some(*body),
                            exit: None,
                            form: MirLoopForm::Unconditional,
                            trip_count: None,
                            copy_cost: loop_copy_cost(function, &shape.blocks),
                            canonical: true,
                            span: block.span,
                            decision: MirOptimizationDecision::Eligible,
                        },
                        shape,
                    ));
                }
            }
        }
        for (row, shape) in &shapes {
            let vector = vector_fact(function, row, shape, &prelude_calls, type_defs);
            function.optimization.loop_facts.push(row.clone());
            function.optimization.vector_facts.push(vector);
        }
        let rows = function.optimization.loop_facts.clone();
        let vectors = function.optimization.vector_facts.clone();
        for vector in &vectors {
            if let Some(row) = rows.iter().find(|row| row.header == vector.loop_header) {
                function
                    .optimization
                    .acceleration_facts
                    .extend(acceleration_facts_for_vector(row, vector));
            }
        }
        function.optimization.fusion_facts = fusion_facts_for_rows(&rows, &vectors);
        function.optimization.loop_facts.sort_by_key(|fact| fact.header);
        function
            .optimization
            .vector_facts
            .sort_by_key(|fact| fact.loop_header);
        function
            .optimization
            .fusion_facts
            .sort_by_key(|fact| (fact.first_loop, fact.second_loop));
        function
            .optimization
            .acceleration_facts

            .sort_by_key(|fact| (fact.loop_header, fact.transform));
        refresh_loop_summaries(function);
    }
}
fn acceleration_facts_for_vector(
    row: &MirLoopFact,
    vector: &MirVectorFact,
) -> [MirAccelerationFact; 2] {
    use crate::MIROptimization::Acceleration::{
        AccelerationProof, AccelerationTransform, AccelerationWorkloadFacts,
    };

    let nested_reuse = vector.layout == MirVectorLayout::ColumnarDirect
        && matches!(row.copy_cost, MirCopyCost::PerIteration(_));
    let workload = AccelerationWorkloadFacts {
        nested_reuse,
        single_pass: !nested_reuse,
    };
    let source_proven = vector.decision.is_eligible();
    let independent_iterations = source_proven
        && vector.no_aliasing
        && vector.no_cross_iteration_dependencies
        && !matches!(
            vector.rule,
            MirVectorRule::ConditionalAccumulate
                | MirVectorRule::EarlyExitSearch
                | MirVectorRule::Reduction
        );
    let proof = AccelerationProof {
        source_proven,
        independent_iterations,
        no_aliasing: vector.no_aliasing,
        no_cross_iteration_dependencies: vector.no_cross_iteration_dependencies,
        no_early_exit: vector.no_early_exit,
        effect_free_body: vector.effect_free_body,
        ownership_safe: source_proven && vector.no_aliasing,
        failure_order_preserved: source_proven
            && vector.effect_free_body
            && vector.no_early_exit,
    };
    [
        MirAccelerationFact::source(
            vector.loop_header,
            AccelerationTransform::TransientColumnCopy,
            workload,
            proof,
            vector.span,
        ),
        MirAccelerationFact::source(
            vector.loop_header,
            AccelerationTransform::PooledParallelChunks,
            workload,
            proof,
            vector.span,
        ),
    ]
}

fn refresh_acceleration_facts(program: &mut MirProgram) {
    for function in &mut program.functions {
        function.optimization.acceleration_facts.clear();
        let rows = function.optimization.loop_facts.clone();
        let vectors = function.optimization.vector_facts.clone();
        for vector in &vectors {
            if let Some(row) = rows.iter().find(|row| row.header == vector.loop_header) {
                function
                    .optimization
                    .acceleration_facts
                    .extend(acceleration_facts_for_vector(row, vector));
            }
        }
        function
            .optimization
            .acceleration_facts
            .sort_by_key(|fact| (fact.loop_header, fact.transform));
    }
}

fn normalized_fixed_reduction_identity(
    function: &MirFunction,
    header: MirBlockId,
    role: &str,
) -> String {
    format!("{}:{}:{}", function.id.0, header.0, role)
}

fn normalized_fixed_reduction_block_id(
    function: &MirFunction,
    header: MirBlockId,
    role: &str,
) -> MirBlockId {
    MirBlockId(stable_id(
        "mir-fixed-reduction-block",
        &normalized_fixed_reduction_identity(function, header, role),
    ))
}

fn normalized_fixed_reduction_op_id(
    function: &MirFunction,
    header: MirBlockId,
    role: &str,
) -> MirOpId {
    MirOpId(stable_id(
        "mir-fixed-reduction-op",
        &normalized_fixed_reduction_identity(function, header, role),
    ))
}

fn normalized_fixed_reduction_value_id(
    function: &MirFunction,
    header: MirBlockId,
    role: &str,
) -> MirValueId {
    MirValueId(stable_id(
        "mir-fixed-reduction-value",
        &normalized_fixed_reduction_identity(function, header, role),
    ))
}

fn normalized_fixed_reduction_shape(
    function: &MirFunction,
    header: MirBlockId,
    then_target: MirBlockId,
    else_target: MirBlockId,
) -> Option<CanonicalLoopShape> {
    let dispatch_entry =
        normalized_fixed_reduction_block_id(function, header, "dispatch-entry");
    if then_target != dispatch_entry {
        return None;
    }
    let header_block = function.blocks.iter().find(|block| block.id == header)?;
    let cursor = header_block.instructions.iter().find_map(|instruction| {
        matches!(
            &instruction.operation,
            MirOperation::LoopRangeHasNext { cursor, .. } if instruction.result.is_some()
        )
        .then(|| match &instruction.operation {
            MirOperation::LoopRangeHasNext { cursor, .. } => *cursor,
            _ => unreachable!(),
        })
    })?;
    let range = canonical_range_for_cursor(function, cursor)?;
    let lane_dispatch =
        normalized_fixed_reduction_block_id(function, header, "lane-dispatch-0");
    let lane_pre = normalized_fixed_reduction_block_id(function, header, "lane-pre-0");
    let post_entry = normalized_fixed_reduction_block_id(function, header, "post-entry");
    let post_seen_entry =
        normalized_fixed_reduction_block_id(function, header, "post-seen-entry");
    let post_unseen_entry =
        normalized_fixed_reduction_block_id(function, header, "post-unseen-entry");
    let tree = normalized_fixed_reduction_block_id(function, header, "tree");
    let continuation = normalized_fixed_reduction_block_id(function, header, "continuation");
    let dispatch = function.blocks.iter().find(|block| block.id == dispatch_entry)?;
    if !matches!(
        &dispatch.terminator,
        MirTerminator::Jump { target } if *target == lane_dispatch
    ) {
        return None;
    }
    let lane_dispatch_block = function.blocks.iter().find(|block| block.id == lane_dispatch)?;
    let MirTerminator::Branch { then_target, .. } = &lane_dispatch_block.terminator else {
        return None;
    };
    if *then_target != lane_pre {
        return None;
    }
    let lane_pre_block = function.blocks.iter().find(|block| block.id == lane_pre)?;
    let MirTerminator::Jump { target: body } = &lane_pre_block.terminator else {
        return None;
    };
    let body = *body;
    if body == header || body == else_target || body == dispatch_entry {
        return None;
    }
    let post_entry_block = function.blocks.iter().find(|block| block.id == post_entry)?;
    if !matches!(
        &post_entry_block.terminator,
        MirTerminator::Jump { target } if *target == post_seen_entry
    ) {
        return None;
    }
    let post_seen_block = function
        .blocks
        .iter()
        .find(|block| block.id == post_seen_entry)?;
    if !matches!(
        &post_seen_block.terminator,
        MirTerminator::Jump { target } if *target == normalized_fixed_reduction_block_id(function, header, "lane-seen-dispatch-0")
    ) {
        return None;
    }
    let post_unseen_block = function
        .blocks
        .iter()
        .find(|block| block.id == post_unseen_entry)?;
    if !matches!(
        &post_unseen_block.terminator,
        MirTerminator::Jump { target } if *target == normalized_fixed_reduction_block_id(function, header, "lane-unseen-dispatch-0")
    ) {
        return None;
    }
    let generated_blocks = normalized_fixed_reduction_generated_blocks(function, header)?;
    if generated_blocks.contains(&else_target) {
        return None;
    }
    let tree_block = function.blocks.iter().find(|block| block.id == tree)?;
    if !matches!(
        &tree_block.terminator,
        MirTerminator::Jump { target } if *target == continuation
    ) {
        return None;
    }
    let exit_block = function.blocks.iter().find(|block| block.id == else_target)?;
    if !matches!(
        &exit_block.terminator,
        MirTerminator::Branch {
            then_target: gate_then,
            else_target: gate_else,
            ..
        } if *gate_then == tree && *gate_else == continuation
    ) {
        return None;
    }
    let advances = function
        .blocks
        .iter()
        .filter(|block| {
            block.instructions.iter().any(|instruction| {
                matches!(
                    &instruction.operation,
                    MirOperation::LoopRangeAdvance { cursor: value, .. } if *value == cursor
                )
            }) && matches!(
                &block.terminator,
                MirTerminator::Jump { target } if *target == header
            )
        })
        .map(|block| block.id)
        .collect::<Vec<_>>();
    if advances.len() != 1 {
        return None;
    }
    let advance = advances[0];
    let mut body_blocks = collect_loop_region(function, header, body, Some(post_entry));
    body_blocks.retain(|block_id| *block_id != advance);
    if body_blocks.is_empty()
        || !body_blocks.iter().any(|block_id| {
            function
                .blocks
                .iter()
                .find(|block| block.id == *block_id)
                .is_some_and(|block| block.terminator.targets().contains(&post_entry))
        })
    {
        return None;
    }
    body_blocks.push(advance);
    body_blocks.sort_unstable();
    Some(CanonicalLoopShape {
        form: MirLoopForm::Counted,
        cursor: Some(cursor),
        cursor_place: None,
        range: Some(range),
        body,
        exit: Some(else_target),
        advance: Some(advance),
        blocks: body_blocks,
    })
}
fn normalized_fixed_reduction_local_id(
    function: &MirFunction,
    header: MirBlockId,
    role: &str,
) -> MirLocalId {
    MirLocalId(stable_id(
        "mir-fixed-reduction-local",
        &normalized_fixed_reduction_identity(function, header, role),
    ))
}

fn normalized_fixed_reduction_place_id(
    function: &MirFunction,
    header: MirBlockId,
    role: &str,
) -> MirPlaceId {
    MirPlaceId(stable_id(
        "mir-fixed-reduction-place",
        &normalized_fixed_reduction_identity(function, header, role),
    ))
}
fn normalized_fixed_reduction_generated_blocks(
    function: &MirFunction,
    header: MirBlockId,
) -> Option<HashSet<MirBlockId>> {
    let order = crate::MIROptimization::Acceleration::D_FRED1_FIXED_ORDER;
    if !order.is_canonical() {
        return None;
    }
    let mut generated = HashSet::from([
        normalized_fixed_reduction_block_id(function, header, "dispatch-entry"),
        normalized_fixed_reduction_block_id(function, header, "post-entry"),
        normalized_fixed_reduction_block_id(function, header, "post-seen-entry"),
        normalized_fixed_reduction_block_id(function, header, "post-unseen-entry"),
        normalized_fixed_reduction_block_id(function, header, "tree"),
        normalized_fixed_reduction_block_id(function, header, "continuation"),
    ]);
    for lane in 0..order.lanes {
        generated.insert(normalized_fixed_reduction_block_id(
            function,
            header,
            &format!("lane-pre-{lane}"),
        ));
        generated.insert(normalized_fixed_reduction_block_id(
            function,
            header,
            &format!("lane-seen-post-{lane}"),
        ));
        generated.insert(normalized_fixed_reduction_block_id(
            function,
            header,
            &format!("lane-unseen-post-{lane}"),
        ));
        if lane + 1 < order.lanes {
            generated.insert(normalized_fixed_reduction_block_id(
                function,
                header,
                &format!("lane-dispatch-{lane}"),
            ));
            generated.insert(normalized_fixed_reduction_block_id(
                function,
                header,
                &format!("lane-seen-dispatch-{lane}"),
            ));
            generated.insert(normalized_fixed_reduction_block_id(
                function,
                header,
                &format!("lane-unseen-dispatch-{lane}"),
            ));
        }
    }
    generated
        .iter()
        .all(|block_id| function.blocks.iter().any(|block| block.id == *block_id))
        .then_some(generated)
}

fn normalized_fixed_reduction_fact(
    function: &MirFunction,
    fact: &MirVectorFact,
) -> Option<MirFixedReductionFact> {
    if fact.rule != MirVectorRule::Reduction || fact.fixed_reduction.is_some() {
        return None;
    }
    let header_block = function
        .blocks
        .iter()
        .find(|block| block.id == fact.loop_header)?;
    let MirTerminator::Branch {
        then_target,
        else_target,
        ..
    } = &header_block.terminator
    else {
        return None;
    };
    let (then_target, else_target) = (*then_target, *else_target);
    let shape =
        normalized_fixed_reduction_shape(function, fact.loop_header, then_target, else_target)?;
    let Some(advance) = shape.advance else {
        return None;
    };
    if fact.body_blocks
        != shape
            .blocks
            .iter()
            .copied()
            .filter(|block_id| *block_id != advance)
            .collect::<Vec<_>>()
    {
        return None;
    }
    let instructions = fact
        .body_blocks
        .iter()
        .flat_map(|block_id| {
            function
                .blocks
                .iter()
                .find(|block| block.id == *block_id)
                .into_iter()
                .flat_map(|block| block.instructions.iter())
        })
        .collect::<Vec<_>>();
    let (accumulator, addend, _) = fixed_reduction_plan(function, &instructions)?;
    let seed_op = normalized_fixed_reduction_op_id(function, fact.loop_header, "seed-read");
    let expected_seed_value =
        normalized_fixed_reduction_value_id(function, fact.loop_header, "seed-read");
    let seed_value = function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .find(|instruction| {
            instruction.id == seed_op && instruction.result == Some(expected_seed_value)
        })
        .and_then(|instruction| match &instruction.operation {
            MirOperation::ReadPlace(place) if *place == accumulator => instruction.result,
            _ => None,
        })?;
    let continuation =
        normalized_fixed_reduction_block_id(function, fact.loop_header, "continuation");
    let tree = normalized_fixed_reduction_block_id(function, fact.loop_header, "tree");
    let tree_block = function.blocks.iter().find(|block| block.id == tree)?;
    if !tree_block.instructions.iter().any(|instruction| {
        matches!(
            &instruction.operation,
            MirOperation::WritePlace { place, .. } if *place == accumulator
        )
    }) {
        return None;
    }
    let source_operations = instructions.iter().map(|instruction| instruction.id).collect();
    Some(MirFixedReductionFact {
        accumulator,
        addend,
        seed: seed_value,
        condition: None,
        source_operations,
        exit: continuation,
        order: crate::MIROptimization::Acceleration::D_FRED1_FIXED_ORDER,
    })
}

fn counted_shape(
    function: &MirFunction,
    header: MirBlockId,
    then_target: MirBlockId,
    else_target: MirBlockId,
) -> Option<CanonicalLoopShape> {
    if let Some(shape) =
        normalized_fixed_reduction_shape(function, header, then_target, else_target)
    {
        return Some(shape);
    }
    let header_block = function.blocks.iter().find(|block| block.id == header)?;
    let cursor_form = header_block
        .instructions
        .iter()
        .find_map(|instruction| match &instruction.operation {
            MirOperation::LoopRangeHasNext { cursor, .. } => {
                Some((MirLoopForm::Counted, *cursor))
            }
            MirOperation::LoopIterHasNext { cursor, .. } => {
                Some((MirLoopForm::Iterator, *cursor))
            }
            _ => None,
        });
    let blocks = collect_loop_region(function, header, then_target, Some(else_target));
    if let Some((form, cursor)) = cursor_form {
        let has_value = blocks.iter().any(|block_id| {
            function
                .blocks
                .iter()
                .find(|block| block.id == *block_id)
                .is_some_and(|block| {
                    block.instructions.iter().any(|instruction| {
                        matches!(
                            &instruction.operation,
                            MirOperation::LoopRangeValue { cursor: value, .. }
                                | MirOperation::LoopIterValue { cursor: value, .. }
                                if *value == cursor
                        )
                    })
                })
        });
        let advance = blocks.iter().find_map(|block_id| {
            let block = function.blocks.iter().find(|block| block.id == *block_id)?;
            let advances = block.instructions.iter().any(|instruction| {
                matches!(
                    &instruction.operation,
                    MirOperation::LoopRangeAdvance { cursor: value, .. }
                        | MirOperation::LoopIterAdvance { cursor: value, .. }
                        if *value == cursor
                )
            });
            advances
                .then(|| matches!(&block.terminator, crate::MIR::MirTerminator::Jump { target } if *target == header))
                .and_then(|is_advance| is_advance.then_some(*block_id))
        });
        if !has_value || advance.is_none() {
            return None;
        }
        return Some(CanonicalLoopShape {
            form,
            cursor: Some(cursor),
            cursor_place: None,
            range: canonical_range_for_cursor(function, cursor),
            body: then_target,
            exit: Some(else_target),
            advance,
            blocks,
        });
    }
    scalar_counted_shape(function, header, then_target, else_target, blocks)
}

fn canonical_range_for_cursor(
    function: &MirFunction,
    cursor: MirValueId,
) -> Option<CanonicalLoopRange> {
    function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .find_map(|instruction| {
            let MirOperation::LoopRangeInit {
                start,
                end,
                step,
                exclusive,
                ..
            } = &instruction.operation
            else {
                return None;
            };
            (instruction.result == Some(cursor)).then_some(CanonicalLoopRange {
                start: *start,
                end: *end,
                step: *step,
                exclusive: *exclusive,
            })
        })
}

fn scalar_counted_shape(
    function: &MirFunction,
    header: MirBlockId,
    then_target: MirBlockId,
    else_target: MirBlockId,
    blocks: Vec<MirBlockId>,
) -> Option<CanonicalLoopShape> {
    if blocks.is_empty() {
        return None;
    }
    let header_block = function.blocks.iter().find(|block| block.id == header)?;
    let MirTerminator::Branch { condition, .. } = &header_block.terminator else {
        return None;
    };
    let (cursor_value, cursor_place, end, exclusive) =
        scalar_loop_condition(function, *condition)?;
    let place = function
        .places
        .iter()
        .find(|place| place.id == cursor_place)?;
    if !matches!(&place.base, MirPlaceBase::Local(_))
        || !place.projections.is_empty()
        || !is_default_int_type(&place.ty)
        || !value_type(function, cursor_value).is_some_and(|ty| is_default_int_type(&ty))
    {
        return None;
    }
    let backedges = blocks.iter().filter(|block_id| {
        function
            .blocks
            .iter()
            .find(|block| block.id == **block_id)
            .is_some_and(|block| block.terminator.targets().contains(&header))
    });
    if backedges.count() != 1 {
        return None;
    }
    let (advance, advance_write, step_value) = blocks.iter().find_map(|block_id| {
        scalar_loop_advance(function, *block_id, header, cursor_place)
            .map(|(write, step)| (*block_id, write, step))
    })?;
    let mut cursor_writes = Vec::new();
    for block in &function.blocks {
        for instruction in &block.instructions {
            let references_cursor = operation_place_refs(&instruction.operation)
                .into_iter()
                .any(|place| places_overlap(function, place, cursor_place));
            if references_cursor {
                match &instruction.operation {
                    MirOperation::WritePlace { .. } => {
                        cursor_writes.push((block.id, instruction));
                    }
                    MirOperation::ReadPlace(_) => {}
                    _ => return None,
                }
            }
        }
    }
    if cursor_writes.len() != 2 {
        return None;
    }
    let init = cursor_writes
        .iter()
        .filter(|(block_id, instruction)| {
            *block_id != advance
                && *block_id != header
                && !blocks.contains(block_id)
                && matches!(
                    function
                        .blocks
                        .iter()
                        .find(|block| block.id == *block_id)
                        .map(|block| &block.terminator),
                    Some(MirTerminator::Jump { target }) if *target == header
                )
                && instruction.id != advance_write
        })
        .collect::<Vec<_>>();
    if init.len() != 1 {
        return None;
    }
    let MirOperation::WritePlace { value: start, .. } = &init[0].1.operation else {
        return None;
    };
    let end_is_invariant = function.blocks.iter().any(|block| {
        block.instructions.iter().any(|instruction| {
            instruction.result == Some(end)
                && (matches!(&instruction.operation, MirOperation::Constant(_))
                    || block.id != header && !blocks.contains(&block.id))
        })
    });
    let constants = collect_scalar_constants(function);
    let step = scalar_int_constant(&constants, step_value)?;
    if step <= 0
        || !end_is_invariant
        || value_depends_on_place(function, *start, cursor_place, &mut HashSet::new())
        || value_depends_on_place(function, end, cursor_place, &mut HashSet::new())
        || !scalar_cursor_references_are_clean(
            function,
            header,
            cursor_value,
            &blocks,
            advance,
            advance_write,
            cursor_place,
        )
    {
        return None;
    }
    let body_cursor = scalar_loop_cursor_value(function, &blocks, advance, cursor_place)
        .unwrap_or(cursor_value);
    Some(CanonicalLoopShape {
        form: MirLoopForm::Counted,
        cursor: Some(body_cursor),
        cursor_place: Some(cursor_place),
        range: Some(CanonicalLoopRange {
            start: *start,
            end,
            step: Some(step_value),
            exclusive,
        }),
        body: then_target,
        exit: Some(else_target),
        advance: Some(advance),
        blocks,
    })
}

fn scalar_loop_condition(
    function: &MirFunction,
    condition: MirValueId,
) -> Option<(MirValueId, MirPlaceId, MirValueId, bool)> {
    let instruction = find_value_instruction(function, condition)?;
    let MirOperation::Binary {
        op,
        dispatch: crate::MIR::MirBinaryDispatch::Primitive,
        left,
        right,
    } = &instruction.operation
    else {
        return None;
    };
    let exclusive = match op {
        MirBinaryOp::Lt => true,
        MirBinaryOp::Le => false,
        _ => return None,
    };
    let cursor_place = direct_read_place(function, *left)?;
    Some((*left, cursor_place, *right, exclusive))
}

fn scalar_loop_advance(
    function: &MirFunction,
    block_id: MirBlockId,
    header: MirBlockId,
    cursor_place: MirPlaceId,
) -> Option<(MirOpId, MirValueId)> {
    let block = function.blocks.iter().find(|block| block.id == block_id)?;
    if !matches!(&block.terminator, MirTerminator::Jump { target } if *target == header) {
        return None;
    }
    let writes = block
        .instructions
        .iter()
        .filter(|instruction| {
            matches!(
                &instruction.operation,
                MirOperation::WritePlace { place, .. } if *place == cursor_place
            )
        })
        .collect::<Vec<_>>();
    let [write] = writes.as_slice() else {
        return None;
    };
    let MirOperation::WritePlace { value, .. } = &write.operation else {
        return None;
    };
    let update = find_value_instruction(function, *value)?;
    let MirOperation::Binary {
        op: MirBinaryOp::Add,
        dispatch: crate::MIR::MirBinaryDispatch::Primitive,
        left,
        right,
    } = &update.operation
    else {
        return None;
    };
    let step = if direct_read_place(function, *left) == Some(cursor_place) {
        *right
    } else if direct_read_place(function, *right) == Some(cursor_place) {
        *left
    } else {
        return None;
    };
    if block
        .instructions
        .iter()
        .filter(|instruction| {
            matches!(
                &instruction.operation,
                MirOperation::ReadPlace(place) if *place == cursor_place
            )
        })
        .count()
        != 1
        || block
            .instructions
            .iter()
            .any(|instruction| match &instruction.operation {
                MirOperation::Constant(_) => false,
                MirOperation::ReadPlace(place) => *place != cursor_place,
                MirOperation::Binary { .. } => instruction.id != update.id,
                MirOperation::WritePlace { place, .. } => {
                    *place != cursor_place || instruction.id != write.id
                }
                _ => true,
            })
    {
        return None;
    }
    (!value_depends_on_place(function, step, cursor_place, &mut HashSet::new()))
        .then_some((write.id, step))
}

fn direct_read_place(function: &MirFunction, value: MirValueId) -> Option<MirPlaceId> {
    find_value_instruction(function, value).and_then(|instruction| match &instruction.operation {
        MirOperation::ReadPlace(place) => Some(*place),
        _ => None,
    })
}

fn cursor_value_matches(
    function: &MirFunction,
    value: MirValueId,
    cursor: MirValueId,
    cursor_place: Option<MirPlaceId>,
) -> bool {
    value == cursor
        || cursor_place.is_some_and(|place| direct_read_place(function, value) == Some(place))
}

fn scalar_int_constant(
    constants: &HashMap<MirValueId, MirConstant>,
    value: MirValueId,
) -> Option<i64> {
    match constants.get(&value) {
        Some(MirConstant::Int { value, .. }) => Some(*value),
        _ => None,
    }
}

fn value_depends_on_place(
    function: &MirFunction,
    value: MirValueId,
    place: MirPlaceId,
    seen: &mut HashSet<MirValueId>,
) -> bool {
    if !seen.insert(value) {
        return true;
    }
    let Some(instruction) = find_value_instruction(function, value) else {
        return true;
    };
    if operation_place_refs(&instruction.operation)
        .into_iter()
        .any(|candidate| places_overlap(function, candidate, place))
    {
        return true;
    }
    instruction
        .operation
        .value_uses()
        .into_iter()
        .any(|operand| value_depends_on_place(function, operand, place, seen))
}

fn places_overlap(function: &MirFunction, left: MirPlaceId, right: MirPlaceId) -> bool {
    let (Some(left), Some(right)) = (
        function.places.iter().find(|place| place.id == left),
        function.places.iter().find(|place| place.id == right),
    ) else {
        return true;
    };
    mir_place_paths_overlap(&mir_place_path(left), &mir_place_path(right))
}

fn scalar_cursor_references_are_clean(
    function: &MirFunction,
    header: MirBlockId,
    condition: MirValueId,
    blocks: &[MirBlockId],
    advance: MirBlockId,
    advance_write: MirOpId,
    cursor_place: MirPlaceId,
) -> bool {
    let Some(header_block) = function.blocks.iter().find(|block| block.id == header) else {
        return false;
    };
    let header_refs = header_block
        .instructions
        .iter()
        .filter(|instruction| {
            operation_place_refs(&instruction.operation)
                .into_iter()
                .any(|place| places_overlap(function, place, cursor_place))
        })
        .collect::<Vec<_>>();
    if header_refs.len() != 1
        || !matches!(
            &header_refs[0].operation,
            MirOperation::ReadPlace(place) if *place == cursor_place
        )
        || header_refs[0].result.is_none_or(|value| value != condition)
    {
        return false;
    }
    for block_id in blocks {
        let Some(block) = function.blocks.iter().find(|block| block.id == *block_id) else {
            return false;
        };
        for instruction in &block.instructions {
            let references_cursor = operation_place_refs(&instruction.operation)
                .into_iter()
                .any(|place| places_overlap(function, place, cursor_place));
            if !references_cursor {
                continue;
            }
            if *block_id == advance {
                let allowed = instruction.id == advance_write
                    && matches!(
                        &instruction.operation,
                        MirOperation::WritePlace { place, .. } if *place == cursor_place
                    )
                    || matches!(
                        &instruction.operation,
                        MirOperation::ReadPlace(place) if *place == cursor_place
                    );
                if !allowed {
                    return false;
                }
            } else if !matches!(
                &instruction.operation,
                MirOperation::ReadPlace(place) if *place == cursor_place
            ) {
                return false;
            }
        }
    }
    true
}

fn scalar_loop_cursor_value(
    function: &MirFunction,
    blocks: &[MirBlockId],
    advance: MirBlockId,
    cursor_place: MirPlaceId,
) -> Option<MirValueId> {
    let mut reads = Vec::new();
    for block_id in blocks {
        if *block_id == advance {
            continue;
        }
        let block = function.blocks.iter().find(|block| block.id == *block_id)?;
        for instruction in &block.instructions {
            if let (Some(result), MirOperation::ReadPlace(place)) =
                (instruction.result, &instruction.operation)
            {
                if *place == cursor_place {
                    reads.push(result);
                }
            }
        }
    }
    reads
        .iter()
        .copied()
        .find(|value| loop_value_is_indexed(function, blocks, advance, *value))
        .or_else(|| reads.into_iter().next())
}

fn loop_value_is_indexed(
    function: &MirFunction,
    blocks: &[MirBlockId],
    advance: MirBlockId,
    value: MirValueId,
) -> bool {
    blocks.iter().filter(|block_id| **block_id != advance).any(|block_id| {
        function
            .blocks
            .iter()
            .find(|block| block.id == *block_id)
            .is_some_and(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(&instruction.operation, MirOperation::Index { index, .. } if *index == value)
                        || operation_place_refs(&instruction.operation).into_iter().any(|place_id| {
                            function
                                .places
                                .iter()
                                .find(|place| place.id == place_id)
                                .is_some_and(|place| {
                                    place.projections.iter().any(|projection| {
                                        matches!(projection, MirProjection::Index { index, .. } if *index == value)
                                    })
                                })
                        })
                })
            })
    })
}

fn collect_loop_region(
    function: &MirFunction,
    header: MirBlockId,
    body: MirBlockId,
    exit: Option<MirBlockId>,
) -> Vec<MirBlockId> {
    let mut pending = vec![body];
    let mut seen = HashSet::new();
    let mut region = Vec::new();
    while let Some(block_id) = pending.pop() {
        if block_id == header || exit == Some(block_id) || !seen.insert(block_id) {
            continue;
        }
        let Some(block) = function.blocks.iter().find(|block| block.id == block_id) else {
            continue;
        };
        region.push(block_id);
        for target in block.terminator.targets() {
            if target != header && exit != Some(target) {
                pending.push(target);
            }
        }
    }
    region.sort_unstable();
    region
}

fn loop_trip_count(function: &MirFunction, range: CanonicalLoopRange) -> Option<u64> {
    let constants = collect_scalar_constants(function);
    let MirConstant::Int { value: start, .. } = constants.get(&range.start)? else {
        return None;
    };
    let MirConstant::Int { value: end, .. } = constants.get(&range.end)? else {
        return None;
    };
    let step = range
        .step
        .and_then(|step| match constants.get(&step) {
            Some(MirConstant::Int { value, .. }) => Some(*value),
            _ => None,
        })
        .unwrap_or(1);
    if step == 0 {
        return None;
    }
    if step > 0 {
        if *start > *end || (*start == *end && range.exclusive) {
            return Some(0);
        }
        let distance = end.checked_sub(*start)?;
        let count = if range.exclusive {
            distance
        } else {
            distance.checked_add(1)?
        };
        let numerator = count.checked_add(step.checked_sub(1)?)?;
        Some(u64::try_from(numerator / step).ok()?)
    } else {
        if *start < *end || (*start == *end && range.exclusive) {
            return Some(0);
        }
        let distance = start.checked_sub(*end)?;
        let magnitude = step.checked_neg()?;
        let count = if range.exclusive {
            distance
        } else {
            distance.checked_add(1)?
        };
        let numerator = count.checked_add(magnitude.checked_sub(1)?)?;
        Some(u64::try_from(numerator / magnitude).ok()?)
    }
}

fn loop_copy_cost(function: &MirFunction, blocks: &[MirBlockId]) -> MirCopyCost {
    let mut copies = 0u32;
    for block_id in blocks {
        let Some(block) = function.blocks.iter().find(|block| block.id == *block_id) else {
            return MirCopyCost::Unknown;
        };
        for instruction in &block.instructions {
            match &instruction.operation {
                MirOperation::Copy { .. } => copies = copies.saturating_add(1),
                MirOperation::BuildList { .. } | MirOperation::BuildMap { .. } => {
                    return MirCopyCost::Unknown
                }
                MirOperation::Call { args, .. }
                | MirOperation::CoreCall { args, .. }
                | MirOperation::IndirectCall { args, .. } => {
                    copies = copies.saturating_add(
                        args.iter()
                            .filter(|arg| {
                                arg.implicit_clone
                                    || arg.shared_auto_clone
                                    || arg.widen_fixed_to_list
                            })
                            .count() as u32,
                    );
                }
                MirOperation::Semantic(
                    MirSemanticOp::StaticPreludeCall { args, .. }
                    | MirSemanticOp::ClosureMethod { args, .. }
                    | MirSemanticOp::HostCall { args, .. },
                ) => {
                    copies = copies.saturating_add(
                        args.iter()
                            .filter(|arg| {
                                arg.implicit_clone
                                    || arg.shared_auto_clone
                                    || arg.widen_fixed_to_list
                            })
                            .count() as u32,
                    );
                }
                _ => {}
            }
        }
    }
    if copies == 0 {
        MirCopyCost::None
    } else {
        MirCopyCost::PerIteration(copies)
    }
}

fn vector_fact(
    function: &MirFunction,
    row: &MirLoopFact,
    shape: &CanonicalLoopShape,
    prelude_calls: &HashMap<MirPreludeCallId, &MirPreludeCall>,
    type_defs: &[MirTypeDef],
) -> MirVectorFact {
    let body_blocks = shape
        .blocks
        .iter()
        .copied()
        .filter(|block| Some(*block) != shape.advance)
        .collect::<Vec<_>>();
    let instructions = body_blocks
        .iter()
        .filter_map(|block_id| function.blocks.iter().find(|block| block.id == *block_id))
        .flat_map(|block| block.instructions.iter())
        .collect::<Vec<_>>();
    let cursor = shape.cursor;
    let cursor_place = shape.cursor_place;
    let accesses = vector_accesses(function, &body_blocks, cursor, cursor_place, type_defs);
    let layouts = accesses
        .iter()
        .filter(|access| access.field.is_some() || access.layout != MirVectorLayout::Flat)
        .map(|access| access.layout)
        .collect::<BTreeSet<_>>();
    let layout = if layouts.len() == 1 {
        *layouts.iter().next().expect("one vector layout")
    } else {
        MirVectorLayout::Flat
    };
    let uniform_layout = layouts.len() <= 1;
    let columnar_write = instructions.iter().any(|instruction| {
        let MirOperation::WritePlace { place, .. } = &instruction.operation else {
            return false;
        };
        let Some(place) = function.places.iter().find(|candidate| candidate.id == *place) else {
            return false;
        };
        let Some(field) = place.projections.iter().rev().find_map(|projection| match projection {
            MirProjection::Field { field, .. } => Some(*field),
            _ => None,
        }) else {
            return false;
        };
        vector_place_layout(function, place, Some(field), type_defs).0
            == MirVectorLayout::ColumnarDirect
    });
    let element_type = vector_element_type(function, &instructions, cursor_place);
    let lane = lane_width(element_type.as_ref());
    let proven_indexing =
        vector_indexing_is_proven(function, &instructions, cursor, cursor_place, shape.range);
    let indexed_input = cursor.is_some_and(|cursor| {
        vector_has_indexed_input(function, &instructions, cursor, cursor_place)
    });
    let has_early_exit = row.exit.is_some_and(|exit| {
        body_blocks.iter().any(|block_id| {
            function
                .blocks
                .iter()
                .find(|block| block.id == *block_id)
                .is_some_and(|block| match &block.terminator {
                    crate::MIR::MirTerminator::Break { target, .. } => *target == exit,
                    crate::MIR::MirTerminator::Branch {
                        then_target,
                        else_target,
                        ..
                    } => *then_target == exit || *else_target == exit,
                    crate::MIR::MirTerminator::Jump { target } => *target == exit,
                    crate::MIR::MirTerminator::Return { .. } => true,
                    _ => false,
                })
        })
    });
    let has_branch = body_blocks.iter().any(|block_id| {
        function
            .blocks
            .iter()
            .find(|block| block.id == *block_id)
            .is_some_and(|block| matches!(&block.terminator, crate::MIR::MirTerminator::Branch { .. }))
    });
    let rmw = vector_rmw(&instructions);
    let rmw_accumulator_type = vector_rmw_accumulator_type(function, &instructions);
    let has_comparison = instructions.iter().any(|instruction| {
        matches!(
            &instruction.operation,
            MirOperation::Binary { op, .. } if op.is_comparison()
        )
    });
    let rule = if has_early_exit && has_comparison {
        MirVectorRule::EarlyExitSearch
    } else if has_branch && rmw.is_some() {
        MirVectorRule::ConditionalAccumulate
    } else if rmw.is_some() && indexed_input {
        MirVectorRule::Reduction
    } else if accesses.iter().any(|access| access.field.is_some()) {
        MirVectorRule::FieldAccess
    } else {
        MirVectorRule::Elementwise
    };
    let effect_free_body = function.effects.direct.is_empty()
        && function.effects.call_edges.is_empty()
        && !instructions.iter().any(|instruction| {
            vector_operation_has_effect(function, &instruction.operation, prelude_calls)
        });
    let no_early_exit = !has_early_exit;
    let (no_aliasing, no_cross_iteration_dependencies) =
        vector_memory_facts(function, &body_blocks, cursor, cursor_place, rule);
    let conditional_plan = if rule == MirVectorRule::ConditionalAccumulate {
        conditional_accumulate_plan(
            function,
            &body_blocks,
            row.exit,
            &instructions,
        )
    } else {
        None
    };
    let early_exit_proven = if rule == MirVectorRule::EarlyExitSearch {
        vector_early_exit_proven(function, &body_blocks, row.exit, cursor, cursor_place)
    } else {
        false
    };
    let packed = element_type.as_ref().is_some_and(|ty| is_packable_for_rule(rule, ty))
        && lane.is_some()
        && !accesses.is_empty()
        && uniform_layout
        && !columnar_write
        && proven_indexing
        && no_aliasing
        && effect_free_body
        && no_cross_iteration_dependencies
        && (no_early_exit || (rule == MirVectorRule::EarlyExitSearch && early_exit_proven));
    let decision = if shape.form != MirLoopForm::Counted {
        MirOptimizationDecision::Rejected(MirOptimizationRejection::DynamicTripCount)
    } else if cursor.is_none() || accesses.is_empty() || element_type.is_none() {
        MirOptimizationDecision::Rejected(MirOptimizationRejection::UnsupportedOperation)
    } else if !proven_indexing {
        MirOptimizationDecision::Rejected(MirOptimizationRejection::ScalarBoundary)
    } else if !element_type
        .as_ref()
        .is_some_and(|ty| is_packable_for_rule(rule, ty))
    {
        MirOptimizationDecision::Rejected(MirOptimizationRejection::UnsupportedOperation)
    } else if !effect_free_body {
        MirOptimizationDecision::Rejected(MirOptimizationRejection::HasEffects)
    } else if !uniform_layout || columnar_write {
        MirOptimizationDecision::Rejected(MirOptimizationRejection::ObservableLayout)
    } else if !no_aliasing {
        MirOptimizationDecision::Rejected(MirOptimizationRejection::MayAlias)
    } else if !no_cross_iteration_dependencies {
        MirOptimizationDecision::Rejected(MirOptimizationRejection::CrossIterationDependency)
    } else if rule == MirVectorRule::ConditionalAccumulate
        && (conditional_plan.is_none()
            || rmw != Some(MirBinaryOp::Add)
            || !indexed_input
            || !element_type.as_ref().is_some_and(is_reduction_type)
            || !rmw_accumulator_type.as_ref().is_some_and(is_reduction_type))
    {
        MirOptimizationDecision::Rejected(MirOptimizationRejection::UnsupportedOperation)
    } else if rule == MirVectorRule::EarlyExitSearch && !early_exit_proven {
        MirOptimizationDecision::Rejected(MirOptimizationRejection::UnsupportedOperation)
    } else if rule == MirVectorRule::Reduction
        && (rmw != Some(MirBinaryOp::Add)
            || !indexed_input
            || !element_type.as_ref().is_some_and(is_reduction_type)
            || !rmw_accumulator_type.as_ref().is_some_and(is_reduction_type))
    {
        MirOptimizationDecision::Rejected(MirOptimizationRejection::UnsupportedOperation)
    } else if !no_early_exit && rule != MirVectorRule::EarlyExitSearch {
        MirOptimizationDecision::Rejected(MirOptimizationRejection::HasEarlyExit)
    } else if lane.is_none() {
        MirOptimizationDecision::Rejected(MirOptimizationRejection::ScalarBoundary)
    } else {
        MirOptimizationDecision::Eligible
    };
    let fixed_reduction = conditional_plan.map(|(accumulator, addend, seed, condition)| {
        MirFixedReductionFact {
            accumulator,
            addend,
            seed,
            condition: Some(condition),
            source_operations: instructions.iter().map(|instruction| instruction.id).collect(),
            exit: row.exit.or(shape.advance).unwrap_or(row.header),
            order: crate::MIROptimization::Acceleration::D_FRED1_FIXED_ORDER,
        }
    });
    let mut accesses = accesses;
    accesses.sort_unstable();
    MirVectorFact {
        loop_header: row.header,
        cursor,
        body_blocks,
        advance_block: shape.advance,
        rule,
        accesses,
        layout,
        element_type,
        packed,
        lane_width: lane,
        no_aliasing,
        no_early_exit,
        effect_free_body,
        no_cross_iteration_dependencies,
        fixed_reduction,
        span: row.span,
        decision,
    }
}

fn vector_element_type(
    function: &MirFunction,
    instructions: &[&crate::MIR::MirInstruction],
    cursor_place: Option<MirPlaceId>,
) -> Option<MirType> {
    let type_for = |instruction: &crate::MIR::MirInstruction| {
        let ty = match &instruction.operation {
            MirOperation::ReadPlace(place)
            | MirOperation::MovePlace { place }
            | MirOperation::WritePlace { place, .. } => function
                .places
                .iter()

                .find(|candidate| candidate.id == *place)
                .map(|place| &place.ty),
            MirOperation::Index { .. }
            | MirOperation::Semantic(MirSemanticOp::ColumnarRead { .. })
            | MirOperation::Field { .. }
            | MirOperation::Binary { .. } => instruction.ty.as_ref(),
            _ => None,
        }?;
        is_packable_type(ty).then(|| ty.clone())
    };
    instructions
        .iter()
        .filter(|instruction| {
            matches!(
                &instruction.operation,
                MirOperation::Index { .. }
                    | MirOperation::Semantic(MirSemanticOp::ColumnarRead { .. })
                    | MirOperation::Field { .. }
                    | MirOperation::Binary { .. }
            ) || matches!(
                &instruction.operation,
                MirOperation::WritePlace { place, .. }
                    if function
                        .places
                        .iter()
                        .find(|candidate| candidate.id == *place)
                        .is_some_and(|place| !place.projections.is_empty())
            )
        })
        .find_map(|instruction| type_for(instruction))
        .or_else(|| {
            instructions.iter().find_map(|instruction| {
                if matches!(
                    &instruction.operation,
                    MirOperation::ReadPlace(place) | MirOperation::MovePlace { place }
                        if cursor_place == Some(*place)
                ) {
                    return None;
                }
                type_for(instruction)
            })
        })
}

fn vector_indexing_is_proven(
    function: &MirFunction,
    instructions: &[&crate::MIR::MirInstruction],
    cursor: Option<MirValueId>,
    cursor_place: Option<MirPlaceId>,
    range: Option<CanonicalLoopRange>,
) -> bool {
    let Some(cursor) = cursor else {
        return false;
    };
    let Some(range) = range else {
        return false;
    };
    if !range.exclusive {
        return false;
    }
    let constants = collect_scalar_constants(function);
    if !matches!(
        constants.get(&range.start),
        Some(MirConstant::Int { value: 0, .. })
    ) {
        return false;
    }
    if let Some(step) = range.step {
        if !matches!(
            constants.get(&step),
            Some(MirConstant::Int { value: 1, .. })
        ) {
            return false;
        }
    }
    let range_end = scalar_int_constant(&constants, range.end)
        .and_then(|end| u64::try_from(end).ok());
    let required_len = |index, cursor_index| {
        if cursor_index {
            range_end
        } else {
            scalar_int_constant(&constants, index)
                .and_then(|index| u64::try_from(index).ok())
                .and_then(|index| index.checked_add(1))
        }
    };
    let mut indexed = false;
    for instruction in instructions {
        if let MirOperation::Index { base, index, kind, .. } = &instruction.operation {
            let cursor_index = cursor_value_matches(function, *index, cursor, cursor_place);
            indexed |= cursor_index;
            if *kind != MirIndexKind::FixedListProof
                && (*kind != MirIndexKind::List
                    || !required_len(*index, cursor_index).is_some_and(|end| {
                        function.values.iter().any(|(value, ty, ..)| {
                            *value == *base && vector_fixed_bound_covers(ty, end)
                        })
                    }))
            {
                return false;
            }
        }
        for place_id in operation_place_refs(&instruction.operation) {
            let Some(place) = function.places.iter().find(|place| place.id == place_id) else {
                return false;
            };
            for (position, projection) in place.projections.iter().enumerate() {
                if let MirProjection::Index { index, kind, .. } = projection {
                    let cursor_index =
                        cursor_value_matches(function, *index, cursor, cursor_place);
                    indexed |= cursor_index;
                    if *kind == MirIndexKind::FixedListProof {
                        continue;
                    }
                    let base_ty = match &place.base {
                        MirPlaceBase::Local(local) => function
                            .locals
                            .iter()
                            .find(|candidate| candidate.id == *local)
                            .map(|local| &local.ty),
                        MirPlaceBase::Parameter(value)
                        | MirPlaceBase::Capture(value)
                        | MirPlaceBase::Temporary(value) => function
                            .values
                            .iter()
                            .find(|(candidate, ..)| candidate == value)
                            .map(|(_, ty, ..)| ty),
                        MirPlaceBase::Static(_) => None,
                    };
                    if *kind != MirIndexKind::List
                        || position != 0
                        || !required_len(*index, cursor_index)
                            .zip(base_ty)
                            .is_some_and(|(end, ty)| vector_fixed_bound_covers(ty, end))
                    {
                        return false;
                    }
                }
            }
        }
    }
    indexed
}

fn vector_fixed_bound_covers(ty: &MirType, required_len: u64) -> bool {
    match ty.kind() {
        MirTypeKind::FixedList {
            len: crate::MIR::MirMeasure::Literal { value, .. },
            ..
        } => required_len <= *value,
        MirTypeKind::Tagged { inner, .. } => vector_fixed_bound_covers(inner, required_len),
        _ => false,
    }
}

fn conditional_accumulate_plan(
    function: &MirFunction,
    body_blocks: &[MirBlockId],
    exit: Option<MirBlockId>,
    instructions: &[&crate::MIR::MirInstruction],
) -> Option<(MirPlaceId, MirValueId, MirValueId, MirValueId)> {
    let (accumulator, addend, seed) = fixed_reduction_plan(function, instructions)?;
    let writes = instructions
        .iter()
        .filter_map(|instruction| match &instruction.operation {
            MirOperation::WritePlace { place, .. } => Some(*place),
            _ => None,
        })
        .collect::<Vec<_>>();
    if writes.len() != 1 || writes[0] != accumulator {
        return None;
    }
    let write_block = body_blocks.iter().copied().find(|block_id| {
        function
            .blocks
            .iter()
            .find(|block| block.id == *block_id)
            .is_some_and(|block| {
                block.instructions.iter().any(|instruction| {
                    matches!(
                        &instruction.operation,
                        MirOperation::WritePlace { place, .. } if *place == accumulator
                    )
                })
            })
    })?;
    let allowed = body_blocks.iter().copied().collect::<HashSet<_>>();
    for block_id in body_blocks {
        let Some(block) = function.blocks.iter().find(|block| block.id == *block_id) else {
            continue;
        };
        let MirTerminator::Branch {
            condition,
            then_target,
            else_target,
        } = &block.terminator
        else {
            continue;
        };
        if exit.is_some_and(|exit| *then_target == exit || *else_target == exit) {
            continue;
        }
        let then_reaches = block_reaches(function, *then_target, write_block, &allowed);
        let else_reaches = block_reaches(function, *else_target, write_block, &allowed);
        if then_reaches == else_reaches {
            continue;
        }
        if !vector_condition_is_comparison(function, *condition) {
            continue;
        }
        return Some((accumulator, addend, seed, *condition));
    }
    None
}

fn block_reaches(
    function: &MirFunction,
    start: MirBlockId,
    target: MirBlockId,
    allowed: &HashSet<MirBlockId>,
) -> bool {
    let mut pending = vec![start];
    let mut seen = HashSet::new();
    while let Some(block_id) = pending.pop() {
        if block_id == target {
            return true;
        }
        if !allowed.contains(&block_id) || !seen.insert(block_id) {
            continue;
        }
        if let Some(block) = function.blocks.iter().find(|block| block.id == block_id) {
            pending.extend(block.terminator.targets());
        }
    }
    false
}

fn vector_condition_is_comparison(function: &MirFunction, value: MirValueId) -> bool {
    find_value_instruction(function, value).is_some_and(|instruction| {
        matches!(
            &instruction.operation,
            MirOperation::Binary { op, .. } if op.is_comparison()
        )
    })
}

fn vector_early_exit_proven(
    function: &MirFunction,
    body_blocks: &[MirBlockId],
    exit: Option<MirBlockId>,
    cursor: Option<MirValueId>,
    cursor_place: Option<MirPlaceId>,
) -> bool {
    let (Some(exit), Some(cursor)) = (exit, cursor) else {
        return false;
    };
    let mut candidates = Vec::new();
    for block_id in body_blocks {
        let Some(block) = function.blocks.iter().find(|block| block.id == *block_id) else {
            continue;
        };
        let MirTerminator::Branch {
            condition,
            then_target,
            else_target,
        } = &block.terminator
        else {
            continue;
        };
        for (target, other) in [(*then_target, *else_target), (*else_target, *then_target)] {
            if target == other || other == exit {
                continue;
            }
            let Some(target_block) = function
                .blocks
                .iter()
                .find(|candidate| candidate.id == target)
            else {
                continue;
            };
            let value = match &target_block.terminator {
                // Packed early search currently lowers only a function
                // `return` carrying the cursor. A `BreakValue` must stay on
                // the scalar CFG until its loop-result carrier has a checked
                // packed representation.
                MirTerminator::Return { value: Some(value) } => *value,
                _ => continue,
            };
            if vector_condition_is_comparison(function, *condition)
                && value_depends_on_cursor(
                    function,
                    *condition,
                    cursor,
                    cursor_place,
                    &mut HashSet::new(),
                )
                && value_depends_on_cursor(
                    function,
                    value,
                    cursor,
                    cursor_place,
                    &mut HashSet::new(),
                )
            {
                candidates.push((block.id, target));
            }
        }
    }
    if candidates.len() != 1 {
        return false;
    }
    let early_targets = candidates
        .iter()
        .map(|(_, target)| *target)
        .collect::<HashSet<_>>();
    body_blocks.iter().all(|block_id| {
        let Some(block) = function.blocks.iter().find(|block| block.id == *block_id) else {
            return false;
        };
        match &block.terminator {
            MirTerminator::Break {
                target: break_target,
                ..
            } if *break_target == exit => false,
            MirTerminator::Return { .. } => early_targets.contains(block_id),
            _ => true,
        }
    })
}

fn value_depends_on_cursor(
    function: &MirFunction,
    value: MirValueId,
    cursor: MirValueId,
    cursor_place: Option<MirPlaceId>,
    seen: &mut HashSet<MirValueId>,
) -> bool {
    let is_cursor = cursor_value_matches(function, value, cursor, cursor_place);
    if is_cursor || !seen.insert(value) {
        return is_cursor;
    }
    let Some(instruction) = find_value_instruction(function, value) else {
        return false;
    };
    match &instruction.operation {
        MirOperation::LoopRangeValue {
            cursor: source_cursor, ..
        }
        | MirOperation::LoopIterValue {
            cursor: source_cursor, ..
        } => *source_cursor == cursor,
        MirOperation::Copy { value }
        | MirOperation::Move { value }
        | MirOperation::AttachTag { value, .. }
        | MirOperation::Unary { value, .. }
        | MirOperation::Convert { value, .. } => {
            value_depends_on_cursor(function, *value, cursor, cursor_place, seen)
        }
        MirOperation::Binary { left, right, .. } => {
            value_depends_on_cursor(function, *left, cursor, cursor_place, seen)
                || value_depends_on_cursor(function, *right, cursor, cursor_place, seen)
        }
        MirOperation::Phi { incoming } => incoming
            .iter()
            .any(|(_, value)| value_depends_on_cursor(function, *value, cursor, cursor_place, seen)),
        MirOperation::Index { base, index, .. } => {
            cursor_value_matches(function, *index, cursor, cursor_place)
                || value_depends_on_cursor(function, *base, cursor, cursor_place, seen)
        }
        MirOperation::Field { base, .. } => {
            value_depends_on_cursor(function, *base, cursor, cursor_place, seen)
        }
        MirOperation::Semantic(MirSemanticOp::ColumnarRead { base, index, .. }) => {
            cursor_value_matches(function, *index, cursor, cursor_place)
                || value_depends_on_cursor(function, *base, cursor, cursor_place, seen)
        }
        MirOperation::ReadPlace(place) => {
            place_has_cursor_index(function, *place, cursor, cursor_place)
        }
        _ => false,
    }
}

fn is_reduction_type(ty: &MirType) -> bool {
    matches!(ty.kind(), MirTypeKind::Float | MirTypeKind::Float32)
}

fn vector_rmw(instructions: &[&crate::MIR::MirInstruction]) -> Option<MirBinaryOp> {
    instructions.iter().find_map(|instruction| {
        let MirOperation::WritePlace { place, value } = &instruction.operation else {
            return None;
        };
        let defining = instructions.iter().find(|candidate| candidate.result == Some(*value))?;
        let MirOperation::Binary {
            op, left, right, ..
        } = &defining.operation
        else {
            return None;
        };
        let reads_place = [left, right].into_iter().any(|operand| {
            instructions.iter().any(|candidate| {
                candidate.result == Some(*operand)
                    && matches!(&candidate.operation, MirOperation::ReadPlace(read) if *read == *place)
            })
        });
        reads_place.then_some(*op)
    })
}
fn vector_rmw_accumulator_type(
    function: &MirFunction,
    instructions: &[&crate::MIR::MirInstruction],
) -> Option<MirType> {
    instructions.iter().find_map(|instruction| {
        let MirOperation::WritePlace { place, value } = &instruction.operation else {
            return None;
        };
        let defining = instructions.iter().find(|candidate| candidate.result == Some(*value))?;
        let MirOperation::Binary { left, right, .. } = &defining.operation else {
            return None;
        };
        let reads_place = [left, right].into_iter().any(|operand| {
            instructions.iter().any(|candidate| {
                candidate.result == Some(*operand)
                    && matches!(&candidate.operation, MirOperation::ReadPlace(read) if *read == *place)
            })
        });
        reads_place.then(|| {
            function
                .places
                .iter()
                .find(|candidate| candidate.id == *place)
                .map(|candidate| candidate.ty.clone())
        })?
    })
}


fn vector_has_indexed_input(
    function: &MirFunction,
    instructions: &[&crate::MIR::MirInstruction],
    cursor: MirValueId,
    cursor_place: Option<MirPlaceId>,
) -> bool {
    instructions.iter().any(|instruction| match &instruction.operation {
        MirOperation::ReadPlace(place) | MirOperation::MovePlace { place } => {
            place_has_cursor_index(function, *place, cursor, cursor_place)
        }
        MirOperation::Index { index, .. } => {
            cursor_value_matches(function, *index, cursor, cursor_place)
        }
        MirOperation::Semantic(MirSemanticOp::ColumnarRead { index, .. }) => {
            cursor_value_matches(function, *index, cursor, cursor_place)
        }
        MirOperation::Field { base, .. } => {
            value_has_cursor_index(function, *base, cursor, cursor_place, &mut HashSet::new())
        }
        _ => false,
    })
}

fn value_has_cursor_index(
    function: &MirFunction,
    value: MirValueId,
    cursor: MirValueId,
    cursor_place: Option<MirPlaceId>,
    seen: &mut HashSet<MirValueId>,
) -> bool {
    if !seen.insert(value) {
        return false;
    }
    let Some(instruction) = find_value_instruction(function, value) else {
        return false;
    };
    match &instruction.operation {
        MirOperation::Index { index, .. } => {
            cursor_value_matches(function, *index, cursor, cursor_place)
        }
        MirOperation::ReadPlace(place) | MirOperation::MovePlace { place } => {
            place_has_cursor_index(function, *place, cursor, cursor_place)
        }
        MirOperation::Semantic(MirSemanticOp::ColumnarRead { index, .. }) => {
            cursor_value_matches(function, *index, cursor, cursor_place)
        }
        MirOperation::Field { base, .. }
        | MirOperation::Copy { value: base }
        | MirOperation::Move { value: base }
        | MirOperation::AttachTag { value: base, .. } => {
            value_has_cursor_index(function, *base, cursor, cursor_place, seen)
        }
        _ => false,
    }
}

fn find_value_instruction<'a>(
    function: &'a MirFunction,
    value: MirValueId,
) -> Option<&'a crate::MIR::MirInstruction> {
    function
        .blocks
        .iter()
        .flat_map(|block| block.instructions.iter())
        .find(|instruction| instruction.result == Some(value))
}

fn place_has_cursor_index(
    function: &MirFunction,
    place_id: MirPlaceId,
    cursor: MirValueId,
    cursor_place: Option<MirPlaceId>,
) -> bool {
    function
        .places
        .iter()
        .find(|place| place.id == place_id)
        .is_some_and(|place| {
            place.projections.iter().any(|projection| {
                matches!(
                    projection,
                    MirProjection::Index { index, .. }
                        if cursor_value_matches(function, *index, cursor, cursor_place)
                )
            })
        })
}

fn vector_accesses(
    function: &MirFunction,
    blocks: &[MirBlockId],
    cursor: Option<MirValueId>,
    cursor_place: Option<MirPlaceId>,
    type_defs: &[MirTypeDef],
) -> Vec<MirVectorAccess> {
    let mut accesses = Vec::new();
    for block_id in blocks {
        let Some(block) = function.blocks.iter().find(|block| block.id == *block_id) else {
            continue;
        };
        for instruction in &block.instructions {
            for place_id in operation_place_refs(&instruction.operation) {
                let Some(place) = function.places.iter().find(|place| place.id == place_id) else {
                    continue;
                };
                let field = place.projections.iter().rev().find_map(|projection| match projection {
                    MirProjection::Field { field, .. } => Some(*field),
                    _ => None,
                });
                let (layout, column_index) =
                    vector_place_layout(function, place, field, type_defs);
                accesses.push(MirVectorAccess {
                    root: MirVectorAccessRoot::Place(place_id),
                    field,
                    layout,
                    column_index,
                });
            }
            match &instruction.operation {
                MirOperation::Index { base, index, kind, .. }
                    if cursor.is_some_and(|cursor| {
                        cursor_value_matches(function, *index, cursor, cursor_place)
                    }) =>
                {
                    accesses.push(MirVectorAccess {
                        root: value_access_root(function, *base),
                        field: None,
                        layout: MirVectorLayout::Flat,
                        column_index: None,
                    });
                    let _ = kind;
                }
                MirOperation::Semantic(MirSemanticOp::ColumnarRead {
                    base,
                    column,
                    column_index,
                    ..
                }) => accesses.push(MirVectorAccess {
                    root: MirVectorAccessRoot::Value(*base),
                    field: Some(*column),
                    layout: MirVectorLayout::ColumnarDirect,
                    column_index: Some(*column_index),
                }),
                MirOperation::Field { base, field } => {
                    if let Some((root, layout, column_index)) =
                        vector_indexed_field_access(
                            function,
                            *base,
                            *field,
                            cursor,
                            cursor_place,
                            type_defs,
                        )
                    {
                        accesses.push(MirVectorAccess {
                            root,
                            field: Some(*field),
                            layout,
                            column_index,
                        });
                    }
                }
                _ => {}
            }
        }
    }
    accesses.sort_unstable();
    accesses.dedup();
    accesses
}

fn vector_place_layout(
    function: &MirFunction,
    place: &MirPlace,
    field: Option<MirFieldId>,
    type_defs: &[MirTypeDef],
) -> (MirVectorLayout, Option<usize>) {
    let Some(field) = field else {
        return (MirVectorLayout::Flat, None);
    };
    let Some(base_type) = place_base_type(function, &place.base) else {
        return (MirVectorLayout::AosStrided, None);
    };
    let Some(element) = list_element_type(&base_type) else {
        return (MirVectorLayout::AosStrided, None);
    };
    if struct_layout(element, type_defs) == Some(MirStructLayout::Columnar) {
        (
            MirVectorLayout::ColumnarDirect,
            field_column_index(element, field, type_defs),
        )
    } else {
        (MirVectorLayout::AosStrided, None)
    }
}

fn vector_indexed_field_access(
    function: &MirFunction,
    base: MirValueId,
    field: MirFieldId,
    cursor: Option<MirValueId>,
    cursor_place: Option<MirPlaceId>,
    type_defs: &[MirTypeDef],
) -> Option<(MirVectorAccessRoot, MirVectorLayout, Option<usize>)> {
    let instruction = find_value_instruction(function, base)?;
    let MirOperation::Index {
        base: collection,
        index,
        kind: MirIndexKind::List | MirIndexKind::FixedListProof,
        ..
    } = &instruction.operation
    else {
        return None;
    };
    if !cursor.is_some_and(|cursor| cursor_value_matches(function, *index, cursor, cursor_place)) {
        return None;
    }
    let collection_type = value_type(function, *collection)?;
    let element = list_element_type(&collection_type)?;
    let layout = if struct_layout(element, type_defs) == Some(MirStructLayout::Columnar) {
        MirVectorLayout::ColumnarDirect
    } else {
        MirVectorLayout::AosStrided
    };
    Some((
        value_access_root(function, *collection),
        layout,
        (layout == MirVectorLayout::ColumnarDirect)
            .then(|| field_column_index(element, field, type_defs))
            .flatten(),
    ))
}

fn place_base_type(function: &MirFunction, base: &MirPlaceBase) -> Option<MirType> {
    match base {
        MirPlaceBase::Local(local) => function
            .locals
            .iter()
            .find(|candidate| candidate.id == *local)
            .map(|local| local.ty.clone()),
        MirPlaceBase::Parameter(value)
        | MirPlaceBase::Capture(value)
        | MirPlaceBase::Temporary(value) => value_type(function, *value),
        MirPlaceBase::Static(_) => None,
    }
}

fn value_type(function: &MirFunction, value: MirValueId) -> Option<MirType> {
    function
        .values
        .iter()
        .find(|(candidate, ..)| *candidate == value)
        .map(|(_, ty, ..)| ty.clone())
}

fn value_access_root(function: &MirFunction, value: MirValueId) -> MirVectorAccessRoot {
    find_value_instruction(function, value)
        .and_then(|instruction| match &instruction.operation {
            MirOperation::ReadPlace(place) | MirOperation::MovePlace { place } => {
                Some(MirVectorAccessRoot::Place(*place))
            }
            MirOperation::Copy { value }
            | MirOperation::Move { value }
            | MirOperation::AttachTag { value, .. }
            | MirOperation::Field { base: value, .. }
            | MirOperation::Index { base: value, .. } => {
                Some(value_access_root(function, *value))
            }
            MirOperation::Semantic(MirSemanticOp::ColumnarRead { base, .. }) => {
                Some(value_access_root(function, *base))
            }
            _ => None,
        })
        .unwrap_or(MirVectorAccessRoot::Value(value))
}

fn list_element_type(ty: &MirType) -> Option<&MirType> {
    match ty.kind() {
        MirTypeKind::List(inner) | MirTypeKind::FixedList { elem: inner, .. } => Some(inner),
        MirTypeKind::Tagged { inner, .. } => list_element_type(inner),
        _ => None,
    }
}

fn struct_layout(ty: &MirType, type_defs: &[MirTypeDef]) -> Option<MirStructLayout> {
    let id = ty.identity.or_else(|| match ty.kind() {
        MirTypeKind::Apply { name, .. } => Some(name.id),
        _ => None,
    })?;
    type_defs.iter().find(|definition| definition.id == id)?.layout
}

fn field_column_index(
    element: &MirType,
    field: MirFieldId,
    type_defs: &[MirTypeDef],
) -> Option<usize> {
    let id = element.identity.or_else(|| match element.kind() {
        MirTypeKind::Apply { name, .. } => Some(name.id),
        _ => None,
    })?;
    let definition = type_defs.iter().find(|definition| definition.id == id)?;
    let MirTypeDefKind::Struct { fields, .. } = &definition.kind else {
        return None;
    };
    fields
        .iter()
        .filter(|field| !field.computed)
        .position(|candidate| candidate.id == field)
}

fn value_root_key(function: &MirFunction, value: MirValueId) -> String {
    fn inner(
        function: &MirFunction,
        value: MirValueId,
        seen: &mut HashSet<MirValueId>,
    ) -> String {
        if !seen.insert(value) {
            return format!("value:{}", value.0);
        }
        let Some(instruction) = find_value_instruction(function, value) else {
            return format!("value:{}", value.0);
        };
        match &instruction.operation {
            MirOperation::ReadPlace(place) | MirOperation::MovePlace { place } => {
                function
                    .places
                    .iter()
                    .find(|candidate| candidate.id == *place)
                    .map(place_root)
                    .unwrap_or_else(|| format!("value:{}", value.0))
            }
            MirOperation::Parameter { index, .. } => format!("parameter:{index}"),
            MirOperation::Capture { slot } => format!("capture:{slot}"),
            MirOperation::Copy { value }
            | MirOperation::Move { value }
            | MirOperation::AttachTag { value, .. }
            | MirOperation::Field { base: value, .. }
            | MirOperation::Index { base: value, .. } => inner(function, *value, seen),
            MirOperation::Semantic(MirSemanticOp::ColumnarRead { base, .. }) => {
                inner(function, *base, seen)
            }
            MirOperation::Phi { incoming } => incoming
                .first()
                .map(|(_, value)| inner(function, *value, seen))
                .unwrap_or_else(|| format!("value:{}", value.0)),
            _ => format!("value:{}", value.0),
        }
    }
    inner(function, value, &mut HashSet::new())
}

fn vector_operation_has_effect(
    function: &MirFunction,
    operation: &MirOperation,
    prelude_calls: &HashMap<MirPreludeCallId, &MirPreludeCall>,
) -> bool {
    match operation {
        MirOperation::CoreCall { route, fallibility, .. } => {
            !prelude_call_is_pure(prelude_calls, *route)
                || !matches!(fallibility, MirCallFallibility::Infallible)
        }
        MirOperation::Todo { call, .. }
        | MirOperation::Index { call, .. }
        | MirOperation::Slice { call, .. } => !prelude_call_is_pure(prelude_calls, *call),
        MirOperation::Convert { conversion, .. } => !conversion_is_pure(conversion, prelude_calls),
        MirOperation::Semantic(operation) => semantic_operation_has_effect(operation, prelude_calls),
        MirOperation::LoopRangeInit { call, .. }
        | MirOperation::LoopRangeHasNext { call, .. }
        | MirOperation::LoopRangeValue { call, .. }
        | MirOperation::LoopRangeAdvance { call, .. }
        | MirOperation::LoopIterInit { call, .. }
        | MirOperation::LoopIterHasNext { call, .. }
        | MirOperation::LoopIterValue { call, .. }
        | MirOperation::LoopIterAdvance { call, .. } => !prelude_call_is_pure(prelude_calls, *call),
        MirOperation::ReadPlace(place) => function
            .places
            .iter()
            .find(|candidate| candidate.id == *place)
            .is_none_or(|place| place_projection_has_effect(place, prelude_calls)),
        MirOperation::MovePlace { .. } => true,
        MirOperation::Binary {
            dispatch: crate::MIR::MirBinaryDispatch::Prelude { call, .. },
            ..
        } => !prelude_call_is_pure(prelude_calls, *call),
        MirOperation::Call {
            callee: MirCallee::Prelude(call),
            ..
        } => !prelude_call_is_pure(prelude_calls, *call),
        MirOperation::Call { .. }
        | MirOperation::IndirectCall { .. }
        | MirOperation::Drop { .. }
        | MirOperation::ScopeEnter { .. }
        | MirOperation::ScopeExit { .. }
        | MirOperation::Global { .. }
        | MirOperation::BuildList { .. }
        | MirOperation::BuildMap { .. }
        | MirOperation::Closure { .. } => true,
        MirOperation::WritePlace { place, .. } => function
            .places
            .iter()
            .find(|candidate| candidate.id == *place)
            .is_some_and(|place| {
                matches!(
                    &place.base,
                    MirPlaceBase::Static(_)
                        | MirPlaceBase::Parameter(_)
                        | MirPlaceBase::Capture(_)
                ) || place_projection_has_effect(place, prelude_calls)
            }),
        MirOperation::InitializeUninit { .. } => true,
        _ => false,
    }
}

fn place_projection_has_effect(
    place: &MirPlace,
    prelude_calls: &HashMap<MirPreludeCallId, &MirPreludeCall>,
) -> bool {
    place.projections.iter().any(|projection| match projection {
        MirProjection::Index {
            call, write_call, ..
        } => {
            !prelude_call_is_pure(prelude_calls, *call)
                || write_call.is_some_and(|call| !prelude_call_is_pure(prelude_calls, call))
        }
        MirProjection::Field { .. } | MirProjection::Deref { .. } => false,
    })
}

fn semantic_operation_has_effect(
    operation: &MirSemanticOp,
    prelude_calls: &HashMap<MirPreludeCallId, &MirPreludeCall>,
) -> bool {
    let calls = operation.prelude_calls();
    calls.is_empty() || calls.into_iter().any(|call| !prelude_call_is_pure(prelude_calls, call))
}

fn vector_memory_facts(
    function: &MirFunction,
    blocks: &[MirBlockId],
    cursor: Option<MirValueId>,
    cursor_place: Option<MirPlaceId>,
    rule: MirVectorRule,
) -> (bool, bool) {
    let mut reads = BTreeSet::new();
    let mut writes = BTreeSet::new();
    let mut indexed_reads = BTreeSet::new();
    let mut indexed_writes = BTreeSet::new();
    let mut dynamic = false;
    for block_id in blocks {
        let Some(block) = function.blocks.iter().find(|block| block.id == *block_id) else {
            continue;
        };
        for instruction in &block.instructions {
            for place_id in operation_place_refs(&instruction.operation) {
                let Some(place) = function.places.iter().find(|place| place.id == place_id) else {
                    continue;
                };
                if place.projections.iter().any(|projection| {
                    matches!(
                        projection,
                        MirProjection::Index {
                            kind: MirIndexKind::Map | MirIndexKind::Pool | MirIndexKind::Lane,
                            ..
                        } | MirProjection::Deref { .. }
                    )
                }) {
                    dynamic = true;
                }
                let root = place_root(place);
                let indexed = cursor.is_some_and(|cursor| {
                    place_has_cursor_index(function, place_id, cursor, cursor_place)
                });
                match &instruction.operation {
                    MirOperation::ReadPlace(_) => {
                        reads.insert(root.clone());
                        if indexed {
                            indexed_reads.insert(root);
                        }
                    }
                    MirOperation::MovePlace { .. }
                    | MirOperation::WritePlace { .. }
                    | MirOperation::InitializeUninit { .. } => {
                        writes.insert(root.clone());
                        if indexed {
                            indexed_writes.insert(root);
                        }
                    }
                    MirOperation::AddressOf {
                        access: MirAccess::Write,
                        ..
                    } => {
                        writes.insert(root.clone());
                        if indexed {
                            indexed_writes.insert(root);
                        }
                    }
                    _ => {}
                }
            }
            match &instruction.operation {
                MirOperation::Index {
                    base, index, kind, ..
                } => {
                    if matches!(kind, MirIndexKind::Map | MirIndexKind::Pool | MirIndexKind::Lane) {
                        dynamic = true;
                    }
                    let root = value_root_key(function, *base);
                    reads.insert(root.clone());
                    if cursor.is_some_and(|cursor| {
                        cursor_value_matches(function, *index, cursor, cursor_place)
                    }) {
                        indexed_reads.insert(root);
                    }
                }
                MirOperation::Field { base, .. } => {
                    let root = value_root_key(function, *base);
                    reads.insert(root);
                    if cursor.is_some_and(|cursor| {
                        value_has_cursor_index(
                            function,
                            *base,
                            cursor,
                            cursor_place,
                            &mut HashSet::new(),
                        )
                    }) {
                        indexed_reads.insert(value_root_key(function, *base));
                    }
                }
                MirOperation::Semantic(MirSemanticOp::ColumnarRead { base, index, .. }) => {
                    let root = value_root_key(function, *base);
                    reads.insert(root.clone());
                    if cursor.is_some_and(|cursor| {
                        cursor_value_matches(function, *index, cursor, cursor_place)
                    }) {
                        indexed_reads.insert(root);
                    }
                }
                _ => {}
            }
        }
    }
    if dynamic {
        return (false, false);
    }
    let no_aliasing = writes.is_empty()
        || writes.iter().all(|root| {
            !root.starts_with("parameter:")
                && !root.starts_with("capture:")
                && !root.starts_with("static:")
                && !root.starts_with("temporary:")
        });
    let mut no_cross = true;
    for root in writes.intersection(&reads) {
        let same_iteration = indexed_writes.contains(root) && indexed_reads.contains(root);
        let loop_carried_accumulator = matches!(
            rule,
            MirVectorRule::ConditionalAccumulate | MirVectorRule::Reduction
        ) && !indexed_writes.contains(root);
        if !same_iteration && !loop_carried_accumulator {
            no_cross = false;
            break;
        }
    }
    (no_aliasing, no_cross)
}

fn place_root(place: &MirPlace) -> String {
    match &place.base {
        MirPlaceBase::Local(local) => format!("local:{}", local.0),
        MirPlaceBase::Parameter(value) => format!("parameter:{}", value.0),
        MirPlaceBase::Capture(value) => format!("capture:{}", value.0),
        MirPlaceBase::Temporary(value) => format!("temporary:{}", value.0),
        MirPlaceBase::Static(name) => format!("static:{name}"),
    }
}

fn is_default_int_type(ty: &MirType) -> bool {
    match ty.kind() {
        MirTypeKind::Int => true,
        MirTypeKind::InlineRange { base, .. }
        | MirTypeKind::Tagged { inner: base, .. }
        | MirTypeKind::Quantity { base, .. } => is_default_int_type(base),
        _ => false,
    }
}

fn is_packable_type(ty: &MirType) -> bool {
    is_default_int_type(ty)
        || matches!(
            ty.kind(),
            MirTypeKind::Float | MirTypeKind::Float32 | MirTypeKind::IntN { .. }
        )
}

fn is_packable_for_rule(rule: MirVectorRule, ty: &MirType) -> bool {
    is_packable_type(ty)
        && (!is_default_int_type(ty) || rule == MirVectorRule::EarlyExitSearch)
}

fn lane_width(ty: Option<&MirType>) -> Option<u16> {
    let ty = ty?;
    if is_default_int_type(ty) {
        return Some(2);
    }
    match ty.kind() {
        MirTypeKind::Float32 => Some(4),
        MirTypeKind::Float => Some(2),
        MirTypeKind::IntN { bits, .. } if *bits <= 32 => Some(4),
        MirTypeKind::IntN { .. } => Some(2),
        _ => None,
    }
}

fn canonicalize_program_order(program: &mut MirProgram) {
    program.modules.sort_by_key(|row| (row.id, row.key.clone()));
    program.imports.sort_by_key(|row| (row.id, row.module));
    program.functions.sort_by_key(|function| (function.id, function.key.clone()));
    program.types.sort_by_key(|ty| (ty.id, ty.key.clone()));
    program.traits.sort_by_key(|row| (row.id, row.key.clone()));
    program.impls.sort_by_key(|row| (row.id, row.key.clone()));
    program.constants.sort_by_key(|row| (row.id, row.key.clone()));
    program.type_instances.sort_by_key(|ty| (ty.identity, ty.canonical_key()));
    program.fields.sort_by_key(|row| (row.id, row.owner));
    program.source_files.sort_by_key(|file| (file.id, file.path.clone(), file.source.clone()));
    program.core_calls.sort_by_key(|call| (call.id, call.key.clone()));
    program.prelude_calls.sort_by_key(|call| (call.id, call.module.clone(), call.member.clone()));
    program.foreign.sort_by_key(|foreign| (foreign.id, foreign.key.clone()));
    program.links.sort_by_key(|link| (link.id, link.crate_spec.clone()));
    program.callbacks.sort_by_key(|callback| (callback.id, callback.symbol.clone()));
    program.handles.sort_by_key(|handle| handle.id);
    program.jobs.sort_by_key(|job| (job.id, job.name.clone()));
    program.tests.sort_by_key(|test| (test.id, test.name.clone()));
    program.harnesses.sort_by_key(|harness| harness.id);
    program.artifacts.sort_by_key(|artifact| (artifact.id, artifact.name.clone()));
    program.unreachable.sort_by_key(|row| (row.span.start, row.span.end, row.construct.clone()));
    for import in &mut program.imports {
        if let MirImportKind::Unqualified { items, .. } = &mut import.kind {
            items.sort_by_key(|item| (item.local.clone(), item.original.clone()));
        }
    }
    for trait_def in &mut program.traits {
        trait_def.methods.sort_by_key(|method| (method.id, method.name.clone()));
    }
    for impl_def in &mut program.impls {
        impl_def.associated_types.sort_by_key(|value| (value.name.clone(), value.span.start, value.span.end));
        impl_def.methods.sort();
    }
    for link in &mut program.links {
        link.link_closure.sort();
    }
    for harness in &mut program.harnesses {
        harness.tests.sort();
        harness.output_checks.sort_by_key(|check| (check.id, check.name.clone()));
        harness.coverage_points.sort_by_key(|point| (point.id, point.function, point.block));
    }
    for artifact in &mut program.artifacts {
        artifact.modules.sort();
        artifact.links.sort();
        artifact.exports.sort_by_key(|export| (export.symbol.clone(), export.function));
    }
    for function in &mut program.functions {
        function.blocks.sort_by_key(|block| block.id);
        function.capture_params.sort_by_key(|capture| (capture.slot, capture.name.clone()));
        function.values.sort_by_key(|(value, _, _, _)| *value);
        function.locals.sort_by_key(|local| local.id);
        function.places.sort_by_key(|place| place.id);
        function.scopes.sort_by_key(|scope| scope.id);
        function.drops.sort_by_key(|drop| (drop.place, drop.span.start, drop.span.end));
    }
}

/// Canonical semantic bytes for a MIR program.  Every semantic row is encoded
/// explicitly; source text remains part of source-file identity, while types
/// use only their neutral `MirTypeKind` representation.
pub fn mir_program_bytes(program: &MirProgram) -> Vec<u8> {
    let mut writer = CanonicalWriter::default();
    writer.tag("mir");
    writer.u16(program.schema_version);
    writer.str(&program.package_identity);
    writer.str(&program.facts.project_root);
    writer.str(&program.facts.edition);
    encode_runtime_parts(&mut writer, &program.facts.runtime_parts);
    writer.str(&program.facts.active_os);
    writer.str(&program.facts.inferred_layer);
    writer.str(&program.facts.allocator);
    let dossier = &program.facts.target_dossier;
    let target_triple = dossier
        .machine
        .as_deref()
        .map(|machine| machine.triple.as_str())
        .unwrap_or_default();
    let dossier_bytes = dossier.cache_bytes(target_triple);
    writer.len(dossier_bytes.len());
    writer.bytes.extend_from_slice(&dossier_bytes);
    encode_app_graph(&mut writer, program.facts.web_app.as_ref());
    encode_hardware_setups(&mut writer, &program.facts.hardware_setups);
    writer.str(&program.facts.hardware_profile_id);
    match program.facts.hardware_profile.as_ref() {
        Some(profile) => {
            writer.bool(true);
            encode_target_hardware_facts(&mut writer, profile);
        }
        None => writer.bool(false),
    }
    encode_target_hardware_use(&mut writer, &program.facts.hardware_use);
    let mut hardware_capabilities = program.facts.hardware_capabilities.iter().collect::<Vec<_>>();
    hardware_capabilities.sort_unstable();
    writer.len(hardware_capabilities.len());
    for capability in hardware_capabilities {
        writer.str(capability);
    }

    let mut modules = program.modules.iter().collect::<Vec<_>>();
    modules.sort_by_key(|row| (row.id, row.key.as_str()));
    writer.len(modules.len());
    for row in modules {
        encode_module(&mut writer, row);
    }
    let mut imports = program.imports.iter().collect::<Vec<_>>();
    imports.sort_by_key(|row| (row.id, row.module));
    writer.len(imports.len());
    for row in imports {
        encode_import(&mut writer, row);
    }
    let mut types = program.types.iter().collect::<Vec<_>>();
    types.sort_by_key(|ty| (ty.id, ty.key.as_str()));
    writer.len(types.len());
    for ty in types {
        encode_type_def(&mut writer, ty);
    }
    let mut traits = program.traits.iter().collect::<Vec<_>>();
    traits.sort_by_key(|row| (row.id, row.key.as_str()));
    writer.len(traits.len());
    for row in traits {
        encode_trait_def(&mut writer, row);
    }
    let mut impls = program.impls.iter().collect::<Vec<_>>();
    impls.sort_by_key(|row| (row.id, row.key.as_str()));
    writer.len(impls.len());
    for row in impls {
        encode_impl_def(&mut writer, row);
    }
    let mut constants = program.constants.iter().collect::<Vec<_>>();
    constants.sort_by_key(|row| (row.id, row.key.as_str()));
    writer.len(constants.len());
    for row in constants {
        encode_constant_def(&mut writer, row);
    }
    let mut fields = program.fields.iter().collect::<Vec<_>>();
    fields.sort_by_key(|row| (row.id, row.owner));
    writer.len(fields.len());
    for row in fields {
        writer.u64(row.id.0);
        writer.u64(row.owner.0);
        encode_field(&mut writer, &row.field);
    }
    let mut source_files = program.source_files.iter().collect::<Vec<_>>();
    source_files.sort_by_key(|source| (source.id, source.path.as_str(), source.source.as_str()));
    writer.len(source_files.len());
    for source in source_files {
        writer.u64(source.id.0);
        writer.str(&source.path);
        writer.str(&source.source);
    }
    let mut functions = program.functions.iter().collect::<Vec<_>>();
    functions.sort_by_key(|function| (function.id, function.key.as_str()));
    writer.len(functions.len());
    for function in functions {
        encode_function(&mut writer, function);
    }
    let mut foreign = program.foreign.iter().collect::<Vec<_>>();
    foreign.sort_by_key(|foreign| (foreign.id, foreign.key.as_str()));
    writer.len(foreign.len());
    for row in foreign {
        encode_foreign(&mut writer, row);
    }
    let mut links = program.links.iter().collect::<Vec<_>>();
    links.sort_by_key(|link| (link.id, link.crate_spec.as_str()));
    writer.len(links.len());
    for row in links {
        encode_link_unit(&mut writer, row);
    }
    let mut callbacks = program.callbacks.iter().collect::<Vec<_>>();
    callbacks.sort_by_key(|callback| (callback.id, callback.symbol.as_str()));
    writer.len(callbacks.len());
    for row in callbacks {
        encode_callback(&mut writer, row);
    }
    let mut handles = program.handles.iter().collect::<Vec<_>>();
    handles.sort_by_key(|handle| handle.id);
    writer.len(handles.len());
    for row in handles {
        encode_handle(&mut writer, row);
    }
    let mut jobs = program.jobs.iter().collect::<Vec<_>>();
    jobs.sort_by_key(|job| (job.id, job.name.as_str()));
    writer.len(jobs.len());
    for row in jobs {
        encode_job(&mut writer, row);
    }
    let mut tests = program.tests.iter().collect::<Vec<_>>();
    tests.sort_by_key(|test| (test.id, test.name.as_str()));
    writer.len(tests.len());
    for row in tests {
        encode_test(&mut writer, row);
    }
    let mut harnesses = program.harnesses.iter().collect::<Vec<_>>();
    harnesses.sort_by_key(|harness| harness.id);
    writer.len(harnesses.len());
    for row in harnesses {
        encode_harness(&mut writer, row);
    }
    let mut artifacts = program.artifacts.iter().collect::<Vec<_>>();
    artifacts.sort_by_key(|artifact| (artifact.id, artifact.name.as_str()));
    writer.len(artifacts.len());
    for row in artifacts {
        encode_artifact(&mut writer, row);
    }
    let mut core_calls = program.core_calls.iter().collect::<Vec<_>>();
    core_calls.sort_by_key(|call| (call.id, call.key.as_str()));
    writer.len(core_calls.len());
    for call in core_calls {
        encode_core_call(&mut writer, call);
    }
    let mut prelude_calls = program.prelude_calls.iter().collect::<Vec<_>>();
    prelude_calls.sort_by_key(|call| (call.id, call.module.as_str(), call.member.as_str()));
    writer.len(prelude_calls.len());
    for call in prelude_calls {
        encode_prelude_call(&mut writer, call);
    }
    let mut type_instances = program.type_instances.iter().collect::<Vec<_>>();
    type_instances.sort_by_key(|ty| (ty.identity, ty.canonical_key()));
    writer.len(type_instances.len());
    for ty in type_instances {
        writer.mir_type(ty);
    }
    let mut unreachable = program.unreachable.iter().collect::<Vec<_>>();
    unreachable.sort_by_key(|row| (row.span.start, row.span.end, row.construct.as_str()));
    writer.len(unreachable.len());
    for row in unreachable {
        writer.str(&row.construct);
        writer.span(row.span);
        encode_erasure_reason(&mut writer, row.reason);
    }
    writer.finish()
}

/// A deterministic 256-bit digest over [`mir_program_bytes`].
pub fn mir_program_digest(program: &MirProgram) -> [u8; 32] {
    let bytes = mir_program_bytes(program);
    let seeds = [
        0xcbf29ce484222325u64,
        0x84222325cbf29ce4u64,
        0x9e3779b185ebca87u64,
        0xd6e8feb86659fd93u64,
    ];
    let mut output = [0u8; 32];
    for (lane, seed) in seeds.into_iter().enumerate() {
        let mut hash = seed;
        for byte in &bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
        output[lane * 8..lane * 8 + 8].copy_from_slice(&hash.to_le_bytes());
    }
    output
}

impl MirProgram {
    pub fn deterministic_bytes(&self) -> Vec<u8> {
        mir_program_bytes(self)
    }

    pub fn deterministic_digest(&self) -> [u8; 32] {
        mir_program_digest(self)
    }
}

#[derive(Default)]
struct CanonicalWriter {
    bytes: Vec<u8>,
}

impl CanonicalWriter {
    fn tag(&mut self, value: &str) { self.str(value); }
    fn str(&mut self, value: &str) { self.len(value.len()); self.bytes.extend_from_slice(value.as_bytes()); }
    fn len(&mut self, value: usize) { self.u64(value as u64); }
    fn u16(&mut self, value: u16) { self.bytes.extend_from_slice(&value.to_le_bytes()); }
    fn u64(&mut self, value: u64) { self.bytes.extend_from_slice(&value.to_le_bytes()); }
    fn bool(&mut self, value: bool) { self.bytes.push(u8::from(value)); }
    fn span(&mut self, span: Span) { self.len(span.start); self.len(span.end); }
    fn option_u64(&mut self, value: Option<u64>) { self.bool(value.is_some()); if let Some(value) = value { self.u64(value); } }
    fn debug<T: fmt::Debug>(&mut self, value: &T) { self.str(&format!("{value:?}")); }
    fn option_str(&mut self, value: Option<&str>) {
        self.bool(value.is_some());
        if let Some(value) = value {
            self.str(value);
        }
    }
    fn mir_type(&mut self, ty: &MirType) {
        self.tag("mir-type");
        self.option_u64(ty.identity.map(|identity| identity.0));
        self.mir_type_kind(ty.kind());
        self.debug(&ty.layout);
    }
    fn mir_type_kind(&mut self, kind: &MirTypeKind) {
        match kind {
            MirTypeKind::Int => self.tag("int"),
            MirTypeKind::Float => self.tag("float"),
            MirTypeKind::Bool => self.tag("bool"),
            MirTypeKind::String => self.tag("string"),
            MirTypeKind::Char => self.tag("char"),
            MirTypeKind::List(inner) => {
                self.tag("list");
                self.mir_type(inner);
            }
            MirTypeKind::Map { key, value } => {
                self.tag("map");
                self.mir_type(key);
                self.mir_type(value);
            }
            MirTypeKind::Shared(inner) => {
                self.tag("shared");
                self.mir_type(inner);
            }
            MirTypeKind::Option(inner) => {
                self.tag("option");
                self.mir_type(inner);
            }
            MirTypeKind::Result { ok, err } => {
                self.tag("result");
                self.mir_type(ok);
                self.mir_type(err);
            }
            MirTypeKind::Fn(signature) => {
                self.tag("fn");
                self.len(signature.params.len());
                for param in &signature.params {
                    self.mir_type(param);
                }
                match &signature.ret {
                    Some(ret) => {
                        self.tag("ret");
                        self.mir_type(ret);
                    }
                    None => self.tag("unit-ret"),
                }
                self.debug(&signature.effect_bound);
                self.debug(&signature.param_contract);
                self.debug(&signature.call_metadata);
                self.debug(&signature.return_view_provenance);
            }
            MirTypeKind::SendFn { params, ret } => {
                self.tag("send-fn");
                self.len(params.len());
                for param in params {
                    self.mir_type(param);
                }
                match ret {
                    Some(ret) => {
                        self.tag("ret");
                        self.mir_type(ret);
                    }
                    None => self.tag("unit-ret"),
                }
            }
            MirTypeKind::Apply { name, args } => {
                self.tag("apply");
                self.u64(name.id.0);
                self.len(args.len());
                for arg in args {
                    self.mir_type(arg);
                }
            }
            MirTypeKind::TraitObject(bounds) => {
                self.tag("trait-object");
                self.len(bounds.len());
                for bound in bounds {
                    self.u64(bound.id.0);
                }
            }
            MirTypeKind::Tuple(fields) => {
                self.tag("tuple");
                self.len(fields.len());
                for (name, field) in fields {
                    self.str(name);
                    self.mir_type(field);
                }
            }
            MirTypeKind::FixedList { elem, len } => {
                self.tag("fixed-list");
                self.mir_type(elem);
                self.debug(len);
            }
            MirTypeKind::IntN { signed, bits } => {
                self.tag("int-n");
                self.bool(*signed);
                self.u64(u64::from(*bits));
            }
            MirTypeKind::InlineRange { base, lo, hi } => {
                self.tag("inline-range");
                self.mir_type(base);
                self.u64(*lo as u64);
                self.u64(*hi as u64);
            }
            MirTypeKind::Float32 => self.tag("float32"),
            MirTypeKind::Tagged { marker, inner } => {
                self.tag("tagged");
                self.debug(marker);
                self.mir_type(inner);
            }
            MirTypeKind::Union(members) => {
                self.tag("union");
                self.len(members.len());
                for member in members {
                    self.mir_type(member);
                }
            }
            MirTypeKind::Quantity { base, dimension } => {
                self.tag("quantity");
                self.mir_type(base);
                self.debug(dimension);
            }
            MirTypeKind::Measure(measure) => {
                self.tag("measure");
                self.debug(measure);
            }
        }
    }
    fn operation(&mut self, operation: &MirOperation) {
        match operation {
            MirOperation::Constant(value) => {
                self.tag("constant");
                encode_mir_constant(self, value);
            }
            MirOperation::MovePlace { place } => {
                self.tag("move-place");
                self.u64(place.0);
            }
            MirOperation::Unary { op, value } => {
                self.tag("unary");
                self.debug(op);
                self.u64(value.0);
            }
            MirOperation::Binary {
                op,
                dispatch,
                left,
                right,
            } => {
                self.tag("binary");
                self.debug(op);
                self.debug(dispatch);
                self.u64(left.0);
                self.u64(right.0);
            }
            MirOperation::Convert {
                value,
                parameters,
                target,
                conversion,
            } => {
                self.tag("convert");
                self.u64(value.0);
                self.len(parameters.len());
                for parameter in parameters {
                    self.u64(parameter.0);
                }
                self.mir_type(target);
                encode_conversion(self, conversion);
            }
            MirOperation::Call {
                callee,
                args,
                type_args,
            } => {
                self.tag("call");
                encode_callee(self, callee);
                encode_call_args(self, args);
                self.len(type_args.len());
                for type_arg in type_args {
                    self.mir_type(type_arg);
                }
            }
            MirOperation::IndirectCall {
                callee,
                args,
                type_args,
            } => {
                self.tag("indirect-call");
                self.u64(callee.0);
                encode_call_args(self, args);
                self.len(type_args.len());
                for type_arg in type_args {
                    self.mir_type(type_arg);
                }
            }
            MirOperation::PtrFromAddr { addr, element } => {
                self.tag("ptr-from-addr");
                self.u64(addr.0);
                self.mir_type(element);
            }
            MirOperation::CoreCall {
                call,
                route,
                args,
                type_args,
                fallibility,
                data_plan,
            } => {
                self.tag("core-call");
                self.u64(call.0);
                self.u64(route.0);
                encode_call_args(self, args);
                self.len(type_args.len());
                for type_arg in type_args {
                    self.mir_type(type_arg);
                }
                encode_call_fallibility(self, fallibility);
                match data_plan {
                    Some(plan) => {
                        self.bool(true);
                        encode_data_plan(self, plan);
                    }
                    None => self.bool(false),
                }
            }
            MirOperation::Todo {
                call,
                location,
                expected_type,
            } => {
                self.tag("todo");
                self.u64(call.0);
                self.debug(location);
                match expected_type {
                    Some(ty) => {
                        self.bool(true);
                        self.mir_type(ty);
                    }
                    None => self.bool(false),
                }
            }
            MirOperation::LoopIterInit {
                call,
                collection,
                step,
                by_value,
                source_kind,
            } => {
                self.tag("loop-iter-init");
                self.u64(call.0);
                self.u64(collection.0);
                self.option_u64(step.map(|value| value.0));
                self.bool(*by_value);
                encode_loop_source_kind(self, source_kind);
            }
            MirOperation::InitializeUninit { place } => {
                self.tag("initialize-uninit");
                self.u64(place.0);
            }
            MirOperation::Semantic(operation) => {
                self.tag("semantic");
                encode_semantic_operation(self, operation);
            }
            _ => self.debug(operation),
        }
    }
    fn finish(self) -> Vec<u8> { self.bytes }
}

fn encode_item_ref(writer: &mut CanonicalWriter, item: &MirItemRef) {
    match item {
        MirItemRef::Type(id) => {
            writer.tag("type");
            writer.u64(id.0);
        }
        MirItemRef::Trait(id) => {
            writer.tag("trait");
            writer.u64(id.0);
        }
        MirItemRef::Function(id) => {
            writer.tag("function");
            writer.u64(id.0);
        }
        MirItemRef::Constant(id) => {
            writer.tag("constant");
            writer.u64(id.0);
        }
        MirItemRef::Impl(id) => {
            writer.tag("impl");
            writer.u64(id.0);
        }
        MirItemRef::Foreign(id) => {
            writer.tag("foreign");
            writer.u64(id.0);
        }
        MirItemRef::Import(id) => {
            writer.tag("import");
            writer.u64(id.0);
        }
    }
}

fn encode_trait_ref(writer: &mut CanonicalWriter, trait_ref: &crate::MIR::MirTraitRef) {
    writer.u64(trait_ref.id.0);
    writer.str(&trait_ref.name);
}
fn encode_visibility(writer: &mut CanonicalWriter, visibility: crate::MIR::MirVisibility) {
    writer.u64(match visibility {
        crate::MIR::MirVisibility::Private => 1,
        crate::MIR::MirVisibility::Package => 2,
        crate::MIR::MirVisibility::Public => 3,
    });
}

fn encode_access(writer: &mut CanonicalWriter, access: Option<crate::MIR::MirAccess>) {
    match access {
        Some(crate::MIR::MirAccess::Read) => writer.u64(1),
        Some(crate::MIR::MirAccess::Write) => writer.u64(2),
        Some(crate::MIR::MirAccess::Move) => writer.u64(3),
        None => writer.u64(0),
    }
}


fn encode_ownership_mode(writer: &mut CanonicalWriter, mode: crate::MIR::MirOwnershipMode) {
    writer.u64(match mode {
        crate::MIR::MirOwnershipMode::Copy => 1,
        crate::MIR::MirOwnershipMode::Owned => 2,
        crate::MIR::MirOwnershipMode::Shared => 3,
        crate::MIR::MirOwnershipMode::ReadBorrow => 4,
        crate::MIR::MirOwnershipMode::WriteBorrow => 5,
        crate::MIR::MirOwnershipMode::Move => 6,
    });
}

fn encode_struct_layout(writer: &mut CanonicalWriter, layout: Option<crate::MIR::MirStructLayout>) {
    match layout {
        Some(crate::MIR::MirStructLayout::C) => writer.u64(1),
        Some(crate::MIR::MirStructLayout::CAligned {
            alignment,
            target,
        }) => {
            writer.u64(3);
            writer.u64(alignment);
            writer.bool(target);
        }
        Some(crate::MIR::MirStructLayout::Columnar) => writer.u64(2),
        None => writer.u64(0),
    }
}
fn encode_layout_alignment(
    writer: &mut CanonicalWriter,
    fact: Option<&crate::Layout::LayoutAlignmentFact>,
) {
    match fact {
        Some(fact) => {
            writer.bool(true);
            writer.u64(fact.requested_alignment);
            writer.u64(fact.effective_alignment);
            writer.bool(fact.target_mode);
            writer.str(&fact.target_profile);
            writer.str(&fact.fact_identity);
        }
        None => writer.bool(false),
    }
}

fn encode_serde_codec(writer: &mut CanonicalWriter, codec: Option<crate::MIR::MirSerdeCodec>) {
    match codec {
        Some(crate::MIR::MirSerdeCodec::Encode) => writer.u64(1),
        Some(crate::MIR::MirSerdeCodec::Decode) => writer.u64(2),
        None => writer.u64(0),
    }
}

fn encode_serde_attributes(writer: &mut CanonicalWriter, attributes: &[crate::MIR::MirSerdeAttribute]) {
    writer.len(attributes.len());
    for attribute in attributes {
        writer.u64(match attribute.kind {
            crate::MIR::MirSerdeAttributeKind::RenameAll => 1,
            crate::MIR::MirSerdeAttributeKind::Tag => 2,
            crate::MIR::MirSerdeAttributeKind::Untagged => 3,
            crate::MIR::MirSerdeAttributeKind::DenyUnknownFields => 4,
        });
        writer.option_str(attribute.value.as_deref());
    }
}

fn encode_target_applicability(writer: &mut CanonicalWriter, target: crate::MIR::MirTargetApplicability) {
    writer.bool(target.rust_aot);
    writer.bool(target.cranelift);
    writer.bool(target.interpreter);
    writer.bool(target.web);
}
fn encode_app_string_list(writer: &mut CanonicalWriter, values: &[String]) {
    writer.len(values.len());
    for value in values {
        writer.str(value);
    }
}
fn encode_app_route_fields(writer: &mut CanonicalWriter, fields: &[crate::App::AppRouteField]) {
    writer.len(fields.len());
    for field in fields {
        writer.str(&field.name);
        writer.str(&field.ty);
        writer.str(&field.codec);
        writer.bool(field.required);
        writer.option_str(field.default.as_deref());
    }
}


fn encode_app_graph(writer: &mut CanonicalWriter, graph: Option<&crate::App::AppGraph>) {
    match graph {
        None => writer.bool(false),
        Some(graph) => {
            writer.bool(true);
            writer.str(&graph.entry_file);
            writer.str(&graph.hydration);
            writer.bool(graph.shared_tir);

            writer.len(graph.routes.len());
            for route in &graph.routes {
                writer.str(&route.path);
                writer.str(&route.handler);
                writer.str(&route.route_identity);
                encode_app_route_fields(writer, &route.path_params);
                encode_app_route_fields(writer, &route.search_params);
                writer.str(&route.search_codec);
                match &route.loader {
                    Some(loader) => {
                        writer.bool(true);
                        writer.str(&loader.handler);
                        writer.str(&loader.data_type);
                        writer.str(&loader.dependency);
                        writer.bool(loader.preload);
                        writer.str(&loader.cache_identity);
                    }
                    None => writer.bool(false),
                }
                writer.option_str(route.boundaries.pending.as_deref());
                writer.option_str(route.boundaries.not_found.as_deref());
                writer.option_str(route.boundaries.error.as_deref());
                writer.str(&route.precedence);
                writer.str(route.render.as_str());
                let facts = &route.render_facts;
                encode_app_string_list(writer, &facts.reason);
                encode_app_string_list(writer, &facts.effects);
                writer.bool(facts.interactive);
                writer.option_str(facts.loader.as_deref());
                writer.option_str(facts.form.as_deref());
                writer.str(facts.cache.as_str());
                match facts.override_mode {
                    Some(mode) => {
                        writer.bool(true);
                        writer.str(mode.as_str());
                    }
                    None => writer.bool(false),
                }
                match &facts.island {
                    Some(island) => {
                        writer.bool(true);
                        writer.str(&island.identity);
                        writer.bool(island.resume_payload.serializable);
                        writer.len(island.resume_payload.captures.len());
                        for capture in &island.resume_payload.captures {
                            writer.str(&capture.name);
                            writer.str(&capture.ty);
                            writer.bool(capture.serializable);
                        }
                    }
                    None => writer.bool(false),
                }
                writer.str(&route.provenance);
                writer.len(route.span_start);
                writer.len(route.span_end);
            }

            writer.len(graph.actions.len());
            for action in &graph.actions {
                writer.str(&action.name);
                writer.str(&action.handler);
                writer.str(&action.kind);
                writer.bool(action.preload);
                writer.str(&action.provenance);
                writer.len(action.span_start);
                writer.len(action.span_end);
            }

            writer.len(graph.mounts.len());
            for mount in &graph.mounts {
                writer.str(&mount.prefix);
                writer.str(&mount.handler);
                encode_app_string_list(writer, &mount.effects);
                encode_app_string_list(writer, &mount.security);
                writer.str(&mount.provenance);
                writer.len(mount.span_start);
                writer.len(mount.span_end);
            }

            writer.len(graph.routes_from.len());
            for root in &graph.routes_from {
                writer.str(&root.root);
                writer.len(root.span_start);
                writer.len(root.span_end);
            }

            encode_app_string_list(writer, &graph.policy.security);
            encode_app_string_list(writer, &graph.policy.assets);
            encode_app_string_list(writer, &graph.policy.split);
            encode_app_string_list(writer, &graph.policy.cache);
            encode_app_string_list(writer, &graph.policy.a11y);
            encode_app_string_list(writer, &graph.policy.adapters);
        }
    }
}

fn encode_foreign_abi(writer: &mut CanonicalWriter, abi: &crate::MIR::MirForeignAbi) {
    match abi {
        crate::MIR::MirForeignAbi::C => writer.u64(1),
        crate::MIR::MirForeignAbi::CUnwind => writer.u64(2),
        crate::MIR::MirForeignAbi::System => writer.u64(3),
        crate::MIR::MirForeignAbi::Stdcall => writer.u64(4),
        crate::MIR::MirForeignAbi::Fastcall => writer.u64(5),
        crate::MIR::MirForeignAbi::Vectorcall => writer.u64(6),
        crate::MIR::MirForeignAbi::Rust => writer.u64(7),
        crate::MIR::MirForeignAbi::Platform(name) => {
            writer.u64(8);
            writer.str(name);
        }
    }
}

fn encode_foreign_language(writer: &mut CanonicalWriter, language: crate::MIR::MirForeignLanguage) {
    writer.u64(match language {
        crate::MIR::MirForeignLanguage::C => 1,
        crate::MIR::MirForeignLanguage::Cpp => 2,
        crate::MIR::MirForeignLanguage::Rust => 3,
        crate::MIR::MirForeignLanguage::Assembly => 4,
    });
}

fn encode_link_artifact_kind(writer: &mut CanonicalWriter, kind: crate::MIR::MirLinkArtifactKind) {
    writer.u64(match kind {
        crate::MIR::MirLinkArtifactKind::Object => 1,
        crate::MIR::MirLinkArtifactKind::StaticLibrary => 2,
        crate::MIR::MirLinkArtifactKind::DynamicLibrary => 3,
        crate::MIR::MirLinkArtifactKind::Framework => 4,
        crate::MIR::MirLinkArtifactKind::GeneratedSource => 5,
    });
}

fn encode_handle_ownership(writer: &mut CanonicalWriter, ownership: crate::MIR::MirHandleOwnership) {
    writer.u64(match ownership {
        crate::MIR::MirHandleOwnership::Owned => 1,
        crate::MIR::MirHandleOwnership::Shared => 2,
        crate::MIR::MirHandleOwnership::Borrowed => 3,
    });
}

fn encode_param_zone(writer: &mut CanonicalWriter, zone: crate::MIR::MirParamZone) {
    writer.u64(match zone {
        crate::MIR::MirParamZone::PositionalOnly => 1,
        crate::MIR::MirParamZone::Either => 2,
        crate::MIR::MirParamZone::LabelOnly => 3,
    });
}

fn encode_cli_value_kind(writer: &mut CanonicalWriter, kind: crate::MIR::MirCliValueKind) {
    writer.u64(match kind {
        crate::MIR::MirCliValueKind::Bool => 1,
        crate::MIR::MirCliValueKind::Int => 2,
        crate::MIR::MirCliValueKind::Float => 3,
        crate::MIR::MirCliValueKind::String => 4,
        crate::MIR::MirCliValueKind::Path => 5,
    });
}

fn encode_entry_kind(writer: &mut CanonicalWriter, kind: crate::MIR::MirEntryKind) {
    writer.u64(match kind {
        crate::MIR::MirEntryKind::Library => 1,
        crate::MIR::MirEntryKind::Command => 2,
        crate::MIR::MirEntryKind::App => 3,
        crate::MIR::MirEntryKind::Service => 4,
        crate::MIR::MirEntryKind::Test => 5,
    });
}

fn encode_entry_output(writer: &mut CanonicalWriter, output: crate::MIR::MirEntryOutput) {
    writer.u64(match output {
        crate::MIR::MirEntryOutput::None => 1,
        crate::MIR::MirEntryOutput::ReturnValue => 2,
        crate::MIR::MirEntryOutput::StandardOutput => 3,
        crate::MIR::MirEntryOutput::ExitStatus => 4,
    });
}

fn encode_job_scope(writer: &mut CanonicalWriter, scope: crate::MIR::MirJobScope) {
    writer.u64(match scope {
        crate::MIR::MirJobScope::Dev => 1,
        crate::MIR::MirJobScope::Ship => 2,
        crate::MIR::MirJobScope::Internal => 3,
    });
}

fn encode_job_dispatch(writer: &mut CanonicalWriter, dispatch: crate::MIR::MirJobDispatch) {
    writer.u64(match dispatch {
        crate::MIR::MirJobDispatch::Direct => 1,
        crate::MIR::MirJobDispatch::Spawn => 2,
        crate::MIR::MirJobDispatch::Scheduled => 3,
    });
}

fn encode_job_cache(writer: &mut CanonicalWriter, cache: crate::MIR::MirJobCachePolicy) {
    writer.u64(match cache {
        crate::MIR::MirJobCachePolicy::Uncached => 1,
        crate::MIR::MirJobCachePolicy::Local => 2,
        crate::MIR::MirJobCachePolicy::Shared => 3,
    });
}

fn encode_test_kind(writer: &mut CanonicalWriter, kind: crate::MIR::MirTestKind) {
    writer.u64(match kind {
        crate::MIR::MirTestKind::Unit => 1,
        crate::MIR::MirTestKind::Property => 2,
    });
}

fn encode_harness_kind(writer: &mut CanonicalWriter, kind: crate::MIR::MirHarnessKind) {
    writer.u64(match kind {
        crate::MIR::MirHarnessKind::Test => 1,
        crate::MIR::MirHarnessKind::Fuzz => 2,
        crate::MIR::MirHarnessKind::Coverage => 3,
    });
}
fn encode_prelude_family(writer: &mut CanonicalWriter, family: crate::MIR::MirPreludeFamily) {
    writer.u64(match family {
        crate::MIR::MirPreludeFamily::MathBuiltin => 1,
        crate::MIR::MirPreludeFamily::PreciseBuiltin => 2,
        crate::MIR::MirPreludeFamily::BuiltinMethod => 3,
        crate::MIR::MirPreludeFamily::HostBorrowCallback => 4,
        crate::MIR::MirPreludeFamily::Overflow => 5,
        crate::MIR::MirPreludeFamily::HandleMethod => 6,
        crate::MIR::MirPreludeFamily::ClosureMethod => 7,
        crate::MIR::MirPreludeFamily::ColumnarAccess => 8,
        crate::MIR::MirPreludeFamily::StaticPrelude => 9,
        crate::MIR::MirPreludeFamily::Host => 10,
    });
}

fn encode_prelude_abi(writer: &mut CanonicalWriter, abi: crate::MIR::MirPreludeAbi) {
    writer.u64(match abi {
        crate::MIR::MirPreludeAbi::Value => 1,
        crate::MIR::MirPreludeAbi::Aggregate => 2,
        crate::MIR::MirPreludeAbi::Control => 3,
        crate::MIR::MirPreludeAbi::Effect => 4,
    });
}

fn encode_core_symbol(writer: &mut CanonicalWriter, symbol: crate::Syntax::CoreCallSymbol) {
    match symbol {
        crate::Syntax::CoreCallSymbol::Prelude(name) => {
            writer.u64(1);
            writer.str(name);
        }
        crate::Syntax::CoreCallSymbol::Rust(name) => {
            writer.u64(2);
            writer.str(name);
        }
    }
}

fn encode_typed_head_kind(writer: &mut CanonicalWriter, kind: crate::Syntax::TypedHeadKind) {
    writer.u64(match kind {
        crate::Syntax::TypedHeadKind::SQL => 1,
        crate::Syntax::TypedHeadKind::HTML => 2,
        crate::Syntax::TypedHeadKind::Sh => 3,
        crate::Syntax::TypedHeadKind::URL => 4,
        crate::Syntax::TypedHeadKind::Path => 5,
        crate::Syntax::TypedHeadKind::DateTime => 6,
    });
}

fn encode_gc_edit_kind(writer: &mut CanonicalWriter, kind: crate::MIR::MirGcEditKind) {
    writer.u64(match kind {
        crate::MIR::MirGcEditKind::Clear => 1,
        crate::MIR::MirGcEditKind::Pop => 2,
        crate::MIR::MirGcEditKind::RemoveIndex => 3,
        crate::MIR::MirGcEditKind::InsertIndex => 4,
        crate::MIR::MirGcEditKind::Prepend => 5,
        crate::MIR::MirGcEditKind::Additive => 6,
        crate::MIR::MirGcEditKind::Plain => 7,
        crate::MIR::MirGcEditKind::EdgeSlot => 8,
    });
}

fn encode_text_hole_kind(writer: &mut CanonicalWriter, kind: crate::MIR::MirTextHoleKind) {
    match kind {
        crate::MIR::MirTextHoleKind::Text => writer.u64(1),
        crate::MIR::MirTextHoleKind::Int => writer.u64(2),
        crate::MIR::MirTextHoleKind::Float => writer.u64(3),
        crate::MIR::MirTextHoleKind::Bool => writer.u64(4),
        crate::MIR::MirTextHoleKind::InlineRange { lo, hi } => {
            writer.u64(5);
            writer.u64(lo as u64);
            writer.u64(hi as u64);
        }
    }
}

fn encode_index_kind(writer: &mut CanonicalWriter, kind: crate::MIR::MirIndexKind) {
    writer.u64(match kind {
        crate::MIR::MirIndexKind::List => 1,
        crate::MIR::MirIndexKind::FixedListProof => 2,
        crate::MIR::MirIndexKind::Map => 3,
        crate::MIR::MirIndexKind::Lane => 4,
        crate::MIR::MirIndexKind::Pool => 5,
    });
}

fn encode_require_kind(writer: &mut CanonicalWriter, kind: crate::MIR::MirRequireKind) {
    writer.u64(match kind {
        crate::MIR::MirRequireKind::Require => 1,
        crate::MIR::MirRequireKind::RequireEq => 2,
        crate::MIR::MirRequireKind::Panic => 3,
    });
}

fn encode_layout_compare_op(writer: &mut CanonicalWriter, op: crate::MIR::MirLayoutCompareOp) {
    writer.u64(match op {
        crate::MIR::MirLayoutCompareOp::Equal => 1,
        crate::MIR::MirLayoutCompareOp::LessEqual => 2,
        crate::MIR::MirLayoutCompareOp::GreaterEqual => 3,
    });
}

fn encode_struct_extra(writer: &mut CanonicalWriter, extra: Option<crate::MIR::MirStructExtra>) {
    match extra {
        Some(crate::MIR::MirStructExtra::HttpRequestParams) => writer.u64(1),
        None => writer.u64(0),
    }
}

fn encode_allocator_kind(writer: &mut CanonicalWriter, kind: crate::MIR::MirAllocatorKind) {
    // Preserve General's historical code as Arena and Fixed's code as Fixed;
    // append the newly distinguished allocator families.
    writer.u64(match kind {
        crate::MIR::MirAllocatorKind::Arena => 1,
        crate::MIR::MirAllocatorKind::Fixed => 2,
        crate::MIR::MirAllocatorKind::Bump => 3,
        crate::MIR::MirAllocatorKind::Pool => 4,
    });
}

fn encode_task_group_kind(writer: &mut CanonicalWriter, kind: crate::MIR::MirTaskGroupKind) {
    writer.u64(match kind {
        crate::MIR::MirTaskGroupKind::All => 1,
        crate::MIR::MirTaskGroupKind::Any => 2,
        crate::MIR::MirTaskGroupKind::Race => 3,
    });
}

fn encode_select_kind(writer: &mut CanonicalWriter, kind: crate::MIR::MirSelectKind) {
    writer.u64(match kind {
        crate::MIR::MirSelectKind::Start => 1,
        crate::MIR::MirSelectKind::Receive => 2,
        crate::MIR::MirSelectKind::After => 3,
        crate::MIR::MirSelectKind::Wait => 4,
    });
}

fn encode_core_closure_kind(writer: &mut CanonicalWriter, kind: crate::MIR::MirCoreClosureKind) {
    let tag = match &kind {
        crate::MIR::MirCoreClosureKind::Spawn => 1,
        crate::MIR::MirCoreClosureKind::Serve => 2,
        crate::MIR::MirCoreClosureKind::OnInterrupt => 3,
        crate::MIR::MirCoreClosureKind::Guard => 4,
        crate::MIR::MirCoreClosureKind::OnCommit => 5,
        crate::MIR::MirCoreClosureKind::OnRollback => 6,
        crate::MIR::MirCoreClosureKind::ReactiveDerived => 7,
        crate::MIR::MirCoreClosureKind::ReactiveEffect => 8,
        crate::MIR::MirCoreClosureKind::UiMount => 9,
        crate::MIR::MirCoreClosureKind::UiAction => 10,
        crate::MIR::MirCoreClosureKind::Realtime => 11,
        crate::MIR::MirCoreClosureKind::UiTextInputOnDrop => 12,
        crate::MIR::MirCoreClosureKind::UiPreview { .. } => 13,
    };
    writer.u64(tag);
    if let crate::MIR::MirCoreClosureKind::UiPreview {
        playground,
        source_file,
        source_span,
        source_start_line,
        source_start_column,
        source_end_line,
        source_end_column,
        build_id,
        revision,
        ..
    } = kind
    {
        writer.bool(playground);
        writer.str(&source_file);
        writer.span(source_span);
        writer.u64(u64::from(source_start_line));
        writer.u64(u64::from(source_start_column));
        writer.u64(u64::from(source_end_line));
        writer.u64(u64::from(source_end_column));
        writer.str(&build_id);
        writer.str(&revision);
    }
}

fn encode_core_pure_route(writer: &mut CanonicalWriter, route: crate::Syntax::CoreCallPureRoute) {
    writer.u64(match route {
        crate::Syntax::CoreCallPureRoute::None => 0,
        crate::Syntax::CoreCallPureRoute::Mime => 1,
        crate::Syntax::CoreCallPureRoute::Email => 2,
        crate::Syntax::CoreCallPureRoute::EncodingXml => 3,
        crate::Syntax::CoreCallPureRoute::Time => 4,
        crate::Syntax::CoreCallPureRoute::Math => 5,
        crate::Syntax::CoreCallPureRoute::Measurement => 6,
        crate::Syntax::CoreCallPureRoute::Date => 7,
        crate::Syntax::CoreCallPureRoute::DateTime => 8,
        crate::Syntax::CoreCallPureRoute::SketchHll => 9,
        crate::Syntax::CoreCallPureRoute::SketchTDigest => 10,
        crate::Syntax::CoreCallPureRoute::SketchCms => 11,
        crate::Syntax::CoreCallPureRoute::SketchReservoir => 12,
        crate::Syntax::CoreCallPureRoute::Ui => 13,
        crate::Syntax::CoreCallPureRoute::Raylib => 14,
        crate::Syntax::CoreCallPureRoute::Io => 15,
        crate::Syntax::CoreCallPureRoute::Net => 16,
        crate::Syntax::CoreCallPureRoute::Crypto => 17,
    });
}

fn encode_interpreter_route(writer: &mut CanonicalWriter, route: crate::Syntax::CoreCallInterpreterRoute) {
    match route {
        crate::Syntax::CoreCallInterpreterRoute::None => writer.u64(0),
        crate::Syntax::CoreCallInterpreterRoute::Pure(route) => {
            writer.u64(1);
            encode_core_pure_route(writer, route);
        }
        crate::Syntax::CoreCallInterpreterRoute::Ambient => writer.u64(2),
        crate::Syntax::CoreCallInterpreterRoute::TypedIntrinsic => writer.u64(3),
    }
}

fn encode_effects(writer: &mut CanonicalWriter, effects: &crate::MIR::MirEffectFacts) {
    for values in [&effects.direct, &effects.solved, &effects.call_edges] {
        writer.len(values.len());
        for value in values.iter() {
            writer.str(value);
        }
    }
    writer.bool(effects.maximal);
    writer.len(effects.direct_spans.len());
    for (name, span) in &effects.direct_spans {
        writer.str(name);
        writer.span(*span);
    }
}

fn encode_generic_params(writer: &mut CanonicalWriter, params: &[crate::MIR::MirGenericParam]) {
    writer.len(params.len());
    for param in params {
        writer.str(&param.name);
        writer.len(param.bounds.len());
        for bound in &param.bounds {
            encode_trait_ref(writer, bound);
        }
    }
}

fn encode_module(writer: &mut CanonicalWriter, module: &MirModule) {
    writer.u64(module.id.0);
    writer.str(&module.key);
    writer.str(&module.name);
    writer.str(&module.path);
    writer.u64(module.source_file.0);
    let mut imports = module.imports.iter().collect::<Vec<_>>();
    imports.sort();
    writer.len(imports.len());
    for import in imports {
        writer.u64(import.0);
    }
    writer.len(module.item_order.len());
    for item in &module.item_order {
        encode_item_ref(writer, item);
    }
}

fn encode_import(writer: &mut CanonicalWriter, import: &MirImport) {
    writer.u64(import.id.0);
    writer.u64(import.module.0);
    encode_visibility(writer, import.visibility);
    writer.str(&import.alias);
    writer.span(import.span);
    match &import.kind {
        MirImportKind::File { path } => {
            writer.tag("file");
            writer.str(path);
        }
        MirImportKind::Module { path } => {
            writer.tag("module");
            writer.str(path);
        }
        MirImportKind::Unqualified { module, items } => {
            writer.tag("unqualified");
            writer.u64(module.0);
            writer.len(items.len());
            for item in items {
                writer.str(&item.original);
                writer.str(&item.local);
                encode_item_ref(writer, &item.item);
            }
        }
    }
}

fn encode_type_def(writer: &mut CanonicalWriter, ty: &MirTypeDef) {
    writer.u64(ty.id.0);
    writer.u64(ty.module.0);
    writer.str(&ty.key);
    writer.str(&ty.name);
    writer.span(ty.span);
    writer.bool(ty.public);
    writer.bool(ty.package_public);
    encode_generic_params(writer, &ty.generic_params);
    writer.len(ty.derives.len());
    for derive in &ty.derives {
        writer.u64(derive.0);
    }
    writer.bool(ty.auto_derive_default);
    writer.bool(ty.auto_printable);
    writer.bool(ty.published_schema);
    writer.bool(ty.single_use);
    writer.bool(ty.must_use);
    encode_struct_layout(writer, ty.layout);
    encode_layout_alignment(writer, ty.layout_alignment.as_ref());
    encode_serde_attributes(writer, &ty.serde);
    writer.len(ty.cli_bindings.len());
    for binding in &ty.cli_bindings {
        writer.str(&binding.name);
        writer.u64(binding.function.0);
    }
    match &ty.cli {
        Some(cli) => {
            writer.bool(true);
            encode_cli_entry(writer, cli);
        }
        None => writer.bool(false),
    }
    encode_ownership_mode(writer, ty.ownership);
    writer.len(ty.boxed_edges.len());
    for edge in &ty.boxed_edges {
        writer.str(edge);
    }
    encode_type_def_kind(writer, &ty.kind);
}

fn encode_trait_method(writer: &mut CanonicalWriter, method: &crate::MIR::MirTraitMethod) {
    writer.u64(method.id.0);
    writer.str(&method.name);
    writer.span(method.span);
    encode_access(writer, method.self_access);
    writer.len(method.params.len());
    for param in &method.params {
        encode_param(writer, param);
    }
    match &method.declared_return {
        Some(ty) => {
            writer.bool(true);
            writer.mir_type(ty);
        }
        None => writer.bool(false),
    }
    writer.mir_type(&method.return_type);
    encode_failure_carrier(writer, &method.failure);
    encode_effects(writer, &method.effects);
    writer.bool(method.is_pure);
    writer.debug(&method.return_view_provenance);
    writer.option_u64(method.default.map(|function| function.0));
}

fn encode_trait_def(writer: &mut CanonicalWriter, trait_def: &MirTraitDef) {
    writer.u64(trait_def.id.0);
    writer.u64(trait_def.module.0);
    writer.str(&trait_def.key);
    writer.str(&trait_def.name);
    writer.span(trait_def.span);
    encode_visibility(writer, trait_def.visibility);
    writer.len(trait_def.associated_types.len());
    for associated in &trait_def.associated_types {
        writer.str(&associated.name);
        writer.span(associated.span);
    }
    let mut methods = trait_def.methods.iter().collect::<Vec<_>>();
    methods.sort_by_key(|method| (method.id, method.name.as_str()));
    writer.len(methods.len());
    for method in methods {
        encode_trait_method(writer, method);
    }
}

fn encode_impl_def(writer: &mut CanonicalWriter, impl_def: &MirImplDef) {
    writer.u64(impl_def.id.0);
    writer.u64(impl_def.module.0);
    writer.str(&impl_def.key);
    writer.span(impl_def.span);
    writer.mir_type(&impl_def.self_type);
    match &impl_def.trait_ref {
        Some(trait_ref) => {
            writer.bool(true);
            encode_trait_ref(writer, trait_ref);
        }
        None => writer.bool(false),
    }
    let mut associated_types = impl_def.associated_types.iter().collect::<Vec<_>>();
    associated_types.sort_by_key(|associated| (associated.name.as_str(), associated.span.start));
    writer.len(associated_types.len());
    for associated in associated_types {
        writer.str(&associated.name);
        writer.mir_type(&associated.ty);
        writer.span(associated.span);
    }
    let mut methods = impl_def.methods.iter().collect::<Vec<_>>();
    methods.sort();
    writer.len(methods.len());
    for method in methods {
        writer.u64(method.0);
    }
    writer.option_u64(impl_def.delegation.map(|field| field.0));
    writer.bool(impl_def.compiler_generated);
    encode_serde_codec(writer, impl_def.serde);
    match &impl_def.operator_rhs {
        Some(ty) => {
            writer.bool(true);
            writer.mir_type(ty);
        }
        None => writer.bool(false),
    }
    match impl_def.operator_marker {
        Some(crate::AST::OperatorMarker::Commutative) => {
            writer.bool(true);
            writer.tag("commutative");
        }
        None => writer.bool(false),
    }
    encode_target_applicability(writer, impl_def.target_applicability);
}

fn encode_constant_def(writer: &mut CanonicalWriter, constant: &MirConstantDef) {
    writer.u64(constant.id.0);
    writer.u64(constant.module.0);
    writer.str(&constant.key);
    writer.str(&constant.name);
    writer.span(constant.span);
    encode_visibility(writer, constant.visibility);
    writer.mir_type(&constant.ty);
    encode_mir_constant(writer, &constant.value);
}

fn encode_link_unit(writer: &mut CanonicalWriter, link: &MirLinkUnit) {
    writer.u64(link.id.0);
    writer.str(&link.crate_spec);
    writer.str(&link.cache_identity);
    encode_target_applicability(writer, link.target_applicability);
    writer.len(link.artifacts.len());
    for artifact in &link.artifacts {
        encode_link_artifact_kind(writer, artifact.kind);
        writer.str(&artifact.path);
    }
    writer.len(link.dependency_dirs.len());
    for path in &link.dependency_dirs {
        writer.str(path);
    }
    writer.len(link.link_closure.len());
    for unit in &link.link_closure {
        writer.u64(unit.0);
    }
}

fn encode_callback(writer: &mut CanonicalWriter, callback: &MirCallbackAdapter) {
    writer.u64(callback.id.0);
    writer.str(&callback.symbol);
    writer.u64(callback.function.0);
    writer.len(callback.params.len());
    for param in &callback.params {
        encode_param(writer, param);
    }
    match &callback.return_type {
        Some(ty) => {
            writer.bool(true);
            writer.mir_type(ty);
        }
        None => writer.bool(false),
    }
    encode_foreign_abi(writer, &callback.abi);
}

fn encode_handle(writer: &mut CanonicalWriter, handle: &MirHandleLifecycle) {
    writer.u64(handle.id.0);
    writer.mir_type(&handle.ty);
    encode_handle_ownership(writer, handle.ownership);
    writer.str(&handle.payload.library);
    writer.str(&handle.payload.typedef_name);
    writer.str(&handle.payload.close);
    writer.option_u64(handle.close.map(|function| function.0));
    writer.option_u64(handle.close_foreign.map(|foreign| foreign.0));
    writer.option_u64(handle.undo.map(|function| function.0));
    writer.bool(handle.send);
    writer.bool(handle.sync);
}

fn encode_cli_input(writer: &mut CanonicalWriter, input: &MirCliInput) {
    writer.len(input.parameter);
    writer.str(&input.name);
    writer.str(&input.label);
    writer.mir_type(&input.ty);
    encode_param_zone(writer, input.zone);
    writer.option_str(input.short.as_deref());
    writer.option_str(input.env.as_deref());
    writer.str(&input.help);
    writer.option_str(input.metavar.as_deref());
    match &input.shape {
        MirCliInputShape::Flag => writer.tag("flag"),
        MirCliInputShape::Value {
            kind,
            optional,
            default,
        } => {
            writer.tag("value");
            encode_cli_value_kind(writer, *kind);
            writer.bool(*optional);
            match default {
                Some(MirCliDefault::TypeDefault) => writer.tag("type-default"),
                Some(MirCliDefault::Value(value)) => {
                    writer.tag("value-default");
                    encode_mir_constant(writer, value);
                }
                None => writer.tag("no-default"),
            }
        }
    }
    writer.option_u64(input.positional.map(u64::from));
    writer.bool(input.variadic);
}

fn encode_cli_entry(writer: &mut CanonicalWriter, cli: &crate::MIR::MirCliEntry) {
    writer.option_str(cli.description.as_deref());
    writer.len(cli.inputs.len());
    for input in &cli.inputs {
        encode_cli_input(writer, input);
    }
    writer.len(cli.commands.len());
    for command in &cli.commands {
        writer.str(&command.name);
        writer.option_str(command.description.as_deref());
        writer.u64(command.function.0);
        writer.option_u64(command.receiver.map(|ty| ty.0));
        writer.len(command.inputs.len());
        for input in &command.inputs {
            encode_cli_input(writer, input);
        }
    }
    writer.bool(cli.standard);
}
fn encode_entry_spec(writer: &mut CanonicalWriter, entry: &MirEntrySpec) {
    encode_entry_kind(writer, entry.kind);
    writer.option_u64(entry.function.map(|function| function.0));
    match &entry.cli {
        Some(cli) => {
            writer.bool(true);
            encode_cli_entry(writer, cli);
        }
        None => writer.bool(false),
    }
    encode_entry_output(writer, entry.output);
    writer.bool(entry.initialize_environment);
    writer.bool(entry.initialize_gc);
    writer.bool(entry.serves_until_stopped);
    writer.str(&entry.package_version);
}

fn encode_runtime_parts(writer: &mut CanonicalWriter, values: &std::collections::BTreeSet<crate::MIR::MirRuntimePartId>) {
    writer.len(values.len());
    for value in values {
        encode_runtime_part(writer, *value);
    }
}
fn encode_hardware_setups(writer: &mut CanonicalWriter, setups: &[MirHardwareSetup]) {
    let mut rows = setups.iter().collect::<Vec<_>>();
    rows.sort_by_key(|setup| match setup {
        MirHardwareSetup::DmaConfigure {
            profile_id,
            channel,
            transfer_width,
            ownership,
        } => format!(
            "dma-configure\0{profile_id}\0{channel}\0{}\0{}",
            transfer_width.as_str(),
            ownership.as_str()
        ),
        MirHardwareSetup::InterruptBind {
            profile_id,
            interrupt,
            vector,
            handler_symbol,
            forbidden_effects,
        } => format!(
            "interrupt-bind\0{profile_id}\0{interrupt}\0{vector}\0{handler_symbol}\0{}",
            forbidden_effects.join("\0")
        ),
    });
    writer.len(rows.len());
    for setup in rows {
        match setup {
            MirHardwareSetup::DmaConfigure {
                profile_id,
                channel,
                transfer_width,
                ownership,
            } => {
                writer.tag("dma-configure");
                writer.str(profile_id);
                writer.str(channel);
                writer.str(transfer_width.as_str());
                writer.str(ownership.as_str());
            }
            MirHardwareSetup::InterruptBind {
                profile_id,
                interrupt,
                vector,
                handler_symbol,
                forbidden_effects,
            } => {
                writer.tag("interrupt-bind");
                writer.str(profile_id);
                writer.str(interrupt);
                writer.u16(*vector);
                writer.str(handler_symbol);
                let mut effects = forbidden_effects.iter().collect::<Vec<_>>();
                effects.sort_unstable();
                writer.len(effects.len());
                for effect in effects {
                    writer.str(effect);
                }
            }
        }
    }
}
fn encode_target_hardware_facts(
    writer: &mut CanonicalWriter,
    facts: &crate::TargetMachine::TargetHardwareFacts,
) {
    match &facts.svd {
        Some(svd) => {
            writer.bool(true);
            writer.str(&svd.source);
            writer.str(&svd.sha256);
        }
        None => writer.bool(false),
    }

    let mut blocks = facts.register_blocks.iter().collect::<Vec<_>>();
    blocks.sort_by_key(|block| (block.name.as_str(), block.base, block.size.bytes));
    writer.len(blocks.len());
    for block in blocks {
        writer.str(&block.name);
        writer.u64(block.base);
        writer.u64(block.size.bytes);
        let mut registers = block.registers.iter().collect::<Vec<_>>();
        registers.sort_by_key(|register| {
            (
                register.name.as_str(),
                register.offset,
                register.width.as_str(),
                register.access.as_str(),
                register.volatile,
            )
        });
        writer.len(registers.len());
        for register in registers {
            writer.str(&register.name);
            writer.u64(register.offset);
            writer.str(register.width.as_str());
            writer.str(register.access.as_str());
            writer.bool(register.volatile);
        }
    }

    let mut interrupts = facts.interrupts.iter().collect::<Vec<_>>();
    interrupts.sort_by_key(|interrupt| (interrupt.name.as_str(), interrupt.vector));
    writer.len(interrupts.len());
    for interrupt in interrupts {
        writer.str(&interrupt.name);
        writer.u16(interrupt.vector);
        writer.len(interrupt.forbidden_effects.len());
        for effect in &interrupt.forbidden_effects {
            writer.str(effect);
        }
        writer.bool(interrupt.bounded);
    }

    let mut channels = facts.dma_channels.iter().collect::<Vec<_>>();
    channels.sort_by_key(|channel| {
        (
            channel.name.as_str(),
            channel.channel,
            channel.transfer_width.as_str(),
            channel.ownership.as_str(),
            channel.max_transfer_bytes,
        )
    });
    writer.len(channels.len());
    for channel in channels {
        writer.str(&channel.name);
        writer.u16(channel.channel);
        writer.str(channel.transfer_width.as_str());
        writer.str(channel.ownership.as_str());
        writer.option_u64(channel.max_transfer_bytes);
    }
}

fn encode_target_register_operation(
    writer: &mut CanonicalWriter,
    operation: crate::TargetMachine::TargetRegisterOperation,
) {
    writer.tag(match operation {
        crate::TargetMachine::TargetRegisterOperation::Read => "read",
        crate::TargetMachine::TargetRegisterOperation::Write => "write",
    });
}

fn encode_target_dma_operation(
    writer: &mut CanonicalWriter,
    operation: crate::TargetMachine::TargetDmaOperation,
) {
    writer.tag(match operation {
        crate::TargetMachine::TargetDmaOperation::Start => "start",
        crate::TargetMachine::TargetDmaOperation::Wait => "wait",
        crate::TargetMachine::TargetDmaOperation::UseBuffer => "use-buffer",
    });
}

fn encode_target_dma_owner(
    writer: &mut CanonicalWriter,
    owner: crate::TargetMachine::TargetDmaOwner,
) {
    writer.tag(match owner {
        crate::TargetMachine::TargetDmaOwner::Cpu => "cpu",
        crate::TargetMachine::TargetDmaOwner::Device => "device",
    });
}

fn encode_target_hardware_use(
    writer: &mut CanonicalWriter,
    hardware: &crate::TargetMachine::TargetHardwareUse,
) {
    writer.len(hardware.register_accesses.len());
    for access in &hardware.register_accesses {
        writer.str(&access.block);
        writer.str(&access.register);
        encode_target_register_operation(writer, access.operation);
        writer.str(access.width.as_str());
    }

    writer.len(hardware.interrupt_handlers.len());
    for handler in &hardware.interrupt_handlers {
        writer.str(&handler.interrupt);
        writer.str(&handler.handler);
        writer.len(handler.effects.len());
        for effect in &handler.effects {
            writer.str(effect);
        }
        writer.len(handler.forbidden_effects.len());
        for effect in &handler.forbidden_effects {
            writer.str(effect);
        }
        writer.bool(handler.bounded);
    }

    writer.len(hardware.dma_operations.len());
    for operation in &hardware.dma_operations {
        writer.str(&operation.channel);
        writer.str(&operation.buffer);
        encode_target_dma_operation(writer, operation.operation);
        encode_target_dma_owner(writer, operation.owner);
    }

    writer.len(hardware.unresolved_references.len());
    for reference in &hardware.unresolved_references {
        match reference {
            crate::TargetMachine::TargetHardwareUnresolvedReference::RegisterBlock { block } => {
                writer.tag("register-block");
                writer.str(block);
            }
            crate::TargetMachine::TargetHardwareUnresolvedReference::Register {
                block,
                register,
            } => {
                writer.tag("register");
                writer.str(block);
                writer.str(register);
            }
            crate::TargetMachine::TargetHardwareUnresolvedReference::DmaChannel { channel } => {
                writer.tag("dma-channel");
                writer.str(channel);
            }
            crate::TargetMachine::TargetHardwareUnresolvedReference::Interrupt { interrupt } => {
                writer.tag("interrupt");
                writer.str(interrupt);
            }
        }
    }
}

fn encode_job(writer: &mut CanonicalWriter, job: &MirJob) {
    writer.u64(job.id.0);
    writer.u64(job.function.0);
    writer.str(&job.name);
    encode_job_scope(writer, job.scope);
    match job.schedule {
        Some(crate::MIR::MirJobSchedule::Duration { nanos }) => {
            writer.tag("duration");
            writer.u64(nanos as u64);
        }
        Some(crate::MIR::MirJobSchedule::WallClockTime { hour, minute }) => {
            writer.tag("wall-clock");
            writer.u64(u64::from(hour));
            writer.u64(u64::from(minute));
        }
        None => writer.tag("no-schedule"),
    }
    writer.len(job.inputs.len());
    for input in &job.inputs {
        encode_cli_input(writer, input);
    }
    encode_job_dispatch(writer, job.dispatch);
    writer.len(job.after.len());
    for dependency in &job.after {
        writer.str(dependency);
    }
    writer.u64(job.parallel as u64);
    writer.len(job.packages.len());
    for package in &job.packages {
        writer.str(package);
    }
    writer.option_str(job.working_directory.as_deref());
    writer.len(job.input_paths.len());
    for path in &job.input_paths {
        writer.str(path);
    }
    writer.len(job.output_paths.len());
    for path in &job.output_paths {
        writer.str(path);
    }
    match &job.skip {
        Some(crate::MIR::MirJobSkip::Always(reason)) => {
            writer.tag("always");
            writer.str(reason);
        }
        Some(crate::MIR::MirJobSkip::UnlessPlatform(platform)) => {
            writer.tag("unless-platform");
            writer.str(platform);
        }
        None => writer.tag("no-skip"),
    }
    encode_job_cache(writer, job.cache);
    writer.len(job.limits.len());
    for (key, value) in &job.limits {
        writer.str(key);
        writer.str(value);
    }
}

fn encode_test(writer: &mut CanonicalWriter, test: &MirTestCase) {
    writer.u64(test.id.0);
    writer.u64(test.function.0);
    writer.str(&test.name);
    writer.span(test.span);
    encode_test_kind(writer, test.kind);
    writer.bool(test.contract_generated);
    writer.option_u64(test.eligibility.map(|id| id.0));
    writer.len(test.parameters.len());
    for param in &test.parameters {
        encode_param(writer, param);
    }
    writer.len(test.faults.len());
    for fault in &test.faults {
        writer.str(fault);
    }
    writer.bool(test.expected_failure);
    writer.option_str(test.generation_unavailable_reason.as_deref());
}

fn encode_harness(writer: &mut CanonicalWriter, harness: &MirHarnessPlan) {
    writer.u64(harness.id.0);
    encode_harness_kind(writer, harness.kind);
    let mut tests = harness.tests.iter().collect::<Vec<_>>();
    tests.sort();
    writer.len(tests.len());
    for test in tests {
        writer.u64(test.0);
    }
    let mut output_checks = harness.output_checks.iter().collect::<Vec<_>>();
    output_checks.sort_by_key(|check| (check.id, check.name.as_str()));
    writer.len(output_checks.len());
    for check in output_checks {
        writer.u64(check.id.0);
        writer.str(&check.name);
        writer.u64(check.function.0);
    }
    writer.option_u64(harness.selected_test.map(|test| test.0));
    let mut coverage_points = harness.coverage_points.iter().collect::<Vec<_>>();
    coverage_points.sort_by_key(|point| (point.id, point.function, point.block));
    writer.len(coverage_points.len());
    for point in coverage_points {
        writer.u64(point.id.0);
        writer.u64(point.function.0);
        writer.u64(point.block.0);
        writer.span(point.span);
    }
    writer.bool(harness.command_override);
}

fn encode_artifact_kind(writer: &mut CanonicalWriter, kind: crate::MIR::MirArtifactKind) {
    let code = match kind {
        crate::MIR::MirArtifactKind::NativeExecutable => 1,
        crate::MIR::MirArtifactKind::NativeLibrary => 2,
        crate::MIR::MirArtifactKind::WebApplication => 3,
        crate::MIR::MirArtifactKind::TestExecutable => 4,
        crate::MIR::MirArtifactKind::FuzzExecutable => 5,
        crate::MIR::MirArtifactKind::SandboxPlugin => 6,
        crate::MIR::MirArtifactKind::TestOverride => 7,
    };
    writer.u64(code);
}

fn encode_artifact_target(writer: &mut CanonicalWriter, target: crate::MIR::MirArtifactTarget) {
    let code = match target {
        crate::MIR::MirArtifactTarget::RustAot => 1,
        crate::MIR::MirArtifactTarget::Cranelift => 2,
        crate::MIR::MirArtifactTarget::Interpreter => 3,
        crate::MIR::MirArtifactTarget::Web => 4,
    };
    writer.u64(code);
}
fn encode_artifact_mode(writer: &mut CanonicalWriter, mode: crate::MIR::MirArtifactBuildMode) {
    let code = match mode {
        crate::MIR::MirArtifactBuildMode::Dev => 1,
        crate::MIR::MirArtifactBuildMode::Release => 2,
        crate::MIR::MirArtifactBuildMode::Test => 3,
        crate::MIR::MirArtifactBuildMode::Fuzz => 4,
        crate::MIR::MirArtifactBuildMode::Coverage => 5,
    };
    writer.u64(code);
}
fn encode_erasure_reason(
    writer: &mut CanonicalWriter,
    reason: crate::MIR::MirErasureReason,
) {
    writer.u64(match reason {
        crate::MIR::MirErasureReason::CompileTimeOnly => 1,
        crate::MIR::MirErasureReason::Disabled => 2,
        crate::MIR::MirErasureReason::ExpandedBeforeLowering => 3,
        crate::MIR::MirErasureReason::SemanticIndexOnly => 4,
    });
}

fn encode_runtime_part(writer: &mut CanonicalWriter, part: crate::MIR::MirRuntimePartId) {
    writer.u64(match part {
        crate::MIR::MirRuntimePartId::Event => 1,
        crate::MIR::MirRuntimePartId::Realtime => 2,
        crate::MIR::MirRuntimePartId::EmbeddedHardware => 3,
        crate::MIR::MirRuntimePartId::Ui => 4,
        crate::MIR::MirRuntimePartId::Devtools => 5,
        crate::MIR::MirRuntimePartId::Gtk => 6,
        crate::MIR::MirRuntimePartId::Apps => 7,
        crate::MIR::MirRuntimePartId::Email => 8,
        crate::MIR::MirRuntimePartId::Game => 9,
        crate::MIR::MirRuntimePartId::Files => 10,
        crate::MIR::MirRuntimePartId::Interrupt => 11,
        crate::MIR::MirRuntimePartId::FsRuntime => 12,
        crate::MIR::MirRuntimePartId::Process => 13,
        crate::MIR::MirRuntimePartId::Crypto => 14,
        crate::MIR::MirRuntimePartId::Math => 15,
        crate::MIR::MirRuntimePartId::Encoding => 16,
        crate::MIR::MirRuntimePartId::Data => 17,
        crate::MIR::MirRuntimePartId::Fmt => 18,
        crate::MIR::MirRuntimePartId::DataFmt => 19,
        crate::MIR::MirRuntimePartId::Compute => 20,
        crate::MIR::MirRuntimePartId::Http => 21,
        crate::MIR::MirRuntimePartId::WebSocket => 22,
        crate::MIR::MirRuntimePartId::Browser => 23,
        crate::MIR::MirRuntimePartId::Args => 24,
        crate::MIR::MirRuntimePartId::Reflect => 25,
        crate::MIR::MirRuntimePartId::AuthTokens => 26,
        crate::MIR::MirRuntimePartId::AuthSession => 27,
        crate::MIR::MirRuntimePartId::Sync => 28,
        crate::MIR::MirRuntimePartId::Services => 29,
        crate::MIR::MirRuntimePartId::Mod => 30,
        crate::MIR::MirRuntimePartId::Gc => 31,
    });
}
fn encode_artifact(writer: &mut CanonicalWriter, artifact: &MirArtifactPlan) {
    writer.u64(artifact.id.0);
    encode_artifact_kind(writer, artifact.kind);
    writer.str(&artifact.name);
    encode_artifact_target(writer, artifact.target);
    encode_artifact_mode(writer, artifact.mode);
    let mut modules = artifact.modules.iter().collect::<Vec<_>>();
    modules.sort();
    writer.len(modules.len());
    for module in modules {
        writer.u64(module.0);
    }
    let mut links = artifact.links.iter().collect::<Vec<_>>();
    links.sort();
    writer.len(links.len());
    for link in links {
        writer.u64(link.0);
    }
    let mut jobs = artifact.jobs.iter().collect::<Vec<_>>();
    jobs.sort();
    writer.len(jobs.len());
    for job in jobs {
        writer.u64(job.0);
    }
    writer.len(artifact.runtime_parts.len());
    for part in &artifact.runtime_parts {
        encode_runtime_part(writer, *part);
    }
    let mut exports = artifact.exports.iter().collect::<Vec<_>>();
    exports.sort_by_key(|export| (export.symbol.as_str(), export.function));
    writer.len(exports.len());
    for export in exports {
        writer.str(&export.symbol);
        writer.u64(export.function.0);
        encode_foreign_abi(writer, &export.abi);
    }
    writer.str(&artifact.provider_identity);
    writer.str(&artifact.closure_identity);
    writer.str(&artifact.artifact_identity);
    match &artifact.entry {
        Some(entry) => {
            writer.bool(true);
            encode_entry_spec(writer, entry);
        }
        None => writer.bool(false),
    }
    writer.option_u64(artifact.harness.map(|harness| harness.0));
}

fn encode_callee(writer: &mut CanonicalWriter, callee: &MirCallee) {
    match callee {
        MirCallee::User(function) => {
            writer.tag("user");
            writer.u64(function.0);
        }
        MirCallee::Associated { function, owner } => {
            writer.tag("associated");
            writer.u64(function.0);
            writer.mir_type(owner);
        }
        MirCallee::Method { function, owner } => {
            writer.tag("method");
            writer.u64(function.0);
            writer.mir_type(owner);
        }
        MirCallee::TraitMethod { method, trait_ref, receiver } => {
            writer.tag("trait_method");
            writer.u64(method.0);
            encode_trait_ref(writer, trait_ref);
            writer.mir_type(receiver);
        }
        MirCallee::Core(call) => {
            writer.tag("core");
            writer.u64(call.0);
        }
        MirCallee::Prelude(call) => {
            writer.tag("prelude");
            writer.u64(call.0);
        }
        MirCallee::Foreign(foreign) => {
            writer.tag("foreign");
            writer.u64(foreign.0);
        }
        MirCallee::Indirect(value) => {
            writer.tag("indirect");
            writer.u64(value.0);
        }
    }
}

fn encode_data_plan_stream(
    writer: &mut CanonicalWriter,
    stream: &crate::AST::DataPlanStreamMode,
) {
    match stream {
        crate::AST::DataPlanStreamMode::Bounded { batch_rows } => {
            writer.tag("bounded");
            writer.option_u64(batch_rows.map(|rows| rows as u64));
        }
        crate::AST::DataPlanStreamMode::Materialized { reason } => {
            writer.tag("materialized");
            writer.str(reason);
        }
    }
}

fn encode_data_plan(writer: &mut CanonicalWriter, plan: &crate::MIR::MirDataPlan) {
    writer.u16(plan.schema_version);
    writer.str(plan.source.as_str());
    writer.len(plan.logical.len());
    for node in &plan.logical {
        writer.u64(node.id.0);
        writer.str(node.operation.as_str());
        writer.len(node.inputs.len());
        for input in &node.inputs {
            writer.u64(input.0);
        }
        writer.option_str(node.source.map(crate::AST::DataPlanSourceKind::as_str));
        writer.mir_type(&node.row_type);
        writer.len(node.columns.len());
        for column in &node.columns {
            writer.str(&column.name);
            writer.mir_type(&column.ty);
            writer.bool(column.nullable);
        }
        match &node.callable {
            Some(callable) => {
                writer.bool(true);
                writer.str(&callable.label);
                writer.mir_type(&callable.parameter);
                writer.mir_type(&callable.result);
                writer.span(callable.span);
            }
            None => writer.bool(false),
        }
        encode_data_plan_stream(writer, &node.stream);
        writer.span(node.span);
    }
    writer.len(plan.physical.len());
    for node in &plan.physical {
        writer.u64(node.logical.0);
        writer.str(node.operator.as_str());
        encode_data_plan_stream(writer, &node.stream);
        writer.str(&node.reason);
    }
    writer.u64(plan.output.0);
    writer.span(plan.source_span);
}

fn encode_call_args(writer: &mut CanonicalWriter, args: &[crate::MIR::MirCallArg]) {
    writer.len(args.len());
    for arg in args {
        writer.u64(arg.value.0);
        writer.option_u64(arg.place.map(|place| place.0));
        writer.debug(&arg.access);
        writer.span(arg.span);
        writer.option_str(arg.label.as_deref());
        match arg.source_index {
            Some(index) => {
                writer.bool(true);
                writer.len(index);
            }
            None => writer.bool(false),
        }
        match arg.binder_slot {
            Some(slot) => {
                writer.bool(true);
                writer.len(slot);
            }
            None => writer.bool(false),
        }
        writer.bool(arg.spread);
        writer.bool(arg.implicit_clone);
        writer.bool(arg.shared_auto_clone);
        writer.bool(arg.owned_last_use);
        writer.bool(arg.authority_boundary);
        match &arg.fn_coercion {
            Some(coercion) => {
                writer.bool(true);
                writer.mir_type(&coercion.ty);
                writer.bool(coercion.already_boxed);
            }
            None => writer.bool(false),
        }
        writer.bool(arg.widen_fixed_to_list);
        match &arg.widen_to_union {
            Some(coercion) => {
                writer.bool(true);
                writer.u64(coercion.union.0);
                writer.str(&coercion.variant);
            }
            None => writer.bool(false),
        }
        writer.option_u64(arg.box_as_trait.map(|type_id| type_id.0));
    }
}

fn encode_conversion(writer: &mut CanonicalWriter, conversion: &MirConversion) {
    match conversion {
        MirConversion::Transparent => writer.tag("transparent"),
        MirConversion::NumericCast => writer.tag("numeric-cast"),
        MirConversion::SendFn => writer.tag("send-fn"),
        MirConversion::Prelude {
            call,
            location,
            fallibility,
        } => {
            writer.tag("prelude");
            writer.u64(call.0);
            writer.debug(location);
            encode_call_fallibility(writer, fallibility);
        }
    }
}

fn encode_values(writer: &mut CanonicalWriter, values: &[MirValueId]) {
    writer.len(values.len());
    for value in values {
        writer.u64(value.0);
    }
}

fn encode_field_path(writer: &mut CanonicalWriter, path: &[MirFieldId]) {
    writer.len(path.len());
    for field in path {
        writer.u64(field.0);
    }
}

fn encode_field_paths(writer: &mut CanonicalWriter, paths: &[Vec<MirFieldId>]) {
    writer.len(paths.len());
    for path in paths {
        encode_field_path(writer, path);
    }
}
fn encode_panic_location(writer: &mut CanonicalWriter, location: &crate::MIR::MirPanicLoc) {
    writer.u64(location.file.0);
    writer.u64(u64::from(location.line));
    writer.u64(u64::from(location.column));
}

fn encode_panic_context(writer: &mut CanonicalWriter, context: &MirPanicContext) {
    writer.str(&context.function);
    writer.str(&context.source_line);
    writer.u64(u64::from(context.caret));
    writer.len(context.locals.len());
    for (name, local) in &context.locals {
        writer.str(name);
        writer.u64(local.0);
    }
}

fn encode_hardware_op(writer: &mut CanonicalWriter, op: &MirHardwareOp) {
    match op {
        MirHardwareOp::RegisterRead {
            profile_id,
            block,
            register,
            width,
        } => {
            writer.tag("register-read");
            writer.str(profile_id);
            writer.str(block);
            writer.str(register);
            writer.str(width.as_str());
        }
        MirHardwareOp::RegisterWrite {
            profile_id,
            block,
            register,
            width,
        } => {
            writer.tag("register-write");
            writer.str(profile_id);
            writer.str(block);
            writer.str(register);
            writer.str(width.as_str());
        }
        MirHardwareOp::DmaStart {
            profile_id,
            channel,
            buffer_ty,
        } => {
            writer.tag("dma-start");
            writer.str(profile_id);
            writer.str(channel);
            writer.mir_type(buffer_ty);
        }
        MirHardwareOp::DmaWait {
            profile_id,
            channel,
            buffer_ty,
        } => {
            writer.tag("dma-wait");
            writer.str(profile_id);
            writer.str(channel);
            writer.mir_type(buffer_ty);
        }
    }
}
fn encode_loop_source_kind(writer: &mut CanonicalWriter, kind: &MirLoopSourceKind) {
    match kind {
        MirLoopSourceKind::Plain => writer.tag("plain"),
        MirLoopSourceKind::Chars => writer.tag("chars"),
        MirLoopSourceKind::LinesFile => writer.tag("lines-file"),
        MirLoopSourceKind::LinesStdin => writer.tag("lines-stdin"),
        MirLoopSourceKind::LinesProcessStream => writer.tag("lines-process-stream"),
        MirLoopSourceKind::ChannelReceiver => writer.tag("channel-receiver"),
        MirLoopSourceKind::EncodingReader { reader_type } => {
            writer.tag("encoding-reader");
            writer.str(reader_type);
        }
        MirLoopSourceKind::Iterable {
            coll_type,
            iter_type,
            iter_symbol,
            next_symbol,
        } => {
            writer.tag("iterable");
            writer.str(coll_type);
            writer.str(iter_type);
            writer.str(iter_symbol);
            writer.str(next_symbol);
        }
    }
}

fn encode_text_pattern_parts(
    writer: &mut CanonicalWriter,
    parts: &[crate::MIR::MirTextPatternPart],
) {
    writer.len(parts.len());
    for part in parts {
        match part {
            crate::MIR::MirTextPatternPart::Literal(value) => {
                writer.tag("literal");
                writer.str(value);
            }
            crate::MIR::MirTextPatternPart::Hole { kind, ty, span } => {
                writer.tag("hole");
                encode_text_hole_kind(writer, *kind);
                writer.u64(ty.0);
                writer.span(*span);
            }
        }
    }
}

fn encode_binary_pattern_parts(
    writer: &mut CanonicalWriter,
    parts: &[crate::MIR::MirBinaryPatternPart],
) {
    writer.len(parts.len());
    for part in parts {
        match part {
            crate::MIR::MirBinaryPatternPart::Literal(value) => {
                writer.tag("literal");
                writer.debug(value);
            }
            crate::MIR::MirBinaryPatternPart::Bits {
                width,
                ty,
                little,
                span,
            } => {
                writer.tag("bits");
                writer.u64(u64::from(*width));
                writer.u64(ty.0);
                writer.bool(*little);
                writer.span(*span);
            }
            crate::MIR::MirBinaryPatternPart::Rest { ty, span } => {
                writer.tag("rest");
                writer.u64(ty.0);
                writer.span(*span);
            }
        }
    }
}
fn encode_component_type(
    writer: &mut CanonicalWriter,
    descriptor: &crate::MIR::ComponentTypeDescriptor,
) {
    match descriptor {
        crate::MIR::ComponentTypeDescriptor::Int => writer.tag("int"),
        crate::MIR::ComponentTypeDescriptor::Float => writer.tag("float"),
        crate::MIR::ComponentTypeDescriptor::Bool => writer.tag("bool"),
        crate::MIR::ComponentTypeDescriptor::String => writer.tag("string"),
        crate::MIR::ComponentTypeDescriptor::List(inner) => {
            writer.tag("list");
            encode_component_type(writer, inner);
        }
        crate::MIR::ComponentTypeDescriptor::Option(inner) => {
            writer.tag("option");
            encode_component_type(writer, inner);
        }
        crate::MIR::ComponentTypeDescriptor::Result { ok, err } => {
            writer.tag("result");
            encode_component_type(writer, ok);
            encode_component_type(writer, err);
        }
        crate::MIR::ComponentTypeDescriptor::Record { name, fields } => {
            writer.tag("record");
            writer.str(name);
            writer.len(fields.len());
            for (index, field, descriptor) in fields {
                writer.len(*index);
                writer.str(field);
                encode_component_type(writer, descriptor);
            }
        }
    }
}

fn encode_component_signature(
    writer: &mut CanonicalWriter,
    signature: &crate::MIR::ComponentSignatureDescriptor,
) {
    writer.tag("component-signature");
    writer.len(signature.params.len());
    for param in &signature.params {
        encode_component_type(writer, param);
    }
    encode_component_type(writer, &signature.result);
}

fn encode_semantic_operation(writer: &mut CanonicalWriter, operation: &MirSemanticOp) {
    match operation {
        MirSemanticOp::DataEntriesToMap { call, local } => {
            writer.tag("data-entries-to-map");
            writer.u64(call.0);
            writer.u64(local.0);
        }
        MirSemanticOp::MathBuiltin { type_id, call, args } => {
            writer.tag("math-builtin");
            writer.u64(type_id.0);
            writer.u64(call.0);
            encode_values(writer, args);
        }
        MirSemanticOp::PreciseBuiltin { type_id, call, args } => {
            writer.tag("precise-builtin");
            writer.u64(type_id.0);
            writer.u64(call.0);
            encode_values(writer, args);
        }
        MirSemanticOp::Print { call, value } => {
            writer.tag("print");
            writer.u64(call.0);
            writer.u64(value.0);
        }
        MirSemanticOp::AmbientInput { call, prompt } => {
            writer.tag("ambient-input");
            writer.u64(call.0);
            writer.option_u64(prompt.map(|value| value.0));
        }
        MirSemanticOp::RequireStop {
            call,
            kind,
            condition,
            location,
            context,
            values,
            always_stops,
        } => {
            writer.tag("require-stop");
            writer.u64(call.0);
            encode_require_kind(writer, *kind);
            writer.option_u64(condition.map(|value| value.0));
            encode_panic_location(writer, location);
            encode_panic_context(writer, context);
            encode_values(writer, values);
            writer.bool(*always_stops);
        }
        MirSemanticOp::LayoutCompare {
            call,
            op,
            left,
            right,
        } => {
            writer.tag("layout-compare");
            writer.u64(call.0);
            encode_layout_compare_op(writer, *op);
            writer.u64(left.0);
            writer.u64(right.0);
        }
        MirSemanticOp::LayoutLiteral { inner } => {
            writer.tag("layout-literal");
            writer.u64(inner.0);
        }
        MirSemanticOp::StructLiteral {
            type_id,
            fields,
            extra,
            trait_coercion,
            boxed_fields,
        } => {
            writer.tag("struct-literal");
            writer.u64(type_id.0);
            writer.len(fields.len());
            for (field, value) in fields {
                writer.u64(field.0);
                writer.u64(value.0);
            }
            encode_struct_extra(writer, *extra);
            writer.option_u64(trait_coercion.map(|ty| ty.0));
            writer.len(boxed_fields.len());
            for field in boxed_fields {
                writer.u64(field.0);
            }
        }
        MirSemanticOp::CellGuardProject {
            map_call,
            split_call,
            guard,
            paths,
            editable,
            edit_paths_disjoint,
        } => {
            writer.tag("cell-guard-project");
            writer.u64(map_call.0);
            writer.option_u64(split_call.map(|call| call.0));
            writer.u64(guard.0);
            encode_field_paths(writer, paths);
            writer.bool(*editable);
            writer.bool(*edit_paths_disjoint);
        }
        MirSemanticOp::SharedGuardMap {
            call,
            guard,
            path,
            editable,
        } => {
            writer.tag("shared-guard-map");
            writer.u64(call.0);
            writer.u64(guard.0);
            writer.len(path.len());
            for field in path {
                writer.u64(field.0);
            }
            writer.bool(*editable);
        }
        MirSemanticOp::SharedGuardSplit {
            call,
            map_call,
            guard,
            first,
            second,
            editable,
        } => {
            writer.tag("shared-guard-split");
            writer.u64(call.0);
            writer.u64(map_call.0);
            writer.u64(guard.0);
            writer.len(2);
            encode_field_path(writer, first);
            encode_field_path(writer, second);
            writer.bool(*editable);
        }
        MirSemanticOp::SharedGuardWait {
            call,
            guard,
            condition,
            predicate,
        } => {
            writer.tag("shared-guard-wait");
            writer.u64(call.0);
            writer.u64(guard.0);
            writer.u64(condition.0);
            writer.u64(predicate.0);
        }
        MirSemanticOp::ConditionNotify {
            call,
            condition,
            all,
        } => {
            writer.tag("condition-notify");
            writer.u64(call.0);
            writer.u64(condition.0);
            writer.bool(*all);
        }
        MirSemanticOp::AllocNew { call, kind, args } => {
            writer.tag("alloc-new");
            writer.u64(call.0);
            encode_allocator_kind(writer, *kind);
            encode_call_args(writer, args);
        }
        MirSemanticOp::ColumnarRead {
            base,
            index,
            column,
            column_index,
            accessor,
        } => {
            writer.tag("columnar-read");
            writer.u64(base.0);
            writer.u64(index.0);
            writer.u64(column.0);
            writer.len(*column_index);
            writer.u64(accessor.0);
        }
        MirSemanticOp::StaticPreludeCall {
            call,
            args,
            owner_type_args,
            type_args,
        } => {
            writer.tag("static-prelude-call");
            writer.u64(call.0);
            encode_call_args(writer, args);
            writer.len(owner_type_args.len());
            for type_arg in owner_type_args {
                match type_arg {
                    crate::MIR::MirPreludeTypeArg::Type(ty) => {
                        writer.tag("type");
                        writer.mir_type(ty);
                    }
                    crate::MIR::MirPreludeTypeArg::HostUsize => writer.tag("host-usize"),
                }
            }
            writer.len(type_args.len());
            for type_arg in type_args {
                writer.mir_type(type_arg);
            }
        }
        MirSemanticOp::DecodeUnder {
            call,
            segment,
            inner,
        } => {
            writer.tag("decode-under");
            writer.u64(call.0);
            writer.u64(segment.0);
            writer.u64(inner.0);
        }
        MirSemanticOp::HardwareCall {
            call,
            op,
            receiver,
            args,
        } => {
            writer.tag("hardware-call");
            writer.u64(call.0);
            encode_hardware_op(writer, op);
            writer.option_u64(receiver.map(|value| value.0));
            encode_call_args(writer, args);
        }
        MirSemanticOp::BuiltinMethod {
            call,
            receiver,
            receiver_place,
            args,
            aggregate_fields,
        } => {
            writer.tag("builtin-method");
            writer.u64(call.0);
            writer.u64(receiver.0);
            writer.option_u64(receiver_place.map(|place| place.0));
            encode_values(writer, args);
            match aggregate_fields {
                Some(fields) => {
                    writer.bool(true);
                    encode_field_path(writer, fields);
                }
                None => writer.bool(false),
            }
        }
        MirSemanticOp::OptionLift2 {
            call,
            function,
            left,
            right,
        } => {
            writer.tag("option-lift2");
            writer.u64(call.0);
            writer.u64(function.0);
            writer.u64(left.0);
            writer.u64(right.0);
        }
        MirSemanticOp::ClosureMethod {
            receiver,
            args,
            call,
        } => {
            writer.tag("closure-method");
            writer.u64(receiver.0);
            writer.u64(call.0);
            encode_call_args(writer, args);
        }
        MirSemanticOp::HostBorrowCallback { callable, params } => {
            writer.tag("host-borrow-callback");
            writer.u64(callable.0);
            writer.len(params.len());
            for param in params {
                writer.mir_type(param);
            }
        }
        MirSemanticOp::TextPatternMatch {
            call,
            subject,
            parts,
        } => {
            writer.tag("text-pattern-match");
            writer.u64(call.0);
            writer.u64(subject.0);
            encode_text_pattern_parts(writer, parts);
        }
        MirSemanticOp::BinaryPatternMatch {
            call,
            subject,
            parts,
        } => {
            writer.tag("binary-pattern-match");
            writer.u64(call.0);
            writer.u64(subject.0);
            encode_binary_pattern_parts(writer, parts);
        }
        MirSemanticOp::NumericMethod { call, receiver } => {
            writer.tag("numeric-method");
            writer.u64(call.0);
            writer.u64(receiver.0);
        }
        MirSemanticOp::NumericBinaryMethod {
            call,
            receiver,
            argument,
        } => {
            writer.tag("numeric-binary-method");
            writer.u64(call.0);
            writer.u64(receiver.0);
            writer.u64(argument.0);
        }
        MirSemanticOp::OverflowOption {
            call,
            left,
            right,
            location,
        } => {
            writer.tag("overflow-option");
            writer.u64(call.0);
            writer.u64(left.0);
            writer.u64(right.0);
            match location {
                Some(location) => {
                    writer.bool(true);
                    encode_panic_location(writer, location);
                }
                None => writer.bool(false),
            }
        }
        MirSemanticOp::HandleMethod {
            call,
            receiver,
            args,
            frame_schedule,
            frame_schedule_derivation,
        } => {
            writer.tag("handle-method");
            writer.u64(call.0);
            writer.u64(receiver.0);
            encode_values(writer, args);
            match frame_schedule {
                Some(schedule) => {
                    writer.bool(true);
                    writer.str(&schedule.canonical_json());
                }
                None => writer.bool(false),
            }
            match frame_schedule_derivation {
                Some(reference) => {
                    writer.bool(true);
                    writer.str(&reference.id);
                }
                None => writer.bool(false),
            }
        }
        MirSemanticOp::PluginInvoke {
            call,
            handle,
            export_name,
            signature,
            args,
        } => {
            writer.tag("plugin-invoke");
            writer.u64(call.0);
            writer.u64(handle.0);
            writer.str(export_name);
            encode_component_signature(writer, signature);
            encode_values(writer, args);
        }
        MirSemanticOp::HttpRouterRegister {
            call,
            receiver,
            path,
            handler,
            method,
            handler_param_names,
            contract_json,
            location,
        } => {
            writer.tag("http-router-register");
            writer.u64(call.0);
            writer.u64(receiver.0);
            writer.u64(path.0);
            writer.u64(handler.0);
            writer.str(method.as_str());
            encode_app_string_list(writer, handler_param_names);
            writer.str(contract_json);
            encode_panic_location(writer, location);
        }
        MirSemanticOp::CoreClosureCall {
            call,
            kind,
            values,
            closure,
            site,
            label,
        } => {
            writer.tag("core-closure-call");
            writer.u64(call.0);
            encode_core_closure_kind(writer, kind.clone());
            encode_values(writer, values);
            writer.option_u64(closure.map(|value| value.0));
            writer.u64(site.0);
            writer.str(label);
        }
        MirSemanticOp::TaskGroup { call, kind, tasks } => {
            writer.tag("task-group");
            writer.u64(call.0);
            encode_task_group_kind(writer, *kind);
            encode_values(writer, tasks);
        }
        MirSemanticOp::Select { call, kind, values } => {
            writer.tag("select");
            writer.u64(call.0);
            encode_select_kind(writer, *kind);
            encode_values(writer, values);
        }
        MirSemanticOp::PolicyFunction { policy, values } => {
            writer.tag("policy-function");
            writer.u64(policy.0);
            encode_values(writer, values);
        }
        MirSemanticOp::InterruptFunction { interrupt, values } => {
            writer.tag("interrupt-function");
            writer.u64(interrupt.0);
            encode_values(writer, values);
        }
        MirSemanticOp::CarrierFact {
            call,
            receiver,
            field,
            notes,
        } => {
            writer.tag("carrier-fact");
            writer.u64(call.0);
            writer.u64(receiver.0);
            writer.u64(field.0);
            writer.bool(*notes);
        }
        MirSemanticOp::GcEdit {
            call,
            root,
            edges,
            edit,
            index,
            kind,
            site,
        } => {
            writer.tag("gc-edit");
            writer.u64(call.0);
            writer.u64(root.0);
            encode_values(writer, edges);
            writer.u64(edit.0);
            writer.option_u64(index.map(|value| value.0));
            encode_gc_edit_kind(writer, *kind);
            writer.u64(site.0);
        }
        MirSemanticOp::TypedTextInterp {
            call,
            kind,
            literals,
            holes,
        } => {
            writer.tag("typed-text-interp");
            writer.u64(call.0);
            encode_typed_head_kind(writer, *kind);
            writer.len(literals.len());
            for literal in literals {
                writer.str(literal);
            }
            encode_values(writer, holes);
        }
        MirSemanticOp::CCallback {
            call,
            callback,
            lambda,
        } => {
            writer.tag("c-callback");
            writer.u64(call.0);
            writer.u64(callback.0);
            writer.u64(lambda.0);
        }
        MirSemanticOp::HostCall { call, args } => {
            writer.tag("host-call");
            writer.u64(call.0);
            encode_call_args(writer, args);
        }
    }
}

fn encode_shape_field_names(
    writer: &mut CanonicalWriter,
    names: &crate::Shape::ShapeFieldNames,
) {
    writer.str(&names.text);
    writer.option_str(names.json.as_deref());
    writer.option_str(names.cbor.as_deref());
    writer.option_str(names.csv.as_deref());
    writer.option_str(names.toml.as_deref());
    writer.option_str(names.yaml.as_deref());
    writer.option_str(names.xml.as_deref());
    writer.str(&names.args);
    writer.str(&names.env);
    writer.option_str(names.db.as_deref());
    writer.option_str(names.layout.as_deref());
}

fn encode_field(writer: &mut CanonicalWriter, field: &crate::MIR::MirField) {
    writer.u64(field.id.0);
    writer.str(&field.name);
    encode_shape_field_names(writer, &field.shape_names);
    writer.bool(field.skip);
    writer.mir_type(&field.ty);
    writer.span(field.span);
    writer.bool(field.public);
    writer.bool(field.package_public);
    writer.bool(field.computed);
    writer.bool(field.has_default);
}

fn encode_type_def_kind(writer: &mut CanonicalWriter, kind: &MirTypeDefKind) {
    match kind {
        MirTypeDefKind::Struct { fields, methods } => {
            writer.tag("struct");
            writer.len(fields.len());
            for field in fields {
                encode_field(writer, field);
            }
            writer.debug(methods);
        }
        MirTypeDefKind::Enum { variants, methods } => {
            writer.tag("enum");
            writer.len(variants.len());
            for variant in variants {
                writer.str(&variant.name);
                writer.str(&variant.wire_name);
                writer.span(variant.span);
                match &variant.payload {
                    crate::MIR::MirVariantPayload::Unit => writer.tag("unit"),
                    crate::MIR::MirVariantPayload::Single(ty) => {
                        writer.tag("single");
                        writer.mir_type(ty);
                    }
                    crate::MIR::MirVariantPayload::Named(fields) => {
                        writer.tag("named");
                        writer.len(fields.len());
                        for field in fields {
                            encode_field(writer, field);
                        }
                    }
                }
                match variant.discriminant {
                    Some(value) => {
                        writer.bool(true);
                        writer.u64(value as u64);
                    }
                    None => writer.bool(false),
                }
            }
            writer.debug(methods);
        }
        MirTypeDefKind::Distinct { base, range } => {
            writer.tag("distinct");
            writer.mir_type(base);
            writer.debug(range);
        }
        MirTypeDefKind::Alias { target } => {
            writer.tag("alias");
            writer.mir_type(target);
        }
        MirTypeDefKind::UnitFamily { members } => {
            writer.tag("unit-family");
            writer.debug(members);
        }
    }
}

fn encode_param(writer: &mut CanonicalWriter, param: &crate::MIR::MirParam) {
    writer.len(param.index);
    writer.str(&param.name);
    writer.span(param.span);
    writer.mir_type(&param.ty);
    writer.debug(&param.access);
    writer.debug(&param.ownership);
    writer.str(&param.public_label);
}

fn encode_capture_param(
    writer: &mut CanonicalWriter,
    param: &crate::MIR::MirCaptureParam,
) {
    writer.len(param.slot);
    writer.str(&param.name);
    writer.span(param.span);
    writer.mir_type(&param.ty);
    writer.debug(&param.access);
    writer.debug(&param.ownership);
}

fn encode_foreign(writer: &mut CanonicalWriter, foreign: &MirForeign) {
    writer.u64(foreign.id.0);
    writer.u64(foreign.module_id.0);
    writer.str(&foreign.key);
    writer.str(&foreign.module);
    writer.str(&foreign.name);
    writer.span(foreign.span);
    writer.str(&foreign.symbol);
    writer.str(&foreign.path);
    writer.len(foreign.params.len());
    for param in &foreign.params {
        encode_param(writer, param);
    }
    match &foreign.return_type {
        Some(ty) => {
            writer.bool(true);
            writer.mir_type(ty);
        }
        None => writer.bool(false),
    }
    encode_foreign_abi(writer, &foreign.foreign_abi);
    encode_foreign_language(writer, foreign.foreign_language);
    encode_target_applicability(writer, foreign.target_applicability);
    encode_effects(writer, &foreign.effects);
    writer.option_u64(foreign.link.map(|link| link.0));
    writer.option_u64(foreign.callback.map(|callback| callback.0));
    writer.option_u64(foreign.handle.map(|handle| handle.0));
    writer.option_u64(foreign.close_function.map(|function| function.0));
    writer.option_u64(foreign.close_foreign.map(|foreign| foreign.0));
    writer.option_u64(foreign.undo_function.map(|function| function.0));
}

fn encode_failure_carrier(writer: &mut CanonicalWriter, carrier: &MirFailureCarrier) {
    match carrier {
        MirFailureCarrier::Infallible => writer.tag("infallible"),
        MirFailureCarrier::Result { success, error } => {
            writer.tag("result");
            writer.mir_type(success);
            writer.mir_type(error);
        }
        MirFailureCarrier::Optional { value } => {
            writer.tag("optional");
            writer.mir_type(value);
        }
        MirFailureCarrier::Diverges { value } => {
            writer.tag("diverges");
            writer.mir_type(value);
        }
    }
}

fn encode_call_fallibility(writer: &mut CanonicalWriter, fallibility: &MirCallFallibility) {
    match fallibility {
        MirCallFallibility::Infallible => writer.tag("infallible"),
        MirCallFallibility::Failure(carrier) => {
            writer.tag("failure");
            encode_failure_carrier(writer, carrier);
        }
    }
}

fn encode_core_call_fallibility(writer: &mut CanonicalWriter, fallibility: CoreCallFallibility) {
    writer.u64(match fallibility {
        CoreCallFallibility::Sema => 1,
    });
}

fn encode_core_call(writer: &mut CanonicalWriter, call: &crate::MIR::MirCoreCall) {
    writer.u64(call.id.0);
    writer.str(&call.key);
    writer.str(&call.module);
    writer.str(&call.member);
    writer.debug(&call.receiver_types);
    writer.len(call.arity);
    writer.len(call.max_arity);
    writer.debug(&call.borrow_mask);
    encode_core_call_fallibility(writer, call.fallibility);
    writer.debug(&call.effect);
    writer.debug(&call.effect_leaf);
    writer.debug(&call.sink_class);
    encode_core_pure_route(writer, call.pure_route);
    encode_interpreter_route(writer, call.interpreter_route);
    encode_core_symbol(writer, call.symbol);
    writer.u64(u64::from(call.coverage_bits));
    writer.bool(call.aot_direct);
    writer.bool(call.jit_direct);
    writer.option_str(call.jit_symbol.as_deref());
    writer.debug(&call.marker);
}
fn encode_prelude_call(writer: &mut CanonicalWriter, call: &MirPreludeCall) {
    writer.u64(call.id.0);
    encode_prelude_family(writer, call.family);
    writer.str(&call.module);
    writer.str(&call.member);
    match &call.symbol {
        crate::MIR::MirSymbol::Prelude(name) => {
            writer.tag("prelude-symbol");
            writer.str(name);
        }
        crate::MIR::MirSymbol::Runtime(name) => {
            writer.tag("runtime-symbol");
            writer.str(name);
        }
    }
    writer.debug(&call.signature);
    writer.debug(&call.effect);
    encode_call_fallibility(writer, &call.fallibility);
    encode_prelude_abi(writer, call.abi);
    match &call.db_metadata {
        Some(metadata) => {
            writer.bool(true);
            writer.u64(metadata.source_file.0);
            writer.str(&metadata.source_path);
            writer.span(metadata.source_span);
            writer.str(&metadata.statement_identity);
            writer.len(metadata.table_facts.len());
            for fact in &metadata.table_facts {
                writer.str(&fact.table_id);
                writer.bool(fact.read);
                writer.bool(fact.write);
            }
        }
        None => writer.bool(false),
    }
}

fn encode_function_form(writer: &mut CanonicalWriter, form: &crate::MIR::MirFunctionForm) {
    match form {
        crate::MIR::MirFunctionForm::TopLevel => writer.tag("top-level"),
        crate::MIR::MirFunctionForm::Method {
            owner,
            self_access,
        } => {
            writer.tag("method");
            writer.mir_type(owner);
            encode_access(writer, *self_access);
        }
        crate::MIR::MirFunctionForm::TraitMethod {
            owner,
            trait_ref,
            self_access,
            serde,
        } => {
            writer.tag("trait-method");
            writer.mir_type(owner);
            encode_trait_ref(writer, trait_ref);
            encode_access(writer, *self_access);
            encode_serde_codec(writer, *serde);
        }
    }
}

fn encode_web_param_reconstruction(
    writer: &mut CanonicalWriter,
    reconstruction: &crate::MIR::MirWebParamReconstruction,
) {
    writer.str(&reconstruction.local);
    writer.mir_type(&reconstruction.ty);
    writer.len(reconstruction.fields.len());
    for field in &reconstruction.fields {
        writer.str(&field.field);
        writer.str(&field.parameter);
        writer.mir_type(&field.ty);
    }
}

fn encode_local(writer: &mut CanonicalWriter, local: &crate::MIR::MirLocal) {
    writer.u64(local.id.0);
    writer.str(&local.name);
    writer.span(local.span);
    writer.mir_type(&local.ty);
    writer.u64(local.place.0);
    writer.debug(&local.ownership);
    writer.bool(local.mutable);
    writer.bool(local.comptime);
    writer.bool(local.uninit);
    writer.bool(local.arena_view);
    writer.bool(local.string_view);
    writer.bool(local.gc_root);
}

fn encode_place(writer: &mut CanonicalWriter, place: &MirPlace) {
    writer.u64(place.id.0);
    writer.span(place.span);
    writer.mir_type(&place.ty);
    match &place.base {
        MirPlaceBase::Local(local) => {
            writer.tag("local");
            writer.u64(local.0);
        }
        MirPlaceBase::Parameter(value) => {
            writer.tag("parameter");
            writer.u64(value.0);
        }
        MirPlaceBase::Capture(value) => {
            writer.tag("capture");
            writer.u64(value.0);
        }
        MirPlaceBase::Temporary(value) => {
            writer.tag("temporary");
            writer.u64(value.0);
        }
        MirPlaceBase::Static(name) => {
            writer.tag("static");
            writer.str(name);
        }
    }
    writer.len(place.projections.len());
    for projection in &place.projections {
        match projection {
            MirProjection::Field { field, span } => {
                writer.tag("field");
                writer.u64(field.0);
                writer.span(*span);
            }
            MirProjection::Index {
                kind,
                index,
                call,
                write_call,
                location,
                context,
                span,
            } => {
                writer.tag("index");
                encode_index_kind(writer, *kind);
                writer.u64(index.0);
                writer.u64(call.0);
                writer.option_u64(write_call.map(|call| call.0));
                writer.debug(location);
                writer.debug(context);
                writer.span(*span);
            }
            MirProjection::Deref { span } => {
                writer.tag("deref");
                writer.span(*span);
            }
        }
    }
    writer.debug(&place.access);
    writer.option_str(place.persist_key.as_deref());
}

fn encode_function(writer: &mut CanonicalWriter, function: &MirFunction) {
    writer.u64(function.id.0);
    writer.u64(function.module_id.0);
    writer.str(&function.key);
    writer.str(&function.module);
    writer.str(&function.name);
    writer.span(function.span);
    writer.debug(&function.kind);
    encode_function_form(writer, &function.form);
    writer.debug(&function.visibility);
    writer.debug(&function.target_applicability);
    writer.debug(&function.web_bucket);
    writer.debug(&function.web_marker);
    encode_generic_params(writer, &function.generic_params);
    let mut capture_params = function.capture_params.iter().collect::<Vec<_>>();
    capture_params.sort_by_key(|param| (param.slot, param.name.as_str()));
    writer.len(capture_params.len());
    for param in capture_params {
        encode_capture_param(writer, param);
    }
    writer.len(function.params.len());
    for param in &function.params {
        encode_param(writer, param);
    }
    match &function.declared_return {
        Some(ty) => {
            writer.bool(true);
            writer.mir_type(ty);
        }
        None => writer.bool(false),
    }
    writer.mir_type(&function.return_type);
    match &function.generator {
        Some(generator) => {
            writer.bool(true);
            writer.mir_type(&generator.item);
        }
        None => writer.bool(false),
    }
    encode_failure_carrier(writer, &function.failure);
    writer.debug(&function.effects);
    writer.debug(&function.captures);
    encode_optimization_facts(writer, &function.optimization);
    writer.bool(function.is_unsafe);
    writer.debug(&function.unsafe_gate);
    writer.bool(function.is_pure);
    writer.debug(&function.memo_bound);
    writer.bool(function.is_reactive);
    writer.debug(&function.reactive_upgrades);
    writer.bool(function.is_inline);
    writer.bool(function.is_inline_always);
    writer.bool(function.is_scalar);
    writer.debug(&function.kernel_proof);
    writer.bool(function.gc_return);
    writer.debug(&function.return_view_provenance);
    writer.len(function.web_param_reconstructions.len());
    for reconstruction in &function.web_param_reconstructions {
        encode_web_param_reconstruction(writer, reconstruction);
    }
    let mut blocks = function.blocks.iter().collect::<Vec<_>>();
    blocks.sort_by_key(|block| block.id);
    writer.len(blocks.len());
    for block in blocks {
        writer.u64(block.id.0);
        writer.span(block.span);
        writer.len(block.instructions.len());
        for instruction in &block.instructions {
            writer.u64(instruction.id.0);
            writer.span(instruction.span);
            writer.debug(&instruction.source_line);
            writer.option_u64(instruction.result.map(|value| value.0));
            match &instruction.ty {
                Some(ty) => {
                    writer.bool(true);
                    writer.mir_type(ty);
                }
                None => writer.bool(false),
            }
            writer.operation(&instruction.operation);
        }
        writer.debug(&block.terminator);
    }
    let mut locals = function.locals.iter().collect::<Vec<_>>();
    locals.sort_by_key(|local| local.id);
    writer.len(locals.len());
    for local in locals {
        encode_local(writer, local);
    }
    let mut values = function.values.iter().collect::<Vec<_>>();
    values.sort_by_key(|(value, _, _, _)| *value);
    writer.len(values.len());
    for (value, ty, span, ownership) in values {
        writer.u64(value.0);
        writer.mir_type(ty);
        writer.span(*span);
        writer.debug(ownership);
    }
    let mut places = function.places.iter().collect::<Vec<_>>();
    places.sort_by_key(|place| place.id);
    writer.len(places.len());
    for place in places {
        encode_place(writer, place);
    }
    let mut scopes = function.scopes.iter().collect::<Vec<_>>();
    scopes.sort_by_key(|scope| scope.id);
    writer.debug(&scopes);
    let mut drops = function.drops.iter().collect::<Vec<_>>();
    drops.sort_by_key(|drop| (drop.place, drop.span.start, drop.span.end));
    writer.debug(&drops);
}

fn encode_optimization_facts(writer: &mut CanonicalWriter, facts: &MirOptimizationFacts) {
    writer.bool(facts.pure);
    writer.bool(facts.replayable);
    writer.bool(facts.diverges);
    writer.bool(facts.inline);
    writer.bool(facts.inline_always);
    writer.bool(facts.gc_return);
    writer.bool(facts.gc_scope);
    writer.bool(facts.kernel);
    writer.bool(facts.auto_vectorizable);
    writer.bool(facts.no_aliasing);
    writer.bool(facts.no_early_exit);
    writer.bool(facts.no_cross_iteration_dependencies);
    writer.debug(&facts.pass_ids);
    writer.debug(&facts.loop_facts);
    writer.debug(&facts.bounds_facts);
    writer.len(facts.vector_facts.len());
    for fact in &facts.vector_facts {
        writer.u64(fact.loop_header.0);
        match fact.cursor {
            Some(cursor) => {
                writer.bool(true);
                writer.u64(cursor.0);
            }
            None => writer.bool(false),
        }
        writer.len(fact.body_blocks.len());
        for block in &fact.body_blocks {
            writer.u64(block.0);
        }
        match fact.advance_block {
            Some(block) => {
                writer.bool(true);
                writer.u64(block.0);
            }
            None => writer.bool(false),
        }
        writer.tag(fact.rule.as_str());
        writer.len(fact.accesses.len());
        for access in &fact.accesses {
            match access.root {
                MirVectorAccessRoot::Place(place) => {
                    writer.tag("place");
                    writer.u64(place.0);
                }
                MirVectorAccessRoot::Value(value) => {
                    writer.tag("value");
                    writer.u64(value.0);
                }
            }
            writer.option_u64(access.field.map(|field| field.0));
            writer.tag(access.layout.as_str());
            writer.option_u64(access.column_index.map(|index| index as u64));
        }
        writer.tag(fact.layout.as_str());
        match &fact.element_type {
            Some(ty) => {
                writer.bool(true);
                writer.mir_type(ty);
            }
            None => writer.bool(false),
        }
        writer.bool(fact.packed);
        writer.option_u64(fact.lane_width.map(u64::from));
        writer.bool(fact.no_aliasing);
        writer.bool(fact.no_early_exit);
        writer.bool(fact.effect_free_body);
        writer.bool(fact.no_cross_iteration_dependencies);
        match &fact.fixed_reduction {
            Some(reduction) => {
                writer.bool(true);
                writer.u64(reduction.accumulator.0);
                writer.u64(reduction.addend.0);
                writer.u64(reduction.seed.0);
                writer.option_u64(reduction.condition.map(|value| value.0));
                writer.len(reduction.source_operations.len());
                for operation in &reduction.source_operations {
                    writer.u64(operation.0);
                }
                writer.u64(reduction.exit.0);
                writer.u64(reduction.order.lanes as u64);
                writer.u64(reduction.order.seed_lane as u64);
                writer.tag(reduction.order.tree_name());
            }
            None => writer.bool(false),
        }
        writer.span(fact.span);
        match &fact.decision {
            MirOptimizationDecision::Eligible => writer.tag("eligible"),
            MirOptimizationDecision::Rejected(reason) => {
                writer.tag("rejected");
                writer.tag(reason.as_str());
            }
        }
    }
    writer.debug(&facts.fusion_facts);
    let mut acceleration_facts = facts.acceleration_facts.iter().collect::<Vec<_>>();
    acceleration_facts.sort_by_key(|fact| (fact.loop_header, fact.transform));
    writer.len(acceleration_facts.len());
    for fact in acceleration_facts {
        writer.u64(fact.loop_header.0);
        writer.tag(fact.transform.as_str());
        writer.bool(fact.workload.nested_reuse);
        writer.bool(fact.workload.single_pass);
        writer.bool(fact.proof.source_proven);
        writer.bool(fact.proof.independent_iterations);
        writer.bool(fact.proof.no_aliasing);
        writer.bool(fact.proof.no_cross_iteration_dependencies);
        writer.bool(fact.proof.no_early_exit);
        writer.bool(fact.proof.effect_free_body);
        writer.bool(fact.proof.ownership_safe);
        writer.bool(fact.proof.failure_order_preserved);
        writer.span(fact.span);
    }
}

fn encode_mir_constant(writer: &mut CanonicalWriter, value: &MirConstant) {
    match value {
        MirConstant::Int {
            value,
            width,
            spelling,
        } => {
            writer.tag("int");
            writer.u64(*value as u64);
            match width {
                Some((signed, bits)) => {
                    writer.bool(true);
                    writer.bool(*signed);
                    writer.u64(u64::from(*bits));
                }
                None => writer.bool(false),
            }
            writer.option_str(spelling.as_deref());
        }
        MirConstant::Float {
            value,
            f32,
            spelling,
        } => {
            writer.tag("float");
            writer.u64(value.to_bits());
            writer.bool(*f32);
            writer.option_str(spelling.as_deref());
        }
        MirConstant::Bool(value) => {
            writer.tag("bool");
            writer.bool(*value);
        }
        MirConstant::Char(value) => {
            writer.tag("char");
            writer.u64(u64::from(*value as u32));
        }
        MirConstant::String(value) => {
            writer.tag("string");
            writer.str(value);
        }
        MirConstant::Bytes(value) => {
            writer.tag("bytes");
            writer.len(value.len());
            writer.bytes.extend_from_slice(value);
        }
        MirConstant::Unit => writer.tag("unit"),
        MirConstant::BigInt(value) => {
            writer.tag("bigint");
            writer.str(value);
        }
        MirConstant::List(values) => {
            writer.tag("list");
            writer.len(values.len());
            for value in values {
                encode_mir_constant(writer, value);
            }
        }
        MirConstant::Map(values) => {
            writer.tag("map");
            writer.len(values.len());
            for (key, value) in values {
                encode_mir_const_key(writer, key);
                encode_mir_constant(writer, value);
            }
        }
        MirConstant::Struct { type_name, fields } => {
            writer.tag("struct");
            writer.str(type_name);
            writer.len(fields.len());
            for (name, value) in fields {
                writer.str(name);
                encode_mir_constant(writer, value);
            }
        }
        MirConstant::Enum {
            type_name,
            variant,
            args,
        } => {
            writer.tag("enum");
            writer.str(type_name);
            writer.str(variant);
            writer.len(args.len());
            for (name, value) in args {
                writer.option_str(name.as_deref());
                encode_mir_constant(writer, value);
            }
        }
        MirConstant::Present(value) => {
            writer.tag("present");
            encode_mir_constant(writer, value);
        }
        MirConstant::Failed(report) => {
            writer.tag("failed");
            match report {
                MirConstReport::Clean(ty) => {
                    writer.tag("clean");
                    writer.mir_type(ty);
                }
                MirConstReport::Told(value) => {
                    writer.tag("told");
                    encode_mir_constant(writer, value);
                }
            }
        }
    }
}

fn encode_mir_const_key(writer: &mut CanonicalWriter, key: &MirConstKey) {
    match key {
        MirConstKey::Int(value) => {
            writer.tag("int");
            writer.u64(*value as u64);
        }
        MirConstKey::String(value) => {
            writer.tag("string");
            writer.str(value);
        }
        MirConstKey::Bool(value) => {
            writer.tag("bool");
            writer.bool(*value);
        }
        MirConstKey::Char(value) => {
            writer.tag("char");
            writer.u64(u64::from(*value as u32));
        }
        MirConstKey::Tuple(values) => {
            writer.tag("tuple");
            writer.len(values.len());
            for (name, key) in values {
                writer.str(name);
                encode_mir_const_key(writer, key);
            }
        }
        MirConstKey::Struct { type_name, fields } => {
            writer.tag("struct");
            writer.str(type_name);
            writer.len(fields.len());
            for (name, key) in fields {
                writer.str(name);
                encode_mir_const_key(writer, key);
            }
        }
        MirConstKey::Enum { type_name, variant } => {
            writer.tag("enum");
            writer.str(type_name);
            writer.str(variant);
        }
    }
}
