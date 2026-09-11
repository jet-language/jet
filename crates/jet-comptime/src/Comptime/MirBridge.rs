//! Dependency-inversion seam: jet-codegen installs the MIR evaluator so
//! comptime/REPL/dev entry points share one engine without a crate cycle.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::sync::OnceLock;

use crate::Comptime::{CtValue, DebugHook, DevSink, PurityStage, ReplAuthorizer};
use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{ComptimeInput, CtFloat, CtKey, CtReport, Expr, Func, ProgramBundle, Stmt, StructDef, Type};
use jet_foundation::MIR::{MirConstKey, MirRuntimeValue, MirType, MirTypeKind};
use jet_foundation::Numeric::CtBigInt;

/// Checked nominal facts retained for a comptime fragment.
///
/// Sema owns the source-to-canonical projection and the checked declaration
/// rows. The fragment lowerer consumes these facts without rediscovering an
/// imported owner from erased `CtValue` data.
#[derive(Clone, Debug, Default)]
pub struct MirFragmentNominalFacts {
    /// Source spellings (including import-qualified and selective-import
    /// names) to canonical `package::module::Type` identities.
    pub nominal_identities: HashMap<String, String>,
    /// Source import alias to the target module's loader alias.
    pub import_modules: HashMap<String, String>,
    /// Canonical nominal identity to the target module's loader alias.
    pub foreign_modules: HashMap<String, String>,
    /// Canonical imported struct identity to its checked declaration row.
    pub structs: HashMap<String, StructDef>,
    /// Checked enum declarations, including their payload shapes.
    pub enums: HashMap<String, crate::AST::EnumDef>,
}

pub struct ExprEvalRequest<'a> {
    pub data_pipeline: &'a mut crate::Comptime::DataPipelineState,
    pub expr: &'a Expr,
    pub funcs: &'a HashMap<String, &'a Func>,
    /// Authoritative static types for names in the active interpreter frame.
    /// Fragment lowering must use these instead of guessing from erased values.
    pub binding_types: &'a HashMap<String, Type>,
    /// Checked `impl Source -> Target` bodies available to fragment lowering.
    /// The evaluator lowers these through the same TIR conversion path as AOT.
    pub error_conversions: &'a [crate::AST::ErrorConvDef],
    /// Exact `(owner, method) -> trait` identity selected by sema for
    /// associated calls. The canonical fragment must carry the same trait
    /// identity as the normal bundle lowering; a method-name-only lookup can
    /// silently turn a literal capability into an inherent call.
    pub method_traits: &'a HashMap<(String, String), String>,
    /// Instance/associated methods are kept in their semantic owner/name table
    /// by comptime. The MIR fragment host needs the same table to lower
    /// computed-field getters and to call user methods.
    pub methods: &'a HashMap<(String, String), &'a Func>,
    pub extern_names: &'a HashSet<String>,
    pub base_dir: &'a Path,
    pub globals: &'a HashMap<String, CtValue>,
    pub core_imports: &'a HashMap<String, String>,
    pub gates: jet_foundation::Policy::GateSet,
    pub initial_impure_depth: usize,
    /// Explicit execution provenance for the canonical fragment. Runtime
    /// producers use the runtime Core dispatcher; comptime evaluation remains
    /// on the gated fragment dispatcher.
    pub runtime_execution: bool,
    pub structs: &'a HashMap<String, &'a StructDef>,
    pub computed_fields: &'a HashMap<(String, String), &'a Expr>,
    pub distinct_ranges: &'a HashMap<String, Option<(i64, i64)>>,
    pub distinct_bases: &'a HashMap<String, Type>,
    /// Unit-family declarations seed the codegen context's canonical facts
    /// for fragment lowering; conversion semantics remain in MIR/Prelude.
    pub unit_families: &'a [crate::AST::UnitFamilyDef],
    pub fuel: u64,
    pub sink: Option<&'a mut DevSink>,
    /// Checked nominal identities and imported struct rows for fragment
    /// lowering. `None` is used by callers without an active module scope.
    pub checked_nominals: Option<MirFragmentNominalFacts>,
    pub repl_mode: bool,
    pub repl_grants: &'a [CtValue],
    pub repl_authorizer: Option<&'a mut dyn ReplAuthorizer>,
    /// D-CTEFFECT1 Tier-1 inputs recorded by the canonical host surface.
    pub embed_inputs: Option<&'a mut Vec<ComptimeInput>>,
    /// Bindings as the fragment left them. An expression can mutate a binding
    /// it reads — `reader.read_u32_le()` advances the reader — and a statement
    /// driver has to see the advance, or the next expression starts over.
    /// `None` for pure const evaluation, which has no caller scope to update.
    pub mutated: Option<&'a mut HashMap<String, CtValue>>,
}
pub struct BlockEvalRequest<'a, 'debug> {
    pub data_pipeline: &'a mut crate::Comptime::DataPipelineState,
    pub stmts: &'a [Stmt],
    pub funcs: &'a HashMap<String, &'a Func>,
    /// Authoritative static types for names in the active interpreter frame.
    /// Fragment lowering must use these instead of guessing from erased values.
    pub binding_types: &'a HashMap<String, Type>,
    pub error_conversions: &'a [crate::AST::ErrorConvDef],
    /// Exact `(owner, method) -> trait` identity selected by sema for
    /// associated calls. The canonical fragment must carry the same trait
    /// identity as the normal bundle lowering.
    pub method_traits: &'a HashMap<(String, String), String>,
    pub methods: &'a HashMap<(String, String), &'a Func>,
    pub extern_names: &'a HashSet<String>,
    pub base_dir: &'a Path,
    pub globals: &'a HashMap<String, CtValue>,
    pub core_imports: &'a HashMap<String, String>,
    pub structs: &'a HashMap<String, &'a StructDef>,
    pub distinct_ranges: &'a HashMap<String, Option<(i64, i64)>>,
    pub distinct_bases: &'a HashMap<String, Type>,
    pub unit_families: &'a [crate::AST::UnitFamilyDef],
    pub fuel: u64,
    pub sink: Option<&'a mut DevSink>,
    /// Checked nominal identities and imported struct rows for fragment
    /// lowering. `None` is used by callers without an active module scope.
    pub checked_nominals: Option<MirFragmentNominalFacts>,
    pub repl_mode: bool,
    pub repl_authorizer: Option<&'a mut dyn ReplAuthorizer>,
    pub gates: jet_foundation::Policy::GateSet,
    pub impure_depth: usize,
    /// Explicit execution provenance for the canonical fragment. Runtime
    /// producers use the runtime Core dispatcher; comptime evaluation remains
    /// on the gated fragment dispatcher.
    pub runtime_execution: bool,
    pub computed_fields: &'a HashMap<(String, String), &'a Expr>,
    /// Optional source debugger carried through the canonical MIR evaluator.
    /// The evaluator owns statement order; the interpreter only supplies the
    /// observation hook.
    pub debugger: Option<&'debug mut dyn DebugHook>,
    pub debug_function: String,
    pub debug_depth: usize,
    /// D-CTEFFECT1 Tier-1 inputs recorded by the canonical host surface.
    pub embed_inputs: Option<&'a mut Vec<ComptimeInput>>,
}

