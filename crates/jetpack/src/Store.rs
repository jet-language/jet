//! Jetpack state + store roots (D-JPK12; hangar per unified ecosystem U2).
//!
//! End-state roots are user-owned by default: Linux uses
//! `$XDG_DATA_HOME/jet` (or `~/.local/share/jet`), macOS uses
//! `~/Library/Application Support/Jet`, and Windows uses
//! `%LOCALAPPDATA%/Jet`; each holds the content-addressed **Hangar**.
//! Jetpack *owns* the lifecycle even when the Nix provider reports canonical
//! `/nix/store` paths — the registration boundary projects their bytes into
//! Hangar and keeps the original path only as producer provenance.
//!
//! A project also has a project-local **`.jet/` managed folder**
//! (`Syntax::SOURCE_ROOT_DIR`) holding the single lockfile (`.jet/lock`),
//! caches, and GC roots — never the realized packages, which live in the shared
//! hangar.
//!
//! U28 / D-JPK-NODAEMON1=A: no root-owned default path. `JETPACK_ROOT`
//! overrides everything (tests set it to a tempdir), but the ordinary path is
//! per-user and coordinated with file locks.
//!
//! Card #367 / D-PRODUCT-SPLIT1=C: root resolution (`Roots`, `resolve`,
//! `managed_dir`, `lock_path`) and read-only listing (`StoreEntry`,
//! `CacheIdentity`, `list`, `parse_meta`) live in `jet_pkg_model::Store` —
//! `jet-driver`'s module loader needs those to resolve `use <pkg>` imports
//! against the hangar without depending on this engine (realization, cache
//! leasing, GC). Re-exported here so every other call site in this crate is
//! unchanged.
pub use jet_pkg_model::Store::{
    legacy_user_hangar_dir, legacy_user_root, lock_path, managed_dir, parse_meta, resolve,
    CacheIdentity, ParsedMeta, Roots, StoreEntry,
};

use crate::TrustRoot::{cache_builder_identity, is_cache_builder_revoked};
use crate::SHA256;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(target_os = "linux")]
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod Producer;
pub use Producer::*;
pub(crate) use Producer::{
    bind_adapter_hook_identity, cache_action_identity, canonical_producer, refresh_nix_lock_digest,
    validate_cached_adapter_hook,
};
mod Cache;
pub use Cache::*;
mod Archive;
pub use Archive::*;
mod Nar;
pub use Nar::*;
pub(crate) mod NixCache;
#[cfg(test)]
pub(crate) use NixCache::encode_zstd_deterministic;
pub(crate) use NixCache::{
    admit_nix_closure_with_progress, plan_nix_downloads, AdmittedNixClosure, NixOutputRequest,
    StoreError,
};
#[cfg(test)]
pub(crate) use NixCache::admit_nix_closure;
mod Broker;
pub use Broker::*;
mod Reproducibility;
pub(crate) use Reproducibility::{
    certify_registration_unlocked, certify_registration_unlocked_with_fresh_agreement,
    reproducibility_blocked,
};

/// Progress facts emitted by a provider while it acquires bytes. The sink is
/// additive so parallel closure workers can report independently; the
/// terminal owns rendering and decides when to redraw.
pub(crate) trait ProgressSink: Send + Sync {
    fn discovered_bytes(&self, bytes: u64);
    fn transferred_bytes(&self, bytes: u64);
    fn phase(&self, _phase: &str) {}
    fn object_progress(&self, _done: usize, _total: usize) {}
}

pub(crate) type ProgressHandle = Arc<dyn ProgressSink>;

thread_local! {
    static CURRENT_PROGRESS: RefCell<Option<ProgressHandle>> = const { RefCell::new(None) };
}

struct ProgressScope {
    previous: Option<ProgressHandle>,
}

impl Drop for ProgressScope {
    fn drop(&mut self) {
        CURRENT_PROGRESS.with(|current| {
            current.replace(self.previous.take());
        });
    }
}

pub(crate) fn with_progress<T>(progress: ProgressHandle, operation: impl FnOnce() -> T) -> T {
    let previous = CURRENT_PROGRESS.with(|current| current.replace(Some(progress)));
    let _scope = ProgressScope { previous };
    operation()
}

pub(crate) fn current_progress() -> Option<ProgressHandle> {
    CURRENT_PROGRESS.with(|current| current.borrow().clone())
}

fn list_unlocked(roots: &Roots) -> std::io::Result<Vec<StoreEntry>> {
    jet_pkg_model::Store::list_checked(roots)
}

/// Inspect package projections without taking a lock or replaying journals.
/// Health/reporting paths use this so observation cannot create or repair
/// store state.
pub(crate) fn list_read_only(roots: &Roots) -> Vec<StoreEntry> {
    jet_pkg_model::Store::list(roots)
}

/// Strictly read the committed package projection without taking the Hangar
/// lock or replaying a migration. Audit uses this boundary so observation
/// cannot publish a repaired projection as a side effect.
pub(crate) fn list_read_only_checked(roots: &Roots) -> std::io::Result<Vec<StoreEntry>> {
    jet_pkg_model::Store::list_checked(roots)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct LeaseInventory {
    pub active: usize,
    pub stale: usize,
}

#[derive(Debug)]
struct LeaseNode {
    path: PathBuf,
    stale: bool,
}

/// Read executable-lease ownership without changing the lease directory.
/// Recovery and doctor share the same parser and kernel-lock rule so a lease
/// cannot be healthy to one command and stale to the other.
pub(crate) fn inspect_leases(roots: &Roots) -> std::io::Result<LeaseInventory> {
    let nodes = lease_nodes_unlocked(roots)?;
    Ok(LeaseInventory {
        active: nodes.iter().filter(|node| !node.stale).count(),
        stale: nodes.iter().filter(|node| node.stale).count(),
    })
}

/// Produce the audit projection without a lock, migration, journal replay, or
/// cleanup. A malformed object or committed closure cannot be omitted from a
/// successful report.
pub(crate) fn audit_read_only(roots: &Roots) -> std::io::Result<AuditSnapshot> {
    let graph = Closure::closure_graph_read_only(roots)?;
    let entries = list_read_only_checked(roots)?;
    let known = entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<BTreeSet<_>>();

    let hangar = roots.hangar_dir();
    match fs::read_dir(&hangar) {
        Ok(directory) => {
            for entry in directory {
                let entry = entry?;
                if !entry.file_type()?.is_dir() {
                    continue;
                }
                let name = entry.file_name();
                if matches!(
                    name.to_str(),
                    Some(
                        "build-scratch"
                            | "failed-scratch"
                            | "objects"
                            | ".stage"
                            | "cas"
                            | "referrers"
                            | "closure-db"
                            | "lifecycle-db"
                            | "quarantine"
                            | "receipts"
                            | "stage"
                            | ".archive-stage"
                            | "reproducibility-staging"
                            | "unreproducible"
                    )
                ) {
                    continue;
                }
                let id = name.to_string_lossy().into_owned();
                if !known.contains(id.as_str()) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Hangar object `{id}` has missing or malformed metadata"),
                    ));
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    for record in graph.records.values() {
        if !known.contains(record.id.as_str()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "closure graph has package record `{}` but its metadata projection is missing",
                    record.id
                ),
            ));
        }
    }

    for entry in &entries {
        let record = graph.records.get(&entry.id).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("closure graph has no package record for `{}`", entry.id),
            )
        })?;
        let metadata_path = hangar.join(&entry.id).join("meta.json");
        let metadata = fs::read_to_string(&metadata_path)?;
        if metadata != record.package_meta {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "metadata projection for `{}` disagrees with the committed closure record",
                    entry.id
                ),
            ));
        }
        let mut expected_outputs = entry.named_outputs.clone();
        expected_outputs.insert("out".to_string(), entry.envelope.output_hash.clone());
        let expected_references = entry.references.iter().cloned().collect::<BTreeSet<_>>();
        if record.primary != entry.envelope.output_hash
            || record.outputs != expected_outputs
            || record.references != expected_references
            || (!record.producer_record.is_empty()
                && record.producer_record != entry.producer_record)
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "closure graph for `{}` disagrees with its metadata projection",
                    entry.id
                ),
            ));
        }
        if entry.envelope.output_hash.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Hangar object `{}` has no output digest", entry.id),
            ));
        }
        let actual = Ingest::try_entry_output_hash(roots, entry).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Hangar object `{}` could not be hashed: {error}", entry.id),
            )
        })?;
        if actual != entry.envelope.output_hash {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Hangar object `{}` failed its content digest: expected {}, got {actual}",
                    entry.id, entry.envelope.output_hash
                ),
            ));
        }
        if !entry.producer_record.is_empty() {
            ProducerRecord::decode(&entry.producer_record).map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Hangar object `{}` has an invalid producer record: {error}", entry.id),
                )
            })?;
        }
    }

    Ok(AuditSnapshot {
        entries,
        leases: inspect_leases(roots)?,
    })
}

#[derive(Debug)]
pub(crate) struct AuditSnapshot {
    pub entries: Vec<StoreEntry>,
    pub leases: LeaseInventory,
}

pub fn list_checked(roots: &Roots) -> std::io::Result<Vec<StoreEntry>> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        // Listing is a checked engine boundary, not the model-only view. Run
        // the same legacy closure migration used by verify, clean, and cache
        // reuse so every consumer observes one receipt-bearing projection.
        Closure::migrate_closure_graph_unlocked(roots)?;
        list_unlocked(roots)
    })
}

/// Read package records after replaying committed closure projections.
/// Corrupt WAL fails closed as an empty list for compatibility with the
/// historical infallible listing API; integrity-sensitive callers use
/// `list_checked`.
pub fn list(roots: &Roots) -> Vec<StoreEntry> {
    list_checked(roots).unwrap_or_default()
}

pub fn migrate_legacy_hangar(roots: &Roots) -> std::io::Result<bool> {
    Lifecycle::migrate_legacy_hangar(roots)
}

