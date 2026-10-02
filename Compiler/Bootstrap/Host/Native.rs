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
use jet_jit::SourceResources::SourceResourceRetireError;
use jet_foundation::MIR::{
    MirFieldId, MirProgram, MirType, MirTypeDef, MirTypeDefKind, MirTypeId, MirVariantPayload,
};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
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

/// The machine record store behind the self-hosted driver's
/// `JetDriverRecordStore`. The host owns where records live; the compiler
/// names a record only by schema and key. Every failure is a miss: an unknown
/// schema, an unreadable or undecodable record, or a rejected publish.
#[derive(Clone)]
pub(crate) struct BootstrapRecordStore {
    store: jet_store::Store,
}

impl BootstrapRecordStore {
    /// The store configured for this process (`JET_STORE_DIR` or the default
    /// machine store), or `None` when it cannot be opened.
    pub(crate) fn from_env() -> Option<Self> {
        jet_store::Store::from_env().ok().map(|store| Self { store })
    }

    pub(crate) fn record(&self, schema: &str, key: &str) -> Option<Vec<u8>> {
        let kind = jet_store::RecordKind::from_schema(schema)?;
        self.store.record(kind, key).ok().flatten()
    }

    pub(crate) fn put_record(&self, schema: &str, key: &str, bytes: &[u8]) -> bool {
        jet_store::RecordKind::from_schema(schema)
            .is_some_and(|kind| self.store.put_record(kind, key, bytes).is_ok())
    }

    /// `size:mtime_ns:ctime_ns:inode` of a regular file, the same stamp the
    /// Rust driver keeps in its stamp table.
    pub(crate) fn stamp(&self, path: &str) -> Option<String> {
        jet_store::project_state::file_stamp(Path::new(path))
    }
}

/// Failure while binding the private generated callback to the exact carriers
/// emitted for the same MIR artifact. These failures are deliberately raised
/// before mutating the generated source buffer.
#[derive(Debug)]
pub(crate) enum BootstrapHostCodecError {
    MissingType(String),
    MissingField { owner: String, field: String },
    MissingVariant { owner: String, variant: String },
    MissingEntry(String),
    InvalidMetadata(String),
    ResourceRetirement {
        cause: Box<BootstrapHostCodecError>,
        retirement: SourceResourceRetireError,
    },
    /// Every failure one packaging run found: packaging keeps going after a
    /// failed lookup or stage so a single stage-zero run reports them all.
    Multiple(Vec<BootstrapHostCodecError>),
}

impl BootstrapHostCodecError {
    /// One error for a non-empty `errors` list: the error itself when alone.
    pub(crate) fn combine(mut errors: Vec<BootstrapHostCodecError>) -> Self {
        if errors.len() == 1 {
            if let Some(error) = errors.pop() {
                return error;
            }
        }
        Self::Multiple(errors)
    }
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
            Self::ResourceRetirement { cause, retirement } => write!(
                formatter,
                "{cause}; Source resource retirement failed: {}",
                retirement.error
            ),
            Self::Multiple(errors) => {
                write!(formatter, "{} bootstrap packaging errors:", errors.len())?;
                for error in errors {
                    write!(formatter, "\n  - {error}")?;
                }
                Ok(())
            }
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
    // The checked program the bindings were emitted from: the authority for
    // payload shapes and the emitter's boxed (recursive) edges.
    program: &'a MirProgram,
    types: BTreeMap<&'a str, &'a str>,
    // First row per key, matching the linear-scan meaning these replace: the
    // host glue walks the whole compiler type graph once per root, so each
    // field and variant lookup must not rescan every row.
    definitions: HashMap<&'a str, MirTypeId>,
    fields: HashMap<(MirTypeId, &'a str), &'a BootstrapFieldBinding>,
    // Enum variants with named payloads may repeat a field name across
    // variants, so their payload fields resolve by checked field identity.
    field_ids: HashMap<(MirTypeId, MirFieldId), &'a BootstrapFieldBinding>,
    variants: HashMap<(MirTypeId, &'a str), &'a str>,
    // Failed symbol lookups. A miss yields `MISSING_SYMBOL` so packaging keeps
    // going and `finish` reports every miss of one run, not just the first.
    missed: RefCell<Vec<BootstrapHostCodecError>>,
}

/// Placeholder spelling returned for a failed lookup; the packaging result is
/// an error whenever one was handed out, so it never reaches emitted Rust.
const MISSING_SYMBOL: &str = "__jet_bootstrap_missing_symbol";

impl<'a> BootstrapCodecSymbols<'a> {
    pub(crate) fn new(
        metadata: &'a BootstrapBindingDescriptor,
        program: &'a MirProgram,
    ) -> Result<Self, BootstrapHostCodecError> {
        let mut types = BTreeMap::new();
        let mut definitions = HashMap::new();
        for row in &metadata.types {
            types.entry(row.source_name.as_str()).or_insert(row.symbol.as_str());
            definitions.entry(row.source_name.as_str()).or_insert(row.ty);
        }
        let mut fields = HashMap::new();
        let mut field_ids = HashMap::new();
        for row in &metadata.fields {
            fields.entry((row.owner, row.source_name.as_str())).or_insert(row);
            field_ids.entry((row.owner, row.field)).or_insert(row);
        }
        let mut variants = HashMap::new();
        for row in &metadata.variants {
            variants
                .entry((row.owner, row.source_name.as_str()))
                .or_insert(row.symbol.as_str());
        }
        Ok(Self {
            metadata,
            program,
            types,
            definitions,
            fields,
            field_ids,
            variants,
            missed: RefCell::new(Vec::new()),
        })
    }

    /// Keep a packaging failure and go on, so `finish` reports it with the rest.
    pub(crate) fn record(&self, error: BootstrapHostCodecError) {
        self.missed.borrow_mut().push(error);
    }

    fn symbol_or_record<'s>(&self, symbol: Result<&'s str, BootstrapHostCodecError>) -> &'s str {
        symbol.unwrap_or_else(|error| {
            self.record(error);
            MISSING_SYMBOL
        })
    }

    /// `result`, unless lookups were recorded: then every recorded failure
    /// plus `result`'s own error, in the order they happened.
    pub(crate) fn finish<T>(
        &self,
        result: Result<T, BootstrapHostCodecError>,
    ) -> Result<T, BootstrapHostCodecError> {
        let mut missed = std::mem::take(&mut *self.missed.borrow_mut());
        match result {
            Ok(value) if missed.is_empty() => Ok(value),
            Ok(_) => Err(BootstrapHostCodecError::combine(missed)),
            Err(error) => {
                missed.push(error);
                Err(BootstrapHostCodecError::combine(missed))
            }
        }
    }

    pub(crate) fn type_symbol(&self, name: &str) -> Result<&str, BootstrapHostCodecError> {
        Ok(self.symbol_or_record(
            self.types
                .get(name)
                .copied()
                .ok_or_else(|| BootstrapHostCodecError::MissingType(name.to_string())),
        ))
    }

    fn definition_id(&self, name: &str) -> Result<MirTypeId, BootstrapHostCodecError> {
        self.definitions
            .get(name)
            .copied()
            .ok_or_else(|| BootstrapHostCodecError::MissingType(name.to_string()))
    }

    pub(crate) fn field_symbol(
        &self,
        owner: &str,
        field: &str,
    ) -> Result<&str, BootstrapHostCodecError> {
        Ok(self.symbol_or_record(self.field_binding(owner, field).map(|row| row.symbol.as_str())))
    }

    pub(crate) fn field_binding(
        &self,
        owner: &str,
        field: &str,
    ) -> Result<&BootstrapFieldBinding, BootstrapHostCodecError> {
        let owner_id = self.definition_id(owner)?;
        self.fields
            .get(&(owner_id, field))
            .copied()
            .ok_or_else(|| BootstrapHostCodecError::MissingField {
                owner: owner.to_string(),
                field: field.to_string(),
            })
    }

    /// The binding of one checked field by identity: the lookup for enum
    /// payload fields, whose names are unique only within their variant.
    pub(crate) fn field_binding_by_id(
        &self,
        owner: MirTypeId,
        owner_name: &str,
        field: MirFieldId,
        field_name: &str,
    ) -> Result<&BootstrapFieldBinding, BootstrapHostCodecError> {
        self.field_ids
            .get(&(owner, field))
            .copied()
            .ok_or_else(|| BootstrapHostCodecError::MissingField {
                owner: owner_name.to_string(),
                field: field_name.to_string(),
            })
    }

    pub(crate) fn variant_symbol(
        &self,
        owner: &str,
        variant: &str,
    ) -> Result<&str, BootstrapHostCodecError> {
        let symbol = self.definition_id(owner).and_then(|owner_id| {
            self.variants.get(&(owner_id, variant)).copied().ok_or_else(|| {
                BootstrapHostCodecError::MissingVariant {
                    owner: owner.to_string(),
                    variant: variant.to_string(),
                }
            })
        });
        Ok(self.symbol_or_record(symbol))
    }

    pub(crate) fn variant_path(
        &self,
        owner: &str,
        variant: &str,
    ) -> Result<String, BootstrapHostCodecError> {
        Ok(self.variant_symbol(owner, variant)?.to_string())
    }

    /// The checked definition row of a bound nominal type.
    pub(crate) fn definition(&self, owner: &str) -> Result<&'a MirTypeDef, BootstrapHostCodecError> {
        let id = self.definition_id(owner)?;
        self.program
            .types
            .iter()
            .find(|definition| definition.id == id)
            .ok_or_else(|| BootstrapHostCodecError::MissingType(owner.to_string()))
    }

    /// The emitted Rust path and checked payload of `owner::variant`.
    fn variant_payload(
        &self,
        owner: &str,
        variant: &str,
    ) -> Result<(String, &'a MirTypeDef, &'a MirVariantPayload), BootstrapHostCodecError> {
        let path = self.variant_path(owner, variant)?;
        let definition = self.definition(owner)?;
        let MirTypeDefKind::Enum { variants, .. } = &definition.kind else {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "bootstrap codec type `{owner}` is not an enum"
            )));
        };
        let payload = variants
            .iter()
            .find(|candidate| candidate.name == variant)
            .map(|candidate| &candidate.payload)
            .ok_or_else(|| BootstrapHostCodecError::MissingVariant {
                owner: owner.to_string(),
                variant: variant.to_string(),
            })?;
        Ok((path, definition, payload))
    }

    /// Pairs each listed payload field with its emitted symbol and whether the
    /// emitter boxes it. The list must name exactly the declared payload, so a
    /// drifted Jet declaration fails packaging instead of emitting stale Rust.
    fn named_payload<'f, T: Copy>(
        &self,
        owner: &str,
        variant: &str,
        definition: &MirTypeDef,
        declared: &[jet_foundation::MIR::MirField],
        listed: &[(&'f str, T)],
    ) -> Result<Vec<(&str, bool, T)>, BootstrapHostCodecError> {
        let mismatch = || {
            BootstrapHostCodecError::InvalidMetadata(format!(
                "host glue lists payload [{}] for `{owner}::{variant}`, the checked payload is [{}]",
                listed.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(", "),
                declared.iter().map(|field| field.name.as_str()).collect::<Vec<_>>().join(", "),
            ))
        };
        if listed.len() != declared.len() {
            return Err(mismatch());
        }
        listed
            .iter()
            .map(|(name, item)| {
                let field = declared.iter().find(|field| field.name == *name).ok_or_else(mismatch)?;
                let symbol = self
                    .field_binding_by_id(definition.id, owner, field.id, &field.name)?
                    .symbol
                    .as_str();
                let boxed = definition.boxed_edges.iter().any(|edge| *edge == format!("{variant}.{name}"));
                Ok((symbol, boxed, *item))
            })
            .collect()
    }

    /// A Rust pattern for `owner::variant`. Each entry is a checked payload
    /// field name, optionally `field: binding` (the binding defaults to the
    /// field name); `[".."]` ignores the whole payload. Boxed payload fields
    /// bind the `Box` itself.
    pub(crate) fn variant_pattern(
        &self,
        owner: &str,
        variant: &str,
        bindings: &[&str],
    ) -> Result<String, BootstrapHostCodecError> {
        let (path, definition, payload) = self.variant_payload(owner, variant)?;
        let ignore_all = bindings == [".."];
        Ok(match payload {
            MirVariantPayload::Unit if bindings.is_empty() || ignore_all => path,
            MirVariantPayload::Single(_) if ignore_all => format!("{path}(..)"),
            MirVariantPayload::Single(_) if bindings.len() == 1 => {
                let binding = bindings[0].split_once(':').map_or(bindings[0], |(_, binding)| binding.trim());
                format!("{path}({binding})")
            }
            MirVariantPayload::Named(_) if ignore_all => format!("{path} {{ .. }}"),
            MirVariantPayload::Named(declared) => {
                let listed = bindings
                    .iter()
                    .map(|entry| match entry.split_once(':') {
                        Some((field, binding)) => (field.trim(), binding.trim()),
                        None => (entry.trim(), entry.trim()),
                    })
                    .collect::<Vec<_>>();
                let fields = self.named_payload(owner, variant, definition, declared, &listed)?;
                format!(
                    "{path} {{ {} }}",
                    fields
                        .iter()
                        .map(|(symbol, _, binding)| format!("{symbol}: {binding}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            _ => {
                return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                    "host glue binds {} payload values of `{owner}::{variant}`, which has a different checked payload",
                    bindings.len()
                )))
            }
        })
    }

    /// A Rust constructor for `owner::variant` from `(field, expression)`
    /// pairs naming exactly the checked payload. Fields the emitter boxes are
    /// wrapped in `Box::new`.
    pub(crate) fn variant_value(
        &self,
        owner: &str,
        variant: &str,
        values: &[(&str, &str)],
    ) -> Result<String, BootstrapHostCodecError> {
        let (path, definition, payload) = self.variant_payload(owner, variant)?;
        let boxed = |boxed: bool, value: &str| {
            if boxed { format!("Box::new({value})") } else { value.to_string() }
        };
        Ok(match payload {
            MirVariantPayload::Unit if values.is_empty() => path,
            MirVariantPayload::Single(_) if values.len() == 1 => format!(
                "{path}({})",
                boxed(definition.boxed_edges.iter().any(|edge| edge == variant), values[0].1)
            ),
            MirVariantPayload::Named(declared) => {
                let fields = self.named_payload(owner, variant, definition, declared, values)?;
                format!(
                    "{path} {{ {} }}",
                    fields
                        .iter()
                        .map(|(symbol, is_boxed, value)| format!("{symbol}: {}", boxed(*is_boxed, value)))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            _ => {
                return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                    "host glue builds `{owner}::{variant}` from {} values, which does not match its checked payload",
                    values.len()
                )))
            }
        })
    }

    /// `symbol: value` for one struct field, wrapping `value` in `Box::new`
    /// when the emitter boxes the field.
    pub(crate) fn field_init(
        &self,
        owner: &str,
        field: &str,
        value: &str,
    ) -> Result<String, BootstrapHostCodecError> {
        let symbol = self.field_symbol(owner, field)?;
        Ok(if self.definition(owner)?.boxed_edges.iter().any(|edge| edge == field) {
            format!("{symbol}: Box::new({value})")
        } else {
            format!("{symbol}: {value}")
        })
    }

    /// `symbol(arguments)` calling one checked Source callable. Each
    /// `(place, owned)` value is passed the way the emitter spells its
    /// parameter (`&mut place`, `&place`, or the owned expression), so the
    /// call follows the checked access of every parameter.
    pub(crate) fn call(
        &self,
        source_name: &str,
        values: &[(&str, &str)],
    ) -> Result<String, BootstrapHostCodecError> {
        let binding = checked_bootstrap_callable(self.metadata, source_name)?;
        if binding.metadata.parameter_types.len() != values.len() {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "host glue calls `{source_name}` with {} arguments; its checked signature has {}",
                values.len(),
                binding.metadata.parameter_types.len()
            )));
        }
        Ok(format!(
            "{}({})",
            binding.metadata.symbol,
            bootstrap_call_arguments(&binding.metadata.parameter_types, values)
        ))
    }

    /// Whether the emitter boxes `owner`'s edge (`field` for a struct field,
    /// `Variant.field` for a payload field, `Variant` for a single payload).
    pub(crate) fn boxed_edge(definition: &MirTypeDef, edge: &str) -> bool {
        definition.boxed_edges.iter().any(|candidate| candidate == edge)
    }

    /// A Rust struct literal of `owner` from `(field, expression)` pairs naming
    /// exactly its checked fields; boxed fields are wrapped in `Box::new`.
    pub(crate) fn struct_value(
        &self,
        owner: &str,
        values: &[(&str, &str)],
    ) -> Result<String, BootstrapHostCodecError> {
        let definition = self.definition(owner)?;
        let MirTypeDefKind::Struct { fields, .. } = &definition.kind else {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "bootstrap codec type `{owner}` is not a struct"
            )));
        };
        let listed = values.iter().map(|(name, _)| *name).collect::<Vec<_>>();
        if listed.len() != fields.len() || fields.iter().any(|field| !listed.contains(&field.name.as_str())) {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "host glue builds `{owner}` from fields [{}], the checked fields are [{}]",
                listed.join(", "),
                fields.iter().map(|field| field.name.as_str()).collect::<Vec<_>>().join(", "),
            )));
        }
        let initializers = values
            .iter()
            .map(|(field, value)| self.field_init(owner, field, value))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(format!("{} {{ {} }}", self.type_symbol(owner)?, initializers.join(", ")))
    }

    /// `*` when the emitter boxes `owner.field` (a struct field) or
    /// `owner::variant.field` (a payload field), else empty: a dereference
    /// prefix that turns a read of the emitted field into the checked type.
    fn boxed_deref(&self, owner: &str, edge: &str) -> Result<&'static str, BootstrapHostCodecError> {
        Ok(if self.definition(owner)?.boxed_edges.iter().any(|candidate| candidate == edge) { "*" } else { "" })
    }

    /// Render a host-glue template against the binding metadata. Markers:
    /// `@t.Type@` type, `@f.Type.field@` field symbol, `@v.Type.Variant@`
    /// variant path, `@deref.Type.field@` / `@deref.Type.Variant.field@` the
    /// boxed-edge dereference prefix, and the shape-checked literals
    /// `@p.Type.Variant@{ field, field: binding }` (pattern),
    /// `@new.Type.Variant@{ field: expr }` (enum value) and
    /// `@s.Type@{ field: expr }` (struct value). Literal fields use checked Jet
    /// names; a unit variant takes no braces and `{ .. }` ignores a payload.
    /// A pattern or enum value may instead list its payload positionally,
    /// directly after the marker: `@p.Type.Variant@(binding, binding)` and
    /// `@new.Type.Variant@(expr, expr)` name every declared payload field in
    /// declaration order (`(..)` ignores a payload).
    ///
    /// A marker that does not match the checked shape is recorded and spelled
    /// `MISSING_SYMBOL`, so one packaging run reports every drifted marker.
    pub(crate) fn render(&self, template: &str) -> Result<String, BootstrapHostCodecError> {
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(start) = rest.find('@') {
            out.push_str(&rest[..start]);
            let after = &rest[start + 1..];
            let marker = after.find('@').map(|end| &after[..end]).filter(|marker| {
                matches!(marker.split('.').next(), Some("t" | "f" | "v" | "p" | "new" | "s" | "deref"))
                    && marker.contains('.')
                    && marker.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
            });
            let Some(marker) = marker else {
                out.push('@');
                rest = after;
                continue;
            };
            rest = &after[marker.len() + 1..];
            match self.render_marker(marker, &mut rest) {
                Ok(rendered) => out.push_str(&rendered),
                Err(error) => {
                    self.record(error);
                    out.push_str(MISSING_SYMBOL);
                }
            }
        }
        out.push_str(rest);
        Ok(out)
    }

    /// One `@marker@`; a literal marker also consumes its `{ ... }` or
    /// `( ... )` body from `rest`.
    fn render_marker(&self, marker: &str, rest: &mut &str) -> Result<String, BootstrapHostCodecError> {
        let parts = marker.split('.').collect::<Vec<_>>();
        let invalid = || BootstrapHostCodecError::InvalidMetadata(format!("malformed host glue marker `@{marker}@`"));
        Ok(match parts.as_slice() {
            ["t", owner] => self.type_symbol(owner)?.to_string(),
            ["f", owner, field] => self.field_symbol(owner, field)?.to_string(),
            ["v", owner, variant] => self.variant_path(owner, variant)?,
            ["deref", owner, field] => self.boxed_deref(owner, field)?.to_string(),
            ["deref", owner, variant, field] => self.boxed_deref(owner, &format!("{variant}.{field}"))?.to_string(),
            [kind @ ("p" | "new" | "s"), owner, tail @ ..] => {
                let entries = match template_literal_body(*rest) {
                    Some(('{', body, remaining)) => {
                        *rest = remaining;
                        template_literal_entries(body)
                    }
                    Some((_, body, remaining)) => {
                        *rest = remaining;
                        let [variant] = tail else { return Err(invalid()) };
                        self.positional_entries(owner, variant, body)?
                    }
                    None => Vec::new(),
                };
                let rendered = entries
                    .iter()
                    .map(|(name, value)| Ok((*name, value.map(|value| self.render(value)).transpose()?)))
                    .collect::<Result<Vec<_>, BootstrapHostCodecError>>()?;
                match (*kind, tail) {
                    ("p", [variant]) => {
                        let bindings = rendered
                            .iter()
                            .map(|(name, binding)| match binding {
                                Some(binding) => format!("{name}: {binding}"),
                                None => name.to_string(),
                            })
                            .collect::<Vec<_>>();
                        let bindings = bindings.iter().map(String::as_str).collect::<Vec<_>>();
                        self.variant_pattern(owner, variant, &bindings)?
                    }
                    ("new", [variant]) => {
                        let values = rendered
                            .iter()
                            .map(|(name, value)| (*name, value.as_deref().unwrap_or(*name)))
                            .collect::<Vec<_>>();
                        self.variant_value(owner, variant, &values)?
                    }
                    ("s", []) => {
                        let values = rendered
                            .iter()
                            .map(|(name, value)| (*name, value.as_deref().unwrap_or(*name)))
                            .collect::<Vec<_>>();
                        self.struct_value(owner, &values)?
                    }
                    _ => return Err(invalid()),
                }
            }
            _ => return Err(invalid()),
        })
    }

    /// Map a positional payload list onto the declared payload of
    /// `owner::variant`, in declaration order, as `(field, Some(entry))`
    /// pairs. `(..)` stays the ignore-all entry.
    fn positional_entries<'t>(
        &'t self,
        owner: &str,
        variant: &str,
        body: &'t str,
    ) -> Result<Vec<(&'t str, Option<&'t str>)>, BootstrapHostCodecError> {
        let entries = template_positional_entries(body);
        if entries == [".."] {
            return Ok(vec![("..", None)]);
        }
        let (_, _, payload) = self.variant_payload(owner, variant)?;
        let declared = match payload {
            MirVariantPayload::Named(declared) => declared.iter().map(|field| field.name.as_str()).collect::<Vec<_>>(),
            MirVariantPayload::Single(_) => vec!["value"],
            _ => Vec::new(),
        };
        if declared.len() != entries.len() {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "host glue lists {} positional payload values for `{owner}::{variant}`, the checked payload is [{}]",
                entries.len(),
                declared.join(", "),
            )));
        }
        Ok(declared.into_iter().zip(entries).map(|(name, entry)| (name, Some(entry))).collect())
    }
    pub(crate) fn callable_symbol(
        &self,
        source_name: &str,
    ) -> Result<&str, BootstrapHostCodecError> {
        let mut rows = self
            .metadata
            .callables
            .iter()
            .filter(|row| row.source_name == source_name);
        let symbol = match (rows.next(), rows.next()) {
            (Some(row), None) => Ok(row.metadata.symbol.as_str()),
            (None, _) => Err(BootstrapHostCodecError::MissingEntry(source_name.to_string())),
            (Some(_), Some(_)) => Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "Source callable `{source_name}` has an ambiguous emitted symbol"
            ))),
        };
        Ok(self.symbol_or_record(symbol))
    }
    pub(crate) fn trait_symbol(&self, name: &str) -> Result<&str, BootstrapHostCodecError> {
        let symbol = self
            .metadata
            .traits
            .iter()
            .find(|row| row.name == name)
            .map(|row| row.symbol.as_str())
            .ok_or_else(|| BootstrapHostCodecError::MissingEntry(format!("trait `{name}`")));
        Ok(self.symbol_or_record(symbol))
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

    /// Every checked method of `trait_name`, in the emitter's declaration
    /// order. The Jet trait is the only method list: host glue iterates this
    /// set instead of keeping its own copy.
    pub(crate) fn trait_methods(
        &self,
        trait_name: &str,
    ) -> Result<Vec<&MirRustTraitMethodMetadata>, BootstrapHostCodecError> {
        let trait_id = self
            .metadata
            .traits
            .iter()
            .find(|row| row.name == trait_name)
            .map(|row| row.trait_id)
            .ok_or_else(|| BootstrapHostCodecError::MissingEntry(format!("trait `{trait_name}`")))?;
        let methods = self
            .metadata
            .trait_methods
            .iter()
            .filter(|row| row.trait_id == trait_id)
            .collect::<Vec<_>>();
        if methods.is_empty() {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "trait `{trait_name}` has no checked methods"
            )));
        }
        Ok(methods)
    }
}

