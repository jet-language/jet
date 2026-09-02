//! Public, read-only projections of the package model.
//!
//! This module owns the typed v1 view shapes used by compiler tooling. The
//! projections are deliberately narrower than [`crate::Package::PackageFacts`]
//! and [`crate::Lock::LockFile`]: they expose declarations and resolved lock
//! entries without exposing authority material, provenance internals, or
//! current-package scalar facts that already have a canonical `@build.*`
//! spelling.

use crate::Lock::{LockFile, LockSource};
use crate::Package::{self, PackageFacts, PackageOutputKind};
use std::collections::BTreeMap;

/// Version of the public package-model field matrix.
pub const PACKAGE_MODEL_SCHEMA_VERSION: u32 = 1;

/// One declared dependency. Git credentials and URL query/fragment material
/// are removed by [`Package::dep_display_redacted`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyView {
    pub name: String,
    pub source: String,
}

/// One declared package target and its ordered target kinds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageTargetView {
    pub name: String,
    pub targets: Vec<String>,
}

/// One declared output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputView {
    pub name: String,
    pub kind: String,
    pub entry: Option<String>,
}

/// One named build profile from the package's `build: { … }` declarations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildProfileView {
    pub name: String,
    pub optimize: String,
    pub debug_info: bool,
    pub small: bool,
    pub panic: Option<String>,
}

/// The uncomposed `package.jet` declaration view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestView {
    pub schema_version: u32,
    pub file: String,
    pub jet: Option<String>,
    pub edition: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
    pub layer: Option<String>,
    pub target: Option<String>,
    pub dependencies: Vec<DependencyView>,
    pub packages: Vec<PackageTargetView>,
    pub outputs: Vec<OutputView>,
    pub build_profiles: Vec<BuildProfileView>,
}

/// The composed package-facts view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageView {
    pub schema_version: u32,
    pub file: String,
    pub jet: Option<String>,
    pub edition: Option<String>,
    pub description: Option<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
    pub layer: Option<String>,
    pub target: Option<String>,
    pub dependencies: Vec<DependencyView>,
    pub packages: Vec<PackageTargetView>,
    pub outputs: Vec<OutputView>,
    pub build_profiles: Vec<BuildProfileView>,
}

/// One resolved lock entry in the public lock view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockedPackageView {
    pub name: String,
    pub version: String,
    pub source_kind: String,
    pub source: Option<String>,
    pub revision: Option<String>,
    pub fingerprint: String,
    pub content_hash: Option<String>,
    pub dependencies: Vec<String>,
    pub layer: Option<String>,
    pub inferred_layer: Option<String>,
}

/// The public projection of `.jet/lock`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockView {
    pub schema_version: u32,
    pub file: String,
    pub version: u32,
    pub root_dependencies: Vec<String>,
    pub packages: Vec<LockedPackageView>,
}

/// One collision entry in a profile declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyValueView {
    pub key: String,
    pub value: String,
}

/// One evaluated package profile declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileView {
    pub name: String,
    pub extends: Vec<String>,
    pub packages: Vec<String>,
    pub collisions: Vec<KeyValueView>,
    pub sources: Vec<String>,
}

impl ProfileView {
    /// Build a canonical profile projection from an environment-model
    /// declaration without coupling this crate to `jet-env-model`.
    pub fn from_parts(
        name: impl Into<String>,
        extends: Vec<String>,
        packages: Vec<String>,
        collisions: impl IntoIterator<Item = (String, String)>,
        sources: Vec<String>,
    ) -> Self {
        Self {
            name: name.into(),
            extends,
            packages,
            collisions: collisions
                .into_iter()
                .map(|(key, value)| KeyValueView { key, value })
                .collect(),
            sources,
        }
    }
}

/// The public projection of the package profile set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileSetView {
    pub schema_version: u32,
    pub file: String,
    pub profiles: Vec<ProfileView>,
}

impl ProfileSetView {
    /// Build a schema-versioned profile set while retaining profile order.
    pub fn from_profiles(profiles: impl IntoIterator<Item = ProfileView>) -> Self {
        Self {
            schema_version: PACKAGE_MODEL_SCHEMA_VERSION,
            file: crate::Syntax::ENV_FILE.to_string(),
            profiles: profiles.into_iter().collect(),
        }
    }
}

