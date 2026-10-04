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
    TirAccess, TirDeclarations, TirParam, lower_core_builtin_owners, lower_mir_access,
    lower_mir_convention, lower_param_zone, lower_type, lower_type_defs, lower_view_provenance,
};
use super::{
    TCallArg, TContract, TContractDisposition, TContractResult, TExpr, TExprKind, TFailureCarrier,
    TFunc, TFuncKind, TGenericParam, THardwareSetup, TLambda, TLambdaBody, TLocal, TPattern,
    TPlace, TPreludeRoute, TRoutePlan, TStmt, TTargetApplicability, TVisibility, TirErasureReason,
    TirProgram,
};
use jet_foundation::AST::{AccessConvention, CtKey, CtReport, CtValue, Type};
use jet_foundation::Diagnostics::Span;
use jet_foundation::MIR::{
    MIR_SCHEMA_VERSION, MirAbi, MirAccess, MirArtifactId, MirArtifactKind, MirArtifactPlan,
    MirArtifactRequest, MirArtifactTarget, MirAssociatedTypeDecl, MirAssociatedTypeValue,
    MirBasicBlock, MirBinaryDispatch, MirBinaryPatternPart, MirBlockId, MirCImportLink, MirCLib,
    MirCOverlayOverride, MirCallArg, MirCallFallibility, MirCallSignature, MirCallbackAdapter,
    MirCallbackId, MirCallee, MirCaptureFacts, MirCaptureOperand, MirCaptureParam, MirCffiFacts,
    MirCliCommand, MirCliDefault, MirCliEntry, MirCliInput, MirCliInputShape, MirCliValueKind,
    MirCloseAdapter, MirConstant, MirConstantDef, MirConstantId, MirCoreCallId, MirCoreClosureKind,
    MirCoveragePoint, MirDbQueryMetadata, MirDbTableFact, MirDropAction, MirDropKind,
    MirEffectFacts, MirEntryKind, MirEntryOutput, MirEntrySpec, MirFailureCarrier, MirFieldId,
    MirForeign, MirForeignAbi, MirForeignId, MirForeignLanguage, MirFunction, MirFunctionForm,
    MirFunctionId, MirFunctionKind, MirGeneratorFacts, MirHandleId, MirHandleLifecycle,
    MirHandleOwnership, MirHandlePayload, MirHardwareSetup, MirHarnessId, MirHarnessKind,
    MirHarnessPlan, MirImplDef, MirImplId, MirImport, MirImportId, MirImportItem, MirImportKind,
    MirIndexKind, MirInstruction, MirJob, MirJobCachePolicy, MirJobDispatch, MirJobId,
    MirJobSchedule, MirJobScope, MirJobSkip, MirKernelFacts, MirLinkArtifact, MirLinkArtifactKind,
    MirLinkUnit, MirLinkUnitId, MirLocal, MirLocalId, MirNameFacts, MirNominalRef, MirOpId,
    MirOperation, MirOptimizationDecision, MirOptimizationFacts, MirOptimizationRejection,
    MirOutputCheck, MirOutputCheckId, MirOwnership, MirOwnershipMode, MirPackageFacts,
    MirPanicContext, MirPanicLoc, MirParam, MirPattern, MirPatternBinding, MirPatternField,
    MirPatternPosition, MirPatternShape, MirPlace, MirPlaceBase, MirPlaceId, MirPreludeAbi,
    MirPreludeCall, MirPreludeCallId, MirPreludeFamily, MirProgram, MirProjection, MirRequireKind,
    MirRuntimePartId, MirScope, MirScopeId, MirScopeKind, MirSemanticOp, MirSerdeCodec, MirSiteId,
    MirSourceFile, MirSourceFileId, MirSymbol, MirTargetApplicability, MirTerminator, MirTestCase,
    MirTestId, MirTestKind, MirTextPatternPart, MirTraitDef, MirTraitId, MirTraitMethod,
    MirTraitMethodId, MirTraitRef, MirType, MirTypeDef, MirTypeDefKind, MirTypeId, MirTypeKind,
    MirUnsafeGate, MirValueId, MirVariant, MirVariantPayload, MirVectorAccess, MirVectorAccessRoot,
    MirVectorFact, MirVectorLayout, MirVectorRule, MirVisibility, MirWebParamField,
    MirWebParamReconstruction, stable_id,
};
/// Keep only derive requests whose implementation is part of this target's
/// checked program. Type-site markers are broader than Rust trait conformance:
/// a user derive provider (for example `#Summarize`) contributes inherent
/// methods, while markers such as `#Numeric` are semantic facts. Carrying
/// those marker names into MIR would synthesize a fake trait row and make the
/// AOT emitter look for an implementation that sema never promised.
fn retain_selected_derives(
    types: &mut [MirTypeDef],
    impls: &[MirImplDef],
    modules: &[TirModuleFact],
    target: MirArtifactTarget,
) {
    let selected_modules = modules
        .iter()
        .map(|module| jet_foundation::MIR::MirModuleId(stable_id("mir-module", &module.key)))
        .collect::<HashSet<_>>();
    for definition in types {
        definition.derives.retain(|derive| {
            is_native_rust_derive(*derive)
                || impls.iter().any(|implementation| {
                    implementation
                        .trait_ref
                        .as_ref()
                        .is_some_and(|trait_ref| trait_ref.id == *derive)
                        && implementation.self_type.identity == Some(definition.id)
                        && selected_modules.contains(&implementation.module)
                        && target_supports_impl(implementation.target_applicability, target)
                })
        });
    }
}

/// These names are emitted directly by `MIRRust::emit_type_def`; they do not
/// need a checked provider implementation.
fn is_native_rust_derive(derive: MirTraitId) -> bool {
    [
        "Clone",
        "Copy",
        "Debug",
        "Default",
        "Eq",
        "Hash",
        "Ord",
        "PartialEq",
        "PartialOrd",
    ]
    .iter()
    .any(|name| MirTraitId(stable_id("mir-trait", name)) == derive)
}

fn target_supports_impl(applicability: MirTargetApplicability, target: MirArtifactTarget) -> bool {
    match target {
        MirArtifactTarget::RustAot => applicability.rust_aot,
        MirArtifactTarget::Cranelift => applicability.cranelift,
        MirArtifactTarget::Interpreter => applicability.interpreter,
        MirArtifactTarget::Web => applicability.web,
    }
}
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
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
    /// Checked return types keyed by the same stable function identity used by
    /// MIR call lowering.  Transaction Rollback lowering uses this fact for
    /// its synthesized snapshot value; it must not guess an associated type.
    return_types: HashMap<MirFunctionId, Option<Type>>,
    generic_params: HashMap<MirFunctionId, Vec<String>>,
    owner_types: HashMap<MirFunctionId, Type>,
    pub(super) receiver_access: HashMap<MirFunctionId, MirAccess>,
    /// D-SHAPE-RESOURCE1=A: each nominal `Close` implementation, keyed by the
    /// checked owner's MIR type identity once the type rows exist. Scope-end
    /// cleanup closes a still-live local of that type through this function.
    close_impl_rows: Vec<(String, Type, Span, MirFunctionId)>,
    close_impls: HashMap<MirTypeId, MirFunctionId>,
}

