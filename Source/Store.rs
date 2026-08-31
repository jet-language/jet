//! Nix-style package store (M12.1, D-PM1/5).
//!
//! Store layout: `~/.jet/store/<name>-<version>-<fingerprint>/`
//! Full plan fingerprint is the path suffix. Lookups use the lockfile,
//! not dirname parsing. Hardlinks into project `.jet-build/deps/` on
//! same device; falls back to copy cross-device. Append-only; `jet clean`
//! removes unreferenced entries (stub in M12.1).

use crate::Diagnostics::Diagnostic;
use crate::Syntax;
use crate::SHA256::{try_tree_hash, TreeHashError};
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

/// A private, immutable-by-construction source snapshot. Callers must keep
/// this value alive while reading the package; the original path is never
/// consulted after the snapshot has been made.
pub struct SourceSnapshot {
    path: PathBuf,
    cleanup_root: PathBuf,
    content_hash: String,
}

impl SourceSnapshot {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }
}

impl Drop for SourceSnapshot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.cleanup_root);
    }
}

/// Copy a source tree into an exclusive temporary directory and hash the
/// copied bytes. The returned hash describes the bytes callers must consume,
/// not a later re-read of the mutable source path.
pub fn snapshot_tree(source_dir: &Path) -> Result<SourceSnapshot, Diagnostic> {
    validate_real_tree_root(source_dir)
        .map_err(|error| io_error("checking package source", source_dir, error))?;
    let root = store_dir().join(".snapshots");
    ensure_directory(&root)
        .map_err(|error| io_error("creating source snapshot root", &root, error))?;
    let cleanup_root = jetpack::Provider::exclusive_temp_dir(&root, "source")
        .map_err(|error| io_error("creating source snapshot", &root, error))?;
    let path = cleanup_root.join("tree");
    if let Err(error) = copy_jet_tree(source_dir, &path) {
        let _ = fs::remove_dir_all(&cleanup_root);
        return Err(io_error("snapshotting package source", source_dir, error));
    }
    let content_hash = match try_tree_hash(&path) {
        Ok(hash) => hash,
        Err(error) => {
            let _ = fs::remove_dir_all(&cleanup_root);
            return Err(io_error(
                "hashing source snapshot",
                &path,
                tree_hash_io_error(error),
            ));
        }
    };
    Ok(SourceSnapshot {
        path,
        cleanup_root,
        content_hash,
    })
}

// ──────────────────────────────────────────────
// Install / link into project
// ──────────────────────────────────────────────

/// Ensure a path dep's source is available in the store.
/// For path deps the store entry is the canonical source copy.
/// Returns `(store_path, content_hash)` — hash of the installed tree (D-CASTORE1=A).
pub fn ensure_path_dep(
    name: &str,
    version: &str,
    fingerprint: &str,
    source_dir: &Path,
) -> Result<(PathBuf, String), Diagnostic> {
    validate_store_component(name).map_err(|reason| store_path_diagnostic(name, &reason))?;
    validate_store_component(version).map_err(|reason| store_path_diagnostic(version, &reason))?;
    let fingerprint = fingerprint.strip_prefix("sha256-").unwrap_or(fingerprint);
    validate_store_component(fingerprint)
        .map_err(|reason| store_path_diagnostic(fingerprint, &reason))?;
    validate_real_tree_root(source_dir)
        .map_err(|e| io_error("checking package source", source_dir, e))?;
    let dest = store_path(name, version, fingerprint);
    validate_store_path(&dest).map_err(|e| io_error("checking store entry", &dest, e))?;
    if is_real_dir(&dest).map_err(|e| io_error("checking store entry", &dest, e))? {
        let hash = try_tree_hash(&dest)
            .map_err(|error| io_error("hashing store entry", &dest, tree_hash_io_error(error)))?;
        return Ok((dest, hash));
    }
    let parent = dest
        .parent()
        .ok_or_else(|| io_error("locating store entry parent", &dest, invalid_path_error()))?;
    ensure_directory(parent).map_err(|e| io_error("creating store entry parent", parent, e))?;
    let staging_root = jetpack::Provider::exclusive_temp_dir(parent, "entry")
        .map_err(|e| io_error("creating store staging entry", parent, e))?;
    let staging = staging_root.join("tree");
    let result = (|| {
        copy_jet_tree(source_dir, &staging).map_err(|e| io_error("copying to store", &dest, e))?;
        let hash = try_tree_hash(&staging)
            .map_err(|error| io_error("hashing store entry", &dest, tree_hash_io_error(error)))?;
        match fs::rename(&staging, &dest) {
            Ok(()) => Ok(hash),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => try_tree_hash(&dest)
                .map_err(|error| {
                    io_error(
                        "hashing concurrent store entry",
                        &dest,
                        tree_hash_io_error(error),
                    )
                }),
            Err(error) => Err(io_error("publishing store entry", &dest, error)),
        }
    })();
    let _ = fs::remove_dir_all(&staging_root);
    let hash = result?;
    Ok((dest, hash))
}

