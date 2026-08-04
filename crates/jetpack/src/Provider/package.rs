//! Canonical Jet package facts used by the core provider.

use super::{PackageManifest, SHA256};
use jet_pkg_model::Package::{MemberRef, PackageFacts, PackageOutputKind};
use std::path::{Path, PathBuf};

/// Resolve the canonical Package that owns a requested package/output. A
/// monorepo root can carry `members: find(...)`; a sparse source checkout then
/// contains both that root marker and the addressed member marker. The member
/// must win, otherwise the provider would compile the workspace root as the
/// requested package and silently lose the package boundary.
pub(super) fn find_canonical_package(
    repo: &Path,
    requested: &str,
) -> Result<Option<(PathBuf, PackageFacts)>, String> {
    let root_marker = repo.join(crate::Syntax::PACKAGE_FILE);
    let root_marker_metadata = match std::fs::symlink_metadata(&root_marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    if root_marker_metadata.file_type().is_symlink() {
        return Err(format!("canonical Package marker is a symlink: {}", root_marker.display()));
    }
    if !root_marker_metadata.is_file() {
        return Ok(None);
    }
    let root = PackageFacts::load(repo)
        .ok_or_else(|| format!("canonical Package {} could not be read", root_marker.display()))?
        .map_err(|error| format!("canonical Package {} is invalid: {error}", root_marker.display()))?;
    let root_matches = root.name == requested || root.outputs.contains_key(requested);
    if root_matches {
        return Ok(Some((repo.to_path_buf(), root)));
    }
    if root.members.is_empty() {
        return Ok(None);
    }

    let mut candidates = Vec::new();
    root.validate_members_in(repo)
        .map_err(|error| format!("canonical Package members are invalid: {error}"))?;
    collect_canonical_packages(repo, &root.members, requested, &mut candidates)?;
    match candidates.len() {
        0 => Err(format!(
            "canonical Package `{requested}` was not found under {}",
            repo.display()
        )),
        1 => Ok(candidates.pop()),
        _ => Err(format!(
            "canonical Package `{requested}` is ambiguous under {}",
            repo.display()
        )),
    }
}

fn collect_canonical_packages(
    repo: &Path,
    members: &[MemberRef],
    requested: &str,
    out: &mut Vec<(PathBuf, PackageFacts)>,
) -> Result<(), String> {
    let mut seen = Vec::new();
    for member in members {
        let (relative, discover) = match member {
            MemberRef::Path(path) => (path.as_str(), false),
            MemberRef::Find(path) => (path.as_str(), true),
        };
        let member_root = repo.join(relative);
        let metadata = std::fs::symlink_metadata(&member_root)
            .map_err(|error| format!("could not inspect Package member `{relative}`: {error}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(format!(
                "Package member `{relative}` is not a regular directory"
            ));
        }
        if discover {
            let entries = std::fs::read_dir(&member_root)
                .map_err(|error| format!("could not read Package member discovery `{relative}`: {error}"))?;
            for entry in entries {
                let path = entry
                    .map_err(|error| error.to_string())?
                    .path();
                let metadata = std::fs::symlink_metadata(&path)
                    .map_err(|error| error.to_string())?;
                let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
                if metadata.file_type().is_symlink()
                    || !metadata.is_dir()
                    || name.starts_with('.')
                    || matches!(name, "build" | "target")
                {
                    continue;
                }
                collect_one_canonical_package(&path, requested, &mut seen, out)?;
            }
        } else {
            collect_one_canonical_package(&member_root, requested, &mut seen, out)?;
        }
    }
    Ok(())
}