/// Project one uncomposed package declaration into the public manifest view.
pub fn manifest_view_from_facts(facts: &PackageFacts) -> ManifestView {
    ManifestView {
        schema_version: PACKAGE_MODEL_SCHEMA_VERSION,
        file: crate::Syntax::PACKAGE_FILE.to_string(),
        jet: facts.jet.clone(),
        edition: facts.edition.clone(),
        description: facts.description.clone(),
        license: facts.license.clone(),
        repository: facts.repository.as_deref().map(redact_repository_url),
        layer: option_layer(facts.layer),
        target: facts.target.clone(),
        dependencies: dependency_views(&facts.deps),
        packages: package_target_views(&facts.packages),
        outputs: output_views(&facts.outputs),
        build_profiles: build_profile_views(&facts.build_profiles),
    }
}

/// Project composed package facts into the public package view.
pub fn package_view_from_facts(facts: &PackageFacts) -> PackageView {
    PackageView {
        schema_version: PACKAGE_MODEL_SCHEMA_VERSION,
        file: crate::Syntax::PACKAGE_FILE.to_string(),
        jet: facts.jet.clone(),
        edition: facts.edition.clone(),
        description: facts.description.clone(),
        license: facts.license.clone(),
        repository: facts.repository.as_deref().map(redact_repository_url),
        layer: option_layer(facts.layer),
        target: facts.target.clone(),
        dependencies: dependency_views(&facts.deps),
        packages: package_target_views(&facts.packages),
        outputs: output_views(&facts.outputs),
        build_profiles: build_profile_views(&facts.build_profiles),
    }
}

/// Project the lock model into the public lock view without exposing authority
/// or secret-bearing source fields.
pub fn lock_view_from_lock(lock: &LockFile) -> LockView {
    LockView {
        schema_version: PACKAGE_MODEL_SCHEMA_VERSION,
        file: crate::Syntax::UNIFIED_LOCK_FILE.to_string(),
        version: lock.version,
        root_dependencies: lock.root_dependencies.clone(),
        packages: lock
            .packages
            .iter()
            .map(|package| LockedPackageView {
                name: package.name.clone(),
                version: package.version.clone(),
                source_kind: lock_source_kind(&package.source).to_string(),
                source: lock_source_reference(&package.source),
                revision: package.locked.as_ref().map(|revision| revision.rev.clone()),
                fingerprint: package.fingerprint.clone(),
                content_hash: package.content_hash.clone(),
                dependencies: package.dependencies.clone(),
                layer: option_layer(package.layer),
                inferred_layer: option_layer(package.inferred_layer),
            })
            .collect(),
    }
}

fn option_layer(layer: Option<crate::Syntax::RuntimeLayer>) -> Option<String> {
    layer.map(|layer| layer.as_str().to_string())
}

fn dependency_views(dependencies: &BTreeMap<String, Package::DepSource>) -> Vec<DependencyView> {
    dependencies
        .iter()
        .map(|(name, source)| DependencyView {
            name: name.clone(),
            source: Package::dep_display_redacted(source),
        })
        .collect()
}

fn target_name(target: &Package::Target) -> &'static str {
    match target {
        Package::Target::Library => "library",
        Package::Target::Executable => "executable",
        Package::Target::Test => "test",
        Package::Target::Example => "example",
        Package::Target::Plugin { .. } => "plugin",
    }
}

fn package_target_views(packages: &[Package::PackageEntry]) -> Vec<PackageTargetView> {
    packages
        .iter()
        .map(|package| PackageTargetView {
            name: package.name.clone(),
            targets: package
                .targets
                .iter()
                .map(|target| target_name(target).to_string())
                .collect(),
        })
        .collect()
}

