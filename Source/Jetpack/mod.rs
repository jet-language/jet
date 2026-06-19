//! Jetpack — Jet's package manager engine and Phase 1 CLI (D-JPK*).
//!
//! Phase 1 is the Nix-`shell`/`devenv`-class temporary environment:
//! `jetpack run <source>:<package>` resolves a ref through a provider, realizes
//! it into the Jetpack store, composes an env, and drops the user into a pretty
//! subshell that `exit` leaves cleanly. Jetpack owns the package lifecycle; Nix
//! is a compatibility provider (D-JPK5).
//!
//! Built std-only (I6) and independent from the `jet` binary (D-JPK1). The
//! consolidated plan lives in `docs/plans/jetpack-jetos/README.md`.

pub mod CLI;
pub mod EnvFile;
pub mod JetOS;
pub mod JSON;
pub mod ManifestTOML;
pub mod Merge;
pub mod ModuleEval;
pub mod Output;
pub mod PackageManifest;
pub mod Provider;
pub mod RefSpec;
pub mod Shell;
pub mod Store;

/// Process entry point used by the `jetpack` binary.
pub fn run(args: Vec<String>) -> i32 {
    CLI::main(args)
}