/// Outcome of evaluating a statement list through the canonical MIR evaluator.
pub enum StmtOutcome {
    /// Finished normally; scope holds bindings.
    Done(HashMap<String, CtValue>),
    /// `return` escaped the fragment.
    Returned {
        value: CtValue,
        scope: HashMap<String, CtValue>,
    },
}

pub struct Hooks {
    pub run_bundle: fn(
        &ProgramBundle,
        &mut DevSink,
        jet_foundation::Policy::GateSet,
    ) -> Result<CtValue, Diagnostic>,
    /// Runtime/deopt callers must carry their stage explicitly. The staged
    /// hook prevents a runtime fragment from inheriting build-time purity
    /// defaults while keeping the direct dev/test seam.
    pub run_bundle_at_stage: fn(
        &ProgramBundle,
        &mut DevSink,
        jet_foundation::Policy::GateSet,
        PurityStage,
    ) -> Result<CtValue, Diagnostic>,
    pub eval_expr: fn(&mut ExprEvalRequest<'_>) -> Result<CtValue, Diagnostic>,
    pub eval_block:
        for<'a, 'debug> fn(&mut BlockEvalRequest<'a, 'debug>) -> Result<StmtOutcome, Diagnostic>,
}

static HOOKS: OnceLock<Hooks> = OnceLock::new();

pub fn install(hooks: Hooks) {
    let _ = HOOKS.set(hooks);
}

fn hooks() -> &'static Hooks {
    HOOKS.get().expect(
        "MIR eval bridge not installed — call Codegen::MIREval::install_mir_bridge()",
    )
}

pub fn run_bundle(
    bundle: &ProgramBundle,
    sink: &mut DevSink,
    gates: jet_foundation::Policy::GateSet,
) -> Result<CtValue, Diagnostic> {
    (hooks().run_bundle)(bundle, sink, gates)
}

/// Run a whole program through an explicit purity stage. Runtime/deopt must
/// use [`PurityStage::RunTime`]; build-time evaluation keeps using the
/// expression/block seams, which retain their build-time gate.
pub fn run_bundle_at_stage(
    bundle: &ProgramBundle,
    sink: &mut DevSink,
    gates: jet_foundation::Policy::GateSet,
    stage: PurityStage,
) -> Result<CtValue, Diagnostic> {
    (hooks().run_bundle_at_stage)(bundle, sink, gates, stage)
}

pub fn eval_expr(req: &mut ExprEvalRequest<'_>) -> Result<CtValue, Diagnostic> {
    (hooks().eval_expr)(req)
}

pub fn eval_block<'a, 'debug>(
    req: &mut BlockEvalRequest<'a, 'debug>,
) -> Result<StmtOutcome, Diagnostic> {
    (hooks().eval_block)(req)
}

fn conversion_error(message: impl Into<String>, span: Span) -> Diagnostic {
    Diagnostic::error(
        "E0956",
        message.into(),
        "the comptime boundary accepts only canonical MIR runtime values".to_string(),
        "keep the value on the canonical Prelude/Core path or use a serializable value".to_string(),
        Some(span),
    )
}

