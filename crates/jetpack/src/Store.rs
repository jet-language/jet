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
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
#[cfg(target_os = "linux")]
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
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
mod Broker;
pub use Broker::*;
mod Reproducibility;
pub(crate) use Reproducibility::{
    certify_registration_unlocked, certify_registration_unlocked_with_fresh_agreement,
    reproducibility_blocked,
};

fn list_unlocked(roots: &Roots) -> std::io::Result<Vec<StoreEntry>> {
    jet_pkg_model::Store::list_checked(roots)
}

/// Inspect package projections without taking a lock or replaying journals.
/// Health/reporting paths use this so observation cannot create or repair
/// store state.
pub(crate) fn list_read_only(roots: &Roots) -> Vec<StoreEntry> {
    jet_pkg_model::Store::list(roots)
}

pub fn list_checked(roots: &Roots) -> std::io::Result<Vec<StoreEntry>> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        Closure::recover_closure_journal_unlocked(roots)?;
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
const AUTO_CLEAN_STAMP: &str = ".last-auto-clean";
const STALE_AFTER: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const AUTO_CLEAN_AFTER: Duration = Duration::from_secs(24 * 60 * 60);
static OPTIMIZE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

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
        let leases = recover_stale_leases_unlocked(roots)?;
        let closure = Closure::recover_closure_journal_unlocked(roots)?;
        Ok(staging + reproducibility + archive + repairs + build_debug + leases + closure)
    })
}

const LEASE_NAME_MAX: usize = 256;

fn recover_stale_leases_unlocked(roots: &Roots) -> std::io::Result<usize> {
    let leases = roots.root.join("leases");
    let metadata = match fs::symlink_metadata(&leases) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Hangar lease directory is not a real directory; repair the path before recovery",
        ));
    }
    let current = std::process::id();
    let mut swept = 0;
    for entry in fs::read_dir(&leases)? {
        let entry = entry?;
        let name = entry.file_name();
        let text = name.to_string_lossy();
        if text.len() > LEASE_NAME_MAX {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "Hangar lease name exceeds the recovery bound",
            ));
        }
        let Some(pid) = text
            .split('-')
            .next()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Hangar lease `{text}` has an invalid owner id"),
            ));
        };
        if pid == current || lease_process_is_alive(pid) {
            continue;
        }
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || metadata.is_file() {
            fs::remove_file(&path)?;
        } else if metadata.is_dir() {
            make_tree_writable_for_removal(&path)?;
            fs::remove_dir_all(&path)?;
        } else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Hangar lease `{}` is not removable", path.display()),
            ));
        }
        swept += 1;
    }
    Ok(swept)
}

#[cfg(unix)]
fn lease_process_is_alive(pid: u32) -> bool {
    PathBuf::from(format!("/proc/{pid}")).is_dir()
}

#[cfg(not(unix))]
fn lease_process_is_alive(pid: u32) -> bool {
    pid == std::process::id()
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
                    return Path::new(destination).join(relative).to_string_lossy().into_owned();
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

/// Return the newest recorded object for an exact ref, if one is already in
/// the hangar. U29 uses this before provider dispatch so a lock-satisfied
/// offline run never asks Nix/git for metadata.
pub fn find_by_reference(roots: &Roots, reference: &str) -> Option<StoreEntry> {
    list(roots)
        .into_iter()
        .filter(|e| e.reference == reference)
        .max_by_key(|e| e.last_used_at)
}

/// Proof attached to a cache reuse decision. Every field must pass; callers
/// must treat a partial proof as a miss. An unsigned artifact is accepted only
/// when the provider independently derives its exact Hangar-owned output. A
/// signed artifact must verify against the configured cache public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheVerification {
    pub output_exists: bool,
    pub output_digest: bool,
    pub source: bool,
    pub recipe: bool,
    pub platform: bool,
    pub policy: bool,
    pub signature_verified: bool,
    pub unsigned_local_allowed: bool,
    pub closure: bool,
}

impl CacheVerification {
    pub fn trusted(self) -> bool {
        self.output_exists
            && self.output_digest
            && self.source
            && self.recipe
            && self.platform
            && self.policy
            && (self.signature_verified || self.unsigned_local_allowed)
            && self.closure
    }
}

pub fn verify_cache_entry(
    roots: &Roots,
    entry: &StoreEntry,
    expected_reference: &str,
    expectation: &CacheExpectation,
) -> CacheVerification {
    let graph = Closure::closure_graph_structure(roots).ok();
    verify_cache_entry_with_graph(
        roots,
        entry,
        expected_reference,
        expectation,
        graph.as_ref(),
    )
}

fn verify_cache_entry_with_graph(
    roots: &Roots,
    entry: &StoreEntry,
    expected_reference: &str,
    expectation: &CacheExpectation,
    graph: Option<&Closure::ClosureGraph>,
) -> CacheVerification {
    let out = Path::new(&entry.out);
    let output_exists = fs::symlink_metadata(out)
        .map(|metadata| {
            !metadata.file_type().is_symlink() && (metadata.is_file() || metadata.is_dir())
        })
        .unwrap_or(false);
    let output_digest = output_exists
        && !entry.envelope.output_hash.is_empty()
        && Ingest::try_entry_output_hash(roots, entry)
            .is_ok_and(|hash| hash == entry.envelope.output_hash);
    let source = !expectation.identity.source_fingerprint.is_empty()
        && entry.cache_identity.source_fingerprint == expectation.identity.source_fingerprint;
    let recipe = !expectation.identity.recipe_fingerprint.is_empty()
        && entry.cache_identity.recipe_fingerprint == expectation.identity.recipe_fingerprint;
    let platform = entry.envelope.platform == expectation.identity.platform
        && entry.cache_identity.platform == expectation.identity.platform;
    let policy = entry.reference == expected_reference
        && !entry.envelope.provenance.is_empty()
        && !expectation.identity.policy_fingerprint.is_empty()
        && entry.cache_identity.policy_fingerprint == expectation.identity.policy_fingerprint
        && producer_authority_verified(roots, entry, expectation);
    let signature_verified = !entry.envelope.signature.is_empty()
        && verify_configured_signature(roots, entry, expectation);
    let canonical_local = Path::new(&entry.out)
        == roots
            .hangar_dir()
            .join(OBJECTS_DIR)
            .join(&entry.envelope.output_hash);
    let unsigned_local_allowed = entry.envelope.signature.is_empty()
        && expectation.allow_unsigned_local
        && (canonical_local
            || expectation
                .owned_output
                .as_ref()
                .is_some_and(|path| path == Path::new(&entry.out)));
    let closure = output_exists
        && closure_is_reachable(roots, entry)
        && graph.is_some_and(|graph| Closure::entry_closure_store_proof(roots, graph, entry));
    CacheVerification {
        output_exists,
        output_digest,
        source,
        recipe,
        platform,
        policy,
        signature_verified,
        unsigned_local_allowed,
        closure,
    }
}