fn collect_one_canonical_package(
    path: &Path,
    requested: &str,
    seen: &mut Vec<PathBuf>,
    out: &mut Vec<(PathBuf, PackageFacts)>,
) -> Result<(), String> {
    if seen.iter().any(|candidate| candidate == path) {
        return Ok(());
    }
    seen.push(path.to_path_buf());
    let marker = [
        path.join(crate::Syntax::PACKAGE_FILE),
        path.join(crate::Syntax::PAYLOAD_FILE),
    ]
    .into_iter()
    .find(|candidate| candidate.is_file())
    .or_else(|| {
        [
            path.join(crate::Syntax::PACKAGE_FILE),
            path.join(crate::Syntax::PAYLOAD_FILE),
        ]
        .into_iter()
        .find(|candidate| candidate.exists())
    });
    let Some(marker) = marker else {
        return Ok(());
    };
    let marker_metadata = std::fs::symlink_metadata(&marker).map_err(|error| error.to_string())?;
    if marker_metadata.file_type().is_symlink() {
        return Err(format!("Package marker is a symlink: {}", marker.display()));
    }
    if !marker_metadata.is_file() {
        return Err(format!("Package marker is not a file: {}", marker.display()));
    }
    let facts = PackageFacts::load(path)
        .ok_or_else(|| format!("canonical Package {} could not be read", marker.display()))?
        .map_err(|error| format!("canonical Package {} is invalid: {error}", marker.display()))?;
    if facts.name == requested || facts.outputs.contains_key(requested) {
        out.push((path.to_path_buf(), facts));
    }
    Ok(())
}

pub(super) fn toolchain_facts(
    toolchain: Option<&crate::Toolchain::Toolchain>,
) -> String {
    let Some(toolchain) = toolchain else {
        return "missing".to_string();
    };
    // Do not put the host-specific cargo path in a cache identity. Hash the
    // pinned executable when it is addressable; for a PATH-resolved dev cargo,
    // record its version output instead. Both forms describe the tool, not the
    // checkout path that happened to contain it.
    let cargo_digest = match std::fs::read(&toolchain.cargo) {
        Ok(bytes) => SHA256::sha256_hex(&bytes),
        Err(_) => std::process::Command::new(&toolchain.cargo)
            .arg("--version")
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| SHA256::sha256_hex(&output.stdout))
            .unwrap_or_else(|| "unavailable".to_string()),
    };
    format!(
        "id={}\nversion={}\npinned={}\ncargo_sha256={}",
        toolchain.id, toolchain.version, toolchain.pinned, cargo_digest
    )
}

/// Strict, path-independent identity for a Core source closure. The generic
/// provider fingerprint is intentionally best-effort for legacy providers;
/// Core source must reject unreadable, non-file, and symlink nodes instead of
/// hashing a narrower tree and reusing a stale object.
pub(super) fn core_tree_fingerprint(root: &Path) -> Result<String, String> {
    validate_core_source_tree(root)?;
    let mut files = Vec::new();
    collect_core_files(root, root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut input = Vec::new();
    input.extend_from_slice(b"jet-core-source-tree-v2");
    for (relative, path) in files {
        let bytes = std::fs::read(&path).map_err(|error| error.to_string())?;
        input.extend_from_slice(&(relative.len() as u64).to_be_bytes());
        input.extend_from_slice(relative.as_bytes());
        input.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
        input.extend_from_slice(&bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::symlink_metadata(&path)
                .map_err(|error| error.to_string())?
                .permissions()
                .mode();
            input.extend_from_slice(&mode.to_be_bytes());
        }
    }
    Ok(SHA256::sha256_hex(&input))
}

fn collect_core_files(
    root: &Path,
    current: &Path,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(current).map_err(|error| error.to_string())? {
        let path = entry.map_err(|error| error.to_string())?.path();
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err(format!("core source tree contains symlink {}", path.display()));
        }
        if metadata.is_dir() {
            collect_core_files(root, &path, out)?;
        } else if metadata.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, path));
        } else {
            return Err(format!("core source tree contains non-file {}", path.display()));
        }
    }
    Ok(())
}