pub(super) fn mir_to_ct_key(key: &MirConstKey) -> CtKey {
    match key {
        MirConstKey::Int(value) => CtKey::Int(*value),
        MirConstKey::String(value) => CtKey::Str(value.clone()),
        MirConstKey::Bool(value) => CtKey::Bool(*value),
        MirConstKey::Char(value) => CtKey::Char(*value),
        MirConstKey::Tuple(fields) => CtKey::Tuple(
            fields
                .iter()
                .map(|(name, key)| (name.clone(), mir_to_ct_key(key)))
                .collect(),
        ),
        MirConstKey::Struct { type_name, fields } => CtKey::Struct {
            type_name: type_name.clone(),
            fields: fields
                .iter()
                .map(|(name, key)| (name.clone(), mir_to_ct_key(key)))
                .collect(),
        },
        MirConstKey::Enum { type_name, variant } => CtKey::Enum {
            type_name: type_name.clone(),
            variant: variant.clone(),
        },
    }
}

pub(super) fn ct_to_mir_key(key: CtKey) -> MirConstKey {
    match key {
        CtKey::Int(value) => MirConstKey::Int(value),
        CtKey::Str(value) => MirConstKey::String(value),
        CtKey::Bool(value) => MirConstKey::Bool(value),
        CtKey::Char(value) => MirConstKey::Char(value),
        CtKey::Tuple(fields) => MirConstKey::Tuple(
            fields
                .into_iter()
                .map(|(name, key)| (name, ct_to_mir_key(key)))
                .collect(),
        ),
        CtKey::Struct { type_name, fields } => MirConstKey::Struct {
            type_name,
            fields: fields
                .into_iter()
                .map(|(name, key)| (name, ct_to_mir_key(key)))
                .collect(),
        },
        CtKey::Enum { type_name, variant } => MirConstKey::Enum { type_name, variant },
    }
}

/// Project an AST type onto the canonical MIR type shape without flattening
/// generic applications into display strings.
pub fn ast_to_mir_type(ty: &Type) -> MirType {
    let kind = match ty {
        Type::Int => MirTypeKind::Int,
        Type::Float => MirTypeKind::Float,
        Type::Bool => MirTypeKind::Bool,
        Type::String => MirTypeKind::String,
        Type::Char => MirTypeKind::Char,
        Type::List(inner) => MirTypeKind::List(Box::new(ast_to_mir_type(inner))),
        Type::Map { key, value, .. } => MirTypeKind::Map {
            key: Box::new(ast_to_mir_type(key)),
            value: Box::new(ast_to_mir_type(value)),
        },
        Type::Shared(inner) => MirTypeKind::Shared(Box::new(ast_to_mir_type(inner))),
        Type::Option(inner) => MirTypeKind::Option(Box::new(ast_to_mir_type(inner))),
        Type::Result { ok, err } => MirTypeKind::Result {
            ok: Box::new(ast_to_mir_type(ok)),
            err: Box::new(ast_to_mir_type(err)),
        },
        Type::Fn {
            params,
            ret,
            effect_bound,
            param_contract,
            call_metadata,
            return_view_provenance,
        } => MirTypeKind::Fn(ast_to_mir_function_signature(
            params,
            ret,
            effect_bound,
            param_contract,
            call_metadata,
            return_view_provenance,
        )),
        Type::Named(name) => MirTypeKind::Apply {
            name: jet_foundation::MIR::MirNominalRef::from_name(name),
            args: Vec::new(),
        },
        Type::Apply { name, args } => MirTypeKind::Apply {
            name: jet_foundation::MIR::MirNominalRef::from_name(name),
            args: args.iter().map(ast_to_mir_type).collect(),
        },
        Type::TraitObject(bounds) => MirTypeKind::TraitObject(
            bounds
                .iter()
                .map(|bound| {
                    jet_foundation::MIR::MirNominalRef::from_name(bound.clone())
                })
                .collect(),
        ),
        Type::Tuple(fields) => MirTypeKind::Tuple(
            fields
                .iter()
                .map(|(name, ty)| (name.clone(), ast_to_mir_type(ty)))
                .collect(),
        ),
        Type::FixedList { elem, len } => MirTypeKind::FixedList {
            elem: Box::new(ast_to_mir_type(elem)),
            len: ast_to_mir_measure(len),
        },
        Type::IntN { signed, bits } => MirTypeKind::IntN {
            signed: *signed,
            bits: *bits,
        },
        Type::InlineRange { base, lo, hi } => MirTypeKind::InlineRange {
            base: Box::new(ast_to_mir_type(base)),
            lo: *lo,
            hi: *hi,
        },
        Type::Float32 => MirTypeKind::Float32,
        Type::Tagged { marker, inner } => MirTypeKind::Tagged {
            marker: ast_to_mir_tag_marker(marker),
            inner: Box::new(ast_to_mir_type(inner)),
        },
        Type::Union(members) => {
            MirTypeKind::Union(members.iter().map(ast_to_mir_type).collect())
        }
        Type::Quantity { base, dimension } => MirTypeKind::Quantity {
            base: Box::new(ast_to_mir_type(base)),
            dimension: ast_to_mir_dimension(dimension),
        },
        Type::Measure(measure) => MirTypeKind::Measure(ast_to_mir_measure(measure)),
    };
    MirType::from_kind(kind).with_identity(jet_foundation::MIR::MirTypeId(
        jet_foundation::MIR::stable_id("mir-type", &ty.identity_key()),
    ))
}