fn producer_authority_verified(
    roots: &Roots,
    entry: &StoreEntry,
    expectation: &CacheExpectation,
) -> bool {
    if reproducibility_blocked(roots, &entry_action_key(entry)).unwrap_or(true) {
        return false;
    }
    let Ok(producer) = ProducerRecord::decode(&entry.producer_record) else {
        return false;
    };
    if producer.provider.trim().is_empty()
        || producer.immutable_source.trim().is_empty()
        || producer.source_digest.trim().is_empty()
        || producer.facts.get("action.recipe").map(String::as_str)
            != Some(expectation.identity.recipe_fingerprint.as_str())
        || producer.policy_facts
            != format!(
                "policy={}\nplatform={}",
                expectation.identity.policy_fingerprint, expectation.identity.platform
            )
    {
        return false;
    }
    let Some(attestation) = producer.facts.get("cache.reproducibility") else {
        return false;
    };
    if !(attestation == "attested-v1" || attestation.starts_with("independent-agreeing-v1:")) {
        return false;
    }
    let provenance = crate::TrustRoot::CacheProvenance {
        reference: producer
            .facts
            .get("cache.reference")
            .cloned()
            .unwrap_or_default(),
        source: producer
            .facts
            .get("cache.source")
            .cloned()
            .unwrap_or_default(),
        builder: producer
            .facts
            .get("cache.builder")
            .cloned()
            .unwrap_or_default(),
        action: producer
            .facts
            .get("cache.action")
            .cloned()
            .unwrap_or_default(),
        output: producer
            .facts
            .get("cache.output")
            .cloned()
            .unwrap_or_default(),
        platform: producer
            .facts
            .get("cache.platform")
            .cloned()
            .unwrap_or_default(),
        sandbox: producer
            .facts
            .get("cache.sandbox")
            .cloned()
            .unwrap_or_default(),
        policy: producer
            .facts
            .get("cache.policy")
            .cloned()
            .unwrap_or_default(),
    };
    let builder = cache_builder_identity(
        &producer.provider,
        &producer.immutable_source,
        &producer.source_digest,
    );
    provenance.validate().is_ok()
        && provenance.reference == entry.reference
        && provenance.source == producer.immutable_source
        && provenance.builder == builder
        && provenance.action
            == cache_action_identity(
                &producer,
                &entry.reference,
                &entry.cache_identity,
                &entry.references,
            )
        && provenance.output == entry.envelope.output_hash
        && provenance.platform == expectation.identity.platform
        && provenance.sandbox == "sandbox:policy-bound"
        && provenance.policy == expectation.identity.policy_fingerprint
        && !is_cache_builder_revoked(&roots.root, &builder).unwrap_or(true)
}

fn enforce_manifest_provenance_floor(
    project: Option<&Path>,
    package: &str,
) -> Result<(), RealizeError> {
    let Some(project) = project else {
        return Ok(());
    };
    let Some(manifest) = crate::Package::PackageFacts::load(project) else {
        return Ok(());
    };
    let manifest = match manifest {
        Ok(manifest) => manifest,
        Err(error) => {
            return Err(RealizeError::Store(std::io::Error::other(format!(
                "package trust manifest could not be loaded: {error:?}"
            ))));
        }
    };
    let Some(requirement) = manifest
        .authority
        .trust
        .as_ref()
        .and_then(|trust| trust.require)
    else {
        return Ok(());
    };
    let Some(lock) = crate::Lock::load(project) else {
        return Err(RealizeError::Integrity(IntegrityFailure {
            package: package.to_string(),
            version: String::new(),
            expected: format!("{} provenance", requirement.label()),
            actual: format!("{} is missing", lock_path(project).display()),
            reason: format!("the package trust policy requires {} provenance", requirement.label()),
            disposition: "Jetpack rejected the package before provider, cache, or remote bytes became usable."
                .to_string(),
            fix: "Refresh `.jet/lock` with the required provenance, then retry.".to_string(),
        }));
    };
    if let Err(detail) = crate::Lock::enforce_provenance_requirement(&lock, requirement) {
        return Err(RealizeError::Integrity(IntegrityFailure {
            package: package.to_string(),
            version: String::new(),
            expected: format!("{} provenance", requirement.label()),
            actual: detail,
            reason: format!("the package trust policy requires {} provenance", requirement.label()),
            disposition: "Jetpack rejected the package before provider, cache, or remote bytes became usable."
                .to_string(),
            fix: "Refresh `.jet/lock` with the required provenance, then retry.".to_string(),
        }));
    }
    Ok(())
}

pub struct VerifiedCacheHit {
    pub entry: StoreEntry,
    pub lease: CacheLease,
}

pub struct CacheLease {
    files: Vec<(PathBuf, fs::File)>,
    executables: Vec<(std::ffi::OsString, fs::File)>,
    out: PathBuf,
    snapshot_root: PathBuf,
    bin_relative: Option<PathBuf>,
    expected_digest: String,
    package: String,
    version: String,
    reference: String,
    store_root: PathBuf,
    status: ConsumptionStatus,
    wrapper_root: Option<PathBuf>,
    /// Logical `/nix/store/<name>` paths mapped to the verified private
    /// snapshot. Shell consumers use this only inside a rootless namespace.
    nix_store_projection: Vec<(String, PathBuf)>,
    _wrapper_dir_handle: Option<fs::File>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsumptionStatus {
    Consumable,
    NonConsumable { reason: String },
}

impl CacheLease {
    pub fn status(&self) -> &ConsumptionStatus {
        &self.status
    }

    pub fn wrapper_dir(&self) -> Option<&Path> {
        self.wrapper_root.as_deref()
    }

    pub(crate) fn nix_store_projection(&self) -> &[(String, PathBuf)] {
        &self.nix_store_projection
    }

    pub fn original_output(&self) -> &Path {
        &self.out
    }

    pub fn original_reference(&self) -> &str {
        &self.reference
    }

    pub(crate) fn profile_install_receipt(&self) -> std::io::Result<ProfileInstallReceipt> {
        self.validate()?;
        let mut executable_members = self
            .executables
            .iter()
            .map(|(name, _)| {
                name.to_str()
                    .map(str::to_string)
                    .ok_or_else(|| std::io::Error::other("tool executable name is not UTF-8"))
            })
            .collect::<std::io::Result<Vec<_>>>()?;
        executable_members.sort();
        if executable_members.is_empty() {
            return Err(std::io::Error::other(
                "verified tool lease has no executables",
            ));
        }
        Ok(ProfileInstallReceipt {
            store_root: self.store_root.clone(),
            package: self.package.clone(),
            version: self.version.clone(),
            reference: self.reference.clone(),
            output_hash: self.expected_digest.clone(),
            executable_members,
        })
    }

    pub(crate) fn copy_profile_executable(
        &self,
        member: &str,
        destination: &Path,
    ) -> std::io::Result<ProfileExecutableProof> {
        self.validate()?;
        let (_, source) = self
            .executables
            .iter()
            .find(|(name, _)| name == member)
            .ok_or_else(|| std::io::Error::other("verified receipt lost executable member"))?;
        copy_open_profile_file(source, destination)
    }

    fn require_consumable(&self) -> std::io::Result<()> {
        match &self.status {
            ConsumptionStatus::Consumable => Ok(()),
            ConsumptionStatus::NonConsumable { reason } => {
                Err(std::io::Error::other(reason.clone()))
            }
        }
    }

