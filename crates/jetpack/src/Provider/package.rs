//! Canonical Jet package facts used by the core provider.

use super::{PackageManifest, SHA256};
use jet_pkg_model::Package::{PackageFacts, PackageOutputKind};
use std::path::{Path, PathBuf};

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

pub(super) fn core_recipe_identity(
    src_dir: &Path,
    package: &str,
    manifest: Option<&PackageManifest::PackManifest>,
    kind: PackageManifest::PackageKind,
    canonical: Option<&PackageFacts>,
) -> String {
    let toolchain = crate::Toolchain::Toolchain::resolve();
    // The recipe is the semantic build identity, not only the manifest and
    // tool name. Include the complete source closure so a package with the
    // same manifest but a changed nested source tree cannot reuse a stale
    // realization.
    let source_tree = super::tree_fingerprint(src_dir);
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
    format!(
        "core-provider-recipe-v4\npackage={package}\nversion={version}\nkind={kind:?}\nartifact={artifact}\nsource_tree={source_tree}\nmanifest={}\ntoolchain={}\n",
        SHA256::sha256_hex(semantics.as_bytes()),
        toolchain_facts(toolchain.as_ref()),
    )
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