/// The `{ ... }` body that follows a shape-checked template marker (after
/// optional whitespace), or the positional `( ... )` body directly after it,
/// with its opening delimiter and the text after its closing one. Nested
/// delimiters and string literals inside the body are skipped.
fn template_literal_body(text: &str) -> Option<(char, &str, &str)> {
    let trimmed = text.trim_start();
    let offset = text.len() - trimmed.len();
    let open = trimmed.chars().next().filter(|open| *open == '{' || (*open == '(' && offset == 0))?;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (index, ch) in trimmed.char_indices() {
        if in_string {
            match ch {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some((open, &trimmed[1..index], &text[offset + index + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

/// Split a template literal body into `field` / `field: expression` entries
/// at its top-level commas. `::` path separators are not field separators.
fn template_literal_entries(body: &str) -> Vec<(&str, Option<&str>)> {
    let mut entries = Vec::new();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut start = 0;
    let mut colon = None;
    let mut angle = 0usize;
    let bytes = body.as_bytes();
    for (index, ch) in body.char_indices() {
        if in_string {
            match ch {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            '<' | '>' => template_angle_step(body, index, ch, &mut angle),
            ':' if depth == 0
                && angle == 0
                && colon.is_none()
                && bytes.get(index + 1) != Some(&b':')
                && (index == 0 || bytes[index - 1] != b':') =>
            {
                colon = Some(index)
            }
            ',' if depth == 0 && angle == 0 => {
                template_literal_entry(&body[start..index], colon.map(|colon| colon - start), &mut entries);
                start = index + 1;
                colon = None;
            }
            _ => {}
        }
    }
    template_literal_entry(&body[start..], colon.map(|colon| colon - start), &mut entries);
    entries
}

/// Split a positional `( ... )` body at its top-level commas.
fn template_positional_entries(body: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    let mut start = 0;
    let mut angle = 0usize;
    for (index, ch) in body.char_indices() {
        if in_string {
            match ch {
                _ if escaped => escaped = false,
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' | '(' | '[' => depth += 1,
            '}' | ')' | ']' => depth -= 1,
            '<' | '>' => template_angle_step(body, index, ch, &mut angle),
            ',' if depth == 0 && angle == 0 => {
                entries.push(body[start..index].trim());
                start = index + 1;
            }
            _ => {}
        }
    }
    entries.push(body[start..].trim());
    entries.retain(|entry| !entry.is_empty());
    entries
}

/// Track generic-argument nesting opened by a turbofish (`::<`), so commas
/// in `collect::<Result<Vec<_>, _>>()` do not split entries. A bare `<` is a
/// comparison and `->` / `=>` are not closers.
fn template_angle_step(body: &str, index: usize, ch: char, angle: &mut usize) {
    match ch {
        '<' if *angle > 0 || body[..index].ends_with("::") => *angle += 1,
        '>' if *angle > 0 && !matches!(body[..index].chars().last(), Some('-' | '=')) => *angle -= 1,
        _ => {}
    }
}

fn template_literal_entry<'t>(entry: &'t str, colon: Option<usize>, entries: &mut Vec<(&'t str, Option<&'t str>)>) {
    match colon {
        Some(colon) => entries.push((entry[..colon].trim(), Some(entry[colon + 1..].trim()))),
        None if !entry.trim().is_empty() => entries.push((entry.trim(), None)),
        None => {}
    }
}

fn bootstrap_required_type_names() -> &'static [&'static str] {
    &[
        "MIRPreludeCall",
        "MIRCallSignature",
        "MIRSymbol",
        "MIRType",
        "MIRTypeKind",
        "MIRTypeID",
        "MIRMeasure",
        "MIRMeasureRule",
        "MIRDimension",
        "MIRDimensionAxis",
        "MIRLayout",
        "MIRABI",
        "MIRScalarKind",
        "MIRSize",
        "MIRTraitID",
        "MIRTraitMethodID",
        "MIRArtifactID",
        "MIRProgram",
        "MIRNominalRef",
        "MIRTupleField",
        "MIRTagMarker",
        "MIRInternalTag",
        "MIRAccess",
        "MIRFunctionID",
        "MIRFieldID",
        "MIRParam",
        "MIROwnership",
        "MIROwnershipMode",
        "MIRDropKind",
        "MIRForeign",
        "MIRForeignABI",
        "MIRForeignLanguage",
        "MIRTargetApplicability",
        "MIREffectFacts",
        "MIRNamedSpan",
        "MIRModuleID",
        "MIRForeignID",
        "MIRLinkUnitID",
        "MIRCallbackID",
        "MIRHandleID",
        "JetEvalHostWriteback",
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
        "MIRArtifactBuildMode",
        "JetEvalHostCoreOwner",
        "MIRCoreOwner",
"MIRSourceFile",
    ]
}

fn bootstrap_required_callable<'a>(
    metadata: &'a BootstrapBindingDescriptor,
    name: &str,
) -> Result<&'a MirRustCallableMetadata, BootstrapHostCodecError> {
    Ok(&checked_bootstrap_callable(metadata, name)?.metadata)
}

/// The checked Source callables the native compile entry reads facts from:
/// the callback-transfer helper names the Source `JetEvalMachine` ABI type
/// (its first parameter), and the host-shape accessor projects checked types
/// into evaluator host shapes.
#[derive(Clone, Debug)]
struct BootstrapNativeHelperRoots {
    callback_transfer: jet_foundation::MIR::MirFunctionId,
    host_type_shape: jet_foundation::MIR::MirFunctionId,
}

fn checked_bootstrap_callable<'a>(
    metadata: &'a BootstrapBindingDescriptor,
    name: &str,
) -> Result<&'a BootstrapCallableBinding, BootstrapHostCodecError> {
    let mut rows = metadata
        .callables
        .iter()
        .filter(|row| row.source_name == name);
    let row = rows
        .next()
        .ok_or_else(|| BootstrapHostCodecError::MissingEntry(name.to_string()))?;
    if rows.next().is_some() {
        return Err(BootstrapHostCodecError::InvalidMetadata(format!(
            "Source callable `{name}` has ambiguous checked bindings"
        )));
    }
    Ok(row)
}

fn checked_bootstrap_native_helper_roots(
    metadata: &BootstrapBindingDescriptor,
    program: &MirProgram,
    artifact_id: jet_foundation::MIR::MirArtifactId,
) -> Result<BootstrapNativeHelperRoots, BootstrapHostCodecError> {
    use jet_foundation::MIR::MirArtifactTarget;

    let artifact = program
        .artifacts
        .iter()
        .find(|artifact| artifact.id == artifact_id)
        .ok_or_else(|| BootstrapHostCodecError::InvalidMetadata(
            "native helper artifact is absent from checked MIR".to_string(),
        ))?;
    if artifact.target != MirArtifactTarget::RustAot {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "native helper roots require the canonical Rust AOT compiler artifact".to_string(),
        ));
    }
    let checked = |name: &str| {
        let binding = checked_bootstrap_callable(metadata, name)?;
        if binding.metadata.symbol.is_empty() {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "Source callable `{name}` has an empty emitted symbol"
            )));
        }
        let function = program
            .functions
            .iter()
            .find(|function| function.id == binding.metadata.function)
            .ok_or_else(|| BootstrapHostCodecError::InvalidMetadata(format!(
                "checked private Source helper `{name}` is absent from MIR"
            )))?;
        if !artifact.modules.contains(&function.module_id)
            || !function.capture_params.is_empty()
            || !function.target_applicability.rust_aot
        {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "checked private Source helper `{name}` is not capture-free and executable in artifact {artifact_id:?}"
            )));
        }
        Ok(function.id)
    };
    Ok(BootstrapNativeHelperRoots {
        callback_transfer: checked("jet_eval_callback_transfer")?,
        host_type_shape: checked("jet_eval_host_type_shape_from_program")?,
    })
}