    pub fn executable(&self, name: &str) -> Option<PathBuf> {
        self.require_consumable().ok()?;
        if name.contains(std::path::MAIN_SEPARATOR) {
            return None;
        }
        let (_, file) = self.executables.iter().find(|(member, _)| member == name)?;
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd as _;
            Some(PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd())))
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = file;
            let bin = self.bin_relative.as_ref()?;
            Some(self.snapshot_root.join(bin).join(name))
        }
    }

    pub(crate) fn projected_executable(&self, name: &str) -> Option<PathBuf> {
        self.require_consumable().ok()?;
        if name.contains(std::path::MAIN_SEPARATOR) {
            return None;
        }
        self.executables.iter().any(|(member, _)| member == name).then(|| {
            self.bin_relative
                .as_ref()
                .map(|bin| self.snapshot_root.join(bin).join(name))
        })?
    }

    pub fn stable_path(&self, path: &str) -> std::io::Result<PathBuf> {
        self.require_consumable()?;
        let relative = Path::new(path).strip_prefix(&self.out).map_err(|_| {
            std::io::Error::other(format!(
                "consumer path `{path}` escapes leased output `{}`",
                self.out.display()
            ))
        })?;
        if relative
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(std::io::Error::other(
                "leased consumer path contains parent traversal",
            ));
        }
        if let Some((_, file)) = self.files.iter().find(|(member, _)| member == relative) {
            #[cfg(target_os = "linux")]
            {
                use std::os::fd::AsRawFd as _;
                return Ok(PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd())));
            }
            #[cfg(not(target_os = "linux"))]
            let _ = file;
        }
        if relative.parent() == self.bin_relative.as_deref() {
            if let Some((_, file)) = self
                .executables
                .iter()
                .find(|(name, _)| Some(name.as_os_str()) == relative.file_name())
            {
                #[cfg(target_os = "linux")]
                {
                    use std::os::fd::AsRawFd as _;
                    return Ok(PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd())));
                }
                #[cfg(not(target_os = "linux"))]
                let _ = file;
            }
        }
        Ok(self.snapshot_root.join(relative))
    }

    /// Revalidate immediately before handing paths to a child consumer. The
    /// archive reader uses no-follow handles and rejects concurrent mutation.
    pub fn validate(&self) -> std::io::Result<()> {
        self.require_consumable()?;
        let actual = super::Envelope::try_output_hash_of(&self.snapshot_root.to_string_lossy())
            .map_err(std::io::Error::other)?;
        if actual != self.expected_digest {
            return Err(std::io::Error::other(format!(
                "leased output changed: expected {}, got {actual}",
                self.expected_digest
            )));
        }
        #[cfg(target_os = "linux")]
        if let Some(wrapper) = &self.wrapper_root {
            let mut actual = fs::read_dir(wrapper)?
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .map(|entry| {
                    let target = fs::read_link(entry.path())?;
                    Ok((entry.file_name(), target))
                })
                .collect::<std::io::Result<Vec<_>>>()?;
            actual.sort_by(|left, right| left.0.cmp(&right.0));
            let mut expected = self
                .executables
                .iter()
                .map(|(name, file)| {
                    use std::os::fd::AsRawFd as _;
                    (
                        name.clone(),
                        PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd())),
                    )
                })
                .collect::<Vec<_>>();
            expected.sort_by(|left, right| left.0.cmp(&right.0));
            if actual != expected {
                return Err(std::io::Error::other("lease executable wrappers changed"));
            }
        }
        Ok(())
    }

    pub fn integrity_failure(&self) -> Option<IntegrityFailure> {
        self.validate()
            .err()
            .map(|error| self.consumption_failure(&error))
    }

    pub(crate) fn consumption_failure(&self, error: &std::io::Error) -> IntegrityFailure {
        IntegrityFailure {
            package: self.package.clone(),
            version: self.version.clone(),
            expected: self.expected_digest.clone(),
            actual: error.to_string(),
            reason: "race-safe pre-consumer revalidation".to_string(),
            disposition: "Jetpack rejected it before handing any path to the consumer."
                .to_string(),
            fix: "Re-run `jet store fetch` after `jet clean`. If the problem persists, audit the source before rebuilding."
                .to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfileInstallReceipt {
    pub(crate) store_root: PathBuf,
    pub(crate) package: String,
    pub(crate) version: String,
    pub(crate) reference: String,
    pub(crate) output_hash: String,
    pub(crate) executable_members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfileExecutableProof {
    pub(crate) digest: String,
    pub(crate) mode: u32,
}

fn copy_open_profile_file(
    source: &fs::File,
    destination: &Path,
) -> std::io::Result<ProfileExecutableProof> {
    use std::io::{Read as _, Seek as _, Write as _};

    let mut input = source.try_clone()?;
    input.seek(std::io::SeekFrom::Start(0))?;
    let metadata = input.metadata()?;
    if !metadata.is_file() {
        return Err(std::io::Error::other(
            "profile executable is not a regular file",
        ));
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut hasher = SHA256::StreamingSha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
        output.write_all(&buffer[..count])?;
    }
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = metadata.permissions().mode() & 0o777;
        fs::set_permissions(destination, fs::Permissions::from_mode(mode))?;
        mode
    };
    #[cfg(not(unix))]
    let mode = 0;
    output.sync_all()?;
    let digest = hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if SHA256::sha256_file_hex(destination)? != digest {
        return Err(std::io::Error::other("copied profile executable changed"));
    }
    Ok(ProfileExecutableProof {
        digest: format!("sha256-{digest}"),
        mode,
    })
}

pub(crate) fn copy_profile_store_member(
    roots: &Roots,
    reference: &str,
    output_hash: &str,
    member: &str,
    destination: &Path,
) -> std::io::Result<ProfileExecutableProof> {
    let entry = list_checked(roots)?
        .into_iter()
        .find(|entry| entry.reference == reference && entry.envelope.output_hash == output_hash)
        .ok_or_else(|| std::io::Error::other("profile StoreEntry authority is unavailable"))?;
    let source = Path::new(&entry.bin).join(member);
    let path_metadata = fs::symlink_metadata(&source)?;
    if !path_metadata.is_file() || path_metadata.file_type().is_symlink() {
        return Err(std::io::Error::other(
            "profile executable member is not a no-follow file",
        ));
    }
    let source_file = fs::File::open(&source)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let opened = source_file.metadata()?;
        if path_metadata.dev() != opened.dev() || path_metadata.ino() != opened.ino() {
            return Err(std::io::Error::other(
                "profile executable changed while opening",
            ));
        }
    }
    copy_open_profile_file(&source_file, destination)
}

pub(crate) fn profile_file_proof(path: &Path) -> std::io::Result<ProfileExecutableProof> {
    let path_metadata = fs::symlink_metadata(path)?;
    if !path_metadata.is_file() || path_metadata.file_type().is_symlink() {
        return Err(std::io::Error::other(
            "profile projection is not a no-follow file",
        ));
    }
    let opened = fs::File::open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
        let metadata = opened.metadata()?;
        if path_metadata.dev() != metadata.dev() || path_metadata.ino() != metadata.ino() {
            return Err(std::io::Error::other(
                "profile projection changed while opening",
            ));
        }
        return Ok(ProfileExecutableProof {
            digest: format!("sha256-{}", SHA256::sha256_file_hex(path)?),
            mode: metadata.permissions().mode() & 0o777,
        });
    }
    #[cfg(not(unix))]
    Ok(ProfileExecutableProof {
        digest: format!("sha256-{}", SHA256::sha256_file_hex(path)?),
        mode: 0,
    })
}

impl Drop for CacheLease {
    fn drop(&mut self) {
        let _ = remove_snapshot_node(&self.snapshot_root);
    }
}

