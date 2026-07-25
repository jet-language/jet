//! AST nodes. Grows with each milestone; keep nodes small and keep spans on
//! anything an error might need to point at.

#[path = "AST/types.rs"]
mod types;
pub use types::{
    AccessConvention, Dimension, Type, int_spelling, numeric_type_from_name, int_range,
    canonicalize_tuple_fields, canonicalize_union, union_member_tag, union_enum_name,
    CORE_CRYPTO_NOMINAL_MARKER, CPP_CALLBACK_ABI_MARKER,
    DETERMINISTIC_CLOCK_MARKER, EXPIRING_SECRET_LOAN_MARKER, SYSTEM_CLOCK_MARKER,
};

#[path = "AST/program_imports.rs"]
mod program_imports;
pub use program_imports::{
    Program, ImportDecl, InlineVersion, ForeignLanguage, ForeignNamespace, ImportKind,
    ProgramBundle, LoadedModule, TryConvert, ErrorConvDef, MigrationDecl, MigrationOp,
};

#[path = "AST/items.rs"]
mod items;
pub use items::{
    Item, CodeModule, ModuleInstanceApplication, ModuleInstanceIdentity, GenericModuleParam, ModuleArg, GenericModuleDef, ModuleAliasDef, CModuleKind,
    CModule, ModuleDecl, SourceDecl, Contribution, ContribValue, EnvLit, SystemLit, SystemField,
    SystemFieldValue, ServiceEntry, OptionEntry, ImageLit, ImageField, ImageFromRef,
    ImageFieldValue, FleetLit, FleetField, FleetFieldValue, HostEntry, VmTestLit, VmTestField,
    VmTestFieldValue, PerfLit, BudgetDecl, BudgetField, Namespace, TypeParam, TraitDef, TagDef, ProtocolDirection, ProtocolMessage,
    ProtocolDecl, StateDecl, DeriveDef, TraitMethodSig, TraitImplBlock, ExternRustBlock, ExternFn,
    TestDef, BenchDef, MaturityTag, Func, InlineForeign, ContractClause, StateTransition, EveryMarker,
    EveryArg, EverySchedule, EveryScheduleError, Param, StructLayout, CEnumTag,
    Marker, StructDef, TypeAliasDef, DistinctDef, QuantityKind, UnitFamilyDef, UnitFamilyMember, UnitRatio, EnumDef,
    EnumGroup, Variant, VariantPayload, VariantField, ImplDef, Field,
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
pub use statements::{is_subjectless_guard, ElseBranch, IfStmt, Stmt, SwitchArm};

#[path = "AST/lvalues.rs"]
mod lvalues;
pub use lvalues::{Binding, ForKind, GcPromotion, GcPromotionEdge, IndexKind, LValue, MetaAttr, MetaFacts, MetaField};

#[path = "AST/expressions.rs"]
mod expressions;
pub use expressions::{
    Call, CallArgFlags, CallArg, BinOp, UnOp, IncDecOp, StrFormat, StrPart, LambdaParam,
    LambdaBody, LambdaMeta, Lambda, PlaceAccess, Expr, TypedLitBody,
};

#[path = "AST/comptime.rs"]
mod comptime;
pub use comptime::{
    canonical_view_provenance_map, ClosureData, CtFloat, CtKey, CtValue, FuncSig, ViewProvenance,
    ViewProvenanceMap, ViewSource,
    ViewSourceProjection,
};

#[path = "AST/ffi.rs"]
mod ffi;
pub use ffi::{CImportLink, CLib, CFfi, ComptimeInput, FfiLink};
