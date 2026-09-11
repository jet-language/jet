//! The single checked-TIR to semantic-MIR lowering boundary.
//!
//! This module accepts only [`super::TirProgram`].  Parsing, checking, and AST
//! lowering happen before this seam; no backend source or engine policy enters
//! the canonical MIR model.

#![allow(dead_code, unused_imports)]

use super::artifact_plan::{
    TirArtifactFacts, TirArtifactKind, TirArtifactPlan, TirArtifactTarget, TirCallbackFact,
    TirCliDefault, TirCliEntry, TirCliInput, TirCliInputShape, TirCoveragePoint, TirEntryOutput,
    TirEntrySpec, TirForeignFact, TirFunctionRef, TirHarnessKind, TirHarnessPlan, TirImportFact,
    TirImportItem, TirImportKind, TirItemRef, TirJobArgument, TirJobFact, TirLinkUnit,
    TirModuleFact, TirParamFact, TirTargetApplicability, TirTestFact, TirTestKind,
};
use super::tir_to_mir_core::lower_core_records;
use super::tir_to_mir_expr::{lower_expr, lower_pattern};
use super::tir_to_mir_stmt::lower_stmts;
use super::tir_to_mir_types::{
    lower_mir_access, lower_mir_convention, lower_param_zone, lower_type, lower_type_defs,
    lower_view_provenance, TirAccess, TirDeclarations, TirParam,
};
use super::{
    TCallArg, TContract, TContractDisposition, TContractResult, TExpr, TExprKind, TFailureCarrier,
    TFunc, TFuncKind, TGenericParam, THardwareSetup, TLambda, TLambdaBody, TLocal, TPattern,
    TPlace, TPreludeRoute, TRoutePlan, TStmt, TTargetApplicability, TVisibility, TirErasureReason,
    TirProgram,
};
use jet_foundation::Diagnostics::Span;
use jet_foundation::AST::{AccessConvention, CtKey, CtReport, CtValue, Type};
use jet_foundation::MIR::{
    stable_id, MirAccess, MirArtifactId, MirArtifactKind, MirArtifactPlan, MirArtifactRequest,
    MirArtifactTarget, MirAssociatedTypeDecl, MirAssociatedTypeValue, MirBasicBlock,
    MirBinaryDispatch, MirBinaryPatternPart, MirBlockId, MirCImportLink, MirCLib,
    MirCOverlayOverride, MirCallArg, MirCallFallibility, MirCallSignature, MirCallbackAdapter,
    MirCallbackId, MirCallee, MirCaptureFacts, MirCaptureOperand, MirCaptureParam, MirCffiFacts,
    MirCliCommand, MirCliDefault, MirCliEntry, MirCliInput, MirCliInputShape, MirCliValueKind,
    MirCloseAdapter, MirConstant, MirConstantDef, MirConstantId, MirCoreCallId, MirCoreClosureKind,
    MirCoveragePoint, MirDropAction, MirDropEdge, MirEffectFacts, MirEntryKind, MirEntryOutput,
    MirEntrySpec, MirFailureCarrier, MirFieldId, MirForeign, MirForeignAbi, MirForeignId,
    MirForeignLanguage, MirFunction, MirFunctionForm, MirFunctionId, MirFunctionKind,
    MirGeneratorFacts, MirHandleId, MirHandleLifecycle, MirHandleOwnership, MirHandlePayload,
    MirHardwareSetup, MirHarnessId, MirHarnessKind, MirHarnessPlan, MirImplDef, MirImplId,
    MirImport, MirImportId, MirImportItem, MirImportKind, MirIndexKind, MirInstruction, MirJob,
    MirJobCachePolicy, MirJobDispatch, MirJobId, MirJobSchedule, MirJobScope, MirJobSkip,
    MirKernelFacts, MirLinkArtifact, MirLinkArtifactKind, MirLinkUnit, MirLinkUnitId, MirLocal,
    MirLocalId, MirNameFacts, MirNominalRef, MirOpId, MirOperation, MirOptimizationFacts,
    MirOutputCheck, MirOutputCheckId, MirOwnership, MirPackageFacts, MirPanicContext, MirPanicLoc,
    MirParam, MirPattern, MirPatternBinding, MirPatternField, MirPatternPosition, MirPatternShape,
    MirPlace, MirPlaceBase, MirPlaceId, MirPreludeAbi, MirPreludeCall, MirPreludeCallId,
    MirPreludeFamily, MirProgram, MirProjection, MirRuntimePartId, MirScope, MirScopeId,
    MirScopeKind, MirSemanticOp, MirSerdeCodec, MirSiteId, MirSourceFile, MirSourceFileId,
    MirSymbol, MirTargetApplicability, MirTerminator, MirTestCase, MirTestId, MirTestKind,
    MirTextPatternPart, MirTraitDef, MirTraitId, MirTraitMethod, MirTraitMethodId, MirTraitRef,
    MirType, MirTypeDef, MirTypeDefKind, MirTypeId, MirTypeKind, MirUnsafeGate, MirValueId,
    MirVariantPayload, MirVisibility, MirWebParamField, MirWebParamReconstruction,
    MIR_SCHEMA_VERSION,
};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

/// A checked-TIR lowering failure.  Every failure remains attached to the
/// source function span; no untyped/opaque MIR node is synthesized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LowerError {
    pub span: Span,
    pub message: String,
}

impl LowerError {
    pub(super) fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}

impl fmt::Display for LowerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "checked TIR cannot lower to MIR at {}..{}: {}",
            self.span.start, self.span.end, self.message
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FunctionTargetKey {
    module: String,
    owner: String,
    trait_name: Option<String>,
    method: String,
    rhs: Option<String>,
}

#[derive(Debug)]
pub(super) struct FunctionRegistry {
    /// The key produced by the checked function lowerer.  This is the only
    /// direct semantic-key index; display names are kept separate so a
    /// qualified display spelling cannot masquerade as a canonical key.
    by_key: HashMap<String, Vec<MirFunctionId>>,
    /// Top-level entry candidates are kept separate from ordinary callable
    /// names so an imported method cannot satisfy a program entry by leaf name.
    top_level_by_key: HashMap<String, Vec<MirFunctionId>>,
    /// Top-level functions grouped by declaring module for unqualified entry
    /// selection.
    top_level_by_module_name: HashMap<(String, String), Vec<MirFunctionId>>,
    /// Rust/JIT display names are exact spellings, not suffix aliases.
    by_name: HashMap<String, Vec<MirFunctionId>>,
    by_module_name: HashMap<(String, String), Vec<MirFunctionId>>,
    /// This is the resolver seam for a checked method reference whose display
    /// spelling omits the module or whose imported owner is already qualified.
    by_target: HashMap<FunctionTargetKey, Vec<MirFunctionId>>,
    /// Untagged instance calls carry only owner + method.  Keep this separate
    /// from `by_target` so an operator reference never falls through to an
    /// unrelated method with the same leaf.
    by_owner_method: HashMap<(String, String, String), Vec<MirFunctionId>>,
    by_identity: HashMap<String, MirFunctionId>,
    pub(super) receiver_access: HashMap<MirFunctionId, MirAccess>,
}

impl FunctionRegistry {
    fn build(functions: &[TFunc]) -> Result<Self, LowerError> {
        let mut registry = Self {
            by_key: HashMap::new(),
            top_level_by_key: HashMap::new(),
            top_level_by_module_name: HashMap::new(),
            by_name: HashMap::new(),
            by_module_name: HashMap::new(),
            by_target: HashMap::new(),
            by_owner_method: HashMap::new(),
            by_identity: HashMap::new(),
            receiver_access: HashMap::new(),
        };
        for function in functions {
            let identity = function_identity(function);
            let id = MirFunctionId(stable_id("mir-function", &identity));
            let receiver = match &function.kind {
                TFuncKind::Method { self_conv, .. } | TFuncKind::TraitMethod { self_conv, .. } => {
                    *self_conv
                }
                TFuncKind::TopLevel => None,
            };
            if let Some(access) = receiver {
                registry
                    .receiver_access
                    .insert(id, lower_mir_convention(access));
            }
            if registry.by_identity.insert(identity.clone(), id).is_some() {
                return Err(LowerError::new(
                    function.source_span,
                    format!("duplicate checked function identity `{identity}`"),
                ));
            }
            registry
                .by_key
                .entry(function.key.clone())
                .or_default()
                .push(id);
            if matches!(&function.kind, TFuncKind::TopLevel) {
                registry
                    .top_level_by_key
                    .entry(function.key.clone())
                    .or_default()
                    .push(id);
            }
            registry
                .by_name
                .entry(function.name.clone())
                .or_default()
                .push(id);
            registry
                .by_module_name
                .entry((function.module.clone(), function.name.clone()))
                .or_default()
                .push(id);
            if matches!(&function.kind, TFuncKind::TopLevel) {
                registry
                    .top_level_by_module_name
                    .entry((function.module.clone(), function.name.clone()))
                    .or_default()
                    .push(id);
            }
            if let Some(target) = function_target_key(function) {
                if matches!(
                    &function.kind,
                    TFuncKind::Method { .. } | TFuncKind::TraitMethod { .. }
                ) {
                    registry
                        .by_owner_method
                        .entry((
                            target.module.clone(),
                            target.owner.clone(),
                            target.method.clone(),
                        ))
                        .or_default()
                        .push(id);
                }
                registry.by_target.entry(target).or_default().push(id);
            }
        }
        Ok(registry)
    }

    fn id_for(&self, function: &TFunc) -> Result<MirFunctionId, LowerError> {
        let identity = function_identity(function);
        self.by_identity.get(&identity).copied().ok_or_else(|| {
            LowerError::new(
                function.source_span,
                format!("missing checked function `{identity}`"),
            )
        })
    }

    fn candidates(&self, name: &str, current_module: &str) -> Vec<MirFunctionId> {
        let mut candidates = if name.contains("::") {
            self.by_key
                .get(name)
                .or_else(|| {
                    self.by_module_name
                        .get(&(current_module.to_string(), name.to_string()))
                })
                .or_else(|| self.by_name.get(name))
                .cloned()
                .unwrap_or_default()
        } else {
            self.by_module_name
                .get(&(current_module.to_string(), name.to_string()))
                .cloned()
                .unwrap_or_default()
        };
        if candidates.is_empty() {
            candidates = self.typed_candidates(name, current_module);
        }
        candidates.sort_unstable();
        candidates.dedup();
        candidates
    }

    fn typed_candidates(&self, name: &str, current_module: &str) -> Vec<MirFunctionId> {
        let Some(separator) = top_level_last_separator(name) else {
            return Vec::new();
        };
        let raw_prefix = &name[..separator];
        let method_part = &name[separator + 2..];
        let Some((method, rhs)) = split_method_rhs(method_part) else {
            return Vec::new();
        };
        let owner_method = canonical_target_type(current_module, raw_prefix);
        if let Some((raw_owner, trait_name)) = raw_prefix.rsplit_once("::") {
            let candidates =
                self.typed_target_candidates(raw_owner, trait_name, method, rhs, current_module);
            if rhs.is_some() || !candidates.is_empty() {
                return candidates;
            }
        }
        if raw_prefix.contains("::") {
            self.by_owner_method
                .iter()
                .filter(|((_, owner, candidate_method), _)| {
                    owner == &owner_method && candidate_method == method
                })
                .flat_map(|(_, ids)| ids.iter().copied())
                .collect()
        } else {
            self.by_owner_method
                .get(&(current_module.to_string(), owner_method, method.to_string()))
                .cloned()
                .unwrap_or_default()
        }
    }

    fn typed_target_candidates(
        &self,
        raw_owner: &str,
        trait_name: &str,
        method: &str,
        rhs: Option<&str>,
        current_module: &str,
    ) -> Vec<MirFunctionId> {
        let owner = canonical_target_type(current_module, raw_owner);
        self.by_target
            .iter()
            .filter(|(target, _)| {
                let rhs_matches = match rhs {
                    Some(rhs) => {
                        let expected = canonical_target_type(&target.module, rhs);
                        target.rhs.as_deref() == Some(expected.as_str())
                    }
                    None => {
                        target.rhs.is_none() || target.rhs.as_deref() == Some(owner.as_str())
                    }
                };
                target.owner == owner
                    && target.trait_name.as_deref() == Some(trait_name)
                    && target.method == method
                    && (raw_owner.contains("::") || target.module == current_module)
                    && rhs_matches
            })
            .flat_map(|(_, ids)| ids.iter().copied())
            .collect()
    }

    fn lookup(&self, name: &str, current_module: &str) -> Option<MirFunctionId> {
        let candidates = self.candidates(name, current_module);
        match candidates.as_slice() {
            [id] => Some(*id),
            _ => None,
        }
    }

    fn resolve(
        &self,
        name: &str,
        current_module: &str,
        span: Span,
    ) -> Result<MirFunctionId, LowerError> {
        let candidates = self.candidates(name, current_module);
        match candidates.as_slice() {
            [id] => Ok(*id),
            [] => Err(LowerError::new(
                span,
                format!("missing checked function target `{name}`"),
            )),
            _ => Err(LowerError::new(
                span,
                format!("ambiguous checked function target `{name}`"),
            )),
        }
    }

    fn resolve_entry(&self, name: &str, span: Span) -> Result<MirFunctionId, LowerError> {
        let mut candidates = self.top_level_by_key.get(name).cloned().unwrap_or_default();
        if candidates.is_empty() && !name.contains("::") {
            candidates = self
                .top_level_by_module_name
                .iter()
                .filter(|((_, function_name), _)| function_name == name)
                .flat_map(|(_, ids)| ids.iter().copied())
                .collect();
            candidates.sort_unstable();
            candidates.dedup();
        }
        match candidates.as_slice() {
            [id] => Ok(*id),
            [] => Err(LowerError::new(
                span,
                format!("missing checked entry function `{name}`"),
            )),
            _ => Err(LowerError::new(
                span,
                format!("ambiguous checked entry function `{name}`"),
            )),
        }
    }
}

fn is_operator_trait(name: &str) -> bool {
    matches!(
        name,
        crate::Syntax::TRAIT_ADD
            | crate::Syntax::TRAIT_SUB
            | crate::Syntax::TRAIT_MUL
            | crate::Syntax::TRAIT_DIV
            | crate::Syntax::TRAIT_EQUATABLE
            | crate::Syntax::TRAIT_COMPARABLE
    )
}

/// Canonicalize one nominal spelling relative to the function's declaring
/// module.  A generated imported owner can already carry that module, while a
/// checked method reference generally carries only the local type leaf.
fn canonical_target_type(module: &str, text: &str) -> String {
    let text = text.trim();
    if module.is_empty() {
        return text.to_string();
    }
    let prefix = format!("{module}::");
    if text.starts_with(&prefix) {
        let mut remainder = text;
        while remainder.starts_with(&prefix) {
            remainder = &remainder[prefix.len()..];
        }
        return format!("{prefix}{remainder}");
    }
    if text.contains("::") || text.contains('.') {
        text.to_string()
    } else {
        format!("{prefix}{text}")
    }
}
fn top_level_last_separator(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut last = None;
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'<' => depth += 1,
            b'>' => depth = depth.saturating_sub(1),
            b':' if depth == 0 && index + 1 < bytes.len() && bytes[index + 1] == b':' => {
                last = Some(index);
                index += 1;
            }
            _ => {}
        }
        index += 1;
    }
    last
}

fn split_method_rhs(method_part: &str) -> Option<(&str, Option<&str>)> {
    if let Some(open) = method_part.find('<') {
        if method_part.ends_with('>') && open > 0 {
            return Some((
                &method_part[..open],
                Some(&method_part[open + 1..method_part.len() - 1]),
            ));
        }
    }
    (!method_part.is_empty()).then_some((method_part, None))
}
fn target_owner_name(ty: &Type) -> String {
    match ty {
        Type::Named(name) => name.clone(),
        _ => ty.name(),
    }
}

fn method_leaf(name: &str) -> &str {
    name.rsplit_once("::").map_or(name, |(_, leaf)| leaf)
}

fn function_operator_rhs(function: &TFunc) -> Option<&str> {
    let separator = top_level_last_separator(&function.key)?;
    let method_part = &function.key[separator + 2..];
    split_method_rhs(method_part).and_then(|(_, rhs)| rhs)
}

fn function_target_key(function: &TFunc) -> Option<FunctionTargetKey> {
    let (owner_type, trait_name) = match &function.kind {
        TFuncKind::Method { owner_type, .. } => (owner_type, None),
        TFuncKind::TraitMethod {
            owner_type,
            trait_name,
            ..
        } => (owner_type, Some(trait_name.as_str())),
        TFuncKind::TopLevel => return None,
    };
    let owner = canonical_target_type(&function.module, &target_owner_name(owner_type));
    let rhs = trait_name
        .filter(|trait_name| is_operator_trait(trait_name))
        .map(|_| {
            function_operator_rhs(function)
                .map(|rhs| canonical_target_type(&function.module, rhs))
                .unwrap_or_else(|| owner.clone())
        });
    Some(FunctionTargetKey {
        module: function.module.clone(),
        owner,
        trait_name: trait_name.map(str::to_string),
        method: method_leaf(&function.name).to_string(),
        rhs,
    })
}

fn function_identity(function: &TFunc) -> String {
    format!(
        "{}|{}|{}|{}",
        function.key, function.source_file, function.source_span.start, function.source_span.end
    )
}

pub fn lower_tir_to_mir(program: &TirProgram) -> Result<MirProgram, LowerError> {
    let function_registry = FunctionRegistry::build(&program.funcs)?;
    let mut types = lower_type_defs(&program.declarations.type_defs)?;
    // Trait names are checked nominal identities too.  Unlike user types,
    // trait rows historically did not enter the projection map, so preserve
    // each declaration's canonical key at the MIR boundary.
    let mut nominal_identities = program.nominal_identities.clone();
    for trait_def in &program.declarations.traits {
        nominal_identities
            .entry(trait_def.name.clone())
            .or_insert_with(|| trait_def.key.clone());
    }
    retarget_type_function_refs(
        &mut types,
        &program.declarations.type_defs,
        &function_registry,
    );
    types.sort_unstable_by_key(|ty| ty.id);
    let mut functions: Vec<MirFunction> = Vec::with_capacity(program.funcs.len());
    let mut prelude_calls: Vec<MirPreludeCall> = Vec::new();
    let mut callbacks: Vec<MirCallbackAdapter> = Vec::new();
    let mut source_files: Vec<MirSourceFile> = Vec::new();
    let mut type_instances = declared_type_instances(&types);
    for definition in &types {
        for embedded in type_def_types(definition) {
            let instance =
                canonical_type_instance(&types, &mir_type_as_ast(&embedded), definition.span)?;
            merge_type_instance(&mut type_instances, instance, definition.span)?;
        }
    }
    for function in &program.funcs {
        let (lowered, calls, files, instances, nested, function_callbacks) = lower_function(
            function,
            &types,
            &nominal_identities,
            &program.declarations.traits,
            &function_registry,
            &program.source_files,
            None,
            None,
            None,
            None,
        )
        .map_err(|error| {
            LowerError::new(error.span, format!("{}: {}", function.name, error.message))
        })?;
        for embedded in function_type_rows(&lowered) {
            let instance =
                canonical_type_instance(&types, &mir_type_as_ast(&embedded), lowered.span)?;
            merge_type_instance(&mut type_instances, instance, lowered.span)?;
        }
        for nested_function in &nested {
            for embedded in function_type_rows(nested_function) {
                let instance = canonical_type_instance(
                    &types,
                    &mir_type_as_ast(&embedded),
                    nested_function.span,
                )?;
                merge_type_instance(&mut type_instances, instance, nested_function.span)?;
            }
        }
        functions.push(lowered);
        functions.extend(nested);
        for instance in instances {
            merge_type_instance(&mut type_instances, instance, function.source_span)?;
        }
        for call in calls {
            if let Some(existing) = prelude_calls.iter().find(|row| row.id == call.id) {
                if existing.module != call.module
                    || existing.member != call.member
                    || existing.symbol != call.symbol
                    || existing.signature != call.signature
                    || existing.abi != call.abi
                {
                    return Err(LowerError::new(
                        function.source_span,
                        format!("conflicting semantic Prelude row {:?}", call.id),
                    ));
                }
            } else {
                prelude_calls.push(call);
            }
        }
        for callback in function_callbacks {
            if let Some(existing) = callbacks.iter().find(|row| row.id == callback.id) {
                if format!("{existing:?}") != format!("{callback:?}") {
                    return Err(LowerError::new(
                        function.source_span,
                        format!("conflicting semantic callback row {:?}", callback.id),
                    ));
                }
            } else {
                callbacks.push(callback);
            }
        }
        for file in files {
            let mut file = file;
            if file.source.is_empty() {
                file.source = program
                    .source_files
                    .get(&file.path)
                    .cloned()
                    .unwrap_or_default();
            }
            if let Some(existing) = source_files.iter().find(|row| row.id == file.id) {
                if existing.path != file.path || existing.source != file.source {
                    return Err(LowerError::new(
                        function.source_span,
                        format!("conflicting source-file row {:?}", file.id),
                    ));
                }
            } else {
                source_files.push(file);
            }
        }
    }
    ensure_artifact_source_files(
        &mut source_files,
        &program.artifact_facts,
        &program.source_files,
    );

    let links = lower_link_rows(&program.artifact_facts.links);
    let mut callbacks = merge_callback_rows(
        callbacks,
        &program.artifact_facts.callbacks,
        &program.funcs,
        &function_registry,
    );
    let handles = lower_handle_rows(
        &program.artifact_facts.handles,
        &program.funcs,
        &function_registry,
    );
    let foreign = lower_foreign_rows(
        &program.artifact_facts.foreign,
        &program.funcs,
        &function_registry,
    );
    for foreign in &foreign {
        for param in &foreign.params {
            let instance =
                canonical_type_instance(&types, &mir_type_as_ast(&param.ty), foreign.span)?;
            merge_type_instance(&mut type_instances, instance, foreign.span)?;
        }
        if let Some(return_type) = &foreign.return_type {
            let instance =
                canonical_type_instance(&types, &mir_type_as_ast(return_type), foreign.span)?;
            merge_type_instance(&mut type_instances, instance, foreign.span)?;
        }
    }
    let mut traits = lower_trait_rows(
        &program.declarations.traits,
        &program.funcs,
        &function_registry,
    );
    for trait_def in &traits {
        for method in &trait_def.methods {
            for ty in trait_method_type_rows(method) {
                let instance = canonical_type_instance(&types, &mir_type_as_ast(&ty), method.span)?;
                merge_type_instance(&mut type_instances, instance, method.span)?;
            }
        }
    }
    let mut impls = lower_impl_rows(
        &program.declarations.impls,
        &program.funcs,
        &function_registry,
        &types,
    )?;
    attach_orphan_methods(&mut impls, &functions);
    for implementation in &impls {
        merge_type_instance(
            &mut type_instances,
            implementation.self_type.clone(),
            implementation.span,
        )?;
        for associated in &implementation.associated_types {
            merge_type_instance(&mut type_instances, associated.ty.clone(), associated.span)?;
        }
        if let Some(ty) = &implementation.operator_rhs {
            merge_type_instance(&mut type_instances, ty.clone(), implementation.span)?;
        }
    }
    ensure_referenced_traits(&mut traits, &impls, &functions, &types);
    let constants = lower_constant_rows(&program.declarations.constants);
    let jobs = lower_job_rows(
        &program.artifact_facts.jobs,
        &program.funcs,
        &function_registry,
    );
    let tests = lower_test_rows(
        &program.artifact_facts.tests,
        &program.funcs,
        &function_registry,
    );
    let harnesses = lower_harness_rows(
        &program.artifact_facts.harnesses,
        &program.artifact_facts.tests,
        &tests,
        &program.funcs,
        &function_registry,
    );
    let artifacts =
        lower_artifact_rows(&program.artifact_facts, &program.funcs, &function_registry);
    let (modules, imports) = lower_module_rows(
        &program.artifact_facts.modules,
        &program.artifact_facts.imports,
        &types,
        &traits,
        &impls,
        &constants,
        &foreign,
        &links,
        &jobs,
        &program.funcs,
        &function_registry,
        &source_files,
    );

    for function in &functions {
        for ty in function_type_rows(function) {
            ensure_type_instance(&mut type_instances, ty);
        }
    }
    for definition in &types {
        for ty in type_def_types(definition) {
            ensure_type_instance(&mut type_instances, ty);
        }
    }
    for trait_def in &traits {
        for method in &trait_def.methods {
            for ty in trait_method_type_rows(method) {
                ensure_type_instance(&mut type_instances, ty);
            }
        }
    }
    for ty in &types {
        if !type_instances
            .iter()
            .any(|instance| instance.identity == Some(ty.id))
        {
            return Err(LowerError::new(
                ty.span,
                format!(
                    "missing canonical MIR type instance for `{key}` ({id:?})",
                    key = ty.key,
                    id = ty.id
                ),
            ));
        }
    }
    type_instances.sort_unstable_by_key(|ty| ty.identity);
    functions.sort_unstable_by(|left, right| {
        left.id
            .cmp(&right.id)
            .then_with(|| left.key.cmp(&right.key))
    });
    prelude_calls.sort_unstable_by_key(|call| call.id);
    callbacks.sort_unstable_by_key(|callback| callback.id);
    source_files.sort_unstable_by_key(|file| file.id);
    let core_calls = lower_core_records(program.core_calls.iter().copied());
    let mut fields: Vec<jet_foundation::MIR::MirFieldRow> = types
        .iter()
        .flat_map(|ty| type_fields(ty).into_iter())
        .collect();
    let mut seen_fields: HashSet<MirFieldId> = fields.iter().map(|row| row.id).collect();
    for instance in &type_instances {
        for row in tuple_type_fields(instance) {
            if seen_fields.insert(row.id) {
                fields.push(row);
            }
        }
    }

    Ok(MirProgram {
        schema_version: MIR_SCHEMA_VERSION,
        package_identity: program.package_identity.clone(),
        facts: lower_package_facts(program),
        cffi: lower_cffi_facts(&program.artifact_facts.cffi),
        names: lower_name_facts(&program.artifact_facts.names),
        modules,
        imports,
        types,
        traits,
        impls,
        constants,
        fields,
        source_files,
        functions,
        foreign,
        links,
        callbacks,
        handles,
        jobs,
        tests,
        harnesses,
        artifacts,
        core_calls,
        prelude_calls,
        type_instances,
        unreachable: program.unreachable.iter().map(|row| row.to_mir()).collect(),
    })
}