pub fn find_verified_by_reference(
    roots: &Roots,
    reference: &str,
    expectation: &CacheExpectation,
) -> std::io::Result<Option<VerifiedCacheHit>> {
    let _global = super::RuntimePolicy::acquire_lock(&roots.root, "hangar")?;
    let graph = Closure::closure_graph_structure_unlocked(roots)?;
    let entry = list_unlocked(roots)?
        .into_iter()
        .filter(|entry| entry.reference == reference)
        .filter(|entry| {
            verify_cache_entry_with_graph(roots, entry, reference, expectation, Some(&graph))
                .trusted()
        })
        .max_by_key(|entry| entry.last_used_at);
    let Some(entry) = entry else {
        return Ok(None);
    };
    let lease = snapshot_lease(roots, &entry)?;
    Ok(Some(VerifiedCacheHit { entry, lease }))
}

fn snapshot_lease(roots: &Roots, entry: &StoreEntry) -> std::io::Result<CacheLease> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    use std::sync::atomic::Ordering;

    let leases = roots.root.join("leases");
    Ingest::ensure_real_directory(&leases, "Hangar lease directory")?;
    let mut components = Path::new(&entry.id).components();
    if !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        return Err(std::io::Error::other(
            "cache lease entry identity is not one path component",
        ));
    }
    let snapshot_root = leases.join(format!(
        "{}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed),
        entry.id
    ));
    if let Ok(producer) = ProducerRecord::decode(&entry.producer_record) {
        super::Provider::validate_nix_build_facts(&producer)?;
    }
    match fs::symlink_metadata(&snapshot_root) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
                return Err(std::io::Error::other(
                    "cache lease snapshot is not a regular node",
                ));
            }
            remove_snapshot_node(&snapshot_root)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    if !Path::new(&entry.out).exists() {
        Ingest::ensure_real_directory(&snapshot_root, "cache lease snapshot")?;
        return Ok(CacheLease {
            files: Vec::new(),
            executables: Vec::new(),
            out: PathBuf::from(&entry.out),
            snapshot_root,
            bin_relative: None,
            expected_digest: String::new(),
            package: entry.name.clone(),
            version: entry.version.clone(),
            reference: entry.reference.clone(),
            store_root: roots.root.clone(),
            status: ConsumptionStatus::NonConsumable {
                reason: "realization has no canonical consumable output".to_string(),
            },
            wrapper_root: None,
            nix_store_projection: Vec::new(),
            _wrapper_dir_handle: None,
        });
    }
    let mut hardlinks = BTreeMap::new();
    copy_snapshot_node(Path::new(&entry.out), &snapshot_root, &mut hardlinks)?;
    let digest = super::Envelope::try_output_hash_of(&snapshot_root.to_string_lossy())
        .map_err(std::io::Error::other)?;
    if digest != entry.envelope.output_hash {
        remove_snapshot_node(&snapshot_root)?;
        return Err(std::io::Error::other(format!(
            "private lease snapshot mismatch: expected {}, got {digest}",
            entry.envelope.output_hash
        )));
    }
    seal_local_output(&snapshot_root)?;
    let sealed_digest = super::Envelope::try_output_hash_of(&snapshot_root.to_string_lossy())
        .map_err(std::io::Error::other)?;
    let mut files = Vec::new();
    open_snapshot_files(&snapshot_root, &snapshot_root, &mut files)?;
    let bin_relative = (!entry.bin.is_empty())
        .then(|| {
            Path::new(&entry.bin)
                .strip_prefix(&entry.out)
                .ok()
                .map(PathBuf::from)
        })
        .flatten();
    let executables = open_snapshot_executables(&snapshot_root, bin_relative.as_deref())?;
    let wrappers = create_exec_wrappers(&snapshot_root, &executables)?;
    let nix_store_projection = nix_store_projection_for_entry(roots, entry, &snapshot_root)?;
    Ok(CacheLease {
        files,
        executables,
        out: PathBuf::from(&entry.out),
        snapshot_root,
        bin_relative,
        expected_digest: sealed_digest,
        package: entry.name.clone(),
        version: entry.version.clone(),
        reference: entry.reference.clone(),
        store_root: roots.root.clone(),
        status: ConsumptionStatus::Consumable,
        wrapper_root: wrappers.as_ref().map(|wrapper| wrapper.root.clone()),
        nix_store_projection,
        _wrapper_dir_handle: wrappers.map(|wrapper| wrapper.directory),
    })
}

fn nix_store_projection_for_entry(
    roots: &Roots,
    entry: &StoreEntry,
    snapshot_root: &Path,
) -> std::io::Result<Vec<(String, PathBuf)>> {
    let Ok(producer) = ProducerRecord::decode(&entry.producer_record) else {
        return Ok(Vec::new());
    };
    if producer.provider != "nix" {
        return Ok(Vec::new());
    }
    let mut projection = Vec::new();
    for (key, path) in &producer.facts {
        let Some(name) = key.strip_prefix("nix.output.") else {
            continue;
        };
        let Some(store_name) = path.strip_prefix("/nix/store/") else {
            continue;
        };
        if store_name.is_empty()
            || store_name.contains('/')
            || store_name == "."
            || store_name == ".."
        {
            return Err(std::io::Error::other(format!(
                "invalid canonical Nix output path `{path}`"
            )));
        }
        let source = if name == "out" {
            snapshot_root.to_path_buf()
        } else {
            let digest = entry.named_outputs.get(name).ok_or_else(|| {
                std::io::Error::other(format!(
                    "Nix output `{name}` has no verified named-output digest"
                ))
            })?;
            let object = roots.hangar_dir().join(OBJECTS_DIR).join(digest);
            let metadata = fs::symlink_metadata(&object).map_err(|error| {
                std::io::Error::other(format!(
                    "Nix output `{name}` projected object is unavailable: {error}"
                ))
            })?;
            if metadata.file_type().is_symlink() || (!metadata.is_dir() && !metadata.is_file()) {
                return Err(std::io::Error::other(format!(
                    "Nix output `{name}` projected object is not a regular node"
                )));
            }
            object
        };
        if let Some((_, existing)) = projection.iter().find(|(logical, _)| logical == path) {
            if existing != &source {
                return Err(std::io::Error::other(format!(
                    "conflicting canonical Nix output path `{path}`"
                )));
            }
            continue;
        }
        projection.push((path.clone(), source));
    }
    Ok(projection)
}

struct ExecWrappers {
    root: PathBuf,
    directory: fs::File,
}

fn required_child_pipe<T>(pipe: Option<T>, message: &'static str) -> std::io::Result<T> {
    pipe.ok_or_else(|| std::io::Error::other(message))
}

fn open_snapshot_executables(
    snapshot_root: &Path,
    bin: Option<&Path>,
) -> std::io::Result<Vec<(std::ffi::OsString, fs::File)>> {
    let Some(bin) = bin else {
        return Ok(Vec::new());
    };
    let path = snapshot_root.join(bin);
    let mut out = Vec::new();
    if !path.is_dir() {
        return Ok(out);
    }
    let mut entries = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let file = fs::File::open(entry.path())?;
        if !file.metadata()?.is_file() {
            continue;
        }
        clear_close_on_exec(&file)?;
        out.push((entry.file_name(), file));
    }
    Ok(out)
}