/// D-CASTORE1=A: verify a store entry's content hash against the recorded lock value.
/// Returns E1204 on mismatch (tampered store) or if entry is missing.
pub fn verify_content_hash(
    pkg_name: &str,
    store_entry: &Path,
    expected_content_hash: &str,
) -> Result<(), Diagnostic> {
    if !is_real_dir(store_entry).unwrap_or(false) {
        return Err(Diagnostic::error(
            "E1204",
            format!("the store entry for `{}` is missing", pkg_name),
            "a package source tree must be present in the store".to_string(),
            "run `jet fetch` to re-download the package".to_string(),
            None,
        ));
    }
    if expected_content_hash.is_empty() {
        return Err(missing_content_hash(pkg_name));
    }
    let actual = try_tree_hash(store_entry).map_err(|error| {
        io_error(
            "hashing store entry",
            store_entry,
            tree_hash_io_error(error),
        )
    })?;
    if actual != expected_content_hash {
        return Err(Diagnostic::error(
            "E1204",
            format!("the store entry for `{}` has been modified", pkg_name),
            format!(
                "expected content hash `{}` but got `{}`",
                expected_content_hash, actual
            ),
            "delete the store entry and run `jet fetch` to re-install".to_string(),
            None,
        ));
    }
    Ok(())
}

/// Ensure a git dep is stored.
/// `git_dir` is the directory where the revision has been checked out.
pub fn ensure_git_dep(
    name: &str,
    version: &str,
    fingerprint: &str,
    git_dir: &Path,
) -> Result<(PathBuf, String), Diagnostic> {
    ensure_path_dep(name, version, fingerprint, git_dir)
}

/// Link a store entry into a project's local deps dir via hardlinks (or copy).
/// `link_root` is typically `<project>/.jet-build/deps/<name>/`.
pub fn link_into_project(store_entry: &Path, link_root: &Path) -> Result<(), Diagnostic> {
    if is_real_dir(link_root).map_err(|e| io_error("checking dep link dir", link_root, e))? {
        let expected = try_tree_hash(store_entry).map_err(|error| {
            io_error(
                "hashing store entry",
                store_entry,
                tree_hash_io_error(error),
            )
        })?;
        let actual = try_tree_hash(link_root).map_err(|error| {
            io_error(
                "hashing dependency link",
                link_root,
                tree_hash_io_error(error),
            )
        })?;
        if expected != actual {
            return Err(io_error(
                "checking dependency link",
                link_root,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "existing dependency link does not match its immutable store entry",
                ),
            ));
        }
        return Ok(());
    }
    let expected = try_tree_hash(store_entry).map_err(|error| {
        io_error(
            "hashing store entry",
            store_entry,
            tree_hash_io_error(error),
        )
    })?;
    let parent = link_root
        .parent()
        .ok_or_else(|| io_error("locating dep link parent", link_root, invalid_path_error()))?;
    ensure_directory(parent).map_err(|e| io_error("creating dep link parent", parent, e))?;
    let staging_root = jetpack::Provider::exclusive_temp_dir(parent, "dep-link")
        .map_err(|e| io_error("creating dep link staging dir", parent, e))?;
    let staging = staging_root.join("tree");
    let result = (|| {
        link_or_copy_tree(store_entry, &staging)?;
        let actual = try_tree_hash(&staging).map_err(|error| {
            io_error(
                "hashing staged dependency link",
                &staging,
                tree_hash_io_error(error),
            )
        })?;
        if actual != expected {
            return Err(io_error(
                "checking staged dependency link",
                &staging,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "staged dependency link does not match its immutable store entry",
                ),
            ));
        }
        match fs::rename(&staging, link_root) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let winner = try_tree_hash(link_root).map_err(|hash_error| {
                    io_error(
                        "hashing concurrent dependency link",
                        link_root,
                        tree_hash_io_error(hash_error),
                    )
                })?;
                if winner == expected {
                    Ok(())
                } else {
                    Err(io_error(
                        "publishing dependency link",
                        link_root,
                        std::io::Error::new(
                            std::io::ErrorKind::AlreadyExists,
                            "concurrent dependency link has different contents",
                        ),
                    ))
                }
            }
            Err(error) => Err(io_error("publishing dependency link", link_root, error)),
        }
    })();
    let _ = fs::remove_dir_all(&staging_root);
    result
}