fn ast_to_mir_function_signature(
    params: &[Type],
    ret: &Option<Box<Type>>,
    effect_bound: &Option<Vec<(String, Span)>>,
    param_contract: &Option<Vec<(String, crate::AST::ParamZone)>>,
    call_metadata: &Option<crate::AST::FunctionCallMetadata>,
    return_view_provenance: &Option<crate::AST::ViewProvenanceMap>,
) -> jet_foundation::MIR::MirFunctionSignature {
    jet_foundation::MIR::MirFunctionSignature {
        params: params.iter().map(ast_to_mir_type).collect(),
        ret: ret.as_deref().map(ast_to_mir_type).map(Box::new),
        effect_bound: effect_bound
            .as_ref()
            .map(|row| row.iter().map(|(name, _)| name.clone()).collect()),
        param_contract: param_contract.as_ref().map(|row| {
            row.iter()
                .map(|(label, zone)| jet_foundation::MIR::MirCallContractRow {
                    label: label.clone(),
                    zone: match zone {
                        crate::AST::ParamZone::PositionalOnly => {
                            jet_foundation::MIR::MirParamZone::PositionalOnly
                        }
                        crate::AST::ParamZone::Either => jet_foundation::MIR::MirParamZone::Either,
                        crate::AST::ParamZone::LabelOnly => {
                            jet_foundation::MIR::MirParamZone::LabelOnly
                        }
                    },
                })
                .collect()
        }),
        call_metadata: call_metadata.as_ref().map(ast_to_mir_call_metadata),
        return_view_provenance: return_view_provenance
            .as_ref()
            .map(ast_to_mir_view_provenance),
    }
}

fn ast_to_mir_call_metadata(
    metadata: &crate::AST::FunctionCallMetadata,
) -> jet_foundation::MIR::MirCallMetadata {
    jet_foundation::MIR::MirCallMetadata {
        names: metadata.names.clone(),
        defaults: metadata.defaults.iter().map(Option::is_some).collect(),
        variadic: metadata.variadic.clone(),
        conventions: metadata
            .conventions
            .iter()
            .copied()
            .map(ast_to_mir_access)
            .collect(),
        policies: jet_foundation::MIR::MirCallablePolicyChain {
            policies: metadata
                .policies
                .policies
                .iter()
                .map(|policy| jet_foundation::MIR::MirCallablePolicy {
                    name: policy.name.clone(),
                    arguments: policy.arguments.clone(),
                })
                .collect(),
        },
    }
}

fn ast_to_mir_access(access: crate::AST::AccessConvention) -> jet_foundation::MIR::MirAccess {
    match access {
        crate::AST::AccessConvention::Read => jet_foundation::MIR::MirAccess::Read,
        crate::AST::AccessConvention::Write => jet_foundation::MIR::MirAccess::Write,
        crate::AST::AccessConvention::Move => jet_foundation::MIR::MirAccess::Move,
    }
}

fn ast_to_mir_view_provenance(
    map: &crate::AST::ViewProvenanceMap,
) -> BTreeMap<Vec<String>, jet_foundation::MIR::MirViewProvenance> {
    map.iter()
        .map(|(path, provenance)| {
            let sources = provenance
                .sources
                .iter()
                .map(|source| jet_foundation::MIR::MirViewSourcePath {
                    source: match &source.source {
                        crate::AST::ViewSource::Receiver => {
                            jet_foundation::MIR::MirViewSource::Receiver
                        }
                        crate::AST::ViewSource::Parameter(index) => {
                            jet_foundation::MIR::MirViewSource::Parameter(*index)
                        }
                        crate::AST::ViewSource::Static { module_path, name } => {
                            jet_foundation::MIR::MirViewSource::Static {
                                module_path: module_path.clone(),
                                name: name.clone(),
                            }
                        }
                    },
                    projections: source
                        .projections
                        .iter()
                        .map(|projection| match projection {
                            crate::AST::ViewSourceProjection::Field(name) => {
                                jet_foundation::MIR::MirViewProjection::Field(name.clone())
                            }
                            crate::AST::ViewSourceProjection::Index => {
                                jet_foundation::MIR::MirViewProjection::Index
                            }
                            crate::AST::ViewSourceProjection::Range => {
                                jet_foundation::MIR::MirViewProjection::Range
                            }
                        })
                        .collect(),
                })
                .collect::<BTreeSet<_>>();
            (
                path.clone(),
                jet_foundation::MIR::MirViewProvenance {
                    sources,
                    mutable: provenance.mutable,
                },
            )
        })
        .collect()
}