#[cfg(target_os = "linux")]
fn create_exec_wrappers(
    snapshot_root: &Path,
    executables: &[(std::ffi::OsString, fs::File)],
) -> std::io::Result<Option<ExecWrappers>> {
    use std::io::{BufRead as _, Write as _};
    use std::os::fd::AsRawFd as _;

    if executables.is_empty() {
        return Ok(None);
    }
    let mountpoint = snapshot_root.with_extension("exec-mount");
    fs::create_dir_all(&mountpoint)?;
    let unshare = find_system_tool("unshare")?;
    let mount = find_system_tool("mount")?;
    let shell = find_system_tool("sh")?;
    let script = r#"set -eu
"$1" -t tmpfs -o size=1m,mode=0755 jetpack-exec "$2"
printf 'ready\n'
IFS= read -r _
"$1" -o remount,bind,ro "$2"
printf 'sealed\n'
IFS= read -r _ || true
"#;
    let mut child = Command::new(unshare)
        .args([
            "--user",
            "--map-root-user",
            "--mount",
            "--propagation",
            "private",
        ])
        .arg(shell)
        .args(["-c", script, "jetpack-lease-keeper"])
        .arg(mount)
        .arg(&mountpoint)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut input = required_child_pipe(child.stdin.take(), "piped lease keeper stdin")?;
    let output = required_child_pipe(child.stdout.take(), "piped lease keeper stdout")?;
    let mut output = std::io::BufReader::new(output);
    let mut line = String::new();
    output.read_line(&mut line)?;
    if line.trim() != "ready" {
        return Err(wrapper_keeper_error(
            child,
            "creating private executable mount",
        ));
    }
    let root = PathBuf::from(format!("/proc/{}/root{}", child.id(), mountpoint.display()));
    for (name, file) in executables {
        std::os::unix::fs::symlink(
            format!("/proc/self/fd/{}", file.as_raw_fd()),
            root.join(name),
        )?;
    }
    input.write_all(b"seal\n")?;
    input.flush()?;
    line.clear();
    output.read_line(&mut line)?;
    if line.trim() != "sealed" {
        return Err(wrapper_keeper_error(
            child,
            "sealing private executable mount",
        ));
    }
    let directory = fs::File::open(&root)?;
    clear_close_on_exec(&directory)?;
    let root = PathBuf::from(format!("/proc/self/fd/{}", directory.as_raw_fd()));
    drop(input);
    let status = child.wait()?;
    if !status.success() {
        return Err(std::io::Error::other(
            "private executable mount keeper failed",
        ));
    }
    fs::remove_dir(&mountpoint)?;
    Ok(Some(ExecWrappers { root, directory }))
}

#[cfg(target_os = "linux")]
fn find_system_tool(name: &str) -> std::io::Result<PathBuf> {
    ["/run/current-system/sw/bin", "/usr/bin", "/bin"]
        .into_iter()
        .map(PathBuf::from)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("required system tool `{name}` not found"),
            )
        })
}

#[cfg(target_os = "linux")]
fn wrapper_keeper_error(mut child: Child, action: &str) -> std::io::Error {
    use std::io::Read as _;
    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    let _ = child.wait();
    std::io::Error::other(format!("{action} failed: {}", stderr.trim()))
}

#[cfg(not(target_os = "linux"))]
fn create_exec_wrappers(
    _snapshot_root: &Path,
    executables: &[(std::ffi::OsString, fs::File)],
) -> std::io::Result<Option<ExecWrappers>> {
    // The private lease snapshot is the executable handoff on macOS/Windows.
    // Epoch 8 owns stronger hostile-process confinement; refusing every
    // executable here would make the tier-1 package path data-only.
    let _ = executables;
    Ok(None)
}

fn copy_snapshot_node(
    src: &Path,
    dst: &Path,
    hardlinks: &mut BTreeMap<(u64, u64), PathBuf>,
) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(src)?;
    if meta.is_dir() {
        fs::create_dir_all(dst)?;
        let mut entries = fs::read_dir(src)?.collect::<Result<Vec<_>, _>>()?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            copy_snapshot_node(&entry.path(), &dst.join(entry.file_name()), hardlinks)?;
        }
        fs::set_permissions(dst, meta.permissions())?;
    } else if meta.is_file() {
        if let Some(key) = snapshot_file_identity(&meta) {
            if let Some(first) = hardlinks.get(&key) {
                fs::hard_link(first, dst)?;
            } else {
                fs::copy(src, dst)?;
                hardlinks.insert(key, dst.to_path_buf());
            }
        } else {
            fs::copy(src, dst)?;
        }
        fs::set_permissions(dst, meta.permissions())?;
    } else if meta.file_type().is_symlink() {
        let target = fs::read_link(src)?;
        #[cfg(unix)]
        std::os::unix::fs::symlink(target, dst)?;
        #[cfg(not(unix))]
        {
            let _ = target;
            return Err(std::io::Error::other(
                "cache symlink snapshots need platform support",
            ));
        }
    } else {
        return Err(std::io::Error::other("special file in cache snapshot"));
    }
    Ok(())
}

fn remove_snapshot_node(path: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    make_tree_writable_for_removal(path)?;
    if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

#[cfg(unix)]
fn snapshot_file_identity(meta: &fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt as _;
    (meta.nlink() > 1).then(|| (meta.dev(), meta.ino()))
}

#[cfg(not(unix))]
fn snapshot_file_identity(_meta: &fs::Metadata) -> Option<(u64, u64)> {
    None
}

fn open_snapshot_files(
    root: &Path,
    path: &Path,
    files: &mut Vec<(PathBuf, fs::File)>,
) -> std::io::Result<()> {
    let relative = path.strip_prefix(root).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "snapshot path `{}` is outside snapshot root `{}`",
                path.display(),
                root.display()
            ),
        )
    })?;
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Ok(());
    }
    if meta.is_dir() {
        for entry in fs::read_dir(path)? {
            open_snapshot_files(root, &entry?.path(), files)?;
        }
        return Ok(());
    }
    let file = fs::File::open(path)?;
    clear_close_on_exec(&file)?;
    files.push((relative.to_path_buf(), file));
    Ok(())
}

