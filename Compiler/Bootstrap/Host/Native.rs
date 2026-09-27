// Native half of the private source-coupled compiler bootstrap.
//
// The Jet half (`Entry.jet`) is linked by the private source-coupled host
// artifact, outside the public source inventory. This half owns the
// filesystem authority lease, the ambient native Prelude scope, and the
// source-coupled carrier codec; it never parses Jet source or invokes the Rust
// front end as a semantic fallback.

use jet_driver::Authority::{
    AuthorityError, AuthorityKind, AuthorityResolver, CheckedFile, FileIdentity,
};
use crate::Codegen::MIRRust::{
    MirRustAotMetadata, MirRustCallableMetadata, MirRustConfig, MirRustTraitMetadata,
    MirRustTraitMethodMetadata, MirRustVariantMetadata,
};
use jet_foundation::MIR::{MirFieldId, MirProgram, MirType, MirTypeId};
use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::path::Path;
#[path = "NativeAdapter.rs"]
mod compiler_bootstrap_native_adapter;
use self::compiler_bootstrap_native_adapter::emit_bootstrap_native_adapter_impl;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AuthorizedSourceFile {
    pub(crate) path: String,
    pub(crate) relative_path: String,
    pub(crate) identity: String,
    pub(crate) source: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AuthorizedSourceRoot {
    pub(crate) canonical_path: String,
    pub(crate) identity: String,
    pub(crate) allow_hardlinks: bool,
    pub(crate) files: Vec<AuthorizedSourceFile>,
    pub(crate) foreign_cache_files: Vec<AuthorizedSourceFile>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AuthorizedSourceSnapshot {
    pub(crate) roots: Vec<AuthorizedSourceRoot>,
    pub(crate) entry_root_identity: String,
    pub(crate) entry_path: String,
}

/// Holds the checked handles for one authorized source root.
struct AuthorizedSourceRootLease {
    resolver: AuthorityResolver,
    root: crate::Authority::CheckedDirectory,
    manifest: CheckedFile,
    files: Vec<CheckedFile>,
    foreign_cache_files: Vec<CheckedFile>,
}

/// Holds the checked handles for the complete source snapshot until the
/// generated Jet callable has returned. Keeping the handles alive is part of
/// the authority proof: source bytes are not detached from the objects that
/// were checked before the call.
pub(crate) struct AuthorizedSourceLease {
    roots: Vec<AuthorizedSourceRootLease>,
    snapshot: AuthorizedSourceSnapshot,
}
impl AuthorizedSourceLease {
    pub(crate) fn snapshot(&self) -> &AuthorizedSourceSnapshot {
        &self.snapshot
    }

    /// Revalidate every checked source root, manifest, and file immediately
    /// before a generated Jet call.
    pub(crate) fn revalidate(&self) -> Result<(), AuthorityError> {
        for root in &self.roots {
            root.resolver.revalidate_directory(&root.root)?;
            root.resolver.revalidate_file(&root.manifest)?;
            for file in &root.files {
                root.resolver.revalidate_file(file)?;
            }
            for file in &root.foreign_cache_files {
                root.resolver.revalidate_file(file)?;
            }
            root.resolver.revalidate_root()?;
        }
        Ok(())
    }
}
struct AuthorizedRootRecord {
    lease: AuthorizedSourceRootLease,
    snapshot: AuthorizedSourceRoot,
}

/// Open one or more descriptor-relative source authorities. The caller names
/// candidate roots but does not select dependencies; Driver receives every
/// checked root and performs package/path/version matching from the snapshot.
pub(crate) fn open_authorized_sources(
    root: &Path,
    entry: &Path,
) -> Result<AuthorizedSourceLease, AuthorityError> {
    open_authorized_source_candidates(&[root], 0, entry)
}

pub(crate) fn open_authorized_source_candidates(
    roots: &[&Path],
    entry_root_index: usize,
    entry: &Path,
) -> Result<AuthorizedSourceLease, AuthorityError> {
    if roots.is_empty() {
        return Err(AuthorityError::Invalid {
            path: entry.to_path_buf(),
            detail: "the native source snapshot has no candidate authority roots".to_string(),
        });
    }
    if entry_root_index >= roots.len() {
        return Err(AuthorityError::Invalid {
            path: entry.to_path_buf(),
            detail: "the selected entry root is outside the candidate authority roots".to_string(),
        });
    }

    let mut records = Vec::with_capacity(roots.len());
    let mut entry_root_identity = None;
    let mut entry_path = None;
    for (root_index, root_path) in roots.iter().enumerate() {
        let resolver = AuthorityResolver::open(root_path)?;
        let root_directory = resolver.checked_directory(Path::new("."))?;
        // This checks both the canonical package.jet and the retired jet.toml
        // marker before either can enter the source snapshot.
        let manifest = resolver.checked_manifest(Path::new("."))?.file;
        let mut files = resolver.discover_source_files()?;
        let checked_entry = if root_index == entry_root_index {
            Some(resolver.checked_file(entry)?)
        } else {
            None
        };
        let entry_file = if let Some(checked) = checked_entry.as_ref() {
            let file = files
                .iter()
                .find(|file| file.relative == checked.relative)
                .ok_or_else(|| AuthorityError::Invalid {
                    path: checked.path.clone(),
                    detail: "the selected entry is not present in the complete authorized source walk"
                        .to_string(),
                })?;
            if file.identity != checked.identity || file.bytes != checked.bytes {
                return Err(AuthorityError::Changed(checked.path.clone()));
            }
            Some(file.relative.clone())
        } else {
            None
        };

        if !files
            .iter()
            .any(|file| file.relative == manifest.relative)
        {
            files.push(manifest.clone());
            files.sort_by(|left, right| left.relative.cmp(&right.relative));
        }
        let foreign_cache_files =
            collect_authorized_foreign_cache_files(&resolver, &files)?;
        let mut authorized_files = Vec::with_capacity(files.len());
        for file in &files {
            resolver.revalidate_file(file)?;
            authorized_files.push(authorized_source_file(file)?);
        }
        let mut authorized_foreign_cache_files =
            Vec::with_capacity(foreign_cache_files.len());
        for file in &foreign_cache_files {
            resolver.revalidate_file(file)?;
            authorized_foreign_cache_files.push(authorized_source_file(file)?);
        }
        resolver.revalidate_file(&manifest)?;
        resolver.revalidate_root()?;

        let root_identity = identity_key(&root_directory.identity);
        let canonical_path = root_directory.path.display().to_string();
        if root_index == entry_root_index {
            let relative = entry_file.ok_or_else(|| AuthorityError::Invalid {
                path: entry.to_path_buf(),
                detail: "the selected entry root did not produce an entry file".to_string(),
            })?;
            let selected = authorized_files
                .iter()
                .find(|file| file.relative_path == relative)
                .ok_or_else(|| AuthorityError::Invalid {
                    path: entry.to_path_buf(),
                    detail: "the selected entry was lost while building the authorized snapshot"
                        .to_string(),
                })?;
            entry_root_identity = Some(root_identity.clone());
            entry_path = Some(selected.path.clone());
        }
        records.push(AuthorizedRootRecord {
            lease: AuthorizedSourceRootLease {
                resolver,
                root: root_directory,
                manifest,
                files,
                foreign_cache_files,
            },
            snapshot: AuthorizedSourceRoot {
                canonical_path,
                identity: root_identity,
                allow_hardlinks: false,
                files: authorized_files,
                foreign_cache_files: authorized_foreign_cache_files,
            },
        });
    }


    records.sort_by(|left, right| {
        left.snapshot
            .canonical_path
            .cmp(&right.snapshot.canonical_path)
    });
    for pair in records.windows(2) {
        if pair[0].snapshot.canonical_path == pair[1].snapshot.canonical_path {
            return Err(AuthorityError::Invalid {
                path: Path::new(&pair[1].snapshot.canonical_path).to_path_buf(),
                detail: "candidate authority roots must be physically distinct".to_string(),
            });
        }
    }
    let snapshot = AuthorizedSourceSnapshot {
        roots: records
            .iter()
            .map(|record| record.snapshot.clone())
            .collect(),
        entry_root_identity: entry_root_identity.ok_or_else(|| AuthorityError::Invalid {
            path: entry.to_path_buf(),
            detail: "the selected entry root was not recorded".to_string(),
        })?,
        entry_path: entry_path.ok_or_else(|| AuthorityError::Invalid {
            path: entry.to_path_buf(),
            detail: "the selected entry path was not recorded".to_string(),
        })?,
    };
    let roots = records.into_iter().map(|record| record.lease).collect();
    Ok(AuthorizedSourceLease { roots, snapshot })
}

fn authorized_source_file(file: &CheckedFile) -> Result<AuthorizedSourceFile, AuthorityError> {
    Ok(AuthorizedSourceFile {
        path: file.path.display().to_string(),
        relative_path: relative_text(&file.relative),
        identity: identity_key(&file.identity),
        source: file.text()?,
    })
}
fn append_authorized_foreign_cache_file(
    foreign_cache_files: &mut Vec<CheckedFile>,
    file: CheckedFile,
    total_files: &mut usize,
    total_bytes: &mut u64,
) -> Result<(), AuthorityError> {
    let next_file_count = (*total_files).saturating_add(1);
    let next_byte_count = (*total_bytes).saturating_add(file.bytes.len() as u64);
    if next_file_count > jet_foundation::SHA256::MAX_TREE_FILES
        || next_byte_count > jet_foundation::SHA256::MAX_TREE_TOTAL_BYTES
    {
        return Err(AuthorityError::Invalid {
            path: file.path.clone(),
            detail: "authorized source and foreign cache snapshot exceeds the authority tree read bounds"
                .to_string(),
        });
    }
    *total_files = next_file_count;
    *total_bytes = next_byte_count;
    foreign_cache_files.push(file);
    Ok(())
}

fn collect_authorized_foreign_cache_files(
    resolver: &AuthorityResolver,
    source_files: &[CheckedFile],
) -> Result<Vec<CheckedFile>, AuthorityError> {
    let mut foreign_cache_files = Vec::new();
    let mut total_files = source_files.len();
    let mut total_bytes = source_files
        .iter()
        .fold(0u64, |total, file| total.saturating_add(file.bytes.len() as u64));
    for cache_directory in [
        Path::new(".jet/bindings/c"),
        Path::new(".jet/bindings/cpp"),
    ] {
        let cache_files = match resolver.discover_files(cache_directory, Some("jet")) {
            Ok(files) => files,
            Err(error) if error.is_missing() => continue,
            Err(error) => return Err(error),
        };
        for cache in cache_files {
            if cache
                .relative
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".adapted.jet"))
            {
                continue;
            }
            for extension in ["metadata", "provenance"] {
                let sidecar = cache.relative.with_extension(extension);
                match resolver.checked_file(&sidecar) {
                    Ok(file) => append_authorized_foreign_cache_file(
                        &mut foreign_cache_files,
                        file,
                        &mut total_files,
                        &mut total_bytes,
                    )?,
                    Err(error) if error.is_missing() => {}
                    Err(error) => return Err(error),
                }
            }
            append_authorized_foreign_cache_file(
                &mut foreign_cache_files,
                cache,
                &mut total_files,
                &mut total_bytes,
            )?;
        }
    }
    foreign_cache_files.sort_by(|left, right| left.relative.cmp(&right.relative));
    Ok(foreign_cache_files)
}

fn relative_text(path: &Path) -> String {
    let text = path.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/");
    if text.is_empty() {
        ".".to_string()
    } else {
        text
    }
}

fn identity_key(identity: &FileIdentity) -> String {
    let kind = match identity.kind {
        AuthorityKind::File => "file",
        AuthorityKind::Directory => "directory",
    };
    let modified = identity
        .modified_ns
        .map_or_else(|| "none".to_string(), |value| value.to_string());
    let device = identity
        .device
        .map_or_else(|| "none".to_string(), |value| value.to_string());
    let inode = identity
        .inode
        .map_or_else(|| "none".to_string(), |value| value.to_string());
    format!(
        "authority-file-v1;kind={kind};length={};modified_ns={modified};device={device};inode={inode}",
        identity.length
    )
}

/// Install the one canonical native Prelude adapter around a generated
/// callable. The adapter itself remains owned by Codegen; Host only installs
/// the existing ambient hook and never duplicates its route table.
pub(crate) fn with_native_prelude<R>(body: impl FnOnce() -> R) -> R {
    crate::Comptime::with_ambient_mir_prelude(
        Some(crate::Codegen::NativePreludeBridge::ambient_call),
        body,
    )
}

/// Revalidate the lease, install the canonical native adapter, and invoke the
/// source-coupled callable. The callable is deliberately generic so the
/// generated Rust artifact owns its exact private request/result types.
pub(crate) fn invoke_with_authority<R>(
    lease: &AuthorizedSourceLease,
    invoke: impl FnOnce(&AuthorizedSourceSnapshot) -> R,
) -> Result<R, AuthorityError> {
    lease.revalidate()?;
    Ok(with_native_prelude(|| invoke(lease.snapshot())))
}

/// Failure while binding the private generated callback to the exact carriers
/// emitted for the same MIR artifact. These failures are deliberately raised
/// before mutating the generated source buffer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum BootstrapHostCodecError {
    MissingType(String),
    MissingField { owner: String, field: String },
    MissingVariant { owner: String, variant: String },
    MissingEntry(String),
    InvalidMetadata(String),
}

impl fmt::Display for BootstrapHostCodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingType(name) => write!(formatter, "bootstrap codec type `{name}` is not emitted"),
            Self::MissingField { owner, field } => {
                write!(formatter, "bootstrap codec field `{owner}.{field}` is not emitted")
            }
            Self::MissingVariant { owner, variant } => {
                write!(formatter, "bootstrap codec variant `{owner}::{variant}` is not emitted")
            }
            Self::MissingEntry(name) => write!(formatter, "bootstrap codec entry `{name}` is not emitted"),
            Self::InvalidMetadata(detail) => write!(formatter, "invalid bootstrap codec metadata: {detail}"),
        }
    }
}
/// Source-coupled bindings emitted by one Rust artifact.
///
/// The ordinary Rust emitter and the Jet emitter expose the same four row
/// families. The initial bootstrap stage adapts `MirRustAotMetadata` here;
/// the self-emitted stage supplies the same rows from its generated manifest.
/// Host glue only consumes these mechanical facts and never infers semantics.
#[derive(Clone, Debug)]
pub(crate) struct BootstrapBindingDescriptor {
    pub(crate) callables: Vec<BootstrapCallableBinding>,
    pub(crate) types: Vec<BootstrapTypeBinding>,
    pub(crate) fields: Vec<BootstrapFieldBinding>,
    pub(crate) variants: Vec<MirRustVariantMetadata>,
    pub(crate) traits: Vec<MirRustTraitMetadata>,
    pub(crate) trait_methods: Vec<MirRustTraitMethodMetadata>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BootstrapCallableBinding {
    pub(crate) source_name: String,
    pub(crate) metadata: MirRustCallableMetadata,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BootstrapTypeBinding {
    pub(crate) ty: MirTypeId,
    pub(crate) source_name: String,
    pub(crate) symbol: String,
}

#[derive(Clone, Debug)]
pub(crate) struct BootstrapFieldBinding {
    pub(crate) field: MirFieldId,
    pub(crate) owner: MirTypeId,
    pub(crate) source_name: String,
    pub(crate) symbol: String,
    pub(crate) ty: MirType,
}

impl BootstrapBindingDescriptor {
    pub(crate) fn from_aot(
        program: &MirProgram,
        metadata: &MirRustAotMetadata,
    ) -> Result<Self, BootstrapHostCodecError> {
        let callables = metadata
            .callables
            .iter()
            .map(|row| {
                let source_name = program
                    .functions
                    .iter()
                    .find(|function| function.id == row.function)
                    .map(|function| function.name.clone())
                    .ok_or_else(|| {
                        BootstrapHostCodecError::InvalidMetadata(format!(
                            "Rust metadata callable {:?} is absent from the MIR program",
                            row.function
                        ))
                    })?;
                Ok((source_name, row.clone()))
            })
            .collect::<Result<Vec<_>, BootstrapHostCodecError>>()?;
        let types = metadata
            .types
            .iter()
            .map(|row| {
                let source_name = program
                    .types
                    .iter()
                    .find(|definition| definition.id == row.ty)
                    .map(|definition| definition.name.clone())
                    .ok_or_else(|| {
                        BootstrapHostCodecError::InvalidMetadata(format!(
                            "Rust metadata type {:?} is absent from the MIR program",
                            row.ty
                        ))
                    })?;
                Ok((row.ty, source_name, row.symbol.clone()))
            })
            .collect::<Result<Vec<_>, BootstrapHostCodecError>>()?;
        let fields = metadata
            .fields
            .iter()
            .map(|row| {
                let field = program
                    .fields
                    .iter()
                    .find(|field| field.id == row.field && field.owner == row.owner)
                    .ok_or_else(|| {
                        BootstrapHostCodecError::InvalidMetadata(format!(
                            "Rust metadata field {:?} is absent from the MIR program",
                            row.field
                        ))
                    })?;
                Ok((
                    row.field,
                    row.owner,
                    field.field.name.clone(),
                    row.symbol.clone(),
                    field.field.ty.clone(),
                ))
            })
            .collect::<Result<Vec<_>, BootstrapHostCodecError>>()?;
        Ok(Self::from_rows(
            callables,
            types,
            fields,
            metadata.variants.clone(),
            metadata.traits.clone(),
            metadata.trait_methods.clone(),
        ))
    }

    pub(crate) fn from_rows(
        callables: Vec<(String, MirRustCallableMetadata)>,
        types: Vec<(MirTypeId, String, String)>,
        fields: Vec<(MirFieldId, MirTypeId, String, String, MirType)>,
        variants: Vec<MirRustVariantMetadata>,
        traits: Vec<MirRustTraitMetadata>,
        trait_methods: Vec<MirRustTraitMethodMetadata>,
    ) -> Self {
        Self {
            callables: callables
                .into_iter()
                .map(|(source_name, metadata)| BootstrapCallableBinding {
                    source_name,
                    metadata,
                })
                .collect(),
            types: types
                .into_iter()
                .map(|(ty, source_name, symbol)| BootstrapTypeBinding {
                    ty,
                    source_name,
                    symbol,
                })
                .collect(),
            fields: fields
                .into_iter()
                .map(|(field, owner, source_name, symbol, ty)| BootstrapFieldBinding {
                    field,
                    owner,
                    source_name,
                    symbol,
                    ty,
                })
                .collect(),
            variants,
            traits,
            trait_methods,
        }
    }
}



pub(crate) struct BootstrapCodecSymbols<'a> {
    metadata: &'a BootstrapBindingDescriptor,
    types: BTreeMap<&'a str, &'a str>,
}

impl<'a> BootstrapCodecSymbols<'a> {
    pub(crate) fn new(metadata: &'a BootstrapBindingDescriptor) -> Result<Self, BootstrapHostCodecError> {
        let mut types = BTreeMap::new();
        for row in &metadata.types {
            types.insert(row.source_name.as_str(), row.symbol.as_str());
        }
        Ok(Self { metadata, types })
    }

    pub(crate) fn type_symbol(&self, name: &str) -> Result<&str, BootstrapHostCodecError> {
        self.types
            .get(name)
            .copied()
            .ok_or_else(|| BootstrapHostCodecError::MissingType(name.to_string()))
    }

    fn definition_id(&self, name: &str) -> Result<MirTypeId, BootstrapHostCodecError> {
        self.metadata
            .types
            .iter()
            .find(|row| row.source_name == name)
            .map(|row| row.ty)
            .ok_or_else(|| BootstrapHostCodecError::MissingType(name.to_string()))
    }

    pub(crate) fn field_symbol(
        &self,
        owner: &str,
        field: &str,
    ) -> Result<&str, BootstrapHostCodecError> {
        self.field_binding(owner, field)
            .map(|row| row.symbol.as_str())
    }

    pub(crate) fn field_binding(
        &self,
        owner: &str,
        field: &str,
    ) -> Result<&BootstrapFieldBinding, BootstrapHostCodecError> {
        let owner_id = self.definition_id(owner)?;
        self.metadata
            .fields
            .iter()
            .find(|row| row.owner == owner_id && row.source_name == field)
            .ok_or_else(|| BootstrapHostCodecError::MissingField {
                owner: owner.to_string(),
                field: field.to_string(),
            })
    }

    fn variant_symbol(&self, owner: &str, variant: &str) -> Result<&str, BootstrapHostCodecError> {
        let owner_id = self.definition_id(owner)?;
        self.metadata
            .variants
            .iter()
            .find(|row| row.owner == owner_id && row.source_name == variant)
            .map(|row| row.symbol.as_str())
            .ok_or_else(|| BootstrapHostCodecError::MissingVariant {
                owner: owner.to_string(),
                variant: variant.to_string(),
            })
    }

    pub(crate) fn variant_path(
        &self,
        owner: &str,
        variant: &str,
    ) -> Result<String, BootstrapHostCodecError> {
        Ok(self.variant_symbol(owner, variant)?.to_string())
    }
    pub(crate) fn trait_symbol(&self, name: &str) -> Result<&str, BootstrapHostCodecError> {
        self.metadata
            .traits
            .iter()
            .find(|row| row.name == name)
            .map(|row| row.symbol.as_str())
            .ok_or_else(|| BootstrapHostCodecError::MissingEntry(format!("trait `{name}`")))
    }

    pub(crate) fn trait_method_metadata(
        &self,
        trait_name: &str,
        method_name: &str,
    ) -> Result<&MirRustTraitMethodMetadata, BootstrapHostCodecError> {
        let trait_id = self
            .metadata
            .traits
            .iter()
            .find(|row| row.name == trait_name)
            .map(|row| row.trait_id)
            .ok_or_else(|| BootstrapHostCodecError::MissingEntry(format!("trait `{trait_name}`")))?;
        self.metadata
            .trait_methods
            .iter()
            .find(|row| row.trait_id == trait_id && row.name == method_name)
            .ok_or_else(|| {
                BootstrapHostCodecError::MissingEntry(format!(
                    "trait method `{trait_name}.{method_name}`"
                ))
            })
    }
}

fn bootstrap_required_type_names() -> &'static [&'static str] {
    &[
        "MirPreludeCall",
        "MirCallSignature",
        "MirSymbol",
        "MirType",
        "MirTypeKind",
        "MirTypeId",
        "MirMeasure",
        "MirMeasureRule",
        "MirDimension",
        "MirDimensionAxis",
        "MirLayout",
        "MirAbi",
        "MirScalarKind",
        "MirSize",
        "MirTraitId",
        "MirTraitMethodId",
        "MirArtifactId",
        "MirProgram",
        "MirNominalRef",
        "MirTupleField",
        "MirTagMarker",
        "MirInternalTag",
        "MirAccess",
        "MirFunctionId",
        "MirFieldId",
        "MirParam",
        "MirOwnership",
        "MirOwnershipMode",
        "MirDropKind",
        "MirForeign",
        "MirForeignAbi",
        "MirForeignLanguage",
        "MirTargetApplicability",
        "MirEffectFacts",
        "MirNamedSpan",
        "MirModuleId",
        "MirForeignId",
        "MirLinkUnitId",
        "MirCallbackId",
        "MirHandleId",
        "JetEvalForeignWriteback",
        "TComptimeValue",
        "JetEvalRuntimeValue",
        "JetEvalHostArgument",
        "JetEvalRuntimeAggregate",
        "JetEvalRuntimeMapEntry",
        "JetEvalRuntimeField",
        "JetEvalRuntimeEnumArg",
        "JetEvalHostResult",
        "JetEvalCallbackOutcome",
        "JetEvalCallbackResult",
        "JetEvalError",
        "JetEvalHostOutcome",
        "JetEvalHostTransfer",
        "JetEvalHostProjection",
        "JetEvalHostValue",
        "JetEvalHostMapEntry",
        "JetEvalHostField",
        "JetEvalHostEnumArg",
        "JetEvalHostOwner",
        "JetEvalHostTypeFieldShape",
        "JetEvalHostTypeArgShape",
        "JetEvalHostTypeVariantShape",
        "JetEvalHostTypeNode",
        "JetEvalHostTypeShape",
        "JetEvalErrorKind",
        "JetEvalInternalProblem",
        "JetEvalMachine",
        "JetEvalConfig",
        "JetDriverCompileResult",
        "JetDriverCompileRequest",
        "JetDriverCompileTarget",
        "JetRustEmitManifest",
        "JetRustEmitCallableMetadata",
        "JetRustEmitFieldMetadata",
        "JetRustEmitVariantMetadata",
        "JetRustEmitTypeMetadata",
        "Diagnostic",
        "TextEdit",
        "Span",
        "JetRustEmitTraitMetadata",
        "JetRustEmitTraitMethodMetadata",
        "JetWebAssets",
        "JetWebTimezoneAsset",
        "JetWebEmitConfig",
        "JetWebEmitFailureKind",
        "JetWebEmitFailure",
        "JetWebEmitResult",
        "JetWebArtifacts",
        "JetReleaseDevtoolsPolicy",
        "JetTargetLayout",
        "MirArtifactBuildMode",
        "JetEvalHostCoreOwner",
        "MirCoreOwner",
"MirSourceFile",
    ]
}

