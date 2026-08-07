//! `pkg.jet` package-manifest loading (M12.1, S52, U1/U10, D-JPK23).
//!
//! The on-disk format is Jet syntax, not TOML — `src/jetpack/packmanifest.rs`
//! is the structural parser for `pkg.jet`'s `payload:`/`deps:`/`exports:`
//! shape; this module owns the compiler-facing `Manifest` type that
//! `loader.rs`/`fetch.rs`/`lock.rs`/`store.rs` operate on, the toolchain
//! check (E1208), and the comment-preserving `jet add`/`jet remove` edits.
//! `pkg.jet` replaces the old TOML `jet.toml` as a clean break (U1/U10) —
//! no back-compat alias.

use crate::Diagnostics::{Diagnostic, Span};
use crate::Package::{self, PackageParseError};
use crate::Syntax;
use std::collections::BTreeMap;
use std::path::Path;

/// The compiler's version string for E1208 toolchain checks.
pub const COMPILER_VERSION: &str = env!("CARGO_PKG_VERSION");

// ──────────────────────────────────────────────
// Editions & release policy (E2-M2, D-REL1…D-REL5)
// ──────────────────────────────────────────────

/// The editions this toolchain understands (D-REL3). An edition opts a project
/// into a specific era of Jet syntax (docs/spec/release-policy.md). The list is
/// ordered oldest→newest; the last entry is the newest stable edition, used by
/// single-file `jet run file.jet` which carries no edition marker (E2-V4).
const LATEST_EDITION: &str = "2027";
pub const SUPPORTED_EDITIONS: &[&str] = &["2026", "2027", "2028"];

/// Parse an edition label like `"2027"` into its year. Unknown labels sort before
/// every supported edition so comparisons fail closed.
pub fn edition_year(edition: &str) -> u16 {
    edition.trim().parse().unwrap_or(0)
}

/// `true` when `edition` is at least as new as `baseline` under numeric ordering.
pub fn edition_at_least(edition: &str, baseline: &str) -> bool {
    edition_year(edition) >= edition_year(baseline)
}

/// Resolve a manifest's effective edition, defaulting to the newest stable one.
pub fn effective_edition(manifest: &Manifest) -> String {
    manifest
        .package
        .edition
        .clone()
        .filter(|e| !e.trim().is_empty())
        .unwrap_or_else(|| latest_edition().to_string())
}

/// The newest stable edition this toolchain ships. Single-file programs and a
/// manifest with no `edition:` field use this.
pub fn latest_edition() -> &'static str {
    LATEST_EDITION
}

/// The registry-protocol version this toolchain speaks (D-REL1: normal SemVer;
/// the registry index format is versioned independently of the compiler).
pub const REGISTRY_COMPAT: &str = "1";

/// `true` if `edition` is one this toolchain supports.
pub fn edition_is_supported(edition: &str) -> bool {
    SUPPORTED_EDITIONS.contains(&edition.trim())
}

/// Validate the `edition:` field from `package.jet` (D-REL3). A manifest that
/// asks for an edition this toolchain doesn't ship is E2001. A manifest with no
/// `edition:` field is fine — it tracks the toolchain's newest stable edition.
pub fn check_edition_support(manifest: &Manifest, _file: &str) -> Result<(), Diagnostic> {
    let Some(edition) = &manifest.package.edition else {
        return Ok(());
    };
    if edition_is_supported(edition) {
        return Ok(());
    }
    Err(e2001(edition))
}

/// E2001 — the manifest requests an edition this toolchain can't provide.
pub fn e2001(requested: &str) -> Diagnostic {
    Diagnostic::error(
        "E2001",
        "this package needs a newer Jet".to_string(),
        format!(
            "editions opt a project into a specific era of Jet syntax. A newer edition can use syntax this compiler does not understand. This toolchain supports editions up to {}, but `{}` asks for `{}`.",
            latest_edition(),
            Syntax::PAYLOAD_FILE,
            requested,
        ),
        format!(
            "upgrade with `{} self upgrade`, or set `{}: \"{}\"` in `{}`.",
            Syntax::BINARY_NAME,
            Syntax::MANIFEST_FIELD_EDITION,
            latest_edition(),
            Syntax::PAYLOAD_FILE,
        ),
        None,
    )
}