fn resolve_function_key(
    key: &str,
    module: &str,
    registry: &FunctionRegistry,
) -> Option<MirFunctionId> {
    registry.lookup(key, module)
}

fn resolve_function_ref(
    reference: &TirFunctionRef,
    functions: &[TFunc],
    registry: &FunctionRegistry,
) -> Option<MirFunctionId> {
    let mut exact = functions.iter().filter(|function| {
        function.source_span == reference.span
            && (function.key == reference.key
                || (function.module == reference.module && function.name == reference.name))
    });
    if let Some(function) = exact.next() {
        if exact.next().is_none() {
            return registry.id_for(function).ok();
        }
    }
    resolve_function_key(&reference.key, &reference.module, registry)
        .or_else(|| resolve_function_key(&reference.name, &reference.module, registry))
}

fn resolve_function_name_at(
    name: &str,
    module: &str,
    span: Span,
    functions: &[TFunc],
    registry: &FunctionRegistry,
) -> Option<MirFunctionId> {
    let mut exact = functions.iter().filter(|function| {
        function.source_span == span
            && (function.key == name || (function.module == module && function.name == name))
    });
    if let Some(function) = exact.next() {
        if exact.next().is_none() {
            return registry.id_for(function).ok();
        }
    }
    resolve_function_key(name, module, registry)
}

fn retarget_type_function_refs(
    types: &mut [MirTypeDef],
    definitions: &[super::tir_to_mir_types::TirTypeDef],
    registry: &FunctionRegistry,
) {
    for (ty, definition) in types.iter_mut().zip(definitions) {
        let methods = match &definition.kind {
            super::tir_to_mir_types::TirTypeDefKind::Struct { methods, .. }
            | super::tir_to_mir_types::TirTypeDefKind::Enum { methods, .. } => methods,
            _ => continue,
        };
        let resolved = methods
            .iter()
            .filter_map(|key| resolve_function_key(key, &definition.module, registry))
            .collect::<Vec<_>>();
        match &mut ty.kind {
            MirTypeDefKind::Struct { methods, .. } | MirTypeDefKind::Enum { methods, .. } => {
                *methods = resolved;
            }
            _ => {}
        }
        ty.cli_bindings.retain(|binding| {
            resolve_function_key(
                &format!("{}::{}", definition.module, binding.name),
                &definition.module,
                registry,
            )
            .is_some()
        });
    }
}

fn ensure_artifact_source_files(
    source_files: &mut Vec<MirSourceFile>,
    facts: &TirArtifactFacts,
    source_text: &BTreeMap<String, String>,
) {
    for module in &facts.modules {
        let id = MirSourceFileId(stable_id("mir-source-file", &module.source_path));
        if let Some(existing) = source_files.iter_mut().find(|file| file.id == id) {
            if existing.source.is_empty() {
                existing.source = source_text
                    .get(&module.source_path)
                    .cloned()
                    .unwrap_or_default();
            }
            continue;
        }
        source_files.push(MirSourceFile {
            id,
            path: module.source_path.clone(),
            source: source_text
                .get(&module.source_path)
                .cloned()
                .unwrap_or_default(),
        });
    }
}