/// Copy an immutable Hangar object into a project without creating hardlinks
/// outside the Hangar CAS. Hangar verification treats external hardlinks as a
/// mutation risk; registry dependencies therefore use this boundary while
/// legacy path/git stores retain `link_into_project`'s inode sharing.
pub fn copy_into_project(store_entry: &Path, project_root: &Path) -> Result<(), Diagnostic> {
    if is_real_dir(project_root).map_err(|e| io_error("checking dep copy dir", project_root, e))? {
        let expected = try_tree_hash(store_entry).map_err(|error| {
            io_error(
                "hashing store entry",
                store_entry,
                tree_hash_io_error(error),
            )
        })?;
        let actual = try_tree_hash(project_root).map_err(|error| {
            io_error("hashing dep copy", project_root, tree_hash_io_error(error))
        })?;
        if expected != actual {
            return Err(io_error(
                "checking dep copy",
                project_root,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "existing dependency copy does not match its immutable store entry",
                ),
            ));
        }
        return Ok(());
    }
    let parent = project_root.parent().ok_or_else(|| {
        io_error(
            "locating dep copy parent",
            project_root,
            invalid_path_error(),
        )
    })?;
    let expected = try_tree_hash(store_entry).map_err(|error| {
        io_error(
            "hashing store entry",
            store_entry,
            tree_hash_io_error(error),
        )
    })?;
    ensure_directory(parent).map_err(|e| io_error("creating dep copy parent", parent, e))?;
    let staging_root = jetpack::Provider::exclusive_temp_dir(parent, "dep-copy")
        .map_err(|e| io_error("creating dep copy staging dir", parent, e))?;
    let staging = staging_root.join("tree");
    let result = (|| {
        copy_jet_tree(store_entry, &staging)
            .map_err(|e| io_error("copying dep tree", &staging, e))?;
        let actual = try_tree_hash(&staging).map_err(|error| {
            io_error(
                "hashing staged dep copy",
                &staging,
                tree_hash_io_error(error),
            )
        })?;
        if actual != expected {
            return Err(io_error(
                "checking staged dep copy",
                &staging,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "staged dependency copy does not match its immutable store entry",
                ),
            ));
        }
        match fs::rename(&staging, project_root) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let winner = try_tree_hash(project_root).map_err(|hash_error| {
                    io_error(
                        "hashing concurrent dep copy",
                        project_root,
                        tree_hash_io_error(hash_error),
                    )
                })?;
                if winner == expected {
                    Ok(())
                } else {
                    Err(io_error(
                        "publishing dep copy",
                        project_root,
                        std::io::Error::new(
                            std::io::ErrorKind::AlreadyExists,
                            "concurrent dependency copy has different contents",
                        ),
                    ))
                }
            }
            Err(error) => Err(io_error("publishing dep copy", project_root, error)),
        }
    })();
    let _ = fs::remove_dir_all(&staging_root);
    result
}

