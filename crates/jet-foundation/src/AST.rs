//! AST nodes. Grows with each milestone; keep nodes small and keep spans on
//! anything an error might need to point at.

#[path = "AST/types.rs"]
mod types;
pub use types::{
    AccessConvention, Type, int_spelling, numeric_type_from_name, int_range,
    canonicalize_tuple_fields,
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
    Item, CodeModule, ModuleInstanceIdentity, GenericModuleParam, ModuleArg, GenericModuleDef, ModuleAliasDef, CModuleKind,
    CModule, ModuleDecl, SourceDecl, Contribution, ContribValue, EnvLit, SystemLit, SystemField,
    SystemFieldValue, ServiceEntry, OptionEntry, ImageLit, ImageField, ImageFromRef,
    ImageFieldValue, FleetLit, FleetField, FleetFieldValue, HostEntry, VmTestLit, VmTestField,
    VmTestFieldValue, PerfLit, Namespace, TypeParam, TraitDef, TagDef, ProtocolDirection, ProtocolMessage,
    ProtocolDecl, StateDecl, DeriveDef, TraitMethodSig, TraitImplBlock, ExternRustBlock, ExternFn,
    TestDef, BenchDef, MaturityTag, Func, InlineForeign, ContractClause, StateTransition, EveryMarker,
    EveryArg, EverySchedule, EveryScheduleError, Param, StructLayout, CEnumTag,
    Marker, StructDef, TypeAliasDef, DistinctDef, UnitFamilyDef, EnumDef, EnumGroup, Variant,
    VariantPayload, VariantField, ImplDef, Field,
};

#[path = "AST/patterns.rs"]
mod patterns;
pub use patterns::{
    PatSlot, Pattern, StrMatchPart, StructPatField, BindName, BindPattern, OrFallback,
    EnumLitArg, ConstAttr, ConstDef, RustConstKind,
};

#[path = "AST/statements.rs"]
mod statements;
pub use statements::{IfStmt, ElseBranch, SwitchArm, Stmt};

#[path = "AST/lvalues.rs"]
mod lvalues;
pub use lvalues::{LValue, IndexKind, MetaField, MetaAttr, MetaFacts, ForKind, Binding};

#[path = "AST/expressions.rs"]
mod expressions;
pub use expressions::{
    Call, CallArgFlags, CallArg, BinOp, UnOp, IncDecOp, StrFormat, StrPart, LambdaParam,
    LambdaBody, LambdaMeta, Lambda, Expr,
};

#[path = "AST/comptime.rs"]
mod comptime;
pub use comptime::{FuncSig, CtValue, ClosureData, CtKey};

#[path = "AST/ffi.rs"]
mod ffi;
pub use ffi::{CImportLink, CLib, CFfi, ComptimeInput, FfiLink};
