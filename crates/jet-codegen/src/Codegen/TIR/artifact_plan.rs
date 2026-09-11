//! Checked artifact/module facts shared by every MIR adapter.
//!
//! This projection deliberately contains semantic rows only.  It does not retain
//! `ProgramBundle`, source text, AST items, generated Rust names, or a second
//! parser.  Adapters resolve the string keys against the function/type rows they
//! already own.

#![allow(dead_code)]
use crate::AST::{
    AccessConvention, CtValue, EverySchedule, Expr, Func, ImportDecl, ImportKind, Item, JobCachePolicy,
    JobScope, JobSkip, OutputKind, Param, ParamZone, ProgramBundle, TestDef, Type,
};
use jet_foundation::Names::{NameAlias, NameDeclaration, NameModule, NameReference, NameVisibility, StructureFact};
use crate::AST::{FfiCloseAdapter, FfiCloseSource, FfiHandleFact, FfiThreadSafety};
use crate::Diagnostics::Span;
use jet_foundation::MIR::{
    MirArtifactBuildMode, MirArtifactRequest,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub(super) use jet_foundation::MIR::{MirArtifactKind as TirArtifactKind, MirArtifactTarget as TirArtifactTarget};


/// The requested execution target is an input to artifact lowering rather than
/// a value rediscovered from emitted source.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TirArtifactEntryKind {
    Library,
    Command,
    App,
    Service,
    Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TirEntryOutput {
    None,
    ReturnValue,
}

/// Target applicability is a checked four-target fact.  The TIR row uses the
/// same shape as MIR so a requested artifact cannot accidentally widen its
/// bridge/link facts to another execution tier.
pub(super) type TirTargetApplicability = jet_foundation::MIR::MirTargetApplicability;

fn target_applicability_for(target: TirArtifactTarget) -> TirTargetApplicability {
    match target {
        TirArtifactTarget::RustAot => TirTargetApplicability {
            rust_aot: true,
            cranelift: false,
            interpreter: false,
            web: false,
        },
        TirArtifactTarget::Cranelift => TirTargetApplicability {
            rust_aot: false,
            cranelift: true,
            interpreter: false,
            web: false,
        },
        TirArtifactTarget::Interpreter => TirTargetApplicability {
            rust_aot: false,
            cranelift: false,
            interpreter: true,
            web: false,
        },
        TirArtifactTarget::Web => TirTargetApplicability {
            rust_aot: false,
            cranelift: false,
            interpreter: false,
            web: true,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TirAccess {
    Read,
    Write,
    Move,
}

fn lower_access(access: AccessConvention) -> TirAccess {
    match access {
        AccessConvention::Read => TirAccess::Read,
        AccessConvention::Write => TirAccess::Write,
        AccessConvention::Move => TirAccess::Move,
    }
}

#[derive(Debug, Clone)]
pub(super) struct TirFunctionRef {
    pub key: String,
    pub module: String,
    pub name: String,
    pub span: Span,
    pub visibility: NameVisibility,
}

#[derive(Debug, Clone)]
pub(super) struct TirFunctionFact {
    pub reference: TirFunctionRef,
    pub params: Vec<TirParamFact>,
    pub return_type: Option<Type>,
    pub is_pure: bool,
}

#[derive(Debug, Clone)]
pub(super) struct TirParamFact {
    pub index: usize,
    pub name: String,
    pub span: Span,
    pub ty: Type,
    pub access: TirAccess,
    pub label: String,
    pub zone: ParamZone,
    pub variadic: bool,
    pub default_present: bool,
}

fn lower_param(param: &Param, index: usize) -> TirParamFact {
    TirParamFact {
        index,
        name: param.name.clone(),
        span: param.name_span,
        ty: param.ty.clone(),
        access: lower_access(param.convention),
        label: param.call_label().to_string(),
        zone: param.zone,
        variadic: param.variadic,
        default_present: param.default.is_some(),
    }
}

fn lower_params(params: &[Param]) -> Vec<TirParamFact> {
    params
        .iter()
        .enumerate()
        .map(|(index, param)| lower_param(param, index))
        .collect()
}

#[derive(Debug, Clone)]
pub(super) struct TirModuleFact {
    pub key: String,
    pub name: String,
    pub path: String,
    pub source_path: String,
    pub imports: Vec<String>,
    pub item_order: Vec<TirItemRef>,
}

#[derive(Debug, Clone)]
pub(super) enum TirItemRef {
    Type(String),
    Trait(String),
    Function(String),
    Constant(String),
    Impl(String),
    Foreign(String),
    Module(String),
    Unknown(String),
}

#[derive(Debug, Clone)]
pub(super) struct TirImportFact {
    pub module: String,
    pub alias: String,
    pub visibility: NameVisibility,
    pub kind: TirImportKind,
    pub span: Span,
    pub version: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) enum TirImportKind {
    File { path: String },
    Module { path: String },
    Unqualified {
        module: String,
        items: Vec<TirImportItem>,
    },
}

#[derive(Debug, Clone)]
pub(super) struct TirImportItem {
    pub original: String,
    pub local: String,
    pub item: TirItemRef,
}

#[derive(Debug, Clone, Default)]
pub(super) struct TirNameFacts {
    pub modules: Vec<NameModule>,
    pub declarations: Vec<NameDeclaration>,
    pub aliases: Vec<NameAlias>,
    pub references: Vec<NameReference>,
    pub structure_facts: Vec<StructureFact>,
}

#[derive(Debug, Clone)]
pub(super) struct TirForeignFact {
    pub key: String,
    pub module: String,
    pub name: String,
    pub span: Span,
    pub symbol: String,
    pub path: String,
    pub params: Vec<TirParamFact>,
    pub return_type: Option<Type>,
    pub abi: String,
    pub language: String,
    pub applicability: TirTargetApplicability,
    pub effect_root: Option<String>,
    pub callback_transport: Option<String>,
    pub callback_plan_digest: Option<String>,
    pub callback_identity: Option<String>,
    pub link_key: Option<String>,
    pub callback_key: Option<String>,
    pub handle_key: Option<String>,
    pub close_function_key: Option<String>,
    pub undo_function_key: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct TirLinkUnit {
    pub key: String,
    pub crate_spec: String,
    pub cache_identity: String,
    pub applicability: TirTargetApplicability,
    pub dependency_dirs: Vec<String>,
    pub link_closure: Vec<String>,
}


#[derive(Debug, Clone)]
pub(super) struct TirCallbackFact {
    pub key: String,
    pub symbol: String,
    pub function: TirFunctionRef,
    pub params: Vec<TirParamFact>,
    pub return_type: Option<Type>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TirHandleOwnership {
    Owned,
}

#[derive(Debug, Clone)]
pub(super) struct TirHandleFact {
    pub key: String,
    pub lib: String,
    pub typedef_name: String,
    pub jet_name: String,
    pub ownership: TirHandleOwnership,
    pub close: String,
    pub close_source: FfiCloseSource,
    pub thread_safety: FfiThreadSafety,
    pub close_function_key: Option<String>,
    pub undo_function_key: Option<String>,
    pub send: bool,
    pub sync: bool,
}

#[derive(Debug, Clone)]
pub(super) struct TirCImportLink {
    pub importing_module: String,
    pub scope: Option<String>,
    pub alias: String,
    pub target_module: String,
}

#[derive(Debug, Clone)]
pub(super) struct TirCLib {
    pub lib: String,
    pub module: String,
}

#[derive(Debug, Clone)]
pub(super) struct TirCOverlayOverride {
    pub lib: String,
    pub generated_symbol: String,
    pub overlay_symbol: String,
}

#[derive(Debug, Clone)]
pub(super) struct TirCloseAdapter {
    pub lib: String,
    pub handle_type: String,
    pub raw_function: String,
    pub adapter_function: String,
}

#[derive(Debug, Clone, Default)]
pub(super) struct TirCffiFacts {
    pub import_links: Vec<TirCImportLink>,
    pub libs: Vec<TirCLib>,
    pub overlay_overrides: Vec<TirCOverlayOverride>,
    pub boundaries: Vec<jet_foundation::AST::FfiBoundaryFacts>,
    pub handle_facts: Vec<TirHandleFact>,
    pub direct_links: Vec<String>,
    pub transitive_links: Vec<String>,
    pub close_adapters: Vec<TirCloseAdapter>,
}

#[derive(Debug, Clone)]
pub(super) enum TirCliDefault {
    TypeDefault,
    Value(CtValue),
    Recorded(String),
}

#[derive(Debug, Clone)]
pub(super) enum TirCliInputShape {
    Flag,
    Value {
        kind: crate::CLISchema::CLIValueKind,
        optional: bool,
        default: Option<TirCliDefault>,
    },
}

#[derive(Debug, Clone)]
pub(super) struct TirCliInput {
    pub field: String,
    pub flag: String,
    pub short: Option<String>,
    pub env: Option<String>,
    pub help: String,
    pub metavar: Option<String>,
    pub shape: TirCliInputShape,
    pub positional: Option<u16>,
    pub variadic: bool,
}

#[derive(Debug, Clone)]
pub(super) struct TirCliCommand {
    pub name: String,
    pub description: Option<String>,
    pub function: Option<TirFunctionRef>,
    pub receiver: Option<String>,
    pub inputs: Vec<TirCliInput>,
}

#[derive(Debug, Clone)]
pub(super) struct TirCliEntry {
    pub record_inputs: bool,
    pub description: Option<String>,
    pub inputs: Vec<TirCliInput>,
    pub commands: Vec<TirCliCommand>,
    pub standard: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct TirEntrySpec {
    pub kind: TirArtifactEntryKind,
    pub function: Option<TirFunctionRef>,
    pub cli: Option<TirCliEntry>,
    pub output: TirEntryOutput,
    pub output_kind: OutputKind,
    pub initialize_environment: bool,
    pub initialize_gc: bool,
    pub serves_until_stopped: bool,
    pub package_version: String,
}

#[derive(Debug, Clone)]
pub(super) struct TirJobArgument {
    pub name: String,
    pub label: String,
    pub ty: String,
    pub required: bool,
    pub default: Option<String>,
    pub variadic: bool,
    pub zone: ParamZone,
}

#[derive(Debug, Clone)]
pub(super) struct TirJobFact {
    pub key: String,
    pub function: Option<TirFunctionRef>,
    pub name: String,
    pub scope: JobScope,
    pub doc: Option<String>,
    pub arguments: Vec<TirJobArgument>,
    pub schedule: Option<EverySchedule>,
    /// D-DX-JOBGRAPH1=A: checked predecessor names and graph bound.
    pub after: Vec<String>,
    pub parallel: usize,
    pub packages: Vec<String>,
    pub working_directory: Option<String>,
    pub input_paths: Vec<String>,
    pub output_paths: Vec<String>,
    pub skip: Option<JobSkip>,
    pub cache: JobCachePolicy,
    pub limits: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TirTestKind {
    Unit,
    Property,
}

#[derive(Debug, Clone)]
pub(super) struct TirTestFact {
    pub key: String,
    pub function: TirFunctionRef,
    pub name: String,
    pub span: Span,
    pub kind: TirTestKind,
    pub parameters: Vec<TirParamFact>,
    pub faults: Vec<String>,
    pub expected_failure: bool,
    /// Compiler-synthesized property row for a callable's `#Pre` contract.
    pub contract_generated: bool,
    /// Sampled contract rows only (#2502): the compiler-synthesized pre-call
    /// eligibility predicate over the candidate's own checked `#Pre`
    /// conditions. The harness evaluates it BEFORE invoking the callable.
    pub eligibility: Option<TirFunctionRef>,
    /// Stable reason when contract input generation cannot be attempted.
    pub generation_unavailable_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct TirOutputCheck {
    pub key: String,
    pub name: String,
    pub function: TirFunctionRef,
}

#[derive(Debug, Clone)]
pub(super) struct TirCoveragePoint {
    pub key: String,
    pub function: TirFunctionRef,
    pub block: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TirHarnessKind {
    Test,
    Fuzz,
    Coverage,
}

#[derive(Debug, Clone)]
pub(super) struct TirHarnessPlan {
    pub key: String,
    pub kind: TirHarnessKind,
    pub tests: Vec<String>,
    pub output_checks: Vec<TirOutputCheck>,
    pub selected_test: Option<String>,
    pub coverage_points: Vec<TirCoveragePoint>,
    pub command_override: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TirExportAbi {
    C,
}

#[derive(Debug, Clone)]
pub(super) struct TirExport {
    pub symbol: String,
    pub function: TirFunctionRef,
    pub abi: TirExportAbi,
}

#[derive(Debug, Clone)]
pub(super) struct TirArtifactPlan {
    pub key: String,
    pub kind: TirArtifactKind,
    pub name: String,
    pub target: TirArtifactTarget,
    pub modules: Vec<String>,
    pub links: Vec<String>,
    pub jobs: Vec<String>,
    pub runtime_parts: BTreeSet<String>,
    pub exports: Vec<TirExport>,
    pub provider_identity: String,
    pub closure_identity: String,
    pub artifact_identity: String,
    pub entry: Option<TirEntrySpec>,
    pub harness: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct TirArtifactFacts {
    pub package_identity: String,
    pub package_version: String,
    pub target: TirArtifactTarget,
    pub build_mode: MirArtifactBuildMode,
    pub build: crate::Facts::BuildFactSnapshot,
    pub names: TirNameFacts,
    pub functions: Vec<TirFunctionFact>,
    pub modules: Vec<TirModuleFact>,
    pub imports: Vec<TirImportFact>,
    pub foreign: Vec<TirForeignFact>,
    pub links: Vec<TirLinkUnit>,
    pub callbacks: Vec<TirCallbackFact>,
    pub handles: Vec<TirHandleFact>,
    pub entry: Option<TirEntrySpec>,
    pub cli: Option<TirCliEntry>,
    pub jobs: Vec<TirJobFact>,
    pub tests: Vec<TirTestFact>,
    pub output_checks: Vec<TirOutputCheck>,
    pub coverage_points: Vec<TirCoveragePoint>,
    pub harnesses: Vec<TirHarnessPlan>,
    pub artifacts: Vec<TirArtifactPlan>,
    pub cffi: TirCffiFacts,
}

pub(super) fn module_identity(bundle: &ProgramBundle, index: usize) -> String {
    crate::Codegen::Context::module_identity(bundle, index)
}

fn key(module: &str, name: &str) -> String {
    if module.is_empty() {
        name.to_string()
    } else if name.is_empty() {
        module.to_string()
    } else {
        format!("{module}::{name}")
    }
}

fn function_ref(
    module: &str,
    name: &str,
    span: Span,
    visibility: NameVisibility,
) -> TirFunctionRef {
    TirFunctionRef {
        key: key(module, name),
        module: module.to_string(),
        name: name.to_string(),
        span,
        visibility,
    }
}

fn function_visibility(function: &Func) -> NameVisibility {
    NameVisibility::from_flags(function.is_pub, function.is_package_pub)
}

fn is_operator_trait(trait_name: &str) -> bool {
    matches!(
        trait_name,
        crate::Syntax::TRAIT_ADD
            | crate::Syntax::TRAIT_SUB
            | crate::Syntax::TRAIT_MUL
            | crate::Syntax::TRAIT_DIV
            | crate::Syntax::TRAIT_EQUATABLE
            | crate::Syntax::TRAIT_COMPARABLE
    )
}

fn method_reference_name(
    owner: &str,
    trait_name: Option<&str>,
    operator_rhs: Option<&Type>,
    method: &Func,
) -> String {
    match trait_name {
        Some(trait_name) if is_operator_trait(trait_name) => {
            let rhs = operator_rhs.expect("sema must normalize every operator RHS");
            crate::Traits::operator_method_identity(owner, trait_name, &method.name, rhs)
        }
        Some(trait_name) => format!("{owner}::{trait_name}::{}", method.name),
        None => format!("{owner}::{}", method.name),
    }
}

fn item_ref(module: &str, item: &Item) -> Option<TirItemRef> {
    let reference = match item {
        Item::Func(function) => TirItemRef::Function(key(module, &function.name)),
        Item::Struct(definition) => TirItemRef::Type(key(module, &definition.name)),
        Item::Enum(definition) => TirItemRef::Type(key(module, &definition.name)),
        Item::Distinct(definition) => TirItemRef::Type(key(module, &definition.name)),
        Item::TypeAlias(definition) => TirItemRef::Type(key(module, &definition.name)),
        Item::UnitFamily(definition) => TirItemRef::Type(key(module, &definition.family)),
        Item::Trait(definition) => TirItemRef::Trait(key(module, &definition.name)),
        Item::Impl(definition) => {
            let local = match definition.trait_name.as_deref() {
                Some(trait_name) if is_operator_trait(trait_name) => {
                    let rhs = definition
                        .operator_rhs
                        .as_ref()
                        .expect("sema must normalize every operator RHS");
                    format!(
                        "impl::{}::{trait_name}<{}>",
                        definition.type_name,
                        rhs.name()
                    )
                }
                Some(trait_name) => format!("impl::{}::{trait_name}", definition.type_name),
                None => format!("impl::{}", definition.type_name),
            };
            TirItemRef::Impl(key(module, &local))
        }
        Item::Const(definition) => TirItemRef::Constant(key(module, &definition.name)),
        Item::Test(definition) => TirItemRef::Unknown(test_key(module, definition)),
        Item::ExternRust(_) => TirItemRef::Foreign(key(module, "extern")),
        Item::CodeModule(definition) if definition.body.is_some() => {
            TirItemRef::Module(key(module, &definition.name))
        }
        // A file module has its own loaded-module identity and is represented
        // by the synthetic File import, not as an inline child of this module.
        Item::CodeModule(_) => return None,
        _ => TirItemRef::Unknown(std::any::type_name::<Item>().to_string()),
    };
    Some(reference)
}

pub(super) fn test_key(module: &str, test: &TestDef) -> String {
    let name = test.name.as_deref().unwrap_or("anonymous");
    key(module, &format!("test::{name}@{}..{}", test.span.start, test.span.end))
}
pub(super) fn contract_key(module: &str, function: &Func) -> String {
    key(
        module,
        &format!(
            "contract::{}@{}..{}",
            function.name, function.span.start, function.span.end
        ),
    )
}

pub(super) fn contract_eligibility_key(module: &str, function: &Func) -> String {
    key(
        module,
        &format!(
            "contract-pre::{}@{}..{}",
            function.name, function.span.start, function.span.end
        ),
    )
}

/// The sampling plan the TIR item walk records for every contract candidate:
/// contract key → `None` (sampled: predicate + wrapper were materialized) or
/// `Some(reason)` (explicit unavailable record; only the trivial unavailable
/// wrapper was materialized).
pub(super) type ContractSamplingPlan = BTreeMap<String, Option<String>>;

/// One canonical eligibility fact for generated contract sampling (#2502).
///
/// Both artifact discovery and TIR item materialization consult this same
/// classification, so every emitted test row resolves to a lowered callable
/// and every non-sampled candidate is an explicit unavailable record — never
/// an unresolved synthetic key and never silently omitted. `covered` is the
/// TIR coverage fact for the candidate in its own module context.
///
/// Returns `None` for a non-candidate (no `#Pre` clauses or no parameters),
/// `Some(None)` for a sampled candidate, and `Some(Some(reason))` for an
/// explicit unavailable record.
pub(super) fn contract_sampling_reason(function: &Func, covered: bool) -> Option<Option<String>> {
    if function.pre.is_empty() || function.params.is_empty() {
        return None;
    }
    let precondition_calls_function = function.pre.iter().any(|clause| {
        let mut found = false;
        clause.cond.for_each_expr(|expr| {
            if matches!(expr, Expr::Call(_) | Expr::MethodCall { .. }) {
                found = true;
            }
        });
        found
    });
    let reason = if function.inline_foreign.is_some() {
        Some("contract_target_foreign")
    } else if !function.type_params.is_empty() {
        Some("contract_target_generic")
    } else if !covered {
        Some("contract_target_unsupported")
    } else if precondition_calls_function {
        Some("precondition_calls_function")
    } else {
        None
    };
    Some(reason.map(str::to_string))
}

fn lower_contract_test(
    function: &Func,
    module: &str,
    contract_rows: &ContractSamplingPlan,
) -> Option<TirTestFact> {
    if function.pre.is_empty() || function.params.is_empty() {
        return None;
    }
    let key = contract_key(module, function);
    // Only candidates the TIR item walk materialized get a row; the walk and
    // this discovery share `contract_sampling_reason`, so a missing entry
    // means the callable is outside the compiled program.
    let reason = contract_rows.get(&key)?.clone();
    let function_ref = TirFunctionRef {
        key: key.clone(),
        module: module.to_string(),
        name: key.clone(),
        span: function.span,
        visibility: NameVisibility::Private,
    };
    let sampled = reason.is_none();
    Some(TirTestFact {
        key: key.clone(),
        function: function_ref,
        name: format!("contract::{}", function.name),
        span: function.span,
        kind: TirTestKind::Property,
        parameters: if sampled {
            lower_params(&function.params)
        } else {
            Vec::new()
        },
        faults: Vec::new(),
        expected_failure: false,
        contract_generated: true,
        eligibility: sampled.then(|| TirFunctionRef {
            key: contract_eligibility_key(module, function),
            module: module.to_string(),
            name: contract_eligibility_key(module, function),
            span: function.span,
            visibility: NameVisibility::Private,
        }),
        generation_unavailable_reason: reason,
    })
}

/// Return the checked Core namespace represented by a parser-level
/// unqualified import. `use core.mem` is parsed as `module_alias = "core"`
/// with `mem` as its only member, while Core has no source-module row in the
/// checked bundle. Keep that edge as a path import instead of manufacturing a
/// MIR module ID for the parser prefix.
fn core_import_module_path(
    module_alias: &str,
    items: &[(String, Option<String>)],
) -> Option<String> {
    crate::AST::core_list_prefix(module_alias)?;

    let mut target = None;
    for (original, _) in items {
        let module = match crate::AST::core_list_path(module_alias, original)? {
            crate::AST::CoreListPath::Module(module)
            | crate::AST::CoreListPath::Item { module, .. } => module,
        };
        if let Some(existing) = &target {
            if existing != &module {
                return Some(module_alias.to_string());
            }
        } else {
            target = Some(module);
        }
    }
    target.or_else(|| Some(module_alias.to_string()))
}

fn lower_import(
    bundle: &ProgramBundle,
    module_index: usize,
    module: &str,
    import: &ImportDecl,
) -> TirImportFact {
    let kind = match &import.kind {
        ImportKind::File(path, _) => TirImportKind::File { path: path.clone() },
        ImportKind::Module(path, _) => TirImportKind::Module { path: path.clone() },
        ImportKind::Unqualified {
            module_alias,
            items,
            ..
        } => {
            if let Some(path) = core_import_module_path(module_alias, items) {
                TirImportKind::Module { path }
            } else {
                let target_module = bundle
                    .name_ledger
                    .import_target(module_index, import.span)
                    .map(|target| module_identity(bundle, target))
                    .unwrap_or_else(|| module_alias.clone());
                let imported = items
                    .iter()
                    .map(|(original, local)| {
                        let local_name = local.as_deref().unwrap_or(original);
                        let resolved = bundle
                            .name_ledger
                            .canonical_path(module_index, local_name)
                            .or_else(|| Some(key(&target_module, original)))
                            .unwrap_or_else(|| original.clone());
                        TirImportItem {
                            original: original.clone(),
                            local: local_name.to_string(),
                            item: TirItemRef::Unknown(resolved),
                        }
                    })
                    .collect();
                TirImportKind::Unqualified {
                    module: target_module,
                    items: imported,
                }
            }
        }
    };
    TirImportFact {
        module: module.to_string(),
        alias: import.alias.clone(),
        visibility: NameVisibility::from_flags(import.is_pub, import.is_package_pub),
        kind,
        span: import.span,
        version: import.inline_version.as_ref().map(|version| version.text.clone()),
    }
}

fn collect_imports(
    bundle: &ProgramBundle,
    module_index: usize,
    module: &str,
    imports: &mut Vec<TirImportFact>,
) {
    let loaded = &bundle.modules[module_index];
    imports.extend(
        loaded
            .imports
            .iter()
            .map(|import| lower_import(bundle, module_index, module, import)),
    );
    fn nested(
        bundle: &ProgramBundle,
        module_index: usize,
        module: &str,
        items: &[Item],
        imports: &mut Vec<TirImportFact>,
    ) {
        for item in items {
            let Item::CodeModule(code_module) = item else { continue };
            let child = key(module, &code_module.name);
            imports.extend(
                code_module
                    .imports
                    .iter()
                    .map(|import| lower_import(bundle, module_index, &child, import)),
            );
            if let Some(body) = &code_module.body {
                nested(bundle, module_index, &child, body, imports);
            }
        }
    }
    nested(bundle, module_index, module, &loaded.items, imports);
}

fn lower_cli_default(default: &crate::CLISchema::CLIDefault) -> TirCliDefault {
    match default {
        crate::CLISchema::CLIDefault::TypeDefault => TirCliDefault::TypeDefault,
        crate::CLISchema::CLIDefault::Value(value) => TirCliDefault::Value(value.clone()),
        crate::CLISchema::CLIDefault::Recorded(value) => TirCliDefault::Recorded(value.clone()),
    }
}

pub(super) fn lower_cli_input(input: &crate::CLISchema::CLIInputSchema) -> TirCliInput {
    let shape = match &input.shape {
        crate::CLISchema::CLIInputShape::Flag => TirCliInputShape::Flag,
        crate::CLISchema::CLIInputShape::Value {
            kind,
            optional,
            default,
        } => TirCliInputShape::Value {
            kind: *kind,
            optional: *optional,
            default: default.as_ref().map(lower_cli_default),
        },
    };
    TirCliInput {
        field: input.field.clone(),
        flag: input.flag.clone(),
        short: input.short.clone(),
        env: input.env.clone(),
        help: input.help.clone(),
        metavar: input.metavar.clone(),
        shape,
        positional: input.positional,
        variadic: false,
    }
}

fn find_entry_struct<'a>(items: &'a [Item], name: &str) -> Option<&'a crate::AST::StructDef> {
    let leaf = name.rsplit("::").next().unwrap_or(name);
    items.iter().find_map(|item| match item {
        Item::Struct(definition) if definition.name == leaf => Some(definition),
        _ => None,
    })
}

fn lower_cli_entry(
    bundle: &ProgramBundle,
    schema: &crate::CLISchema::CLICommandSchema,
    module: &str,
) -> Option<TirCliEntry> {
    if schema.entry_type.is_empty()
        && schema.inputs.is_empty()
        && schema.commands.is_empty()
        && !schema.standard
    {
        return None;
    }
    let inputs = schema.inputs.iter().map(lower_cli_input).collect();
    let root = bundle.modules.get(bundle.entry)?;
    let structure = find_entry_struct(&root.items, &schema.entry_type);
    let commands = schema
        .commands
        .iter()
        .map(|command| {
            let target = structure.and_then(|structure| {
                crate::CLISchema::command_target(
                    structure,
                    command,
                    &root.items,
                    &schema.entry_type,
                )
            });
            let function = target.as_ref().map(|target| {
                let method = target.is_method;
                let function_module = module;
                let function_name = if method {
                    format!("{}::{}", schema.entry_type, target.function.name)
                } else {
                    target.function.name.clone()
                };
                function_ref(
                    function_module,
                    &function_name,
                    target.function.span,
                    function_visibility(&target.function),
                )
            });
            let receiver = target
                .as_ref()
                .filter(|target| target.bound_shared)
                .map(|_| key(module, &schema.entry_type));
            TirCliCommand {
                name: command.name.clone(),
                description: command.description.clone(),
                function,
                receiver,
                inputs: command.inputs.iter().map(lower_cli_input).collect(),
            }
        })
        .collect();
    Some(TirCliEntry {
        record_inputs: !schema.entry_type.is_empty(),
        description: schema.description.clone(),
        inputs,
        commands,
        standard: schema.standard,
        version: schema.version.clone(),
    })
}

fn output_entry(bundle: &ProgramBundle) -> Option<crate::AST::ResolvedOutput> {
    bundle.modules.iter().flat_map(|module| module.items.iter()).find_map(|item| {
        let Item::Const(constant) = item else { return None };
        let output = constant.resolved_output.as_ref()?;
        output.selected.then(|| output.clone())
    })
}

fn entry_kind(kind: OutputKind) -> TirArtifactEntryKind {
    match kind {
        OutputKind::Library => TirArtifactEntryKind::Library,
        OutputKind::Executable => TirArtifactEntryKind::Command,
        OutputKind::Service => TirArtifactEntryKind::Service,
        OutputKind::Check => TirArtifactEntryKind::Test,
        OutputKind::Environment
        | OutputKind::Image
        | OutputKind::Bundle
        | OutputKind::System
        | OutputKind::Fleet => TirArtifactEntryKind::App,
    }
}

fn entry_output(kind: OutputKind) -> TirEntryOutput {
    match kind {
        OutputKind::Executable | OutputKind::Service | OutputKind::Check => TirEntryOutput::ReturnValue,
        OutputKind::Library
        | OutputKind::Environment
        | OutputKind::Image
        | OutputKind::Bundle
        | OutputKind::System
        | OutputKind::Fleet => TirEntryOutput::None,
    }
}

fn no_os_profile(bundle: &ProgramBundle) -> bool {
    bundle
        .build_facts
        .target_dossier
        .machine
        .as_deref()
        .is_some_and(|machine| machine.no_os)
}

fn initialize_environment(bundle: &ProgramBundle, target: TirArtifactTarget) -> bool {
    matches!(target, TirArtifactTarget::RustAot | TirArtifactTarget::Cranelift)
        && !no_os_profile(bundle)
}

fn lower_entry(
    bundle: &ProgramBundle,
    target: TirArtifactTarget,
    cli: Option<TirCliEntry>,
) -> Option<TirEntrySpec> {
    if let Some(output) = output_entry(bundle) {
        let module = module_identity(bundle, output.module);
        let function = Some(function_ref(
            &module,
            &output.semantic_name,
            output.definition,
            NameVisibility::Public,
        ));
        return Some(TirEntrySpec {
            kind: entry_kind(output.kind),
            function,
            cli,
            output: entry_output(output.kind),
            output_kind: output.kind,
            initialize_environment: initialize_environment(bundle, target),
            initialize_gc: false,
            serves_until_stopped: matches!(output.kind, OutputKind::Service)
                || crate::AST::bundle_serves_until_stopped(bundle),
            package_version: bundle.build_facts.package_version.clone(),
        });
    }
    // Sema has checked `run`; retain typed entries with their canonical CLI
    // decoder just as we retain parameterless entries.
    let function = bundle.modules.get(bundle.entry)?.items.iter().find_map(|item| match item {
        Item::Func(function)
            if function.name == "run" && (function.params.is_empty() || cli.is_some()) => {
            Some(function)
        }
        _ => None,
    })?;
    let module = module_identity(bundle, bundle.entry);
    Some(TirEntrySpec {
        kind: TirArtifactEntryKind::Command,
        function: Some(function_ref(
            &module,
            &function.name,
            function.span,
            function_visibility(function),
        )),
        cli,
        output: TirEntryOutput::ReturnValue,
        output_kind: OutputKind::Executable,
        initialize_environment: initialize_environment(bundle, target),
        initialize_gc: false,
        serves_until_stopped: crate::AST::bundle_serves_until_stopped(bundle),
        package_version: bundle.build_facts.package_version.clone(),
    })
}

fn lower_job(function: &Func, module: &str) -> Option<TirJobFact> {
    let metadata = function.job_metadata.as_ref()?;
    let job = crate::CLISchema::JobFact::from_function(function);
    Some(TirJobFact {
        key: key(module, &format!("job::{}", function.name)),
        function: Some(function_ref(
            module,
            &function.name,
            function.span,
            function_visibility(function),
        )),
        name: job.name,
        scope: metadata.scope,
        doc: job.doc,
        arguments: job
            .arguments
            .into_iter()
            .map(|argument| TirJobArgument {
                name: argument.name,
                label: argument.label,
                ty: argument.ty,
                required: argument.required,
                default: argument.default,
                variadic: argument.variadic,
                zone: argument.zone,
            })
            .collect(),
        schedule: function.every.as_ref().and_then(|every| every.resolved),
        after: metadata.after.clone(),
        parallel: metadata.parallel.unwrap_or(1),
        packages: metadata.packages.clone(),
        working_directory: metadata.cwd.clone(),
        input_paths: metadata.inputs.clone(),
        output_paths: metadata.outputs.clone(),
        skip: metadata.skip.clone(),
        cache: metadata.cache,
        limits: metadata.limits.clone(),
    })
}

fn lower_test(test: &TestDef, module: &str) -> TirTestFact {
    let key = test_key(module, test);
    let function = TirFunctionRef {
        key: key.clone(),
        module: module.to_string(),
        name: key.clone(),
        span: test.span,
        visibility: NameVisibility::Private,
    };
    TirTestFact {
        key: key.clone(),
        function,
        name: test.name.clone().unwrap_or_else(|| key.clone()),
        span: test.span,
        kind: if test.params.is_empty() {
            TirTestKind::Unit
        } else {
            TirTestKind::Property
        },
        parameters: lower_params(&test.params),
        faults: test.faults.clone(),
        expected_failure: test.expected_fail,
        contract_generated: false,
        eligibility: None,
        generation_unavailable_reason: None,
    }
}

fn lower_foreign(
    function: &crate::AST::ExternFn,
    module: &str,
    crate_spec: &str,
    target: TirArtifactTarget,
) -> TirForeignFact {
    let function_key = key(module, &function.rust_path);
    TirForeignFact {
        key: function_key.clone(),
        module: module.to_string(),
        name: function.name.clone(),
        span: function.span,
        symbol: function_key,
        path: function.rust_path.clone(),
        params: lower_params(&function.params),
        return_type: function.return_type.clone(),
        abi: function
            .abi
            .as_ref()
            .map_or_else(|| "C".to_string(), |(abi, _)| abi.clone()),
        language: format!("rust:{crate_spec}"),
        applicability: target_applicability_for(target),
        effect_root: function.effect_root.clone(),
        callback_transport: None,
        callback_plan_digest: None,
        callback_identity: None,
        link_key: Some(key(module, crate_spec)),
        callback_key: None,
        handle_key: None,
        close_function_key: function
            .close
            .as_ref()
            .map(|(name, _)| key(module, name)),
        undo_function_key: function
            .undo
            .as_ref()
            .map(|(name, _)| key(module, name)),
    }
}

fn lower_c_foreign(
    function: &crate::AST::ExternFn,
    module: &str,
    lib: &str,
    handles: &[FfiHandleFact],
    target: TirArtifactTarget,
) -> TirForeignFact {
    let function_key = key(module, &function.name);
    let handle_key = function
        .return_type
        .as_ref()
        .and_then(|ty| match ty {
            Type::Named(name)
                if handles
                    .iter()
                    .any(|handle| handle.lib == lib && handle.jet_name == *name) =>
            {
                Some(format!("c::{lib}::{name}"))
            }
            _ => None,
        })
        .or_else(|| {
            function.params.iter().find_map(|param| match &param.ty {
                Type::Named(name)
                    if handles
                        .iter()
                        .any(|handle| handle.lib == lib && handle.jet_name == *name) =>
                {
                    Some(format!("c::{lib}::{name}"))
                }
                _ => None,
            })
        });
    TirForeignFact {
        key: function_key,
        module: module.to_string(),
        name: function.name.clone(),
        span: function.span,
        symbol: function.rust_path.clone(),
        path: function.rust_path.clone(),
        params: lower_params(&function.params),
        return_type: function.return_type.clone(),
        abi: function
            .abi
            .as_ref()
            .map_or_else(|| "C".to_string(), |(abi, _)| abi.clone()),
        language: "c".to_string(),
        applicability: target_applicability_for(target),
        effect_root: function.effect_root.clone(),
        callback_transport: function.callback_transport.clone(),
        callback_plan_digest: function.callback_plan_digest.clone(),
        callback_identity: function.callback_identity.clone(),
        link_key: Some(format!("c::{lib}")),
        callback_key: None,
        handle_key,
        close_function_key: function
            .close
            .as_ref()
            .map(|(name, _)| key(module, name)),
        undo_function_key: function
            .undo
            .as_ref()
            .map(|(name, _)| key(module, name)),
    }
}

fn push_function(facts: &mut TirArtifactFacts, module: &str, function: &Func, name: String) {
    facts.functions.push(TirFunctionFact {
        reference: function_ref(module, &name, function.span, function_visibility(function)),
        params: lower_params(&function.params),
        return_type: function.return_type.clone(),
        is_pure: function.is_pure,
    });
    if let Some(job) = lower_job(function, module) {
        facts.jobs.push(job);
    }
}

fn collect_items(
    bundle: &ProgramBundle,
    module_index: usize,
    module: &str,
    items: &[Item],
    facts: &mut TirArtifactFacts,
    target: TirArtifactTarget,
    contract_rows: &ContractSamplingPlan,
) {
    for item in items {
        let reference = item_ref(module, item);
        match item {
            Item::Func(function) => {
                push_function(facts, module, function, function.name.clone());
                if let Some(test) = lower_contract_test(function, module, contract_rows) {
                    facts.tests.push(test);
                }
            }
            Item::Struct(definition) => {
                for function in &definition.methods {
                    push_function(
                        facts,
                        module,
                        function,
                        format!("{}::{}", definition.name, function.name),
                    );
                }
                for implementation in &definition.trait_impls {
                    for function in &implementation.methods {
                        push_function(
                            facts,
                            module,
                            function,
                            method_reference_name(
                                &definition.name,
                                Some(implementation.trait_name.as_str()),
                                implementation.operator_rhs.as_ref(),
                                function,
                            ),
                        );
                    }
                }
            }
            Item::Enum(definition) => {
                for function in &definition.methods {
                    push_function(
                        facts,
                        module,
                        function,
                        format!("{}::{}", definition.name, function.name),
                    );
                }
                for implementation in &definition.trait_impls {
                    for function in &implementation.methods {
                        push_function(
                            facts,
                            module,
                            function,
                            method_reference_name(
                                &definition.name,
                                Some(implementation.trait_name.as_str()),
                                implementation.operator_rhs.as_ref(),
                                function,
                            ),
                        );
                    }
                }
            }
            Item::Impl(implementation) => {
                for function in &implementation.methods {
                    let name = method_reference_name(
                        &implementation.type_name,
                        implementation.trait_name.as_deref(),
                        implementation.operator_rhs.as_ref(),
                        function,
                    );
                    push_function(facts, module, function, name);
                }
            }
            Item::Test(test) => facts.tests.push(lower_test(test, module)),
            Item::ExternRust(block) => {
                for function in &block.functions {
                    let row = lower_foreign(function, module, &block.crate_spec, target);
                    facts.foreign.push(row);
                }
            }
            Item::CModule(c_module) => {
                for function in c_module.functions.iter().filter(|function| {
                    function.hidden_c_bridge_compatible_with_handles(
                        &bundle.cffi.handle_facts,
                    )
                }) {
                    facts.foreign.push(lower_c_foreign(
                        function,
                        module,
                        &c_module.lib,
                        &bundle.cffi.handle_facts,
                        target,
                    ));
                }
            }
            Item::CodeModule(code_module) => {
                if let Some(body) = &code_module.body {
                    let child = key(module, &code_module.name);
                    facts.modules.push(TirModuleFact {
                        key: child.clone(),
                        name: code_module.name.clone(),
                        path: child.clone(),
                        source_path: bundle.modules[module_index].display.clone(),
                        imports: code_module
                            .imports
                            .iter()
                            .map(|import| key(&child, &import.alias))
                            .collect(),
                        item_order: body
                            .iter()
                            .filter_map(|item| item_ref(&child, item))
                            .collect(),
                    });
                    collect_items(bundle, module_index, &child, body, facts, target, contract_rows);
                }
            }
            _ => {}
        }
        if let Some(reference) = reference {
            if let TirItemRef::Unknown(_) = &reference {
                // Unknown item kinds are still represented in module order; the
                // semantic row carries the checked spelling rather than source text.
            }
            if let Some(module_row) = facts.modules.iter_mut().find(|row| row.key == module) {
                module_row.item_order.push(reference);
            }
        }
    }
}

fn lower_names(bundle: &ProgramBundle) -> TirNameFacts {
    let mut facts = TirNameFacts {
        modules: bundle
            .modules
            .iter()
            .enumerate()
            .filter_map(|(index, _)| bundle.name_ledger.module(index).cloned())
            .collect(),
        declarations: bundle.name_ledger.declarations().cloned().collect(),
        aliases: bundle.name_ledger.aliases().cloned().collect(),
        references: bundle.name_ledger.references().values().cloned().collect(),
        structure_facts: bundle.name_ledger.structure_facts().to_vec(),
    };
    facts
        .declarations
        .sort_by_key(|row| (row.module, row.name.clone(), row.path.clone()));
    facts
        .aliases
        .sort_by_key(|row| (row.module, row.name.clone(), row.target.clone()));
    facts.references.sort_by_key(|row| {
        (
            row.module_path.clone(),
            row.kind.clone(),
            row.def_span.start,
            row.def_span.end,
        )
    });
    facts.structure_facts.sort_by_key(|row| {
        (
            row.subject.clone(),
            row.source.clone(),
            row.span.start,
            row.span.end,
        )
    });
    facts
}

fn lower_cffi(bundle: &ProgramBundle) -> TirCffiFacts {
    TirCffiFacts {
        import_links: bundle
            .cffi
            .import_links
            .iter()
            .map(|link| TirCImportLink {
                importing_module: module_identity(bundle, link.importing_idx),
                scope: link.scope.clone(),
                alias: link.alias.clone(),
                target_module: module_identity(bundle, link.target_idx),
            })
            .collect(),
        libs: bundle
            .cffi
            .libs
            .iter()
            .map(|lib| TirCLib {
                lib: lib.lib.clone(),
                module: module_identity(bundle, lib.module_idx),
            })
            .collect(),
        overlay_overrides: bundle
            .cffi
            .overlay_overrides
            .iter()
            .map(|override_row| TirCOverlayOverride {
                lib: override_row.lib.clone(),
                generated_symbol: override_row.generated_symbol.clone(),
                overlay_symbol: override_row.overlay_symbol.clone(),
            })
            .collect(),
        boundaries: bundle.cffi.boundaries.clone(),
        handle_facts: bundle
            .cffi
            .handle_facts
            .iter()
            .map(lower_handle)
            .collect(),
        direct_links: bundle.cffi.link_closure.direct.clone(),
        transitive_links: bundle.cffi.link_closure.transitive.clone(),
        close_adapters: bundle
            .cffi
            .close_adapters
            .iter()
            .map(lower_close_adapter)
            .collect(),
    }
}

fn lower_handle(fact: &FfiHandleFact) -> TirHandleFact {
    let key = format!("c::{}::{}", fact.lib, fact.jet_name);
    TirHandleFact {
        key,
        lib: fact.lib.clone(),
        typedef_name: fact.typedef_name.clone(),
        jet_name: fact.jet_name.clone(),
        ownership: TirHandleOwnership::Owned,
        close: fact.close.clone(),
        close_source: fact.close_source,
        thread_safety: fact.thread_safety,
        close_function_key: Some(format!("c::{}::{}", fact.lib, fact.close)),
        undo_function_key: None,
        send: matches!(fact.thread_safety, FfiThreadSafety::Safe),
        sync: matches!(fact.thread_safety, FfiThreadSafety::Safe),
    }
}

fn lower_close_adapter(adapter: &FfiCloseAdapter) -> TirCloseAdapter {
    TirCloseAdapter {
        lib: adapter.lib.clone(),
        handle_type: adapter.handle_type.clone(),
        raw_function: adapter.raw_function.clone(),
        adapter_function: adapter.adapter_function.clone(),
    }
}

fn lower_links(bundle: &ProgramBundle, target: TirArtifactTarget) -> Vec<TirLinkUnit> {
    let cache_identity = bundle.cffi.link_closure.stable_key();
    let applicability = target_applicability_for(target);
    let mut links = bundle
        .cffi
        .libs
        .iter()
        .map(|lib| TirLinkUnit {
            key: format!("c::{}", lib.lib),
            crate_spec: lib.lib.clone(),
            cache_identity: cache_identity.clone(),
            applicability,
            dependency_dirs: Vec::new(),
            link_closure: bundle.cffi.link_closure.libraries().map(str::to_string).collect(),
        })
        .collect::<Vec<_>>();
    if links.is_empty() && !cache_identity.is_empty() {
        links.push(TirLinkUnit {
            key: "c::closure".to_string(),
            crate_spec: "c".to_string(),
            cache_identity,
            applicability,
            dependency_dirs: Vec::new(),
            link_closure: bundle.cffi.link_closure.libraries().map(str::to_string).collect(),
        });
    }
    links
}

fn lower_callbacks(bundle: &ProgramBundle, facts: &TirArtifactFacts) -> Vec<TirCallbackFact> {
    let mut functions = HashMap::new();
    for function in &facts.functions {
        functions.insert(function.reference.name.clone(), function);
        functions.insert(function.reference.key.clone(), function);
    }
    let mut names = bundle.ffi_callback_fns.iter().cloned().collect::<Vec<_>>();
    names.sort();
    names.into_iter()
        .filter_map(|name| {
            let function = functions.get(&name).or_else(|| {
                name.rsplit_once("::")
                    .and_then(|(_, leaf)| functions.get(leaf))
            })?;
            Some(TirCallbackFact {
                key: format!("{}::callback", function.reference.key),
                symbol: function.reference.key.clone(),
                function: function.reference.clone(),
                params: function.params.clone(),
                return_type: function.return_type.clone(),
            })
        })
        .collect()
}

fn lower_cli_and_entry(
    bundle: &ProgramBundle,
    target: TirArtifactTarget,
) -> (Option<TirCliEntry>, Option<TirEntrySpec>) {
    let schema = crate::CLISchema::executable_schema(bundle);
    let module = module_identity(bundle, bundle.entry);
    let cli = lower_cli_entry(bundle, &schema, &module);
    let entry = lower_entry(bundle, target, cli.clone());
    (cli, entry)
}
fn lower_output_checks(bundle: &ProgramBundle, facts: &TirArtifactFacts) -> Vec<TirOutputCheck> {
    let mut rows = Vec::new();
    for module in &bundle.modules {
        let module_index = bundle
            .modules
            .iter()
            .position(|candidate| std::ptr::eq(candidate, module))
            .unwrap_or(bundle.entry);
        let module_key = module_identity(bundle, module_index);
        for item in &module.items {
            let Item::Const(constant) = item else { continue };
            let Some(output) = constant
                .resolved_output
                .as_ref()
                .filter(|output| output.selected && output.kind == OutputKind::Check)
            else {
                continue;
            };
            let callable_name = if output.semantic_name.is_empty() {
                output.source_name.as_str()
            } else {
                output.semantic_name.as_str()
            };
            if callable_name.is_empty() {
                continue;
            }
            let function = facts
                .functions
                .iter()
                .find(|function| {
                    function.reference.module == module_key
                        && (function.reference.name == callable_name
                            || function.reference.key == callable_name)
                })
                .map(|function| function.reference.clone())
                .unwrap_or_else(|| {
                    function_ref(
                        &module_key,
                        callable_name,
                        output.definition,
                        NameVisibility::Private,
                    )
                });
            rows.push(TirOutputCheck {
                key: format!(
                    "{}::output-check::{}@{}..{}",
                    module_key, output.output_name, output.definition.start, output.definition.end
                ),
                name: output.output_name.clone(),
                function,
            });
        }
    }
    rows
}

fn lower_coverage_points(facts: &TirArtifactFacts) -> Vec<TirCoveragePoint> {
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    for test in &facts.tests {
        let key = format!("{}::coverage::entry", test.key);
        if seen.insert(key.clone()) {
            rows.push(TirCoveragePoint {
                key,
                function: test.function.clone(),
                block: Some("entry".to_string()),
                span: test.span,
            });
        }
    }
    for function in &facts.functions {
        let key = format!("{}::coverage::entry", function.reference.key);
        if seen.insert(key.clone()) {
            rows.push(TirCoveragePoint {
                key,
                function: function.reference.clone(),
                block: Some("entry".to_string()),
                span: function.reference.span,
            });
        }
    }
    rows
}


fn lower_harness(facts: &TirArtifactFacts) -> Option<TirHarnessPlan> {
    if facts.tests.is_empty()
        && facts.output_checks.is_empty()
        && facts.coverage_points.is_empty()
    {
        return None;
    }
    let kind = if facts.tests.is_empty() && facts.output_checks.is_empty() {
        TirHarnessKind::Coverage
    } else {
        TirHarnessKind::Test
    };
    Some(TirHarnessPlan {
        key: format!("{}::harness::{kind:?}", facts.package_identity),
        kind,
        tests: facts.tests.iter().map(|test| test.key.clone()).collect(),
        output_checks: facts.output_checks.clone(),
        selected_test: None,
        coverage_points: facts.coverage_points.clone(),
        command_override: false,
    })
}

fn lower_exports(
    bundle: &ProgramBundle,
    facts: &TirArtifactFacts,
    kind: TirArtifactKind,
) -> Vec<TirExport> {
    let surface = match kind {
        TirArtifactKind::NativeLibrary => crate::Sema::guest_export_surface(bundle),
        TirArtifactKind::SandboxPlugin => crate::Sema::sandbox_export_surface(bundle),
        TirArtifactKind::NativeExecutable
        | TirArtifactKind::WebApplication
        | TirArtifactKind::TestExecutable
        | TirArtifactKind::FuzzExecutable
        | TirArtifactKind::TestOverride => Vec::new(),
    };
    surface
        .into_iter()
        .filter(|export| match kind {
            TirArtifactKind::SandboxPlugin => true,
            _ => export.scalar.is_some(),
        })
        .filter_map(|export| {
            let function = facts
                .functions
                .iter()
                .find(|function| function.reference.name == export.name)?;
            Some(TirExport {
                symbol: export.name,
                function: function.reference.clone(),
                abi: TirExportAbi::C,
            })
        })
        .collect()
}

fn lower_artifact_plan(
    bundle: &ProgramBundle,
    target: TirArtifactTarget,
    facts: &TirArtifactFacts,
) -> TirArtifactPlan {
    let entry = facts.entry.clone();
    let kind = match entry.as_ref().map(|entry| entry.output_kind) {
        Some(OutputKind::Library) => TirArtifactKind::NativeLibrary,
        Some(OutputKind::Service) => TirArtifactKind::WebApplication,
        Some(OutputKind::Check) => TirArtifactKind::TestExecutable,
        _ => match target {
            TirArtifactTarget::Web => TirArtifactKind::WebApplication,
            _ => TirArtifactKind::NativeExecutable,
        },
    };
    let dossier = &bundle.build_facts.target_dossier;
    TirArtifactPlan {
        key: format!("{}::artifact::{target:?}", facts.package_identity),
        kind,
        name: facts.package_identity.clone(),
        target,
        modules: facts.modules.iter().map(|module| module.key.clone()).collect(),
        links: facts.links.iter().map(|link| link.key.clone()).collect(),
        jobs: facts.jobs.iter().map(|job| job.key.clone()).collect(),
        runtime_parts: bundle.used_core.iter().cloned().collect(),
        exports: lower_exports(bundle, facts, kind),
        provider_identity: dossier.provider_identity.clone(),
        closure_identity: dossier.closure_identity.clone(),
        artifact_identity: format!(
            "{}:{}:{}",
            dossier.compiler_identity, dossier.environment_identity, bundle.build_facts.target_triple
        ),
        entry,
        harness: facts.harnesses.first().map(|harness| harness.key.clone()),
    }
}
/// Lower one explicitly requested artifact.  The request owns target, kind,
/// build mode, and optional artifact name; no profile or artifact kind is
/// inferred from source output.
pub(super) fn lower_tir_artifact_facts_for_request(
    bundle: &ProgramBundle,
    request: MirArtifactRequest,
    contract_rows: &ContractSamplingPlan,
) -> TirArtifactFacts {
    let MirArtifactRequest {
        target,
        kind,
        mode,
        name,
    } = request;
    let mut facts = lower_tir_artifact_facts_for_target(bundle, target, contract_rows);
    facts.build_mode = mode;
    if let Some(name) = name {
        if let Some(plan) = facts.artifacts.first_mut() {
            plan.name = name;
        }
    }
    let exports = lower_exports(bundle, &facts, kind);
    if let Some(plan) = facts.artifacts.first_mut() {
        plan.kind = kind;
        plan.exports = exports;
    }
    if matches!(kind, TirArtifactKind::TestExecutable | TirArtifactKind::FuzzExecutable) {
        facts.entry = None;
        if let Some(plan) = facts.artifacts.first_mut() {
            plan.entry = None;
        }
    }
    if mode == MirArtifactBuildMode::Fuzz {
        if let Some(harness) = facts.harnesses.first_mut() {
            harness.kind = TirHarnessKind::Fuzz;
            harness.key = format!("{}::harness::{:?}", facts.package_identity, harness.kind);
        }
        if let Some(plan) = facts.artifacts.first_mut() {
            plan.harness = facts.harnesses.first().map(|harness| harness.key.clone());
        }
    }
    if matches!(
        (kind, mode),
        (TirArtifactKind::TestOverride, MirArtifactBuildMode::Test)
    ) {
        let module = module_identity(bundle, bundle.entry);
        if let Some(function) = facts
            .functions
            .iter()
            .find(|function| {
                function.reference.module == module && function.reference.name == "test"
            })
            .map(|function| function.reference.clone())
        {
            let entry = TirEntrySpec {
                kind: TirArtifactEntryKind::Test,
                function: Some(function),
                cli: None,
                output: TirEntryOutput::ReturnValue,
                output_kind: OutputKind::Check,
                initialize_environment: initialize_environment(bundle, target),
                initialize_gc: false,
                serves_until_stopped: false,
                package_version: bundle.build_facts.package_version.clone(),
            };
            facts.entry = Some(entry.clone());
            if let Some(harness) = facts.harnesses.first_mut() {
                harness.command_override = true;
            }
            if let Some(plan) = facts.artifacts.first_mut() {
                plan.entry = Some(entry);
                plan.harness = facts.harnesses.first().map(|harness| harness.key.clone());
            }
        }
    }
    facts
}

/// Target-aware artifact projection.  The target is typed so no adapter parses
/// a command line or guesses a backend from generated Rust.
pub(super) fn lower_tir_artifact_facts_for_target(
    bundle: &ProgramBundle,
    target: TirArtifactTarget,
    contract_rows: &ContractSamplingPlan,
) -> TirArtifactFacts {
    let package_identity = bundle.build_facts.package_name.clone();
    let mut facts = TirArtifactFacts {
        package_identity: package_identity.clone(),
        package_version: bundle.build_facts.package_version.clone(),
        target,
        build_mode: MirArtifactBuildMode::Dev,
        build: bundle.build_facts.clone(),
        names: lower_names(bundle),
        functions: Vec::new(),
        modules: Vec::new(),
        imports: Vec::new(),
        foreign: Vec::new(),
        links: Vec::new(),
        callbacks: Vec::new(),
        handles: Vec::new(),
        entry: None,
        cli: None,
        jobs: Vec::new(),
        tests: Vec::new(),
        output_checks: Vec::new(),
        coverage_points: Vec::new(),
        harnesses: Vec::new(),
        artifacts: Vec::new(),
        cffi: lower_cffi(bundle),
    };

    for (module_index, module) in bundle.modules.iter().enumerate() {
        let identity = module_identity(bundle, module_index);
        facts.modules.push(TirModuleFact {
            key: identity.clone(),
            name: module.alias.clone(),
            path: module.display.clone(),
            source_path: module.display.clone(),
            imports: module
                .imports
                .iter()
                .map(|import| key(&identity, &import.alias))
                .collect(),
            item_order: module
                .items
                .iter()
                .filter_map(|item| item_ref(&identity, item))
                .collect(),
        });
        collect_imports(bundle, module_index, &identity, &mut facts.imports);
        collect_items(
            bundle,
            module_index,
            &identity,
            &module.items,
            &mut facts,
            target,
            contract_rows,
        );
    }
    facts.links = lower_links(bundle, target);
    facts.handles = facts.cffi.handle_facts.clone();
    let (cli, entry) = lower_cli_and_entry(bundle, target);
    facts.cli = cli;
    facts.entry = entry;
    facts.output_checks = lower_output_checks(bundle, &facts);
    facts.coverage_points = lower_coverage_points(&facts);
    if let Some(harness) = lower_harness(&facts) {
        facts.harnesses.push(harness);
    }
    facts.artifacts.push(lower_artifact_plan(bundle, target, &facts));
    facts.callbacks = lower_callbacks(bundle, &facts);
    facts
}
