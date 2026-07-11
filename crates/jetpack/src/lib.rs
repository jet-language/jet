//! Jetpack — Jet's package manager engine and Phase 1 CLI (D-JPK*).
//!
//! Phase 1 is the Nix-`shell`/`devenv`-class temporary environment:
//! `jetpack run <source>:<package>` resolves a ref through a provider, realizes
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

pub mod Bridge;
pub mod BuildDebug;
pub mod CBind;
pub mod CFFI;
pub mod CLI;
pub mod Components;
pub mod Discovery;
pub mod Doctor;
pub mod EffectBudget;
pub mod EnvFile;
pub mod Envelope;
pub mod FFI;
pub mod Image;
pub mod JSON;
pub mod JetOS;
pub mod JetPin;
pub mod Lock;
pub mod Manifest;
pub mod ManifestTOML;
pub mod Merge;
pub mod MigrationImport;
pub mod ModuleEval;
pub mod Output;
pub mod Overlay;
pub mod PackageGraph;
pub mod PackageManifest;
pub mod Platform;
pub mod PluginExport;
pub mod Provider;
pub mod ProviderGraph;
pub mod Recipe;
pub mod RefSpec;
pub mod Replacement;
pub mod RuntimePolicy;
pub mod ScriptDeps;
pub mod ScriptLock;
pub mod Secrets;
pub mod SemanticLock;
pub mod Services;
pub mod Shell;
pub mod Store;
pub mod TOML;
pub mod Toolchain;
pub mod Trust;
pub mod WorkspaceFile;
pub mod WorkspaceLock;

/// Process entry point used by the `jetpack` binary.
pub fn run(args: Vec<String>) -> i32 {
    CLI::main(args)
}
