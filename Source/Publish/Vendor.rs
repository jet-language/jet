use crate::Diagnostics::Diagnostic;
use crate::Lock::LockFile;
use std::path::{Path, PathBuf};

// ──────────────────────────────────────────────
// `jet vendor` — copy resolved deps into vendor/
// ──────────────────────────────────────────────

/// Copy all resolved dependency store entries into `<project_root>/vendor/<name>/`.
/// After vendoring, `--locked` builds can run offline by reading from vendor/.
pub fn vendor(
    project_root: &Path,
    lock: &LockFile,
    dep_dirs: &std::collections::HashMap<String, PathBuf>,
) -> Result<Vec<String>, Diagnostic> {
    let vendor_dir = project_root.join("vendor");
    std::fs::create_dir_all(&vendor_dir).map_err(|e| Diagnostic::error(
        "E2604",
        format!("couldn't create vendor/ directory: {}", e),
        "vendor/ is where `jet vendor` writes offline copies of dependencies.".into(),
        "check write permissions on the project directory.".into(),
        None,
    ))?;

    let mut copied = Vec::new();
    for (name, src_dir) in dep_dirs {
        let dest = vendor_dir.join(name);
        if dest.exists() {
            // Remove stale copy.
            std::fs::remove_dir_all(&dest).ok();
        }
        copy_dir_recursive(src_dir, &dest).map_err(|e| Diagnostic::error(
            "E2604",
            format!("failed to vendor `{}`: {}", name, e),
            "jet vendor copies dependency source into vendor/ for offline builds.".into(),
            "check that the dependency is correctly fetched first with `jet fetch`.".into(),
            None,
        ))?;
        copied.push(name.clone());
    }
    let _ = lock; // lock used for hash verification in a future pass
    Ok(copied)
}

/// Recursively copy a directory tree.
fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}