#[cfg(target_os = "linux")]
fn clear_close_on_exec(file: &fs::File) -> std::io::Result<()> {
    use std::os::fd::AsRawFd as _;
    const F_SETFD: i32 = 2;
    // Must match studio_transactions.rs: variadic fcntl (clashing_extern_declarations).
    unsafe extern "C" {
        fn fcntl(fd: i32, command: i32, ...) -> i32;
    }
    if unsafe { fcntl(file.as_raw_fd(), F_SETFD, 0) } == -1 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn clear_close_on_exec(_file: &fs::File) -> std::io::Result<()> {
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

                if cache_bindings.is_empty() {
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
    promote_shared_entry(roots, &entry).map_err(RealizeError::Store)?;
    if realized.source_state == super::Provider::SourceState::Built {
        publish_realized_to_bound_caches(roots, &entry);
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
    promote_shared_entry(roots, &entry).map_err(RealizeError::Store)?;
    publish_realized_to_bound_caches(roots, &entry);
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
    super::RuntimePolicy::with_project_lock(project, "receipt-projection", || {
        record_receipt_projection(
            project,
            &entry.name,
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
    let raw = match fs::read_to_string(&lock_path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
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
    let Some(package) = lock.packages.iter_mut().find(|package| {
        package.name == package_name && receipt_package_matches(package, reference, output_hash)
    }) else {
        return Ok(false);
    };
    if package.receipt.as_deref() == Some(receipt) {
        return Ok(false);
    }
    package.receipt = Some(receipt.to_string());
    crate::Lock::ensure_build_stamp(project_root, &mut lock);
    write_project_lock_atomically(&lock_path, &crate::Lock::write(&lock))?;
    Ok(true)
}

fn receipt_package_matches(
    package: &super::Lock::LockedPackage,
    reference: &str,
    output_hash: &str,
) -> bool {
    let source_matches = match &package.source {
        super::Lock::LockSource::Root => reference == "." || reference == "root",
        super::Lock::LockSource::Path(path) => path == reference,
        super::Lock::LockSource::Git { url, .. } => url == reference,
        super::Lock::LockSource::Nix {
            reference: value, ..
        }
        | super::Lock::LockSource::Cran {
            reference: value, ..
        }
        | super::Lock::LockSource::LuaRocks {
            reference: value, ..
        }
        | super::Lock::LockSource::Registry {
            reference: value, ..
        } => value == reference,
    };
    let output_matches = package
        .envelope
        .as_ref()
        .is_some_and(|envelope| !output_hash.is_empty() && envelope.output_hash == output_hash);
    source_matches || output_matches
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
                // `default` is the typed surface's name for the built-in nixpkgs
                // provider. It is not a named SourceTable entry.
                super::RefSpec::RefSpec {
                    source: super::RefSpec::Source::Nixpkgs,
                    package: dependency.name.clone(),
                    raw,
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
    let id = Lifecycle::RootId::new(format!(
        "profile-generation:{owner}:{profile}:{generation}"
    ))?;
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
    let mut live = current_lock_roots();
    let lifecycle = Lifecycle::protected_targets_unlocked(roots)?;
    let graph = Closure::lifecycle_closure_graph_unlocked(roots)?;
    let mut targets = live.output_hashes.clone();
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

/// D-JPK-GC1=B / U22: collect only unreferenced stale hangar objects, sweep
/// orphan build scratch, then optimize duplicate Jet-owned files. Lockfile
/// reachable entries and unknown legacy records are retained.
pub fn clean_plan(roots: &Roots) -> std::io::Result<CleanReport> {
    let store = roots.hangar_dir();
    match fs::symlink_metadata(&store) {
        Ok(_) => Ingest::require_real_directory(&store, "Hangar root")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CleanReport::default())
        }
        Err(error) => return Err(error),
    }
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || clean_plan_unlocked(roots))
}

fn clean_plan_unlocked(roots: &Roots) -> std::io::Result<CleanReport> {
    let store = roots.hangar_dir();
    let live = live_roots_unlocked(roots)?;
    let mut report = sweep_build_scratch_plan(&store)?;
    let now = now_secs();

    for ent in object_dirs(&store)? {
        let path = ent.path();
        let id = ent.file_name().to_string_lossy().into_owned();
        let Some(meta) = read_meta(&path) else {
            continue;
        };
        if is_live(&id, &meta, &live) || meta.last_used_at.is_none() {
            continue;
        }
        let last_used = meta.last_used_at.unwrap_or(now);
        if now.saturating_sub(last_used) < STALE_AFTER.as_secs() {
            continue;
        }
        report.removed_objects += 1;
        report.removed_bytes += dir_size(&path);
    }

    let opt = optimize_hangar_plan(&store)?;
    report.optimized_files += opt.optimized_files;
    report.optimized_bytes += opt.optimized_bytes;
    let cas = optimize_objects_cas_pool_plan(&store)?;
    report.optimized_files += cas.optimized_files;
    report.optimized_bytes += cas.optimized_bytes;
    Ok(report)
}

pub fn clean(roots: &Roots) -> std::io::Result<CleanReport> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || clean_unlocked(roots))
}

fn clean_unlocked(roots: &Roots) -> std::io::Result<CleanReport> {
    let store = roots.hangar_dir();
    Ingest::ensure_real_directory(&store, "Hangar root")?;
    let live = live_roots_unlocked(roots)?;
    let mut report = sweep_build_scratch(&store)?;
    let now = now_secs();

    for ent in object_dirs(&store)? {
        let path = ent.path();
        let id = ent.file_name().to_string_lossy().into_owned();
        let Some(meta) = read_meta(&path) else {
            continue;
        };
        if is_live(&id, &meta, &live) || meta.last_used_at.is_none() {
            continue;
        }
        let last_used = meta.last_used_at.unwrap_or(now);
        if now.saturating_sub(last_used) < STALE_AFTER.as_secs() {
            continue;
        }
        let bytes = dir_size(&path);
        Closure::tombstone_closure_record_unlocked(roots, &id)?;
        fs::remove_dir_all(&path)?;
        report.removed_objects += 1;
        report.removed_bytes += bytes;
    }

    let opt = optimize_hangar(&store)?;
    report.optimized_files += opt.optimized_files;
    report.optimized_bytes += opt.optimized_bytes;
    Ok(report)
}

pub fn maybe_auto_clean(roots: &Roots) -> std::io::Result<Option<CleanReport>> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        let hangar = roots.hangar_dir();
        Ingest::ensure_real_directory(&hangar, "Hangar root")?;
        let stamp = hangar.join(AUTO_CLEAN_STAMP);
        let now = SystemTime::now();
        if std::env::var_os("JETPACK_AUTO_CLEAN_ALWAYS").is_none() {
            if let Ok(meta) = fs::metadata(&stamp) {
                if let Ok(modified) = meta.modified() {
                    if now.duration_since(modified).unwrap_or_default() < AUTO_CLEAN_AFTER {
                        return Ok(None);
                    }
                }
            }
        }
        let report = clean_unlocked(roots)?;
        let _ = fs::write(stamp, now_secs().to_string());
        Ok(Some(report))
    })
}

fn sweep_build_scratch_plan(hangar: &Path) -> std::io::Result<CleanReport> {
    let root = hangar.join(BUILD_SCRATCH_DIR);
    let mut report = CleanReport::default();
    match fs::symlink_metadata(&root) {
        Ok(_) => Ingest::require_real_directory(&root, "Hangar build scratch")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(report),
        Err(error) => return Err(error),
    }
    let rd = fs::read_dir(&root)?;
    for ent in rd {
        let ent = ent?;
        let path = ent.path();
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Hangar build scratch entry is a symlink: {}", path.display()),
            ));
        }
        if super::Provider::active_tmp_marker_is_live(&path) {
            continue;
        }
        report.swept_tmp += 1;
        report.swept_tmp_bytes += dir_size(&path);
    };
    Ok(report)
}

fn sweep_build_scratch(hangar: &Path) -> std::io::Result<CleanReport> {
    let root = hangar.join(BUILD_SCRATCH_DIR);
    let mut report = CleanReport::default();
    match fs::symlink_metadata(&root) {
        Ok(_) => Ingest::require_real_directory(&root, "Hangar build scratch")?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(report),
        Err(error) => return Err(error),
    };
    for ent in fs::read_dir(&root)? {
        let ent = ent?;
        let path = ent.path();
        if fs::symlink_metadata(&path)?.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Hangar build scratch entry is a symlink: {}", path.display()),
            ));
        }
        if super::Provider::active_tmp_marker_is_live(&path) {
            continue;
        }
        let bytes = dir_size(&path);
        if fs::symlink_metadata(&path)?.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
        report.swept_tmp += 1;
        report.swept_tmp_bytes += bytes;
    }
    Ok(report)
}

