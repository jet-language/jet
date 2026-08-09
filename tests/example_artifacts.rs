//! Keep generated build output out of `examples/`.

use std::fs;
use std::path::{Path, PathBuf};

fn artifact_paths(path: &Path, found: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(path).unwrap_or_else(|error| {
        panic!("cannot read examples directory {}: {error}", path.display())
    });
    for entry in entries {
        let entry = entry.unwrap();
        let path = entry.path();
        let file_type = entry.file_type().unwrap();
        if file_type.is_dir() {
            if entry.file_name().to_str() == Some("build") {
                found.push(path);
            } else {
                artifact_paths(&path, found);
            }
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) == Some("rs")
            || is_executable(&path)
        {
            found.push(path);
        }
    }
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    fs::metadata(path)
        .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|extension| extension.to_str()),
        Some("exe") | Some("dll") | Some("com")
    )
}

#[test]
fn examples_contain_no_build_artifacts() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples");
    let mut found = Vec::new();
    artifact_paths(&root, &mut found);
    found.sort();
    assert!(
        found.is_empty(),
        "generated build artifacts under examples:\n{}",
        found
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join("\n")
    );
}
