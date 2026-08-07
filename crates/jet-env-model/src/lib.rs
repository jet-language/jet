//! `jet-env-model` — the shared, pure plan model (card #367, D-PRODUCT-SPLIT1=C
//! slice 4): `ModuleEval` (the computed-modules evaluator, Stages 2-4) and its
//! typed plan outputs (`EnvPlan`/`SystemPlan`/`ImagePlan`/`FleetPlan`/`HostPlan`/
//! `VmTestPlan`/`ServicePlan`/`DevServicePlan`/`AdapterPlan`/`EvaluatedModule`).
//!
//! This crate parses an `env.jet`/`config.jet` module surface, runs it through
//! the M9.5 comptime interpreter, feeds §6 structural merge, and emits typed
//! plans. It holds no provider/store/network/shell engine code — those are the
//! two realizers that sit *above* this crate:
//!
//! - `jetpack`'s env-runtime (`run_enter_dev`/`Shell`/`Overlay`/`EnvFile`/
//!   `Services`) — the dev-shell tier.
//! - `jetpack`'s JetOS realization (`JetOS/*`) — the system/image/fleet tier.
//!
//! Both depend *down* on this crate for the shared plan model (layering, not
//! surgery — `ModuleEval` was already pure before this split).
//!
//! Deps: `jet-pkg-model` (the `Merge`/`RefSpec`/`Package`/`Recipe`
//! data types this crate's plans embed) + `jet-codegen` (the compiler frontend
//! funnel `jetpack` already uses: AST/Comptime/Parser/Lexer/Diagnostics/Sema/
//! Syntax) — both path-only (I6). No provider/store/network/shell dep.

#![allow(non_snake_case)]
#![deny(warnings)]

// Re-export the compiler frontend funnel so files in this crate can use
// `crate::AST`, `crate::Comptime`, `crate::Diagnostics`, `crate::Lexer`,
// `crate::Parser`, `crate::Sema`, `crate::Syntax` unchanged from their
// original `jetpack` paths — same pattern `jetpack` and `jet-pkg-model`
// already use for this re-export.
pub use jet_codegen::{Comptime, Diagnostics, Lexer, Parser, Sema, Syntax, AST};

// Re-export the L1 plan-data types so `ModuleEval`'s internal
// `super::super::{Merge,Package,RefSpec,Recipe}` paths (unchanged from their
// original `jetpack`-relative depth: `ModuleEval/*.rs` sits one level under
// this crate root, exactly as it did under `jetpack`'s) resolve without a
// text rewrite.
pub use jet_pkg_model::{Merge, Package, Recipe, RefSpec};

// Card #367 slice 5: `WorkspaceFile` (load/evaluate) now lives here — L2
// eval layer on top of L1 plan types (`WorkspacePlan`/`WorkspaceMember` in
// `jet-pkg-model::WorkspacePlan`, overlay parse in `jet-pkg-model::Overlay`).
// Re-export the L1 Overlay module and WorkspacePlan types under `crate::`
// paths so `WorkspaceFile.rs` (one level down) can reference them as
// `crate::Overlay::…` / `crate::WorkspacePlan::…` unchanged.
pub use jet_pkg_model::{Overlay, WorkspaceLock, WorkspacePlan};

pub mod ModuleEval;
pub mod WorkspaceFile;