fn bootstrap_required_callable<'a>(
    metadata: &'a BootstrapBindingDescriptor,
    name: &str,
) -> Result<&'a MirRustCallableMetadata, BootstrapHostCodecError> {
    metadata
        .callables
        .iter()
        .find(|callable| callable.source_name == name)
        .map(|callable| &callable.metadata)
        .ok_or_else(|| BootstrapHostCodecError::MissingEntry(name.to_string()))
}

fn validate_bootstrap_numeric_callable_type(
    field: &BootstrapFieldBinding,
) -> Result<(), BootstrapHostCodecError> {
    let callback = match &field.ty.kind {
        jet_foundation::MIR::MirTypeKind::Option(callback) => callback,
        _ => {
            return Err(BootstrapHostCodecError::InvalidMetadata(
                "JetEvalConfig.numeric_unit_conversion_exact must be an optional checked function type"
                    .to_string(),
            ));
        }
    };
    let signature = callback.function_signature().ok_or_else(|| {
        BootstrapHostCodecError::InvalidMetadata(
            "JetEvalConfig.numeric_unit_conversion_exact must contain a checked Fn type".to_string(),
        )
    })?;
    let parameter_types = signature
        .params
        .iter()
        .map(MirType::canonical_key)
        .collect::<Vec<_>>();
    let expected = ["Float", "String", "String", "String", "String"];
    if parameter_types.len() != expected.len()
        || parameter_types
            .iter()
            .zip(expected)
            .any(|(actual, expected)| actual != expected)
    {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "JetEvalConfig.numeric_unit_conversion_exact must have its exact checked (Float, String, String, String, String) parameter types"
                .to_string(),
        ));
    }
    let returns_optional_float = matches!(
        signature.ret.as_deref().and_then(MirType::option_inner),
        Some(output) if output.canonical_key() == "Float"
    );
    if !returns_optional_float {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "JetEvalConfig.numeric_unit_conversion_exact must return the exact checked optional Float"
                .to_string(),
        ));
    }
    Ok(())
}


/// Append the private callback/entry splice using one binding descriptor
/// mechanically adapted from either initial Rust metadata or the generated
/// Jet manifest.
pub(crate) fn append_bootstrap_host_glue(
    source: &mut String,
    config: &MirRustConfig<'_>,
    bindings: &BootstrapBindingDescriptor,
    program: &MirProgram,
) -> Result<(), BootstrapHostCodecError> {
    let symbols = BootstrapCodecSymbols::new(bindings)?;
    validate_bootstrap_numeric_callable_type(
        symbols.field_binding("JetEvalConfig", "numeric_unit_conversion_exact")?,
    )?;
    for name in bootstrap_required_type_names() {
        let _ = symbols.type_symbol(name)?;
    }
    let entry = bootstrap_required_callable(bindings, "jet_bootstrap_compile")?;
    if entry.parameter_types.len() != 1 || entry.parameter_access.len() != 1 {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "jet_bootstrap_compile must retain exactly one owned request parameter".to_string(),
        ));
    }
    if entry.parameter_types[0].starts_with('&') {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "jet_bootstrap_compile request must be an owned parameter".to_string(),
        ));
    }

    let eval_default = bootstrap_required_callable(bindings, "jet_eval_default_config")?;
    if !eval_default.parameter_types.is_empty() || !eval_default.parameter_access.is_empty() {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "jet_eval_default_config must be a zero-parameter callable".to_string(),
        ));
    }
    let eval_runtime_config = bootstrap_required_callable(bindings, "jet_eval_runtime_config")?;
    if eval_runtime_config.parameter_types.len() != 1
        || eval_runtime_config.parameter_access.as_slice()
            != [jet_foundation::MIR::MirAccess::Read]
    {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "jet_eval_runtime_config must be a one-parameter Read fork".to_string(),
        ));
    }

    let mut glue = String::new();
    emit_bootstrap_type_codec(&mut glue, &symbols)?;
    emit_bootstrap_value_codec(&mut glue, &symbols)?;
    emit_bootstrap_host_value_codec(&mut glue, &symbols)?;
    emit_bootstrap_host_type_shape_codec(&mut glue, &symbols)?;
    crate::compiler_bootstrap_diagnostic_codec::append_diagnostic_codec(&mut glue, &symbols)?;
    crate::compiler_bootstrap_runtime_mir_codec::append_runtime_mir_codec(&mut glue, &symbols)?;
    crate::compiler_bootstrap_entry_codec::append_bootstrap_entry_codec(
        &mut glue,
        &symbols,
        program,
        entry.metadata.function,
    )?;
    emit_bootstrap_source_resource_bridge(&mut glue, &symbols)?;
    emit_bootstrap_callback(&mut glue, &symbols)?;
    emit_bootstrap_foreign_callback(&mut glue, &symbols)?;
    emit_bootstrap_native_adapter_impl(&mut glue, &symbols)?;
    emit_bootstrap_web_asset_provider(&mut glue, &symbols)?;

    emit_bootstrap_manifest_adapter(&mut glue, &symbols)?;
    emit_bootstrap_host_factory(
        &mut glue,
        &symbols,
        &eval_default.symbol,
        &eval_runtime_config.symbol,
    )?;

    let eval_config_host_adapter_field = symbols.field_symbol("JetEvalConfig", "host_adapter")?;
    let eval_config_numeric_unit_field =
        symbols.field_symbol("JetEvalConfig", "numeric_unit_conversion_exact")?;
    let eval_config_type = symbols.type_symbol("JetEvalConfig")?;
    writeln!(
        glue,
        "fn __jet_bootstrap_numeric_unit_conversion_exact(\n\
             value: f64,\n\
             scale_num: String,\n\
             scale_den: String,\n\
             offset_num: String,\n\
             offset_den: String,\n\
         ) -> Result<f64, ::jet_foundation::Outcome::JetAbsent> {{\n\
             ::jet_foundation::jet_unit_conversion_exact(\n\
                 value,\n\
                 &scale_num,\n\
                 &scale_den,\n\
                 &offset_num,\n\
                 &offset_den,\n\
             )\n\
             .ok_or(::jet_foundation::Outcome::JetAbsent)\n\
         }}\n",
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    let request_type = &entry.parameter_types[0];
    let result_type = &entry.return_type;
    let entry_symbol = &entry.symbol;
    let runtime_config_symbol = &eval_runtime_config.symbol;
    writeln!(
        glue,
        "#[doc(hidden)]\n\
         struct __JetBootstrapCompileLease {{\n\
             result: {result_type},\n\
             resources: ::jet_jit::SourceResources::SourceResourceSession,\n\
             runtime_config: {eval_config_type},\n\
         }}\n\
         #[doc(hidden)]\n\
         pub fn __jet_bootstrap_compile_with_native(\n\
             mut __jet_request: {request_type},\n\
         ) -> Result<__JetBootstrapCompileLease, crate::BootstrapHostCodecError> {{\n\
             let __jet_resources = ::jet_jit::SourceResources::SourceResourceSession::new();\n\
             let __jet_root_lease = __jet_resources.retain_root()\n\
                 .map_err(crate::BootstrapHostCodecError::InvalidMetadata)?;\n\
             __jet_request.{request_eval_config}.{eval_config_host_adapter_field} = Ok(Box::new(__JetBootstrapNativeAdapter::new(__jet_root_lease)));\n\
             __jet_request.{request_eval_config}.{eval_config_numeric_unit_field} = Ok(__jet_bootstrap_numeric_unit_conversion_exact);\n\
             let __jet_runtime_config = {runtime_config_symbol}(&__jet_request.{request_eval_config});\n\
             let __jet_result = {{\n\
                 let _activation = __jet_resources.activate();\n\
                 {entry_symbol}(__jet_request)\n\
             }};\n\
             Ok(__JetBootstrapCompileLease {{ result: __jet_result, resources: __jet_resources, runtime_config: __jet_runtime_config }})\n\
         }}\n",
        request_eval_config = symbols.field_symbol("JetDriverCompileRequest", "eval_config")?,
        eval_config_host_adapter_field = eval_config_host_adapter_field,
        eval_config_numeric_unit_field = eval_config_numeric_unit_field,
        eval_config_type = eval_config_type,
        runtime_config_symbol = runtime_config_symbol,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    // `root_prefix` is deliberately read here so a future source-coupled
    // emitter can reject an incompatible root before this private splice is
    // linked. The current bootstrap host is linked as the compiler crate.

    if config.root_prefix.is_empty() {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "generated Rust root prefix is empty".to_string(),
        ));
    }
    source.push_str(&glue);
    Ok(())

}