/// A deprecated language item: the edition it was deprecated in, its replacement,
/// and the edition it is removed in (the end of its migration window). The
/// registry is honest and currently empty — Jet is pre-1.0, so nothing post-1.0
/// has been deprecated yet. E2002/L2001 read from this table, so they become
/// reachable the moment the first real deprecation is registered, without
/// touching the diagnostic plumbing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deprecation {
    /// The deprecated item as the user writes it (a keyword, sigil, or name).
    pub item: &'static str,
    /// The edition in which the item became deprecated.
    pub since_edition: &'static str,
    /// What to use instead (named in the diagnostic).
    pub replacement: &'static str,
    /// The edition in which the item stops compiling (the migration window's end).
    pub removed_in_edition: &'static str,
}

/// The deprecation registry (D-REL5). Encoding CBOR forwarding entries ship with
/// card #712 / D-ENC-CBOR-SURFACE1.
pub const DEPRECATIONS: &[Deprecation] = &[
    Deprecation {
        item: "cbor.encode",
        since_edition: "2027",
        replacement: "cbor.to_bytes",
        removed_in_edition: "2028",
    },
    Deprecation {
        item: "cbor.decode",
        since_edition: "2027",
        replacement: "cbor.parse",
        removed_in_edition: "2028",
    },
];

/// Look up a deprecation by the item's spelling.
pub fn lookup_deprecation(item: &str) -> Option<&'static Deprecation> {
    DEPRECATIONS.iter().find(|d| d.item == item)
}

/// E2002 — a deprecated item is used past its migration window (i.e. in an
/// edition at or after `removed_in_edition`). Names the replacement.
pub fn e2002(dep: &Deprecation, span: Option<Span>) -> Diagnostic {
    Diagnostic::error(
        "E2002",
        format!("`{}` was removed in edition {}", dep.item, dep.removed_in_edition),
        format!(
            "`{}` was deprecated in edition {} and no longer exists in this edition; it has reached the end of its migration window.",
            dep.item, dep.since_edition,
        ),
        format!(
            "use `{}` instead, or run `{} fix` to migrate automatically.",
            dep.replacement,
            Syntax::BINARY_NAME,
        ),
        span,
    )
}

/// L2001 — a lint: an item is deprecated in this edition but still compiles
/// during its migration window. Suggests `jet fix`.
pub fn l2001(dep: &Deprecation, span: Option<Span>) -> Diagnostic {
    Diagnostic::lint(
        "L2001",
        format!("`{}` is deprecated", dep.item),
        format!(
            "`{}` was deprecated in edition {} and will be removed in edition {}; it still works for now but should be migrated.",
            dep.item, dep.since_edition, dep.removed_in_edition,
        ),
        format!(
            "use `{}` instead, or run `{} fix` to migrate automatically.",
            dep.replacement,
            Syntax::BINARY_NAME,
        ),
        span,
    )
}

/// The `jet --version` banner (E2-D1). Deterministic and golden-testable: it
/// states the compiler SemVer, the supported epoch/edition range, the newest
/// stable edition, and the registry-protocol compatibility.
pub fn version_banner() -> String {
    let editions = SUPPORTED_EDITIONS.join(", ");
    format!(
        "{lang} {ver}\nsupported editions: {editions} (newest: {latest})\nregistry protocol: v{registry}\n",
        lang = Syntax::LANG_NAME,
        ver = COMPILER_VERSION,
        editions = editions,
        latest = latest_edition(),
        registry = REGISTRY_COMPAT,
    )
}

#[cfg(test)]
mod tests {
    use super::{
        edition_is_supported, latest_edition, version_banner, SUPPORTED_EDITIONS,
    };

