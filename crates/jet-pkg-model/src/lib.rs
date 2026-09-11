//! `jet-pkg-model` — the shared, read-only package/config data model (card
//! #367, D-PRODUCT-SPLIT1=C).
//!
//! This crate is the manifest/lock/store/ref/workspace/script *data* layer:
//! parsing `package.jet`, the compiler-facing `Manifest`/`Lock` types, the C FFI
//! binding-generation surface, inline script dependencies, and the read-only
//! subset of the Jetpack hangar store (root resolution + listing already
//! recorded entries). It holds no network/provider/shell engine code — that
//! stays in the `jetpack` crate, which re-exports every module here under
//! its historical paths so its own internal call sites are unchanged.
//!
//! `jet-driver` (the compiler's module loader) depends on this crate instead
//! of the full `jetpack` engine, so the compiler's dependency graph never
//! needs Jetpack's provider/network/shell machinery to resolve `use <pkg>`
//! imports against already-realized packages.

#![allow(non_snake_case)]
#![deny(warnings)]

// Re-export lower seams so files in this crate can use `crate::Diagnostics`,
// `crate::SHA256`, and, in the host compiler build, `crate::AST`,
// `crate::Lexer`, `crate::Parser`, `crate::Policy`, `crate::Sema`,
// `crate::Syntax`, and `crate::TargetMachine` without cross-crate path
// changes.  The model/provider runtime deliberately does not pull the
// compiler-only semantic preparation graph.
pub use jet_foundation::{Diagnostics, SHA256};
#[cfg(feature = "compiler")]
pub use jet_sema::{AST, Lexer, Parser, Policy, Sema, Syntax, TargetMachine};

#[cfg(feature = "compiler")]
pub mod AdaBind;
#[cfg(feature = "compiler")]
pub mod Authority;
#[cfg(feature = "compiler")]
pub mod Bindgen;
#[cfg(feature = "compiler")]
pub mod CBind;
#[cfg(feature = "compiler")]
pub mod CFFI;
#[cfg(feature = "compiler")]
pub mod CobolBind;
#[cfg(feature = "compiler")]
pub mod ComBind;
#[cfg(feature = "compiler")]
pub mod CppBind;
#[cfg(feature = "compiler")]
pub mod DartBind;
#[cfg(feature = "compiler")]
pub mod DotNetBind;
#[cfg(feature = "compiler")]
pub mod FortranBind;
#[cfg(feature = "compiler")]
pub mod GoBind;
#[cfg(feature = "compiler")]
pub mod JavaBind;
#[cfg(feature = "compiler")]
pub mod JavaScriptBind;
#[cfg(feature = "compiler")]
pub mod LuaBind;
#[cfg(feature = "compiler")]
pub mod OctaveBind;
#[cfg(feature = "compiler")]
pub mod PascalBind;
#[cfg(feature = "compiler")]
pub mod PerlBind;
#[cfg(feature = "compiler")]
pub mod PhpBind;
#[cfg(feature = "compiler")]
pub mod PowerShellBind;
#[cfg(feature = "compiler")]
pub mod PythonBind;
#[cfg(feature = "compiler")]
pub mod RBind;
#[cfg(feature = "compiler")]
pub mod RubyBind;
#[cfg(feature = "compiler")]
pub mod TclBind;

// Card #367 / D-PRODUCT-SPLIT1=C slice 3: pure policy computation over the
// manifest/effect-fixpoint data (no network/provider/shell engine code, same
// bar as the rest of this crate) — moved here from `jetpack` so `jet`'s own
// `build`/`run` effect-budget summary and lint-policy enforcement no longer
// need the full Jetpack engine, only this read-only-adjacent data/policy
// layer. `jetpack` re-exports both under their historical paths.
#[cfg(feature = "compiler")]
pub mod EffectBudget;
#[cfg(feature = "compiler")]
pub mod Envelope;
#[cfg(feature = "compiler")]
pub mod ForeignBridge;
// D-DX5-HOOK1=A / Tower #549: pure compiler-extension protocol/snapshot model.
// The Wasmtime host source stays beside `Prelude/Plugin.rs`, but only the
// isolated `jetpack` binary compiles it; the compiler never links Wasmtime.
#[cfg(feature = "compiler")]
pub mod CompilerExtension;
#[cfg(feature = "compiler")]
pub mod FFI;
// Card #1421 criteria 2-3: `.jetlib` loadable-library artifact stamp and the
// load-time trust boundary (D-LIB-REUSE1=B compiler-identity pin,
// D-LIB-DYNTRUST1=A declared-effect grant). The driver and embedded Prelude
// consume this pure data/check machinery; no second loader policy is allowed.
#[cfg(feature = "compiler")]
pub mod JetLib;
pub use jet_foundation::JSON;
#[cfg(feature = "compiler")]
pub mod LintPolicy;
#[cfg(feature = "compiler")]
pub mod Lock;
#[cfg(feature = "compiler")]
pub mod Manifest;
#[cfg(feature = "compiler")]
pub mod MCP;
// Card #367 slice 4: `Merge` (§6 structural merge, pure/std-only) sunk from
// `jetpack` — `ModuleEval` (jet-env-model, L2) and both realizers need it, so
// it belongs at the plan-model's foundation, not inside one engine crate.
#[cfg(feature = "compiler")]
pub mod Merge;
#[cfg(feature = "compiler")]
pub mod Package;
pub mod Model;
/// Package parsing extensions over the runtime-owned model contract.
pub use Model::*;
// Deterministic desktop/game bundle layout data; platform packers stay in adapters.
#[cfg(feature = "compiler")]
pub mod Bundler;
#[cfg(feature = "compiler")]
pub mod Public;
#[cfg(feature = "compiler")]
pub mod Platform;
#[cfg(feature = "compiler")]
pub mod ProviderFacts;
// selection. Pure data/matching — jetpack + BuildPlan action keys consume it.
#[cfg(feature = "compiler")]
pub mod Variant;
// build engine (validate/run/fetch/exec/sandbox) stays in `jetpack`'s
// `Recipe.rs`, which imports these types from here (data-down / engine-up,
// same pattern as `EffectBudget`/`LintPolicy`).
#[cfg(feature = "compiler")]
pub mod Recipe;
#[cfg(feature = "compiler")]
pub mod RefSpec;
#[cfg(feature = "compiler")]
pub mod ScriptDeps;
// Envelope's canonical-archive walker: the walk dominates once hashing is
// fast, so it lives in this opt-level=3 crate.
#[cfg(feature = "compiler")]
pub mod SealWalk;
#[cfg(feature = "compiler")]
pub mod Store;
// types, and lock read API — moved here from `jetpack` so Canvas / devserver
// can scan workspaces via `jet-env-model` / `jet-pkg-model` without pulling in
// the full engine crate.  `jetpack` re-exports all three under their historical
// paths so its own internal call sites are unchanged.
#[cfg(feature = "compiler")]
pub mod Overlay;
#[cfg(feature = "compiler")]
pub mod WorkspaceLock;
#[cfg(feature = "compiler")]
pub mod WorkspacePlan;
