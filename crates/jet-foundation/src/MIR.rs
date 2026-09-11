//! Canonical semantic MIR shared by every execution adapter.
//!
//! This module contains semantic facts, not target instructions or generated
//! source. Checked analytical TIR lowers into this model exactly once; later
//! adapters only marshal its values, places, calls, and control-flow edges.

use crate::Diagnostics::Span;
use crate::JSON::json_escape;
use crate::Layout::LayoutAlignmentFact;
use crate::Effects::Effect;
use crate::Shape::ShapeFieldNames;
use crate::Syntax::{
    CoreCallFallibility, CoreCallInterpreterRoute, CoreCallPureRoute, CoreCallSymbol,
    CoreMarkerApplication, SinkClass,
};
use crate::Facts::{
    DerivationDisposition, DerivationIdentity, DerivationMethod, DerivationRecord, DerivationRef,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, Mutex};

pub const MIR_SCHEMA_VERSION: u16 = 3;

macro_rules! mir_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u64);
    };
}

mir_id!(MirFunctionId);
mir_id!(MirBlockId);
mir_id!(MirValueId);
mir_id!(MirPlaceId);
mir_id!(MirLocalId);
mir_id!(MirOpId);
mir_id!(MirTypeId);
mir_id!(MirCoreCallId);
mir_id!(MirScopeId);
mir_id!(MirClosureId);
mir_id!(MirModuleId);
mir_id!(MirImportId);
mir_id!(MirTraitId);
mir_id!(MirImplId);
mir_id!(MirConstantId);
mir_id!(MirGcEditSiteId);
mir_id!(MirForeignId);
mir_id!(MirLinkUnitId);
mir_id!(MirCallbackId);
mir_id!(MirHandleId);
mir_id!(MirArtifactId);
mir_id!(MirHarnessId);
mir_id!(MirJobId);
mir_id!(MirTestId);
mir_id!(MirCoveragePointId);
mir_id!(MirTraitMethodId);
mir_id!(MirOutputCheckId);

mir_id!(MirDataPlanNodeId);


