//! Nix-style package store (M12.1, D-PM1/5).
//!
//! Store layout: `~/.jet/store/<name>-<version>-<fingerprint>/`
//! Full plan fingerprint is the path suffix. Lookups use the lockfile,
//! not dirname parsing. Hardlinks into project `.jet-build/deps/` on
//! same device; falls back to copy cross-device. Append-only; `jet gc`
//! removes unreferenced entries (stub in M12.1).

use crate::diag::Diagnostic;
use crate::sha256::tree_hash;
use std::fs;
use std::path::{Path, PathBuf};

// ──────────────────────────────────────────────
// Store location
// ──────────────────────────────────────────────

/// Returns `~/.jet/store`.
/// If `JET_STORE_DIR` is set, that directory is used instead (for testing).
pub fn store_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("JET_STORE_DIR") {
        return PathBuf::from(dir);
    }
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".jet").join("store")
}

/// Store path for a package: `~/.jet/store/<name>-<version>-<fingerprint>/`.
pub fn store_path(name: &str, version: &str, fingerprint: &str) -> PathBuf {
    let fp = fingerprint.strip_prefix("sha256-").unwrap_or(fingerprint);
    store_dir().join(format!("{}-{}-{}", name, version, fp))
}

// ──────────────────────────────────────────────
// Install / link into project
// ──────────────────────────────────────────────

/// Ensure a path dep's source is available in the store.
/// For path deps the store entry is the canonical source copy.
/// Returns the store path (creates it if missing).
pub fn ensure_path_dep(
    name: &str,
    version: &str,
    fingerprint: &str,
    source_dir: &Path,
) -> Result<PathBuf, Diagnostic> {
    let dest = store_path(name, version, fingerprint);
    if dest.is_dir() {
        return Ok(dest);
    }
    fs::create_dir_all(&dest).map_err(|e| io_error("creating store entry", &dest, e))?;
    copy_jet_tree(source_dir, &dest).map_err(|e| io_error("copying to store", &dest, e))?;
    Ok(dest)
}

/// Ensure a git dep is stored.
/// `git_dir` is the directory where the revision has been checked out.
pub fn ensure_git_dep(
    name: &str,
    version: &str,
    fingerprint: &str,
    git_dir: &Path,
) -> Result<PathBuf, Diagnostic> {
    ensure_path_dep(name, version, fingerprint, git_dir)
}

/// Link a store entry into a project's local deps dir via hardlinks (or copy).
/// `link_root` is typically `<project>/.jet-build/deps/<name>/`.
pub fn link_into_project(
    store_entry: &Path,
    link_root: &Path,
) -> Result<(), Diagnostic> {
    if link_root.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(link_root).map_err(|e| io_error("creating dep link dir", link_root, e))?;
    link_or_copy_tree(store_entry, link_root)
}

/// Verify the content hash of a store entry matches expected. Returns E1204 on mismatch.
pub fn verify_entry(
    pkg_name: &str,
    store_entry: &Path,
    expected_tree_hash: &str,
) -> Result<(), Diagnostic> {
    if !store_entry.is_dir() {
        return Err(Diagnostic::error(
            "E1204",
            format!("the store entry for `{}` is missing", pkg_name),
            "a package source tree must be present in the store before it can be used".to_string(),
            "run `jet fetch` to re-download the package".to_string(),
            None,
        ));
    }
    if expected_tree_hash.is_empty() {
        return Ok(());
    }
    let actual = tree_hash(store_entry);
    if actual != expected_tree_hash {
        return Err(Diagnostic::error(
            "E1204",
            format!("the store entry for `{}` has been modified", pkg_name),
            "the content hash of the stored source tree doesn't match the fingerprint in jet.lock"
                .to_string(),
            "run `jet fetch` to re-download the package, or run `jet store verify` to check all entries"
                .to_string(),
            None,
        ));
    }
    Ok(())
}

// ──────────────────────────────────────────────
// `jet store verify`
// ──────────────────────────────────────────────

/// Re-verify all store entries against their expected tree hashes.
/// The `entries` map is `(name, store_path, expected_tree_hash)`.
pub fn verify_all(
    entries: &[(&str, &Path, &str)],
) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for (name, path, expected) in entries {
        if let Err(d) = verify_entry(name, path, expected) {
            diags.push(d);
        }
    }
    diags
}

// ──────────────────────────────────────────────
// `jet gc` — remove unreferenced store entries (stub)
// ──────────────────────────────────────────────

/// List all store entries.
pub fn list_entries() -> Vec<PathBuf> {
    let dir = store_dir();
    let Ok(rd) = fs::read_dir(&dir) else { return Vec::new() };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    out.sort();
    out
}

/// Remove store entries whose fingerprints are not in the provided set.
/// The set contains the fingerprint suffix (without `sha256-` prefix) of in-use entries.
pub fn gc(in_use_fingerprints: &std::collections::HashSet<String>) -> Vec<PathBuf> {
    let entries = list_entries();
    let mut removed = Vec::new();
    for entry in entries {
        let dir_name = entry.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // Last `-`-separated segment is the fingerprint.
        let fp = dir_name.rsplitn(2, '-').next().unwrap_or("");
        if !in_use_fingerprints.contains(fp) {
            if fs::remove_dir_all(&entry).is_ok() {
                removed.push(entry);
            }
        }
    }
    removed
}

// ──────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────

fn copy_jet_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    for entry in fs::read_dir(src)?.flatten() {
        let src_path = entry.path();
        let name = src_path.file_name().unwrap_or_default();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') || name_str == "build" || name_str == "target" {
            continue;
        }
        let dst_path = dst.join(name);
        if src_path.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_jet_tree(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn link_or_copy_tree(src: &Path, dst: &Path) -> Result<(), Diagnostic> {
    for entry in fs::read_dir(src).map_err(|e| io_error("reading store entry", src, e))?.flatten() {
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            fs::create_dir_all(&dst_path).map_err(|e| io_error("creating link dir", &dst_path, e))?;
            link_or_copy_tree(&src_path, &dst_path)?;
        } else {
            // Try hardlink first, fall back to copy.
            if fs::hard_link(&src_path, &dst_path).is_err() {
                fs::copy(&src_path, &dst_path).map_err(|e| io_error("copying dep file", &dst_path, e))?;
            }
        }
    }
    Ok(())
}

fn io_error(action: &str, path: &Path, err: std::io::Error) -> Diagnostic {
    Diagnostic::error(
        "E1206",
        format!("I/O error while {} at `{}`", action, path.display()),
        "a filesystem operation failed during package installation".to_string(),
        format!("check permissions and disk space: {}", err),
        None,
    )
}
