//! U11 (D-JPK-SCRIPTDEP1=A): inline script dependencies — a manifest-less
//! `.jet` script may open with `use pkg#version;` instead of shipping a
//! `package.jet`. `jet run` resolves + locks by file-content hash, `jet fetch --lock`
//! writes a `<script>.lock` sidecar, and `jet init` lifts the inline refs
//! into a generated `package.jet`. See
//! Docs/spec/reference/jetpack-epoch5.md.
//!
//! Resolution is intentionally local and offline. Inline dependencies use a
//! committed or explicitly materialized local source tree, not an implicit
//! registry network request:
//!
//!   1. `<script_dir>/.jet/inline-deps/<name>/<version>/` — a committed (or
//!      previously `jet fetch --lock`-populated) local copy. `.jet/` is the existing
//!      managed-folder convention (`.jet/lock`, `.jet/inline-deps`).
//!   2. `JET_INLINE_DEPS_FIXTURES=<dir>` — an offline test/dev override with
//!      the same `<name>/<version>/` shape, checked only when the env var is
//!      set (mirrors Jetpack's own `JETPACK_FIXTURES` test convention).
//!
//! Anything else is E1253 — an honest unresolved dependency, never a fake
//! success (I2/I3).

use crate::Diagnostics::{Diagnostic, Span};
use crate::AST::{ImportDecl, ImportKind, InlineVersion, Program};
use crate::SHA256::{sha256_file_hex, sha256_hex, try_tree_hash, TreeHashError};
use std::fs;
use std::path::{Path, PathBuf};

const INLINE_DEPS_FIXTURES_ENV: &str = "JET_INLINE_DEPS_FIXTURES";

fn framed(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend_from_slice(&(value.len() as u64).to_be_bytes());
    bytes.extend_from_slice(value.as_bytes());
}

fn framed_count(bytes: &mut Vec<u8>, count: usize) {
    bytes.extend_from_slice(&(count as u64).to_be_bytes());
}

fn valid_package_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .bytes()
            .all(|byte| byte != b'/' && byte != b'\\' && byte != 0)
}

fn numeric_version(value: &str) -> Option<Vec<u64>> {
    let parts: Vec<&str> = value.split('.').collect();
    if !(1..=3).contains(&parts.len()) {
        return None;
    }
    parts
        .into_iter()
        .map(|part| {
            if part.is_empty() || (part.len() > 1 && part.starts_with('0')) {
                return None;
            }
            part.parse::<u64>().ok()
        })
        .collect()
}

/// One `use pkg#version;` ref collected from a manifest-less script.
#[derive(Debug, Clone)]
pub struct InlineDep {
    pub name: String,
    pub selector: String,
    pub span: Span,
}

/// Collect every inline dependency ref from a parsed program's top-level
/// imports (only a single-segment `ImportKind::Module` import carries one —
/// see `Parser::inline_version`).
pub fn collect(program: &Program) -> Vec<InlineDep> {
    collect_from_imports(&program.imports)
}

pub fn collect_from_imports(imports: &[ImportDecl]) -> Vec<InlineDep> {
    imports
        .iter()
        .filter_map(|imp| {
            let v: &InlineVersion = imp.inline_version.as_ref()?;
            let ImportKind::Module(name, _) = &imp.kind else {
                return None;
            };
            Some(InlineDep {
                name: name.clone(),
                selector: v.text.clone(),
                span: v.span,
            })
        })
        .collect()
}

/// A selector is "pinned" when it names an exact three-part version
/// (`1.4.2`). Anything looser (`1.4`, `1`, `latest`, `*`) is L0203 — fine to
/// write (rung 0 stays magic), but not reproducible without `jet fetch --lock`.
pub fn is_pinned(selector: &str) -> bool {
    numeric_version(selector).is_some_and(|parts| parts.len() == 3)
}

/// The content-addressed identity carried from an inline reference into a
/// lock, package lift, or build cache. The selector is retained separately
/// from the selected version: changing a loose selector is a semantic change
/// even when it happens to select the same source today.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DependencyIdentity {
    pub name: String,
    pub selector: String,
    pub resolved_version: String,
    pub content_hash: String,
}