fn validate_bootstrap_numeric_callable_type(
    field: &BootstrapFieldBinding,
) -> Result<(), BootstrapHostCodecError> {
    let callback = match &field.ty.kind {
        jet_foundation::MIR::MirTypeKind::Option(callback) => callback,
        _ => {
            return Err(BootstrapHostCodecError::InvalidMetadata(
                "SemaRegistrationHostHooks.numeric_unit_conversion_exact must be an optional checked function type"
                    .to_string(),
            ));
        }
    };
    let signature = callback.function_signature().ok_or_else(|| {
        BootstrapHostCodecError::InvalidMetadata(
            "SemaRegistrationHostHooks.numeric_unit_conversion_exact must contain a checked Fn type".to_string(),
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
            "SemaRegistrationHostHooks.numeric_unit_conversion_exact must have its exact checked (Float, String, String, String, String) parameter types"
                .to_string(),
        ));
    }
    let returns_optional_float = matches!(
        signature.ret.as_deref().and_then(MirType::option_inner),
        Some(output) if output.canonical_key() == "Float"
    );
    if !returns_optional_float {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "SemaRegistrationHostHooks.numeric_unit_conversion_exact must return the exact checked optional Float"
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
    let symbols = BootstrapCodecSymbols::new(bindings, program)?;
    // Packaging keeps going after a failed check or stage, recording its error
    // beside the failed lookups, so one stage-zero run reports every failure.
    let keep = |result: Result<(), BootstrapHostCodecError>| {
        if let Err(error) = result {
            symbols.record(error);
        }
    };
    keep(
        symbols
            .field_binding("SemaRegistrationHostHooks", "numeric_unit_conversion_exact")
            .and_then(validate_bootstrap_numeric_callable_type),
    );
    for name in bootstrap_required_type_names() {
        let _ = symbols.type_symbol(name)?;
    }
    let entry = bootstrap_required_callable(bindings, "jet_bootstrap_compile")
        .and_then(|entry| {
            if entry.parameter_types.len() != 1 || entry.parameter_access.len() != 1 {
                return Err(BootstrapHostCodecError::InvalidMetadata(
                    "jet_bootstrap_compile must retain exactly one owned request parameter"
                        .to_string(),
                ));
            }
            if entry.parameter_types[0].starts_with('&') {
                return Err(BootstrapHostCodecError::InvalidMetadata(
                    "jet_bootstrap_compile request must be an owned parameter".to_string(),
                ));
            }
            Ok(entry)
        })
        .map_err(|error| symbols.record(error))
        .ok();

    let eval_default = bootstrap_required_callable(bindings, "jet_eval_default_config")
        .and_then(|eval_default| {
            if !eval_default.parameter_types.is_empty() || !eval_default.parameter_access.is_empty() {
                return Err(BootstrapHostCodecError::InvalidMetadata(
                    "jet_eval_default_config must be a zero-parameter callable".to_string(),
                ));
            }
            Ok(eval_default)
        })
        .map_err(|error| symbols.record(error))
        .ok();
    let eval_runtime_config = bootstrap_required_callable(bindings, "jet_eval_runtime_config")
        .and_then(|eval_runtime_config| {
            if eval_runtime_config.parameter_types.len() != 1
                || eval_runtime_config.parameter_access.as_slice()
                    != [jet_foundation::MIR::MirAccess::Read]
            {
                return Err(BootstrapHostCodecError::InvalidMetadata(
                    "jet_eval_runtime_config must be a one-parameter Read fork".to_string(),
                ));
            }
            Ok(eval_runtime_config)
        })
        .map_err(|error| symbols.record(error))
        .ok();

    let native_helper_roots = checked_bootstrap_native_helper_roots(
        bindings,
        program,
        config.execution.artifact,
    )
    .map_err(|error| symbols.record(error))
    .ok();

    let mut glue = String::new();
    emit_bootstrap_type_codec(&mut glue);
    emit_bootstrap_value_codec(&mut glue);
    emit_bootstrap_host_value_codec(&mut glue);
    crate::compiler_bootstrap_diagnostic_codec::append_diagnostic_codec(&mut glue);
    keep(crate::compiler_bootstrap_runtime_mir_codec::append_runtime_mir_codec(&mut glue, &symbols));
    if let Some(entry) = entry {
        keep(crate::compiler_bootstrap_entry_codec::append_bootstrap_entry_codec(
            &mut glue,
            bindings,
            &symbols,
            program,
            entry.function,
        ));
    }
    keep(emit_bootstrap_source_resource_bridge(&mut glue, &symbols));
    keep(emit_bootstrap_callback(&mut glue, &symbols));
    keep(emit_bootstrap_foreign_callback(&mut glue, &symbols));
    keep(emit_bootstrap_native_adapter_impl(&mut glue, &symbols));
    keep(emit_bootstrap_web_asset_provider(&mut glue, &symbols));

    keep(emit_bootstrap_manifest_adapter(&mut glue, &symbols));
    if let (Some(eval_default), Some(eval_runtime_config)) = (eval_default, eval_runtime_config) {
        keep(emit_bootstrap_host_factory(
            &mut glue,
            &symbols,
            &eval_default.symbol,
            &eval_runtime_config.symbol,
        ));
    }

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
    glue.push_str(
        r#"#[doc(hidden)]
pub(crate) const __JET_BOOTSTRAP_NUMERIC_UNIT_CONVERSION_KEY: &str =
    "jet_eval.numeric_unit_conversion_exact";
fn __jet_bootstrap_native_numeric_unit_conversion(
    call: &mut ::jet_jit::SourceInterfaces::NativeCallableCall,
) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {
    let identity = call.identity().ok_or_else(|| "numeric-unit native call has no checked identity".to_string())?;
    if identity.key.as_str() != __JET_BOOTSTRAP_NUMERIC_UNIT_CONVERSION_KEY {
        return Err("numeric-unit native call has a different registration key".to_string());
    }
    if call.signature().parameters.len() != 5 || call.argument(5).is_some() {
        return Err("numeric-unit native call does not have the exact five-argument signature".to_string());
    }
    let value = match call.argument(0).map(|argument| &argument.value) {
        Some(::jet_foundation::MIR::MirRuntimeValue::Float { value, f32: false }) => *value,
        _ => return Err("numeric-unit native call argument 0 is not checked Float".to_string()),
    };
    let read_string = |index| -> Result<&str, String> {
        match call.argument(index).map(|argument| &argument.value) {
            Some(::jet_foundation::MIR::MirRuntimeValue::String(value)) => Ok(value),
            _ => Err(format!("numeric-unit native call argument {index} is not checked String")),
        }
    };
    let scale_num = read_string(1)?;
    let scale_den = read_string(2)?;
    let offset_num = read_string(3)?;
    let offset_den = read_string(4)?;
    let return_type = call.signature().return_type.as_ref()
        .ok_or_else(|| "numeric-unit native callable has no checked return type".to_string())?;
    let ::jet_foundation::MIR::MirTypeKind::Option(element) = return_type.kind() else {
        return Err("numeric-unit native callable does not return checked optional Float".to_string());
    };
    if !matches!(element.kind(), ::jet_foundation::MIR::MirTypeKind::Float) {
        return Err("numeric-unit native callable does not return checked optional Float".to_string());
    }
    match ::jet_foundation::jet_unit_conversion_exact(
        value,
        scale_num,
        scale_den,
        offset_num,
        offset_den,
    ) {
        Some(value) => Ok(::jet_foundation::MIR::MirRuntimeValue::Present(Box::new(
            ::jet_foundation::MIR::MirRuntimeValue::Float { value, f32: false },
        ))),
        None => Ok(::jet_foundation::MIR::MirRuntimeValue::Absent {
            element: (**element).clone(),
        }),
    }
}
"#,
    );
    if let (Some(entry), Some(eval_runtime_config), Some(native_helper_roots)) =
        (entry, eval_runtime_config, &native_helper_roots)
    {
        keep(emit_bootstrap_native_compile_entry(
            &mut glue,
            bindings,
            &symbols,
            entry,
            &eval_runtime_config.symbol,
            native_helper_roots,
        ));
    }
    keep(emit_bootstrap_task_roots_fixture(&mut glue, bindings, &symbols));

    // `root_prefix` is deliberately read here so a future source-coupled
    // emitter can reject an incompatible root before this private splice is
    // linked. The current bootstrap host is linked as the compiler crate.

    if config.root_prefix.is_empty() {
        symbols.record(BootstrapHostCodecError::InvalidMetadata(
            "generated Rust root prefix is empty".to_string(),
        ));
    }
    // Every emitter writes shape-dependent spellings as metadata markers; one
    // pass resolves them against the checked bindings and boxed edges.
    let glue = symbols.render(&glue);
    let glue = symbols.finish(glue)?;
    source.push_str(&glue);
    Ok(())

}

fn emit_bootstrap_native_compile_entry(
    out: &mut String,
    bindings: &BootstrapBindingDescriptor,
    symbols: &BootstrapCodecSymbols<'_>,
    entry: &MirRustCallableMetadata,
    runtime_config_symbol: &str,
    helpers: &BootstrapNativeHelperRoots,
) -> Result<(), BootstrapHostCodecError> {
    let eval_config = symbols.type_symbol("JetEvalConfig")?;
    let request_eval_config = symbols.field_symbol("JetDriverCompileRequest", "eval_config")?;
    let request_host_adapter = symbols.field_symbol("JetEvalConfig", "host_adapter")?;
    let request_host_hooks = symbols.field_symbol("JetEvalConfig", "host_hooks")?;
    let host_hooks_type = symbols.type_symbol("SemaRegistrationHostHooks")?;
    let hook_numeric = symbols.field_symbol("SemaRegistrationHostHooks", "numeric_unit_conversion_exact")?;
    let hook_boundary = symbols.field_symbol("SemaRegistrationHostHooks", "typed_boundary_literal_validate")?;
    let host_field = symbols.field_binding("JetEvalConfig", "host_adapter")?;
    let numeric_field = symbols.field_binding("SemaRegistrationHostHooks", "numeric_unit_conversion_exact")?;
    let entry_id = entry.function.0.to_string();
    let callback_transfer = helpers.callback_transfer.0.to_string();
    let host_type_shape = helpers.host_type_shape.0.to_string();
    let shape_callable = checked_bootstrap_callable(bindings, "jet_eval_host_type_shape_from_program")?;
    if shape_callable.metadata.parameter_types.len() != 3 {
        return Err(BootstrapHostCodecError::InvalidMetadata(
            "Source host-shape accessor must take its program, checked type, and source span".to_string(),
        ));
    }
    let shape_arguments = bootstrap_call_arguments(
        &shape_callable.metadata.parameter_types,
        &[
            ("__jet_shape_program", "__jet_shape_program.clone()"),
            ("__jet_shape_type", "__jet_shape_type.clone()"),
            ("__jet_shape_span", "__jet_shape_span.clone()"),
        ],
    );
    let generated = r#"
#[doc(hidden)]
fn __jet_bootstrap_native_collect_shared_payload_types(
    program: &::jet_foundation::MIR::MirProgram,
) -> ::std::collections::BTreeMap<String, ::jet_foundation::MIR::MirType> {
    use ::jet_foundation::MIR::MirTypeKind as K;

    fn visit(
        ty: &::jet_foundation::MIR::MirType,
        payloads: &mut ::std::collections::BTreeMap<
            String,
            ::jet_foundation::MIR::MirType,
        >,
    ) {
        match ty.kind() {
            K::Shared(inner) => {
                payloads
                    .entry(inner.canonical_key())
                    .or_insert_with(|| inner.as_ref().clone());
                visit(inner, payloads);
            }
            K::List(inner)
            | K::Option(inner)
            | K::FixedList { elem: inner, .. }
            | K::InlineRange { base: inner, .. }
            | K::Tagged { inner, .. }
            | K::Quantity { base: inner, .. } => visit(inner, payloads),
            K::Map { key, value } => {
                visit(key, payloads);
                visit(value, payloads);
            }
            K::Result { ok, err } => {
                visit(ok, payloads);
                visit(err, payloads);
            }
            K::Fn(signature) => {
                for parameter in &signature.params {
                    visit(parameter, payloads);
                }
                if let Some(result) = &signature.ret {
                    visit(result, payloads);
                }
            }
            K::SendFn { params, ret, .. } => {
                for parameter in params {
                    visit(parameter, payloads);
                }
                if let Some(result) = ret {
                    visit(result, payloads);
                }
            }
            K::Apply { args, .. } => {
                for argument in args {
                    visit(argument, payloads);
                }
            }
            K::Tuple(fields) => {
                for (_, field_type) in fields {
                    visit(field_type, payloads);
                }
            }
            K::Union(variants) => {
                for variant in variants {
                    visit(variant, payloads);
                }
            }
            _ => {}
        }
    }

    let mut payloads = ::std::collections::BTreeMap::new();
    for ty in &program.type_instances {
        visit(ty, &mut payloads);
    }
    payloads
}

struct __JetBootstrapCompileLease {
    result: __COMPILE_RESULT__,
    resources: ::jet_jit::SourceResources::SourceResourceSession,
    runtime_config: __EVAL_CONFIG__,
    selected_factory_tier: crate::BootstrapFactoryTier,
    actual_factory_tier: crate::BootstrapFactoryTier,
}

#[doc(hidden)]
pub fn __jet_bootstrap_compile_with_native(
    mut __jet_request: __COMPILE_REQUEST__,
    __jet_selected_tier: crate::BootstrapFactoryTier,
    __jet_execution: &crate::Codegen::MIRRust::MirRustExecutionConfig,
    __jet_completion_owner: &mut crate::compiler_bootstrap_runner::BootstrapRunCompletionOwner,
) -> Result<__JetBootstrapCompileLease, crate::BootstrapHostCodecError> {
    let __jet_compiler_image = crate::__jet_bootstrap_compiler_image()
        .map_err(crate::BootstrapHostCodecError::InvalidMetadata)?;
    let __jet_image_entry = __jet_compiler_image.header.entry_function;
    if __jet_image_entry != ::jet_foundation::MIR::MirFunctionId(__ENTRY_ID__) {
        return Err(crate::BootstrapHostCodecError::InvalidMetadata(
            "restored compiler image entry differs from the checked generated factory".to_string(),
        ));
    }
    if __jet_execution.artifact != __jet_compiler_image.header.artifact {
        return Err(crate::BootstrapHostCodecError::InvalidMetadata(
            "requested compiler factory artifact differs from the embedded canonical image".to_string(),
        ));
    }
    let __jet_image_artifact = __jet_compiler_image.program.artifacts.iter()
        .find(|artifact| artifact.id == __jet_compiler_image.header.artifact)
        .ok_or_else(|| crate::BootstrapHostCodecError::InvalidMetadata(
            "restored compiler image artifact is absent from its checked MIR".to_string(),
        ))?;
    if __jet_image_artifact.target != ::jet_foundation::MIR::MirArtifactTarget::RustAot {
        return Err(crate::BootstrapHostCodecError::InvalidMetadata(
            "restored compiler image is not the checked Rust AOT compiler factory root".to_string(),
        ));
    }
    let __jet_image_factory = __jet_compiler_image.program.functions.iter()
        .find(|function| function.id == __jet_image_entry)
        .ok_or_else(|| crate::BootstrapHostCodecError::InvalidMetadata(
            "restored compiler image factory function is absent".to_string(),
        ))?;
    if !__jet_image_factory.capture_params.is_empty() {
        return Err(crate::BootstrapHostCodecError::InvalidMetadata(
            "restored compiler image factory unexpectedly captures values".to_string(),
        ));
    }

    let __jet_completion_scope = __jet_completion_owner.completion_scope();
    let __jet_callback_jobs: ::std::sync::Arc<
        dyn ::jet_jit::SourceCallbacks::SourceCallbackJobOwner,
    > = __JetBootstrapNativeCallbackJobs::new();
    __jet_completion_owner
        .register_callback_jobs(__jet_callback_jobs.clone())
        .map_err(|_| crate::BootstrapHostCodecError::InvalidMetadata(
            "compiler invocation already has a callback-job owner".to_string(),
        ))?;
    __jet_completion_scope.with_current(|| {
    let __jet_resources = ::jet_jit::SourceResources::SourceResourceSession::new();
    __jet_completion_owner.set_resources(__jet_resources.clone());
    let mut __jet_resources = Some(__jet_resources);
    let mut __jet_runtime_config: Option<__EVAL_CONFIG__> = None;
    let __jet_result = (|| -> Result<__JetBootstrapCompileLease, String> {
        let __jet_resource_session = __jet_resources.as_ref()
            .ok_or_else(|| "Source resource session is absent".to_string())?;
        let __jet_root_lease = __jet_resource_session.retain_root()?;
        let __jet_bindings = ::std::sync::Arc::new(
            ::jet_jit::SourceInterfaces::NativeInterfaceBindings::new(),
        );
        let __jet_receiver_field = crate::__jet_bootstrap_entry_field_type(
            __jet_compiler_image.program.as_ref(),
            ::jet_foundation::MIR::MirTypeId(__HOST_FIELD_OWNER__),
            ::jet_foundation::MIR::MirFieldId(__HOST_FIELD_ID__),
        )?;
        let __jet_receiver_type = __jet_receiver_field.option_inner()
            .ok_or_else(|| "checked JetEvalConfig.host_adapter lost its Option leaf".to_string())?
            .clone();
        let __jet_template = __jet_bootstrap_native_adapter_template(
            &__jet_bindings,
            __jet_compiler_image.program.as_ref(),
            __jet_compiler_image.header.artifact,
            __jet_receiver_type.clone(),
        )?;
        let (__jet_numeric_object, __jet_numeric_identity) =
            __jet_bootstrap_native_numeric_callable_template(
                &__jet_bindings,
                __jet_compiler_image.program.as_ref(),
                __jet_compiler_image.header.artifact,
            )?;
        let __jet_numeric_type = crate::__jet_bootstrap_entry_field_type(
            __jet_compiler_image.program.as_ref(),
            ::jet_foundation::MIR::MirTypeId(__NUMERIC_FIELD_OWNER__),
            ::jet_foundation::MIR::MirFieldId(__NUMERIC_FIELD_ID__),
        )?;
        let __jet_numeric_callable_type = __jet_numeric_type.option_inner()
            .ok_or_else(|| "checked numeric callback field lost its Option leaf".to_string())?
            .clone();
        if !__jet_numeric_callable_type.same_checked_type(&__jet_numeric_identity.callable_type) {
            return Err("numeric callable template differs from the exact checked config field".to_string());
        }
        let __jet_numeric_registration = __jet_bootstrap_native_binding_registration(
            &__jet_root_lease,
            None,
            __jet_compiler_image.program.as_ref(),
            __jet_compiler_image.header.artifact,
            vec!["SemaRegistrationHostHooks".to_string(), "numeric_unit_conversion_exact".to_string()],
            @new.JetEvalNativeBindingIdentity.Callable@{ binding: @s.JetEvalNativeCallableBinding@{
                key: __jet_numeric_identity.key.clone(),
                callable_type: __jet_bootstrap_type_from_host(&__jet_numeric_identity.callable_type)?,
            } },
            ::jet_jit::SourceResources::SourceNativeBindingIdentity::Callable(
                __jet_numeric_identity,
            ),
            __jet_numeric_object,
            false,
        )?;
        // ABI metadata comes from the canonical Source accessor, not a second
        // Rust implementation of nominal/core-owner shape rules.
        let __jet_shape_function = __jet_compiler_image.program.functions.iter()
            .find(|function| function.id == ::jet_foundation::MIR::MirFunctionId(__HOST_TYPE_SHAPE_ID__))
            .ok_or_else(|| "checked Source host-shape accessor disappeared".to_string())?;
        let __jet_machine_type = __jet_compiler_image.program.functions.iter()
            .find(|function| function.id == ::jet_foundation::MIR::MirFunctionId(__CALLBACK_TRANSFER_ID__))
            .and_then(|function| function.params.first())
            .map(|parameter| &parameter.ty)
            .ok_or_else(|| "checked Source Machine parameter disappeared".to_string())?;
        // The Source projection exists only for these host-shape queries,
        // which read type, field, Core-owner and handle rows: it is converted
        // from the body-free signature program and dropped below, so the image
        // keeps only the native program resident.
        let mut __jet_shape_program = crate::__jet_bootstrap_mir_program_from_host(
            &crate::compiler_bootstrap_compiler_image::compiler_image_signature_program(
                __jet_compiler_image.program.as_ref(),
            ),
        )?;
        let mut __jet_shape_for = |checked_type: &::jet_foundation::MIR::MirType|
            -> Result<crate::compiler_bootstrap_entry_codec::BootstrapEntryHostTypeShape, String> {
            let mut __jet_shape_type = __jet_bootstrap_type_from_host(checked_type)?;
            let mut __jet_shape_span = __jet_bootstrap_span_from_host(&__jet_shape_function.span)?;
            let __jet_source_shape = __HOST_TYPE_SHAPE_SYMBOL__(__HOST_TYPE_SHAPE_ARGUMENTS__)
                .map_err(|_| format!(
                    "Source host-shape accessor rejected checked ABI type {}",
                    checked_type.canonical_key(),
                ))?;
            __jet_bootstrap_entry_host_type_shape_from_source(&__jet_source_shape)
        };
        let __jet_machine_abi_shape = __jet_shape_for(__jet_machine_type)?;
        let mut __jet_shared_payload_shapes = ::std::collections::BTreeMap::new();
        for (key, payload_type) in __jet_bootstrap_native_collect_shared_payload_types(
            __jet_compiler_image.program.as_ref(),
        ) {
            __jet_shared_payload_shapes.insert(key, __jet_shape_for(&payload_type)?);
        }
        drop(__jet_shape_for);
        drop(__jet_shape_program);
        let __jet_native_adapter = __JetBootstrapNativeAdapter::new_with_bindings(
            __jet_root_lease,
            __jet_bindings,
            __jet_compiler_image.program.clone(),
            __jet_machine_abi_shape,
            __jet_shared_payload_shapes,
            __jet_completion_scope.clone(),
            __jet_callback_jobs.clone(),
            __jet_compiler_image.header.artifact,
            __jet_receiver_type,
            __jet_template,
            vec![__jet_numeric_registration],
        )?;
        __jet_request.__REQUEST_EVAL_CONFIG__.__REQUEST_HOST_ADAPTER__ =
            Ok(Box::new(__jet_native_adapter.clone()));
        __jet_request.__REQUEST_EVAL_CONFIG__.__REQUEST_HOST_HOOKS__ =
            crate::jet_std::JetShared::new(__HOOKS_TYPE__ {
                __HOOK_NUMERIC__: Ok(__jet_bootstrap_native_numeric_wrapper_for_adapter(&__jet_native_adapter)?),
                __HOOK_BOUNDARY__: Err(jet_foundation::Outcome::JetAbsent),
            });
        __jet_runtime_config = Some(__RUNTIME_CONFIG__(&__jet_request.__REQUEST_EVAL_CONFIG__));
        let _activation = __jet_resource_session.activate();
        // D-EXEC1: the compiler factory runs only as the linked Rust AOT root.
        let crate::BootstrapFactoryTier::Aot = __jet_selected_tier;
        let __jet_result = match __jet_image_entry {
            ::jet_foundation::MIR::MirFunctionId(__ENTRY_ID__) => __FACTORY_SYMBOL__(__jet_request),
            _ => return Err("restored compiler image selected an unknown generated factory entry".to_string()),
        };
        drop(_activation);
        drop(__jet_resource_session);
        Ok(__JetBootstrapCompileLease {
            result: __jet_result,
            resources: __jet_resources.take()
                .ok_or_else(|| "Source resource session was consumed before factory completion".to_string())?,
            runtime_config: __jet_runtime_config.take()
                .ok_or_else(|| "Source runtime config was consumed before factory completion".to_string())?,
            selected_factory_tier: __jet_selected_tier,
            actual_factory_tier: crate::BootstrapFactoryTier::Aot,
        })
    })();

    match __jet_result {
        Ok(result) => Ok(result),
        Err(error) => {
            let cause = crate::BootstrapHostCodecError::InvalidMetadata(error);
            let retirement = match __jet_resources.take() {
                Some(resources) => resources.retire(),
                None => Ok(Vec::new()),
            };
            drop(__jet_runtime_config);
            match retirement {
                Ok(completions) => {
                    for completion in completions {
                        __jet_completion_scope.record(completion);
                    }
                    Err(cause)
                }
                Err(retirement) => Err(crate::BootstrapHostCodecError::ResourceRetirement {
                    cause: Box::new(cause),
                    retirement,
                }),
            }
        }
    }
    })
}
"#;
    let generated = generated
        .replace("__COMPILE_RESULT__", &entry.return_type)
        .replace("__EVAL_CONFIG__", eval_config)
        .replace("__COMPILE_REQUEST__", &entry.parameter_types[0])
        .replace("__ENTRY_ID__", &entry_id)
        .replace("__CALLBACK_TRANSFER_ID__", &callback_transfer)
        .replace("__HOST_TYPE_SHAPE_ID__", &host_type_shape)
        .replace("__HOST_TYPE_SHAPE_SYMBOL__", &shape_callable.metadata.symbol)
        .replace("__HOST_TYPE_SHAPE_ARGUMENTS__", &shape_arguments)
        .replace("__HOST_FIELD_OWNER__", &host_field.owner.0.to_string())
        .replace("__HOST_FIELD_ID__", &host_field.field.0.to_string())
        .replace("__NUMERIC_FIELD_OWNER__", &numeric_field.owner.0.to_string())
        .replace("__NUMERIC_FIELD_ID__", &numeric_field.field.0.to_string())
        .replace("__REQUEST_EVAL_CONFIG__", request_eval_config)
        .replace("__REQUEST_HOST_ADAPTER__", request_host_adapter)
        .replace("__REQUEST_HOST_HOOKS__", request_host_hooks)
        .replace("__HOOKS_TYPE__", host_hooks_type)
        .replace("__HOOK_NUMERIC__", hook_numeric)
        .replace("__HOOK_BOUNDARY__", hook_boundary)
        .replace("__RUNTIME_CONFIG__", runtime_config_symbol)
        .replace("__FACTORY_SYMBOL__", &entry.symbol);
    out.push_str(&generated);
    Ok(())
}

/// Emit the `JetDriverRecordStore` carrier over the host's
/// `BootstrapRecordStore`, plus `__jet_bootstrap_record_store()`, which yields
/// the request's optional store (absent when the machine store cannot open).
/// The impl follows the checked trait's own method list.
fn emit_bootstrap_record_store(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let trait_symbol = symbols.trait_symbol("JetDriverRecordStore")?;
    out.push_str(
        "#[doc(hidden)]\n\
         #[derive(Clone)]\n\
         struct __JetBootstrapRecordStore {\n\
             store: crate::compiler_bootstrap_host::BootstrapRecordStore,\n\
         }\n",
    );
    writeln!(out, "impl {trait_symbol} for __JetBootstrapRecordStore {{")
        .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    for metadata in symbols.trait_methods("JetDriverRecordStore")? {
        let receiver = match metadata.receiver_access {
            Some(jet_foundation::MIR::MirAccess::Read) => "&self",
            Some(jet_foundation::MIR::MirAccess::Write) => "&mut self",
            Some(jet_foundation::MIR::MirAccess::Move) | None => {
                return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                    "JetDriverRecordStore.{} has no borrowed (read or write) checked receiver",
                    metadata.name
                )))
            }
        };
        let (arity, body) = match metadata.name.as_str() {
            "record" => (2, "match self.store.record(&__jet_arg_0, &__jet_arg_1) { Some(bytes) => Ok(bytes), None => Err(Default::default()) }"),
            // The checked Bool return is emitted fallible (`JetOutcome<bool, JetErr>`).
            "put_record" => (3, "Ok(self.store.put_record(&__jet_arg_0, &__jet_arg_1, &__jet_arg_2))"),
            "stamp" => (1, "match self.store.stamp(&__jet_arg_0) { Some(stamp) => Ok(stamp), None => Err(Default::default()) }"),
            name => {
                return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                    "JetDriverRecordStore.{name} has no host implementation"
                )))
            }
        };
        if metadata.parameter_types.len() != arity {
            return Err(BootstrapHostCodecError::InvalidMetadata(format!(
                "JetDriverRecordStore.{} does not take {arity} checked arguments",
                metadata.name
            )));
        }
        let parameters = metadata
            .parameter_types
            .iter()
            .enumerate()
            .map(|(index, ty)| format!(", __jet_arg_{index}: {ty}"))
            .collect::<String>();
        writeln!(
            out,
            "    fn {}({receiver}{parameters}) -> {} {{ {body} }}",
            metadata.symbol, metadata.return_type,
        )
        .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    }
    writeln!(
        out,
        "}}\n\
         #[doc(hidden)]\n\
         fn __jet_bootstrap_record_store() -> crate::JetOutcome<Box<dyn {trait_symbol}>, crate::JetAbsent> {{\n\
             match crate::compiler_bootstrap_host::BootstrapRecordStore::from_env() {{\n\
                 Some(store) => Ok(Box::new(__JetBootstrapRecordStore {{ store }})),\n\
                 None => Err(Default::default()),\n\
             }}\n\
         }}"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn emit_bootstrap_host_factory(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
    default_symbol: &str,
    _runtime_config_symbol: &str,
) -> Result<(), BootstrapHostCodecError> {
    emit_bootstrap_record_store(out, symbols)?;
    let request_type = symbols.type_symbol("JetDriverCompileRequest")?;
    let request_sources = symbols.field_symbol("JetDriverCompileRequest", "authorized_sources")?;
    let request_host_facts = symbols.field_symbol("JetDriverCompileRequest", "host_facts")?;
    let request_eval_config = symbols.field_symbol("JetDriverCompileRequest", "eval_config")?;
    let request_target = symbols.field_symbol("JetDriverCompileRequest", "target")?;
    let request_effect_source = symbols.field_symbol("JetDriverCompileRequest", "canonical_effect_source")?;
    let request_record_store = symbols.field_symbol("JetDriverCompileRequest", "record_store")?;
    let request_mir_lint_every_pass =
        symbols.field_symbol("JetDriverCompileRequest", "mir_lint_every_pass")?;
    let request_core_sources = symbols.field_symbol("JetDriverCompileRequest", "core_sources")?;
    let core_source_type = symbols.type_symbol("JetDriverCoreSource")?;
    let core_source_module_name = symbols.field_symbol("JetDriverCoreSource", "module_name")?;
    let core_source_alias = symbols.field_symbol("JetDriverCoreSource", "alias")?;
    let core_source_path = symbols.field_symbol("JetDriverCoreSource", "path")?;
    let core_source_owner = symbols.field_symbol("JetDriverCoreSource", "owner")?;
    let core_source_owned_members = symbols.field_symbol("JetDriverCoreSource", "owned_members")?;
    let core_source_text = symbols.field_symbol("JetDriverCoreSource", "source")?;
    let target_type = symbols.type_symbol("JetDriverCompileTarget")?;
    let mir_program_type = symbols.type_symbol("MIRProgram")?;
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
            "    // The Core library bodies the Rust loader embeds: every public source\n",
            "    // module row plus the compiler-private parts, in table order.\n",
            "    let mut __jet_core_sources = Vec::new();\n",
            "    for (row, owner) in ::jet_foundation::CoreModuleExports::core_source_modules().iter().map(|row| (row, \"\"))\n",
            "        .chain(::jet_foundation::CoreSourceParts::CORE_PRIVATE_SOURCE_PARTS.iter().map(|part| (&part.source, part.owner)))\n",
            "    {{\n",
            "        let text = ::jet_driver::CoreSources::core_source_text(row.module).ok_or_else(|| {{\n",
            "            crate::BootstrapHostCodecError::InvalidMetadata(format!(\"Core source module `{{}}` has no body text\", row.module))\n",
            "        }})?;\n",
            "        __jet_core_sources.push({core_source_type} {{\n",
            "            {core_source_module_name}: row.module.to_string(),\n",
            "            {core_source_alias}: row.alias.to_string(),\n",
            "            {core_source_path}: row.path.to_string(),\n",
            "            {core_source_owner}: owner.to_string(),\n",
            "            {core_source_owned_members}: row.owned_members.iter().map(|member| member.to_string()).collect::<Vec<_>>(),\n",
            "            {core_source_text}: text.to_string(),\n",
            "        }});\n",
            "    }}\n",
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
            "            {host_compiler_identity}: option_env!(\"JET_COMPILER_SOURCE_ID\").map(|value| Ok(format!(\"{{}}@{{}}#{{}}\", ::jet_foundation::Syntax::BINARY_NAME, ::jet_pkg_model::Manifest::COMPILER_VERSION, value))).unwrap_or_else(|| Err(Default::default())),\n",
            "        }},\n",
            "        {request_effect_source}: ::jet_foundation::Effects::EFFECT_SOURCE.to_string(),\n",
            "        {request_core_sources}: __jet_core_sources,\n",
            "        {request_eval_config}: __jet_eval_config,\n",
            "        {request_target}: __jet_target,\n",
            "        {request_record_store}: __jet_bootstrap_record_store(),\n",
            "        // Full verification (MIR Lint after every optimizer pass) is an\n",
            "        // explicit host opt-in for tests and verification runs.\n",
            "        {request_mir_lint_every_pass}: ::std::env::var_os(\"JET_BOOTSTRAP_MIR_LINT_EVERY_PASS\").is_some_and(|value| value == \"1\"),\n",
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
        request_record_store = request_record_store,
        request_mir_lint_every_pass = request_mir_lint_every_pass,
        request_core_sources = request_core_sources,
        core_source_type = core_source_type,
        core_source_module_name = core_source_module_name,
        core_source_alias = core_source_alias,
        core_source_path = core_source_path,
        core_source_owner = core_source_owner,
        core_source_owned_members = core_source_owned_members,
        core_source_text = core_source_text,
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
            "    __jet_selected_tier: crate::BootstrapFactoryTier,\n",
            "    __jet_execution: &crate::Codegen::MIRRust::MirRustExecutionConfig,\n",
            "    __jet_completion_owner: &mut crate::compiler_bootstrap_runner::BootstrapRunCompletionOwner,\n",
            ") -> Result<crate::BootstrapJetCompileResult<{mir_program_type}, {eval_config_type}>, crate::BootstrapHostCodecError> {{\n",
            "    let __jet_request = __jet_bootstrap_request_from_host(__jet_snapshot, __jet_requested_target)?;\n",
            "    let __jet_completion_scope = __jet_completion_owner.completion_scope();\n",
            "    let __jet_result = __jet_bootstrap_compile_with_native(__jet_request, __jet_selected_tier, __jet_execution, __jet_completion_owner)?;\n",
            "    let mut __jet_runtime_config = Some(__jet_result.runtime_config);\n",
            "    let __jet_output = __jet_completion_scope.with_current(|| {{\n",
            "        let _activation = __jet_result.resources.activate();\n",
            "        __jet_bootstrap_bindings_from_result(__jet_result.result, __jet_snapshot, &mut __jet_runtime_config, __jet_result.selected_factory_tier, __jet_result.actual_factory_tier)\n",
            "    }});\n",
            "    match __jet_output {{\n",
            "        Ok(mut output) => {{ output.resources = Some(__jet_result.resources); Ok(output) }},\n",
            "        Err(error) => {{ let error = crate::BootstrapHostCodecError::InvalidMetadata(error); match __jet_completion_scope.with_current(|| __jet_result.resources.retire()) {{\n",
            "            Ok(completions) => {{\n",
            "                for completion in completions {{ __jet_completion_scope.record(completion); }}\n",
            "                Err(error)\n",
            "            }},\n",
            "            Err(retirement) => Err(crate::BootstrapHostCodecError::ResourceRetirement {{ cause: Box::new(error), retirement }}),\n",
            "        }} }},\n",
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


/// Rust argument expressions for one call to an emitted callable: each
/// `(place, owned)` value is passed the way its checked parameter is emitted:
/// `&mut place`, `&place`, or the `owned` expression (a move of the place, or
/// a clone/fork when the caller keeps it).
fn bootstrap_call_arguments(parameter_types: &[String], values: &[(&str, &str)]) -> String {
    parameter_types
        .iter()
        .zip(values)
        .map(|(ty, (place, owned))| {
            if ty.starts_with("&mut ") {
                format!("&mut {place}")
            } else if ty.starts_with('&') {
                format!("&{place}")
            } else {
                (*owned).to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn checked_task_roots_callable<'a>(
    bindings: &'a BootstrapBindingDescriptor,
    name: &str,
    arity: usize,
    returns_bool: bool,
) -> Result<&'a MirRustCallableMetadata, BootstrapHostCodecError> {
    let callable = bootstrap_required_callable(bindings, name)?;
    if callable.parameter_types.len() != arity || (returns_bool && callable.return_type != "bool") {
        return Err(BootstrapHostCodecError::InvalidMetadata(format!(
            "task-root fixture `{name}` does not have its checked {arity}-parameter signature"
        )));
    }
    Ok(callable)
}

/// Entry points of the task-root fixture (Compiler/JetEval/Tests/TaskRoots.jet)
/// for the harness `main`'s `task-roots` mode. The fixture is part of the
/// compiler unit only when the harness appends it, so the wrappers are emitted
/// only when its checked callables are bound. Every Source name resolves
/// through the binding rows (the emitted symbol and parameter passing differ
/// between the reference Rust emitter and the Jet emitter); host arguments
/// convert through the generated Source-MIR codec.
fn emit_bootstrap_task_roots_fixture(
    out: &mut String,
    bindings: &BootstrapBindingDescriptor,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    if !bindings
        .callables
        .iter()
        .any(|callable| callable.source_name == "jet_eval_task_roots_fixture")
    {
        return Ok(());
    }
    let program = symbols.type_symbol("MIRProgram")?;
    let config = symbols.type_symbol("JetEvalConfig")?;
    let span = symbols.type_symbol("Span")?;
    let carrier = symbols.type_symbol("JetEvalSharedHostCarrier")?;
    let fixture = checked_task_roots_callable(bindings, "jet_eval_task_roots_fixture", 6, true)?;
    let produce =
        checked_task_roots_callable(bindings, "jet_eval_task_roots_fixture_shared_produce", 3, false)?;
    let consume =
        checked_task_roots_callable(bindings, "jet_eval_task_roots_fixture_shared_consume", 4, true)?;
    // The harness keeps its program and config across the fixture calls: an
    // owned program parameter takes a clone, an owned config a fork (the
    // config's host adapter is not `Clone`), as Source code forks it.
    let fork = bootstrap_required_callable(bindings, "jet_eval_runtime_config")?;
    let config_fork = format!("{}(&*__jet_config)", fork.symbol);
    let program_place = ("(*__jet_program)", "(*__jet_program).clone()");
    let config_place = ("(*__jet_config)", config_fork.as_str());
    let fixture_arguments = bootstrap_call_arguments(
        &fixture.parameter_types,
        &[
            program_place,
            config_place,
            ("__jet_callback", "__jet_callback"),
            ("__jet_resource_handle", "__jet_resource_handle"),
            ("__jet_resource_raw", "__jet_resource_raw"),
            ("__jet_span", "__jet_span"),
        ],
    );
    let produce_arguments = bootstrap_call_arguments(
        &produce.parameter_types,
        &[program_place, config_place, ("__jet_span", "__jet_span")],
    );
    // `T?` returns are emitted as an outcome carrier; the harness takes an Option.
    let produce_present = if produce.return_type.starts_with("Option<") { "" } else { ".ok()" };
    let consume_arguments = bootstrap_call_arguments(
        &consume.parameter_types,
        &[program_place, config_place, ("__jet_carrier", "__jet_carrier"), ("__jet_span", "__jet_span")],
    );
    writeln!(
        out,
        "#[doc(hidden)]\n\
         fn __jet_bootstrap_task_roots_span() -> Result<{span}, String> {{\n\
         \x20   __jet_bootstrap_span_from_host(&::jet_foundation::Diagnostics::Span {{ start: 0, end: 0 }})\n\
         }}\n\
         #[doc(hidden)]\n\
         #[allow(unused_mut)]\n\
         pub(crate) fn __jet_bootstrap_task_roots_fixture(\n\
         \x20   __jet_program: &mut {program},\n\
         \x20   __jet_config: &mut {config},\n\
         \x20   __jet_callback: &::jet_foundation::MIR::MirFunctionId,\n\
         \x20   __jet_resource_handle: &::jet_foundation::MIR::MirHandleId,\n\
         \x20   __jet_resource_raw: i64,\n\
         ) -> Result<bool, String> {{\n\
         \x20   let mut __jet_callback = __jet_bootstrap_mir_MIRFunctionID_from_host(__jet_callback)?;\n\
         \x20   let mut __jet_resource_handle = __jet_bootstrap_mir_MIRHandleID_from_host(__jet_resource_handle)?;\n\
         \x20   let mut __jet_resource_raw = jet_foundation::Numeric::JetInt::from_i64(__jet_resource_raw);\n\
         \x20   let mut __jet_span = __jet_bootstrap_task_roots_span()?;\n\
         \x20   Ok({fixture_symbol}({fixture_arguments}))\n\
         }}\n\
         #[doc(hidden)]\n\
         #[allow(unused_mut)]\n\
         pub(crate) fn __jet_bootstrap_task_roots_fixture_shared_produce(\n\
         \x20   __jet_program: &mut {program},\n\
         \x20   __jet_config: &mut {config},\n\
         ) -> Result<Option<{carrier}>, String> {{\n\
         \x20   let mut __jet_span = __jet_bootstrap_task_roots_span()?;\n\
         \x20   Ok({produce_symbol}({produce_arguments}){produce_present})\n\
         }}\n\
         #[doc(hidden)]\n\
         #[allow(unused_mut)]\n\
         pub(crate) fn __jet_bootstrap_task_roots_fixture_shared_consume(\n\
         \x20   __jet_program: &mut {program},\n\
         \x20   __jet_config: &mut {config},\n\
         \x20   mut __jet_carrier: {carrier},\n\
         ) -> Result<bool, String> {{\n\
         \x20   let mut __jet_span = __jet_bootstrap_task_roots_span()?;\n\
         \x20   Ok({consume_symbol}({consume_arguments}))\n\
         }}",
        fixture_symbol = fixture.symbol,
        produce_symbol = produce.symbol,
        consume_symbol = consume.symbol,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))
}

fn emit_bootstrap_web_asset_provider(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let target_type = symbols.type_symbol("JetDriverCompileTarget")?;
    let target_native = symbols.variant_path("JetDriverCompileTarget", "Native")?;
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
    ]
    .iter()
    .map(|name| Ok::<_, BootstrapHostCodecError>((*name, symbols.field_symbol("JetWebAssets", name)?)))
    .collect::<Result<Vec<_>, _>>()?;
    let field = |name: &str| {
        asset_fields
            .iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|(_, symbol)| *symbol)
            .ok_or_else(|| BootstrapHostCodecError::MissingEntry(name.to_string()))
    };
    writeln!(
        out,
        r#"fn __jet_bootstrap_target_with_raw_web_assets(
            target: {target_type},
        ) -> Result<{target_type}, String> {{
            match target {{
                {target_native} => Ok({target_native}),
                @p.JetDriverCompileTarget.Web@(assets, release_policy, profile) => {{
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
                    }};
                    Ok(@new.JetDriverCompileTarget.Web@(assets, release_policy, profile))
                }}
            }}
        }}"#,
        target_type = target_type,
        target_native = target_native,
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
    let mir_program_type = symbols.type_symbol("MIRProgram")?;
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
    let function_value = symbols.field_symbol("MIRFunctionID", "value")?;
    let type_value = symbols.field_symbol("MIRTypeID", "value")?;
    let artifact_value = symbols.field_symbol("MIRArtifactID", "value")?;
    let field_value = symbols.field_symbol("MIRFieldID", "value")?;
    let access_read = symbols.variant_path("MIRAccess", "Read")?;
    let access_write = symbols.variant_path("MIRAccess", "Write")?;
    let access_move = symbols.variant_path("MIRAccess", "Move")?;
    let trait_id_value = symbols.field_symbol("MIRTraitID", "value")?;
    let trait_method_id_value = symbols.field_symbol("MIRTraitMethodID", "value")?;

    writeln!(
        out,
        concat!(
            "#[doc(hidden)]\n",
            "pub(crate) fn __jet_bootstrap_bindings_from_result(\n",
            "    {value}: {result_type},\n",
            "    snapshot: &crate::compiler_bootstrap_host::AuthorizedSourceSnapshot,\n",
            "    runtime_config: &mut Option<{eval_config_type}>,\n",
            "    selected_factory_tier: crate::BootstrapFactoryTier,\n",
            "    actual_factory_tier: crate::BootstrapFactoryTier,\n",
            ") -> Result<crate::BootstrapJetCompileResult<{mir_program_type}, {eval_config_type}>, String> {{\n",
            "    let internal_problem = ({value}).{result_internal_problem}.as_ref().ok().cloned();\n",
            "    let complete = ({value}).{result_complete};\n",
            "    let comptime_stdout = ({value}).{result_comptime_stdout}.clone();\n",
            "    let comptime_stderr = ({value}).{result_comptime_stderr}.clone();\n",
            "    let soft_stop = ({value}).{result_soft_stop};\n",
            "    let exit_code = ({value}).{result_exit_code}.as_ref().ok().map(|code| code.to_string_rep().parse::<i64>().map_err(|_| \"Jet compile exit code does not fit i64\".to_string())).transpose()?;\n",
            "    let reports = __jet_bootstrap_reports_from_result(&{value}, snapshot)?;\n",
            "    let source_program = ({value}).{result_mir}.ok();\n",
            "    let mir = source_program.as_ref().map(__jet_bootstrap_mir_program_to_host).transpose()?;\n",
            "    let entry_function = ({value}).{result_entry_function}.as_ref().ok().map(|function| Ok::<_, String>(::jet_foundation::MIR::MirFunctionId(function.{function_value}.to_string_rep().parse::<u64>().map_err(|_| \"Jet entry function ID is not an unsigned integer\".to_string())?))).transpose()?;\n",
            "    let runtime_artifact = ({value}).{result_runtime_artifact}.as_ref().ok().map(|artifact| Ok::<_, String>(::jet_foundation::MIR::MirArtifactId(artifact.{artifact_value}.to_string_rep().parse::<u64>().map_err(|_| \"Jet runtime artifact ID is not an unsigned integer\".to_string())?))).transpose()?;\n",
            "    let web_artifact = ({value}).{result_web_artifact}.as_ref().ok().map(|artifact| Ok::<_, String>(::jet_foundation::MIR::MirArtifactId(artifact.{artifact_value}.to_string_rep().parse::<u64>().map_err(|_| \"Jet Web artifact ID is not an unsigned integer\".to_string())?))).transpose()?;\n",
            "    let web_artifacts = ({value}).{result_web_artifacts}.as_ref().ok().map(|artifacts| crate::BootstrapWebArtifacts {{\n",
            "        manifest_json: artifacts.{web_manifest_json}.clone(),\n",
            "        wasm_rust: artifacts.{web_wasm_rust}.clone(),\n",
            "        rustc_incremental_identity: artifacts.{web_incremental_identity}.clone(),\n",
            "        js_app: artifacts.{web_js_app}.clone(),\n",
            "        js_source_map: artifacts.{web_js_source_map}.clone(),\n",
            "        source_names: artifacts.{web_source_names}.clone(),\n",
            "        source_contents: artifacts.{web_source_contents}.clone(),\n",
            "        dom_runtime: artifacts.{web_dom_runtime}.clone(),\n",
            "        index_html: artifacts.{web_index_html}.clone(),\n",
            "        explicit_html_path: artifacts.{web_explicit_html_path}.as_ref().ok().cloned(),\n",
            "        command_record: artifacts.{web_command_record}.clone(),\n",
            "    }});\n",
            "    if internal_problem.is_some() || !complete {{\n",
            "        return Ok(crate::BootstrapJetCompileResult {{ selected_factory_tier, actual_factory_tier, complete, emitted_source: None, bindings: None, source_program, runtime_config: runtime_config.take(), mir, entry_function, runtime_artifact, web_artifact, web_artifacts, comptime_stdout, comptime_stderr, soft_stop, exit_code, internal_problem, reports, resources: None }});\n",
            "    }}\n",
            "    if runtime_artifact.is_some() {{\n",
            "        if mir.is_none() || entry_function.is_none() {{ return Err(\"complete Source runtime result has no typed MIR or entry function\".to_string()); }}\n",
            "        return Ok(crate::BootstrapJetCompileResult {{ selected_factory_tier, actual_factory_tier, complete, emitted_source: None, bindings: None, source_program, runtime_config: runtime_config.take(), mir, entry_function, runtime_artifact, web_artifact, web_artifacts, comptime_stdout, comptime_stderr, soft_stop, exit_code, internal_problem, reports, resources: None }});\n",
            "    }}\n",
            "    if web_artifact.is_some() || web_artifacts.is_some() {{\n",
            "        if web_artifact.is_none() || web_artifacts.is_none() || mir.is_none() || entry_function.is_none() {{ return Err(\"complete Web result has no exact artifact, artifacts, MIR, or entry function\".to_string()); }}\n",
            "        return Ok(crate::BootstrapJetCompileResult {{ selected_factory_tier, actual_factory_tier, complete, emitted_source: None, bindings: None, source_program, runtime_config: runtime_config.take(), mir, entry_function, runtime_artifact, web_artifact, web_artifacts, comptime_stdout, comptime_stderr, soft_stop, exit_code, internal_problem, reports, resources: None }});\n",
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
            "        let ty = mir.as_ref().and_then(|program| program.fields.iter().find(|candidate| candidate.id == field_id && candidate.owner == owner_id)).map(|candidate| candidate.field.ty.clone()).ok_or_else(|| format!(\"manifest field {{:?}}/{{:?}} is absent from converted MIR\", field_id, owner_id))?;\n",
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
            "    let source = ({value}).{result_source}.as_ref().ok().cloned().ok_or_else(|| \"complete bootstrap compiler result has no emitted Rust source\".to_string())?;\n",
            "    Ok(crate::BootstrapJetCompileResult {{ selected_factory_tier, actual_factory_tier, complete: true, emitted_source: Some(source), bindings: Some(bindings), source_program, runtime_config: runtime_config.take(), mir, entry_function, runtime_artifact, web_artifact: None, web_artifacts: None, comptime_stdout, comptime_stderr, soft_stop, exit_code, internal_problem, reports, resources: None }})\n",
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

/// The Source `MIRType` graph codec. Payload shapes and boxed edges come
/// from the binding metadata through the `@…@` markers.
fn emit_bootstrap_type_codec(out: &mut String) {
    out.push_str(
        r#"
fn __jet_bootstrap_type_id_to_host(value: &@t.MIRTypeID@) -> Result<::jet_foundation::MIR::MirTypeId, String> {
    let text = value.@f.MIRTypeID.value@.to_string_rep();
    let id = text.parse::<u64>().map_err(|_| "MIR type identity is not an unsigned integer".to_string())?;
    if id == 0 {
        return Err("MIR type identity is zero".to_string());
    }
    Ok(::jet_foundation::MIR::MirTypeId(id))
}
fn __jet_bootstrap_type_id_from_host(value: ::jet_foundation::MIR::MirTypeId) -> Result<@t.MIRTypeID@, String> {
    let value = jet_foundation::Numeric::JetInt::from_big(jet_foundation::Numeric::CtBigInt::from_u64(value.0));
    Ok(@s.MIRTypeID@{ value })
}
fn __jet_bootstrap_measure_to_host(value: &@t.MIRMeasure@) -> Result<::jet_foundation::MIR::MirMeasure, String> {
    match value {
        @p.MIRMeasure.Literal@{ kind, value } => Ok(::jet_foundation::MIR::MirMeasure::Literal {
            kind: kind.clone(),
            value: value.to_string_rep().parse::<u64>().map_err(|_| "measure literal is not an unsigned integer".to_string())?,
        }),
        @p.MIRMeasure.SignedLiteral@{ kind, value } => Ok(::jet_foundation::MIR::MirMeasure::SignedLiteral {
            kind: kind.clone(),
            value: value.to_i64().ok_or_else(|| "signed measure literal exceeds i64".to_string())?,
        }),
        @p.MIRMeasure.Symbol@{ kind, name } => Ok(::jet_foundation::MIR::MirMeasure::Symbol {
            kind: kind.clone(),
            name: name.clone(),
        }),
        @p.MIRMeasure.Combined@{ kind, rule, left, right } => {
            let rule = match rule {
                @v.MIRMeasureRule.Add@ => ::jet_foundation::MIR::MirMeasureRule::Add,
                @v.MIRMeasureRule.Mul@ => ::jet_foundation::MIR::MirMeasureRule::Mul,
                @v.MIRMeasureRule.Match@ => ::jet_foundation::MIR::MirMeasureRule::Match,
            };
            Ok(::jet_foundation::MIR::MirMeasure::Combined {
                kind: kind.clone(),
                rule,
                left: Box::new(__jet_bootstrap_measure_to_host(left)?),
                right: Box::new(__jet_bootstrap_measure_to_host(right)?),
            })
        }
    }
}
fn __jet_bootstrap_measure_from_host(value: &::jet_foundation::MIR::MirMeasure) -> Result<@t.MIRMeasure@, String> {
    match value {
        ::jet_foundation::MIR::MirMeasure::Literal { kind, value } => Ok(@new.MIRMeasure.Literal@{
            kind: kind.clone(),
            value: jet_foundation::Numeric::JetInt::from_big(jet_foundation::Numeric::CtBigInt::from_u64(*value)),
        }),
        ::jet_foundation::MIR::MirMeasure::SignedLiteral { kind, value } => Ok(@new.MIRMeasure.SignedLiteral@{
            kind: kind.clone(),
            value: jet_foundation::Numeric::JetInt::from_i64(*value),
        }),
        ::jet_foundation::MIR::MirMeasure::Symbol { kind, name } => Ok(@new.MIRMeasure.Symbol@{ kind: kind.clone(), name: name.clone() }),
        ::jet_foundation::MIR::MirMeasure::Combined { kind, rule, left, right } => {
            let rule = match rule {
                ::jet_foundation::MIR::MirMeasureRule::Add => @v.MIRMeasureRule.Add@,
                ::jet_foundation::MIR::MirMeasureRule::Mul => @v.MIRMeasureRule.Mul@,
                ::jet_foundation::MIR::MirMeasureRule::Match => @v.MIRMeasureRule.Match@,
            };
            Ok(@new.MIRMeasure.Combined@{
                kind: kind.clone(),
                rule,
                left: __jet_bootstrap_measure_from_host(left)?,
                right: __jet_bootstrap_measure_from_host(right)?,
            })
        }
    }
}
fn __jet_bootstrap_dimension_to_host(value: &@t.MIRDimension@) -> Result<::jet_foundation::MIR::MirDimension, String> {
    let mut axes = ::std::collections::BTreeMap::new();
    for axis in value.@f.MIRDimension.axes@.iter() {
        let name = axis.@f.MIRDimensionAxis.name@.clone();
        let exponent = __jet_bootstrap_measure_to_host(&axis.@f.MIRDimensionAxis.exponent@)?;
        if axes.insert(name, exponent).is_some() {
            return Err("MIR dimension contains a duplicate axis name".to_string());
        }
    }
    Ok(::jet_foundation::MIR::MirDimension { axes })
}
fn __jet_bootstrap_dimension_from_host(value: &::jet_foundation::MIR::MirDimension) -> Result<@t.MIRDimension@, String> {
    let mut axes = Vec::with_capacity(value.axes.len());
    for (name, exponent) in &value.axes {
        axes.push(@s.MIRDimensionAxis@{
            name: name.clone(),
            exponent: __jet_bootstrap_measure_from_host(exponent)?,
        });
    }
    Ok(@s.MIRDimension@{ axes })
}
fn __jet_bootstrap_tag_to_host(value: &@t.MIRTagMarker@) -> ::jet_foundation::MIR::MirTagMarker {
    match value {
        @p.MIRTagMarker.User@{ name } => ::jet_foundation::MIR::MirTagMarker::User(name.clone()),
        @p.MIRTagMarker.Internal@{ internal_tag } => ::jet_foundation::MIR::MirTagMarker::Internal(match &*@deref.MIRTagMarker.Internal.internal_tag@internal_tag {
            @v.MIRInternalTag.CoreCryptoNominal@ => ::jet_foundation::MIR::MirInternalTag::CoreCryptoNominal,
            @v.MIRInternalTag.DeterministicClock@ => ::jet_foundation::MIR::MirInternalTag::DeterministicClock,
            @v.MIRInternalTag.SystemClock@ => ::jet_foundation::MIR::MirInternalTag::SystemClock,
            @v.MIRInternalTag.ExpiringSecretLoan@ => ::jet_foundation::MIR::MirInternalTag::ExpiringSecretLoan,
            @v.MIRInternalTag.SharedGuardRead@ => ::jet_foundation::MIR::MirInternalTag::SharedGuardRead,
            @v.MIRInternalTag.SharedGuardEdit@ => ::jet_foundation::MIR::MirInternalTag::SharedGuardEdit,
            @v.MIRInternalTag.TerminalFactSet@ => ::jet_foundation::MIR::MirInternalTag::TerminalFactSet,
            @v.MIRInternalTag.CppCallbackABI@ => ::jet_foundation::MIR::MirInternalTag::CppCallbackAbi,
            @v.MIRInternalTag.AllocatorView@ => ::jet_foundation::MIR::MirInternalTag::AllocatorView,
        }),
    }
}
fn __jet_bootstrap_tag_from_host(value: &::jet_foundation::MIR::MirTagMarker) -> Result<@t.MIRTagMarker@, String> {
    match value {
        ::jet_foundation::MIR::MirTagMarker::User(name) => Ok(@new.MIRTagMarker.User@{ name: name.clone() }),
        ::jet_foundation::MIR::MirTagMarker::Internal(tag) => Ok(@new.MIRTagMarker.Internal@{ internal_tag: match tag {
            ::jet_foundation::MIR::MirInternalTag::CoreCryptoNominal => @v.MIRInternalTag.CoreCryptoNominal@,
            ::jet_foundation::MIR::MirInternalTag::DeterministicClock => @v.MIRInternalTag.DeterministicClock@,
            ::jet_foundation::MIR::MirInternalTag::SystemClock => @v.MIRInternalTag.SystemClock@,
            ::jet_foundation::MIR::MirInternalTag::ExpiringSecretLoan => @v.MIRInternalTag.ExpiringSecretLoan@,
            ::jet_foundation::MIR::MirInternalTag::SharedGuardRead => @v.MIRInternalTag.SharedGuardRead@,
            ::jet_foundation::MIR::MirInternalTag::SharedGuardEdit => @v.MIRInternalTag.SharedGuardEdit@,
            ::jet_foundation::MIR::MirInternalTag::TerminalFactSet => @v.MIRInternalTag.TerminalFactSet@,
            ::jet_foundation::MIR::MirInternalTag::CppCallbackAbi => @v.MIRInternalTag.CppCallbackABI@,
            ::jet_foundation::MIR::MirInternalTag::AllocatorView => @v.MIRInternalTag.AllocatorView@,
        } }),
    }
}
fn __jet_bootstrap_abi_to_host(value: &@t.MIRABI@) -> ::jet_foundation::MIR::MirAbi {
    match value {
        @p.MIRABI.Scalar@{ kind } => ::jet_foundation::MIR::MirAbi::Scalar(match &*@deref.MIRABI.Scalar.kind@kind {
            @v.MIRScalarKind.Int@ => ::jet_foundation::MIR::MirScalarKind::Int,
            @v.MIRScalarKind.Float@ => ::jet_foundation::MIR::MirScalarKind::Float,
            @v.MIRScalarKind.Float32@ => ::jet_foundation::MIR::MirScalarKind::Float32,
            @v.MIRScalarKind.Bool@ => ::jet_foundation::MIR::MirScalarKind::Bool,
            @v.MIRScalarKind.Char@ => ::jet_foundation::MIR::MirScalarKind::Char,
            @v.MIRScalarKind.Pointer@ => ::jet_foundation::MIR::MirScalarKind::Pointer,
        }),
        @v.MIRABI.Aggregate@ => ::jet_foundation::MIR::MirAbi::Aggregate,
        @v.MIRABI.Sequence@ => ::jet_foundation::MIR::MirAbi::Sequence,
        @v.MIRABI.Function@ => ::jet_foundation::MIR::MirAbi::Function,
        @v.MIRABI.Nominal@ => ::jet_foundation::MIR::MirAbi::Nominal,
        @v.MIRABI.Dynamic@ => ::jet_foundation::MIR::MirAbi::Dynamic,
        @v.MIRABI.Never@ => ::jet_foundation::MIR::MirAbi::Never,
    }
}
fn __jet_bootstrap_abi_from_host(value: &::jet_foundation::MIR::MirAbi) -> @t.MIRABI@ {
    match value {
        ::jet_foundation::MIR::MirAbi::Scalar(kind) => @new.MIRABI.Scalar@{ kind: match kind {
            ::jet_foundation::MIR::MirScalarKind::Int => @v.MIRScalarKind.Int@,
            ::jet_foundation::MIR::MirScalarKind::Float => @v.MIRScalarKind.Float@,
            ::jet_foundation::MIR::MirScalarKind::Float32 => @v.MIRScalarKind.Float32@,
            ::jet_foundation::MIR::MirScalarKind::Bool => @v.MIRScalarKind.Bool@,
            ::jet_foundation::MIR::MirScalarKind::Char => @v.MIRScalarKind.Char@,
            ::jet_foundation::MIR::MirScalarKind::Pointer => @v.MIRScalarKind.Pointer@,
        } },
        ::jet_foundation::MIR::MirAbi::Aggregate => @v.MIRABI.Aggregate@,
        ::jet_foundation::MIR::MirAbi::Sequence => @v.MIRABI.Sequence@,
        ::jet_foundation::MIR::MirAbi::Function => @v.MIRABI.Function@,
        ::jet_foundation::MIR::MirAbi::Nominal => @v.MIRABI.Nominal@,
        ::jet_foundation::MIR::MirAbi::Dynamic => @v.MIRABI.Dynamic@,
        ::jet_foundation::MIR::MirAbi::Never => @v.MIRABI.Never@,
    }
}
fn __jet_bootstrap_size_to_host(value: &@t.MIRSize@) -> Result<::jet_foundation::MIR::MirSize, String> {
    match value {
        @p.MIRSize.Static@{ bytes } => Ok(::jet_foundation::MIR::MirSize::Static(bytes.to_string_rep().parse::<u64>().map_err(|_| "layout size is not an unsigned integer".to_string())?)),
        @v.MIRSize.Dynamic@ => Ok(::jet_foundation::MIR::MirSize::Dynamic),
    }
}
fn __jet_bootstrap_size_from_host(value: &::jet_foundation::MIR::MirSize) -> Result<@t.MIRSize@, String> {
    match value {
        ::jet_foundation::MIR::MirSize::Static(bytes) => Ok(@new.MIRSize.Static@{ bytes: jet_foundation::Numeric::JetInt::from_big(jet_foundation::Numeric::CtBigInt::from_u64(*bytes)) }),
        ::jet_foundation::MIR::MirSize::Dynamic => Ok(@v.MIRSize.Dynamic@),
    }
}
fn __jet_bootstrap_layout_from_host(value: &::jet_foundation::MIR::MirLayout) -> Result<@t.MIRLayout@, String> {
    Ok(@s.MIRLayout@{
        abi: __jet_bootstrap_abi_from_host(&value.abi),
        size: __jet_bootstrap_size_from_host(&value.size)?,
        align: __jet_bootstrap_size_from_host(&value.align)?,
    })
}
fn __jet_bootstrap_type_to_host(value: &@t.MIRType@) -> Result<::jet_foundation::MIR::MirType, String> {
    let kind = match &@deref.MIRType.kind@value.@f.MIRType.kind@ {
        @v.MIRTypeKind.Int@ => ::jet_foundation::MIR::MirTypeKind::Int,
        @v.MIRTypeKind.Float@ => ::jet_foundation::MIR::MirTypeKind::Float,
        @v.MIRTypeKind.Bool@ => ::jet_foundation::MIR::MirTypeKind::Bool,
        @v.MIRTypeKind.String@ => ::jet_foundation::MIR::MirTypeKind::String,
        @v.MIRTypeKind.Char@ => ::jet_foundation::MIR::MirTypeKind::Char,
        @v.MIRTypeKind.Float32@ => ::jet_foundation::MIR::MirTypeKind::Float32,
        @p.MIRTypeKind.List@{ inner } => ::jet_foundation::MIR::MirTypeKind::List(Box::new(__jet_bootstrap_type_to_host(inner)?)),
        @p.MIRTypeKind.Map@{ key, value: item } => ::jet_foundation::MIR::MirTypeKind::Map {
            key: Box::new(__jet_bootstrap_type_to_host(key)?),
            value: Box::new(__jet_bootstrap_type_to_host(item)?),
        },
        @p.MIRTypeKind.Shared@{ inner } => ::jet_foundation::MIR::MirTypeKind::Shared(Box::new(__jet_bootstrap_type_to_host(inner)?)),
        @p.MIRTypeKind.Option@{ inner } => ::jet_foundation::MIR::MirTypeKind::Option(Box::new(__jet_bootstrap_type_to_host(inner)?)),
        @p.MIRTypeKind.Result@{ ok, err } => ::jet_foundation::MIR::MirTypeKind::Result {
            ok: Box::new(__jet_bootstrap_type_to_host(ok)?),
            err: Box::new(__jet_bootstrap_type_to_host(err)?),
        },
        @p.MIRTypeKind.Fn@{ signature } => ::jet_foundation::MIR::MirTypeKind::Fn(__jet_bootstrap_mir_MIRFunctionSignature_to_host(signature)?),
        @p.MIRTypeKind.SendFn@{ params, ret, conventions } => ::jet_foundation::MIR::MirTypeKind::SendFn {
            params: params.iter().map(__jet_bootstrap_type_to_host).collect::<Result<Vec<_>, _>>()?,
            ret: match (&*@deref.MIRTypeKind.SendFn.ret@ret).as_ref() {
                Ok(ret) => Some(Box::new(__jet_bootstrap_type_to_host(ret)?)),
                Err(_) => None,
            },
            conventions: conventions.iter().map(__jet_bootstrap_mir_MIRAccess_to_host).collect::<Result<Vec<_>, _>>()?,
        },
        @p.MIRTypeKind.Apply@{ name, args } => ::jet_foundation::MIR::MirTypeKind::Apply {
            name: ::jet_foundation::MIR::MirNominalRef {
                id: __jet_bootstrap_type_id_to_host(&name.@f.MIRNominalRef.id@)?,
                name: name.@f.MIRNominalRef.name@.clone(),
            },
            args: args.iter().map(__jet_bootstrap_type_to_host).collect::<Result<Vec<_>, _>>()?,
        },
        @p.MIRTypeKind.TraitObject@{ bounds } => ::jet_foundation::MIR::MirTypeKind::TraitObject(
            bounds.iter().map(|bound| {
                let id = __jet_bootstrap_source_u64(&bound.@f.MIRTraitRef.id@.@f.MIRTraitID.value@, "MIR trait identity")?;
                if id == 0 { return Err("MIR trait identity is zero".to_string()); }
                // Host trait-object bounds are nominal refs keyed by the trait's MIR identity.
                Ok(::jet_foundation::MIR::MirNominalRef { id: ::jet_foundation::MIR::MirTypeId(id), name: bound.@f.MIRTraitRef.name@.clone() })
            }).collect::<Result<Vec<_>, String>>()?,
        ),
        @p.MIRTypeKind.Tuple@{ fields } => ::jet_foundation::MIR::MirTypeKind::Tuple(
            fields.iter().map(|field| Ok((
                field.@f.MIRTupleField.name@.clone(),
                __jet_bootstrap_type_to_host(&field.@f.MIRTupleField.ty@)?,
            ))).collect::<Result<Vec<_>, String>>()?,
        ),
        @p.MIRTypeKind.FixedList@{ elem, len } => ::jet_foundation::MIR::MirTypeKind::FixedList {
            elem: Box::new(__jet_bootstrap_type_to_host(elem)?),
            len: __jet_bootstrap_measure_to_host(len)?,
        },
        @p.MIRTypeKind.IntN@{ signed, bits } => ::jet_foundation::MIR::MirTypeKind::IntN {
            signed: *signed,
            bits: bits.to_string_rep().parse::<u8>().map_err(|_| "integer width is outside u8".to_string())?,
        },
        @p.MIRTypeKind.InlineRange@{ base, lo, hi } => ::jet_foundation::MIR::MirTypeKind::InlineRange {
            base: Box::new(__jet_bootstrap_type_to_host(base)?),
            lo: lo.to_i64().ok_or_else(|| "inline range lower bound exceeds i64".to_string())?,
            hi: hi.to_i64().ok_or_else(|| "inline range upper bound exceeds i64".to_string())?,
        },
        @p.MIRTypeKind.Tagged@{ marker, inner } => ::jet_foundation::MIR::MirTypeKind::Tagged {
            marker: __jet_bootstrap_tag_to_host(marker),
            inner: Box::new(__jet_bootstrap_type_to_host(inner)?),
        },
        @p.MIRTypeKind.Union@{ members } => ::jet_foundation::MIR::MirTypeKind::Union(
            members.iter().map(__jet_bootstrap_type_to_host).collect::<Result<Vec<_>, _>>()?,
        ),
        @p.MIRTypeKind.Quantity@{ base, dimension } => ::jet_foundation::MIR::MirTypeKind::Quantity {
            base: Box::new(__jet_bootstrap_type_to_host(base)?),
            dimension: __jet_bootstrap_dimension_to_host(dimension)?,
        },
        @p.MIRTypeKind.Measure@{ value: measure } => ::jet_foundation::MIR::MirTypeKind::Measure(
            __jet_bootstrap_measure_to_host(measure)?,
        ),
    };
    let mut result = ::jet_foundation::MIR::MirType::from_kind(kind);
    if let Ok(identity) = (&@deref.MIRType.identity@value.@f.MIRType.identity@).as_ref() {
        result = result.with_identity(__jet_bootstrap_type_id_to_host(identity)?);
    }
    Ok(result)
}
fn __jet_bootstrap_type_from_host(value: &::jet_foundation::MIR::MirType) -> Result<@t.MIRType@, String> {
    let kind = match &value.kind {
        ::jet_foundation::MIR::MirTypeKind::Int => @v.MIRTypeKind.Int@,
        ::jet_foundation::MIR::MirTypeKind::Float => @v.MIRTypeKind.Float@,
        ::jet_foundation::MIR::MirTypeKind::Bool => @v.MIRTypeKind.Bool@,
        ::jet_foundation::MIR::MirTypeKind::String => @v.MIRTypeKind.String@,
        ::jet_foundation::MIR::MirTypeKind::Char => @v.MIRTypeKind.Char@,
        ::jet_foundation::MIR::MirTypeKind::Float32 => @v.MIRTypeKind.Float32@,
        ::jet_foundation::MIR::MirTypeKind::List(inner) => @new.MIRTypeKind.List@{ inner: __jet_bootstrap_type_from_host(inner)? },
        ::jet_foundation::MIR::MirTypeKind::Map { key, value } => @new.MIRTypeKind.Map@{
            key: __jet_bootstrap_type_from_host(key)?,
            value: __jet_bootstrap_type_from_host(value)?,
        },
        ::jet_foundation::MIR::MirTypeKind::Shared(inner) => @new.MIRTypeKind.Shared@{ inner: __jet_bootstrap_type_from_host(inner)? },
        ::jet_foundation::MIR::MirTypeKind::Option(inner) => @new.MIRTypeKind.Option@{ inner: __jet_bootstrap_type_from_host(inner)? },
        ::jet_foundation::MIR::MirTypeKind::Result { ok, err } => @new.MIRTypeKind.Result@{
            ok: __jet_bootstrap_type_from_host(ok)?,
            err: __jet_bootstrap_type_from_host(err)?,
        },
        ::jet_foundation::MIR::MirTypeKind::Fn(signature) => @new.MIRTypeKind.Fn@{ signature: __jet_bootstrap_mir_MIRFunctionSignature_from_host(signature)? },
        ::jet_foundation::MIR::MirTypeKind::SendFn { params, ret, conventions } => @new.MIRTypeKind.SendFn@{
            params: params.iter().map(__jet_bootstrap_type_from_host).collect::<Result<Vec<_>, _>>()?,
            ret: match ret.as_deref() {
                Some(ret) => Ok(__jet_bootstrap_type_from_host(ret)?),
                None => Err(jet_foundation::Outcome::JetAbsent),
            },
            conventions: conventions.iter().map(__jet_bootstrap_mir_MIRAccess_from_host).collect::<Result<Vec<_>, _>>()?,
        },
        ::jet_foundation::MIR::MirTypeKind::Apply { name, args } => @new.MIRTypeKind.Apply@{
            name: @s.MIRNominalRef@{
                id: __jet_bootstrap_type_id_from_host(name.id)?,
                name: name.name.clone(),
            },
            args: args.iter().map(__jet_bootstrap_type_from_host).collect::<Result<Vec<_>, _>>()?,
        },
        ::jet_foundation::MIR::MirTypeKind::TraitObject(bounds) => @new.MIRTypeKind.TraitObject@{
            bounds: bounds.iter().map(|bound| {
                if bound.id.0 == 0 { return Err("MIR trait identity is zero".to_string()); }
                Ok(@s.MIRTraitRef@{
                    id: @s.MIRTraitID@{ value: jet_foundation::Numeric::JetInt::from_big(jet_foundation::Numeric::CtBigInt::from_u64(bound.id.0)) },
                    name: bound.name.clone(),
                })
            }).collect::<Result<Vec<_>, String>>()?,
        },
        ::jet_foundation::MIR::MirTypeKind::Tuple(fields) => @new.MIRTypeKind.Tuple@{
            fields: fields.iter().map(|(name, ty)| Ok(@s.MIRTupleField@{
                name: name.clone(),
                ty: __jet_bootstrap_type_from_host(ty)?,
            })).collect::<Result<Vec<_>, String>>()?,
        },
        ::jet_foundation::MIR::MirTypeKind::FixedList { elem, len } => @new.MIRTypeKind.FixedList@{
            elem: __jet_bootstrap_type_from_host(elem)?,
            len: __jet_bootstrap_measure_from_host(len)?,
        },
        ::jet_foundation::MIR::MirTypeKind::IntN { signed, bits } => @new.MIRTypeKind.IntN@{
            signed: *signed,
            bits: jet_foundation::Numeric::JetInt::from_i64(i64::from(*bits)),
        },
        ::jet_foundation::MIR::MirTypeKind::InlineRange { base, lo, hi } => @new.MIRTypeKind.InlineRange@{
            base: __jet_bootstrap_type_from_host(base)?,
            lo: jet_foundation::Numeric::JetInt::from_i64(*lo),
            hi: jet_foundation::Numeric::JetInt::from_i64(*hi),
        },
        ::jet_foundation::MIR::MirTypeKind::Tagged { marker, inner } => @new.MIRTypeKind.Tagged@{
            marker: __jet_bootstrap_tag_from_host(marker)?,
            inner: __jet_bootstrap_type_from_host(inner)?,
        },
        ::jet_foundation::MIR::MirTypeKind::Union(members) => @new.MIRTypeKind.Union@{
            members: members.iter().map(__jet_bootstrap_type_from_host).collect::<Result<Vec<_>, _>>()?,
        },
        ::jet_foundation::MIR::MirTypeKind::Quantity { base, dimension } => @new.MIRTypeKind.Quantity@{
            base: __jet_bootstrap_type_from_host(base)?,
            dimension: __jet_bootstrap_dimension_from_host(dimension)?,
        },
        ::jet_foundation::MIR::MirTypeKind::Measure(measure) => @new.MIRTypeKind.Measure@{ value: __jet_bootstrap_measure_from_host(measure)? },
    };
    Ok(@s.MIRType@{
        kind,
        identity: match value.identity.as_ref() {
            Some(identity) => Ok(__jet_bootstrap_type_id_from_host(*identity)?),
            None => Err(jet_foundation::Outcome::JetAbsent),
        },
        layout: __jet_bootstrap_layout_from_host(&value.layout)?,
    })
}
"#,
    );
}

/// The Source comptime-value and evaluator-value codecs (shapes from the
/// binding metadata through the `@…@` markers). The reverse key projection is
/// kept separate from value projection so a map's checked key carrier cannot
/// accidentally be stringified.
fn emit_bootstrap_value_codec(out: &mut String) {
    out.push_str(
        r#"
fn __jet_bootstrap_key_fields_to_host(fields: &[@t.TComptimeKeyField@]) -> Result<Vec<(String, ::jet_foundation::MIR::MirConstKey)>, String> {
    fields.iter().map(|field| Ok((
        field.@f.TComptimeKeyField.name@.clone(),
        __jet_bootstrap_key_to_host(&field.@f.TComptimeKeyField.key@)?,
    ))).collect()
}
fn __jet_bootstrap_key_to_host(value: &@t.TComptimeKey@) -> Result<::jet_foundation::MIR::MirConstKey, String> {
    match value {
        @p.TComptimeKey.Int@{ value } => Ok(::jet_foundation::MIR::MirConstKey::Int(value.to_i64().ok_or_else(|| "constant key exceeds i64".to_string())?)),
        @p.TComptimeKey.String@{ value } => Ok(::jet_foundation::MIR::MirConstKey::String(value.clone())),
        @p.TComptimeKey.Bool@{ value } => Ok(::jet_foundation::MIR::MirConstKey::Bool(*value)),
        @p.TComptimeKey.Char@{ value } => Ok(::jet_foundation::MIR::MirConstKey::Char(char::from_u32(value.to_i64().ok_or_else(|| "constant key character exceeds i64".to_string())? as u32).ok_or_else(|| "constant key character is invalid".to_string())?)),
        @p.TComptimeKey.Tuple@{ fields } => Ok(::jet_foundation::MIR::MirConstKey::Tuple(__jet_bootstrap_key_fields_to_host(fields)?)),
        @p.TComptimeKey.Struct@{ type_name, fields } => Ok(::jet_foundation::MIR::MirConstKey::Struct {
            type_name: type_name.clone(),
            fields: __jet_bootstrap_key_fields_to_host(fields)?,
        }),
        @p.TComptimeKey.Enum@{ type_name, variant } => Ok(::jet_foundation::MIR::MirConstKey::Enum {
            type_name: type_name.clone(),
            variant: variant.clone(),
        }),
    }
}
fn __jet_bootstrap_ct_to_host(value: &@t.TComptimeValue@) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {
    match value {
        @p.TComptimeValue.Int@{ value } => Ok(match value.to_i64() {
            Some(value) => ::jet_foundation::MIR::MirRuntimeValue::Int(value),
            None => ::jet_foundation::MIR::MirRuntimeValue::BigInt(value.to_string_rep()),
        }),
        @p.TComptimeValue.Float@{ value, bits } => Ok(::jet_foundation::MIR::MirRuntimeValue::Float { value: *value, f32: bits.to_i64() == Some(32) }),
        @p.TComptimeValue.Bool@{ value } => Ok(::jet_foundation::MIR::MirRuntimeValue::Bool(*value)),
        @p.TComptimeValue.Char@{ value } => Ok(::jet_foundation::MIR::MirRuntimeValue::Char(char::from_u32(value.to_i64().ok_or_else(|| "character exceeds i64".to_string())? as u32).ok_or_else(|| "invalid character value".to_string())?)),
        @p.TComptimeValue.String@{ value } => Ok(::jet_foundation::MIR::MirRuntimeValue::String(value.clone())),
        @p.TComptimeValue.BigInt@{ decimal } => Ok(::jet_foundation::MIR::MirRuntimeValue::BigInt(decimal.clone())),
        @p.TComptimeValue.Bytes@{ value } => Ok(::jet_foundation::MIR::MirRuntimeValue::Bytes(value.to_vec())),
        @p.TComptimeValue.List@{ values } => Ok(::jet_foundation::MIR::MirRuntimeValue::List(values.iter().map(__jet_bootstrap_ct_to_host).collect::<Result<Vec<_>, _>>()?)),
        @p.TComptimeValue.Map@{ entries } => Ok(::jet_foundation::MIR::MirRuntimeValue::Map(entries.iter().map(|entry| Ok((
            __jet_bootstrap_key_to_host(&entry.@f.TComptimeMapEntry.key@)?,
            __jet_bootstrap_ct_to_host(&entry.@f.TComptimeMapEntry.value@)?,
        ))).collect::<Result<Vec<_>, String>>()?)),
        @p.TComptimeValue.Struct@{ type_name, fields } => Ok(::jet_foundation::MIR::MirRuntimeValue::Struct {
            type_name: type_name.clone(),
            fields: fields.iter().map(|field| Ok((
                field.@f.TComptimeField.name@.clone(),
                __jet_bootstrap_ct_to_host(&field.@f.TComptimeField.value@)?,
            ))).collect::<Result<Vec<_>, String>>()?,
        }),
        @p.TComptimeValue.Enum@{ type_name, variant, args } => Ok(::jet_foundation::MIR::MirRuntimeValue::Enum {
            type_name: type_name.clone(),
            variant: variant.clone(),
            args: args.iter().map(|arg| Ok((
                (&@deref.TComptimeEnumArg.name@arg.@f.TComptimeEnumArg.name@).as_ref().ok().cloned(),
                __jet_bootstrap_ct_to_host(&arg.@f.TComptimeEnumArg.value@)?,
            ))).collect::<Result<Vec<_>, String>>()?,
        }),
        @p.TComptimeValue.Present@{ value } => Ok(::jet_foundation::MIR::MirRuntimeValue::Present(Box::new(__jet_bootstrap_ct_to_host(value)?))),
        @p.TComptimeValue.Failed@{ report } => match &*@deref.TComptimeValue.Failed.report@report {
            @p.TComptimeReport.Told@{ value } => Ok(::jet_foundation::MIR::MirRuntimeValue::FailedTold(Box::new(__jet_bootstrap_ct_to_host(value)?))),
            @p.TComptimeReport.Clean@{ .. } => Err("clean comptime failures carry an AST type, not a MIR type".to_string()),
        },
        @v.TComptimeValue.Unit@ => Ok(::jet_foundation::MIR::MirRuntimeValue::Unit),
        @p.TComptimeValue.Closure@{ .. } => Err("closure values are not materializable at the native Prelude boundary".to_string()),
    }
}
fn __jet_bootstrap_ct_from_host(value: &::jet_foundation::MIR::MirRuntimeValue) -> Result<@t.TComptimeValue@, String> {
    match value {
        ::jet_foundation::MIR::MirRuntimeValue::Int(value) => Ok(@new.TComptimeValue.Int@{ value: jet_foundation::Numeric::JetInt::from_i64(*value) }),
        ::jet_foundation::MIR::MirRuntimeValue::BigInt(value) => Ok(@new.TComptimeValue.BigInt@{ decimal: value.clone() }),
        ::jet_foundation::MIR::MirRuntimeValue::Float { value, f32 } => Ok(@new.TComptimeValue.Float@{
            value: *value,
            bits: jet_foundation::Numeric::JetInt::from_i64(if *f32 { 32 } else { 64 }),
        }),
        ::jet_foundation::MIR::MirRuntimeValue::Bool(value) => Ok(@new.TComptimeValue.Bool@{ value: *value }),
        ::jet_foundation::MIR::MirRuntimeValue::Char(value) => Ok(@new.TComptimeValue.Char@{ value: jet_foundation::Numeric::JetInt::from_i64(i64::from(*value as u32)) }),
        ::jet_foundation::MIR::MirRuntimeValue::String(value) => Ok(@new.TComptimeValue.String@{ value: value.clone() }),
        ::jet_foundation::MIR::MirRuntimeValue::Bytes(value) => Ok(@new.TComptimeValue.Bytes@{ value: value.clone() }),
        ::jet_foundation::MIR::MirRuntimeValue::List(values) => Ok(@new.TComptimeValue.List@{ values: values.iter().map(__jet_bootstrap_ct_from_host).collect::<Result<Vec<_>, _>>()? }),
        ::jet_foundation::MIR::MirRuntimeValue::Map(entries) => Ok(@new.TComptimeValue.Map@{
            entries: entries.iter().map(|(key, value)| Ok(@s.TComptimeMapEntry@{
                key: __jet_bootstrap_key_from_host(key)?,
                value: __jet_bootstrap_ct_from_host(value)?,
            })).collect::<Result<Vec<_>, String>>()?,
        }),
        ::jet_foundation::MIR::MirRuntimeValue::Struct { type_name, fields } => Ok(@new.TComptimeValue.Struct@{
            type_name: type_name.clone(),
            fields: fields.iter().map(|(name, value)| Ok(@s.TComptimeField@{
                name: name.clone(),
                value: __jet_bootstrap_ct_from_host(value)?,
            })).collect::<Result<Vec<_>, String>>()?,
        }),
        ::jet_foundation::MIR::MirRuntimeValue::Enum { type_name, variant, args } => Ok(@new.TComptimeValue.Enum@{
            type_name: type_name.clone(),
            variant: variant.clone(),
            args: args.iter().map(|(name, value)| Ok(@s.TComptimeEnumArg@{
                name: name.clone().ok_or(jet_foundation::Outcome::JetAbsent),
                value: __jet_bootstrap_ct_from_host(value)?,
            })).collect::<Result<Vec<_>, String>>()?,
        }),
        ::jet_foundation::MIR::MirRuntimeValue::Present(value) => Ok(@new.TComptimeValue.Present@{ value: __jet_bootstrap_ct_from_host(value)? }),
        ::jet_foundation::MIR::MirRuntimeValue::FailedTold(value) => Ok(@new.TComptimeValue.Failed@{
            report: @new.TComptimeReport.Told@{ value: __jet_bootstrap_ct_from_host(value)? },
        }),
        ::jet_foundation::MIR::MirRuntimeValue::Absent { .. } => Err("a typed absent value is returned directly as JetEvalRuntimeValue::Absent".to_string()),
        ::jet_foundation::MIR::MirRuntimeValue::Unit => Ok(@v.TComptimeValue.Unit@),
        ::jet_foundation::MIR::MirRuntimeValue::Moved | ::jet_foundation::MIR::MirRuntimeValue::Closure(_) | ::jet_foundation::MIR::MirRuntimeValue::NativeCursor(_) | ::jet_foundation::MIR::MirRuntimeValue::NativeOwned(_) => Err("native Prelude returned an unmaterializable value".to_string()),
    }
}
fn __jet_bootstrap_runtime_key_from_host(value: &::jet_foundation::MIR::MirRuntimeValue) -> Result<::jet_foundation::MIR::MirConstKey, String> {
    match value {
        ::jet_foundation::MIR::MirRuntimeValue::Int(value) => Ok(::jet_foundation::MIR::MirConstKey::Int(*value)),
        ::jet_foundation::MIR::MirRuntimeValue::String(value) => Ok(::jet_foundation::MIR::MirConstKey::String(value.clone())),
        ::jet_foundation::MIR::MirRuntimeValue::Bool(value) => Ok(::jet_foundation::MIR::MirConstKey::Bool(*value)),
        ::jet_foundation::MIR::MirRuntimeValue::Char(value) => Ok(::jet_foundation::MIR::MirConstKey::Char(*value)),
        ::jet_foundation::MIR::MirRuntimeValue::Struct { type_name, fields } if type_name == "Tuple" => Ok(::jet_foundation::MIR::MirConstKey::Tuple(fields.iter().map(|(name, value)| Ok((name.clone(), __jet_bootstrap_runtime_key_from_host(value)?))).collect::<Result<Vec<_>, String>>()?)),
        ::jet_foundation::MIR::MirRuntimeValue::Struct { type_name, fields } => Ok(::jet_foundation::MIR::MirConstKey::Struct {
            type_name: type_name.clone(),
            fields: fields.iter().map(|(name, value)| Ok((name.clone(), __jet_bootstrap_runtime_key_from_host(value)?))).collect::<Result<Vec<_>, String>>()?,
        }),
        ::jet_foundation::MIR::MirRuntimeValue::Enum { type_name, variant, args } if args.is_empty() => Ok(::jet_foundation::MIR::MirConstKey::Enum {
            type_name: type_name.clone(),
            variant: variant.clone(),
        }),
        _ => Err("MIR runtime value is not a canonical constant map key".to_string()),
    }
}
fn __jet_bootstrap_eval_key_to_host(value: &@t.JetEvalRuntimeValue@) -> Result<::jet_foundation::MIR::MirConstKey, String> {
    match value {
        @p.JetEvalRuntimeValue.Data@{ value } => __jet_bootstrap_ct_to_host(value).and_then(|value| __jet_bootstrap_runtime_key_from_host(&value)),
        _ => Err("runtime map key is not materialized immutable data".to_string()),
    }
}
fn __jet_bootstrap_eval_aggregate_to_host(
    value: &@t.JetEvalRuntimeAggregate@,
    allow_foreign_handles: bool,
) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {
    match value {
        @p.JetEvalRuntimeAggregate.List@{ values } => Ok(::jet_foundation::MIR::MirRuntimeValue::List(values.iter().map(|value| __jet_bootstrap_eval_to_host_inner(value, allow_foreign_handles)).collect::<Result<Vec<_>, String>>()?)),
        @p.JetEvalRuntimeAggregate.Map@{ entries } => Ok(::jet_foundation::MIR::MirRuntimeValue::Map(entries.iter().map(|entry| Ok((
            __jet_bootstrap_eval_key_to_host(&entry.@f.JetEvalRuntimeMapEntry.key@)?,
            __jet_bootstrap_eval_to_host_inner(&entry.@f.JetEvalRuntimeMapEntry.value@, allow_foreign_handles)?,
        ))).collect::<Result<Vec<_>, String>>()?)),
        @p.JetEvalRuntimeAggregate.Struct@{ type_name, fields } => Ok(::jet_foundation::MIR::MirRuntimeValue::Struct {
            type_name: type_name.clone(),
            fields: fields.iter().map(|field| Ok((
                field.@f.JetEvalRuntimeField.name@.clone(),
                __jet_bootstrap_eval_to_host_inner(&field.@f.JetEvalRuntimeField.value@, allow_foreign_handles)?,
            ))).collect::<Result<Vec<_>, String>>()?,
        }),
        @p.JetEvalRuntimeAggregate.Enum@{ type_name, variant, args } => Ok(::jet_foundation::MIR::MirRuntimeValue::Enum {
            type_name: type_name.clone(),
            variant: variant.clone(),
            args: args.iter().map(|arg| Ok((
                (&@deref.JetEvalRuntimeEnumArg.name@arg.@f.JetEvalRuntimeEnumArg.name@).as_ref().ok().cloned(),
                __jet_bootstrap_eval_to_host_inner(&arg.@f.JetEvalRuntimeEnumArg.value@, allow_foreign_handles)?,
            ))).collect::<Result<Vec<_>, String>>()?,
        }),
    }
}
fn __jet_bootstrap_eval_to_host_inner(
    value: &@t.JetEvalRuntimeValue@,
    allow_foreign_handles: bool,
) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {
    match value {
        @v.JetEvalRuntimeValue.Moved@ => Err("moved values cannot cross a native host boundary".to_string()),
        @p.JetEvalRuntimeValue.Data@{ value } => __jet_bootstrap_ct_to_host(value),
        @p.JetEvalRuntimeValue.Absent@{ element } => Ok(::jet_foundation::MIR::MirRuntimeValue::Absent { element: __jet_bootstrap_type_to_host(element)? }),
        @p.JetEvalRuntimeValue.Result@{ ok, value } => {
            let value = __jet_bootstrap_eval_to_host_inner(value, allow_foreign_handles)?;
            Ok(if *ok {
                ::jet_foundation::MIR::MirRuntimeValue::Present(Box::new(value))
            } else {
                ::jet_foundation::MIR::MirRuntimeValue::FailedTold(Box::new(value))
            })
        }
        @p.JetEvalRuntimeValue.ForeignHandle@{ handle: _, raw, token: _ } if allow_foreign_handles => Ok(::jet_foundation::MIR::MirRuntimeValue::Int(raw.to_i64().ok_or_else(|| "foreign handle is outside i64".to_string())?)),
        @p.JetEvalRuntimeValue.ForeignHandle@{ .. } => Err("foreign handles cross only their checked C foreign-call ABI boundary".to_string()),
        @p.JetEvalRuntimeValue.Aggregate@{ value: aggregate } => __jet_bootstrap_eval_aggregate_to_host(aggregate, allow_foreign_handles),
        @p.JetEvalRuntimeValue.RuntimeFailure@{ .. } => Err("terminal runtime failures cannot cross a native host boundary".to_string()),
        @p.JetEvalRuntimeValue.HostCursor@{ .. } | @p.JetEvalRuntimeValue.Stream@{ .. } => Err("native iterator cursors cannot cross a serializable host boundary".to_string()),
        @p.JetEvalRuntimeValue.Closure@{ .. } | @p.JetEvalRuntimeValue.Address@{ .. } | @p.JetEvalRuntimeValue.SharedCell@{ .. } | @p.JetEvalRuntimeValue.Shared@{ .. } | @p.JetEvalRuntimeValue.SharedWeak@{ .. } | @p.JetEvalRuntimeValue.SharedGuard@{ .. } | @p.JetEvalRuntimeValue.SharedSnapshot@{ .. } | @p.JetEvalRuntimeValue.Condition@{ .. } | @p.JetEvalRuntimeValue.RangeCursor@{ .. } | @p.JetEvalRuntimeValue.ListCursor@{ .. } => Err("runtime handles and closures cannot cross a native host boundary".to_string()),
    }
}
fn __jet_bootstrap_eval_to_host(value: &@t.JetEvalRuntimeValue@) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {
    __jet_bootstrap_eval_to_host_inner(value, false)
}
// This projection is confined to a checked MIR foreign-call row, whose parameter facts own the C ABI.
fn __jet_bootstrap_eval_to_foreign_host(value: &@t.JetEvalRuntimeValue@) -> Result<::jet_foundation::MIR::MirRuntimeValue, String> {
    __jet_bootstrap_eval_to_host_inner(value, true)
}
fn __jet_bootstrap_key_fields_from_host(fields: &[(String, ::jet_foundation::MIR::MirConstKey)]) -> Result<Vec<@t.TComptimeKeyField@>, String> {
    fields.iter().map(|(name, key)| Ok(@s.TComptimeKeyField@{
        name: name.clone(),
        key: __jet_bootstrap_key_from_host(key)?,
    })).collect()
}
fn __jet_bootstrap_key_from_host(value: &::jet_foundation::MIR::MirConstKey) -> Result<@t.TComptimeKey@, String> {
    match value {
        ::jet_foundation::MIR::MirConstKey::Int(value) => Ok(@new.TComptimeKey.Int@{ value: jet_foundation::Numeric::JetInt::from_i64(*value) }),
        ::jet_foundation::MIR::MirConstKey::String(value) => Ok(@new.TComptimeKey.String@{ value: value.clone() }),
        ::jet_foundation::MIR::MirConstKey::Bool(value) => Ok(@new.TComptimeKey.Bool@{ value: *value }),
        ::jet_foundation::MIR::MirConstKey::Char(value) => Ok(@new.TComptimeKey.Char@{ value: jet_foundation::Numeric::JetInt::from_i64(i64::from(*value as u32)) }),
        ::jet_foundation::MIR::MirConstKey::Tuple(fields) => Ok(@new.TComptimeKey.Tuple@{ fields: __jet_bootstrap_key_fields_from_host(fields)? }),
        ::jet_foundation::MIR::MirConstKey::Struct { type_name, fields } => Ok(@new.TComptimeKey.Struct@{
            type_name: type_name.clone(),
            fields: __jet_bootstrap_key_fields_from_host(fields)?,
        }),
        ::jet_foundation::MIR::MirConstKey::Enum { type_name, variant } => Ok(@new.TComptimeKey.Enum@{ type_name: type_name.clone(), variant: variant.clone() }),
    }
}
"#,
    );
}

/// Native Prelude results back into Source host values, and checked host
/// type shapes into MIR types (shapes through the `@…@` markers).
fn emit_bootstrap_host_value_codec(out: &mut String) {
    out.push_str(
        r#"
fn __jet_bootstrap_ct_key_fields_from_host(fields: &[(String, ::jet_foundation::MIR::MirConstKey)]) -> Result<Vec<@t.TComptimeField@>, String> {
    fields.iter().map(|(name, key)| Ok(@s.TComptimeField@{
        name: name.clone(),
        value: __jet_bootstrap_ct_key_value_from_host(key)?,
    })).collect()
}
fn __jet_bootstrap_ct_key_value_from_host(value: &::jet_foundation::MIR::MirConstKey) -> Result<@t.TComptimeValue@, String> {
    match value {
        ::jet_foundation::MIR::MirConstKey::Int(value) => Ok(@new.TComptimeValue.Int@{ value: jet_foundation::Numeric::JetInt::from_i64(*value) }),
        ::jet_foundation::MIR::MirConstKey::String(value) => Ok(@new.TComptimeValue.String@{ value: value.clone() }),
        ::jet_foundation::MIR::MirConstKey::Bool(value) => Ok(@new.TComptimeValue.Bool@{ value: *value }),
        ::jet_foundation::MIR::MirConstKey::Char(value) => Ok(@new.TComptimeValue.Char@{ value: jet_foundation::Numeric::JetInt::from_i64(i64::from(*value as u32)) }),
        ::jet_foundation::MIR::MirConstKey::Tuple(fields) => Ok(@new.TComptimeValue.Struct@{
            type_name: "Tuple".to_string(),
            fields: __jet_bootstrap_ct_key_fields_from_host(fields)?,
        }),
        ::jet_foundation::MIR::MirConstKey::Struct { type_name, fields } => Ok(@new.TComptimeValue.Struct@{
            type_name: type_name.clone(),
            fields: __jet_bootstrap_ct_key_fields_from_host(fields)?,
        }),
        ::jet_foundation::MIR::MirConstKey::Enum { type_name, variant } => Ok(@new.TComptimeValue.Enum@{
            type_name: type_name.clone(),
            variant: variant.clone(),
            args: Vec::<@t.TComptimeEnumArg@>::new(),
        }),
    }
}
fn __jet_bootstrap_host_value_from_runtime(value: &::jet_foundation::MIR::MirRuntimeValue) -> Result<@t.JetEvalHostValue@, String> {
    match value {
        ::jet_foundation::MIR::MirRuntimeValue::Present(value) => Ok(@new.JetEvalHostValue.Present@{ value: __jet_bootstrap_host_value_from_runtime(value)? }),
        ::jet_foundation::MIR::MirRuntimeValue::FailedTold(value) => Ok(@new.JetEvalHostValue.Failed@{ value: __jet_bootstrap_host_value_from_runtime(value)? }),
        ::jet_foundation::MIR::MirRuntimeValue::Absent { element } => Ok(@new.JetEvalHostValue.Absent@{ element: __jet_bootstrap_type_from_host(element)? }),
        ::jet_foundation::MIR::MirRuntimeValue::List(values) => Ok(@new.JetEvalHostValue.List@{
            values: values.iter().map(__jet_bootstrap_host_value_from_runtime).collect::<Result<Vec<_>, String>>()?,
        }),
        ::jet_foundation::MIR::MirRuntimeValue::Map(entries) => Ok(@new.JetEvalHostValue.Map@{
            entries: entries.iter().map(|(key, value)| Ok(@s.JetEvalHostMapEntry@{
                key: @new.JetEvalHostValue.Data@{ value: __jet_bootstrap_ct_key_value_from_host(key)? },
                value: __jet_bootstrap_host_value_from_runtime(value)?,
            })).collect::<Result<Vec<_>, String>>()?,
        }),
        ::jet_foundation::MIR::MirRuntimeValue::Struct { type_name, fields } => Ok(@new.JetEvalHostValue.Struct@{
            type_name: type_name.clone(),
            fields: fields.iter().map(|(name, value)| Ok(@s.JetEvalHostField@{
                name: name.clone(),
                value: __jet_bootstrap_host_value_from_runtime(value)?,
            })).collect::<Result<Vec<_>, String>>()?,
        }),
        ::jet_foundation::MIR::MirRuntimeValue::Enum { type_name, variant, args } => Ok(@new.JetEvalHostValue.Enum@{
            type_name: type_name.clone(),
            variant: variant.clone(),
            args: args.iter().map(|(name, value)| Ok(@s.JetEvalHostEnumArg@{
                name: name.clone().ok_or(jet_foundation::Outcome::JetAbsent),
                value: __jet_bootstrap_host_value_from_runtime(value)?,
            })).collect::<Result<Vec<_>, String>>()?,
        }),
        ::jet_foundation::MIR::MirRuntimeValue::Moved | ::jet_foundation::MIR::MirRuntimeValue::Closure(_) | ::jet_foundation::MIR::MirRuntimeValue::NativeCursor(_) | ::jet_foundation::MIR::MirRuntimeValue::NativeOwned(_) => Err("native runtime resource requires its checked owner packet".to_string()),
        _ => Ok(@new.JetEvalHostValue.Data@{ value: __jet_bootstrap_ct_from_host(value)? }),
    }
}
fn __jet_bootstrap_type_from_shape(shape: &@t.JetEvalHostTypeShape@) -> Result<::jet_foundation::MIR::MirType, String> {
    let root = __jet_bootstrap_source_index(&shape.@f.JetEvalHostTypeShape.root@, "host result type root")?;
    __jet_bootstrap_type_from_shape_node(shape, root, 0)
}
fn __jet_bootstrap_type_from_shape_node(
    shape: &@t.JetEvalHostTypeShape@,
    index: usize,
    depth: usize,
) -> Result<::jet_foundation::MIR::MirType, String> {
    if depth >= shape.@f.JetEvalHostTypeShape.nodes@.len() {
        return Err("checked host type shape contains a recursive non-nominal type".to_string());
    }
    let node = shape.@f.JetEvalHostTypeShape.nodes@.get(index)
        .ok_or_else(|| "checked host type shape has an invalid node reference".to_string())?;
    let child = |node: &jet_foundation::Numeric::JetInt, label: &str| {
        __jet_bootstrap_type_from_shape_node(shape, __jet_bootstrap_source_index(node, label)?, depth + 1)
    };
    match node {
        @p.JetEvalHostTypeNode.Scalar@{ ty } => __jet_bootstrap_type_to_host(ty),
        @p.JetEvalHostTypeNode.Option@{ inner } => Ok(::jet_foundation::MIR::MirType::from_kind(
            ::jet_foundation::MIR::MirTypeKind::Option(Box::new(child(inner, "option type node")?)),
        )),
        @p.JetEvalHostTypeNode.Result@{ ok, error } => Ok(::jet_foundation::MIR::MirType::from_kind(
            ::jet_foundation::MIR::MirTypeKind::Result {
                ok: Box::new(child(ok, "result success type node")?),
                err: Box::new(child(error, "result error type node")?),
            },
        )),
        @p.JetEvalHostTypeNode.List@{ inner } => Ok(::jet_foundation::MIR::MirType::from_kind(
            ::jet_foundation::MIR::MirTypeKind::List(Box::new(child(inner, "list element type node")?)),
        )),
        @p.JetEvalHostTypeNode.Map@{ key, value } => Ok(::jet_foundation::MIR::MirType::from_kind(
            ::jet_foundation::MIR::MirTypeKind::Map {
                key: Box::new(child(key, "map key type node")?),
                value: Box::new(child(value, "map value type node")?),
            },
        )),
        @p.JetEvalHostTypeNode.Tuple@{ fields } => Ok(::jet_foundation::MIR::MirType::from_kind(
            ::jet_foundation::MIR::MirTypeKind::Tuple(fields.iter().map(|field| Ok((
                field.@f.JetEvalHostTypeFieldShape.name@.clone(),
                child(&field.@f.JetEvalHostTypeFieldShape.node@, "tuple field type node")?,
            ))).collect::<Result<Vec<_>, String>>()?),
        )),
        @p.JetEvalHostTypeNode.Struct@{ type_id, type_name, args, fields: _ } | @p.JetEvalHostTypeNode.Enum@{ type_id, type_name, args, variants: _ } => {
            let id = ::jet_foundation::MIR::MirTypeId(__jet_bootstrap_source_u64(&type_id.@f.MIRTypeID.value@, "host nominal type ID")?);
            let args = args.iter().map(|arg| child(arg, "nominal argument type node")).collect::<Result<Vec<_>, String>>()?;
            Ok(::jet_foundation::MIR::MirType::from_kind(::jet_foundation::MIR::MirTypeKind::Apply {
                name: ::jet_foundation::MIR::MirNominalRef { id, name: type_name.clone() },
                args,
            }).with_identity(id))
        }
        @p.JetEvalHostTypeNode.Handle@{ ty: _, owner } => match owner {
            @p.JetEvalHostOwner.Core@{ owner } => __jet_bootstrap_type_to_host(&owner.@f.JetEvalHostCoreOwner.ty@),
            @p.JetEvalHostOwner.Declared@{ ty } => __jet_bootstrap_type_to_host(ty),
            _ => Err("cursor owner shape does not expose a native MIR type".to_string()),
        },
        @p.JetEvalHostTypeNode.Closure@{ .. } => Err("closure type shape cannot be passed to a native Prelude route".to_string()),
    }
}
"#,
    );
}


fn emit_bootstrap_source_resource_bridge(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let row = symbols.type_symbol("MIRPreludeCall")?;
    let eval = symbols.type_symbol("JetEvalRuntimeValue")?;
    let host_argument = symbols.type_symbol("JetEvalHostArgument")?;
    let host_argument_value = symbols.field_symbol("JetEvalHostArgument", "value")?;
    let host_result = symbols.type_symbol("JetEvalHostResult")?;
    let host_outcome = symbols.type_symbol("JetEvalHostOutcome")?;
    let host_transfer = symbols.type_symbol("JetEvalHostTransfer")?;
    let host_value = symbols.type_symbol("JetEvalHostValue")?;
    let host_writeback = symbols.type_symbol("JetEvalHostWriteback")?;
    let transfer_argument = symbols.field_symbol("JetEvalHostTransfer", "argument")?;
    let transfer_path = symbols.field_symbol("JetEvalHostTransfer", "path")?;
    let transfer_token = symbols.field_symbol("JetEvalHostTransfer", "token")?;
    let transfer_handle = symbols.field_symbol("JetEvalHostTransfer", "handle")?;
    let transfer_raw = symbols.field_symbol("JetEvalHostTransfer", "raw")?;
    let host_owner = symbols.type_symbol("JetEvalHostOwner")?;
    let _host_core_owner = symbols.type_symbol("JetEvalHostCoreOwner")?;
    let host_core_owner_fact = symbols.field_symbol("JetEvalHostCoreOwner", "fact")?;
    let host_core_owner_ty = symbols.field_symbol("JetEvalHostCoreOwner", "ty")?;
    let source_handle = symbols.type_symbol("MIRHandleID")?;
    let handle_value = symbols.field_symbol("MIRHandleID", "value")?;
    let row_module = symbols.field_symbol("MIRPreludeCall", "module_name")?;
    let row_member = symbols.field_symbol("MIRPreludeCall", "member")?;
    let row_symbol = symbols.field_symbol("MIRPreludeCall", "symbol")?;
    let shape = symbols.type_symbol("JetEvalHostTypeShape")?;
    let shape_root = symbols.field_symbol("JetEvalHostTypeShape", "root")?;
    let shape_nodes = symbols.field_symbol("JetEvalHostTypeShape", "nodes")?;
    let host_owner_cursor = symbols.variant_path("JetEvalHostOwner", "Cursor")?;
    let cap_type = "::jet_jit::SourceResources::SourceResourceHandle";
    let mir_type_kind = "::jet_foundation::MIR::MirTypeKind";
    let mir_handle = "::jet_foundation::MIR::MirHandleId";
    let native_error = "::jet_foundation::MIR::MirNativeCursorError";
    let resource_key = "::jet_codegen::Codegen::NativeLoopCursor::NativeLoopResourceKey";

    writeln!(
        out,
r#"fn __jet_bootstrap_host_reply_with_transfers_and_writebacks(
             outcome: {host_outcome},
             transfers: Vec<{host_transfer}>,
             writebacks: Vec<{host_writeback}>,
         ) -> {host_result} {{
             @s.JetEvalHostResult@{{
                 transfers,
                 callback_transfers: Vec::new(),
                 fixed_backing_transfers: Vec::new(),
                 writebacks,
                 outcome,
             }}
         }}
         fn __jet_bootstrap_host_reply_with_transfers(outcome: {host_outcome}, transfers: Vec<{host_transfer}>) -> {host_result} {{
             __jet_bootstrap_host_reply_with_transfers_and_writebacks(outcome, transfers, Vec::new())
         }}
         fn __jet_bootstrap_host_reply(outcome: {host_outcome}) -> {host_result} {{
             __jet_bootstrap_host_reply_with_transfers_and_writebacks(outcome, Vec::new(), Vec::new())
         }}
         fn __jet_bootstrap_shape_owner(shape: &{shape}) -> Result<{host_owner}, String> {{
             let mut node_index = __jet_bootstrap_source_index(&shape.{shape_root}, "host resource type root")?;
             for _ in 0..shape.{shape_nodes}.len() {{
                 let node = shape.{shape_nodes}.get(node_index)
                     .ok_or_else(|| "host resource type shape has an invalid node reference".to_string())?;
                 match node {{
                     @p.JetEvalHostTypeNode.Option@{{ inner }} => node_index = __jet_bootstrap_source_index(inner, "host resource type node")?,
                     @p.JetEvalHostTypeNode.Result@{{ ok, error: _ }} => node_index = __jet_bootstrap_source_index(ok, "host resource result node")?,
                     @p.JetEvalHostTypeNode.Handle@{{ ty: _, owner }} => return Ok(owner.clone()),
                     _ => return Err("checked host resource type shape is not a native handle".to_string()),
                 }}
             }}
             Err("host resource type shape cycles through result wrappers".to_string())
         }}
        fn __jet_bootstrap_core_owner_from_shape<T, P>(
            machine: &mut crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineAccess<'_, T>,
            shape: &{shape},
            compiler_program: &::jet_foundation::MIR::MirProgram,
            physical: &P,
        ) -> Result<({mir_handle}, {host_owner}, ::jet_foundation::MIR::MirCoreOwner, ::jet_foundation::MIR::MirType, ::jet_jit::SourceResources::SourceResourceKind), String>
        where
            T: crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineCoreOwnerRows,
            P: crate::BootstrapEntryPhysicalBindings,
        {{
            let owner = __jet_bootstrap_shape_owner(shape)?;
            let core_owner = match &owner {{
                @p.JetEvalHostOwner.Core@{{ owner: core_owner }} => core_owner,
                _ => return Err("checked resource shape does not declare a Core owner".to_string()),
            }};
            let fact = __jet_bootstrap_mir_MIRCoreOwner_to_host(&core_owner.{host_core_owner_fact})?;
            let owner_type = __jet_bootstrap_type_to_host(&core_owner.{host_core_owner_ty})?;
            let {mir_type_kind}::Apply {{ name, .. }} = &owner_type.kind else {{
                return Err("checked Core resource owner is not a nominal Apply".to_string());
            }};
            if name.id != fact.nominal_id || owner_type.identity.is_some_and(|identity| identity != fact.nominal_id) {{
                return Err("checked Core resource type disagrees with its registered nominal identity".to_string());
            }}
            if !crate::__jet_bootstrap_entry_verify_core_owner(machine, &fact, compiler_program, physical)? {{
                return Err("checked Core owner fact is absent from the active MIR program".to_string());
            }}
            let core_kind = ::jet_jit::SourceResources::SourceResourceKind::from_core_owner_type(&fact, &owner_type)?;
            Ok(({mir_handle}(name.id.0), owner, fact, owner_type, core_kind))
        }}
         fn __jet_bootstrap_resource_host_handle(
             capability: &{cap_type},
             owner: {host_owner},
         ) -> Result<{host_value}, String> {{
             let handle_value = jet_foundation::Numeric::JetInt::from_big(jet_foundation::Numeric::CtBigInt::from_u64(capability.handle.0));
             Ok(@new.JetEvalHostValue.Handle@{{
                 handle: {source_handle} {{ {handle_value}: handle_value }},
                 raw: jet_foundation::Numeric::JetInt::from_i64(capability.raw),
                 owner,
                 payload: @new.JetEvalHostValue.Data@{{ value: @v.TComptimeValue.Unit@ }},
             }})
         }}
         fn __jet_bootstrap_resource_factory_result(
             result: Result<{cap_type}, {native_error}>,
             owner: {host_owner},
             span: {span},
         ) -> {host_result} {{
             match result {{
                 Ok(capability) => match __jet_bootstrap_resource_host_handle(&capability, owner) {{
                    Ok(value) => __jet_bootstrap_host_reply(@new.JetEvalHostOutcome.Value@{{ value }}),
                     Err(detail) => __jet_bootstrap_host_failure(span, detail),
                 }},
                 Err({native_error}::Value(value)) => match __jet_bootstrap_host_value_from_runtime(&value) {{
                    Ok(value) => __jet_bootstrap_host_reply(@new.JetEvalHostOutcome.Value@{{ value: @new.JetEvalHostValue.Failed@{{ value }} }}),
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
                    Ok(value) => __jet_bootstrap_host_reply_with_transfers(@new.JetEvalHostOutcome.RuntimeFailure@{{ value, span }}, transfers),
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
            let @p.JetEvalRuntimeValue.ForeignHandle@{{ handle, raw, token }} = value else {{
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
                {transfer_handle}: {source_handle} {{ {handle_value}: jet_foundation::Numeric::JetInt::from_big(jet_foundation::Numeric::CtBigInt::from_u64(handle)) }},
                {transfer_raw}: raw.clone(),
            }})
        }}
         fn __jet_bootstrap_cursor_error(error: {native_error}, span: {span}) -> {host_result} {{
             match error {{
                 {native_error}::Value(value) => match __jet_bootstrap_host_value_from_runtime(&value) {{
                    Ok(value) => __jet_bootstrap_host_reply(@new.JetEvalHostOutcome.RuntimeFailure@{{ value, span }}),
                     Err(detail) => __jet_bootstrap_host_failure(span, detail),
                 }},
                 {native_error}::Internal(detail) => __jet_bootstrap_host_failure(span, detail),
             }}
         }}
         fn __jet_bootstrap_source_handle_from_eval(value: &{eval}) -> Result<({mir_handle}, i64), String> {{
             let @p.JetEvalRuntimeValue.ForeignHandle@{{ handle, raw, token: _ }} = value else {{
                 return Err("checked Source resource operation requires a live native handle".to_string());
             }};
             let handle = __jet_bootstrap_source_u64(&handle.{handle_value}, "Source resource handle")?;
             let raw = raw.to_string_rep().parse::<i64>()
                 .map_err(|_| "Source resource capability is outside i64".to_string())?;
             Ok(({mir_handle}(handle), raw))
         }}
         fn __jet_bootstrap_native_source_resource_call<T, P>(
             machine: &mut crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineAccess<'_, T>,
             row: &{row},
            args: &[{host_argument}],
             result_shape: Option<&{shape}>,
             arg_shapes: &[{shape}],
             span: {span},
             compiler_program: &::jet_foundation::MIR::MirProgram,
             physical: &P,
         ) -> Option<Result<{host_result}, jet_foundation::Outcome::JetAbsent>>
         where
             T: crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineCoreOwnerRows,
             P: crate::BootstrapEntryPhysicalBindings,
         {{
             let module = row.{row_module}.as_str();
             let member = row.{row_member}.as_str();
             let host_symbol = match __jet_bootstrap_mir_MIRSymbol_to_host(&row.{row_symbol}) {{
                 Ok(symbol) => symbol,
                 Err(detail) => return Some(Ok(__jet_bootstrap_host_failure(span, detail))),
             }};
             let symbol = host_symbol.name();
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
                let (checked_handle, _, _, _, core_kind) = match __jet_bootstrap_core_owner_from_shape(machine, source_shape, compiler_program, physical) {{
                    Ok(value) => value,
                    Err(detail) => return Some(Ok(__jet_bootstrap_host_failure(span, detail))),
                }};
                if checked_handle != source_handle {{
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
                let loop_kind = match ::jet_jit::SourceResources::SourceResourceKind::from_loop_source(&key.source_kind) {{
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
                    Ok((capability, _)) => match __jet_bootstrap_resource_host_handle(&capability, @v.JetEvalHostOwner.Cursor@) {{
                        Ok(value) => __jet_bootstrap_host_reply_with_transfers(@new.JetEvalHostOutcome.Value@{{ value }}, transfers),
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
                     "loop_iter_has_next" => cursor.has_next().map(|value| @new.JetEvalHostValue.Data@{{ value: @new.TComptimeValue.Bool@{{ value }} }}),
                     "loop_iter_value" => cursor.value().and_then(|value| {{
                         __jet_bootstrap_host_value_from_runtime(&value)
                             .map_err(|detail| {native_error}::internal(detail))
                     }}),
                     _ => cursor.advance().map(|_| @new.JetEvalHostValue.Data@{{ value: @v.TComptimeValue.Unit@ }}),
                 }};
                 return Some(Ok(match value {{
                    Ok(value) => __jet_bootstrap_host_reply(@new.JetEvalHostOutcome.Value@{{ value }}),
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
                let (handle, owner, _, _, _) = match __jet_bootstrap_core_owner_from_shape(machine, result_shape, compiler_program, physical) {{
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
                let (handle, owner, _, _, _) = match __jet_bootstrap_core_owner_from_shape(machine, result_shape, compiler_program, physical) {{
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
         fn __jet_bootstrap_native_handle_call<T>(
             _machine: &mut crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineAccess<'_, T>,
             operation: &String,
             handle: &{source_handle},
             raw: &jet_foundation::Numeric::JetInt,
             span: &{span},
         ) -> ::std::result::Result<{host_result}, jet_foundation::Outcome::JetAbsent> {{
             if operation != "resource_release" {{
                 return Err(jet_foundation::Outcome::JetAbsent);
             }}
             let span = span.clone();
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
                Ok(_released) => Ok(__jet_bootstrap_host_reply(@new.JetEvalHostOutcome.Value@{{ value: @new.JetEvalHostValue.Data@{{ value: @v.TComptimeValue.Unit@ }} }})),
                 Err(detail) => Ok(__jet_bootstrap_host_failure(span, detail)),
             }}
         }}"#,
        row = row,
        eval = eval,
        host_argument = host_argument,
        host_argument_value = host_argument_value,
        host_result = host_result,
        host_writeback = host_writeback,
        host_value = host_value,
        host_owner = host_owner,
        source_handle = source_handle,
        handle_value = handle_value,
        row_module = row_module,
        row_member = row_member,
        row_symbol = row_symbol,
        host_owner_cursor = host_owner_cursor,
        host_core_owner_fact = host_core_owner_fact,
        host_core_owner_ty = host_core_owner_ty,
        shape = shape,
        shape_root = shape_root,
        shape_nodes = shape_nodes,
        cap_type = cap_type,
        mir_type_kind = mir_type_kind,
        mir_handle = mir_handle,
        native_error = native_error,
        resource_key = resource_key,
        host_outcome = host_outcome,
        host_transfer = host_transfer,
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
    let row = symbols.type_symbol("MIRPreludeCall")?;
    let shape = symbols.type_symbol("JetEvalHostTypeShape")?;
    let host_argument = symbols.type_symbol("JetEvalHostArgument")?;
    let host_argument_value = symbols.field_symbol("JetEvalHostArgument", "value")?;
    let host_result = symbols.type_symbol("JetEvalHostResult")?;
    let host_outcome = symbols.type_symbol("JetEvalHostOutcome")?;
    let span = symbols.type_symbol("Span")?;
    let internal_problem = symbols.type_symbol("JetEvalInternalProblem")?;
    let internal_problem_span = symbols.field_symbol("JetEvalInternalProblem", "span")?;
    let internal_problem_message = symbols.field_symbol("JetEvalInternalProblem", "message")?;

    writeln!(
        out,
        "fn __jet_bootstrap_host_failure_outcome(span: {span}, detail: String) -> {host_outcome} {{
            @new.JetEvalHostOutcome.InternalProblem@{{ problem: {internal_problem} {{
                {internal_problem_span}: Ok(span),
                {internal_problem_message}: detail,
            }} }}
        }}
        fn __jet_bootstrap_host_failure(span: {span}, detail: String) -> {host_result} {{
            __jet_bootstrap_host_reply(__jet_bootstrap_host_failure_outcome(span, detail))
        }}
         fn __jet_bootstrap_native_host_call<T, P>(
             machine: &mut crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineAccess<'_, T>,
             target: &@t.JetEvalHostCallTarget@,
             static_operand: &::std::result::Result<@t.JetEvalHostStaticOperand@, jet_foundation::Outcome::JetAbsent>,
             args: Vec<{host_argument}>,
             result_shape: &::std::result::Result<{shape}, jet_foundation::Outcome::JetAbsent>,
             arg_shapes: &Vec<{shape}>,
             span: &{span},
             compiler_program: &::jet_foundation::MIR::MirProgram,
             physical: &P,
         ) -> ::std::result::Result<{host_result}, jet_foundation::Outcome::JetAbsent>
         where
             T: crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineCoreOwnerRows,
             P: crate::BootstrapEntryPhysicalBindings,
         {{
             // The host serves plain Prelude rows; take-pattern targets and
             // static operands stay unavailable, which the evaluator reports.
             let @p.JetEvalHostCallTarget.Prelude@{{ row }} = target else {{
                 return Err(jet_foundation::Outcome::JetAbsent);
             }};
             if static_operand.is_ok() {{
                 return Err(jet_foundation::Outcome::JetAbsent);
             }}
             let row = &@deref.JetEvalHostCallTarget.Prelude.row@*row;
             let span = span.clone();
             let result_shape = result_shape.as_ref().ok();
             if let Some(outcome) = __jet_bootstrap_native_source_resource_call(machine, row, &args, result_shape, arg_shapes, span.clone(), compiler_program, physical) {{
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
             let native_span = match __jet_bootstrap_span_to_host(&span) {{
                 Ok(value) => value,
                 Err(detail) => return Ok(__jet_bootstrap_host_failure(span, detail)),
             }};
             let native_row = match __jet_bootstrap_mir_MIRPreludeCall_to_host(row) {{
                 Ok(row) => row,
                 Err(detail) => return Ok(__jet_bootstrap_host_failure(span, detail)),
             }};
             let outcome = crate::Codegen::NativePreludeBridge::ambient_call(
                 &native_row,
                 native_args,
                 native_result_ty,
                 native_span,
             );
             match outcome {{
                 None => Err(jet_foundation::Outcome::JetAbsent),
                Some(Ok(crate::Comptime::AmbientMirPreludeResult::Value(value))) => match __jet_bootstrap_host_value_from_runtime(&value) {{
                    Ok(value) => Ok(__jet_bootstrap_host_reply(@new.JetEvalHostOutcome.Value@{{ value }})),
                    Err(detail) => Ok(__jet_bootstrap_host_failure(span, detail)),
                }},
                Some(Ok(crate::Comptime::AmbientMirPreludeResult::Effect {{ value, stdout, stderr }})) => match __jet_bootstrap_host_value_from_runtime(&value) {{
                    Ok(value) => Ok(__jet_bootstrap_host_reply(@new.JetEvalHostOutcome.Effect@{{ value, stdout, stderr }})),
                    Err(detail) => Ok(__jet_bootstrap_host_failure(span, detail)),
                }},
                Some(Ok(crate::Comptime::AmbientMirPreludeResult::Control {{ value, stdout, stderr, exit_code }})) => match __jet_bootstrap_host_value_from_runtime(&value) {{
                    Ok(value) => Ok(__jet_bootstrap_host_reply(@new.JetEvalHostOutcome.Control@{{ value, stdout, stderr, exit_code: jet_foundation::Numeric::JetInt::from_i64(i64::from(exit_code)) }})),
                    Err(detail) => Ok(__jet_bootstrap_host_failure(span, detail)),
                }},
                Some(Err(error)) => match __jet_bootstrap_diagnostic_from_host(&error) {{
                    Ok(diagnostic) => Ok(__jet_bootstrap_host_reply(@new.JetEvalHostOutcome.Failure@{{ kind: @v.JetEvalErrorKind.Source@, diagnostic }})),
                    Err(detail) => Ok(__jet_bootstrap_host_failure(span, detail)),
                }},
             }}
         }}"
        ,
        host_outcome = host_outcome,
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    Ok(())
}
fn emit_bootstrap_foreign_callback(
    out: &mut String,
    symbols: &BootstrapCodecSymbols<'_>,
) -> Result<(), BootstrapHostCodecError> {
    let foreign = symbols.type_symbol("MIRForeign")?;
    let param = symbols.type_symbol("MIRParam")?;
    let ownership = symbols.type_symbol("MIROwnership")?;
    let access = symbols.type_symbol("MIRAccess")?;
    let effects = symbols.type_symbol("MIREffectFacts")?;
    let _named_span = symbols.type_symbol("MIRNamedSpan")?;
    let _applicability = symbols.type_symbol("MIRTargetApplicability")?;
    let _eval = symbols.type_symbol("JetEvalRuntimeValue")?;
    let host_argument = symbols.type_symbol("JetEvalHostArgument")?;
    let host_argument_value = symbols.field_symbol("JetEvalHostArgument", "value")?;
    let host_result = symbols.type_symbol("JetEvalHostResult")?;
    let host_writeback = symbols.type_symbol("JetEvalHostWriteback")?;
    let host_writeback_argument = symbols.field_symbol("JetEvalHostWriteback", "argument")?;
    let host_writeback_value = symbols.field_symbol("JetEvalHostWriteback", "value")?;
    let span = symbols.type_symbol("Span")?;
    let param_index = symbols.field_symbol("MIRParam", "index")?;
    let param_name = symbols.field_symbol("MIRParam", "name")?;
    let param_span = symbols.field_symbol("MIRParam", "span")?;
    let param_ty = symbols.field_symbol("MIRParam", "ty")?;
    let param_access = symbols.field_symbol("MIRParam", "access")?;
    let param_ownership = symbols.field_symbol("MIRParam", "ownership")?;
    let param_public_label = symbols.field_symbol("MIRParam", "public_label")?;
    let param_variadic = symbols.field_symbol("MIRParam", "variadic")?;
    let param_default_present = symbols.field_symbol("MIRParam", "default_present")?;
    let ownership_mode = symbols.field_symbol("MIROwnership", "mode")?;
    let ownership_drop = symbols.field_symbol("MIROwnership", "drop")?;
    let ownership_moved = symbols.field_symbol("MIROwnership", "moved")?;
    let ownership_last_use = symbols.field_symbol("MIROwnership", "last_use")?;
    let ownership_gc_root = symbols.field_symbol("MIROwnership", "gc_root")?;
    let effects_direct = symbols.field_symbol("MIREffectFacts", "direct")?;
    let effects_solved = symbols.field_symbol("MIREffectFacts", "solved")?;
    let effects_call_edges = symbols.field_symbol("MIREffectFacts", "call_edges")?;
    let effects_maximal = symbols.field_symbol("MIREffectFacts", "maximal")?;
    let effects_direct_spans = symbols.field_symbol("MIREffectFacts", "direct_spans")?;
    let named_span_name = symbols.field_symbol("MIRNamedSpan", "name")?;
    let named_span_span = symbols.field_symbol("MIRNamedSpan", "span")?;
    let target_rust_aot = symbols.field_symbol("MIRTargetApplicability", "rust_aot")?;
    let target_cranelift = symbols.field_symbol("MIRTargetApplicability", "cranelift")?;
    let target_interpreter = symbols.field_symbol("MIRTargetApplicability", "interpreter")?;
    let target_web = symbols.field_symbol("MIRTargetApplicability", "web")?;
    let foreign_id = symbols.field_symbol("MIRForeign", "id")?;
    let foreign_module_id = symbols.field_symbol("MIRForeign", "module_id")?;
    let foreign_key = symbols.field_symbol("MIRForeign", "key")?;
    let foreign_module_name = symbols.field_symbol("MIRForeign", "module_name")?;
    let foreign_name = symbols.field_symbol("MIRForeign", "name")?;
    let foreign_span = symbols.field_symbol("MIRForeign", "span")?;
    let foreign_symbol = symbols.field_symbol("MIRForeign", "symbol")?;
    let foreign_path = symbols.field_symbol("MIRForeign", "path")?;
    let foreign_params = symbols.field_symbol("MIRForeign", "params")?;
    let foreign_bridge_eligible = symbols.field_symbol("MIRForeign", "bridge_eligible")?;
    let foreign_raw_scalar_abi = symbols.field_symbol("MIRForeign", "raw_scalar_abi")?;
    let foreign_return_type = symbols.field_symbol("MIRForeign", "return_type")?;
    let foreign_abi = symbols.field_symbol("MIRForeign", "foreign_abi")?;
    let foreign_language = symbols.field_symbol("MIRForeign", "foreign_language")?;
    let foreign_target_applicability = symbols.field_symbol("MIRForeign", "target_applicability")?;
    let foreign_effects = symbols.field_symbol("MIRForeign", "effects")?;
    let foreign_callback_transport = symbols.field_symbol("MIRForeign", "callback_transport")?;
    let foreign_callback_plan_digest = symbols.field_symbol("MIRForeign", "callback_plan_digest")?;
    let foreign_callback_identity = symbols.field_symbol("MIRForeign", "callback_identity")?;
    let foreign_link = symbols.field_symbol("MIRForeign", "link")?;
    let foreign_callback = symbols.field_symbol("MIRForeign", "callback")?;
    let foreign_handle = symbols.field_symbol("MIRForeign", "handle")?;
    let foreign_close_function = symbols.field_symbol("MIRForeign", "close_function")?;
    let foreign_close_foreign = symbols.field_symbol("MIRForeign", "close_foreign")?;
    let foreign_undo_function = symbols.field_symbol("MIRForeign", "undo_function")?;
    let foreign_id_value = symbols.field_symbol("MIRForeignID", "value")?;
    let module_id_value = symbols.field_symbol("MIRModuleID", "value")?;
    let link_unit_id_value = symbols.field_symbol("MIRLinkUnitID", "value")?;
    let callback_id_value = symbols.field_symbol("MIRCallbackID", "value")?;
    let handle_id_value = symbols.field_symbol("MIRHandleID", "value")?;
    let function_id_value = symbols.field_symbol("MIRFunctionID", "value")?;
    let foreign_abi_c = symbols.variant_path("MIRForeignABI", "C")?;
    let foreign_abi_c_unwind = symbols.variant_path("MIRForeignABI", "CUnwind")?;
    let foreign_abi_system = symbols.variant_path("MIRForeignABI", "System")?;
    let foreign_abi_stdcall = symbols.variant_path("MIRForeignABI", "Stdcall")?;
    let foreign_abi_fastcall = symbols.variant_path("MIRForeignABI", "Fastcall")?;
    let foreign_abi_vectorcall = symbols.variant_path("MIRForeignABI", "Vectorcall")?;
    let foreign_abi_rust = symbols.variant_path("MIRForeignABI", "Rust")?;
    let foreign_language_c = symbols.variant_path("MIRForeignLanguage", "C")?;
    let foreign_language_cpp = symbols.variant_path("MIRForeignLanguage", "Cpp")?;
    let foreign_language_rust = symbols.variant_path("MIRForeignLanguage", "Rust")?;
    let foreign_language_assembly = symbols.variant_path("MIRForeignLanguage", "Assembly")?;
    let ownership_mode_copy = symbols.variant_path("MIROwnershipMode", "Copy")?;
    let ownership_mode_owned = symbols.variant_path("MIROwnershipMode", "Owned")?;
    let ownership_mode_shared = symbols.variant_path("MIROwnershipMode", "Shared")?;
    let ownership_mode_read_borrow = symbols.variant_path("MIROwnershipMode", "ReadBorrow")?;
    let ownership_mode_write_borrow = symbols.variant_path("MIROwnershipMode", "WriteBorrow")?;
    let ownership_mode_move = symbols.variant_path("MIROwnershipMode", "Move")?;
    let drop_no_drop = symbols.variant_path("MIRDropKind", "NoDrop")?;
    let drop_value = symbols.variant_path("MIRDropKind", "Value")?;
    let drop_shared = symbols.variant_path("MIRDropKind", "Shared")?;
    let drop_view = symbols.variant_path("MIRDropKind", "View")?;
    let drop_foreign_handle = symbols.variant_path("MIRDropKind", "ForeignHandle")?;
    let access_read = symbols.variant_path("MIRAccess", "Read")?;
    let access_write = symbols.variant_path("MIRAccess", "Write")?;
    let access_move = symbols.variant_path("MIRAccess", "Move")?;

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
                 @p.MIRForeignABI.Platform@{{ name }} => ::jet_foundation::MIR::MirForeignAbi::Platform(name),
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
         fn __jet_bootstrap_native_foreign_call<T>(
             _machine: &mut crate::compiler_bootstrap_entry_codec::BootstrapEntryMachineAccess<'_, T>,
             row: &{foreign},
             args: Vec<{host_argument}>,
             span: &{span},
         ) -> ::std::result::Result<{host_result}, jet_foundation::Outcome::JetAbsent> {{
             let span = span.clone();
             let native_row = match __jet_bootstrap_foreign_to_host(row.clone()) {{
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
                 None => Err(jet_foundation::Outcome::JetAbsent),
                 Some(Err(error)) => match __jet_bootstrap_diagnostic_from_host(&error) {{
                    Ok(diagnostic) => Ok(__jet_bootstrap_host_reply(@new.JetEvalHostOutcome.Failure@{{ kind: @v.JetEvalErrorKind.Source@, diagnostic }})),
                     Err(detail) => Ok(__jet_bootstrap_host_failure(span, detail)),
                 }},
                 Some(Ok(result)) => {{
                     let value = match __jet_bootstrap_host_value_from_runtime(&result.value) {{
                         Ok(value) => value,
                         Err(detail) => return Ok(__jet_bootstrap_host_failure(span, detail)),
                     }};
                     let writebacks = match result.writebacks.iter().map(|writeback| {{
                         let argument = i64::try_from(writeback.parameter).map_err(|_| \"foreign writeback index exceeds Jet Int\".to_string())?;
                         Ok({host_writeback} {{
                             {host_writeback_argument}: jet_foundation::Numeric::JetInt::from_i64(argument),
                             {host_writeback_value}: __jet_bootstrap_host_value_from_runtime(&writeback.value)?,
                         }})
                     }}).collect::<Result<Vec<_>, String>>() {{
                         Ok(value) => value,
                         Err(detail) => return Ok(__jet_bootstrap_host_failure(span, detail)),
                     }};
                     Ok(__jet_bootstrap_host_reply_with_transfers_and_writebacks(
                         @new.JetEvalHostOutcome.Value@{{ value }},
                         Vec::new(),
                         writebacks,
                     ))
                 }}
             }}
         }}"
    )
    .map_err(|error| BootstrapHostCodecError::InvalidMetadata(error.to_string()))?;
    Ok(())
}