impl FunctionRegistry {
    fn build(
        functions: &[TFunc],
        entry_sibling_calls: Option<&(String, String)>,
    ) -> Result<Self, LowerError> {
        let mut registry = Self {
            by_key: HashMap::new(),
            top_level_by_key: HashMap::new(),
            top_level_by_module_name: HashMap::new(),
            by_name: HashMap::new(),
            by_module_name: HashMap::new(),
            by_target: HashMap::new(),
            by_owner_method: HashMap::new(),
            by_identity: HashMap::new(),
            return_types: HashMap::new(),
            owner_types: HashMap::new(),
            generic_params: HashMap::new(),
            receiver_access: HashMap::new(),
            close_impl_rows: Vec::new(),
            close_impls: HashMap::new(),
        };
        // Resolve stable-ID hash collisions from a canonical identity order.
        // Normal rows retain their historical IDs; only a real collision gets
        // a deterministic salted identity, so allocation order is irrelevant.
        let mut ordered = functions
            .iter()
            .map(|function| (registry_function_identity(function), function))
            .collect::<Vec<_>>();
        ordered.sort_unstable_by(|(left, _), (right, _)| left.cmp(right));
        let mut assigned = HashMap::<String, MirFunctionId>::new();
        let mut owners = HashMap::<MirFunctionId, String>::new();
        for (identity, function) in ordered {
            if assigned.contains_key(&identity) {
                return Err(LowerError::new(
                    function.source_span,
                    format!("duplicate checked function identity `{identity}`"),
                ));
            }
            let mut salt = 0u64;
            let id = loop {
                let candidate_identity = if salt == 0 {
                    identity.clone()
                } else {
                    format!("{identity}|collision:{salt}")
                };
                let candidate = MirFunctionId(stable_id("mir-function", &candidate_identity));
                match owners.get(&candidate) {
                    None => {
                        owners.insert(candidate, identity.clone());
                        break candidate;
                    }
                    Some(owner) if owner == &identity => break candidate,
                    Some(_) => salt += 1,
                }
            };
            assigned.insert(identity, id);
        }
        for function in functions {
            let identity = registry_function_identity(function);
            let id = assigned[&identity];
            let receiver = match &function.kind {
                TFuncKind::Method { self_conv, .. } | TFuncKind::TraitMethod { self_conv, .. } => {
                    *self_conv
                }
                TFuncKind::TopLevel => None,
            };
            if let Some(owner_type) = match &function.kind {
                TFuncKind::Method { owner_type, .. }
                | TFuncKind::TraitMethod { owner_type, .. } => Some(owner_type),
                TFuncKind::TopLevel => None,
            } {
                registry.owner_types.insert(id, owner_type.clone());
            }
            if let Some(access) = receiver {
                registry
                    .receiver_access
                    .insert(id, lower_mir_convention(access));
            }
            if let TFuncKind::TraitMethod {
                owner_type,
                trait_name,
                ..
            } = &function.kind
            {
                let leaf = trait_name.rsplit("::").next().unwrap_or(trait_name);
                if leaf == crate::Syntax::TRAIT_CLOSE && method_leaf(&function.name) == "close" {
                    registry.close_impl_rows.push((
                        function.module.clone(),
                        owner_type.clone(),
                        function.source_span,
                        id,
                    ));
                }
            }
            registry.by_identity.insert(identity, id);
            registry.return_types.insert(id, function.ret.clone());
            registry.generic_params.insert(
                id,
                function
                    .generic_params
                    .iter()
                    .map(|param| param.name.clone())
                    .collect(),
            );
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
            // D-MOD-CYCLE1=A: a package sibling calls an entry function by the
            // imported spelling `<entry alias>::<name>`; it names this row.
            if let Some((entry_module, prefix)) = entry_sibling_calls {
                if matches!(&function.kind, TFuncKind::TopLevel)
                    && function.module == *entry_module
                    && !function.name.contains("::")
                {
                    registry
                        .by_name
                        .entry(format!("{prefix}{}", crate::Codegen::mangle(&function.name)))
                        .or_default()
                        .push(id);
                }
            }
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

    /// Resolve every `Close` owner to its checked MIR type identity. Runs once
    /// the type rows exist, before any function body is lowered.
    fn index_close_impls(&mut self, types: &[MirTypeDef]) -> Result<(), LowerError> {
        for (module, owner, span, function) in std::mem::take(&mut self.close_impl_rows) {
            let owner = module_relative_type_instance(types, &owner, &module, span)?;
            let Some(identity) = owner.identity else {
                return Err(LowerError::new(span, "checked Close owner has no nominal identity"));
            };
            if self.close_impls.insert(identity, function).is_some_and(|other| other != function) {
                return Err(LowerError::new(span, "checked Close owner has two implementations"));
            }
        }
        Ok(())
    }

    /// The nominal `Close` implementation for a value of this MIR type.
    pub(super) fn close_impl_for(&self, ty: &MirType) -> Option<MirFunctionId> {
        self.close_impls.get(&ty.identity?).copied()
    }

    fn id_for(&self, function: &TFunc) -> Result<MirFunctionId, LowerError> {
        let identity = registry_function_identity(function);
        if let Some(id) = self.by_identity.get(&identity).copied() {
            return Ok(id);
        }
        // Nested synthetic helpers are created during lowering after the
        // top-level registry is built.  They still use this exact canonical
        // identity, so every emitted row and reference shares one ID.
        if function.synthetic {
            return Ok(MirFunctionId(stable_id("mir-function", &identity)));
        }
        Err(LowerError::new(
            function.source_span,
            format!("missing checked function `{identity}`"),
        ))
    }
    pub(super) fn return_type_for(&self, id: MirFunctionId) -> Option<Type> {
        self.return_types.get(&id).cloned().flatten()
    }
    pub(super) fn call_return_type_for(
        &self,
        id: MirFunctionId,
        type_args: &[Type],
    ) -> Result<Option<Type>, &'static str> {
        let Some(return_type) = self.return_types.get(&id) else {
            return Err("checked call target has no return-type fact");
        };
        let Some(return_type) = return_type else {
            return Ok(None);
        };
        let generic_params = self
            .generic_params
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if generic_params.is_empty() {
            return Ok(Some(return_type.clone()));
        }
        if generic_params.len() != type_args.len() {
            return Err("checked generic call has incomplete return-type arguments");
        }
        let substitutions = generic_params
            .iter()
            .cloned()
            .zip(type_args.iter().cloned())
            .collect::<HashMap<_, _>>();
        Ok(Some(crate::Generics::substitute_type(
            return_type,
            &substitutions,
        )))
    }

    pub(super) fn method_call_return_type_for(
        &self,
        id: MirFunctionId,
        type_args: &[Type],
        receiver_type: &Type,
    ) -> Result<Option<Type>, &'static str> {
        let Some(return_type) = self.return_types.get(&id) else {
            return Err("checked method target has no return-type fact");
        };
        let Some(return_type) = return_type else {
            return Ok(None);
        };
        let generic_params = self
            .generic_params
            .get(&id)
            .map(Vec::as_slice)
            .unwrap_or_default();
        if generic_params.len() != type_args.len() {
            return Err("checked generic method has incomplete return-type arguments");
        }
        let mut substitutions = generic_params
            .iter()
            .cloned()
            .zip(type_args.iter().cloned())
            .collect::<HashMap<_, _>>();
        let owner_type = self
            .owner_types
            .get(&id)
            .ok_or("checked method target has no owner-type fact")?
            .without_user_tags();
        let receiver_type = receiver_type.without_user_tags();
        let (owner_name, owner_params) = match &owner_type {
            Type::Apply { name, args } => (name, args.as_slice()),
            _ => return Ok(Some(crate::Generics::substitute_type(return_type, &substitutions))),
        };
        let (receiver_name, owner_args) = match &receiver_type {
            Type::Apply { name, args } => (name, args.as_slice()),
            Type::Named(name) => (name, &[][..]),
            _ => return Err("checked method receiver is not a nominal type"),
        };
        // The target's owner is canonicalized (`<module>::Box<T>`) while a
        // checked receiver may keep its source leaf (`Box<Int>`); both name one
        // nominal, so compare the leaf identity.
        let leaf = |name: &str| name.rsplit("::").next().unwrap_or(name).to_string();
        if leaf(owner_name) != leaf(receiver_name) || owner_params.len() != owner_args.len() {
            return Err("checked method receiver owner arguments disagree with its target");
        }
        for (parameter, argument) in owner_params.iter().zip(owner_args) {
            match parameter {
                Type::Named(name) => {
                    substitutions.insert(name.clone(), argument.clone());
                }
                // A demanded instance (`Box<Int>::new`) is already specialized:
                // its owner arguments and return type are concrete, so there is
                // no binder left to substitute.
                _ => {}
            }
        }
        Ok(Some(crate::Generics::substitute_type(
            return_type,
            &substitutions,
        )))
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
            let top_level = self
                .top_level_by_module_name
                .get(&(current_module.to_string(), name.to_string()))
                .cloned()
                .unwrap_or_default();
            if top_level.is_empty() {
                self.by_module_name
                    .get(&(current_module.to_string(), name.to_string()))
                    .cloned()
                    .unwrap_or_default()
            } else {
                top_level
            }
        };
        if candidates.is_empty() {
            candidates = self.typed_candidates(name, current_module);
        }
        // Unqualified sibling calls (`mark_ready(state, changed)`) miss when
        // the caller module spelling disagrees with the callee's stored
        // module. Fall back to that leaf name. Do not strip `Type::method`
        // — `encode`/`decode` would become ambiguous across every impl.
        if candidates.is_empty() && !name.contains("::") {
            candidates = self.by_name.get(name).cloned().unwrap_or_default();
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
        // Static generic calls carry the full applied owner (`Box<Int>::new`)
        // while the checked method target is indexed by owner + method. Resolve
        // that exact owner before interpreting an inner `::` as a trait
        // separator; canonical nominal identities themselves contain `::`.
        let direct_owner = self
            .by_owner_method
            .iter()
            .filter(|((_, owner, candidate_method), _)| {
                rhs.is_none() && owner == &owner_method && candidate_method == method
            })
            .flat_map(|(_, ids)| ids.iter().copied())
            .collect::<Vec<_>>();
        if !direct_owner.is_empty() {
            return direct_owner;
        }
        if let Some((raw_owner, trait_name)) = raw_prefix.rsplit_once("::") {
            let candidates =
                self.typed_target_candidates(raw_owner, trait_name, method, rhs, current_module);
            if rhs.is_some() || !candidates.is_empty() {
                return candidates;
            }
            // A bounded generic receiver can reach MIR with no concrete owner
            // spelling. Keep the lookup checked and deterministic: a lone
            // implementation of this trait method is usable; several remain
            // ambiguous rather than guessing.
            if raw_owner.is_empty() {
                let candidates = self
                    .by_target
                    .iter()
                    .filter(|(target, _)| {
                        target.module == current_module
                            && target.trait_name.as_deref() == Some(trait_name)
                            && target.method == method
                            && match rhs {
                                Some(rhs) => target.rhs.as_deref() == Some(rhs),
                                None => target.rhs.is_none(),
                            }
                    })
                    .flat_map(|(_, ids)| ids.iter().copied())
                    .collect();
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
            let local = self
                .by_owner_method
                .get(&(current_module.to_string(), owner_method, method.to_string()))
                .cloned()
                .unwrap_or_default();
            if local.is_empty() && rhs.is_none() {
                return self.carrier_record_methods(raw_prefix, method);
            }
            local
        }
    }

    /// A bare built-in boundary spelling (`URL`) that the calling module does
    /// not declare names its Core carrier record (`row_claims_bare_name`), so
    /// a method on it is that record's checked method.
    fn carrier_record_methods(&self, owner: &str, method: &str) -> Vec<MirFunctionId> {
        let Some(suffix) = crate::Syntax::typed_head_kind(owner)
            .and_then(|kind| kind.carrier_record_module())
            .and_then(jet_foundation::CoreModuleExports::core_source_module)
            .map(|source| format!("{}::{owner}", source.path))
        else {
            return Vec::new();
        };
        self.by_owner_method
            .iter()
            .filter(|((_, candidate_owner, candidate_method), _)| {
                candidate_method == method && candidate_owner.ends_with(&suffix)
            })
            .flat_map(|(_, ids)| ids.iter().copied())
            .collect()
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
                    None => target.rhs.is_none() || target.rhs.as_deref() == Some(owner.as_str()),
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

    pub(super) fn resolve(
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
    let (_, rhs) = split_method_rhs(method_part)?;
    rhs
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

fn synthetic_function_identity(function: &TFunc) -> String {
    format!(
        "{}|synthetic|{}|{}",
        function_identity(function),
        function.module,
        function.name
    )
}

fn registry_function_identity(function: &TFunc) -> String {
    if function.synthetic {
        synthetic_function_identity(function)
    } else {
        function_identity(function)
    }
}

fn anonymous_union_shape_matches(left: &[MirVariant], right: &[MirVariant]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.name == right.name
                && matches!(
                    (&left.payload, &right.payload),
                    (
                        MirVariantPayload::Single(left),
                        MirVariantPayload::Single(right)
                    ) if left.same_checked_type(right)
                        || left.canonical_key() == right.canonical_key()
                )
        })
}

/// Materialize the checked anonymous-union carriers that are recorded in TIR
/// but may not have survived into the declaration list.
fn lower_anonymous_union_type_defs(
    program: &TirProgram,
    types: &mut Vec<MirTypeDef>,
) -> Result<(), LowerError> {
    let module = program
        .artifact_facts
        .modules
        .iter()
        .find(|module| {
            module.path == program.source_file || module.source_path == program.source_file
        })
        .or_else(|| program.artifact_facts.modules.first())
        .map(|module| module.key.as_str())
        .unwrap_or_default()
        .to_string();
    let module_id = jet_foundation::MIR::MirModuleId(stable_id("mir-module", &module));
    let span = Span::new(0, 0);
    let mut names = program
        .enum_variants
        .keys()
        .filter(|name| name.starts_with("__JetUnion_"))
        .cloned()
        .collect::<Vec<_>>();
    names.sort_unstable();

    for name in names {
        let variant_names = program.enum_variants.get(&name).ok_or_else(|| {
            LowerError::new(span, format!("anonymous union `{name}` has no variants"))
        })?;
        let mut members = Vec::with_capacity(variant_names.len());
        for variant in variant_names {
            let payload_key = format!(
                "{}::{}",
                crate::Codegen::mangle_path(&name),
                crate::Codegen::mangle_path(variant),
            );
            let payloads = program
                .enum_variant_payload_types
                .get(&payload_key)
                .or_else(|| {
                    program.enum_variant_payload_types.get(&format!(
                        "{}::{variant}",
                        crate::Codegen::mangle_path(&name)
                    ))
                })
                .or_else(|| {
                    program
                        .enum_variant_payload_types
                        .get(&format!("{name}::{variant}"))
                })
                .ok_or_else(|| {
                    LowerError::new(
                        span,
                        format!(
                            "anonymous union `{name}` has no payload facts for variant `{variant}`"
                        ),
                    )
                })?;
            let [member] = payloads.as_slice() else {
                return Err(LowerError::new(
                    span,
                    format!("anonymous union `{name}` variant `{variant}` must have one payload"),
                ));
            };
            let member_tag = crate::AST::union_member_tag(member);
            if variant != &member_tag && variant != &crate::Codegen::mangle_path(&member_tag) {
                return Err(LowerError::new(
                    span,
                    format!("anonymous union `{name}` has a mismatched variant tag"),
                ));
            }
            members.push(member.clone());
        }
        if crate::AST::union_enum_name(&members) != name {
            return Err(LowerError::new(
                span,
                format!("anonymous union `{name}` has a non-canonical shape"),
            ));
        }

        // The carrier identity is the checked instance identity: members name
        // their canonical declaration keys (`module::DBError`), exactly as
        // `canonical_type_instance` spells every value of this union type.
        let canonical_union = canonicalize_type(types, &Type::Union(members.clone()), span)?;
        let union_id = MirTypeId(stable_id("mir-type", &type_identity_key(&canonical_union)));
        if types
            .iter()
            .any(|definition| definition.id == union_id || definition.key == name)
        {
            continue;
        }

        // Each payload is the member's checked instance, so a member value
        // built anywhere in the program has exactly this payload identity.
        let variants = members
            .iter()
            .map(|member| {
                let tag = crate::AST::union_member_tag(member);
                Ok(MirVariant {
                    name: tag.clone(),
                    wire_name: tag,
                    span,
                    payload: MirVariantPayload::Single(canonical_type_instance(
                        types, member, span,
                    )?),
                    discriminant: None,
                })
            })
            .collect::<Result<Vec<_>, LowerError>>()?;
        let qualified_suffix = format!("::{name}");
        let metadata = types
            .iter()
            .find(|definition| {
                definition.generic_params.is_empty()
                    && definition.name == name
                    && definition.key.ends_with(&qualified_suffix)
                    && matches!(
                        &definition.kind,
                        MirTypeDefKind::Enum {
                            variants: candidate_variants,
                            ..
                        } if anonymous_union_shape_matches(&variants, candidate_variants)
                    )
            })
            .cloned();
        let mut definition = MirTypeDef {
            id: union_id,
            module: module_id,
            key: name.clone(),
            name,
            span,
            public: false,
            package_public: false,
            generic_params: Vec::new(),
            derives: Vec::new(),
            auto_derive_default: true,
            auto_printable: false,
            published_schema: false,
            single_use: false,
            must_use: false,
            compiler_builtin: None,
            layout: None,
            c_layout_tag: None,
            enum_layout: None,
            layout_alignment: None,
            serde: Vec::new(),
            cli_bindings: Vec::new(),
            cli: None,
            ownership: MirOwnershipMode::Owned,
            boxed_edges: Vec::new(),
            kind: MirTypeDefKind::Enum {
                variants,
                methods: Vec::new(),
            },
        };
        if let Some(source) = metadata {
            // The qualified declaration owns the checked trait/codec metadata,
            // while this row must retain the canonical carrier identity.
            definition.span = source.span;
            definition.public = source.public;
            definition.package_public = source.package_public;
            definition.generic_params = source.generic_params;
            definition.derives = source.derives;
            definition.auto_derive_default = source.auto_derive_default;
            definition.auto_printable = source.auto_printable;
            definition.published_schema = source.published_schema;
            definition.single_use = source.single_use;
            definition.must_use = source.must_use;
            definition.compiler_builtin = source.compiler_builtin;
            definition.c_layout_tag = source.c_layout_tag;
            definition.enum_layout = source.enum_layout;
            definition.layout_alignment = source.layout_alignment;
            definition.serde = source.serde;
            definition.cli_bindings = source.cli_bindings;
            definition.cli = source.cli;
            definition.ownership = source.ownership;
            definition.boxed_edges = source.boxed_edges;
        }
        types.push(definition);
    }
    Ok(())
}

/// D-UNIONTYPE1=A: an anonymous union has one identity. Sema declares its
/// carrier enum in every module that spells the union
/// (`module::__JetUnion_A_B`), but all of those declarations name the one
/// structural type. Fold each declaration into the canonical row: that row
/// keeps the structural id and key and gains the declarations' checked
/// methods; the module copies leave the table, so no value, pattern, or impl
/// can be typed by a second carrier. `canonical_nominal_name` resolves the
/// qualified spelling to the canonical key. Runs after method retargeting,
/// which pairs rows with their TIR declarations by position.
fn fold_anonymous_union_declarations(types: &mut Vec<MirTypeDef>) {
    let mut folded = HashSet::new();
    for canonical_index in 0..types.len() {
        let canonical = &types[canonical_index];
        if canonical.key != canonical.name || !canonical.name.starts_with("__JetUnion_") {
            continue;
        }
        let MirTypeDefKind::Enum { variants, .. } = &canonical.kind else {
            continue;
        };
        let qualified_suffix = format!("::{}", canonical.name);
        let declarations = types
            .iter()
            .enumerate()
            .filter(|(_, declaration)| {
                declaration.generic_params.is_empty()
                    && declaration.name == canonical.name
                    && declaration.key.ends_with(&qualified_suffix)
                    && matches!(
                        &declaration.kind,
                        MirTypeDefKind::Enum {
                            variants: declared,
                            ..
                        } if anonymous_union_shape_matches(variants, declared)
                    )
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let mut declared_methods = Vec::new();
        for index in declarations {
            if let MirTypeDefKind::Enum { methods, .. } = &types[index].kind {
                declared_methods.extend(methods.iter().copied());
            }
            folded.insert(index);
        }
        if let MirTypeDefKind::Enum { methods, .. } = &mut types[canonical_index].kind {
            for method in declared_methods {
                if !methods.contains(&method) {
                    methods.push(method);
                }
            }
        }
    }
    let mut index = 0;
    types.retain(|_| {
        let keep = !folded.contains(&index);
        index += 1;
        keep
    });
}

/// `JET_DEBUG_LOWER_ALL`: print one failure line at its source position.
fn report_lower_sweep_failure(
    indexed_sources: &BTreeMap<String, IndexedSource<'_>>,
    source_file: &str,
    error: &LowerError,
) {
    let (line, column) = indexed_sources
        .get(source_file)
        .map_or((0, 0), |source| source.lines.line_col(source.text, error.span.start));
    eprintln!(
        "JET_DEBUG_LOWER_ALL ice {source_file}:{line}:{column}: {}",
        error.message
    );
}

/// Lower a TIR program to MIR. The program is consumed: each function's body
/// is released as soon as it is lowered, so a large program never holds its
/// whole TIR and whole MIR at once.
pub fn lower_tir_to_mir(mut program: TirProgram) -> Result<MirProgram, LowerError> {
    let mut function_registry =
        FunctionRegistry::build(&program.funcs, program.entry_sibling_calls.as_ref())?;
    let mut types = lower_type_defs(&program.declarations.type_defs, &function_registry)?;
    lower_anonymous_union_type_defs(&program, &mut types)?;
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
    fold_anonymous_union_declarations(&mut types);
    function_registry.index_close_impls(&types)?;
    types.sort_unstable_by_key(|ty| ty.id);
    let mut functions: Vec<MirFunction> = Vec::with_capacity(program.funcs.len());
    let mut prelude_calls: Vec<MirPreludeCall> = Vec::new();
    let mut callbacks: Vec<MirCallbackAdapter> = Vec::new();
    let mut source_files: Vec<MirSourceFile> = Vec::new();
    // Row positions by id, so merging each function's rows stays constant
    // time per row (the first row per id wins, as a linear search would).
    let mut prelude_call_positions: HashMap<MirPreludeCallId, usize> = HashMap::new();
    let mut callback_positions: HashMap<MirCallbackId, usize> = HashMap::new();
    let mut source_file_positions: HashMap<MirSourceFileId, usize> = HashMap::new();
    let mut type_instances = declared_type_instances(&types);
    for definition in &types {
        for embedded in type_def_types(definition) {
            let instance =
                canonical_type_instance(&types, &mir_type_as_ast(&embedded), definition.span)?;
            merge_type_instance(&mut type_instances, instance, definition.span)?;
        }
    }
    // Index every source once; per-function lowering reads positions from it.
    let indexed_sources: BTreeMap<String, IndexedSource<'_>> = program
        .source_files
        .iter()
        .map(|(path, text)| {
            let source = IndexedSource {
                text: text.as_str(),
                lines: jet_foundation::Diagnostics::LineIndex::new(text),
            };
            (path.clone(), source)
        })
        .collect();
    // Debug sweep (off by default): `JET_DEBUG_LOWER_ALL=1` lowers every
    // function even after one fails, prints one line per failure, then
    // returns the first failure exactly as the default path would.
    let sweep = std::env::var_os("JET_DEBUG_LOWER_ALL").is_some_and(|value| value != "0");
    let mut sweep_failures: Vec<LowerError> = Vec::new();
    for index in 0..program.funcs.len() {
        let function = &program.funcs[index];
        let lower = || {
            lower_function(
                function,
                &types,
                &nominal_identities,
                &program.reflect_paths,
                &program.declarations.traits,
                &function_registry,
                &indexed_sources,
                &program.artifact_facts.modules,
                None,
                None,
            )
            .map_err(|error| {
                LowerError::new(error.span, format!("{}: {}", function.name, error.message))
            })
        };
        let result = if sweep {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(lower)).unwrap_or_else(
                |payload| {
                    let message = payload
                        .downcast_ref::<String>()
                        .map(String::as_str)
                        .or_else(|| payload.downcast_ref::<&str>().copied())
                        .unwrap_or("non-string panic");
                    Err(LowerError::new(
                        function.source_span,
                        format!("{}: panic: {message}", function.name),
                    ))
                },
            )
        } else {
            lower()
        };
        let (lowered, calls, files, instances, nested, function_callbacks) = match result {
            Ok(lowered) => lowered,
            Err(error) if sweep => {
                report_lower_sweep_failure(&indexed_sources, &function.source_file, &error);
                sweep_failures.push(error);
                continue;
            }
            Err(error) => return Err(error),
        };
        // Merge the function's rows into the program. Under the debug sweep a
        // conflicting row is one more reported failure, not the end of the run.
        let merge = || -> Result<(), LowerError> {
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
                if let Some(existing) =
                    prelude_call_positions.get(&call.id).map(|position| &prelude_calls[*position])
                {
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
                    prelude_call_positions.insert(call.id, prelude_calls.len());
                    prelude_calls.push(call);
                }
            }
            for callback in function_callbacks {
                if let Some(existing) =
                    callback_positions.get(&callback.id).map(|position| &callbacks[*position])
                {
                    if format!("{existing:?}") != format!("{callback:?}") {
                        return Err(LowerError::new(
                            function.source_span,
                            format!("conflicting semantic callback row {:?}", callback.id),
                        ));
                    }
                } else {
                    callback_positions.insert(callback.id, callbacks.len());
                    callbacks.push(callback);
                }
            }
            // `lower_function` names files by path only; the text is attached
            // once per file here instead of being cloned and compared per
            // function.
            for file in files {
                if let Some(existing) =
                    source_file_positions.get(&file.id).map(|position| &source_files[*position])
                {
                    if existing.path != file.path {
                        return Err(LowerError::new(
                            function.source_span,
                            format!("conflicting source-file row {:?}", file.id),
                        ));
                    }
                } else {
                    let source = program
                        .source_files
                        .get(&file.path)
                        .cloned()
                        .unwrap_or_default();
                    source_file_positions.insert(file.id, source_files.len());
                    source_files.push(MirSourceFile { source, ..file });
                }
            }
            Ok(())
        };
        let merged = if sweep {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(merge)).unwrap_or_else(|_| {
                Err(LowerError::new(function.source_span, "panic while merging rows"))
            })
        } else {
            merge()
        };
        match merged {
            Ok(()) => {}
            Err(error) if sweep => {
                let error = LowerError::new(
                    error.span,
                    format!("{}: merge: {}", function.name, error.message),
                );
                report_lower_sweep_failure(&indexed_sources, &function.source_file, &error);
                sweep_failures.push(error);
            }
            Err(error) => return Err(error),
        }
        // Only this function's lowering reads its body.
        program.funcs[index].body = Vec::new();
    }
    if let Some(first) = sweep_failures.first() {
        eprintln!(
            "JET_DEBUG_LOWER_ALL summary: {} of {} functions failed to lower",
            sweep_failures.len(),
            program.funcs.len()
        );
        return Err(first.clone());
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
    let foreign = lower_foreign_rows(
        &program.artifact_facts.foreign,
        &program.funcs,
        &function_registry,
    );
    let mut handles = lower_handle_rows(
        &program.artifact_facts.handles,
        &program.funcs,
        &foreign,
        &function_registry,
    );
    append_core_process_handle_rows(&mut handles, &prelude_calls);
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
    retain_selected_derives(
        &mut types,
        &impls,
        &program.artifact_facts.modules,
        program.artifact_facts.target,
    );
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
    let mut constants = lower_constant_rows(&program.declarations.constants);
    for row in program
        .declarations
        .constants
        .iter()
        .filter(|row| is_runtime_constant(row))
    {
        // A bare spelling resolves in the constant's own module first, so a
        // user `FontStyle` row never reads as a same-named Core declaration.
        let instance = module_relative_type_instance(&types, &row.ty, &row.module, row.span);
        // A prepared constant is materialized only when its folded value has
        // a canonical MIR type; reads of any other stay folded at the use.
        if row.is_comptime && instance.is_err() {
            constants.retain(|constant| constant.key != row.key);
            continue;
        }
        let instance = instance?;
        // The row carries the checked instance, so its nominal members name
        // the same type identities as the program's type table.
        if let Some(constant) = constants.iter_mut().find(|constant| constant.key == row.key) {
            constant.ty = instance.clone();
        }
        merge_type_instance(&mut type_instances, instance, row.span)?;
    }
    let jobs = lower_job_rows(
        &program.artifact_facts.jobs,
        &program.funcs,
        &function_registry,
        &functions,
    );
    let tests = lower_test_rows(
        &program.artifact_facts.tests,
        &program.funcs,
        &functions,
        &function_registry,
    )?;
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
        if !type_instances.contains(ty.id) {
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
    let mut type_instances = type_instances.into_rows();
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
    inline_known_constant_reads(&mut functions, &program.declarations.constants, &constants);

    Ok(MirProgram {
        schema_version: MIR_SCHEMA_VERSION,
        package_identity: program.package_identity.clone(),
        facts: lower_package_facts(&program),
        cffi: lower_cffi_facts(&program.artifact_facts.cffi),
        names: lower_name_facts(&program.artifact_facts.names),
        modules,
        imports,
        types,
        core_owners: lower_core_builtin_owners(),
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
        codec_migrations: program.codec_migrations.clone(),
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
    foreign: &[MirForeign],
    registry: &FunctionRegistry,
) -> Vec<MirHandleLifecycle> {
    let mut result = rows
        .iter()
        .map(|row| {
            let handle_id = MirHandleId(stable_id("mir-handle", &row.key));
            let close = foreign
                .iter()
                .filter(|candidate| {
                    candidate.handle == Some(handle_id)
                        && candidate.foreign_language == MirForeignLanguage::C
                })
                .find_map(|candidate| candidate.close_function)
                .or_else(|| {
                    row.close_function_key
                        .as_deref()
                        .and_then(|key| resolve_function_key(key, "", registry))
                })
                .or_else(|| {
                    row.close_function_key.as_deref().and_then(|key| {
                        functions
                            .iter()
                            .find(|function| function.key == key)
                            .and_then(|function| registry.id_for(function).ok())
                    })
                });
            let close_foreign = if close.is_some() {
                None
            } else {
                foreign
                    .iter()
                    .find(|candidate| {
                        candidate.handle == Some(handle_id)
                            && candidate.name == row.close
                            && candidate.foreign_language == MirForeignLanguage::C
                    })
                    .map(|candidate| candidate.id)
            };
            Some(MirHandleLifecycle {
                id: handle_id,
                ty: lower_type(&Type::Named(row.jet_name.clone())),
                ownership: MirHandleOwnership::Owned,
                payload: MirHandlePayload {
                    library: row.lib.clone(),
                    typedef_name: row.typedef_name.clone(),
                    close: row.close.clone(),
                },
                close_foreign,
                close_source: Some(row.close_source.clone()),
                thread_safety: Some(row.thread_safety.clone()),
                close,
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
        })
        .flatten()
        .collect::<Vec<_>>();
    result.sort_unstable_by_key(|row| row.id);
    result
}
/// Core process handles are runtime-owned rather than C-FFI facts, but the
/// interpreter still needs the same checked lifecycle identity for result
/// extraction, method calls, and drop cleanup.
fn append_core_process_handle_rows(
    handles: &mut Vec<MirHandleLifecycle>,
    prelude_calls: &[MirPreludeCall],
) {
    let needs_child = prelude_calls.iter().any(|row| {
        row.module == "core.handle"
            && (row.member == "process.spec.spawn" || row.member.starts_with("process.child."))
    });
    let needs_stdin = prelude_calls
        .iter()
        .any(|row| row.module == "core.handle" && row.member.starts_with("process.stdin_"));
    let rows = [
        (needs_child, "ProcessChild", "process.child.close"),
        (needs_stdin, "ProcessStdin", "process.stdin_close"),
    ];
    for (needed, typedef_name, close) in rows {
        if !needed
            || handles.iter().any(|row| {
                row.payload.library == "core.process" && row.payload.typedef_name == typedef_name
            })
        {
            continue;
        }
        let key = format!("core::core.process::{typedef_name}");
        handles.push(MirHandleLifecycle {
            id: MirHandleId(stable_id("mir-handle", &key)),
            ty: lower_type(&Type::Named(typedef_name.to_string())),
            ownership: MirHandleOwnership::Owned,
            payload: MirHandlePayload {
                library: "core.process".to_string(),
                typedef_name: typedef_name.to_string(),
                close: close.to_string(),
            },
            close: None,
            close_foreign: None,
            undo: None,
            send: false,
            sync: false,
            close_source: None,
            thread_safety: None,
        });
    }
    handles.sort_unstable_by_key(|row| row.id);
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
                bridge_eligible: row.bridge_eligible,
                raw_scalar_abi: row.raw_scalar_abi,
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
/// #3740 (D-FAILURE-FOUNDATION1): a callable whose failure set is empty
/// (`T Never!`, written or inferred) returns its success value directly on
/// every tier. Checked expressions keep naming the `Result<T, Never>`
/// carrier; this is that carrier's success type.
pub(super) fn never_carrier_success(ty: &Type) -> Option<&Type> {
    match ty.without_user_tags() {
        Type::Result { ok, err } if err.is_never() => Some(ok.as_ref()),
        _ => None,
    }
}

/// A checked `Result` carrier expression. #3708: callers checked before the
/// failure solve name the default `Result<T, Err>` of a callee that now
/// returns plainly, so reconciliation accepts any `Result` expression.
pub(super) fn is_result_carrier(ty: &Type) -> bool {
    matches!(ty.without_user_tags(), Type::Result { .. })
}

/// An executable return that is neither a failure nor an absence carrier.
pub(super) fn is_plain_return(ty: &Type) -> bool {
    !matches!(
        ty.without_user_tags(),
        Type::Result { .. } | Type::Option(_)
    ) && !ty.is_never()
}

/// #3740: whether a user callee's executable return `returned` is the plain
/// success value of the checked carrier `checked` its call site names. A
/// value return pairs with any `Result` carrier (#3708: a caller checked
/// before the failure solve names the default `Result<T, Err>`). An `Option`
/// return is plain only as the exact success of a `T? Never!` carrier, since
/// a callee that keeps its `T?` carrier returns that `Option` itself.
pub(super) fn is_plain_call_return(returned: &Type, checked: &Type) -> bool {
    if is_plain_return(returned) {
        return is_result_carrier(checked);
    }
    matches!(returned.without_user_tags(), Type::Option(_))
        && never_carrier_success(checked)
            .is_some_and(|success| success.without_user_tags() == returned.without_user_tags())
}

/// The adaptation a raw-payload function value needs to fill a callable slot
/// with the shared `Result` carrier: the slot parameters, the source payload
/// return, and the slot's carrier return. `None` when the two already agree.
fn fn_value_carrier_adaptation(source: &Type, slot: &Type) -> Option<(Vec<Type>, Type, Type)> {
    let (
        Type::Fn {
            params: source_params,
            ret: Some(source_ret),
            ..
        },
        Type::Fn {
            params: slot_params,
            ret: Some(slot_ret),
            ..
        },
    ) = (source.without_user_tags(), slot.without_user_tags())
    else {
        return None;
    };
    if source_params.len() != slot_params.len()
        || !matches!(slot_ret.as_ref(), Type::Result { .. })
        || matches!(source_ret.as_ref(), Type::Result { .. } | Type::Option(_))
        || matches!(source_ret.as_ref(), Type::Named(name) if name == crate::Syntax::TYPE_NEVER)
    {
        return None;
    }
    let Type::Fn {
        ret: Some(effective_ret),
        ..
    } = source.without_user_tags().with_effective_fn_returns()
    else {
        return None;
    };
    (effective_ret == *slot_ret).then(|| {
        (
            slot_params.clone(),
            source_ret.as_ref().clone(),
            slot_ret.as_ref().clone(),
        )
    })
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
fn synthesized_trait_associated_types(name: &str) -> Vec<MirAssociatedTypeDecl> {
    let names: &[&str] = match name.rsplit("::").next().unwrap_or(name) {
        crate::Syntax::TRAIT_ITERATOR => &["Item"],
        crate::Syntax::TRAIT_ITERABLE => &["Iter"],
        crate::Syntax::TRAIT_INDEX => &["Key", "Value"],
        crate::Syntax::TRAIT_ROLLBACK => &["Snapshot"],
        _ => &[],
    };
    names
        .iter()
        .map(|name| MirAssociatedTypeDecl {
            name: (*name).to_string(),
            span: Span::new(0, 0),
        })
        .collect()
}

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
            associated_types: synthesized_trait_associated_types(&trait_ref.name),
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
        let declared_self_type =
            module_relative_type_instance(types, &row.self_type, &row.module, row.span)?;
        let trait_name = row.trait_ref.as_ref().or(row.trait_name.as_ref());
        let mut method_groups: Vec<(MirType, Vec<MirFunctionId>)> = Vec::new();
        for method_key in &row.methods {
            let mut candidates = Vec::new();
            if let Some(id) = resolve_function_key(method_key, &row.module, registry) {
                if let Some(function) = functions
                    .iter()
                    .find(|function| registry.id_for(function).ok() == Some(id))
                {
                    candidates.push((id, function));
                }
            }
            for function in functions
                .iter()
                .filter(|function| impl_method_matches(function, row, method_key))
            {
                let id = registry.id_for(function)?;
                if !candidates.iter().any(|(candidate, _)| *candidate == id) {
                    candidates.push((id, function));
                }
            }
            for (id, function) in candidates {
                let Some((owner, _)) = tfunc_method_owner(function) else {
                    continue;
                };
                let owner = module_relative_type_instance(types, owner, &row.module, row.span)?;
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
        // A source impl that lost every method to Core reachability is not an
        // executable implementation. Preserve genuinely empty impl rows (for
        // example, traits implemented entirely by defaults).
        if !row.methods.is_empty()
            && method_groups.iter().all(|(_, methods)| methods.is_empty())
        {
            continue;
        }

        for (self_type, methods) in method_groups {
            let key = canonical_impl_key(&row.module, &self_type, trait_name.map(String::as_str));
            result.push(MirImplDef {
                id: MirImplId(stable_id("mir-impl", &key)),
                module: jet_foundation::MIR::MirModuleId(stable_id("mir-module", &row.module)),
                key,
                span: row.span,
                self_type,
                // A literal capability impl is an inherent block of its hook.
                trait_ref: trait_name
                    .filter(|name| !is_literal_capability_trait(name))
                    .map(|name| MirTraitRef {
                        id: MirTraitId(stable_id("mir-trait", name)),
                        name: name.clone(),
                    }),
                associated_types: row
                    .associated_types
                    .iter()
                    .map(|associated| {
                        Ok(MirAssociatedTypeValue {
                            name: associated.name.clone(),
                            ty: module_relative_type_instance(
                                types,
                                &associated.ty,
                                &row.module,
                                associated.span,
                            )?,
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
                    .map(|ty| module_relative_type_instance(types, ty, &row.module, row.span))
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
    // Separate inherent impl blocks share one canonical MIR identity. Merge their
    // checked method edges before validation rather than emitting duplicate IDs.
    let mut impl_indices = HashMap::<String, usize>::new();
    let mut deduplicated: Vec<MirImplDef> = Vec::with_capacity(result.len());
    for mut row in result {
        if let Some(index) = impl_indices.get(&row.key).copied() {
            let existing = &mut deduplicated[index];
            for method in row.methods.drain(..) {
                if !existing.methods.contains(&method) {
                    existing.methods.push(method);
                }
            }
        } else {
            let index = deduplicated.len();
            impl_indices.insert(row.key.clone(), index);
            deduplicated.push(row);
        }
    }
    let mut result = deduplicated;

    // Keep only checked method references; a method that did not enter the
    // executable TIR cannot be represented as a valid MIR edge. The checked
    // function ids are collected once (not rescanned per impl method).
    let checked_ids: HashSet<MirFunctionId> = functions
        .iter()
        .filter_map(|function| registry.id_for(function).ok())
        .collect();
    result.retain(|row| row.methods.iter().all(|id| checked_ids.contains(id)));
    result.sort_unstable_by_key(|row| row.id);
    Ok(result)
}

fn canonical_impl_key(module: &str, owner: &MirType, trait_name: Option<&str>) -> String {
    let qualified_display = canonical_target_type(module, &owner.display_name());
    let owner_display = if module.is_empty() {
        qualified_display.clone()
    } else {
        qualified_display
            .strip_prefix(&format!("{module}::"))
            .unwrap_or(&qualified_display)
            .to_string()
    };
    // Keep historical keys for plain nominal/scalar owners. Structured
    // instances (in particular `Owner<Args>`) retain their canonical MIR kind
    // identity as part of the key, so distinct generic owners cannot allocate
    // one impl ID merely because their display spellings agree.
    let owner = if owner.canonical_key() == qualified_display {
        owner_display
    } else {
        format!("{owner_display}__instance__{}", owner.canonical_key())
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

/// Every module constant a function body may read through a `Global`: runtime
/// constants and storage, and (D-PREP-SURFACE2=A) prepared constants whose
/// build-time value is folded. A plain ALL_CAPS read lowers to the same
/// `Global` either way, so every tier must find the value here.
fn is_runtime_constant(row: &super::tir_to_mir_types::TirConstantDef) -> bool {
    !(row.is_comptime && matches!(row.value, CtValue::Unit))
        && !matches!(
            &row.ty,
            Type::Named(name)
                if name == crate::Syntax::TYPE_OUTPUT
                    || name == crate::Syntax::TYPE_OUTPUT_DEFAULTS
        )
}

fn lower_constant_rows(rows: &[super::tir_to_mir_types::TirConstantDef]) -> Vec<MirConstantDef> {
    let mut result = rows
        .iter()
        .filter(|row| is_runtime_constant(row))
        .filter_map(|row| {
            Some(MirConstantDef {
                id: jet_foundation::MIR::MirConstantId(stable_id("mir-constant", &row.key)),
                module: jet_foundation::MIR::MirModuleId(stable_id("mir-module", &row.module)),
                key: row.key.clone(),
                name: row.name.clone(),
                span: row.span,
                visibility: lower_decl_visibility(row.visibility),
                ty: lower_type(&row.ty),
                is_storage: row.is_storage,
                value: lower_constant_value(&row.value)?,
            })
        })
        .collect::<Vec<_>>();
    result.sort_unstable_by_key(|row| row.id);
    result
}

/// The most scalar leaves a constant may hold and still be copied into each
/// use site; larger values stay one shared item.
const INLINE_CONSTANT_LEAVES: usize = 8;
/// The longest text a copied constant may carry per leaf.
const INLINE_CONSTANT_TEXT_BYTES: usize = 256;

/// Card #4258 (D-CONSTMARK1): a read of an immutable module constant whose
/// folded value is a scalar, text, or a small record or enum case of those
/// becomes a MIR `Constant` at the use site, so the optimizer can fold it and
/// no backend calls an accessor. `#Static` keeps the `Global` read of the
/// one addressable item; `#Inline` copies any value.
/// Tables stay one shared item; the emitter caches a large one.
fn inline_known_constant_reads(
    functions: &mut [MirFunction],
    rows: &[super::tir_to_mir_types::TirConstantDef],
    constants: &[MirConstantDef],
) {
    let rows = rows
        .iter()
        .map(|row| (row.key.as_str(), row))
        .collect::<HashMap<_, _>>();
    let values = constants
        .iter()
        .filter(|constant| !constant.is_storage)
        .filter_map(|constant| {
            let row = rows.get(constant.key.as_str())?;
            let copied = !row.addressable
                && (row.force_inline || inline_constant_fits(&constant.value, &mut 0));
            copied.then_some((constant.key.as_str(), &constant.value))
        })
        .collect::<HashMap<_, _>>();
    if values.is_empty() {
        return;
    }
    for function in functions {
        for block in &mut function.blocks {
            for instruction in &mut block.instructions {
                let MirOperation::Global { name } = &instruction.operation else {
                    continue;
                };
                if let Some(value) = values.get(name.as_str()) {
                    // A sized-integer read keeps its width, as a literal does.
                    let mut constant = (*value).clone();
                    if let MirConstant::Int { width: width @ None, .. } = &mut constant {
                        *width = instruction.ty.as_ref().and_then(MirType::fixed_int);
                    }
                    instruction.operation = MirOperation::Constant(constant);
                }
            }
        }
    }
}

fn inline_constant_fits(value: &MirConstant, leaves: &mut usize) -> bool {
    match value {
        MirConstant::Int { .. }
        | MirConstant::Float { .. }
        | MirConstant::Bool(_)
        | MirConstant::Char(_)
        | MirConstant::Unit
        | MirConstant::BigInt(_) => {}
        MirConstant::String(text) if text.len() <= INLINE_CONSTANT_TEXT_BYTES => {}
        MirConstant::Struct { fields, .. } => {
            return fields
                .iter()
                .all(|(_, field)| inline_constant_fits(field, leaves));
        }
        MirConstant::Enum { args, .. } => {
            return args.iter().all(|(_, arg)| inline_constant_fits(arg, leaves));
        }
        MirConstant::Present(inner) => return inline_constant_fits(inner, leaves),
        _ => return false,
    }
    *leaves += 1;
    *leaves <= INLINE_CONSTANT_LEAVES
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
    lowered: &[MirFunction],
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
                inputs: match &row.record_inputs {
                    // One `#CLI` record parameter: its fields are the inputs,
                    // lowered exactly like a record entry's.
                    Some(record) => record
                        .iter()
                        .enumerate()
                        .map(|(ordinal, input)| lower_cli_input(input, ordinal, None))
                        .collect(),
                    None => {
                        // Each input binds one checked job parameter and carries
                        // that parameter's canonical MIR type.
                        let params = lowered
                            .iter()
                            .find(|candidate| candidate.id == function)
                            .map(|candidate| candidate.params.as_slice())
                            .unwrap_or(&[]);
                        row.arguments
                            .iter()
                            .enumerate()
                            .map(|(index, argument)| {
                                lower_job_argument(
                                    argument,
                                    index,
                                    params.get(index).map(|param| param.ty.clone()),
                                )
                            })
                            .collect()
                    }
                },
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

/// The command-line value kind of a checked job parameter type.
fn job_value_kind(ty: &Type) -> MirCliValueKind {
    match ty {
        Type::Option(inner) => job_value_kind(inner),
        Type::Bool => MirCliValueKind::Bool,
        Type::Int | Type::IntN { .. } => MirCliValueKind::Int,
        Type::Float | Type::Float32 => MirCliValueKind::Float,
        Type::Named(name) if name == crate::Syntax::TYPE_PATH => MirCliValueKind::Path,
        _ => MirCliValueKind::String,
    }
}

/// One scalar job parameter as a command input. The shape follows the `#CLI`
/// record field rules (`CLISchema::command_schema_with_items`): `Bool` is a
/// flag, `?T` is optional, a default makes a non-optional value with that
/// default, and a required value fills positionally.
fn lower_job_argument(
    argument: &TirJobArgument,
    index: usize,
    lowered_ty: Option<jet_foundation::MIR::MirType>,
) -> MirCliInput {
    let flag = matches!(argument.param_ty, Type::Bool) && !argument.variadic;
    let optional = matches!(argument.param_ty, Type::Option(_));
    let default = argument
        .default
        .as_ref()
        .filter(|_| !optional)
        .map(|value| MirCliDefault::Value(MirConstant::String(value.clone())));
    MirCliInput {
        parameter: index,
        name: argument.name.clone(),
        label: argument.label.clone(),
        ty: lowered_ty.unwrap_or_else(|| lower_type(&argument.param_ty)),
        zone: lower_param_zone(argument.zone),
        short: None,
        env: None,
        // Same flag, help, and metavar text a `#CLI` record field gets, so a
        // job's inputs pass the one CLI legality rule and render identically.
        help: format!("value for --{}", argument.name),
        metavar: (!flag).then(|| argument.name.replace('-', "_").to_uppercase()),
        shape: if flag {
            MirCliInputShape::Flag
        } else {
            MirCliInputShape::Value {
                kind: job_value_kind(&argument.param_ty),
                optional,
                default,
            }
        },
        positional: (argument.required || argument.variadic).then_some(index as u16),
        variadic: argument.variadic,
        flag: argument.name.clone(),
    }
}

fn lower_test_rows(
    rows: &[TirTestFact],
    functions: &[TFunc],
    checked_functions: &[MirFunction],
    registry: &FunctionRegistry,
) -> Result<Vec<MirTestCase>, LowerError> {
    let mut result = Vec::new();
    for row in rows {
        let Some(function) = resolve_function_ref(&row.function, functions, registry) else {
            continue;
        };
        let checked = checked_functions.iter().find(|candidate| candidate.id == function)
            .ok_or_else(|| LowerError::new(row.span, "test target has no checked MIR signature"))?;
        let mut parameters = Vec::with_capacity(row.parameters.len());
        for parameter in &row.parameters {
            let checked_parameter = checked.params.iter()
                .find(|candidate| candidate.index == parameter.index)
                .ok_or_else(|| LowerError::new(parameter.span, "test parameter has no checked MIR signature slot"))?;
            parameters.push(MirParam {
                index: checked_parameter.index,
                name: parameter.name.clone(),
                span: parameter.span,
                ty: checked_parameter.ty.clone(),
                access: checked_parameter.access,
                ownership: checked_parameter.ownership,
                public_label: parameter.label.clone(),
                variadic: checked_parameter.variadic,
                default_present: checked_parameter.default_present,
            });
        }
        result.push(MirTestCase {
            id: jet_foundation::MIR::MirTestId(stable_id("mir-test", &row.key)),
            function,
            name: row.name.clone(),
            span: row.span,
            kind: match row.kind {
                TirTestKind::Unit => MirTestKind::Unit,
                TirTestKind::Property => MirTestKind::Property,
            },
            parameters,
            faults: row.faults.clone(),
            expected_failure: row.expected_failure,
            contract_generated: row.contract_generated,
            eligibility: row.eligibility.as_ref()
                .and_then(|reference| resolve_function_ref(reference, functions, registry)),
            generation_unavailable_reason: row.generation_unavailable_reason.clone(),
        });
    }
    result.sort_unstable_by_key(|row| row.id);
    Ok(result)
}

fn function_block_id(
    reference: &TirFunctionRef,
    block: Option<&str>,
    functions: &[TFunc],
    registry: &FunctionRegistry,
) -> Option<(MirFunctionId, MirBlockId)> {
    let function = functions.iter().find(|function| {
        function.source_span == reference.span
            && (function.key == reference.key
                || (function.module == reference.module && function.name == reference.name))
    })?;
    let function_id = registry.id_for(function).ok()?;
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
                    let (function, block) = function_block_id(
                        &point.function,
                        point.block.as_deref(),
                        functions,
                        registry,
                    )?;
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
        record_inputs: entry.record_inputs,
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
        // Artifact entry references carry the canonical module-qualified
        // semantic key. Resolve that identity through the top-level index so
        // loader aliases and generated Rust spellings cannot select a method
        // or an unrelated same-named function.
        function: spec
            .function
            .as_ref()
            .and_then(|reference| registry.resolve_entry(&reference.key, reference.span).ok()),
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

/// Resolve the source-facing target of an unqualified import to the same
/// canonical key used by its `TirModuleFact` row.
///
/// File-module imports carry only their checked alias when the import itself
/// is unqualified, while inline modules are represented as children of the
/// importing module. Prefer an exact canonical key, then the importing
/// module's child path, and finally a unique module name. An ambiguous name
/// remains unchanged rather than guessing a target.
fn canonical_module_key(modules: &[TirModuleFact], owner: &str, target: &str) -> String {
    if target.is_empty() {
        return target.to_string();
    }
    if modules.iter().any(|row| row.key == target) {
        return target.to_string();
    }

    let qualified = target
        .split('.')
        .fold(owner.to_string(), |mut key, segment| {
            if !segment.is_empty() {
                if !key.is_empty() {
                    key.push_str("::");
                }
                key.push_str(segment);
            }
            key
        });
    if modules.iter().any(|row| row.key == qualified) {
        return qualified;
    }

    let mut matches = modules.iter().filter(|row| row.name == target);
    match (matches.next(), matches.next()) {
        (Some(row), None) => row.key.clone(),
        _ => target.to_string(),
    }
}

fn lower_import_item(
    item: &TirImportItem,
    target_alias: &str,
    target_module: &str,
    types: &[MirTypeDef],
    traits: &[MirTraitDef],
    impls: &[MirImplDef],
    constants: &[MirConstantDef],
    foreign: &[MirForeign],
    functions: &[TFunc],
    registry: &FunctionRegistry,
) -> Option<jet_foundation::MIR::MirItemRef> {
    let resolved = lower_module_item(
        &item.item,
        types,
        traits,
        impls,
        constants,
        foreign,
        functions,
        registry,
        target_module,
    );
    if resolved.is_some() {
        return resolved;
    }

    let TirItemRef::Unknown(raw) = &item.item else {
        return None;
    };
    let suffix = raw
        .strip_prefix(target_alias)
        .and_then(|rest| rest.strip_prefix("::").or_else(|| rest.strip_prefix('.')))
        .filter(|suffix| !suffix.is_empty())
        .or_else(|| raw.rsplit_once('.').map(|(_, leaf)| leaf))
        .or_else(|| raw.rsplit_once("::").map(|(_, leaf)| leaf))
        .unwrap_or(item.original.as_str());
    let canonical = format!("{target_module}::{suffix}");
    lower_module_item(
        &TirItemRef::Unknown(canonical),
        types,
        traits,
        impls,
        constants,
        foreign,
        functions,
        registry,
        target_module,
    )
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
                TirImportKind::Unqualified { module, items } => {
                    let module_key = canonical_module_key(modules, &row.module, module);
                    MirImportKind::Unqualified {
                        module: jet_foundation::MIR::MirModuleId(stable_id(
                            "mir-module",
                            &module_key,
                        )),
                        items: items
                            .iter()
                            .filter_map(|item| {
                                Some(MirImportItem {
                                    original: item.original.clone(),
                                    local: item.local.clone(),
                                    item: lower_import_item(
                                        item,
                                        module,
                                        &module_key,
                                        types,
                                        traits,
                                        impls,
                                        constants,
                                        foreign,
                                        functions,
                                        registry,
                                    )?,
                                })
                            })
                            .collect(),
                    }
                }
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
                    redact: false,
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
    lower_checked_mir_program_for_with_debug(bundle, request, false)
}

/// Lower checked MIR with the native debug source-map markers enabled.
pub fn lower_checked_mir_program_for_with_debug(
    bundle: &crate::AST::ProgramBundle,
    request: MirArtifactRequest,
    debug_linemap: bool,
) -> Result<(MirProgram, MirArtifactId), LowerError> {
    let target = request.target;
    let kind = request.kind;
    let tir = super::lower_checked_tir_program_for_with_debug(bundle, request, debug_linemap)?;
    let mut mir = lower_tir_to_mir(tir)?;
    // Compile time is explicit in the checker, so ordinary immutable bindings
    // over pure work are folded here, once, for every execution tier.
    crate::Codegen::MIREval::fold_pure_calls(&mut mir);
    let mir = jet_foundation::MIR::optimize_mir_program(
        mir,
        &jet_foundation::MIR::MirOptimizationPolicy::conservative(),
    )
    .map_err(|error| {
        LowerError::new(
            Span::new(0, 0),
            format!("canonical MIR optimization failed: {error}"),
        )
    })?;
    // Debug hook for the Jet backend harness (Compiler/JetBackend/Tests):
    // JET_DUMP_MIR=<path> writes the canonical program's Debug form. Inert
    // when unset.
    if let Some(path) = std::env::var_os("JET_DUMP_MIR") {
        let _ = std::fs::write(path, format!("{mir:#?}"));
    }
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

fn declared_type_instances(types: &[MirTypeDef]) -> TypeInstances {
    let mut instances = TypeInstances::default();
    instances.extend(
        types
            .iter()
            .map(|ty| lower_type(&Type::Named(ty.key.clone())).with_identity(ty.id)),
    );
    instances
}

/// Canonical type instances in insertion order, indexed by identity so that
/// merging a row never rescans the list. `positions` keeps the first row per
/// identity, the row a linear `find` would return.
#[derive(Default)]
pub(super) struct TypeInstances {
    rows: Vec<MirType>,
    positions: HashMap<MirTypeId, usize>,
}

impl TypeInstances {
    fn get(&self, identity: MirTypeId) -> Option<&MirType> {
        self.positions.get(&identity).map(|position| &self.rows[*position])
    }

    fn contains(&self, identity: MirTypeId) -> bool {
        self.positions.contains_key(&identity)
    }

    fn push(&mut self, instance: MirType) {
        if let Some(identity) = instance.identity {
            self.positions.entry(identity).or_insert(self.rows.len());
        }
        self.rows.push(instance);
    }

    /// Appends every row, duplicates included (the program merge checks them).
    pub(super) fn extend(&mut self, instances: impl IntoIterator<Item = MirType>) {
        for instance in instances {
            self.push(instance);
        }
    }

    pub(super) fn into_rows(self) -> Vec<MirType> {
        self.rows
    }
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

fn ensure_type_instance(instances: &mut TypeInstances, instance: MirType) {
    let Some(identity) = instance.identity else {
        return;
    };
    if !instances.contains(identity) {
        instances.push(instance);
    }
}

fn merge_nested_type_instances(
    instances: &mut TypeInstances,
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
        MirTypeKind::SendFn { params, ret, .. } => {
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
    instances: &mut TypeInstances,
    instance: MirType,
    span: Span,
) -> Result<(), LowerError> {
    let Some(identity) = instance.identity else {
        return Err(LowerError::new(
            span,
            "MIR type instance has no canonical identity",
        ));
    };
    if let Some(existing) = instances.get(identity) {
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
        target_dossier: program.artifact_facts.build.target_dossier.clone(),
        web_app: program.facts.web_app.clone(),
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
            if name.contains("::")
                || super::tir_to_mir_types::is_compiler_owned_trait(name)
                || name == crate::Generics::CHECKED_TEXT
            {
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
            ..
        } if is_literal_capability_trait(trait_name) => Ok(MirFunctionForm::Method {
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

/// D-FOUND-LITERAL1=A: `Literal.Int` and `Literal.Float` are compiler-intrinsic
/// capabilities, not runtime traits. Their `from_literal` hook is an ordinary
/// static method of the implementing type in every artifact.
fn is_literal_capability_trait(name: &str) -> bool {
    name.rsplit("::")
        .next()
        .is_some_and(crate::Generics::is_literal_capability)
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
    reflect_paths: &HashMap<String, String>,
    trait_defs: &[super::tir_to_mir_types::TirTraitDef],
    function_registry: &FunctionRegistry,
    source_texts: &BTreeMap<String, IndexedSource<'_>>,
    modules: &[TirModuleFact],
    body: Option<&TLambdaBody>,
    lambda: Option<&TLambda>,
) -> Result<LoweredFunction, LowerError> {
    let mut ctx = LowerCtx::new(
        f,
        type_defs,
        nominal_identities,
        reflect_paths,
        trait_defs,
        function_registry,
        source_texts,
        modules,
    );
    let source_file = ctx.source_file_id_for(&f.source_file);

    if let Some(lambda) = lambda {
        ctx.bind_captures(lambda)?;
    }
    let mut param_index = 0;
    if let Some((owner_type, conv)) = f.kind.receiver() {
        ctx.bind_parameter(0, jet_foundation::Syntax::KW_SELF, owner_type, conv)?;
        param_index = 1;
    }
    for (offset, (name, ty, access)) in f.params.iter().enumerate() {
        ctx.bind_parameter(param_index + offset, name, ty, *access)?;
    }
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
    ctx.seal_all_exit_chains()?;
    ctx.move_last_uses()?;
    ctx.resolve_drop_flags();

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
    optimization.checked_vector_facts = ctx.checked_vector_facts;
    let mut scopes = ctx.scopes;
    scopes.sort_unstable_by_key(|scope| scope.id);
    let mut blocks = ctx.blocks;
    blocks.sort_unstable_by_key(|block| block.id);
    let mir_function = MirFunction {
        id: function_id,
        module_id: jet_foundation::MIR::MirModuleId(stable_id("mir-module", &f.module)),
        source_file,
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
        ctx.type_instances.into_rows(),
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
        Type::Fn {
            params,
            ret,
            call_metadata,
            ..
        } => {
            let conventions = (0..params.len())
                .map(|index| {
                    call_metadata
                        .as_ref()
                        .and_then(|metadata| metadata.conventions.get(index))
                        .copied()
                        .unwrap_or(AccessConvention::Read)
                })
                .map(|convention| match convention {
                    AccessConvention::Read => 'R',
                    AccessConvention::Write => 'W',
                    AccessConvention::Move => 'M',
                })
                .collect::<String>();
            let params = params
                .iter()
                .map(type_identity_key)
                .collect::<Vec<_>>()
                .join(",");
            let ret = ret
                .as_deref()
                .map(type_identity_key)
                .unwrap_or_else(|| "Unit".to_string());
            format!("Fn({params})->{ret};conventions={conventions}")
        }
        Type::Named(name) if name == crate::Syntax::TYPE_TASKGROUP => {
            BUILTIN_TASK_GROUP_IDENTITY_KEY.to_string()
        }
        Type::Named(name) => name.clone(),
        Type::Apply { name, args } if args.is_empty() && name == crate::Syntax::TYPE_TASKGROUP => {
            BUILTIN_TASK_GROUP_IDENTITY_KEY.to_string()
        }
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

/// The bare `Group` is the compiler-owned task-group handle. The same-leaf
/// Core record (`core.data::Group<K, V>`) is generic and always spelled with
/// arguments, so it never claims the bare spelling; only a non-generic checked
/// row keyed exactly `Group` may.
fn is_builtin_task_group(type_defs: &[MirTypeDef], name: &str) -> bool {
    name == crate::Syntax::TYPE_TASKGROUP
        && !type_defs
            .iter()
            .any(|row| row.key == name && row.generic_params.is_empty())
}

/// The task-group handle's identity key. Every lowering path derives the
/// handle's `mir-type` identity from it, so it must differ from the generic
/// `Group<K, V>` record row keyed `Group`; otherwise runtime adapters resolve
/// the handle to that record's field layout (#3779).
const BUILTIN_TASK_GROUP_IDENTITY_KEY: &str = "builtin:task-group";

/// A bare built-in boundary spelling (`Path`, `URL`, `DateTime`) names only its
/// carrier record (`core.net.url`'s `URL`); a loaded record that merely shares
/// the leaf (`core.files.path`'s `Path`) is a different type and never claims
/// the bare spelling. Every other name may match by leaf.
fn row_claims_bare_name(row: &MirTypeDef, name: &str) -> bool {
    let Some(kind) = crate::Syntax::typed_head_kind(name) else {
        return true;
    };
    kind.carrier_record_module()
        .and_then(jet_foundation::CoreModuleExports::core_source_module)
        .is_some_and(|source| row.key.ends_with(&format!("{}::{name}", source.path)))
}

fn canonical_nominal_name(
    type_defs: &[MirTypeDef],
    name: &str,
    span: Span,
) -> Result<String, LowerError> {
    if let Some(carrier) = crate::Codegen::TIR::tir_to_mir_types::core_term_stream_carrier(name) {
        return Ok(carrier.to_string());
    }
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
    // D-UNIONTYPE1=A: a module-qualified anonymous-union spelling names the
    // one structural carrier row (`fold_anonymous_union_declarations`).
    if let Some(leaf) = name
        .rsplit("::")
        .next()
        .filter(|leaf| *leaf != name && leaf.starts_with("__JetUnion_"))
    {
        if type_defs.iter().any(|ty| ty.key == leaf) {
            return Ok(leaf.to_string());
        }
    }
    let matches = type_defs
        .iter()
        .filter(|ty| ty.name == name && row_claims_bare_name(ty, name))
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
        Type::Named(name) if is_builtin_task_group(type_defs, name) => ty.clone(),
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
        Type::Named(name) if is_builtin_task_group(type_defs, name) => {
            MirTypeId(stable_id("mir-type", &key))
        }
        Type::Named(name) => type_defs
            .iter()
            .find(|row| row.key == *name || (row.name == *name && row_claims_bare_name(row, name)))
            .map(|row| row.id)
            .unwrap_or_else(|| MirTypeId(stable_id("mir-type", &key))),
        Type::Apply { name, args } if args.is_empty() => type_defs
            .iter()
            .find(|row| row.key == *name || (row.name == *name && row_claims_bare_name(row, name)))
            .map(|row| row.id)
            .unwrap_or_else(|| MirTypeId(stable_id("mir-type", &key))),
        _ => MirTypeId(stable_id("mir-type", &key)),
    };
    Ok(lower_type(&source).with_identity(identity))
}
fn module_relative_type_instance(
    type_defs: &[MirTypeDef],
    ty: &Type,
    module: &str,
    span: Span,
) -> Result<MirType, LowerError> {
    let qualified = ty.map_named_types(&|name| {
        if module.is_empty() || name.contains("::") || name.contains('.') {
            return None;
        }
        let candidate = format!("{module}::{name}");
        type_defs
            .iter()
            .any(|definition| definition.key == candidate && definition.name == name)
            .then_some(candidate)
    });
    canonical_type_instance(type_defs, &qualified, span)
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
        MirTypeKind::SendFn { params, ret, .. } => Type::Fn {
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

#[derive(Clone)]
struct ContractScopeState {
    result: TContractResult,
    carrier_place: MirPlaceId,
    binding_place: MirPlaceId,
    post: Vec<TContract>,
}
#[derive(Debug, Clone, Copy)]
enum DeferredCleanup {
    /// Index into `LowerCtx::deferred_closes`.
    Close(usize),
    Guard(MirPlaceId),
    FileOwner { place: MirPlaceId, live: MirPlaceId },
    DropPlace {
        place: MirPlaceId,
        live: MirPlaceId,
        kind: MirDropKind,
    },
}

/// One `#Transact` snapshot: the snapshotted place, the local holding its
/// saved state, the snapshot type, and, for a type implementing `Rollback`,
/// the `Rollback::restore` call that replaces the plain write-back.
#[derive(Clone)]
pub(super) struct TransactionRestore {
    pub(super) place: MirPlaceId,
    pub(super) saved: MirPlaceId,
    pub(super) ty: Type,
    pub(super) custom: Option<TExpr>,
}

/// One `defer close(^resource)` action. The close expression is lowered into
/// each drop chain of its scope (and inline on the paths that clean up in
/// place), so the consuming `^resource` is an ordinary move of the resource
/// place at that point.
struct DeferredClose {
    close: TExpr,
    resource: String,
    place: MirPlaceId,
    ty: Type,
    span: Span,
}

#[derive(Debug, Default)]
struct DeferFrame {
    owner: Option<MirScopeId>,
    actions: Vec<DeferredCleanup>,
    /// Actions in `actions` other than `DropPlace`: the explicit ones, which
    /// run unconditionally and so only on exits registered after them.
    explicit: usize,
    /// Length of `LowerCtx::shadowed_locals` when the frame opened; leaving
    /// the frame restores every name rebound inside it.
    shadow_mark: usize,
    /// Drop-chain entry blocks exits from this frame jump to, filled when the
    /// frame closes (`LowerCtx::seal_exit_chains`).
    pending_exits: Vec<PendingExit>,
    /// Exits that leave this frame clean up inline: the frame's scope rewrites
    /// its exit blocks afterwards (scope members, transactions) or the Rust
    /// emitter tracks it per path (debug-only), so its exits never share a
    /// chain with other paths.
    inline_exits: bool,
}

impl DeferFrame {
    fn push_action(&mut self, action: DeferredCleanup) {
        if !matches!(action, DeferredCleanup::DropPlace { .. }) {
            self.explicit += 1;
        }
        self.actions.push(action);
    }
}

/// Where a shared drop chain ends: the exit terminator its entering exits
/// share. A returned value travels in its exit-value slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ExitTarget {
    Return(Option<MirPlaceId>),
    Break(MirBlockId),
    Continue(MirBlockId),
}

/// One drop-chain entry of a frame: exits toward `target` taken after the
/// frame's first `explicit` explicit actions. `cut` is the frame depth the
/// target's cleanup stops at.
#[derive(Debug)]
struct PendingExit {
    target: ExitTarget,
    explicit: usize,
    cut: usize,
    block: MirBlockId,
}

/// The binding a name had before a later source-named binding replaced
/// it in the name-keyed local maps. Source names are unique per function
/// except for a same-name refinement (D-FLOWTYPE1 `x == .Val(x)`), whose
/// payload binding ends with its branch or block. The outer binding comes
/// back when the inner one's scope ends.
#[derive(Debug)]
pub(super) struct ShadowedLocal {
    name: String,
    place: Option<MirPlaceId>,
    ty: Option<Type>,
    value: Option<MirValueId>,
}

/// Put `previous` back as `name`'s entry; returns the entry it replaces.
fn restore_entry<V>(map: &mut HashMap<String, V>, name: &str, previous: Option<V>) -> Option<V> {
    match previous {
        Some(value) => map.insert(name.to_string(), value),
        None => map.remove(name),
    }
}

pub(super) fn checked_operation_place_refs(operation: &MirOperation) -> Vec<MirPlaceId> {
    let mut places = match operation {
        MirOperation::ReadPlace(place)
        | MirOperation::MovePlace { place }
        | MirOperation::InitializeUninit { place }
        | MirOperation::RawAddressOf { place }
        | MirOperation::AddressOf { place, .. }
        | MirOperation::WritePlace { place, .. }
        | MirOperation::ReplacePlace { place, .. } => vec![*place],
        MirOperation::Closure { captures, .. } => captures
            .iter()
            .filter_map(|capture| match capture {
                MirCaptureOperand::Place(place) => Some(*place),
                MirCaptureOperand::Value(_) => None,
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

/// One checked source text with its line index, built once per program so
/// lowering reads positions without rescanning the text from byte 0.
pub(super) struct IndexedSource<'s> {
    pub(super) text: &'s str,
    pub(super) lines: jet_foundation::Diagnostics::LineIndex,
}

pub(super) struct LowerCtx<'a> {
    pub(super) function: &'a TFunc,
    pub(super) type_defs: &'a [MirTypeDef],
    pub(super) nominal_identities: &'a HashMap<String, String>,
    pub(super) reflect_paths: &'a HashMap<String, String>,
    pub(super) trait_defs: &'a [super::tir_to_mir_types::TirTraitDef],
    pub(super) function_registry: &'a FunctionRegistry,
    pub(super) trait_method_traits: HashMap<(String, String), String>,
    pub(super) source_texts: &'a BTreeMap<String, IndexedSource<'a>>,
    pub(super) modules: &'a [TirModuleFact],
    pub(super) entry: MirBlockId,
    pub(super) current: MirBlockId,
    pub(super) blocks: Vec<MirBasicBlock>,
    /// Position of each block in `blocks`, kept by `new_block`, so the
    /// per-instruction "current block" lookups stay constant time.
    pub(super) block_positions: HashMap<MirBlockId, usize>,
    /// Block position of each value's defining instruction (blocks only
    /// gain instructions, so a value never changes block).
    pub(super) value_blocks: HashMap<MirValueId, usize>,
    /// Values that are `None` with no payload of their own: `Absent`, and
    /// phis/copies/moves of only such values. TIR types them with a
    /// placeholder option; `retype_absent` gives each the exact option type
    /// of the slot it flows into.
    pub(super) absent_values: HashSet<MirValueId>,
    pub(super) locals: Vec<MirLocal>,
    pub(super) values: Vec<(MirValueId, MirType, Span, MirOwnership)>,
    pub(super) places: Vec<MirPlace>,
    pub(super) params: Vec<MirParam>,
    pub(super) capture_params: Vec<MirCaptureParam>,
    pub(super) capture_facts: Option<MirCaptureFacts>,
    pub(super) scopes: Vec<MirScope>,
    pub(super) drops: Vec<MirDropAction>,
    pub(super) nested_functions: Vec<MirFunction>,
    pub(super) checked_vector_facts: Vec<MirVectorFact>,
    pub(super) loops: Vec<(Option<String>, MirBlockId, MirBlockId)>,
    loop_defer_depths: Vec<usize>,
    defer_stack: Vec<DeferFrame>,
    /// The final block of each shared drop chain (see `ExitTarget`).
    exit_tails: HashMap<ExitTarget, MirBlockId>,
    /// Locals that carry a returned value through a shared drop chain, one
    /// per returned type.
    exit_value_slots: Vec<(MirType, MirPlaceId)>,
    deferred_closes: Vec<std::rc::Rc<DeferredClose>>,
    pub(super) transaction_restores: Vec<(MirScopeId, Vec<TransactionRestore>)>,
    contract_scopes: Vec<ContractScopeState>,
    pub(super) local_places: HashMap<String, MirPlaceId>,
    pub(super) local_types: HashMap<String, Type>,
    pub(super) local_values: HashMap<String, MirValueId>,
    /// Outer bindings replaced by a nested same-name binding, restored when
    /// the nested binding's frame or branch ends (see [`ShadowedLocal`]).
    shadowed_locals: Vec<ShadowedLocal>,
    /// D-OPT-WRITE1 (#3974): pattern subjects lowered from a `&place` write
    /// window, keyed by the subject value the pattern tests read. Payload
    /// bindings under such a subject alias the place through
    /// `MirProjection::Payload` instead of copying the payload out.
    pattern_windows: HashMap<MirValueId, MirPlaceId>,
    /// Generated locals bound with `name := &place` (a nested-pattern switch
    /// evaluates its `&place` subject once into one); a pattern on such a
    /// local is a write window like the `&place` itself.
    pub(super) window_aliases: HashSet<String>,
    /// D-MEM-COPYSEM1: window places rooted at a read parameter. A pattern
    /// on such a place tests it in place and binds each payload name as a
    /// read-only alias: the parameter cannot change while the function runs,
    /// so the alias reads exactly what a copy would hold.
    read_windows: HashSet<MirPlaceId>,
    /// D-MEM-COPYSEM1: the `Parameter` values of the function's read
    /// parameters, so a read-parameter place is recognized without scanning
    /// the emitted instructions.
    read_parameter_values: HashSet<MirValueId>,
    pub(super) drop_live_places: HashMap<MirPlaceId, MirPlaceId>,
    /// Every file-owner leaf place registered for cleanup in this function.
    pub(super) file_owner_places: Vec<MirPlaceId>,
    send_fn_locals: HashSet<String>,
    capture_values: HashMap<String, MirValueId>,
    pub(super) prelude_calls: Vec<MirPreludeCall>,
    /// Position of each id's first `prelude_calls` row.
    prelude_call_positions: HashMap<MirPreludeCallId, usize>,
    pub(super) callbacks: Vec<MirCallbackAdapter>,
    pub(super) source_files: HashMap<String, MirSourceFileId>,
    pub(super) type_instances: TypeInstances,
    pub(super) current_span: Span,
    pub(super) current_line: Option<u32>,
    switch_subject: Option<MirValueId>,
    identity_counts: HashMap<String, usize>,
    /// #3740: address of the `Try` operand whose plain-return call may hand
    /// its success value straight to that `Try`. Argument calls lowered
    /// beneath it never share the address.
    pub(super) plain_try_operand: Option<usize>,
    pub(super) plain_try_taken: bool,
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
            if matches!(
                name.as_str(),
                crate::Syntax::TYPE_SHARED_GUARD
                    | "CellReadGuard"
                    | "CellEditGuard"
                    | crate::Syntax::TYPE_PIN
            ) && args.len() == 1 =>
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
            if matches!(
                name.name.as_str(),
                crate::Syntax::TYPE_SHARED_GUARD
                    | "CellReadGuard"
                    | "CellEditGuard"
                    | crate::Syntax::TYPE_PIN
            ) && args.len() == 1 =>
        {
            field_owner_mir_type(&args[0])
        }
        _ => ty,
    }
}
fn is_shared_guard_mir_type(ty: &MirType) -> bool {
    match ty.kind() {
        MirTypeKind::Tagged { inner, .. } => is_shared_guard_mir_type(inner),
        MirTypeKind::Apply { name, .. } => name.name.as_str() == crate::Syntax::TYPE_SHARED_GUARD,
        _ => false,
    }
}

fn tuple_field_name<'a, T>(fields: &'a [(String, T)], key: &str) -> Option<&'a str> {
    if let Ok(index) = key.parse::<usize>() {
        return fields.get(index).map(|(name, _)| name.as_str());
    }
    fields
        .iter()
        .find(|(name, _)| name == key)
        .map(|(name, _)| name.as_str())
}

fn math_field_name<'a>(ty: &'a MirTypeDef, key: &str) -> Option<&'a str> {
    let index = key.parse::<usize>().ok()?;
    if !matches!(
        ty.name.as_str(),
        "F32x4" | "F64x2" | "Vec2" | "Vec3" | "Vec4"
    ) {
        return None;
    }
    let MirTypeDefKind::Struct { fields, .. } = &ty.kind else {
        return None;
    };
    fields.get(index).and_then(|field| {
        matches!(field.name.as_str(), "x" | "y" | "z" | "w").then_some(field.name.as_str())
    })
}

/// A place row can be referenced by several operations. Keep the strongest
/// access ever required so a later read cannot invalidate an earlier borrow or
/// move operation.
pub(super) fn retain_place_access(place: &mut MirPlace, requested: MirAccess) {
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
        reflect_paths: &'a HashMap<String, String>,
        trait_defs: &'a [super::tir_to_mir_types::TirTraitDef],
        function_registry: &'a FunctionRegistry,
        source_texts: &'a BTreeMap<String, IndexedSource<'a>>,
        modules: &'a [TirModuleFact],
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
            reflect_paths,
            type_defs,
            trait_defs,
            function_registry,
            trait_method_traits: HashMap::new(),
            source_texts,
            modules,
            entry,
            current: entry,
            blocks: vec![block],
            block_positions: HashMap::from([(entry, 0)]),
            value_blocks: HashMap::new(),
            absent_values: HashSet::new(),
            locals: Vec::new(),
            values: Vec::new(),
            places: Vec::new(),
            params: Vec::new(),
            capture_params: Vec::new(),
            capture_facts: None,
            scopes: Vec::new(),
            drops: Vec::new(),
            checked_vector_facts: Vec::new(),
            nested_functions: Vec::new(),
            loops: Vec::new(),
            loop_defer_depths: Vec::new(),
            defer_stack: vec![DeferFrame::default()],
            exit_tails: HashMap::new(),
            exit_value_slots: Vec::new(),
            deferred_closes: Vec::new(),
            transaction_restores: Vec::new(),
            contract_scopes: Vec::new(),
            local_places: HashMap::new(),
            local_types: HashMap::new(),
            local_values: HashMap::new(),
            shadowed_locals: Vec::new(),
            pattern_windows: HashMap::new(),
            window_aliases: HashSet::new(),
            read_windows: HashSet::new(),
            read_parameter_values: HashSet::new(),
            drop_live_places: HashMap::new(),
            file_owner_places: Vec::new(),
            send_fn_locals: HashSet::new(),
            capture_values: HashMap::new(),
            prelude_calls: Vec::new(),
            prelude_call_positions: HashMap::new(),
            callbacks: Vec::new(),
            source_files: HashMap::new(),
            type_instances: TypeInstances::default(),
            current_span: function.source_span,
            current_line: None,
            switch_subject: None,
            identity_counts,
            plain_try_operand: None,
            plain_try_taken: false,
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
    pub(super) fn trait_method_return_type(
        &self,
        trait_name: &str,
        method_name: &str,
    ) -> Result<Type, LowerError> {
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
        row.methods
            .iter()
            .find(|method| method.name == method_name)
            .map(|method| method.return_type.clone())
            .ok_or_else(|| {
                self.error(
                    self.span(),
                    format!(
                        "missing checked method `{method_name}` on trait `{}`",
                        row.name
                    ),
                )
            })
    }
    pub(super) fn is_trait_name(&self, name: &str) -> bool {
        self.trait_defs
            .iter()
            .any(|row| row.key == name || row.name == name)
    }

    fn reserve_identity_occurrence(&mut self, base: &str) -> usize {
        let ordinal = self.identity_counts.entry(base.to_string()).or_insert(0);
        let occurrence = *ordinal;
        *ordinal += 1;
        occurrence
    }

    pub(super) fn reserve_identity(
        &mut self,
        kind: &str,
        span: Span,
        role: &str,
        detail: &str,
    ) -> Result<String, LowerError> {
        let base = construct_identity(self.function, kind, span, role, detail);
        let occurrence = self.reserve_identity_occurrence(&base);
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

    pub(super) fn loop_body_blocks(
        &self,
        header: MirBlockId,
        body: MirBlockId,
        exit: Option<MirBlockId>,
        advance: Option<MirBlockId>,
    ) -> Vec<MirBlockId> {
        let mut pending = vec![body];
        let mut seen = HashSet::new();
        let mut region = Vec::new();
        while let Some(block_id) = pending.pop() {
            if block_id == header
                || exit == Some(block_id)
                || advance == Some(block_id)
                || !seen.insert(block_id)
            {
                continue;
            }
            let Some(block) = self.block_by_id(block_id) else {
                continue;
            };
            region.push(block_id);
            pending.extend(block.terminator.targets());
        }
        region.sort_unstable();
        region
    }

    /// Return the checked place compared by a counted-loop condition.
    ///
    /// Canonicalized range loops use `local < end`/`local <= end`; retaining
    /// the place here lets the source row carry the same stable cursor scope
    /// that MIR's scalar loop analysis recovers after lowering.
    pub(super) fn loop_cursor_place(&self, condition: &TExpr) -> Option<MirPlaceId> {
        let TExprKind::Binary { lhs, .. } = &condition.kind else {
            return None;
        };
        let TExprKind::Local(local) = &lhs.kind else {
            return None;
        };
        self.local_places.get(&local.name).copied()
    }

    pub(super) fn checked_loop_cursor(
        &self,
        body_blocks: &[MirBlockId],
        advance: Option<MirBlockId>,
        cursor_place: Option<MirPlaceId>,
        hinted: Option<MirValueId>,
    ) -> Option<MirValueId> {
        if hinted.is_some() {
            return hinted;
        }
        let mut reads = Vec::new();
        for block_id in body_blocks {
            if advance == Some(*block_id) {
                continue;
            }
            let Some(block) = self.block_by_id(*block_id) else {
                continue;
            };
            for instruction in &block.instructions {
                if let (Some(result), MirOperation::ReadPlace(place)) =
                    (instruction.result, &instruction.operation)
                {
                    if cursor_place == Some(*place) {
                        reads.push(result);
                    }
                }
            }
        }
        reads
            .iter()
            .copied()
            .find(|value| self.checked_cursor_value_is_indexed(body_blocks, advance, *value))
            .or_else(|| reads.into_iter().next())
    }

    fn checked_cursor_value_is_indexed(
        &self,
        body_blocks: &[MirBlockId],
        advance: Option<MirBlockId>,
        cursor: MirValueId,
    ) -> bool {
        body_blocks
            .iter()
            .filter(|block_id| advance != Some(**block_id))
            .any(|block_id| {
                let Some(block) = self.block_by_id(*block_id) else {
                    return false;
                };
                block
                    .instructions
                    .iter()
                    .any(|instruction| match &instruction.operation {
                        MirOperation::Index { index, .. } => *index == cursor,
                        _ => checked_operation_place_refs(&instruction.operation)
                            .into_iter()
                            .any(|place_id| {
                                self.places
                                    .iter()
                                    .find(|place| place.id == place_id)
                                    .is_some_and(|place| {
                                        place.projections.iter().any(|projection| {
                                        matches!(
                                            projection,
                                            MirProjection::Index { index, .. } if *index == cursor
                                        )
                                    })
                                    })
                            }),
                    })
            })
    }

    fn checked_cursor_matches(
        &self,
        value: MirValueId,
        cursor: MirValueId,
        cursor_place: Option<MirPlaceId>,
        seen: &mut HashSet<MirValueId>,
    ) -> bool {
        if value == cursor || !seen.insert(value) {
            return value == cursor;
        }
        let Some(instruction) = self
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find(|instruction| instruction.result == Some(value))
        else {
            return false;
        };
        match &instruction.operation {
            MirOperation::ReadPlace(place) => cursor_place == Some(*place),
            MirOperation::Copy { value, .. }
            | MirOperation::Move { value }
            | MirOperation::AttachTag { value, .. }
            | MirOperation::Convert { value, .. } => {
                self.checked_cursor_matches(*value, cursor, cursor_place, seen)
            }
            _ => false,
        }
    }

    fn checked_value_access_root(
        &self,
        value: MirValueId,
        seen: &mut HashSet<MirValueId>,
    ) -> MirVectorAccessRoot {
        if !seen.insert(value) {
            return MirVectorAccessRoot::Value(value);
        }
        let Some(instruction) = self
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find(|instruction| instruction.result == Some(value))
        else {
            return MirVectorAccessRoot::Value(value);
        };
        match &instruction.operation {
            MirOperation::ReadPlace(place) | MirOperation::MovePlace { place } => {
                MirVectorAccessRoot::Place(*place)
            }
            MirOperation::Copy { value, .. }
            | MirOperation::Move { value }
            | MirOperation::AttachTag { value, .. }
            | MirOperation::Field { base: value, .. }
            | MirOperation::Index { base: value, .. } => {
                self.checked_value_access_root(*value, seen)
            }
            MirOperation::Semantic(MirSemanticOp::ColumnarRead { base, .. }) => {
                self.checked_value_access_root(*base, seen)
            }
            _ => MirVectorAccessRoot::Value(value),
        }
    }

    fn checked_value_type(&self, value: MirValueId) -> Option<MirType> {
        self.values
            .iter()
            .find(|(candidate, ..)| *candidate == value)
            .map(|(_, ty, ..)| ty.clone())
    }

    fn checked_place_base_type(&self, place: &MirPlace) -> Option<MirType> {
        match &place.base {
            MirPlaceBase::Local(local) => self
                .locals
                .iter()
                .find(|candidate| candidate.id == *local)
                .map(|local| local.ty.clone()),
            MirPlaceBase::Parameter(value)
            | MirPlaceBase::Capture(value)
            | MirPlaceBase::Temporary(value) => self.checked_value_type(*value),
            MirPlaceBase::Static(_) => None,
        }
    }

    fn checked_vector_place_layout(
        &self,
        place: &MirPlace,
        field: Option<MirFieldId>,
    ) -> (MirVectorLayout, Option<usize>) {
        let Some(field) = field else {
            return (MirVectorLayout::Flat, None);
        };
        let Some(base_type) = self.checked_place_base_type(place) else {
            return (MirVectorLayout::AosStrided, None);
        };
        let element = match base_type.kind() {
            MirTypeKind::List(inner) | MirTypeKind::FixedList { elem: inner, .. } => inner.as_ref(),
            MirTypeKind::Tagged { inner, .. } => match inner.kind() {
                MirTypeKind::List(inner) | MirTypeKind::FixedList { elem: inner, .. } => {
                    inner.as_ref()
                }
                _ => return (MirVectorLayout::AosStrided, None),
            },
            _ => return (MirVectorLayout::AosStrided, None),
        };
        let Some(identity) = element.identity.or_else(|| match element.kind() {
            MirTypeKind::Apply { name, .. } => Some(name.id),
            _ => None,
        }) else {
            return (MirVectorLayout::AosStrided, None);
        };
        let Some(definition) = self
            .type_defs
            .iter()
            .find(|definition| definition.id == identity)
        else {
            return (MirVectorLayout::AosStrided, None);
        };
        let MirTypeDefKind::Struct { fields, .. } = &definition.kind else {
            return (MirVectorLayout::AosStrided, None);
        };
        if definition.layout != Some(jet_foundation::MIR::MirStructLayout::Columnar) {
            return (MirVectorLayout::AosStrided, None);
        }
        (
            MirVectorLayout::ColumnarDirect,
            fields.iter().position(|candidate| candidate.id == field),
        )
    }

    fn checked_indexed_field_access(
        &self,
        base: MirValueId,
        field: MirFieldId,
        cursor: Option<MirValueId>,
        cursor_place: Option<MirPlaceId>,
    ) -> Option<(MirVectorAccessRoot, MirVectorLayout, Option<usize>)> {
        let instruction = self
            .blocks
            .iter()
            .flat_map(|block| block.instructions.iter())
            .find(|instruction| instruction.result == Some(base))?;
        let MirOperation::Index {
            base: collection,
            index,
            kind: MirIndexKind::List | MirIndexKind::FixedListProof,
            ..
        } = &instruction.operation
        else {
            return None;
        };
        let cursor = cursor?;
        if !self.checked_cursor_matches(*index, cursor, cursor_place, &mut HashSet::new()) {
            return None;
        }
        let collection_type = self.checked_value_type(*collection)?;
        let element = match collection_type.kind() {
            MirTypeKind::List(inner) | MirTypeKind::FixedList { elem: inner, .. } => inner.as_ref(),
            MirTypeKind::Tagged { inner, .. } => match inner.kind() {
                MirTypeKind::List(inner) | MirTypeKind::FixedList { elem: inner, .. } => {
                    inner.as_ref()
                }
                _ => return None,
            },
            _ => return None,
        };
        let identity = element.identity.or_else(|| match element.kind() {
            MirTypeKind::Apply { name, .. } => Some(name.id),
            _ => None,
        })?;
        let definition = self
            .type_defs
            .iter()
            .find(|definition| definition.id == identity)?;
        let MirTypeDefKind::Struct { fields, .. } = &definition.kind else {
            return None;
        };
        let layout = if definition.layout == Some(jet_foundation::MIR::MirStructLayout::Columnar) {
            MirVectorLayout::ColumnarDirect
        } else {
            MirVectorLayout::AosStrided
        };
        Some((
            self.checked_value_access_root(*collection, &mut HashSet::new()),
            layout,
            (layout == MirVectorLayout::ColumnarDirect)
                .then(|| fields.iter().position(|candidate| candidate.id == field))
                .flatten(),
        ))
    }

    fn checked_vector_accesses(
        &self,
        blocks: &[MirBlockId],
        cursor: Option<MirValueId>,
        cursor_place: Option<MirPlaceId>,
    ) -> Vec<MirVectorAccess> {
        let mut accesses = Vec::new();
        for block_id in blocks {
            let Some(block) = self.block_by_id(*block_id) else {
                continue;
            };
            for instruction in &block.instructions {
                for place_id in checked_operation_place_refs(&instruction.operation) {
                    let Some(place) = self.places.iter().find(|place| place.id == place_id) else {
                        continue;
                    };
                    let field =
                        place
                            .projections
                            .iter()
                            .rev()
                            .find_map(|projection| match projection {
                                MirProjection::Field { field, .. } => Some(*field),
                                _ => None,
                            });
                    let (layout, column_index) = self.checked_vector_place_layout(place, field);
                    accesses.push(MirVectorAccess {
                        root: MirVectorAccessRoot::Place(place_id),
                        field,
                        layout,
                        column_index,
                    });
                }
                match &instruction.operation {
                    MirOperation::Index { base, index, .. }
                        if cursor.is_some_and(|cursor| {
                            self.checked_cursor_matches(
                                *index,
                                cursor,
                                cursor_place,
                                &mut HashSet::new(),
                            )
                        }) =>
                    {
                        accesses.push(MirVectorAccess {
                            root: self.checked_value_access_root(*base, &mut HashSet::new()),
                            field: None,
                            layout: MirVectorLayout::Flat,
                            column_index: None,
                        });
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
                            self.checked_indexed_field_access(*base, *field, cursor, cursor_place)
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

    fn checked_lane_width(ty: &MirType, rule: MirVectorRule) -> Option<u16> {
        match ty.kind() {
            MirTypeKind::Int => (rule == MirVectorRule::EarlyExitSearch).then_some(2),
            MirTypeKind::Float => Some(2),
            MirTypeKind::Float32 => Some(4),
            MirTypeKind::IntN { bits, .. } => Some(if *bits <= 32 { 4 } else { 2 }),
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. }
            | MirTypeKind::Quantity { base, .. } => Self::checked_lane_width(base, rule),
            _ => None,
        }
    }

    pub(super) fn record_checked_vector_fact(
        &mut self,
        header: MirBlockId,
        cursor: Option<MirValueId>,
        body_blocks: Vec<MirBlockId>,
        advance_block: Option<MirBlockId>,
        cursor_place: Option<MirPlaceId>,
        facts: &crate::AST::AutoVectorizationFacts,
        span: Span,
    ) -> Result<(), LowerError> {
        let element_type = self.mir_type(&facts.element_type)?;
        let accesses = self.checked_vector_accesses(&body_blocks, cursor, cursor_place);
        let layouts = accesses
            .iter()
            .filter(|access| access.field.is_some() || access.layout != MirVectorLayout::Flat)
            .map(|access| access.layout)
            .collect::<BTreeSet<_>>();
        let layout = if layouts.len() == 1 {
            *layouts.iter().next().expect("one checked vector layout")
        } else {
            MirVectorLayout::Flat
        };
        let lane_width = Self::checked_lane_width(&element_type, MirVectorRule::Elementwise);
        let packed = lane_width.is_some()
            && !accesses.is_empty()
            && facts.no_aliasing
            && facts.no_early_exit
            && facts.effect_free_body
            && facts.no_cross_iteration_deps;
        self.checked_vector_facts.push(MirVectorFact {
            loop_header: header,
            cursor,
            body_blocks,
            advance_block,
            rule: MirVectorRule::Elementwise,
            accesses,
            layout,
            element_type: Some(element_type),
            packed,
            lane_width,
            no_aliasing: facts.no_aliasing,
            no_early_exit: facts.no_early_exit,
            effect_free_body: facts.effect_free_body,
            no_cross_iteration_dependencies: facts.no_cross_iteration_deps,
            fixed_reduction: None,
            span,
            // This row is source evidence, not a final optimization decision.
            decision: MirOptimizationDecision::Rejected(
                MirOptimizationRejection::UnsupportedOperation,
            ),
        });
        Ok(())
    }

    pub(super) fn set_span(&mut self, span: Span) {
        self.current_span = span;
    }
    pub(super) fn set_line_marker(&mut self, line: u32) {
        self.current_line = Some(line);
    }
    /// Source text and line index of the function being lowered.
    pub(super) fn function_source(&self) -> Option<&'a IndexedSource<'a>> {
        self.source_texts.get(&self.function.source_file)
    }
    pub(super) fn source_line(&self) -> u32 {
        if let Some(line) = self.current_line {
            return line;
        }
        // LineMarker is debug-only; SourceSpan is always present for jet run.
        if let Some(source) = self.function_source() {
            return source.lines.line_col(source.text, self.current_span.start).0 as u32;
        }
        self.function.line as u32
    }
    /// 1-based character column of the statement being lowered, or 0 when
    /// its source text is not retained.
    pub(super) fn source_column(&self) -> u32 {
        self.function_source().map_or(0, |source| {
            source.lines.line_col(source.text, self.current_span.start).1 as u32
        })
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
                self.function_source().map(|source| {
                    source
                        .lines
                        .line_text(source.text, line as usize)
                        .to_owned()
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
    fn lower_db_query_metadata(&mut self, metadata: super::TDbQueryMetadata) -> MirDbQueryMetadata {
        let source_path = metadata.source_file;
        MirDbQueryMetadata {
            source_file: self.source_file_id_for(&source_path),
            source_path,
            source_span: metadata.source_span,
            statement_identity: metadata.statement_identity,
            table_facts: metadata
                .table_facts
                .into_iter()
                .map(|fact| MirDbTableFact {
                    table_id: fact.table_id,
                    read: fact.read,
                    write: fact.write,
                })
                .collect(),
        }
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
        // A bare name declared by the function's own module shadows a
        // compiler-owned row of the same leaf (a user `Effect` over the Core
        // export row keyed `Effect`), as `mir_type` resolves the value type.
        let scoped = (!self.function.module.is_empty() && !key.contains("::"))
            .then(|| format!("{}::{key}", self.function.module))
            .filter(|scoped| {
                self.type_defs
                    .iter()
                    .any(|ty| ty.key == *scoped && ty.name == key)
            });
        let canonical_key = canonical_nominal_name(
            self.type_defs,
            scoped.as_deref().unwrap_or(key),
            self.span(),
        )?;
        let exact = self
            .type_defs
            .iter()
            .filter(|ty| ty.key == canonical_key)
            .collect::<Vec<_>>();
        match exact.as_slice() {
            [ty] => Ok(ty.id),
            [] => match self
                .trait_defs
                .iter()
                .filter(|trait_def| trait_def.key == key || trait_def.name == key)
                .count()
            {
                0 => Ok(MirTypeId(stable_id("mir-type", &canonical_key))),
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
    /// Return the flattened MIR leaves rooted at a checked enum group path.
    ///
    /// Groups are a source/TIR pattern convenience.  Canonical MIR stores only
    /// their dotted leaf variants, so consumers must test each descendant leaf.
    pub(super) fn enum_group_leaves(&self, owner: MirTypeId, group: &str) -> Option<Vec<String>> {
        let row = self.type_defs.iter().find(|row| row.id == owner)?;
        let MirTypeDefKind::Enum { variants, .. } = &row.kind else {
            return None;
        };
        let prefix = format!("{group}.");
        let leaves = variants
            .iter()
            .filter(|variant| variant.name.starts_with(&prefix))
            .map(|variant| variant.name.clone())
            .collect::<Vec<_>>();
        (!leaves.is_empty()).then_some(leaves)
    }

    fn normalize_contextual_type(&self, ty: &Type) -> Type {
        ty.map_named_types(&|name| {
            if self
                .function
                .generic_params
                .iter()
                .any(|parameter| parameter.name == name)
            {
                return None;
            }
            let scoped = format!("{}::{name}", self.function.module);
            // A bare built-in type name (`Path`) outside its declaring module is
            // the built-in; a loaded module's same-named record is reached only
            // through its scoped or canonical spelling.
            let bare = if name == crate::Syntax::TYPE_TASKGROUP
                || crate::Syntax::typed_head_kind(name).is_some()
            {
                None
            } else {
                self.nominal_identities.get(name)
            };
            self.nominal_identities
                .get(&scoped)
                .or(bare)
                .cloned()
                .or_else(|| {
                    let qualified = (!self.function.module.is_empty()).then_some(scoped)?;
                    self.type_defs
                        .iter()
                        .any(|definition| definition.key == qualified && definition.name == name)
                        .then_some(qualified)
                })
        })
    }

    pub(super) fn mir_type(&mut self, ty: &Type) -> Result<MirType, LowerError> {
        let span = self.span();
        let normalized = self.normalize_contextual_type(ty);
        let contextual_owner = match &normalized {
            Type::Named(name) => {
                let owner_name = match &self.function.kind {
                    super::TFuncKind::Method { owner_type, .. }
                    | super::TFuncKind::TraitMethod { owner_type, .. } => Some(owner_type.name()),
                    super::TFuncKind::TopLevel => None,
                };
                owner_name.and_then(|owner_name| {
                    let same_leaf = owner_name
                        .rsplit("::")
                        .next()
                        .unwrap_or(owner_name.as_str())
                        == name;
                    let ambiguous = self
                        .type_defs
                        .iter()
                        .filter(|definition| {
                            definition.name == *name
                                || definition
                                    .key
                                    .rsplit("::")
                                    .next()
                                    .unwrap_or(&definition.key)
                                    == name.as_str()
                        })
                        .count()
                        > 1;
                    (same_leaf && ambiguous).then_some(Type::Named(owner_name))
                })
            }
            _ => None,
        };
        let resolved_ty = contextual_owner.as_ref().unwrap_or(&normalized);
        let instance = canonical_type_instance(self.type_defs, resolved_ty, span)?;
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
        let lookup_key = math_field_name(ty, key).unwrap_or(key);
        let field = match &ty.kind {
            MirTypeDefKind::Struct { fields, .. } => {
                fields.iter().find(|field| field.name == lookup_key)
            }
            MirTypeDefKind::Enum { variants, .. } => {
                variants.iter().find_map(|variant| match &variant.payload {
                    jet_foundation::MIR::MirVariantPayload::Named(fields) => {
                        fields.iter().find(|field| field.name == lookup_key)
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

    /// Whether `ty` names a checked MIR record row. The compiler-owned
    /// Duration quantity has none; its `ns` read is a scalar projection.
    pub(super) fn has_field_owner(&self, ty: &Type) -> bool {
        self.field_owner_id_for_type(ty)
            .is_ok_and(|owner| self.type_defs.iter().any(|definition| definition.id == owner))
    }

    pub(super) fn field_owner_id_for_type(&self, ty: &Type) -> Result<MirTypeId, LowerError> {
        let normalized = self.normalize_contextual_type(ty);
        let ty = field_owner_type(&normalized);
        let key = match ty {
            Type::Apply { name, .. } => name.clone(),
            // A structural union owns the generated carrier row whose identity
            // is keyed by its canonical members, as in `mir_type`.
            Type::Union(_) => {
                type_identity_key(&canonicalize_type(self.type_defs, ty, self.span())?)
            }
            _ => type_identity_key(ty),
        };
        let id = self.type_id_for(&key)?;
        Ok(id)
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
                ));
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
        let normalized = self.normalize_contextual_type(ty);
        let ty = field_owner_type(&normalized);
        if let Type::Tuple(fields) = ty {
            if let Some(field_name) = tuple_field_name(fields, key) {
                let identity =
                    canonical_type_instance(self.type_defs, ty, self.span())?.identity_key();
                return Ok(MirFieldId(stable_id(
                    "mir-field",
                    &format!("{identity}::{field_name}"),
                )));
            }
        }
        let owner = self.field_owner_id_for_type(ty)?;
        self.field_id_for(owner, key)
    }

    fn field_id_for_mir_type(&self, ty: &MirType, key: &str) -> Result<MirFieldId, LowerError> {
        let ty = field_owner_mir_type(ty);
        if let Some(fields) = ty.tuple_fields() {
            if let Some(field_name) = tuple_field_name(fields, key) {
                return Ok(MirFieldId(stable_id(
                    "mir-field",
                    &format!("{}::{field_name}", ty.identity_key()),
                )));
            }
        }
        // Applied values carry an instance identity (`Pair<Int>`, `Sender<String>`, ...),
        // but field rows belong to the nominal declaration. Resolve the owner through
        // the checked nominal reference instead of treating the applied instance as a
        // declaration row.
        let owner = match ty.kind() {
            MirTypeKind::Apply { name, .. } => self.type_id_for(&name.name)?,
            _ => ty.nominal_id().ok_or_else(|| {
                self.error(self.span(), format!("missing MIR field owner for `{key}`"))
            })?,
        };
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
        if let Some(existing) =
            self.prelude_call_positions.get(&row.id).map(|position| &self.prelude_calls[*position])
        {
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
        self.prelude_call_positions.insert(row.id, self.prelude_calls.len());
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
        let db_metadata = route
            .db_metadata
            .map(|metadata| self.lower_db_query_metadata(metadata));
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
            db_metadata,
        })
    }

    pub(super) fn function_id(&self) -> Result<MirFunctionId, LowerError> {
        self.function_registry.id_for(self.function)
    }

    pub(super) fn current_block(&self) -> MirBlockId {
        self.current
    }

    pub(super) fn switch_to(&mut self, block: MirBlockId) {
        self.current = block;
    }

    pub(super) fn block_mut(&mut self, id: MirBlockId) -> Result<&mut MirBasicBlock, LowerError> {
        match self.block_positions.get(&id).copied() {
            Some(index) => Ok(&mut self.blocks[index]),
            None => Err(self.error(self.span(), format!("missing MIR block {id:?}"))),
        }
    }

    /// The block with `id` (first row wins, as a linear `find` would).
    pub(super) fn block_by_id(&self, id: MirBlockId) -> Option<&MirBasicBlock> {
        self.block_positions
            .get(&id)
            .map(|position| &self.blocks[*position])
    }

    fn current_block_mut(&mut self) -> Option<&mut MirBasicBlock> {
        let position = *self.block_positions.get(&self.current)?;
        Some(&mut self.blocks[position])
    }

    /// The instruction that defines `value`.
    fn value_instruction(&self, value: MirValueId) -> Option<&MirInstruction> {
        self.blocks[*self.value_blocks.get(&value)?]
            .instructions
            .iter()
            .find(|instruction| instruction.result == Some(value))
    }

    pub(super) fn new_block(&mut self, span: Span, role: &str) -> Result<MirBlockId, LowerError> {
        let identity = self.reserve_identity("block", span, role, "")?;
        let id = MirBlockId(stable_id("mir-block", &identity));
        self.block_positions.entry(id).or_insert(self.blocks.len());
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
            self.block_by_id(self.current).map(|block| &block.terminator),
            Some(MirTerminator::Unreachable { reason }) if reason == "lowering in progress"
        )
    }

    pub(super) fn terminate(&mut self, terminator: MirTerminator) {
        if let Some(block) = self.current_block_mut() {
            block.terminator = terminator;
        }
    }

    /// End the current path with `terminator`, running the cleanups of every
    /// frame from `from_depth` up first. Each frame's cleanups are lowered
    /// once, as a drop chain its exits share (`seal_exit_chains`), so cleanup
    /// code grows with the frame's locals, not with exits times locals. Exits
    /// that cannot share a chain clean up inline on their own path.
    pub(super) fn terminate_with_cleanup(
        &mut self,
        terminator: MirTerminator,
        from_depth: usize,
    ) -> Result<(), LowerError> {
        self.emit_transaction_exit_restores(&terminator, from_depth)?;
        if let MirTerminator::Return { value: Some(value) } = &terminator {
            self.retype_absent_return(*value)?;
        }
        let from_depth = from_depth.min(self.defer_stack.len());
        if let Some(target) = self.shared_exit_target(&terminator, from_depth)? {
            if let (ExitTarget::Return(Some(slot)), MirTerminator::Return { value: Some(value) }) =
                (target, &terminator)
            {
                self.emit(
                    "exit.value.write",
                    None,
                    MirOperation::WritePlace { place: slot, value: *value },
                )?;
            }
            let depth = self.defer_stack.len() - 1;
            let entry = self.exit_chain_entry(depth, target, from_depth)?;
            self.terminate(MirTerminator::Jump { target: entry });
            return Ok(());
        }
        // Preserve deferred actions so a sibling path gets its own
        // reverse-order cleanup sequence.
        self.emit_deferred_cleanups_from(from_depth, false)?;
        self.terminate(terminator);
        Ok(())
    }

    /// Roll back every `#Transact` scope an exit leaves before that exit's
    /// cleanups release the saved snapshots: a `return`, `?` propagation,
    /// `break` or `continue` out of the body restores each snapshot, innermost
    /// transaction and newest snapshot first. The body's fallthrough commits.
    fn emit_transaction_exit_restores(
        &mut self,
        terminator: &MirTerminator,
        from_depth: usize,
    ) -> Result<(), LowerError> {
        if !matches!(
            terminator,
            MirTerminator::Return { .. } | MirTerminator::Break { .. } | MirTerminator::Continue { .. }
        ) {
            return Ok(());
        }
        // Taken while the restores lower, so a restore call never re-enters.
        let transactions = std::mem::take(&mut self.transaction_restores);
        let left = transactions
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, (scope, _))| {
                self.defer_stack
                    .iter()
                    .rposition(|frame| frame.owner == Some(*scope))
                    .is_some_and(|depth| depth >= from_depth)
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let mut result = Ok(());
        'restore: for transaction in left {
            for (index, restore) in transactions[transaction].1.iter().enumerate().rev() {
                if let Err(error) = self.emit_transaction_restore(index, restore) {
                    result = Err(error);
                    break 'restore;
                }
            }
        }
        self.transaction_restores = transactions;
        result
    }

    fn emit_transaction_restore(
        &mut self,
        index: usize,
        restore: &TransactionRestore,
    ) -> Result<(), LowerError> {
        if let Some(call) = &restore.custom {
            lower_expr(self, call)?;
            return Ok(());
        }
        let value = self.emit(
            &format!("transaction.snapshot.{index}.restore.read"),
            Some(restore.ty.clone()),
            MirOperation::ReadPlace(restore.saved),
        )?;
        self.emit(
            &format!("transaction.snapshot.{index}.restore.write"),
            None,
            MirOperation::WritePlace {
                place: restore.place,
                value,
            },
        )?;
        Ok(())
    }

    /// The drop chain an exit toward `terminator` enters, or `None` when it
    /// cleans up inline: nothing is registered to clean up, the terminator
    /// carries a value no slot can hold (a break value feeds its loop's phi
    /// by predecessor), or a frame it leaves keeps its exits inline.
    fn shared_exit_target(
        &mut self,
        terminator: &MirTerminator,
        from_depth: usize,
    ) -> Result<Option<ExitTarget>, LowerError> {
        let frames = &self.defer_stack[from_depth..];
        if frames.iter().all(|frame| frame.actions.is_empty())
            || frames.iter().any(|frame| frame.inline_exits)
        {
            return Ok(None);
        }
        Ok(match terminator {
            MirTerminator::Return { value: None } => Some(ExitTarget::Return(None)),
            MirTerminator::Return { value: Some(value) } => {
                self.exit_value_slot(*value)?.map(|slot| ExitTarget::Return(Some(slot)))
            }
            MirTerminator::Break { target, value: None } => Some(ExitTarget::Break(*target)),
            MirTerminator::Continue { target } => Some(ExitTarget::Continue(*target)),
            _ => None,
        })
    }

    /// The local a returned `value` waits in while its drop chain runs, one
    /// per returned type; `None` for a borrow, which stays on its own path.
    fn exit_value_slot(&mut self, value: MirValueId) -> Result<Option<MirPlaceId>, LowerError> {
        // The returned value was produced just before the exit, so the scan
        // from the newest row is short.
        let Some((ty, ownership)) = self
            .values
            .iter()
            .rev()
            .find(|row| row.0 == value)
            .map(|row| (row.1.clone(), row.3))
        else {
            return Ok(None);
        };
        if matches!(
            ownership.mode,
            MirOwnershipMode::ReadBorrow | MirOwnershipMode::WriteBorrow
        ) {
            return Ok(None);
        }
        if let Some((_, slot)) = self.exit_value_slots.iter().find(|(slot_ty, _)| *slot_ty == ty) {
            return Ok(Some(*slot));
        }
        let name = format!("exit_value_{}", self.exit_value_slots.len());
        let span = self.span();
        let local_identity = self.reserve_identity("local", span, &name, "")?;
        let local_id = MirLocalId(stable_id("mir-local", &local_identity));
        let place = self.place_id("local", &name)?;
        self.places.push(MirPlace {
            id: place,
            span,
            ty: ty.clone(),
            base: MirPlaceBase::Local(local_id),
            projections: Vec::new(),
            access: MirAccess::Move,
            persist_key: None,
        });
        self.locals.push(MirLocal {
            id: local_id,
            name,
            span,
            ty: ty.clone(),
            place,
            mutable: true,
            ownership: MirOwnership {
                moved: false,
                last_use: false,
                ..ownership
            },
            comptime: false,
            uninit: false,
            arena_view: false,
            string_view: false,
            gc_root: false,
        });
        self.exit_value_slots.push((ty, place));
        Ok(Some(place))
    }

    /// The block an exit from frame `depth` toward `target` jumps to: one
    /// per target and per count of the frame's explicit actions so far.
    fn exit_chain_entry(
        &mut self,
        depth: usize,
        target: ExitTarget,
        cut: usize,
    ) -> Result<MirBlockId, LowerError> {
        let frame = &self.defer_stack[depth];
        let explicit = frame.explicit;
        if let Some(exit) = frame
            .pending_exits
            .iter()
            .find(|exit| exit.target == target && exit.explicit == explicit)
        {
            return Ok(exit.block);
        }
        let block = self.new_block(self.span(), "cleanup.chain")?;
        self.defer_stack[depth].pending_exits.push(PendingExit {
            target,
            explicit,
            cut,
            block,
        });
        Ok(block)
    }

    /// The block that ends every drop chain toward `target` with the exit.
    fn exit_tail(&mut self, target: ExitTarget) -> Result<MirBlockId, LowerError> {
        if let Some(block) = self.exit_tails.get(&target) {
            return Ok(*block);
        }
        let block = self.new_block(self.span(), "cleanup.chain.exit")?;
        self.switch_to(block);
        let terminator = match target {
            ExitTarget::Return(None) => MirTerminator::Return { value: None },
            ExitTarget::Return(Some(slot)) => {
                let ty = self
                    .exit_value_slots
                    .iter()
                    .find(|(_, place)| *place == slot)
                    .map(|(ty, _)| ty.clone())
                    .ok_or_else(|| self.error(self.span(), "exit value slot has no type"))?;
                let value = self.emit_mir_type(
                    "exit.value.read",
                    Some(ty),
                    MirOperation::MovePlace { place: slot },
                )?;
                MirTerminator::Return { value: Some(value) }
            }
            ExitTarget::Break(target) => MirTerminator::Break {
                target,
                value: None,
            },
            ExitTarget::Continue(target) => MirTerminator::Continue { target },
        };
        self.terminate(terminator);
        self.exit_tails.insert(target, block);
        Ok(block)
    }

    /// Lower the drop chains that exits from frame `depth` jump to, using
    /// the frame's actions as they stand now. Per target, the chain entered
    /// after `e` explicit actions runs those actions newest first, then
    /// releases every owned local the frame registered, newest first, then
    /// continues into the enclosing frame's chain or, at the target's frame,
    /// ends with the exit. Each release is guarded by the local's live flag,
    /// which reads false on an exit taken before the local was bound, so one
    /// release sequence serves every exit of the frame; explicit actions run
    /// unconditionally, hence the per-count entries. Runs when the frame
    /// closes, before its actions are consumed, and at the end of the body.
    fn seal_exit_chains(&mut self, depth: usize) -> Result<(), LowerError> {
        let saved = self.current;
        loop {
            let mut pending = std::mem::take(&mut self.defer_stack[depth].pending_exits);
            if pending.is_empty() {
                break;
            }
            pending.sort_by_key(|exit| exit.explicit);
            let actions = self.defer_stack[depth].actions.clone();
            let (automatic, explicit): (Vec<_>, Vec<_>) = actions
                .into_iter()
                .partition(|action| matches!(action, DeferredCleanup::DropPlace { .. }));
            let mut sealed: Vec<ExitTarget> = Vec::new();
            for exit in &pending {
                if sealed.contains(&exit.target) {
                    continue;
                }
                sealed.push(exit.target);
                let entries: Vec<&PendingExit> =
                    pending.iter().filter(|other| other.target == exit.target).collect();
                let next = if depth <= exit.cut {
                    self.exit_tail(exit.target)?
                } else {
                    self.exit_chain_entry(depth - 1, exit.target, exit.cut)?
                };
                let entry = |count: usize, ctx: &mut Self| match entries
                    .iter()
                    .find(|candidate| candidate.explicit == count)
                {
                    Some(candidate) => Ok(candidate.block),
                    None => ctx.new_block(ctx.span(), "cleanup.chain"),
                };
                let mut block = entry(0, self)?;
                self.switch_to(block);
                for action in automatic.iter().rev() {
                    self.emit_cleanup_action(*action, false)?;
                }
                self.terminate(MirTerminator::Jump { target: next });
                let deepest = entries.last().map_or(0, |last| last.explicit);
                for count in 1..=deepest {
                    let following = block;
                    block = entry(count, self)?;
                    self.switch_to(block);
                    let action = explicit.get(count - 1).copied().ok_or_else(|| {
                        self.error(self.span(), "drop chain entry outlives its explicit cleanup")
                    })?;
                    self.emit_cleanup_action(action, false)?;
                    self.terminate(MirTerminator::Jump { target: following });
                }
            }
        }
        self.switch_to(saved);
        Ok(())
    }

    /// Seal every active frame's drop chains, innermost first, so each
    /// enclosing frame sees the entries its inner chains continue into.
    fn seal_all_exit_chains(&mut self) -> Result<(), LowerError> {
        for depth in (0..self.defer_stack.len()).rev() {
            self.seal_exit_chains(depth)?;
        }
        Ok(())
    }

    /// Consume every active deferred action before an explicit process stop.
    ///
    /// Native stop unwinds after the call, while the resident JIT branches to
    /// its stop block as soon as the host records the exit.  Cleanup therefore
    /// has to be in the MIR before the stop call, not on a successor edge.
    /// Exits taken before the stop keep the actions in their sealed chains.
    pub(super) fn emit_explicit_stop_cleanups(&mut self) -> Result<(), LowerError> {
        self.seal_all_exit_chains()?;
        self.emit_deferred_cleanups_from(0, true)
    }

    /// D-SHAPE-RESOURCE1=A: run the cleanups that must happen before the one
    /// CFG path that stops the program (`panic`, a failed `assert`/`require`):
    /// deferred actions and the automatic `Close` of still-live resources.
    /// Plain releases are skipped because the process is ending anyway, and
    /// the actions stay registered for sibling paths that continue.
    pub(super) fn emit_stop_path_cleanups(&mut self) -> Result<(), LowerError> {
        self.emit_cleanup_actions(0, false, true)
    }

    /// Whether a stop at this point owes any cleanup: a deferred action, or
    /// the automatic `Close` of a still-registered resource local.
    pub(super) fn has_stop_path_cleanups(&self) -> bool {
        self.defer_stack
            .iter()
            .flat_map(|frame| frame.actions.iter())
            .any(|action| match *action {
                DeferredCleanup::DropPlace { place, .. } => self.place_has_scope_end_close(place),
                _ => true,
            })
    }

    /// Locals that `emit_stop_path_cleanups` moves out. A stop report must not
    /// read them afterwards.
    pub(super) fn stop_path_moved_locals(&self) -> HashSet<MirLocalId> {
        self.defer_stack
            .iter()
            .flat_map(|frame| frame.actions.iter())
            .filter_map(|action| match *action {
                DeferredCleanup::Close(index) => Some(self.deferred_closes[index].place),
                DeferredCleanup::Guard(place) | DeferredCleanup::FileOwner { place, .. } => {
                    Some(place)
                }
                DeferredCleanup::DropPlace { place, .. } => {
                    self.place_has_scope_end_close(place).then_some(place)
                }
            })
            .filter_map(|place| {
                match self.places.iter().find(|row| row.id == place)?.base {
                    MirPlaceBase::Local(local) => Some(local),
                    _ => None,
                }
            })
            .collect()
    }

    fn place_has_scope_end_close(&self, place: MirPlaceId) -> bool {
        self.places
            .iter()
            .find(|row| row.id == place)
            .is_some_and(|row| self.scope_end_close(&row.ty).is_some())
    }

    fn emit_deferred_cleanups(&mut self, from_depth: usize) -> Result<(), LowerError> {
        self.emit_deferred_cleanups_from(from_depth, true)
    }

    fn emit_deferred_cleanups_from(
        &mut self,
        from_depth: usize,
        consume: bool,
    ) -> Result<(), LowerError> {
        self.emit_cleanup_actions(from_depth, consume, false)
    }

    fn emit_cleanup_actions(
        &mut self,
        from_depth: usize,
        consume: bool,
        stop_path: bool,
    ) -> Result<(), LowerError> {
        let from_depth = from_depth.min(self.defer_stack.len());
        for depth in (from_depth..self.defer_stack.len()).rev() {
            let actions = if consume {
                self.defer_stack[depth].explicit = 0;
                std::mem::take(&mut self.defer_stack[depth].actions)
            } else {
                self.defer_stack[depth].actions.clone()
            };
            // D-SHAPE-RESOURCE1=A: explicit deferred actions run first in
            // reverse registration order; the automatic safety net then
            // closes or releases every still-live local in reverse
            // declaration order.
            let (automatic, explicit): (Vec<_>, Vec<_>) = actions
                .into_iter()
                .partition(|action| matches!(action, DeferredCleanup::DropPlace { .. }));
            for action in explicit.into_iter().rev().chain(automatic.into_iter().rev()) {
                self.emit_cleanup_action(action, stop_path)?;
            }
        }
        Ok(())
    }

    /// Lower one cleanup action into the current block. On a stop path a
    /// plain release is skipped: the process is ending anyway.
    fn emit_cleanup_action(
        &mut self,
        action: DeferredCleanup,
        stop_path: bool,
    ) -> Result<(), LowerError> {
        match action {
            DeferredCleanup::Close(index) => self.emit_deferred_close(index)?,
            DeferredCleanup::Guard(place) => {
                let ty = self
                    .places
                    .iter()
                    .find(|candidate| candidate.id == place)
                    .map(|candidate| candidate.ty.clone())
                    .ok_or_else(|| {
                        self.error(
                            self.span(),
                            "scope guard cleanup targets an unavailable place",
                        )
                    })?;
                let value = self.emit_mir_type(
                    "scope.guard.cleanup.move",
                    Some(ty),
                    MirOperation::MovePlace { place },
                )?;
                self.emit(
                    "scope.guard.cleanup.drop",
                    None,
                    MirOperation::Drop {
                        value,
                        kind: MirDropKind::Value,
                    },
                )?;
            }
            DeferredCleanup::FileOwner { place, live } => {
                self.emit_file_owner_cleanup(place, live)?;
            }
            DeferredCleanup::DropPlace { place, live, kind } => {
                if !stop_path || self.place_has_scope_end_close(place) {
                    self.emit_owned_place_cleanup(place, live, kind)?;
                }
            }
        }
        Ok(())
    }

    pub(super) fn emit_scope_cleanups(&mut self, scope: MirScopeId) -> Result<(), LowerError> {
        let Some(depth) = self
            .defer_stack
            .iter()
            .rposition(|frame| frame.owner == Some(scope))
        else {
            return Err(self.error(
                self.span(),
                format!("scope cleanup has no active frame for {scope:?}"),
            ));
        };
        self.emit_deferred_cleanups_from(depth, false)
    }

    /// The name a user local is bound under. A binding whose source spelling
    /// collides with a reserved identifier may be bound under its mangled
    /// Rust spelling; every lookup resolves through this one rule.
    pub(super) fn resolved_local_name(&self, local: &TLocal) -> String {
        let mangled = crate::Codegen::mangle(&local.name);
        if !local.generated && mangled != local.name && self.local_places.contains_key(&mangled) {
            mangled
        } else {
            local.name.clone()
        }
    }

    /// Register `close` to run at every exit of the innermost lexical frame.
    /// The resource place is resolved now, so a later binding that shadows the
    /// resource name cannot redirect the close.
    pub(super) fn register_defer_close(
        &mut self,
        close: TExpr,
        resource: &TLocal,
    ) -> Result<(), LowerError> {
        if self.defer_stack.is_empty() {
            return Err(self.error(self.span(), "defer registered without lexical scope"));
        }
        let resource = self.resolved_local_name(resource);
        let ty = self.local_types.get(&resource).cloned().ok_or_else(|| {
            self.error(
                self.span(),
                format!("unbound deferred resource `{resource}`"),
            )
        })?;
        let place = self.place_for_local(&TLocal::user(resource.clone()), MirAccess::Move)?;
        let index = self.deferred_closes.len();
        self.deferred_closes.push(std::rc::Rc::new(DeferredClose {
            close,
            resource,
            place,
            ty,
            span: self.span(),
        }));
        self.defer_stack
            .last_mut()
            .expect("checked non-empty defer stack")
            .push_action(DeferredCleanup::Close(index));
        Ok(())
    }

    /// Lower one deferred close at the current exit edge. The resource name is
    /// rebound to the place captured at registration while the close lowers,
    /// and the close gets its own lexical frame so any temporaries it owns are
    /// cleaned up here rather than joining the frame being exited.
    fn emit_deferred_close(&mut self, index: usize) -> Result<(), LowerError> {
        let deferred = std::rc::Rc::clone(&self.deferred_closes[index]);
        let exit_span = self.span();
        let previous_place = self
            .local_places
            .insert(deferred.resource.clone(), deferred.place);
        let previous_ty = self
            .local_types
            .insert(deferred.resource.clone(), deferred.ty.clone());
        self.set_span(deferred.span);
        self.push_lexical_frame();
        let lowered = self
            .lower_child(&deferred.close)
            .and_then(|_| self.pop_lexical_frame());
        self.set_span(exit_span);
        match previous_place {
            Some(place) => self.local_places.insert(deferred.resource.clone(), place),
            None => self.local_places.remove(&deferred.resource),
        };
        match previous_ty {
            Some(ty) => self.local_types.insert(deferred.resource.clone(), ty),
            None => self.local_types.remove(&deferred.resource),
        };
        lowered
    }

    pub(super) fn register_scope_guard(&mut self, place: MirPlaceId) -> Result<(), LowerError> {
        let Some(row) = self
            .places
            .iter_mut()
            .find(|candidate| candidate.id == place)
        else {
            return Err(self.error(
                self.span(),
                "scope guard cleanup targets an unavailable place",
            ));
        };
        if !matches!(
            row.ty.nominal_name(),
            Some("ScopeGuard") | Some("EventScope")
        ) {
            return Err(self.error(self.span(), "scope guard cleanup targets a non-guard place"));
        }
        let event_scope = row.ty.nominal_name() == Some("EventScope");
        retain_place_access(row, MirAccess::Move);
        let Some(frame) = self.defer_stack.last_mut() else {
            if event_scope {
                return Ok(());
            }
            return Err(self.error(self.span(), "scope guard registered without lexical scope"));
        };
        frame.push_action(DeferredCleanup::Guard(place));
        Ok(())
    }
    pub(super) fn is_core_file_owner(&self, identity: MirTypeId) -> bool {
        let Some(source) = jet_foundation::CoreModuleExports::core_source_module("core.files") else {
            return false;
        };
        self.type_defs.iter().any(|definition| {
            definition.id == identity
                && matches!(definition.name.as_str(), "FileReader" | "FileWriter" | "TempDir" | "TempFile" | "FileLock")
                && self.modules.iter().any(|module| {
                    module.path == source.path
                        && definition.module == jet_foundation::MIR::MirModuleId(stable_id("mir-module", &module.key))
                        && definition.key == format!("{}::{}", module.key, definition.name)
                })
        })
    }

    fn owned_file_paths(
        &mut self,
        ty: &MirType,
        path: &mut Vec<(String, Type)>,
        visiting: &mut Vec<MirTypeId>,
        output: &mut Vec<Vec<(String, Type)>>,
    ) -> Result<(), LowerError> {
        if ty.identity.is_some_and(|identity| self.is_core_file_owner(identity)) {
            output.push(path.clone());
            return Ok(());
        }
        if let Some(fields) = ty.tuple_fields() {
            for (name, field_ty) in fields {
                path.push((name.clone(), mir_type_as_ast(field_ty)));
                self.owned_file_paths(field_ty, path, visiting, output)?;
                path.pop();
            }
            return Ok(());
        }
        let Some(identity) = ty.identity else { return Ok(()) };
        if visiting.contains(&identity) {
            return Ok(());
        }
        let definitions = self.type_defs;
        let Some(definition) = definitions.iter().find(|row| row.id == identity) else {
            return Ok(());
        };
        if !matches!(definition.ownership, MirOwnershipMode::Owned | MirOwnershipMode::Move) {
            return Ok(());
        }
        let MirTypeDefKind::Struct { fields, .. } = &definition.kind else {
            return Ok(());
        };
        visiting.push(identity);
        let owner = mir_type_as_ast(ty);
        for field in fields.iter().filter(|field| !field.computed) {
            let field_ty = self.field_type_for_type(&owner, &field.name)?;
            let checked = self.mir_type(&field_ty)?;
            path.push((field.name.clone(), field_ty));
            self.owned_file_paths(&checked, path, visiting, output)?;
            path.pop();
        }
        visiting.pop();
        Ok(())
    }

    fn binding_borrow_access(&self, value: MirValueId, seen: &mut Vec<MirValueId>) -> Option<MirAccess> {
        if seen.contains(&value) {
            return None;
        }
        seen.push(value);
        let operation = &self.value_instruction(value)?.operation;
        match operation {
            MirOperation::ReadPlace(_) => Some(MirAccess::Read),
            MirOperation::AddressOf { access: MirAccess::Read, .. } => Some(MirAccess::Read),
            MirOperation::AddressOf { access: MirAccess::Write, .. } => Some(MirAccess::Write),
            MirOperation::Move { value } | MirOperation::AttachTag { value, .. } =>
                self.binding_borrow_access(*value, seen),
            MirOperation::Field { base, .. } | MirOperation::ProjectMembers { base, .. }
            | MirOperation::Index { base, .. } => self.binding_borrow_access(*base, seen),
            MirOperation::Phi { incoming } => incoming.iter()
                .find_map(|(_, value)| self.binding_borrow_access(*value, seen)),
            _ => None,
        }
    }

    pub(super) fn register_file_owner_binding(
        &mut self,
        place: MirPlaceId,
        initializer: MirValueId,
    ) -> Result<(), LowerError> {
        if self.binding_borrow_access(initializer, &mut Vec::new()).is_some() {
            return Ok(());
        }
        self.register_file_owner(place)
    }

    /// Only owned record/tuple storage participates; views, shared storage,
    /// containers and recursive back-edges retain their existing lifecycle.
    pub(super) fn register_file_owner(&mut self, place: MirPlaceId) -> Result<(), LowerError> {
        let row = self.places.iter().find(|row| row.id == place)
            .ok_or_else(|| self.error(self.span(), "file owner cleanup has no place"))?;
        let owned = row.projections.is_empty() && match row.base {
            MirPlaceBase::Local(id) => self.locals.iter().any(|local| {
                local.id == id && local.place == place
                    && matches!(local.ownership.mode, MirOwnershipMode::Owned | MirOwnershipMode::Move)
            }),
            MirPlaceBase::Parameter(value) => self.value_instruction(value)
                .and_then(|instruction| match instruction.operation {
                    MirOperation::Parameter { index, .. } => self.params.get(index),
                    _ => None,
                })
                .is_some_and(|parameter| parameter.access == MirAccess::Move),
            _ => false,
        };
        if !owned {
            return Ok(());
        }
        let ty = row.ty.clone();
        let mut paths = Vec::new();
        self.owned_file_paths(&ty, &mut Vec::new(), &mut Vec::new(), &mut paths)?;
        // Deferred bindings unwind in reverse order; record fields unwind in
        // declaration order within each binding.
        for path in paths.into_iter().rev() {
            let mut leaf = place;
            for (name, ty) in path {
                leaf = self.project_field_place(leaf, &name, ty, self.span())?;
            }
            self.register_file_owner_leaf(leaf)?;
        }
        Ok(())
    }

    fn register_file_owner_leaf(&mut self, place: MirPlaceId) -> Result<(), LowerError> {
        let name = format!("file_owner_live_{}", place.0);
        let flag = self.bind_local(&TLocal::generated(name), Type::Bool, true, false, false)?;
        self.clear_live_flag_at_entry(flag)?;
        let initialized = self.emit(
            "file.owner.initialized",
            Some(Type::Bool),
            MirOperation::Constant(MirConstant::Bool(true)),
        )?;
        self.emit(
            "file.owner.live.write",
            None,
            MirOperation::WritePlace {
                place: flag,
                value: initialized,
            },
        )?;
        if let Some(row) = self.places.iter_mut().find(|row| row.id == place) {
            retain_place_access(row, MirAccess::Move);
        }
        self.file_owner_places.push(place);
        self.defer_stack
            .last_mut()
            .expect("function always has a lexical cleanup frame")
            .push_action(DeferredCleanup::FileOwner { place, live: flag });
        Ok(())
    }

    fn file_owner_leaves(&self, place: MirPlaceId) -> Vec<(MirPlaceId, MirPlaceId)> {
        let Some(root) = self.places.iter().find(|row| row.id == place) else {
            return Vec::new();
        };
        self.defer_stack.iter().rev().flat_map(|frame| frame.actions.iter().rev())
            .filter_map(|action| {
                let DeferredCleanup::FileOwner { place: owner, live } = action else {
                    return None;
                };
                let leaf = self.places.iter().find(|row| row.id == *owner)?;
                (root.base == leaf.base
                    && root.projections.len() <= leaf.projections.len()
                    && root.projections.iter().zip(&leaf.projections).all(|(a, b)| {
                        matches!((a, b),
                            (MirProjection::Field { field: a, .. }, MirProjection::Field { field: b, .. }) if a == b)
                    }))
                    .then_some((*owner, *live))
            })
            .collect()
    }

    fn emit_file_owner_cleanup(&mut self, place: MirPlaceId, live: MirPlaceId) -> Result<(), LowerError> {
        let condition = self.emit("file.owner.live", Some(Type::Bool), MirOperation::ReadPlace(live))?;
        let drop_block = self.new_block(self.span(), "file.owner.drop")?;
        let after = self.new_block(self.span(), "file.owner.after-drop")?;
        self.terminate(MirTerminator::Branch {
            condition,
            then_target: drop_block,
            else_target: after,
        });
        self.switch_to(drop_block);
        let ty = self.places.iter().find(|row| row.id == place)
            .map(|row| row.ty.clone())
            .ok_or_else(|| self.error(self.span(), "file owner cleanup has no place"))?;
        let value = self.emit_mir_type("file.owner.cleanup.move", Some(ty), MirOperation::MovePlace { place })?;
        self.emit("file.owner.cleanup.drop", None, MirOperation::Drop { value, kind: MirDropKind::Value })?;
        self.terminate(MirTerminator::Jump { target: after });
        self.switch_to(after);
        Ok(())
    }
    fn emit_owned_place_cleanup(
        &mut self,
        place: MirPlaceId,
        live: MirPlaceId,
        kind: MirDropKind,
    ) -> Result<(), LowerError> {
        let condition = self.emit(
            "owned.local.live",
            Some(Type::Bool),
            MirOperation::ReadPlace(live),
        )?;
        let drop_block = self.new_block(self.span(), "owned.local.drop")?;
        let after = self.new_block(self.span(), "owned.local.after-drop")?;
        self.terminate(MirTerminator::Branch {
            condition,
            then_target: drop_block,
            else_target: after,
        });
        self.switch_to(drop_block);
        // The cleanup consumes the owned value, so its shared place row must
        // carry Move access, exactly as an explicit last-use move would.
        let span = self.span();
        let ty = {
            let row = self
                .places
                .iter_mut()
                .find(|row| row.id == place)
                .ok_or_else(|| LowerError::new(span, "owned local cleanup has no place"))?;
            retain_place_access(row, MirAccess::Move);
            row.ty.clone()
        };
        let value = self.emit_mir_type(
            "owned.local.cleanup.move",
            Some(ty.clone()),
            MirOperation::MovePlace { place },
        )?;
        match self.scope_end_close(&ty) {
            Some(function) => self.emit_scope_end_close(function, ty, value)?,
            None => {
                self.emit(
                    "owned.local.cleanup.drop",
                    None,
                    MirOperation::Drop { value, kind },
                )?;
            }
        }
        self.terminate(MirTerminator::Jump { target: after });
        self.switch_to(after);
        Ok(())
    }

    /// D-SHAPE-RESOURCE1=A: a still-live local whose type has a nominal
    /// `Close` implementation is closed, not merely released, at scope end.
    /// Inside that type's own `close` body the receiver is the value being
    /// closed, so values of the type are released there without re-entering
    /// `close`.
    pub(super) fn scope_end_close(&self, ty: &MirType) -> Option<MirFunctionId> {
        let function = self.function_registry.close_impl_for(ty)?;
        let current = self.function_registry.id_for(self.function).ok();
        (current != Some(function)).then_some(function)
    }

    /// The same consuming receiver call an explicit `close(^value)` lowers to.
    fn emit_scope_end_close(
        &mut self,
        function: MirFunctionId,
        owner: MirType,
        value: MirValueId,
    ) -> Result<(), LowerError> {
        let return_type = self
            .function_registry
            .return_type_for(function)
            .unwrap_or_else(|| Type::Named(crate::Syntax::INTERNAL_UNIT_TYPE.to_string()));
        let arg = MirCallArg {
            value,
            access: MirAccess::Move,
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
        };
        self.emit(
            "owned.local.cleanup.close",
            Some(return_type),
            MirOperation::Call {
                callee: MirCallee::Method { function, owner },
                args: vec![arg],
                type_args: Vec::new(),
            },
        )?;
        Ok(())
    }

    /// Write `false` to a cleanup live flag in the function's entry block.
    /// The binding site sets the real value, but a cleanup frame may be
    /// exited on a path that never reached the binding; the flag must read
    /// `false` there, never an uninitialized slot. Entry-block instructions
    /// run before control leaves the entry block, so this dominates every
    /// path. When the binding itself is in the entry block, this write lands
    /// before the binding's own write, which is the existing order.
    fn clear_live_flag_at_entry(&mut self, flag: MirPlaceId) -> Result<(), LowerError> {
        let Some(entry) = self.blocks.first().map(|block| block.id) else {
            return Ok(());
        };
        let saved = self.current;
        self.current = entry;
        let result = (|| {
            let off = self.emit(
                "cleanup.live.entry",
                Some(Type::Bool),
                MirOperation::Constant(MirConstant::Bool(false)),
            )?;
            self.emit(
                "cleanup.live.entry.write",
                None,
                MirOperation::WritePlace { place: flag, value: off },
            )
        })();
        self.current = saved;
        result.map(|_| ())
    }

    fn register_owned_drop(
        &mut self,
        place: MirPlaceId,
        ty: &Type,
        ownership: MirOwnership,
        initialized: bool,
    ) -> Result<(), LowerError> {
        if ownership.drop == MirDropKind::None
            || matches!(self.ownership_for(ty).mode, MirOwnershipMode::Copy)
        {
            return Ok(());
        }
        let live_local = TLocal::generated(format!("owned_live_{}", place.0)).as_mutable();
        let live = self.bind_local(&live_local, Type::Bool, true, false, false)?;
        self.clear_live_flag_at_entry(live)?;
        let initial = self.emit(
            "owned.local.live.initial",
            Some(Type::Bool),
            MirOperation::Constant(MirConstant::Bool(initialized)),
        )?;
        self.emit(
            "owned.local.live.initialize",
            None,
            MirOperation::WritePlace {
                place: live,
                value: initial,
            },
        )?;
        self.drop_live_places.insert(place, live);
        self.defer_stack
            .last_mut()
            .expect("function always has a lexical cleanup frame")
            .push_action(DeferredCleanup::DropPlace {
                place,
                live,
                kind: ownership.drop,
            });
        Ok(())
    }

    pub(super) fn emit_file_owner_replacement(&mut self, place: MirPlaceId) -> Result<(), LowerError> {
        for (owner, live) in self.file_owner_leaves(place) {
            self.emit_file_owner_cleanup(owner, live)?;
        }
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
        let forced_ownership = match &operation {
            MirOperation::MovePlace { .. } | MirOperation::Move { .. } => None,
            _ => Some(MirOwnership::Owned),
        };
        self.emit_mir_type_with_ownership(role, value_type, operation, forced_ownership)
    }
    pub(super) fn operation_ownership(
        &self,
        operation: &MirOperation,
        value_type: &MirType,
    ) -> Result<MirOwnership, LowerError> {
        match operation {
            MirOperation::MovePlace { .. } | MirOperation::Move { .. } => {
                Ok(MirOwnership::from_access(MirAccess::Move))
            }
            MirOperation::AddressOf { access, .. } => {
                Ok(MirOwnership::from_access(*access))
            }
            MirOperation::RawAddressOf { .. } => Ok(MirOwnership {
                mode: MirOwnershipMode::ReadBorrow,
                drop: MirDropKind::None,
                moved: false,
                last_use: false,
                gc_root: false,
            }),
            MirOperation::Phi { incoming } => self.phi_ownership(incoming),
            _ => Ok(self.ownership_for(&mir_type_as_ast(value_type))),
        }
    }

    fn phi_ownership(
        &self,
        incoming: &[(MirBlockId, MirValueId)],
    ) -> Result<MirOwnership, LowerError> {
        let Some((_, first)) = incoming.first() else {
            return Err(self.error(self.span(), "checked MIR Phi has no incoming values"));
        };
        let first_ownership = self
            .values
            .iter()
            .find(|(value, _, _, _)| value == first)
            .map(|(_, _, _, ownership)| *ownership)
            .ok_or_else(|| self.error(self.span(), "checked MIR Phi input has no ownership fact"))?;
        let mut all_copy = first_ownership.mode == MirOwnershipMode::Copy;
        let mut all_read_borrow = first_ownership.mode == MirOwnershipMode::ReadBorrow;
        let mut all_write_borrow = first_ownership.mode == MirOwnershipMode::WriteBorrow;
        let mut all_shared = first_ownership.mode == MirOwnershipMode::Shared;
        let mut all_move = first_ownership.mode == MirOwnershipMode::Move;
        let mut all_owned = first_ownership.mode == MirOwnershipMode::Owned;
        for (_, value) in incoming.iter().skip(1) {
            let ownership = self
                .values
                .iter()
                .find(|(candidate, _, _, _)| candidate == value)
                .map(|(_, _, _, ownership)| *ownership)
                .ok_or_else(|| {
                    self.error(self.span(), "checked MIR Phi input has no ownership fact")
                })?;
            if ownership.drop != first_ownership.drop {
                return Err(self.error(
                    self.span(),
                    "checked MIR Phi inputs disagree on their drop obligation",
                ));
            }
            all_copy &= ownership.mode == MirOwnershipMode::Copy;
            all_read_borrow &= ownership.mode == MirOwnershipMode::ReadBorrow;
            all_write_borrow &= ownership.mode == MirOwnershipMode::WriteBorrow;
            all_shared &= ownership.mode == MirOwnershipMode::Shared;
            all_move &= ownership.mode == MirOwnershipMode::Move;
            all_owned &= ownership.mode == MirOwnershipMode::Owned;
        }
        if all_copy || all_read_borrow || all_write_borrow || all_shared || all_move || all_owned {
            return Ok(first_ownership);
        }
        let all_owned_values = incoming.iter().all(|(_, value)| {
            self.values
                .iter()
                .find(|(candidate, _, _, _)| candidate == value)
                .is_some_and(|(_, _, _, ownership)| {
                    matches!(
                        ownership.mode,
                        MirOwnershipMode::Owned | MirOwnershipMode::Move | MirOwnershipMode::Shared
                    )
                })
        });
        if all_owned_values {
            return Ok(MirOwnership {
                mode: MirOwnershipMode::Owned,
                drop: first_ownership.drop,
                moved: false,
                last_use: false,
                gc_root: false,
            });
        }
        Err(self.error(
            self.span(),
            "checked MIR Phi inputs disagree on ownership mode",
        ))
    }

    fn emit_mir_type_with_ownership(
        &mut self,
        role: &str,
        value_type: Option<MirType>,
        operation: MirOperation,
        forced_ownership: Option<MirOwnership>,
    ) -> Result<MirValueId, LowerError> {
        // D-MEM-COPYSEM1: a read parameter's place has no storage to give
        // up. Consuming a read alias of it (a read window or a read-place
        // binding) copies the place, exactly as consuming the copy it
        // stands for would.
        let operation = match operation {
            MirOperation::MovePlace { place } if self.read_parameter_place(place) => {
                MirOperation::ReadPlace(place)
            }
            operation => operation,
        };
        self.retype_absent_operands(&operation, value_type.as_ref())?;
        let absent = value_type
            .as_ref()
            .is_some_and(|ty| matches!(ty.kind(), MirTypeKind::Option(_)))
            && match &operation {
                MirOperation::Absent => true,
                MirOperation::Phi { incoming } => {
                    !incoming.is_empty()
                        && incoming.iter().all(|(_, value)| self.absent_values.contains(value))
                }
                MirOperation::Copy { value, .. } | MirOperation::Move { value } => {
                    self.absent_values.contains(value)
                }
                _ => false,
            };
        self.promote_spawn_closure_to_send(&operation)?;
        let (file_live_places, file_live) = match &operation {
            MirOperation::MovePlace { place } => (self.file_owner_leaves(*place), false),
            MirOperation::WritePlace { place, .. }
            | MirOperation::ReplacePlace { place, .. } => (self.file_owner_leaves(*place), true),
            _ => (Vec::new(), false),
        };
        let (drop_live_place, drop_live) = match &operation {
            MirOperation::MovePlace { place } => (self.drop_live_places.get(place).copied(), false),
            MirOperation::InitializeUninit { place } => {
                (self.drop_live_places.get(place).copied(), true)
            }
            MirOperation::WritePlace { place, .. }
            | MirOperation::ReplacePlace { place, .. } => {
                (self.drop_live_places.get(place).copied(), true)
            }
            _ => (None, false),
        };
        let span = self.span();
        let source_line = self.current_line;
        let detail = format!("{operation:?}");
        let identity = self.reserve_identity("operation", span, role, &detail)?;
        let value = MirValueId(stable_id("mir-value", &identity));
        let op_id = jet_foundation::MIR::MirOpId(stable_id("mir-op", &identity));
        if let Some(value_type) = &value_type {
            let ownership = match forced_ownership {
                Some(ownership) => ownership,
                None => self.operation_ownership(&operation, value_type)?,
            };
            self.values.push((value, value_type.clone(), span, ownership));
        }
        if absent {
            self.absent_values.insert(value);
        }
        if let Some(position) = self.block_positions.get(&self.current).copied() {
            if value_type.is_some() {
                self.value_blocks.entry(value).or_insert(position);
            }
            let block = &mut self.blocks[position];
            block.instructions.push(MirInstruction {
                id: op_id,
                span,
                source_line,
                result: value_type.as_ref().map(|_| value),
                ty: value_type,
                operation,
            });
        }
        for (_, flag) in file_live_places {
            let next = self.emit(
                "file.owner.live.state",
                Some(Type::Bool),
                MirOperation::Constant(MirConstant::Bool(file_live)),
            )?;
            self.emit(
                "file.owner.live.update",
                None,
                MirOperation::WritePlace { place: flag, value: next },
            )?;
        }
        if let Some(flag) = drop_live_place {
            let next = self.emit(
                "owned.local.live.state",
                Some(Type::Bool),
                MirOperation::Constant(MirConstant::Bool(drop_live)),
            )?;
            self.emit(
                "owned.local.live.update",
                None,
                MirOperation::WritePlace { place: flag, value: next },
            )?;
        }
        Ok(value)
    }

    /// Give the absent values an operation consumes the exact option type
    /// of the slot each one fills: a phi's joined type, a written place's
    /// type, a tuple field's type, or a list literal's element type.
    fn retype_absent_operands(
        &mut self,
        operation: &MirOperation,
        value_type: Option<&MirType>,
    ) -> Result<(), LowerError> {
        match operation {
            MirOperation::Phi { incoming } => {
                if let Some(ty) = value_type {
                    for (_, value) in incoming {
                        self.retype_absent(*value, ty)?;
                    }
                }
            }
            MirOperation::WritePlace { place, value } | MirOperation::ReplacePlace { place, value } => {
                if self.absent_values.contains(value) {
                    if let Some(ty) = self
                        .places
                        .iter()
                        .find(|row| row.id == *place)
                        .map(|row| row.ty.clone())
                    {
                        self.retype_absent(*value, &ty)?;
                    }
                }
            }
            MirOperation::Tuple { fields, .. } => {
                if let Some(MirTypeKind::Tuple(types)) = value_type.map(MirType::kind) {
                    for ((_, value), (_, ty)) in fields.iter().zip(types) {
                        self.retype_absent(*value, ty)?;
                    }
                }
            }
            MirOperation::BuildList { values, .. } => {
                if let Some(MirTypeKind::List(elem)) = value_type.map(MirType::kind) {
                    for value in values {
                        self.retype_absent(*value, elem)?;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// A returned absent value takes the function's option return type (or
    /// the option success slot of a fallible return) before any exit chain
    /// stores it in a typed slot.
    fn retype_absent_return(&mut self, value: MirValueId) -> Result<(), LowerError> {
        if !self.absent_values.contains(&value) {
            return Ok(());
        }
        let Some(ret) = self.function.ret.clone() else {
            return Ok(());
        };
        let ret = self.mir_type(&ret)?;
        let ok = match ret.kind() {
            MirTypeKind::Result { ok, .. } if matches!(ok.kind(), MirTypeKind::Option(_)) => {
                Some((**ok).clone())
            }
            _ => None,
        };
        let expected = ok.unwrap_or(ret);
        self.retype_absent(value, &expected)
    }

    /// Retype an absent value (and the absent values a phi, copy or move of
    /// it reads) to `expected` when that is an option type it differs from.
    fn retype_absent(&mut self, value: MirValueId, expected: &MirType) -> Result<(), LowerError> {
        if !self.absent_values.contains(&value)
            || !matches!(expected.kind(), MirTypeKind::Option(_))
        {
            return Ok(());
        }
        let mut pending = vec![value];
        while let Some(value) = pending.pop() {
            let Some(row) = self.values.iter().rposition(|row| row.0 == value) else {
                continue;
            };
            if self.values[row].1 == *expected {
                continue;
            }
            let Some(position) = self.value_blocks.get(&value).copied() else {
                continue;
            };
            let Some(operation) = self.blocks[position]
                .instructions
                .iter()
                .find(|instruction| instruction.result == Some(value))
                .map(|instruction| instruction.operation.clone())
            else {
                continue;
            };
            let ownership = self.operation_ownership(&operation, expected)?;
            self.values[row].1 = expected.clone();
            self.values[row].3 = ownership;
            if let Some(instruction) = self.blocks[position]
                .instructions
                .iter_mut()
                .find(|instruction| instruction.result == Some(value))
            {
                instruction.ty = Some(expected.clone());
            }
            match operation {
                MirOperation::Phi { incoming } => {
                    pending.extend(incoming.into_iter().map(|(_, value)| value));
                }
                MirOperation::Copy { value, .. } | MirOperation::Move { value } => {
                    pending.push(value);
                }
                _ => {}
            }
        }
        Ok(())
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

    /// Materialize the one checked S48 concrete-to-single-trait coercion.
    /// Callers provide the already-resolved source and destination slot types;
    /// this helper only projects that fact to the canonical MIR target ID.
    pub(super) fn trait_box_value(
        &mut self,
        value: MirValueId,
        source: &Type,
        target: &Type,
    ) -> Result<MirValueId, LowerError> {
        // Only declared TIR traits have a dynamic Rust/JIT object row. Builtin
        // structural capabilities are checked statically and never become an
        // S48 single-trait slot here.
        let target = match target.without_user_tags() {
            Type::TraitObject(bounds) if bounds.len() == 1 => {
                let trait_name = bounds.first().expect("single trait bound");
                if !self.is_trait_name(trait_name) {
                    return Err(self.error(
                        self.span(),
                        "checked trait coercion target is not a declared single-trait object",
                    ));
                }
                Type::TraitObject(bounds.clone())
            }
            Type::TraitObject(_) => return Ok(value),
            Type::Named(name) if self.is_trait_name(name) => {
                Type::TraitObject(vec![name.clone()])
            }
            _ => return Ok(value),
        };
        let source = source.without_user_tags();
        if matches!(source, Type::TraitObject(_))
            || matches!(source, Type::Named(name) if self.is_trait_name(name))
        {
            return Ok(value);
        }
        if !matches!(source, Type::Named(_) | Type::Apply { .. }) {
            return Err(self.error(
                self.span(),
                "checked single-trait coercion has no canonical nominal source representation",
            ));
        }
        let target_mir = self.mir_type(&target)?;
        let Some(target_id) = target_mir.identity else {
            return Err(self.error(
                self.span(),
                "checked trait coercion target has no canonical MIR identity",
            ));
        };
        if !matches!(target_mir.kind(), jet_foundation::MIR::MirTypeKind::TraitObject(bounds) if bounds.len() == 1)
        {
            return Err(self.error(
                self.span(),
                "checked trait coercion target is not a single-trait object",
            ));
        }
        self.emit(
            "trait-box",
            Some(target),
            MirOperation::TraitBox {
                value,
                target: target_id,
            },
        )
    }

    pub(super) fn lower_nested_stmts(&mut self, stmts: &[TStmt]) -> Result<(), LowerError> {
        super::tir_to_mir_stmt::lower_stmts(self, stmts)
    }

    pub(super) fn drop_temporary_shared_guard(
        &mut self,
        place: MirPlaceId,
    ) -> Result<(), LowerError> {
        let Some((value, kind)) = (|| {
            let place = self.places.iter().find(|candidate| candidate.id == place)?;
            let MirPlaceBase::Temporary(value) = &place.base else {
                return None;
            };
            let (_, ty, _, ownership) = self
                .values
                .iter()
                .find(|(candidate, ..)| candidate == value)?;
            if !is_shared_guard_mir_type(ty) || ownership.drop == MirDropKind::None {
                return None;
            }
            Some((*value, ownership.drop))
        })() else {
            return Ok(());
        };
        self.emit(
            "temporary.shared-guard.drop",
            None,
            MirOperation::Drop { value, kind },
        )?;
        Ok(())
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
            MirIndexKind::List | MirIndexKind::FixedListProof => match base.without_user_tags() {
                Type::List(inner) | Type::FixedList { elem: inner, .. } => Ok((**inner).clone()),
                Type::Apply { name, args }
                    if args.len() == 1
                        && matches!(name.as_str(), "View" | "ViewMut" | "ComputeViewMut") =>
                {
                    Ok(args[0].clone())
                }
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
        let line = self.source_line();
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

    pub(super) fn lower_index_place(
        &mut self,
        base: &TExpr,
        index: &TExpr,
        kind: MirIndexKind,
        access: MirAccess,
    ) -> Result<MirPlaceId, LowerError> {
        // D-MEM-COPYSEM1: reading one element of a compiler-copied container
        // place reads that element in place. The copy of the whole container
        // (a whole `local.list` on every `local.list[i]` read) is never
        // observable through a read, and the element read copies the element
        // as before. A user `~` copy is a distinct node and is never elided.
        let base = match &base.kind {
            TExprKind::Clone(inner) if access == MirAccess::Read && tir_place_path(inner) => {
                &**inner
            }
            _ => base,
        };
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
            Some(self.intern_prelude_route(super::index_write_route(
                kind, &base.ty, &result_ty, &carrier,
            )?)?)
        } else {
            None
        };
        let line = self.source_line();
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

    pub(super) fn project_deref_place(
        &mut self,
        base: MirPlaceId,
        ty: Type,
        span: Span,
    ) -> Result<MirPlaceId, LowerError> {
        let root = self
            .places
            .iter()
            .find(|place| place.id == base)
            .cloned()
            .ok_or_else(|| self.error(span, "missing checked deref base place"))?;
        let id = self.place_id("deref", &format!("{}", root.id.0))?;
        let mut projections = root.projections;
        projections.push(MirProjection::Deref { span });
        let ty = self.mir_type(&ty)?;
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

    /// D-SHAPE-PLACE1=A: project the fixed-length range window `range` of a
    /// list place. The window keeps the list type, so a write argument or a
    /// mutating receiver reaches the owner's storage on every tier.
    pub(super) fn project_range_place(
        &mut self,
        base: MirPlaceId,
        range: MirValueId,
        line: usize,
        span: Span,
    ) -> Result<MirPlaceId, LowerError> {
        let root = self
            .places
            .iter()
            .find(|place| place.id == base)
            .cloned()
            .ok_or_else(|| self.error(span, "missing checked range window base place"))?;
        let id = self.place_id("range", &format!("{}:{}", root.id.0, range.0))?;
        let location = self.panic_location_at(u32::try_from(line).unwrap_or(u32::MAX));
        let mut projections = root.projections;
        projections.push(MirProjection::Range {
            range,
            location,
            span,
        });
        self.places.push(MirPlace {
            id,
            span,
            ty: root.ty,
            base: root.base,
            projections,
            access: root.access,
            persist_key: None,
        });
        Ok(id)
    }

    pub(super) fn bind_parameter(
        &mut self,
        index: usize,
        name: &str,
        ty: &Type,
        access: AccessConvention,
    ) -> Result<(), LowerError> {
        let access = lower_mir_convention(access);
        let parameter_type = self.mir_type(ty)?;
        let param_value = self.emit_mir_type_with_ownership(
            &format!("parameter.{index}.{name}"),
            Some(parameter_type),
            MirOperation::Parameter {
                index,
                name: name.to_string(),
            },
            Some(MirOwnership::from_access(access)),
        )?;
        if access == MirAccess::Read {
            self.read_parameter_values.insert(param_value);
        }
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
        if matches!(access, MirAccess::Move) {
            self.register_owned_drop(place, ty, MirOwnership::from_access(access), true)?;
            self.register_file_owner(place)?;
        }
        Ok(())
    }

    /// Remember the binding `name` has before a source-named binding
    /// replaces it. A first binding has nothing to restore and is not
    /// recorded. Generated temporaries are never recorded: a re-lowered
    /// compiler temporary must keep its latest binding.
    fn record_shadowed_local(&mut self, name: &str) {
        let place = self.local_places.get(name).copied();
        let ty = self.local_types.get(name).cloned();
        if place.is_none() && ty.is_none() {
            return;
        }
        let value = self.local_values.get(name).copied();
        self.shadowed_locals.push(ShadowedLocal {
            name: name.to_string(),
            place,
            ty,
            value,
        });
    }

    pub(super) fn shadow_mark(&self) -> usize {
        self.shadowed_locals.len()
    }

    /// End every binding recorded since `mark`: each rebound name reads its
    /// outer binding again. Returns the inner bindings, innermost last, so a
    /// caller can lower a sibling path without them and then reinstate them
    /// with [`Self::reinstate_shadowed_locals`].
    pub(super) fn restore_shadowed_locals(&mut self, mark: usize) -> Vec<ShadowedLocal> {
        let mut inner = Vec::new();
        while self.shadowed_locals.len() > mark {
            let Some(outer) = self.shadowed_locals.pop() else {
                break;
            };
            inner.push(ShadowedLocal {
                place: restore_entry(&mut self.local_places, &outer.name, outer.place),
                ty: restore_entry(&mut self.local_types, &outer.name, outer.ty),
                value: restore_entry(&mut self.local_values, &outer.name, outer.value),
                name: outer.name,
            });
        }
        inner.reverse();
        inner
    }

    /// Bring back inner bindings that [`Self::restore_shadowed_locals`]
    /// ended, recording the outer ones again for the enclosing scope's end.
    pub(super) fn reinstate_shadowed_locals(&mut self, inner: Vec<ShadowedLocal>) {
        for binding in inner {
            self.record_shadowed_local(&binding.name);
            restore_entry(&mut self.local_places, &binding.name, binding.place);
            restore_entry(&mut self.local_types, &binding.name, binding.ty);
            restore_entry(&mut self.local_values, &binding.name, binding.value);
        }
    }

    pub(super) fn bind_local(
        &mut self,
        local: &TLocal,
        ty: Type,
        mutable: bool,
        comptime: bool,
        uninit: bool,
    ) -> Result<MirPlaceId, LowerError> {
        if !local.generated {
            self.record_shadowed_local(&local.name);
        }
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
        let ownership = self.ownership_for(&ty);
        self.locals.push(MirLocal {
            id: local_id,
            name: local.name.clone(),
            span: self.span(),
            ty: mir_ty,
            place,
            mutable,
            ownership,
            comptime,
            uninit,
            arena_view: false,
            string_view: false,
            gc_root: false,
        });
        self.register_owned_drop(place, &ty, ownership, false)?;
        Ok(place)
    }
    /// Bind a source-level place window to its checked owner place. A single
    /// `&list[index]` cannot carry a stable raw address in resident tiers, so
    /// these locals stay as MIR aliases rather than copied scalar values.
    pub(super) fn bind_local_alias(
        &mut self,
        local: &TLocal,
        ty: Type,
        place: MirPlaceId,
        mutable: bool,
    ) -> Result<MirPlaceId, LowerError> {
        if !local.generated {
            self.record_shadowed_local(&local.name);
        }
        self.local_types.insert(local.name.clone(), ty.clone());
        let mir_ty = self.mir_type(&ty)?;
        let local_identity = self.reserve_identity("local", self.span(), &local.name, "")?;
        let local_id = MirLocalId(stable_id("mir-local", &local_identity));
        self.local_places.insert(local.name.clone(), place);
        self.locals.push(MirLocal {
            id: local_id,
            name: local.name.clone(),
            span: self.span(),
            ty: mir_ty,
            place,
            mutable,
            ownership: MirOwnership::from_access(if mutable { MirAccess::Write } else { MirAccess::Read }),
            comptime: false,
            uninit: false,
            arena_view: false,
            string_view: false,
            gc_root: false,
        });
        Ok(place)
    }

    /// D-OPT-WRITE1 (#3974): lower a pattern subject. A `&place` subject, or
    /// the generated local a nested-pattern switch binds to one, is a write
    /// window: the variant tests read the place, and every payload binding
    /// aliases it through `MirProjection::Payload`, so edits reach the owner's
    /// storage instead of a copy. D-MEM-COPYSEM1: a subject naming a read
    /// parameter's place is a read window the same way, so testing it never
    /// copies the whole subject.
    pub(super) fn lower_pattern_subject(
        &mut self,
        subject: &TExpr,
    ) -> Result<MirValueId, LowerError> {
        let window = match &subject.kind {
            TExprKind::Borrow {
                place,
                mutable: true,
            } => Some(self.lower_place(&TPlace::Expr(place.clone()), MirAccess::Write)?),
            TExprKind::Local(local)
                if local.generated && self.window_aliases.contains(&local.name) =>
            {
                self.local_places.get(&local.name).copied()
            }
            _ => None,
        };
        let Some(window) = window else {
            if let Some(value) = self.lower_read_window(subject)? {
                return Ok(value);
            }
            return self.lower_child(subject);
        };
        let value = self.emit(
            "pattern.window",
            Some(subject.ty.clone()),
            MirOperation::ReadPlace(window),
        )?;
        self.pattern_windows.insert(value, window);
        Ok(value)
    }

    /// D-MEM-COPYSEM1: read `subject` as a read window when it names a read
    /// parameter's place; `None` leaves the subject to the ordinary value path.
    /// A compiler-owned subject keeps the value path: its native Rust carrier
    /// can hold a payload in a host representation (`DataTree.Int` is `i64`)
    /// that only the checked payload readers widen to the Jet type, so a
    /// payload place aliased through it would read the wrong type.
    pub(super) fn lower_read_window(
        &mut self,
        subject: &TExpr,
    ) -> Result<Option<MirValueId>, LowerError> {
        let compiler_owned = match &subject.ty {
            Type::Named(name) | Type::Apply { name, .. } => {
                super::tir_to_mir_types::is_compiler_owned_type(name)
            }
            _ => false,
        };
        if compiler_owned {
            return Ok(None);
        }
        let Some(window) = self.read_window_place(subject)? else {
            return Ok(None);
        };
        let value = self.emit(
            "pattern.read-window",
            Some(subject.ty.clone()),
            MirOperation::ReadPlace(window),
        )?;
        self.read_windows.insert(window);
        self.pattern_windows.insert(value, window);
        Ok(Some(value))
    }

    /// The read-window place `subject` was lowered from, if any.
    pub(super) fn read_window_of(&self, subject: MirValueId) -> Option<MirPlaceId> {
        self.pattern_windows
            .get(&subject)
            .copied()
            .filter(|place| self.read_windows.contains(place))
    }

    /// D-MEM-COPYSEM1: the place `expr` names when it is a plain local or
    /// field-read path (under at most a compiler-inserted clone or a sema
    /// read window `Borrow`) over a read parameter. A user `~` copy is never
    /// elided.
    pub(super) fn read_window_place(
        &mut self,
        expr: &TExpr,
    ) -> Result<Option<MirPlaceId>, LowerError> {
        let expr = match &expr.kind {
            TExprKind::Clone(inner)
            | TExprKind::Borrow {
                place: inner,
                mutable: false,
            } => &**inner,
            _ => expr,
        };
        if !tir_place_path(expr) {
            return Ok(None);
        }
        let Some(place) =
            super::tir_to_mir_expr::lower_receiver_place(self, expr, MirAccess::Read)?
        else {
            return Ok(None);
        };
        Ok(self.read_parameter_place(place).then_some(place))
    }

    /// D-MEM-COPYSEM1: true when `place` is rooted at a read parameter through
    /// field, payload, and list-index projections only. A read parameter
    /// cannot change while the function runs, so reading such a place in
    /// place is indistinguishable from reading a copy of it.
    pub(super) fn read_parameter_place(&self, place: MirPlaceId) -> bool {
        let Some(row) = self.places.iter().find(|candidate| candidate.id == place) else {
            return false;
        };
        let MirPlaceBase::Parameter(value) = row.base else {
            return false;
        };
        row.projections.iter().all(|projection| {
            matches!(
                projection,
                MirProjection::Field { .. }
                    | MirProjection::Payload { .. }
                    | MirProjection::Index {
                        kind: MirIndexKind::List,
                        ..
                    }
            )
        }) && self.read_parameter_values.contains(&value)
    }

    /// `base` extended by one projection, typed `ty`.
    fn project_window_place(
        &mut self,
        base: MirPlaceId,
        projection: MirProjection,
        ty: &Type,
    ) -> Result<MirPlaceId, LowerError> {
        let span = self.span();
        let root = self
            .places
            .iter()
            .find(|place| place.id == base)
            .cloned()
            .ok_or_else(|| self.error(span, "missing checked pattern window place"))?;
        let id = self.place_id("pattern-window", &format!("{}:{projection:?}", root.id.0))?;
        let read = self.read_windows.contains(&base);
        let mut projections = root.projections;
        projections.push(projection);
        let ty = self.mir_type(ty)?;
        self.places.push(MirPlace {
            id,
            span,
            ty,
            base: root.base,
            projections,
            access: if read { MirAccess::Read } else { MirAccess::Write },
            persist_key: None,
        });
        if read {
            self.read_windows.insert(id);
        }
        Ok(id)
    }

    pub(super) fn project_payload_place(
        &mut self,
        base: MirPlaceId,
        kind: jet_foundation::MIR::MirPayloadKind,
        ty: &Type,
    ) -> Result<MirPlaceId, LowerError> {
        let span = self.span();
        self.project_window_place(base, MirProjection::Payload { kind, span }, ty)
    }

    /// Bind a payload name under a window subject as an alias of `place`:
    /// reads (and, under a write window, edits) go through the owner, nothing
    /// is copied.
    fn bind_window_binding(
        &mut self,
        name: &str,
        ty: Type,
        place: MirPlaceId,
    ) -> Result<(), LowerError> {
        if name.is_empty() || name == "_" {
            return Ok(());
        }
        let mutable = !self.read_windows.contains(&place);
        self.bind_local_alias(&TLocal::user(name.to_string()), ty, place, mutable)?;
        Ok(())
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
        // A mutable binding owns its copy; only an immutable one may alias a
        // read window.
        let window = self
            .pattern_windows
            .get(&subject)
            .copied()
            .filter(|window| !(pattern.mutable && self.read_windows.contains(window)));
        let condition = self.lower_pattern_shape_condition(
            subject,
            &subject_ty,
            pattern.owner,
            &shape,
            pattern.mutable,
            false,
            window,
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
        // D-OPT-WRITE1 (#3974): the owner place `subject` was read from when
        // the pattern matches through a `&place` write window.
        window: Option<MirPlaceId>,
    ) -> Result<MirValueId, LowerError> {
        match shape {
            MirPatternShape::Variant {
                variant, bindings, ..
            } => {
                // A checked Result may arrive as an enum-style `.Ok(value)` or
                // `.Err(error)` pattern. It is still the same owning carrier as
                // Pattern::Ok/Err: test by reference and extract only on the
                // selected arm with ResultValue, never EnumPayload's clone.
                if matches!(subject_ty.without_user_tags(), Type::Result { .. })
                    && matches!(variant.as_str(), "Ok" | "Err")
                {
                    let (binding, binding_span, inner) = match bindings.as_slice() {
                        [] | [MirPatternBinding::Wildcard] => ("", self.span(), None),
                        [MirPatternBinding::Bind { name, span }] => (name.as_str(), *span, None),
                        [MirPatternBinding::Nested(inner)] => ("", self.span(), Some(inner.clone())),
                        _ => {
                            return Err(self.error(
                                self.span(),
                                "checked Result variant requires one payload binding",
                            ))
                        }
                    };
                    let shape = if variant == "Ok" {
                        MirPatternShape::Ok {
                            binding: binding.to_string(),
                            binding_span,
                            inner,
                            span: self.span(),
                        }
                    } else {
                        MirPatternShape::Err {
                            binding: binding.to_string(),
                            binding_span,
                            inner,
                            span: self.span(),
                        }
                    };
                    return self.lower_pattern_shape_condition(
                        subject,
                        subject_ty,
                        None,
                        &shape,
                        mutable,
                        reuse_bindings,
                        window,
                    );
                }
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
                                if let Some(window) = window {
                                    let place = ctx.project_payload_place(
                                        window,
                                        jet_foundation::MIR::MirPayloadKind::Enum {
                                            owner,
                                            variant: variant.clone(),
                                            index,
                                        },
                                        &ty,
                                    )?;
                                    ctx.bind_window_binding(name, ty, place)?;
                                    continue;
                                }
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
                            MirPatternBinding::Nested(inner) => {
                                let ty = ctx
                                    .enum_payload_type_for(subject_ty, variant, index)
                                    .or_else(|_| ctx.variant_payload_type(owner, variant, index))?;
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
                                let nested_window = match window {
                                    Some(window) => Some(ctx.project_payload_place(
                                        window,
                                        jet_foundation::MIR::MirPayloadKind::Enum {
                                            owner,
                                            variant: variant.clone(),
                                            index,
                                        },
                                        &ty,
                                    )?),
                                    None => None,
                                };
                                tests.push(ctx.lower_nested_pattern_condition(
                                    value,
                                    &ty,
                                    inner,
                                    mutable,
                                    reuse_bindings,
                                    nested_window,
                                )?);
                            }
                        }
                    }
                    ctx.combine_pattern_tests(tests, jet_foundation::AST::BinOp::And)
                })
            }
            MirPatternShape::Present {
                binding,
                inner: nested,
                ..
            } => {
                let inner = match subject_ty {
                    Type::Option(inner) => (**inner).clone(),
                    _ => {
                        return Err(self.error(
                            self.span(),
                            "checked present pattern has a non-optional subject",
                        ));
                    }
                };
                let test = self.emit_checked(
                    "pattern",
                    Some(&Type::Bool),
                    MirOperation::OptionIsSome { subject },
                )?;
                if nested.is_none() && (binding.is_empty() || binding == "_") {
                    return Ok(test);
                }
                self.lower_guarded_pattern(test, |ctx| {
                    if let (Some(window), None) = (window, nested) {
                        let place = ctx.project_payload_place(
                            window,
                            jet_foundation::MIR::MirPayloadKind::Option,
                            &inner,
                        )?;
                        ctx.bind_window_binding(binding, inner, place)?;
                        return Ok(test);
                    }
                    let value = ctx.emit_checked(
                        "pattern",
                        Some(&inner),
                        MirOperation::OptionValue { subject },
                    )?;
                    if let Some(nested) = nested {
                        let nested_window = match window {
                            Some(window) => Some(ctx.project_payload_place(
                                window,
                                jet_foundation::MIR::MirPayloadKind::Option,
                                &inner,
                            )?),
                            None => None,
                        };
                        return ctx.lower_nested_pattern_condition(
                            value,
                            &inner,
                            nested,
                            mutable,
                            reuse_bindings,
                            nested_window,
                        );
                    }
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
            MirPatternShape::Ok {
                binding,
                inner: nested,
                ..
            }
            | MirPatternShape::Err {
                binding,
                inner: nested,
                ..
            } => {
                let ok = matches!(shape, MirPatternShape::Ok { .. });
                let (success, error) = match subject_ty {
                    Type::Result { ok, err } => ((**ok).clone(), (**err).clone()),
                    _ => {
                        return Err(self.error(
                            self.span(),
                            "checked result pattern has a non-result subject",
                        ));
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
                if nested.is_none() && (binding.is_empty() || binding == "_") {
                    return Ok(test);
                }
                self.lower_guarded_pattern(test, |ctx| {
                    let value_ty = if ok { success } else { error };
                    let payload = jet_foundation::MIR::MirPayloadKind::Result { ok };
                    if let (Some(window), None) = (window, nested) {
                        let place = ctx.project_payload_place(window, payload, &value_ty)?;
                        ctx.bind_window_binding(binding, value_ty, place)?;
                        return Ok(test);
                    }
                    let value = ctx.emit_checked(
                        "pattern",
                        Some(&value_ty),
                        MirOperation::ResultValue { subject, ok },
                    )?;
                    if let Some(nested) = nested {
                        let nested_window = match window {
                            Some(window) => {
                                Some(ctx.project_payload_place(window, payload, &value_ty)?)
                            }
                            None => None,
                        };
                        return ctx.lower_nested_pattern_condition(
                            value,
                            &value_ty,
                            nested,
                            mutable,
                            reuse_bindings,
                            nested_window,
                        );
                    }
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
                        // One name cannot alias two payload slots: or-pattern
                        // bindings stay read-only values even under a `&place`
                        // subject (sema declares them so).
                        None,
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
                    if let (Some(window), Some(name)) = (window, binding) {
                        let span = self.span();
                        let place = self.project_window_place(
                            window,
                            MirProjection::Field {
                                field: field_id,
                                span,
                            },
                            &value_ty,
                        )?;
                        self.bind_window_binding(name, value_ty, place)?;
                        continue;
                    }
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

    /// S31: test a payload value against a nested pattern. The nested
    /// pattern's owner is the payload type's canonical owner; a D-TAG1 group
    /// head expands to its leaves exactly as at the top level.
    fn lower_nested_pattern_condition(
        &mut self,
        subject: MirValueId,
        subject_ty: &Type,
        shape: &MirPatternShape,
        mutable: bool,
        reuse_bindings: bool,
        window: Option<MirPlaceId>,
    ) -> Result<MirValueId, LowerError> {
        let owner = self.field_owner_id_for_type(subject_ty).ok();
        let shape = self.expand_nested_group_heads(shape, owner);
        self.lower_pattern_shape_condition(
            subject,
            subject_ty,
            owner,
            &shape,
            mutable,
            reuse_bindings,
            window,
        )
    }

    fn expand_nested_group_heads(
        &self,
        shape: &MirPatternShape,
        owner: Option<MirTypeId>,
    ) -> MirPatternShape {
        match shape {
            MirPatternShape::Variant {
                variant,
                bindings,
                leading_dot,
                span,
            } if bindings.is_empty() => {
                match owner.and_then(|owner| self.enum_group_leaves(owner, variant)) {
                    Some(leaves) => MirPatternShape::Or {
                        alternatives: leaves
                            .into_iter()
                            .map(|variant| MirPatternShape::Variant {
                                variant,
                                bindings: Vec::new(),
                                leading_dot: *leading_dot,
                                span: *span,
                            })
                            .collect(),
                        span: *span,
                    },
                    None => shape.clone(),
                }
            }
            MirPatternShape::Or { alternatives, span } => MirPatternShape::Or {
                alternatives: alternatives
                    .iter()
                    .map(|alternative| self.expand_nested_group_heads(alternative, owner))
                    .collect(),
                span: *span,
            },
            _ => shape.clone(),
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

    pub(super) fn bind_success_pattern(
        &mut self,
        subject: MirValueId,
        pattern: &jet_foundation::MIR::MirPattern,
    ) -> Result<(), LowerError> {
        let subject_ty = self.value_source_type(subject)?;
        self.bind_success_shape(
            subject,
            &subject_ty,
            pattern.owner,
            &pattern.shape,
            pattern.mutable,
        )
    }

    fn bind_success_shape(
        &mut self,
        subject: MirValueId,
        subject_ty: &Type,
        owner: Option<jet_foundation::MIR::MirTypeId>,
        shape: &jet_foundation::MIR::MirPatternShape,
        mutable: bool,
    ) -> Result<(), LowerError> {
        use jet_foundation::MIR::{MirOperation, MirPatternBinding, MirPatternShape, MirPayloadKind};
        // D-MEM-COPYSEM1: under a read window an immutable binding aliases
        // the payload in place instead of copying it out.
        let window = self
            .pattern_windows
            .get(&subject)
            .copied()
            .filter(|window| !mutable && self.read_windows.contains(window));
        match shape {
            MirPatternShape::Variant {
                variant, bindings, ..
            } => {
                let Some(owner) = owner else {
                    return Ok(());
                };
                for (index, binding) in bindings.iter().enumerate() {
                    match binding {
                        MirPatternBinding::Bind { name, .. } => {
                            let ty = self.variant_payload_type(owner, variant, index)?;
                            if let Some(window) = window {
                                let kind = MirPayloadKind::Enum {
                                    owner,
                                    variant: variant.clone(),
                                    index,
                                };
                                let place = self.project_payload_place(window, kind, &ty)?;
                                self.bind_window_binding(name, ty, place)?;
                                continue;
                            }
                            let value = self.emit_checked(
                                "pattern",
                                Some(&ty),
                                MirOperation::EnumPayload {
                                    subject,
                                    owner,
                                    variant: variant.clone(),
                                    index,
                                },
                            )?;
                            self.bind_pattern_value(name, ty, value, mutable, false)?;
                        }
                        MirPatternBinding::Nested(inner) => {
                            let ty = self
                                .enum_payload_type_for(subject_ty, variant, index)
                                .or_else(|_| self.variant_payload_type(owner, variant, index))?;
                            let value = self.emit_checked(
                                "pattern",
                                Some(&ty),
                                MirOperation::EnumPayload {
                                    subject,
                                    owner,
                                    variant: variant.clone(),
                                    index,
                                },
                            )?;
                            let owner = self.field_owner_id_for_type(&ty).ok();
                            self.bind_success_shape(value, &ty, owner, inner, mutable)?;
                        }
                        MirPatternBinding::Wildcard | MirPatternBinding::Range { .. } => {}
                    }
                }
                Ok(())
            }
            MirPatternShape::Present {
                binding,
                inner: nested,
                ..
            } => {
                if nested.is_none() && (binding.is_empty() || binding == "_") {
                    return Ok(());
                }
                let inner = match subject_ty {
                    Type::Option(inner) => (**inner).clone(),
                    _ => {
                        return Err(self.error(
                            self.span(),
                            "checked present pattern has a non-optional subject",
                        ));
                    }
                };
                if let (Some(window), None) = (window, nested) {
                    let place = self.project_payload_place(window, MirPayloadKind::Option, &inner)?;
                    return self.bind_window_binding(binding, inner, place);
                }
                let value = self.emit_checked(
                    "pattern",
                    Some(&inner),
                    MirOperation::OptionValue { subject },
                )?;
                if let Some(nested) = nested {
                    let owner = self.field_owner_id_for_type(&inner).ok();
                    return self.bind_success_shape(value, &inner, owner, nested, mutable);
                }
                self.bind_pattern_value(binding, inner, value, mutable, false)
            }
            MirPatternShape::Ok {
                binding,
                inner: nested,
                ..
            }
            | MirPatternShape::Err {
                binding,
                inner: nested,
                ..
            } => {
                if nested.is_none() && (binding.is_empty() || binding == "_") {
                    return Ok(());
                }
                let ok = matches!(shape, MirPatternShape::Ok { .. });
                let (success, error) = match subject_ty {
                    Type::Result { ok, err } => ((**ok).clone(), (**err).clone()),
                    _ => {
                        return Err(self.error(
                            self.span(),
                            "checked result pattern has a non-result subject",
                        ));
                    }
                };
                let value_ty = if ok { success } else { error };
                if let (Some(window), None) = (window, nested) {
                    let place =
                        self.project_payload_place(window, MirPayloadKind::Result { ok }, &value_ty)?;
                    return self.bind_window_binding(binding, value_ty, place);
                }
                let value = self.emit_checked(
                    "pattern",
                    Some(&value_ty),
                    MirOperation::ResultValue { subject, ok },
                )?;
                if let Some(nested) = nested {
                    let owner = self.field_owner_id_for_type(&value_ty).ok();
                    return self.bind_success_shape(value, &value_ty, owner, nested, mutable);
                }
                self.bind_pattern_value(binding, value_ty, value, mutable, false)
            }
            MirPatternShape::Or { alternatives, .. } => {
                for alt in alternatives {
                    self.bind_success_shape(subject, subject_ty, owner, alt, mutable)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
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

    pub(super) fn enum_payload_type_for(
        &self,
        ty: &Type,
        variant: &str,
        index: usize,
    ) -> Result<Type, LowerError> {
        let owner = self.field_owner_id_for_type(ty)?;
        let payload = self.variant_payload_type(owner, variant, index)?;
        if let Type::Apply { args, .. } = ty {
            if let Some(definition) = self.type_defs.iter().find(|definition| definition.id == owner) {
                let substitutions = definition
                    .generic_params
                    .iter()
                    .zip(args)
                    .map(|(param, arg)| (param.name.clone(), arg.clone()))
                    .collect();
                return Ok(crate::Generics::substitute_type(&payload, &substitutions));
            }
        }
        Ok(payload)
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
        let normalized = self.normalize_contextual_type(ty);
        let ty = field_owner_type(&normalized);
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

    /// Whether `field` is a stored (not computed) field of the nominal struct
    /// `ty`. Only a stored field can be read through a projected place.
    pub(super) fn is_stored_struct_field(&mut self, ty: &Type, field: &str) -> bool {
        let Ok(mir_ty) = self.mir_type(ty) else {
            return false;
        };
        let Some(identity) = mir_ty.identity else {
            return false;
        };
        self.type_defs.iter().any(|definition| {
            definition.id == identity
                && matches!(&definition.kind, MirTypeDefKind::Struct { fields, .. }
                    if fields.iter().any(|row| row.name == field && !row.computed))
        })
    }

    /// String-view bindings retain their Jet-level `String` type for source
    /// dispatch, but their MIR place carries the borrowed `View<str>` ABI
    /// carrier. Local reads must use that place type or the emitter creates an
    /// `Option<String>` value slot for a borrowed `&str` load.
    pub(super) fn place_is_string_view(&self, place: MirPlaceId) -> bool {
        self.places
            .iter()
            .find(|candidate| candidate.id == place)
            .is_some_and(|candidate| {
                matches!(
                    candidate.ty.kind(),
                    MirTypeKind::Apply { name, args }
                        if name.name == "View"
                            && matches!(
                                args.as_slice(),
                                [arg]
                                    if matches!(
                                        arg.kind(),
                                        MirTypeKind::Apply { name, args }
                                            if args.is_empty() && name.name == "str"
                                    )
                            )
                )
            })
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

    pub(super) fn place_id(&mut self, kind: &str, name: &str) -> Result<MirPlaceId, LowerError> {
        let identity = self.reserve_identity("place", self.span(), kind, name)?;
        Ok(MirPlaceId(stable_id("mir-place", &identity)))
    }

    fn place_shares_record_handle(&self, place: MirPlaceId) -> bool {
        let Some(row) = self.places.iter().find(|candidate| candidate.id == place) else {
            return false;
        };
        matches!(row.projections.last(), Some(MirProjection::Index { .. }))
            && matches!(
                row.ty.layout.abi,
                MirAbi::Nominal | MirAbi::Aggregate | MirAbi::Sequence | MirAbi::Dynamic
            )
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
            let ownership = if moved {
                MirOwnership::from_access(MirAccess::Move)
            } else if cloned {
                MirOwnership::Owned
            } else {
                MirOwnership::from_access(access)
            };
            let mir_ty = self.mir_type(ty)?;
            let capture = self.emit_mir_type_with_ownership(
                &format!("capture.{slot}.{source}"),
                Some(mir_ty.clone()),
                MirOperation::Capture { slot },
                Some(ownership),
            )?;
            self.capture_params.push(MirCaptureParam {
                slot,
                name: source.clone(),
                span,
                ty: mir_ty.clone(),
                access,
                ownership,
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
            if moved {
                let source_local = TLocal::user(source.clone());
                let source_place = self.place_for_local(&source_local, access)?;
                self.register_owned_drop(source_place, ty, ownership, true)?;
            }
        }
        Ok(())
    }

    pub(super) fn lower_lambda(&mut self, lambda: &TLambda) -> Result<MirValueId, LowerError> {
        self.lower_lambda_kind(lambda, false, None)
    }
    /// Spawn closures cloned from one fenced statement share source spans.
    /// Preserve the first identity and suffix only later occurrences.
    pub(super) fn lower_spawn_lambda(
        &mut self,
        lambda: &TLambda,
    ) -> Result<MirValueId, LowerError> {
        let base = construct_identity(self.function, "lambda", lambda.source_span, "spawn", "");
        let occurrence = self.reserve_identity_occurrence(&base);
        let identity = (occurrence > 0).then(|| format!("spawn-{occurrence}"));
        self.lower_lambda_kind(lambda, false, identity.as_deref())
    }

    pub(super) fn lower_synthetic_lambda(
        &mut self,
        lambda: &TLambda,
        purpose: &str,
    ) -> Result<MirValueId, LowerError> {
        let identity = format!("{purpose}-{}", self.nested_functions.len());
        self.lower_lambda_kind(lambda, false, Some(&identity))
    }

    /// A function value whose checked return is a raw payload `R` entering a
    /// slot whose effective callable return is `Result<R, E>` (a `fn(T) R`
    /// parameter, see `Type::with_effective_fn_returns`). Lambdas bound or
    /// returned without that slot keep their payload return, so the value is
    /// wrapped once, here, in a closure that calls it and returns `Ok`. Every
    /// tier then sees one callable ABI at the slot.
    pub(super) fn adapt_fn_value_carrier(
        &mut self,
        value: MirValueId,
        source: &Type,
        slot: &Type,
    ) -> Result<MirValueId, LowerError> {
        let Some((params, raw_ret, carrier_ret)) = fn_value_carrier_adaptation(source, slot) else {
            return Ok(value);
        };
        let source = source.without_user_tags().clone();
        let local = TLocal::generated(format!("fn_carrier_{}", value.0));
        let name = local.name.clone();
        let place = self.bind_local(&local, source.clone(), false, false, false)?;
        self.emit(
            "fn-carrier.bind",
            None,
            MirOperation::WritePlace { place, value },
        )?;
        let source_params = (0..params.len())
            .map(|index| format!("fn_carrier_arg_{index}"))
            .collect::<Vec<_>>();
        let args = source_params
            .iter()
            .zip(params.iter())
            .map(|(param, ty)| TCallArg {
                value: TExpr {
                    ty: ty.clone(),
                    kind: TExprKind::Local(TLocal::user(param)),
                },
                template_items: None,
                borrow: !ty.is_scalar(),
                mut_borrow: false,
                clone: false,
                arc_clone: false,
                fn_coerce: None,
                widen_to_vec: false,
                widen_to_union: None,
                box_as_trait: None,
            })
            .collect::<Vec<_>>();
        let call = TExpr {
            ty: raw_ret,
            kind: TExprKind::FnValue {
                kind: super::TFnValueKind::Call {
                    callee: Box::new(TExpr {
                        ty: source.clone(),
                        kind: TExprKind::Local(local),
                    }),
                    args,
                },
            },
        };
        let body = TExpr {
            ty: carrier_ret.clone(),
            kind: TExprKind::Ok(Box::new(call)),
        };
        let lambda = TLambda {
            executable: TLambdaBody::Expr(Box::new(body)),
            source_span: self.span(),
            frame_schedule: None,
            frame_schedule_derivation: None,
            // The adapter lives only for the call it is passed to, so it
            // borrows the bound callable like any nonescaping read capture.
            capture_facts: super::TCaptureFacts::default(),
            host_param_conventions: None,
            failure_carrier: TFailureCarrier::from_checked_type(&carrier_ret),
            effects: super::TEffectFacts::default(),
            source_params,
            param_types: params,
            ret: Some(carrier_ret),
            is_move: false,
            boxed: false,
            rc: false,
            arc: false,
            captures: vec![(name.clone(), name, source)],
            capture_origins: Default::default(),
            materialized_captures: Vec::new(),
            frozen_captures: Vec::new(),
            uses_stack_sentry: false,
            jit_name: String::new(),
        };
        self.lower_synthetic_lambda(&lambda, "fn-carrier")
    }

    /// #3740: a function value keeps the `Result` carrier its checked `fn`
    /// type names, while a plain-return function (`T Never!`) returns `T`.
    /// Close over the call once and lift its value into `Ok` — the same seam
    /// `adapt_fn_value_carrier` gives a raw-payload value — so every tier
    /// sees one callable ABI in the value slot. The slot may name `T Never!`
    /// or, when it was checked before the failure solve (a generated policy
    /// wrapper's `call` slot, #3708), the default `Result<T, Err>`. `None`
    /// when the named function already returns the value type's carrier.
    pub(super) fn plain_named_fn_value(
        &mut self,
        name: &str,
        function: MirFunctionId,
        value_ty: &Type,
        send: bool,
    ) -> Result<Option<MirValueId>, LowerError> {
        let Type::Fn {
            params,
            ret: Some(ret),
            ..
        } = value_ty.without_user_tags()
        else {
            return Ok(None);
        };
        let Ok(Some(returned)) = self.function_registry.call_return_type_for(function, &[]) else {
            return Ok(None);
        };
        if !is_plain_call_return(&returned, ret) {
            return Ok(None);
        }
        let source_params = (0..params.len())
            .map(|index| format!("plain_fn_arg_{index}"))
            .collect::<Vec<_>>();
        let args = source_params
            .iter()
            .zip(params.iter())
            .map(|(param, ty)| TCallArg {
                value: TExpr {
                    ty: ty.clone(),
                    kind: TExprKind::Local(TLocal::user(param)),
                },
                template_items: None,
                borrow: !ty.is_scalar(),
                mut_borrow: false,
                clone: false,
                arc_clone: false,
                fn_coerce: None,
                widen_to_vec: false,
                widen_to_union: None,
                box_as_trait: None,
            })
            .collect::<Vec<_>>();
        let carrier = ret.as_ref().clone();
        let body = TExpr {
            ty: carrier.clone(),
            kind: TExprKind::Ok(Box::new(TExpr {
                ty: returned,
                kind: TExprKind::Call {
                    name: name.to_string(),
                    type_args: Vec::new(),
                    args,
                },
            })),
        };
        let lambda = TLambda {
            executable: TLambdaBody::Expr(Box::new(body)),
            source_span: self.span(),
            frame_schedule: None,
            frame_schedule_derivation: None,
            capture_facts: super::TCaptureFacts::default(),
            host_param_conventions: None,
            failure_carrier: TFailureCarrier::from_checked_type(&carrier),
            effects: super::TEffectFacts::default(),
            source_params,
            param_types: params.clone(),
            ret: Some(carrier),
            is_move: false,
            boxed: false,
            rc: false,
            arc: false,
            captures: Vec::new(),
            capture_origins: Default::default(),
            materialized_captures: Vec::new(),
            frozen_captures: Vec::new(),
            uses_stack_sentry: false,
            jit_name: String::new(),
        };
        let identity = format!("plain-fn-{}", self.nested_functions.len());
        self.lower_lambda_kind(&lambda, send, Some(&identity)).map(Some)
    }

    /// #3740: reconcile a user call's executable return with the carrier its
    /// checked expression names. A plain-return callee hands its value
    /// straight to the propagating `Try` whose operand it is, and is lifted
    /// into `Ok` once where the source still consumes the `T Never!`
    /// carrier (a `??`, a match, a carrier-typed slot).
    pub(super) fn reconcile_plain_call_return(
        &mut self,
        expr: &TExpr,
        value: MirValueId,
        returned: &Type,
    ) -> Result<MirValueId, LowerError> {
        if !is_plain_call_return(returned, &expr.ty) {
            return Ok(value);
        }
        if self.plain_try_operand == Some(expr as *const TExpr as usize) {
            self.plain_try_operand = None;
            self.plain_try_taken = true;
            return Ok(value);
        }
        self.emit(
            "plain-return.ok",
            Some(expr.ty.clone()),
            MirOperation::ResultOk { value },
        )
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
                .enumerate()
                .map(|(index, (name, ty))| {
                    let convention = lambda
                        .host_param_conventions
                        .as_ref()
                        .and_then(|conventions| conventions.get(index))
                        .copied()
                        .unwrap_or(AccessConvention::Read);
                    (name, ty, convention)
                })
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
            self.reflect_paths,
            self.trait_defs,
            self.function_registry,
            self.source_texts,
            self.modules,
            Some(&lambda.executable),
            Some(lambda),
        )?;
        self.nested_functions.push(nested);
        self.nested_functions.extend(deeper);
        for call in calls {
            self.prelude_call_positions.entry(call.id).or_insert(self.prelude_calls.len());
            self.prelude_calls.push(call);
        }
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
            // `runtime` is the canonical TIR slot used by the lambda body and,
            // for borrowed and moved captures, the exact enclosing slot (which
            // may carry a generated spelling). A cloned/materialized capture's
            // body reads a fresh slot; its value comes from the enclosing slot
            // the TIR recorded, which differs from the Jet name for a resource
            // binding (`conn :: db.open(…) ?? panic(…)`).
            let outer_slot = lambda
                .capture_origins
                .get(source)
                .cloned()
                .unwrap_or_else(|| runtime.clone());
            let outer_access = if cloned { MirAccess::Read } else { access };
            let outer_name = self.resolved_local_name(&TLocal::user(outer_slot));
            let place = self.place_for_local(&TLocal::user(outer_name), outer_access)?;
            if borrowed {
                captures.push(MirCaptureOperand::Place(place));
                continue;
            }
            let value = if cloned {
                let read = self.emit(
                    &format!("closure.capture.{slot}.read"),
                    Some(ty.clone()),
                    MirOperation::ReadPlace(place),
                )?;
                if facts.mutable.contains(source) && self.place_shares_record_handle(place) {
                    // D-TASKBORROW1=A: share the heap record. Copy would drop writes.
                    read
                } else {
                    self.emit_owned(
                        &format!("closure.capture.{slot}.clone"),
                        Some(ty.clone()),
                        MirOperation::Copy {
                            value: read,
                            materialize_view: facts.materialized.contains(source),
                        },
                    )?
                }
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
            call_metadata: lambda.host_param_conventions.as_ref().map(|conventions| {
                crate::AST::FunctionCallMetadata {
                    conventions: conventions.clone(),
                    ..Default::default()
                }
            }),
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
            Some(Type::IntN {
                signed: false,
                bits: 32,
            }),
            MirOperation::Constant(MirConstant::Int {
                value: line as i64,
                width: Some((false, 32)),
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
        ) || matches!(ty, Type::Named(name) if name == crate::Syntax::INTERNAL_UNIT_TYPE || name == crate::Syntax::TYPE_NEVER)
        {
            MirOwnership::copy()
        } else {
            MirOwnership::Owned
        }
    }

    pub(super) fn lower_call_arg(&mut self, arg: &TCallArg) -> Result<MirCallArg, LowerError> {
        // A source-level box fact is redundant when the checked value already
        // has the trait-object type (including a preceding canonical TraitBox).
        // Keep one representation of the coercion so every backend avoids a
        // second allocation/tagging step.
        let source_is_trait_object = matches!(
            arg.value.ty.without_user_tags(),
            Type::TraitObject(_)
        ) || matches!(
            arg.value.ty.without_user_tags(),
            Type::Named(name) if self.is_trait_name(name)
        );
        let box_as_trait = (!source_is_trait_object).then_some(arg.box_as_trait.as_ref()).flatten();
        // A checked concrete-to-trait coercion transfers the concrete value to
        // the box even when the trait parameter's convention is Read. Keep the
        // ordinary borrow path for already-boxed or genuinely borrowed values.
        let consumes_trait_box = box_as_trait.is_some()
            && arg.borrow
            && !arg.mut_borrow
            && !arg.clone
            && !arg.arc_clone;
        let consumes_union = arg.widen_to_union.is_some()
            && !arg.mut_borrow
            && !arg.clone
            && !arg.arc_clone
            && !matches!(
                self.ownership_for(&arg.value.ty).mode,
                jet_foundation::MIR::MirOwnershipMode::Copy
            );
        let access = if arg.mut_borrow {
            MirAccess::Write
        } else if consumes_trait_box || consumes_union {
            MirAccess::Move
        } else if arg.borrow {
            MirAccess::Read
        } else if (arg.clone
            || arg.arc_clone
            || matches!(arg.fact_channel().exclusivity, super::TExclusivity::Move))
            // Copy values cannot be consumed: a literal handed to a `^T`
            // parameter is a copy, the same rule Core-call arguments use.
            && !matches!(
                self.ownership_for(&arg.value.ty).mode,
                jet_foundation::MIR::MirOwnershipMode::Copy
            )
        {
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
                    // A checked mutable argument may be an rvalue (for example, a freshly-created runtime handle). Materialize it in a mutable temporary before taking the call borrow; rejecting it here turns a valid checked program into an ICE.
                    let value = self.lower_child(&arg.value)?;
                    let local = TLocal::generated(format!("call_arg_{}", value.0)).as_mutable();
                    let place =
                        self.bind_local(&local, arg.value.ty.clone(), true, false, false)?;
                    self.emit(
                        "call-arg-temp",
                        None,
                        MirOperation::WritePlace { place, value },
                    )?;
                    Some(place)
                }
                None => None,
            }
        } else {
            None
        };
        let value = if let Some(place) = place {
            let operation = if consumes_trait_box || consumes_union {
                MirOperation::MovePlace { place }
            } else {
                MirOperation::ReadPlace(place)
            };
            // A range write window keeps its owner's list type as a place.
            let place_ty = match &arg.value.kind {
                TExprKind::BuiltinMethod {
                    recv,
                    op: super::TBuiltinOp::ViewMutNew { .. },
                    ..
                } if arg.mut_borrow => recv.ty.clone(),
                _ => arg.value.ty.clone(),
            };
            self.emit("call-place", Some(place_ty), operation)?
        } else {
            self.lower_child(&arg.value)?
        };
        // A raw-payload function value filling a `Result`-carrier callable
        // slot crosses as a fresh adapter closure, not as the source place.
        let adapted = match &arg.fn_coerce {
            Some(coerce) if fn_value_carrier_adaptation(&arg.value.ty, &coerce.ty).is_some() => {
                Some(self.adapt_fn_value_carrier(value, &arg.value.ty, &coerce.ty)?)
            }
            _ => None,
        };
        let value = adapted.unwrap_or(value);
        let call_place = if consumes_trait_box || consumes_union || adapted.is_some() {
            None
        } else {
            place
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
            owned_last_use: matches!(access, MirAccess::Move) || box_as_trait.is_some(),
            authority_boundary: false,
            fn_coercion: None,
            widen_fixed_to_list: false,
            widen_to_union: match &arg.widen_to_union {
                Some(ty) => {
                    let union = self.mir_type(ty)?.identity.ok_or_else(|| {
                        self.error(self.span(), "checked MIR union type has no identity")
                    })?;
                    Some(jet_foundation::MIR::MirUnionCoercion {
                        union,
                        variant: crate::AST::union_member_tag(&arg.value.ty),
                    })
                }
                None => None,
            },
            box_as_trait: match box_as_trait {
                Some(ty) => self.mir_type(ty)?.identity,
                None => None,
            },
            place: call_place,
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

    pub(super) fn lower_pattern(
        &mut self,
        pattern: &TPattern,
    ) -> Result<jet_foundation::MIR::MirPattern, LowerError> {
        super::tir_to_mir_expr::lower_pattern(self, pattern)
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
            deadline: None,
            facts: Default::default(),
        });
        self.emit(
            "scope.enter",
            None,
            MirOperation::ScopeEnter {
                scope: id,
                test_member: None,
            },
        )?;
        self.defer_stack.push(DeferFrame {
            owner: Some(id),
            shadow_mark: self.shadowed_locals.len(),
            inline_exits: matches!(
                kind,
                MirScopeKind::ScopeMember | MirScopeKind::Transaction | MirScopeKind::DebugOnly
            ),
            ..DeferFrame::default()
        });
        Ok(id)
    }
    pub(super) fn set_scope_fact(
        &mut self,
        scope: MirScopeId,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), LowerError> {
        let Some(row) = self
            .scopes
            .iter_mut()
            .find(|candidate| candidate.id == scope)
        else {
            return Err(self.error(
                self.span(),
                format!("missing MIR scope row {scope:?} for scope fact"),
            ));
        };
        row.facts.insert(key.into(), value.into());
        Ok(())
    }
    pub(super) fn set_scope_deadline(
        &mut self,
        scope: MirScopeId,
        value: jet_foundation::MIR::MirValueId,
    ) -> Result<(), LowerError> {
        let Some(row) = self
            .scopes
            .iter_mut()
            .find(|candidate| candidate.id == scope)
        else {
            return Err(self.error(
                self.span(),
                format!("missing MIR scope row {scope:?} for deadline operand"),
            ));
        };
        row.deadline = Some(value);
        Ok(())
    }
    pub(super) fn exit_scope(&mut self, scope: MirScopeId) -> Result<(), LowerError> {
        let Some(frame) = self.defer_stack.last() else {
            return Err(self.error(self.span(), "scope exit has no active lexical frame"));
        };
        if frame.owner != Some(scope) {
            return Err(self.error(
                self.span(),
                format!("scope exit does not match the active scope {scope:?}"),
            ));
        }
        self.seal_exit_chains(self.defer_stack.len() - 1)?;
        if !self.is_terminated() {
            self.emit_deferred_cleanups(self.defer_stack.len() - 1)?;
        }
        if let Some(frame) = self.defer_stack.pop() {
            self.restore_shadowed_locals(frame.shadow_mark);
        }
        if !self.is_terminated() {
            self.emit("scope.exit", None, MirOperation::ScopeExit { scope })?;
        }
        Ok(())
    }

    pub(super) fn push_lexical_frame(&mut self) {
        self.defer_stack.push(DeferFrame {
            shadow_mark: self.shadowed_locals.len(),
            ..DeferFrame::default()
        });
    }

    pub(super) fn pop_lexical_frame(&mut self) -> Result<(), LowerError> {
        if self.defer_stack.len() <= 1 {
            return Err(self.error(self.span(), "lexical defer frame underflow"));
        }
        if self
            .defer_stack
            .last()
            .and_then(|frame| frame.owner)
            .is_some()
        {
            return Err(self.error(
                self.span(),
                "lexical defer frame cannot pop an active MIR scope",
            ));
        }
        self.seal_exit_chains(self.defer_stack.len() - 1)?;
        if !self.is_terminated() {
            self.emit_deferred_cleanups(self.defer_stack.len() - 1)?;
        }
        if let Some(frame) = self.defer_stack.pop() {
            self.restore_shadowed_locals(frame.shadow_mark);
        }
        Ok(())
    }

    pub(super) fn lexical_frame_has_cleanups(&self) -> bool {
        self.defer_stack.last().is_some_and(|frame| !frame.actions.is_empty())
    }

    /// Leave the innermost lexical frame along one CFG edge: emit its cleanups
    /// for this path only; the frame stays active for its sibling paths.
    pub(super) fn jump_leaving_lexical_frame(&mut self, target: MirBlockId) -> Result<(), LowerError> {
        let depth = self.defer_stack.len().saturating_sub(1);
        self.terminate_with_cleanup(MirTerminator::Jump { target }, depth)
    }

    pub(super) fn error(&self, span: Span, message: impl Into<String>) -> LowerError {
        LowerError::new(span, message)
    }

    fn activate_contract_result(&mut self, state: &ContractScopeState) {
        self.local_places
            .insert(state.result.carrier_local.name.clone(), state.carrier_place);
        self.local_types.insert(
            state.result.carrier_local.name.clone(),
            state.result.carrier_ty.clone(),
        );
        self.local_places
            .insert(state.result.binding_local.name.clone(), state.binding_place);
        self.local_types.insert(
            state.result.binding_local.name.clone(),
            state.result.binding_ty.clone(),
        );
    }

    fn bind_contract_result(
        &mut self,
        result: &TContractResult,
        post: &[TContract],
    ) -> Result<ContractScopeState, LowerError> {
        let carrier_place = self.bind_local(
            &result.carrier_local,
            result.carrier_ty.clone(),
            false,
            false,
            false,
        )?;
        self.place_for_local(&result.carrier_local, MirAccess::Move)?;
        let binding_place = if result.carrier_local.name == result.binding_local.name {
            carrier_place
        } else {
            let place = self.bind_local(
                &result.binding_local,
                result.binding_ty.clone(),
                false,
                false,
                false,
            )?;
            self.place_for_local(&result.binding_local, MirAccess::Move)?;
            place
        };
        Ok(ContractScopeState {
            result: result.clone(),
            carrier_place,
            binding_place,
            post: post.to_vec(),
        })
    }

    fn lower_contract_posts(&mut self, state: &ContractScopeState) -> Result<(), LowerError> {
        self.activate_contract_result(state);
        for contract in &state.post {
            self.lower_contract(contract)?;
        }
        Ok(())
    }

    fn contract_is_unit_type(ty: &Type) -> bool {
        match ty {
            Type::Named(name) => name == crate::Syntax::INTERNAL_UNIT_TYPE,
            Type::Tagged { inner, .. } => Self::contract_is_unit_type(inner),
            _ => false,
        }
    }

    fn lower_implicit_contract_return(
        &mut self,
        state: &ContractScopeState,
    ) -> Result<(), LowerError> {
        if !Self::contract_is_unit_type(&state.result.binding_ty) {
            return Err(self.error(self.span(), "checked contract return is missing its value"));
        }
        let binding_value = self.emit(
            "contract.return.unit",
            Some(state.result.binding_ty.clone()),
            MirOperation::Constant(MirConstant::Unit),
        )?;
        self.emit(
            "contract.return.binding.write",
            None,
            MirOperation::WritePlace {
                place: state.binding_place,
                value: binding_value,
            },
        )?;
        if matches!(
            state.result.mode,
            super::TContractResultMode::ResultPayload | super::TContractResultMode::OptionPayload
        ) {
            let carrier_payload = self.emit(
                "contract.return.carrier.unit",
                Some(state.result.binding_ty.clone()),
                MirOperation::Constant(MirConstant::Unit),
            )?;
            let carrier_value = self.emit(
                "contract.return.carrier",
                Some(state.result.carrier_ty.clone()),
                match state.result.mode {
                    super::TContractResultMode::ResultPayload => MirOperation::ResultOk {
                        value: carrier_payload,
                    },
                    super::TContractResultMode::OptionPayload => MirOperation::Present {
                        value: carrier_payload,
                    },
                    super::TContractResultMode::Direct => unreachable!(),
                },
            )?;
            self.emit(
                "contract.return.carrier.write",
                None,
                MirOperation::WritePlace {
                    place: state.carrier_place,
                    value: carrier_value,
                },
            )?;
        }
        self.lower_contract_posts(state)?;
        let returned = self.emit(
            "contract.return.move",
            Some(state.result.carrier_ty.clone()),
            MirOperation::MovePlace {
                place: state.carrier_place,
            },
        )?;
        self.terminate_with_cleanup(
            MirTerminator::Return {
                value: Some(returned),
            },
            0,
        )
    }

    fn lower_contract_return(&mut self, value: Option<MirValueId>) -> Result<(), LowerError> {
        let Some(state) = self.contract_scopes.last().cloned() else {
            return Err(self.error(
                self.span(),
                "contract return lowered without an active scope",
            ));
        };
        if self.is_terminated() {
            return Ok(());
        }
        self.activate_contract_result(&state);
        let Some(value) = value else {
            return self.lower_implicit_contract_return(&state);
        };
        match state.result.mode {
            super::TContractResultMode::Direct => {
                self.emit(
                    "contract.return.binding.write",
                    None,
                    MirOperation::WritePlace {
                        place: state.binding_place,
                        value,
                    },
                )?;
                self.lower_contract_posts(&state)?;
                let returned = self.emit(
                    "contract.return.move",
                    Some(state.result.carrier_ty.clone()),
                    MirOperation::MovePlace {
                        place: state.carrier_place,
                    },
                )?;
                self.terminate_with_cleanup(
                    MirTerminator::Return {
                        value: Some(returned),
                    },
                    0,
                )
            }
            super::TContractResultMode::ResultPayload
            | super::TContractResultMode::OptionPayload => {
                let condition = self.emit(
                    "contract.return.carrier.test",
                    Some(Type::Bool),
                    match state.result.mode {
                        super::TContractResultMode::ResultPayload => {
                            MirOperation::ResultIsOk { subject: value }
                        }
                        super::TContractResultMode::OptionPayload => {
                            MirOperation::OptionIsSome { subject: value }
                        }
                        super::TContractResultMode::Direct => unreachable!(),
                    },
                )?;
                let success = self.new_block(self.span(), "contract.return.success")?;
                let failure = self.new_block(self.span(), "contract.return.failure")?;
                self.terminate(MirTerminator::Branch {
                    condition,
                    then_target: success,
                    else_target: failure,
                });

                self.switch_to(success);
                self.emit(
                    "contract.return.carrier.write",
                    None,
                    MirOperation::WritePlace {
                        place: state.carrier_place,
                        value,
                    },
                )?;
                let carrier_copy = self.emit(
                    "contract.return.carrier.read",
                    Some(state.result.carrier_ty.clone()),
                    MirOperation::ReadPlace(state.carrier_place),
                )?;
                let binding = self.emit(
                    "contract.return.binding.extract",
                    Some(state.result.binding_ty.clone()),
                    match state.result.mode {
                        super::TContractResultMode::ResultPayload => MirOperation::ResultValue {
                            subject: carrier_copy,
                            ok: true,
                        },
                        super::TContractResultMode::OptionPayload => MirOperation::OptionValue {
                            subject: carrier_copy,
                        },
                        super::TContractResultMode::Direct => unreachable!(),
                    },
                )?;
                self.emit(
                    "contract.return.binding.write",
                    None,
                    MirOperation::WritePlace {
                        place: state.binding_place,
                        value: binding,
                    },
                )?;
                self.lower_contract_posts(&state)?;
                let returned = self.emit(
                    "contract.return.move",
                    Some(state.result.carrier_ty.clone()),
                    MirOperation::MovePlace {
                        place: state.carrier_place,
                    },
                )?;
                self.terminate_with_cleanup(
                    MirTerminator::Return {
                        value: Some(returned),
                    },
                    0,
                )?;

                self.switch_to(failure);
                self.emit(
                    "contract.return.carrier.write",
                    None,
                    MirOperation::WritePlace {
                        place: state.carrier_place,
                        value,
                    },
                )?;
                let returned = self.emit(
                    "contract.return.move",
                    Some(state.result.carrier_ty.clone()),
                    MirOperation::MovePlace {
                        place: state.carrier_place,
                    },
                )?;
                self.terminate_with_cleanup(
                    MirTerminator::Return {
                        value: Some(returned),
                    },
                    0,
                )
            }
        }
    }

    pub(super) fn lower_return(&mut self, value: Option<&TExpr>) -> Result<(), LowerError> {
        let value = match value {
            Some(expr) => {
                let plain = match self.function.ret.clone() {
                    Some(expected) if self.contract_scopes.is_empty() => {
                        super::tir_to_mir_expr::lower_plain_return_value(self, expr, &expected)?
                    }
                    _ => None,
                };
                if plain.is_some() {
                    plain
                } else {
                    let value = self.lower_child(expr)?;
                    if self.contract_scopes.is_empty() {
                        if let Some(expected) = self.function.ret.as_ref() {
                            Some(self.trait_box_value(value, &expr.ty, expected)?)
                        } else {
                            Some(value)
                        }
                    } else {
                        Some(value)
                    }
                }
            }
            None => None,
        };
        if self.contract_scopes.is_empty() {
            if !self.is_terminated() {
                self.terminate_with_cleanup(MirTerminator::Return { value }, 0)?;
            }
            return Ok(());
        }
        self.lower_contract_return(value)
    }

    pub(super) fn lower_contract(&mut self, contract: &TContract) -> Result<(), LowerError> {
        let previous_span = self.span();
        self.set_span(contract.span);
        let lowered = (|| match contract.disposition {
            TContractDisposition::Check => {
                let condition = self.lower_child(&contract.condition)?;
                let message = self.lower_child(&contract.message)?;
                let unit = Type::Named(crate::Syntax::INTERNAL_UNIT_TYPE.to_string());
                let carrier = TFailureCarrier::Infallible;
                let call = self.intern_prelude_route(super::require_stop_route(
                    "require", 1, &unit, &carrier,
                )?)?;
                let location = MirPanicLoc {
                    file: self.source_file_id_for(&contract.file),
                    line: contract.line,
                    column: 0,
                };
                let context = self.panic_context_at(contract.line, None);
                self.emit(
                    "contract.require",
                    Some(unit),
                    MirOperation::Semantic(MirSemanticOp::RequireStop {
                        call,
                        kind: MirRequireKind::Require,
                        condition: Some(condition),
                        location,
                        context,
                        values: vec![message],
                        always_stops: false,
                    }),
                )?;
                Ok(())
            }
            TContractDisposition::Proven | TContractDisposition::Stripped => Ok(()),
        })();
        self.set_span(previous_span);
        lowered
    }

    pub(super) fn lower_contract_scope(
        &mut self,
        pre: &[TContract],
        body: &[TStmt],
        post: &[TContract],
        result: &TContractResult,
    ) -> Result<(), LowerError> {
        for contract in pre {
            self.lower_contract(contract)?;
        }
        let state = self.bind_contract_result(result, post)?;
        self.contract_scopes.push(state);
        let lowered = lower_stmts(self, body).and_then(|()| {
            if !self.is_terminated() {
                self.lower_return(None)?;
            }
            Ok(())
        });
        self.contract_scopes.pop();
        if let Some(parent) = self.contract_scopes.last().cloned() {
            self.activate_contract_result(&parent);
        }
        lowered
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
        let type_name = match base.ty.without_user_tags() {
            Type::Named(name) => name,
            _ => {
                return Err(self.error(self.span(), "checked swizzle place has no named lane type"));
            }
        };
        let scalar_ty = crate::Sema::math_scalar_ty(&type_name);
        let carrier = TFailureCarrier::from_checked_type(&scalar_ty);
        let root_place = self
            .places
            .iter()
            .find(|place| place.id == root)
            .cloned()
            .ok_or_else(|| self.error(self.span(), "missing checked swizzle base place"))?;
        let span = self.span();
        let line = self.source_line();
        let location = self.panic_location_at(line);
        lanes
            .iter()
            .map(|lane| {
                let index = self.emit(
                    "math-swizzle-place-index",
                    Some(Type::Int),
                    MirOperation::Constant(MirConstant::Int {
                        value: i64::from(*lane),
                        width: None,
                        spelling: None,
                    }),
                )?;
                let call = self.intern_prelude_route(super::lane_index_route(
                    &type_name, &scalar_ty, &carrier,
                )?)?;
                let write_call = self.intern_prelude_route(super::lane_index_write_route(
                    &type_name, &scalar_ty, &carrier,
                )?)?;
                let id = self.place_id(
                    "lane",
                    &format!("{}:{}:{}", root_place.id.0, span.start, lane),
                )?;
                let mut projections = root_place.projections.clone();
                projections.push(MirProjection::Index {
                    kind: MirIndexKind::Lane,
                    index,
                    call,
                    write_call: Some(write_call),
                    location: location.clone(),
                    context: None,
                    span,
                });
                let ty = self.mir_type(&scalar_ty)?;
                self.places.push(MirPlace {
                    id,
                    span,
                    ty,
                    base: root_place.base.clone(),
                    projections,
                    access,
                    persist_key: None,
                });
                Ok(id)
            })
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
                let place = self.lower_place(&TPlace::Expr(Box::new((**place).clone())), access)?;
                (access, Some(place))
            }
            TExprKind::Clone(_)
            | TExprKind::ExplicitCopy(_)
            | TExprKind::MaterializeView(_)
            | TExprKind::Move(_)
            | TExprKind::ResourceTake(_) => (MirAccess::Move, None),
            _ if crate::Collections::is_iter_type(&arg.ty) => (MirAccess::Move, None),
            _ => (MirAccess::Read, None),
        };
        // Copy values cannot be consumed, even when an explicit copy or clone
        // expression provides an owned argument to a Core provider.
        let access = if matches!(access, MirAccess::Move)
            && matches!(
                self.ownership_for(&arg.ty).mode,
                jet_foundation::MIR::MirOwnershipMode::Copy
            )
        {
            MirAccess::Read
        } else {
            access
        };
        let value = if let Some(place) = place {
            self.emit(
                "core-call-place",
                Some(arg.ty.clone()),
                MirOperation::ReadPlace(place),
            )?
        } else if matches!(access, MirAccess::Move) && crate::Collections::is_iter_type(&arg.ty) {
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
        if let Some(adapted) = self.plain_named_fn_value(name, function, ty, true)? {
            return Ok(adapted);
        }
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
        let mut conventions = signature
            .call_metadata
            .as_ref()
            .map(|metadata| metadata.conventions.clone())
            .unwrap_or_default();
        conventions.resize(signature.params.len(), MirAccess::Read);
        let kind = MirTypeKind::SendFn {
            params: signature.params,
            ret: signature.ret,
            conventions,
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

/// D-MEM-COPYSEM1: `expr` names a plain place: a non-persistent local or an
/// unboxed field path below one.
fn tir_place_path(expr: &TExpr) -> bool {
    match &expr.kind {
        TExprKind::Local(local) => !local.is_persistent(),
        TExprKind::Field {
            recv, boxed: false, ..
        } => tir_place_path(recv),
        _ => false,
    }
}