const BUILD_SCRATCH_DIR: &str = "build-scratch";
const FAILED_SCRATCH_DIR: &str = "failed-scratch";
const AUTO_CLEAN_STAMP: &str = ".last-auto-clean";
const STALE_AFTER: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const AUTO_CLEAN_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
const MAX_GC_METADATA_BYTES: u64 = 1 << 20;
static OPTIMIZE_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static GC_QUARANTINE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

const PRIVATE_UNTRUSTED_BUILD: &str = "private-untrusted";

pub(crate) fn is_private_untrusted_build(producer: &ProducerRecord) -> bool {
    producer.facts.get("build.trust").map(String::as_str) == Some(PRIVATE_UNTRUSTED_BUILD)
}

#[cfg(any(test, windows))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowsDirectorySyncContract {
    read: bool,
    write: bool,
    share_mode: u32,
    custom_flags: u32,
}

#[cfg(any(test, windows))]
fn windows_directory_sync_contract() -> WindowsDirectorySyncContract {
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;

    WindowsDirectorySyncContract {
        read: true,
        write: true,
        share_mode: FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
        custom_flags: FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
    }
}

pub(crate) fn sync_store_node(path: &Path, directory: bool) -> std::io::Result<()> {
    if !directory {
        return fs::File::open(path)?.sync_all();
    }
    #[cfg(unix)]
    {
        return sync_store_directory_handle(&fs::File::open(path)?);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;

        let contract = windows_directory_sync_contract();

        // BACKUP_SEMANTICS is the Win32 directory-open contract. sync_all is
        // std's durable FlushFileBuffers-equivalent for the resulting handle.
        let directory = fs::OpenOptions::new()
            .read(contract.read)
            .write(contract.write)
            .share_mode(contract.share_mode)
            .custom_flags(contract.custom_flags)
            .open(path)?;
        return sync_store_directory_handle(&directory);
    }
    #[cfg(not(any(unix, windows)))]
    {
        // No std directory-handle contract exists on this target. File bytes
        // were synced recursively above; the parent publication rename keeps
        // its platform durability guarantee at the caller boundary.
        let _ = path;
        Ok(())
    }
}

pub(super) fn sync_store_directory_handle(directory: &fs::File) -> std::io::Result<()> {
    directory.sync_all()
}

pub(super) fn sync_store_directory(path: &Path) -> std::io::Result<()> {
    sync_store_node(path, true)
}

/// Recover every Hangar crash surface under one advisory-lock ownership
/// interval. The unlocked helpers are also used by already-locked operations.
pub fn recover_hangar(roots: &Roots) -> std::io::Result<usize> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        Ingest::ensure_real_directory(&roots.hangar_dir(), "Hangar root")?;
        let staging = Ingest::recover_hangar_staging_unlocked(roots)?;
        let reproducibility = Reproducibility::recover_certification_staging_unlocked(roots)?;
        let archive = Archive::recover_archive_staging_unlocked(roots)?;
        let repairs = Archive::recover_repair_quarantine_unlocked(roots)?;
        let build_debug = super::BuildDebug::recover_scratch(&roots.hangar_dir())?;
        // The authenticated protocol owns receipt validation and only removes
        // a snapshot after its inherited owner lock is idle.  The existing
        // lease-container sweep then removes the now-empty container.
        let _authenticated_leases = super::RuntimePolicy::ExecutableLeaseProtocol::open(
            &roots.root,
        )?
        .recover_stale_leases()?;
        let leases = recover_stale_leases_unlocked(roots)?;
        let closure = Closure::recover_closure_journal_unlocked(roots)?;
        let migrated = Closure::migrate_closure_graph_unlocked(roots)?.0;
        Ok(staging
            + reproducibility
            + archive
            + repairs
            + build_debug
            + leases
            + closure
            + migrated)
    })
}

const LEASE_NAME_MAX: usize = 256;

fn recover_stale_leases_unlocked(roots: &Roots) -> std::io::Result<usize> {
    let nodes = lease_nodes_unlocked(roots)?;
    let mut swept = 0;
    for node in nodes.into_iter().filter(|node| node.stale) {
        let metadata = match fs::symlink_metadata(&node.path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if metadata.is_dir() && !metadata.file_type().is_symlink() {
            make_tree_writable_for_removal(&node.path)?;
            fs::remove_dir_all(&node.path)?;
        } else if metadata.file_type().is_symlink() || metadata.is_file() {
            fs::remove_file(&node.path)?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Hangar lease `{}` is not removable", node.path.display()),
            ));
        }
        swept += 1;
    }
    Ok(swept)
}

fn lease_nodes_unlocked(roots: &Roots) -> std::io::Result<Vec<LeaseNode>> {
    let leases = roots.root.join("leases");
    let metadata = match fs::symlink_metadata(&leases) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Hangar lease directory is not a real directory; repair the path before recovery",
        ));
    }
    let mut nodes = Vec::new();
    for entry in fs::read_dir(&leases)? {
        let entry = entry?;
        let name = entry.file_name();
        let text = name.to_str().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Hangar lease name is not valid UTF-8",
            )
        })?;
        if text.len() > LEASE_NAME_MAX {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Hangar lease name exceeds the recovery bound",
            ));
        }
        if !lease_name_is_valid(text) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Hangar lease `{text}` has an invalid identity"),
            ));
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_dir() && !metadata.file_type().is_symlink() && !metadata.is_file() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Hangar lease `{}` is not a supported filesystem node", path.display()),
            ));
        }
        let stale = if metadata.is_dir() && !metadata.file_type().is_symlink() {
            let lock = path.join(".locks").join("live.lock");
            !matches!(
                super::RuntimePolicy::lock_state(&lock)?,
                super::RuntimePolicy::LockState::Held
            )
        } else {
            true
        };
        nodes.push(LeaseNode { path, stale });
    }
    Ok(nodes)
}

fn lease_name_is_valid(name: &str) -> bool {
    let mut fields = name.splitn(3, '-');
    let Some(pid) = fields.next().and_then(|value| value.parse::<u32>().ok()) else {
        return false;
    };
    let Some(_sequence) = fields.next().and_then(|value| value.parse::<u64>().ok()) else {
        return false;
    };
    let Some(entry_id) = fields.next() else {
        return false;
    };
    pid != 0 && !entry_id.is_empty()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheExpectation {
    pub identity: CacheIdentity,
    pub owned_output: Option<PathBuf>,
    pub allow_unsigned_local: bool,
}

pub enum RealizeRequest<'a> {
    Package {
        spec: &'a super::RefSpec::RefSpec,
        table: &'a super::RefSpec::SourceTable,
    },
    Adapter {
        plan: &'a jet_env_model::ModuleEval::AdapterPlan,
        table: &'a super::RefSpec::SourceTable,
        expectation: &'a CacheExpectation,
    },
}

/// Controls a source-only independent-root reproducibility certification.
///
/// A retry always starts both builds in fresh roots. The callback is polled
/// between build/promotion boundaries; a cancelled run never enters Hangar.
#[derive(Clone, Copy)]
pub struct IndependentRootOptions<'a> {
    pub retries: usize,
    pub cancelled: Option<&'a dyn Fn() -> bool>,
}

impl Default for IndependentRootOptions<'_> {
    fn default() -> Self {
        Self {
            retries: 1,
            cancelled: None,
        }
    }
}

/// The durable result of a successful independent-root certification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndependentRootCertification {
    pub entry: StoreEntry,
    pub action_key: String,
    pub attestation: String,
}

pub struct VerifiedRealization {
    entry: StoreEntry,
    source_state: super::Provider::SourceState,
    lease: CacheLease,
}

impl VerifiedRealization {
    pub fn metadata(&self) -> &StoreEntry {
        &self.entry
    }

    pub fn source_state(&self) -> super::Provider::SourceState {
        self.source_state
    }

    pub fn consumption_status(&self) -> &ConsumptionStatus {
        self.lease.status()
    }

    pub fn original_output(&self) -> &Path {
        self.lease.original_output()
    }

    pub fn original_reference(&self) -> &str {
        self.lease.original_reference()
    }

    pub(crate) fn into_parts(self) -> (StoreEntry, super::Provider::SourceState, CacheLease) {
        (self.entry, self.source_state, self.lease)
    }
}

#[derive(Debug)]
pub enum RealizeError {
    Provider(super::Provider::ProviderError),
    Store(std::io::Error),
    Integrity(IntegrityFailure),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrityFailure {
    pub package: String,
    pub version: String,
    pub expected: String,
    pub actual: String,
    pub reason: String,
    pub disposition: String,
    pub fix: String,
}

impl IntegrityFailure {
    pub fn what(&self) -> String {
        format!(
            "Integrity check failed for `{}` `{}` — expected `{}`, got `{}`.",
            self.package, self.version, self.expected, self.actual
        )
    }

    pub fn why(&self) -> String {
        format!(
            "The cached artifact failed {}. {}",
            self.reason, self.disposition
        )
    }

