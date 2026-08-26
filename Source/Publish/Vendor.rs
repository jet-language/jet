use crate::Diagnostics::Diagnostic;
use crate::Lock::LockFile;
use std::path::{Component, Path, PathBuf};

// ──────────────────────────────────────────────
// `jet registry vendor` — copy resolved deps into vendor/
// ──────────────────────────────────────────────

/// Copy all resolved dependency store entries into `<vendor_dir>/<name>/`.
/// `vendor_dir` is `<project_root>/vendor` by default (D-SUPPLY1 `--vendor-dir`
/// relocates it). After vendoring, `--locked` builds can run offline by reading
/// from the vendor tree.
///
/// Also writes `<vendor_dir>/manifest.json` recording each dependency's name,
/// version, and tree-hash fingerprint, so an offline build can verify the copy
/// against the lockfile before trusting it (Tier B integrity floor).
pub fn vendor(
    project_root: &Path,
    lock: &LockFile,
    dep_dirs: &std::collections::HashMap<String, PathBuf>,
    vendor_dir: &Path,
) -> Result<Vec<String>, Diagnostic> {
    let _ = project_root; // resolved by the caller into `vendor_dir`
    ensure_directory(vendor_dir).map_err(|e| {
        Diagnostic::error(
            "E2604",
            format!(
                "couldn't create vendor directory `{}`: {}",
                vendor_dir.display(),
                e
            ),
            "the vendor directory is where `jet registry vendor` writes offline copies of dependencies."
                .into(),
            "check write permissions, or pass a writable `--vendor-dir <path>`.".into(),
            None,
        )
    })?;

    let mut copied = Vec::new();
    for (name, src_dir) in dep_dirs {
        if !safe_component(name) {
            return Err(Diagnostic::error(
                "E2604",
                format!("dependency `{name}` cannot be written to the vendor tree"),
                "dependency names must be one safe path component".into(),
                "use a dependency name without `/`, `\\`, `:`, or `..`".into(),
                None,
            ));
        }
        validate_source_tree(src_dir).map_err(|e| {
            Diagnostic::error(
                "E2604",
                format!("failed to inspect `{name}`: {e}"),
                "vendored dependency trees must contain real files and directories".into(),
                "remove symlinks from the dependency source and run `jet registry vendor` again".into(),
                None,
            )
        })?;
        let dest = vendor_dir.join(name);
        if let Ok(metadata) = std::fs::symlink_metadata(&dest) {
            if metadata.file_type().is_symlink() {
                return Err(Diagnostic::error(
                    "E2604",
                    format!("vendor destination `{}` is a symlink", dest.display()),
                    "refusing to remove or overwrite a symlink while vendoring".into(),
                    "replace the destination with a real directory and run `jet registry vendor` again".into(),
                    None,
                ));
            }
            // Remove stale copy.
            std::fs::remove_dir_all(&dest).ok();
        }
        copy_dir_recursive(src_dir, &dest).map_err(|e| {
            Diagnostic::error(
                "E2604",
                format!("failed to vendor `{}`: {}", name, e),
                "jet registry vendor copies dependency source into the vendor tree for offline builds."
                    .into(),
                "check that the dependency is correctly fetched first with `jet store fetch`.".into(),
                None,
            )
        })?;
        copied.push(name.clone());
    }
    copied.sort();

    // Write the vendor manifest from the lock so offline builds can re-verify.
    let manifest = vendor_manifest_json(lock, &copied);
    let manifest_path = vendor_dir.join("manifest.json");
    reject_existing_symlink(&manifest_path).map_err(|e| {
        Diagnostic::error(
            "E2604",
            format!("couldn't prepare the vendor manifest: {e}"),
            "the vendor manifest must be a regular file inside the vendor directory".into(),
            "replace a symlink at vendor/manifest.json with a regular file".into(),
            None,
        )
    })?;
    std::fs::write(&manifest_path, manifest).map_err(|e| {
        Diagnostic::error(
            "E2604",
            format!("couldn't write the vendor manifest: {}", e),
            "vendor/manifest.json records each dependency's name, version, and fingerprint.".into(),
            "check write permissions on the vendor directory.".into(),
            None,
        )
    })?;

    Ok(copied)
}

/// Build the `vendor/manifest.json` body — a small JSON object listing every
/// vendored package with its locked version and tree-hash fingerprint.
fn vendor_manifest_json(lock: &LockFile, copied: &[String]) -> String {
    let mut entries = Vec::new();
    for name in copied {
        if let Some(pkg) = lock.packages.iter().find(|p| &p.name == name) {
            entries.push(format!(
                "    {{ \"name\": {}, \"version\": {}, \"fingerprint\": {} }}",
                json_str(&pkg.name),
                json_str(&pkg.version),
                json_str(&pkg.fingerprint),
            ));
        }
    }
    format!(
        "{{\n  \"vendor_format\": 1,\n  \"packages\": [\n{}\n  ]\n}}\n",
        entries.join(",\n")
    )
}

fn json_str(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Recursively copy a directory tree.
fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    validate_source_tree(src)?;
    ensure_directory(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        let metadata = std::fs::symlink_metadata(&src_path)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("refusing symlink in dependency tree: {}", src_path.display()),
            ));
        }
        if metadata.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else if metadata.is_file() {
            reject_existing_symlink(&dest_path)?;
            std::fs::copy(&src_path, &dest_path)?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("unsupported dependency tree entry: {}", src_path.display()),
            ));
        }
    }
    Ok(())
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains(['/', '\\', ':'])
        && !value.chars().any(char::is_control)
        && matches!(Path::new(value).components().next(), Some(Component::Normal(_)))
        && Path::new(value).components().nth(1).is_none()
}

fn validate_source_tree(path: &Path) -> std::io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "dependency tree root must be a real directory",
        ));
    }
    Ok(())
}

fn reject_existing_symlink(path: &Path) -> std::io::Result<()> {
    if let Ok(metadata) = std::fs::symlink_metadata(path) {
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
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "directory must not be a symlink",
        )),
        Ok(metadata) if metadata.is_dir() => Ok(()),
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "directory path is not a directory",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if let Some(parent) = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
            {
                ensure_directory(parent)?;
            }
            std::fs::create_dir(path)
        }
        Err(error) => Err(error),
    }
}