/// Verify the content hash of a store entry matches expected. Returns E1204 on mismatch.
pub fn verify_entry(
    pkg_name: &str,
    store_entry: &Path,
    expected_tree_hash: &str,
) -> Result<(), Diagnostic> {
    if !is_real_dir(store_entry).unwrap_or(false) {
        return Err(Diagnostic::error(
            "E1204",
            format!("the store entry for `{}` is missing", pkg_name),
            "a package source tree must be present in the store before it can be used".to_string(),
            "run `jet fetch` to re-download the package".to_string(),
            None,
        ));
    }
    if expected_tree_hash.is_empty() {
        return Err(missing_content_hash(pkg_name));
    }
    let actual = try_tree_hash(store_entry).map_err(|error| {
        io_error(
            "hashing store entry",
            store_entry,
            tree_hash_io_error(error),
        )
    })?;
    if actual != expected_tree_hash {
        return Err(Diagnostic::error(
            "E1204",
            format!("the store entry for `{}` has been modified", pkg_name),
            format!(
                "the content hash of the stored source tree doesn't match the fingerprint in {}",
                Syntax::UNIFIED_LOCK_FILE
            ),
            "run `jet fetch` to re-download the package, or run `jetpack hangar verify` to check all entries"
                .to_string(),
            None,
        ));
    }
    Ok(())
}

// ──────────────────────────────────────────────
// `jetpack hangar verify`
// ──────────────────────────────────────────────

/// Re-verify all store entries against their expected tree hashes.
/// The `entries` map is `(name, store_path, expected_tree_hash)`.
pub fn verify_all(entries: &[(&str, &Path, &str)]) -> Vec<Diagnostic> {
    let mut diags = Vec::new();
    for (name, path, expected) in entries {
        if let Err(d) = verify_entry(name, path, expected) {
            diags.push(d);
        }
    }
    diags
}

// ──────────────────────────────────────────────
// `jet clean` — remove unreferenced store entries (stub)
// ──────────────────────────────────────────────

/// List all store entries.
pub fn list_entries() -> Vec<PathBuf> {
    let dir = store_dir();
    let Ok(rd) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|name| name.to_str()) != Some(".snapshots"))
        .filter(|p| is_real_dir(p).unwrap_or(false))
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
    let source_metadata = validate_real_tree_root(src)?;
    ensure_directory(dst)?;
    let mut names = fs::read_dir(src)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    names.sort_unstable();
    for name in &names {
        let src_path = src.join(name);
        let metadata = fs::symlink_metadata(&src_path)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!(
                    "refusing symlink in package source tree: {}",
                    src_path.display()
                ),
            ));
        }
        if !metadata.is_dir() && !metadata.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsupported package source entry: {}", src_path.display()),
            ));
        }
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') || name_str == "build" || name_str == "target" {
            continue;
        }
        let dst_path = dst.join(name);
        if metadata.is_dir() {
            ensure_directory(&dst_path)?;
            copy_jet_tree(&src_path, &dst_path)?;
            let after = fs::symlink_metadata(&src_path)?;
            if !same_store_identity(&metadata, &after) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "package source directory changed while copying: {}",
                        src_path.display()
                    ),
                ));
            }
        } else if metadata.is_file() {
            reject_existing_symlink(&dst_path)?;
            copy_regular_file_nofollow(&src_path, &dst_path, &metadata)?;
        }
    }
    let mut after_names = fs::read_dir(src)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    after_names.sort_unstable();
    let after_metadata = fs::symlink_metadata(src)?;
    if !same_store_identity(&source_metadata, &after_metadata) || names != after_names {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("package source changed while copying `{}`", src.display()),
        ));
    }
    Ok(())
}

fn copy_regular_file_nofollow(
    src: &Path,
    dst: &Path,
    expected: &fs::Metadata,
) -> std::io::Result<()> {
    let mut source_options = fs::OpenOptions::new();
    source_options.read(true);
    add_nofollow_flags(&mut source_options);
    let mut source = source_options.open(src)?;
    let opened = source.metadata()?;
    if !opened.is_file() || !same_store_file_identity(expected, &opened) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("package source file changed before copy: {}", src.display()),
        ));
    }
    let mut destination_options = fs::OpenOptions::new();
    destination_options.write(true).create(true).truncate(true);
    add_nofollow_flags(&mut destination_options);
    let mut destination = destination_options.open(dst)?;
    std::io::copy(&mut source, &mut destination)?;
    destination.sync_all()?;
    let after = fs::symlink_metadata(src)?;
    if !same_store_file_identity(expected, &after) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!(
                "package source file changed while copying: {}",
                src.display()
            ),
        ));
    }
    Ok(())
}