    pub fn fix(&self) -> &str {
        &self.fix
    }
}

pub fn report_integrity(theme: &super::Output::Theme, failure: &IntegrityFailure) {
    theme.error_coded("E2604", &failure.what(), &failure.why(), failure.fix());
}

/// Build the store id for a realization — human-readable `<name>-<version>`
/// first, then a short fingerprint of (ref, out) so two realizations of the
/// same name+version from different refs don't collide (D-PM1). The version
/// segment is dropped when unknown, leaving `<name>-<fp>`. Never hash-first:
/// identity for correctness is the lockfile, the dir name is for humans.
pub fn entry_id(name: &str, version: &str, reference: &str, out: &str) -> String {
    let fp = SHA256::sha256_hex(format!("{reference}\n{out}").as_bytes());
    let short = &fp[..12];
    if version.is_empty() {
        format!("{name}-{short}")
    } else {
        format!("{name}-{version}-{short}")
    }
}

/// Record (or refresh) a store entry; returns the entry with its id filled in.
#[cfg(test)]
pub fn record(
    roots: &Roots,
    name: &str,
    version: &str,
    reference: &str,
    out: &str,
    bin: &str,
    rlib: &str,
    envelope: &super::Envelope::Envelope,
) -> std::io::Result<StoreEntry> {
    record_verified(
        roots,
        name,
        version,
        reference,
        out,
        bin,
        rlib,
        envelope,
        &CacheIdentity::default(),
    )
}

#[cfg(test)]
pub fn record_verified(
    roots: &Roots,
    name: &str,
    version: &str,
    reference: &str,
    out: &str,
    bin: &str,
    rlib: &str,
    envelope: &super::Envelope::Envelope,
    cache_identity: &CacheIdentity,
) -> std::io::Result<StoreEntry> {
    record_verified_mode(
        roots,
        name,
        version,
        reference,
        out,
        bin,
        rlib,
        envelope,
        cache_identity,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
#[cfg(test)]
fn record_verified_mode(
    roots: &Roots,
    name: &str,
    version: &str,
    reference: &str,
    out: &str,
    bin: &str,
    rlib: &str,
    envelope: &super::Envelope::Envelope,
    cache_identity: &CacheIdentity,
    canonicalize_local: bool,
) -> std::io::Result<StoreEntry> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        let (out, bin, rlib) = if canonicalize_local {
            canonicalize_local_output_unlocked(roots, out, bin, rlib, &envelope.output_hash)?
        } else {
            (out.to_string(), bin.to_string(), rlib.to_string())
        };
        let id = entry_id(name, version, reference, &out);
        let dir = roots.hangar_dir().join(&id);
        let now = now_secs();
        let realized_at = read_meta(&dir).and_then(|m| m.realized_at).unwrap_or(now);
        let mut producer = ProducerRecord::decode(&canonical_producer(
            "store-record",
            &format!("cas:{}", cache_identity.source_fingerprint),
            &envelope.output_hash,
            cache_identity,
            BTreeMap::from([("reference".into(), reference.to_string())]),
        )?)
        .map_err(std::io::Error::other)?;
        producer.bind_cache_provenance(reference, &envelope.output_hash, cache_identity, &[]);
        super::Provider::refresh_provider_facts(&mut producer, reference)
            .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
        let mut entry = StoreEntry {
            id: id.clone(),
            name: name.to_string(),
            version: version.to_string(),
            reference: reference.to_string(),
            out: out.clone(),
            bin,
            rlib,
            envelope: envelope.clone(),
            cache_identity: cache_identity.clone(),
            references: Vec::new(),
            named_outputs: BTreeMap::new(),
            platform_artifact_kind: String::new(),
            producer_record: producer.encode(),
            receipt: String::new(),
            realized_at,
            last_used_at: now,
        };
        let created_dir = !dir.exists();
        let gc_root = dir.join(NIX_GC_ROOT);
        let had_gc_root = fs::symlink_metadata(&gc_root).is_ok();
        fs::create_dir_all(&dir)?;
        let registration = (|| {
            pin_nix_gc_root(&dir, &out)?;
            Closure::prepare_entry_receipt(roots, &mut entry)?;
            register_entry_unlocked(roots, &entry)
        })();
        if let Err(error) = registration {
            Closure::rollback_registration_dir(&dir, created_dir, had_gc_root)?;
            return Err(error);
        }
        Ok(entry)
    })
}

pub(crate) fn record_realized_mode(
    roots: &Roots,
    realized: &super::Provider::Realized,
) -> std::io::Result<StoreEntry> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        record_realized_mode_unlocked(roots, realized, None)
    })
}

pub(crate) fn record_realized_mode_with_fresh_agreement(
    roots: &Roots,
    action_key: &str,
    realized: &super::Provider::Realized,
) -> std::io::Result<StoreEntry> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        record_realized_mode_unlocked(roots, realized, Some(action_key))
    })
}

fn record_realized_mode_unlocked(
    roots: &Roots,
    realized: &super::Provider::Realized,
    fresh_action_key: Option<&str>,
) -> std::io::Result<StoreEntry> {
    ProducerRecord::decode(&realized.producer.encode()).map_err(std::io::Error::other)?;
    super::Provider::validate_nix_build_facts(&realized.producer)?;
    let graph = Closure::closure_graph_structure_unlocked(roots)?;
    Closure::validate_universe_references(
        &realized.producer.provider,
        &realized.references,
        &graph,
    )
    .map_err(std::io::Error::other)?;
    // Capture named-output identities before projection may move a local
    // staging path into Hangar. The bytes, not the transient source spelling,
    // are the durable output facts.
    let mut named_outputs = BTreeMap::new();
    for (name, path) in &realized.named_outputs {
        let digest = super::Envelope::try_output_hash_of(path).map_err(std::io::Error::other)?;
        if name == "out" && digest != realized.envelope.output_hash {
            return Err(std::io::Error::other(format!(
                "Nix primary output changed during Store registration: expected {}, got {digest}",
                realized.envelope.output_hash
            )));
        }
        named_outputs.insert(name.clone(), digest);
    }
    let (out, bin, rlib) = if realized.producer.provider == "nix" {
        project_nix_outputs_unlocked(roots, realized)?
    } else {
        canonicalize_local_output_unlocked(
            roots,
            &realized.out,
            &realized.bin,
            &realized.rlib,
            &realized.envelope.output_hash,
        )?
    };
    named_outputs.insert("out".into(), realized.envelope.output_hash.clone());
    let id = entry_id(&realized.name, &realized.version, &realized.reference, &out);
    let dir = roots.hangar_dir().join(&id);
    let now = now_secs();
    let realized_at = read_meta(&dir)
        .and_then(|meta| meta.realized_at)
        .unwrap_or(now);
    let mut producer = realized.producer.clone();
    if producer.provider == "nix" {
        // D-JPK-NIXSTORE1: the provider's canonical path remains a
        // durable fact, but the bytes and closure are now Hangar-owned.
        // This is the boundary that prevents a raw host `/nix/store`
        // path from entering a reusable Store record.
        producer
            .facts
            .insert("closure.authority".into(), "hangar-cas".into());
        producer
            .facts
            .insert("nix.projection.authority".into(), "hangar-cas".into());
        producer
            .facts
            .insert("nix.projection.mode".into(), "canonical-hangar".into());
    }
    producer.bind_cache_provenance(
        &realized.reference,
        &realized.envelope.output_hash,
        &realized.cache_identity,
        &realized.references,
    );
    super::Provider::refresh_provider_facts(&mut producer, &realized.reference)
        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
    let mut entry = StoreEntry {
        id,
        name: realized.name.clone(),
        version: realized.version.clone(),
        reference: realized.reference.clone(),
        out: out.clone(),
        bin,
        rlib,
        envelope: realized.envelope.clone(),
        cache_identity: realized.cache_identity.clone(),
        references: realized.references.clone(),
        named_outputs,
        platform_artifact_kind: String::new(),
        producer_record: producer.encode(),
        receipt: String::new(),
        realized_at,
        last_used_at: now,
    };
    let created_dir = !dir.exists();
    let gc_root = dir.join(NIX_GC_ROOT);
    let had_gc_root = fs::symlink_metadata(&gc_root).is_ok();
    fs::create_dir_all(&dir)?;
    let registration = (|| {
        pin_nix_gc_root(&dir, &out)?;
        Closure::prepare_entry_receipt(roots, &mut entry)?;
        if let Some(action_key) = fresh_action_key {
            Closure::register_entry_unlocked_after_fresh_agreement(roots, &entry, action_key)
        } else {
            register_entry_unlocked(roots, &entry)
        }
    })();
    if let Err(error) = registration {
        Closure::rollback_registration_dir(&dir, created_dir, had_gc_root)?;
        return Err(error);
    }
    Ok(entry)
}

/// Project every Nix output into the Hangar CAS before Store registration.
/// The original `/nix/store` spelling is retained in the producer facts for
/// runtime namespace projection; it is never used as the durable output root.
fn project_nix_outputs_unlocked(
    roots: &Roots,
    realized: &super::Provider::Realized,
) -> std::io::Result<(String, String, String)> {
    let mut projected = BTreeMap::new();
    let mut seen_sources: BTreeMap<String, String> = BTreeMap::new();
    for (name, source) in &realized.named_outputs {
        let digest = super::Envelope::try_output_hash_of(source).map_err(std::io::Error::other)?;
        if name == "out" && digest != realized.envelope.output_hash {
            return Err(std::io::Error::other(format!(
                "Nix primary output changed during Store projection: expected {}, got {digest}",
                realized.envelope.output_hash
            )));
        }
        let canonical = if let Some(existing) = seen_sources.get(source) {
            existing.clone()
        } else {
            let canonical = project_nix_output_unlocked(roots, source, &digest)?;
            seen_sources.insert(source.clone(), canonical.clone());
            canonical
        };
        projected.insert(name.clone(), canonical);
    }

    let primary_name = if projected.contains_key("out") {
        "out"
    } else {
        "bin"
    };
    let primary = projected.get(primary_name).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Nix provider returned no projected primary output",
        )
    })?;
    let remap = |member: &str| {
        if member.is_empty() {
            return String::new();
        }
        for (name, source) in &realized.named_outputs {
            if let Ok(relative) = Path::new(member).strip_prefix(source) {
                if let Some(destination) = projected.get(name) {
                    return Path::new(destination)
                        .join(relative)
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }
        member.to_string()
    };
    Ok((primary.clone(), remap(&realized.bin), remap(&realized.rlib)))
}

fn project_nix_output_unlocked(
    roots: &Roots,
    source: &str,
    digest: &str,
) -> std::io::Result<String> {
    let source_path = Path::new(source);
    if source_path.starts_with(roots.hangar_dir()) {
        let (out, _, _) = canonicalize_local_output_unlocked(roots, source, "", "", digest)?;
        return Ok(out);
    }
    project_external_output_unlocked(roots, source_path, digest)
}

/// Copy an external Nix output into the Hangar CAS without mutating the host
/// store. The no-follow ingest and content re-hash are the authority; a
/// missing/unreadable output fails the realization instead of leaving a raw
/// external path behind.
fn project_external_output_unlocked(
    roots: &Roots,
    source: &Path,
    digest: &str,
) -> std::io::Result<String> {
    if digest.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Nix output projection has an empty content digest",
        ));
    }
    let hangar = roots.hangar_dir();
    let objects = hangar.join(OBJECTS_DIR);
    Ingest::ensure_real_directory(&objects, "Hangar object pool")?;
    let destination = objects.join(digest);
    let verify = |path: &Path| -> std::io::Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::other(format!(
                "Nix output projection `{}` is a symlink",
                path.display()
            )));
        }
        seal_node(path)?;
        let actual =
            super::Envelope::try_output_hash_of_in_hangar(&path.to_string_lossy(), &hangar, false)
                .map_err(std::io::Error::other)?;
        if actual != digest {
            return Err(std::io::Error::other(format!(
                "Nix output `{}` re-hashed as `{actual}`, expected `{digest}`",
                source.display()
            )));
        }
        fsync_tree(path)?;
        Ok(())
    };

    match fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(std::io::Error::other(format!(
                "Hangar object `{digest}` is a symlink"
            )))
        }
        Ok(_) => {
            verify(&destination)?;
            sync_store_directory(&objects)?;
            return Ok(destination.to_string_lossy().into_owned());
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let stage = objects.join(format!(
        ".nix-projection-{}-{}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        Ingest::copy_nofollow_tree(source, &stage).map_err(|error| {
            std::io::Error::other(format!(
                "copying Nix output `{}` into Hangar failed: {}",
                source.display(),
                error.what()
            ))
        })?;
        verify(&stage)?;
        ensure_hangar_capacity(
            roots,
            admission_reservation(admission_size(&stage)?),
            Some(&stage),
        )?;
        sync_store_directory(&objects)?;
        fs::rename(&stage, &destination)?;
        sync_store_directory(&objects)?;
        Ok(destination.to_string_lossy().into_owned())
    })();
    if result.is_err() {
        if let Ok(metadata) = fs::symlink_metadata(&stage) {
            let _ = if metadata.is_dir() {
                let _ = make_tree_writable_for_removal(&stage);
                fs::remove_dir_all(&stage)
            } else {
                fs::remove_file(&stage)
            };
        }
    }
    result
}