fn ast_to_mir_measure(
    measure: &crate::AST::Measure,
) -> jet_foundation::MIR::MirMeasure {
    match measure {
        crate::AST::Measure::Literal { kind, value } => {
            jet_foundation::MIR::MirMeasure::Literal {
                kind: kind.clone(),
                value: *value,
            }
        }
        crate::AST::Measure::SignedLiteral { kind, value } => {
            jet_foundation::MIR::MirMeasure::SignedLiteral {
                kind: kind.clone(),
                value: *value,
            }
        }
        crate::AST::Measure::Symbol { kind, name } => {
            jet_foundation::MIR::MirMeasure::Symbol {
                kind: kind.clone(),
                name: name.clone(),
            }
        }
        crate::AST::Measure::Combined {
            kind,
            rule,
            left,
            right,
        } => jet_foundation::MIR::MirMeasure::Combined {
            kind: kind.clone(),
            rule: match rule {
                crate::AST::MeasureRule::Add => jet_foundation::MIR::MirMeasureRule::Add,
                crate::AST::MeasureRule::Mul => jet_foundation::MIR::MirMeasureRule::Mul,
                crate::AST::MeasureRule::Match => jet_foundation::MIR::MirMeasureRule::Match,
            },
            left: Box::new(ast_to_mir_measure(left)),
            right: Box::new(ast_to_mir_measure(right)),
        },
    }
}

fn ast_to_mir_dimension(
    dimension: &crate::AST::Dimension,
) -> jet_foundation::MIR::MirDimension {
    jet_foundation::MIR::MirDimension {
        axes: dimension
            .measure_exponents()
            .map(|(axis, exponent)| (axis.to_string(), ast_to_mir_measure(&exponent)))
            .collect(),
    }
}

fn ast_to_mir_tag_marker(
    marker: &crate::AST::TagMarker,
) -> jet_foundation::MIR::MirTagMarker {
    match marker {
        crate::AST::TagMarker::User(name) => {
            jet_foundation::MIR::MirTagMarker::User(name.clone())
        }
        crate::AST::TagMarker::Internal(tag) => {
            jet_foundation::MIR::MirTagMarker::Internal(match tag {
                crate::AST::InternalTag::CoreCryptoNominal => {
                    jet_foundation::MIR::MirInternalTag::CoreCryptoNominal
                }
                crate::AST::InternalTag::DeterministicClock => {
                    jet_foundation::MIR::MirInternalTag::DeterministicClock
                }
                crate::AST::InternalTag::SystemClock => {
                    jet_foundation::MIR::MirInternalTag::SystemClock
                }
                crate::AST::InternalTag::ExpiringSecretLoan => {
                    jet_foundation::MIR::MirInternalTag::ExpiringSecretLoan
                }
                crate::AST::InternalTag::SharedGuardRead => {
                    jet_foundation::MIR::MirInternalTag::SharedGuardRead
                }
                crate::AST::InternalTag::SharedGuardEdit => {
                    jet_foundation::MIR::MirInternalTag::SharedGuardEdit
                }
                crate::AST::InternalTag::TerminalFactSet => {
                    jet_foundation::MIR::MirInternalTag::TerminalFactSet
                }
                crate::AST::InternalTag::CppCallbackAbi => {
                    jet_foundation::MIR::MirInternalTag::CppCallbackAbi
                }
                crate::AST::InternalTag::AllocatorView => {
                    jet_foundation::MIR::MirInternalTag::AllocatorView
                }
            })
        }
    }
}

