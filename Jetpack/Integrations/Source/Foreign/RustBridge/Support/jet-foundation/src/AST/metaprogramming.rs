//! Ratified shared-preparation and reflection contract types.
//!
//! These records define public shapes only. Parsing, checking, evaluation, and
//! runtime retention are owned by the compiler delivery cards.

/// D-PREP-SURFACE2=A: the total phase reported by `$phase` at its evaluation site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Phase {
    Preparation,
    Build,
    Runtime,
}

impl Phase {
    pub const ALL: [Self; 3] = [Self::Preparation, Self::Build, Self::Runtime];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Preparation => "Preparation",
            Self::Build => "Build",
            Self::Runtime => "Runtime",
        }
    }
}

/// D-META-REFLECT2=A: compiler stage at which a checked fact becomes available.
/// This is distinct from the language execution phase returned by `$phase`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CompilerStage {
    Parsed,
    Resolved,
    Typed,
    Prepared,
    Emitted,
}

impl CompilerStage {
    pub const ALL: [Self; 5] = [
        Self::Parsed,
        Self::Resolved,
        Self::Typed,
        Self::Prepared,
        Self::Emitted,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Parsed => "Parsed",
            Self::Resolved => "Resolved",
            Self::Typed => "Typed",
            Self::Prepared => "Prepared",
            Self::Emitted => "Emitted",
        }
    }
}

/// Compiler-issued identities cannot be created from source names or casts.
macro_rules! opaque_metadata_identity {
    ($($name:ident),+ $(,)?) => {
        $(
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
            pub struct $name {
                _identity: u64,
            }
        )+
    };
}

opaque_metadata_identity!(
    SnapshotId,
    TypeId,
    DeclarationHandle,
    ScopeId,
    NodeId,
    ValueHandle,
);

/// Package identity in one checked source snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageId {
    pub name: String,
    pub version: String,
    pub digest: String,
}

/// Exact source identity used by metadata facts, rather than a mutable path alone.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceId {
    pub package: PackageId,
    pub path: Option<String>,
    pub digest: String,
}

/// D-META-REFLECT2=A: byte range in an exact source snapshot; end is exclusive.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub source: SourceId,
    pub start: i64,
    pub end: i64,
}

/// Existing typed compiler diagnostic reused by `MetadataFailure::Invalid`.
pub type CompilerDiagnostic = crate::Diagnostics::Diagnostic;

/// Closed compiler fact availability stage for metadata failures.
#[derive(Debug, Clone)]
pub enum MetadataFailure {
    Unavailable {
        required_stage: CompilerStage,
        reason: String,
    },
    Unsupported {
        capability: String,
    },
    Stale {
        requested: SnapshotId,
        current: SnapshotId,
    },
    Invalid {
        diagnostics: Vec<CompilerDiagnostic>,
    },
    Denied {
        scope: ScopeId,
        reason: String,
    },
    Truncated {
        limit: i64,
        observed: i64,
    },
}

impl MetadataFailure {
    pub const fn kind_name(&self) -> &'static str {
        match self {
            Self::Unavailable { .. } => "Unavailable",
            Self::Unsupported { .. } => "Unsupported",
            Self::Stale { .. } => "Stale",
            Self::Invalid { .. } => "Invalid",
            Self::Denied { .. } => "Denied",
            Self::Truncated { .. } => "Truncated",
        }
    }
}

/// D-META-REFLECT2=A: a checked metadata query's explicit typed failure.
#[derive(Debug, Clone)]
pub struct MetadataError {
    pub kind: MetadataFailure,
    pub query: String,
    pub source: Option<SourceSpan>,
}
/// Static contract descriptors are schema only. They do not imply that a
/// parser, checker, reflection producer, or runtime retention path exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataIdentity {
    Snapshot,
    Type,
    Declaration,
    Scope,
    Node,
    Value,
}

impl MetadataIdentity {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Snapshot => "SnapshotId",
            Self::Type => "TypeId",
            Self::Declaration => "DeclarationHandle",
            Self::Scope => "ScopeId",
            Self::Node => "NodeId",
            Self::Value => "ValueHandle",
        }
    }
}