fn canonicalize_local_output_unlocked(
    roots: &Roots,
    out: &str,
    bin: &str,
    rlib: &str,
    digest: &str,
) -> std::io::Result<(String, String, String)> {
    let source = Path::new(out);
    if digest.is_empty() || source.starts_with("/nix/store") {
        return Ok((out.to_string(), bin.to_string(), rlib.to_string()));
    }
    if !source.starts_with(roots.hangar_dir()) {
        let actual = super::Envelope::try_output_hash_of(out).map_err(std::io::Error::other)?;
        if actual != digest {
            return Err(std::io::Error::other(format!(
                "provider output `{out}` re-hashed as `{actual}`, expected `{digest}`"
            )));
        }
        return Err(std::io::Error::other(format!(
            "provider output `{out}` is outside Hangar and `/nix/store`"
        )));
    }
    let objects = roots.hangar_dir().join(OBJECTS_DIR);
    let destination = objects.join(digest);
    Ingest::ensure_real_directory(&objects, "Hangar object pool").map_err(|error| {
        std::io::Error::new(
            error.kind(),
            format!("creating canonical object directory: {error}"),
        )
    })?;
    if source.starts_with(&objects) {
        if source != destination {
            return Err(std::io::Error::other(format!(
                "canonical object path `{}` disagrees with digest `{digest}`",
                source.display()
            )));
        }
        seal_node(&destination)?;
        let actual = super::Envelope::try_output_hash_of_in_hangar(
            &destination.to_string_lossy(),
            &roots.hangar_dir(),
            false,
        )
        .map_err(std::io::Error::other)?;
        if actual != digest {
            return Err(std::io::Error::other(format!(
                "canonical object `{digest}` re-hashed as `{actual}`"
            )));
        }
        fsync_tree(&destination)?;
        sync_store_directory(&objects)?;
        return Ok((out.to_string(), bin.to_string(), rlib.to_string()));
    }
    if destination.exists() {
        seal_node(&destination)?;
        let actual = super::Envelope::try_output_hash_of_in_hangar(
            &destination.to_string_lossy(),
            &roots.hangar_dir(),
            false,
        )
        .map_err(std::io::Error::other)?;
        if actual != digest {
            return Err(std::io::Error::other(format!(
                "canonical object `{digest}` re-hashed as `{actual}`"
            )));
        }
        fsync_tree(&destination)?;
        sync_store_directory(&objects)?;
        if source != destination && source.exists() {
            make_tree_writable_for_removal(source)?;
            fs::remove_dir_all(source).map_err(|error| {
                std::io::Error::new(
                    error.kind(),
                    format!("removing duplicate provider output: {error}"),
                )
            })?;
        }
    } else {
        // Provider outputs are sealed before this registration boundary. Some
        // tier-1 filesystems deny renaming a read-only directory, so reopen it
        // only while the Hangar transaction lock is held, publish, then seal
        // the canonical path again before metadata becomes visible.
        ensure_hangar_capacity(
            roots,
            admission_reservation(admission_size(source)?),
            Some(source),
        )?;
        make_tree_writable_for_removal(source)?;
        let source_parent = source.parent().map(Path::to_path_buf);
        fs::rename(source, &destination).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("publishing canonical provider output: {error}"),
            )
        })?;
        if let Some(parent) = source_parent {
            sync_store_directory(&parent)?;
        }
        seal_node(&destination)?;
        let actual = super::Envelope::try_output_hash_of_in_hangar(
            &destination.to_string_lossy(),
            &roots.hangar_dir(),
            false,
        )
        .map_err(std::io::Error::other)?;
        if actual != digest {
            return Err(std::io::Error::other(format!(
                "canonical object `{digest}` re-hashed as `{actual}`"
            )));
        }
        fsync_tree(&destination)?;
        sync_store_directory(&objects).map_err(|error| {
            std::io::Error::new(
                error.kind(),
                format!("syncing canonical object directory: {error}"),
            )
        })?;
    }
    let remap = |member: &str| {
        if member.is_empty() {
            return String::new();
        }
        Path::new(member)
            .strip_prefix(source)
            .ok()
            .map(|relative| destination.join(relative).to_string_lossy().into_owned())
            .unwrap_or_else(|| member.to_string())
    };
    Ok((
        destination.to_string_lossy().into_owned(),
        remap(bin),
        remap(rlib),
    ))
}

const NIX_GC_ROOT: &str = "nix-gc-root";

/// Reject a raw `/nix/store` registration that bypassed the native projection.
/// Jetpack never asks `nix-store` to create a root; projected Nix outputs use
/// the normal Hangar closure proof below.
fn pin_nix_gc_root(_entry_dir: &Path, out: &str) -> std::io::Result<()> {
    let out_path = Path::new(out);
    if out_path.starts_with("/nix/store") {
        return Err(std::io::Error::other(format!(
            "Nix compatibility output `{out}` needs a verified native store authority"
        )));
    }
    Ok(())
}

