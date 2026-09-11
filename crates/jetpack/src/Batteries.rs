//! The first-party Jet library battery catalog and its source materializer.
//!
//! Battery sources are compiled into the jetpack binary by `build.rs`. At
//! realization time they are copied to a deterministic source-cache directory,
//! so the ordinary `CoreProvider` path can discover the package manifest,
//! fingerprint its source tree, and record the same lock/store identity as any
//! other core package.

use crate::Provider;
use crate::SHA256;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

pub(crate) const SOURCE_NAME: &str = "batteries";
pub(crate) const UPSTREAM: &str = "embedded:batteries";

#[derive(Debug, Clone, Copy)]
pub(crate) struct EmbeddedFile {
    pub(crate) path: &'static str,
    pub(crate) mode: u32,
    pub(crate) bytes: &'static [u8],
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct EmbeddedPackage {
    pub(crate) slug: &'static str,
    pub(crate) files: &'static [EmbeddedFile],
}

include!(concat!(env!("OUT_DIR"), "/batteries_registry.rs"));

pub(crate) fn materialize_package(store_dir: &Path, slug: &str) -> Result<PathBuf, String> {
    let package = PACKAGES
        .iter()
        .find(|package| package.slug == slug)
        .ok_or_else(|| format!("unknown first-party battery package `{slug}`"))?;
    let key = source_key(package);
    let source_root = store_dir
        .parent()
        .unwrap_or(store_dir)
        .join("sources")
        .join(SOURCE_NAME);
    ensure_real_directory(&source_root)?;
    let destination = source_root.join(format!("{}-{key}", package.slug));

    match std::fs::symlink_metadata(&destination) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(format!(
                    "embedded battery cache entry is not a real directory: {}",
                    destination.display()
                ));
            }
            verify_package_tree(&destination, package)?;
            return Ok(destination);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "could not inspect embedded battery cache {}: {error}",
                destination.display()
            ));
        }
    }

    let staging = Provider::exclusive_temp_dir(&source_root, "battery-source")
        .map_err(|error| format!("could not create embedded battery staging directory: {error}"))?;
    let result = write_package_tree(&staging, package).and_then(|()| {
        std::fs::rename(&staging, &destination).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                format!("destination already exists: {}", destination.display())
            } else {
                format!(
                    "could not publish embedded battery source {}: {error}",
                    destination.display()
                )
            }
        })
    });
    match result {
        Ok(()) => Ok(destination),
        Err(error) if error == format!("destination already exists: {}", destination.display()) => {
            let _ = std::fs::remove_dir_all(&staging);
            verify_package_tree(&destination, package)?;
            Ok(destination)
        }
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(error)
        }
    }
}

fn source_key(package: &EmbeddedPackage) -> String {
    let mut identity = Vec::new();
    identity.extend_from_slice(b"jet-batteries-source-v1");
    append_bytes(&mut identity, package.slug.as_bytes());
    for file in package.files {
        append_bytes(&mut identity, file.path.as_bytes());
        identity.extend_from_slice(&file.mode.to_be_bytes());
        append_bytes(&mut identity, file.bytes);
    }
    SHA256::sha256_hex(&identity)
}

fn append_bytes(target: &mut Vec<u8>, bytes: &[u8]) {
    target.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    target.extend_from_slice(bytes);
}

fn ensure_real_directory(path: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => Err(format!(
            "embedded battery cache path is not a real directory: {}",
            path.display()
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir_all(path).map_err(|error| {
                format!(
                    "could not create embedded battery cache directory {}: {error}",
                    path.display()
                )
            })?;
            let metadata = std::fs::symlink_metadata(path).map_err(|error| {
                format!(
                    "could not inspect embedded battery cache directory {}: {error}",
                    path.display()
                )
            })?;
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                return Err(format!(
                    "embedded battery cache path is not a real directory: {}",
                    path.display()
                ));
            }
            Ok(())
        }
        Err(error) => Err(format!(
            "could not inspect embedded battery cache path {}: {error}",
            path.display()
        )),
    }
}

fn write_package_tree(root: &Path, package: &EmbeddedPackage) -> Result<(), String> {
    for file in package.files {
        let relative = safe_relative_path(file.path)?;
        let destination = root.join(relative);
        let parent = destination.parent().ok_or_else(|| {
            format!("embedded battery file has no parent directory: {}", file.path)
        })?;
        ensure_real_directory(parent)?;
        std::fs::write(&destination, file.bytes).map_err(|error| {
            format!(
                "could not write embedded battery file {}: {error}",
                destination.display()
            )
        })?;
        set_file_mode(&destination, file.mode)?;
    }
    Ok(())
}

fn verify_package_tree(root: &Path, package: &EmbeddedPackage) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    verify_directory(root, Path::new(""), package, &mut seen)?;
    if seen.len() != package.files.len() {
        return Err(format!(
            "embedded battery cache is missing files for `{}`",
            package.slug
        ));
    }
    Ok(())
}


fn verify_directory(
    root: &Path,
    relative: &Path,
    package: &EmbeddedPackage,
    seen: &mut BTreeSet<String>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(root).map_err(|error| {
        format!(
            "could not read embedded battery cache {}: {error}",
            root.display()
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            format!(
                "could not inspect embedded battery cache entry {}: {error}",
                path.display()
            )
        })?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "embedded battery cache contains a symlink: {}",
                path.display()
            ));
        }
        let name = entry.file_name();
        let next = relative.join(&name);
        if metadata.is_dir() {
            verify_directory(&path, &next, package, seen)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(format!(
                "embedded battery cache contains a non-file: {}",
                path.display()
            ));
        }
        let relative_name = next
            .to_str()
            .ok_or_else(|| {
                format!(
                    "embedded battery cache path is not UTF-8: {}",
                    path.display()
                )
            })?
            .replace('\\', "/");
        let file = package
            .files
            .iter()
            .find(|file| file.path == relative_name.as_str())
            .ok_or_else(|| {
                format!(
                    "embedded battery cache contains an unexpected file: {}",
                    path.display()
                )
            })?;
        let bytes = std::fs::read(&path).map_err(|error| {
            format!(
                "could not read embedded battery cache file {}: {error}",
                path.display()
            )
        })?;
        if bytes != file.bytes {
            return Err(format!(
                "embedded battery cache bytes do not match bundled `{}`",
                file.path
            ));
        }
        if file_mode(&metadata) != file.mode {
            return Err(format!(
                "embedded battery cache mode does not match bundled `{}`",
                file.path
            ));
        }
        seen.insert(relative_name.to_string());
    }
    Ok(())
}

fn safe_relative_path(path: &str) -> Result<PathBuf, String> {
    let relative = Path::new(path);
    if path.is_empty() || relative.is_absolute() {
        return Err(format!("embedded battery path is not relative: {path:?}"));
    }
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("embedded battery path is unsafe: {path:?}"));
    }
    Ok(relative.to_path_buf())
}

fn set_file_mode(path: &Path, mode: u32) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(mode);
        std::fs::set_permissions(path, permissions)
            .map_err(|error| format!("could not set embedded battery file mode: {error}"))?;
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
    Ok(())
}

fn file_mode(metadata: &std::fs::Metadata) -> u32 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o7777
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        0o644
    }
}