fn optimize_hangar_plan(hangar: &Path) -> std::io::Result<CleanReport> {
    let mut seen: BTreeMap<(u64, String), PathBuf> = BTreeMap::new();
    let mut report = CleanReport::default();
    for obj in object_dirs(hangar)? {
        for file in files_under(&obj.path()) {
            if file.file_name().and_then(|n| n.to_str()) == Some("meta.json") {
                continue;
            }
            let Ok(meta) = fs::metadata(&file) else {
                continue;
            };
            if !meta.is_file() || meta.len() == 0 {
                continue;
            }
            let Ok(bytes) = fs::read(&file) else { continue };
            let key = (meta.len(), SHA256::sha256_hex(&bytes));
            if seen.contains_key(&key) {
                report.optimized_files += 1;
                report.optimized_bytes += meta.len();
            } else {
                seen.insert(key, file);
            }
        }
    }
    Ok(report)
}

/// Read-only counterpart to [`optimize_objects_cas_pool`]. Keep the plan
/// honest for Store-v2 objects: `clean` applies the CAS pass even when there
/// are no legacy package directories at the Hangar root.
fn optimize_objects_cas_pool_plan(hangar: &Path) -> std::io::Result<CleanReport> {
    let objects = hangar.join(OBJECTS_DIR);
    let objects_metadata = match fs::symlink_metadata(&objects) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CleanReport::default())
        }
        Err(error) => return Err(error),
    };
    if objects_metadata.file_type().is_symlink() || !objects_metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Hangar object pool is not a real directory: {}", objects.display()),
        ));
    }

    let cas = hangar.join(CAS_DIR);
    match fs::symlink_metadata(&cas) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Hangar CAS pool is not a real directory: {}", cas.display()),
            ))
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }

    let mut object_dirs = Vec::new();
    for ent in fs::read_dir(&objects)? {
        let ent = ent?;
        let path = ent.path();
        let name = ent.file_name().to_string_lossy().into_owned();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Hangar object entry is a symlink: {}", path.display()),
            ));
        }
        if metadata.is_dir() && !name.ends_with(PARTIAL_SUFFIX) {
            object_dirs.push(path);
        }
    }

    let mut report = CleanReport::default();
    for object_dir in object_dirs {
        for file in files_under(&object_dir) {
            let metadata = fs::symlink_metadata(&file)?;
            if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() == 0 {
                continue;
            }
            let bytes = fs::read(&file)?;
            let digest = format!(
                "{}-{:08x}",
                SHA256::sha256_hex(&bytes),
                permission_identity(&metadata)
            );
            let cas_file = cas.join(&digest);
            match fs::symlink_metadata(&cas_file) {
                Ok(existing) if existing.file_type().is_symlink() || !existing.is_file() => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Hangar CAS entry is not a regular file: {}", cas_file.display()),
                    ));
                }
                Ok(existing) => {
                    if existing.len() != metadata.len()
                        || permission_identity(&existing) != permission_identity(&metadata)
                        || fs::read(&cas_file)? != bytes
                    {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Hangar CAS entry is corrupt: {}", cas_file.display()),
                        ));
                    }
                    if !same_file_inode(&file, &cas_file) {
                        report.optimized_files += 1;
                        report.optimized_bytes += metadata.len();
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    report.optimized_files += 1;
                    report.optimized_bytes += metadata.len();
                }
                Err(error) => return Err(error),
            }
        }
    }
    Ok(report)
}

fn optimize_hangar(hangar: &Path) -> std::io::Result<CleanReport> {
    let mut report = optimize_package_tree_hardlinks(hangar)?;
    let cas = optimize_objects_cas_pool(hangar)?;
    report.optimized_files += cas.optimized_files;
    report.optimized_bytes += cas.optimized_bytes;
    Ok(report)
}

/// Legacy package-dir hardlink dedupe (pre-objects/ layout).
fn optimize_package_tree_hardlinks(hangar: &Path) -> std::io::Result<CleanReport> {
    let mut seen: BTreeMap<(u64, String), PathBuf> = BTreeMap::new();
    let mut report = CleanReport::default();
    for obj in object_dirs(hangar)? {
        for file in files_under(&obj.path()) {
            if file.file_name().and_then(|n| n.to_str()) == Some("meta.json") {
                continue;
            }
            let Ok(meta) = fs::metadata(&file) else {
                continue;
            };
            if !meta.is_file() || meta.len() == 0 {
                continue;
            }
            let Ok(bytes) = fs::read(&file) else { continue };
            let key = (meta.len(), SHA256::sha256_hex(&bytes));
            if let Some(first) = seen.get(&key) {
                if hardlink_replace(first, &file).is_ok() {
                    report.optimized_files += 1;
                    report.optimized_bytes += meta.len();
                }
            } else {
                seen.insert(key, file);
            }
        }
    }
    Ok(report)
}

/// Store v2: content-addressed file-byte pool under `hangar/cas/`.
/// Ingest never links into cas (keeps sealed objects at nlink=1 until clean).
/// After optimize, verify uses [`try_output_hash_of_in_hangar`] so cas peers
/// are hangar-internal while outside-hangar hardlinks still reject.
fn optimize_objects_cas_pool(hangar: &Path) -> std::io::Result<CleanReport> {
    let objects = hangar.join(OBJECTS_DIR);
    let cas = hangar.join(CAS_DIR);
    let mut report = CleanReport::default();
    Ingest::ensure_real_directory(hangar, "Hangar root")?;
    Ingest::ensure_real_directory(&objects, "Hangar object pool")?;
    Ingest::ensure_real_directory(&cas, "Hangar CAS pool")?;
    let mut object_dirs = Vec::new();
    for ent in fs::read_dir(&objects)? {
        let ent = ent?;
        let path = ent.path();
        let name = ent.file_name().to_string_lossy().into_owned();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Hangar object entry is a symlink: {}", path.display()),
            ));
        }
        if !metadata.is_dir() || name.ends_with(PARTIAL_SUFFIX) {
            continue;
        }
        object_dirs.push(path);
    }
    for path in object_dirs {
        make_tree_writable_for_removal(&path)?;
        for file in files_under(&path) {
            let Ok(meta) = fs::symlink_metadata(&file) else {
                continue;
            };
            if meta.file_type().is_symlink() || !meta.is_file() || meta.len() == 0 {
                continue;
            }
            let Ok(bytes) = fs::read(&file) else {
                continue;
            };
            let digest = format!(
                "{}-{:08x}",
                SHA256::sha256_hex(&bytes),
                permission_identity(&meta)
            );
            let cas_file = cas.join(&digest);
            match fs::symlink_metadata(&cas_file) {
                Ok(existing) if existing.file_type().is_symlink() || !existing.is_file() => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("Hangar CAS entry is not a regular file: {}", cas_file.display()),
                    ));
                }
                Ok(existing) => {
                    if existing.len() != meta.len()
                        || permission_identity(&existing) != permission_identity(&meta)
                        || fs::read(&cas_file)? != bytes
                    {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!("Hangar CAS entry is corrupt: {}", cas_file.display()),
                        ));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let tmp = cas.join(format!("{digest}.partial"));
                    if let Ok(partial) = fs::symlink_metadata(&tmp) {
                        if partial.file_type().is_symlink() || !partial.is_file() {
                            return Err(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!("Hangar CAS partial is not a regular file: {}", tmp.display()),
                            ));
                        }
                        fs::remove_file(&tmp)?;
                    }
                    let mut partial = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&tmp)?;
                    use std::io::Write as _;
                    partial.write_all(&bytes)?;
                    partial.sync_all()?;
                    fs::set_permissions(&tmp, meta.permissions())?;
                    if let Err(error) = fs::rename(&tmp, &cas_file) {
                        let _ = fs::remove_file(&tmp);
                        return Err(error);
                    }
                }
                Err(error) => return Err(error),
            }
            if same_file_inode(&file, &cas_file) {
                continue;
            }
            if hardlink_replace(&cas_file, &file).is_ok() {
                report.optimized_files += 1;
                report.optimized_bytes += meta.len();
            }
        }
        seal_node(&path)?;
        fsync_tree(&path)?;
    }
    fs::File::open(&objects)?.sync_all()?;
    Ok(report)
}