fn add_nofollow_flags(options: &mut fs::OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        #[cfg(any(target_os = "linux", target_os = "android"))]
        const O_CLOEXEC: i32 = 0o2000000;
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        const O_CLOEXEC: i32 = 0x01000000;
        #[cfg(any(target_os = "linux", target_os = "android"))]
        const O_NOFOLLOW: i32 = 0o400000;
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        const O_NOFOLLOW: i32 = 0x0100;
        options.custom_flags(O_NOFOLLOW | O_CLOEXEC);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
}

fn same_store_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return left.dev() == right.dev() && left.ino() == right.ino();
    }
    #[cfg(not(unix))]
    {
        left.file_type() == right.file_type() && left.len() == right.len()
    }
}

fn same_store_file_identity(left: &fs::Metadata, right: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        return left.dev() == right.dev()
            && left.ino() == right.ino()
            && left.len() == right.len()
            && left.modified().ok() == right.modified().ok();
    }
    #[cfg(not(unix))]
    {
        left.file_type() == right.file_type()
            && left.len() == right.len()
            && left.modified().ok() == right.modified().ok()
    }
}

fn link_or_copy_tree(src: &Path, dst: &Path) -> Result<(), Diagnostic> {
    validate_real_tree_root(src).map_err(|e| io_error("checking store entry", src, e))?;
    ensure_directory(dst).map_err(|e| io_error("creating link dir", dst, e))?;
    for entry in fs::read_dir(src).map_err(|e| io_error("reading store entry", src, e))? {
        let entry = entry.map_err(|e| io_error("reading store entry", src, e))?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let metadata = fs::symlink_metadata(&src_path)
            .map_err(|e| io_error("checking store entry", &src_path, e))?;
        if metadata.file_type().is_symlink() {
            return Err(io_error(
                "linking store entry",
                &src_path,
                std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "store entries must not contain symlinks",
                ),
            ));
        }
        if metadata.is_dir() {
            ensure_directory(&dst_path).map_err(|e| io_error("creating link dir", &dst_path, e))?;
            link_or_copy_tree(&src_path, &dst_path)?;
        } else if metadata.is_file() {
            reject_existing_symlink(&dst_path)
                .map_err(|e| io_error("checking dep file", &dst_path, e))?;
            // Try hardlink first, fall back to copy.
            if fs::hard_link(&src_path, &dst_path).is_err() {
                copy_regular_file_nofollow(&src_path, &dst_path, &metadata)
                    .map_err(|e| io_error("copying dep file", &dst_path, e))?;
            }
        } else {
            return Err(io_error(
                "linking store entry",
                &src_path,
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "store entries must contain only regular files and directories",
                ),
            ));
        }
    }
    Ok(())
}

fn validate_store_component(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains(['/', '\\', ':'])
        || value.chars().any(char::is_control)
        || !matches!(
            Path::new(value).components().next(),
            Some(std::path::Component::Normal(_))
        )
        || Path::new(value).components().nth(1).is_some()
    {
        return Err("the value must be one safe path component".to_string());
    }
    Ok(())
}

fn store_path_diagnostic(value: &str, reason: &str) -> Diagnostic {
    Diagnostic::error(
        "E1206",
        format!("package store path component `{value}` is not allowed"),
        reason.to_string(),
        "use a package name, version, and fingerprint without path separators".to_string(),
        None,
    )
}

fn validate_real_tree_root(path: &Path) -> std::io::Result<fs::Metadata> {
    let components = path.components().collect::<Vec<_>>();
    if components.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "package tree root is empty",
        ));
    }
    let mut current = PathBuf::new();
    for component in components {
        if component == std::path::Component::ParentDir {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "package tree root contains a parent component",
            ));
        }
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "package tree root must contain only real directories",
            ));
        }
    }
    fs::symlink_metadata(path)
}

fn validate_store_path(path: &Path) -> std::io::Result<()> {
    let root = store_dir();
    match fs::symlink_metadata(&root) {
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "store root must be a real directory",
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "store entry must not be a symlink",
            ));
        }
    }
    if let Ok(root) = fs::canonicalize(&root) {
        if let Ok(candidate) = fs::canonicalize(path) {
            if !candidate.starts_with(&root) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "store entry escapes the store root",
                ));
            }
        }
    }
    Ok(())
}

