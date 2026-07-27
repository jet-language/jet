// ── C-FFI data types ──────────────────────────────────────────────────────────

use std::path::{Path, PathBuf};

/// The result of resolving one C `use` in one file.
#[derive(Debug, Clone)]
pub struct CImportLink {
    pub importing_idx: usize,
    pub alias: String,
    pub target_idx: usize,
}

/// One C library that the program links against.
#[derive(Debug, Clone)]
pub struct CLib {
    pub lib: String,
    pub module_idx: usize,
}

/// Gathered C-FFI artifacts threaded into sema and codegen.
#[derive(Debug, Default, Clone)]
pub struct CFfi {
    pub import_links: Vec<CImportLink>,
    pub libs: Vec<CLib>,
}

impl CFfi {
    pub fn target_for(&self, importing_idx: usize, alias: &str) -> Option<usize> {
        self.import_links
            .iter()
            .find(|l| l.importing_idx == importing_idx && l.alias == alias)
            .map(|l| l.target_idx)
    }

    pub fn links_c(&self) -> bool {
        !self.libs.is_empty()
    }
}

// ── Comptime embed input ──────────────────────────────────────────────────────

/// D-CTEFFECT1 (Tier-1): one comptime embed input recorded for reproducibility.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComptimeInput {
    pub path: String,
    pub hash: String,
}

// ── Rust FFI link artifact ────────────────────────────────────────────────────

/// Built FFI bridge artifact paths for rustc linking (M7).
#[derive(Debug, Clone)]
pub struct FfiLink {
    pub crate_name: String,
    pub rlib_path: PathBuf,
    /// Shared library with `*_cabi` trampolines for the resident Cranelift JIT.
    pub cdylib_path: PathBuf,
    /// Selected-target runtime dependencies emitted by Cargo.
    pub target_deps_dir: PathBuf,
    /// Host artifacts needed while rustc loads target metadata (notably proc macros).
    pub host_deps_dir: PathBuf,
    /// Path to the built `jet-crypto-helper` binary, present only when the
    /// bridge was built with `needs_crypto` (card c146 — package signing shells
    /// out to this helper for Ed25519 keygen/sign/verify). `None` otherwise.
    pub helper_bin_path: Option<PathBuf>,
    /// U13 (D-JPK-SECRETCRYPTO1): path to the built `jet-secrets-helper`
    /// binary, present only when the bridge was built with `needs_secrets` —
    /// `jetpack secrets set/get/recipients/keygen` shells out to this for the
    /// age-style encrypt/decrypt/keygen operations. `None` otherwise.
    pub secrets_helper_bin_path: Option<PathBuf>,
}

impl FfiLink {
    /// Cargo's dependency search paths, target artifacts first and without duplicates.
    pub fn dependency_dirs(&self) -> impl Iterator<Item = &Path> {
        std::iter::once(self.target_deps_dir.as_path()).chain(
            (self.host_deps_dir != self.target_deps_dir).then_some(self.host_deps_dir.as_path()),
        )
    }
}
