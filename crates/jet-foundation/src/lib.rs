#![allow(non_snake_case)]
#![deny(warnings)]
pub mod AST;
mod BuildEffects;
pub mod CanonicalAST;
pub mod CliSchema;
pub mod Collections;
pub mod Diagnostics;
mod ExactUnitConversion;
pub mod ExitCodes;
pub mod Generics;
pub mod generated;
pub mod JitBackend;
pub mod JSON;
pub mod Numeric;
pub mod OsTarget;
pub mod JetTrace;
pub mod PerformanceBudget;
pub mod Policy;
pub mod RingLayer;
pub mod SHA256;
pub mod Syntax;
pub mod TargetProfile;
pub mod Terminal;
pub mod Traits;
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
