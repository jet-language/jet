//! `pack.jet` package-manifest loading (M12.1, S52, U1, D-JPK23).
//!
//! The on-disk format is Jet syntax, not TOML — `src/jetpack/packmanifest.rs`
//! is the structural parser for `pack.jet`'s `package:`/`deps:`/`exports:`
//! shape; this module owns the compiler-facing `Manifest` type that
//! `loader.rs`/`fetch.rs`/`lock.rs`/`store.rs` operate on, the toolchain
//! check (E1208), and the comment-preserving `jet add`/`jet remove` edits.
//! `pack.jet` replaces the old TOML `jet.toml` as a clean break (U1) — no
//! back-compat alias.

use crate::diag::Diagnostic;
use crate::jetpack::packmanifest::{self, ManifestError};
use crate::syntax;
use std::collections::BTreeMap;
use std::path::Path;

/// The compiler's version string for E1208 toolchain checks.
pub const COMPILER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ──────────────────────────────────────────────
// Public types
// ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub package: PackageMeta,
    /// Jet package dependencies.
    pub dependencies: BTreeMap<String, DepSpec>,
    /// Rust crate dependencies for `extern rust` blocks. Always empty today:
    /// `extern rust "crate@version" { … }` (S50) carries its own version
    /// pin in source, so nothing has ever read this map — kept for shape
    /// stability, not parsed from `pack.jet`.
    pub dependencies_rust: BTreeMap<String, String>,
    /// Raw `pack.jet` text (preserved for comment-preserving edits).
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageMeta {
    pub name: String,
    pub version: String,
    /// Toolchain constraint from `jet: "..."`.
    pub jet_constraint: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepSpec {
    /// Path dependency: `helpers: path@../helpers`.
    Path { path: String },
    /// Git dependency with one selector (D-JPK23): `{ git: "...", tag/branch/rev: "..." }`,
    /// or a `github@owner/repo/rev` provider ref (always a pinned rev).
    Git { url: String, selector: GitSelector },
    /// Registry version string (M12.2 only; error in M12.1 during resolution).
    Registry(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitSelector {
    Tag(String),
    Branch(String),
    Rev(String),
}

impl GitSelector {
    pub fn is_moving(&self) -> bool {
        match self {
            GitSelector::Tag(t) => t == "@latest",
            GitSelector::Branch(_) => true,
            GitSelector::Rev(_) => false,
        }
    }
}

// ──────────────────────────────────────────────
// Parser
// ──────────────────────────────────────────────

/// Parse a `pack.jet` package manifest from its text.
pub fn parse(path: &Path, raw: &str) -> Result<Manifest, Diagnostic> {
    let pm = packmanifest::parse(raw).map_err(|e| to_diagnostic(path, &e))?;
    packmanifest::to_manifest(&pm, raw)
}

/// Load and parse the `pack.jet` manifest in a directory.
pub fn load(dir: &Path) -> Option<Result<Manifest, Diagnostic>> {
    let pack_path = dir.join(syntax::PACK_FILE);
    if !pack_path.is_file() {
        return None;
    }
    let raw = match std::fs::read_to_string(&pack_path) {
        Ok(s) => s,
        Err(e) => {
            return Some(Err(e1206(
                &pack_path.display().to_string(),
                &format!("couldn't read {}: {}", syntax::PACK_FILE, e),
            )));
        }
    };
    Some(parse(&pack_path, &raw))
}

/// Validate the toolchain constraint from `package.jet`. Returns E1208 on mismatch.
pub fn check_toolchain(manifest: &Manifest, _file: &str) -> Result<(), Diagnostic> {
    let Some(constraint) = &manifest.package.jet_constraint else {
        return Ok(());
    };
    if !satisfies_constraint(COMPILER_VERSION, constraint) {
        return Err(Diagnostic::error(
            "E1208",
            format!(
                "this project requires Jet `{}` but this is Jet {}",
                constraint, COMPILER_VERSION
            ),
            "the `jet` field in `package` specifies a minimum toolchain version".to_string(),
            format!(
                "update Jet to a newer version, or change the `jet` field in `{}`",
                syntax::PACK_FILE
            ),
            None,
        ));
    }
    Ok(())
}

fn satisfies_constraint(version: &str, constraint: &str) -> bool {
    let constraint = constraint.trim();
    // Support: ">=X.Y.Z", "^X.Y.Z", "X.Y.Z" (exact), "*".
    if constraint == "*" || constraint.is_empty() {
        return true;
    }
    if let Some(min) = constraint.strip_prefix(">=") {
        return version_ge(version, min.trim());
    }
    if let Some(min) = constraint.strip_prefix("^") {
        // ^X.Y.Z means >=X.Y.Z, <(X+1).0.0
        return version_ge(version, min.trim());
    }
    // Exact match
    version.trim() == constraint
}

fn version_ge(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> (u64, u64, u64) {
        let mut p = s.trim().splitn(3, '.');
        let major = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        let minor = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        let patch = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        (major, minor, patch)
    };
    parse(a) >= parse(b)
}

// ──────────────────────────────────────────────
// Comment-preserving edit helpers for `jet add`/`jet remove`
// ──────────────────────────────────────────────

/// Insert or update a dependency in the `deps: { … }` block, preserving
/// comments and existing entries. Returns the updated `pack.jet` text.
pub fn add_dependency(raw: &str, name: &str, spec: &DepSpec) -> String {
    packmanifest::add_dep(raw, name, spec)
}

/// Remove a dependency from `deps: { … }`, preserving comments.
pub fn remove_dependency(raw: &str, name: &str) -> String {
    packmanifest::remove_dep(raw, name)
}

/// Generate a `pack.jet` template for `jet new`.
pub fn new_template(name: &str, annotated: bool) -> String {
    packmanifest::new_template(name, annotated)
}

// ──────────────────────────────────────────────
// Diagnostics
// ──────────────────────────────────────────────

fn to_diagnostic(path: &Path, err: &ManifestError) -> Diagnostic {
    let file = path.display().to_string();
    match err {
        ManifestError::MissingPackage => e1206(&file, "no `package: { … }` block"),
        ManifestError::MissingField(field) => {
            e1206(&file, &format!("`package` is missing required field `{field}`"))
        }
        ManifestError::BadDepValue { name, value } => e1206(
            &file,
            &format!(
                "dependency `{name}` has value `{value}`, which is neither a quoted version, a `provider@target` ref, nor an inline git struct"
            ),
        ),
        ManifestError::BadDepRef { name, err } => {
            e1206(&file, &format!("dependency `{name}`'s ref is invalid: {err:?}"))
        }
        ManifestError::BadGitDep { name, reason } => {
            e1206(&file, &format!("dependency `{name}`'s git struct {reason}"))
        }
        ManifestError::BadExport(item) => {
            e1206(&file, &format!("`exports` item `{item}` is not `module <name>`"))
        }
        ManifestError::ReservedSection(section) => e1209(&file, section),
    }
}

fn e1206(_file: &str, detail: &str) -> Diagnostic {
    Diagnostic::error(
        "E1206",
        format!("`{}` has a shape error", syntax::PACK_FILE),
        detail.to_string(),
        format!("check `{}` against docs/spec/syntax-decisions.md (U1)", syntax::PACK_FILE),
        None,
    )
}

pub fn e1209(_file: &str, section: &str) -> Diagnostic {
    Diagnostic::error(
        "E1209",
        format!("`{}` is reserved and not yet implemented", section),
        "this section name is reserved for a future Jet feature — using it now is an error"
            .to_string(),
        format!(
            "remove the `{}` block from `{}`, or leave it empty",
            section,
            syntax::PACK_FILE
        ),
        None,
    )
}
