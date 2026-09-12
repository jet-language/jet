//! Semantic projections shared by the checked TIR and canonical MIR.
//!
//! This module deliberately contains no executable lowering.  It snapshots the
//! checked type/foreign/erasure facts that the canonical MIR owner can project
//! without looking back at the AST.  Runtime representation is constructed
//! here from checked source facts; MIR itself remains frontend-neutral.

#![allow(dead_code)]
use super::{TirErasure, TirErasureReason};
use crate::Diagnostics::Span;
use crate::AST::{
    AccessConvention, CLICommandBinding, ConstDef, CtValue, Dimension, DistinctDef, EnumDef, Expr,
    Field, Func, FunctionCallMetadata, ImplDef, InternalTag, Item, Marker, Measure, MeasureRule,
    Param, ParamZone, ProgramBundle, StructDef, StructLayout, TagMarker, TraitDef, TraitImplBlock,
    Type, TypeAliasDef, UnitFamilyDef, Variant, VariantPayload, ViewProvenanceMap, ViewSource,
    ViewSourceProjection,
};
use jet_foundation::CanonicalPass;
use jet_foundation::Layout::{LayoutAlignmentFact, TargetLayoutEngine};
use jet_foundation::Shape::ShapeFieldNames;
use jet_foundation::MIR::{
    stable_id, MirAccess, MirCallContractRow, MirCallMetadata, MirCallablePolicy,
    MirCallablePolicyChain, MirCliBinding, MirCliCommand, MirCliEntry,
    MirDimension, MirDropKind, MirErasureReason, MirField,
    MirFieldId, MirFunctionId, MirFunctionSignature, MirGenericParam, MirInternalTag, MirMeasure,
    MirMeasureRule, MirModuleId, MirNominalRef, MirOwnership, MirOwnershipMode, MirParam,
    MirParamZone, MirSerdeAttribute, MirSerdeAttributeKind, MirStructLayout, MirTagMarker,
    MirTraitId, MirTraitRef, MirType, MirTypeDef, MirTypeDefKind, MirTypeId, MirVariant,
    MirVariantPayload, MirViewProjection, MirViewProvenance, MirViewSource, MirViewSourcePath,
};
use std::collections::{BTreeMap, HashSet};

/// All declaration rows projected from one checked program.  The executable
/// function table remains owned by the sibling TIR lowering.
#[derive(Debug, Clone, Default)]
pub(super) struct TirDeclarations {
    pub type_defs: Vec<TirTypeDef>,
    pub traits: Vec<TirTraitDef>,
    pub impls: Vec<TirImplDef>,
    pub constants: Vec<TirConstantDef>,
}

/// Canonicalize the one source spelling whose meaning depends on checked
/// namespace facts: a bare trait name is a trait object unless a nominal or
/// active generic binder owns that name.
pub(crate) fn canonicalize_checked_trait_types(
    ty: &Type,
    trait_names: &HashSet<String>,
    nominal_names: &HashSet<String>,
    binders: &HashSet<String>,
) -> Type {
    fn visit(
        ty: &Type,
        trait_names: &HashSet<String>,
        nominal_names: &HashSet<String>,
        binders: &HashSet<String>,
    ) -> Type {
        match ty {
            Type::Named(name)
                if trait_names.contains(name)
                    && !nominal_names.contains(name)
                    && !binders.contains(name) =>
            {
                Type::TraitObject(vec![name.clone()])
            }
            Type::List(inner) => Type::List(Box::new(visit(
                inner,
                trait_names,
                nominal_names,
                binders,
            ))),
            Type::Map {
                key,
                key_span,
                value,
            } => Type::Map {
                key: Box::new(visit(key, trait_names, nominal_names, binders)),
                key_span: *key_span,
                value: Box::new(visit(value, trait_names, nominal_names, binders)),
            },
            Type::Shared(inner) => Type::Shared(Box::new(visit(
                inner,
                trait_names,
                nominal_names,
                binders,
            ))),
            Type::Option(inner) => Type::Option(Box::new(visit(
                inner,
                trait_names,
                nominal_names,
                binders,
            ))),
            Type::Result { ok, err } => Type::Result {
                ok: Box::new(visit(ok, trait_names, nominal_names, binders)),
                err: Box::new(visit(err, trait_names, nominal_names, binders)),
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
                    .map(|param| visit(param, trait_names, nominal_names, binders))
                    .collect(),
                ret: ret.as_ref().map(|ret| {
                    Box::new(visit(ret, trait_names, nominal_names, binders))
                }),
                effect_bound: effect_bound.clone(),
                param_contract: param_contract.clone(),
                call_metadata: call_metadata.clone(),
                return_view_provenance: return_view_provenance.clone(),
            },
            Type::Apply { name, args } => Type::Apply {
                name: name.clone(),
                args: args
                    .iter()
                    .map(|arg| visit(arg, trait_names, nominal_names, binders))
                    .collect(),
            },
            Type::Tuple(fields) => Type::Tuple(
                fields
                    .iter()
                    .map(|(name, ty)| {
                        (
                            name.clone(),
                            Box::new(visit(ty, trait_names, nominal_names, binders)),
                        )
                    })
                    .collect(),
            ),
            Type::FixedList { elem, len } => Type::FixedList {
                elem: Box::new(visit(elem, trait_names, nominal_names, binders)),
                len: len.clone(),
            },
            Type::InlineRange { base, lo, hi } => Type::InlineRange {
                base: Box::new(visit(base, trait_names, nominal_names, binders)),
                lo: *lo,
                hi: *hi,
            },
            Type::Tagged { marker, inner } => Type::Tagged {
                marker: marker.clone(),
                inner: Box::new(visit(inner, trait_names, nominal_names, binders)),
            },
            Type::Union(members) => Type::Union(
                members
                    .iter()
                    .map(|member| visit(member, trait_names, nominal_names, binders))
                    .collect(),
            ),
            Type::Quantity { base, dimension } => Type::Quantity {
                base: Box::new(visit(base, trait_names, nominal_names, binders)),
                dimension: dimension.clone(),
            },
            _ => ty.clone(),
        }
    }

    visit(ty, trait_names, nominal_names, binders)
}

/// Access selected by sema for a parameter or call slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum TirAccess {
    Read,
    Write,
    Move,
}

/// Ownership mode before the canonical MIR projection.
///
/// Keeping this small semantic enum in TIR avoids making analytical TIR depend
/// on MIR's storage details.  The MIR owner maps it one-for-one to
/// `MirOwnershipMode` and supplies drop/last-use bits from executable facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum TirOwnership {
    Owned,
    ReadBorrow,
    WriteBorrow,
    Move,
}

/// Visibility is a checked declaration fact, not an emitter choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum TirVisibility {
    Private,
    Package,
    Public,
}

/// Checked nominal type definition.  Method names are semantic keys; the MIR
/// owner derives IDs from them only after all function rows have been collected.
#[derive(Debug, Clone)]
pub(super) struct TirTypeDef {
    pub module: String,
    pub key: String,
    pub name: String,
    pub span: Span,
    pub public: bool,
    pub package_public: bool,
    pub generic_params: Vec<super::TGenericParam>,
    pub derives: Vec<String>,
    pub auto_derive_default: bool,
    pub auto_printable: bool,
    pub published_schema: bool,
    pub single_use: bool,
    pub must_use: bool,
    pub layout: Option<StructLayout>,
    pub layout_alignment: Option<LayoutAlignmentFact>,
    pub serde: Vec<TirSerdeAttribute>,
    pub cli_bindings: Vec<TirCliBinding>,
    /// D-SHAPE-PROJECT1: `#CLI` input rows derived by the same entry schema
    /// builder `fn run(args: T)` uses. `None` for every non-CLI type.
    pub cli: Option<super::artifact_plan::TirCliEntry>,
    pub ownership: TirOwnership,
    /// Checked recursive-layout edge keys owned by this declaration.  The
    /// producer is `Cx.boxed_edges`; declaration lowering carries the exact
    /// owner-local keys to MIR without recomputing them.
    pub boxed_edges: Vec<String>,
    pub kind: TirTypeDefKind,
}

