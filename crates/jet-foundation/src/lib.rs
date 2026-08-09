#![allow(non_snake_case)]
#![deny(warnings)]
pub mod AST;
mod BuildEffects;
pub mod CanonicalAST;
pub mod CLISchema;
pub mod Collections;
/// Canonical, dependency-free `core.archive` ABI kernel. The same source is
/// included by the package bridge, JIT host, and resident evaluator.
#[path = "CoreArchive.rs"]
pub mod CoreArchive;
pub mod CoreModuleExports;
pub mod Diagnostics;
mod ExactUnitConversion;
pub mod ExitCodes;
pub mod Facts;
pub mod Generics;
pub mod generated;
pub mod JitBackend;
pub mod JSON;
pub mod Numeric;
pub mod OSTarget;
pub mod Outcome;
pub mod JetTrace;
pub mod PerformanceBudget;
pub mod Persist;
pub mod Policy;
pub mod RingLayer;
pub mod RegexSyntax;
pub mod Registry;
pub mod SHA256;
pub mod MatchScan;
pub mod Names;
pub mod StreamCursor;
pub mod StructuralDebug;
pub mod Syntax;
pub mod TargetMachine;
pub mod Terminal;
pub mod Traits;
pub mod WasmDebug;
pub mod WebApp;
pub mod WebPartition;
pub mod XmlPull;
#[path = "BaseEncodingStrict.rs"]
pub mod base_encoding_strict;
#[path = "BaseEncodingDispatch.rs"]
pub mod base_encoding_dispatch;
#[path = "PackageEdition.rs"]
pub mod PackageEdition;
pub use BuildEffects::BuildEffect;
pub use ExactUnitConversion::{
    UnitRoundingMode, UNIT_ROUNDING_NEGATIVE_DIGITS, UNIT_ROUNDING_UNREPRESENTABLE,
    jet_unit_conversion_exact, jet_unit_conversion_rounded,
};