/// Single realization boundary for every product consumer. Cache reuse,
/// quarantine, provider execution, and recording cannot be bypassed by CLI or
/// JetOS callers.
pub fn realize_verified(
    roots: &Roots,
    ctx: &super::Provider::Ctx<'_>,
    request: RealizeRequest<'_>,
) -> Result<VerifiedRealization, RealizeError> {
    if let RealizeRequest::Package { spec, .. } = &request {
        enforce_manifest_provenance_floor(ctx.project_dir, &spec.package)?;
    }
    // WAL is authority. Recover package projections before cache verification;
    // selected-candidate proofs run below so invalid bytes can be quarantined.
    Closure::closure_graph_structure(roots).map_err(RealizeError::Store)?;
    let (reference, expectation) = match &request {
        RealizeRequest::Package { spec, table } => {
            super::Provider::validate_cache_authority(spec, table, ctx)
                .map_err(RealizeError::Provider)?;
            (
                spec.raw.clone(),
                super::Provider::cache_expectation(spec, table, ctx),
            )
        }
        RealizeRequest::Adapter {
            plan, expectation, ..
        } => (
            format!("adapt:{}:{}", plan.name, plan.source),
            Some((*expectation).clone()),
        ),
    };

    // A missing bindings directory means "no configured cache". A present
    // but malformed or unreadable binding is trust state, not a cache miss;
    // fail before any local, remote, or newly built result becomes usable.
    let cache_bindings = list_cache_bindings(roots).map_err(RealizeError::Store)?;
    let indexed_nix_repair = match &request {
        RealizeRequest::Package { spec, table } => {
            super::Provider::can_repair_indexed_nix(spec, table, ctx)
        }
        RealizeRequest::Adapter { .. } => false,
    };

    if let (Some(candidate), Some(expectation)) =
        (find_by_reference(roots, &reference), expectation.as_ref())
    {
        match find_verified_by_reference(roots, &reference, expectation)
            .map_err(RealizeError::Store)?
        {
            Some(hit) => {
                if let RealizeRequest::Adapter { plan, table, .. } = &request {
                    validate_cached_adapter_hook(&hit.entry, plan, table, expectation)
                        .map_err(RealizeError::Store)?;
                }
                project_receipt_projection(ctx, &hit.entry)?;
                return Ok(VerifiedRealization {
                    entry: hit.entry,
                    source_state: super::Provider::SourceState::Cached,
                    lease: hit.lease,
                });
            }
            None => {
                let proof = verify_cache_entry(roots, &candidate, &reference, expectation);
                if !cache_bindings.is_empty()
                    && try_substitute_invalid_candidate(
                        roots,
                        &candidate,
                        expectation,
                        &cache_bindings,
                    )
                    .map_err(RealizeError::Store)?
                {
                    if let Some(hit) = find_verified_by_reference(roots, &reference, expectation)
                        .map_err(RealizeError::Store)?
                    {
                        if let RealizeRequest::Adapter { plan, table, .. } = &request {
                            validate_cached_adapter_hook(&hit.entry, plan, table, expectation)
                                .map_err(RealizeError::Store)?;
                        }
                        project_receipt_projection(ctx, &hit.entry)?;
                        return Ok(VerifiedRealization {
                            entry: hit.entry,
                            source_state: super::Provider::SourceState::Substituted,
                            lease: hit.lease,
                        });
                    }
                }

                if cache_bindings.is_empty() && !indexed_nix_repair {
                    let mut failure = integrity_failure(roots, &candidate, expectation, proof);
                    if let Err(error) = quarantine_invalid_entry(roots, &candidate, expectation) {
                        failure.actual = format!("{}; quarantine failed: {error}", failure.actual);
                    }
                    return Err(RealizeError::Integrity(failure));
                }

                if let Err(error) = quarantine_invalid_entry(roots, &candidate, expectation) {
                    let mut failure = integrity_failure(roots, &candidate, expectation, proof);
                    failure.actual = format!("{}; quarantine failed: {error}", failure.actual);
                    return Err(RealizeError::Integrity(failure));
                }
            }
        }
    }

    if let Some(expectation) = expectation.as_ref() {
        if reuse_shared_entry(roots, &reference, expectation)
            .map_err(RealizeError::Store)?
            .is_some()
        {
            if let Some(hit) = find_verified_by_reference(roots, &reference, expectation)
                .map_err(RealizeError::Store)?
            {
                if let RealizeRequest::Adapter { plan, table, .. } = &request {
                    validate_cached_adapter_hook(&hit.entry, plan, table, expectation)
                        .map_err(RealizeError::Store)?;
                }
                project_receipt_projection(ctx, &hit.entry)?;
                return Ok(VerifiedRealization {
                    entry: hit.entry,
                    source_state: super::Provider::SourceState::Cached,
                    lease: hit.lease,
                });
            }
        }
    }

    let independent = Some(Reproducibility::build_for_cache(
        roots,
        ctx,
        &request,
        &IndependentRootOptions::default(),
        false,
    )?);
    let mut realized = if let Some(prepared) = independent.as_ref() {
        prepared.realized.clone()
    } else {
        Reproducibility::realize_uncached(roots, ctx, &request)?
    };
    if let Some(attestation) = independent
        .as_ref()
        .and_then(|prepared| prepared.attestation.as_ref())
    {
        realized
            .producer
            .facts
            .insert("cache.reproducibility".into(), attestation.clone());
    }
    // The provider snapshots its Nix lock facts before producing bytes. Check
    // that snapshot again immediately before the Store registration so a
    // concurrent lock edit cannot detach the producer record from the output
    // that is about to become reusable.
    super::Provider::validate_nix_lock_before_store(ctx, &realized)
        .map_err(RealizeError::Provider)?;
    let mut entry = if let Some(action_key) = independent
        .as_ref()
        .and_then(|prepared| prepared.action_key.as_deref())
    {
        record_realized_mode_with_fresh_agreement(roots, action_key, &realized)
    } else {
        record_realized_mode(roots, &realized)
    }
    .map_err(RealizeError::Store)?;
    entry = super::Provider::record_nix_lock_after_store(ctx, roots, &entry)
        .map_err(RealizeError::Provider)?;
    project_receipt_projection(ctx, &entry)?;
    if !is_private_untrusted_build(&realized.producer) {
        promote_shared_entry(roots, &entry).map_err(RealizeError::Store)?;
        if matches!(
            realized.source_state,
            super::Provider::SourceState::Built | super::Provider::SourceState::Downloaded
        ) {
            publish_realized_to_bound_caches(roots, &entry);
        }
    }
    let lease = snapshot_lease(roots, &entry).map_err(RealizeError::Store)?;
    if let Some(action_key) = independent
        .as_ref()
        .and_then(|prepared| prepared.action_key.as_deref())
    {
        Reproducibility::clear_reproducibility_report(roots, action_key)
            .map_err(RealizeError::Store)?;
    }
    Ok(VerifiedRealization {
        entry,
        source_state: realized.source_state,
        lease,
    })
}

/// Run and publish a fresh, source-only independent-root certification.
///
/// This entry point deliberately bypasses existing local/shared hits: both
/// provider executions start in private roots and the first result is not
/// promoted until the second result agrees on action, output, and provenance.
pub fn certify_independent_root_build(
    roots: &Roots,
    ctx: &super::Provider::Ctx<'_>,
    request: RealizeRequest<'_>,
    options: IndependentRootOptions<'_>,
) -> Result<IndependentRootCertification, RealizeError> {
    if let RealizeRequest::Package { spec, table } = &request {
        enforce_manifest_provenance_floor(ctx.project_dir, &spec.package)?;
        super::Provider::validate_cache_authority(spec, table, ctx)
            .map_err(RealizeError::Provider)?;
    }
    Closure::closure_graph_structure(roots).map_err(RealizeError::Store)?;
    let prepared = Reproducibility::build_for_cache(roots, ctx, &request, &options, true)?;
    let action_key = prepared.action_key.clone().ok_or_else(|| {
        RealizeError::Store(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "independent certification produced no action identity",
        ))
    })?;
    if options.cancelled.is_some_and(|cancelled| cancelled()) {
        return Err(RealizeError::Store(std::io::Error::new(
            std::io::ErrorKind::Interrupted,
            "independent reproducibility certification cancelled",
        )));
    }
    let attestation = prepared.attestation.clone().ok_or_else(|| {
        RealizeError::Store(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "independent certification has no agreeing attestation",
        ))
    })?;
    let mut realized = prepared.realized.clone();
    realized
        .producer
        .facts
        .insert("cache.reproducibility".into(), attestation.clone());
    super::Provider::validate_nix_lock_before_store(ctx, &realized)
        .map_err(RealizeError::Provider)?;
    let mut entry = record_realized_mode_with_fresh_agreement(roots, &action_key, &realized)
        .map_err(RealizeError::Store)?;
    entry = super::Provider::record_nix_lock_after_store(ctx, roots, &entry)
        .map_err(RealizeError::Provider)?;
    project_receipt_projection(ctx, &entry)?;
    if !is_private_untrusted_build(&realized.producer) {
        promote_shared_entry(roots, &entry).map_err(RealizeError::Store)?;
        publish_realized_to_bound_caches(roots, &entry);
    }
    Reproducibility::clear_reproducibility_report(roots, &action_key)
        .map_err(RealizeError::Store)?;
    Ok(IndependentRootCertification {
        entry,
        action_key,
        attestation,
    })
}

fn project_receipt_projection(
    ctx: &super::Provider::Ctx<'_>,
    entry: &StoreEntry,
) -> Result<(), RealizeError> {
    let Some(project) = ctx.project_dir.filter(|path| path.is_dir()) else {
        return Ok(());
    };
    if entry.receipt.is_empty() {
        return Err(RealizeError::Store(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Hangar entry `{}` has no connected receipt", entry.id),
        )));
    }
    let lock_path = project.join(crate::Syntax::UNIFIED_LOCK_FILE);
    if !validate_project_lock_path(&lock_path).map_err(RealizeError::Store)? {
        return Ok(());
    }
    super::RuntimePolicy::with_project_lock(project, "receipt-projection", || {
        if let Ok(producer) = ProducerRecord::decode(&entry.producer_record) {
            if producer.provider == "nix" {
                if let Some(expected) = producer.facts.get("nix.lock.digest") {
                    let current = super::Provider::project_lock_digest(ctx.project_dir)
                        .map_err(|error| std::io::Error::other(format!("{error:?}")))?;
                    if &current != expected {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!(
                                "Nix project lock changed before receipt projection: prepared `{expected}`, current `{current}`"
                            ),
                        ));
                    }
                }
            }
        }
        record_receipt_projection(
            project,
            &entry.name,
            &entry.version,
            &entry.reference,
            &entry.envelope.output_hash,
            &entry.receipt,
        )
        .map_err(std::io::Error::other)
    })
    .map(|_| ())
    .map_err(RealizeError::Store)
}

fn record_receipt_projection(
    project_root: &Path,
    package_name: &str,
    package_version: &str,
    reference: &str,
    output_hash: &str,
    receipt: &str,
) -> std::io::Result<bool> {
    if !valid_receipt_digest(receipt) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("invalid Hangar receipt digest `{receipt}`"),
        ));
    }
    if output_hash.is_empty() && reference.is_empty() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Hangar receipt projection has no package identity",
        ));
    }
    let lock_path = project_root.join(crate::Syntax::UNIFIED_LOCK_FILE);
    if !validate_project_lock_path(&lock_path)? {
        return Ok(false);
    }
    let raw = match fs::read_to_string(&lock_path) {
        Ok(raw) => raw,
        Err(error) => return Err(error),
    };
    let mut lock = crate::Lock::parse(&raw).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "could not parse project lock `{}`: {error}",
                lock_path.display()
            ),
        )
    })?;
    let package_index = lock
        .packages
        .iter()
        .position(|package| {
            package.name == package_name
                && package.version == package_version
                && receipt_source_matches(package, reference)
                && receipt_projection_envelope_matches(package, output_hash)
        })
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Hangar receipt `{receipt}` has no matching package in project lock `{}`",
                    lock_path.display()
                ),
            )
        })?;
    let package = &mut lock.packages[package_index];
    if package.receipt.as_deref() == Some(receipt) {
        return Ok(false);
    }
    package.receipt = Some(receipt.to_string());
    crate::Lock::ensure_build_stamp(project_root, &mut lock);
    write_project_lock_atomically(&lock_path, &crate::Lock::write(&lock))?;
    Ok(true)
}

fn validate_project_lock_path(path: &Path) -> std::io::Result<bool> {
    let managed = path
        .parent()
        .ok_or_else(|| std::io::Error::other("project lock has no managed directory"))?;
    match fs::symlink_metadata(managed) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "project managed directory `{}` is not a real directory",
                    managed.display()
                ),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("project lock `{}` is not a real file", path.display()),
            ))
        }
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