fn lower_link_rows(rows: &[TirLinkUnit]) -> Vec<MirLinkUnit> {
    let known = rows
        .iter()
        .map(|row| {
            (
                row.key.clone(),
                MirLinkUnitId(stable_id("mir-link", &row.key)),
            )
        })
        .collect::<HashMap<_, _>>();
    let mut result = rows
        .iter()
        .map(|row| MirLinkUnit {
            id: MirLinkUnitId(stable_id("mir-link", &row.key)),
            crate_spec: row.crate_spec.clone(),
            cache_identity: row.cache_identity.clone(),
            target_applicability: lower_target_applicability(row.applicability),
            artifacts: Vec::new(),
            dependency_dirs: row.dependency_dirs.clone(),
            link_closure: row
                .link_closure
                .iter()
                .filter_map(|key| {
                    known
                        .get(key)
                        .copied()
                        .or_else(|| known.get(&format!("c::{key}")).copied())
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    result.sort_unstable_by_key(|row| row.id);
    result
}

fn merge_callback_rows(
    mut existing: Vec<MirCallbackAdapter>,
    rows: &[TirCallbackFact],
    functions: &[TFunc],
    registry: &FunctionRegistry,
) -> Vec<MirCallbackAdapter> {
    for row in rows {
        let Some(function) = resolve_function_ref(&row.function, functions, registry) else {
            continue;
        };
        let id = MirCallbackId(stable_id("mir-callback", &row.key));
        if existing.iter().any(|callback| callback.id == id) {
            continue;
        }
        existing.push(MirCallbackAdapter {
            id,
            symbol: row.symbol.clone(),
            function,
            params: row.params.iter().map(lower_artifact_param).collect(),
            return_type: row.return_type.as_ref().map(lower_type),
            abi: MirForeignAbi::C,
            managed: false,
            plan_digest: None,
            callback_identity: None,
        });
    }
    existing.sort_unstable_by_key(|callback| callback.id);
    existing
}

fn lower_handle_rows(
    rows: &[super::artifact_plan::TirHandleFact],
    functions: &[TFunc],
    registry: &FunctionRegistry,
) -> Vec<MirHandleLifecycle> {
    let mut result = rows
        .iter()
        .map(|row| MirHandleLifecycle {
            id: MirHandleId(stable_id("mir-handle", &row.key)),
            ty: lower_type(&Type::Named(row.jet_name.clone())),
            ownership: MirHandleOwnership::Owned,
            payload: MirHandlePayload {
                library: row.lib.clone(),
                typedef_name: row.typedef_name.clone(),
                close: row.close.clone(),
            },
            close_foreign: None,
            close_source: Some(row.close_source.clone()),
            thread_safety: Some(row.thread_safety.clone()),
            close: row
                .close_function_key
                .as_deref()
                .and_then(|key| resolve_function_key(key, "", registry))
                .or_else(|| {
                    row.close_function_key.as_deref().and_then(|key| {
                        functions
                            .iter()
                            .find(|function| function.key == key)
                            .and_then(|function| registry.id_for(function).ok())
                    })
                }),
            undo: row
                .undo_function_key
                .as_deref()
                .and_then(|key| resolve_function_key(key, "", registry))
                .or_else(|| {
                    row.undo_function_key.as_deref().and_then(|key| {
                        functions
                            .iter()
                            .find(|function| function.key == key)
                            .and_then(|function| registry.id_for(function).ok())
                    })
                }),
            send: row.send,
            sync: row.sync,
        })
        .collect::<Vec<_>>();
    result.sort_unstable_by_key(|row| row.id);
    result
}

fn lower_foreign_rows(
    rows: &[TirForeignFact],
    _functions: &[TFunc],
    registry: &FunctionRegistry,
) -> Vec<MirForeign> {
    let mut result = rows
        .iter()
        .map(|row| {
            let module_id = jet_foundation::MIR::MirModuleId(stable_id("mir-module", &row.module));
            let mut effects = MirEffectFacts::default();
            if let Some(root) = &row.effect_root {
                effects.direct.insert(root.clone());
                effects.solved.insert(root.clone());
                effects.direct_spans.insert(root.clone(), row.span);
            }
            MirForeign {
                id: jet_foundation::MIR::MirForeignId(stable_id("mir-foreign", &row.key)),
                module_id,
                key: row.key.clone(),
                module: row.module.clone(),
                name: row.name.clone(),
                span: row.span,
                symbol: row.symbol.clone(),
                path: row.path.clone(),
                params: row.params.iter().map(lower_artifact_param).collect(),
                return_type: row.return_type.as_ref().map(lower_type),
                foreign_abi: lower_foreign_abi(&row.abi),
                foreign_language: lower_foreign_language(&row.language),
                target_applicability: lower_target_applicability(row.applicability),
                effects,
                callback_transport: row.callback_transport.clone(),
                callback_plan_digest: row.callback_plan_digest.clone(),
                callback_identity: row.callback_identity.clone(),
                link: row
                    .link_key
                    .as_deref()
                    .map(|key| MirLinkUnitId(stable_id("mir-link", key))),
                callback: row
                    .callback_key
                    .as_deref()
                    .map(|key| MirCallbackId(stable_id("mir-callback", key))),
                handle: row
                    .handle_key
                    .as_deref()
                    .map(|key| MirHandleId(stable_id("mir-handle", key))),
                close_function: row
                    .close_function_key
                    .as_deref()
                    .and_then(|key| resolve_function_key(key, &row.module, registry)),
                undo_function: row
                    .undo_function_key
                    .as_deref()
                    .and_then(|key| resolve_function_key(key, &row.module, registry)),
                close_foreign: None,
            }
        })
        .collect::<Vec<_>>();
    result.sort_unstable_by_key(|row| row.id);
    result
}

fn lower_target_applicability(
    applicability: TirTargetApplicability,
) -> jet_foundation::MIR::MirTargetApplicability {
    applicability
}

fn lower_foreign_abi(abi: &str) -> MirForeignAbi {
    match abi.to_ascii_lowercase().as_str() {
        "c" => MirForeignAbi::C,
        "c-unwind" | "cunwind" => MirForeignAbi::CUnwind,
        "system" => MirForeignAbi::System,
        "stdcall" => MirForeignAbi::Stdcall,
        "fastcall" => MirForeignAbi::Fastcall,
        "vectorcall" => MirForeignAbi::Vectorcall,
        "rust" => MirForeignAbi::Rust,
        _ => MirForeignAbi::Platform(abi.to_string()),
    }
}

fn lower_foreign_language(language: &str) -> MirForeignLanguage {
    match language
        .split(':')
        .next()
        .unwrap_or(language)
        .to_ascii_lowercase()
        .as_str()
    {
        "c" => MirForeignLanguage::C,
        "cpp" | "c++" => MirForeignLanguage::Cpp,
        "asm" | "assembly" => MirForeignLanguage::Assembly,
        _ => MirForeignLanguage::Rust,
    }
}

fn lower_artifact_param(param: &TirParamFact) -> MirParam {
    let access = match param.access {
        super::artifact_plan::TirAccess::Read => MirAccess::Read,
        super::artifact_plan::TirAccess::Write => MirAccess::Write,
        super::artifact_plan::TirAccess::Move => MirAccess::Move,
    };
    let ownership = MirOwnership::from_access(access);
    MirParam {
        index: param.index,
        name: param.name.clone(),
        span: param.span,
        ty: lower_type(&param.ty),
        access,
        ownership,
        public_label: param.label.clone(),
        variadic: false,
        default_present: false,
    }
}
fn lower_failure_carrier_types(carrier: &TFailureCarrier) -> MirFailureCarrier {
    match carrier {
        TFailureCarrier::Infallible => MirFailureCarrier::Infallible,
        TFailureCarrier::Result { success, error } => MirFailureCarrier::Result {
            success: lower_type(success),
            error: lower_type(error),
        },
        TFailureCarrier::Optional { value } => MirFailureCarrier::Optional {
            value: lower_type(value),
        },
        TFailureCarrier::Diverges { value } => MirFailureCarrier::Diverges {
            value: lower_type(value),
        },
    }
}

fn lower_effects(declared: Option<&Vec<(String, Span)>>) -> MirEffectFacts {
    let mut effects = MirEffectFacts::default();
    if let Some(declared) = declared {
        for (effect, span) in declared {
            effects.direct.insert(effect.clone());
            effects.solved.insert(effect.clone());
            effects.direct_spans.insert(effect.clone(), *span);
        }
    }
    effects
}

fn lower_trait_rows(
    rows: &[super::tir_to_mir_types::TirTraitDef],
    functions: &[TFunc],
    registry: &FunctionRegistry,
) -> Vec<MirTraitDef> {
    let mut result = rows
        .iter()
        .map(|row| MirTraitDef {
            id: MirTraitId(stable_id("mir-trait", &row.key)),
            module: jet_foundation::MIR::MirModuleId(stable_id("mir-module", &row.module)),
            key: row.key.clone(),
            name: row.name.clone(),
            span: row.span,
            visibility: lower_decl_visibility(row.visibility),
            associated_types: row
                .associated_types
                .iter()
                .map(|associated| MirAssociatedTypeDecl {
                    name: associated.name.clone(),
                    span: associated.span,
                })
                .collect(),
            methods: row
                .methods
                .iter()
                .map(|method| MirTraitMethod {
                    id: MirTraitMethodId(stable_id(
                        "mir-trait-method",
                        &format!("{}::{}", row.key, method.key),
                    )),
                    name: method.name.clone(),
                    span: method.span,
                    self_access: method.self_access.map(super::tir_to_mir_types::mir_access),
                    params: method.params.iter().map(TirParam::to_mir).collect(),
                    declared_return: method.declared_return.as_ref().map(lower_type),
                    return_type: lower_type(&method.return_type),
                    failure: lower_failure_carrier_types(&method.failure),
                    effects: lower_effects(method.declared_effects.as_ref()),
                    is_pure: method.is_pure,
                    return_view_provenance: method
                        .return_view_provenance
                        .as_ref()
                        .map(lower_view_provenance),
                    declared_return_view_provenance: None,
                    default: method.default_function_key.as_deref().and_then(|key| {
                        resolve_function_name_at(key, &row.module, method.span, functions, registry)
                            .or_else(|| resolve_function_key(key, &row.module, registry))
                    }),
                })
                .collect(),
        })
        .collect::<Vec<_>>();
    result.sort_unstable_by_key(|row| row.id);
    result
}

/// Compiler-owned traits (Display, Equatable, Encode, …) arrive as impls and
/// trait methods without an `Item::Trait`. Canonical MIR still requires every
/// `MirTraitId` referenced by a function or impl to exist in the traits table.
fn ensure_referenced_traits(
    traits: &mut Vec<MirTraitDef>,
    impls: &[MirImplDef],
    functions: &[MirFunction],
    types: &[MirTypeDef],
) {
    let mut known: HashSet<MirTraitId> = traits.iter().map(|row| row.id).collect();
    let mut needed: Vec<MirTraitRef> = Vec::new();
    let take = |trait_ref: &MirTraitRef,
                known: &mut HashSet<MirTraitId>,
                needed: &mut Vec<MirTraitRef>| {
        if known.insert(trait_ref.id) {
            needed.push(trait_ref.clone());
        }
    };
    for implementation in impls {
        if let Some(trait_ref) = &implementation.trait_ref {
            take(trait_ref, &mut known, &mut needed);
        }
    }
    for function in functions {
        match &function.form {
            MirFunctionForm::TraitMethod { trait_ref, .. } => {
                take(trait_ref, &mut known, &mut needed);
            }
            MirFunctionForm::TopLevel | MirFunctionForm::Method { .. } => {}
        }
        for generic in &function.generic_params {
            for bound in &generic.bounds {
                take(bound, &mut known, &mut needed);
            }
        }
    }
    for ty in types {
        for derive in &ty.derives {
            if known.insert(*derive) {
                needed.push(MirTraitRef {
                    id: *derive,
                    name: format!("derive#{}", derive.0),
                });
            }
        }
        for generic in &ty.generic_params {
            for bound in &generic.bounds {
                take(bound, &mut known, &mut needed);
            }
        }
    }
    if needed.is_empty() {
        return;
    }
    let module = traits
        .first()
        .map(|row| row.module)
        .or_else(|| impls.first().map(|row| row.module))
        .or_else(|| functions.first().map(|row| row.module_id))
        .unwrap_or_else(|| jet_foundation::MIR::MirModuleId(stable_id("mir-module", "compiler")));
    for trait_ref in needed {
        traits.push(MirTraitDef {
            id: trait_ref.id,
            module,
            key: trait_ref.name.clone(),
            name: trait_ref.name.clone(),
            span: Span::new(0, 0),
            visibility: MirVisibility::Public,
            associated_types: Vec::new(),
            methods: Vec::new(),
        });
    }
    traits.sort_unstable_by_key(|row| row.id);
}

fn tfunc_method_owner(function: &TFunc) -> Option<(&Type, Option<&str>)> {
    match &function.kind {
        TFuncKind::Method { owner_type, .. } => Some((owner_type, None)),
        TFuncKind::TraitMethod {
            owner_type,
            trait_name,
            ..
        } => Some((owner_type, Some(trait_name.as_str()))),
        TFuncKind::TopLevel => None,
    }
}

fn nominal_owner_name(ty: &Type) -> String {
    match ty {
        Type::Named(name) | Type::Apply { name, .. } => name.clone(),
        _ => ty.name(),
    }
}

fn method_base_leaf(name: &str) -> &str {
    let leaf = method_leaf(name);
    let leaf = leaf.split("__generic__").next().unwrap_or(leaf);
    leaf.split('<').next().unwrap_or(leaf)
}

fn impl_method_matches(
    function: &TFunc,
    row: &super::tir_to_mir_types::TirImplDef,
    method_key: &str,
) -> bool {
    let Some((owner, function_trait)) = tfunc_method_owner(function) else {
        return false;
    };
    if function.module != row.module
        || method_base_leaf(method_key) != method_base_leaf(&function.name)
    {
        return false;
    }
    let row_trait = row.trait_ref.as_ref().or(row.trait_name.as_ref());
    if function_trait != row_trait.map(String::as_str) {
        return false;
    }
    let expected_owner = canonical_target_type(&row.module, &nominal_owner_name(&row.self_type));
    let actual_owner = canonical_target_type(&function.module, &nominal_owner_name(owner));
    if expected_owner != actual_owner {
        return false;
    }
    if let Some(expected_rhs) = &row.operator_rhs {
        let Some(actual_rhs) = function_operator_rhs(function) else {
            return false;
        };
        if canonical_target_type(&row.module, &expected_rhs.name())
            != canonical_target_type(&function.module, actual_rhs)
        {
            return false;
        }
    }
    true
}

fn lower_impl_rows(
    rows: &[super::tir_to_mir_types::TirImplDef],
    functions: &[TFunc],
    registry: &FunctionRegistry,
    types: &[MirTypeDef],
) -> Result<Vec<MirImplDef>, LowerError> {
    let mut result = Vec::new();
    for row in rows {
        let declared_self_type = canonical_type_instance(types, &row.self_type, row.span)?;
        let trait_name = row.trait_ref.as_ref().or(row.trait_name.as_ref());
        let mut method_groups: Vec<(MirType, Vec<MirFunctionId>)> = Vec::new();
        for method_key in &row.methods {
            let mut candidates = Vec::new();
            if let Some(id) = resolve_function_key(method_key, &row.module, registry) {
                if let Some(function) = functions.iter().find(|function| {
                    MirFunctionId(stable_id("mir-function", &function_identity(function))) == id
                }) {
                    candidates.push((id, function));
                }
            }
            for function in functions
                .iter()
                .filter(|function| impl_method_matches(function, row, method_key))
            {
                let id = MirFunctionId(stable_id("mir-function", &function_identity(function)));
                if !candidates
                    .iter()
                    .any(|(candidate, _)| *candidate == id)
                {
                    candidates.push((id, function));
                }
            }
            for (id, function) in candidates {
                let Some((owner, _)) = tfunc_method_owner(function) else {
                    continue;
                };
                let owner = canonical_type_instance(types, owner, row.span)?;
                if let Some((_group_owner, group_methods)) = method_groups
                    .iter_mut()
                    .find(|(group_owner, _)| same_impl_owner(group_owner, &owner))
                {
                    if !group_methods.contains(&id) {
                        group_methods.push(id);
                    }
                } else {
                    method_groups.push((owner, vec![id]));
                }
            }
        }
        if method_groups.is_empty() {
            method_groups.push((
                declared_self_type,
                row.methods
                    .iter()
                    .filter_map(|key| resolve_function_key(key, &row.module, registry))
                    .collect(),
            ));
        }
        for (self_type, methods) in method_groups {
            let key = canonical_impl_key(&row.module, &self_type, trait_name.map(String::as_str));
            result.push(MirImplDef {
                id: MirImplId(stable_id("mir-impl", &key)),
                module: jet_foundation::MIR::MirModuleId(stable_id("mir-module", &row.module)),
                key,
                span: row.span,
                self_type,
                trait_ref: trait_name.map(|name| MirTraitRef {
                    id: MirTraitId(stable_id("mir-trait", name)),
                    name: name.clone(),
                }),
                associated_types: row
                    .associated_types
                    .iter()
                    .map(|associated| {
                        Ok(MirAssociatedTypeValue {
                            name: associated.name.clone(),
                            ty: canonical_type_instance(types, &associated.ty, associated.span)?,
                            span: associated.span,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                methods,
                delegation: row
                    .delegation_field
                    .as_deref()
                    .and_then(|name| resolve_field_id(types, name)),
                compiler_generated: row.compiler_generated,
                serde: row.serde.map(|serde| match serde {
                    super::tir_to_mir_types::TirSerdeCodec::Encode => MirSerdeCodec::Encode,
                    super::tir_to_mir_types::TirSerdeCodec::Decode => MirSerdeCodec::Decode,
                }),
                operator_rhs: row
                    .operator_rhs
                    .as_ref()
                    .map(|ty| canonical_type_instance(types, ty, row.span))
                    .transpose()?,
                operator_marker: row.operator_marker.clone(),
                target_os: row.target_os.clone(),
                target_applicability: MirTargetApplicability {
                    rust_aot: true,
                    cranelift: true,
                    interpreter: true,
                    web: true,
                },
            });
        }
    }
    // Keep only checked method references; a method that did not enter the
    // executable TIR cannot be represented as a valid MIR edge.
    result.retain(|row| {
        row.methods.iter().all(|id| {
            functions.iter().any(|function| {
                function_identity(function)
                    .as_bytes()
                    .iter()
                    .fold(0u64, |acc, byte| acc.wrapping_add(*byte as u64))
                    != 0
                    && MirFunctionId(stable_id("mir-function", &function_identity(function))) == *id
            })
        })
    });
    result.sort_unstable_by_key(|row| row.id);
    Ok(result)
}

fn canonical_impl_key(module: &str, owner: &MirType, trait_name: Option<&str>) -> String {
    let owner = canonical_target_type(module, &owner.display_name());
    let owner = if module.is_empty() {
        owner
    } else {
        owner
            .strip_prefix(&format!("{module}::"))
            .unwrap_or(&owner)
            .to_string()
    };
    match trait_name {
        Some(trait_name) if !module.is_empty() => format!("{module}::{owner}::{trait_name}"),
        Some(trait_name) => format!("{owner}::{trait_name}"),
        None if !module.is_empty() => format!("{module}::{owner}"),
        None => owner,
    }
}


fn same_impl_owner(row: &MirType, owner: &MirType) -> bool {
    row.same_checked_type(owner)
}

fn find_impl_index(
    impls: &[MirImplDef],
    owner: &MirType,
    trait_ref: Option<&MirTraitRef>,
    module: jet_foundation::MIR::MirModuleId,
) -> Option<usize> {
    impls.iter().enumerate().find_map(|(index, row)| {
        let trait_ok = match (row.trait_ref.as_ref(), trait_ref) {
            (None, None) => true,
            (Some(existing), Some(wanted)) => {
                existing.id == wanted.id && existing.name == wanted.name
            }
            _ => false,
        };
        (row.module == module && trait_ok && same_impl_owner(&row.self_type, owner))
            .then_some(index)
    })
}

/// Inherent methods live on the type def, and nested derive methods are TFuncs,
/// but AOT only emits `MirFunctionForm::Method` / `TraitMethod` from an impl
/// row. Attach any function the declaration snapshot failed to name.
fn attach_orphan_methods(impls: &mut Vec<MirImplDef>, functions: &[MirFunction]) {
    let mut owned: HashSet<MirFunctionId> = impls
        .iter()
        .flat_map(|row| row.methods.iter().copied())
        .collect();
    for function in functions {
        if !owned.insert(function.id) {
            continue;
        }
        let (owner, trait_ref) = match &function.form {
            MirFunctionForm::TopLevel => continue,
            MirFunctionForm::Method { owner, .. } => (owner, None),
            MirFunctionForm::TraitMethod {
                owner,
                trait_ref,
                serde: Some(_),
                ..
            } => {
                let _ = (owner, trait_ref);
                continue;
            }
            MirFunctionForm::TraitMethod {
                owner, trait_ref, ..
            } => (owner, Some(trait_ref)),
        };
        if let Some(index) = find_impl_index(impls, owner, trait_ref, function.module_id) {
            impls[index].methods.push(function.id);
            continue;
        }
        let key = canonical_impl_key(
            &function.module,
            owner,
            trait_ref.map(|trait_ref| trait_ref.name.as_str()),
        );
        impls.push(MirImplDef {
            id: MirImplId(stable_id("mir-impl", &key)),
            module: function.module_id,
            key,
            span: function.span,
            self_type: owner.clone(),
            trait_ref: trait_ref.cloned(),
            associated_types: Vec::new(),
            methods: vec![function.id],
            delegation: None,
            compiler_generated: false,
            serde: None,
            operator_rhs: None,
            operator_marker: None,
            target_os: None,
            target_applicability: MirTargetApplicability {
                rust_aot: true,
                cranelift: true,
                interpreter: true,
                web: true,
            },
        });
    }
    impls.sort_unstable_by_key(|row| row.id);
}

fn resolve_field_id(types: &[MirTypeDef], name: &str) -> Option<MirFieldId> {
    types.iter().find_map(|ty| match &ty.kind {
        MirTypeDefKind::Struct { fields, .. } => fields
            .iter()
            .find(|field| field.name == name)
            .map(|field| field.id),
        MirTypeDefKind::Enum { variants, .. } => {
            variants.iter().find_map(|variant| match &variant.payload {
                MirVariantPayload::Named(fields) => fields
                    .iter()
                    .find(|field| field.name == name)
                    .map(|field| field.id),
                _ => None,
            })
        }
        _ => None,
    })
}

fn lower_decl_visibility(visibility: super::tir_to_mir_types::TirVisibility) -> MirVisibility {
    match visibility {
        super::tir_to_mir_types::TirVisibility::Private => MirVisibility::Private,
        super::tir_to_mir_types::TirVisibility::Package => MirVisibility::Package,
        super::tir_to_mir_types::TirVisibility::Public => MirVisibility::Public,
    }
}

fn lower_constant_rows(rows: &[super::tir_to_mir_types::TirConstantDef]) -> Vec<MirConstantDef> {
    let mut result = rows
        .iter()
        .filter_map(|row| {
            Some(MirConstantDef {
                id: jet_foundation::MIR::MirConstantId(stable_id("mir-constant", &row.key)),
                module: jet_foundation::MIR::MirModuleId(stable_id("mir-module", &row.module)),
                key: row.key.clone(),
                name: row.name.clone(),
                span: row.span,
                visibility: lower_decl_visibility(row.visibility),
                ty: lower_type(&row.ty),
                value: lower_constant_value(&row.value)?,
            })
        })
        .collect::<Vec<_>>();
    result.sort_unstable_by_key(|row| row.id);
    result
}

fn lower_constant_value(value: &CtValue) -> Option<MirConstant> {
    Some(match value {
        CtValue::Int(value) => MirConstant::Int {
            value: *value,
            width: None,
            spelling: None,
        },
        CtValue::Float(value) => MirConstant::Float {
            value: value.as_f64(),
            f32: matches!(value, jet_foundation::AST::CtFloat::F32(_)),
            spelling: None,
        },
        CtValue::Bool(value) => MirConstant::Bool(*value),
        CtValue::Char(value) => MirConstant::Char(*value),
        CtValue::Str(value) => MirConstant::String(value.clone()),
        CtValue::BigInt(value) => MirConstant::BigInt(value.to_string_rep()),
        CtValue::Bytes(value) => MirConstant::Bytes(value.clone()),
        CtValue::List(values) => MirConstant::List(
            values
                .iter()
                .map(lower_constant_value)
                .collect::<Option<Vec<_>>>()?,
        ),
        CtValue::Map(values) => MirConstant::Map(
            values
                .iter()
                .map(|(key, value)| Some((lower_const_key(key)?, lower_constant_value(value)?)))
                .collect::<Option<BTreeMap<_, _>>>()?,
        ),
        CtValue::Struct { type_name, fields } => MirConstant::Struct {
            type_name: type_name.clone(),
            fields: fields
                .iter()
                .map(|(name, value)| Some((name.clone(), lower_constant_value(value)?)))
                .collect::<Option<Vec<_>>>()?,
        },
        CtValue::Enum {
            type_name,
            variant,
            args,
        } => MirConstant::Enum {
            type_name: type_name.clone(),
            variant: variant.clone(),
            args: args
                .iter()
                .map(|(name, value)| Some((name.clone(), lower_constant_value(value)?)))
                .collect::<Option<Vec<_>>>()?,
        },
        CtValue::Present(value) => MirConstant::Present(Box::new(lower_constant_value(value)?)),
        CtValue::Failed(report) => MirConstant::Failed(match report {
            CtReport::Clean(ty) => jet_foundation::MIR::MirConstReport::Clean(lower_type(ty)),
            CtReport::Told(value) => {
                jet_foundation::MIR::MirConstReport::Told(Box::new(lower_constant_value(value)?))
            }
        }),
        CtValue::Unit => MirConstant::Unit,
        CtValue::Closure(_) => return None,
    })
}

fn lower_const_key(key: &CtKey) -> Option<jet_foundation::MIR::MirConstKey> {
    Some(match key {
        CtKey::Int(value) => jet_foundation::MIR::MirConstKey::Int(*value),
        CtKey::Str(value) => jet_foundation::MIR::MirConstKey::String(value.clone()),
        CtKey::Bool(value) => jet_foundation::MIR::MirConstKey::Bool(*value),
        CtKey::Char(value) => jet_foundation::MIR::MirConstKey::Char(*value),
        CtKey::Tuple(fields) => jet_foundation::MIR::MirConstKey::Tuple(
            fields
                .iter()
                .map(|(name, key)| Some((name.clone(), lower_const_key(key)?)))
                .collect::<Option<Vec<_>>>()?,
        ),
        CtKey::Struct { type_name, fields } => jet_foundation::MIR::MirConstKey::Struct {
            type_name: type_name.clone(),
            fields: fields
                .iter()
                .map(|(name, key)| Some((name.clone(), lower_const_key(key)?)))
                .collect::<Option<Vec<_>>>()?,
        },
        CtKey::Enum { type_name, variant } => jet_foundation::MIR::MirConstKey::Enum {
            type_name: type_name.clone(),
            variant: variant.clone(),
        },
    })
}
fn lower_job_rows(
    rows: &[TirJobFact],
    functions: &[TFunc],
    registry: &FunctionRegistry,
) -> Vec<MirJob> {
    let mut result = rows
        .iter()
        .filter_map(|row| {
            let function = row
                .function
                .as_ref()
                .and_then(|reference| resolve_function_ref(reference, functions, registry))?;
            Some(MirJob {
                id: jet_foundation::MIR::MirJobId(stable_id("mir-job", &row.key)),
                function,
                name: row.name.clone(),
                doc: row.doc.clone(),
                after: row.after.clone(),
                parallel: row.parallel,
                scope: match row.scope {
                    crate::AST::JobScope::Dev => MirJobScope::Dev,
                    crate::AST::JobScope::Ship => MirJobScope::Ship,
                    crate::AST::JobScope::Internal => MirJobScope::Internal,
                },
                schedule: row.schedule.map(|schedule| match schedule {
                    crate::AST::EverySchedule::Duration { nanos } => {
                        MirJobSchedule::Duration { nanos }
                    }
                    crate::AST::EverySchedule::WallClockTime { hour, minute } => {
                        MirJobSchedule::WallClockTime { hour, minute }
                    }
                }),
                inputs: row
                    .arguments
                    .iter()
                    .enumerate()
                    .map(|(index, argument)| lower_job_argument(argument, index))
                    .collect(),
                dispatch: if row.schedule.is_some() {
                    MirJobDispatch::Scheduled
                } else {
                    MirJobDispatch::Direct
                },
                packages: row.packages.clone(),
                working_directory: row.working_directory.clone(),
                input_paths: row.input_paths.clone(),
                output_paths: row.output_paths.clone(),
                skip: row.skip.as_ref().map(|skip| match skip {
                    crate::AST::JobSkip::Always(reason) => MirJobSkip::Always(reason.clone()),
                    crate::AST::JobSkip::UnlessPlatform { platform } => {
                        MirJobSkip::UnlessPlatform(platform.clone())
                    }
                }),
                cache: match row.cache {
                    crate::AST::JobCachePolicy::Uncached => MirJobCachePolicy::Uncached,
                    crate::AST::JobCachePolicy::Local => MirJobCachePolicy::Local,
                    crate::AST::JobCachePolicy::Shared => MirJobCachePolicy::Shared,
                },
                limits: row.limits.clone(),
            })
        })
        .collect::<Vec<_>>();
    result.sort_unstable_by_key(|row| row.id);
    result
}

fn lower_job_argument(argument: &TirJobArgument, index: usize) -> MirCliInput {
    let kind = cli_value_kind_from_name(&argument.ty);
    let default = argument
        .default
        .as_ref()
        .map(|value| MirCliDefault::Value(MirConstant::String(value.clone())));
    MirCliInput {
        parameter: index,
        name: argument.name.clone(),
        label: argument.label.clone(),
        ty: lower_type(&Type::Named(argument.ty.clone())),
        zone: lower_param_zone(argument.zone),
        short: None,
        env: None,
        help: String::new(),
        metavar: None,
        shape: MirCliInputShape::Value {
            kind,
            optional: !argument.required,
            default,
        },
        positional: argument.required.then_some(index as u16),
        variadic: argument.variadic,
        flag: String::new(),
    }
}

fn lower_test_rows(
    rows: &[TirTestFact],
    functions: &[TFunc],
    registry: &FunctionRegistry,
) -> Vec<MirTestCase> {
    let mut result = rows
        .iter()
        .filter_map(|row| {
            let function = resolve_function_ref(&row.function, functions, registry)?;
            Some(MirTestCase {
                id: jet_foundation::MIR::MirTestId(stable_id("mir-test", &row.key)),
                function,
                name: row.name.clone(),
                span: row.span,
                kind: match row.kind {
                    TirTestKind::Unit => MirTestKind::Unit,
                    TirTestKind::Property => MirTestKind::Property,
                },
                parameters: row.parameters.iter().map(lower_artifact_param).collect(),
                faults: row.faults.clone(),
                expected_failure: row.expected_failure,
                contract_generated: row.contract_generated,
                eligibility: row
                    .eligibility
                    .as_ref()
                    .and_then(|reference| resolve_function_ref(reference, functions, registry)),
                generation_unavailable_reason: row.generation_unavailable_reason.clone(),
            })
        })
        .collect::<Vec<_>>();
    result.sort_unstable_by_key(|row| row.id);
    result
}

fn function_block_id(
    reference: &TirFunctionRef,
    block: Option<&str>,
    functions: &[TFunc],
) -> Option<(MirFunctionId, MirBlockId)> {
    let function = functions.iter().find(|function| {
        function.source_span == reference.span
            && (function.key == reference.key
                || (function.module == reference.module && function.name == reference.name))
    })?;
    let function_id = MirFunctionId(stable_id("mir-function", &function_identity(function)));
    let role = block.unwrap_or("entry");
    let identity = construct_identity(function, "block", reference.span, role, "");
    Some((function_id, MirBlockId(stable_id("mir-block", &identity))))
}

fn lower_harness_rows(
    rows: &[TirHarnessPlan],
    test_rows: &[TirTestFact],
    tests: &[MirTestCase],
    functions: &[TFunc],
    registry: &FunctionRegistry,
) -> Vec<MirHarnessPlan> {
    let test_ids = test_rows
        .iter()
        .filter(|row| {
            let id = MirTestId(stable_id("mir-test", &row.key));
            tests.iter().any(|test| test.id == id)
        })
        .map(|row| (row.key.clone(), MirTestId(stable_id("mir-test", &row.key))))
        .collect::<HashMap<_, _>>();
    let mut result = rows
        .iter()
        .map(|row| {
            let output_checks = row
                .output_checks
                .iter()
                .filter_map(|check| {
                    let function = resolve_function_ref(&check.function, functions, registry)?;
                    Some(MirOutputCheck {
                        id: jet_foundation::MIR::MirOutputCheckId(stable_id(
                            "mir-output-check",
                            &check.key,
                        )),
                        name: check.name.clone(),
                        function,
                    })
                })
                .collect::<Vec<_>>();
            let coverage_points = row
                .coverage_points
                .iter()
                .filter_map(|point| {
                    let (function, block) =
                        function_block_id(&point.function, point.block.as_deref(), functions)?;
                    Some(MirCoveragePoint {
                        id: jet_foundation::MIR::MirCoveragePointId(stable_id(
                            "mir-coverage",
                            &point.key,
                        )),
                        function,
                        block,
                        span: point.span,
                    })
                })
                .collect::<Vec<_>>();
            MirHarnessPlan {
                id: jet_foundation::MIR::MirHarnessId(stable_id("mir-harness", &row.key)),
                kind: match row.kind {
                    TirHarnessKind::Test => MirHarnessKind::Test,
                    TirHarnessKind::Fuzz => MirHarnessKind::Fuzz,
                    TirHarnessKind::Coverage => MirHarnessKind::Coverage,
                },
                tests: row
                    .tests
                    .iter()
                    .filter_map(|key| test_ids.get(key).copied())
                    .collect(),
                output_checks,
                selected_test: row
                    .selected_test
                    .as_ref()
                    .and_then(|key| test_ids.get(key).copied()),
                coverage_points,
                command_override: row.command_override,
            }
        })
        .filter(|row| {
            !row.tests.is_empty()
                || !row.output_checks.is_empty()
                || !row.coverage_points.is_empty()
        })
        .collect::<Vec<_>>();
    result.sort_unstable_by_key(|row| row.id);
    result
}

fn cli_value_kind(kind: crate::CLISchema::CLIValueKind) -> MirCliValueKind {
    match kind {
        crate::CLISchema::CLIValueKind::Bool => MirCliValueKind::Bool,
        crate::CLISchema::CLIValueKind::Int => MirCliValueKind::Int,
        crate::CLISchema::CLIValueKind::Float => MirCliValueKind::Float,
        crate::CLISchema::CLIValueKind::String => MirCliValueKind::String,
        crate::CLISchema::CLIValueKind::Path => MirCliValueKind::Path,
    }
}

fn cli_value_kind_from_name(name: &str) -> MirCliValueKind {
    match name {
        "Bool" | "bool" => MirCliValueKind::Bool,
        "Int" | "int" => MirCliValueKind::Int,
        "Float" | "float" | "Float32" | "float32" => MirCliValueKind::Float,
        "Path" | "path" => MirCliValueKind::Path,
        _ => MirCliValueKind::String,
    }
}

fn lower_cli_default(default: &TirCliDefault) -> Option<MirCliDefault> {
    Some(match default {
        TirCliDefault::TypeDefault => MirCliDefault::TypeDefault,
        TirCliDefault::Value(value) => MirCliDefault::Value(lower_constant_value(value)?),
        // The checked CLI schema records this form as a rendered value. Keep
        // the value in MIR rather than dropping the default edge.
        TirCliDefault::Recorded(value) => MirCliDefault::Value(MirConstant::String(value.clone())),
    })
}

pub(super) fn lower_cli_input(
    input: &TirCliInput,
    ordinal: usize,
    parameter: Option<&TirParamFact>,
) -> MirCliInput {
    let (kind, optional, mir_default) = match &input.shape {
        TirCliInputShape::Flag => (MirCliValueKind::Bool, false, None),
        TirCliInputShape::Value {
            kind,
            optional,
            default,
        } => (
            cli_value_kind(*kind),
            *optional,
            default.as_ref().and_then(lower_cli_default),
        ),
    };
    MirCliInput {
        parameter: parameter.map_or(ordinal, |param| param.index),
        name: input.field.clone(),
        label: parameter.map_or_else(|| input.field.clone(), |param| param.label.clone()),
        ty: parameter.map_or_else(
            || {
                let scalar = match kind {
                    MirCliValueKind::Bool => Type::Bool,
                    MirCliValueKind::Int => Type::Int,
                    MirCliValueKind::Float => Type::Float,
                    MirCliValueKind::String | MirCliValueKind::Path => Type::String,
                };
                lower_type(&if input.variadic {
                    Type::List(Box::new(scalar))
                } else if optional {
                    Type::Option(Box::new(scalar))
                } else {
                    scalar
                })
            },
            |param| lower_type(&param.ty),
        ),
        zone: parameter.map_or(jet_foundation::MIR::MirParamZone::Either, |param| {
            lower_param_zone(param.zone)
        }),
        short: input.short.clone(),
        env: input.env.clone(),
        help: input.help.clone(),
        metavar: input.metavar.clone(),
        shape: match &input.shape {
            TirCliInputShape::Flag => MirCliInputShape::Flag,
            TirCliInputShape::Value { .. } => MirCliInputShape::Value {
                kind,
                optional,
                default: mir_default,
            },
        },
        positional: input.positional,
        variadic: input.variadic,
        flag: input.flag.clone(),
    }
}

fn cli_parameter<'a>(
    input: &TirCliInput,
    ordinal: usize,
    function: Option<&TirFunctionRef>,
    facts: &'a TirArtifactFacts,
) -> Option<&'a TirParamFact> {
    let function = function?;
    let function = facts.functions.iter().find(|candidate| {
        candidate.reference.key == function.key && candidate.reference.span == function.span
    })?;
    function
        .params
        .iter()
        .find(|param| param.name == input.field || param.label == input.field)
        .or_else(|| function.params.get(ordinal))
}

fn lower_cli_entry(
    entry: &TirCliEntry,
    function: Option<&TirFunctionRef>,
    facts: &TirArtifactFacts,
    functions: &[TFunc],
    registry: &FunctionRegistry,
) -> MirCliEntry {
    let inputs = entry
        .inputs
        .iter()
        .enumerate()
        .map(|(ordinal, input)| {
            lower_cli_input(
                input,
                ordinal,
                if entry.record_inputs {
                    None
                } else {
                    cli_parameter(input, ordinal, function, facts)
                },
            )
        })
        .collect();
    let commands = entry
        .commands
        .iter()
        .filter_map(|command| {
            let function = command
                .function
                .as_ref()
                .and_then(|reference| resolve_function_ref(reference, functions, registry))?;
            Some(MirCliCommand {
                name: command.name.clone(),
                description: command.description.clone(),
                function,
                receiver: command
                    .receiver
                    .as_ref()
                    .map(|receiver| MirTypeId(stable_id("mir-type", receiver))),
                inputs: command
                    .inputs
                    .iter()
                    .enumerate()
                    .map(|(ordinal, input)| {
                        lower_cli_input(
                            input,
                            ordinal,
                            cli_parameter(input, ordinal, command.function.as_ref(), facts),
                        )
                    })
                    .collect(),
            })
        })
        .collect();
    MirCliEntry {
        description: entry.description.clone(),
        inputs,
        commands,
        standard: entry.standard,
        version: entry.version.clone(),
    }
}

fn lower_entry_spec(
    spec: &TirEntrySpec,
    facts: &TirArtifactFacts,
    functions: &[TFunc],
    registry: &FunctionRegistry,
) -> MirEntrySpec {
    MirEntrySpec {
        kind: match spec.kind {
            super::artifact_plan::TirArtifactEntryKind::Library => MirEntryKind::Library,
            super::artifact_plan::TirArtifactEntryKind::Command => MirEntryKind::Command,
            super::artifact_plan::TirArtifactEntryKind::App => MirEntryKind::App,
            super::artifact_plan::TirArtifactEntryKind::Service => MirEntryKind::Service,
            super::artifact_plan::TirArtifactEntryKind::Test => MirEntryKind::Test,
        },
        function: spec
            .function
            .as_ref()
            .and_then(|reference| resolve_function_ref(reference, functions, registry)),
        cli: spec.cli.as_ref().map(|entry| {
            lower_cli_entry(entry, spec.function.as_ref(), facts, functions, registry)
        }),
        output: match spec.output {
            TirEntryOutput::None => MirEntryOutput::None,
            TirEntryOutput::ReturnValue => MirEntryOutput::ReturnValue,
        },
        initialize_environment: spec.initialize_environment,
        initialize_gc: spec.initialize_gc,
        serves_until_stopped: spec.serves_until_stopped,
        package_version: spec.package_version.clone(),
    }
}

fn lower_artifact_rows(
    facts: &TirArtifactFacts,
    functions: &[TFunc],
    registry: &FunctionRegistry,
) -> Vec<MirArtifactPlan> {
    let mut result = facts
        .artifacts
        .iter()
        .map(|row| MirArtifactPlan {
            id: MirArtifactId(stable_id("mir-artifact", &row.key)),
            kind: row.kind,
            name: row.name.clone(),
            target: row.target,
            mode: facts.build_mode,
            modules: row
                .modules
                .iter()
                .map(|key| jet_foundation::MIR::MirModuleId(stable_id("mir-module", key)))
                .collect(),
            links: row
                .links
                .iter()
                .map(|key| MirLinkUnitId(stable_id("mir-link", key)))
                .collect(),
            jobs: row
                .jobs
                .iter()
                .map(|key| MirJobId(stable_id("mir-job", key)))
                .collect(),
            runtime_parts: {
                let mut parts = crate::Codegen::runtime_parts_for_used_core(&row.runtime_parts);
                for key in &row.runtime_parts {
                    if let Some(part) = parse_runtime_part(key) {
                        parts.insert(part);
                    }
                }
                parts
            },
            exports: row
                .exports
                .iter()
                .filter_map(|export| {
                    Some(jet_foundation::MIR::MirExport {
                        symbol: export.symbol.clone(),
                        function: resolve_function_ref(&export.function, functions, registry)?,
                        abi: match export.abi {
                            super::artifact_plan::TirExportAbi::C => MirForeignAbi::C,
                        },
                    })
                })
                .collect(),
            provider_identity: row.provider_identity.clone(),
            closure_identity: row.closure_identity.clone(),
            artifact_identity: row.artifact_identity.clone(),
            entry: row
                .entry
                .as_ref()
                .or(facts.entry.as_ref())
                .map(|entry| lower_entry_spec(entry, facts, functions, registry)),
            harness: row
                .harness
                .as_deref()
                .map(|key| jet_foundation::MIR::MirHarnessId(stable_id("mir-harness", key))),
        })
        .collect::<Vec<_>>();
    result.sort_unstable_by_key(|row| row.id);
    result
}

fn lower_module_item(
    item: &TirItemRef,
    types: &[MirTypeDef],
    traits: &[MirTraitDef],
    impls: &[MirImplDef],
    constants: &[MirConstantDef],
    foreign: &[MirForeign],
    functions: &[TFunc],
    registry: &FunctionRegistry,
    module: &str,
) -> Option<jet_foundation::MIR::MirItemRef> {
    use jet_foundation::MIR::MirItemRef;
    match item {
        TirItemRef::Type(key) => types
            .iter()
            .find(|row| row.key == *key)
            .map(|row| MirItemRef::Type(row.id)),
        TirItemRef::Trait(key) => traits
            .iter()
            .find(|row| row.key == *key)
            .map(|row| MirItemRef::Trait(row.id)),
        TirItemRef::Function(key) => {
            resolve_function_key(key, module, registry).map(MirItemRef::Function)
        }
        TirItemRef::Constant(key) => constants
            .iter()
            .find(|row| row.key == *key)
            .map(|row| MirItemRef::Constant(row.id)),
        TirItemRef::Impl(key) => impls
            .iter()
            .find(|row| row.key == *key)
            .map(|row| MirItemRef::Impl(row.id)),
        TirItemRef::Foreign(key) => foreign
            .iter()
            .find(|row| row.key == *key)
            .map(|row| MirItemRef::Foreign(row.id)),
        TirItemRef::Unknown(key) => types
            .iter()
            .find(|row| row.key == *key)
            .map(|row| MirItemRef::Type(row.id))
            .or_else(|| {
                traits
                    .iter()
                    .find(|row| row.key == *key)
                    .map(|row| MirItemRef::Trait(row.id))
            })
            .or_else(|| {
                constants
                    .iter()
                    .find(|row| row.key == *key)
                    .map(|row| MirItemRef::Constant(row.id))
            })
            .or_else(|| {
                impls
                    .iter()
                    .find(|row| row.key == *key)
                    .map(|row| MirItemRef::Impl(row.id))
            })
            .or_else(|| {
                foreign
                    .iter()
                    .find(|row| row.key == *key)
                    .map(|row| MirItemRef::Foreign(row.id))
            })
            .or_else(|| resolve_function_key(key, module, registry).map(MirItemRef::Function))
            .or_else(|| {
                functions
                    .iter()
                    .find(|function| function.key == *key)
                    .and_then(|function| registry.id_for(function).ok())
                    .map(MirItemRef::Function)
            }),
        TirItemRef::Module(_) => None,
    }
}

fn lower_module_rows(
    modules: &[TirModuleFact],
    import_rows: &[TirImportFact],
    types: &[MirTypeDef],
    traits: &[MirTraitDef],
    impls: &[MirImplDef],
    constants: &[MirConstantDef],
    foreign: &[MirForeign],
    links: &[MirLinkUnit],
    jobs: &[MirJob],
    functions: &[TFunc],
    registry: &FunctionRegistry,
    _source_files: &[MirSourceFile],
) -> (Vec<jet_foundation::MIR::MirModule>, Vec<MirImport>) {
    let mut imports = import_rows
        .iter()
        .enumerate()
        .map(|(ordinal, row)| {
            let id = MirImportId(stable_id(
                "mir-import",
                &format!(
                    "{}|{}|{}..{}|{}",
                    row.module, row.alias, row.span.start, row.span.end, ordinal
                ),
            ));
            let kind = match &row.kind {
                TirImportKind::File { path } => MirImportKind::File { path: path.clone() },
                TirImportKind::Module { path } => MirImportKind::Module { path: path.clone() },
                TirImportKind::Unqualified { module, items } => MirImportKind::Unqualified {
                    module: jet_foundation::MIR::MirModuleId(stable_id("mir-module", module)),
                    items: items
                        .iter()
                        .filter_map(|item| {
                            Some(MirImportItem {
                                original: item.original.clone(),
                                local: item.local.clone(),
                                item: lower_module_item(
                                    &item.item, types, traits, impls, constants, foreign,
                                    functions, registry, module,
                                )?,
                            })
                        })
                        .collect(),
                },
            };
            MirImport {
                id,
                module: jet_foundation::MIR::MirModuleId(stable_id("mir-module", &row.module)),
                visibility: lower_name_visibility(row.visibility),
                alias: row.alias.clone(),
                kind,
                span: row.span,
                version: row.version.clone(),
            }
        })
        .collect::<Vec<_>>();
    imports.sort_unstable_by_key(|row| row.id);

    let mut module_rows = modules
        .iter()
        .map(|row| {
            let module_id = jet_foundation::MIR::MirModuleId(stable_id("mir-module", &row.key));
            let module_imports = imports
                .iter()
                .filter(|import| import.module == module_id)
                .map(|import| import.id)
                .collect();
            let item_order = row
                .item_order
                .iter()
                .filter_map(|item| {
                    lower_module_item(
                        item, types, traits, impls, constants, foreign, functions, registry,
                        &row.key,
                    )
                })
                .collect();
            let _ = (links, jobs);
            jet_foundation::MIR::MirModule {
                id: module_id,
                key: row.key.clone(),
                name: row.name.clone(),
                path: row.path.clone(),
                source_file: MirSourceFileId(stable_id("mir-source-file", &row.source_path)),
                imports: module_imports,
                children: row
                    .item_order
                    .iter()
                    .filter_map(|item| match item {
                        TirItemRef::Module(key) => Some(jet_foundation::MIR::MirModuleId(
                            stable_id("mir-module", key),
                        )),
                        _ => None,
                    })
                    .collect(),
                item_order,
            }
        })
        .collect::<Vec<_>>();
    module_rows.sort_unstable_by_key(|row| row.id);
    (module_rows, imports)
}

fn lower_name_visibility(visibility: crate::Names::NameVisibility) -> MirVisibility {
    match visibility {
        crate::Names::NameVisibility::Private => MirVisibility::Private,
        crate::Names::NameVisibility::Package => MirVisibility::Package,
        crate::Names::NameVisibility::Public => MirVisibility::Public,
    }
}

fn tuple_type_fields(ty: &MirType) -> Vec<jet_foundation::MIR::MirFieldRow> {
    let Some(fields) = ty.tuple_fields() else {
        return Vec::new();
    };
    let Some(owner) = ty.identity else {
        return Vec::new();
    };
    fields
        .iter()
        .map(|(name, field_ty)| {
            let id = MirFieldId(stable_id(
                "mir-field",
                &format!("{}::{name}", ty.identity_key()),
            ));
            jet_foundation::MIR::MirFieldRow {
                id,
                owner,
                field: jet_foundation::MIR::MirField {
                    id,
                    name: name.clone(),
                    shape_names: jet_foundation::Shape::ShapeFieldNames::from_source(name),
                    skip: false,
                    ty: field_ty.clone(),
                    span: Span::new(0, 0),
                    public: true,
                    package_public: true,
                    computed: false,
                    has_default: false,
                },
            }
        })
        .collect()
}

fn type_fields(ty: &MirTypeDef) -> Vec<jet_foundation::MIR::MirFieldRow> {
    let mut rows = Vec::new();
    match &ty.kind {
        MirTypeDefKind::Struct { fields, .. } => {
            rows.extend(
                fields
                    .iter()
                    .cloned()
                    .map(|field| jet_foundation::MIR::MirFieldRow {
                        id: field.id,
                        owner: ty.id,
                        field,
                    }),
            )
        }
        MirTypeDefKind::Enum { variants, .. } => {
            for variant in variants {
                if let jet_foundation::MIR::MirVariantPayload::Named(fields) = &variant.payload {
                    rows.extend(fields.iter().cloned().map(|field| {
                        jet_foundation::MIR::MirFieldRow {
                            id: field.id,
                            owner: ty.id,
                            field,
                        }
                    }));
                }
            }
        }
        MirTypeDefKind::Distinct { .. }
        | MirTypeDefKind::Alias { .. }
        | MirTypeDefKind::UnitFamily { .. } => {}
    }
    rows
}
/// Lower one explicitly selected checked artifact and return its stable MIR
/// artifact identity alongside the complete canonical program.
pub fn lower_checked_mir_program_for(
    bundle: &crate::AST::ProgramBundle,
    request: MirArtifactRequest,
) -> Result<(MirProgram, MirArtifactId), LowerError> {
    let target = request.target;
    let kind = request.kind;
    let tir = super::lower_checked_tir_program_for(bundle, request)?;
    let mir = lower_tir_to_mir(&tir)?;
    let mir = jet_foundation::MIR::optimize_mir_program(
        &mir,
        &jet_foundation::MIR::MirOptimizationPolicy::conservative(),
    )
    .map_err(|error| {
        LowerError::new(
            Span::new(0, 0),
            format!("canonical MIR optimization failed: {error}"),
        )
    })?;
    let artifact = mir
        .artifacts
        .iter()
        .find(|artifact| artifact.target == target && artifact.kind == kind)
        .map(|artifact| artifact.id)
        .ok_or_else(|| {
            LowerError::new(
                Span::new(0, 0),
                format!(
                    "checked artifact {:?}/{:?} did not produce a MIR artifact row",
                    target, kind
                ),
            )
        })?;
    Ok((mir, artifact))
}

fn declared_type_instances(types: &[MirTypeDef]) -> Vec<MirType> {
    types
        .iter()
        .map(|ty| lower_type(&Type::Named(ty.key.clone())).with_identity(ty.id))
        .collect()
}

fn type_def_types(definition: &MirTypeDef) -> Vec<MirType> {
    match &definition.kind {
        MirTypeDefKind::Struct { fields, .. } => {
            fields.iter().map(|field| field.ty.clone()).collect()
        }
        MirTypeDefKind::Enum { variants, .. } => variants
            .iter()
            .flat_map(|variant| match &variant.payload {
                MirVariantPayload::Unit => Vec::new(),
                MirVariantPayload::Single(ty) => vec![ty.clone()],
                MirVariantPayload::Named(fields) => {
                    fields.iter().map(|field| field.ty.clone()).collect()
                }
            })
            .collect(),
        MirTypeDefKind::Distinct { base, .. } => vec![base.clone()],
        MirTypeDefKind::Alias { target } => vec![target.clone()],
        MirTypeDefKind::UnitFamily { .. } => Vec::new(),
    }
}

fn function_type_rows(function: &MirFunction) -> Vec<MirType> {
    let mut rows = Vec::new();
    rows.extend(function.params.iter().map(|param| param.ty.clone()));
    if let Some(ty) = &function.declared_return {
        rows.push(ty.clone());
    }
    rows.push(function.return_type.clone());
    match &function.failure {
        jet_foundation::MIR::MirFailureCarrier::Infallible => {}
        jet_foundation::MIR::MirFailureCarrier::Result { success, error } => {
            rows.push(success.clone());
            rows.push(error.clone());
        }
        jet_foundation::MIR::MirFailureCarrier::Optional { value }
        | jet_foundation::MIR::MirFailureCarrier::Diverges { value } => rows.push(value.clone()),
    }
    rows.extend(function.locals.iter().map(|local| local.ty.clone()));
    rows.extend(function.values.iter().map(|(_, ty, _, _)| ty.clone()));
    rows.extend(function.places.iter().map(|place| place.ty.clone()));
    rows.extend(function.web_param_reconstructions.iter().flat_map(|row| {
        std::iter::once(row.ty.clone()).chain(row.fields.iter().map(|field| field.ty.clone()))
    }));
    rows.extend(function.capture_params.iter().map(|param| param.ty.clone()));
    if let Some(generator) = &function.generator {
        rows.push(generator.item.clone());
    }
    for block in &function.blocks {
        for instruction in &block.instructions {
            if let Some(ty) = &instruction.ty {
                rows.push(ty.clone());
            }
        }
    }
    rows
}

fn trait_method_type_rows(method: &MirTraitMethod) -> Vec<MirType> {
    let mut rows = Vec::new();
    rows.extend(method.params.iter().map(|param| param.ty.clone()));
    if let Some(ty) = &method.declared_return {
        rows.push(ty.clone());
    }
    rows.push(method.return_type.clone());
    match &method.failure {
        jet_foundation::MIR::MirFailureCarrier::Infallible => {}
        jet_foundation::MIR::MirFailureCarrier::Result { success, error } => {
            rows.push(success.clone());
            rows.push(error.clone());
        }
        jet_foundation::MIR::MirFailureCarrier::Optional { value }
        | jet_foundation::MIR::MirFailureCarrier::Diverges { value } => rows.push(value.clone()),
    }
    rows
}

fn ensure_type_instance(instances: &mut Vec<MirType>, instance: MirType) {
    let Some(identity) = instance.identity else {
        return;
    };
    if !instances
        .iter()
        .any(|candidate| candidate.identity == Some(identity))
    {
        instances.push(instance);
    }
}

fn merge_nested_type_instances(
    instances: &mut Vec<MirType>,
    ty: &MirType,
    span: Span,
) -> Result<(), LowerError> {
    match &ty.kind {
        MirTypeKind::List(inner)
        | MirTypeKind::Shared(inner)
        | MirTypeKind::Option(inner)
        | MirTypeKind::FixedList { elem: inner, .. }
        | MirTypeKind::InlineRange { base: inner, .. }
        | MirTypeKind::Tagged { inner, .. } => {
            merge_type_instance(instances, (**inner).clone(), span)?;
        }
        MirTypeKind::Map { key, value }
        | MirTypeKind::Result {
            ok: key,
            err: value,
        } => {
            merge_type_instance(instances, (**key).clone(), span)?;
            merge_type_instance(instances, (**value).clone(), span)?;
        }
        MirTypeKind::Fn(signature) => {
            for param in &signature.params {
                merge_type_instance(instances, param.clone(), span)?;
            }
            if let Some(ret) = &signature.ret {
                merge_type_instance(instances, (**ret).clone(), span)?;
            }
        }
        MirTypeKind::SendFn { params, ret } => {
            for param in params {
                merge_type_instance(instances, param.clone(), span)?;
            }
            if let Some(ret) = ret {
                merge_type_instance(instances, (**ret).clone(), span)?;
            }
        }
        MirTypeKind::Apply { args, .. } => {
            for arg in args {
                merge_type_instance(instances, arg.clone(), span)?;
            }
        }
        MirTypeKind::Tuple(fields) => {
            for (_, field) in fields {
                merge_type_instance(instances, field.clone(), span)?;
            }
        }
        MirTypeKind::Union(members) => {
            for member in members {
                merge_type_instance(instances, member.clone(), span)?;
            }
        }
        MirTypeKind::Quantity { base, .. } => {
            merge_type_instance(instances, (**base).clone(), span)?;
        }
        MirTypeKind::Int
        | MirTypeKind::Float
        | MirTypeKind::Bool
        | MirTypeKind::String
        | MirTypeKind::Char
        | MirTypeKind::TraitObject(_)
        | MirTypeKind::IntN { .. }
        | MirTypeKind::Float32
        | MirTypeKind::Measure(_) => {}
    }
    Ok(())
}

fn merge_type_instance(
    instances: &mut Vec<MirType>,
    instance: MirType,
    span: Span,
) -> Result<(), LowerError> {
    let Some(identity) = instance.identity else {
        return Err(LowerError::new(
            span,
            "MIR type instance has no canonical identity",
        ));
    };
    if let Some(existing) = instances
        .iter()
        .find(|candidate| candidate.identity == Some(identity))
    {
        if existing != &instance {
            return Err(LowerError::new(
                span,
                format!("conflicting canonical MIR type instance {identity:?}"),
            ));
        }
    } else {
        instances.push(instance.clone());
    }
    merge_nested_type_instances(instances, &instance, span)
}

fn lower_package_facts(program: &TirProgram) -> MirPackageFacts {
    MirPackageFacts {
        project_root: program.facts.project_root.clone(),
        edition: program.edition.clone(),
        runtime_parts: crate::Codegen::runtime_parts_for_used_core(&program.facts.used_core),
        uses_arrow: program
            .facts
            .used_core
            .iter()
            .any(|name| name == "core.data.arrow" || name.starts_with("core.data.arrow.")),
        active_os: program.facts.active_os.clone(),
        inferred_layer: program.facts.inferred_layer.clone(),
        allocator: program.facts.allocator.clone(),
        package_version: program.artifact_facts.package_version.clone(),
        artifact_target: Some(program.artifact_facts.target),
        target_dossier: Default::default(),
        web_app: program.facts.web_app.clone(),
        model_outputs: program.facts.model_outputs.clone(),
        authority_needs: program.facts.authority_needs.clone(),
        hardware_use: program.facts.hardware_use.clone(),
        hardware_profile: program.facts.hardware_profile.clone(),
        hardware_profile_id: program.facts.hardware_profile_id.clone(),
        hardware_capabilities: program.facts.hardware_capabilities.clone(),
        hardware_setups: program
            .facts
            .hardware_setups
            .iter()
            .map(lower_hardware_setup)
            .collect(),
    }
}

fn lower_hardware_setup(setup: &THardwareSetup) -> MirHardwareSetup {
    match setup {
        THardwareSetup::DmaConfigure {
            profile_id,
            channel,
            transfer_width,
            ownership,
        } => MirHardwareSetup::DmaConfigure {
            profile_id: profile_id.clone(),
            channel: channel.clone(),
            transfer_width: *transfer_width,
            ownership: ownership.clone(),
        },
        THardwareSetup::InterruptBind {
            profile_id,
            interrupt,
            vector,
            handler_symbol,
            forbidden_effects,
        } => MirHardwareSetup::InterruptBind {
            profile_id: profile_id.clone(),
            interrupt: interrupt.clone(),
            vector: *vector,
            handler_symbol: handler_symbol.clone(),
            forbidden_effects: forbidden_effects.clone(),
        },
    }
}

fn lower_cffi_facts(facts: &super::artifact_plan::TirCffiFacts) -> MirCffiFacts {
    MirCffiFacts {
        import_links: facts
            .import_links
            .iter()
            .map(|row| MirCImportLink {
                importing_module: row.importing_module.clone(),
                scope: row.scope.clone(),
                alias: row.alias.clone(),
                target_module: row.target_module.clone(),
            })
            .collect(),
        libs: facts
            .libs
            .iter()
            .map(|row| MirCLib {
                lib: row.lib.clone(),
                module: row.module.clone(),
            })
            .collect(),
        overlay_overrides: facts
            .overlay_overrides
            .iter()
            .map(|row| MirCOverlayOverride {
                lib: row.lib.clone(),
                generated_symbol: row.generated_symbol.clone(),
                overlay_symbol: row.overlay_symbol.clone(),
            })
            .collect(),
        boundaries: facts.boundaries.clone(),
        direct_links: facts.direct_links.clone(),
        transitive_links: facts.transitive_links.clone(),
        close_adapters: facts
            .close_adapters
            .iter()
            .map(|row| MirCloseAdapter {
                lib: row.lib.clone(),
                handle_type: row.handle_type.clone(),
                raw_function: row.raw_function.clone(),
                adapter_function: row.adapter_function.clone(),
            })
            .collect(),
    }
}

fn lower_name_facts(facts: &super::artifact_plan::TirNameFacts) -> MirNameFacts {
    MirNameFacts {
        modules: facts.modules.clone(),
        declarations: facts.declarations.clone(),
        aliases: facts.aliases.clone(),
        references: facts.references.clone(),
        structure_facts: facts.structure_facts.clone(),
    }
}
type LoweredFunction = (
    MirFunction,
    Vec<MirPreludeCall>,
    Vec<MirSourceFile>,
    Vec<MirType>,
    Vec<MirFunction>,
    Vec<MirCallbackAdapter>,
);
fn lower_trait_ref(ctx: &LowerCtx<'_>, name: &str) -> Result<MirTraitRef, LowerError> {
    let canonical = ctx
        .nominal_identities
        .get(name)
        .cloned()
        .or_else(|| {
            if name.contains("::") || super::tir_to_mir_types::is_compiler_owned_trait(name) {
                Some(name.to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| {
            ctx.error(
                ctx.span(),
                format!("missing canonical trait identity `{name}`"),
            )
        })?;
    Ok(MirTraitRef::from_name(canonical))
}

fn lower_function_form(
    ctx: &mut LowerCtx<'_>,
    kind: &TFuncKind,
) -> Result<MirFunctionForm, LowerError> {
    match kind {
        TFuncKind::TopLevel => Ok(MirFunctionForm::TopLevel),
        TFuncKind::Method {
            owner_type,
            self_conv,
        } => Ok(MirFunctionForm::Method {
            owner: ctx.mir_type(owner_type)?,
            self_access: self_conv.map(lower_mir_convention),
        }),
        TFuncKind::TraitMethod {
            owner_type,
            trait_name,
            self_conv,
            serde,
            ..
        } => Ok(MirFunctionForm::TraitMethod {
            owner: ctx.mir_type(owner_type)?,
            trait_ref: lower_trait_ref(ctx, trait_name)?,
            self_access: self_conv.map(lower_mir_convention),
            serde: serde.map(|codec| match codec {
                super::SerdeCodec::Encode => MirSerdeCodec::Encode,
                super::SerdeCodec::Decode => MirSerdeCodec::Decode,
            }),
        }),
    }
}

fn parse_runtime_part(key: &str) -> Option<MirRuntimePartId> {
    use MirRuntimePartId::*;
    [
        Gc,
        Event,
        Realtime,
        EmbeddedHardware,
        Ui,
        Devtools,
        Gtk,
        Apps,
        Email,
        Game,
        Files,
        Interrupt,
        FsRuntime,
        Process,
        Crypto,
        Math,
        Encoding,
        Data,
        Fmt,
        DataFmt,
        Compute,
        Http,
        WebSocket,
        Browser,
        Args,
        Reflect,
        AuthTokens,
        AuthSession,
        Sync,
        Services,
        Mod,
    ]
    .into_iter()
    .find(|part| part.as_str() == key)
}

fn int_width(ty: &Type) -> Option<(bool, u8)> {
    match ty.without_user_tags() {
        Type::Int => Some((true, 64)),
        Type::IntN { signed, bits } => Some((*signed, *bits)),
        Type::InlineRange { base, .. } => int_width(base),
        Type::Tagged { inner, .. } => int_width(inner),
        _ => None,
    }
}
fn lower_function(
    f: &TFunc,
    type_defs: &[MirTypeDef],
    nominal_identities: &HashMap<String, String>,
    trait_defs: &[super::tir_to_mir_types::TirTraitDef],
    function_registry: &FunctionRegistry,
    source_texts: &BTreeMap<String, String>,
    body: Option<&TLambdaBody>,
    expression: Option<&TExpr>,
    lambda: Option<&TLambda>,
    capture_specs: Option<(&[(String, String, Type)], MirCaptureFacts)>,
) -> Result<LoweredFunction, LowerError> {
    let mut ctx = LowerCtx::new(
        f,
        type_defs,
        nominal_identities,
        trait_defs,
        function_registry,
        source_texts,
    );
    ctx.source_file_id_for(&f.source_file);

    if let Some(lambda) = lambda {
        ctx.bind_captures(lambda)?;
    } else if let Some((captures, facts)) = capture_specs {
        ctx.bind_capture_specs(captures, facts, f.source_span)?;
    }
    let mut param_index = 0;
    if let Some((owner_type, conv)) = f.kind.receiver() {
        ctx.bind_parameter(0, jet_foundation::Syntax::KW_SELF, owner_type, conv)?;
        param_index = 1;
    }
    for (offset, (name, ty, access)) in f.params.iter().enumerate() {
        ctx.bind_parameter(param_index + offset, name, ty, *access)?;
    }
    if let Some(expr) = expression {
        let value = ctx.lower_child(expr)?;
        if !ctx.is_terminated() {
            ctx.terminate_with_cleanup(MirTerminator::Return { value: Some(value) }, 0)?;
        }
    } else {
        match body {
            Some(TLambdaBody::Expr(expr)) => {
                let value = ctx.lower_child(expr)?;
                if !ctx.is_terminated() {
                    ctx.terminate_with_cleanup(MirTerminator::Return { value: Some(value) }, 0)?;
                }
            }
            Some(TLambdaBody::Block(stmts)) => {
                lower_stmts(&mut ctx, stmts)?;
                if !ctx.is_terminated() {
                    ctx.terminate_with_cleanup(MirTerminator::Return { value: None }, 0)?;
                }
            }
            Some(TLambdaBody::SharedBlock(stmts)) => {
                lower_stmts(&mut ctx, stmts.as_ref())?;
                if !ctx.is_terminated() {
                    ctx.terminate_with_cleanup(MirTerminator::Return { value: None }, 0)?;
                }
            }
            None => {
                lower_stmts(&mut ctx, &f.body)?;
                if !ctx.is_terminated() {
                    ctx.terminate_with_cleanup(MirTerminator::Return { value: None }, 0)?;
                }
            }
        }
    }

    let return_source = f
        .ret
        .clone()
        .unwrap_or_else(|| Type::Named(jet_foundation::Syntax::INTERNAL_UNIT_TYPE.to_string()));
    let return_type = ctx.mir_type(&return_source)?;
    let function_id = ctx.function_id()?;
    let declared_return = f.ret.as_ref().map(|ty| ctx.mir_type(ty)).transpose()?;
    let generator = match f.ret.as_ref() {
        Some(Type::Apply { name, args }) if name == crate::Syntax::TYPE_STREAM => {
            let [item] = args.as_slice() else {
                return Err(ctx.error(
                    f.source_span,
                    "checked Stream return must carry exactly one item type",
                ));
            };
            Some(MirGeneratorFacts {
                item: ctx.mir_type(item)?,
            })
        }
        Some(Type::Named(name)) if name == crate::Syntax::TYPE_STREAM => {
            return Err(ctx.error(
                f.source_span,
                "checked Stream return is missing its item type",
            ));
        }
        _ => None,
    };
    let failure = lower_failure_carrier(&mut ctx, &f.failure_carrier)?;
    let web_param_reconstructions = f
        .web_param_reconstructions
        .iter()
        .map(|row| {
            Ok::<_, LowerError>(jet_foundation::MIR::MirWebParamReconstruction {
                local: row.local.name.clone(),
                ty: ctx.mir_type(&row.ty)?,
                fields: row
                    .fields
                    .iter()
                    .map(|(field, parameter, ty)| {
                        Ok::<_, LowerError>(jet_foundation::MIR::MirWebParamField {
                            field: field.clone(),
                            parameter: parameter.clone(),
                            ty: ctx.mir_type(ty)?,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut optimization = jet_foundation::MIR::MirOptimizationFacts::default();
    optimization.pure = f.is_pure;
    optimization.inline = f.is_inline;
    optimization.inline_always = f.is_inline_always;
    optimization.gc_return = f.gc_return;
    optimization.kernel = f.kernel_proof.is_some();

    let form = lower_function_form(&mut ctx, &f.kind)?;
    let mut scopes = ctx.scopes;
    scopes.sort_unstable_by_key(|scope| scope.id);
    let mut blocks = ctx.blocks;
    blocks.sort_unstable_by_key(|block| block.id);
    let mir_function = MirFunction {
        id: function_id,
        module_id: jet_foundation::MIR::MirModuleId(stable_id("mir-module", &f.module)),
        key: f.key.clone(),
        module: f.module.clone(),
        name: f.name.clone(),
        span: f.source_span,
        kind: MirFunctionKind::Jet,
        form,
        visibility: lower_visibility(f.visibility),
        target_applicability: MirTargetApplicability {
            rust_aot: f.target_applicability.rust_aot,
            cranelift: f.target_applicability.cranelift,
            interpreter: f.target_applicability.interpreter,
            web: f.target_applicability.web,
        },
        web_bucket: f.web_bucket.clone(),
        web_marker: f.web_marker.clone(),
        generic_params: f
            .generic_params
            .iter()
            .map(|param| jet_foundation::MIR::MirGenericParam {
                name: param.name.clone(),
                bounds: param
                    .bounds
                    .iter()
                    .map(|bound| MirTraitRef::from_name(bound.clone()))
                    .collect(),
            })
            .collect(),
        capture_params: ctx.capture_params,
        params: ctx.params,
        declared_return,
        return_type,
        failure,
        effects: MirEffectFacts {
            direct: f.effects.direct.clone(),
            solved: f.effects.solved.clone(),
            call_edges: f.effects.call_edges.clone(),
            maximal: f.effects.maximal,
            direct_spans: f.effects.direct_spans.clone(),
        },
        unsafe_gate: f
            .unsafe_gate
            .as_ref()
            .map(|gate| jet_foundation::MIR::MirUnsafeGate {
                file: gate.file.clone(),
                line: gate.line,
                reason: gate.reason.clone(),
                enabled: gate.enabled,
                fenced: gate.fenced,
            }),
        is_pure: f.is_pure,
        is_unsafe: f.is_unsafe,
        memo_bound: f.memo_bound,
        is_reactive: f.is_reactive,
        reactive_upgrades: f.reactive_upgrades.clone(),
        captures: ctx.capture_facts,
        generator,
        optimization,
        is_inline: f.is_inline,
        is_inline_always: f.is_inline_always,
        is_scalar: f.is_scalar,
        kernel_proof: f
            .kernel_proof
            .map(|proof| jet_foundation::MIR::MirKernelFacts {
                mode: jet_foundation::MIR::MirKernelMode::Parallel,
                bounds: proof.bounds,
                alias_free: proof.alias_free,
                captures: proof.captures,
                race_free: proof.race_free,
                barriers_uniform: proof.barriers_uniform,
                control_flow: proof.control_flow,
            }),
        gc_return: f.gc_return,
        return_view_provenance: f.return_view_provenance.as_ref().map(lower_view_provenance),
        web_param_reconstructions,
        blocks,
        entry: ctx.entry,
        locals: ctx.locals,
        values: ctx.values,
        places: ctx.places,
        scopes,

        drops: ctx.drops,
        foreign_language: f.foreign.as_ref().map(|foreign| foreign.language.clone()),
    };
    let mut source_files = ctx
        .source_files
        .into_iter()
        .map(|(path, id)| MirSourceFile {
            id,
            path,
            source: String::new(),
        })
        .collect::<Vec<_>>();
    source_files.sort_unstable_by_key(|file| file.id);

    let nested_functions = ctx.nested_functions;
    let mut prelude_calls = ctx.prelude_calls;
    prelude_calls.sort_unstable_by_key(|call| call.id);
    let mut callbacks = ctx.callbacks;
    callbacks.sort_unstable_by_key(|callback| callback.id);
    Ok((
        mir_function,
        prelude_calls,
        source_files,
        ctx.type_instances,
        nested_functions,
        callbacks,
    ))
}

pub(super) fn type_identity_key(ty: &Type) -> String {
    match ty {
        Type::Int => "Int".to_string(),
        Type::Float => "Float".to_string(),
        Type::Float32 => "Float32".to_string(),
        Type::Bool => "Bool".to_string(),
        Type::String => "String".to_string(),
        Type::Char => "Char".to_string(),
        Type::List(inner) => format!("List<{}>", type_identity_key(inner)),
        Type::Map { key, value, .. } => {
            format!(
                "Map<{},{}>",
                type_identity_key(key),
                type_identity_key(value)
            )
        }
        Type::Shared(inner) => format!("Shared<{}>", type_identity_key(inner)),
        Type::Option(inner) => format!("Option<{}>", type_identity_key(inner)),
        Type::Result { ok, err } => {
            format!(
                "Result<{},{}>",
                type_identity_key(ok),
                type_identity_key(err)
            )
        }
        Type::Fn { params, ret, .. } => {
            let params = params
                .iter()
                .map(type_identity_key)
                .collect::<Vec<_>>()
                .join(",");
            let ret = ret
                .as_deref()
                .map(type_identity_key)
                .unwrap_or_else(|| "Unit".to_string());
            format!("Fn({params})->{ret}")
        }
        Type::Named(name) => name.clone(),
        Type::Apply { name, args } if args.is_empty() => name.clone(),
        Type::Apply { name, args } => format!(
            "{}<{}>",
            name,
            args.iter()
                .map(type_identity_key)
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::TraitObject(bounds) => format!("Trait({})", bounds.join("+")),
        Type::Tuple(fields) => format!(
            "Tuple({})",
            fields
                .iter()
                .map(|(name, ty)| format!("{name}:{}", type_identity_key(ty)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        Type::FixedList { elem, len } => {
            format!("FixedList<{};{len:?}>", type_identity_key(elem))
        }
        Type::IntN { signed, bits } => format!("{}Int{bits}", if *signed { "I" } else { "U" }),
        Type::InlineRange { base, lo, hi } => {
            format!("Range<{};{lo}..{hi}>", type_identity_key(base))
        }
        Type::Tagged { marker, inner } => match marker {
            jet_foundation::AST::TagMarker::User(_) => type_identity_key(inner),
            _ => format!("Tagged({marker:?};{})", type_identity_key(inner)),
        },
        Type::Union(members) => format!(
            "Union({})",
            members
                .iter()
                .map(type_identity_key)
                .collect::<Vec<_>>()
                .join("|")
        ),
        Type::Quantity { base, dimension } => {
            format!("Quantity<{};{dimension:?}>", type_identity_key(base))
        }
        Type::Measure(measure) => format!("Measure({measure:?})"),
    }
}

fn canonical_nominal_name(
    type_defs: &[MirTypeDef],
    name: &str,
    span: Span,
) -> Result<String, LowerError> {
    let exact = type_defs
        .iter()
        .filter(|ty| ty.key == name)
        .collect::<Vec<_>>();
    if let [ty] = exact.as_slice() {
        return Ok(ty.key.clone());
    }
    if exact.len() > 1 {
        return Err(LowerError::new(
            span,
            format!("ambiguous checked MIR type key `{name}`"),
        ));
    }
    let matches = type_defs
        .iter()
        .filter(|ty| ty.name == name)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [ty] => Ok(ty.key.clone()),
        [] => Ok(name.to_string()),
        _ => Err(LowerError::new(
            span,
            format!("ambiguous checked MIR type name `{name}`"),
        )),
    }
}

fn canonicalize_type(type_defs: &[MirTypeDef], ty: &Type, span: Span) -> Result<Type, LowerError> {
    Ok(match ty {
        Type::List(inner) => Type::List(Box::new(canonicalize_type(type_defs, inner, span)?)),
        Type::Map {
            key,
            key_span,
            value,
        } => Type::Map {
            key: Box::new(canonicalize_type(type_defs, key, span)?),
            key_span: *key_span,
            value: Box::new(canonicalize_type(type_defs, value, span)?),
        },
        Type::Shared(inner) => Type::Shared(Box::new(canonicalize_type(type_defs, inner, span)?)),
        Type::Option(inner) => Type::Option(Box::new(canonicalize_type(type_defs, inner, span)?)),
        Type::Result { ok, err } => Type::Result {
            ok: Box::new(canonicalize_type(type_defs, ok, span)?),
            err: Box::new(canonicalize_type(type_defs, err, span)?),
        },
        Type::Fn {
            params,
            ret,
            effect_bound,
            param_contract,
            call_metadata,
            return_view_provenance,
        } => Type::Fn {
            params: params
                .iter()
                .map(|param| canonicalize_type(type_defs, param, span))
                .collect::<Result<Vec<_>, _>>()?,
            ret: ret
                .as_deref()
                .map(|ret| canonicalize_type(type_defs, ret, span).map(Box::new))
                .transpose()?,
            effect_bound: effect_bound.clone(),
            param_contract: param_contract.clone(),
            call_metadata: call_metadata.clone(),
            return_view_provenance: return_view_provenance.clone(),
        },
        Type::Named(name) => Type::Named(canonical_nominal_name(type_defs, name, span)?),
        Type::Apply { name, args } => Type::Apply {
            name: canonical_nominal_name(type_defs, name, span)?,
            args: args
                .iter()
                .map(|arg| canonicalize_type(type_defs, arg, span))
                .collect::<Result<Vec<_>, _>>()?,
        },
        Type::Tuple(fields) => Type::Tuple(
            fields
                .iter()
                .map(|(name, ty)| {
                    canonicalize_type(type_defs, ty, span).map(|ty| (name.clone(), Box::new(ty)))
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Type::FixedList { elem, len } => Type::FixedList {
            elem: Box::new(canonicalize_type(type_defs, elem, span)?),
            len: len.clone(),
        },
        Type::InlineRange { base, lo, hi } => Type::InlineRange {
            base: Box::new(canonicalize_type(type_defs, base, span)?),
            lo: *lo,
            hi: *hi,
        },
        Type::Tagged { marker, inner } => Type::Tagged {
            marker: marker.clone(),
            inner: Box::new(canonicalize_type(type_defs, inner, span)?),
        },
        Type::Union(members) => Type::Union(
            members
                .iter()
                .map(|member| canonicalize_type(type_defs, member, span))
                .collect::<Result<Vec<_>, _>>()?,
        ),
        Type::Quantity { base, dimension } => Type::Quantity {
            base: Box::new(canonicalize_type(type_defs, base, span)?),
            dimension: dimension.clone(),
        },
        _ => ty.clone(),
    })
}
fn canonical_type_instance(
    type_defs: &[MirTypeDef],
    ty: &Type,
    span: Span,
) -> Result<MirType, LowerError> {
    let source = canonicalize_type(type_defs, ty, span)?;
    let key = type_identity_key(&source);
    let identity = match &source {
        Type::Named(name) => type_defs
            .iter()
            .find(|row| row.key == *name || row.name == *name)
            .map(|row| row.id)
            .unwrap_or_else(|| MirTypeId(stable_id("mir-type", &key))),
        Type::Apply { name, args } if args.is_empty() => type_defs
            .iter()
            .find(|row| row.key == *name || row.name == *name)
            .map(|row| row.id)
            .unwrap_or_else(|| MirTypeId(stable_id("mir-type", &key))),
        _ => MirTypeId(stable_id("mir-type", &key)),
    };
    Ok(lower_type(&source).with_identity(identity))
}
fn lower_visibility(visibility: super::TVisibility) -> jet_foundation::MIR::MirVisibility {
    match visibility {
        super::TVisibility::Private => jet_foundation::MIR::MirVisibility::Private,
        super::TVisibility::Package => jet_foundation::MIR::MirVisibility::Package,
        super::TVisibility::Public => jet_foundation::MIR::MirVisibility::Public,
    }
}

fn lower_failure_carrier(
    ctx: &mut LowerCtx<'_>,
    carrier: &TFailureCarrier,
) -> Result<jet_foundation::MIR::MirFailureCarrier, LowerError> {
    Ok(match carrier {
        TFailureCarrier::Infallible => jet_foundation::MIR::MirFailureCarrier::Infallible,
        TFailureCarrier::Result { success, error } => {
            jet_foundation::MIR::MirFailureCarrier::Result {
                success: ctx.mir_type(success)?,
                error: ctx.mir_type(error)?,
            }
        }
        TFailureCarrier::Optional { value } => jet_foundation::MIR::MirFailureCarrier::Optional {
            value: ctx.mir_type(value)?,
        },
        TFailureCarrier::Diverges { value } => jet_foundation::MIR::MirFailureCarrier::Diverges {
            value: ctx.mir_type(value)?,
        },
    })
}
fn mir_type_as_ast(ty: &MirType) -> Type {
    match &ty.kind {
        MirTypeKind::Int => Type::Int,
        MirTypeKind::Float => Type::Float,
        MirTypeKind::Bool => Type::Bool,
        MirTypeKind::String => Type::String,
        MirTypeKind::Char => Type::Char,
        MirTypeKind::List(inner) => Type::List(Box::new(mir_type_as_ast(inner))),
        MirTypeKind::Map { key, value } => Type::Map {
            key: Box::new(mir_type_as_ast(key)),
            key_span: None,
            value: Box::new(mir_type_as_ast(value)),
        },
        MirTypeKind::Shared(inner) => Type::Shared(Box::new(mir_type_as_ast(inner))),
        MirTypeKind::Option(inner) => Type::Option(Box::new(mir_type_as_ast(inner))),
        MirTypeKind::Result { ok, err } => Type::Result {
            ok: Box::new(mir_type_as_ast(ok)),
            err: Box::new(mir_type_as_ast(err)),
        },
        MirTypeKind::Fn(signature) => Type::Fn {
            params: signature.params.iter().map(mir_type_as_ast).collect(),
            ret: signature.ret.as_deref().map(mir_type_as_ast).map(Box::new),
            effect_bound: None,
            param_contract: None,
            call_metadata: None,
            return_view_provenance: None,
        },
        MirTypeKind::SendFn { params, ret } => Type::Fn {
            params: params.iter().map(mir_type_as_ast).collect(),
            ret: ret.as_deref().map(mir_type_as_ast).map(Box::new),
            effect_bound: None,
            param_contract: None,
            call_metadata: None,
            return_view_provenance: None,
        },
        MirTypeKind::Apply { name, args } if args.is_empty() => Type::Named(name.name.clone()),
        MirTypeKind::Apply { name, args } => Type::Apply {
            name: name.name.clone(),
            args: args.iter().map(mir_type_as_ast).collect(),
        },
        MirTypeKind::TraitObject(bounds) => {
            Type::TraitObject(bounds.iter().map(|bound| bound.name.clone()).collect())
        }
        MirTypeKind::Tuple(fields) => Type::Tuple(
            fields
                .iter()
                .map(|(name, ty)| (name.clone(), Box::new(mir_type_as_ast(ty))))
                .collect(),
        ),
        MirTypeKind::FixedList { elem, len } => Type::FixedList {
            elem: Box::new(mir_type_as_ast(elem)),
            len: crate::AST::Measure::symbol("mir-measure", len.to_string()),
        },
        MirTypeKind::IntN { signed, bits } => Type::IntN {
            signed: *signed,
            bits: *bits,
        },
        MirTypeKind::InlineRange { base, lo, hi } => Type::InlineRange {
            base: Box::new(mir_type_as_ast(base)),
            lo: *lo,
            hi: *hi,
        },
        MirTypeKind::Float32 => Type::Float32,
        MirTypeKind::Tagged { inner, .. } => mir_type_as_ast(inner),
        MirTypeKind::Union(members) => Type::Union(members.iter().map(mir_type_as_ast).collect()),
        MirTypeKind::Quantity { base, .. } => mir_type_as_ast(base),
        MirTypeKind::Measure(measure) => Type::Measure(crate::AST::Measure::symbol(
            "mir-measure",
            measure.to_string(),
        )),
    }
}

fn call_fallibility(
    ctx: &mut LowerCtx<'_>,
    carrier: &TFailureCarrier,
) -> Result<MirCallFallibility, LowerError> {
    Ok(match carrier {
        TFailureCarrier::Infallible => MirCallFallibility::Infallible,
        _ => MirCallFallibility::Failure(lower_failure_carrier(ctx, carrier)?),
    })
}

pub(super) struct LowerCtx<'a> {
    pub(super) function: &'a TFunc,
    pub(super) type_defs: &'a [MirTypeDef],
    pub(super) nominal_identities: &'a HashMap<String, String>,
    pub(super) trait_defs: &'a [super::tir_to_mir_types::TirTraitDef],
    pub(super) function_registry: &'a FunctionRegistry,
    pub(super) trait_method_traits: HashMap<(String, String), String>,
    pub(super) source_texts: &'a BTreeMap<String, String>,
    pub(super) entry: MirBlockId,
    pub(super) current: MirBlockId,
    pub(super) blocks: Vec<MirBasicBlock>,
    pub(super) locals: Vec<MirLocal>,
    pub(super) values: Vec<(MirValueId, MirType, Span, MirOwnership)>,
    pub(super) places: Vec<MirPlace>,
    pub(super) params: Vec<MirParam>,
    pub(super) capture_params: Vec<MirCaptureParam>,
    pub(super) capture_facts: Option<MirCaptureFacts>,
    pub(super) scopes: Vec<MirScope>,
    pub(super) drops: Vec<MirDropAction>,
    pub(super) nested_functions: Vec<MirFunction>,
    pub(super) loops: Vec<(Option<String>, MirBlockId, MirBlockId)>,
    loop_defer_depths: Vec<usize>,
    defer_stack: Vec<Vec<MirValueId>>,
    pub(super) local_places: HashMap<String, MirPlaceId>,
    pub(super) local_types: HashMap<String, Type>,
    pub(super) local_values: HashMap<String, MirValueId>,
    send_fn_locals: HashSet<String>,
    capture_values: HashMap<String, MirValueId>,
    pub(super) prelude_calls: Vec<MirPreludeCall>,
    pub(super) callbacks: Vec<MirCallbackAdapter>,
    pub(super) source_files: HashMap<String, MirSourceFileId>,
    pub(super) type_instances: Vec<MirType>,
    pub(super) current_span: Span,
    pub(super) current_line: Option<u32>,
    switch_subject: Option<MirValueId>,
    identity_counts: HashMap<String, usize>,
}

fn construct_identity(
    function: &TFunc,
    kind: &str,
    span: Span,
    role: &str,
    detail: &str,
) -> String {
    format!(
        "{}|{}|{}|{}|{}|{}",
        function_identity(function),
        kind,
        span.start,
        span.end,
        role,
        detail
    )
}

fn field_owner_type(ty: &Type) -> &Type {
    match ty {
        Type::Tagged { inner, .. } => field_owner_type(inner),
        Type::Apply { name, args }
            if name == crate::Syntax::TYPE_SHARED_GUARD && args.len() == 1 =>
        {
            field_owner_type(&args[0])
        }
        _ => ty,
    }
}

fn field_owner_mir_type(ty: &MirType) -> &MirType {
    match ty.kind() {
        MirTypeKind::Tagged { inner, .. } => field_owner_mir_type(inner),
        MirTypeKind::Apply { name, args }
            if name.name == crate::Syntax::TYPE_SHARED_GUARD && args.len() == 1 =>
        {
            field_owner_mir_type(&args[0])
        }
        _ => ty,
    }
}
/// A place row can be referenced by several operations. Keep the strongest
/// access ever required so a later read cannot invalidate an earlier borrow or
/// move operation.
fn retain_place_access(place: &mut MirPlace, requested: MirAccess) {
    place.access = match (place.access, requested) {
        (MirAccess::Move, _) | (_, MirAccess::Move) => MirAccess::Move,
        (MirAccess::Write, _) | (_, MirAccess::Write) => MirAccess::Write,
        _ => MirAccess::Read,
    };
}

impl<'a> LowerCtx<'a> {
    fn new(
        function: &'a TFunc,
        type_defs: &'a [MirTypeDef],
        nominal_identities: &'a HashMap<String, String>,
        trait_defs: &'a [super::tir_to_mir_types::TirTraitDef],
        function_registry: &'a FunctionRegistry,
        source_texts: &'a BTreeMap<String, String>,
    ) -> Self {
        let entry_identity =
            construct_identity(function, "block", function.source_span, "entry", "");
        let entry = MirBlockId(stable_id("mir-block", &entry_identity));
        let block = MirBasicBlock {
            id: entry,
            span: function.source_span,
            instructions: Vec::new(),
            terminator: MirTerminator::Unreachable {
                reason: "lowering in progress".to_string(),
            },
        };
        let mut identity_counts = HashMap::new();
        identity_counts.insert(entry_identity, 1);
        Self {
            function,
            nominal_identities,
            type_defs,
            trait_defs,
            function_registry,
            trait_method_traits: HashMap::new(),
            source_texts,
            entry,
            current: entry,
            blocks: vec![block],
            locals: Vec::new(),
            values: Vec::new(),
            places: Vec::new(),
            params: Vec::new(),
            capture_params: Vec::new(),
            capture_facts: None,
            scopes: Vec::new(),
            drops: Vec::new(),
            nested_functions: Vec::new(),
            loops: Vec::new(),
            loop_defer_depths: Vec::new(),
            defer_stack: vec![Vec::new()],
            local_places: HashMap::new(),
            local_types: HashMap::new(),
            local_values: HashMap::new(),
            send_fn_locals: HashSet::new(),
            callbacks: Vec::new(),
            capture_values: HashMap::new(),
            prelude_calls: Vec::new(),
            source_files: HashMap::new(),
            type_instances: Vec::new(),
            current_span: function.source_span,
            current_line: None,
            switch_subject: None,
            identity_counts,
        }
    }

    pub(super) fn trait_method_identity(
        &self,
        trait_name: &str,
        method_name: &str,
    ) -> Result<(MirTraitRef, MirTraitMethodId, Option<MirAccess>), LowerError> {
        let trait_ref = lower_trait_ref(self, trait_name)?;
        let row = self
            .trait_defs
            .iter()
            .find(|row| row.key == trait_ref.name || row.name == trait_name)
            .ok_or_else(|| {
                self.error(
                    self.span(),
                    format!("missing checked trait row `{}`", trait_ref.name),
                )
            })?;
        let method = row
            .methods
            .iter()
            .find(|method| method.name == method_name)
            .ok_or_else(|| {
                self.error(
                    self.span(),
                    format!(
                        "missing checked method `{method_name}` on trait `{}`",
                        row.name
                    ),
                )
            })?;
        let method_id = MirTraitMethodId(stable_id(
            "mir-trait-method",
            &format!("{}::{}", row.key, method.key),
        ));
        Ok((
            trait_ref,
            method_id,
            method.self_access.map(super::tir_to_mir_types::mir_access),
        ))
    }

    fn reserve_identity(
        &mut self,
        kind: &str,
        span: Span,
        role: &str,
        detail: &str,
    ) -> Result<String, LowerError> {
        let base = construct_identity(self.function, kind, span, role, detail);
        let ordinal = self.identity_counts.entry(base.clone()).or_insert(0);
        let occurrence = *ordinal;
        *ordinal += 1;
        let identity = if occurrence == 0 {
            base
        } else {
            format!("{base}#{occurrence}")
        };
        Ok(identity)
    }
    pub(super) fn span(&self) -> Span {
        self.current_span
    }

    pub(super) fn set_span(&mut self, span: Span) {
        self.current_span = span;
    }
    pub(super) fn set_line_marker(&mut self, line: u32) {
        self.current_line = Some(line);
    }
    pub(super) fn with_switch_subject<R>(
        &mut self,
        subject: MirValueId,
        lower: impl FnOnce(&mut Self) -> Result<R, LowerError>,
    ) -> Result<R, LowerError> {
        let previous = self.switch_subject.replace(subject);
        let result = lower(self);
        self.switch_subject = previous;
        result
    }

    pub(super) fn switch_subject(&self) -> Result<MirValueId, LowerError> {
        self.switch_subject
            .ok_or_else(|| self.error(self.span(), "switch subject is not available"))
    }
    pub(super) fn panic_context_at(
        &self,
        line: u32,
        source_line_override: Option<&str>,
    ) -> MirPanicContext {
        let source_line = source_line_override
            .map(str::to_owned)
            .or_else(|| {
                self.source_texts
                    .get(&self.function.source_file)
                    .and_then(|source| {
                        source
                            .lines()
                            .nth(line.saturating_sub(1) as usize)
                            .map(str::to_owned)
                    })
            })
            .unwrap_or_default();
        MirPanicContext {
            function: self.function.name.clone(),
            source_line,
            caret: 0,
            locals: Vec::new(),
        }
    }

    pub(super) fn source_file_id_for(&mut self, file: &str) -> MirSourceFileId {
        if let Some(id) = self.source_files.get(file).copied() {
            return id;
        }
        let id = MirSourceFileId(stable_id("mir-source-file", file));
        self.source_files.insert(file.to_string(), id);
        id
    }

    pub(super) fn site_id_for(&self, span: Span, purpose: &str) -> MirSiteId {
        MirSiteId(stable_id(
            "mir-site",
            &format!(
                "{}:{purpose}:{}:{}",
                function_identity(self.function),
                span.start,
                span.end
            ),
        ))
    }

    pub(super) fn type_id_for(&self, key: &str) -> Result<MirTypeId, LowerError> {
        let exact = self
            .type_defs
            .iter()
            .filter(|ty| ty.key == key)
            .collect::<Vec<_>>();
        let matches = if exact.is_empty() {
            self.type_defs
                .iter()
                .filter(|ty| ty.name == key)
                .collect::<Vec<_>>()
        } else {
            exact
        };
        match matches.as_slice() {
            [ty] => Ok(ty.id),
            [] => match self
                .trait_defs
                .iter()
                .filter(|trait_def| trait_def.key == key || trait_def.name == key)
                .count()
            {
                0 => Ok(MirTypeId(stable_id("mir-type", key))),
                1 => Ok(MirTypeId(stable_id("mir-type", &format!("Trait({key})")))),
                _ => Err(self.error(
                    self.span(),
                    format!("ambiguous checked MIR trait key `{key}`"),
                )),
            },
            _ => Err(self.error(
                self.span(),
                format!("ambiguous checked MIR type key `{key}`"),
            )),
        }
    }

    pub(super) fn mir_type(&mut self, ty: &Type) -> Result<MirType, LowerError> {
        let span = self.span();
        let instance = canonical_type_instance(self.type_defs, ty, span)?;
        merge_type_instance(&mut self.type_instances, instance.clone(), span)?;
        Ok(instance)
    }

    pub(super) fn value_source_type(&self, value: MirValueId) -> Result<Type, LowerError> {
        self.values
            .iter()
            .find(|(id, _, _, _)| *id == value)
            .map(|(_, ty, _, _)| mir_type_as_ast(ty))
            .ok_or_else(|| {
                self.error(
                    self.span(),
                    format!("missing checked MIR value type {value:?}"),
                )
            })
    }

    pub(super) fn call_value_access(
        &self,
        value: MirValueId,
        borrowed: bool,
    ) -> Result<MirAccess, LowerError> {
        if borrowed {
            return Ok(MirAccess::Read);
        }
        let ownership = self
            .values
            .iter()
            .rev()
            .find(|(id, _, _, _)| *id == value)
            .map(|(_, _, _, ownership)| ownership)
            .ok_or_else(|| {
                self.error(
                    self.span(),
                    format!("missing checked MIR value ownership {value:?}"),
                )
            })?;
        Ok(
            if matches!(ownership.mode, jet_foundation::MIR::MirOwnershipMode::Copy) {
                MirAccess::Read
            } else {
                MirAccess::Move
            },
        )
    }

    pub(super) fn field_id_for(
        &self,
        owner: jet_foundation::MIR::MirTypeId,
        key: &str,
    ) -> Result<MirFieldId, LowerError> {
        let Some(ty) = self.type_defs.iter().find(|ty| ty.id == owner) else {
            return Err(self.error(
                self.span(),
                format!("missing checked MIR owner type {owner:?}"),
            ));
        };
        let field = match &ty.kind {
            MirTypeDefKind::Struct { fields, .. } => fields.iter().find(|field| field.name == key),
            MirTypeDefKind::Enum { variants, .. } => {
                variants.iter().find_map(|variant| match &variant.payload {
                    jet_foundation::MIR::MirVariantPayload::Named(fields) => {
                        fields.iter().find(|field| field.name == key)
                    }
                    _ => None,
                })
            }
            MirTypeDefKind::Distinct { .. }
            | MirTypeDefKind::Alias { .. }
            | MirTypeDefKind::UnitFamily { .. } => None,
        };
        field.map(|field| field.id).ok_or_else(|| {
            self.error(
                self.span(),
                format!("missing checked MIR field `{key}` on {owner:?}"),
            )
        })
    }

    pub(super) fn field_owner_id_for_type(&self, ty: &Type) -> Result<MirTypeId, LowerError> {
        let ty = field_owner_type(ty);
        let key = match ty {
            Type::Apply { name, .. } => name.clone(),
            _ => type_identity_key(ty),
        };
        self.type_id_for(&key)
    }

    pub(super) fn field_name_for_type(
        &self,
        ty: &Type,
        index: usize,
    ) -> Result<String, LowerError> {
        let ty = field_owner_type(ty);
        if let Type::Tuple(fields) = ty {
            return fields
                .get(index)
                .map(|(name, _)| name.clone())
                .ok_or_else(|| {
                    self.error(
                        self.span(),
                        format!("missing checked tuple field at position {index}"),
                    )
                });
        }
        let owner = self.field_owner_id_for_type(ty)?;
        let definition = self
            .type_defs
            .iter()
            .find(|definition| definition.id == owner)
            .ok_or_else(|| {
                self.error(
                    self.span(),
                    format!("missing checked MIR owner type {owner:?}"),
                )
            })?;
        let fields = match &definition.kind {
            MirTypeDefKind::Struct { fields, .. } => fields,
            MirTypeDefKind::Enum { .. }
            | MirTypeDefKind::Distinct { .. }
            | MirTypeDefKind::Alias { .. }
            | MirTypeDefKind::UnitFamily { .. } => {
                return Err(self.error(
                    self.span(),
                    format!("checked MIR type {owner:?} has no ordered fields"),
                ))
            }
        };
        fields
            .get(index)
            .map(|field| field.name.clone())
            .ok_or_else(|| {
                self.error(
                    self.span(),
                    format!("missing checked MIR field at position {index} on {owner:?}"),
                )
            })
    }

    pub(super) fn field_id_for_type(&self, ty: &Type, key: &str) -> Result<MirFieldId, LowerError> {
        let ty = field_owner_type(ty);
        if let Type::Tuple(fields) = ty {
            if fields.iter().any(|(name, _)| name == key) {
                let identity =
                    canonical_type_instance(self.type_defs, ty, self.span())?.identity_key();
                return Ok(MirFieldId(stable_id(
                    "mir-field",
                    &format!("{identity}::{key}"),
                )));
            }
        }
        let owner = self.field_owner_id_for_type(ty)?;
        self.field_id_for(owner, key)
    }

    fn field_id_for_mir_type(&self, ty: &MirType, key: &str) -> Result<MirFieldId, LowerError> {
        let ty = field_owner_mir_type(ty);
        if let Some(fields) = ty.tuple_fields() {
            if fields.iter().any(|(name, _)| name == key) {
                return Ok(MirFieldId(stable_id(
                    "mir-field",
                    &format!("{}::{key}", ty.identity_key()),
                )));
            }
        }
        let owner = ty.nominal_id().ok_or_else(|| {
            self.error(self.span(), format!("missing MIR field owner for `{key}`"))
        })?;
        self.field_id_for(owner, key)
    }

    pub(super) fn enum_named_payload_field(
        &self,
        owner: &str,
        variant: &str,
        label: &str,
    ) -> Result<(MirFieldId, usize), LowerError> {
        let owner_id = self.type_id_for(owner)?;
        let row = self
            .type_defs
            .iter()
            .find(|row| row.id == owner_id)
            .ok_or_else(|| self.error(self.span(), format!("missing MIR enum owner `{owner}`")))?;
        let MirTypeDefKind::Enum { variants, .. } = &row.kind else {
            return Err(self.error(
                self.span(),
                format!("MIR enum payload owner `{owner}` is not an enum"),
            ));
        };
        let variant_row = variants
            .iter()
            .find(|candidate| candidate.name == variant)
            .ok_or_else(|| {
                self.error(
                    self.span(),
                    format!("missing MIR enum variant `{owner}.{variant}`"),
                )
            })?;
        let MirVariantPayload::Named(fields) = &variant_row.payload else {
            return Err(self.error(
                self.span(),
                format!("MIR enum variant `{owner}.{variant}` has no named payload"),
            ));
        };
        fields
            .iter()
            .enumerate()
            .find(|(_, field)| field.name == label)
            .map(|(order, field)| (field.id, order))
            .ok_or_else(|| {
                self.error(
                    self.span(),
                    format!("missing MIR enum payload field `{owner}.{variant}.{label}`"),
                )
            })
    }

    pub(super) fn shared_guard_field_path(
        &self,
        guard_ty: &Type,
        path: &[String],
    ) -> Result<Vec<MirFieldId>, LowerError> {
        let mut current = field_owner_type(guard_ty).clone();
        let mut fields = Vec::with_capacity(path.len());
        for label in path {
            let field = self.field_id_for_type(&current, label)?;
            let field_ty = self.field_type_for_type(&current, label)?;
            fields.push(field);
            current = field_ty;
        }
        Ok(fields)
    }

    pub(super) fn intern_prelude_call(
        &mut self,
        mut row: MirPreludeCall,
    ) -> Result<MirPreludeCallId, LowerError> {
        let symbol = row.symbol.name();
        if symbol.is_empty() {
            return Err(self.error(self.span(), "checked Prelude row has an empty symbol"));
        }
        let identity = format!(
            "{:?}:{}:{}:{}:{}:{}:{:?}:{:?}:{:?}:{:?}",
            row.family,
            row.module,
            row.member,
            symbol,
            row.signature.arity,
            row.signature.max_arity,
            row.signature.borrow_mask,
            row.effect,
            row.fallibility,
            row.abi
        );

        row.id = MirPreludeCallId(stable_id("mir-prelude-call", &identity));
        if let Some(existing) = self.prelude_calls.iter().find(|call| call.id == row.id) {
            if existing.family != row.family
                || existing.module != row.module
                || existing.member != row.member
                || existing.symbol != row.symbol
                || existing.signature != row.signature
                || existing.effect != row.effect
                || existing.fallibility != row.fallibility
                || existing.abi != row.abi
            {
                return Err(self.error(
                    self.span(),
                    format!("conflicting checked Prelude row {:?}", row.id),
                ));
            }
            return Ok(row.id);
        }
        self.prelude_calls.push(row);
        Ok(self.prelude_calls.last().expect("row just inserted").id)
    }
    pub(super) fn intern_core_route(
        &mut self,
        record: &'static jet_foundation::Syntax::CoreCallRecord,
        carrier: &TFailureCarrier,
    ) -> Result<MirPreludeCallId, LowerError> {
        let symbol = match record.symbol {
            jet_foundation::Syntax::CoreCallSymbol::Prelude(name) => {
                jet_foundation::MIR::MirSymbol::Prelude(name.to_string())
            }
            jet_foundation::Syntax::CoreCallSymbol::Rust(name) => {
                jet_foundation::MIR::MirSymbol::Runtime(name.to_string())
            }
        };
        let fallibility = call_fallibility(self, carrier)?;
        self.intern_prelude_call(MirPreludeCall {
            id: MirPreludeCallId(0),
            family: MirPreludeFamily::StaticPrelude,
            module: if record.module.is_empty() {
                format!("receiver({})", record.receiver_types.join("|"))
            } else {
                record.module.to_string()
            },
            member: record.member.to_string(),
            symbol,
            signature: MirCallSignature {
                arity: record.signature.arity,
                max_arity: record.signature.max_arity,
                borrow_mask: record.signature.borrow_mask.to_vec(),
            },
            effect: record.effect,
            fallibility,
            abi: MirPreludeAbi::Value,
            authority: None,
            db_metadata: None,
        })
    }
    pub(super) fn lower_call_fallibility(
        &mut self,
        carrier: &TFailureCarrier,
    ) -> Result<MirCallFallibility, LowerError> {
        call_fallibility(self, carrier)
    }

    pub(super) fn local_id_for(&self, local: &TLocal) -> Result<MirLocalId, LowerError> {
        self.locals
            .iter()
            .find(|candidate| self.local_places.get(&local.name) == Some(&candidate.place))
            .map(|candidate| candidate.id)
            .ok_or_else(|| {
                self.error(
                    self.span(),
                    format!("missing checked MIR local row `{}`", local.name),
                )
            })
    }

    pub(super) fn intern_prelude_route(
        &mut self,
        route: TPreludeRoute,
    ) -> Result<MirPreludeCallId, LowerError> {
        let fallibility = call_fallibility(self, &route.fallibility)?;
        self.intern_prelude_call(MirPreludeCall {
            id: MirPreludeCallId(0),
            family: route.family,
            module: route.module,
            member: route.member,
            symbol: route.symbol,
            signature: route.signature,
            effect: route.effect,
            fallibility,
            abi: route.abi,
            authority: None,
            db_metadata: None,
        })
    }

    pub(super) fn function_id(&self) -> Result<MirFunctionId, LowerError> {
        if self.function.synthetic {
            return Ok(MirFunctionId(stable_id(
                "mir-function",
                &function_identity(self.function),
            )));
        }
        self.function_registry.id_for(self.function)
    }

    pub(super) fn current_block(&self) -> MirBlockId {
        self.current
    }

    pub(super) fn switch_to(&mut self, block: MirBlockId) {
        self.current = block;
    }

    pub(super) fn block_mut(&mut self, id: MirBlockId) -> Result<&mut MirBasicBlock, LowerError> {
        let index = self.blocks.iter().position(|block| block.id == id);
        match index {
            Some(index) => Ok(&mut self.blocks[index]),
            None => Err(self.error(self.span(), format!("missing MIR block {id:?}"))),
        }
    }

    pub(super) fn new_block(&mut self, span: Span, role: &str) -> Result<MirBlockId, LowerError> {
        let identity = self.reserve_identity("block", span, role, "")?;
        let id = MirBlockId(stable_id("mir-block", &identity));
        self.blocks.push(MirBasicBlock {
            id,
            span,
            instructions: Vec::new(),
            terminator: MirTerminator::Unreachable {
                reason: "lowering in progress".to_string(),
            },
        });
        Ok(id)
    }

    pub(super) fn is_terminated(&self) -> bool {
        !matches!(
            self.blocks
                .iter()
                .find(|block| block.id == self.current)
                .map(|block| &block.terminator),
            Some(MirTerminator::Unreachable { reason }) if reason == "lowering in progress"
        )
    }

    pub(super) fn terminate(&mut self, terminator: MirTerminator) {
        if let Some(block) = self
            .blocks
            .iter_mut()
            .find(|block| block.id == self.current)
        {
            block.terminator = terminator;
        }
    }

    pub(super) fn terminate_with_cleanup(
        &mut self,
        terminator: MirTerminator,
        from_depth: usize,
    ) -> Result<(), LowerError> {
        self.emit_deferred_cleanups(from_depth)?;
        self.terminate(terminator);
        Ok(())
    }

    fn emit_deferred_cleanups(&mut self, from_depth: usize) -> Result<(), LowerError> {
        let from_depth = from_depth.min(self.defer_stack.len());
        let block = self.current.0;
        for scope in (from_depth..self.defer_stack.len()).rev() {
            let deferred = self.defer_stack[scope].clone();
            for (ordinal, thunk) in deferred.into_iter().rev().enumerate() {
                self.emit(
                    &format!("defer.cleanup.{block}.{scope}.{ordinal}"),
                    None,
                    MirOperation::IndirectCall {
                        callee: thunk,
                        args: Vec::new(),
                        type_args: Vec::new(),
                    },
                )?;
            }
        }
        Ok(())
    }

    pub(super) fn register_defer(&mut self, thunk: MirValueId) -> Result<(), LowerError> {
        if self.defer_stack.is_empty() {
            return Err(self.error(self.span(), "defer registered without lexical scope"));
        }
        self.defer_stack
            .last_mut()
            .expect("checked non-empty defer stack")
            .push(thunk);
        Ok(())
    }

    pub(super) fn emit(
        &mut self,
        role: &str,
        ty: Option<Type>,
        operation: MirOperation,
    ) -> Result<MirValueId, LowerError> {
        let value_type = ty.map(|source| self.mir_type(&source)).transpose()?;
        self.emit_mir_type(role, value_type, operation)
    }

    pub(super) fn emit_checked(
        &mut self,
        role: &str,
        ty: Option<&Type>,
        operation: MirOperation,
    ) -> Result<MirValueId, LowerError> {
        let value_type = ty.map(|source| self.mir_type(source)).transpose()?;
        self.emit_mir_type(role, value_type, operation)
    }

    /// Emit a value that is about to cross an owned closure boundary.
    ///
    /// A scalar source is `Copy` in ordinary expressions, but a closure
    /// capture operand is still an owned value consumed by the closure
    /// operation. Keep that boundary explicit so legality verification does
    /// not mistake the capture for a second use of a copy-typed temporary.
    pub(super) fn emit_owned(
        &mut self,
        role: &str,
        ty: Option<Type>,
        operation: MirOperation,
    ) -> Result<MirValueId, LowerError> {
        let value_type = ty.map(|source| self.mir_type(&source)).transpose()?;
        self.emit_mir_type_with_ownership(role, value_type, operation, Some(MirOwnership::Owned))
    }

    fn emit_mir_type_with_ownership(
        &mut self,
        role: &str,
        value_type: Option<MirType>,
        operation: MirOperation,
        forced_ownership: Option<MirOwnership>,
    ) -> Result<MirValueId, LowerError> {
        self.promote_spawn_closure_to_send(&operation)?;
        let span = self.span();
        let source_line = self.current_line;
        let detail = format!("{operation:?}");
        let identity = self.reserve_identity("operation", span, role, &detail)?;
        let value = MirValueId(stable_id("mir-value", &identity));
        let op_id = jet_foundation::MIR::MirOpId(stable_id("mir-op", &identity));
        if let Some(value_type) = &value_type {
            self.values.push((
                value,
                value_type.clone(),
                span,
                forced_ownership
                    .unwrap_or_else(|| self.ownership_for(&mir_type_as_ast(value_type))),
            ));
        }
        if let Some(block) = self
            .blocks
            .iter_mut()
            .find(|block| block.id == self.current)
        {
            block.instructions.push(MirInstruction {
                id: op_id,
                span,
                source_line,
                result: value_type.as_ref().map(|_| value),
                ty: value_type,
                operation,
            });
        }
        Ok(value)
    }

    fn promote_spawn_closure_to_send(
        &mut self,
        operation: &MirOperation,
    ) -> Result<(), LowerError> {
        let Some(closure) = (match operation {
            MirOperation::Semantic(MirSemanticOp::CoreClosureCall {
                kind: MirCoreClosureKind::Spawn,
                closure: Some(closure),
                ..
            }) => Some(*closure),
            _ => None,
        }) else {
            return Ok(());
        };
        let source = self
            .values
            .iter()
            .find(|(value, _, _, _)| *value == closure)
            .map(|(_, ty, _, _)| ty.clone())
            .ok_or_else(|| self.error(self.span(), "spawn closure has no MIR value type"))?;
        let target = match source.kind() {
            MirTypeKind::SendFn { .. } => return Ok(()),
            MirTypeKind::Fn(_) => self.mir_send_fn_type(&mir_type_as_ast(&source))?,
            _ => return Err(self.error(self.span(), "spawn closure is not callable")),
        };
        if let Some((_, ty, _, _)) = self
            .values
            .iter_mut()
            .find(|(value, _, _, _)| *value == closure)
        {
            *ty = target.clone();
        }
        for block in &mut self.blocks {
            for instruction in &mut block.instructions {
                if instruction.result == Some(closure) {
                    instruction.ty = Some(target.clone());
                }
            }
        }
        Ok(())
    }

    pub(super) fn emit_mir_type(
        &mut self,
        role: &str,
        value_type: Option<MirType>,
        operation: MirOperation,
    ) -> Result<MirValueId, LowerError> {
        self.emit_mir_type_with_ownership(role, value_type, operation, None)
    }

    pub(super) fn lower_child(&mut self, expr: &TExpr) -> Result<MirValueId, LowerError> {
        super::tir_to_mir_expr::lower_expr(self, expr)
    }

    pub(super) fn lower_nested_stmts(&mut self, stmts: &[TStmt]) -> Result<(), LowerError> {
        super::tir_to_mir_stmt::lower_stmts(self, stmts)
    }

    pub(super) fn lower_place(
        &mut self,
        place: &TPlace,
        access: MirAccess,
    ) -> Result<MirPlaceId, LowerError> {
        match place {
            TPlace::Local(local) => self.place_for_local(local, access),
            TPlace::Expr(expr) => {
                if let Some(place) =
                    super::tir_to_mir_expr::lower_receiver_place(self, expr, access)?
                {
                    return Ok(place);
                }
                let value = self.lower_child(expr)?;
                let id = self.place_id("temporary", &format!("{}", value.0))?;
                let span = self.span();
                let ty = self.mir_type(&expr.ty)?;
                self.places.push(MirPlace {
                    id,
                    span,
                    ty,
                    base: MirPlaceBase::Temporary(value),
                    projections: Vec::new(),
                    access,
                    persist_key: None,
                });
                Ok(id)
            }
        }
    }

    pub(super) fn panic_location_at(&mut self, line: u32) -> MirPanicLoc {
        let file = self.function.source_file.clone();
        MirPanicLoc {
            file: self.source_file_id_for(&file),
            line,
            column: 0,
        }
    }

    fn index_element_type(&self, base: &Type, kind: MirIndexKind) -> Result<Type, LowerError> {
        match kind {
            MirIndexKind::List | MirIndexKind::FixedListProof => match base {
                Type::List(inner) | Type::FixedList { elem: inner, .. } => Ok((**inner).clone()),
                _ => Err(self.error(self.span(), "checked list index has a non-list base")),
            },
            MirIndexKind::Map => match base {
                Type::Map { value, .. } => Ok((**value).clone()),
                _ => Err(self.error(self.span(), "checked map index has a non-map base")),
            },
            MirIndexKind::Pool => match base.without_user_tags() {
                Type::Apply { args, .. } if args.len() == 1 => Ok(args[0].clone()),
                _ => Err(self.error(self.span(), "checked pool index has no element type")),
            },
            MirIndexKind::Lane => Err(self.error(
                self.span(),
                "checked lane index has no assignable place route",
            )),
        }
    }

    pub(super) fn lower_index_value(
        &mut self,
        base: &TExpr,
        index: &TExpr,
        kind: MirIndexKind,
        result_ty: &Type,
        access: MirAccess,
    ) -> Result<MirValueId, LowerError> {
        let base_value = self.lower_child(base)?;
        let index_value = self.lower_child(index)?;
        self.emit_index_value(base_value, index_value, kind, result_ty, access, "index")
    }

    pub(super) fn emit_index_value(
        &mut self,
        base_value: MirValueId,
        index_value: MirValueId,
        kind: MirIndexKind,
        result_ty: &Type,
        access: MirAccess,
        role: &str,
    ) -> Result<MirValueId, LowerError> {
        let carrier = TFailureCarrier::from_checked_type(result_ty);
        let call =
            self.intern_prelude_route(super::index_route(kind, access, result_ty, &carrier)?)?;
        let line = self.current_line.unwrap_or(self.function.line as u32);
        let location = self.panic_location_at(line);
        let context = self.panic_context_at(line, None);
        self.emit(
            role,
            Some(result_ty.clone()),
            MirOperation::Index {
                call,
                base: base_value,
                index: index_value,
                kind,
                access,
                location,
                context,
            },
        )
    }

    pub(super) fn lower_slice_value(
        &mut self,
        base: &TExpr,
        start: &TExpr,
        end: &TExpr,
        range: Option<&TExpr>,
        result_ty: &Type,
    ) -> Result<MirValueId, LowerError> {
        let base_value = self.lower_child(base)?;
        let start_value = self.lower_child(start)?;
        let end_value = self.lower_child(end)?;
        let range_value = range.map(|expr| self.lower_child(expr)).transpose()?;
        let carrier = TFailureCarrier::from_checked_type(result_ty);
        let call = self.intern_prelude_route(super::slice_route(
            &base.ty,
            range_value.is_some(),
            result_ty,
            &carrier,
        )?)?;
        let location = self.panic_location_at(self.function.line as u32);
        self.emit(
            "slice",
            Some(result_ty.clone()),
            MirOperation::Slice {
                call,
                base: base_value,
                start: start_value,
                end: end_value,
                range: range_value,
                location,
            },
        )
    }

    pub(super) fn lower_index_place(
        &mut self,
        base: &TExpr,
        index: &TExpr,
        kind: MirIndexKind,
        access: MirAccess,
    ) -> Result<MirPlaceId, LowerError> {
        let result_ty = self.index_element_type(&base.ty, kind)?;
        let base_place = if let Some(place) =
            super::tir_to_mir_expr::lower_receiver_place(self, base, access)?
        {
            place
        } else {
            let value = self.lower_child(base)?;
            let id = self.place_id("temporary", &format!("{}", value.0))?;
            let span = self.span();
            let ty = self.mir_type(&base.ty)?;
            self.places.push(MirPlace {
                id,
                span,
                ty,
                base: MirPlaceBase::Temporary(value),
                projections: Vec::new(),
                access,
                persist_key: None,
            });
            id
        };
        let index_value = self.lower_child(index)?;
        let carrier = TFailureCarrier::from_checked_type(&result_ty);
        let call =
            self.intern_prelude_route(super::index_route(kind, access, &result_ty, &carrier)?)?;
        let write_call = if access == MirAccess::Write {
            Some(self.intern_prelude_route(super::index_write_route(kind, &result_ty, &carrier)?)?)
        } else {
            None
        };
        let line = self.current_line.unwrap_or(self.function.line as u32);
        let location = self.panic_location_at(line);
        let context = matches!(kind, MirIndexKind::Pool | MirIndexKind::Map)
            .then(|| self.panic_context_at(line, None));
        let root = self
            .places
            .iter()
            .find(|place| place.id == base_place)
            .cloned()
            .ok_or_else(|| self.error(self.span(), "missing checked index base place"))?;
        let id = self.place_id(
            "index",
            &format!("{}:{}:{}", root.id.0, self.span().start, self.span().end),
        )?;
        let mut projections = root.projections;
        projections.push(MirProjection::Index {
            kind,
            index: index_value,
            call,
            write_call,
            location,
            context,
            span: self.span(),
        });
        let span = self.span();
        let ty = self.mir_type(&result_ty)?;
        self.places.push(MirPlace {
            id,
            span,
            ty,
            base: root.base,
            projections,
            access,
            persist_key: None,
        });
        Ok(id)
    }

    pub(super) fn project_field_place(
        &mut self,
        base: MirPlaceId,
        field: &str,
        field_ty: Type,
        span: Span,
    ) -> Result<MirPlaceId, LowerError> {
        let root = self
            .places
            .iter()
            .find(|place| place.id == base)
            .cloned()
            .ok_or_else(|| self.error(span, "missing checked field base place"))?;
        let field_id = self.field_id_for_mir_type(&root.ty, field)?;
        let id = self.place_id("field", &format!("{}:{field}", root.id.0))?;
        let mut projections = root.projections;
        projections.push(MirProjection::Field {
            field: field_id,
            span,
        });
        let ty = self.mir_type(&field_ty)?;
        self.places.push(MirPlace {
            id,
            span,
            ty,
            base: root.base,
            projections,
            access: root.access,
            persist_key: None,
        });
        Ok(id)
    }

    pub(super) fn lower_local(
        &mut self,
        local: &TLocal,
        ty: &Type,
    ) -> Result<MirValueId, LowerError> {
        let place = self.place_for_local(local, MirAccess::Read)?;
        let value = self.emit(
            "local.read",
            Some(ty.clone()),
            MirOperation::ReadPlace(place),
        )?;
        self.local_values.insert(local.name.clone(), value);
        Ok(value)
    }

    pub(super) fn bind_parameter(
        &mut self,
        index: usize,
        name: &str,
        ty: &Type,
        access: AccessConvention,
    ) -> Result<(), LowerError> {
        let access = lower_mir_convention(access);
        let param_value = self.emit(
            &format!("parameter.{index}.{name}"),
            Some(ty.clone()),
            MirOperation::Parameter {
                index,
                name: name.to_string(),
            },
        )?;
        self.local_types.insert(name.to_string(), ty.clone());
        self.local_values.insert(name.to_string(), param_value);
        let local = TLocal::user(name.to_string());
        let place = self.place_for_local(&local, access)?;
        let mir_ty = self.mir_type(ty)?;
        self.params.push(MirParam {
            index,
            name: name.to_string(),
            span: self.span(),
            ty: mir_ty.clone(),
            access,
            ownership: MirOwnership::from_access(access),
            public_label: name.to_string(),
            variadic: false,
            default_present: false,
        });
        let local_identity = self.reserve_identity("local", self.span(), name, "")?;
        let local_id = MirLocalId(stable_id("mir-local", &local_identity));
        self.locals.push(MirLocal {
            id: local_id,
            name: name.to_string(),
            span: self.span(),
            ty: mir_ty,
            place,
            mutable: matches!(access, MirAccess::Write),
            ownership: MirOwnership::from_access(access),
            comptime: false,
            uninit: false,
            arena_view: false,
            string_view: false,
            gc_root: false,
        });
        if let Some(mir_place) = self
            .places
            .iter_mut()
            .find(|candidate| candidate.id == place)
        {
            mir_place.base = MirPlaceBase::Parameter(param_value);
        }
        Ok(())
    }

    pub(super) fn bind_local(
        &mut self,
        local: &TLocal,
        ty: Type,
        mutable: bool,
        comptime: bool,
        uninit: bool,
    ) -> Result<MirPlaceId, LowerError> {
        self.local_types.insert(local.name.clone(), ty.clone());
        let access = if mutable {
            MirAccess::Write
        } else {
            MirAccess::Read
        };
        let mir_ty = self.mir_type(&ty)?;
        let local_identity = self.reserve_identity("local", self.span(), &local.name, "")?;
        let local_id = MirLocalId(stable_id("mir-local", &local_identity));
        let place = self.place_id("local", &local.name)?;
        self.places.push(MirPlace {
            id: place,
            span: self.span(),
            ty: mir_ty.clone(),
            base: MirPlaceBase::Local(local_id),
            projections: Vec::new(),
            access,
            persist_key: None,
        });
        self.local_places.insert(local.name.clone(), place);
        self.locals.push(MirLocal {
            id: local_id,
            name: local.name.clone(),
            span: self.span(),
            ty: mir_ty,
            place,
            mutable,
            ownership: self.ownership_for(&ty),
            comptime,
            uninit,
            arena_view: false,
            string_view: false,
            gc_root: false,
        });
        Ok(place)
    }
    pub(super) fn bind_data_entries_temp(
        &mut self,
        local: &TLocal,
        owner: MirTypeId,
        variant: &str,
    ) -> Result<MirLocalId, LowerError> {
        if let Some(local_id) = self
            .locals
            .iter()
            .rfind(|candidate| candidate.name == local.name)
            .map(|candidate| candidate.id)
        {
            return Ok(local_id);
        }
        let payload_ty = self.variant_payload_type(owner, variant, 0)?;
        self.bind_local(local, payload_ty, true, false, true)?;
        self.local_id_for(local)
    }

    pub(super) fn bind_named_local(
        &mut self,
        name: &str,
        ty: Type,
        mutable: bool,
    ) -> Result<MirPlaceId, LowerError> {
        let local = TLocal::user(name.to_string());
        self.bind_local(&local, ty, mutable, false, false)
    }
    /// Lower a checked pattern into structural MIR tests and explicit payload
    /// projections.  The result is always a typed boolean; bindings are writes
    /// to ordinary MIR locals, not hidden backend pattern syntax.
    pub(super) fn lower_pattern_condition(
        &mut self,
        subject: MirValueId,
        pattern: &MirPattern,
    ) -> Result<MirValueId, LowerError> {
        let subject_ty = self.value_source_type(subject)?;
        // Object patterns bind their ordered payload to `temp`; the public
        // map binding is a later then-body declaration.  Suppress the generic
        // variant binder here so both writes do not alias one local place with
        // incompatible list/map types.
        let shape = match &pattern.position {
            MirPatternPosition::DataEntries { .. } => match &pattern.shape {
                MirPatternShape::Variant {
                    variant,
                    leading_dot,
                    span,
                    ..
                } => MirPatternShape::Variant {
                    variant: variant.clone(),
                    bindings: vec![MirPatternBinding::Wildcard],
                    leading_dot: *leading_dot,
                    span: *span,
                },
                _ => pattern.shape.clone(),
            },
            _ => pattern.shape.clone(),
        };
        let condition = self.lower_pattern_shape_condition(
            subject,
            &subject_ty,
            pattern.owner,
            &shape,
            pattern.mutable,
            false,
        )?;
        if let MirPatternPosition::DataEntries { temp } = &pattern.position {
            if let MirPatternShape::Variant { variant, .. } = &pattern.shape {
                let owner = pattern.owner.ok_or_else(|| {
                    self.error(
                        self.span(),
                        "checked data-entry pattern is missing its owner",
                    )
                })?;
                return self.lower_guarded_pattern(condition, |ctx| {
                    let payload_ty = ctx.variant_payload_type(owner, variant, 0)?;
                    let payload = ctx.emit_checked(
                        "pattern",
                        Some(&payload_ty),
                        MirOperation::EnumPayload {
                            subject,
                            owner,
                            variant: variant.clone(),
                            index: 0,
                        },
                    )?;
                    ctx.write_local_id(*temp, payload)?;
                    Ok(condition)
                });
            }
        }
        Ok(condition)
    }

    fn lower_pattern_shape_condition(
        &mut self,
        subject: MirValueId,
        subject_ty: &Type,
        owner: Option<MirTypeId>,
        shape: &MirPatternShape,
        mutable: bool,
        reuse_bindings: bool,
    ) -> Result<MirValueId, LowerError> {
        match shape {
            MirPatternShape::Variant {
                variant, bindings, ..
            } => {
                let owner = owner.ok_or_else(|| {
                    self.error(self.span(), "checked variant pattern is missing its owner")
                })?;
                let test = self.emit_checked(
                    "pattern",
                    Some(&Type::Bool),
                    MirOperation::EnumIs {
                        subject,
                        owner,
                        variant: variant.clone(),
                    },
                )?;
                if bindings
                    .iter()
                    .all(|binding| matches!(binding, MirPatternBinding::Wildcard))
                {
                    return Ok(test);
                }
                self.lower_guarded_pattern(test, |ctx| {
                    let mut tests = vec![test];
                    for (index, binding) in bindings.iter().enumerate() {
                        match binding {
                            MirPatternBinding::Wildcard => {}
                            MirPatternBinding::Bind { name, .. } => {
                                let ty = ctx.variant_payload_type(owner, variant, index)?;
                                let value = ctx.emit_checked(
                                    "pattern",
                                    Some(&ty),
                                    MirOperation::EnumPayload {
                                        subject,
                                        owner,
                                        variant: variant.clone(),
                                        index,
                                    },
                                )?;
                                ctx.bind_pattern_value(name, ty, value, mutable, reuse_bindings)?;
                            }
                            MirPatternBinding::Range { lo, hi } => {
                                let ty = ctx.variant_payload_type(owner, variant, index)?;
                                let value = ctx.emit_checked(
                                    "pattern",
                                    Some(&ty),
                                    MirOperation::EnumPayload {
                                        subject,
                                        owner,
                                        variant: variant.clone(),
                                        index,
                                    },
                                )?;
                                tests.push(ctx.lower_numeric_range(value, &ty, *lo, *hi)?);
                            }
                        }
                    }
                    ctx.combine_pattern_tests(tests, jet_foundation::AST::BinOp::And)
                })
            }
            MirPatternShape::Present { binding, .. } => {
                let inner = match subject_ty {
                    Type::Option(inner) => (**inner).clone(),
                    _ => {
                        return Err(self.error(
                            self.span(),
                            "checked present pattern has a non-optional subject",
                        ))
                    }
                };
                let test = self.emit_checked(
                    "pattern",
                    Some(&Type::Bool),
                    MirOperation::OptionIsSome { subject },
                )?;
                if binding.is_empty() || binding == "_" {
                    return Ok(test);
                }
                self.lower_guarded_pattern(test, |ctx| {
                    let value = ctx.emit_checked(
                        "pattern",
                        Some(&inner),
                        MirOperation::OptionValue { subject },
                    )?;
                    ctx.bind_pattern_value(binding, inner, value, mutable, reuse_bindings)?;
                    Ok(test)
                })
            }
            MirPatternShape::Absent(_) => {
                let present = self.emit_checked(
                    "pattern",
                    Some(&Type::Bool),
                    MirOperation::OptionIsSome { subject },
                )?;
                self.emit_checked(
                    "pattern",
                    Some(&Type::Bool),
                    MirOperation::Unary {
                        op: super::mir_unary_op(jet_foundation::AST::UnOp::Not),
                        value: present,
                    },
                )
            }
            MirPatternShape::Ok { binding, .. } | MirPatternShape::Err { binding, .. } => {
                let ok = matches!(shape, MirPatternShape::Ok { .. });
                let (success, error) = match subject_ty {
                    Type::Result { ok, err } => ((**ok).clone(), (**err).clone()),
                    _ => {
                        return Err(self.error(
                            self.span(),
                            "checked result pattern has a non-result subject",
                        ))
                    }
                };
                let test = self.emit_checked(
                    "pattern",
                    Some(&Type::Bool),
                    MirOperation::ResultIsOk { subject },
                )?;
                let test = if ok {
                    test
                } else {
                    self.emit_checked(
                        "pattern",
                        Some(&Type::Bool),
                        MirOperation::Unary {
                            op: super::mir_unary_op(jet_foundation::AST::UnOp::Not),
                            value: test,
                        },
                    )?
                };
                if binding.is_empty() || binding == "_" {
                    return Ok(test);
                }
                self.lower_guarded_pattern(test, |ctx| {
                    let value_ty = if ok { success } else { error };
                    let value = ctx.emit_checked(
                        "pattern",
                        Some(&value_ty),
                        MirOperation::ResultValue { subject, ok },
                    )?;
                    ctx.bind_pattern_value(binding, value_ty, value, mutable, reuse_bindings)?;
                    Ok(test)
                })
            }
            MirPatternShape::Range { lo, hi, .. } => {
                self.lower_numeric_range(subject, subject_ty, *lo, *hi)
            }
            MirPatternShape::Or { alternatives, .. } => {
                let mut tests = Vec::with_capacity(alternatives.len());
                for (index, alternative) in alternatives.iter().enumerate() {
                    tests.push(self.lower_pattern_shape_condition(
                        subject,
                        subject_ty,
                        owner,
                        alternative,
                        mutable,
                        reuse_bindings || index > 0,
                    )?);
                }
                self.combine_pattern_tests(tests, jet_foundation::AST::BinOp::Or)
            }
            MirPatternShape::Struct { fields, .. } => {
                let owner = owner.ok_or_else(|| {
                    self.error(self.span(), "checked struct pattern is missing its owner")
                })?;
                let mut tests = vec![self.emit_checked(
                    "pattern",
                    Some(&Type::Bool),
                    MirOperation::Constant(MirConstant::Bool(true)),
                )?];
                for field in fields {
                    let (field_id, value_ty, binding) = match field {
                        MirPatternField::Bind { field, local, .. } => (
                            *field,
                            self.field_type_for_id(owner, *field)?,
                            Some(local.as_str()),
                        ),
                        MirPatternField::Value {
                            field, value: _, ..
                        } => (*field, self.field_type_for_id(owner, *field)?, None),
                    };
                    let projected = self.emit_checked(
                        "pattern",
                        Some(&value_ty),
                        MirOperation::Field {
                            base: subject,
                            field: field_id,
                        },
                    )?;
                    if let Some(name) = binding {
                        self.bind_pattern_value(
                            name,
                            value_ty,
                            projected,
                            mutable,
                            reuse_bindings,
                        )?;
                    } else if let MirPatternField::Value { value, .. } = field {
                        tests.push(self.emit_checked(
                            "pattern",
                            Some(&Type::Bool),
                            MirOperation::Binary {
                                op: super::mir_binary_op(jet_foundation::AST::BinOp::Eq),
                                dispatch: MirBinaryDispatch::Primitive,
                                left: projected,
                                right: *value,
                            },
                        )?);
                    }
                }
                self.combine_pattern_tests(tests, jet_foundation::AST::BinOp::And)
            }
            MirPatternShape::Text(parts, _) => self.lower_text_pattern(subject, subject_ty, parts),
            MirPatternShape::Binary(parts, _) => {
                self.lower_binary_pattern(subject, subject_ty, parts)
            }
        }
    }

    fn lower_guarded_pattern(
        &mut self,
        test: MirValueId,
        matched: impl FnOnce(&mut Self) -> Result<MirValueId, LowerError>,
    ) -> Result<MirValueId, LowerError> {
        let unmatched_from = self.current_block();
        let matched_block = self.new_block(self.span(), "pattern-matched")?;
        let join = self.new_block(self.span(), "pattern-join")?;
        self.terminate(MirTerminator::Branch {
            condition: test,
            then_target: matched_block,
            else_target: join,
        });
        self.switch_to(matched_block);
        let result = matched(self)?;
        let matched_from = self.current_block();
        self.terminate(MirTerminator::Jump { target: join });
        self.switch_to(join);
        self.emit_checked(
            "pattern-phi",
            Some(&Type::Bool),
            MirOperation::Phi {
                incoming: vec![(unmatched_from, test), (matched_from, result)],
            },
        )
    }

    fn lower_text_pattern(
        &mut self,
        _subject: MirValueId,
        _subject_ty: &Type,
        _parts: &[MirTextPatternPart],
    ) -> Result<MirValueId, LowerError> {
        Err(self.error(self.span(), "checked text pattern route is not registered"))
    }

    fn lower_binary_pattern(
        &mut self,
        _subject: MirValueId,
        _subject_ty: &Type,
        _parts: &[MirBinaryPatternPart],
    ) -> Result<MirValueId, LowerError> {
        Err(self.error(
            self.span(),
            "checked binary pattern route is not registered",
        ))
    }

    fn combine_pattern_tests(
        &mut self,
        mut tests: Vec<MirValueId>,
        op: jet_foundation::AST::BinOp,
    ) -> Result<MirValueId, LowerError> {
        let Some(mut result) = tests.pop() else {
            return self.emit_checked(
                "pattern",
                Some(&Type::Bool),
                MirOperation::Constant(MirConstant::Bool(true)),
            );
        };
        while let Some(next) = tests.pop() {
            result = self.emit_checked(
                "pattern",
                Some(&Type::Bool),
                MirOperation::Binary {
                    op: super::mir_binary_op(op),
                    dispatch: MirBinaryDispatch::Primitive,
                    left: result,
                    right: next,
                },
            )?;
        }
        Ok(result)
    }

    fn lower_numeric_range(
        &mut self,
        subject: MirValueId,
        ty: &Type,
        lo: i64,
        hi: i64,
    ) -> Result<MirValueId, LowerError> {
        let lower = self.emit_checked(
            "pattern",
            Some(ty),
            MirOperation::Constant(MirConstant::Int {
                value: lo,
                width: int_width(ty),
                spelling: None,
            }),
        )?;
        let upper = self.emit_checked(
            "pattern",
            Some(ty),
            MirOperation::Constant(MirConstant::Int {
                value: hi,
                width: int_width(ty),
                spelling: None,
            }),
        )?;
        let ge = self.emit_checked(
            "pattern",
            Some(&Type::Bool),
            MirOperation::Binary {
                op: super::mir_binary_op(jet_foundation::AST::BinOp::Ge),
                dispatch: MirBinaryDispatch::Primitive,
                left: subject,
                right: lower,
            },
        )?;
        let le = self.emit_checked(
            "pattern",
            Some(&Type::Bool),
            MirOperation::Binary {
                op: super::mir_binary_op(jet_foundation::AST::BinOp::Le),
                dispatch: MirBinaryDispatch::Primitive,
                left: subject,
                right: upper,
            },
        )?;
        self.emit_checked(
            "pattern",
            Some(&Type::Bool),
            MirOperation::Binary {
                op: super::mir_binary_op(jet_foundation::AST::BinOp::And),
                dispatch: MirBinaryDispatch::Primitive,
                left: ge,
                right: le,
            },
        )
    }

    fn bind_pattern_value(
        &mut self,
        name: &str,
        ty: Type,
        value: MirValueId,
        mutable: bool,
        reuse_binding: bool,
    ) -> Result<(), LowerError> {
        if name.is_empty() || name == "_" {
            return Ok(());
        }
        let place = if reuse_binding {
            self.local_places.get(name).copied().ok_or_else(|| {
                self.error(
                    self.span(),
                    format!("missing checked alternative binding `{name}`"),
                )
            })?
        } else {
            self.bind_named_local(name, ty, mutable)?
        };
        self.emit_checked("pattern", None, MirOperation::WritePlace { place, value })?;
        Ok(())
    }

    fn write_local_id(&mut self, local: MirLocalId, value: MirValueId) -> Result<(), LowerError> {
        let place = self
            .locals
            .iter()
            .find(|candidate| candidate.id == local)
            .map(|candidate| candidate.place)
            .ok_or_else(|| {
                self.error(self.span(), format!("missing MIR pattern local {local:?}"))
            })?;
        self.emit_checked("pattern", None, MirOperation::WritePlace { place, value })?;
        Ok(())
    }

    fn variant_payload_type(
        &self,
        owner: MirTypeId,
        variant: &str,
        index: usize,
    ) -> Result<Type, LowerError> {
        let row = self
            .type_defs
            .iter()
            .find(|row| row.id == owner)
            .ok_or_else(|| self.error(self.span(), format!("missing MIR owner {owner:?}")))?;
        let MirTypeDefKind::Enum { variants, .. } = &row.kind else {
            return Err(self.error(self.span(), format!("MIR owner {owner:?} is not an enum")));
        };
        let variant = variants
            .iter()
            .find(|candidate| candidate.name == variant)
            .ok_or_else(|| {
                self.error(self.span(), format!("missing MIR enum variant `{variant}`"))
            })?;
        match &variant.payload {
            MirVariantPayload::Unit => Err(self.error(
                self.span(),
                format!("unit MIR variant `{}` has no payload {index}", variant.name),
            )),
            MirVariantPayload::Single(ty) if index == 0 => Ok(mir_type_as_ast(ty)),
            MirVariantPayload::Single(_) => Err(self.error(
                self.span(),
                format!(
                    "single MIR variant `{}` has no payload {index}",
                    variant.name
                ),
            )),
            MirVariantPayload::Named(fields) => fields
                .get(index)
                .map(|field| mir_type_as_ast(&field.ty))
                .ok_or_else(|| {
                    self.error(
                        self.span(),
                        format!("MIR variant `{}` has no payload {index}", variant.name),
                    )
                }),
        }
    }

    fn field_type_for_id(&self, owner: MirTypeId, field: MirFieldId) -> Result<Type, LowerError> {
        let row = self
            .type_defs
            .iter()
            .find(|row| row.id == owner)
            .ok_or_else(|| self.error(self.span(), format!("missing MIR owner {owner:?}")))?;
        let fields = match &row.kind {
            MirTypeDefKind::Struct { fields, .. } => fields,
            MirTypeDefKind::Enum { variants, .. } => {
                for variant in variants {
                    if let MirVariantPayload::Named(fields) = &variant.payload {
                        if let Some(found) = fields.iter().find(|candidate| candidate.id == field) {
                            return Ok(mir_type_as_ast(&found.ty));
                        }
                    }
                }
                return Err(self.error(self.span(), format!("missing MIR field {field:?}")));
            }
            _ => return Err(self.error(self.span(), format!("MIR owner {owner:?} has no fields"))),
        };
        fields
            .iter()
            .find(|candidate| candidate.id == field)
            .map(|field| mir_type_as_ast(&field.ty))
            .ok_or_else(|| self.error(self.span(), format!("missing MIR field {field:?}")))
    }

    pub(super) fn field_type(&self, ty: &Type, field: &str) -> Option<Type> {
        match ty {
            Type::Tuple(fields) => fields
                .iter()
                .find(|(name, _)| name == field)
                .map(|(_, ty)| (**ty).clone()),
            _ => None,
        }
    }

    fn field_type_for_type(&self, ty: &Type, field: &str) -> Result<Type, LowerError> {
        let ty = field_owner_type(ty);
        if let Some(field_ty) = self.field_type(ty, field) {
            return Ok(field_ty);
        }
        let owner = self.field_owner_id_for_type(ty)?;
        let field_id = self.field_id_for(owner, field)?;
        let field_ty = self.field_type_for_id(owner, field_id)?;
        if let Type::Apply { args, .. } = ty {
            if let Some(definition) = self
                .type_defs
                .iter()
                .find(|definition| definition.id == owner)
            {
                let substitutions = definition
                    .generic_params
                    .iter()
                    .zip(args)
                    .map(|(param, arg)| (param.name.clone(), arg.clone()))
                    .collect();
                return Ok(crate::Generics::substitute_type(&field_ty, &substitutions));
            }
        }
        Ok(field_ty)
    }

    pub(super) fn checked_field_type(&self, ty: &Type, field: &str) -> Result<Type, LowerError> {
        self.field_type_for_type(ty, field)
    }

    pub(super) fn place_for_local(
        &mut self,
        local: &TLocal,
        access: MirAccess,
    ) -> Result<MirPlaceId, LowerError> {
        if let Some(id) = self.local_places.get(&local.name).copied() {
            if let Some(place) = self.places.iter_mut().find(|place| place.id == id) {
                retain_place_access(place, access);
            }
            return Ok(id);
        }
        let Some(ty) = self.local_types.get(&local.name).cloned().or_else(|| {
            self.params
                .iter()
                .find(|param| param.name == local.name)
                .map(|param| mir_type_as_ast(&param.ty))
        }) else {
            return Err(self.error(self.span(), format!("unbound TIR local `{}`", local.name)));
        };
        let id = self.place_id("local", &local.name)?;
        let base = if let Some(value) = self.capture_values.get(&local.name).copied() {
            MirPlaceBase::Capture(value)
        } else if let Some(local_id) = self
            .locals
            .iter()
            .rfind(|candidate| candidate.name == local.name)
            .map(|candidate| candidate.id)
        {
            MirPlaceBase::Local(local_id)
        } else if let Some(value) = self.local_values.get(&local.name).copied() {
            MirPlaceBase::Parameter(value)
        } else {
            return Err(self.error(
                self.span(),
                format!("missing MIR local identity `{}`", local.name),
            ));
        };
        let span = self.span();
        let mir_ty = self.mir_type(&ty)?;
        self.places.push(MirPlace {
            id,
            span,
            ty: mir_ty,
            base,
            projections: Vec::new(),
            access,
            persist_key: None,
        });
        self.local_places.insert(local.name.clone(), id);
        Ok(id)
    }

    fn place_id(&mut self, kind: &str, name: &str) -> Result<MirPlaceId, LowerError> {
        let identity = self.reserve_identity("place", self.span(), kind, name)?;
        Ok(MirPlaceId(stable_id("mir-place", &identity)))
    }

    fn bind_captures(&mut self, lambda: &TLambda) -> Result<(), LowerError> {
        let facts = self.capture_facts_for(lambda);
        self.bind_capture_specs(&lambda.captures, facts, lambda.source_span)
    }

    fn bind_capture_specs(
        &mut self,
        captures: &[(String, String, Type)],
        facts: MirCaptureFacts,
        span: Span,
    ) -> Result<(), LowerError> {
        self.capture_facts = Some(facts.clone());
        let mut seen = HashSet::new();
        for (slot, (source, runtime, ty)) in captures.iter().enumerate() {
            if source.is_empty()
                || runtime.is_empty()
                || !seen.insert(source.clone())
                || (runtime != source && !seen.insert(runtime.clone()))
            {
                return Err(self.error(
                    span,
                    format!("invalid or duplicate checked lambda capture `{source}`"),
                ));
            }
            // A clone/materialization owns the closure environment once; it
            // does not consume that environment on every callback invocation.
            // Only a sema-proven consuming/resource capture is a per-call move.
            let moved = facts.moved.contains(source);
            let cloned =
                !moved && (facts.cloned.contains(source) || facts.materialized.contains(source));
            let access = if moved {
                MirAccess::Move
            } else if facts.mutable.contains(source) {
                MirAccess::Write
            } else {
                MirAccess::Read
            };
            let ownership = if moved || cloned {
                MirOwnership::Owned
            } else {
                MirOwnership::from_access(access)
            };
            let capture = self.emit(
                &format!("capture.{slot}.{source}"),
                Some(ty.clone()),
                MirOperation::Capture { slot },
            )?;
            let mir_ty = self.mir_type(ty)?;
            self.capture_params.push(MirCaptureParam {
                slot,
                name: source.clone(),
                span,
                ty: mir_ty.clone(),
                access,
                ownership: ownership.clone(),
            });
            let names: &[&str] = if runtime == source {
                &[source.as_str()]
            } else {
                &[source.as_str(), runtime.as_str()]
            };
            for name in names {
                self.local_types.insert((*name).to_string(), ty.clone());
                self.local_values.insert((*name).to_string(), capture);
                self.capture_values.insert((*name).to_string(), capture);
                let local = if *name == runtime && runtime != source {
                    TLocal::generated(*name)
                } else {
                    TLocal::user(*name)
                };
                let place = self.place_for_local(&local, access)?;
                let local_identity = self.reserve_identity("local", span, name, "")?;
                let local_id = MirLocalId(stable_id("mir-local", &local_identity));
                self.locals.push(MirLocal {
                    id: local_id,
                    name: (*name).to_string(),
                    span,
                    ty: mir_ty.clone(),
                    place,
                    mutable: matches!(access, MirAccess::Write),
                    ownership: ownership.clone(),
                    comptime: false,
                    uninit: false,
                    arena_view: false,
                    string_view: false,
                    gc_root: false,
                });
            }
        }
        Ok(())
    }

    pub(super) fn lower_lambda(&mut self, lambda: &TLambda) -> Result<MirValueId, LowerError> {
        self.lower_lambda_kind(lambda, false, None)
    }

    pub(super) fn lower_synthetic_lambda(
        &mut self,
        lambda: &TLambda,
        purpose: &str,
    ) -> Result<MirValueId, LowerError> {
        let identity = format!("{purpose}-{}", self.nested_functions.len());
        self.lower_lambda_kind(lambda, false, Some(&identity))
    }

    fn lower_lambda_kind(
        &mut self,
        lambda: &TLambda,
        send: bool,
        synthetic_identity: Option<&str>,
    ) -> Result<MirValueId, LowerError> {
        let start = lambda.source_span.start;
        let end = lambda.source_span.end;
        let mut key = format!("{}::lambda@{start}..{end}", self.function.key);
        let mut name = format!("__jet_lambda_{start}_{end}");
        if let Some(identity) = synthetic_identity {
            key.push_str("::");
            key.push_str(identity);
            name.push('_');
            name.push_str(identity);
        }
        let function = TFunc {
            name,
            module: self.function.module.clone(),
            key,
            source_file: self.function.source_file.clone(),
            source_span: lambda.source_span,
            failure_carrier: lambda.failure_carrier.clone(),
            effects: lambda.effects.clone(),
            target_applicability: TTargetApplicability {
                rust_aot: true,
                cranelift: true,
                interpreter: true,
                web: true,
            },
            web_bucket: self.function.web_bucket,
            web_marker: None,
            visibility: TVisibility::Private,
            foreign: None,
            params: lambda
                .source_params
                .iter()
                .cloned()
                .zip(lambda.param_types.iter().cloned())
                .map(|(name, ty)| (name, ty, AccessConvention::Read))
                .collect(),
            web_param_reconstructions: Vec::new(),
            ret: lambda.ret.clone(),
            gc_return: false,
            return_view_provenance: None,
            generic_params: Vec::new(),
            clone_types: Vec::new(),
            is_main: false,
            line: self.function.line,
            synthetic: true,
            is_unsafe: false,
            unsafe_gate: None,
            is_pure: lambda.effects.direct.is_empty(),
            memo_bound: None,
            is_reactive: false,
            reactive_upgrades: Vec::new(),
            is_inline: false,
            is_inline_always: false,
            is_scalar: false,
            kernel_proof: None,
            memo_field: None,
            uses_stack_sentry: lambda.uses_stack_sentry,
            body: Vec::new(),
            kind: TFuncKind::TopLevel,
            gc_scope: false,
        };
        let (nested, calls, files, instances, deeper, callbacks) = lower_function(
            &function,
            self.type_defs,
            self.nominal_identities,
            self.trait_defs,
            self.function_registry,
            self.source_texts,
            Some(&lambda.executable),
            None,
            Some(lambda),
            None,
        )?;
        self.nested_functions.push(nested);
        self.nested_functions.extend(deeper);
        self.prelude_calls.extend(calls);
        self.callbacks.extend(callbacks);
        self.type_instances.extend(instances);
        for file in files {
            self.source_files.entry(file.path).or_insert(file.id);
        }

        let facts = self.capture_facts_for(lambda);
        let mut captures = Vec::with_capacity(lambda.captures.len());
        for (slot, (source, runtime, ty)) in lambda.captures.iter().enumerate() {
            // Cloned/materialized captures are owned by the closure environment,
            // but reusable callbacks borrow that environment on each call.
            // Keep Move exclusively for sema-proven consuming captures.
            let moved = facts.moved.contains(source);
            let cloned =
                !moved && (facts.cloned.contains(source) || facts.materialized.contains(source));
            let access = if moved {
                MirAccess::Move
            } else if facts.mutable.contains(source) {
                MirAccess::Write
            } else {
                MirAccess::Read
            };
            let borrowed = matches!(access, MirAccess::Read | MirAccess::Write) && !cloned;
            // `runtime` is the canonical TIR slot used by the lambda body.
            // Only a cloned/materialized capture has a fresh runtime slot;
            // those values are read from the enclosing source slot. Borrowed
            // and moved captures must resolve the exact checked runtime slot,
            // which may carry a generated spelling.
            let outer_slot = if cloned && runtime != source {
                source
            } else {
                runtime
            };
            let outer_access = if cloned { MirAccess::Read } else { access };
            let place = self.place_for_local(&TLocal::user(outer_slot), outer_access)?;
            if borrowed {
                captures.push(MirCaptureOperand::Place(place));
                continue;
            }
            let value = if cloned {
                self.emit_owned(
                    &format!("closure.capture.{slot}.clone"),
                    Some(ty.clone()),
                    MirOperation::ReadPlace(place),
                )?
            } else if matches!(access, MirAccess::Move) {
                self.emit_owned(
                    &format!("closure.capture.{slot}.move"),
                    Some(ty.clone()),
                    MirOperation::MovePlace { place },
                )?
            } else {
                self.emit(
                    &format!("closure.capture.{slot}.read"),
                    Some(ty.clone()),
                    MirOperation::ReadPlace(place),
                )?
            };
            captures.push(MirCaptureOperand::Value(value));
        }
        let function_id = self
            .nested_functions
            .iter()
            .find(|candidate| candidate.key == function.key)
            .map(|candidate| candidate.id)
            .ok_or_else(|| self.error(lambda.source_span, "missing lowered lambda function"))?;
        let closure_ty = Type::Fn {
            params: lambda.param_types.clone(),
            ret: lambda.ret.clone().map(Box::new),
            effect_bound: None,
            param_contract: None,
            call_metadata: None,
            return_view_provenance: None,
        };
        let role = format!("closure.lambda.{start}.{end}");
        if send {
            let target = self.mir_send_fn_type(&closure_ty)?;
            self.emit_mir_type(
                &role,
                Some(target),
                MirOperation::Closure {
                    function: function_id,
                    captures,
                    facts,
                },
            )
        } else {
            self.emit(
                &role,
                Some(closure_ty),
                MirOperation::Closure {
                    function: function_id,
                    captures,
                    facts,
                },
            )
        }
    }

    pub(super) fn lower_defer_thunk(
        &mut self,
        close: &TExpr,
        resource: &str,
        id: impl std::fmt::Display,
    ) -> Result<MirValueId, LowerError> {
        let resource_ty = self.local_types.get(resource).cloned().ok_or_else(|| {
            self.error(
                self.span(),
                format!("unbound deferred resource `{resource}`"),
            )
        })?;
        let key = format!("{}::defer@{id}", self.function.key);
        let function = TFunc {
            name: format!("__jet_defer_{id}"),
            module: self.function.module.clone(),
            key,
            source_file: self.function.source_file.clone(),
            source_span: self.span(),
            failure_carrier: TFailureCarrier::from_checked_type(&close.ty),
            effects: self.function.effects.clone(),
            target_applicability: TTargetApplicability {
                rust_aot: true,
                cranelift: true,
                interpreter: true,
                web: true,
            },
            web_bucket: self.function.web_bucket,
            web_marker: None,
            visibility: TVisibility::Private,
            foreign: None,
            params: Vec::new(),
            web_param_reconstructions: Vec::new(),
            ret: Some(close.ty.clone()),
            gc_return: false,
            return_view_provenance: None,
            generic_params: Vec::new(),
            clone_types: Vec::new(),
            is_main: false,
            line: self.function.line,
            synthetic: true,
            is_unsafe: false,
            unsafe_gate: None,
            is_pure: false,
            memo_bound: None,
            is_reactive: false,
            reactive_upgrades: Vec::new(),
            is_inline: false,
            is_inline_always: false,
            is_scalar: false,
            kernel_proof: None,
            memo_field: None,
            uses_stack_sentry: false,
            body: Vec::new(),
            kind: TFuncKind::TopLevel,
            gc_scope: false,
        };
        let captures = vec![(resource.to_string(), resource.to_string(), resource_ty)];
        let (nested, calls, files, instances, deeper, callbacks) = lower_function(
            &function,
            self.type_defs,
            self.nominal_identities,
            self.trait_defs,
            self.function_registry,
            self.source_texts,
            None,
            Some(close),
            None,
            Some((&captures, MirCaptureFacts::default())),
        )?;
        let function_key = function.key.clone();
        self.nested_functions.push(nested);
        self.nested_functions.extend(deeper);
        self.prelude_calls.extend(calls);
        self.callbacks.extend(callbacks);
        self.type_instances.extend(instances);
        for file in files {
            self.source_files.entry(file.path).or_insert(file.id);
        }
        let function_id = self
            .nested_functions
            .iter()
            .find(|candidate| candidate.key == function_key)
            .map(|candidate| candidate.id)
            .ok_or_else(|| self.error(self.span(), "missing lowered defer thunk"))?;
        let place = self.place_for_local(&TLocal::user(resource), MirAccess::Read)?;
        let closure_ty = Type::Fn {
            params: Vec::new(),
            ret: Some(Box::new(close.ty.clone())),
            effect_bound: None,
            param_contract: None,
            call_metadata: None,
            return_view_provenance: None,
        };
        self.emit(
            &format!("defer.thunk.{id}"),
            Some(closure_ty),
            MirOperation::Closure {
                function: function_id,
                captures: vec![MirCaptureOperand::Place(place)],
                facts: MirCaptureFacts::default(),
            },
        )
    }
    pub(super) fn lower_index_hook_failure(&mut self, line: usize) -> Result<(), LowerError> {
        let never = Type::Named(crate::Syntax::TYPE_NEVER.to_string());
        let route = super::index_miss_route(
            &never,
            &TFailureCarrier::Diverges {
                value: never.clone(),
            },
        )?;
        let call = self.intern_prelude_route(route)?;
        let file = self.emit(
            "index-miss.file",
            Some(Type::String),
            MirOperation::Constant(MirConstant::String(self.function.source_file.clone())),
        )?;
        let line = self.emit(
            "index-miss.line",
            Some(Type::Int),
            MirOperation::Constant(MirConstant::Int {
                value: line as i64,
                width: None,
                spelling: None,
            }),
        )?;
        let message = self.emit(
            "index-miss.message",
            Some(Type::String),
            MirOperation::Constant(MirConstant::String("index miss".to_string())),
        )?;
        let span = self.span();
        let args = [file, line, message]
            .into_iter()
            .enumerate()
            .map(|(source_index, value)| MirCallArg {
                value,
                access: if source_index == 1 {
                    MirAccess::Read
                } else {
                    MirAccess::Read
                },
                span,
                label: None,
                source_index: Some(source_index),
                binder_slot: None,
                spread: false,
                implicit_clone: false,
                shared_auto_clone: false,
                owned_last_use: false,
                authority_boundary: false,
                fn_coercion: None,
                widen_fixed_to_list: false,
                widen_to_union: None,
                box_as_trait: None,
                place: None,
            })
            .collect::<Vec<_>>();
        self.emit_deferred_cleanups(0)?;
        self.emit(
            &format!("index-miss.panic.{}", self.current.0),
            Some(never),
            MirOperation::Call {
                callee: MirCallee::Prelude(call),
                args,
                type_args: Vec::new(),
            },
        )?;
        self.terminate(MirTerminator::Unreachable {
            reason: "index miss".to_string(),
        });
        Ok(())
    }
    fn capture_facts_for(&self, lambda: &TLambda) -> MirCaptureFacts {
        MirCaptureFacts {
            escapes: lambda.capture_facts.escapes,
            needs_fn_mut: lambda.capture_facts.needs_fn_mut,
            mutable: lambda.capture_facts.mutable.iter().cloned().collect(),
            cloned: lambda.capture_facts.cloned.iter().cloned().collect(),
            frozen: lambda.capture_facts.frozen.iter().cloned().collect(),
            materialized: lambda.capture_facts.materialized.iter().cloned().collect(),
            moved: lambda.capture_facts.moved.iter().cloned().collect(),
            frame_schedule: None,
            frame_schedule_derivation: None,
        }
    }

    pub(super) fn ownership_for(&self, ty: &Type) -> MirOwnership {
        if matches!(
            ty,
            Type::Int | Type::Float | Type::Float32 | Type::Bool | Type::Char
        ) {
            MirOwnership::copy()
        } else {
            MirOwnership::Owned
        }
    }

    pub(super) fn lower_call_arg(&mut self, arg: &TCallArg) -> Result<MirCallArg, LowerError> {
        let access = if arg.mut_borrow {
            MirAccess::Write
        } else if arg.borrow {
            MirAccess::Read
        } else if arg.clone || arg.arc_clone {
            MirAccess::Move
        } else {
            MirAccess::Read
        };
        let place = if arg.mut_borrow || arg.borrow {
            let place_expr = match &arg.value.kind {
                TExprKind::Borrow { place, .. } => place.as_ref(),
                _ => &arg.value,
            };
            match super::tir_to_mir_expr::lower_receiver_place(self, place_expr, access)? {
                Some(place) => Some(place),
                None if arg.mut_borrow => {
                    return Err(self.error(
                        self.span(),
                        "mutable call argument is not backed by a checked local place",
                    ));
                }
                None => None,
            }
        } else {
            None
        };
        let value = if let Some(place) = place {
            self.emit(
                "call-place",
                Some(arg.value.ty.clone()),
                MirOperation::ReadPlace(place),
            )?
        } else {
            self.lower_child(&arg.value)?
        };
        Ok(MirCallArg {
            value,
            access,
            span: self.span(),
            label: None,
            source_index: None,
            binder_slot: None,
            spread: false,
            implicit_clone: arg.clone,
            shared_auto_clone: arg.arc_clone,
            // Sema consumes the concrete value when it accepts trait boxing.
            owned_last_use: matches!(access, MirAccess::Move) || arg.box_as_trait.is_some(),
            authority_boundary: false,
            fn_coercion: None,
            widen_fixed_to_list: false,
            widen_to_union: None,
            box_as_trait: match &arg.box_as_trait {
                Some(ty) => self.mir_type(ty)?.identity,
                None => None,
            },
            place,
        })
    }
    pub(super) fn lower_plain_arg(&mut self, expr: &TExpr) -> Result<MirCallArg, LowerError> {
        Ok(MirCallArg {
            value: self.lower_child(expr)?,
            access: MirAccess::Read,
            span: self.span(),
            label: None,
            source_index: None,
            binder_slot: None,
            spread: false,
            implicit_clone: false,
            shared_auto_clone: false,
            owned_last_use: false,
            authority_boundary: false,
            fn_coercion: None,
            widen_fixed_to_list: false,
            widen_to_union: None,
            box_as_trait: None,
            place: None,
        })
    }

    pub(super) fn function_id_for(&self, name: &str) -> Result<MirFunctionId, LowerError> {
        self.function_registry
            .resolve(name, &self.function.module, self.span())
    }

    pub(super) fn resolve_function_id(&self, name: &str) -> Result<MirFunctionId, LowerError> {
        self.function_id_for(name)
    }

    pub(super) fn resolve_core(
        &self,
        module: &str,
        member: &str,
        span: Span,
    ) -> Result<jet_foundation::MIR::MirCoreCallId, LowerError> {
        jet_foundation::Syntax::CORE_CALLS
            .iter()
            .find(|row| row.module == module && row.member == member)
            .map(|row| jet_foundation::MIR::MirCoreCall::from_record(row).id)
            .ok_or_else(|| {
                self.error(
                    span,
                    format!("missing checked Core registry row `{module}.{member}`"),
                )
            })
    }

    pub(super) fn lower_pattern(
        &mut self,
        pattern: &TPattern,
    ) -> Result<jet_foundation::MIR::MirPattern, LowerError> {
        super::tir_to_mir_expr::lower_pattern(self, pattern)
    }

    pub(super) fn add_drop(&mut self, place: MirPlaceId, edge: MirDropEdge, span: Span) {
        self.drops.push(MirDropAction { place, edge, span });
    }

    pub(super) fn push_loop(
        &mut self,
        label: Option<String>,
        break_target: MirBlockId,
        continue_target: MirBlockId,
    ) {
        self.loops.push((label, break_target, continue_target));
        self.loop_defer_depths.push(self.defer_stack.len());
    }

    pub(super) fn pop_loop(&mut self) {
        self.loops.pop();
        self.loop_defer_depths.pop();
    }

    pub(super) fn resolve_break(&self, label: Option<&str>) -> Result<MirBlockId, LowerError> {
        self.loops
            .iter()
            .enumerate()
            .rev()
            .find(|(_, (name, _, _))| match label {
                Some(label) => name.as_deref() == Some(label),
                None => true,
            })
            .map(|(_, (_, target, _))| *target)
            .ok_or_else(|| self.error(self.span(), "break outside loop"))
    }

    pub(super) fn resolve_continue(&self, label: Option<&str>) -> Result<MirBlockId, LowerError> {
        self.loops
            .iter()
            .enumerate()
            .rev()
            .find(|(_, (name, _, _))| match label {
                Some(label) => name.as_deref() == Some(label),
                None => true,
            })
            .map(|(_, (_, _, target))| *target)
            .ok_or_else(|| self.error(self.span(), "continue outside loop"))
    }

    pub(super) fn break_cleanup_depth(&self, label: Option<&str>) -> Result<usize, LowerError> {
        self.loop_defer_depths
            .iter()
            .enumerate()
            .rev()
            .find(|(index, _)| match label {
                Some(label) => self.loops[*index].0.as_deref() == Some(label),
                None => true,
            })
            .map(|(_, depth)| *depth)
            .ok_or_else(|| self.error(self.span(), "break outside loop"))
    }

    pub(super) fn continue_cleanup_depth(&self, label: Option<&str>) -> Result<usize, LowerError> {
        self.break_cleanup_depth(label)
    }

    pub(super) fn enter_scope(
        &mut self,
        kind: MirScopeKind,
        span: Span,
        name: Option<String>,
    ) -> Result<MirScopeId, LowerError> {
        let role = format!("{kind:?}");
        let detail = name.as_deref().unwrap_or("");
        let identity = self.reserve_identity("scope", span, &role, detail)?;
        let id = MirScopeId(stable_id("mir-scope", &identity));
        self.scopes.push(MirScope {
            id,
            kind,
            span,
            name,
            facts: Default::default(),
        });
        self.emit(
            "scope.enter",
            None,
            MirOperation::ScopeEnter { scope: id, test_member: None },
        )?;
        self.defer_stack.push(Vec::new());
        Ok(id)
    }

    pub(super) fn exit_scope(&mut self, scope: MirScopeId) -> Result<(), LowerError> {
        let has_open_scope = self.defer_stack.len() > 1;
        if has_open_scope && !self.is_terminated() {
            self.emit_deferred_cleanups(self.defer_stack.len() - 1)?;
        }
        if has_open_scope {
            self.defer_stack.pop();
        }
        if !self.is_terminated() {
            self.emit("scope.exit", None, MirOperation::ScopeExit { scope })?;
        }
        Ok(())
    }

    pub(super) fn push_lexical_frame(&mut self) {
        self.defer_stack.push(Vec::new());
    }

    pub(super) fn pop_lexical_frame(&mut self) -> Result<(), LowerError> {
        if self.defer_stack.len() <= 1 {
            return Err(self.error(self.span(), "lexical defer frame underflow"));
        }
        if !self.is_terminated() {
            self.emit_deferred_cleanups(self.defer_stack.len() - 1)?;
        }
        self.defer_stack.pop();
        Ok(())
    }

    pub(super) fn error(&self, span: Span, message: impl Into<String>) -> LowerError {
        LowerError::new(span, message)
    }

    pub(super) fn lower_return(&mut self, value: Option<&TExpr>) -> Result<(), LowerError> {
        let value = match value {
            Some(expr) => Some(self.lower_child(expr)?),
            None => None,
        };
        if !self.is_terminated() {
            self.terminate_with_cleanup(MirTerminator::Return { value }, 0)?;
        }
        Ok(())
    }

    pub(super) fn lower_contract(&mut self, contract: &TContract) -> Result<(), LowerError> {
        match contract.disposition {
            TContractDisposition::Check => {
                let _ = self.lower_child(&contract.condition)?;
                let _ = self.lower_child(&contract.message)?;
                Ok(())
            }
            TContractDisposition::Proven | TContractDisposition::Stripped => Ok(()),
        }
    }

    pub(super) fn lower_contract_scope(
        &mut self,
        pre: &[TContract],
        body: &[TStmt],
        post: &[TContract],
        _result: &TContractResult,
    ) -> Result<(), LowerError> {
        for contract in pre {
            self.lower_contract(contract)?;
        }
        lower_stmts(self, body)?;
        for contract in post {
            self.lower_contract(contract)?;
        }
        Ok(())
    }

    pub(super) fn record_erasure(
        &mut self,
        _construct: &str,
        _span: Span,
        _reason: TirErasureReason,
    ) {
    }

    pub(super) fn lower_swizzle_place(
        &mut self,
        base: &TExpr,
        lanes: &[u8],
        access: MirAccess,
    ) -> Result<Vec<MirPlaceId>, LowerError> {
        let root = if let TExprKind::Local(local) = &base.kind {
            self.place_for_local(local, access)?
        } else {
            let value = self.lower_child(base)?;
            let id = self.place_id("temporary", &format!("{}", value.0))?;
            let span = self.span();
            let ty = self.mir_type(&base.ty)?;
            self.places.push(MirPlace {
                id,
                span,
                ty,
                base: MirPlaceBase::Temporary(value),
                projections: Vec::new(),
                access,
                persist_key: None,
            });
            id
        };
        lanes
            .iter()
            .map(|lane| self.project_field_place(root, &lane.to_string(), Type::Int, self.span()))
            .collect()
    }

    pub(super) fn lower_core_call_arg(
        &mut self,
        arg: &TExpr,
        index: usize,
        record: &'static jet_foundation::Syntax::CoreCallRecord,
        widen_to_vec: bool,
    ) -> Result<MirCallArg, LowerError> {
        let (access, place) = match &arg.kind {
            TExprKind::Borrow { place, mutable } => {
                let access = if *mutable {
                    MirAccess::Write
                } else {
                    MirAccess::Read
                };
                let place = match &place.kind {
                    TExprKind::Local(local) => Some(self.place_for_local(local, access)?),
                    _ if *mutable => {
                        return Err(self.error(
                            self.span(),
                            "mutable Core argument is not backed by a checked local place",
                        ));
                    }
                    _ => None,
                };
                (access, place)
            }
            TExprKind::Clone(_)
            | TExprKind::ExplicitCopy(_)
            | TExprKind::MaterializeView(_) => (MirAccess::Move, None),
            _ if crate::Collections::is_iter_type(&arg.ty) => (MirAccess::Move, None),
            _ => (MirAccess::Read, None),
        };
        let value = if let Some(place) = place {
            self.emit(
                "core-call-place",
                Some(arg.ty.clone()),
                MirOperation::ReadPlace(place),
            )?
        } else if matches!(access, MirAccess::Move)
            && crate::Collections::is_iter_type(&arg.ty)
        {
            if let Some(place) =
                super::tir_to_mir_expr::lower_receiver_place(self, arg, MirAccess::Move)?
            {
                self.emit(
                    "core-call-owned-place",
                    Some(arg.ty.clone()),
                    MirOperation::MovePlace { place },
                )?
            } else {
                self.lower_child(arg)?
            }
        } else {
            self.lower_child(arg)?
        };
        Ok(MirCallArg {
            value,
            place,
            access,
            span: self.span(),
            label: None,
            source_index: Some(index),
            binder_slot: None,
            spread: false,
            implicit_clone: false,
            shared_auto_clone: false,
            owned_last_use: matches!(access, MirAccess::Move),
            authority_boundary: record.module == "core.plugin"
                && record.member == "load"
                && index == 1,
            fn_coercion: None,
            widen_fixed_to_list: widen_to_vec,
            widen_to_union: None,
            box_as_trait: None,
        })
    }

    pub(super) fn lower_lambda_send(&mut self, lambda: &TLambda) -> Result<MirValueId, LowerError> {
        self.lower_lambda_kind(lambda, true, None)
    }

    pub(super) fn lower_named_fn_send(
        &mut self,
        name: &str,
        ty: &Type,
    ) -> Result<MirValueId, LowerError> {
        let function = self.function_id_for(name)?;
        let target = self.mir_send_fn_type(ty)?;
        self.emit_mir_type(
            "named-fn-send",
            Some(target),
            MirOperation::Closure {
                function,
                captures: Vec::new(),
                facts: MirCaptureFacts::default(),
            },
        )
    }

    pub(super) fn mir_send_fn_type(&mut self, ty: &Type) -> Result<MirType, LowerError> {
        let span = self.span();
        let lowered = self.mir_type(ty)?;
        let MirTypeKind::Fn(signature) = lowered.kind else {
            return Err(LowerError::new(
                span,
                "checked SendFn binding does not carry a function type",
            ));
        };
        let kind = MirTypeKind::SendFn {
            params: signature.params,
            ret: signature.ret,
        };
        let key = kind.canonical_key();
        Ok(MirType::from_kind(kind).with_identity(MirTypeId(stable_id("mir-type", &key))))
    }

    pub(super) fn bind_local_send_fn(
        &mut self,
        local: &TLocal,
        ty: Type,
        mutable: bool,
        comptime: bool,
        uninit: bool,
    ) -> Result<MirPlaceId, LowerError> {
        self.send_fn_locals.insert(local.name.clone());
        self.bind_local(local, ty, mutable, comptime, uninit)
    }

    pub(super) fn nominal_type_id(&mut self, name: &str) -> Result<MirTypeId, LowerError> {
        let ty = self.mir_type(&Type::Named(name.to_string()))?;
        ty.identity.ok_or_else(|| {
            self.error(
                self.span(),
                format!("checked MIR type `{name}` has no identity"),
            )
        })
    }

    pub(super) fn distinct_range(&self, name: &str) -> Option<(i64, i64)> {
        self.type_defs.iter().find_map(|ty| match &ty.kind {
            MirTypeDefKind::Distinct { range, .. } if ty.key == name || ty.name == name => *range,
            _ => None,
        })
    }

    pub(super) fn distinct_base(&self, name: &str) -> Option<Type> {
        self.type_defs.iter().find_map(|ty| match &ty.kind {
            MirTypeDefKind::Distinct { base, .. } if ty.key == name || ty.name == name => {
                Some(mir_type_as_ast(base))
            }
            _ => None,
        })
    }

    pub(super) fn foreign_id_for(&self, symbol: &str) -> Result<MirForeignId, LowerError> {
        Ok(MirForeignId(stable_id("mir-foreign", symbol)))
    }

    pub(super) fn lower_pattern_match(
        &mut self,
        subject: MirValueId,
        shape: &MirPatternShape,
    ) -> Result<MirValueId, LowerError> {
        match shape {
            MirPatternShape::Text(parts, _) => {
                let ty = Type::Option(Box::new(Type::Tuple(Vec::new())));
                let carrier = TFailureCarrier::from_checked_type(&ty);
                let call =
                    self.intern_prelude_route(super::pattern_match_route(false, &ty, &carrier)?)?;
                self.emit(
                    "pattern-match",
                    Some(ty),
                    MirOperation::Semantic(MirSemanticOp::TextPatternMatch {
                        call,
                        subject,
                        parts: parts.clone(),
                    }),
                )
            }
            MirPatternShape::Binary(parts, _) => {
                let ty = Type::Option(Box::new(Type::Tuple(Vec::new())));
                let carrier = TFailureCarrier::from_checked_type(&ty);
                let call =
                    self.intern_prelude_route(super::pattern_match_route(true, &ty, &carrier)?)?;
                self.emit(
                    "pattern-match",
                    Some(ty),
                    MirOperation::Semantic(MirSemanticOp::BinaryPatternMatch {
                        call,
                        subject,
                        parts: parts.clone(),
                    }),
                )
            }
            _ => Err(self.error(self.span(), "checked scan is not a text or binary pattern")),
        }
    }
}
