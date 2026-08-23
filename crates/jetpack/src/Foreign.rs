//! Package-backed projections for the unified foreign namespace.
//!
//! A foreign package is realized as one verified Hangar object. Only after
//! Hangar has copied and re-hashed the provider output do we project the
//! generated binding into the project binding cache.

use crate::Provider::{Ctx, ProviderError};
use crate::RefSpec::{self, ProviderRef, Source};
use crate::Store::{self, CacheIdentity, IngestRequest, StoreEntry};
use crate::{Lock, Manifest, Syntax, SHA256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// The result of one package-provider projection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Realization {
    pub name: String,
    pub language: crate::AST::ForeignLanguage,
    /// The provider ref inside the language-qualified manifest value.
    pub reference: String,
    /// The canonical cache/lock identity (`<language>@<provider-ref>`).
    pub provider_reference: String,
    pub entry: StoreEntry,
    pub binding: PathBuf,
    pub provenance: PathBuf,
}

/// Realize every foreign dependency in a compiler manifest. Missing or
/// malformed provider output is an error, never an empty binding or a guessed
/// fallback.
pub fn realize_manifest_dependencies(
    roots: &Store::Roots,
    project_root: &Path,
    manifest: &Manifest::Manifest,
    ctx: &Ctx<'_>,
) -> Result<Vec<Realization>, ProviderError> {
    let mut realized = Vec::new();
    for (name, dependency) in &manifest.dependencies {
        let Manifest::DepSpec::Foreign {
            language,
            reference,
        } = dependency
        else {
            continue;
        };
        realized.push(realize_one(
            roots,
            project_root,
            name,
            *language,
            reference,
            ctx,
        )?);
    }
    // Publish the project lock only after every provider artifact has passed
    // Hangar ingest and projection. A failed later dependency must not leave
    // a half-written lock that claims the earlier one succeeded.
    for item in &realized {
        Lock::record_foreign_realization(
            project_root,
            &item.name,
            &item.entry.version,
            item.language,
            &item.reference,
            &item.entry.out,
            Lock::LockEnvelope {
                output_hash: item.entry.envelope.output_hash.clone(),
                platform: item.entry.envelope.platform.clone(),
                signature: item.entry.envelope.signature.clone(),
                provenance: item.entry.envelope.provenance.clone(),
            },
        )
        .map_err(ProviderError::Ingest)?;
    }
    Ok(realized)
}

fn realize_one(
    roots: &Store::Roots,
    project_root: &Path,
    name: &str,
    language: crate::AST::ForeignLanguage,
    reference: &str,
    ctx: &Ctx<'_>,
) -> Result<Realization, ProviderError> {
    validate_library_name(name)?;
    let spec = RefSpec::classify_provider_ref(reference).map_err(|error| {
        ProviderError::ForeignProjection(format!(
            "foreign dependency {name} has an invalid provider ref {reference}: {error:?}"
        ))
    })?;
    let source = provider_artifact_root(project_root, &spec, ctx, name)?;
    let expected_binding = source
        .join(Syntax::SOURCE_ROOT_DIR)
        .join(language.bindings_subdir())
        .join(format!("{name}.{}", Syntax::FILE_EXT));
    require_regular_file(&expected_binding, "generated foreign binding")?;

    let foreign_reference = format!("{}@{}", language.root(), reference);
    let Some(version) = exact_version(&spec) else {
        return Err(ProviderError::ForeignProjection(format!(
            "foreign dependency {name} must pin its provider package with `#version=...`"
        )));
    };
    let source_fingerprint = SHA256::tree_hash(&source);
    let identity = CacheIdentity {
        source_fingerprint,
        recipe_fingerprint: SHA256::sha256_hex(b"foreign-binding-projection-v1"),
        policy_fingerprint: crate::RuntimePolicy::cache_policy_fingerprint(ctx.offline),
        platform: crate::Envelope::host_platform(),
    };
    let ingested = Store::ingest_tree(
        roots,
        &IngestRequest {
            name: name.to_string(),
            version: version.clone(),
            reference: foreign_reference.clone(),
            cache_identity: identity,
            references: Vec::new(),
            outputs: BTreeMap::from([(String::from("out"), source.clone())]),
            signature: String::new(),
            provenance: format!(
                "foreign-provider-v1 language={} provider={} reference={}",
                language.root(),
                spec.provider.label(),
                reference
            ),
            platform_artifact_kind: String::new(),
        },
    )
    .map_err(|error| ProviderError::Ingest(error.what()))?;
    let entry = ingested.entry;
    let stored_binding = Path::new(&entry.out)
        .join(Syntax::SOURCE_ROOT_DIR)
        .join(language.bindings_subdir())
        .join(format!("{name}.{}", Syntax::FILE_EXT));
    require_regular_file(&stored_binding, "verified foreign binding")?;

    let binding = project_binding_path(project_root, language, name);
    copy_verified_file(&stored_binding, &binding)?;
    let provenance = provenance_path(project_root, language, name);
    let provenance_text = format!(
        "schema = foreign-binding-v1\nlanguage = {}\nname = {}\nreference = {}\nstore-id = {}\noutput-hash = {}\n",
        language.root(),
        name,
        foreign_reference,
        entry.id,
        entry.envelope.output_hash
    );
    write_verified_file(&provenance, provenance_text.as_bytes())?;
    Ok(Realization {
        name: name.to_string(),
        language,
        reference: reference.to_string(),
        provider_reference: foreign_reference,
        entry,
        binding,
        provenance,
    })
}

