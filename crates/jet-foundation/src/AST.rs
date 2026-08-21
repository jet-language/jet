//! AST nodes. Grows with each milestone; keep nodes small and keep spans on
//! anything an error might need to point at.

#[path = "AST/types.rs"]
mod types;
pub use types::{
    canonicalize_tuple_fields, canonicalize_union, int_range, int_spelling, numeric_type_from_name,
    union_enum_name, union_member_tag, AccessConvention, CallablePolicy, CallablePolicyChain,
    CompositeTypePairError, Dimension, Exactness, FunctionCallMetadata, FunctionObligations,
    InternalTag, KnowledgeEntry, KnowledgeFact, KnowledgeVector, Measure, MeasureRule, TagMarker,
    Type, TypeIdentity,
};

#[path = "AST/program_imports.rs"]
mod program_imports;
pub use crate::Names::{mangle, mangle_generated, mangle_path, member_name, NameLedger};
pub use program_imports::{
    core_import_maps, core_list_path, core_list_prefix, member_import_local,
    rewrite_core_item_call, walk_imports, AppliedRuleApplication, CoreListPath, ErrorConvDef,
    FencedNames, FencedStatement, ForeignImportError, ForeignLanguage, ForeignNamespace,
    ImportBinding, ImportDecl, ImportKind, InlineVersion, LoadedModule, MigrationDecl, MigrationOp,
    PackageGuarantees, Program, ProgramBundle, TryConvert,
};

#[path = "AST/items.rs"]
mod items;
pub use items::{
    app_entry_run_fn, bundle_serves_until_stopped, memo_bound_from_markers,
    resolved_decode_wire_shapes, type_is_app, BudgetDecl, BudgetField, CEnumTag, CLICommandBinding,
    CModule, CModuleKind, CodeModule, CompileWorkloadDecl, ContractClause, ContribValue,
    Contribution, DeriveBodyItem, DeriveDef, DistinctDef, EffectDecl, EnumDef, EnumGroup, EnvLit,
    EveryArg, EveryMarker, EverySchedule, EveryScheduleError, ExternFn, ExternRustBlock, FactDecl,
    Field, FleetField, FleetFieldValue, FleetLit, Func, GenericModuleDef, GenericModuleParam,
    HostEntry, ImageField, ImageFieldValue, ImageFromRef, ImageLit, ImplDef, InlineForeign,
    Item, ItemTemplateLoop,
    JobCachePolicy, JobMetadata, JobScope, JobSkip, KernelMarker, KernelMode, KernelProof, Marker,
    MarkerDecl, MarkerDeclParam, MarkerTextDecl, MaturityTag, ModuleAliasDef, ModuleArg,
    ModuleDecl, ModuleInstanceApplication, ModuleInstanceIdentity, Namespace, OptionEntry, Param,
    ParamZone, PerfLit, ProfileLit, ProtocolDecl, ProtocolDirection, ProtocolMessage, QuantityKind,
    SerdeWireShape, ServiceEntry, SourceDecl, StateDecl, StateTransition, StructDef, StructLayout,
    SystemField, SystemFieldValue, SystemLit, TagDef, TestDef, TraitDef, TraitImplBlock,
    TraitMethodSig, TypeAliasDef, TypeParam, UnitDimensionDecl, UnitFamilyDef, UnitFamilyMember,
    UnitRatio, UnitScaleProvenance, UserPolicyDecl, Variant, VariantField, VariantPayload,
    VmTestField, VmTestFieldValue, VmTestLit, DEFAULT_MEMO_BOUND,
};

#[path = "AST/patterns.rs"]
mod patterns;
pub use patterns::{
    BinEndian, BinMatchPart, BinSpec, BindName, BindPattern, ConstAttr, ConstDef, EnumLitArg,
    OrFallback, OutputCallableAuthority, OutputKind, PatSlot, Pattern, ResolvedOutput,
    RustConstKind, StrMatchPart, StructPatField,
};

#[path = "AST/statements.rs"]
mod statements;
pub use statements::{
    is_subjectless_guard, noelse_terminated, readiness_head, switched_off,
    uses_classic_if_spelling, ElseBranch, IfStmt, ReadinessHead, Stmt, SwitchArm,
};

#[path = "AST/lvalues.rs"]
mod lvalues;
pub use lvalues::{
    Binding, ForKind, GcPromotion, GcPromotionEdge, IndexKind, LValue, MetaAttr, MetaFacts,
    MetaField,
};

#[path = "AST/expressions.rs"]
mod expressions;
pub use expressions::{
    BinOp, Call, CallArg, CallArgFlags, Expr, IncDecOp, Lambda, LambdaBody, LambdaMeta,
    LambdaParam, PlaceAccess, StrFormat, StrPart, TypedLitBody, UnOp, UnitFormat,
};

#[path = "AST/comptime.rs"]
mod comptime;
pub use comptime::{
    canonical_view_provenance_map, ClosureData, CtFloat, CtKey, CtOpaque, CtReport, CtValue,
    Deprecation, FuncSig, ViewProvenance, ViewProvenanceCell, ViewProvenanceMap, ViewSource, ViewSourcePath,
    ViewSourceProjection,
};

#[path = "AST/ffi.rs"]
mod ffi;
pub use ffi::{CFfi, CImportLink, CLib, ComptimeInput, FfiLink};