/// Recover an AST type for the comptime boundary without converting an
/// applied type into a single display-name string.
pub fn mir_to_ast_type(ty: &MirType) -> Type {
    match ty.kind() {
        MirTypeKind::Int => Type::Int,
        MirTypeKind::Float => Type::Float,
        MirTypeKind::Bool => Type::Bool,
        MirTypeKind::String => Type::String,
        MirTypeKind::Char => Type::Char,
        MirTypeKind::List(inner) => Type::List(Box::new(mir_to_ast_type(inner))),
        MirTypeKind::Map { key, value } => Type::Map {
            key: Box::new(mir_to_ast_type(key)),
            key_span: None,
            value: Box::new(mir_to_ast_type(value)),
        },
        MirTypeKind::Shared(inner) => Type::Shared(Box::new(mir_to_ast_type(inner))),
        MirTypeKind::Option(inner) => Type::Option(Box::new(mir_to_ast_type(inner))),
        MirTypeKind::Result { ok, err } => Type::Result {
            ok: Box::new(mir_to_ast_type(ok)),
            err: Box::new(mir_to_ast_type(err)),
        },
        MirTypeKind::Fn(signature) => Type::Fn {
            params: signature.params.iter().map(mir_to_ast_type).collect(),
            ret: signature
                .ret
                .as_deref()
                .map(mir_to_ast_type)
                .map(Box::new),
            effect_bound: signature.effect_bound.as_ref().map(|row| {
                row.iter()
                    .map(|name| (name.clone(), Span::new(0, 0)))
                    .collect()
            }),
            param_contract: signature.param_contract.as_ref().map(|row| {
                row.iter()
                    .map(|row| {
                        (
                            row.label.clone(),
                            match row.zone {
                                jet_foundation::MIR::MirParamZone::PositionalOnly => {
                                    crate::AST::ParamZone::PositionalOnly
                                }
                                jet_foundation::MIR::MirParamZone::Either => {
                                    crate::AST::ParamZone::Either
                                }
                                jet_foundation::MIR::MirParamZone::LabelOnly => {
                                    crate::AST::ParamZone::LabelOnly
                                }
                            },
                        )
                    })
                    .collect()
            }),
            call_metadata: signature
                .call_metadata
                .as_ref()
                .map(mir_to_ast_call_metadata),
            return_view_provenance: signature
                .return_view_provenance
                .as_ref()
                .map(mir_to_ast_view_provenance),
        },
        MirTypeKind::SendFn { params, ret } => Type::Fn {
            params: params.iter().map(mir_to_ast_type).collect(),
            ret: ret.as_deref().map(mir_to_ast_type).map(Box::new),
            effect_bound: None,
            param_contract: None,
            call_metadata: None,
            return_view_provenance: None,
        },
        MirTypeKind::Apply { name, args } => {
            if args.is_empty() {
                Type::Named(name.name.clone())
            } else {
                Type::Apply {
                    name: name.name.clone(),
                    args: args.iter().map(mir_to_ast_type).collect(),
                }
            }
        }
        MirTypeKind::TraitObject(bounds) => {
            Type::TraitObject(bounds.iter().map(|bound| bound.name.clone()).collect())
        }
        MirTypeKind::Tuple(fields) => Type::Tuple(
            fields
                .iter()
                .map(|(name, ty)| (name.clone(), Box::new(mir_to_ast_type(ty))))
                .collect(),
        ),
        MirTypeKind::FixedList { elem, len } => Type::FixedList {
            elem: Box::new(mir_to_ast_type(elem)),
            len: mir_to_ast_measure(len),
        },
        MirTypeKind::IntN { signed, bits } => Type::IntN {
            signed: *signed,
            bits: *bits,
        },
        MirTypeKind::InlineRange { base, lo, hi } => Type::InlineRange {
            base: Box::new(mir_to_ast_type(base)),
            lo: *lo,
            hi: *hi,
        },
        MirTypeKind::Float32 => Type::Float32,
        MirTypeKind::Tagged { marker, inner } => Type::Tagged {
            marker: mir_to_ast_tag_marker(marker),
            inner: Box::new(mir_to_ast_type(inner)),
        },
        MirTypeKind::Union(members) => {
            Type::Union(members.iter().map(mir_to_ast_type).collect())
        }
        MirTypeKind::Quantity { base, dimension } => Type::Quantity {
            base: Box::new(mir_to_ast_type(base)),
            dimension: mir_to_ast_dimension(dimension),
        },
        MirTypeKind::Measure(measure) => Type::Measure(mir_to_ast_measure(measure)),
    }
}

fn mir_to_ast_call_metadata(
    metadata: &jet_foundation::MIR::MirCallMetadata,
) -> crate::AST::FunctionCallMetadata {
    crate::AST::FunctionCallMetadata {
        names: metadata.names.clone(),
        defaults: metadata
            .defaults
            .iter()
            .map(|_| None::<crate::AST::Expr>)
            .collect(),
        variadic: metadata.variadic.clone(),
        conventions: metadata
            .conventions
            .iter()
            .copied()
            .map(mir_to_ast_access)
            .collect(),
        policies: crate::AST::CallablePolicyChain {
            policies: metadata
                .policies
                .policies
                .iter()
                .map(|policy| crate::AST::CallablePolicy {
                    name: policy.name.clone(),
                    arguments: policy.arguments.clone(),
                })
                .collect(),
        },
    }
}

fn mir_to_ast_access(access: jet_foundation::MIR::MirAccess) -> crate::AST::AccessConvention {
    match access {
        jet_foundation::MIR::MirAccess::Read => crate::AST::AccessConvention::Read,
        jet_foundation::MIR::MirAccess::Write => crate::AST::AccessConvention::Write,
        jet_foundation::MIR::MirAccess::Move => crate::AST::AccessConvention::Move,
    }
}

fn mir_to_ast_view_provenance(
    map: &BTreeMap<Vec<String>, jet_foundation::MIR::MirViewProvenance>,
) -> crate::AST::ViewProvenanceMap {
    map.iter()
        .map(|(path, provenance)| {
            let sources = provenance
                .sources
                .iter()
                .map(|source| crate::AST::ViewSourcePath {
                    source: match &source.source {
                        jet_foundation::MIR::MirViewSource::Receiver => {
                            crate::AST::ViewSource::Receiver
                        }
                        jet_foundation::MIR::MirViewSource::Parameter(index) => {
                            crate::AST::ViewSource::Parameter(*index)
                        }
                        jet_foundation::MIR::MirViewSource::Static { module_path, name } => {
                            crate::AST::ViewSource::Static {
                                module_path: module_path.clone(),
                                name: name.clone(),
                            }
                        }
                    },
                    projections: source
                        .projections
                        .iter()
                        .map(|projection| match projection {
                            jet_foundation::MIR::MirViewProjection::Field(name) => {
                                crate::AST::ViewSourceProjection::Field(name.clone())
                            }
                            jet_foundation::MIR::MirViewProjection::Index => {
                                crate::AST::ViewSourceProjection::Index
                            }
                            jet_foundation::MIR::MirViewProjection::Range => {
                                crate::AST::ViewSourceProjection::Range
                            }
                        })
                        .collect(),
                })
                .collect::<BTreeSet<_>>();
            (
                path.clone(),
                crate::AST::ViewProvenance {
                    sources,
                    mutable: provenance.mutable,
                },
            )
        })
        .collect()
}

