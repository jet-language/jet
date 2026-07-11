use crate::Diagnostics::Diagnostic;
use crate::Lock::LockFile;
use std::path::{Path, PathBuf};

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
    std::fs::create_dir_all(vendor_dir).map_err(|e| {
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
        let dest = vendor_dir.join(name);
        if dest.exists() {
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
    std::fs::write(vendor_dir.join("manifest.json"), manifest).map_err(|e| {
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