/// The facts that identify a prepared script result. Paths and ambient
/// process state are deliberately absent: only recorded source/dependency
/// content plus the selected toolchain and target can authorize reuse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheIdentity {
    pub source_hash: String,
    pub dependencies: Vec<DependencyIdentity>,
    pub toolchain: String,
    pub target: String,
}

impl CacheIdentity {
    pub fn new(
        source_hash: impl Into<String>,
        dependencies: impl IntoIterator<Item = DependencyIdentity>,
        toolchain: impl Into<String>,
        target: impl Into<String>,
    ) -> Self {
        let mut dependencies: Vec<_> = dependencies.into_iter().collect();
        dependencies.sort();
        Self {
            source_hash: source_hash.into(),
            dependencies,
            toolchain: toolchain.into(),
            target: target.into(),
        }
    }

    /// Stable cache key; length-framing keeps distinct fields distinct even
    /// when a name, hash, or toolchain contains delimiters.
    pub fn digest(&self) -> String {
        let mut bytes = Vec::new();
        framed(&mut bytes, "jet-script-cache.v1");
        framed(&mut bytes, &self.source_hash);
        framed_count(&mut bytes, self.dependencies.len());
        for dependency in &self.dependencies {
            framed(&mut bytes, &dependency.name);
            framed(&mut bytes, &dependency.selector);
            framed(&mut bytes, &dependency.resolved_version);
            framed(&mut bytes, &dependency.content_hash);
        }
        framed(&mut bytes, &self.toolchain);
        framed(&mut bytes, &self.target);
        format!("sha256-{}", sha256_hex(&bytes))
    }
}

/// Build the identity for a resolved script without reading or executing any
/// dependency source. Callers can recompute it after a source edit and reject
/// a stale prepared result before execution.
pub fn cache_identity(
    source_hash: &str,
    dependencies: &[Resolved],
    toolchain: &str,
    target: &str,
) -> CacheIdentity {
    CacheIdentity::new(
        source_hash,
        dependencies.iter().map(Resolved::dependency_identity),
        toolchain,
        target,
    )
}

/// A resolved inline dependency: `dir` is the module search root that
/// satisfies it (fed into `Loader::PkgResolution::realized_libs`).
#[derive(Debug, Clone)]
pub struct Resolved {
    pub name: String,
    pub selector: String,
    pub resolved_version: String,
    pub dir: PathBuf,
    pub content_hash: String,
}

impl Resolved {
    /// Return the portable dependency identity. `dir` is intentionally not
    /// part of it: the same prepared source must identify the same way after
    /// a clone moves the workspace.
    pub fn dependency_identity(&self) -> DependencyIdentity {
        DependencyIdentity {
            name: self.name.clone(),
            selector: self.selector.clone(),
            resolved_version: self.resolved_version.clone(),
            content_hash: self.content_hash.clone(),
        }
    }
}


/// Why an inline dep didn't resolve (E1253's `{reason}` half).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unresolved {
    /// The import name would escape the local dependency root.
    InvalidName,
    /// The selector is not the supported dotted numeric form.
    InvalidSelector,
    /// No local source has ever heard of `name`.
    UnknownPackage,
    /// `name` is known locally, but no version satisfies the selector.
    NoMatch,
    /// A matching source exists, but its contents cannot be hashed safely.
    InvalidTree(TreeHashError),
}