#[cfg(unix)]
fn permission_identity(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt as _;
    meta.permissions().mode() & 0o7777
}

#[cfg(not(unix))]
fn permission_identity(meta: &fs::Metadata) -> u32 {
    u32::from(meta.permissions().readonly())
}

fn same_file_inode(a: &Path, b: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let Ok(ma) = fs::metadata(a) else {
            return false;
        };
        let Ok(mb) = fs::metadata(b) else {
            return false;
        };
        ma.dev() == mb.dev() && ma.ino() == mb.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = (a, b);
        false
    }
}

/// Run the cas-pool hardlink optimizer (also invoked from `clean`).
pub fn optimize_cas_pool(roots: &Roots) -> std::io::Result<CleanReport> {
    super::RuntimePolicy::with_lock(&roots.root, "hangar", || {
        optimize_objects_cas_pool(&roots.hangar_dir())
    })
}

/// Re-hash a hangar object with cas-peer hardlink law (hangar-internal OK).
pub fn verify_hangar_object(roots: &Roots, entry: &StoreEntry) -> Result<(), IngestError> {
    let _lock = super::RuntimePolicy::acquire_lock(&roots.root, "hangar")
        .map_err(|error| IngestError::IO(error.to_string()))?;
    verify_hangar_object_unlocked(roots, entry)
}

pub(super) fn verify_hangar_object_unlocked(
    roots: &Roots,
    entry: &StoreEntry,
) -> Result<(), IngestError> {
    let hangar = roots.hangar_dir();
    let allow = !entry.platform_artifact_kind.is_empty();
    let graph = Closure::lifecycle_closure_graph_unlocked(roots)
        .map_err(|error| IngestError::Invalid(error.to_string()))?;
    let record = graph.records.get(&entry.id).ok_or_else(|| {
        IngestError::Invalid(format!("closure graph has no record `{}`", entry.id))
    })?;
    if record.primary != entry.envelope.output_hash
        || record.action_key != entry_action_key(entry)
        || record.references != entry.references.iter().cloned().collect()
    {
        return Err(IngestError::Invalid(format!(
            "closure graph disagrees with record `{}`",
            entry.id
        )));
    }
    let mut expected_outputs = entry.named_outputs.clone();
    expected_outputs.insert("out".to_string(), entry.envelope.output_hash.clone());
    if record.outputs != expected_outputs {
        return Err(IngestError::Invalid(format!(
            "closure graph output map disagrees with record `{}`",
            entry.id
        )));
    }
    for (name, expected) in &expected_outputs {
        let object = graph.objects.get(expected).ok_or_else(|| {
            IngestError::Invalid(format!(
                "closure graph output `{name}` is missing `{expected}`"
            ))
        })?;
        let digest = super::Envelope::try_output_hash_of_in_hangar(&object.path, &hangar, allow)
            .map_err(IngestError::Invalid)?;
        if &digest != expected {
            return Err(IngestError::Invalid(format!(
                "output `{name}` records `{expected}`, re-hash produced `{digest}`"
            )));
        }
    }
    if let Some(missing) = record
        .references
        .iter()
        .find(|digest| !graph.objects.contains_key(*digest))
    {
        return Err(IngestError::Invalid(format!(
            "closure record `{}` references missing object `{missing}`",
            entry.id
        )));
    }
    Ok(())
}

fn hardlink_replace(first: &Path, file: &Path) -> std::io::Result<()> {
    if first == file {
        return Ok(());
    }
    let sequence = OPTIMIZE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let tmp = file.with_extension(format!("jet-dedup-{}-{sequence}", std::process::id()));
    #[cfg(unix)]
    {
        fs::hard_link(first, &tmp)?;
        match fs::rename(&tmp, file) {
            Ok(()) => Ok(()),
            Err(error) => {
                let _ = fs::remove_file(&tmp);
                Err(error)
            }
        }
    }
    #[cfg(not(unix))]
    {
        fs::rename(file, &tmp)?;
        match fs::hard_link(first, file) {
            Ok(()) => {
                let _ = fs::remove_file(&tmp);
                Ok(())
            }
            Err(error) => {
                let _ = fs::rename(&tmp, file);
                Err(error)
            }
        }
    }
}

fn object_dirs(hangar: &Path) -> std::io::Result<Vec<fs::DirEntry>> {
    let mut out = Vec::new();
    for ent in fs::read_dir(hangar)? {
        let ent = ent?;
        let path = ent.path();
        let name = ent.file_name().to_string_lossy().into_owned();
        let reserved = name == BUILD_SCRATCH_DIR
            || name == STAGE_DIR
            || name == OBJECTS_DIR
            || name == CAS_DIR
            || name == REFERRERS_DIR
            || name == "receipts"
            || name == "lifecycle-db"
            || name == "closure-db"
            || name == "quarantine"
            || name.starts_with('.');
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Hangar object entry is a symlink: {}", path.display()),
            ));
        }
        if metadata.is_dir() && !reserved {
            out.push(ent);
        }
    }
    out.sort_by_key(|e| e.file_name());
    Ok(out)
}

fn files_under(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(rd) = fs::read_dir(root) {
        for ent in rd.flatten() {
            let p = ent.path();
            let Ok(metadata) = fs::symlink_metadata(&p) else {
                continue;
            };
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                out.extend(files_under(&p));
            } else if metadata.is_file() {
                out.push(p);
            }
        }
    }
    out
}

fn read_meta(dir: &Path) -> Option<ParsedMeta> {
    let text = fs::read_to_string(dir.join("meta.json")).ok()?;
    parse_meta(&text)
}

#[derive(Default)]
struct LiveRoots {
    ids: BTreeSet<String>,
    output_hashes: BTreeSet<String>,
    name_versions: BTreeSet<(String, String)>,
}

fn current_lock_roots() -> LiveRoots {
    let Ok(cwd) = std::env::current_dir() else {
        return LiveRoots::default();
    };
    let Some(lock_path) = nearest_lock_path(&cwd) else {
        return LiveRoots::default();
    };
    let Ok(raw) = fs::read_to_string(lock_path) else {
        return LiveRoots::default();
    };
    let Ok(lock) = crate::Lock::parse(&raw) else {
        return LiveRoots::default();
    };
    let mut roots = LiveRoots::default();
    for pkg in lock.packages {
        roots.name_versions.insert((pkg.name, pkg.version));
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
    roots
}

fn nearest_lock_path(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(current) = dir {
        let lock = lock_path(current);
        if lock.is_file() {
            return Some(lock);
        }
        dir = current.parent();
    }
    None
}

fn is_live(id: &str, meta: &ParsedMeta, roots: &LiveRoots) -> bool {
    roots.ids.contains(id)
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

mod Ingest;
pub use Ingest::*;
mod Closure;
pub use Closure::*;
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