fn receipt_source_matches(package: &super::Lock::LockedPackage, reference: &str) -> bool {
    match &package.source {
        super::Lock::LockSource::Root => reference == "." || reference == "root",
        super::Lock::LockSource::Path(path) => path == reference,
        super::Lock::LockSource::Git { url, .. } => url == reference,
        super::Lock::LockSource::Nix {
            reference: value, ..
        } => super::RefSpec::canonical_locked_ref(value)
            == super::RefSpec::canonical_locked_ref(reference),
        super::Lock::LockSource::Cran {
            reference: value, ..
        }
        | super::Lock::LockSource::LuaRocks {
            reference: value, ..
        }
        | super::Lock::LockSource::Registry {
            reference: value, ..
        } => value == reference,
        // Only a foreign source carries a language, so it cannot share the
        // or-pattern above: it also matches the `language@reference` spelling.
        super::Lock::LockSource::Foreign {
            language,
            reference: value,
            ..
        } => value == reference || format!("{}@{}", language.root(), value) == reference,
    }
}

fn receipt_envelope_matches(package: &super::Lock::LockedPackage, output_hash: &str) -> bool {
    package
        .envelope
        .as_ref()
        .is_none_or(|envelope| output_hash.is_empty() || envelope.output_hash == output_hash)
}

fn receipt_projection_envelope_matches(
    package: &super::Lock::LockedPackage,
    output_hash: &str,
) -> bool {
    package
        .envelope
        .as_ref()
        .is_some_and(|envelope| !output_hash.is_empty() && envelope.output_hash == output_hash)
}

fn valid_receipt_digest(value: &str) -> bool {
    let Some(hex) = value.strip_prefix("sha256-") else {
        return false;
    };
    hex.len() == 64
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn write_project_lock_atomically(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write as _;

    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("project lock has no parent directory"))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| std::io::Error::other("project lock has no UTF-8 file name"))?;
    let mut temporary = None;
    for attempt in 0..32u32 {
        let candidate = parent.join(format!(
            ".{file_name}.{}.partial",
            std::process::id() + attempt
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(mut file) => {
                if let Err(error) = file
                    .write_all(contents.as_bytes())
                    .and_then(|()| file.sync_all())
                {
                    let _ = fs::remove_file(&candidate);
                    return Err(error);
                }
                temporary = Some(candidate);
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    let temporary = temporary.ok_or_else(|| {
        std::io::Error::other("could not allocate a temporary project lock path after 32 attempts")
    })?;
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }
    sync_store_directory(parent)
}

/// Try every host-owned cache role before a bad local candidate is rebuilt.
/// Cache bindings are optional: a machine with no binding keeps the existing
/// fail-closed local-integrity behavior. A bound but unavailable/corrupt cache
/// is a miss, so the deterministic provider path remains the source fallback.
fn try_substitute_invalid_candidate(
    roots: &Roots,
    candidate: &StoreEntry,
    expectation: &CacheExpectation,
    bindings: &[CacheBinding],
) -> std::io::Result<bool> {
    let output = Path::new(&candidate.out);
    if candidate.envelope.output_hash.is_empty()
        || candidate.envelope.output_hash.contains('/')
        || output
            != roots
                .hangar_dir()
                .join(OBJECTS_DIR)
                .join(&candidate.envelope.output_hash)
    {
        return Ok(false);
    }

    static NEXT_STAGE: AtomicU64 = AtomicU64::new(0);
    let objects = roots.hangar_dir().join(OBJECTS_DIR);
    let stage = objects.join(format!(
        ".substitute-{}-{}",
        std::process::id(),
        NEXT_STAGE.fetch_add(1, Ordering::Relaxed)
    ));
    // Keep the verified private stage inside the Hangar object directory. The
    // production policy permits atomic publication within that sealed tree;
    // crossing from the cache scratch tree would be rejected by the host.
    let mut permissions = Ingest::MovePathPermissions::default();
    permissions.make_writable(&objects, &roots.hangar_dir())?;
    let mut substituted = false;
    for binding in bindings {
        discard_substitution_stage(&stage)?;
        if substitute_cache_entry(roots, &candidate.id, &binding.role, &stage).is_ok() {
            substituted = true;
            break;
        }
    }
    if !substituted {
        discard_substitution_stage(&stage)?;
        return Ok(false);
    }

    // The remote bytes are already verified and staged. Remove the invalid
    // local projection only after that proof; then publish the complete tree
    // at its canonical CAS path and re-register the original metadata. This
    // keeps the local action identity, closure edges, capabilities, and
    // provenance unchanged.
    if let Err(error) = quarantine_invalid_entry(roots, candidate, expectation) {
        discard_substitution_stage(&stage)?;
        return Err(error);
    }
    let restored = restore_substituted_candidate(roots, candidate, &stage)?;
    if !restored {
        discard_substitution_stage(&stage)?;
    }
    Ok(restored)
}

fn restore_substituted_candidate(
    roots: &Roots,
    candidate: &StoreEntry,
    stage: &Path,
) -> std::io::Result<bool> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        if list_unlocked(roots)?
            .into_iter()
            .any(|entry| entry.id == candidate.id)
        {
            return Ok(false);
        }

        let output = Path::new(&candidate.out);
        let metadata = fs::symlink_metadata(output);
        let output_is_valid = metadata
            .as_ref()
            .is_ok_and(|metadata| !metadata.file_type().is_symlink())
            && Ingest::try_entry_output_hash(roots, candidate)
                .is_ok_and(|actual| actual == candidate.envelope.output_hash);
        if !output_is_valid {
            if metadata.is_ok() {
                return Ok(false);
            }
            let parent = output
                .parent()
                .ok_or_else(|| std::io::Error::other("substituted output has no parent"))?;
            Ingest::ensure_real_directory(parent, "Hangar object directory")?;
            let mut permissions = Ingest::MovePathPermissions::default();
            permissions.make_writable(parent, &roots.hangar_dir())?;
            let operation = (|| {
                fs::rename(stage, output)?;
                sync_store_directory(parent)
            })();
            let restored = permissions.restore();
            match (operation, restored) {
                (Ok(()), Ok(())) => {}
                (Err(error), Ok(())) => return Err(error),
                (Ok(()), Err(error)) => return Err(error),
                (Err(error), Err(restore)) => {
                    return Err(std::io::Error::other(format!(
                        "{error}; restoring Hangar permissions failed: {restore}"
                    )))
                }
            }
        }

        let mut entry = candidate.clone();
        entry.last_used_at = now_secs();
        let dir = roots.hangar_dir().join(&entry.id);
        let created_dir = !dir.exists();
        let gc_root = dir.join(NIX_GC_ROOT);
        let had_gc_root = fs::symlink_metadata(&gc_root).is_ok();
        fs::create_dir_all(&dir)?;
        let registration = (|| {
            pin_nix_gc_root(&dir, &entry.out)?;
            Closure::register_entry_unlocked(roots, &entry)
        })();
        if let Err(error) = registration {
            Closure::rollback_registration_dir(&dir, created_dir, had_gc_root)?;
            return Err(error);
        }
        Ok(true)
    })
}

fn discard_substitution_stage(path: &Path) -> std::io::Result<()> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if metadata.file_type().is_symlink() || metadata.is_file() {
        return fs::remove_file(path);
    }
    if metadata.is_dir() {
        Cache::make_tree_writable_for_removal(path)?;
        fs::remove_dir_all(path)
    } else {
        Err(std::io::Error::other("substitution stage is not removable"))
    }
}

/// Publication is best-effort cache acceleration. The binding's write grant
/// authorizes the operation; a failed mirror never turns a successful source
/// build into a failed realization.
fn publish_realized_to_bound_caches(roots: &Roots, entry: &StoreEntry) {
    let Ok(producer) = ProducerRecord::decode(&entry.producer_record) else {
        return;
    };
    if is_private_untrusted_build(&producer) {
        return;
    }
    let Ok(bindings) = list_cache_bindings(roots) else {
        return;
    };
    for binding in bindings.into_iter().filter(|binding| binding.allow_write) {
        let _ = publish_cache_entry(roots, &entry.id, &binding.role);
    }
}

/// Resolve every declared adapter dependency through the normal verified-store
/// boundary and expose its exact executable members to the recipe. Keeping the
/// leases alive for the duration of the adapter build prevents a dependency
/// snapshot from disappearing while the child process is using it.
fn realize_adapter_tools(
    roots: &Roots,
    ctx: &super::Provider::Ctx<'_>,
    plan: &jet_env_model::ModuleEval::AdapterPlan,
    table: &super::RefSpec::SourceTable,
) -> Result<(HashMap<String, PathBuf>, Vec<CacheLease>), RealizeError> {
    let mut tools = HashMap::new();
    let mut dependency_leases = Vec::new();

    for dependency in &plan.deps {
        let raw = jet_env_model::ModuleEval::pkg_ref(dependency);
        let spec =
            if dependency.source.is_empty() || dependency.source == crate::Syntax::DEFAULT_SOURCE {
                // `default` is the typed surface's name for the built-in Jetpack
                // provider. It is not a named SourceTable entry.
                super::RefSpec::RefSpec {
                    source: super::RefSpec::Source::Jetpack,
                    package: dependency.name.clone(),
                    raw: super::RefSpec::with_default_source(&dependency.name),
                }
            } else {
                super::RefSpec::classify_in(&raw, table).map_err(|error| {
                    RealizeError::Provider(super::Provider::ProviderError::Adapter(format!(
                        "build dependency `{raw}` is not a resolvable package ref: {error:?}"
                    )))
                })?
            };
        let realized =
            realize_verified(roots, ctx, RealizeRequest::Package { spec: &spec, table })?;
        let (_entry, _state, lease) = realized.into_parts();
        let receipt = lease.profile_install_receipt().map_err(|error| {
            RealizeError::Provider(super::Provider::ProviderError::Adapter(format!(
                "build dependency `{}` has no verified executable output: {error}",
                dependency.name
            )))
        })?;
        for member in receipt.executable_members {
            let path = lease.executable(&member).ok_or_else(|| {
                RealizeError::Provider(super::Provider::ProviderError::Adapter(format!(
                    "build dependency `{}` lost executable `{member}` from its verified lease",
                    dependency.name
                )))
            })?;
            if tools.insert(member.clone(), path).is_some() {
                return Err(RealizeError::Provider(
                    super::Provider::ProviderError::Adapter(format!(
                        "build dependencies provide the same executable `{member}`"
                    )),
                ));
            }
        }
        dependency_leases.push(lease);
    }

    Ok((tools, dependency_leases))
}