    #[test]
    fn latest_edition_is_last_supported_and_drives_banner() {
        assert_eq!(SUPPORTED_EDITIONS.last().copied(), Some(latest_edition()));
        assert!(edition_is_supported(latest_edition()));
        assert!(version_banner().contains(&format!("newest: {}", latest_edition())));
    }
}

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
    /// stability, not parsed from `pkg.jet`.
    pub dependencies_rust: BTreeMap<String, String>,
    /// Raw `pkg.jet` text (preserved for comment-preserving edits).
    pub raw: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageMeta {
    pub name: String,
    pub version: String,
    /// Project compatibility marker from `edition: "..."` (D-REL3). A toolchain
    /// supports a fixed set of editions (`SUPPORTED_EDITIONS`); a future edition
    /// is E2001. `None` means "use the toolchain's newest stable edition".
    pub edition: Option<String>,
    /// Toolchain constraint from `jet: "..."`.
    pub jet_constraint: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
    /// D-RINGLAYER1=A: optional runtime ceiling from `runtime:` in `payload`.
    pub layer: Option<crate::Syntax::RuntimeLayer>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepSpec {
    /// Path dependency: `helpers: ../helpers`.
    Path { path: String },
    /// Git dependency with one selector (D-JPK23): `{ git: "...", tag/branch/rev: "..." }`,
    /// or an `owner/repo/rev@github` provider ref (always a pinned rev).
    Git { url: String, selector: GitSelector },
    /// Registry version string. `jet fetch` materializes its verified source
    /// tree before the compiler builds module search paths.
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

/// Parse a `package.jet` package manifest from its text (D-CONF-PLANE1: the
/// one role-typed parser, no legacy-vocabulary fallback).
pub fn parse(path: &Path, raw: &str) -> Result<Manifest, Diagnostic> {
    // `parse_uncomposed`, not `parse`: the compile path's toolchain/edition/
    // dependency checks operate on this package's own declared facts and
    // never load file-backed `Config` contributions (that composition, and
    // the `defaults:`/`outputs:` validation it enables, is a tooling/Canvas
    // concern via `PackageFacts::load`). Validating `defaults:` here against
    // an uncomposed read would reject any package whose default output is
    // declared in a `configs:`-referenced file.
    let facts = Package::PackageFacts::parse_uncomposed(raw, path.display().to_string())
        .map_err(|e| to_diagnostic(path, &e))?;
    Package::to_manifest(&facts, raw)
}

/// The path to the package manifest in a project dir. The canonical
/// `package.jet` wins; `pkg.jet` is accepted only as migration-era input.
pub fn manifest_path_in(dir: &Path) -> std::path::PathBuf {
    let canonical = dir.join(Syntax::PACKAGE_FILE);
    if canonical.is_file() {
        canonical
    } else {
        dir.join(Syntax::PAYLOAD_FILE)
    }
}

/// Whether both manifest spellings exist in this directory — ambiguous.
pub fn has_both_manifests(dir: &Path) -> bool {
    dir.join(Syntax::PACKAGE_FILE).is_file() && dir.join(Syntax::PAYLOAD_FILE).is_file()
}

/// Load and parse the nearest package manifest in a directory.
pub fn load(dir: &Path) -> Option<Result<Manifest, Diagnostic>> {
    if has_both_manifests(dir) {
        return Some(Err(to_diagnostic(
            &dir.join(Syntax::PACKAGE_FILE),
            &PackageParseError::Composition(format!(
                "both `{}` and migration-era `{}` exist; keep one Package root",
                Syntax::PACKAGE_FILE,
                Syntax::PAYLOAD_FILE,
            )),
        )));
    }
    let pack_path = manifest_path_in(dir);
    if !pack_path.is_file() {
        return None;
    }
    let raw = match std::fs::read_to_string(&pack_path) {
        Ok(s) => s,
        Err(e) => {
            return Some(Err(e1206(
                &pack_path.display().to_string(),
                &format!("couldn't read {}: {}", Syntax::PAYLOAD_FILE, e),
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
    // D-JPK-TOOLCHAIN1=A (#179): the `jet:` field is a toolchain *pin* — a
    // channel ref (`0.4`, `0.4.2`, `main`). A channel-form value is owned by
    // the version-dispatch path (`Jetpack::JetPin`: E1249 for a malformed pin,
    // realize + re-exec or E1251 for a mismatch), NOT by this compatibility
    // gate. Only an explicit range/operator constraint (`>=x`, `^x`, `*`) keeps
    // the legacy E1208 "minimum toolchain" semantics.
    if !is_range_constraint(constraint) {
        return Ok(());
    }
    if !satisfies_constraint(COMPILER_VERSION, constraint) {
        return Err(Diagnostic::error(
            "E1208",
            format!(
                "this project requires Jet `{}` but this is Jet {}",
                constraint, COMPILER_VERSION
            ),
            "the top-level `jet` field specifies a minimum toolchain version".to_string(),
            format!(
                "update Jet to a newer version, or change the `jet` field in `{}`",
                Syntax::PACKAGE_FILE
            ),
            None,
        ));
    }
    Ok(())
}

/// A `jet:` value is a legacy range constraint (E1208 gate) when it leads with
/// a comparison/wildcard operator; otherwise it is a D-JPK-TOOLCHAIN1 channel
/// pin, handled by the version-dispatch path.
fn is_range_constraint(value: &str) -> bool {
    matches!(
        value.trim().bytes().next(),
        Some(b'>') | Some(b'<') | Some(b'=') | Some(b'^') | Some(b'~') | Some(b'*')
    )
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
/// comments and existing entries. Returns the updated `pkg.jet` text.
pub fn add_dependency(raw: &str, name: &str, spec: &DepSpec) -> String {
    Package::add_dep(raw, name, spec)
}

/// Remove a dependency from `deps: { … }`, preserving comments.
pub fn remove_dependency(raw: &str, name: &str) -> String {
    Package::remove_dep(raw, name)
}

/// Generate a `pkg.jet` template for `jet new`.
pub fn new_template(name: &str, annotated: bool) -> String {
    Package::new_template(name, annotated)
}

// ──────────────────────────────────────────────
// Diagnostics
// ──────────────────────────────────────────────

fn to_diagnostic(path: &Path, err: &PackageParseError) -> Diagnostic {
    let file = path.display().to_string();
    match err {
        PackageParseError::UnknownField(field) => e1206_unknown_field(field),
        PackageParseError::MissingName => {
            e1206(&file, &format!("`{}` needs a `name` field", Syntax::PACKAGE_FILE))
        }
        PackageParseError::MissingRecord(field) => {
            e1206(&file, &format!("`{field}:` needs a record value"))
        }
        PackageParseError::MalformedField(value) => {
            e1206(&file, &format!("malformed field `{value}`"))
        }
        PackageParseError::UnknownOutputKind(kind) => {
            e1206(&file, &format!("unknown Output kind `{kind}`"))
        }
        PackageParseError::InvalidValue { field, value } => {
            e1206(&file, &format!("invalid value for `{field}`: `{value}`"))
        }
        PackageParseError::ConfigMembers => {
            e1206(&file, "a Config file cannot declare `members`")
        }
        PackageParseError::Composition(detail) => e1206(&file, detail),
        PackageParseError::BadTarget { name, value, reserved: true } => e1210(
            &file,
            &format!(
                "package `{name}` lists target `{value}`, which has no backend yet (reserved for a future increment)"
            ),
        ),
        PackageParseError::BadTarget { name, value, reserved: false } => e1210(
            &file,
            &format!("package `{name}` lists target `{value}`, which is not a known target"),
        ),
        PackageParseError::KindFieldRemoved { name } => e1211(
            &file,
            &format!(
                "package `{name}` uses `{}:`, which was removed",
                Syntax::PACKAGE_FIELD_KIND_REMOVED,
            ),
        ),
        PackageParseError::BadTargetField { name, detail } => {
            e1216(&file, &format!("package `{name}`: {detail}"))
        }
        PackageParseError::ReservedSection(section) => e1209(&file, section),
        PackageParseError::BadEffectsBlock(detail) => e1221(&file, detail),
        PackageParseError::BadMemoryPolicy { detail } => {
            e1206(&file, &format!("memory policy is malformed: {detail}"))
        }
        PackageParseError::BadAutoDerivePolicy { detail } => {
            e1206(&file, &format!("`policy.auto_derive` is malformed: {detail}"))
        }
    }
}

/// D-CONF-PLANE1/D-CONF-NAME1: an unknown top-level manifest field — most
/// often a retired spelling (`payload:`, `identity:`, a `packages:` typo).
/// This is the one diagnostic every wrong-vocabulary `package.jet` hits, now
/// that there is one parser and one field list.
fn e1206_unknown_field(field: &str) -> Diagnostic {
    Diagnostic::error(
        "E1206",
        format!("unknown field `{field}:` in `{}`", Syntax::PACKAGE_FILE),
        format!(
            "`{field}:` is not part of the Package vocabulary. Identity is bare `name:` and `version:` at the top level (D-CONF-NAME1) — there is no `identity:` or `payload:` wrapper."
        ),
        "use `name:`, `version:`, `deps:`, `outputs:`, `settings:`, `build:`, `policy:`, or `members:`; see docs/spec/syntax-decisions.md D-CONF-NAME1".to_string(),
        None,
    )
}

/// D-EFFBUDGET1 (E1221): a malformed `effects:`/`grants:` block — an unknown
/// field, a non-list value, or an effect name outside the closed D-EFF4
/// vocabulary.
fn e1221(_file: &str, detail: &str) -> Diagnostic {
    Diagnostic::error(
        "E1221",
        format!(
            "`{}` has a malformed `{}`/`{}` block",
            Syntax::PAYLOAD_FILE,
            Syntax::MANIFEST_BLOCK_EFFECTS,
            Syntax::MANIFEST_BLOCK_GRANTS,
        ),
        detail.to_string(),
        format!(
            "`{}: {{ allow: […], deny: […] }}` and `{}: {{ \"dep\": […] }}` only take effect names from the ten-effect vocabulary",
            Syntax::MANIFEST_BLOCK_EFFECTS,
            Syntax::MANIFEST_BLOCK_GRANTS,
        ),
        None,
    )
}

fn e1206(_file: &str, detail: &str) -> Diagnostic {
    Diagnostic::error(
        "E1206",
        format!("`{}` has a shape error", Syntax::PAYLOAD_FILE),
        detail.to_string(),
        format!(
            "check `{}` against docs/spec/syntax-decisions.md (U1)",
            Syntax::PAYLOAD_FILE
        ),
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
            Syntax::PAYLOAD_FILE
        ),
        None,
    )
}

fn e1210(_file: &str, detail: &str) -> Diagnostic {
    Diagnostic::error(
        "E1210",
        format!("`{}` lists an unknown target", Syntax::PAYLOAD_FILE),
        detail.to_string(),
        format!(
            "use a shipped target: `{}`, `{}`, `{}`, `{}`, `{}`, or `{}`",
            Syntax::TARGET_LIBRARY,
            Syntax::TARGET_EXECUTABLE,
            Syntax::TARGET_TEST,
            Syntax::TARGET_EXAMPLE,
            Syntax::TARGET_BENCHMARK,
            Syntax::TARGET_PLUGIN,
        ),
        None,
    )
}

fn e1211(_file: &str, detail: &str) -> Diagnostic {
    Diagnostic::error(
        "E1211",
        format!("`{}` uses the removed `kind:` field", Syntax::PAYLOAD_FILE),
        detail.to_string(),
        format!(
            "write `{}: [{}]` (or `[{}]`) instead",
            Syntax::PACKAGE_FIELD_TARGETS,
            Syntax::TARGET_EXECUTABLE,
            Syntax::TARGET_LIBRARY,
        ),
        None,
    )
}

fn e1216(_file: &str, detail: &str) -> Diagnostic {
    Diagnostic::error(
        "E1216",
        format!("`{}` has an invalid target field", Syntax::PAYLOAD_FILE),
        detail.to_string(),
        format!(
            "a target block accepts `{}: \"…\"` and `{}: \"…\"`",
            Syntax::TARGET_FIELD_ENTRY,
            Syntax::TARGET_FIELD_NAME,
        ),
        None,
    )
}

pub fn e1212(_file: &str, name: &str) -> Diagnostic {
    Diagnostic::error(
        "E1212",
        format!(
            "`{}` declares package `{name}` but no `module {name}` was found",
            Syntax::PAYLOAD_FILE
        ),
        format!(
            "each `packages:` name must correspond to a `module <name> {{ … }}` declaration in a `.{}` file in the source tree",
            Syntax::FILE_EXT
        ),
        format!(
            "add a `.{}` file containing `module {name} {{ … }}`, or remove `{name}` from `packages:` in `{}`",
            Syntax::FILE_EXT,
            Syntax::PAYLOAD_FILE,
        ),
        None,
    )
}

pub fn e1213(_file: &str, name: &str, paths: &[std::path::PathBuf]) -> Diagnostic {
    let list = paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    Diagnostic::error(
        "E1213",
        format!(
            "`{}` declares package `{name}` but `module {name}` is ambiguous",
            Syntax::PAYLOAD_FILE
        ),
        format!("`module {name}` was found in multiple files: {list}; each package name must map to exactly one module"),
        format!("rename one of the conflicting `module {name}` declarations so each package has a unique name"),
        None,
    )
}

/// D-BUILDPROFILE1: emit E1219 when the user passes `--profile=<name>` but
/// `name` is not a blessed default (`release`/`debug`) or defined in `pkg.jet`'s
/// `build { }` block. `defined` is the sorted list of profiles the user did define.
/// E1258 (D-PLUGIN1=B, c81): a `target: plugin` package's own code uses an
/// effect — plugins are deny-by-default (the wasmtime host registers zero
/// host imports), so any effect would fail to instantiate at load time.
pub fn e1258(effects: &str) -> Diagnostic {
    Diagnostic::error(
        "E1258",
        "a plugin can't use any effect".to_string(),
        format!(
            "this package builds as `target: plugin` (D-PLUGIN1=B) — it uses: {effects}. Plugins run fully sandboxed with zero host capabilities; there is no gate or grant to widen this (I1 — the sandbox is the safety boundary, not an opt-in)."
        ),
        "remove the effectful call, or move it out of the plugin into the host program that loads it".to_string(),
        None,
    )
}

/// E1259 (D-DEP-WASM1=A, c81): the plugin's wasm32 Component Model build
/// failed — missing toolchain (`wasm-tools`) or a rejected module. Never a raw
/// process crash reaching the user (I2): named tool, named failure.
pub fn e1259(detail: &str) -> Diagnostic {
    Diagnostic::error(
        "E1259",
        "couldn't build the plugin's WASM Component".to_string(),
        detail.to_string(),
        "make sure `rustc` supports `--target wasm32-unknown-unknown` and `wasm-tools` is on PATH (both ship in the project's `nix develop` shell)".to_string(),
        None,
    )
}

pub fn e1219(name: &str, defined: &[String]) -> Diagnostic {
    let note = if defined.is_empty() {
        "no profiles are defined in the `build { }` block of `pkg.jet`".to_string()
    } else {
        format!("defined profiles: {}", defined.join(", "))
    };
    Diagnostic::error(
        "E1219",
        format!("unknown build profile `{name}`"),
        note,
        format!(
            "use `--release` for `--profile={}`, `--profile={}` for a debug build, `--profile={}` for CI, or add `{name}: Build.{{ optimize: full }}` to the `build {{ }}` block in `{}`",
            Syntax::BUILD_PROFILE_RELEASE,
            Syntax::BUILD_PROFILE_DEBUG,
            Syntax::BUILD_PROFILE_CI,
            Syntax::PAYLOAD_FILE,
        ),
        None,
    )
}