fn output_kind_name(kind: PackageOutputKind) -> &'static str {
    match kind {
        PackageOutputKind::Library => "library",
        PackageOutputKind::Executable => "executable",
        PackageOutputKind::Service => "service",
        PackageOutputKind::Check => "check",
        PackageOutputKind::Environment => "environment",
        PackageOutputKind::Image => "image",
        PackageOutputKind::Bundle => "bundle",
        PackageOutputKind::System => "system",
        PackageOutputKind::Fleet => "fleet",
    }
}

fn output_views(outputs: &BTreeMap<String, Package::OutputFact>) -> Vec<OutputView> {
    outputs
        .values()
        .map(|output| OutputView {
            name: output.name.clone(),
            kind: output_kind_name(output.kind).to_string(),
            entry: output.entry.clone(),
        })
        .collect()
}

fn build_profile_views(profiles: &[Package::BuildProfileDef]) -> Vec<BuildProfileView> {
    profiles
        .iter()
        .map(|profile| BuildProfileView {
            name: profile.name.clone(),
            optimize: profile.optimize.as_str().to_string(),
            debug_info: profile.debug_info,
            small: profile.small,
            panic: profile.panic.map(|panic| match panic {
                Package::BuildPanic::Unwind => "unwind".to_string(),
                Package::BuildPanic::Abort => "abort".to_string(),
            }),
        })
        .collect()
}

fn lock_source_kind(source: &LockSource) -> &'static str {
    match source {
        LockSource::Root => "root",
        LockSource::Path(_) => "path",
        LockSource::Git { .. } => "git",
        LockSource::Nix { .. } => "nix",
        LockSource::Cran { .. } => "cran",
        LockSource::LuaRocks { .. } => "lua_rocks",
        LockSource::Registry { .. } => "registry",
        LockSource::Foreign { .. } => "foreign",
    }
}

fn lock_source_reference(source: &LockSource) -> Option<String> {
    match source {
        LockSource::Root | LockSource::Path(_) => None,
        LockSource::Git { selector, .. } => Some(selector.clone()),
        LockSource::Nix { reference, .. }
        | LockSource::Cran { reference, .. }
        | LockSource::LuaRocks { reference, .. }
        | LockSource::Registry { reference, .. }
        | LockSource::Foreign { reference, .. } => Some(reference.clone()),
    }
}

fn redact_repository_url(value: &str) -> String {
    let value = value.split(['?', '#']).next().unwrap_or(value);
    let Some(separator) = value.find("://") else {
        return value.to_string();
    };
    let authority_start = separator + 3;
    let authority_end = value[authority_start..]
        .find(['/', '?', '#'])
        .map(|offset| authority_start + offset)
        .unwrap_or(value.len());
    let authority = &value[authority_start..authority_end];
    let Some(at) = authority.rfind('@') else {
        return value.to_string();
    };
    format!(
        "{}{}{}",
        &value[..authority_start],
        &authority[at + 1..],
        &value[authority_end..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Package::PackageFacts;

    #[test]
    fn package_views_preserve_manifest_composition_and_redact_sources() {
        let facts = PackageFacts::parse(
            r#"name: "demo"
version: "1.0.0"
repository: "https://user:secret@example.test/demo?token=hidden"
deps: { remote: { git: "https://user:secret@example.test/repo?token=hidden", tag: "v1" } }
"#,
            "package.jet",
        )
        .expect("manifest fixture parses");
        let view = manifest_view_from_facts(&facts);
        assert_eq!(view.schema_version, PACKAGE_MODEL_SCHEMA_VERSION);
        assert_eq!(view.repository.as_deref(), Some("https://example.test/demo"));
        assert_eq!(view.dependencies[0].source, r#"{ git: "https://example.test/repo", tag: "v1" }"#);
    }

    #[test]
    fn profile_views_keep_declared_order_and_collision_keys() {
        let profile = ProfileView::from_parts(
            "dev",
            vec!["base".to_string()],
            vec!["default.fd".to_string()],
            [("bin/fd".to_string(), "fd@default".to_string())],
            vec!["profile.dev".to_string()],
        );
        let view = ProfileSetView::from_profiles([profile]);
        assert_eq!(view.schema_version, 1);
        assert_eq!(view.file, crate::Syntax::ENV_FILE);
        assert_eq!(view.profiles[0].collisions[0].key, "bin/fd");
    }
}