/// FNV-1a is intentionally used instead of a process-random hash.  Every ID
/// includes a semantic namespace and a source identity supplied by lowering;
/// allocation order is never part of an ID.
pub fn stable_id(namespace: &str, identity: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in namespace
        .as_bytes()
        .iter()
        .chain([0].iter())
        .chain(identity.as_bytes().iter())
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    if hash == 0 { 1 } else { hash }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirVisibility {
    Private,
    Package,
    Public,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirFunctionKind {
    Jet,
    Foreign,
    Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirSerdeCodec {
    Encode,
    Decode,
}

#[derive(Debug, Clone)]
pub enum MirFunctionForm {
    TopLevel,
    Method {
        owner: MirType,
        self_access: Option<MirAccess>,
    },
    TraitMethod {
        owner: MirType,
        trait_ref: MirTraitRef,
        self_access: Option<MirAccess>,
        serde: Option<MirSerdeCodec>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirAccess {
    Read,
    Write,
    Move,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirOwnershipMode {
    Copy,
    Owned,
    Shared,
    ReadBorrow,
    WriteBorrow,
    Move,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirDropKind {
    None,
    Value,
    Shared,
    View,
    ForeignHandle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirOwnership {
    pub mode: MirOwnershipMode,
    pub drop: MirDropKind,
    pub moved: bool,
    pub last_use: bool,
    pub gc_root: bool,
}

impl MirOwnership {
    pub const fn copy() -> Self {
        Self {
            mode: MirOwnershipMode::Copy,
            drop: MirDropKind::None,
            moved: false,
            last_use: false,
            gc_root: false,
        }
    }
    /// Owned semantic values carry the ordinary drop obligation.
    #[allow(non_upper_case_globals)]
    pub const Owned: Self = Self {
        mode: MirOwnershipMode::Owned,
        drop: MirDropKind::Value,
        moved: false,
        last_use: false,
        gc_root: false,
    };

    pub const fn from_access(access: MirAccess) -> Self {
        match access {
            MirAccess::Read => Self {
                mode: MirOwnershipMode::ReadBorrow,
                drop: MirDropKind::None,
                moved: false,
                last_use: false,
                gc_root: false,
            },
            MirAccess::Write => Self {
                mode: MirOwnershipMode::WriteBorrow,
                drop: MirDropKind::View,
                moved: false,
                last_use: false,
                gc_root: false,
            },
            MirAccess::Move => Self {
                mode: MirOwnershipMode::Move,
                drop: MirDropKind::Value,
                moved: false,
                last_use: true,
                gc_root: false,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MirMeasureRule {
    Add,
    Mul,
    Match,
}

/// Target-neutral copy of one compile-time measure.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MirMeasure {
    Literal {
        kind: String,
        value: u64,
    },
    SignedLiteral {
        kind: String,
        value: i64,
    },
    Symbol {
        kind: String,
        name: String,
    },
    Combined {
        kind: String,
        rule: MirMeasureRule,
        left: Box<MirMeasure>,
        right: Box<MirMeasure>,
    },
}

impl MirMeasure {

    pub fn kind(&self) -> &str {
        match self {
            Self::Literal { kind, .. }
            | Self::SignedLiteral { kind, .. }
            | Self::Symbol { kind, .. }
            | Self::Combined { kind, .. } => kind,
        }
    }

    pub fn literal_value(&self) -> Option<u64> {
        match self {
            Self::Literal { value, .. } => Some(*value),
            Self::Combined {
                rule: MirMeasureRule::Add,
                left,
                right,
                ..
            } => left.literal_value()?.checked_add(right.literal_value()?),
            Self::Combined {
                rule: MirMeasureRule::Mul,
                left,
                right,
                ..
            } => left.literal_value()?.checked_mul(right.literal_value()?),
            _ => None,
        }
    }

    pub fn signed_literal_value(&self) -> Option<i64> {
        match self {
            Self::SignedLiteral { value, .. } => Some(*value),
            Self::Literal { value, .. } => i64::try_from(*value).ok(),
            Self::Combined {
                rule: MirMeasureRule::Add,
                left,
                right,
                ..
            } => left
                .signed_literal_value()?
                .checked_add(right.signed_literal_value()?),
            Self::Combined {
                rule: MirMeasureRule::Mul,
                left,
                right,
                ..
            } => left
                .signed_literal_value()?
                .checked_mul(right.signed_literal_value()?),
            _ => None,
        }
    }

    pub fn symbol_name(&self) -> Option<&str> {
        match self {
            Self::Symbol { name, .. } => Some(name),
            _ => None,
        }
    }

    pub fn expression(&self) -> String {
        match self {
            Self::Literal { value, .. } => value.to_string(),
            Self::SignedLiteral { value, .. } => value.to_string(),
            Self::Symbol { name, .. } => name.clone(),
            Self::Combined {
                rule: MirMeasureRule::Add,
                left,
                right,
                ..
            } => format!("({left} + {right})"),
            Self::Combined {
                rule: MirMeasureRule::Mul,
                left,
                right,
                ..
            } => format!("({left} * {right})"),
            Self::Combined {
                rule: MirMeasureRule::Match,
                left,
                right,
                ..
            } => format!("match({left}, {right})"),
        }
    }
}

impl fmt::Display for MirMeasure {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.expression())
    }
}


/// Target-neutral dimension row. Axes remain sorted by their map key.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct MirDimension {
    pub axes: BTreeMap<String, MirMeasure>,
}

impl MirDimension {
    pub fn scalar() -> Self {
        Self::default()
    }


    pub fn measure_exponents(&self) -> impl Iterator<Item = (&str, &MirMeasure)> {
        self.axes.iter().map(|(axis, exponent)| (axis.as_str(), exponent))
    }

    pub fn axes(&self) -> impl Iterator<Item = (&str, i32)> {
        self.measure_exponents().map(|(axis, exponent)| {
            (
                axis,
                exponent
                    .signed_literal_value()
                    .and_then(|value| i32::try_from(value).ok())
                    .expect("dimension exponent must be a concrete i32 measure"),
            )
        })
    }

    pub fn identity(&self) -> String {
        self.axes
            .iter()
            .map(|(axis, exponent)| format!("{axis}:{exponent}"))
            .collect::<Vec<_>>()
            .join(";")
    }

    pub fn display_name(&self) -> String {
        let mut parts = Vec::new();
        for (axis, exponent) in self.axes() {
            let name = axis.rsplit("::").next().unwrap_or(axis);
            parts.push(if exponent == 1 {
                name.to_string()
            } else {
                format!("{name}^{exponent}")
            });
        }
        if parts.is_empty() {
            "Scalar".to_string()
        } else {
            parts.join(" * ")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MirInternalTag {
    CoreCryptoNominal,
    DeterministicClock,
    SystemClock,
    ExpiringSecretLoan,
    SharedGuardRead,
    SharedGuardEdit,
    TerminalFactSet,
    CppCallbackAbi,
    AllocatorView,
}


impl MirInternalTag {
    pub fn spelling(self) -> &'static str {
        match self {
            Self::CoreCryptoNominal => "core.crypto",
            Self::DeterministicClock => "clock.deterministic",
            Self::SystemClock => "clock.system",
            Self::ExpiringSecretLoan => "expiring_secret.loan",
            Self::SharedGuardRead => "shared_guard.read",
            Self::SharedGuardEdit => "shared_guard.edit",
            Self::TerminalFactSet => "terminal.fact_set",
            Self::CppCallbackAbi => "cpp.callback_abi",
            Self::AllocatorView => "allocator.view",
        }
    }

    pub fn identity_bearing(self) -> bool {
        matches!(self, Self::CoreCryptoNominal)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MirTagMarker {
    User(String),
    Internal(MirInternalTag),
}


impl MirTagMarker {
    pub fn identity_bearing(&self) -> bool {
        matches!(self, Self::Internal(tag) if tag.identity_bearing())
    }
}

impl fmt::Display for MirTagMarker {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User(name) => formatter.write_str(name),
            Self::Internal(tag) => formatter.write_str(tag.spelling()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum MirParamZone {
    PositionalOnly,
    #[default]
    Either,
    LabelOnly,
}
impl MirParamZone {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PositionalOnly => "positional-only",
            Self::Either => "either",
            Self::LabelOnly => "label-only",
        }
    }
}



/// One public label and its accepted call form.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MirCallContractRow {
    pub label: String,
    pub zone: MirParamZone,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MirCallablePolicy {
    pub name: String,
    pub arguments: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MirCallablePolicyChain {
    pub policies: Vec<MirCallablePolicy>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MirCallMetadata {
    pub names: Vec<String>,
    pub defaults: Vec<bool>,
    pub variadic: Vec<bool>,
    pub conventions: Vec<MirAccess>,
    pub policies: MirCallablePolicyChain,
}

impl MirCallMetadata {

    pub fn is_variadic(&self) -> bool {
        self.variadic.iter().any(|is_variadic| *is_variadic)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirNominalRef {
    pub id: MirTypeId,
    pub name: String,
}

impl MirNominalRef {
    pub fn from_name(name: impl Into<String>) -> Self {
        let name = name.into();
        let id = MirTypeId(stable_id("mir-nominal", &name));
        Self { id, name }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirTraitRef {
    pub id: MirTraitId,
    pub name: String,
}

impl MirTraitRef {
    pub fn from_name(name: impl Into<String>) -> Self {
        let name = name.into();
        let id = MirTraitId(stable_id("mir-trait", &name));
        Self { id, name }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirFunctionSignature {
    pub params: Vec<MirType>,
    pub ret: Option<Box<MirType>>,
    /// Effects are a use-site obligation, not callable identity.
    pub effect_bound: Option<Vec<String>>,
    /// Public labels/zones are callable identity.
    pub param_contract: Option<Vec<MirCallContractRow>>,
    pub call_metadata: Option<MirCallMetadata>,
    /// Returned-view provenance is checked directionally, not by identity.
    pub return_view_provenance: Option<BTreeMap<Vec<String>, MirViewProvenance>>,
}

impl MirFunctionSignature {
    pub fn display_name(&self) -> String {
        let params = self
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                self.param_contract
                    .as_ref()
                    .and_then(|rows| rows.get(index))
                    .map(|row| format!("{}: {}", row.label, param.display_name()))
                    .unwrap_or_else(|| param.display_name())
            })
            .collect::<Vec<_>>()
            .join(", ");
        let mut signature = format!("fn({params})");
        if let Some(ret) = &self.ret {
            if !ret.is_unit() {
                signature.push(' ');
                signature.push_str(&ret.display_name());
            }
        }
        if let Some(effect_bound) = &self.effect_bound {
            signature.push_str(" -[");
            signature.push_str(&effect_bound.join(","));
            signature.push_str("]>");
        }
        signature
    }


    fn same_identity(&self, other: &Self) -> bool {
        self.params.len() == other.params.len()
            && self
                .params
                .iter()
                .zip(&other.params)
                .all(|(left, right)| left.same_checked_type(right))
            && match (&self.ret, &other.ret) {
                (None, None) => true,
                (Some(left), Some(right)) => left.same_checked_type(right),
                _ => false,
            }
            && self.param_contract == other.param_contract
            && self
                .call_metadata
                .as_ref()
                .map(|metadata| metadata.is_variadic())
                == other
                    .call_metadata
                    .as_ref()
                    .map(|metadata| metadata.is_variadic())
                && self
                    .call_metadata
                    .as_ref()
                    .filter(|metadata| metadata.is_variadic())
                    .map(|metadata| &metadata.variadic)
                    == other
                        .call_metadata
                        .as_ref()
                        .filter(|metadata| metadata.is_variadic())
                        .map(|metadata| &metadata.variadic)
    }
}


/// Backend-neutral scalar kind used by the checked layout row. Adapters map
/// these onto their own machine representation; this file never selects one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirScalarKind {
    Int,
    Float,
    Float32,
    Bool,
    Char,
    Pointer,
}

/// Backend-neutral ABI classification for a checked type's layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirAbi {
    Scalar(MirScalarKind),
    Aggregate,
    Sequence,
    Function,
    Nominal,
    Dynamic,
    Never,
}

/// A checked size or alignment, either a known static byte count or a
/// backend-computed dynamic value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirSize {
    Static(u64),
    Dynamic,
}

/// Checked, backend-neutral layout row attached to every `MirType`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirLayout {
    pub abi: MirAbi,
    pub size: MirSize,
    pub align: MirSize,
}
#[derive(Debug, Clone)]
pub enum MirTypeKind {
    Int,
    Float,
    Bool,
    String,
    Char,
    List(Box<MirType>),
    Map {
        key: Box<MirType>,
        value: Box<MirType>,
    },
    Shared(Box<MirType>),
    Option(Box<MirType>),
    Result {
        ok: Box<MirType>,
        err: Box<MirType>,
    },
    Fn(MirFunctionSignature),
    /// Thread-safe callable carrier. Unlike `Fn`, its identity is only the
    /// checked parameter and return representation.
    SendFn {
        params: Vec<MirType>,
        ret: Option<Box<MirType>>,
    },
    Apply {
        name: MirNominalRef,
        args: Vec<MirType>,
    },
    TraitObject(Vec<MirNominalRef>),
    Tuple(Vec<(String, MirType)>),
    FixedList {
        elem: Box<MirType>,
        len: MirMeasure,
    },
    IntN {
        signed: bool,
        bits: u8,
    },
    InlineRange {
        base: Box<MirType>,
        lo: i64,
        hi: i64,
    },
    Float32,
    Tagged {
        marker: MirTagMarker,
        inner: Box<MirType>,
    },
    Union(Vec<MirType>),
    Quantity {
        base: Box<MirType>,
        dimension: MirDimension,
    },
    Measure(MirMeasure),
}

impl MirTypeKind {

    pub fn same_identity(&self, other: &Self) -> bool {
        let left = transparent_tag_inner(self);
        let right = transparent_tag_inner(other);
        if let Some(left) = left {
            return if let Some(right) = right {
                left.same_checked_type(right)
            } else {
                same_kind_identity(left.kind(), other)
            };
        }
        if let Some(right) = right {
            return same_kind_identity(self, right.kind());
        }
        same_kind_identity(self, other)
    }

    pub fn canonical_key(&self) -> String {
        match self {
            Self::Int => "Int".to_string(),
            Self::Float => "Float".to_string(),
            Self::Bool => "Bool".to_string(),
            Self::String => "String".to_string(),
            Self::Char => "Char".to_string(),
            Self::List(inner) => format!("List<{}>", inner.canonical_key()),
            Self::Map { key, value } => {
                format!("Map<{},{}>", key.canonical_key(), value.canonical_key())
            }
            Self::Shared(inner) => format!("Shared<{}>", inner.canonical_key()),
            Self::Option(inner) => format!("Option<{}>", inner.canonical_key()),
            Self::Result { ok, err } => {
                format!("Result<{},{}>", ok.canonical_key(), err.canonical_key())
            }
            Self::Fn(signature) => {
                let params = signature
                    .params
                    .iter()
                    .map(MirType::canonical_key)
                    .collect::<Vec<_>>()
                    .join(",");
                let ret = signature
                    .ret
                    .as_deref()
                    .map(MirType::canonical_key)
                    .unwrap_or_else(|| "Unit".to_string());
                let contract = signature
                    .param_contract
                    .as_ref()
                    .map(|rows| {
                        rows.iter()
                            .map(|row| format!("{}:{}", row.label, row.zone.as_str()))
                            .collect::<Vec<_>>()
                            .join(",")
                    })
                    .unwrap_or_default();
                let variadic = signature
                    .call_metadata
                    .as_ref()
                    .filter(|metadata| metadata.is_variadic())
                    .map(|metadata| {
                        metadata
                            .variadic
                            .iter()
                            .map(|variadic| if *variadic { '1' } else { '0' })
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                format!("Fn({params})->{ret};contract={contract};variadic={variadic}")
            }
            Self::SendFn { params, ret } => {
                let params = params
                    .iter()
                    .map(MirType::canonical_key)
                    .collect::<Vec<_>>()
                    .join(",");
                let ret = ret
                    .as_deref()
                    .map(MirType::canonical_key)
                    .unwrap_or_else(|| "Unit".to_string());
                format!("SendFn({params})->{ret}")
            }
            Self::Apply { name, args } if args.is_empty() => name.name.clone(),
            Self::Apply { name, args } => format!(
                "Apply({})<{}>",
                name.id.0,
                args.iter()
                    .map(MirType::canonical_key)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::TraitObject(bounds) => format!(
                "Trait({})",
                bounds
                    .iter()
                    .map(|bound| bound.id.0.to_string())
                    .collect::<Vec<_>>()
                    .join("+")
            ),
            Self::Tuple(fields) => format!(
                "Tuple({})",
                fields
                    .iter()
                    .map(|(name, ty)| format!("{name}:{}", ty.canonical_key()))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::FixedList { elem, len } => {
                format!("FixedList<{};{}>", elem.canonical_key(), len.expression())
            }
            Self::IntN { signed, bits } => {
                format!("{}Int{bits}", if *signed { "I" } else { "U" })
            }
            Self::InlineRange { base, lo, hi } => {
                format!("Range<{};{lo}..{hi}>", base.canonical_key())
            }
            Self::Float32 => "Float32".to_string(),
            Self::Tagged { marker, inner } if marker.identity_bearing() => {
                format!("Tagged({marker:?};{})", inner.canonical_key())
            }
            Self::Tagged { inner, .. } => inner.canonical_key(),
            Self::Union(members) => format!(
                "Union({})",
                members
                    .iter()
                    .map(MirType::canonical_key)
                    .collect::<Vec<_>>()
                    .join("|")
            ),
            Self::Quantity { base, dimension } => {
                format!("Quantity<{};{}>", base.canonical_key(), dimension.identity())
            }
            Self::Measure(measure) => format!("Measure({}:{})", measure.kind(), measure),
        }
    }

    pub fn display_name(&self) -> String {
        match self {
            Self::Int => "Int".to_string(),
            Self::Float => "Float".to_string(),
            Self::Bool => "Bool".to_string(),
            Self::String => "String".to_string(),
            Self::Char => "Char".to_string(),
            Self::List(inner) => format!("[{}]", inner.display_name()),
            Self::Map { key, value } => {
                format!("[{}:{}]", key.display_name(), value.display_name())
            }
            Self::Shared(inner) => format!("Shared<{}>", inner.display_name()),
            Self::Option(inner) => format!("?{}", inner.display_name()),
            Self::Result { ok, err } => mir_result_name(ok, err),
            Self::Fn(signature) => signature.display_name(),
            Self::SendFn { params, ret } => {
                let params = params
                    .iter()
                    .map(MirType::display_name)
                    .collect::<Vec<_>>()
                    .join(", ");
                let ret = ret
                    .as_deref()
                    .map(MirType::display_name)
                    .unwrap_or_else(|| "Unit".to_string());
                format!("SendFn({params}) -> {ret}")
            }
            Self::Apply { name, args } => {
                let args = args
                    .iter()
                    .map(MirType::display_name)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{}<{}>", name.name, args)
            }
            Self::TraitObject(bounds) => bounds
                .iter()
                .map(|bound| bound.name.as_str())
                .collect::<Vec<_>>()
                .join(" + "),
            Self::Tuple(fields) => format!(
                "({})",
                fields
                    .iter()
                    .map(|(name, ty)| format!("{name}: {}", ty.display_name()))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::FixedList { elem, len } => {
                format!("[{}#{}]", elem.display_name(), len.expression())
            }
            Self::IntN { signed, bits } => mir_int_spelling(*signed, *bits),
            Self::InlineRange { base, lo, hi } => {
                format!("{}({lo}..{hi})", base.display_name())
            }
            Self::Float32 => "F32".to_string(),
            Self::Tagged {
                marker: MirTagMarker::Internal(_),
                inner,
            } => inner.display_name(),
            Self::Tagged {
                marker: MirTagMarker::User(marker),
                inner,
            } => format!("#{marker} {}", inner.display_name()),
            Self::Union(members) => members
                .iter()
                .map(MirType::display_name)
                .collect::<Vec<_>>()
                .join(" | "),
            Self::Quantity { base, dimension } => format!(
                "Quantity<{}, {}; {}>",
                dimension.display_name(),
                base.display_name(),
                dimension.identity()
            ),
            Self::Measure(measure) => measure.expression(),
        }
    }
}

impl PartialEq for MirTypeKind {
    fn eq(&self, other: &Self) -> bool {
        self.same_identity(other)
    }
}

impl Eq for MirTypeKind {}

#[derive(Debug, Clone)]
pub struct MirType {
    pub kind: MirTypeKind,
    pub identity: Option<MirTypeId>,
    pub layout: MirLayout,
}

impl MirType {
    pub fn from_kind(kind: MirTypeKind) -> Self {
        let layout = layout_for_kind(&kind);
        Self {
            kind,
            identity: None,
            layout,
        }
    }

    pub fn with_identity(mut self, identity: MirTypeId) -> Self {
        self.identity = Some(identity);
        self
    }

    pub fn kind(&self) -> &MirTypeKind {
        &self.kind
    }

    pub fn same_checked_type(&self, other: &Self) -> bool {
        match (self.identity, other.identity) {
            (Some(left), Some(right)) => left == right,
            _ => self.kind.same_identity(&other.kind),
        }
    }

    pub fn canonical_key(&self) -> String {
        self.kind.canonical_key()
    }

    pub fn identity_key(&self) -> String {
        self.identity
            .map(|identity| format!("id:{}", identity.0))
            .unwrap_or_else(|| self.canonical_key())
    }


    pub fn send_fn_signature(&self) -> Option<(&[MirType], Option<&MirType>)> {
        match &self.kind {
            MirTypeKind::SendFn { params, ret } => Some((params, ret.as_deref())),
            _ => None,
        }
    }
    pub fn display_name(&self) -> String {
        self.kind.display_name()
    }

    pub fn name(&self) -> String {
        self.display_name()
    }

    pub fn option_inner(&self) -> Option<&MirType> {
        match &self.kind {
            MirTypeKind::Option(inner) => Some(inner),
            _ => None,
        }
    }

    pub fn result_parts(&self) -> Option<(&MirType, &MirType)> {
        match &self.kind {
            MirTypeKind::Result { ok, err } => Some((ok, err)),
            _ => None,
        }
    }

    pub fn function_signature(&self) -> Option<&MirFunctionSignature> {
        match &self.kind {
            MirTypeKind::Fn(signature) => Some(signature),
            _ => None,
        }
    }

    pub fn nominal_id(&self) -> Option<MirTypeId> {
        if self.identity.is_some() {
            return self.identity;
        }
        match &self.kind {
            MirTypeKind::Apply { name, .. } => Some(name.id),
            MirTypeKind::TraitObject(bounds) if bounds.len() == 1 => {
                bounds.first().map(|bound| bound.id)
            }
            _ => None,
        }
    }

    pub fn list_element(&self) -> Option<&MirType> {
        match &self.kind {
            MirTypeKind::List(inner) | MirTypeKind::FixedList { elem: inner, .. } => Some(inner),
            MirTypeKind::Tagged { inner, .. } => inner.list_element(),
            _ => None,
        }
    }

    pub fn map_parts(&self) -> Option<(&MirType, &MirType)> {
        match &self.kind {
            MirTypeKind::Map { key, value } => Some((key, value)),
            _ => None,
        }
    }

    pub fn fixed_list_parts(&self) -> Option<(&MirType, &MirMeasure)> {
        match &self.kind {
            MirTypeKind::FixedList { elem, len } => Some((elem, len)),
            _ => None,
        }
    }

    pub fn nominal_name(&self) -> Option<&str> {
        match &self.kind {
            MirTypeKind::Apply { name, .. } => Some(name.name.as_str()),
            MirTypeKind::TraitObject(bounds) if bounds.len() == 1 => {
                bounds.first().map(|bound| bound.name.as_str())
            }
            _ => None,
        }
    }

    pub fn trait_bounds(&self) -> Option<&[MirNominalRef]> {
        match &self.kind {
            MirTypeKind::TraitObject(bounds) => Some(bounds),
            _ => None,
        }
    }

    pub fn apply_args(&self) -> Option<&[MirType]> {
        match &self.kind {
            MirTypeKind::Apply { args, .. } => Some(args),
            _ => None,
        }
    }

    pub fn tuple_fields(&self) -> Option<&[(String, MirType)]> {
        match &self.kind {
            MirTypeKind::Tuple(fields) => Some(fields),
            _ => None,
        }
    }

    pub fn union_members(&self) -> Option<&[MirType]> {
        match &self.kind {
            MirTypeKind::Union(members) => Some(members),
            _ => None,
        }
    }

    pub fn tagged_inner(&self) -> Option<&MirType> {
        match &self.kind {
            MirTypeKind::Tagged { inner, .. } => Some(inner),
            _ => None,
        }
    }

    pub fn quantity_parts(&self) -> Option<(&MirType, &MirDimension)> {
        match &self.kind {
            MirTypeKind::Quantity { base, dimension } => Some((base, dimension)),
            _ => None,
        }
    }

    pub fn inline_range_parts(&self) -> Option<(&MirType, i64, i64)> {
        match &self.kind {
            MirTypeKind::InlineRange { base, lo, hi } => Some((base, *lo, *hi)),
            _ => None,
        }
    }

    pub fn measure(&self) -> Option<&MirMeasure> {
        match &self.kind {
            MirTypeKind::Measure(measure) => Some(measure),
            _ => None,
        }
    }

    pub fn fixed_int(&self) -> Option<(bool, u8)> {
        match &self.kind {
            MirTypeKind::IntN { signed, bits } => Some((*signed, *bits)),
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. } => base.fixed_int(),
            _ => None,
        }
    }

    pub fn is_bool(&self) -> bool {
        match &self.kind {
            MirTypeKind::Bool => true,
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. } => base.is_bool(),
            _ => false,
        }
    }

    pub fn is_string(&self) -> bool {
        match &self.kind {
            MirTypeKind::String => true,
            MirTypeKind::Tagged { inner, .. } => inner.is_string(),
            _ => false,
        }
    }

    pub fn is_char(&self) -> bool {
        match &self.kind {
            MirTypeKind::Char => true,
            MirTypeKind::Tagged { inner, .. } => inner.is_char(),
            _ => false,
        }
    }

    pub fn is_scalar(&self) -> bool {
        matches!(self.layout.abi, MirAbi::Scalar(_))
    }

    pub fn is_integer(&self) -> bool {
        match &self.kind {
            MirTypeKind::Int | MirTypeKind::IntN { .. } => true,
            MirTypeKind::InlineRange { base, .. }
            | MirTypeKind::Tagged { inner: base, .. } => base.is_integer(),
            _ => false,
        }
    }

    pub fn is_float(&self) -> bool {
        match &self.kind {
            MirTypeKind::Float | MirTypeKind::Float32 => true,
            MirTypeKind::Tagged { inner, .. } => inner.is_float(),
            MirTypeKind::Quantity { base, .. } => base.is_float(),
            _ => false,
        }
    }

    pub fn is_numeric(&self) -> bool {
        self.is_integer() || self.is_float()
    }

    pub fn is_option(&self) -> bool {
        self.option_inner().is_some()
    }

    pub fn is_result(&self) -> bool {
        self.result_parts().is_some()
    }

    pub fn is_function(&self) -> bool {
        self.function_signature().is_some() || self.send_fn_signature().is_some()
    }

    pub fn is_nominal(&self) -> bool {
        matches!(
            &self.kind,
            MirTypeKind::Apply { .. } | MirTypeKind::TraitObject(_)
        )
    }

    pub fn is_list(&self) -> bool {
        self.list_element().is_some()
    }

    pub fn is_map(&self) -> bool {
        self.map_parts().is_some()
    }

    pub fn is_fixed_list(&self) -> bool {
        self.fixed_list_parts().is_some()
    }

    pub fn is_tuple(&self) -> bool {
        self.tuple_fields().is_some()
    }

    pub fn is_union(&self) -> bool {
        self.union_members().is_some()
    }

    pub fn is_measure(&self) -> bool {
        self.measure().is_some()
    }

    pub fn is_never(&self) -> bool {
        matches!(
            &self.kind,
            MirTypeKind::Apply { name, args } if args.is_empty() && name.name == crate::Syntax::TYPE_NEVER
        )
    }

    pub fn is_unit(&self) -> bool {
        matches!(
            &self.kind,
            MirTypeKind::Apply { name, args } if args.is_empty() && name.name == crate::Syntax::INTERNAL_UNIT_TYPE
        )
    }

    /// Whether the stored ABI projection still agrees with the canonical kind.
    pub fn has_valid_layout(&self) -> bool {
        self.layout == layout_for_kind(&self.kind)
    }
}

impl PartialEq for MirType {
    fn eq(&self, other: &Self) -> bool {
        self.same_checked_type(other)
    }
}

impl Eq for MirType {}

pub fn same_checked_type(left: &MirType, right: &MirType) -> bool {
    left.same_checked_type(right)
}

fn transparent_tag_inner(kind: &MirTypeKind) -> Option<&MirType> {
    match kind {
        MirTypeKind::Tagged { marker, inner } if !marker.identity_bearing() => Some(inner),
        _ => None,
    }
}


fn same_kind_identity(left: &MirTypeKind, right: &MirTypeKind) -> bool {
    match (left, right) {
        (MirTypeKind::Int, MirTypeKind::Int)
        | (MirTypeKind::Float, MirTypeKind::Float)
        | (MirTypeKind::Bool, MirTypeKind::Bool)
        | (MirTypeKind::String, MirTypeKind::String)
        | (MirTypeKind::Char, MirTypeKind::Char)
        | (MirTypeKind::Float32, MirTypeKind::Float32) => true,
        (MirTypeKind::List(left), MirTypeKind::List(right))
        | (MirTypeKind::Shared(left), MirTypeKind::Shared(right))
        | (MirTypeKind::Option(left), MirTypeKind::Option(right)) => {
            left.same_checked_type(right)
        }
        (
            MirTypeKind::Map {
                key: left_key,
                value: left_value,
            },
            MirTypeKind::Map {
                key: right_key,
                value: right_value,
            },
        ) => left_key.same_checked_type(right_key) && left_value.same_checked_type(right_value),
        (
            MirTypeKind::Result {
                ok: left_ok,
                err: left_err,
            },
            MirTypeKind::Result {
                ok: right_ok,
                err: right_err,
            },
        ) => left_ok.same_checked_type(right_ok) && left_err.same_checked_type(right_err),
        (MirTypeKind::Fn(left), MirTypeKind::Fn(right)) => left.same_identity(right),
        (
            MirTypeKind::SendFn {
                params: left_params,
                ret: left_ret,
            },
            MirTypeKind::SendFn {
                params: right_params,
                ret: right_ret,
            },
        ) => {
            left_params.len() == right_params.len()
                && left_params
                    .iter()
                    .zip(right_params)
                    .all(|(left, right)| left.same_checked_type(right))
                && match (left_ret, right_ret) {
                    (Some(left), Some(right)) => left.same_checked_type(right),
                    (None, None) => true,
                    _ => false,
                }
        }
        (
            MirTypeKind::Apply {
                name: left_name,
                args: left_args,
            },
            MirTypeKind::Apply {
                name: right_name,
                args: right_args,
            },
        ) => {
            left_name.id == right_name.id
                && left_args.len() == right_args.len()
                && left_args
                    .iter()
                    .zip(right_args)
                    .all(|(left, right)| left.same_checked_type(right))
        }
        (MirTypeKind::TraitObject(left), MirTypeKind::TraitObject(right)) => {
            left.len() == right.len()
                && left.iter().zip(right).all(|(left, right)| left.id == right.id)
        }
        (MirTypeKind::Tuple(left), MirTypeKind::Tuple(right)) => {
            left.len() == right.len()
                && left.iter().zip(right).all(|((left_name, left), (right_name, right))| {
                    left_name == right_name && left.same_checked_type(right)
                })
        }
        (
            MirTypeKind::FixedList {
                elem: left_elem,
                len: left_len,
            },
            MirTypeKind::FixedList {
                elem: right_elem,
                len: right_len,
            },
        ) => left_elem.same_checked_type(right_elem) && left_len == right_len,
        (
            MirTypeKind::IntN {
                signed: left_signed,
                bits: left_bits,
            },
            MirTypeKind::IntN {
                signed: right_signed,
                bits: right_bits,
            },
        ) => left_signed == right_signed && left_bits == right_bits,
        (
            MirTypeKind::InlineRange {
                base: left_base,
                lo: left_lo,
                hi: left_hi,
            },
            MirTypeKind::InlineRange {
                base: right_base,
                lo: right_lo,
                hi: right_hi,
            },
        ) => {
            left_lo == right_lo
                && left_hi == right_hi
                && left_base.same_checked_type(right_base)
        }
        (
            MirTypeKind::Tagged {
                marker: left_marker,
                inner: left_inner,
            },
            MirTypeKind::Tagged {
                marker: right_marker,
                inner: right_inner,
            },
        ) => {
            left_marker == right_marker
                && left_inner.same_checked_type(right_inner)
        }
        (MirTypeKind::Union(left), MirTypeKind::Union(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| left.same_checked_type(right))
        }
        (
            MirTypeKind::Quantity {
                base: left_base,
                dimension: left_dimension,
            },
            MirTypeKind::Quantity {
                base: right_base,
                dimension: right_dimension,
            },
        ) => left_base.same_checked_type(right_base) && left_dimension == right_dimension,
        (MirTypeKind::Measure(left), MirTypeKind::Measure(right)) => left == right,
        _ => false,
    }
}

fn mir_result_name(ok: &MirType, err: &MirType) -> String {
    let default_error = err.nominal_name() == Some(crate::Syntax::TYPE_ERR);
    let unit_success = ok.is_unit();
    let error = (!default_error).then(|| err.display_name());
    if unit_success {
        error.map_or_else(
            || format!("{}{}", crate::Syntax::TYPE_FALLIBLE_SEP, crate::Syntax::TYPE_ERR),
            |error| format!("{}{}", crate::Syntax::TYPE_FALLIBLE_SEP, error),
        )
    } else if let Some(error) = error {
        format!(
            "{} {}{}",
            ok.display_name(),
            crate::Syntax::TYPE_FALLIBLE_SEP,
            error
        )
    } else {
        ok.display_name()
    }
}

fn mir_int_spelling(signed: bool, bits: u8) -> String {
    format!("{}{}", if signed { 'I' } else { 'U' }, bits)
}

fn layout_for_kind(kind: &MirTypeKind) -> MirLayout {
    let scalar = |kind, bytes| MirLayout {
        abi: MirAbi::Scalar(kind),
        size: MirSize::Static(bytes),
        align: MirSize::Static(bytes.min(8)),
    };
    match kind {
        MirTypeKind::Int => MirLayout {
            abi: MirAbi::Scalar(MirScalarKind::Int),
            size: MirSize::Dynamic,
            align: MirSize::Static(8),
        },
        MirTypeKind::Float => scalar(MirScalarKind::Float, 8),
        MirTypeKind::Float32 => scalar(MirScalarKind::Float32, 4),
        MirTypeKind::Bool => scalar(MirScalarKind::Bool, 1),
        MirTypeKind::Char => scalar(MirScalarKind::Char, 4),
        MirTypeKind::IntN { bits, .. } => {
            scalar(MirScalarKind::Int, u64::from((*bits).max(8) / 8))
        }
        MirTypeKind::InlineRange { base, .. }
        | MirTypeKind::Tagged { inner: base, .. }
        | MirTypeKind::Quantity { base, .. } => base.layout,
        MirTypeKind::Fn(_) | MirTypeKind::SendFn { .. } => MirLayout {
            abi: MirAbi::Function,
            size: MirSize::Dynamic,
            align: MirSize::Static(8),
        },
        MirTypeKind::String
        | MirTypeKind::FixedList { .. }
        | MirTypeKind::List(_)
        | MirTypeKind::Map { .. } => MirLayout {
            abi: MirAbi::Sequence,
            size: MirSize::Dynamic,
            align: MirSize::Static(8),
        },
        MirTypeKind::Apply { name, args }
            if args.is_empty() && name.name == crate::Syntax::TYPE_NEVER =>
        {
            MirLayout {
                abi: MirAbi::Never,
                size: MirSize::Static(0),
                align: MirSize::Static(1),
            }
        }
        MirTypeKind::Apply { name, args }
            if (name.name == "Atomic"
                || name.name.ends_with("::Atomic")
                || name.name.ends_with(".Atomic"))
                && args.len() == 1
                && matches!(
                    args[0].kind(),
                    MirTypeKind::Bool
                        | MirTypeKind::Int
                        | MirTypeKind::IntN {
                            signed: true,
                            bits: 32,
                        }
                        | MirTypeKind::IntN {
                            signed: false,
                            bits: 32,
                        }
                        | MirTypeKind::IntN {
                            signed: false,
                            bits: 64,
                        }
                ) =>
        {
            // The semantic value may be a tagged/default-Int handle, but the
            // checked Atomic carrier is always one 64-bit machine word.
            MirLayout {
                abi: MirAbi::Scalar(MirScalarKind::Int),
                size: MirSize::Static(8),
                align: MirSize::Static(8),
            }
        }
        MirTypeKind::Apply { .. }
        | MirTypeKind::TraitObject(_) => MirLayout {
            abi: MirAbi::Nominal,
            size: MirSize::Dynamic,
            align: MirSize::Static(8),
        },
        MirTypeKind::Option(_)
        | MirTypeKind::Result { .. }
        | MirTypeKind::Tuple(_)
        | MirTypeKind::Union(_)
        | MirTypeKind::Shared(_) => MirLayout {
            abi: MirAbi::Aggregate,
            size: MirSize::Dynamic,
            align: MirSize::Static(8),
        },
        MirTypeKind::Measure(_) => MirLayout {
            abi: MirAbi::Dynamic,
            size: MirSize::Dynamic,
            align: MirSize::Static(8),
        },
    }
}



#[derive(Debug, Clone)]
pub struct MirFieldRow {
    pub id: MirFieldId,
    pub owner: MirTypeId,
    pub field: MirField,
}
#[derive(Debug, Clone)]
pub struct MirField {
    pub id: MirFieldId,
    pub name: String,
    /// Canonical per-projection field names. Adapters select the requested
    /// projection from this checked record instead of re-reading marker syntax.
    pub shape_names: ShapeFieldNames,
    /// True when the checked Codable contract omits this field.
    pub skip: bool,
    pub ty: MirType,
    pub span: Span,
    pub public: bool,
    pub package_public: bool,
    pub computed: bool,
    pub has_default: bool,
}

#[derive(Debug, Clone)]
pub struct MirVariant {
    pub name: String,
    /// Checked Codable wire name; adapters must not derive it from `name`.
    pub wire_name: String,
    pub span: Span,
    pub payload: MirVariantPayload,
    pub discriminant: Option<i64>,
}

#[derive(Debug, Clone)]
pub enum MirVariantPayload {
    Unit,
    Single(MirType),
    Named(Vec<MirField>),
}

#[derive(Debug, Clone)]
pub enum MirTypeDefKind {
    Struct {
        fields: Vec<MirField>,
        methods: Vec<MirFunctionId>,
    },
    Enum {
        variants: Vec<MirVariant>,
        methods: Vec<MirFunctionId>,
    },
    Distinct {
        base: MirType,
        range: Option<(i64, i64)>,
    },
    Alias {
        target: MirType,
    },
    UnitFamily {
        members: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirImportKind {
    File { path: String },
    Module { path: String },
    Unqualified {
        module: MirModuleId,
        items: Vec<MirImportItem>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirImportItem {
    pub original: String,
    pub local: String,
    pub item: MirItemRef,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirImport {
    pub id: MirImportId,
    pub module: MirModuleId,
    pub visibility: MirVisibility,
    pub alias: String,
    pub kind: MirImportKind,
    pub span: Span,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MirItemRef {
    Type(MirTypeId),
    Trait(MirTraitId),
    Function(MirFunctionId),
    Constant(MirConstantId),
    Impl(MirImplId),
    Foreign(MirForeignId),
    Import(MirImportId),
}

#[derive(Debug, Clone)]
pub struct MirModule {
    pub id: MirModuleId,
    pub key: String,
    pub name: String,
    pub path: String,
    pub source_file: MirSourceFileId,
    pub imports: Vec<MirImportId>,
    /// Checked inline-module children.  Keeping this relation separate from
    /// `item_order` avoids making module references masquerade as declarations.
    pub children: Vec<MirModuleId>,
    pub item_order: Vec<MirItemRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirStructLayout {
    C,
    /// C layout with an explicit checked alignment in bytes. The target bit is
    /// retained so adapters cannot erase the expert opt-in mode.
    CAligned {
        alignment: u64,
        target: bool,
    },
    Columnar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirSerdeAttributeKind {
    RenameAll,
    Tag,
    Untagged,
    DenyUnknownFields,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirSerdeAttribute {
    pub kind: MirSerdeAttributeKind,
    pub value: Option<String>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirCliBinding {
    pub name: String,
    pub function: MirFunctionId,
    pub span: Span,
    pub markers: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct MirTypeDef {
    pub id: MirTypeId,
    pub module: MirModuleId,
    pub key: String,
    pub name: String,
    pub span: Span,
    pub public: bool,
    pub package_public: bool,
    pub generic_params: Vec<MirGenericParam>,
    pub derives: Vec<MirTraitId>,
    pub auto_derive_default: bool,
    /// Sema proved the automatic Printable capability for this exact nominal type.
    pub auto_printable: bool,
    pub published_schema: bool,
    pub single_use: bool,
    pub must_use: bool,
    pub layout: Option<MirStructLayout>,
    /// Exact sema-checked requested/effective alignment and target fact identity.
    pub layout_alignment: Option<LayoutAlignmentFact>,
    pub serde: Vec<MirSerdeAttribute>,
    pub cli_bindings: Vec<MirCliBinding>,
    /// D-SHAPE-PROJECT1: the `#CLI` input rows of this struct, identical to the
    /// rows an entry `fn run(args: T)` derives, so `args.decode<T>()` and
    /// `T.merge` build the same builder spec and print the same help.
    pub cli: Option<MirCliEntry>,
    pub ownership: MirOwnershipMode,
    /// Sema-checked recursive-layout edge keys owned by this declaration.
    /// Rust emission consumes this fact without re-inferring recursion.
    pub boxed_edges: Vec<String>,
    pub kind: MirTypeDefKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirAssociatedTypeDecl {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct MirTraitMethod {
    pub id: MirTraitMethodId,
    pub name: String,
    pub span: Span,
    pub self_access: Option<MirAccess>,
    pub params: Vec<MirParam>,
    pub declared_return: Option<MirType>,
    pub return_type: MirType,
    pub failure: MirFailureCarrier,
    pub effects: MirEffectFacts,
    pub is_pure: bool,
    pub return_view_provenance: Option<BTreeMap<Vec<String>, MirViewProvenance>>,
    pub declared_return_view_provenance: Option<BTreeMap<Vec<String>, MirViewProvenance>>,
    pub default: Option<MirFunctionId>,
}

#[derive(Debug, Clone)]
pub struct MirTraitDef {
    pub id: MirTraitId,
    pub module: MirModuleId,
    pub key: String,
    pub name: String,
    pub span: Span,
    pub visibility: MirVisibility,
    pub associated_types: Vec<MirAssociatedTypeDecl>,
    pub methods: Vec<MirTraitMethod>,
}

#[derive(Debug, Clone)]
pub struct MirAssociatedTypeValue {
    pub name: String,
    pub ty: MirType,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct MirImplDef {
    pub id: MirImplId,
    pub module: MirModuleId,
    pub key: String,
    pub span: Span,
    pub self_type: MirType,
    pub trait_ref: Option<MirTraitRef>,
    pub associated_types: Vec<MirAssociatedTypeValue>,
    pub methods: Vec<MirFunctionId>,
    pub delegation: Option<MirFieldId>,
    pub compiler_generated: bool,
    pub serde: Option<MirSerdeCodec>,
    pub operator_rhs: Option<MirType>,
    pub operator_marker: Option<crate::AST::OperatorMarker>,
    pub target_os: Option<crate::OSTarget::OSTarget>,
    pub target_applicability: MirTargetApplicability,
}

#[derive(Debug, Clone)]
pub struct MirConstantDef {
    pub id: MirConstantId,
    pub module: MirModuleId,
    pub key: String,
    pub name: String,
    pub span: Span,
    pub visibility: MirVisibility,
    pub ty: MirType,
    pub value: MirConstant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirFailureCarrier {
    Infallible,
    Result { success: MirType, error: MirType },
    Optional { value: MirType },
    Diverges { value: MirType },
}


#[derive(Debug, Clone, Default)]
pub struct MirEffectFacts {
    pub direct: BTreeSet<String>,
    pub solved: BTreeSet<String>,
    pub call_edges: BTreeSet<String>,
    pub maximal: bool,
    pub direct_spans: BTreeMap<String, Span>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MirTargetApplicability {
    pub rust_aot: bool,
    pub cranelift: bool,
    pub interpreter: bool,
    pub web: bool,
}

#[derive(Debug, Clone, Default)]
pub struct MirCaptureFacts {
    pub escapes: bool,
    pub needs_fn_mut: bool,
    pub mutable: BTreeSet<String>,
    pub cloned: BTreeSet<String>,
    pub frozen: BTreeSet<String>,
    pub materialized: BTreeSet<String>,
    pub moved: BTreeSet<String>,
    pub frame_schedule: Option<crate::ResourceSchedule::JetFrameSchedule>,
    pub frame_schedule_derivation: Option<crate::Facts::DerivationRef>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirGenericParam {
    pub name: String,
    pub bounds: Vec<MirTraitRef>,
}
/// Checked generator ABI. Adapters provide the hidden yield sink mechanically;
/// source parameters remain unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirGeneratorFacts {
    pub item: MirType,
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirUnsafeGate {
    pub file: String,
    pub line: u32,
    pub reason: String,
    pub enabled: bool,
    pub fenced: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirKernelMode {
    Parallel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirKernelFacts {
    pub mode: MirKernelMode,
    pub bounds: bool,
    pub alias_free: bool,
    pub captures: bool,
    pub race_free: bool,
    pub barriers_uniform: bool,
    pub control_flow: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MirViewSource {
    Receiver,
    Parameter(usize),
    Static { module_path: String, name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MirViewProjection {
    Field(String),
    Index,
    Range,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MirViewSourcePath {
    pub source: MirViewSource,
    pub projections: Vec<MirViewProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirViewProvenance {
    pub sources: BTreeSet<MirViewSourcePath>,
    pub mutable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirWebParamField {
    pub field: String,
    pub parameter: String,
    pub ty: MirType,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirWebParamReconstruction {
    pub local: String,
    pub ty: MirType,
    pub fields: Vec<MirWebParamField>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MirPreludeCallId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirPreludeTypeArg {
    Type(MirType),
    HostUsize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MirFieldId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MirSourceFileId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MirSiteId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirSourceFile {
    pub id: MirSourceFileId,
    pub path: String,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirPreludeFamily {
    MathBuiltin,
    PreciseBuiltin,
    BuiltinMethod,
    HostBorrowCallback,
    Overflow,
    HandleMethod,
    ClosureMethod,
    ColumnarAccess,
    StaticPrelude,
    Host,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirPreludeAbi {
    Value,
    Aggregate,
    Control,
    Effect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirSymbol {
    Prelude(String),
    Runtime(String),
}

impl MirSymbol {
    pub fn name(&self) -> &str {
        match self {
            Self::Prelude(name) | Self::Runtime(name) => name,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirCallFallibility {
    Infallible,
    Failure(MirFailureCarrier),
}
#[derive(Debug, Clone)]
pub enum MirConversion {
    Transparent,
    /// A scalar numeric cast already proven total by sema.
    NumericCast,
    /// Preserve a checked callable already represented as `SendFn`.
    ///
    /// Ordinary `Fn` values, including capture-free closures, must be
    /// constructed directly with the `SendFn` carrier at their origin.
    SendFn,
    Prelude {
        call: MirPreludeCallId,
        location: MirPanicLoc,
        fallibility: MirCallFallibility,
    },
}


#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirCallSignature {
    pub arity: usize,
    pub max_arity: usize,
    pub borrow_mask: Vec<bool>,
}

/// One authority checkpoint attached to a checked Prelude route.  The import
/// fact is the canonical operation identity; `authority_argument` identifies
/// the explicit Authority value on a loader call, while `inherited` marks
/// calls that reuse the decision stored by the loaded handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirAuthorityDecision {
    pub import: crate::Authority::HostImportFact,
    pub authority_argument: Option<usize>,
    pub inherited: bool,
}

impl MirAuthorityDecision {
    pub fn plugin_load() -> Self {
        Self {
            import: crate::Authority::HostImportFact::new(
                "core.plugin.authority",
                "core.plugin.host-import",
                "FS.Read",
                ["String".to_string(), "Authority".to_string()],
                Some("Plugin".to_string()),
            ),
            authority_argument: Some(1),
            inherited: false,
        }
    }

    pub fn plugin_call() -> Self {
        Self {
            import: Self::plugin_load().import,
            authority_argument: None,
            inherited: true,
        }
    }
}

impl Default for MirAuthorityDecision {
    fn default() -> Self {
        Self::plugin_load()
    }
}

/// Resolve the authority fact shared by the plugin loader and all calls on
/// the resulting handle.  Keeping this constructor in foundation prevents
/// adapters from inventing operation-specific grant names.
pub fn plugin_authority_import_fact() -> crate::Authority::HostImportFact {
    MirAuthorityDecision::plugin_load().import
}

/// One checked database table relation attached to a query route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirDbTableFact {
    pub table_id: String,
    pub read: bool,
    pub write: bool,
}

/// Checked database query metadata carried by the canonical MIR Prelude row.
/// Runtime adapters receive this immutable source/table projection and append
/// only observed driver facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirDbQueryMetadata {
    pub source_file: MirSourceFileId,
    pub source_path: String,
    pub source_span: Span,
    pub statement_identity: String,
    pub table_facts: Vec<MirDbTableFact>,
}

impl MirDbQueryMetadata {
    /// Stable length-prefixed carrier for the final Prelude/driver boundary.
    pub fn to_wire(&self) -> String {
        fn field(output: &mut String, value: &str) {
            output.push_str(&value.len().to_string());
            output.push(':');
            output.push_str(value);
        }
        let mut output = String::from("JDB1:");
        field(&mut output, &self.source_path);
        field(&mut output, &self.source_file.0.to_string());
        field(&mut output, &self.source_span.start.to_string());
        field(&mut output, &self.source_span.end.to_string());
        field(&mut output, &self.statement_identity);
        output.push_str(&self.table_facts.len().to_string());
        output.push(':');
        for fact in &self.table_facts {
            field(&mut output, &fact.table_id);
            field(&mut output, if fact.read { "1" } else { "0" });
            field(&mut output, if fact.write { "1" } else { "0" });
        }
        output
    }
}

/// A recursively closed Component Model value shape.
///
/// This descriptor is semantic MIR data, not a backend encoding.  TIR creates
/// it once from checked types; AOT, JIT, interpreter, and web adapters consume
/// the same row and must not rediscover a wire shape from rendered Rust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentTypeDescriptor {
    Int,
    Float,
    Bool,
    String,
    List(Box<Self>),
    Option(Box<Self>),
    Result {
        ok: Box<Self>,
        err: Box<Self>,
    },
    Record {
        name: String,
        fields: Vec<(usize, String, Self)>,
    },
}

impl ComponentTypeDescriptor {
    pub fn wire(&self) -> String {
        let mut out = String::new();
        self.write_wire(&mut out);
        out
    }

    fn write_wire(&self, out: &mut String) {
        match self {
            Self::Int => out.push('i'),
            Self::Float => out.push('f'),
            Self::Bool => out.push('b'),
            Self::String => out.push('s'),
            Self::List(inner) => {
                out.push('l');
                inner.write_wire(out);
            }
            Self::Option(inner) => {
                out.push('o');
                inner.write_wire(out);
            }
            Self::Result { ok, err } => {
                out.push('q');
                ok.write_wire(out);
                err.write_wire(out);
            }
            Self::Record { name, fields } => {
                out.push('r');
                write_component_text(out, name);
                out.push_str(&fields.len().to_string());
                out.push(':');
                for (index, field, ty) in fields {
                    out.push_str(&index.to_string());
                    out.push(':');
                    write_component_text(out, field);
                    ty.write_wire(out);
                }
            }
        }
    }

    pub fn wit_type(&self) -> String {
        match self {
            Self::Int => "s64".to_string(),
            Self::Float => "f64".to_string(),
            Self::Bool => "bool".to_string(),
            Self::String => "string".to_string(),
            Self::List(inner) => format!("list<{}>", inner.wit_type()),
            Self::Option(inner) => format!("option<{}>", inner.wit_type()),
            Self::Result { ok, err } => {
                format!("result<{}, {}>", ok.wit_type(), err.wit_type())
            }
            Self::Record { name, .. } => name.replace('_', "-"),
        }
    }

    fn append_wit_definitions(
        &self,
        seen: &mut std::collections::BTreeSet<String>,
        definitions: &mut Vec<String>,
    ) {
        match self {
            Self::List(inner) | Self::Option(inner) => {
                inner.append_wit_definitions(seen, definitions)
            }
            Self::Result { ok, err } => {
                ok.append_wit_definitions(seen, definitions);
                err.append_wit_definitions(seen, definitions);
            }
            Self::Record { name, fields } => {
                let name = name.replace('_', "-");
                if !seen.insert(name.clone()) {
                    return;
                }
                for (_, _, ty) in fields {
                    ty.append_wit_definitions(seen, definitions);
                }
                let fields = fields
                    .iter()
                    .map(|(_, field, ty)| {
                        format!("  {}: {},", field.replace('_', "-"), ty.wit_type())
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                definitions.push(format!("record {name} {{\n{fields}\n}}\n"));
            }
            Self::Int | Self::Float | Self::Bool | Self::String => {}
        }
    }
}

fn write_component_text(out: &mut String, text: &str) {
    out.push_str(&text.len().to_string());
    out.push(':');
    out.push_str(text);
}

/// Complete checked Component Model signature and its stable structural wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentSignatureDescriptor {
    pub params: Vec<ComponentTypeDescriptor>,
    pub result: ComponentTypeDescriptor,
}

impl ComponentSignatureDescriptor {
    pub fn wire(&self) -> String {
        let mut out = format!("C{}:", self.params.len());
        for param in &self.params {
            param.write_wire(&mut out);
        }
        out.push('r');
        self.result.write_wire(&mut out);
        out
    }

    pub fn wit_definitions(&self) -> Vec<String> {
        let mut seen = std::collections::BTreeSet::new();
        let mut definitions = Vec::new();
        for param in &self.params {
            param.append_wit_definitions(&mut seen, &mut definitions);
        }
        self.result
            .append_wit_definitions(&mut seen, &mut definitions);
        definitions
    }
}

#[derive(Debug, Clone)]
pub struct MirPreludeCall {
    pub id: MirPreludeCallId,
    pub family: MirPreludeFamily,
    pub module: String,
    pub member: String,
    pub symbol: MirSymbol,
    pub signature: MirCallSignature,
    pub effect: Option<Effect>,
    pub fallibility: MirCallFallibility,
    pub abi: MirPreludeAbi,
    /// One canonical authority decision, if this route crosses a capability
    /// boundary.  `None` is the ordinary pure/runtime route.
    pub authority: Option<MirAuthorityDecision>,
    /// Checked DB source/table metadata, if this route is a DB sink.
    pub db_metadata: Option<MirDbQueryMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirFieldOwner {
    Type(MirTypeId),
    Function(MirFunctionId),
}


/// The stable order of the semantic MIR optimization pipeline.
///
/// These IDs are semantic metadata.  They intentionally do not mention a
/// backend, target compiler, or machine representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MirOptimizationPassId {
    LegalityVerification,
    UnreachableBlockElimination,
    CfgSimplification,
    ExactConstantFolding,
    BoundsCheckElimination,
    DeadPureValueElimination,
    CanonicalLoopFacts,
}

impl MirOptimizationPassId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LegalityVerification => "legality-verification",
            Self::UnreachableBlockElimination => "unreachable-block-elimination",
            Self::CfgSimplification => "cfg-simplification",
            Self::ExactConstantFolding => "exact-constant-folding",
            Self::BoundsCheckElimination => "bounds-check-elimination",
            Self::DeadPureValueElimination => "dead-pure-value-elimination",
            Self::CanonicalLoopFacts => "canonical-loop-facts",
        }
    }
}

/// The user-facing compiler-decision categories.  These rows are facts
/// produced by the canonical MIR/JIT producers; inspect only renders them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MirDecisionKind {
    Tier,
    Inline,
    Vectorize,
    Parallel,
    Copy,
    Bounds,
    Deopt,
    Unreachable,
    LoopInvariant,
}

impl MirDecisionKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tier => "tier",
            Self::Inline => "inline",
            Self::Vectorize => "vectorize",
            Self::Parallel => "parallel",
            Self::Copy => "copy",
            Self::Bounds => "bounds",
            Self::Deopt => "deopt",
            Self::Unreachable => "unreachable",
            Self::LoopInvariant => "loop-invariant",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MirDecisionDisposition {
    Accepted,
    Rejected,
    Selected,
    NotAttempted,
    Unavailable,
}

impl MirDecisionDisposition {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Selected => "selected",
            Self::NotAttempted => "not-attempted",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Build/source/run identity attached to every materialized decision row.
///
/// `DerivationIdentity` is intentionally a compact four-field relation used
/// throughout the evidence plane.  This wider identity keeps the decision
/// view honest about the configuration, profile, implementation, and
/// artifact that produced a row while still linking that row to the shared
/// derivation table.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MirDecisionIdentity {
    pub source: String,
    pub configuration: String,
    pub profile: String,
    pub target: String,
    pub implementation: String,
    pub artifact: String,
    pub run: String,
}

impl MirDecisionIdentity {
    /// Derivation records use the compact build component for compatibility.
    /// Length framing prevents boundary collisions in the concatenated
    /// decision identity.
    pub fn derivation_identity(&self) -> DerivationIdentity {
        let mut build = Vec::new();
        for value in [
            self.configuration.as_str(),
            self.profile.as_str(),
            self.implementation.as_str(),
            self.artifact.as_str(),
        ] {
            build.extend_from_slice(&(value.len() as u64).to_le_bytes());
            build.extend_from_slice(value.as_bytes());
        }
        DerivationIdentity::new(
            self.source.clone(),
            crate::SHA256::sha256_hex(&build),
            self.run.clone(),
            self.target.clone(),
        )
    }

    /// Derive the complete identity from checked MIR/package facts.  This
    /// method reads facts already carried by MIR; it does not inspect files,
    /// rerun a proof, or consult an execution engine.
    pub fn from_program(
        program: &MirProgram,
        artifact: Option<MirArtifactId>,
        profile: impl Into<String>,
        run: impl Into<String>,
    ) -> Self {
        let artifact_plan = artifact
            .and_then(|id| program.artifacts.iter().find(|candidate| candidate.id == id))
            .or_else(|| program.artifacts.first());
        let mut source_material = Vec::new();
        let mut source_files = program.source_files.iter().collect::<Vec<_>>();
        source_files.sort_by(|left, right| left.path.cmp(&right.path).then(left.id.cmp(&right.id)));
        for source in source_files {
            append_decision_identity_frame(&mut source_material, &source.path);
            append_decision_identity_frame(
                &mut source_material,
                &crate::SHA256::sha256_hex(source.source.as_bytes()),
            );
        }
        if source_material.is_empty() {
            append_decision_identity_frame(&mut source_material, &program.package_identity);
        }
        let source = crate::SHA256::sha256_hex(&source_material);

        let mut runtime_parts = program
            .facts
            .runtime_parts
            .iter()
            .map(|part| part.as_str())
            .collect::<Vec<_>>();
        runtime_parts.sort_unstable();
        let configuration = format!(
            "package={};edition={};os={};layer={};allocator={};runtime={};arrow={};hardware={}",
            program.package_identity,
            program.facts.edition,
            program.facts.active_os,
            program.facts.inferred_layer,
            program.facts.allocator,
            runtime_parts.join(","),
            program.facts.uses_arrow,
            program.facts.hardware_profile_id,
        );

        let profile = {
            let profile = profile.into();
            if profile.is_empty() {
                artifact_plan
                    .map(|plan| plan.mode.as_str().to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            } else {
                profile
            }
        };
        let target = format!(
            "triple={};artifact={};machine={}",
            program
                .facts
                .target_dossier
                .machine
                .as_deref()
                .map_or("hosted-default", |machine| machine.triple.as_str()),
            artifact_plan.map_or("unknown", |plan| plan.target.as_str()),
            program
                .facts
                .target_dossier
                .machine
                .as_deref()
                .map_or("hosted-default", |machine| machine.name.as_str()),
        );
        let dossier = &program.facts.target_dossier;
        let implementation = format!(
            "compiler={};provider={};closure={};linker={};tier={};environment={};dependencies={}",
            dossier.compiler_identity,
            artifact_plan.map_or(dossier.provider_identity.as_str(), |plan| {
                plan.provider_identity.as_str()
            }),
            artifact_plan.map_or(dossier.closure_identity.as_str(), |plan| {
                plan.closure_identity.as_str()
            }),
            dossier.linker_identity,
            dossier.tier_identity,
            dossier.environment_identity,
            dossier.dependency_identity,
        );
        let artifact = artifact_plan.map_or_else(
            || artifact.map_or_else(|| "unavailable".to_string(), |id| format!("artifact:{}", id.0)),
            |plan| {
                if plan.artifact_identity.is_empty() {
                    format!("artifact:{}", plan.id.0)
                } else {
                    plan.artifact_identity.clone()
                }
            },
        );
        let run = {
            let run = run.into();
            if run.is_empty() {
                "not-observed".to_string()
            } else {
                run
            }
        };
        Self {
            source,
            configuration,
            profile,
            target,
            implementation,
            artifact,
            run,
        }
    }
}

fn append_decision_identity_frame(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(&(value.len() as u64).to_le_bytes());
    out.extend_from_slice(value.as_bytes());
}

/// One canonical source-linked compiler decision.
///
/// Producers append rows when they have a checked answer.  Consumers must not
/// infer a missing row as a rejection or rerun the producer's proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirDecisionEdit {
    pub span: Span,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirDecisionRow {
    pub id: u64,
    pub kind: MirDecisionKind,
    pub disposition: MirDecisionDisposition,
    pub function: Option<MirFunctionId>,
    pub function_name: String,
    pub span: Span,
    pub rule: String,
    pub reason: String,
    pub producer: String,
    pub evidence: String,
    pub edit: Option<MirDecisionEdit>,
    /// Shared checked derivation relation.  Producers leave this empty until
    /// the owner materializes the one ledger for a command projection.
    pub derivation: Option<DerivationRef>,
    /// Full decision identity, including fields not present in the compact
    /// derivation relation.
    pub identity: Option<MirDecisionIdentity>,
    /// Evidence method is retained beside the row rather than inferred from
    /// its disposition.
    pub evidence_method: Option<DerivationMethod>,
    /// Applicability of the linked derivation record.  This carries stale,
    /// redacted, unsupported, and budget-exhausted states without overloading
    /// the producer's accepted/rejected answer.
    pub derivation_disposition: Option<DerivationDisposition>,
}

impl MirDecisionRow {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        kind: MirDecisionKind,
        disposition: MirDecisionDisposition,
        function: Option<MirFunctionId>,
        function_name: impl Into<String>,
        span: Span,
        rule: impl Into<String>,
        reason: impl Into<String>,
        producer: impl Into<String>,
        evidence: impl Into<String>,
    ) -> Self {
        let function_name = function_name.into();
        let rule = rule.into();
        let reason = reason.into();
        let producer = producer.into();
        let evidence = evidence.into();
        let identity = format!(
            "{}|{}|{}|{}|{}|{}|{}",
            kind.as_str(),
            disposition.as_str(),
            function.map(|id| id.0).unwrap_or_default(),
            span.start,
            span.end,
            rule,
            reason,
        );
        Self {
            id: stable_id("mir-decision", &identity),
            kind,
            disposition,
            function,
            function_name,
            span,
            rule,
            reason,
            producer,
            evidence,
            edit: None,
            derivation: None,
            identity: None,
            evidence_method: None,
            derivation_disposition: None,
        }
    }
    pub fn with_edit(
        mut self,
        span: Span,
        replacement: impl Into<String>,
    ) -> Self {
        self.edit = Some(MirDecisionEdit {
            span,
            replacement: replacement.into(),
        });
        self
    }
}

/// The one decision ledger used by compiler, linter, CLI, and receipt views.
///
/// `from_rows` is the only materialization point: it interns each row's
/// checked derivation and attaches the same reference and identity to every
/// consumer projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirDecisionLedger {
    pub identity: MirDecisionIdentity,
    pub rows: Vec<MirDecisionRow>,
    pub derivations: Vec<DerivationRecord>,
}

impl MirDecisionLedger {
    pub fn from_rows(
        identity: MirDecisionIdentity,
        rows: impl IntoIterator<Item = MirDecisionRow>,
    ) -> Self {
        let mut canonical_rows: Vec<MirDecisionRow> = Vec::new();
        for row in rows {
            let existing_index = canonical_rows
                .iter()
                .position(|existing| existing.id == row.id);
            if let Some(existing_index) = existing_index {
                let existing = &mut canonical_rows[existing_index];
                if existing.edit.is_none() {
                    existing.edit = row.edit.clone();
                }
                if existing.evidence.is_empty() {
                    existing.evidence = row.evidence.clone();
                }
                continue;
            }
            canonical_rows.push(row);
        }

        let mut derivations = Vec::with_capacity(canonical_rows.len());
        for row in &mut canonical_rows {
            let method = row.evidence_method.unwrap_or_else(|| {
                if row.evidence.contains("proof") || row.evidence.contains("static-flow") {
                    DerivationMethod::FormalProof
                } else {
                    DerivationMethod::StaticDerivation
                }
            });
            let derivation_disposition = match row.disposition {
                MirDecisionDisposition::Unavailable => DerivationDisposition::Unavailable,
                MirDecisionDisposition::NotAttempted => DerivationDisposition::Unknown,
                MirDecisionDisposition::Accepted
                | MirDecisionDisposition::Rejected
                | MirDecisionDisposition::Selected => DerivationDisposition::Current,
            };
            let subject = format!(
                "{}:{}:{}-{}",
                row.kind.as_str(),
                row.function_name,
                row.span.start,
                row.span.end
            );
            let claim = format!(
                "{}:{}:{}",
                row.kind.as_str(),
                row.disposition.as_str(),
                row.rule
            );
            let derivation = DerivationRecord::new(
                subject,
                claim,
                row.producer.clone(),
                method,
                row.rule.clone(),
                [
                    row.reason.clone(),
                    row.evidence.clone(),
                    format!("row-id={}", row.id),
                ],
                identity.derivation_identity(),
            )
            .with_disposition(derivation_disposition);
            row.derivation = Some(derivation.reference());
            row.identity = Some(identity.clone());
            row.evidence_method = Some(method);
            row.derivation_disposition = Some(derivation_disposition);
            derivations.push(derivation);
        }
        Self {
            identity,
            rows: canonical_rows,
            derivations,
        }
    }
    /// Serialize the materialized ledger as the one transport payload shared
    /// by live snapshots and replay receipts.  The payload carries the full
    /// identity and each row's checked derivation reference; consumers parse
    /// this back into `MirDecisionLedger` instead of reconstructing claims.
    pub fn canonical_json(&self) -> String {
        let rows = self
            .rows
            .iter()
            .map(decision_row_json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"identity\":{},\"rows\":[{}],\"schema\":\"jet.mir.decisions.v1\"}}",
            decision_identity_json(&self.identity),
            rows,
        )
    }

    /// Parse and validate a transport payload emitted by `canonical_json`.
    /// Stable row ids, identities, evidence methods, and derivation references
    /// are checked against the canonical materializer before the ledger is
    /// returned to a view or receipt consumer.
    pub fn from_json(payload: &str) -> Result<Self, String> {
        let root = crate::JSON::parse_json(payload)
            .map_err(|()| "decision ledger payload is not valid JSON".to_string())?;
        let root_fields = match root {
            crate::DataTree::DataTree::Object(fields) => {
                fields.into_iter().collect::<BTreeMap<_, _>>()
            }
            _ => return Err("decision ledger payload must be an object".to_string()),
        };
        if root_fields.len() != 3
            || !matches!(
                root_fields.get("schema"),
                Some(crate::DataTree::DataTree::Text(schema))
                    if schema == "jet.mir.decisions.v1"
            )
        {
            return Err("decision ledger payload has an unsupported schema".to_string());
        }
        let identity = parse_decision_identity(
            root_fields
                .get("identity")
                .ok_or_else(|| "decision ledger payload has no identity".to_string())?,
        )?;
        let rows_value = root_fields
            .get("rows")
            .ok_or_else(|| "decision ledger payload has no rows".to_string())?;
        let crate::DataTree::DataTree::Array(rows_value) = rows_value else {
            return Err("decision ledger payload rows must be an array".to_string());
        };
        if rows_value.len() > 4096 {
            return Err("decision ledger payload has too many rows".to_string());
        }
        let mut rows = Vec::with_capacity(rows_value.len());
        for value in rows_value {
            rows.push(parse_decision_row(value)?);
        }
        let transported = rows.clone();
        let ledger = Self::from_rows(identity, rows);
        for row in &ledger.rows {
            let Some(source) = transported.iter().find(|candidate| candidate.id == row.id) else {
                return Err("decision ledger payload lost a row during materialization".to_string());
            };
            if source.identity.as_ref() != Some(&ledger.identity)
                || source.derivation.as_ref() != row.derivation.as_ref()
                || source.evidence_method != row.evidence_method
                || source.derivation_disposition != row.derivation_disposition
            {
                return Err(format!(
                    "decision ledger row {} has an untrusted derivation relation",
                    row.id
                ));
            }
        }
        if ledger.canonical_json() != payload {
            return Err("decision ledger payload is not canonical JSON".to_string());
        }
        Ok(ledger)
    }

    pub fn row(&self, id: u64) -> Option<&MirDecisionRow> {
        self.rows.iter().find(|row| row.id == id)
    }


    pub fn derivation(&self, reference: &DerivationRef) -> Option<&DerivationRecord> {
        self.derivations
            .iter()
            .find(|derivation| derivation.id == reference.id)
    }
}
fn decision_json_string(value: &str) -> String {
    format!("\"{}\"", crate::JSON::json_escape(value))
}

fn decision_identity_json(identity: &MirDecisionIdentity) -> String {
    format!(
        "{{\"artifact\":{},\"configuration\":{},\"implementation\":{},\"profile\":{},\"run\":{},\"source\":{},\"target\":{}}}",
        decision_json_string(&identity.artifact),
        decision_json_string(&identity.configuration),
        decision_json_string(&identity.implementation),
        decision_json_string(&identity.profile),
        decision_json_string(&identity.run),
        decision_json_string(&identity.source),
        decision_json_string(&identity.target),
    )
}

fn decision_span_json(span: Span) -> String {
    format!("{{\"end\":{},\"start\":{}}}", span.end, span.start)
}

fn decision_optional_identity_json(identity: Option<&MirDecisionIdentity>) -> String {
    identity.map_or_else(|| "null".to_string(), decision_identity_json)
}

fn decision_edit_json(edit: Option<&MirDecisionEdit>) -> String {
    edit.map_or_else(
        || "null".to_string(),
        |edit| {
            format!(
                "{{\"replacement\":{},\"span\":{}}}",
                decision_json_string(&edit.replacement),
                decision_span_json(edit.span),
            )
        },
    )
}

fn decision_derivation_ref_json(reference: Option<&DerivationRef>) -> String {
    reference.map_or_else(
        || "null".to_string(),
        |reference| format!("{{\"id\":{}}}", decision_json_string(&reference.id)),
    )
}

fn decision_row_json(row: &MirDecisionRow) -> String {
    let function = row.function.map_or_else(
        || "null".to_string(),
        |_| decision_json_string(&row.function_name),
    );
    let function_id = row.function.map_or_else(
        || "null".to_string(),
        |id| decision_json_string(&id.0.to_string()),
    );
    let evidence_method = row
        .evidence_method
        .map_or_else(|| "null".to_string(), |method| decision_json_string(method.as_str()));
    let derivation_disposition = row.derivation_disposition.map_or_else(
        || "null".to_string(),
        |disposition| decision_json_string(disposition.as_str()),
    );
    format!(
        "{{\"derivation_disposition\":{},\"derivation_ref\":{},\"disposition\":{},\"edit\":{},\"evidence\":{},\"evidence_method\":{},\"function\":{},\"function_id\":{},\"function_name\":{},\"id\":{},\"identity\":{},\"kind\":{},\"producer\":{},\"reason\":{},\"rule\":{},\"span\":{}}}",
        derivation_disposition,
        decision_derivation_ref_json(row.derivation.as_ref()),
        decision_json_string(row.disposition.as_str()),
        decision_edit_json(row.edit.as_ref()),
        decision_json_string(&row.evidence),
        evidence_method,
        function,
        function_id,
        decision_json_string(&row.function_name),
        decision_json_string(&row.id.to_string()),
        decision_optional_identity_json(row.identity.as_ref()),
        decision_json_string(row.kind.as_str()),
        decision_json_string(&row.producer),
        decision_json_string(&row.reason),
        decision_json_string(&row.rule),
        decision_span_json(row.span),
    )
}

fn decision_object(
    value: &crate::DataTree::DataTree,
    label: &str,
) -> Result<BTreeMap<String, crate::DataTree::DataTree>, String> {
    let crate::DataTree::DataTree::Object(fields) = value else {
        return Err(format!("{label} must be an object"));
    };
    let fields = fields.iter().cloned().collect::<BTreeMap<_, _>>();
    Ok(fields)
}

fn decision_text(
    fields: &BTreeMap<String, crate::DataTree::DataTree>,
    key: &str,
    label: &str,
) -> Result<String, String> {
    crate::JSON::json_get(
        &crate::DataTree::DataTree::Object(
            fields.iter().map(|(key, value)| (key.clone(), value.clone())).collect(),
        ),
        key,
    )
    .and_then(crate::JSON::json_str)
    .map(str::to_string)
    .ok_or_else(|| format!("{label} has invalid `{key}`"))
}

fn decision_u64(
    fields: &BTreeMap<String, crate::DataTree::DataTree>,
    key: &str,
    label: &str,
) -> Result<u64, String> {
    let value = fields
        .get(key)
        .ok_or_else(|| format!("{label} has invalid `{key}`"))?;
    match value {
        crate::DataTree::DataTree::Text(value) => value
            .parse::<u64>()
            .map_err(|_| format!("{label} has invalid `{key}`")),
        _ => Err(format!("{label} has invalid `{key}`")),
    }
}

fn decision_optional_u64(
    fields: &BTreeMap<String, crate::DataTree::DataTree>,
    key: &str,
    label: &str,
) -> Result<Option<u64>, String> {
    match fields.get(key) {
        Some(crate::DataTree::DataTree::Null) => Ok(None),
        Some(_) => decision_u64(fields, key, label).map(Some),
        None => Err(format!("{label} has no `{key}`")),
    }
}

fn parse_decision_identity(
    value: &crate::DataTree::DataTree,
) -> Result<MirDecisionIdentity, String> {
    let fields = decision_object(value, "decision identity")?;
    const KEYS: [&str; 7] = [
        "artifact",
        "configuration",
        "implementation",
        "profile",
        "run",
        "source",
        "target",
    ];
    if fields.len() != KEYS.len() || KEYS.iter().any(|key| !fields.contains_key(*key)) {
        return Err("decision identity has missing or unsafe fields".to_string());
    }
    Ok(MirDecisionIdentity {
        artifact: decision_text(&fields, "artifact", "decision identity")?,
        configuration: decision_text(&fields, "configuration", "decision identity")?,
        implementation: decision_text(&fields, "implementation", "decision identity")?,
        profile: decision_text(&fields, "profile", "decision identity")?,
        run: decision_text(&fields, "run", "decision identity")?,
        source: decision_text(&fields, "source", "decision identity")?,
        target: decision_text(&fields, "target", "decision identity")?,
    })
}

fn parse_decision_span(value: &crate::DataTree::DataTree) -> Result<Span, String> {
    let fields = decision_object(value, "decision span")?;
    if fields.len() != 2 || !fields.contains_key("start") || !fields.contains_key("end") {
        return Err("decision span has missing or unsafe fields".to_string());
    }
    let start = usize::try_from(decision_u64(&fields, "start", "decision span")?)
        .map_err(|_| "decision span start is out of range".to_string())?;
    let end = usize::try_from(decision_u64(&fields, "end", "decision span")?)
        .map_err(|_| "decision span end is out of range".to_string())?;
    Ok(Span::new(start, end))
}

fn parse_decision_kind(value: &str) -> Option<MirDecisionKind> {
    match value {
        "tier" => Some(MirDecisionKind::Tier),
        "inline" => Some(MirDecisionKind::Inline),
        "vectorize" => Some(MirDecisionKind::Vectorize),
        "parallel" => Some(MirDecisionKind::Parallel),
        "copy" => Some(MirDecisionKind::Copy),
        "bounds" => Some(MirDecisionKind::Bounds),
        "deopt" => Some(MirDecisionKind::Deopt),
        "unreachable" => Some(MirDecisionKind::Unreachable),
        "loop-invariant" => Some(MirDecisionKind::LoopInvariant),
        _ => None,
    }
}

fn parse_decision_disposition(value: &str) -> Option<MirDecisionDisposition> {
    match value {
        "accepted" => Some(MirDecisionDisposition::Accepted),
        "rejected" => Some(MirDecisionDisposition::Rejected),
        "selected" => Some(MirDecisionDisposition::Selected),
        "not-attempted" => Some(MirDecisionDisposition::NotAttempted),
        "unavailable" => Some(MirDecisionDisposition::Unavailable),
        _ => None,
    }
}

fn parse_decision_optional_identity(
    value: Option<&crate::DataTree::DataTree>,
) -> Result<Option<MirDecisionIdentity>, String> {
    match value {
        None | Some(crate::DataTree::DataTree::Null) => Ok(None),
        Some(value) => parse_decision_identity(value).map(Some),
    }
}

fn parse_decision_optional_edit(
    value: Option<&crate::DataTree::DataTree>,
) -> Result<Option<MirDecisionEdit>, String> {
    let Some(value) = value else {
        return Err("decision row has no `edit`".to_string());
    };
    if matches!(value, crate::DataTree::DataTree::Null) {
        return Ok(None);
    }
    let fields = decision_object(value, "decision edit")?;
    if fields.len() != 2 || !fields.contains_key("replacement") || !fields.contains_key("span") {
        return Err("decision edit has missing or unsafe fields".to_string());
    }
    Ok(Some(MirDecisionEdit {
        replacement: decision_text(&fields, "replacement", "decision edit")?,
        span: parse_decision_span(
            fields
                .get("span")
                .ok_or_else(|| "decision edit has no span".to_string())?,
        )?,
    }))
}

fn parse_decision_optional_ref(
    value: Option<&crate::DataTree::DataTree>,
) -> Result<Option<DerivationRef>, String> {
    let Some(value) = value else {
        return Err("decision row has no `derivation_ref`".to_string());
    };
    if matches!(value, crate::DataTree::DataTree::Null) {
        return Ok(None);
    }
    let fields = decision_object(value, "decision derivation reference")?;
    if fields.len() != 1 || !fields.contains_key("id") {
        return Err("decision derivation reference has missing or unsafe fields".to_string());
    }
    Ok(Some(DerivationRef::new(decision_text(
        &fields,
        "id",
        "decision derivation reference",
    )?)))
}

fn parse_decision_row(value: &crate::DataTree::DataTree) -> Result<MirDecisionRow, String> {
    let fields = decision_object(value, "decision row")?;
    const KEYS: [&str; 16] = [
        "derivation_disposition",
        "derivation_ref",
        "disposition",
        "edit",
        "evidence",
        "evidence_method",
        "function",
        "function_id",
        "function_name",
        "id",
        "identity",
        "kind",
        "producer",
        "reason",
        "rule",
        "span",
    ];
    if fields.len() != KEYS.len() || KEYS.iter().any(|key| !fields.contains_key(*key)) {
        return Err("decision row has missing or unsafe fields".to_string());
    }
    let kind_text = decision_text(&fields, "kind", "decision row")?;
    let kind = parse_decision_kind(&kind_text)
        .ok_or_else(|| format!("decision row has unsupported kind `{kind_text}`"))?;
    let disposition_text = decision_text(&fields, "disposition", "decision row")?;
    let disposition = parse_decision_disposition(&disposition_text)
        .ok_or_else(|| format!("decision row has unsupported disposition `{disposition_text}`"))?;
    let function_name = decision_text(&fields, "function_name", "decision row")?;
    let function = decision_optional_u64(&fields, "function_id", "decision row")?
        .map(MirFunctionId);
    let span = parse_decision_span(
        fields
            .get("span")
            .ok_or_else(|| "decision row has no span".to_string())?,
    )?;
    let mut row = MirDecisionRow::new(
        kind,
        disposition,
        function,
        function_name,
        span,
        decision_text(&fields, "rule", "decision row")?,
        decision_text(&fields, "reason", "decision row")?,
        decision_text(&fields, "producer", "decision row")?,
        decision_text(&fields, "evidence", "decision row")?,
    );
    let id = decision_u64(&fields, "id", "decision row")?;
    if row.id != id {
        return Err(format!("decision row id {id} does not match its canonical fields"));
    }
    row.edit = parse_decision_optional_edit(fields.get("edit"))?;
    row.derivation = parse_decision_optional_ref(fields.get("derivation_ref"))?;
    row.identity = parse_decision_optional_identity(fields.get("identity"))?;
    row.evidence_method = match fields.get("evidence_method") {
        Some(crate::DataTree::DataTree::Null) => None,
        Some(value) => {
            let method = crate::JSON::json_str(value)
                .ok_or_else(|| "decision row has invalid `evidence_method`".to_string())?;
            Some(
                DerivationMethod::from_str(method)
                    .ok_or_else(|| "decision row has unsupported `evidence_method`".to_string())?,
            )
        }
        None => return Err("decision row has no `evidence_method`".to_string()),
    };
    row.derivation_disposition = match fields.get("derivation_disposition") {
        Some(crate::DataTree::DataTree::Null) => None,
        Some(value) => {
            let disposition = crate::JSON::json_str(value).ok_or_else(|| {
                "decision row has invalid `derivation_disposition`".to_string()
            })?;
            Some(DerivationDisposition::from_str(disposition).ok_or_else(|| {
                "decision row has unsupported `derivation_disposition`".to_string()
            })?)
        }
        None => return Err("decision row has no `derivation_disposition`".to_string()),
    };
    Ok(row)
}

/// A conservative reason why a semantic optimization cannot be selected.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum MirOptimizationRejection {
    MissingProof,
    MayTrap,
    HasEffects,
    HasEarlyExit,
    MayAlias,
    CrossIterationDependency,
    DynamicTripCount,
    UnknownCopyCost,
    ScalarBoundary,
    ObservableLayout,
    OwnershipObligation,
    UnsupportedOperation,
}

impl MirOptimizationRejection {
    /// Stable machine-readable spelling used by inspect projections.
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::MissingProof => "missing-proof",
            Self::MayTrap => "may-trap",
            Self::HasEffects => "has-effects",
            Self::HasEarlyExit => "has-early-exit",
            Self::MayAlias => "may-alias",
            Self::CrossIterationDependency => "cross-iteration-dependency",
            Self::DynamicTripCount => "dynamic-trip-count",
            Self::UnknownCopyCost => "unknown-copy-cost",
            Self::ScalarBoundary => "scalar-boundary",
            Self::ObservableLayout => "observable-layout",
            Self::OwnershipObligation => "ownership-obligation",
            Self::UnsupportedOperation => "unsupported-operation",
        }
    }
}

/// The canonical semantic shape selected by the vector proof.  This is a MIR
/// fact, not a backend-specific instruction name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MirVectorRule {
    Elementwise,
    FieldAccess,
    ConditionalAccumulate,
    EarlyExitSearch,
    Reduction,
}

impl MirVectorRule {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Elementwise => "elementwise",
            Self::FieldAccess => "field-access",
            Self::ConditionalAccumulate => "conditional-accumulate",
            Self::EarlyExitSearch => "early-exit-search",
            Self::Reduction => "reduction",
        }
    }
}

/// Physical access selected by the shared proof.  Backends use this to choose
/// their already-existing carrier/load adapter; they must not infer layout
/// from rendered Rust or JavaScript types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MirVectorLayout {
    Flat,
    AosStrided,
    ColumnarDirect,
}

impl MirVectorLayout {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Flat => "flat",
            Self::AosStrided => "aos-strided",
            Self::ColumnarDirect => "columnar-direct",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MirVectorAccessRoot {
    Place(MirPlaceId),
    Value(MirValueId),
}

/// One member of a proven per-field access set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MirVectorAccess {
    pub root: MirVectorAccessRoot,
    pub field: Option<MirFieldId>,
    pub layout: MirVectorLayout,
    pub column_index: Option<usize>,
}

impl MirVectorAccess {
    pub const fn field_name_required(self) -> bool {
        self.field.is_some()
    }
}

/// Every derived fact is either a proof-backed eligibility or an explicit
/// conservative rejection.  Absence is not represented as an unchecked
/// success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirOptimizationDecision {
    Eligible,
    Rejected(MirOptimizationRejection),
}

impl MirOptimizationDecision {
    pub const fn is_eligible(&self) -> bool {
        matches!(self, Self::Eligible)
    }
}

/// A canonical loop shape.  The source span is retained even when the
/// control-flow spelling is reduced to the shared counted/iterator shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MirLoopForm {
    Unconditional,
    Counted,
    Iterator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MirCopyCost {
    None,
    PerIteration(u32),
    Unknown,
}

/// One canonical loop row.  The source span is retained even when the
/// control-flow spelling is reduced to the shared counted/iterator shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirLoopFact {
    pub header: MirBlockId,
    pub body: Option<MirBlockId>,
    pub exit: Option<MirBlockId>,
    pub form: MirLoopForm,
    pub trip_count: Option<u64>,
    pub copy_cost: MirCopyCost,
    pub canonical: bool,
    pub span: Span,
    pub decision: MirOptimizationDecision,
}

/// A bounds fact is attached to the exact operation and keeps the failure
/// contract visible.  `elided` is true only for an explicit proof kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirBoundsFact {
    pub operation: MirOpId,
    pub place: Option<MirPlaceId>,
    pub index: Option<MirValueId>,
    pub kind: MirIndexKind,
    pub proven: bool,
    pub elided: bool,
    pub failure_preserved: bool,
    pub span: Span,
}

/// A fixed-order reduction row consumed by every execution adapter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirFixedReductionFact {
    /// The scalar accumulator place retained by the source loop.
    pub accumulator: MirPlaceId,
    /// The source value added by the original loop body.
    pub addend: MirValueId,
    /// A fresh preheader read of `accumulator`, used as the exact seed.
    pub seed: MirValueId,
    /// The checked predicate for a masked accumulation, when present.
    pub condition: Option<MirValueId>,
    /// Original body operations retained for packed expression marshalling.
    pub source_operations: Vec<MirOpId>,
    /// Generated continuation after the seed-preserving exit/finalizer gate.
    pub exit: MirBlockId,
    /// Canonical D-FRED1 reduction order; adapters must not choose another tree.
    pub order: crate::MIROptimization::Acceleration::FixedReductionOrder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirVectorFact {
    pub loop_header: MirBlockId,
    pub cursor: Option<MirValueId>,
    pub body_blocks: Vec<MirBlockId>,
    pub advance_block: Option<MirBlockId>,
    pub rule: MirVectorRule,
    pub accesses: Vec<MirVectorAccess>,
    pub layout: MirVectorLayout,
    pub element_type: Option<MirType>,
    pub packed: bool,
    pub lane_width: Option<u16>,
    pub no_aliasing: bool,
    pub no_early_exit: bool,
    pub effect_free_body: bool,
    pub no_cross_iteration_dependencies: bool,
    pub fixed_reduction: Option<MirFixedReductionFact>,
    pub span: Span,
    pub decision: MirOptimizationDecision,
}


/// Fusion is represented as a proof row even when the conservative answer is
/// rejection.  No pass fuses based on a guessed alias or effect summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirFusionFact {
    pub first_loop: MirBlockId,
    pub second_loop: MirBlockId,
    pub packed: bool,
    pub same_iteration_domain: bool,
    pub no_aliasing: bool,
    pub effect_free: bool,
    pub no_cross_iteration_dependencies: bool,
    pub span: Span,
    pub decision: MirOptimizationDecision,
}

/// One deterministic source-side acceleration row.  Runtime measurements and
/// release activation stay outside canonical MIR; adapters evaluate this row
/// with their last-run gate input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirAccelerationFact {
    pub loop_header: MirBlockId,
    pub transform: crate::MIROptimization::Acceleration::AccelerationTransform,
    pub workload: crate::MIROptimization::Acceleration::AccelerationWorkloadFacts,
    pub proof: crate::MIROptimization::Acceleration::AccelerationProof,
    pub span: Span,
}

impl MirAccelerationFact {
    pub const fn source(
        loop_header: MirBlockId,
        transform: crate::MIROptimization::Acceleration::AccelerationTransform,
        workload: crate::MIROptimization::Acceleration::AccelerationWorkloadFacts,
        proof: crate::MIROptimization::Acceleration::AccelerationProof,
        span: Span,
    ) -> Self {
        Self {
            loop_header,
            transform,
            workload,
            proof,
            span,
        }
    }

    pub fn evaluate(
        &self,
        gate: crate::MIROptimization::Acceleration::AccelerationGate,
        mut input: crate::MIROptimization::Acceleration::AccelerationGateInput,
    ) -> crate::MIROptimization::Acceleration::AccelerationDecision {
        input.workload = self.workload;
        input.proof = self.proof;
        gate.evaluate(self.transform, input)
    }
}

#[derive(Debug, Clone, Default)]
pub struct MirOptimizationFacts {
    pub pure: bool,
    pub replayable: bool,
    pub diverges: bool,
    pub inline: bool,
    pub inline_always: bool,
    pub gc_return: bool,
    pub gc_scope: bool,
    pub kernel: bool,
    pub auto_vectorizable: bool,
    pub no_aliasing: bool,
    pub no_early_exit: bool,
    pub no_cross_iteration_dependencies: bool,
    /// Internal marker for the MIR state that owns the derived rows below.
    /// This is invalidation metadata, not a semantic fact.
    pub(crate) derived_from_digest: Option<[u8; 32]>,
    /// Producer-authored rows that do not have a narrower legacy fact type.
    pub decision_rows: Vec<MirDecisionRow>,
    /// IDs of passes that have populated this function's semantic rows.
    pub pass_ids: Vec<MirOptimizationPassId>,
    pub loop_facts: Vec<MirLoopFact>,
    pub bounds_facts: Vec<MirBoundsFact>,
    pub vector_facts: Vec<MirVectorFact>,
    pub fusion_facts: Vec<MirFusionFact>,
    pub acceleration_facts: Vec<MirAccelerationFact>,
}

impl MirOptimizationFacts {
    pub(crate) fn clear_derived(&mut self) {
        self.auto_vectorizable = false;
        self.no_aliasing = false;
        self.no_early_exit = false;
        self.no_cross_iteration_dependencies = false;
        self.derived_from_digest = None;
        self.pass_ids.clear();
        self.loop_facts.clear();
        self.bounds_facts.clear();
        self.vector_facts.clear();
        self.fusion_facts.clear();
        self.acceleration_facts.clear();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirScopeKind {
    Unsafe,
    Impure,
    Reactive,
    Shield,
    Region,
    Policy,
    TaskGroup,
    Layout,
    Authority,
    Context,
    Live,
    AssumeDeterministic,
    Transaction,
    ScopeMember,
    /// A lexical region whose Rust AOT body is excluded from `jet_release`.
    DebugOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirTestScopeMember {
    ExpectFail { expected_code: Option<String> },
    Timeout { duration: MirValueId },
    Skip { whole_test: bool },
    Measure,
}

#[derive(Debug, Clone)]
pub struct MirScope {
    pub id: MirScopeId,
    pub kind: MirScopeKind,
    pub span: Span,
    pub name: Option<String>,
    pub facts: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct MirParam {
    pub index: usize,
    pub name: String,
    pub span: Span,
    pub ty: MirType,
    pub access: MirAccess,
    pub ownership: MirOwnership,
    pub public_label: String,
    pub variadic: bool,
    pub default_present: bool,
}
#[derive(Debug, Clone)]
pub struct MirCaptureParam {
    pub slot: usize,
    pub name: String,
    pub span: Span,
    pub ty: MirType,
    pub access: MirAccess,
    pub ownership: MirOwnership,
}
#[derive(Debug, Clone)]
pub enum MirCaptureOperand {
    Value(MirValueId),
    Place(MirPlaceId),
}



#[derive(Debug, Clone)]
pub struct MirLocal {
    pub id: MirLocalId,
    pub name: String,
    pub span: Span,
    pub ty: MirType,
    pub place: MirPlaceId,
    pub mutable: bool,
    pub ownership: MirOwnership,
    pub comptime: bool,
    pub uninit: bool,
    pub arena_view: bool,
    pub string_view: bool,
    pub gc_root: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirPlaceBase {
    Local(MirLocalId),
    Parameter(MirValueId),
    Capture(MirValueId),
    Temporary(MirValueId),
    Static(String),
}

#[derive(Debug, Clone)]
pub enum MirProjection {
    Field { field: MirFieldId, span: Span },
    Index {
        kind: MirIndexKind,
        index: MirValueId,
        /// Exact checked getter/mutable-accessor row.
        call: MirPreludeCallId,
        /// Exact checked setter row for write places.
        write_call: Option<MirPreludeCallId>,
        location: MirPanicLoc,
        /// Rich checked context required by setters whose stop contract names
        /// the containing function and source line (currently Pool writes).
        context: Option<MirPanicContext>,
        span: Span,
    },
    Deref { span: Span },
}

#[derive(Debug, Clone)]
pub struct MirPlace {
    pub id: MirPlaceId,
    pub span: Span,
    pub ty: MirType,
    pub base: MirPlaceBase,
    pub projections: Vec<MirProjection>,
    pub access: MirAccess,
    /// D-DEVTOOLS-LOCALRAIL1: checked identity for an approved `#Persist`
    /// root. Projection lowering carries this fact to writes; emitters never
    /// infer observability from a local's rendered name.
    pub persist_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirIndexKind {
    List,
    FixedListProof,
    Map,
    Lane,
    Pool,
}


#[derive(Debug, Clone)]
pub struct MirFnCoercion {
    pub ty: MirType,
    pub already_boxed: bool,
}
#[derive(Debug, Clone)]
pub struct MirUnionCoercion {
    pub union: MirTypeId,
    pub variant: String,
}


#[derive(Debug, Clone)]
pub struct MirCallArg {
    /// Value used for ordinary read/move arguments and as a type-bearing
    /// placeholder for place arguments.
    pub value: MirValueId,
    /// The checked place for a write argument. Write arguments must carry this
    /// field so every backend can mutate the same canonical slot.
    pub place: Option<MirPlaceId>,
    pub access: MirAccess,
    pub span: Span,
    pub label: Option<String>,
    pub source_index: Option<usize>,
    pub binder_slot: Option<usize>,
    pub spread: bool,
    pub implicit_clone: bool,
    pub shared_auto_clone: bool,
    pub owned_last_use: bool,
    pub authority_boundary: bool,
    pub fn_coercion: Option<MirFnCoercion>,
    pub widen_fixed_to_list: bool,
    pub widen_to_union: Option<MirUnionCoercion>,
    pub box_as_trait: Option<MirTypeId>,
}
fn call_arg_type_uses(args: &[MirCallArg]) -> impl Iterator<Item = MirTypeId> + '_ {
    args.iter().flat_map(|arg| {
        [
            arg.fn_coercion
                .as_ref()
                .and_then(|coercion| coercion.ty.identity),
            arg.widen_to_union.as_ref().map(|coercion| coercion.union),
            arg.box_as_trait,
        ]
        .into_iter()
        .flatten()
    })
}

fn failure_type_uses(failure: &MirFailureCarrier) -> [Option<MirTypeId>; 2] {
    match failure {
        MirFailureCarrier::Infallible => [None, None],
        MirFailureCarrier::Result { success, error } => [success.identity, error.identity],
        MirFailureCarrier::Optional { value } | MirFailureCarrier::Diverges { value } => {
            [value.identity, None]
        }
    }
}

fn fallibility_type_uses(fallibility: &MirCallFallibility) -> [Option<MirTypeId>; 2] {
    match fallibility {
        MirCallFallibility::Infallible => [None, None],
        MirCallFallibility::Failure(failure) => failure_type_uses(failure),
    }
}


#[derive(Debug, Clone)]
pub enum MirCallee {
    User(MirFunctionId),
    /// A checked associated function. `owner` preserves the instantiated
    /// receiver type needed by representation adapters; `function` is the
    /// canonical semantic target used by execution engines.
    Associated {
        function: MirFunctionId,
        owner: MirType,
    },
    /// A checked dynamic trait-object method. `method` and `trait_ref` are
    /// semantic identities projected from the trait declaration; `receiver`
    /// preserves the instantiated trait-object type for every backend.
    TraitMethod {
        method: MirTraitMethodId,
        trait_ref: MirTraitRef,
        receiver: MirType,
    },
    /// A checked instance method. The receiver is the first call argument;
    /// `owner` selects the canonical impl without adapter-side name lookup.
    Method {
        function: MirFunctionId,
        owner: MirType,
    },
    Core(MirCoreCallId),
    Prelude(MirPreludeCallId),
    Foreign(MirForeignId),
    Indirect(MirValueId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirUnaryOp {
    Neg,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirBinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Rem,
    Pow,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    Compare,
    And,
    Or,
}

impl MirBinaryOp {
    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            Self::Eq | Self::Ne | Self::Lt | Self::Gt | Self::Le | Self::Ge
        )
    }

    /// The user-typed spelling for diagnostics and target adapters.
    pub fn spell(self) -> &'static str {
        match self {
            Self::Add => "+",
            Self::Sub => "-",
            Self::Mul => "*",
            Self::Div => "/",
            Self::FloorDiv => "/%",
            Self::Mod => "%",
            Self::Rem => "%%",
            Self::Pow => "^",
            Self::BitAnd => "&",
            Self::BitOr => "|",
            Self::BitXor => "~|",
            Self::Shl => "<<",
            Self::Shr => ">>",
            Self::Eq => "==",
            Self::Ne => "!=",
            Self::Lt => "<",
            Self::Gt => ">",
            Self::Le => "<=",
            Self::Ge => ">=",
            Self::Compare => "<=>",
            Self::And => "&&",
            Self::Or => "||",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MirConstKey {
    Int(i64),
    String(String),
    Bool(bool),
    Char(char),
    Tuple(Vec<(String, MirConstKey)>),
    Struct {
        type_name: String,
        fields: Vec<(String, MirConstKey)>,
    },
    Enum {
        type_name: String,
        variant: String,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum MirConstant {
    Int {
        value: i64,
        width: Option<(bool, u8)>,
        spelling: Option<String>,
    },
    Float {
        value: f64,
        f32: bool,
        spelling: Option<String>,
    },
    Bool(bool),
    Char(char),
    String(String),
    Bytes(Vec<u8>),
    Unit,
    BigInt(String),
    List(Vec<MirConstant>),
    Map(BTreeMap<MirConstKey, MirConstant>),
    Struct {
        type_name: String,
        fields: Vec<(String, MirConstant)>,
    },
    Enum {
        type_name: String,
        variant: String,
        args: Vec<(Option<String>, MirConstant)>,
    },
    Present(Box<MirConstant>),
    Failed(MirConstReport),
}

#[derive(Debug, Clone, PartialEq)]
pub enum MirConstReport {
    Clean(MirType),
    Told(Box<MirConstant>),
}

/// Public, target-neutral runtime data carried between MIR execution adapters
/// and shared Prelude kernels. Host resources remain private adapter handles.
#[derive(Debug, Clone, PartialEq)]
pub enum MirRuntimeValue {
    /// Internal move-state sentinel.  It is never a user-visible runtime value,
    /// never serialized, and is consumed by projected/direct place moves.
    Moved,
    Int(i64),
    BigInt(String),
    Float { value: f64, f32: bool },
    Bool(bool),
    Char(char),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<MirRuntimeValue>),
    Map(Vec<(MirConstKey, MirRuntimeValue)>),
    Struct {
        type_name: String,
        fields: Vec<(String, MirRuntimeValue)>,
    },
    Enum {
        type_name: String,
        variant: String,
        args: Vec<(Option<String>, MirRuntimeValue)>,
    },
    Present(Box<MirRuntimeValue>),
    FailedTold(Box<MirRuntimeValue>),
    Absent { element: MirType },
    Unit,
    Closure(MirRuntimeClosure),
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirRuntimeClosure {
    pub function: MirFunctionId,
    pub captures: Vec<MirRuntimeValue>,
}

#[derive(Debug, Clone)]
pub enum MirStringPart {
    Literal(String),
    Value(MirValueId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirTextHoleKind {
    Text,
    Int,
    Float,
    Bool,
    InlineRange { lo: i64, hi: i64 },
}

#[derive(Debug, Clone)]
pub enum MirTextPatternPart {
    Literal(String),
    Hole {
        kind: MirTextHoleKind,
        ty: MirTypeId,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub enum MirBinaryPatternPart {
    Literal(Vec<u8>),
    Bits {
        width: u8,
        ty: MirTypeId,
        little: bool,
        span: Span,
    },
    Rest {
        ty: MirTypeId,
        span: Span,
    },
}

#[derive(Debug, Clone)]
pub struct MirPattern {
    pub shape: MirPatternShape,
    pub owner: Option<MirTypeId>,
    pub position: MirPatternPosition,
    pub mutable: bool,
    pub boxed: bool,
}

#[derive(Debug, Clone)]
pub enum MirPatternPosition {
    Binding,
    OptionBinding,
    Arm,
    VariantPath,
    DataEntries { temp: MirLocalId },
}

#[derive(Debug, Clone)]
pub enum MirPatternShape {
    Variant {
        variant: String,
        bindings: Vec<MirPatternBinding>,
        leading_dot: bool,
        span: Span,
    },
    Present {
        binding: String,
        binding_span: Span,
        span: Span,
    },
    Absent(Span),
    Ok {
        binding: String,
        binding_span: Span,
        span: Span,
    },
    Err {
        binding: String,
        binding_span: Span,
        span: Span,
    },
    Range {
        lo: i64,
        hi: i64,
        span: Span,
    },
    Or {
        alternatives: Vec<MirPatternShape>,
        span: Span,
    },
    Struct {
        fields: Vec<MirPatternField>,
        rest: Option<Span>,
        span: Span,
    },
    Text(Vec<MirTextPatternPart>, Span),
    Binary(Vec<MirBinaryPatternPart>, Span),
}

#[derive(Debug, Clone)]
pub enum MirPatternBinding {
    Wildcard,
    Bind {
        name: String,
        span: Span,
    },
    Range {
        lo: i64,
        hi: i64,
    },
}

#[derive(Debug, Clone)]
pub enum MirPatternField {
    Bind {
        field: MirFieldId,
        local: String,
        span: Span,
    },
    Value {
        field: MirFieldId,
        value: MirValueId,
        span: Span,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirStructExtra {
    HttpRequestParams,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirLayoutCompareOp {
    Equal,
    LessEqual,
    GreaterEqual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirRequireKind {
    Require,
    RequireEq,
    Panic,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirHttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
    Options,
}

impl MirHttpMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        }
    }
}
/// Checked OpenAPI schema facts carried from route lowering to every execution
/// adapter.  This is deliberately a small semantic model rather than a target
/// JSON value: the OpenAPI Prelude is the sole renderer, while AOT/JIT/eval/Web
/// only marshal its wire projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirHttpSchema {
    Any,
    Null,
    Boolean,
    Integer,
    Number,
    String,
    Array(Box<MirHttpSchema>),
    Object {
        properties: Vec<MirHttpPropertyFact>,
        additional_properties: bool,
    },
    OneOf(Vec<MirHttpSchema>),
    Nullable(Box<MirHttpSchema>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirHttpPropertyFact {
    pub name: String,
    pub schema: MirHttpSchema,
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum MirHttpParameterLocation {
    Path,
    Query,
    Header,
    Cookie,
}

impl MirHttpParameterLocation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Query => "query",
            Self::Header => "header",
            Self::Cookie => "cookie",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirHttpParameterFact {
    pub name: String,
    pub location: MirHttpParameterLocation,
    pub required: bool,
    pub catch_all: bool,
    pub schema: MirHttpSchema,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirHttpRequestBodyFact {
    pub required: bool,
    pub content_type: String,
    pub schema: MirHttpSchema,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirHttpResponseFact {
    pub status: i64,
    pub description: String,
    pub schema: Option<MirHttpSchema>,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirHttpSecurityFact {
    pub scheme: String,
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirHttpRouteFacts {
    pub method: MirHttpMethod,
    /// The checked source template (`:id`/`*rest`), retained so the shared
    /// HTTP route parser remains the authority for canonical path parameters.
    pub pattern: String,
    /// Canonical OpenAPI path (`{id}`/`{rest}`), derived from `pattern`.
    pub path: String,
    pub operation_id: String,
    pub summary: Option<String>,
    pub parameters: Vec<MirHttpParameterFact>,
    pub request_body: Option<MirHttpRequestBodyFact>,
    pub responses: Vec<MirHttpResponseFact>,
    pub security: Vec<MirHttpSecurityFact>,
    pub provenance: String,
}

fn mir_http_json_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len().saturating_add(2));
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

impl MirHttpSchema {
    fn to_wire(&self) -> String {
        match self {
            Self::Any => "{}".to_string(),
            Self::Null => "{\"type\":\"null\"}".to_string(),
            Self::Boolean => "{\"type\":\"boolean\"}".to_string(),
            Self::Integer => "{\"type\":\"integer\",\"format\":\"int64\"}".to_string(),
            Self::Number => "{\"type\":\"number\",\"format\":\"double\"}".to_string(),
            Self::String => "{\"type\":\"string\"}".to_string(),
            Self::Array(item) => format!("{{\"type\":\"array\",\"items\":{}}}", item.to_wire()),
            Self::Object {
                properties,
                additional_properties,
            } => {
                let mut required = Vec::new();
                let mut rendered = Vec::with_capacity(properties.len());
                for property in properties {
                    if property.required {
                        required.push(mir_http_json_quote(&property.name));
                    }
                    rendered.push(format!(
                        "{}:{}",
                        mir_http_json_quote(&property.name),
                        property.schema.to_wire()
                    ));
                }
                let mut out = format!(
                    "{{\"type\":\"object\",\"properties\":{{{}}},\"additionalProperties\":{}",
                    rendered.join(","),
                    additional_properties
                );
                if !required.is_empty() {
                    out.push_str(",\"required\":[");
                    out.push_str(&required.join(","));
                    out.push(']');
                }
                out.push('}');
                out
            }
            Self::OneOf(items) => format!(
                "{{\"oneOf\":[{}]}}",
                items.iter().map(Self::to_wire).collect::<Vec<_>>().join(",")
            ),
            Self::Nullable(item) => {
                format!("{{\"anyOf\":[{},{{\"type\":\"null\"}}]}}", item.to_wire())
            }
        }
    }
}

impl MirHttpRouteFacts {
    /// Stable operation identity is a function of checked method + canonical
    /// path only.  It never depends on source order or host state.
    pub fn operation_id(method: MirHttpMethod, path: &str) -> String {
        let mut hash = 14695981039346656037u64;
        for byte in b"jet-openapi-operation-v1:" {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(1099511628211);
        }
        for byte in method
            .as_str()
            .bytes()
            .chain(std::iter::once(b'\0'))
            .chain(path.bytes())
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(1099511628211);
        }
        format!("jet_{}_{hash:016x}", method.as_str().to_ascii_lowercase())
    }

    /// Marshal checked facts for the shared Prelude projection.  The string
    /// is a carrier only; no adapter may add defaults or reinterpret fields.
    pub fn to_wire(&self) -> String {
        let parameters = self
            .parameters
            .iter()
            .map(|parameter| {
                format!(
                    "{{\"name\":{},\"in\":{},\"required\":{},\"catch_all\":{},\"schema\":{}}}",
                    mir_http_json_quote(&parameter.name),
                    mir_http_json_quote(parameter.location.as_str()),
                    parameter.required,
                    parameter.catch_all,
                    parameter.schema.to_wire()
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let request_body = self.request_body.as_ref().map_or_else(
            || "null".to_string(),
            |body| {
                format!(
                    "{{\"required\":{},\"content_type\":{},\"schema\":{}}}",
                    body.required,
                    mir_http_json_quote(&body.content_type),
                    body.schema.to_wire()
                )
            },
        );
        let responses = self
            .responses
            .iter()
            .map(|response| {
                let schema = response
                    .schema
                    .as_ref()
                    .map_or_else(|| "null".to_string(), MirHttpSchema::to_wire);
                let content_type = response
                    .content_type
                    .as_deref()
                    .map_or_else(|| "null".to_string(), mir_http_json_quote);
                format!(
                    "{{\"status\":{},\"description\":{},\"schema\":{},\"content_type\":{}}}",
                    response.status,
                    mir_http_json_quote(&response.description),
                    schema,
                    content_type
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let security = self
            .security
            .iter()
            .map(|requirement| {
                format!(
                    "{{\"scheme\":{},\"scopes\":[{}]}}",
                    mir_http_json_quote(&requirement.scheme),
                    requirement
                        .scopes
                        .iter()
                        .map(|scope| mir_http_json_quote(scope))
                        .collect::<Vec<_>>()
                        .join(",")
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let summary = self
            .summary
            .as_deref()
            .map_or_else(|| "null".to_string(), mir_http_json_quote);
        format!(
            "{{\"method\":{},\"pattern\":{},\"path\":{},\"operation_id\":{},\"summary\":{},\"parameters\":[{}],\"request_body\":{},\"responses\":[{}],\"security\":[{}],\"provenance\":{}}}",
            mir_http_json_quote(self.method.as_str()),
            mir_http_json_quote(&self.pattern),
            mir_http_json_quote(&self.path),
            mir_http_json_quote(&self.operation_id),
            summary,
            parameters,
            request_body,
            responses,
            security,
            mir_http_json_quote(&self.provenance)
        )
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirPanicLoc {
    pub file: MirSourceFileId,
    pub line: u32,
    pub column: u32,
}
/// Checked rich diagnostic context for require/panic. Adapters render or
/// marshal these facts; they never rediscover source text or visible locals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirPanicContext {
    pub function: String,
    pub source_line: String,
    pub caret: u32,
    pub locals: Vec<(String, MirLocalId)>,
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirAllocatorKind {
    General,
    Fixed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirTaskGroupKind {
    All,
    Any,
    Race,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirSelectKind {
    Start,
    Receive,
    After,
    Wait,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirCoreClosureKind {
    Spawn,
    Realtime,
    Serve,
    OnInterrupt,
    Guard,
    OnCommit,
    OnRollback,
    ReactiveDerived,
    ReactiveEffect,
    UiMount,
    /// D-UI-CLOSURE1=A: one canonical button callback route with metadata.
    UiAction,
    /// D-UI-DROP1=A: text-input drop callback over the shared UI node host.
    UiTextInputOnDrop,
    /// D-UI-PREVIEW1=A: named preview callback over the canonical UiNode.
    /// Compiler provenance is carried here so all execution tiers can attach
    /// source/build identity without widening the public Core call.
    UiPreview {
        playground: bool,
        source_file: String,
        source_span: Span,
        source_start_line: u32,
        source_start_column: u32,
        source_end_line: u32,
        source_end_column: u32,
        build_id: String,
        revision: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirGcEditKind {
    Clear,
    Pop,
    RemoveIndex,
    InsertIndex,
    Prepend,
    Additive,
    Plain,
    EdgeSlot,
}

/// Checked target-profile hardware operation metadata.
///
/// These variants contain only sema-owned identity and capability facts.
/// Runtime values remain in the generic [`MirSemanticOp::HardwareCall`]
/// receiver and argument fields so every adapter marshals one common shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirHardwareOp {
    RegisterRead {
        profile_id: String,
        block: String,
        register: String,
        width: crate::TargetMachine::RegisterWidth,
    },
    RegisterWrite {
        profile_id: String,
        block: String,
        register: String,
        width: crate::TargetMachine::RegisterWidth,
    },
    DmaStart {
        profile_id: String,
        channel: String,
        buffer_ty: MirType,
    },
    DmaWait {
        profile_id: String,
        channel: String,
        buffer_ty: MirType,
    },
}

/// Checked package-level hardware setup installed before execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirHardwareSetup {
    DmaConfigure {
        profile_id: String,
        channel: String,
        transfer_width: crate::TargetMachine::RegisterWidth,
        ownership: crate::TargetMachine::TargetDmaOwnership,
    },
    InterruptBind {
        profile_id: String,
        interrupt: String,
        vector: u16,
        handler_symbol: String,
        forbidden_effects: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub enum MirSemanticOp {
    DataEntriesToMap {
        call: MirPreludeCallId,
        local: MirLocalId,
    },
    MathBuiltin {
        type_id: MirTypeId,
        call: MirPreludeCallId,
        args: Vec<MirValueId>,
    },
    PreciseBuiltin {
        type_id: MirTypeId,
        call: MirPreludeCallId,
        args: Vec<MirValueId>,
    },
    Print {
        call: MirPreludeCallId,
        value: MirValueId,
    },
    AmbientInput {
        call: MirPreludeCallId,
        prompt: Option<MirValueId>,
    },
    RequireStop {
        call: MirPreludeCallId,
        kind: MirRequireKind,
        /// Checked truth value. Present for require/require_eq, absent for panic.
        condition: Option<MirValueId>,
        location: MirPanicLoc,
        context: MirPanicContext,
        values: Vec<MirValueId>,
        always_stops: bool,
    },
    LayoutCompare {
        call: MirPreludeCallId,
        op: MirLayoutCompareOp,
        left: MirValueId,
        right: MirValueId,
    },
    LayoutLiteral { inner: MirValueId },
    StructLiteral {
        type_id: MirTypeId,
        fields: Vec<(MirFieldId, MirValueId)>,
        extra: Option<MirStructExtra>,
        trait_coercion: Option<MirTypeId>,
        boxed_fields: Vec<MirFieldId>,
    },
    /// Projection of a task-local Cell read/edit guard. Paths are checked
    /// stored-field identities; adapters marshal the guard representation but
    /// never rediscover fields from text.
    CellGuardProject {
        map_call: MirPreludeCallId,
        split_call: Option<MirPreludeCallId>,
        guard: MirValueId,
        paths: Vec<Vec<MirFieldId>>,
        editable: bool,
        edit_paths_disjoint: bool,
    },
    SharedGuardMap {
        call: MirPreludeCallId,
        guard: MirValueId,
        path: Vec<MirFieldId>,
        editable: bool,
    },
    SharedGuardSplit {
        call: MirPreludeCallId,
        /// Exact map row used for a common prefix and remaining suffixes.
        map_call: MirPreludeCallId,
        guard: MirValueId,
        first: Vec<MirFieldId>,
        second: Vec<MirFieldId>,
        editable: bool,
    },
    SharedGuardWait {
        call: MirPreludeCallId,
        guard: MirValueId,
        condition: MirValueId,
        predicate: MirValueId,
    },
    ConditionNotify {
        call: MirPreludeCallId,
        condition: MirValueId,
        all: bool,
    },
    AllocNew {
        call: MirPreludeCallId,
        kind: MirAllocatorKind,
    },
    ColumnarRead {
        base: MirValueId,
        index: MirValueId,
        column: MirFieldId,
        /// Checked physical column offset in the canonical column store.
        column_index: usize,
        accessor: MirPreludeCallId,
    },
    StaticPreludeCall {
        call: MirPreludeCallId,
        args: Vec<MirCallArg>,
        owner_type_args: Vec<MirPreludeTypeArg>,
        type_args: Vec<MirType>,
    },

    DecodeUnder {
        call: MirPreludeCallId,
        segment: MirValueId,
        inner: MirValueId,
    },
    HardwareCall {
        call: MirPreludeCallId,
        op: MirHardwareOp,
        receiver: Option<MirValueId>,
        args: Vec<MirCallArg>,
    },
    BuiltinMethod {
        call: MirPreludeCallId,
        receiver: MirValueId,
        /// Checked place for mutating receiver methods; `None` means the
        /// operation is value-based and does not write through a receiver.
        receiver_place: Option<MirPlaceId>,
        args: Vec<MirValueId>,
    },
    OptionLift2 {
        call: MirPreludeCallId,
        function: MirValueId,
        left: MirValueId,
        right: MirValueId,
    },
    ClosureMethod {
        receiver: MirValueId,
        args: Vec<MirCallArg>,
        call: MirPreludeCallId,
    },
    HostBorrowCallback {
        callable: MirValueId,
        params: Vec<MirType>,
    },
    TextPatternMatch {
        call: MirPreludeCallId,
        subject: MirValueId,
        parts: Vec<MirTextPatternPart>,
    },
    BinaryPatternMatch {
        call: MirPreludeCallId,
        subject: MirValueId,
        parts: Vec<MirBinaryPatternPart>,
    },
    NumericMethod {
        call: MirPreludeCallId,
        receiver: MirValueId,
    },
    NumericBinaryMethod {
        call: MirPreludeCallId,
        receiver: MirValueId,
        argument: MirValueId,
    },
    OverflowOption {
        call: MirPreludeCallId,
        left: MirValueId,
        right: MirValueId,
        location: Option<MirPanicLoc>,
    },
    HandleMethod {
        call: MirPreludeCallId,
        receiver: MirValueId,
        args: Vec<MirValueId>,
        frame_schedule: Option<crate::ResourceSchedule::JetFrameSchedule>,
        frame_schedule_derivation: Option<crate::Facts::DerivationRef>,
    },
    /// One typed invocation of a statically registered Component export.
    /// `signature` is the canonical descriptor selected by sema/TIR; runtime
    /// adapters marshal `handle`, `export_name`, and each argument uniformly.
    PluginInvoke {
        call: MirPreludeCallId,
        handle: MirValueId,
        export_name: String,
        signature: ComponentSignatureDescriptor,
        args: Vec<MirValueId>,
    },
    HttpRouterRegister {
        call: MirPreludeCallId,
        receiver: MirValueId,
        path: MirValueId,
        handler: MirValueId,
        method: MirHttpMethod,
        /// Handler parameter names paired with the checked callable signature.
        /// The native adapter uses these names to bind request values.
        handler_param_names: Vec<String>,
        /// Checked route/OpenAPI facts serialized once by TIR. Execution
        /// adapters marshal this seventh argument without re-inference.
        contract_json: String,
        location: MirPanicLoc,
    },
    CoreClosureCall {
        call: MirPreludeCallId,
        kind: MirCoreClosureKind,
        values: Vec<MirValueId>,
        closure: Option<MirValueId>,
        site: MirSiteId,
        label: String,
    },
    TaskGroup {
        call: MirPreludeCallId,
        kind: MirTaskGroupKind,
        tasks: Vec<MirValueId>,
    },
    Select {
        call: MirPreludeCallId,
        kind: MirSelectKind,
        values: Vec<MirValueId>,
    },
    PolicyFunction {
        policy: MirValueId,
        values: Vec<MirValueId>,
    },
    InterruptFunction {
        interrupt: MirValueId,
        values: Vec<MirValueId>,
    },
    CarrierFact {
        call: MirPreludeCallId,
        receiver: MirValueId,
        field: MirFieldId,
        notes: bool,
    },
    GcEdit {
        call: MirPreludeCallId,
        root: MirValueId,
        edges: Vec<MirValueId>,
        edit: MirValueId,
        index: Option<MirValueId>,
        kind: MirGcEditKind,
        site: MirGcEditSiteId,
    },
    TypedTextInterp {
        call: MirPreludeCallId,
        kind: crate::Syntax::TypedHeadKind,
        literals: Vec<String>,
        holes: Vec<MirValueId>,
    },
    CCallback {
        call: MirPreludeCallId,
        callback: MirCallbackId,
        lambda: MirValueId,
    },
    HostCall {
        call: MirPreludeCallId,
        args: Vec<MirCallArg>,
    },
}

impl MirSemanticOp {
    pub fn value_uses(&self) -> Vec<MirValueId> {
        match self {
            Self::DataEntriesToMap { .. } => Vec::new(),
            Self::MathBuiltin { args, .. }
            | Self::PreciseBuiltin { args, .. }
            | Self::TaskGroup { tasks: args, .. }
            | Self::Select { values: args, .. } => args.clone(),
            Self::Print { value, .. } | Self::LayoutLiteral { inner: value } => vec![*value],
            Self::AmbientInput { prompt, .. } => prompt.iter().copied().collect(),
            Self::RequireStop {
                condition, values, ..
            } => condition
                .iter()
                .copied()
                .chain(values.iter().copied())
                .collect(),
            Self::LayoutCompare { left, right, .. } => vec![*left, *right],
            Self::OptionLift2 {
                function,
                left,
                right,
                ..
            } => vec![*function, *left, *right],
            Self::StructLiteral { fields, .. } => {
                fields.iter().map(|(_, value)| *value).collect()
            }
            Self::CellGuardProject { guard, .. }
            | Self::SharedGuardMap { guard, .. }
            | Self::SharedGuardSplit { guard, .. } => vec![*guard],
            Self::SharedGuardWait {
                guard, condition, ..
            } => vec![*guard, *condition],
            Self::ConditionNotify { condition, .. } => vec![*condition],
            Self::AllocNew { .. } => Vec::new(),
            Self::ColumnarRead { base, index, .. } => vec![*base, *index],
            Self::StaticPreludeCall { args, .. } | Self::HostCall { args, .. } => {
                args.iter().map(|arg| arg.value).collect()
            }
            Self::DecodeUnder { segment, inner, .. } => vec![*segment, *inner],
            Self::HardwareCall { receiver, args, .. } => receiver
                .iter()
                .copied()
                .chain(args.iter().map(|arg| arg.value))
                .collect(),
            Self::BuiltinMethod { receiver, args, .. }
            | Self::HandleMethod { receiver, args, .. } => {
                std::iter::once(*receiver).chain(args.iter().copied()).collect()
            }
            Self::PluginInvoke { handle, args, .. } => {
                std::iter::once(*handle).chain(args.iter().copied()).collect()
            }
            Self::HttpRouterRegister {
                receiver,
                path,
                handler,
                ..
            } => vec![*receiver, *path, *handler],
            Self::TextPatternMatch { subject, .. }
            | Self::BinaryPatternMatch { subject, .. } => vec![*subject],
            Self::ClosureMethod { receiver, args, .. } => {
                std::iter::once(*receiver)
                    .chain(args.iter().map(|arg| arg.value))
                    .collect()
            }
            Self::HostBorrowCallback { callable, .. } => vec![*callable],
            Self::PolicyFunction { policy, values }
            | Self::InterruptFunction {
                interrupt: policy,
                values,
            } => std::iter::once(*policy)
                .chain(values.iter().copied())
                .collect(),
            Self::NumericMethod { receiver, .. } => vec![*receiver],
            Self::NumericBinaryMethod {
                receiver, argument, ..
            }
            | Self::OverflowOption {
                left: receiver,
                right: argument,
                ..
            } => vec![*receiver, *argument],
            Self::CoreClosureCall {
                values, closure, ..
            } => values
                .iter()
                .copied()
                .chain(closure.iter().copied())
                .collect(),
            Self::CarrierFact { receiver, .. } => vec![*receiver],
            Self::GcEdit {
                root,
                edges,
                edit,
                index,
                ..
            } => std::iter::once(*root)
                .chain(edges.iter().copied())
                .chain(std::iter::once(*edit))
                .chain(index.iter().copied())
                .collect(),
            Self::TypedTextInterp { holes, .. } => holes.clone(),
            Self::CCallback { lambda, .. } => vec![*lambda],
        }
    }
}

impl MirSemanticOp {
    pub fn prelude_calls(&self) -> Vec<MirPreludeCallId> {
        match self {
            Self::Print { call, .. }
            | Self::AmbientInput { call, .. }
            | Self::RequireStop { call, .. }
            | Self::DataEntriesToMap { call, .. }
            | Self::LayoutCompare { call, .. }
            | Self::SharedGuardMap { call, .. }
            | Self::SharedGuardWait { call, .. }
            | Self::ConditionNotify { call, .. }
            | Self::AllocNew { call, .. }
            | Self::DecodeUnder { call, .. }
            | Self::OptionLift2 { call, .. }
            | Self::MathBuiltin { call, .. }
            | Self::PreciseBuiltin { call, .. }
            | Self::StaticPreludeCall { call, .. }
            | Self::HardwareCall { call, .. }
            | Self::BuiltinMethod { call, .. }
            | Self::ClosureMethod { call, .. }
            | Self::NumericMethod { call, .. }
            | Self::NumericBinaryMethod { call, .. }
            | Self::OverflowOption { call, .. }
            | Self::HandleMethod { call, .. }
            | Self::PluginInvoke { call, .. }
            | Self::HttpRouterRegister { call, .. }
            | Self::CoreClosureCall { call, .. }
            | Self::TaskGroup { call, .. }
            | Self::Select { call, .. }
            | Self::HostCall { call, .. }
            | Self::TextPatternMatch { call, .. }
            | Self::BinaryPatternMatch { call, .. } => vec![*call],
            Self::CarrierFact { call, .. }
            | Self::GcEdit { call, .. }
            | Self::TypedTextInterp { call, .. }
            | Self::CCallback { call, .. } => vec![*call],
            Self::CellGuardProject {
                map_call,
                split_call,
                ..
            } => std::iter::once(*map_call)
                .chain(split_call.iter().copied())
                .collect(),
            Self::SharedGuardSplit { call, map_call, .. } => vec![*call, *map_call],
            _ => Vec::new(),
        }
    }

    pub fn field_uses(&self) -> Vec<MirFieldId> {
        match self {
            Self::StructLiteral {
                fields,
                boxed_fields,
                ..
            } => fields
                .iter()
                .map(|(field, _)| *field)
                .chain(boxed_fields.iter().copied())
                .collect(),
            Self::CellGuardProject { paths, .. } => {
                paths.iter().flatten().copied().collect()
            }
            Self::SharedGuardMap { path, .. } => path.clone(),
            Self::SharedGuardSplit { first, second, .. } => {
                first.iter().copied().chain(second.iter().copied()).collect()
            }
            Self::ColumnarRead { column, .. } => vec![*column],
            Self::CarrierFact { field, .. } => vec![*field],
            Self::GcEdit { .. } => Vec::new(),
            _ => Vec::new(),
        }
    }
    pub fn type_uses(&self) -> Vec<MirTypeId> {
        match self {
            Self::MathBuiltin { type_id, .. }
            | Self::PreciseBuiltin { type_id, .. } => vec![*type_id],
            Self::StructLiteral {
                type_id,
                trait_coercion,
                ..
            } => std::iter::once(*type_id)
                .chain(trait_coercion.iter().copied())
                .collect(),
            Self::StaticPreludeCall {
                args,
                owner_type_args,
                type_args,
                ..
            } => call_arg_type_uses(args)
                .chain(owner_type_args.iter().filter_map(|type_arg| match type_arg {
                    MirPreludeTypeArg::Type(ty) => ty.identity,
                    MirPreludeTypeArg::HostUsize => None,
                }))
                .chain(type_args.iter().filter_map(|ty| ty.identity))
                .collect(),
            Self::HardwareCall { args, .. } => call_arg_type_uses(args).collect(),
            Self::ClosureMethod { args, .. } | Self::HostCall { args, .. } => {
                call_arg_type_uses(args).collect()
            }
            Self::HostBorrowCallback { params, .. } => {
                params.iter().filter_map(|ty| ty.identity).collect()
            }
            Self::TextPatternMatch { parts, .. } => parts
                .iter()
                .filter_map(|part| match part {
                    MirTextPatternPart::Hole { ty, .. } => Some(*ty),
                    MirTextPatternPart::Literal(_) => None,
                })
                .collect(),
            Self::BinaryPatternMatch { parts, .. } => parts
                .iter()
                .filter_map(|part| match part {
                    MirBinaryPatternPart::Bits { ty, .. }
                    | MirBinaryPatternPart::Rest { ty, .. } => Some(*ty),
                    MirBinaryPatternPart::Literal(_) => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }
    pub fn source_file_uses(&self) -> Vec<MirSourceFileId> {
        match self {
            Self::RequireStop { location, .. }
            | Self::HttpRouterRegister { location, .. } => vec![location.file],
            Self::OverflowOption { location, .. } => {
                location.iter().map(|location| location.file).collect()
            }
            _ => Vec::new(),
        }
    }
}


/// Compile-time-selected implementation of a binary operation. A Prelude row
/// names the exact shared kernel; adapters only marshal operands and location.
#[derive(Debug, Clone)]
pub enum MirBinaryDispatch {
    Primitive,
    Prelude {
        call: MirPreludeCallId,
        location: Option<MirPanicLoc>,
    },
}

#[derive(Debug, Clone)]
pub struct MirEnumArg {
    /// Present for a named payload slot; positional payloads remain ordered.
    pub field: Option<MirFieldId>,
    pub value: MirValueId,
    /// Checked recursive-layout indirection for this payload slot.
    pub boxed: bool,
}

/// One named field in the checked row schema carried by a MIR data plan.
#[derive(Debug, Clone)]
pub struct MirDataPlanColumn {
    pub name: String,
    pub ty: MirType,
    pub nullable: bool,
}

/// A resolved callable signature attached to a data-plan operation.
#[derive(Debug, Clone)]
pub struct MirDataPlanCallable {
    pub label: String,
    pub parameter: MirType,
    pub result: MirType,
    pub span: Span,
}

/// One logical node in the topologically ordered MIR data plan.
#[derive(Debug, Clone)]
pub struct MirDataPlanNode {
    pub id: MirDataPlanNodeId,
    pub operation: crate::AST::DataPlanOperationKind,
    pub inputs: Vec<MirDataPlanNodeId>,
    pub source: Option<crate::AST::DataPlanSourceKind>,
    pub row_type: MirType,
    pub columns: Vec<MirDataPlanColumn>,
    pub callable: Option<MirDataPlanCallable>,
    pub stream: crate::AST::DataPlanStreamMode,
    pub span: Span,
}

/// One backend-neutral physical choice for a logical data-plan node.
#[derive(Debug, Clone)]
pub struct MirDataPlanPhysicalNode {
    pub logical: MirDataPlanNodeId,
    pub operator: crate::AST::DataPlanPhysicalOperatorKind,
    pub stream: crate::AST::DataPlanStreamMode,
    pub reason: String,
}

/// Complete checked table-plan facts carried through semantic MIR.
#[derive(Debug, Clone)]
pub struct MirDataPlan {
    pub schema_version: u16,
    pub source: crate::AST::DataPlanSourceKind,
    pub logical: Vec<MirDataPlanNode>,
    pub physical: Vec<MirDataPlanPhysicalNode>,
    pub output: MirDataPlanNodeId,
    pub source_span: Span,
}

impl MirDataPlan {
    pub const SCHEMA_VERSION: u16 = 1;

    pub fn validate(&self) -> bool {
        self.schema_version == Self::SCHEMA_VERSION
            && !self.logical.is_empty()
            && self.output.0 < self.logical.len() as u64
            && self.logical.iter().enumerate().all(|(index, node)| {
                node.id.0 == index as u64
                    && node.inputs.iter().all(|input| input.0 < node.id.0)
                    && node.columns.iter().all(|column| !column.name.is_empty())
            })
            && self.physical.iter().all(|node| {
                node.logical.0 < self.logical.len() as u64 && !node.reason.trim().is_empty()
            })
    }
}

/// Checked source form for an iterator loop.
///
/// This is a lossless projection of the checked `TForInMethod` fact.  Adapters
/// dispatch on this explicit row; they never infer iteration semantics from
/// the collection type or from backend-specific spellings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirLoopSourceKind {
    Plain,
    Chars,
    LinesFile,
    LinesStdin,
    LinesProcessStream,
    ChannelReceiver,
    EncodingReader { reader_type: String },
    Iterable {
        coll_type: String,
        iter_type: String,
        iter_symbol: String,
        next_symbol: String,
    },
}

impl MirLoopSourceKind {
    /// Encode the checked source fact for the five-argument iterator Prelude
    /// ABI. The result is embedded as a literal by each adapter, so this
    /// compiler-side allocation never occurs in the generated program.
    ///
    /// Fixed forms are `plain`, `chars`, `lines:file`, `lines:stdin`,
    /// `lines:process`, and `channel`. Payload forms use byte lengths:
    /// `encoding:<n>:<reader>` and
    /// `iterable:<n>:<collection>:<n>:<iterator>:<n>:<iter-symbol>:<n>:<next-symbol>`.
    pub fn wire(&self) -> String {
        match self {
            Self::Plain => "plain".to_owned(),
            Self::Chars => "chars".to_owned(),
            Self::LinesFile => "lines:file".to_owned(),
            Self::LinesStdin => "lines:stdin".to_owned(),
            Self::LinesProcessStream => "lines:process".to_owned(),
            Self::ChannelReceiver => "channel".to_owned(),
            Self::EncodingReader { reader_type } => {
                format!("encoding:{}:{}", reader_type.len(), reader_type)
            }
            Self::Iterable {
                coll_type,
                iter_type,
                iter_symbol,
                next_symbol,
            } => format!(
                "iterable:{}:{}:{}:{}:{}:{}:{}:{}",
                coll_type.len(),
                coll_type,
                iter_type.len(),
                iter_type,
                iter_symbol.len(),
                iter_symbol,
                next_symbol.len(),
                next_symbol
            ),
        }
    }

    /// Decode the canonical iterator source wire form without delimiter-based
    /// guessing. Lengths are byte counts, and malformed UTF-8 boundaries,
    /// trailing bytes, or empty checked IDs are rejected.
    pub fn from_wire(wire: &str) -> Option<Self> {
        fn take(input: &str) -> Option<(&str, &str)> {
            let separator = input.find(':')?;
            let length = input.get(..separator)?.parse::<usize>().ok()?;
            let payload_start = separator.checked_add(1)?;
            let payload_end = payload_start.checked_add(length)?;
            let payload = input.get(payload_start..payload_end)?;
            let rest = input.get(payload_end..)?;
            Some((payload, rest))
        }

        match wire {
            "plain" => Some(Self::Plain),
            "chars" => Some(Self::Chars),
            "lines:file" => Some(Self::LinesFile),
            "lines:stdin" => Some(Self::LinesStdin),
            "lines:process" => Some(Self::LinesProcessStream),
            "channel" => Some(Self::ChannelReceiver),
            _ => {
                if let Some(payload) = wire.strip_prefix("encoding:") {
                    let (reader_type, rest) = take(payload)?;
                    if reader_type.is_empty() || !rest.is_empty() {
                        return None;
                    }
                    return Some(Self::EncodingReader {
                        reader_type: reader_type.to_owned(),
                    });
                }
                let payload = wire.strip_prefix("iterable:")?;
                let (coll_type, rest) = take(payload)?;
                let rest = rest.strip_prefix(':')?;
                let (iter_type, rest) = take(rest)?;
                let rest = rest.strip_prefix(':')?;
                let (iter_symbol, rest) = take(rest)?;
                let rest = rest.strip_prefix(':')?;
                let (next_symbol, rest) = take(rest)?;
                if coll_type.is_empty()
                    || iter_type.is_empty()
                    || iter_symbol.is_empty()
                    || next_symbol.is_empty()
                    || !rest.is_empty()
                {
                    return None;
                }
                Some(Self::Iterable {
                    coll_type: coll_type.to_owned(),
                    iter_type: iter_type.to_owned(),
                    iter_symbol: iter_symbol.to_owned(),
                    next_symbol: next_symbol.to_owned(),
                })
            }
        }
    }
}

#[derive(Debug, Clone)]
pub enum MirOperation {

    Parameter { index: usize, name: String },
    Capture { slot: usize },
    Global { name: String },
    Phi { incoming: Vec<(MirBlockId, MirValueId)> },
    ReadPlace(MirPlaceId),
    MovePlace { place: MirPlaceId },
    WritePlace { place: MirPlaceId, value: MirValueId },
    InitializeUninit { place: MirPlaceId },
    Copy { value: MirValueId },
    Move { value: MirValueId },
    Constant(MirConstant),
    Unary {
        op: MirUnaryOp,
        value: MirValueId,
    },
    Binary {
        op: MirBinaryOp,
        dispatch: MirBinaryDispatch,
        left: MirValueId,
        right: MirValueId,
    },
    BuildString {
        parts: Vec<MirStringPart>,
    },
    BuildList {
        values: Vec<MirValueId>,
    },
    BuildMap {
        entries: Vec<(MirValueId, MirValueId)>,
    },
    EnumIs {
        subject: MirValueId,
        owner: MirTypeId,
        variant: String,
    },
    EnumPayload {
        subject: MirValueId,
        owner: MirTypeId,
        variant: String,
        index: usize,
    },
    OptionIsSome { subject: MirValueId },
    OptionValue { subject: MirValueId },
    ResultIsOk { subject: MirValueId },
    ResultValue { subject: MirValueId, ok: bool },
    PatternCapture { matched: MirValueId, index: usize },
    PatternMatched { matched: MirValueId },
    ProjectMembers {
        base: MirValueId,
        members: Vec<MirFieldId>,
    },
    Index {
        call: MirPreludeCallId,
        base: MirValueId,
        index: MirValueId,
        kind: MirIndexKind,
        access: MirAccess,
        location: MirPanicLoc,
        /// Checked rich context for index kernels whose exact ABI reports the
        /// function, source line, caret, or locals.
        context: MirPanicContext,
    },
    Slice {
        call: MirPreludeCallId,
        base: MirValueId,
        start: MirValueId,
        end: MirValueId,
        range: Option<MirValueId>,
        location: MirPanicLoc,
    },
    Range {
        start: MirValueId,
        end: MirValueId,
        exclusive: bool,
    },
    Field {
        base: MirValueId,
        field: MirFieldId,
    },

    Struct {
        type_id: MirTypeId,
        fields: Vec<(MirFieldId, MirValueId)>,
    },
    Enum {
        type_id: MirTypeId,
        variant: String,
        args: Vec<MirEnumArg>,
    },
    Tuple {
        type_id: MirTypeId,
        fields: Vec<(MirFieldId, MirValueId)>,
    },
    Present { value: MirValueId },
    Convert {
        value: MirValueId,
        parameters: Vec<MirValueId>,
        target: MirType,
        conversion: MirConversion,
    },
    Absent,
    ResultOk { value: MirValueId },
    ResultErr { value: MirValueId },
    Call { callee: MirCallee, args: Vec<MirCallArg>, type_args: Vec<MirType> },
    IndirectCall { callee: MirValueId, args: Vec<MirCallArg>, type_args: Vec<MirType> },
    Closure { function: MirFunctionId, captures: Vec<MirCaptureOperand>, facts: MirCaptureFacts },
    PtrFromAddr { addr: MirValueId, element: MirType },
    Deref { value: MirValueId },
    RawAddressOf { place: MirPlaceId },
    AddressOf { place: MirPlaceId, access: MirAccess },
    CoreCall {
        call: MirCoreCallId,
        route: MirPreludeCallId,
        args: Vec<MirCallArg>,
        type_args: Vec<MirType>,
        fallibility: MirCallFallibility,
        data_plan: Option<MirDataPlan>,
    },
    AttachTag { value: MirValueId, tag: Option<String> },
    Todo {
        call: MirPreludeCallId,
        location: MirPanicLoc,
        expected_type: Option<MirType>,
    },
    Never { reason: String },
    Semantic(MirSemanticOp),
    LoopRangeInit {
        call: MirPreludeCallId,
        start: MirValueId,
        end: MirValueId,
        step: Option<MirValueId>,
        exclusive: bool,
    },
    LoopRangeHasNext { call: MirPreludeCallId, cursor: MirValueId },
    LoopRangeValue { call: MirPreludeCallId, cursor: MirValueId },
    LoopRangeAdvance { call: MirPreludeCallId, cursor: MirValueId },
    LoopIterInit {
        call: MirPreludeCallId,
        collection: MirValueId,
        step: Option<MirValueId>,
        by_value: bool,
        source_kind: MirLoopSourceKind,
    },
    LoopIterHasNext { call: MirPreludeCallId, cursor: MirValueId },
    LoopIterValue { call: MirPreludeCallId, cursor: MirValueId },
    LoopIterAdvance { call: MirPreludeCallId, cursor: MirValueId },
    ScopeEnter {
        scope: MirScopeId,
        test_member: Option<MirTestScopeMember>,
    },
    ScopeExit { scope: MirScopeId },
    Drop { value: MirValueId, kind: MirDropKind },
}

impl MirOperation {
    pub fn value_uses(&self) -> Vec<MirValueId> {
        let mut out = Vec::new();
        match self {
            Self::Parameter { .. }
            | Self::Capture { .. }
            | Self::Global { .. }
            | Self::Constant(_)
            | Self::Absent
            | Self::Todo { .. }
            | Self::Never { .. }
            | Self::ScopeExit { .. } => {}
            Self::ScopeEnter { test_member, .. } => {
                if let Some(MirTestScopeMember::Timeout { duration }) = test_member {
                    out.push(*duration);
                }
            }
            Self::Phi { incoming } => out.extend(incoming.iter().map(|(_, value)| *value)),
            Self::ReadPlace(_) | Self::MovePlace { .. } | Self::InitializeUninit { .. } => {}
            Self::WritePlace { value, .. }
            | Self::Copy { value }
            | Self::Move { value }
            | Self::Unary { value, .. }
            | Self::Deref { value }
            | Self::AttachTag { value, .. }
            | Self::Drop { value, .. }
            | Self::Present { value }
            | Self::ResultOk { value }
            | Self::ResultErr { value } => out.push(*value),
            Self::Binary { left, right, .. } => {
                out.push(*left);
                out.push(*right);
            }
            Self::BuildList { values } => out.extend(values.iter().copied()),
            Self::EnumIs { subject, .. }
            | Self::EnumPayload { subject, .. }
            | Self::OptionIsSome { subject }
            | Self::OptionValue { subject }
            | Self::ResultIsOk { subject }
            | Self::ResultValue { subject, .. }
            | Self::PatternCapture {
                matched: subject, ..
            }
            | Self::PatternMatched { matched: subject } => out.push(*subject),
            Self::BuildString { parts } => out.extend(parts.iter().filter_map(|part| match part {
                MirStringPart::Value(value) => Some(*value),
                MirStringPart::Literal(_) => None,
            })),
            Self::BuildMap { entries } => {
                for (key, value) in entries {
                    out.push(*key);
                    out.push(*value);
                }
            }
            Self::ProjectMembers { base, .. } | Self::Field { base, .. } => out.push(*base),
            Self::Index { base, index, .. } => {
                out.push(*base);
                out.push(*index);
            }
            Self::Slice { base, start, end, range, .. } => {
                out.push(*base);
                out.push(*start);
                out.push(*end);
                if let Some(range) = range {
                    out.push(*range);
                }
            }
            Self::Range { start, end, .. } => {
                out.push(*start);
                out.push(*end);
            }
            Self::Struct { fields, .. } | Self::Tuple { fields, .. } => {
                out.extend(fields.iter().map(|(_, value)| *value));
            }
            Self::Enum { args, .. } => out.extend(args.iter().map(|arg| arg.value)),
            Self::Call { callee, args, .. } => {
                collect_callee_uses(callee, &mut out);
                out.extend(args.iter().map(|arg| arg.value));
            }
            Self::CoreCall { args, .. } => out.extend(args.iter().map(|arg| arg.value)),
            Self::IndirectCall { callee, args, .. } => {
                out.push(*callee);
                out.extend(args.iter().map(|arg| arg.value));
            }
            Self::Closure { captures, .. } => out.extend(captures.iter().filter_map(|capture| match capture {
                MirCaptureOperand::Value(value) => Some(*value),
                MirCaptureOperand::Place(_) => None,
            })),
            Self::PtrFromAddr { addr, .. } => out.push(*addr),
            Self::RawAddressOf { .. } | Self::AddressOf { .. } => {}
            Self::Convert {
                value, parameters, ..
            } => {
                out.push(*value);
                out.extend(parameters.iter().copied());
            }
            Self::Semantic(operation) => out.extend(operation.value_uses()),
            Self::LoopRangeInit { start, end, step, .. } => {
                out.push(*start);
                out.push(*end);
                if let Some(step) = step {
                    out.push(*step);
                }
            }
            Self::LoopRangeHasNext { cursor, .. }
            | Self::LoopRangeValue { cursor, .. }
            | Self::LoopRangeAdvance { cursor, .. }
            | Self::LoopIterHasNext { cursor, .. }
            | Self::LoopIterValue { cursor, .. }
            | Self::LoopIterAdvance { cursor, .. } => out.push(*cursor),
            Self::LoopIterInit { collection, step, .. } => {
                out.push(*collection);
                if let Some(step) = step {
                    out.push(*step);
                }
            }
        }
        out
    }
}

fn collect_callee_uses(callee: &MirCallee, out: &mut Vec<MirValueId>) {
    if let MirCallee::Indirect(value) = callee {
        out.push(*value);
    }
}



#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirForeignLanguage {
    C,
    Cpp,
    Rust,
    Assembly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirForeignAbi {
    C,
    CUnwind,
    System,
    Stdcall,
    Fastcall,
    Vectorcall,
    Rust,
    Platform(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirLinkArtifactKind {
    Object,
    StaticLibrary,
    DynamicLibrary,
    Framework,
    GeneratedSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirLinkArtifact {
    pub kind: MirLinkArtifactKind,
    pub path: String,
}

#[derive(Debug, Clone)]
pub struct MirLinkUnit {
    pub id: MirLinkUnitId,
    pub crate_spec: String,
    pub cache_identity: String,
    pub target_applicability: MirTargetApplicability,
    pub artifacts: Vec<MirLinkArtifact>,
    pub dependency_dirs: Vec<String>,
    pub link_closure: Vec<MirLinkUnitId>,
}

#[derive(Debug, Clone)]
pub struct MirCallbackAdapter {
    pub id: MirCallbackId,
    pub symbol: String,
    pub function: MirFunctionId,
    pub params: Vec<MirParam>,
    pub return_type: Option<MirType>,
    pub abi: MirForeignAbi,
    /// D-FFI-CALLBACK2=A: managed registrations may retain captures and must
    /// carry the exact generated plan identity into the runtime glue.
    pub managed: bool,
    pub plan_digest: Option<String>,
    pub callback_identity: Option<String>,
}
/// Checked C import resolution retained for native bridge consumers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirCImportLink {
    pub importing_module: String,
    pub scope: Option<String>,
    pub alias: String,
    pub target_module: String,
}

/// One checked C library binding and its owning module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirCLib {
    pub lib: String,
    pub module: String,
}

/// A generated C symbol replaced by a checked user overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirCOverlayOverride {
    pub lib: String,
    pub generated_symbol: String,
    pub overlay_symbol: String,
}

/// Compiler-owned adapter for a status-returning native close function.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirCloseAdapter {
    pub lib: String,
    pub handle_type: String,
    pub raw_function: String,
    pub adapter_function: String,
}

/// Complete checked C-FFI bridge facts. Handle lifecycle rows live in
/// `MirProgram::handles`; this row carries the bridge/link facts around them.
#[derive(Debug, Clone, Default)]
pub struct MirCffiFacts {
    pub import_links: Vec<MirCImportLink>,
    pub libs: Vec<MirCLib>,
    pub overlay_overrides: Vec<MirCOverlayOverride>,
    pub boundaries: Vec<crate::AST::FfiBoundaryFacts>,
    pub direct_links: Vec<String>,
    pub transitive_links: Vec<String>,
    pub close_adapters: Vec<MirCloseAdapter>,
}

impl MirCffiFacts {
    pub fn boundary_for(&self, library: &str) -> Option<&crate::AST::FfiBoundaryFacts> {
        self.boundaries
            .iter()
            .find(|boundary| boundary.library == library)
    }

    pub fn validate_boundaries(&self) -> Result<(), String> {
        let mut libraries = std::collections::BTreeSet::new();
        for boundary in &self.boundaries {
            boundary.validate()?;
            if !libraries.insert(boundary.library.as_str()) {
                return Err(format!(
                    "MIR carries duplicate foreign boundary `{}`",
                    boundary.library
                ));
            }
        }
        Ok(())
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirHandleOwnership {
    Owned,
    Shared,
    Borrowed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirHandlePayload {
    pub library: String,
    pub typedef_name: String,
    pub close: String,
}

#[derive(Debug, Clone)]
pub struct MirHandleLifecycle {
    pub id: MirHandleId,
    pub ty: MirType,
    pub ownership: MirHandleOwnership,
    pub payload: MirHandlePayload,
    pub close: Option<MirFunctionId>,
    pub close_foreign: Option<MirForeignId>,
    pub undo: Option<MirFunctionId>,
    pub send: bool,
    pub sync: bool,
    /// Checked provenance for the native close symbol. `None` is used by
    /// non-C runtime-owned handles.
    pub close_source: Option<crate::AST::FfiCloseSource>,
    /// Checked concurrency contract for the native handle.
    pub thread_safety: Option<crate::AST::FfiThreadSafety>,
}

/// One owned foreign resource token shared by every runtime alias of a handle.
///
/// The token carries the checked handle identity separately from the native
/// payload. Adapters may clone the token while lowering a read or passing a
/// borrowed argument, but only the first `take_raw` succeeds. This keeps
/// close-once ownership independent of native address reuse.
#[derive(Debug, Clone)]
pub struct MirHandleToken {
    state: Arc<MirHandleTokenState>,
}

#[derive(Debug)]
struct MirHandleTokenState {
    handle: MirHandleId,
    raw: Mutex<Option<i64>>,
}

impl MirHandleToken {
    pub fn new(handle: MirHandleId, raw: i64) -> Self {
        Self {
            state: Arc::new(MirHandleTokenState {
                handle,
                raw: Mutex::new(Some(raw)),
            }),
        }
    }

    pub fn handle_id(&self) -> MirHandleId {
        self.state.handle
    }

    pub fn identity(&self) -> usize {
        Arc::as_ptr(&self.state) as usize
    }

    pub fn raw(&self) -> Option<i64> {
        self.state
            .raw
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .copied()
    }

    pub fn take_raw(&self) -> Option<(MirHandleId, i64)> {
        let raw = self
            .state
            .raw
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()?;
        Some((self.state.handle, raw))
    }

    pub fn is_taken(&self) -> bool {
        self.raw().is_none()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirEntryKind {
    Library,
    Command,
    App,
    Service,
    Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirEntryOutput {
    None,
    ReturnValue,
    StandardOutput,
    ExitStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirCliValueKind {
    Bool,
    Int,
    Float,
    String,
    Path,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MirCliDefault {
    TypeDefault,
    Value(MirConstant),
}

#[derive(Debug, Clone, PartialEq)]
pub enum MirCliInputShape {
    Flag,
    Value {
        kind: MirCliValueKind,
        optional: bool,
        default: Option<MirCliDefault>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirCliInput {
    pub parameter: usize,
    pub name: String,
    pub label: String,
    pub ty: MirType,
    pub zone: MirParamZone,
    /// Long option spelling retained from checked CLI binding facts.
    pub flag: String,
    pub short: Option<String>,
    pub env: Option<String>,
    pub help: String,
    pub metavar: Option<String>,
    pub shape: MirCliInputShape,
    pub positional: Option<u16>,
    pub variadic: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirCliCommand {
    pub name: String,
    pub description: Option<String>,
    pub function: MirFunctionId,
    pub receiver: Option<MirTypeId>,
    pub inputs: Vec<MirCliInput>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirCliEntry {
    pub description: Option<String>,
    pub inputs: Vec<MirCliInput>,
    pub commands: Vec<MirCliCommand>,
    pub standard: bool,
    pub version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MirEntrySpec {
    pub kind: MirEntryKind,
    pub function: Option<MirFunctionId>,
    pub cli: Option<MirCliEntry>,
    pub output: MirEntryOutput,
    pub initialize_environment: bool,
    pub initialize_gc: bool,
    pub serves_until_stopped: bool,
    pub package_version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirJobScope {
    Dev,
    Ship,
    Internal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirJobSchedule {
    Duration { nanos: i64 },
    WallClockTime { hour: u8, minute: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirJobDispatch {
    Direct,
    Spawn,
    Scheduled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirJobSkip {
    Always(String),
    UnlessPlatform(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirJobCachePolicy {
    Uncached,
    Local,
    Shared,
}

#[derive(Debug, Clone)]
pub struct MirJob {
    pub id: MirJobId,
    pub function: MirFunctionId,
    pub name: String,
    pub doc: Option<String>,
    pub scope: MirJobScope,
    pub schedule: Option<MirJobSchedule>,
    pub inputs: Vec<MirCliInput>,
    pub dispatch: MirJobDispatch,
    /// D-DX-JOBGRAPH1=A: checked predecessor names in the one job namespace.
    pub after: Vec<String>,
    pub parallel: usize,
    pub packages: Vec<String>,
    pub working_directory: Option<String>,
    pub input_paths: Vec<String>,
    pub output_paths: Vec<String>,
    pub skip: Option<MirJobSkip>,
    pub cache: MirJobCachePolicy,
    pub limits: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirTestKind {
    Unit,
    Property,
}

#[derive(Debug, Clone)]
pub struct MirTestCase {
    pub id: MirTestId,
    pub function: MirFunctionId,
    pub name: String,
    pub span: Span,
    pub kind: MirTestKind,
    /// True when the test's property cases are generated under a checked
    /// contract rather than supplied directly by the source.
    pub contract_generated: bool,
    /// Contract-generated rows only (#2502): the compiler-synthesized
    /// pre-call eligibility predicate built from the candidate's own checked
    /// `#Pre` conditions. The harness calls it over the generated arguments
    /// BEFORE invoking `function`; a false result is a rejected input that
    /// never executes the callable and never becomes contract evidence.
    /// `None` on ordinary property rows and on unavailable contract rows.
    pub eligibility: Option<MirFunctionId>,
    pub parameters: Vec<MirParam>,
    pub faults: Vec<String>,
    pub expected_failure: bool,
    /// Stable reason when a synthetic property generator could not be
    /// materialized; `None` means generation is available or not applicable.
    pub generation_unavailable_reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MirOutputCheck {
    pub id: MirOutputCheckId,
    pub name: String,
    pub function: MirFunctionId,
}

#[derive(Debug, Clone)]
pub struct MirCoveragePoint {
    pub id: MirCoveragePointId,
    pub function: MirFunctionId,
    pub block: MirBlockId,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirHarnessKind {
    Test,
    Fuzz,
    Coverage,
}

#[derive(Debug, Clone)]
pub struct MirHarnessPlan {
    pub id: MirHarnessId,
    pub kind: MirHarnessKind,
    pub tests: Vec<MirTestId>,
    pub output_checks: Vec<MirOutputCheck>,
    pub selected_test: Option<MirTestId>,
    pub coverage_points: Vec<MirCoveragePoint>,
    pub command_override: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirArtifactKind {
    NativeExecutable,
    NativeLibrary,
    SandboxPlugin,
    WebApplication,
    TestExecutable,
    FuzzExecutable,
    TestOverride,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirArtifactTarget {
    RustAot,
    Cranelift,
    Interpreter,
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirArtifactBuildMode {
    Dev,
    Release,
    Test,
    Fuzz,
    Coverage,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirArtifactRequest {
    pub target: MirArtifactTarget,
    pub kind: MirArtifactKind,
    pub mode: MirArtifactBuildMode,
    pub name: Option<String>,
}

impl MirArtifactRequest {
    pub const fn new(
        target: MirArtifactTarget,
        kind: MirArtifactKind,
        mode: MirArtifactBuildMode,
    ) -> Self {
        Self {
            target,
            kind,
            mode,
            name: None,
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }
}

#[derive(Debug, Clone)]
pub struct MirExport {
    pub symbol: String,
    pub function: MirFunctionId,
    pub abi: MirForeignAbi,
}

impl MirArtifactKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NativeExecutable => "native-executable",
            Self::NativeLibrary => "native-library",
            Self::SandboxPlugin => "sandbox-plugin",
            Self::WebApplication => "web-application",
            Self::TestExecutable => "test-executable",
            Self::FuzzExecutable => "fuzz-executable",
            Self::TestOverride => "test-override",
        }
    }
}

impl MirArtifactTarget {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RustAot => "rust-aot",
            Self::Cranelift => "cranelift",
            Self::Interpreter => "interpreter",
            Self::Web => "web",
        }
    }
}

impl MirArtifactBuildMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Release => "release",
            Self::Test => "test",
            Self::Fuzz => "fuzz",
            Self::Coverage => "coverage",
        }
    }
}

#[derive(Debug, Clone)]
pub struct MirArtifactPlan {
    pub id: MirArtifactId,
    pub kind: MirArtifactKind,
    pub name: String,
    pub target: MirArtifactTarget,
    pub mode: MirArtifactBuildMode,
    pub modules: Vec<MirModuleId>,
    pub links: Vec<MirLinkUnitId>,
    pub jobs: Vec<MirJobId>,
    pub runtime_parts: BTreeSet<MirRuntimePartId>,
    pub exports: Vec<MirExport>,
    pub provider_identity: String,
    pub closure_identity: String,
    pub artifact_identity: String,
    pub entry: Option<MirEntrySpec>,
    pub harness: Option<MirHarnessId>,
}

/// Version shared by artifact, execution, and frame identities.
pub const MIR_IDENTITY_SCHEMA_VERSION: u16 = 1;

/// Stable JSON schema for the complete optimized MIR program identity.
pub const MIR_IDENTITY_SCHEMA: &str = "mir-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirSourceMapIdentity {
    pub id: u64,
    pub path: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirProgramIdentity {
    pub semantic_hash: String,
    pub optimized_hash: String,
    pub function_ids: Vec<u64>,
    pub core_ids: Vec<u64>,
    pub target_facts: Vec<(String, String)>,
    pub source_map: Vec<MirSourceMapIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirArtifactIdentity {
    pub schema_version: u16,
    pub mir_schema_version: u16,
    pub program_digest: [u8; 32],
    pub package_identity: String,
    pub artifact: MirArtifactId,
    pub name: String,
    pub kind: MirArtifactKind,
    pub target: MirArtifactTarget,
    pub mode: MirArtifactBuildMode,
    pub provider_identity: String,
    pub closure_identity: String,
    pub artifact_identity: String,
    pub program_identity: MirProgramIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirExecutionIdentity {
    pub schema_version: u16,
    pub artifact: MirArtifactIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirFrameIdentity {
    pub schema_version: u16,
    pub execution: MirExecutionIdentity,
    pub function: MirFunctionId,
    pub block: Option<MirBlockId>,
    pub sequence: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirIdentityError {
    MissingArtifact(MirArtifactId),
    AmbiguousArtifact,
    Unoptimized(String),
}

impl fmt::Display for MirIdentityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingArtifact(id) => write!(formatter, "MIR artifact {:?} is missing", id),
            Self::AmbiguousArtifact => {
                formatter.write_str("MIR execution identity needs an explicit artifact")
            }
            Self::Unoptimized(reason) => formatter.write_str(reason),
        }
    }
}

impl std::error::Error for MirIdentityError {}

fn write_identity_field(
    out: &mut impl fmt::Write,
    name: &str,
    value: &str,
) -> fmt::Result {
    write!(out, "{}:{}:{};", name.len(), name, value.len())?;
    out.write_str(value)?;
    out.write_char(';')
}

fn write_digest(out: &mut impl fmt::Write, digest: &[u8; 32]) -> fmt::Result {
    for byte in digest {
        write!(out, "{byte:02x}")?;
    }
    Ok(())
}

fn digest_hex(digest: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    write_digest(&mut out, digest).expect("writing a MIR digest to String cannot fail");
    out
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            character if character < ' ' => {
                out.push_str(&format!("\\u{:04x}", character as u32))
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

impl MirProgramIdentity {
    pub fn unavailable(program_digest: String, source_map: Vec<MirSourceMapIdentity>) -> Self {
        Self {
            semantic_hash: program_digest.clone(),
            optimized_hash: program_digest,
            function_ids: Vec::new(),
            core_ids: Vec::new(),
            target_facts: vec![("availability".to_string(), "unavailable".to_string())],
            source_map,
        }
    }

    fn canonical_parts(&self, ids_as_strings: bool) -> (String, String, String, String) {
        let mut function_ids = self.function_ids.clone();
        function_ids.sort_unstable();
        function_ids.dedup();
        let function_ids = function_ids
            .iter()
            .map(|id| {
                if ids_as_strings {
                    json_string(&id.to_string())
                } else {
                    id.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(",");

        let mut core_ids = self.core_ids.clone();
        core_ids.sort_unstable();
        core_ids.dedup();
        let core_ids = core_ids
            .iter()
            .map(|id| {
                if ids_as_strings {
                    json_string(&id.to_string())
                } else {
                    id.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join(",");

        let mut source_map = self.source_map.clone();
        source_map.sort_by(|left, right| left.id.cmp(&right.id).then(left.path.cmp(&right.path)));
        let source_map = source_map
            .iter()
            .map(|entry| {
                let id = if ids_as_strings {
                    json_string(&entry.id.to_string())
                } else {
                    entry.id.to_string()
                };
                format!(
                    "{{\"digest\":{},\"id\":{},\"path\":{}}}",
                    json_string(&entry.digest),
                    id,
                    json_string(&entry.path)
                )
            })
            .collect::<Vec<_>>()
            .join(",");

        let mut target_facts = self.target_facts.clone();
        target_facts.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        let target_facts = target_facts
            .iter()
            .map(|(key, value)| {
                format!(
                    "{{\"key\":{},\"value\":{}}}",
                    json_string(key),
                    json_string(value)
                )
            })
            .collect::<Vec<_>>()
            .join(",");

        (core_ids, function_ids, source_map, target_facts)
    }

    fn canonical_payload(&self) -> String {
        let (core_ids, function_ids, source_map, target_facts) = self.canonical_parts(false);
        format!(
            "{{\"core_ids\":[{core_ids}],\"function_ids\":[{function_ids}],\"optimized_hash\":{},\"schema\":{},\"semantic_hash\":{},\"source_map\":[{source_map}],\"target_facts\":[{target_facts}]}}",
            json_string(&self.optimized_hash),
            json_string(MIR_IDENTITY_SCHEMA),
            json_string(&self.semantic_hash),
        )
    }

    pub fn identity_digest(&self) -> String {
        crate::SHA256::sha256_hex(self.canonical_payload().as_bytes())
    }

    pub fn canonical_json(&self) -> String {
        let (core_ids, function_ids, source_map, target_facts) = self.canonical_parts(true);
        format!(
            "{{\"core_ids\":[{core_ids}],\"function_ids\":[{function_ids}],\"identity_digest\":{},\"optimized_hash\":{},\"schema\":{},\"semantic_hash\":{},\"source_map\":[{source_map}],\"target_facts\":[{target_facts}]}}",
            json_string(&self.identity_digest()),
            json_string(&self.optimized_hash),
            json_string(MIR_IDENTITY_SCHEMA),
            json_string(&self.semantic_hash),
        )
    }
}

impl MirArtifactIdentity {
    pub fn write_canonical(&self, out: &mut impl fmt::Write) -> fmt::Result {
        write!(
            out,
            "mir-artifact-identity:{}:{}:",
            self.schema_version, self.mir_schema_version
        )?;
        write_digest(out, &self.program_digest)?;
        out.write_char(';')?;
        write_identity_field(out, "package", &self.package_identity)?;
        write!(out, "artifact:{};", self.artifact.0)?;
        write_identity_field(out, "name", &self.name)?;
        write_identity_field(out, "kind", self.kind.as_str())?;
        write_identity_field(out, "target", self.target.as_str())?;
        write_identity_field(out, "mode", self.mode.as_str())?;
        write_identity_field(out, "provider", &self.provider_identity)?;
        write_identity_field(out, "closure", &self.closure_identity)?;
        write_identity_field(out, "artifact-key", &self.artifact_identity)
    }

    pub fn canonical_text(&self) -> String {
        let mut out = String::new();
        self.write_canonical(&mut out)
            .expect("writing a MIR identity to String cannot fail");
        out
    }

    pub fn identity_digest(&self) -> String {
        self.program_identity.identity_digest()
    }

    pub fn canonical_json(&self) -> String {
        self.program_identity.canonical_json()
    }
}


impl MirExecutionIdentity {
    pub fn write_canonical(&self, out: &mut impl fmt::Write) -> fmt::Result {
        write!(out, "mir-execution-identity:{};", self.schema_version)?;
        self.artifact.write_canonical(out)
    }

    pub fn canonical_text(&self) -> String {
        let mut out = String::new();
        self.write_canonical(&mut out)
            .expect("writing a MIR identity to String cannot fail");
        out
    }
}

impl MirFrameIdentity {
    pub fn write_canonical(&self, out: &mut impl fmt::Write) -> fmt::Result {
        write!(out, "mir-frame-identity:{};", self.schema_version)?;
        self.execution.write_canonical(out)?;
        write!(out, ";function:{};", self.function.0)?;
        match self.block {
            Some(block) => write!(out, "block:{};", block.0)?,
            None => out.write_str("block:none;")?,
        }
        write!(out, "sequence:{};", self.sequence)
    }

    pub fn canonical_text(&self) -> String {
        let mut out = String::new();
        self.write_canonical(&mut out)
            .expect("writing a MIR identity to String cannot fail");
        out
    }
}

#[derive(Debug, Clone)]
pub struct MirForeign {
    pub id: MirForeignId,
    pub module_id: MirModuleId,
    pub key: String,
    pub module: String,
    pub name: String,
    pub span: Span,
    pub symbol: String,
    pub path: String,
    pub params: Vec<MirParam>,
    pub return_type: Option<MirType>,
    pub foreign_abi: MirForeignAbi,
    pub foreign_language: MirForeignLanguage,
    pub target_applicability: MirTargetApplicability,
    pub effects: MirEffectFacts,
    /// D-FFI-CALLBACK2=A: generated callback transport and exact plan facts.
    pub callback_transport: Option<String>,
    pub callback_plan_digest: Option<String>,
    pub callback_identity: Option<String>,
    pub link: Option<MirLinkUnitId>,
    pub callback: Option<MirCallbackId>,
    pub handle: Option<MirHandleId>,
    pub close_function: Option<MirFunctionId>,
    pub close_foreign: Option<MirForeignId>,
    pub undo_function: Option<MirFunctionId>,
}
#[derive(Debug, Clone)]
pub struct MirInstruction {
    pub id: MirOpId,
    pub span: Span,
    /// Checked source line carried separately from byte-offset Span.
    pub source_line: Option<u32>,
    pub result: Option<MirValueId>,
    pub ty: Option<MirType>,
    pub operation: MirOperation,
}
#[derive(Debug, Clone)]
pub struct MirBasicBlock {
    pub id: MirBlockId,
    pub span: Span,
    pub instructions: Vec<MirInstruction>,
    pub terminator: MirTerminator,
}

#[derive(Debug, Clone)]
pub struct MirSwitchArm {
    pub condition: MirValueId,
    pub target: MirBlockId,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum MirTerminator {
    Jump { target: MirBlockId },
    Branch { condition: MirValueId, then_target: MirBlockId, else_target: MirBlockId },
    Switch { subject: MirValueId, arms: Vec<MirSwitchArm>, otherwise: MirBlockId },
    Return { value: Option<MirValueId> },
    Yield { value: MirValueId, resume: MirBlockId },
    Break { target: MirBlockId, value: Option<MirValueId> },
    Continue { target: MirBlockId },
    Unreachable { reason: String },
}

impl MirTerminator {
    pub fn targets(&self) -> Vec<MirBlockId> {
        match self {
            Self::Jump { target }
            | Self::Continue { target }
            | Self::Yield { resume: target, .. } => vec![*target],
            Self::Branch { then_target, else_target, .. } => vec![*then_target, *else_target],
            Self::Switch { arms, otherwise, .. } => arms
                .iter()
                .map(|arm| arm.target)
                .chain(std::iter::once(*otherwise))
                .collect(),
            Self::Break { target, .. } => vec![*target],
            Self::Return { .. } | Self::Unreachable { .. } => Vec::new(),
        }
    }

    pub fn value_uses(&self) -> Vec<MirValueId> {
        match self {
            Self::Branch { condition, .. } => vec![*condition],
            Self::Switch { subject, arms, .. } => std::iter::once(*subject)
                .chain(arms.iter().map(|arm| arm.condition))
                .collect(),
            Self::Return { value } | Self::Break { value, .. } => {
                value.iter().copied().collect()
            }
            Self::Yield { value, .. } => vec![*value],
            Self::Jump { .. } | Self::Continue { .. } | Self::Unreachable { .. } => Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum MirDropEdge {
    Normal,
    Return,
    Failure(MirBlockId),
    Unwind(MirBlockId),
}

#[derive(Debug, Clone)]
pub struct MirDropAction {
    pub place: MirPlaceId,
    pub edge: MirDropEdge,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct MirFunction {
    pub id: MirFunctionId,
    pub module_id: MirModuleId,
    pub key: String,
    pub module: String,
    pub name: String,
    pub span: Span,
    pub kind: MirFunctionKind,
    pub form: MirFunctionForm,
    pub visibility: MirVisibility,
    pub target_applicability: MirTargetApplicability,
    pub web_bucket: Option<crate::WebPartition::WebBucket>,
    pub web_marker: Option<crate::WebPartition::WebPartitionMarker>,
    pub generic_params: Vec<MirGenericParam>,
    pub capture_params: Vec<MirCaptureParam>,
    pub params: Vec<MirParam>,
    pub declared_return: Option<MirType>,
    pub return_type: MirType,
    pub failure: MirFailureCarrier,
    pub effects: MirEffectFacts,
    pub captures: Option<MirCaptureFacts>,
    pub generator: Option<MirGeneratorFacts>,
    pub optimization: MirOptimizationFacts,
    pub is_unsafe: bool,
    pub unsafe_gate: Option<MirUnsafeGate>,
    pub is_pure: bool,
    pub memo_bound: Option<Option<usize>>,
    pub is_reactive: bool,
    pub reactive_upgrades: Vec<String>,
    pub is_inline: bool,
    pub is_inline_always: bool,
    pub is_scalar: bool,
    pub kernel_proof: Option<MirKernelFacts>,
    pub gc_return: bool,
    pub return_view_provenance: Option<BTreeMap<Vec<String>, MirViewProvenance>>,
    pub web_param_reconstructions: Vec<MirWebParamReconstruction>,
    pub blocks: Vec<MirBasicBlock>,
    pub entry: MirBlockId,
    pub locals: Vec<MirLocal>,
    pub values: Vec<(MirValueId, MirType, Span, MirOwnership)>,
    pub places: Vec<MirPlace>,
    pub scopes: Vec<MirScope>,
    pub drops: Vec<MirDropAction>,
    pub foreign_language: Option<String>,
}

impl MirFunction {
    pub fn test_scope_member(&self, scope: MirScopeId) -> Option<&MirTestScopeMember> {
        self.blocks.iter().find_map(|block| {
            block.instructions.iter().find_map(|instruction| {
                match &instruction.operation {
                    MirOperation::ScopeEnter {
                        scope: candidate,
                        test_member,
                    } if *candidate == scope => test_member.as_ref(),
                    _ => None,
                }
            })
        })
    }

    pub fn is_decode_for(&self, target: &MirType) -> bool {
        let MirFunctionForm::TraitMethod {
            owner,
            self_access,
            serde: Some(MirSerdeCodec::Decode),
            ..
        } = &self.form
        else {
            return false;
        };
        self_access.is_none()
            && self.generic_params.is_empty()
            && (owner.same_checked_type(target)
                || self
                    .name
                    .strip_suffix("::decode")
                    .is_some_and(|owner| owner == target.display_name()))
    }
}

#[derive(Debug, Clone)]
pub struct MirCoreCall {
    pub id: MirCoreCallId,
    pub key: String,
    pub module: String,
    pub member: String,
    pub receiver_types: Vec<String>,
    pub arity: usize,
    pub max_arity: usize,
    pub borrow_mask: Vec<bool>,
    pub fallibility: CoreCallFallibility,
    pub effect: Option<Effect>,
    pub effect_leaf: Option<String>,
    pub sink_class: Option<SinkClass>,
    pub pure_route: CoreCallPureRoute,
    pub interpreter_route: CoreCallInterpreterRoute,
    pub symbol: CoreCallSymbol,
    pub coverage_bits: u8,
    pub aot_direct: bool,
    pub jit_direct: bool,
    pub jit_symbol: Option<String>,
    pub marker: Option<CoreMarkerApplication>,
}

impl MirCoreCall {
    pub fn from_record(record: &'static crate::Syntax::CoreCallRecord) -> Self {
        let key = if record.receiver_types.is_empty() {
            format!("{}.{}", record.module, record.member)
        } else {
            format!("receiver({}).{}", record.receiver_types.join("|"), record.member)
        };
        Self {
            id: MirCoreCallId(stable_id("core-call", &key)),
            key,
            module: record.module.to_string(),
            member: record.member.to_string(),
            receiver_types: record.receiver_types.iter().map(|name| (*name).to_string()).collect(),
            arity: record.signature.arity,
            max_arity: record.signature.max_arity,
            borrow_mask: record.signature.borrow_mask.to_vec(),
            fallibility: record.fallibility,
            effect: record.effect,
            effect_leaf: record.effect_leaf.map(str::to_string),
            sink_class: record.sink_class,
            pure_route: record.pure_route,
            interpreter_route: record.interpreter_route,
            symbol: record.symbol,
            coverage_bits: record.coverage.bits(),
            aot_direct: record.aot_direct,
            jit_direct: record.jit_direct,
            jit_symbol: record.jit_symbol.map(str::to_string),
            marker: record.marker,
        }
    }
}

/// Typed runtime/Prelude parts selected during semantic lowering.
///
/// Adapters may map these IDs to target artifacts, but never rediscover the
/// selection from textual Core module names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MirRuntimePartId {
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
}
impl MirRuntimePartId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Gc => "gc",
            Self::Event => "event",
            Self::Realtime => "realtime",
            Self::EmbeddedHardware => "embedded-hardware",
            Self::Ui => "ui",
            Self::Devtools => "devtools",
            Self::Gtk => "gtk",
            Self::Apps => "apps",
            Self::Email => "email",
            Self::Game => "game",
            Self::Files => "files",
            Self::Interrupt => "interrupt",
            Self::FsRuntime => "fs-runtime",
            Self::Process => "process",
            Self::Crypto => "crypto",
            Self::Math => "math",
            Self::Encoding => "encoding",
            Self::Data => "data",
            Self::Fmt => "fmt",
            Self::DataFmt => "data-fmt",
            Self::Compute => "compute",
            Self::Http => "http",
            Self::WebSocket => "websocket",
            Self::Browser => "browser",
            Self::Args => "args",
            Self::Reflect => "reflect",
            Self::AuthTokens => "auth-tokens",
            Self::AuthSession => "auth-session",
            Self::Sync => "sync",
            Self::Services => "services",
            Self::Mod => "mod",
        }
    }
}

/// Checked name/reference ledger carried across the canonical MIR boundary.
#[derive(Debug, Clone, Default)]
pub struct MirNameFacts {
    pub modules: Vec<crate::Names::NameModule>,
    pub declarations: Vec<crate::Names::NameDeclaration>,
    pub aliases: Vec<crate::Names::NameAlias>,
    pub references: Vec<crate::Names::NameReference>,
    pub structure_facts: Vec<crate::Names::StructureFact>,
}

#[derive(Debug, Clone, Default)]
pub struct MirPackageFacts {
    pub project_root: String,
    pub edition: String,
    pub runtime_parts: BTreeSet<MirRuntimePartId>,
    /// Whether the checked Core closure reaches `core.data.arrow`.
    ///
    /// `core_calls` is the static Core ABI registry, not a reachability list;
    /// keep this fact explicit so AOT Prelude emission does not infer Arrow
    /// from every available Core row.
    pub uses_arrow: bool,
    pub active_os: String,
    pub inferred_layer: String,
    pub allocator: String,
    /// Package version selected by checked build facts.
    pub package_version: String,
    /// Artifact target selected for this MIR request.
    pub artifact_target: Option<MirArtifactTarget>,
    /// Checked runtime layer, concrete providers, and their artifact identity.
    pub target_dossier: crate::Facts::TargetDossier,
    pub web_app: Option<crate::App::AppGraph>,
    /// D-MODEL-SIGNATURE1: loader-projected model output payloads consumed by
    /// native, web, and resident hosts without compiler/package-model linkage.
    pub model_outputs: Vec<crate::AST::ModelOutputFact>,
    /// D-PLUGIN-AUTHORITY1: package-declared guest capability needs.
    pub authority_needs: Vec<String>,
    pub hardware_use: crate::TargetMachine::TargetHardwareUse,
    pub hardware_profile: Option<crate::TargetMachine::TargetHardwareFacts>,
    pub hardware_profile_id: String,
    pub hardware_capabilities: Vec<String>,
    /// Checked package-level DMA configuration and interrupt bindings.
    pub hardware_setups: Vec<MirHardwareSetup>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirErasureReason {
    CompileTimeOnly,
    Disabled,
    ExpandedBeforeLowering,
    SemanticIndexOnly,
}

#[derive(Debug, Clone)]
pub struct MirUnreachable {
    pub construct: String,
    pub span: Span,
    pub reason: MirErasureReason,
}

#[derive(Debug, Clone)]
pub struct MirProgram {
    pub cffi: MirCffiFacts,
    pub schema_version: u16,
    pub package_identity: String,
    pub facts: MirPackageFacts,
    pub names: MirNameFacts,
    pub modules: Vec<MirModule>,
    pub imports: Vec<MirImport>,
    pub types: Vec<MirTypeDef>,
    pub traits: Vec<MirTraitDef>,
    pub impls: Vec<MirImplDef>,
    pub constants: Vec<MirConstantDef>,
    pub fields: Vec<MirFieldRow>,
    pub source_files: Vec<MirSourceFile>,
    pub functions: Vec<MirFunction>,
    pub foreign: Vec<MirForeign>,
    pub links: Vec<MirLinkUnit>,
    pub callbacks: Vec<MirCallbackAdapter>,
    pub handles: Vec<MirHandleLifecycle>,
    pub jobs: Vec<MirJob>,
    pub tests: Vec<MirTestCase>,
    pub harnesses: Vec<MirHarnessPlan>,
    pub artifacts: Vec<MirArtifactPlan>,
    pub core_calls: Vec<MirCoreCall>,
    pub prelude_calls: Vec<MirPreludeCall>,
    pub type_instances: Vec<MirType>,
    pub unreachable: Vec<MirUnreachable>,
}

/// Structured snapshot of the canonical MIR program used by the opt-in
/// pass-boundary journal. Every function, block, instruction, and typed
/// operation is taken directly from the lowered program.
pub fn canonical_payload(program: &MirProgram) -> String {
    let functions = program
        .functions
        .iter()
        .map(canonical_function)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"representation\":\"mir\",\"schema_version\":{},\"package\":\"{}\",\"functions\":[{}]}}",
        program.schema_version,
        json_escape(&program.package_identity),
        functions
    )
}

pub fn canonical_identity(program: &MirProgram) -> String {
    let functions = program
        .functions
        .iter()
        .map(|function| {
            format!(
                "{{\"id\":\"{}\",\"key\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}}}}",
                function.id.0,
                json_escape(&function.key),
                function.span.start,
                function.span.end
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"representation\":\"mir\",\"schema_version\":{},\"package\":\"{}\",\"functions\":[{}]}}",
        program.schema_version,
        json_escape(&program.package_identity),
        functions
    )
}

fn canonical_function(function: &MirFunction) -> String {
    let blocks = function
        .blocks
        .iter()
        .map(canonical_block)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"id\":\"{}\",\"key\":\"{}\",\"module\":\"{}\",\"name\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}},\"return_type\":{},\"blocks\":[{}]}}",
        function.id.0,
        json_escape(&function.key),
        json_escape(&function.module),
        json_escape(&function.name),
        function.span.start,
        function.span.end,
        canonical_type(&function.return_type),
        blocks
    )
}

fn canonical_block(block: &MirBasicBlock) -> String {
    let instructions = block
        .instructions
        .iter()
        .map(canonical_instruction)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"id\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}},\"instructions\":[{}],\"terminator\":\"{}\"}}",
        block.id.0,
        block.span.start,
        block.span.end,
        instructions,
        canonical_terminator(&block.terminator)
    )
}

fn canonical_instruction(instruction: &MirInstruction) -> String {
    let result = instruction
        .result
        .map(|value| format!("\"{}\"", value.0))
        .unwrap_or_else(|| "null".to_string());
    let ty = instruction
        .ty
        .as_ref()
        .map(canonical_type)
        .unwrap_or_else(|| "null".to_string());
    let uses = instruction
        .operation
        .value_uses()
        .iter()
        .map(|value| format!("\"{}\"", value.0))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"id\":\"{}\",\"span\":{{\"start\":{},\"end\":{}}},\"source_line\":{},\"result\":{},\"type\":{},\"operation\":{{\"kind\":\"{}\",\"value_uses\":[{}]}}}}",
        instruction.id.0,
        instruction.span.start,
        instruction.span.end,
        instruction
            .source_line
            .map(|line| line.to_string())
            .unwrap_or_else(|| "null".to_string()),
        result,
        ty,
        operation_kind(&instruction.operation),
        uses
    )
}

fn canonical_type(ty: &MirType) -> String {
    let identity = ty
        .identity
        .map(|id| format!("\"{}\"", id.0))
        .unwrap_or_else(|| "null".to_string());
    format!(
        "{{\"key\":\"{}\",\"identity\":{},\"abi\":\"{:?}\",\"size\":{},\"align\":{}}}",
        json_escape(&ty.canonical_key()),
        identity,
        ty.layout.abi,
        canonical_size(ty.layout.size),
        canonical_size(ty.layout.align)
    )
}

fn canonical_size(size: MirSize) -> String {
    match size {
        MirSize::Static(value) => value.to_string(),
        MirSize::Dynamic => "null".to_string(),
    }
}

fn canonical_terminator(terminator: &MirTerminator) -> &'static str {
    match terminator {
        MirTerminator::Jump { .. } => "Jump",
        MirTerminator::Branch { .. } => "Branch",
        MirTerminator::Switch { .. } => "Switch",
        MirTerminator::Return { .. } => "Return",
        MirTerminator::Yield { .. } => "Yield",
        MirTerminator::Break { .. } => "Break",
        MirTerminator::Continue { .. } => "Continue",
        MirTerminator::Unreachable { .. } => "Unreachable",
    }
}

fn operation_kind(operation: &MirOperation) -> &'static str {
    match operation {
        MirOperation::Parameter { .. } => "Parameter",
        MirOperation::Capture { .. } => "Capture",
        MirOperation::Global { .. } => "Global",
        MirOperation::Phi { .. } => "Phi",
        MirOperation::ReadPlace(_) => "ReadPlace",
        MirOperation::MovePlace { .. } => "MovePlace",
        MirOperation::WritePlace { .. } => "WritePlace",
        MirOperation::InitializeUninit { .. } => "InitializeUninit",
        MirOperation::Copy { .. } => "Copy",
        MirOperation::Move { .. } => "Move",
        MirOperation::Constant(_) => "Constant",
        MirOperation::Unary { .. } => "Unary",
        MirOperation::Binary { .. } => "Binary",
        MirOperation::BuildString { .. } => "BuildString",
        MirOperation::BuildList { .. } => "BuildList",
        MirOperation::BuildMap { .. } => "BuildMap",
        MirOperation::EnumIs { .. } => "EnumIs",
        MirOperation::EnumPayload { .. } => "EnumPayload",
        MirOperation::OptionIsSome { .. } => "OptionIsSome",
        MirOperation::OptionValue { .. } => "OptionValue",
        MirOperation::ResultIsOk { .. } => "ResultIsOk",
        MirOperation::ResultValue { .. } => "ResultValue",
        MirOperation::PatternCapture { .. } => "PatternCapture",
        MirOperation::PatternMatched { .. } => "PatternMatched",
        MirOperation::ProjectMembers { .. } => "ProjectMembers",
        MirOperation::Index { .. } => "Index",
        MirOperation::Slice { .. } => "Slice",
        MirOperation::Range { .. } => "Range",
        MirOperation::Field { .. } => "Field",
        MirOperation::Struct { .. } => "Struct",
        MirOperation::Enum { .. } => "Enum",
        MirOperation::Tuple { .. } => "Tuple",
        MirOperation::Present { .. } => "Present",
        MirOperation::Convert { .. } => "Convert",
        MirOperation::Absent => "Absent",
        MirOperation::ResultOk { .. } => "ResultOk",
        MirOperation::ResultErr { .. } => "ResultErr",
        MirOperation::Call { .. } => "Call",
        MirOperation::IndirectCall { .. } => "IndirectCall",
        MirOperation::Closure { .. } => "Closure",
        MirOperation::PtrFromAddr { .. } => "PtrFromAddr",
        MirOperation::Deref { .. } => "Deref",
        MirOperation::RawAddressOf { .. } => "RawAddressOf",
        MirOperation::AddressOf { .. } => "AddressOf",
        MirOperation::CoreCall { .. } => "CoreCall",
        MirOperation::AttachTag { .. } => "AttachTag",
        MirOperation::Todo { .. } => "Todo",
        MirOperation::Never { .. } => "Never",
        MirOperation::Semantic(_) => "Semantic",
        MirOperation::LoopRangeInit { .. } => "LoopRangeInit",
        MirOperation::LoopRangeHasNext { .. } => "LoopRangeHasNext",
        MirOperation::LoopRangeValue { .. } => "LoopRangeValue",
        MirOperation::LoopRangeAdvance { .. } => "LoopRangeAdvance",
        MirOperation::LoopIterInit { .. } => "LoopIterInit",
        MirOperation::LoopIterHasNext { .. } => "LoopIterHasNext",
        MirOperation::LoopIterValue { .. } => "LoopIterValue",
        MirOperation::LoopIterAdvance { .. } => "LoopIterAdvance",
        MirOperation::ScopeEnter { .. } => "ScopeEnter",
        MirOperation::ScopeExit { .. } => "ScopeExit",
        MirOperation::Drop { .. } => "Drop",
    }
}

pub fn canonical_operation_payload(operation: &MirOperation) -> String {
    let uses = operation
        .value_uses()
        .iter()
        .map(|value| format!("\"{}\"", value.0))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"representation\":\"mir\",\"operation_kind\":\"{}\",\"value_uses\":[{}]}}",
        operation_kind(operation),
        uses
    )
}

pub fn canonical_operation_identity(operation: &MirOperation) -> String {
    format!(
        "{{\"representation\":\"mir\",\"operation_kind\":\"{}\",\"value_use_count\":{}}}",
        operation_kind(operation),
        operation.value_uses().len()
    )
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirValidationError {
    SchemaVersion(u16),
    ZeroId { kind: &'static str, span: Option<Span> },
    DuplicateId { kind: &'static str, id: u64 },
    DuplicateKey { kind: &'static str, key: String },
    MissingReference { kind: &'static str, id: u64 },
    MissingEntry(MirFunctionId),
    MissingBlock { function: MirFunctionId, block: MirBlockId },
    MissingType {
        function: MirFunctionId,
        ty: MirTypeId,
    },
    MismatchedValueType { function: MirFunctionId, value: MirValueId },
    MissingValue { function: MirFunctionId, value: MirValueId },
    MissingTypeIdentity,
    MissingTypeInstance(MirTypeId),
    MissingPlace { function: MirFunctionId, place: MirPlaceId },
    MissingLocal { function: MirFunctionId, local: MirLocalId },
    MissingScope { function: MirFunctionId, scope: MirScopeId },
    InvalidParameter { function: MirFunctionId, index: usize },
    InvalidCapture { function: MirFunctionId, slot: usize },
    InvalidCaptureCount {
        function: MirFunctionId,
        target: MirFunctionId,
        expected: usize,
        actual: usize,
    },
    MismatchedCaptureType {
        function: MirFunctionId,
        target: MirFunctionId,
        slot: usize,
    },
    MissingCoreCall { function: MirFunctionId, call: MirCoreCallId },
    MissingPreludeCall { function: MirFunctionId, call: MirPreludeCallId },
    MissingField { function: MirFunctionId, field: MirFieldId },
    MissingSourceFile {
        function: MirFunctionId,
        source_file: MirSourceFileId,
    },
    UseBeforeDefinition { function: MirFunctionId, value: MirValueId, span: Span },
    InvalidTerminator { function: MirFunctionId, block: MirBlockId },
    InvalidDropEdge { function: MirFunctionId, block: MirBlockId },
    InvalidYield { function: MirFunctionId, block: MirBlockId },
    InvalidOperation { function: MirFunctionId, message: String },
}

impl fmt::Display for MirValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SchemaVersion(version) => write!(f, "unsupported MIR schema version {version}"),
            Self::ZeroId { kind, .. } => write!(f, "MIR {kind} has a zero stable ID"),
            Self::DuplicateId { kind, id } => write!(f, "duplicate MIR {kind} ID {id}"),
            Self::DuplicateKey { kind, key } => write!(f, "duplicate MIR {kind} key {key}"),
            Self::MissingReference { kind, id } => {
                write!(f, "MIR {kind} references missing ID {id}")
            }
            Self::MissingEntry(id) => write!(f, "MIR entry function {id:?} is missing"),
            Self::MissingBlock { function, block } => write!(f, "function {function:?} targets missing block {block:?}"),
            Self::MissingCoreCall { function, call } => write!(f, "function {function:?} uses missing Core call {call:?}"),
            Self::MissingPreludeCall { function, call } => write!(f, "function {function:?} uses missing Prelude call {call:?}"),
            Self::MissingType { function, ty } => {
                write!(f, "function {function:?} uses missing type {ty:?}")
            }
            Self::MissingTypeIdentity => write!(f, "MIR type instance has no canonical identity"),
            Self::MissingTypeInstance(id) => {
                write!(f, "MIR type definition {id:?} has no canonical type instance")
            }
            Self::MissingValue { function, value } => {
                write!(f, "function {function:?} uses missing value {value:?}")
            }
            Self::MismatchedValueType { function, value } => {
                write!(f, "function {function:?} has mismatched type metadata for value {value:?}")
            }
            Self::MissingPlace { function, place } => {
                write!(f, "function {function:?} uses missing place {place:?}")
            }
            Self::MissingField { function, field } => write!(f, "function {function:?} uses missing field {field:?}"),
            Self::MissingLocal { function, local } => {
                write!(f, "function {function:?} uses missing local {local:?}")
            }
            Self::MissingScope { function, scope } => {
                write!(f, "function {function:?} uses missing scope {scope:?}")
            }
            Self::InvalidParameter { function, index } => {
                write!(f, "function {function:?} uses missing parameter {index}")
            }
            Self::InvalidCapture { function, slot } => {
                write!(f, "function {function:?} uses missing capture slot {slot}")
            }
            Self::InvalidCaptureCount {
                function,
                target,
                expected,
                actual,
            } => write!(
                f,
                "function {function:?} constructs closure {target:?} with {actual} captures; expected {expected}"
            ),
            Self::MismatchedCaptureType {
                function,
                target,
                slot,
            } => write!(
                f,
                "function {function:?} constructs closure {target:?} with a mismatched value at capture slot {slot}"
            ),
            Self::MissingSourceFile {
                function,
                source_file,
            } => write!(
                f,
                "function {function:?} uses missing source file {source_file:?}"
            ),
            Self::UseBeforeDefinition { function, value, span } => write!(f, "function {function:?} uses value {value:?} before its definition at {}..{}", span.start, span.end),
            Self::InvalidTerminator { function, block } => write!(f, "function {function:?} has an invalid terminator in block {block:?}"),
            Self::InvalidDropEdge { function, block } => write!(f, "function {function:?} has an invalid drop edge in block {block:?}"),
            Self::InvalidYield { function, block } => {
                write!(f, "function {function:?} has a yield incompatible with its generator ABI in block {block:?}")
            }
            Self::InvalidOperation { function, message } => {
                write!(f, "function {function:?} has an invalid operation: {message}")
            }
        }
    }
}

impl std::error::Error for MirValidationError {}

impl MirProgram {
    pub fn artifact_identity(
        &self,
        id: MirArtifactId,
    ) -> Result<MirArtifactIdentity, MirIdentityError> {
        crate::MIROptimization::require_canonical_mir_optimization(self)
            .map_err(|error| MirIdentityError::Unoptimized(error.to_string()))?;
        let artifact = self
            .artifacts
            .iter()
            .find(|artifact| artifact.id == id)
            .ok_or(MirIdentityError::MissingArtifact(id))?;
        let program_digest = crate::MIROptimization::mir_program_digest(self);
        let program_digest_hex = digest_hex(&program_digest);
        let mut function_ids = self
            .functions
            .iter()
            .map(|function| function.id.0)
            .collect::<Vec<_>>();
        function_ids.sort_unstable();
        function_ids.dedup();
        let mut core_ids = self
            .core_calls
            .iter()
            .map(|call| call.id.0)
            .collect::<Vec<_>>();
        core_ids.sort_unstable();
        core_ids.dedup();

        let mut source_map = self
            .source_files
            .iter()
            .map(|source| MirSourceMapIdentity {
                id: source.id.0,
                path: source.path.clone(),
                digest: crate::SHA256::sha256_hex(source.source.as_bytes()),
            })
            .collect::<Vec<_>>();
        source_map.sort_by(|left, right| left.id.cmp(&right.id).then(left.path.cmp(&right.path)));

        let mut target_facts = vec![
            ("artifact.id".to_string(), artifact.id.0.to_string()),
            ("package.identity".to_string(), self.package_identity.clone()),
            ("package.project_root".to_string(), self.facts.project_root.clone()),
            ("package.edition".to_string(), self.facts.edition.clone()),
            ("package.active_os".to_string(), self.facts.active_os.clone()),
            (
                "package.inferred_layer".to_string(),
                self.facts.inferred_layer.clone(),
            ),
            ("package.allocator".to_string(), self.facts.allocator.clone()),
            (
                "package.target_dossier".to_string(),
                crate::SHA256::sha256_hex(&self.facts.target_dossier.cache_bytes(
                    self.facts
                        .target_dossier
                        .machine
                        .as_deref()
                        .map(|machine| machine.triple.as_str())
                        .unwrap_or_default(),
                )),
            ),
        ];
        for part in &self.facts.runtime_parts {
            target_facts.push((
                format!("package.runtime_part.{}", part.as_str()),
                "enabled".to_string(),
            ));
        }
        target_facts.extend([
            ("artifact.kind".to_string(), artifact.kind.as_str().to_string()),
            ("artifact.target".to_string(), artifact.target.as_str().to_string()),
            ("artifact.mode".to_string(), artifact.mode.as_str().to_string()),
            ("artifact.name".to_string(), artifact.name.clone()),
            ("artifact.provider".to_string(), artifact.provider_identity.clone()),
            ("artifact.closure".to_string(), artifact.closure_identity.clone()),
            ("artifact.identity".to_string(), artifact.artifact_identity.clone()),
        ]);
        for function in &self.functions {
            let applicability = function.target_applicability;
            target_facts.push((
                format!("function.{}.targets", function.id.0),
                format!(
                    "rust_aot={},cranelift={},interpreter={},web={}",
                    applicability.rust_aot,
                    applicability.cranelift,
                    applicability.interpreter,
                    applicability.web
                ),
            ));
        }
        target_facts.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        let program_identity = MirProgramIdentity {
            semantic_hash: program_digest_hex.clone(),
            optimized_hash: program_digest_hex,
            function_ids,
            core_ids,
            target_facts,
            source_map,
        };
        Ok(MirArtifactIdentity {
            schema_version: MIR_IDENTITY_SCHEMA_VERSION,
            mir_schema_version: self.schema_version,
            program_digest,
            package_identity: self.package_identity.clone(),
            artifact: artifact.id,
            name: artifact.name.clone(),
            kind: artifact.kind,
            target: artifact.target,
            mode: artifact.mode,
            provider_identity: artifact.provider_identity.clone(),
            closure_identity: artifact.closure_identity.clone(),
            artifact_identity: artifact.artifact_identity.clone(),
            program_identity,
        })
    }

    pub fn execution_identity(
        &self,
        artifact: Option<MirArtifactId>,
    ) -> Result<MirExecutionIdentity, MirIdentityError> {
        let artifact = match artifact {
            Some(id) => self.artifact_identity(id)?,
            None if self.artifacts.len() == 1 => self.artifact_identity(self.artifacts[0].id)?,
            None => return Err(MirIdentityError::AmbiguousArtifact),
        };
        Ok(MirExecutionIdentity {
            schema_version: MIR_IDENTITY_SCHEMA_VERSION,
            artifact,
        })
    }

    pub fn frame_identity(
        &self,
        artifact: Option<MirArtifactId>,
        function: MirFunctionId,
        block: Option<MirBlockId>,
        sequence: u64,
    ) -> Result<MirFrameIdentity, MirIdentityError> {
        Ok(MirFrameIdentity {
            schema_version: MIR_IDENTITY_SCHEMA_VERSION,
            execution: self.execution_identity(artifact)?,
            function,
            block,
            sequence,
        })
    }

    pub fn validate(&self) -> Result<(), MirValidationError> {
        if self.schema_version != MIR_SCHEMA_VERSION {
            return Err(MirValidationError::SchemaVersion(self.schema_version));
        }
        macro_rules! id_set {
            ($name:ident, $rows:expr, $field:ident, $kind:literal) => {
                let mut $name = HashSet::new();
                for row in $rows {
                    if row.$field.0 == 0 {
                        return Err(MirValidationError::ZeroId {
                            kind: $kind,
                            span: None,
                        });
                    }
                    if !$name.insert(row.$field) {
                        return Err(MirValidationError::DuplicateId {
                            kind: $kind,
                            id: row.$field.0,
                        });
                    }
                }
            };
        }
        id_set!(module_ids, &self.modules, id, "module");
        id_set!(import_ids, &self.imports, id, "import");
        id_set!(trait_ids, &self.traits, id, "trait");
        id_set!(impl_ids, &self.impls, id, "impl");
        id_set!(constant_ids, &self.constants, id, "constant");
        id_set!(foreign_ids, &self.foreign, id, "foreign");
        id_set!(link_ids, &self.links, id, "link unit");
        id_set!(callback_ids, &self.callbacks, id, "callback");
        id_set!(handle_ids, &self.handles, id, "handle");
        id_set!(job_ids, &self.jobs, id, "job");
        id_set!(test_ids, &self.tests, id, "test");
        id_set!(harness_ids, &self.harnesses, id, "harness");
        id_set!(artifact_ids, &self.artifacts, id, "artifact");
        let mut function_ids = HashSet::new();
        let mut function_keys = HashSet::new();
        let mut function_rows = HashMap::new();
        for function in &self.functions {
            if function.id.0 == 0 {
                return Err(MirValidationError::ZeroId {
                    kind: "function",
                    span: Some(function.span),
                });
            }
            if !function_ids.insert(function.id) {
                return Err(MirValidationError::DuplicateId {
                    kind: "function",
                    id: function.id.0,
                });
            }
            if !function_keys.insert(function.key.clone()) {
                return Err(MirValidationError::DuplicateKey {
                    kind: "function",
                    key: function.key.clone(),
                });
            }
            function_rows.insert(function.id, function);
        }

        let mut type_def_ids = HashSet::new();
        let mut type_keys = HashSet::new();
        for ty in &self.types {
            if ty.id.0 == 0 {
                return Err(MirValidationError::ZeroId {
                    kind: "type definition",
                    span: Some(ty.span),
                });
            }
            if !type_def_ids.insert(ty.id) {
                return Err(MirValidationError::DuplicateId {
                    kind: "type definition",
                    id: ty.id.0,
                });
            }
            if !type_keys.insert(ty.key.clone()) {
                return Err(MirValidationError::DuplicateKey {
                    kind: "type definition",
                    key: ty.key.clone(),
                });
            }
        }
        let mut type_ids = HashSet::new();
        for ty in &self.type_instances {
            let Some(id) = ty.identity else {
                return Err(MirValidationError::MissingTypeIdentity);
            };
            if id.0 == 0 {
                return Err(MirValidationError::ZeroId {
                    kind: "type instance",
                    span: None,
                });
            }
            if !type_ids.insert(id) {
                return Err(MirValidationError::DuplicateId {
                    kind: "type instance",
                    id: id.0,
                });
            }
        }
        if let Some(missing) = type_def_ids.iter().find(|id| !type_ids.contains(id)) {
            return Err(MirValidationError::MissingTypeInstance(*missing));
        }
        for def in &self.types {
            validate_type_definition(def, &type_ids, &function_ids)?;
        }
        for foreign in &self.foreign {
            for param in &foreign.params {
                validate_mir_type(&param.ty, &type_ids)?;
            }
            if let Some(return_type) = &foreign.return_type {
                validate_mir_type(return_type, &type_ids)?;
            }
        }

        let mut core_ids = HashSet::new();
        let mut core_keys = HashSet::new();
        for call in &self.core_calls {
            if call.id.0 == 0 {
                return Err(MirValidationError::ZeroId {
                    kind: "Core call",
                    span: None,
                });
            }
            if !core_ids.insert(call.id) {
                return Err(MirValidationError::DuplicateId {
                    kind: "Core call",
                    id: call.id.0,
                });
            }
            if !core_keys.insert(call.key.clone()) {
                return Err(MirValidationError::DuplicateKey {
                    kind: "Core call",
                    key: call.key.clone(),
                });
            }
        }

        let mut field_ids = HashSet::new();
        for field in &self.fields {
            if field.id.0 == 0 {
                return Err(MirValidationError::ZeroId {
                    kind: "field",
                    span: Some(field.field.span),
                });
            }
            if !field_ids.insert(field.id) {
                return Err(MirValidationError::DuplicateId {
                    kind: "field",
                    id: field.id.0,
                });
            }
            if !type_ids.contains(&field.owner) {
                return Err(MirValidationError::MissingTypeInstance(field.owner));
            }
            validate_mir_type(&field.field.ty, &type_ids)?;
        }

        let mut prelude_ids = HashSet::new();
        for call in &self.prelude_calls {
            validate_call_fallibility(&call.fallibility, &type_ids)?;
            if call.id.0 == 0 {
                return Err(MirValidationError::ZeroId {
                    kind: "Prelude call",
                    span: None,
                });
            }
            if !prelude_ids.insert(call.id) {
                return Err(MirValidationError::DuplicateId {
                    kind: "Prelude call",
                    id: call.id.0,
                });
            }
        }

        let mut source_file_ids = HashSet::new();
        for source_file in &self.source_files {
            if source_file.id.0 == 0 {
                return Err(MirValidationError::ZeroId {
                    kind: "source file",
                    span: None,
                });
            }
            if !source_file_ids.insert(source_file.id) {
                return Err(MirValidationError::DuplicateId {
                    kind: "source file",
                    id: source_file.id.0,
                });
            }
        }

        macro_rules! require_id {
            ($set:expr, $id:expr, $kind:literal) => {
                if !$set.contains(&$id) {
                    return Err(MirValidationError::MissingReference {
                        kind: $kind,
                        id: $id.0,
                    });
                }
            };
        }
        for module in &self.modules {
            require_id!(source_file_ids, module.source_file, "module source file");
            for import in &module.imports {
                require_id!(import_ids, *import, "module import");
            }
            for child in &module.children {
                require_id!(module_ids, *child, "module child");
            }
            for item in &module.item_order {
                match item {
                    MirItemRef::Type(id) => require_id!(type_def_ids, *id, "module type"),
                    MirItemRef::Trait(id) => require_id!(trait_ids, *id, "module trait"),
                    MirItemRef::Function(id) => {
                        require_id!(function_ids, *id, "module function")
                    }
                    MirItemRef::Constant(id) => {
                        require_id!(constant_ids, *id, "module constant")
                    }
                    MirItemRef::Impl(id) => require_id!(impl_ids, *id, "module impl"),
                    MirItemRef::Foreign(id) => require_id!(foreign_ids, *id, "module foreign"),
                    MirItemRef::Import(id) => require_id!(import_ids, *id, "module import"),
                }
            }
        }
        for import in &self.imports {
            require_id!(module_ids, import.module, "import owner module");
            if let MirImportKind::Unqualified { module, items } = &import.kind {
                require_id!(module_ids, *module, "import target module");
                for item in items {
                    match item.item {
                        MirItemRef::Type(id) => require_id!(type_def_ids, id, "import type"),
                        MirItemRef::Trait(id) => require_id!(trait_ids, id, "import trait"),
                        MirItemRef::Function(id) => {
                            require_id!(function_ids, id, "import function")
                        }
                        MirItemRef::Constant(id) => {
                            require_id!(constant_ids, id, "import constant")
                        }
                        MirItemRef::Impl(id) => require_id!(impl_ids, id, "import impl"),
                        MirItemRef::Foreign(id) => require_id!(foreign_ids, id, "import foreign"),
                        MirItemRef::Import(id) => require_id!(import_ids, id, "import re-export"),
                    }
                }
            }
        }
        for function in &self.functions {
            require_id!(module_ids, function.module_id, "function module");
            match &function.form {
                MirFunctionForm::TopLevel => {}
                MirFunctionForm::Method { owner, .. } => {
                    validate_mir_type(owner, &type_ids)?;
                }
                MirFunctionForm::TraitMethod {
                    owner, trait_ref, ..
                } => {
                    validate_mir_type(owner, &type_ids)?;
                    require_id!(trait_ids, trait_ref.id, "trait method trait");
                }
            }
            for generic in &function.generic_params {
                for bound in &generic.bounds {
                    require_id!(trait_ids, bound.id, "function generic bound");
                }
            }
        }
        for ty in &self.types {
            require_id!(module_ids, ty.module, "type module");
            for derive in &ty.derives {
                require_id!(trait_ids, *derive, "derived trait");
            }
            for binding in &ty.cli_bindings {
                require_id!(function_ids, binding.function, "CLI binding function");
            }
            for generic in &ty.generic_params {
                for bound in &generic.bounds {
                    require_id!(trait_ids, bound.id, "type generic bound");
                }
            }
        }
        let mut trait_method_ids = HashSet::new();
        for trait_def in &self.traits {
            require_id!(module_ids, trait_def.module, "trait module");
            for method in &trait_def.methods {
                if method.id.0 == 0 {
                    return Err(MirValidationError::ZeroId {
                        kind: "trait method",
                        span: Some(method.span),
                    });
                }
                if !trait_method_ids.insert(method.id) {
                    return Err(MirValidationError::DuplicateId {
                        kind: "trait method",
                        id: method.id.0,
                    });
                }
                for param in &method.params {
                    validate_mir_type(&param.ty, &type_ids)?;
                }
                validate_mir_type(&method.return_type, &type_ids)?;
                if let Some(ty) = &method.declared_return {
                    validate_mir_type(ty, &type_ids)?;
                }
                validate_failure(&method.failure, &type_ids)?;
                if let Some(default) = method.default {
                    require_id!(function_ids, default, "trait default method");
                }
            }
        }
        for implementation in &self.impls {
            require_id!(module_ids, implementation.module, "impl module");
            validate_mir_type(&implementation.self_type, &type_ids)?;
            if let Some(trait_ref) = &implementation.trait_ref {
                require_id!(trait_ids, trait_ref.id, "impl trait");
            }
            for associated in &implementation.associated_types {
                validate_mir_type(&associated.ty, &type_ids)?;
            }
            for method in &implementation.methods {
                require_id!(function_ids, *method, "impl method");
            }
            if let Some(field) = implementation.delegation {
                require_id!(field_ids, field, "impl delegation field");
            }
            if let Some(ty) = &implementation.operator_rhs {
                validate_mir_type(ty, &type_ids)?;
            }
        }
        for constant in &self.constants {
            require_id!(module_ids, constant.module, "constant module");
            validate_mir_type(&constant.ty, &type_ids)?;
        }
        for foreign in &self.foreign {
            require_id!(module_ids, foreign.module_id, "foreign module");
            for param in &foreign.params {
                validate_mir_type(&param.ty, &type_ids)?;
            }
            if let Some(ty) = &foreign.return_type {
                validate_mir_type(ty, &type_ids)?;
            }
            if let Some(id) = foreign.link {
                require_id!(link_ids, id, "foreign link");
            }
            if let Some(id) = foreign.callback {
                require_id!(callback_ids, id, "foreign callback");
            }
            if let Some(id) = foreign.handle {
                require_id!(handle_ids, id, "foreign handle");
            }
            for function in [foreign.close_function, foreign.undo_function]
                .into_iter()
                .flatten()
            {
                require_id!(function_ids, function, "foreign lifecycle function");
            }
            if let Some(id) = foreign.close_foreign {
                require_id!(foreign_ids, id, "foreign close function");
            }
        }
        for link in &self.links {
            for dependency in &link.link_closure {
                require_id!(link_ids, *dependency, "link closure");
            }
        }
        for callback in &self.callbacks {
            require_id!(function_ids, callback.function, "callback function");
            for param in &callback.params {
                validate_mir_type(&param.ty, &type_ids)?;
            }
            if let Some(ty) = &callback.return_type {
                validate_mir_type(ty, &type_ids)?;
            }
        }
        for handle in &self.handles {
            validate_mir_type(&handle.ty, &type_ids)?;
            for function in [handle.close, handle.undo].into_iter().flatten() {
                require_id!(function_ids, function, "handle lifecycle function");
            }
            if let Some(foreign) = handle.close_foreign {
                require_id!(foreign_ids, foreign, "handle close foreign");
            }
        }
        for job in &self.jobs {
            require_id!(function_ids, job.function, "job function");
            for input in &job.inputs {
                validate_mir_type(&input.ty, &type_ids)?;
            }
        }
        for test in &self.tests {
            require_id!(function_ids, test.function, "test function");
            if let Some(eligibility) = test.eligibility {
                require_id!(function_ids, eligibility, "test eligibility predicate");
            }
            for param in &test.parameters {
                validate_mir_type(&param.ty, &type_ids)?;
            }
        }
        for harness in &self.harnesses {
            for test in &harness.tests {
                require_id!(test_ids, *test, "harness test");
            }
            if let Some(test) = harness.selected_test {
                require_id!(test_ids, test, "selected fuzz test");
            }
            for check in &harness.output_checks {
                if check.id.0 == 0 {
                    return Err(MirValidationError::ZeroId {
                        kind: "output check",
                        span: None,
                    });
                }
                require_id!(function_ids, check.function, "output check function");
            }
            for point in &harness.coverage_points {
                if point.id.0 == 0 {
                    return Err(MirValidationError::ZeroId {
                        kind: "coverage point",
                        span: Some(point.span),
                    });
                }
                require_id!(function_ids, point.function, "coverage function");
            }
        }
        for artifact in &self.artifacts {
            for module in &artifact.modules {
                require_id!(module_ids, *module, "artifact module");
            }
            for link in &artifact.links {
                require_id!(link_ids, *link, "artifact link");
            }
            for job in &artifact.jobs {
                require_id!(job_ids, *job, "artifact job");
            }
            if let Some(harness) = artifact.harness {
                require_id!(harness_ids, harness, "artifact harness");
            }
            if let Some(entry) = &artifact.entry {
                if let Some(function) = entry.function {
                    require_id!(function_ids, function, "artifact entry function");
                }
                if let Some(cli) = &entry.cli {
                    for input in &cli.inputs {
                        validate_mir_type(&input.ty, &type_ids)?;
                    }
                    for command in &cli.commands {
                        require_id!(function_ids, command.function, "CLI command function");
                        for input in &command.inputs {
                            validate_mir_type(&input.ty, &type_ids)?;
                        }
                    }
                }
            }
            for export in &artifact.exports {
                require_id!(function_ids, export.function, "artifact export");
            }
        }
        for function in &self.functions {
            validate_function_types(function, &type_ids)?;
            validate_function(
                function,
                &function_ids,
                &function_rows,
                &core_ids,
                &prelude_ids,
                &callback_ids,
                &foreign_ids,
                &type_ids,
                &field_ids,
                &source_file_ids,
            )?;
        }
        Ok(())
    }
}

fn validate_mir_type(
    ty: &MirType,
    type_ids: &HashSet<MirTypeId>,
) -> Result<(), MirValidationError> {
    let Some(id) = ty.identity else {
        return Err(MirValidationError::MissingTypeIdentity);
    };
    if !type_ids.contains(&id) {
        return Err(MirValidationError::MissingTypeInstance(id));
    }
    Ok(())
}

fn validate_failure(
    failure: &MirFailureCarrier,
    type_ids: &HashSet<MirTypeId>,
) -> Result<(), MirValidationError> {
    match failure {
        MirFailureCarrier::Infallible => Ok(()),
        MirFailureCarrier::Result { success, error } => {
            validate_mir_type(success, type_ids)?;
            validate_mir_type(error, type_ids)
        }
        MirFailureCarrier::Optional { value } | MirFailureCarrier::Diverges { value } => {
            validate_mir_type(value, type_ids)
        }
    }
}

fn validate_call_fallibility(
    fallibility: &MirCallFallibility,
    type_ids: &HashSet<MirTypeId>,
) -> Result<(), MirValidationError> {
    match fallibility {
        MirCallFallibility::Infallible => Ok(()),
        MirCallFallibility::Failure(failure) => validate_failure(failure, type_ids),
    }
}

fn validate_field_types(
    fields: &[MirField],
    type_ids: &HashSet<MirTypeId>,
) -> Result<(), MirValidationError> {
    for field in fields {
        validate_mir_type(&field.ty, type_ids)?;
    }
    Ok(())
}

fn validate_type_definition(
    def: &MirTypeDef,
    type_ids: &HashSet<MirTypeId>,
    function_ids: &HashSet<MirFunctionId>,
) -> Result<(), MirValidationError> {
    match &def.kind {
        MirTypeDefKind::Struct { fields, methods } => {
            validate_field_types(fields, type_ids)?;
            if let Some(missing) = methods.iter().find(|id| !function_ids.contains(id)) {
                return Err(MirValidationError::MissingEntry(*missing));
            }
        }
        MirTypeDefKind::Enum { variants, methods } => {
            for variant in variants {
                match &variant.payload {
                    MirVariantPayload::Unit => {}
                    MirVariantPayload::Single(ty) => validate_mir_type(ty, type_ids)?,
                    MirVariantPayload::Named(fields) => validate_field_types(fields, type_ids)?,
                }
            }
            if let Some(missing) = methods.iter().find(|id| !function_ids.contains(id)) {
                return Err(MirValidationError::MissingEntry(*missing));
            }
        }
        MirTypeDefKind::Distinct { base, .. } => validate_mir_type(base, type_ids)?,
        MirTypeDefKind::Alias { target } => validate_mir_type(target, type_ids)?,
        MirTypeDefKind::UnitFamily { .. } => {}
    }
    Ok(())
}

fn validate_function_types(
    function: &MirFunction,
    type_ids: &HashSet<MirTypeId>,
) -> Result<(), MirValidationError> {
    for param in &function.params {
        validate_mir_type(&param.ty, type_ids)?;
    }
    for capture in &function.capture_params {
        validate_mir_type(&capture.ty, type_ids)?;
    }
    for (slot, capture) in function.capture_params.iter().enumerate() {
        if capture.slot != slot {
            return Err(MirValidationError::DuplicateId {
                kind: "capture slot",
                id: capture.slot as u64,
            });
        }
    }
    if let Some(ty) = &function.declared_return {
        validate_mir_type(ty, type_ids)?;
    }
    validate_mir_type(&function.return_type, type_ids)?;
    if let Some(generator) = &function.generator {
        validate_mir_type(&generator.item, type_ids)?;
    }
    validate_failure(&function.failure, type_ids)?;
    for local in &function.locals {
        validate_mir_type(&local.ty, type_ids)?;
    }
    for (_, ty, _, _) in &function.values {
        validate_mir_type(ty, type_ids)?;
    }
    for place in &function.places {
        validate_mir_type(&place.ty, type_ids)?;
    }
    Ok(())
}

fn validate_function(
    function: &MirFunction,
    function_ids: &HashSet<MirFunctionId>,
    function_rows: &HashMap<MirFunctionId, &MirFunction>,
    core_ids: &HashSet<MirCoreCallId>,
    prelude_ids: &HashSet<MirPreludeCallId>,
    callback_ids: &HashSet<MirCallbackId>,
    foreign_ids: &HashSet<MirForeignId>,
    type_ids: &HashSet<MirTypeId>,
    field_ids: &HashSet<MirFieldId>,
    source_file_ids: &HashSet<MirSourceFileId>,
) -> Result<(), MirValidationError> {
    let mut declared_values = HashMap::new();
    for (value, ty, span, _) in &function.values {
        if value.0 == 0 {
            return Err(MirValidationError::ZeroId {
                kind: "value",
                span: Some(*span),
            });
        }
        if declared_values.insert(*value, ty).is_some() {
            return Err(MirValidationError::DuplicateId {
                kind: "value metadata",
                id: value.0,
            });
        }
    }
    let mut blocks = HashMap::new();
    for (index, block) in function.blocks.iter().enumerate() {
        if block.id.0 == 0 {
            return Err(MirValidationError::ZeroId { kind: "block", span: Some(block.span) });
        }
        if blocks.insert(block.id, index).is_some() {
            return Err(MirValidationError::DuplicateId { kind: "block", id: block.id.0 });
        }
    }
    if !blocks.contains_key(&function.entry) {
        return Err(MirValidationError::MissingBlock { function: function.id, block: function.entry });
    }
    let mut places = HashSet::new();
    let mut place_rows = HashMap::new();
    for place in &function.places {
        if place.id.0 == 0 {
            return Err(MirValidationError::ZeroId { kind: "place", span: Some(place.span) });
        }
        if !places.insert(place.id) {
            return Err(MirValidationError::DuplicateId { kind: "place", id: place.id.0 });
        }
        place_rows.insert(place.id, place);
        for projection in &place.projections {
            match projection {
                MirProjection::Field { field, .. } => {
                    if !field_ids.contains(field) {
                        return Err(MirValidationError::MissingField {
                            function: function.id,
                            field: *field,
                        });
                    }
                }
                MirProjection::Index {
                    call,
                    write_call,
                    location,
                    ..
                } => {
                    for call in std::iter::once(call).chain(write_call.iter()) {
                        if !prelude_ids.contains(call) {
                            return Err(MirValidationError::MissingPreludeCall {
                                function: function.id,
                                call: *call,
                            });
                        }
                    }
                    if place.access == MirAccess::Write && write_call.is_none() {
                        return Err(MirValidationError::InvalidOperation {
                            function: function.id,
                            message: "write index projection has no exact setter row".to_string(),
                        });
                    }
                    if place.access != MirAccess::Write && write_call.is_some() {
                        return Err(MirValidationError::InvalidOperation {
                            function: function.id,
                            message: "non-write index projection carries a setter row".to_string(),
                        });
                    }
                    if !source_file_ids.contains(&location.file) {
                        return Err(MirValidationError::MissingSourceFile {
                            function: function.id,
                            source_file: location.file,
                        });
                    }
                }
                MirProjection::Deref { .. } => {}
            }
        }
    }
    let mut local_ids = HashSet::new();
    for local in &function.locals {
        if local.id.0 == 0 {
            return Err(MirValidationError::ZeroId {
                kind: "local",
                span: Some(local.span),
            });
        }
        if !local_ids.insert(local.id) {
            return Err(MirValidationError::DuplicateId {
                kind: "local",
                id: local.id.0,
            });
        }
        if !places.contains(&local.place) {
            return Err(MirValidationError::MissingPlace {
                function: function.id,
                place: local.place,
            });
        }
    }
    let mut scope_ids = HashSet::new();
    for scope in &function.scopes {
        if scope.id.0 == 0 {
            return Err(MirValidationError::ZeroId {
                kind: "scope",
                span: Some(scope.span),
            });
        }
        if !scope_ids.insert(scope.id) {
            return Err(MirValidationError::DuplicateId {
                kind: "scope",
                id: scope.id.0,
            });
        }
    }
    let mut values: HashMap<MirValueId, (MirBlockId, usize)> = HashMap::new();
    for (block_index, block) in function.blocks.iter().enumerate() {
        for (instruction_index, instruction) in block.instructions.iter().enumerate() {
            if instruction.id.0 == 0 {
                return Err(MirValidationError::ZeroId { kind: "operation", span: Some(instruction.span) });
            }
            if let Some(result) = instruction.result {
                if result.0 == 0 {
                    return Err(MirValidationError::ZeroId { kind: "value", span: Some(instruction.span) });
                }
                let Some(declared_type) = declared_values.get(&result) else {
                    return Err(MirValidationError::MissingValue {
                        function: function.id,
                        value: result,
                    });
                };
                if instruction.ty.as_ref() != Some(*declared_type) {
                    return Err(MirValidationError::MismatchedValueType {
                        function: function.id,
                        value: result,
                    });
                }
                if values.insert(result, (block.id, instruction_index)).is_some() {
                    return Err(MirValidationError::DuplicateId { kind: "value", id: result.0 });
                }
            }
            match &instruction.operation {
                MirOperation::Parameter { index, .. } if *index >= function.params.len() => {
                    return Err(MirValidationError::InvalidParameter {
                        function: function.id,
                        index: *index,
                    });
                }
                MirOperation::Capture { slot } if *slot >= function.capture_params.len() => {
                    return Err(MirValidationError::InvalidCapture {
                        function: function.id,
                        slot: *slot,
                    });
                }
                _ => {}
            }
            for place in operation_places(&instruction.operation) {
                if !places.contains(&place) {
                    return Err(MirValidationError::MissingPlace { function: function.id, place });
                }
            }
            for local in operation_locals(&instruction.operation) {
                if !local_ids.contains(&local) {
                    return Err(MirValidationError::MissingLocal {
                        function: function.id,
                        local,
                    });
                }
            }
            for scope in operation_scopes(&instruction.operation) {
                if !scope_ids.contains(&scope) {
                    return Err(MirValidationError::MissingScope {
                        function: function.id,
                        scope,
                    });
                }
            }
            if let Some(ty) = &instruction.ty {
                validate_mir_type(ty, type_ids)?;
            }
            for call in operation_core_calls(&instruction.operation) {
                if !core_ids.contains(&call) {
                    return Err(MirValidationError::MissingCoreCall { function: function.id, call });
                }
            }
            for callee in operation_function_calls(&instruction.operation) {
                if !function_ids.contains(&callee) {
                    return Err(MirValidationError::MissingEntry(callee));
                }
            }
            if let MirOperation::Closure {
                function: target,
                captures,
                ..
            } = &instruction.operation
            {
                let target_row = function_rows
                    .get(target)
                    .expect("function existence checked above");
                if captures.len() != target_row.capture_params.len() {
                    return Err(MirValidationError::InvalidCaptureCount {
                        function: function.id,
                        target: *target,
                        expected: target_row.capture_params.len(),
                        actual: captures.len(),
                    });
                }
                for (slot, (operand, capture)) in captures
                    .iter()
                    .zip(&target_row.capture_params)
                    .enumerate()
                {
                    let matches = match operand {
                        MirCaptureOperand::Value(value) => {
                            matches!(
                                capture.ownership.mode,
                                MirOwnershipMode::Owned | MirOwnershipMode::Move
                                    | MirOwnershipMode::Copy | MirOwnershipMode::Shared
                            )
                                && declared_values.get(value).copied() == Some(&capture.ty)
                        }
                        MirCaptureOperand::Place(place) => place_rows
                            .get(place)
                            .is_some_and(|row| {
                                capture.access != MirAccess::Move
                                    && matches!(
                                        capture.ownership.mode,
                                        MirOwnershipMode::ReadBorrow | MirOwnershipMode::WriteBorrow
                                    )
                                    && row.ty == capture.ty
                                    && (capture.access != MirAccess::Write
                                        || row.access == MirAccess::Write)
                            }),
                    };
                    if !matches {
                        return Err(MirValidationError::MismatchedCaptureType {
                            function: function.id,
                            target: *target,
                            slot,
                        });
                    }
                }
            }
            for call in operation_prelude_calls(&instruction.operation) {
                if !prelude_ids.contains(&call) {
                    return Err(MirValidationError::MissingPreludeCall {
                        function: function.id,
                        call,
                    });
                }
            }
            if let MirOperation::Call {
                callee: MirCallee::Foreign(foreign),
                ..
            } = &instruction.operation
            {
                if !foreign_ids.contains(foreign) {
                    return Err(MirValidationError::MissingReference {
                        kind: "foreign call",
                        id: foreign.0,
                    });
                }
            }
            if let MirOperation::Semantic(MirSemanticOp::CCallback { callback, .. }) =
                &instruction.operation
            {
                if !callback_ids.contains(callback) {
                    return Err(MirValidationError::MissingReference {
                        kind: "semantic callback",
                        id: callback.0,
                    });
                }
            }
            if let MirOperation::Semantic(MirSemanticOp::GcEdit { site, .. }) =
                &instruction.operation
            {
                if site.0 == 0 {
                    return Err(MirValidationError::ZeroId {
                        kind: "GC edit site",
                        span: Some(instruction.span),
                    });
                }
            }
            for ty in operation_types(&instruction.operation) {
                if !type_ids.contains(&ty) {
                    return Err(MirValidationError::MissingType {
                        function: function.id,
                        ty,
                    });
                }
            }
            for field in operation_fields(&instruction.operation) {
                if !field_ids.contains(&field) {
                    return Err(MirValidationError::MissingField {
                        function: function.id,
                        field,
                    });
                }
            }
            for source_file in operation_source_files(&instruction.operation) {
                if !source_file_ids.contains(&source_file) {
                    return Err(MirValidationError::MissingSourceFile {
                        function: function.id,
                        source_file,
                    });
                }
            }
            let _ = block_index;
        }
    }
    if let Some(value) = declared_values.keys().find(|value| !values.contains_key(value)) {
        return Err(MirValidationError::MissingValue {
            function: function.id,
            value: *value,
        });
    }
    let all_blocks: BTreeSet<MirBlockId> = blocks.keys().copied().collect();
    let mut dominators: HashMap<MirBlockId, BTreeSet<MirBlockId>> = blocks
        .keys()
        .copied()
        .map(|id| (id, all_blocks.clone()))
        .collect();
    dominators.insert(function.entry, [function.entry].into_iter().collect());
    let mut changed = true;
    while changed {
        changed = false;
        for block in &function.blocks {
            if block.id == function.entry {
                continue;
            }
            let predecessors: Vec<MirBlockId> = function
                .blocks
                .iter()
                .filter(|candidate| candidate.terminator.targets().contains(&block.id))
                .map(|candidate| candidate.id)
                .collect();
            if predecessors.is_empty() {
                continue;
            }
            let mut next = all_blocks.clone();
            for predecessor in predecessors {
                if let Some(dom) = dominators.get(&predecessor) {
                    next = next.intersection(dom).copied().collect();
                }
            }
            next.insert(block.id);
            if dominators.get(&block.id) != Some(&next) {
                dominators.insert(block.id, next);
                changed = true;
            }
        }
    }
    for block in &function.blocks {
        for instruction in &block.instructions {
            if let MirOperation::Phi { incoming } = &instruction.operation {
                for (predecessor, value) in incoming {
                    if !function
                        .blocks
                        .iter()
                        .any(|candidate| candidate.id == *predecessor && candidate.terminator.targets().contains(&block.id))
                    {
                        return Err(MirValidationError::InvalidTerminator { function: function.id, block: *predecessor });
                    }
                    ensure_use(function, &values, &dominators, &blocks, *predecessor, function.blocks.iter().find(|candidate| candidate.id == *predecessor).map_or(0, |candidate| candidate.instructions.len()), *value, instruction.span)?;
                }
            } else {
                for value in instruction.operation.value_uses() {
                    ensure_use(function, &values, &dominators, &blocks, block.id, instruction_index(block, instruction), value, instruction.span)?;
                }
            }
            for place_id in operation_places(&instruction.operation) {
                if let Some(place) = place_rows.get(&place_id) {
                    for value in place_value_uses(place) {
                        ensure_use(
                            function,
                            &values,
                            &dominators,
                            &blocks,
                            block.id,
                            instruction_index(block, instruction),
                            value,
                            instruction.span,
                        )?;
                    }
                }
            }
        }
        for value in block.terminator.value_uses() {
            ensure_use(function, &values, &dominators, &blocks, block.id, block.instructions.len(), value, block.span)?;
        }
        if let MirTerminator::Yield { value, .. } = &block.terminator {
            let Some(generator) = &function.generator else {
                return Err(MirValidationError::InvalidYield {
                    function: function.id,
                    block: block.id,
                });
            };
            if declared_values.get(value).copied() != Some(&generator.item) {
                return Err(MirValidationError::InvalidYield {
                    function: function.id,
                    block: block.id,
                });
            }
        }
        for target in block.terminator.targets() {
            if !blocks.contains_key(&target) {
                return Err(MirValidationError::MissingBlock { function: function.id, block: target });
            }
        }
    }
    for drop in &function.drops {
        if !places.contains(&drop.place) {
            return Err(MirValidationError::MissingPlace { function: function.id, place: drop.place });
        }
        if let MirDropEdge::Failure(target) | MirDropEdge::Unwind(target) = drop.edge {
            if !blocks.contains_key(&target) {
                return Err(MirValidationError::InvalidDropEdge { function: function.id, block: target });
            }
        }
    }
    Ok(())
}

fn instruction_index(block: &MirBasicBlock, instruction: &MirInstruction) -> usize {
    block
        .instructions
        .iter()
        .position(|candidate| candidate.id == instruction.id)
        .unwrap_or(block.instructions.len())
}

fn ensure_use(
    function: &MirFunction,
    values: &HashMap<MirValueId, (MirBlockId, usize)>,
    dominators: &HashMap<MirBlockId, BTreeSet<MirBlockId>>,
    blocks: &HashMap<MirBlockId, usize>,
    use_block: MirBlockId,
    use_index: usize,
    value: MirValueId,
    span: Span,
) -> Result<(), MirValidationError> {
    let Some((definition_block, definition_index)) = values.get(&value).copied() else {
        return Err(MirValidationError::MissingValue { function: function.id, value });
    };
    let valid = if definition_block == use_block {
        definition_index < use_index
    } else {
        dominators
            .get(&use_block)
            .is_some_and(|dominated| dominated.contains(&definition_block))
    };
    if !valid || !blocks.contains_key(&definition_block) {
        return Err(MirValidationError::UseBeforeDefinition { function: function.id, value, span });
    }
    Ok(())
}


fn place_value_uses(place: &MirPlace) -> impl Iterator<Item = MirValueId> + '_ {
    let base = match place.base {
        MirPlaceBase::Parameter(value)
        | MirPlaceBase::Capture(value)
        | MirPlaceBase::Temporary(value) => Some(value),
        MirPlaceBase::Local(_) | MirPlaceBase::Static(_) => None,
    };
    base.into_iter().chain(place.projections.iter().filter_map(|projection| {
        match projection {
            MirProjection::Index { index, .. } => Some(*index),
            MirProjection::Field { .. } | MirProjection::Deref { .. } => None,
        }
    }))
}

fn operation_places(operation: &MirOperation) -> Vec<MirPlaceId> {
    match operation {
        MirOperation::ReadPlace(place)
        | MirOperation::MovePlace { place }
        | MirOperation::InitializeUninit { place }
        | MirOperation::RawAddressOf { place }
        | MirOperation::AddressOf { place, .. }
        | MirOperation::WritePlace { place, .. } => vec![*place],
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
    }
}

fn operation_locals(operation: &MirOperation) -> Vec<MirLocalId> {
    match operation {
        MirOperation::Semantic(MirSemanticOp::DataEntriesToMap { local, .. }) => vec![*local],
        MirOperation::Semantic(MirSemanticOp::RequireStop { context, .. }) => {
            context.locals.iter().map(|(_, local)| *local).collect()
        }
        _ => Vec::new(),
    }
}

fn operation_scopes(operation: &MirOperation) -> Vec<MirScopeId> {
    match operation {
        MirOperation::ScopeEnter { scope, .. } | MirOperation::ScopeExit { scope } => vec![*scope],
        _ => Vec::new(),
    }
}

fn operation_function_calls(operation: &MirOperation) -> Vec<MirFunctionId> {
    match operation {
        MirOperation::Call {
            callee:
                MirCallee::User(function)
                | MirCallee::Associated { function, .. }
                | MirCallee::Method { function, .. },
            ..
        }
        | MirOperation::Closure { function, .. } => vec![*function],
        _ => Vec::new(),
    }
}

fn operation_core_calls(operation: &MirOperation) -> Vec<MirCoreCallId> {
    match operation {
        MirOperation::CoreCall { call, .. }
        | MirOperation::Call {
            callee: MirCallee::Core(call),
            ..
        } => vec![*call],
        _ => Vec::new(),
    }
}
fn operation_prelude_calls(operation: &MirOperation) -> Vec<MirPreludeCallId> {
    match operation {
        MirOperation::Call {
            callee: MirCallee::Prelude(route),
            ..
        }
        |
        MirOperation::CoreCall { route, .. }
        | MirOperation::Todo { call: route, .. }
        | MirOperation::Index { call: route, .. }
        | MirOperation::Slice { call: route, .. }
        | MirOperation::LoopRangeInit { call: route, .. }
        | MirOperation::LoopRangeHasNext { call: route, .. }
        | MirOperation::LoopRangeValue { call: route, .. }
        | MirOperation::LoopRangeAdvance { call: route, .. }
        | MirOperation::LoopIterInit { call: route, .. }
        | MirOperation::LoopIterHasNext { call: route, .. }
        | MirOperation::LoopIterValue { call: route, .. }
        | MirOperation::LoopIterAdvance { call: route, .. }
        => vec![*route],
        MirOperation::Convert {
            conversion: MirConversion::Prelude { call, .. },
            ..
        } => vec![*call],
        MirOperation::Binary {
            dispatch: MirBinaryDispatch::Prelude { call, .. },
            ..
        } => vec![*call],
        MirOperation::Semantic(operation) => operation.prelude_calls(),
        _ => Vec::new(),
    }
}

fn operation_types(operation: &MirOperation) -> Vec<MirTypeId> {
    match operation {
        MirOperation::Struct { type_id, .. }
        | MirOperation::Enum { type_id, .. }
        | MirOperation::Tuple { type_id, .. } => vec![*type_id],
        MirOperation::EnumIs { owner, .. } | MirOperation::EnumPayload { owner, .. } => {
            vec![*owner]
        }
        MirOperation::Convert {
            target, conversion, ..
        } => target
            .identity
            .into_iter()
            .chain(
                match conversion {
                    MirConversion::Transparent | MirConversion::NumericCast | MirConversion::SendFn => [None, None],
                    MirConversion::Prelude { fallibility, .. } => {
                        fallibility_type_uses(fallibility)
                    }
                }
                .into_iter()
                .flatten(),
            )
            .collect(),
        MirOperation::Call {
            callee,
            args,
            type_args,
            ..
        } => call_arg_type_uses(args)
            .chain(type_args.iter().filter_map(|ty| ty.identity))
            .chain(match callee {
                MirCallee::Associated { owner, .. }
                | MirCallee::Method { owner, .. } => owner.identity,
                MirCallee::TraitMethod { receiver, .. } => receiver.identity,
                _ => None,
            })
            .collect(),
        MirOperation::IndirectCall {
            args, type_args, ..
        } => call_arg_type_uses(args)
            .chain(type_args.iter().filter_map(|ty| ty.identity))
            .collect(),
        MirOperation::CoreCall {
            args,
            type_args,
            fallibility,
            data_plan,
            ..
        } => call_arg_type_uses(args)
            .chain(type_args.iter().filter_map(|ty| ty.identity))
            .chain(fallibility_type_uses(fallibility).into_iter().flatten())
            .chain(data_plan.iter().flat_map(data_plan_type_uses))
            .collect(),
        MirOperation::PtrFromAddr { element, .. } => element.identity.into_iter().collect(),
        MirOperation::Todo { expected_type, .. } => expected_type
            .as_ref()
            .and_then(|ty| ty.identity)
            .into_iter()
            .collect(),
        MirOperation::Semantic(operation) => operation.type_uses(),
        _ => Vec::new(),
    }
}

fn data_plan_type_uses(plan: &MirDataPlan) -> Vec<MirTypeId> {
    plan.logical
        .iter()
        .flat_map(|node| {
            node.row_type
                .identity
                .into_iter()
                .chain(node.columns.iter().filter_map(|column| column.ty.identity))
                .chain(node.callable.iter().flat_map(|callable| {
                    [callable.parameter.identity, callable.result.identity]
                        .into_iter()
                        .flatten()
                }))
        })
        .collect()
}

fn operation_fields(operation: &MirOperation) -> Vec<MirFieldId> {
    match operation {
        MirOperation::ProjectMembers { members, .. } => members.clone(),
        MirOperation::Field { field, .. } => vec![*field],
        MirOperation::Struct { fields, .. } | MirOperation::Tuple { fields, .. } => {
            fields.iter().map(|(field, _)| *field).collect()
        }
        MirOperation::Semantic(operation) => operation.field_uses(),
        _ => Vec::new(),
    }
}

fn operation_source_files(operation: &MirOperation) -> Vec<MirSourceFileId> {
    match operation {
        MirOperation::Todo { location, .. }
        | MirOperation::Index { location, .. }
        | MirOperation::Slice { location, .. } => vec![location.file],
        MirOperation::Convert {
            conversion: MirConversion::Prelude { location, .. },
            ..
        } => vec![location.file],
        MirOperation::Binary {
            dispatch:
                MirBinaryDispatch::Prelude {
                    location: Some(location),
                    ..
                },
            ..
        } => vec![location.file],
        MirOperation::Semantic(operation) => operation.source_file_uses(),
        _ => Vec::new(),
    }
}

pub use crate::MIROptimization::{
    mir_program_bytes, mir_program_digest, optimize_mir_program,
    require_canonical_mir_optimization, verify_mir_legality, MirLegalityError,
    MirOptimizationError, MirOptimizationPolicy, MIR_OPTIMIZATION_PASS_ORDER,
};
