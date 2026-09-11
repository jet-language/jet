#![allow(non_snake_case)]
#![deny(warnings)]
pub mod AST;
pub mod Authority;
mod BuildEffects;
pub mod CLISchema;
pub mod CanonicalAST;
pub mod Collections;
pub mod CompilerStack;
/// Canonical, dependency-free `core.archive` ABI kernel. The same source is
/// included by the package bridge, JIT host, and resident evaluator.
#[path = "CoreArchive.rs"]
pub mod CoreArchive;
pub mod CoreModuleExports;
pub mod Evidence;
pub mod Diagnostics;
pub mod Coverage;
pub mod Effects;
/// Backend-neutral semantic rows shared by purity, replayability, comptime,
/// invocation, and transaction checks.
pub mod sema;
mod ExactUnitConversion;
pub mod ExitCodes;
pub mod Facts;
pub mod Generics;
pub mod GzipKernel;
pub mod JSON;
pub mod DataTree;
pub mod JSONNumber;
pub mod JitBackend;
pub mod HotSwap;
/// Canonical game development swap facts and state kernel shared by hosts and generated Prelude code.
pub mod Game;
/// Canonical checked frame-resource dependency, lifetime, transfer, and reuse facts.
pub mod ResourceSchedule;
/// Derivation records shared by evidence reports and generated AOT Prelude.
pub mod FactsDerivation;
pub mod CanonicalPass;
pub mod MIR;
pub mod MIROptimization;
pub mod generated;
#[allow(unused_imports)]
pub(crate) use JSONNumber as jet_json_number;
pub mod EncodingErrors;
pub mod EncodingJson;
#[allow(unused_imports)]
pub(crate) use EncodingErrors as jet_encoding_errors;
pub mod Devtools;
pub mod DevtoolsControl;
pub mod AppMiddleware;
pub mod App;
#[path = "CborBudget.rs"]
pub mod CborBudget;
#[path = "CborKernel.rs"]
pub mod CborKernel;
#[path = "CsvKernel.rs"]
pub mod CsvKernel;
/// Canonical loader lifecycle, identity, credential-sanitization, and snapshot
/// kernel shared by generated Prelude, JIT, and comptime adapters.
#[path = "PreludeDataFlow.rs"]
pub mod PreludeDataFlow;
/// Tier-neutral generation/cancellation/publication lifecycle for typed live
/// adapters and the global live registry.
pub mod LiveLifecycle;
/// Checked ownership and layout validation for the Arrow C data interface.
pub mod ArrowData;
/// Registered Arrow/Parquet byte-source provider boundary.
pub mod ArrowFileReader;

pub mod JetTrace;
pub mod Layout;
pub mod LintPolicy;
pub mod MachineOutput;
pub mod MatchScan;
pub mod MemSentry;
/// One monotonic epoch for every in-process adapter. AOT embeds the same
/// source once; JIT, TIR, and comptime call this Foundation module instead of
/// creating textual copies of the state.
pub mod Monotonic;
pub mod Names;
pub mod Numeric;
pub mod NumericConversion;
pub mod OSTarget;
pub mod Outcome;
#[path = "PackageEdition.rs"]
pub mod PackageEdition;
pub mod PerformanceBudget;
pub mod Persist;
pub mod PluginWire;
pub mod Policy;
pub mod Prelude;
pub mod Reflection;
pub mod RegexSyntax;
pub mod Registry;
pub mod Report;
pub mod RuntimeDiagnosticCore;
pub mod RingLayer;
pub mod SHA256;
pub mod ServiceTree;
pub mod StreamCursor;
pub mod StructuralDebug;
pub mod Shape;
/// Canonical published-schema migration plans and receipts shared by sema,
/// codegen, and resident execution tiers.
pub mod SchemaMigration;
pub mod Syntax;
pub mod TargetMachine;
pub mod Terminal;
pub mod TestingComparison;
pub mod TestingHistory;
pub mod Traits;
pub mod WasmDebug;
pub mod WebPartition;
pub mod XmlKernel;
pub mod XmlPull;
#[path = "BaseEncodingDispatch.rs"]
pub mod base_encoding_dispatch;
#[path = "BaseEncodingStrict.rs"]
pub mod base_encoding_strict;
pub use BuildEffects::BuildEffect;
pub use ExactUnitConversion::{
    jet_unit_conversion_exact, jet_unit_conversion_rounded, UnitRoundingMode,
    UNIT_ROUNDING_NEGATIVE_DIGITS, UNIT_ROUNDING_UNREPRESENTABLE,
};