#[derive(Debug, Clone)]
pub(super) struct TirSerdeAttribute {
    pub kind: TirSerdeAttributeKind,
    pub value: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TirSerdeAttributeKind {
    RenameAll,
    Tag,
    Untagged,
    DenyUnknownFields,
}

#[derive(Debug, Clone)]
pub(super) struct TirCliBinding {
    pub name: String,
    pub span: Span,
    pub function_key: String,
    pub markers: Vec<String>,
}

#[derive(Debug, Clone)]
pub(super) struct TirTraitDef {
    pub module: String,
    pub key: String,
    pub name: String,
    pub span: Span,
    pub visibility: TirVisibility,
    pub associated_types: Vec<TirAssociatedTypeDecl>,
    pub methods: Vec<TirTraitMethod>,
}

#[derive(Debug, Clone)]
pub(super) struct TirAssociatedTypeDecl {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub(super) struct TirAssociatedTypeValue {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub(super) struct TirTraitMethod {
    pub key: String,
    pub name: String,
    pub span: Span,
    /// The checked receiver convention, if this signature declares `self`.
    /// Implementations consume the same fact; adapters must not rediscover it
    /// from parameter names.
    pub self_access: Option<TirAccess>,
    /// Trait method parameters exclude the receiver.  The ordinary default
    /// function row retains the complete declared parameter contract.
    pub params: Vec<TirParam>,
    pub declared_return: Option<Type>,
    pub return_type: Type,
    pub failure: super::TFailureCarrier,
    pub is_pure: bool,
    pub declared_effects: Option<Vec<(String, Span)>>,
    pub return_view_provenance: Option<crate::AST::ViewProvenanceMap>,
    pub declared_return_view_provenance: Option<crate::AST::ViewProvenanceMap>,
    pub default_function_key: Option<String>,
}

#[derive(Debug, Clone)]
pub(super) struct TirImplDef {
    pub module: String,
    pub key: String,
    pub span: Span,
    pub self_type: Type,
    pub trait_name: Option<String>,
    pub trait_ref: Option<String>,
    pub associated_types: Vec<TirAssociatedTypeValue>,
    pub methods: Vec<String>,
    pub delegation_field: Option<String>,
    pub compiler_generated: bool,
    pub serde: Option<TirSerdeCodec>,
    pub operator_rhs: Option<Type>,
    pub operator_marker: Option<crate::AST::OperatorMarker>,
    pub target_os: Option<crate::OSTarget::OSTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TirSerdeCodec {
    Encode,
    Decode,
}

#[derive(Debug, Clone)]
pub(super) struct TirConstantDef {
    pub module: String,
    pub key: String,
    pub name: String,
    pub span: Span,
    pub visibility: TirVisibility,
    pub ty: Type,
    pub value: CtValue,
}

#[derive(Debug, Clone)]
pub(super) enum TirTypeDefKind {
    Struct {
        fields: Vec<TirField>,
        methods: Vec<String>,
    },
    Enum {
        variants: Vec<TirVariant>,
        methods: Vec<String>,
    },
    Distinct {
        base: Type,
        range: Option<(i64, i64)>,
    },
    Alias {
        target: Type,
    },
    UnitFamily {
        members: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub(super) struct TirField {
    pub name: String,
    /// Canonical per-projection names from the sema ShapeFact.
    pub shape_names: ShapeFieldNames,
    /// True when the field is absent from the checked wire shape.
    pub skip: bool,
    pub ty: Type,
    pub span: Span,
    pub public: bool,
    pub package_public: bool,
    pub computed: bool,
    pub has_default: bool,
}

#[derive(Debug, Clone)]
pub(super) struct TirVariant {
    pub name: String,
    /// Checked Codable wire name; adapters must not derive it from `name`.
    pub wire_name: String,
    pub span: Span,
    pub payload: TirVariantPayload,
    pub discriminant: Option<i64>,
}

#[derive(Debug, Clone)]
pub(super) enum TirVariantPayload {
    Unit,
    Single(Type),
    Named(Vec<TirField>),
}

/// Checked parameter metadata for foreign declarations.  The same shape is
/// useful to the MIR owner for signature projection without retaining AST
/// `Param` nodes.
#[derive(Debug, Clone)]
pub(super) struct TirParam {
    pub index: usize,
    pub name: String,
    pub span: Span,
    pub ty: Type,
    pub access: TirAccess,
    pub ownership: TirOwnership,
    pub public_label: String,
    pub variadic: bool,
    pub default_present: bool,
}

/// Project one checked source type into the canonical ABI-bearing type carrier.
/// The semantic type key is deterministic and remains present on every MIR
pub(super) fn lower_type(ty: &Type) -> MirType {
    let lowered = {
        let kind = lower_type_kind(ty);
        MirType::from_kind(kind).with_identity(MirTypeId(stable_id(
            "mir-type",
            &super::mir::type_identity_key(ty),
        )))
    };
    if CanonicalPass::enabled() {
        CanonicalPass::record(
            "lowering",
            "mir.lower-type",
            "crates/jet-codegen/src/Codegen/TIR/tir_to_mir_types.rs",
            "tir",
            CanonicalPass::debug_payload("tir", "mir.lower-type", ty),
            CanonicalPass::debug_identity("tir", "mir.lower-type", ty),
            "mir",
            CanonicalPass::debug_payload("mir", "mir.lower-type", &lowered),
            CanonicalPass::debug_identity("mir", "mir.lower-type", &lowered),
            "preserve",
        );
    }
    lowered
}

fn lower_type_kind(ty: &Type) -> jet_foundation::MIR::MirTypeKind {
    use jet_foundation::MIR::MirTypeKind;

    match ty {
        Type::Int => MirTypeKind::Int,
        Type::Float => MirTypeKind::Float,
        Type::Bool => MirTypeKind::Bool,
        Type::String => MirTypeKind::String,
        Type::Char => MirTypeKind::Char,
        Type::List(inner) => MirTypeKind::List(Box::new(lower_type(inner))),
        Type::Map { key, value, .. } => MirTypeKind::Map {
            key: Box::new(lower_type(key)),
            value: Box::new(lower_type(value)),
        },
        Type::Shared(inner) => MirTypeKind::Shared(Box::new(lower_type(inner))),
        Type::Option(inner) => MirTypeKind::Option(Box::new(lower_type(inner))),
        Type::Result { ok, err } => MirTypeKind::Result {
            ok: Box::new(lower_type(ok)),
            err: Box::new(lower_type(err)),
        },
        Type::Fn {
            params,
            ret,
            effect_bound,
            param_contract,
            call_metadata,
            return_view_provenance,
        } => MirTypeKind::Fn(MirFunctionSignature {
            params: params.iter().map(lower_type).collect(),
            ret: ret.as_ref().map(|ty| Box::new(lower_type(ty))),
            effect_bound: effect_bound
                .as_ref()
                .map(|row| row.iter().map(|(name, _)| name.clone()).collect()),
            param_contract: param_contract.as_ref().map(|rows| {
                rows.iter()
                    .map(|(label, zone)| MirCallContractRow {
                        label: label.clone(),
                        zone: lower_param_zone(*zone),
                    })
                    .collect()
            }),
            call_metadata: call_metadata.as_ref().map(lower_call_metadata),
            return_view_provenance: return_view_provenance.as_ref().map(lower_view_provenance),
        }),
        Type::Named(name) => MirTypeKind::Apply {
            name: MirNominalRef {
                id: MirTypeId(stable_id("mir-type", name)),
                name: name.clone(),
            },
            args: Vec::new(),
        },
        Type::Apply { name, args } => MirTypeKind::Apply {
            name: MirNominalRef {
                id: MirTypeId(stable_id("mir-type", name)),
                name: name.clone(),
            },
            args: args.iter().map(lower_type).collect(),
        },
        Type::TraitObject(bounds) => MirTypeKind::TraitObject(
            bounds
                .iter()
                .map(|name| MirNominalRef {
                    id: MirTypeId(stable_id("mir-trait", name)),
                    name: name.clone(),
                })
                .collect(),
        ),
        Type::Tuple(fields) => MirTypeKind::Tuple(
            fields
                .iter()
                .map(|(name, ty)| (name.clone(), lower_type(ty)))
                .collect(),
        ),
        Type::FixedList { elem, len } => MirTypeKind::FixedList {
            elem: Box::new(lower_type(elem)),
            len: lower_measure(len),
        },
        Type::IntN { signed, bits } => MirTypeKind::IntN {
            signed: *signed,
            bits: *bits,
        },
        Type::InlineRange { base, lo, hi } => MirTypeKind::InlineRange {
            base: Box::new(lower_type(base)),
            lo: *lo,
            hi: *hi,
        },
        Type::Float32 => MirTypeKind::Float32,
        Type::Tagged { marker, inner } => MirTypeKind::Tagged {
            marker: lower_tag_marker(marker),
            inner: Box::new(lower_type(inner)),
        },
        Type::Union(members) => MirTypeKind::Union(members.iter().map(lower_type).collect()),
        Type::Quantity { base, dimension } => MirTypeKind::Quantity {
            base: Box::new(lower_type(base)),
            dimension: lower_dimension(dimension),
        },
        Type::Measure(measure) => MirTypeKind::Measure(lower_measure(measure)),
    }
}

pub(super) fn lower_mir_access(access: TirAccess) -> MirAccess {
    match access {
        TirAccess::Read => MirAccess::Read,
        TirAccess::Write => MirAccess::Write,
        TirAccess::Move => MirAccess::Move,
    }
}

pub(super) fn lower_mir_convention(access: AccessConvention) -> MirAccess {
    match access {
        AccessConvention::Read => MirAccess::Read,
        AccessConvention::Write => MirAccess::Write,
        AccessConvention::Move => MirAccess::Move,
    }
}

pub(super) fn mir_access(access: TirAccess) -> MirAccess {
    lower_mir_access(access)
}

pub(super) fn lower_param_zone(zone: ParamZone) -> MirParamZone {
    match zone {
        ParamZone::PositionalOnly => MirParamZone::PositionalOnly,
        ParamZone::Either => MirParamZone::Either,
        ParamZone::LabelOnly => MirParamZone::LabelOnly,
    }
}

pub(super) fn lower_view_provenance(
    provenance: &ViewProvenanceMap,
) -> BTreeMap<Vec<String>, MirViewProvenance> {
    provenance
        .iter()
        .map(|(path, view)| (path.clone(), lower_view_slot(view)))
        .collect()
}

fn lower_view_slot(view: &crate::AST::ViewProvenance) -> MirViewProvenance {
    MirViewProvenance {
        sources: view.sources.iter().map(lower_view_source_path).collect(),
        mutable: view.mutable,
    }
}

fn lower_view_source_path(path: &crate::AST::ViewSourcePath) -> MirViewSourcePath {
    MirViewSourcePath {
        source: match &path.source {
            ViewSource::Receiver => MirViewSource::Receiver,
            ViewSource::Parameter(index) => MirViewSource::Parameter(*index),
            ViewSource::Static { module_path, name } => MirViewSource::Static {
                module_path: module_path.clone(),
                name: name.clone(),
            },
        },
        projections: path
            .projections
            .iter()
            .map(|projection| match projection {
                ViewSourceProjection::Field(name) => MirViewProjection::Field(name.clone()),
                ViewSourceProjection::Index => MirViewProjection::Index,
                ViewSourceProjection::Range => MirViewProjection::Range,
            })
            .collect(),
    }
}

fn lower_call_metadata(metadata: &FunctionCallMetadata) -> MirCallMetadata {
    MirCallMetadata {
        names: metadata.names.clone(),
        defaults: metadata.defaults.iter().map(Option::is_some).collect(),
        variadic: metadata.variadic.clone(),
        conventions: metadata
            .conventions
            .iter()
            .copied()
            .map(lower_access_convention)
            .map(lower_mir_access)
            .collect(),
        policies: MirCallablePolicyChain {
            policies: metadata
                .policies
                .policies
                .iter()
                .map(|policy| MirCallablePolicy {
                    name: policy.name.clone(),
                    arguments: policy.arguments.clone(),
                })
                .collect(),
        },
    }
}

fn lower_measure(measure: &Measure) -> MirMeasure {
    match measure {
        Measure::Literal { kind, value } => MirMeasure::Literal {
            kind: kind.clone(),
            value: *value,
        },
        Measure::SignedLiteral { kind, value } => MirMeasure::SignedLiteral {
            kind: kind.clone(),
            value: *value,
        },
        Measure::Symbol { kind, name } => MirMeasure::Symbol {
            kind: kind.clone(),
            name: name.clone(),
        },
        Measure::Combined {
            kind,
            rule,
            left,
            right,
        } => MirMeasure::Combined {
            kind: kind.clone(),
            rule: match rule {
                MeasureRule::Add => MirMeasureRule::Add,
                MeasureRule::Mul => MirMeasureRule::Mul,
                MeasureRule::Match => MirMeasureRule::Match,
            },
            left: Box::new(lower_measure(left)),
            right: Box::new(lower_measure(right)),
        },
    }
}

fn lower_dimension(dimension: &Dimension) -> MirDimension {
    MirDimension {
        axes: dimension
            .measure_exponents()
            .map(|(axis, exponent)| (axis.to_string(), lower_measure(&exponent)))
            .collect(),
    }
}

fn lower_tag_marker(marker: &TagMarker) -> MirTagMarker {
    match marker {
        TagMarker::User(name) => MirTagMarker::User(name.clone()),
        TagMarker::Internal(tag) => MirTagMarker::Internal(match tag {
            InternalTag::CoreCryptoNominal => MirInternalTag::CoreCryptoNominal,
            InternalTag::DeterministicClock => MirInternalTag::DeterministicClock,
            InternalTag::SystemClock => MirInternalTag::SystemClock,
            InternalTag::ExpiringSecretLoan => MirInternalTag::ExpiringSecretLoan,
            InternalTag::SharedGuardRead => MirInternalTag::SharedGuardRead,
            InternalTag::SharedGuardEdit => MirInternalTag::SharedGuardEdit,
            InternalTag::TerminalFactSet => MirInternalTag::TerminalFactSet,
            InternalTag::CppCallbackAbi => MirInternalTag::CppCallbackAbi,
            InternalTag::AllocatorView => MirInternalTag::AllocatorView,
        }),
    }
}

fn lower_access_convention(convention: AccessConvention) -> TirAccess {
    match convention {
        AccessConvention::Read => TirAccess::Read,
        AccessConvention::Write => TirAccess::Write,
        AccessConvention::Move => TirAccess::Move,
    }
}

fn ownership_mode(ownership: TirOwnership) -> MirOwnershipMode {
    match ownership {
        TirOwnership::Owned => MirOwnershipMode::Owned,
        TirOwnership::ReadBorrow => MirOwnershipMode::ReadBorrow,
        TirOwnership::WriteBorrow => MirOwnershipMode::WriteBorrow,
        TirOwnership::Move => MirOwnershipMode::Move,
    }
}

fn ownership_from_tir(ownership: TirOwnership) -> MirOwnership {
    match ownership {
        TirOwnership::Owned => MirOwnership::Owned,
        TirOwnership::ReadBorrow => MirOwnership {
            mode: MirOwnershipMode::ReadBorrow,
            drop: MirDropKind::None,
            moved: false,
            last_use: false,
            gc_root: false,
        },
        TirOwnership::WriteBorrow => MirOwnership {
            mode: MirOwnershipMode::WriteBorrow,
            drop: MirDropKind::None,
            moved: false,
            last_use: false,
            gc_root: false,
        },
        TirOwnership::Move => MirOwnership {
            mode: MirOwnershipMode::Move,
            drop: MirDropKind::Value,
            moved: false,
            last_use: false,
            gc_root: false,
        },
    }
}

fn ownership_for_definition(single_use: bool) -> TirOwnership {
    if single_use {
        TirOwnership::Move
    } else {
        TirOwnership::Owned
    }
}

fn qualified_key(module: &str, name: &str) -> String {
    if module.is_empty() {
        name.to_string()
    } else {
        format!("{module}::{name}")
    }
}

fn is_auto_printable(auto_printable: &HashSet<String>, module: &str, name: &str) -> bool {
    auto_printable.contains(name) || auto_printable.contains(&qualified_key(module, name))
}

fn is_auto_debug(auto_debug: &HashSet<String>, module: &str, name: &str) -> bool {
    auto_debug.contains(name) || auto_debug.contains(&qualified_key(module, name))
}

fn lower_generic_params(params: &[crate::AST::TypeParam]) -> Vec<super::TGenericParam> {
    params
        .iter()
        .map(|param| super::TGenericParam {
            name: param.name.clone(),
            bounds: param.bounds.clone(),
        })
        .collect()
}

fn lower_derive_names(derives: &[(String, Span)], auto_debug: bool) -> Vec<String> {
    let mut names = derives
        .iter()
        .filter(|(name, _)| name != jet_foundation::Syntax::MARKER_CLI)
        .map(|(name, _)| name.clone())
        .collect::<Vec<_>>();
    if auto_debug && !names.iter().any(|name| name == crate::Generics::DEBUG) {
        names.push(crate::Generics::DEBUG.to_string());
    }
    names
}

fn marker_arg_text(marker: &Marker) -> Option<String> {
    marker
        .args
        .first()
        .and_then(|arg| arg.as_expr())
        .and_then(|expr| match expr {
            Expr::Ident(name, _) => Some(name.clone()),
            _ => None,
        })
}

fn lower_serde_markers(markers: &[Marker]) -> Vec<TirSerdeAttribute> {
    markers
        .iter()
        .filter_map(|marker| {
            let kind = match marker.name.as_str() {
                "RenameAll" => TirSerdeAttributeKind::RenameAll,
                "Discriminant" | "Tag" => TirSerdeAttributeKind::Tag,
                "Untagged" => TirSerdeAttributeKind::Untagged,
                "DenyUnknownFields" => TirSerdeAttributeKind::DenyUnknownFields,
                _ => return None,
            };
            Some(TirSerdeAttribute {
                kind,
                value: marker_arg_text(marker),
                span: marker.span,
            })
        })
        .collect()
}

fn field_is_skipped(field: &Field) -> bool {
    field
        .serde_markers
        .iter()
        .any(|marker| marker.name == "Skip" || marker.name == crate::Syntax::MARKER_SKIP)
}

fn function_name_from_expr(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Ident(name, _) => Some(name.clone()),
        _ => None,
    }
}

fn lower_cli_bindings(bindings: &[CLICommandBinding], module: &str) -> Vec<TirCliBinding> {
    bindings
        .iter()
        .map(|binding| TirCliBinding {
            name: binding.name.clone(),
            span: binding.name_span,
            function_key: function_name_from_expr(&binding.target)
                .map(|name| qualified_key(module, &name))
                .unwrap_or_else(|| qualified_key(module, &binding.name)),
            markers: binding
                .markers
                .iter()
                .map(|marker| marker.name.clone())
                .collect(),
        })
        .collect()
}

fn lower_field(
    field: &Field,
    shape_names: ShapeFieldNames,
    qualify_type: &impl Fn(&Type, &[String]) -> Type,
    binders: &[String],
) -> TirField {
    TirField {
        name: field.name.clone(),
        shape_names,
        skip: field_is_skipped(field),
        ty: qualify_type(&field.ty, binders),
        span: field.name_span,
        public: field.is_pub,
        package_public: field.is_package_pub,
        computed: field.computed.is_some(),
        has_default: field.default.is_some(),
    }
}

fn container_rename_all(markers: &[Marker]) -> Option<String> {
    crate::Codegen::Items::container_rename_all(markers)
}

fn lower_variant_field(
    field: &crate::AST::VariantField,
    qualify_type: &impl Fn(&Type, &[String]) -> Type,
    binders: &[String],
) -> TirField {
    TirField {
        name: field.name.clone(),
        shape_names: ShapeFieldNames::from_source(&field.name),
        skip: false,
        ty: qualify_type(&field.ty, binders),
        span: field.name_span,
        public: true,
        package_public: false,
        computed: false,
        has_default: false,
    }
}

fn lower_variant(
    variant: &Variant,
    style: Option<&str>,
    qualify_type: &impl Fn(&Type, &[String]) -> Type,
    binders: &[String],
) -> TirVariant {
    let wire_name = crate::Codegen::Items::variant_wire_key(style, variant);
    TirVariant {
        name: variant.name.clone(),
        wire_name,
        span: variant.name_span,
        payload: match &variant.payload {
            VariantPayload::Unit => TirVariantPayload::Unit,
            VariantPayload::Single(ty, _) => {
                TirVariantPayload::Single(qualify_type(ty, binders))
            }
            VariantPayload::Named(fields) => TirVariantPayload::Named(
                fields
                    .iter()
                    .map(|field| lower_variant_field(field, qualify_type, binders))
                    .collect(),
            ),
        },
        discriminant: variant.discriminant,
    }
}

fn owner_boxed_edges(owner: &str, boxed_edges: &HashSet<(String, String)>) -> Vec<String> {
    let mut keys = boxed_edges
        .iter()
        .filter(|(candidate, _)| candidate == owner)
        .map(|(_, key)| key.clone())
        .collect::<Vec<_>>();
    keys.sort_unstable();
    keys
}

fn type_names_owner(ty: &Type, owner_key: &str, owner_name: &str) -> bool {
    match ty {
        Type::Named(name) => name == owner_key || name == owner_name,
        Type::Apply { name, args } if args.is_empty() => name == owner_key || name == owner_name,
        _ => false,
    }
}

fn lower_type_for_owner(ty: &Type, owner_key: &str, owner_name: &str) -> MirType {
    let lowered = lower_type(ty);
    if type_names_owner(ty, owner_key, owner_name) {
        lowered.with_identity(MirTypeId(stable_id("mir-type", owner_key)))
    } else {
        lowered
    }
}

fn lower_mir_field(owner: &str, field: &TirField) -> MirField {
    lower_mir_field_for_owner(owner, field, None, None)
}

fn lower_mir_field_for_owner(
    owner: &str,
    field: &TirField,
    identity_key: Option<&str>,
    identity_name: Option<&str>,
) -> MirField {
    let ty = match (identity_key, identity_name) {
        (Some(key), Some(name)) => lower_type_for_owner(&field.ty, key, name),
        _ => lower_type(&field.ty),
    };
    MirField {
        id: MirFieldId(stable_id("mir-field", &format!("{owner}::{}", field.name))),
        name: field.name.clone(),
        shape_names: field.shape_names.clone(),
        skip: field.skip,
        ty,
        span: field.span,
        public: field.public,
        package_public: field.package_public,
        computed: field.computed,
        has_default: field.has_default,
    }
}

fn lower_type_cli_entry(entry: &super::artifact_plan::TirCliEntry) -> MirCliEntry {
    MirCliEntry {
        description: entry.description.clone(),
        inputs: entry
            .inputs
            .iter()
            .enumerate()
            .map(|(ordinal, input)| super::mir::lower_cli_input(input, ordinal, None))
            .collect(),
        commands: entry
            .commands
            .iter()
            .filter_map(|command| {
                let function = command.function.as_ref()?;
                Some(MirCliCommand {
                    name: command.name.clone(),
                    description: command.description.clone(),
                    function: MirFunctionId(stable_id("mir-function", &function.key)),
                    receiver: command
                        .receiver
                        .as_ref()
                        .map(|receiver| MirTypeId(stable_id("mir-type", receiver))),
                    inputs: command
                        .inputs
                        .iter()
                        .enumerate()
                        .map(|(ordinal, input)| super::mir::lower_cli_input(input, ordinal, None))
                        .collect(),
                })
            })
            .collect(),
        standard: entry.standard,
        version: entry.version.clone(),
    }
}

pub(super) fn lower_type_defs(
    definitions: &[TirTypeDef],
) -> Result<Vec<MirTypeDef>, super::LowerError> {
    let mut seen = std::collections::HashSet::with_capacity(definitions.len());
    let mut rows = Vec::with_capacity(definitions.len());
    for definition in definitions {
        if !seen.insert(definition.key.clone()) {
            return Err(super::LowerError::new(
                definition.span,
                format!("duplicate checked type definition `{}`", definition.key),
            ));
        }
        rows.push(definition.to_mir()?);
    }
    Ok(rows)
}

impl TirTypeDef {
    pub(super) fn to_mir(&self) -> Result<MirTypeDef, super::LowerError> {
        let kind = match &self.kind {
            TirTypeDefKind::Struct { fields, methods } => MirTypeDefKind::Struct {
                fields: fields
                    .iter()
                    .map(|field| lower_mir_field(&self.key, field))
                    .collect(),
                methods: methods
                    .iter()
                    .map(|key| MirFunctionId(stable_id("mir-function", key)))
                    .collect(),
            },
            TirTypeDefKind::Enum { variants, methods } => MirTypeDefKind::Enum {
                variants: variants
                    .iter()
                    .map(|variant| MirVariant {
                        name: variant.name.clone(),
                        wire_name: variant.wire_name.clone(),
                        span: variant.span,
                        payload: match &variant.payload {
                            TirVariantPayload::Unit => MirVariantPayload::Unit,
                            TirVariantPayload::Single(ty) => MirVariantPayload::Single(
                                lower_type_for_owner(ty, &self.key, &self.name),
                            ),
                            TirVariantPayload::Named(fields) => MirVariantPayload::Named(
                                fields
                                    .iter()
                                    .map(|field| {
                                        lower_mir_field_for_owner(
                                            &format!("{}::{}", self.key, variant.name),
                                            field,
                                            Some(&self.key),
                                            Some(&self.name),
                                        )
                                    })
                                    .collect(),
                            ),
                        },
                        discriminant: variant.discriminant,
                    })
                    .collect(),
                methods: methods
                    .iter()
                    .map(|key| MirFunctionId(stable_id("mir-function", key)))
                    .collect(),
            },
            TirTypeDefKind::Distinct { base, range } => MirTypeDefKind::Distinct {
                base: lower_type(base),
                range: *range,
            },
            TirTypeDefKind::Alias { target } => MirTypeDefKind::Alias {
                target: lower_type(target),
            },
            TirTypeDefKind::UnitFamily { members } => MirTypeDefKind::UnitFamily {
                members: members.clone(),
            },
        };
        Ok(MirTypeDef {
            id: MirTypeId(stable_id("mir-type", &self.key)),
            module: MirModuleId(stable_id("mir-module", &self.module)),
            key: self.key.clone(),
            name: self.name.clone(),
            span: self.span,
            public: self.public,
            package_public: self.package_public,
            generic_params: self
                .generic_params
                .iter()
                .map(|parameter| MirGenericParam {
                    name: parameter.name.clone(),
                    bounds: parameter
                        .bounds
                        .iter()
                        .map(|bound| MirTraitRef {
                            id: MirTraitId(stable_id("mir-trait", bound)),
                            name: bound.clone(),
                        })
                        .collect(),
                })
                .collect(),
            derives: self
                .derives
                .iter()
                .map(|name| MirTraitId(stable_id("mir-trait", name)))
                .collect(),
            auto_derive_default: self.auto_derive_default,
            auto_printable: self.auto_printable,
            published_schema: self.published_schema,
            single_use: self.single_use,
            must_use: self.must_use,
            layout: self.layout.as_ref().map(|layout| match layout {
                StructLayout::C => MirStructLayout::C,
                StructLayout::CAligned { alignment, target } => MirStructLayout::CAligned {
                    alignment: *alignment,
                    target: *target,
                },
                StructLayout::Columnar => MirStructLayout::Columnar,
            }),
            layout_alignment: self.layout_alignment.clone(),
            serde: self
                .serde
                .iter()
                .map(|attribute| MirSerdeAttribute {
                    kind: match attribute.kind {
                        TirSerdeAttributeKind::RenameAll => MirSerdeAttributeKind::RenameAll,
                        TirSerdeAttributeKind::Tag => MirSerdeAttributeKind::Tag,
                        TirSerdeAttributeKind::Untagged => MirSerdeAttributeKind::Untagged,
                        TirSerdeAttributeKind::DenyUnknownFields => {
                            MirSerdeAttributeKind::DenyUnknownFields
                        }
                    },
                    value: attribute.value.clone(),
                    span: attribute.span,
                })
                .collect(),
            cli_bindings: self
                .cli_bindings
                .iter()
                .map(|binding| MirCliBinding {
                    name: binding.name.clone(),
                    function: MirFunctionId(stable_id("mir-function", &binding.function_key)),
                    span: binding.span,
                    markers: binding.markers.clone(),
                })
                .collect(),
            cli: self.cli.as_ref().map(lower_type_cli_entry),
            ownership: ownership_mode(self.ownership),
            boxed_edges: self.boxed_edges.clone(),
            kind,
        })
    }
}

impl TirParam {
    pub(super) fn to_mir(&self) -> MirParam {
        MirParam {
            index: self.index,
            name: self.name.clone(),
            span: self.span,
            ty: lower_type(&self.ty),
            access: lower_mir_access(self.access),
            ownership: ownership_from_tir(self.ownership),
            public_label: self.public_label.clone(),
            variadic: self.variadic,
            default_present: self.default_present,
        }
    }
}

impl TirErasure {
    pub(super) fn to_mir(&self) -> jet_foundation::MIR::MirUnreachable {
        jet_foundation::MIR::MirUnreachable {
            construct: self.construct.clone(),
            span: self.span,
            reason: match self.reason {
                TirErasureReason::CompileTimeOnly => MirErasureReason::CompileTimeOnly,
                TirErasureReason::Disabled => MirErasureReason::Disabled,
                TirErasureReason::ExpandedBeforeLowering => {
                    MirErasureReason::ExpandedBeforeLowering
                }
                TirErasureReason::SemanticIndexOnly => MirErasureReason::SemanticIndexOnly,
            },
        }
    }
}

pub(super) fn lower_erasure(
    construct: impl Into<String>,
    span: Span,
    reason: TirErasureReason,
) -> TirErasure {
    TirErasure {
        construct: construct.into(),
        span,
        reason,
    }
}

pub(super) fn lower_tir_declarations(
    bundle: &ProgramBundle,
    boxed_edges_by_module: &BTreeMap<String, HashSet<(String, String)>>,
    auto_printable_by_module: &BTreeMap<String, HashSet<String>>,
    auto_debug_by_module: &BTreeMap<String, HashSet<String>>,
) -> TirDeclarations {
    let mut out = TirDeclarations::default();
    let empty_boxed_edges = HashSet::new();
    let empty_auto_printable = HashSet::new();
    let empty_auto_debug = HashSet::new();
    for (index, module) in bundle.modules.iter().enumerate() {
        let module_name = crate::Codegen::Context::module_identity(bundle, index);
        let boxed_edges = boxed_edges_by_module
            .get(&module_name)
            .unwrap_or(&empty_boxed_edges);
        let auto_printable = auto_printable_by_module
            .get(&module_name)
            .unwrap_or(&empty_auto_printable);
        let auto_debug = auto_debug_by_module
            .get(&module_name)
            .unwrap_or(&empty_auto_debug);
        let seed_parse_error =
            !super::module_owned_type_names(&module.items).contains("ParseError");
        collect_items(
            &mut out,
            &module.items,
            &module_name,
            boxed_edges,
            auto_printable,
            auto_debug,
            &|ty, binders| {
                super::qualify_imported_type(bundle, index, &module_name, binders, ty)
            },
            seed_parse_error,
        );
    }
    ensure_builtin_debug_trait(&mut out);
    out
}

pub(super) fn lower_declarations_from_items(items: &[Item], module: &str) -> TirDeclarations {
    lower_declarations_from_items_with_boxed_edges(
        items,
        module,
        &HashSet::new(),
        &HashSet::new(),
        &HashSet::new(),
    )
}

pub(super) fn lower_declarations_from_items_with_boxed_edges(
    items: &[Item],
    module: &str,
    boxed_edges: &HashSet<(String, String)>,
    auto_printable: &HashSet<String>,
    auto_debug: &HashSet<String>,
) -> TirDeclarations {
    let mut out = TirDeclarations::default();
    collect_items(
        &mut out,
        items,
        module,
        boxed_edges,
        auto_printable,
        auto_debug,
        &|ty, _| ty.clone(),
        !super::module_owned_type_names(items).contains("ParseError"),
    );
    ensure_builtin_debug_trait(&mut out);
    out
}

fn collect_items(
    out: &mut TirDeclarations,
    items: &[Item],
    module: &str,
    boxed_edges: &HashSet<(String, String)>,
    auto_printable: &HashSet<String>,
    auto_debug: &HashSet<String>,
    qualify_type: &impl Fn(&Type, &[String]) -> Type,
    seed_parse_error: bool,
) {
    let layout_engine = TargetLayoutEngine::host(items);
    for item in items {
        match item {
            Item::Struct(definition) => {
                out.type_defs.push(lower_struct(
                    definition,
                    module,
                    qualify_type,
                    &layout_engine,
                    boxed_edges,
                    is_auto_printable(auto_printable, module, &definition.name),
                    is_auto_debug(auto_debug, module, &definition.name),
                ));
                push_nested_trait_impls(out, &definition.name, &definition.trait_impls, module);
            }
            Item::Enum(definition) => {
                out.type_defs.push(lower_enum(
                    definition,
                    module,
                    qualify_type,
                    boxed_edges,
                    is_auto_printable(auto_printable, module, &definition.name),
                    is_auto_debug(auto_debug, module, &definition.name),
                ));
                push_nested_trait_impls(out, &definition.name, &definition.trait_impls, module);
            }
            Item::Distinct(definition) => out.type_defs.push(lower_distinct(
                definition,
                module,
                qualify_type,
                is_auto_printable(auto_printable, module, &definition.name),
            )),
            Item::TypeAlias(definition) => out.type_defs.push(lower_alias(
                definition,
                module,
                qualify_type,
                is_auto_printable(auto_printable, module, &definition.name),
            )),
            Item::UnitFamily(definition) => {
                out.type_defs.push(lower_unit_family(
                    definition,
                    module,
                    is_auto_printable(auto_printable, module, &definition.family),
                ));
                for member in definition.distinct_defs() {
                    out.type_defs.push(lower_distinct(
                        &member,
                        module,
                        qualify_type,
                        is_auto_printable(auto_printable, module, &member.name),
                    ));
                }
            }
            Item::Trait(definition) => {
                out.traits
                    .push(lower_trait(definition, module, qualify_type))
            }
            Item::Impl(definition) => out.impls.push(lower_impl(definition, module)),
            Item::Const(definition) => out.constants.push(lower_constant(definition, module)),
            _ => {}
        }
    }
    for row in compiler_owned_type_defs(module, seed_parse_error) {
        if !out.type_defs.iter().any(|existing| existing.key == row.key) {
            out.type_defs.push(row);
        }
    }
}

fn ensure_builtin_debug_trait(out: &mut TirDeclarations) {
    let Some(module) = out
        .type_defs
        .iter()
        .find(|definition| {
            definition
                .derives
                .iter()
                .any(|name| name == crate::Generics::DEBUG)
        })
        .map(|definition| definition.module.clone())
    else {
        return;
    };
    if out
        .traits
        .iter()
        .any(|definition| definition.name == crate::Generics::DEBUG)
    {
        return;
    }
    out.traits.push(TirTraitDef {
        module,
        key: crate::Generics::DEBUG.to_string(),
        name: crate::Generics::DEBUG.to_string(),
        span: Span::new(0, 0),
        visibility: TirVisibility::Public,
        associated_types: Vec::new(),
        methods: Vec::new(),
    });
}

// TIR rewrites every method implementation to a module-qualified semantic key.
// Trait methods retain the trait identity; operator methods retain their concrete
// RHS identity, including the owner type for a bare same-type implementation.
fn method_keys(
    module: &str,
    type_name: &str,
    trait_name: Option<&str>,
    operator_rhs: Option<&Type>,
    methods: &[Func],
) -> Vec<String> {
    methods
        .iter()
        .map(|method| {
            let name = match trait_name {
                Some(trait_name)
                    if matches!(
                        trait_name,
                        crate::Syntax::TRAIT_ADD
                            | crate::Syntax::TRAIT_SUB
                            | crate::Syntax::TRAIT_MUL
                            | crate::Syntax::TRAIT_DIV
                            | crate::Syntax::TRAIT_EQUATABLE
                            | crate::Syntax::TRAIT_COMPARABLE
                    ) =>
                {
                    crate::Traits::operator_method_identity(
                        type_name,
                        trait_name,
                        &method.name,
                        operator_rhs.unwrap_or(&Type::Named(type_name.to_string())),
                    )
                }
                Some(trait_name) => {
                    format!("{type_name}::{trait_name}::{}", method.name)
                }
                None => format!("{type_name}::{}", method.name),
            };
            qualified_key(module, &name)
        })
        .collect()
}

fn lower_struct(
    definition: &StructDef,
    module: &str,
    qualify_type: &impl Fn(&Type, &[String]) -> Type,
    layout_engine: &TargetLayoutEngine<'_>,
    boxed_edges: &HashSet<(String, String)>,
    auto_printable: bool,
    auto_debug: bool,
) -> TirTypeDef {
    let layout_alignment = layout_engine
        .checked_struct_facts(definition)
        .ok()
        .and_then(|facts| facts.alignment);
    let shape_fact = crate::Sema::Schema::shape_fact_from_struct(module, definition).ok();
    let binders = definition
        .type_params
        .iter()
        .map(|param| param.name.clone())
        .collect::<Vec<_>>();
    TirTypeDef {
        module: module.to_string(),
        key: qualified_key(module, &definition.name),
        name: definition.name.clone(),
        span: definition.span,
        public: definition.is_pub,
        package_public: definition.is_package_pub,
        generic_params: lower_generic_params(&definition.type_params),
        derives: lower_derive_names(&definition.derives, auto_debug),
        auto_derive_default: definition.auto_derive_default,
        auto_printable,
        published_schema: definition.is_published_schema,
        single_use: definition.is_single_use,
        must_use: definition.is_must_use,
        layout: definition.layout.clone(),
        layout_alignment,
        serde: lower_serde_markers(&definition.serde_markers),
        cli_bindings: lower_cli_bindings(&definition.cli_bindings, module),
        cli: None,
        ownership: ownership_for_definition(definition.is_single_use),
        boxed_edges: owner_boxed_edges(&definition.name, boxed_edges),
        kind: TirTypeDefKind::Struct {
            fields: definition
                .fields
                .iter()
                .map(|field| {
                    let shape_names = shape_fact
                        .as_ref()
                        .and_then(|fact| {
                            fact.fields
                                .iter()
                                .find(|candidate| candidate.name == field.name)
                                .map(|candidate| candidate.names.clone())
                        })
                        .unwrap_or_else(|| ShapeFieldNames::from_source(&field.name));
                    {
                        let mut field = field.clone();
                        let mut params = std::collections::HashMap::new();
                        params.insert(
                            definition.name.clone(),
                            definition
                                .type_params
                                .iter()
                                .map(|param| param.name.clone())
                                .collect(),
                        );
                        let owner = if definition.type_params.is_empty() {
                            Type::Named(definition.name.clone())
                        } else {
                            Type::Apply {
                                name: definition.name.clone(),
                                args: definition
                                    .type_params
                                    .iter()
                                    .map(|param| Type::Named(param.name.clone()))
                                    .collect(),
                            }
                        };
                        field.ty = super::substitute_reflect_field_type(&params, &owner, &field.ty);
                        lower_field(&field, shape_names, qualify_type, &binders)
                    }
                })
                .collect(),
            methods: method_keys(module, &definition.name, None, None, &definition.methods),
        },
    }
}

fn lower_enum(
    definition: &EnumDef,
    module: &str,
    qualify_type: &impl Fn(&Type, &[String]) -> Type,
    boxed_edges: &HashSet<(String, String)>,
    auto_printable: bool,
    auto_debug: bool,
) -> TirTypeDef {
    let style = container_rename_all(&definition.serde_markers);
    let binders = definition
        .type_params
        .iter()
        .map(|param| param.name.clone())
        .collect::<Vec<_>>();
    TirTypeDef {
        module: module.to_string(),
        key: qualified_key(module, &definition.name),
        name: definition.name.clone(),
        span: definition.span,
        public: definition.is_pub,
        package_public: definition.is_package_pub,
        generic_params: lower_generic_params(&definition.type_params),
        derives: lower_derive_names(&definition.derives, auto_debug),
        auto_derive_default: definition.auto_derive_default,
        auto_printable,
        published_schema: false,
        single_use: definition.is_single_use,
        must_use: definition.is_must_use,
        layout: None,
        layout_alignment: None,
        serde: lower_serde_markers(&definition.serde_markers),
        cli_bindings: Vec::new(),
        cli: None,
        ownership: ownership_for_definition(definition.is_single_use),
        boxed_edges: owner_boxed_edges(&definition.name, boxed_edges),
        kind: TirTypeDefKind::Enum {
            variants: definition
                .variants
                .iter()
                .map(|variant| {
                    lower_variant(variant, style.as_deref(), qualify_type, &binders)
                })
                .collect(),
            methods: method_keys(module, &definition.name, None, None, &definition.methods),
        },
    }
}

fn lower_distinct(
    definition: &DistinctDef,
    module: &str,
    qualify_type: &impl Fn(&Type, &[String]) -> Type,
    auto_printable: bool,
) -> TirTypeDef {
    TirTypeDef {
        module: module.to_string(),
        key: qualified_key(module, &definition.name),
        name: definition.name.clone(),
        span: definition.span,
        public: definition.is_pub,
        package_public: definition.is_package_pub,
        generic_params: Vec::new(),
        derives: lower_derive_names(&definition.derives, false),
        auto_derive_default: false,
        auto_printable,
        published_schema: false,
        single_use: false,
        must_use: false,
        layout: None,
        layout_alignment: None,
        serde: Vec::new(),
        cli_bindings: Vec::new(),
        cli: None,
        ownership: TirOwnership::Owned,
        boxed_edges: Vec::new(),
        kind: TirTypeDefKind::Distinct {
            base: qualify_type(&definition.base, &[]),
            range: definition.range.map(|(lo, hi, _)| (lo, hi)),
        },
    }
}

fn lower_alias(
    definition: &TypeAliasDef,
    module: &str,
    qualify_type: &impl Fn(&Type, &[String]) -> Type,
    auto_printable: bool,
) -> TirTypeDef {
    let binders = definition
        .type_params
        .iter()
        .map(|param| param.name.clone())
        .collect::<Vec<_>>();
    TirTypeDef {
        module: module.to_string(),
        key: qualified_key(module, &definition.name),
        name: definition.name.clone(),
        span: definition.span,
        public: definition.is_pub,
        package_public: definition.is_package_pub,
        generic_params: lower_generic_params(&definition.type_params),
        derives: Vec::new(),
        auto_derive_default: false,
        auto_printable,
        published_schema: false,
        single_use: false,
        must_use: false,
        layout: None,
        layout_alignment: None,
        serde: Vec::new(),
        cli_bindings: Vec::new(),
        cli: None,
        ownership: TirOwnership::Owned,
        boxed_edges: Vec::new(),
        kind: TirTypeDefKind::Alias {
            target: qualify_type(&definition.target, &binders),
        },
    }
}

fn lower_unit_family(definition: &UnitFamilyDef, module: &str, auto_printable: bool) -> TirTypeDef {
    TirTypeDef {
        module: module.to_string(),
        key: qualified_key(module, &definition.family),
        name: definition.family.clone(),
        span: definition.span,
        public: definition.is_pub,
        package_public: definition.is_package_pub,
        generic_params: Vec::new(),
        derives: Vec::new(),
        auto_derive_default: false,
        auto_printable,
        published_schema: false,
        single_use: false,
        must_use: false,
        layout: None,
        layout_alignment: None,
        serde: Vec::new(),
        cli_bindings: Vec::new(),
        cli: None,
        ownership: TirOwnership::Owned,
        boxed_edges: Vec::new(),
        kind: TirTypeDefKind::UnitFamily {
            members: definition
                .distinct_defs()
                .into_iter()
                .map(|member| member.name)
                .collect(),
        },
    }
}

fn lower_trait_param(
    index: usize,
    param: &Param,
    qualify_type: &impl Fn(&Type, &[String]) -> Type,
) -> TirParam {
    TirParam {
        index,
        name: param.name.clone(),
        span: param.name_span,
        ty: qualify_type(&param.ty, &[]),
        access: lower_access_convention(param.convention),
        ownership: match param.convention {
            AccessConvention::Move => TirOwnership::Move,
            AccessConvention::Write => TirOwnership::WriteBorrow,
            AccessConvention::Read => TirOwnership::ReadBorrow,
        },
        public_label: param
            .public_label
            .as_ref()
            .map(|(label, _)| label.clone())
            .unwrap_or_else(|| param.name.clone()),
        variadic: param.variadic,
        default_present: param.default.is_some(),
    }
}

fn lower_trait(
    definition: &TraitDef,
    module: &str,
    qualify_type: &impl Fn(&Type, &[String]) -> Type,
) -> TirTraitDef {
    TirTraitDef {
        module: module.to_string(),
        key: definition.name.clone(),
        name: definition.name.clone(),
        span: definition.span,
        visibility: if definition.is_pub {
            TirVisibility::Public
        } else if definition.is_package_pub {
            TirVisibility::Package
        } else {
            TirVisibility::Private
        },
        associated_types: definition
            .assoc_types
            .iter()
            .map(|(name, span)| TirAssociatedTypeDecl {
                name: name.clone(),
                span: *span,
            })
            .collect(),
        methods: definition
            .methods
            .iter()
            .map(|method| {
                let mut params = Vec::new();
                let mut self_access = None;
                for param in &method.params {
                    if params.is_empty() && self_access.is_none() && param.name == "self" {
                        self_access = Some(lower_access_convention(param.convention));
                        continue;
                    }
                    params.push(lower_trait_param(params.len(), param, qualify_type));
                }
                TirTraitMethod {
                    key: qualified_key(module, &method.name),
                    name: method.name.clone(),
                    span: method.span,
                    self_access,
                    params,
                    declared_return: method
                        .return_type
                        .as_ref()
                        .map(|ty| qualify_type(ty, &[])),
                    return_type: qualify_type(&method.effective_return_type(), &[]),
                    failure: super::TFailureCarrier::from_contract(&method.failure_contract()),
                    is_pure: method.is_pure,
                    declared_effects: method.declared_effects.clone(),
                    return_view_provenance: method.return_view_provenance.get(),
                    declared_return_view_provenance: method.declared_return_view_provenance.clone(),
                    default_function_key: method
                        .default_body
                        .as_ref()
                        .map(|_| qualified_key(module, &method.name)),
                }
            })
            .collect(),
    }
}

fn push_nested_trait_impls(
    out: &mut TirDeclarations,
    type_name: &str,
    blocks: &[TraitImplBlock],
    module: &str,
) {
    for block in blocks {
        out.impls
            .push(lower_nested_trait_impl(type_name, block, module));
    }
}

fn lower_nested_trait_impl(type_name: &str, block: &TraitImplBlock, module: &str) -> TirImplDef {
    TirImplDef {
        module: module.to_string(),
        key: format!("{}::{type_name}::{}", module, block.trait_name),
        span: block.trait_span,
        // Impl rows carry the same module-qualified nominal as the type
        // definition.  MIR can therefore match an imported owner by its
        // checked identity, never by a colliding leaf name.
        self_type: Type::Named(qualified_key(module, type_name)),
        trait_name: Some(block.trait_name.clone()),
        trait_ref: Some(block.trait_name.clone()),
        associated_types: block
            .assoc_type_impls
            .iter()
            .map(|(name, span, ty)| TirAssociatedTypeValue {
                name: name.clone(),
                ty: ty.clone(),
                span: *span,
            })
            .collect(),
        methods: method_keys(
            module,
            type_name,
            Some(&block.trait_name),
            block.operator_rhs.as_ref(),
            &block.methods,
        ),
        delegation_field: None,
        compiler_generated: block.compiler_generated,
        serde: match block.trait_name.as_str() {
            crate::Generics::ENCODE => Some(TirSerdeCodec::Encode),
            crate::Generics::DECODE => Some(TirSerdeCodec::Decode),
            _ => None,
        },
        operator_rhs: match block.trait_name.as_str() {
            crate::Syntax::TRAIT_ADD
            | crate::Syntax::TRAIT_SUB
            | crate::Syntax::TRAIT_MUL
            | crate::Syntax::TRAIT_DIV => block.operator_rhs.clone(),
            _ => None,
        },
        operator_marker: block.operator_marker,
        target_os: None,
    }
}

fn lower_impl(definition: &ImplDef, module: &str) -> TirImplDef {
    TirImplDef {
        module: module.to_string(),
        key: match definition.trait_name.as_deref() {
            Some(trait_name) => format!("{}::{}::{trait_name}", module, definition.type_name),
            None => format!("{}::{}", module, definition.type_name),
        },
        span: definition.span,
        self_type: Type::Named(qualified_key(module, &definition.type_name)),
        trait_name: definition.trait_name.clone(),
        trait_ref: definition.trait_name.clone(),
        associated_types: definition
            .assoc_type_impls
            .iter()
            .map(|(name, span, ty)| TirAssociatedTypeValue {
                name: name.clone(),
                ty: ty.clone(),
                span: *span,
            })
            .collect(),
        methods: method_keys(
            module,
            &definition.type_name,
            definition.trait_name.as_deref(),
            definition.operator_rhs.as_ref(),
            &definition.methods,
        ),
        delegation_field: definition.delegation_field.clone(),
        compiler_generated: definition.is_generated_serde,
        serde: match definition.trait_name.as_deref() {
            Some(crate::Generics::ENCODE) => Some(TirSerdeCodec::Encode),
            Some(crate::Generics::DECODE) => Some(TirSerdeCodec::Decode),
            _ => None,
        },
        operator_rhs: definition.operator_rhs.clone(),
        operator_marker: definition.operator_marker,
        target_os: definition.os_target,
    }
}

fn lower_constant(definition: &ConstDef, module: &str) -> TirConstantDef {
    TirConstantDef {
        module: module.to_string(),
        key: qualified_key(module, &definition.name),
        name: definition.name.clone(),
        span: definition.span,
        visibility: TirVisibility::Public,
        ty: definition
            .ty
            .clone()
            .or_else(|| definition.ct.as_ref().map(|value| value.jet_type()))
            .unwrap_or(Type::Named("Unit".into())),
        value: definition.ct.clone().unwrap_or(CtValue::Unit),
    }
}

const COMPILER_OWNED_ENUMS: &[(&str, &[&str])] = &[
    (
        crate::Syntax::TYPE_ORDERING,
        crate::Syntax::ORDERING_VARIANTS,
    ),
    (
        crate::Syntax::TYPE_TASK_FAILURE,
        &[
            crate::Syntax::TASK_FAILURE_CANCELLED,
            crate::Syntax::TASK_FAILURE_DEADLINE_BLOWN,
            crate::Syntax::TASK_FAILURE_PANICKED,
        ],
    ),
    (
        crate::Syntax::DURATION_UNIT_TYPE,
        crate::Syntax::DURATION_UNITS,
    ),
    ("FontStyle", &["Body", "Title", "Monospace"]),
    ("GlyphShaper", &["HarfBuzz", "HeadlessFallback"]),
    (
        "DataTree",
        &[
            "Null",
            "Bool",
            "Int",
            "Float",
            "Number",
            "TypedText",
            "Text",
            "Bytes",
            "Array",
            "Object",
        ],
    ),
];
// Compiler-owned Core records are checked values, not user declarations. Keep
// their owner rows in the checked declaration table so a field projection can
// use the same nominal identity in every engine. The field list is only the
// Core oracle's vocabulary; field types are read below through
// `core_struct_field_type`, never guessed from a folded value.
const COMPILER_OWNED_CORE_RECORDS: &[(&str, &[&str])] = &[
    ("DirEntry", &["name", "path", "is_dir"]),
    ("WalkEntry", &["path", "relative", "is_dir", "depth"]),
    (
        "Stat",
        &[
            "size",
            "modified_ms",
            "created_ms",
            "readonly",
            "is_file",
            "is_dir",
            "is_symlink",
            "kind",
            "mode",
        ],
    ),
    ("DataColumn", &["id", "name", "type_name", "nullable"]),
    (
        "DataStatus",
        &[
            "step",
            "path",
            "copy",
            "ownership",
            "trust",
            "fallback",
            "replacement",
        ],
    ),
    ("FontFace", &["family", "size", "style"]),
    (
        "Glyph",
        &["id", "cluster", "x", "y", "advance_x", "advance_y"],
    ),
    (
        "GlyphRun",
        &[
            "glyphs",
            "advance_x",
            "advance_y",
            "shaper",
            "deterministic",
            "approximate",
        ],
    ),
    (
        crate::Syntax::TYPE_TYPE_INFO,
        &[
            "name",
            "path",
            "module",
            "identity",
            "kind",
            "layout",
            "fields",
            "methods",
            "type_params",
            "markers",
            "expanded_markers",
            "implements",
            "states",
            "transitions",
            "facts",
            "dimensions",
            "span",
        ],
    ),
    (
        crate::Syntax::TYPE_LAYOUT_INFO,
        &[
            "kind",
            "target",
            "guarantee",
            "source",
            "size",
            "alignment",
            "stride",
            "requested_alignment",
            "effective_alignment",
            "fields",
        ],
    ),
    (
        crate::Syntax::TYPE_LAYOUT_FIELD,
        &[
            "name",
            "ty",
            "target",
            "guarantee",
            "source",
            "offset",
            "size",
        ],
    ),
    ("MarkerInfo", &["name", "args"]),
    ("MarkerArgInfo", &["name", "ty", "value"]),
    ("StateInfo", &["name", "path", "terminal", "reachable"]),
    ("StateRef", &["owner", "name", "path"]),
    ("TransitionInfo", &["operation", "from", "to"]),
    ("FactInfo", &["kind", "name", "path", "value"]),
    (
        "FactValue",
        &[
            "kind",
            "name",
            "members",
            "range",
            "dimension",
            "measure",
            "exactness",
            "layout",
            "classification",
            "nominal",
            "obligation",
            "state",
            "sendability",
            "view_provenance",
            "movedness",
            "attribution",
            "origin",
            "unit_scale_provenance",
            "maturity",
        ],
    ),
    ("MeasureInfo", &["kind", "value", "symbol"]),
    ("ExactnessInfo", &["kind", "precision"]),
    ("LayoutFact", &["bytes"]),
    ("ClassificationInfo", &["name"]),
    ("NominalInfo", &["name"]),
    ("ObligationParamInfo", &["name", "zone"]),
    (
        "ObligationInfo",
        &["effect_bound", "param_contract", "variadic"],
    ),
    ("SendabilityInfo", &["known"]),
    ("MovednessInfo", &["known"]),
    ("ViewProvenanceInfo", &["sources", "mutable"]),
    ("AttributionInfo", &["source", "code"]),
    (
        "OriginInfo",
        &["tracked", "source", "line", "column", "ambiguity"],
    ),
    (
        "UnitScaleProvenanceInfo",
        &["kind", "value", "source", "uncertainty"],
    ),
    ("MaturityInfo", &["level"]),
    ("DimensionInfo", &["axes", "identity", "display"]),
    ("DimensionAxis", &["name", "exponent"]),
    (
        "FunctionInfo",
        &[
            "name",
            "module",
            "identity",
            "params",
            "span",
            "effects",
            "reaches_panic",
            "arithmetic",
            "facts",
        ],
    ),
    (
        "ArithmeticOperationInfo",
        &["operation", "policy", "operation_span", "scope_span"],
    ),
    ("PackageInfo", &["name", "identity", "types", "functions"]),
    ("EffectInfo", &["values"]),
    (
        "MethodInfo",
        &[
            "name",
            "module",
            "identity",
            "return_type",
            "signature",
            "params",
            "effects",
            "markers",
            "dimensions",
            "facts",
            "arithmetic",
            "is_pub",
            "span",
        ],
    ),
    (
        "FieldInfo",
        &[
            "name",
            "ty",
            "index",
            "fields",
            "markers",
            "dimensions",
            "facts",
            "is_pub",
            "span",
        ],
    ),
    ("TypeParamInfo", &["name", "bounds", "span"]),
    ("SourceSpan", &["start", "end"]),
];

pub(crate) fn is_compiler_owned_type(name: &str) -> bool {
    COMPILER_OWNED_ENUMS.iter().any(|(owned, _)| *owned == name)
        || COMPILER_OWNED_CORE_RECORDS
            .iter()
            .any(|(owned, _)| *owned == name)
        || name == crate::Syntax::TYPE_ERR
        || matches!(
            name,
            "VjpRun"
                | "Group"
                | "DataJoin"
                | "DataPivotCell"
                | "DataSummary"
                | "FieldError"
                | "AllocError"
                | crate::Syntax::TYPE_RANGE
        )
}

pub(crate) fn compiler_owned_enum_variants(name: &str) -> Option<&'static [&'static str]> {
    COMPILER_OWNED_ENUMS
        .iter()
        .find(|(owned, _)| *owned == name)
        .map(|(_, variants)| *variants)
}

fn compiler_owned_default_err(module: &str) -> TirTypeDef {
    let span = Span::new(0, 0);
    let field = |name: &str, ty: Type| TirField {
        name: name.to_string(),
        shape_names: ShapeFieldNames::from_source(name),
        skip: false,
        ty,
        span,
        public: true,
        package_public: false,
        computed: false,
        has_default: false,
    };
    TirTypeDef {
        module: module.to_string(),
        key: crate::Syntax::TYPE_ERR.to_string(),
        name: crate::Syntax::TYPE_ERR.to_string(),
        span,
        public: true,
        package_public: false,
        generic_params: Vec::new(),
        derives: Vec::new(),
        auto_derive_default: false,
        auto_printable: false,
        published_schema: false,
        single_use: false,
        must_use: false,
        layout: None,
        layout_alignment: None,
        serde: Vec::new(),
        cli_bindings: Vec::new(),
        cli: None,
        ownership: TirOwnership::Owned,
        boxed_edges: Vec::new(),
        kind: TirTypeDefKind::Struct {
            fields: vec![
                field("message", Type::String),
                field("code", Type::Option(Box::new(Type::String))),
                field(
                    "cause",
                    Type::Option(Box::new(Type::Named(crate::Syntax::TYPE_ERR.to_string()))),
                ),
            ],
            methods: Vec::new(),
        },
    }
}
fn compiler_owned_record(
    module: &str,
    name: &str,
    fields: impl IntoIterator<Item = (&'static str, Type)>,
) -> TirTypeDef {
    let span = Span::new(0, 0);
    TirTypeDef {
        module: module.to_string(),
        key: name.to_string(),
        name: name.to_string(),
        span,
        public: true,
        package_public: false,
        generic_params: Vec::new(),
        derives: Vec::new(),
        auto_derive_default: false,
        auto_printable: false,
        published_schema: false,
        single_use: false,
        must_use: false,
        layout: None,
        layout_alignment: None,
        serde: Vec::new(),
        cli_bindings: Vec::new(),
        cli: None,
        ownership: TirOwnership::Owned,
        boxed_edges: Vec::new(),
        kind: TirTypeDefKind::Struct {
            fields: fields
                .into_iter()
                .map(|(name, ty)| TirField {
                    name: name.to_string(),
                    shape_names: ShapeFieldNames::from_source(name),
                    skip: false,
                    ty,
                    span,
                    public: true,
                    package_public: false,
                    computed: false,
                    has_default: false,
                })
                .collect(),
            methods: Vec::new(),
        },
    }
}
/// Build one compiler-owned Core record from the checked field oracle.  This
/// keeps reflection/metadata declarations aligned with sema when a field is
/// projected through checked TIR.
fn compiler_owned_core_record(
    module: &str,
    name: &'static str,
    fields: &'static [&'static str],
) -> TirTypeDef {
    compiler_owned_record(
        module,
        name,
        fields.iter().copied().map(|field| {
            (
                field,
                crate::Sema::core_struct_field_type(name, field, &[])
                    .unwrap_or_else(|| panic!("missing canonical Core field {name}.{field}")),
            )
        }),
    )
}

fn compiler_owned_vjp_run(module: &str) -> TirTypeDef {
    let span = Span::new(0, 0);
    let field = |name: &str, ty: Type| TirField {
        name: name.to_string(),
        shape_names: ShapeFieldNames::from_source(name),
        skip: false,
        ty,
        span,
        public: true,
        package_public: false,
        computed: false,
        has_default: false,
    };
    TirTypeDef {
        module: module.to_string(),
        key: "VjpRun".to_string(),
        name: "VjpRun".to_string(),
        span,
        public: true,
        package_public: false,
        generic_params: vec![super::TGenericParam {
            name: "T".to_string(),
            bounds: Vec::new(),
        }],
        derives: Vec::new(),
        auto_derive_default: false,
        auto_printable: false,
        published_schema: false,
        single_use: false,
        must_use: false,
        layout: None,
        layout_alignment: None,
        serde: Vec::new(),
        cli_bindings: Vec::new(),
        cli: None,
        ownership: TirOwnership::Owned,
        boxed_edges: Vec::new(),
        kind: TirTypeDefKind::Struct {
            fields: vec![
                field("value", Type::Named("Tensor".to_string())),
                field(
                    "pull",
                    Type::Fn {
                        params: vec![Type::Named("Tensor".to_string())],
                        ret: Some(Box::new(Type::Named("T".to_string()))),
                        effect_bound: None,
                        param_contract: None,
                        call_metadata: None,
                        return_view_provenance: None,
                    },
                ),
                field(
                    "grads",
                    Type::Fn {
                        params: Vec::new(),
                        ret: Some(Box::new(Type::Named("T".to_string()))),
                        effect_bound: None,
                        param_contract: None,
                        call_metadata: None,
                        return_view_provenance: None,
                    },
                ),
            ],
            methods: Vec::new(),
        },
    }
}

fn compiler_owned_parse_error(module: &str) -> TirTypeDef {
    let span = Span::new(0, 0);
    let mut row = lower_alias(
        &TypeAliasDef {
            is_pub: true,
            is_package_pub: false,
            name: "ParseError".to_string(),
            name_span: span,
            type_params: Vec::new(),
            target: Type::String,
            target_span: span,
            span,
        },
        module,
        &|ty, _| ty.clone(),
        false,
    );
    // Compiler-owned rows use the shared Prelude spelling rather than a module
    // qualification. Keep alias lowering responsible for the representation.
    row.key = "ParseError".to_string();
    row
}

fn compiler_owned_type_defs(
    module: &str,
    seed_parse_error: bool,
) -> impl Iterator<Item = TirTypeDef> + '_ {
    let span = Span::new(0, 0);
    COMPILER_OWNED_ENUMS
        .iter()
        .map(move |(name, variants)| TirTypeDef {
            module: module.to_string(),
            key: name.to_string(),
            name: name.to_string(),
            span,
            public: true,
            package_public: false,
            generic_params: Vec::new(),
            derives: Vec::new(),
            auto_derive_default: false,
            auto_printable: false,
            published_schema: false,
            single_use: false,
            must_use: false,
            layout: None,
            layout_alignment: None,
            serde: Vec::new(),
            cli_bindings: Vec::new(),
            cli: None,
            ownership: TirOwnership::Owned,
            boxed_edges: Vec::new(),
            kind: TirTypeDefKind::Enum {
                variants: variants
                    .iter()
                    .map(|variant| TirVariant {
                        name: variant.to_string(),
                        wire_name: variant.to_string(),
                        span,
                        payload: if *name == crate::Syntax::TYPE_TASK_FAILURE {
                            // This row mirrors the canonical Prelude enum. The
                            // runtime owns failure conversion; MIR only needs
                            // the nominal variant shape for typed projections.
                            if *variant == crate::Syntax::TASK_FAILURE_PANICKED {
                                TirVariantPayload::Single(Type::String)
                            } else {
                                TirVariantPayload::Unit
                            }
                        } else if *name == "DataTree" {
                            match *variant {
                                "Bool" => TirVariantPayload::Single(Type::Bool),
                                "Int" => TirVariantPayload::Single(Type::Int),
                                "Float" => TirVariantPayload::Single(Type::Float),
                                "Number" | "TypedText" | "Text" => {
                                    TirVariantPayload::Single(Type::String)
                                }
                                "Bytes" => {
                                    TirVariantPayload::Single(Type::List(Box::new(Type::IntN {
                                        signed: false,
                                        bits: 8,
                                    })))
                                }
                                "Array" => TirVariantPayload::Single(Type::List(Box::new(
                                    Type::Named("DataTree".to_string()),
                                ))),
                                "Object" => TirVariantPayload::Single(Type::List(Box::new(
                                    Type::Tuple(vec![
                                        ("key".to_string(), Box::new(Type::String)),
                                        (
                                            "value".to_string(),
                                            Box::new(Type::Named("DataTree".to_string())),
                                        ),
                                    ]),
                                ))),
                                _ => TirVariantPayload::Unit,
                            }
                        } else {
                            TirVariantPayload::Unit
                        },
                        discriminant: None,
                    })
                    .collect(),
                methods: Vec::new(),
            },
        })
        .chain(std::iter::once(compiler_owned_default_err(module)))
        .chain(std::iter::once(compiler_owned_record(
            module,
            "FieldError",
            ["path", "reason"].map(|name| {
                (
                    name,
                    crate::Sema::core_struct_field_type("FieldError", name, &[])
                        .expect("canonical FieldError field"),
                )
            }),
        )))
        .chain(std::iter::once(compiler_owned_record(
            module,
            crate::Syntax::TYPE_RANGE,
            [
                ("start", Type::Int),
                ("end", Type::Int),
                ("exclusive", Type::Bool),
            ],
        )))
        .chain(
            [
                ("DataJoin", ["L", "R"], ["left", "right"]),
                ("Group", ["K", "V"], ["key", "value"]),
            ]
            .into_iter()
            .map(move |(name, params, fields)| {
                let args = params.map(|name| Type::Named(name.to_string()));
                let mut row = compiler_owned_record(
                    module,
                    name,
                    fields.map(|field| {
                        (
                            field,
                            crate::Sema::core_struct_field_type(name, field, &args)
                                .expect("canonical generic Core record field"),
                        )
                    }),
                );
                row.generic_params = params
                    .into_iter()
                    .map(|name| super::TGenericParam {
                        name: name.to_string(),
                        bounds: Vec::new(),
                    })
                    .collect();
                row
            }),
        )
        .chain(std::iter::once(compiler_owned_record(
            module,
            "DataPivotCell",
            ["row_key", "column_key", "count", "sum", "mean"].map(|name| {
                (
                    name,
                    crate::Sema::core_struct_field_type("DataPivotCell", name, &[])
                        .expect("canonical DataPivotCell field"),
                )
            }),
        )))
        .chain(std::iter::once(compiler_owned_record(
            module,
            "AllocError",
            ["requested_bytes", "allocator"].map(|name| {
                (
                    name,
                    crate::Sema::core_struct_field_type("AllocError", name, &[])
                        .expect("canonical AllocError field"),
                )
            }),
        )))
        .chain(std::iter::once(compiler_owned_record(
            module,
            "DataSummary",
            [
                "count", "sum", "mean", "min", "max", "median", "variance", "stddev",
            ]
            .map(|name| {
                (
                    name,
                    crate::Sema::core_struct_field_type("DataSummary", name, &[])
                        .expect("canonical DataSummary field"),
                )
            }),
        )))
        .chain(std::iter::once(compiler_owned_vjp_run(module)))
        .chain(
            COMPILER_OWNED_CORE_RECORDS
                .iter()
                .map(move |(name, fields)| compiler_owned_core_record(module, *name, *fields)),
        )
        .chain(seed_parse_error.then(|| compiler_owned_parse_error(module)))
}

pub(crate) fn is_compiler_owned_trait(name: &str) -> bool {
    matches!(
        name,
        crate::Generics::ENCODE
            | crate::Generics::DECODE
            | crate::Generics::DISPLAY
            | crate::Generics::DEBUG
            | crate::Generics::EQUATABLE
            | crate::Generics::COMPARABLE
            | crate::Generics::CLOSE
            | crate::Generics::ADD
            | crate::Generics::SUB
            | crate::Generics::MUL
            | crate::Generics::DIV
    )
}