fn mir_to_ast_measure(measure: &jet_foundation::MIR::MirMeasure) -> crate::AST::Measure {
    match measure {
        jet_foundation::MIR::MirMeasure::Literal { kind, value } => {
            crate::AST::Measure::Literal {
                kind: kind.clone(),
                value: *value,
            }
        }
        jet_foundation::MIR::MirMeasure::SignedLiteral { kind, value } => {
            crate::AST::Measure::SignedLiteral {
                kind: kind.clone(),
                value: *value,
            }
        }
        jet_foundation::MIR::MirMeasure::Symbol { kind, name } => {
            crate::AST::Measure::Symbol {
                kind: kind.clone(),
                name: name.clone(),
            }
        }
        jet_foundation::MIR::MirMeasure::Combined {
            kind,
            rule,
            left,
            right,
        } => crate::AST::Measure::Combined {
            kind: kind.clone(),
            rule: match rule {
                jet_foundation::MIR::MirMeasureRule::Add => crate::AST::MeasureRule::Add,
                jet_foundation::MIR::MirMeasureRule::Mul => crate::AST::MeasureRule::Mul,
                jet_foundation::MIR::MirMeasureRule::Match => crate::AST::MeasureRule::Match,
            },
            left: Box::new(mir_to_ast_measure(left)),
            right: Box::new(mir_to_ast_measure(right)),
        },
    }
}

fn mir_to_ast_dimension(
    dimension: &jet_foundation::MIR::MirDimension,
) -> crate::AST::Dimension {
    crate::AST::Dimension::from_identity(&dimension.identity())
        .expect("canonical MIR dimensions use concrete exponents")
}

fn mir_to_ast_tag_marker(
    marker: &jet_foundation::MIR::MirTagMarker,
) -> crate::AST::TagMarker {
    match marker {
        jet_foundation::MIR::MirTagMarker::User(name) => {
            crate::AST::TagMarker::User(name.clone())
        }
        jet_foundation::MIR::MirTagMarker::Internal(tag) => {
            crate::AST::TagMarker::Internal(match tag {
                jet_foundation::MIR::MirInternalTag::CoreCryptoNominal => {
                    crate::AST::InternalTag::CoreCryptoNominal
                }
                jet_foundation::MIR::MirInternalTag::DeterministicClock => {
                    crate::AST::InternalTag::DeterministicClock
                }
                jet_foundation::MIR::MirInternalTag::SystemClock => {
                    crate::AST::InternalTag::SystemClock
                }
                jet_foundation::MIR::MirInternalTag::ExpiringSecretLoan => {
                    crate::AST::InternalTag::ExpiringSecretLoan
                }
                jet_foundation::MIR::MirInternalTag::SharedGuardRead => {
                    crate::AST::InternalTag::SharedGuardRead
                }
                jet_foundation::MIR::MirInternalTag::SharedGuardEdit => {
                    crate::AST::InternalTag::SharedGuardEdit
                }
                jet_foundation::MIR::MirInternalTag::TerminalFactSet => {
                    crate::AST::InternalTag::TerminalFactSet
                }
                jet_foundation::MIR::MirInternalTag::CppCallbackAbi => {
                    crate::AST::InternalTag::CppCallbackAbi
                }
                jet_foundation::MIR::MirInternalTag::AllocatorView => {
                    crate::AST::InternalTag::AllocatorView
                }
            })
        }
    }
}