/// Resolve one inline dep against the script's local `.jet/inline-deps/`
/// cache, then (if set) `JET_INLINE_DEPS_FIXTURES`. It hashes the selected
pub fn resolve(dep: &InlineDep, script_dir: &Path) -> Result<Resolved, Unresolved> {
    if !valid_package_name(&dep.name) {
        return Err(Unresolved::InvalidName);
    }
    if numeric_version(&dep.selector).is_none() {
        return Err(Unresolved::InvalidSelector);
    }

    let local_root = script_dir.join(".jet").join("inline-deps");
    let fixture_root = std::env::var_os(INLINE_DEPS_FIXTURES_ENV).map(PathBuf::from);

    // The project-local source is authoritative. If the project has a package
    // directory but the requested version is absent, do not keep searching a
    // fixture (or a future registry) and silently change the dependency.
    let roots = std::iter::once(local_root).chain(fixture_root);
    for root in roots {
        let Some(candidates) = list_versions(&root, &dep.name)? else {
            continue;
        };
        let Some((version, dir)) = best_match(&dep.selector, &candidates) else {
            return Err(Unresolved::NoMatch);
        };
        // `try_tree_hash` returns the same `sha256-<hex>`-prefixed string
        // as `LockedPackage::content_hash`/`IndexEntry`, while preserving
        // any hostile-tree failure for the caller.
        let content_hash = try_tree_hash(&dir).map_err(Unresolved::InvalidTree)?;
        return Ok(Resolved {
            name: dep.name.clone(),
            selector: dep.selector.clone(),
            resolved_version: version,
            dir,
            content_hash,
        });
    }
    Err(Unresolved::UnknownPackage)
}

/// Every version directory under `<root>/<name>`, or `None` if `<name>`
/// itself doesn't exist under `root`. Filesystem errors and links fail closed:
/// a missing source is not interchangeable with another source root.
fn list_versions(
    root: &Path,
    name: &str,
) -> Result<Option<Vec<(String, PathBuf)>>, Unresolved> {
    let root_metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Unresolved::InvalidTree(TreeHashError::Io {
                path: root.to_path_buf(),
                detail: error.to_string(),
            }))
        }
    };
    if root_metadata.file_type().is_symlink() {
        return Err(Unresolved::InvalidTree(TreeHashError::Symlink(
            root.to_path_buf(),
        )));
    }
    if !root_metadata.is_dir() {
        return Ok(None);
    }

    let pkg_dir = root.join(name);
    let package_metadata = match fs::symlink_metadata(&pkg_dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Unresolved::InvalidTree(TreeHashError::Io {
                path: pkg_dir,
                detail: error.to_string(),
            }))
        }
    };
    if package_metadata.file_type().is_symlink() {
        return Err(Unresolved::InvalidTree(TreeHashError::Symlink(pkg_dir)));
    }
    if !package_metadata.is_dir() {
        return Ok(None);
    }

    let entries = fs::read_dir(&pkg_dir).map_err(|error| {
        Unresolved::InvalidTree(TreeHashError::Io {
            path: pkg_dir.clone(),
            detail: error.to_string(),
        })
    })?;
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| {
            Unresolved::InvalidTree(TreeHashError::Io {
                path: pkg_dir.clone(),
                detail: error.to_string(),
            })
        })?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            Unresolved::InvalidTree(TreeHashError::Io {
                path: path.clone(),
                detail: error.to_string(),
            })
        })?;
        if metadata.is_dir() || metadata.file_type().is_symlink() {
            if let Some(version) = path.file_name().and_then(|name| name.to_str()) {
                out.push((version.to_string(), path));
            }
        }
    }
    Ok(Some(out))
}

/// The highest version whose dotted prefix matches `selector` (`1.4` matches
/// `1.4.2`; an exact `1.4.2` selector matches only that version).
fn best_match(selector: &str, candidates: &[(String, PathBuf)]) -> Option<(String, PathBuf)> {
    let selector_parts = numeric_version(selector)?;
    let mut matches: Vec<&(String, PathBuf)> = candidates
        .iter()
        .filter(|(version, _)| {
            let Some(version_parts) = numeric_version(version) else {
                return false;
            };
            if selector_parts.len() == 3 {
                version_parts == selector_parts
            } else {
                version_parts.len() == 3
                    && version_parts
                        .iter()
                        .zip(&selector_parts)
                        .all(|(actual, selected)| actual == selected)
            }
        })
        .collect();
    matches.sort_by_key(|(version, _)| version_key(version));
    matches.last().map(|&(ref version, ref path)| (version.clone(), path.clone()))
}