fn integrity_failure(
    roots: &Roots,
    entry: &StoreEntry,
    expectation: &CacheExpectation,
    proof: CacheVerification,
) -> IntegrityFailure {
    let (reason, expected, actual) = if !proof.output_exists {
        (
            "output existence",
            entry.envelope.output_hash.clone(),
            "missing output".to_string(),
        )
    } else if !proof.output_digest {
        (
            "content digest verification",
            entry.envelope.output_hash.clone(),
            Ingest::try_entry_output_hash(roots, entry)
                .unwrap_or_else(|error| format!("unreadable output: {error}")),
        )
    } else if !proof.source {
        (
            "source identity verification",
            expectation.identity.source_fingerprint.clone(),
            entry.cache_identity.source_fingerprint.clone(),
        )
    } else if !proof.recipe {
        (
            "recipe identity verification",
            expectation.identity.recipe_fingerprint.clone(),
            entry.cache_identity.recipe_fingerprint.clone(),
        )
    } else if !proof.platform {
        (
            "platform verification",
            expectation.identity.platform.clone(),
            entry.cache_identity.platform.clone(),
        )
    } else if !proof.policy {
        (
            "policy identity verification",
            expectation.identity.policy_fingerprint.clone(),
            entry.cache_identity.policy_fingerprint.clone(),
        )
    } else if !proof.signature_verified && !proof.unsigned_local_allowed {
        (
            "signature verification",
            "a trusted cache signature or an exact Hangar-owned local output".to_string(),
            if entry.envelope.signature.is_empty() {
                "unsigned non-local artifact".to_string()
            } else {
                "untrusted signature".to_string()
            },
        )
    } else {
        (
            "closure containment verification",
            "canonical closure members strictly below the output root".to_string(),
            "closure member escaped or disappeared".to_string(),
        )
    };
    IntegrityFailure {
        package: entry.name.clone(),
        version: entry.version.clone(),
        expected,
        actual,
        reason: reason.to_string(),
        disposition: "Jetpack quarantined it instead of using or silently repairing it."
            .to_string(),
        fix: "Re-run `jet store fetch` after `jet clean`. If the problem persists, audit the source before rebuilding."
            .to_string(),
    }
}

fn verify_configured_signature(
    roots: &Roots,
    entry: &StoreEntry,
    expectation: &CacheExpectation,
) -> bool {
    let Ok(public_key) = fs::read_to_string(roots.root.join("trust/cache.ed25519.pub")) else {
        return false;
    };
    let Some(public_key) = decode_ed25519::<32>(&public_key) else {
        return false;
    };
    let signature = entry
        .envelope
        .signature
        .strip_prefix("ed25519:")
        .unwrap_or(&entry.envelope.signature);
    let Some(signature) = decode_hex::<64>(signature) else {
        return false;
    };
    VerifyingKey::from_bytes(&public_key).is_ok_and(|key| {
        key.verify(
            cache_signature_message(entry, expectation).as_bytes(),
            &Signature::from_bytes(&signature),
        )
        .is_ok()
    })
}

fn decode_ed25519<const N: usize>(text: &str) -> Option<[u8; N]> {
    decode_hex(text.trim().strip_prefix("ed25519:").unwrap_or(text.trim()))
}

fn decode_hex<const N: usize>(text: &str) -> Option<[u8; N]> {
    if text.len() != N * 2 {
        return None;
    }
    let mut bytes = [0; N];
    for (byte, pair) in bytes.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        let digit = |value: u8| match value {
            b'0'..=b'9' => Some(value - b'0'),
            b'a'..=b'f' => Some(value - b'a' + 10),
            b'A'..=b'F' => Some(value - b'A' + 10),
            _ => None,
        };
        *byte = digit(pair[0])? << 4 | digit(pair[1])?;
    }
    Some(bytes)
}

#[cfg(test)]
fn verify_configured_signature_with(
    roots: &Roots,
    entry: &StoreEntry,
    expectation: &CacheExpectation,
    verifier: impl FnOnce(&str, &str, &str) -> bool,
) -> bool {
    let Ok(public_key) = fs::read_to_string(roots.root.join("trust/cache.ed25519.pub")) else {
        return false;
    };
    let signature = entry
        .envelope
        .signature
        .strip_prefix("ed25519:")
        .unwrap_or(&entry.envelope.signature);
    verifier(
        public_key.trim(),
        &cache_signature_message(entry, expectation),
        signature,
    )
}

fn cache_signature_message(entry: &StoreEntry, expectation: &CacheExpectation) -> String {
    format!(
        "jet-cache-v1\nreference={}\nsource={}\nrecipe={}\npolicy={}\nplatform={}\noutput={}\n",
        entry.reference,
        expectation.identity.source_fingerprint,
        expectation.identity.recipe_fingerprint,
        expectation.identity.policy_fingerprint,
        expectation.identity.platform,
        entry.envelope.output_hash,
    )
}

fn closure_is_reachable(roots: &Roots, entry: &StoreEntry) -> bool {
    let out = Path::new(&entry.out);
    if out.starts_with("/nix/store") {
        let root = roots.hangar_dir().join(&entry.id).join(NIX_GC_ROOT);
        return root.exists() && fs::canonicalize(&root).ok() == fs::canonicalize(out).ok();
    }
    let Ok(canonical_out) = fs::canonicalize(out) else {
        return false;
    };
    [&entry.bin, &entry.rlib].into_iter().all(|member| {
        if member.is_empty() {
            return true;
        }
        let member = Path::new(member);
        let Ok(canonical_member) = fs::canonicalize(member) else {
            return false;
        };
        canonical_member != canonical_out && canonical_member.starts_with(&canonical_out)
    })
}

/// Opaque receipt for one prepared profile-generation root. Profile producers
/// can commit only the exact incarnation and witness they prepared.
pub(crate) struct PreparedProfileGenerationRoot {
    id: Lifecycle::RootId,
    incarnation: Lifecycle::Incarnation,
    witness: Lifecycle::RootWitness,
}

pub(crate) fn prepare_profile_generation_root(
    roots: &Roots,
    owner: &str,
    profile: &str,
    generation: u64,
    witness: &str,
    targets: Vec<String>,
    at: u64,
) -> std::io::Result<PreparedProfileGenerationRoot> {
    let id = Lifecycle::RootId::new(format!("profile-generation:{owner}:{profile}:{generation}"))?;
    let incarnation = Lifecycle::Incarnation::new(1)?;
    let witness = Lifecycle::RootWitness::new(witness)?;
    let identity = Lifecycle::RootIdentity::new(
        Lifecycle::RootKind::ProfileGeneration,
        id.clone(),
        Lifecycle::ProducerId::new("jetpack-profile-generation")?,
        incarnation,
        witness.clone(),
    );
    Lifecycle::prepare(
        roots,
        identity,
        targets,
        Lifecycle::LifecycleTimestamp::from_unix_seconds(at),
    )?;
    Ok(PreparedProfileGenerationRoot {
        id,
        incarnation,
        witness,
    })
}

pub(crate) fn commit_profile_generation_root(
    roots: &Roots,
    prepared: &PreparedProfileGenerationRoot,
    at: u64,
) -> std::io::Result<()> {
    Lifecycle::commit(
        roots,
        &prepared.id,
        prepared.incarnation,
        &prepared.witness,
        Lifecycle::LifecycleTimestamp::from_unix_seconds(at),
    )?;
    Ok(())
}

/// Check that a package-generation record still owns the exact committed GC
/// root that its completion witness names. Listing and activation use this
/// gate so a missing or rebound lifecycle root cannot look like a live profile.
pub(crate) fn validate_profile_generation_root(
    roots: &Roots,
    owner: &str,
    profile: &str,
    generation: u64,
    witness: &str,
    targets: &BTreeSet<String>,
) -> std::io::Result<()> {
    if targets.is_empty() {
        return Ok(());
    }
    let id = Lifecycle::RootId::new(format!("profile-generation:{owner}:{profile}:{generation}"))?;
    let snapshot = Lifecycle::snapshot(roots)?;
    let root = snapshot.roots.get(&id).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "profile generation lifecycle root is missing",
        )
    })?;
    if root.phase != Lifecycle::RootPhase::Committed {
        return Err(std::io::Error::other(
            "profile generation lifecycle root is not committed",
        ));
    }
    if root.identity.kind != Lifecycle::RootKind::ProfileGeneration
        || root.identity.producer.as_str() != "jetpack-profile-generation"
        || root.identity.incarnation.get() != 1
        || root.identity.witness.as_str() != witness
        || root.targets != *targets
        || root.protected_targets != *targets
    {
        return Err(std::io::Error::other(
            "profile generation lifecycle root disagrees with its record",
        ));
    }
    Ok(())
}

/// Opaque receipt for a durable generation owned by a consumer outside the
/// package-generation engines. The consumer controls only its own stable key;
/// lifecycle kind, producer, incarnation, and witness matching stay here.
pub(crate) struct PreparedExternalConsumerRoot {
    id: Lifecycle::RootId,
    incarnation: Lifecycle::Incarnation,
    witness: Lifecycle::RootWitness,
}

