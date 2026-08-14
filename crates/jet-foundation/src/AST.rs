//! AST nodes. Grows with each milestone; keep nodes small and keep spans on
//! anything an error might need to point at.

#[path = "AST/types.rs"]
mod types;
pub use types::{
    AccessConvention, CallablePolicy, CallablePolicyChain, CompositeTypePairError, Dimension,
    FunctionCallMetadata, FunctionObligations, InternalTag,
    KnowledgeEntry,
    KnowledgeFact, KnowledgeVector, Measure, TagMarker, Type, TypeIdentity, int_spelling,
    numeric_type_from_name, int_range, canonicalize_tuple_fields, canonicalize_union,
    union_member_tag, union_enum_name,
};

#[path = "AST/program_imports.rs"]
mod program_imports;
pub use program_imports::{
    AppliedRuleApplication, ErrorConvDef, FencedNames, FencedStatement, ForeignLanguage,
    ForeignImportError, ForeignNamespace, ImportBinding, ImportDecl, ImportKind, InlineVersion,
    LoadedModule, MigrationDecl,
    MigrationOp, Program, ProgramBundle, TryConvert, CoreListPath, core_list_path,
    core_list_prefix, member_import_local,
    walk_imports,
};
pub use crate::Names::{
    mangle, mangle_generated, mangle_path, member_name, NameLedger,
};

#[path = "AST/items.rs"]
mod items;
pub use items::{
    Item, CodeModule, ModuleInstanceApplication, ModuleInstanceIdentity, GenericModuleParam, ModuleArg, GenericModuleDef, ModuleAliasDef, CModuleKind,
    CModule, ModuleDecl, SourceDecl, Contribution, ContribValue, EnvLit, SystemLit, SystemField,
    SystemFieldValue, ServiceEntry, OptionEntry, ImageLit, ImageField, ImageFromRef,
    ImageFieldValue, FleetLit, FleetField, FleetFieldValue, HostEntry, VmTestLit, VmTestField,
    VmTestFieldValue, PerfLit, CompileWorkloadDecl, ProfileLit, BudgetDecl, BudgetField, Namespace, TypeParam, TraitDef, TagDef, ProtocolDirection, ProtocolMessage,
    ProtocolDecl, StateDecl, EffectDecl, MarkerDecl, MarkerDeclParam, MarkerTextDecl, FactDecl, DeriveDef, DeriveBodyItem, TraitMethodSig, TraitImplBlock, ExternRustBlock, ExternFn,
    TestDef, BenchDef, MaturityTag, KernelMode, KernelProof, KernelMarker, Func, JobScope, TaskMetadata, TaskSkip, TaskCachePolicy, InlineForeign, ContractClause, StateTransition, EveryMarker,
    DEFAULT_MEMO_BOUND, memo_bound_from_markers,
    EveryArg, EverySchedule, EveryScheduleError, Param, ParamZone, StructLayout, CEnumTag,
    Marker, StructDef, TypeAliasDef, DistinctDef, QuantityKind, UnitDimensionDecl, UnitFamilyDef, UnitFamilyMember, UnitRatio, UnitScaleProvenance, EnumDef,
    EnumGroup, Variant, VariantPayload, VariantField, ImplDef, Field, SerdeWireShape,
    resolved_decode_wire_shapes,
};

#[path = "AST/patterns.rs"]
mod patterns;
pub use patterns::{
    PatSlot, Pattern, StrMatchPart, StructPatField, BindName, BindPattern, OrFallback,
    EnumLitArg, ConstAttr, ConstDef, OutputCallableAuthority, OutputKind, ResolvedOutput,
    RustConstKind,
    BinMatchPart, BinSpec, BinEndian,
};

#[path = "AST/statements.rs"]
mod statements;
pub use statements::{
    ElseBranch, IfStmt, Stmt, SwitchArm, is_subjectless_guard, noelse_terminated,
    switched_off, uses_classic_if_spelling,
};

#[path = "AST/lvalues.rs"]
mod lvalues;
pub use lvalues::{Binding, ForKind, GcPromotion, GcPromotionEdge, IndexKind, LValue, MetaAttr, MetaFacts, MetaField};

#[path = "AST/expressions.rs"]
mod expressions;
pub use expressions::{
    Call, CallArgFlags, CallArg, BinOp, UnOp, IncDecOp, StrFormat, UnitFormat, StrPart, LambdaParam,
    LambdaBody, LambdaMeta, Lambda, PlaceAccess, Expr, TypedLitBody,
};

#[path = "AST/comptime.rs"]
mod comptime;
pub use comptime::{
    canonical_view_provenance_map, ClosureData, CtFloat, CtKey, CtOpaque, CtReport,
    CtValue, FuncSig, ViewProvenance,
    ViewProvenanceCell, ViewProvenanceMap, ViewSource, ViewSourcePath,
    ViewSourceProjection,
};

#[path = "AST/ffi.rs"]
mod ffi;
pub use ffi::{CImportLink, CLib, CFfi, ComptimeInput, FfiLink};
