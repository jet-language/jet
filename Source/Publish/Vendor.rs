use crate::Diagnostics::Diagnostic;
use crate::Lock::LockFile;
use crate::Store;
use std::ffi::OsStr;
use std::io;
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
    let vendor = Store::ensure_directory_authority(vendor_dir)
        .map_err(|error| vendor_io_error("creating vendor directory", vendor_dir, error))?;

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

        let source = Store::open_directory_authority(src_dir)
            .map_err(|error| vendor_io_error("inspecting dependency source", src_dir, error))?;
        let source_hash = Store::hash_directory_authority(&source, true)
            .map_err(|error| vendor_io_error("hashing dependency source", src_dir, error))?;
        let destination_name = OsStr::new(name);

        match vendor.open_child_directory(destination_name) {
            Ok(existing) => {
                let existing_hash =
                    Store::hash_directory_authority(&existing, false).map_err(|error| {
                        vendor_io_error(
                            "checking existing vendor copy",
                            &vendor_dir.join(name),
                            error,
                        )
                    })?;
                if existing_hash == source_hash {
                    copied.push(name.clone());
                    continue;
                }
                drop(existing);
                vendor
                    .remove_child_tree(destination_name)
                    .map_err(|error| {
                        vendor_io_error("removing stale vendor copy", &vendor_dir.join(name), error)
                    })?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(vendor_io_error(
                    "checking vendor destination",
                    &vendor_dir.join(name),
                    error,
                ))
            }
        }

        let (staging_name, staging) = vendor.create_private_child("vendor").map_err(|error| {
            vendor_io_error("creating vendor staging directory", vendor_dir, error)
        })?;
        let result = (|| {
            Store::copy_directory_authority(&source, &staging, true).map_err(|error| {
                vendor_io_error("copying dependency into vendor tree", src_dir, error)
            })?;
            let staging_hash =
                Store::hash_directory_authority(&staging, false).map_err(|error| {
                    vendor_io_error("checking staged vendor copy", &vendor_dir.join(name), error)
                })?;
            if staging_hash != source_hash {
                return Err(vendor_io_error(
                    "checking staged vendor copy",
                    &vendor_dir.join(name),
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "staged vendor copy does not match the source tree",
                    ),
                ));
            }
            match Store::publish_directory(&vendor, &staging_name, &vendor, destination_name) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let winner =
                        vendor
                            .open_child_directory(destination_name)
                            .map_err(|error| {
                                vendor_io_error(
                                    "opening concurrent vendor copy",
                                    &vendor_dir.join(name),
                                    error,
                                )
                            })?;
                    let winner_hash =
                        Store::hash_directory_authority(&winner, false).map_err(|error| {
                            vendor_io_error(
                                "checking concurrent vendor copy",
                                &vendor_dir.join(name),
                                error,
                            )
                        })?;
                    if winner_hash == source_hash {
                        Ok(())
                    } else {
                        Err(vendor_io_error(
                            "publishing vendor copy",
                            &vendor_dir.join(name),
                            io::Error::new(
                                io::ErrorKind::AlreadyExists,
                                "concurrent vendor copy has different contents",
                            ),
                        ))
                    }
                }
                Err(error) => Err(vendor_io_error(
                    "publishing vendor copy",
                    &vendor_dir.join(name),
                    error,
                )),
            }
        })();
        let _ = vendor.remove_child_tree(&staging_name);
        result?;
        copied.push(name.clone());
    }
    copied.sort();

    let manifest = vendor_manifest_json(lock, &copied);
    vendor
        .write_child_file(OsStr::new("manifest.json"), manifest.as_bytes())
        .map_err(|error| {
            vendor_io_error(
                "writing vendor manifest",
                &vendor_dir.join("manifest.json"),
                error,
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

fn vendor_io_error(action: &str, path: &Path, error: io::Error) -> Diagnostic {
    Diagnostic::error(
        "E2604",
        format!("couldn't {} `{}`: {}", action, path.display(), error),
        "vendored dependency trees must contain real files and directories.".into(),
        "check the dependency source and vendor directory permissions.".into(),
        None,
    )
}

fn safe_component(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.contains(['/', '\\', ':'])
        && !value.chars().any(char::is_control)
        && matches!(
            Path::new(value).components().next(),
            Some(Component::Normal(_))
        )
        && Path::new(value).components().nth(1).is_none()
}