pub(super) fn core_recipe_identity(
    src_dir: &Path,
    package: &str,
    manifest: Option<&PackageManifest::PackManifest>,
    kind: PackageManifest::PackageKind,
    canonical: Option<&PackageFacts>,
    toolchain: Option<&crate::Toolchain::Toolchain>,
) -> Result<String, String> {
    // The recipe is the semantic build identity, not only the manifest and
    // tool name. Include the complete source closure so a package with the
    // same manifest but a changed nested source tree cannot reuse a stale
    // realization.
    let source_tree = core_tree_fingerprint(src_dir)?;
    let artifact = if kind == PackageManifest::PackageKind::Library
        && src_dir.join("Cargo.toml").is_file()
    {
        "cargo-rlib"
    } else if kind == PackageManifest::PackageKind::Library {
        "jet-library-source"
    } else {
        "executable-tree"
    };
    let version = canonical
        .and_then(|facts| facts.version.as_deref())
        .or_else(|| manifest.map(|manifest| manifest.package.version.as_str()))
        .unwrap_or("");
    let semantics = canonical.map_or_else(
        || normalized_manifest_semantics(manifest),
        canonical_package_semantics,
    );
    let toolchain = if artifact == "cargo-rlib" {
        toolchain_facts(toolchain)
    } else {
        "not-required".to_string()
    };
    Ok(format!(
        "core-provider-recipe-v4\npackage={package}\nversion={version}\nkind={kind:?}\nartifact={artifact}\nsource_tree={source_tree}\nmanifest={}\ntoolchain={}\n",
        SHA256::sha256_hex(semantics.as_bytes()),
        toolchain,
    ))
}

/// `PackageFacts` carries parse origins for diagnostics. Origins are not build
/// semantics and must not make two identical packages at different checkout
/// paths miss the same cache entry.
fn canonical_package_semantics(facts: &PackageFacts) -> String {
    let mut facts = facts.clone();
    facts.origin.clear();
    for config in facts.inline_configs.values_mut() {
        config.origin.clear();
    }
    format!("canonical-package-facts-v1:{facts:?}")
}

pub(super) fn canonical_package_kind(
    facts: &PackageFacts,
    package: &str,
) -> Option<PackageManifest::PackageKind> {
    let output = facts
        .outputs
        .get(package)
        .or_else(|| facts.select_output("run", None, None).ok())?;
    match output.kind {
        PackageOutputKind::Library => Some(PackageManifest::PackageKind::Library),
        PackageOutputKind::Executable | PackageOutputKind::Service => {
            Some(PackageManifest::PackageKind::Executable)
        }
        PackageOutputKind::Check
        | PackageOutputKind::Environment
        | PackageOutputKind::Image
        | PackageOutputKind::Bundle
        | PackageOutputKind::System
        | PackageOutputKind::Fleet => None,
    }
}

pub(super) fn canonical_source_dir(repo: &Path, facts: &PackageFacts) -> Option<PathBuf> {
    let relative = facts.source.as_deref().unwrap_or(".");
    let path = Path::new(relative);
    if path.is_absolute()
        || path
            .components()
            .any(|component| component == std::path::Component::ParentDir)
    {
        return None;
    }
    let source = repo.join(path);
    source.is_dir().then_some(source)
}

pub(super) fn validate_core_source_tree(root: &Path) -> Result<(), String> {
    let metadata = std::fs::symlink_metadata(root).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(format!(
            "core source root is not a real directory: {}",
            root.display()
        ));
    }
    let entries = std::fs::read_dir(root).map_err(|error| error.to_string())?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err(format!("core source tree contains symlink {}", path.display()));
        }
        if metadata.is_dir() {
            validate_core_source_tree(&path)?;
        } else if !metadata.is_file() {
            return Err(format!("core source tree contains non-file {}", path.display()));
        }
    }
    Ok(())
}

fn normalized_manifest_semantics(
    manifest: Option<&PackageManifest::PackManifest>,
) -> String {
    let Some(manifest) = manifest else {
        return "manifest=absent".to_string();
    };
    let mut manifest = manifest.clone();
    manifest.deps.sort_by(|a, b| a.name.cmp(&b.name));
    manifest.packages.sort_by(|a, b| a.name.cmp(&b.name));
    for package in &mut manifest.packages {
        package.targets.sort_by_key(|target| format!("{target:?}"));
    }
    manifest.build_profiles.sort_by(|a, b| a.name.cmp(&b.name));
    manifest.grants.sort_by(|a, b| a.0.cmp(&b.0));
    for (_, effects) in &mut manifest.grants {
        effects.sort();
    }
    if let Some(effects) = &mut manifest.effects_allow {
        effects.sort();
    }
    if let Some(effects) = &mut manifest.effects_deny {
        effects.sort();
    }
    if let Some(policy) = &mut manifest.trust_policy {
        policy.services.sort_by(|a, b| a.0.cmp(&b.0));
    }
    format!("{manifest:?}")
}