/// Prepare or resume one immutable external consumer root. An exact committed
/// retry is already complete and returns `None`; the stable key can never be
/// rebound to another witness or target set.
pub(crate) fn reconcile_external_consumer_root(
    roots: &Roots,
    consumer: &str,
    key: &str,
    witness: &str,
    mut targets: Vec<String>,
    at: u64,
) -> std::io::Result<Option<PreparedExternalConsumerRoot>> {
    targets.sort();
    targets.dedup();
    if targets.is_empty() {
        return Err(std::io::Error::other(
            "external consumer root requires at least one Hangar object",
        ));
    }
    let objects = roots.hangar_dir().join(OBJECTS_DIR);
    let objects_metadata = fs::symlink_metadata(&objects)?;
    if objects_metadata.file_type().is_symlink() || !objects_metadata.is_dir() {
        return Err(std::io::Error::other(
            "external consumer target pool is not a real Hangar directory",
        ));
    }
    for target in &targets {
        let metadata = fs::symlink_metadata(objects.join(target));
        if target.len() != 71
            || !target.starts_with("sha256-")
            || !target[7..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || !matches!(metadata, Ok(meta) if meta.is_dir() && !meta.file_type().is_symlink())
        {
            return Err(std::io::Error::other(
                "external consumer target is not a canonical Hangar object",
            ));
        }
    }
    let id = Lifecycle::RootId::new(format!(
        "external-consumer:{consumer}:{}",
        SHA256::sha256_hex(key.as_bytes())
    ))?;
    let witness = Lifecycle::RootWitness::new(witness)?;
    let incarnation = Lifecycle::Incarnation::new(1)?;
    let identity = Lifecycle::RootIdentity::new(
        Lifecycle::RootKind::ExternalConsumer,
        id.clone(),
        Lifecycle::ProducerId::new(consumer)?,
        incarnation,
        witness.clone(),
    );
    let snapshot = Lifecycle::prepare_if_absent(
        roots,
        identity,
        targets,
        Lifecycle::LifecycleTimestamp::from_unix_seconds(at),
    )?;
    let root = snapshot
        .roots
        .get(&id)
        .ok_or_else(|| std::io::Error::other("external consumer root disappeared after prepare"))?;
    if root.phase == Lifecycle::RootPhase::Committed {
        return Ok(None);
    }
    Ok(Some(PreparedExternalConsumerRoot {
        id,
        incarnation,
        witness,
    }))
}

pub(crate) fn commit_external_consumer_root(
    roots: &Roots,
    prepared: &PreparedExternalConsumerRoot,
    at: u64,
) -> std::io::Result<()> {
    Lifecycle::commit(
        roots,
        &prepared.id,
        prepared.incarnation,
        &prepared.witness,
        Lifecycle::LifecycleTimestamp::from_unix_seconds(at),
    )?;
    Ok(())
}

fn live_roots_unlocked(roots: &Roots) -> std::io::Result<LiveRoots> {
    let cwd = std::env::current_dir()?;
    let graph = Closure::lifecycle_closure_graph_unlocked(roots)?;
    live_roots_from_graph(roots, &cwd, &graph)
}

#[cfg(test)]
fn live_roots_from(roots: &Roots, start: &Path) -> std::io::Result<LiveRoots> {
    let graph = Closure::lifecycle_closure_graph_unlocked(roots)?;
    live_roots_from_graph(roots, start, &graph)
}

fn live_roots_from_graph(
    roots: &Roots,
    start: &Path,
    graph: &Closure::ClosureGraph,
) -> std::io::Result<LiveRoots> {
    let mut live = lock_roots_from(start)?;
    let lifecycle = Lifecycle::protected_targets_unlocked(roots)?;
    let mut targets = live.output_hashes.clone();
    for (receipt, package) in &live.receipt_packages {
        let mut matching = Vec::new();
        for record in graph.records.values().filter(|record| {
            parse_meta(&record.package_meta).is_some_and(|meta| meta.receipt == *receipt)
        }) {
            let meta = parse_meta(&record.package_meta).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Hangar closure record `{}` has invalid package metadata for receipt `{receipt}`",
                        record.id
                    ),
                )
            })?;
            if meta.name == package.name
                && meta.version == package.version
                && receipt_source_matches(package, &meta.reference)
                && receipt_envelope_matches(package, &meta.envelope.output_hash)
            {
                matching.push((record, meta));
            }
        }
        if matching.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "project lock receipt `{receipt}` disagrees with Hangar closure record: no matching package identity"
                ),
            ));
        }
        // A receipt is the lock's projection of the action root. Reachability
        // follows the same graph closure as output and lifecycle roots.
        for (record, _) in matching {
            targets.insert(record.primary.clone());
        }
    }
    for id in &live.ids {
        if let Some(record) = graph.records.get(id) {
            targets.insert(record.primary.clone());
        }
    }
    for record in graph.records.values() {
        let Some(meta) = parse_meta(&record.package_meta) else {
            continue;
        };
        if live
            .name_versions
            .contains(&(meta.name.clone(), meta.version.clone()))
        {
            targets.insert(record.primary.clone());
        }
    }
    targets.extend(lifecycle);
    for target in targets {
        live.output_hashes.extend(graph.closure(&target));
    }
    Ok(live)
}

fn current_project_root() -> std::io::Result<Option<PathBuf>> {
    let cwd = std::env::current_dir()?;
    let mut dir = Some(cwd.clone());
    while let Some(current) = dir {
        let managed = managed_dir(&current);
        match fs::symlink_metadata(&managed) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "project managed directory `{}` is not a real directory",
                        managed.display()
                    ),
                ));
            }
            Ok(_) => return Ok(Some(current)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                dir = current.parent().map(Path::to_path_buf);
            }
            Err(error) => return Err(error),
        }
    }
    // A project can be preparing its first lock. Serialize that preparation
    // against cleanup at the current root even before `.jet/lock` exists.
    // Filesystem root is the neutral cwd used by no-project commands; never
    // try to create `/.jet` there.
    let is_below_filesystem_root = cwd
        .parent()
        .is_some_and(|parent| !parent.as_os_str().is_empty() && parent != cwd.as_path());
    Ok(is_below_filesystem_root.then_some(cwd))
}

#[derive(Debug, Default)]
pub(crate) struct LiveRoots {
    ids: BTreeSet<String>,
    output_hashes: BTreeSet<String>,
    name_versions: BTreeSet<(String, String)>,
    receipts: BTreeSet<String>,
    receipt_packages: Vec<(String, crate::Lock::LockedPackage)>,
}

fn lock_roots_from(start: &Path) -> std::io::Result<LiveRoots> {
    let Some(lock_path) = nearest_lock_path(start)? else {
        return Ok(LiveRoots::default());
    };
    let raw = fs::read_to_string(&lock_path).map_err(|error| {
        std::io::Error::new(
            error.kind(),
            format!(
                "could not read project lock `{}`: {error}",
                lock_path.display()
            ),
        )
    })?;
    let lock = crate::Lock::parse(&raw).map_err(|error| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "could not parse project lock `{}`: {error}",
                lock_path.display()
            ),
        )
    })?;
    let mut roots = LiveRoots::default();
    for pkg in lock.packages {
        roots
            .name_versions
            .insert((pkg.name.clone(), pkg.version.clone()));
        if let Some(receipt) = pkg.receipt.clone() {
            if !valid_receipt_digest(&receipt) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "project lock `{}` contains an invalid Hangar receipt",
                        lock_path.display()
                    ),
                ));
            }
            roots.receipts.insert(receipt.clone());
            roots.receipt_packages.push((receipt, pkg.clone()));
        }
        if let Some(env) = pkg.envelope {
            if !env.output_hash.is_empty() {
                roots.output_hashes.insert(env.output_hash);
            }
        }
    }
    for toolchain in lock.toolchains {
        roots.ids.insert(toolchain.id);
        if !toolchain.envelope.output_hash.is_empty() {
            roots.output_hashes.insert(toolchain.envelope.output_hash);
        }
    }
    Ok(roots)
}

fn nearest_lock_path(start: &Path) -> std::io::Result<Option<PathBuf>> {
    let mut dir = Some(start);
    while let Some(current) = dir {
        let managed = managed_dir(current);
        match fs::symlink_metadata(&managed) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "project managed directory `{}` is a symlink; repair it before Hangar cleanup",
                        managed.display()
                    ),
                ));
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "project managed directory `{}` is not a directory; repair it before Hangar cleanup",
                        managed.display()
                    ),
                ));
            }
            Ok(_) => {
                let lock = lock_path(current);
                match fs::symlink_metadata(&lock) {
                    Ok(metadata) if metadata.file_type().is_symlink() => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!(
                                "project lock `{}` is a symlink; repair it before Hangar cleanup",
                                lock.display()
                            ),
                        ));
                    }
                    Ok(metadata) if metadata.is_file() => return Ok(Some(lock)),
                    Ok(_) => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!(
                                "project lock `{}` is not a regular file; repair it before Hangar cleanup",
                                lock.display()
                            ),
                        ));
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        dir = current.parent();
    }
    Ok(None)
}

fn is_live(id: &str, meta: &ParsedMeta, roots: &LiveRoots) -> bool {
    roots.receipts.contains(&meta.receipt)
        || roots.ids.contains(id)
        || (!meta.envelope.output_hash.is_empty()
            && roots.output_hashes.contains(&meta.envelope.output_hash))
        || (meta.envelope.output_hash.is_empty()
            && roots
                .name_versions
                .contains(&(meta.name.clone(), meta.version.clone())))
}

// ── E4-JP1 Hangar Store v2: atomic staged ingest ─────────────────────────

const STAGE_DIR: &str = ".stage";
const OBJECTS_DIR: &str = "objects";
const CAS_DIR: &str = "cas";
const REFERRERS_DIR: &str = "referrers";
const PARTIAL_SUFFIX: &str = ".partial";

mod Reuse;
pub use Reuse::*;

mod Cleanup;
pub use Cleanup::*;

mod Ingest;
pub use Ingest::*;
mod Closure;
pub use Closure::*;
mod Quota;
pub(crate) use Quota::{admission_reservation, admission_size, ensure_hangar_capacity};
mod Explain;
pub(crate) use Cache::{fsync_tree, make_tree_writable_for_removal, seal_node};
pub(crate) use Closure::dir_size;
pub use Explain::*;
pub(crate) mod Lifecycle;
pub(crate) use Lifecycle::{
    external_root_closure_size, list_external_roots, reconcile_profile_generation_root,
    register_external_root_at, unregister_external_root_at, ExternalRootError,
};
#[cfg(test)]
mod Tests;