/// The canonical project-side location used by the compiler foreign loader.
pub fn project_binding_path(
    project_root: &Path,
    language: crate::AST::ForeignLanguage,
    name: &str,
) -> PathBuf {
    project_root
        .join(Syntax::SOURCE_ROOT_DIR)
        .join(language.bindings_subdir())
        .join(format!("{name}.{}", Syntax::FILE_EXT))
}

/// The canonical provenance sidecar location.
pub fn provenance_path(
    project_root: &Path,
    language: crate::AST::ForeignLanguage,
    name: &str,
) -> PathBuf {
    project_root
        .join(Syntax::SOURCE_ROOT_DIR)
        .join(language.bindings_subdir())
        .join(format!("{name}.provenance"))
}

fn provider_artifact_root(
    project_root: &Path,
    spec: &ProviderRef,
    ctx: &Ctx<'_>,
    name: &str,
) -> Result<PathBuf, ProviderError> {
    let candidate = match &spec.provider {
        Source::Path => {
            let target = spec
                .target
                .split_once("#version=")
                .map_or(spec.target.as_str(), |(path, _)| path);
            let path = Path::new(target);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                project_root.join(path)
            }
        }
        _ => {
            let fixtures = ctx.fixtures.ok_or_else(|| {
                ProviderError::Unsupported(format!(
                    "foreign provider {} has no verified artifact cache for {}; fetch the provider output before compiling",
                    spec.provider.label(),
                    spec.raw
                ))
            })?;
            fixtures
                .join("foreign")
                .join(spec.provider.label())
                .join(safe_ref_name(name, &spec.target))
        }
    };
    require_directory(&candidate, "foreign provider artifact")?;
    Ok(candidate)
}

fn safe_ref_name(name: &str, reference: &str) -> String {
    let mut out = String::with_capacity(name.len() + reference.len() + 1);
    out.push_str(name);
    out.push('-');
    for ch in reference.chars() {
        out.push(if ch.is_ascii_alphanumeric() { ch } else { '_' });
    }
    out
}

fn exact_version(spec: &ProviderRef) -> Option<String> {
    let value = spec.target.split_once("#version=")?.1;
    (!value.is_empty()).then(|| value.to_string())
}

fn validate_library_name(name: &str) -> Result<(), ProviderError> {
    let path = Path::new(name);
    if name.is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
        || name.contains(['/', '\\'])
    {
        return Err(ProviderError::ForeignProjection(format!(
            "foreign library name {name} is not one path component"
        )));
    }
    Ok(())
}

fn require_directory(path: &Path, label: &str) -> Result<(), ProviderError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        ProviderError::ForeignProjection(format!(
            "{label} {} is unavailable: {error}",
            path.display()
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(ProviderError::ForeignProjection(format!(
            "{label} {} is not a real directory",
            path.display()
        )));
    }
    Ok(())
}

fn require_regular_file(path: &Path, label: &str) -> Result<(), ProviderError> {
    let metadata = fs::symlink_metadata(path).map_err(|error| {
        ProviderError::ForeignProjection(format!(
            "{label} {} is unavailable: {error}",
            path.display()
        ))
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ProviderError::ForeignProjection(format!(
            "{label} {} is not a regular file",
            path.display()
        )));
    }
    Ok(())
}

fn copy_verified_file(source: &Path, destination: &Path) -> Result<(), ProviderError> {
    let bytes = fs::read(source).map_err(|error| {
        ProviderError::ForeignProjection(format!(
            "could not read verified binding {}: {error}",
            source.display()
        ))
    })?;
    if bytes.is_empty() {
        return Err(ProviderError::ForeignProjection(format!(
            "verified binding {} is empty",
            source.display()
        )));
    }
    write_verified_file(destination, &bytes)
}

fn write_verified_file(destination: &Path, bytes: &[u8]) -> Result<(), ProviderError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| {
            ProviderError::ForeignProjection(format!(
                "could not create binding directory {}: {error}",
                parent.display()
            ))
        })?;
    }
    if let Ok(metadata) = fs::symlink_metadata(destination) {
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err(ProviderError::ForeignProjection(format!(
                "binding destination {} is not a regular file",
                destination.display()
            )));
        }
    }
    let file_name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            ProviderError::ForeignProjection(format!(
                "binding destination {} has no valid file name",
                destination.display()
            ))
        })?;
    let digest = SHA256::sha256_hex(bytes.as_ref());
    let temporary = destination.with_file_name(format!(
        ".{file_name}.{}-{}.tmp",
        std::process::id(),
        &digest[..12]
    ));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .map_err(|error| {
            ProviderError::ForeignProjection(format!(
                "could not stage binding {}: {error}",
                temporary.display()
            ))
        })?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        let _ = fs::remove_file(&temporary);
        return Err(ProviderError::ForeignProjection(format!(
            "could not write binding {}: {error}",
            destination.display()
        )));
    }
    if let Err(error) = fs::rename(&temporary, destination) {
        let _ = fs::remove_file(&temporary);
        return Err(ProviderError::ForeignProjection(format!(
            "could not publish binding {}: {error}",
            destination.display()
        )));
    }
    Ok(())
}