/// Public metadata records and their canonical source records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataRecord {
    Phase,
    CompilerStage,
    MetadataError,
    MetadataFailure,
    PackageId,
    SourceId,
    SourceSpan,
    ParameterInfo,
    DefaultInfo,
    CallableSignature,
    FunctionInfo,
    MethodInfo,
    ClosureInfo,
    CaptureInfo,
    EffectInfo,
    FailureInfo,
    GenericBinding,
    GenericArgument,
    MeasureInfo,
    TypeForm,
    TypeInfo,
    TupleTypeField,
    FieldInfo,
    CaseInfo,
    TraitInfo,
    LayoutInfo,
    ProgramInfo,
    ScopeInfo,
    CheckedNode,
    CheckedNodeKind,
    ObligationFact,
    SymbolDef,
    ReferenceFact,
    CallFact,
    StateGraph,
    FactDefinition,
    DerivationFact,
    SourceInfo,
    CommentInfo,
    CommentKind,
    ExpansionInfo,
    AttributeInfo,
    AttributeOrigin,
    PackageInfo,
    DependencyInfo,
    ImportInfo,
    SettingInfo,
    ProfileInfo,
    TargetInfo,
    ContributionInfo,
    ContributionKind,
    ValueInfo,
    ValueFieldInfo,
    TypeCatalog,
    RetentionScope,
    MetadataFamily,
    Visibility,
    ParamZone,
    AccessConvention,
    CallingConvention,
    FailureSource,
    CaptureMode,
    PreparedValue,
    CodeRef,
    DimensionInfo,
    TagMarker,
    PolicyFact,
    ViewProvenanceFact,
    CompilerDiagnostic,
    AbiInfo,
    TargetId,
}

impl MetadataRecord {
    /// Canonical public record spelling.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Phase => "Phase",
            Self::CompilerStage => "CompilerStage",
            Self::MetadataError => "MetadataError",
            Self::MetadataFailure => "MetadataFailure",
            Self::PackageId => "PackageId",
            Self::SourceId => "SourceId",
            Self::SourceSpan => "SourceSpan",
            Self::ParameterInfo => "ParameterInfo",
            Self::DefaultInfo => "DefaultInfo",
            Self::CallableSignature => "CallableSignature",
            Self::FunctionInfo => "FunctionInfo",
            Self::MethodInfo => "MethodInfo",
            Self::ClosureInfo => "ClosureInfo",
            Self::CaptureInfo => "CaptureInfo",
            Self::EffectInfo => "EffectInfo",
            Self::FailureInfo => "FailureInfo",
            Self::GenericBinding => "GenericBinding",
            Self::GenericArgument => "GenericArgument",
            Self::MeasureInfo => "MeasureInfo",
            Self::TypeForm => "TypeForm",
            Self::TypeInfo => "TypeInfo",
            Self::TupleTypeField => "TupleTypeField",
            Self::FieldInfo => "FieldInfo",
            Self::CaseInfo => "CaseInfo",
            Self::TraitInfo => "TraitInfo",
            Self::LayoutInfo => "LayoutInfo",
            Self::ProgramInfo => "ProgramInfo",
            Self::ScopeInfo => "ScopeInfo",
            Self::CheckedNode => "CheckedNode",
            Self::CheckedNodeKind => "CheckedNodeKind",
            Self::ObligationFact => "ObligationFact",
            Self::SymbolDef => "SymbolDef",
            Self::ReferenceFact => "ReferenceFact",
            Self::CallFact => "CallFact",
            Self::StateGraph => "StateGraph",
            Self::FactDefinition => "FactDefinition",
            Self::DerivationFact => "DerivationFact",
            Self::SourceInfo => "SourceInfo",
            Self::CommentInfo => "CommentInfo",
            Self::CommentKind => "CommentKind",
            Self::ExpansionInfo => "ExpansionInfo",
            Self::AttributeInfo => "AttributeInfo",
            Self::AttributeOrigin => "AttributeOrigin",
            Self::PackageInfo => "PackageInfo",
            Self::DependencyInfo => "DependencyInfo",
            Self::ImportInfo => "ImportInfo",
            Self::SettingInfo => "SettingInfo",
            Self::ProfileInfo => "ProfileInfo",
            Self::TargetInfo => "TargetInfo",
            Self::ContributionInfo => "ContributionInfo",
            Self::ContributionKind => "ContributionKind",
            Self::ValueInfo => "ValueInfo",
            Self::ValueFieldInfo => "ValueFieldInfo",
            Self::TypeCatalog => "TypeCatalog",
            Self::RetentionScope => "RetentionScope",
            Self::MetadataFamily => "MetadataFamily",
            Self::Visibility => "Visibility",
            Self::ParamZone => "ParamZone",
            Self::AccessConvention => "AccessConvention",
            Self::CallingConvention => "CallingConvention",
            Self::FailureSource => "FailureSource",
            Self::CaptureMode => "CaptureMode",
            Self::PreparedValue => "PreparedValue",
            Self::CodeRef => "CodeRef",
            Self::DimensionInfo => "DimensionInfo",
            Self::TagMarker => "TagMarker",
            Self::PolicyFact => "PolicyFact",
            Self::ViewProvenanceFact => "ViewProvenanceFact",
            Self::CompilerDiagnostic => "CompilerDiagnostic",
            Self::AbiInfo => "AbiInfo",
            Self::TargetId => "TargetId",
        }
    }
}