fn is_real_dir(path: &Path) -> std::io::Result<bool> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    Ok(metadata.is_dir() && !metadata.file_type().is_symlink())
}

fn reject_existing_symlink(path: &Path) -> std::io::Result<()> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "destination must not be a symlink",
            ));
        }
    }
    Ok(())
}

fn ensure_directory(path: &Path) -> std::io::Result<()> {
    let components = path.components().collect::<Vec<_>>();
    if components.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "directory path is empty",
        ));
    }
    let mut current = PathBuf::new();
    for component in components {
        if component == std::path::Component::ParentDir {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "directory path contains a parent component",
            ));
        }
        current.push(component.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "directory must not be a symlink",
                ));
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    "directory path is not a directory",
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(create_error)
                        if create_error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(create_error) => return Err(create_error),
                }
                let metadata = fs::symlink_metadata(&current)?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "directory must not be a symlink or non-directory",
                    ));
                }
            }
            Err(error) => return Err(error),
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

fn tree_hash_io_error(error: TreeHashError) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string())
}

fn invalid_path_error() -> std::io::Error {
    std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        "store entry has no parent",
    )
}

fn missing_content_hash(pkg_name: &str) -> Diagnostic {
    Diagnostic::error(
        "E1204",
        format!("the store entry for `{pkg_name}` has no content hash"),
        "locked package sources must carry a content hash before they are used".to_string(),
        "run `jet fetch` to recreate the lock with verified content hashes".to_string(),
        None,
    )
}

// ──────────────────────────────────────────────
// D-PURE3=B (E2-M16): signed cache + generation tracking
// ──────────────────────────────────────────────

/// Generation log path: `~/.jet/store/generations.log`.
/// Each line: `<generation_number> <timestamp_utc> <store_entry_list_hash>`.
pub fn generations_log_path() -> PathBuf {
    store_dir().join("generations.log")
}

/// One generation record.
#[derive(Debug, Clone)]
pub struct Generation {
    pub number: u64,
    pub timestamp: String,
    pub entry_hash: String,
}

/// Record the current store state as a new generation.
/// Returns the new generation number.
pub fn record_generation() -> u64 {
    let entries = list_entries();
    let mut entry_names: Vec<String> = entries
        .iter()
        .map(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_string()
        })
        .collect();
    entry_names.sort();
    let entry_list = entry_names.join(",");
    let entry_hash = {
        // Simple hash: sha256 of the sorted entry list.
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        entry_list.hash(&mut h);
        format!("{:016x}", h.finish())
    };

    let log_path = generations_log_path();
    // Read existing generations.
    let existing_raw = fs::read_to_string(&log_path).unwrap_or_default();
    let next_gen = existing_raw
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter_map(|n| n.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        + 1;

    // Append the new generation.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());
    let line = format!("{} {} {}\n", next_gen, ts, entry_hash);
    let _ = fs::create_dir_all(store_dir());
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(line.as_bytes())
        });

    next_gen
}

/// Read all recorded generations from the log.
pub fn list_generations() -> Vec<Generation> {
    let log_path = generations_log_path();
    let raw = fs::read_to_string(&log_path).unwrap_or_default();
    raw.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let number = parts.next()?.parse::<u64>().ok()?;
            let timestamp = parts.next()?.to_string();
            let entry_hash = parts.next().unwrap_or("").to_string();
            Some(Generation {
                number,
                timestamp,
                entry_hash,
            })
        })
        .collect()
}

/// Roll back to a prior generation number.
/// In this implementation, rollback writes a new generation entry that
/// records the intent (store entries cannot be erased — the store is
/// append-only; a rollback marks which generation is "current").
/// Returns the rolled-back-to generation number, or an error string.
pub fn rollback_to(gen_number: u64) -> Result<Generation, String> {
    let gens = list_generations();
    let target = gens
        .iter()
        .find(|g| g.number == gen_number)
        .ok_or_else(|| format!("generation {} does not exist", gen_number))?
        .clone();
    // Write a "rollback" marker generation pointing at the target hash.
    let log_path = generations_log_path();
    let next_gen = gens.iter().map(|g| g.number).max().unwrap_or(0) + 1;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string());
    let line = format!("{} {} rollback-to-{}\n", next_gen, ts, gen_number);
    let _ = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(line.as_bytes())
        });
    Ok(target)
}