fn version_key(v: &str) -> (u64, u64, u64) {
    let parts = numeric_version(v).unwrap_or_default();
    (
        parts.first().copied().unwrap_or(0),
        parts.get(1).copied().unwrap_or(0),
        parts.get(2).copied().unwrap_or(0),
    )
}


// ──────────────────────────────────────────────
// Diagnostics
// ──────────────────────────────────────────────

/// E1253 (D-JPK-SCRIPTDEP1=A): an inline `use pkg#version;` ref that can't be
/// resolved — unknown package, unreachable or unsafe source tree, or no version
/// satisfies the selector.
pub fn e1253(dep: &InlineDep, reason: &Unresolved) -> Diagnostic {
    let (why, fix) = match reason {
        Unresolved::InvalidName => (
            format!(
                "`{}` is not a safe local package name.",
                dep.name
            ),
            "use a single package name without path separators.".to_string(),
        ),
        Unresolved::InvalidSelector => (
            format!(
                "`{}` is not a supported dotted numeric version selector.",
                dep.selector
            ),
            "use `major`, `major.minor`, or `major.minor.patch`.".to_string(),
        ),
        Unresolved::UnknownPackage => (
            format!(
                "no local source knows a package named `{}` — an inline dependency must resolve from a committed or explicitly materialized local copy.",
                dep.name
            ),
            format!(
                "commit a copy at `.jet/inline-deps/{}/<version>/`, materialize the source locally first, or run `jet init` and depend on `{}` through `package.jet`.",
                dep.name, dep.name
            ),
        ),
        Unresolved::NoMatch => (
            format!(
                "`{}` is available locally, but no version satisfies `#{}`.",
                dep.name, dep.selector
            ),
            format!(
                "commit a matching version under `.jet/inline-deps/{}/`, or loosen the selector to one you have.",
                dep.name
            ),
        ),
        Unresolved::InvalidTree(error) => (
            format!(
                "the local source for `{}` cannot be hashed safely: {error}.",
                dep.name
            ),
            format!(
                "remove the hostile or oversized entry from `.jet/inline-deps/{}/`, then try again.",
                dep.name
            ),
        ),
    };
    Diagnostic::error(
        "E1253",
        format!(
            "inline dependency `{}#{}` didn't resolve",
            dep.name, dep.selector
        ),
        why,
        fix,
        Some(dep.span),
    )
}

/// L0203 (D-JPK-SCRIPTDEP1=A): an inline dep pinned to a loose selector
/// (anything but an exact `major.minor.patch`).
pub fn l0203_unpinned(dep: &InlineDep) -> Diagnostic {
    Diagnostic::from_row(
        "L0203",
        &[
            ("name", dep.name.as_str()),
            ("selector", dep.selector.as_str()),
        ],
        Some(dep.span),
    )
}