/// A field's base type. Collection, optionality, and checked-result shape are
/// encoded separately by `MetadataFieldCardinality`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataValueType {
    Bool,
    Int,
    String,
    Identity(MetadataIdentity),
    Record(MetadataRecord),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataFieldCardinality {
    One,
    Optional,
    List,
    FallibleOne,
    FallibleList,
}

/// Availability is a producer obligation, never an empty/default value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataAvailability {
    EvaluationSite,
    CheckedSnapshot,
    AtStage(CompilerStage),
    TargetDependent,
    RuntimeRetained,
    ProducerReported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataProducerStatus {
    ContractOnly,
    NotImplemented,
    ExistingCallableProducerNeedsExtension,
}

/// Existing canonical type/record owner. Entries are references, not new
/// copies of the source model or claims that a public metadata producer exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataCanonicalSource {
    AstPhase,
    AstCompilerStage,
    AstMetadataFailure,
    AstMetadataError,
    AstPackageId,
    AstSourceId,
    AstSourceSpan,
    AstType,
    AstMeasure,
    AstDimension,
    AstTagMarker,
    AstParamZone,
    AstAccessConvention,
    NameVisibility,
    AstCallablePolicy,
    ComptimeValue,
    Diagnostic,
    SemanticCallableSignatureFact,
    SemanticCallableParameterFact,
    SemanticViewProvenanceFact,
    SemanticSymbolDef,
    SemanticSymbolRef,
    SemanticCallEdge,
    SemanticStateGraphFact,
    SemanticCompilerFact,
    DerivationRecord,
    PackageDependencyRecord,
    PackageImportRecord,
    PackageProfileRecord,
    PackageTargetRecord,
    CodeRef,
    CheckedNodeKind,
    LocalObligationFact,
    CallingConvention,
    FailureSource,
    CaptureMode,
    AbiInfo,
    TargetId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataCanonicalStatus {
    ExistingTypeOrRecord,
    ExistingStringProjectionNeedsTypedExtension,
    ContractTypeHasNoCurrentDefinition,
    ProducerNotImplemented,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetadataFieldSchema {
    pub name: &'static str,
    pub value_type: MetadataValueType,
    pub cardinality: MetadataFieldCardinality,
    pub error_type: Option<MetadataRecord>,
    pub availability: MetadataAvailability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetadataVariantSchema {
    pub name: MetadataVariant,
    pub fields: &'static [MetadataFieldSchema],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataRecordShape {
    Fields(&'static [MetadataFieldSchema]),
    FieldsFrom {
        source: MetadataCanonicalSource,
        status: MetadataCanonicalStatus,
        fields: &'static [MetadataFieldSchema],
    },
    Extends {
        base: MetadataRecord,
        fields: &'static [MetadataFieldSchema],
    },
    Variants(&'static [MetadataVariantSchema]),
    VariantsFrom {
        source: MetadataCanonicalSource,
        status: MetadataCanonicalStatus,
        variants: &'static [MetadataVariantSchema],
    },
    Canonical {
        source: MetadataCanonicalSource,
        status: MetadataCanonicalStatus,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetadataRecordSchema {
    pub record: MetadataRecord,
    pub shape: MetadataRecordShape,
    pub producer: MetadataProducerStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataVariant {
    Preparation,
    Build,
    Runtime,
    Parsed,
    Resolved,
    Typed,
    Prepared,
    Emitted,
    Unavailable,
    Unsupported,
    Stale,
    Invalid,
    Denied,
    Truncated,
    TypeArgument,
    ValueArgument,
    MeasureArgument,
    FormInt,
    FormFloat,
    FormBool,
    FormString,
    FormChar,
    FormList,
    FormMap,
    FormShared,
    FormOption,
    FormResult,
    FormFunction,
    FormNamed,
    FormApply,
    FormTraitObject,
    FormTuple,
    FormFixedList,
    FormIntN,
    FormInlineRange,
    FormFloat32,
    FormTagged,
    FormUnion,
    FormQuantity,
    FormMeasure,
    RequestedRoots,
    WholeProgram,
    Types,
    Callables,
    Program,
    Source,
    Packages,
    LocalState,
    All,
    Line,
    Block,
    Documentation,
    Written,
    Expanded,
    Declaration,
    Profile,
    CommandLine,
}

impl MetadataVariant {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Preparation => "Preparation",
            Self::Build => "Build",
            Self::Runtime => "Runtime",
            Self::Parsed => "Parsed",
            Self::Resolved => "Resolved",
            Self::Typed => "Typed",
            Self::Prepared => "Prepared",
            Self::Emitted => "Emitted",
            Self::Unavailable => "Unavailable",
            Self::Unsupported => "Unsupported",
            Self::Stale => "Stale",
            Self::Invalid => "Invalid",
            Self::Denied => "Denied",
            Self::Truncated => "Truncated",
            Self::TypeArgument => "Type",
            Self::ValueArgument => "Value",
            Self::MeasureArgument => "Measure",
            Self::FormInt => "Int",
            Self::FormFloat => "Float",
            Self::FormBool => "Bool",
            Self::FormString => "String",
            Self::FormChar => "Char",
            Self::FormList => "List",
            Self::FormMap => "Map",
            Self::FormShared => "Shared",
            Self::FormOption => "Option",
            Self::FormResult => "Result",
            Self::FormFunction => "Function",
            Self::FormNamed => "Named",
            Self::FormApply => "Apply",
            Self::FormTraitObject => "TraitObject",
            Self::FormTuple => "Tuple",
            Self::FormFixedList => "FixedList",
            Self::FormIntN => "IntN",
            Self::FormInlineRange => "InlineRange",
            Self::FormFloat32 => "Float32",
            Self::FormTagged => "Tagged",
            Self::FormUnion => "Union",
            Self::FormQuantity => "Quantity",
            Self::FormMeasure => "Measure",
            Self::RequestedRoots => "RequestedRoots",
            Self::WholeProgram => "WholeProgram",
            Self::Types => "Types",
            Self::Callables => "Callables",
            Self::Program => "Program",
            Self::Source => "Source",
            Self::Packages => "Packages",
            Self::LocalState => "LocalState",
            Self::All => "All",
            Self::Line => "Line",
            Self::Block => "Block",
            Self::Documentation => "Documentation",
            Self::Written => "Written",
            Self::Expanded => "Expanded",
            Self::Declaration => "Declaration",
            Self::Profile => "Profile",
            Self::CommandLine => "CommandLine",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataRootInputType {
    TypeExpression,
    CallableSelection,
    MethodName,
    MethodHandle,
    AuthorizedSnapshotHandle,
    RuntimeValue,
    Int,
}

impl MetadataRootInputType {
    pub const fn identity(self) -> Option<MetadataIdentity> {
        match self {
            Self::MethodHandle => Some(MetadataIdentity::Declaration),
            Self::AuthorizedSnapshotHandle => Some(MetadataIdentity::Snapshot),
            Self::TypeExpression
            | Self::CallableSelection
            | Self::MethodName
            | Self::RuntimeValue
            | Self::Int => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataArgumentMode {
    Positional,
    NamedOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetadataRootArgument {
    pub name: &'static str,
    pub input_type: MetadataRootInputType,
    pub cardinality: MetadataFieldCardinality,
    pub mode: MetadataArgumentMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataRootOverload {
    AuthorizedSnapshotHandle,
    TypedMethodHandle,
}

impl MetadataRootOverload {
    pub const fn input_type(self) -> MetadataRootInputType {
        match self {
            Self::AuthorizedSnapshotHandle => MetadataRootInputType::AuthorizedSnapshotHandle,
            Self::TypedMethodHandle => MetadataRootInputType::MethodHandle,
        }
    }
}


#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataRootScope {
    EvaluationSite,
    LexicalSnapshot,
    CurrentAuthorizedProgram,
    LexicalPackage,
    LexicalSource,
    RuntimeValue,
    RetainedRuntimeTypeCatalog,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetadataRootResult {
    pub success: MetadataValueType,
    pub error: Option<MetadataRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataOperationInputType {
    String,
    Generated,
    Provider,
    Identity(MetadataIdentity),
    Record(MetadataRecord),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetadataOperationArgument {
    pub name: &'static str,
    pub input_type: MetadataOperationInputType,
    pub cardinality: MetadataFieldCardinality,
}
