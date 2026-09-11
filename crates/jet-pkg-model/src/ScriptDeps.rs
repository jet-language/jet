//! U11 (D-JPK-SCRIPTDEP1=A): inline script dependencies — a manifest-less
//! `.jet` script may open with `use pkg#version;` instead of shipping a
//! `package.jet`. `jet run` resolves + locks by file-content hash, `jet fetch --lock`
//! writes a `<script>.lock` sidecar, and `jet init` lifts the inline refs
//! into a generated `package.jet`. See
//! docs/spec/reference/jetpack-epoch5.md.
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
use crate::SHA256::{sha256_hex, try_tree_hash, TreeHashError};
use std::fs;
use std::path::{Path, PathBuf};

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
    let parts: Vec<&str> = selector.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
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

/// Why an inline dep didn't resolve (E1253's `{reason}` half).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unresolved {
    /// No local source has ever heard of `name`.
    UnknownPackage,
    /// `name` is known locally, but no version satisfies the selector.
    NoMatch,
    /// A matching source exists, but its contents cannot be hashed safely.
    InvalidTree(TreeHashError),
}

/// Resolve one inline dep against the script's local `.jet/inline-deps/`
/// cache, then (if set) `JET_INLINE_DEPS_FIXTURES`. It hashes the selected
/// source only after pure directory lookup; no network or code execution is
/// involved, exactly like reading an already-realized hangar entry. A registry
/// dependency is not silently substituted for this local source cache; it must
/// be lifted into a manifest and fetched through the package workflow first.
pub fn resolve(dep: &InlineDep, script_dir: &Path) -> Result<Resolved, Unresolved> {
    let mut roots = vec![script_dir.join(".jet").join("inline-deps")];
    if let Ok(fixtures) = std::env::var("JET_INLINE_DEPS_FIXTURES") {
        roots.push(PathBuf::from(fixtures));
    }

    let mut saw_package = false;
    for root in &roots {
        let Some(candidates) = list_versions(root, &dep.name) else {
            continue;
        };
        saw_package = true;
        if let Some((version, dir)) = best_match(&dep.selector, &candidates) {
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
    }
    Err(if saw_package {
        Unresolved::NoMatch
    } else {
        Unresolved::UnknownPackage
    })
}

/// Every version directory under `<root>/<name>/`, or `None` if `<name>`
/// itself doesn't exist under `root`.
fn list_versions(root: &Path, name: &str) -> Option<Vec<(String, PathBuf)>> {
    let pkg_dir = root.join(name);
    if !pkg_dir.is_dir() {
        return None;
    }
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(&pkg_dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                if let Some(v) = p.file_name().and_then(|n| n.to_str()) {
                    out.push((v.to_string(), p));
                }
            }
        }
    }
    Some(out)
}

/// The highest version whose dotted prefix matches `selector` (`1.4` matches
/// `1.4.2`; an exact `1.4.2` selector matches only that version).
fn best_match(selector: &str, candidates: &[(String, PathBuf)]) -> Option<(String, PathBuf)> {
    let sel_parts: Vec<&str> = selector.split('.').filter(|p| !p.is_empty()).collect();
    let mut matches: Vec<&(String, PathBuf)> = candidates
        .iter()
        .filter(|(v, _)| {
            let vp: Vec<&str> = v.split('.').collect();
            vp.len() >= sel_parts.len() && vp.iter().zip(&sel_parts).all(|(a, b)| a == b)
        })
        .collect();
    matches.sort_by_key(|(v, _)| version_key(v));
    matches.last().map(|&(ref v, ref p)| (v.clone(), p.clone()))
}

fn version_key(v: &str) -> (u64, u64, u64) {
    let mut p = v.split('.');
    let a = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    let b = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    let c = p.next().and_then(|x| x.parse().ok()).unwrap_or(0);
    (a, b, c)
}

// ──────────────────────────────────────────────
// Diagnostics
// ──────────────────────────────────────────────

/// E1253 (D-JPK-SCRIPTDEP1=A): an inline `use pkg#version;` ref that can't be
/// resolved — unknown package, unreachable or unsafe source tree, or no version
/// satisfies the selector.
pub fn e1253(dep: &InlineDep, reason: &Unresolved) -> Diagnostic {
    let (why, fix) = match reason {
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
    let bytes = fs::read(path)?;
    Ok(format!("sha256-{}", sha256_hex(&bytes)))
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
