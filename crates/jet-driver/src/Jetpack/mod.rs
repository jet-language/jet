//! Jetpack — Jet's package manager engine and Phase 1 CLI (D-JPK*).
//!
//! Phase 1 is the Nix-`shell`/`devenv`-class temporary environment:
//! `jetpack run <source>:<package>` resolves a ref through a provider, realizes
//! it into the Jetpack store, composes an env, and drops the user into a pretty
//! subshell that `exit` leaves cleanly. Jetpack owns the package lifecycle; Nix
//! is a compatibility provider (D-JPK5).
//!
//! Built std-only (I6) and independent from the `jet` binary (D-JPK1). The
//! consolidated plan lives in `tools/Tower/docs/plans/epoch-5/README.md`.

pub mod Bridge;
pub mod CLI;
pub mod Components;
pub mod EffectBudget;
pub mod Envelope;
pub mod EnvFile;
pub mod Image;
pub mod JSON;
pub mod JetOS;
pub mod JetPin;
pub mod ManifestTOML;
pub mod Merge;
pub mod ModuleEval;
pub mod Output;
pub mod PackageManifest;
pub mod PluginExport;
pub mod Provider;
pub mod Recipe;
pub mod RefSpec;
pub mod ScriptDeps;
pub mod ScriptLock;
pub mod Secrets;
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
