#![allow(non_snake_case)]
#![deny(warnings)]
pub mod AST;
mod BuildEffects;
pub mod CanonicalAST;
pub mod Collections;
pub mod Diagnostics;
pub mod Generics;
pub mod generated;
pub mod JitBackend;
pub mod Numeric;
pub mod OsTarget;
pub mod PerformanceBudget;
pub mod RingLayer;
pub mod SHA256;
pub mod Syntax;
pub mod TargetProfile;
pub mod Traits;
pub mod WebPartition;
pub use BuildEffects::BuildEffect;
