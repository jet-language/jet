//! Jetpack — Jet's package manager engine and Phase 1 CLI (D-JPK*).
//!
//! Phase 1 is the Nix-`shell`/`devenv`-class temporary environment:
//! `jetpack run <package>@<source>` resolves a ref through a provider, realizes
//! it into the Jetpack store, composes an env, and drops the user into a pretty
//! subshell that `exit` leaves cleanly. Jetpack owns the package lifecycle; Nix
//! is a compatibility provider (D-JPK5).
//!
//! Built std-only (I6) and independent from the `jet` binary (D-JPK1). The
//! consolidated plan lives in `docs/plans/epoch-5/README.md`.

#![allow(non_snake_case)]
#![deny(warnings)]

pub use jet_codegen::{
    Codegen, Comptime, Diagnostics, Lexer, Parser, Sema, Syntax, AST, SHA256,
};

// Card #367 / D-PRODUCT-SPLIT1=C: the read-only package/config data model
// (manifest/lock/store-listing/ref/FFI-binding/script-dep parsing), plus the
// pure effect-budget/lint-policy computation slice 3 also moved out (neither
// touches network/provider/shell), lives in `jet-pkg-model` so `jet-driver`
// (and now `jet` itself) can depend on it without pulling in this crate's
// provider/network/shell engine. Re-exported under their historical paths so
// every internal call site in this crate (`crate::Package`,
// `super::RefSpec`, `crate::EffectBudget`, etc.) is unchanged.
pub use jet_pkg_model::{
    AdaBind, CBind, CFFI, ComBind, CppBind, DartBind, DotNetBind, EffectBudget, Envelope, FFI, FortranBind,
    JavaBind, JSON, LintPolicy, Lock, Manifest, Package, PascalBind, Platform,
    PowerShellBind, RefSpec, ScriptDeps, TclBind, Variant,
};
// Card #367 slice 5: WorkspacePlan/WorkspaceMember + WorkspaceLock read path
// now live in jet-pkg-model (L1). WorkspaceFile eval lives in jet-env-model
// (L2). jetpack re-exports under the historical module paths via the thin
// shims WorkspaceFile.rs and WorkspaceLock.rs in this crate.

pub mod Bridge;
pub mod BrowserLock;
pub mod BuildDebug;
pub mod CLI;
pub mod Components;
pub mod Discovery;
pub mod Doctor;
pub mod EnvFile;
pub mod EnvFiles;
pub mod EnvHook;
pub mod Image;
pub mod JetOS;
pub mod JetPin;
pub mod ManifestTOML;
pub mod MemberSelect;
pub mod Merge;
pub mod MigrationImport;
// Card #367 slice 4: `ModuleEval` (the computed-modules evaluator + plan
// types) now lives in `jet-env-model` (L2, pure eval) — both realizers,
// jetpack's env-runtime and JetOS realization, name it directly
// (`jet_env_model::ModuleEval`) instead of sharing it by living in the same
// crate. No re-export here; that was the step-2 shim, now dropped.
/// E4-JP8 — native Nix `.drv` / path calculus (internal compat surface).
pub mod NixDrv;
/// E4-JP9 — native evaluator internals and the bounded typed devShell entry.
pub(crate) mod NixEval;
pub mod Output;
pub mod Overlay;
pub mod PackageGraph;
pub mod Provider;
pub mod ProviderGraph;
pub mod Recipe;
pub mod Replacement;
pub mod RuntimePolicy;
pub mod ScriptLock;
pub mod Secrets;
pub mod SemanticLock;
pub mod Services;
pub mod Shell;
pub mod Store;
pub mod TOML;
pub mod Toolchain;
pub mod Trust;
pub mod TrustRoot;
pub mod Transition;
pub mod WorkspaceFile;
pub mod WorkspaceLock;

/// Process entry point used by the `jetpack` binary.
pub fn run(args: Vec<String>) -> i32 {
    CLI::main(args)
}
