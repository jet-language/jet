#![allow(non_snake_case)]
#![deny(warnings)]
// Re-export lower seams so `crate::AST`, `crate::Lexer`, `crate::Parser`, `crate::Comptime`
// all work within Sema source files without cross-crate path changes.
pub use jet_comptime::Comptime;
pub use jet_parser::{
    CanonicalAST, Collections, Diagnostics, Formatter, Generics, Lexer, Numeric, Parser, Syntax,
    TargetProfile, Traits, AST, SHA256,
};
pub mod Sema;
pub use Sema::{effect_key, DefinitionAnchorFact, SemIndexEffectFacts};
pub use Sema::{collect_budget_specs, BudgetApplicability, BudgetAxis, BudgetSpec};
pub use Sema::{
    collect_policy_facts, collect_policy_facts_from_program, PolicyDomain, PolicyFact,
    PolicyFactGraph,
};