fn emit_bootstrap_host_factory(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
    default_symbol: &str,
    runtime_config_symbol: &str,
) -> Result<(), BootstrapHostCodecError> {
    let request_type = symbols.type_symbol("JetDriverCompileRequest")?;
    let request_sources = symbols.field_symbol("JetDriverCompileRequest", "authorized_sources")?;
    let request_host_facts = symbols.field_symbol("JetDriverCompileRequest", "host_facts")?;
    let request_eval_config = symbols.field_symbol("JetDriverCompileRequest", "eval_config")?;
    let request_target = symbols.field_symbol("JetDriverCompileRequest", "target")?;
    let request_effect_source = symbols.field_symbol("JetDriverCompileRequest", "canonical_effect_source")?;
    let target_type = symbols.type_symbol("JetDriverCompileTarget")?;
    let mir_program_type = symbols.type_symbol("MirProgram")?;
    let eval_config_type = symbols.type_symbol("JetEvalConfig")?;
    let snapshot_type = symbols.type_symbol("JetDriverAuthorizedSourceSnapshot")?;
    let snapshot_roots = symbols.field_symbol(
        "JetDriverAuthorizedSourceSnapshot",
        "roots",
    )?;
    let snapshot_entry_root = symbols.field_symbol(
        "JetDriverAuthorizedSourceSnapshot",
        "entry_root_identity",
    )?;
    let snapshot_entry_path =
        symbols.field_symbol("JetDriverAuthorizedSourceSnapshot", "entry_path")?;
    let root_type = symbols.type_symbol("JetDriverAuthorizedSourceRoot")?;
    let root_canonical_path =
        symbols.field_symbol("JetDriverAuthorizedSourceRoot", "canonical_path")?;
    let root_identity = symbols.field_symbol("JetDriverAuthorizedSourceRoot", "identity")?;
    let root_allow_hardlinks =
        symbols.field_symbol("JetDriverAuthorizedSourceRoot", "allow_hardlinks")?;
    let root_files = symbols.field_symbol("JetDriverAuthorizedSourceRoot", "files")?;
    let root_foreign_cache_files = symbols.field_symbol(
        "JetDriverAuthorizedSourceRoot",
        "foreign_cache_files",
    )?;
    let file_type = symbols.type_symbol("JetDriverAuthorizedSourceFile")?;
    let file_path = symbols.field_symbol("JetDriverAuthorizedSourceFile", "path")?;
    let file_relative_path =
        symbols.field_symbol("JetDriverAuthorizedSourceFile", "relative_path")?;
    let file_identity = symbols.field_symbol("JetDriverAuthorizedSourceFile", "identity")?;
    let file_source = symbols.field_symbol("JetDriverAuthorizedSourceFile", "source")?;
    let host_facts_type = symbols.type_symbol("JetDriverHostFacts")?;
    let host_target_triple =
        symbols.field_symbol("JetDriverHostFacts", "target_triple")?;
    let host_active_os = symbols.field_symbol("JetDriverHostFacts", "active_os")?;
    let host_compiler_identity =
        symbols.field_symbol("JetDriverHostFacts", "compiler_identity")?;

    writeln!(
        out,
        concat!(
            "#[doc(hidden)]\n",
            "pub(crate) fn __jet_bootstrap_request_from_host(\n",
            "    __jet_snapshot: &crate::compiler_bootstrap_host::AuthorizedSourceSnapshot,\n",
            "    __jet_requested_target: {target_type},\n",
            ") -> Result<{request_type}, crate::BootstrapHostCodecError> {{\n",
            "    let __jet_roots = __jet_snapshot.roots.iter().map(|root| {root_type} {{\n",
            "        {root_canonical_path}: root.canonical_path.clone(),\n",
            "        {root_identity}: root.identity.clone(),\n",
            "        {root_allow_hardlinks}: root.allow_hardlinks,\n",
            "        {root_files}: root.files.iter().map(|file| {file_type} {{\n",
            "            {file_path}: file.path.clone(),\n",
            "            {file_relative_path}: file.relative_path.clone(),\n",
            "            {file_identity}: file.identity.clone(),\n",
            "            {file_source}: file.source.clone(),\n",
            "        }}).collect::<Vec<_>>(),\n",
            "        {root_foreign_cache_files}: root.foreign_cache_files.iter().map(|file| {file_type} {{\n",
            "            {file_path}: file.path.clone(),\n",
            "            {file_relative_path}: file.relative_path.clone(),\n",
            "            {file_identity}: file.identity.clone(),\n",
            "            {file_source}: file.source.clone(),\n",
            "        }}).collect::<Vec<_>>(),\n",
            "    }}).collect::<Vec<_>>();\n",
            "    let __jet_target = __jet_bootstrap_target_with_raw_web_assets(__jet_requested_target)\n",
            "        .map_err(crate::BootstrapHostCodecError::InvalidMetadata)?;\n",
            "    let __jet_eval_config = {default_symbol}();\n",
            "    let __jet_request = {request_type} {{\n",
            "        {request_sources}: {snapshot_type} {{\n",
            "            {snapshot_roots}: __jet_roots,\n",
            "            {snapshot_entry_root}: __jet_snapshot.entry_root_identity.clone(),\n",
            "            {snapshot_entry_path}: __jet_snapshot.entry_path.clone(),\n",
            "        }},\n",
            "        {request_host_facts}: {host_facts_type} {{\n",
            "            {host_target_triple}: Ok(::jet_foundation::Layout::TargetLayout::host_triple()),\n",
            "            {host_active_os}: Ok(::std::env::consts::OS.to_string()),\n",
            "            {host_compiler_identity}: option_env!(\"JET_COMPILER_BUILD_ID\").map(|value| Ok(value.to_string())).unwrap_or_else(|| Err(Default::default())),\n",
            "        }},\n",
            "        {request_effect_source}: ::jet_foundation::Effects::EFFECT_SOURCE.to_string(),\n",
            "        {request_eval_config}: __jet_eval_config,\n",
            "        {request_target}: __jet_target,\n",
            "    }};\n",
            "    Ok(__jet_request)\n",
            "}}\n",
        ),
        request_type = request_type,
        request_sources = request_sources,
        request_host_facts = request_host_facts,
        request_eval_config = request_eval_config,
        request_target = request_target,
        target_type = target_type,
        snapshot_type = snapshot_type,
        snapshot_roots = snapshot_roots,
        snapshot_entry_root = snapshot_entry_root,
        snapshot_entry_path = snapshot_entry_path,
        root_type = root_type,
        root_canonical_path = root_canonical_path,
        root_identity = root_identity,
        root_allow_hardlinks = root_allow_hardlinks,
        root_files = root_files,
        root_foreign_cache_files = root_foreign_cache_files,
        file_type = file_type,
        file_path = file_path,
        file_relative_path = file_relative_path,
        file_identity = file_identity,
        file_source = file_source,
        host_facts_type = host_facts_type,
        host_target_triple = host_target_triple,
        host_active_os = host_active_os,
        request_effect_source = request_effect_source,
        host_compiler_identity = host_compiler_identity,
        default_symbol = default_symbol,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    writeln!(
        out,
        concat!(
            "#[doc(hidden)]\n",
            "pub(crate) fn __jet_bootstrap_compile_from_host(\n",
            "    __jet_snapshot: &crate::compiler_bootstrap_host::AuthorizedSourceSnapshot,\n",
            "    __jet_requested_target: {target_type},\n",
            ") -> Result<crate::BootstrapJetCompileResult<{mir_program_type}, {eval_config_type}>, crate::BootstrapHostCodecError> {{\n",
            "    let __jet_request = __jet_bootstrap_request_from_host(__jet_snapshot, __jet_requested_target)?;\n",
            "    let __jet_result = __jet_bootstrap_compile_with_native(__jet_request)?;\n",
            "    let __jet_output = {{\n",
            "        let _activation = __jet_result.resources.activate();\n",
            "        __jet_bootstrap_bindings_from_result(__jet_result.result, __jet_snapshot, __jet_result.runtime_config)\n",
            "            .map_err(crate::BootstrapHostCodecError::InvalidMetadata)\n",
            "    }};\n",
            "    match __jet_output {{\n",
            "        Ok(mut output) => {{ output.resources = Some(__jet_result.resources); Ok(output) }},\n",
            "        Err(error) => match __jet_result.resources.retire() {{\n",
            "            Ok(()) => Err(error),\n",
            "            Err(retirement) => Err(crate::BootstrapHostCodecError::InvalidMetadata(format!(\"{{error:?}}; Source resource retirement failed: {{retirement}}\"))),\n",
            "        }},\n",
            "    }}\n",
            "}}\n",
        ),
        target_type = target_type,
        mir_program_type = mir_program_type,
        eval_config_type = eval_config_type,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    Ok(())
}


fn emit_bootstrap_web_asset_provider(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let target_type = symbols.type_symbol("JetDriverCompileTarget")?;
    let target_native = symbols.variant_path("JetDriverCompileTarget", "Native")?;
    let target_web = symbols.variant_path("JetDriverCompileTarget", "Web")?;
    let assets_type = symbols.type_symbol("JetWebAssets")?;
    let timezone_type = symbols.type_symbol("JetWebTimezoneAsset")?;
    let timezone_name = symbols.field_symbol("JetWebTimezoneAsset", "name")?;
    let timezone_bytes = symbols.field_symbol("JetWebTimezoneAsset", "bytes")?;
    let asset_fields = [
        "dom_runtime",
        "execution_prelude",
        "event_prelude",
        "realtime_prelude",
        "data_prelude",
        "history_prelude",
        "compute_prelude",
        "raylib_prelude",
        "game_prelude",
        "task_group_prelude",
        "runtime_stop_metadata",
        "game_default_frame_budget",
        "game_frame_budget_error",
        "harfbuzz_wasm",
        "timezone_data",
        "index_html",
        "explicit_html_path",
        "source_names",
        "source_contents",
        "onnx_runtime_js",
        "onnx_runtime_worker_js",
    ]
    .iter()
    .map(|name| Ok::<_, BootstrapHostCodecError>((*name, symbols.field_symbol("JetWebAssets", name)?)))
    .collect::<Result<Vec<_>, _>>()?;
    let field = |name: &str| {
        asset_fields
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, symbol)| symbol.as_str())
            .ok_or_else(|| BootstrapHostCodecError::MissingEntry(name.to_string()))
    };
    writeln!(
        out,
        r#"fn __jet_bootstrap_target_with_raw_web_assets(
            target: {target_type},
        ) -> Result<{target_type}, String> {{
            match target {{
                {target_native} => Ok({target_native}),
                {target_web}(assets, release_policy, profile) => {{
                    let raw = crate::Codegen::canonical_web_raw_assets()
                        .map_err(|error| error.to_string())?;
                    let assets = {assets_type} {{
                        {dom_runtime}: raw.dom_runtime.to_string(),
                        {execution_prelude}: raw.execution_prelude.to_string(),
                        {event_prelude}: raw.event_prelude.to_string(),
                        {realtime_prelude}: raw.realtime_prelude.to_string(),
                        {data_prelude}: raw.data_prelude.to_string(),
                        {history_prelude}: raw.history_prelude.to_string(),
                        {compute_prelude}: raw.compute_prelude.to_string(),
                        {raylib_prelude}: raw.raylib_prelude.to_string(),
                        {game_prelude}: raw.game_prelude.to_string(),
                        {task_group_prelude}: raw.task_group_prelude.to_string(),
                        {runtime_stop_metadata}: raw.runtime_stop_metadata,
                        {game_default_frame_budget}: jet_foundation::Numeric::JetInt::from_i64(raw.game_default_frame_budget),
                        {game_frame_budget_error}: raw.game_frame_budget_error.to_string(),
                        {harfbuzz_wasm}: raw.harfbuzz_wasm.to_vec(),
                        {timezone_data}: raw.timezone_data.into_iter().map(|(name, bytes)| {timezone_type} {{
                            {timezone_name}: name,
                            {timezone_bytes}: bytes,
                        }}).collect(),
                        {index_html}: assets.{index_html},
                        {explicit_html_path}: assets.{explicit_html_path},
                        {source_names}: assets.{source_names},
                        {source_contents}: assets.{source_contents},
                        {onnx_runtime_js}: assets.{onnx_runtime_js},
                        {onnx_runtime_worker_js}: assets.{onnx_runtime_worker_js},
                    }};
                    Ok({target_web}(assets, release_policy, profile))
                }}
            }}
        }}"#,
        target_type = target_type,
        target_native = target_native,
        target_web = target_web,
        assets_type = assets_type,
        timezone_type = timezone_type,
        timezone_name = timezone_name,
        timezone_bytes = timezone_bytes,
        dom_runtime = field("dom_runtime")?,
        execution_prelude = field("execution_prelude")?,
        event_prelude = field("event_prelude")?,
        realtime_prelude = field("realtime_prelude")?,
        data_prelude = field("data_prelude")?,
        history_prelude = field("history_prelude")?,
        compute_prelude = field("compute_prelude")?,
        raylib_prelude = field("raylib_prelude")?,
        game_prelude = field("game_prelude")?,
        task_group_prelude = field("task_group_prelude")?,
        runtime_stop_metadata = field("runtime_stop_metadata")?,
        game_default_frame_budget = field("game_default_frame_budget")?,
        game_frame_budget_error = field("game_frame_budget_error")?,
        harfbuzz_wasm = field("harfbuzz_wasm")?,
        timezone_data = field("timezone_data")?,
        index_html = field("index_html")?,
        explicit_html_path = field("explicit_html_path")?,
        source_names = field("source_names")?,
        source_contents = field("source_contents")?,
        onnx_runtime_js = field("onnx_runtime_js")?,
        onnx_runtime_worker_js = field("onnx_runtime_worker_js")?,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    writeln!(
        out,
        "#[doc(hidden)]\npub(crate) fn __jet_bootstrap_native_compile_target() -> {target_type} {{ {target_native} }}\n",
        target_type = target_type,
        target_native = target_native,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    Ok(())
}
fn emit_bootstrap_manifest_adapter(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let result_type = symbols.type_symbol("JetDriverCompileResult")?;
    let mir_program_type = symbols.type_symbol("MirProgram")?;
    let eval_config_type = symbols.type_symbol("JetEvalConfig")?;
    let result_source = symbols.field_symbol("JetDriverCompileResult", "emitted_source")?;
    let result_internal_problem = symbols.field_symbol("JetDriverCompileResult", "internal_problem")?;
    let result_complete = symbols.field_symbol("JetDriverCompileResult", "complete")?;
    let result_mir = symbols.field_symbol("JetDriverCompileResult", "mir")?;
    let result_entry_function = symbols.field_symbol("JetDriverCompileResult", "entry_function")?;
    let result_runtime_artifact = symbols.field_symbol("JetDriverCompileResult", "runtime_artifact")?;
    let result_manifest = symbols.field_symbol("JetDriverCompileResult", "emit_manifest")?;
    let result_web_artifact = symbols.field_symbol("JetDriverCompileResult", "web_artifact")?;
    let result_web_artifacts = symbols.field_symbol("JetDriverCompileResult", "web_artifacts")?;
    let result_comptime_stdout = symbols.field_symbol("JetDriverCompileResult", "comptime_stdout")?;
    let result_comptime_stderr = symbols.field_symbol("JetDriverCompileResult", "comptime_stderr")?;
    let result_soft_stop = symbols.field_symbol("JetDriverCompileResult", "soft_stop")?;
    let result_exit_code = symbols.field_symbol("JetDriverCompileResult", "exit_code")?;
    let web_manifest_json = symbols.field_symbol("JetWebArtifacts", "manifest_json")?;
    let web_wasm_rust = symbols.field_symbol("JetWebArtifacts", "wasm_rust")?;
    let web_incremental_identity = symbols.field_symbol("JetWebArtifacts", "rustc_incremental_identity")?;
    let web_js_app = symbols.field_symbol("JetWebArtifacts", "js_app")?;
    let web_js_source_map = symbols.field_symbol("JetWebArtifacts", "js_source_map")?;
    let web_source_names = symbols.field_symbol("JetWebArtifacts", "source_names")?;
    let web_source_contents = symbols.field_symbol("JetWebArtifacts", "source_contents")?;
    let web_dom_runtime = symbols.field_symbol("JetWebArtifacts", "dom_runtime")?;
    let web_onnx_runtime_js = symbols.field_symbol("JetWebArtifacts", "onnx_runtime_js")?;
    let web_onnx_worker_js = symbols.field_symbol("JetWebArtifacts", "onnx_runtime_worker_js")?;
    let web_index_html = symbols.field_symbol("JetWebArtifacts", "index_html")?;
    let web_explicit_html_path = symbols.field_symbol("JetWebArtifacts", "explicit_html_path")?;
    let web_command_record = symbols.field_symbol("JetWebArtifacts", "command_record")?;
    let manifest_callables =
        symbols.field_symbol("JetRustEmitManifest", "callable_metadata")?;
    let manifest_fields = symbols.field_symbol("JetRustEmitManifest", "field_metadata")?;
    let manifest_variants = symbols.field_symbol("JetRustEmitManifest", "variant_metadata")?;
    let manifest_type_metadata =
        symbols.field_symbol("JetRustEmitManifest", "type_metadata")?;
    let manifest_traits = symbols.field_symbol("JetRustEmitManifest", "traits")?;
    let manifest_trait_methods = symbols.field_symbol("JetRustEmitManifest", "trait_methods")?;
    let trait_id = symbols.field_symbol("JetRustEmitTraitMetadata", "trait_id")?;
    let trait_key = symbols.field_symbol("JetRustEmitTraitMetadata", "key")?;
    let trait_name = symbols.field_symbol("JetRustEmitTraitMetadata", "name")?;
    let trait_symbol = symbols.field_symbol("JetRustEmitTraitMetadata", "symbol")?;
    let trait_method_trait_id = symbols.field_symbol("JetRustEmitTraitMethodMetadata", "trait_id")?;
    let trait_method_id = symbols.field_symbol("JetRustEmitTraitMethodMetadata", "method_id")?;
    let trait_method_key = symbols.field_symbol("JetRustEmitTraitMethodMetadata", "key")?;
    let trait_method_name = symbols.field_symbol("JetRustEmitTraitMethodMetadata", "name")?;
    let trait_method_symbol = symbols.field_symbol("JetRustEmitTraitMethodMetadata", "symbol")?;
    let trait_method_receiver_access = symbols.field_symbol("JetRustEmitTraitMethodMetadata", "receiver_access")?;
    let trait_method_parameter_types = symbols.field_symbol("JetRustEmitTraitMethodMetadata", "parameter_types")?;
    let trait_method_parameter_access = symbols.field_symbol("JetRustEmitTraitMethodMetadata", "parameter_access")?;
    let trait_method_return_type = symbols.field_symbol("JetRustEmitTraitMethodMetadata", "return_type")?;

    let callable_function =
        symbols.field_symbol("JetRustEmitCallableMetadata", "function")?;
    let callable_symbol = symbols.field_symbol("JetRustEmitCallableMetadata", "symbol")?;
    let callable_source_name =
        symbols.field_symbol("JetRustEmitCallableMetadata", "source_name")?;
    let callable_parameter_types =
        symbols.field_symbol("JetRustEmitCallableMetadata", "parameter_types")?;
    let callable_parameter_access =
        symbols.field_symbol("JetRustEmitCallableMetadata", "parameter_access")?;
    let callable_return_type =
        symbols.field_symbol("JetRustEmitCallableMetadata", "return_type")?;
    let callable_type =
        symbols.field_symbol("JetRustEmitCallableMetadata", "callable_type")?;

    let field_field = symbols.field_symbol("JetRustEmitFieldMetadata", "field")?;
    let field_owner = symbols.field_symbol("JetRustEmitFieldMetadata", "owner")?;
    let field_symbol = symbols.field_symbol("JetRustEmitFieldMetadata", "symbol")?;
    let field_source_name =
        symbols.field_symbol("JetRustEmitFieldMetadata", "source_name")?;

    let variant_owner = symbols.field_symbol("JetRustEmitVariantMetadata", "owner")?;
    let variant_source_name =
        symbols.field_symbol("JetRustEmitVariantMetadata", "source_name")?;
    let variant_wire_name = symbols.field_symbol("JetRustEmitVariantMetadata", "wire_name")?;
    let variant_symbol = symbols.field_symbol("JetRustEmitVariantMetadata", "symbol")?;
    let variant_payload_types =
        symbols.field_symbol("JetRustEmitVariantMetadata", "payload_types")?;

    let type_ty = symbols.field_symbol("JetRustEmitTypeMetadata", "ty")?;
    let type_source_name =
        symbols.field_symbol("JetRustEmitTypeMetadata", "source_name")?;
    let type_symbol = symbols.field_symbol("JetRustEmitTypeMetadata", "symbol")?;
    let function_value = symbols.field_symbol("MirFunctionId", "value")?;
    let type_value = symbols.field_symbol("MirTypeId", "value")?;
    let artifact_value = symbols.field_symbol("MirArtifactId", "value")?;
    let field_value = symbols.field_symbol("MirFieldId", "value")?;
    let access_read = symbols.variant_path("MirAccess", "Read")?;
    let access_write = symbols.variant_path("MirAccess", "Write")?;
    let access_move = symbols.variant_path("MirAccess", "Move")?;
    let trait_id_value = symbols.field_symbol("MirTraitId", "value")?;
    let trait_method_id_value = symbols.field_symbol("MirTraitMethodId", "value")?;

    writeln!(
        out,
        concat!(
            "#[doc(hidden)]\n",
            "pub(crate) fn __jet_bootstrap_bindings_from_result(\n",
            "    {value}: {result_type},\n",
            "    snapshot: &crate::compiler_bootstrap_host::AuthorizedSourceSnapshot,\n",
            "    runtime_config: {eval_config_type},\n",
            ") -> Result<crate::BootstrapJetCompileResult<{mir_program_type}, {eval_config_type}>, String> {{\n",
            "    let internal_problem = ({value}).{result_internal_problem}.as_ref().ok().cloned();\n",
            "    let complete = ({value}).{result_complete};\n",
            "    let comptime_stdout = ({value}).{result_comptime_stdout}.clone();\n",
            "    let comptime_stderr = ({value}).{result_comptime_stderr}.clone();\n",
            "    let soft_stop = ({value}).{result_soft_stop};\n",
            "    let source_program = ({value}).{result_mir}.ok();\n",
            "    let mir = source_program.as_ref().map(__jet_bootstrap_mir_program_to_host).transpose()?;\n",
            "    let entry_function = ({value}).{result_entry_function}.as_ref().ok().map(|function| Ok(::jet_foundation::MIR::MirFunctionId(function.{function_value}.to_string_rep().parse::<u64>().map_err(|_| \"Jet entry function ID is not an unsigned integer\".to_string())?))).transpose()?;\n",
            "    let runtime_artifact = ({value}).{result_runtime_artifact}.as_ref().ok().map(|artifact| Ok(::jet_foundation::MIR::MirArtifactId(artifact.{artifact_value}.to_string_rep().parse::<u64>().map_err(|_| \"Jet runtime artifact ID is not an unsigned integer\".to_string())?))).transpose()?;\n",
            "    let web_artifact = ({value}).{result_web_artifact}.as_ref().ok().map(|artifact| Ok(::jet_foundation::MIR::MirArtifactId(artifact.{artifact_value}.to_string_rep().parse::<u64>().map_err(|_| \"Jet Web artifact ID is not an unsigned integer\".to_string())?))).transpose()?;\n",
            "    let web_artifacts = ({value}).{result_web_artifacts}.as_ref().ok().map(|artifacts| crate::BootstrapWebArtifacts {{\n",
            "        manifest_json: artifacts.{web_manifest_json}.clone(),\n",
            "        wasm_rust: artifacts.{web_wasm_rust}.clone(),\n",
            "        rustc_incremental_identity: artifacts.{web_incremental_identity}.clone(),\n",
            "        js_app: artifacts.{web_js_app}.clone(),\n",
            "        js_source_map: artifacts.{web_js_source_map}.clone(),\n",
            "        source_names: artifacts.{web_source_names}.clone(),\n",
            "        source_contents: artifacts.{web_source_contents}.clone(),\n",
            "        dom_runtime: artifacts.{web_dom_runtime}.clone(),\n",
            "        onnx_runtime_js: artifacts.{web_onnx_runtime_js}.clone(),\n",
            "        onnx_runtime_worker_js: artifacts.{web_onnx_worker_js}.clone(),\n",
            "        index_html: artifacts.{web_index_html}.clone(),\n",
            "        explicit_html_path: artifacts.{web_explicit_html_path}.as_ref().ok().cloned(),\n",
            "        command_record: artifacts.{web_command_record}.clone(),\n",
            "    }});\n",
            "    if internal_problem.is_some() || !complete {{\n",
            "        return Ok(crate::BootstrapJetCompileResult {{ complete, emitted_source: None, bindings: None, source_program, runtime_config, mir, entry_function, runtime_artifact, web_artifact, web_artifacts, comptime_stdout, comptime_stderr, soft_stop, exit_code, internal_problem, reports, resources: None }});\n",
            "    }}\n",
            "    if runtime_artifact.is_some() {{\n",
            "        if mir.is_none() || entry_function.is_none() {{ return Err(\"complete Source runtime result has no typed MIR or entry function\".to_string()); }}\n",
            "        return Ok(crate::BootstrapJetCompileResult {{ complete, emitted_source: None, bindings: None, source_program, runtime_config, mir, entry_function, runtime_artifact, web_artifact, web_artifacts, comptime_stdout, comptime_stderr, soft_stop, exit_code, internal_problem, reports, resources: None }});\n",
            "    }}\n",
            "    if web_artifact.is_some() || web_artifacts.is_some() {{\n",
            "        if web_artifact.is_none() || web_artifacts.is_none() || mir.is_none() || entry_function.is_none() {{ return Err(\"complete Web result has no exact artifact, artifacts, MIR, or entry function\".to_string()); }}\n",
            "        return Ok(crate::BootstrapJetCompileResult {{ complete, emitted_source: None, bindings: None, source_program, runtime_config, mir, entry_function, runtime_artifact, web_artifact, web_artifacts, comptime_stdout, comptime_stderr, soft_stop, exit_code, internal_problem, reports, resources: None }});\n",
            "    }}\n",
            "    let manifest = ({value}).{result_manifest}.as_ref().ok().ok_or_else(|| \"complete bootstrap compiler result has no Rust emission manifest\".to_string())?;\n",
            "    let callables = manifest.{manifest_callables}.iter().map(|row| Ok((({row}).{callable_source_name}.clone(), crate::Codegen::MIRRust::MirRustCallableMetadata {{\n",
            "        function: ::jet_foundation::MIR::MirFunctionId(({row}).{callable_function}.{function_value}.to_string_rep().parse::<u64>().map_err(|_| \"manifest callable function ID is not an unsigned integer\".to_string())?),\n",
            "        symbol: ({row}).{callable_symbol}.clone(),\n",
            "        parameter_types: ({row}).{callable_parameter_types}.clone(),\n",
            "        parameter_access: ({row}).{callable_parameter_access}.iter().map(|access| match access {{\n",
            "            {access_read} => Ok(::jet_foundation::MIR::MirAccess::Read),\n",
            "            {access_write} => Ok(::jet_foundation::MIR::MirAccess::Write),\n",
            "            {access_move} => Ok(::jet_foundation::MIR::MirAccess::Move),\n",
            "            _ => Err(\"manifest callable access has an unknown variant\".to_string()),\n",
            "        }}).collect::<Result<Vec<_>, String>>()?,\n",
            "        return_type: ({row}).{callable_return_type}.clone(),\n",
            "        callable_type: ({row}).{callable_type}.clone(),\n",
            "    }}))).collect::<Result<Vec<_>, String>>()?;\n",
            "    let types = manifest.{manifest_type_metadata}.iter().map(|row| Ok((\n",
            "        ::jet_foundation::MIR::MirTypeId(({row}).{type_ty}.{type_value}.to_string_rep().parse::<u64>().map_err(|_| \"manifest type ID is not an unsigned integer\".to_string())?),\n",
            "        ({row}).{type_source_name}.clone(),\n",
            "        ({row}).{type_symbol}.clone(),\n",
            "    ))).collect::<Result<Vec<_>, String>>()?;\n",
            "    let fields = manifest.{manifest_fields}.iter().map(|row| {{\n",
            "        let field_id = ::jet_foundation::MIR::MirFieldId(({row}).{field_field}.{field_value}.to_string_rep().parse::<u64>().map_err(|_| \"manifest field ID is not an unsigned integer\".to_string())?);\n",
            "        let owner_id = ::jet_foundation::MIR::MirTypeId(({row}).{field_owner}.{type_value}.to_string_rep().parse::<u64>().map_err(|_| \"manifest field owner ID is not an unsigned integer\".to_string())?);\n",
            "        let ty = mir.as_ref().and_then(|program| program.fields.iter().find(|candidate| candidate.id == field_id && candidate.owner == owner_id)).map(|candidate| candidate.field.ty.clone()).ok_or_else(|| format!(\"manifest field {:?}/{:?} is absent from converted MIR\", field_id, owner_id))?;\n",
            "        Ok((field_id, owner_id, ({row}).{field_source_name}.clone(), ({row}).{field_symbol}.clone(), ty))\n",
            "    }}).collect::<Result<Vec<_>, String>>()?;\n",
            "    let variants = manifest.{manifest_variants}.iter().map(|row| Ok(crate::Codegen::MIRRust::MirRustVariantMetadata {{\n",
            "        owner: ::jet_foundation::MIR::MirTypeId(({row}).{variant_owner}.{type_value}.to_string_rep().parse::<u64>().map_err(|_| \"manifest variant owner ID is not an unsigned integer\".to_string())?),\n",
            "        source_name: ({row}).{variant_source_name}.clone(),\n",
            "        wire_name: ({row}).{variant_wire_name}.clone(),\n",
            "        symbol: ({row}).{variant_symbol}.clone(),\n",
            "        payload_types: ({row}).{variant_payload_types}.clone(),\n",
            "    }})).collect::<Result<Vec<_>, String>>()?;\n",
            "    let traits = manifest.{manifest_traits}.iter().map(|row| Ok(crate::Codegen::MIRRust::MirRustTraitMetadata {{\n",
            "        trait_id: ::jet_foundation::MIR::MirTraitId(({row}).{trait_id}.{trait_id_value}.to_string_rep().parse::<u64>().map_err(|_| \"manifest trait ID is not an unsigned integer\".to_string())?),\n",
            "        key: ({row}).{trait_key}.clone(),\n",
            "        name: ({row}).{trait_name}.clone(),\n",
            "        symbol: ({row}).{trait_symbol}.clone(),\n",
            "    }})).collect::<Result<Vec<_>, String>>()?;\n",
            "    let trait_methods = manifest.{manifest_trait_methods}.iter().map(|row| Ok(crate::Codegen::MIRRust::MirRustTraitMethodMetadata {{\n",
            "        trait_id: ::jet_foundation::MIR::MirTraitId(({row}).{trait_method_trait_id}.{trait_id_value}.to_string_rep().parse::<u64>().map_err(|_| \"manifest trait-method owner ID is not an unsigned integer\".to_string())?),\n",
            "        method_id: ::jet_foundation::MIR::MirTraitMethodId(({row}).{trait_method_id}.{trait_method_id_value}.to_string_rep().parse::<u64>().map_err(|_| \"manifest trait-method ID is not an unsigned integer\".to_string())?),\n",
            "        key: ({row}).{trait_method_key}.clone(),\n",
            "        name: ({row}).{trait_method_name}.clone(),\n",
            "        symbol: ({row}).{trait_method_symbol}.clone(),\n",
            "        receiver_access: match ({row}).{trait_method_receiver_access}.as_ref().ok() {{\n",
            "            Some(access) => Some(match access {{\n",
            "                {access_read} => ::jet_foundation::MIR::MirAccess::Read,\n",
            "                {access_write} => ::jet_foundation::MIR::MirAccess::Write,\n",
            "                {access_move} => ::jet_foundation::MIR::MirAccess::Move,\n",
            "            }}),\n",
            "            None => None,\n",
            "        }},\n",
            "        parameter_types: ({row}).{trait_method_parameter_types}.clone(),\n",
            "        parameter_access: ({row}).{trait_method_parameter_access}.iter().map(|access| match access {{\n",
            "            {access_read} => Ok(::jet_foundation::MIR::MirAccess::Read),\n",
            "            {access_write} => Ok(::jet_foundation::MIR::MirAccess::Write),\n",
            "            {access_move} => Ok(::jet_foundation::MIR::MirAccess::Move),\n",
            "        }}).collect::<Result<Vec<_>, String>>()?,\n",
            "        return_type: ({row}).{trait_method_return_type}.clone(),\n",
            "    }})).collect::<Result<Vec<_>, String>>()?;\n",
            "    let bindings = {descriptor}::from_rows(callables, types, fields, variants, traits, trait_methods);\n",
            "    Ok(crate::BootstrapJetCompileResult {{ complete: true, emitted_source: Some(source), bindings: Some(bindings), source_program, runtime_config, mir, entry_function, runtime_artifact, web_artifact: None, web_artifacts: None, comptime_stdout, comptime_stderr, soft_stop, exit_code, internal_problem, reports, resources: None }})\n",
            "}}\n",
        ),
        value = "value",
        result_type = result_type,
        result_source = result_source,
        result_complete = result_complete,
        result_internal_problem = result_internal_problem,
        result_mir = result_mir,
        result_entry_function = result_entry_function,
        result_runtime_artifact = result_runtime_artifact,
        result_web_artifact = result_web_artifact,
        result_web_artifacts = result_web_artifacts,
        result_comptime_stdout = result_comptime_stdout,
        result_comptime_stderr = result_comptime_stderr,
        result_soft_stop = result_soft_stop,
        mir_program_type = mir_program_type,
        eval_config_type = eval_config_type,
        result_exit_code = result_exit_code,
        web_manifest_json = web_manifest_json,
        web_wasm_rust = web_wasm_rust,
        web_incremental_identity = web_incremental_identity,
        web_js_app = web_js_app,
        web_js_source_map = web_js_source_map,
        web_source_names = web_source_names,
        web_source_contents = web_source_contents,
        web_dom_runtime = web_dom_runtime,
        web_onnx_runtime_js = web_onnx_runtime_js,
        web_onnx_worker_js = web_onnx_worker_js,
        web_index_html = web_index_html,
        web_explicit_html_path = web_explicit_html_path,
        web_command_record = web_command_record,
        result_manifest = result_manifest,
        manifest_callables = manifest_callables,
        manifest_fields = manifest_fields,
        manifest_variants = manifest_variants,
        manifest_type_metadata = manifest_type_metadata,
        manifest_traits = manifest_traits,
        manifest_trait_methods = manifest_trait_methods,
        trait_id = trait_id,
        trait_key = trait_key,
        trait_name = trait_name,
        trait_symbol = trait_symbol,
        trait_method_trait_id = trait_method_trait_id,
        trait_method_id = trait_method_id,
        trait_method_key = trait_method_key,
        trait_method_name = trait_method_name,
        trait_method_symbol = trait_method_symbol,
        trait_method_receiver_access = trait_method_receiver_access,
        trait_method_parameter_types = trait_method_parameter_types,
        trait_method_parameter_access = trait_method_parameter_access,
        trait_method_return_type = trait_method_return_type,
        trait_id_value = trait_id_value,
        trait_method_id_value = trait_method_id_value,
        callable_function = callable_function,
        callable_source_name = callable_source_name,
        callable_symbol = callable_symbol,
        callable_parameter_types = callable_parameter_types,
        callable_parameter_access = callable_parameter_access,
        callable_return_type = callable_return_type,
        callable_type = callable_type,
        field_field = field_field,
        field_owner = field_owner,
        field_symbol = field_symbol,
        field_source_name = field_source_name,
        variant_owner = variant_owner,
        variant_source_name = variant_source_name,
        variant_wire_name = variant_wire_name,
        variant_symbol = variant_symbol,
        variant_payload_types = variant_payload_types,
        type_ty = type_ty,
        type_source_name = type_source_name,
        type_symbol = type_symbol,
        function_value = function_value,
        artifact_value = artifact_value,
        type_value = type_value,
        field_value = field_value,
        access_read = access_read,
        access_write = access_write,
        access_move = access_move,
        row = "row",
        descriptor = "crate::BootstrapBindingDescriptor",
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    Ok(())
}

fn emit_bootstrap_type_codec(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let mir_type = symbols.type_symbol("MirType")?;
    let mir_type_id = symbols.type_symbol("MirTypeId")?;
    let mir_measure = symbols.type_symbol("MirMeasure")?;
    let mir_dimension = symbols.type_symbol("MirDimension")?;
    let mir_dimension_axis = symbols.type_symbol("MirDimensionAxis")?;
    let mir_abi = symbols.type_symbol("MirAbi")?;
    let mir_size = symbols.type_symbol("MirSize")?;
    let mir_layout = symbols.type_symbol("MirLayout")?;
    let mir_nominal_ref = symbols.type_symbol("MirNominalRef")?;
    let mir_trait_ref = symbols.type_symbol("MirTraitRef")?;
    let mir_trait_id = symbols.type_symbol("MirTraitId")?;
    let mir_tuple_field = symbols.type_symbol("MirTupleField")?;
    let mir_tag_marker = symbols.type_symbol("MirTagMarker")?;

    let type_kind = symbols.field_symbol("MirType", "kind")?;
    let type_identity = symbols.field_symbol("MirType", "identity")?;
    let type_layout = symbols.field_symbol("MirType", "layout")?;
    let id_value = symbols.field_symbol("MirTypeId", "value")?;
    let dimension_axes = symbols.field_symbol("MirDimension", "axes")?;
    let axis_name = symbols.field_symbol("MirDimensionAxis", "name")?;
    let axis_exponent = symbols.field_symbol("MirDimensionAxis", "exponent")?;
    let nominal_id = symbols.field_symbol("MirNominalRef", "id")?;
    let nominal_name = symbols.field_symbol("MirNominalRef", "name")?;
    let trait_ref_id = symbols.field_symbol("MirTraitRef", "id")?;
    let trait_ref_name = symbols.field_symbol("MirTraitRef", "name")?;
    let trait_id_value = symbols.field_symbol("MirTraitId", "value")?;
    let tuple_name = symbols.field_symbol("MirTupleField", "name")?;
    let tuple_ty = symbols.field_symbol("MirTupleField", "ty")?;
    let layout_abi = symbols.field_symbol("MirLayout", "abi")?;
    let layout_size = symbols.field_symbol("MirLayout", "size")?;
    let layout_align = symbols.field_symbol("MirLayout", "align")?;

    let kind_variants = [
        ("Int", symbols.variant_path("MirTypeKind", "Int")?),
        ("Float", symbols.variant_path("MirTypeKind", "Float")?),
        ("Bool", symbols.variant_path("MirTypeKind", "Bool")?),
        ("String", symbols.variant_path("MirTypeKind", "String")?),
        ("Char", symbols.variant_path("MirTypeKind", "Char")?),
        ("List", symbols.variant_path("MirTypeKind", "List")?),
        ("Map", symbols.variant_path("MirTypeKind", "Map")?),
        ("Shared", symbols.variant_path("MirTypeKind", "Shared")?),
        ("Option", symbols.variant_path("MirTypeKind", "Option")?),
        ("Result", symbols.variant_path("MirTypeKind", "Result")?),
        ("Fn", symbols.variant_path("MirTypeKind", "Fn")?),
        ("SendFn", symbols.variant_path("MirTypeKind", "SendFn")?),
        ("Apply", symbols.variant_path("MirTypeKind", "Apply")?),
        ("TraitObject", symbols.variant_path("MirTypeKind", "TraitObject")?),
        ("Tuple", symbols.variant_path("MirTypeKind", "Tuple")?),
        ("FixedList", symbols.variant_path("MirTypeKind", "FixedList")?),
        ("IntN", symbols.variant_path("MirTypeKind", "IntN")?),
        ("InlineRange", symbols.variant_path("MirTypeKind", "InlineRange")?),
        ("Float32", symbols.variant_path("MirTypeKind", "Float32")?),
        ("Tagged", symbols.variant_path("MirTypeKind", "Tagged")?),
        ("Union", symbols.variant_path("MirTypeKind", "Union")?),
        ("Quantity", symbols.variant_path("MirTypeKind", "Quantity")?),
        ("Measure", symbols.variant_path("MirTypeKind", "Measure")?),
    ];
    let kind = |name: &str| {
        kind_variants
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, path)| path.clone())
            .unwrap_or_else(|| name.to_string())
    };

    let measure_variants = [
        ("Literal", symbols.variant_path("MirMeasure", "Literal")?),
        ("SignedLiteral", symbols.variant_path("MirMeasure", "SignedLiteral")?),
        ("Symbol", symbols.variant_path("MirMeasure", "Symbol")?),
        ("Combined", symbols.variant_path("MirMeasure", "Combined")?),
    ];
    let measure = |name: &str| {
        measure_variants
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, path)| path.clone())
            .unwrap_or_else(|| name.to_string())
    };

    let abi_variants = [
        ("Scalar", symbols.variant_path("MirAbi", "Scalar")?),
        ("Aggregate", symbols.variant_path("MirAbi", "Aggregate")?),
        ("Sequence", symbols.variant_path("MirAbi", "Sequence")?),
        ("Function", symbols.variant_path("MirAbi", "Function")?),
        ("Nominal", symbols.variant_path("MirAbi", "Nominal")?),
        ("Dynamic", symbols.variant_path("MirAbi", "Dynamic")?),
        ("Never", symbols.variant_path("MirAbi", "Never")?),
    ];
    let abi = |name: &str| {
        abi_variants
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, path)| path.clone())
            .unwrap_or_else(|| name.to_string())
    };
    let size_variants = [
        ("Static", symbols.variant_path("MirSize", "Static")?),
        ("Dynamic", symbols.variant_path("MirSize", "Dynamic")?),
    ];
    let size = |name: &str| {
        size_variants
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, path)| path.clone())
            .unwrap_or_else(|| name.to_string())
    };
    let scalar_variants = [
        ("Int", symbols.variant_path("MirScalarKind", "Int")?),
        ("Float", symbols.variant_path("MirScalarKind", "Float")?),
        ("Float32", symbols.variant_path("MirScalarKind", "Float32")?),
        ("Bool", symbols.variant_path("MirScalarKind", "Bool")?),
        ("Char", symbols.variant_path("MirScalarKind", "Char")?),
        ("Pointer", symbols.variant_path("MirScalarKind", "Pointer")?),
    ];
    let scalar = |name: &str| {
        scalar_variants
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, path)| path.clone())
            .unwrap_or_else(|| name.to_string())
    };
    let tag_variants = [
        ("User", symbols.variant_path("MirTagMarker", "User")?),
        ("Internal", symbols.variant_path("MirTagMarker", "Internal")?),
    ];
    let tag = |name: &str| {
        tag_variants
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, path)| path.clone())
            .unwrap_or_else(|| name.to_string())
    };
    let internal_names = [
        "CoreCryptoNominal",
        "DeterministicClock",
        "SystemClock",
        "ExpiringSecretLoan",
        "SharedGuardRead",
        "SharedGuardEdit",
        "TerminalFactSet",
        "CppCallbackAbi",
        "AllocatorView",
    ];
    let internal_paths = internal_names
        .iter()
        .map(|name| {
            Ok::<_, BootstrapHostCodecError>((
                *name,
                symbols.variant_path("MirInternalTag", name)?,
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let internal = |name: &str| {
        internal_paths
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, path)| path.clone())
            .unwrap_or_else(|| name.to_string())
    };

    writeln!(
        out,
        "fn __jet_bootstrap_type_id_to_host(value: &{mir_type_id}) -> Result<::jet_foundation::MIR::MirTypeId, String> {{\n\
             let text = ({value}).{id_value}.to_string_rep();\n\
             let id = text.parse::<u64>().map_err(|_| \"MIR type identity is not an unsigned integer\".to_string())?;\n\
             if id == 0 {{\n\
                 return Err(\"MIR type identity is zero\".to_string());\n\
             }}\n\
             Ok(::jet_foundation::MIR::MirTypeId(id))\n\
         }}\n\
         fn __jet_bootstrap_type_id_from_host(value: ::jet_foundation::MIR::MirTypeId) -> Result<{mir_type_id}, String> {{\n\
             let value = jet_foundation::Numeric::JetInt::from_str(&value.0.to_string())?;\n\
             Ok({mir_type_id} {{ {id_value}: value }})\n\
         }}"
        , mir_type_id = mir_type_id, value = "value", id_value = id_value
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    writeln!(
        out,
        "fn __jet_bootstrap_measure_to_host(value: &{mir_measure}) -> Result<::jet_foundation::MIR::MirMeasure, String> {{
             match value {{
                 {literal}(kind, value) => Ok(::jet_foundation::MIR::MirMeasure::Literal {{
                     kind: kind.clone(),
                     value: value.to_string_rep().parse::<u64>().map_err(|_| \"measure literal is not an unsigned integer\".to_string())?,
                 }}),
                 {signed}(kind, value) => Ok(::jet_foundation::MIR::MirMeasure::SignedLiteral {{
                     kind: kind.clone(),
                     value: value.to_i64().ok_or_else(|| \"signed measure literal exceeds i64\".to_string())?,
                 }}),
                 {symbol}(kind, name) => Ok(::jet_foundation::MIR::MirMeasure::Symbol {{
                     kind: kind.clone(),
                     name: name.clone(),
                 }}),
                 {combined}(kind, rule, left, right) => {{
                     let rule = match rule {{
                         {add} => ::jet_foundation::MIR::MirMeasureRule::Add,
                         {mul} => ::jet_foundation::MIR::MirMeasureRule::Mul,
                         {mat} => ::jet_foundation::MIR::MirMeasureRule::Match,
                     }};
                     Ok(::jet_foundation::MIR::MirMeasure::Combined {{
                         kind: kind.clone(),
                         rule,
                         left: Box::new(__jet_bootstrap_measure_to_host(left)?),
                         right: Box::new(__jet_bootstrap_measure_to_host(right)?),
                     }})
                 }}
             }}
         }}"
        ,
        literal = measure("Literal"),
        signed = measure("SignedLiteral"),
        symbol = measure("Symbol"),
        combined = measure("Combined"),
        add = symbols.variant_path("MirMeasureRule", "Add")?,
        mul = symbols.variant_path("MirMeasureRule", "Mul")?,
        mat = symbols.variant_path("MirMeasureRule", "Match")?,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    writeln!(
        out,
        "fn __jet_bootstrap_measure_from_host(value: &::jet_foundation::MIR::MirMeasure) -> Result<{mir_measure}, String> {{
             match value {{
                 ::jet_foundation::MIR::MirMeasure::Literal {{ kind, value }} => Ok({literal}(kind.clone(), jet_foundation::Numeric::JetInt::from_str(&value.to_string())?)),
                 ::jet_foundation::MIR::MirMeasure::SignedLiteral {{ kind, value }} => Ok({signed}(kind.clone(), jet_foundation::Numeric::JetInt::from_i64(*value))),
                 ::jet_foundation::MIR::MirMeasure::Symbol {{ kind, name }} => Ok({symbol}(kind.clone(), name.clone())),
                 ::jet_foundation::MIR::MirMeasure::Combined {{ kind, rule, left, right }} => {{
                     let rule = match rule {{
                         ::jet_foundation::MIR::MirMeasureRule::Add => {add},
                         ::jet_foundation::MIR::MirMeasureRule::Mul => {mul},
                         ::jet_foundation::MIR::MirMeasureRule::Match => {mat},
                     }};
                     Ok({combined}(
                         kind.clone(), rule,
                         __jet_bootstrap_measure_from_host(left)?,
                         __jet_bootstrap_measure_from_host(right)?,
                     ))
                 }}
             }}
         }}"
        ,
        literal = measure("Literal"),
        signed = measure("SignedLiteral"),
        symbol = measure("Symbol"),
        combined = measure("Combined"),
        add = symbols.variant_path("MirMeasureRule", "Add")?,
        mul = symbols.variant_path("MirMeasureRule", "Mul")?,
        mat = symbols.variant_path("MirMeasureRule", "Match")?,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    writeln!(
        out,
        "fn __jet_bootstrap_dimension_to_host(value: &{mir_dimension}) -> Result<::jet_foundation::MIR::MirDimension, String> {{
             let mut axes = ::std::collections::BTreeMap::new();
             for axis in &({value}).{dimension_axes} {{
                 let name = ({axis}).{axis_name}.clone();
                 let exponent = __jet_bootstrap_measure_to_host(&({axis}).{axis_exponent})?;
                 if axes.insert(name, exponent).is_some() {{
                     return Err(\"MIR dimension contains a duplicate axis name\".to_string());
                 }}
             }}
             Ok(::jet_foundation::MIR::MirDimension {{ axes }})
         }}
         fn __jet_bootstrap_dimension_from_host(value: &::jet_foundation::MIR::MirDimension) -> Result<{mir_dimension}, String> {{
             let mut axes = Vec::with_capacity(value.axes.len());
             for (name, exponent) in &value.axes {{
                 axes.push({mir_dimension_axis} {{
                     {axis_name}: name.clone(),
                     {axis_exponent}: __jet_bootstrap_measure_from_host(exponent)?,
                 }});
             }}
             Ok({mir_dimension} {{ {dimension_axes}: axes }})
         }}"
        ,
        value = "value",
        axis = "axis",
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    writeln!(
        out,
        "fn __jet_bootstrap_tag_to_host(value: &{mir_tag_marker}) -> ::jet_foundation::MIR::MirTagMarker {{
             match value {{
                 {user}(name) => ::jet_foundation::MIR::MirTagMarker::User(name.clone()),
                 {internal}(tag) => ::jet_foundation::MIR::MirTagMarker::Internal(match tag {{
                     {core} => ::jet_foundation::MIR::MirInternalTag::CoreCryptoNominal,
                     {clock_det} => ::jet_foundation::MIR::MirInternalTag::DeterministicClock,
                     {clock_sys} => ::jet_foundation::MIR::MirInternalTag::SystemClock,
                     {loan} => ::jet_foundation::MIR::MirInternalTag::ExpiringSecretLoan,
                     {guard_read} => ::jet_foundation::MIR::MirInternalTag::SharedGuardRead,
                     {guard_edit} => ::jet_foundation::MIR::MirInternalTag::SharedGuardEdit,
                     {terminal} => ::jet_foundation::MIR::MirInternalTag::TerminalFactSet,
                     {cpp} => ::jet_foundation::MIR::MirInternalTag::CppCallbackAbi,
                     {allocator} => ::jet_foundation::MIR::MirInternalTag::AllocatorView,
                 }}),
             }}
         }}
         fn __jet_bootstrap_tag_from_host(value: &::jet_foundation::MIR::MirTagMarker) -> Result<{mir_tag_marker}, String> {{
             match value {{
                 ::jet_foundation::MIR::MirTagMarker::User(name) => Ok({user}(name.clone())),
                 ::jet_foundation::MIR::MirTagMarker::Internal(tag) => Ok({internal}(match tag {{
                     ::jet_foundation::MIR::MirInternalTag::CoreCryptoNominal => {core},
                     ::jet_foundation::MIR::MirInternalTag::DeterministicClock => {clock_det},
                     ::jet_foundation::MIR::MirInternalTag::SystemClock => {clock_sys},
                     ::jet_foundation::MIR::MirInternalTag::ExpiringSecretLoan => {loan},
                     ::jet_foundation::MIR::MirInternalTag::SharedGuardRead => {guard_read},
                     ::jet_foundation::MIR::MirInternalTag::SharedGuardEdit => {guard_edit},
                     ::jet_foundation::MIR::MirInternalTag::TerminalFactSet => {terminal},
                     ::jet_foundation::MIR::MirInternalTag::CppCallbackAbi => {cpp},
                     ::jet_foundation::MIR::MirInternalTag::AllocatorView => {allocator},
                 }})),
             }}
         }}"
        ,
        user = tag("User"),
        internal = tag("Internal"),
        core = internal("CoreCryptoNominal"),
        clock_det = internal("DeterministicClock"),
        clock_sys = internal("SystemClock"),
        loan = internal("ExpiringSecretLoan"),
        guard_read = internal("SharedGuardRead"),
        guard_edit = internal("SharedGuardEdit"),
        terminal = internal("TerminalFactSet"),
        cpp = internal("CppCallbackAbi"),
        allocator = internal("AllocatorView"),
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    writeln!(
        out,
        "fn __jet_bootstrap_abi_to_host(value: &{mir_abi}) -> ::jet_foundation::MIR::MirAbi {{
             match value {{
                 {scalar_abi}(kind) => ::jet_foundation::MIR::MirAbi::Scalar(match kind {{
                     {scalar_int} => ::jet_foundation::MIR::MirScalarKind::Int,
                     {scalar_float} => ::jet_foundation::MIR::MirScalarKind::Float,
                     {scalar_float32} => ::jet_foundation::MIR::MirScalarKind::Float32,
                     {scalar_bool} => ::jet_foundation::MIR::MirScalarKind::Bool,
                     {scalar_char} => ::jet_foundation::MIR::MirScalarKind::Char,
                     {scalar_pointer} => ::jet_foundation::MIR::MirScalarKind::Pointer,
                 }}),
                 {aggregate} => ::jet_foundation::MIR::MirAbi::Aggregate,
                 {sequence} => ::jet_foundation::MIR::MirAbi::Sequence,
                 {function} => ::jet_foundation::MIR::MirAbi::Function,
                 {nominal} => ::jet_foundation::MIR::MirAbi::Nominal,
                 {dynamic} => ::jet_foundation::MIR::MirAbi::Dynamic,
                 {never} => ::jet_foundation::MIR::MirAbi::Never,
             }}
         }}
         fn __jet_bootstrap_abi_from_host(value: &::jet_foundation::MIR::MirAbi) -> {mir_abi} {{
             match value {{
                 ::jet_foundation::MIR::MirAbi::Scalar(kind) => {scalar_abi}(match kind {{
                     ::jet_foundation::MIR::MirScalarKind::Int => {scalar_int},
                     ::jet_foundation::MIR::MirScalarKind::Float => {scalar_float},
                     ::jet_foundation::MIR::MirScalarKind::Float32 => {scalar_float32},
                     ::jet_foundation::MIR::MirScalarKind::Bool => {scalar_bool},
                     ::jet_foundation::MIR::MirScalarKind::Char => {scalar_char},
                     ::jet_foundation::MIR::MirScalarKind::Pointer => {scalar_pointer},
                 }}),
                 ::jet_foundation::MIR::MirAbi::Aggregate => {aggregate},
                 ::jet_foundation::MIR::MirAbi::Sequence => {sequence},
                 ::jet_foundation::MIR::MirAbi::Function => {function},
                 ::jet_foundation::MIR::MirAbi::Nominal => {nominal},
                 ::jet_foundation::MIR::MirAbi::Dynamic => {dynamic},
                 ::jet_foundation::MIR::MirAbi::Never => {never},
             }}
         }}"
        ,
        scalar_abi = abi("Scalar"),
        aggregate = abi("Aggregate"),
        sequence = abi("Sequence"),
        function = abi("Function"),
        nominal = abi("Nominal"),
        dynamic = abi("Dynamic"),
        never = abi("Never"),
        scalar_int = scalar("Int"),
        scalar_float = scalar("Float"),
        scalar_float32 = scalar("Float32"),
        scalar_bool = scalar("Bool"),
        scalar_char = scalar("Char"),
        scalar_pointer = scalar("Pointer"),
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    writeln!(
        out,
        "fn __jet_bootstrap_size_to_host(value: &{mir_size}) -> Result<::jet_foundation::MIR::MirSize, String> {{
             match value {{
                 {static_size}(value) => Ok(::jet_foundation::MIR::MirSize::Static(value.to_string_rep().parse::<u64>().map_err(|_| \"layout size is not an unsigned integer\".to_string())?)),
                 {dynamic_size} => Ok(::jet_foundation::MIR::MirSize::Dynamic),
             }}
         }}
         fn __jet_bootstrap_size_from_host(value: &::jet_foundation::MIR::MirSize) -> Result<{mir_size}, String> {{
             match value {{
                 ::jet_foundation::MIR::MirSize::Static(value) => Ok({static_size}(jet_foundation::Numeric::JetInt::from_str(&value.to_string())?)),
                 ::jet_foundation::MIR::MirSize::Dynamic => Ok({dynamic_size}),
             }}
         }}
         fn __jet_bootstrap_layout_from_host(value: &::jet_foundation::MIR::MirLayout) -> Result<{mir_layout}, String> {{
             Ok({mir_layout} {{
                 {layout_abi}: __jet_bootstrap_abi_from_host(&value.abi),
                 {layout_size}: __jet_bootstrap_size_from_host(&value.size)?,
                 {layout_align}: __jet_bootstrap_size_from_host(&value.align)?,
             }})
         }}"
        ,
        static_size = size("Static"),
        dynamic_size = size("Dynamic"),
        layout_abi = layout_abi,
        layout_size = layout_size,
        layout_align = layout_align,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    writeln!(
        out,
        "fn __jet_bootstrap_type_to_host(value: &{mir_type}) -> Result<::jet_foundation::MIR::MirType, String> {{
             let kind = match &({value}).{type_kind} {{
                 {int} => ::jet_foundation::MIR::MirTypeKind::Int,
                 {float} => ::jet_foundation::MIR::MirTypeKind::Float,
                 {bool_kind} => ::jet_foundation::MIR::MirTypeKind::Bool,
                 {string} => ::jet_foundation::MIR::MirTypeKind::String,
                 {char_kind} => ::jet_foundation::MIR::MirTypeKind::Char,
                 {list}(inner) => ::jet_foundation::MIR::MirTypeKind::List(Box::new(__jet_bootstrap_type_to_host(inner)?)),
                 {map}(key, item) => ::jet_foundation::MIR::MirTypeKind::Map {{
                     key: Box::new(__jet_bootstrap_type_to_host(key)?),
                     value: Box::new(__jet_bootstrap_type_to_host(item)?),
                 }},
                 {shared}(inner) => ::jet_foundation::MIR::MirTypeKind::Shared(Box::new(__jet_bootstrap_type_to_host(inner)?)),
                 {option}(inner) => ::jet_foundation::MIR::MirTypeKind::Option(Box::new(__jet_bootstrap_type_to_host(inner)?)),
                 {result}(ok, err) => ::jet_foundation::MIR::MirTypeKind::Result {{
                     ok: Box::new(__jet_bootstrap_type_to_host(ok)?),
                     err: Box::new(__jet_bootstrap_type_to_host(err)?),
                 }},
                 {fn_kind}(_) | {send_fn}(_, _, _) => return Err(\"function carriers are not native Prelude data\".to_string()),
                 {apply}(name, args) => ::jet_foundation::MIR::MirTypeKind::Apply {{
                     name: ::jet_foundation::MIR::MirNominalRef {{
                         id: __jet_bootstrap_type_id_to_host(&name.{nominal_id})?,
                         name: name.{nominal_name}.clone(),
                     }},
                     args: args.iter().map(__jet_bootstrap_type_to_host).collect::<Result<Vec<_>, _>>()?,
                 }},
                 {trait_object}(bounds) => ::jet_foundation::MIR::MirTypeKind::TraitObject(
                     bounds.iter().map(|bound| {{
                         let id = __jet_bootstrap_source_u64(&bound.{trait_ref_id}.{trait_id_value}, \"MIR trait identity\")?;
                         if id == 0 {{ return Err(\"MIR trait identity is zero\".to_string()); }}
                         Ok(::jet_foundation::MIR::MirTraitRef {{ id: ::jet_foundation::MIR::MirTraitId(id), name: bound.{trait_ref_name}.clone() }})
                     }}).collect::<Result<Vec<_>, String>>()?,
                 ),
                 {tuple}(fields) => ::jet_foundation::MIR::MirTypeKind::Tuple(
                     fields.iter().map(|field| Ok((
                         field.{tuple_name}.clone(),
                         __jet_bootstrap_type_to_host(&field.{tuple_ty})?,
                     ))).collect::<Result<Vec<_>, String>>()?,
                 ),
                 {fixed_list}(inner, len) => ::jet_foundation::MIR::MirTypeKind::FixedList {{
                     elem: Box::new(__jet_bootstrap_type_to_host(inner)?),
                     len: __jet_bootstrap_measure_to_host(len)?,
                 }},
                 {int_n}(signed, bits) => ::jet_foundation::MIR::MirTypeKind::IntN {{
                     signed: *signed,
                     bits: bits.to_string_rep().parse::<u8>().map_err(|_| \"integer width is outside u8\".to_string())?,
                 }},
                 {range}(base, lo, hi) => ::jet_foundation::MIR::MirTypeKind::InlineRange {{
                     base: Box::new(__jet_bootstrap_type_to_host(base)?),
                     lo: lo.to_i64().ok_or_else(|| \"inline range lower bound exceeds i64\".to_string())?,
                     hi: hi.to_i64().ok_or_else(|| \"inline range upper bound exceeds i64\".to_string())?,
                 }},
                 {float32} => ::jet_foundation::MIR::MirTypeKind::Float32,
                 {tagged}(marker, inner) => ::jet_foundation::MIR::MirTypeKind::Tagged {{
                     marker: __jet_bootstrap_tag_to_host(marker),
                     inner: Box::new(__jet_bootstrap_type_to_host(inner)?),
                 }},
                 {union}(members) => ::jet_foundation::MIR::MirTypeKind::Union(
                     members.iter().map(__jet_bootstrap_type_to_host).collect::<Result<Vec<_>, _>>()?,
                 ),
                 {quantity}(base, dimension) => ::jet_foundation::MIR::MirTypeKind::Quantity {{
                     base: Box::new(__jet_bootstrap_type_to_host(base)?),
                     dimension: __jet_bootstrap_dimension_to_host(dimension)?,
                 }},
                 {measure_kind}(value) => ::jet_foundation::MIR::MirTypeKind::Measure(
                     __jet_bootstrap_measure_to_host(value)?,
                 ),
             }};
             let mut result = ::jet_foundation::MIR::MirType::from_kind(kind);
             if let Some(identity) = ({value}).{type_identity}.as_ref().ok() {{
                 result = result.with_identity(__jet_bootstrap_type_id_to_host(identity)?);
             }}
             Ok(result)
         }}
         fn __jet_bootstrap_type_from_host(value: &::jet_foundation::MIR::MirType) -> Result<{mir_type}, String> {{
             let kind = match &value.kind {{
                 ::jet_foundation::MIR::MirTypeKind::Int => {int},
                 ::jet_foundation::MIR::MirTypeKind::Float => {float},
                 ::jet_foundation::MIR::MirTypeKind::Bool => {bool_kind},
                 ::jet_foundation::MIR::MirTypeKind::String => {string},
                 ::jet_foundation::MIR::MirTypeKind::Char => {char_kind},
                 ::jet_foundation::MIR::MirTypeKind::List(inner) => {list}(__jet_bootstrap_type_from_host(inner)?),
                 ::jet_foundation::MIR::MirTypeKind::Map {{ key, value }} => {map}(__jet_bootstrap_type_from_host(key)?, __jet_bootstrap_type_from_host(value)?),
                 ::jet_foundation::MIR::MirTypeKind::Shared(inner) => {shared}(__jet_bootstrap_type_from_host(inner)?),
                 ::jet_foundation::MIR::MirTypeKind::Option(inner) => {option}(__jet_bootstrap_type_from_host(inner)?),
                 ::jet_foundation::MIR::MirTypeKind::Result {{ ok, err }} => {result}(__jet_bootstrap_type_from_host(ok)?, __jet_bootstrap_type_from_host(err)?),
                 ::jet_foundation::MIR::MirTypeKind::Fn(_) | ::jet_foundation::MIR::MirTypeKind::SendFn {{ .. }} => return Err(\"function carriers are not native Prelude data\".to_string()),
                 ::jet_foundation::MIR::MirTypeKind::Apply {{ name, args }} => {apply}({mir_nominal_ref} {{
                     {nominal_id}: __jet_bootstrap_type_id_from_host(name.id)?,
                     {nominal_name}: name.name.clone(),
                 }}, args.iter().map(__jet_bootstrap_type_from_host).collect::<Result<Vec<_>, _>>()?),
                 ::jet_foundation::MIR::MirTypeKind::TraitObject(bounds) => {trait_object}(bounds.iter().map(|bound| {{
                     if bound.id.0 == 0 {{ return Err(\"MIR trait identity is zero\".to_string()); }}
                     Ok({mir_trait_ref} {{ {trait_ref_id}: {mir_trait_id} {{ {trait_id_value}: jet_foundation::Numeric::JetInt::from_str(&bound.id.0.to_string())? }}, {trait_ref_name}: bound.name.clone() }})
                 }}).collect::<Result<Vec<_>, String>>()?),
                 ::jet_foundation::MIR::MirTypeKind::Tuple(fields) => {tuple}(fields.iter().map(|(name, ty)| Ok({mir_tuple_field} {{
                     {tuple_name}: name.clone(),
                     {tuple_ty}: __jet_bootstrap_type_from_host(ty)?,
                 }})).collect::<Result<Vec<_>, String>>()?),
                 ::jet_foundation::MIR::MirTypeKind::FixedList {{ elem, len }} => {fixed_list}(__jet_bootstrap_type_from_host(elem)?, __jet_bootstrap_measure_from_host(len)?),
                 ::jet_foundation::MIR::MirTypeKind::IntN {{ signed, bits }} => {int_n}(*signed, jet_foundation::Numeric::JetInt::from_i64(i64::from(*bits))),
                 ::jet_foundation::MIR::MirTypeKind::InlineRange {{ base, lo, hi }} => {range}(__jet_bootstrap_type_from_host(base)?, jet_foundation::Numeric::JetInt::from_i64(*lo), jet_foundation::Numeric::JetInt::from_i64(*hi)),
                 ::jet_foundation::MIR::MirTypeKind::Float32 => {float32},
                 ::jet_foundation::MIR::MirTypeKind::Tagged {{ marker, inner }} => {tagged}(__jet_bootstrap_tag_from_host(marker)?, __jet_bootstrap_type_from_host(inner)?),
                 ::jet_foundation::MIR::MirTypeKind::Union(members) => {union}(members.iter().map(__jet_bootstrap_type_from_host).collect::<Result<Vec<_>, _>>()?),
                 ::jet_foundation::MIR::MirTypeKind::Quantity {{ base, dimension }} => {quantity}(__jet_bootstrap_type_from_host(base)?, __jet_bootstrap_dimension_from_host(dimension)?),
                 ::jet_foundation::MIR::MirTypeKind::Measure(value) => {measure_kind}(__jet_bootstrap_measure_from_host(value)?),
             }};
             Ok({mir_type} {{
                 {type_kind}: kind,
                 {type_identity}: value.identity.as_ref().map(__jet_bootstrap_type_id_from_host).transpose()?,
                 {type_layout}: __jet_bootstrap_layout_from_host(&value.layout)?,
             }})
         }}"
        ,
        value = "value",
        int = kind("Int"),
        float = kind("Float"),
        bool_kind = kind("Bool"),
        string = kind("String"),
        char_kind = kind("Char"),
        list = kind("List"),
        map = kind("Map"),
        shared = kind("Shared"),
        option = kind("Option"),
        result = kind("Result"),
        fn_kind = kind("Fn"),
        send_fn = kind("SendFn"),
        apply = kind("Apply"),
        trait_object = kind("TraitObject"),
        tuple = kind("Tuple"),
        fixed_list = kind("FixedList"),
        int_n = kind("IntN"),
        range = kind("InlineRange"),
        float32 = kind("Float32"),
        tagged = kind("Tagged"),
        union = kind("Union"),
        quantity = kind("Quantity"),
        measure_kind = kind("Measure"),
        mir_type = mir_type,
        type_kind = type_kind,
        type_identity = type_identity,
        mir_nominal_ref = mir_nominal_ref,
        nominal_id = nominal_id,
        nominal_name = nominal_name,
        mir_trait_ref = mir_trait_ref,
        mir_trait_id = mir_trait_id,
        trait_ref_id = trait_ref_id,
        trait_ref_name = trait_ref_name,
        trait_id_value = trait_id_value,
        mir_tuple_field = mir_tuple_field,
        tuple_name = tuple_name,
        tuple_ty = tuple_ty,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    Ok(())
}

fn emit_bootstrap_value_codec(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let ct = symbols.type_symbol("TComptimeValue")?;
    let key = symbols.type_symbol("TComptimeKey")?;
    let key_field = symbols.type_symbol("TComptimeKeyField")?;
    let map_entry = symbols.type_symbol("TComptimeMapEntry")?;
    let field = symbols.type_symbol("TComptimeField")?;
    let enum_arg = symbols.type_symbol("TComptimeEnumArg")?;
    let eval = symbols.type_symbol("JetEvalRuntimeValue")?;
    let runtime_aggregate = symbols.type_symbol("JetEvalRuntimeAggregate")?;
    let runtime_map_key = symbols.field_symbol("JetEvalRuntimeMapEntry", "key")?;
    let runtime_map_value = symbols.field_symbol("JetEvalRuntimeMapEntry", "value")?;
    let runtime_field_name = symbols.field_symbol("JetEvalRuntimeField", "name")?;
    let runtime_field_value = symbols.field_symbol("JetEvalRuntimeField", "value")?;
    let runtime_enum_name = symbols.field_symbol("JetEvalRuntimeEnumArg", "name")?;
    let runtime_enum_value = symbols.field_symbol("JetEvalRuntimeEnumArg", "value")?;

    let key_name = symbols.field_symbol("TComptimeKeyField", "name")?;
    let key_value = symbols.field_symbol("TComptimeKeyField", "key")?;
    let map_key = symbols.field_symbol("TComptimeMapEntry", "key")?;
    let map_value = symbols.field_symbol("TComptimeMapEntry", "value")?;
    let field_name = symbols.field_symbol("TComptimeField", "name")?;
    let field_value = symbols.field_symbol("TComptimeField", "value")?;
    let enum_name = symbols.field_symbol("TComptimeEnumArg", "name")?;
    let enum_value = symbols.field_symbol("TComptimeEnumArg", "value")?;

    let key_variants = [
        ("Int", symbols.variant_path("TComptimeKey", "Int")?),
        ("String", symbols.variant_path("TComptimeKey", "String")?),
        ("Bool", symbols.variant_path("TComptimeKey", "Bool")?),
        ("Char", symbols.variant_path("TComptimeKey", "Char")?),
        ("Tuple", symbols.variant_path("TComptimeKey", "Tuple")?),
        ("Struct", symbols.variant_path("TComptimeKey", "Struct")?),
        ("Enum", symbols.variant_path("TComptimeKey", "Enum")?),
    ];
    let key_variant = |name: &str| {
        key_variants
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, path)| path.clone())
            .unwrap_or_else(|| name.to_string())
    };
    let ct_names = [
        "Int", "Float", "Bool", "Char", "String", "BigInt", "Bytes", "List", "Map", "Struct",
        "Enum", "Present", "Failed", "Unit", "Closure",
    ];
    let ct_paths = ct_names
        .iter()
        .map(|name| {
            Ok::<_, BootstrapHostCodecError>((*name, symbols.variant_path("TComptimeValue", name)?))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let ct_variant = |name: &str| {
        ct_paths
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, path)| path.clone())
            .unwrap_or_else(|| name.to_string())
    };
    let report_told = symbols.variant_path("TComptimeReport", "Told")?;
    let eval_names = [
        "Moved",
        "Data",
        "Absent",
        "Result",
        "Closure",
        "Address",
        "SharedCell",
        "RangeCursor",
        "ListCursor",
        "ForeignHandle",
        "Aggregate",
        "RuntimeFailure",
        "HostCursor",
    ];
    let eval_paths = eval_names
        .iter()
        .map(|name| {
            Ok::<_, BootstrapHostCodecError>((
                *name,
                symbols.variant_path("JetEvalRuntimeValue", name)?,
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let eval_variant = |name: &str| {
        eval_paths
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, path)| path.clone())
            .unwrap_or_else(|| name.to_string())
    };
    let aggregate_names = ["List", "Map", "Struct", "Enum"];
    let aggregate_paths = aggregate_names
        .iter()
        .map(|name| {
            Ok::<_, BootstrapHostCodecError>((
                *name,
                symbols.variant_path("JetEvalRuntimeAggregate", name)?,
            ))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let aggregate_variant = |name: &str| {
        aggregate_paths
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, path)| path.clone())
            .unwrap_or_else(|| name.to_string())
    };

    let ct_int = ct_variant("Int");
    let ct_float = ct_variant("Float");
    let ct_bool = ct_variant("Bool");
    let ct_char = ct_variant("Char");
    let ct_string = ct_variant("String");
    let ct_bigint = ct_variant("BigInt");
    let ct_bytes = ct_variant("Bytes");
    let ct_list = ct_variant("List");
    let ct_map = ct_variant("Map");
    let ct_struct = ct_variant("Struct");
    let ct_enum = ct_variant("Enum");
    let ct_present = ct_variant("Present");
    let ct_failed = ct_variant("Failed");
    let ct_unit = ct_variant("Unit");
    let ct_closure = ct_variant("Closure");

    writeln!(
        out,
        "fn __jet_bootstrap_key_to_host(value: &{key}) -> Result<::jet_foundation::MIR::MirConstKey, String> {{
             match value {{
                 {key_int}(value) => Ok(::jet_foundation::MIR::MirConstKey::Int(value.to_i64().ok_or_else(|| \"constant key exceeds i64\".to_string())?)),
                 {key_string}(value) => Ok(::jet_foundation::MIR::MirConstKey::String(value.clone())),
                 {key_bool}(value) => Ok(::jet_foundation::MIR::MirConstKey::Bool(*value)),
                 {key_char}(value) => Ok(::jet_foundation::MIR::MirConstKey::Char(char::from_u32(value.to_i64().ok_or_else(|| \"constant key character exceeds i64\".to_string())? as u32).ok_or_else(|| \"constant key character is invalid\".to_string())?)),
                 {key_tuple}(fields) => Ok(::jet_foundation::MIR::MirConstKey::Tuple(fields.iter().map(|field| Ok((
                     field.{key_name}.clone(),
                     __jet_bootstrap_key_to_host(&field.{key_value})?,
                 ))).collect::<Result<Vec<_>, String>>()?)),
                 {key_struct}(type_name, fields) => Ok(::jet_foundation::MIR::MirConstKey::Struct {{
                     type_name: type_name.clone(),
                     fields: fields.iter().map(|field| Ok((
                         field.{key_name}.clone(),
                         __jet_bootstrap_key_to_host(&field.{key_value})?,
                     ))).collect::<Result<Vec<_>, String>>()?,
                 }}),
                 {key_enum}(type_name, variant) => Ok(::jet_foundation::MIR::MirConstKey::Enum {{
                     type_name: type_name.clone(),
                     variant: variant.clone(),
                 }}),
             }}
         }}"
        ,
        key_int = key_variant("Int"),
        key_string = key_variant("String"),
        key_bool = key_variant("Bool"),
        key_char = key_variant("Char"),
        key_tuple = key_variant("Tuple"),
        key_struct = key_variant("Struct"),
        key_enum = key_variant("Enum"),
        key_name = key_name,
        key_value = key_value,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    writeln!(
        out,
        "fn __jet_bootstrap_ct_to_host(value: &{ct}) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {{
             match value {{
                 {ct_int}(value) => Ok(match value.to_i64() {{
                     Some(value) => ::jet_foundation::MIR::MirRuntimeValue::Int(value),
                     None => ::jet_foundation::MIR::MirRuntimeValue::BigInt(value.to_string_rep()),
                 }}),
                 {ct_float}(value, bits) => Ok(::jet_foundation::MIR::MirRuntimeValue::Float {{ value: *value, f32: bits.to_i64() == Some(32) }}),
                 {ct_bool}(value) => Ok(::jet_foundation::MIR::MirRuntimeValue::Bool(*value)),
                 {ct_char}(value) => Ok(::jet_foundation::MIR::MirRuntimeValue::Char(char::from_u32(value.to_i64().ok_or_else(|| \"character exceeds i64\".to_string())? as u32).ok_or_else(|| \"invalid character value\".to_string())?)),
                 {ct_string}(value) => Ok(::jet_foundation::MIR::MirRuntimeValue::String(value.clone())),
                 {ct_bigint}(value) => Ok(::jet_foundation::MIR::MirRuntimeValue::BigInt(value.clone())),
                 {ct_bytes}(value) => Ok(::jet_foundation::MIR::MirRuntimeValue::Bytes(value.clone())),
                 {ct_list}(values) => Ok(::jet_foundation::MIR::MirRuntimeValue::List(values.iter().map(__jet_bootstrap_ct_to_host).collect::<Result<Vec<_>, _>>()?)),
                 {ct_map}(entries) => Ok(::jet_foundation::MIR::MirRuntimeValue::Map(entries.iter().map(|entry| Ok((
                     __jet_bootstrap_key_to_host(&entry.{map_key})?,
                     __jet_bootstrap_ct_to_host(&entry.{map_value})?,
                 ))).collect::<Result<Vec<_>, String>>()?)),
                 {ct_struct}(type_name, fields) => Ok(::jet_foundation::MIR::MirRuntimeValue::Struct {{
                     type_name: type_name.clone(),
                     fields: fields.iter().map(|field| Ok((
                         field.{field_name}.clone(),
                         __jet_bootstrap_ct_to_host(&field.{field_value})?,
                     ))).collect::<Result<Vec<_>, String>>()?,
                 }}),
                 {ct_enum}(type_name, variant, args) => Ok(::jet_foundation::MIR::MirRuntimeValue::Enum {{
                     type_name: type_name.clone(),
                     variant: variant.clone(),
                     args: args.iter().map(|arg| Ok((
                         arg.{enum_name}.clone(),
                         __jet_bootstrap_ct_to_host(&arg.{enum_value})?,
                     ))).collect::<Result<Vec<_>, String>>()?,
                 }}),
                 {ct_present}(value) => Ok(::jet_foundation::MIR::MirRuntimeValue::Present(Box::new(__jet_bootstrap_ct_to_host(value)?))),
                 {ct_failed}(failure) => match failure {{
                     {report_told}(value) => Ok(::jet_foundation::MIR::MirRuntimeValue::FailedTold(Box::new(__jet_bootstrap_ct_to_host(value)?))),
                     _ => Err(\"clean comptime failures carry an AST type, not a MIR type\".to_string()),
                 }},
                 {ct_unit} => Ok(::jet_foundation::MIR::MirRuntimeValue::Unit),
                 {ct_closure}(_) => Err(\"closure values are not materializable at the native Prelude boundary\".to_string()),
             }}
         }}"
        ,
        ct_int = ct_int,
        ct_float = ct_float,
        ct_bool = ct_bool,
        ct_char = ct_char,
        ct_string = ct_string,
        ct_bigint = ct_bigint,
        ct_bytes = ct_bytes,
        ct_list = ct_list,
        ct_map = ct_map,
        ct_struct = ct_struct,
        ct_enum = ct_enum,
        ct_present = ct_present,
        ct_failed = ct_failed,
        ct_unit = ct_unit,
        ct_closure = ct_closure,
        map_key = map_key,
        map_value = map_value,
        field_name = field_name,
        field_value = field_value,
        enum_name = enum_name,
        enum_value = enum_value,
        report_told = report_told,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    writeln!(
        out,
        "fn __jet_bootstrap_ct_from_host(value: &::jet_foundation::MIR::MirRuntimeValue) -> Result<{ct}, String> {{
             match value {{
                 ::jet_foundation::MIR::MirRuntimeValue::Int(value) => Ok({ct_int}(jet_foundation::Numeric::JetInt::from_i64(*value))),
                 ::jet_foundation::MIR::MirRuntimeValue::BigInt(value) => Ok({ct_bigint}(value.clone())),
                 ::jet_foundation::MIR::MirRuntimeValue::Float {{ value, f32 }} => Ok({ct_float}(*value, jet_foundation::Numeric::JetInt::from_i64(if *f32 {{ 32 }} else {{ 64 }}))),
                 ::jet_foundation::MIR::MirRuntimeValue::Bool(value) => Ok({ct_bool}(*value)),
                 ::jet_foundation::MIR::MirRuntimeValue::Char(value) => Ok({ct_char}(jet_foundation::Numeric::JetInt::from_i64(i64::from(*value as u32)))),
                 ::jet_foundation::MIR::MirRuntimeValue::String(value) => Ok({ct_string}(value.clone())),
                 ::jet_foundation::MIR::MirRuntimeValue::Bytes(value) => Ok({ct_bytes}(value.clone())),
                 ::jet_foundation::MIR::MirRuntimeValue::List(values) => Ok({ct_list}(values.iter().map(__jet_bootstrap_ct_from_host).collect::<Result<Vec<_>, _>>()?)),
                 ::jet_foundation::MIR::MirRuntimeValue::Map(entries) => Ok({ct_map}(entries.iter().map(|(key, value)| Ok({map_entry} {{
                     {map_key}: __jet_bootstrap_key_from_host(key)?,
                     {map_value}: __jet_bootstrap_ct_from_host(value)?,
                 }})).collect::<Result<Vec<_>, String>>()?)),
                 ::jet_foundation::MIR::MirRuntimeValue::Struct {{ type_name, fields }} => Ok({ct_struct}(type_name.clone(), fields.iter().map(|(name, value)| Ok({field} {{
                     {field_name}: name.clone(),
                     {field_value}: __jet_bootstrap_ct_from_host(value)?,
                 }})).collect::<Result<Vec<_>, String>>()?)),
                 ::jet_foundation::MIR::MirRuntimeValue::Enum {{ type_name, variant, args }} => Ok({ct_enum}(type_name.clone(), variant.clone(), args.iter().map(|(name, value)| Ok({enum_arg} {{
                     {enum_name}: name.clone(),
                     {enum_value}: __jet_bootstrap_ct_from_host(value)?,
                 }})).collect::<Result<Vec<_>, String>>()?)),
                 ::jet_foundation::MIR::MirRuntimeValue::Present(value) => Ok({ct_present}(__jet_bootstrap_ct_from_host(value)?)),
                 ::jet_foundation::MIR::MirRuntimeValue::FailedTold(value) => Ok({ct_failed}({report_told}(__jet_bootstrap_ct_from_host(value)?))),
                 ::jet_foundation::MIR::MirRuntimeValue::Absent {{ .. }} => return Err(\"a typed absent value is returned directly as JetEvalRuntimeValue::Absent\".to_string()),
                 ::jet_foundation::MIR::MirRuntimeValue::Unit => Ok({ct_unit}),
                ::jet_foundation::MIR::MirRuntimeValue::Moved | ::jet_foundation::MIR::MirRuntimeValue::Closure(_) | ::jet_foundation::MIR::MirRuntimeValue::NativeCursor(_) | ::jet_foundation::MIR::MirRuntimeValue::NativeOwned(_) => return Err("native Prelude returned an unmaterializable value".to_string()),
             }}
         }}"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    writeln!(
        out,
        r#"fn __jet_bootstrap_runtime_key_from_host(value: &::jet_foundation::MIR::MirRuntimeValue) -> Result<::jet_foundation::MIR::MirConstKey, String> {{
             match value {{
                 ::jet_foundation::MIR::MirRuntimeValue::Int(value) => Ok(::jet_foundation::MIR::MirConstKey::Int(*value)),
                 ::jet_foundation::MIR::MirRuntimeValue::String(value) => Ok(::jet_foundation::MIR::MirConstKey::String(value.clone())),
                 ::jet_foundation::MIR::MirRuntimeValue::Bool(value) => Ok(::jet_foundation::MIR::MirConstKey::Bool(*value)),
                 ::jet_foundation::MIR::MirRuntimeValue::Char(value) => Ok(::jet_foundation::MIR::MirConstKey::Char(*value)),
                 ::jet_foundation::MIR::MirRuntimeValue::Struct {{ type_name, fields }} if type_name == "Tuple" => Ok(::jet_foundation::MIR::MirConstKey::Tuple(fields.iter().map(|(name, value)| Ok((name.clone(), __jet_bootstrap_runtime_key_from_host(value)?))).collect::<Result<Vec<_>, String>>()?)),
                 ::jet_foundation::MIR::MirRuntimeValue::Struct {{ type_name, fields }} => Ok(::jet_foundation::MIR::MirConstKey::Struct {{
                     type_name: type_name.clone(),
                     fields: fields.iter().map(|(name, value)| Ok((name.clone(), __jet_bootstrap_runtime_key_from_host(value)?))).collect::<Result<Vec<_>, String>>()?,
                 }}),
                 ::jet_foundation::MIR::MirRuntimeValue::Enum {{ type_name, variant, args }} if args.is_empty() => Ok(::jet_foundation::MIR::MirConstKey::Enum {{
                     type_name: type_name.clone(),
                     variant: variant.clone(),
                 }}),
                 _ => Err("MIR runtime value is not a canonical constant map key".to_string()),
             }}
         }}
         fn __jet_bootstrap_eval_key_to_host(value: &{eval}) -> Result<::jet_foundation::MIR::MirConstKey, String> {{
             match value {{
                 {eval_data}(value) => __jet_bootstrap_ct_to_host(value).and_then(|value| __jet_bootstrap_runtime_key_from_host(&value)),
                 _ => Err("runtime map key is not materialized immutable data".to_string()),
             }}
         }}
         fn __jet_bootstrap_eval_aggregate_to_host(
             value: &{runtime_aggregate},
             allow_foreign_handles: bool,
         ) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {{
             match value {{
                 {aggregate_list}(values) => Ok(::jet_foundation::MIR::MirRuntimeValue::List(values.iter().map(|value| __jet_bootstrap_eval_to_host_inner(value, allow_foreign_handles)).collect::<Result<Vec<_>, String>>()?)),
                 {aggregate_map}(entries) => Ok(::jet_foundation::MIR::MirRuntimeValue::Map(entries.iter().map(|entry| Ok((
                     __jet_bootstrap_eval_key_to_host(&entry.{runtime_map_key})?,
                     __jet_bootstrap_eval_to_host_inner(&entry.{runtime_map_value}, allow_foreign_handles)?,
                 ))).collect::<Result<Vec<_>, String>>()?)),
                 {aggregate_struct}(type_name, fields) => Ok(::jet_foundation::MIR::MirRuntimeValue::Struct {{
                     type_name: type_name.clone(),
                     fields: fields.iter().map(|field| Ok((
                         field.{runtime_field_name}.clone(),
                         __jet_bootstrap_eval_to_host_inner(&field.{runtime_field_value}, allow_foreign_handles)?,
                     ))).collect::<Result<Vec<_>, String>>()?,
                 }}),
                 {aggregate_enum}(type_name, variant, args) => Ok(::jet_foundation::MIR::MirRuntimeValue::Enum {{
                     type_name: type_name.clone(),
                     variant: variant.clone(),
                     args: args.iter().map(|arg| Ok((
                         arg.{runtime_enum_name}.as_ref().ok().cloned(),
                         __jet_bootstrap_eval_to_host_inner(&arg.{runtime_enum_value}, allow_foreign_handles)?,
                     ))).collect::<Result<Vec<_>, String>>()?,
                 }}),
             }}
         }}
         fn __jet_bootstrap_eval_to_host_inner(
             value: &{eval},
             allow_foreign_handles: bool,
         ) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {{
             match value {{
                 {eval_moved} => Err("moved values cannot cross a native host boundary".to_string()),
                 {eval_data}(value) => __jet_bootstrap_ct_to_host(value),
                 {eval_absent}(element) => Ok(::jet_foundation::MIR::MirRuntimeValue::Absent {{ element: __jet_bootstrap_type_to_host(element)? }}),
                 {eval_result}(ok, value) => {{
                     let value = __jet_bootstrap_eval_to_host_inner(value, allow_foreign_handles)?;
                     Ok(if *ok {{
                         ::jet_foundation::MIR::MirRuntimeValue::Present(Box::new(value))
                     }} else {{
                         ::jet_foundation::MIR::MirRuntimeValue::FailedTold(Box::new(value))
                     }})
                 }}
                 {eval_foreign_handle}(_, raw, _) if allow_foreign_handles => Ok(::jet_foundation::MIR::MirRuntimeValue::Int(raw.to_i64().ok_or_else(|| "foreign handle is outside i64".to_string())?)),
                 {eval_foreign_handle}(_, _, _) => Err("foreign handles cross only their checked C foreign-call ABI boundary".to_string()),
                 {eval_aggregate}(aggregate) => __jet_bootstrap_eval_aggregate_to_host(aggregate, allow_foreign_handles),
                 {eval_runtime_failure}(_, _) => Err("terminal runtime failures cannot cross a native host boundary".to_string()),
                 {eval_host_cursor}(_, _, _, _) => Err("native iterator cursors cannot cross a serializable host boundary".to_string()),
                 {eval_closure}(_, _) | {eval_address}(_, _, _) | {eval_shared}(_, _) | {eval_range}(_, _, _, _, _) | {eval_list_cursor}(_, _, _) => Err("runtime handles and closures cannot cross a native host boundary".to_string()),
             }}
         }}
         fn __jet_bootstrap_eval_to_host(value: &{eval}) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {{
             __jet_bootstrap_eval_to_host_inner(value, false)
         }}
         // This projection is confined to a checked MIR foreign-call row, whose parameter facts own the C ABI.
         fn __jet_bootstrap_eval_to_foreign_host(value: &{eval}) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {{
             __jet_bootstrap_eval_to_host_inner(value, true)
         }}"#,
        eval = eval,
        runtime_aggregate = runtime_aggregate,
        runtime_map_key = runtime_map_key,
        runtime_map_value = runtime_map_value,
        runtime_field_name = runtime_field_name,
        runtime_field_value = runtime_field_value,
        runtime_enum_name = runtime_enum_name,
        runtime_enum_value = runtime_enum_value,
        aggregate_list = aggregate_variant("List"),
        aggregate_map = aggregate_variant("Map"),
        aggregate_struct = aggregate_variant("Struct"),
        aggregate_enum = aggregate_variant("Enum"),
        eval_moved = eval_variant("Moved"),
        eval_data = eval_variant("Data"),
        eval_absent = eval_variant("Absent"),
        eval_result = eval_variant("Result"),
        eval_foreign_handle = eval_variant("ForeignHandle"),
        eval_aggregate = eval_variant("Aggregate"),
        eval_runtime_failure = eval_variant("RuntimeFailure"),
        eval_host_cursor = eval_variant("HostCursor"),
        eval_closure = eval_variant("Closure"),
        eval_address = eval_variant("Address"),
        eval_shared = eval_variant("SharedCell"),
        eval_range = eval_variant("RangeCursor"),
        eval_list_cursor = eval_variant("ListCursor"),
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;

    // The reverse key projection is kept separate from value projection so a
    // map's checked key carrier cannot accidentally be stringified.
    writeln!(
        out,
        "fn __jet_bootstrap_key_from_host(value: &::jet_foundation::MIR::MirConstKey) -> Result<{key}, String> {{
             match value {{
                 ::jet_foundation::MIR::MirConstKey::Int(value) => Ok({key_int}(jet_foundation::Numeric::JetInt::from_i64(*value))),
                 ::jet_foundation::MIR::MirConstKey::String(value) => Ok({key_string}(value.clone())),
                 ::jet_foundation::MIR::MirConstKey::Bool(value) => Ok({key_bool}(*value)),
                 ::jet_foundation::MIR::MirConstKey::Char(value) => Ok({key_char}(jet_foundation::Numeric::JetInt::from_i64(i64::from(*value as u32)))),
                 ::jet_foundation::MIR::MirConstKey::Tuple(fields) => Ok({key_tuple}(fields.iter().map(|(name, key)| Ok({key_field} {{
                     {key_name}: name.clone(),
                     {key_value}: __jet_bootstrap_key_from_host(key)?,
                 }})).collect::<Result<Vec<_>, String>>()?)),
                 ::jet_foundation::MIR::MirConstKey::Struct {{ type_name, fields }} => Ok({key_struct}(type_name.clone(), fields.iter().map(|(name, key)| Ok({key_field} {{
                     {key_name}: name.clone(),
                     {key_value}: __jet_bootstrap_key_from_host(key)?,
                 }})).collect::<Result<Vec<_>, String>>()?)),
                 ::jet_foundation::MIR::MirConstKey::Enum {{ type_name, variant }} => Ok({key_enum}(type_name.clone(), variant.clone())),
             }}
         }}"
        ,
        key = key,
        key_int = key_variant("Int"),
        key_string = key_variant("String"),
        key_bool = key_variant("Bool"),
        key_char = key_variant("Char"),
        key_tuple = key_variant("Tuple"),
        key_struct = key_variant("Struct"),
        key_enum = key_variant("Enum"),
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    Ok(())
}


fn emit_bootstrap_host_value_codec(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let ct = symbols.type_symbol("TComptimeValue")?;
    let field = symbols.type_symbol("TComptimeField")?;
    let enum_arg = symbols.type_symbol("TComptimeEnumArg")?;
    let host = symbols.type_symbol("JetEvalHostValue")?;
    let host_map_entry = symbols.type_symbol("JetEvalHostMapEntry")?;
    let host_field = symbols.type_symbol("JetEvalHostField")?;
    let host_enum_arg = symbols.type_symbol("JetEvalHostEnumArg")?;
    let host_data = symbols.variant_path("JetEvalHostValue", "Data")?;
    let host_absent = symbols.variant_path("JetEvalHostValue", "Absent")?;
    let host_present = symbols.variant_path("JetEvalHostValue", "Present")?;
    let host_failed = symbols.variant_path("JetEvalHostValue", "Failed")?;
    let host_list = symbols.variant_path("JetEvalHostValue", "List")?;
    let host_map = symbols.variant_path("JetEvalHostValue", "Map")?;
    let host_struct = symbols.variant_path("JetEvalHostValue", "Struct")?;
    let host_enum = symbols.variant_path("JetEvalHostValue", "Enum")?;
    let host_map_key = symbols.field_symbol("JetEvalHostMapEntry", "key")?;
    let host_map_value = symbols.field_symbol("JetEvalHostMapEntry", "value")?;
    let host_field_name = symbols.field_symbol("JetEvalHostField", "name")?;
    let host_field_value = symbols.field_symbol("JetEvalHostField", "value")?;
    let host_enum_name = symbols.field_symbol("JetEvalHostEnumArg", "name")?;
    let host_enum_value = symbols.field_symbol("JetEvalHostEnumArg", "value")?;
    let ct_int = symbols.variant_path("TComptimeValue", "Int")?;
    let ct_string = symbols.variant_path("TComptimeValue", "String")?;
    let ct_bool = symbols.variant_path("TComptimeValue", "Bool")?;
    let ct_char = symbols.variant_path("TComptimeValue", "Char")?;
    let ct_struct = symbols.variant_path("TComptimeValue", "Struct")?;
    let ct_enum = symbols.variant_path("TComptimeValue", "Enum")?;
    let ct_field_name = symbols.field_symbol("TComptimeField", "name")?;
    let ct_field_value = symbols.field_symbol("TComptimeField", "value")?;

    writeln!(
        out,
        r#"fn __jet_bootstrap_ct_key_value_from_host(value: &::jet_foundation::MIR::MirConstKey) -> Result<{ct}, String> {{
             match value {{
                 ::jet_foundation::MIR::MirConstKey::Int(value) => Ok({ct_int}(jet_foundation::Numeric::JetInt::from_i64(*value))),
                 ::jet_foundation::MIR::MirConstKey::String(value) => Ok({ct_string}(value.clone())),
                 ::jet_foundation::MIR::MirConstKey::Bool(value) => Ok({ct_bool}(*value)),
                 ::jet_foundation::MIR::MirConstKey::Char(value) => Ok({ct_char}(jet_foundation::Numeric::JetInt::from_i64(i64::from(*value as u32)))),
                 ::jet_foundation::MIR::MirConstKey::Tuple(fields) => Ok({ct_struct}("Tuple".to_string(), fields.iter().map(|(name, key)| Ok({field} {{
                     {ct_field_name}: name.clone(),
                     {ct_field_value}: __jet_bootstrap_ct_key_value_from_host(key)?,
                 }})).collect::<Result<Vec<_>, String>>()?)),
                 ::jet_foundation::MIR::MirConstKey::Struct {{ type_name, fields }} => Ok({ct_struct}(type_name.clone(), fields.iter().map(|(name, key)| Ok({field} {{
                     {ct_field_name}: name.clone(),
                     {ct_field_value}: __jet_bootstrap_ct_key_value_from_host(key)?,
                 }})).collect::<Result<Vec<_>, String>>()?)),
                 ::jet_foundation::MIR::MirConstKey::Enum {{ type_name, variant }} => Ok({ct_enum}(type_name.clone(), variant.clone(), Vec::<{enum_arg}>::new())),
             }}
         }}
         fn __jet_bootstrap_host_value_from_runtime(value: &::jet_foundation::MIR::MirRuntimeValue) -> Result<{host}, String> {{
             match value {{
                 ::jet_foundation::MIR::MirRuntimeValue::Present(value) => Ok({host_present}(__jet_bootstrap_host_value_from_runtime(value)?)),
                 ::jet_foundation::MIR::MirRuntimeValue::FailedTold(value) => Ok({host_failed}(__jet_bootstrap_host_value_from_runtime(value)?)),
                 ::jet_foundation::MIR::MirRuntimeValue::Absent {{ element }} => Ok({host_absent}(__jet_bootstrap_type_from_host(element)?)),
                 ::jet_foundation::MIR::MirRuntimeValue::List(values) => Ok({host_list}(values.iter().map(__jet_bootstrap_host_value_from_runtime).collect::<Result<Vec<_>, String>>()?)),
                 ::jet_foundation::MIR::MirRuntimeValue::Map(entries) => Ok({host_map}(entries.iter().map(|(key, value)| Ok({host_map_entry} {{
                     {host_map_key}: {host_data}(__jet_bootstrap_ct_key_value_from_host(key)?),
                     {host_map_value}: __jet_bootstrap_host_value_from_runtime(value)?,
                 }})).collect::<Result<Vec<_>, String>>()?)),
                 ::jet_foundation::MIR::MirRuntimeValue::Struct {{ type_name, fields }} => Ok({host_struct}(type_name.clone(), fields.iter().map(|(name, value)| Ok({host_field} {{
                     {host_field_name}: name.clone(),
                     {host_field_value}: __jet_bootstrap_host_value_from_runtime(value)?,
                 }})).collect::<Result<Vec<_>, String>>()?)),
                 ::jet_foundation::MIR::MirRuntimeValue::Enum {{ type_name, variant, args }} => Ok({host_enum}(type_name.clone(), variant.clone(), args.iter().map(|(name, value)| Ok({host_enum_arg} {{
                     {host_enum_name}: name.clone().ok_or(::jet_foundation::Outcome::JetAbsent),
                     {host_enum_value}: __jet_bootstrap_host_value_from_runtime(value)?,
                 }})).collect::<Result<Vec<_>, String>>()?)),
                ::jet_foundation::MIR::MirRuntimeValue::Moved | ::jet_foundation::MIR::MirRuntimeValue::Closure(_) | ::jet_foundation::MIR::MirRuntimeValue::NativeCursor(_) | ::jet_foundation::MIR::MirRuntimeValue::NativeOwned(_) => Err("native runtime resource requires its checked owner packet".to_string()),
                 _ => Ok({host_data}(__jet_bootstrap_ct_from_host(value)?)),
             }}
         }}"#,
        ct = ct,
        field = field,
        enum_arg = enum_arg,
        host = host,
        host_map_entry = host_map_entry,
        host_field = host_field,
        host_enum_arg = host_enum_arg,
        host_data = host_data,
        host_absent = host_absent,
        host_present = host_present,
        host_failed = host_failed,
        host_list = host_list,
        host_map = host_map,
        host_struct = host_struct,
        host_enum = host_enum,
        host_map_key = host_map_key,
        host_map_value = host_map_value,
        host_field_name = host_field_name,
        host_field_value = host_field_value,
        host_enum_name = host_enum_name,
        host_enum_value = host_enum_value,
        ct_int = ct_int,
        ct_string = ct_string,
        ct_bool = ct_bool,
        ct_char = ct_char,
        ct_struct = ct_struct,
        ct_enum = ct_enum,
        ct_field_name = ct_field_name,
        ct_field_value = ct_field_value,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    Ok(())
}
fn emit_bootstrap_host_type_shape_codec(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let shape = symbols.type_symbol("JetEvalHostTypeShape")?;
    let type_id_value = symbols.field_symbol("MirTypeId", "value")?;
    let shape_nodes = symbols.field_symbol("JetEvalHostTypeShape", "nodes")?;
    let shape_root = symbols.field_symbol("JetEvalHostTypeShape", "root")?;
    let field_name = symbols.field_symbol("JetEvalHostTypeFieldShape", "name")?;
    let field_node = symbols.field_symbol("JetEvalHostTypeFieldShape", "node")?;
    let owner_core = symbols.variant_path("JetEvalHostOwner", "Core")?;
    let owner_declared = symbols.variant_path("JetEvalHostOwner", "Declared")?;
    let host_core_owner_ty = symbols.field_symbol("JetEvalHostCoreOwner", "ty")?;
    let nodes = [
        "Scalar", "Option", "Result", "List", "Map", "Tuple", "Struct", "Enum", "Closure",
        "Handle",
    ]
    .iter()
    .map(|name| {
        Ok::<_, BootstrapHostCodecError>((
            *name,
            symbols.variant_path("JetEvalHostTypeNode", name)?,
        ))
    })
    .collect::<Result<Vec<_>, _>>()?;
    let node = |name: &str| {
        nodes
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, path)| path.clone())
            .unwrap_or_else(|| name.to_string())
    };

    writeln!(
        out,
        r#"fn __jet_bootstrap_type_from_shape(shape: &{shape}) -> Result<::jet_foundation::MIR::MirType, String> {{
             let root = __jet_bootstrap_source_index(&shape.{shape_root}, "host result type root")?;
             __jet_bootstrap_type_from_shape_node(shape, root, 0)
         }}
         fn __jet_bootstrap_type_from_shape_node(
             shape: &{shape},
             index: usize,
             depth: usize,
         ) -> Result<::jet_foundation::MIR::MirType, String> {{
             if depth >= shape.{shape_nodes}.len() {{
                 return Err("checked host type shape contains a recursive non-nominal type".to_string());
             }}
             let node = shape.{shape_nodes}.get(index)
                 .ok_or_else(|| "checked host type shape has an invalid node reference".to_string())?;
             match node {{
                 {scalar}(ty) => __jet_bootstrap_type_to_host(ty),
                 {option}(inner) => Ok(::jet_foundation::MIR::MirType::from_kind(
                     ::jet_foundation::MIR::MirTypeKind::Option(Box::new(
                         __jet_bootstrap_type_from_shape_node(shape, __jet_bootstrap_source_index(inner, "option type node")?, depth + 1)?,
                     )),
                 )),
                 {result}(ok, error) => Ok(::jet_foundation::MIR::MirType::from_kind(
                     ::jet_foundation::MIR::MirTypeKind::Result {{
                         ok: Box::new(__jet_bootstrap_type_from_shape_node(shape, __jet_bootstrap_source_index(ok, "result success type node")?, depth + 1)?),
                         err: Box::new(__jet_bootstrap_type_from_shape_node(shape, __jet_bootstrap_source_index(error, "result error type node")?, depth + 1)?),
                     }},
                 )),
                 {list}(inner) => Ok(::jet_foundation::MIR::MirType::from_kind(
                     ::jet_foundation::MIR::MirTypeKind::List(Box::new(
                         __jet_bootstrap_type_from_shape_node(shape, __jet_bootstrap_source_index(inner, "list element type node")?, depth + 1)?,
                     )),
                 )),
                 {map}(key, value) => Ok(::jet_foundation::MIR::MirType::from_kind(
                     ::jet_foundation::MIR::MirTypeKind::Map {{
                         key: Box::new(__jet_bootstrap_type_from_shape_node(shape, __jet_bootstrap_source_index(key, "map key type node")?, depth + 1)?),
                         value: Box::new(__jet_bootstrap_type_from_shape_node(shape, __jet_bootstrap_source_index(value, "map value type node")?, depth + 1)?),
                     }},
                 )),
                 {tuple}(fields) => Ok(::jet_foundation::MIR::MirType::from_kind(
                     ::jet_foundation::MIR::MirTypeKind::Tuple(fields.iter().map(|field| Ok((
                         field.{field_name}.clone(),
                         __jet_bootstrap_type_from_shape_node(shape, __jet_bootstrap_source_index(&field.{field_node}, "tuple field type node")?, depth + 1)?,
                     ))).collect::<Result<Vec<_>, String>>()?),
                 )),
                 {structure}(type_id, type_name, args, _) | {enumeration}(type_id, type_name, args, _) => {{
                     let id = ::jet_foundation::MIR::MirTypeId(__jet_bootstrap_source_u64(&type_id.{type_id_value}, "host nominal type ID")?);
                     let args = args.iter().map(|arg| {{
                         __jet_bootstrap_type_from_shape_node(shape, __jet_bootstrap_source_index(arg, "nominal argument type node")?, depth + 1)
                     }}).collect::<Result<Vec<_>, String>>()?;
                     Ok(::jet_foundation::MIR::MirType::from_kind(::jet_foundation::MIR::MirTypeKind::Apply {{
                         name: ::jet_foundation::MIR::MirNominalRef {{ id, name: type_name.clone() }},
                         args,
                     }}).with_identity(id))
                 }}
                {handle}(owner) => match owner {{
                    {owner_core}(owner) => __jet_bootstrap_type_to_host(&owner.{host_core_owner_ty}),
                    {owner_declared}(ty) => __jet_bootstrap_type_to_host(ty),
                    _ => Err("cursor owner shape does not expose a native MIR type".to_string()),
                }},
                 {closure}(_, _) => Err("closure type shape cannot be passed to a native Prelude route".to_string()),
             }}
         }}"#,
        shape = shape,
        shape_root = shape_root,
        shape_nodes = shape_nodes,
        field_name = field_name,
        field_node = field_node,
        type_id_value = type_id_value,
        scalar = node("Scalar"),
        option = node("Option"),
        result = node("Result"),
        list = node("List"),
        map = node("Map"),
        tuple = node("Tuple"),
        structure = node("Struct"),
        enumeration = node("Enum"),
        handle = node("Handle"),
        closure = node("Closure"),
        owner_core = owner_core,
        owner_declared = owner_declared,
        host_core_owner_ty = host_core_owner_ty,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    Ok(())
}


fn emit_bootstrap_source_resource_bridge(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let row = symbols.type_symbol("MirPreludeCall")?;
    let eval = symbols.type_symbol("JetEvalRuntimeValue")?;
    let host_argument = symbols.type_symbol("JetEvalHostArgument")?;
    let host_argument_value = symbols.field_symbol("JetEvalHostArgument", "value")?;
    let host_result = symbols.type_symbol("JetEvalHostResult")?;
    let host_outcome = symbols.type_symbol("JetEvalHostOutcome")?;
    let host_transfer = symbols.type_symbol("JetEvalHostTransfer")?;
    let host_value = symbols.type_symbol("JetEvalHostValue")?;
    let host_result_outcome = symbols.field_symbol("JetEvalHostResult", "outcome")?;
    let host_result_transfers = symbols.field_symbol("JetEvalHostResult", "transfers")?;
    let transfer_argument = symbols.field_symbol("JetEvalHostTransfer", "argument")?;
    let transfer_path = symbols.field_symbol("JetEvalHostTransfer", "path")?;
    let transfer_token = symbols.field_symbol("JetEvalHostTransfer", "token")?;
    let transfer_handle = symbols.field_symbol("JetEvalHostTransfer", "handle")?;
    let transfer_raw = symbols.field_symbol("JetEvalHostTransfer", "raw")?;
    let host_owner = symbols.type_symbol("JetEvalHostOwner")?;
    let host_core_owner = symbols.type_symbol("JetEvalHostCoreOwner")?;
    let host_core_owner_fact = symbols.field_symbol("JetEvalHostCoreOwner", "fact")?;
    let host_core_owner_ty = symbols.field_symbol("JetEvalHostCoreOwner", "ty")?;
    let eval_machine = symbols.type_symbol("JetEvalMachine")?;
    let machine_program = symbols.field_symbol("JetEvalMachine", "program")?;
    let program_core_owners = symbols.field_symbol("MirProgram", "core_owners")?;
    let core_owner_id = symbols.field_symbol("MirCoreOwner", "id")?;
    let type_id_value = symbols.field_symbol("MirTypeId", "value")?;
    let source_handle = symbols.type_symbol("MirHandleId")?;
    let handle_value = symbols.field_symbol("MirHandleId", "value")?;
    let row_module = symbols.field_symbol("MirPreludeCall", "module_name")?;
    let row_member = symbols.field_symbol("MirPreludeCall", "member")?;
    let row_symbol = symbols.field_symbol("MirPreludeCall", "symbol")?;
    let shape = symbols.type_symbol("JetEvalHostTypeShape")?;
    let shape_root = symbols.field_symbol("JetEvalHostTypeShape", "root")?;
    let shape_nodes = symbols.field_symbol("JetEvalHostTypeShape", "nodes")?;
    let node_option = symbols.variant_path("JetEvalHostTypeNode", "Option")?;
    let node_result = symbols.variant_path("JetEvalHostTypeNode", "Result")?;
    let node_handle = symbols.variant_path("JetEvalHostTypeNode", "Handle")?;
    let host_data = symbols.variant_path("JetEvalHostValue", "Data")?;
    let host_handle = symbols.variant_path("JetEvalHostValue", "Handle")?;
    let host_failed = symbols.variant_path("JetEvalHostValue", "Failed")?;
    let host_owner_cursor = symbols.variant_path("JetEvalHostOwner", "Cursor")?;
    let host_owner_core = symbols.variant_path("JetEvalHostOwner", "Core")?;
    let result_value = symbols.variant_path("JetEvalHostOutcome", "Value")?;
    let result_runtime_failure =
        symbols.variant_path("JetEvalHostOutcome", "RuntimeFailure")?;
    let eval_foreign_handle = symbols.variant_path("JetEvalRuntimeValue", "ForeignHandle")?;
    let ct_bool = symbols.variant_path("TComptimeValue", "Bool")?;
    let ct_unit = symbols.variant_path("TComptimeValue", "Unit")?;
    let cap_type = "::jet_jit::SourceResources::SourceResourceHandle";
    let mir_type_kind = "::jet_foundation::MIR::MirTypeKind";
    let mir_handle = "::jet_foundation::MIR::MirHandleId";
    let native_error = "::jet_foundation::MIR::MirNativeCursorError";
    let resource_key = "::jet_codegen::Codegen::NativeLoopCursor::NativeLoopResourceKey";

    writeln!(
        out,
r#"fn __jet_bootstrap_host_reply_with_transfers(
             outcome: {host_outcome},
             transfers: Vec<{host_transfer}>,
         ) -> {host_result} {{
             {host_result} {{
                 {host_result_transfers}: transfers,
                 {host_result_outcome}: outcome,
             }}
         }}
         fn __jet_bootstrap_host_reply(outcome: {host_outcome}) -> {host_result} {{
             __jet_bootstrap_host_reply_with_transfers(outcome, Vec::new())
         }}
         fn __jet_bootstrap_shape_owner(shape: &{shape}) -> Result<{host_owner}, String> {{
             let mut node_index = __jet_bootstrap_source_index(&shape.{shape_root}, "host resource type root")?;
             for _ in 0..shape.{shape_nodes}.len() {{
                 let node = shape.{shape_nodes}.get(node_index)
                     .ok_or_else(|| "host resource type shape has an invalid node reference".to_string())?;
                 match node {{
                     {node_option}(inner) => node_index = __jet_bootstrap_source_index(inner, "host resource type node")?,
                     {node_result}(ok, _) => node_index = __jet_bootstrap_source_index(ok, "host resource result node")?,
                     {node_handle}(owner) => return Ok(owner.clone()),
                     _ => return Err("checked host resource type shape is not a native handle".to_string()),
                 }}
             }}
             Err("host resource type shape cycles through result wrappers".to_string())
         }}
        fn __jet_bootstrap_core_owner_from_shape(
            machine: &{eval_machine},
            shape: &{shape},
        ) -> Result<({mir_handle}, {host_owner}, ::jet_foundation::MIR::MirCoreOwner, ::jet_foundation::MIR::MirType, ::jet_jit::SourceResources::SourceResourceKind), String> {{
            let owner = __jet_bootstrap_shape_owner(shape)?;
            let core_owner = match &owner {{
                {host_owner_core}(core_owner) => core_owner,
                _ => return Err("checked resource shape does not declare a Core owner".to_string()),
            }};
            let fact = __jet_bootstrap_mir_MirCoreOwner_to_host(&core_owner.{host_core_owner_fact})?;
            let owner_type = __jet_bootstrap_type_to_host(&core_owner.{host_core_owner_ty})?;
            let {mir_type_kind}::Apply {{ name, .. }} = &owner_type.kind else {{
                return Err("checked Core resource owner is not a nominal Apply".to_string());
            }};
            if name.id != fact.nominal_id || owner_type.identity.is_some_and(|identity| identity != fact.nominal_id) {{
                return Err("checked Core resource type disagrees with its registered nominal identity".to_string());
            }}
            let mut registered = false;
            for row in machine.{machine_program}.{program_core_owners}.iter() {{
                let row_id = __jet_bootstrap_source_u64(&row.{core_owner_id}.{type_id_value}, "Core owner registration ID")?;
                if row_id == fact.id.0 {{
                    if registered {{
                        return Err("checked Core owner registration ID is not unique".to_string());
                    }}
                    let row_fact = __jet_bootstrap_mir_MirCoreOwner_to_host(row)?;
                    if row_fact != fact {{
                        return Err("checked Core owner fact disagrees with its registered MIR row".to_string());
                    }}
                    registered = true;
                }}
            }}
            if !registered {{
                return Err("checked Core owner fact is absent from the active MIR program".to_string());
            }}
            let core_kind = ::jet_jit::SourceResources::SourceResourceKind::from_core_owner_type(&fact, &owner_type)?;
            Ok(({mir_handle}(name.id.0), owner, fact, owner_type, core_kind))
        }}
         fn __jet_bootstrap_resource_host_handle(
             capability: &{cap_type},
             owner: {host_owner},
         ) -> Result<{host_value}, String> {{
             let handle_value = jet_foundation::Numeric::JetInt::from_str(&capability.handle.0.to_string())?;
             Ok({host_handle}(
                 {source_handle} {{ {handle_value}: handle_value }},
                 jet_foundation::Numeric::JetInt::from_i64(capability.raw),
                 owner,
                {host_data}({ct_unit}),
             ))
         }}
         fn __jet_bootstrap_resource_factory_result(
             result: Result<{cap_type}, {native_error}>,
             owner: {host_owner},
             span: {span},
         ) -> {host_result} {{
             match result {{
                 Ok(capability) => match __jet_bootstrap_resource_host_handle(&capability, owner) {{
                    Ok(value) => __jet_bootstrap_host_reply({result_value}(value)),
                     Err(detail) => __jet_bootstrap_host_failure(span, detail),
                 }},
                 Err({native_error}::Value(value)) => match __jet_bootstrap_host_value_from_runtime(&value) {{
                    Ok(value) => __jet_bootstrap_host_reply({result_value}({host_failed}(value))),
                     Err(detail) => __jet_bootstrap_host_failure(span, detail),
                 }},
                 Err({native_error}::Internal(detail)) => __jet_bootstrap_host_failure(span, detail),
             }}
         }}
        fn __jet_bootstrap_cursor_error_with_transfers(
            error: {native_error},
            span: {span},
            transfers: Vec<{host_transfer}>,
        ) -> {host_result} {{
            match error {{
                {native_error}::Value(value) => match __jet_bootstrap_host_value_from_runtime(&value) {{
                    Ok(value) => __jet_bootstrap_host_reply_with_transfers({result_runtime_failure}(value, span), transfers),
                    Err(detail) => __jet_bootstrap_host_failure_with_transfers(span, detail, transfers),
                }},
                {native_error}::Internal(detail) => __jet_bootstrap_host_failure_with_transfers(span, detail, transfers),
            }}
        }}
        fn __jet_bootstrap_host_failure_with_transfers(
            span: {span},
            detail: String,
            transfers: Vec<{host_transfer}>,
        ) -> {host_result} {{
            __jet_bootstrap_host_reply_with_transfers(__jet_bootstrap_host_failure_outcome(span, detail), transfers)
        }}
        fn __jet_bootstrap_resource_transfer_from_eval(
            value: &{eval},
            consumed: &{cap_type},
        ) -> Result<{host_transfer}, String> {{
            let {eval_foreign_handle}(handle, raw, token) = value else {{
                return Err("committed Source resource transfer has no checked foreign-handle input".to_string());
            }};
            let handle = __jet_bootstrap_source_u64(&handle.{handle_value}, "Source resource transfer handle")?;
            let raw_value = raw.to_string_rep().parse::<i64>()
                .map_err(|_| "Source resource transfer capability is outside i64".to_string())?;
            if consumed.handle != {mir_handle}(handle) || consumed.raw != raw_value {{
                return Err("committed Source resource transfer disagrees with its checked input capability".to_string());
            }}
            Ok({host_transfer} {{
                {transfer_argument}: jet_foundation::Numeric::JetInt::from_i64(0),
                {transfer_path}: Vec::new(),
                {transfer_token}: token.clone(),
                {transfer_handle}: {mir_handle}(handle),
                {transfer_raw}: raw.clone(),
            }})
        }}
         fn __jet_bootstrap_cursor_error(error: {native_error}, span: {span}) -> {host_result} {{
             match error {{
                 {native_error}::Value(value) => match __jet_bootstrap_host_value_from_runtime(&value) {{
                    Ok(value) => __jet_bootstrap_host_reply({result_runtime_failure}(value, span)),
                     Err(detail) => __jet_bootstrap_host_failure(span, detail),
                 }},
                 {native_error}::Internal(detail) => __jet_bootstrap_host_failure(span, detail),
             }}
         }}
         fn __jet_bootstrap_source_handle_from_eval(value: &{eval}) -> Result<({mir_handle}, i64), String> {{
             let {eval_foreign_handle}(handle, raw, _) = value else {{
                 return Err("checked Source resource operation requires a live native handle".to_string());
             }};
             let handle = __jet_bootstrap_source_u64(&handle.{handle_value}, "Source resource handle")?;
             let raw = raw.to_string_rep().parse::<i64>()
                 .map_err(|_| "Source resource capability is outside i64".to_string())?;
             Ok(({mir_handle}(handle), raw))
         }}
         fn __jet_bootstrap_native_source_resource_call(
             machine: &{eval_machine},
             row: &{row},
            args: &[{host_argument}],
             result_shape: Option<&{shape}>,
             arg_shapes: &[{shape}],
             span: {span},
         ) -> Option<Result<{host_result}, ::jet_foundation::Outcome::JetAbsent>> {{
             let module = row.{row_module}.as_str();
             let member = row.{row_member}.as_str();
             let symbol = __jet_bootstrap_symbol_name(&row.{row_symbol});
             if module == "core.prelude" && member == "loop_iter_init" {{
                 if symbol != "jet_loop_iter_init" || args.len() != 5 {{
                     return Some(Ok(__jet_bootstrap_host_failure(span, "non-canonical native loop initializer row".to_string())));
                 }}
                let (source_handle, source_raw) = match __jet_bootstrap_source_handle_from_eval(&args[0].{host_argument_value}) {{
                     Ok(value) => value,
                     Err(detail) => return Some(Ok(__jet_bootstrap_host_failure(span, detail))),
                 }};
                 let Some(source_shape) = arg_shapes.first() else {{
                     return Some(Ok(__jet_bootstrap_host_failure(span, "native loop initializer has no checked source type shape".to_string())));
                 }};
                let (typed_handle, _, _, _, core_kind) = match __jet_bootstrap_core_owner_from_shape(machine, source_shape) {{
                    Ok(value) => value,
                    Err(detail) => return Some(Ok(__jet_bootstrap_host_failure(span, detail))),
                }};
                if typed_handle != source_handle {{
                    return Some(Ok(__jet_bootstrap_host_failure(span, "native loop source handle disagrees with its checked Core owner shape".to_string())));
                }}
                 let step_args = match __jet_bootstrap_eval_to_host(&args[1].{host_argument_value}) {{
                     Ok(value) => value,
                     Err(detail) => return Some(Ok(__jet_bootstrap_host_failure(span, detail))),
                 }};
                 let (step_value, has_step) = match step_args {{
                     ::jet_foundation::MIR::MirRuntimeValue::Unit => (1, false),
                     ::jet_foundation::MIR::MirRuntimeValue::Int(value) => (value, true),
                     _ => return Some(Ok(__jet_bootstrap_host_failure(span, "native loop step has a non-Int carrier".to_string()))),
                 }};
                 let bool_arg = |index: usize, label: &str| -> Result<bool, String> {{
                     match __jet_bootstrap_eval_to_host(&args[index].{host_argument_value})? {{
                         ::jet_foundation::MIR::MirRuntimeValue::Bool(value) => Ok(value),
                         _ => Err(format!("native loop {{label}} has a non-Bool carrier")),
                     }}
                 }};
                 let by_value = match bool_arg(3, "ownership mode") {{
                     Ok(value) => value,
                     Err(detail) => return Some(Ok(__jet_bootstrap_host_failure(span, detail))),
                 }};
                 let source_wire = match __jet_bootstrap_eval_to_host(&args[4].{host_argument_value}) {{
                     Ok(::jet_foundation::MIR::MirRuntimeValue::String(value)) => value,
                     Ok(_) => return Some(Ok(__jet_bootstrap_host_failure(span, "native loop source kind is not a String".to_string()))),
                     Err(detail) => return Some(Ok(__jet_bootstrap_host_failure(span, detail))),
                 }};
                 let source_kind = match ::jet_foundation::MIR::MirLoopSourceKind::from_wire(&source_wire) {{
                     Some(value) => value,
                     None => return Some(Ok(__jet_bootstrap_host_failure(span, "invalid checked native loop source kind".to_string()))),
                 }};
                 let key = {resource_key} {{
                     handle: source_handle,
                     raw: source_raw,
                     source_kind,
                 }};
                let loop_kind = match ::jet_jit::SourceResources::SourceResourceKind::from_loop_source(&source_kind) {{
                    Ok(value) => value,
                    Err(detail) => return Some(Ok(__jet_bootstrap_host_failure(span, detail))),
                }};
                if core_kind != loop_kind {{
                    return Some(Ok(__jet_bootstrap_host_failure(span, "native loop source kind disagrees with its checked Core owner registration and type".to_string())));
                }}
                 let Some(arena) = ::jet_jit::SourceResources::active_source_resource_arena() else {{
                     return Some(Ok(__jet_bootstrap_host_failure(span, "native loop resource has no active Source arena".to_string())));
                 }};
                let cursor = arena.init_cursor(&key, step_value, has_step, by_value);
                let mut transfers = Vec::<{host_transfer}>::new();
                if let Some(consumed) = cursor.consumed.as_ref() {{
                    match __jet_bootstrap_resource_transfer_from_eval(&args[0].{host_argument_value}, consumed) {{
                        Ok(transfer) => transfers.push(transfer),
                        Err(detail) => return Some(Ok(__jet_bootstrap_host_failure_with_transfers(span, detail, transfers))),
                    }}
                }}
                return Some(Ok(match cursor.result {{
                    Ok((capability, _)) => match __jet_bootstrap_resource_host_handle(&capability, {host_owner_cursor}) {{
                        Ok(value) => __jet_bootstrap_host_reply_with_transfers({result_value}(value), transfers),
                        Err(detail) => __jet_bootstrap_host_failure_with_transfers(span, detail, transfers),
                    }},
                    Err(error) => __jet_bootstrap_cursor_error_with_transfers(error, span, transfers),
                }}));
             }}
             if module == "core.prelude" && (member == "loop_iter_has_next" || member == "loop_iter_value" || member == "loop_iter_advance") {{
                 let expected_symbol = match member {{
                     "loop_iter_has_next" => "jet_loop_iter_has_next",
                     "loop_iter_value" => "jet_loop_iter_value",
                     _ => "jet_loop_iter_advance",
                 }};
                 if symbol != expected_symbol || args.len() != 1 {{
                     return Some(Ok(__jet_bootstrap_host_failure(span, "non-canonical native loop cursor operation row".to_string())));
                 }}
                 let (handle, raw) = match __jet_bootstrap_source_handle_from_eval(&args[0].{host_argument_value}) {{
                     Ok(value) => value,
                     Err(detail) => return Some(Ok(__jet_bootstrap_host_failure(span, detail))),
                 }};
                 let Some(cursor_shape) = arg_shapes.first() else {{
                     return Some(Ok(__jet_bootstrap_host_failure(span, "native loop cursor operation has no checked receiver type shape".to_string())));
                 }};
                 let cursor_owner = match __jet_bootstrap_shape_owner(cursor_shape) {{
                     Ok(value) => value,
                     Err(detail) => return Some(Ok(__jet_bootstrap_host_failure(span, detail))),
                 }};
                 if !matches!(cursor_owner, {host_owner_cursor}) {{
                     return Some(Ok(__jet_bootstrap_host_failure(span, "native loop cursor receiver shape is not a Cursor owner".to_string())));
                 }}
                 let Some(arena) = ::jet_jit::SourceResources::active_source_resource_arena() else {{
                     return Some(Ok(__jet_bootstrap_host_failure(span, "native loop cursor has no active Source arena".to_string())));
                 }};
                 let cursor = match arena.lookup_cursor(handle, raw) {{
                     Ok(value) => value,
                     Err(detail) => return Some(Ok(__jet_bootstrap_host_failure(span, detail))),
                 }};
                 let value = match member {{
                     "loop_iter_has_next" => cursor.has_next().map(|value| {host_data}({ct_bool}(value))),
                     "loop_iter_value" => cursor.value().and_then(|value| {{
                         __jet_bootstrap_host_value_from_runtime(&value)
                             .map_err(|detail| {native_error}::internal(detail))
                     }}),
                     _ => cursor.advance().map(|_| {host_data}({ct_unit})),
                 }};
                 return Some(Ok(match value {{
                    Ok(value) => __jet_bootstrap_host_reply({result_value}(value)),
                     Err(error) => __jet_bootstrap_cursor_error(error, span),
                 }}));
             }}
             if module == "core.files" && (member == "open" || member == "create" || member == "append") {{
                 let expected_symbol = match member {{
                     "open" => "jet_std_files_open",
                     "create" => "jet_std_files_create",
                     _ => "jet_std_files_append",
                 }};
                 if symbol != expected_symbol || args.len() != 1 {{
                     return Some(Ok(__jet_bootstrap_host_failure(span, "non-canonical Core file resource factory row".to_string())));
                 }}
                 let Some(result_shape) = result_shape else {{
                     return Some(Ok(__jet_bootstrap_host_failure(span, "Core file resource factory has no checked result type shape".to_string())));
                 }};
                let (handle, owner, _, _, _) = match __jet_bootstrap_core_owner_from_shape(machine, result_shape) {{
                    Ok(value) => value,
                    Err(detail) => return Some(Ok(__jet_bootstrap_host_failure(span, detail))),
                }};
                 let path = match __jet_bootstrap_eval_to_host(&args[0].{host_argument_value}) {{
                     Ok(::jet_foundation::MIR::MirRuntimeValue::String(value)) => value,
                     Ok(_) => return Some(Ok(__jet_bootstrap_host_failure(span, "Core file resource path is not a String".to_string()))),
                     Err(detail) => return Some(Ok(__jet_bootstrap_host_failure(span, detail))),
                 }};
                 let Some(arena) = ::jet_jit::SourceResources::active_source_resource_arena() else {{
                     return Some(Ok(__jet_bootstrap_host_failure(span, "Core file resource factory has no active Source arena".to_string())));
                 }};
                 let result = match member {{
                     "open" => arena.open_file(handle, &path),
                     "create" => arena.open_file_writer(handle, &path),
                     _ => arena.append_file_writer(handle, &path),
                 }};
                 return Some(Ok(__jet_bootstrap_resource_factory_result(result, owner, span)));
             }}
             if module == "core.term" && member == "stdin" {{
                 if symbol != "jet_std_io_stdin" || !args.is_empty() {{
                     return Some(Ok(__jet_bootstrap_host_failure(span, "non-canonical Core stdin resource factory row".to_string())));
                 }}
                 let Some(result_shape) = result_shape else {{
                     return Some(Ok(__jet_bootstrap_host_failure(span, "Core stdin resource factory has no checked result type shape".to_string())));
                 }};
                let (handle, owner, _, _, _) = match __jet_bootstrap_core_owner_from_shape(machine, result_shape) {{
                    Ok(value) => value,
                    Err(detail) => return Some(Ok(__jet_bootstrap_host_failure(span, detail))),
                }};
                 let Some(arena) = ::jet_jit::SourceResources::active_source_resource_arena() else {{
                     return Some(Ok(__jet_bootstrap_host_failure(span, "Core stdin resource factory has no active Source arena".to_string())));
                 }};
                 let result = arena.register_stdin(handle).map_err({native_error}::internal);
                 return Some(Ok(__jet_bootstrap_resource_factory_result(result, owner, span)));
             }}
             None
         }}
         fn __jet_bootstrap_native_handle_call(
             operation: String,
             handle: {source_handle},
             raw: jet_foundation::Numeric::JetInt,
             span: {span},
         ) -> ::std::result::Result<{host_result}, ::jet_foundation::Outcome::JetAbsent> {{
             if operation != "resource_release" {{
                 return Err(::jet_foundation::Outcome::JetAbsent);
             }}
             let handle = match __jet_bootstrap_source_u64(&handle.{handle_value}, "Source resource handle") {{
                 Ok(value) => {mir_handle}(value),
                 Err(detail) => return Ok(__jet_bootstrap_host_failure(span, detail)),
             }};
             let raw = match raw.to_string_rep().parse::<i64>() {{
                 Ok(value) => value,
                 Err(_) => return Ok(__jet_bootstrap_host_failure(span, "Source resource capability is outside i64".to_string())),
             }};
             let Some(arena) = ::jet_jit::SourceResources::active_source_resource_arena() else {{
                 return Ok(__jet_bootstrap_host_failure(span, "Source resource release has no active arena".to_string()));
             }};
             match arena.release_resource(handle, raw) {{
                Ok(()) => Ok(__jet_bootstrap_host_reply({result_value}({host_data}({ct_unit}))),
                 Err(detail) => Ok(__jet_bootstrap_host_failure(span, detail)),
             }}
         }}"#,
        row = row,
        eval = eval,
        host_argument = host_argument,
        host_argument_value = host_argument_value,
        host_result = host_result,
        host_value = host_value,
        host_owner = host_owner,
        source_handle = source_handle,
        handle_value = handle_value,
        row_module = row_module,
        row_member = row_member,
        row_symbol = row_symbol,
        host_data = host_data,
        host_handle = host_handle,
        host_failed = host_failed,
        host_owner_cursor = host_owner_cursor,
        host_owner_core = host_owner_core,
        host_core_owner_fact = host_core_owner_fact,
        host_core_owner_ty = host_core_owner_ty,
        eval_machine = eval_machine,
        machine_program = machine_program,
        program_core_owners = program_core_owners,
        core_owner_id = core_owner_id,
        type_id_value = type_id_value,
        result_value = result_value,
        result_runtime_failure = result_runtime_failure,
        eval_foreign_handle = eval_foreign_handle,
        ct_bool = ct_bool,
        ct_unit = ct_unit,
        shape = shape,
        shape_root = shape_root,
        shape_nodes = shape_nodes,
        node_option = node_option,
        node_result = node_result,
        node_handle = node_handle,
        cap_type = cap_type,
        mir_type_kind = mir_type_kind,
        mir_handle = mir_handle,
        native_error = native_error,
        resource_key = resource_key,
        host_outcome = host_outcome,
        host_transfer = host_transfer,
        host_result_outcome = host_result_outcome,
        host_result_transfers = host_result_transfers,
        transfer_argument = transfer_argument,
        transfer_path = transfer_path,
        transfer_token = transfer_token,
        transfer_handle = transfer_handle,
        transfer_raw = transfer_raw,
        span = symbols.type_symbol("Span")?,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    Ok(())
}

fn emit_bootstrap_callback(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let row = symbols.type_symbol("MirPreludeCall")?;
    let symbol = symbols.type_symbol("MirSymbol")?;
    let shape = symbols.type_symbol("JetEvalHostTypeShape")?;
    let eval = symbols.type_symbol("JetEvalRuntimeValue")?;
    let host_argument = symbols.type_symbol("JetEvalHostArgument")?;
    let host_argument_value = symbols.field_symbol("JetEvalHostArgument", "value")?;
    let eval_machine = symbols.type_symbol("JetEvalMachine")?;
    let host_result = symbols.type_symbol("JetEvalHostResult")?;
    let host_outcome = symbols.type_symbol("JetEvalHostOutcome")?;
    let span = symbols.type_symbol("Span")?;
    let row_module = symbols.field_symbol("MirPreludeCall", "module_name")?;
    let row_member = symbols.field_symbol("MirPreludeCall", "member")?;
    let row_family = symbols.field_symbol("MirPreludeCall", "family")?;
    let row_abi = symbols.field_symbol("MirPreludeCall", "abi")?;
    let row_fallibility = symbols.field_symbol("MirPreludeCall", "fallibility")?;
    let row_effect_name = symbols.field_symbol("MirPreludeCall", "effect_name")?;
    let sig_arity = symbols.field_symbol("MirCallSignature", "arity")?;
    let sig_max_arity = symbols.field_symbol("MirCallSignature", "max_arity")?;
    let sig_borrow_mask = symbols.field_symbol("MirCallSignature", "borrow_mask")?;
    let symbol_prelude = symbols.variant_path("MirSymbol", "Prelude")?;
    let symbol_runtime = symbols.variant_path("MirSymbol", "Runtime")?;
    let result_value = symbols.variant_path("JetEvalHostOutcome", "Value")?;
    let result_effect = symbols.variant_path("JetEvalHostOutcome", "Effect")?;
    let result_control = symbols.variant_path("JetEvalHostOutcome", "Control")?;
    let result_failure = symbols.variant_path("JetEvalHostOutcome", "Failure")?;
    let internal_problem = symbols.type_symbol("JetEvalInternalProblem")?;
    let internal_problem_span = symbols.field_symbol("JetEvalInternalProblem", "span")?;
    let internal_problem_message = symbols.field_symbol("JetEvalInternalProblem", "message")?;
    let result_internal_problem = symbols.variant_path("JetEvalHostOutcome", "InternalProblem")?;
    let host_result_outcome = symbols.field_symbol("JetEvalHostResult", "outcome")?;
    let host_result_transfers = symbols.field_symbol("JetEvalHostResult", "transfers")?;
    let error_source = symbols.variant_path("JetEvalErrorKind", "Source")?;

    writeln!(
        out,
        "fn __jet_bootstrap_symbol_name(value: &{symbol}) -> &str {{
             match value {{
                 {symbol_prelude}(name) | {symbol_runtime}(name) => name.as_str(),
             }}
         }}
        fn __jet_bootstrap_host_failure_outcome(span: {span}, detail: String) -> {host_outcome} {{
            {result_internal_problem}({internal_problem} {{
                {internal_problem_span}: Ok(span),
                {internal_problem_message}: detail,
            }})
        }}
        fn __jet_bootstrap_host_failure(span: {span}, detail: String) -> {host_result} {{
            __jet_bootstrap_host_reply(__jet_bootstrap_host_failure_outcome(span, detail))
        }}
         fn __jet_bootstrap_native_host_call(
             machine: &{eval_machine},
             row: {row},
             args: Vec<{host_argument}>,
             result_shape: ::std::result::Result<{shape}, ::jet_foundation::Outcome::JetAbsent>,
             arg_shapes: Vec<{shape}>,
             span: {span},
         ) -> ::std::result::Result<{host_result}, ::jet_foundation::Outcome::JetAbsent> {{
             let result_shape = result_shape.as_ref().ok();
             if let Some(outcome) = __jet_bootstrap_native_source_resource_call(machine, &row, &args, result_shape, &arg_shapes, span) {{
                 return outcome;
             }}
             let native_args = match args.iter().map(|value| __jet_bootstrap_eval_to_host(&value.{host_argument_value})).collect::<Result<Vec<_>, String>>() {{
                 Ok(args) => args,
                 Err(detail) => return Ok(__jet_bootstrap_host_failure(span, detail)),
             }};
             let native_result_ty = match result_shape.map(__jet_bootstrap_type_from_shape).transpose() {{
                 Ok(value) => value,
                 Err(detail) => return Ok(__jet_bootstrap_host_failure(span, detail)),
             }};
             let arity = match ({row}).{row_signature}.{sig_arity}.to_string_rep().parse::<usize>() {{
                 Ok(value) => value,
                 Err(_) => return Ok(__jet_bootstrap_host_failure(span, \"Prelude arity is not a usize\".to_string())),
             }};
             let max_arity = match ({row}).{row_signature}.{sig_max_arity}.to_string_rep().parse::<usize>() {{
                 Ok(value) => value,
                 Err(_) => return Ok(__jet_bootstrap_host_failure(span, \"Prelude maximum arity is not a usize\".to_string())),
             }};
             let native_span = match __jet_bootstrap_span_to_host(&span) {{
                 Ok(value) => value,
                 Err(detail) => return Ok(__jet_bootstrap_host_failure(span, detail)),
             }};
             let outcome = crate::Codegen::NativePreludeBridge::ambient_call_route(
                 ({row}).{row_module}.as_str(),
                 ({row}).{row_member}.as_str(),
                 __jet_bootstrap_symbol_name(&({row}).{row_symbol}),
                 arity,
                ({row}).{row_family}.to_string_rep().as_str(),
                ({row}).{row_abi}.to_string_rep().as_str(),
                ({row}).{row_fallibility}.to_string_rep().as_str(),
                ({row}).{row_effect_name}.as_ref().ok().map(|effect| effect.as_str()),
                 max_arity,
                 &({row}).{row_signature}.{sig_borrow_mask},
                 native_args,
                 native_result_ty,
                 native_span,
             );
             match outcome {{
                 None => Err(::jet_foundation::Outcome::JetAbsent),
                Some(Ok(crate::Comptime::AmbientMirPreludeResult::Value(value))) => match __jet_bootstrap_host_value_from_runtime(&value) {{
                    Ok(value) => Ok(__jet_bootstrap_host_reply({result_value}(value))),
                    Err(detail) => Ok(__jet_bootstrap_host_failure(span, detail)),
                }},
                Some(Ok(crate::Comptime::AmbientMirPreludeResult::Effect {{ value, stdout, stderr }})) => match __jet_bootstrap_host_value_from_runtime(&value) {{
                    Ok(value) => Ok(__jet_bootstrap_host_reply({result_effect}(value, stdout, stderr))),
                    Err(detail) => Ok(__jet_bootstrap_host_failure(span, detail)),
                }},
                Some(Ok(crate::Comptime::AmbientMirPreludeResult::Control {{ value, stdout, stderr, exit_code }})) => match __jet_bootstrap_host_value_from_runtime(&value) {{
                    Ok(value) => Ok(__jet_bootstrap_host_reply({result_control}(value, stdout, stderr, jet_foundation::Numeric::JetInt::from_i64(i64::from(exit_code))))),
                    Err(detail) => Ok(__jet_bootstrap_host_failure(span, detail)),
                }},
                Some(Err(error)) => match __jet_bootstrap_diagnostic_from_host(&error) {{
                    Ok(diagnostic) => Ok(__jet_bootstrap_host_reply({result_failure}({error_source}, diagnostic))),
                    Err(detail) => Ok(__jet_bootstrap_host_failure(span, detail)),
                }},
             }}
         }}"
        ,
        host_outcome = host_outcome,
        value = "value",
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    Ok(())
}
fn emit_bootstrap_foreign_callback(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let foreign = symbols.type_symbol("MirForeign")?;
    let param = symbols.type_symbol("MirParam")?;
    let ownership = symbols.type_symbol("MirOwnership")?;
    let access = symbols.type_symbol("MirAccess")?;
    let effects = symbols.type_symbol("MirEffectFacts")?;
    let named_span = symbols.type_symbol("MirNamedSpan")?;
    let applicability = symbols.type_symbol("MirTargetApplicability")?;
    let eval = symbols.type_symbol("JetEvalRuntimeValue")?;
    let host_argument = symbols.type_symbol("JetEvalHostArgument")?;
    let host_argument_value = symbols.field_symbol("JetEvalHostArgument", "value")?;
    let host_result = symbols.type_symbol("JetEvalHostResult")?;
    let writeback = symbols.type_symbol("JetEvalForeignWriteback")?;
    let span = symbols.type_symbol("Span")?;
    let writeback_argument = symbols.field_symbol("JetEvalForeignWriteback", "argument")?;
    let writeback_value = symbols.field_symbol("JetEvalForeignWriteback", "value")?;
    let param_index = symbols.field_symbol("MirParam", "index")?;
    let param_name = symbols.field_symbol("MirParam", "name")?;
    let param_span = symbols.field_symbol("MirParam", "span")?;
    let param_ty = symbols.field_symbol("MirParam", "ty")?;
    let param_access = symbols.field_symbol("MirParam", "access")?;
    let param_ownership = symbols.field_symbol("MirParam", "ownership")?;
    let param_public_label = symbols.field_symbol("MirParam", "public_label")?;
    let param_variadic = symbols.field_symbol("MirParam", "variadic")?;
    let param_default_present = symbols.field_symbol("MirParam", "default_present")?;
    let ownership_mode = symbols.field_symbol("MirOwnership", "mode")?;
    let ownership_drop = symbols.field_symbol("MirOwnership", "drop")?;
    let ownership_moved = symbols.field_symbol("MirOwnership", "moved")?;
    let ownership_last_use = symbols.field_symbol("MirOwnership", "last_use")?;
    let ownership_gc_root = symbols.field_symbol("MirOwnership", "gc_root")?;
    let effects_direct = symbols.field_symbol("MirEffectFacts", "direct")?;
    let effects_solved = symbols.field_symbol("MirEffectFacts", "solved")?;
    let effects_call_edges = symbols.field_symbol("MirEffectFacts", "call_edges")?;
    let effects_maximal = symbols.field_symbol("MirEffectFacts", "maximal")?;
    let effects_direct_spans = symbols.field_symbol("MirEffectFacts", "direct_spans")?;
    let named_span_name = symbols.field_symbol("MirNamedSpan", "name")?;
    let named_span_span = symbols.field_symbol("MirNamedSpan", "span")?;
    let target_rust_aot = symbols.field_symbol("MirTargetApplicability", "rust_aot")?;
    let target_cranelift = symbols.field_symbol("MirTargetApplicability", "cranelift")?;
    let target_interpreter = symbols.field_symbol("MirTargetApplicability", "interpreter")?;
    let target_web = symbols.field_symbol("MirTargetApplicability", "web")?;
    let foreign_id = symbols.field_symbol("MirForeign", "id")?;
    let foreign_module_id = symbols.field_symbol("MirForeign", "module_id")?;
    let foreign_key = symbols.field_symbol("MirForeign", "key")?;
    let foreign_module_name = symbols.field_symbol("MirForeign", "module_name")?;
    let foreign_name = symbols.field_symbol("MirForeign", "name")?;
    let foreign_span = symbols.field_symbol("MirForeign", "span")?;
    let foreign_symbol = symbols.field_symbol("MirForeign", "symbol")?;
    let foreign_path = symbols.field_symbol("MirForeign", "path")?;
    let foreign_params = symbols.field_symbol("MirForeign", "params")?;
    let foreign_bridge_eligible = symbols.field_symbol("MirForeign", "bridge_eligible")?;
    let foreign_raw_scalar_abi = symbols.field_symbol("MirForeign", "raw_scalar_abi")?;
    let foreign_return_type = symbols.field_symbol("MirForeign", "return_type")?;
    let foreign_abi = symbols.field_symbol("MirForeign", "foreign_abi")?;
    let foreign_language = symbols.field_symbol("MirForeign", "foreign_language")?;
    let foreign_target_applicability = symbols.field_symbol("MirForeign", "target_applicability")?;
    let foreign_effects = symbols.field_symbol("MirForeign", "effects")?;
    let foreign_callback_transport = symbols.field_symbol("MirForeign", "callback_transport")?;
    let foreign_callback_plan_digest = symbols.field_symbol("MirForeign", "callback_plan_digest")?;
    let foreign_callback_identity = symbols.field_symbol("MirForeign", "callback_identity")?;
    let foreign_link = symbols.field_symbol("MirForeign", "link")?;
    let foreign_callback = symbols.field_symbol("MirForeign", "callback")?;
    let foreign_handle = symbols.field_symbol("MirForeign", "handle")?;
    let foreign_close_function = symbols.field_symbol("MirForeign", "close_function")?;
    let foreign_close_foreign = symbols.field_symbol("MirForeign", "close_foreign")?;
    let foreign_undo_function = symbols.field_symbol("MirForeign", "undo_function")?;
    let foreign_id_value = symbols.field_symbol("MirForeignId", "value")?;
    let module_id_value = symbols.field_symbol("MirModuleId", "value")?;
    let link_unit_id_value = symbols.field_symbol("MirLinkUnitId", "value")?;
    let callback_id_value = symbols.field_symbol("MirCallbackId", "value")?;
    let handle_id_value = symbols.field_symbol("MirHandleId", "value")?;
    let function_id_value = symbols.field_symbol("MirFunctionId", "value")?;
    let foreign_abi_c = symbols.variant_path("MirForeignAbi", "C")?;
    let foreign_abi_c_unwind = symbols.variant_path("MirForeignAbi", "CUnwind")?;
    let foreign_abi_system = symbols.variant_path("MirForeignAbi", "System")?;
    let foreign_abi_stdcall = symbols.variant_path("MirForeignAbi", "Stdcall")?;
    let foreign_abi_fastcall = symbols.variant_path("MirForeignAbi", "Fastcall")?;
    let foreign_abi_vectorcall = symbols.variant_path("MirForeignAbi", "Vectorcall")?;
    let foreign_abi_rust = symbols.variant_path("MirForeignAbi", "Rust")?;
    let foreign_abi_platform = symbols.variant_path("MirForeignAbi", "Platform")?;
    let foreign_language_c = symbols.variant_path("MirForeignLanguage", "C")?;
    let foreign_language_cpp = symbols.variant_path("MirForeignLanguage", "Cpp")?;
    let foreign_language_rust = symbols.variant_path("MirForeignLanguage", "Rust")?;
    let foreign_language_assembly = symbols.variant_path("MirForeignLanguage", "Assembly")?;
    let ownership_mode_copy = symbols.variant_path("MirOwnershipMode", "Copy")?;
    let ownership_mode_owned = symbols.variant_path("MirOwnershipMode", "Owned")?;
    let ownership_mode_shared = symbols.variant_path("MirOwnershipMode", "Shared")?;
    let ownership_mode_read_borrow = symbols.variant_path("MirOwnershipMode", "ReadBorrow")?;
    let ownership_mode_write_borrow = symbols.variant_path("MirOwnershipMode", "WriteBorrow")?;
    let ownership_mode_move = symbols.variant_path("MirOwnershipMode", "Move")?;
    let drop_no_drop = symbols.variant_path("MirDropKind", "NoDrop")?;
    let drop_value = symbols.variant_path("MirDropKind", "Value")?;
    let drop_shared = symbols.variant_path("MirDropKind", "Shared")?;
    let drop_view = symbols.variant_path("MirDropKind", "View")?;
    let drop_foreign_handle = symbols.variant_path("MirDropKind", "ForeignHandle")?;
    let access_read = symbols.variant_path("MirAccess", "Read")?;
    let access_write = symbols.variant_path("MirAccess", "Write")?;
    let access_move = symbols.variant_path("MirAccess", "Move")?;
    let result_foreign = symbols.variant_path("JetEvalHostOutcome", "Foreign")?;
    let result_failure = symbols.variant_path("JetEvalHostOutcome", "Failure")?;
    let error_source = symbols.variant_path("JetEvalErrorKind", "Source")?;

    writeln!(
        out,
        "fn __jet_bootstrap_source_u64(value: &jet_foundation::Numeric::JetInt, label: &str) -> Result<u64, String> {{
             value.to_string_rep().parse::<u64>().map_err(|_| format!(\"{{label}} is not a nonnegative integer\"))
         }}
         fn __jet_bootstrap_source_index(value: &jet_foundation::Numeric::JetInt, label: &str) -> Result<usize, String> {{
             value.to_string_rep().parse::<usize>().map_err(|_| format!(\"{{label}} is not a usize\"))
         }}
         fn __jet_bootstrap_mir_access_to_host(value: &{access}) -> Result<::jet_foundation::MIR::MirAccess, String> {{
             match value {{
                 {access_read} => Ok(::jet_foundation::MIR::MirAccess::Read),
                 {access_write} => Ok(::jet_foundation::MIR::MirAccess::Write),
                 {access_move} => Ok(::jet_foundation::MIR::MirAccess::Move),
             }}
         }}
         fn __jet_bootstrap_mir_ownership_to_host(value: &{ownership}) -> Result<::jet_foundation::MIR::MirOwnership, String> {{
             let mode = match &value.{ownership_mode} {{
                 {ownership_mode_copy} => ::jet_foundation::MIR::MirOwnershipMode::Copy,
                 {ownership_mode_owned} => ::jet_foundation::MIR::MirOwnershipMode::Owned,
                 {ownership_mode_shared} => ::jet_foundation::MIR::MirOwnershipMode::Shared,
                 {ownership_mode_read_borrow} => ::jet_foundation::MIR::MirOwnershipMode::ReadBorrow,
                 {ownership_mode_write_borrow} => ::jet_foundation::MIR::MirOwnershipMode::WriteBorrow,
                 {ownership_mode_move} => ::jet_foundation::MIR::MirOwnershipMode::Move,
             }};
             let drop = match &value.{ownership_drop} {{
                 {drop_no_drop} => ::jet_foundation::MIR::MirDropKind::None,
                 {drop_value} => ::jet_foundation::MIR::MirDropKind::Value,
                 {drop_shared} => ::jet_foundation::MIR::MirDropKind::Shared,
                 {drop_view} => ::jet_foundation::MIR::MirDropKind::View,
                 {drop_foreign_handle} => ::jet_foundation::MIR::MirDropKind::ForeignHandle,
             }};
             Ok(::jet_foundation::MIR::MirOwnership {{
                 mode,
                 drop,
                 moved: value.{ownership_moved},
                 last_use: value.{ownership_last_use},
                 gc_root: value.{ownership_gc_root},
             }})
         }}
         fn __jet_bootstrap_mir_param_to_host(value: {param}) -> Result<::jet_foundation::MIR::MirParam, String> {{
             Ok(::jet_foundation::MIR::MirParam {{
                 index: __jet_bootstrap_source_index(&value.{param_index}, \"foreign parameter index\")?,
                 name: value.{param_name},
                 span: __jet_bootstrap_span_to_host(&value.{param_span})?,
                 ty: __jet_bootstrap_type_to_host(&value.{param_ty})?,
                 access: __jet_bootstrap_mir_access_to_host(&value.{param_access})?,
                 ownership: __jet_bootstrap_mir_ownership_to_host(&value.{param_ownership})?,
                 public_label: value.{param_public_label},
                 variadic: value.{param_variadic},
                 default_present: value.{param_default_present},
             }})
         }}
         fn __jet_bootstrap_mir_effects_to_host(value: {effects}) -> Result<::jet_foundation::MIR::MirEffectFacts, String> {{
             let mut direct_spans = ::std::collections::BTreeMap::new();
             for fact in value.{effects_direct_spans} {{
                 if direct_spans.insert(fact.{named_span_name}, __jet_bootstrap_span_to_host(&fact.{named_span_span})?).is_some() {{
                     return Err(\"foreign MIR effect facts contain duplicate direct span names\".to_string());
                 }}
             }}
             Ok(::jet_foundation::MIR::MirEffectFacts {{
                 direct: value.{effects_direct}.into_iter().collect(),
                 solved: value.{effects_solved}.into_iter().collect(),
                 call_edges: value.{effects_call_edges}.into_iter().collect(),
                 maximal: value.{effects_maximal},
                 direct_spans,
             }})
         }}
         fn __jet_bootstrap_foreign_to_host(value: {foreign}) -> Result<::jet_foundation::MIR::MirForeign, String> {{
             let foreign_abi = match value.{foreign_abi} {{
                 {foreign_abi_c} => ::jet_foundation::MIR::MirForeignAbi::C,
                 {foreign_abi_c_unwind} => ::jet_foundation::MIR::MirForeignAbi::CUnwind,
                 {foreign_abi_system} => ::jet_foundation::MIR::MirForeignAbi::System,
                 {foreign_abi_stdcall} => ::jet_foundation::MIR::MirForeignAbi::Stdcall,
                 {foreign_abi_fastcall} => ::jet_foundation::MIR::MirForeignAbi::Fastcall,
                 {foreign_abi_vectorcall} => ::jet_foundation::MIR::MirForeignAbi::Vectorcall,
                 {foreign_abi_rust} => ::jet_foundation::MIR::MirForeignAbi::Rust,
                 {foreign_abi_platform}(name) => ::jet_foundation::MIR::MirForeignAbi::Platform(name),
             }};
             let foreign_language = match value.{foreign_language} {{
                 {foreign_language_c} => ::jet_foundation::MIR::MirForeignLanguage::C,
                 {foreign_language_cpp} => ::jet_foundation::MIR::MirForeignLanguage::Cpp,
                 {foreign_language_rust} => ::jet_foundation::MIR::MirForeignLanguage::Rust,
                 {foreign_language_assembly} => ::jet_foundation::MIR::MirForeignLanguage::Assembly,
             }};
             let target = &value.{foreign_target_applicability};
             let target_applicability = ::jet_foundation::MIR::MirTargetApplicability {{
                 rust_aot: target.{target_rust_aot},
                 cranelift: target.{target_cranelift},
                 interpreter: target.{target_interpreter},
                 web: target.{target_web},
             }};
             let return_type = value.{foreign_return_type}.ok().map(|ty| __jet_bootstrap_type_to_host(&ty)).transpose()?;
             let link = value.{foreign_link}.ok().map(|id| __jet_bootstrap_source_u64(&id.{link_unit_id_value}, \"foreign link id\").map(::jet_foundation::MIR::MirLinkUnitId)).transpose()?;
             let callback = value.{foreign_callback}.ok().map(|id| __jet_bootstrap_source_u64(&id.{callback_id_value}, \"foreign callback id\").map(::jet_foundation::MIR::MirCallbackId)).transpose()?;
             let handle = value.{foreign_handle}.ok().map(|id| __jet_bootstrap_source_u64(&id.{handle_id_value}, \"foreign handle id\").map(::jet_foundation::MIR::MirHandleId)).transpose()?;
             let close_function = value.{foreign_close_function}.ok().map(|id| __jet_bootstrap_source_u64(&id.{function_id_value}, \"foreign close function id\").map(::jet_foundation::MIR::MirFunctionId)).transpose()?;
             let close_foreign = value.{foreign_close_foreign}.ok().map(|id| __jet_bootstrap_source_u64(&id.{foreign_id_value}, \"foreign close id\").map(::jet_foundation::MIR::MirForeignId)).transpose()?;
             let undo_function = value.{foreign_undo_function}.ok().map(|id| __jet_bootstrap_source_u64(&id.{function_id_value}, \"foreign undo function id\").map(::jet_foundation::MIR::MirFunctionId)).transpose()?;
             Ok(::jet_foundation::MIR::MirForeign {{
                 id: ::jet_foundation::MIR::MirForeignId(__jet_bootstrap_source_u64(&value.{foreign_id}.{foreign_id_value}, \"foreign id\")?),
                 module_id: ::jet_foundation::MIR::MirModuleId(__jet_bootstrap_source_u64(&value.{foreign_module_id}.{module_id_value}, \"foreign module id\")?),
                 key: value.{foreign_key},
                 module: value.{foreign_module_name},
                 name: value.{foreign_name},
                 span: __jet_bootstrap_span_to_host(&value.{foreign_span})?,
                 symbol: value.{foreign_symbol},
                 path: value.{foreign_path},
                 params: value.{foreign_params}.into_iter().map(__jet_bootstrap_mir_param_to_host).collect::<Result<Vec<_>, String>>()?,
                 bridge_eligible: value.{foreign_bridge_eligible},
                 raw_scalar_abi: value.{foreign_raw_scalar_abi},
                 return_type,
                 foreign_abi,
                 foreign_language,
                 target_applicability,
                 effects: __jet_bootstrap_mir_effects_to_host(value.{foreign_effects})?,
                 callback_transport: value.{foreign_callback_transport}.ok(),
                 callback_plan_digest: value.{foreign_callback_plan_digest}.ok(),
                 callback_identity: value.{foreign_callback_identity}.ok(),
                 link,
                 callback,
                 handle,
                 close_function,
                 close_foreign,
                 undo_function,
             }})
         }}
         fn __jet_bootstrap_native_foreign_call(
             row: {foreign},
             args: Vec<{host_argument}>,
             span: {span},
         ) -> ::std::result::Result<{host_result}, ::jet_foundation::Outcome::JetAbsent> {{
             let native_row = match __jet_bootstrap_foreign_to_host(&row) {{
                 Ok(value) => value,
                 Err(detail) => return Ok(__jet_bootstrap_host_failure(span, detail)),
             }};
             let native_args = match args.iter().map(|value| __jet_bootstrap_eval_to_foreign_host(&value.{host_argument_value})).collect::<Result<Vec<_>, String>>() {{
                 Ok(value) => value,
                 Err(detail) => return Ok(__jet_bootstrap_host_failure(span, detail)),
             }};
             let native_span = match __jet_bootstrap_span_to_host(&span) {{
                 Ok(value) => value,
                 Err(detail) => return Ok(__jet_bootstrap_host_failure(span, detail)),
             }};
             match crate::Comptime::try_ambient_mir_extern_call(&native_row, native_args, native_span) {{
                 None => Err(::jet_foundation::Outcome::JetAbsent),
                 Some(Err(error)) => match __jet_bootstrap_diagnostic_from_host(&error) {{
                    Ok(diagnostic) => Ok(__jet_bootstrap_host_reply({result_failure}({error_source}, diagnostic))),
                     Err(detail) => Ok(__jet_bootstrap_host_failure(span, detail)),
                 }},
                 Some(Ok(result)) => {{
                     let value = match __jet_bootstrap_host_value_from_runtime(&result.value) {{
                         Ok(value) => value,
                         Err(detail) => return Ok(__jet_bootstrap_host_failure(span, detail)),
                     }};
                     let writebacks = match result.writebacks.iter().map(|writeback| {{
                         let argument = i64::try_from(writeback.parameter).map_err(|_| \"foreign writeback index exceeds Jet Int\".to_string())?;
                         Ok({writeback} {{
                             {writeback_argument}: jet_foundation::Numeric::JetInt::from_i64(argument),
                             {writeback_value}: __jet_bootstrap_host_value_from_runtime(&writeback.value)?,
                         }})
                     }}).collect::<Result<Vec<_>, String>>() {{
                         Ok(value) => value,
                         Err(detail) => return Ok(__jet_bootstrap_host_failure(span, detail)),
                     }};
                    Ok(__jet_bootstrap_host_reply({result_foreign}(value, writebacks)))
                 }}
             }}
         }}"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    Ok(())
}