/// SHA-256 of a script file's bytes, formatted as `sha256-<hex>` (the same
/// `content_hash` shape as `tree_hash`/`LockedPackage`/`IndexEntry`) — the
/// key `jet fetch --lock`/`jet run` use to detect an edited script (U11 "locks by
/// file-content hash").
pub fn file_hash(path: &Path) -> std::io::Result<String> {
    Ok(format!("sha256-{}", sha256_file_hex(path)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_requires_exact_three_parts() {
        assert!(is_pinned("1.4.2"));
        assert!(!is_pinned("1.4"));
        assert!(!is_pinned("1"));
        assert!(!is_pinned("latest"));
        assert!(!is_pinned("*"));
        assert!(!is_pinned("^1.4.2"));
        assert!(!is_pinned("01.4.2"));
        assert!(!is_pinned("1.4.999999999999999999999999"));
    }

    #[test]
    fn exact_match_does_not_accept_incompatible_or_malformed_versions() {
        let cands = vec![
            ("1.4.2".to_string(), PathBuf::from("exact")),
            ("1.4.3".to_string(), PathBuf::from("newer")),
            ("1.4.2.1".to_string(), PathBuf::from("malformed")),
        ];
        assert_eq!(best_match("1.4.2", &cands).unwrap().0, "1.4.2");
        assert!(best_match("1.4.1", &cands).is_none());
    }

    #[test]
    fn cache_identity_is_order_independent_but_binds_all_facts() {
        let left = Resolved {
            name: "textkit".to_string(),
            selector: "1.4".to_string(),
            resolved_version: "1.4.2".to_string(),
            dir: PathBuf::from("/first/checkout"),
            content_hash: "sha256-source".to_string(),
        };
        let mut right = left.clone();
        right.name = "other".to_string();
        right.dir = PathBuf::from("/second/checkout");

        let first =
            cache_identity("sha256-script", &[left.clone(), right.clone()], "jet-1", "x86_64");
        let reordered =
            cache_identity("sha256-script", &[right.clone(), left.clone()], "jet-1", "x86_64");
        assert_eq!(first.digest(), reordered.digest());

        let changed_source = CacheIdentity::new(
            "sha256-script-edited",
            [left.dependency_identity(), right.dependency_identity()],
            "jet-1",
            "x86_64",
        );
        let changed_toolchain = CacheIdentity::new(
            "sha256-script",
            [left.dependency_identity(), right.dependency_identity()],
            "jet-2",
            "x86_64",
        );
        assert_ne!(first.digest(), changed_source.digest());
        assert_ne!(first.digest(), changed_toolchain.digest());
    }

    #[test]
    fn best_match_prefers_prefix_and_highest() {
        let cands = vec![
            ("1.4.0".to_string(), PathBuf::from("a")),
            ("1.4.2".to_string(), PathBuf::from("b")),
            ("2.0.0".to_string(), PathBuf::from("c")),
        ];
        let (v, _) = best_match("1.4", &cands).unwrap();
        assert_eq!(v, "1.4.2");
        let (v, _) = best_match("1.4.0", &cands).unwrap();
        assert_eq!(v, "1.4.0");
        assert!(best_match("9.9", &cands).is_none());
    }
    fn temp_root(tag: &str) -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "jet-script-deps-{tag}-{}-{stamp}",
            std::process::id()
        ))
    }

    fn dep(name: &str) -> InlineDep {
        InlineDep {
            name: name.to_string(),
            selector: "1.0.0".to_string(),
            span: Span::new(0, 0),
        }
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_recursive_symlink_as_structured_failure() {
        use std::os::unix::fs::symlink;

        let root = temp_root("symlink");
        let package = root.join(".jet/inline-deps/hostile/1.0.0");
        std::fs::create_dir_all(package.join("src")).unwrap();
        std::fs::write(package.join("src/value.jet"), b"stable\n").unwrap();
        symlink(".", package.join("src/loop")).unwrap();

        let dependency = dep("hostile");
        let result = resolve(&dependency, &root);
        let error = result.unwrap_err();
        assert!(matches!(
            &error,
            Unresolved::InvalidTree(TreeHashError::Symlink(path))
                if path.ends_with("src/loop")
        ));
        let diagnostic = e1253(&dependency, &error);
        assert_eq!(diagnostic.code, "E1253");
        assert!(diagnostic.why.contains("symlink"));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_rejects_oversized_recursive_source_as_structured_failure() {
        let root = temp_root("depth");
        let package = root.join(".jet/inline-deps/hostile/1.0.0");
        std::fs::create_dir_all(&package).unwrap();
        let mut current = package.clone();
        for index in 0..=(crate::SHA256::MAX_TREE_DEPTH + 1) {
            current.push(format!("d{index}"));
            std::fs::create_dir(&current).unwrap();
        }
        std::fs::write(current.join("payload.jet"), b"stable\n").unwrap();

        let result = resolve(&dep("hostile"), &root);
        assert!(matches!(
            result,
            Err(Unresolved::InvalidTree(TreeHashError::TooLarge { limit, .. }))
                if limit == crate::SHA256::MAX_TREE_DEPTH as u64
        ));

        std::fs::remove_dir_all(root).unwrap();
    }
}