/// Marshal one canonical MIR value into the comptime value boundary.
pub fn mir_to_ct_value(value: MirRuntimeValue, span: Span) -> Result<CtValue, Diagnostic> {
    match value {
        MirRuntimeValue::Int(value) => Ok(CtValue::Int(value)),
        MirRuntimeValue::BigInt(value) => Ok(CtValue::BigInt(
            CtBigInt::from_str(&value)
                .map_err(|message| conversion_error(message, span))?,
        )),
        MirRuntimeValue::Float { value, f32 } => {
            Ok(CtValue::Float(if f32 { CtFloat::F32(value as f32) } else { CtFloat::F64(value) }))
        }
        MirRuntimeValue::Bool(value) => Ok(CtValue::Bool(value)),
        MirRuntimeValue::Char(value) => Ok(CtValue::Char(value)),
        MirRuntimeValue::String(value) => Ok(CtValue::Str(value)),
        MirRuntimeValue::Bytes(value) => Ok(CtValue::Bytes(value)),
        MirRuntimeValue::List(values) => values
            .into_iter()
            .map(|value| mir_to_ct_value(value, span))
            .collect::<Result<Vec<_>, Diagnostic>>()
            .map(CtValue::List),
        MirRuntimeValue::Map(values) => values
            .into_iter()
            .map(|(key, value)| Ok((mir_to_ct_key(&key), mir_to_ct_value(value, span)?)))
            .collect::<Result<BTreeMap<_, _>, Diagnostic>>()
            .map(CtValue::Map),
        MirRuntimeValue::Struct { type_name, fields } => fields
            .into_iter()
            .map(|(name, value)| Ok((name, mir_to_ct_value(value, span)?)))
            .collect::<Result<Vec<_>, Diagnostic>>()
            .map(|fields| CtValue::Struct { type_name: user_type_name(type_name), fields }),
        MirRuntimeValue::Enum {
            type_name,
            variant,
            args,
        } => args
            .into_iter()
            .map(|(name, value)| Ok((name, mir_to_ct_value(value, span)?)))
            .collect::<Result<Vec<_>, Diagnostic>>()
            .map(|args| CtValue::Enum {
                type_name: user_type_name(type_name),
                variant,
                args,
            }),
        MirRuntimeValue::Present(value) => Ok(CtValue::Present(Box::new(mir_to_ct_value(
            *value, span,
        )?))),
        MirRuntimeValue::FailedTold(value) => Ok(CtValue::Failed(CtReport::Told(Box::new(
            mir_to_ct_value(*value, span)?,
        )))),
        MirRuntimeValue::Absent { element } => Ok(CtValue::absent(mir_to_ast_type(&element))),
        MirRuntimeValue::Unit => Ok(CtValue::Unit),
        MirRuntimeValue::Moved => Err(conversion_error(
            "internal moved value cannot cross the comptime boundary",
            span,
        )),
        MirRuntimeValue::Closure(_) => Err(conversion_error(
            "MIR Core call cannot marshal a closure through the comptime boundary",
            span,
        )),
    }
}


fn user_type_name(name: String) -> String {
    let stripped = name.strip_prefix("__comptime::").unwrap_or(&name);
    stripped.strip_suffix("<>").unwrap_or(stripped).to_string()
}

/// Marshal one comptime value into the canonical MIR value boundary.
pub fn ct_to_mir_value(value: CtValue, span: Span) -> Result<MirRuntimeValue, Diagnostic> {
    match value {
        CtValue::Int(value) => Ok(MirRuntimeValue::Int(value)),
        CtValue::Float(value) => Ok(match value {
            CtFloat::F32(value) => MirRuntimeValue::Float {
                value: f64::from(value),
                f32: true,
            },
            CtFloat::F64(value) => MirRuntimeValue::Float { value, f32: false },
        }),
        CtValue::Bool(value) => Ok(MirRuntimeValue::Bool(value)),
        CtValue::Char(value) => Ok(MirRuntimeValue::Char(value)),
        CtValue::Str(value) => Ok(MirRuntimeValue::String(value)),
        CtValue::BigInt(value) => Ok(MirRuntimeValue::BigInt(value.to_string_rep())),
        CtValue::Bytes(value) => Ok(MirRuntimeValue::Bytes(value)),
        CtValue::List(values) => values
            .into_iter()
            .map(|value| ct_to_mir_value(value, span))
            .collect::<Result<Vec<_>, Diagnostic>>()
            .map(MirRuntimeValue::List),
        CtValue::Map(values) => values
            .into_iter()
            .map(|(key, value)| Ok((ct_to_mir_key(key), ct_to_mir_value(value, span)?)))
            .collect::<Result<Vec<_>, Diagnostic>>()
            .map(MirRuntimeValue::Map),
        CtValue::Struct { type_name, fields } => fields
            .into_iter()
            .map(|(name, value)| Ok((name, ct_to_mir_value(value, span)?)))
            .collect::<Result<Vec<_>, Diagnostic>>()
            .map(|fields| MirRuntimeValue::Struct { type_name, fields }),
        CtValue::Enum {
            type_name,
            variant,
            args,
        } => args
            .into_iter()
            .map(|(name, value)| Ok((name, ct_to_mir_value(value, span)?)))
            .collect::<Result<Vec<_>, Diagnostic>>()
            .map(|args| MirRuntimeValue::Enum {
                type_name,
                variant,
                args,
            }),
        CtValue::Present(value) => Ok(MirRuntimeValue::Present(Box::new(ct_to_mir_value(
            *value, span,
        )?))),
        CtValue::Failed(CtReport::Told(value)) => Ok(MirRuntimeValue::FailedTold(Box::new(
            ct_to_mir_value(*value, span)?,
        ))),
        CtValue::Failed(CtReport::Clean(element)) => Ok(MirRuntimeValue::Absent {
            element: ast_to_mir_type(&element),
        }),
        CtValue::Unit => Ok(MirRuntimeValue::Unit),
        CtValue::Closure(_) => Err(conversion_error(
            "the comptime boundary cannot materialize a closure as a canonical MIR value",
            span,
        )),
    }
}
